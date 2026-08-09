# `--serve` background running Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let `beckon --serve` run at logon with no console window and no deprecated VBScript shim on Windows, and with a one-command install on macOS — without adding a daemon to beckon itself.

**Architecture:** A new Windows-only `--log <PATH>` flag opens a log file, points std's stderr and stdout at it with `SetStdHandle`, then calls `FreeConsole()`. Because std's Windows stdio resolves `GetStdHandle` on *every* write instead of caching it, this redirects all 105 existing print sites with zero call-site changes. The Scheduled Task then invokes `beckon.exe` directly — no `cmd.exe` for the `2>` redirect, no `wscript.exe` to hide the window. Separately, a `service do` block in the Homebrew formula template gives macOS `brew services start beckon`.

**Tech Stack:** Rust 2021 (workspace floor 1.75), `windows` crate 0.61, clap 4 derive, Homebrew formula DSL, Windows Task Scheduler XML.

## Global Constraints

- **`--serve` stays a foreground process.** launchd and Task Scheduler both require it, and Windows `RegisterHotKey` additionally requires an interactive desktop session (`crates/beckon-windows/src/hotkey.rs:195` already warns on session 0). Nothing in this plan forks, re-execs, or detaches the process itself.
- **`--log` exists only on Windows** (`#[cfg(target_os = "windows")]` on the clap field) and only with `--serve` (clap `requires = "serve"`). macOS gets the same result from the plist's `StandardErrorPath`; a Unix implementation would need `dup2`, i.e. a new `libc` dependency in `beckon-cli`, for no gain.
- **Console detach happens if and only if `--log` was given.** This coupling is load-bearing, not a convenience: detaching without a redirect would leave stderr pointing at a destroyed console, and `std::io::stdio::print_to` panics on any write error that is not `ERROR_INVALID_HANDLE` (6). One flag expresses the whole "running under a supervisor" intent, and makes the dangerous combination unrepresentable.
- **Everything fallible must happen before `FreeConsole()`.** `main` reports errors with `eprintln!` (`crates/beckon-cli/src/main.rs:74`); an `Err` returned from after the detach would turn `exit(1)` into a silent panic. The redirect function therefore does all of `create_dir_all` / `open` / the first `SetStdHandle` while the console is still alive, and treats the detach and everything after it as best-effort.
- **The `windows` crate lives in `beckon-windows`, not `beckon-cli`.** `crates/beckon-cli/Cargo.toml` depends only on beckon-core, anyhow, clap, fs4 and notify. The Win32 code goes in `crates/beckon-windows/src/logfile.rs` and beckon-cli calls it behind `#[cfg(target_os = "windows")]`.
- **No `Co-Authored-By` lines in commit messages.** Repo convention: conventional-commit subject (`feat(cli):`, `fix(cli):`, `docs:`, `test:`), lowercase, with a body explaining *why*.
- **Local verification is type-check only.** `cargo build` / `cargo test` for a Windows target fail on this macOS host with `error: linker link.exe not found`. `cargo check` and `cargo clippy` work fully and resolve real Win32 types. Behavioral verification happens on CI (`windows-latest`) and on the a14 box.

### Local verification commands (run after every Rust edit)

```bash
cd /Users/lenamkhanh/Documents/dev/beckon

# CI parity for the windows-latest job, plus --target. Exit 0 here => that job passes.
cargo clippy --target x86_64-pc-windows-msvc --workspace \
    --exclude beckon-linux --exclude beckon-macos --all-targets -- -D warnings

# The a14 box's native triple; release.yml builds it too.
cargo check --target aarch64-pc-windows-msvc --workspace --all-targets

# The macOS CI job must stay green as well (beckon-cli is shared).
cargo clippy --workspace --exclude beckon-linux --exclude beckon-windows --all-targets -- -D warnings
cargo test --workspace --exclude beckon-linux --exclude beckon-windows

# Separate CI job, target-independent, easy to forget.
cargo fmt --all -- --check
```

Both Windows targets are already installed — do **not** run `rustup target add`. Cold check ~22s, incremental ~0.5s.

---

### Task 1: Windows log-file redirect module

**Files:**
- Create: `crates/beckon-windows/src/logfile.rs`
- Modify: `crates/beckon-windows/Cargo.toml` (add one `windows` feature + a dev-dependency)
- Modify: `crates/beckon-windows/src/lib.rs` (declare the module)
- Test: `crates/beckon-windows/src/logfile.rs` (inline `#[cfg(test)] mod tests`)

**Interfaces:**
- Consumes: nothing from earlier tasks.
- Produces: `pub fn beckon_windows::logfile::redirect_to_log(path: &std::path::Path) -> anyhow::Result<()>` — Task 2 calls exactly this. Also `pub(crate) fn open_log(path: &Path) -> std::io::Result<std::fs::File>`, used only by this module's tests.

- [ ] **Step 1: Add the cargo feature and dev-dependency**

