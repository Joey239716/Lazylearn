//! Lays a voiced lesson out in time (intro, chapter cards, shots, transitions,
//! captions, sound-effect cues) and renders any frame of it. `Renderer::frame`
//! is a pure function of time, so chunks of the video can render in parallel.

use crate::character::{Actor, Look};
use crate::gfx::{clamp01, ease_back, ease_in_out, ease_out, hex, prog, rnd, Align, Font, Gfx};
use crate::lesson::{Lesson, Pose, Shot, ShotKind, Speaker};
use crate::shots::{self, Cast, Ctx, Sfx, R};
use crate::styles::{self, Theme};
use std::path::{Path, PathBuf};

const INTRO: f32 = 2.8;
const CHAPTER: f32 = 1.55;
const OUTRO: f32 = 4.0;
const GAP: f32 = 0.16;
const SCENE_TAIL: f32 = 0.35;
const LEAD: f32 = 0.12;
const TRANS: f32 = 0.38;
const MIN_SHOT: f32 = 0.7;
pub const ENV_HZ: f32 = 30.0;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Trans {
    None,
    WhipLeft,
    WhipUp,
    Zoom,
    PopSwap,
    Circle,
}

#[derive(Clone, Debug)]
pub enum Vis {
    Intro,
    Chapter { n: usize, heading: String },
    Outro,
    Shot(Shot),
}

#[derive(Clone, Debug)]
pub struct Seg {
    pub vis: Vis,
    pub start: f32,
    pub end: f32,
    pub beats: Vec<f32>,
    pub trans: Trans,
    pub seed: u32,
    pub scene: usize,
}

#[derive(Clone, Debug)]
pub struct Word {
    pub text: String,
    pub t0: f32,
    pub t1: f32,
}

#[derive(Clone, Debug)]
pub struct LineSeg {
    pub speaker: Speaker,
    pub start: f32,
    pub end: f32,
    pub pose: Pose,
    pub listener_pose: Pose,
    pub words: Vec<Word>,
    /// mouth openness sampled at ENV_HZ from line start
    pub env: Vec<f32>,
    pub audio: Option<PathBuf>,
    pub audio_start: f32,
}

#[derive(Clone, Debug)]
pub struct Caption {
    pub words: Vec<Word>,
    pub t0: f32,
    pub t1: f32,
}

pub struct Layout {
    pub stage: R,
    pub full: R,
    pub presenter: (f32, f32, f32),
    pub peek: (f32, f32, f32),
    pub caption_y: f32,
    pub caption_size: f32,
}

pub fn layout(w: f32, h: f32) -> Layout {
    if w >= h {
        Layout {
            stage: R { x: 430.0, y: 70.0, w: w - 430.0 - 60.0, h: h * 0.78 },
            full: R { x: 90.0, y: 70.0, w: w - 180.0, h: h * 0.78 },
            presenter: (225.0, h * 0.85, 1.22),
            peek: (w - 160.0, h + 150.0, 1.1),
            caption_y: h - 92.0,
            caption_size: 58.0,
        }
    } else {
        Layout {
            stage: R { x: 50.0, y: 230.0, w: w - 100.0, h: h * 0.5 },
            full: R { x: 50.0, y: 230.0, w: w - 100.0, h: h * 0.5 },
            presenter: (w * 0.27, h * 0.86, 1.15),
            peek: (w - 160.0, h + 120.0, 1.0),
            caption_y: h * 0.72,
            caption_size: 62.0,
        }
    }
}

pub struct Timeline {
    pub w: f32,
    pub h: f32,
    pub th: Theme,
    pub title: String,
    pub duo: bool,
    pub segs: Vec<Seg>,
    pub lines: Vec<LineSeg>,
    pub captions: Vec<Caption>,
    pub sfx: Vec<(f32, Sfx)>,
    pub scene_marks: Vec<(f32, String)>,
    pub total: f32,
    pub n_quiz: usize,
}

