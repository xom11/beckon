# `beckon-serve.exe` — Windows app Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship a second Windows binary, `beckon-serve.exe`, that runs the existing resident hotkey service with no console window, a tray context menu (edit config / reload / open log / pause / start-with-Windows / quit), and a first-run path that writes a starter config — so resident mode installs by double-click instead of by Scheduled Task XML.

**Architecture:** `beckon-cli` gains a library target so two binaries can share `serve.rs`. `beckon-serve.exe` is a `windows_subsystem = "windows"` bin in the same package that redirects stderr to a log before anything can print, then calls the same `cmd_serve`. The existing tray icon in `beckon-windows/src/hotkey.rs` grows a tooltip (`set_status`) and a context menu (`set_menu`); `hotkey.rs` owns the menu chrome and knows no policy, while `serve.rs` decides what each item means. Autostart is a `HKCU\…\Run` value. `beckon.exe serve <CONFIG>` is untouched.

**Tech Stack:** Rust 2021, `clap` 4 derive, `windows` crate 0.61 (Win32 shell/menu/registry), `notify` 6, `fs4` 0.8, `embed-resource` (build-dep, Task 8 only).

**Spec:** `docs/superpowers/specs/2026-08-10-windows-serve-app-design.md`

## Global Constraints

- **Rust floor is 1.75** (`workspace.package.rust-version`). No feature newer than that.
- **No new runtime dependencies.** New `windows` crate *features* are allowed; new crates are not, with one exception: `embed-resource` as a **build**-dependency in Task 8.
- **`cargo clippy --all-targets -- -D warnings` must pass** on every platform CI builds. A warning is a build failure.
- **`cargo fmt --all` must be clean.** Run it before every commit.
- **`serve` log messages stay ASCII.** Windows PowerShell 5.1 `Get-Content` defaults to ANSI, so a UTF-8 em-dash renders as `â€"` in the log. Terminal-facing output (`doctor`, `resolve`) keeps its emoji; anything that can reach the log file must not.
- **Commit messages: no `Co-Authored-By` lines.** Conventional-commit prefixes matching the repo (`feat(scope):`, `fix(scope):`, `docs:`, `test:`, `chore:`).
- **CI excludes crates by platform** (`.github/workflows/ci.yml`): the Linux and macOS jobs pass `--exclude beckon-windows`. **Tests inside `beckon-windows` therefore only ever run on the `windows-latest` runner.** Any logic that should be tested on every runner must live in `beckon-cli`, which is built everywhere. This is why Tasks 5 and 6 put pure string/path logic in `beckon-cli` and syscalls in `beckon-windows`.
- **`beckon.exe serve <CONFIG>` behaviour must not change.** Its tests, its flags, its output. Every task that touches `serve.rs` re-runs `cargo test -p beckon-cli` to prove it.

---

## File Structure

**Created:**

| Path | Responsibility |
|---|---|
| `crates/beckon-cli/src/lib.rs` | Today's `main.rs` body. Exposes `cli_main()` and (Windows) `serve_app_main()`. |
| `crates/beckon-cli/src/bin/beckon-serve.rs` | GUI-subsystem entry point. Nothing but the subsystem attribute and a call into the lib. |
| `crates/beckon-cli/src/serve_app.rs` | Portable policy for the app front door: default paths, starter template, Run-key command-line construction, `ensure_config`. Compiles and tests on **every** platform. Windows-only glue (`serve_app_main`) sits behind `cfg`. |
| `crates/beckon-windows/src/autostart.rs` | `HKCU\…\Run` read/write/delete. Syscalls only — no path or quoting policy. |
| `crates/beckon-windows/src/shell.rs` | `open_path` (ShellExecuteW) and `error_dialog` (MessageBoxW). |
| `crates/beckon-cli/build.rs` | Task 8 only: embeds the icon resource. |
| `assets/beckon.ico` | Task 8 only. **Asset does not exist yet.** |

**Modified:**

| Path | Change |
|---|---|
| `crates/beckon-cli/src/main.rs` | Reduced to a ~3-line shim. |
| `crates/beckon-cli/Cargo.toml` | `[lib]`, second `[[bin]]`, build-dep in Task 8. |
| `crates/beckon-windows/src/hotkey.rs` | `set_status`, `set_menu`, `request_quit`; `NIF_MESSAGE` on the tray icon; two new `wndproc` branches. |
| `crates/beckon-windows/src/lib.rs` | Declare the two new modules. |
| `crates/beckon-windows/Cargo.toml` | `Win32_System_Registry` feature. |
| `crates/beckon-cli/src/serve.rs` | `ServeState` gains `paused`, `log`, `last_phrase`; menu wiring behind one `cfg(windows)` block; `reload` respects `paused`. |
| `packaging/scoop/beckon.json.template` | Second binary + Start Menu shortcut. |
| `.github/workflows/release.yml` | Copy `beckon-serve.exe` into the Windows zip. |
| `README.md`, `CLAUDE.md`, `examples/windows/serve/README.md` | Task 9. |

---

## Task 0: Establish a Windows compile-check loop — **DONE 2026-08-10**

**Result: the GNU route works.** `WINCHECK` is **two** commands, and both are
required:

```bash
cargo check  --target x86_64-pc-windows-gnu -p beckon-windows -p beckon-cli --all-targets
cargo clippy --target x86_64-pc-windows-gnu -p beckon-windows -p beckon-cli --all-targets -- -D warnings
```

**The clippy half was added after it was needed.** Task 6 originally ran only
the `check` half, passed locally, passed review — and failed on
`windows-latest` with four `dead_code` errors. `check` reports `dead_code` as
a *warning* and exits 0; CI runs `cargo clippy … -- -D warnings`, where the
same warning is a build failure. A `#[cfg_attr(not(target_os = "windows"),
allow(dead_code))]` therefore looked sufficient locally while leaving the
Windows build broken, because the code was unused on **both** platforms.

Reproduced on the macOS host after the fact: the clippy command above emits
exactly the four errors CI did. One local command would have caught it.

Verified against unmodified `main`: `beckon-core`, `beckon-windows` and
`beckon-cli` all check clean in 7.35 s. `cargo check` does not link, so no
MSVC toolchain is involved.

**It is a fast gate, not the authority.** The shipped target is `-msvc`, and
CI is what proves it. Two things `WINCHECK` structurally cannot do:

- **Run any test.** `cargo test` needs a linker and a Windows host. Tests
  written into `beckon-windows` (Tasks 2 and 3) execute only on the
  `windows-latest` CI runner.
- **Catch `-msvc`-only breakage.** Rare for this API surface, but real.

`ci.yml` triggers on `pull_request` and on pushes to `main` only, so a
**draft PR is open for this branch** — that is what makes every push run the
three-OS matrix and actually execute the Windows tests. Each task's final
step is: push, then read the `windows-latest` job.

The steps below are kept for the record.

### Original steps

The development machine is macOS. Tasks 2–7 write Windows-only code that `cargo build` on macOS never compiles, because `.github/workflows/ci.yml` and local builds both exclude `beckon-windows` off-Windows. Without a check loop, every one of those tasks is written blind.

**Files:** none — this task produces a documented command, not a diff.

**Interfaces:**
- Produces: the exact verification command that later tasks refer to as **`WINCHECK`**.

- [ ] **Step 1: See what targets are installed**

```bash
rustup target list --installed
```

- [ ] **Step 2: Add the GNU Windows target**

`cargo check` does not link, so the MSVC toolchain (unavailable on macOS) is not needed — only the target's `std`.

```bash
rustup target add x86_64-pc-windows-gnu
```

- [ ] **Step 3: Try the check**

```bash
cargo check --target x86_64-pc-windows-gnu \
  -p beckon-windows -p beckon-cli --all-targets
```

Expected: compiles, or fails only with errors in *your* code once you start editing. If it fails **now**, on unmodified `main`, the route is unusable — record the error and go to Step 4.

- [ ] **Step 4: If Step 3 failed, pick a fallback and write it down**

In order of preference:

1. **CI.** Push the branch; `.github/workflows/ci.yml` runs `cargo build --workspace --exclude beckon-linux --exclude beckon-macos --all-targets`, `cargo test`, and `cargo clippy … -D warnings` on `windows-latest`. Slow (minutes per push) but authoritative — it is MSVC, which is what ships.
2. **Build on a14 over SSH.** Note the hazard recorded in memory: SSH to a14 lands in **session 0**. Building is fine there; *running* a tray app is not.

- [ ] **Step 5: Record the decision in the plan file**

Edit this file's Task 0 to state which route was chosen and the exact command, so every later task's "run WINCHECK" step is unambiguous. Commit:

```bash
git add docs/superpowers/plans/2026-08-10-windows-serve-app.md
git commit -m "docs(plan): record the Windows compile-check route"
```

---

## Task 1: `beckon-cli` gains a library target

Pure refactor. No behaviour change, no new feature. Lands independently and is worth landing even if the rest of the plan is abandoned.

**Files:**
- Create: `crates/beckon-cli/src/lib.rs`
- Modify: `crates/beckon-cli/src/main.rs` (replaced entirely), `crates/beckon-cli/Cargo.toml`

**Interfaces:**
- Produces: `beckon_cli::cli_main()` — takes no arguments, returns `()`, exits the process itself on error (it already calls `std::process::exit(1)`).
- Produces: the modules `lockfile`, `notify`, `serve`, `stable_id` become modules of the **library** crate. They stay private; nothing outside the crate needs them.

- [ ] **Step 1: Declare the library target**

Edit `crates/beckon-cli/Cargo.toml`, inserting `[lib]` immediately before the existing `[[bin]]` block:

```toml
[lib]
name = "beckon_cli"
path = "src/lib.rs"

[[bin]]
name = "beckon"
path = "src/main.rs"
```

- [ ] **Step 2: Move the body of `main.rs` into `lib.rs`**

Create `crates/beckon-cli/src/lib.rs` containing **the entire current contents of `crates/beckon-cli/src/main.rs`**, with exactly two edits:

1. Add a crate-level doc comment at the very top:

```rust
//! `beckon`'s command surface, as a library so that both binaries in this
//! package can share it: `beckon` (console subsystem, the CLI) and
//! `beckon-serve` (GUI subsystem, the Windows tray app). Splitting it out
//! is what lets `serve.rs` have exactly one implementation.
```

2. Rename `fn main()` to `pub fn cli_main()`. Its body is unchanged:

