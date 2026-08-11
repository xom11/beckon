# beckon-serve settings window + Caps Lock as the beckon key (Windows)

Date: 2026-08-11
Status: design approved, not built
Supersedes: the *Deferred: the settings window* section of
`2026-08-10-windows-serve-app-design.md`

## Motivation

Two complaints from a non-developer using `beckon-serve.exe`:

1. **Hand-editing a TOML file is hard and unfamiliar.** There is no way to
   see what is currently bound without opening a text file. The tray menu
   answers "is beckon alive and how many keys registered", not "what are my
   keys".
2. **The default chord `ctrl+super+alt+<key>` needs a third-party remapper
   to be comfortable.** Every experienced user reaches for kanata,
   AutoHotkey or PowerToys to put that chord on Caps Lock. A beginner has
   to install and learn a second tool before beckon feels good.

Both are solved without changing what beckon *is*: the TOML file stays the
source of truth and stays hand-editable, and the Caps feature is a tick box
that writes into that same file.

**Scope: Windows only.** `beckon-serve.exe` already owns a tray icon, a
message loop and a menu. macOS `serve` runs under launchd as a UIElement
process with no UI surface at all; giving it one means building an
`NSStatusItem` and menu from scratch, which is a separate project. macOS
users keep editing the file.

## Why one spec for two features

They are independent in implementation and share exactly three things: the
config file, the tray entry point, and `ServeState`. Splitting them would
mean designing the settings window without knowing how it must present the
Caps option, and designing the Caps option without knowing where its tick
box lives. The plan derived from this spec can still land them in two
phases — and should, since Part B has a measurement gate in front of it.

## Decisions taken (and the alternatives rejected)

| Question | Decision | Rejected |
|---|---|---|
| Platform | Windows only | macOS (no UI surface exists); both at once (scope) |
| Window scope | View **and** edit, typed text | Read-only viewer; edit with chord capture |
| App column | Combo box: free typing **plus** suggestions from the installed catalog | Text-only (beginner still guesses); dropdown-only (blocks ad-hoc apps beckon deliberately supports) |
| Caps mechanism | `WH_KEYBOARD_LL` hook | Registry `Scancode Map` (admin + reboot, 1:1 only, cannot make a hyper); Raw Input (observes but cannot suppress); leader key via `RegisterHotKey(VK_CAPITAL)` |
| Caps semantics | Caps is an **alias for `ctrl+super+alt`** | A real fifth `caps+` modifier token |
| Bare Caps tap | Configurable, default = the original Caps Lock toggle | Default Escape; no knob at all |
| Toolkit | Raw Win32, list + detail panel | Raw Win32 in-place grid editing; WebView2 |

### Why the alias, not a fifth modifier

Making `caps` a real modifier means `Combo` grows a field, `canonical()`
changes, `parse_shortcuts` changes, and `serve` must split the shortcut
table into two groups — one registered with `RegisterHotKey`, one matched by
the hook itself. The hook then has to do binding lookup and dispatch, which
is where a low-level hook gets dangerous.

The alias approach touches none of that. The hook translates Caps into the
chord that `RegisterHotKey` is already listening for, and every existing
code path runs unchanged. Consequences that matter:

- **The config file is identical on a machine with the tick and one
  without.** The same file is portable, and the starter template works the
  moment the box is ticked — no binding has to be rewritten.
- **The hook never calls `backend.beckon()`.** A `WH_KEYBOARD_LL` callback
  that exceeds `LowLevelHooksTimeout` (default 300 ms) is silently
  unhooked by Windows, with no error anywhere. `backend.beckon()` was
  measured at ~57 ms typical and ~945 ms on the miss path — inside a hook
  that is a live grenade. Under the alias design the hook does a hash lookup
  and one `SendInput`; the real work happens later on the ordinary
  `WM_HOTKEY` path, long after the hook returned. The hazard is designed
  out, not guarded against.

The cost is that the chord is fixed at `ctrl+super+alt`. Bindings using any
other chord are not reachable through Caps. Making the chord configurable
was offered and declined; it can be added later as one string key without
disturbing anything else.

## Module layout

