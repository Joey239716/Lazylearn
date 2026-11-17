//! Turn a voiced lesson into an MP4: mix the soundtrack, render frames in
//! parallel chunks (each piped into its own ffmpeg), then join everything.

use crate::audio;
use crate::lesson::Lesson;
use crate::timeline::{Renderer, Timeline};
use anyhow::{bail, Context, Result};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicUsize, Ordering};

pub struct RenderOptions {
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    /// Only render the first N seconds (quick tests)
    pub until: Option<f32>,
    pub music: Option<PathBuf>,
    pub no_music: bool,
    pub no_sfx: bool,
    pub music_db: f32,
    pub workers: Option<usize>,
    pub out: PathBuf,
}

/// The ffmpeg binary: $STICKSTUDY_FFMPEG, else `ffmpeg` on PATH.
pub fn ffmpeg() -> Result<String> {
    let bin = std::env::var("STICKSTUDY_FFMPEG").unwrap_or_else(|_| "ffmpeg".into());
    match crate::command(&bin).arg("-version").stdout(Stdio::null()).stderr(Stdio::null()).status() {
        Ok(s) if s.success() => Ok(bin),
        _ => bail!(
            "ffmpeg not found. Install it (Windows: `winget install Gyan.FFmpeg`, macOS: download from \
             https://evermeet.cx/ffmpeg/ or `brew install ffmpeg`) or set STICKSTUDY_FFMPEG to its path"
        ),
    }
}

pub fn render(lesson: &Lesson, dir: &Path, o: &RenderOptions, p: &crate::progress::Progress) -> Result<PathBuf> {
    let ff = ffmpeg()?;
    let tl = Timeline::build(lesson, dir, o.width, o.height);
    let end = o.until.map(|u| u.min(tl.total)).unwrap_or(tl.total);
    let work = dir.join(".render");
    std::fs::create_dir_all(&work)?;
    if let Some(parent) = o.out.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let shots = tl.segs.iter().filter(|s| matches!(s.vis, crate::timeline::Vis::Shot(_))).count();
    p.info(format!(
        "{} shots, {} sound effects, {}:{:04.1} long{}",
        shots,
        tl.sfx.len(),
        (tl.total / 60.0) as u32,
        tl.total % 60.0,
        if o.until.is_some() { format!(" (rendering the first {end:.0}s)") } else { String::new() }
    ));

    // ---- soundtrack
    p.frac("mixing voice, sound effects and music…", 0.0);
    let mix = audio::mix(
        &tl,
        &audio::MixOptions {
            ffmpeg: &ff,
            music: o.music.as_deref(),
            no_music: o.no_music,
            no_sfx: o.no_sfx,
            music_db: o.music_db,
            until: end,
        },
    )?;
    let wav = work.join("mix.wav");
    audio::write_wav(&wav, &mix)?;

    // ---- frames, in parallel chunks
    let nframes = (end * o.fps as f32).ceil() as usize;
    let cpus = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4);
    let workers = o.workers.unwrap_or((cpus / 2).max(1)).clamp(1, 16).min((nframes / (o.fps as usize * 3)).max(1));
    let cuts: Vec<usize> = (0..=workers).map(|i| nframes * i / workers).collect();
    let parts: Vec<PathBuf> = (0..workers).map(|i| work.join(format!("part_{i:02}.mp4"))).collect();
    let done = AtomicUsize::new(0);
    let started = std::time::Instant::now();
    p.info(format!("rendering {nframes} frames on {workers} threads"));
    let cancel = std::sync::atomic::AtomicBool::new(false);
    let results: Vec<Result<()>> = std::thread::scope(|s| {
        let handles: Vec<_> = (0..workers)
            .map(|i| {
                let (tl, ff, part, done) = (&tl, &ff, &parts[i], &done);
                let (f0, f1) = (cuts[i], cuts[i + 1]);
                let cancel = &cancel;
                s.spawn(move || render_chunk(tl, ff, f0, f1, o, part, done, cancel))
            })
            .collect();
        // progress
        loop {
            let d = done.load(Ordering::Relaxed);
            let el = started.elapsed().as_secs_f32();
            let eta = if d > 0 { el / d as f32 * (nframes - d) as f32 } else { 0.0 };
            let f = d as f32 / nframes.max(1) as f32;
            p.frac(format!("{d}/{nframes} frames  ({:.0}%)  ~{:.0}s left", 100.0 * f, eta), f);
            if p.cancelled() {
                cancel.store(true, Ordering::Relaxed);
            }
            if handles.iter().all(|h| h.is_finished()) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(250));
        }
        handles.into_iter().map(|h| h.join().unwrap_or_else(|_| Err(anyhow::anyhow!("render thread panicked")))).collect()
    });
    p.check()?;
    for r in results {
        r?;
    }

    // ---- join + soundtrack
    p.frac(format!("joining ({:.0}s of rendering)…", started.elapsed().as_secs_f32()), 1.0);
    let list = work.join("parts.txt");
    let abs = |p: &PathBuf| std::path::absolute(p).unwrap_or_else(|_| p.clone()).display().to_string().replace('\\', "/");
    std::fs::write(&list, parts.iter().map(|p| format!("file '{}'\n", abs(p))).collect::<String>())?;
    let st = crate::command(&ff)
        .args(["-y", "-v", "error", "-f", "concat", "-safe", "0", "-i"])
        .arg(&list)
        .arg("-i")
        .arg(&wav)
        .args(["-map", "0:v", "-map", "1:a", "-c:v", "copy", "-ar", "48000", "-c:a", "aac", "-b:a", "192k", "-shortest", "-movflags", "+faststart"])
        .arg(&o.out)
        .status()
        .context("running ffmpeg to join the video")?;
    if !st.success() {
        bail!("ffmpeg failed while joining the video");
    }
    write_srt(&tl, end, &o.out.with_extension("srt"))?;
    let _ = std::fs::remove_dir_all(&work);
    Ok(o.out.clone())
}

