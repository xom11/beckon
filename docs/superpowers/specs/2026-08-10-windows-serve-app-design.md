# `beckon-serve.exe` — `serve` as a Windows app

Status: design, approved 2026-08-10. Target release: 0.7.0.

## Goal

Make resident mode on Windows installable and controllable by someone who
does not want to read a README:

- **Double-click and it works.** No Scheduled Task XML, no PowerShell string
  replacement, no SID lookup. The one-time setup becomes a tick box in a tray
  menu.
- **No console window, ever.** Not the ~60 ms flash at start, and not the
  Windows Terminal tab that `CTRL_CLOSE_EVENT`s the daemon dead when the user
  closes it.
- **Controllable while running.** Reload, pause, open the config, open the
  log, quit — without Task Manager.

`beckon.exe serve <CONFIG>` is unchanged. Every word of its current
behaviour, flags and docs stays valid. This adds a second front door; it
does not move the existing one.

## Why now

`docs/superpowers/specs/2026-08-09-serve-background-design.md` listed *"a
GUI-subsystem Windows binary"* under **Non-goals**, "held in reserve as the
escalation path if the console flash measures badly". It was then measured
(CLAUDE.md, *Phase 3 Windows notes*): on Windows 11 ARM64 build 26200,
`serve --log` shows a console window at ~150 ms that is gone by ~210 ms —
and where Windows Terminal is the default terminal, that console arrives as
a WT **tab**, so closing the tab sends `CTRL_CLOSE_EVENT` and kills the
daemon.

This spec is that escalation being taken. The reserve clause is spent.

## Non-goals

- **A settings window.** Editing shortcuts through a GUI — picking apps from
  a list, capturing a chord — is a separate project with its own spec, and it
  is deliberately deferred until this one has been lived with. See
  *Deferred: the settings window* below for the finding that makes it more
  expensive than it looks.
- **An installer (MSI/NSIS), code signing, or auto-update.** Scoop is the
  install path. Signing buys only the SmartScreen prompt, and shipping a
  browser-downloaded MSI would *introduce* Mark-of-the-Web friction that
  Scoop does not currently have.
- **Replacing the Scheduled Task.** `examples/windows/serve/beckon-serve.xml`
  stays, documented as the advanced path for anyone who genuinely wants
  `RestartOnFailure`.
- **Status-dependent icon art.** One icon; the tooltip carries the state.
- **Anything on macOS or Linux.** macOS has `brew services` and a
  LaunchAgent; Linux is compositor-bound by design.
- **Multiple configs / multiple instances.** One `serve` per config path,
  still enforced by the existing lock file.

## Constraints

- **A tray app must run in an interactive session.** Session 0 has no
  taskbar and no keyboard input. `hotkey::install` already warns about this;
  nothing here weakens it.
- **`RegisterHotKey` needs a desktop.** Unchanged, and the reason `serve` is
  a foreground process rather than a Windows service.
- **The subsystem is a PE header bit, not a runtime switch.** One binary
  cannot be both. Hence a second binary rather than a flag.
- **GUI-subsystem processes have no usable stderr.** Every diagnostic beckon
  writes today goes to `eprintln!`. This is the single largest source of risk
  in this design; §2 addresses it.

## Architecture

### 1. `beckon-cli` gains a library target

`serve.rs`, `notify.rs`, `lockfile.rs` and `pick_backend` / `is_expected`
are private to the `beckon-cli` **binary** crate today, so a second binary in
the same package cannot reach them.

```
crates/beckon-cli/src/lib.rs               # today's main.rs body
crates/beckon-cli/src/main.rs              # ~15-line shim -> beckon_cli::cli_main()
crates/beckon-cli/src/bin/beckon-serve.rs  # GUI-subsystem entry point
```

This is a mechanical move of roughly 600 lines. It changes no behaviour and
no shipped artifact for `beckon.exe`. The 0.6.0 subcommand tests
(`parse_checked` and friends) move with it and become tests of a real API
rather than of a binary crate's interior.

**Rejected alternatives.**

