//! niri Wayland backend over the official IPC socket (`NIRI_SOCKET`).
//!
//! Wire protocol (measured on niri 26.04, 2026-08-22): one JSON line per
//! request, one JSON line per reply, wrapper `{"Ok":T}` / `{"Err":"..."}`.
//! Unit variants are sent as `null` (`{"Windows":null}`), never `{}` — the
//! latter is a parse error.
//!
//! Hand-rolled rather than the official `niri-ipc` crate: that crate is
//! GPL-3.0-or-later while beckon is MIT OR Apache-2.0 and ships prebuilt
//! binaries; serde_json is already a dependency and the used surface is
//! four requests.
//!
//! Traps measured on 26.04, kept here so nobody re-measures them:
//!   - `FocusWindow` with an unknown id still replies `{"Ok":"Handled"}` —
//!     the reply is NOT proof of focus.
//!   - No minimize/scratchpad action exists (three spellings all `Err`).
//!     Step 5c therefore moves the window to a far-index workspace with
//!     `focus:false`: it leaves the view but stays alive, and a later
//!     `FocusWindow` navigates to its workspace — self-unparking, unlike
//!     Hyprland's `special:beckon` which needs an explicit move back.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};

use beckon_core::{BackendError, Result};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::algorithm::WindowSnapshot;

/// Step 5c target: far enough that no real session reaches it. niri creates
/// the workspace on demand and FocusWindow later navigates back to it.
const HIDE_WORKSPACE_INDEX: u64 = 1_000_000;

pub struct NiriBackend;

impl NiriBackend {
    pub fn new() -> Result<Self> {
        // Verify the socket answers at construction so a dead or foreign
        // NIRI_SOCKET errors before any user-facing action — same shape as
        // I3IpcBackend::new.
        let _: VersionReply = call_env(req_version())?;
        Ok(Self)
    }
}

// ---- wire types ----

#[derive(Deserialize)]
struct VersionReply {
    Version: String,
}

#[derive(Deserialize, Debug)]
struct WindowsReply {
    Windows: Vec<NiriWindow>,
}

#[derive(Deserialize, Debug, Clone)]
pub(crate) struct NiriWindow {
    pub id: u64,
    /// niri exposes a single class-like field; windows without one are
    /// skipped exactly as i3ipc skips windows with neither app_id nor class.
    pub app_id: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub is_focused: bool,
    #[serde(default)]
    pub focus_timestamp: Option<FocusTimestamp>,
}

/// `SystemTime` as niri serializes it. Field order makes the derived
/// `Ord` compare seconds first — exactly the ordering recency needs.
#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct FocusTimestamp {
    pub secs: i64,
    pub nanos: u32,
}

/// Reply envelope. `#[serde(untagged)]` tries `Ok` first; an `Err` reply
/// carries the compositor's message string.
#[derive(Deserialize)]
#[serde(untagged)]
enum Reply<T> {
    Ok { Ok: T },
    Err { Err: String },
}

// ---- requests (exact wire shapes; unit-tested as strings) ----

fn req_version() -> Value {
    json!({ "Version": null })
}

fn req_windows() -> Value {
    json!({ "Windows": null })
}

fn req_focus(id: u64) -> Value {
    json!({ "Action": { "FocusWindow": { "id": id } } })
}

fn req_move_to_workspace(window_id: u64, index: u64) -> Value {
    json!({ "Action": { "MoveWindowToWorkspace": {
        "window_id": window_id,
        "reference": { "Index": index },
        "focus": false
    } } })
}

// ---- framing ----

fn socket_path() -> Result<PathBuf> {
    // NIRI_SOCKET is the only source of truth (niri exports it to children).
    // Never glob $XDG_RUNTIME_DIR/niri.*.sock — nested instances are real.
    std::env::var_os("NIRI_SOCKET")
        .map(PathBuf::from)
        .ok_or_else(|| BackendError::Ipc("NIRI_SOCKET is not set".to_string()))
}

