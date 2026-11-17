//! Turn extracted class material into a lesson script (dialogue + shot list)
//! by asking Gemini (via Antigravity or API) or the local `claude` CLI.

use crate::icons::ICON_NAMES;
use crate::lesson::{Lesson, POSE_NAMES, SHOT_NAMES};
use anyhow::{anyhow, bail, Context, Result};
use clap::ValueEnum;
use serde_json::{json, Value};
use std::io::Write;
use std::process::{Command, Stdio};

const MAX_MATERIAL_CHARS: usize = 150_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, ValueEnum)]
pub enum CastMode {
    /// Teacher + student banter (two voices)
    #[default]
    Duo,
    /// One witty narrator who talks to you
    Solo,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, ValueEnum)]
pub enum Provider {
    /// Gemini (via Antigravity agy CLI or GEMINI_API_KEY)
    #[default]
    Gemini,
    /// Claude (via Claude Code claude CLI)
    Claude,
}

pub struct ScriptOptions {
    pub scenes: Option<u32>,
    pub focus: Option<String>,
    pub model: Option<String>,
    /// Replaces DEFAULT_STYLE (loaded from --teaching <file>).
    pub teaching: Option<String>,
    pub cast: CastMode,
    pub provider: Provider,
}

/// The teaching instructions. Override with `--teaching my_style.md`
/// (`stickstudy prompt` prints this so you can copy and edit it).
pub const DEFAULT_STYLE: &str = "\
- VIBE: TikTok / YouTube Shorts energy. Fun, fast, never boring, zero filler. Every line earns its place. This is a fun alternative to studying, so it should feel like a video people WANT to watch.
- HOOK HARD: the very first line of the lesson is a scroll-stopping hook (a surprising fact, a bold claim, a relatable problem, or a \"wait, what?\" question). Each scene also opens with a mini-hook so attention never dips.
- PUNCHY LINES: short sentences, conversational, like talking to a friend. Use reactions (\"No way.\", \"Okay that's actually wild.\", \"Hold up.\"), playful banter and the occasional meme-style joke, but the joke must always serve the concept.
- FROM SCRATCH: assume the viewer knows nothing about this topic. Define every term in plain words the first time it appears, before using it. Build strictly step by step: each scene relies only on what earlier scenes explained. If something needs background knowledge, teach that background first in one quick line.
- UNDERSTAND: intuition first (what problem does this solve, a vivid everyday analogy), then the precise definition or rule. Show how ideas connect.
- EXAM PREP: give key definitions in the exact precise form an exam would expect. Voice a common trap or misconception (have a character fall for it, or say \"most people think...\"), then fix it. For calculations or procedures, do a quick worked example step by step.
- MEMORIZE: catchy mnemonics, acronyms, rhymes or memory hooks for anything worth memorizing, and repeat the most important facts at least twice (when introduced and in the recap).
- Stay faithful to the material; never invent facts that contradict it. You may add widely-known context and examples.
- The final scene is a rapid-fire recap: the key takeaways and mnemonics in quick punchy lines.";

fn cast_prompt(cast: CastMode) -> &'static str {
    match cast {
        CastMode::Duo => "\
CAST (two doodle characters, two voices)
- teacher (name it \"Prof. Stix\" unless the material suggests a funnier fit): hyped, clear, loves a good analogy.
- student (\"Pip\" unless you pick a better fun name): curious and funny. Asks the questions a real beginner would, falls for common misconceptions so the teacher can fix them, and sometimes sums things up in their own words.
- Use speaker values \"teacher\" and \"student\" only. Mostly teacher lines, with the student jumping in every 2-4 lines.",
        CastMode::Solo => "\
CAST (one doodle host, one voice)
- A single narrator (speaker \"narrator\" for every line), drawn on screen as a little doodle host. Witty, warm, a bit dramatic: talks straight to the viewer as \"you\", uses vivid metaphors and the occasional dry joke.
- teacher_name: the host's fun name (e.g. \"Doodle\"). student_name: \"You\".",
    }
}

const SHOT_GUIDE: &str = "\
SHOTS (the visuals; this is an animated video, so visuals must change OFTEN)
Every line has a \"shots\" list. The renderer animates each shot in with its own entrance, transition and sound effects, so more shots = more energy.
- Change the picture roughly every 2-4 seconds of speech (about every 6-12 spoken words). Any line over 12 words MUST have at least 2 shots, and any line over 20 words 3 shots. Short reaction lines (under 8 words) can have an empty list (the previous shot stays on screen) or a quick talk shot.
- The first line of every scene must have at least one shot.
- For the 2nd and 3rd shot of a line, set \"cue\" to the word in that line where the new shot should appear.
- Never use the same kind twice in a row. Mix big moments (title, number, myth, compare) with diagrams (flow, cycle, hub, list) and character moments (talk, question).
- On-screen text is SHORT: item labels 1-3 words, titles max 7 words, notes max 6 words, other text max 14 words (definition text max 25). Never paste the whole spoken line on screen; captions already show it.
- Item labels should be words that are actually SAID in that line or the next one: each item pops in exactly when the narrator says it.
- \"icon\" fields: pick from this prop list when one fits: ICONS. Any other simple noun also works (it becomes a lettered badge), but prefer the list.

Shot kinds and the fields they use:
- talk: the cast up close reacting. \"text\" = optional short speech-bubble quip (max 8 words, e.g. \"Wait, WHAT?\"), \"icon\" = optional prop floating next to them. Use for banter, jokes, reactions, transitions.
- title: kinetic typography that slams in word by word. \"text\" = the punchline or hook (max 7 words), \"emphasis\" = the one word to highlight, optional \"icon\". Use for hooks, big claims, punchlines.
- icon: one huge animated prop. \"icon\", \"label\" (1-3 words), optional \"text\". Use when introducing a single thing.
- list: \"title\" + 2-4 \"items\" (label, icon, optional note). Parts, types, examples, steps without order.
- flow: 2-4 \"items\" joined by arrows, optional \"title\". A process, cause and effect, input to output.
- cycle: 3-5 \"items\" in a loop, optional \"title\" (shown in the middle). Cycles that repeat.
- compare: exactly 2 \"items\" (label, icon, note = the key difference in max 6 words). A vs B, before/after.
- hub: \"title\" (+ optional \"icon\") in the center with 2-5 \"items\" around it. A concept and its parts.
- definition: \"title\" = the term, \"text\" = the precise exam-ready definition, optional \"icon\". Use whenever a key term is defined.
- number: \"value\" = a striking figure as written on screen (\"70%\", \"3.8 billion\", \"1905\"), \"text\" = what it means, optional \"icon\". It counts up dramatically.
- formula: \"text\" = an equation or formula with spaces between tokens (\"6 CO2 + 6 H2O → C6H12O6 + 6 O2\", \"F = m × a\"). Symbols are fine here, it is only shown. Optional \"title\" and \"label\" (what it means).
- mnemonic: \"title\" = the acronym (\"PMAT\"), \"items\" = one word per letter, in order. Use for anything worth memorizing.
- myth: \"title\" = the misconception (as people say it), \"text\" = the truth. A MYTH stamp slams on the first, then the FACT appears. Use for exam traps.
- timeline: 2-5 \"items\" in order along a line, \"note\" = the date or step number. History, stages, phases.
- question: \"text\" = a question to make the viewer think (max 12 words), optional \"icon\".
- recap: \"title\" + 2-5 \"items\" that get ticked off. Use in the final recap scene.";

pub fn write_lesson(material: &str, opts: &ScriptOptions) -> Result<Lesson> {
    let prompt = build_prompt(material, opts);
    let schema_val = schema(opts.cast);
    let schema_str = schema_val.to_string();
    let mut last_err = None;
    for attempt in 1..=2 {
        let res = match opts.provider {
            Provider::Claude => ask_claude(&prompt, &schema_str, opts.model.as_deref()),
            Provider::Gemini => crate::gemini::ask_gemini(&prompt, &schema_val, opts.model.as_deref()),
        };
        match res {
            Ok(mut lesson) => {
                lesson.sanitize();
                if lesson.scenes.is_empty() {
                    last_err = Some(anyhow!("Model returned a lesson with no scenes"));
                } else {
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

pub fn build_prompt(material: &str, opts: &ScriptOptions) -> String {
    let mut material = material.to_string();
    // Gemini reads ~1M tokens at once, so whole textbooks fit; Claude gets a cap.
    let max = if opts.provider == Provider::Gemini { 3_000_000 } else { MAX_MATERIAL_CHARS };
    if material.len() > max {
        eprintln!("  ⚠️  material is {} characters; only the first {max} are used", material.len());
        let mut cut = max;
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
    let style = opts.teaching.clone().unwrap_or_else(|| DEFAULT_STYLE.to_string());
    let focus = opts
        .focus
        .as_ref()
        .map(|f| format!("\nThe learner specifically wants to focus on: {f}\n"))
        .unwrap_or_default();
    let shots = SHOT_GUIDE.replace("ICONS", &ICON_NAMES.join(", "));

    format!(
        r#"Write the script for a short animated study video that helps a university student learn the class material below from scratch and remember it. The video is fully animated with doodle characters, props, diagrams, captions, sound effects and music.

{cast}

TEACHING STYLE
{style}

{shots}

POSES (for every line: \"pose\" = the speaker's pose, \"listener_pose\" = the other character's)
{poses}. Use wave in the very first line, point when showing a diagram, excited/cheer for wow moments, think/confused for questions, shocked for surprising facts, facepalm for classic mistakes.

LENGTH
- Use {scenes}.{focus}
- Keep it tight: no repeated explanations except in the recap. Short material (a page or two) should come out around 2-3 minutes of speech; a full lecture around 6-10 minutes.

SCRIPT RULES
- Each scene: a short heading (max 5 words, shown on a chapter card) and 4-12 lines.
- Each line is spoken aloud by text-to-speech: max 28 words (most lines much shorter), punchy conversational sentences, no markdown, no emoji, no bullet symbols, no parentheses. Spell out symbols and formulas in words (say "x squared", not "x^2").
- Add a multiple-choice quiz to roughly every other scene, and always to the final recap scene. The quizzes are played together after the video, so they can recall anything from the lesson. 3-4 choices, answer_index is 0-based, explanation is 1-2 sentences. Quizzes should test understanding, not trivia. Vary which position holds the correct answer.

Do not call or use any tools. Respond with only the JSON lesson matching the schema. Leave out any field you don't use (no empty strings or empty lists).

CLASS MATERIAL
<<<
{material}
>>>"#,
        cast = cast_prompt(opts.cast),
        poses = POSE_NAMES.join(", "),
    )
}

pub fn schema(cast: CastMode) -> Value {
    let speakers: &[&str] = match cast {
        CastMode::Duo => &["teacher", "student"],
        CastMode::Solo => &["narrator"],
    };
    let pose = json!({ "type": "string", "enum": POSE_NAMES });
    let item = json!({
        "type": "object",
        "properties": {
            "label": { "type": "string" },
            "icon": { "type": "string" },
            "note": { "type": "string" }
        },
        "required": ["label"]
    });
    let shot = json!({
        "type": "object",
        "properties": {
            "kind": { "type": "string", "enum": SHOT_NAMES },
            "cue": { "type": "string" },
            "title": { "type": "string" },
            "text": { "type": "string" },
            "emphasis": { "type": "string" },
            "icon": { "type": "string" },
            "label": { "type": "string" },
            "value": { "type": "string" },
            "items": { "type": "array", "items": item }
        },
        "required": ["kind"]
    });
    let line = json!({
        "type": "object",
        "properties": {
            "speaker": { "type": "string", "enum": speakers },
            "text": { "type": "string" },
            "pose": pose,
            "listener_pose": pose,
            "shots": { "type": "array", "items": shot }
        },
        "required": ["speaker", "text", "pose", "listener_pose", "shots"]
    });
    json!({
        "type": "object",
        "properties": {
            "title": { "type": "string" },
            "teacher_name": { "type": "string" },
            "student_name": { "type": "string" },
            "scenes": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "heading": { "type": "string" },
                        "lines": { "type": "array", "items": line },
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
                    },
                    "required": ["heading", "lines"]
                }
            }
        },
        "required": ["title", "teacher_name", "student_name", "scenes"]
    })
}

fn claude_bin() -> String {
    std::env::var("STICKSTUDY_CLAUDE").unwrap_or_else(|_| "claude".into())
}

fn ask_claude(prompt: &str, schema: &str, model: Option<&str>) -> Result<Lesson> {
    let mut cmd = Command::new(claude_bin());
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
        "You are an award-winning educational scriptwriter and animation director. You output only valid JSON.",
    ]);
    // Sonnet writes great scripts and is much lighter on usage limits than the largest model.
    cmd.args(["--model", model.unwrap_or("sonnet")]);
    // Allow running from inside a Claude Code terminal session.
    cmd.env_remove("CLAUDECODE");
    cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());

    let mut child = cmd
        .spawn()
        .context("couldn't run `claude` — is Claude Code installed and on your PATH?")?;
    {
        let mut stdin = child.stdin.take().unwrap();
        stdin.write_all(prompt.as_bytes()).context("couldn't send material to claude")?;
    }
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
