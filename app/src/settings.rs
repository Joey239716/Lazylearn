//! App settings, stored as a small JSON file in the user's config folder.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// where lessons are made and listed
    pub lessons_dir: PathBuf,
    /// OmniVoice engine folder (STICKSTUDY_OMNIVOICE_DIR); empty = auto-detect
    pub omnivoice_dir: String,
    /// ffmpeg binary (STICKSTUDY_FFMPEG); empty = on PATH
    pub ffmpeg: String,
    /// "gemini" or "claude"
    pub provider: String,
    pub style: String,
    /// "duo" or "solo"
    pub cast: String,
    pub teacher_voice: String,
    pub student_voice: String,
    /// "calm" | "upbeat" | "hype"
    pub energy: String,
    pub pace: f32,
    pub vertical: bool,
    pub vision: bool,
    /// the composer makes a course (several videos) by default
    pub course_mode: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            lessons_dir: default_lessons_dir(),
            omnivoice_dir: String::new(),
            ffmpeg: String::new(),
            provider: "gemini".into(),
            style: "doodle".into(),
            cast: "duo".into(),
            teacher_voice: String::new(),
            student_voice: String::new(),
            energy: "upbeat".into(),
            pace: 1.0,
            vertical: false,
            vision: true,
            course_mode: true,
        }
    }
}

fn config_dir() -> PathBuf {
    #[cfg(windows)]
    let base = std::env::var_os("APPDATA").map(PathBuf::from);
    #[cfg(target_os = "macos")]
    let base = std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Library/Application Support"));
    #[cfg(all(not(windows), not(target_os = "macos")))]
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")));
    base.unwrap_or_else(|| PathBuf::from(".")).join("Lazylearn")
}

/// `lessons/` next to the repo when run from it (dev), else ~/Documents/Lazylearn.
fn default_lessons_dir() -> PathBuf {
    if let Ok(cwd) = std::env::current_dir() {
        for d in cwd.ancestors() {
            if d.join("lessons").is_dir() && d.join("Cargo.toml").is_file() {
                return d.join("lessons");
            }
        }
    }
    let home = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")).map(PathBuf::from);
    home.map(|h| h.join("Documents").join("Lazylearn")).unwrap_or_else(|| PathBuf::from("lessons"))
}

impl Settings {
    fn path() -> PathBuf {
        config_dir().join("settings.json")
    }

    pub fn load() -> Settings {
        std::fs::read_to_string(Self::path()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
    }

    pub fn save(&self) {
        // dev runs against another lessons folder must not overwrite the real settings
        if cfg!(debug_assertions) && std::env::var_os("LAZYLEARN_LESSONS").is_some() {
            return;
        }
        let p = Self::path();
        if let Some(d) = p.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        if let Ok(s) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(p, s);
        }
    }

    /// Push the engine-facing settings into the environment variables the engine reads.
    pub fn apply_env(&self) {
        // SAFETY (edition 2021: set_var is safe): called on the UI thread before jobs start.
        if !self.omnivoice_dir.trim().is_empty() {
            std::env::set_var("STICKSTUDY_OMNIVOICE_DIR", self.omnivoice_dir.trim());
        }
        if !self.ffmpeg.trim().is_empty() {
            std::env::set_var("STICKSTUDY_FFMPEG", self.ffmpeg.trim());
        }
    }

    pub fn ffmpeg_bin(&self) -> String {
        if self.ffmpeg.trim().is_empty() {
            std::env::var("STICKSTUDY_FFMPEG").unwrap_or_else(|_| "ffmpeg".into())
        } else {
            self.ffmpeg.trim().to_string()
        }
    }
}
