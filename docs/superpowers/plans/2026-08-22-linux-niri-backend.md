# niri Backend Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a `NiriBackend` to `beckon-linux`, peer to sway/Hyprland, speaking niri's official IPC over `NIRI_SOCKET`.

**Architecture:** One new module `niri.rs` converts niri's `{"Windows":null}` reply into the shared `WindowSnapshot` list and feeds `algorithm::decide`, exactly like `i3ipc.rs`. Launch reuses the `setsid -f` recipe (niri IPC has no `exec`). Hide maps to `MoveWindowToWorkspace` to a far index (no minimize action exists). Dispatch: `NIRI_SOCKET` checked in `pick_backend()` right after Hyprland.

**Tech Stack:** Rust, `serde`/`serde_json` (already deps of `beckon-linux`), `std::os::unix::net::UnixStream`. **No new dependencies.**

**Spec:** `~/Documents/dev/beckon-niri-prompt.md` (measured 2026-08-22 on niri 26.04; wire facts and three traps live there — this plan argues from it).

## Global Constraints

- beckon is `MIT OR Apache-2.0` and ships prebuilt binaries. The official `niri-ipc` crate (26.4.0) is **GPL-3.0-or-later** — do NOT add it. Hand-roll the framing; the protocol is one JSON line each way.
- Unit variant requests send `null`, never `{}`: `{"Windows":null}` works, `{"Windows":{}}` returns `{"Err":"error parsing request"}` (measured).
- `FocusWindow` with an unknown id returns `{"Ok":"Handled"}` silently (measured). Never trust the reply as proof of focus; tests assert on server state, not on the reply.
- No minimize/scratchpad action exists in niri 26.04 (`ToggleWindowMinimized` / `MinimizeWindow` / `SetWindowMinimized` all `Err`, measured).
- `NIRI_SOCKET` env var is the only socket source of truth — never glob `$XDG_RUNTIME_DIR/niri.*.sock` (nested instances are real).
- Do not change any existing backend's behavior. `pick_backend` order stays: SWAYSOCK/I3SOCK → HYPRLAND → **NIRI (new)** → WAYLAND_DISPLAY → DISPLAY.
- Shared `CARGO_TARGET_DIR=~/Documents/dev/beckon/target` for `check`/`clippy`/`fmt`; a **private** target dir for anything that runs the built binary (`cargo test` — e2e spawns `CARGO_BIN_EXE_beckon`).
- First exec of a freshly linked binary may be killed (exit 137): re-run before believing an empty result.
- Commits: `git commit --only <file>` (never the whole index); verify with `git show --stat HEAD`.
- Error message update: the "unrecognised Wayland compositor" text must mention niri.

## File Structure

| file | action | responsibility |
|---|---|---|
| `crates/beckon-linux/src/niri.rs` | create | wire types, framing, `NiriBackend` impl of `Backend`, unit tests |
| `crates/beckon-linux/src/lib.rs` | modify | `pub mod niri;`, `pick_backend()` NIRI branch, `detect_compositor()`, error text |
| `crates/beckon-cli/src/lib.rs` | modify | `cmd_doctor` prints `NIRI_SOCKET` |
| `crates/beckon-cli/tests/niri_e2e.rs` | create | fake niri socket server + full-binary tests (model: `hyprland_e2e.rs`) |
| `CLAUDE.md`, `docs/notes/linux-backends.md` | modify | dispatch table row, measured wire notes |

---

### Task 1: Wire types, framing, unit tests (`niri.rs`)

**Files:**
- Create: `crates/beckon-linux/src/niri.rs`
- Modify: `crates/beckon-linux/src/lib.rs` (add `pub mod niri;` only — no dispatch yet)

**Interfaces:**
- Produces (Task 2 relies on these exact names):
  - `pub struct NiriBackend` with `pub fn new() -> Result<Self>` (probes `Version`).
  - `pub(crate) struct NiriWindow { id: u64, app_id: Option<String>, title: Option<String>, is_focused: bool, focus_timestamp: Option<FocusTimestamp> }` and `pub(crate) struct FocusTimestamp { secs: i64, nanos: u32 }` (`Deserialize`, `Clone`, `Debug`, `PartialEq`, `Eq`, `PartialOrd`, `Ord` on the timestamp — field order gives correct compare).
  - `fn req_windows() -> serde_json::Value`, `fn req_focus(id: u64) -> Value`, `fn req_move_to_workspace(window_id: u64, index: u64) -> Value` (private, unit-tested by exact string).
  - `fn call<T: DeserializeOwned>(path: &Path, req: Value) -> Result<T>` — core framing, parameterized by socket path so unit tests never touch env. `fn call_env<T>(req) -> Result<T>` resolves `NIRI_SOCKET` and delegates.

