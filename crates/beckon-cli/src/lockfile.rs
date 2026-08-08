//! Single-instance lock per config path. flock-based: the lock dies with
//! the process, so a crashed serve never wedges its successor.
// Used by --serve (macOS, Windows); tests exercise it on every OS.
#![cfg_attr(
    all(not(test), not(any(target_os = "macos", target_os = "windows"))),
    allow(dead_code)
)]

use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};

fn lock_path(config: &Path) -> PathBuf {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    config.hash(&mut h);
    std::env::temp_dir().join(format!("beckon-serve-{:016x}.lock", h.finish()))
}

pub fn acquire(config: &Path) -> Result<File, String> {
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
        .map_err(|e| format!("cannot open lock file `{}`: {e}", path.display()))?;
    file.try_lock_exclusive().map_err(|_| {
        format!(
            "another `beckon --serve` is already running for this config (lock `{}`)",
            path.display()
        )
    })?;
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
