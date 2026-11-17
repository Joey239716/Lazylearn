//! OmniVoice through its native `tts-server` (omnivoice.cpp, from the VoiceStudio project):
//! no Python, GPU through Vulkan (AMD / NVIDIA / Intel) or CPU.
//!
//! Folder layout (point STICKSTUDY_OMNIVOICE_DIR at it):
//!   bin/tts-server-<platform>[.exe] + its ggml libraries
//!   models/omnivoice-base-<QUANT>.gguf + models/omnivoice-tokenizer-<QUANT>.gguf
//!
//! Each character's voice is designed once from a description ("male, middle-aged,
//! low pitch") or cloned from a .wav, saved in the lesson's `voices/` folder, and
//! registered with the server, so a character sounds the same on every line.

use anyhow::{bail, Context, Result};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

pub const DEFAULT_TEACHER: &str = "male, middle-aged, low pitch, american accent";
pub const DEFAULT_STUDENT: &str = "female, young adult, high pitch, american accent";
/// What a designed voice says once, to become the reference clip for every line.
const REF_TEXT: &str =
    "Hey there! Today we're going to learn something really cool, and I promise it's going to be fun.";
const REF_SEED: i64 = 1234;

/// The OmniVoice folder, if it is set up.
pub fn find_dir() -> Option<PathBuf> {
    let mut candidates = vec![];
    if let Ok(d) = std::env::var("STICKSTUDY_OMNIVOICE_DIR") {
        candidates.push(PathBuf::from(d));
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(d) = exe.parent() {
            candidates.push(d.join("omnivoice"));
        }
    }
    candidates.into_iter().find(|d| server_bin(d).is_some() && models(d).is_some())
}

fn server_bin(dir: &Path) -> Option<PathBuf> {
    std::fs::read_dir(dir.join("bin"))
        .ok()?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .find(|p| {
            p.file_name()
                .map(|n| n.to_string_lossy().starts_with("tts-server"))
                .unwrap_or(false)
        })
}

/// (base model, tokenizer); prefers Q8_0, then BF16, then whatever matching pair exists.
fn models(dir: &Path) -> Option<(PathBuf, PathBuf)> {
    let m = dir.join("models");
    let pair = |q: &str| {
        let b = m.join(format!("omnivoice-base-{q}.gguf"));
        let t = m.join(format!("omnivoice-tokenizer-{q}.gguf"));
        (b.exists() && t.exists()).then_some((b, t))
    };
    if let Ok(q) = std::env::var("STICKSTUDY_OMNIVOICE_QUANT") {
        return pair(&q);
    }
    for q in ["Q8_0", "BF16", "Q4_K_M", "F32"] {
        if let Some(p) = pair(q) {
            return Some(p);
        }
    }
    None
}

pub struct Server {
    child: Child,
    url: String,
    http: reqwest::blocking::Client,
    voices: HashMap<String, String>,
    /// Where designed reference clips are kept (the lesson's voices/ folder)
    pub voices_dir: Option<PathBuf>,
    /// Which device the model runs on, for the progress output
    pub device: String,
}

impl Server {
    pub fn start() -> Result<Server> {
        let Some(dir) = find_dir() else {
            bail!(
                "OmniVoice isn't set up. Set STICKSTUDY_OMNIVOICE_DIR to a folder with bin/tts-server-* and \
                 models/omnivoice-base-Q8_0.gguf + omnivoice-tokenizer-Q8_0.gguf (see README)"
            );
        };
        let bin = server_bin(&dir).unwrap();
        let (base, tok) = models(&dir).unwrap();
        let port = std::net::TcpListener::bind("127.0.0.1:0")?.local_addr()?.port();
        let log_path = std::env::temp_dir().join(format!("stickstudy-omnivoice-{port}.log"));
        let log = std::fs::File::create(&log_path)?;
        let child = Command::new(&bin)
            .arg("--model")
            .arg(&base)
            .arg("--codec")
            .arg(&tok)
            .args(["--host", "127.0.0.1", "--port", &port.to_string(), "--lang", "English"])
            .current_dir(bin.parent().unwrap())
            .stdout(Stdio::from(log.try_clone()?))
            .stderr(Stdio::from(log))
            .spawn()
            .with_context(|| format!("starting {}", bin.display()))?;
        let http = reqwest::blocking::Client::builder().timeout(Duration::from_secs(600)).build()?;
        let mut server = Server {
            child,
            url: format!("http://127.0.0.1:{port}"),
            http,
            voices: HashMap::new(),
            voices_dir: None,
            device: String::new(),
        };
        let t0 = Instant::now();
        loop {
            if let Ok(Some(status)) = server.child.try_wait() {
                let log = std::fs::read_to_string(&log_path).unwrap_or_default();
                bail!("OmniVoice server stopped ({status}):\n{}", tail(&log, 15));
            }
            let ok = server
                .http
                .get(format!("{}/v1/models", server.url))
                .timeout(Duration::from_secs(2))
                .send()
                .map(|r| r.status().is_success())
                .unwrap_or(false);
            if ok {
                break;
            }
            if t0.elapsed() > Duration::from_secs(240) {
                bail!("OmniVoice server didn't start within 4 minutes (log: {})", log_path.display());
            }
            std::thread::sleep(Duration::from_millis(400));
        }
        let log = std::fs::read_to_string(&log_path).unwrap_or_default();
        let gpu = log.lines().find(|l| l.contains("ggml_vulkan: 0 =")).map(|l| {
            l.split('=').nth(1).unwrap_or("").split('|').next().unwrap_or("").trim().to_string()
        });
        let backend = log.lines().find_map(|l| l.split("LM backend: ").nth(1)).unwrap_or("?").trim().to_string();
        server.device = match gpu {
            Some(g) if backend.starts_with("Vulkan") => format!("OmniVoice on {g} (Vulkan)"),
            _ => format!("OmniVoice on {backend}"),
        };
        Ok(server)
    }

