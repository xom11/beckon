//! Linux backend dispatcher: detects compositor/DE via env vars at runtime
//! and returns the appropriate Backend implementation.
//!
//! sway and i3 share the same IPC protocol — both go through `i3ipc::I3IpcBackend`,
//! distinguished only by which socket env var is set. X11 generic (any other DE)
//! is handled by `x11::X11Backend` via EWMH.

use beckon_core::{Backend, BackendError, Result};

#[cfg(target_os = "linux")]
pub mod algorithm;

#[cfg(target_os = "linux")]
pub mod desktop;

#[cfg(target_os = "linux")]
pub mod state;

#[cfg(target_os = "linux")]
pub mod i3ipc;

#[cfg(target_os = "linux")]
pub mod hyprland;

#[cfg(target_os = "linux")]
pub mod x11;

#[cfg(target_os = "linux")]
pub fn pick_backend() -> Result<Box<dyn Backend>> {
    // sway sets BOTH SWAYSOCK and I3SOCK (i3-compat). i3 sets only I3SOCK.
    // Either case → same backend, since the IPC protocol is identical.
    if std::env::var_os("SWAYSOCK").is_some() || std::env::var_os("I3SOCK").is_some() {
        return Ok(Box::new(i3ipc::I3IpcBackend::new()?));
    }
    if std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE").is_some() {
        return Ok(Box::new(hyprland::HyprlandBackend::new()?));
    }
    if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        return Err(BackendError::UnsupportedEnvironment(
            "Wayland compositor without sway/Hyprland — \
             beckon does not work on GNOME/KDE Wayland (compositor blocks external focus). \
             Switch to X11 session, or use sway/Hyprland."
                .to_string(),
        ));
    }
    if std::env::var_os("DISPLAY").is_some() {
        return Ok(Box::new(x11::X11Backend::new()?));
    }
    Err(BackendError::UnsupportedEnvironment(
        "no supported display server detected".to_string(),
    ))
}

/// Distinguishes which compositor we resolved via env vars. Used by `-d`
/// to give the user a precise message even though the IPC backend is shared.
#[cfg(target_os = "linux")]
pub fn detect_compositor() -> Option<&'static str> {
    if std::env::var_os("SWAYSOCK").is_some() {
        Some("sway")
    } else if std::env::var_os("I3SOCK").is_some() {
        Some("i3")
    } else if std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE").is_some() {
        Some("Hyprland")
    } else if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        Some("Wayland (unsupported compositor)")
    } else if std::env::var_os("DISPLAY").is_some() {
        Some("X11")
    } else {
        None
    }
}

#[cfg(not(target_os = "linux"))]
pub fn pick_backend() -> Result<Box<dyn Backend>> {
    Err(BackendError::UnsupportedEnvironment(
        "beckon-linux only compiles on Linux".to_string(),
    ))
}
