//! Voice generation. Several interchangeable engines; every engine produces a
//! 16-bit WAV per dialogue line so the player can time animations to speech.

use crate::lesson::{Lesson, Speaker};
use crate::omnivoice;
use anyhow::{bail, Context, Result};
use clap::{Args, ValueEnum};
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum Backend {
    /// Best free offline voice here: omnivoice if installed, else sapi (Windows) / say (macOS) / espeak
    Auto,
    /// Windows built-in voices (free, offline)
    Sapi,
    /// macOS built-in voices (free, offline)
    Say,
    /// Piper neural TTS (free, offline, natural) — needs `piper` + a .onnx voice model
    Piper,
    /// OpenAI gpt-4o-mini-tts (very natural, paid) — needs OPENAI_API_KEY
    Openai,
    /// OmniVoice (free, local, natural; GPU via Vulkan, voice design + cloning) — needs STICKSTUDY_OMNIVOICE_DIR
    Omnivoice,
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
    #[arg(long = "voice", value_enum, default_value_t = Backend::Auto)]
    pub backend: Backend,
    /// Teacher's / narrator's voice (voice name, voice ID, or .onnx model path for piper)
    #[arg(long)]
    pub teacher_voice: Option<String>,
    /// Student's voice (voice name, voice ID, or .onnx model path for piper)
    #[arg(long)]
    pub student_voice: Option<String>,
    /// Speaking speed (words per minute for say/espeak; -10..10 for sapi; 0.25–4.0 multiplier for openai)
    #[arg(long)]
    pub speed: Option<f32>,
    /// omnivoice: how lively the voices sound
    #[arg(long, value_enum, default_value_t = omnivoice::Energy::Upbeat)]
    pub energy: omnivoice::Energy,
    /// Speed every line up (or down) afterwards without changing pitch, e.g. 1.15 = 15% faster
    #[arg(long, default_value_t = 1.0)]
    pub pace: f32,
}

pub struct Voice {
    backend: Backend,
    teacher: String,
    student: String,
    speed: Option<f32>,
    http: Option<reqwest::blocking::Client>,
    /// the running OmniVoice server (started on first use, stopped when the Voice is dropped)
    omni: std::cell::RefCell<Option<omnivoice::Server>>,
    energy: omnivoice::Energy,
    pace: f32,
}