- [ ] **Step 1: Write the failing unit tests** (inside `niri.rs` `#[cfg(test)] mod tests`)

```rust
// helper: bind a temp UnixListener, serve one canned reply per connection,
// record request lines into Arc<Mutex<Vec<String>>>; stop via AtomicBool.
// Model: FakeServer in crates/beckon-cli/tests/hyprland_e2e.rs, but in-process.

#[test]
fn requests_serialize_exactly_as_measured() {
    assert_eq!(serde_json::to_string(&req_version()).unwrap(), r#"{"Version":null}"#);
    assert_eq!(serde_json::to_string(&req_windows()).unwrap(), r#"{"Windows":null}"#);
    assert_eq!(
        serde_json::to_string(&req_focus(2)).unwrap(),
        r#"{"Action":{"FocusWindow":{"id":2}}}"#
    );
    assert_eq!(
        serde_json::to_string(&req_move_to_workspace(7, 1_000_000)).unwrap(),
        r#"{"Action":{"MoveWindowToWorkspace":{"window_id":7,"reference":{"Index":1000000},"focus":false}}}"#
    );
}

#[test]
fn measured_window_fixture_parses() {
    // Verbatim Window object measured on niri 26.04 (spec, 2026-08-22).
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
    // The measured malformed-spelling reply. The unit variant MUST be sent as
    // null; this pins what happens if a future edit sends {} instead.
    let srv = TestSocket::start(vec![r#"{"Err":"error parsing request"}"#.into()]);
    let r: std::result::Result<WindowsReply, _> =
        call(srv.path.as_path(), req_windows());
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
    // focus_timestamp is niri's REAL focus order — sort desc before numbering
    // recency so step 5b lands on the app the user actually left (sway only
    // has tree order; this is the one MRU upgrade niri gives us).
    let mk = |id: u64, app: Option<&str>, secs: i64| NiriWindow {
        id,
        app_id: app.map(String::from),
        title: None,
        is_focused: false,
        focus_timestamp: Some(FocusTimestamp { secs, nanos: 0 }),
    };
    let windows = vec![mk(1, Some("kitty"), 100), mk(2, None, 200), mk(3, Some("claude"), 300)];
    let snaps = snapshots_from(&windows);
    assert_eq!(snaps.len(), 2, "window without app_id is skipped like i3ipc");
    assert_eq!(snaps[0].address, "3", "newest focus_timestamp gets recency 0");
    assert_eq!(snaps[0].recency, 0);
    assert_eq!(snaps[1].address, "1");
    assert_eq!(snaps[1].recency, 1);
}
```

- [ ] **Step 2: Run tests, verify FAIL**

```sh
export CARGO_TARGET_DIR=~/Documents/dev/beckon/target
cargo test -p beckon-linux niri 2>&1 | tail -20
```
Expected: compile error (`niri` module or types not found).

- [ ] **Step 3: Implement the module**

```rust
//! niri Wayland backend over the official IPC socket (`NIRI_SOCKET`).
//!
//! Wire protocol (measured on niri 26.04, 2026-08-22 — see the spec):
//! one JSON line per request, one JSON line per reply, wrapper
//! `{"Ok":T}` / `{"Err":"..."}`. Unit variants are sent as `null`
//! (`{"Windows":null}`), never `{}` — the latter is a parse error.
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

use beckon_core::{Backend, BackendError, BeckonAction, InstalledApp, Result, RunningApp};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::algorithm::{decide, Decision, WindowSnapshot};

/// Step 5c target: far enough that no real session reaches it. niri creates
/// the workspace on demand and FocusWindow later navigates back to it.
const HIDE_WORKSPACE_INDEX: u64 = 1_000_000;

pub struct NiriBackend;

// ---- wire types ----

#[derive(Deserialize)]
struct VersionReply {
    Version: String,
}

#[derive(Deserialize)]
struct WindowsReply {
    Windows: Vec<NiriWindow>,
}

#[derive(Deserialize, Debug, Clone)]
pub(crate) struct NiriWindow {
    pub id: u64,
    /// niri exposes a single class-like field; `None` windows are skipped
    /// exactly as i3ipc skips windows with neither app_id nor class.
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
    let reply: Reply<T> = serde_json::from_str(buf.trim()).map_err(|e| {
        BackendError::Ipc(format!("bad reply `{}`: {e}", buf.trim()))
    })?;
    match reply {
        Reply::Ok { Ok: ok } => Ok(ok),
        Reply::Err { Err: err } => Err(BackendError::Ipc(err)),
    }
}

fn call_env<T: DeserializeOwned>(req: Value) -> Result<T> {
    call(&socket_path()?, req)
}
```

