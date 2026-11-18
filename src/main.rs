use stickstudy::progress::Progress;
use stickstudy::{lesson, pipeline, render, script, styles, tts};

use anyhow::{Context, Result};
use clap::{Args, Parser, Subcommand};
use lesson::Lesson;
use std::path::PathBuf;

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
    /// Background music: a file, or a folder to pick a random track from. Default: the folder in
    /// STICKSTUDY_MUSIC_DIR if set, otherwise a generated lo-fi loop
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
        /// Skip Gemini's reading pass over diagrams, charts, slide images and scanned pages
        #[arg(long)]
        no_vision: bool,
        /// Kind of video: overview, terms (definition drill), questions (how to answer them), cram
        #[arg(long, value_enum, default_value_t = script::Recipe::Overview)]
        recipe: script::Recipe,
        /// Replace the teaching-style instructions with the contents of this file
        /// (see `stickstudy prompt` for the default)
        #[arg(long)]
        teaching: Option<PathBuf>,
        #[command(flatten)]
        voice: tts::VoiceArgs,
        #[command(flatten)]
        video: VideoArgs,
    },
    /// Split material into several videos following a direction (e.g. one per chapter), then make them.
    /// Run it again on the course folder to resume, or after editing its plan.json.
    Course {
        /// Material files, or an existing course folder (with plan.json) to resume
        #[arg(required = true)]
        inputs: Vec<PathBuf>,
        /// Course folder (default: lessons/<first file name>)
        #[arg(short, long)]
        out: Option<PathBuf>,
        /// How to split it and what you need, in plain words, e.g.
        /// "split by chapter; I only need the term definitions and how to answer the end-of-chapter questions"
        #[arg(short, long, default_value = "")]
        direction: String,
        /// Don't ask before making the videos
        #[arg(short, long)]
        yes: bool,
        /// Only make the plan (edit plan.json, then run `stickstudy course <folder>`)
        #[arg(long)]
        plan_only: bool,
        /// Only make these videos (numbers from the plan), e.g. --only 1 3
        #[arg(long, num_args = 1..)]
        only: Vec<usize>,
        #[arg(long, value_enum, default_value_t = script::CastMode::Duo)]
        cast: script::CastMode,
        #[arg(long)]
        model: Option<String>,
        #[arg(long, value_enum, default_value_t = script::Provider::Gemini)]
        provider: script::Provider,
        #[arg(long)]
        no_vision: bool,
        #[arg(long)]
        teaching: Option<PathBuf>,
        #[command(flatten)]
        voice: tts::VoiceArgs,
        #[command(flatten)]
        video: VideoArgs,
    },
    /// Write a study set for a lesson folder: multiple choice (quiz.json) then flashcards (flashcards.json)
    Study {
        dir: PathBuf,
        #[arg(long, value_enum, default_value_t = script::Provider::Gemini)]
        provider: script::Provider,
        #[arg(long)]
        model: Option<String>,
        /// What to cover (default: the lesson's focus from info.json)
        #[arg(long)]
        focus: Option<String>,
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
    let p = Progress::cli();
    match Cli::parse().cmd {
        Cmd::Make { inputs, out, cast, scenes, focus, model, provider, no_vision, recipe, teaching, voice, video } => {
            let teaching = match teaching {
                Some(f) => Some(std::fs::read_to_string(&f).with_context(|| format!("reading {}", f.display()))?),
                None => None,
            };
            let opts = pipeline::MakeOptions {
                inputs,
                out,
                lessons_dir: PathBuf::from("lessons"),
                cast,
                scenes,
                focus,
                model,
                provider,
                recipe,
                no_vision,
                teaching,
                voice,
                video: video.into(),
            };
            pipeline::make(&opts, &p, |_| {})?;
        }
        Cmd::Course { inputs, out, direction, yes, plan_only, only, cast, model, provider, no_vision, teaching, voice, video } => {
            let teaching = match teaching {
                Some(p) => Some(std::fs::read_to_string(&p).with_context(|| format!("reading {}", p.display()))?),
                None => None,
            };
            let p = Progress::cli();
            let o = pipeline::CourseOptions {
                inputs: inputs.clone(),
                out,
                lessons_dir: PathBuf::from("lessons"),
                direction,
                cast,
                model,
                provider,
                no_vision,
                teaching,
                voice,
                video: video.into(),
            };
            let resume = inputs.len() == 1 && inputs[0].is_dir() && inputs[0].join("plan.json").exists();
            let (dir, plan) = if resume {
                eprintln!("📂 Resuming the course in {}", inputs[0].display());
                (inputs[0].clone(), stickstudy::course::Plan::load(&inputs[0])?)
            } else {
                pipeline::plan_course(&o, &p, |_| {})?
            };
            plan.print();
            if plan_only {
                eprintln!("Plan saved to {}. Edit it if you like, then run:
  stickstudy course {}", dir.join("plan.json").display(), dir.display());
                return Ok(());
            }
            if !yes {
                eprint!("Make these {} videos? [Y/n] ", plan.videos.len());
                let mut answer = String::new();
                std::io::stdin().read_line(&mut answer)?;
                if answer.trim().to_lowercase().starts_with('n') {
                    eprintln!("Okay. The plan is saved; run `stickstudy course {}` when you're ready.", dir.display());
                    return Ok(());
                }
            }
            let made = pipeline::build_course(&dir, &o, &only, &p)?;
            eprintln!("
✅ Course ready: {} video(s) in {}", made.len(), dir.display());
            for d in &made {
                eprintln!("   {}", d.join("video.mp4").display());
            }
        }
        Cmd::Study { dir, provider, model, focus } => {
            let material = std::fs::read_to_string(dir.join("material.txt")).context("the lesson folder has no material.txt")?;
            let info = pipeline::LessonInfo::load(&dir).unwrap_or_default();
            let focus = focus.or(info.focus.clone()).unwrap_or_else(|| "The key terms and study questions in this material.".into());
            let title = if info.title.is_empty() { "Study set".to_string() } else { info.title.clone() };
            pipeline::write_study_set(&dir, &title, &material, &focus, provider, model.as_deref(), &Progress::cli())?;
        }
        Cmd::Render { dir, video } => {
            let mut lesson = Lesson::load(&dir)?;
            pipeline::render_videos(&mut lesson, &dir, &video.into(), &p)?;
        }
        Cmd::Revoice { dir, voice, video } => {
            pipeline::revoice(&dir, &voice, &video.into(), &p)?;
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

impl From<VideoArgs> for pipeline::VideoOptions {
    fn from(v: VideoArgs) -> Self {
        pipeline::VideoOptions {
            style: v.style,
            vertical: v.vertical,
            vertical_only: v.vertical_only,
            music: v.music,
            music_db: v.music_db,
            no_music: v.no_music,
            no_sfx: v.no_sfx,
            fps: v.fps,
            until: v.until,
            workers: v.workers,
        }
    }
}
