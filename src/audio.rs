//! The soundtrack: voice lines, synthesized sound effects, and background music
//! (a generated lo-fi loop, or your own file) that ducks under the voice.

use crate::gfx::rnd;
use crate::shots::Sfx;
use crate::timeline::Timeline;
use anyhow::{Context, Result};
use std::f32::consts::{PI, TAU};
use std::path::Path;

pub const SR: u32 = 48_000;
const SRF: f32 = SR as f32;

struct Rng(u32);
impl Rng {
    fn next(&mut self) -> f32 {
        self.0 = self.0.wrapping_add(1);
        rnd(self.0) * 2.0 - 1.0
    }
}

fn env(i: usize, attack: f32, decay: f32) -> f32 {
    let t = i as f32 / SRF;
    (t / attack.max(1e-4)).min(1.0) * (-t / decay).exp()
}

fn normalize(mut v: Vec<f32>) -> Vec<f32> {
    let m = v.iter().fold(0.0f32, |a, x| a.max(x.abs())).max(1e-6);
    for x in &mut v {
        *x /= m;
    }
    v
}

fn lowpass(x: &[f32], a: impl Fn(usize) -> f32) -> Vec<f32> {
    let mut acc = 0.0;
    x.iter().enumerate().map(|(i, v)| { acc += a(i) * (v - acc); acc }).collect()
}

fn pop(seed: u32) -> Vec<f32> {
    let f0 = 520.0 + rnd(seed) * 260.0;
    let n = (0.09 * SRF) as usize;
    let mut ph = 0.0;
    let mut rng = Rng(seed);
    normalize(
        (0..n)
            .map(|i| {
                let t = i as f32 / SRF;
                let f = f0 * (1.0 + 0.8 * (-t / 0.012).exp());
                ph += TAU * f / SRF;
                let click = if i < 150 { rng.next() * 0.15 * (1.0 - i as f32 / 150.0) } else { 0.0 };
                ((ph).sin() + 0.25 * (2.0 * ph).sin()) * env(i, 0.002, 0.022) + click
            })
            .collect(),
    )
}

fn whoosh(dur: f32, seed: u32, bright: f32) -> Vec<f32> {
    let n = (dur * SRF) as usize;
    let mut rng = Rng(seed);
    let noise: Vec<f32> = (0..n).map(|_| rng.next()).collect();
    let sweep = |i: usize| (PI * i as f32 / n as f32).sin().powf(1.5);
    let hi = lowpass(&noise, |i| 0.02 + bright * sweep(i));
    let lo = lowpass(&noise, |i| (0.02 + bright * sweep(i)) * 0.15);
    normalize(
        hi.iter()
            .zip(&lo)
            .enumerate()
            .map(|(i, (a, b))| (a - b) * (PI * (i as f32 / n as f32).powf(0.7)).sin().powi(2))
            .collect(),
    )
}

fn stamp(seed: u32) -> Vec<f32> {
    let n = (0.35 * SRF) as usize;
    let mut rng = Rng(seed);
    let mut ph = 0.0;
    let noise: Vec<f32> = (0..n).map(|_| rng.next()).collect();
    let thud = lowpass(&noise, |_| 0.05);
    normalize(
        (0..n)
            .map(|i| {
                let t = i as f32 / SRF;
                let f = 70.0 + 120.0 * (-t / 0.03).exp();
                ph += TAU * f / SRF;
                ph.sin() * env(i, 0.001, 0.09) * 1.0 + thud[i] * 6.0 * env(i, 0.0005, 0.04) + noise[i] * 0.3 * env(i, 0.0003, 0.008)
            })
            .collect(),
    )
}

fn ding(f: f32) -> Vec<f32> {
    let n = (0.7 * SRF) as usize;
    normalize(
        (0..n)
            .map(|i| {
                let t = i as f32 / SRF;
                ((TAU * f * t).sin() + 0.35 * (TAU * f * 1.5 * t).sin() + 0.15 * (TAU * f * 2.0 * t).sin()) * env(i, 0.003, 0.18)
            })
            .collect(),
    )
}

fn tick(seed: u32) -> Vec<f32> {
    let n = (0.03 * SRF) as usize;
    let mut rng = Rng(seed);
    let raw: Vec<f32> = (0..n).map(|i| rng.next() * env(i, 0.0005, 0.004)).collect();
    normalize(lowpass(&raw, |_| 0.35))
}

