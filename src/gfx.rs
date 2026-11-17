//! Drawing layer: a tiny-skia canvas with a transform/alpha stack, simple shape
//! helpers and vector text (glyph outlines from the bundled fonts, so text stays
//! sharp at any scale or rotation).

use ab_glyph::{Font as _, FontRef, GlyphId, OutlineCurve};
use std::collections::HashMap;
use std::sync::OnceLock;
use tiny_skia::{
    FillRule, LineCap, LineJoin, Paint, Path, PathBuilder, Pixmap, Rect, Shader, Stroke, Transform,
};

// ------------------------------------------------------------------ color

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Col(pub f32, pub f32, pub f32, pub f32);

pub const fn hex(h: u32) -> Col {
    Col(
        ((h >> 16) & 255) as f32 / 255.0,
        ((h >> 8) & 255) as f32 / 255.0,
        (h & 255) as f32 / 255.0,
        1.0,
    )
}

impl Col {
    pub fn a(self, a: f32) -> Col {
        Col(self.0, self.1, self.2, self.3 * a)
    }
    pub fn mix(self, o: Col, t: f32) -> Col {
        Col(
            self.0 + (o.0 - self.0) * t,
            self.1 + (o.1 - self.1) * t,
            self.2 + (o.2 - self.2) * t,
            self.3 + (o.3 - self.3) * t,
        )
    }
    pub fn lighten(self, t: f32) -> Col {
        self.mix(Col(1.0, 1.0, 1.0, self.3), t)
    }
    pub fn darken(self, t: f32) -> Col {
        self.mix(Col(0.0, 0.0, 0.0, self.3), t)
    }
    fn sk(self, alpha: f32) -> tiny_skia::Color {
        tiny_skia::Color::from_rgba(
            self.0.clamp(0.0, 1.0),
            self.1.clamp(0.0, 1.0),
            self.2.clamp(0.0, 1.0),
            (self.3 * alpha).clamp(0.0, 1.0),
        )
        .unwrap()
    }
}

// ------------------------------------------------------------------ fonts

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Font {
    /// Chunky rounded display font (titles, labels, captions)
    Display,
    /// Handwriting (speech bubbles, notes)
    Hand,
}

static DISPLAY_TTF: &[u8] = include_bytes!("../assets/fonts/LilitaOne-Regular.ttf");
static HAND_TTF: &[u8] = include_bytes!("../assets/fonts/PatrickHand-Regular.ttf");

fn font_ref(f: Font) -> &'static FontRef<'static> {
    static D: OnceLock<FontRef<'static>> = OnceLock::new();
    static H: OnceLock<FontRef<'static>> = OnceLock::new();
    match f {
        Font::Display => D.get_or_init(|| FontRef::try_from_slice(DISPLAY_TTF).expect("display font")),
        Font::Hand => H.get_or_init(|| FontRef::try_from_slice(HAND_TTF).expect("hand font")),
    }
}

/// Horizontal anchor for text.
#[derive(Clone, Copy, PartialEq)]
#[allow(dead_code)]
pub enum Align {
    Left,
    Center,
    Right,
}

// ------------------------------------------------------------------ paths

pub fn circle_path(cx: f32, cy: f32, r: f32) -> Option<Path> {
    PathBuilder::from_circle(cx, cy, r.max(0.01))
}

pub fn ellipse_path(cx: f32, cy: f32, rx: f32, ry: f32) -> Option<Path> {
    Rect::from_xywh(cx - rx, cy - ry, (rx * 2.0).max(0.01), (ry * 2.0).max(0.01)).and_then(PathBuilder::from_oval)
}

