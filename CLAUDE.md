# StickStudy: agent guide

Context for any AI agent (or human) picking up this project. Read this before you change anything.

## What this is

A Rust desktop app that turns a student's class material (PDF slides, PowerPoint, lecture transcripts,
notes) into short, fun, voiced animated lessons with quizzes. It runs locally on macOS.

Pipeline: **extract text → Claude writes a lesson script (JSON) → TTS voices each line → interactive player**.

The lesson script is written by the user's local **Claude Code CLI** (`claude -p`), not the Anthropic API.
The user has no API key, so don't add an API-key dependency for script writing.

## Product requirements (from the user)

- **Content style:** TikTok / YouTube Shorts energy. Fun, concise, never boring, with a hook first. It must
  still **teach from scratch**: define every term before using it and build step by step.
- **Goals, all three:** understand concepts (intuition then precise definitions), exam prep (exact
  definitions, common traps, worked examples), and memorization (mnemonics, repetition, recap).
- **Length:** scales with the amount of material (not a fixed length).
- **Quizzes:** multiple choice, on roughly every other scene, plus the final recap scene.
- **Several video formats**, each with its own script instructions and visuals (see below). Keep adding
  formats in this pattern.
- **Several voice options**, from free and local up to paid and natural-sounding.
- **Input formats:** PDF, .pptx, and transcripts (.vtt/.srt/.txt). Markdown also works.
- The `explainer` format is *inspired by* high-production science-explainer channels (flat, colorful,
  cosmic). Keep its art and mascot **original**; don't copy any specific channel's characters, branding or look.

## Repo layout

```
Cargo.toml
src/
  main.rs       CLI (clap): make / play / revoice / voices / formats / style
  lesson.rs     Data model (Lesson, Scene, Line, Quiz, Visual, Pose, Shape, Layout) + load/save/sanitize
  extract.rs    Text extraction: PDF (pdf-extract, falls back to pdftotext), PPTX (zip + regex on <a:t>), VTT/SRT cleanup
  formats.rs    Format enum + per-format prompt text + capability flags (narrated / uses_poses / uses_visuals)
  script.rs     Builds the prompt + JSON schema per format, shells out to `claude -p`, parses and validates
  tts.rs        Voice backends -> one WAV per line; fills Line.audio + Line.duration
  player.rs     macroquad window: timing, audio (rodio), input, header/progress bar, subtitles, quiz, end screen,
                stickman stage; dispatches the other formats to looks.rs
  looks.rs      Renderers for the explainer, whiteboard and chat formats
  stickman.rs   Procedural stick figure rig (joint angles), poses, drawing, blinking, lip-flap
samples/photosynthesis.txt   Small test lecture
lessons/                     Generated lessons (gitignored)
```

## Build and run

Rust was installed with rustup (`~/.cargo/bin`). In a non-interactive shell, run `. "$HOME/.cargo/env"` first.

```bash
cargo build
./target/debug/stickstudy make samples/photosynthesis.txt -f explainer   # writes lessons/photosynthesis-explainer/
./target/debug/stickstudy play lessons/photosynthesis-explainer [--scene N] [--mute]
./target/debug/stickstudy revoice lessons/X --voice openai
./target/debug/stickstudy formats | voices | style [-f chat]
```

`make` options: `-f/--format`, `-o/--out`, `--scenes N`, `--focus "..."`, `--model opus|sonnet`,
`--style file.md` (replaces the shared teaching style), voice flags (`--voice`, `--teacher-voice`,
`--student-voice`, `--speed`), `--no-play`.

A lesson folder contains `lesson.json`, `material.txt` (the extracted source text) and `audio/sXX_lYY.wav`.

## Script generation (script.rs)

- Runs `claude -p --output-format json --tools "" --no-session-persistence --json-schema <schema> --system-prompt <...>`
  with the prompt on **stdin**. It removes the `CLAUDECODE` env var so it also works inside a Claude Code session.
- Reads `structured_output` from the JSON result if present; otherwise it extracts the outermost `{...}` from `result`.
  Retries once. Then `Lesson::sanitize()` drops malformed quizzes, empty lines and empty scenes.
- The prompt has three parts: the **format prompt** (`Format::prompt()`: cast, voice, how to use on-screen elements),
  the **shared teaching style** (`script::DEFAULT_STYLE`: the TikTok-style, from-scratch, exam and mnemonic rules),
  and **script rules** (line length ≤ 28 words, TTS-friendly text, quiz rules).
- The schema is built per format. Narrated formats only allow `speaker: "narrator"`. Stickman requires
  `pose`/`listener_pose`. Explainer and whiteboard require `visuals` + `layout`.
- Material is truncated at 150k characters.

## lesson.json model

```
Lesson { title, format, teacher_name, student_name, scenes[] }
Scene  { heading, board[] (key bullets), visuals[] {label, shape}, layout, lines[], quiz? }
Line   { speaker: teacher|student|narrator, text, pose, listener_pose, audio?, duration }
Quiz   { question, choices[], answer_index (0-based), explanation }
Pose   idle|explain|point|wave|think|excited|shrug|confused   (unknown -> Unknown, drawn as idle)
Shape  circle|square|triangle|hexagon|star
Layout flow|compare|cycle|hub   (hub: the first visual is the center)
```

