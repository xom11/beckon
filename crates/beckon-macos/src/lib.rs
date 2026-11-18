//! macOS backend for beckon. Uses NSWorkspace / NSRunningApplication for
//! launch + activation, and the Accessibility (AX) API for per-window cycling.
//! Z-order ("most-recent other app") comes from CGWindowListCopyWindowInfo.
//!
//! Algorithm mirrors `beckon-linux::i3ipc`:
//!   3. not running                 → launch
//!   4. running, not focused        → activate
//!   5a. focused, app has more wins → AX-raise the next window of same app
//!   5b. focused, only one window   → activate the most-recent OTHER app:
//!                                     MRU "previous" first (handles fullscreen
//!                                     apps on another Space), else z-order
//!   5c. nothing else exists        → hide current app (NSRunningApplication.hide)
//!
//! Accessibility permission: required for window-level operations (5a). Without
//! it, focus / launch / hide still work but cycle degrades to "do nothing
//! visible". `beckon doctor` reports state and how to grant.

#[cfg(not(target_os = "macos"))]
use beckon_core::BackendError;
use beckon_core::{Backend, Result};

#[cfg(target_os = "macos")]
mod apps;
#[cfg(target_os = "macos")]
pub mod caps_tap;
#[cfg(target_os = "macos")]
mod ffi;
#[cfg(target_os = "macos")]
pub mod hotkey;
// One `#[cfg]` per `pub mod`, as its own complete pair, and never a `mod`
// slipped between an existing attribute and the item it gates. That is not
// style: `c33fcf6` did exactly that in `beckon-windows/src/lib.rs`, leaving
// `shell` ungated, and every Linux and macOS `nix build` failed E0433 for a
// month while nothing in CI could see it. The step that catches it now —
// `cargo check --workspace --all-targets`, unexcluded — runs on this crate
// too, so a mistake here is caught; the shape below is what keeps it from
// being made.
#[cfg(target_os = "macos")]
pub mod shell;
#[cfg(target_os = "macos")]
mod state;
#[cfg(target_os = "macos")]
pub mod tray;
#[cfg(target_os = "macos")]
mod windows;

#[cfg(target_os = "macos")]
mod backend;

#[cfg(target_os = "macos")]
pub use backend::MacBackend;

#[cfg(target_os = "macos")]
pub fn pick_backend() -> Result<Box<dyn Backend>> {
    Ok(Box::new(MacBackend::new()?))
}

#[cfg(not(target_os = "macos"))]
pub fn pick_backend() -> Result<Box<dyn Backend>> {
    Err(BackendError::UnsupportedEnvironment(
        "beckon-macos only compiles on macOS".to_string(),
    ))
}

/// Ask the window server what it can see, for the probes.
///
/// The honest answer to "did the menu bar item appear?" without a
/// screenshot, and therefore without the Screen Recording grant that has
/// nothing to do with the question. A status item is a real window at a
/// high layer owned by this process; if the window server does not list
/// one, it is not on screen.
///
/// **Use it with the control it makes possible**: other applications' menu
/// bar extras are listed the same way, so "we see theirs but not ours" is a
/// real negative, while "we see nothing at any layer" means the enumeration
/// itself is blind and the run proves nothing.
#[cfg(target_os = "macos")]
pub fn window_server_windows() -> Vec<CgWindow> {
    ffi::cg_windows_all()
}

/// One window as the window server describes it. Re-exported alone rather
/// than by making `ffi` public: that module's surface is raw `unsafe`
/// bindings with hand-written lifetime contracts, and none of it belongs to
/// anyone outside this crate.
#[cfg(target_os = "macos")]
pub use ffi::CgWindow;

/// Whether the current process is trusted for the Accessibility API.
/// Used by `beckon doctor`. Returns `false` on non-macOS.
#[cfg(target_os = "macos")]
pub fn is_accessibility_trusted() -> bool {
    ffi::ax_is_process_trusted()
}

#[cfg(not(target_os = "macos"))]
pub fn is_accessibility_trusted() -> bool {
    false
}

