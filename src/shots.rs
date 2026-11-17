//! Shot choreography. For every shot kind:
//!  - `plan` decides WHEN each element appears ("beats"), synced to the narration
//!    word that mentions it when possible, and which sound effect goes with it;
//!  - `draw` renders the shot at a local time, animating elements in from their beats.
//! Planning and drawing share the beat list, so visuals and sound effects stay in sync.

use crate::character::{Actor, Look};
use crate::gfx::{
    clamp01, ease_back, ease_in_out, ease_out, elastic, hex, prog, rnd, Align, Col, Font, Gfx,
};
use crate::icons;
use crate::lesson::{Pose, Shot, ShotKind};
use crate::styles::Theme;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Sfx {
    Pop,
    Whoosh,
    Swoosh,
    Stamp,
    Ding,
    Tick,
    Sparkle,
    Count,
    Type,
}

#[derive(Clone, Copy, Debug)]
pub struct R {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}
impl R {
    pub fn cx(&self) -> f32 {
        self.x + self.w / 2.0
    }
    pub fn cy(&self) -> f32 {
        self.y + self.h / 2.0
    }
}

/// Shot kinds that use the whole stage (no presenter at the side).
pub fn full_stage(k: ShotKind) -> bool {
    matches!(k, ShotKind::Talk | ShotKind::Title | ShotKind::Mnemonic | ShotKind::Timeline)
}

// ------------------------------------------------------------------ planning

const STOP: &[&str] = &["the", "a", "an", "of", "and", "to", "in", "on", "for", "is", "are", "its", "it", "your", "with", "by"];

fn norm(s: &str) -> String {
    s.to_lowercase().chars().filter(|c| c.is_alphanumeric()).collect()
}

/// First time (>= after) that the narration says a word matching `label`.
fn find_word(label: &str, words: &[(String, f32)], after: f32) -> Option<f32> {
    let keys: Vec<String> = label
        .split_whitespace()
        .map(norm)
        .filter(|w| !w.is_empty() && !STOP.contains(&w.as_str()))
        .collect();
    let key = keys.first()?;
    let stem: String = key.chars().take(5.max(key.chars().count().min(5))).collect();
    let stem = if key.chars().count() <= 4 { key.clone() } else { stem };
    words
        .iter()
        .filter(|(_, t)| *t >= after - 0.05)
        .find(|(w, _)| {
            let w = norm(w);
            w.starts_with(&stem) || (w.len() >= 4 && key.starts_with(&w))
        })
        .map(|(_, t)| *t)
}

/// Times for `labels` appearing one after another, synced to the narration when a label is spoken.
fn sync_times(labels: &[String], words: &[(String, f32)], first: f32, dur: f32) -> Vec<f32> {
    let n = labels.len().max(1);
    let span_end = (dur * 0.72).min(first + n as f32 * 1.3).max(first + 0.3);
    let even = |i: usize| first + (span_end - first) * i as f32 / n as f32;
    let mut out = vec![];
    let mut last = first - 0.3;
    for (i, l) in labels.iter().enumerate() {
        let mut t = find_word(l, words, last + 0.1).map(|t| t - 0.12).unwrap_or_else(|| even(i));
        // never earlier than the previous one + a beat, never too late in the shot
        t = t.max(last + 0.28).max(first).min((dur - 0.45).max(first));
        out.push(t);
        last = t;
    }
    out
}

pub fn plan(shot: &Shot, dur: f32, words: &[(String, f32)]) -> (Vec<f32>, Vec<(f32, Sfx)>) {
    let mut beats: Vec<f32> = vec![];
    let mut fx: Vec<(f32, Sfx)> = vec![];
    let items: Vec<String> = shot.items.iter().map(|i| i.label.clone()).collect();
    match shot.kind {
        ShotKind::Talk => {
            beats.push(0.25);
            if !shot.text.is_empty() {
                fx.push((0.25, Sfx::Pop));
            }
        }
        ShotKind::Title | ShotKind::Unknown => {
            let ws: Vec<String> = title_text(shot).split_whitespace().map(String::from).collect();
            let mut last = 0.0f32;
            for (i, w) in ws.iter().enumerate() {
                let quick = 0.08 + i as f32 * 0.13;
                let t = find_word(w, words, last).map(|t| t - 0.08).unwrap_or(quick).max(last + 0.09).min(quick + 0.9);
                beats.push(t);
                last = t;
            }
            for (i, &b) in beats.iter().enumerate() {
                let emph = !shot.emphasis.is_empty() && norm(&ws[i]).contains(&norm(&shot.emphasis));
                fx.push((b, if emph { Sfx::Stamp } else if i == 0 { Sfx::Whoosh } else { Sfx::Tick }));
            }
        }
        ShotKind::Icon => {
            beats = vec![0.1, 0.55, 0.95];
            fx.push((0.1, Sfx::Pop));
            fx.push((0.55, Sfx::Swoosh));
        }
        ShotKind::List | ShotKind::Recap | ShotKind::Flow | ShotKind::Cycle | ShotKind::Hub | ShotKind::Timeline => {
            beats.push(0.05);
            if !shot.title.is_empty() {
                fx.push((0.05, Sfx::Swoosh));
            }
            let ts = sync_times(&items, words, 0.4, dur);
            for &t in &ts {
                if matches!(shot.kind, ShotKind::Flow | ShotKind::Cycle | ShotKind::Timeline) && t > 0.5 {
                    fx.push((t - 0.3, Sfx::Swoosh));
                }
                fx.push((t, if shot.kind == ShotKind::Recap { Sfx::Ding } else { Sfx::Pop }));
            }
            beats.extend(ts);
        }
        ShotKind::Compare => {
            let ts = sync_times(&items, words, 0.1, dur);
            let left = ts.first().copied().unwrap_or(0.1);
            let right = ts.get(1).copied().unwrap_or(left + 0.8);
            let vs = (left + 0.35).min(right - 0.1).max(left + 0.15);
            beats = vec![left, vs, right];
            fx.push((left, Sfx::Whoosh));
            fx.push((vs, Sfx::Stamp));
            fx.push((right, Sfx::Whoosh));
        }
        ShotKind::Definition => {
            let text_t = find_word(&shot.text, words, 0.4).map(|t| t - 0.1).unwrap_or(0.75).clamp(0.6, 1.4);
            beats = vec![0.05, 0.3, text_t];
            fx.push((0.05, Sfx::Whoosh));
            fx.push((0.3, Sfx::Pop));
            fx.push((text_t, Sfx::Type));
        }
        ShotKind::Number => {
            beats = vec![0.15, 1.45];
            fx.push((0.15, Sfx::Count));
            fx.push((1.4, Sfx::Sparkle));
        }
        ShotKind::Formula => {
            let toks = shot.text.split_whitespace().count().max(1);
            let step = ((dur * 0.6) / toks as f32).clamp(0.08, 0.35);
            for i in 0..toks {
                let t = 0.15 + i as f32 * step;
                beats.push(t);
                fx.push((t, Sfx::Tick));
            }
            let end = beats.last().copied().unwrap_or(0.2) + 0.35;
            beats.push(end);
            fx.push((end, Sfx::Swoosh));
        }
        ShotKind::Mnemonic => {
            let n = items.len();
            for i in 0..n {
                let t = 0.08 + i as f32 * 0.14;
                beats.push(t);
                fx.push((t, Sfx::Pop));
            }
            let first = 0.15 + n as f32 * 0.14 + 0.2;
            let ts = sync_times(&items, words, first, dur);
            for &t in &ts {
                fx.push((t, Sfx::Swoosh));
            }
            beats.extend(ts);
        }
        ShotKind::Myth => {
            let fact = find_word(&shot.text, words, 0.9).map(|t| t - 0.1).unwrap_or((dur * 0.5).max(1.3)).clamp(1.2, (dur - 0.6).max(1.2));
            let stamp = (fact * 0.55).clamp(0.55, 1.6).min(fact - 0.3).max(0.4);
            beats = vec![0.05, stamp, fact];
            fx.push((0.05, Sfx::Whoosh));
            fx.push((stamp, Sfx::Stamp));
            fx.push((fact, Sfx::Ding));
        }
        ShotKind::Question => {
            beats = vec![0.1, 0.35];
            fx.push((0.1, Sfx::Pop));
        }
    }
    (beats, fx)
}