`crates/beckon-windows/Cargo.toml`: insert `"Win32_System_Console",` into the `features` list, keeping alphabetical order — immediately after `"Win32_System_Com_StructuredStorage",`. `HANDLE` comes from `Win32_Foundation`, which is already enabled; `Win32_System_Console` pulls `Win32_System` itself.

Then append a dev-dependencies section to the same file (the crate has none today):

```toml
[dev-dependencies]
tempfile = "3"
```

- [ ] **Step 2: Write the failing test**

Create `crates/beckon-windows/src/logfile.rs` with only the test module and a stub, so the test compiles and fails for the right reason:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_log_creates_missing_parent_directories() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("does/not/exist/serve.log");
        let _file = open_log(&path).expect("open_log");
        assert!(path.exists(), "open_log must create the parent directory");
    }

    #[test]
    fn open_log_appends_instead_of_truncating() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("serve.log");
        std::fs::write(&path, "previous run\n").unwrap();

        {
            use std::io::Write;
            let mut file = open_log(&path).expect("open_log");
            writeln!(file, "this run").unwrap();
        }

        let body = std::fs::read_to_string(&path).unwrap();
        assert!(
            body.contains("previous run") && body.contains("this run"),
            "append mode must keep the previous run's evidence: {body:?}"
        );
    }
}
```

- [ ] **Step 3: Run the check to verify it fails**

Run: `cargo check --target x86_64-pc-windows-msvc -p beckon-windows --all-targets`
Expected: FAIL with `error[E0425]: cannot find function 'open_log' in this scope` (and `error[E0432]` for the module not being declared, until Step 5).

Note: the test *body* cannot be executed on this macOS host — `cargo test` cannot link for a Windows target. The compile failure is the local signal; the assertion runs on CI's `windows-latest` job.

- [ ] **Step 4: Write the implementation**

Prepend to `crates/beckon-windows/src/logfile.rs` (above the test module):

```rust
//! `--log <PATH>`: send this process's stderr and stdout to a file, then
//! detach from the console.
//!
//! Exists so a Scheduled Task can invoke `beckon.exe` directly. Before this,
//! the task went through `cmd.exe` purely for a `2>` redirect (Task Scheduler
//! discards a process's stderr, and stderr is where beckon reports how many
//! hotkeys actually registered) — and then through a `wscript.exe` VBScript
//! shim purely to hide the console window `cmd.exe` left on the desktop.
//! VBScript is a deprecated feature-on-demand; this module removes both hops.
//!
//! **Why no call site had to change.** std's Windows stdio deliberately does
//! not cache the handle: `sys::stdio::windows::write` calls
//! `get_handle(STD_ERROR_HANDLE)` -> `GetStdHandle` on *every* write, with a
//! comment naming `SetStdHandle` as the reason (rust-lang/rust#40490). So
//! swapping the handle redirects every `eprintln!` in the process, including
//! the ones in beckon-cli and this crate. std pins this with its own
//! regression test, `library/std/tests/switch-stdout.rs`. Verified identical
//! at the workspace's 1.75 floor and at 1.97.
//!
//! **Ordering is load-bearing.** Everything that can fail runs while the
//! console is still attached, because `main` reports errors with `eprintln!`
//! and `std::io::stdio::print_to` panics rather than returning on a write
//! error. An `Err` from after the detach would turn a clean `exit(1)` into a
//! silent panic in a process nobody is watching.

use anyhow::{Context, Result};
use std::fs::{File, OpenOptions};
use std::os::windows::io::AsRawHandle;
use std::path::Path;
use std::sync::OnceLock;

use windows::Win32::Foundation::HANDLE;
use windows::Win32::System::Console::{
    FreeConsole, SetStdHandle, STD_ERROR_HANDLE, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE,
};

/// The redirected file, parked in a `static` so it is never dropped.
///
/// `SetStdHandle` stores the handle value without duplicating it, so dropping
/// the `File` would `CloseHandle` it and leave stderr pointing at a dead — and
/// eventually recycled — handle value. A `static` is the cheapest way to say
/// "this lives as long as the process", which is exactly the lifetime the std
/// handle slots need.
static LOG_FILE: OnceLock<File> = OnceLock::new();

/// Open `path` for appending, creating its parent directory if needed.
///
/// Append, not truncate, and that is a deliberate change from the `cmd /c …
/// 2> log` this replaces: `2>` truncates on every start, so under the task's
/// `RestartOnFailure` the log explaining why the daemon died was destroyed by
/// the restart that followed it.
fn open_log(path: &Path) -> std::io::Result<File> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    OpenOptions::new().create(true).append(true).open(path)
}

