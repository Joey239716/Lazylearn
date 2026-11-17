//! Background jobs (make / re-render / re-voice). Each runs on its own thread; the
//! pipeline's progress events are collected into a shared `JobState` that the UI reads.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use stickstudy::progress::{Event, Progress, Stage};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StageStatus {
    Running,
    Done,
    Failed,
}

#[derive(Debug, Clone)]
pub struct StageCard {
    pub stage: Stage,
    pub title: String,
    pub details: Vec<String>,
    /// latest fractional status line + value
    pub frac: Option<(String, f32)>,
    pub status: StageStatus,
    pub started: std::time::Instant,
    pub elapsed: Option<f32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobKind {
    Make,
    Render,
    Revoice,
    /// Gemini plans a course
    Plan,
    /// make the videos of a planned course
    Course,
}

#[derive(Debug, Default)]
pub struct JobState {
    pub cards: Vec<StageCard>,
    pub dir: Option<PathBuf>,
    pub error: Option<String>,
    pub finished: bool,
    pub cancelled: bool,
    /// course builds: (1-based video number, total) and where that video's stage cards start
    pub video: Option<(usize, usize)>,
    pub video_cards_from: usize,
}

impl JobState {
    fn apply(&mut self, e: Event) {
        match e {
            Event::Start(stage, msg) => {
                // a stage can repeat (e.g. horizontal + vertical renders): close the old one
                self.close_running();
                self.cards.push(StageCard {
                    stage,
                    title: msg,
                    details: vec![],
                    frac: None,
                    status: StageStatus::Running,
                    started: std::time::Instant::now(),
                    elapsed: None,
                });
            }
            Event::Info(_, msg) => {
                // the CLI's emoji prefixes look out of place in the monospace detail lines
                let msg = msg.trim_start_matches(|c: char| !c.is_alphanumeric() && c != '"' && c != '(').to_string();
                // "Video 2/5: title" (pipeline::build_course)
                if let Some(rest) = msg.strip_prefix("Video ") {
                    let num: String = rest.chars().take_while(|c| c.is_ascii_digit() || *c == '/').collect();
                    if let Some((a, b)) = num.split_once('/') {
                        if let (Ok(a), Ok(b)) = (a.parse(), b.parse()) {
                            self.video = Some((a, b));
                            self.video_cards_from = self.cards.len();
                        }
                    }
                }
                if let Some(c) = self.cards.last_mut() {
                    c.details.push(msg);
                    if c.details.len() > 40 {
                        c.details.remove(0);
                    }
                }
            }
            Event::Frac(_, msg, f) => {
                if let Some(c) = self.cards.last_mut() {
                    c.frac = Some((msg, f));
                }
            }
            Event::Done(_) => self.close_running(),
        }
    }

    fn close_running(&mut self) {
        if let Some(c) = self.cards.last_mut() {
            if c.status == StageStatus::Running {
                c.status = StageStatus::Done;
                c.elapsed = Some(c.started.elapsed().as_secs_f32());
            }
        }
    }

    /// Overall fraction (0..1) across the expected stages.
    pub fn overall(&self, kind: JobKind) -> f32 {
        if kind == JobKind::Course {
            let Some((i, n)) = self.video else { return 0.0 };
            let per = JobState { cards: self.cards[self.video_cards_from.min(self.cards.len())..].to_vec(), ..Default::default() };
            return (((i - 1) as f32 + per.overall(JobKind::Make)) / n.max(1) as f32).clamp(0.0, 1.0);
        }
        let weights: &[(Stage, f32)] = match kind {
            JobKind::Make => &[(Stage::Reading, 0.03), (Stage::Vision, 0.10), (Stage::Script, 0.25), (Stage::Voices, 0.22), (Stage::Render, 0.40)],
            JobKind::Revoice => &[(Stage::Voices, 0.4), (Stage::Render, 0.6)],
            JobKind::Render => &[(Stage::Render, 1.0)],
            JobKind::Plan => &[(Stage::Reading, 0.1), (Stage::Vision, 0.35), (Stage::Script, 0.55)],
            JobKind::Course => unreachable!(),
        };
        let total: f32 = weights.iter().map(|w| w.1).sum();
        let mut acc = 0.0;
        for (s, w) in weights {
            if let Some(c) = self.cards.iter().rev().find(|c| c.stage == *s) {
                acc += w * match c.status {
                    StageStatus::Running => c.frac.as_ref().map(|f| f.1).unwrap_or(0.3),
                    _ => 1.0,
                };
            }
        }
        (acc / total).clamp(0.0, 1.0)
    }

    pub fn current(&self) -> Option<&StageCard> {
        self.cards.iter().rev().find(|c| c.status == StageStatus::Running)
    }
}

pub struct Job {
    pub id: u64,
    pub kind: JobKind,
    /// the lesson or course folder this job works on
    pub dir: Option<PathBuf>,
    pub state: Arc<Mutex<JobState>>,
    pub progress: Progress,
}

impl Job {
    /// Start `work` on a background thread. `work` gets the progress handle and a setter for the
    /// lesson folder (call it as soon as the folder exists).
    pub fn spawn(
        id: u64,
        kind: JobKind,
        title: String,
        dir: Option<PathBuf>,
        work: impl FnOnce(&Progress, &dyn Fn(PathBuf)) -> anyhow::Result<()> + Send + 'static,
    ) -> Job {
        let state = Arc::new(Mutex::new(JobState { dir: dir.clone(), ..Default::default() }));
        let s2 = state.clone();
        let progress = Progress::new(move |e| s2.lock().unwrap().apply(e));
        let (p2, s3) = (progress.clone(), state.clone());
        std::thread::Builder::new()
            .name(format!("job-{id}"))
            .spawn(move || {
                let s4 = s3.clone();
                let set_dir = move |d: PathBuf| s4.lock().unwrap().dir = Some(d);
                let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| work(&p2, &set_dir)));
                let mut st = s3.lock().unwrap();
                match res {
                    Ok(Ok(())) => st.close_running(),
                    Ok(Err(e)) => {
                        if p2.cancelled() {
                            st.cancelled = true;
                        } else {
                            st.error = Some(format!("{e:#}"));
                        }
                        if let Some(c) = st.cards.last_mut() {
                            if c.status == StageStatus::Running {
                                c.status = StageStatus::Failed;
                            }
                        }
                    }
                    Err(_) => st.error = Some("the job crashed".into()),
                }
                st.finished = true;
            })
            .expect("spawning job thread");
        let _ = title;
        Job { id, kind, dir, state, progress }
    }

    pub fn dir(&self) -> Option<PathBuf> {
        self.dir.clone()
    }
    pub fn finished(&self) -> bool {
        self.state.lock().unwrap().finished
    }
}
