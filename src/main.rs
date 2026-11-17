mod extract;
mod formats;
mod looks;
mod lesson;
mod player;
mod script;
mod stickman;
mod tts;

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use lesson::Lesson;
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "stickstudy",
    version,
    about = "Turn class material (PDF, PowerPoint, transcripts) into fun, voiced animated lessons with quizzes"
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Create a lesson from class material, then play it
    Make {
        /// PDF, .pptx, .vtt/.srt transcript, .txt or .md files (several are combined into one lesson)
        #[arg(required = true)]
        inputs: Vec<PathBuf>,
        /// Output lesson folder (default: lessons/<first file name>)
        #[arg(short, long)]
        out: Option<PathBuf>,
        /// Video format: stickman, explainer, whiteboard or chat (`stickstudy formats` to compare)
        #[arg(short, long, value_enum, default_value_t = formats::Format::Stickman)]
        format: formats::Format,
        /// Force a number of scenes (default: scales with the material)
        #[arg(long)]
        scenes: Option<u32>,
        /// Topic or angle to focus on, e.g. "the midterm topics" or "explain the proofs"
        #[arg(long)]
        focus: Option<String>,
        /// Claude model alias for script writing (e.g. opus, sonnet)
        #[arg(long)]
        model: Option<String>,
        /// Replace the teaching-style instructions with the contents of this file
        /// (see `stickstudy style` for the default)
        #[arg(long)]
        style: Option<PathBuf>,
        #[command(flatten)]
        voice: tts::VoiceArgs,
        /// Don't open the player when done
        #[arg(long)]
        no_play: bool,
    },
    /// Watch a lesson you already made
    Play {
        dir: PathBuf,
        /// Start from this scene number
        #[arg(long, default_value_t = 1)]
        scene: usize,
        /// Play without sound
        #[arg(long)]
        mute: bool,
    },
    /// Re-generate the voices of an existing lesson (try different engines!)
    Revoice {
        dir: PathBuf,
        #[command(flatten)]
        voice: tts::VoiceArgs,
    },
    /// List voice engines and the voices available on this Mac
    Voices,
    /// List the video formats
    Formats,
    /// Print the script instructions Claude follows (copy, edit, pass with --style)
    Style {
        /// Also show this format's own instructions
        #[arg(short, long, value_enum)]
        format: Option<formats::Format>,
    },
}

fn main() {
    if let Err(e) = run() {
        eprintln!("\n❌ {e:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    match Cli::parse().cmd {
        Cmd::Make { inputs, out, format, scenes, focus, model, style, voice, no_play } => {
            for p in &inputs {
                if !p.exists() {
                    bail!("file not found: {}", p.display());
                }
            }
            let voice_engine = tts::Voice::new(&voice)?; // validate voice setup before the slow part
            let style = match style {
                Some(p) => Some(std::fs::read_to_string(&p).with_context(|| format!("reading {}", p.display()))?),
                None => None,
            };
            let dir = out.unwrap_or_else(|| {
                let stem = inputs[0].file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or("lesson".into());
                PathBuf::from("lessons").join(format!("{stem}-{}", format.name()))
            });

            eprintln!("📚 Reading your material…");
            let material = extract::extract_all(&inputs)?;
            std::fs::create_dir_all(&dir)?;
            std::fs::write(dir.join("material.txt"), &material)?;
            eprintln!("  {} characters of text extracted", material.len());

            eprintln!("🎬 Format: {} — {}", format.name(), format.tagline());
            eprintln!("✍️  Claude is writing your lesson script (usually 1–3 minutes)…");
            let opts = script::ScriptOptions { scenes, focus, model, style, format };
            let mut lesson = script::write_lesson(&material, &opts)?;
            lesson.save(&dir)?;
            let quizzes = lesson.scenes.iter().filter(|s| s.quiz.is_some()).count();
            eprintln!("  \"{}\" — {} scenes, {} quizzes", lesson.title, lesson.scenes.len(), quizzes);

            eprintln!("🗣  Recording voices: {}", voice_engine.describe());
            tts::voice_lesson(&mut lesson, &dir, &voice_engine)?;
            lesson.save(&dir)?;

            eprintln!("✅ Lesson saved to {}", dir.display());
            eprintln!("   watch again any time:  stickstudy play {}", dir.display());
            if !no_play {
                player::run(lesson, dir, 0, false);
            }
        }
        Cmd::Play { dir, scene, mute } => {
            let lesson = Lesson::load(&dir)?;
            player::run(lesson, dir, scene.saturating_sub(1), mute);
        }
        Cmd::Revoice { dir, voice } => {
            let mut lesson = Lesson::load(&dir)?;
            let v = tts::Voice::new(&voice)?;
            eprintln!("🗣  Recording voices: {}", v.describe());
            tts::voice_lesson(&mut lesson, &dir, &v)?;
            lesson.save(&dir)?;
            eprintln!("✅ Done. Play it:  stickstudy play {}", dir.display());
        }
        Cmd::Voices => tts::print_voice_guide(),
        Cmd::Formats => {
            println!("Video formats (pick with --format <name>):\n");
            for f in formats::ALL {
                println!("  {:<11} {}", f.name(), f.tagline());
            }
            println!("\nExample:  stickstudy make lecture.pdf --format explainer");
            println!("See a format's script instructions:  stickstudy style --format chat");
        }
        Cmd::Style { format } => {
            println!("# Teaching style (shared by all formats; replace with --style <file>)\n");
            println!("{}", script::DEFAULT_STYLE);
            if let Some(f) = format {
                println!("\n# {} format instructions (built in)\n", f.name());
                println!("{}", f.prompt());
            }
        }
    }
    Ok(())
}
