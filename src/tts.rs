//! Voice generation. Several interchangeable engines; every engine produces a
//! 16-bit WAV per dialogue line so the player can time animations to speech.

use crate::lesson::{Lesson, Speaker};
use anyhow::{bail, Context, Result};
use clap::{Args, ValueEnum};
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum Backend {
    /// macOS built-in voices (free, offline)
    Say,
    /// Piper neural TTS (free, offline, natural) — needs `piper` + a .onnx voice model
    Piper,
    /// OpenAI gpt-4o-mini-tts (very natural, paid) — needs OPENAI_API_KEY
    Openai,
    /// ElevenLabs (most expressive, paid) — needs ELEVENLABS_API_KEY
    Elevenlabs,
    /// eSpeak NG (free, offline, robotic retro sound) — `brew install espeak-ng`
    Espeak,
    /// No audio; subtitles only
    None,
}

#[derive(Args, Debug, Clone)]
pub struct VoiceArgs {
    /// Voice engine to use
    #[arg(long = "voice", value_enum, default_value_t = Backend::Say)]
    pub backend: Backend,
    /// Teacher's / narrator's voice (voice name, voice ID, or .onnx model path for piper)
    #[arg(long)]
    pub teacher_voice: Option<String>,
    /// Student's voice (voice name, voice ID, or .onnx model path for piper)
    #[arg(long)]
    pub student_voice: Option<String>,
    /// Speaking speed (words per minute for say/espeak; 0.25–4.0 multiplier for openai)
    #[arg(long)]
    pub speed: Option<f32>,
}

pub struct Voice {
    backend: Backend,
    teacher: String,
    student: String,
    speed: Option<f32>,
    http: Option<reqwest::blocking::Client>,
}

impl Voice {
    pub fn new(a: &VoiceArgs) -> Result<Voice> {
        let (dt, ds) = match a.backend {
            Backend::Say => ("Daniel", "Samantha"),
            Backend::Openai => ("fable", "nova"),
            Backend::Elevenlabs => ("pNInz6obpgDQGcFmaJgB", "21m00Tcm4TlvDq8ikWAM"), // Adam, Rachel
            Backend::Espeak => ("en-gb+m3", "en-us+f3"),
            Backend::Piper | Backend::None => ("", ""),
        };
        let teacher = a.teacher_voice.clone().unwrap_or_else(|| dt.to_string());
        let student = a
            .student_voice
            .clone()
            .unwrap_or_else(|| if a.backend == Backend::Piper { teacher.clone() } else { ds.to_string() });

        match a.backend {
            Backend::Piper if teacher.is_empty() => bail!(
                "piper needs a voice model: --teacher-voice path/to/en_US-lessac-medium.onnx \
                 (download voices from https://huggingface.co/rhasspy/piper-voices)"
            ),
            Backend::Openai if std::env::var("OPENAI_API_KEY").is_err() => {
                bail!("set OPENAI_API_KEY to use --voice openai")
            }
            Backend::Elevenlabs if std::env::var("ELEVENLABS_API_KEY").is_err() => {
                bail!("set ELEVENLABS_API_KEY to use --voice elevenlabs")
            }
            _ => {}
        }
        let http = matches!(a.backend, Backend::Openai | Backend::Elevenlabs).then(|| {
            reqwest::blocking::Client::builder()
                .timeout(std::time::Duration::from_secs(120))
                .build()
                .expect("http client")
        });
        Ok(Voice { backend: a.backend, teacher, student, speed: a.speed, http })
    }

    pub fn describe(&self) -> String {
        match self.backend {
            Backend::None => "no voice (subtitles only)".into(),
            b => format!("{b:?} — teacher: {}, student: {}", self.teacher, self.student),
        }
    }

