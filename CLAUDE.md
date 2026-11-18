# StickStudy: agent guide

Context for any AI agent (or human) picking up this project. Read this before you change anything.

## What this is

A Rust tool (CLI today, GPUI desktop app next — see "NEXT" below) that turns a student's class material (PDF slides, PowerPoint, lecture transcripts, notes) into a
fun, fully animated, voiced **study video (MP4)**: doodle characters, props, diagrams, kinetic captions, sound
effects and music. Quizzes are written alongside and are meant to be taken **after** the video.
It runs on **Windows and macOS**.

Pipeline: **extract text → Claude writes the script + shot list (JSON) → TTS voices each line → timeline →
frames rendered in parallel → ffmpeg MP4**.

The script is written by the user's local **Claude Code CLI** (`claude -p`), not the Anthropic API.
The user has no API key, so don't add an API-key dependency for script writing.

The pipeline was modeled on the user's hand-built EvenTheRain video project (`F:\EvenTheRain`, Python):
per-element pop-in animation, sound effects, ducked music, burned-in captions, chunked parallel rendering.

## Product requirements (from the user)

- **Output: MP4 only.** No live player. Clickable quizzes come **after** the video (planned: `stickstudy watch`
  plays the MP4, then opens a small quiz window built from `quiz.json`).
- **Animations and transitions are the top priority.** It's meant to be a fun alternative to studying. Many
  shots (more than one per 1–2 sentences is fine), every element animates in, every cut has a transition.
- **Content style:** TikTok / YouTube Shorts energy, hook first, but still **teach from scratch**
  (define every term, build step by step). Goals: understanding, exam prep, memorization.
- **Visuals:** Claude composes shots from a fixed vocabulary of shot kinds + a prop (icon) library.
  Several visual **styles** share the same shots and props.
- **Voice:** TTS only (no recording). Free local default per OS, paid options available.
- **Extras:** sound effects, music with ducking, burned-in captions + `.srt`, 9:16 vertical export.
- **Length** scales with the material. **Inputs:** PDF, .pptx, .vtt/.srt/.txt, .md.
- Keep art **original** (don't copy any channel's characters or branding).

## Repo layout

```
Cargo.toml      workspace: the engine + CLI (`stickstudy`) and the desktop app (`app/`, `lazylearn`)
assets/fonts/   LilitaOne (display) + PatrickHand (handwriting), OFL, embedded with include_bytes!
                geist/  Geist + Geist Mono (OFL): the app's UI fonts
src/
  lib.rs        The engine as a library (all modules) + `command()` (child processes without console windows)
  main.rs       CLI (clap): make / render / revoice / frames / voices / styles / prompt / course
  pipeline.rs   make / render_videos / revoice / plan_course / build_course as library calls; info.json, thumb.png
  progress.rs   Progress: stage events (start/info/frac/done) + cancel; the CLI prints, the app shows cards
  lesson.rs     Data model: Lesson, Scene, Line, Shot (+ShotKind), Item, Quiz, Pose + sanitize
  extract.rs    Text extraction: PDF (pdf-extract, falls back to pdftotext), PPTX (zip + regex), VTT/SRT cleanup
  script.rs     Prompt (cast + teaching style + SHOT_GUIDE) + JSON schema, shells out to `claude -p`
  tts.rs        Voice backends -> one WAV per line; fills Line.audio / audio_start / duration (silence trimmed)
  gfx.rs        tiny-skia canvas: transform/alpha stack, shapes, arrows, path trimming, vector text, easing
  styles.rs     Style enum (doodle/cosmic/whiteboard/chalk) -> Theme (palette, line weight, fonts) + backgrounds
  character.rs  Doodle cast rig: joint angles, poses, blending, blinking, lip-sync from the audio envelope
  icons.rs      Prop library (~90 line-art icons, several animated) + alias/keyword matching + letter-badge fallback
  shots.rs      Per shot kind: `plan` (element beat times + sfx cues) and `draw` (layout + entrances); intro/outro/chapter cards
  timeline.rs   Lays everything out in time (segments, transitions, captions, sfx, poses) + Renderer::frame(t)
  audio.rs      Synthesized sfx, generated lo-fi music, voice mixing, ducking, soft limiter
  render.rs     Parallel chunk rendering into ffmpeg, concat + audio, .srt, snapshots
app/                         Lazylearn desktop app (GPUI + gpui-kit), see "The Lazylearn desktop app"
samples/photosynthesis.txt   Small test lecture
lessons/                     Generated lessons (gitignored)
```

