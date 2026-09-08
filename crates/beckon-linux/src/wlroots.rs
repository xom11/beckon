//! Generic wlroots backend over `zwlr_foreign_toplevel_manager_v1`.
//!
//! This is the one Linux backend that is **not** named after a compositor.
//! Every other module here speaks an IPC that exactly one project ships
//! (`SWAYSOCK`, `HYPRLAND_INSTANCE_SIGNATURE`, `NIRI_SOCKET`,
//! `MANGO_INSTANCE_SIGNATURE`, a GNOME extension, a KWin script). labwc has
//! no IPC at all — no socket, no `labwcmsg` — so a per-compositor module
//! would have had nothing to talk to. What labwc does have is the wlroots
//! foreign-toplevel protocol, and so do river, wayfire, japokwm, dwl and
//! every other wlroots compositor that has not grown its own control
//! channel. One module covers all of them.
//!
//! **The dispatcher's test is therefore a capability, not a name.** There is
//! no env var to read: `WlrootsBackend::new` connects to `WAYLAND_DISPLAY`
//! and asks the compositor's own registry whether it implements the
//! protocol. A compositor that does not advertise it fails the bind and the
//! caller falls through — no list of compositor names to keep up to date.
//!
//! ## What the protocol gives, and what it does not
//!
//! `zwlr_foreign_toplevel_manager_v1` announces one
//! `zwlr_foreign_toplevel_handle_v1` per toplevel window, each followed by
//! `app_id`, `title`, a full `state` array and a `done`. That is enough for
//! four of the five steps: `activate(seat)` focuses, `set_minimized` hides,
//! `unset_minimized` restores, and `app_id` is the same string a Wayland
//! client reports as its `app_id` elsewhere — comparable to the `.desktop`
//! filename stem exactly as in `niri.rs` and `mango.rs`.
//!
//! Two things it does NOT give, both of which degrade behaviour rather than
//! break it, and neither of which a future reader should try to "fix" by
//! inventing data:
//!
//!   - **No focus timestamp.** niri has `focus_timestamp` and Hyprland has
//!     `focusHistoryID`; this protocol has only the `activated` flag on the
//!     currently focused window. `recency` is therefore *activated first,
//!     then announcement order* — the same shape `mango.rs` settles for.
//!     Step 5b (toggle-back) consequently leans on the cross-backend MRU
//!     file at `$XDG_RUNTIME_DIR/beckon-mru` for its good answers and on
//!     announcement order only as a fallback. Step 5a (cycle) is unaffected:
//!     `algorithm::decide` orders the cycle ring by ADDRESS, never by
//!     recency, precisely so that a backend without real focus history still
//!     visits every window once per lap.
//!
//!   - **No `WM_CLASS` instance half.** wlroots reports an XWayland window's
//!     `WM_CLASS` *class* as its `app_id`, and the pair's other half is not
//!     in the protocol at all. `WindowSnapshot::instance` stays `None`, so a
//!     browser-installed web app running under XWayland is matched the way
//!     it was before that field existed. Wayland-native PWAs are unaffected:
//!     they carry the PWA's own `app_id`.
//!
//! ## Addresses are wayland object ids
//!
//! `WindowSnapshot::address` has to be a string the backend can turn back
//! into a window, and it has to order the cycle ring. A foreign-toplevel
//! handle is a protocol object, not a number the compositor exposes — so the
//! address is the handle's own **wayland object id**, and the live handles
//! are kept in `Session::state` for the length of the invocation to map it
//! back.
//!
//! That id is minted by the compositor, ascends in announcement order, and
//! is stable for the object's lifetime — which is what the ring needs. It is
//! NOT stable across connections, and it does not have to be: beckon reads
//! the window list and acts on it inside a single connection. What must hold
//! across invocations is the ORDER, and it does: an unchanged set of
//! toplevels is announced in the same order every time, so consecutive
//! keypresses rotate through the same ring.
//!
//! wlroots announces newest-toplevel-first (its handle list is built with
//! `wl_list_insert`, which prepends), so a lap runs newest → oldest. The
//! direction differs from sway's tree order; the property `algorithm`
//! actually depends on — every window reached exactly once per lap — does
//! not.

