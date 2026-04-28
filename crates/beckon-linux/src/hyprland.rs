//! Hyprland backend. Talks to the compositor via the request socket at
//! `$XDG_RUNTIME_DIR/hypr/$HYPRLAND_INSTANCE_SIGNATURE/.socket.sock` (or
//! `/tmp/hypr/<sig>/.socket.sock` on Hyprland < 0.40).
//!
//! Algorithm steps mirror `i3ipc.rs`:
//!   3.  not running                 → `dispatch exec <Exec>` from .desktop
//!   4.  running, not focused        → `dispatch focuswindow address:0xN`
//!   5a. focused, app has more wins  → cycle to next-most-recent same-app window
//!   5b. focused, only one window    → toggle to MRU other-app window
//!   5c. focused, nothing else       → hide via movetoworkspacesilent special:beckon
//!
//! Window identity: Hyprland exposes `class` for both Wayland (= app_id) and
//! XWayland (= WM_CLASS) clients, so a single field is enough — no fallback
//! chain like sway/i3.
//!
//! Cycle order uses `focusHistoryID` (0 = most-recent). With two windows of
//! the same app this gives a clean A↔B toggle on repeated invocations,
//! matching the practical behaviour of `i3ipc.rs`.

use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::time::Duration;

use beckon_core::{Backend, BackendError, BeckonAction, InstalledApp, Result, RunningApp};
use serde::Deserialize;

use crate::algorithm::{decide, Decision, WindowSnapshot};

/// Special workspace name beckon parks the focused window on for step 5c.
/// A single shared name is fine: re-invoking `beckon <id>` finds the window
/// in `j/clients`, sees it's not focused, and brings it back via
/// `dispatch focuswindow` (Hyprland surfaces the special workspace).
const HIDE_WORKSPACE: &str = "special:beckon";

pub struct HyprlandBackend;

impl HyprlandBackend {
    pub fn new() -> Result<Self> {
        // Probe the socket so connection problems surface up-front, before
        // any user-visible action runs.
        let _ = send("version")?;
        Ok(Self)
    }
}

#[derive(Debug, Deserialize, Clone, PartialEq, Eq)]
pub(crate) struct Client {
    pub(crate) address: String,
    pub(crate) class: String,
    #[serde(default)]
    pub(crate) title: String,
    #[serde(rename = "focusHistoryID", default)]
    pub(crate) focus_history_id: i32,
}

#[derive(Debug, Deserialize)]
struct ActiveWindow {
    #[serde(default)]
    address: String,
}

fn socket_path() -> Result<PathBuf> {
    let sig = std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE").ok_or_else(|| {
        BackendError::Ipc("HYPRLAND_INSTANCE_SIGNATURE not set (Hyprland not running?)".to_string())
    })?;

    // Hyprland 0.40+ moved the socket under XDG_RUNTIME_DIR. Prefer that;
    // fall back to /tmp for older versions.
    if let Some(runtime) = std::env::var_os("XDG_RUNTIME_DIR") {
        let p = PathBuf::from(runtime)
            .join("hypr")
            .join(&sig)
            .join(".socket.sock");
        if p.exists() {
            return Ok(p);
        }
    }
    let p = PathBuf::from("/tmp/hypr").join(&sig).join(".socket.sock");
    if p.exists() {
        return Ok(p);
    }
    Err(BackendError::Ipc(
        "Hyprland socket not found in $XDG_RUNTIME_DIR/hypr/<sig> or /tmp/hypr/<sig>".to_string(),
    ))
}

fn send(cmd: &str) -> Result<String> {
    let path = socket_path()?;
    let mut stream = UnixStream::connect(&path)
        .map_err(|e| BackendError::Ipc(format!("connect {}: {}", path.display(), e)))?;
    // Bound the hot path: a wedged compositor must not hang a hotkey press.
    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(2)));
    stream
        .write_all(cmd.as_bytes())
        .map_err(|e| BackendError::Ipc(format!("write `{}`: {}", cmd, e)))?;
    let mut buf = String::new();
    stream
        .read_to_string(&mut buf)
        .map_err(|e| BackendError::Ipc(format!("read `{}`: {}", cmd, e)))?;
    Ok(buf)
}

fn list_clients() -> Result<Vec<Client>> {
    let raw = send("j/clients")?;
    parse_clients(&raw)
}

fn active_address() -> Result<Option<String>> {
    let raw = send("j/activewindow")?;
    parse_active(&raw)
}