- `#[path = "../serve.rs"] mod serve;` in the second bin. Compiles the
  shared modules twice, and `serve.rs` still refers to `crate::notify` and
  `crate::pick_backend`, so those must be extracted regardless — it is the
  same refactor with worse ergonomics.
- A new `beckon-serve-app` crate. `serve.rs` depends on `notify` and
  `pick_backend`; extracting it drags that whole cluster along.
- Flipping `beckon.exe` itself to `windows_subsystem = "windows"` and calling
  `AttachConsole(ATTACH_PARENT_PROCESS)` to recover output. This works for
  redirection (`beckon list > f.txt`) but not for interactive use: the shell
  returns its prompt before the process writes, so `beckon doctor` and
  `beckon resolve` print into the middle of the next prompt. CLAUDE.md
  already rules the whole-binary switch out; this is the measurable reason
  the ruling stands even though `AttachConsole` exists.

**Cross-platform wart, accepted.** Cargo cannot gate a `[[bin]]` on
`target_os`. On non-Windows the file compiles to a `main` that prints
`beckon-serve is Windows-only` and exits 1, so `cargo build --workspace`
needs no special casing. The release workflow must **exclude
`beckon-serve`** from the Linux and macOS artifacts.

**Its argument surface**, both optional:

```
beckon-serve.exe [CONFIG] [--log PATH]
```

Same two operands `beckon serve` takes, with defaults (§7, §2) so that the
zero-argument invocation — a double-click, or a bare Run-key value — is the
normal one. Parsed with the same clap derive, so `--log` remains scoped to
this binary the same structural way it is scoped to the `Serve` variant
today.

### 2. stderr, and the assumption that dies here

`beckon-serve.exe` calls `logfile::redirect_to_log` as the **first statement
of `main`**, before anything can print, defaulting to
`%LOCALAPPDATA%\beckon\serve.log`. `--log <PATH>` still overrides it.

The module doc of `logfile.rs` currently says:

> **Everything fallible runs before `FreeConsole`**, because `main` reports
> errors with `eprintln!` — an `Err` returned from after the detach turns
> `exit(1)` into a silent panic.

**That guarantee does not exist in a GUI-subsystem process**: there is no
console to report through at any point, not even before the detach. So
`beckon-serve.exe` owns its own failure path — if `redirect_to_log` returns
`Err`, it shows a `MessageBoxW` and exits 1. This is new code, not a
copy-paste of the CLI path, and it is why the redirect must come first:
after it, every existing `eprintln!` in the process lands in the log, and
the question of what std does with a NULL stderr handle never arises.

`FreeConsole()` inside `redirect_to_log` will fail (there is no console to
free). It is already `let _ =`, so this is harmless — but the module doc
should say so rather than leave a reader to wonder.

**`CREATE_NO_WINDOW` on the toast spawn stays required.** A console-subsystem
child (PowerShell) of a console-less parent is given a brand-new *visible*
console by `CreateProcess`. That is true of a GUI-subsystem parent for
exactly the same reason it is true after `FreeConsole`. The flag is invisible
from the call site; it must not be "cleaned up".

### 3. Tray: from indicator to control surface

`tray_add` gains `NIF_MESSAGE` and a `uCallbackMessage` of `WM_APP + 1`.
`wndproc` gains one branch: on `WM_RBUTTONUP` / `WM_CONTEXTMENU`, build and
show the popup menu; on `WM_LBUTTONDBLCLK`, open the config.

Two Win32 requirements, documented behaviour rather than folklore
(Microsoft KB135788): call `SetForegroundWindow(hwnd)` **before**
`TrackPopupMenu`, and `PostMessage(hwnd, WM_NULL, 0, 0)` **after**. Without
them the menu does not dismiss when the user clicks away.

```
beckon — 5/5 shortcuts          (disabled, read-only)
────────────────────────────
Edit shortcuts…                 ShellExecuteW "open" on the config
Reload now
Open log
────────────────────────────
☐ Pause hotkeys
☑ Start with Windows
────────────────────────────
Quit
```

- The header line is `registration_phrase(ok, total)` — the function that
  already exists and is already unit-tested, reused verbatim.
