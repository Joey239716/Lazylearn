mod audio;
mod character;
mod extract;
mod gemini;
mod gfx;
mod icons;
mod lesson;
mod omnivoice;
mod render;
mod script;
mod shots;
mod styles;
mod timeline;
mod tts;

use anyhow::{bail, Context, Result};
use clap::{Args, Parser, Subcommand};
use lesson::Lesson;
use std::path::{Path, PathBuf};

#[derive(Parser)]
#[command(
    name = "stickstudy",
    version,
    about = "Turn class material (PDF, PowerPoint, transcripts) into fun, animated, voiced study videos (MP4)"
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Args, Debug, Clone)]
struct VideoArgs {
    /// Visual style: doodle, cosmic, whiteboard or chalk (`stickstudy styles` to compare)
    #[arg(short, long, value_enum)]
    style: Option<styles::Style>,
    /// Also render a 9:16 vertical version (Shorts / TikTok / Reels)
    #[arg(long)]
    vertical: bool,
    /// Only render the vertical version
    #[arg(long, conflicts_with = "vertical")]
    vertical_only: bool,
    /// Background music file (mp3, wav, …). Default: a generated lo-fi loop
    #[arg(long)]
    music: Option<PathBuf>,
    /// Music level under the voice in dB (more negative = quieter)
    #[arg(long, default_value_t = -16.0, allow_negative_numbers = true)]
    music_db: f32,
    /// No background music
    #[arg(long)]
    no_music: bool,
    /// No sound effects
    #[arg(long)]
    no_sfx: bool,
    /// Frames per second
    #[arg(long, default_value_t = 30)]
    fps: u32,
    /// Only render the first N seconds (quick preview)
    #[arg(long)]
    until: Option<f32>,
    /// Render threads (default: half your CPU threads)
    #[arg(long)]
    workers: Option<usize>,
}

#[derive(Subcommand)]
enum Cmd {
    /// Create a study video from class material
    Make {
        /// PDF, .pptx, .vtt/.srt transcript, .txt or .md files (several are combined into one lesson)
        #[arg(required = true)]
        inputs: Vec<PathBuf>,
        /// Output lesson folder (default: lessons/<first file name>)
        #[arg(short, long)]
        out: Option<PathBuf>,
        /// duo = teacher + student banter, solo = one narrator
        #[arg(long, value_enum, default_value_t = script::CastMode::Duo)]
        cast: script::CastMode,
        /// Force a number of scenes (default: scales with the material)
        #[arg(long)]
        scenes: Option<u32>,
        /// Topic or angle to focus on, e.g. "the midterm topics" or "explain the proofs"
        #[arg(long)]
        focus: Option<String>,
        /// Model alias or name for script writing (e.g. gemini-2.5-flash or sonnet)
        #[arg(long)]
        model: Option<String>,
        /// Provider for script writing: gemini (default, via Antigravity or GEMINI_API_KEY) or claude
        #[arg(long, value_enum, default_value_t = script::Provider::Gemini)]
        provider: script::Provider,
        /// Replace the teaching-style instructions with the contents of this file
        /// (see `stickstudy prompt` for the default)
        #[arg(long)]
        teaching: Option<PathBuf>,
        #[command(flatten)]
        voice: tts::VoiceArgs,
        #[command(flatten)]
        video: VideoArgs,
    },
    /// Re-render the video of an existing lesson (e.g. in a different style)
    Render {
        dir: PathBuf,
        #[command(flatten)]
        video: VideoArgs,
    },
    /// Re-generate the voices of an existing lesson, then re-render it
    Revoice {
        dir: PathBuf,
        #[command(flatten)]
        voice: tts::VoiceArgs,
        #[command(flatten)]
        video: VideoArgs,
    },
    /// Save still frames of a lesson as PNGs (no video), e.g. to check a style
    Frames {
        dir: PathBuf,
        /// Specific times in seconds to snapshot (default: 3/4 through each shot)
        #[arg(long)]
        at: Vec<f32>,
        /// Style to use for the snapshots (default: whatever style the lesson was made in)
        #[arg(long, value_enum)]
        style: Option<styles::Style>,
        #[arg(long)]
        vertical: bool,
        #[arg(short, long)]
        out: Option<PathBuf>,
    },
    /// List the voice engines and the installed voices on your computer
    Voices,
    /// List the visual styles
    Styles,
    /// Print the teaching-style prompt Claude and Gemini follow, so you can edit it
    Prompt,
}