/// Whether the current process has been granted Input Monitoring permission
/// (`IOHIDCheckAccess(kIOHIDRequestTypeListenEvent)`). Required when
/// `keyboard.caps = true` in `apps.toml`. Used by `beckon doctor`.
#[cfg(target_os = "macos")]
pub fn is_input_monitoring_trusted() -> bool {
    caps_tap::input_monitoring_granted()
}

#[cfg(not(target_os = "macos"))]
pub fn is_input_monitoring_trusted() -> bool {
    false
}

/// Ask macOS for Accessibility, raising the system dialog.
///
/// **The counterpart `is_accessibility_trusted` is not.** That one reads a
/// recorded answer; this one asks the question, and it is the only way a
/// process with no TCC row can acquire one. macOS raises the panel only when
/// no answer is recorded and returns the stored verdict silently afterwards,
/// so a caller must offer the settings pane as well rather than instead.
#[cfg(target_os = "macos")]
pub fn request_accessibility() -> bool {
    ffi::ax_is_process_trusted_prompt()
}

#[cfg(not(target_os = "macos"))]
pub fn request_accessibility() -> bool {
    false
}

/// One resolution report per name, for `beckon check --resolve`.
///
/// A batch rather than a loop over `apps::resolve`, because the scans are the
/// expensive half and they are per-call there: `installed_apps()` walks three
/// roots one level deep and reads one `Info.plist` per bundle, and a shortcuts
/// file with eighteen bindings is an ordinary one.
///
/// `running_apps()` is part of the answer because it is tiers 1 and 2 of the
/// ladder — an app running but installed somewhere this scan does not reach
/// still resolves, and resolves *exactly*. `Finder` is the everyday case: its
/// bundle is `/System/Library/CoreServices/Finder.app`, under none of the
/// three roots. That is the one place this answer depends on the session
/// rather than on the disk, and it matches what `beckon resolve` reports.
#[cfg(target_os = "macos")]
pub fn resolve_reports(names: &[&str]) -> Result<Vec<beckon_core::certainty::NameReport>> {
    Ok(apps::resolve_reports(names))
}

/// Returns an error rather than an empty vector: an empty one reads as
/// "every name resolved", which is the one answer this cannot know.
#[cfg(not(target_os = "macos"))]
pub fn resolve_reports(_names: &[&str]) -> Result<Vec<beckon_core::certainty::NameReport>> {
    Err(BackendError::UnsupportedEnvironment(
        "beckon-macos only compiles on macOS".to_string(),
    ))
}

/// The installed-app catalog, **without a backend**, for the same reason
/// `resolve_reports` takes none: `.app` bundles and their `Info.plist` are on
/// disk whether or not there is a window server, and this answer does not
/// mention windows. `MacBackend::list_installed` delegates here.
#[cfg(target_os = "macos")]
pub fn list_installed() -> Result<Vec<beckon_core::InstalledApp>> {
    let mut apps = apps::installed_apps();
    apps.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(apps
        .into_iter()
        .map(|a| beckon_core::InstalledApp {
            id: a.bundle_id,
            name: a.name,
            exec: Some(a.bundle_path.display().to_string()),
        })
        .collect())
}

#[cfg(not(target_os = "macos"))]
pub fn list_installed() -> Result<Vec<beckon_core::InstalledApp>> {
    Err(BackendError::UnsupportedEnvironment(
        "beckon-macos only compiles on macOS".to_string(),
    ))
}

/// Print a `resolve` resolution report for `id` on stdout. Mirrors the Linux
/// `cmd_resolve_linux` shape but uses macOS metadata (running apps + installed
/// .app bundles).
#[cfg(target_os = "macos")]
pub fn print_resolve_report(id: &str) -> Result<()> {
    backend::print_resolve_report(id)
}

#[cfg(not(target_os = "macos"))]
pub fn print_resolve_report(_id: &str) -> Result<()> {
    Err(BackendError::UnsupportedEnvironment(
        "beckon-macos only compiles on macOS".to_string(),
    ))
}