impl Voice {
    pub fn new(a: &VoiceArgs) -> Result<Voice> {
        let backend = match a.backend {
            Backend::Auto if omnivoice::find_dir().is_some() => Backend::Omnivoice,
            Backend::Auto if cfg!(windows) => Backend::Sapi,
            Backend::Auto if cfg!(target_os = "macos") => Backend::Say,
            Backend::Auto => Backend::Espeak,
            b => b,
        };
        let a = &VoiceArgs { backend, ..a.clone() };
        let (dt, ds) = match a.backend {
            Backend::Sapi => ("Microsoft David Desktop", "Microsoft Zira Desktop"),
            Backend::Say => ("Daniel", "Samantha"),
            Backend::Openai => ("fable", "nova"),
            Backend::Omnivoice => (omnivoice::DEFAULT_TEACHER, omnivoice::DEFAULT_STUDENT),
            Backend::Elevenlabs => ("pNInz6obpgDQGcFmaJgB", "21m00Tcm4TlvDq8ikWAM"), // Adam, Rachel
            Backend::Espeak => ("en-gb+m3", "en-us+f3"),
            Backend::Piper | Backend::None | Backend::Auto => ("", ""),
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
        Ok(Voice { backend: a.backend, teacher, student, speed: a.speed, http, omni: std::cell::RefCell::new(None), energy: a.energy, pace: a.pace.clamp(0.5, 2.0) })
    }

    pub fn describe(&self) -> String {
        match self.backend {
            Backend::None => "no voice (subtitles only)".into(),
            Backend::Omnivoice => format!(
                "omnivoice — teacher/narrator: {}, student: {}, energy: {:?}{}",
                self.teacher,
                self.student,
                self.energy,
                if (self.pace - 1.0).abs() > 0.01 { format!(", pace ×{}", self.pace) } else { String::new() }
            ),
            b => format!("{b:?} — teacher/narrator: {}, student: {}", self.teacher, self.student),
        }
    }

    fn synth(&self, text: &str, who: Speaker, out: &Path) -> Result<()> {
        let voice = match who {
            Speaker::Teacher | Speaker::Narrator => &self.teacher,
            Speaker::Student => &self.student,
        };
        match self.backend {
            Backend::Sapi => sapi_batch(&[(text.to_string(), voice.clone(), out.to_path_buf())], self.speed),
            Backend::Auto => unreachable!(),
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
            Backend::Omnivoice => {
                let mut omni = self.omni.borrow_mut();
                let server = omni.as_mut().context("OmniVoice server not started")?;
                let name = server.voice_for(voice)?;
                server.speak(text, &name, out)
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

/// Generate audio for every line, filling in `audio`, `audio_start` and `duration`.
pub fn voice_lesson(lesson: &mut Lesson, dir: &Path, voice: &Voice) -> Result<()> {
    let audio_dir = dir.join("audio");
    if audio_dir.exists() {
        std::fs::remove_dir_all(&audio_dir)?;
    }
    std::fs::create_dir_all(&audio_dir)?;
    let total = lesson.line_count();
    if voice.backend == Backend::Omnivoice {
        eprintln!("  🚀 starting OmniVoice…");
        let mut server = omnivoice::Server::start()?;
        server.voices_dir = Some(dir.join("voices"));
        server.energy = voice.energy;
        eprintln!("  {}", server.device);
        *voice.omni.borrow_mut() = Some(server);
    }
    if voice.backend == Backend::Sapi {
        // one PowerShell process for the whole lesson (starting one per line is slow)
        let mut jobs = vec![];
        for (si, scene) in lesson.scenes.iter().enumerate() {
            for (li, line) in scene.lines.iter().enumerate() {
                let v = match line.speaker {
                    Speaker::Student => voice.student.clone(),
                    _ => voice.teacher.clone(),
                };
                jobs.push((line.text.clone(), v, dir.join(format!("audio/s{si:02}_l{li:02}.wav"))));
            }
        }
        eprintln!("  🎙  voicing {total} lines with Windows speech…");
        sapi_batch(&jobs, voice.speed)?;
    }
    let mut done = 0;
    for (si, scene) in lesson.scenes.iter_mut().enumerate() {
        for (li, line) in scene.lines.iter_mut().enumerate() {
            done += 1;
            if voice.backend != Backend::Sapi {
                eprint!("\r  🎙  voicing line {done}/{total}   ");
                let _ = std::io::stderr().flush();
            }
            if voice.backend == Backend::None {
                line.audio = None;
                line.audio_start = 0.0;
                line.duration = estimate_seconds(&line.text);
                continue;
            }
            let rel = format!("audio/s{si:02}_l{li:02}.wav");
            let path = dir.join(&rel);
            if voice.backend != Backend::Sapi {
                voice
                    .synth(&line.text, line.speaker, &path)
                    .with_context(|| format!("voicing scene {} line {}", si + 1, li + 1))?;
            }
            if (voice.pace - 1.0).abs() > 0.01 {
                change_pace(&path, voice.pace)?;
            }
            let (start, dur) = speech_span(&path)
                .with_context(|| format!("reading generated audio {}", path.display()))?;
            line.audio_start = start;
            line.duration = dur;
            line.audio = Some(rel);
        }
    }
    if voice.backend != Backend::Sapi {
        eprintln!();
    }
    Ok(())
}

/// Where speech starts and how long it lasts, ignoring silence at both ends.
pub fn speech_span(p: &Path) -> Result<(f32, f32)> {
    let mut r = hound::WavReader::open(p)?;
    let spec = r.spec();
    let ch = spec.channels.max(1) as usize;
    let sr = spec.sample_rate as f32;
    let s: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Int => {
            let max = (1i64 << (spec.bits_per_sample - 1)) as f32;
            r.samples::<i32>().filter_map(|x| x.ok()).map(|x| x as f32 / max).collect()
        }
        hound::SampleFormat::Float => r.samples::<f32>().filter_map(|x| x.ok()).collect(),
    };
    let mono: Vec<f32> = s.chunks(ch).map(|c| c.iter().map(|v| v.abs()).fold(0.0, f32::max)).collect();
    let total = mono.len() as f32 / sr;
    let peak = mono.iter().fold(0.0f32, |a, v| a.max(*v));
    if peak <= 0.0 || mono.is_empty() {
        return Ok((0.0, total.max(0.3)));
    }
    let thr = peak * 0.04;
    let first = mono.iter().position(|v| *v > thr).unwrap_or(0) as f32 / sr;
    let last = mono.iter().rposition(|v| *v > thr).unwrap_or(mono.len() - 1) as f32 / sr;
    let start = (first - 0.04).max(0.0);
    let end = (last + 0.08).min(total);
    Ok((start, (end - start).max(0.3)))
}

/// Windows SAPI via PowerShell's System.Speech. `jobs` = (text, voice name, output wav).
fn sapi_batch(jobs: &[(String, String, std::path::PathBuf)], speed: Option<f32>) -> Result<()> {
    let tmp = std::env::temp_dir().join(format!("stickstudy_sapi_{}", std::process::id()));
    std::fs::create_dir_all(&tmp)?;
    let list: Vec<serde_json::Value> = jobs
        .iter()
        .map(|(t, v, o)| serde_json::json!({ "text": t, "voice": v, "out": o.to_string_lossy() }))
        .collect();
    let jobs_path = tmp.join("jobs.json");
    std::fs::write(&jobs_path, serde_json::to_string(&list)?)?;
    let rate = speed.map(|s| s.clamp(-10.0, 10.0) as i32).unwrap_or(2);
    let script = format!(
        r#"$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Speech
$jobs = Get-Content -Raw -Encoding UTF8 '{jobs}' | ConvertFrom-Json
$s = New-Object System.Speech.Synthesis.SpeechSynthesizer
$s.Rate = {rate}
$fmt = New-Object System.Speech.AudioFormat.SpeechAudioFormatInfo(22050, [System.Speech.AudioFormat.AudioBitsPerSample]::Sixteen, [System.Speech.AudioFormat.AudioChannel]::Mono)
$names = $s.GetInstalledVoices() | ForEach-Object {{ $_.VoiceInfo.Name }}
foreach ($j in $jobs) {{
  if ($names -contains $j.voice) {{ $s.SelectVoice($j.voice) }}
  $s.SetOutputToWaveFile($j.out, $fmt)
  $s.Speak($j.text)
  $s.SetOutputToNull()
}}
$s.Dispose()
"#,
        jobs = jobs_path.display(),
        rate = rate
    );
    let ps1 = tmp.join("voice.ps1");
    // UTF-8 with BOM so Windows PowerShell 5 reads non-ASCII text correctly
    let mut bytes = vec![0xEF, 0xBB, 0xBF];
    bytes.extend_from_slice(script.as_bytes());
    std::fs::write(&ps1, bytes)?;
    let out = Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-File"])
        .arg(&ps1)
        .output()
        .context("couldn't run PowerShell for Windows speech")?;
    let _ = std::fs::remove_dir_all(&tmp);
    if !out.status.success() {
        bail!("Windows speech failed: {}", String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(())
}

pub fn estimate_seconds(text: &str) -> f32 {
    (text.split_whitespace().count() as f32 / 2.6).max(1.5)
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

/// Time-stretch a WAV in place (pitch stays the same).
fn change_pace(path: &Path, pace: f32) -> Result<()> {
    let ff = crate::render::ffmpeg()?;
    let tmp = path.with_extension("pace.wav");
    let st = Command::new(&ff)
        .args(["-v", "error", "-y", "-i"])
        .arg(path)
        .args(["-af", &format!("atempo={pace}"), "-c:a", "pcm_s16le"])
        .arg(&tmp)
        .status()
        .context("running ffmpeg to change the pace")?;
    if !st.success() {
        bail!("ffmpeg couldn't change the pace of {}", path.display());
    }
    std::fs::rename(&tmp, path)?;
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
    let finder = if cfg!(windows) { "where" } else { "which" };
    Command::new(finder).arg(bin).output().map(|o| o.status.success()).unwrap_or(false)
}

/// `stickstudy voices` — show every engine and how to use it.
pub fn print_voice_guide() {
    println!("Voice engines (pick with --voice <engine>):\n");
    println!("  auto        [default] sapi on Windows, say on macOS, espeak on Linux");
    println!("  sapi        Windows built-in. Free, offline, zero setup.");
    println!("              default voices: teacher=Microsoft David Desktop, student=Microsoft Zira Desktop");
    println!("              tip: --speed -10..10 (default 2, a little brisk)");
    println!("  say         macOS built-in. Free, offline, zero setup.");
    println!("              default voices: teacher=Daniel, student=Samantha");
    println!("              tip: download nicer 'Premium'/'Enhanced' voices in System Settings →");
    println!("              Accessibility → Spoken Content → System Voice → Manage Voices");
    println!("  piper       Neural, free, offline, natural.  `pip install piper-tts`, then download a");
    println!("              voice from https://huggingface.co/rhasspy/piper-voices and pass");
    println!("              --teacher-voice en_US-ryan-high.onnx --student-voice en_US-amy-medium.onnx");
    println!("  openai      Very natural & expressive (acting instructions per character). Paid, ~1-3¢/lesson.");
    println!("              export OPENAI_API_KEY=…  voices: alloy ash ballad coral echo fable nova onyx sage shimmer verse");
    println!("  omnivoice   Free, local, natural. Runs on your GPU through Vulkan (AMD/NVIDIA/Intel) or the CPU.");
    println!("              Needs STICKSTUDY_OMNIVOICE_DIR (bin/ + models/). Voices are either a description from:");
    println!("              male|female, child|teenager|young adult|middle-aged|elderly,");
    println!("              very low|low|moderate|high|very high pitch, whisper, american|british|australian|... accent");
    println!("              e.g. --teacher-voice \"female, middle-aged, low pitch, british accent\"");
    println!("              or a .wav to clone (put its transcript in a .txt next to it),");
    println!("              or a gallery preset name (default: teacher \"The Champion\", student \"The Neighbor\"):");
    for chunk in omnivoice::PRESETS.chunks(4) {
        println!("                {}", chunk.iter().map(|(n, _)| format!("{n:<20}")).collect::<String>());
    }
    println!("  elevenlabs  Most expressive. Paid/free tier.  export ELEVENLABS_API_KEY=…");
    println!("              voices are IDs from elevenlabs.io/app/voice-library (defaults: Adam, Rachel)");
    println!("  espeak      Retro robot voice, free. `brew install espeak-ng`  voices: en-us+m3, en-gb+f2, …");
    println!("  none        Silent, subtitles only.\n");

    if cfg!(windows) {
        let ps = "Add-Type -AssemblyName System.Speech; (New-Object System.Speech.Synthesis.SpeechSynthesizer).GetInstalledVoices() | ForEach-Object { $_.VoiceInfo.Name }";
        if let Ok(out) = Command::new("powershell").args(["-NoProfile", "-Command", ps]).output() {
            println!("Windows voices installed here (use with --voice sapi --teacher-voice \"<name>\"):");
            for l in String::from_utf8_lossy(&out.stdout).lines().filter(|l| !l.trim().is_empty()) {
                println!("  {}", l.trim());
            }
        }
    }
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
