use std::sync::atomic::{AtomicBool, Ordering};
use thiserror::Error;

/// Process-wide verbose flag. Backends consult `verbose()` from any thread
/// to decide whether to surface "soft" failures (e.g. SetForegroundWindow
/// returning false, AX permission missing) that the algorithm tolerates
/// but the user often wants to see during debugging.
static VERBOSE: AtomicBool = AtomicBool::new(false);

pub fn set_verbose(v: bool) {
    VERBOSE.store(v, Ordering::Relaxed);
}

pub fn verbose() -> bool {
    VERBOSE.load(Ordering::Relaxed)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WindowId(pub i64);

#[derive(Debug, Clone)]
pub struct RunningApp {
    pub id: String,
    pub name: String,
    pub window_count: usize,
}

#[derive(Debug, Clone)]
pub struct InstalledApp {
    pub id: String,
    pub name: String,
    pub exec: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BeckonAction {
    Launched,
    Focused,
    Cycled,
    ToggledBack,
    Hidden,
}

#[derive(Debug, Error)]
pub enum BackendError {
    #[error("not running on a supported display server: {0}")]
    UnsupportedEnvironment(String),

    #[error("IPC connection failed: {0}")]
    Ipc(String),

    #[error("window not found for id `{0}`")]
    WindowNotFound(String),

    #[error("failed to launch `{id}`: {reason}")]
    LaunchFailed { id: String, reason: String },

    /// Nothing on THIS MACHINE answers to this id: no installed-app entry
    /// matched it, and no running window carries it as a class either.
    ///
    /// Split out of [`LaunchFailed`] so a candidate chain can step over it.
    /// The distinction the ladder needs is "this id does not exist here",
    /// which is recoverable by trying the next candidate, against "the
    /// compositor refused / IPC broke", which is not and must abort the whole
    /// press rather than silently retrying against a broken connection.
    ///
    /// **Every backend already computed this predicate**; it was spelled as a
    /// `LaunchFailed` because there was nothing else to do with it. Note where
    /// the site sits on Linux: INSIDE the `Decision::Launch` arm, so it is
    /// reached only when the window tree ALSO holds no window of that class.
    /// That is what lets a running ad-hoc app with no `.desktop` file win its
    /// rung -- a ladder built on `Certainty` instead would grade it `NoMatch`
    /// and skip past a window that is right there on screen.
    #[error("no app matches `{id}`{}", if .hint.is_empty() { String::new() } else { format!(": {}", .hint) })]
    NoMatch { id: String, hint: String },

    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, BackendError>;

pub trait Backend {
    fn list_running(&self) -> Result<Vec<RunningApp>>;
    fn list_installed(&self) -> Result<Vec<InstalledApp>>;

    /// Single entry point — implements the full algorithm:
    /// launch / focus / cycle-same-app / toggle-other-app / hide.
    fn beckon(&self, id: &str) -> Result<BeckonAction>;
}

pub mod candidates;
pub mod caps;
pub mod capture;
pub mod certainty;
pub mod config_write;
pub mod menu;
pub mod page_plan;
pub mod paths;
pub mod settings;
pub mod shortcuts;
pub mod theme;
