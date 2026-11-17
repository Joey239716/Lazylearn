# StickStudy 🎓

Turn your class material into fun, TikTok-paced animated lessons with voices,
on-screen notes and quizzes along the way. Pick from several video formats.

The lesson script is written by your local **Claude Code** (`claude` command), so
you don't need an API key.

## Quick start

```bash
cargo build --release
./target/release/stickstudy make lecture5.pdf
```

That reads the file, has Claude write the lesson, records voices, and opens the player.

Works with: **PDF**, **PowerPoint (.pptx)**, transcripts (**.vtt / .srt / .txt**),
and Markdown notes. Pass several files to combine them into one lesson.

## Video formats

Choose with `--format` (or `-f`). Each format has its own script style and animation.

| Format | Look | Script |
|---|---|---|
| `stickman` (default) | two stick figures and a chalkboard | teacher and student banter |
| `explainer` | cosmic flat-design backdrop, glowing concept shapes, a small mascot | one witty, wonder-filled narrator |
| `whiteboard` | marker notes and doodles that draw themselves | narrator sketching it out step by step |
| `chat` | phone screen with text bubbles and a sticky note of key points | two friends texting, one explaining to the other |

```bash
stickstudy make lecture.pdf --format explainer
stickstudy make slides.pptx -f chat
```

Every format follows the same shared teaching style: a hook up front, short punchy lines, teaching from
scratch, exam traps, mnemonics, and a recap at the end. To see exactly what Claude is told, run
`stickstudy style --format explainer`.

## Commands

| Command | What it does |
|---|---|
| `stickstudy make <files…>` | Create a lesson, then play it |
| `stickstudy play <lesson-folder>` | Watch a saved lesson again |
| `stickstudy revoice <lesson-folder> --voice openai` | Swap to a different voice engine without rewriting the script |
| `stickstudy formats` | List the video formats |
| `stickstudy voices` | List every voice engine and the voices installed on your Mac |
| `stickstudy style [-f format]` | Print the instructions Claude follows |

### Useful options for `make`

- `--focus "only the stuff on the midterm"`: steer what the lesson covers
- `--scenes 5`: force the length (the default scales with how much material there is)
- `--model opus`: choose the Claude model used to write the script
- `--style my_style.md`: replace the teaching-style instructions with your own
  (run `stickstudy style > my_style.md` to get the default as a starting point)
- `-o lessons/bio-week3`: choose where the lesson is saved

## Voices

Pick with `--voice <engine>`. Give each character a voice with `--teacher-voice` / `--student-voice`.

| Engine | Cost | Sound | Setup |
|---|---|---|---|
| `say` (default) | free | OK, robotic | none (macOS). Download "Premium" voices in System Settings → Accessibility → Spoken Content for better quality |
| `piper` | free | natural | `pip install piper-tts` plus a `.onnx` voice from huggingface.co/rhasspy/piper-voices |
| `openai` | ~1–3¢/lesson | very natural, acted | `export OPENAI_API_KEY=…` |
| `elevenlabs` | free tier/paid | most expressive | `export ELEVENLABS_API_KEY=…` |
| `espeak` | free | retro robot | `brew install espeak-ng` |
| `none` | free | silent | subtitles only |

Examples:

```bash
stickstudy make notes.pptx --voice say --teacher-voice "Rishi" --student-voice "Karen"
stickstudy revoice lessons/notes --voice openai --teacher-voice onyx --student-voice coral
```

## Player controls

| Key | Action |
|---|---|
| Space | pause / resume |
| ← / → | previous / next scene |
| R | replay scene |
| 1–4 or click | answer a quiz |
| Enter | continue after a quiz |
| click the progress bar | jump to a scene (green = quiz right, red = quiz wrong) |
| Q / Esc | quit |
