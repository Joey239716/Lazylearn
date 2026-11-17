//! Renderers for the non-stickman formats: explainer, whiteboard and chat.

use crate::lesson::{Layout, Shape, Speaker, Visual};
use crate::player::{Ink, Phase, Player, CHALK, STUD_COL};
use crate::stickman::INK;
use macroquad::prelude::*;

const W: f32 = 1280.0;
const H: f32 = 720.0;

// ---------------------------------------------------------------- helpers

fn rnd(i: u32) -> f32 {
    let x = (i as f32 * 12.9898 + 78.233).sin() * 43758.545;
    x - x.floor()
}

/// (seconds into the scene, total scene seconds). After the scene's lines
/// finish (quiz / done) everything counts as fully revealed.
fn scene_clock(p: &Player) -> (f32, f32) {
    let n = p.lesson.scenes[p.scene].lines.len();
    let total: f32 = (0..n).map(|i| p.line_len(p.scene, i)).sum();
    if !matches!(p.phase, Phase::Playing) {
        return (total + 100.0, total);
    }
    let done: f32 = (0..p.line).map(|i| p.line_len(p.scene, i)).sum::<f32>() + p.t;
    (done, total)
}

/// When item `i` of `n` appears, spread across the first `span` of the scene.
fn reveal_at(i: usize, n: usize, total: f32, span: f32) -> f32 {
    if n == 0 { 0.0 } else { total * span * i as f32 / n as f32 }
}

fn ease_out_back(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    let c1 = 1.70158;
    let c3 = c1 + 1.0;
    1.0 + c3 * (x - 1.0).powi(3) + c1 * (x - 1.0).powi(2)
}

fn rrect(x: f32, y: f32, w: f32, h: f32, r: f32, c: Color) {
    let r = r.min(w / 2.0).min(h / 2.0);
    draw_rectangle(x + r, y, w - 2.0 * r, h, c);
    draw_rectangle(x, y + r, w, h - 2.0 * r, c);
    for (cx, cy) in [(x + r, y + r), (x + w - r, y + r), (x + r, y + h - r), (x + w - r, y + h - r)] {
        draw_circle(cx, cy, r, c);
    }
}

fn arrow_head(from: Vec2, to: Vec2, size: f32, c: Color) {
    let d = (to - from).normalize_or_zero();
    let n = vec2(-d.y, d.x);
    draw_triangle(to, to - d * size + n * size * 0.55, to - d * size - n * size * 0.55, c);
}

/// Node positions and edges for a layout inside `area`.
fn layout_points(layout: Layout, n: usize, area: Rect) -> (Vec<Vec2>, Vec<(usize, usize)>) {
    let c = area.center();
    if n <= 1 {
        return (vec![c], vec![]);
    }
    match layout {
        Layout::Cycle => {
            let pts = (0..n)
                .map(|i| {
                    let a = -std::f32::consts::FRAC_PI_2 + std::f32::consts::TAU * i as f32 / n as f32;
                    c + vec2(a.cos() * area.w * 0.36, a.sin() * area.h * 0.36)
                })
                .collect();
            (pts, (0..n).map(|i| (i, (i + 1) % n)).collect())
        }
        Layout::Hub => {
            let mut pts = vec![c];
            let k = n - 1;
            for i in 0..k {
                let a = -std::f32::consts::FRAC_PI_2 + std::f32::consts::TAU * i as f32 / k as f32;
                pts.push(c + vec2(a.cos() * area.w * 0.38, a.sin() * area.h * 0.36));
            }
            (pts, (1..n).map(|i| (0, i)).collect())
        }
        Layout::Compare => {
            let pts = (0..n)
                .map(|i| vec2(area.x + area.w * (i as f32 + 0.5) / n as f32, c.y))
                .collect();
            (pts, vec![])
        }
        Layout::Flow | Layout::Other => {
            let pts = (0..n)
                .map(|i| {
                    let side = if i % 2 == 0 { -1.0 } else { 1.0 };
                    let zig = if n > 3 { side * area.h * 0.12 } else { 0.0 };
                    vec2(area.x + area.w * (i as f32 + 0.5) / n as f32, c.y + zig)
                })
                .collect();
            (pts, (0..n - 1).map(|i| (i, i + 1)).collect())
        }
    }
}

