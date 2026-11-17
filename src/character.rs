//! The doodle cast: a joint-angle rig with chunky outlined limbs, expressive
//! faces, blinking and lip-sync. Stateless: everything is a function of time,
//! so frames can be rendered in any order (and in parallel).

use crate::gfx::{ease_in_out, Col, Gfx};
use crate::lesson::Pose;
use crate::styles::Theme;

const TORSO: f32 = 112.0;
const UPPER_ARM: f32 = 60.0;
const FOREARM: f32 = 58.0;
const THIGH: f32 = 64.0;
const SHIN: f32 = 62.0;
pub const HEAD_R: f32 = 48.0;
/// Approximate full height at scale 1 (feet to top of head).
pub const HEIGHT: f32 = TORSO + THIGH + SHIN + HEAD_R * 2.0 + 10.0;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Look {
    Teacher,
    Student,
    Host,
}

#[derive(Clone, Copy, Default)]
struct Rig {
    fu: f32,
    fl: f32,
    bu: f32,
    bl: f32,
    ft: f32,
    fs: f32,
    bt: f32,
    bs: f32,
    lean: f32,
    tilt: f32,
    lift: f32,
}

impl Rig {
    fn idle() -> Rig {
        Rig { fu: 0.18, fl: -0.08, bu: -0.18, bl: 0.08, ft: 0.13, bt: -0.13, ..Default::default() }
    }
    fn lerp(self, o: Rig, k: f32) -> Rig {
        let l = |a: f32, b: f32| a + (b - a) * k;
        Rig {
            fu: l(self.fu, o.fu),
            fl: l(self.fl, o.fl),
            bu: l(self.bu, o.bu),
            bl: l(self.bl, o.bl),
            ft: l(self.ft, o.ft),
            fs: l(self.fs, o.fs),
            bt: l(self.bt, o.bt),
            bs: l(self.bs, o.bs),
            lean: l(self.lean, o.lean),
            tilt: l(self.tilt, o.tilt),
            lift: l(self.lift, o.lift),
        }
    }
}

fn pose_rig(p: Pose, t: f32) -> Rig {
    let mut r = Rig::idle();
    match p {
        Pose::Idle | Pose::Unknown => {
            let sway = (t * 1.4).sin() * 0.06;
            r.fu += sway;
            r.bu -= sway;
        }
        Pose::Explain => {
            r.fu = 1.0 + (t * 2.7).sin() * 0.2;
            r.fl = 0.5 + (t * 3.3).sin() * 0.25;
            r.bu = -0.95 - (t * 2.3).sin() * 0.14;
            r.bl = -0.5;
            r.lean = 0.04;
        }
        Pose::Point => {
            r.fu = 1.85 + (t * 2.0).sin() * 0.05;
            r.fl = 0.05 + (t * 4.0).sin() * 0.07;
            r.bu = -0.55;
            r.bl = 1.55;
            r.lean = 0.07;
        }
        Pose::Wave => {
            r.fu = 2.6;
            r.fl = 0.35 + (t * 10.0).sin() * 0.6;
        }
        Pose::Think => {
            r.fu = 0.55;
            r.fl = 2.75;
            r.bu = -0.15;
            r.bl = 1.25;
            r.tilt = 0.2 + (t * 1.2).sin() * 0.05;
            r.lean = -0.03;
        }
        Pose::Excited | Pose::Cheer => {
            let j = (t * 7.5).sin().abs();
            let k = 1.0 - j;
            r.fu = 2.6 + (t * 7.5).sin() * 0.1;
            r.fl = 0.25;
            r.bu = -2.6;
            r.bl = -0.25;
            r.lift = -30.0 * j;
            r.ft = 0.13 + 0.38 * k;
            r.fs = -0.75 * k;
            r.bt = -0.13 + 0.38 * k;
            r.bs = -0.75 * k;
            if p == Pose::Excited {
                r.fu = 1.4;
                r.fl = 1.9;
                r.bu = -1.4;
                r.bl = -1.9;
            }
        }
        Pose::Shrug => {
            r.fu = 0.8;
            r.fl = 1.2;
            r.bu = -0.8;
            r.bl = -1.2;
            r.tilt = 0.16;
        }
        Pose::Confused => {
            r.bu = -2.3;
            r.bl = -1.9 + (t * 11.0).sin() * 0.25;
            r.fu = 0.1;
            r.tilt = -0.2;
        }
        Pose::Shocked => {
            r.fu = 2.3;
            r.fl = 2.2;
            r.bu = -2.3;
            r.bl = -2.2;
            r.lean = -0.12;
            r.lift = -6.0 * (t * 30.0).sin().abs();
        }
        Pose::Facepalm => {
            r.fu = 0.9;
            r.fl = 2.6;
            r.bu = -0.2;
            r.tilt = 0.35;
            r.lean = 0.08;
        }
        Pose::Sad => {
            r.fu = 0.05;
            r.bu = -0.05;
            r.fl = 0.0;
            r.bl = 0.0;
            r.tilt = 0.32;
            r.lean = 0.1;
        }
    }
    r
}