pub fn rrect_path(x: f32, y: f32, w: f32, h: f32, r: f32) -> Option<Path> {
    let r = r.min(w / 2.0).min(h / 2.0).max(0.0);
    let k = 0.5523 * r;
    let mut pb = PathBuilder::new();
    pb.move_to(x + r, y);
    pb.line_to(x + w - r, y);
    pb.cubic_to(x + w - r + k, y, x + w, y + r - k, x + w, y + r);
    pb.line_to(x + w, y + h - r);
    pb.cubic_to(x + w, y + h - r + k, x + w - r + k, y + h, x + w - r, y + h);
    pb.line_to(x + r, y + h);
    pb.cubic_to(x + r - k, y + h, x, y + h - r + k, x, y + h - r);
    pb.line_to(x, y + r);
    pb.cubic_to(x, y + r - k, x + r - k, y, x + r, y);
    pb.close();
    pb.finish()
}

pub fn poly_path(pts: &[(f32, f32)], closed: bool) -> Option<Path> {
    let mut pb = PathBuilder::new();
    let (first, rest) = pts.split_first()?;
    pb.move_to(first.0, first.1);
    for p in rest {
        pb.line_to(p.0, p.1);
    }
    if closed {
        pb.close();
    }
    pb.finish()
}

/// Quadratic bezier sampled into points (for arrows that draw on).
pub fn bezier(a: (f32, f32), c: (f32, f32), b: (f32, f32), n: usize) -> Vec<(f32, f32)> {
    (0..=n)
        .map(|i| {
            let t = i as f32 / n as f32;
            let u = 1.0 - t;
            (u * u * a.0 + 2.0 * u * t * c.0 + t * t * b.0, u * u * a.1 + 2.0 * u * t * c.1 + t * t * b.1)
        })
        .collect()
}

/// The first `frac` (0..1) of a polyline by length.
pub fn trim(pts: &[(f32, f32)], frac: f32) -> Vec<(f32, f32)> {
    if frac >= 1.0 || pts.len() < 2 {
        return pts.to_vec();
    }
    let lens: Vec<f32> = pts.windows(2).map(|w| (w[1].0 - w[0].0).hypot(w[1].1 - w[0].1)).collect();
    let total: f32 = lens.iter().sum();
    let mut want = total * frac.max(0.0);
    let mut out = vec![pts[0]];
    for (i, l) in lens.iter().enumerate() {
        if want >= *l {
            out.push(pts[i + 1]);
            want -= l;
        } else {
            let t = if *l > 0.0 { want / l } else { 0.0 };
            out.push((pts[i].0 + (pts[i + 1].0 - pts[i].0) * t, pts[i].1 + (pts[i + 1].1 - pts[i].1) * t));
            break;
        }
    }
    out
}

// ------------------------------------------------------------------ canvas

pub struct Gfx {
    pub pm: Pixmap,
    pub w: f32,
    pub h: f32,
    ts: Transform,
    alpha: f32,
    stack: Vec<(Transform, f32)>,
    glyphs: HashMap<(Font, GlyphId), Option<Path>>,
}

impl Gfx {
    pub fn new(w: u32, h: u32) -> Gfx {
        Gfx {
            pm: Pixmap::new(w, h).expect("pixmap"),
            w: w as f32,
            h: h as f32,
            ts: Transform::identity(),
            alpha: 1.0,
            stack: vec![],
            glyphs: HashMap::new(),
        }
    }

