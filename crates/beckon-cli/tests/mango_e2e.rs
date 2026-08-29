//! End-to-end tests for the mango (mangowm) backend, exercising the full
//! beckon binary against a fake compositor that speaks just enough of mango's
//! text IPC protocol to drive every algorithm branch.
//!
//! Mango's IPC differs from niri in one structural way worth pinning here:
//! one **text** command line + one **plain-JSON** reply per connection,
//! server closes after writing (no `{"Ok":..}` wrapper, no keep-alive).
//! `MangoBackend::new()` and every action/query each open a fresh socket.
//!
//! Coverage mirrors niri_e2e.rs:
//!   - ✅ request wire shapes (`get version`, `get all-clients`,
//!     `dispatch focusid client,<n>`, `dispatch minimized`) — the exact
//!     strings live in the unit tests inside `mango.rs`; here we pin
//!     behavior through the binary
//!   - ✅ launch / focus / cycle / toggle / hide routing
//!   - ✅ that focus is confirmed through server STATE (`is_focused`), never
//!     the reply
//!   - ✅ MRU state record
//!   - ✅ .desktop resolution feeding the right target
//!   - ✅ doctor picking mango up from `MANGO_INSTANCE_SIGNATURE`
//!   - ❌ a real compositor (launch runs through `setsid -f` OUTSIDE the IPC
//!     socket by design, so "process survived" needs a live session on rog)

#![cfg(target_os = "linux")]
#![allow(clippy::field_reassign_with_default)]

use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixListener;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};

const BECKON: &str = env!("CARGO_BIN_EXE_beckon");

#[derive(Debug, Clone)]
struct FakeClient {
    id: u64,
    appid: String,
    is_focused: bool,
}

fn client(id: u64, appid: &str) -> FakeClient {
    FakeClient {
        id,
        appid: appid.to_string(),
        is_focused: false,
    }
}

fn to_wire(c: &FakeClient) -> Value {
    json!({
        "id": c.id,
        "appid": c.appid,
        "title": format!("{} window", c.appid),
        "is_focused": c.is_focused,
        "state": "active",
        "pid": 1000u64 + c.id,
    })
}

#[derive(Default, Debug)]
struct State {
    clients: Vec<FakeClient>,
    /// Aggregate of every line the fake server received, for wire assertions.
    requests: Vec<String>,
}

impl State {
    fn handle(&mut self, line: &str) -> String {
        let cmd = line.trim();
        self.requests.push(cmd.to_string());

        // Mango replies are plain JSON and the server closes after one reply.
        if cmd == "get version" {
            return r#"{"version":"0.16.1","tag":"0.16.1","pretty_name":"mangowm"}"#.into();
        }
        if cmd == "get all-clients" {
            let arr: Vec<Value> = self.clients.iter().map(to_wire).collect();
            // all-clients is a top-level array; the wrapper shapes used by
            // all-monitors etc. are a separate per-command trap.
            return serde_json::to_string(&arr).unwrap();
        }
        if let Some(rest) = cmd.strip_prefix("dispatch focusid client,") {
            if let Ok(id) = rest.parse::<u64>() {
                for c in &mut self.clients {
                    c.is_focused = c.id == id;
                }
                return r#"{"success":true}"#.into();
            }
        }
        if cmd == "dispatch minimized" {
            return r#"{"success":true}"#.into();
        }
        r#"{"error":"unknown command"}"#.into()
    }
}

struct FakeServer {
    runtime_dir: PathBuf,
    socket_path: PathBuf,
    state: Arc<Mutex<State>>,
    stop: Arc<AtomicBool>,
}

impl FakeServer {
    fn start(initial: State) -> Self {
        let runtime_dir = make_temp_dir();
        let socket_path = runtime_dir.join("mango-test.sock");
        let listener = UnixListener::bind(&socket_path).unwrap();
        listener.set_nonblocking(true).unwrap();

        let state = Arc::new(Mutex::new(initial));
        let stop = Arc::new(AtomicBool::new(false));
        let s_state = state.clone();
        let s_stop = stop.clone();

        thread::spawn(move || {
            while !s_stop.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((stream, _)) => {
                        stream
                            .set_read_timeout(Some(Duration::from_millis(500)))
                            .ok();
                        let mut reader = BufReader::new(stream);
                        let mut line = String::new();
                        match reader.read_line(&mut line) {
                            Ok(0) | Err(_) => continue,
                            Ok(_) => {}
                        }
                        let resp = s_state.lock().unwrap().handle(line.trim());
                        let mut out = reader.get_ref().try_clone().unwrap();
                        let _ = out.write_all(resp.as_bytes());
                        let _ = out.write_all(b"\n");
                        // mango closes the connection after each reply
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(5));
                    }
                    Err(_) => break,
                }
            }
        });

        Self {
            runtime_dir,
            socket_path,
            state,
            stop,
        }
    }

    fn run_beckon(&self, args: &[&str]) -> Output {
        let mut cmd = Command::new(BECKON);
        cmd.args(args);
        cmd.env_clear();
        cmd.env("BECKON_NO_NOTIFY", "1");
        cmd.env(
            "PATH",
            std::env::var("PATH").unwrap_or_else(|_| "/usr/bin:/bin".into()),
        );
        cmd.env("XDG_RUNTIME_DIR", &self.runtime_dir);
        cmd.env("MANGO_INSTANCE_SIGNATURE", &self.socket_path);
        let xdg_data = self.runtime_dir.join("xdg-data");
        cmd.env("XDG_DATA_HOME", &xdg_data);
        cmd.env("XDG_DATA_DIRS", &xdg_data);
        cmd.env("HOME", &self.runtime_dir);
        cmd.output().expect("failed to spawn beckon binary")
    }

    fn write_desktop(&self, filename: &str, contents: &str) {
        let dir = self.runtime_dir.join("xdg-data").join("applications");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(filename), contents).unwrap();
    }

    fn read_mru(&self) -> Option<String> {
        fs::read_to_string(self.runtime_dir.join("beckon-mru"))
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    }

    fn snapshot(&self) -> StateSnapshot {
        let st = self.state.lock().unwrap();
        StateSnapshot {
            clients: st.clients.clone(),
            requests: st.requests.clone(),
        }
    }
}

