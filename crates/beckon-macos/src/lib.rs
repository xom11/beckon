//! macOS backend for beckon. Uses NSWorkspace / NSRunningApplication for
//! launch + activation, and the Accessibility (AX) API for per-window cycling.
//! Z-order ("most-recent other app") comes from CGWindowListCopyWindowInfo.
//!
//! Algorithm mirrors `beckon-linux::i3ipc`:
//!   3. not running                 → launch
//!   4. running, not focused        → activate
//!   5a. focused, app has more wins → AX-raise the next window of same app
//!   5b. focused, only one window   → activate the most-recent OTHER app via z-order
//!   5c. nothing else exists        → hide current app (NSRunningApplication.hide)
//!
//! Accessibility permission: required for window-level operations (5a). Without
//! it, focus / launch / hide still work but cycle degrades to "do nothing
//! visible". `beckon -d` reports state and how to grant.

use beckon_core::{Backend, Result};
#[cfg(not(target_os = "macos"))]
use beckon_core::BackendError;

#[cfg(target_os = "macos")]
mod apps;
#[cfg(target_os = "macos")]
mod ffi;
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

/// Whether the current process is trusted for the Accessibility API.
/// Used by `beckon -d`. Returns `false` on non-macOS.
#[cfg(target_os = "macos")]
pub fn is_accessibility_trusted() -> bool {
    ffi::ax_is_process_trusted()
}

#[cfg(not(target_os = "macos"))]
pub fn is_accessibility_trusted() -> bool {
    false
}

/// Print a `-r` resolution report for `id` on stdout. Mirrors the Linux
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