#[derive(Clone, Copy)]
pub struct Actor {
    pub look: Look,
    pub x: f32,
    pub floor: f32,
    pub scale: f32,
    /// +1 faces right, -1 faces left
    pub facing: f32,
    pub pose: Pose,
    pub prev_pose: Pose,
    /// Seconds since `pose` was set (blends from `prev_pose`).
    pub since: f32,
    /// Mouth openness 0..1 (lip-sync)
    pub mouth: f32,
    pub talking: bool,
    /// Seed so two actors don't blink in sync
    pub seed: f32,
}

impl Actor {
    pub fn new(look: Look, x: f32, floor: f32, scale: f32, facing: f32) -> Actor {
        Actor {
            look,
            x,
            floor,
            scale,
            facing,
            pose: Pose::Idle,
            prev_pose: Pose::Idle,
            since: 10.0,
            mouth: 0.0,
            talking: false,
            seed: if look == Look::Student { 1.7 } else { 0.0 },
        }
    }

    pub fn draw(&self, g: &mut Gfx, th: &Theme, t: f32) {
        let k = ease_in_out(self.since / 0.28);
        let tt = t + self.seed;
        let r = pose_rig(self.prev_pose, tt).lerp(pose_rig(self.pose, tt), k);
        g.save();
        g.translate(self.x, self.floor);
        g.scale(self.scale);
        // squash & stretch on pose changes
        if self.since < 0.35 {
            let q = (self.since / 0.35 * std::f32::consts::PI).sin() * 0.06;
            g.scale_xy(1.0 + q, 1.0 - q);
        }
        self.draw_body(g, th, r, tt);
        g.restore();
    }