- **Quit needs no new logic.** `run_forever`'s `WM_QUIT` branch already
  unregisters every hotkey, sends `NIM_DELETE` and `exit(0)`. The menu item
  is `PostQuitMessage(0)`.
- The same string goes into the tooltip via `NIM_MODIFY` after every
  registration pass. `szTip` is 128 UTF-16 units; the phrase fits with room
  to spare.

This is what retires the README's caveat that the icon is a one-directional
signal ("icon absent means dead **or** not ready yet"). Hovering now
distinguishes *alive with 5 keys*, *alive with 2 keys failing*, *alive but
paused*, and *no config yet*.

**`TrackPopupMenu` is itself a modal message pump.** A hotkey pressed while
the menu is open therefore arrives through `wndproc`, not through
`run_forever` — which is precisely the case `hotkey.rs`'s module doc was
written for, and `HOTKEY_PENDING` already handles. No new machinery.

### 4. The new boundary inside `hotkey.rs`

`hotkey.rs` must not learn what a config path or a registry key is. It owns
the menu *chrome*; `serve.rs` owns what each item *means*. The contract:

```rust
pub struct MenuEntry {
    pub id: u32,
    pub label: String,
    pub checked: Option<bool>,   // None = not a checkable item
    pub enabled: bool,
}

/// `build` is called each time the menu opens, so check states are live.
pub fn set_menu(build: Box<dyn Fn() -> Vec<MenuEntry>>, on_click: Box<dyn FnMut(u32)>);

/// Update the tray tooltip (NIM_MODIFY).
pub fn set_status(text: &str);
```

A separator is `MenuEntry` with an empty label.

`dispatch_menu` copies `dispatch_tick`'s take-then-run discipline exactly:
take the callback out of its `thread_local` slot before invoking it, so a
re-entrant delivery through a nested pump cannot double-borrow the `RefCell`
and panic across the `extern "system"` boundary.

### 5. Pause

`Pause hotkeys` calls `mgr.unregister_all()`; unpausing calls
`register_all()`. The flag lives in `ServeState` beside `shortcuts` and
`config`.

While paused, a config reload updates the shortcut table but does **not**
register — otherwise a file save would silently un-pause the user's keys.
The tooltip reads `beckon — paused (5 shortcuts)`.

`unregister_all` already drains the OS message queue and `HOTKEY_PENDING` of
stale presses, for the reason documented there (an id is resolved against
whatever table is live when it finally runs). Pause inherits that correctness
for free.

### 6. Start with Windows — registry Run key

Value `beckon` under `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`,
set to the quoted executable path — **plus whichever of `CONFIG` and `--log`
differ from their defaults**, quoted, so that a user who ticks the box while
running against a non-default config gets that same config at logon rather
than silently reverting to `~/.config/beckon/apps.toml`. Reading the value
back drives the ✓ (present = ticked; its contents are not parsed).
Unticking deletes the value. New crate feature `Win32_System_Registry`; no
new dependency.

Chosen over a Scheduled Task created through COM `ITaskService`, and over a
Startup-folder `.lnk`, because:

- It appears in **Task Manager → Startup apps** and **Settings → Apps →
  Startup**, so the user can disable it the way they disable every other app.
  A Scheduled Task appears in neither.
- ~20 lines against ~150–250 lines of COM.
- The `RestartOnFailure` it gives up was guarding mostly against a cause this
  design removes outright: the Windows Terminal tab that sends
  `CTRL_CLOSE_EVENT`. A GUI-subsystem process has no console, so no tab, so
  no such event. Users who still want supervision keep the Scheduled Task
  XML.

