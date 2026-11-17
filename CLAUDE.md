# StickStudy: agent guide

Context for any AI agent (or human) picking up this project. Read this before you change anything.

## What this is

A Rust CLI that turns a student's class material (PDF slides, PowerPoint, lecture transcripts, notes) into a
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
Cargo.toml
assets/fonts/   LilitaOne (display) + PatrickHand (handwriting), OFL, embedded with include_bytes!
src/
  main.rs       CLI (clap): make / render / revoice / frames / voices / styles / prompt
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
2. **Voices** (`tts.rs`): `auto` = `sapi` on Windows (one batched PowerShell/System.Speech call for all
   lines), `say` on macOS, `espeak` on Linux. `speech_span` trims leading/trailing silence so pacing is tight.
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

## Roadmap

1. ✅ Renderer, MP4 export, shot system, props, doodle style
2. ✅ Sound effects, music + ducking, captions
3. More styles polished (cosmic / whiteboard / chalk exist but need a visual pass), vertical 9:16 layouts
4. `stickstudy watch`: play the MP4, then a clickable quiz window from `quiz.json`
