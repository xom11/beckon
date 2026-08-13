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
mod ffi;
#[cfg(target_os = "macos")]
pub mod hotkey;
#[cfg(target_os = "macos")]
pub mod settings_window;
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

/// Installed-app display names, for the settings window's App field.
///
/// Names only: the window is filling in a Name while someone authors a
/// binding, which is the job `beckon search` already has. It never focuses
/// or launches anything, so it has no use for a bundle id or a path.
#[cfg(target_os = "macos")]
pub fn installed_app_names() -> Vec<String> {
    let mut v: Vec<String> = apps::installed_apps().into_iter().map(|a| a.name).collect();
    v.sort();
    v.dedup();
    v
}

#[cfg(not(target_os = "macos"))]
pub fn installed_app_names() -> Vec<String> {
    Vec::new()
}

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