```rust
pub fn cli_main() {
    let args = Args::parse_checked();
    beckon_core::set_verbose(args.verbose);
    if let Err(e) = run(&args) {
        // Always to stderr.
        eprintln!("beckon: {e:#}");
        let message = format!("{e:#}");
        // `serve` is the one command a supervisor restarts on a fixed
        // interval forever (launchd KeepAlive, a Task Scheduler repetition),
        // so it is the one command whose failure here can repeat with nobody
        // asking. Every other command failed because a human just ran it.
        //
        // Widen this and the 5-minute Windows watchdog posts a desktop
        // notification every five minutes forever;
        // `notify_policy::repeated_serve_startup_failures_notify_once` is what
        // notices.
        let cause = if matches!(args.command, Some(Command::Serve { .. })) {
            notify::Cause::MachineRepeat
        } else {
            notify::Cause::HumanAction
        };
        notify::report_expected(&message, cause, is_expected(&e));
        std::process::exit(1);
    }
}
```

Everything else — `RESERVED`, `Args`, `Command`, `parse_checked`, `explain_shadowed_verb`, `is_expected`, `run`, `require_id`, `pick_backend`, every `cmd_*`, and the `#[cfg(test)] mod tests` block — moves verbatim.

- [ ] **Step 3: Reduce `main.rs` to a shim**

Replace the entire contents of `crates/beckon-cli/src/main.rs` with:

```rust
fn main() {
    beckon_cli::cli_main();
}
```

- [ ] **Step 4: Build and run the existing tests**

```bash
cargo fmt --all
cargo build --workspace --exclude beckon-linux --exclude beckon-windows --all-targets
cargo test --workspace --exclude beckon-linux --exclude beckon-windows
cargo clippy --workspace --exclude beckon-linux --exclude beckon-windows --all-targets -- -D warnings
```

Expected: all pass. The three tests in the moved `mod tests` (`already_running_is_expected`, `lock_open_failure_is_not_expected`, `ordinary_errors_are_not_expected`) still run — they are now library tests.

- [ ] **Step 5: Smoke-test the binary by hand**

The refactor is only correct if the CLI is byte-identical in behaviour. Run each and compare against expectation:

```bash
./target/debug/beckon --version          # prints "beckon 0.6.0"
./target/debug/beckon --help             # usage line reads: beckon [OPTIONS] <ID>
./target/debug/beckon list               # lists running apps, exit 0
./target/debug/beckon resolve            # exits 2 with the shadowed-verb hint
echo $?                                  # 2
./target/debug/beckon                    # exits 2, help text (arg_required_else_help)
```

The `beckon resolve` case is the one that proves `explain_shadowed_verb` survived the move — it must print "``resolve`` is a subcommand name, not an app id."

- [ ] **Step 6: Commit**

```bash
git add crates/beckon-cli/Cargo.toml crates/beckon-cli/src/lib.rs crates/beckon-cli/src/main.rs
git commit -m "refactor(cli): give beckon-cli a library target

A second binary in this package needs serve.rs, notify.rs and
pick_backend, all of which were private to the binary crate. Moving the
body into lib.rs is the only way to have one implementation of serve
rather than two. main.rs becomes a shim; no behaviour changes, and the
parse_checked tests become library tests."
```

---

## Task 2: Tray tooltip carries the registration state

The tray icon is currently a one-directional signal: present means alive, absent means dead **or** not-yet-ready. `registration_phrase` already produces the sentence that resolves it; this task puts that sentence in the tooltip.

**Files:**
- Modify: `crates/beckon-windows/src/hotkey.rs`

**Interfaces:**
- Produces: `beckon_windows::hotkey::set_status(text: &str)` — updates the tray tooltip via `NIM_MODIFY`. Safe to call before `install()` (no-op) and after a failed tray add (best-effort, logs once).
- Produces: `fn fill_tip(dst: &mut [u16; 128], text: &str)` — private, unit-tested.

> `NOTIFYICONDATAW::szTip` is `[u16; 128]` in the modern struct, which is what the array length above assumes. If `WINCHECK` reports a different length, change `fill_tip` to take `&mut [u16]` and adjust the tests to read `dst.len()` rather than hard-coding 127/128 — do not silently edit the constant, because the "clear the tail" behaviour is the part that matters.

- [ ] **Step 1: Write the failing test**

Append to the bottom of `crates/beckon-windows/src/hotkey.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fill_tip_writes_text_and_terminates() {
        let mut buf = [0xFFFFu16; 128];
        fill_tip(&mut buf, "beckon - 5 shortcuts");
        let text: String = String::from_utf16(&buf[..20]).unwrap();
        assert_eq!(text, "beckon - 5 shortcuts");
        assert_eq!(buf[20], 0, "must be NUL-terminated right after the text");
    }

    #[test]
    fn fill_tip_truncates_and_still_terminates() {
        let mut buf = [0xFFFFu16; 128];
        let long = "x".repeat(500);
        fill_tip(&mut buf, &long);
        assert_eq!(buf[127], 0, "the last slot must always be the NUL");
        assert!(buf[..127].iter().all(|&c| c == b'x' as u16));
    }

    #[test]
    fn fill_tip_clears_the_tail_of_a_reused_buffer() {
        let mut buf = [0u16; 128];
        fill_tip(&mut buf, "a long previous tooltip");
        fill_tip(&mut buf, "hi");
        assert_eq!(String::from_utf16(&buf[..2]).unwrap(), "hi");
        assert!(
            buf[2..].iter().all(|&c| c == 0),
            "stale text from the previous call must not survive"
        );
    }
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run `WINCHECK` (Task 0). Expected: `cannot find function 'fill_tip' in this scope`.

If the CI fallback is in use, push and read the `windows-latest` job's compile error.

- [ ] **Step 3: Extract `fill_tip` and add `set_status`**

Add `NIM_MODIFY` to the `windows::Win32::UI::Shell` import list at the top of the file. Add `use std::cell::RefCell;` if not already present (it is).

Add to the `thread_local!` block, with this comment:

```rust
    // The tooltip text currently displayed. Kept so a TaskbarCreated re-add
    // (Explorer restart, logon race) restores the live status instead of
    // reverting to the startup placeholder — the icon coming back with a
    // stale tooltip would be a worse signal than no icon at all.
    static TRAY_TIP: RefCell<String> = RefCell::new(String::from("beckon serve"));
```

Add the helper above `tray_add`:

```rust
/// Copy `text` into a fixed-size UTF-16 tip buffer, always NUL-terminated
/// and always clearing whatever the buffer held before.
///
/// `szTip` is a fixed array, not a pointer: a shorter second call would
/// otherwise leave the tail of the first call's text in place and the tray
/// would show a concatenation of both.
fn fill_tip(dst: &mut [u16; 128], text: &str) {
    let utf16: Vec<u16> = text.encode_utf16().collect();
    let max = dst.len() - 1; // leave room for the NUL terminator
    let n = utf16.len().min(max);
    dst[..n].copy_from_slice(&utf16[..n]);
    for slot in dst[n..].iter_mut() {
        *slot = 0;
    }
}
```

Replace the tip-copying block inside `tray_add` with:

```rust
    TRAY_TIP.with(|t| fill_tip(&mut nid.szTip, &t.borrow()));
