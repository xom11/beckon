//! End-to-end tests for the Hyprland backend, exercising the full beckon
//! binary against a fake compositor that speaks just enough of the Hyprland
//! IPC protocol to drive every algorithm branch.
//!
//! What this covers vs. what it doesn't:
//!   - ✅ socket layout (`$XDG_RUNTIME_DIR/hypr/<sig>/.socket.sock`)
//!   - ✅ command wire format (`version`, `j/clients`, `j/activewindow`,
//!     `dispatch focuswindow address:0xN`,
//!     `dispatch movetoworkspacesilent <ws>,address:0xN`,
//!     `dispatch exec <cmdline>`)
//!   - ✅ algorithm wiring: launch / focus / cycle / toggle / hide
//!   - ✅ MRU state file integration for step 5b
//!   - ✅ .desktop resolution feeding the right target into the algorithm
//!   - ❌ rendering and real focus changes (requires a live compositor —
//!     out of reach on this Ubuntu 26.04 host because Hyprland 0.53.3
//!     needs xdg_wm_base v6 while sway 1.11 / weston 14.0.2 only expose v5)
//!
//! The fake server is intentionally tiny: it serves one command per accepted
//! connection, then closes so the client sees EOF (Hyprland's actual wire
//! pattern). State lives in an `Arc<Mutex<State>>` shared between the server
//! thread and the test body so assertions can inspect what beckon mutated.

#![cfg(target_os = "linux")]

use std::fs;
use std::io::{Read, Write};
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const BECKON: &str = env!("CARGO_BIN_EXE_beckon");

#[derive(Debug, Clone)]
struct FakeClient {
    address: String,
    class: String,
    title: String,
    workspace: String,
    fhid: i32,
}

impl FakeClient {
    fn new(address: &str, class: &str, fhid: i32) -> Self {
        Self {
            address: address.to_string(),
            class: class.to_string(),
            title: format!("{} window", class),
            workspace: "1".to_string(),
            fhid,
        }
    }

    /// Encode as the JSON object Hyprland's `j/clients` returns. Only the
    /// fields beckon actually reads are populated; everything else is left
    /// off and serde defaults handle it. Hand-written so the test crate
    /// doesn't need a serde dep.
    fn to_json(&self) -> String {
        format!(
            r#"{{"address":"{}","class":"{}","title":"{}","focusHistoryID":{}}}"#,
            json_escape(&self.address),
            json_escape(&self.class),
            json_escape(&self.title),
            self.fhid
        )
    }
}

fn json_escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

#[derive(Default, Debug)]
struct State {
    clients: Vec<FakeClient>,
    active: Option<String>,
    /// Mutating commands beckon issued, in order. Queries (`j/clients`,
    /// `j/activewindow`, `version`) are excluded so test assertions stay
    /// focused on the actions that change compositor state.
    dispatches: Vec<String>,
}

impl State {
    fn handle(&mut self, cmd: &str) -> String {
        match cmd {
            "j/clients" => {
                let body: Vec<String> = self.clients.iter().map(|c| c.to_json()).collect();
                return format!("[{}]", body.join(","));
            }
            "j/activewindow" => {
                return match self.active.as_deref() {
                    Some(addr) => self
                        .clients
                        .iter()
                        .find(|c| c.address == addr)
                        .map(|c| c.to_json())
                        .unwrap_or_else(|| "{}".into()),
                    None => "{}".into(),
                };
            }
            "version" => return "ok".into(),
            _ => {}
        }

        self.dispatches.push(cmd.to_string());

        if let Some(addr) = cmd.strip_prefix("dispatch focuswindow address:") {
            // Bring the named window to focus and bump focus history so a
            // follow-up call sees the same MRU shape Hyprland would produce.
            for c in &mut self.clients {
                if c.address == addr {
                    c.fhid = 0;
                } else {
                    c.fhid += 1;
                }
            }
            if self.clients.iter().any(|c| c.address == addr) {
                self.active = Some(addr.to_string());
                return "ok".into();
            }
            return format!("error: unknown address {}", addr);
        }

        if let Some(rest) = cmd.strip_prefix("dispatch movetoworkspacesilent ") {
            // Format: "<ws>,address:0xN"
            if let Some((ws, addr_part)) = rest.split_once(",address:") {
                for c in &mut self.clients {
                    if c.address == addr_part {
                        c.workspace = ws.to_string();
                    }
                }
                return "ok".into();
            }
            return format!("error: malformed movetoworkspacesilent `{}`", rest);
        }

        if cmd.starts_with("dispatch exec ") {
            // Real Hyprland would launch the process; the fake server only
            // records the call so tests can assert on the launch path.
            return "ok".into();
        }

        format!("error: unknown command `{}`", cmd)
    }
}

