//! Generic X11 backend over `x11rb` + EWMH. Targets every X11 desktop
//! environment that exposes the EWMH `_NET_*` atoms — GNOME-X11, KDE-X11,
//! XFCE, openbox, awesome, fluxbox, etc. (i3 has its own dedicated path
//! through `i3ipc.rs`.)
//!
//! Window enumeration, focus changes and hide/restore go through the four
//! canonical EWMH/ICCCM mechanisms:
//!   - `_NET_CLIENT_LIST_STACKING` for the window list (z-order: bottom→top,
//!     reversed here so index 0 = topmost ≈ most-recently focused).
//!   - `_NET_ACTIVE_WINDOW` (root property) for the currently focused window.
//!   - `_NET_ACTIVE_WINDOW` ClientMessage for focus requests, with source
//!     indication `2` (pager/taskbar) so anti-focus-stealing rules let the
//!     request through. This mirrors what tools like `wmctrl -a` send.
//!   - `WM_CHANGE_STATE` ClientMessage with `IconicState` (3) for hide;
//!     ICCCM-defined and respected by every WM. We deliberately avoid
//!     `_NET_WM_STATE_HIDDEN`, which is documented as a hint the WM sets,
//!     not something clients toggle.
//!
//! Window-class matching mirrors the other Linux backends: `WM_CLASS[1]`
//! (the "class" component of the property — same string the user typically
//! sets via `StartupWMClass=` in `.desktop` files) is compared against the
//! resolved `target` from `desktop::resolve`.

use std::process::{Command, Stdio};

use beckon_core::{Backend, BackendError, BeckonAction, InstalledApp, Result, RunningApp};
use x11rb::atom_manager;
use x11rb::connection::Connection;
use x11rb::properties::WmClass;
use x11rb::protocol::xproto::{
    AtomEnum, ClientMessageEvent, ConnectionExt as _, EventMask, Window,
};
use x11rb::rust_connection::RustConnection;
use x11rb::CURRENT_TIME;

use crate::algorithm::{decide, Decision, WindowSnapshot};

atom_manager! {
    /// EWMH / ICCCM atoms beckon talks. Interned once per backend instance.
    pub Atoms: AtomsCookie {
        _NET_CLIENT_LIST_STACKING,
        _NET_ACTIVE_WINDOW,
        _NET_WM_NAME,
        _NET_SUPPORTED,
        WM_CLASS,
        WM_NAME,
        WM_CHANGE_STATE,
        UTF8_STRING,
        STRING,
    }
}

pub struct X11Backend {
    conn: RustConnection,
    atoms: Atoms,
    root: Window,
}

impl X11Backend {
    pub fn new() -> Result<Self> {
        let (conn, screen_num) = x11rb::connect(None)
            .map_err(|e| BackendError::Ipc(format!("X11 connect: {}", e)))?;
        let root = conn.setup().roots[screen_num].root;
        let atoms = Atoms::new(&conn)
            .map_err(|e| BackendError::Ipc(format!("X11 intern atoms: {}", e)))?
            .reply()
            .map_err(|e| BackendError::Ipc(format!("X11 intern atoms reply: {}", e)))?;
        Ok(Self { conn, atoms, root })
    }
}

#[derive(Debug, Clone)]
struct X11Window {
    id: Window,
    /// `WM_CLASS[1]` (class). Empty if the window doesn't expose one.
    class: String,
    /// Best-effort window title (`_NET_WM_NAME` → `WM_NAME` fallback).
    name: String,
}

