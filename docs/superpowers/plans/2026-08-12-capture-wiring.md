# Landing 2b-v: the hook refcount and capture wiring — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let the user record a shortcut by pressing it — the last piece of
Landing 2b, and the only one that arms a keyboard hook for a second reason.

**Architecture:** §F.2 is emphatic: **do not install a second hook.**
`WH_KEYBOARD_LL` hooks chain, and a separate capture hook would (a) record the
alias `Caps+T` injects rather than the key pressed, and (b) swallow the
Caps-up, leaving `CapsState.held` stuck true. So the one hook checks a capture
mode **first**, and the hook's lifetime becomes a two-reason thing: Caps may
want it resident while capture wants it transiently.

**Tech Stack:** Rust. `beckon-core`, `beckon-windows`, `beckon-cli`.

**Spec:** `docs/superpowers/specs/2026-08-11-settings-window-redesign.md`
§F.1, §F.2, §F.3, §F.4.

## Global Constraints

- **ABORT-CLASS.** Never hold a `RefCell` borrow of `UI`, `ServeState`, or the
  hook's own `STATE` / `CONFIG` across any `SendMessageW` / `PostMessageW` /
  `SetWindowPos` / `SetFocus` / `SetWindowTextW`. A second borrow across the
  `extern "system"` boundary **aborts the process** rather than unwinding.
- **`Outcome::PassThrough` is the ONE outcome where the hook must not return
  `1`.** §F.2's sketch returns `LRESULT(1)` unconditionally while armed; that
  sketch predates the outcome. Wiring that returns 1 for everything except
  `Disarmed` reinstates the stuck-modifier bug **with every `beckon-core` test
  still green**, because nothing there can see what the callback returns. The
  bug: hold `Ctrl` physically, click `Record` with the **mouse**, press
  `Alt+T`; the drain eats the Ctrl-up and the system believes Ctrl is held
  forever.
- **The callback does three things only**: read `vkCode`, update a fixed-size
  held array, `PostMessage`. **No allocation, no string formatting, no
  `CallNextHookEx` while capturing.** Windows silently unhooks a callback that
  overruns `LowLevelHooksTimeout` (300 ms default) — no error anywhere.
- **Nothing on the hook's thread may block.** The callback is dispatched by
  the message loop of the thread that installed it, which is the thread
  hosting the settings window and `WM_HOTKEY`. A modal loop or a synchronous
  scan starves it as effectively as a slow callback does.
- **The refusal beep must be de-duplicated by vk.** Auto-repeat of a refused
  bare key re-refuses on every repeat. Do **not** fix that by admitting
  refused keys to the held set — rolled-over bare keys would consume the
  `HELD_MAX` slots and silently drop a real modifier.
- Display strings ASCII and, where §F.3 gives them, **verbatim**.
- Gates: `cargo fmt --all -- --check`, `cargo test -p beckon-core`,
  `cargo clippy -p beckon-core --all-targets -- -D warnings`,
  `cargo check --target x86_64-pc-windows-gnu -p beckon-windows --all-targets`,
  `cargo check --target x86_64-pc-windows-gnu -p beckon-cli`.
- `cargo test --workspace` is already broken on macOS; pre-existing, ignore.

---

## File Structure

| File | Responsibility | Task |
|---|---|---|
| `crates/beckon-core/src/capture.rs` | `HookOwners`, and the §F.3 hint strings | 1 |
| `crates/beckon-windows/src/caps_hook.rs` | two-reason lifetime; capture mode in `hook_proc` | 1, 2 |
| `crates/beckon-windows/src/settings_window.rs` | `Record` / `Reset`, `WM_CAPTURE`, watchdog, focus loss | 3 |
| `crates/beckon-cli/src/serve.rs` | wire the capture callbacks | 3 |

---

## Task 1: two reasons to hold one hook, and the strings

**Files:**
- Modify: `crates/beckon-core/src/capture.rs`, `crates/beckon-windows/src/caps_hook.rs`
- Test: `crates/beckon-core/src/capture.rs`

