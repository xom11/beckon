//! Hyprland backend. Talks to the compositor via the request socket at
//! `$XDG_RUNTIME_DIR/hypr/$HYPRLAND_INSTANCE_SIGNATURE/.socket.sock` (or
//! `/tmp/hypr/<sig>/.socket.sock` on Hyprland < 0.40).
//!
//! Algorithm steps mirror `i3ipc.rs`:
//!   3.  not running                 → `dispatch exec <Exec>` from .desktop
//!   4.  running, not focused        → `dispatch focuswindow address:0xN`
//!   5a. focused, app has more wins  → rotate the address-ordered ring
//!   5b. focused, only one window    → toggle to the most-recent other app
//!   5c. focused, nothing else       → hide via movetoworkspacesilent special:beckon
//!
//! Window identity: Hyprland exposes `class` for both Wayland (= app_id) and
//! XWayland (= WM_CLASS) clients, so a single field is enough — no fallback
//! chain like sway/i3.
//!
//! `focusHistoryID` (0 = focused) feeds `WindowSnapshot::recency`, which
//! drives steps 4 and 5b. It deliberately does **not** drive step 5a: focusing
//! a window promotes it to 0 and demotes the one just left, so a recency ring
//! is a 2-cycle and windows 3..N are unreachable. `algorithm::decide` rotates
//! an address-ordered ring instead — see the comment there.
//!
//! Unlike every other Linux backend, this one passes `previous_app = None` to
//! `decide`. `$XDG_RUNTIME_DIR/beckon-mru` exists because the sway tree
//! carries no focus history; Hyprland's `focusHistoryID` is real MRU and
//! tracks focus changes beckon never saw (mouse clicks, native binds), so
//! consulting a file that only records beckon's own actions can only make
//! step 5b less accurate. Measured on Hyprland 0.56.0: focusing a window with
//! `hyprctl dispatch focuswindow`, i.e. outside beckon entirely, reorders
//! `focusHistoryID` immediately.

use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::time::Duration;

use beckon_core::{Backend, BackendError, BeckonAction, InstalledApp, Result, RunningApp};
use serde::Deserialize;

use crate::algorithm::{decide, Decision, WindowSnapshot};

/// Special workspace name beckon parks the focused window on for step 5c.
///
/// Coming back out is beckon's job, not the compositor's. `dispatch
/// focuswindow` on a parked window makes Hyprland *show* the special
/// workspace as an overlay (`Actions::focus` → `changeWorkspace` →
/// `setSpecialWorkspace`) but never re-parents it, so the window keeps
/// belonging to `special:beckon`: the moment focus moves elsewhere it
/// disappears again, and `movetoworkspace`, `movefocus` and the user's own
/// `$mod+1..4` all behave as if it no longer exists. sway does not have this
/// problem because `focus` on a scratchpad container runs
/// `root_scratchpad_show`, which detaches it and adds it to the workspace the
/// user is looking at. `unpark_if_needed` is beckon's equivalent — without it
/// hide is a one-way door, and the *second* hide is a silent no-op because
/// the window is already on the destination workspace.
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

#[derive(Debug, Deserialize, Clone, PartialEq, Eq, Default)]
pub(crate) struct Workspace {
    #[serde(default)]
    pub(crate) id: i64,
    #[serde(default)]
    pub(crate) name: String,
}

#[derive(Debug, Deserialize, Clone, PartialEq, Eq)]
pub(crate) struct Client {
    pub(crate) address: String,
    pub(crate) class: String,
    #[serde(default)]
    pub(crate) title: String,
    #[serde(rename = "focusHistoryID", default)]
    pub(crate) focus_history_id: i32,
    /// Where the window lives. Only the name is load-bearing, and only to
    /// spot beckon's own parking workspace — see `unpark_if_needed`.
    #[serde(default)]
    pub(crate) workspace: Workspace,
    /// Hyprland sets this on windows it is deliberately keeping off screen
    /// (terminal swallowing). Note this is NOT how an inactive group tab is
    /// reported — measured on 0.56.0, a backgrounded tab is
    /// `hidden=false, visible=false`, which is why the filter below tests
    /// `hidden` and must never test `visible`: filtering on `visible` would
    /// hide every group tab but the front one and break step 5a cycling
    /// through a tabbed group.
    #[serde(default)]
    pub(crate) hidden: bool,
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
    Ok(parse_clients(&raw)?
        .into_iter()
        .filter(is_beckonable)
        .collect())
}