/// EWMH says `_NET_CLIENT_LIST_STACKING` is bottom-to-top. We want
/// top-first so the algorithm's "lowest recency = most recent" maps to
/// "topmost in stack". Returns windows along with their already-loaded
/// class so `list_running` and `beckon` share one round-trip per window.
fn collect_windows(conn: &RustConnection, root: Window, atoms: &Atoms) -> Result<Vec<X11Window>> {
    let reply = conn
        .get_property(
            false,
            root,
            atoms._NET_CLIENT_LIST_STACKING,
            AtomEnum::WINDOW,
            0,
            u32::MAX,
        )
        .map_err(|e| BackendError::Ipc(format!("get _NET_CLIENT_LIST_STACKING: {}", e)))?
        .reply()
        .map_err(|e| BackendError::Ipc(format!("reply _NET_CLIENT_LIST_STACKING: {}", e)))?;
    let stack: Vec<Window> = reply
        .value32()
        .ok_or_else(|| {
            BackendError::Ipc(
                "_NET_CLIENT_LIST_STACKING missing or wrong format — \
                 the running window manager likely doesn't speak EWMH"
                    .to_string(),
            )
        })?
        .collect();

    let mut out = Vec::with_capacity(stack.len());
    for win in stack.into_iter().rev() {
        let class = read_wm_class(conn, win).unwrap_or_default();
        if class.is_empty() {
            // Skip windows with no WM_CLASS — usually transient chrome
            // (notifications, pop-ups) we don't want to surface as apps.
            continue;
        }
        let name = read_window_name(conn, atoms, win).unwrap_or_default();
        out.push(X11Window {
            id: win,
            class,
            name,
        });
    }
    Ok(out)
}

fn active_window(conn: &RustConnection, root: Window, atoms: &Atoms) -> Result<Option<Window>> {
    let reply = conn
        .get_property(false, root, atoms._NET_ACTIVE_WINDOW, AtomEnum::WINDOW, 0, 1)
        .map_err(|e| BackendError::Ipc(format!("get _NET_ACTIVE_WINDOW: {}", e)))?
        .reply()
        .map_err(|e| BackendError::Ipc(format!("reply _NET_ACTIVE_WINDOW: {}", e)))?;
    let mut iter = match reply.value32() {
        Some(it) => it,
        None => return Ok(None),
    };
    Ok(iter.next().filter(|&w| w != 0))
}

fn read_wm_class(conn: &RustConnection, win: Window) -> Result<String> {
    let cookie = WmClass::get(conn, win)
        .map_err(|e| BackendError::Ipc(format!("WmClass cookie: {}", e)))?;
    // WmClass::reply() is Result<Option<WmClass>>: outer error = X11 IO,
    // inner None = property missing (some chrome windows have no class).
    let reply = match cookie.reply() {
        Ok(Some(r)) => r,
        _ => return Ok(String::new()),
    };
    Ok(String::from_utf8_lossy(reply.class()).into_owned())
}

fn read_window_name(conn: &RustConnection, atoms: &Atoms, win: Window) -> Result<String> {
    // Prefer UTF-8 _NET_WM_NAME; fall back to legacy WM_NAME (Latin-1).
    let utf8 = conn
        .get_property(false, win, atoms._NET_WM_NAME, atoms.UTF8_STRING, 0, 1024)
        .ok()
        .and_then(|c| c.reply().ok())
        .map(|r| r.value)
        .filter(|v| !v.is_empty());
    if let Some(bytes) = utf8 {
        return Ok(String::from_utf8_lossy(&bytes).into_owned());
    }
    let legacy = conn
        .get_property(false, win, atoms.WM_NAME, atoms.STRING, 0, 1024)
        .ok()
        .and_then(|c| c.reply().ok())
        .map(|r| r.value)
        .unwrap_or_default();
    Ok(String::from_utf8_lossy(&legacy).into_owned())
}