Pure logic goes in `beckon-core` / `beckon-cli`; Win32 goes in
`beckon-windows`. This is not a style preference: `.github/workflows/ci.yml`
passes `--exclude beckon-windows` on the Linux and macOS jobs, so anything
placed in `beckon-windows` is tested by one job out of three. It is the same
reasoning that put `serve_app.rs` in `beckon-cli`.

| File | Responsibility | Tested by |
|---|---|---|
| `beckon-core/src/shortcuts.rs` | extend: `parse_config()` → `Config { shortcuts, keyboard }`; `parse_shortcuts()` becomes a thin wrapper | every job |
| `beckon-core/src/config_write.rs` *(new)* | render/edit TOML through `toml_edit`, preserving comments. Pure — no I/O | every job |
| `beckon-cli/src/settings.rs` *(new)* | window model: rows, per-row status, validation, model → TOML. No Win32 | every job |
| `beckon-cli/src/caps.rs` *(new)* | `caps::decide` — the hook's state machine, as a pure function | every job |
| `beckon-windows/src/settings_window.rs` *(new)* | Win32 only: window class, ListView, detail panel, controls, worker thread for the catalog scan | `windows-latest` + manual |
| `beckon-windows/src/caps_hook.rs` *(new)* | `SetWindowsHookExW` shim: `KBDLLHOOKSTRUCT` → `KeyEvent`, `Action` → `SendInput`. ~80 lines, no decisions | `windows-latest` + manual |
| `beckon-cli/src/serve.rs` | one menu row renamed, hook lifecycle, per-index `RegisterOutcome` | every job |
| `beckon-windows/src/hotkey.rs` | `IsDialogMessage` in `run_forever` | `windows-latest` |

New dependency: **`toml_edit = "0.22"`**, already present in `Cargo.lock` as
a transitive dependency of `toml 0.8`, so the build graph does not grow.

---

# Part A — the settings window

## A.1 Config schema

The file grows keyboard settings, written as **dotted keys**, not a
`[keyboard]` table header:

```toml
keyboard.caps = true              # absent or false = feature off
keyboard.caps_tap = "capslock"    # "capslock" | "escape" | "none"

"ctrl+super+alt+t" = "Terminal"
"ctrl+super+alt+e" = "File Explorer"
```

**Why dotted keys and not `[keyboard]`.** TOML places every bare key-value
pair that follows a table header *inside that table*. A user who appends a
new shortcut to the bottom of a file that ends with a `[keyboard]` section
silently creates `keyboard."ctrl+super+alt+x"` — a shortcut that never
registers and produces no error. Dotted keys introduce no header, so
appending is always safe, which is the property that matters for a file
this spec explicitly keeps hand-editable.

A `[keyboard]` header written by hand is still parsed correctly. The parser
adds a guard for the failure above: any key **inside** the `keyboard` table
that parses as a valid `Combo` is a hard error naming that key and telling
the user to move it above the header.

## A.2 Parser changes (`beckon-core/src/shortcuts.rs`)

```rust
pub struct KeyboardConfig { pub caps: bool, pub caps_tap: CapsTap }
pub enum CapsTap { CapsLock, Escape, None }   // default: CapsLock
pub struct Config { pub shortcuts: Vec<Shortcut>, pub keyboard: KeyboardConfig }

pub fn parse_config(text: &str) -> Result<Config, String>;
pub fn parse_shortcuts(text: &str) -> Result<Vec<Shortcut>, String>;  // wrapper
```

Rules:

- Top-level key `keyboard` whose value is a Table → keyboard settings.
- Every other top-level key → must be a `Combo`, exactly as today.
- Unknown key inside `keyboard` → error. A typo like `caps_tab` that
  silently does nothing is worse than a refusal.
- `caps` must be a boolean; `caps_tap` must be one of the three strings.
- Absent `keyboard` → `KeyboardConfig::default()`. Every existing config
  file keeps working untouched.

`beckon check` validates both halves for free, because it already calls the
parser.

**The parser lives in `beckon-core` and is therefore cross-platform, on
purpose.** A file carrying `keyboard.caps = true` parses fine on macOS and
Linux, where the setting is simply ignored — `serve` only reads it on
Windows, and Linux has no `serve` at all. `beckon check` accepts it
everywhere rather than warning: the whole point of Name-based ids is that
one config file travels between machines, and a per-OS setting that fails
validation on the other OS would break that.

