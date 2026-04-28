//! Windows backend for beckon. Uses Win32 API for window management and
//! COM IShellLinkW for Start Menu shortcut (.lnk) parsing.
//!
//! Algorithm mirrors `beckon-macos` / `beckon-linux::i3ipc`:
//!   3. not running                 -> launch via ShellExecuteW
//!   4. running, not focused        -> SetForegroundWindow (with anti-focus-stealing)
//!   5a. focused, app has more wins -> cycle to next window of same app
//!   5b. focused, only one window   -> focus most-recent OTHER app via z-order
//!   5c. nothing else exists        -> minimize current window
//!
//! Z-order from EnumWindows is front-to-back (MRU) -> no state file needed.

#[cfg(not(target_os = "windows"))]
use beckon_core::BackendError;
use beckon_core::{Backend, Result};

#[cfg(target_os = "windows")]
pub mod apps;
#[cfg(target_os = "windows")]
mod backend;
#[cfg(target_os = "windows")]
pub mod window_ops;

#[cfg(target_os = "windows")]
pub use backend::WindowsBackend;

#[cfg(target_os = "windows")]
pub fn pick_backend() -> Result<Box<dyn Backend>> {
    Ok(Box::new(WindowsBackend))
}

#[cfg(not(target_os = "windows"))]
pub fn pick_backend() -> Result<Box<dyn Backend>> {
    Err(BackendError::UnsupportedEnvironment(
        "beckon-windows only runs on Windows".to_string(),
    ))
}

/// `beckon -r <id>` report on Windows.
#[cfg(target_os = "windows")]
pub fn print_resolve_report(id: &str) -> Result<()> {
    backend::print_resolve_report(id)
}

#[cfg(not(target_os = "windows"))]
pub fn print_resolve_report(_id: &str) -> Result<()> {
    Err(BackendError::UnsupportedEnvironment(
        "beckon-windows only runs on Windows".to_string(),
    ))
}
