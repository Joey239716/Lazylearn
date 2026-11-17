//! The prop library: line-art icons Claude can name in a shot ("atom", "money",
//! "brain"...). Each icon is drawn in a 200x200 box centered on (0, 0) with the
//! style's outline, and some of them move a little (gears turn, hearts beat).

use crate::gfx::{bezier, hex, Align, Col, Font, Gfx};
use crate::styles::Theme;
use std::f32::consts::{PI, TAU};

/// Canonical icon names, listed in the script prompt.
pub const ICON_NAMES: &[&str] = &[
    "atom", "molecule", "dna", "cell", "virus", "pill", "heart", "brain", "eye", "leaf", "tree", "sprout", "apple",
    "sun", "moon", "planet", "star", "cloud", "rain", "snowflake", "lightning", "fire", "water", "mountain",
    "globe", "flask", "microscope", "magnet", "battery", "plug", "thermometer", "gear", "hammer", "box",
    "rocket", "car", "plane", "ship", "house", "factory", "bank", "money", "chart_up", "chart_down", "scale",
    "key", "lock", "shield", "crown", "swords", "flag", "pin", "compass", "clock", "hourglass", "calendar",
    "book", "scroll", "document", "pencil", "graduation", "lightbulb", "puzzle", "target", "trophy", "check",
    "cross", "question", "warning", "skull", "magnifier", "megaphone", "bell", "chat", "mail", "person",
    "people", "robot", "computer", "phone", "code", "database", "wifi", "link", "recycle", "exchange",
    "music", "sound", "coffee",
];

fn canonical(name: &str) -> &'static str {
    let n: String = name.to_lowercase().chars().filter(|c| c.is_ascii_alphanumeric()).collect();
    let alias: &[(&str, &str)] = &[
        ("chartup", "chart_up"), ("growth", "chart_up"), ("increase", "chart_up"), ("profit", "chart_up"),
        ("chartdown", "chart_down"), ("decline", "chart_down"), ("decrease", "chart_down"), ("loss", "chart_down"),
        ("chart", "chart_up"), ("graph", "chart_up"), ("idea", "lightbulb"), ("bulb", "lightbulb"),
        ("energy", "lightning"), ("electricity", "lightning"), ("power", "lightning"), ("bolt", "lightning"),
        ("drop", "water"), ("droplet", "water"), ("ocean", "water"), ("sea", "water"), ("flame", "fire"),
        ("heat", "fire"), ("hot", "thermometer"), ("temperature", "thermometer"), ("cold", "snowflake"),
        ("ice", "snowflake"), ("earth", "globe"), ("world", "globe"), ("global", "globe"), ("home", "house"),
        ("industry", "factory"), ("government", "bank"), ("law", "scale"), ("justice", "scale"),
        ("balance", "scale"), ("coin", "money"), ("cash", "money"), ("dollar", "money"), ("price", "money"),
        ("cost", "money"), ("time", "clock"), ("deadline", "hourglass"), ("date", "calendar"),
        ("history", "scroll"), ("paper", "document"), ("page", "document"), ("file", "document"),
        ("note", "document"), ("notes", "document"), ("write", "pencil"), ("school", "graduation"),
        ("exam", "graduation"), ("study", "book"), ("read", "book"), ("knowledge", "book"), ("goal", "target"),
        ("aim", "target"), ("win", "trophy"), ("winner", "trophy"), ("award", "trophy"), ("correct", "check"),
        ("yes", "check"), ("true", "check"), ("wrong", "cross"), ("no", "cross"), ("false", "cross"),
        ("x", "cross"), ("why", "question"), ("unknown", "question"), ("alert", "warning"),
        ("danger", "warning"), ("caution", "warning"), ("death", "skull"), ("poison", "skull"),
        ("search", "magnifier"), ("zoom", "magnifier"), ("find", "magnifier"), ("announce", "megaphone"),
        ("talk", "chat"), ("speech", "chat"), ("message", "chat"), ("letter", "mail"), ("email", "mail"),
        ("user", "person"), ("human", "person"), ("individual", "person"), ("group", "people"),
        ("society", "people"), ("population", "people"), ("team", "people"), ("community", "people"),
        ("ai", "robot"), ("machine", "robot"), ("laptop", "computer"), ("pc", "computer"),
        ("software", "code"), ("program", "code"), ("data", "database"), ("storage", "database"),
        ("network", "wifi"), ("internet", "wifi"), ("signal", "wifi"), ("chain", "link"),
        ("connection", "link"), ("cycle", "recycle"), ("loop", "recycle"), ("trade", "exchange"),
        ("swap", "exchange"), ("handshake", "exchange"), ("deal", "exchange"), ("song", "music"),
        ("audio", "sound"), ("cup", "coffee"), ("tea", "coffee"), ("chemistry", "flask"), ("beaker", "flask"),
        ("experiment", "flask"), ("lab", "flask"), ("biology", "cell"), ("bacteria", "virus"),
        ("germ", "virus"), ("disease", "virus"), ("medicine", "pill"), ("drug", "pill"), ("health", "heart"),
        ("love", "heart"), ("mind", "brain"), ("think", "brain"), ("thinking", "brain"), ("see", "eye"),
        ("vision", "eye"), ("plant", "sprout"), ("seed", "sprout"), ("grow", "sprout"), ("forest", "tree"),
        ("nature", "leaf"), ("food", "apple"), ("fruit", "apple"), ("light", "sun"), ("sunlight", "sun"),
        ("night", "moon"), ("space", "planet"), ("universe", "planet"), ("weather", "cloud"),
        ("storm", "lightning"), ("tool", "hammer"), ("build", "hammer"), ("work", "gear"),
        ("process", "gear"), ("system", "gear"), ("mechanism", "gear"), ("engine", "gear"),
        ("package", "box"), ("container", "box"), ("product", "box"), ("goods", "box"), ("launch", "rocket"),
        ("fast", "rocket"), ("speed", "rocket"), ("travel", "plane"), ("transport", "car"), ("boat", "ship"),
        ("protect", "shield"), ("defense", "shield"), ("security", "lock"), ("secure", "lock"),
        ("secret", "lock"), ("unlock", "key"), ("solution", "key"), ("king", "crown"), ("queen", "crown"),
        ("ruler", "crown"), ("royal", "crown"), ("war", "swords"), ("battle", "swords"), ("conflict", "swords"),
        ("fight", "swords"), ("country", "flag"), ("nation", "flag"), ("location", "pin"), ("place", "pin"),
        ("map", "pin"), ("direction", "compass"), ("navigation", "compass"), ("notification", "bell"),
        ("reminder", "bell"), ("teacher", "graduation"), ("magnetism", "magnet"), ("attraction", "magnet"),
        ("charge", "battery"), ("rule", "scale"), ("formula", "flask"),
        ("genetics", "dna"), ("gene", "dna"), ("genes", "dna"), ("chemical", "molecule"), ("compound", "molecule"),
        ("element", "atom"), ("physics", "atom"), ("nucleus", "atom"), ("mountains", "mountain"),
        ("hill", "mountain"), ("rainy", "rain"), ("pieces", "puzzle"), ("problem", "puzzle"),
        ("solve", "puzzle"), ("trophy", "trophy"),
    ];
    for &c in ICON_NAMES {
        if n == c.replace('_', "") {
            return c;
        }
    }
    for (a, c) in alias {
        if n == *a {
            return c;
        }
    }
    // partial matches: "solar_energy" -> sun, "red_blood_cell" -> cell
    for &c in ICON_NAMES {
        let c2 = c.replace('_', "");
        if c2.len() >= 3 && n.contains(&c2) {
            return c;
        }
    }
    for (a, c) in alias {
        if a.len() >= 4 && n.contains(a) {
            return c;
        }
    }
    ""
}

struct P<'a> {
    g: &'a mut Gfx,
    ink: Col,
    lw: f32,
    body: Col,
    c: Col,
    th: Theme,
}

impl P<'_> {
    fn s(&self) -> Option<(Col, f32)> {
        Some((self.ink, self.lw))
    }
    fn circ(&mut self, x: f32, y: f32, r: f32, fill: Col) {
        let s = self.s();
        self.g.circle(x, y, r, Some(fill), s);
    }
    fn ell(&mut self, x: f32, y: f32, rx: f32, ry: f32, fill: Col) {
        let s = self.s();
        self.g.ellipse(x, y, rx, ry, Some(fill), s);
    }
    fn rr(&mut self, x: f32, y: f32, w: f32, h: f32, r: f32, fill: Col) {
        let s = self.s();
        self.g.rrect(x, y, w, h, r, Some(fill), s);
    }
    fn poly(&mut self, pts: &[(f32, f32)], fill: Col) {
        let s = self.s();
        self.g.poly(pts, Some(fill), s);
    }
    fn line(&mut self, pts: &[(f32, f32)]) {
        let (c, w) = (self.ink, self.lw);
        self.g.line(pts, c, w);
    }
    fn seg(&mut self, a: (f32, f32), b: (f32, f32)) {
        self.line(&[a, b]);
    }
    fn curve(&mut self, a: (f32, f32), c: (f32, f32), b: (f32, f32)) {
        let pts = bezier(a, c, b, 16);
        self.line(&pts);
    }
    fn dot(&mut self, x: f32, y: f32, r: f32) {
        let c = self.ink;
        self.g.circle(x, y, r, Some(c), None);
    }
    fn text(&mut self, s: &str, size: f32, x: f32, y: f32, col: Col) {
        self.g.text(Font::Display, s, size, x, y, Align::Center, col);
    }
}

fn ring(cx: f32, cy: f32, r: f32, n: usize) -> Vec<(f32, f32)> {
    (0..n).map(|i| {
        let a = TAU * i as f32 / n as f32;
        (cx + a.cos() * r, cy + a.sin() * r)
    }).collect()
}

