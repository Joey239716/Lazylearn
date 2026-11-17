//! The Lazylearn window: integrated title bar, projects sidebar, and the main pane
//! (new-lesson composer, a project's stage cards + player + quiz, or settings).

use crate::jobs::{Job, JobKind, JobState, StageStatus};
use crate::library::{self, Project, QuizResult};
use crate::player::{fmt_time, Player};
use crate::settings::Settings;
use crate::theme::{self, *};
use crate::theme::{green, red};
use clap::ValueEnum;
use gpui::prelude::FluentBuilder;
use gpui::*;
use gpui_kit::component::input::{Input, InputEvent, InputState, Textarea, TextareaState};
use gpui_kit::component::{Sizable, TitleBar};
use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant};
use stickstudy::lesson::Quiz;
use stickstudy::pipeline::{self, LessonInfo, MakeOptions, VideoOptions};
use stickstudy::{omnivoice, script, styles, tts};

const SIDEBAR_W: f32 = 264.;
const COLUMN_W: f32 = 780.;


#[derive(Clone, PartialEq)]
enum Page {
    New,
    Project(PathBuf),
    Settings,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Menu {
    Style,
    Cast,
    Teacher,
    Student,
    Energy,
    Pace,
    Provider,
    Rerender,
    SetStyle,
    SetCast,
    SetTeacher,
    SetStudent,
    SetEnergy,
    SetPace,
}

/// Options picked in the composer (start from the settings' defaults).
#[derive(Clone)]
struct Draft {
    style: String,
    cast: String,
    teacher: String,
    student: String,
    energy: String,
    pace: f32,
    vertical: bool,
    provider: String,
}

impl Draft {
    fn from(s: &Settings) -> Draft {
        Draft {
            style: s.style.clone(),
            cast: s.cast.clone(),
            teacher: s.teacher_voice.clone(),
            student: s.student_voice.clone(),
            energy: s.energy.clone(),
            pace: s.pace,
            vertical: s.vertical,
            provider: s.provider.clone(),
        }
    }
}

struct QuizRun {
    dir: PathBuf,
    questions: Vec<Quiz>,
    idx: usize,
    picked: Option<usize>,
    score: usize,
    result: Option<QuizResult>,
}

pub struct LazyApp {
    settings: Settings,
    projects: Vec<Project>,
    page: Page,
    jobs: Vec<Job>,
    handled: Vec<u64>,
    next_job: u64,
    sidebar_open: bool,
    // composer
    attachments: Vec<PathBuf>,
    prompt: Entity<TextareaState>,
    draft: Draft,
    menu: Option<Menu>,
    menu_closed: Option<(Menu, Instant)>,
    // library
    search: Entity<InputState>,
    // project view
    player: Option<Player>,
    player_vertical: bool,
    quiz: Option<QuizRun>,
    theater: bool,
    scrub: Option<f32>,
    seek_bounds: Rc<Cell<Bounds<Pixels>>>,
    last_move: Instant,
    confirm_delete: Option<(PathBuf, Instant)>,
    /// open this project's player on the next render (after a job finishes)
    pending_open: Option<(PathBuf, bool)>,
    // settings inputs
    in_lessons: Entity<InputState>,
    in_omni: Entity<InputState>,
    in_ffmpeg: Entity<InputState>,
    toast: Option<(String, Instant)>,
    focus: FocusHandle,
    has_omnivoice: bool,
    backfilled: std::sync::Arc<std::sync::atomic::AtomicBool>,
    _subs: Vec<Subscription>,
    _poll: Task<()>,
}

fn parse<T: ValueEnum + Default>(s: &str) -> T {
    T::from_str(s, true).unwrap_or_default()
}

fn pretty_stem(p: &Path) -> String {
    let s = p.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "lesson".into());
    let s = s.replace(['_', '-'], " ");
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => s,
    }
}

fn file_icon(p: &Path) -> &'static str {
    match p.extension().map(|e| e.to_string_lossy().to_lowercase()).as_deref() {
        Some("pdf") => "file-text",
        Some("pptx") | Some("ppt") | Some("key") => "presentation",
        Some("png") | Some("jpg") | Some("jpeg") | Some("webp") | Some("gif") | Some("heic") => "image",
        Some("vtt") | Some("srt") => "captions",
        _ => "file",
    }
}

impl LazyApp {
    pub fn new(settings: Settings, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let prompt = cx.new(|cx| {
            TextareaState::new(window, cx)
                .auto_grow(2, 10)
                .placeholder("What should the video focus on? e.g. \"chapters 1–7 for my midterm, go slow on the proofs\"")
        });
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Search"));
        let mk = |v: String, ph: &'static str, window: &mut Window, cx: &mut Context<Self>| {
            cx.new(|cx| InputState::new(window, cx).placeholder(ph).default_value(v))
        };
        let in_lessons = mk(settings.lessons_dir.display().to_string(), "Folder for your lessons", window, cx);
        let in_omni = mk(settings.omnivoice_dir.clone(), "Auto-detect", window, cx);
        let in_ffmpeg = mk(settings.ffmpeg.clone(), "ffmpeg on PATH", window, cx);

        let mut subs = vec![];
        subs.push(cx.subscribe_in(&search, window, |_, _, ev: &InputEvent, _, cx| {
            if matches!(ev, InputEvent::Change) {
                cx.notify();
            }
        }));
        subs.push(cx.subscribe_in(&prompt, window, |this, _, ev: &InputEvent, window, cx| {
            if let InputEvent::PressEnter { secondary: true, .. } = ev {
                this.make(window, cx);
            }
        }));
        for (i, input) in [in_lessons.clone(), in_omni.clone(), in_ffmpeg.clone()].into_iter().enumerate() {
            subs.push(cx.subscribe_in(&input, window, move |this, state, ev: &InputEvent, _, cx| {
                if matches!(ev, InputEvent::Change) {
                    let v = state.read(cx).value().to_string();
                    match i {
                        0 => {
                            if !v.trim().is_empty() {
                                this.settings.lessons_dir = PathBuf::from(v.trim());
                            }
                        }
                        1 => this.settings.omnivoice_dir = v,
                        _ => this.settings.ffmpeg = v,
                    }
                    this.settings.save();
                    this.settings.apply_env();
                    this.has_omnivoice = omnivoice::find_dir().is_some();
                    if i == 0 {
                        this.rescan();
                    }
                    cx.notify();
                }
            }));
        }

        let poll = cx.spawn(async move |this: WeakEntity<LazyApp>, cx: &mut AsyncApp| loop {
            cx.background_executor().timer(Duration::from_millis(150)).await;
            if this.update(cx, |this, cx| this.poll(cx)).is_err() {
                break;
            }
        });