/// Point stderr and stdout at `path`, then detach the console.
///
/// Call once, from the `--serve` path only, before anything else can fail.
pub fn redirect_to_log(path: &Path) -> Result<()> {
    let file = open_log(path).with_context(|| format!("open log file `{}`", path.display()))?;
    // Park the owner before publishing the handle: the moment `SetStdHandle`
    // returns, any thread may write through it.
    let file = LOG_FILE.get_or_init(|| file);
    let handle = HANDLE(file.as_raw_handle());

    // The last fallible step, and it runs with the console still attached so a
    // failure can still be reported the ordinary way.
    unsafe { SetStdHandle(STD_ERROR_HANDLE, handle) }.context("SetStdHandle(stderr)")?;

    // Everything below is best-effort by design — see the module doc.
    unsafe {
        // stdout too, so the detach below leaves no slot pointing at the dead
        // console. `--serve` logs through stderr; stdout is a `LineWriter` std
        // only flushes on newline or at a normal exit, and `--serve` exits by
        // being killed.
        let _ = SetStdHandle(STD_OUTPUT_HANDLE, handle);
        let _ = FreeConsole();
        // Re-arm: whether `FreeConsole` clears the slots is undocumented, and
        // two syscalls are cheaper than depending on the answer.
        let _ = SetStdHandle(STD_ERROR_HANDLE, handle);
        let _ = SetStdHandle(STD_OUTPUT_HANDLE, handle);
        // stdin has no replacement, and leaving it pointing at a closed console
        // handle would hand that stale value to any child spawned with the
        // default inherit behaviour. NULL is what std itself treats as "no
        // handle".
        let _ = SetStdHandle(STD_INPUT_HANDLE, HANDLE(std::ptr::null_mut()));
    }
    Ok(())
}
```

- [ ] **Step 5: Declare the module**

In `crates/beckon-windows/src/lib.rs`, add alongside the existing per-item `#[cfg(target_os = "windows")]` declarations (every item in that file is individually gated — match the surrounding style exactly):

```rust
#[cfg(target_os = "windows")]
pub mod logfile;
```

- [ ] **Step 6: Verify it type-checks and lints clean**

Run:
```bash
cargo check --target x86_64-pc-windows-msvc -p beckon-windows --all-targets
cargo clippy --target x86_64-pc-windows-msvc -p beckon-windows --all-targets -- -D warnings
```
Expected: exit 0, no warnings.

If clippy reports `unnecessary_cast` on the `HANDLE(...)` line, the cast is redundant — `std::os::windows::io::RawHandle` already *is* `*mut c_void`. The code above is written without a cast for this reason; do not add one.

If it reports `unresolved import windows::Win32::System::Console`, Step 1's feature was not added.

- [ ] **Step 7: Commit**

```bash
git add crates/beckon-windows/Cargo.toml crates/beckon-windows/src/lib.rs crates/beckon-windows/src/logfile.rs
git commit -m "feat(windows): redirect stderr to a file and detach the console

Task Scheduler cannot redirect a process's stderr, so the task went
through cmd.exe for a 2> — which left a console window on the desktop,
which needed a second hop through a VBScript shim to hide. VBScript is a
deprecated feature-on-demand, so that install is on a clock.

SetStdHandle is enough on its own: std's Windows stdio resolves
GetStdHandle on every write rather than caching it (rust-lang/rust#40490),
so one swap redirects all 105 print sites with no call-site changes.

Everything fallible runs before FreeConsole. main reports errors with
eprintln!, and print_to panics rather than returning on a write error, so
an Err from after the detach would turn exit(1) into a silent panic."
```

---

### Task 2: The `--log` flag

**Files:**
- Modify: `crates/beckon-cli/src/main.rs` (the `Args` struct around line 62, and `run()` around line 108)

**Interfaces:**
- Consumes: `beckon_windows::logfile::redirect_to_log(&Path) -> anyhow::Result<()>` from Task 1.
- Produces: the CLI surface `beckon --serve <CONFIG> --log <PATH>`, which Task 3 tests and Task 4's Scheduled Task XML invokes.

- [ ] **Step 1: Add the flag to `Args`**

In `crates/beckon-cli/src/main.rs`, immediately after the `serve` field (which ends at line 62) and before `verbose`:

```rust
    /// Send stderr to PATH and detach the console (Windows, with --serve).
    ///
    /// For supervisor-hosted runs: a Scheduled Task cannot redirect stderr,
    /// and stderr is the only place beckon reports how many hotkeys actually
    /// registered. Detaching the console is part of the same flag on purpose
    /// — detaching without redirecting would leave stderr pointing at a
    /// destroyed console, where a failed write panics instead of returning.
    #[cfg(target_os = "windows")]
    #[arg(long, value_name = "PATH", requires = "serve")]
    log: Option<std::path::PathBuf>,
```

- [ ] **Step 2: Wire it into `run()`**

In `run()`, replace the existing serve branch (lines 109-120) with:

```rust
    if let Some(path) = args.serve.as_deref() {
        #[cfg(target_os = "windows")]
        {
            // Before the lock, so the "already running" refusal is logged too,
            // and before anything else can fail — see logfile's module doc.
            if let Some(log) = args.log.as_deref() {
                beckon_windows::logfile::redirect_to_log(log)?;
            }
            return serve::cmd_serve(path);
        }
        #[cfg(target_os = "macos")]
        {
            return serve::cmd_serve(path);
        }
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        {
            let _ = path;
            return Err(anyhow!("--serve is only implemented on macOS and Windows"));
        }
    }
```

- [ ] **Step 3: Verify all three platforms still compile and lint**

Run:
```bash
cargo clippy --target x86_64-pc-windows-msvc --workspace --exclude beckon-linux --exclude beckon-macos --all-targets -- -D warnings
cargo clippy --workspace --exclude beckon-linux --exclude beckon-windows --all-targets -- -D warnings
cargo fmt --all -- --check
```
Expected: all exit 0. The second command is what catches an accidentally ungated `args.log` reference on macOS.

- [ ] **Step 4: Confirm the flag is absent on macOS**

Run: `cargo run -q -- --serve /tmp/x.toml --log /tmp/x.log`
Expected: clap exits non-zero with `unexpected argument '--log' found`. The flag is Windows-only and must not silently no-op elsewhere.

- [ ] **Step 5: Commit**

```bash
git add crates/beckon-cli/src/main.rs
git commit -m "feat(cli): add --log for supervisor-hosted --serve on Windows

One flag, two effects, deliberately fused: redirect stderr to the file
and detach the console. Detaching without redirecting would leave stderr
pointing at a destroyed console, where print_to panics rather than
returning — so the dangerous half cannot be asked for on its own.

Windows-only. macOS gets the same result from the launchd plist's
StandardErrorPath, and a Unix implementation would need dup2, i.e. a libc
dependency in beckon-cli, for nothing."
```

---

### Task 3: Integration tests for `--log`

**Files:**
- Create: `crates/beckon-cli/tests/serve_log.rs`

**Interfaces:**
- Consumes: the `beckon --serve <CONFIG> --log <PATH>` CLI from Task 2, and the existing `mod common; use common::beckon;` helper (`crates/beckon-cli/tests/common/mod.rs`), which spawns `env!("CARGO_BIN_EXE_beckon")` with `BECKON_NO_NOTIFY=1` set.
- Produces: nothing consumed by later tasks.

These tests use a **deliberately unreadable config** so `--serve` exits instead of running forever — the same trick `crates/beckon-cli/tests/serve.rs` already uses. The redirect is installed before the config is read, so the resulting error is what lands in the log.

- [ ] **Step 1: Write the failing tests**

Create `crates/beckon-cli/tests/serve_log.rs`:

```rust
//! `--log`: the flag a Scheduled Task uses instead of a `cmd.exe` redirect.
//!
//! Windows-only, because the flag is. Every test drives `--serve` at a config
//! that cannot be read, which is the only way to make a resident service exit
//! on its own — and, conveniently, produces the error line we then look for in
//! the log.

#![cfg(target_os = "windows")]

mod common;

use common::beckon;
use std::path::Path;

/// Run `--serve` against a config that does not exist, logging to `log`.
fn serve_failing(log: &Path) -> std::process::Output {
    beckon()
        .arg("--serve")
        .arg("Z:\\nonexistent\\beckon-test-apps.toml")
        .arg("--log")
        .arg(log)
        .output()
        .expect("run beckon")
}

#[test]
fn log_captures_stderr_that_would_otherwise_be_lost() {
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("serve.log");

    let out = serve_failing(&log);

    assert!(!out.status.success(), "a missing config must exit non-zero");
    let body = std::fs::read_to_string(&log).expect("log file must exist");
    assert!(
        body.contains("beckon:"),
        "the error belongs in the log, not on a console nobody reads: {body:?}"
    );
    assert!(
        out.stderr.is_empty(),
        "stderr was redirected, so the pipe must be empty: {:?}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn log_creates_missing_parent_directories() {
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("does/not/exist/serve.log");

    serve_failing(&log);

    assert!(
        log.exists(),
        "a Scheduled Task cannot run mkdir first, so beckon must"
    );
}

#[test]
fn log_appends_rather_than_truncating() {
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("serve.log");
    std::fs::write(&log, "evidence from the previous run\n").unwrap();

    serve_failing(&log);

    let body = std::fs::read_to_string(&log).unwrap();
    assert!(
        body.contains("evidence from the previous run"),
        "RestartOnFailure must not destroy the log explaining the failure: {body:?}"
    );
    assert!(body.contains("beckon:"), "and this run must be there too: {body:?}");
}

#[test]
fn log_without_serve_is_rejected() {
    let out = beckon()
        .args(["--log", "C:\\Windows\\Temp\\beckon-test.log"])
        .output()
        .expect("run beckon");
    assert!(!out.status.success());
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("--serve"),
        "clap must name the missing requirement"
    );
}
```

- [ ] **Step 2: Verify the test file compiles for Windows**