```

Add the public function next to `add_tick`:

```rust
/// Update the tray tooltip. Best effort: a tooltip that will not update must
/// not take the hotkeys down, but it must not be silent either — the whole
/// point of the tooltip is that it is the honest answer to "is this alive and
/// how many keys does it hold".
pub fn set_status(text: &str) {
    TRAY_TIP.with(|t| *t.borrow_mut() = text.to_string());
    let hwnd = TRAY_HWND.with(|c| c.get());
    if hwnd.0.is_null() {
        return; // install() has not run yet; tray_add will pick the text up
    }
    let mut nid = NOTIFYICONDATAW {
        cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: hwnd,
        uID: 1,
        uFlags: NIF_TIP,
        ..Default::default()
    };
    fill_tip(&mut nid.szTip, text);
    if !unsafe { Shell_NotifyIconW(NIM_MODIFY, &nid) }.as_bool() {
        eprintln!("hotkey: Shell_NotifyIconW(NIM_MODIFY) failed - tooltip is stale");
    }
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run `WINCHECK`, then the Windows test job (or push for CI). Expected: three new tests pass.

- [ ] **Step 5: Call it from `serve.rs`**

In `crates/beckon-cli/src/serve.rs`, both places that build a `registration_phrase` now also publish it. In `cmd_serve`, replace the final `eprintln!` block before `run_forever()` with:

```rust
    let phrase = registration_phrase(outcome.ok, state.borrow().shortcuts.len());
    eprintln!("beckon serve: {} from {}", phrase, config.display());
    set_tray_status(&phrase);
    if let Some(toast) = failure_toast(&outcome.failed) {
        crate::notify::report(&toast, crate::notify::Cause::MachineRepeat);
    }
    HotkeyManager::run_forever();
```

And in `reload`'s `Ok(new)` arm, after the existing `eprintln!`:

```rust
            let phrase = registration_phrase(outcome.ok, state.borrow().shortcuts.len());
            eprintln!("beckon serve: reloaded - {phrase}");
            set_tray_status(&phrase);
```

`serve.rs` is shared with macOS, which has no tray, so add this shim near the top of the file (after the `use` block):

```rust
/// Publish a one-line status where the user can see it without reading the
/// log. Windows has the tray tooltip; macOS has nowhere to put it, and the
/// LaunchAgent's stderr already goes to a file launchd owns.
#[cfg(target_os = "windows")]
fn set_tray_status(text: &str) {
    hotkey::set_status(text);
}
#[cfg(not(target_os = "windows"))]
fn set_tray_status(_text: &str) {}
```

- [ ] **Step 6: Verify the shared path still builds on macOS**

```bash
cargo fmt --all
cargo build --workspace --exclude beckon-linux --exclude beckon-windows --all-targets
cargo test --workspace --exclude beckon-linux --exclude beckon-windows
cargo clippy --workspace --exclude beckon-linux --exclude beckon-windows --all-targets -- -D warnings
```

Then `WINCHECK`.

- [ ] **Step 7: Commit**

```bash
git add crates/beckon-windows/src/hotkey.rs crates/beckon-cli/src/serve.rs
git commit -m "feat(windows): put the registration count in the tray tooltip

The icon was a one-directional signal -- present means alive, absent
means dead OR not ready yet -- which is why the README had to spend a
paragraph telling users not to trust it. registration_phrase already
produced the sentence that answers the question; NIM_MODIFY puts it
where hovering finds it. The text is kept in a thread_local so a
TaskbarCreated re-add restores the live status rather than the startup
placeholder."
```

---

## Task 3: Tray context menu in `hotkey.rs`

`hotkey.rs` gets the menu *chrome* and no policy: it is handed a list of entries to draw and reports back which id was clicked.

**Files:**
- Modify: `crates/beckon-windows/src/hotkey.rs`

**Interfaces:**
- Produces: `pub struct MenuEntry { pub id: u32, pub label: String, pub checked: Option<bool>, pub enabled: bool }` and `MenuEntry::separator()`.
- Produces: `pub fn set_menu(build: Box<dyn Fn() -> Vec<MenuEntry>>, on_click: Box<dyn FnMut(u32)>)`. `build` is called each time the menu opens, so check states are always live.
- Produces: `pub const MENU_ID_DOUBLE_CLICK: u32 = u32::MAX;` — delivered to `on_click` when the icon is double-clicked. Callers must not use it as a real entry id.
- Produces: `pub fn request_quit()` — posts `WM_QUIT`, so `serve.rs` never imports the `windows` crate.

- [ ] **Step 1: Write the failing test**

Add to the `mod tests` block created in Task 2:

```rust
    #[test]
    fn separator_is_recognisable_by_its_empty_label() {
        let sep = MenuEntry::separator();
        assert!(sep.label.is_empty());
        assert_eq!(sep.checked, None);
    }

> **Revised during execution (2026-08-10).** This step originally also
> specified `double_click_id_cannot_collide_with_a_real_entry`, asserting
> `MENU_ID_DOUBLE_CLICK == u32::MAX` and `> 1000`. Both sides are
> compile-time constants, so it asserted something the compiler already
> guarantees — it proved nothing, and clippy's `assertions_on_constants`
> said so, which would have needed an `#[allow]` to silence. The invariant
> actually worth protecting is *"no real menu entry ever collides with the
> reserved id"*, and real menu entries are built in `serve.rs`, not here —
> so it moved to Task 4, where `build_entries` exists to test it against.
> Ruling: human, during execution. Do not reinstate it here.
```

- [ ] **Step 2: Run the test to verify it fails**

Run `WINCHECK`. Expected: `cannot find type 'MenuEntry' in this scope`.

- [ ] **Step 3: Add the imports**

Extend the existing `windows::Win32::UI::WindowsAndMessaging` import list with:

```rust
    AppendMenuW, CreatePopupMenu, DestroyMenu, GetCursorPos, PostMessageW,
    SetForegroundWindow, TrackPopupMenu, MF_CHECKED, MF_GRAYED, MF_SEPARATOR,
    MF_STRING, TPM_RIGHTBUTTON, WM_APP, WM_COMMAND, WM_CONTEXTMENU,
    WM_LBUTTONDBLCLK, WM_NULL, WM_RBUTTONUP,
```

and add `use windows::Win32::Foundation::POINT;` plus `use windows::core::PCWSTR;` to the top-of-file imports.

> **Verify signatures against the crate, do not trust this listing.** `windows` 0.61 wraps several of these parameters in `Option<…>` (`PostMessageW`'s `hwnd`, `TrackPopupMenu`'s `prcrect`) and returns `windows::core::Result<()>` where older versions returned `BOOL`. `WINCHECK` is the arbiter; adjust the call shapes below to whatever it demands rather than fighting it.

- [ ] **Step 4: Add the types and thread-local slots**

```rust
/// One row of the tray context menu. `hotkey.rs` draws it and reports the
/// click; what any row *means* is entirely the caller's business, which is
/// why there is no enum of actions here.
pub struct MenuEntry {
    pub id: u32,
    pub label: String,
    /// `None` for a plain item, `Some(bool)` for a check box.
    pub checked: Option<bool>,
    pub enabled: bool,
}

impl MenuEntry {
    /// A horizontal rule. Recognised by its empty label.
    pub fn separator() -> Self {
        Self { id: 0, label: String::new(), checked: None, enabled: false }
    }
}

/// Delivered to `on_click` when the tray icon is double-clicked. Callers
/// must number their real entries below this.
pub const MENU_ID_DOUBLE_CLICK: u32 = u32::MAX;

/// Our tray icon's callback message. WM_APP+1 rather than WM_USER+n: WM_USER
/// is only private to a window *class*, and this window's class is shared
/// with nothing, but WM_APP is private to the application, which is the
/// guarantee actually wanted here.
const WM_TRAY: u32 = WM_APP + 1;

type MenuBuilder = Box<dyn Fn() -> Vec<MenuEntry>>;
type MenuHandler = Box<dyn FnMut(u32)>;
```

Add to the `thread_local!` block:

```rust
    static MENU_BUILD: RefCell<Option<MenuBuilder>> = const { RefCell::new(None) };
    static MENU_CB: RefCell<Option<MenuHandler>> = const { RefCell::new(None) };
    // Same role as HOTKEY_PENDING: TrackPopupMenu runs its own modal message
    // pump, so a second click can reach dispatch_menu while the first
    // callback is still on the stack. Queue rather than drop.
    static MENU_PENDING: RefCell<VecDeque<u32>> = const { RefCell::new(VecDeque::new()) };
```

- [ ] **Step 5: Add the dispatcher, the builder and the public API**

```rust
fn dispatch_menu(id: u32) {
    // Take-then-run, exactly as dispatch_hotkey and dispatch_tick do: a
    // menu action may itself pump (ShellExecuteW does), and re-entering a
    // live RefCell borrow would panic across the extern "system" boundary,
    // which aborts the process rather than the callback.
    let Some(mut cb) = MENU_CB.with(|slot| slot.borrow_mut().take()) else {
        MENU_PENDING.with(|p| p.borrow_mut().push_back(id));
        return;
    };
    cb(id);
    MENU_CB.with(|slot| *slot.borrow_mut() = Some(cb));
    while let Some(next) = MENU_PENDING.with(|p| p.borrow_mut().pop_front()) {
        dispatch_menu(next);
    }
}

fn show_menu(hwnd: HWND) {
    let Some(entries) = MENU_BUILD.with(|b| b.borrow().as_ref().map(|f| f())) else {
        return;
    };
    unsafe {
        let Ok(menu) = CreatePopupMenu() else {
            eprintln!("hotkey: CreatePopupMenu failed - no tray menu this time");
            return;
        };
        for e in &entries {
            if e.label.is_empty() {
                let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
                continue;
            }
            let mut flags = MF_STRING;
            if !e.enabled {
                flags |= MF_GRAYED;
            }
            if e.checked == Some(true) {
                flags |= MF_CHECKED;
            }
            let label: Vec<u16> = e.label.encode_utf16().chain(std::iter::once(0)).collect();
            let _ = AppendMenuW(menu, flags, e.id as usize, PCWSTR(label.as_ptr()));
        }
        let mut pt = POINT::default();
        let _ = GetCursorPos(&mut pt);
        // Both of these are required, and neither is folklore: without the
        // SetForegroundWindow the menu never dismisses when the user clicks
        // away, and without the trailing PostMessage the *next* menu fails
        // to appear. Documented in Microsoft KB135788.
        let _ = SetForegroundWindow(hwnd);
        let _ = TrackPopupMenu(menu, TPM_RIGHTBUTTON, pt.x, pt.y, 0, hwnd, None);
        let _ = PostMessageW(Some(hwnd), WM_NULL, WPARAM(0), LPARAM(0));
        let _ = DestroyMenu(menu);
    }
}

/// Install the tray context menu.
///
/// `build` runs every time the menu opens rather than once at install, so
/// check marks reflect state at the moment of the click instead of at
/// startup. `on_click` receives the `MenuEntry::id` that was chosen, or
/// `MENU_ID_DOUBLE_CLICK` for a double-click on the icon itself.
///
/// Call after `HotkeyManager::install`, which creates the window this needs.
pub fn set_menu(build: MenuBuilder, on_click: MenuHandler) {
    MENU_BUILD.with(|b| *b.borrow_mut() = Some(build));
    MENU_CB.with(|c| *c.borrow_mut() = Some(on_click));
}

/// Ask the message loop to exit. `run_forever`'s WM_QUIT arm already
/// unregisters every hotkey and removes the tray icon, so this is the whole
/// of an orderly shutdown.
pub fn request_quit() {
    unsafe { PostQuitMessage(0) };
}
```

Add `PostQuitMessage` to the `WindowsAndMessaging` import list.

- [ ] **Step 6: Turn the tray icon into a message source**

In `tray_add`, change the `uFlags` line and add the callback message, replacing the existing "No NIF_MESSAGE" comment block entirely:

```rust
        // NIF_MESSAGE is what turns the icon from a lamp into a control:
        // Shell_NotifyIcon posts WM_TRAY to this hwnd for every mouse event
        // on the icon, and wndproc turns the right-click into the menu.
        uFlags: NIF_ICON | NIF_TIP | NIF_MESSAGE,
        uCallbackMessage: WM_TRAY,
```

Add `NIF_MESSAGE` to the `Shell` import list.

- [ ] **Step 7: Route the messages in `wndproc`**

Insert into `wndproc`, immediately after the `WM_TIMER` branch:

```rust
    if msg == WM_TRAY {
        // lParam carries the mouse event; wParam is the icon's uID.
        match l.0 as u32 {
            WM_RBUTTONUP | WM_CONTEXTMENU => show_menu(hwnd),
            WM_LBUTTONDBLCLK => dispatch_menu(MENU_ID_DOUBLE_CLICK),
            _ => {}
        }
        return LRESULT(0);
    }
    if msg == WM_COMMAND {
        // TrackPopupMenu without TPM_RETURNCMD posts the chosen id here.
        // The high word is the notification code and is 0 for a menu.
        dispatch_menu((w.0 & 0xFFFF) as u32);
        return LRESULT(0);
    }
```

`run_forever` needs no change: it only short-circuits `WM_HOTKEY` and `WM_TIMER`, and everything else already falls through to `DispatchMessageW` and therefore to `wndproc`.

- [ ] **Step 8: Run the tests to verify they pass**

Run `WINCHECK`, then the Windows test job. Expected: the two new tests pass and the crate compiles.

- [ ] **Step 9: Commit**

```bash
git add crates/beckon-windows/src/hotkey.rs
git commit -m "feat(windows): give the tray icon a context menu

hotkey.rs owns the chrome and no policy: set_menu takes a closure that
builds rows and a closure that receives the clicked id, so config paths
and registry keys stay in serve.rs where they belong.

TrackPopupMenu runs its own modal pump, which is exactly the reentrancy
the module doc was already written for -- dispatch_menu therefore copies
dispatch_tick's take-then-run discipline and queues a click that arrives
mid-callback instead of double-borrowing across extern \"system\".

The SetForegroundWindow before TrackPopupMenu and the PostMessage after
it are KB135788 requirements, not cargo cult: without the first the menu
never dismisses on click-away, without the second the next one never
opens."
```

---

## Task 4: Wire the menu to `serve` — edit, reload, open log, pause, quit

Autostart is deliberately **not** in this task; it is Task 5, so this one can be reviewed and tested on its own.

**Files:**
- Modify: `crates/beckon-cli/src/serve.rs`
- Create: `crates/beckon-windows/src/shell.rs`
- Modify: `crates/beckon-windows/src/lib.rs`

**Interfaces:**
- Consumes: `hotkey::set_menu`, `hotkey::MenuEntry`, `hotkey::MENU_ID_DOUBLE_CLICK`, `hotkey::request_quit`, `hotkey::set_status` (Tasks 2–3).
- Produces: `beckon_windows::shell::open_path(path: &Path) -> Result<(), String>`.
- Produces: `serve::cmd_serve_with_log(config: &Path, log: Option<PathBuf>) -> Result<()>` — `cmd_serve` becomes a thin wrapper passing `None`. Task 7 calls the `_with_log` form so the menu's "Open log" knows where the log is.
- Produces: `MenuModel` and `build_entries(&MenuModel) -> Vec<MenuEntry>` — pure, unit-tested.

- [ ] **Step 1: Create the shell helper**

Create `crates/beckon-windows/src/shell.rs`:

```rust
//! Two one-shot Win32 conveniences the tray app needs and the CLI does not:
//! opening a file with its registered handler, and telling the user
//! something when there is no stderr to tell them through.

use std::path::Path;
use windows::core::{HSTRING, PCWSTR};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONERROR, MB_OK, SW_SHOWNORMAL};

