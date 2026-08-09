//! Single-instance lock per config path. flock-based: the lock dies with
//! the process, so a crashed serve never wedges its successor.
// Used by --serve (macOS, Windows); tests exercise it on every OS.
#![cfg_attr(
    all(not(test), not(any(target_os = "macos", target_os = "windows"))),
    allow(dead_code)
)]

use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};

/// Why `acquire` gave up.
///
/// Two outcomes that read alike on stderr but mean opposite things, so the
/// caller must not have to match on message text to tell them apart:
///
/// * `AlreadyRunning` is the *expected* answer to a liveness probe. The
///   documented way to keep resident mode up (see `examples/`) is a timer —
///   launchd, Task Scheduler, a cron-alike — that simply re-runs the serve
///   command; a dead serve comes back, and a live one refuses right here.
///   That refusal is the healthy signal, not a fault.
/// * `Open` means the lock file itself is unusable — a real fault.
///
/// Both used to go through `main`'s catch-all, which fires a desktop
/// notification whenever stderr is not a terminal — and such a timer
/// redirects stderr to a log, so it never is. The healthy refusal therefore
/// raised one notification per tick, forever. See `is_expected` in `main.rs`.
#[derive(Debug)]
pub enum AcquireError {
    AlreadyRunning(PathBuf),
    Open {
        path: PathBuf,
        source: std::io::Error,
    },
}

impl std::fmt::Display for AcquireError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            // Wording is load-bearing: this is what lands in
            // serve-watchdog.log, the only evidence a watchdog tick leaves.
            Self::AlreadyRunning(path) => write!(
                f,
                "another `beckon --serve` is already running for this config (lock `{}`)",
                path.display()
            ),
            Self::Open { path, source } => {
                write!(f, "cannot open lock file `{}`: {source}", path.display())
            }
        }
    }
}

impl std::error::Error for AcquireError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::AlreadyRunning(_) => None,
            Self::Open { source, .. } => Some(source),
        }
    }
}

fn lock_path(config: &Path) -> PathBuf {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    config.hash(&mut h);
    std::env::temp_dir().join(format!("beckon-serve-{:016x}.lock", h.finish()))
}

pub fn acquire(config: &Path) -> Result<File, AcquireError> {
    use fs4::FileExt;
    // Two spellings of the same file must contend for the same lock, so
    // hash the canonical path. A config that does not exist (yet) cannot
    // be canonicalized — fall back to the raw path; serve fails on the
    // read that follows anyway.
    let canonical = config
        .canonicalize()
        .unwrap_or_else(|_| config.to_path_buf());
    let path = lock_path(&canonical);
    let file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(&path)
        .map_err(|source| AcquireError::Open {
            path: path.clone(),
            source,
        })?;
    file.try_lock_exclusive()
        .map_err(|_| AcquireError::AlreadyRunning(path))?;
    Ok(file)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn second_lock_on_same_config_fails() {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("apps.toml");
        std::fs::write(&config, "").unwrap();
        let _first = acquire(&config).expect("first lock");
        assert!(acquire(&config).is_err(), "second lock must fail");
    }

    /// The caller has to be able to tell "a serve is already up" apart from
    /// a real fault without matching on message text — `main` suppresses the
    /// desktop notification for exactly this variant.
    #[test]
    fn second_lock_reports_already_running_variant() {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("apps.toml");
        std::fs::write(&config, "").unwrap();
        let _first = acquire(&config).expect("first lock");
        match acquire(&config) {
            Err(AcquireError::AlreadyRunning(path)) => {
                assert!(
                    path.file_name()
                        .and_then(|n| n.to_str())
                        .is_some_and(|n| n.starts_with("beckon-serve-")),
                    "variant must carry the lock path, got `{}`",
                    path.display()
                );
            }
            other => panic!("expected AlreadyRunning, got {other:?}"),
        }
    }

    #[test]
    fn lock_released_on_drop() {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("apps.toml");
        std::fs::write(&config, "").unwrap();
        let first = acquire(&config).expect("first lock");
        drop(first);
        acquire(&config).expect("lock must be reacquirable after drop");
    }

    #[test]
    fn different_configs_do_not_contend() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.toml");
        let b = dir.path().join("b.toml");
        std::fs::write(&a, "").unwrap();
        std::fs::write(&b, "").unwrap();
        let _la = acquire(&a).expect("lock a");
        acquire(&b).expect("lock b must not contend with a");
    }

    #[test]
    fn different_spellings_of_same_config_contend() {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("apps.toml");
        std::fs::write(&config, "").unwrap();
        let dotted = dir.path().join(".").join("apps.toml");
        let _first = acquire(&config).expect("first lock");
        assert!(acquire(&dotted).is_err(), "dotted spelling must contend");
    }
}
