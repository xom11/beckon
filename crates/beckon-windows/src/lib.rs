//! Windows backend for beckon. Uses Win32 API for window management,
//! COM IShellLinkW for Start Menu shortcuts, and AppX registration data
//! for MSIX/Store apps.
//!
//! Algorithm mirrors `beckon-macos` / `beckon-linux::i3ipc`:
//!   3. not running                 -> launch via ShellExecuteW
//!   4. running, not focused        -> SetForegroundWindow (with anti-focus-stealing)
//!   5a. focused, app has more wins -> cycle to next window of same app
//!   5b. focused, only one window   -> focus most-recent OTHER app via z-order
//!   5c. nothing else exists        -> minimize current window
//!
//! Z-order from EnumWindows is front-to-back (MRU) -> no state file needed.
//!
//! **Every `mod` below must carry `#[cfg(target_os = "windows")]`, without
//! exception.** The `windows` crate is a target-gated dependency, so off
//! Windows the name `windows` does not exist and any module that reaches for
//! it fails to resolve. This crate is a plain, unconditional workspace
//! member, and a bare `cargo build` at the workspace root builds every member
//! on every OS -- that is exactly what `rustPlatform.buildRustPackage` does,
//! so a missing guard here breaks `nix build` on Linux and takes NixOS hosts
//! down with it. `beckon-cli`'s target-gated dependency on this crate does
//! not help: workspace membership, not the dependency edge, is what pulls it
//! into the build.
//!
//! CI cannot be relied on to notice. `.github/workflows/ci.yml` passes
//! `--exclude beckon-windows` on the Linux and macOS jobs, so the only thing
//! that compiles this crate off Windows is the extra unexcluded workspace
//! check that job runs -- keep it.

#[cfg(not(target_os = "windows"))]
use beckon_core::BackendError;
use beckon_core::{Backend, Result};

#[cfg(target_os = "windows")]
pub mod apps;
#[cfg(target_os = "windows")]
pub mod autostart;
#[cfg(target_os = "windows")]
mod backend;
#[cfg(target_os = "windows")]
pub mod caps_hook;
#[cfg(target_os = "windows")]
pub mod hotkey;
#[cfg(target_os = "windows")]
pub mod logfile;
#[cfg(target_os = "windows")]
pub mod settings_window;
#[cfg(target_os = "windows")]
pub mod shell;
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

/// `beckon resolve <id>` report on Windows.
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