/// Point on a shape's outline (t in 0..1), used for both filled and sketchy drawing.
fn outline(shape: Shape, c: Vec2, r: f32, t: f32) -> Vec2 {
    let a = -std::f32::consts::FRAC_PI_2 + t * std::f32::consts::TAU;
    let corners = |k: usize, rot: f32| {
        let seg = t * k as f32;
        let i = seg.floor() as usize % k;
        let f = seg - seg.floor();
        let p = |j: usize| {
            let a = rot + std::f32::consts::TAU * j as f32 / k as f32;
            c + vec2(a.cos(), a.sin()) * r
        };
        p(i).lerp(p((i + 1) % k), f)
    };
    match shape {
        Shape::Circle | Shape::Other => c + vec2(a.cos(), a.sin()) * r,
        Shape::Square => corners(4, std::f32::consts::FRAC_PI_4),
        Shape::Triangle => corners(3, -std::f32::consts::FRAC_PI_2),
        Shape::Hexagon => corners(6, 0.0),
        Shape::Star => {
            let seg = t * 10.0;
            let i = seg.floor() as usize % 10;
            let f = seg - seg.floor();
            let p = |j: usize| {
                let a = -std::f32::consts::FRAC_PI_2 + std::f32::consts::TAU * j as f32 / 10.0;
                let rr = if j % 2 == 0 { r * 1.1 } else { r * 0.5 };
                c + vec2(a.cos(), a.sin()) * rr
            };
            p(i).lerp(p((i + 1) % 10), f)
        }
    }
}

fn fill_shape(shape: Shape, c: Vec2, r: f32, col: Color) {
    let n = 60;
    for i in 0..n {
        let a = outline(shape, c, r, i as f32 / n as f32);
        let b = outline(shape, c, r, (i + 1) as f32 / n as f32);
        draw_triangle(c, a, b, col);
    }
}

// ---------------------------------------------------------------- explainer

const PALETTE: [Color; 6] = [
    Color::new(1.0, 0.42, 0.42, 1.0),
    Color::new(0.25, 0.85, 0.78, 1.0),
    Color::new(1.0, 0.8, 0.3, 1.0),
    Color::new(0.62, 0.48, 1.0, 1.0),
    Color::new(0.35, 0.68, 1.0, 1.0),
    Color::new(1.0, 0.55, 0.85, 1.0),
];