/// Estimated word timings inside a spoken line (no speech recognizer needed):
/// time is shared out by word length, with extra weight for punctuation pauses.
fn word_times(text: &str, t0: f32, dur: f32) -> Vec<Word> {
    let words: Vec<&str> = text.split_whitespace().collect();
    let weight = |w: &str| {
        let n = w.chars().filter(|c| c.is_alphanumeric()).count() as f32 + 2.5;
        let pause = if w.ends_with(['.', '!', '?']) { 4.0 } else if w.ends_with([',', ';', ':']) { 2.5 } else { 0.0 };
        (n, pause)
    };
    let total: f32 = words.iter().map(|w| { let (a, b) = weight(w); a + b }).sum::<f32>().max(1.0);
    let mut acc = 0.0;
    words
        .iter()
        .map(|w| {
            let (n, pause) = weight(w);
            let a = t0 + dur * acc / total;
            let b = t0 + dur * (acc + n) / total;
            acc += n + pause;
            Word { text: w.to_string(), t0: a, t1: b }
        })
        .collect()
}

/// Mouth-openness envelope from a WAV (RMS per frame, normalized).
fn envelope(path: &Path, start: f32, dur: f32) -> Vec<f32> {
    let Ok(mut r) = hound::WavReader::open(path) else { return vec![] };
    let spec = r.spec();
    let samples: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Int => {
            let max = (1i64 << (spec.bits_per_sample - 1)) as f32;
            r.samples::<i32>().filter_map(|s| s.ok()).map(|s| s as f32 / max).collect()
        }
        hound::SampleFormat::Float => r.samples::<f32>().filter_map(|s| s.ok()).collect(),
    };
    let ch = spec.channels.max(1) as usize;
    let sr = spec.sample_rate as f32;
    let win = (sr / ENV_HZ) as usize * ch;
    let s0 = (start * sr) as usize * ch;
    let n = (dur * ENV_HZ).ceil() as usize + 1;
    let mut env: Vec<f32> = (0..n)
        .map(|i| {
            let a = (s0 + i * win).min(samples.len());
            let b = (a + win).min(samples.len());
            if b <= a {
                return 0.0;
            }
            (samples[a..b].iter().map(|x| x * x).sum::<f32>() / (b - a) as f32).sqrt()
        })
        .collect();
    let mut sorted = env.clone();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let p90 = sorted.get(sorted.len() * 9 / 10).copied().unwrap_or(1.0).max(1e-4);
    for v in &mut env {
        *v = clamp01(*v / p90).powf(0.7);
    }
    env
}

fn captions_for(words: &[Word], end: f32) -> Vec<Caption> {
    let mut out: Vec<Caption> = vec![];
    let mut cur: Vec<Word> = vec![];
    let mut chars = 0;
    for (i, w) in words.iter().enumerate() {
        cur.push(w.clone());
        chars += w.text.len() + 1;
        let brk = w.text.ends_with(['.', '!', '?', ',', ';', ':']);
        let last = i + 1 == words.len();
        if cur.len() >= 4 || chars > 22 || brk || last {
            let t0 = cur[0].t0;
            out.push(Caption { words: std::mem::take(&mut cur), t0, t1: 0.0 });
            chars = 0;
        }
    }
    for i in 0..out.len() {
        out[i].t1 = if i + 1 < out.len() { out[i + 1].t0 } else { end + 0.25 };
    }
    out
}

