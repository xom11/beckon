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
pub mod kde;

#[cfg(target_os = "linux")]
pub mod niri;

/// Which in-compositor collaborator a Wayland session needs.
#[cfg(target_os = "linux")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WaylandDesktop {
    Gnome,
    Kde,
    Unknown,
}

/// Read the desktop out of `XDG_CURRENT_DESKTOP`, which is a colon-separated
/// list (`KDE`, `ubuntu:GNOME`, `GNOME-Flashback:GNOME`, …). Matching is
/// case-insensitive and per-component, so a distro prefix doesn't hide the
/// desktop behind it.
#[cfg(target_os = "linux")]
fn wayland_desktop() -> WaylandDesktop {
    let raw = match std::env::var("XDG_CURRENT_DESKTOP") {
        Ok(v) => v,
        Err(_) => return WaylandDesktop::Unknown,
    };
    for part in raw.split(':') {
        let part = part.trim().to_ascii_uppercase();
        if part == "KDE" || part == "PLASMA" {
            return WaylandDesktop::Kde;
        }
        if part == "GNOME" {
            return WaylandDesktop::Gnome;
        }
    }
    WaylandDesktop::Unknown
}

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
    // niri exports NIRI_SOCKET to children; the env var is the only source
    // of truth (nested instances are real, so never derive the path).
    if std::env::var_os("NIRI_SOCKET").is_some() {
        return Ok(Box::new(niri::NiriBackend::new()?));
    }
    if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        // Mutter (GNOME) and KWin (KDE) both refuse to let an outside
        // process focus a window, so each needs a collaborator running
        // *inside* the compositor. Which one to try is decided by
        // XDG_CURRENT_DESKTOP — guessing wrong produces a confusing error
        // that talks about the wrong desktop entirely.
        return match wayland_desktop() {
            WaylandDesktop::Kde => kde::KdeBackend::new().map(|b| Box::new(b) as Box<dyn Backend>),
            WaylandDesktop::Gnome => {
                gnome::GnomeBackend::new().map(|b| Box::new(b) as Box<dyn Backend>)
            }
            WaylandDesktop::Unknown => {
                // No desktop hint. Probe both before giving up: the user may
                // simply have an unset XDG_CURRENT_DESKTOP.
                gnome::GnomeBackend::new()
                    .map(|b| Box::new(b) as Box<dyn Backend>)
                    .or_else(|gnome_err| {
                        kde::KdeBackend::new()
                            .map(|b| Box::new(b) as Box<dyn Backend>)
                            .map_err(|kde_err| {
                                BackendError::UnsupportedEnvironment(format!(
                                    "unrecognised Wayland compositor (XDG_CURRENT_DESKTOP is \
                                 unset or unknown, and it is not sway, Hyprland or niri). \
                                 GNOME probe: {gnome_err} \
                                 KWin probe: {kde_err}"
                                ))
                            })
                    })
            }
        };
    }
    if std::env::var_os("DISPLAY").is_some() {
        return Ok(Box::new(x11::X11Backend::new()?));
    }
    Err(BackendError::UnsupportedEnvironment(
        "no supported display server detected".to_string(),
    ))
}

/// One resolution report per name, for `beckon check --resolve`.
///
/// A batch rather than a loop over `desktop::resolve_detailed`, which re-runs
/// `scan()` — every `applications/` directory in `$XDG_DATA_DIRS`, recursively
/// — on every call.
///
/// Takes no backend: this is the resolution half of step 2, and `.desktop`
/// files are on disk whether or not a compositor is running.
#[cfg(target_os = "linux")]
pub fn resolve_reports(names: &[&str]) -> Result<Vec<beckon_core::certainty::NameReport>> {
    Ok(desktop::resolve_reports(names))
}

/// Returns an error rather than an empty vector: an empty one reads as
/// "every name resolved", which is the one answer this cannot know.
#[cfg(not(target_os = "linux"))]
pub fn resolve_reports(_names: &[&str]) -> Result<Vec<beckon_core::certainty::NameReport>> {
    Err(BackendError::UnsupportedEnvironment(
        "beckon-linux only compiles on Linux".to_string(),
    ))
}

/// Distinguishes which compositor we resolved via env vars. Used by
/// `beckon doctor` to give the user a precise message even though the IPC
/// backend is shared.
#[cfg(target_os = "linux")]
pub fn detect_compositor() -> Option<&'static str> {
    if std::env::var_os("SWAYSOCK").is_some() {
        Some("sway")
    } else if std::env::var_os("I3SOCK").is_some() {
        Some("i3")
    } else if std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE").is_some() {
        Some("Hyprland")
    } else if std::env::var_os("NIRI_SOCKET").is_some() {
        Some("niri")
    } else if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        match wayland_desktop() {
            WaylandDesktop::Gnome => Some("GNOME Wayland (via shell extension)"),
            WaylandDesktop::Kde => Some("KDE Wayland (via KWin script)"),
            WaylandDesktop::Unknown => Some("Wayland (desktop not identified)"),
        }
    } else if std::env::var_os("DISPLAY").is_some() {
        Some("X11")
    } else {
        None
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    /// `wayland_desktop` reads a process-wide env var, so these cases have to
    /// run one at a time — hence one test, not five.
    #[test]
    fn wayland_desktop_matches_per_component() {
        let cases = [
            ("KDE", WaylandDesktop::Kde),
            ("plasma", WaylandDesktop::Kde),
            ("GNOME", WaylandDesktop::Gnome),
            // Distro prefixes must not hide the desktop behind them.
            ("ubuntu:GNOME", WaylandDesktop::Gnome),
            ("GNOME-Flashback:GNOME", WaylandDesktop::Gnome),
            ("KDE:plasma", WaylandDesktop::Kde),
            // Not a substring match: "GNOME" must not fall out of "GNOMEISH".
            ("GNOMEISH", WaylandDesktop::Unknown),
            ("sway", WaylandDesktop::Unknown),
            ("", WaylandDesktop::Unknown),
        ];
        for (value, want) in cases {
            std::env::set_var("XDG_CURRENT_DESKTOP", value);
            assert_eq!(wayland_desktop(), want, "XDG_CURRENT_DESKTOP={value:?}");
        }
        std::env::remove_var("XDG_CURRENT_DESKTOP");
        assert_eq!(wayland_desktop(), WaylandDesktop::Unknown, "unset");
    }
}

#[cfg(not(target_os = "linux"))]
pub fn pick_backend() -> Result<Box<dyn Backend>> {
    Err(BackendError::UnsupportedEnvironment(
        "beckon-linux only compiles on Linux".to_string(),
    ))
}
