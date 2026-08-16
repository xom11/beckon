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
    Atom, AtomEnum, ClientMessageEvent, ConnectionExt as _, EventMask, MapState, Window,
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
        WM_STATE,
        UTF8_STRING,
        STRING,
        _NET_WM_WINDOW_TYPE,
        _NET_WM_WINDOW_TYPE_NORMAL,
        _NET_WM_WINDOW_TYPE_DIALOG,
        _NET_WM_WINDOW_TYPE_UTILITY,
        _NET_WM_DESKTOP,
        _NET_CURRENT_DESKTOP,
    }
}

/// Is this window something the user would call "an app window"?
///
/// `_NET_CLIENT_LIST_STACKING` contains panels, docks, desktop-icon windows
/// and notification surfaces alongside real apps. They carry a `WM_CLASS`,
/// so the class filter alone lets them through — and then step 5b happily
/// "toggles back" to xfce4-panel, which the WM refuses to focus, so beckon
/// reports success and nothing moves. EWMH says a window with no
/// `_NET_WM_WINDOW_TYPE` is to be treated as NORMAL (legacy clients), so
/// absence of the property is a pass, not a reject.
fn is_app_window(conn: &RustConnection, atoms: &Atoms, win: Window) -> bool {
    let Ok(cookie) = conn.get_property(
        false,
        win,
        atoms._NET_WM_WINDOW_TYPE,
        AtomEnum::ATOM,
        0,
        u32::MAX,
    ) else {
        return true;
    };
    let Ok(reply) = cookie.reply() else {
        return true;
    };
    let Some(types) = reply.value32() else {
        return true; // property absent → legacy client → treat as NORMAL
    };
    let types: Vec<u32> = types.collect();
    if types.is_empty() {
        return true;
    }
    // The property is an ordered list, most-preferred first. Accept the
    // window if any listed type is one a user switches to by name.
    types.iter().any(|t| {
        *t == atoms._NET_WM_WINDOW_TYPE_NORMAL
            || *t == atoms._NET_WM_WINDOW_TYPE_DIALOG
            || *t == atoms._NET_WM_WINDOW_TYPE_UTILITY
    })
}

pub struct X11Backend {
    conn: RustConnection,
    atoms: Atoms,
    root: Window,
}

impl X11Backend {
    pub fn new() -> Result<Self> {
        let (conn, screen_num) =
            x11rb::connect(None).map_err(|e| BackendError::Ipc(format!("X11 connect: {}", e)))?;
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
    /// `WM_CLASS[0]` (instance / `res_name`) — the other half of the same
    /// property, read from the same reply at no extra round trip. Empty when
    /// the window exposes no `WM_CLASS` at all, which is already filtered.
    ///
    /// It is only ever different from `class` in a way that matters for
    /// browser-installed web apps: measured on rog 2026-08-16, a Brave PWA
    /// reports `("crx_<hash>", "Brave-browser")` while the browser itself
    /// reports `("brave-browser", "Brave-browser")`. See
    /// `algorithm::WindowSnapshot::instance`.
    instance: String,
    /// Best-effort window title (`_NET_WM_NAME` → `WM_NAME` fallback).
    /// Empty unless the caller asked for it — see `collect_windows`.
    name: String,
}

/// EWMH says `_NET_CLIENT_LIST_STACKING` is bottom-to-top. We want
/// top-first so the algorithm's "lowest recency = most recent" maps to
/// "topmost in stack". Returns windows along with their already-loaded
/// class so `list_running` and `beckon` share one round-trip per window.
///
/// `want_names` is there for the hot path, not for convenience: only `list_running`
/// ever reads `X11Window::name`, while `beckon` decides entirely from
/// `id` / `class` / `instance`. Reading it there bought a blocking
/// `_NET_WM_NAME` round trip per window — two where the property is absent
/// and the `WM_NAME` fallback runs — and threw every byte away, on a desktop
/// where 30 open windows is ordinary and the budget is 50 ms.
fn collect_windows(
    conn: &RustConnection,
    root: Window,
    atoms: &Atoms,
    want_names: bool,
) -> Result<Vec<X11Window>> {
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
        let (instance, class) = read_wm_class(conn, win).unwrap_or_default();
        if class.is_empty() {
            // Skip windows with no WM_CLASS — usually transient chrome
            // (notifications, pop-ups) we don't want to surface as apps.
            continue;
        }
        if !is_app_window(conn, atoms, win) {
            continue;
        }
        let name = if want_names {
            read_window_name(conn, atoms, win).unwrap_or_default()
        } else {
            String::new()
        };
        out.push(X11Window {
            id: win,
            class,
            instance,
            name,
        });
    }
    Ok(out)
}