Then `snapshots_from`, `NiriBackend::new` (probes `Version` via `call_env`, so a dead socket errors at construction like `I3IpcBackend::new`), and the `#[cfg(test)]` module with `TestSocket` + the five tests from Step 1. `TestSocket` binds a `UnixListener` in a fresh temp dir, serves one canned reply per accepted connection (read one line, write reply, drop), records request lines into `Arc<Mutex<Vec<String>>>`, stops on an `AtomicBool`.

Also in `lib.rs`, next to the other `pub mod` lines:

```rust
#[cfg(target_os = "linux")]
pub mod niri;
```

- [ ] **Step 4: Run tests, verify PASS**

```sh
cargo test -p beckon-linux niri 2>&1 | tail -20
```
Expected: 5 passed.

- [ ] **Step 5: Commit**

```sh
git commit --only crates/beckon-linux/src/niri.rs --only crates/beckon-linux/src/lib.rs -m "linux: niri wire types, framing, unit tests"
```

---

### Task 2: `Backend` impl + dispatch + doctor + error text

**Files:**
- Modify: `crates/beckon-linux/src/niri.rs` (impl block)
- Modify: `crates/beckon-linux/src/lib.rs:66-112` (`pick_backend`), `lib.rs:140-158` (`detect_compositor`), `lib.rs:96-99` (error text)
- Modify: `crates/beckon-cli/src/lib.rs:989-1000` (`cmd_doctor` env list)

**Interfaces:**
- Consumes: `desktop::resolve`, `desktop::target_classes`, `desktop::scan`, `desktop::visible`, `state::read_previous`, `state::write_previous`, `algorithm::decide` — all with the signatures used by `i3ipc.rs:158-285`.
- Produces: `pick_backend()` returns `NiriBackend` when `NIRI_SOCKET` is set; `detect_compositor()` returns `Some("niri")`.

- [ ] **Step 1: Implement `impl Backend for NiriBackend`** (append to `niri.rs`)

```rust
fn parse_window_id(addr: &str) -> Result<u64> {
    addr.parse::<u64>()
        .map_err(|e| BackendError::Ipc(format!("bad window id `{addr}`: {e}")))
}

/// Same recipe as `gnome.rs`/`kde.rs`/`x11.rs`: niri's IPC has no `exec`
/// action (measured), so launch through `setsid -f` so the app survives
/// beckon exiting.
fn launch_exec(exec: &str) -> Result<()> {
    use std::process::{Command, Stdio};
    Command::new("/bin/sh")
        .arg("-c")
        .arg(format!("setsid -f {} >/dev/null 2>&1", exec))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| BackendError::LaunchFailed {
            id: exec.to_string(),
            reason: e.to_string(),
        })?;
    Ok(())
}
```

Wait — `LaunchFailed.id` should be the beckon id, not exec; match `gnome.rs:227` call-site shape (pass `id` through). Adjust signature to `fn launch_exec(id: &str, exec: &str)`.

Snapshots + beckon body (mirrors `i3ipc.rs::beckon`, with two niri-specific lines):

```rust
fn snapshots_from(windows: &[NiriWindow]) -> Vec<WindowSnapshot> {
    let mut ws: Vec<&NiriWindow> = windows.iter().filter(|w| w.app_id.is_some()).collect();
    // Real focus order: sort newest-first BEFORE numbering recency. sway
    // only has tree order; niri's focus_timestamp is the genuine MRU, so
    // step 5b toggles to the app the user actually left.
    ws.sort_by(|a, b| {
        b.focus_timestamp
            .cmp(&a.focus_timestamp)
            .then_with(|| a.id.cmp(&b.id))
    });
    ws.iter()
        .enumerate()
        .map(|(idx, w)| {
            WindowSnapshot::new(w.id.to_string(), w.app_id.clone().unwrap(), idx as i32)
        })
        .collect()
}
```