        let draft = Draft::from(&settings);
        let projects = library::scan(&settings.lessons_dir);
        let backfilled = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        backfill(&projects, backfilled.clone());
        let mut this = LazyApp {
            settings,
            projects,
            page: Page::New,
            jobs: vec![],
            handled: vec![],
            next_job: 1,
            sidebar_open: true,
            attachments: vec![],
            prompt,
            draft,
            menu: None,
            menu_closed: None,
            search,
            player: None,
            player_vertical: false,
            quiz: None,
            theater: false,
            scrub: None,
            seek_bounds: Rc::new(Cell::new(Bounds::default())),
            last_move: Instant::now(),
            confirm_delete: None,
            pending_open: None,
            in_lessons,
            in_omni,
            in_ffmpeg,
            toast: None,
            focus: cx.focus_handle(),
            has_omnivoice: omnivoice::find_dir().is_some(),
            backfilled,
            _subs: subs,
            _poll: poll,
        };
        #[cfg(debug_assertions)]
        this.dev_start(window, cx);
        this
    }

    /// Dev hooks for checking screens without clicking: LAZYLEARN_OPEN=<lesson dir>|settings,
    /// LAZYLEARN_SEEK=<s>, LAZYLEARN_QUIZ=1, LAZYLEARN_PICK=<n>, LAZYLEARN_MENU=<Menu>, LAZYLEARN_ATTACH=<a;b>.
    #[cfg(debug_assertions)]
    fn dev_start(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let env = |k: &str| std::env::var(k).ok().filter(|v| !v.is_empty());
        if let Some(a) = env("LAZYLEARN_ATTACH") {
            self.attach(a.split(';').map(PathBuf::from));
        }
        match env("LAZYLEARN_OPEN").as_deref() {
            Some("settings") => self.page = Page::Settings,
            Some(d) => {
                let d = std::path::absolute(d).unwrap_or_else(|_| PathBuf::from(d));
                let d = self.projects.iter().find(|p| p.dir.canonicalize().ok() == d.canonicalize().ok()).map(|p| p.dir.clone()).unwrap_or(d);
                self.open_page(Page::Project(d), window, cx);
            }
            None => {}
        }
        if let (Some(t), Some(p)) = (env("LAZYLEARN_SEEK"), &mut self.player) {
            p.seek(t.parse().unwrap_or(0.0));
        }
        if env("LAZYLEARN_QUIZ").is_some() {
            if let Page::Project(d) = self.page.clone() {
                self.start_quiz(d);
                if let Some(n) = env("LAZYLEARN_PICK") {
                    self.quiz_pick(n.parse::<usize>().unwrap_or(1).saturating_sub(1));
                }
            }
        }
        if let Some(m) = env("LAZYLEARN_MENU") {
            self.menu = [Menu::Style, Menu::Cast, Menu::Teacher, Menu::Energy, Menu::Pace, Menu::Provider, Menu::Rerender, Menu::SetStyle]
                .into_iter()
                .find(|x| format!("{x:?}").eq_ignore_ascii_case(&m));
        }
    }

    // ------------------------------------------------------------------ state

    fn rescan(&mut self) {
        self.projects = library::scan(&self.settings.lessons_dir);
    }

    fn poll(&mut self, cx: &mut Context<Self>) {
        let mut dirty = false;
        if self.backfilled.swap(false, std::sync::atomic::Ordering::Relaxed) {
            self.rescan();
            dirty = true;
        }
        let mut finished = vec![];
        for j in &self.jobs {
            if self.handled.contains(&j.id) {
                continue;
            }
            dirty = true;
            if j.finished() {
                finished.push((j.id, j.dir(), j.kind));
            }
        }
        for (id, dir, kind) in finished {
            self.handled.push(id);
            self.rescan();
            let st = self.jobs.iter().find(|j| j.id == id).map(|j| j.state.clone());
            let ok = st.map(|s| s.lock().unwrap().error.is_none()).unwrap_or(false);
            if let (Some(d), true) = (dir, ok) {
                if self.page == Page::Project(d.clone()) && self.player.is_none() {
                    self.pending_open = Some((d.clone(), kind == JobKind::Make));
                }
                let name = self.projects.iter().find(|p| p.dir == d).map(|p| p.title.clone()).unwrap_or_default();
                self.toast(format!("“{name}” is ready"));
            }
        }
        if let Some((_, t)) = &self.toast {
            if t.elapsed() > Duration::from_secs(4) {
                self.toast = None;
                dirty = true;
            }
        }
        if let Some((_, t)) = &self.confirm_delete {
            if t.elapsed() > Duration::from_secs(4) {
                self.confirm_delete = None;
                dirty = true;
            }
        }
        if self.theater && self.last_move.elapsed() < Duration::from_secs(4) {
            dirty = true;
        }
        if dirty || self.pending_open.is_some() {
            cx.notify();
        }
    }

    fn toast(&mut self, s: impl Into<String>) {
        self.toast = Some((s.into(), Instant::now()));
    }

    fn job_for(&self, dir: &Path) -> Option<&Job> {
        self.jobs.iter().rev().find(|j| j.dir().as_deref() == Some(dir))
    }

    fn running_job_for(&self, dir: &Path) -> Option<&Job> {
        self.job_for(dir).filter(|j| !j.finished())
    }

    fn open_page(&mut self, page: Page, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(mut p) = self.player.take() {
            p.release(window);
        }
        self.quiz = None;
        self.theater = false;
        self.menu = None;
        self.scrub = None;
        if let Page::Project(d) = &page {
            self.player_vertical = false;
            self.open_player(d.clone(), false, window);
            window.focus(&self.focus, cx);
        }
        if page == Page::New {
            let p = self.prompt.clone();
            p.update(cx, |s, cx| s.focus(window, cx));
        }
        self.page = page;
        cx.notify();
    }

    fn open_player(&mut self, dir: PathBuf, autoplay: bool, window: &mut Window) {
        if let Some(mut p) = self.player.take() {
            p.release(window);
        }
        if self.running_job_for(&dir).is_some() {
            return;
        }
        let Some(proj) = self.projects.iter().find(|p| p.dir == dir) else { return };
        let file = if self.player_vertical { proj.vertical_video.clone() } else { proj.video.clone() }
            .or_else(|| proj.video.clone())
            .or_else(|| proj.vertical_video.clone());
        if let Some(f) = file {
            self.player_vertical = f.file_name().map(|n| n.to_string_lossy().contains("vertical")).unwrap_or(false);
            self.player = Some(Player::open(f, self.settings.ffmpeg_bin(), proj.duration, autoplay));
        }
    }

    fn voice_args(&self) -> tts::VoiceArgs {
        let d = &self.draft;
        let opt = |s: &str| (!s.trim().is_empty()).then(|| s.trim().to_string());
        tts::VoiceArgs {
            backend: tts::Backend::Auto,
            teacher_voice: if self.has_omnivoice { opt(&d.teacher) } else { None },
            student_voice: if self.has_omnivoice { opt(&d.student) } else { None },
            speed: None,
            energy: parse(&d.energy),
            pace: d.pace,
        }
    }

    fn make(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.attachments.is_empty() {
            self.toast("Attach your class material first: PDF, slides, notes or a transcript");
            cx.notify();
            return;
        }
        let focus = self.prompt.read(cx).value().trim().to_string();
        let lessons = self.settings.lessons_dir.clone();
        if let Err(e) = std::fs::create_dir_all(&lessons) {
            self.toast(format!("Can't create the lessons folder: {e}"));
            cx.notify();
            return;
        }
        let first = self.attachments[0].clone();
        let dir = pipeline::unique_dir(&lessons, &first.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default());
        let _ = std::fs::create_dir_all(&dir);
        let info = LessonInfo {
            title: pretty_stem(&first),
            duration: 0.0,
            style: self.draft.style.clone(),
            inputs: self.attachments.iter().map(|p| p.display().to_string()).collect(),
            focus: (!focus.is_empty()).then(|| focus.clone()),
            created: pipeline::now_secs(),
        };
        let _ = info.save(&dir);
        self.start_make(dir.clone(), self.attachments.clone(), focus, window, cx);
        self.attachments.clear();
        self.prompt.update(cx, |s, cx| s.set_value("", window, cx));
        self.rescan();
        self.open_page(Page::Project(dir), window, cx);
    }

    fn start_make(&mut self, dir: PathBuf, inputs: Vec<PathBuf>, focus: String, _window: &mut Window, cx: &mut Context<Self>) {
        let d = self.draft.clone();
        let opts = MakeOptions {
            inputs,
            out: Some(dir.clone()),
            lessons_dir: self.settings.lessons_dir.clone(),
            cast: parse(&d.cast),
            scenes: None,
            focus: (!focus.is_empty()).then_some(focus),
            model: None,
            provider: parse(&d.provider),
            no_vision: !self.settings.vision,
            teaching: None,
            voice: self.voice_args(),
            video: VideoOptions { style: Some(parse(&d.style)), vertical: d.vertical, ..Default::default() },
        };
        let id = self.next_job;
        self.next_job += 1;
        let title = LessonInfo::load(&dir).map(|i| i.title).unwrap_or_default();
        self.jobs.push(Job::spawn(id, JobKind::Make, title, Some(dir), move |p, _| {
            pipeline::make(&opts, p, |_| {}).map(|_| ())
        }));
        cx.notify();
    }

    fn rerender(&mut self, dir: PathBuf, style: Option<styles::Style>, vertical: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.running_job_for(&dir).is_some() {
            return;
        }
        if let Some(mut p) = self.player.take() {
            p.release(window);
        }
        self.quiz = None;
        let id = self.next_job;
        self.next_job += 1;
        let video = VideoOptions { style, vertical, ..Default::default() };
        let d2 = dir.clone();
        self.jobs.push(Job::spawn(id, JobKind::Render, String::new(), Some(dir), move |p, _| {
            let mut lesson = stickstudy::lesson::Lesson::load(&d2)?;
            pipeline::render_videos(&mut lesson, &d2, &video, p)
        }));
        cx.notify();
    }

    fn revoice(&mut self, dir: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        if self.running_job_for(&dir).is_some() {
            return;
        }
        if let Some(mut p) = self.player.take() {
            p.release(window);
        }
        self.quiz = None;
        let id = self.next_job;
        self.next_job += 1;
        let voice = self.voice_args();
        let has_vertical = dir.join("video-vertical.mp4").is_file();
        let video = VideoOptions { vertical: has_vertical, ..Default::default() };
        let d2 = dir.clone();
        self.jobs.push(Job::spawn(id, JobKind::Revoice, String::new(), Some(dir), move |p, _| {
            pipeline::revoice(&d2, &voice, &video, p)
        }));
        cx.notify();
    }

    fn delete(&mut self, dir: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(j) = self.running_job_for(&dir) {
            j.progress.cancel();
        }
        if let Some(mut p) = self.player.take() {
            p.release(window);
        }
        // only ever delete inside the lessons folder
        if dir.starts_with(&self.settings.lessons_dir) && dir.join("info.json").exists() || dir.join("lesson.json").exists() {
            let _ = std::fs::remove_dir_all(&dir);
        }
        self.confirm_delete = None;
        self.rescan();
        self.open_page(Page::New, window, cx);
    }

    fn attach(&mut self, paths: impl IntoIterator<Item = PathBuf>) {
        for p in paths {
            if p.is_file() && !self.attachments.contains(&p) {
                self.attachments.push(p);
            }
        }
    }

    fn pick_files(&mut self, cx: &mut Context<Self>) {
        let rx = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: Some("Attach".into()),
        });
        cx.spawn(async move |this: WeakEntity<LazyApp>, cx: &mut AsyncApp| {
            if let Ok(Ok(Some(paths))) = rx.await {
                let _ = this.update(cx, |this, cx| {
                    this.attach(paths);
                    cx.notify();
                });
            }
        })
        .detach();
    }

    fn pick_lessons_dir(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let rx = cx.prompt_for_paths(PathPromptOptions { files: false, directories: true, multiple: false, prompt: None });
        let input = self.in_lessons.clone();
        cx.spawn_in(window, async move |_this: WeakEntity<LazyApp>, cx: &mut AsyncWindowContext| {
            if let Ok(Ok(Some(paths))) = rx.await {
                if let Some(p) = paths.into_iter().next() {
                    let _ = input.update_in(cx, |s, window, cx| {
                        s.set_value(p.display().to_string(), window, cx);
                        cx.emit(InputEvent::Change);
                    });
                }
            }
        })
        .detach();
    }

    fn toggle_menu(&mut self, m: Menu, cx: &mut Context<Self>) {
        // a click on the open menu's own chip closes it (mouse-down-out already did)
        if let Some((closed, t)) = self.menu_closed {
            if closed == m && t.elapsed() < Duration::from_millis(250) {
                self.menu_closed = None;
                return;
            }
        }
        self.menu = if self.menu == Some(m) { None } else { Some(m) };
        cx.notify();
    }

    fn close_menu(&mut self, cx: &mut Context<Self>) {
        if let Some(m) = self.menu.take() {
            self.menu_closed = Some((m, Instant::now()));
            cx.notify();
        }
    }

    fn start_quiz(&mut self, dir: PathBuf) {
        let questions = library::load_quiz(&dir);
        if questions.is_empty() {
            self.toast("This lesson has no quiz");
            return;
        }
        self.quiz = Some(QuizRun { dir, questions, idx: 0, picked: None, score: 0, result: None });
    }

    fn quiz_pick(&mut self, i: usize) {
        let Some(q) = &mut self.quiz else { return };
        if q.result.is_some() || q.picked.is_some() || i >= q.questions[q.idx].choices.len() {
            return;
        }
        q.picked = Some(i);
        if i == q.questions[q.idx].answer_index {
            q.score += 1;
        }
    }

    fn quiz_next(&mut self) {
        let Some(q) = &mut self.quiz else { return };
        if q.picked.is_none() {
            return;
        }
        if q.idx + 1 < q.questions.len() {
            q.idx += 1;
            q.picked = None;
        } else if q.result.is_none() {
            q.result = Some(QuizResult::record(&q.dir, q.score, q.questions.len()));
            let dir = q.dir.clone();
            if let Some(p) = self.projects.iter_mut().find(|p| p.dir == dir) {
                p.quiz = QuizResult::load(&dir);
            }
        }
    }

    fn set_theater(&mut self, on: bool, window: &mut Window) {
        if self.theater != on {
            self.theater = on;
            if window.is_fullscreen() != on {
                window.toggle_fullscreen();
            }
            self.last_move = Instant::now();
        }
    }

    fn on_key(&mut self, ev: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let k = ev.keystroke.key.as_str();
        let m = &ev.keystroke.modifiers;
        if (m.control || m.platform) && k == "n" {
            self.open_page(Page::New, window, cx);
            return;
        }
        if (m.control || m.platform) && k == "b" {
            self.sidebar_open = !self.sidebar_open;
            cx.notify();
            return;
        }
        if !matches!(self.page, Page::Project(_)) || !self.focus.is_focused(window) {
            return;
        }
        if let Some(q) = &self.quiz {
            if q.result.is_none() {
                match k {
                    "1" | "2" | "3" | "4" => {
                        self.quiz_pick(k.parse::<usize>().unwrap() - 1);
                        cx.notify();
                        return;
                    }
                    "enter" | "right" => {
                        self.quiz_next();
                        cx.notify();
                        return;
                    }
                    _ => {}
                }
            }
        }
        if let Some(p) = &mut self.player {
            match k {
                "space" | "k" => p.toggle(),
                "left" | "j" => {
                    let t = p.time();
                    p.seek(t - 5.0)
                }
                "right" | "l" => {
                    let t = p.time();
                    p.seek(t + 5.0)
                }
                "m" => p.set_volume(if p.volume > 0.0 { 0.0 } else { 1.0 }),
                "f" => {
                    let on = !self.theater;
                    self.set_theater(on, window);
                }
                "escape" => self.set_theater(false, window),
                _ => return,
            }
            cx.notify();
        }
    }
}

