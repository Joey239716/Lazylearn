//! Study sets: practice for after a video. Multiple choice first (`quiz.json`, which the app
//! already opens when a video ends), then flashcards (`flashcards.json`): every key term, and
//! every study question with a model answer.

use crate::lesson::Quiz;
use crate::script::{self, Provider};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Card {
    /// The term, or the study question
    pub front: String,
    /// The definition (who/what, when, where, why it matters), or a model answer
    pub back: String,
    /// "term" or "question"
    #[serde(default)]
    pub kind: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StudySet {
    pub title: String,
    pub questions: Vec<Quiz>,
    pub cards: Vec<Card>,
}

fn schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "questions": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "question": { "type": "string" },
                        "choices": { "type": "array", "items": { "type": "string" } },
                        "answer_index": { "type": "integer" },
                        "explanation": { "type": "string" }
                    },
                    "required": ["question", "choices", "answer_index", "explanation"]
                }
            },
            "cards": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "front": { "type": "string" },
                        "back": { "type": "string" },
                        "kind": { "type": "string", "enum": ["term", "question"] }
                    },
                    "required": ["front", "back", "kind"]
                }
            }
        },
        "required": ["questions", "cards"]
    })
}

/// Write a study set for `material` (a chapter plus its notes). `focus` says what to cover.
pub fn make(title: &str, material: &str, focus: &str, provider: Provider, model: Option<&str>) -> Result<StudySet> {
    let prompt = format!(
        r#"You are writing the practice set a student uses right after watching a study video. They are preparing for a written test with short answers (named people, dates, examples) and long answers.

WHAT THIS COVERS
{focus}

WRITE TWO THINGS

1. "questions": multiple-choice questions, in this order:
   - one for EVERY key term in the material's key-term list and "also worth knowing" list (if the material has such lists; otherwise the 15-25 most testable terms). Test who or what it is, when, where, or why it matters, not trivia.
   - then 2-3 for EACH study / review / end-of-chapter question, each testing a key fact, cause, comparison or example a strong answer needs.
   4 choices each, plausible wrong answers, vary which position is correct. answer_index is 0-based. explanation: 1-2 sentences with the named people, dates or examples that make the answer right.

2. "cards": flashcards, in this order:
   - one per key term (kind "term"): front = the term; back = 1-3 sentences: who or what, when, where, and why it matters (add the page number if the material gives one).
   - then one per study question (kind "question"): front = the question exactly as written; back = a model answer paragraph of 4-7 sentences packed with named people, dates and concrete examples from the material.

Use only facts from the material. No markdown. Respond with only the JSON.

MATERIAL
<<<
{material}
>>>"#
    );
    let v = script::ask_json(provider, &prompt, &schema(), model)?;
    let mut set: StudySet = serde_json::from_value(json!({
        "title": title,
        "questions": v.get("questions").cloned().unwrap_or(json!([])),
        "cards": v.get("cards").cloned().unwrap_or(json!([])),
    }))
    .context("the study set didn't match the expected format")?;
    set.questions.retain(|q| q.choices.len() >= 2 && q.answer_index < q.choices.len() && !q.question.trim().is_empty());
    set.cards.retain(|c| !c.front.trim().is_empty() && !c.back.trim().is_empty());
    if set.questions.is_empty() && set.cards.is_empty() {
        anyhow::bail!("the study set came back empty");
    }
    Ok(set)
}

impl StudySet {
    /// quiz.json (multiple choice, same format the app's after-video quiz reads) + flashcards.json
    pub fn save(&self, dir: &Path) -> Result<()> {
        std::fs::write(
            dir.join("quiz.json"),
            serde_json::to_string_pretty(&json!({ "title": self.title, "questions": self.questions }))?,
        )?;
        std::fs::write(
            dir.join("flashcards.json"),
            serde_json::to_string_pretty(&json!({ "title": self.title, "cards": self.cards }))?,
        )?;
        Ok(())
    }
}