pub(crate) fn parse_clients(raw: &str) -> Result<Vec<Client>> {
    serde_json::from_str(raw).map_err(|e| {
        BackendError::Ipc(format!("parse j/clients: {} (raw: {:.200})", e, raw.trim()))
    })
}

pub(crate) fn parse_active(raw: &str) -> Result<Option<String>> {
    let trimmed = raw.trim();
    // Hyprland returns `{}` (sometimes with whitespace) when nothing is focused.
    if trimmed.is_empty() || trimmed == "{}" {
        return Ok(None);
    }
    let aw: ActiveWindow = serde_json::from_str(trimmed).map_err(|e| {
        BackendError::Ipc(format!(
            "parse j/activewindow: {} (raw: {:.200})",
            e, trimmed
        ))
    })?;
    if aw.address.is_empty() || aw.address == "0x0" {
        Ok(None)
    } else {
        Ok(Some(aw.address))
    }
}

/// Send a dispatch command and treat any non-`ok` body as a failure.
fn dispatch(cmd: &str) -> Result<()> {
    let resp = send(cmd)?;
    let trimmed = resp.trim();
    if trimmed.eq_ignore_ascii_case("ok") {
        return Ok(());
    }
    Err(BackendError::Ipc(format!(
        "command `{}` returned `{}`",
        cmd, trimmed
    )))
}

fn persist_previous(app: Option<&str>) {
    if let Some(a) = app {
        crate::state::write_previous(a);
    }
}

fn snapshots_from(clients: &[Client]) -> Vec<WindowSnapshot> {
    clients
        .iter()
        .map(|c| WindowSnapshot::new(&c.address, &c.class, c.focus_history_id))
        .collect()
}

impl Backend for HyprlandBackend {
    fn beckon(&self, id: &str) -> Result<BeckonAction> {
        let clients = list_clients()?;
        let active = active_address()?;

        // Capture the class focused before this invocation so we can store it
        // as the new "previous" once we change focus. State file is shared
        // with the i3ipc backend (same XDG runtime path) — harmless because
        // a user runs only one compositor at a time.
        let pre_focused_class = active
            .as_deref()
            .and_then(|addr| clients.iter().find(|c| c.address == addr))
            .map(|c| c.class.clone());

        let previous_app = crate::state::read_previous();

        let entry = crate::desktop::resolve(id);
        let target = entry
            .as_ref()
            .map(|e| e.id.as_str())
            .unwrap_or(id)
            .to_string();

        let snapshots = snapshots_from(&clients);
        let decision = decide(
            &snapshots,
            active.as_deref(),
            &target,
            previous_app.as_deref(),
        );

        let action = match decision {
            Decision::Launch => {
                let entry = entry.ok_or_else(|| BackendError::LaunchFailed {
                    id: id.to_string(),
                    reason: format!(
                        "no .desktop entry matches `{}` and no running window has class `{}`. \
                         Run `beckon -L` to list installed apps, or `beckon -s {}` to search.",
                        id, id, id
                    ),
                })?;
                dispatch(&format!("dispatch exec {}", entry.exec)).map_err(|e| {
                    BackendError::LaunchFailed {
                        id: id.to_string(),
                        reason: e.to_string(),
                    }
                })?;
                BeckonAction::Launched
            }
            Decision::Focus(addr) => {
                dispatch(&format!("dispatch focuswindow address:{}", addr))?;
                BeckonAction::Focused
            }
            Decision::Cycle(addr) => {
                dispatch(&format!("dispatch focuswindow address:{}", addr))?;
                BeckonAction::Cycled
            }
            Decision::ToggleBack(addr) => {
                dispatch(&format!("dispatch focuswindow address:{}", addr))?;
                BeckonAction::ToggledBack
            }
            Decision::Hide(addr) => {
                dispatch(&format!(
                    "dispatch movetoworkspacesilent {},address:{}",
                    HIDE_WORKSPACE, addr
                ))?;
                BeckonAction::Hidden
            }
        };

        persist_previous(pre_focused_class.as_deref());
        Ok(action)
    }

    fn list_running(&self) -> Result<Vec<RunningApp>> {
        let clients = list_clients()?;
        let mut by_class: std::collections::BTreeMap<String, (String, usize)> = Default::default();
        for c in clients {
            let entry = by_class
                .entry(c.class)
                .or_insert_with(|| (c.title.clone(), 0));
            entry.1 += 1;
        }
        Ok(by_class
            .into_iter()
            .map(|(id, (name, window_count))| RunningApp {
                id,
                name,
                window_count,
            })
            .collect())
    }

