//! Lazylearn's look: near-black surfaces, hairline borders, small crisp type (Geist),
//! white pill buttons, colored status dots. One palette, used everywhere.

use gpui::{div, prelude::*, px, rgb, rgba, Div, Hsla, Rgba, SharedString};
use gpui_kit::component::Icon;

pub const FONT: &str = "Geist";
pub const MONO: &str = "Geist Mono";

pub fn c(hex: u32) -> Hsla {
    rgb(hex).into()
}
pub fn ca(hex: u32, a: f32) -> Hsla {
    let r: Rgba = rgba((hex << 8) | ((a.clamp(0.0, 1.0) * 255.0) as u32));
    r.into()
}

// surfaces
pub fn bg() -> Hsla {
    c(0x131417)
}
pub fn sidebar() -> Hsla {
    c(0x0e0f11)
}
pub fn surface() -> Hsla {
    c(0x18191c)
}
pub fn surface2() -> Hsla {
    c(0x1e1f23)
}
pub fn hover() -> Hsla {
    ca(0xffffff, 0.045)
}
pub fn active() -> Hsla {
    ca(0xffffff, 0.075)
}
pub fn border() -> Hsla {
    c(0x232428)
}
pub fn border2() -> Hsla {
    c(0x2d2e33)
}
// text
pub fn text() -> Hsla {
    c(0xececee)
}
pub fn muted() -> Hsla {
    c(0x8e9098)
}
pub fn faint() -> Hsla {
    c(0x5d5f67)
}
// accents
pub fn gold() -> Hsla {
    c(0xf2b84b)
}
pub fn green() -> Hsla {
    c(0x3ecf8e)
}
pub fn red() -> Hsla {
    c(0xf0645a)
}
pub fn violet() -> Hsla {
    c(0x8b7cf6)
}
pub fn blue() -> Hsla {
    c(0x5aa9f0)
}

pub fn icon(name: &str) -> Icon {
    Icon::empty().path(SharedString::from(format!("icons/{name}.svg")))
}

/// Small colored status dot.
pub fn dot(color: Hsla) -> Div {
    div().size(px(7.)).rounded_full().bg(color).flex_shrink_0()
}

/// Uppercase section label.
pub fn label(s: impl Into<SharedString>) -> Div {
    div().text_size(px(11.)).font_weight(gpui::FontWeight::MEDIUM).text_color(faint()).child(s.into())
}

/// Keyboard hint badge.
pub fn kbd(s: impl Into<SharedString>) -> Div {
    div()
        .px(px(5.))
        .h(px(18.))
        .min_w(px(18.))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(4.))
        .border_1()
        .border_color(border2())
        .text_size(px(11.))
        .font_family(MONO)
        .text_color(muted())
        .child(s.into())
}

/// Thin progress bar.
pub fn bar(frac: f32, color: Hsla) -> Div {
    div()
        .h(px(3.))
        .w_full()
        .rounded_full()
        .bg(ca(0xffffff, 0.07))
        .child(div().h_full().rounded_full().bg(color).w(gpui::relative(frac.clamp(0.0, 1.0))))
}
