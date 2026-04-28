//! i3-IPC backend. Works for both sway (Wayland) and i3 (X11) — they share
//! the same IPC protocol. `swayipc` transparently picks `SWAYSOCK` or
//! `I3SOCK` from the environment.
//!
//! Algorithm steps (see `CLAUDE.md` "Focus algorithm"):
//!   3. not running                 → exec via the WM
//!   4. running, not focused        → focus first window
//!   5a. focused, app has more wins → cycle to next window of same app
//!   5b. focused, only one window   → toggle to a different-app window (best-effort A)
//!   5c. focused, nothing else      → move to scratchpad (hide)
//!
//! Best-effort 5b: pick any window with a different app_id from the tree.
//! No MRU history exposed through IPC.
//!
//! Window identity:
//!   - Wayland (sway): `node.app_id` is the canonical id.
//!   - X11 (i3, or XWayland under sway): no `app_id`; fall back to
//!     `window_properties.class` (second token of WM_CLASS).

use beckon_core::{Backend, BackendError, BeckonAction, InstalledApp, Result, RunningApp};
use swayipc::{Connection, Node, NodeType};

use crate::algorithm::{decide, Decision, WindowSnapshot};

pub struct I3IpcBackend;

impl I3IpcBackend {
    pub fn new() -> Result<Self> {
        // Verify connection works at construction so errors surface before
        // any user-facing action.
        Connection::new().map_err(|e| BackendError::Ipc(e.to_string()))?;
        Ok(Self)
    }

    fn connect(&self) -> Result<Connection> {
        Connection::new().map_err(|e| BackendError::Ipc(e.to_string()))
    }
}

#[derive(Debug, Clone)]
struct WindowInfo {
    con_id: i64,
    app_id: String,
    name: String,
    focused: bool,
}

fn collect_windows(node: &Node, out: &mut Vec<WindowInfo>) {
    let is_leaf = node.nodes.is_empty() && node.floating_nodes.is_empty();
    let is_window = matches!(node.node_type, NodeType::Con | NodeType::FloatingCon);

    if is_leaf && is_window {
        // Wayland windows expose `app_id`; XWayland windows expose
        // `window_properties.class`. Prefer the former, fall back to the latter.
        let app_id = node
            .app_id
            .clone()
            .or_else(|| node.window_properties.as_ref().and_then(|wp| wp.class.clone()));

        if let Some(app_id) = app_id {
            out.push(WindowInfo {
                con_id: node.id,
                app_id,
                name: node.name.clone().unwrap_or_default(),
                focused: node.focused,
            });
        }
    }

    for child in &node.nodes {
        collect_windows(child, out);
    }
    for child in &node.floating_nodes {
        collect_windows(child, out);
    }
}

fn run_sway(conn: &mut Connection, cmd: &str) -> Result<()> {
    let outcomes = conn
        .run_command(cmd)
        .map_err(|e| BackendError::Ipc(e.to_string()))?;
    for outcome in outcomes {
        outcome.map_err(|e| BackendError::Ipc(e.to_string()))?;
    }
    Ok(())
}

fn focus_con(conn: &mut Connection, con_id: i64) -> Result<()> {
    run_sway(conn, &format!("[con_id={}] focus", con_id))
}

fn persist_previous(app: Option<&str>) {
    if let Some(a) = app {
        crate::state::write_previous(a);
    }
}

fn hide_con(conn: &mut Connection, con_id: i64) -> Result<()> {
    run_sway(
        conn,
        &format!("[con_id={}] move container to scratchpad", con_id),
    )
}

/// Build the neutral snapshot list the shared algorithm consumes. Tree
/// traversal order doubles as `recency`: the algorithm uses it for "most
/// recent" picks, which preserves i3ipc.rs's previous "first match wins"
/// behaviour because every window ends up with a unique increasing index.
fn snapshots_from(windows: &[WindowInfo]) -> Vec<WindowSnapshot> {
    windows
        .iter()
        .enumerate()
        .map(|(idx, w)| WindowSnapshot::new(w.con_id.to_string(), &w.app_id, idx as i32))
        .collect()
}