fn star_pts(cx: f32, cy: f32, r: f32, inner: f32, n: usize) -> Vec<(f32, f32)> {
    (0..n * 2)
        .map(|i| {
            let a = -PI / 2.0 + PI * i as f32 / n as f32;
            let rr = if i % 2 == 0 { r } else { r * inner };
            (cx + a.cos() * rr, cy + a.sin() * rr)
        })
        .collect()
}

fn gear_pts(r: f32, teeth: usize, rot: f32) -> Vec<(f32, f32)> {
    let mut v = vec![];
    for i in 0..teeth {
        let a0 = rot + TAU * i as f32 / teeth as f32;
        let step = TAU / teeth as f32;
        for (da, rr) in [(0.0, r * 0.8), (0.12, r), (0.38, r), (0.5, r * 0.8)] {
            let a = a0 + step * da;
            v.push((a.cos() * rr, a.sin() * rr));
        }
    }
    v
}

/// Draw icon `name` centered at (cx, cy), `size` pixels across, mainly in color `c`.
pub fn draw(g: &mut Gfx, th: &Theme, name: &str, cx: f32, cy: f32, size: f32, c: Col, t: f32) {
    let k = size / 200.0;
    g.save();
    g.translate(cx, cy);
    g.scale(k);
    if th.glow {
        g.glow(0.0, 0.0, 120.0, c.a(0.35));
    }
    let lw = (th.line_w / k).clamp(4.0, 14.0);
    let mut p = P { g, ink: th.ink, lw, body: th.body, c, th: *th };
    let canon = canonical(name);
    draw_named(&mut p, canon, name, t);
    p.g.restore();
}

