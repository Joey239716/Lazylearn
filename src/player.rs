//! Interactive lesson player: animated stickmen, chalkboard, synced voice,
//! subtitles, quizzes and scene navigation.

use crate::formats::Format;
use crate::lesson::{Lesson, Pose, Speaker};
use crate::looks;
use crate::stickman::{Actor, Look, INK, PAPER};
use macroquad::prelude::*;
use std::io::Cursor;
use std::path::PathBuf;

const W: f32 = 1280.0;
const H: f32 = 720.0;
const GAP: f32 = 0.22; // snappy pause between lines
const FLOOR: f32 = 600.0;

pub(crate) const BOARD: Rect = Rect { x: 330.0, y: 96.0, w: 620.0, h: 330.0 };
pub(crate) const TEACH_COL: Color = Color::new(0.82, 0.2, 0.22, 1.0);
pub(crate) const STUD_COL: Color = Color::new(0.2, 0.45, 0.85, 1.0);
pub(crate) const CHALK: Color = Color::new(0.95, 0.95, 0.9, 1.0);
pub(crate) const GOOD: Color = Color::new(0.2, 0.62, 0.32, 1.0);
pub(crate) const BAD: Color = Color::new(0.82, 0.25, 0.22, 1.0);

pub fn run(lesson: Lesson, dir: PathBuf) {
    let conf = Conf {
        window_title: format!("StickStudy — {}", lesson.title),
        window_width: W as i32,
        window_height: H as i32,
        high_dpi: true,
        sample_count: 4,
        window_resizable: true,
        ..Default::default()
    };
    macroquad::Window::from_config(conf, async move {
        player_main(lesson, dir).await;
        std::process::exit(0);
    });
}

pub(crate) struct Fonts {
    ui: Option<Font>,
    chalk: Option<Font>,
    hand: Option<Font>,
}

fn load_font(path: &str) -> Option<Font> {
    let bytes = std::fs::read(path).ok()?;
    let f = load_ttf_font_from_bytes(&bytes).ok()?;
    Some(f)
}

/// Text drawing in 1280x720 world units, rasterized at the real pixel scale.
pub(crate) struct Ink<'a> {
    px: f32, // pixels per world unit
    fonts: &'a Fonts,
}

impl Ink<'_> {
    pub(crate) fn params<'f>(&self, font: Option<&'f Font>, size: f32, color: Color) -> TextParams<'f> {
        let fs = ((size * self.px).round() as u16).max(1);
        TextParams { font, font_size: fs, font_scale: size / fs as f32, color, ..Default::default() }
    }
    pub(crate) fn width(&self, font: Option<&Font>, s: &str, size: f32) -> f32 {
        let fs = ((size * self.px).round() as u16).max(1);
        measure_text(s, font, fs, size / fs as f32).width
    }
    pub(crate) fn draw(&self, font: Option<&Font>, s: &str, x: f32, y: f32, size: f32, color: Color) {
        draw_text_ex(s, x, y, self.params(font, size, color));
    }
    pub(crate) fn centered(&self, font: Option<&Font>, s: &str, cx: f32, y: f32, size: f32, color: Color) {
        let w = self.width(font, s, size);
        self.draw(font, s, cx - w / 2.0, y, size, color);
    }
    pub(crate) fn wrap(&self, font: Option<&Font>, text: &str, max_w: f32, size: f32) -> Vec<String> {
        let mut lines = Vec::new();
        let mut cur = String::new();
        for word in text.split_whitespace() {
            let trial = if cur.is_empty() { word.to_string() } else { format!("{cur} {word}") };
            if self.width(font, &trial, size) > max_w && !cur.is_empty() {
                lines.push(std::mem::take(&mut cur));
                cur = word.to_string();
            } else {
                cur = trial;
            }
        }
        if !cur.is_empty() {
            lines.push(cur);
        }
        lines
    }
    pub(crate) fn ui(&self) -> Option<&Font> {
        self.fonts.ui.as_ref()
    }
    pub(crate) fn chalk(&self) -> Option<&Font> {
        self.fonts.chalk.as_ref().or(self.fonts.ui.as_ref())
    }
    pub(crate) fn hand(&self) -> Option<&Font> {
        self.fonts.hand.as_ref().or(self.fonts.ui.as_ref())
    }
}

pub(crate) enum Phase {
    Playing,
    Quiz { picked: Option<usize> },
    Done,
}

