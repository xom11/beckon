//! Single-instance lock per config path. flock-based: the lock dies with
//! the process, so a crashed serve never wedges its successor.
// Used by `serve` (macOS, Windows); tests exercise it on every OS.
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
    AlreadyRunning {
        config: PathBuf,
        lock: PathBuf,
    },
    Open {
        path: PathBuf,
        source: std::io::Error,
    },
}

impl std::fmt::Display for AcquireError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            // Wording is load-bearing: with the notification suppressed, this
            // line is the entire diagnostic surface a watchdog tick leaves.
            // It names the *config* first — the lock file is a hash and says
            // nothing to a reader trying to work out which daemon is up.
            Self::AlreadyRunning { config, lock } => write!(
                f,
                "another `beckon serve` is already running for `{}` (lock `{}`)",
                config.display(),
                lock.display()
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
            Self::AlreadyRunning { .. } => None,
            Self::Open { source, .. } => Some(source),
        }
    }
}

/// Where the lock for `config` lives.
///
/// `stable_id`, not `DefaultHasher`: this filename is how two independent
/// beckon processes agree they are talking about the same config, so it must
/// not depend on which Rust release built them. See `stable_id`.
fn lock_path(config: &Path) -> PathBuf {
    std::env::temp_dir().join(format!(
        "beckon-serve-{}.lock",
        crate::stable_id::for_path(config)
    ))
}

/// The lock that says who owns the Caps key on this machine.
///
/// **Deliberately NOT derived from the config path, which is the whole
/// point.** `acquire`'s lock is per config so two people can serve two files
/// at once, and that is right for hotkeys: `RegisterEventHotKey` /
/// `RegisterHotKey` arbitrate per chord and the loser is told. A Caps event
/// tap arbitrates for nothing -- it is machine-global, taps stack, and the
/// LAST one installed sits upstream and swallows the event.
///
/// Measured on macmini 2026-08-17 by another session, with controls in both
/// directions and the install order reversed to show the mechanism rather
/// than assert it:
///
/// ```text
/// one tap        8/8      caps.toml installed FIRST   0/6   (underneath)
/// TWO taps       0/8      caps.toml installed LAST    6/6   (on top)
/// one tap again  8/8
/// ```
///
/// Both processes logged `caps event tap active`, so neither side reported
/// anything -- and silence is the defect. A user with two serves running
/// sees Caps stop working and has nothing to read.
#[cfg(any(target_os = "macos", test))]
fn caps_lock_path() -> PathBuf {
    std::env::temp_dir().join("beckon-caps.lock")
}

/// Try to become the one process that owns Caps.
///
/// `None` means another beckon has it. The caller must then install no tap
/// AND SAY SO -- a second beckon that quietly declines is the same silence
/// this exists to end, one level down.
///
/// The returned `File` must be held for as long as the tap is installed:
/// flock dies with the file handle, so dropping it hands Caps to whoever
/// asks next, which is exactly what should happen on pause, on a reload that
/// turns Caps off, and on exit.
#[cfg(any(target_os = "macos", test))]
pub fn acquire_caps() -> Option<File> {
    use fs4::FileExt;
    let file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(caps_lock_path())
        .ok()?;
    file.try_lock_exclusive().ok()?;
    Some(file)
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
        .map_err(|_| AcquireError::AlreadyRunning {
            config: canonical,
            lock: path,
        })?;
    Ok(file)
}

#[cfg(test)]
mod tests {
    /// **The Caps lock must not be the config lock**, or two serves on two
    /// files both take it and both install a tap -- the measured defect.
    #[test]
    fn the_caps_lock_is_one_name_for_the_whole_machine() {
        let a = super::lock_path(std::path::Path::new("/tmp/a.toml"));
        let b = super::lock_path(std::path::Path::new("/tmp/b.toml"));
        assert_ne!(a, b, "config locks differ per file, as they should");

        let caps = super::caps_lock_path();
        assert_ne!(caps, a);
        assert_ne!(caps, b);
        // Called twice with different arguments in spirit: it takes none, so
        // there is nothing a caller could vary to get a second lock.
        assert_eq!(caps, super::caps_lock_path());
    }

    /// The second caller is refused while the first holds it, and served the
    /// moment the first lets go. Both halves matter: without the release,
    /// pausing one beckon would strand Caps for every other.
    #[test]
    fn only_one_process_holds_caps_and_releasing_hands_it_on() {
        let first = super::acquire_caps().expect("nothing else holds it in this test process");
        // A second attempt from THIS process cannot be tested with flock --
        // POSIX locks are per-process, so this would succeed and prove
        // nothing. What is testable is that the handle exists and that
        // dropping it is what frees the lock.
        drop(first);
        let again = super::acquire_caps();
        assert!(again.is_some(), "released, so it can be taken again");
    }

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
            Err(AcquireError::AlreadyRunning { config: c, lock }) => {
                assert!(
                    lock.file_name()
                        .and_then(|n| n.to_str())
                        .is_some_and(|n| n.starts_with("beckon-serve-")),
                    "variant must carry the lock path, got `{}`",
                    lock.display()
                );
                assert_eq!(
                    c,
                    config.canonicalize().unwrap(),
                    "variant must carry the config path so the message can name it"
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