/// Open `path` with whatever the user has registered for it — the editor for
/// a `.toml`, the log viewer for a `.log`.
///
/// **This pumps the calling thread's message queue.** ShellExecuteW performs
/// an out-of-process shell activation, and the caller must therefore hold no
/// `RefCell` borrow across it; see `beckon-cli/src/serve.rs`'s module doc for
/// why that is a process-abort rather than a panic.
pub fn open_path(path: &Path) -> Result<(), String> {
    let wide = HSTRING::from(path.as_os_str());
    // ShellExecuteW returns a fake HINSTANCE; <= 32 means failure.
    let rc = unsafe {
        ShellExecuteW(
            None,
            windows::core::w!("open"),
            PCWSTR(wide.as_ptr()),
            PCWSTR::null(),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        )
    };
    if rc.0 as usize <= 32 {
        return Err(format!("ShellExecuteW failed for `{}`", path.display()));
    }
    Ok(())
}

/// A modal error box.
///
/// The GUI-subsystem binary has no stderr before its log is open and no
/// console ever, so for the handful of failures that happen before or
/// instead of logging, this is the only channel that reaches a person.
pub fn error_dialog(title: &str, body: &str) {
    let title = HSTRING::from(title);
    let body = HSTRING::from(body);
    unsafe {
        MessageBoxW(
            None,
            PCWSTR(body.as_ptr()),
            PCWSTR(title.as_ptr()),
            MB_OK | MB_ICONERROR,
        )
    };
}
```

Declare it in `crates/beckon-windows/src/lib.rs`, after the `logfile` module:

```rust
#[cfg(target_os = "windows")]
pub mod shell;
```

- [ ] **Step 2: Write the failing test for the pure menu model**

Append to `crates/beckon-cli/src/serve.rs`'s existing `#[cfg(test)] mod tests`:

```rust
    #[cfg(target_os = "windows")]
    #[test]
    fn menu_shows_the_phrase_and_reflects_pause() {
        let m = MenuModel {
            phrase: "5 shortcuts registered".into(),
            paused: false,
            autostart: false,
            has_log: true,
        };
        let rows = build_entries(&m);
        assert_eq!(rows[0].label, "beckon - 5 shortcuts registered");
        assert!(!rows[0].enabled, "the status row is a label, not a button");
        let pause = rows.iter().find(|r| r.id == MENU_PAUSE).unwrap();
        assert_eq!(pause.checked, Some(false));

        // `..m.clone()`, not `..m`: Task 5 appends another case to this test
        // and needs `m` to still be alive.
        let paused = MenuModel { paused: true, ..m.clone() };
        let rows = build_entries(&paused);
        assert_eq!(
            rows.iter().find(|r| r.id == MENU_PAUSE).unwrap().checked,
            Some(true)
        );
    }

    /// Moved here from Task 3, where the same intent could only be written
    /// as an assertion about a constant. Here it runs against the real
    /// entry list, so adding a menu row that collides with the reserved
    /// double-click id fails the build instead of silently making
    /// double-click fire that row.
    #[cfg(target_os = "windows")]
    #[test]
    fn no_real_entry_collides_with_the_reserved_double_click_id() {
        let m = MenuModel {
            phrase: "5 shortcuts registered".into(),
            paused: false,
            autostart: false,
            has_log: true,
        };
        for row in build_entries(&m) {
            assert_ne!(
                row.id,
                hotkey::MENU_ID_DOUBLE_CLICK,
                "entry {:?} shadows the reserved double-click id",
                row.label
            );
        }
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn open_log_is_disabled_when_there_is_no_log() {
        let m = MenuModel {
            phrase: "0 shortcuts registered".into(),
            paused: false,
            autostart: false,
            has_log: false,
        };
        let rows = build_entries(&m);
        assert!(!rows.iter().find(|r| r.id == MENU_LOG).unwrap().enabled);
    }
```

`MenuModel` needs `#[derive(Clone)]` for the `..m` struct-update above.

- [ ] **Step 3: Run the test to verify it fails**

Run `WINCHECK`. Expected: `cannot find struct 'MenuModel'`.

- [ ] **Step 4: Add `paused`, `log` and `last_phrase` to `ServeState`**

```rust
struct ServeState {
    shortcuts: Vec<Shortcut>,
    config: PathBuf,
    /// Hotkeys deliberately unregistered from the tray menu. A reload while
    /// paused updates the table but must not re-register — a file save is
    /// not a request to un-pause.
    paused: bool,
    /// Where stderr went, when it went to a file. `None` on the CLI path,
    /// which leaves the menu's "Open log" greyed out rather than lying.
    log: Option<PathBuf>,
    /// The most recent `registration_phrase`, so the menu can show it
    /// without re-running a registration pass.
    last_phrase: String,
}
```

Update the single construction site in `cmd_serve` accordingly (`paused: false`, `log`, `last_phrase: String::new()`).

- [ ] **Step 5: Split `cmd_serve` and add the menu block**

Rename the existing `pub fn cmd_serve(config: &Path)` to:

```rust
pub fn cmd_serve(config: &Path) -> Result<()> {
    cmd_serve_with_log(config, None)
}

/// `log` is only ever `Some` on the Windows app path, where stderr was
/// redirected to a file this process chose. It exists so the tray menu's
/// "Open log" can point at the right file instead of guessing.
pub fn cmd_serve_with_log(config: &Path, log: Option<PathBuf>) -> Result<()> {
```

Inside, after `add_tick` is registered and before the final `eprintln!`, insert:

```rust
    #[cfg(target_os = "windows")]
    install_tray_menu(&state, &mgr);
```

- [ ] **Step 6: Add the Windows-only menu block**

Append to `crates/beckon-cli/src/serve.rs`:

```rust
// ---------------------------------------------------------------------------
// Tray menu (Windows only). macOS `serve` runs under launchd with no tray.
// ---------------------------------------------------------------------------

#[cfg(target_os = "windows")]
const MENU_STATUS: u32 = 1;
#[cfg(target_os = "windows")]
const MENU_EDIT: u32 = 2;
#[cfg(target_os = "windows")]
const MENU_RELOAD: u32 = 3;
#[cfg(target_os = "windows")]
const MENU_LOG: u32 = 4;
#[cfg(target_os = "windows")]
const MENU_PAUSE: u32 = 5;
#[cfg(target_os = "windows")]
const MENU_QUIT: u32 = 7;

/// Everything the menu needs to draw itself, snapshotted out of `ServeState`
/// so the drawing is a pure function and can be tested without a tray, a
/// message loop or a registry.
#[cfg(target_os = "windows")]
#[derive(Clone)]
struct MenuModel {
    phrase: String,
    paused: bool,
    autostart: bool,
    has_log: bool,
}

#[cfg(target_os = "windows")]
fn build_entries(m: &MenuModel) -> Vec<hotkey::MenuEntry> {
    use hotkey::MenuEntry;
    let head = if m.paused {
        format!("beckon - paused ({})", m.phrase)
    } else {
        format!("beckon - {}", m.phrase)
    };
    vec![
        MenuEntry { id: MENU_STATUS, label: head, checked: None, enabled: false },
        MenuEntry::separator(),
        MenuEntry { id: MENU_EDIT, label: "Edit shortcuts...".into(), checked: None, enabled: true },
        MenuEntry { id: MENU_RELOAD, label: "Reload now".into(), checked: None, enabled: true },
        MenuEntry { id: MENU_LOG, label: "Open log".into(), checked: None, enabled: m.has_log },
        MenuEntry::separator(),
        MenuEntry { id: MENU_PAUSE, label: "Pause hotkeys".into(), checked: Some(m.paused), enabled: true },
        MenuEntry::separator(),
        MenuEntry { id: MENU_QUIT, label: "Quit".into(), checked: None, enabled: true },
    ]
}

#[cfg(target_os = "windows")]
fn install_tray_menu(state: &Rc<RefCell<ServeState>>, mgr: &Rc<RefCell<HotkeyManager>>) {
    let st_build = Rc::clone(state);
    let build = Box::new(move || {
        let s = st_build.borrow();
        build_entries(&MenuModel {
            phrase: s.last_phrase.clone(),
            paused: s.paused,
            autostart: false, // Task 5 fills this in
            has_log: s.log.is_some(),
        })
    });

    let st = Rc::clone(state);
    let mg = Rc::clone(mgr);
    let on_click = Box::new(move |id: u32| {
        match id {
            // ShellExecuteW pumps this thread's queue, so the path is cloned
            // out and every borrow is dropped BEFORE the call -- the same
            // rule the module doc states for backend.beckon().
            MENU_EDIT | hotkey::MENU_ID_DOUBLE_CLICK => {
                let path = st.borrow().config.clone();
                if let Err(e) = beckon_windows::shell::open_path(&path) {
                    eprintln!("beckon serve: {e}");
                }
            }
            MENU_LOG => {
                let path = st.borrow().log.clone();
                if let Some(path) = path {
                    if let Err(e) = beckon_windows::shell::open_path(&path) {
                        eprintln!("beckon serve: {e}");
                    }
                }
            }
            MENU_RELOAD => reload(&st, &mg),
            MENU_PAUSE => {
                let now = !st.borrow().paused;
                set_paused(&st, &mg, now);
            }
            MENU_QUIT => {
                eprintln!("beckon serve: quit requested from the tray menu");
                hotkey::request_quit();
            }
            _ => {}
        }
    });

    hotkey::set_menu(build, on_click);
}

/// Unregister or re-register every hotkey, and say so in the tooltip.
///
/// Neither `unregister_all` nor `register_all` pumps the message queue, so
/// holding both borrows across them is sound here for the same reason
/// `reload` may hold them.
#[cfg(target_os = "windows")]
fn set_paused(state: &Rc<RefCell<ServeState>>, mgr: &Rc<RefCell<HotkeyManager>>, paused: bool) {
    let mut m = mgr.borrow_mut();
    if paused {
        m.unregister_all();
        state.borrow_mut().paused = true;
        let phrase = state.borrow().last_phrase.clone();
        eprintln!("beckon serve: paused - {phrase}");
        hotkey::set_status(&format!("beckon - paused ({phrase})"));
    } else {
        state.borrow_mut().paused = false;
        let outcome = register_all(&mut m, &state.borrow().shortcuts);
        let phrase = registration_phrase(outcome.ok, state.borrow().shortcuts.len());
        state.borrow_mut().last_phrase = phrase.clone();
        eprintln!("beckon serve: resumed - {phrase}");
        hotkey::set_status(&phrase);
    }
}
```

