//! Progress reporting for the pipeline. The CLI prints it; the desktop app turns it into
//! stage cards and progress bars. Long work checks `cancelled()` between steps.

use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum Stage {
    Reading,
    Vision,
    Script,
    Voices,
    Render,
}

impl Stage {
    pub const ALL: [Stage; 5] = [Stage::Reading, Stage::Vision, Stage::Script, Stage::Voices, Stage::Render];

    pub fn label(self) -> &'static str {
        match self {
            Stage::Reading => "Reading your material",
            Stage::Vision => "Reading diagrams and scans",
            Stage::Script => "Writing the script",
            Stage::Voices => "Recording voices",
            Stage::Render => "Rendering the video",
        }
    }

    fn emoji(self) -> &'static str {
        match self {
            Stage::Reading => "📚",
            Stage::Vision => "👀",
            Stage::Script => "✍️ ",
            Stage::Voices => "🗣 ",
            Stage::Render => "🎬",
        }
    }
}

#[derive(Debug, Clone)]
pub enum Event {
    /// A stage begins. `msg` says what's happening.
    Start(Stage, String),
    /// A detail line for the current stage.
    Info(Stage, String),
    /// Fractional progress inside the current stage (0..=1).
    Frac(Stage, String, f32),
    /// The stage finished.
    Done(Stage),
}

type Sink = dyn Fn(Event) + Send + Sync;

#[derive(Clone)]
pub struct Progress {
    sink: Arc<Sink>,
    stage: Arc<Mutex<Stage>>,
    cancel: Arc<AtomicBool>,
}

impl Progress {
    pub fn new(sink: impl Fn(Event) + Send + Sync + 'static) -> Self {
        Progress { sink: Arc::new(sink), stage: Arc::new(Mutex::new(Stage::Reading)), cancel: Arc::new(AtomicBool::new(false)) }
    }

    /// Prints to stderr (the CLI).
    pub fn cli() -> Self {
        let in_frac = Arc::new(AtomicBool::new(false));
        Progress::new(move |e| {
            let nl = |f: &AtomicBool| {
                if f.swap(false, Ordering::Relaxed) {
                    eprintln!();
                }
            };
            match e {
                Event::Start(s, m) => {
                    nl(&in_frac);
                    eprintln!("{} {m}", s.emoji());
                }
                Event::Info(_, m) => {
                    nl(&in_frac);
                    eprintln!("  {m}");
                }
                Event::Frac(_, m, _) => {
                    in_frac.store(true, Ordering::Relaxed);
                    eprint!("\r  {m}   ");
                    let _ = std::io::stderr().flush();
                }
                Event::Done(_) => nl(&in_frac),
            }
        })
    }

    /// Discards everything.
    pub fn silent() -> Self {
        Progress::new(|_| {})
    }

    pub fn start(&self, s: Stage, msg: impl Into<String>) {
        *self.stage.lock().unwrap() = s;
        (self.sink)(Event::Start(s, msg.into()));
    }
    pub fn info(&self, msg: impl Into<String>) {
        (self.sink)(Event::Info(self.stage(), msg.into()));
    }
    pub fn frac(&self, msg: impl Into<String>, f: f32) {
        (self.sink)(Event::Frac(self.stage(), msg.into(), f.clamp(0.0, 1.0)));
    }
    pub fn done(&self) {
        (self.sink)(Event::Done(self.stage()));
    }
    pub fn stage(&self) -> Stage {
        *self.stage.lock().unwrap()
    }

    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
    pub fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }
    /// Err if the job was cancelled.
    pub fn check(&self) -> anyhow::Result<()> {
        if self.cancelled() {
            anyhow::bail!("cancelled");
        }
        Ok(())
    }
}