impl Timeline {
    pub fn build(lesson: &Lesson, dir: &Path, w: u32, h: u32) -> Timeline {
        let th = lesson.style.theme();
        let duo = lesson.scenes.iter().flat_map(|s| &s.lines).any(|l| l.speaker == Speaker::Student);
        let mut segs: Vec<Seg> = vec![];
        let mut lines: Vec<LineSeg> = vec![];
        let mut scene_marks = vec![];
        let mut pending: Vec<(f32, Shot, usize, bool)> = vec![]; // (start, shot, scene, first_of_scene)
        segs.push(Seg { vis: Vis::Intro, start: 0.0, end: INTRO, beats: vec![], trans: Trans::None, seed: 1, scene: 0 });
        let mut t = INTRO;
        for (si, scene) in lesson.scenes.iter().enumerate() {
            if si > 0 {
                segs.push(Seg {
                    vis: Vis::Chapter { n: si, heading: scene.heading.clone() },
                    start: t,
                    end: t + CHAPTER,
                    beats: vec![],
                    trans: Trans::None,
                    seed: si as u32 * 31,
                    scene: si,
                });
                t += CHAPTER;
            }
            scene_marks.push((t, scene.heading.clone()));
            let scene_start = t;
            for (li, line) in scene.lines.iter().enumerate() {
                let d = line.duration.max(0.6);
                let words = word_times(&line.text, t, d);
                let audio = line.audio.as_ref().map(|a| dir.join(a));
                let env = audio.as_ref().map(|p| envelope(p, line.audio_start, d)).unwrap_or_default();
                let mut shots = line.shots.clone();
                if shots.is_empty() && li == 0 {
                    shots.push(Shot { kind: ShotKind::Talk, ..Default::default() });
                }
                let n = shots.len();
                for (k, shot) in shots.into_iter().enumerate() {
                    let cue = (!shot.cue.is_empty())
                        .then(|| {
                            let key = shot.cue.to_lowercase();
                            words.iter().find(|w| w.text.to_lowercase().trim_matches(|c: char| !c.is_alphanumeric()).starts_with(key.trim())).map(|w| w.t0 - 0.1)
                        })
                        .flatten();
                    let start = if li == 0 && k == 0 {
                        scene_start
                    } else if k == 0 {
                        cue.unwrap_or(t - LEAD)
                    } else {
                        cue.unwrap_or(t + d * k as f32 / n as f32 - LEAD)
                    };
                    pending.push((start, shot, si, li == 0 && k == 0));
                }
                lines.push(LineSeg {
                    speaker: line.speaker,
                    start: t,
                    end: t + d,
                    pose: line.pose,
                    listener_pose: line.listener_pose,
                    words,
                    env,
                    audio,
                    audio_start: line.audio_start,
                });
                t += d + GAP;
            }
            t += SCENE_TAIL;
        }
        let outro_start = t;
        let total = t + OUTRO;

        // turn pending shots into segments: keep order, drop ones that would flash by
        pending.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
        let mut shot_segs: Vec<Seg> = vec![];
        for (start, shot, scene, first) in pending {
            if let Some(prev) = shot_segs.last() {
                if !first && start - prev.start < MIN_SHOT {
                    continue;
                }
            }
            shot_segs.push(Seg {
                vis: Vis::Shot(shot),
                start,
                end: 0.0,
                beats: vec![],
                trans: Trans::None,
                seed: (shot_segs.len() as u32 + 1) * 7919,
                scene,
            });
        }
        segs.extend(shot_segs);
        segs.push(Seg { vis: Vis::Outro, start: outro_start, end: total, beats: vec![], trans: Trans::Zoom, seed: 99, scene: lesson.scenes.len().saturating_sub(1) });
        segs.sort_by(|a, b| a.start.partial_cmp(&b.start).unwrap());
        for i in 0..segs.len() {
            let next = segs.get(i + 1).map(|s| s.start).unwrap_or(total);
            if !matches!(segs[i].vis, Vis::Chapter { .. } | Vis::Intro) {
                segs[i].end = next;
            }
        }

        // transitions + plans + sound cues
        let mut sfx: Vec<(f32, Sfx)> = vec![(0.15, Sfx::Whoosh), (0.55, Sfx::Pop), (outro_start + 0.1, Sfx::Stamp), (outro_start + 0.45, Sfx::Sparkle)];
        let trans_cycle = [Trans::WhipLeft, Trans::PopSwap, Trans::Zoom, Trans::WhipUp, Trans::Circle, Trans::WhipLeft, Trans::PopSwap];
        let mut tc = 0usize;
        for i in 0..segs.len() {
            let prev_vis = if i > 0 { Some(segs[i - 1].vis.clone()) } else { None };
            let seg = &mut segs[i];
            if let Vis::Chapter { .. } = seg.vis {
                sfx.push((seg.start, Sfx::Whoosh));
                sfx.push((seg.start + 0.25, Sfx::Stamp));
                sfx.push((seg.end - 0.3, Sfx::Whoosh));
            }
            let Vis::Shot(shot) = &seg.vis else { continue };
            seg.trans = match &prev_vis {
                Some(Vis::Intro) => Trans::Circle,
                Some(Vis::Shot(p)) if p.kind == ShotKind::Talk && shot.kind == ShotKind::Talk => Trans::None,
                Some(Vis::Shot(_)) => {
                    tc += 1;
                    trans_cycle[(tc + (rnd(seg.seed) * 2.0) as usize) % trans_cycle.len()]
                }
                _ => Trans::None,
            };
            if seg.trans != Trans::None {
                sfx.push((seg.start, if seg.trans == Trans::PopSwap { Sfx::Pop } else { Sfx::Whoosh }));
            }
            let dur = seg.end - seg.start;
            let rel: Vec<(String, f32)> = lines
                .iter()
                .flat_map(|l| &l.words)
                .filter(|w| w.t0 >= seg.start - 0.3 && w.t0 < seg.end)
                .map(|w| (w.text.clone(), w.t0 - seg.start))
                .collect();
            if dur >= 4.2 && shot.kind != ShotKind::Title {
                let mut lt = 2.6;
                while lt < dur - 1.0 {
                    sfx.push((seg.start + lt, Sfx::Swoosh));
                    lt += 2.4;
                }
            }
            let (beats, fx) = shots::plan(shot, dur, &rel);
            let offset = if seg.trans != Trans::None { TRANS * 0.5 } else { 0.0 };
            seg.beats = beats.iter().map(|b| b + offset).collect();
            for (ft, s) in fx {
                if ft + offset < dur {
                    sfx.push((seg.start + ft + offset, s));
                }
            }
        }
        sfx.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
        // thin out effects so they never pile up
        let mut thinned: Vec<(f32, Sfx)> = vec![];
        for (t, s) in sfx {
            if thinned.last().map_or(true, |(lt, _)| t - lt > 0.11) {
                thinned.push((t, s));
            }
        }

        let mut captions = vec![];
        for l in &lines {
            captions.extend(captions_for(&l.words, l.end));
        }
        Timeline {
            w: w as f32,
            h: h as f32,
            th,
            title: lesson.title.clone(),
            duo,
            segs,
            lines,
            captions,
            sfx: thinned,
            scene_marks,
            total,
            n_quiz: lesson.quizzes().len(),
        }
    }