struct Audio {
    _stream: rodio::OutputStream,
    handle: rodio::OutputStreamHandle,
    sink: Option<rodio::Sink>,
}

pub(crate) struct Player {
    pub(crate) lesson: Lesson,
    clips: Vec<Vec<Option<std::sync::Arc<[u8]>>>>,
    audio: Option<Audio>,
    pub(crate) scene: usize,
    pub(crate) line: usize,
    pub(crate) t: f32,
    paused: bool,
    pub(crate) phase: Phase,
    results: Vec<Option<bool>>,
    pub(crate) teacher: Actor,
    student: Actor,
}

impl Player {
    fn new(lesson: Lesson, dir: &PathBuf) -> Player {
        let clips = lesson
            .scenes
            .iter()
            .map(|s| {
                s.lines
                    .iter()
                    .map(|l| {
                        l.audio
                            .as_ref()
                            .and_then(|a| std::fs::read(dir.join(a)).ok())
                            .map(|b| b.into())
                    })
                    .collect()
            })
            .collect();
        let audio = rodio::OutputStream::try_default()
            .ok()
            .map(|(s, h)| Audio { _stream: s, handle: h, sink: None });
        if audio.is_none() {
            eprintln!("⚠️  no audio output device found — playing silently");
        }
        let n = lesson.scenes.len();
        let mut p = Player {
            lesson,
            clips,
            audio,
            scene: 0,
            line: 0,
            t: 0.0,
            paused: false,
            phase: Phase::Playing,
            results: vec![None; n],
            teacher: Actor::new(180.0, FLOOR, 1.0, Look::Teacher),
            student: Actor::new(1100.0, FLOOR, -1.0, Look::Student),
        };
        p.start_line();
        p
    }

    pub(crate) fn line_len(&self, si: usize, li: usize) -> f32 {
        self.lesson.scenes[si].lines[li].duration.max(0.8) + GAP
    }

    fn stop_audio(&mut self) {
        if let Some(a) = &mut self.audio {
            if let Some(s) = a.sink.take() {
                s.stop();
            }
        }
    }

    fn start_line(&mut self) {
        self.t = 0.0;
        self.stop_audio();
        let line = &self.lesson.scenes[self.scene].lines[self.line];
        match line.speaker {
            Speaker::Teacher => {
                self.teacher.pose = line.pose;
                self.student.pose = line.listener_pose;
            }
            Speaker::Student => {
                self.student.pose = line.pose;
                self.teacher.pose = line.listener_pose;
            }
            Speaker::Narrator => {}
        }
        if let (Some(a), Some(bytes)) = (&mut self.audio, &self.clips[self.scene][self.line]) {
            if let Ok(sink) = rodio::Sink::try_new(&a.handle) {
                if let Ok(src) = rodio::Decoder::new(Cursor::new(bytes.clone())) {
                    sink.append(src);
                    if self.paused {
                        sink.pause();
                    }
                    a.sink = Some(sink);
                }
            }
        }
    }

    fn goto(&mut self, scene: usize) {
        self.scene = scene.min(self.lesson.scenes.len() - 1);
        self.line = 0;
        self.paused = false;
        self.phase = Phase::Playing;
        self.start_line();
    }

    fn next_scene(&mut self) {
        if self.scene + 1 < self.lesson.scenes.len() {
            self.goto(self.scene + 1);
        } else {
            self.stop_audio();
            self.phase = Phase::Done;
            self.teacher.pose = Pose::Excited;
            self.student.pose = Pose::Excited;
        }
    }

    fn end_of_scene(&mut self) {
        self.stop_audio();
        if self.lesson.scenes[self.scene].quiz.is_some() && self.results[self.scene].is_none() {
            self.phase = Phase::Quiz { picked: None };
            self.teacher.pose = Pose::Point;
            self.student.pose = Pose::Think;
        } else {
            self.next_scene();
        }
    }

    fn set_paused(&mut self, p: bool) {
        self.paused = p;
        if let Some(Audio { sink: Some(s), .. }) = &self.audio {
            if p { s.pause() } else { s.play() }
        }
    }

    fn answer(&mut self, i: usize) {
        if let Phase::Quiz { picked: None } = self.phase {
            let correct = self.lesson.scenes[self.scene].quiz.as_ref().unwrap().answer_index == i;
            self.results[self.scene] = Some(correct);
            self.phase = Phase::Quiz { picked: Some(i) };
            self.teacher.pose = if correct { Pose::Excited } else { Pose::Explain };
            self.student.pose = if correct { Pose::Excited } else { Pose::Shrug };
        }
    }