- [ ] **Step 7: Make `reload` respect `paused` and record `last_phrase`**

Replace `reload`'s `Ok(new)` arm with:

```rust
        Ok(new) => {
            let mut m = mgr.borrow_mut();
            m.unregister_all();
            state.borrow_mut().shortcuts = new;
            let paused = state.borrow().paused;
            if paused {
                // A file save is not a request to un-pause. The table is
                // updated so resuming picks up the edit; nothing registers.
                let phrase = format!("{} shortcuts", state.borrow().shortcuts.len());
                state.borrow_mut().last_phrase = phrase.clone();
                eprintln!("beckon serve: reloaded while paused - {phrase}");
                set_tray_status(&format!("beckon - paused ({phrase})"));
                return;
            }
            let outcome = register_all(&mut m, &state.borrow().shortcuts);
            let phrase = registration_phrase(outcome.ok, state.borrow().shortcuts.len());
            state.borrow_mut().last_phrase = phrase.clone();
            eprintln!("beckon serve: reloaded - {phrase}");
            set_tray_status(&phrase);
            if let Some(toast) = failure_toast(&outcome.failed) {
                crate::notify::report(&toast, crate::notify::Cause::MachineRepeat);
            }
        }
```

`state.borrow().paused` is `false` on every macOS build (nothing ever sets it), so this branch is dead there and behaviour is unchanged — which is what `cargo test` on macOS must confirm.

Also set `last_phrase` at startup in `cmd_serve_with_log`, right where `phrase` is first computed.

- [ ] **Step 8: Run the tests**

```bash
cargo fmt --all
cargo test --workspace --exclude beckon-linux --exclude beckon-windows
cargo clippy --workspace --exclude beckon-linux --exclude beckon-windows --all-targets -- -D warnings
```

Expected: the existing `serve` tests still pass unchanged on macOS. Then `WINCHECK`, then the Windows CI job for the two new `build_entries` tests.

- [ ] **Step 9: Commit**

```bash
git add crates/beckon-windows/src/shell.rs crates/beckon-windows/src/lib.rs crates/beckon-cli/src/serve.rs
git commit -m "feat(cli): tray menu drives serve -- edit, reload, log, pause, quit

build_entries is a pure function over a snapshot of ServeState, so the
menu's contents are unit-testable without a tray or a message loop.

Two correctness notes worth keeping: ShellExecuteW pumps this thread's
queue, so the Edit and Open-log arms clone the path out and drop every
borrow before calling -- the same rule the module doc already states for
backend.beckon(). And a reload while paused updates the shortcut table
without registering, because saving a file is not a request to un-pause."
```

---

## Task 5: Start with Windows — the Run key

**Files:**
- Create: `crates/beckon-windows/src/autostart.rs`
- Modify: `crates/beckon-windows/src/lib.rs`, `crates/beckon-windows/Cargo.toml`
- Create: `crates/beckon-cli/src/serve_app.rs`
- Modify: `crates/beckon-cli/src/lib.rs`, `crates/beckon-cli/src/serve.rs`
- Modify: `docs/superpowers/specs/2026-08-10-windows-serve-app-design.md`

**Interfaces:**
- Produces: `beckon_cli::serve_app::run_key_command_line(exe: &Path, config: Option<&Path>, log: Option<&Path>) -> String` — pure, tested on every runner.
- Produces: `beckon_cli::serve_app::scoop_current_path(exe: &Path) -> PathBuf` — pure, tested on every runner.
- Produces: `beckon_windows::autostart::{is_enabled() -> bool, enable(command: &str) -> Result<(), String>, disable() -> Result<(), String>}`.

- [ ] **Step 1: Correct the spec's testing claim**

The spec says the new pure logic is *"Unit, on every platform including CI Linux"*. That is only true if it lives in `beckon-cli`: `.github/workflows/ci.yml` passes `--exclude beckon-windows` on the Linux and macOS jobs, so tests inside `beckon-windows` run on `windows-latest` alone. Edit the **Testing** section of `docs/superpowers/specs/2026-08-10-windows-serve-app-design.md` to say so, and to state that this is why the pure helpers live in `beckon-cli`.

```bash
git add docs/superpowers/specs/2026-08-10-windows-serve-app-design.md
git commit -m "docs(spec): pure helpers live in beckon-cli because CI excludes beckon-windows"
```

- [ ] **Step 2: Write the failing tests**

Create `crates/beckon-cli/src/serve_app.rs` with only this test module for now:

```rust
//! The Windows app front door: where its config and log live by default,
//! what a fresh config looks like, and what goes in the autostart value.
//!
//! Deliberately in `beckon-cli` and free of `cfg(windows)` for everything
//! but the glue at the bottom. CI excludes `beckon-windows` from the Linux
//! and macOS jobs, so logic placed there is only ever tested on one runner;
//! placed here, these tests run on all three.

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn command_line_is_just_the_quoted_exe_when_everything_is_default() {
        let exe = PathBuf::from(r"C:\Program Files\beckon\beckon-serve.exe");
        assert_eq!(
            run_key_command_line(&exe, None, None),
            r#""C:\Program Files\beckon\beckon-serve.exe""#
        );
    }

    #[test]
    fn command_line_carries_a_non_default_config_and_log() {
        let exe = PathBuf::from(r"C:\bin\beckon-serve.exe");
        let cfg = PathBuf::from(r"D:\my keys.toml");
        let log = PathBuf::from(r"D:\logs\b.log");
        assert_eq!(
            run_key_command_line(&exe, Some(&cfg), Some(&log)),
            r#""C:\bin\beckon-serve.exe" "D:\my keys.toml" --log "D:\logs\b.log""#
        );
    }

    #[test]
    fn scoop_versioned_path_is_rewritten_to_current() {
        let p = PathBuf::from(r"C:\Users\me\scoop\apps\beckon\0.7.0\beckon-serve.exe");
        assert_eq!(
            scoop_current_path(&p),
            PathBuf::from(r"C:\Users\me\scoop\apps\beckon\current\beckon-serve.exe")
        );
    }

    #[test]
    fn scoop_current_path_is_left_alone() {
        let p = PathBuf::from(r"C:\Users\me\scoop\apps\beckon\current\beckon-serve.exe");
        assert_eq!(scoop_current_path(&p), p);
    }

    #[test]
    fn a_path_merely_containing_the_word_scoop_is_untouched() {
        let p = PathBuf::from(r"C:\scoop-backups\beckon\0.7.0\beckon-serve.exe");
        assert_eq!(scoop_current_path(&p), p);
    }

    #[test]
    fn non_scoop_paths_are_untouched() {
        let p = PathBuf::from(r"C:\Program Files\beckon\beckon-serve.exe");
        assert_eq!(scoop_current_path(&p), p);
    }
}
```

Declare the module in `crates/beckon-cli/src/lib.rs`, next to the other `mod` lines:

```rust
mod serve_app;
```

- [ ] **Step 3: Run the tests to verify they fail**

```bash
cargo test -p beckon-cli
```

Expected: `cannot find function 'run_key_command_line' in this scope`.

- [ ] **Step 4: Implement the two pure helpers**

Add above the test module in `serve_app.rs`:

```rust
use std::path::{Path, PathBuf};

/// Build the `HKCU\…\Run` value.
///
/// `config` and `log` are passed as `Some` **only when they differ from the
/// defaults**. Ticking "Start with Windows" while running against a
/// non-default config must not silently hand the user the default config at
/// next logon; omitting the defaults keeps the common value short enough to
/// read in regedit.
pub fn run_key_command_line(exe: &Path, config: Option<&Path>, log: Option<&Path>) -> String {
    let mut s = format!("\"{}\"", exe.display());
    if let Some(c) = config {
        s.push_str(&format!(" \"{}\"", c.display()));
    }
    if let Some(l) = log {
        s.push_str(&format!(" --log \"{}\"", l.display()));
    }
    s
}

/// Rewrite a Scoop versioned install path to the `current` junction.
///
/// Scoop lays out `…\scoop\apps\<name>\<version>\` and keeps a `current`
/// junction pointing at the active one. A Run value naming the versioned
/// directory stops working at the next `scoop update`, which deletes it —
/// and because the entry no longer launches, it never gets the chance to
/// repair itself. Anything that is not that exact shape is returned
/// unchanged.
pub fn scoop_current_path(exe: &Path) -> PathBuf {
    let parts: Vec<_> = exe.components().collect();
    // Need at least: … scoop, apps, <name>, <version>, <file>
    for i in 0..parts.len().saturating_sub(4) {
        let seg = |n: usize| parts[n].as_os_str().to_str().unwrap_or_default();
        if !seg(i).eq_ignore_ascii_case("scoop") || !seg(i + 1).eq_ignore_ascii_case("apps") {
            continue;
        }
        if seg(i + 3).eq_ignore_ascii_case("current") {
            return exe.to_path_buf();
        }
        let mut out = PathBuf::new();
        for (n, part) in parts.iter().enumerate() {
            if n == i + 3 {
                out.push("current");
            } else {
                out.push(part.as_os_str());
            }
        }
        return out;
    }
    exe.to_path_buf()
}
```