/// One request, one fresh connection, one reply line. niri keeps a socket
/// open per client, but beckon's calls are one-shot; a fresh connection per
/// request sidesteps any read-buffering question entirely.
fn call<T: DeserializeOwned>(path: &Path, req: Value) -> Result<T> {
    let mut stream = UnixStream::connect(path)
        .map_err(|e| BackendError::Ipc(format!("connect {}: {e}", path.display())))?;
    let mut line = serde_json::to_string(&req)
        .map_err(|e| BackendError::Ipc(format!("serialize request: {e}")))?;
    line.push('\n');
    stream
        .write_all(line.as_bytes())
        .map_err(|e| BackendError::Ipc(format!("write request: {e}")))?;
    stream
        .flush()
        .map_err(|e| BackendError::Ipc(format!("flush request: {e}")))?;

    let mut reader = BufReader::new(stream);
    let mut buf = String::new();
    reader
        .read_line(&mut buf)
        .map_err(|e| BackendError::Ipc(format!("read reply: {e}")))?;
    let reply: Reply<T> =
        serde_json::from_str(buf.trim()).map_err(|e| BackendError::Ipc(format!(
            "bad reply `{}`: {e}",
            buf.trim()
        )))?;
    match reply {
        Reply::Ok { Ok: ok } => Ok(ok),
        Reply::Err { Err: err } => Err(BackendError::Ipc(err)),
    }
}

fn call_env<T: DeserializeOwned>(req: Value) -> Result<T> {
    call(&socket_path()?, req)
}

