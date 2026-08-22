//! End-to-end tests for the niri backend, exercising the full beckon binary
//! against a fake compositor that speaks just enough of niri's IPC protocol
//! (one JSON line per request over `NIRI_SOCKET`, `{"Ok":..}`/`{"Err":..}`
//! wrapper) to drive every algorithm branch.
//!
//! What this covers vs. what it doesn't:
//!   - ✅ request wire shapes (`{"Windows":null}`, `FocusWindow`,
//!     `MoveWindowToWorkspace`) — the exact-string pins live in the unit
//!     tests inside `niri.rs`; this file pins behavior
//!   - ✅ algorithm wiring: launch / focus / cycle / toggle / hide
//!   - ✅ that focus is confirmed through server STATE (`is_focused`), never
//!     through the reply — `FocusWindow` on an unknown id answers Ok too,
//!     which is trap #1 from the measurement and is pinned by its own test
//!   - ✅ that the shared MRU state file records the app we left
//!   - ✅ .desktop resolution feeding the right target into the algorithm
//!   - ✅ doctor picking niri up from `NIRI_SOCKET`
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

/// The empty trailing workspace idx the fake `Workspaces` reply offers; the
/// hide test asserts beckon picks exactly it.
const HIDE_TARGET_IDX: u64 = 9;

#[derive(Debug, Clone)]
struct FakeWindow {
    id: u64,
    app_id: &'static str,
    title: String,
    is_focused: bool,
    ts_secs: i64,
}

impl FakeWindow {
    fn new(id: u64, app_id: &'static str, ts_secs: i64) -> Self {
        Self {
            id,
            app_id,
            title: format!("{app_id} window"),
            is_focused: false,
            ts_secs,
        }
    }

    /// Encode as the measured shape of one `Windows` element. Only fields
    /// beckon reads are populated meaningfully; `layout` is null because
    /// beckon ignores it.
    fn to_wire(&self) -> Value {
        json!({
            "id": self.id,
            "title": self.title,
            "app_id": self.app_id,
            "pid": 1000u64 + self.id,
            "workspace_id": 1,
            "is_focused": self.is_focused,
            "is_floating": false,
            "is_urgent": false,
            "layout": null,
            "focus_timestamp": { "secs": self.ts_secs, "nanos": 0 }
        })
    }
}

#[derive(Default, Debug)]
struct State {
    windows: Vec<FakeWindow>,
    /// Actions beckon issued, in order. Queries are excluded so assertions
    /// stay focused on what changes compositor state.
    actions: Vec<String>,
}

impl State {
    fn handle(&mut self, line: &str) -> String {
        let req: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => return r#"{"Err":"error parsing request"}"#.into(),
        };

        if req.get("Windows").is_some() {
            let wins: Vec<Value> = self.windows.iter().map(|w| w.to_wire()).collect();
            return json!({ "Ok": { "Windows": wins } }).to_string();
        }

        if req.get("Workspaces").is_some() {
            // Focused workspace on "winit" plus the guaranteed-empty trailing
            // one at idx 9 — the shape `hide_target_index` picks from. One
            // line: the fake socket writes this verbatim and the client reads
            // one line per reply.
            return r#"{"Ok":{"Workspaces":[{"id":1,"idx":1,"name":null,"output":"winit","is_urgent":false,"is_active":true,"is_focused":true,"active_window_id":7},{"id":2,"idx":9,"name":null,"output":"winit","is_urgent":false,"is_active":false,"is_focused":false,"active_window_id":null}]}}"#.into();
        }

        if req.get("Version").is_some_and(|v| v.is_null()) {
            return r#"{"Ok":{"Version":"25.02-test"}}"#.into();
        }

        if let Some(action) = req.get("Action") {
            if let Some(focus) = action.get("FocusWindow") {
                let id = focus.get("id").and_then(Value::as_u64).unwrap_or(0);
                self.actions.push(format!("focus:{id}"));
                let exists = self.windows.iter().any(|w| w.id == id);
                if exists {
                    // Model real focus semantics: exclusive focus + newest
                    // timestamp, so a follow-up query sees the same MRU
                    // shape the real compositor would produce.
                    let max_ts = self.windows.iter().map(|w| w.ts_secs).max().unwrap_or(0);
                    for w in &mut self.windows {
                        w.is_focused = w.id == id;
                        if w.id == id {
                            w.ts_secs = max_ts + 1;
                        }
                    }
                }
                // Unknown id: still Handled, state untouched. That is not a
                // fixture shortcut — it is the measured silent no-op (trap
                // #1), kept faithful so nothing can quietly start trusting
                // the reply.
                return r#"{"Ok":"Handled"}"#.into();
            }
            if action.get("MoveWindowToWorkspace").is_some() {
                self.actions.push(action.to_string());
                return r#"{"Ok":"Handled"}"#.into();
            }
        }

