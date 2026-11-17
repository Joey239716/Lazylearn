//! Pull plain text out of class material: PDFs, PowerPoints, transcripts, notes.

use anyhow::{bail, Context, Result};
use regex::Regex;
use std::io::Read;
use std::path::Path;

/// `visual_pass`: images and scanned PDFs are allowed to have no text, because
/// Gemini's reading pass (vision.rs) will read them instead.
pub fn extract_all(paths: &[std::path::PathBuf], visual_pass: bool, prog: &crate::progress::Progress) -> Result<String> {
    let mut out = String::new();
    for p in paths {
        let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        prog.info(format!("📄 reading {name}"));
        if crate::vision::is_image(p) {
            if !visual_pass {
                bail!("{} is an image; images are read by Gemini (use --provider gemini)", p.display());
            }
            continue;
        }
        let text = extract(p).with_context(|| format!("couldn't read {}", p.display()))?;
        let text = tidy(&text);
        if text.trim().is_empty() && visual_pass {
            prog.info("  no selectable text — Gemini will read the pages instead");
            continue;
        }
        if text.trim().is_empty() {
            bail!("{} contains no readable text (scanned PDF or image-only slides?)", p.display());
        }
        out.push_str(&format!("=== Source: {name} ===\n{text}\n\n"));
    }
    Ok(out)
}

pub fn extract(path: &Path) -> Result<String> {
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "pdf" => pdf(path),
        "pptx" => pptx(path),
        "vtt" | "srt" => Ok(transcript(&std::fs::read_to_string(path)?)),
        "ppt" => bail!("old .ppt files aren't supported — open it in PowerPoint/Keynote and save as .pptx or PDF"),
        _ => Ok(std::fs::read_to_string(path)?),
    }
}

fn pdf(path: &Path) -> Result<String> {
    let p = path.to_path_buf();
    // pdf-extract can panic on unusual PDFs; contain it and fall back to pdftotext.
    let res = std::panic::catch_unwind(move || pdf_extract::extract_text(&p));
    match res {
        Ok(Ok(t)) if !t.trim().is_empty() => Ok(t),
        _ => {
            let out = crate::command("pdftotext")
                .arg("-layout")
                .arg(path)
                .arg("-")
                .output()
                .context("couldn't parse this PDF (install poppler for a fallback: `brew install poppler`)")?;
            Ok(String::from_utf8_lossy(&out.stdout).to_string())
        }
    }
}

fn pptx(path: &Path) -> Result<String> {
    let file = std::fs::File::open(path)?;
    let mut zip = zip::ZipArchive::new(file).context("not a valid .pptx file")?;
    let slide_re = Regex::new(r"^ppt/slides/slide(\d+)\.xml$").unwrap();
    let notes_re = Regex::new(r"^ppt/notesSlides/notesSlide(\d+)\.xml$").unwrap();

    let mut slides: Vec<(u32, String)> = Vec::new();
    let mut notes: Vec<(u32, String)> = Vec::new();
    for i in 0..zip.len() {
        let mut f = zip.by_index(i)?;
        let name = f.name().to_string();
        let target = if let Some(c) = slide_re.captures(&name) {
            Some((&mut slides, c[1].parse::<u32>().unwrap_or(0)))
        } else if let Some(c) = notes_re.captures(&name) {
            Some((&mut notes, c[1].parse::<u32>().unwrap_or(0)))
        } else {
            None
        };
        if let Some((list, n)) = target {
            let mut xml = String::new();
            f.read_to_string(&mut xml)?;
            list.push((n, xml_paragraphs(&xml)));
        }
    }
    slides.sort_by_key(|s| s.0);
    notes.sort_by_key(|s| s.0);

    let mut out = String::new();
    for (n, text) in &slides {
        out.push_str(&format!("--- Slide {n} ---\n{text}\n"));
        if let Some((_, note)) = notes.iter().find(|(m, _)| m == n) {
            let note = note.trim();
            // Notes slides repeat the slide number as a lone digit; skip trivial ones.
            if note.len() > 3 {
                out.push_str(&format!("Speaker notes: {note}\n"));
            }
        }
    }
    Ok(out)
}

/// Collect `<a:t>` runs, one output line per `<a:p>` paragraph.
fn xml_paragraphs(xml: &str) -> String {
    let run = Regex::new(r"<a:t>([^<]*)</a:t>").unwrap();
    xml.split("</a:p>")
        .map(|para| run.captures_iter(para).map(|c| unescape(&c[1])).collect::<String>())
        .filter(|s| !s.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

fn unescape(s: &str) -> String {
    s.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

/// Strip cue numbers, timestamps and tags from .srt / .vtt captions.
fn transcript(raw: &str) -> String {
    let tag = Regex::new(r"<[^>]+>").unwrap();
    let mut out: Vec<String> = Vec::new();
    for line in raw.lines() {
        let l = line.trim();
        if l.is_empty()
            || l == "WEBVTT"
            || l.contains("-->")
            || l.starts_with("NOTE")
            || l.starts_with("Kind:")
            || l.starts_with("Language:")
            || l.chars().all(|c| c.is_ascii_digit())
        {
            continue;
        }
        let clean = tag.replace_all(l, "").to_string();
        // Auto-captions often repeat the previous line; skip exact repeats.
        if out.last() != Some(&clean) {
            out.push(clean);
        }
    }
    out.join(" ")
}

fn tidy(s: &str) -> String {
    let blank = Regex::new(r"\n{3,}").unwrap();
    let spaces = Regex::new(r"[ \t]{2,}").unwrap();
    let s = spaces.replace_all(s, " ");
    blank.replace_all(&s, "\n\n").trim().to_string()
}
