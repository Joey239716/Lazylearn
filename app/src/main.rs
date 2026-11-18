//! Lazylearn: the desktop app for StickStudy. Turn class material into an animated,
//! voiced study video, watch it in the app, then take the quiz.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

extern crate gpui_kit as gpui;

mod app;
mod jobs;
mod library;
mod player;
mod settings;
mod theme;

use gpui::{px, size, App, AppContext, AssetSource, Bounds, Result, SharedString, WindowBounds, WindowOptions};
use gpui_kit::component::{Theme, ThemeMode, TitleBar};
use std::borrow::Cow;

/// Our icons first (Lucide, a few beyond the component set), then the component library's.
struct Assets;

macro_rules! icons {
    ($($n:literal),* $(,)?) => {
        fn own_icon(path: &str) -> Option<&'static [u8]> {
            match path {
                $(concat!("icons/", $n, ".svg") => Some(include_bytes!(concat!("../assets/icons/", $n, ".svg"))),)*
                _ => None,
            }
        }
    };
}
icons!(
    "arrow-up", "audio-lines", "book-open", "brain", "captions", "check", "chevron-down", "chevron-left",
    "chevron-right", "circle-check", "circle-x", "clapperboard", "ellipsis", "external-link", "eye", "file-text",
    "file", "film", "folder-open", "gauge", "graduation-cap", "image", "list-checks", "loader-circle", "maximize",
    "mic", "minimize", "monitor", "palette", "panel-left", "paperclip", "pause", "pen-line", "play", "plus",
    "presentation", "refresh-cw", "rotate-ccw", "search", "settings", "skip-back", "skip-forward", "smartphone",
    "sparkles", "square", "timer", "trash", "triangle-alert", "trophy", "undo-2", "user", "users", "video",
    "volume-2", "volume-x", "wand-sparkles", "x",
);

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if let Some(b) = own_icon(path) {
            return Ok(Some(Cow::Borrowed(b)));
        }
        gpui_kit::assets::Assets.load(path)
    }
    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        gpui_kit::assets::Assets.list(path)
    }
}

fn load_fonts(cx: &mut App) {
    let fonts: Vec<Cow<'static, [u8]>> = vec![
        Cow::Borrowed(include_bytes!("../../assets/fonts/geist/Geist-Regular.ttf")),
        Cow::Borrowed(include_bytes!("../../assets/fonts/geist/Geist-Medium.ttf")),
        Cow::Borrowed(include_bytes!("../../assets/fonts/geist/Geist-SemiBold.ttf")),
        Cow::Borrowed(include_bytes!("../../assets/fonts/geist/GeistMono-Regular.ttf")),
        Cow::Borrowed(include_bytes!("../../assets/fonts/LilitaOne-Regular.ttf")),
    ];
    let _ = cx.text_system().add_fonts(fonts);
}

fn apply_theme(cx: &mut App) {
    Theme::change(ThemeMode::Dark, None, cx);
    Theme::update(cx, |t| {
        use theme::*;
        t.font_family = FONT.into();
        t.mono_font_family = MONO.into();
        t.font_size = px(14.);
        t.radius = px(8.);
        t.radius_lg = px(12.);
        t.shadow = false;
        t.background = bg();
        t.foreground = text();
        t.border = border();
        t.input = border2();
        t.ring = ca(0xffffff, 0.25);
        t.muted = surface2();
        t.muted_foreground = muted();
        t.title_bar = sidebar();
        t.title_bar_border = border();
        t.sidebar = sidebar();
        t.sidebar_border = border();
        t.popover = surface();
        t.popover_foreground = text();
        t.accent = hover();
        t.accent_foreground = text();
        t.primary = text();
        t.primary_hover = c(0xffffff);
        t.primary_active = c(0xd8d8dc);
        t.primary_foreground = c(0x0b0b0c);
        t.secondary = surface2();
        t.secondary_hover = active();
        t.secondary_active = ca(0xffffff, 0.1);
        t.secondary_foreground = text();
        t.selection = ca(0xf2b84b, 0.28);
        t.caret = gold();
        t.scrollbar = gpui::transparent_black();
        t.scrollbar_thumb = ca(0xffffff, 0.12);
        t.scrollbar_thumb_hover = ca(0xffffff, 0.2);
        t.danger = c(0xc4314b);
        t.danger_hover = c(0xd23a55);
        t.danger_active = c(0xa82a40);
        t.danger_foreground = c(0xffffff);
        t.progress_bar = gold();
    });
}

fn main() {
    #[allow(unused_mut)]
    let mut settings = settings::Settings::load();
    // dev: test against another lessons folder, in a window that doesn't take the screen
    #[cfg(debug_assertions)]
    if let Some(d) = std::env::var_os("LAZYLEARN_LESSONS") {
        settings.lessons_dir = d.into();
    }
    let offscreen = cfg!(debug_assertions) && std::env::var_os("LAZYLEARN_OFFSCREEN").is_some();
    settings.apply_env();
    gpui_kit::application().with_assets(Assets).run(move |cx| {
        gpui_kit::init(cx);
        load_fonts(cx);
        apply_theme(cx);
        // closing the window quits the app (GPUI keeps running otherwise)
        cx.on_window_closed(|cx, _| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();
        let mut bounds = Bounds::centered(None, size(px(1280.), px(840.)), cx);
        if offscreen {
            bounds.origin = gpui::point(px(-3000.), px(40.));
        }
        let opts = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            window_min_size: Some(size(px(880.), px(580.))),
            app_id: Some("lazylearn".into()),
            // dev screenshots: don't pop up over whatever the user is doing
            focus: !offscreen,

            ..TitleBar::window_options()
        };
        gpui_kit::open_window(opts, cx, move |window, cx| cx.new(|cx| app::LazyApp::new(settings.clone(), window, cx)))
            .expect("opening the Lazylearn window");
        if !offscreen {
            cx.activate(true);
        }
    });
}