// ====================================================================== render

impl Render for LazyApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if let Some((dir, autoplay)) = self.pending_open.take() {
            if self.page == Page::Project(dir.clone()) {
                self.open_player(dir, autoplay, window);
            }
        }
        if let Some(p) = &mut self.player {
            p.tick(window);
            if p.playing || !p.ready() || p.frame.is_none() {
                window.request_animation_frame();
            }
            if p.ended && self.quiz.is_none() {
                if let Page::Project(d) = &self.page {
                    let d = d.clone();
                    self.start_quiz(d);
                    self.set_theater(false, window);
                }
            }
        }
        if self.theater && !window.is_fullscreen() && self.player.is_some() {
            // the user left fullscreen with the OS (e.g. Esc handled by the window)
        }

        let root = div()
            .id("root")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key))
            .size_full()
            .relative()
            .font_family(FONT)
            .text_size(px(14.))
            .text_color(text())
            .bg(sidebar())
            .on_drop(cx.listener(|this, paths: &ExternalPaths, window, cx| {
                this.attach(paths.paths().iter().cloned());
                if this.page != Page::New {
                    this.open_page(Page::New, window, cx);
                }
                cx.notify();
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    if let (Some(f), Some(p)) = (this.scrub.take(), &mut this.player) {
                        let d = p.duration;
                        p.seek(f * d);
                        cx.notify();
                    }
                }),
            )
            .on_mouse_move(cx.listener(|this, ev: &MouseMoveEvent, _, cx| {
                if this.theater {
                    this.last_move = Instant::now();
                }
                if this.scrub.is_some() {
                    let b = this.seek_bounds.get();
                    if b.size.width > px(0.) {
                        this.scrub = Some(((ev.position.x - b.origin.x) / b.size.width).clamp(0.0, 1.0));
                        cx.notify();
                    }
                }
            }));

        if self.theater && self.player.is_some() {
            return root.bg(c(0x000000)).child(self.render_player(true, window, cx)).into_any_element();
        }

        root.child(
            div()
                .size_full()
                .flex()
                .flex_col()
                .child(self.render_title_bar(cx))
                .child(
                    div()
                        .flex()
                        .flex_1()
                        .min_h_0()
                        .when(self.sidebar_open, |d| d.child(self.render_sidebar(cx)))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .h_full()
                                .bg(bg())
                                .border_t_1()
                                .border_color(border())
                                .when(self.sidebar_open, |d| d.border_l_1().rounded_tl(px(12.)))
                                .overflow_hidden()
                                .child(match self.page.clone() {
                                    Page::New => self.render_new(cx).into_any_element(),
                                    Page::Project(d) => self.render_project(&d, window, cx).into_any_element(),
                                    Page::Settings => self.render_settings(cx).into_any_element(),
                                }),
                        ),
                ),
        )
        .when_some(self.toast.clone(), |d, (msg, _)| {
            d.child(
                div().absolute().bottom(px(22.)).left_0().right_0().flex().justify_center().child(
                    div()
                        .px(px(14.))
                        .py(px(8.))
                        .rounded_full()
                        .bg(surface2())
                        .border_1()
                        .border_color(border2())
                        .text_size(px(13.))
                        .shadow_lg()
                        .child(msg),
                ),
            )
        })
        .into_any_element()
    }
}

// ------------------------------------------------------------------ pieces

fn icon_btn(id: impl Into<ElementId>, name: &str) -> Stateful<Div> {
    div()
        .id(id)
        .size(px(28.))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(7.))
        .text_color(muted())
        .cursor_pointer()
        .hover(|s| s.bg(hover()).text_color(text()))
        .active(|s| s.bg(active()))
        .child(theme::icon(name).size(px(16.)))
}

fn ghost_btn(id: impl Into<ElementId>, icon_name: &str, label: impl Into<SharedString>) -> Stateful<Div> {
    div()
        .id(id)
        .h(px(30.))
        .px(px(11.))
        .flex()
        .items_center()
        .gap(px(7.))
        .rounded(px(8.))
        .border_1()
        .border_color(border2())
        .text_size(px(13.))
        .text_color(text())
        .cursor_pointer()
        .hover(|s| s.bg(hover()))
        .active(|s| s.bg(active()))
        .child(theme::icon(icon_name).size(px(14.)).text_color(muted()))
        .child(label.into())
}