fn active_window(conn: &RustConnection, root: Window, atoms: &Atoms) -> Result<Option<Window>> {
    let reply = conn
        .get_property(
            false,
            root,
            atoms._NET_ACTIVE_WINDOW,
            AtomEnum::WINDOW,
            0,
            1,
        )
        .map_err(|e| BackendError::Ipc(format!("get _NET_ACTIVE_WINDOW: {}", e)))?
        .reply()
        .map_err(|e| BackendError::Ipc(format!("reply _NET_ACTIVE_WINDOW: {}", e)))?;
    let mut iter = match reply.value32() {
        Some(it) => it,
        None => return Ok(None),
    };
    Ok(iter.next().filter(|&w| w != 0))
}

/// Both halves of `WM_CLASS`, as `(instance, class)`.
///
/// One reply, two strings: `x11rb`'s `WmClass` already parses the pair out of
/// the single `GetProperty` this function was making anyway, so reading the
/// instance costs nothing on the hot path. Returning them together is what
/// keeps that true -- a second `read_wm_instance` would be a second round
/// trip for a property already in hand.
fn read_wm_class(conn: &RustConnection, win: Window) -> Result<(String, String)> {
    let cookie =
        WmClass::get(conn, win).map_err(|e| BackendError::Ipc(format!("WmClass cookie: {}", e)))?;
    // WmClass::reply() is Result<Option<WmClass>>: outer error = X11 IO,
    // inner None = property missing (some chrome windows have no class).
    let reply = match cookie.reply() {
        Ok(Some(r)) => r,
        _ => return Ok((String::new(), String::new())),
    };
    Ok((
        String::from_utf8_lossy(reply.instance()).into_owned(),
        String::from_utf8_lossy(reply.class()).into_owned(),
    ))
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

const ICONIC_STATE: u32 = 3;

/// How long to give the window manager to finish de-iconifying before we
/// send the focus request anyway. Only ever paid when restoring a hidden
/// window; a normal focus costs one round-trip and no sleep.
const MAP_WAIT: std::time::Duration = std::time::Duration::from_millis(400);
const MAP_POLL: std::time::Duration = std::time::Duration::from_millis(10);

/// EWMH's `_NET_WM_DESKTOP` value for "show on all desktops" (sticky).
const ALL_DESKTOPS: u32 = 0xFFFF_FFFF;

fn is_viewable(conn: &RustConnection, win: Window) -> bool {
    conn.get_window_attributes(win)
        .ok()
        .and_then(|c| c.reply().ok())
        .is_some_and(|a| a.map_state == MapState::VIEWABLE)
}

/// First word of a single-CARDINAL property, or `None` if it is absent or
/// unreadable. Both callers want exactly one number.
fn read_cardinal(conn: &RustConnection, win: Window, prop: Atom) -> Option<u32> {
    conn.get_property(false, win, prop, AtomEnum::CARDINAL, 0, 1)
        .ok()
        .and_then(|c| c.reply().ok())
        .and_then(|r| r.value32()?.next())
}

/// Is the target parked on a virtual desktop other than the one on screen?
///
/// Split from the two X11 reads so the decision itself is testable without a
/// server. Absence of either property answers `false`: a WM that publishes no
/// `_NET_CURRENT_DESKTOP` has no virtual desktops to be elsewhere on, and the
/// old unconditional wait is the safe answer there.
fn is_on_another_desktop(win_desktop: Option<u32>, current_desktop: Option<u32>) -> bool {
    match (win_desktop, current_desktop) {
        // A sticky window is on this desktop too, so it is never elsewhere —
        // and `0xFFFFFFFF` read as an ordinary desktop number would compare
        // unequal to every real one, skipping the wait for exactly the
        // iconified windows it exists for.
        (Some(w), Some(c)) => w != ALL_DESKTOPS && w != c,
        _ => false,
    }
}

/// Bring an iconified window back before asking for focus, and wait until
/// the window manager has actually done it.
///
/// EWMH §_NET_ACTIVE_WINDOW says the WM "SHOULD" bring the window forward,
/// which we used to read as "every WM de-iconifies on a focus request".
/// openbox does not: measured on Ubuntu 26.04 + Xvfb + openbox, after
/// beckon's own hide (step 5c) the window sat at `WM_STATE = Iconic` with
/// `_NET_ACTIVE_WINDOW` stuck at `0`, so the next hotkey press could never
/// bring it back — the window was stranded for good. ICCCM §4.1.4 gives the
/// portable answer: to return an iconified window to `NormalState`, map it.
/// The WM holds SubstructureRedirect on the root, so our MapRequest is
/// redirected to it and handled as a de-iconify — exactly what
/// `xdotool windowmap` does, which does restore the window here.
///
/// The wait is the other half of the fix, and it is not optional. The WM is
/// just another client: flushing the MapRequest only guarantees the *server*
/// saw it. Sending the activation in the same breath loses the race — the
/// window came back Iconic every time, while the same map-then-activate pair
/// issued by two separate `xdotool` invocations (which are naturally spaced
/// apart) always worked. We poll `map_state`, which is server state rather
/// than the WM-owned `WM_STATE` property, so there is nothing to race.
///
/// **A window that is merely on another virtual desktop is exempt from both
/// halves.** A WM unmaps the windows of the desktops it is not showing, so
/// `is_viewable` is false for those as well — and nothing ever de-iconifies
/// them, because they were never iconified, so the loop burned the full
/// `MAP_WAIT` on an ordinary focus of an app one desktop over. `WM_STATE`
/// cannot tell the two apart: openbox reports `IconicState` for an
/// off-desktop window too, so testing it distinguishes nothing.
/// `_NET_WM_DESKTOP` against `_NET_CURRENT_DESKTOP` can, and getting there is
/// the activation's job rather than ours — EWMH has the WM handling
/// `_NET_ACTIVE_WINDOW` switch to the window's desktop, which is the very
/// request `request_focus` sends on the next line.
fn ensure_mapped(conn: &RustConnection, root: Window, atoms: &Atoms, target: Window) -> Result<()> {
    if is_viewable(conn, target) {
        return Ok(());
    }
    // **The MapRequest is sent whatever desktop the window is on.** It is the
    // half that returns an iconified window to `NormalState` (ICCCM §4.1.4),
    // and a window beckon itself hid with step 5c can ALSO be sitting on
    // another desktop — skipping the map for those two conditions together
    // strands the window exactly the way openbox stranded it before this
    // function existed, and the hotkey could never bring it back.
    conn.map_window(target)
        .map_err(|e| BackendError::Ipc(format!("map window: {}", e)))?;
    conn.flush()
        .map_err(|e| BackendError::Ipc(format!("flush map request: {}", e)))?;

    // **The WAIT is the half the desktop test may skip**, and skipping it is
    // the whole point of that test. `map_state` cannot become `Viewable`
    // while the window belongs to a desktop that is not on screen, so the
    // poll below is 400 ms that can never succeed — it is not a race we lose,
    // it is a condition that cannot hold. The `_NET_ACTIVE_WINDOW` request
    // the caller sends next is what makes the WM switch desktops, and the WM
    // maps it there.
    if is_on_another_desktop(
        read_cardinal(conn, target, atoms._NET_WM_DESKTOP),
        read_cardinal(conn, root, atoms._NET_CURRENT_DESKTOP),
    ) {
        return Ok(());
    }

    let deadline = std::time::Instant::now() + MAP_WAIT;
    while std::time::Instant::now() < deadline {
        if is_viewable(conn, target) {
            return Ok(());
        }
        std::thread::sleep(MAP_POLL);
    }
    // Best effort: fall through and let the focus request try anyway rather
    // than failing a hotkey press outright.
    Ok(())
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
    ensure_mapped(conn, root, atoms, target)?;

    let event = ClientMessageEvent::new(
        32,
        target,
        atoms._NET_ACTIVE_WINDOW,
        [
            2,            // source indication: pager/taskbar
            CURRENT_TIME, // timestamp
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
/// beckon call — see `request_focus`, which maps the window before asking
/// for focus because not every WM de-iconifies on a focus request alone.
fn request_iconify(
    conn: &RustConnection,
    root: Window,
    atoms: &Atoms,
    target: Window,
) -> Result<()> {
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
        .map(|(idx, w)| {
            WindowSnapshot::new(w.id.to_string(), &w.class, idx as i32)
                .with_instance(Some(w.instance.as_str()))
        })
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
        let windows = collect_windows(&self.conn, self.root, &self.atoms, false)?;
        let active = active_window(&self.conn, self.root, &self.atoms)?;
        let active_addr = active.map(|w| w.to_string());

        let pre_focused_class = active_addr
            .as_deref()
            .and_then(|addr| windows.iter().find(|w| w.id.to_string() == addr))
            .map(|w| w.class.clone());

        let previous_app = crate::state::read_previous();

        let entry = crate::desktop::resolve(id);

        // On X11 the running window advertises `WM_CLASS`, which is what
        // `StartupWMClass=` records — so that is the strongest candidate.
        // The `.desktop` filename stem stays in the set as a fallback for
        // the many entries that omit `StartupWMClass` and whose stem does
        // match (`kitty.desktop` ⇒ `kitty`). Matching is case-insensitive:
        // `xterm.desktop` has no `StartupWMClass` and the window reports
        // `XTerm`, which a byte-wise compare would miss.
        let target_class = crate::algorithm::Target::new(
            entry
                .as_ref()
                .map(|e| {
                    [e.startup_wm_class.clone(), Some(e.id.clone())]
                        .into_iter()
                        .flatten()
                        .collect::<Vec<_>>()
                })
                .unwrap_or_else(|| vec![id.to_string()]),
        );

        let snapshots = snapshots_from(&windows);
        let decision = decide(
            &snapshots,
            active_addr.as_deref(),
            target_class,
            previous_app.as_deref(),
        );

        let action = match decision {
            Decision::Launch => {
                let entry = entry.ok_or_else(|| BackendError::NoMatch {
                    id: id.to_string(),
                    hint: format!(
                        "no .desktop entry matches `{}` and no running window has WM_CLASS=`{}`. \
                         Run `beckon installed` to list installed apps, \
                         or `beckon search {}` to search.",
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
        let windows = collect_windows(&self.conn, self.root, &self.atoms, true)?;
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
                instance: String::new(),
                name: "k".into(),
            },
            X11Window {
                id: 200,
                class: "claude".into(),
                instance: String::new(),
                name: "c".into(),
            },
            X11Window {
                id: 300,
                class: "firefox".into(),
                instance: String::new(),
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
    fn a_window_on_another_desktop_skips_the_map_wait() {
        // The whole point: an unmapped window one desktop over is not
        // iconified, so `ensure_mapped` used to poll `MAP_WAIT` (400 ms) on
        // every ordinary focus of such a window and then give up anyway.
        assert!(is_on_another_desktop(Some(1), Some(0)));
        assert!(is_on_another_desktop(Some(0), Some(3)));
    }

    #[test]
    fn a_window_on_the_current_desktop_still_waits() {
        // This is the openbox restore path the wait was written for, and it
        // must be untouched.
        assert!(!is_on_another_desktop(Some(2), Some(2)));
        assert!(!is_on_another_desktop(Some(0), Some(0)));
    }

    #[test]
    fn a_sticky_window_is_never_elsewhere() {
        // 0xFFFFFFFF is EWMH's "on all desktops". Compared as a plain number
        // it differs from every real desktop, which would skip the wait for a
        // window that is right here and genuinely iconified.
        assert!(!is_on_another_desktop(Some(ALL_DESKTOPS), Some(0)));
        assert!(!is_on_another_desktop(Some(ALL_DESKTOPS), Some(7)));
    }

    #[test]
    fn a_missing_desktop_property_keeps_the_unconditional_wait() {
        // A WM that publishes neither atom has no virtual desktops we can
        // reason about; the pre-existing behaviour is the safe answer.
        assert!(!is_on_another_desktop(None, Some(0)));
        assert!(!is_on_another_desktop(Some(1), None));
        assert!(!is_on_another_desktop(None, None));
    }

    #[test]
    fn snapshots_from_address_round_trips_through_parse_window() {
        let ws = vec![X11Window {
            id: 0xdead_beef_u32 & 0x7fff_ffff,
            class: "x".into(),
            instance: String::new(),
            name: "x".into(),
        }];
        let snaps = snapshots_from(&ws);
        let id = parse_window(&snaps[0].address).unwrap();
        assert_eq!(id, ws[0].id);
    }
}
