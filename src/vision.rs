//! Gemini "reading pass": before the script is written, Gemini looks at the
//! visual parts of the material (PDF pages including scans, images inside
//! PowerPoint slides, photos of notes) and writes them up as text: every
//! diagram, chart, table and equation, plus transcriptions of pages that have
//! no selectable text. The notes are appended to the material for the script writer.

use crate::gemini;
use anyhow::{Context, Result};
use regex::Regex;
use serde_json::{json, Value};
use std::io::Read;
use std::path::{Path, PathBuf};

const IMAGE_EXT: &[&str] = &["png", "jpg", "jpeg", "webp", "gif", "bmp", "heic"];
/// Images smaller than this inside slides are usually logos and bullets.
const MIN_SLIDE_IMAGE_BYTES: u64 = 12_000;

pub fn is_image(p: &Path) -> bool {
    p.extension().map(|e| IMAGE_EXT.contains(&e.to_string_lossy().to_lowercase().as_str())).unwrap_or(false)
}

fn ext(p: &Path) -> String {
    p.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default()
}

/// Copy everything Gemini should look at into `work`; returns (file name, what it is).
fn prepare(inputs: &[PathBuf], work: &Path) -> Result<Vec<(String, String)>> {
    if work.exists() {
        std::fs::remove_dir_all(work)?;
    }
    std::fs::create_dir_all(work)?;
    let mut files = vec![];
    for (i, p) in inputs.iter().enumerate() {
        let src_name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        match ext(p).as_str() {
            "pdf" => {
                let name = format!("doc{i:02}.pdf");
                std::fs::copy(p, work.join(&name))?;
                files.push((name, format!("the PDF \"{src_name}\" (refer to places as \"page N\")")));
            }
            "pptx" => {
                for (slide, k, bytes, e) in slide_images(p)? {
                    let name = format!("deck{i:02}_slide{slide:03}_img{k}.{e}");
                    std::fs::write(work.join(&name), bytes)?;
                    files.push((name, format!("an image on slide {slide} of \"{src_name}\"")));
                }
            }
            _ if is_image(p) => {
                let name = format!("image{i:02}.{}", ext(p));
                std::fs::copy(p, work.join(&name))?;
                files.push((name, format!("the image \"{src_name}\"")));
            }
            _ => {}
        }
    }
    Ok(files)
}

/// Pictures inside a .pptx with the slide they appear on: (slide, index, bytes, extension).
fn slide_images(path: &Path) -> Result<Vec<(usize, usize, Vec<u8>, String)>> {
    let f = std::fs::File::open(path)?;
    let mut zip = zip::ZipArchive::new(f).context("not a valid .pptx")?;
    let rel_re = Regex::new(r"^ppt/slides/_rels/slide(\d+)\.xml\.rels$").unwrap();
    let target_re = Regex::new(r#"Target="\.\./media/([^"]+)""#).unwrap();
    let mut by_slide: Vec<(usize, String)> = vec![];
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i)?;
        let name = entry.name().to_string();
        if let Some(c) = rel_re.captures(&name) {
            let slide: usize = c[1].parse().unwrap_or(0);
            let mut xml = String::new();
            entry.read_to_string(&mut xml)?;
            for t in target_re.captures_iter(&xml) {
                by_slide.push((slide, t[1].to_string()));
            }
        }
    }
    by_slide.sort();
    let mut out = vec![];
    let mut count: std::collections::HashMap<usize, usize> = Default::default();
    for (slide, media) in by_slide {
        let e = media.rsplit('.').next().unwrap_or("").to_lowercase();
        if !IMAGE_EXT.contains(&e.as_str()) {
            continue; // emf/wmf/svg/video: Gemini can't read them as images
        }
        let Ok(mut entry) = zip.by_name(&format!("ppt/media/{media}")) else { continue };
        if entry.size() < MIN_SLIDE_IMAGE_BYTES {
            continue;
        }
        let mut bytes = vec![];
        entry.read_to_end(&mut bytes)?;
        let k = count.entry(slide).or_insert(0);
        *k += 1;
        out.push((slide, *k, bytes, e));
    }
    Ok(out)
}

fn schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "visuals": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "source": { "type": "string" },
                        "where": { "type": "string" },
                        "kind": { "type": "string", "enum": ["diagram", "chart", "table", "equation", "photo", "figure", "transcribed text"] },
                        "content": { "type": "string" }
                    },
                    "required": ["source", "where", "kind", "content"]
                }
            }
        },
        "required": ["visuals"]
    })
}