**Interfaces:**
- Produces: `capture::HookOwners`, `capture::hint(...)`, and
  `caps_hook::{install_for, uninstall_for, HookReason}`.

- [ ] **Step 1: Write the failing core tests**

```rust
    #[test]
    fn the_hook_lives_while_either_reason_holds_it() {
        let mut o = HookOwners::default();
        assert!(!o.wanted());
        assert!(o.add(HookReason::Caps));            // true = the OS call is needed
        assert!(o.wanted());
        assert!(!o.add(HookReason::Capture), "already installed; no second SetWindowsHookEx");
        assert!(!o.remove(HookReason::Capture), "Caps still wants it");
        assert!(o.wanted());
        assert!(o.remove(HookReason::Caps));         // true = now unhook
        assert!(!o.wanted());
    }

    /// The reason the refcount exists at all: capture ending must not reset
    /// the Caps state machine, and a config reload during capture must not
    /// reinstall the hook underneath it.
    #[test]
    fn dropping_capture_while_caps_holds_does_not_ask_for_an_unhook() {
        let mut o = HookOwners::default();
        o.add(HookReason::Caps);
        o.add(HookReason::Capture);
        assert!(!o.remove(HookReason::Capture));
        assert!(o.wanted());
    }

    #[test]
    fn removing_a_reason_that_never_held_it_changes_nothing() {
        let mut o = HookOwners::default();
        o.add(HookReason::Caps);
        assert!(!o.remove(HookReason::Capture));
        assert!(o.wanted());
    }
```

Plus one test per §F.3 hint string, asserting the text verbatim:
`Press the shortcut. Esc stops recording.`,
`A alone is not a shortcut - hold Ctrl, Win or Alt as well. Press Record and try again.`
(with the key name substituted),
`beckon has no name for that key. Pick one from the Key list.`,
`Cannot record here. Use the modifier boxes and the Key list instead.`

- [ ] **Step 2: Run and confirm they fail.**

- [ ] **Step 3: Implement**

`HookOwners` in `beckon-core`, pure, two bools. `add` and `remove` return
whether the **OS call** is now needed — install on the first reason, unhook on
the last. Pure so all three CI jobs test it, for the reason `caps.rs` gives
about state machines.

`hint(outcome, refused_keycap) -> &'static str` or a small enum-to-string, with
§F.3's text verbatim. The one string carrying a key name is built on the UI
thread, never in the callback.

In `caps_hook.rs`, replace `install()` / `uninstall()` with `install_for` /
`uninstall_for(HookReason)` driving a module-level `HookOwners`. **Only a real
unhook resets `CapsState`** — the current `uninstall()` resets it
unconditionally, which is exactly what would break Caps when a capture ends.

Update the two call sites in `serve.rs` to pass `HookReason::Caps`.

- [ ] **Step 4: Run, confirm green, and confirm nothing previously green went red.**

- [ ] **Step 5: Break it on purpose.** Make `remove` always return `true`.
`dropping_capture_while_caps_holds_does_not_ask_for_an_unhook` must FAIL.
Restore.

- [ ] **Step 6: Gates and commit.**

---

## Task 2: capture mode inside the one hook

**Files:**
- Modify: `crates/beckon-windows/src/caps_hook.rs`

**Interfaces:**
- Consumes: `capture::{step, CaptureState, Outcome}` and Task 1's refcount.
- Produces: `caps_hook::{arm_capture, disarm_capture, capture_armed}`.

- [ ] **Step 1: The capture arm, first in `hook_proc`**

Before `caps::decide` is consulted:

```rust
    if capture_armed() && GetForegroundWindow() == settings_window::hwnd() {
        let outcome = CAP.with(|c| capture::step(ev, &mut c.borrow_mut()));
        if outcome.post() {
            let _ = PostMessageW(Some(hwnd), WM_CAPTURE, WPARAM(outcome as usize), LPARAM(0));
        }
        return match outcome {
            // The ONE outcome that must reach the system. See the constraint.
            Outcome::PassThrough => CallNextHookEx(None, code, wparam, lparam),
            _ => LRESULT(1),
        };
    }
```