    // ---- state
    pub fn save(&mut self) {
        self.stack.push((self.ts, self.alpha));
    }
    pub fn restore(&mut self) {
        if let Some((t, a)) = self.stack.pop() {
            self.ts = t;
            self.alpha = a;
        }
    }
    pub fn reset(&mut self) {
        self.ts = Transform::identity();
        self.alpha = 1.0;
        self.stack.clear();
    }
    pub fn translate(&mut self, x: f32, y: f32) {
        self.ts = self.ts.pre_translate(x, y);
    }
    pub fn scale(&mut self, s: f32) {
        self.ts = self.ts.pre_scale(s.max(0.0001), s.max(0.0001));
    }
    pub fn scale_xy(&mut self, sx: f32, sy: f32) {
        self.ts = self.ts.pre_scale(sx.max(0.0001), sy.max(0.0001));
    }
    /// Rotate by `deg` degrees around the current origin.
    pub fn rotate(&mut self, deg: f32) {
        self.ts = self.ts.pre_concat(Transform::from_rotate(deg));
    }
    /// Scale and rotate around a point.
    pub fn around(&mut self, cx: f32, cy: f32, s: f32, deg: f32) {
        self.translate(cx, cy);
        if deg != 0.0 {
            self.rotate(deg);
        }
        self.scale(s);
        self.translate(-cx, -cy);
    }
    pub fn fade(&mut self, a: f32) {
        self.alpha *= a.clamp(0.0, 1.0);
    }
    // ---- raw
    fn paint(&self, c: Col) -> Paint<'static> {
        let mut p = Paint::default();
        p.shader = Shader::SolidColor(c.sk(self.alpha));
        p.anti_alias = true;
        p
    }
    pub fn fill(&mut self, path: &Path, c: Col) {
        if self.alpha <= 0.002 || c.3 <= 0.0 {
            return;
        }
        let p = self.paint(c);
        self.pm.fill_path(path, &p, FillRule::Winding, self.ts, None);
    }
    pub fn fill_eo(&mut self, path: &Path, c: Col) {
        if self.alpha <= 0.002 {
            return;
        }
        let p = self.paint(c);
        self.pm.fill_path(path, &p, FillRule::EvenOdd, self.ts, None);
    }
    pub fn stroke(&mut self, path: &Path, c: Col, width: f32) {
        if self.alpha <= 0.002 || width <= 0.0 || c.3 <= 0.0 {
            return;
        }
        let p = self.paint(c);
        let s = Stroke { width, line_cap: LineCap::Round, line_join: LineJoin::Round, ..Default::default() };
        self.pm.stroke_path(path, &p, &s, self.ts, None);
    }
    pub fn shape(&mut self, path: Option<Path>, fill: Option<Col>, stroke: Option<(Col, f32)>) {
        if let Some(p) = path {
            if let Some(f) = fill {
                self.fill(&p, f);
            }
            if let Some((c, w)) = stroke {
                self.stroke(&p, c, w);
            }
        }
    }

    pub fn clear(&mut self, c: Col) {
        self.pm.fill(c.sk(1.0));
    }

    /// Fill the whole canvas (ignoring the transform) with a vertical gradient.
    pub fn gradient(&mut self, top: Col, bottom: Col) {
        let mut p = Paint::default();
        p.shader = tiny_skia::LinearGradient::new(
            tiny_skia::Point::from_xy(0.0, 0.0),
            tiny_skia::Point::from_xy(0.0, self.h),
            vec![
                tiny_skia::GradientStop::new(0.0, top.sk(1.0)),
                tiny_skia::GradientStop::new(1.0, bottom.sk(1.0)),
            ],
            tiny_skia::SpreadMode::Pad,
            Transform::identity(),
        )
        .unwrap();
        let r = Rect::from_xywh(0.0, 0.0, self.w, self.h).unwrap();
        self.pm.fill_rect(r, &p, Transform::identity(), None);
    }

    /// Soft radial glow (for highlights and the cosmic look).
    pub fn glow(&mut self, cx: f32, cy: f32, r: f32, c: Col) {
        if self.alpha <= 0.002 {
            return;
        }
        let mut p = Paint::default();
        p.anti_alias = true;
        if let Some(sh) = tiny_skia::RadialGradient::new(
            tiny_skia::Point::from_xy(cx, cy),
            tiny_skia::Point::from_xy(cx, cy),
            r.max(1.0),
            vec![
                tiny_skia::GradientStop::new(0.0, c.sk(self.alpha)),
                tiny_skia::GradientStop::new(1.0, c.a(0.0).sk(1.0)),
            ],
            tiny_skia::SpreadMode::Pad,
            Transform::identity(),
        ) {
            p.shader = sh;
            if let Some(path) = circle_path(cx, cy, r) {
                self.pm.fill_path(&path, &p, FillRule::Winding, self.ts, None);
            }
        }
    }

    // ---- shapes
    pub fn circle(&mut self, cx: f32, cy: f32, r: f32, fill: Option<Col>, stroke: Option<(Col, f32)>) {
        self.shape(circle_path(cx, cy, r), fill, stroke);
    }
    pub fn ellipse(&mut self, cx: f32, cy: f32, rx: f32, ry: f32, fill: Option<Col>, stroke: Option<(Col, f32)>) {
        self.shape(ellipse_path(cx, cy, rx, ry), fill, stroke);
    }
    pub fn rrect(&mut self, x: f32, y: f32, w: f32, h: f32, r: f32, fill: Option<Col>, stroke: Option<(Col, f32)>) {
        self.shape(rrect_path(x, y, w, h, r), fill, stroke);
    }
    pub fn rect(&mut self, x: f32, y: f32, w: f32, h: f32, c: Col) {
        if let Some(r) = Rect::from_xywh(x, y, w.max(0.01), h.max(0.01)) {
            self.fill(&PathBuilder::from_rect(r), c);
        }
    }
    pub fn poly(&mut self, pts: &[(f32, f32)], fill: Option<Col>, stroke: Option<(Col, f32)>) {
        self.shape(poly_path(pts, true), fill, stroke);
    }
    pub fn line(&mut self, pts: &[(f32, f32)], c: Col, w: f32) {
        if pts.len() >= 2 {
            if let Some(p) = poly_path(pts, false) {
                self.stroke(&p, c, w);
            }
        }
    }
    pub fn seg(&mut self, a: (f32, f32), b: (f32, f32), c: Col, w: f32) {
        self.line(&[a, b], c, w);
    }

    /// Arrow from a to b, bent by `bend` (fraction of length), drawn up to `frac`.
    pub fn arrow(&mut self, a: (f32, f32), b: (f32, f32), bend: f32, c: Col, w: f32, frac: f32) {
        let (mx, my) = ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0);
        let (nx, ny) = (-(b.1 - a.1), b.0 - a.0);
        let pts = bezier(a, (mx + nx * bend, my + ny * bend), b, 28);
        let part = trim(&pts, frac);
        self.line(&part, c, w);
        if frac > 0.85 && part.len() >= 2 {
            let k = ((frac - 0.85) / 0.15).min(1.0);
            let tip = *part.last().unwrap();
            let prev = part[part.len().saturating_sub(4)];
            let ang = (tip.1 - prev.1).atan2(tip.0 - prev.0);
            let l = w * 4.2 * k;
            for da in [2.55f32, -2.55] {
                self.seg(tip, (tip.0 + l * (ang + da).cos(), tip.1 + l * (ang + da).sin()), c, w);
            }
        }
    }

    // ---- text
    fn glyph_path(&mut self, font: Font, id: GlyphId) -> Option<Path> {
        if let Some(p) = self.glyphs.get(&(font, id)) {
            return p.clone();
        }
        let f = font_ref(font);
        let path = f.outline(id).and_then(|o| {
            let mut pb = PathBuilder::new();
            let mut cur: Option<(f32, f32)> = None;
            let flip = |p: ab_glyph::Point| (p.x, -p.y);
            for c in &o.curves {
                let (start, end) = match c {
                    OutlineCurve::Line(a, b) => (flip(*a), flip(*b)),
                    OutlineCurve::Quad(a, _, b) => (flip(*a), flip(*b)),
                    OutlineCurve::Cubic(a, _, _, b) => (flip(*a), flip(*b)),
                };
                if cur.map_or(true, |p| (p.0 - start.0).abs() > 0.01 || (p.1 - start.1).abs() > 0.01) {
                    if cur.is_some() {
                        pb.close();
                    }
                    pb.move_to(start.0, start.1);
                }
                match c {
                    OutlineCurve::Line(_, b) => {
                        let b = flip(*b);
                        pb.line_to(b.0, b.1)
                    }
                    OutlineCurve::Quad(_, c1, b) => {
                        let (c1, b) = (flip(*c1), flip(*b));
                        pb.quad_to(c1.0, c1.1, b.0, b.1)
                    }
                    OutlineCurve::Cubic(_, c1, c2, b) => {
                        let (c1, c2, b) = (flip(*c1), flip(*c2), flip(*b));
                        pb.cubic_to(c1.0, c1.1, c2.0, c2.1, b.0, b.1)
                    }
                }
                cur = Some(end);
            }
            if cur.is_some() {
                pb.close();
            }
            pb.finish()
        });
        self.glyphs.insert((font, id), path.clone());
        path
    }

    fn units(font: Font, size: f32) -> f32 {
        size / font_ref(font).units_per_em().unwrap_or(1000.0)
    }

    /// Pixel layout of `s`: (font, glyph, x offset in px) per char, plus total width.
    /// Characters missing from `font` fall back to the other bundled font.
    fn layout_glyphs(font: Font, s: &str, size: f32) -> (Vec<(Font, GlyphId, f32)>, f32) {
        let other = if font == Font::Display { Font::Hand } else { Font::Display };
        let mut out = vec![];
        let mut pen = 0.0;
        let mut prev: Option<(Font, GlyphId)> = None;
        for ch in s.chars() {
            let mut fnt = font;
            let mut id = font_ref(font).glyph_id(ch);
            if id.0 == 0 && !ch.is_whitespace() {
                let alt = font_ref(other).glyph_id(ch);
                if alt.0 != 0 {
                    fnt = other;
                    id = alt;
                }
            }
            let f = font_ref(fnt);
            let k = Self::units(fnt, size);
            if let Some((pf, pid)) = prev {
                if pf == fnt {
                    pen += f.kern_unscaled(pid, id) * k;
                }
            }
            out.push((fnt, id, pen));
            pen += f.h_advance_unscaled(id) * k;
            prev = Some((fnt, id));
        }
        (out, pen)
    }

    /// Width of `s` in pixels at `size`.
    pub fn measure(&self, font: Font, s: &str, size: f32) -> f32 {
        Self::layout_glyphs(font, s, size).1
    }

    /// Greedy word wrap.
    pub fn wrap(&self, font: Font, s: &str, size: f32, max_w: f32) -> Vec<String> {
        let mut lines = vec![];
        let mut cur = String::new();
        for word in s.split_whitespace() {
            let trial = if cur.is_empty() { word.to_string() } else { format!("{cur} {word}") };
            if self.measure(font, &trial, size) > max_w && !cur.is_empty() {
                lines.push(std::mem::take(&mut cur));
                cur = word.to_string();
            } else {
                cur = trial;
            }
        }
        if !cur.is_empty() {
            lines.push(cur);
        }
        lines
    }

    /// Largest size (<= `size`) at which `s` wraps into at most `max_lines` lines of `max_w`.
    pub fn fit(&self, font: Font, s: &str, mut size: f32, max_w: f32, max_lines: usize) -> (f32, Vec<String>) {
        loop {
            let lines = self.wrap(font, s, size, max_w);
            let widest = lines.iter().map(|l| self.measure(font, l, size)).fold(0.0, f32::max);
            if (lines.len() <= max_lines && widest <= max_w) || size < 14.0 {
                return (size, lines);
            }
            size *= 0.92;
        }
    }

    /// Draw one line of text. `y` is the vertical middle of capital letters.
    /// `reveal` = how many characters are visible (fractional fades the next one in).
    pub fn text_ex(
        &mut self,
        font: Font,
        s: &str,
        size: f32,
        x: f32,
        y: f32,
        align: Align,
        c: Col,
        outline: Option<(Col, f32)>,
        reveal: f32,
    ) -> f32 {
        let (glyphs, w) = Self::layout_glyphs(font, s, size);
        let x0 = match align {
            Align::Left => x,
            Align::Center => x - w / 2.0,
            Align::Right => x - w,
        };
        let base = y + size * 0.34;
        let saved = (self.ts, self.alpha);
        for pass in 0..2 {
            if pass == 0 && outline.is_none() {
                continue;
            }
            for (i, &(fnt, id, px)) in glyphs.iter().enumerate() {
                let vis = (reveal - i as f32).clamp(0.0, 1.0);
                if vis <= 0.0 {
                    break;
                }
                let k = Self::units(fnt, size);
                if let Some(path) = self.glyph_path(fnt, id) {
                    self.ts = saved.0.pre_translate(x0 + px, base).pre_scale(k, k);
                    self.alpha = saved.1 * vis;
                    if pass == 0 {
                        let (oc, ow) = outline.unwrap();
                        self.stroke(&path, oc, ow / k);
                    } else {
                        self.fill(&path, c);
                    }
                }
            }
        }
        self.ts = saved.0;
        self.alpha = saved.1;
        w
    }

    pub fn text(&mut self, font: Font, s: &str, size: f32, x: f32, y: f32, align: Align, c: Col) -> f32 {
        self.text_ex(font, s, size, x, y, align, c, None, f32::MAX)
    }

    /// Several lines centered on (x, y).
    pub fn text_block(
        &mut self,
        font: Font,
        lines: &[String],
        size: f32,
        x: f32,
        y: f32,
        align: Align,
        c: Col,
        outline: Option<(Col, f32)>,
        reveal: f32,
    ) {
        let lh = size * 1.15;
        let top = y - lh * (lines.len() as f32 - 1.0) / 2.0;
        let mut shown = 0.0;
        for (i, l) in lines.iter().enumerate() {
            let n = l.chars().count() as f32;
            self.text_ex(font, l, size, x, top + lh * i as f32, align, c, outline, reveal - shown);
            shown += n + 1.0;
        }
    }

    // ---- output
    /// RGBA bytes (the background is opaque, so premultiplied == straight alpha).
    pub fn bytes(&self) -> &[u8] {
        self.pm.data()
    }

    pub fn save_png(&self, path: &std::path::Path) -> anyhow::Result<()> {
        self.pm.save_png(path).map_err(|e| anyhow::anyhow!("saving {}: {e}", path.display()))
    }
}