    fn update(&mut self, dt: f32, time: f32) {
        if matches!(self.phase, Phase::Playing) && !self.paused {
            self.t += dt;
            if self.t >= self.line_len(self.scene, self.line) {
                if self.line + 1 < self.lesson.scenes[self.scene].lines.len() {
                    self.line += 1;
                    self.start_line();
                } else {
                    self.end_of_scene();
                }
            }
        }
        let speaking = matches!(self.phase, Phase::Playing)
            && !self.paused
            && self.t < self.lesson.scenes[self.scene].lines[self.line].duration;
        let who = self.lesson.scenes[self.scene].lines[self.line].speaker;
        self.teacher.talking = speaking && who != Speaker::Student;
        self.student.talking = speaking && who == Speaker::Student;
        self.teacher.update(dt, time);
        self.student.update(dt, time);
    }

    /// Fraction of the current scene that has played (for board reveal & progress bar).
    pub(crate) fn scene_progress(&self) -> f32 {
        if !matches!(self.phase, Phase::Playing) {
            return 1.0;
        }
        let n = self.lesson.scenes[self.scene].lines.len();
        let total: f32 = (0..n).map(|i| self.line_len(self.scene, i)).sum();
        let done: f32 = (0..self.line).map(|i| self.line_len(self.scene, i)).sum::<f32>() + self.t;
        (done / total).clamp(0.0, 1.0)
    }
}

async fn player_main(lesson: Lesson, dir: PathBuf) {
    let fonts = Fonts {
        ui: load_font("/System/Library/Fonts/Supplemental/Arial Rounded Bold.ttf"),
        chalk: load_font("/System/Library/Fonts/Supplemental/Chalkduster.ttf"),
        hand: load_font("/System/Library/Fonts/Supplemental/Comic Sans MS Bold.ttf"),
    };
    let mut p = Player::new(lesson, &dir);

    loop {
        let dt = get_frame_time().min(0.05);
        let time = get_time() as f32;

        // letterboxed 16:9 camera
        let (sw, sh) = (screen_width(), screen_height());
        let px = (sw / W).min(sh / H);
        let (vw, vh) = (W * px, H * px);
        let cam = Camera2D {
            viewport: Some((((sw - vw) / 2.0) as i32, ((sh - vh) / 2.0) as i32, vw as i32, vh as i32)),
            ..Camera2D::from_display_rect(Rect::new(0.0, 0.0, W, H))
        };
        clear_background(BLACK);
        set_camera(&cam);
        let mouse = cam.screen_to_world(mouse_position().into());
        let clicked = is_mouse_button_pressed(MouseButton::Left);
        let ink = Ink { px, fonts: &fonts };

        // ---------- input ----------
        if is_key_pressed(KeyCode::Q) || is_key_pressed(KeyCode::Escape) {
            p.stop_audio();
            break;
        }
        match p.phase {
            Phase::Playing => {
                if is_key_pressed(KeyCode::Space) {
                    let np = !p.paused;
                    p.set_paused(np);
                }
                if is_key_pressed(KeyCode::Right) {
                    p.end_of_scene();
                }
                if is_key_pressed(KeyCode::Left) {
                    let s = if p.line == 0 && p.t < 1.5 { p.scene.saturating_sub(1) } else { p.scene };
                    p.goto(s);
                }
                if is_key_pressed(KeyCode::R) {
                    p.goto(p.scene);
                }
            }
            Phase::Quiz { picked } => {
                let keys = [KeyCode::Key1, KeyCode::Key2, KeyCode::Key3, KeyCode::Key4];
                for (i, k) in keys.iter().enumerate() {
                    let n = p.lesson.scenes[p.scene].quiz.as_ref().unwrap().choices.len();
                    if is_key_pressed(*k) && i < n {
                        p.answer(i);
                    }
                }
                if picked.is_some()
                    && (is_key_pressed(KeyCode::Enter) || is_key_pressed(KeyCode::Space) || is_key_pressed(KeyCode::Right))
                {
                    p.next_scene();
                }
                if is_key_pressed(KeyCode::R) {
                    p.goto(p.scene);
                }
            }
            Phase::Done => {
                if is_key_pressed(KeyCode::R) {
                    p.results.iter_mut().for_each(|r| *r = None);
                    p.goto(0);
                }
            }
        }

        p.update(dt, time);

        // ---------- draw ----------
        let format = p.lesson.format;
        match format {
            Format::Stickman => {
                draw_rectangle(0.0, 0.0, W, H, PAPER);
                draw_line(0.0, FLOOR + 3.0, W, FLOOR + 3.0, 3.0, Color::new(0.8, 0.77, 0.7, 1.0));
                if !matches!(p.phase, Phase::Done) {
                    draw_board(&p, &ink);
                }
                p.teacher.draw(time);
                p.student.draw(time);
                name_tag(&ink, &p.lesson.teacher_name, p.teacher.x, TEACH_COL);
                name_tag(&ink, &p.lesson.student_name, p.student.x, STUD_COL);
            }
            Format::Explainer => looks::explainer(&p, &ink, time),
            Format::Whiteboard => looks::whiteboard(&p, &ink, time),
            Format::Chat => looks::chat(&p, &ink, time),
        }
        let dark = format == Format::Explainer;
        draw_header(&mut p, &ink, mouse, clicked, dark);

        match p.phase {
            Phase::Playing if format != Format::Chat => draw_subtitle(&p, &ink),
            Phase::Playing => {}
            Phase::Quiz { picked } => {
                if let Some(i) = draw_quiz(&p, &ink, picked, mouse, clicked) {
                    if picked.is_none() {
                        p.answer(i);
                    } else {
                        p.next_scene();
                    }
                }
            }
            Phase::Done => draw_done(&p, &ink),
        }

        if p.paused {
            draw_rectangle(0.0, 0.0, W, H, Color::new(0.0, 0.0, 0.0, 0.35));
            ink.centered(ink.ui(), "Paused", W / 2.0, H / 2.0, 56.0, WHITE);
            ink.centered(ink.ui(), "press Space to resume", W / 2.0, H / 2.0 + 44.0, 22.0, WHITE);
        }

        next_frame().await;
    }
}

