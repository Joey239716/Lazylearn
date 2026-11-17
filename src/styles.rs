//! Visual styles. Every style draws the same shots and props; a style only
//! changes the palette, background, line weight and fonts.

use crate::gfx::{hex, rnd, Col, Font, Gfx};
use clap::ValueEnum;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum Style {
    /// Paper-texture doodles: black line-art, bright cards with offset shadows, gold badges
    #[default]
    Doodle,
    /// Dark cosmic explainer: deep-space gradient, glowing neon props
    Cosmic,
    /// Clean whiteboard with marker colors
    Whiteboard,
    /// Green chalkboard with chalk lines
    Chalk,
    #[serde(other)]
    #[value(skip)]
    Unknown,
}

pub const ALL: &[Style] = &[Style::Doodle, Style::Cosmic, Style::Whiteboard, Style::Chalk];

impl Style {
    pub fn name(self) -> &'static str {
        match self {
            Style::Doodle | Style::Unknown => "doodle",
            Style::Cosmic => "cosmic",
            Style::Whiteboard => "whiteboard",
            Style::Chalk => "chalk",
        }
    }
    pub fn tagline(self) -> &'static str {
        match self {
            Style::Doodle | Style::Unknown => "paper texture, black line-art doodles, bright cards with offset shadows",
            Style::Cosmic => "deep-space gradient, glowing neon props, starry and dramatic",
            Style::Whiteboard => "clean whiteboard, marker colors, crisp and classroom-y",
            Style::Chalk => "green chalkboard, chalk lines, old-school lecture hall",
        }
    }
    pub fn theme(self) -> Theme {
        match self {
            Style::Doodle | Style::Unknown => Theme {
                style: self,
                dark: false,
                ink: hex(0x161616),
                body: hex(0xFFFFFF),
                text: hex(0x161616),
                card: hex(0xFFFFFF),
                shadow_offset: 12.0,
                accents: [hex(0xF2B233), hex(0x3478DC), hex(0xD64034), hex(0x2EAA6A), hex(0x9A5ABF), hex(0xFF7A28)],
                gold: hex(0xF2B233),
                line_w: 6.0,
                glow: false,
                label_font: Font::Display,
                body_font: Font::Hand,
                bar: hex(0xF2B233),
                bar_track: hex(0xCDCDCD),
            },
            Style::Cosmic => Theme {
                style: self,
                dark: true,
                ink: hex(0xE9ECFF),
                body: hex(0x1B1D3A),
                text: hex(0xF4F5FF),
                card: hex(0x24275A),
                shadow_offset: 0.0,
                accents: [hex(0xFFC94A), hex(0x58C7FF), hex(0xFF5E9C), hex(0x6CF0B0), hex(0xB58CFF), hex(0xFF9A4A)],
                gold: hex(0xFFC94A),
                line_w: 5.0,
                glow: true,
                label_font: Font::Display,
                body_font: Font::Display,
                bar: hex(0x58C7FF),
                bar_track: hex(0x2B2E5C),
            },
            Style::Whiteboard => Theme {
                style: self,
                dark: false,
                ink: hex(0x1E2A44),
                body: hex(0xFFFFFF),
                text: hex(0x1E2A44),
                card: hex(0xFFFFFF),
                shadow_offset: 0.0,
                accents: [hex(0x1F6FE5), hex(0xE5392F), hex(0x17A05D), hex(0xF59E0B), hex(0x8B5CF6), hex(0x0EA5B7)],
                gold: hex(0xF59E0B),
                line_w: 5.5,
                glow: false,
                label_font: Font::Hand,
                body_font: Font::Hand,
                bar: hex(0x1F6FE5),
                bar_track: hex(0xDDE3EC),
            },
            Style::Chalk => Theme {
                style: self,
                dark: true,
                ink: hex(0xF3F1E7),
                body: hex(0x2F4A3B),
                text: hex(0xF3F1E7),
                card: hex(0x35523F),
                shadow_offset: 0.0,
                accents: [hex(0xFFE58A), hex(0x8FD3FF), hex(0xFF9C9C), hex(0xA8F0B4), hex(0xD9B8FF), hex(0xFFC58A)],
                gold: hex(0xFFE58A),
                line_w: 5.5,
                glow: false,
                label_font: Font::Hand,
                body_font: Font::Hand,
                bar: hex(0xFFE58A),
                bar_track: hex(0x3F5E4A),
            },
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Theme {
    pub style: Style,
    pub dark: bool,
    /// Outline / line color
    pub ink: Col,
    /// Fill for heads, limbs and plain props
    pub body: Col,
    pub text: Col,
    pub card: Col,
    /// Offset of the colored card shadow (0 = no shadow)
    pub shadow_offset: f32,
    pub accents: [Col; 6],
    pub gold: Col,
    pub line_w: f32,
    pub glow: bool,
    pub label_font: Font,
    pub body_font: Font,
    pub bar: Col,
    pub bar_track: Col,
}

impl Theme {
    pub fn accent(&self, i: usize) -> Col {
        self.accents[i % self.accents.len()]
    }
}

/// Paint the style's background into a fresh canvas (computed once, then copied every frame).
pub fn background(style: Style, w: u32, h: u32) -> Gfx {
    let mut g = Gfx::new(w, h);
    let (wf, hf) = (w as f32, h as f32);
    match style {
        Style::Doodle | Style::Unknown => {
            g.clear(hex(0xF0EFEB));
            grain(&mut g, 7.0, 0.11);
        }
        Style::Cosmic => {
            g.gradient(hex(0x0B0D2A), hex(0x2A1450));
            g.glow(wf * 0.8, hf * 0.15, wf * 0.45, hex(0x5A3CC8).a(0.35));
            g.glow(wf * 0.1, hf * 0.95, wf * 0.4, hex(0x1E6FD0).a(0.3));
            let n = (wf * hf / 9000.0) as u32;
            for i in 0..n {
                let (x, y) = (rnd(i * 3) * wf, rnd(i * 3 + 1) * hf);
                let r = 0.6 + rnd(i * 3 + 2).powi(3) * 2.4;
                g.circle(x, y, r, Some(hex(0xFFFFFF).a(0.35 + 0.6 * rnd(i * 7))), None);
            }
        }
        Style::Whiteboard => {
            g.gradient(hex(0xFBFCFD), hex(0xEEF1F4));
            grain(&mut g, 3.0, 0.04);
            g.glow(wf * 0.3, hf * 0.2, wf * 0.35, hex(0xFFFFFF).a(0.6));
        }
        Style::Chalk => {
            g.gradient(hex(0x2C4637), hex(0x223829));
            grain(&mut g, 10.0, 0.18);
            // faint eraser smudges
            for i in 0..14u32 {
                let (x, y) = (rnd(i * 5) * wf, rnd(i * 5 + 1) * hf);
                g.glow(x, y, 120.0 + rnd(i * 5 + 2) * 260.0, hex(0xFFFFFF).a(0.035));
            }
        }
    }
    g
}

/// Per-pixel noise plus a soft vignette.
fn grain(g: &mut Gfx, amount: f32, vignette: f32) {
    let (w, h) = (g.w as usize, g.h as usize);
    let data = g.pm.data_mut();
    let (cx, cy) = (w as f32 / 2.0, h as f32 / 2.0);
    let maxd = (cx * cx + cy * cy).sqrt();
    for y in 0..h {
        for x in 0..w {
            let i = (y * w + x) * 4;
            let n = (rnd((y * w + x) as u32 ^ 0x9E37_79B9) - 0.5) * amount;
            let d = ((x as f32 - cx).hypot(y as f32 - cy) / maxd).powi(2);
            let v = 1.0 - vignette * d;
            for c in 0..3 {
                let base = data[i + c] as f32;
                data[i + c] = ((base + n) * v).clamp(0.0, 255.0) as u8;
            }
        }
    }
}