fn title_text(shot: &Shot) -> String {
    for s in [&shot.text, &shot.title, &shot.label] {
        if !s.trim().is_empty() {
            return s.trim().to_string();
        }
    }
    "...".into()
}

// ------------------------------------------------------------------ drawing context

pub struct Cast {
    pub duo: bool,
    pub speaker_is_student: bool,
    pub teacher: Actor,
    pub student: Actor,
}

#[allow(dead_code)]
pub struct Ctx<'a> {
    pub th: &'a Theme,
    /// seconds since the shot started
    pub lt: f32,
    pub dur: f32,
    pub beats: &'a [f32],
    pub seed: u32,
    pub stage: R,
    pub w: f32,
    pub h: f32,
}

impl Ctx<'_> {
    fn beat(&self, i: usize) -> f32 {
        self.beats.get(i).copied().unwrap_or(0.1 + i as f32 * 0.3)
    }
    /// 0..1 progress of element i's entrance.
    fn p(&self, i: usize, dur: f32) -> f32 {
        prog(self.lt, self.beat(i), dur)
    }
    fn accent(&self, i: usize) -> Col {
        self.th.accent(i + (self.seed as usize % 3))
    }
}

#[derive(Clone, Copy)]
#[allow(dead_code)]
pub enum Enter {
    Pop,
    Drop,
    SlideL,
    SlideR,
    SlideUp,
    Slam,
    Fade,
}

fn bounce(x: f32) -> f32 {
    let x = clamp01(x);
    let (n1, d1) = (7.5625, 2.75);
    if x < 1.0 / d1 {
        n1 * x * x
    } else if x < 2.0 / d1 {
        let x = x - 1.5 / d1;
        n1 * x * x + 0.75
    } else if x < 2.5 / d1 {
        let x = x - 2.25 / d1;
        n1 * x * x + 0.9375
    } else {
        let x = x - 2.625 / d1;
        n1 * x * x + 0.984375
    }
}

/// Apply an entrance transform around (cx, cy). Returns false if not visible yet.
/// On true, the caller must call `g.restore()` when done drawing the element.
fn enter(g: &mut Gfx, cx: f32, cy: f32, p: f32, how: Enter) -> bool {
    if p <= 0.0 {
        return false;
    }
    g.save();
    match how {
        Enter::Pop => {
            let s = elastic(p).max(0.0);
            let rot = (1.0 - ease_out(p)) * -14.0;
            g.around(cx, cy, s, rot);
            g.fade(p * 4.0);
        }
        Enter::Drop => {
            let dy = -(1.0 - bounce(p)) * 320.0;
            g.translate(0.0, dy);
            g.fade(p * 5.0);
        }
        Enter::SlideL | Enter::SlideR => {
            let d = if matches!(how, Enter::SlideL) { -1.0 } else { 1.0 };
            let e = ease_back(p);
            g.translate(d * (1.0 - e) * 420.0, 0.0);
            g.around(cx, cy, 1.0, d * (1.0 - ease_out(p)) * 8.0);
            g.fade(p * 3.0);
        }
        Enter::SlideUp => {
            g.translate(0.0, (1.0 - ease_back(p)) * 160.0);
            g.fade(p * 3.0);
        }
        Enter::Slam => {
            let e = ease_out(p);
            let s = 1.0 + (1.0 - e) * 1.8;
            g.around(cx, cy, s, (1.0 - e) * 6.0);
            g.fade(p * 3.5);
        }
        Enter::Fade => g.fade(ease_out(p)),
    }
    true
}

fn bob(lt: f32, i: usize) -> f32 {
    (lt * 2.1 + i as f32 * 1.3).sin() * 5.0
}

// ------------------------------------------------------------------ shared pieces

/// A card with the style's colored offset shadow.
pub fn card(g: &mut Gfx, th: &Theme, x: f32, y: f32, w: f32, h: f32, shadow: Col) {
    let r = 26.0f32.min(h / 2.0);
    if th.shadow_offset > 0.0 {
        g.rrect(x + th.shadow_offset, y + th.shadow_offset, w, h, r, Some(shadow), Some((th.ink, th.line_w)));
    }
    if th.glow {
        g.glow(x + w / 2.0, y + h / 2.0, w.max(h) * 0.7, shadow.a(0.18));
    }
    let edge = if th.glow { shadow } else { th.ink };
    g.rrect(x, y, w, h, r, Some(th.card), Some((edge, th.line_w)));
}

/// A pill-shaped label.
pub fn badge(g: &mut Gfx, th: &Theme, text: &str, cx: f32, cy: f32, size: f32, fill: Col) {
    let tw = g.measure(th.label_font, text, size);
    let (w, h) = (tw + size * 1.1, size * 1.55);
    g.rrect(cx - w / 2.0, cy - h / 2.0, w, h, h / 2.0, Some(fill), Some((th.ink, th.line_w * 0.8)));
    let tc = if luminance(fill) > 0.55 { hex(0x161616) } else { hex(0xFFFFFF) };
    g.text(th.label_font, text, size, cx, cy, Align::Center, tc);
}

fn luminance(c: Col) -> f32 {
    0.299 * c.0 + 0.587 * c.1 + 0.114 * c.2
}

fn body_text(g: &mut Gfx, th: &Theme, text: &str, size: f32, cx: f32, cy: f32, max_w: f32, max_lines: usize, reveal: f32, align: Align) {
    let (sz, lines) = g.fit(th.body_font, text, size, max_w, max_lines);
    let x = match align {
        Align::Left => cx - max_w / 2.0,
        _ => cx,
    };
    g.text_block(th.body_font, &lines, sz, x, cy, align, th.text, None, reveal);
}

/// Icon on a soft disk.
fn icon_disk(g: &mut Gfx, th: &Theme, icon: &str, cx: f32, cy: f32, size: f32, c: Col, t: f32) {
    let r = size / 2.0;
    if th.shadow_offset > 0.0 {
        g.circle(cx + th.shadow_offset * 0.6, cy + th.shadow_offset * 0.6, r, Some(c), Some((th.ink, th.line_w)));
    }
    let fill = if th.dark { th.card } else { c.lighten(0.78) };
    g.circle(cx, cy, r, Some(fill), Some((if th.glow { c } else { th.ink }, th.line_w)));
    icons::draw(g, th, icon, cx, cy, size * 0.68, c, t);
}

/// Rotating sunburst behind hero elements.
fn sunburst(g: &mut Gfx, cx: f32, cy: f32, r: f32, c: Col, t: f32, alpha: f32) {
    g.save();
    g.translate(cx, cy);
    g.rotate(t * 12.0);
    g.fade(alpha);
    let n = 16;
    for i in 0..n {
        if i % 2 == 0 {
            let a0 = std::f32::consts::TAU * i as f32 / n as f32;
            let a1 = std::f32::consts::TAU * (i + 1) as f32 / n as f32;
            g.poly(&[(0.0, 0.0), (a0.cos() * r, a0.sin() * r), (a1.cos() * r, a1.sin() * r)], Some(c), None);
        }
    }
    g.restore();
}

/// Confetti burst; `p` = seconds since the burst.
pub fn confetti(g: &mut Gfx, th: &Theme, cx: f32, cy: f32, p: f32, seed: u32, n: u32) {
    if !(0.0..1.6).contains(&p) {
        return;
    }
    for i in 0..n {
        let a = rnd(seed + i * 7) * std::f32::consts::TAU;
        let v = 380.0 + rnd(seed + i * 7 + 1) * 520.0;
        let x = cx + a.cos() * v * p;
        let y = cy + a.sin() * v * p * 0.8 + 700.0 * p * p;
        let rot = p * 500.0 * (rnd(seed + i * 7 + 2) - 0.5);
        let c = th.accent(i as usize);
        g.save();
        g.translate(x, y);
        g.rotate(rot);
        g.fade(1.0 - (p / 1.6).powi(2));
        if i % 3 == 0 {
            g.circle(0.0, 0.0, 7.0, Some(c), None);
        } else {
            g.rect(-9.0, -4.0, 18.0, 8.0, c);
        }
        g.restore();
    }
}