    fn synth(&self, text: &str, who: Speaker, out: &Path) -> Result<()> {
        let voice = match who {
            Speaker::Teacher | Speaker::Narrator => &self.teacher,
            Speaker::Student => &self.student,
        };
        match self.backend {
            Backend::Say => {
                let mut c = Command::new("say");
                c.args(["-v", voice, "--data-format=LEI16@22050", "-o"]).arg(out);
                // Slightly brisk by default for a snappy, short-form feel.
                c.args(["-r", &format!("{}", self.speed.unwrap_or(200.0) as u32)]);
                c.args(["-f", "-"]);
                run_with_stdin(c, text, "say")
            }
            Backend::Piper => {
                let bin = std::env::var("PIPER_BIN").unwrap_or_else(|_| "piper".into());
                let mut c = Command::new(bin);
                c.args(["--model", voice, "--output_file"]).arg(out);
                run_with_stdin(c, text, "piper (install: `pip install piper-tts`)")
            }
            Backend::Espeak => {
                let bin = if which("espeak-ng") { "espeak-ng" } else { "espeak" };
                let mut c = Command::new(bin);
                c.args(["-v", voice, "-w"]).arg(out);
                if let Some(s) = self.speed {
                    c.args(["-s", &format!("{}", s as u32)]);
                }
                c.arg("--stdin");
                run_with_stdin(c, text, "espeak-ng (install: `brew install espeak-ng`)")
            }
            Backend::Openai => {
                let instructions = match who {
                    Speaker::Teacher => "Speak like a warm, enthusiastic teacher who loves this topic. Lively but clear.",
                    Speaker::Student => "Speak like a curious, playful university student. Natural and expressive.",
                    Speaker::Narrator => "Speak like a witty, warm documentary narrator full of wonder. Upbeat pacing, dramatic little pauses.",
                };
                let mut body = serde_json::json!({
                    "model": "gpt-4o-mini-tts",
                    "input": text,
                    "voice": voice,
                    "instructions": instructions,
                    "response_format": "pcm",
                });
                if let Some(s) = self.speed {
                    body["speed"] = s.into();
                }
                let bytes = self
                    .http
                    .as_ref()
                    .unwrap()
                    .post("https://api.openai.com/v1/audio/speech")
                    .bearer_auth(std::env::var("OPENAI_API_KEY")?)
                    .json(&body)
                    .send()?
                    .error_for_status()
                    .context("OpenAI TTS request failed")?
                    .bytes()?;
                write_pcm16(out, &bytes, 24_000)
            }
            Backend::Elevenlabs => {
                let url = format!(
                    "https://api.elevenlabs.io/v1/text-to-speech/{voice}?output_format=pcm_22050"
                );
                let bytes = self
                    .http
                    .as_ref()
                    .unwrap()
                    .post(url)
                    .header("xi-api-key", std::env::var("ELEVENLABS_API_KEY")?)
                    .json(&serde_json::json!({ "text": text, "model_id": "eleven_turbo_v2_5" }))
                    .send()?
                    .error_for_status()
                    .context("ElevenLabs request failed")?
                    .bytes()?;
                write_pcm16(out, &bytes, 22_050)
            }
            Backend::None => Ok(()),
        }
    }
}

/// Generate audio for every line, filling in `audio` and `duration`.
pub fn voice_lesson(lesson: &mut Lesson, dir: &Path, voice: &Voice) -> Result<()> {
    let audio_dir = dir.join("audio");
    if audio_dir.exists() {
        std::fs::remove_dir_all(&audio_dir)?;
    }
    std::fs::create_dir_all(&audio_dir)?;
    let total = lesson.line_count();
    let mut done = 0;
    for (si, scene) in lesson.scenes.iter_mut().enumerate() {
        for (li, line) in scene.lines.iter_mut().enumerate() {
            done += 1;
            eprint!("\r  🎙  voicing line {done}/{total}   ");
            let _ = std::io::stderr().flush();
            if voice.backend == Backend::None {
                line.audio = None;
                line.duration = estimate_seconds(&line.text);
                continue;
            }
            let rel = format!("audio/s{si:02}_l{li:02}.wav");
            let path = dir.join(&rel);
            voice
                .synth(&line.text, line.speaker, &path)
                .with_context(|| format!("voicing scene {} line {}", si + 1, li + 1))?;
            line.duration = wav_seconds(&path)
                .with_context(|| format!("reading generated audio {}", path.display()))?;
            line.audio = Some(rel);
        }
    }
    eprintln!();
    Ok(())
}

