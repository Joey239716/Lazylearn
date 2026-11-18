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
   4 choices each, plausible wrong answers (the choices are shuffled afterwards). Make all 4 choices about the same length and level of detail: the right answer must never stand out by being the longest or most specific; wrong answers use real names, dates and terms from the material in the wrong place. answer_index is 0-based. explanation: 1-2 sentences with the named people, dates or examples that make the answer right.

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
    for q in &mut set.questions {
        q.shuffle();
    }
    set.cards.retain(|c| !c.front.trim().is_empty() && !c.back.trim().is_empty());
    if let Err(e) = balance(set.questions.iter_mut().collect(), provider, model) {
        eprintln!("warning: couldn't even out the answer lengths: {e:#}");
    }
    if set.questions.is_empty() && set.cards.is_empty() {
        anyhow::bail!("the study set came back empty");
    }
    Ok(set)
}

/// True when the right answer gives itself away by being clearly the longest choice.
pub fn telltale(q: &Quiz) -> bool {
    if q.choices.len() < 2 || q.answer_index >= q.choices.len() {
        return false;
    }
    let len = |s: &String| s.chars().count() as f32;
    let right = len(&q.choices[q.answer_index]);
    let wrong: Vec<f32> = q.choices.iter().enumerate().filter(|(i, _)| *i != q.answer_index).map(|(_, c)| len(c)).collect();
    let avg = wrong.iter().sum::<f32>() / wrong.len() as f32;
    let longest_wrong = wrong.iter().cloned().fold(0.0, f32::max);
    right > longest_wrong && right > avg * 1.15
}

/// AI writers make the right answer the longest, most careful choice, so a student can guess by
/// length. Rewrite the choices of every question where that shows so all are about the same
/// length and detail, then shuffle. Returns how many questions were rewritten.
pub fn balance(mut qs: Vec<&mut Quiz>, provider: Provider, model: Option<&str>) -> Result<usize> {
    let mut fixed = 0;
    for _round in 0..3 {
        let todo: Vec<usize> = (0..qs.len()).filter(|&i| telltale(qs[i])).collect();
        if todo.is_empty() {
            break;
        }
        for chunk in todo.chunks(25) {
            let items: Vec<Value> = chunk
                .iter()
                .map(|&i| {
                    let q = &qs[i];
                    json!({
                        "id": i,
                        "question": q.question,
                        "right": q.choices[q.answer_index],
                        "wrong": q.choices.iter().enumerate().filter(|(j, _)| *j != q.answer_index).map(|(_, c)| c).collect::<Vec<_>>(),
                    })
                })
                .collect();
            let prompt = format!(
                r#"These multiple-choice questions have a flaw: the right answer is clearly longer and more detailed than the wrong ones, so a student can guess it without knowing anything.

Rewrite the choices of each question so that ALL choices are about the same length (within a few words of each other) and equally specific:
- "right": the correct answer, same meaning. Shorten it if it is long (keep the key fact).
- "wrong": the same number of wrong answers, each as long and as detailed as the right one, plausible to someone who half-knows the material (use real names, dates and terms in the wrong place), but clearly wrong to someone who knows it.
- At least one wrong answer must be LONGER (in characters) than the right answer. Count them.
- Don't make the right answer the most precise or most hedged one. No "all/none of the above". No markdown.
Keep each "id". Respond with only the JSON.

QUESTIONS
{}"#,
                serde_json::to_string_pretty(&items)?
            );
            let schema = json!({
                "type": "object",
                "properties": { "items": { "type": "array", "items": { "type": "object",
                    "properties": {
                        "id": { "type": "integer" },
                        "right": { "type": "string" },
                        "wrong": { "type": "array", "items": { "type": "string" } }
                    },
                    "required": ["id", "right", "wrong"] } } },
                "required": ["items"]
            });
            let v = script::ask_json(provider, &prompt, &schema, model)?;
            for it in v.get("items").and_then(|x| x.as_array()).cloned().unwrap_or_default() {
                let Some(i) = it.get("id").and_then(|x| x.as_u64()).map(|x| x as usize) else { continue };
                if !chunk.contains(&i) {
                    continue;
                }
                let right = it.get("right").and_then(|x| x.as_str()).unwrap_or("").trim().to_string();
                let wrong: Vec<String> = it
                    .get("wrong")
                    .and_then(|x| x.as_array())
                    .map(|a| a.iter().filter_map(|w| w.as_str()).map(|w| w.trim().to_string()).filter(|w| !w.is_empty()).collect())
                    .unwrap_or_default();
                if right.is_empty() || wrong.is_empty() {
                    continue;
                }
                let q = &mut qs[i];
                q.choices = std::iter::once(right).chain(wrong).collect();
                q.answer_index = 0;
                q.shuffle();
                fixed += 1;
            }
        }
    }
    Ok(fixed)
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
