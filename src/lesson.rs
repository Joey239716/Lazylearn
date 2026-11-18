//! The lesson script: what Claude writes, what TTS fills in, what the renderer draws.

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
    Cheer,
    Shocked,
    Facepalm,
    Sad,
    #[serde(other)]
    Unknown,
}

pub const POSE_NAMES: &[&str] = &[
    "idle", "explain", "point", "wave", "think", "excited", "shrug", "confused", "cheer", "shocked",
    "facepalm", "sad",
];

/// What kind of picture a shot is. Each kind has its own layout and choreography.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum ShotKind {
    /// The cast up close; `text` becomes a speech bubble, `icon` floats beside them.
    #[default]
    Talk,
    /// Kinetic typography: `text` slams in word by word, `emphasis` word is highlighted.
    Title,
    /// One big hero icon with a `label` (and optional `text` underneath).
    Icon,
    /// `title` + 2-4 `items` popping in one after another.
    List,
    /// 2-4 `items` connected left to right by arrows (a process, cause and effect).
    Flow,
    /// 3-5 `items` in a loop.
    Cycle,
    /// Exactly 2 `items`, A versus B, each with a short `note`.
    Compare,
    /// `title` (+ `icon`) in the middle, 2-5 `items` around it.
    Hub,
    /// `title` = the term, `text` = the precise definition, optional `icon`.
    Definition,
    /// `value` (e.g. "70%", "3.8 billion") counts up, `text` explains it.
    Number,
    /// `text` = a formula or equation (symbols allowed, it is only shown, never read aloud).
    Formula,
    /// `title` = acronym, `items` = one word per letter.
    Mnemonic,
    /// `title` = the misconception, `text` = the truth. MYTH stamp, then FACT.
    Myth,
    /// 2-5 `items` along a line; `note` is the date or step number.
    Timeline,
    /// `text` = a question, big question mark, cast thinking.
    Question,
    /// `title` + 2-5 `items` that get ticked off (recaps, checklists).
    Recap,
    #[serde(other)]
    Unknown,
}

pub const SHOT_NAMES: &[&str] = &[
    "talk", "title", "icon", "list", "flow", "cycle", "compare", "hub", "definition", "number", "formula",
    "mnemonic", "myth", "timeline", "question", "recap",
];

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Item {
    pub label: String,
    #[serde(default)]
    pub icon: String,
    #[serde(default)]
    pub note: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Shot {
    pub kind: ShotKind,
    /// The word in the line where this shot should appear (default: shots split the line evenly).
    #[serde(default)]
    pub cue: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub emphasis: String,
    #[serde(default)]
    pub icon: String,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub value: String,
    #[serde(default)]
    pub items: Vec<Item>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Line {
    pub speaker: Speaker,
    pub text: String,
    #[serde(default)]
    pub pose: Pose,
    #[serde(default)]
    pub listener_pose: Pose,
    #[serde(default)]
    pub shots: Vec<Shot>,
    /// Audio file path relative to the lesson folder (filled in by the voice step).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audio: Option<String>,
    /// Seconds of speech after trimming silence (filled in by the voice step).
    #[serde(default)]
    pub duration: f32,
    /// Where the speech starts inside the audio file (leading silence that gets skipped).
    #[serde(default)]
    pub audio_start: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Quiz {
    pub question: String,
    pub choices: Vec<String>,
    pub answer_index: usize,
    #[serde(default)]
    pub explanation: String,
}

impl Quiz {
    /// Put the choices in a random order (AI writers favour one position for the right answer,
    /// so the order is never left to them). Keeps `answer_index` pointing at the right choice.
    pub fn shuffle(&mut self) {
        let n = self.choices.len();
        if n < 2 || self.answer_index >= n {
            return;
        }
        let right = self.choices[self.answer_index].clone();
        for i in (1..n).rev() {
            let j = (random_u64() % (i as u64 + 1)) as usize;
            self.choices.swap(i, j);
        }
        self.answer_index = self.choices.iter().position(|c| *c == right).unwrap_or(0);
    }
}

/// Small random numbers without an extra crate: the std hasher is seeded randomly per process,
/// mixed with a counter and the clock.
pub(crate) fn random_u64() -> u64 {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let mut h = RandomState::new().build_hasher();
    h.write_u64(COUNTER.fetch_add(1, Ordering::Relaxed));
    h.write_u128(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0));
    h.finish()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Scene {
    pub heading: String,
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
    pub style: crate::styles::Style,
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

    /// Drop anything malformed so the renderer never has to deal with it.
    pub fn sanitize(&mut self) {
        for scene in &mut self.scenes {
            scene.lines.retain(|l| !l.text.trim().is_empty());
            for line in &mut scene.lines {
                line.shots.retain(|s| s.kind != ShotKind::Unknown);
                line.shots.truncate(4);
                for s in &mut line.shots {
                    s.items.retain(|i| !i.label.trim().is_empty());
                    s.items.truncate(5);
                    // Shots that need items but got none become a title card.
                    let needs_items = matches!(
                        s.kind,
                        ShotKind::List | ShotKind::Flow | ShotKind::Cycle | ShotKind::Compare | ShotKind::Hub
                            | ShotKind::Mnemonic | ShotKind::Timeline | ShotKind::Recap
                    );
                    if needs_items && s.items.len() < 2 {
                        s.kind = if s.icon.is_empty() { ShotKind::Title } else { ShotKind::Icon };
                        if s.text.is_empty() {
                            s.text = if s.title.is_empty() { s.label.clone() } else { s.title.clone() };
                        }
                    }
                    if s.kind == ShotKind::Compare {
                        s.items.truncate(2);
                    }
                }
            }
            if let Some(q) = &mut scene.quiz {
                if q.choices.len() < 2 || q.answer_index >= q.choices.len() {
                    scene.quiz = None;
                } else {
                    q.shuffle();
                }
            }
        }
        self.scenes.retain(|s| !s.lines.is_empty());
    }

    pub fn line_count(&self) -> usize {
        self.scenes.iter().map(|s| s.lines.len()).sum()
    }

    pub fn quizzes(&self) -> Vec<&Quiz> {
        self.scenes.iter().filter_map(|s| s.quiz.as_ref()).collect()
    }
}

#[cfg(test)]
mod shuffle_tests {
    use super::Quiz;

    #[test]
    fn shuffle_keeps_the_right_answer_and_spreads_it() {
        let mut seen = [0usize; 4];
        for _ in 0..400 {
            let mut q = Quiz {
                question: "q".into(),
                choices: vec!["a".into(), "right".into(), "c".into(), "d".into()],
                answer_index: 1,
                explanation: String::new(),
            };
            q.shuffle();
            assert_eq!(q.choices[q.answer_index], "right");
            seen[q.answer_index] += 1;
        }
        assert!(seen.iter().all(|&n| n > 50), "{seen:?}");
    }
}
