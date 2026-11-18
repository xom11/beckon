//! Hand a file to LaunchServices via `/usr/bin/open`, the way
//! `beckon_windows::shell` hands one to Explorer.
//!
//! Through `/usr/bin/open` rather than `NSWorkspace` directly: `NSWorkspace`'s
//! modern equivalents are `@MainActor` and completion-handler-only, whereas
//! `open` returns in ~10-20 ms and is only reached when a person clicks a menu
//! item (`Open Config` / `Open Log`).

use std::path::Path;

/// Open a file with whatever the user has associated with it.
pub fn open_path(p: &Path) -> Result<(), String> {
    std::process::Command::new("/usr/bin/open")
        .arg(p.as_os_str())
        .status()
        .map_err(|e| format!("cannot open {}: {e}", p.display()))
        .and_then(|s| {
            if s.success() {
                Ok(())
            } else {
                Err(format!("open {} exited {s}", p.display()))
            }
        })
}