## A.3 Writing the file (`beckon-core/src/config_write.rs`, new)

`toml::Table` loses every comment on re-serialization. This file is one the
user is invited to edit by hand, so discarding their comments is not
acceptable. Writing goes through **`toml_edit`**, which edits the document
in place and preserves comments, spacing and key order.

`toml_edit = "0.22"` is already in `Cargo.lock` as a transitive dependency
of `toml 0.8`, so this adds nothing to the build graph.

**Atomic write**: render to `<config>.tmp` in the same directory, then
`fs::rename` over the original. Two reasons, both load-bearing:

1. A crash or a full disk mid-write must not destroy a working config.
2. `watch_config` watches the **parent directory** by file name precisely
   because "vim/sed replace the file by rename, which kills an inode-level
   watch silently" (its own comment). A rename is therefore the write shape
   the watcher was built for.

## A.4 Window model (`beckon-cli/src/settings.rs`, new — pure)

```rust
pub struct Row { pub combo: String, pub app: String }   // exactly as typed
pub struct RowStatus {
    pub parse: Result<Combo, String>,
    pub registered: Option<Result<(), String>>,   // from ServeState
    pub resolves: Option<bool>,                   // None = catalog unknown
}
pub struct Model { pub rows: Vec<Row>, pub keyboard: KeyboardConfig, pub dirty: bool }
```

`registered` requires a change to `RegisterOutcome`, which today keeps only
`ok: usize` and `failed: Vec<String>` of canonical combo strings. The window
needs the result **per index** so the status column marks the right row.

**Validation runs through the real parser.** The model renders its TOML
string and feeds it to `parse_config`; it writes only on `Ok`. This makes
"what the UI writes is what beckon reads" true by construction, rather than
maintaining a second set of validation rules that can drift from the first.

## A.5 Layout

```
beckon                                             [_][□][X]
┌─ Shortcuts ────────────┬────────────────────────────────┐
│ ✓ ctrl+super+alt+t     │ Shortcut                       │
│   Terminal             │ [ctrl+super+alt+t            ] │
│                        │                                │
│ ✓ ctrl+super+alt+e     │ App                            │
│   File Explorer        │ [Terminal                   ▾] │
│                        │                                │
│ ✗ ctrl+super+alt+c     │ ✓ registered                   │
│   Claude               │ ✗ no installed app matches     │
│                        │                                │
│ [+]  [−]               │                    [Apply]     │
└────────────────────────┴────────────────────────────────┘
┌─ Keyboard ───────────────────────────────────────────────┐
│ ☑ Use Caps Lock as the beckon key                        │
│   Tapping Caps alone:  (•) Caps Lock  ( ) Esc  ( ) nothing│
└──────────────────────────────────────────────────────────┘
              [Open config file]        [Close]
```

A list plus a detail panel, not an editable grid. Win32 has no editable
grid: in-place editing means manually overlaying an `EDIT` control on a
ListView subitem and hand-handling horizontal scroll, column resize, Tab to
the next cell, Esc to cancel, and the ambiguous "click outside — commit or
cancel?". That is 400–600 extra lines for the same result. The detail panel
is also closer to how Windows' own settings dialogs read.

`Open config file` keeps the Notepad path reachable, so neither editing
route is second-class.

**Button semantics, stated because "Apply" is ambiguous in Windows
dialogs.** There is exactly one write action. `Apply` validates the whole
model and writes the entire file; it is disabled unless the model is dirty
*and* valid. Editing a field, `[+]` and `[−]` mutate the in-memory model
only. `Close` with a dirty model prompts `Save changes? [Save] [Discard]
[Cancel]`; with a clean model it closes immediately. There is no
auto-save-on-blur — a config that rewrites itself while the user is still
thinking is exactly the surprise this window exists to remove.

The Keyboard group is part of the same model and the same `Apply`: ticking
the box marks the model dirty and is written by `Apply` like any other
change, so the tick and a hand-edit of `keyboard.caps` are literally the
same operation.

## A.6 Three things that must be right or they break what already works