    /// Register (once) and return the server-side voice name for a description or a .wav to clone.
    pub fn voice_for(&mut self, spec: &str) -> Result<String> {
        if let Some(n) = self.voices.get(spec) {
            return Ok(n.clone());
        }
        let (wav, ref_text) = if spec.to_lowercase().ends_with(".wav") {
            let p = PathBuf::from(spec);
            let txt = p.with_extension("txt");
            let wav = std::fs::read(&p).with_context(|| format!("reading voice sample {}", p.display()))?;
            let text = std::fs::read_to_string(&txt).with_context(|| {
                format!("cloning {} needs its transcript in {}", p.display(), txt.display())
            })?;
            (wav, text.trim().to_string())
        } else {
            (self.designed_clip(spec)?, REF_TEXT.to_string())
        };
        let name = format!("ss-{:x}", fxhash(spec));
        let resp = self
            .http
            .post(format!("{}/v1/audio/voices", self.url))
            .json(&serde_json::json!({ "name": name, "ref_text": ref_text, "wav_b64": b64(&wav) }))
            .send()
            .context("registering a voice with OmniVoice")?;
        if !resp.status().is_success() {
            bail!("OmniVoice wouldn't register voice '{spec}': {}", resp.text().unwrap_or_default());
        }
        self.voices.insert(spec.to_string(), name.clone());
        Ok(name)
    }

    /// A reference clip for a voice description, reused from the lesson folder when it exists.
    fn designed_clip(&self, desc: &str) -> Result<Vec<u8>> {
        let cache = self.voices_dir.as_ref().map(|d| d.join(format!("{}.wav", slug(desc))));
        if let Some(c) = &cache {
            if let Ok(b) = std::fs::read(c) {
                return Ok(b);
            }
        }
        let wav = self.post_speech(serde_json::json!({
            "input": REF_TEXT,
            "instructions": desc,
            "seed": REF_SEED,
            "language": "English",
            "response_format": "wav",
        }))?;
        if let Some(c) = &cache {
            std::fs::create_dir_all(c.parent().unwrap())?;
            std::fs::write(c, &wav)?;
        }
        Ok(wav)
    }

    pub fn speak(&self, text: &str, voice_name: &str, out: &Path) -> Result<()> {
        let wav = self.post_speech(serde_json::json!({
            "input": text,
            "voice": voice_name,
            "language": "English",
            "response_format": "wav",
        }))?;
        std::fs::write(out, wav)?;
        Ok(())
    }

    fn post_speech(&self, body: serde_json::Value) -> Result<Vec<u8>> {
        let resp = self
            .http
            .post(format!("{}/v1/audio/speech", self.url))
            .json(&body)
            .send()
            .context("OmniVoice request failed")?;
        let status = resp.status();
        let bytes = resp.bytes()?.to_vec();
        if !status.is_success() || !bytes.starts_with(b"RIFF") {
            bail!("OmniVoice said {status}: {}", String::from_utf8_lossy(&bytes[..bytes.len().min(300)]));
        }
        Ok(bytes)
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn tail(s: &str, n: usize) -> String {
    let lines: Vec<&str> = s.lines().collect();
    lines[lines.len().saturating_sub(n)..].join("\n")
}

fn slug(s: &str) -> String {
    let mut out: String = s
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    while out.contains("--") {
        out = out.replace("--", "-");
    }
    out.trim_matches('-').chars().take(60).collect()
}

fn fxhash(s: &str) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in s.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

fn b64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for c in data.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if c.len() > 1 { T[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if c.len() > 2 { T[n as usize & 63] as char } else { '=' });
    }
    out
}