fn draw_named(p: &mut P, canon: &str, raw: &str, t: f32) {
    let c = p.c;
    let b = p.body;
    let a2 = p.th.accent(1);
    let a3 = p.th.accent(2);
    let gold = p.th.gold;
    let green = p.th.accent(3);
    let blue = p.th.accent(1);
    let red = p.th.accent(2);
    match canon {
        "atom" => {
            p.circ(0.0, 0.0, 20.0, c);
            for i in 0..3 {
                p.g.save();
                p.g.rotate(i as f32 * 60.0);
                let s = Some((p.ink, p.lw * 0.8));
                p.g.ellipse(0.0, 0.0, 88.0, 32.0, None, s);
                let a = t * 2.2 + i as f32 * 2.1;
                p.circ(a.cos() * 88.0, a.sin() * 32.0, 9.0, b);
                p.g.restore();
            }
        }
        "molecule" => {
            let pts = [(-55.0, -35.0), (0.0, 0.0), (60.0, -30.0), (10.0, 62.0), (-62.0, 50.0)];
            for &q in &pts[..] {
                if q != pts[1] {
                    p.seg(pts[1], q);
                }
            }
            p.circ(0.0, 0.0, 34.0, c);
            for (i, q) in pts.iter().enumerate().filter(|(i, _)| *i != 1) {
                let r = 20.0 + (t * 3.0 + i as f32).sin() * 2.0;
                p.circ(q.0, q.1, r, p.th.accent(i + 1));
            }
        }
        "dna" => {
            let n = 26;
            let off = t * 2.0;
            let mut l = vec![];
            let mut r = vec![];
            for i in 0..=n {
                let y = -90.0 + 180.0 * i as f32 / n as f32;
                let a = y / 30.0 + off;
                l.push((a.sin() * 42.0, y));
                r.push((-a.sin() * 42.0, y));
            }
            for i in (1..n).step_by(3) {
                let col = if (i / 3) % 2 == 0 { c } else { a2 };
                let (x1, y1) = l[i];
                let (x2, _) = r[i];
                p.g.seg((x1, y1), (x2, y1), col, p.lw * 1.3);
            }
            let (ink, lw) = (p.ink, p.lw * 1.4);
            p.g.line(&l, ink, lw);
            p.g.line(&r, ink, lw);
        }
        "cell" => {
            let pts: Vec<(f32, f32)> = (0..40)
                .map(|i| {
                    let a = TAU * i as f32 / 40.0;
                    let r = 86.0 + (a * 3.0 + t * 1.5).sin() * 5.0 + (a * 5.0).cos() * 3.0;
                    (a.cos() * r, a.sin() * r * 0.85)
                })
                .collect();
            p.poly(&pts, c.lighten(0.55));
            p.circ(-12.0, -6.0, 30.0, c);
            p.circ(-18.0, -12.0, 9.0, c.darken(0.25));
            for (i, q) in ring(0.0, 0.0, 55.0, 5).iter().enumerate() {
                if i != 3 {
                    p.ell(q.0 + 8.0, q.1 * 0.8, 10.0, 6.0, a3.lighten(0.2));
                }
            }
        }
        "virus" => {
            let wob = (t * 3.0).sin() * 3.0;
            for (i, q) in ring(0.0, 0.0, 62.0, 10).iter().enumerate() {
                let (dx, dy) = (q.0 / 62.0, q.1 / 62.0);
                p.seg((q.0 * 0.8, q.1 * 0.8), (q.0 + dx * (20.0 + wob), q.1 + dy * (20.0 + wob)));
                p.circ(q.0 + dx * (26.0 + wob), q.1 + dy * (26.0 + wob), 9.0, if i % 2 == 0 { c } else { b });
            }
            p.circ(0.0, 0.0, 58.0, c);
            for q in [(-18.0, -14.0), (20.0, 4.0), (-4.0, 24.0)] {
                p.circ(q.0, q.1, 9.0, c.darken(0.3));
            }
        }
        "pill" => {
            p.g.save();
            p.g.rotate(-35.0);
            p.rr(-90.0, -36.0, 180.0, 72.0, 36.0, b);
            p.g.save();
            let s = p.s();
            if let Some(path) = crate::gfx::rrect_path(-90.0, -36.0, 90.0, 72.0, 36.0) {
                p.g.fill(&path, c);
            }
            p.g.rrect(-90.0, -36.0, 180.0, 72.0, 36.0, None, s);
            p.g.restore();
            p.seg((0.0, -36.0), (0.0, 36.0));
            p.g.restore();
        }
        "heart" => {
            let beat = 1.0 + ((t * 2.4).fract() * TAU).sin().max(0.0).powi(4) * 0.12;
            p.g.save();
            p.g.scale(beat);
            heart(p, 0.0, 5.0, 85.0, c);
            p.g.restore();
        }
        "brain" => {
            let col = hex(0xF4A6B8).mix(c, 0.25);
            let blobs = [(-38.0, -30.0, 42.0), (8.0, -42.0, 44.0), (48.0, -10.0, 40.0), (-48.0, 20.0, 38.0), (0.0, 18.0, 46.0), (40.0, 34.0, 34.0)];
            for &(x, y, r) in &blobs {
                p.circ(x, y, r, col);
            }
            for &(x, y, r) in &blobs {
                p.g.circle(x, y, r - p.lw * 0.5, Some(col), None);
            }
            p.curve((-60.0, -10.0), (-30.0, -40.0), (0.0, -10.0));
            p.curve((10.0, 20.0), (40.0, -20.0), (60.0, 10.0));
            p.curve((-30.0, 40.0), (-10.0, 10.0), (20.0, 50.0));
            p.seg((0.0, -60.0), (0.0, -20.0));
            let pulse = ((t * 2.0).sin() * 0.5 + 0.5) * 0.5;
            p.g.glow(10.0, -10.0, 70.0, gold.a(pulse * 0.6));
        }
        "eye" => {
            let pts: Vec<(f32, f32)> = bezier((-92.0, 0.0), (0.0, -88.0), (92.0, 0.0), 16)
                .into_iter()
                .chain(bezier((92.0, 0.0), (0.0, 88.0), (-92.0, 0.0), 16))
                .collect();
            p.poly(&pts, b);
            let look = (t * 0.9).sin() * 14.0;
            p.circ(look, 0.0, 34.0, c);
            p.dot(look, 0.0, 15.0);
            p.g.circle(look + 8.0, -9.0, 6.0, Some(hex(0xFFFFFF)), None);
        }
        "leaf" => {
            p.g.save();
            p.g.rotate(-30.0 + (t * 1.5).sin() * 5.0);
            let pts: Vec<(f32, f32)> = bezier((0.0, 90.0), (-95.0, 0.0), (0.0, -90.0), 18)
                .into_iter()
                .chain(bezier((0.0, -90.0), (95.0, 0.0), (0.0, 90.0), 18))
                .collect();
            p.poly(&pts, green);
            p.seg((0.0, 100.0), (0.0, -70.0));
            for i in 0..3 {
                let y = -40.0 + i as f32 * 35.0;
                p.seg((0.0, y + 15.0), (-32.0, y - 10.0));
                p.seg((0.0, y + 15.0), (32.0, y - 10.0));
            }
            p.g.restore();
        }
        "tree" => {
            p.rr(-14.0, 10.0, 28.0, 90.0, 6.0, hex(0x9B6A43));
            let sway = (t * 1.3).sin() * 3.0;
            for &(x, y, r) in &[(-40.0, -10.0, 42.0), (40.0, -10.0, 42.0), (0.0, -50.0, 52.0), (0.0, -5.0, 45.0)] {
                p.circ(x + sway, y, r, green);
            }
            for &(x, y, r) in &[(-40.0, -10.0, 42.0), (40.0, -10.0, 42.0), (0.0, -50.0, 52.0), (0.0, -5.0, 45.0)] {
                p.g.circle(x + sway, y, r - p.lw * 0.5, Some(green), None);
            }
            for q in [(-30.0, -30.0), (25.0, -55.0), (35.0, 5.0)] {
                p.circ(q.0 + sway, q.1, 7.0, red);
            }
        }
        "sprout" => {
            p.rr(-50.0, 40.0, 100.0, 55.0, 10.0, hex(0xC9774A));
            p.rr(-60.0, 30.0, 120.0, 20.0, 6.0, hex(0xD98A5C));
            let grow = 0.9 + (t * 2.0).sin() * 0.05;
            p.g.save();
            p.g.translate(0.0, 30.0);
            p.g.scale(grow);
            p.seg((0.0, 0.0), (0.0, -80.0));
            let l1: Vec<(f32, f32)> = bezier((0.0, -50.0), (-30.0, -100.0), (-75.0, -70.0), 10)
                .into_iter()
                .chain(bezier((-75.0, -70.0), (-40.0, -35.0), (0.0, -50.0), 10))
                .collect();
            p.poly(&l1, green);
            let l2: Vec<(f32, f32)> = bezier((0.0, -75.0), (30.0, -125.0), (75.0, -100.0), 10)
                .into_iter()
                .chain(bezier((75.0, -100.0), (40.0, -60.0), (0.0, -75.0), 10))
                .collect();
            p.poly(&l2, green);
            p.g.restore();
        }
        "apple" => {
            let pts: Vec<(f32, f32)> = (0..40)
                .map(|i| {
                    let a = TAU * i as f32 / 40.0;
                    let r = 75.0 * (1.0 - 0.18 * (a + PI / 2.0).sin().powi(8).max(0.0));
                    let dent = if (a - 3.0 * PI / 2.0).abs() < 0.4 { -12.0 * (1.0 - (a - 3.0 * PI / 2.0).abs() / 0.4) } else { 0.0 };
                    (a.cos() * r * 1.05, a.sin() * r + 10.0 - dent * a.sin().signum())
                })
                .collect();
            p.poly(&pts, red);
            p.seg((0.0, -55.0), (8.0, -90.0));
            let leaf: Vec<(f32, f32)> = bezier((6.0, -75.0), (30.0, -105.0), (55.0, -82.0), 8)
                .into_iter()
                .chain(bezier((55.0, -82.0), (30.0, -62.0), (6.0, -75.0), 8))
                .collect();
            p.poly(&leaf, green);
            p.g.ellipse(-30.0, -10.0, 9.0, 18.0, Some(hex(0xFFFFFF).a(0.6)), None);
        }
        "sun" => {
            p.g.save();
            p.g.rotate(t * 20.0);
            let pulse = 1.0 + (t * 3.0).sin() * 0.08;
            for i in 0..10 {
                let a = TAU * i as f32 / 10.0;
                let (r0, r1) = (62.0, 90.0 * pulse);
                p.seg((a.cos() * r0, a.sin() * r0), (a.cos() * r1, a.sin() * r1));
            }
            p.g.restore();
            p.g.glow(0.0, 0.0, 95.0, gold.a(0.4));
            p.circ(0.0, 0.0, 50.0, gold);
            p.dot(-16.0, -6.0, 5.0);
            p.dot(16.0, -6.0, 5.0);
            let sm = bezier((-16.0, 14.0), (0.0, 28.0), (16.0, 14.0), 8);
            p.line(&sm);
        }
        "moon" => {
            let pts: Vec<(f32, f32)> = (0..=24)
                .map(|i| {
                    let a = -PI / 2.0 + PI * i as f32 / 24.0;
                    (a.cos() * 80.0, a.sin() * 80.0)
                })
                .chain((0..=24).rev().map(|i| {
                    let a = -PI / 2.0 + PI * i as f32 / 24.0;
                    (a.cos() * 40.0, a.sin() * 80.0)
                }))
                .collect();
            p.g.save();
            p.g.rotate(160.0);
            p.poly(&pts, gold.lighten(0.3));
            p.g.restore();
            let tw = (t * 3.0).sin() * 0.5 + 0.5;
            let s = star_pts(55.0, -55.0, 14.0 + tw * 4.0, 0.45, 4);
            p.poly(&s, gold);
        }
        "planet" => {
            p.circ(0.0, 0.0, 58.0, c);
            p.g.save();
            p.g.rotate(-18.0);
            let s = Some((p.ink, p.lw));
            let ring_c = gold.lighten(0.2);
            p.g.ellipse(0.0, 0.0, 100.0, 24.0, None, Some((ring_c, p.lw * 2.2)));
            p.g.ellipse(0.0, 0.0, 100.0, 24.0, None, s);
            p.g.restore();
            p.g.save();
            if let Some(path) = crate::gfx::rrect_path(-60.0, -60.0, 120.0, 52.0, 0.0) {
                let _ = path;
            }
            p.g.restore();
            p.ell(-20.0, -18.0, 16.0, 8.0, c.darken(0.15));
            p.ell(22.0, 14.0, 12.0, 6.0, c.darken(0.15));
        }
        "star" => {
            let s = 1.0 + (t * 3.0).sin() * 0.06;
            p.g.save();
            p.g.scale(s);
            p.g.rotate((t * 1.5).sin() * 8.0);
            let pts = star_pts(0.0, 8.0, 92.0, 0.45, 5);
            p.poly(&pts, gold);
            p.g.restore();
        }
        "cloud" | "rain" => {
            let dy = if canon == "rain" { -25.0 } else { 0.0 };
            let drift = (t * 1.2).sin() * 6.0;
            let blobs = [(-45.0, 10.0, 36.0), (0.0, -15.0, 48.0), (48.0, 8.0, 36.0), (0.0, 22.0, 34.0)];
            let col = if p.th.dark { hex(0xDCE3F5) } else { hex(0xFFFFFF) };
            for &(x, y, r) in &blobs {
                p.circ(x + drift, y + dy, r, col);
            }
            for &(x, y, r) in &blobs {
                p.g.circle(x + drift, y + dy, r - p.lw * 0.5, Some(col), None);
            }
            if canon == "rain" {
                for i in 0..4 {
                    let x = -45.0 + i as f32 * 30.0;
                    let ph = (t * 1.6 + i as f32 * 0.37).fract();
                    let y = 40.0 + ph * 50.0;
                    p.g.fade(1.0 - ph);
                    drop(p, x + drift, y, 12.0, blue);
                    p.g.fade(1.0);
                }
            }
        }
        "snowflake" => {
            p.g.save();
            p.g.rotate(t * 25.0);
            let col = hex(0x7CC6F2);
            let (ink, lw) = (p.ink, p.lw);
            for i in 0..6 {
                p.g.save();
                p.g.rotate(i as f32 * 60.0);
                p.g.seg((0.0, 0.0), (0.0, -85.0), ink, lw * 2.3);
                p.g.seg((0.0, -50.0), (-22.0, -72.0), ink, lw * 2.3);
                p.g.seg((0.0, -50.0), (22.0, -72.0), ink, lw * 2.3);
                p.g.seg((0.0, 0.0), (0.0, -85.0), col, lw);
                p.g.seg((0.0, -50.0), (-22.0, -72.0), col, lw);
                p.g.seg((0.0, -50.0), (22.0, -72.0), col, lw);
                p.g.restore();
            }
            p.g.restore();
        }
        "lightning" => {
            let flick = if (t * 6.0).sin() > 0.7 { 1.08 } else { 1.0 };
            p.g.save();
            p.g.scale(flick);
            p.g.glow(0.0, 0.0, 90.0, gold.a(0.35));
            p.poly(&[(15.0, -95.0), (-50.0, 10.0), (-5.0, 10.0), (-20.0, 95.0), (52.0, -18.0), (8.0, -18.0)], gold);
            p.g.restore();
        }
        "fire" => {
            let f = |k: f32, ph: f32| (t * 7.0 + ph).sin() * k;
            let outer: Vec<(f32, f32)> = bezier((0.0, 90.0), (-90.0, 60.0), (-45.0 + f(6.0, 0.0), -20.0), 10)
                .into_iter()
                .chain(bezier((-45.0 + f(6.0, 0.0), -20.0), (-20.0, 20.0), (0.0 + f(8.0, 1.0), -95.0), 10))
                .chain(bezier((0.0 + f(8.0, 1.0), -95.0), (40.0, -20.0), (50.0 + f(6.0, 2.0), -30.0), 10))
                .chain(bezier((50.0 + f(6.0, 2.0), -30.0), (90.0, 60.0), (0.0, 90.0), 10))
                .collect();
            p.g.glow(0.0, 20.0, 100.0, hex(0xFF7A28).a(0.35));
            p.poly(&outer, hex(0xFF7A28));
            let inner: Vec<(f32, f32)> = outer.iter().map(|q| (q.0 * 0.55, 40.0 + (q.1 - 40.0) * 0.55)).collect();
            p.g.poly(&inner, Some(gold), None);
        }
        "water" => drop(p, 0.0, 10.0, 80.0 * (1.0 + (t * 2.5).sin() * 0.03), blue),
        "mountain" => {
            p.poly(&[(-100.0, 80.0), (-30.0, -60.0), (40.0, 80.0)], hex(0x8A97A8));
            p.poly(&[(-10.0, 80.0), (45.0, -20.0), (100.0, 80.0)], hex(0x6E7C8F));
            p.poly(&[(-30.0, -60.0), (-50.0, -20.0), (-38.0, -26.0), (-30.0, -15.0), (-20.0, -26.0), (-10.0, -20.0)], hex(0xFFFFFF));
            p.g.circle(60.0, -70.0, 18.0, Some(gold), None);
        }
        "globe" => {
            p.circ(0.0, 0.0, 85.0, blue);
            let spin = (t * 0.5).fract();
            let land = green;
            for (k, (x, y, rx, ry)) in [(-30.0, -25.0, 30.0, 22.0), (25.0, 20.0, 26.0, 34.0), (50.0, -40.0, 18.0, 12.0)].iter().enumerate() {
                let xx = ((x / 85.0 + spin * 2.0 + k as f32 * 0.1 + 1.0).rem_euclid(2.0) - 1.0) * 85.0;
                if xx.abs() < 70.0 {
                    let squash = (1.0 - (xx / 85.0).powi(2)).sqrt();
                    p.g.ellipse(xx, *y, rx * squash, *ry, Some(land), Some((p.ink, p.lw * 0.6)));
                }
            }
            let s = Some((p.ink, p.lw * 0.6));
            p.g.ellipse(0.0, 0.0, 85.0, 30.0, None, s);
            p.g.ellipse(0.0, 0.0, 35.0, 85.0, None, s);
            p.g.circle(0.0, 0.0, 85.0, None, Some((p.ink, p.lw)));
        }
        "flask" => {
            let body = [(-20.0, -90.0), (20.0, -90.0), (20.0, -30.0), (75.0, 75.0), (65.0, 90.0), (-65.0, 90.0), (-75.0, 75.0), (-20.0, -30.0)];
            p.poly(&body, b);
            let lvl = 20.0 + (t * 2.0).sin() * 4.0;
            p.g.poly(&[(-48.0 + 0.0, lvl), (48.0, lvl), (72.0, 76.0), (63.0, 86.0), (-63.0, 86.0), (-72.0, 76.0)], Some(green), None);
            for i in 0..3 {
                let ph = (t * 0.9 + i as f32 * 0.33).fract();
                p.g.circle(-20.0 + i as f32 * 20.0, lvl + 50.0 - ph * 70.0, 6.0 + i as f32 * 2.0, Some(hex(0xFFFFFF).a(1.0 - ph)), None);
            }
            p.poly(&body, Col(0.0, 0.0, 0.0, 0.0));
            p.seg((-30.0, -90.0), (30.0, -90.0));
        }
        "microscope" => {
            p.rr(-70.0, 70.0, 140.0, 22.0, 8.0, c);
            p.line(&[(30.0, 70.0), (30.0, 10.0), (0.0, -10.0)]);
            p.g.save();
            p.g.rotate(-30.0);
            p.rr(-30.0, -95.0, 34.0, 100.0, 8.0, b);
            p.rr(-36.0, -105.0, 46.0, 18.0, 5.0, c);
            p.g.restore();
            p.rr(-50.0, 30.0, 100.0, 12.0, 4.0, b);
            p.circ(40.0, 30.0, 14.0, c);
        }
        "magnet" => {
            let mut pts = vec![];
            for i in 0..=20 {
                let a = PI + PI * i as f32 / 20.0;
                pts.push((a.cos() * 75.0, 0.0 - a.sin() * -75.0));
            }
            let mut outer = vec![(-75.0, 80.0), (-75.0, 0.0)];
            outer.extend((0..=20).map(|i| {
                let a = PI + PI * i as f32 / 20.0;
                (a.cos() * 75.0, a.sin() * 75.0)
            }));
            outer.extend([(75.0, 0.0), (75.0, 80.0), (35.0, 80.0), (35.0, 0.0)]);
            outer.extend((0..=20).rev().map(|i| {
                let a = PI + PI * i as f32 / 20.0;
                (a.cos() * 35.0, a.sin() * 35.0)
            }));
            outer.extend([(-35.0, 0.0), (-35.0, 80.0)]);
            p.poly(&outer, red);
            p.rr(-75.0, 50.0, 40.0, 30.0, 0.0, b);
            p.rr(35.0, 50.0, 40.0, 30.0, 0.0, b);
            let k = (t * 4.0).sin() * 4.0;
            for s in [-1.0f32, 1.0] {
                p.g.arrow((s * 100.0, 80.0 + k), (s * 100.0, 40.0 + k), 0.0, gold, 5.0, 1.0);
            }
            let _ = pts;
        }
        "battery" => {
            p.rr(-80.0, -45.0, 150.0, 90.0, 12.0, b);
            p.rr(70.0, -18.0, 16.0, 36.0, 4.0, p.ink);
            let lvl = ((t * 0.6).fract() * 4.0).floor() as usize + 1;
            for i in 0..lvl.min(4) {
                p.g.rrect(-68.0 + i as f32 * 34.0, -33.0, 28.0, 66.0, 5.0, Some(green), None);
            }
        }
        "plug" => {
            p.rr(-45.0, -40.0, 90.0, 80.0, 18.0, b);
            p.rr(-30.0, -85.0, 14.0, 45.0, 4.0, hex(0xBBBBBB));
            p.rr(16.0, -85.0, 14.0, 45.0, 4.0, hex(0xBBBBBB));
            p.curve((0.0, 40.0), (0.0, 90.0), (60.0, 90.0));
            p.g.glow(0.0, -95.0, 40.0, gold.a(((t * 5.0).sin() * 0.5 + 0.5) * 0.7));
        }
        "thermometer" => {
            p.rr(-22.0, -95.0, 44.0, 140.0, 22.0, b);
            p.circ(0.0, 60.0, 36.0, red);
            let lvl = -60.0 + (t * 1.5).sin() * 20.0;
            p.g.rrect(-9.0, lvl, 18.0, 70.0 - lvl, 9.0, Some(red), None);
            for i in 0..4 {
                p.seg((22.0, -70.0 + i as f32 * 25.0), (38.0, -70.0 + i as f32 * 25.0));
            }
        }
        "gear" => {
            let g1 = gear_pts(70.0, 10, t * 1.2);
            p.g.save();
            p.g.translate(-15.0, 10.0);
            p.poly(&g1, c);
            p.circ(0.0, 0.0, 22.0, b);
            p.g.restore();
            let g2 = gear_pts(36.0, 7, -t * 1.2 * 10.0 / 7.0 + 0.2);
            p.g.save();
            p.g.translate(62.0, -62.0);
            p.poly(&g2, a2);
            p.circ(0.0, 0.0, 11.0, b);
            p.g.restore();
        }
        "hammer" => {
            p.g.save();
            p.g.rotate(-35.0 + ((t * 3.0).sin().max(0.0)) * 15.0);
            p.rr(-12.0, -40.0, 24.0, 140.0, 8.0, hex(0xB07A4F));
            p.rr(-60.0, -80.0, 120.0, 48.0, 10.0, hex(0x9AA3AF));
            p.g.restore();
        }
        "box" => {
            p.poly(&[(-80.0, -30.0), (0.0, -65.0), (80.0, -30.0), (0.0, 5.0)], hex(0xE7B57A));
            p.poly(&[(-80.0, -30.0), (0.0, 5.0), (0.0, 95.0), (-80.0, 55.0)], hex(0xD39A5E));
            p.poly(&[(80.0, -30.0), (0.0, 5.0), (0.0, 95.0), (80.0, 55.0)], hex(0xC0874D));
            p.seg((-40.0, -47.0), (40.0, -12.0));
        }
        "rocket" => {
            let shake = (t * 30.0).sin() * 1.5;
            p.g.save();
            p.g.translate(shake, 0.0);
            p.g.rotate(35.0);
            let fl = 1.0 + (t * 14.0).sin() * 0.2;
            p.poly(&[(-18.0, 60.0), (0.0, 60.0 + 50.0 * fl), (18.0, 60.0)], hex(0xFF7A28));
            p.poly(&[(-30.0, 30.0), (-55.0, 70.0), (-25.0, 62.0)], red);
            p.poly(&[(30.0, 30.0), (55.0, 70.0), (25.0, 62.0)], red);
            let body: Vec<(f32, f32)> = bezier((-30.0, 62.0), (-40.0, -40.0), (0.0, -95.0), 14)
                .into_iter()
                .chain(bezier((0.0, -95.0), (40.0, -40.0), (30.0, 62.0), 14))
                .collect();
            p.poly(&body, b);
            p.circ(0.0, -20.0, 16.0, blue);
            p.g.restore();
        }
        "car" => {
            let bump = ((t * 8.0).sin()).abs() * 3.0;
            p.g.save();
            p.g.translate(0.0, -bump);
            p.poly(&[(-95.0, 30.0), (-90.0, -5.0), (-50.0, -12.0), (-25.0, -50.0), (40.0, -50.0), (65.0, -12.0), (95.0, -5.0), (95.0, 30.0)], c);
            p.poly(&[(-18.0, -40.0), (30.0, -40.0), (48.0, -14.0), (-36.0, -14.0)], hex(0xBFE3FF));
            p.g.restore();
            for x in [-50.0, 55.0] {
                p.circ(x, 34.0, 22.0, hex(0x333333));
                p.g.save();
                p.g.translate(x, 34.0);
                p.g.rotate(t * 400.0);
                p.dot(8.0, 0.0, 4.0);
                p.g.restore();
            }
        }
        "plane" => {
            let bob = (t * 2.0).sin() * 5.0;
            p.g.save();
            p.g.translate(0.0, bob);
            p.g.rotate(-15.0);
            p.poly(&[(-10.0, -5.0), (-40.0, -80.0), (-15.0, -80.0), (35.0, -5.0)], blue);
            p.poly(&[(-95.0, -10.0), (75.0, -12.0), (98.0, 0.0), (75.0, 12.0), (-95.0, 10.0)], b);
            p.poly(&[(-10.0, 5.0), (-40.0, 70.0), (-15.0, 70.0), (35.0, 5.0)], blue);
            p.poly(&[(-95.0, -8.0), (-100.0, -45.0), (-80.0, -45.0), (-65.0, -8.0)], red);
            p.g.restore();
        }
        "ship" => {
            let rock = (t * 2.0).sin() * 5.0;
            p.g.save();
            p.g.rotate(rock);
            p.seg((0.0, 20.0), (0.0, -95.0));
            p.poly(&[(5.0, -88.0), (70.0, 0.0), (5.0, 0.0)], b);
            p.poly(&[(-5.0, -70.0), (-55.0, 0.0), (-5.0, 0.0)], b);
            p.poly(&[(-95.0, 15.0), (95.0, 15.0), (65.0, 60.0), (-65.0, 60.0)], hex(0xA0643C));
            p.g.restore();
            let w: Vec<(f32, f32)> = (0..=20).map(|i| (-100.0 + i as f32 * 10.0, 72.0 + ((i as f32 * 0.9) + t * 4.0).sin() * 5.0)).collect();
            let (ink, lw) = (p.ink, p.lw);
            p.g.line(&w, blue, lw * 1.6);
            let _ = ink;
        }
        "house" => {
            p.rr(-70.0, -10.0, 140.0, 100.0, 4.0, b);
            p.poly(&[(-90.0, -5.0), (0.0, -90.0), (90.0, -5.0)], red);
            p.rr(-18.0, 30.0, 36.0, 60.0, 4.0, hex(0x9B6A43));
            p.rr(30.0, 10.0, 28.0, 28.0, 3.0, hex(0xBFE3FF));
            p.rr(-58.0, 10.0, 28.0, 28.0, 3.0, hex(0xBFE3FF));
        }
        "factory" => {
            p.poly(&[(-95.0, 90.0), (-95.0, -10.0), (-50.0, 20.0), (-50.0, -10.0), (-5.0, 20.0), (-5.0, -10.0), (40.0, 20.0), (40.0, -60.0), (75.0, -60.0), (75.0, 90.0)], hex(0x9AA3AF));
            for i in 0..3 {
                let ph = (t * 0.7 + i as f32 * 0.33).fract();
                p.g.circle(58.0 + ph * 25.0, -75.0 - ph * 50.0, 12.0 + ph * 14.0, Some(hex(0xBBBBBB).a(1.0 - ph)), None);
            }
            for x in [-75.0, -30.0, 15.0] {
                p.rr(x, 45.0, 22.0, 22.0, 2.0, gold);
            }
        }
        "bank" => {
            p.poly(&[(-95.0, -35.0), (0.0, -92.0), (95.0, -35.0)], b);
            p.rr(-90.0, -35.0, 180.0, 16.0, 2.0, b);
            for i in 0..4 {
                p.rr(-74.0 + i as f32 * 44.0, -16.0, 22.0, 82.0, 2.0, b);
            }
            p.rr(-95.0, 66.0, 190.0, 22.0, 2.0, b);
            p.text("$", 40.0, 0.0, -50.0, p.ink);
        }
        "money" => {
            let spin = (t * 2.5).sin();
            p.g.save();
            p.g.scale_xy(0.35 + 0.65 * spin.abs(), 1.0);
            p.circ(0.0, 0.0, 82.0, gold);
            p.g.circle(0.0, 0.0, 62.0, None, Some((gold.darken(0.3), p.lw * 0.8)));
            if spin > 0.0 {
                p.text("$", 100.0, 0.0, 2.0, gold.darken(0.45));
            }
            p.g.restore();
        }
        "chart_up" | "chart_down" => {
            p.rr(-90.0, -85.0, 180.0, 170.0, 14.0, b);
            let up = canon == "chart_up";
            for i in 0..4 {
                let h = if up { 30.0 + i as f32 * 28.0 } else { 115.0 - i as f32 * 28.0 };
                let grow = crate::gfx::ease_out((t * 0.8 - i as f32 * 0.12).fract().min(1.0) * 2.0);
                let hh = h * (0.85 + 0.15 * grow);
                p.g.rrect(-70.0 + i as f32 * 36.0, 70.0 - hh, 26.0, hh, 4.0, Some(p.th.accent(i)), Some((p.ink, p.lw * 0.7)));
            }
            let pts = if up { [(-70.0, 20.0), (-20.0, -10.0), (10.0, 5.0), (75.0, -65.0)] } else { [(-70.0, -60.0), (-20.0, -25.0), (10.0, -35.0), (75.0, 40.0)] };
            let col = if up { green } else { red };
            p.g.arrow(pts[2], pts[3], 0.0, col, p.lw * 1.4, 1.0);
            p.g.line(&pts[..3], col, p.lw * 1.4);
        }
        "scale" => {
            let tilt = (t * 1.5).sin() * 10.0;
            p.rr(-10.0, -70.0, 20.0, 150.0, 6.0, gold);
            p.rr(-55.0, 75.0, 110.0, 18.0, 6.0, gold);
            p.g.save();
            p.g.translate(0.0, -65.0);
            p.g.rotate(tilt);
            p.rr(-90.0, -6.0, 180.0, 12.0, 6.0, gold);
            for s in [-1.0f32, 1.0] {
                p.g.save();
                p.g.translate(s * 80.0, 0.0);
                p.g.rotate(-tilt);
                p.seg((0.0, 0.0), (-25.0, 55.0));
                p.seg((0.0, 0.0), (25.0, 55.0));
                let pan: Vec<(f32, f32)> = (0..=12).map(|i| {
                    let a = PI * i as f32 / 12.0;
                    (-a.cos() * 34.0, 55.0 + a.sin() * 22.0)
                }).collect();
                p.poly(&pan, gold.lighten(0.3));
                p.g.restore();
            }
            p.g.restore();
            p.circ(0.0, -70.0, 12.0, gold);
        }
        "key" => {
            p.g.save();
            p.g.rotate(-30.0 + (t * 2.0).sin() * 6.0);
            p.poly(&[(-15.0, -10.0), (90.0, -10.0), (90.0, 30.0), (75.0, 30.0), (75.0, 10.0), (60.0, 10.0), (60.0, 25.0), (45.0, 25.0), (45.0, 10.0), (-15.0, 10.0)], gold);
            p.circ(-45.0, 0.0, 40.0, gold);
            p.circ(-50.0, 0.0, 14.0, b);
            p.g.restore();
        }
        "lock" => {
            let open = ((t * 0.8).sin() * 0.5 + 0.5) * 18.0;
            let arc: Vec<(f32, f32)> = (0..=16).map(|i| {
                let a = PI + PI * i as f32 / 16.0;
                (a.cos() * 42.0, -10.0 - open + a.sin() * 50.0)
            }).collect();
            let (ink, lw) = (p.ink, p.lw);
            p.g.line(&arc, ink, lw * 3.8);
            p.g.line(&arc, hex(0xB8C0CC), lw * 1.8);
            p.rr(-65.0, -15.0, 130.0, 105.0, 16.0, gold);
            p.circ(0.0, 25.0, 13.0, p.ink);
            p.rr(-5.0, 30.0, 10.0, 30.0, 4.0, p.ink);
        }
        "shield" => {
            let pts: Vec<(f32, f32)> = vec![(0.0, -90.0), (75.0, -65.0)]
                .into_iter()
                .chain(bezier((75.0, -65.0), (80.0, 50.0), (0.0, 95.0), 14))
                .chain(bezier((0.0, 95.0), (-80.0, 50.0), (-75.0, -65.0), 14))
                .collect();
            p.g.glow(0.0, 0.0, 100.0, blue.a(0.25 + 0.15 * (t * 3.0).sin()));
            p.poly(&pts, blue);
            p.line(&[(-32.0, 2.0), (-8.0, 28.0), (38.0, -26.0)]);
        }
        "crown" => {
            let bob = (t * 2.0).sin() * 4.0;
            p.g.save();
            p.g.translate(0.0, bob);
            p.poly(&[(-85.0, 55.0), (-90.0, -40.0), (-45.0, 0.0), (0.0, -65.0), (45.0, 0.0), (90.0, -40.0), (85.0, 55.0)], gold);
            for (x, col) in [(-45.0, red), (0.0, blue), (45.0, green)] {
                p.circ(x, 30.0, 11.0, col);
            }
            for q in [(-90.0, -40.0), (0.0, -65.0), (90.0, -40.0)] {
                p.circ(q.0, q.1, 9.0, gold);
            }
            p.g.restore();
        }
        "swords" => {
            for s in [-1.0f32, 1.0] {
                p.g.save();
                p.g.rotate(s * (40.0 + (t * 4.0).sin() * 3.0));
                p.poly(&[(-9.0, -95.0), (0.0, -108.0), (9.0, -95.0), (9.0, 30.0), (-9.0, 30.0)], hex(0xD5DBE3));
                p.rr(-35.0, 30.0, 70.0, 14.0, 5.0, gold);
                p.rr(-8.0, 44.0, 16.0, 42.0, 5.0, hex(0x8B5A2B));
                p.g.restore();
            }
        }
        "flag" => {
            p.rr(-75.0, -90.0, 12.0, 185.0, 5.0, hex(0x9AA3AF));
            let wave: Vec<(f32, f32)> = (0..=12).map(|i| {
                let x = -63.0 + i as f32 * 12.5;
                (x, -85.0 + ((i as f32 * 0.6) - t * 5.0).sin() * 7.0 * i as f32 / 12.0)
            }).collect();
            let mut pts = wave.clone();
            pts.extend(wave.iter().rev().map(|q| (q.0, q.1 + 80.0)));
            p.poly(&pts, c);
        }
        "pin" => {
            let bounce = (t * 3.0).sin().abs() * 12.0;
            p.g.ellipse(0.0, 88.0, 30.0 - bounce * 0.8, 8.0, Some(p.ink.a(0.2)), None);
            p.g.save();
            p.g.translate(0.0, -bounce);
            let pts: Vec<(f32, f32)> = (0..=24)
                .map(|i| {
                    let a = PI * 0.8 + PI * 1.4 * i as f32 / 24.0;
                    (a.cos() * 55.0, -25.0 + a.sin() * 55.0)
                })
                .chain(std::iter::once((0.0, 80.0)))
                .collect();
            p.poly(&pts, red);
            p.circ(0.0, -25.0, 20.0, b);
            p.g.restore();
        }
        "compass" => {
            p.circ(0.0, 0.0, 85.0, b);
            p.circ(0.0, 0.0, 70.0, b.mix(c, 0.1));
            p.g.save();
            p.g.rotate((t * 1.3).sin() * 25.0 + 20.0);
            p.poly(&[(0.0, -62.0), (14.0, 0.0), (-14.0, 0.0)], red);
            p.poly(&[(0.0, 62.0), (14.0, 0.0), (-14.0, 0.0)], hex(0xBBBBBB));
            p.g.restore();
            p.circ(0.0, 0.0, 7.0, gold);
            p.text("N", 26.0, 0.0, -76.0, p.ink);
        }
        "clock" | "hourglass" if canon == "clock" => {
            p.circ(0.0, 0.0, 86.0, b);
            for i in 0..12 {
                let a = TAU * i as f32 / 12.0;
                p.seg((a.cos() * 70.0, a.sin() * 70.0), (a.cos() * 78.0, a.sin() * 78.0));
            }
            let m = t * 1.8;
            let hh = t * 0.15;
            p.seg((0.0, 0.0), ((hh - PI / 2.0).cos() * 40.0, (hh - PI / 2.0).sin() * 40.0));
            let (lw, rc) = (p.lw * 0.8, red);
            p.g.seg((0.0, 0.0), ((m - PI / 2.0).cos() * 62.0, (m - PI / 2.0).sin() * 62.0), rc, lw);
            p.circ(0.0, 0.0, 8.0, p.ink);
        }
        "hourglass" => {
            let rot = if (t * 0.25).fract() > 0.9 { ((t * 0.25).fract() - 0.9) * 10.0 * 180.0 } else { 0.0 };
            p.g.save();
            p.g.rotate(rot);
            let ph = (t * 0.25).fract().min(0.9) / 0.9;
            p.poly(&[(-60.0, -80.0), (60.0, -80.0), (8.0, 0.0), (60.0, 80.0), (-60.0, 80.0), (-8.0, 0.0)], b.mix(blue, 0.15));
            let top = 70.0 * (1.0 - ph);
            p.g.poly(&[(-45.0 * (top / 70.0), -top + 0.0 - 5.0), (45.0 * (top / 70.0), -top - 5.0), (5.0, -3.0), (-5.0, -3.0)], Some(gold), None);
            let bot = 70.0 * ph;
            p.g.poly(&[(-50.0 * (bot / 70.0).max(0.2), 75.0), (50.0 * (bot / 70.0).max(0.2), 75.0), (0.0, 75.0 - bot)], Some(gold), None);
            p.g.seg((0.0, 0.0), (0.0, 74.0), gold, 4.0);
            p.rr(-75.0, -95.0, 150.0, 18.0, 6.0, hex(0xA0643C));
            p.rr(-75.0, 77.0, 150.0, 18.0, 6.0, hex(0xA0643C));
            p.g.restore();
        }
        "calendar" => {
            p.rr(-85.0, -70.0, 170.0, 160.0, 14.0, b);
            p.g.rrect(-85.0, -70.0, 170.0, 45.0, 14.0, Some(red), Some((p.ink, p.lw)));
            for x in [-45.0, 45.0] {
                p.rr(x - 7.0, -90.0, 14.0, 36.0, 6.0, hex(0x9AA3AF));
            }
            let today = ((t * 1.5) as usize) % 9;
            for i in 0..9 {
                let (cx, cy) = (-55.0 + (i % 3) as f32 * 55.0, 0.0 + (i / 3) as f32 * 30.0);
                if i == today {
                    p.g.circle(cx, cy, 14.0, Some(gold), None);
                }
                p.g.rect(cx - 6.0, cy - 6.0, 12.0, 12.0, p.ink.a(0.7));
            }
        }
        "book" => {
            let flap = (t * 2.0).sin() * 6.0;
            p.poly(&[(0.0, -60.0), (-90.0, -75.0 + flap), (-90.0, 65.0 + flap), (0.0, 80.0)], b);
            p.poly(&[(0.0, -60.0), (90.0, -75.0 - flap), (90.0, 65.0 - flap), (0.0, 80.0)], b);
            for i in 0..4 {
                let y = -40.0 + i as f32 * 25.0;
                p.g.seg((-75.0, y - 8.0 + flap * 0.5), (-15.0, y), p.ink.a(0.5), 4.0);
                p.g.seg((15.0, y), (75.0, y - 8.0 - flap * 0.5), p.ink.a(0.5), 4.0);
            }
            p.g.rrect(-8.0, -66.0, 16.0, 150.0, 4.0, Some(c), Some((p.ink, p.lw * 0.6)));
        }
        "scroll" => {
            p.rr(-60.0, -70.0, 120.0, 140.0, 4.0, hex(0xF3E3BE));
            p.rr(-75.0, -90.0, 150.0, 28.0, 14.0, hex(0xE2C995));
            p.rr(-75.0, 62.0, 150.0, 28.0, 14.0, hex(0xE2C995));
            for i in 0..4 {
                let y = -40.0 + i as f32 * 25.0;
                p.g.seg((-40.0, y), (40.0 - (i % 2) as f32 * 20.0, y), p.ink.a(0.55), 4.0);
            }
        }
        "document" => {
            p.poly(&[(-65.0, -90.0), (35.0, -90.0), (70.0, -55.0), (70.0, 90.0), (-65.0, 90.0)], b);
            p.poly(&[(35.0, -90.0), (35.0, -55.0), (70.0, -55.0)], b.darken(0.08));
            for i in 0..5 {
                let y = -45.0 + i as f32 * 25.0;
                let w = if i == 4 { 50.0 } else { 100.0 };
                let col = if i == 0 { c } else { p.ink.a(0.45) };
                p.g.seg((-45.0, y), (-45.0 + w, y), col, 5.0);
            }
        }
        "pencil" => {
            p.g.save();
            p.g.rotate(-45.0 + (t * 6.0).sin() * 4.0);
            p.rr(-20.0, -90.0, 40.0, 140.0, 4.0, gold);
            p.rr(-20.0, -100.0, 40.0, 25.0, 8.0, hex(0xF4A6B8));
            p.poly(&[(-20.0, 50.0), (20.0, 50.0), (0.0, 95.0)], hex(0xF3D9B1));
            p.poly(&[(-7.0, 80.0), (7.0, 80.0), (0.0, 95.0)], p.ink);
            p.g.restore();
        }
        "graduation" => {
            let tilt = (t * 2.0).sin() * 5.0;
            p.g.save();
            p.g.rotate(tilt);
            p.poly(&[(-50.0, 0.0), (-50.0, 50.0), (50.0, 50.0), (50.0, 0.0)], p.th.ink.mix(blue, 0.4));
            p.poly(&[(0.0, -60.0), (100.0, -15.0), (0.0, 30.0), (-100.0, -15.0)], p.th.ink.mix(blue, 0.3));
            p.line(&[(0.0, -15.0), (70.0, 0.0), (70.0, 55.0)]);
            p.circ(70.0, 60.0, 10.0, gold);
            p.g.restore();
        }
        "lightbulb" => {
            let on = ((t * 1.2).sin() * 0.5 + 0.5).powf(0.5);
            p.g.glow(0.0, -20.0, 110.0, gold.a(0.55 * on));
            let pts: Vec<(f32, f32)> = (0..=28)
                .map(|i| {
                    let a = PI * 0.75 + PI * 1.5 * i as f32 / 28.0;
                    (a.cos() * 60.0, -25.0 + a.sin() * 60.0)
                })
                .collect();
            let mut bulb = pts.clone();
            bulb.extend([(25.0, 45.0), (-25.0, 45.0)]);
            p.poly(&bulb, b.mix(gold, 0.35 + 0.65 * on));
            p.rr(-27.0, 45.0, 54.0, 40.0, 8.0, hex(0x9AA3AF));
            p.seg((-27.0, 58.0), (27.0, 58.0));
            p.seg((-27.0, 71.0), (27.0, 71.0));
            p.line(&[(-14.0, 40.0), (-14.0, -10.0), (-4.0, -25.0), (4.0, -10.0), (14.0, -25.0), (14.0, 40.0)]);
            if on > 0.7 {
                for i in 0..5 {
                    let a = -PI / 2.0 + (i as f32 - 2.0) * 0.55;
                    p.g.seg((a.cos() * 78.0, -25.0 + a.sin() * 78.0), (a.cos() * 98.0, -25.0 + a.sin() * 98.0), gold.darken(0.2), 6.0);
                }
            }
        }
        "puzzle" => {
            let snap = ((t * 0.7).fract() * 2.0).min(1.0);
            let off = (1.0 - crate::gfx::ease_back(snap)) * 40.0;
            puzzle_piece(p, -42.0 - off, 0.0, c);
            puzzle_piece(p, 42.0 + off * 0.0, 0.0, a2);
        }
        "target" => {
            for (i, r) in [85.0, 62.0, 40.0, 18.0].iter().enumerate() {
                p.circ(0.0, 0.0, *r, if i % 2 == 0 { red } else { b });
            }
            let hit = ((t * 0.8).fract() * 3.0).min(1.0);
            let ax = 120.0 * (1.0 - crate::gfx::ease_out(hit));
            p.g.save();
            p.g.translate(ax, -ax * 0.6);
            p.g.rotate(-30.0);
            p.seg((0.0, 0.0), (85.0, 0.0));
            p.poly(&[(85.0, 0.0), (105.0, -14.0), (100.0, 0.0), (105.0, 14.0)], gold);
            p.g.restore();
        }
        "trophy" => {
            p.g.glow(0.0, -20.0, 100.0, gold.a(0.3 + 0.2 * (t * 3.0).sin()));
            p.g.circle(-62.0, -45.0, 26.0, None, Some((p.ink, p.lw * 2.6)));
            p.g.circle(62.0, -45.0, 26.0, None, Some((p.ink, p.lw * 2.6)));
            p.g.circle(-62.0, -45.0, 26.0, None, Some((gold, p.lw)));
            p.g.circle(62.0, -45.0, 26.0, None, Some((gold, p.lw)));
            let cup: Vec<(f32, f32)> = vec![(-62.0, -85.0), (62.0, -85.0)]
                .into_iter()
                .chain(bezier((62.0, -85.0), (60.0, 10.0), (0.0, 20.0), 12))
                .chain(bezier((0.0, 20.0), (-60.0, 10.0), (-62.0, -85.0), 12))
                .collect();
            p.poly(&cup, gold);
            p.rr(-12.0, 18.0, 24.0, 40.0, 2.0, gold);
            p.rr(-50.0, 58.0, 100.0, 30.0, 6.0, hex(0x8B5A2B));
            let s = star_pts(0.0, -40.0, 24.0, 0.45, 5);
            p.poly(&s, b);
        }
        "check" => {
            p.circ(0.0, 0.0, 86.0, green);
            let (ink, lw) = (p.ink, p.lw);
            p.g.line(&[(-42.0, 2.0), (-12.0, 34.0), (46.0, -32.0)], ink, lw * 3.4);
            p.g.line(&[(-42.0, 2.0), (-12.0, 34.0), (46.0, -32.0)], hex(0xFFFFFF), lw * 1.6);
        }
        "cross" => {
            p.circ(0.0, 0.0, 86.0, red);
            let (ink, lw) = (p.ink, p.lw);
            for (a, bb) in [((-35.0, -35.0), (35.0, 35.0)), ((35.0, -35.0), (-35.0, 35.0))] {
                p.g.seg(a, bb, ink, lw * 3.4);
            }
            for (a, bb) in [((-35.0, -35.0), (35.0, 35.0)), ((35.0, -35.0), (-35.0, 35.0))] {
                p.g.seg(a, bb, hex(0xFFFFFF), lw * 1.6);
            }
        }
        "question" => {
            let wob = (t * 3.0).sin() * 8.0;
            p.g.save();
            p.g.rotate(wob);
            p.circ(0.0, 0.0, 86.0, a2);
            p.text("?", 130.0, 0.0, 4.0, hex(0xFFFFFF));
            p.g.restore();
        }
        "warning" => {
            let pulse = 1.0 + ((t * 4.0).sin().max(0.0)) * 0.06;
            p.g.save();
            p.g.scale(pulse);
            let tri = [(0.0, -88.0), (95.0, 78.0), (-95.0, 78.0)];
            p.poly(&tri, gold);
            p.rr(-9.0, -35.0, 18.0, 70.0, 9.0, p.ink);
            p.circ(0.0, 56.0, 10.0, p.ink);
            p.g.restore();
        }
        "skull" => {
            let jaw = ((t * 5.0).sin().max(0.0)) * 6.0;
            p.rr(-40.0, 30.0 + jaw, 80.0, 45.0, 10.0, b);
            for x in [-20.0, 0.0, 20.0] {
                p.seg((x, 35.0 + jaw), (x, 70.0 + jaw));
            }
            p.circ(0.0, -20.0, 75.0, b);
            p.ell(-28.0, -15.0, 20.0, 22.0, p.ink);
            p.ell(28.0, -15.0, 20.0, 22.0, p.ink);
            p.poly(&[(0.0, 15.0), (-9.0, 32.0), (9.0, 32.0)], p.ink);
        }
        "magnifier" => {
            let mv = ((t * 1.5).sin() * 15.0, (t * 1.1).cos() * 10.0);
            p.g.save();
            p.g.translate(mv.0, mv.1);
            p.g.save();
            p.g.rotate(45.0);
            p.rr(45.0, -14.0, 70.0, 28.0, 12.0, hex(0x8B5A2B));
            p.g.restore();
            p.circ(-15.0, -15.0, 58.0, hex(0xBFE3FF).a(0.85));
            p.g.circle(-15.0, -15.0, 58.0, None, Some((p.ink, p.lw * 2.0)));
            p.g.ellipse(-35.0, -35.0, 12.0, 20.0, Some(hex(0xFFFFFF).a(0.7)), None);
            p.g.restore();
        }
        "megaphone" => {
            let k = (t * 6.0).sin() * 0.5 + 0.5;
            p.poly(&[(-60.0, -20.0), (40.0, -70.0), (40.0, 70.0), (-60.0, 20.0)], red);
            p.rr(-85.0, -24.0, 30.0, 48.0, 8.0, b);
            p.rr(-50.0, 15.0, 20.0, 45.0, 6.0, b);
            for i in 0..3 {
                let r = 60.0 + i as f32 * 20.0 + k * 8.0;
                let arc = bezier((40.0 + r * 0.4, -r * 0.7), (40.0 + r, 0.0), (40.0 + r * 0.4, r * 0.7), 10);
                let (ink, lw) = (p.ink.a(1.0 - i as f32 * 0.25), p.lw);
                p.g.line(&arc, ink, lw);
            }
        }
        "bell" => {
            let ring_a = (t * 10.0).sin() * 12.0 * ((t * 1.0).sin().max(0.0));
            p.g.save();
            p.g.translate(0.0, -80.0);
            p.g.rotate(ring_a);
            p.g.translate(0.0, 80.0);
            let pts: Vec<(f32, f32)> = vec![(-75.0, 55.0)]
                .into_iter()
                .chain(bezier((-55.0, 40.0), (-65.0, -75.0), (0.0, -75.0), 12))
                .chain(bezier((0.0, -75.0), (65.0, -75.0), (55.0, 40.0), 12))
                .chain(std::iter::once((75.0, 55.0)))
                .collect();
            p.poly(&pts, gold);
            p.circ(0.0, 68.0, 16.0, gold);
            p.circ(0.0, -82.0, 10.0, gold);
            p.g.restore();
        }
        "chat" => {
            let pts: Vec<(f32, f32)> = vec![(-85.0, -60.0), (85.0, -60.0), (85.0, 40.0), (-10.0, 40.0), (-45.0, 80.0), (-40.0, 40.0), (-85.0, 40.0)];
            p.g.rrect(-85.0, -60.0, 170.0, 100.0, 24.0, Some(b), Some((p.ink, p.lw)));
            p.g.poly(&pts[3..6], Some(b), None);
            p.line(&[(-10.0, 40.0), (-45.0, 80.0), (-40.0, 40.0)]);
            p.g.rect(-38.0, 34.0, 26.0, 8.0, b);
            for i in 0..3 {
                let up = ((t * 6.0 - i as f32 * 0.8).sin().max(0.0)) * 8.0;
                p.g.circle(-35.0 + i as f32 * 35.0, -10.0 - up, 10.0, Some(c), None);
            }
        }
        "mail" => {
            p.rr(-90.0, -60.0, 180.0, 120.0, 10.0, b);
            p.line(&[(-90.0, -55.0), (0.0, 15.0), (90.0, -55.0)]);
            let k = (t * 3.0).sin() * 0.5 + 0.5;
            p.g.glow(70.0, -55.0, 30.0, red.a(k));
            p.circ(80.0, -60.0, 16.0, red);
        }
        "person" => {
            let bob = (t * 2.0).sin() * 3.0;
            p.g.save();
            p.g.translate(0.0, bob);
            let body: Vec<(f32, f32)> = (0..=16)
                .map(|i| {
                    let a = PI + PI * i as f32 / 16.0;
                    (a.cos() * 70.0, 85.0 + a.sin() * 75.0)
                })
                .collect();
            p.poly(&body, c);
            p.circ(0.0, -40.0, 45.0, b);
            p.dot(-14.0, -45.0, 5.0);
            p.dot(14.0, -45.0, 5.0);
            let sm = bezier((-14.0, -25.0), (0.0, -15.0), (14.0, -25.0), 8);
            p.line(&sm);
            p.g.restore();
        }
        "people" => {
            for (i, (x, s)) in [(-55.0, 0.75), (55.0, 0.75), (0.0, 0.9)].iter().enumerate() {
                let bob = (t * 2.0 + i as f32).sin() * 3.0;
                p.g.save();
                p.g.translate(*x, 15.0 + bob);
                p.g.scale(*s);
                let body: Vec<(f32, f32)> = (0..=16)
                    .map(|j| {
                        let a = PI + PI * j as f32 / 16.0;
                        (a.cos() * 60.0, 90.0 + a.sin() * 65.0)
                    })
                    .collect();
                p.poly(&body, p.th.accent(i + 1));
                p.circ(0.0, -15.0, 38.0, b);
                p.dot(-12.0, -18.0, 4.5);
                p.dot(12.0, -18.0, 4.5);
                p.g.restore();
            }
        }
        "robot" => {
            let blink = (t * 0.7).fract() > 0.93;
            p.seg((0.0, -70.0), (0.0, -95.0));
            p.circ(0.0, -98.0, 10.0, red);
            p.rr(-75.0, -70.0, 150.0, 120.0, 24.0, hex(0xC9D3DF));
            p.rr(-90.0, -35.0, 16.0, 45.0, 6.0, hex(0x9AA3AF));
            p.rr(74.0, -35.0, 16.0, 45.0, 6.0, hex(0x9AA3AF));
            for x in [-30.0, 30.0] {
                if blink {
                    p.seg((x - 14.0, -15.0), (x + 14.0, -15.0));
                } else {
                    p.circ(x, -15.0, 16.0, blue);
                    p.g.circle(x + 5.0, -20.0, 5.0, Some(hex(0xFFFFFF)), None);
                }
            }
            p.rr(-35.0, 18.0, 70.0, 16.0, 6.0, p.ink);
            for i in 0..4 {
                p.g.rect(-28.0 + i as f32 * 17.0, 21.0, 8.0, 10.0, gold);
            }
            p.rr(-50.0, 50.0, 100.0, 45.0, 10.0, hex(0xC9D3DF));
        }
        "computer" => {
            p.rr(-90.0, -75.0, 180.0, 115.0, 12.0, hex(0x3A3F4B));
            p.g.rrect(-78.0, -63.0, 156.0, 91.0, 6.0, Some(blue.lighten(0.3)), None);
            let n = ((t * 3.0) as usize) % 5;
            for i in 0..4 {
                let w = [90.0, 60.0, 110.0, 40.0][i];
                let col = if i == n % 4 { gold } else { hex(0xFFFFFF).a(0.8) };
                p.g.rrect(-62.0, -50.0 + i as f32 * 20.0, w, 10.0, 5.0, Some(col), None);
            }
            p.poly(&[(-100.0, 45.0), (100.0, 45.0), (115.0, 75.0), (-115.0, 75.0)], hex(0x9AA3AF));
        }
        "phone" => {
            p.rr(-50.0, -95.0, 100.0, 190.0, 18.0, hex(0x2D3140));
            p.g.rrect(-40.0, -78.0, 80.0, 150.0, 8.0, Some(blue.lighten(0.4)), None);
            let k = ((t * 1.5) as usize) % 3;
            for i in 0..3 {
                let left = i % 2 == 0;
                let col = if left { hex(0xFFFFFF) } else { green.lighten(0.2) };
                if i <= k {
                    p.g.rrect(if left { -34.0 } else { -6.0 }, -66.0 + i as f32 * 42.0, 40.0, 30.0, 10.0, Some(col), None);
                }
            }
            p.g.circle(0.0, 83.0, 6.0, Some(hex(0x888888)), None);
        }
        "code" => {
            p.rr(-90.0, -70.0, 180.0, 140.0, 14.0, hex(0x26293A));
            p.g.text(Font::Display, "</>", 76.0, 0.0, 0.0, Align::Center, gold);
            if (t * 2.0).fract() > 0.5 {
                p.g.rect(48.0, 18.0, 16.0, 6.0, hex(0xFFFFFF));
            }
        }
        "database" => {
            for i in (0..3).rev() {
                let y = -50.0 + i as f32 * 45.0;
                p.rr(-70.0, y, 140.0, 45.0, 0.0, p.th.accent(i + 1));
                p.ell(0.0, y + 45.0, 70.0, 18.0, p.th.accent(i + 1));
                p.ell(0.0, y, 70.0, 18.0, p.th.accent(i + 1).lighten(0.25));
                let on = ((t * 3.0 + i as f32) as usize) % 2 == 0;
                p.g.circle(45.0, y + 25.0, 6.0, Some(if on { green } else { hex(0x555555) }), None);
            }
        }
        "wifi" => {
            let k = ((t * 2.0).fract() * 4.0) as usize;
            for i in 0..3 {
                let r = 40.0 + i as f32 * 35.0;
                let arc: Vec<(f32, f32)> = (0..=14)
                    .map(|j| {
                        let a = -PI * 0.75 + PI * 0.5 * j as f32 / 14.0;
                        (a.cos() * r, 60.0 + a.sin() * r)
                    })
                    .collect();
                let col = if i < k { c } else { p.ink.a(0.25) };
                let lw = p.lw;
                p.g.line(&arc, col, lw * 2.6);
            }
            p.circ(0.0, 60.0, 12.0, c);
        }
        "link" => {
            let k = (t * 2.0).sin() * 6.0;
            for (i, s) in [-1.0f32, 1.0].iter().enumerate() {
                p.g.save();
                p.g.translate(s * (32.0 + k), 0.0);
                p.g.rotate(-35.0);
                let lw = p.lw;
                p.g.rrect(-55.0, -28.0, 110.0, 56.0, 28.0, None, Some((p.ink, lw * 4.4)));
                p.g.rrect(-55.0, -28.0, 110.0, 56.0, 28.0, None, Some((p.th.accent(i + 1), lw * 2.2)));
                p.g.restore();
            }
        }
        "recycle" | "exchange" => {
            let n = if canon == "recycle" { 3 } else { 2 };
            p.g.save();
            p.g.rotate(t * 40.0);
            for i in 0..n {
                let a0 = TAU * i as f32 / n as f32 + 0.25;
                let a1 = a0 + TAU / n as f32 - 0.55;
                let pts: Vec<(f32, f32)> = (0..=14)
                    .map(|j| {
                        let a = a0 + (a1 - a0) * j as f32 / 14.0;
                        (a.cos() * 70.0, a.sin() * 70.0)
                    })
                    .collect();
                let col = p.th.accent(i + 3);
                let lw = p.lw;
                p.g.line(&pts, p.ink, lw * 4.0);
                p.g.line(&pts, col, lw * 2.0);
                let tip = pts[pts.len() - 1];
                let dir = (-(a1).sin(), a1.cos());
                let nrm = (a1.cos(), a1.sin());
                p.poly(
                    &[
                        (tip.0 + dir.0 * 28.0, tip.1 + dir.1 * 28.0),
                        (tip.0 + nrm.0 * 24.0, tip.1 + nrm.1 * 24.0),
                        (tip.0 - nrm.0 * 24.0, tip.1 - nrm.1 * 24.0),
                    ],
                    col,
                );
            }
            p.g.restore();
        }
        "music" => {
            let b1 = (t * 4.0).sin() * 6.0;
            p.g.save();
            p.g.translate(0.0, b1);
            p.ell(-45.0, 55.0, 26.0, 20.0, c);
            p.ell(45.0, 35.0, 26.0, 20.0, c);
            p.seg((-22.0, 55.0), (-22.0, -60.0));
            p.seg((68.0, 35.0), (68.0, -80.0));
            p.poly(&[(-22.0, -60.0), (68.0, -80.0), (68.0, -55.0), (-22.0, -35.0)], p.ink);
            p.g.restore();
        }
        "sound" => {
            p.poly(&[(-85.0, -25.0), (-45.0, -25.0), (0.0, -70.0), (0.0, 70.0), (-45.0, 25.0), (-85.0, 25.0)], c);
            for i in 0..3 {
                let r = 35.0 + i as f32 * 25.0 + ((t * 4.0 - i as f32).sin()) * 4.0;
                let arc = bezier((15.0, -r * 0.8), (15.0 + r * 1.2, 0.0), (15.0, r * 0.8), 10);
                p.line(&arc);
            }
        }
        "coffee" => {
            for i in 0..3 {
                let ph = (t * 0.8 + i as f32 * 0.33).fract();
                let x = -25.0 + i as f32 * 25.0 + (ph * 8.0).sin() * 6.0;
                p.g.seg((x, -50.0 - ph * 40.0), (x + 4.0, -70.0 - ph * 40.0), p.ink.a(1.0 - ph), 5.0);
            }
            p.g.circle(65.0, 10.0, 24.0, None, Some((p.ink, p.lw * 3.0)));
            p.g.circle(65.0, 10.0, 24.0, None, Some((b, p.lw)));
            p.poly(&[(-65.0, -40.0), (65.0, -40.0), (50.0, 80.0), (-50.0, 80.0)], b);
            p.g.rect(-62.0, -35.0, 124.0, 30.0, hex(0x7A4A2A));
            p.rr(-60.0, 10.0, 120.0, 18.0, 4.0, c);
        }
        _ => {
            // Unknown prop: a badge with its first letter, so the shot still works.
            let letter: String = raw.chars().find(|c| c.is_alphanumeric()).map(|c| c.to_uppercase().to_string()).unwrap_or("?".into());
            let wob = (t * 2.0).sin() * 4.0;
            p.g.save();
            p.g.rotate(wob);
            p.circ(0.0, 0.0, 82.0, c);
            p.g.circle(0.0, 0.0, 64.0, None, Some((hex(0xFFFFFF).a(0.6), 6.0)));
            p.text(&letter, 100.0, 0.0, 2.0, hex(0xFFFFFF));
            p.g.restore();
        }
    }
    let _ = (a3, gold);
}

