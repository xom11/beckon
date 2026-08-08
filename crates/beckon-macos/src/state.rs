//! Tiny single-app MRU state, persisted to `$TMPDIR/beckon-mru`.
//!
//! Stores the **bundle id** of the app that was frontmost immediately before
//! the most recent beckon action. Used by step 5b (toggle-back) to land on the
//! app the user actually came from.
//!
//! Why this exists on macOS specifically:
//!   - Step 5b's other source of truth is `CGWindowListCopyWindowInfo` with
//!     `kCGWindowListOptionOnScreenOnly`, which only sees the *active* Space.
//!   - A natively-fullscreen app lives on its own Space, so it vanishes from
//!     that list — toggle-back would then skip past it to the wrong app.
//!   - `NSWorkspace.frontmostApplication` reports the frontmost app regardless
//!     of Space, so recording its bundle id here captures fullscreen apps the
//!     z-order stack can't. Mirrors `beckon-linux::state`.
//!
//! Why `$TMPDIR`, not a config/cache dir:
//!   - macOS gives each user a private `$TMPDIR` (`/var/folders/.../T/`) that
//!     is periodically reaped — MRU should not persist forever because running
//!     apps reset across sessions. Equivalent to Linux's `$XDG_RUNTIME_DIR`.
//!   - `std::env::temp_dir()` always returns a path (falls back to `/tmp`), and
//!     every operation here is best-effort, so a missing dir never fails the
//!     hot path.
//!
//! Why a single line, not JSON:
//!   - One value, no schema. Avoids pulling in serde for 10 lines of work.

use std::fs;
use std::path::{Path, PathBuf};

fn state_path() -> PathBuf {
    std::env::temp_dir().join("beckon-mru")
}

/// The bundle id of the app frontmost before the most recent beckon action,
/// or `None` if the file is missing / empty / unreadable. Best-effort: any
/// I/O error returns `None` rather than failing the hot path.
pub fn read_previous() -> Option<String> {
    read_previous_at(&state_path())
}

/// Persist `bundle_id` as the new "previous". Best-effort: any I/O error is
/// ignored — losing MRU is degraded UX, never a fatal error.
///
/// Writes to a sibling `.tmp` file then `rename`s into place so concurrent
/// invocations never see a torn read.
pub fn write_previous(bundle_id: &str) {
    write_previous_at(&state_path(), bundle_id);
}

/// Read implementation parameterized by path — the public API consults
/// `$TMPDIR`, but tests want a temp directory.
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
pub(crate) fn write_previous_at(path: &Path, bundle_id: &str) {
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let tmp = path.with_extension(format!("tmp.{}", std::process::id()));
    if fs::write(&tmp, bundle_id).is_ok() && fs::rename(&tmp, path).is_err() {
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
            "beckon-macos-state-test-{}-{}-{}-{}",
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
        write_previous_at(&path, "com.apple.Safari");
        assert_eq!(read_previous_at(&path).as_deref(), Some("com.apple.Safari"));
    }

    #[test]
    fn read_trims_trailing_newline() {
        let dir = scratch_dir("trim");
        let path = dir.join("beckon-mru");
        fs::write(&path, "com.apple.Safari\n").unwrap();
        assert_eq!(read_previous_at(&path).as_deref(), Some("com.apple.Safari"));
    }

    #[test]
    fn write_overwrites_previous_value() {
        let dir = scratch_dir("over");
        let path = dir.join("beckon-mru");
        write_previous_at(&path, "com.alpha");
        write_previous_at(&path, "com.beta");
        assert_eq!(read_previous_at(&path).as_deref(), Some("com.beta"));
    }

    #[test]
    fn write_creates_parent_directory() {
        let dir = scratch_dir("mkparent");
        let nested = dir.join("does/not/exist/yet");
        let path = nested.join("beckon-mru");
        write_previous_at(&path, "com.x.kitty");
        assert!(path.exists());
        assert_eq!(read_previous_at(&path).as_deref(), Some("com.x.kitty"));
    }

    #[test]
    fn write_does_not_leave_tmp_files_on_success() {
        let dir = scratch_dir("notmp");
        let path = dir.join("beckon-mru");
        write_previous_at(&path, "com.x.kitty");
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
        assert!(leftovers.is_empty(), "tmp files leaked: {leftovers:?}");
    }

    #[test]
    fn write_handles_pwa_and_dotted_ids() {
        let dir = scratch_dir("ids");
        let path = dir.join("beckon-mru");
        // Brave PWA ids contain hashes; bundle ids contain dots. Both
        // round-trip identically.
        let id = "brave-fmpnliohjhemenmnlpbfagaolkdacoja-Default";
        write_previous_at(&path, id);
        assert_eq!(read_previous_at(&path).as_deref(), Some(id));
    }
}