fn name_tag(ink: &Ink, name: &str, x: f32, col: Color) {
    ink.centered(ink.ui(), name, x, FLOOR + 30.0, 18.0, col);
}

fn draw_header(p: &mut Player, ink: &Ink, mouse: Vec2, clicked: bool, dark: bool) {
    let fg = if dark { WHITE } else { INK };
    ink.draw(ink.ui(), &p.lesson.title, 24.0, 38.0, 26.0, fg);
    let n = p.lesson.scenes.len();
    let info = match p.phase {
        Phase::Done => "Complete!".to_string(),
        _ => format!("Scene {}/{}", p.scene + 1, n),
    };
    let w = ink.width(ink.ui(), &info, 18.0);
    ink.draw(ink.ui(), &info, W - 24.0 - w, 36.0, 18.0, Color::new(0.4, 0.4, 0.45, 1.0));

    // clickable segmented progress bar
    let (x0, y, total_w, gap) = (24.0, 54.0, W - 48.0, 6.0);
    let seg_w = (total_w - gap * (n as f32 - 1.0)) / n as f32;
    let mut jump = None;
    for i in 0..n {
        let x = x0 + i as f32 * (seg_w + gap);
        let hover = Rect::new(x, y - 8.0, seg_w, 24.0).contains(mouse);
        let base = match (hover, dark) {
            (true, _) => Color::new(0.75, 0.75, 0.8, 1.0),
            (false, false) => Color::new(0.88, 0.87, 0.84, 1.0),
            (false, true) => Color::new(1.0, 1.0, 1.0, 0.18),
        };
        draw_rectangle(x, y, seg_w, 8.0, base);
        let fill = if matches!(p.phase, Phase::Done) || i < p.scene {
            1.0
        } else if i == p.scene {
            p.scene_progress()
        } else {
            0.0
        };
        let col = match p.results[i] {
            Some(true) => GOOD,
            Some(false) => BAD,
            None if dark => Color::new(1.0, 0.82, 0.35, 1.0),
            None => Color::new(0.35, 0.35, 0.4, 1.0),
        };
        draw_rectangle(x, y, seg_w * fill, 8.0, col);
        if hover && clicked {
            jump = Some(i);
        }
    }
    if let Some(i) = jump {
        p.goto(i);
    }
    let help = "Space pause   <- -> scenes   R replay   Q quit   (click the bar to jump)";
    ink.draw(ink.ui(), help, 24.0, 84.0, 13.0, Color::new(0.55, 0.55, 0.58, 1.0));
}