struct FakeServer {
    runtime_dir: PathBuf,
    signature: String,
    state: Arc<Mutex<State>>,
    stop: Arc<AtomicBool>,
}

impl FakeServer {
    fn start(initial: State) -> Self {
        let runtime_dir = make_temp_dir();
        let signature = "beckon-test".to_string();
        let hypr_dir = runtime_dir.join("hypr").join(&signature);
        fs::create_dir_all(&hypr_dir).unwrap();
        let socket_path = hypr_dir.join(".socket.sock");
        let listener = UnixListener::bind(&socket_path).unwrap();
        // Poll-style accept so the thread can wind down when the test ends.
        listener.set_nonblocking(true).unwrap();

        let state = Arc::new(Mutex::new(initial));
        let stop = Arc::new(AtomicBool::new(false));
        let s_state = state.clone();
        let s_stop = stop.clone();

        thread::spawn(move || {
            while !s_stop.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        // Hyprland sends one command per connection without a
                        // terminator, then expects a body and EOF back. Read
                        // a single chunk; for our short commands one read is
                        // always enough.
                        stream
                            .set_read_timeout(Some(Duration::from_millis(500)))
                            .ok();
                        let mut buf = [0u8; 4096];
                        let n = match stream.read(&mut buf) {
                            Ok(n) => n,
                            Err(_) => continue,
                        };
                        let cmd = std::str::from_utf8(&buf[..n]).unwrap_or("").trim();
                        if cmd.is_empty() {
                            continue;
                        }
                        let resp = s_state.lock().unwrap().handle(cmd);
                        let _ = stream.write_all(resp.as_bytes());
                        // Dropping `stream` closes the socket → EOF on client.
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
            signature,
            state,
            stop,
        }
    }

    fn run_beckon(&self, args: &[&str]) -> Output {
        let mut cmd = Command::new(BECKON);
        cmd.args(args);
        // Build a clean env so beckon can't be sidetracked into another
        // backend (sway, i3, X11, etc.) by ambient session vars.
        cmd.env_clear();
        cmd.env(
            "PATH",
            std::env::var("PATH").unwrap_or_else(|_| "/usr/bin:/bin".into()),
        );
        cmd.env("XDG_RUNTIME_DIR", &self.runtime_dir);
        cmd.env("HYPRLAND_INSTANCE_SIGNATURE", &self.signature);
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

    fn write_mru(&self, app: &str) {
        fs::write(self.runtime_dir.join("beckon-mru"), app).unwrap();
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
            active: st.active.clone(),
            dispatches: st.dispatches.clone(),
        }
    }
}

impl Drop for FakeServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        // Best-effort: leave the temp dir behind for forensic inspection if a
        // test fails. They live under /tmp so the OS will reap them.
    }
}

#[derive(Debug)]
struct StateSnapshot {
    clients: Vec<FakeClient>,
    active: Option<String>,
    dispatches: Vec<String>,
}