use std::cell::RefCell;
use std::collections::BTreeMap;

use beckon_core::{Backend, BackendError, BeckonAction, InstalledApp, Result, RunningApp};
use wayland_client::globals::{registry_queue_init, GlobalListContents};
use wayland_client::protocol::{wl_registry, wl_seat};
use wayland_client::{event_created_child, Connection, Dispatch, EventQueue, Proxy, QueueHandle};
use wayland_protocols_wlr::foreign_toplevel::v1::client::{
    zwlr_foreign_toplevel_handle_v1::{self, ZwlrForeignToplevelHandleV1},
    zwlr_foreign_toplevel_manager_v1::{self, ZwlrForeignToplevelManagerV1},
};

use crate::algorithm::{decide, Decision, WindowSnapshot};

/// The two `state` values this backend reads, resolved from the generated
/// enum so a protocol renumbering is a compile error rather than a silent
/// misread. The `state` event carries a raw little-endian `u32` array.
const ST_MINIMIZED: u32 = zwlr_foreign_toplevel_handle_v1::State::Minimized as u32;
const ST_ACTIVATED: u32 = zwlr_foreign_toplevel_handle_v1::State::Activated as u32;

/// Everything about one toplevel that any decision here depends on, with no
/// wayland types in it. Split out from the live handle so the projection
/// into `WindowSnapshot` — the only part of this file with real policy in it
/// — is a pure function with unit tests, the way `beckon_core` keeps its own
/// decisions testable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ToplevelView {
    /// Wayland object id of the handle. See the module docs.
    pub id: u32,
    pub app_id: Option<String>,
    pub title: Option<String>,
    pub activated: bool,
    pub minimized: bool,
}

/// A live toplevel: its handle plus the view above.
struct Toplevel {
    handle: ZwlrForeignToplevelHandleV1,
    view: ToplevelView,
    /// The protocol declares `title` / `app_id` / `state` pending until
    /// `done`. We do not double-buffer them (nothing here acts mid-burst),
    /// but we DO gate reading on `done` — see [`Session::sync`].
    done: bool,
    closed: bool,
}

#[derive(Default)]
struct Toplevels {
    windows: Vec<Toplevel>,
    /// The compositor is going away; the manager object is dead.
    finished: bool,
}

impl Toplevels {
    fn find_mut(&mut self, id: u32) -> Option<&mut Toplevel> {
        self.windows
            .iter_mut()
            .find(|t| t.view.id == id && !t.closed)
    }

    /// Live toplevels, sorted by object id. The Vec is already in
    /// announcement order and server ids ascend with it, but the server's
    /// id map does reuse freed ids, so sorting is what actually guarantees
    /// the order rather than an assumption about allocation.
    fn views(&self) -> Vec<ToplevelView> {
        let mut out: Vec<ToplevelView> = self
            .windows
            .iter()
            .filter(|t| !t.closed)
            .map(|t| t.view.clone())
            .collect();
        out.sort_by_key(|v| v.id);
        out
    }

    fn all_done(&self) -> bool {
        self.windows.iter().all(|t| t.done || t.closed)
    }
}