/// Hand-drawn marker swipe behind a word (highlighter).
fn highlight(g: &mut Gfx, x: f32, y: f32, w: f32, h: f32, c: Col, p: f32) {
    if p <= 0.0 {
        return;
    }
    let ww = w * ease_out(p);
    g.rrect(x, y - h / 2.0, ww, h, h * 0.3, Some(c.a(0.75)), None);
}

// ------------------------------------------------------------------ the shots

pub fn draw(g: &mut Gfx, shot: &Shot, c: &Ctx, cast: &mut Cast) {
    match shot.kind {
        ShotKind::Talk => talk(g, shot, c, cast),
        ShotKind::Title | ShotKind::Unknown => title(g, shot, c),
        ShotKind::Icon => hero_icon(g, shot, c),
        ShotKind::List | ShotKind::Recap => list(g, shot, c),
        ShotKind::Flow => flow(g, shot, c),
        ShotKind::Cycle => cycle(g, shot, c),
        ShotKind::Compare => compare(g, shot, c),
        ShotKind::Hub => hub(g, shot, c),
        ShotKind::Definition => definition(g, shot, c),
        ShotKind::Number => number(g, shot, c),
        ShotKind::Formula => formula(g, shot, c),
        ShotKind::Mnemonic => mnemonic(g, shot, c),
        ShotKind::Myth => myth(g, shot, c),
        ShotKind::Timeline => timeline(g, shot, c),
        ShotKind::Question => question(g, shot, c),
    }
}

fn header(g: &mut Gfx, c: &Ctx, text: &str, y: f32) {
    if text.trim().is_empty() {
        return;
    }
    let p = c.p(0, 0.45);
    let cx = c.stage.cx();
    if enter(g, cx, y, p, Enter::Slam) {
        let th = c.th;
        let (sz, lines) = g.fit(th.label_font, text, 66.0, c.stage.w * 0.9, 1);
        let tw = g.measure(th.label_font, &lines[0], sz);
        highlight(g, cx - tw / 2.0 - 16.0, y + sz * 0.22, tw + 32.0, sz * 0.5, th.gold, prog(c.lt, c.beat(0) + 0.25, 0.4));
        g.text(th.label_font, &lines[0], sz, cx, y, Align::Center, th.text);
        g.restore();
    }
}

fn talk(g: &mut Gfx, shot: &Shot, c: &Ctx, cast: &mut Cast) {
    let th = c.th;
    let floor = c.h * 0.86;
    // soft spotlight floor
    g.ellipse(c.w / 2.0, floor + 8.0, c.w * 0.36, 40.0, Some(th.ink.a(0.05)), None);
    let speaker_x;
    if cast.duo {
        cast.teacher.x = c.w * 0.36;
        cast.student.x = c.w * 0.64;
        for a in [&mut cast.teacher, &mut cast.student] {
            a.floor = floor;
            a.scale = 1.45;
        }
        cast.teacher.facing = 1.0;
        cast.student.facing = -1.0;
        cast.teacher.draw(g, th, c.lt + 3.0);
        cast.student.draw(g, th, c.lt);
        speaker_x = if cast.speaker_is_student { cast.student.x } else { cast.teacher.x };
    } else {
        cast.teacher.x = c.w * 0.42;
        cast.teacher.floor = floor;
        cast.teacher.scale = 1.6;
        cast.teacher.facing = 1.0;
        cast.teacher.draw(g, th, c.lt);
        speaker_x = cast.teacher.x;
    }
    let head_y = floor - crate::character::HEIGHT * 1.5;
    // floating prop
    if !shot.icon.is_empty() {
        let p = prog(c.lt, 0.2, 0.5);
        let (ix, iy) = if cast.duo { (c.w * 0.5, floor - crate::character::HEIGHT * 1.45 * 0.62) } else { (c.w * 0.68, c.h * 0.42) };
        let size = if cast.duo { 200.0 } else { 300.0 };
        if enter(g, ix, iy, p, Enter::Pop) {
            sunburst(g, ix, iy + bob(c.lt, 0), size * 0.95, th.gold.a(0.22), c.lt, 1.0);
            icon_disk(g, th, &shot.icon, ix, iy + bob(c.lt, 0), size, c.accent(0), c.lt);
            g.restore();
        }
    }
    // speech bubble
    if !shot.text.is_empty() {
        let p = c.p(0, 0.4);
        let right = !cast.duo || !cast.speaker_is_student;
        let bw = 560.0f32;
        let bx = if cast.duo {
            if right { speaker_x - 60.0 } else { speaker_x - bw + 60.0 }
        } else {
            c.w * 0.5
        };
        let by = if cast.duo { head_y - 230.0 } else { c.h * 0.12 };
        let bx = bx.clamp(30.0, c.w - bw - 30.0);
        if enter(g, bx + bw / 2.0, by + 80.0, p, Enter::Pop) {
            let (sz, lines) = g.fit(th.body_font, &shot.text, 54.0, bw - 60.0, 3);
            let bh = sz * 1.15 * lines.len() as f32 + 60.0;
            let tail_x = (speaker_x + if right { 20.0 } else { -20.0 }).clamp(bx + 40.0, bx + bw - 40.0);
            let tail = [(tail_x - 28.0, by + bh - 4.0), (tail_x + 28.0, by + bh - 4.0), (speaker_x, by + bh + 70.0)];
            g.poly(&tail, Some(th.card), Some((th.ink, th.line_w)));
            g.rrect(bx, by, bw, bh, 34.0, Some(th.card), Some((th.ink, th.line_w)));
            g.poly(&[(tail_x - 24.0, by + bh - 8.0), (tail_x + 24.0, by + bh - 8.0), (tail_x, by + bh + 2.0)], Some(th.card), None);
            let reveal = (c.lt - c.beat(0)) * 40.0;
            g.text_block(th.body_font, &lines, sz, bx + bw / 2.0, by + bh / 2.0, Align::Center, th.text, None, reveal);
            g.restore();
        }
    }
}

fn title(g: &mut Gfx, shot: &Shot, c: &Ctx) {
    let th = c.th;
    let text = title_text(shot);
    let (cx, cy) = (c.w / 2.0, c.h * 0.44);
    sunburst(g, cx, cy, c.w * 0.75, c.accent(0).a(if th.dark { 0.12 } else { 0.14 }), c.lt, clamp01(c.lt * 3.0));
    let (sz, lines) = g.fit(th.label_font, &text, 150.0, c.w * 0.8, 3);
    let lh = sz * 1.12;
    let top = cy - lh * (lines.len() as f32 - 1.0) / 2.0;
    let emph = norm(&shot.emphasis);
    let mut wi = 0;
    let space = g.measure(th.label_font, " ", sz);
    // camera shake on the emphasized slam
    for (li, line) in lines.iter().enumerate() {
        let words: Vec<&str> = line.split(' ').collect();
        let total: f32 = words.iter().map(|w| g.measure(th.label_font, w, sz)).sum::<f32>() + space * (words.len() as f32 - 1.0);
        let mut x = cx - total / 2.0;
        let y = top + lh * li as f32;
        for w in words {
            let ww = g.measure(th.label_font, w, sz);
            let p = c.p(wi, 0.32);
            let is_emph = !emph.is_empty() && norm(w).contains(&emph);
            if enter(g, x + ww / 2.0, y, p, Enter::Slam) {
                let wob = if is_emph { (c.lt * 5.0).sin() * 2.5 } else { 0.0 };
                g.around(x + ww / 2.0, y, 1.0 + if is_emph { 0.04 * (c.lt * 4.0).sin() } else { 0.0 }, wob);
                if is_emph {
                    highlight(g, x - 14.0, y + sz * 0.1, ww + 28.0, sz * 0.62, th.gold, prog(c.lt, c.beat(wi) + 0.15, 0.3));
                }
                let col = if is_emph && th.dark { th.gold } else { th.text };
                let outline = if th.dark { None } else { Some((th.card, 14.0)) };
                g.text_ex(th.label_font, w, sz, x, y, Align::Left, col, outline, f32::MAX);
                g.restore();
            }
            x += ww + space;
            wi += 1;
        }
    }
    if !shot.icon.is_empty() {
        let p = prog(c.lt, c.beats.last().copied().unwrap_or(0.3) + 0.15, 0.5);
        let (ix, iy) = (c.w * 0.86, c.h * 0.2);
        if enter(g, ix, iy, p, Enter::Pop) {
            icons::draw(g, th, &shot.icon, ix, iy + bob(c.lt, 2), 210.0, c.accent(1), c.lt);
            g.restore();
        }
    }
}