fn pill_btn(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Stateful<Div> {
    div()
        .id(id)
        .h(px(32.))
        .px(px(16.))
        .flex()
        .items_center()
        .justify_center()
        .gap(px(7.))
        .rounded_full()
        .bg(text())
        .text_color(c(0x0b0b0c))
        .text_size(px(13.))
        .font_weight(FontWeight::MEDIUM)
        .cursor_pointer()
        .hover(|s| s.bg(c(0xffffff)))
        .active(|s| s.bg(c(0xd6d6da)))
        .child(label.into())
}

fn spinner(id: impl Into<ElementId>, color: Hsla) -> impl IntoElement {
    theme::icon("loader-circle").size(px(14.)).text_color(color).with_animation(
        id,
        Animation::new(Duration::from_millis(900)).repeat(),
        |icon, delta| icon.transform(Transformation::rotate(percentage(delta))),
    )
}

fn brand_mark(size: f32) -> Div {
    div()
        .size(px(size))
        .rounded(px(size * 0.28))
        .bg(gold())
        .flex()
        .items_center()
        .justify_center()
        .font_family("Lilita One")
        .text_size(px(size * 0.68))
        .text_color(c(0x141414))
        .child("L")
}

impl LazyApp {
    fn render_title_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let crumb: Option<SharedString> = match &self.page {
            Page::New => Some("New lesson".into()),
            Page::Settings => Some("Settings".into()),
            Page::Project(d) => self.projects.iter().find(|p| &p.dir == d).map(|p| p.title.clone().into()),
        };
        TitleBar::new()
            .bg(transparent_black())
            .border_color(transparent_black())
            .child(
                div()
                    .flex()
                    .items_center()
                    .h_full()
                    .child(
                        div()
                            .w(px(if self.sidebar_open { SIDEBAR_W - 12. } else { 0. }))
                            .when(!self.sidebar_open, |d| d.w_auto().pr(px(10.)))
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(8.))
                                    .child(brand_mark(18.))
                                    .child(div().text_size(px(13.)).font_weight(FontWeight::SEMIBOLD).child("Lazylearn")),
                            )
                            .child(
                                icon_btn("toggle-sidebar", "panel-left")
                                    .size(px(26.))
                                    .on_mouse_down(MouseButton::Left, |_, window, _| window.prevent_default())
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.sidebar_open = !this.sidebar_open;
                                        cx.notify();
                                    })),
                            ),
                    )
                    .when_some(crumb, |d, c| {
                        d.child(
                            div()
                                .pl(px(18.))
                                .text_size(px(13.))
                                .text_color(muted())
                                .max_w(px(520.))
                                .truncate()
                                .child(c),
                        )
                    }),
            )
    }

    fn render_sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let q = self.search.read(cx).value().to_lowercase();
        let current = match &self.page {
            Page::Project(d) => Some(d.clone()),
            _ => None,
        };
        let items: Vec<AnyElement> = self
            .projects
            .iter()
            .filter(|p| q.is_empty() || p.title.to_lowercase().contains(&q))
            .enumerate()
            .map(|(i, p)| self.render_project_row(i, p, current.as_deref() == Some(&p.dir), cx).into_any_element())
            .collect();
        let empty = items.is_empty();

        div()
            .w(px(SIDEBAR_W))
            .h_full()
            .flex_shrink_0()
            .bg(sidebar())
            .flex()
            .flex_col()
            .px(px(10.))
            .pb(px(10.))
            .gap(px(2.))
            // new lesson
            .child(
                div()
                    .id("new-lesson")
                    .mt(px(6.))
                    .h(px(34.))
                    .px(px(10.))
                    .flex()
                    .items_center()
                    .gap(px(9.))
                    .rounded(px(8.))
                    .cursor_pointer()
                    .when(self.page == Page::New, |d| d.bg(active()))
                    .hover(|s| s.bg(hover()))
                    .child(
                        div()
                            .size(px(20.))
                            .rounded_full()
                            .bg(text())
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(theme::icon("plus").size(px(13.)).text_color(c(0x0b0b0c))),
                    )
                    .child(div().flex_1().text_size(px(14.)).font_weight(FontWeight::MEDIUM).child("New lesson"))
                    .child(kbd("Ctrl N"))
                    .on_click(cx.listener(|this, _, window, cx| this.open_page(Page::New, window, cx))),
            )
            // search
            .child(
                div()
                    .h(px(32.))
                    .px(px(10.))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .rounded(px(8.))
                    .text_color(faint())
                    .hover(|s| s.bg(hover()))
                    .child(theme::icon("search").size(px(14.)))
                    .child(div().flex_1().text_size(px(13.)).child(Input::new(&self.search).appearance(false).small())),
            )
            .child(div().pt(px(14.)).pb(px(6.)).px(px(10.)).child(label("Lessons")))
            .child(
                div()
                    .id("projects")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .gap(px(1.))
                    .children(items)
                    .when(empty, |d| {
                        d.child(
                            div().px(px(10.)).py(px(8.)).text_size(px(13.)).text_color(faint()).child(if q.is_empty() {
                                "Your lessons will show up here."
                            } else {
                                "No matches."
                            }),
                        )
                    }),
            )
            .child(div().h(px(1.)).bg(border()).mx(px(6.)).my(px(6.)))
            .child(
                div()
                    .id("settings")
                    .h(px(34.))
                    .px(px(10.))
                    .flex()
                    .items_center()
                    .gap(px(9.))
                    .rounded(px(8.))
                    .cursor_pointer()
                    .text_color(muted())
                    .text_size(px(13.))
                    .when(self.page == Page::Settings, |d| d.bg(active()).text_color(text()))
                    .hover(|s| s.bg(hover()).text_color(text()))
                    .child(theme::icon("settings").size(px(15.)))
                    .child("Settings")
                    .on_click(cx.listener(|this, _, window, cx| this.open_page(Page::Settings, window, cx))),
            )
    }

    fn render_project_row(&self, i: usize, p: &Project, selected: bool, cx: &mut Context<Self>) -> impl IntoElement {
        let job = self.running_job_for(&p.dir).map(|j| (j.kind, j.state.lock().unwrap().overall(j.kind), j.state.lock().unwrap().current().map(|c| c.stage)));
        let failed = self
            .job_for(&p.dir)
            .map(|j| j.finished() && j.state.lock().unwrap().error.is_some())
            .unwrap_or(false);
        let meta: AnyElement = if let Some((kind, f, stage)) = job {
            let what = match stage {
                Some(s) => short_stage(s),
                None => if kind == JobKind::Make { "Starting" } else { "Working" },
            };
            div()
                .flex()
                .items_center()
                .gap(px(6.))
                .child(dot(violet()))
                .child(format!("{what} · {:.0}%", f * 100.0))
                .into_any_element()
        } else if failed || (p.video.is_none() && p.vertical_video.is_none()) {
            div()
                .flex()
                .items_center()
                .gap(px(6.))
                .child(dot(if failed { red() } else { faint() }))
                .child(if failed { "Failed" } else { "Not finished" })
                .into_any_element()
        } else {
            let mut parts = vec![];
            let len = p.length_label();
            if !len.is_empty() {
                parts.push(len);
            }
            parts.push(library::ago(p.created));
            div()
                .flex()
                .items_center()
                .gap(px(6.))
                .child(parts.join(" · "))
                .when_some(p.quiz.clone(), |d, q| {
                    let perfect = q.best >= q.total && q.total > 0;
                    d.child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(3.))
                            .text_color(if perfect { green() } else { muted() })
                            .child(theme::icon("check").size(px(11.)))
                            .child(format!("{}/{}", q.best, q.total)),
                    )
                })
                .into_any_element()
        };
        let dir = p.dir.clone();
        div()
            .id(("proj", i))
            .px(px(8.))
            .py(px(6.))
            .flex()
            .items_center()
            .gap(px(10.))
            .rounded(px(8.))
            .cursor_pointer()
            .when(selected, |d| d.bg(active()))
            .when(!selected, |d| d.hover(|s| s.bg(hover())))
            .child(
                div()
                    .w(px(46.))
                    .h(px(26.))
                    .flex_shrink_0()
                    .rounded(px(5.))
                    .overflow_hidden()
                    .bg(surface2())
                    .border_1()
                    .border_color(border())
                    .flex()
                    .items_center()
                    .justify_center()
                    .map(|d| match &p.thumb {
                        Some(t) => d.child(img(t.clone()).size_full().object_fit(ObjectFit::Cover)),
                        None => d.child(theme::icon("film").size(px(12.)).text_color(faint())),
                    }),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(px(1.))
                    .child(
                        div()
                            .text_size(px(13.))
                            .text_color(if selected { text() } else { c(0xd4d5d9) })
                            .truncate()
                            .child(p.title.clone()),
                    )
                    .child(div().text_size(px(12.)).text_color(faint()).child(meta)),
            )
            .on_click(cx.listener(move |this, _, window, cx| this.open_page(Page::Project(dir.clone()), window, cx)))
    }

    // ------------------------------------------------------------- new lesson

    fn render_new(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let recent: Vec<Project> = self.projects.iter().filter(|p| p.video.is_some()).take(3).cloned().collect();
        div()
            .id("new-page")
            .size_full()
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .items_center()
            .px(px(32.))
            .child(div().h(relative(0.16)).min_h(px(40.)))
            .child(
                div()
                    .w_full()
                    .max_w(px(COLUMN_W - 40.))
                    .flex()
                    .flex_col()
                    .gap(px(22.))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .items_center()
                            .gap(px(10.))
                            .child(brand_mark(34.))
                            .child(
                                div()
                                    .text_size(px(28.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(text())
                                    .child("What are we learning today?"),
                            )
                            .child(
                                div()
                                    .text_size(px(14.))
                                    .text_color(muted())
                                    .child("Drop in your slides, notes or lecture transcript. You'll get an animated video and a quiz."),
                            ),
                    )
                    .child(self.render_composer(cx))
                    .child(
                        div()
                            .flex()
                            .justify_center()
                            .gap(px(14.))
                            .text_size(px(12.))
                            .text_color(faint())
                            .child("PDF · PowerPoint · .vtt / .srt · .txt / .md · images")
                            .child("·")
                            .child(div().flex().items_center().gap(px(5.)).child(kbd("Ctrl")).child(kbd("Enter")).child("to make")),
                    )
                    .when(!recent.is_empty(), |d| {
                        d.child(
                            div()
                                .mt(px(26.))
                                .flex()
                                .flex_col()
                                .gap(px(10.))
                                .child(label("Recent"))
                                .child(
                                    div().flex().gap(px(12.)).children(recent.into_iter().enumerate().map(|(i, p)| {
                                        let dir = p.dir.clone();
                                        div()
                                            .id(("recent", i))
                                            .flex_1()
                                            .min_w_0()
                                            .flex()
                                            .flex_col()
                                            .rounded(px(10.))
                                            .border_1()
                                            .border_color(border())
                                            .bg(surface())
                                            .overflow_hidden()
                                            .cursor_pointer()
                                            .hover(|s| s.border_color(border2()).bg(surface2()))
                                            .child(div().w_full().aspect_ratio(16. / 9.).bg(c(0x000000)).map(|d| match &p.thumb {
                                                Some(t) => d.child(img(t.clone()).size_full().object_fit(ObjectFit::Cover)),
                                                None => d,
                                            }))
                                            .child(
                                                div()
                                                    .px(px(10.))
                                                    .py(px(8.))
                                                    .flex()
                                                    .flex_col()
                                                    .gap(px(2.))
                                                    .child(div().text_size(px(13.)).truncate().child(p.title.clone()))
                                                    .child(
                                                        div()
                                                            .text_size(px(12.))
                                                            .text_color(faint())
                                                            .child([p.length_label(), library::ago(p.created)].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" · ")),
                                                    ),
                                            )
                                            .on_click(cx.listener(move |this, _, window, cx| {
                                                this.open_page(Page::Project(dir.clone()), window, cx)
                                            }))
                                    })),
                                ),
                        )
                    }),
            )
            .child(div().h(px(60.)))
    }

    fn render_composer(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let atts: Vec<AnyElement> = self
            .attachments
            .iter()
            .enumerate()
            .map(|(i, p)| {
                let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                let ext = p.extension().map(|e| e.to_string_lossy().to_uppercase()).unwrap_or_default();
                div()
                    .id(("att", i))
                    .group("att")
                    .relative()
                    .h(px(46.))
                    .max_w(px(230.))
                    .pl(px(8.))
                    .pr(px(12.))
                    .flex()
                    .items_center()
                    .gap(px(9.))
                    .rounded(px(9.))
                    .bg(surface2())
                    .border_1()
                    .border_color(border2())
                    .child(
                        div()
                            .size(px(30.))
                            .rounded(px(6.))
                            .bg(ca(0xf2b84b, 0.12))
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(theme::icon(file_icon(p)).size(px(15.)).text_color(gold())),
                    )
                    .child(
                        div()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .child(div().text_size(px(13.)).truncate().child(name))
                            .child(div().text_size(px(11.)).text_color(faint()).child(ext)),
                    )
                    .child(
                        div()
                            .id(("att-x", i))
                            .absolute()
                            .top(px(-6.))
                            .right(px(-6.))
                            .size(px(18.))
                            .rounded_full()
                            .bg(border2())
                            .border_1()
                            .border_color(bg())
                            .flex()
                            .items_center()
                            .justify_center()
                            .invisible()
                            .group_hover("att", |s| s.visible())
                            .cursor_pointer()
                            .child(theme::icon("x").size(px(10.)).text_color(text()))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if i < this.attachments.len() {
                                    this.attachments.remove(i);
                                }
                                cx.notify();
                            })),
                    )
                    .into_any_element()
            })
            .collect();
        let has_atts = !atts.is_empty();
        let ready = has_atts;
        let d = &self.draft;
        let style_name = cap(&d.style);
        let voices = if !self.has_omnivoice {
            "System voices".to_string()
        } else if d.cast == "solo" {
            short_voice(&d.teacher, omnivoice::DEFAULT_TEACHER)
        } else {
            format!("{} + {}", short_voice(&d.teacher, omnivoice::DEFAULT_TEACHER), short_voice(&d.student, omnivoice::DEFAULT_STUDENT))
        };

        div()
            .id("composer")
            .w_full()
            .flex()
            .flex_col()
            .rounded(px(16.))
            .bg(surface())
            .border_1()
            .border_color(border2())
            .shadow_lg()
            .drag_over::<ExternalPaths>(|s, _, _, _| s.border_color(gold()).bg(c(0x17171a)))
            .child(
                div()
                    .when(has_atts, |d| d.px(px(14.)).pt(px(14.)).flex().flex_wrap().gap(px(10.)).children(atts)),
            )
            .child(
                div()
                    .px(px(16.))
                    .pt(px(12.))
                    .pb(px(4.))
                    .text_size(px(15.))
                    .line_height(px(22.))
                    .child(Textarea::new(&self.prompt).appearance(false)),
            )
            .child(
                div()
                    .px(px(10.))
                    .pb(px(10.))
                    .pt(px(4.))
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .child(
                        icon_btn("attach", "paperclip")
                            .size(px(32.))
                            .flex_shrink_0()
                            .rounded_full()
                            .border_1()
                            .border_color(border2())
                            .on_click(cx.listener(|this, _, _, cx| this.pick_files(cx))),
                    )
                    .child(self.chip(Menu::Style, "palette", style_name, cx))
                    .child(self.chip(Menu::Cast, if d.cast == "solo" { "user" } else { "users" }, if d.cast == "solo" { "Solo".into() } else { "Duo".into() }, cx))
                    .child(self.chip(Menu::Teacher, "audio-lines", voices, cx))
                    .child(self.chip(Menu::Energy, "sparkles", cap(&d.energy), cx))
                    .child(self.chip(Menu::Pace, "gauge", format!("{:.2}×", d.pace).replace(".00×", "×").replace("0×", "×"), cx))
                    .child(
                        div()
                            .id("vertical-chip")
                            .h(px(30.))
                            .px(px(10.))
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .rounded_full()
                            .border_1()
                            .text_size(px(13.))
                            .cursor_pointer()
                            .map(|e| {
                                if d.vertical {
                                    e.border_color(ca(0xf2b84b, 0.5)).bg(ca(0xf2b84b, 0.1)).text_color(gold())
                                } else {
                                    e.border_color(border2()).text_color(muted()).hover(|s| s.text_color(text()).bg(hover()))
                                }
                            })
                            .child(theme::icon("smartphone").size(px(13.)))
                            .child("9:16")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.draft.vertical = !this.draft.vertical;
                                cx.notify();
                            })),
                    )
                    .child(div().flex_1())
                    .child(
                        div()
                            .id("make")
                            .size(px(34.))
                            .flex_shrink_0()
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded_full()
                            .map(|e| {
                                if ready {
                                    e.bg(text()).text_color(c(0x0b0b0c)).cursor_pointer().hover(|s| s.bg(c(0xffffff)))
                                } else {
                                    e.bg(border2()).text_color(faint())
                                }
                            })
                            .tooltip(|window, cx| gpui_kit::component::tooltip::Tooltip::new("Make video  (Ctrl+Enter)").build(window, cx))
                            .child(theme::icon("arrow-up").size(px(17.)))
                            .on_click(cx.listener(|this, _, window, cx| this.make(window, cx))),
                    ),
            )
    }

    fn chip(&self, m: Menu, icon_name: &str, value: String, cx: &mut Context<Self>) -> impl IntoElement {
        let open = self.menu == Some(m);
        div()
            .relative()
            .child(
                div()
                    .id(SharedString::from(format!("chip-{m:?}")))
                    .h(px(30.))
                    .px(px(10.))
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .rounded_full()
                    .border_1()
                    .border_color(if open { border2() } else { border() })
                    .text_size(px(13.))
                    .text_color(if open { text() } else { muted() })
                    .when(open, |d| d.bg(active()))
                    .cursor_pointer()
                    .hover(|s| s.text_color(text()).bg(hover()))
                    .child(theme::icon(icon_name).size(px(13.)))
                    .child(div().max_w(px(150.)).truncate().child(value))
                    .child(theme::icon("chevron-down").size(px(11.)).text_color(faint()))
                    .on_click(cx.listener(move |this, _, _, cx| this.toggle_menu(m, cx))),
            )
            .when(open, |d| d.child(self.menu_panel(m, false, cx)))
    }

    /// The dropdown for a chip. `below`: open downwards (else upwards).
    fn menu_panel(&self, m: Menu, below: bool, cx: &mut Context<Self>) -> impl IntoElement {
        let s = &self.settings;
        let d = &self.draft;
        // (label, description, selected, value)
        let mut items: Vec<(String, String, bool, String)> = vec![];
        let mut title = "";
        match m {
            Menu::Style | Menu::SetStyle => {
                title = "Visual style";
                let cur = if m == Menu::Style { &d.style } else { &s.style };
                for st in styles::ALL {
                    items.push((cap(st.name()), st.tagline().to_string(), cur == st.name(), st.name().to_string()));
                }
            }
            Menu::Cast | Menu::SetCast => {
                title = "Cast";
                let cur = if m == Menu::Cast { &d.cast } else { &s.cast };
                items.push(("Duo".into(), "A teacher and a curious student banter".into(), cur == "duo", "duo".into()));
                items.push(("Solo".into(), "One witty narrator talks to you".into(), cur == "solo", "solo".into()));
            }
            Menu::Teacher | Menu::SetTeacher | Menu::Student | Menu::SetStudent => {
                let teacher = matches!(m, Menu::Teacher | Menu::SetTeacher);
                title = if teacher { "Teacher / narrator voice" } else { "Student voice" };
                let cur = match m {
                    Menu::Teacher => &d.teacher,
                    Menu::Student => &d.student,
                    Menu::SetTeacher => &s.teacher_voice,
                    _ => &s.student_voice,
                };
                let default = if teacher { omnivoice::DEFAULT_TEACHER } else { omnivoice::DEFAULT_STUDENT };
                for (name, desc) in omnivoice::PRESETS {
                    let sel = if cur.is_empty() { *name == default } else { cur == name };
                    items.push((name.to_string(), desc.to_string(), sel, name.to_string()));
                }
            }
            Menu::Energy | Menu::SetEnergy => {
                title = "Energy";
                let cur = if m == Menu::Energy { &d.energy } else { &s.energy };
                for (v, desc) in [("calm", "Relaxed, clear delivery"), ("upbeat", "Bright and friendly"), ("hype", "Big YouTube-host energy")] {
                    items.push((cap(v), desc.into(), cur == v, v.into()));
                }
            }
            Menu::Pace | Menu::SetPace => {
                title = "Speaking pace";
                let cur = if m == Menu::Pace { d.pace } else { s.pace };
                for (v, desc) in [(0.9, "Slower"), (1.0, "Natural"), (1.1, "A bit faster"), (1.2, "Fast"), (1.3, "Speed-run")] {
                    items.push((format!("{v:.1}×"), desc.into(), (cur - v).abs() < 0.01, format!("{v}")));
                }
            }
            Menu::Provider => {
                title = "Script writer";
                for (v, desc) in [("gemini", "Gemini (default): reads whole textbooks and diagrams"), ("claude", "Claude Sonnet via Claude Code")] {
                    items.push((cap(v), desc.into(), s.provider == v, v.into()));
                }
            }
            Menu::Rerender => {
                title = "Re-render in style";
                for st in styles::ALL {
                    items.push((cap(st.name()), st.tagline().to_string(), false, st.name().to_string()));
                }
            }
        }
        // the voices chip in the composer edits both voices
        let duo_voices = m == Menu::Teacher && d.cast != "solo" && self.has_omnivoice;
        let many = items.len() > 6;
        let rows: Vec<AnyElement> = items
            .into_iter()
            .enumerate()
            .map(|(i, (lab, desc, sel, val))| {
                div()
                    .id(("mi", i))
                    .px(px(10.))
                    .py(px(7.))
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .rounded(px(7.))
                    .cursor_pointer()
                    .hover(|s| s.bg(hover()))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap(px(1.))
                            .child(div().text_size(px(13.)).text_color(text()).child(lab))
                            .child(div().text_size(px(12.)).text_color(faint()).truncate().child(desc)),
                    )
                    .when(sel, |d| d.child(theme::icon("check").size(px(14.)).text_color(gold())))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.pick(m, &val, window, cx);
                    }))
                    .into_any_element()
            })
            .collect();

        let panel = div()
            .id(SharedString::from(format!("menu-{m:?}")))
            .occlude()
            .w(px(if duo_voices { 300. } else { 290. }))
            .p(px(5.))
            .rounded(px(12.))
            .bg(surface())
            .border_1()
            .border_color(border2())
            .shadow_xl()
            .flex()
            .flex_col()
            .on_mouse_down_out(cx.listener(|this, _, _, cx| this.close_menu(cx)))
            .child(div().px(px(10.)).pt(px(6.)).pb(px(4.)).child(label(title)))
            .when(duo_voices, |d| {
                d.child(div().px(px(10.)).pb(px(6.)).text_size(px(12.)).text_color(faint()).child("Pick the teacher here; the student voice is in Settings."))
            })
            .child(
                div()
                    .id("menu-scroll")
                    .when(many, |d| d.max_h(px(330.)).overflow_y_scroll())
                    .flex()
                    .flex_col()
                    .children(rows),
            );
        let anchor = if below { Anchor::TopRight } else { Anchor::BottomLeft };
        div()
            .absolute()
            .when(below, |d| d.bottom_0().right_0())
            .when(!below, |d| d.top_0().left_0())
            .child(deferred(
                anchored()
                    .anchor(anchor)
                    .snap_to_window_with_margin(px(8.))
                    .child(div().when(below, |d| d.mt(px(6.))).when(!below, |d| d.mb(px(8.))).child(panel)),
            ))
    }

    fn pick(&mut self, m: Menu, val: &str, window: &mut Window, cx: &mut Context<Self>) {
        let s = &mut self.settings;
        let d = &mut self.draft;
        match m {
            Menu::Style => d.style = val.into(),
            Menu::Cast => d.cast = val.into(),
            Menu::Teacher => d.teacher = val.into(),
            Menu::Student => d.student = val.into(),
            Menu::Energy => d.energy = val.into(),
            Menu::Pace => d.pace = val.parse().unwrap_or(1.0),
            Menu::Provider => {
                s.provider = val.into();
                d.provider = val.into();
            }
            Menu::SetStyle => {
                s.style = val.into();
                d.style = val.into();
            }
            Menu::SetCast => {
                s.cast = val.into();
                d.cast = val.into();
            }
            Menu::SetTeacher => {
                s.teacher_voice = val.into();
                d.teacher = val.into();
            }
            Menu::SetStudent => {
                s.student_voice = val.into();
                d.student = val.into();
            }
            Menu::SetEnergy => {
                s.energy = val.into();
                d.energy = val.into();
            }
            Menu::SetPace => {
                s.pace = val.parse().unwrap_or(1.0);
                d.pace = s.pace;
            }
            Menu::Rerender => {
                if let Page::Project(dir) = self.page.clone() {
                    let vertical = dir.join("video-vertical.mp4").is_file();
                    self.menu = None;
                    self.rerender(dir, Some(parse(val)), vertical, window, cx);
                    return;
                }
            }
        }
        if !matches!(m, Menu::Style | Menu::Cast | Menu::Teacher | Menu::Student | Menu::Energy | Menu::Pace) {
            self.settings.save();
        }
        self.menu = None;
        cx.notify();
    }

    // ------------------------------------------------------------- project

    fn render_project(&mut self, dir: &Path, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let proj = self.projects.iter().find(|p| p.dir == dir).cloned();
        let job = self.job_for(dir).map(|j| (j.kind, j.state.clone(), j.finished()));
        let running = job.as_ref().map(|j| !j.2).unwrap_or(false);
        let title = proj.as_ref().map(|p| p.title.clone()).unwrap_or_else(|| "Lesson".into());
        let mut meta = vec![];
        if let Some(p) = &proj {
            let l = p.length_label();
            if !l.is_empty() {
                meta.push(l);
            }
            if !p.style.is_empty() {
                meta.push(cap(&p.style));
            }
            meta.push(library::ago(p.created));
            if p.question_count > 0 {
                meta.push(format!("{} quiz questions", p.question_count));
            }
        }
        let has_video = proj.as_ref().map(|p| p.video.is_some() || p.vertical_video.is_some()).unwrap_or(false);
        let has_lesson = dir.join("lesson.json").is_file();
        let confirm = self.confirm_delete.as_ref().map(|(d, _)| d == dir).unwrap_or(false);
        let d1 = dir.to_path_buf();
        let d2 = dir.to_path_buf();
        let d3 = dir.to_path_buf();
        let d4 = dir.to_path_buf();

        let header = div()
            .flex()
            .items_start()
            .justify_between()
            .gap(px(16.))
            .child(
                div()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(px(5.))
                    .child(div().text_size(px(22.)).font_weight(FontWeight::SEMIBOLD).line_height(px(28.)).child(title))
                    .child(div().text_size(px(13.)).text_color(faint()).child(meta.join("  ·  "))),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .flex_shrink_0()
                    .when(has_lesson && !running, |d| {
                        d.child(
                            div()
                                .relative()
                                .child(
                                    ghost_btn("rerender", "refresh-cw", "Re-render")
                                        .on_click(cx.listener(|this, _, _, cx| this.toggle_menu(Menu::Rerender, cx))),
                                )
                                .when(self.menu == Some(Menu::Rerender), |d| d.child(self.menu_panel(Menu::Rerender, true, cx))),
                        )
                        .child(
                            icon_btn("revoice", "mic")
                                .tooltip(|window, cx| gpui_kit::component::tooltip::Tooltip::new("Re-voice with the current voices").build(window, cx))
                                .on_click(cx.listener(move |this, _, window, cx| this.revoice(d1.clone(), window, cx))),
                        )
                    })
                    .child(
                        icon_btn("open-folder", "folder-open")
                            .tooltip(|window, cx| gpui_kit::component::tooltip::Tooltip::new("Open folder").build(window, cx))
                            .on_click(cx.listener(move |_, _, _, cx| cx.open_with_system(&d2))),
                    )
                    .child(if confirm {
                        div()
                            .id("delete-confirm")
                            .h(px(28.))
                            .px(px(10.))
                            .flex()
                            .items_center()
                            .rounded(px(7.))
                            .bg(c(0xc4314b))
                            .text_size(px(13.))
                            .text_color(c(0xffffff))
                            .cursor_pointer()
                            .hover(|s| s.bg(c(0xd23a55)))
                            .child("Delete lesson")
                            .on_click(cx.listener(move |this, _, window, cx| this.delete(d3.clone(), window, cx)))
                            .into_any_element()
                    } else {
                        div()
                            .id("delete")
                            .size(px(28.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(px(7.))
                            .text_color(muted())
                            .cursor_pointer()
                            .hover(|s| s.bg(hover()).text_color(red()))
                            .child(theme::icon("trash").size(px(16.)))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.confirm_delete = Some((d4.clone(), Instant::now()));
                                cx.notify();
                            }))
                            .into_any_element()
                    }),
            );

        // the "message": what went in
        let inputs = proj.as_ref().map(|p| p.inputs.clone()).unwrap_or_default();
        let focus = proj.as_ref().and_then(|p| p.focus.clone());
        let request = (!inputs.is_empty() || focus.is_some()).then(|| {
            div()
                .flex()
                .flex_col()
                .items_end()
                .gap(px(8.))
                .child(div().flex().flex_wrap().justify_end().gap(px(8.)).children(inputs.iter().enumerate().map(|(i, f)| {
                    let p = PathBuf::from(f);
                    let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| f.clone());
                    div()
                        .id(("in", i))
                        .h(px(30.))
                        .px(px(10.))
                        .flex()
                        .items_center()
                        .gap(px(7.))
                        .rounded(px(8.))
                        .bg(surface())
                        .border_1()
                        .border_color(border())
                        .text_size(px(13.))
                        .text_color(muted())
                        .child(theme::icon(file_icon(&p)).size(px(13.)).text_color(gold()))
                        .child(div().max_w(px(260.)).truncate().child(name))
                })))
                .when_some(focus, |d, f| {
                    d.child(
                        div()
                            .max_w(px(560.))
                            .px(px(14.))
                            .py(px(10.))
                            .rounded(px(14.))
                            .rounded_tr(px(4.))
                            .bg(surface2())
                            .text_size(px(14.))
                            .line_height(px(21.))
                            .child(f),
                    )
                })
        });

        let cards = job.as_ref().map(|(kind, st, _)| self.render_stage_cards(*kind, &st.lock().unwrap(), dir, cx));

        let not_finished = !running && !has_video && job.as_ref().map(|j| j.2).unwrap_or(true);
        let retry_inputs: Vec<PathBuf> = inputs.iter().map(PathBuf::from).filter(|p| p.is_file()).collect();
        let retry_focus = proj.as_ref().and_then(|p| p.focus.clone()).unwrap_or_default();
        let d5 = dir.to_path_buf();
        let d6 = dir.to_path_buf();

        div()
            .id("project-page")
            .size_full()
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .items_center()
            .px(px(32.))
            .child(
                div()
                    .w_full()
                    .max_w(px(COLUMN_W))
                    .pt(px(28.))
                    .pb(px(80.))
                    .flex()
                    .flex_col()
                    .gap(px(22.))
                    .child(header)
                    .children(request)
                    .children(cards)
                    .when(not_finished && job.is_none(), |d| {
                        d.child(
                            div()
                                .p(px(16.))
                                .rounded(px(12.))
                                .border_1()
                                .border_color(border())
                                .bg(surface())
                                .flex()
                                .items_center()
                                .gap(px(12.))
                                .child(theme::icon("triangle-alert").size(px(16.)).text_color(gold()))
                                .child(
                                    div()
                                        .flex_1()
                                        .text_size(px(14.))
                                        .text_color(muted())
                                        .child(if has_lesson { "The script is written but the video isn't rendered yet." } else { "This lesson didn't finish." }),
                                )
                                .when(has_lesson, |d| {
                                    d.child(pill_btn("finish", "Voice and render").on_click(cx.listener(move |this, _, window, cx| {
                                        this.revoice(d5.clone(), window, cx)
                                    })))
                                })
                                .when(!has_lesson && !retry_inputs.is_empty(), |d| {
                                    d.child(pill_btn("retry", "Try again").on_click(cx.listener(move |this, _, window, cx| {
                                        this.start_make(d6.clone(), retry_inputs.clone(), retry_focus.clone(), window, cx)
                                    })))
                                }),
                        )
                    })
                    .when(self.player.is_some(), |d| d.child(self.render_player(false, window, cx)))
                    .when(has_video && self.player.is_none() && !running, |d| {
                        let dd = dir.to_path_buf();
                        d.child(
                            div()
                                .id("load-player")
                                .w_full()
                                .aspect_ratio(16. / 9.)
                                .rounded(px(14.))
                                .bg(c(0x000000))
                                .border_1()
                                .border_color(border())
                                .flex()
                                .items_center()
                                .justify_center()
                                .cursor_pointer()
                                .child(play_disc(64.))
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.open_player(dd.clone(), true, window);
                                    cx.notify();
                                })),
                        )
                    })
                    .when(has_video && !running, |d| d.child(self.render_quiz(dir, cx))),
            )
    }

    fn render_stage_cards(&self, kind: JobKind, st: &JobState, dir: &Path, cx: &mut Context<Self>) -> impl IntoElement {
        let running = !st.finished;
        let overall = st.overall(kind);
        let total_elapsed: f32 = st.cards.iter().map(|c| c.elapsed.unwrap_or_else(|| c.started.elapsed().as_secs_f32())).sum();
        let head = match (running, &st.error, st.cancelled) {
            (true, _, _) => format!("Making your video · {:.0}%", overall * 100.0),
            (false, Some(_), _) => "Something went wrong".into(),
            (false, None, true) => "Cancelled".into(),
            (false, None, false) => format!("Done in {}", fmt_dur(total_elapsed)),
        };
        let mut col = div().flex().flex_col().gap(px(8.));
        col = col.child(
            div()
                .flex()
                .items_center()
                .gap(px(10.))
                .child(if running {
                    spinner("job-spin", violet()).into_any_element()
                } else if st.error.is_some() {
                    theme::icon("circle-x").size(px(15.)).text_color(red()).into_any_element()
                } else {
                    theme::icon("circle-check").size(px(15.)).text_color(green()).into_any_element()
                })
                .child(div().flex_1().text_size(px(13.)).text_color(muted()).child(head))
                .when(running, |d| {
                    let dir = dir.to_path_buf();
                    d.child(
                        div()
                            .id("cancel")
                            .text_size(px(13.))
                            .text_color(faint())
                            .cursor_pointer()
                            .hover(|s| s.text_color(red()))
                            .child("Cancel")
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if let Some(j) = this.running_job_for(&dir) {
                                    j.progress.cancel();
                                }
                                cx.notify();
                            })),
                    )
                }),
        );
        if running {
            col = col.child(div().pb(px(4.)).child(bar(overall, violet())));
        }
        for (i, card) in st.cards.iter().enumerate() {
            let (accent, status_el): (Hsla, AnyElement) = match card.status {
                StageStatus::Running => (violet(), spinner(("stage-spin", i), violet()).into_any_element()),
                StageStatus::Done => (green(), theme::icon("check").size(px(14.)).text_color(green()).into_any_element()),
                StageStatus::Failed => (red(), theme::icon("x").size(px(14.)).text_color(red()).into_any_element()),
            };
            let elapsed = card.elapsed.unwrap_or_else(|| card.started.elapsed().as_secs_f32());
            let is_running = card.status == StageStatus::Running;
            let details: Vec<String> = card.details.iter().rev().take(if is_running { 3 } else { 1 }).rev().cloned().collect();
            col = col.child(
                div()
                    .flex()
                    .rounded(px(10.))
                    .bg(surface())
                    .border_1()
                    .border_color(if is_running { border2() } else { border() })
                    .overflow_hidden()
                    .child(div().w(px(2.)).bg(accent).flex_shrink_0())
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .px(px(14.))
                            .py(px(11.))
                            .flex()
                            .flex_col()
                            .gap(px(6.))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(10.))
                                    .child(status_el)
                                    .child(div().text_size(px(12.)).text_color(faint()).child(card.stage.label()))
                                    .child(div().flex_1())
                                    .child(div().text_size(px(12.)).font_family(MONO).text_color(faint()).child(fmt_dur(elapsed))),
                            )
                            .child(div().text_size(px(14.)).text_color(if is_running { text() } else { muted() }).child(card.title.clone()))
                            .when_some(card.frac.clone().filter(|_| is_running), |d, (msg, f)| {
                                d.child(
                                    div()
                                        .flex()
                                        .flex_col()
                                        .gap(px(6.))
                                        .child(div().text_size(px(12.)).font_family(MONO).text_color(muted()).child(msg))
                                        .child(bar(f, gold())),
                                )
                            })
                            .when(!details.is_empty(), |d| {
                                d.child(div().flex().flex_col().gap(px(2.)).children(details.into_iter().map(|l| {
                                    div().text_size(px(12.)).font_family(MONO).text_color(faint()).truncate().child(l)
                                })))
                            }),
                    ),
            );
        }
        if let Some(e) = &st.error {
            col = col.child(
                div()
                    .p(px(14.))
                    .rounded(px(10.))
                    .bg(ca(0xf0645a, 0.08))
                    .border_1()
                    .border_color(ca(0xf0645a, 0.3))
                    .text_size(px(13.))
                    .font_family(MONO)
                    .text_color(c(0xf3a19a))
                    .child(e.clone()),
            );
        }
        col
    }

    fn render_player(&mut self, theater: bool, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = self.player.as_ref().unwrap();
        let t = p.time();
        let dur = p.duration.max(0.01);
        let frac = self.scrub.unwrap_or(t / dur);
        let shown_t = self.scrub.map(|f| f * dur).unwrap_or(t);
        let playing = p.playing;
        let muted_ = p.volume == 0.0;
        let frame = p.frame.clone();
        let loading = frame.is_none();
        let dir = match &self.page {
            Page::Project(d) => Some(d.clone()),
            _ => None,
        };
        let proj = dir.as_ref().and_then(|d| self.projects.iter().find(|p| &p.dir == d)).cloned();
        let has_both = proj.as_ref().map(|p| p.video.is_some() && p.vertical_video.is_some()).unwrap_or(false);
        let show_controls = !theater || !playing || self.last_move.elapsed() < Duration::from_millis(2500);
        let bounds = self.seek_bounds.clone();

        let controls = div()
            .absolute()
            .left_0()
            .right_0()
            .bottom_0()
            .pt(px(36.))
            .px(px(14.))
            .pb(px(10.))
            .bg(linear_gradient(
                180.,
                linear_color_stop(ca(0x000000, 0.0), 0.),
                linear_color_stop(ca(0x000000, 0.72), 1.),
            ))
            .flex()
            .flex_col()
            .gap(px(6.))
            .when(!show_controls, |d| d.invisible())
            // seek bar
            .child(
                div()
                    .id("seek")
                    .group("seek")
                    .h(px(14.))
                    .flex()
                    .items_center()
                    .cursor_pointer()
                    .child(
                        div()
                            .relative()
                            .w_full()
                            .h(px(3.))
                            .group_hover("seek", |s| s.h(px(5.)))
                            .rounded_full()
                            .bg(ca(0xffffff, 0.22))
                            .child(
                                canvas(move |b, _, _| bounds.set(b), |_, _, _, _| {})
                                    .absolute()
                                    .size_full(),
                            )
                            .child(div().h_full().rounded_full().bg(gold()).w(relative(frac.clamp(0.0, 1.0))))
                            .child(
                                div()
                                    .absolute()
                                    .top(px(-4.5))
                                    .left(relative(frac.clamp(0.0, 1.0)))
                                    .ml(px(-6.))
                                    .size(px(12.))
                                    .rounded_full()
                                    .bg(c(0xffffff))
                                    .invisible()
                                    .group_hover("seek", |s| s.visible()),
                            ),
                    )
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, ev: &MouseDownEvent, _, cx| {
                            cx.stop_propagation();
                            let b = this.seek_bounds.get();
                            if b.size.width > px(0.) {
                                this.scrub = Some(((ev.position.x - b.origin.x) / b.size.width).clamp(0.0, 1.0));
                                cx.notify();
                            }
                        }),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .text_color(c(0xffffff))
                    .child(
                        icon_btn("pp", if playing { "pause" } else { "play" })
                            .text_color(c(0xffffff))
                            .on_click(cx.listener(|this, _, window, cx| {
                                cx.stop_propagation();
                                if let Some(p) = &mut this.player {
                                    p.toggle();
                                }
                                window.focus(&this.focus, cx);
                                cx.notify();
                            })),
                    )
                    .child(
                        icon_btn("mute", if muted_ { "volume-x" } else { "volume-2" })
                            .text_color(c(0xffffff))
                            .on_click(cx.listener(|this, _, _, cx| {
                                cx.stop_propagation();
                                if let Some(p) = &mut this.player {
                                    let v = if p.volume > 0.0 { 0.0 } else { 1.0 };
                                    p.set_volume(v);
                                }
                                cx.notify();
                            })),
                    )
                    .child(
                        div()
                            .pl(px(6.))
                            .text_size(px(12.))
                            .font_family(MONO)
                            .text_color(ca(0xffffff, 0.85))
                            .child(format!("{} / {}", fmt_time(shown_t), fmt_time(dur))),
                    )
                    .child(div().flex_1())
                    .when(has_both, |d| {
                        let vertical = self.player_vertical;
                        let dir = dir.clone();
                        d.child(
                            div()
                                .id("orient")
                                .h(px(26.))
                                .px(px(8.))
                                .flex()
                                .items_center()
                                .gap(px(5.))
                                .rounded(px(6.))
                                .text_size(px(12.))
                                .cursor_pointer()
                                .text_color(ca(0xffffff, 0.85))
                                .hover(|s| s.bg(ca(0xffffff, 0.12)))
                                .child(theme::icon(if vertical { "smartphone" } else { "monitor" }).size(px(13.)))
                                .child(if vertical { "9:16" } else { "16:9" })
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    cx.stop_propagation();
                                    this.player_vertical = !vertical;
                                    if let Some(d) = &dir {
                                        this.open_player(d.clone(), true, window);
                                    }
                                    cx.notify();
                                })),
                        )
                    })
                    .child(
                        icon_btn("fs", if theater { "minimize" } else { "maximize" })
                            .text_color(c(0xffffff))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                cx.stop_propagation();
                                this.set_theater(!theater, window);
                                window.focus(&this.focus, cx);
                                cx.notify();
                            })),
                    ),
            );

        div()
            .id("player")
            .relative()
            .w_full()
            .when(theater, |d| d.size_full())
            .when(!theater, |d| d.aspect_ratio(16. / 9.).rounded(px(14.)).border_1().border_color(border()))
            .overflow_hidden()
            .bg(c(0x000000))
            .cursor_pointer()
            .when_some(frame, |d, f| d.child(img(ImageSource::Render(f)).size_full().object_fit(ObjectFit::Contain)))
            .when(loading, |d| {
                d.child(div().absolute().top_0().left_0().right_0().bottom_0().flex().items_center().justify_center().child(spinner("player-load", muted())))
            })
            .when(!playing && !loading, |d| {
                d.child(div().absolute().top_0().left_0().right_0().bottom_0().flex().items_center().justify_center().child(play_disc(64.)))
            })
            .child(controls)
            .on_click(cx.listener(|this, _, window, cx| {
                if let Some(p) = &mut this.player {
                    p.toggle();
                }
                window.focus(&this.focus, cx);
                cx.notify();
            }))
    }

    fn render_quiz(&self, dir: &Path, cx: &mut Context<Self>) -> impl IntoElement {
        let card = div().w_full().rounded(px(14.)).border_1().border_color(border()).bg(surface()).p(px(22.)).flex().flex_col();
        let Some(q) = self.quiz.as_ref().filter(|q| q.dir == dir) else {
            // invitation
            let proj = self.projects.iter().find(|p| p.dir == dir);
            let n = proj.map(|p| p.question_count).unwrap_or(0);
            if n == 0 {
                return div().into_any_element();
            }
            let best = proj.and_then(|p| p.quiz.clone());
            let d = dir.to_path_buf();
            return card
                .flex_row()
                .items_center()
                .gap(px(16.))
                .child(
                    div()
                        .size(px(40.))
                        .rounded(px(10.))
                        .bg(ca(0xf2b84b, 0.12))
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(theme::icon("list-checks").size(px(19.)).text_color(gold())),
                )
                .child(
                    div()
                        .flex_1()
                        .flex()
                        .flex_col()
                        .gap(px(3.))
                        .child(div().text_size(px(15.)).font_weight(FontWeight::MEDIUM).child("Quiz time"))
                        .child(div().text_size(px(13.)).text_color(muted()).child(match &best {
                            Some(b) => format!("{n} questions · best score {}/{} · {} attempt{}", b.best, b.total, b.attempts, if b.attempts == 1 { "" } else { "s" }),
                            None => format!("{n} questions. Opens when the video ends, or take it now."),
                        })),
                )
                .child(pill_btn("take-quiz", if best.is_some() { "Retake quiz" } else { "Take the quiz" }).on_click(cx.listener(move |this, _, window, cx| {
                    if let Some(p) = &mut this.player {
                        if p.playing {
                            p.pause();
                        }
                    }
                    this.start_quiz(d.clone());
                    window.focus(&this.focus, cx);
                    cx.notify();
                })))
                .into_any_element();
        };

        let total = q.questions.len();
        if let Some(r) = &q.result {
            let pct = r.score as f32 / r.total.max(1) as f32;
            let (headline, color) = if pct >= 0.999 {
                ("Perfect score!", green())
            } else if pct >= 0.7 {
                ("Nice work!", gold())
            } else {
                ("Worth another watch", c(0xf3a19a))
            };
            let d = dir.to_path_buf();
            return card
                .items_center()
                .gap(px(10.))
                .py(px(30.))
                .child(theme::icon("trophy").size(px(28.)).text_color(color))
                .child(div().text_size(px(40.)).font_weight(FontWeight::SEMIBOLD).child(format!("{}/{}", r.score, r.total)))
                .child(div().text_size(px(15.)).text_color(color).child(headline))
                .child(div().text_size(px(13.)).text_color(faint()).child(format!("Best {}/{} · attempt {}", r.best, r.total, r.attempts)))
                .child(
                    div()
                        .pt(px(10.))
                        .flex()
                        .gap(px(8.))
                        .child(ghost_btn("rewatch", "rotate-ccw", "Rewatch").on_click(cx.listener(|this, _, window, cx| {
                            this.quiz = None;
                            if let Some(p) = &mut this.player {
                                p.seek(0.0);
                                p.play();
                            }
                            window.focus(&this.focus, cx);
                            cx.notify();
                        })))
                        .child(pill_btn("retake", "Retake quiz").on_click(cx.listener(move |this, _, window, cx| {
                            this.start_quiz(d.clone());
                            window.focus(&this.focus, cx);
                            cx.notify();
                        }))),
                )
                .into_any_element();
        }

        let qq = &q.questions[q.idx];
        let picked = q.picked;
        let options: Vec<AnyElement> = qq
            .choices
            .iter()
            .enumerate()
            .map(|(i, ch)| {
                let correct = i == qq.answer_index;
                let (border_c, bg_c, badge_c) = match picked {
                    Some(p) if correct => (ca(0x3ecf8e, 0.6), ca(0x3ecf8e, 0.09), green()),
                    Some(p) if p == i => (ca(0xf0645a, 0.6), ca(0xf0645a, 0.09), red()),
                    Some(_) => (border(), transparent_black(), faint()),
                    None => (border2(), transparent_black(), muted()),
                };
                let dim = picked.is_some() && !correct && picked != Some(i);
                div()
                    .id(("opt", i))
                    .px(px(14.))
                    .py(px(12.))
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .rounded(px(10.))
                    .border_1()
                    .border_color(border_c)
                    .bg(bg_c)
                    .when(dim, |d| d.opacity(0.5))
                    .when(picked.is_none(), |d| d.cursor_pointer().hover(|s| s.bg(hover()).border_color(c(0x3a3b42))))
                    .child(
                        div()
                            .size(px(24.))
                            .flex_shrink_0()
                            .rounded(px(6.))
                            .border_1()
                            .border_color(badge_c)
                            .text_color(badge_c)
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_size(px(12.))
                            .font_family(MONO)
                            .child(match picked {
                                Some(_) if correct => theme::icon("check").size(px(13.)).into_any_element(),
                                Some(p) if p == i => theme::icon("x").size(px(13.)).into_any_element(),
                                _ => format!("{}", i + 1).into_any_element(),
                            }),
                    )
                    .child(div().flex_1().text_size(px(14.)).line_height(px(20.)).child(ch.clone()))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.quiz_pick(i);
                        window.focus(&this.focus, cx);
                        cx.notify();
                    }))
                    .into_any_element()
            })
            .collect();
        let right = picked.map(|p| p == qq.answer_index);
        card.gap(px(16.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .child(div().text_size(px(12.)).text_color(faint()).child(format!("Question {} of {total}", q.idx + 1)))
                    .child(div().flex_1().child(bar((q.idx + picked.map(|_| 1).unwrap_or(0)) as f32 / total as f32, gold())))
                    .child(div().text_size(px(12.)).font_family(MONO).text_color(faint()).child(format!("{} correct", q.score))),
            )
            .child(div().text_size(px(18.)).font_weight(FontWeight::MEDIUM).line_height(px(26.)).child(qq.question.clone()))
            .child(div().flex().flex_col().gap(px(8.)).children(options))
            .when_some(right, |d, right| {
                d.child(
                    div()
                        .flex()
                        .items_start()
                        .gap(px(14.))
                        .child(
                            div()
                                .flex_1()
                                .flex()
                                .flex_col()
                                .gap(px(4.))
                                .child(
                                    div()
                                        .text_size(px(14.))
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(if right { green() } else { c(0xf3a19a) })
                                        .child(if right { "Correct!" } else { "Not quite." }),
                                )
                                .when(!qq.explanation.is_empty(), |d| {
                                    d.child(div().text_size(px(13.)).line_height(px(19.)).text_color(muted()).child(qq.explanation.clone()))
                                }),
                        )
                        .child(
                            pill_btn("next", if q.idx + 1 < total { "Next" } else { "See score" })
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.quiz_next();
                                    window.focus(&this.focus, cx);
                                    cx.notify();
                                })),
                        ),
                )
            })
            .when(picked.is_none(), |d| {
                d.child(div().flex().items_center().gap(px(6.)).text_size(px(12.)).text_color(faint()).child("Click an answer or press").child(kbd("1")).child("–").child(kbd(format!("{}", qq.choices.len().min(4)))))
            })
            .into_any_element()
    }

    // ------------------------------------------------------------- settings

    fn render_settings(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let s = &self.settings;
        let section = |title: &'static str| div().pt(px(18.)).pb(px(4.)).child(label(title));
        let row = |name: &'static str, hint: SharedString, control: AnyElement| {
            div()
                .py(px(12.))
                .flex()
                .items_center()
                .gap(px(20.))
                .border_b_1()
                .border_color(border())
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap(px(2.))
                        .child(div().text_size(px(14.)).child(name))
                        .child(div().text_size(px(12.)).text_color(faint()).child(hint)),
                )
                .child(div().flex_shrink_0().child(control))
        };
        let field = |input: &Entity<InputState>| {
            div()
                .w(px(300.))
                .h(px(32.))
                .px(px(8.))
                .flex()
                .items_center()
                .rounded(px(8.))
                .border_1()
                .border_color(border2())
                .bg(bg())
                .text_size(px(13.))
                .child(Input::new(input).appearance(false).small())
                .into_any_element()
        };
        let omni = omnivoice::find_dir();
        let ff_ok = stickstudy::render::ffmpeg().is_ok();

        div()
            .id("settings-page")
            .size_full()
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .items_center()
            .px(px(32.))
            .child(
                div()
                    .w_full()
                    .max_w(px(680.))
                    .pt(px(28.))
                    .pb(px(80.))
                    .flex()
                    .flex_col()
                    .child(div().text_size(px(22.)).font_weight(FontWeight::SEMIBOLD).pb(px(4.)).child("Settings"))
                    .child(div().text_size(px(13.)).text_color(muted()).child("Saved automatically."))
                    .child(section("Library"))
                    .child(row(
                        "Lessons folder",
                        "Where new lessons are saved and listed from.".into(),
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .child(field(&self.in_lessons))
                            .child(icon_btn("browse", "folder-open").on_click(cx.listener(|this, _, window, cx| this.pick_lessons_dir(window, cx))))
                            .into_any_element(),
                    ))
                    .child(section("Engines"))
                    .child(row(
                        "ffmpeg",
                        if ff_ok { "Found. Leave empty to use ffmpeg on your PATH.".into() } else { "Not found: install ffmpeg or paste the path to ffmpeg.exe.".into() },
                        field(&self.in_ffmpeg),
                    ))
                    .child(row(
                        "OmniVoice folder",
                        match &omni {
                            Some(d) => format!("Using {}", d.display()).into(),
                            None => "Not found: lessons use your system voices.".into(),
                        },
                        field(&self.in_omni),
                    ))
                    .child(row("Script writer", "Who writes the script and shot list.".into(), self.setting_chip(Menu::Provider, cap(&s.provider), cx)))
                    .child(row(
                        "Read diagrams and scans",
                        "Gemini describes figures, charts and scanned pages before writing.".into(),
                        toggle("vision", s.vision, cx.listener(|this, _, _, cx| {
                            this.settings.vision = !this.settings.vision;
                            this.settings.save();
                            cx.notify();
                        })),
                    ))
                    .child(section("Defaults for new lessons"))
                    .child(row("Style", "The look of the video.".into(), self.setting_chip(Menu::SetStyle, cap(&s.style), cx)))
                    .child(row("Cast", "Duo banter or a solo narrator.".into(), self.setting_chip(Menu::SetCast, cap(&s.cast), cx)))
                    .child(row(
                        "Teacher voice",
                        "OmniVoice preset for the teacher or narrator.".into(),
                        self.setting_chip(Menu::SetTeacher, short_voice(&s.teacher_voice, omnivoice::DEFAULT_TEACHER), cx),
                    ))
                    .child(row(
                        "Student voice",
                        "OmniVoice preset for the student.".into(),
                        self.setting_chip(Menu::SetStudent, short_voice(&s.student_voice, omnivoice::DEFAULT_STUDENT), cx),
                    ))
                    .child(row("Energy", "How lively the voices sound.".into(), self.setting_chip(Menu::SetEnergy, cap(&s.energy), cx)))
                    .child(row("Pace", "Speed every line up without changing pitch.".into(), self.setting_chip(Menu::SetPace, format!("{:.1}×", s.pace), cx)))
                    .child(row(
                        "Also make a 9:16 version",
                        "For Shorts, TikTok and Reels.".into(),
                        toggle("vertical", s.vertical, cx.listener(|this, _, _, cx| {
                            this.settings.vertical = !this.settings.vertical;
                            this.draft.vertical = this.settings.vertical;
                            this.settings.save();
                            cx.notify();
                        })),
                    )),
            )
    }

    fn setting_chip(&self, m: Menu, value: String, cx: &mut Context<Self>) -> AnyElement {
        let open = self.menu == Some(m);
        div()
            .relative()
            .child(
                div()
                    .id(SharedString::from(format!("set-{m:?}")))
                    .h(px(30.))
                    .min_w(px(140.))
                    .px(px(10.))
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(8.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(border2())
                    .text_size(px(13.))
                    .cursor_pointer()
                    .when(open, |d| d.bg(active()))
                    .hover(|s| s.bg(hover()))
                    .child(value)
                    .child(theme::icon("chevron-down").size(px(12.)).text_color(faint()))
                    .on_click(cx.listener(move |this, _, _, cx| this.toggle_menu(m, cx))),
            )
            .when(open, |d| d.child(self.menu_panel(m, true, cx)))
            .into_any_element()
    }
}