pub fn explainer(p: &Player, ink: &Ink, time: f32) {
    let si = p.scene;
    // sky gradient
    let strips = 30;
    for i in 0..strips {
        let f = i as f32 / strips as f32;
        let c = Color::new(0.05 + 0.1 * f, 0.06 + 0.03 * f, 0.18 + 0.12 * f, 1.0);
        draw_rectangle(0.0, H * f, W, H / strips as f32 + 1.0, c);
    }
    // twinkling stars
    for i in 0..110u32 {
        let x = rnd(i * 3) * W;
        let y = rnd(i * 3 + 1) * H;
        let tw = 0.35 + 0.65 * ((time * (0.6 + rnd(i * 3 + 2) * 2.0) + i as f32).sin() * 0.5 + 0.5);
        draw_circle(x, y, 0.8 + rnd(i * 7) * 1.6, Color::new(1.0, 1.0, 1.0, tw * 0.8));
    }
    // a planet that changes color per scene, drifting slowly
    let pc = PALETTE[si % PALETTE.len()];
    let drift = (time * 0.15).sin() * 12.0;
    let planet = vec2(1170.0 + drift, 690.0);
    draw_poly(planet.x, planet.y, 96, 240.0, 0.0, Color::new(pc.r * 0.55, pc.g * 0.55, pc.b * 0.6, 1.0));
    draw_poly(planet.x - 30.0, planet.y - 30.0, 96, 205.0, 0.0, Color::new(pc.r * 0.7, pc.g * 0.7, pc.b * 0.75, 1.0));
    draw_ellipse_lines(planet.x, planet.y, 330.0, 46.0, -0.25, 6.0, Color::new(1.0, 1.0, 1.0, 0.25));
    let moon = vec2(120.0 + drift * 0.5, 150.0);
    draw_circle(moon.x, moon.y, 26.0, Color::new(0.85, 0.85, 0.95, 0.9));
    draw_circle(moon.x + 8.0, moon.y - 5.0, 6.0, Color::new(0.7, 0.7, 0.82, 0.9));

    if matches!(p.phase, Phase::Done) {
        mascot(p, vec2(640.0, 560.0), time, 1.6);
        return;
    }

    let scene = &p.lesson.scenes[si];
    ink.centered(ink.ui(), &scene.heading, W / 2.0, 135.0, 36.0, WHITE);

    let (now, total) = scene_clock(p);
    let area = Rect::new(170.0, 175.0, 940.0, 300.0);
    if scene.visuals.is_empty() {
        // fall back to big floating chips of the key points
        for (i, b) in scene.board.iter().enumerate() {
            let at = reveal_at(i, scene.board.len(), total, 0.8);
            if now < at {
                break;
            }
            let s = ease_out_back((now - at) / 0.5);
            let y = 210.0 + i as f32 * 62.0;
            let w = ink.width(ink.ui(), b, 22.0) + 48.0;
            let col = PALETTE[(i + si) % PALETTE.len()];
            rrect(W / 2.0 - w * s / 2.0, y, w * s, 46.0, 23.0, col);
            if s > 0.9 {
                ink.centered(ink.ui(), b, W / 2.0, y + 31.0, 22.0, Color::new(0.08, 0.08, 0.15, 1.0));
            }
        }
    } else {
        draw_visual_graph_glow(&scene.visuals, scene.layout, area, now, total, time, ink, si);
        // key point caption
        let n = scene.board.len();
        if n > 0 {
            let idx = ((now / total.max(0.1)) * n as f32).floor().clamp(0.0, (n - 1) as f32) as usize;
            let text = &scene.board[idx];
            let w = (ink.width(ink.ui(), text, 22.0) + 56.0).min(1000.0);
            rrect(W / 2.0 - w / 2.0, 548.0, w, 50.0, 25.0, Color::new(1.0, 0.82, 0.35, 1.0));
            ink.centered(ink.ui(), text, W / 2.0, 581.0, 22.0, Color::new(0.1, 0.08, 0.2, 1.0));
        }
    }
    mascot(p, vec2(92.0, 560.0), time, 1.0);
}

#[allow(clippy::too_many_arguments)]
fn draw_visual_graph_glow(vs: &[Visual], layout: Layout, area: Rect, now: f32, total: f32, time: f32, ink: &Ink, si: usize) {
    let n = vs.len();
    let (pts, edges) = layout_points(layout, n, area);
    let r = if n <= 3 { 64.0 } else { 52.0 };
    let shown = |i: usize| now >= reveal_at(i, n, total, 0.75);
    let scale = |i: usize| ease_out_back((now - reveal_at(i, n, total, 0.75)) / 0.55);

    for &(a, b) in &edges {
        if !(shown(a) && shown(b)) {
            continue;
        }
        let d = (pts[b] - pts[a]).normalize_or_zero();
        let s = pts[a] + d * (r + 16.0);
        let e = pts[b] - d * (r + 16.0);
        let grow = ((now - reveal_at(b, n, total, 0.75)) / 0.5).clamp(0.0, 1.0);
        let e = s.lerp(e, grow);
        draw_line(s.x, s.y, e.x, e.y, 4.0, Color::new(1.0, 1.0, 1.0, 0.55));
        arrow_head(s, e, 16.0, Color::new(1.0, 1.0, 1.0, 0.8));
        // a little energy dot travelling along the arrow
        let f = (time * 0.7 + a as f32 * 0.3).fract();
        let dot = s.lerp(e, f);
        draw_circle(dot.x, dot.y, 5.0, Color::new(1.0, 0.9, 0.5, 0.9));
    }
    if layout == Layout::Compare && n >= 2 && shown(1) {
        for i in 0..n - 1 {
            let m = (pts[i] + pts[i + 1]) / 2.0;
            ink.centered(ink.ui(), "VS", m.x, m.y + 12.0, 34.0, Color::new(1.0, 0.82, 0.35, 1.0));
        }
    }
    for (i, v) in vs.iter().enumerate() {
        if !shown(i) {
            continue;
        }
        let s = scale(i);
        let bob = (time * 1.6 + i as f32).sin() * 5.0;
        let c = pts[i] + vec2(0.0, bob);
        let col = PALETTE[(i + si) % PALETTE.len()];
        for (k, a) in [(1.6, 0.06), (1.35, 0.1), (1.15, 0.16)] {
            fill_shape(v.shape, c, r * s * k, Color::new(col.r, col.g, col.b, a));
        }
        fill_shape(v.shape, c, r * s, col);
        fill_shape(v.shape, c + vec2(-r * 0.15, -r * 0.18) * s, r * s * 0.55, Color::new(1.0, 1.0, 1.0, 0.18));
        if s > 0.6 {
            let rows = ink.wrap(ink.ui(), &v.label, 200.0, 20.0);
            for (k, row) in rows.iter().enumerate() {
                ink.centered(ink.ui(), row, c.x, c.y + r * 1.2 + 26.0 + k as f32 * 24.0, 20.0, WHITE);
            }
        }
    }
}