    fn list_installed(&self) -> Result<Vec<InstalledApp>> {
        // Same shape as i3ipc: .desktop filename is the runtime id, and on
        // Wayland clients (Hyprland exposes Wayland app_id as `class`) the
        // filename matches the runtime class for the apps we care about.
        let mut entries = crate::desktop::scan();
        entries.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(entries
            .into_iter()
            .map(|e| InstalledApp {
                id: e.id,
                name: e.name,
                exec: Some(e.exec),
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn client(addr: &str, class: &str, fhid: i32) -> Client {
        Client {
            address: addr.to_string(),
            class: class.to_string(),
            title: format!("{} window", class),
            focus_history_id: fhid,
        }
    }

    // ----------------- snapshots_from() -----------------
    //
    // The decision algorithm itself lives in `algorithm.rs` and is covered
    // by tests there. Here we only verify the Hyprland-specific projection
    // from `Client` → `WindowSnapshot`.

    #[test]
    fn snapshots_from_maps_class_address_and_focus_history_id() {
        let clients = vec![client("0xA", "kitty", 0), client("0xB", "claude", 3)];
        let snaps = snapshots_from(&clients);
        assert_eq!(snaps.len(), 2);
        assert_eq!(snaps[0].address, "0xA");
        assert_eq!(snaps[0].class, "kitty");
        assert_eq!(snaps[0].recency, 0);
        assert_eq!(snaps[1].address, "0xB");
        assert_eq!(snaps[1].class, "claude");
        assert_eq!(snaps[1].recency, 3);
    }

    // ----------------- parse_clients / parse_active -----------------

    #[test]
    fn parse_clients_basic() {
        let raw = r#"[
            {"address":"0x55a","class":"kitty","title":"vim","focusHistoryID":0},
            {"address":"0x55b","class":"firefox","title":"hn","focusHistoryID":1}
        ]"#;
        let parsed = parse_clients(raw).unwrap();
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].address, "0x55a");
        assert_eq!(parsed[0].class, "kitty");
        assert_eq!(parsed[0].focus_history_id, 0);
        assert_eq!(parsed[1].class, "firefox");
        assert_eq!(parsed[1].focus_history_id, 1);
    }

    #[test]
    fn parse_clients_ignores_extra_fields() {
        // Real Hyprland output has many more fields — must round-trip cleanly.
        let raw = r#"[{
            "address":"0x55a","class":"kitty","title":"t","focusHistoryID":0,
            "mapped":true,"hidden":false,"at":[1,2],"size":[3,4],
            "workspace":{"id":1,"name":"1"},"floating":false,"monitor":0,
            "initialClass":"kitty","initialTitle":"t","pid":1,"xwayland":false,
            "pinned":false,"fullscreen":0,"fullscreenClient":0,"grouped":[],
            "tags":[],"swallowing":"0x0","inhibitingIdle":false
        }]"#;
        let parsed = parse_clients(raw).unwrap();
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].class, "kitty");
    }

    #[test]
    fn parse_clients_empty_array() {
        let parsed = parse_clients("[]").unwrap();
        assert!(parsed.is_empty());
    }

    #[test]
    fn parse_clients_missing_optional_fields() {
        // title and focusHistoryID may be absent on edge-case clients —
        // serde defaults must keep parsing alive.
        let raw = r#"[{"address":"0x1","class":"kitty"}]"#;
        let parsed = parse_clients(raw).unwrap();
        assert_eq!(parsed[0].title, "");
        assert_eq!(parsed[0].focus_history_id, 0);
    }

    #[test]
    fn parse_active_object() {
        let raw = r#"{"address":"0xdead","class":"kitty","title":"x","focusHistoryID":0}"#;
        assert_eq!(parse_active(raw).unwrap(), Some("0xdead".to_string()));
    }

    #[test]
    fn parse_active_empty_object_means_none() {
        assert_eq!(parse_active("{}").unwrap(), None);
        assert_eq!(parse_active("  {}  \n").unwrap(), None);
    }

    #[test]
    fn parse_active_empty_string_means_none() {
        assert_eq!(parse_active("").unwrap(), None);
        assert_eq!(parse_active("   ").unwrap(), None);
    }

    #[test]
    fn parse_active_zero_address_means_none() {
        let raw = r#"{"address":"0x0","class":""}"#;
        assert_eq!(parse_active(raw).unwrap(), None);
    }

    #[test]
    fn parse_clients_invalid_json_returns_ipc_error() {
        let err = parse_clients("not json").unwrap_err();
        assert!(matches!(err, BackendError::Ipc(_)));
    }
}