**The Scoop path hazard.** Scoop installs to
`…\scoop\apps\beckon\<version>\` and exposes a `current` junction. If
`std::env::current_exe()` reports the versioned path, the Run value points at
a directory that the next `scoop update` deletes — and because the entry no
longer launches, it never gets a chance to repair itself. Mitigation: before
writing, if the path matches `\scoop\apps\<name>\<segment>\` and `<segment>`
is not `current`, substitute `current`. Pure string work, unit-testable on
any OS.

> **To verify on hardware before relying on it:** whether `current_exe()`
> actually returns the versioned path or the junction path when launched
> through the junction. The mitigation is correct either way (it is a no-op
> when the path already says `current`), but the *hazard* is asserted from
> how Scoop lays out its store, not from measurement.

### 7. First run

With no positional argument the config path defaults to
`%USERPROFILE%\.config\beckon\apps.toml` — matching what the README already
tells Windows users to create, and matching macOS, over the more native
`%APPDATA%\beckon\`. Consistency across the three OSes beats platform idiom
here, because the shortcuts file is explicitly designed to validate on every
platform.

If the file does not exist:

1. Create the parent directory.
2. Write a commented starter template with two or three example bindings.
3. `ShellExecuteW "open"` it, so the user's default editor comes up.
4. Continue starting normally — tray icon appears, tooltip reads
   `beckon — 0 shortcuts`, the watcher is live, so saving the file applies it
   immediately.

If step 1 or 2 fails, `MessageBoxW` and exit 1.

**The template must pass `parse_shortcuts`.** Pinned by a unit test that runs
on every platform, CI Linux included — a starter file that fails validation
would make the very first impression a failure toast.

### 8. Already-running

`acquire_lock(config)` refuses a second instance today, prints to stderr and
exits non-zero. With a Start Menu shortcut, double-launching becomes a normal
user action rather than an operator error, and with no console the refusal is
invisible.

`beckon-serve.exe` therefore shows a `MessageBoxW` ("beckon serve is already
running") and exits 1 — the same code the CLI path returns, so the two front
doors do not disagree about what happened. It stays an `is_expected` outcome:
no desktop notification, and the line still goes to the log.

### 9. Icon

`IDI_APPLICATION` reads as unfinished in the Start Menu, in Alt-Tab and in
the tray. `beckon-serve.exe` embeds a real `.ico` through the
`embed-resource` build-dependency, which drives the `rc.exe` already present
in the Windows SDK the project requires.

**The asset does not exist in the repo yet** and is a blocker for the polish,
not for the function. One icon only.

### 10. Packaging

- Scoop manifest (`packaging/scoop/beckon.json.template`): `"bin"` becomes
  `["beckon.exe", "beckon-serve.exe"]`, plus a `"shortcuts"` entry so the app
  gets a Start Menu item.
- The release workflow already builds the whole workspace; it needs the new
  file added to the two Windows artifacts and **kept out of** the Linux and
  macOS ones (§1).
- Homebrew formula: untouched.

## Data flow

```
beckon-serve.exe (no args)
  └─ redirect_to_log(default or --log)      MessageBox+exit on failure
  └─ resolve config path (default or positional)
       └─ missing? create dir + template + ShellExecute "open"
  └─ acquire_lock(config)                   MessageBox+exit if held
  └─ HotkeyManager::install(on_hotkey)      hidden window + tray icon
  └─ hotkey::set_menu(build, on_click)
  └─ register_all() ──► set_status(registration_phrase(..))
  └─ watch_config()  ──► add_tick(1s) ──► reload() ──► set_status(..)
  └─ run_forever()
        WM_HOTKEY  ──► on_hotkey ──► backend.beckon()
        WM_APP+1   ──► TrackPopupMenu ──► WM_COMMAND ──► on_click(id)
        WM_QUIT    ──► unregister all + NIM_DELETE + exit(0)