/// "Blip", an original round mascot that bounces and reacts.
fn mascot(p: &Player, pos: Vec2, time: f32, size: f32) {
    let talking = p.teacher.talking;
    let quiz = matches!(p.phase, Phase::Quiz { .. });
    let done = matches!(p.phase, Phase::Done);
    let hop = if done { (time * 6.0).sin().abs() * 30.0 } else if talking { (time * 5.0).sin().abs() * 8.0 } else { 0.0 };
    let squash = 1.0 + (time * 2.4).sin() * 0.03;
    let c = pos - vec2(0.0, hop);
    let r = 34.0 * size;
    let body = Color::new(1.0, 0.82, 0.35, 1.0);
    draw_ellipse(pos.x, pos.y + r * 0.95, r * 0.8, r * 0.18, 0.0, Color::new(0.0, 0.0, 0.0, 0.25));
    draw_line(c.x, c.y - r, c.x + 8.0 * size, c.y - r - 22.0 * size, 3.0 * size, body);
    draw_circle(c.x + 8.0 * size, c.y - r - 24.0 * size, 6.0 * size, Color::new(1.0, 0.42, 0.42, 1.0));
    draw_ellipse(c.x, c.y, r * squash, r / squash, 0.0, body);
    draw_ellipse(c.x - r * 0.25, c.y - r * 0.3, r * 0.4, r * 0.25, -0.4, Color::new(1.0, 1.0, 1.0, 0.35));
    let look = if quiz { vec2(0.0, -3.0) } else { vec2(3.0, 0.0) } * size;
    for ex in [-0.32, 0.32] {
        let e = c + vec2(ex * r, -0.08 * r);
        draw_circle(e.x, e.y, 9.0 * size, WHITE);
        draw_circle(e.x + look.x, e.y + look.y, 4.5 * size, Color::new(0.1, 0.08, 0.2, 1.0));
    }
    let m = c + vec2(0.0, r * 0.4);
    if talking {
        let open = ((time * 14.0).sin() * 0.5 + 0.5) * 7.0 * size;
        draw_ellipse(m.x, m.y, 8.0 * size, 2.0 + open, 0.0, Color::new(0.35, 0.1, 0.15, 1.0));
    } else {
        draw_ellipse(m.x, m.y, 9.0 * size, 3.0 * size, 0.0, Color::new(0.35, 0.1, 0.15, 1.0));
        draw_ellipse(m.x, m.y - 2.0 * size, 10.0 * size, 3.0 * size, 0.0, body);
    }
}

// ---------------------------------------------------------------- whiteboard

const MARKER_BLUE: Color = Color::new(0.12, 0.3, 0.75, 1.0);
const MARKER_RED: Color = Color::new(0.8, 0.15, 0.15, 1.0);
const MARKER_GREEN: Color = Color::new(0.1, 0.55, 0.3, 1.0);