fn hero_icon(g: &mut Gfx, shot: &Shot, c: &Ctx) {
    let th = c.th;
    let s = c.stage;
    let (cx, cy) = (s.cx(), s.y + s.h * 0.4);
    let col = c.accent(0);
    let p = c.p(0, 0.6);
    sunburst(g, cx, cy, 420.0 * ease_out(p), col.a(0.16), c.lt, 1.0);
    if enter(g, cx, cy, p, Enter::Pop) {
        icons::draw(g, th, if shot.icon.is_empty() { &shot.label } else { &shot.icon }, cx, cy + bob(c.lt, 0), 400.0, col, c.lt);
        g.restore();
    }
    let lbl = if shot.label.is_empty() { &shot.title } else { &shot.label };
    if !lbl.is_empty() {
        let p = c.p(1, 0.4);
        let y = s.y + s.h * 0.78;
        if enter(g, cx, y, p, Enter::Slam) {
            badge(g, th, lbl, cx, y, 64.0, th.gold);
            g.restore();
        }
    }
    if !shot.text.is_empty() {
        let y = s.y + s.h * 0.93;
        if c.lt > c.beat(2) {
            body_text(g, th, &shot.text, 44.0, cx, y, s.w * 0.85, 1, (c.lt - c.beat(2)) * 45.0, Align::Center);
        }
    }
}

fn list(g: &mut Gfx, shot: &Shot, c: &Ctx) {
    let th = c.th;
    let s = c.stage;
    let has_title = !shot.title.is_empty();
    header(g, c, &shot.title, s.y + 60.0);
    let n = shot.items.len().max(1);
    let top = s.y + if has_title { 160.0 } else { 40.0 };
    let avail = s.y + s.h - top;
    let row_h = (avail / n as f32).min(185.0);
    let block_h = row_h * n as f32;
    let y0 = top + (avail - block_h) / 2.0;
    let card_w = (s.w * 0.86).min(1150.0);
    let x0 = s.cx() - card_w / 2.0;
    let recap = shot.kind == ShotKind::Recap;
    for (i, it) in shot.items.iter().enumerate() {
        let p = c.p(i + 1, 0.5);
        let y = y0 + row_h * i as f32;
        let ch = row_h - 26.0;
        let col = c.accent(i);
        if enter(g, x0 + card_w / 2.0, y + ch / 2.0, p, Enter::SlideR) {
            g.translate(0.0, bob(c.lt, i) * 0.4);
            card(g, th, x0, y, card_w, ch, col);
            let disk = ch * 0.78;
            let dx = x0 + 24.0 + disk / 2.0;
            if recap {
                // check box that ticks
                let bx = dx - disk * 0.32;
                let bs = disk * 0.64;
                g.rrect(bx, y + ch / 2.0 - bs / 2.0, bs, bs, 12.0, Some(th.card), Some((th.ink, th.line_w)));
                let tp = prog(c.lt, c.beat(i + 1) + 0.15, 0.35);
                let pts = [(bx + bs * 0.18, y + ch / 2.0), (bx + bs * 0.42, y + ch / 2.0 + bs * 0.24), (bx + bs * 0.86, y + ch / 2.0 - bs * 0.3)];
                let part = crate::gfx::trim(&pts, tp);
                g.line(&part, th.accent(3), th.line_w * 2.0);
            } else {
                let icon = if it.icon.is_empty() { &it.label } else { &it.icon };
                icon_disk(g, th, icon, dx, y + ch / 2.0, disk, col, c.lt + i as f32);
            }
            let tx = dx + disk / 2.0 + 30.0;
            let tw = x0 + card_w - tx - 30.0;
            if it.note.is_empty() {
                let (sz, lines) = g.fit(th.label_font, &it.label, (ch * 0.42).min(60.0), tw, 2);
                g.text_block(th.label_font, &lines, sz, tx, y + ch / 2.0, Align::Left, th.text, None, f32::MAX);
            } else {
                let (sz, l1) = g.fit(th.label_font, &it.label, (ch * 0.34).min(54.0), tw, 1);
                g.text_block(th.label_font, &l1, sz, tx, y + ch * 0.34, Align::Left, th.text, None, f32::MAX);
                let (sz2, l2) = g.fit(th.body_font, &it.note, (ch * 0.27).min(40.0), tw, 1);
                let reveal = (c.lt - c.beat(i + 1) - 0.2) * 50.0;
                g.text_block(th.body_font, &l2, sz2, tx, y + ch * 0.72, Align::Left, th.text.a(0.8), None, reveal);
            }
            g.restore();
        }
    }
}

fn item_node(g: &mut Gfx, c: &Ctx, it: &crate::lesson::Item, cx: f32, cy: f32, size: f32, i: usize, p: f32) {
    let th = c.th;
    if enter(g, cx, cy, p, Enter::Pop) {
        let y = cy + bob(c.lt, i);
        let icon = if it.icon.is_empty() { &it.label } else { &it.icon };
        icon_disk(g, th, icon, cx, y, size, c.accent(i), c.lt + i as f32 * 0.7);
        let lsz = (size * 0.24).clamp(30.0, 50.0);
        let (sz, lines) = g.fit(th.label_font, &it.label, lsz, size * 1.5, 2);
        let ly = y + size / 2.0 + 18.0 + sz * 0.6 * lines.len() as f32;
        g.text_block(th.label_font, &lines, sz, cx, ly, Align::Center, th.text, if th.dark { None } else { Some((th.card, 8.0)) }, f32::MAX);
        if !it.note.is_empty() {
            let ny = ly + sz * 0.6 * lines.len() as f32 + 30.0;
            body_text(g, th, &it.note, (sz * 0.8).min(36.0), cx, ny, size * 1.6, 2, (c.lt - c.beat(i + 1) - 0.2) * 50.0, Align::Center);
        }
        g.restore();
    }
}

fn flow(g: &mut Gfx, shot: &Shot, c: &Ctx) {
    let th = c.th;
    let s = c.stage;
    header(g, c, &shot.title, s.y + 60.0);
    let n = shot.items.len();
    let gap = s.w / n as f32;
    let size = (gap * 0.55).min(250.0);
    let cy = s.y + s.h * 0.48;
    let pos: Vec<(f32, f32)> = (0..n)
        .map(|i| (s.x + gap * (i as f32 + 0.5), cy + if i % 2 == 1 { 30.0 } else { -30.0 }))
        .collect();
    for i in 1..n {
        let (a, b) = (pos[i - 1], pos[i]);
        let start = c.beat(i + 1) - 0.35;
        let p = prog(c.lt, start, 0.35);
        if p > 0.0 {
            let r = size / 2.0 + 18.0;
            let d = ((b.0 - a.0).hypot(b.1 - a.1)).max(1.0);
            let (ux, uy) = ((b.0 - a.0) / d, (b.1 - a.1) / d);
            g.arrow((a.0 + ux * r, a.1 + uy * r), (b.0 - ux * r, b.1 - uy * r), -0.18, th.ink, th.line_w * 1.2, ease_in_out(p));
        }
    }
    // a little "packet" travels along the flow once everything is in
    let all_in = c.beat(n) + 0.5;
    if c.lt > all_in && n >= 2 {
        let k = ((c.lt - all_in) * 0.6).fract() * (n - 1) as f32;
        let i = k.floor() as usize;
        let f = ease_in_out(k.fract());
        let (a, b) = (pos[i], pos[i + 1]);
        let (x, y) = (a.0 + (b.0 - a.0) * f, a.1 + (b.1 - a.1) * f - (f * std::f32::consts::PI).sin() * 70.0);
        g.circle(x, y, 13.0, Some(th.gold), Some((th.ink, 3.5)));
    }
    for (i, it) in shot.items.iter().enumerate() {
        item_node(g, c, it, pos[i].0, pos[i].1, size, i, c.p(i + 1, 0.55));
    }
}

