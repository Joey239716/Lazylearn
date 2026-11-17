//! The whole pipeline as library calls: `make` (material → video), `render_videos`, `revoice`.
//! Shared by the CLI and the desktop app; progress goes through a [`Progress`].

use crate::lesson::Lesson;
use crate::progress::{Progress, Stage};
use crate::{extract, render, script, styles, tts, vision};
use anyhow::{bail, Context, Result};
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
    /// Kind of video (overview / terms / questions / cram)
    pub recipe: script::Recipe,
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
    // keep a title the caller may have set (the app shows the project right away)
    let mut info = LessonInfo {
        title: LessonInfo::load(&dir).map(|i| i.title).unwrap_or_default(),
        duration: 0.0,
        style: o.video.style.unwrap_or_default().name().to_string(),
        inputs: o.inputs.iter().map(|f| f.display().to_string()).collect(),
        focus: o.focus.clone(),
        created: now_secs(),
    };
    info.save(&dir)?;
    on_dir(&dir);

    let material = read_material(&o.inputs, &dir, o.provider == script::Provider::Gemini && !o.no_vision, p)?;
    let opts = script::ScriptOptions {
        scenes: o.scenes,
        focus: o.focus.clone(),
        model: o.model.clone(),
        teaching: o.teaching.clone(),
        cast: o.cast,
        provider: o.provider,
        recipe: o.recipe,
    };
    produce(&material, &dir, &opts, voice, &o.video, &mut info, p)?;
    Ok(dir)
}

pub const VISUALS_HEADER: &str = "=== Diagrams, charts, tables and scanned pages (read from the material's images) ===";

/// Extract the text and (Gemini) read the visuals; saves material.txt / visuals.txt in `dir`.
pub fn read_material(inputs: &[PathBuf], dir: &Path, visual_pass: bool, p: &Progress) -> Result<String> {
    p.start(Stage::Reading, "Reading your material…");
    let mut material = extract::extract_all(inputs, visual_pass, p)?;
    p.done();
    p.check()?;
    if visual_pass {
        p.start(Stage::Vision, "Gemini is reading the diagrams, charts and scans…");
        let notes = vision::read_visuals(inputs, dir, None, p)?;
        if !notes.is_empty() {
            std::fs::write(dir.join("visuals.txt"), &notes)?;
            material.push_str(&format!("{VISUALS_HEADER}\n{notes}\n"));
        }
        p.done();
        p.check()?;
    }
    if material.trim().is_empty() {
        bail!("found nothing to learn from in the input files");
    }
    std::fs::create_dir_all(dir)?;
    std::fs::write(dir.join("material.txt"), &material)?;
    p.info(format!("{} characters of text extracted", material.len()));
    Ok(material)
}

/// Script → voices → quiz → video for one lesson folder. Consumes the voice engine so a local
/// voice server stops before the CPU-heavy render.
pub fn produce(
    material: &str,
    dir: &Path,
    opts: &script::ScriptOptions,
    voice: tts::Voice,
    video: &VideoOptions,
    info: &mut LessonInfo,
    p: &Progress,
) -> Result<()> {
    let mut lesson = write_script(material, dir, opts, video, info, p)?;
    record_voices(&mut lesson, dir, &voice, p)?;
    drop(voice);
    render_videos(&mut lesson, dir, video, p)
}

/// Step 1: the script, quiz and info.json.
pub fn write_script(
    material: &str,
    dir: &Path,
    opts: &script::ScriptOptions,
    video: &VideoOptions,
    info: &mut LessonInfo,
    p: &Progress,
) -> Result<Lesson> {
    let provider_name = match opts.provider {
        script::Provider::Gemini => "Gemini",
        script::Provider::Claude => "Claude",
    };
    p.start(Stage::Script, format!("{provider_name} is writing the script and shot list ({} video)…", opts.recipe.name()));
    let mut lesson = script::write_lesson(material, opts, p)?;
    lesson.style = video.style.unwrap_or_default();
    lesson.save(dir)?;
    write_quiz(&lesson, dir)?;
    info.title = lesson.title.clone();
    info.save(dir)?;
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
    Ok(lesson)
}

