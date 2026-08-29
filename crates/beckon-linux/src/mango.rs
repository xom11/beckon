//! Mangowm (mango) Wayland backend over a newline-delimited text IPC socket.
//!
//! Wire format measured on mangowm 0.16.1 (2026-08-29): one text command per
//! line (e.g. `get all-clients\n`), one JSON object per reply, server closes
//! the connection after writing. Mango does not wrap successes in `Ok` —
//! plain JSON, errors come as `{"error":"..."}` and an early-`closed` stream
//! is also a refusal (no separate status).
//!
//! Mango sets `MANGO_INSTANCE_SIGNATURE` in child processes' environments to
//! the absolute path of its IPC socket, exactly the way sway sets `SWAYSOCK`.
//! Nested instances are real (no `unique-instance` check), so the env var is
//! the only source of truth — never glob for `mango-*.sock` in
//! `$XDG_RUNTIME_DIR`.
//!
//! Hand-rolled rather than pulling in a crate: mangowm has no published IPC
//! crate, serde_json is already a dependency, and the used surface is four
//! commands.
//!
//! Traps measured on 0.16.1, kept here so nobody re-measures them:
//!   - Replies are PLAIN JSON, not `{"Ok":..}` like niri — niri-shaped
//!     parsing silently returns empty results.
//!   - `get all-clients` returns a top-level array, but `get all-monitors`
//!     wraps its list in `{"monitors": [...]}` (different shape per command).
//!   - `dispatch` is text: `dispatch view,<idx>,0` — the third positional
//!     is `synctag` (0 = off), required by the parser; passing only two
//!     fields gives `Ok` but does nothing.
//!   - No `exec` action: launch through `setsid -f` (same recipe as niri).
//!   - `appid` is wayland `app_id`, comparable to .desktop filename stem
//!     on Linux.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};

use beckon_core::{Backend, BackendError, BeckonAction, InstalledApp, Result, RunningApp};
use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::Value;

use crate::algorithm::{decide, Decision, WindowSnapshot};

pub struct MangoBackend;

impl MangoBackend {
    pub fn new() -> Result<Self> {
        // Verify the socket answers at construction so a dead or foreign
        // MANGO_INSTANCE_SIGNATURE errors before any user-facing action. The
        // version is read and dropped — a parse here IS the probe.
        let v: VersionReply = call_env(req_version())?;
        drop(v.version);
        Ok(Self)
    }
}

// ---- wire types ----

#[derive(Deserialize, Debug)]
struct VersionReply {
    version: String,
}

#[derive(Deserialize, Debug)]
struct ClientsReply(Vec<MangoClient>);

#[derive(Deserialize, Debug, Clone)]
struct MangoClient {
    id: u64,
    /// wayland app_id; windows without one are skipped, like i3ipc and niri.
    #[serde(default)]
    appid: Option<String>,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    is_focused: bool,
}

// ---- requests (text lines) ----

fn req_version() -> &'static str {
    "get version"
}

fn req_all_clients() -> &'static str {
    "get all-clients"
}

fn req_focusid(client_id: u64) -> String {
    format!("dispatch focusid client,{client_id}")
}

fn req_minimized() -> &'static str {
    "dispatch minimized"
}

// ---- framing ----

fn socket_path() -> Result<PathBuf> {
    std::env::var_os("MANGO_INSTANCE_SIGNATURE")
        .map(PathBuf::from)
        .ok_or_else(|| BackendError::Ipc("MANGO_INSTANCE_SIGNATURE is not set".to_string()))
}

/// One request, one fresh connection, one JSON reply, then EOF. Mango
/// closes the socket after each reply; a fresh connection per request
/// sidesteps any read-buffering question.
fn call<T: DeserializeOwned>(path: &Path, req: &str) -> Result<T> {
    let mut stream = UnixStream::connect(path)
        .map_err(|e| BackendError::Ipc(format!("connect {}: {e}", path.display())))?;
    let mut line = req.to_string();
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
    let trimmed = buf.trim();
    // Mango surfaces errors as {"error": "..."} with a 200 reply — distinct
    // from the success shape so a single match catches both.
    if let Ok(v) = serde_json::from_str::<Value>(trimmed) {
        if let Some(err) = v.get("error").and_then(Value::as_str) {
            return Err(BackendError::Ipc(err.to_string()));
        }
    }
    serde_json::from_str(trimmed)
        .map_err(|e| BackendError::Ipc(format!("bad reply `{trimmed}`: {e}")))
}