fn cycle(g: &mut Gfx, shot: &Shot, c: &Ctx) {
    let th = c.th;
    let s = c.stage;
    let n = shot.items.len();
    let has_title = !shot.title.is_empty();
    let (cx, cy) = (s.cx(), s.y + s.h * if has_title { 0.53 } else { 0.5 });
    let rad = (s.h * 0.34).min(s.w * 0.3);
    let size = (rad * 0.85).min(200.0);
    let pos: Vec<(f32, f32)> = (0..n)
        .map(|i| {
            let a = -std::f32::consts::FRAC_PI_2 + std::f32::consts::TAU * i as f32 / n as f32;
            (cx + a.cos() * rad * 1.25, cy + a.sin() * rad)
        })
        .collect();
    if has_title {
        let p = c.p(0, 0.45);
        if enter(g, cx, cy, p, Enter::Pop) {
            badge(g, th, &shot.title, cx, cy, 46.0, th.gold);
            g.restore();
        }
    }
    for i in 0..n {
        let j = (i + 1) % n;
        let start = if j == 0 { c.beat(n) + 0.35 } else { c.beat(j + 1) - 0.35 };
        let p = prog(c.lt, start, 0.4);
        if p > 0.0 {
            let (a, b) = (pos[i], pos[j]);
            let r = size / 2.0 + 16.0;
            let d = ((b.0 - a.0).hypot(b.1 - a.1)).max(1.0);
            let (ux, uy) = ((b.0 - a.0) / d, (b.1 - a.1) / d);
            g.arrow((a.0 + ux * r, a.1 + uy * r), (b.0 - ux * r, b.1 - uy * r), 0.22, th.ink, th.line_w * 1.2, ease_in_out(p));
        }
    }
    let all_in = c.beat(n) + 0.8;
    if c.lt > all_in {
        let k = ((c.lt - all_in) * 0.5).fract() * n as f32;
        let i = k.floor() as usize % n;
        let f = ease_in_out(k.fract());
        let (a, b) = (pos[i], pos[(i + 1) % n]);
        let (mx, my) = ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0);
        let (nx, ny) = (-(b.1 - a.1), b.0 - a.0);
        let pts = crate::gfx::bezier(a, (mx + nx * 0.22, my + ny * 0.22), b, 20);
        let q = pts[((f * 20.0) as usize).min(20)];
        g.circle(q.0, q.1, 13.0, Some(th.gold), Some((th.ink, 3.5)));
    }
    for (i, it) in shot.items.iter().enumerate() {
        item_node(g, c, it, pos[i].0, pos[i].1, size, i, c.p(i + 1, 0.55));
    }
}

fn compare(g: &mut Gfx, shot: &Shot, c: &Ctx) {
    let th = c.th;
    let s = c.stage;
    let cw = s.w * 0.4;
    let ch = s.h * 0.82;
    let y = s.y + (s.h - ch) / 2.0;
    let sides = [(s.x + s.w * 0.06, 0usize, Enter::SlideL, th.accent(1)), (s.x + s.w * 0.54, 2usize, Enter::SlideR, th.accent(2))];
    for (k, (x, bi, how, col)) in sides.iter().enumerate() {
        let Some(it) = shot.items.get(k) else { continue };
        let p = c.p(*bi, 0.55);
        if enter(g, x + cw / 2.0, y + ch / 2.0, p, *how) {
            g.around(x + cw / 2.0, y + ch / 2.0, 1.0, (c.lt * 1.3 + k as f32).sin() * 0.8);
            card(g, th, *x, y, cw, ch, *col);
            g.rrect(*x, y, cw, 90.0, 26.0, Some(*col), Some((th.ink, th.line_w)));
            g.rect(*x + th.line_w / 2.0, y + 60.0, cw - th.line_w, 27.0, *col);
            let (sz, l) = g.fit(th.label_font, &it.label, 54.0, cw - 50.0, 1);
            let tc = if luminance(*col) > 0.55 { hex(0x161616) } else { hex(0xFFFFFF) };
            g.text_block(th.label_font, &l, sz, x + cw / 2.0, y + 45.0, Align::Center, tc, None, f32::MAX);
            let icon = if it.icon.is_empty() { &it.label } else { &it.icon };
            icons::draw(g, th, icon, x + cw / 2.0, y + 90.0 + ch * 0.27 + bob(c.lt, k), (ch * 0.36).min(cw * 0.6), *col, c.lt + k as f32);
            if !it.note.is_empty() {
                body_text(g, th, &it.note, 44.0, x + cw / 2.0, y + ch * 0.8, cw - 60.0, 3, (c.lt - c.beat(*bi) - 0.3) * 50.0, Align::Center);
            }
            g.restore();
        }
    }
    let p = c.p(1, 0.35);
    let (vx, vy) = (s.cx(), s.cy());
    if enter(g, vx, vy, p, Enter::Slam) {
        let spin = (c.lt * 2.0).sin() * 6.0;
        g.around(vx, vy, 1.0, spin);
        let pts: Vec<(f32, f32)> = (0..18)
            .map(|i| {
                let a = std::f32::consts::TAU * i as f32 / 18.0;
                let r = if i % 2 == 0 { 92.0 } else { 70.0 };
                (vx + a.cos() * r, vy + a.sin() * r)
            })
            .collect();
        g.poly(&pts, Some(th.gold), Some((th.ink, th.line_w)));
        g.text(th.label_font, "VS", 70.0, vx, vy, Align::Center, hex(0x161616));
        g.restore();
    }
}

fn hub(g: &mut Gfx, shot: &Shot, c: &Ctx) {
    let th = c.th;
    let s = c.stage;
    let (cx, cy) = (s.cx(), s.cy());
    let n = shot.items.len();
    let rx = s.w * 0.36;
    let ry = s.h * 0.36;
    let size = (s.h * 0.24).min(200.0);
    let pos: Vec<(f32, f32)> = (0..n)
        .map(|i| {
            let a = -std::f32::consts::FRAC_PI_2 + std::f32::consts::TAU * i as f32 / n as f32 + if n == 2 { std::f32::consts::FRAC_PI_2 } else { 0.0 };
            (cx + a.cos() * rx, cy + a.sin() * ry)
        })
        .collect();
    for (i, q) in pos.iter().enumerate() {
        let p = prog(c.lt, c.beat(i + 1) - 0.3, 0.3);
        if p > 0.0 {
            let pts = crate::gfx::trim(&[(cx, cy), *q], ease_out(p));
            g.line(&pts, th.ink.a(0.7), th.line_w);
        }
    }
    let p = c.p(0, 0.6);
    if enter(g, cx, cy, p, Enter::Pop) {
        let r = 130.0;
        g.circle(cx + th.shadow_offset, cy + th.shadow_offset, r, Some(th.gold), Some((th.ink, th.line_w)));
        g.circle(cx, cy, r, Some(th.card), Some((th.ink, th.line_w)));
        if !shot.icon.is_empty() {
            icons::draw(g, th, &shot.icon, cx, cy - 30.0, 130.0, th.gold, c.lt);
            let (sz, l) = g.fit(th.label_font, &shot.title, 40.0, r * 1.7, 1);
            g.text_block(th.label_font, &l, sz, cx, cy + 62.0, Align::Center, th.text, None, f32::MAX);
        } else {
            let (sz, l) = g.fit(th.label_font, &shot.title, 54.0, r * 1.6, 3);
            g.text_block(th.label_font, &l, sz, cx, cy, Align::Center, th.text, None, f32::MAX);
        }
        g.restore();
    }
    for (i, it) in shot.items.iter().enumerate() {
        item_node(g, c, it, pos[i].0, pos[i].1 - 30.0, size, i, c.p(i + 1, 0.55));
    }
}

