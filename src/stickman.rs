//! Procedural stickman: a tiny joint-angle rig, a set of poses, and drawing.
//!
//! Angles are measured from "straight down"; positive rotates toward the
//! direction the character faces. "f" = front arm/leg, "b" = back.

use crate::lesson::Pose;
use macroquad::prelude::*;

const S: f32 = 1.5; // overall character scale
const TORSO: f32 = 60.0 * S;
const UPPER_ARM: f32 = 33.0 * S;
const FOREARM: f32 = 32.0 * S;
const THIGH: f32 = 36.0 * S;
const SHIN: f32 = 35.0 * S;
pub const HEAD_R: f32 = 20.0 * S;
const THICK: f32 = 5.0;

pub const INK: Color = Color::new(0.10, 0.10, 0.13, 1.0);
pub const PAPER: Color = Color::new(0.985, 0.972, 0.945, 1.0);

#[derive(Clone, Copy, Default)]
pub struct Rig {
    fu: f32, // front upper arm
    fl: f32, // front forearm (relative to upper arm)
    bu: f32,
    bl: f32,
    ft: f32, // front thigh
    fs: f32, // front shin (relative)
    bt: f32,
    bs: f32,
    lean: f32,
    tilt: f32, // head tilt
    lift: f32, // vertical hop (negative = up)
}

impl Rig {
    fn idle() -> Rig {
        Rig { fu: 0.16, fl: -0.06, bu: -0.16, bl: 0.06, ft: 0.12, bt: -0.12, ..Default::default() }
    }
    fn to_arr(self) -> [f32; 11] {
        [self.fu, self.fl, self.bu, self.bl, self.ft, self.fs, self.bt, self.bs, self.lean, self.tilt, self.lift]
    }
    fn from_arr(a: [f32; 11]) -> Rig {
        Rig { fu: a[0], fl: a[1], bu: a[2], bl: a[3], ft: a[4], fs: a[5], bt: a[6], bs: a[7], lean: a[8], tilt: a[9], lift: a[10] }
    }
    fn approach(&mut self, target: Rig, k: f32) {
        let mut a = self.to_arr();
        let b = target.to_arr();
        for i in 0..11 {
            a[i] += (b[i] - a[i]) * k;
        }
        *self = Rig::from_arr(a);
    }
}

pub fn pose_rig(p: Pose, t: f32) -> Rig {
    let mut r = Rig::idle();
    match p {
        Pose::Idle | Pose::Unknown => {
            let sway = (t * 1.3).sin() * 0.05;
            r.fu += sway;
            r.bu -= sway;
        }
        Pose::Explain => {
            r.fu = 0.95 + (t * 2.6).sin() * 0.18;
            r.fl = 0.45 + (t * 3.1).sin() * 0.22;
            r.bu = -0.9 - (t * 2.2).sin() * 0.12;
            r.bl = -0.45;
            r.lean = 0.03;
        }
        Pose::Point => {
            r.fu = 1.8 + (t * 2.0).sin() * 0.04;
            r.fl = 0.05 + (t * 4.0).sin() * 0.06;
            r.bu = -0.6;
            r.bl = 1.6;
            r.lean = 0.06;
        }
        Pose::Wave => {
            r.fu = 2.55;
            r.fl = 0.35 + (t * 9.0).sin() * 0.55;
        }
        Pose::Think => {
            r.fu = 0.55;
            r.fl = 2.75;
            r.bu = -0.15;
            r.bl = 1.25;
            r.tilt = 0.18 + (t * 1.1).sin() * 0.04;
            r.lean = -0.03;
        }
        Pose::Excited => {
            let j = (t * 7.0).sin().abs();
            let k = 1.0 - j;
            r.fu = 2.55;
            r.fl = 0.25;
            r.bu = -2.55;
            r.bl = -0.25;
            r.lift = -22.0 * j;
            r.ft = 0.12 + 0.35 * k;
            r.fs = -0.7 * k;
            r.bt = -0.12 + 0.35 * k;
            r.bs = -0.7 * k;
        }
        Pose::Shrug => {
            r.fu = 0.75;
            r.fl = 1.15;
            r.bu = -0.75;
            r.bl = -1.15;
            r.tilt = 0.15;
        }
        Pose::Confused => {
            r.bu = -2.3;
            r.bl = -1.88 + (t * 10.0).sin() * 0.25;
            r.fu = 0.1;
            r.tilt = -0.18;
        }
    }
    r
}

#[derive(Clone, Copy, PartialEq)]
pub enum Look {
    Teacher,
    Student,
}

pub struct Actor {
    pub x: f32,
    pub floor_y: f32,
    pub facing: f32,
    pub pose: Pose,
    pub talking: bool,
    pub look: Look,
    rig: Rig,
    blink_at: f32,
}

impl Actor {
    pub fn new(x: f32, floor_y: f32, facing: f32, look: Look) -> Actor {
        Actor {
            x,
            floor_y,
            facing,
            pose: Pose::Idle,
            talking: false,
            look,
            rig: Rig::idle(),
            blink_at: 2.0,
        }
    }

    pub fn update(&mut self, dt: f32, t: f32) {
        let target = pose_rig(self.pose, t + self.x * 0.01);
        self.rig.approach(target, (dt * 9.0).min(1.0));
        if t > self.blink_at + 0.15 {
            self.blink_at = t + 2.0 + ((t * 7.31).sin().abs() * 3.0);
        }
    }

