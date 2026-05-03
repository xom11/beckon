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
pub mod gnome;

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
        // Mutter (GNOME) and KWin (KDE) block external focus by design.
        // We work around that by talking to a small GNOME Shell extension
        // we ship — try it before giving up. If the extension isn't loaded
        // (KDE, or GNOME without our extension installed), the probe fails
        // with a hint pointing the user at the install instructions.
        return gnome::GnomeBackend::new()
            .map(|b| Box::new(b) as Box<dyn Backend>)
            .map_err(|e| BackendError::UnsupportedEnvironment(format!(
                "Wayland compositor without sway/Hyprland. \
                 Tried the GNOME Shell extension fallback and it was unreachable: {e} \
                 (KDE Wayland is unsupported; on GNOME Wayland, install the \
                 `beckon@xom11.github.io` extension shipped in the beckon repo)."
            )));
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
        // We can't tell GNOME from KDE without probing — leave that to the
        // backend selector. This label is for `-d` only.
        if std::env::var("XDG_CURRENT_DESKTOP")
            .map(|v| v.to_uppercase().contains("GNOME"))
            .unwrap_or(false)
        {
            Some("GNOME Wayland (via shell extension)")
        } else {
            Some("Wayland (unsupported compositor)")
        }
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
