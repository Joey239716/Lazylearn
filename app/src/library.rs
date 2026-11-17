//! The projects in the lessons folder (one folder per lesson), newest first.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use stickstudy::pipeline::LessonInfo;

#[derive(Debug, Clone)]
pub struct Project {
    pub dir: PathBuf,
    pub title: String,
    /// seconds (0 = unknown)
    pub duration: f32,
    pub created: u64,
    pub style: String,
    pub inputs: Vec<String>,
    pub focus: Option<String>,
    pub thumb: Option<PathBuf>,
    pub video: Option<PathBuf>,
    pub vertical_video: Option<PathBuf>,
    pub quiz: Option<QuizResult>,
    pub question_count: usize,
}

impl Project {
    pub fn load(dir: &Path) -> Option<Project> {
        let info = LessonInfo::load(dir);
        let has_lesson = dir.join("lesson.json").is_file();
        if info.is_none() && !has_lesson {
            return None;
        }
        let info = info.unwrap_or_default();
        let mut title = info.title.clone();
        if title.is_empty() && has_lesson {
            title = stickstudy::lesson::Lesson::load(dir).map(|l| l.title).unwrap_or_default();
        }
        if title.is_empty() {
            title = dir.file_name().map(|n| n.to_string_lossy().replace(['-', '_'], " ")).unwrap_or_default();
        }
        let created = if info.created > 0 {
            info.created
        } else {
            std::fs::metadata(dir)
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs())
                .unwrap_or(0)
        };
        let some = |p: PathBuf| p.is_file().then_some(p);
        let question_count = std::fs::read_to_string(dir.join("quiz.json"))
            .ok()
            .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
            .and_then(|v| v.get("questions").and_then(|q| q.as_array()).map(|a| a.len()))
            .unwrap_or(0);
        Some(Project {
            dir: dir.to_path_buf(),
            title,
            duration: info.duration,
            created,
            style: info.style,
            inputs: info.inputs,
            focus: info.focus,
            thumb: some(dir.join("thumb.png")),
            video: some(dir.join("video.mp4")),
            vertical_video: some(dir.join("video-vertical.mp4")),
            quiz: QuizResult::load(dir),
            question_count,
        })
    }

    pub fn length_label(&self) -> String {
        if self.duration <= 0.0 {
            return String::new();
        }
        let s = self.duration.round() as u32;
        format!("{}:{:02}", s / 60, s % 60)
    }
}

pub fn scan(lessons_dir: &Path) -> Vec<Project> {
    let mut out: Vec<Project> = std::fs::read_dir(lessons_dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().is_dir())
        .filter_map(|e| Project::load(&e.path()))
        .collect();
    out.sort_by(|a, b| b.created.cmp(&a.created));
    out
}

/// `quiz_results.json`: the best and the latest attempt.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct QuizResult {
    pub score: usize,
    pub total: usize,
    #[serde(default)]
    pub best: usize,
    #[serde(default)]
    pub attempts: u32,
    #[serde(default)]
    pub taken: u64,
}

impl QuizResult {
    pub fn load(dir: &Path) -> Option<QuizResult> {
        serde_json::from_str(&std::fs::read_to_string(dir.join("quiz_results.json")).ok()?).ok()
    }

    pub fn record(dir: &Path, score: usize, total: usize) -> QuizResult {
        let prev = QuizResult::load(dir).unwrap_or_default();
        let r = QuizResult {
            score,
            total,
            best: prev.best.max(score),
            attempts: prev.attempts + 1,
            taken: stickstudy::pipeline::now_secs(),
        };
        if let Ok(s) = serde_json::to_string_pretty(&r) {
            let _ = std::fs::write(dir.join("quiz_results.json"), s);
        }
        r
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct QuizFile {
    #[serde(default)]
    pub questions: Vec<stickstudy::lesson::Quiz>,
}

pub fn load_quiz(dir: &Path) -> Vec<stickstudy::lesson::Quiz> {
    std::fs::read_to_string(dir.join("quiz.json"))
        .ok()
        .and_then(|s| serde_json::from_str::<QuizFile>(&s).ok())
        .map(|q| q.questions)
        .unwrap_or_default()
}

/// "2h ago", "yesterday", "Mar 3"
pub fn ago(t: u64) -> String {
    let now = stickstudy::pipeline::now_secs();
    let d = now.saturating_sub(t);
    match d {
        0..=59 => "just now".into(),
        60..=3599 => format!("{}m ago", d / 60),
        3600..=86_399 => format!("{}h ago", d / 3600),
        86_400..=172_799 => "yesterday".into(),
        _ => format!("{}d ago", d / 86_400),
    }
}