> `Path::components` on a Unix host does not split `C:\a\b` — the tests above use backslash literals, so on macOS/Linux they exercise a single component and the function correctly returns the input unchanged. **This makes four of the six tests vacuous off-Windows.** Verify them on the `windows-latest` CI job before believing them; if they pass vacuously everywhere, rewrite the helper to split on both separators explicitly so the logic is exercised on every runner. Prefer the rewrite — a test that only really runs on one platform is the thing this task's Step 1 was about.

- [ ] **Step 5: Run the tests to verify they pass**

```bash
cargo test -p beckon-cli
```

Then push and read the `windows-latest` job, per the warning above.

- [ ] **Step 6: Implement the registry side**

Create `crates/beckon-windows/src/autostart.rs`:

```rust
//! "Start with Windows" as an `HKCU\…\Run` value.
//!
//! Chosen over a Scheduled Task (which needs ~200 lines of COM and appears
//! in no user-facing list) and over a Startup-folder shortcut. A Run value
//! shows up in Task Manager -> Startup apps and in Settings -> Apps ->
//! Startup, so the user can turn it off the way they turn off every other
//! app. The `RestartOnFailure` it gives up was mostly guarding against the
//! Windows Terminal tab that CTRL_CLOSE_EVENTs a console-hosted serve --
//! a cause a GUI-subsystem binary does not have.
//!
//! No path or quoting policy lives here; see
//! `beckon-cli/src/serve_app.rs::run_key_command_line`.

use windows::core::{HSTRING, PCWSTR};
use windows::Win32::System::Registry::{
    RegCloseKey, RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW, HKEY,
    HKEY_CURRENT_USER, KEY_READ, KEY_WRITE, REG_SZ,
};

const RUN_KEY: PCWSTR = windows::core::w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
const VALUE: PCWSTR = windows::core::w!("beckon");

fn open(access: windows::Win32::System::Registry::REG_SAM_FLAGS) -> Option<HKEY> {
    let mut key = HKEY::default();
    let rc = unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, RUN_KEY, Some(0), access, &mut key) };
    rc.is_ok().then_some(key)
}

/// Is the autostart value present? Its contents are deliberately not parsed
/// -- the tick box answers "will this start at logon", nothing finer.
pub fn is_enabled() -> bool {
    let Some(key) = open(KEY_READ) else { return false };
    let rc = unsafe { RegQueryValueExW(key, VALUE, None, None, None, None) };
    let _ = unsafe { RegCloseKey(key) };
    rc.is_ok()
}

pub fn enable(command: &str) -> Result<(), String> {
    let key = open(KEY_WRITE).ok_or_else(|| "cannot open the Run key for writing".to_string())?;
    let wide = HSTRING::from(command);
    // REG_SZ wants the byte length INCLUDING the NUL terminator.
    let bytes = unsafe {
        std::slice::from_raw_parts(
            wide.as_ptr() as *const u8,
            (wide.len() + 1) * std::mem::size_of::<u16>(),
        )
    };
    let rc = unsafe { RegSetValueExW(key, VALUE, Some(0), REG_SZ, Some(bytes)) };
    let _ = unsafe { RegCloseKey(key) };
    rc.ok().map_err(|e| format!("RegSetValueExW failed: {e}"))
}

pub fn disable() -> Result<(), String> {
    let key = open(KEY_WRITE).ok_or_else(|| "cannot open the Run key for writing".to_string())?;
    let rc = unsafe { RegDeleteValueW(key, VALUE) };
    let _ = unsafe { RegCloseKey(key) };
    rc.ok().map_err(|e| format!("RegDeleteValueW failed: {e}"))
}
```

Add the feature to `crates/beckon-windows/Cargo.toml`, keeping the list alphabetical:

```toml
    "Win32_System_Registry",
```

Declare the module in `crates/beckon-windows/src/lib.rs`:

```rust
#[cfg(target_os = "windows")]
pub mod autostart;
```

> Signatures again: `RegOpenKeyExW`/`RegSetValueExW` return `WIN32_ERROR` in `windows` 0.61, which has `.is_ok()` and `.ok()`. `WIN32_ERROR::ok()` returns `windows::core::Result<()>`. Let `WINCHECK` correct the shapes above.

- [ ] **Step 7: Add the menu item**

In `crates/beckon-cli/src/serve.rs`, add the constant beside the others:

```rust
#[cfg(target_os = "windows")]
const MENU_AUTOSTART: u32 = 6;
```

Insert into `build_entries`, between the `MENU_PAUSE` row and the separator that precedes Quit:

```rust
        hotkey::MenuEntry {
            id: MENU_AUTOSTART,
            label: "Start with Windows".into(),
            checked: Some(m.autostart),
            enabled: true,
        },
```

In `install_tray_menu`, replace `autostart: false, // Task 5 fills this in` with:

```rust
            autostart: beckon_windows::autostart::is_enabled(),
```

and add this arm to `on_click`:

```rust
            MENU_AUTOSTART => {
                let result = if beckon_windows::autostart::is_enabled() {
                    beckon_windows::autostart::disable()
                } else {
                    match std::env::current_exe() {
                        Ok(exe) => {
                            let exe = crate::serve_app::scoop_current_path(&exe);
                            let s = st.borrow();
                            let cmd = crate::serve_app::run_key_command_line(
                                &exe,
                                s.autostart_config.as_deref(),
                                s.autostart_log.as_deref(),
                            );
                            drop(s);
                            beckon_windows::autostart::enable(&cmd)
                        }
                        Err(e) => Err(format!("cannot find our own path: {e}")),
                    }
                };
                if let Err(e) = result {
                    eprintln!("beckon serve: autostart: {e}");
                    // The user just clicked this; tell them every time.
                    crate::notify::report(
                        &format!("could not change autostart: {e}"),
                        crate::notify::Cause::HumanAction,
                    );
                }
            }
```

Add the two fields to `ServeState`, set to `None` in `cmd_serve_with_log` and filled in by Task 7:

```rust
    /// Config path to bake into the autostart value, or `None` when the
    /// running config is already the default. See
    /// `serve_app::run_key_command_line`.
    autostart_config: Option<PathBuf>,
    /// Log path to bake into the autostart value, or `None` when default.
    autostart_log: Option<PathBuf>,
```

- [ ] **Step 8: Update the menu test**

Extend `menu_shows_the_phrase_and_reflects_pause` with:

```rust
        let on = MenuModel { autostart: true, ..m.clone() };
        assert_eq!(
            build_entries(&on).iter().find(|r| r.id == MENU_AUTOSTART).unwrap().checked,
            Some(true)
        );
```

- [ ] **Step 9: Run everything and commit**

```bash
cargo fmt --all
cargo test --workspace --exclude beckon-linux --exclude beckon-windows
cargo clippy --workspace --exclude beckon-linux --exclude beckon-windows --all-targets -- -D warnings
```

Then `WINCHECK` and the Windows CI job.

```bash
git add crates/beckon-windows/src/autostart.rs crates/beckon-windows/src/lib.rs \
        crates/beckon-windows/Cargo.toml crates/beckon-cli/src/serve_app.rs \
        crates/beckon-cli/src/lib.rs crates/beckon-cli/src/serve.rs
git commit -m "feat(windows): 'Start with Windows' as an HKCU Run value

Answers open question #1 (serve --install vs serve install) with a third
option -- neither. A tick box costs no top-level verb and shows up in
Task Manager's Startup tab, which is where users already go to turn
startup apps off.

The value carries a non-default config and log so ticking the box while
running against one config does not silently hand back the default at
logon. Scoop's versioned install path is rewritten to the 'current'
junction first: a Run value naming the versioned directory dies at the
next scoop update and, because it no longer launches, cannot repair
itself."
```

---

## Task 6: Default paths, starter config, first run

**Files:**
- Modify: `crates/beckon-cli/src/serve_app.rs`

**Interfaces:**
- Produces: `default_config_path(home: &Path) -> PathBuf`, `default_log_path(local_appdata: &Path) -> PathBuf`, `starter_template() -> &'static str`, `ensure_config(path: &Path) -> std::io::Result<bool>` (`true` = it was created now). All pure or filesystem-only, tested on every runner.

- [ ] **Step 1: Write the failing tests**

Add to `serve_app.rs`'s `mod tests`:

```rust
    #[test]
    fn default_paths_sit_where_the_readme_says() {
        assert_eq!(
            default_config_path(Path::new(r"C:\Users\me")),
            PathBuf::from(r"C:\Users\me").join(".config").join("beckon").join("apps.toml")
        );
        assert_eq!(
            default_log_path(Path::new(r"C:\Users\me\AppData\Local")),
            PathBuf::from(r"C:\Users\me\AppData\Local").join("beckon").join("serve.log")
        );
    }

    #[test]
    fn the_starter_template_is_a_valid_shortcuts_file() {
        let parsed = beckon_core::shortcuts::parse_shortcuts(starter_template())
            .expect("the very first file a new user sees must not fail validation");
        assert!(
            !parsed.is_empty(),
            "an empty template teaches nothing and registers nothing"
        );
    }

    #[test]
    fn ensure_config_creates_once_then_leaves_it_alone() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = dir.path().join("nested").join("apps.toml");

        assert!(ensure_config(&cfg).unwrap(), "first call must create it");
        assert!(cfg.exists());
        std::fs::write(&cfg, "\"ctrl+alt+z\" = \"Zed\"\n").unwrap();

        assert!(!ensure_config(&cfg).unwrap(), "second call must not create");
        assert_eq!(
            std::fs::read_to_string(&cfg).unwrap(),
            "\"ctrl+alt+z\" = \"Zed\"\n",
            "an existing config must never be overwritten"
        );
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

```bash
cargo test -p beckon-cli
```

Expected: `cannot find function 'default_config_path'`.

- [ ] **Step 3: Implement**

Add to `serve_app.rs`:

```rust
/// `%USERPROFILE%\.config\beckon\apps.toml`.
///
/// `.config` rather than `%APPDATA%`: it is the path the README already
/// tells Windows users to create, and it is the path macOS uses. The
/// shortcuts file is designed to validate on every platform, so one
/// location across all three beats one platform's idiom.
pub fn default_config_path(home: &Path) -> PathBuf {
    home.join(".config").join("beckon").join("apps.toml")
}