**1. The catalog scan runs on a worker thread.** `scan_installed_apps()`
was measured at ~370–500 ms, and `run_forever`'s message loop is the same
thread that dispatches `WM_HOTKEY`. Scanning inline stalls every hotkey for
half a second each time the window opens. A worker thread does its own
`CoInitializeEx(COINIT_APARTMENTTHREADED)` (per the existing catalog-thread
rule; an MTA worker would get a marshalling proxy back to the host STA and
serialise anyway) and `PostMessage(WM_APP+2)` with the result. Until it
arrives the combo box accepts free text with no suggestions, and `resolves`
stays `None`.

**2. The window is modeless.** Hotkeys must keep firing while it is open.
This requires adding `IsDialogMessage` to `hotkey.rs::run_forever` so Tab,
Esc and arrow navigation work inside the window without swallowing
`WM_HOTKEY`.

**3. No IPC.** The window writes the file; `serve`'s existing watcher sees
the rename and the 1 s tick calls `reload()`. This is the property the
previous spec already established: "a settings app only ever needs to write
the TOML. No IPC, no named pipe, no protocol." There is deliberately **no**
shortcut path that calls `reload()` directly — the watcher would fire
anyway and reload a second time, so a direct call buys under a second of
latency at the cost of a second code path.

## A.7 External edits while the window is open

Guaranteed to happen, because this spec insists both editing routes stay
first-class.

- Window not dirty → reload it silently from disk.
- Window dirty → show a bar: `File changed on disk  [Reload] [Keep mine]`.
  beckon does not choose for the user.

## A.8 Entry point

The tray row `Edit shortcuts...` becomes `Settings...` and opens the
window. Double-clicking the tray icon opens the window too (today it opens
Notepad). No new top-level verb and no new flag, so the CLI growth rule is
untouched.

---

# Part B — Caps Lock as the beckon key

## B.1 Mechanism

```
state:  caps_held: bool
        caps_used: bool
        bound:     HashSet<VK>     // keys bound to exactly ctrl+super+alt+X
        consumed:  HashSet<VK>

event carrying our own dwExtraInfo marker  → pass through (recursion guard)

Caps ↓            → swallow; caps_held = true
<vk> ↓ while caps_held:
    vk ∈ bound, not consumed  → swallow; SendInput one burst:
                                  ctrl↓ win↓ alt↓ vk↓ vk↑ alt↑ win↑ ctrl↑
                                caps_used = true; consumed += vk
    vk ∈ bound, consumed      → swallow (auto-repeat; do nothing)
    vk ∉ bound                → pass through untouched
<vk> ↑ while vk ∈ consumed    → swallow; consumed -= vk
Caps ↑            → swallow; if !caps_used, perform caps_tap:
                      CapsLock → SendInput VK_CAPITAL ↓↑
                      Escape   → SendInput VK_ESCAPE  ↓↑
                      None     → nothing
                    reset caps_held, caps_used
```

Every injected event carries a private `dwExtraInfo` marker so the hook
ignores its own output.

**Why this shape rather than "hold the modifiers down while Caps is held".**
The naive version presses `ctrl+win+alt` on Caps-down and releases them on
Caps-up. That has two defects, and injecting the whole chord as one burst
removes both:

- *Start menu*: a bare Caps tap would press and release the Windows key
  with nothing in between, which is exactly the gesture that opens Start.
  In the burst form, `Win↓` and `Win↑` always have a real key between them
  inside a single `SendInput` call, and a bare tap never presses Win at all.
- *Stray Windows chords*: with the modifiers physically held, `Caps+<any
  key>` becomes a genuine `ctrl+win+alt+<key>` that Windows may act on.
  Restricting injection to keys in `bound` means `Caps+F5` is still `F5`.

## B.2 Where the logic lives

Pure decision function in `beckon-cli/src/caps.rs`:

```rust
pub enum Action { PassThrough, Swallow, SwallowAndInject(Vec<KeyEvent>) }
pub fn decide(ev: KeyEvent, st: &mut CapsState, bound: &BoundSet) -> Action;
```

`beckon-windows/src/caps_hook.rs` is ~80 lines of unsafe translating
`KBDLLHOOKSTRUCT` → `KeyEvent` and `Action` → `SendInput`, containing no
decisions.