/// Send the EWMH `_NET_ACTIVE_WINDOW` ClientMessage to root. Source = 2
/// (pager/taskbar) so focus-stealing prevention treats this like a user
/// action rather than an unsolicited app raise.
fn request_focus(
    conn: &RustConnection,
    root: Window,
    atoms: &Atoms,
    target: Window,
    current_active: Option<Window>,
) -> Result<()> {
    let event = ClientMessageEvent::new(
        32,
        target,
        atoms._NET_ACTIVE_WINDOW,
        [
            2,                        // source indication: pager/taskbar
            CURRENT_TIME,             // timestamp
            current_active.unwrap_or(0),
            0,
            0,
        ],
    );
    conn.send_event(
        false,
        root,
        EventMask::SUBSTRUCTURE_NOTIFY | EventMask::SUBSTRUCTURE_REDIRECT,
        event,
    )
    .map_err(|e| BackendError::Ipc(format!("send _NET_ACTIVE_WINDOW: {}", e)))?;
    conn.flush()
        .map_err(|e| BackendError::Ipc(format!("flush focus request: {}", e)))?;
    Ok(())
}

/// Send the ICCCM `WM_CHANGE_STATE` ClientMessage with `IconicState` (3) so
/// the WM iconifies/minimizes the target. Restoration happens on the next
/// beckon call: a focus request to the same window de-iconifies it (per
/// EWMH §6.6 the WM SHOULD raise iconified windows on focus request).
fn request_iconify(conn: &RustConnection, root: Window, atoms: &Atoms, target: Window) -> Result<()> {
    const ICONIC_STATE: u32 = 3;
    let event = ClientMessageEvent::new(
        32,
        target,
        atoms.WM_CHANGE_STATE,
        [ICONIC_STATE, 0, 0, 0, 0],
    );
    conn.send_event(
        false,
        root,
        EventMask::SUBSTRUCTURE_NOTIFY | EventMask::SUBSTRUCTURE_REDIRECT,
        event,
    )
    .map_err(|e| BackendError::Ipc(format!("send WM_CHANGE_STATE: {}", e)))?;
    conn.flush()
        .map_err(|e| BackendError::Ipc(format!("flush iconify: {}", e)))?;
    Ok(())
}

/// Spawn a fully-detached child process for the .desktop `Exec` line. We
/// shell out to `/bin/sh -c` so quoted arguments and shell escapes survive
/// — this matches what XDG launchers (gtk-launch, dex) do.
fn launch_exec(exec: &str) -> Result<()> {
    Command::new("/bin/sh")
        .arg("-c")
        // `setsid` detaches the new process group from beckon's controlling
        // tty; if we exit (or are killed) the launched app keeps running.
        .arg(format!("setsid -f {} >/dev/null 2>&1", exec))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| BackendError::LaunchFailed {
            id: exec.to_string(),
            reason: format!("spawn /bin/sh: {}", e),
        })?;
    Ok(())
}

fn snapshots_from(windows: &[X11Window]) -> Vec<WindowSnapshot> {
    windows
        .iter()
        .enumerate()
        .map(|(idx, w)| WindowSnapshot::new(w.id.to_string(), &w.class, idx as i32))
        .collect()
}

fn parse_window(addr: &str) -> Result<Window> {
    addr.parse::<Window>()
        .map_err(|e| BackendError::Ipc(format!("bad window id `{}`: {}", addr, e)))
}

fn persist_previous(class: Option<&str>) {
    if let Some(c) = class {
        crate::state::write_previous(c);
    }
}