#[allow(clippy::too_many_arguments)]
fn render_chunk(tl: &Timeline, ff: &str, f0: usize, f1: usize, o: &RenderOptions, part: &Path, done: &AtomicUsize, cancel: &std::sync::atomic::AtomicBool) -> Result<()> {
    let mut child = crate::command(ff)
        .args(["-y", "-v", "error", "-f", "rawvideo", "-pix_fmt", "rgba", "-s", &format!("{}x{}", o.width, o.height), "-r", &o.fps.to_string(), "-i", "-"])
        .args(["-c:v", "libx264", "-preset", "fast", "-crf", "19", "-pix_fmt", "yuv420p", "-threads", "2", "-an"])
        .arg(part)
        .stdin(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("starting ffmpeg")?;
    let mut stdin = child.stdin.take().unwrap();
    let mut r = Renderer::new(tl);
    for f in f0..f1 {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        r.frame(f as f32 / o.fps as f32);
        if stdin.write_all(r.g.bytes()).is_err() {
            break;
        }
        done.fetch_add(1, Ordering::Relaxed);
    }
    drop(stdin);
    let out = child.wait_with_output()?;
    if !out.status.success() {
        bail!("ffmpeg (chunk) failed: {}", String::from_utf8_lossy(&out.stderr));
    }
    Ok(())
}

fn write_srt(tl: &Timeline, end: f32, path: &Path) -> Result<()> {
    let ts = |x: f32| {
        let ms = (x * 1000.0).round() as u64;
        format!("{:02}:{:02}:{:02},{:03}", ms / 3_600_000, ms / 60_000 % 60, ms / 1000 % 60, ms % 1000)
    };
    let mut s = String::new();
    for (i, c) in tl.captions.iter().filter(|c| c.t0 < end).enumerate() {
        let text: Vec<&str> = c.words.iter().map(|w| w.text.as_str()).collect();
        s.push_str(&format!("{}\n{} --> {}\n{}\n\n", i + 1, ts(c.t0), ts(c.t1.min(end)), text.join(" ")));
    }
    std::fs::write(path, s)?;
    Ok(())
}

/// Save still frames at the given times (for checking the look without rendering a video).
pub fn snapshots(lesson: &Lesson, dir: &Path, w: u32, h: u32, times: &[f32], out_dir: &Path) -> Result<Vec<PathBuf>> {
    let tl = Timeline::build(lesson, dir, w, h);
    std::fs::create_dir_all(out_dir)?;
    let mut r = Renderer::new(&tl);
    let mut out = vec![];
    for &t in times {
        let t = if t < 0.0 { tl.total + t } else { t };
        r.frame(t);
        let p = out_dir.join(format!("frame_{t:07.2}.png"));
        r.g.save_png(&p)?;
        out.push(p);
    }
    Ok(out)
}

/// Shot start times (handy for snapshotting every shot).
pub fn shot_times(lesson: &Lesson, dir: &Path, w: u32, h: u32) -> Vec<(f32, f32, String)> {
    let tl = Timeline::build(lesson, dir, w, h);
    tl.segs
        .iter()
        .map(|s| {
            let name = match &s.vis {
                crate::timeline::Vis::Intro => "intro".to_string(),
                crate::timeline::Vis::Outro => "outro".to_string(),
                crate::timeline::Vis::Chapter { n, .. } => format!("chapter{n}"),
                crate::timeline::Vis::Shot(sh) => format!("{:?}", sh.kind).to_lowercase(),
            };
            (s.start, s.end, name)
        })
        .collect()
}