const INSTRUCTIONS: &str = "\
You are helping turn class material into a study video. Open and look carefully at every file listed below \
(they are in your workspace folder). Do not modify, create or delete any files, and do not run any commands \
other than what you need to view the files.

For every diagram, chart, graph, table, equation, labelled figure or meaningful photo, write one entry:
- source: which file / document it is from (use the description given below)
- where: \"page N\" or \"slide N\"
- kind: diagram, chart, table, equation, photo or figure
- content: everything a student needs from it, in plain words: all labels and values, the relationships and \
arrows, what it shows and the key takeaway. Tables: give the rows. Equations: write them out in words and symbols.
Skip decorations, logos, stock photos and slide backgrounds.

If a page or image is a scan or photo of text (no selectable text, e.g. a scanned textbook page or a photo of \
handwritten notes), add an entry with kind \"transcribed text\" containing the full text of that page.

Be complete: textbooks can have many figures. Respond with only the JSON.";

/// Run the reading pass. Returns notes to append to the material ("" if there was nothing visual).
pub fn read_visuals(inputs: &[PathBuf], lesson_dir: &Path, model: Option<&str>, p: &crate::progress::Progress) -> Result<String> {
    let work = lesson_dir.join(".vision");
    let files = prepare(inputs, &work)?;
    if files.is_empty() {
        let _ = std::fs::remove_dir_all(&work);
        return Ok(String::new());
    }
    p.info(format!("Gemini is reading {} visual file(s): diagrams, charts, tables, scans…", files.len()));
    let listing: String = files.iter().map(|(n, what)| format!("- {n}: {what}\n")).collect();
    let prompt = format!("{INSTRUCTIONS}\n\nFILES\n{listing}");
    let v = if let Some(key) = std::env::var("GEMINI_API_KEY").ok().filter(|k| !k.trim().is_empty()) {
        api_read(&prompt, &files, &work, key.trim(), model, p)?
    } else {
        gemini::run_agy(&prompt, &schema(), model, Some(&work))?
    };
    let _ = std::fs::remove_dir_all(&work);
    let items = v.get("visuals").and_then(Value::as_array).cloned().unwrap_or_default();
    let mut out = String::new();
    for it in &items {
        let g = |k: &str| it.get(k).and_then(Value::as_str).unwrap_or("").trim().to_string();
        let content = g("content");
        if content.is_empty() {
            continue;
        }
        out.push_str(&format!("[{} — {}, {}]\n{}\n\n", g("source"), g("where"), g("kind"), content));
    }
    p.info(format!("{} visuals described", items.len()));
    Ok(out)
}

/// Same pass through the Gemini API (files sent inline).
fn api_read(prompt: &str, files: &[(String, String)], work: &Path, key: &str, model: Option<&str>, p: &crate::progress::Progress) -> Result<Value> {
    let mut parts = vec![json!({ "text": prompt })];
    let mut total = 0usize;
    for (name, _) in files {
        let bytes = std::fs::read(work.join(name))?;
        total += bytes.len();
        if total > 18_000_000 {
            p.info(format!("⚠️  material too large to send inline; skipping {name} and later files"));
            break;
        }
        let mime = match ext(Path::new(name)).as_str() {
            "pdf" => "application/pdf",
            "png" => "image/png",
            "webp" => "image/webp",
            "gif" => "image/gif",
            "heic" => "image/heic",
            _ => "image/jpeg",
        };
        parts.push(json!({ "text": format!("File {name}:") }));
        parts.push(json!({ "inline_data": { "mime_type": mime, "data": crate::omnivoice::b64(&bytes) } }));
    }
    let model = model.unwrap_or("gemini-2.5-flash");
    let url = format!("https://generativelanguage.googleapis.com/v1beta/models/{model}:generateContent?key={key}");
    let body = json!({
        "contents": [{ "parts": parts }],
        "generationConfig": { "response_mime_type": "application/json", "response_schema": schema() }
    });
    let res = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(600))
        .build()?
        .post(url)
        .json(&body)
        .send()
        .context("sending files to the Gemini API")?;
    if !res.status().is_success() {
        let st = res.status();
        anyhow::bail!("Gemini API error {st}: {}", res.text().unwrap_or_default());
    }
    let v: Value = res.json()?;
    let text = v.pointer("/candidates/0/content/parts/0/text").and_then(Value::as_str).unwrap_or("{}");
    Ok(serde_json::from_str(text).unwrap_or(json!({ "visuals": [] })))
}