    pub fn draw(&self, t: f32) {
        let r = &self.rig;
        let f = self.facing;
        let dir = |a: f32| vec2(a.sin() * f, a.cos());

        // Feet relative to hip, so we can plant the lowest foot on the floor.
        let fk = dir(r.ft) * THIGH;
        let ff = fk + dir(r.ft + r.fs) * SHIN;
        let bk = dir(r.bt) * THIGH;
        let bf = bk + dir(r.bt + r.bs) * SHIN;
        let breathe = (t * 2.2 + self.x).sin() * 1.5;
        let hip = vec2(self.x, self.floor_y - ff.y.max(bf.y) + r.lift);

        let neck = hip + dir(std::f32::consts::PI + r.lean) * (TORSO + breathe);
        let shoulder = neck + (hip - neck) * 0.1;
        let head = neck + dir(std::f32::consts::PI + r.lean + r.tilt * 0.5) * (HEAD_R + 5.0);

        let seg = |a: Vec2, b: Vec2| {
            draw_line(a.x, a.y, b.x, b.y, THICK, INK);
            draw_circle(b.x, b.y, THICK / 2.0, INK);
        };

        // back limbs first so front limbs overlap them
        let be = shoulder + dir(r.bu) * UPPER_ARM;
        let bh = be + dir(r.bu + r.bl) * FOREARM;
        seg(hip, hip + bk);
        seg(hip + bk, hip + bf);
        seg(shoulder, be);
        seg(be, bh);

        seg(hip, neck);
        draw_circle(hip.x, hip.y, THICK / 2.0, INK);

        seg(hip, hip + fk);
        seg(hip + fk, hip + ff);

        if self.look == Look::Teacher {
            // little tie
            let a = neck + vec2(0.0, 4.0);
            draw_triangle(a, a + vec2(-6.0, 26.0), a + vec2(6.0, 26.0), Color::new(0.82, 0.2, 0.22, 1.0));
        }

        self.draw_head(head, r.tilt, t);

        let fe = shoulder + dir(r.fu) * UPPER_ARM;
        let fh = fe + dir(r.fu + r.fl) * FOREARM;
        seg(shoulder, fe);
        seg(fe, fh);
        draw_circle(fh.x, fh.y, THICK * 0.9, INK);
        draw_circle(bh.x, bh.y, THICK * 0.9, INK);
    }

    fn draw_head(&self, c: Vec2, tilt: f32, t: f32) {
        let f = self.facing;
        let rot = |v: Vec2| {
            let (s, co) = (tilt * f).sin_cos();
            c + vec2(v.x * co - v.y * s, v.x * s + v.y * co)
        };
        draw_circle(c.x, c.y, HEAD_R, PAPER);

        if self.look == Look::Student {
            // baseball cap: filled top half + brim
            let cap = Color::new(0.2, 0.45, 0.85, 1.0);
            let n = 16;
            for i in 0..n {
                let a0 = std::f32::consts::PI + std::f32::consts::PI * i as f32 / n as f32;
                let a1 = std::f32::consts::PI + std::f32::consts::PI * (i + 1) as f32 / n as f32;
                let p0 = rot(vec2(a0.cos(), a0.sin() - 0.12) * HEAD_R);
                let p1 = rot(vec2(a1.cos(), a1.sin() - 0.12) * HEAD_R);
                draw_triangle(rot(vec2(0.0, -0.12 * HEAD_R)), p0, p1, cap);
            }
            let b0 = rot(vec2(0.2 * f * HEAD_R, -0.2 * HEAD_R));
            let b1 = rot(vec2(1.45 * f * HEAD_R, -0.1 * HEAD_R));
            draw_line(b0.x, b0.y, b1.x, b1.y, 7.0, cap);
        }

        draw_circle_lines(c.x, c.y, HEAD_R, THICK, INK);

        // eyes (looking toward facing side)
        let blinking = t > self.blink_at && t < self.blink_at + 0.15;
        for ex in [0.02, 0.56] {
            let e = rot(vec2(ex * f * HEAD_R, -0.05 * HEAD_R));
            if blinking {
                draw_line(e.x - 4.0, e.y, e.x + 4.0, e.y, 2.5, INK);
            } else {
                draw_circle(e.x, e.y, 3.6, INK);
            }
            if self.look == Look::Teacher {
                draw_circle_lines(e.x, e.y, 7.5, 2.0, INK);
            }
        }
        if self.look == Look::Teacher {
            let a = rot(vec2(0.02 * f * HEAD_R + 7.5 * f, -0.05 * HEAD_R));
            let b = rot(vec2(0.56 * f * HEAD_R - 7.5 * f, -0.05 * HEAD_R));
            draw_line(a.x, a.y, b.x, b.y, 2.0, INK);
        }

        // mouth
        let m = rot(vec2(0.36 * f * HEAD_R, 0.42 * HEAD_R));
        if self.talking {
            let open = ((t * 15.0).sin() * 0.5 + 0.5) * (0.55 + 0.45 * (t * 5.3).sin().abs());
            draw_ellipse(m.x, m.y, 6.5, 1.5 + open * 6.5, 0.0, INK);
        } else {
            let smile = match self.pose {
                Pose::Excited => 5.0,
                Pose::Confused | Pose::Shrug => -2.0,
                Pose::Think => 0.0,
                _ => 2.5,
            };
            let l = vec2(m.x - 7.0, m.y - smile * 0.5);
            let r = vec2(m.x + 7.0, m.y - smile * 0.5);
            let mid = vec2(m.x, m.y + smile * 0.5);
            draw_line(l.x, l.y, mid.x, mid.y, 2.5, INK);
            draw_line(mid.x, mid.y, r.x, r.y, 2.5, INK);
        }
    }
}