/// Build the neutral snapshot list the shared algorithm consumes. Sorted by
/// `focus_timestamp`, newest first, BEFORE numbering recency: that field is
/// real focus history (sway only has tree order), so step 4 and step 5b pick
/// the window/app the user actually left. Ties break by id for determinism.
fn snapshots_from(windows: &[NiriWindow]) -> Vec<WindowSnapshot> {
    let mut ws: Vec<&NiriWindow> = windows.iter().filter(|w| w.app_id.is_some()).collect();
    ws.sort_by(|a, b| {
        b.focus_timestamp
            .cmp(&a.focus_timestamp)
            .then_with(|| a.id.cmp(&b.id))
    });
    ws.iter()
        .enumerate()
        .map(|(idx, w)| WindowSnapshot::new(w.id.to_string(), w.app_id.clone().unwrap(), idx as i32))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::os::unix::net::UnixListener;
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    use std::sync::{Arc, Mutex};
    use std::thread;

    /// Serve one canned reply per connection, recording each request line.
    /// In-process twin of the e2e FakeServer: enough framing to exercise
    /// `call` end-to-end without touching `NIRI_SOCKET`.
    struct TestSocket {
        path: PathBuf,
        _requests: Arc<Mutex<Vec<String>>>,
        stop: Arc<AtomicBool>,
    }

    impl TestSocket {
        fn start(replies: Vec<String>) -> Self {
            static COUNTER: AtomicU64 = AtomicU64::new(0);
            let n = COUNTER.fetch_add(1, Ordering::Relaxed);
            let dir = std::env::temp_dir().join(format!(
                "beckon-niri-unit-{}-{}-{}",
                std::process::id(),
                thread::current().name().unwrap_or("t").replace('/', "_"),
                n
            ));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            let path = dir.join("test.sock");
            let listener = UnixListener::bind(&path).unwrap();
            listener.set_nonblocking(true).unwrap();

            let requests: Arc<Mutex<Vec<String>>> = Arc::default();
            let stop = Arc::new(AtomicBool::new(false));

            let s_requests = requests.clone();
            let s_stop = stop.clone();
            thread::spawn(move || {
                for reply in replies.into_iter().cycle() {
                    if s_stop.load(Ordering::Relaxed) {
                        return;
                    }
                    match listener.accept() {
                        Ok((mut stream, _)) => {
                            stream.set_read_timeout(Some(std::time::Duration::from_millis(500))).ok();
                            let mut buf = String::new();
                            use std::io::Read;
                            if stream.read_to_string(&mut buf).is_err() && buf.is_empty() {
                                continue;
                            }
                            s_requests.lock().unwrap().push(buf.trim().to_string());
                            use std::io::Write;
                            let _ = stream.write_all(reply.as_bytes());
                        }
                        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                            thread::sleep(std::time::Duration::from_millis(2));
                        }
                        Err(_) => return,
                    }
                }
            });

            Self { path, _requests: requests, stop }
        }
    }

    impl Drop for TestSocket {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::Relaxed);
        }
    }

    #[test]
    fn requests_serialize_exactly_as_measured() {
        assert_eq!(
            serde_json::to_string(&req_version()).unwrap(),
            r#"{"Version":null}"#
        );
        assert_eq!(
            serde_json::to_string(&req_windows()).unwrap(),
            r#"{"Windows":null}"#
        );
        // Unit-variant null is the one byte-level contract ({} is a parse
        // error), so those two pin the exact string. The Action payloads
        // compare as Values: serde_json orders object keys alphabetically
        // and niri parses with serde, so key order is not part of the wire
        // contract — shape is.
        assert_eq!(
            serde_json::from_str::<Value>(&serde_json::to_string(&req_focus(2)).unwrap()).unwrap(),
            serde_json::json!({ "Action": { "FocusWindow": { "id": 2 } } })
        );
        assert_eq!(
            serde_json::from_str::<Value>(
                &serde_json::to_string(&req_move_to_workspace(7, 1_000_000)).unwrap()
            )
            .unwrap(),
            serde_json::json!({ "Action": { "MoveWindowToWorkspace": {
                "window_id": 7, "reference": { "Index": 1_000_000 }, "focus": false } } })
        );
    }

    #[test]
    fn measured_window_fixture_parses() {
        // Verbatim Window object from `{"Windows":null}` on niri 26.04.
        let raw = r#"{"id":2,"title":"kitty","app_id":"kitty","pid":220493,
            "workspace_id":1,"is_focused":true,"is_floating":false,"is_urgent":false,
            "layout":{"pos_in_scrolling_layout":[1,1],"tile_size":[960.0,2072.0],
                      "window_size":[960,2072],"tile_pos_in_workspace_view":null,
                      "window_offset_in_tile":[0.0,0.0]},
            "focus_timestamp":{"secs":62743,"nanos":114657837}}"#;
        let w: NiriWindow = serde_json::from_str(raw).unwrap();
        assert_eq!(w.id, 2);
        assert_eq!(w.app_id.as_deref(), Some("kitty"));
        assert!(w.is_focused);
        assert_eq!(w.focus_timestamp.map(|t| t.secs), Some(62743));
    }

    #[test]
    fn err_reply_maps_to_ipc_error() {
        // What a malformed spelling gets back (`{"Windows":{}}`, measured).
        // The unit variant MUST be sent as null; this pins what happens to
        // the Err arm when it is not.
        let srv = TestSocket::start(vec![r#"{"Err":"error parsing request"}"#.into()]);
        let r: Result<WindowsReply> = call(srv.path.as_path(), req_windows());
        let err = r.unwrap_err().to_string();
        assert!(err.contains("error parsing request"), "got: {err}");
    }

    #[test]
    fn ok_reply_unwraps_the_inner_value() {
        let srv = TestSocket::start(vec![r#"{"Ok":{"Version":"26.04 (Nixpkgs)"}}"#.into()]);
        let v: VersionReply = call(srv.path.as_path(), req_version()).unwrap();
        assert_eq!(v.Version, "26.04 (Nixpkgs)");
    }

    #[test]
    fn snapshots_sort_by_focus_timestamp_and_skip_missing_app_id() {
        // focus_timestamp is niri's REAL focus order — sort desc before
        // numbering recency so step 5b lands on the app the user actually
        // left (sway only has tree order; this is niri's MRU upgrade).
        let mk = |id: u64, app: Option<&str>, secs: i64| NiriWindow {
            id,
            app_id: app.map(String::from),
            title: None,
            is_focused: false,
            focus_timestamp: Some(FocusTimestamp { secs, nanos: 0 }),
        };
        let windows = vec![
            mk(1, Some("kitty"), 100),
            mk(2, None, 200),
            mk(3, Some("claude"), 300),
        ];
        let snaps = snapshots_from(&windows);
        assert_eq!(snaps.len(), 2, "window without app_id is skipped like i3ipc");
        assert_eq!(snaps[0].address, "3", "newest focus_timestamp gets recency 0");
        assert_eq!(snaps[0].recency, 0);
        assert_eq!(snaps[1].address, "1");
        assert_eq!(snaps[1].recency, 1);
    }
}