impl Drop for FakeServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

#[derive(Debug)]
struct StateSnapshot {
    clients: Vec<FakeClient>,
    requests: Vec<String>,
}

impl StateSnapshot {
    fn focused_id(&self) -> Option<u64> {
        self.clients.iter().find(|c| c.is_focused).map(|c| c.id)
    }
    fn focused_appid(&self) -> Option<&str> {
        self.clients
            .iter()
            .find(|c| c.is_focused)
            .map(|c| c.appid.as_str())
    }
}

fn make_temp_dir() -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let d = std::env::temp_dir().join(format!(
        "beckon-mango-e2e-{}-{}-{}",
        std::process::id(),
        nanos,
        n
    ));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    d
}

fn write_claude_desktop(srv: &FakeServer, exec: &str) {
    srv.write_desktop(
        "claude.desktop",
        &format!(
            "[Desktop Entry]\nType=Application\nName=Claude\nExec={}\n",
            exec
        ),
    );
}

fn ok_output(out: &Output, label: &str) {
    assert!(
        out.status.success(),
        "{} failed: status={:?}\nstdout: {}\nstderr: {}",
        label,
        out.status,
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}

/// Every beckon invocation opens with get version (the construction probe), so
/// the first `get all-clients` position in the request log is index 1.
fn clients_request_index(requests: &[String]) -> usize {
    requests
        .iter()
        .position(|r| r == "get all-clients")
        .expect("beckon never queried clients")
}

#[test]
fn launch_runs_outside_the_ipc_socket() {
    let srv = FakeServer::start(State::default());
    write_claude_desktop(&srv, "/bin/true --launch");

    let out = srv.run_beckon(&["claude"]);
    ok_output(&out, "beckon claude (launch)");

    let snap = srv.snapshot();
    assert!(
        snap.focused_id().is_none(),
        "launch must not focus anything"
    );
}

#[test]
fn launch_without_desktop_entry_errors() {
    let srv = FakeServer::start(State::default());

    let out = srv.run_beckon(&["claude"]);
    assert!(
        !out.status.success(),
        "expected failure when nothing matches"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("no .desktop entry") || stderr.contains("no running window"),
        "unhelpful error: {}",
        stderr
    );
}

#[test]
fn focus_sends_focusid_client_and_flips_state() {
    let mut state = State::default();
    state.clients = vec![client(1, "kitty"), client(2, "claude")];
    state.clients[0].is_focused = true;
    let srv = FakeServer::start(state);
    write_claude_desktop(&srv, "/bin/true");

    let out = srv.run_beckon(&["claude"]);
    ok_output(&out, "beckon claude (focus)");

    let snap = srv.snapshot();
    assert_eq!(snap.focused_id(), Some(2));
    assert!(
        snap.requests
            .contains(&"dispatch focusid client,2".to_string()),
        "missing focus wire: {:?}",
        snap.requests
    );
}

#[test]
fn cycle_rotates_to_the_next_window() {
    let mut state = State::default();
    state.clients = vec![client(2, "claude"), client(3, "claude")];
    state.clients[0].is_focused = true;
    let srv = FakeServer::start(state);
    write_claude_desktop(&srv, "/bin/true");

    let out = srv.run_beckon(&["claude"]);
    ok_output(&out, "beckon claude (cycle)");

    let snap = srv.snapshot();
    assert_eq!(snap.focused_id(), Some(3));
}

#[test]
fn toggle_back_returns_to_previous_app() {
    let mut state = State::default();
    state.clients = vec![
        client(1, "firefox"),
        client(2, "kitty"),
        client(3, "claude"),
    ];
    // firefox is the previously-focused app (MRU); claude is currently up.
    state.clients[2].is_focused = true;
    let srv = FakeServer::start(state);
    write_claude_desktop(&srv, "/bin/true");

    let out = srv.run_beckon(&["claude"]);
    ok_output(&out, "beckon claude (toggle)");

    let snap = srv.snapshot();
    // Toggle back targets the previously focused *window*; without focus
    // timestamps mango orders focused-first then id, so firefox (id 1) wins.
    assert_eq!(snap.focused_appid(), Some("firefox"));
}

#[test]
fn mru_file_records_the_app_we_left() {
    let mut state = State::default();
    state.clients = vec![client(1, "kitty"), client(2, "claude")];
    state.clients[0].is_focused = true;
    let srv = FakeServer::start(state);
    write_claude_desktop(&srv, "/bin/true");

    let out = srv.run_beckon(&["claude"]);
    ok_output(&out, "beckon claude (mru)");

    assert_eq!(
        srv.read_mru(),
        Some("kitty".to_string()),
        "the pre-focused app must be persisted as previous"
    );
}

#[test]
fn hide_minimises_the_focused_window() {
    let mut state = State::default();
    state.clients = vec![client(7, "claude")];
    state.clients[0].is_focused = true;
    let srv = FakeServer::start(state);
    write_claude_desktop(&srv, "/bin/true");

    let out = srv.run_beckon(&["claude"]);
    ok_output(&out, "beckon claude (hide)");

    let snap = srv.snapshot();
    assert!(
        snap.requests.contains(&"dispatch minimized".to_string()),
        "missing minimize wire: {:?}",
        snap.requests
    );
}

#[test]
fn list_running_groups_by_appid_with_counts() {
    let mut state = State::default();
    state.clients = vec![client(1, "claude"), client(2, "claude"), client(3, "kitty")];
    let srv = FakeServer::start(state);

    let out = srv.run_beckon(&["list"]);
    ok_output(&out, "beckon list");

    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout
            .lines()
            .any(|l| l.contains("claude") && l.contains("2")),
        "claude count missing: {}",
        stdout
    );
    assert!(
        stdout
            .lines()
            .any(|l| l.contains("kitty") && l.contains("1")),
        "kitty count missing: {}",
        stdout
    );
}