pub fn whiteboard(p: &Player, ink: &Ink, time: f32) {
    draw_rectangle(0.0, 0.0, W, H, Color::new(0.88, 0.87, 0.84, 1.0));
    let board = Rect::new(36.0, 100.0, W - 72.0, 520.0);
    draw_rectangle(board.x - 8.0, board.y - 8.0, board.w + 16.0, board.h + 16.0, Color::new(0.7, 0.72, 0.75, 1.0));
    draw_rectangle(board.x, board.y, board.w, board.h, WHITE);
    draw_rectangle(board.x + 200.0, board.y + board.h + 8.0, board.w - 400.0, 10.0, Color::new(0.6, 0.62, 0.65, 1.0));
    // markers on the tray
    for (i, c) in [MARKER_BLUE, MARKER_RED, MARKER_GREEN].iter().enumerate() {
        let x = board.x + 260.0 + i as f32 * 70.0;
        draw_rectangle(x, board.y + board.h + 1.0, 50.0, 8.0, *c);
    }

    if matches!(p.phase, Phase::Done) {
        return;
    }
    let scene = &p.lesson.scenes[p.scene];
    let (now, total) = scene_clock(p);

    // heading writes itself, then gets underlined
    let chars = (now * 28.0) as usize;
    let heading: String = scene.heading.chars().take(chars).collect();
    ink.draw(ink.hand(), &heading, board.x + 40.0, board.y + 58.0, 38.0, MARKER_RED);
    let full_w = ink.width(ink.hand(), &scene.heading, 38.0);
    let ul = ((now - scene.heading.len() as f32 / 28.0) / 0.4).clamp(0.0, 1.0);
    if ul > 0.0 {
        wobbly_line(vec2(board.x + 38.0, board.y + 72.0), vec2(board.x + 38.0 + full_w * ul, board.y + 74.0), 4.0, MARKER_RED, 7);
    }

    // notes on the left, hand-written at marker speed
    let notes_w = if scene.visuals.is_empty() { board.w - 100.0 } else { 520.0 };
    let mut y = board.y + 120.0;
    let n = scene.board.len();
    for (i, note) in scene.board.iter().enumerate() {
        let at = 1.0 + reveal_at(i, n, total, 0.7);
        if now < at {
            break;
        }
        let shown = ((now - at) * 32.0) as usize;
        let rows = ink.wrap(ink.hand(), note, notes_w - 40.0, 24.0);
        let mut left = shown;
        wobbly_line(vec2(board.x + 44.0, y - 8.0), vec2(board.x + 60.0, y - 8.0), 3.5, MARKER_BLUE, 1);
        for row in rows {
            if y > board.y + board.h - 20.0 {
                break;
            }
            let part: String = row.chars().take(left).collect();
            left = left.saturating_sub(row.chars().count() + 1);
            ink.draw(ink.hand(), &part, board.x + 72.0, y, 24.0, MARKER_BLUE);
            y += 32.0;
        }
        y += 12.0;
    }

    // doodles on the right, drawn stroke by stroke
    if !scene.visuals.is_empty() {
        let area = Rect::new(660.0, 190.0, 540.0, 330.0);
        let vs = &scene.visuals;
        let k = vs.len();
        let (pts, edges) = layout_points(scene.layout, k, area);
        let r = if k <= 3 { 58.0 } else { 46.0 };
        let at = |i: usize| 0.5 + reveal_at(i, k, total, 0.75);
        for (i, v) in vs.iter().enumerate() {
            if now < at(i) {
                continue;
            }
            let prog = ((now - at(i)) / 0.9).clamp(0.0, 1.0);
            let col = if i == 0 && scene.layout == Layout::Hub { MARKER_RED } else { INK };
            sketch_shape(v.shape, pts[i], r, prog, col, i as u32 + p.scene as u32 * 10);
            if prog > 0.6 {
                let rows = ink.wrap(ink.hand(), &v.label, 170.0, 20.0);
                for (j, row) in rows.iter().enumerate() {
                    ink.centered(ink.hand(), row, pts[i].x, pts[i].y + r * 1.15 + 22.0 + j as f32 * 24.0, 20.0, col);
                }
            }
        }
        for &(a, b) in &edges {
            let start = at(a.max(b)) + 0.6;
            if now < start {
                continue;
            }
            let g = ((now - start) / 0.4).clamp(0.0, 1.0);
            let d = (pts[b] - pts[a]).normalize_or_zero();
            let s = pts[a] + d * (r + 12.0);
            let e = s.lerp(pts[b] - d * (r + 12.0), g);
            wobbly_line(s, e, 3.5, MARKER_GREEN, 5);
            if g >= 1.0 {
                let n = vec2(-d.y, d.x);
                wobbly_line(e, e - d * 18.0 + n * 10.0, 3.5, MARKER_GREEN, 1);
                wobbly_line(e, e - d * 18.0 - n * 10.0, 3.5, MARKER_GREEN, 1);
            }
        }
        if scene.layout == Layout::Compare && k >= 2 && now > at(1) {
            for i in 0..k - 1 {
                let m = (pts[i] + pts[i + 1]) / 2.0;
                ink.centered(ink.hand(), "vs", m.x, m.y + 10.0, 30.0, MARKER_RED);
            }
        }
    }
    let _ = time;
}