fn draw_board(p: &Player, ink: &Ink) {
    let b = BOARD;
    draw_rectangle(b.x - 12.0, b.y - 12.0, b.w + 24.0, b.h + 24.0, Color::new(0.55, 0.36, 0.2, 1.0));
    draw_rectangle(b.x, b.y, b.w, b.h, Color::new(0.17, 0.3, 0.24, 1.0));
    draw_rectangle(b.x + 40.0, b.y + b.h + 12.0, b.w - 80.0, 8.0, Color::new(0.45, 0.29, 0.16, 1.0));

    let scene = &p.lesson.scenes[p.scene];
    let mut y = b.y + 48.0;
    for l in ink.wrap(ink.chalk(), &scene.heading, b.w - 60.0, 28.0) {
        ink.centered(ink.chalk(), &l, b.x + b.w / 2.0, y, 28.0, Color::new(1.0, 0.92, 0.55, 1.0));
        y += 36.0;
    }
    y += 10.0;

    // bullets appear progressively through the first 80% of the scene
    let prog = p.scene_progress();
    let n = scene.board.len().max(1) as f32;
    for (i, bullet) in scene.board.iter().enumerate() {
        if prog < (i as f32 / n) * 0.8 {
            break;
        }
        draw_circle(b.x + 36.0, y - 7.0, 4.0, CHALK);
        for l in ink.wrap(ink.chalk(), bullet, b.w - 90.0, 20.0) {
            if y > b.y + b.h - 10.0 {
                break;
            }
            ink.draw(ink.chalk(), &l, b.x + 52.0, y, 20.0, CHALK);
            y += 28.0;
        }
        y += 8.0;
    }
}

fn draw_subtitle(p: &Player, ink: &Ink) {
    let line = &p.lesson.scenes[p.scene].lines[p.line];
    let (name, col) = match line.speaker {
        Speaker::Teacher => (&p.lesson.teacher_name, TEACH_COL),
        Speaker::Student => (&p.lesson.student_name, STUD_COL),
        Speaker::Narrator => (&p.lesson.teacher_name, Color::new(0.85, 0.55, 0.1, 1.0)),
    };
    let size = 23.0;
    let rows = ink.wrap(ink.ui(), &line.text, W - 120.0, size);
    // show two rows at a time, advancing in step with the speech
    let chunks: Vec<&[String]> = rows.chunks(2).collect();
    let total: usize = rows.iter().map(|r| r.len()).sum::<usize>().max(1);
    let frac = (p.t / line.duration.max(0.1)).clamp(0.0, 0.999);
    let mut acc = 0usize;
    let mut idx = chunks.len() - 1;
    for (i, c) in chunks.iter().enumerate() {
        acc += c.iter().map(|r| r.len()).sum::<usize>();
        if frac < acc as f32 / total as f32 {
            idx = i;
            break;
        }
    }
    let (y0, h) = (636.0, 76.0);
    draw_rectangle(24.0, y0, W - 48.0, h, Color::new(1.0, 1.0, 1.0, 0.85));
    draw_rectangle(24.0, y0, 6.0, h, col);
    ink.draw(ink.ui(), &name.to_uppercase(), 44.0, y0 + 20.0, 14.0, col);
    for (r, text) in chunks[idx].iter().enumerate() {
        ink.draw(ink.ui(), text, 44.0, y0 + 44.0 + r as f32 * 27.0, size, INK);
    }
}