## Build and run

```bash
cargo build --release
./target/release/stickstudy make samples/photosynthesis.txt            # -> lessons/photosynthesis/video.mp4
./target/release/stickstudy render lessons/X --style cosmic --vertical  # re-render, no new script/voices
./target/release/stickstudy frames lessons/X                            # PNG of every shot -> lessons/X/frames
./target/release/stickstudy revoice lessons/X --voice openai
./target/release/stickstudy styles | voices | prompt
```

A lesson folder: `lesson.json`, `material.txt`, `quiz.json`, `audio/sXX_lYY.wav`, `video.mp4`, `video.srt`
(+ `video-vertical.mp4`). Useful render flags: `--until 20` (quick preview), `--no-music`, `--no-sfx`,
`--music file.mp3`, `--music-db -16`, `--fps`, `--workers`.

**Windows dev notes (this machine):** the C: drive is nearly full. Rust lives on F:
(`CARGO_HOME=F:\Tools\rust\cargo`, `RUSTUP_HOME=F:\Tools\rust\rustup`); set `TEMP`/`TMP` to a folder on F:
when building. ffmpeg must be on PATH or in `STICKSTUDY_FFMPEG`. `STICKSTUDY_CLAUDE` overrides the claude binary.

## How a video is made

1. **Script** (`script.rs`): one `claude -p --json-schema` call. The schema requires `shots` on every line.
   `SHOT_GUIDE` documents each shot kind and its fields, the pacing rule (new picture every 2–4 s of speech),
   on-screen text limits, and the icon list. Cast is `--cast duo` (teacher + student) or `solo` (narrator host).
1b. **Reading pass** (`vision.rs`, Gemini only, skip with `--no-vision`): PDFs (incl. scanned), images inside
   .pptx slides (from `ppt/media`, mapped to slides via the slide rels, tiny/EMF ones skipped) and image inputs
   are copied to `<lesson>/.vision/` and Gemini (`agy` with `--add-dir`, or the API with inline files) writes up
   every diagram/chart/table/equation and transcribes pages without selectable text. Saved as `visuals.txt` and
   appended to the material. With Gemini the material cap is 3M chars (whole textbooks); Claude keeps 150k.
2. **Voices** (`tts.rs`, `omnivoice.rs`): `auto` = `omnivoice` when `STICKSTUDY_OMNIVOICE_DIR` (or
   `<exe dir>/omnivoice`) holds `bin/tts-server-*` + `models/omnivoice-{base,tokenizer}-Q8_0.gguf`, else `sapi`
   on Windows (one batched PowerShell/System.Speech call), `say` on macOS, `espeak` on Linux. OmniVoice:
   Lazylearn spawns the native omnivoice.cpp `tts-server` (taken from VoiceStudio's installer, no Python) on
   a free port, designs each character's voice once from attribute tags (seeded), caches the clip in
   `<lesson>/voices/`, registers it via `POST /v1/audio/voices`, then voices lines with `POST /v1/audio/speech`
   (`{input, voice, language, response_format:"wav"}`). The user rejected the VoiceStudio desktop app as too
   heavy; use only the stripped engine. On this machine: RX 6650 XT via Vulkan, ~1× realtime. `speech_span` trims leading/trailing silence so pacing is tight.
3. **Timeline** (`timeline.rs`): intro card → per scene (chapter card for scenes 2+) → lines back to back
   (`GAP` 0.16 s) → outro "QUIZ TIME" card. Each line's shots start at their `cue` word (or split the line
   evenly); shots shorter than `MIN_SHOT` are dropped. Word timings are **estimated** from text length +
   punctuation pauses (no speech recognizer). Captions are 1–4 word chunks with the current word in gold.
4. **Beats** (`shots::plan`): for each shot, the time each element appears. Items are synced to the narration
   word that names them (`find_word`), so things pop in as they're said. The same beats drive the sound effects,
   so audio and visuals can't drift.
5. **Frames** (`Renderer::frame`): pure function of time (stateless), so chunks render in parallel threads,
   each piping RGBA into its own ffmpeg; then concat + mixed WAV. Camera push-in per shot, shake on stamps,
   transitions (whip left/up, zoom, pop-swap, circle wipe) between shots; Talk→Talk is a hard cut.