fn sparkle(seed: u32) -> Vec<f32> {
    let n = (0.9 * SRF) as usize;
    let mut out = vec![0.0; n];
    for k in 0..6 {
        let at = (k as f32 * 0.07 * SRF) as usize;
        let f = 1800.0 + rnd(seed + k) * 1800.0;
        for i in 0..(0.3 * SRF) as usize {
            if at + i < n {
                let t = i as f32 / SRF;
                out[at + i] += (TAU * f * t).sin() * env(i, 0.002, 0.06) * (1.0 - k as f32 * 0.12);
            }
        }
    }
    normalize(out)
}

fn count(seed: u32) -> Vec<f32> {
    let n = (1.25 * SRF) as usize;
    let mut out = vec![0.0; n];
    let mut t = 0.0f32;
    let mut k = 0;
    while t < 1.2 {
        let at = (t * SRF) as usize;
        let f = 900.0 + 900.0 * t / 1.2;
        for i in 0..(0.03 * SRF) as usize {
            if at + i < n {
                out[at + i] += (TAU * f * i as f32 / SRF).sin() * env(i, 0.001, 0.008);
            }
        }
        // ticks accelerate then slow as the number settles
        t += 0.035 + 0.09 * (t / 1.2).powi(3);
        k += 1;
    }
    let _ = (seed, k);
    normalize(out)
}

fn typing(seed: u32) -> Vec<f32> {
    let n = (0.5 * SRF) as usize;
    let mut out = vec![0.0; n];
    for k in 0..5 {
        let at = ((k as f32 * 0.09 + rnd(seed + k) * 0.03) * SRF) as usize;
        let c = tick(seed + k * 13);
        for (i, v) in c.iter().enumerate() {
            if at + i < n {
                out[at + i] += v * (0.6 + 0.4 * rnd(seed + k + 7));
            }
        }
    }
    normalize(out)
}

fn effect(s: Sfx, seed: u32) -> (Vec<f32>, f32) {
    match s {
        Sfx::Pop => (pop(seed), 0.16),
        Sfx::Whoosh => (whoosh(0.42, seed, 0.25), 0.12),
        Sfx::Swoosh => (whoosh(0.22, seed, 0.4), 0.07),
        Sfx::Stamp => (stamp(seed), 0.3),
        Sfx::Ding => (ding(1568.0), 0.11),
        Sfx::Tick => (tick(seed), 0.08),
        Sfx::Sparkle => (sparkle(seed), 0.07),
        Sfx::Count => (count(seed), 0.06),
        Sfx::Type => (typing(seed), 0.06),
    }
}