fn definition(g: &mut Gfx, shot: &Shot, c: &Ctx) {
    let th = c.th;
    let s = c.stage;
    let cw = s.w * 0.88;
    let (dsz, dlines) = g.fit(th.body_font, &shot.text, 58.0, cw - 120.0, 4);
    let ch = (230.0 + dsz * 1.15 * dlines.len() as f32 + 50.0).max(s.h * 0.42);
    let (x, y) = (s.cx() - cw / 2.0, s.cy() - ch / 2.0 + 20.0);
    let p = c.p(0, 0.55);
    if enter(g, s.cx(), s.cy(), p, Enter::SlideUp) {
        card(g, th, x, y, cw, ch, c.accent(0));
        badge(g, th, "DEFINITION", x + 170.0, y, 34.0, th.gold);
        let term = if shot.title.is_empty() { &shot.label } else { &shot.title };
        let tp = c.p(1, 0.45);
        let (tsz, tl) = g.fit(th.label_font, term, 96.0, cw - 360.0, 1);
        let tw = g.measure(th.label_font, &tl[0], tsz);
        let ty = y + 120.0;
        let tx = x + 60.0;
        highlight(g, tx - 14.0, ty + tsz * 0.12, tw + 28.0, tsz * 0.55, th.gold, prog(c.lt, c.beat(1) + 0.2, 0.35));
        if enter(g, tx + tw / 2.0, ty, tp, Enter::Slam) {
            g.text(th.label_font, &tl[0], tsz, tx, ty, Align::Left, th.text);
            g.restore();
        }
        let reveal = (c.lt - c.beat(2)) * 38.0;
        if reveal > 0.0 {
            let (sz, lines) = (dsz, dlines.clone());
            let lh = sz * 1.15;
            let top = y + 230.0 + lh * 0.5;
            let mut shown = 0.0;
            for (i, l) in lines.iter().enumerate() {
                g.text_ex(th.body_font, l, sz, x + 60.0, top + lh * i as f32, Align::Left, th.text, None, reveal - shown);
                shown += l.chars().count() as f32 + 1.0;
            }
        }
        if !shot.icon.is_empty() {
            let ip = prog(c.lt, c.beat(1) + 0.3, 0.5);
            let (ix, iy) = (x + cw - 120.0, y + 110.0);
            if enter(g, ix, iy, ip, Enter::Pop) {
                g.around(ix, iy, 1.0, 10.0 + (c.lt * 2.0).sin() * 4.0);
                icon_disk(g, th, &shot.icon, ix, iy, 170.0, c.accent(1), c.lt);
                g.restore();
            }
        }
        g.restore();
    }
}

/// Split "3.8 billion" / "70%" / "$1,200" into (prefix, number, decimals, suffix).
fn parse_value(v: &str) -> Option<(String, f64, usize, String)> {
    let start = v.find(|c: char| c.is_ascii_digit())?;
    let rest = &v[start..];
    let end = rest.find(|c: char| !(c.is_ascii_digit() || c == '.' || c == ',')).unwrap_or(rest.len());
    let num_s = rest[..end].trim_end_matches(['.', ',']);
    let clean: String = num_s.chars().filter(|c| *c != ',').collect();
    let n: f64 = clean.parse().ok()?;
    let decimals = clean.split('.').nth(1).map(|d| d.len()).unwrap_or(0);
    Some((v[..start].to_string(), n, decimals, rest[num_s.len()..].to_string()))
}

fn fmt_num(n: f64, decimals: usize, commas: bool) -> String {
    let s = format!("{n:.decimals$}");
    if !commas {
        return s;
    }
    let (int, frac) = s.split_once('.').map(|(a, b)| (a.to_string(), format!(".{b}"))).unwrap_or((s.clone(), String::new()));
    let mut out = String::new();
    for (i, ch) in int.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    out.chars().rev().collect::<String>() + &frac
}

fn number(g: &mut Gfx, shot: &Shot, c: &Ctx) {
    let th = c.th;
    let s = c.stage;
    let cx = s.cx();
    let cy = s.y + s.h * 0.42;
    let p = c.p(0, 0.4);
    sunburst(g, cx, cy, 520.0 * ease_out(p), c.accent(0).a(0.13), c.lt, 1.0);
    let raw = if shot.value.is_empty() { &shot.title } else { &shot.value };
    let shown = match parse_value(raw) {
        Some((pre, n, dec, suf)) => {
            let k = ease_out(prog(c.lt, c.beat(0), 1.2));
            format!("{pre}{}{suf}", fmt_num(n * k as f64, dec, raw.contains(',')))
        }
        None => raw.clone(),
    };
    if enter(g, cx, cy, p, Enter::Slam) {
        let landed = prog(c.lt, c.beat(1), 0.4);
        let pulse = 1.0 + (1.0 - landed) * landed * 0.5;
        g.around(cx, cy, pulse, 0.0);
        let (sz, l) = g.fit(th.label_font, raw, 230.0, s.w * 0.9, 1);
        let outline = if th.dark { None } else { Some((th.card, 16.0)) };
        g.text_ex(th.label_font, &shown, sz, cx, cy, Align::Center, c.accent(0).darken(if th.dark { 0.0 } else { 0.1 }), outline, f32::MAX);
        let _ = l;
        g.restore();
    }
    confetti(g, th, cx, cy, c.lt - c.beat(1), c.seed, 40);
    let text = if shot.text.is_empty() { &shot.label } else { &shot.text };
    if !text.is_empty() && c.lt > c.beat(1) - 0.2 {
        body_text(g, th, text, 56.0, cx, s.y + s.h * 0.78, s.w * 0.85, 2, (c.lt - c.beat(1) + 0.2) * 45.0, Align::Center);
    }
    if !shot.icon.is_empty() {
        let ip = prog(c.lt, 0.3, 0.5);
        let (ix, iy) = (s.x + s.w * 0.88, s.y + s.h * 0.15);
        if enter(g, ix, iy, ip, Enter::Pop) {
            icons::draw(g, th, &shot.icon, ix, iy + bob(c.lt, 1), 170.0, c.accent(1), c.lt);
            g.restore();
        }
    }
}