fn call_env<T: DeserializeOwned>(req: &str) -> Result<T> {
    call(&socket_path()?, req)
}

// ---- snapshots ----

fn snapshots_from(clients: &[MangoClient]) -> Vec<WindowSnapshot> {
    let mut cs: Vec<&MangoClient> = clients.iter().filter(|c| c.appid.is_some()).collect();
    // Mango has no focus_timestamp (only `is_focused`), so MRU = the order
    // `is_focused` then any stable order. With one focused window at a time,
    // this gives step 5b a stable recency order: focused first, then the
    // rest in id order.
    cs.sort_by(|a, b| {
        b.is_focused
            .cmp(&a.is_focused)
            .then_with(|| a.id.cmp(&b.id))
    });
    cs.iter()
        .enumerate()
        .map(|(idx, c)| WindowSnapshot::new(c.id.to_string(), c.appid.clone().unwrap(), idx as i32))
        .collect()
}

fn parse_client_id(addr: &str) -> Result<u64> {
    addr.parse::<u64>()
        .map_err(|e| BackendError::Ipc(format!("bad client id `{addr}`: {e}")))
}

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

impl Backend for MangoBackend {
    fn beckon(&self, id: &str) -> Result<BeckonAction> {
        let reply: ClientsReply = call_env(req_all_clients())?;
        let clients = reply.0;

        let snapshots = snapshots_from(&clients);
        let active = clients
            .iter()
            .find(|c| c.is_focused)
            .map(|c| c.id.to_string());

        let pre_focused_app = clients
            .iter()
            .find(|c| c.is_focused)
            .and_then(|c| c.appid.clone());

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
                let _: Value = call_env(&req_focusid(parse_client_id(&addr)?))?;
                BeckonAction::Focused
            }
            Decision::Cycle(addr) => {
                let _: Value = call_env(&req_focusid(parse_client_id(&addr)?))?;
                BeckonAction::Cycled
            }
            Decision::ToggleBack(addr) => {
                let _: Value = call_env(&req_focusid(parse_client_id(&addr)?))?;
                BeckonAction::ToggledBack
            }
            Decision::Hide(addr) => {
                // mango minimised has no client-id form; the focused window
                // is the one the user wants to hide in step 5c. Take the
                // current focus as the address — same effect as niri's
                // `MoveWindowToWorkspace` because step 5c only fires when
                // the user is already looking at the target app.
                let _: Value = call_env(req_minimized())?;
                let _ = addr; // intentionally unused
                BeckonAction::Hidden
            }
        };

        persist_previous(pre_focused_app.as_deref());
        Ok(action)
    }

    fn list_running(&self) -> Result<Vec<RunningApp>> {
        let reply: ClientsReply = call_env(req_all_clients())?;

        let mut by_id: std::collections::BTreeMap<String, (String, usize)> = Default::default();
        for c in reply.0.into_iter().filter(|c| c.appid.is_some()) {
            let entry = by_id
                .entry(c.appid.unwrap())
                .or_insert_with(|| (c.title.unwrap_or_default(), 0));
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

    use std::io::Read;
    use std::os::unix::net::UnixListener;
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    use std::sync::{Arc, Mutex};
    use std::thread;

    /// One request, one JSON reply per connection, recording what the
    /// client wrote. In-process twin of the e2e fake — enough to exercise
    /// `call` end-to-end without touching `MANGO_INSTANCE_SIGNATURE`.
    struct TestSocket {
        path: PathBuf,
        received: Arc<Mutex<Vec<String>>>,
        stop: Arc<AtomicBool>,
    }

    impl TestSocket {
        fn start(replies: Vec<String>) -> Self {
            static COUNTER: AtomicU64 = AtomicU64::new(0);
            let n = COUNTER.fetch_add(1, Ordering::Relaxed);
            let dir = std::env::temp_dir().join(format!(
                "beckon-mango-unit-{}-{}-{}",
                std::process::id(),
                thread::current().name().unwrap_or("t").replace('/', "_"),
                n
            ));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            let path = dir.join("test.sock");
            let listener = UnixListener::bind(&path).unwrap();
            listener.set_nonblocking(true).unwrap();

            let received: Arc<Mutex<Vec<String>>> = Arc::default();
            let stop = Arc::new(AtomicBool::new(false));

            let s_recv = received.clone();
            let s_stop = stop.clone();
            thread::spawn(move || {
                for reply in replies.into_iter().cycle() {
                    if s_stop.load(Ordering::Relaxed) {
                        return;
                    }
                    match listener.accept() {
                        Ok((mut stream, _)) => {
                            stream
                                .set_read_timeout(Some(std::time::Duration::from_millis(500)))
                                .ok();
                            let mut buf = String::new();
                            if stream.read_to_string(&mut buf).is_err() && buf.is_empty() {
                                continue;
                            }
                            s_recv.lock().unwrap().push(buf.trim().to_string());
                            let _ = stream.write_all(reply.as_bytes());
                            let _ = stream.write_all(b"\n");
                        }
                        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                            thread::sleep(std::time::Duration::from_millis(2));
                        }
                        Err(_) => return,
                    }
                }
            });

            Self {
                path,
                received,
                stop,
            }
        }

        fn received(&self) -> Vec<String> {
            self.received.lock().unwrap().clone()
        }
    }

    impl Drop for TestSocket {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::Relaxed);
        }
    }

    #[test]
    fn request_lines_match_measured_protocol() {
        // Mangowm's text IPC: each request is one lowercase text line +
        // space-separated args, terminated by \n. These pin the exact
        // bytes that go on the wire.
        assert_eq!(req_version(), "get version");
        assert_eq!(req_all_clients(), "get all-clients");
        assert_eq!(req_focusid(7), "dispatch focusid client,7");
        assert_eq!(req_minimized(), "dispatch minimized");
    }

    #[test]
    fn call_writes_line_and_reads_one_reply() {
        let srv = TestSocket::start(vec![r#"{"version":"0.16.1"}"#.into()]);
        let v: VersionReply = call(srv.path.as_path(), req_version()).unwrap();
        assert_eq!(v.version, "0.16.1");
        assert_eq!(srv.received(), vec!["get version".to_string()]);
    }

    #[test]
    fn call_maps_error_object_to_ipc_error() {
        let srv = TestSocket::start(vec![r#"{"error":"unknown command"}"#.into()]);
        let r: Result<VersionReply> = call(srv.path.as_path(), "garbage");
        let err = r.unwrap_err().to_string();
        assert!(err.contains("unknown command"), "got: {err}");
    }

    #[test]
    fn snapshots_skip_missing_appid_and_put_focused_first() {
        let mk = |id: u64, app: Option<&'static str>, focused: bool| MangoClient {
            id,
            appid: app.map(String::from),
            title: None,
            is_focused: focused,
        };
        let clients = vec![
            mk(1, Some("kitty"), false),
            mk(2, None, true),
            mk(3, Some("claude"), false),
        ];
        let snaps = snapshots_from(&clients);
        assert_eq!(snaps.len(), 2, "missing appid is skipped");
        // The focused window had no appid and was filtered out, so neither
        // snap carries is_focused here. Recency still falls in id order.
        assert_eq!(snaps[0].address, "1");
        assert_eq!(snaps[1].address, "3");
    }

    #[test]
    fn snapshots_focused_window_with_appid_lands_first() {
        let clients = vec![
            MangoClient {
                id: 1,
                appid: Some("kitty".into()),
                title: None,
                is_focused: false,
            },
            MangoClient {
                id: 2,
                appid: Some("claude".into()),
                title: None,
                is_focused: true,
            },
        ];
        let snaps = snapshots_from(&clients);
        assert_eq!(snaps[0].address, "2", "focused lands at recency 0");
        assert_eq!(snaps[1].address, "1");
    }

    #[test]
    fn parse_client_id_round_trip() {
        assert_eq!(parse_client_id("42").unwrap(), 42);
        assert!(parse_client_id("not a number").is_err());
    }
}