    fn seg_at(&self, t: f32) -> usize {
        match self.segs.binary_search_by(|s| s.start.partial_cmp(&t).unwrap()) {
            Ok(i) => i,
            Err(i) => i.saturating_sub(1),
        }
    }

    fn line_at(&self, t: f32) -> Option<usize> {
        if self.lines.is_empty() || t < self.lines[0].start - 0.2 {
            return None;
        }
        let i = match self.lines.binary_search_by(|l| l.start.partial_cmp(&t).unwrap()) {
            Ok(i) => i,
            Err(i) => i.saturating_sub(1),
        };
        Some(i)
    }

    /// Who is doing what at time t.
    fn cast_at(&self, t: f32) -> Cast {
        let lay = layout(self.w, self.h);
        let mut teacher = Actor::new(if self.duo { Look::Teacher } else { Look::Host }, lay.presenter.0, lay.presenter.1, lay.presenter.2, 1.0);
        let mut student = Actor::new(Look::Student, lay.peek.0, lay.peek.1, lay.peek.2, -1.0);
        let mut speaker_is_student = false;
        if let Some(i) = self.line_at(t) {
            let l = &self.lines[i];
            let prev = if i > 0 { Some(&self.lines[i - 1]) } else { None };
            let pose_for = |line: &LineSeg, student_actor: bool| {
                let speaks = (line.speaker == Speaker::Student) == student_actor;
                if speaks { line.pose } else { line.listener_pose }
            };
            for (a, is_student) in [(&mut teacher, false), (&mut student, true)] {
                a.pose = pose_for(l, is_student);
                a.prev_pose = prev.map(|p| pose_for(p, is_student)).unwrap_or(Pose::Idle);
                a.since = (t - l.start + 0.15).max(0.0);
                let speaks = (l.speaker == Speaker::Student) == is_student;
                a.talking = speaks && t >= l.start && t < l.end;
                if a.talking {
                    let k = (t - l.start) * ENV_HZ;
                    let i0 = k.floor() as usize;
                    let f = k.fract();
                    let v0 = l.env.get(i0).copied();
                    let v1 = l.env.get(i0 + 1).copied().or(v0);
                    a.mouth = match (v0, v1) {
                        (Some(a), Some(b)) => a + (b - a) * f,
                        _ => ((t * 14.0).sin() * 0.5 + 0.5) * 0.8,
                    };
                }
            }
            speaker_is_student = l.speaker == Speaker::Student;
        }
        Cast { duo: self.duo, speaker_is_student, teacher, student }
    }
}