fn formula(g: &mut Gfx, shot: &Shot, c: &Ctx) {
    let th = c.th;
    let s = c.stage;
    let toks: Vec<&str> = shot.text.split_whitespace().collect();
    let cw = s.w * 0.92;
    let ch = s.h * 0.5;
    let (x, y) = (s.cx() - cw / 2.0, s.cy() - ch / 2.0);
    let board = if th.dark { th.card } else { hex(0x22313F) };
    g.rrect(x + th.shadow_offset, y + th.shadow_offset, cw, ch, 26.0, Some(c.accent(0)), Some((th.ink, th.line_w)));
    g.rrect(x, y, cw, ch, 26.0, Some(board), Some((th.ink, th.line_w)));
    if !shot.title.is_empty() {
        badge(g, th, &shot.title, x + cw / 2.0, y, 38.0, th.gold);
    }
    let joined: String = toks.iter().map(|t| if matches!(*t, "→" | "->" | "=>" | "⟶") { "MM" } else { t }).collect::<Vec<_>>().join(" ");
    let mut sz = 110.0f32;
    while g.measure(th.label_font, &joined, sz) > cw - 100.0 && sz > 20.0 {
        sz *= 0.93;
    }
    let space = g.measure(th.label_font, " ", sz);
    let total = g.measure(th.label_font, &joined, sz);
    let mut tx = s.cx() - total / 2.0;
    let cy = y + ch / 2.0;
    for (i, tok) in toks.iter().enumerate() {
        let w = g.measure(th.label_font, tok, sz);
        let arrow = matches!(*tok, "→" | "->" | "=>" | "⟶");
        let w = if arrow { sz * 1.1 } else { w };
        let op = arrow || matches!(*tok, "=" | "+" | "-" | "×" | "x" | "÷" | "/" | "*" | "<" | ">" | "≈");
        let col = if op { th.gold } else { hex(0xF6F4EC) };
        if enter(g, tx + w / 2.0, cy, c.p(i, 0.4), Enter::Drop) {
            if arrow {
                g.arrow((tx + 6.0, cy), (tx + w - 6.0, cy), 0.0, col, sz * 0.09, 1.0);
            } else {
                g.text(th.label_font, tok, sz, tx, cy, Align::Left, col);
            }
            g.restore();
        }
        tx += w + space;
    }
    let up = prog(c.lt, c.beat(toks.len()), 0.5);
    if up > 0.0 {
        let pts = crate::gfx::trim(&crate::gfx::bezier((s.cx() - total / 2.0, cy + sz * 0.7), (s.cx(), cy + sz * 0.95), (s.cx() + total / 2.0, cy + sz * 0.62), 20), ease_out(up));
        g.line(&pts, th.gold, 8.0);
    }
    if !shot.label.is_empty() {
        let lp = prog(c.lt, c.beat(toks.len()) + 0.2, 0.4);
        if enter(g, s.cx(), y + ch + 90.0, lp, Enter::SlideUp) {
            body_text(g, th, &shot.label, 50.0, s.cx(), y + ch + 90.0, cw, 1, f32::MAX, Align::Center);
            g.restore();
        }
    }
}

fn mnemonic(g: &mut Gfx, shot: &Shot, c: &Ctx) {
    let th = c.th;
    let s = c.stage;
    let n = shot.items.len();
    let letters: Vec<char> = {
        let from_title: Vec<char> = shot.title.chars().filter(|c| c.is_alphanumeric()).collect();
        if from_title.len() == n { from_title } else { shot.items.iter().map(|i| i.label.chars().next().unwrap_or('?')).collect() }
    };
    let gap = s.w / n as f32;
    let tile = (gap * 0.72).min(210.0);
    let ty = s.y + s.h * 0.36;
    for i in 0..n {
        let cx = s.x + gap * (i as f32 + 0.5);
        let col = c.accent(i);
        if enter(g, cx, ty, c.p(i, 0.6), Enter::Drop) {
            let wob = (c.lt * 2.0 + i as f32).sin() * 4.0;
            g.around(cx, ty, 1.0, wob);
            g.rrect(cx - tile / 2.0 + th.shadow_offset, ty - tile / 2.0 + th.shadow_offset, tile, tile, 28.0, Some(th.ink.a(0.85)), None);
            g.rrect(cx - tile / 2.0, ty - tile / 2.0, tile, tile, 28.0, Some(col), Some((th.ink, th.line_w)));
            let tc = if luminance(col) > 0.55 { hex(0x161616) } else { hex(0xFFFFFF) };
            g.text(th.label_font, &letters[i].to_uppercase().to_string(), tile * 0.72, cx, ty, Align::Center, tc);
            g.restore();
        }
        let wp = c.p(n + i, 0.45);
        let it = &shot.items[i];
        let wy = ty + tile / 2.0 + 80.0;
        if enter(g, cx, wy, wp, Enter::SlideUp) {
            // the matching tile gets a little bounce when its word lands
            let (sz, l) = g.fit(th.label_font, &it.label, 52.0, gap * 0.95, 2);
            g.text_block(th.label_font, &l, sz, cx, wy, Align::Center, th.text, None, f32::MAX);
            if !it.note.is_empty() {
                body_text(g, th, &it.note, 34.0, cx, wy + sz * 1.3 + 30.0, gap * 0.95, 2, f32::MAX, Align::Center);
            }
            g.restore();
        }
    }
}

fn myth(g: &mut Gfx, shot: &Shot, c: &Ctx) {
    let th = c.th;
    let s = c.stage;
    let cw = s.w * 0.86;
    let ch = s.h * 0.36;
    let x = s.cx() - cw / 2.0;
    let y1 = s.y + 20.0;
    let myth_txt = if shot.title.is_empty() { &shot.label } else { &shot.title };
    if enter(g, s.cx(), y1 + ch / 2.0, c.p(0, 0.5), Enter::SlideL) {
        let fade = 1.0 - 0.35 * prog(c.lt, c.beat(1), 0.3);
        g.fade(fade);
        card(g, th, x, y1, cw, ch, th.accent(2));
        let (sz, lines) = g.fit(th.body_font, &format!("\u{201C}{myth_txt}\u{201D}"), 62.0, cw - 120.0, 3);
        g.text_block(th.body_font, &lines, sz, s.cx(), y1 + ch / 2.0, Align::Center, th.text, None, f32::MAX);
        // strike-through draws on after the stamp
        let sp = prog(c.lt, c.beat(1) + 0.1, 0.35);
        if sp > 0.0 {
            let pts = [(x + 50.0, y1 + ch * 0.55), (x + cw - 50.0, y1 + ch * 0.45)];
            let part = crate::gfx::trim(&pts, ease_out(sp));
            g.line(&part, th.accent(2), 12.0);
        }
        g.restore();
    }
    let sp = c.p(1, 0.3);
    let (sx, sy) = (x + cw - 170.0, y1 + 40.0);
    if enter(g, sx, sy, sp, Enter::Slam) {
        g.around(sx, sy, 1.0, -14.0);
        let red = th.accent(2);
        g.rrect(sx - 150.0, sy - 55.0, 300.0, 110.0, 16.0, Some(red.a(0.12)), Some((red, 10.0)));
        g.text(Font::Display, "MYTH", 82.0, sx, sy, Align::Center, red);
        g.restore();
    }
    let y2 = y1 + ch + 70.0;
    let fact_txt = &shot.text;
    if enter(g, s.cx(), y2 + ch / 2.0, c.p(2, 0.55), Enter::SlideUp) {
        card(g, th, x, y2, cw, ch, th.accent(3));
        badge(g, th, "FACT", x + 110.0, y2, 40.0, th.accent(3));
        icons::draw(g, th, "check", x + 90.0, y2 + ch / 2.0 + 10.0, 110.0, th.accent(3), c.lt);
        let reveal = (c.lt - c.beat(2) - 0.15) * 45.0;
        body_text(g, th, fact_txt, 58.0, s.cx() + 60.0, y2 + ch / 2.0, cw - 260.0, 3, reveal, Align::Center);
        g.restore();
    }
    confetti(g, th, s.cx(), y2 + ch / 2.0, c.lt - c.beat(2) - 0.1, c.seed, 24);
}

fn timeline(g: &mut Gfx, shot: &Shot, c: &Ctx) {
    let th = c.th;
    let s = c.stage;
    header(g, c, &shot.title, s.y + 60.0);
    let n = shot.items.len();
    let ly = s.y + s.h * 0.55;
    let (x0, x1) = (s.x + 80.0, s.x + s.w - 80.0);
    let lp = prog(c.lt, 0.15, (c.beat(n) - 0.15).max(0.4));
    let pts = crate::gfx::trim(&[(x0, ly), (x1, ly)], ease_out(lp));
    g.line(&pts, th.ink, th.line_w * 1.6);
    if lp >= 1.0 {
        g.arrow((x1 - 10.0, ly), (x1 + 30.0, ly), 0.0, th.ink, th.line_w * 1.6, 1.0);
    }
    let gap = (x1 - x0) / n as f32;
    for (i, it) in shot.items.iter().enumerate() {
        let x = x0 + gap * (i as f32 + 0.5);
        let up = i % 2 == 0;
        let p = c.p(i + 1, 0.5);
        if enter(g, x, ly, p, Enter::Pop) {
            g.circle(x, ly, 20.0, Some(c.accent(i)), Some((th.ink, th.line_w)));
            let dy = if up { -1.0 } else { 1.0 };
            g.seg((x, ly + dy * 22.0), (x, ly + dy * 70.0), th.ink, th.line_w);
            let iy = ly + dy * 170.0;
            let icon = if it.icon.is_empty() { &it.label } else { &it.icon };
            icon_disk(g, th, icon, x, iy + bob(c.lt, i), 150.0, c.accent(i), c.lt + i as f32);
            let ty = if up { iy - 115.0 } else { iy + 115.0 };
            let (sz, l) = g.fit(th.label_font, &it.label, 42.0, gap * 0.95, 2);
            g.text_block(th.label_font, &l, sz, x, ty, Align::Center, th.text, None, f32::MAX);
            if !it.note.is_empty() {
                badge(g, th, &it.note, x, ly + dy * -48.0, 28.0, th.gold);
            }
            g.restore();
        }
    }
}