Run: `cargo check --target x86_64-pc-windows-msvc -p beckon-cli --all-targets`
Expected: exit 0.

Then confirm the file is inert elsewhere: `cargo test -p beckon-cli` on this macOS host must still pass, compiling `serve_log.rs` to nothing because of the crate-level `#![cfg(target_os = "windows")]`.

- [ ] **Step 3: Verify clippy parity**

Run: `cargo clippy --target x86_64-pc-windows-msvc --workspace --exclude beckon-linux --exclude beckon-macos --all-targets -- -D warnings`
Expected: exit 0.

- [ ] **Step 4: Commit**

```bash
git add crates/beckon-cli/tests/serve_log.rs
git commit -m "test: pin --log's redirect, append and mkdir behaviour

These run on CI's windows-latest job; they cannot run on a macOS host,
where cargo test for a Windows target fails at link. Each drives --serve
at an unreadable config, which is the only way a resident service exits
on its own and also produces the error line the assertions look for.

The append test is the one that would otherwise rot: truncate-on-open
looks correct until RestartOnFailure destroys the log explaining the
failure it is restarting from."
```

---

### Task 4: Scheduled Task with one action, and the VBS shim deleted

**Files:**
- Modify: `examples/windows/serve/beckon-serve.xml`
- Delete: `examples/windows/serve/beckon-serve.vbs`
- Modify: `examples/windows/serve/README.md`

**Interfaces:**
- Consumes: `beckon --serve <CONFIG> --log <PATH>` from Task 2.
- Produces: nothing consumed by later tasks.

- [ ] **Step 1: Replace the actions block**

In `examples/windows/serve/beckon-serve.xml`, replace the whole `<Actions Context="Author">` element (lines 96-107) with:

```xml
  <Actions Context="Author">
    <Exec>
      <Command>C:\Users\YOUR_USERNAME\.cargo\bin\beckon.exe</Command>
      <Arguments>--serve "C:\Users\YOUR_USERNAME\.config\beckon\apps.toml" --log "C:\Users\YOUR_USERNAME\AppData\Local\beckon\serve.log"</Arguments>
    </Exec>
  </Actions>
```

Leave `<Triggers>`, `<Principals>` and `<Settings>` exactly as they are — the SID principal, `InteractiveToken`, `PT0S` execution limit and `RestartOnFailure` are all still load-bearing and were verified against a real machine.

- [ ] **Step 2: Update the XML header comment**

Replace the paragraph beginning "The action below goes through cmd.exe purely to get the 2> redirect:" (lines 33-41) with:

```
  beckon writes its own log. The flag spelled out in Arguments below sends
  stderr to a file and detaches the console in one step, so this task runs
  beckon.exe directly: no cmd.exe for a redirect, and no wscript.exe shim to
  hide the window cmd.exe would have left behind. Task Scheduler discards a
  process's stderr, and stderr is where beckon reports how many hotkeys
  actually registered -- without the log there is no way to tell "20
  registered" from "20 parsed, 0 registered".

  The log is APPENDED to, not truncated. The cmd.exe redirect this replaces
  truncated on every start, so RestartOnFailure destroyed the log explaining
  the failure it was restarting from.
```

(Keep it free of double hyphens outside that quoted text — XML comments cannot contain `--`, which is why the flag name is spelled out only in `<Arguments>`.)

- [ ] **Step 3: Delete the shim**

```bash
git rm examples/windows/serve/beckon-serve.vbs
```

- [ ] **Step 4: Rewrite the README sections**

In `examples/windows/serve/README.md`:

1. In the install snippet (lines 52-65), delete the `mkdir -Force (Split-Path $log) | Out-Null` line — beckon creates the directory itself now. Keep every other line, including the `$sid` lookup.
2. Replace the whole **"## The console window"** section (lines 108-131) with:

```markdown
## The console window

There isn't one. `--log` sends stderr to the file and calls `FreeConsole()`
in the same step, so the task runs `beckon.exe` directly and the console
Windows allocates for it closes immediately.

Earlier versions needed two extra hops for this — `cmd.exe` for the `2>`
redirect, then a `wscript.exe` VBScript shim to hide the window `cmd.exe`
left behind. VBScript is a deprecated feature-on-demand, so that install
was on a clock. Both are gone.

What may remain is a **brief flash** at logon: Task Scheduler has no way to
start a console-subsystem process without allocating a console first, and
`<Hidden>` in the task XML hides the task from the Task Scheduler UI, not
the window. If that ever becomes intolerable the next step is a separate
GUI-subsystem `beckon-serve.exe`; a whole-binary `windows_subsystem =
"windows"` is not an option, because it would silently swallow the output
of `beckon -l`, `-L`, `-s`, `-r` and `-d`.
```

3. In **"## The log"** (lines 95-106), replace "The task action runs beckon through `cmd.exe` for one reason: the `2>` redirect." with:

```markdown
The task action passes `--log`. Task Scheduler throws a process's stderr
away, and stderr is the only place beckon reports **how many hotkeys
actually registered**.

The log is **appended** to, so a restart does not destroy the lines
explaining what it is restarting from. Nothing rotates it; if it ever grows
inconvenient, delete it while the daemon is stopped.
```

4. In the watchdog section (line 172), replace "Point its `2>` at a *different* log file" with "Point its `--log` at a *different* file".
5. In Troubleshooting (line 209-212), replace "that is what the `2>` redirect in the task action is for" with "that is what `--log` in the task action is for".

- [ ] **Step 5: Verify no stale references survive**

Run:
```bash
grep -rn "beckon-serve.vbs\|wscript\|cmd.exe\|2>" examples/windows/ || echo "clean"
```
Expected: `clean`, or only hits inside the "Earlier versions needed…" paragraph you just wrote.

- [ ] **Step 6: Commit**

```bash
git add -A examples/windows/serve/
git commit -m "docs(windows): one-action Scheduled Task, no VBScript shim

The task ran beckon through cmd.exe for a 2> redirect, which left a
console window, which needed a wscript.exe shim to hide. beckon does the
redirect itself now, so both hops are gone and with them a dependency on
a deprecated Windows feature-on-demand.

The README's own note that 'the real fix is a FreeConsole() on the --serve
path in beckon itself, which does not exist yet' is now obsolete."
```

---

### Task 5: `brew services` on macOS

**Files:**
- Modify: `packaging/homebrew/beckon.rb.template`
- Modify: `examples/macos/serve/README.md`
- Modify: `README.md`

**Interfaces:**
- Consumes: nothing from earlier tasks (`--serve` already exists on macOS).
- Produces: nothing consumed by later tasks.

The template lives in this repo; `.github/workflows/bump-packagers.yml` substitutes only `{{VERSION}}` and the four `{{SHA256_*}}` placeholders before pushing to `xom11/homebrew-tap`, so anything else added here propagates on the next release and survives every future bump.

- [ ] **Step 1: Add the service block**

In `packaging/homebrew/beckon.rb.template`, insert between the `def install … end` block and `test do`:

```ruby
  # `--serve` (the resident hotkey host) exists only on macOS and Windows; on
  # Linux the compositor owns the keybind, so there is deliberately no service
  # there. `OS.mac?` is evaluated when the formula is loaded, so on Linux no
  # service is registered at all and beckon never appears in `brew services
  # list`.
  #
  # Do NOT nest this in `on_macos do` — `brew style` and `brew audit --strict`
  # both reject that outright ("on_macos cannot include service"), and no
  # formula in homebrew-core does it. Do NOT "simplify" it to `run macos:`
  # either: that leaves `service?` true on Linux, so `brew services start`
  # there fails with "has not implemented #plist, #service or provided a
  # locatable service file" — a broken service instead of no service.
  if OS.mac?
    service do
      run [opt_bin/"beckon", "--serve", "#{Dir.home}/.config/beckon/apps.toml"]
      keep_alive true
      process_type :interactive
      log_path var/"log/beckon.log"
      error_log_path var/"log/beckon.log"
    end
  end

  def caveats
    return unless OS.mac?

    <<~EOS
      Resident hotkey mode reads a shortcuts file. Create and validate it
      BEFORE starting the service — `keep_alive` restarts a serve that cannot
      read its config every ~10 seconds, forever:

        mkdir -p ~/.config/beckon
        printf '"ctrl+super+alt+t" = "kitty"\\n' > ~/.config/beckon/apps.toml
        beckon --check ~/.config/beckon/apps.toml
        brew services start beckon

      Focusing other apps needs Accessibility permission:
      System Settings -> Privacy & Security -> Accessibility.
    EOS
  end
```

`log_path` uses `var`, not `Dir.home`: Homebrew creates a referenced `#{var}/log` directory at install time, and gives no such treatment to a home-relative path — launchd would then fail to open it silently. `Dir.home` is correct for the *config* path and has direct homebrew-core precedent (`sleepwatcher.rb` is nearly the same shape).

- [ ] **Step 2: Validate the rendered formula**

```bash
cd /Users/lenamkhanh/Documents/dev/beckon
sed -e 's|{{VERSION}}|0.5.2|g' \
    -e 's|{{SHA256_DARWIN_ARM}}|0000000000000000000000000000000000000000000000000000000000000000|g' \
    -e 's|{{SHA256_DARWIN_X86}}|0000000000000000000000000000000000000000000000000000000000000000|g' \
    -e 's|{{SHA256_LINUX_ARM}}|0000000000000000000000000000000000000000000000000000000000000000|g' \
    -e 's|{{SHA256_LINUX_X86}}|0000000000000000000000000000000000000000000000000000000000000000|g' \
    packaging/homebrew/beckon.rb.template > /tmp/beckon.rb
ruby -c /tmp/beckon.rb
brew style /tmp/beckon.rb
```