/// Camera punch-ins for shots that stay on screen a long time: (zoom, focus point as fraction of the frame).
fn jump_zoom(kind: ShotKind, lt: f32, dur: f32, seed: u32) -> (f32, (f32, f32)) {
    const FIRST: f32 = 2.6;
    const EVERY: f32 = 2.4;
    if dur < 4.2 || lt < FIRST || kind == ShotKind::Title {
        return (1.0, (0.5, 0.45));
    }
    let k = ((lt - FIRST) / EVERY).floor() as u32;
    // never start a new punch-in in the last second of the shot
    let last_ok = ((dur - 1.0 - FIRST) / EVERY).floor().max(0.0) as u32;
    let k = k.min(last_ok);
    let levels = [1.09f32, 1.0, 1.13, 1.04];
    let focus = [(0.6, 0.42), (0.5, 0.45), (0.62, 0.48), (0.55, 0.4)];
    let off = (seed % 4) as usize;
    let cur = levels[(k as usize + off) % 4];
    let prev = if k == 0 { 1.0 } else { levels[(k as usize + off - 1) % 4] };
    let cf = focus[(k as usize + off) % 4];
    let pf = if k == 0 { (0.5, 0.45) } else { focus[(k as usize + off - 1) % 4] };
    let p = ease_out(clamp01((lt - FIRST - k as f32 * EVERY) / 0.22));
    (prev + (cur - prev) * p, (pf.0 + (cf.0 - pf.0) * p, pf.1 + (cf.1 - pf.1) * p))
}

pub struct Renderer<'a> {
    pub tl: &'a Timeline,
    pub g: Gfx,
    bg: Vec<u8>,
}