/// `%LOCALAPPDATA%\beckon\serve.log` — the path the Scheduled Task example
/// already uses, so an existing install's log does not move.
pub fn default_log_path(local_appdata: &Path) -> PathBuf {
    local_appdata.join("beckon").join("serve.log")
}

/// The file a brand-new user gets. Every binding here must parse, because
/// this is the first thing beckon ever shows them.
///
/// ASCII only: this text can be echoed into the log, and Windows
/// PowerShell 5.1's Get-Content defaults to ANSI.
pub fn starter_template() -> &'static str {
    r#"# beckon shortcuts. Edit and save -- beckon reloads automatically.
#
#   "<modifiers>+<key>" = "<app Name>"
#
# Modifiers: ctrl, super (the Windows key), alt, shift -- any order.
# Keys are lowercase: a-z, 0-9, f1-f20, and names like space, comma, pageup.
#
# Find the Name to use on the right-hand side with:
#   beckon installed
#   beckon search <part of the name>
#
# Check a file without starting anything:
#   beckon check "%USERPROFILE%\.config\beckon\apps.toml"

"ctrl+super+alt+t" = "Terminal"
"ctrl+super+alt+e" = "File Explorer"
"#
}

/// Create `path` with the starter template if it is not there.
///
/// Returns `true` when it created the file. Never overwrites: a user whose
/// config exists must keep it, whatever else goes wrong.
pub fn ensure_config(path: &Path) -> std::io::Result<bool> {
    if path.exists() {
        return Ok(false);
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, starter_template())?;
    Ok(true)
}
```

`tempfile` is already a dev-dependency of `beckon-cli`; `beckon_core` is already a dependency.

- [ ] **Step 4: Run the tests to verify they pass**

```bash
cargo test -p beckon-cli
```

Expected: all three pass, on macOS as well as Windows.

- [ ] **Step 5: Commit**

```bash
git add crates/beckon-cli/src/serve_app.rs
git commit -m "feat(cli): default paths and a starter shortcuts file

A double-click with no config must not be an error message. ensure_config
writes a commented template and never overwrites an existing file; a test
parses that template through parse_shortcuts, because the first file a new
user sees failing validation would be the worst possible introduction.

Config defaults to %USERPROFILE%\\.config\\beckon\\apps.toml -- what the
README already tells Windows users to make, and what macOS uses."
```

---

## Task 7: The GUI-subsystem binary

**Files:**
- Create: `crates/beckon-cli/src/bin/beckon-serve.rs`
- Modify: `crates/beckon-cli/Cargo.toml`, `crates/beckon-cli/src/lib.rs`, `crates/beckon-cli/src/serve_app.rs`, `crates/beckon-windows/src/logfile.rs`

**Interfaces:**
- Consumes: everything from Tasks 1–6.
- Produces: `beckon_cli::serve_app_main()` (Windows only) — never returns normally; exits the process.

- [ ] **Step 1: Declare the second binary**

Append to `crates/beckon-cli/Cargo.toml`:

```toml
[[bin]]
name = "beckon-serve"
path = "src/bin/beckon-serve.rs"
```

- [ ] **Step 2: Write the entry point**

Create `crates/beckon-cli/src/bin/beckon-serve.rs`:

```rust
//! `beckon serve` with no console: the Windows tray app.
//!
//! Only the subsystem attribute and a call into the library live here. The
//! attribute is the entire reason this binary exists — the subsystem is a
//! bit in the PE header, not a runtime switch, so `beckon.exe` cannot be
//! both this and a working CLI. Flipping the whole binary would break
//! `list`, `installed`, `search`, `resolve` and `doctor`, whose output the
//! shell prints after returning its prompt.
#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

#[cfg(target_os = "windows")]
fn main() {
    beckon_cli::serve_app_main();
}

// Cargo cannot gate a [[bin]] on target_os, so this target still builds on
// Linux and macOS. It is never packaged there: the release workflow's unix
// step copies `beckon` by name and nothing else.
#[cfg(not(target_os = "windows"))]
fn main() {
    eprintln!("beckon-serve is Windows-only; use `beckon serve <CONFIG>` instead");
    std::process::exit(1);
}
```

- [ ] **Step 3: Note the GUI-subsystem reality in `logfile.rs`**

The module doc currently promises *"Everything fallible runs before `FreeConsole`, because `main` reports errors with `eprintln!`"*. Append to that paragraph:

```rust
//! **That promise is void in a GUI-subsystem process** (`beckon-serve.exe`):
//! there is no console at any point, not even before the detach, so an
//! `Err` from here reaches nobody through stderr. `serve_app_main` owns its
//! own failure path — a MessageBox — and calls this before anything else
//! can print. `FreeConsole` itself simply fails there, which is why it is
//! already `let _ =`.
```

- [ ] **Step 4: Write `serve_app_main`**

Append to `crates/beckon-cli/src/serve_app.rs`:

```rust
#[cfg(target_os = "windows")]
mod app {
    use super::*;
    use clap::Parser;

    /// `beckon-serve.exe [CONFIG] [--log PATH]` — the same two operands
    /// `beckon serve` takes, both optional so that a double-click and a bare
    /// Run-key value are the normal invocation.
    #[derive(Parser, Debug)]
    #[command(name = "beckon-serve", version, about = "beckon resident hotkey service (tray app)")]
    struct ServeAppArgs {
        #[arg(value_name = "CONFIG")]
        config: Option<PathBuf>,

        /// Send stderr to PATH instead of the default log.
        #[arg(long, value_name = "PATH")]
        log: Option<PathBuf>,
    }

    fn die(body: &str) -> ! {
        beckon_windows::shell::error_dialog("beckon serve", body);
        std::process::exit(1);
    }

    pub fn main() {
        let args = ServeAppArgs::parse();

        // 1. The log, before anything can print. Every eprintln! in this
        //    process lands in the file after this returns; before it, there
        //    is nowhere for one to go.
        let log_default = std::env::var_os("LOCALAPPDATA")
            .map(|p| default_log_path(Path::new(&p)))
            .unwrap_or_else(|| PathBuf::from("beckon-serve.log"));
        let log = args.log.clone().unwrap_or(log_default.clone());
        if let Err(e) = beckon_windows::logfile::redirect_to_log(&log) {
            die(&format!("Cannot open the log file:\n{}\n\n{e:#}", log.display()));
        }

        // 2. The config, created on first run so a double-click works with
        //    nothing read beforehand.
        let cfg_default = std::env::var_os("USERPROFILE")
            .map(|p| default_config_path(Path::new(&p)))
            .unwrap_or_else(|| PathBuf::from("apps.toml"));
        let config = args.config.clone().unwrap_or(cfg_default.clone());
        match ensure_config(&config) {
            Err(e) => die(&format!("Cannot create the config file:\n{}\n\n{e}", config.display())),
            Ok(true) => {
                eprintln!("beckon serve: created {}", config.display());
                if let Err(e) = beckon_windows::shell::open_path(&config) {
                    eprintln!("beckon serve: {e}");
                }
            }
            Ok(false) => {}
        }

        // 3. Only non-default values go into the autostart command line.
        let autostart_config = (config != cfg_default).then(|| config.clone());
        let autostart_log = (log != log_default).then(|| log.clone());

        if let Err(e) = crate::serve::cmd_serve_app(
            &config,
            Some(log),
            autostart_config,
            autostart_log,
        ) {
            eprintln!("beckon serve: {e:#}");
            // The lock refusal is a designed outcome, not a fault -- but with
            // no console the user needs telling, or a double-click looks like
            // it did nothing at all.
            die(&format!("{e:#}"));
        }
        // cmd_serve_app only returns on error; run_forever exits the process.
    }
}

/// Entry point for `beckon-serve.exe`. Never returns normally.
#[cfg(target_os = "windows")]
pub fn serve_app_main() {
    app::main()
}
```

Re-export it from `crates/beckon-cli/src/lib.rs`:

```rust
#[cfg(target_os = "windows")]
pub use serve_app::serve_app_main;
```

`mod serve_app;` stays private — re-exporting a `pub` item out of a private module is allowed, and `serve.rs` reaches the helpers as `crate::serve_app::…` because they are `pub fn` inside a sibling module. Do **not** widen the module itself.

- [ ] **Step 5: Add `cmd_serve_app` to `serve.rs`**

Rename `cmd_serve_with_log` (Task 4) to take the two autostart paths as well, keeping `cmd_serve` and `cmd_serve_with_log` as wrappers so no existing call site changes:

```rust
pub fn cmd_serve(config: &Path) -> Result<()> {
    cmd_serve_app(config, None, None, None)
}