        // Anything else — including {"Windows":{}}-style malformations — is
        // what the real compositor answers.
        r#"{"Err":"error parsing request"}"#.into()
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
        let socket_path = runtime_dir.join("niri-test.sock");
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
                        // niri's wire pattern: one JSON line per connection
                        // side. Read one line, answer one line, close.
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
        // Clean env so beckon can't be sidetracked into another backend by
        // ambient session vars — env_clear is what keeps SWAYSOCK etc. from
        // hijacking dispatch order.
        cmd.env_clear();
        cmd.env("BECKON_NO_NOTIFY", "1");
        cmd.env(
            "PATH",
            std::env::var("PATH").unwrap_or_else(|_| "/usr/bin:/bin".into()),
        );
        cmd.env("XDG_RUNTIME_DIR", &self.runtime_dir);
        cmd.env("NIRI_SOCKET", &self.socket_path);
        // Confine the .desktop scan to the test fixture dir.
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
            windows: st.windows.clone(),
            actions: st.actions.clone(),
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
    windows: Vec<FakeWindow>,
    actions: Vec<String>,
}

impl StateSnapshot {
    fn focused_id(&self) -> Option<u64> {
        self.windows.iter().find(|w| w.is_focused).map(|w| w.id)
    }

    fn window_by_app(&self, app: &str) -> Option<&FakeWindow> {
        self.windows.iter().find(|w| w.app_id == app)
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
        "beckon-niri-e2e-{}-{}-{}",
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

// ============================================================================
// Tests
// ============================================================================

#[test]
fn launch_runs_outside_the_ipc_socket() {
    // niri's IPC has no exec action; launch goes through setsid -f locally.
    // The fake server must therefore see NO action at all for a launch.
    let srv = FakeServer::start(State::default());
    write_claude_desktop(&srv, "/bin/true --launch");

    let out = srv.run_beckon(&["claude"]);
    ok_output(&out, "beckon claude (launch)");

    let snap = srv.snapshot();
    assert!(
        snap.actions.is_empty(),
        "launch must not touch the IPC socket: {:?}",
        snap.actions
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
fn focus_picks_the_most_recent_window_and_flips_is_focused() {
    // kitty holds focus; two claude windows exist, id 3 focused more
    // recently than id 2. Step 4 must pick id 3, and the confirmation is
    // the server's `is_focused` flip — never the reply.
    let mut state = State::default();
    state.windows = vec![
        FakeWindow::new(1, "kitty", 5),
        FakeWindow::new(2, "claude", 3),
        FakeWindow::new(3, "claude", 4),
    ];
    state.windows[0].is_focused = true;
    let srv = FakeServer::start(state);
    write_claude_desktop(&srv, "/bin/true");

    let out = srv.run_beckon(&["claude"]);
    ok_output(&out, "beckon claude (focus)");

    let snap = srv.snapshot();
    assert_eq!(snap.focused_id(), Some(3));
    assert_eq!(snap.actions, vec!["focus:3".to_string()]);
}

#[test]
fn cycle_rotates_to_the_next_window_by_address_order() {
    let mut state = State::default();
    state.windows = vec![
        FakeWindow::new(2, "claude", 9),
        FakeWindow::new(3, "claude", 8),
    ];
    state.windows[0].is_focused = true;
    let srv = FakeServer::start(state);
    write_claude_desktop(&srv, "/bin/true");

    let out = srv.run_beckon(&["claude"]);
    ok_output(&out, "beckon claude (cycle)");

    let snap = srv.snapshot();
    assert_eq!(snap.focused_id(), Some(3));
    assert_eq!(snap.actions, vec!["focus:3".to_string()]);
}

#[test]
fn toggle_back_follows_focus_timestamp() {
    // Single claude window focused; firefox was focused more recently than
    // kitty, so step 5b lands there.
    let mut state = State::default();
    state.windows = vec![
        FakeWindow::new(1, "claude", 10),
        FakeWindow::new(2, "kitty", 1),
        FakeWindow::new(3, "firefox", 4),
    ];
    state.windows[0].is_focused = true;
    let srv = FakeServer::start(state);
    write_claude_desktop(&srv, "/bin/true");

    let out = srv.run_beckon(&["claude"]);
    ok_output(&out, "beckon claude (toggle)");

    let snap = srv.snapshot();
    assert_eq!(snap.focused_id(), Some(3));
}

#[test]
fn mru_file_records_the_app_we_left() {
    // Same contract as i3ipc: after the action, the previously focused app
    // lands in $XDG_RUNTIME_DIR/beckon-mru.
    let mut state = State::default();
    state.windows = vec![
        FakeWindow::new(1, "kitty", 5),
        FakeWindow::new(2, "claude", 3),
    ];
    state.windows[0].is_focused = true;
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
fn hide_moves_to_empty_trailing_workspace_without_focus() {
    // No minimize exists in niri (measured); step 5c moves the lone window
    // to the highest-index EMPTY workspace with focus:false so the view does
    // not chase it. A later FocusWindow navigates back — self-unparking.
    let mut state = State::default();
    state.windows = vec![FakeWindow::new(7, "claude", 1)];
    state.windows[0].is_focused = true;
    let srv = FakeServer::start(state);
    write_claude_desktop(&srv, "/bin/true");

    let out = srv.run_beckon(&["claude"]);
    ok_output(&out, "beckon claude (hide)");

    let snap = srv.snapshot();
    assert_eq!(snap.actions.len(), 1, "{:?}", snap.actions);
    assert!(
        snap.actions[0].contains("\"window_id\":7"),
        "wrong target: {:?}",
        snap.actions[0]
    );
    assert!(
        snap.actions[0].contains(&format!("\"Index\":{HIDE_TARGET_IDX}")),
        "expected the empty trailing index: {:?}",
        snap.actions[0]
    );
    assert!(
        snap.actions[0].contains("\"focus\":false"),
        "hide must not follow the window: {:?}",
        snap.actions[0]
    );
}

#[test]
fn focusing_a_missing_id_is_a_silent_noop() {
    // Trap #1, pinned at the fixture level: FocusWindow on an unknown id
    // answers Ok and changes nothing. This is why every focus assertion in
    // this file reads server state instead of the reply.
    let mut state = State::default();
    state.windows = vec![FakeWindow::new(1, "kitty", 1)];
    state.windows[0].is_focused = true;
    let srv = FakeServer::start(state);

    let reply = {
        use std::io::{BufRead, BufReader};
        let mut stream = std::os::unix::net::UnixStream::connect(&srv.socket_path).unwrap();
        stream
            .write_all(br#"{"Action":{"FocusWindow":{"id":999}}}"#)
            .unwrap();
        stream.write_all(b"\n").unwrap();
        let mut reader = BufReader::new(stream);
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        line
    };

    assert!(reply.contains(r#""Ok""#), "got: {reply}");
    let snap = srv.snapshot();
    assert_eq!(snap.focused_id(), Some(1), "focus must not have moved");
    assert!(snap.window_by_app("kitty").is_some());
}

#[test]
fn list_running_groups_by_app_id_with_counts() {
    let mut state = State::default();
    state.windows = vec![
        FakeWindow::new(1, "claude", 3),
        FakeWindow::new(2, "claude", 4),
        FakeWindow::new(3, "kitty", 5),
    ];
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
fn doctor_reports_niri_backend_and_ipc_ok() {
    let mut state = State::default();
    state.windows = vec![
        FakeWindow::new(1, "claude", 1),
        FakeWindow::new(2, "kitty", 2),
    ];
    let srv = FakeServer::start(state);

    let out = srv.run_beckon(&["doctor"]);
    ok_output(&out, "beckon doctor");

    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("NIRI_SOCKET"),
        "doctor didn't probe NIRI_SOCKET: {}",
        stdout
    );
    assert!(
        stdout.contains("Backend selected"),
        "doctor didn't pick the niri backend: {}",
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
    // User typed the human-readable Name "Claude"; the resolver must map it
    // to the .desktop stem and match the running window's app_id.
    let mut state = State::default();
    state.windows = vec![FakeWindow::new(4, "claude", 1)];
    state.windows[0].is_focused = false;
    let srv = FakeServer::start(state);
    write_claude_desktop(&srv, "/bin/true");

    let out = srv.run_beckon(&["Claude"]);
    ok_output(&out, "beckon Claude (name resolution)");

    let snap = srv.snapshot();
    assert_eq!(snap.focused_id(), Some(4));
}
