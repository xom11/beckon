# macOS menu bar item for `serve`

Status: **design, one measurement outstanding.** Written 2026-08-12. The
run-loop question in §5 decides between the design below and its fallback,
and it has not been answered yet — see §6 for why it could not be answered
from the session that wrote this.

## 1. Scope

`beckon serve` on macOS runs with no visible surface at all. There is no way
to tell it is alive, no way to pause it, and no way to make it re-read its
config short of `brew services restart`. This spec adds one thing: a menu bar
item with a context menu.

It is the macOS counterpart of the Windows tray icon, and deliberately **only**
that. The Windows side also has a settings window (`settings_window.rs`, 4979
lines) and a Caps Lock hook (`caps_hook.rs`, 527 lines). Both are out of scope
here and each needs its own spec — see §10.

## 2. No second binary

`beckon-serve.exe` exists on Windows because the console/GUI split is a bit in
the PE header, fixed at link time: one binary cannot be both a working CLI and
a windowed app. macOS has no such bit. The equivalent is
`TransformProcessType(→ kProcessTransformToUIElementApplication)`, a runtime
call, and `crates/beckon-macos/src/hotkey.rs:159` **already makes it** — every
`beckon serve` on macOS today is already a UIElement process with no Dock icon
and no menu bar of its own. It simply has nothing to show.

So the menu bar item grows into the existing `beckon serve`. Nothing about
distribution changes: the Homebrew formula's `service do` block still names
`opt_bin/"beckon"`, the LaunchAgent is untouched, and — the reason this matters
most — the user's existing Accessibility grant, which is bound to the binary's
code signature, is not disturbed. Adding a `beckon-serve` binary on macOS would
have forced a re-grant, and a lost Accessibility grant is exactly how macOS
window cycling silently stopped working on macmini in the 2026-08-10 trial: the
symptom is not an error, it is beckon quietly doing half its job.