fn main() {
    if let Err(e) = run() {
        eprintln!("\n❌ {e:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    match Cli::parse().cmd {
        Cmd::Make { inputs, out, cast, scenes, focus, model, provider, teaching, voice, video } => {
            for p in &inputs {
                if !p.exists() {
                    bail!("file not found: {}", p.display());
                }
            }
            render::ffmpeg()?; // fail fast, before the slow parts
            let voice_engine = tts::Voice::new(&voice)?;
            let teaching = match teaching {
                Some(p) => Some(std::fs::read_to_string(&p).with_context(|| format!("reading {}", p.display()))?),
                None => None,
            };
            let dir = out.unwrap_or_else(|| {
                let stem = inputs[0].file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or("lesson".into());
                PathBuf::from("lessons").join(stem)
            });

            eprintln!("📚 Reading your material…");
            let material = extract::extract_all(&inputs)?;
            std::fs::create_dir_all(&dir)?;
            std::fs::write(dir.join("material.txt"), &material)?;
            eprintln!("  {} characters of text extracted", material.len());

            let provider_name = match provider {
                script::Provider::Gemini => "Gemini",
                script::Provider::Claude => "Claude",
            };
            eprintln!("✍️  {provider_name} is writing the script and shot list…");
            let opts = script::ScriptOptions { scenes, focus, model, teaching, cast, provider };
            let mut lesson = script::write_lesson(&material, &opts)?;
            lesson.style = video.style.unwrap_or_default();
            lesson.save(&dir)?;
            let shots: usize = lesson.scenes.iter().flat_map(|s| &s.lines).map(|l| l.shots.len()).sum();
            eprintln!(
                "  \"{}\" — {} scenes, {} lines, {} shots, {} quiz questions",
                lesson.title,
                lesson.scenes.len(),
                lesson.line_count(),
                shots,
                lesson.quizzes().len()
            );

            eprintln!("🗣  Recording voices: {}", voice_engine.describe());
            tts::voice_lesson(&mut lesson, &dir, &voice_engine)?;
            lesson.save(&dir)?;
            write_quiz(&lesson, &dir)?;
            render_videos(&mut lesson, &dir, &video)?;
        }
        Cmd::Render { dir, video } => {
            let mut lesson = Lesson::load(&dir)?;
            render_videos(&mut lesson, &dir, &video)?;
        }
        Cmd::Revoice { dir, voice, video } => {
            let mut lesson = Lesson::load(&dir)?;
            let v = tts::Voice::new(&voice)?;
            eprintln!("🗣  Recording voices: {}", v.describe());
            tts::voice_lesson(&mut lesson, &dir, &v)?;
            lesson.save(&dir)?;
            render_videos(&mut lesson, &dir, &video)?;
        }
        Cmd::Frames { dir, at, style, vertical, out } => {
            let mut lesson = Lesson::load(&dir)?;
            if let Some(s) = style {
                lesson.style = s;
            }
            let (w, h) = if vertical { (1080, 1920) } else { (1920, 1080) };
            let times: Vec<f32> = if at.is_empty() {
                render::shot_times(&lesson, &dir, w, h).iter().map(|(a, b, _)| a + (b - a).min(4.0) * 0.75).collect()
            } else {
                at
            };
            let out = out.unwrap_or_else(|| dir.join("frames"));
            let files = render::snapshots(&lesson, &dir, w, h, &times, &out)?;
            eprintln!("✅ {} frames saved to {}", files.len(), out.display());
        }
        Cmd::Voices => tts::print_voice_guide(),
        Cmd::Styles => {
            println!("Visual styles (pick with --style <name>; re-render any lesson in another style with `stickstudy render <lesson> --style <name>`):\n");
            for s in styles::ALL {
                println!("  {:<11} {}", s.name(), s.tagline());
            }
        }
        Cmd::Prompt => {
            println!("# Teaching style (replace with --teaching <file>)\n");
            println!("{}", script::DEFAULT_STYLE);
        }
    }
    Ok(())
}

fn render_videos(lesson: &mut Lesson, dir: &Path, v: &VideoArgs) -> Result<()> {
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
        eprintln!("🎬 Rendering {name} ({w}×{h}, {} style)…", lesson.style.name());
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
        let out = render::render(lesson, dir, &opts)?;
        eprintln!("✅ {}", out.display());
    }
    Ok(())
}

/// The quiz the video ends with, for the quiz window.
fn write_quiz(lesson: &Lesson, dir: &Path) -> Result<()> {
    let qs = lesson.quizzes();
    std::fs::write(dir.join("quiz.json"), serde_json::to_string_pretty(&serde_json::json!({ "title": lesson.title, "questions": qs }))?)?;
    Ok(())
}