fn wobbly_line(a: Vec2, b: Vec2, thick: f32, c: Color, segs: usize) {
    let n = segs.max(1);
    let d = b - a;
    let perp = vec2(-d.y, d.x).normalize_or_zero();
    let mut prev = a;
    for i in 1..=n {
        let t = i as f32 / n as f32;
        let wob = if i == n { 0.0 } else { (rnd((a.x + a.y) as u32 + i as u32) - 0.5) * 3.0 };
        let p = a + d * t + perp * wob;
        draw_line(prev.x, prev.y, p.x, p.y, thick, c);
        draw_circle(p.x, p.y, thick / 2.0, c);
        prev = p;
    }
}

fn sketch_shape(shape: Shape, c: Vec2, r: f32, prog: f32, col: Color, seed: u32) {
    let n = 72;
    let upto = (n as f32 * prog * 1.04) as usize;
    let jitter = |i: usize| 1.0 + (rnd(seed * 97 + (i % n) as u32) - 0.5) * 0.06;
    let mut prev = outline(shape, c, r * jitter(0), 0.0);
    for i in 1..=upto.min(n + 3) {
        let t = i as f32 / n as f32;
        let p = outline(shape, c, r * jitter(i), t.min(1.04) % 1.0001);
        draw_line(prev.x, prev.y, p.x, p.y, 4.0, col);
        draw_circle(p.x, p.y, 2.0, col);
        prev = p;
    }
}

// ---------------------------------------------------------------- chat