/// Returns the index of a clicked choice, or Some(usize::MAX) for "continue".
fn draw_quiz(p: &Player, ink: &Ink, picked: Option<usize>, mouse: Vec2, clicked: bool) -> Option<usize> {
    let q = p.lesson.scenes[p.scene].quiz.as_ref().unwrap();
    draw_rectangle(0.0, 0.0, W, H, Color::new(0.0, 0.0, 0.0, 0.45));
    let panel = Rect::new(230.0, 70.0, 820.0, 580.0);
    draw_rectangle(panel.x, panel.y, panel.w, panel.h, PAPER);
    draw_rectangle_lines(panel.x, panel.y, panel.w, panel.h, 4.0, INK);

    ink.draw(ink.ui(), "QUICK CHECK", panel.x + 36.0, panel.y + 42.0, 16.0, TEACH_COL);
    let mut y = panel.y + 80.0;
    for l in ink.wrap(ink.ui(), &q.question, panel.w - 72.0, 25.0) {
        ink.draw(ink.ui(), &l, panel.x + 36.0, y, 25.0, INK);
        y += 33.0;
    }
    y += 10.0;

    let mut result = None;
    for (i, choice) in q.choices.iter().enumerate() {
        let rows = ink.wrap(ink.ui(), choice, panel.w - 150.0, 19.0);
        let h = 22.0 + rows.len() as f32 * 24.0;
        let r = Rect::new(panel.x + 36.0, y, panel.w - 72.0, h);
        let hover = picked.is_none() && r.contains(mouse);
        let (bg, border) = match picked {
            Some(_) if i == q.answer_index => (Color::new(0.86, 0.95, 0.87, 1.0), GOOD),
            Some(sel) if sel == i => (Color::new(0.98, 0.88, 0.87, 1.0), BAD),
            _ if hover => (Color::new(0.93, 0.93, 0.96, 1.0), INK),
            _ => (WHITE, Color::new(0.75, 0.75, 0.78, 1.0)),
        };
        draw_rectangle(r.x, r.y, r.w, r.h, bg);
        draw_rectangle_lines(r.x, r.y, r.w, r.h, 2.5, border);
        ink.draw(ink.ui(), &format!("{}", i + 1), r.x + 18.0, r.y + 33.0, 20.0, border);
        for (k, row) in rows.iter().enumerate() {
            ink.draw(ink.ui(), row, r.x + 56.0, r.y + 34.0 + k as f32 * 24.0, 19.0, INK);
        }
        if hover && clicked {
            result = Some(i);
        }
        y += h + 10.0;
    }

    if let Some(sel) = picked {
        let correct = sel == q.answer_index;
        y += 8.0;
        let verdict = if correct { "Correct!" } else { "Not quite." };
        ink.draw(ink.ui(), verdict, panel.x + 36.0, y + 18.0, 22.0, if correct { GOOD } else { BAD });
        y += 30.0;
        for l in ink.wrap(ink.ui(), &q.explanation, panel.w - 72.0, 18.0).iter().take(4) {
            ink.draw(ink.ui(), l, panel.x + 36.0, y + 18.0, 18.0, INK);
            y += 24.0;
        }
        let btn = Rect::new(panel.x + panel.w - 236.0, panel.y + panel.h - 64.0, 200.0, 44.0);
        let hover = btn.contains(mouse);
        draw_rectangle(btn.x, btn.y, btn.w, btn.h, if hover { Color::new(0.25, 0.25, 0.3, 1.0) } else { INK });
        ink.centered(ink.ui(), "Continue  (Enter)", btn.x + btn.w / 2.0, btn.y + 29.0, 18.0, WHITE);
        if hover && clicked {
            result = Some(usize::MAX);
        }
    } else {
        ink.draw(
            ink.ui(),
            "Click an answer or press 1-4",
            panel.x + 36.0,
            panel.y + panel.h - 30.0,
            15.0,
            Color::new(0.5, 0.5, 0.55, 1.0),
        );
    }
    result
}

fn draw_done(p: &Player, ink: &Ink) {
    let answered = p.results.iter().filter(|r| r.is_some()).count();
    let right = p.results.iter().filter(|r| **r == Some(true)).count();
    draw_rectangle(240.0, 180.0, 800.0, 290.0, Color::new(0.985, 0.972, 0.945, 0.94));
    ink.centered(ink.ui(), "Lesson complete!", W / 2.0, 250.0, 60.0, INK);
    if answered > 0 {
        let pct = right * 100 / answered;
        let msg = format!("Quiz score: {right}/{answered}  ({pct}%)");
        ink.centered(ink.ui(), &msg, W / 2.0, 320.0, 32.0, if pct >= 70 { GOOD } else { BAD });
        let note = if pct == 100 {
            "Perfect! You've got this."
        } else if pct >= 70 {
            "Nice work - replay any red scenes on the bar above to patch the gaps."
        } else {
            "Worth another pass - click the red scenes on the bar above to rewatch them."
        };
        ink.centered(ink.ui(), note, W / 2.0, 370.0, 20.0, INK);
    }
    ink.centered(ink.ui(), "R  watch again      Q  quit", W / 2.0, 430.0, 20.0, Color::new(0.45, 0.45, 0.5, 1.0));
}