/// Windows beckon is willing to focus, cycle to, or toggle back to.
///
/// Every other Linux backend filters its enumeration and each learned to the
/// hard way: `x11.rs` drops panels by `_NET_WM_WINDOW_TYPE` and `kde.rs`
/// drops `skipTaskbar`, both because step 5b toggling to a window the
/// compositor then refuses to focus makes beckon report success while
/// nothing moves. This backend filtered nothing at all.
///
/// Deliberately narrow: only a window Hyprland is actively keeping off
/// screen, or one with no class to match against in the first place. In
/// particular an inactive group tab stays in — see `Client::hidden`.
fn is_beckonable(c: &Client) -> bool {
    !c.hidden && !c.class.trim().is_empty()
}

fn active_address() -> Result<Option<String>> {
    let raw = send("j/activewindow")?;
    parse_active(&raw)
}

/// The workspace the focused monitor is showing. Only consulted on the
/// restore path, so the ordinary focus hot path still costs the same two
/// queries it always did.
fn active_workspace_id() -> Result<i64> {
    let raw = send("j/activeworkspace")?;
    parse_active_workspace(&raw)
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

pub(crate) fn parse_active_workspace(raw: &str) -> Result<i64> {
    let trimmed = raw.trim();
    let ws: Workspace = serde_json::from_str(trimmed).map_err(|e| {
        BackendError::Ipc(format!(
            "parse j/activeworkspace: {} (raw: {:.200})",
            e, trimmed
        ))
    })?;
    Ok(ws.id)
}

/// Move a window off beckon's parking workspace before focusing it.
///
/// Returns the command that has to run first, or `None` when the window is
/// somewhere ordinary. Split out from the IPC so the decision is testable
/// without a live compositor.
///
/// Only `special:beckon` is unparked. A user's own special workspace
/// (`special:magic`, a scratchpad they set up in their config) is where they
/// deliberately put that window, so beckon shows it as an overlay the way
/// Hyprland does and leaves it where it found it.
pub(crate) fn unpark_command(clients: &[Client], addr: &str, active_ws: i64) -> Option<String> {
    let parked = clients
        .iter()
        .any(|c| c.address == addr && c.workspace.name == HIDE_WORKSPACE);
    if !parked {
        return None;
    }
    Some(format!(
        "dispatch movetoworkspacesilent {},address:{}",
        active_ws, addr
    ))
}

/// Focus a window, first returning it to the user's workspace if beckon had
/// parked it. `movetoworkspacesilent` does not move focus, so the
/// `focuswindow` that follows is still what actually raises it.
fn focus_window(clients: &[Client], addr: &str) -> Result<()> {
    let needs_unpark = clients
        .iter()
        .any(|c| c.address == addr && c.workspace.name == HIDE_WORKSPACE);
    if needs_unpark {
        let ws = active_workspace_id()?;
        if let Some(cmd) = unpark_command(clients, addr, ws) {
            dispatch(&cmd)?;
        }
    }
    dispatch(&format!("dispatch focuswindow address:{}", addr))
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

        let entry = crate::desktop::resolve(id);
        // Hyprland's `class` comes from the Wayland `app_id` for native
        // clients and from `WM_CLASS` for XWayland ones, so the same
        // candidate set as sway applies: filename stem first, then
        // `StartupWMClass` for the XWayland case.
        let target = crate::desktop::target_classes(entry.as_ref(), id);

        let snapshots = snapshots_from(&clients);
        // `previous_app = None` on purpose: `focusHistoryID` already carries
        // real MRU, including focus changes beckon never made. See the
        // module docs.
        let decision = decide(&snapshots, active.as_deref(), target, None);

        let action = match decision {
            Decision::Launch => {
                let entry = entry.ok_or_else(|| BackendError::NoMatch {
                    id: id.to_string(),
                    hint: format!(
                        "no .desktop entry matches `{}` and no running window has class `{}`. \
                         Run `beckon installed` to list installed apps, \
                         or `beckon search {}` to search.",
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
                focus_window(&clients, &addr)?;
                BeckonAction::Focused
            }
            Decision::Cycle(addr) => {
                focus_window(&clients, &addr)?;
                BeckonAction::Cycled
            }
            Decision::ToggleBack(addr) => {
                focus_window(&clients, &addr)?;
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
        let mut entries = crate::desktop::visible(crate::desktop::scan());
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
            workspace: Workspace {
                id: 1,
                name: "1".to_string(),
            },
            hidden: false,
        }
    }

    fn parked(addr: &str, class: &str, fhid: i32) -> Client {
        Client {
            workspace: Workspace {
                id: -99,
                name: HIDE_WORKSPACE.to_string(),
            },
            ..client(addr, class, fhid)
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

    // ----------------- is_beckonable() -----------------

    #[test]
    fn beckonable_accepts_an_ordinary_window() {
        assert!(is_beckonable(&client("0xA", "kitty", 0)));
    }

    #[test]
    fn beckonable_rejects_hidden_and_classless_windows() {
        let mut h = client("0xA", "kitty", 0);
        h.hidden = true;
        assert!(!is_beckonable(&h), "a swallowed window is not a target");

        assert!(!is_beckonable(&client("0xB", "", 0)));
        assert!(!is_beckonable(&client("0xC", "   ", 0)));
    }

    #[test]
    fn beckonable_keeps_a_backgrounded_group_tab() {
        // Measured on Hyprland 0.56.0: the tab of a group that is not on top
        // reports `hidden=false, visible=false`. It must stay in the list or
        // step 5a can never cycle into it. This test exists to fail if
        // anyone extends the filter to `visible`.
        let raw = r#"[{
            "address":"0x1","class":"foot","title":"t","focusHistoryID":1,
            "hidden":false,"visible":false,"grouped":["0x1","0x2"],
            "workspace":{"id":1,"name":"1"}
        }]"#;
        let parsed = parse_clients(raw).unwrap();
        assert!(is_beckonable(&parsed[0]));
    }

    #[test]
    fn beckonable_keeps_a_parked_window() {
        // A window beckon hid must remain findable, or the next press
        // launches a duplicate instead of bringing it back.
        assert!(is_beckonable(&parked("0xA", "kitty", 2)));
    }

    // ----------------- unpark_command() -----------------

    #[test]
    fn unpark_returns_none_for_an_ordinary_window() {
        let clients = vec![client("0xA", "kitty", 0)];
        assert_eq!(unpark_command(&clients, "0xA", 3), None);
    }

    #[test]
    fn unpark_moves_a_parked_window_to_the_live_workspace() {
        let clients = vec![parked("0xA", "kitty", 1)];
        assert_eq!(
            unpark_command(&clients, "0xA", 3).as_deref(),
            Some("dispatch movetoworkspacesilent 3,address:0xA")
        );
    }

    #[test]
    fn unpark_leaves_a_users_own_special_workspace_alone() {
        // `special:magic` is where the user put it on purpose. Showing it as
        // an overlay is Hyprland's behaviour and beckon should not override
        // the placement.
        let mut c = client("0xA", "kitty", 1);
        c.workspace = Workspace {
            id: -7,
            name: "special:magic".to_string(),
        };
        assert_eq!(unpark_command(&[c], "0xA", 3), None);
    }

    #[test]
    fn unpark_ignores_an_address_that_is_not_in_the_list() {
        let clients = vec![parked("0xA", "kitty", 1)];
        assert_eq!(unpark_command(&clients, "0xZ", 3), None);
    }

    // ----------------- parse_active_workspace -----------------

    #[test]
    fn parse_active_workspace_reads_the_id() {
        assert_eq!(
            parse_active_workspace(r#"{"id":2,"name":"2","monitor":"HDMI-A-1"}"#).unwrap(),
            2
        );
    }

    #[test]
    fn parse_active_workspace_rejects_garbage() {
        assert!(matches!(
            parse_active_workspace("not json").unwrap_err(),
            BackendError::Ipc(_)
        ));
    }

    // ----------------- parse_clients / parse_active -----------------

    #[test]
    fn parse_clients_reads_workspace_and_hidden() {
        let raw = r#"[
            {"address":"0x1","class":"kitty","workspace":{"id":-99,"name":"special:beckon"},"hidden":false},
            {"address":"0x2","class":"foot","workspace":{"id":1,"name":"1"},"hidden":true}
        ]"#;
        let parsed = parse_clients(raw).unwrap();
        assert_eq!(parsed[0].workspace.name, "special:beckon");
        assert_eq!(parsed[0].workspace.id, -99);
        assert!(!parsed[0].hidden);
        assert!(parsed[1].hidden);
    }

    #[test]
    fn parse_clients_defaults_workspace_when_absent() {
        let parsed = parse_clients(r#"[{"address":"0x1","class":"kitty"}]"#).unwrap();
        assert_eq!(parsed[0].workspace.name, "");
        assert!(!parsed[0].hidden);
    }

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