This split is the existing house pattern — `algorithm::decide` on Linux,
`hyprland::decide`, and the whole reason `serve_app.rs` sits in
`beckon-cli`. CI passes `--exclude beckon-windows` on the Linux and macOS
jobs, so anything inside `beckon-windows` is tested by one job out of three.
A keyboard state machine is the last thing that should be tested by one job
out of three.

## B.3 Lifecycle

- Installed by `cmd_serve_app`, Windows only, only when
  `keyboard.caps == true`.
- `SetWindowsHookExW(WH_KEYBOARD_LL, …, 0)` needs a thread with a message
  loop; the `serve` thread is one.
- `reload()` rebuilds `bound` from the new shortcut table and installs or
  removes the hook if the flag flipped.
- `set_paused(true)` **must unhook**. Otherwise Caps stays swallowed while
  nothing works — the worst possible state.
- Quit unhooks explicitly rather than relying on process teardown.
- `SetWindowsHookExW` failure is never fatal: log, toast with
  `Cause::HumanAction` (the user just ticked the box), untick the box in the
  UI so it does not lie, and keep serving.

## B.4 Gaps that go in the README, not hidden

- **UIPI.** beckon runs at normal integrity, so the hook never sees keys
  while an elevated window has focus; Caps silently does nothing there.
  Typing `ctrl+super+alt+t` by hand **still works**, because `RegisterHotKey`
  is not subject to UIPI. There is always a working fallback and the user
  should be told what it is.
- **Conflicts with kanata / PowerToys / AutoHotkey.** If another remapper
  already consumes Caps, beckon never sees it. Detection is unreliable;
  document it rather than guess.
- **EDR / antivirus.** A low-level keyboard hook is the classic keylogger
  signature. This is a real risk for a publicly distributed binary and it is
  the reason `CLAUDE.md` recorded "no event tap, no LLHOOK" as a decision.
  That decision is being reversed knowingly, for one opt-in feature that is
  off by default.

## B.5 Measurements required on a14 before this is built

Through a scheduled task in **session 1** — SSH into a14 lands in session 0,
which can see neither the desktop nor the keyboard, and will produce
confident false negatives. Use `-EncodedCommand` to avoid quoting damage.
Recorded in memory as *a14 Windows remote testing*.

| # | Measure | If it fails |
|---|---|---|
| **1** | Does a `SendInput` chord trigger our **own** `RegisterHotKey`? | **The design collapses** — fall back to a real fifth `caps+` modifier |
| 2 | With a real key between `Win↓` and `Win↑` in one burst, does Start open? | Insert a filler key |
| 3 | Does swallowing physical Caps prevent the Caps Lock state toggling? | `caps_tap = "capslock"` inverts |
| 4 | Does an injected `VK_CAPITAL` toggle the Caps Lock state? | `caps_tap = "capslock"` is not implementable |
| 5 | Behaviour while an elevated window has focus | Confirms §B.4 instead of assuming it |
| 6 | Callback duration under load | Anywhere near 300 ms means something is wrong elsewhere |

**Measurement #1 is the gate.** It is the load-bearing assumption of the
entire alias design. It is very likely true — AutoHotkey's `Send` triggers
other applications' hotkeys by exactly this mechanism — but "very likely"
is not measured. The plan runs it **first**, before any window code, so a
failure is cheap.

---

## Error handling

| Failure | Response |
|---|---|
| Bad `caps_tap` value / unknown key under `keyboard` / non-boolean `caps` | `parse_config` errors → reload **keeps the running keys**, one throttled toast (`MachineRepeat`). `beckon check` exits non-zero. Existing path, unchanged |
| Shortcut nested under a `[keyboard]` header | Error naming the key, telling the user to move it above the header |
| Combo typed wrong in the window | Row marked red, panel shows `Combo::parse`'s message verbatim, Apply disabled, file untouched |
| Two combos identical after canonicalization | **Both** rows flagged, with the shared canonical string |
| Empty app name | Apply disabled |
| Write fails (full disk, read-only, dropped share) | `MessageBoxW` — the window has somewhere to report, unlike bare `serve`. The old file survives because the write is rename-atomic. Running shortcuts unaffected |
| Catalog scan fails or returns nothing | Empty combo box, free typing still works, the app column shows `?` and **not** `✗` — a scan that did not run cannot prove absence |
| `SetWindowsHookExW` fails | Log + toast (`HumanAction`), untick the box in the UI, keep serving |
| Windows silently unhooks on timeout | No watchdog. The callback is microseconds; building a watchdog for a hazard the design removed is unfounded code |
| Window already open | `SetForegroundWindow` on the existing one; never a second window |
| File changed on disk while the window is dirty | Warning bar, user chooses |