`beckon(&self, id)`: `call_env::<WindowsReply>(req_windows())` → snapshots; `active` = the `is_focused` window's id; `pre_focused_app` likewise; `previous_app = state::read_previous()`; `desktop::resolve` + `target_classes`; `decide`. Map:
- `Launch` → `entry.ok_or(BackendError::NoMatch{...})` (copy the i3ipc hint verbatim) → `launch_exec(id, &entry.exec)` → `BeckonAction::Launched`
- `Focus(addr)` → `call_env::<Value>(req_focus(parse_window_id(&addr)?))` → `Focused`
- `Cycle(addr)` → same → `Cycled`
- `ToggleBack(addr)` → same → `ToggledBack`
- `Hide(addr)` → `req_move_to_workspace(parse_window_id(&addr)?, HIDE_WORKSPACE_INDEX)` → `Hidden`
- end: `persist_previous(pre_focused_app.as_deref())` — same tiny helper as `i3ipc.rs:122`.

`list_running`: group by `app_id` in a `BTreeMap<String,(String,usize)>` exactly like `i3ipc.rs:250-264` (name = first window's title or empty). `list_installed`: copy `i3ipc.rs:266-285` body unchanged.

- [ ] **Step 2: Wire dispatch** — `lib.rs`, after the Hyprland block in `pick_backend()`:

```rust
    if std::env::var_os("NIRI_SOCKET").is_some() {
        return Ok(Box::new(niri::NiriBackend::new()?));
    }
```

In `detect_compositor()`, after the Hyprland arm:

```rust
    } else if std::env::var_os("NIRI_SOCKET").is_some() {
        Some("niri")
```

Error text (both places it appears is one place — `lib.rs:96`): change `it is not sway or Hyprland` to `it is not sway, Hyprland or niri`.

- [ ] **Step 3: Doctor env line** — `cmd_doctor`, after the HYPRLAND line:

```rust
        println!("  NIRI_SOCKET                 = {:?}", std::env::var("NIRI_SOCKET").ok());
```

- [ ] **Step 4: Verify** (shared target dir)

```sh
export CARGO_TARGET_DIR=~/Documents/dev/beckon/target
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings 2>&1 | tail -5
cargo test -p beckon-linux 2>&1 | tail -5
```
Expected: all green.

- [ ] **Step 5: Commit**

```sh
git commit --only crates/beckon-linux/src/niri.rs --only crates/beckon-linux/src/lib.rs --only crates/beckon-cli/src/lib.rs -m "linux: NiriBackend + NIRI_SOCKET dispatch, doctor row"
```

---

### Task 3: E2E suite (`niri_e2e.rs`)

**Files:**
- Create: `crates/beckon-cli/tests/niri_e2e.rs`

**Interfaces:**
- Consumes: the built `beckon` binary via `env!("CARGO_BIN_EXE_beckon")`, driven exactly like `hyprland_e2e.rs` (env_clear + explicit env, `BECKON_NO_NOTIFY=1`, confined `XDG_DATA_*`).

- [ ] **Step 1: Write the fake niri server + tests.** Structure copied from `hyprland_e2e.rs`; differences:

Server: `UnixListener` in temp dir; per connection read ONE line, reply ONE line, close. `State { windows: Vec<FakeWindow>, actions: Vec<String> }` with `FakeWindow { id: u64, app_id, title, is_focused, ts_secs: i64 }`.

`handle(line)`:
- key `"Windows"` → `{"Ok":{"Windows":[…]}}`, each window serialized as the measured shape (id/title/app_id/pid/workspace_id/is_focused/is_urgent/layout:null/focus_timestamp {secs: ts_secs, nanos: 0}).
- key `"Version"` → `{"Ok":{"Version":"25.02-test"}}`.
- `Action.FocusWindow{id}` → if found: clear all `is_focused`, set target's, bump its `ts_secs` to `max+1`; push `"focus:<id>"` to `actions`; reply `{"Ok":"Handled"}`. If NOT found: reply `{"Ok":"Handled"}` anyway and change nothing — this pins trap #1.
- `Action.MoveWindowToWorkspace` → push the raw JSON to `actions`; reply `{"Ok":"Handled"}`.
- anything else → `{"Err":"error parsing request"}`.

`run_beckon` env: clear; set `NIRI_SOCKET`, `XDG_RUNTIME_DIR`, `XDG_DATA_HOME`/`XDG_DATA_DIRS` (fixture dir), `HOME`, `PATH`, `BECKON_NO_NOTIFY=1`. Nothing else — `env_clear` is what keeps sway/Hyprland vars from hijacking dispatch.

Tests (each asserts server STATE, never the focus reply):
1. `launch_runs_outside_the_ipc_socket` — `.desktop` with `Exec=/bin/true`; `beckon claude` succeeds; `actions` empty (launch never touches IPC).
2. `launch_without_desktop_entry_errors` — failure, stderr mentions `no .desktop entry` or `no running window`.
3. `focus_picks_the_most_recent_window_and_flips_is_focused` — kitty(id1, ts5, focused), claude id2 ts3, claude id3 ts4 → after `beckon claude`: id3 focused, action `focus:3`.
4. `cycle_rotates_by_address` — claude id2 + id3, id2 focused → action `focus:3`.
5. `toggle_back_follows_focus_timestamp` — claude focused; kitty ts1; firefox ts4 → action `focus:<firefox>`.
6. `mru_file_records_the_app_we_left` — after test-3 flow, `$XDG_RUNTIME_DIR/beckon-mru` == `kitty`.
7. `hide_moves_to_far_workspace_without_focus` — lone focused claude → `actions[0]` contains `"Index":1000000` and `"focus":false`.
8. `focusing_a_missing_id_is_a_silent_noop` — direct `handle()` of `{"Action":{"FocusWindow":{"id":999}}}` → `{"Ok":"Handled"}`, state unchanged (documents trap #1 at the fixture level).
9. `list_running_groups_by_app_id` — claude×2 + kitty → stdout rows with counts 2 and 1.
10. `doctor_reports_niri` — stdout contains `NIRI_SOCKET` and `Backend selected`.
11. `name_resolution_routes_through_desktop_entry` — `beckon Claude` focuses the claude window.

- [ ] **Step 2: Run with a PRIVATE target dir** (the test spawns the real binary)

```sh
unset SWAYSOCK I3SOCK NIRI_SOCKET HYPRLAND_INSTANCE_SIGNATURE
CARGO_TARGET_DIR=/tmp/opencode/beckon-niri-target cargo test -p beckon-cli --test niri_e2e 2>&1 | tail -15
```
Expected: 11 passed. (Re-run once if output is empty — fresh-binary kill rule.)

- [ ] **Step 3: Commit**

```sh
git commit --only crates/beckon-cli/tests/niri_e2e.rs -m "test: niri e2e suite over a fake NIRI_SOCKET server"
```

---

### Task 4: Docs + full gates

**Files:**
- Modify: `CLAUDE.md` (Linux dispatch table: add `NIRI_SOCKET | niri | ✅` row; add niri to the pick_backend snippet comment)
- Modify: `docs/notes/linux-backends.md` (short niri section: wire format, the three measured traps, the GPL license decision, `focus_timestamp` MRU note)

- [ ] **Step 1: Docs.** Keep it to the table row + one section; no README/site changes unless they already list compositors (check `rg -i hyprland README.md site/index.html` — update only where Hyprland already appears).

- [ ] **Step 2: Full gates** (shared target for lint, private for test):

```sh
export CARGO_TARGET_DIR=~/Documents/dev/beckon/target
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings 2>&1 | tail -3
cargo clippy --target aarch64-pc-windows-msvc --all-targets -- -D warnings 2>&1 | tail -3
CARGO_TARGET_DIR=/tmp/opencode/beckon-niri-target cargo test --workspace 2>&1 | grep -E "test result|error" | tail -20
```
Expected: fmt clean, both clippies clean, every `test result: ok`.

- [ ] **Step 3: Acceptance sweep against the spec** — items provable without a live compositor are covered by Task 3's suite; the live `NIRI_SOCKET` checks (doctor/focus/launch on rog) are manual follow-ups on the target machine, listed in the PR body.

- [ ] **Step 4: Commit docs**

```sh
git commit --only CLAUDE.md --only docs/notes/linux-backends.md -m "docs: niri backend notes and dispatch table"
```

## Self-Review

- Spec coverage: framing (T1), pick_backend + detect_compositor + error text (T2), doctor (T2/T3), unit tests incl. `{"Windows":{}}→Err` (T1), e2e with is_focused confirmation + setsid launch (T3), MRU timestamp note (T1/T3), no-touch-others (gates). Launch-process-survives check is recipe-identical to three existing backends; live verification deferred to rog.
- Placeholders: none — all code shown or referenced by exact file:line.
- Type consistency: `NiriWindow`/`FocusTimestamp`/`call`/`call_env`/`req_*` names match across tasks; `HIDE_WORKSPACE_INDEX = 1_000_000` matches the e2e assertion.