pub fn estimate_seconds(text: &str) -> f32 {
    (text.split_whitespace().count() as f32 / 2.6).max(1.5)
}

fn wav_seconds(p: &Path) -> Result<f32> {
    let r = hound::WavReader::open(p)?;
    Ok(r.duration() as f32 / r.spec().sample_rate as f32)
}

fn write_pcm16(out: &Path, bytes: &[u8], rate: u32) -> Result<()> {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut w = hound::WavWriter::create(out, spec)?;
    for ch in bytes.chunks_exact(2) {
        w.write_sample(i16::from_le_bytes([ch[0], ch[1]]))?;
    }
    w.finalize()?;
    Ok(())
}

fn run_with_stdin(mut c: Command, text: &str, what: &str) -> Result<()> {
    c.stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::piped());
    let mut child = c.spawn().with_context(|| format!("couldn't run {what}"))?;
    child.stdin.take().unwrap().write_all(text.as_bytes())?;
    let out = child.wait_with_output()?;
    if !out.status.success() {
        bail!("{what} failed: {}", String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(())
}

fn which(bin: &str) -> bool {
    Command::new("which").arg(bin).output().map(|o| o.status.success()).unwrap_or(false)
}

/// `stickstudy voices` — show every engine and how to use it.
pub fn print_voice_guide() {
    println!("Voice engines (pick with --voice <engine>):\n");
    println!("  say         macOS built-in. Free, offline, zero setup.  [default]");
    println!("              default voices: teacher=Daniel, student=Samantha");
    println!("              tip: download nicer 'Premium'/'Enhanced' voices in System Settings →");
    println!("              Accessibility → Spoken Content → System Voice → Manage Voices");
    println!("  piper       Neural, free, offline, natural.  `pip install piper-tts`, then download a");
    println!("              voice from https://huggingface.co/rhasspy/piper-voices and pass");
    println!("              --teacher-voice en_US-ryan-high.onnx --student-voice en_US-amy-medium.onnx");
    println!("  openai      Very natural & expressive (acting instructions per character). Paid, ~1-3¢/lesson.");
    println!("              export OPENAI_API_KEY=…  voices: alloy ash ballad coral echo fable nova onyx sage shimmer verse");
    println!("  elevenlabs  Most expressive. Paid/free tier.  export ELEVENLABS_API_KEY=…");
    println!("              voices are IDs from elevenlabs.io/app/voice-library (defaults: Adam, Rachel)");
    println!("  espeak      Retro robot voice, free. `brew install espeak-ng`  voices: en-us+m3, en-gb+f2, …");
    println!("  none        Silent, subtitles only.\n");

    if let Ok(out) = Command::new("say").args(["-v", "?"]).output() {
        let list = String::from_utf8_lossy(&out.stdout);
        let novelty = [
            "Bad News", "Bahh", "Bells", "Boing", "Bubbles", "Cellos", "Good News", "Jester", "Organ",
            "Superstar", "Trinoids", "Whisper", "Wobble", "Zarvox", "Albert", "Fred", "Junior", "Ralph",
            "Kathy",
        ];
        let names: Vec<String> = list
            .lines()
            .filter(|l| l.contains("en_"))
            .filter_map(|l| l.split("  ").next().map(|s| s.trim().to_string()))
            .filter(|n| !novelty.iter().any(|x| n.starts_with(x)))
            .collect();
        if !names.is_empty() {
            println!("English macOS voices installed on this Mac (use with --voice say):");
            for chunk in names.chunks(4) {
                println!("  {}", chunk.iter().map(|n| format!("{n:<28}")).collect::<String>());
            }
            println!("\nTry one:  say -v \"Daniel\" \"Hello class\"");
        }
    }
}