## Testing

**Unit, every platform** (these live in `beckon-core` / `beckon-cli`, which
all three CI jobs build and test):

- `parse_config` — no `keyboard` key → defaults; dotted keys; a hand-written
  `[keyboard]` header; a shortcut nested under `keyboard` → error naming it;
  all three valid `caps_tap` values and one invalid; non-boolean `caps`.
- `config_write` — comments survive; value edited in place; key added; key
  removed.
- `caps::decide` — the full state machine: bare tap under each of the three
  `caps_tap` settings; Caps + bound key; Caps + unbound key; auto-repeat;
  key-up after a swallowed key-down; an event carrying our own marker.
- **Round trip, the load-bearing test**: every valid `Model` → TOML →
  `parse_config` → the same `Model` back. This is the structural guarantee
  behind "the UI maps 1:1 to the config file", instead of hoping the two
  sides agree.

**Unit, every platform, for the window's own drawing** — the projection
`Model → Vec<ControlState>` is pure and lives in `settings.rs`, so it is
tested everywhere: list row labels and their ✓/✗ marks, tick box state,
radio selection, and `Apply` enabled only when dirty **and** valid. This
mirrors `MenuModel` / `build_entries`, which already snapshot `ServeState`
into a pure structure "so the drawing is a pure function and can be tested
without a tray, a message loop or a registry". Only the `HWND` plumbing that
consumes `ControlState` is Windows-only, and it holds no decisions.

**Manual on a14, session 1, via scheduled task** — the six measurements in
§B.5, plus:

- Hotkeys still fire while the window is open (the entire reason it is
  modeless).
- Tab reaches every control; Esc closes.
- Apply → watcher reload within 1 s → status column updates.
- Edit with Notepad while the window is open, in both the dirty and
  not-dirty branches.
- Tick Caps → `Caps+T` focuses Terminal. Untick → Caps Lock behaves
  normally again.
- 150 % display scaling. The previous spec lists high-DPI menu placement as
  still unverified; this window is the reason to finally check it.

**Known constraint**: a14 cannot be rebooted unattended into a signed-in
state (`AutoAdminLogon` is 0, `shutdown /g` returns error 87). Anything
needing a reboot needs a person present.

## Documentation to update

- **`CLAUDE.md`**
  - *Out of scope → GUI / TUI*: the settings window is no longer deferred.
    Rewrite the exception clause; beckon-serve now has a tray menu **and** a
    settings window, and neither is a launcher.
  - *Known constraints*: add the LLHOOK entry — UIPI gap, remapper
    conflicts, EDR signature — and record that "no event tap, no LLHOOK" was
    reversed deliberately for one opt-in, default-off feature.
  - *Crate dependencies*: `toml_edit` under the resident-mode block.
  - The line "The **only** file beckon reads is the `serve` shortcuts TOML"
    stays true; it is now also the only file beckon *writes*.
- **README**: the Windows resident-mode section gains the settings window
  and the Caps tick box, including the UIPI fallback.
- **`2026-08-10-windows-serve-app-design.md`**: a pointer at the end of
  *Deferred: the settings window* to this spec, noting that its measurement
  gate is satisfied by not building chord capture.

## What this spec does not do

- No chord capture by pressing keys. Combos are typed. This is what lets the
  window skip the measurement the previous spec demanded.
- No macOS equivalent of either feature.
- No configurable Caps chord. It is fixed at `ctrl+super+alt`. Adding one
  string key later disturbs nothing.
- No launcher UI. The window never focuses or launches an app; it lists
  installed apps only to fill in a name during authoring, which is what
  `beckon search` already exists to do.