    fn draw_body(&self, g: &mut Gfx, th: &Theme, r: Rig, t: f32) {
        let f = self.facing;
        let dir = |a: f32| (a.sin() * f, a.cos());
        let add = |p: (f32, f32), d: (f32, f32), l: f32| (p.0 + d.0 * l, p.1 + d.1 * l);

        let breathe = (t * 2.3).sin() * 2.0;
        let fk = add((0.0, 0.0), dir(r.ft), THIGH);
        let ff = add(fk, dir(r.ft + r.fs), SHIN);
        let bk = add((0.0, 0.0), dir(r.bt), THIGH);
        let bf = add(bk, dir(r.bt + r.bs), SHIN);
        let hip = (0.0, -ff.1.max(bf.1) + r.lift);
        let neck = add(hip, dir(std::f32::consts::PI + r.lean), TORSO + breathe);
        let sh = (neck.0 + (hip.0 - neck.0) * 0.12, neck.1 + (hip.1 - neck.1) * 0.12);
        let head = add(neck, dir(std::f32::consts::PI + r.lean + r.tilt * 0.5), HEAD_R + 4.0);

        let o = 4.6;
        let limb = |g: &mut Gfx, pts: &[(f32, f32)], w: f32| {
            g.line(pts, th.ink, w);
        };
        let limb_fill = |g: &mut Gfx, pts: &[(f32, f32)], w: f32| {
            g.line(pts, th.body, w - 2.0 * o);
        };
        let p = |a: (f32, f32)| (hip.0 + a.0, hip.1 + a.1);

        // ground shadow
        g.ellipse(0.0, 2.0, 62.0 - r.lift.abs() * 0.6, 9.0, Some(th.ink.a(0.12)), None);

        let be = add(sh, dir(r.bu), UPPER_ARM);
        let bh = add(be, dir(r.bu + r.bl), FOREARM);
        let fe = add(sh, dir(r.fu), UPPER_ARM);
        let fh = add(fe, dir(r.fu + r.fl), FOREARM);

        // back limbs, torso, front limbs: outline pass then fill pass per layer
        let back: Vec<(Vec<(f32, f32)>, f32)> =
            vec![(vec![hip, p(bk), p(bf)], 20.0), (vec![sh, be, bh], 18.0)];
        for (pts, w) in &back {
            limb(g, pts, *w);
        }
        for (pts, w) in &back {
            limb_fill(g, pts, *w);
        }
        let front_legs = vec![hip, p(fk), p(ff)];
        limb(g, &[neck, hip], 32.0);
        limb(g, &front_legs, 20.0);
        limb_fill(g, &[neck, hip], 32.0);
        limb_fill(g, &front_legs, 20.0);

        // costume on the torso
        match self.look {
            Look::Teacher => {
                let a = (neck.0, neck.1 + 10.0);
                let tie = [a, (a.0 - 9.0, a.1 + 14.0), (a.0, a.1 + 50.0), (a.0 + 9.0, a.1 + 14.0)];
                g.poly(&tie, Some(th.accent(2)), Some((th.ink, 3.0)));
            }
            Look::Host => {
                // little star badge
                let c = (neck.0 + 2.0 * f, neck.1 + 34.0);
                star(g, c, 11.0, th.gold, th.ink);
            }
            Look::Student => {}
        }

        self.draw_head(g, th, head, r.tilt, t);

        let fa = vec![sh, fe, fh];
        limb(g, &fa, 18.0);
        limb_fill(g, &fa, 18.0);
        for hnd in [bh, fh] {
            g.circle(hnd.0, hnd.1, 11.5, Some(th.body), Some((th.ink, o)));
        }
    }

