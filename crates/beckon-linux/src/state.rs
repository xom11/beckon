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
use std::path::PathBuf;

fn state_path() -> Option<PathBuf> {
    std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .map(|d| d.join("beckon-mru"))
}

/// The `app_id` of the app focused before the most recent beckon action,
/// or `None` if the file is missing / empty / unreadable. Best-effort: any
/// I/O error returns `None` rather than failing the hot path.
pub fn read_previous() -> Option<String> {
    let path = state_path()?;
    let raw = fs::read_to_string(path).ok()?;
    let trimmed = raw.trim().to_string();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

/// Persist `app_id` as the new "previous". Best-effort: any I/O error is
/// ignored — losing MRU is degraded UX, never a fatal error.
///
/// Writes to a sibling `.tmp` file then `rename`s into place so concurrent
/// invocations never see a torn read.
pub fn write_previous(app_id: &str) {
    let Some(path) = state_path() else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let tmp = path.with_extension(format!("tmp.{}", std::process::id()));
    if fs::write(&tmp, app_id).is_ok() {
        if fs::rename(&tmp, &path).is_err() {
            let _ = fs::remove_file(&tmp);
        }
    }
}