fn question(g: &mut Gfx, shot: &Shot, c: &Ctx) {
    let th = c.th;
    let s = c.stage;
    let (qx, qy) = (s.x + s.w * 0.2, s.cy());
    if enter(g, qx, qy, c.p(0, 0.6), Enter::Pop) {
        sunburst(g, qx, qy, 300.0, th.accent(1).a(0.15), c.lt, 1.0);
        icons::draw(g, th, if shot.icon.is_empty() { "question" } else { &shot.icon }, qx, qy + bob(c.lt, 0), 300.0, th.accent(1), c.lt);
        g.restore();
    }
    let bw = s.w * 0.58;
    let (bx, by) = (s.x + s.w * 0.38, s.cy());
    if enter(g, bx + bw / 2.0, by, c.p(1, 0.45), Enter::SlideR) {
        let txt = title_text(shot);
        let (sz, lines) = g.fit(th.label_font, &txt, 76.0, bw - 80.0, 4);
        let bh = sz * 1.15 * lines.len() as f32 + 90.0;
        card(g, th, bx, by - bh / 2.0, bw, bh, th.gold);
        g.text_block(th.label_font, &lines, sz, bx + bw / 2.0, by, Align::Center, th.text, None, f32::MAX);
        // thinking dots
        for i in 0..3 {
            let up = ((c.lt * 5.0 - i as f32 * 0.7).sin().max(0.0)) * 12.0;
            g.circle(bx + bw / 2.0 - 40.0 + i as f32 * 40.0, by + bh / 2.0 + 50.0 - up, 11.0, Some(th.accent(i)), Some((th.ink, 3.0)));
        }
        g.restore();
    }
}

// ------------------------------------------------------------------ cards outside the script

pub fn intro(g: &mut Gfx, th: &Theme, title: &str, lt: f32, cast: &mut Cast, w: f32, h: f32) {
    let cx = w / 2.0;
    sunburst(g, cx, h * 0.42, w * 0.8, th.gold.a(0.2), lt, clamp01(lt * 3.0));
    let p = prog(lt, 0.15, 0.5);
    if enter(g, cx, h * 0.36, p, Enter::Slam) {
        let (sz, lines) = g.fit(th.label_font, title, 140.0, w * 0.82, 2);
        let outline = if th.dark { None } else { Some((th.card, 18.0)) };
        g.text_block(th.label_font, &lines, sz, cx, h * 0.34, Align::Center, th.text, outline, f32::MAX);
        g.restore();
    }
    let bp = prog(lt, 0.55, 0.4);
    if enter(g, cx, h * 0.58, bp, Enter::Pop) {
        badge(g, th, "LET'S LEARN THIS FAST", cx, h * 0.56, 40.0, th.gold);
        g.restore();
    }
    cast_wave(g, th, lt, cast, w, h, Pose::Wave);
}

pub fn outro(g: &mut Gfx, th: &Theme, n_quiz: usize, lt: f32, cast: &mut Cast, w: f32, h: f32, seed: u32) {
    let cx = w / 2.0;
    sunburst(g, cx, h * 0.38, w * 0.8, th.accent(3).a(0.18), lt, clamp01(lt * 3.0));
    if enter(g, cx, h * 0.3, prog(lt, 0.1, 0.5), Enter::Slam) {
        let outline = if th.dark { None } else { Some((th.card, 18.0)) };
        g.text_ex(th.label_font, "QUIZ TIME!", 170.0, cx, h * 0.3, Align::Center, th.text, outline, f32::MAX);
        g.restore();
    }
    confetti(g, th, cx, h * 0.3, lt - 0.45, seed, 60);
    let msg = if n_quiz == 0 { "Nice work. You made it to the end!".to_string() } else { format!("{n_quiz} questions are waiting for you") };
    if enter(g, cx, h * 0.5, prog(lt, 0.7, 0.45), Enter::Pop) {
        badge(g, th, &msg, cx, h * 0.5, 44.0, th.gold);
        g.restore();
    }
    cast_wave(g, th, lt, cast, w, h, Pose::Cheer);
}

fn cast_wave(g: &mut Gfx, th: &Theme, lt: f32, cast: &mut Cast, w: f32, h: f32, pose: Pose) {
    let floor = h * 0.95;
    let pop = prog(lt, 0.4, 0.5);
    let actors: Vec<&mut Actor> = if cast.duo { vec![&mut cast.teacher, &mut cast.student] } else { vec![&mut cast.teacher] };
    let n = actors.len();
    for (i, a) in actors.into_iter().enumerate() {
        a.x = if n == 1 { w * 0.5 } else if i == 0 { w * 0.3 } else { w * 0.7 };
        a.floor = floor + (1.0 - ease_back(pop)) * 500.0;
        a.scale = 1.3;
        a.facing = if i == 0 { 1.0 } else { -1.0 };
        a.pose = pose;
        a.prev_pose = Pose::Idle;
        a.since = (lt - 0.4).max(0.0);
        a.talking = false;
        a.draw(g, th, lt + i as f32);
    }
    let _ = Look::Host;
}

/// Chapter card between scenes: a colored panel sweeps across, shows the heading, sweeps away.
pub fn chapter(g: &mut Gfx, th: &Theme, n: usize, heading: &str, lt: f32, dur: f32, w: f32, h: f32, seed: u32) {
    let col = th.accent(n + 1);
    let enter_p = ease_in_out(prog(lt, 0.0, 0.32));
    let exit_p = ease_in_out(prog(lt, dur - 0.32, 0.32));
    let skew = 160.0;
    let x0 = w + skew - (w + skew * 2.0) * enter_p - (w + skew * 2.0) * exit_p;
    let pts = [(x0, 0.0), (x0 + w + skew * 2.0, 0.0), (x0 + w + skew, h), (x0 - skew, h)];
    g.poly(&pts, Some(col), None);
    let stripe = [(x0 - 60.0, 0.0), (x0, 0.0), (x0 - skew, h), (x0 - skew - 60.0, h)];
    g.poly(&stripe, Some(th.gold), None);
    let tx = x0 + w / 2.0 + skew / 2.0;
    let tc = if luminance(col) > 0.6 { hex(0x161616) } else { hex(0xFFFFFF) };
    let ink = hex(0x161616);
    let tp = prog(lt, 0.2, 0.4);
    if enter(g, tx, h * 0.38, tp, Enter::Slam) {
        let label = format!("PART {}", n + 1);
        let bw = g.measure(Font::Display, &label, 48.0) + 60.0;
        g.rrect(tx - bw / 2.0, h * 0.38 - 38.0, bw, 76.0, 38.0, Some(hex(0xFFFFFF)), Some((ink, 5.0)));
        g.text(Font::Display, &label, 48.0, tx, h * 0.38, Align::Center, ink);
        g.restore();
    }
    let hp = prog(lt, 0.32, 0.45);
    if enter(g, tx, h * 0.55, hp, Enter::Slam) {
        let (sz, lines) = g.fit(Font::Display, heading, 120.0, w * 0.8, 2);
        g.text_block(Font::Display, &lines, sz, tx, h * 0.56, Align::Center, tc, Some((ink.a(0.25), 10.0)), f32::MAX);
        g.restore();
    }
    let _ = seed;
}
