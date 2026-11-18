//! StickStudy engine: shared by the `stickstudy` CLI and the Lazylearn desktop app.
pub mod audio;
pub mod character;
pub mod course;
pub mod extract;
pub mod gemini;
pub mod gfx;
pub mod icons;
pub mod lesson;
pub mod omnivoice;
pub mod render;
pub mod script;
pub mod shots;
pub mod study;
pub mod styles;
pub mod timeline;
pub mod tts;
pub mod vision;
pub mod pipeline;
pub mod progress;

/// `std::process::Command` that never pops up a console window on Windows
/// (the desktop app is a GUI process, so every child would otherwise get one).
pub fn command(bin: impl AsRef<std::ffi::OsStr>) -> std::process::Command {
    #[allow(unused_mut)]
    let mut c = std::process::Command::new(bin);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        c.creation_flags(CREATE_NO_WINDOW);
    }
    c
}