// ---- dispatch ----

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for Toplevels {
    fn event(
        _: &mut Self,
        _: &wl_registry::WlRegistry,
        _: wl_registry::Event,
        _: &GlobalListContents,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ZwlrForeignToplevelManagerV1, ()> for Toplevels {
    fn event(
        state: &mut Self,
        _: &ZwlrForeignToplevelManagerV1,
        event: zwlr_foreign_toplevel_manager_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            zwlr_foreign_toplevel_manager_v1::Event::Toplevel { toplevel } => {
                let id = toplevel.id().protocol_id();
                state.windows.push(Toplevel {
                    handle: toplevel,
                    view: ToplevelView {
                        id,
                        app_id: None,
                        title: None,
                        activated: false,
                        minimized: false,
                    },
                    done: false,
                    closed: false,
                });
            }
            zwlr_foreign_toplevel_manager_v1::Event::Finished => state.finished = true,
            _ => {}
        }
    }

    // The `toplevel` event carries a server-created object, so wayland-client
    // needs to be told what user data to give it. Without this the manager
    // does not compile at all.
    event_created_child!(Toplevels, ZwlrForeignToplevelManagerV1, [
        zwlr_foreign_toplevel_manager_v1::EVT_TOPLEVEL_OPCODE => (ZwlrForeignToplevelHandleV1, ()),
    ]);
}

impl Dispatch<ZwlrForeignToplevelHandleV1, ()> for Toplevels {
    fn event(
        state: &mut Self,
        proxy: &ZwlrForeignToplevelHandleV1,
        event: zwlr_foreign_toplevel_handle_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let id = proxy.id().protocol_id();
        let Some(top) = state.find_mut(id) else {
            return;
        };
        match event {
            zwlr_foreign_toplevel_handle_v1::Event::Title { title } => {
                top.view.title = Some(title);
            }
            zwlr_foreign_toplevel_handle_v1::Event::AppId { app_id } => {
                top.view.app_id = Some(app_id);
            }
            // The array is the COMPLETE state, not a delta, so both flags
            // are recomputed from scratch on every event.
            zwlr_foreign_toplevel_handle_v1::Event::State { state: bytes } => {
                let flags = decode_state(&bytes);
                top.view.activated = flags.contains(&ST_ACTIVATED);
                top.view.minimized = flags.contains(&ST_MINIMIZED);
            }
            zwlr_foreign_toplevel_handle_v1::Event::Done => top.done = true,
            zwlr_foreign_toplevel_handle_v1::Event::Closed => top.closed = true,
            _ => {}
        }
    }
}

wayland_client::delegate_noop!(Toplevels: ignore wl_seat::WlSeat);

/// The `state` event's argument is a wayland array of `uint`: little-endian,
/// four bytes each. A trailing partial word (which no compositor should
/// send) is dropped rather than misread.
/// `as_chunks` rather than `chunks_exact(4)`: clippy 1.98 rejects a constant
/// chunk size on a slice (`chunks_exact_to_as_chunks`), and this form hands
/// `from_ne_bytes` a `[u8; 4]` instead of indexing one out by hand. Stable
/// since 1.88.0 — which is exactly this workspace's `rust-version` floor, so
/// it costs nothing there either.
pub(crate) fn decode_state(bytes: &[u8]) -> Vec<u32> {
    bytes
        .as_chunks::<4>()
        .0
        .iter()
        .copied()
        .map(u32::from_ne_bytes)
        .collect()
}

// ---- session ----

/// One wayland connection with the manager and a seat bound on it. Held for
/// the life of the backend so that the object ids handed out as addresses
/// stay valid between reading the window list and acting on it — a second
/// connection would renumber every handle.
struct Session {
    queue: EventQueue<Toplevels>,
    state: Toplevels,
    seat: wl_seat::WlSeat,
    // Dropping the connection invalidates every proxy above, so it is kept
    // even though nothing reads it directly.
    _conn: Connection,
    _manager: ZwlrForeignToplevelManagerV1,
}

impl Session {
    /// Connect, bind, and read the initial burst.
    ///
    /// The bind IS the probe: `GlobalList::bind` fails with `NotPresent` on
    /// a compositor that does not implement the protocol, which is exactly
    /// the question `pick_backend` needs answered and the only one it can
    /// answer without a hardcoded list of compositor names.
    fn open() -> Result<Self> {
        let conn = Connection::connect_to_env().map_err(|e| {
            BackendError::Ipc(format!("cannot connect to the wayland display: {e}"))
        })?;
        let (globals, queue) = registry_queue_init::<Toplevels>(&conn)
            .map_err(|e| BackendError::Ipc(format!("wayland registry: {e}")))?;
        let qh = queue.handle();

        let manager: ZwlrForeignToplevelManagerV1 = globals.bind(&qh, 1..=3, ()).map_err(|e| {
            BackendError::UnsupportedEnvironment(format!(
                "this compositor does not implement zwlr_foreign_toplevel_manager_v1: {e}"
            ))
        })?;
        // `activate` takes a seat, so a session with no seat can list
        // windows but could never focus one. Fail here rather than at the
        // keypress.
        let seat: wl_seat::WlSeat = globals
            .bind(&qh, 1..=1, ())
            .map_err(|e| BackendError::Ipc(format!("no wl_seat to activate windows with: {e}")))?;

        let mut session = Session {
            queue,
            state: Toplevels::default(),
            seat,
            _conn: conn,
            _manager: manager,
        };
        session.sync()?;
        Ok(session)
    }

    /// Bring `state` up to date.
    ///
    /// One roundtrip is already sufficient and the loop is belt-and-braces:
    /// the bind request and the sync request leave in the same write, and
    /// wayland processes a client's requests in order, so every `toplevel`
    /// event the compositor queues in response to the bind is written before
    /// the sync callback it answers with. The `done` gate is the protocol's
    /// own "this toplevel's state is complete" signal, so looping on it
    /// costs nothing when the first pass was enough — which is every time so
    /// far — and cannot spin: the bound is three.
    fn sync(&mut self) -> Result<()> {
        for _ in 0..3 {
            self.queue
                .roundtrip(&mut self.state)
                .map_err(|e| BackendError::Ipc(format!("wayland roundtrip: {e}")))?;
            if self.state.finished {
                return Err(BackendError::Ipc(
                    "the compositor withdrew zwlr_foreign_toplevel_manager_v1".to_string(),
                ));
            }
            if self.state.all_done() {
                break;
            }
        }
        Ok(())
    }

    /// Send the queued requests and wait for the compositor to have handled
    /// them.
    ///
    /// A flush would only prove the socket accepted the bytes. beckon exits
    /// immediately after this returns, and a client that disconnects is not
    /// owed processing of what it left in flight — so the roundtrip is the
    /// part that makes the focus actually happen. Same lesson as `x11.rs`'s
    /// `ensure_mapped`, one layer down: there the window manager was a
    /// second client, here the compositor is the server, so a sync answers
    /// the question outright.
    fn commit(&mut self) -> Result<()> {
        self.queue
            .roundtrip(&mut self.state)
            .map_err(|e| BackendError::Ipc(format!("wayland roundtrip: {e}")))?;
        Ok(())
    }

    /// Focus one window, un-minimising it first when it is minimised.
    ///
    /// The unminimise is not optional and not merely defensive: step 5c
    /// parks a window with `set_minimized`, so the very next press on the
    /// same binding is the request that has to bring it back. Requests are
    /// ordered on the wire, so the pair needs no sync between its halves —
    /// unlike the X11 map/activate pair, where the WM is a separate client
    /// and the race is real.
    fn activate(&mut self, id: u32) -> Result<()> {
        let seat = self.seat.clone();
        let top = self
            .state
            .find_mut(id)
            .ok_or_else(|| BackendError::WindowNotFound(id.to_string()))?;
        if top.view.minimized {
            top.handle.unset_minimized();
        }
        top.handle.activate(&seat);
        self.commit()
    }

    fn minimize(&mut self, id: u32) -> Result<()> {
        let top = self
            .state
            .find_mut(id)
            .ok_or_else(|| BackendError::WindowNotFound(id.to_string()))?;
        top.handle.set_minimized();
        self.commit()
    }
}

pub struct WlrootsBackend {
    // `Backend` takes `&self`; the wayland queue needs `&mut`. One-shot
    // process, single thread, so a RefCell is the whole of the story.
    session: RefCell<Session>,
}

impl WlrootsBackend {
    pub fn new() -> Result<Self> {
        Ok(Self {
            session: RefCell::new(Session::open()?),
        })
    }
}

// ---- projection (pure) ----

/// Build the neutral snapshot list the shared algorithm consumes.
///
/// Windows without an `app_id` are skipped, exactly as `i3ipc.rs` skips
/// windows with neither `app_id` nor class and as `niri.rs` skips a `null`
/// `app_id`: they are transient chrome, and letting one through makes step
/// 5b toggle to something the compositor will not focus.
///
/// The activated window is hoisted to recency 0 and the rest keep
/// announcement order. That is the whole of the MRU this protocol can
/// support — see the module docs.
pub(crate) fn snapshots_from(views: &[ToplevelView]) -> Vec<WindowSnapshot> {
    let mut vs: Vec<&ToplevelView> = views.iter().filter(|v| v.app_id.is_some()).collect();
    vs.sort_by(|a, b| b.activated.cmp(&a.activated).then_with(|| a.id.cmp(&b.id)));
    vs.iter()
        .enumerate()
        .map(|(idx, v)| {
            WindowSnapshot::new(v.id.to_string(), v.app_id.clone().unwrap(), idx as i32)
        })
        .collect()
}

/// Parse a snapshot address back into the wayland object id it was minted
/// from. Surfaced as an IPC error rather than a panic, like `parse_con_id`
/// in `i3ipc.rs` and `parse_window_id` in `niri.rs`.
fn parse_object_id(addr: &str) -> Result<u32> {
    addr.parse::<u32>()
        .map_err(|e| BackendError::Ipc(format!("bad toplevel id `{addr}`: {e}")))
}

/// Same recipe as `niri.rs` / `mango.rs` / `x11.rs`: the protocol has no
/// exec action, so launch through `setsid -f` and let the app outlive
/// beckon.
fn launch_exec(id: &str, exec: &str) -> Result<()> {
    use std::process::{Command, Stdio};
    Command::new("/bin/sh")
        .arg("-c")
        .arg(format!("setsid -f {exec} >/dev/null 2>&1"))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| BackendError::LaunchFailed {
            id: id.to_string(),
            reason: e.to_string(),
        })?;
    Ok(())
}

