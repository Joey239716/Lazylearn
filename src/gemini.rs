//! Gemini backend for lesson script generation.
//! Can use either:
//! 1. Direct Gemini API if `GEMINI_API_KEY` is set.
//! 2. Antigravity CLI (`agy`) which is pre-authenticated on the system.

use crate::lesson::Lesson;
use anyhow::{anyhow, bail, Context, Result};
use serde_json::Value;
use std::io::Write;
use std::process::{Command, Stdio};

pub fn agy_bin() -> String {
    std::env::var("STICKSTUDY_AGY")
        .or_else(|_| std::env::var("STICKSTUDY_GEMINI"))
        .unwrap_or_else(|_| "agy".into())
}

/// Ask Gemini to write the lesson script using either GEMINI_API_KEY or the `agy` CLI.
pub fn ask_gemini(prompt: &str, schema: &Value, model: Option<&str>) -> Result<Lesson> {
    if let Ok(key) = std::env::var("GEMINI_API_KEY") {
        let key = key.trim();
        if !key.is_empty() {
            return ask_gemini_api(prompt, schema, key, model);
        }
    }
    ask_gemini_agy(prompt, schema, model)
}

/// Direct Google Gemini REST API structured generation.
pub fn ask_gemini_api(prompt: &str, schema: &Value, api_key: &str, model: Option<&str>) -> Result<Lesson> {
    let model = model.unwrap_or("gemini-2.5-flash");
    let url = format!(
        "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent?key={}",
        model, api_key
    );

    let body = serde_json::json!({
        "system_instruction": {
            "parts": [{ "text": "You are an award-winning educational scriptwriter and animation director. You output only valid JSON." }]
        },
        "contents": [
            { "parts": [{ "text": prompt }] }
        ],
        "generationConfig": {
            "response_mime_type": "application/json",
            "response_schema": schema
        }
    });

    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(180))
        .build()?;

    let res = client
        .post(&url)
        .json(&body)
        .send()
        .context("sending request to Gemini API")?;

    if !res.status().is_success() {
        let status = res.status();
        let err_text = res.text().unwrap_or_default();
        bail!("Gemini API error {}: {}", status, err_text);
    }

    let res_json: Value = res.json().context("parsing Gemini API response JSON")?;
    let text = res_json
        .pointer("/candidates/0/content/parts/0/text")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("no text returned in Gemini response: {}", res_json))?;

    serde_json::from_str(json_slice(text)).context("Gemini JSON didn't match the lesson format")
}

/// Structured generation via local Antigravity CLI (`agy`).
pub fn ask_gemini_agy(prompt: &str, schema: &Value, model: Option<&str>) -> Result<Lesson> {
    let v = run_agy(prompt, schema, model, None)?;
    serde_json::from_value(v).context("Gemini's JSON didn't match the lesson format")
}

/// Run one `agy` turn with a JSON schema and return the structured result.
/// `workdir` (if any) is added to the agent's workspace so it can open the files in it.
pub fn run_agy(prompt: &str, schema: &Value, model: Option<&str>, workdir: Option<&std::path::Path>) -> Result<Value> {
    let rand_id: u64 = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64;
    let temp_schema = std::env::temp_dir().join(format!("stickstudy_schema_{rand_id}.json"));
    std::fs::write(&temp_schema, serde_json::to_string(schema)?)?;

    let mut cmd = Command::new(agy_bin());
    cmd.args([
        "--json-schema",
        &temp_schema.to_string_lossy(),
        "--output-format",
        "json",
        "--dangerously-skip-permissions",
        "--disable-slash-commands",
    ]);
    if let Some(d) = workdir {
        cmd.arg("--add-dir").arg(d).current_dir(d);
    }
    if let Some(m) = model {
        cmd.args(["--model", m]);
    }
    cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());

    let mut child = cmd
        .spawn()
        .context("couldn't run `agy` — is Antigravity CLI installed and on your PATH?")?;
    {
        let mut stdin = child.stdin.take().unwrap();
        stdin.write_all(prompt.as_bytes()).context("couldn't send prompt to agy")?;
    }
    let out = child.wait_with_output()?;
    let _ = std::fs::remove_file(&temp_schema);

    let stdout = String::from_utf8_lossy(&out.stdout);
    if !out.status.success() {
        bail!(
            "agy exited with {}: {}{}",
            out.status,
            String::from_utf8_lossy(&out.stderr).trim(),
            stdout.trim()
        );
    }
    let v = parse_agy_output(&stdout)?;
    if v.get("status").and_then(Value::as_str) == Some("ERROR") {
        bail!("agy reported an error: {}", v.get("error").cloned().unwrap_or_default());
    }
    match v.get("structured_output") {
        Some(s) if s.is_object() => Ok(s.clone()),
        _ => {
            let resp = v.get("response").and_then(Value::as_str).unwrap_or("");
            serde_json::from_str(json_slice(resp))
                .with_context(|| format!("agy response didn't contain valid JSON: {}", preview(resp)))
        }
    }
}

fn parse_agy_output(stdout: &str) -> Result<Value> {
    if let Ok(v) = serde_json::from_str::<Value>(stdout.trim()) {
        return Ok(v);
    }
    for line in stdout.lines().rev() {
        let line = line.trim();
        if line.starts_with('{') && line.ends_with('}') {
            if let Ok(v) = serde_json::from_str::<Value>(line) {
                if v.get("status").is_some() || v.get("structured_output").is_some() {
                    return Ok(v);
                }
            }
        }
    }
    bail!("unexpected output from agy: {}", preview(stdout))
}

fn json_slice(s: &str) -> &str {
    match (s.find('{'), s.rfind('}')) {
        (Some(a), Some(b)) if b > a => &s[a..=b],
        _ => s,
    }
}

fn preview(s: &str) -> String {
    s.chars().take(300).collect()
}