impl StateSnapshot {
    fn workspace_of(&self, addr: &str) -> Option<&str> {
        self.clients
            .iter()
            .find(|c| c.address == addr)
            .map(|c| c.workspace.as_str())
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
        "beckon-hypr-e2e-{}-{}-{}",
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
fn launch_when_no_window_dispatches_exec() {
    let srv = FakeServer::start(State::default());
    write_claude_desktop(&srv, "/bin/true --launch");

    let out = srv.run_beckon(&["claude"]);
    ok_output(&out, "beckon claude (launch)");

    let snap = srv.snapshot();
    let exec_calls: Vec<&String> = snap
        .dispatches
        .iter()
        .filter(|c| c.starts_with("dispatch exec"))
        .collect();
    assert_eq!(
        exec_calls.len(),
        1,
        "expected exactly one exec dispatch, got: {:?}",
        snap.dispatches
    );
    assert_eq!(exec_calls[0], "dispatch exec /bin/true --launch");
}

#[test]
fn launch_without_desktop_entry_errors() {
    // No .desktop file written and no running window — beckon should fail
    // with a hint, not silently succeed.
    let srv = FakeServer::start(State::default());

    let out = srv.run_beckon(&["claude"]);
    assert!(
        !out.status.success(),
        "expected failure when nothing matches: stdout={} stderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("no .desktop entry") || stderr.contains("no running window"),
        "unhelpful error: {}",
        stderr
    );
}

#[test]
fn focus_when_target_running_but_other_app_focused() {
    let mut state = State::default();
    state.clients = vec![
        FakeClient::new("0xA", "kitty", 0),
        FakeClient::new("0xB", "claude", 1),
        FakeClient::new("0xC", "claude", 2),
    ];
    state.active = Some("0xA".into());
    let srv = FakeServer::start(state);
    write_claude_desktop(&srv, "/bin/true");

    let out = srv.run_beckon(&["claude"]);
    ok_output(&out, "beckon claude (focus)");

    let snap = srv.snapshot();
    assert_eq!(snap.active.as_deref(), Some("0xB"));
    assert_eq!(
        snap.dispatches,
        vec!["dispatch focuswindow address:0xB".to_string()]
    );
}

#[test]
fn cycle_picks_next_recent_same_app_window() {
    let mut state = State::default();
    state.clients = vec![
        FakeClient::new("0xA", "claude", 0),
        FakeClient::new("0xB", "claude", 1),
        FakeClient::new("0xC", "claude", 2),
    ];
    state.active = Some("0xA".into());
    let srv = FakeServer::start(state);
    write_claude_desktop(&srv, "/bin/true");

    let out = srv.run_beckon(&["claude"]);
    ok_output(&out, "beckon claude (cycle)");

    let snap = srv.snapshot();
    assert_eq!(snap.active.as_deref(), Some("0xB"));
    assert_eq!(
        snap.dispatches,
        vec!["dispatch focuswindow address:0xB".to_string()]
    );
}

#[test]
fn toggle_back_when_only_one_target_window_picks_other_by_fhid() {
    let mut state = State::default();
    state.clients = vec![
        FakeClient::new("0xA", "claude", 0), // focused
        FakeClient::new("0xB", "kitty", 1),  // most-recent other-app
        FakeClient::new("0xC", "firefox", 2),
    ];
    state.active = Some("0xA".into());
    let srv = FakeServer::start(state);
    write_claude_desktop(&srv, "/bin/true");

    let out = srv.run_beckon(&["claude"]);
    ok_output(&out, "beckon claude (toggle)");

    let snap = srv.snapshot();
    assert_eq!(snap.active.as_deref(), Some("0xB"));
}

#[test]
fn toggle_back_uses_mru_state_file_when_present() {
    let mut state = State::default();
    state.clients = vec![
        FakeClient::new("0xA", "claude", 0),
        FakeClient::new("0xB", "kitty", 5),    // older
        FakeClient::new("0xC", "firefox", 1), // newer
    ];
    state.active = Some("0xA".into());
    let srv = FakeServer::start(state);
    write_claude_desktop(&srv, "/bin/true");
    // Saved "previous" was kitty — must beat firefox even though firefox is
    // more recent in the focus history.
    srv.write_mru("kitty");

    let out = srv.run_beckon(&["claude"]);
    ok_output(&out, "beckon claude (toggle MRU)");

    let snap = srv.snapshot();
    assert_eq!(
        snap.active.as_deref(),
        Some("0xB"),
        "expected MRU previous (kitty) to win"
    );
}

#[test]
fn hide_when_only_target_window_exists_moves_to_special_workspace() {
    let mut state = State::default();
    state.clients = vec![FakeClient::new("0xA", "claude", 0)];
    state.active = Some("0xA".into());
    let srv = FakeServer::start(state);
    write_claude_desktop(&srv, "/bin/true");

    let out = srv.run_beckon(&["claude"]);
    ok_output(&out, "beckon claude (hide)");

    let snap = srv.snapshot();
    assert_eq!(
        snap.workspace_of("0xA"),
        Some("special:beckon"),
        "expected hide to park window on special:beckon"
    );
    assert_eq!(
        snap.dispatches,
        vec!["dispatch movetoworkspacesilent special:beckon,address:0xA".to_string()]
    );
}

#[test]
fn beckon_persists_pre_focused_class_into_mru_file() {
    // After a focus change, the previously-focused app's class should land
    // in the MRU file so the next invocation can use it for step 5b.
    let mut state = State::default();
    state.clients = vec![
        FakeClient::new("0xA", "kitty", 0),
        FakeClient::new("0xB", "claude", 1),
    ];
    state.active = Some("0xA".into());
    let srv = FakeServer::start(state);
    write_claude_desktop(&srv, "/bin/true");

    let out = srv.run_beckon(&["claude"]);
    ok_output(&out, "beckon claude (mru persist)");

    assert_eq!(
        srv.read_mru().as_deref(),
        Some("kitty"),
        "expected pre-focused class kitty to be persisted to MRU file"
    );
}

#[test]
fn name_resolution_routes_through_desktop_entry() {
    // User typed the human-readable Name "Claude". The .desktop filename is
    // "claude" — beckon's resolver must map Name → filename and use the
    // filename as the runtime class to match in j/clients.
    let mut state = State::default();
    state.clients = vec![FakeClient::new("0xA", "claude", 0)];
    // Nothing is focused — beckon should still focus the only claude window.
    state.active = None;
    let srv = FakeServer::start(state);
    write_claude_desktop(&srv, "/bin/true");

    let out = srv.run_beckon(&["Claude"]);
    ok_output(&out, "beckon Claude (name resolution)");

    let snap = srv.snapshot();
    assert_eq!(snap.active.as_deref(), Some("0xA"));
}

#[test]
fn list_running_emits_apps_grouped_by_class() {
    let mut state = State::default();
    state.clients = vec![
        FakeClient::new("0xA", "claude", 0),
        FakeClient::new("0xB", "claude", 2),
        FakeClient::new("0xC", "kitty", 1),
    ];
    state.active = Some("0xA".into());
    let srv = FakeServer::start(state);

    let out = srv.run_beckon(&["-l"]);
    ok_output(&out, "beckon -l");

    let stdout = String::from_utf8_lossy(&out.stdout);
    // Output is fixed-width: ID column, then WINS, then NAME. We only
    // verify the rows exist with the right counts — column formatting is
    // a separate concern.
    assert!(
        stdout.contains("claude") && stdout.contains("kitty"),
        "missing app rows: {}",
        stdout
    );
    // claude has 2 windows, kitty has 1. Look for the count column.
    assert!(
        stdout.lines().any(|l| l.contains("claude") && l.contains("2")),
        "claude count missing: {}",
        stdout
    );
    assert!(
        stdout.lines().any(|l| l.contains("kitty") && l.contains("1")),
        "kitty count missing: {}",
        stdout
    );
}

#[test]
fn doctor_reports_hyprland_backend_and_running_count() {
    let mut state = State::default();
    state.clients = vec![
        FakeClient::new("0xA", "claude", 0),
        FakeClient::new("0xB", "kitty", 1),
    ];
    let srv = FakeServer::start(state);

    let out = srv.run_beckon(&["-d"]);
    ok_output(&out, "beckon -d");

    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("HYPRLAND_INSTANCE_SIGNATURE"),
        "doctor didn't probe Hyprland env: {}",
        stdout
    );
    assert!(
        stdout.contains("Backend selected") || stdout.contains("✅ Backend"),
        "doctor didn't pick Hyprland backend: {}",
        stdout
    );
    // 2 windows visible.
    assert!(
        stdout.contains("2 running window"),
        "wrong window count: {}",
        stdout
    );
}

// Sanity check that the fixtures dir layout matches what beckon expects —
// catches future renames of the socket path quickly.
#[test]
fn socket_path_layout_matches_beckon_expectation() {
    let srv = FakeServer::start(State::default());
    let expected = srv
        .runtime_dir
        .join("hypr")
        .join(&srv.signature)
        .join(".socket.sock");
    assert!(
        Path::new(&expected).exists(),
        "socket missing at expected path {:?}",
        expected
    );
}