fn persist_previous(app: Option<&str>) {
    if let Some(a) = app {
        crate::state::write_previous(a);
    }
}

impl Backend for WlrootsBackend {
    fn beckon(&self, id: &str) -> Result<BeckonAction> {
        let mut session = self.session.borrow_mut();
        // The session was opened when the backend was constructed; refresh
        // it so the list is as fresh as the protocol allows.
        session.sync()?;
        let views = session.state.views();

        let snapshots = snapshots_from(&views);
        let active = views
            .iter()
            .find(|v| v.activated && v.app_id.is_some())
            .map(|v| v.id.to_string());

        // What is focused right now, before any action — this becomes
        // "previous" for the next call's step 5b, as in every other backend.
        let pre_focused_app = views
            .iter()
            .find(|v| v.activated)
            .and_then(|v| v.app_id.clone());

        let previous_app = crate::state::read_previous();

        let entry = crate::desktop::resolve(id);
        let target = crate::desktop::target_classes(entry.as_ref(), id);

        let decision = decide(
            &snapshots,
            active.as_deref(),
            target,
            previous_app.as_deref(),
        );

        let action = match decision {
            Decision::Launch => {
                let entry = entry.ok_or_else(|| BackendError::NoMatch {
                    id: id.to_string(),
                    hint: format!(
                        "no .desktop entry matches `{id}` and no running window has that app_id. \
                         Run `beckon installed` to list installed apps, \
                         or `beckon search {id}` to search.",
                    ),
                })?;
                launch_exec(id, &entry.exec)?;
                BeckonAction::Launched
            }
            Decision::Focus(addr) => {
                session.activate(parse_object_id(&addr)?)?;
                BeckonAction::Focused
            }
            Decision::Cycle(addr) => {
                session.activate(parse_object_id(&addr)?)?;
                BeckonAction::Cycled
            }
            Decision::ToggleBack(addr) => {
                session.activate(parse_object_id(&addr)?)?;
                BeckonAction::ToggledBack
            }
            Decision::Hide(addr) => {
                session.minimize(parse_object_id(&addr)?)?;
                BeckonAction::Hidden
            }
        };

        persist_previous(pre_focused_app.as_deref());
        Ok(action)
    }