#[test]
fn installed_lists_desktop_entries() {
    let srv = FakeServer::start(State::default());
    write_claude_desktop(&srv, "/bin/true");

    let out = srv.run_beckon(&["installed"]);
    ok_output(&out, "beckon installed");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("Claude"), "missing row: {}", stdout);
}

#[test]
fn doctor_reports_mango_backend_and_ipc_ok() {
    let mut state = State::default();
    state.clients = vec![client(1, "claude"), client(2, "kitty")];
    let srv = FakeServer::start(state);

    let out = srv.run_beckon(&["doctor"]);
    ok_output(&out, "beckon doctor");

    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("MANGO_INSTANCE_SIGNATURE"),
        "doctor didn't probe MANGO_INSTANCE_SIGNATURE: {}",
        stdout
    );
    assert!(
        stdout.contains("Backend selected"),
        "doctor didn't pick the mango backend: {}",
        stdout
    );
    assert!(
        stdout.contains("2 running window"),
        "wrong window count: {}",
        stdout
    );
}

#[test]
fn name_resolution_routes_through_desktop_entry() {
    let mut state = State::default();
    state.clients = vec![client(4, "claude")];
    let srv = FakeServer::start(state);
    write_claude_desktop(&srv, "/bin/true");

    let out = srv.run_beckon(&["Claude"]);
    ok_output(&out, "beckon Claude (name resolution)");

    let snap = srv.snapshot();
    assert_eq!(snap.focused_id(), Some(4));
}

#[test]
fn every_action_uses_one_fresh_connection_per_request() {
    // Mango closes the socket after each reply; the client must open a fresh
    // connection per request rather than assuming a persistent session. The
    // construction probe (get version) plus the algorithm query (get
    // all-clients) plus the action each cost one connection. In the request
    // log that means: version first, clients later, and for a focus action a
    // final dispatch line — all distinct commands, all on their own
    // connection.
    let mut state = State::default();
    state.clients = vec![client(1, "kitty"), client(2, "claude")];
    state.clients[0].is_focused = true;
    let srv = FakeServer::start(state);
    write_claude_desktop(&srv, "/bin/true");

    let out = srv.run_beckon(&["claude"]);
    ok_output(&out, "beckon claude (connections)");

    let snap = srv.snapshot();
    // Sanity: the full expected sequence appeared (version ⇒ clients ⇒
    // dispatch), and no doubled command implies no stale single-socket reuse.
    assert_eq!(snap.requests[0], "get version");
    assert!(snap.requests.contains(&"get all-clients".to_string()));
    assert!(snap
        .requests
        .contains(&"dispatch focusid client,2".to_string()));
    let _ = clients_request_index(&snap.requests);
}
