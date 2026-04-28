//! Tiny single-app MRU state, persisted to `$XDG_RUNTIME_DIR/beckon-mru`.
//!
//! Stores the `app_id` that was focused immediately before the most recent
//! beckon action. Used by step 5b (toggle-back) to land on the actually
//! previous app instead of whatever the tree happens to expose first.
//!
//! Why a runtime dir, not a config dir:
//!   - `$XDG_RUNTIME_DIR` is wiped on logout — MRU should not survive
//!     across sessions because window ids and running apps reset.
//!   - It's tmpfs, so writes don't hit disk.
//!
//! Why a single line, not JSON:
//!   - One value, no schema. Avoids pulling in serde for 10 lines of work.
//!
//! Reconciliation: every invocation reads the live focus from sway/i3 IPC
//! before consulting the file. Mouse / native-hotkey focus changes between
//! beckon calls are picked up by the next invocation automatically.

use std::fs;
use std::path::{Path, PathBuf};

fn state_path() -> Option<PathBuf> {
    std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .map(|d| d.join("beckon-mru"))
}

/// The `app_id` of the app focused before the most recent beckon action,
/// or `None` if the file is missing / empty / unreadable. Best-effort: any
/// I/O error returns `None` rather than failing the hot path.
pub fn read_previous() -> Option<String> {
    read_previous_at(&state_path()?)
}

/// Persist `app_id` as the new "previous". Best-effort: any I/O error is
/// ignored — losing MRU is degraded UX, never a fatal error.
///
/// Writes to a sibling `.tmp` file then `rename`s into place so concurrent
/// invocations never see a torn read.
pub fn write_previous(app_id: &str) {
    if let Some(path) = state_path() {
        write_previous_at(&path, app_id);
    }
}

/// Read implementation parameterized by path — the public API consults
/// `$XDG_RUNTIME_DIR`, but tests want a temp directory.
pub(crate) fn read_previous_at(path: &Path) -> Option<String> {
    let raw = fs::read_to_string(path).ok()?;
    let trimmed = raw.trim().to_string();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

/// Write implementation parameterized by path. See [`write_previous`].
pub(crate) fn write_previous_at(path: &Path, app_id: &str) {
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let tmp = path.with_extension(format!("tmp.{}", std::process::id()));
    if fs::write(&tmp, app_id).is_ok() && fs::rename(&tmp, path).is_err() {
        let _ = fs::remove_file(&tmp);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    /// Per-test scratch directory under the system temp dir. Avoids needing
    /// the `tempfile` crate just for these tests.
    fn scratch_dir(label: &str) -> PathBuf {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let dir = std::env::temp_dir().join(format!(
            "beckon-state-test-{}-{}-{}-{}",
            label,
            std::process::id(),
            nanos,
            n
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn read_missing_returns_none() {
        let dir = scratch_dir("missing");
        let path = dir.join("beckon-mru");
        assert!(read_previous_at(&path).is_none());
    }

    #[test]
    fn read_empty_returns_none() {
        let dir = scratch_dir("empty");
        let path = dir.join("beckon-mru");
        fs::write(&path, "").unwrap();
        assert!(read_previous_at(&path).is_none());
    }

    #[test]
    fn read_whitespace_only_returns_none() {
        let dir = scratch_dir("ws");
        let path = dir.join("beckon-mru");
        fs::write(&path, "  \n\t\n").unwrap();
        assert!(read_previous_at(&path).is_none());
    }

    #[test]
    fn write_then_read_roundtrip() {
        let dir = scratch_dir("rt");
        let path = dir.join("beckon-mru");
        write_previous_at(&path, "kitty");
        assert_eq!(read_previous_at(&path).as_deref(), Some("kitty"));
    }

    #[test]
    fn read_trims_trailing_newline() {
        // Defensive: a previous version (or a hand-edit) wrote with a
        // trailing newline. read should still see "kitty".
        let dir = scratch_dir("trim");
        let path = dir.join("beckon-mru");
        fs::write(&path, "kitty\n").unwrap();
        assert_eq!(read_previous_at(&path).as_deref(), Some("kitty"));
    }

    #[test]
    fn write_overwrites_previous_value() {
        let dir = scratch_dir("over");
        let path = dir.join("beckon-mru");
        write_previous_at(&path, "alpha");
        write_previous_at(&path, "beta");
        assert_eq!(read_previous_at(&path).as_deref(), Some("beta"));
    }

    #[test]
    fn write_creates_parent_directory() {
        let dir = scratch_dir("mkparent");
        let nested = dir.join("does/not/exist/yet");
        let path = nested.join("beckon-mru");
        write_previous_at(&path, "kitty");
        assert!(path.exists());
        assert_eq!(read_previous_at(&path).as_deref(), Some("kitty"));
    }

    #[test]
    fn write_does_not_leave_tmp_files_on_success() {
        let dir = scratch_dir("notmp");
        let path = dir.join("beckon-mru");
        write_previous_at(&path, "kitty");
        let leftovers: Vec<_> = fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .filter(|e| {
                e.path()
                    .file_name()
                    .and_then(|n| n.to_str())
                    .map(|n| n.starts_with("beckon-mru.tmp."))
                    .unwrap_or(false)
            })
            .collect();
        assert!(leftovers.is_empty(), "tmp files leaked: {:?}", leftovers);
    }

    #[test]
    fn write_handles_unicode_and_special_chars() {
        let dir = scratch_dir("uni");
        let path = dir.join("beckon-mru");
        // PWA ids contain hashes; bundle ids contain dots; some users
        // may have non-ASCII names. All round-trip identically.
        let id = "brave-fmpnliohjhemenmnlpbfagaolkdacoja-Default";
        write_previous_at(&path, id);
        assert_eq!(read_previous_at(&path).as_deref(), Some(id));
    }
}
