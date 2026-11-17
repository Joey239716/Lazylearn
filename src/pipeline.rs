//! The whole pipeline as library calls: `make` (material → video), `render_videos`, `revoice`.
//! Shared by the CLI and the desktop app; progress goes through a [`Progress`].

use crate::lesson::Lesson;
use crate::progress::{Progress, Stage};
use crate::{extract, render, script, styles, tts, vision};
use anyhow::{bail, Result};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct VideoOptions {
    pub style: Option<styles::Style>,
    /// also render a 9:16 version
    pub vertical: bool,
    /// only render the 9:16 version
    pub vertical_only: bool,
    pub music: Option<PathBuf>,
    pub music_db: f32,
    pub no_music: bool,
    pub no_sfx: bool,
    pub fps: u32,
    pub until: Option<f32>,
    pub workers: Option<usize>,
}

impl Default for VideoOptions {
    fn default() -> Self {
        VideoOptions {
            style: None,
            vertical: false,
            vertical_only: false,
            music: None,
            music_db: -16.0,
            no_music: false,
            no_sfx: false,
            fps: 30,
            until: None,
            workers: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct MakeOptions {
    pub inputs: Vec<PathBuf>,
    /// lesson folder (default: `<lessons_dir>/<first file name>`, made unique)
    pub out: Option<PathBuf>,
    pub lessons_dir: PathBuf,
    pub cast: script::CastMode,
    pub scenes: Option<u32>,
    pub focus: Option<String>,
    pub model: Option<String>,
    pub provider: script::Provider,
    pub no_vision: bool,
    /// teaching-style instructions replacing the default
    pub teaching: Option<String>,
    pub voice: tts::VoiceArgs,
    pub video: VideoOptions,
}

/// Small summary saved next to the video for the app's library (`info.json`).
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct LessonInfo {
    #[serde(default)]
    pub title: String,
    /// seconds
    #[serde(default)]
    pub duration: f32,
    #[serde(default)]
    pub style: String,
    #[serde(default)]
    pub inputs: Vec<String>,
    #[serde(default)]
    pub focus: Option<String>,
    /// unix seconds
    #[serde(default)]
    pub created: u64,
}

impl LessonInfo {
    pub fn load(dir: &Path) -> Option<LessonInfo> {
        serde_json::from_str(&std::fs::read_to_string(dir.join("info.json")).ok()?).ok()
    }
    pub fn save(&self, dir: &Path) -> Result<()> {
        std::fs::write(dir.join("info.json"), serde_json::to_string_pretty(self)?)?;
        Ok(())
    }
}

pub fn now_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// A folder name that doesn't exist yet: `base`, `base-2`, `base-3`, …
pub fn unique_dir(parent: &Path, stem: &str) -> PathBuf {
    let clean: String = stem
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '-' })
        .collect::<String>()
        .trim_matches('-')
        .to_lowercase();
    let clean = if clean.is_empty() { "lesson".to_string() } else { clean };
    let mut p = parent.join(&clean);
    let mut n = 2;
    while p.exists() {
        p = parent.join(format!("{clean}-{n}"));
        n += 1;
    }
    p
}

/// Material → script → voices → video. Returns the lesson folder.
/// `on_dir` is called as soon as the folder exists (so a UI can show the project early).
pub fn make(o: &MakeOptions, p: &Progress, on_dir: impl FnOnce(&Path)) -> Result<PathBuf> {
    for f in &o.inputs {
        if !f.exists() {
            bail!("file not found: {}", f.display());
        }
    }
    if o.inputs.is_empty() {
        bail!("no input files");
    }
    render::ffmpeg()?; // fail fast, before the slow parts
    let voice = tts::Voice::new(&o.voice)?;
    let dir = o.out.clone().unwrap_or_else(|| {
        let stem = o.inputs[0].file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or("lesson".into());
        unique_dir(&o.lessons_dir, &stem)
    });
    std::fs::create_dir_all(&dir)?;
    let mut info = LessonInfo {
        title: String::new(),
        duration: 0.0,
        style: o.video.style.unwrap_or_default().name().to_string(),
        inputs: o.inputs.iter().map(|f| f.display().to_string()).collect(),
        focus: o.focus.clone(),
        created: now_secs(),
    };
    info.save(&dir)?;
    on_dir(&dir);

    // ---- read
    p.start(Stage::Reading, "Reading your material…");
    let visual_pass = o.provider == script::Provider::Gemini && !o.no_vision;
    let mut material = extract::extract_all(&o.inputs, visual_pass, p)?;
    p.done();
    p.check()?;
    if visual_pass {
        p.start(Stage::Vision, "Gemini is reading the diagrams, charts and scans…");
        let notes = vision::read_visuals(&o.inputs, &dir, None, p)?;
        if !notes.is_empty() {
            std::fs::write(dir.join("visuals.txt"), &notes)?;
            material.push_str(&format!(
                "=== Diagrams, charts, tables and scanned pages (read from the material's images) ===\n{notes}\n"
            ));
        }
        p.done();
        p.check()?;
    }
    if material.trim().is_empty() {
        bail!("found nothing to learn from in the input files");
    }
    std::fs::write(dir.join("material.txt"), &material)?;
    p.info(format!("{} characters of text extracted", material.len()));

    // ---- script
    let provider_name = match o.provider {
        script::Provider::Gemini => "Gemini",
        script::Provider::Claude => "Claude",
    };
    p.start(Stage::Script, format!("{provider_name} is writing the script and shot list…"));
    let opts = script::ScriptOptions {
        scenes: o.scenes,
        focus: o.focus.clone(),
        model: o.model.clone(),
        teaching: o.teaching.clone(),
        cast: o.cast,
        provider: o.provider,
    };
    let mut lesson = script::write_lesson(&material, &opts, p)?;
    lesson.style = o.video.style.unwrap_or_default();
    lesson.save(&dir)?;
    write_quiz(&lesson, &dir)?;
    info.title = lesson.title.clone();
    info.save(&dir)?;
    let shots: usize = lesson.scenes.iter().flat_map(|s| &s.lines).map(|l| l.shots.len()).sum();
    p.info(format!(
        "\"{}\" — {} scenes, {} lines, {} shots, {} quiz questions",
        lesson.title,
        lesson.scenes.len(),
        lesson.line_count(),
        shots,
        lesson.quizzes().len()
    ));
    p.done();
    p.check()?;

    // ---- voices
    p.start(Stage::Voices, format!("Recording voices: {}", voice.describe()));
    tts::voice_lesson(&mut lesson, &dir, &voice, p)?;
    drop(voice); // stops a local voice server before the CPU-heavy render
    lesson.save(&dir)?;
    p.done();
    p.check()?;

    // ---- video
    render_videos(&mut lesson, &dir, &o.video, p)?;
    Ok(dir)
}

/// Re-generate the voices of a lesson, then re-render it.
pub fn revoice(dir: &Path, voice: &tts::VoiceArgs, video: &VideoOptions, p: &Progress) -> Result<()> {
    let mut lesson = Lesson::load(dir)?;
    let v = tts::Voice::new(voice)?;
    p.start(Stage::Voices, format!("Recording voices: {}", v.describe()));
    tts::voice_lesson(&mut lesson, dir, &v, p)?;
    drop(v);
    lesson.save(dir)?;
    p.done();
    p.check()?;
    render_videos(&mut lesson, dir, video, p)
}

/// Render `video.mp4` (and/or `video-vertical.mp4`), then the thumbnail and `info.json`.
pub fn render_videos(lesson: &mut Lesson, dir: &Path, v: &VideoOptions, p: &Progress) -> Result<()> {
    if let Some(s) = v.style {
        lesson.style = s;
    }
    let mut sizes = vec![];
    if !v.vertical_only {
        sizes.push((1920u32, 1080u32, "video.mp4"));
    }
    if v.vertical || v.vertical_only {
        sizes.push((1080, 1920, "video-vertical.mp4"));
    }
    for (w, h, name) in sizes {
        p.start(Stage::Render, format!("Rendering {name} ({w}×{h}, {} style)…", lesson.style.name()));
        let opts = render::RenderOptions {
            width: w,
            height: h,
            fps: v.fps,
            until: v.until,
            music: v.music.clone(),
            no_music: v.no_music,
            no_sfx: v.no_sfx,
            music_db: v.music_db,
            workers: v.workers,
            out: dir.join(name),
        };
        let out = render::render(lesson, dir, &opts, p)?;
        p.info(format!("✅ {}", out.display()));
        p.done();
    }
    lesson.save(dir)?;
    // a still for the library, and the summary
    let _ = save_thumb(lesson, dir);
    let mut info = LessonInfo::load(dir).unwrap_or_default();
    info.title = lesson.title.clone();
    info.style = lesson.style.name().to_string();
    info.duration = crate::timeline::Timeline::build(lesson, dir, 1920, 1080).total;
    if info.created == 0 {
        info.created = now_secs();
    }
    info.save(dir)?;
    Ok(())
}

/// `thumb.png` (480×270): a full-size frame from the first picture shot, box-downscaled.
pub fn save_thumb(lesson: &Lesson, dir: &Path) -> Result<PathBuf> {
    use crate::timeline::{Renderer, Timeline, Vis};
    let tl = Timeline::build(lesson, dir, 1920, 1080);
    let t = tl
        .segs
        .iter()
        .filter(|s| matches!(&s.vis, Vis::Shot(sh) if sh.kind != crate::lesson::ShotKind::Talk))
        .find(|s| s.end - s.start > 2.0)
        .map(|s| s.start + (s.end - s.start).min(4.0) * 0.8)
        .unwrap_or(tl.total / 3.0);
    let mut r = Renderer::new(&tl);
    r.frame(t);
    let src = r.g.bytes();
    let (sw, k) = (1920usize, 4usize);
    let (w, h) = (sw / k, 1080 / k);
    let mut out = vec![0u8; w * h * 4];
    for y in 0..h {
        for x in 0..w {
            let mut acc = [0u32; 4];
            for dy in 0..k {
                let row = (y * k + dy) * sw * 4;
                for dx in 0..k {
                    let i = row + (x * k + dx) * 4;
                    for c in 0..4 {
                        acc[c] += src[i + c] as u32;
                    }
                }
            }
            for c in 0..4 {
                out[(y * w + x) * 4 + c] = (acc[c] / (k * k) as u32) as u8;
            }
        }
    }
    let path = dir.join("thumb.png");
    let f = std::io::BufWriter::new(std::fs::File::create(&path)?);
    let mut enc = png::Encoder::new(f, w as u32, h as u32);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header()?.write_image_data(&out)?;
    Ok(path)
}

/// The quiz the video ends with (`quiz.json`), taken in the app after the video.
pub fn write_quiz(lesson: &Lesson, dir: &Path) -> Result<()> {
    let qs = lesson.quizzes();
    std::fs::write(
        dir.join("quiz.json"),
        serde_json::to_string_pretty(&serde_json::json!({ "title": lesson.title, "questions": qs }))?,
    )?;
    Ok(())
}