pub fn chat(p: &Player, ink: &Ink, time: f32) {
    let strips = 30;
    for i in 0..strips {
        let f = i as f32 / strips as f32;
        let c = Color::new(0.98 - 0.08 * f, 0.86 - 0.06 * f, 0.93, 1.0);
        draw_rectangle(0.0, H * f, W, H / strips as f32 + 1.0, c);
    }
    // floating bubbles in the background
    for i in 0..14u32 {
        let x = rnd(i * 5) * W;
        let y = (rnd(i * 5 + 1) * H - time * (8.0 + rnd(i) * 14.0)).rem_euclid(H + 80.0) - 40.0;
        draw_circle(x, y, 10.0 + rnd(i * 5 + 2) * 30.0, Color::new(1.0, 1.0, 1.0, 0.25));
    }

    // phone
    let ph = Rect::new(450.0, 100.0, 380.0, 612.0);
    rrect(ph.x, ph.y, ph.w, ph.h, 44.0, Color::new(0.08, 0.08, 0.1, 1.0));
    let sc = Rect::new(ph.x + 12.0, ph.y + 12.0, ph.w - 24.0, ph.h - 24.0);
    rrect(sc.x, sc.y, sc.w, sc.h, 34.0, Color::new(0.97, 0.97, 0.98, 1.0));
    rrect(ph.x + ph.w / 2.0 - 50.0, ph.y + 20.0, 100.0, 26.0, 13.0, Color::new(0.08, 0.08, 0.1, 1.0));

    // contact header
    let hy = sc.y + 46.0;
    draw_rectangle(sc.x, hy, sc.w, 62.0, Color::new(0.94, 0.94, 0.96, 1.0));
    draw_circle(sc.x + 40.0, hy + 31.0, 20.0, Color::new(0.82, 0.2, 0.22, 1.0));
    let initial: String = p.lesson.teacher_name.chars().take(1).collect();
    ink.centered(ink.ui(), &initial, sc.x + 40.0, hy + 39.0, 20.0, WHITE);
    ink.draw(ink.ui(), &p.lesson.teacher_name, sc.x + 72.0, hy + 28.0, 18.0, INK);
    let status = if p.teacher.talking { "typing..." } else { "online" };
    ink.draw(ink.ui(), status, sc.x + 72.0, hy + 48.0, 13.0, Color::new(0.45, 0.45, 0.5, 1.0));

    let scene = &p.lesson.scenes[p.scene];

    // sticky note with key points
    let note = Rect::new(80.0, 150.0, 320.0, 400.0);
    draw_rectangle(note.x + 6.0, note.y + 8.0, note.w, note.h, Color::new(0.0, 0.0, 0.0, 0.12));
    draw_rectangle(note.x, note.y, note.w, note.h, Color::new(1.0, 0.92, 0.45, 1.0));
    draw_circle(note.x + note.w / 2.0, note.y + 12.0, 9.0, Color::new(0.85, 0.2, 0.2, 1.0));
    ink.draw(ink.hand(), "KEY POINTS", note.x + 24.0, note.y + 52.0, 22.0, INK);
    let (now, total) = scene_clock(p);
    let mut y = note.y + 92.0;
    for (i, b) in scene.board.iter().enumerate() {
        if now < reveal_at(i, scene.board.len(), total, 0.8) || y > note.y + note.h - 20.0 {
            break;
        }
        draw_circle(note.x + 30.0, y - 7.0, 4.0, INK);
        for row in ink.wrap(ink.hand(), b, note.w - 70.0, 19.0) {
            ink.draw(ink.hand(), &row, note.x + 44.0, y, 19.0, INK);
            y += 25.0;
        }
        y += 10.0;
    }

    // scene heading on the right
    if !matches!(p.phase, Phase::Done) {
        let rows = ink.wrap(ink.hand(), &scene.heading, 330.0, 34.0);
        let mut hy2 = 260.0;
        for row in rows {
            ink.draw(ink.hand(), &row, 880.0, hy2, 34.0, Color::new(0.35, 0.2, 0.55, 1.0));
            hy2 += 42.0;
        }
    }

    // messages, newest at the bottom
    let thread_top = hy + 70.0;
    let mut bottom = sc.y + sc.h - 24.0;
    let max_w = sc.w * 0.72;
    let last = if matches!(p.phase, Phase::Playing) { p.line } else { scene.lines.len() - 1 };

    // typing indicator during the pause before the next message
    if matches!(p.phase, Phase::Playing) && p.line + 1 < scene.lines.len() && p.t > scene.lines[p.line].duration {
        let next = scene.lines[p.line + 1].speaker;
        let mine = next == Speaker::Student;
        let bw = 70.0;
        let bx = if mine { sc.x + sc.w - 16.0 - bw } else { sc.x + 16.0 };
        rrect(bx, bottom - 38.0, bw, 38.0, 19.0, if mine { STUD_COL } else { Color::new(0.88, 0.88, 0.91, 1.0) });
        for k in 0..3 {
            let a = 0.4 + 0.6 * ((time * 6.0 - k as f32 * 0.6).sin() * 0.5 + 0.5);
            let dc = if mine { Color::new(1.0, 1.0, 1.0, a) } else { Color::new(0.4, 0.4, 0.45, a) };
            draw_circle(bx + 20.0 + k as f32 * 15.0, bottom - 19.0, 4.5, dc);
        }
        bottom -= 50.0;
    }

    for li in (0..=last).rev() {
        let line = &scene.lines[li];
        let mine = line.speaker == Speaker::Student;
        let rows = ink.wrap(ink.ui(), &line.text, max_w - 28.0, 17.0);
        let widest = rows.iter().map(|r| ink.width(ink.ui(), r, 17.0)).fold(0.0, f32::max);
        let bw = widest + 28.0;
        let bh = rows.len() as f32 * 22.0 + 18.0;
        let pop = if li == p.line && matches!(p.phase, Phase::Playing) { ease_out_back(p.t / 0.3) } else { 1.0 };
        let top = bottom - bh;
        if top < thread_top {
            break;
        }
        let bx = if mine { sc.x + sc.w - 16.0 - bw } else { sc.x + 16.0 };
        let (bg, fg) = if mine { (STUD_COL, WHITE) } else { (Color::new(0.88, 0.88, 0.91, 1.0), INK) };
        let cx = if mine { bx + bw } else { bx };
        let sx = cx + (bx - cx) * pop;
        rrect(sx, top, bw * pop, bh, 17.0, bg);
        if pop > 0.85 {
            for (k, row) in rows.iter().enumerate() {
                ink.draw(ink.ui(), row, bx + 14.0, top + 27.0 + k as f32 * 22.0, 17.0, fg);
            }
        }
        bottom = top - 10.0;
    }
    let _ = CHALK;
}