All new fields must be `#[serde(default)]` so older lesson.json files still load.

## Formats

| Format | Speakers | Visuals |
|---|---|---|
| `stickman` | teacher + student | two rigged stick figures, chalkboard with progressively revealed bullets |
| `explainer` | narrator (mascot "Blip" reacts, never speaks) | starfield gradient, a planet whose color changes per scene, glowing shapes popping in along arrows, a key-point pill caption |
| `whiteboard` | narrator | marker notes typed out at marker speed on the left, sketchy doodles drawn stroke by stroke with arrows on the right |
| `chat` | teacher + student as two friends texting | phone UI, bubbles that pop in, typing dots during gaps, sticky note of key points |

**To add a format:**
1. Add a variant to `formats::Format`, plus `ALL`, `name`, `tagline`, `prompt`, and the capability flags.
2. Adjust `script::schema` if it needs new fields (and add them to `lesson.rs` with serde defaults).
3. Add a renderer in `looks.rs` and a match arm in `player_main`. Set `dark` for the header colors if the background is dark.
4. Generate a sample and visually verify it (see below).

## Voices (tts.rs)

Backends: `say` (macOS, default; teacher Daniel, student Samantha, rate 200), `piper` (needs a .onnx model),
`openai` (gpt-4o-mini-tts, `OPENAI_API_KEY`, per-character acting `instructions`, raw PCM 24 kHz → WAV),
`elevenlabs` (`ELEVENLABS_API_KEY`, pcm_22050 → WAV), `espeak` (`espeak-ng`), `none` (estimated durations).
The narrator uses the teacher voice. Every backend must produce a WAV whose duration `hound` can read, because
the player times everything off `Line.duration`.

## Player internals (player.rs)

- Everything is drawn in a fixed **1280×720 world**, letterboxed into the window.
- **Gotchas that already bit us:**
  - macroquad 0.4's `Camera2D::from_display_rect` is **y-up**. We negate `cam.zoom.y`, or everything renders upside down.
  - `screen_width()` is in logical points, but the GL **viewport needs physical pixels**. Multiply by `screen_dpi_scale()`,
    or the scene only fills a quarter of the window on Retina displays.
  - The mouse is mapped manually through the same letterbox transform (`pt` = points per world unit).
  - Text goes through `Ink`, which rasterizes at the real pixel scale (`px = pt * dpi`) so it stays sharp.
- Fonts are macOS system fonts loaded at runtime (Arial Rounded Bold for the UI, Chalkduster for chalk, Comic Sans
  MS Bold for handwriting), with a fallback to macroquad's default font if they're missing.
- Timing: each line lasts `duration.max(0.8) + GAP (0.22s)`. Board bullets, visuals and subtitles are revealed from
  `scene_progress()` / `scene_clock()`. Audio uses one rodio `Sink` per line. Pausing pauses both the sink and the clock.
- Phases: `Playing` → (`Quiz` if the scene has one and it hasn't been answered) → the next scene … → `Done`.
- Controls: Space pause, ←/→ scenes, R replay, 1–4 or click to answer, Enter to continue, click the progress bar
  to jump (green = right, red = wrong), Q/Esc quit.

## Visual testing (important)

The terminal lacks macOS Screen Recording permission, so `screencapture` only captures the wallpaper. Use the
built-in snapshot hook instead. It renders a frame to a PNG and exits:

```bash
STICKSTUDY_SNAPSHOT=/tmp/out.png STICKSTUDY_SNAPSHOT_AT=8 \
  ./target/debug/stickstudy play lessons/photosynthesis-whiteboard --mute --scene 3
# optional: STICKSTUDY_SNAPSHOT_PHASE=quiz | answered | done
sips -Z 1600 /tmp/out.png --out /tmp/out_s.png   # downscale before viewing
```

The PNG matches the on-screen orientation. **Don't flip it.** If a snapshot is upside down, the app is upside down.
After any rendering change, snapshot every format plus the quiz and done phases, and look at them.

## Environment notes

- macOS 14 on Apple Silicon. Homebrew here is "Tier 3" and builds big formulas (rust, llvm) from source, which
  takes hours. Don't use brew for toolchains.
- `pdftotext`, `piper` and `espeak-ng` are not installed by default; the code gives install hints when they're missing.
- `claude -p` calls cost the user's Claude usage. Generate test lessons sparingly (one sample per format is enough).

## Git workflow

- Remote: `https://github.com/Joey239716/Lazylearn.git`, branch `main`.
- Commit and push **directly to `main`** whenever a piece of work is done and verified. No PRs or branches needed.
- **No `Co-Authored-By` / Claude attribution lines** in commit messages.
- `target/` and `lessons/` are gitignored.

## Ideas / next steps

- Export a lesson to MP4 (render frames with the snapshot path + ffmpeg).
- More formats (game-show quiz host, comic-strip panels, podcast duo).
- Spaced-repetition review mode built from the quiz results.
- A drag-and-drop GUI launcher instead of the CLI.
- OCR fallback for scanned PDFs.