Expected: `Syntax OK`, and `brew style` reporting **no offenses**.

If `brew style` reports `FormulaAudit/ComponentsOrder: on_macos cannot include service`, the block was nested in `on_macos` — it must be a top-level `if OS.mac?`.

`brew audit --strict` additionally reports `Stable: version 0.5.2 is redundant with version scanned from URL`. That finding is **pre-existing** and unrelated to this change; `bump-packagers.yml` never runs audit. Do not "fix" it here.

- [ ] **Step 3: Add the Homebrew path to the macOS README**

In `examples/macos/serve/README.md`, insert a new section immediately before **"## Load at login via launchd"**:

```markdown
## Load at login via `brew services` (Homebrew installs)

If you installed with `brew install xom11/tap/beckon`, the formula ships a
LaunchAgent already:

```sh
brew services start beckon
brew services list                       # beckon should be `started`
tail -f "$(brew --prefix)/var/log/beckon.log"
```

It reads `~/.config/beckon/apps.toml` — create and `beckon --check` it
first, because `keep_alive` will otherwise restart the failing service
every ~10 seconds.

Two ways this ships broken, both silent:

- **`sudo brew services start`** installs a LaunchDaemon instead of a
  per-user LaunchAgent. A daemon has no window-server session, so
  `RegisterEventHotKey` succeeds and no key ever fires. Never use `sudo`
  here.
- **Starting it over SSH** can drop the agent out of the `gui/<uid>` domain
  for the same reason. Start it from a terminal in the desktop session.

Confirm you got the right domain:

```sh
launchctl print gui/$(id -u)/homebrew.mxcl.beckon | head -20
```

The hand-written plist below is the fallback for non-Homebrew installs, and
still the reference for what the agent actually does.
```

Then change the existing heading "## Load at login via launchd" to "## Load at login via launchd (manual install)".

- [ ] **Step 4: Mention it in the root README**

In `README.md`, in the "## Resident mode (macOS & Windows)" section (around line 219), after the sentence pointing at the ready-to-use setups, add:

```markdown
On macOS installed via Homebrew, `brew services start beckon` is the whole
install — the formula ships the LaunchAgent. Create and `beckon --check`
`~/.config/beckon/apps.toml` first.
```

- [ ] **Step 5: Commit**

```bash
git add packaging/homebrew/beckon.rb.template examples/macos/serve/README.md README.md
git commit -m "feat(packaging): ship a LaunchAgent in the Homebrew formula

brew services start beckon replaces a sed-the-plist-then-launchctl-bootstrap
install. The template lives here and bump-packagers.yml only substitutes the
version and four hashes, so this propagates on the next release.

Guarded with a top-level 'if OS.mac?', not 'on_macos do' (brew style rejects
a service there outright) and not 'run macos:' (which leaves service? true on
Linux, so brew services start fails there instead of the formula simply
having no service). log_path uses var so brew creates the directory; Dir.home
would leave launchd failing to open it silently."
```

---

### Task 6: Record the decisions in CLAUDE.md

**Files:**
- Modify: `CLAUDE.md`

**Interfaces:**
- Consumes: everything above.
- Produces: nothing.

Three things here are non-obvious enough that a future reader would re-derive them wrongly, and one is a rejected design that will otherwise be re-proposed.

- [ ] **Step 1: Update the resident-mode entry in "Open questions"**

In `CLAUDE.md`, in open question 1 ("Daemon vs one-shot CLI"), append to the decided paragraph:

```markdown
   beckon itself never daemonizes, and that is a decision, not an omission.
   Surveyed skhd, yabai, espanso, kanata, AutoHotkey and caddy: effectively
   no hotkey daemon forks. On macOS a detached process loses the login
   session's bootstrap namespace — beckon already needs
   `TransformProcessType(→ UIElement)` because a launchd-spawned process has
   no window-server identity, and without one `RegisterEventHotKey` returns
   success while never delivering a press. On Windows there is no `fork` at
   all. Above all it solves the wrong problem: forking buys "survives closing
   the terminal", while what users need is "starts at login" and "restarts if
   it dies" — both of which still require launchd / Task Scheduler
   afterwards. The ergonomic step that *is* open is a `--service
   install/start/stop` subcommand (the skhd / espanso pattern); it is not
   built.
```

- [ ] **Step 2: Add a `--log` note to the Phase 3 Windows section**

In `CLAUDE.md`, in "### Phase 3 Windows notes (for future maintenance)", add a bullet after the "Toast notifications" one:

```markdown
- **`--log <PATH>` (with `--serve`) redirects stderr and detaches the
  console** — `crates/beckon-windows/src/logfile.rs`. It exists so a
  Scheduled Task can run `beckon.exe` directly: Task Scheduler cannot
  redirect stderr, so the task used to go through `cmd.exe` for a `2>`, which
  left a console window, which needed a `wscript.exe` VBScript shim to hide.
  VBScript is a deprecated feature-on-demand; both hops are gone.
    - **Why no call site changed.** std's Windows stdio resolves
      `GetStdHandle` on *every* write instead of caching it, with a comment
      naming `SetStdHandle` as the reason (rust-lang/rust#40490), and std
      pins it with `library/std/tests/switch-stdout.rs`. One swap redirects
      all 105 print sites. Verified identical at the 1.75 floor and at 1.97.
    - **Redirect and detach are one flag on purpose.** Detaching without
      redirecting leaves stderr pointing at a destroyed console, and
      `print_to` panics rather than returning on a write error that is not
      `ERROR_INVALID_HANDLE`. Fusing them makes that state unrepresentable.
    - **Everything fallible runs before `FreeConsole`**, because `main`
      reports errors with `eprintln!` — an `Err` from after the detach turns
      `exit(1)` into a silent panic.
    - **Append, not truncate.** `2>` truncated on every start, so
      `RestartOnFailure` destroyed the log explaining the failure it was
      restarting from.
    - **Pre-existing hazard this does not fix**: whenever stderr is a file
      (which was already true under `cmd /c … 2>`), a write failure — full
      disk, disconnected network share — panics the printing thread rather
      than returning an error. In `--serve` that surfaces as "hotkeys
      silently stop" rather than a crash.
```

- [ ] **Step 3: Note the Homebrew service in Distribution**

In `CLAUDE.md`, under "## Distribution", after the Homebrew tap bullet:

```markdown
- The formula ships a **macOS LaunchAgent** (`service do` in
  `packaging/homebrew/beckon.rb.template`), so `brew services start beckon`
  is the whole resident-mode install. Guarded by a top-level `if OS.mac?`:
  `brew style` rejects a `service` block nested in `on_macos do`, and the
  `run macos:` form leaves `service?` true on Linux (where `--serve` does not
  exist), which makes `brew services start` fail there instead of the formula
  simply having no service.
```

- [ ] **Step 4: Commit**

```bash
git add CLAUDE.md
git commit -m "docs: record why beckon does not daemonize, and how --log works

The no-fork decision has a real rationale (macOS bootstrap namespace, no
fork on Windows, and it solves the wrong problem anyway) and will be
re-proposed by anyone who does not find it written down.

The SetStdHandle mechanism reads like a bug without the note that std
resolves GetStdHandle per write by design."
```

---

## Self-Review

**Spec coverage** — every section of `docs/superpowers/specs/2026-08-09-serve-background-design.md` maps to a task:

| Spec section | Task |
|---|---|
| 1. `--log <PATH>` (Windows only) | 1, 2 |
| `create_dir_all` on the parent | 1 (Step 4), tested in 1 and 3 |
| Append, not truncate | 1 (Step 4), tested in 1 and 3 |
| 2. `FreeConsole()` on the serve path | 1 (Step 4) |
| 3. Scheduled Task reduced to one action | 4 |
| 4. `service do` in the formula template | 5 |
| Testing table | 1 (Steps 3, 6), 3, 5 (Step 2) |
| Documentation | 4, 5, 6 |

**Three deliberate departures from the spec**, each from a verified finding:

1. **`service do` is guarded by a top-level `if OS.mac?`, not nested in `on_macos do`.** The spec said to nest it. `brew style` and `brew audit --strict` both reject that outright — verified by running them, and corroborated by 0 of 354 homebrew-core formulae with services doing it.
2. **Console detach is conditional on `--log`, not on `--serve`.** The spec detached on the serve path generally. That would leave stderr pointing at a destroyed console whenever `--log` was absent, and `print_to` panics on write errors other than `ERROR_INVALID_HANDLE`. Fusing the two into one flag makes the hazardous combination unrepresentable — the same move `cce3256` made for the notification policy.
3. **stdout is redirected too, and stdin is nulled.** The spec named only stderr. Leaving those slots pointing at the destroyed console leaves stale handle values that a later `CreateFile` can recycle, and hands the stale stdin to any child spawned with default inheritance.

**Notification policy — checked, no change needed.** `notify::decide` branches on `IsTerminal::is_terminal(&stderr())`, and `AsHandle for io::Stderr` calls `GetStdHandle` fresh, so after the redirect it reports false. That is the same verdict the current `cmd /c … 2> log` produces, so supervisor-hosted behaviour is unchanged. Running `--serve … --log …` by hand from a terminal now also reports false — which is correct: the output is going to a file, not to the human.

**Placeholder scan** — no TBD/TODO; every code step carries the literal text to write.

**Type consistency** — `redirect_to_log(&Path) -> anyhow::Result<()>` is defined in Task 1 and called with that exact signature in Task 2. `open_log(&Path) -> std::io::Result<File>` is private to Task 1's module and used only by its own tests. The clap field is `log: Option<PathBuf>`, read as `args.log.as_deref()` in Task 2.