/// A gentle generated lo-fi loop: Cmaj7 - Am7 - Fmaj7 - G7 with keys, bass and soft drums.
pub fn lofi(len: usize) -> Vec<f32> {
    let bpm = 88.0;
    let beat = 60.0 / bpm;
    let bar = beat * 4.0;
    let chords: [[f32; 4]; 4] = [
        [48.0, 52.0, 55.0, 59.0],
        [45.0, 52.0, 55.0, 60.0],
        [41.0, 48.0, 52.0, 57.0],
        [43.0, 50.0, 53.0, 59.0],
    ];
    let hz = |m: f32| 440.0 * 2f32.powf((m - 69.0) / 12.0);
    let mut out = vec![0.0f32; len];
    let mut rng = Rng(4242);
    let total = len as f32 / SRF;
    // keys: each chord struck on beats 1 and 3 (plus a pushed "and of 2")
    let mut t = 0.0;
    let mut bar_i = 0usize;
    while t < total {
        let ch = chords[bar_i % 4];
        for (hit, at) in [(1.0f32, 0.0f32), (0.6, beat * 1.5), (0.85, beat * 2.0)] {
            let start = ((t + at) * SRF) as usize;
            let dur = (beat * 2.2 * SRF) as usize;
            for (k, &m) in ch.iter().enumerate() {
                let f = hz(m + 12.0);
                let det = 1.0 + (k as f32 - 1.5) * 0.0015;
                for i in 0..dur {
                    let j = start + i;
                    if j >= len {
                        break;
                    }
                    let tt = i as f32 / SRF;
                    let e = (tt / 0.01).min(1.0) * (-tt / 0.9).exp();
                    let tone = (TAU * f * det * tt).sin() + 0.3 * (TAU * f * 2.0 * tt).sin() * (-tt / 0.2).exp();
                    out[j] += tone * e * 0.05 * hit;
                }
            }
        }
        // bass
        for (at, m_off) in [(0.0f32, 0.0f32), (beat * 2.5, 7.0)] {
            let start = ((t + at) * SRF) as usize;
            let f = hz(ch[0] - 12.0 + m_off);
            for i in 0..(beat * 1.4 * SRF) as usize {
                let j = start + i;
                if j >= len {
                    break;
                }
                let tt = i as f32 / SRF;
                out[j] += (TAU * f * tt).sin() * (tt / 0.01).min(1.0) * (-tt / 0.6).exp() * 0.16;
            }
        }
        // drums: kick 1 & 3-and, snare 2 & 4, swung hats
        for b in 0..4 {
            let bt = t + b as f32 * beat;
            if b == 0 || b == 2 {
                let at = if b == 2 { bt + beat * 0.5 } else { bt };
                let start = (at * SRF) as usize;
                let mut ph = 0.0;
                for i in 0..(0.25 * SRF) as usize {
                    let j = start + i;
                    if j >= len {
                        break;
                    }
                    let tt = i as f32 / SRF;
                    ph += TAU * (50.0 + 90.0 * (-tt / 0.03).exp()) / SRF;
                    out[j] += ph.sin() * (-tt / 0.12).exp() * 0.32;
                }
            }
            if b == 1 || b == 3 {
                let start = (bt * SRF) as usize;
                let mut acc = 0.0;
                for i in 0..(0.2 * SRF) as usize {
                    let j = start + i;
                    if j >= len {
                        break;
                    }
                    let tt = i as f32 / SRF;
                    acc += 0.3 * (rng.next() - acc);
                    out[j] += acc * (-tt / 0.06).exp() * 0.22;
                }
            }
            for hh in [0.0, 0.62] {
                let start = ((bt + beat * hh) * SRF) as usize;
                let mut prev = 0.0;
                for i in 0..(0.04 * SRF) as usize {
                    let j = start + i;
                    if j >= len {
                        break;
                    }
                    let tt = i as f32 / SRF;
                    let n = rng.next();
                    out[j] += (n - prev) * (-tt / 0.012).exp() * 0.06;
                    prev = n;
                }
            }
        }
        t += bar;
        bar_i += 1;
    }
    // vinyl warmth: gentle low-pass + crackle
    let mut acc = 0.0;
    for (i, v) in out.iter_mut().enumerate() {
        acc += 0.35 * (*v - acc);
        *v = acc;
        if rnd(i as u32 ^ 0xC0FFEE) > 0.99985 {
            *v += 0.04 * rng.next();
        }
    }
    out
}

/// Decode any audio file to mono f32 at SR using ffmpeg.
pub fn decode(ffmpeg: &str, path: &Path) -> Result<Vec<f32>> {
    let out = crate::command(ffmpeg)
        .args(["-v", "error", "-i"])
        .arg(path)
        .args(["-ac", "1", "-ar", &SR.to_string(), "-f", "f32le", "-"])
        .output()
        .with_context(|| format!("running ffmpeg to read {}", path.display()))?;
    if !out.status.success() {
        anyhow::bail!("ffmpeg couldn't read {}: {}", path.display(), String::from_utf8_lossy(&out.stderr));
    }
    Ok(out.stdout.chunks_exact(4).map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]])).collect())
}

fn read_wav(path: &Path) -> Option<(Vec<f32>, u32)> {
    let mut r = hound::WavReader::open(path).ok()?;
    let spec = r.spec();
    let ch = spec.channels.max(1) as usize;
    let raw: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Int => {
            let max = (1i64 << (spec.bits_per_sample - 1)) as f32;
            r.samples::<i32>().filter_map(|s| s.ok()).map(|s| s as f32 / max).collect()
        }
        hound::SampleFormat::Float => r.samples::<f32>().filter_map(|s| s.ok()).collect(),
    };
    let mono: Vec<f32> = raw.chunks(ch).map(|c| c.iter().sum::<f32>() / ch as f32).collect();
    Some((mono, spec.sample_rate))
}

fn resample(x: &[f32], from: u32) -> Vec<f32> {
    if from == SR {
        return x.to_vec();
    }
    let ratio = from as f64 / SR as f64;
    let n = (x.len() as f64 / ratio) as usize;
    (0..n)
        .map(|i| {
            let p = i as f64 * ratio;
            let j = p as usize;
            let f = (p - j as f64) as f32;
            let a = x.get(j).copied().unwrap_or(0.0);
            let b = x.get(j + 1).copied().unwrap_or(a);
            a + (b - a) * f
        })
        .collect()
}

fn place(buf: &mut [f32], t: f32, x: &[f32], gain: f32) {
    let start = (t.max(0.0) * SRF) as usize;
    for (i, v) in x.iter().enumerate() {
        if let Some(o) = buf.get_mut(start + i) {
            *o += v * gain;
        } else {
            break;
        }
    }
}