/// Parse a snapshot address back into the swayipc `con_id` it was minted
/// from. Round-trip should never fail for addresses the backend itself
/// produced; if it does, surface as IPC error rather than panicking.
fn parse_con_id(addr: &str) -> Result<i64> {
    addr.parse::<i64>()
        .map_err(|e| BackendError::Ipc(format!("bad con_id `{}`: {}", addr, e)))
}

impl Backend for I3IpcBackend {
    fn beckon(&self, id: &str) -> Result<BeckonAction> {
        let mut conn = self.connect()?;
        let tree = conn
            .get_tree()
            .map_err(|e| BackendError::Ipc(e.to_string()))?;

        let mut windows = Vec::new();
        collect_windows(&tree, &mut windows);

        // Snapshots feed the shared algorithm; `address` is con_id-as-string.
        let snapshots = snapshots_from(&windows);
        let active = windows
            .iter()
            .find(|w| w.focused)
            .map(|w| w.con_id.to_string());

        // What's focused right now (before any action). Used at the end to
        // update the MRU file: after we change focus, this app becomes
        // "the one we left" — i.e. the previous app for the next call.
        let pre_focused_app = windows
            .iter()
            .find(|w| w.focused)
            .map(|w| w.app_id.clone());

        // What was focused before the most recent beckon action ran.
        // Used by step 5b to land on the app the user *actually* came from.
        let previous_app = crate::state::read_previous();

        // Resolve user input → (target_app_id_pattern, optional desktop entry).
        // If a .desktop entry is found, its filename is the runtime app_id
        // pattern to match against the tree (and its Exec is used to launch).
        // If no entry matches, fall back to treating `id` as a literal
        // app_id — this still allows focusing apps that aren't in any
        // .desktop file (e.g. ad-hoc programs the user launched manually).
        let entry = crate::desktop::resolve(id);
        let target = entry
            .as_ref()
            .map(|e| e.id.as_str())
            .unwrap_or(id)
            .to_string();

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
                        "no .desktop entry matches `{}` and no running window has that app_id. \
                         Run `beckon -L` to list installed apps, or `beckon -s {}` to search.",
                        id, id
                    ),
                })?;
                run_sway(&mut conn, &format!("exec {}", entry.exec)).map_err(|e| {
                    BackendError::LaunchFailed {
                        id: id.to_string(),
                        reason: e.to_string(),
                    }
                })?;
                BeckonAction::Launched
            }
            Decision::Focus(addr) => {
                focus_con(&mut conn, parse_con_id(&addr)?)?;
                BeckonAction::Focused
            }
            Decision::Cycle(addr) => {
                focus_con(&mut conn, parse_con_id(&addr)?)?;
                BeckonAction::Cycled
            }
            Decision::ToggleBack(addr) => {
                focus_con(&mut conn, parse_con_id(&addr)?)?;
                BeckonAction::ToggledBack
            }
            Decision::Hide(addr) => {
                hide_con(&mut conn, parse_con_id(&addr)?)?;
                BeckonAction::Hidden
            }
        };

        persist_previous(pre_focused_app.as_deref());
        Ok(action)
    }

    fn list_running(&self) -> Result<Vec<RunningApp>> {
        let mut conn = self.connect()?;
        let tree = conn
            .get_tree()
            .map_err(|e| BackendError::Ipc(e.to_string()))?;

        let mut windows = Vec::new();
        collect_windows(&tree, &mut windows);

        let mut by_id: std::collections::BTreeMap<String, (String, usize)> = Default::default();
        for w in windows {
            let entry = by_id
                .entry(w.app_id)
                .or_insert_with(|| (w.name.clone(), 0));
            entry.1 += 1;
        }

        Ok(by_id
            .into_iter()
            .map(|(id, (name, window_count))| RunningApp {
                id,
                name,
                window_count,
            })
            .collect())
    }

    fn list_installed(&self) -> Result<Vec<InstalledApp>> {
        // Use the .desktop filename as the id. On sway, this matches the
        // runtime `app_id` for Brave PWAs (`brave-<hash>-Default.desktop` →
        // sway sets `app_id = brave-<hash>-Default`) and for most desktop
        // apps. StartupWMClass is unreliable on Wayland because clients
        // like Brave ignore it and pick the filename instead.
        //
        // After running the app once, `beckon -l` is the source of truth:
        // copy the id from there into the dotfile.
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
