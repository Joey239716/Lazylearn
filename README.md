# StickStudy 🎓

Turn your class material into a fun, fully animated study video: doodle characters, props that pop in
as they're mentioned, diagrams that draw themselves, kinetic captions, sound effects and music, plus a
quiz to take afterwards.

The script is written by your local **Claude Code** (`claude` command), so you don't need an API key.
It runs on **Windows and macOS**.

## Quick start

You need [Rust](https://rustup.rs), [ffmpeg](https://ffmpeg.org/download.html) on your PATH, and Claude Code.

```bash
cargo build --release
./target/release/stickstudy make lecture5.pdf
```

That reads the file, has Claude write a script and a shot list, records the voices, and renders
`lessons/lecture5/video.mp4` (plus `video.srt` subtitles and `quiz.json`).

Works with: **PDF**, **PowerPoint (.pptx)**, transcripts (**.vtt / .srt / .txt**) and Markdown notes.
Pass several files to combine them into one lesson.

## What's in a video

- **Intro and chapter cards**: each part of the lesson gets its own animated title card.
- **16 kinds of shots**, picked by Claude to suit the content: kinetic titles, definitions, flows,
  cycles, A vs B comparisons, counting-up numbers, equations, mnemonic letter tiles, MYTH → FACT
  busters, timelines, ticked-off recaps and more. The picture changes every few seconds.
- **Props that animate**: gears turn, hearts beat, DNA twists, electrons orbit. There are about 90 of them.
- **A doodle cast**: teacher and student (or one narrator) with poses, blinking and lip-sync.
- **Sync**: each item pops in at the moment the narrator says it, with its own sound effect.
- **Transitions** between every shot: whips, zooms, pop-swaps and circle wipes.
- **TikTok-style captions**, with the word being spoken highlighted.
- **Background music** that gets quieter under the voice: a built-in lo-fi loop, or your own track.


## Courses: split material into several videos

Got a whole textbook, a pile of readings, or a semester of slides? Describe what you need in plain words
and Lazylearn plans a series of videos, then makes them:

```bash
stickstudy course reader.pdf -d "Split it by chapter. I only need the term definitions and how to answer
the questions at the end of each chapter, plus a solid enough understanding of the material."
```

Gemini reads everything (including diagrams and scanned pages) and proposes a plan. Each video gets a part
of the material and a **format**:

| Format | What the video does |
|---|---|
| `overview` | understand the material: big picture, key ideas, examples, traps, recap |
| `terms` | drill every key term: exact definition, example, memory hook, definition quizzes |
| `questions` | how to answer the review / end-of-chapter questions: what each asks, key points, model answer, common mistake |
| `cram` | only what's most likely to be tested, fast |

You see the plan and confirm it before anything gets made. `--plan-only` saves it as `plan.json` so you can
edit it (titles, formats, focus, which part each video covers). Run `stickstudy course <course folder>` to make
it, or to continue after a stop: finished videos are skipped. `--only 2 5` makes just those videos. Each video
is a normal lesson folder inside the course folder. A single video can use a format too:
`stickstudy make notes.pdf --recipe terms`.

## Commands

| Command | What it does |
|---|---|
| `stickstudy make <files…>` | Make a lesson video from your material |
| `stickstudy course <files…> -d "…"` | Plan and make a series of videos (e.g. one per chapter) following your direction |
| `stickstudy render <lesson>` | Render the video again (try another `--style`, `--vertical`, other music) without rewriting or re-voicing |
| `stickstudy revoice <lesson> --voice openai` | Swap the voice engine, then render again |
| `stickstudy frames <lesson>` | Save a still of every shot as a PNG (a quick check of the look) |
| `stickstudy styles` | List the visual styles |
| `stickstudy voices` | List the voice engines and the voices installed on your computer |
| `stickstudy prompt` | Print the teaching instructions Claude follows |

### Useful options

- `--style doodle|cosmic|whiteboard|chalk`: the visual style
- `--vertical`: also render a 9:16 version for Shorts, TikTok and Reels (`--vertical-only` for just that one)
- `--cast duo|solo`: a teacher and student bantering, or one narrator
- `--focus "only the stuff on the midterm"`: steer what the lesson covers
- `--scenes 5`: force the length (by default it scales with how much material there is)
- `--model opus`: choose the Claude model that writes the script
- `--music song.mp3`, `--music-db -16`, `--no-music`, `--no-sfx`
- `--until 20`: render only the first 20 seconds (a quick preview)
- `--teaching my_style.md`: replace the teaching instructions with your own
  (`stickstudy prompt > my_style.md` gives you the default to start from)

## Voices

Pick one with `--voice <engine>`. Give each character a voice with `--teacher-voice` and `--student-voice`.

| Engine | Cost | Sound | Setup |
|---|---|---|---|
| `auto` (default) | free | — | `omnivoice` when it's set up, otherwise `sapi` on Windows and `say` on macOS |
| `omnivoice` | free | natural, **recommended** | local and offline; uses your GPU through Vulkan (AMD, NVIDIA or Intel) or the CPU. See below |
| `sapi` | free | OK, robotic | none (Windows built-in) |
| `say` | free | OK | none (macOS). Download "Premium" voices in System Settings → Accessibility → Spoken Content for better quality |
| `piper` | free | natural | `pip install piper-tts` plus a `.onnx` voice from huggingface.co/rhasspy/piper-voices |
| `openai` | ~1–3¢ per lesson | very natural, acted | `OPENAI_API_KEY=…` |
| `elevenlabs` | free tier / paid | most expressive | `ELEVENLABS_API_KEY=…` |
| `espeak` | free | retro robot | `espeak-ng` |
| `none` | free | silent | captions only |

### Setting up OmniVoice (recommended)

OmniVoice is an open voice model. Lazylearn runs its small native server (`tts-server`, from
[omnivoice.cpp](https://github.com/debpalash/VoiceStudio), the engine inside VoiceStudio). It needs no
Python and no app. Make a folder like this:

```
omnivoice/
  bin/     tts-server-<platform>[.exe] and its ggml libraries (in VoiceStudio's installer, under resources/bin)
  models/  omnivoice-base-Q8_0.gguf + omnivoice-tokenizer-Q8_0.gguf
           (from huggingface.co/Serveurperso/OmniVoice-GGUF, about 945 MB)
```

Point `STICKSTUDY_OMNIVOICE_DIR` at it, or put it next to the `stickstudy` executable. Voices are
**described** (`--teacher-voice "female, middle-aged, low pitch, british accent"`) or **cloned** from a
`.wav` with its transcript in a `.txt` next to it. Each character's voice is made once and saved in the
lesson's `voices/` folder, so it sounds the same on every line. The defaults are the gallery presets
**The Champion** (teacher) and **The Neighbor** (student); any preset works by name (`stickstudy voices`).

Liveliness: `--energy calm|upbeat|hype` (default `upbeat`) sets how excited the voices sound, and
`--pace 1.15` speeds every line up 15% without changing the pitch (works with any voice engine). It runs at about real-time speed on a
mid-range GPU.