pub struct MixOptions<'a> {
    pub ffmpeg: &'a str,
    /// None = generated lo-fi loop; Some(path) = your own track
    pub music: Option<&'a Path>,
    pub no_music: bool,
    pub no_sfx: bool,
    pub music_db: f32,
    pub until: f32,
}

pub fn mix(tl: &Timeline, o: &MixOptions) -> Result<Vec<f32>> {
    let end = tl.total.min(o.until);
    let n = (end * SRF) as usize + SR as usize;
    let mut voice = vec![0.0f32; n];
    for l in &tl.lines {
        let Some(p) = &l.audio else { continue };
        let Some((x, sr)) = read_wav(p) else { continue };
        let x = resample(&x, sr);
        let s0 = (l.audio_start * SRF) as usize;
        let s1 = (s0 + ((l.end - l.start + 0.08) * SRF) as usize).min(x.len());
        if s0 >= s1 {
            continue;
        }
        let clip = &x[s0..s1];
        // even out loudness between lines
        let rms = (clip.iter().map(|v| v * v).sum::<f32>() / clip.len() as f32).sqrt().max(1e-4);
        let g = (0.12 / rms).min(4.0);
        let fade = (0.01 * SRF) as usize;
        let shaped: Vec<f32> = clip
            .iter()
            .enumerate()
            .map(|(i, v)| v * g * (i as f32 / fade as f32).min(1.0) * ((clip.len() - i) as f32 / fade as f32).min(1.0))
            .collect();
        place(&mut voice, l.start, &shaped, 1.0);
    }
    let mut out = voice.clone();
    if !o.no_sfx {
        for (i, (t, s)) in tl.sfx.iter().enumerate() {
            if *t < end {
                let (x, g) = effect(*s, i as u32 * 101 + 7);
                place(&mut out, *t, &x, g);
            }
        }
    }
    if !o.no_music {
        let music = match o.music {
            Some(p) => {
                let m = decode(o.ffmpeg, p)?;
                if m.is_empty() {
                    lofi(n)
                } else {
                    // loop the track to length with a short crossfade
                    let mut v = Vec::with_capacity(n);
                    while v.len() < n {
                        v.extend_from_slice(&m[..m.len().min(n - v.len())]);
                    }
                    v
                }
            }
            None => lofi(n),
        };
        // level relative to the voice, ducking a bit more while someone talks
        let win = (0.3 * SRF) as usize;
        let mut talking = vec![false; n];
        let mut acc = 0.0f32;
        for i in 0..n {
            acc += voice[i].abs();
            if i >= win {
                acc -= voice[i - win].abs();
            }
            talking[i] = acc / win as f32 > 0.01;
        }
        let v_rms = {
            let (s, c) = voice.iter().zip(&talking).filter(|(_, t)| **t).fold((0.0f64, 0usize), |(s, c), (v, _)| (s + (*v as f64).powi(2), c + 1));
            if c > 0 { (s / c as f64).sqrt() as f32 } else { 0.1 }
        };
        let m_rms = (music.iter().map(|v| v * v).sum::<f32>() / music.len().max(1) as f32).sqrt().max(1e-6);
        let g = v_rms / m_rms * 10f32.powf(o.music_db / 20.0);
        let mut duck = 1.0f32;
        let fi = 1.5 * SRF;
        let fo = 3.5 * SRF;
        for i in 0..n.min(music.len()) {
            let target = if talking[i] { 0.62 } else { 1.0 };
            duck += (target - duck) * 0.00006;
            let fade_in = (i as f32 / fi).min(1.0);
            let fade_out = (((end * SRF) - i as f32) / fo).clamp(0.0, 1.0);
            out[i] += music[i] * g * duck * fade_in * fade_out;
        }
    }
    out.truncate((end * SRF) as usize);
    Ok(out)
}

pub fn write_wav(path: &Path, x: &[f32]) -> Result<()> {
    let spec = hound::WavSpec { channels: 1, sample_rate: SR, bits_per_sample: 16, sample_format: hound::SampleFormat::Int };
    let mut w = hound::WavWriter::create(path, spec)?;
    // voice sits around -18 dBFS RMS; a soft limiter catches effect peaks without pumping
    for &v in x {
        let a = v.abs();
        let y = if a > 0.85 { v.signum() * (0.85 + 0.14 * ((a - 0.85) / 0.14).tanh()) } else { v };
        w.write_sample((y * 32767.0).clamp(-32768.0, 32767.0) as i16)?;
    }
    w.finalize()?;
    Ok(())
}