There is no `--no-tray` flag. The growth rule in `CLAUDE.md` ("new capabilities
are flags on an existing verb") permits one, but a UIElement process with a
menu bar item is what `serve` *is* on macOS; a flag to remove it would be a
flag to make the daemon invisible again.

## 3. The menu

```
beckon — 18 shortcuts registered      (disabled; the status line)
──────────────
Reload now
──────────────
Pause hotkeys                          (checkable)
──────────────
Quit
```

Four rows against Windows' seven. Each omission is a decision, not an
oversight:

- **`Settings…` is omitted** because the window does not exist on macOS yet.
  A row that opens nothing is worse than no row. It returns with subsystem 2.
- **`Open log` is omitted** because `--log` is `#[cfg(target_os = "windows")]`
  (`crates/beckon-cli/src/lib.rs:130`), so `ServeState::log` is structurally
  always `None` on macOS and the row could only ever be greyed out. beckon does
  not own its log on macOS — under `brew services` launchd writes
  `$(brew --prefix)/var/log/beckon.log` via the formula's `log_path`, and under
  nix it is a different agent with a different path. Teaching beckon to
  interrogate launchd for `StandardErrorPath` was considered and rejected as
  scope creep against a problem launchd already solves correctly.
- **`Start with Windows` has no counterpart.** `ServeState::autostart` is
  documented as always `None` on macOS (`serve.rs:67`), and login lifecycle is
  `brew services start beckon`'s job. beckon must not write a competing
  LaunchAgent behind brew's back.

The status line's text comes from `last_phrase`, the same string
`set_tray_status` already formats at `serve.rs:386/621/630` — those call sites
are unchanged.

## 4. Architecture: port the contract, not the code

`crates/beckon-windows/src/hotkey.rs:70` already defines an OS-neutral surface,
and says so:

```rust
/// One row of the tray context menu. `hotkey.rs` draws it and reports the
/// click; what any row *means* is entirely the caller's business, which is
/// why there is no enum of actions here.
pub struct MenuEntry { pub id: u32, pub label: String,
                       pub checked: Option<bool>, pub enabled: bool }
```

A new `crates/beckon-macos/src/tray.rs` implements the same four items —
`MenuEntry`, `MenuEntry::separator()`, `set_menu(build, on_click)`,
`set_status(text)` — plus `MENU_ID_DOUBLE_CLICK` for parity even though nothing
consumes it this round.

The consequence is the point of the whole design: **`build_entries`
(`serve.rs:680`) loses its `#[cfg(target_os = "windows")]` and becomes shared.**
Menu composition, the pause interlock, the status phrasing and the
`autostart.is_some()` row-visibility rule are then one implementation tested by
all three CI jobs, and the macOS-specific work is a view — roughly 400–600
lines of `objc2` against `NSStatusBar` / `NSStatusItem` / `NSMenu`, with an
`objc2::define_class!` object holding the target-action that routes a click
back to `on_click`.

Bindings verified present in the pinned crates on 2026-08-12:
`NSStatusBar::statusItemWithLength`, `NSStatusItem::{button, setMenu, setLength}`,
plus `NSMenu`, `NSMenuItem`, `NSImage`, `NSApplication` — all cargo features of
`objc2-app-kit` 0.3.2 — and `objc2::define_class!` in `objc2` 0.6.4. The
features currently enabled are only `NSWorkspace` and `NSRunningApplication`, so
`crates/beckon-macos/Cargo.toml` grows a feature list. No new dependency.

### Main thread is a type, not a convention

`NSStatusItem::button` and `NSStatusItem::menu` both take a
`MainThreadMarker`. objc2 enforces the main-thread requirement in the type
system, so a violation is a compile error rather than a runtime surprise.

For this round the requirement is already satisfied: the only
`std::thread::spawn` in `serve.rs` is `spawn_catalog_scan`, which is
Windows-gated and belongs to the settings window, and all three
`set_tray_status` call sites run on the run-loop thread.

**But the API must not be shaped around that.** The settings window (subsystem
2) brings a catalog worker that has to hand results back to AppKit, and an API
that demanded a `MainThreadMarker` *from its caller* would have to change then,
at every call site. So the marker stays out of the public signatures:
`set_menu` and `set_status` take plain arguments and acquire the marker
themselves.

What they do when it is absent is deliberately **not** a main-queue hop.
`set_menu` returns `Err` and `set_status` logs under `verbose()` and returns.
Building a `dispatch_async` trampoline now would be writing an untested path
for a caller that does not exist — the hop lands with subsystem 2, and because
the marker was never in the signature, it lands without touching a single call
site. That is the whole point of keeping it out.

`set_status` is also not the primary surface. The same string is the menu's
first row, which is where it is actually readable; the tooltip is the
redundant copy.

## 5. THE OPEN MEASUREMENT — which run loop

`hotkey::run_forever` currently calls Carbon's `RunApplicationEventLoop()`,
which pairs with Carbon's `RegisterEventHotKey`. `NSStatusItem` is Cocoa and
needs `NSApplication`. Whether a status item is *drawn and interactive* under
the Carbon loop is not documented anywhere trustworthy and must be measured.

`crates/beckon-macos/examples/tray_probe.rs`, driven by
`testing/macos_tray_probe.sh`, answers it. The probe drives **the real
`tray.rs`**, not a mock, so one run measures the loop question and exercises
the implementation at the same time. It enters each loop in turn and the
script screenshots the menu bar each time, with a baseline frame as the
control.

Both halves refuse rather than mislead when the bootstrap namespace is not
`Aqua` — see §6 for why that guard exists and what it is protecting against.

| Observation | Consequence |
|---|---|
| `BKNPROBE` visible under **both** loops | Best case. `run_forever` is untouched, hotkeys are not put at risk at all, and §4 ships as written. |
| Visible under **`[NSApp run]` only** | Swap the loop — and then a **second, mandatory** measurement: does `RegisterEventHotKey` still deliver under `[NSApp run]`? A silent regression here breaks the only feature `serve` has. |
| Visible under **neither**, clock visible | §4 is dead. Fall back to the two-process design in §11. |
| Clock not visible either | Nothing was measured. Re-run with Screen Recording granted. |

Do not begin the view code before this reads out. The choice is cheap now and
expensive after 500 lines are written against the wrong loop.

## 6. What was measured on 2026-08-12, and what it invalidates

The probe could not be run from the session that wrote this spec, and the
reason is itself a finding worth keeping.

An SSH shell on macOS — which is what a Claude Code session on macmini is —
sits in the **Background** bootstrap namespace, not Aqua (`launchctl
managername` → `Background`). In that namespace:

- `screencapture` fails outright: `could not create image from display`,
  exit 1, no file written. It is not a blank frame; it never starts.
- **AppKit reports success and renders nothing.**
  `TransformProcessType` returned `OSStatus 0` and `statusItemWithLength`
  returned a live `NSStatusItem` with a non-nil `button`, on a machine where
  this process has no window server connection at all.
- `launchctl asuser $(id -u)` cannot be used to hop into the GUI session —
  `Operation not permitted`, it needs root.

**Therefore: a non-nil `NSStatusItem` is not evidence that the icon appeared,
and no probe in this project may treat it as such.** This is the same shape as
the hazard already recorded at `hotkey.rs:147` — `RegisterEventHotKey` returns
`noErr` under launchd while never delivering a press — and it is the macOS twin
of the a14 session-0 trap in `CLAUDE.md`. Every menu bar measurement must run
from Terminal.app on the machine and must carry a positive control in the same
frame.

Incidental, but it will bite the implementation: `RunApplicationEventLoop` has
been removed from the public Carbon headers on current SDKs. It is still
exported by HIToolbox, so it must be declared by hand — which is exactly why
`hotkey.rs:91` already does.

## 7. Icon

`assets/beckon.ico` cannot be reused. A menu bar item wants a **template
image**: monochrome plus alpha, `setTemplate: YES`, ~18×18 pt at @1x and @2x,
so the system tints it for light mode, dark mode, and the highlighted state.
Shipping a colour bitmap produces an item that looks wrong in half of macOS's
appearances and inverts badly when the menu is open.

The asset is a small piece of design work, not code, so it must not block the
mechanism. `tray.rs` therefore sets `button.title` to a short text label when
no image is supplied, and takes the image when one exists — the probe already
demonstrates the text path works. A derived template asset lands as its own
step.

## 8. Errors

The menu bar item is a reporting surface, and the failure that matters is it
not appearing. `set_menu` returns a `Result`; `cmd_serve_app` logs the failure
to stderr and **continues serving**. This mirrors
`BrokenConfig::ServeAnyway` on Windows: hotkeys are the feature, the icon is
the control surface, and losing the control surface must not take the feature
with it. It must not become a modal dialog — on Windows that was measured to
strand the user with no tray icon and no way to reach the one window built for
the situation (`CLAUDE.md`, commit `4f82b94`).

## 9. Testing

Unit tests: `build_entries` becomes cross-platform, so the existing Windows
tests around it (`serve.rs:1770-1863` — the `Start with Windows` row appearing
exactly once, the `Settings...` label) start running on macOS and Linux too.
The macOS-specific expectations — four rows, no `Open log`, no autostart row —
are added there, in the crate all three CI jobs compile.

Everything else needs hardware and a control:

| Claim | How it is shown | Control that proves the test is not blind |
|---|---|---|
| The item appears | screenshot of the menu bar | system clock legible in the same frame |
| The menu opens and dispatches | click each row, observe the effect | a row whose effect is externally visible (`Reload now` → log line) |
| Hotkeys still fire | press a bound chord with `serve` up | the same chord with `serve` stopped does nothing |
| `Pause` actually pauses | press a bound chord while paused | the same chord un-paused works |

The last two are the ones the Windows work found were never checked, and they
are listed here so the same gap is not reproduced.

## 10. Out of scope this round

- **Settings window.** Its model is already OS-neutral —
  `beckon_core::settings` is 3169 lines of `Model` / `probe_plan` /
  `control_state` / `row_condition` that the Windows window only draws — so the
  macOS work is a view, not a rewrite. Separate spec.
- **Caps Lock and chord capture.** Needs a `CGEventTap` in place of
  `WH_KEYBOARD_LL`, which means Input Monitoring *and* Accessibility, both
  bound to code signature. `beckon_core::caps` and `beckon_core::capture`
  (1223 + 1268 lines) already hold the decisions. Separate spec, and the
  riskiest of the three.
- **A `.app` bundle.** Not needed: `TransformProcessType` is the programmatic
  equivalent of `LSUIElement`, and bundling would change the TCC identity that
  the user's Accessibility grant is pinned to.

## 11. Rejected alternatives

**`tray-icon` + `muda` crates.** Roughly 100 lines instead of 500. Rejected
because it does not remove the risk it appears to remove: those crates also
require `NSApplication` to be running, so §5 is unanswered either way — only
now behind an abstraction that cannot be edited when it misbehaves. It also
brings a channel-poll event model that does not match the existing callback
shape, and a dependency tree into a crate that currently has five deps.

**Two processes** — parent keeps the Carbon loop and hotkeys, child runs
`NSApplication` and owns the status item, talking over a Unix socket. This is
the fallback if §5 rules out a single loop, and only then. It is genuinely
worse: two lifecycles, an IPC protocol, and a failure mode where the child dies
and the user sees "beckon is gone" while every hotkey still works.

**A separate `beckon-serve` binary for symmetry with Windows.** Costs a
Homebrew formula change, a `service do` change, and a forced Accessibility
re-grant, to buy naming symmetry with a platform whose constraint does not
exist here. See §2.