fn toggle(id: &'static str, on: bool, f: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> AnyElement {
    div()
        .id(id)
        .w(px(34.))
        .h(px(20.))
        .p(px(2.))
        .rounded_full()
        .cursor_pointer()
        .bg(if on { gold() } else { border2() })
        .flex()
        .when(on, |d| d.justify_end())
        .child(div().size(px(16.)).rounded_full().bg(if on { c(0x141414) } else { muted() }))
        .on_click(f)
        .into_any_element()
}

fn play_disc(size: f32) -> impl IntoElement {
    div()
        .size(px(size))
        .rounded_full()
        .bg(ca(0xffffff, 0.92))
        .flex()
        .items_center()
        .justify_center()
        .shadow_lg()
        .child(theme::icon("play").size(px(size * 0.4)).text_color(c(0x0b0b0c)).ml(px(size * 0.05)))
}

fn cap(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

fn short_voice(v: &str, default: &str) -> String {
    let v = if v.trim().is_empty() { default } else { v.trim() };
    v.strip_prefix("The ").unwrap_or(v).to_string()
}

fn short_stage(s: stickstudy::progress::Stage) -> &'static str {
    use stickstudy::progress::Stage::*;
    match s {
        Reading => "Reading",
        Vision => "Reading figures",
        Script => "Writing script",
        Voices => "Voicing",
        Render => "Rendering",
    }
}