    fn list_running(&self) -> Result<Vec<RunningApp>> {
        let mut session = self.session.borrow_mut();
        session.sync()?;

        let mut by_id: BTreeMap<String, (String, usize)> = BTreeMap::new();
        for v in session.state.views() {
            let Some(app_id) = v.app_id else { continue };
            let entry = by_id
                .entry(app_id)
                .or_insert_with(|| (v.title.clone().unwrap_or_default(), 0));
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
        // Delegated: the catalog is `.desktop` files on disk and has nothing
        // to do with which compositor is running. This was eight identical
        // copies, one per backend, and `beckon installed` took a backend only
        // to reach one of them -- which made it fail outright over SSH.
        crate::list_installed()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(id: u32, app_id: Option<&str>, activated: bool) -> ToplevelView {
        ToplevelView {
            id,
            app_id: app_id.map(String::from),
            title: None,
            activated,
            minimized: false,
        }
    }

    /// The `state` array is little-endian `uint`s. Reading it as bytes, or
    /// as big-endian, silently reports every window as neither activated nor
    /// minimized — which looks exactly like "no window is focused" and sends
    /// step 4 to focus a window that is already focused.
    #[test]
    fn state_array_decodes_as_native_endian_u32s() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&ST_ACTIVATED.to_ne_bytes());
        bytes.extend_from_slice(&ST_MINIMIZED.to_ne_bytes());
        assert_eq!(decode_state(&bytes), vec![ST_ACTIVATED, ST_MINIMIZED]);
        // Empty state — every flag off — is the common case and must not
        // be confused with a parse failure.
        assert_eq!(decode_state(&[]), Vec::<u32>::new());
        // A trailing partial word is dropped, never misread.
        assert_eq!(decode_state(&[1, 0, 0]), Vec::<u32>::new());
    }

