use thiserror::Error;

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