/// Step 2: the voices.
pub fn record_voices(lesson: &mut Lesson, dir: &Path, voice: &tts::Voice, p: &Progress) -> Result<()> {
    p.start(Stage::Voices, format!("Recording voices: {}", voice.describe()));
    tts::voice_lesson(lesson, dir, voice, p)?;
    lesson.save(dir)?;
    p.done();
    p.check()
}

// ------------------------------------------------------------------ courses

/// Options for a course: several videos planned from one pile of material.
#[derive(Clone)]
pub struct CourseOptions {
    pub inputs: Vec<PathBuf>,
    /// Course folder (default: a new folder in `lessons_dir`)
    pub out: Option<PathBuf>,
    pub lessons_dir: PathBuf,
    /// How to split it and what the student needs, in plain words
    pub direction: String,
    pub cast: script::CastMode,
    pub model: Option<String>,
    pub provider: script::Provider,
    pub no_vision: bool,
    pub teaching: Option<String>,
    pub voice: tts::VoiceArgs,
    pub video: VideoOptions,
}

/// Read the material and have Gemini plan the videos. Saves `material.txt` and `plan.json`
/// in the course folder and returns it. Review/edit the plan, then call `build_course`.
pub fn plan_course(o: &CourseOptions, p: &Progress, on_dir: impl FnOnce(&Path)) -> Result<(PathBuf, crate::course::Plan)> {
    if o.inputs.is_empty() {
        bail!("no input files");
    }
    for f in &o.inputs {
        if !f.exists() {
            bail!("file not found: {}", f.display());
        }
    }
    let dir = o.out.clone().unwrap_or_else(|| {
        let stem = o.inputs[0].file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or("course".into());
        unique_dir(&o.lessons_dir, &stem)
    });
    std::fs::create_dir_all(&dir)?;
    on_dir(&dir);
    let material = read_material(&o.inputs, &dir, !o.no_vision, p)?;
    p.start(Stage::Script, "Gemini is planning the videos…");
    let body = material.split(VISUALS_HEADER).next().unwrap_or(&material);
    let plan = crate::course::plan(body, &o.direction, None)?;
    plan.save(&dir)?;
    p.info(format!("{} videos planned", plan.videos.len()));
    p.done();
    Ok((dir, plan))
}