The foreground gate is the third of §F.4's three focus layers, and the only
one that fires when a UAC prompt or an elevated window takes foreground
without sending `WM_KILLFOCUS` or `WM_ACTIVATE`.

Encode the outcome into the `WPARAM` rather than sending a pointer: the
callback must not allocate, and the UI thread rebuilds everything from
`CaptureState` when it handles `WM_CAPTURE`.

- [ ] **Step 2: `arm_capture` / `disarm_capture`**

`arm_capture` resets `CaptureState`, sets the flag, and takes the hook through
`install_for(HookReason::Capture)`; it returns whether the hook is actually
installed, so a `SetWindowsHookExW` failure can be reported rather than
entering Armed. `disarm_capture` clears the flag and calls
`uninstall_for(HookReason::Capture)`.

- [ ] **Step 3: Read the diff against the two hard rules**, and say in the
report what you checked: that `PassThrough` is the only branch not returning
1, and that nothing in the capture arm allocates or formats.

- [ ] **Step 4: Gates and commit.**

---

## Task 3: Record, Reset, and the state machine on screen

**Files:**
- Modify: `crates/beckon-windows/src/settings_window.rs`, `crates/beckon-cli/src/serve.rs`

**Ids:** `IDC_RECORD = 1032`, `IDC_RESET = 1033`. Mnemonics must not collide —
check `mod cap`'s table, which is the only guard.

- [ ] **Step 1: The buttons and the states**

`Record` arms; while armed its caption is `Stop`, the four modifier boxes and
the key list are `EnableWindow(false)`, and the hint reads
`Press the shortcut. Esc stops recording.` `Reset` clears the row's combo.

On `WM_CAPTURE` the UI thread reads `CaptureState` and updates: `Partial`
redraws the live combo; `Captured` writes it through the existing
`on_edit_combo`, re-enables the controls and lets the probe run; `Refused`
beeps **once per vk** and shows the hint; `Cancelled` restores; `Disarmed`
releases the hook.

- [ ] **Step 2: The watchdog and the three focus layers**

A 10 s timer bounds the armed window — §F.2 notes `is_installed()` can lie,
because past `LowLevelHooksTimeout` Windows removes the hook silently and
there is no API to ask. Disarm on `WM_KILLFOCUS`, on `WM_ACTIVATE` /
`WM_ACTIVATEAPP` with `WA_INACTIVE`, and on the per-event foreground gate
already in Task 2.

**`WM_CLOSE` disarms before the save prompt; `WM_DESTROY` disarms skipping the
drain.** There must be no path where the window dies with the hook armed —
holding it one beat longer than the window leaves a swallowed keyboard.

- [ ] **Step 3: If `SetWindowsHookExW` fails**

Do not enter Armed, and do not fall back to message-queue capture — that path
cannot see the Windows key, so it fails on precisely the chords beckon
recommends. Hint: `Cannot record here. Use the modifier boxes and the Key list instead.`

- [ ] **Step 4: Gates, ABORT-class read-through, commit.**

---

## Self-Review

**Spec coverage:** §F.1's "one hook, two modes" is Tasks 1-2; §F.2's refcount
and the callback's three jobs are Tasks 1-2; §F.3's states, strings and the
`SetWindowsHookExW` failure path are Tasks 1 and 3; §F.4's focus layers, the
`WM_CLOSE`/`WM_DESTROY` rule and auto-repeat are Tasks 2-3.

**Deliberately NOT here:** Sticky Keys' `GetAsyncKeyState` union at commit
(§F.4) — it needs a live OS read that `capture::step` cannot make, so it is a
parameter a later pass supplies. The `Working. beckon received {combo}.`
string needs a real keypress after Apply.
