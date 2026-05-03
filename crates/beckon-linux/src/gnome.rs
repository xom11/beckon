//! GNOME Wayland backend over the beckon GNOME Shell extension.
//!
//! GNOME Wayland (Mutter) intentionally has no protocol that lets an external
//! process enumerate or focus arbitrary windows — the security model only
//! allows the compositor itself to do that. So beckon ships a tiny GNOME
//! Shell extension (`extensions/beckon@xom11.github.io/`) that runs *inside*
//! gnome-shell and re-exposes just enough surface to D-Bus for this client
//! to drive the focus algorithm.
//!
//! Bus surface (defined in `extension.js`):
//!   destination = "org.gnome.Shell"
//!   path        = "/com/github/xom11/beckon"
//!   interface   = "org.gnome.Shell.Extensions.Beckon"
//!     ListWindows()        → a(tssbu)   (id, class, title, focused, monitor)
//!     ActivateWindow(t)    → b
//!     MinimizeWindow(t)    → b
//!     property Version     → s          probe — read at startup to verify the
//!                                       extension is loaded before we trust
//!                                       any other call to succeed
//!
//! Window identity: `MetaWindow.get_stable_sequence()` (uint32, fits uint64).
//! Stable for the window's lifetime, available on every supported GNOME.
//!
//! Recency: the extension orders windows by `Meta.TabList.NORMAL_ALL`, which
//! is the alt-tab order Mutter maintains internally — index 0 is the most-
//! recently-focused window across all workspaces. The shared algorithm
//! consumes this directly via `WindowSnapshot.recency`.

use std::process::{Command, Stdio};

use beckon_core::{Backend, BackendError, BeckonAction, InstalledApp, Result, RunningApp};
use zbus::blocking::{Connection, Proxy};

use crate::algorithm::{decide, Decision, WindowSnapshot};

const DEST: &str = "org.gnome.Shell";
const PATH: &str = "/com/github/xom11/beckon";
const IFACE: &str = "org.gnome.Shell.Extensions.Beckon";

/// Wire shape of `ListWindows` reply: `a(tssbu)` →
/// `Vec<(window_id, class, title, focused, monitor)>`.
type WindowRow = (u64, String, String, bool, u32);

pub struct GnomeBackend {
    conn: Connection,
}

impl GnomeBackend {
    pub fn new() -> Result<Self> {
        let conn = Connection::session().map_err(|e| {
            BackendError::Ipc(format!("session bus connect: {e}"))
        })?;

        // Probe the extension. `Version` is a tiny read-only property we
        // expose specifically so this call costs near-nothing while still
        // confirming both that gnome-shell is the bus name owner *and* that
        // it has our exported object at the expected path. Scope the proxy
        // borrow so we can move `conn` into Self after the probe returns.
        {
            let proxy = Self::proxy(&conn)?;
            proxy.get_property::<String>("Version").map_err(|e| {
                BackendError::Ipc(format!(
                    "GNOME Shell extension `beckon@xom11.github.io` not reachable on D-Bus: {e}. \
                     Install it (see beckon README) and run \
                     `gnome-extensions enable beckon@xom11.github.io`. \
                     On Wayland the shell only loads new extensions after logout/login."
                ))
            })?;
        }

        Ok(Self { conn })
    }

    fn proxy(conn: &Connection) -> Result<Proxy<'_>> {
        Proxy::new(conn, DEST, PATH, IFACE)
            .map_err(|e| BackendError::Ipc(format!("D-Bus proxy: {e}")))
    }

    fn list_windows(&self) -> Result<Vec<WindowRow>> {
        let proxy = Self::proxy(&self.conn)?;
        proxy
            .call::<_, _, Vec<WindowRow>>("ListWindows", &())
            .map_err(|e| BackendError::Ipc(format!("ListWindows: {e}")))
    }

    fn activate(&self, window_id: u64) -> Result<()> {
        let proxy = Self::proxy(&self.conn)?;
        let ok: bool = proxy
            .call("ActivateWindow", &(window_id,))
            .map_err(|e| BackendError::Ipc(format!("ActivateWindow: {e}")))?;
        if !ok {
            return Err(BackendError::WindowNotFound(window_id.to_string()));
        }
        Ok(())
    }

    fn minimize(&self, window_id: u64) -> Result<()> {
        let proxy = Self::proxy(&self.conn)?;
        let ok: bool = proxy
            .call("MinimizeWindow", &(window_id,))
            .map_err(|e| BackendError::Ipc(format!("MinimizeWindow: {e}")))?;
        if !ok {
            return Err(BackendError::WindowNotFound(window_id.to_string()));
        }
        Ok(())
    }
}