fn heart(p: &mut P, cx: f32, cy: f32, r: f32, c: Col) {
    let pts: Vec<(f32, f32)> = (0..48)
        .map(|i| {
            let tt = TAU * i as f32 / 48.0;
            let x = 16.0 * tt.sin().powi(3);
            let y = -(13.0 * tt.cos() - 5.0 * (2.0 * tt).cos() - 2.0 * (3.0 * tt).cos() - (4.0 * tt).cos());
            (cx + x * r / 16.0, cy + y * r / 16.0)
        })
        .collect();
    p.poly(&pts, c);
    p.g.ellipse(cx - r * 0.45, cy - r * 0.35, r * 0.12, r * 0.2, Some(hex(0xFFFFFF).a(0.6)), None);
}

fn drop(p: &mut P, cx: f32, cy: f32, r: f32, c: Col) {
    let pts: Vec<(f32, f32)> = std::iter::once((cx, cy - r * 1.25))
        .chain((0..=20).map(|i| {
            let a = -PI * 0.15 + PI * 1.3 * i as f32 / 20.0;
            (cx + a.cos() * r * 0.75, cy + r * 0.2 + a.sin() * r * 0.75)
        }))
        .collect();
    p.poly(&pts, c);
    p.g.ellipse(cx - r * 0.3, cy + r * 0.05, r * 0.1, r * 0.2, Some(hex(0xFFFFFF).a(0.7)), None);
}

fn puzzle_piece(p: &mut P, cx: f32, cy: f32, c: Col) {
    let s = 40.0;
    let mut pts = vec![(cx - s, cy - s), (cx - 10.0, cy - s)];
    pts.extend((0..=10).map(|i| {
        let a = PI + PI * i as f32 / 10.0;
        (cx + a.cos() * 12.0, cy - s - 10.0 + a.sin() * 14.0)
    }));
    pts.extend([(cx + 10.0, cy - s), (cx + s, cy - s), (cx + s, cy - 10.0)]);
    pts.extend((0..=10).map(|i| {
        let a = -PI / 2.0 + PI * i as f32 / 10.0;
        (cx + s + 10.0 + a.cos() * 14.0, cy + a.sin() * 12.0)
    }));
    pts.extend([(cx + s, cy + 10.0), (cx + s, cy + s), (cx - s, cy + s)]);
    p.poly(&pts, c);
}
