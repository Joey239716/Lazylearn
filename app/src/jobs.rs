//! Background jobs (make / re-render / re-voice). Each runs on its own thread; the
//! pipeline's progress events are collected into a shared `JobState` that the UI reads.

use std::collections::BTreeMap;
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
    /// course builds: each video's own stage cards, by 1-based number (videos overlap:
    /// one can be rendering while the next is voiced and a third is scripted)
    pub videos: BTreeMap<usize, VideoTrack>,
    pub video_total: usize,
}

#[derive(Debug, Default, Clone)]
pub struct VideoTrack {
    pub cards: Vec<StageCard>,
    pub done: bool,
}

impl VideoTrack {
    pub fn running(&self) -> Option<&StageCard> {
        self.cards.iter().rev().find(|c| c.status == StageStatus::Running)
    }
    fn close_running(&mut self) {
        if let Some(c) = self.cards.last_mut() {
            if c.status == StageStatus::Running {
                c.status = StageStatus::Done;
                c.elapsed = Some(c.started.elapsed().as_secs_f32());
            }
        }
    }
    /// 0..1 for this video (script → voices → render)
    pub fn fraction(&self) -> f32 {
        if self.done {
            return 1.0;
        }
        let w = [(Stage::Script, 0.4), (Stage::Voices, 0.25), (Stage::Render, 0.35)];
        w.iter()
            .map(|(s, wt)| {
                wt * match self.cards.iter().rev().find(|c| c.stage == *s) {
                    Some(c) if c.status == StageStatus::Running => c.frac.as_ref().map(|f| f.1).unwrap_or(0.2),
                    Some(_) => 1.0,
                    None => 0.0,
                }
            })
            .sum()
    }
}

/// "[2/15] Recording voices" → (2, 15, "Recording voices")
fn split_tag(msg: &str) -> Option<(usize, usize, String)> {
    let rest = msg.strip_prefix('[')?;
    let (tag, body) = rest.split_once("] ")?;
    let (a, b) = tag.split_once('/')?;
    Some((a.trim().parse().ok()?, b.trim().parse().ok()?, body.to_string()))
}

fn clean(msg: &str) -> String {
    // the CLI's emoji prefixes look out of place in the monospace detail lines
    msg.trim_start_matches(|c: char| !c.is_alphanumeric() && c != '"' && c != '(' && c != '[').to_string()
}

fn new_card(stage: Stage, title: String) -> StageCard {
    StageCard {
        stage,
        title,
        details: vec![],
        frac: None,
        status: StageStatus::Running,
        started: std::time::Instant::now(),
        elapsed: None,
    }
}


impl JobState {
    fn apply(&mut self, e: Event) {
        // per-video events of a course build carry an "[i/N] " tag
        let tagged = match &e {
            Event::Start(_, m) | Event::Info(_, m) | Event::Frac(_, m, _) => split_tag(m),
            Event::Done(_) => None,
        };
        if let Some((i, n, body)) = tagged {
            self.video_total = self.video_total.max(n);
            let t = self.videos.entry(i).or_default();
            match e {
                Event::Start(stage, _) => {
                    t.close_running();
                    t.cards.push(new_card(stage, clean(&body)));
                }
                Event::Info(_, _) => {
                    let body = clean(&body);
                    if body == "done" || body.ends_with(" done") {
                        t.close_running();
                        t.done = true;
                    } else if let Some(c) = t.cards.last_mut() {
                        c.details.push(body);
                        if c.details.len() > 40 {
                            c.details.remove(0);
                        }
                    }
                }
                Event::Frac(_, _, f) => {
                    if let Some(c) = t.cards.last_mut() {
                        c.frac = Some((clean(&body), f));
                    }
                }
                Event::Done(_) => {}
            }
            return;
        }
        match e {
            Event::Start(stage, msg) => {
                // a stage can repeat (e.g. horizontal + vertical renders): close the old one
                self.close_running();
                self.cards.push(new_card(stage, msg));
            }
            Event::Info(_, msg) => {
                // "Video 2/5: title" / "Video 2/5 "title" is already made" (pipeline::build_course)
                if let Some(rest) = msg.strip_prefix("Video ") {
                    let num: String = rest.chars().take_while(|c| c.is_ascii_digit() || *c == '/').collect();
                    if let Some((a, b)) = num.split_once('/') {
                        if let (Ok(a), Ok(b)) = (a.parse::<usize>(), b.parse::<usize>()) {
                            self.video_total = self.video_total.max(b);
                            let t = self.videos.entry(a).or_default();
                            if msg.contains("already made") {
                                t.done = true;
                            }
                            return;
                        }
                    }
                }
                let msg = clean(&msg);
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
            Event::Done(stage) => {
                // untagged: close that stage wherever it's running (one video per stage at a time)
                if let Some(t) = self.videos.values_mut().find(|t| t.running().map(|c| c.stage) == Some(stage)) {
                    t.close_running();
                } else {
                    self.close_running();
                }
            }
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
            if self.video_total == 0 {
                return 0.0;
            }
            let sum: f32 = self.videos.values().map(|t| t.fraction()).sum();
            return (sum / self.video_total as f32).clamp(0.0, 1.0);
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
        self.cards
            .iter()
            .rev()
            .find(|c| c.status == StageStatus::Running)
            .or_else(|| self.videos.values().find_map(|t| t.running()))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn course_events_are_grouped_per_video() {
        let mut st = JobState::default();
        let ev = |st: &mut JobState, e| st.apply(e);
        ev(&mut st, Event::Info(Stage::Script, "Video 1/3: Intro".into()));
        ev(&mut st, Event::Start(Stage::Script, "[1/3] Gemini is writing the script".into()));
        ev(&mut st, Event::Done(Stage::Script));
        ev(&mut st, Event::Start(Stage::Voices, "[1/3] 🗣 Recording voices".into()));
        ev(&mut st, Event::Info(Stage::Script, "Video 2/3: Terms".into()));
        ev(&mut st, Event::Start(Stage::Script, "[2/3] Gemini is writing the script".into()));
        ev(&mut st, Event::Frac(Stage::Voices, "[1/3] voicing line 3/10".into(), 0.3));
        ev(&mut st, Event::Done(Stage::Voices));
        ev(&mut st, Event::Start(Stage::Render, "[1/3] Rendering video.mp4".into()));
        ev(&mut st, Event::Info(Stage::Render, "[1/3] ✅ done".into()));
        ev(&mut st, Event::Info(Stage::Script, "Video 3/3 \"Cram\" is already made".into()));

        assert_eq!(st.video_total, 3);
        assert!(st.videos[&1].done);
        assert!(st.videos[&3].done);
        let v2 = &st.videos[&2];
        assert!(!v2.done);
        assert_eq!(v2.running().map(|c| c.stage), Some(Stage::Script));
        assert_eq!(st.videos[&1].cards[1].title, "Recording voices");
        assert!(st.cards.is_empty(), "tagged events must not land in the job's main cards");
        let f = st.overall(JobKind::Course);
        assert!(f > 0.66 && f < 0.9, "overall {f}");
    }
}