fn fmt_dur(s: f32) -> String {
    let s = s.max(0.0) as u32;
    if s < 60 {
        format!("{s}s")
    } else {
        format!("{}m {:02}s", s / 60, s % 60)
    }
}

/// Older lessons (made before info.json/thumb.png existed): render a thumbnail and record
/// the length in the background, then tell the UI to rescan.
fn backfill(projects: &[Project], done: std::sync::Arc<std::sync::atomic::AtomicBool>) {
    let todo: Vec<PathBuf> = projects
        .iter()
        .filter(|p| p.video.is_some() && (p.thumb.is_none() || p.duration <= 0.0 || !p.dir.join("quiz.json").exists()))
        .map(|p| p.dir.clone())
        .collect();
    if todo.is_empty() {
        return;
    }
    std::thread::spawn(move || {
        for dir in todo {
            let Ok(lesson) = stickstudy::lesson::Lesson::load(&dir) else { continue };
            if !dir.join("quiz.json").exists() {
                let _ = pipeline::write_quiz(&lesson, &dir);
            }
            if !dir.join("thumb.png").exists() {
                let _ = pipeline::save_thumb(&lesson, &dir);
            }
            let mut info = LessonInfo::load(&dir).unwrap_or_default();
            if info.duration <= 0.0 {
                info.duration = stickstudy::timeline::Timeline::build(&lesson, &dir, 1920, 1080).total;
                if info.title.is_empty() {
                    info.title = lesson.title.clone();
                }
                if info.style.is_empty() {
                    info.style = lesson.style.name().to_string();
                }
                if info.created == 0 {
                    info.created = std::fs::metadata(dir.join("lesson.json"))
                        .and_then(|m| m.modified())
                        .ok()
                        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                        .map(|d| d.as_secs())
                        .unwrap_or(0);
                }
                let _ = info.save(&dir);
            }
            done.store(true, std::sync::atomic::Ordering::Relaxed);
        }
    });
}
