//! Turn extracted class material into a lesson script by asking the local
//! `claude` CLI (Claude Code in print mode) — no API key needed.

use crate::formats::Format;
use crate::lesson::{Lesson, LAYOUT_NAMES, POSE_NAMES, SHAPE_NAMES};
use anyhow::{anyhow, bail, Context, Result};
use serde_json::{json, Value};
use std::io::Write;
use std::process::{Command, Stdio};

const MAX_MATERIAL_CHARS: usize = 150_000;

pub struct ScriptOptions {
    pub scenes: Option<u32>,
    pub focus: Option<String>,
    pub model: Option<String>,
    /// Replaces DEFAULT_STYLE (loaded from --style <file>).
    pub style: Option<String>,
    pub format: Format,
}

/// The teaching instructions. Override with `--style my_style.md`
/// (`stickstudy style` prints this so you can copy and edit it).
pub const DEFAULT_STYLE: &str = "\
- VIBE: TikTok / YouTube Shorts energy. Fun, fast, never boring, zero filler. Every line earns its place.
- HOOK HARD: the very first line of the lesson is a scroll-stopping hook (a surprising fact, a bold claim, a relatable problem, or a \"wait, what?\" question). Each scene also opens with a mini-hook so attention never dips.
- PUNCHY LINES: short sentences, conversational, like talking to a friend. Use reactions (\"No way.\", \"Okay that's actually wild.\", \"Hold up.\"), playful banter and the occasional meme-style joke, but the joke must always serve the concept.
- FROM SCRATCH: assume the viewer knows nothing about this topic. Define every term in plain words the first time it appears, before using it. Build strictly step by step: each scene relies only on what earlier scenes explained. If something needs background knowledge, teach that background first in one quick line.
- UNDERSTAND: intuition first (what problem does this solve, a vivid everyday analogy), then the precise definition or rule. Show how ideas connect.
- EXAM PREP: give key definitions in the exact precise form an exam would expect. Voice a common trap or misconception (have a character fall for it, or say \"most people think...\"), then fix it. For calculations or procedures, do a quick worked example step by step.
- MEMORIZE: catchy mnemonics, acronyms, rhymes or memory hooks for anything worth memorizing, and repeat the most important facts at least twice (when introduced and in the recap).
- Stay faithful to the material; never invent facts that contradict it. You may add widely-known context and examples.
- The final scene is a rapid-fire recap: the key takeaways and mnemonics in quick punchy lines.";

pub fn write_lesson(material: &str, opts: &ScriptOptions) -> Result<Lesson> {
    let prompt = build_prompt(material, opts);
    let schema = schema(opts.format).to_string();
    let mut last_err = None;
    for attempt in 1..=2 {
        match ask_claude(&prompt, &schema, opts.model.as_deref()) {
            Ok(mut lesson) => {
                lesson.sanitize();
                if lesson.scenes.is_empty() {
                    last_err = Some(anyhow!("Claude returned a lesson with no scenes"));
                } else {
                    lesson.format = opts.format;
                    return Ok(lesson);
                }
            }
            Err(e) => last_err = Some(e),
        }
        if attempt == 1 {
            eprintln!("  ⚠️  {:#} — retrying once…", last_err.as_ref().unwrap());
        }
    }
    Err(last_err.unwrap())
}

fn build_prompt(material: &str, opts: &ScriptOptions) -> String {
    let mut material = material.to_string();
    if material.len() > MAX_MATERIAL_CHARS {
        let mut cut = MAX_MATERIAL_CHARS;
        while !material.is_char_boundary(cut) {
            cut -= 1;
        }
        material.truncate(cut);
        material.push_str("\n[...material truncated...]");
    }
    let scenes = match opts.scenes {
        Some(n) => format!("exactly {n} scenes"),
        None => "a number of scenes that scales with the material: roughly one scene per distinct key idea, \
                 about 3-5 scenes for a handful of slides, 6-10 for a typical lecture, up to 15 for a whole \
                 chapter. Never pad thin material and never skip important ideas in dense material"
            .into(),
    };
    let style = opts.style.clone().unwrap_or_else(|| DEFAULT_STYLE.to_string());
    let focus = opts
        .focus
        .as_ref()
        .map(|f| format!("\nThe learner specifically wants to focus on: {f}\n"))
        .unwrap_or_default();

    format!(
        r#"Write the script for a short-form animated video lesson that helps a university student learn the class material below from scratch and remember it.

{format_prompt}

TEACHING STYLE (applies to every format)
{style}

LENGTH
- Use {scenes}.{focus}

SCRIPT RULES
- Each scene: a short heading (max 6 words), 2-5 board bullet points (max 10 words each), and 3-10 lines.
- Each line is spoken aloud by text-to-speech: max 28 words (most lines much shorter), punchy conversational sentences, no markdown, no emoji, no bullet symbols, no parentheses. Spell out symbols and formulas in words (say "x squared", not "x^2").
- Add a multiple-choice quiz to roughly every other scene, and always to the final recap scene. 3-4 choices, answer_index is 0-based, explanation is 1-2 sentences. Quizzes should test understanding, not trivia. Vary which position holds the correct answer.

Respond with only the JSON lesson.

CLASS MATERIAL
<<<
{material}
>>>"#,
        format_prompt = opts.format.prompt()
    )
}

fn schema(format: Format) -> Value {
    let speakers: &[&str] = if format.narrated() { &["narrator"] } else { &["teacher", "student"] };
    let mut line_props = json!({
        "speaker": { "type": "string", "enum": speakers },
        "text": { "type": "string" }
    });
    let mut line_req = vec!["speaker", "text"];
    if format.uses_poses() {
        let pose = json!({ "type": "string", "enum": POSE_NAMES });
        line_props["pose"] = pose.clone();
        line_props["listener_pose"] = pose;
        line_req.extend(["pose", "listener_pose"]);
    }
    let mut scene_props = json!({
        "heading": { "type": "string" },
        "board": { "type": "array", "items": { "type": "string" } },
        "lines": {
            "type": "array",
            "items": { "type": "object", "properties": line_props, "required": line_req }
        },
        "quiz": {
            "type": "object",
            "properties": {
                "question": { "type": "string" },
                "choices": { "type": "array", "items": { "type": "string" } },
                "answer_index": { "type": "integer" },
                "explanation": { "type": "string" }
            },
            "required": ["question", "choices", "answer_index", "explanation"]
        }
    });
    let mut scene_req = vec!["heading", "board", "lines"];
    if format.uses_visuals() {
        scene_props["visuals"] = json!({
            "type": "array",
            "items": {
                "type": "object",
                "properties": {
                    "label": { "type": "string" },
                    "shape": { "type": "string", "enum": SHAPE_NAMES }
                },
                "required": ["label", "shape"]
            }
        });
        scene_props["layout"] = json!({ "type": "string", "enum": LAYOUT_NAMES });
        scene_req.extend(["visuals", "layout"]);
    }
    json!({
        "type": "object",
        "properties": {
            "title": { "type": "string" },
            "teacher_name": { "type": "string" },
            "student_name": { "type": "string" },
            "scenes": {
                "type": "array",
                "items": { "type": "object", "properties": scene_props, "required": scene_req }
            }
        },
        "required": ["title", "teacher_name", "student_name", "scenes"]
    })
}

fn ask_claude(prompt: &str, schema: &str, model: Option<&str>) -> Result<Lesson> {
    let mut cmd = Command::new("claude");
    cmd.args([
        "-p",
        "--output-format",
        "json",
        "--tools",
        "",
        "--no-session-persistence",
        "--json-schema",
        schema,
        "--system-prompt",
        "You are an award-winning educational scriptwriter. You output only valid JSON.",
    ]);
    if let Some(m) = model {
        cmd.args(["--model", m]);
    }
    // Allow running from inside a Claude Code terminal session.
    cmd.env_remove("CLAUDECODE");
    cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());

    let mut child = cmd
        .spawn()
        .context("couldn't run `claude` — is Claude Code installed and on your PATH?")?;
    child
        .stdin
        .take()
        .unwrap()
        .write_all(prompt.as_bytes())
        .context("couldn't send material to claude")?;
    let out = child.wait_with_output()?;
    let stdout = String::from_utf8_lossy(&out.stdout);
    if !out.status.success() {
        bail!(
            "claude exited with {}: {}{}",
            out.status,
            String::from_utf8_lossy(&out.stderr).trim(),
            stdout.trim()
        );
    }

    let v: Value = serde_json::from_str(stdout.trim())
        .with_context(|| format!("unexpected output from claude: {}", preview(&stdout)))?;
    if v.get("is_error").and_then(Value::as_bool) == Some(true) {
        bail!("claude reported an error: {}", v.get("result").cloned().unwrap_or_default());
    }
    let lesson_val = match v.get("structured_output") {
        Some(s) if s.is_object() => s.clone(),
        _ => {
            let text = v
                .get("result")
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow!("no result in claude output"))?;
            serde_json::from_str(json_slice(text))
                .with_context(|| format!("claude didn't return valid JSON: {}", preview(text)))?
        }
    };
    serde_json::from_value(lesson_val).context("claude's JSON didn't match the lesson format")
}

/// Grab the outermost {...} in case the model wrapped JSON in prose or code fences.
fn json_slice(s: &str) -> &str {
    match (s.find('{'), s.rfind('}')) {
        (Some(a), Some(b)) if b > a => &s[a..=b],
        _ => s,
    }
}

fn preview(s: &str) -> String {
    s.chars().take(300).collect()
}