    fn draw_head(&self, g: &mut Gfx, th: &Theme, c: (f32, f32), tilt: f32, t: f32) {
        let f = self.facing;
        let (s, co) = (tilt * f).sin_cos();
        let rot = |x: f32, y: f32| (c.0 + x * co - y * s, c.1 + x * s + y * co);
        let o = 4.6;

        if self.look == Look::Host {
            // hair tuft
            for (dx, lean) in [(-0.28f32, -0.25f32), (0.0, 0.0), (0.28, 0.25)] {
                let a = rot(dx * HEAD_R, -HEAD_R * 0.9);
                let b = rot((dx + lean) * HEAD_R, -HEAD_R * 1.38);
                g.seg(a, b, th.ink, o * 1.1);
            }
        }
        g.circle(c.0, c.1, HEAD_R, Some(th.body), Some((th.ink, o)));

        if self.look == Look::Student {
            let cap = th.accent(1);
            let mut pts = vec![];
            for i in 0..=16 {
                let a = std::f32::consts::PI + std::f32::consts::PI * i as f32 / 16.0;
                pts.push(rot(a.cos() * HEAD_R * 1.02, a.sin() * HEAD_R * 1.02 - 0.1 * HEAD_R));
            }
            g.poly(&pts, Some(cap), Some((th.ink, o * 0.8)));
            let b0 = rot(0.3 * f * HEAD_R, -0.12 * HEAD_R);
            let b1 = rot(1.55 * f * HEAD_R, -0.06 * HEAD_R);
            g.seg(b0, b1, th.ink, 13.0);
            g.seg(b0, b1, cap, 7.0);
        }

        // eyes look toward the facing side
        let blink_phase = (t * 0.37 + self.seed * 0.13).fract();
        let blinking = blink_phase > 0.965;
        let surprised = matches!(self.pose, Pose::Shocked);
        let eye_h = if surprised { 13.0 } else { 10.0 };
        for ex in [0.05f32, 0.58] {
            let e = rot(ex * f * HEAD_R, -0.08 * HEAD_R);
            if blinking {
                g.seg((e.0 - 6.0, e.1), (e.0 + 6.0, e.1), th.ink, 3.5);
            } else {
                g.ellipse(e.0, e.1, 5.6, eye_h, Some(th.ink), None);
                g.circle(e.0 + 1.5 * f, e.1 - 4.0, 1.8, Some(th.body), None);
            }
            if self.look == Look::Teacher {
                g.circle(e.0, e.1, 13.0, None, Some((th.ink, 3.0)));
            }
            // eyebrows for strong emotions
            let brow = match self.pose {
                Pose::Think | Pose::Confused => Some((-4.0, 0.25 * if ex < 0.3 { 1.0 } else { -1.0 })),
                Pose::Shocked => Some((-10.0, 0.0)),
                Pose::Sad => Some((-2.0, -0.3 * if ex < 0.3 { 1.0 } else { -1.0 } * f)),
                Pose::Excited | Pose::Cheer => Some((-7.0, 0.0)),
                _ => None,
            };
            if let Some((dy, slope)) = brow {
                let y = e.1 - 20.0 + dy;
                g.seg((e.0 - 8.0, y - slope * 8.0), (e.0 + 8.0, y + slope * 8.0), th.ink, 3.5);
            }
        }
        if self.look == Look::Teacher {
            let a = rot(0.05 * f * HEAD_R + 13.0 * f, -0.08 * HEAD_R);
            let b = rot(0.58 * f * HEAD_R - 13.0 * f, -0.08 * HEAD_R);
            g.seg(a, b, th.ink, 3.0);
        }

        // mouth
        let m = rot(0.36 * f * HEAD_R, 0.42 * HEAD_R);
        if self.talking && self.mouth > 0.04 {
            let open = 2.0 + self.mouth * 13.0;
            g.ellipse(m.0, m.1 + open * 0.2, 9.0 + self.mouth * 2.0, open, Some(th.ink), None);
            if open > 7.0 {
                g.ellipse(m.0, m.1 + open * 0.75, 5.0, open * 0.28, Some(th.accent(2).lighten(0.2)), None);
            }
        } else {
            match self.pose {
                Pose::Excited | Pose::Cheer => {
                    // big open grin
                    let pts: Vec<(f32, f32)> = (0..=12)
                        .map(|i| {
                            let a = std::f32::consts::PI * i as f32 / 12.0;
                            (m.0 - 13.0 * a.cos(), m.1 - 3.0 + 13.0 * a.sin())
                        })
                        .collect();
                    g.poly(&pts, Some(th.ink), None);
                }
                Pose::Shocked => g.ellipse(m.0, m.1 + 3.0, 7.0, 10.0, Some(th.ink), None),
                Pose::Confused => {
                    let pts: Vec<(f32, f32)> =
                        (0..=8).map(|i| (m.0 - 11.0 + i as f32 * 2.75, m.1 + (i as f32 * 1.4).sin() * 3.0)).collect();
                    g.line(&pts, th.ink, 3.2);
                }
                Pose::Sad | Pose::Facepalm => smile(g, m, -5.0, th.ink),
                Pose::Think | Pose::Shrug => smile(g, m, 0.5, th.ink),
                _ => smile(g, m, 5.0, th.ink),
            }
        }
        let _ = Col(0.0, 0.0, 0.0, 0.0);
    }
}

fn smile(g: &mut Gfx, m: (f32, f32), curve: f32, ink: Col) {
    let pts = crate::gfx::bezier((m.0 - 12.0, m.1 - curve * 0.4), (m.0, m.1 + curve * 1.4), (m.0 + 12.0, m.1 - curve * 0.4), 10);
    g.line(&pts, ink, 3.4);
}

pub fn star(g: &mut Gfx, c: (f32, f32), r: f32, fill: Col, ink: Col) {
    let pts: Vec<(f32, f32)> = (0..10)
        .map(|i| {
            let a = -std::f32::consts::FRAC_PI_2 + std::f32::consts::PI * i as f32 / 5.0;
            let rr = if i % 2 == 0 { r } else { r * 0.45 };
            (c.0 + a.cos() * rr, c.1 + a.sin() * rr)
        })
        .collect();
    g.poly(&pts, Some(fill), Some((ink, 2.5)));
}
