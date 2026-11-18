//! In-app video player: ffmpeg decodes frames (BGRA, fixed size) on a background thread,
//! rodio plays the soundtrack (decoded to PCM once), and frames are shown against the audio clock.
//! Playback speed (0.75×–2×): the soundtrack is re-decoded through ffmpeg's `atempo` (pitch kept) and
//! the video decoder drops frames so the screen still updates ~30 times a second.

use gpui::{RenderImage, Window};
use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{sync_channel, Receiver, TryRecvError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const FPS: f32 = 30.0;
const RATE: u32 = 48_000;

struct Pcm {
    data: Arc<Vec<i16>>,
    pos: usize,
}

impl Iterator for Pcm {
    type Item = i16;
    fn next(&mut self) -> Option<i16> {
        let s = self.data.get(self.pos).copied();
        self.pos += 1;
        s
    }
}

impl rodio::Source for Pcm {
    fn current_frame_len(&self) -> Option<usize> {
        None
    }
    fn channels(&self) -> u16 {
        2
    }
    fn sample_rate(&self) -> u32 {
        RATE
    }
    fn total_duration(&self) -> Option<Duration> {
        None
    }
}

struct Decoder {
    rx: Receiver<(f32, Vec<u8>)>,
    child: Arc<Mutex<Option<std::process::Child>>>,
}

impl Drop for Decoder {
    fn drop(&mut self) {
        if let Some(mut c) = self.child.lock().unwrap().take() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }
}

pub const SPEEDS: [f32; 5] = [0.75, 1.0, 1.25, 1.5, 2.0];

fn speed_key(s: f32) -> u32 {
    (s * 100.0).round() as u32
}

/// Media frames per second to decode at `speed` (≤ 30 shown per second).
fn media_fps(speed: f32) -> f32 {
    if speed > 1.0 { FPS / speed } else { FPS }
}

fn start_decoder(ff: &str, path: &Path, t: f32, w: u32, h: u32, speed: f32) -> Decoder {
    let fps = media_fps(speed);
    let (tx, rx) = sync_channel::<(f32, Vec<u8>)>(6);
    let child = stickstudy::command(ff)
        .args(["-v", "error", "-ss", &format!("{t:.3}"), "-i"])
        .arg(path)
        .args(["-an", "-vf", &format!("fps={fps},scale={w}:{h}:flags=bilinear"), "-f", "rawvideo", "-pix_fmt", "bgra", "-"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .ok();
    let child = Arc::new(Mutex::new(child));
    let stdout = child.lock().unwrap().as_mut().and_then(|c| c.stdout.take());
    if let Some(mut out) = stdout {
        std::thread::spawn(move || {
            let size = (w * h * 4) as usize;
            let mut i = 0u32;
            loop {
                let mut buf = vec![0u8; size];
                if out.read_exact(&mut buf).is_err() {
                    break;
                }
                if tx.send((t + i as f32 / fps, buf)).is_err() {
                    break;
                }
                i += 1;
            }
        });
    }
    Decoder { rx, child }
}

pub struct Player {
    pub path: PathBuf,
    ff: String,
    pub w: u32,
    pub h: u32,
    /// seconds; known once the soundtrack is decoded (else the info.json guess)
    pub duration: f32,
    /// soundtracks by speed key (100 = 1×); the 1× one gives the true duration
    audio: Arc<Mutex<HashMap<u32, Arc<Vec<i16>>>>>,
    decoding: Arc<Mutex<HashSet<u32>>>,
    /// playback speed (1.0 = normal)
    pub speed: f32,
    out: Option<(rodio::OutputStream, rodio::OutputStreamHandle)>,
    sink: Option<rodio::Sink>,
    /// media time where the current clock run started
    base: f32,
    /// wall clock fallback when there's no audio device
    started: Option<Instant>,
    pub playing: bool,
    pub ended: bool,
    want_play: bool,
    decoder: Option<Decoder>,
    pending: Option<(f32, Vec<u8>)>,
    pub frame: Option<Arc<RenderImage>>,
    stale: Vec<Arc<RenderImage>>,
    /// show the next decoded frame even when paused (after a seek)
    need_frame: bool,
    pub volume: f32,
}

impl Player {
    pub fn open(path: PathBuf, ff: String, duration_hint: f32, autoplay: bool, speed: f32) -> Player {
        let vertical = path.file_name().map(|n| n.to_string_lossy().contains("vertical")).unwrap_or(false);
        let (w, h) = if vertical { (720, 1280) } else { (1280, 720) };
        let speed = if SPEEDS.iter().any(|s| (s - speed).abs() < 0.01) { speed } else { 1.0 };
        let out = rodio::OutputStream::try_default().ok();
        let mut p = Player {
            decoder: Some(start_decoder(&ff, &path, 0.0, w, h, speed)),
            path,
            ff,
            w,
            h,
            duration: duration_hint,
            audio: Arc::new(Mutex::new(HashMap::new())),
            decoding: Arc::new(Mutex::new(HashSet::new())),
            speed,
            out,
            sink: None,
            base: 0.0,
            started: None,
            playing: false,
            ended: false,
            want_play: autoplay,
            pending: None,
            frame: None,
            stale: vec![],
            need_frame: true,
            volume: 1.0,
        };
        p.duration = p.duration.max(0.0);
        // the 1× soundtrack (true duration), and the one for the chosen speed
        p.decode_audio(1.0);
        p.decode_audio(speed);
        p
    }

    /// Decode the soundtrack at `speed` in the background (once).
    fn decode_audio(&self, speed: f32) {
        let key = speed_key(speed);
        if self.audio.lock().unwrap().contains_key(&key) || !self.decoding.lock().unwrap().insert(key) {
            return;
        }
        let (audio, decoding, ff, path) = (self.audio.clone(), self.decoding.clone(), self.ff.clone(), self.path.clone());
        std::thread::spawn(move || {
            let mut cmd = stickstudy::command(&ff);
            cmd.args(["-v", "error", "-i"]).arg(&path).arg("-vn");
            if key != 100 {
                cmd.args(["-af", &format!("atempo={speed}")]);
            }
            let out = cmd
                .args(["-f", "s16le", "-ac", "2", "-ar", &RATE.to_string(), "-"])
                .stdin(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .output();
            let pcm: Vec<i16> = match out {
                Ok(o) => o.stdout.chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]])).collect(),
                Err(_) => vec![],
            };
            audio.lock().unwrap().insert(key, Arc::new(pcm));
            decoding.lock().unwrap().remove(&key);
        });
    }

    /// The soundtrack for the current speed, once decoded.
    fn pcm(&self) -> Option<Arc<Vec<i16>>> {
        self.audio.lock().unwrap().get(&speed_key(self.speed)).cloned()
    }

    /// Change the playback speed, keeping the position (and playing if it was).
    pub fn set_speed(&mut self, speed: f32) {
        if (speed - self.speed).abs() < 0.001 {
            return;
        }
        let t = self.time();
        let was = self.playing || self.want_play;
        self.speed = speed;
        // keep only the 1× soundtrack and the new one (they're big)
        let keep = [100, speed_key(speed)];
        self.audio.lock().unwrap().retain(|k, _| keep.contains(k));
        self.decode_audio(speed);
        self.playing = false;
        self.seek(t);
        if was {
            self.play();
        }
    }

    pub fn ready(&self) -> bool {
        self.audio.lock().unwrap().contains_key(&100)
    }

    /// Waiting for the soundtrack of the current speed.
    pub fn buffering(&self) -> bool {
        self.want_play && self.pcm().is_none()
    }

    /// Current media time.
    pub fn time(&self) -> f32 {
        if !self.playing {
            return self.base;
        }
        let run = match (&self.sink, self.started) {
            (Some(s), _) => s.get_pos().as_secs_f32(),
            (None, Some(st)) => st.elapsed().as_secs_f32(),
            _ => 0.0,
        };
        // the soundtrack is time-stretched: one second of it is `speed` seconds of video
        (self.base + run * self.speed).min(self.duration.max(0.001))
    }

    pub fn play(&mut self) {
        if self.ended {
            self.seek(0.0);
            self.ended = false;
        }
        let Some(pcm) = self.pcm() else {
            self.want_play = true;
            return;
        };
        self.start_audio(&pcm);
        self.playing = true;
        self.want_play = false;
    }

    fn start_audio(&mut self, pcm: &Arc<Vec<i16>>) {
        self.sink = None;
        self.started = Some(Instant::now());
        if let Some((_, handle)) = &self.out {
            if let Ok(sink) = rodio::Sink::try_new(handle) {
                let start = ((self.base / self.speed * RATE as f32) as usize * 2).min(pcm.len());
                sink.set_volume(self.volume);
                sink.append(Pcm { data: pcm.clone(), pos: start });
                self.sink = Some(sink);
            }
        }
    }

    pub fn pause(&mut self) {
        self.base = self.time();
        self.playing = false;
        self.want_play = false;
        self.sink = None;
        self.started = None;
    }

    pub fn toggle(&mut self) {
        if self.playing {
            self.pause()
        } else {
            self.play()
        }
    }

    pub fn seek(&mut self, t: f32) {
        let t = t.clamp(0.0, (self.duration - 0.05).max(0.0));
        let was = self.playing;
        self.playing = false;
        self.sink = None;
        self.base = t;
        self.ended = false;
        self.pending = None;
        self.decoder = Some(start_decoder(&self.ff, &self.path, t, self.w, self.h, self.speed));
        self.need_frame = true;
        if was {
            self.play();
        }
    }

    pub fn set_volume(&mut self, v: f32) {
        self.volume = v.clamp(0.0, 1.0);
        if let Some(s) = &self.sink {
            s.set_volume(self.volume);
        }
    }

    /// Advance: pick up the soundtrack when it's ready, swap in the frame that's due.
    /// Returns true if something changed (redraw needed).
    pub fn tick(&mut self, window: &mut Window) -> bool {
        for img in self.stale.drain(..) {
            let _ = window.drop_image(img);
        }
        let mut changed = false;
        let one = self.audio.lock().unwrap().get(&100).cloned();
        if let Some(pcm) = one {
            let real = pcm.len() as f32 / (RATE as f32 * 2.0);
            if real > 0.0 && (self.duration - real).abs() > 0.01 {
                self.duration = real;
                changed = true;
            }
        }
        if self.pcm().is_some() {
            if self.want_play {
                self.play();
                changed = true;
            }
        }
        let now = self.time();
        let mut newest: Option<Vec<u8>> = None;
        loop {
            if self.pending.is_none() {
                match self.decoder.as_ref().map(|d| d.rx.try_recv()) {
                    Some(Ok(f)) => self.pending = Some(f),
                    Some(Err(TryRecvError::Empty)) | None => break,
                    Some(Err(TryRecvError::Disconnected)) => break,
                }
            }
            let due = self.pending.as_ref().map(|f| f.0 <= now + 0.5 / media_fps(self.speed)).unwrap_or(false);
            if due || self.need_frame {
                let (_, buf) = self.pending.take().unwrap();
                newest = Some(buf);
                self.need_frame = false;
                if !self.playing {
                    break;
                }
            } else {
                break;
            }
        }
        if let Some(buf) = newest {
            if let Some(img) = image::RgbaImage::from_raw(self.w, self.h, buf) {
                let frame = Arc::new(RenderImage::new(vec![image::Frame::new(img)]));
                if let Some(old) = self.frame.replace(frame) {
                    self.stale.push(old);
                }
                changed = true;
            }
        }
        if self.playing && self.duration > 0.0 && now >= self.duration - 0.04 {
            self.base = self.duration;
            self.playing = false;
            self.sink = None;
            self.ended = true;
            changed = true;
        }
        changed
    }

    pub fn release(&mut self, window: &mut Window) {
        for img in self.stale.drain(..) {
            let _ = window.drop_image(img);
        }
        if let Some(f) = self.frame.take() {
            let _ = window.drop_image(f);
        }
        self.sink = None;
        self.decoder = None;
    }
}

pub fn fmt_time(t: f32) -> String {
    let s = t.max(0.0) as u32;
    format!("{}:{:02}", s / 60, s % 60)
}