    /// The two values this file reads are distinct and are the ones the
    /// protocol assigns. A renumbering upstream would make `activated` and
    /// `minimized` swap silently.
    #[test]
    fn the_two_state_values_are_the_protocols_own() {
        assert_eq!(ST_MINIMIZED, 1);
        assert_eq!(ST_ACTIVATED, 2);
    }

    #[test]
    fn snapshots_hoist_the_activated_window_and_keep_announcement_order() {
        let views = vec![
            v(10, Some("kitty"), false),
            v(11, Some("claude"), true),
            v(12, Some("firefox"), false),
        ];
        let snaps = snapshots_from(&views);
        assert_eq!(snaps[0].address, "11", "activated window is recency 0");
        assert_eq!(snaps[0].recency, 0);
        assert_eq!(snaps[1].address, "10");
        assert_eq!(snaps[2].address, "12");
    }

    #[test]
    fn snapshots_skip_toplevels_without_an_app_id() {
        let views = vec![v(10, Some("kitty"), false), v(11, None, true)];
        let snaps = snapshots_from(&views);
        assert_eq!(snaps.len(), 1);
        assert_eq!(snaps[0].address, "10");
    }

    /// This protocol cannot see the `WM_CLASS` instance half at all, so the
    /// field must stay `None` — the value that means "this backend cannot
    /// see it" and degrades to the behaviour before the field existed.
    #[test]
    fn snapshots_never_claim_a_wm_class_instance() {
        let snaps = snapshots_from(&[v(10, Some("Brave-browser"), true)]);
        assert_eq!(snaps[0].instance, None);
    }

    /// The one property the cycle ring actually needs from an address, given
    /// that this backend has no focus history: an unchanged set of toplevels
    /// rotates through every window once per lap. `decide` orders the ring by
    /// address, and these addresses are object ids, so this holds even though
    /// `recency` reshuffles on every press.
    #[test]
    fn cycling_reaches_every_window_with_no_focus_history() {
        let ids = [10u32, 11, 12];
        let mut focused = 10u32;
        let mut visited = vec![focused];
        for _ in 0..5 {
            let views: Vec<ToplevelView> = ids
                .iter()
                .map(|&i| v(i, Some("claude"), i == focused))
                .collect();
            match decide(
                &snapshots_from(&views),
                Some(&focused.to_string()),
                "claude",
                None,
            ) {
                Decision::Cycle(next) => {
                    focused = next.parse().unwrap();
                    visited.push(focused);
                }
                other => panic!("expected Cycle, got {other:?}"),
            }
        }
        assert_eq!(visited, vec![10, 11, 12, 10, 11, 12]);
    }

    #[test]
    fn a_bad_address_is_an_error_not_a_panic() {
        assert!(parse_object_id("not-an-id").is_err());
        assert_eq!(parse_object_id("4278190081").unwrap(), 4_278_190_081);
    }
}
