use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Speaker {
    Teacher,
    Student,
    Narrator,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum Pose {
    #[default]
    Idle,
    Explain,
    Point,
    Wave,
    Think,
    Excited,
    Shrug,
    Confused,
    #[serde(other)]
    Unknown,
}

pub const POSE_NAMES: &[&str] = &[
    "idle", "explain", "point", "wave", "think", "excited", "shrug", "confused",
];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Line {
    pub speaker: Speaker,
    pub text: String,
    #[serde(default)]
    pub pose: Pose,
    #[serde(default)]
    pub listener_pose: Pose,
    /// Audio file path relative to the lesson folder (filled in by the voice step).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audio: Option<String>,
    /// Seconds of speech (filled in by the voice step).
    #[serde(default)]
    pub duration: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Quiz {
    pub question: String,
    pub choices: Vec<String>,
    pub answer_index: usize,
    #[serde(default)]
    pub explanation: String,
}

/// A concept object drawn in the explainer / whiteboard formats.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Visual {
    pub label: String,
    #[serde(default)]
    pub shape: Shape,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum Shape {
    #[default]
    Circle,
    Square,
    Triangle,
    Hexagon,
    Star,
    #[serde(other)]
    Other,
}
pub const SHAPE_NAMES: &[&str] = &["circle", "square", "triangle", "hexagon", "star"];

/// How a scene's visuals relate to each other.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum Layout {
    /// A -> B -> C (process, cause and effect)
    #[default]
    Flow,
    /// A vs B (differences)
    Compare,
    /// A -> B -> C -> back to A
    Cycle,
    /// parts around a central idea (first visual is the center)
    Hub,
    #[serde(other)]
    Other,
}
pub const LAYOUT_NAMES: &[&str] = &["flow", "compare", "cycle", "hub"];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Scene {
    pub heading: String,
    #[serde(default)]
    pub board: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub visuals: Vec<Visual>,
    #[serde(default)]
    pub layout: Layout,
    pub lines: Vec<Line>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quiz: Option<Quiz>,
}

fn default_teacher() -> String {
    "Prof. Stix".into()
}
fn default_student() -> String {
    "Pip".into()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Lesson {
    pub title: String,
    #[serde(default)]
    pub format: crate::formats::Format,
    #[serde(default = "default_teacher")]
    pub teacher_name: String,
    #[serde(default = "default_student")]
    pub student_name: String,
    pub scenes: Vec<Scene>,
}

impl Lesson {
    pub fn load(dir: &Path) -> Result<Lesson> {
        let p = dir.join("lesson.json");
        let s = std::fs::read_to_string(&p)
            .with_context(|| format!("couldn't read {} (is this a lesson folder?)", p.display()))?;
        serde_json::from_str(&s).with_context(|| format!("{} is not a valid lesson", p.display()))
    }

    pub fn save(&self, dir: &Path) -> Result<()> {
        std::fs::create_dir_all(dir)?;
        std::fs::write(dir.join("lesson.json"), serde_json::to_string_pretty(self)?)?;
        Ok(())
    }

    /// Drop anything malformed so the player never has to deal with it.
    pub fn sanitize(&mut self) {
        for scene in &mut self.scenes {
            scene.lines.retain(|l| !l.text.trim().is_empty());
            scene.board.retain(|b| !b.trim().is_empty());
            scene.visuals.retain(|v| !v.label.trim().is_empty());
            scene.visuals.truncate(5);
            if let Some(q) = &scene.quiz {
                if q.choices.len() < 2 || q.answer_index >= q.choices.len() {
                    scene.quiz = None;
                }
            }
        }
        self.scenes.retain(|s| !s.lines.is_empty());
    }

    pub fn line_count(&self) -> usize {
        self.scenes.iter().map(|s| s.lines.len()).sum()
    }
}