// ------------------------------------------------------------------ easing

pub fn clamp01(x: f32) -> f32 {
    x.clamp(0.0, 1.0)
}
pub fn ease_out(x: f32) -> f32 {
    1.0 - (1.0 - clamp01(x)).powi(3)
}
pub fn ease_in_out(x: f32) -> f32 {
    let x = clamp01(x);
    if x < 0.5 {
        4.0 * x * x * x
    } else {
        1.0 - (-2.0 * x + 2.0).powi(3) / 2.0
    }
}
pub fn ease_back(x: f32) -> f32 {
    let x = clamp01(x);
    let c1 = 1.9;
    let c3 = c1 + 1.0;
    1.0 + c3 * (x - 1.0).powi(3) + c1 * (x - 1.0).powi(2)
}
/// Springy settle: overshoots and wobbles, ends at 1.
pub fn elastic(x: f32) -> f32 {
    let x = clamp01(x);
    if x <= 0.0 || x >= 1.0 {
        return x;
    }
    let c4 = std::f32::consts::TAU / 3.2;
    2f32.powf(-10.0 * x) * ((x * 10.0 - 0.75) * c4).sin() + 1.0
}
/// Progress of an animation that starts at `start` and lasts `dur`.
pub fn prog(t: f32, start: f32, dur: f32) -> f32 {
    clamp01((t - start) / dur.max(0.001))
}
/// Deterministic pseudo-random in [0,1).
pub fn rnd(i: u32) -> f32 {
    let mut x = i.wrapping_mul(747796405).wrapping_add(2891336453);
    x = ((x >> ((x >> 28) + 4)) ^ x).wrapping_mul(277803737);
    x = (x >> 22) ^ x;
    x as f32 / u32::MAX as f32
}
