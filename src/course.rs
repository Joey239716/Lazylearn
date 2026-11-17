//! Courses: turn a pile of material plus a plain-language direction
//! ("split by chapter, I only need the terms and how to answer the end-of-chapter
//! questions") into a plan of several videos, each with its own slice of the
//! material, format (recipe) and focus. The plan is saved as plan.json so it can
//! be reviewed or edited, and building resumes where it left off.

use crate::gemini;
use crate::script::Recipe;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Plan {
    pub title: String,
    #[serde(default)]
    pub direction: String,
    #[serde(default)]
    pub notes: String,
    pub videos: Vec<PlannedVideo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlannedVideo {
    pub title: String,
    #[serde(default)]
    pub recipe: Recipe,
    /// Human description of what part of the material this covers ("Chapter 3: Markets")
    #[serde(default)]
    pub covers: String,
    /// Verbatim text where this video's part of the material starts ("" = from the beginning)
    #[serde(default)]
    pub start: String,
    /// Verbatim text where it ends ("" = to the end)
    #[serde(default)]
    pub end: String,
    /// What to focus on, in the planner's words
    #[serde(default)]
    pub focus: String,
    /// Target length in minutes of speech
    #[serde(default)]
    pub minutes: f32,
}

impl PlannedVideo {
    /// Folder name inside the course folder: "03-chapter-3-key-terms"
    pub fn folder(&self, i: usize) -> String {
        let slug: String = self
            .title
            .to_lowercase()
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
            .collect::<String>()
            .split('-')
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join("-");
        format!("{:02}-{}", i + 1, slug.chars().take(48).collect::<String>())
    }
}

impl Plan {
    pub fn load(dir: &Path) -> Result<Plan> {
        let p = dir.join("plan.json");
        serde_json::from_str(&std::fs::read_to_string(&p).with_context(|| format!("reading {}", p.display()))?)
            .with_context(|| format!("{} is not a valid course plan", p.display()))
    }
    pub fn save(&self, dir: &Path) -> Result<()> {
        std::fs::create_dir_all(dir)?;
        std::fs::write(dir.join("plan.json"), serde_json::to_string_pretty(self)?)?;
        Ok(())
    }
    pub fn print(&self) {
        eprintln!("\n📋 {} — {} videos", self.title, self.videos.len());
        if !self.notes.is_empty() {
            eprintln!("   {}", self.notes);
        }
        for (i, v) in self.videos.iter().enumerate() {
            eprintln!(
                "  {:>2}. {}  [{}, ~{:.0} min]\n      covers: {}\n      focus:  {}",
                i + 1,
                v.title,
                v.recipe.name(),
                v.minutes.max(1.0),
                v.covers,
                v.focus
            );
        }
        eprintln!();
    }
}

fn schema() -> Value {
    let video = json!({
        "type": "object",
        "properties": {
            "title": { "type": "string" },
            "recipe": { "type": "string", "enum": ["overview", "terms", "questions", "cram"] },
            "covers": { "type": "string" },
            "start": { "type": "string" },
            "end": { "type": "string" },
            "focus": { "type": "string" },
            "minutes": { "type": "number" }
        },
        "required": ["title", "recipe", "covers", "start", "end", "focus", "minutes"]
    });
    json!({
        "type": "object",
        "properties": {
            "title": { "type": "string" },
            "notes": { "type": "string" },
            "videos": { "type": "array", "items": video }
        },
        "required": ["title", "notes", "videos"]
    })
}

/// Ask Gemini to plan the videos.
pub fn plan(material: &str, direction: &str, model: Option<&str>) -> Result<Plan> {
    let direction = if direction.trim().is_empty() {
        "Split it into sensible parts (one per chapter or major topic) and give a solid understanding of each."
    } else {
        direction.trim()
    };
    let prompt = format!(
        r#"You are planning a series of short animated study videos for a university student. Read the class material below and the student's direction, then decide which videos to make.

STUDENT'S DIRECTION
{direction}

VIDEO FORMATS (pick one "recipe" per video)
- overview: understand the material (big picture, key ideas step by step, examples, traps, recap).
- terms: a drill of every key term with its exact definition, an example and a memory hook.
- questions: how to answer the review / discussion / end-of-chapter questions, with model answers.
- cram: only what is most likely to be tested, fast.

RULES
- Follow the direction exactly: if it says to split by chapter, make the split follow the chapters; if it says the student only needs certain things (for example terms and the end-of-chapter questions), make only videos that serve those needs. If it also asks for general understanding, include overview videos (or a short overview part).
- When one chapter needs several things (e.g. understanding + terms + questions), you may make several videos for it, or combine them into one video when the chapter is short. Prefer videos of 4-10 minutes of speech; never more than 12.
- Keep the videos in the order of the material.
- "start" and "end" mark each video's part of the material: copy a short, distinctive phrase (5-12 words) EXACTLY as it appears in the material, such as the chapter heading line. "start" is where the part begins, "end" is where the next part begins (so the text before it belongs to this video). Use "" for start to mean the beginning and "" for end to mean the end of the material. Videos that cover the same chapter use the same start and end.
- "covers": a short human description of the part ("Chapter 3: Supply and Demand").
- "focus": 1-3 sentences telling the scriptwriter exactly what this video must cover and emphasise, based on the direction.
- "title": a short catchy video title (max 8 words) that says which chapter or topic it is.
- "notes": one sentence explaining how you split the material.

Respond with only the JSON plan.

CLASS MATERIAL
<<<
{material}
>>>"#
    );
    let v = gemini::ask_json(&prompt, &schema(), model)?;
    let mut p: Plan = serde_json::from_value(v).context("Gemini's plan didn't match the plan format")?;
    p.direction = direction.to_string();
    p.videos.retain(|v| !v.title.trim().is_empty());
    if p.videos.is_empty() {
        anyhow::bail!("Gemini planned no videos");
    }
    Ok(p)
}

fn squash(s: &str) -> (String, Vec<usize>) {
    // lowercase, collapse whitespace; remember byte positions in the original
    let mut out = String::new();
    let mut map = vec![];
    let mut last_space = true;
    for (i, ch) in s.char_indices() {
        if ch.is_whitespace() {
            if !last_space {
                out.push(' ');
                map.push(i);
                last_space = true;
            }
        } else {
            for lc in ch.to_lowercase() {
                let mut buf = [0u8; 4];
                for _ in lc.encode_utf8(&mut buf).bytes() {
                    map.push(i);
                }
                out.push(lc);
            }
            last_space = false;
        }
    }
    map.push(s.len());
    (out, map)
}

fn find(hay: &str, map: &[usize], needle: &str, from: usize) -> Option<usize> {
    let (n, _) = squash(needle);
    let n = n.trim();
    if n.len() < 4 {
        return None;
    }
    hay.get(from..)?.find(n).map(|i| map[from + i])
}

/// This video's slice of the material, or None if its markers can't be found.
pub fn slice<'a>(material: &'a str, v: &PlannedVideo) -> Option<&'a str> {
    let (hay, map) = squash(material);
    let start = if v.start.trim().is_empty() { 0 } else { find(&hay, &map, &v.start, 0)? };
    // search for the end after the start (in squashed coordinates)
    let start_sq = map.iter().position(|&b| b >= start).unwrap_or(0);
    let end = if v.end.trim().is_empty() {
        material.len()
    } else {
        find(&hay, &map, &v.end, start_sq + 1).unwrap_or(material.len())
    };
    (end > start).then(|| &material[start..end])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn video(start: &str, end: &str) -> PlannedVideo {
        PlannedVideo {
            title: "t".into(),
            recipe: Recipe::Overview,
            covers: String::new(),
            start: start.into(),
            end: end.into(),
            focus: String::new(),
            minutes: 5.0,
        }
    }

    #[test]
    fn slices_between_markers_ignoring_case_and_spacing() {
        let m = "Intro text\nChapter 1:  The   Start\nalpha beta\nChapter 2: Next\ngamma";
        let s = slice(m, &video("chapter 1: the start", "Chapter 2: Next")).unwrap();
        assert!(s.starts_with("Chapter 1:"));
        assert!(s.contains("alpha beta"));
        assert!(!s.contains("gamma"));
        let last = slice(m, &video("Chapter 2: Next", "")).unwrap();
        assert!(last.contains("gamma") && !last.contains("alpha"));
        let first = slice(m, &video("", "Chapter 1: The Start")).unwrap();
        assert_eq!(first.trim(), "Intro text");
        assert!(slice(m, &video("not in the text at all", "")).is_none());
    }
}