6. **Audio** (`audio.rs`): voice clips RMS-normalized, sfx synthesized in code, music = generated lo-fi loop
   (or `--music`), ducked under speech, soft limiter. No loudnorm (it pumped the music up between lines).

## Courses (course.rs, pipeline::plan_course / build_course)

The user's main use: paste a lot of material + a plain-language **direction** ("split by chapter; I only need
term definitions and how to answer the end-of-chapter questions, plus a solid understanding") and get several
videos. `course::plan` (Gemini via `gemini::ask_json`) returns a `Plan` of `PlannedVideo { title, recipe,
covers, start, end, focus, minutes }`; `start`/`end` are verbatim phrases from the material, matched
case/whitespace-insensitively by `course::slice` (fallback: whole material + "cover ONLY …" in the focus).
Saved as `<course>/plan.json` + `material.txt`; each video is a normal lesson folder `<course>/NN-slug/`.
`build_course` skips videos that already have `video.mp4` (resume) and runs as an **overlapping pipeline**:
three threads (scripts → voices → render) joined by bounded channels, so video N renders (CPU) while N+1
records voices (GPU) and N+2's script is written (network); a course takes about as long as its voices.
`Video i/N: title` is emitted when a video's script starts; each video's own events go through
`Progress::tagged("i/N")` (messages prefixed `[i/N] `, shared cancel). Single lessons use `produce`
(= `write_script` → `record_voices` → `render_videos`). Recipes (`script::Recipe`: overview,
terms, questions, cram) add a VIDEO FORMAT section to the script prompt; `make --recipe` uses them for one video.
**One video per chapter + practice (the user's preferred format, 2026-11-17):** recipe `chapter` teaches the
whole chapter in order, explains every key term as it comes up and covers everything the study questions ask
about; the key terms and study questions are practised AFTER the video as a **study set** (`study.rs`,
`PlannedVideo.study = true`, or `stickstudy study <lesson dir>`): `quiz.json` (multiple choice: one per key
term + 2-3 per study question) first, then `flashcards.json` (`{title, cards:[{front, back, kind: term|question}]}`;
question cards carry a model answer). Written in the script stage of the course pipeline. `script::ask_json`
sends any structured request to the configured provider.
The visual notes (`pipeline::VISUALS_HEADER` section) are whole-document, so they're appended to every slice.
App: the New-lesson composer's text box should be the course **direction**; show the plan for confirmation.

## Shot kinds (lesson.json)

`talk` (cast close-up + speech bubble), `title` (kinetic words), `icon` (hero prop), `list`, `flow`, `cycle`,
`compare`, `hub`, `definition`, `number` (counts up + confetti), `formula` (tokens drop in, arrows drawn),
`mnemonic` (letter tiles), `myth` (MYTH stamp → FACT), `timeline`, `question`, `recap` (ticked off).
Fields: `kind, cue, title, text, emphasis, icon, label, value, items[{label, icon, note}]`. Non-full-stage
shots get the presenter on the left; in duo lessons the student peeks in from the bottom-right when talking.

**To add a shot kind:** add it to `ShotKind` + `SHOT_NAMES` (lesson.rs), document it in `SHOT_GUIDE`,
add a `plan` arm and a draw function in `shots.rs`, then snapshot it. **To add a prop:** add the name to
`ICON_NAMES`, aliases in `canonical`, and a match arm in `draw_named` (200×200 box centered on 0,0).
**To add a style:** a `Style` variant + `Theme` + background in `styles.rs`.

All new lesson fields must be `#[serde(default)]` so older lesson.json files still load.

## Visual testing (important)

Render stills instead of watching the whole video:

```bash
./target/release/stickstudy frames lessons/test            # middle of every shot
./target/release/stickstudy frames lessons/test --at 3 12.5 -1 --style chalk --vertical
```

`lessons/test/lesson.json` is a hand-written lesson that uses **every** shot kind; revoice it with
`stickstudy revoice lessons/test` (free, no Claude call). For motion, extract frames from the MP4 with
`ffmpeg -ss <t> -t 0.5 -i video.mp4 -vf fps=16 out_%02d.png`. After any rendering change, look at every shot.

## Notes

- `claude -p` calls cost the user's Claude usage. Generate test lessons sparingly.
- Performance: ~3× realtime at 1080p30 on a 12-thread CPU (release build).

## Git workflow

- Remote: `https://github.com/Joey239716/Lazylearn.git`, branch `main`.
- Commit and push **directly to `main`** whenever a piece of work is done and verified. No PRs or branches needed.
- **No `Co-Authored-By` / Claude attribution lines** in commit messages.
- `target/` and `lessons/` are gitignored (except `lessons/test/lesson.json`).


## The Lazylearn desktop app (`app/`, GPUI)

`cargo build --release -p lazylearn` → `target/release/lazylearn.exe` (no console window in release). Built on
**gpui-kit 0.10** (pins `gpui-pre 0.3.12`; `extern crate gpui_kit as gpui` in main.rs). The first build takes
~10 minutes. The CLI stays separate (`cargo build --release -p stickstudy`), so it doesn't compile GPUI.

Look: Herdr / Claude desktop. Near-black, hairline borders, Geist type, white pill buttons, gold accent, colored
status dots. Custom title bar drawn inside the window (gpui-component `TitleBar`, transparent) with the Windows
controls on the right. The sidebar color runs under it, and the main pane is an inset panel with a rounded top-left
corner. Palette and helpers are in `app/src/theme.rs`. **Don't use a background darker than #0e0f11 behind small grey
text**: GPUI's text-contrast shader speckles glyphs on near-black. Use whole-pixel font sizes. Never call
`.hover()` twice on one element (GPUI panics), and never lock a job's `state` mutex twice in one expression
(deadlock).

- `app.rs`: the root view `LazyApp`. Pages: New (composer), Project (a lesson or a course), Settings. Sidebar
  lists projects newest first. A course's videos nest under it while it's open.
- Composer: attach (button or drop files anywhere), a text box (the focus, or the course **direction**), chips:
  Course/One video, style, cast, teacher voice, energy, 9:16, and the ↑ button (Ctrl+Enter).
  Course mode → `pipeline::plan_course` → the plan shows for review (change a video's recipe, remove videos) →
  "Make N videos" → `pipeline::build_course`. The app writes `.course` into the folder before planning so it shows
  up as a course right away.
- `jobs.rs`: each make / render / revoice / plan / course build runs on its own thread; `Progress` events fill a
  shared `JobState` (stage cards, fractions, current course video). The UI polls every 150 ms.
- `player.rs`: ffmpeg decodes BGRA frames (1280×720 / 720×1280) on a thread; the soundtrack is decoded to PCM
  once and played by rodio. Frames are shown against the audio clock (`Sink::get_pos`). Seeking restarts the decoder
  with `-ss`. Old frames are freed with `window.drop_image`. When the video ends, the quiz opens.
- Quiz: one question at a time, click or 1–4, green/red + explanation, score → `quiz_results.json` (best,
  attempts), shown in the sidebar.
- `library.rs`: scans the lessons folder (`info.json`, `thumb.png`, `quiz_results.json`, courses via
  `plan.json`). Old lessons get a thumbnail, length and `quiz.json` backfilled on startup.
- `settings.rs`: `%APPDATA%\Lazylearn\settings.json` (lessons folder, OmniVoice folder, ffmpeg, provider,
  defaults). OmniVoice/ffmpeg paths are pushed into `STICKSTUDY_OMNIVOICE_DIR` / `STICKSTUDY_FFMPEG`.
- Keys: Ctrl+N new lesson, Ctrl+B sidebar, Space/K play, ←/→ seek 5 s, M mute, F fullscreen, Esc, 1–4 answers.

**Checking screens without clicking** (debug builds): `LAZYLEARN_OPEN=<lesson dir>|settings`,
`LAZYLEARN_SEEK=<s>`, `LAZYLEARN_PLAY=1`, `LAZYLEARN_THEATER=1`, `LAZYLEARN_QUIZ=1` (+`LAZYLEARN_PICK=n`),
`LAZYLEARN_MENU=Style|Teacher|…`, `LAZYLEARN_ATTACH=a;b`, `LAZYLEARN_JOB=render|revoice`. Capture the window with
PrintWindow (it works even when the window is covered). Don't drive the user's real mouse: it lands on whatever
window is on top.

## Roadmap

1. ✅ Renderer, MP4 export, shot system, props, doodle style
2. ✅ Sound effects, music + ducking, captions
3. More styles polished (cosmic / whiteboard / chalk exist but need a visual pass), vertical 9:16 layouts
4. ~~`stickstudy watch` quiz window~~ → superseded by the GPUI app (built-in player + quiz) ✅
5. App next: macOS build check, volume slider, keyboard focus in the composer chips, editing plan titles