impl<'a> Renderer<'a> {
    pub fn new(tl: &'a Timeline) -> Renderer<'a> {
        let bg = styles::background(tl.th.style, tl.w as u32, tl.h as u32);
        Renderer { tl, g: Gfx::new(tl.w as u32, tl.h as u32), bg: bg.pm.data().to_vec() }
    }

    pub fn frame(&mut self, t: f32) {
        self.g.pm.data_mut().copy_from_slice(&self.bg);
        self.g.reset();
        let tl = self.tl;
        let (w, h) = (tl.w, tl.h);
        let k = tl.seg_at(t);
        let seg = &tl.segs[k];
        let lt = t - seg.start;

        // camera: slow push-in + shake on stamps
        self.g.save();
        if let Vis::Shot(shot) = &seg.vis {
            let dur = (seg.end - seg.start).max(0.1);
            let mut z = 1.0 + 0.03 * ease_in_out(lt / dur);
            // long shots get quick "jump zoom" reframes every ~2.4 s to keep the energy up
            let (zoom, (fx, fy)) = jump_zoom(shot.kind, lt, dur, seg.seed);
            z *= zoom;
            self.g.around(w * fx, h * fy, z, 0.0);
        }
        let shake = tl
            .sfx
            .iter()
            .filter(|(st, s)| *s == Sfx::Stamp && t >= *st && t - *st < 0.28)
            .map(|(st, _)| 1.0 - (t - st) / 0.28)
            .fold(0.0, f32::max);
        if shake > 0.0 {
            self.g.translate((t * 90.0).sin() * 14.0 * shake, (t * 77.0).cos() * 10.0 * shake);
        }

        let in_trans = matches!(seg.vis, Vis::Shot(_)) && seg.trans != Trans::None && lt < TRANS && k > 0;
        if in_trans {
            self.transition(k, t, lt / TRANS);
        } else {
            self.draw_seg(k, t, lt);
        }
        self.g.restore();
        self.g.reset();
        self.overlays(t, k);
    }

    fn draw_seg(&mut self, k: usize, t: f32, lt: f32) {
        let tl = self.tl;
        let seg = &tl.segs[k];
        let th = tl.th;
        let (w, h) = (tl.w, tl.h);
        let mut cast = tl.cast_at(t);
        match &seg.vis {
            Vis::Intro => shots::intro(&mut self.g, &th, &tl.title, lt, &mut cast, w, h),
            Vis::Outro => shots::outro(&mut self.g, &th, tl.n_quiz, lt, &mut cast, w, h, seg.seed),
            Vis::Chapter { n, heading } => {
                let dur = seg.end - seg.start;
                // underneath: the previous shot (frozen) while the panel arrives, then the next one
                if lt < dur / 2.0 {
                    if k > 0 && matches!(tl.segs[k - 1].vis, Vis::Shot(_)) {
                        let p = &tl.segs[k - 1];
                        self.draw_seg(k - 1, seg.start - 0.01, p.end - p.start - 0.01);
                    }
                } else if k + 1 < tl.segs.len() {
                    self.draw_seg(k + 1, tl.segs[k + 1].start, 0.0);
                }
                shots::chapter(&mut self.g, &th, *n, heading, lt, dur, w, h, seg.seed);
            }
            Vis::Shot(shot) => {
                let lay = layout(w, h);
                let full = shots::full_stage(shot.kind);
                let stage = if full { lay.full } else { lay.stage };
                if !full {
                    // presenter at the side, facing the stage
                    cast.teacher.x = lay.presenter.0;
                    cast.teacher.floor = lay.presenter.1;
                    cast.teacher.scale = lay.presenter.2;
                    let pres_in = ease_back(prog(lt, 0.0, 0.35));
                    self.g.save();
                    self.g.translate(-(1.0 - pres_in) * 120.0, 0.0);
                    self.g.fade(clamp01(lt * 6.0 + 0.3));
                    cast.teacher.draw(&mut self.g, &th, t);
                    self.g.restore();
                }
                let ctx = Ctx { th: &th, lt, dur: seg.end - seg.start, beats: &seg.beats, seed: seg.seed, stage, w, h };
                shots::draw(&mut self.g, shot, &ctx, &mut cast);
            }
        }
    }

    fn transition(&mut self, k: usize, t: f32, p: f32) {
        let tl = self.tl;
        let seg = &tl.segs[k];
        let prev = &tl.segs[k - 1];
        let (w, h) = (tl.w, tl.h);
        let prev_lt = (t - prev.start).min(prev.end - prev.start + 2.0);
        let cur_lt = t - seg.start;
        match seg.trans {
            Trans::WhipLeft | Trans::WhipUp => {
                let e = ease_in_out(p);
                let horiz = seg.trans == Trans::WhipLeft;
                let (dx, dy) = if horiz { (w, 0.0) } else { (0.0, h) };
                self.g.save();
                self.g.translate(-dx * e, -dy * e);
                self.draw_seg(k - 1, t, prev_lt);
                self.g.restore();
                self.g.save();
                self.g.translate(dx * (1.0 - e), dy * (1.0 - e));
                self.draw_seg(k, t, cur_lt);
                self.g.restore();
                // speed lines
                let s = (p * std::f32::consts::PI).sin();
                for i in 0..9 {
                    let r = rnd(seg.seed + i);
                    let c = tl.th.ink.a(0.18 * s);
                    if horiz {
                        let y = h * (0.08 + 0.84 * r);
                        let x = w * rnd(seg.seed + i + 50);
                        self.g.seg((x, y), (x + 300.0 + 400.0 * r, y), c, 6.0);
                    } else {
                        let x = w * (0.08 + 0.84 * r);
                        let y = h * rnd(seg.seed + i + 50);
                        self.g.seg((x, y), (x, y + 200.0 + 300.0 * r), c, 6.0);
                    }
                }
            }
            Trans::Zoom => {
                if p < 0.5 {
                    self.g.save();
                    let e = (p / 0.5).powi(2);
                    self.g.around(w / 2.0, h / 2.0, 1.0 + e * 1.6, 0.0);
                    self.g.fade(1.0 - e);
                    self.draw_seg(k - 1, t, prev_lt);
                    self.g.restore();
                }
                if p > 0.3 {
                    let q = (p - 0.3) / 0.7;
                    self.g.save();
                    self.g.around(w / 2.0, h / 2.0, 0.6 + 0.4 * ease_back(q), 0.0);
                    self.g.fade(q * 2.0);
                    self.draw_seg(k, t, cur_lt);
                    self.g.restore();
                }
            }
            Trans::PopSwap => {
                if p < 0.45 {
                    self.g.save();
                    let e = ease_in_out(p / 0.45);
                    self.g.around(w / 2.0, h / 2.0, 1.0 - e * 0.85, e * 10.0);
                    self.g.fade(1.0 - e);
                    self.draw_seg(k - 1, t, prev_lt);
                    self.g.restore();
                }
                if p > 0.35 {
                    let q = (p - 0.35) / 0.65;
                    self.g.save();
                    self.g.around(w / 2.0, h / 2.0, 0.3 + 0.7 * ease_back(q), (1.0 - ease_out(q)) * -8.0);
                    self.g.fade(q * 3.0);
                    self.draw_seg(k, t, cur_lt);
                    self.g.restore();
                }
            }
            Trans::Circle | Trans::None => {
                let diag = (w * w + h * h).sqrt() / 2.0 + 20.0;
                let col = tl.th.accent((seg.seed % 5) as usize);
                if p < 0.5 {
                    self.draw_seg(k - 1, t, prev_lt);
                    let r = diag * ease_in_out(p / 0.5);
                    self.g.circle(w / 2.0, h / 2.0, r, Some(col), Some((tl.th.ink, 8.0)));
                } else {
                    self.draw_seg(k, t, cur_lt);
                    let r = diag * ease_in_out((p - 0.5) / 0.5);
                    let mut pb = tiny_skia::PathBuilder::new();
                    pb.push_circle(w / 2.0, h / 2.0, diag + 10.0);
                    pb.push_circle(w / 2.0, h / 2.0, r.max(0.5));
                    if let Some(path) = pb.finish() {
                        self.g.fill_eo(&path, col);
                    }
                    self.g.circle(w / 2.0, h / 2.0, r, None, Some((tl.th.ink, 8.0)));
                }
            }
        }
    }

    fn overlays(&mut self, t: f32, k: usize) {
        let tl = self.tl;
        let th = tl.th;
        let (w, h) = (tl.w, tl.h);
        let lay = layout(w, h);
        let seg = &tl.segs[k];

        // sidekick peeks in from the corner when they talk during a diagram shot
        if tl.duo {
            if let Vis::Shot(shot) = &seg.vis {
                if !shots::full_stage(shot.kind) {
                    if let Some(i) = tl.line_at(t) {
                        let l = &tl.lines[i];
                        if l.speaker == Speaker::Student {
                            let inn = ease_back(prog(t, l.start - 0.25, 0.35));
                            let out = ease_in_out(prog(t, l.end + 0.05, 0.3));
                            let vis = inn * (1.0 - out);
                            if vis > 0.0 {
                                let mut cast = tl.cast_at(t);
                                cast.student.x = lay.peek.0;
                                cast.student.floor = lay.peek.1 + (1.0 - vis) * 420.0;
                                cast.student.scale = lay.peek.2;
                                cast.student.facing = -1.0;
                                cast.student.draw(&mut self.g, &th, t);
                            }
                        }
                    }
                }
            }
        }

        // captions: chunky outlined words, the current word pops in gold
        if let Some(c) = tl.captions.iter().find(|c| t >= c.t0 && t < c.t1) {
            let size = lay.caption_size;
            let font = Font::Display;
            let space = self.g.measure(font, " ", size) + 16.0;
            let widths: Vec<f32> = c.words.iter().map(|wd| self.g.measure(font, &wd.text, size)).collect();
            let total: f32 = widths.iter().sum::<f32>() + space * (widths.len() as f32 - 1.0);
            let appear = ease_back(prog(t, c.t0, 0.18));
            let mut x = w / 2.0 - total / 2.0;
            let y = lay.caption_y;
            self.g.save();
            self.g.around(w / 2.0, y, 0.85 + 0.15 * appear, 0.0);
            self.g.fade(appear * 2.0);
            for (wi, (wd, ww)) in c.words.iter().zip(&widths).enumerate() {
                let next = c.words.get(wi + 1).map(|n| n.t0).unwrap_or(wd.t1 + 0.15);
                let active = t >= wd.t0 && t < next;
                let pop = if active { 1.0 + 0.12 * (1.0 - ease_out(prog(t, wd.t0, 0.15))) + 0.04 } else { 1.0 };
                let col = if active { th.gold } else { hex(0xFFFFFF) };
                self.g.save();
                self.g.around(x + ww / 2.0, y, pop, 0.0);
                self.g.text_ex(font, &wd.text, size, x, y, Align::Left, col, Some((hex(0x111111), 10.0)), f32::MAX);
                self.g.restore();
                x += ww + space;
            }
            self.g.restore();
        }

        // progress bar with chapter ticks
        let bar_h = 10.0;
        self.g.rect(0.0, 0.0, w, bar_h, th.bar_track);
        self.g.rect(0.0, 0.0, w * clamp01(t / tl.total), bar_h, th.bar);
        for (mt, _) in tl.scene_marks.iter().skip(1) {
            let x = w * mt / tl.total;
            self.g.rect(x - 2.0, 0.0, 4.0, bar_h, th.ink.a(0.5));
        }
        // current part label
        if let Vis::Shot(_) = seg.vis {
            let si = seg.scene;
            if let Some((_, heading)) = tl.scene_marks.get(si) {
                let label = format!("{} · {}", si + 1, heading);
                let sz = 26.0;
                let tw = self.g.measure(Font::Display, &label, sz);
                let a = clamp01((t - tl.scene_marks[si].0) * 2.0);
                self.g.save();
                self.g.fade(a * 0.9);
                self.g.rrect(24.0, 26.0, tw + 36.0, 46.0, 23.0, Some(th.card.a(0.9)), Some((th.ink, 3.0)));
                self.g.text(Font::Display, &label, sz, 42.0, 49.0, Align::Left, th.text);
                self.g.restore();
            }
        }
    }
}