/// Fully-detached child process for the .desktop `Exec` line. Same recipe as
/// the X11 backend — `setsid -f` so the launched app survives beckon exiting,
/// stdio nulled so a stale fd can't keep the parent terminal alive when we
/// were invoked from a hotkey daemon.
fn launch_exec(exec: &str) -> Result<()> {
    Command::new("/bin/sh")
        .arg("-c")
        .arg(format!("setsid -f {} >/dev/null 2>&1", exec))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| BackendError::LaunchFailed {
            id: exec.to_string(),
            reason: format!("spawn /bin/sh: {e}"),
        })?;
    Ok(())
}

fn snapshots_from(rows: &[WindowRow]) -> Vec<WindowSnapshot> {
    rows.iter()
        .enumerate()
        .map(|(idx, (id, class, _title, _focused, _mon))| {
            WindowSnapshot::new(id.to_string(), class, idx as i32)
        })
        .collect()
}

fn parse_window_id(addr: &str) -> Result<u64> {
    addr.parse::<u64>()
        .map_err(|e| BackendError::Ipc(format!("bad window id `{addr}`: {e}")))
}

fn persist_previous(class: Option<&str>) {
    if let Some(c) = class {
        crate::state::write_previous(c);
    }
}

impl Backend for GnomeBackend {
    fn beckon(&self, id: &str) -> Result<BeckonAction> {
        let rows = self.list_windows()?;

        let active_addr = rows
            .iter()
            .find(|(_, _, _, focused, _)| *focused)
            .map(|(wid, _, _, _, _)| wid.to_string());

        let pre_focused_class = rows
            .iter()
            .find(|(_, _, _, focused, _)| *focused)
            .map(|(_, cls, _, _, _)| cls.clone());

        let previous_app = crate::state::read_previous();

        let entry = crate::desktop::resolve(id);
        let target = entry
            .as_ref()
            .map(|e| e.id.as_str())
            .unwrap_or(id)
            .to_string();

        // GNOME Wayland window class: prefer StartupWMClass when the
        // .desktop sets it (matches WM_CLASS for XWayland and most Wayland
        // toolkits that honor the hint). Otherwise fall through to the
        // resolved id, which equals the .desktop filename.
        let target_class = entry
            .as_ref()
            .and_then(|e| e.startup_wm_class.clone())
            .unwrap_or_else(|| target.clone());

        let snapshots = snapshots_from(&rows);
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
                        "no .desktop entry matches `{0}` and no running window has class=`{0}`. \
                         Run `beckon -L` to list installed apps, or `beckon -s {0}` to search.",
                        id
                    ),
                })?;
                launch_exec(&entry.exec)?;
                BeckonAction::Launched
            }
            Decision::Focus(addr) => {
                self.activate(parse_window_id(&addr)?)?;
                BeckonAction::Focused
            }
            Decision::Cycle(addr) => {
                self.activate(parse_window_id(&addr)?)?;
                BeckonAction::Cycled
            }
            Decision::ToggleBack(addr) => {
                self.activate(parse_window_id(&addr)?)?;
                BeckonAction::ToggledBack
            }
            Decision::Hide(addr) => {
                self.minimize(parse_window_id(&addr)?)?;
                BeckonAction::Hidden
            }
        };

        persist_previous(pre_focused_class.as_deref());
        Ok(action)
    }

    fn list_running(&self) -> Result<Vec<RunningApp>> {
        let rows = self.list_windows()?;
        let mut by_class: std::collections::BTreeMap<String, (String, usize)> = Default::default();
        for (_id, class, title, _focused, _mon) in rows {
            let entry = by_class.entry(class).or_insert_with(|| (title.clone(), 0));
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

    fn row(id: u64, class: &str, focused: bool) -> WindowRow {
        (id, class.to_string(), String::new(), focused, 0)
    }

    #[test]
    fn parse_window_id_round_trips() {
        let id: u64 = 4_294_967_295; // u32::MAX, biggest stable_sequence we expect
        assert_eq!(parse_window_id(&id.to_string()).unwrap(), id);
    }

    #[test]
    fn parse_window_id_rejects_garbage() {
        assert!(parse_window_id("0xff").is_err());
        assert!(parse_window_id("not a number").is_err());
        assert!(parse_window_id("-1").is_err());
    }

    #[test]
    fn snapshots_preserve_input_order_as_recency() {
        // The extension already returns rows in MRU order; index in the
        // Vec is the recency. Verify the projection.
        let rows = vec![
            row(10, "kitty", true),
            row(20, "claude", false),
            row(30, "firefox", false),
        ];
        let snaps = snapshots_from(&rows);
        assert_eq!(snaps.len(), 3);
        assert_eq!(snaps[0].address, "10");
        assert_eq!(snaps[0].class, "kitty");
        assert_eq!(snaps[0].recency, 0);
        assert_eq!(snaps[2].address, "30");
        assert_eq!(snaps[2].recency, 2);
    }
}
