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
//! on every OS. That used to be exactly what `rustPlatform.buildRustPackage`
//! did, which is how a missing guard here broke `nix build` on Linux from
//! v0.8.0 to v0.9.3 and took NixOS hosts down with it. `nix/package.nix` now
//! passes `-p beckon-cli --bin beckon`, so that particular alarm will not
//! ring again -- which makes the guard *more* important, not less: nothing
//! outside CI compiles this crate off Windows any more.
//! `beckon-cli`'s target-gated dependency on this crate does not help
//! either way: workspace membership, not the dependency edge, is what pulls
//! it into a root build.
//!
//! CI cannot be relied on to notice. `.github/workflows/ci.yml` passes
//! `--exclude beckon-windows` on the Linux and macOS jobs, so the only thing
//! that compiles this crate off Windows is the extra unexcluded workspace
//! check that job runs -- keep it.

#[cfg(not(target_os = "windows"))]
use beckon_core::BackendError;
use beckon_core::{Backend, Result};

// Every module below MUST carry the cfg: the `windows` crate is itself a
// `cfg(target_os = "windows")` dependency (see Cargo.toml), so any module
// that names it fails to compile off Windows with E0433. `shell` lost its
// cfg in c33fcf6, which inserted `pub mod settings_window;` and a blank
// line between the existing attribute and `pub mod shell;` -- so every
// release from v0.8.0 on (`git tag --contains c33fcf6`) could not be built
// off Windows, and `nix build .#beckon` was the only consumer that noticed:
// the CI matrix passes `--exclude beckon-windows` on the Linux and macOS
// jobs, and `release.yml` already built `-p beckon-cli`.
//
// The `nix` CI job does NOT re-cover this, and must not be trusted to:
// `nix/package.nix` now passes `-p beckon-cli --bin beckon`, so nix no
// longer compiles this crate off Windows at all. The `the whole workspace
// still compiles, unexcluded` step in CI's build matrix is what covers it,
// and it is now the only thing that does; locally,
// `cargo check -p beckon-windows` is the same check in one step.
#[cfg(target_os = "windows")]
pub mod apps;
#[cfg(target_os = "windows")]
pub mod autostart;
#[cfg(target_os = "windows")]
mod backend;
#[cfg(target_os = "windows")]
pub mod caps_hook;
/// Put text on the clipboard -- the About page's three copy buttons, and
/// nothing else. See its module header for why it sits beside `shell.rs`.
#[cfg(target_os = "windows")]
pub mod clipboard;
#[cfg(target_os = "windows")]
pub mod hotkey;
#[cfg(target_os = "windows")]
pub mod logfile;
/// The settings window's own look, in `HKCU\Software\beckon` -- the one file
/// beckon writes that is not the shortcuts TOML. See its module header for
/// why it is a separate store from `apps.toml`.
#[cfg(target_os = "windows")]
pub mod prefs;
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

/// One resolution report per name, for `beckon check --resolve`.
///
/// A batch rather than a loop, and deliberately over the full
/// `scan_installed_apps()` rather than `resolve_lazy`: the AppsFolder half of
/// that scan costs several hundred milliseconds, so paying it eighteen times
/// for an eighteen-binding file is the thing to avoid, while paying it once
/// buys the same completeness `installed` / `resolve` are given.
#[cfg(target_os = "windows")]
pub fn resolve_reports(names: &[&str]) -> Result<Vec<beckon_core::certainty::NameReport>> {
    Ok(apps::resolve_reports(names))
}

/// Returns an error rather than an empty vector: an empty one reads as
/// "every name resolved", which is the one answer this cannot know.
#[cfg(not(target_os = "windows"))]
pub fn resolve_reports(_names: &[&str]) -> Result<Vec<beckon_core::certainty::NameReport>> {
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