```

`on_click` dispatch, all in `serve.rs`:

| Item | Action |
|---|---|
| Edit shortcuts | `ShellExecuteW "open"` on the config path |
| Reload now | `reload(&state, &mgr)` — the existing function |
| Open log | `ShellExecuteW "open"` on the log path |
| Pause | toggle `state.paused`, `unregister_all` / `register_all`, `set_status` |
| Start with Windows | write or delete the Run value, report failure by toast |
| Quit | `PostQuitMessage(0)` |

## Error handling

| Failure | Response |
|---|---|
| Log file cannot be opened | `MessageBoxW`, exit 1 (no other channel exists) |
| Config directory/template cannot be written | `MessageBoxW`, exit 1 |
| Another instance holds the lock | `MessageBoxW`, exit 1; logged; no toast (`is_expected`) |
| Config parse error on reload | Unchanged — keep current keys, log, one throttled toast |
| Some hotkeys fail to register | Unchanged — `failure_toast`, plus the count now visible in the tooltip |
| `Shell_NotifyIconW` fails | Unchanged — logged, non-fatal; hotkeys are unaffected |
| Registry write for autostart fails | Toast (`Cause::HumanAction` — the user just clicked it), tick stays off |
| Session 0 | Existing warning, now also unreachable in practice via the Run key, which is per-user and interactive |

## Testing

**Unit, on every platform including CI Linux — this is most of the new
logic:**

- `run_key_command_line()` — quoting; the Scoop `current` substitution,
  including the no-op case and paths that merely contain the word `scoop`;
  and that non-default `CONFIG` / `--log` are carried through while defaults
  are omitted.
- `default_config_path()` and `default_log_path()`.
- `starter_template()` round-trips through `parse_shortcuts` with the
  expected number of bindings.
- Menu construction: given a `ServeState`, `build()` returns the right
  labels, check states and enabled flags (paused vs not, autostart on vs
  off, zero shortcuts vs many).

**Manual, on a14 (Windows 11 ARM64):**

- No console window at any point during startup — the control being the
  measured ~150–210 ms flash that `serve --log` still shows.
- Tray menu opens, every item does what it says, ✓ states reflect reality.
- Tick "Start with Windows", reboot, confirm it comes back; untick, reboot,
  confirm it does not.
- A hotkey pressed while the menu is open still fires (the `HOTKEY_PENDING`
  path).
- First-run flow on a machine with no `apps.toml`.

**Known testing hazard** (recorded in memory as *a14 Windows remote
testing*): SSH into a14 lands in **session 0**, where a tray app has no
taskbar and hotkeys never fire. Every check above must go through a
scheduled-task probe in the interactive session, using `-EncodedCommand` to
avoid quoting damage. Testing this over a plain SSH shell will produce
confident false negatives.

## Documentation

- **README**: a Windows resident-mode section rewritten around
  `beckon-serve.exe`; the Scheduled Task demoted to "advanced". The caveat
  about the tray icon being a one-directional signal can go — the tooltip
  now answers the question.
- **CLAUDE.md**, three edits:
  1. *Out of scope → "GUI / TUI — CLI only"* must be rewritten. A tray menu
     is `serve`'s control surface, not a launcher UI. The deferred settings
     window is named there as still-open, deliberately.
  2. *Open questions → #1* asks whether the install lifecycle should be
     `serve --install` or `serve install`. It is answered by a third option:
     **neither** — a tick box in the tray, no new verb and no new flag. That
     preserves the "no new top-level verbs" growth rule by not needing it.
  3. *Phase 3 Windows notes → `--log`*: record that `beckon-serve.exe` has no
     console flash, and that `CREATE_NO_WINDOW` on the toast spawn is
     load-bearing for the GUI binary too.
- **`examples/windows/serve/README.md`**: keep the XML, reframe it as the
  supervised path.

## Deferred: the settings window

Editing shortcuts through a GUI is the natural next request, and it is worth
recording why it is not in this spec.

The only thing a settings window offers over Notepad is **capturing a chord
by pressing it**. That is the expensive part:

- The stock Windows control for this, `msctls_hotkey32`, does not capture the
  Windows key — and beckon's whole default scheme is `ctrl+super+alt+…`.
- `Win+T` and friends are shell hotkeys; Explorer consumes them before a
  normal window sees them.

So the feature that justifies the window is also the one that needs measuring
on real hardware before any toolkit is chosen. Do that measurement first; if
a chord like `ctrl+win+alt+t` cannot be captured in a normal window, a
settings GUI is worth much less than it appears and the answer may be a
smarter template plus `beckon check`.

The coupling is already solved either way: `serve` watches the config file,
so a settings app only ever needs to **write the TOML**. No IPC, no named
pipe, no protocol.