/// The Windows app entry: `log` tells the tray menu's "Open log" where to
/// point, and the two `autostart_*` paths are baked into the Run value only
/// when they differ from the defaults.
pub fn cmd_serve_app(
    config: &Path,
    log: Option<PathBuf>,
    autostart_config: Option<PathBuf>,
    autostart_log: Option<PathBuf>,
) -> Result<()> {
```

Delete the intermediate `cmd_serve_with_log` if nothing else calls it. Populate the four new `ServeState` fields from the parameters.

- [ ] **Step 6: Build and verify the non-Windows stub is clean**

```bash
cargo fmt --all
cargo build --workspace --exclude beckon-linux --exclude beckon-windows --all-targets
cargo clippy --workspace --exclude beckon-linux --exclude beckon-windows --all-targets -- -D warnings
./target/debug/beckon-serve      # prints the Windows-only line
echo $?                          # 1
cargo test --workspace --exclude beckon-linux --exclude beckon-windows
```

Then `WINCHECK`.

- [ ] **Step 7: Commit**

```bash
git add crates/beckon-cli/Cargo.toml crates/beckon-cli/src/bin/beckon-serve.rs \
        crates/beckon-cli/src/lib.rs crates/beckon-cli/src/serve_app.rs \
        crates/beckon-cli/src/serve.rs crates/beckon-windows/src/logfile.rs
git commit -m "feat(windows): beckon-serve.exe, a GUI-subsystem tray app

The subsystem is a PE header bit, not a runtime switch, so this has to be
a second binary -- flipping beckon.exe would break every command whose
output the shell prints after returning its prompt.

logfile.rs promised that everything fallible runs before FreeConsole
'because main reports errors with eprintln!'. That promise is void here:
there is no console at any point. So the redirect runs first and owns its
own failure path, a MessageBox, and the same box is what reports the
single-instance refusal -- with no console, a double-click that silently
exits looks like a broken app."
```

- [ ] **Step 8: Manual verification on a14**

This is the first point at which the app can be run. Every check below **must happen in the interactive session**: SSH to a14 lands in session 0, where there is no taskbar and hotkeys never fire, so a plain SSH shell produces confident false negatives. Drive it through a scheduled-task probe in the logged-on session, using `-EncodedCommand` to avoid quoting damage.

Record actual results next to each:

- [ ] No console window appears at any point during startup. Control: `beckon.exe serve <cfg> --log <path>` still shows one for ~60 ms.
- [ ] Tray icon appears; hovering shows `beckon - N shortcuts registered`.
- [ ] Right-click opens the menu; every item does what it says.
- [ ] Pause unregisters (a bound chord stops working) and unpausing restores it.
- [ ] Editing and saving the config updates the tooltip within ~1 s.
- [ ] A hotkey pressed **while the menu is open** still fires — this is the `MENU_PENDING` / `HOTKEY_PENDING` path.
- [ ] Tick "Start with Windows"; confirm the value in `HKCU\…\Run`; reboot; confirm the tray comes back. Untick; reboot; confirm it does not.
- [ ] Delete `apps.toml`, launch: template is written, editor opens, tooltip reads `beckon - 0 shortcuts registered`.
- [ ] Launch a second copy: MessageBox says it is already running; the first keeps working.
- [ ] Quit from the menu: icon disappears, process exits, hotkeys stop.

---

## Task 8: Application icon

**Blocked on an asset that does not exist.** If no `.ico` is available, skip this task and land Task 9 — everything works, it just wears the generic Windows icon.

**Files:**
- Create: `assets/beckon.ico`, `crates/beckon-cli/build.rs`, `crates/beckon-cli/beckon.rc`
- Modify: `crates/beckon-cli/Cargo.toml`, `crates/beckon-windows/src/hotkey.rs`

- [ ] **Step 1: Obtain the icon**

A multi-resolution `.ico` containing at least 16×16, 32×32, 48×48 and 256×256. 16×16 is the tray size and 32×32 the Alt-Tab size; a single-size file is upscaled and looks blurry in exactly the two places users see it most.

- [ ] **Step 2: Add the resource script**

Create `crates/beckon-cli/beckon.rc`:

```
1 ICON "../../assets/beckon.ico"
```

Resource id 1 so `LoadIconW(hinst, MAKEINTRESOURCE(1))` finds it.

- [ ] **Step 3: Add the build script**

Add to `crates/beckon-cli/Cargo.toml`:

```toml
[build-dependencies]
embed-resource = "2"
```

Create `crates/beckon-cli/build.rs`:

```rust
fn main() {
    // MSVC only, deliberately. embed-resource shells out to a resource
    // compiler: rc.exe for -msvc, windres for -gnu. The dev host has
    // neither, and WINCHECK cross-checks against x86_64-pc-windows-gnu --
    // so compiling the resource unconditionally would break the project's
    // own local Windows gate on a machine that never ships a binary anyway.
    // The icon only has to exist in what we release, and every released
    // Windows artifact is -msvc.
    //
    // Applies to every binary in the package, so `beckon.exe` gets the icon
    // in Explorer too.
    // Both directives are required, and the icon one is the load-bearing
    // half. `embed-resource` documents that it emits no rerun-if-changed
    // annotation of its own, so with none here Cargo falls back to
    // "rescan the package directory" -- and `assets/` is at the repo root,
    // OUTSIDE this package. Editing the icon alone would then not rebuild
    // the resource, and a stale icon would stay embedded until an
    // unrelated change in `crates/beckon-cli/` or a `cargo clean`.
    //
    // Naming beckon.rc as well is belt-and-braces: the default heuristic
    // already covers it, but stating it keeps the two inputs symmetrical
    // for the next reader.
    println!("cargo:rerun-if-changed=../../assets/beckon.ico");
    println!("cargo:rerun-if-changed=beckon.rc");

    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        embed_resource::compile("beckon.rc", embed_resource::NONE);
    }
}
```

**Measured, not assumed:** `command -v windres x86_64-w64-mingw32-windres
llvm-rc rc` finds nothing on the macOS dev host, so the guard is load-bearing
rather than defensive. CI's `windows-latest` runner is `-msvc` and does have
`rc.exe`, so the resource genuinely gets compiled and verified there.

> Check `embed-resource`'s major version before writing this. In 2.x
> `compile` returns `()` and panics on failure; in 3.x it returns a
> `CompilationResult` that should be inspected. Match whichever the
> lockfile resolves.

- [ ] **Step 4: Use it for the tray icon**

In `hotkey.rs`'s `tray_add`, replace the `LoadIconW(None, IDI_APPLICATION)` line with:

```rust
    // Resource id 1 from beckon.rc; fall back to the stock icon if the
    // resource is missing, so an icon-less build still shows *something*
    // rather than no tray icon at all.
    nid.hIcon = unsafe {
        let hinst = GetModuleHandleW(None).unwrap_or_default();
        LoadIconW(Some(hinst.into()), PCWSTR(1 as *const u16))
            .or_else(|_| LoadIconW(None, IDI_APPLICATION))
    }
    .unwrap_or_default();
```

- [ ] **Step 5: Build and check by eye**

`WINCHECK`, then build on Windows and confirm the icon appears in the tray, in Alt-Tab, and on the file in Explorer.

- [ ] **Step 6: Commit**

```bash
git add assets/beckon.ico crates/beckon-cli/beckon.rc crates/beckon-cli/build.rs \
        crates/beckon-cli/Cargo.toml crates/beckon-windows/src/hotkey.rs
git commit -m "feat(windows): embed an application icon

IDI_APPLICATION reads as an unfinished app in the tray, in Alt-Tab and in
Explorer -- the three places this binary is now visible that the CLI never
was. Falls back to the stock icon if the resource is missing so an
icon-less build still shows a tray icon."
```

---

## Task 9: Packaging and documentation

**Files:**
- Modify: `packaging/scoop/beckon.json.template`, `.github/workflows/release.yml`, `README.md`, `CLAUDE.md`, `examples/windows/serve/README.md`

- [ ] **Step 1: Ship the second binary in the Windows zip**

In `.github/workflows/release.yml`, in the `Package (windows)` step, after the existing `Copy-Item … beckon.exe` line:

```powershell
          Copy-Item 'target/${{ matrix.target }}/release/beckon-serve.exe' -Destination $stage
```

The `Package (unix)` step needs **no** change: it copies `target/…/release/beckon` by name, so `beckon-serve` never enters the tarball. (The spec called for an explicit exclusion; the workflow's copy-by-name already provides it.)

- [ ] **Step 2: Update the Scoop manifest**

In `packaging/scoop/beckon.json.template`, replace `"bin": "beckon.exe",` with:

```json
  "bin": [
    "beckon.exe",
    "beckon-serve.exe"
  ],
  "shortcuts": [
    [
      "beckon-serve.exe",
      "beckon serve"
    ]
  ],
```

- [ ] **Step 3: Rewrite the README's Windows resident-mode text**

In `README.md`'s *Resident mode (macOS & Windows)* section:

- Lead the Windows half with `beckon-serve.exe`: install with Scoop, launch from the Start Menu, tick **Start with Windows** in the tray menu. Note that a first launch with no config writes a starter file and opens it.
- **Delete** the paragraph warning that the tray icon is a one-directional signal ("icon absent means either the daemon is dead OR the tray just isn't ready yet"). The tooltip now answers it. Replace with one line: the tooltip reports how many shortcuts actually registered, and `paused` when paused.
- Keep the "Trust the registration count, not the shortcut count" paragraph — it is still true and now has a second surface.
- Demote the Scheduled Task to a sentence pointing at `examples/windows/serve/`, described as the supervised path for anyone who wants `RestartOnFailure`.

- [ ] **Step 4: Make the three CLAUDE.md edits**

1. **Out of scope → "GUI / TUI — CLI only"**: rewrite. A tray context menu is `serve`'s control surface, not a launcher UI. Name the deferred settings window as still open, and record why it is expensive: `msctls_hotkey32` cannot capture the Windows key, and `Win+T` is a shell hotkey, so chord capture must be measured on hardware before a toolkit is chosen.
2. **Open questions → #1**: it asked whether the install lifecycle should be `serve --install` or `serve install`. Record the answer: neither — a tick box in the tray, which costs no top-level verb, and the growth rule never had to be spent.
3. **Phase 3 Windows notes → the `--log` bullet list**: add that `beckon-serve.exe` has no console flash at all because it is GUI-subsystem, and that `CREATE_NO_WINDOW` on the toast spawn is load-bearing there for the same reason it is after `FreeConsole` — a console-subsystem child of a console-less parent is given a new, visible console.

- [ ] **Step 5: Reframe the Scheduled Task example**

In `examples/windows/serve/README.md`, add a lead paragraph: most users want `beckon-serve.exe` and the tray tick box; this XML is for supervised runs that need `RestartOnFailure`. Keep every existing measured note (the SID principal, the `encoding="UTF-16"` declaration) — both were hit against real hardware and remain true.

- [ ] **Step 6: Verify the docs against the code**

Re-read each changed doc beside the implementation. Every command shown must be one that now exists; every path must be one the code actually uses.

- [ ] **Step 7: Commit**

```bash
git add packaging/scoop/beckon.json.template .github/workflows/release.yml \
        README.md CLAUDE.md examples/windows/serve/README.md
git commit -m "docs: beckon-serve.exe is the Windows resident-mode front door

Scoop ships both binaries and a Start Menu shortcut; the Scheduled Task
becomes the supervised path rather than the default one.

Drops the README's warning that the tray icon is a one-directional
signal -- the tooltip now says how many shortcuts actually registered,
so there is nothing left to warn about. Closes open question #1 in
CLAUDE.md and rewrites the 'CLI only' scope line, which predated serve."
```

---

## Definition of done

- [ ] `cargo test --workspace` green on all three CI runners.
- [ ] `cargo clippy --all-targets -- -D warnings` green on all three.
- [ ] `beckon.exe --version`, `--help`, `list`, `resolve`, `doctor` behave exactly as before Task 1.
- [ ] Every manual check in Task 7 Step 8 ticked with a real observation on a14, in the interactive session.
- [ ] `docs/superpowers/specs/2026-08-10-windows-serve-app-design.md` still describes what was built; anything that changed during implementation is corrected there.