/// Make the videos of a planned course (all, or only the 1-based numbers in `only`).
/// Videos that already have a video.mp4 are skipped, so this resumes after a stop.
/// Each video is a normal lesson folder inside the course folder (`01-chapter-1-…`).
///
/// The three slow steps run as an overlapping pipeline: while video N renders (CPU), video N+1
/// records its voices (GPU) and video N+2's script is written (network). A course takes about as
/// long as its voices alone. Progress: `Video i/N: title` when a video starts (its script), and
/// every detail line is tagged `[i/N]`.
pub fn build_course(dir: &Path, o: &CourseOptions, only: &[usize], p: &Progress) -> Result<Vec<PathBuf>> {
    use std::sync::mpsc::sync_channel;

    let plan = crate::course::Plan::load(dir)?;
    let material = std::fs::read_to_string(dir.join("material.txt")).context("the course folder has no material.txt")?;
    render::ffmpeg()?;
    let (body, visuals) = match material.find(VISUALS_HEADER) {
        Some(i) => (&material[..i], &material[i..]),
        None => (material.as_str(), ""),
    };
    let total = plan.videos.len();

    // what to make (cheap, done up front)
    struct Job {
        i: usize,
        title: String,
        dir: PathBuf,
        material: String,
        opts: script::ScriptOptions,
        info: LessonInfo,
    }
    let mut jobs = vec![];
    let mut made = vec![];
    for (i, v) in plan.videos.iter().enumerate() {
        if !only.is_empty() && !only.contains(&(i + 1)) {
            continue;
        }
        let vdir = dir.join(v.folder(i));
        if vdir.join("video.mp4").exists() {
            p.info(format!("Video {}/{total} \"{}\" is already made", i + 1, v.title));
            made.push((i, vdir));
            continue;
        }
        let (part, found) = match crate::course::slice(body, v) {
            Some(s) => (s.to_string(), true),
            None => (body.to_string(), false),
        };
        let mut focus = format!("This video is titled \"{}\" and covers {}. {}", v.title, v.covers, v.focus);
        if v.minutes > 0.0 {
            focus.push_str(&format!(" Aim for about {:.0} minutes of speech.", v.minutes));
        }
        if !found {
            focus.push_str(&format!(" The material below is the whole document: cover ONLY {}.", v.covers));
        }
        if !visuals.is_empty() {
            focus.push_str(" The diagram notes at the end cover the whole document; use only the ones that belong to this part.");
        }
        let info = LessonInfo {
            title: v.title.clone(),
            duration: 0.0,
            style: o.video.style.unwrap_or_default().name().to_string(),
            inputs: o.inputs.iter().map(|f| f.display().to_string()).collect(),
            focus: Some(focus.clone()),
            created: now_secs(),
        };
        let opts = script::ScriptOptions {
            scenes: None,
            focus: Some(focus),
            model: o.model.clone(),
            teaching: o.teaching.clone(),
            cast: o.cast,
            provider: o.provider,
            recipe: v.recipe,
        };
        jobs.push(Job { i, title: v.title.clone(), dir: vdir, material: format!("{part}\n{visuals}"), opts, info });
    }
    if jobs.is_empty() {
        made.sort();
        return Ok(made.into_iter().map(|(_, d)| d).collect());
    }

    // script -> voices -> render, one bounded handoff between each
    let (to_voices, voices_in) = sync_channel::<(usize, PathBuf, Lesson)>(1);
    let (to_render, render_in) = sync_channel::<(usize, PathBuf, Lesson)>(1);
    let tag = |i: usize| p.tagged(format!("{}/{total}", i + 1));
    let results = std::thread::scope(|s| {
        let scripts = s.spawn(move || -> Result<()> {
            for mut j in jobs {
                p.check()?;
                p.info(format!("Video {}/{total}: {}", j.i + 1, j.title));
                std::fs::create_dir_all(&j.dir)?;
                std::fs::write(j.dir.join("material.txt"), &j.material)?;
                j.info.save(&j.dir)?;
                let lesson = write_script(&j.material, &j.dir, &j.opts, &o.video, &mut j.info, &tag(j.i))?;
                if to_voices.send((j.i, j.dir, lesson)).is_err() {
                    break; // a later step failed; its error is reported
                }
            }
            Ok(())
        });
        let voices = s.spawn(move || -> Result<()> {
            let mut voice: Option<tts::Voice> = None;
            for (i, vdir, mut lesson) in voices_in {
                let v = match &voice {
                    Some(v) => v,
                    None => voice.insert(tts::Voice::new(&o.voice)?),
                };
                record_voices(&mut lesson, &vdir, v, &tag(i))?;
                if to_render.send((i, vdir, lesson)).is_err() {
                    break;
                }
            }
            Ok(())
        });
        let renders = s.spawn(move || -> Result<Vec<(usize, PathBuf)>> {
            let mut done = vec![];
            for (i, vdir, mut lesson) in render_in {
                render_videos(&mut lesson, &vdir, &o.video, &tag(i))?;
                tag(i).info("✅ done");
                done.push((i, vdir));
            }
            Ok(done)
        });
        let join = |h: std::thread::ScopedJoinHandle<'_, Result<()>>| h.join().unwrap_or_else(|_| Err(anyhow::anyhow!("a worker thread panicked")));
        let r_render = renders.join().unwrap_or_else(|_| Err(anyhow::anyhow!("the render thread panicked")));
        let r_voices = join(voices);
        let r_scripts = join(scripts);
        (r_scripts, r_voices, r_render)
    });
    let (r_scripts, r_voices, r_render) = results;
    // report the earliest failure in the chain (a later step stopping is just a consequence)
    let rendered = r_render?;
    r_voices?;
    r_scripts?;
    made.extend(rendered);
    made.sort();
    Ok(made.into_iter().map(|(_, d)| d).collect())
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