impl Backend for X11Backend {
    fn beckon(&self, id: &str) -> Result<BeckonAction> {
        let windows = collect_windows(&self.conn, self.root, &self.atoms)?;
        let active = active_window(&self.conn, self.root, &self.atoms)?;
        let active_addr = active.map(|w| w.to_string());

        let pre_focused_class = active_addr
            .as_deref()
            .and_then(|addr| windows.iter().find(|w| w.id.to_string() == addr))
            .map(|w| w.class.clone());

        let previous_app = crate::state::read_previous();

        let entry = crate::desktop::resolve(id);
        let target = entry
            .as_ref()
            .map(|e| e.id.as_str())
            .unwrap_or(id)
            .to_string();

        // X11 .desktop entries store StartupWMClass — it's usually the right
        // class to match against. Prefer it over the .desktop filename when
        // both differ, since that's what apps actually advertise via
        // WM_CLASS at runtime.
        let target_class = entry
            .as_ref()
            .and_then(|e| e.startup_wm_class.clone())
            .unwrap_or_else(|| target.clone());

        let snapshots = snapshots_from(&windows);
        let decision = decide(
            &snapshots,
            active_addr.as_deref(),
            &target_class,
            previous_app.as_deref(),
        );

        let action = match decision {
            Decision::Launch => {
                let entry = entry.ok_or_else(|| BackendError::LaunchFailed {
                    id: id.to_string(),
                    reason: format!(
                        "no .desktop entry matches `{}` and no running window has WM_CLASS=`{}`. \
                         Run `beckon -L` to list installed apps, or `beckon -s {}` to search.",
                        id, id, id
                    ),
                })?;
                launch_exec(&entry.exec)?;
                BeckonAction::Launched
            }
            Decision::Focus(addr) => {
                request_focus(
                    &self.conn,
                    self.root,
                    &self.atoms,
                    parse_window(&addr)?,
                    active,
                )?;
                BeckonAction::Focused
            }
            Decision::Cycle(addr) => {
                request_focus(
                    &self.conn,
                    self.root,
                    &self.atoms,
                    parse_window(&addr)?,
                    active,
                )?;
                BeckonAction::Cycled
            }
            Decision::ToggleBack(addr) => {
                request_focus(
                    &self.conn,
                    self.root,
                    &self.atoms,
                    parse_window(&addr)?,
                    active,
                )?;
                BeckonAction::ToggledBack
            }
            Decision::Hide(addr) => {
                request_iconify(&self.conn, self.root, &self.atoms, parse_window(&addr)?)?;
                BeckonAction::Hidden
            }
        };

        persist_previous(pre_focused_class.as_deref());
        Ok(action)
    }

    fn list_running(&self) -> Result<Vec<RunningApp>> {
        let windows = collect_windows(&self.conn, self.root, &self.atoms)?;
        let mut by_class: std::collections::BTreeMap<String, (String, usize)> = Default::default();
        for w in windows {
            let entry = by_class
                .entry(w.class.clone())
                .or_insert_with(|| (w.name.clone(), 0));
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

    #[test]
    fn parse_window_round_trips_decimal_id() {
        // x11rb's `Window` type formats with Display as decimal — make sure
        // our address mint/parse pair agrees.
        let id: Window = 0x0040_0001;
        let s = id.to_string();
        assert_eq!(parse_window(&s).unwrap(), id);
    }

    #[test]
    fn parse_window_rejects_garbage() {
        assert!(parse_window("not-a-number").is_err());
        assert!(parse_window("0xdeadbeef").is_err()); // hex not supported by parse::<u32>()
    }

    #[test]
    fn snapshots_from_uses_stack_index_as_recency() {
        let ws = vec![
            X11Window {
                id: 100,
                class: "kitty".into(),
                name: "k".into(),
            },
            X11Window {
                id: 200,
                class: "claude".into(),
                name: "c".into(),
            },
            X11Window {
                id: 300,
                class: "firefox".into(),
                name: "f".into(),
            },
        ];
        let snaps = snapshots_from(&ws);
        assert_eq!(snaps.len(), 3);
        // Topmost (kitty) gets recency 0 — algorithm reads it as MRU.
        assert_eq!(snaps[0].address, "100");
        assert_eq!(snaps[0].class, "kitty");
        assert_eq!(snaps[0].recency, 0);
        assert_eq!(snaps[2].class, "firefox");
        assert_eq!(snaps[2].recency, 2);
    }

    #[test]
    fn snapshots_from_address_round_trips_through_parse_window() {
        let ws = vec![X11Window {
            id: 0xdead_beef_u32 & 0x7fff_ffff,
            class: "x".into(),
            name: "x".into(),
        }];
        let snaps = snapshots_from(&ws);
        let id = parse_window(&snaps[0].address).unwrap();
        assert_eq!(id, ws[0].id);
    }
}
