# Windows backend — measurements and maintenance notes

Extracted from `CLAUDE.md` 2026-08-17. Read before changing anything in
`crates/beckon-windows/` or `logfile.rs`. Every "measured on a14" claim is
scoped to that machine's OS — re-run the probe rather than the reasoning before
carrying it elsewhere.

## Window enumeration

`EnumWindows` returns windows in z-order (front-to-back), which gives MRU order
for free — no state file needed (mirrors macOS `CGWindowListCopyWindowInfo`).
We filter out invisible, cloaked (via `DwmGetWindowAttribute(DWMWA_CLOAKED)`),
tool windows (`WS_EX_TOOLWINDOW`), and owner windows.

**There is a fifth filter, `window_ops::is_shell_window`** — the classes
`Progman`, `WorkerW` and `Shell_TrayWnd`, case-insensitively like every other
class compare in that file. It is **defence, not a bug fix**, and the entry
that landed with it said otherwise.

**CORRECTED 2026-08-16, same day, by measurement on a14.** The version written
a few hours earlier claimed *"The desktop passes all four filters listed above
— class `Progman`, caption `Program Manager`, visible, uncloaked, not a tool
window, unowned — so it sat in `enum_visible_windows` permanently: step 5b
always found an 'other app' to toggle to, **step 5c (minimize) was therefore
unreachable**, and `beckon list` printed a running app called *Program
Manager*."* **Every clause after "class `Progman`" is false on Windows 11 ARM64
build 26200.** Enumerated in session 1 through a scheduled task, 182 top-level
windows, printing each filter input beside the class:

```text
Progman       visible=True  cloaked=0x0 toolwin=True  owned=False  title='Program Manager'
WorkerW  x15  visible=False cloaked=0x0 toolwin=False owned=False
Shell_TrayWnd visible=True  cloaked=0x0 toolwin=True  owned=False
CoreWindow x4 visible=True  cloaked=0x2 toolwin=False owned=False  title='Settings' / 'Windows Input Experience'
```

**`Progman` carries `WS_EX_TOOLWINDOW`**, so the tool-window filter already
excluded it; not one shell-class window passes all four. The control settles it
rather than the reasoning: the pre-fix binary — scoop's `0.9.6 (2488702)`,
which predates `84ace1e` — was run through the same probe in the same session
and **its `beckon list` does not print *Program Manager* either**. So step 5c
was never unreachable and nothing was ever mis-listed here.

Keep the filter: it is cheap, it is the role `_NET_WM_WINDOW_TYPE` plays in the
X11 backend, and `WS_EX_TOOLWINDOW` on `Progman` is a shell implementation
detail no document promises. But **do not cite this entry as a bug that was
observed** — it was found by reading code, survived two adversarial verifiers
who also only read code, and was refuted the first time anyone ran it on
Windows.

**The `CoreWindow` rows above are the load-bearing measurement in this block**,
and they belong to a different question: both real ones read `cloaked=0x2`,
which is `DWM_CLOAKED_SHELL` — *the same value a window on another virtual
desktop carries*. Anything that tries to fix the other-virtual-desktop miss by
admitting `0x2` re-admits these two as focusable ghosts. That is why the branch
attempting it uses `IVirtualDesktopManager` and not a cloak-value test.

**`Windows.UI.Core.CoreWindow` was proposed alongside these and is deliberately
left out**, pinned by `an_ordinary_app_window_is_not_a_shell_window`. A UWP app
that is not hosted by `ApplicationFrameHost` presents one as its own top-level
window, so denying it makes beckon launch a second copy on every keypress — the
more expensive of the two failures, and the same shape as the Hyprland
`visible` filter.

## Focus, resolution, launch

- **Anti-focus-stealing**: Win10+ blocks `SetForegroundWindow` from background
  processes. We use the `AttachThreadInput` trick: attach our thread input to
  the foreground thread, call `SetForegroundWindow` + `BringWindowToTop`, then
  detach. This works because beckon is invoked from AHK, which holds the
  foreground.
- **Name resolution**: Start Menu `.lnk` files are parsed via COM `IShellLinkW`
  + `IPersistFile::Load`; MSIX/AppX entries and the built-in `File Explorer`
  shell app are enumerated natively from shell `AppsFolder` with friendly name
  and AUMID. Priority: display name (exact) > AUMID > exe stem/name > display
  name (substring). Use the exact name `File Explorer`, since `Explorer` may
  collide with a shortcut targeting `explorer.exe`.
- **Matching running windows**: packaged apps match by HWND
  `PKEY_AppUserModel_ID`, falling back to process AUMID from
  `GetApplicationUserModelId`; `CabinetWClass` windows map to the built-in
  `Microsoft.Windows.Explorer` AUMID; classic applications retain exe filename
  and title fallback matching. Browser PWAs sharing an exe still require
  browser-specific validation.
- **UWP/Store apps**: cataloged by friendly name and AUMID; launch uses
  `IApplicationActivationManager::ActivateApplication`.
- **Launch path**: classic shortcut entries use `ShellExecuteW` with the exe
  path and arguments extracted from the `.lnk`; MSIX/AppX entries use
  `IApplicationActivationManager::ActivateApplication` with the AUMID.
  `Microsoft.Windows.Explorer` is identified by AUMID/class but launches
  through `explorer.exe`, since activation manager rejects that built-in shell
  AppID.
- **COM initialization**: `CoInitializeEx(COINIT_APARTMENTTHREADED)` is called
  for catalog and activation threads. The call is idempotent (returns `S_FALSE`
  if already initialized on the thread).
- **Toast notifications**: when stderr is not a terminal (hotkey invocation),
  errors are surfaced via PowerShell-spawned Windows toast notifications
  (best-effort, same pattern as Linux `notify-send`).
- **Build requirements**: `aarch64-pc-windows-msvc` requires VS Build Tools
  2022 with the ARM64 component
  (`Microsoft.VisualStudio.Component.VC.Tools.ARM64`) and Windows SDK. The
  `.cargo/config.toml` is NOT committed — each machine uses its own
  MSVC/linker setup.

## Hot-path catalog cost — three layers, measured on ARM64 Windows 11

The naive `beckon <id>` cost was ~443 ms because it built the whole
installed-app catalog on every keypress. It is now ~57 ms. **Do not undo these
in the name of simplification:**

1. **Name tier resolves from filenames, no COM** — a shortcut's display name
   *is* its filename stem (`parse_lnk` never reads a name from the `.lnk`
   body), so `apps::resolve_start_menu_by_name` walks the tree and parses only
   the stem matches: one parse instead of ~120. This is the whole reason the
   hot path is fast; 186 ms → 57 ms on its own.
2. **AppsFolder stays lazy** — `apps::resolve_lazy` reaches for
   `scan_shell_apps()` only when no shortcut matches by exact display name.
   That top tier can't be beaten by a packaged app (a shortcut sorts ahead of
   an AppX entry of the same name), so skipping it cannot change the answer;
   `resolve_lazy_agrees_with_one_shot_resolve` pins the equivalence. Weaker
   tiers (AUMID, exe stem, name substring) all lose to a packaged app's exact
   name, so those still pay for the full scan.
3. **The two scans overlap on the fallback path** — by then the name tier is
   already ruled out, so `resolve_lazy` is guaranteed to call its loader and
   the AppsFolder enumeration can start eagerly. Worth ~60 ms on the miss path
   (1005 ms → 945 ms, of which ~700 ms is the error toast, not scanning).

**Do not parallelise the `.lnk` parse.** After (3), `scan_start_menu` (~150 ms)
runs alongside `scan_shell_apps` (~370 ms) and is no longer the critical path,
so a thread pool there buys zero wall-clock — while costing per-thread STA
`CoInitializeEx` (an MTA worker would get a marshalling proxy back to the host
STA and serialise anyway) plus a two-phase walk to keep the traversal-order
dedupe intact. Measured, not assumed.

Discovery commands (`list`, `installed`, `resolve`, `search`) deliberately keep
using the full `scan_installed_apps` — correctness and completeness beat
latency there.

## `--log <PATH>` — redirect stderr and detach the console

`crates/beckon-windows/src/logfile.rs`. It exists so a Scheduled Task can run
`beckon.exe` directly: Task Scheduler cannot redirect stderr, so the task used
to go through `cmd.exe` for a `2>`, which left a console window, which needed a
`wscript.exe` VBScript shim to hide. VBScript is a deprecated
feature-on-demand; both hops are gone.

- **Why no call site changed.** std's Windows stdio resolves `GetStdHandle` on
  *every* write instead of caching it, with a comment naming `SetStdHandle` as
  the reason (rust-lang/rust#40490), and std pins it with
  `library/std/tests/switch-stdout.rs`. One swap redirects every print site.
  Verified identical at the 1.75 floor and at 1.97.
- **Redirect and detach are one flag on purpose.** Detaching without
  redirecting leaves stderr pointing at a destroyed console, and `print_to`
  panics rather than returning on a write error that is not
  `ERROR_INVALID_HANDLE`. Fusing them makes that state unrepresentable.
- **Everything fallible runs before `FreeConsole`**, because `main` reports
  errors with `eprintln!` — an `Err` returned from after the detach turns
  `exit(1)` into a silent panic.
- **Append, not truncate.** `2>` truncated on every start, so
  `RestartOnFailure` destroyed the log explaining the failure it was restarting
  from.
- **Bounded at 5 MiB, one generation** (`roll_if_oversized`): past the limit
  the file becomes `<name>.1` and a fresh one starts, so the pair caps at
  10 MiB. The check runs *when the log is opened*, which is why there is no
  timer and no background thread — and the frequency lands where the growth is
  on its own: the daemon opens its log once per logon and writes a couple of
  lines per boot, while a 5-minute watchdog opens its log 288 times a day and
  is the only writer producing a line on a schedule (~55 KB/day, measured on
  a14). `beckon <id>` never reaches this code. Owning the file is *why* this is
  beckon's job: on macOS launchd owns it via `StandardErrorPath` and on Linux
  journald owns it, but Task Scheduler discards stderr entirely, so on Windows
  nobody else can.
- **`serve` log messages stay ASCII.** Windows PowerShell 5.1's `Get-Content`
  defaults to ANSI, so a UTF-8 em-dash came back as `�?"` in the log. The
  doctor/resolve output keeps its emoji — those go to a terminal, never to
  `--log`.
- **Pre-existing hazard this does not fix**: whenever stderr is a file (already
  true under `cmd /c … 2>`), a write failure — full disk, disconnected network
  share — panics the printing thread rather than returning an error. In `serve`
  that surfaces as "hotkeys silently stop", not a crash.
- **The toast spawn needs `CREATE_NO_WINDOW` because of this.** After
  `FreeConsole`, `CreateProcess` hands a console-subsystem child of a
  console-less parent a brand-new console, *shown* — std passes only
  `CREATE_UNICODE_ENVIRONMENT` and never sets `STARTF_USESHOWWINDOW`. So the
  PowerShell toast in `notify.rs` would flash a black window on every post,
  including once per keypress (`on_hotkey` uses `Cause::HumanAction`, which is
  never throttled), undoing the entire point of `--log`. The flag is invisible
  from the call site; do not "clean it up".
- **A ~60 ms flash remains, measured.** Task Scheduler cannot start a
  console-subsystem process without allocating a console first; `FreeConsole`
  only closes it afterwards. On Windows 11 ARM64 (build 26200), inside session
  1, 25 ms sampling, with a control: bare `serve` leaves a console **and** a
  `PseudoConsoleWindow` up for the life of the daemon; `serve --log` shows one
  window at ~150 ms that is gone by ~210 ms and leaves nothing;
  `conhost.exe --headless` in front of the same command shows nothing at any
  point. Worse than it sounds where Windows Terminal is the default terminal:
  the console arrives as a new WT *tab*, and closing that tab sends
  `CTRL_CLOSE_EVENT` and kills the daemon.
- **Point the task at the real exe.** A launcher that stays alive as a parent —
  a Scoop shim, `cmd /c` — holds the console, so beckon's `FreeConsole` does
  not close it. Verified: the shim's pid is the `ParentProcessId` of the real
  beckon process.

## `beckon-serve.exe`

The escalation that was taken. A second binary
(`crates/beckon-cli/src/bin/beckon-serve.rs`), GUI-subsystem on just that
`[[bin]]` target — never the whole crate, which would swallow the output of
`list`, `installed`, `search`, `resolve`, `doctor`. It has no console at any
point, so it has none of the flash measured above: there is no PE console
subsystem for `CreateProcess` to allocate one against in the first place.

It calls `redirect_to_log` before anything else in `main` tries to print — the
only step ahead of it is argument parsing, which already reports its own errors
through a dialog, not `eprintln!` — and reports its own startup failures the
same way, with `MessageBoxW` rather than stderr, since there is no console to
fall back to even before the redirect runs.

**`CREATE_NO_WINDOW` on the toast spawn stays load-bearing here for the same
reason it matters after `FreeConsole`**: `CreateProcess` gives a
console-subsystem child (PowerShell) of a console-less parent a brand-new
*visible* console, and a GUI-subsystem parent is console-less from the start,
not just after detaching.

Tray menu, autostart Run-key and first-run design:
`docs/superpowers/specs/2026-08-10-windows-serve-app-design.md`.

### Verified on a14, 2026-08-11

Windows 11 Home build 26200, ARM64, built natively and driven from **session
1** — an SSH shell is session 0 and cannot see the desktop at all, so every
observation went through a one-shot scheduled task. Registering that task hit
the SID-not-`DOMAIN\user` failure `examples/windows/serve/README.md`
documents, which is a live confirmation that note is still accurate.

- PE subsystem read from the header: `beckon.exe` = 3 (console),
  `beckon-serve.exe` = 2 (GUI).
- **No window of any kind** from `beckon-serve.exe`, `EnumWindows` sampled at
  25 ms for 4 s. The control fired as expected in the same run:
  `beckon.exe serve --log` produced `CASCADIA_HOSTING_WINDOW_CLASS` (a Windows
  Terminal tab) at 243 ms, gone by 245 ms. Always run that control — a broken
  probe and a clean result look identical without one.
- Tray icon is real: `Shell_NotifyIconGetRect` returns `hr=0` with a screen
  rect while running, and `0x80004005` after Quit, which is how you prove
  `NIM_DELETE` actually ran.
- Menu contents read out of the live process with `MN_GETHMENU` on the `#32768`
  popup, then `GetMenuStringW`. That is the only way to see another process's
  menu text, and it is what proved **"Start with Windows" is present for
  `beckon-serve.exe` and absent for `beckon.exe serve`** — the row is omitted
  where a Run value could never work.
- Quit from the menu exits in under 500 ms with code 0. This was the risk
  `TPM_RETURNCMD` was adopted to remove: without it `WM_COMMAND` arrives inside
  the menu's own modal loop, where a `PostQuitMessage` that failed to break out
  would look exactly like a freeze.
- `--version` / `--help` show a dialog and exit 0; an unknown flag shows a
  dialog and exits 2, matching `beckon.exe`'s usage-error code.
- First run wrote the starter config and `beckon check` accepted it.
- **Autostart survives a reboot.** Ticked through the tray menu, then rebooted
  a14: boot 09:15:34, logon 09:15:48, `beckon-serve` up at 09:16:01 — 13 s
  after logon — with a fresh pid, the exact Run command line,
  `18 shortcuts registered` in the log, and **parent process `explorer.exe`**,
  which is what a Run-key launch looks like and is the part that distinguishes
  it from a leftover process.
- The Run value it wrote names the scoop **`current` junction**, not the
  version directory. That mitigation was reasoned from how scoop lays out its
  store; it is now observed.
- **Still unverified, because it needs a human at the keyboard**: the hover
  tooltip's text, the menu dismissing on click-away (the `SetForegroundWindow`
  half of KB135788), menu placement on a high-DPI display, whether Pause
  actually swallows a physical keypress (only the unregister-and-report half
  was checked), a hotkey pressed while the menu is open, and
  config-edit-to-tooltip latency.
- **a14 cannot be rebooted unattended into a signed-in state.**
  `AutoAdminLogon` is 0 and `shutdown /g` (restart + auto sign-on) is rejected
  with error 87, so the machine stops at the sign-in screen and a Run value
  does not fire until someone signs in. Enabling autologon would mean storing
  the password in the registry in the clear — don't. Plan on a person being
  present.

## Caps Lock as the beckon key — the LLHOOK exception

`keyboard.caps = true` installs a `WH_KEYBOARD_LL` hook. That **reverses** the
"RegisterEventHotKey / RegisterHotKey: no event tap, no LLHOOK" decision. The
reversal is deliberate and narrow: off by default, on one OS.

**Since 2026-08-12 there are TWO reasons to hold that hook, not one**: Caps,
and a settings-window chord capture. There is still exactly one hook —
`capture::HookOwners` refcounts the two reasons, and `hook_proc` consults the
capture arm **first** — but the exception is now reachable on a machine where
the user left `keyboard.caps = false`, for the seconds a recording lasts. The
capture arm's own rules are in `docs/notes/settings-window.md`.

Caps is an **alias for the configured chord** — `ctrl+super+alt` by default,
`keyboard.caps_hold` to change it — not a fifth modifier. The hook injects the
chord `RegisterHotKey` already listens for, so `Combo`, `parse_shortcuts` and
`register_all` are untouched and the config file is identical on a machine with
the tick and one without. Decisions live in `beckon_core::caps::decide` (pure,
tested on all three CI jobs); `beckon-windows/src/caps_hook.rs` only translates
`KBDLLHOOKSTRUCT` to `SendInput`.

Two hazards are removed by construction, not guarded against, and both are easy
to reintroduce by "simplifying":

- **The chord is injected as one burst.** Holding `ctrl+win+alt` down across
  real time would make a bare Caps tap press and release Win alone — the
  gesture that opens the Start menu.
- **Only keys bound to the chord are injected for.** Otherwise
  `Caps+<anything>` becomes a genuine `ctrl+win+alt` chord the shell may act
  on.

**The hook must never call `backend.beckon()`.** A callback that outruns
`LowLevelHooksTimeout` (300 ms default) is silently unhooked by Windows with no
error anywhere, and `backend.beckon()` measured ~57 ms typical / ~945 ms on the
miss path. The alias design keeps the callback at a hash lookup plus one
`SendInput` — **13 ms cold, 5.2 ms warm, measured on a14**, so 2–4 % of budget.
(An earlier estimate of "microseconds" was wrong by three orders of magnitude;
the headroom is real but it is not unlimited, so nothing else belongs in that
callback.) The real work happens later on the ordinary `WM_HOTKEY` path.

**Measured on a14 2026-08-11, not reasoned:** an injected chord does fire our
own `RegisterHotKey`; the one-burst chord does not open the Start menu
(verified against a control that proved a bare Win tap does — without that
control a blind detector and a clean result are indistinguishable); an injected
`VK_CAPITAL` flips the toggle, so `caps_tap = "capslock"` is implementable; and
end-to-end, `Caps+N` focused Notepad with `serve` running and did nothing
without it.

**That `VK_CAPITAL` row is TRUE ONLY HERE.** The macOS port carried it across
and shipped a dead option; see `docs/notes/macos-backend.md`.

Known gaps, documented in the README rather than hidden:

- **UIPI.** beckon runs at normal integrity, so the hook never sees keys while
  an elevated window has focus; Caps silently does nothing there. The typed
  `ctrl+super+alt+t` chord **does** still work, because `RegisterHotKey` is not
  subject to UIPI — there is always a fallback. Both halves measured by hand on
  a14 2026-08-11 with Task Manager elevated and focused, against a
  normal-window control run first.
- **Other remappers.** kanata / PowerToys / AHK claiming Caps means beckon never
  sees it. Detection is unreliable; documented, not guessed.
- **EDR.** A low-level keyboard hook is the classic keylogger signature.

Pausing must never leave Caps able to swallow a keystroke. That used to mean
`set_paused(true)` unhooks outright — true while Caps was the only reason to
hold the hook, no longer guaranteed now that capture can hold it too:
`sync_caps_hook`'s `uninstall_for(HookReason::Caps)` (`serve.rs:869`) drops only
the Caps reason, and the HHOOK survives if a capture also owns it. What actually
makes pausing safe is `clear_bindings()`, called first: it zeroes
`Config::wanted`, and `hook_proc`'s `!c.wanted && st.at_rest()` arm passes every
event straight through once nothing swallowed is still owed a matching up —
installed or not, a paused hook eats nothing.

**Windows deliberately does NOT take the machine-global Caps flock macOS uses**,
and the comment on that arm says so with the recipe to close it: two
`beckon.exe serve` on two configs, one Caps binding each, press both.
`WH_KEYBOARD_LL` chains rather than shadows, so the failure *looks* likely and
looking is not measuring.

## Live Windows probes

`crates/beckon-windows/examples/` holds probes that drive the real binary on
real hardware. They exist for the same reason `testing/linux_live_test.py`
does: they are the **only** layer that can reach a tray icon, a message loop or
a keyboard hook, and every defect listed below was invisible to 159 green unit
tests and to both `WINCHECK` commands.

| Probe | Answers |
|---|---|
| `caps_probe` | Does an injected chord fire our own `RegisterHotKey`? Does the burst open Start (with a control that proves the detector works)? Does an injected `VK_CAPITAL` toggle? What does `SendInput` cost? |
| `caps_live` | End-to-end `Caps+<key>`, run once without `serve` and once with it — the difference is the result |
| `settings_probe` | Opens the settings window via the tray's own double-click notification, reads every control back with `EnumChildWindows`, drives an edit and an Apply |
| `combo_probe` | Does a populated `CBS_DROPDOWN` rewrite its own edit text as you type? (No.) Builds the control in-process, subclasses its child EDIT, and runs an empty combo, a plain EDIT, comctl32 v5-vs-v6 and `SendInput` as controls |

Defects they caught, none reachable from a unit test:

- Three settings labels shared control id `-1`, and `layout` positions through
  `GetDlgItem`, which resolves every `-1` to the same first match — so two
  controls were never placed.
- Typing "Notepad" into the App combo wrote `"d"` to the model while the screen
  said "Debuggable Package Manager". **The cause is not the combo box.**
  `apply_state` runs on every keystroke and ends by calling `layout`, whose
  `SetWindowPos` makes a *populated* combo re-synchronise its edit to the
  closest matching item and select the whole string — so the next character
  replaced all of it. A `CBS_DROPDOWN` does **not** autocomplete while you
  type; `combo_probe` measured that under comctl32 6.16 with real keystrokes,
  and the first fix failed on hardware precisely because it assumed otherwise.
  Guarded by `Ui::shown_external`; see
  `docs/superpowers/measurements/2026-08-11-landing-1-a14.md` §24–26.

**Running them: SSH into a14 lands in session 0**, which has no desktop and no
keyboard, so every result there is a confident false negative. Go through a
scheduled task in session 1, registered with `New-ScheduledTaskSettingsSet
-AllowStartIfOnBatteries -Priority 4`. **Both flags, not one.** `schtasks`'
defaults refuse to start on battery and leave the task `Queued` forever on a
laptop; separately, `New-ScheduledTask*` defaults to **priority 7**, and a task
left there on battery produces no diagnostic of any kind — it looks exactly
like the thing under test hanging, which is unfalsifiable when the thing under
test is a GUI you cannot see. Use `-EncodedCommand` for the PowerShell, and a
`.bat` for anything with a redirect, or the quoting is eaten.

**`cargo build --examples` does not build `[[bin]]` targets** — use
`--all-targets`, or you will test a stale `beckon-serve.exe`.

Reading control text across processes needs `SendMessage(WM_GETTEXT)`;
`GetWindowText` returns the kernel-side caption instead and reads back empty
for an EDIT or COMBOBOX.

## `cargo fmt` DOES cover the cfg-gated Windows modules

**REFUTED 2026-08-12: "`cargo fmt --all -- --check` does not cover
`crates/beckon-windows/src/*`."** Landing 2a lost time to this belief and it
was about to be written down as fact. The reasoning was plausible — `lib.rs`
gates nine modules behind `#[cfg(target_os = "windows")]`, so on a macOS host
those `mod` items are not compiled, and CI's `fmt` job runs on
`ubuntu-latest`, meaning nothing anywhere would ever have looked at them.

**Measured on rustfmt 1.9.0-stable, and it is wrong: rustfmt does not evaluate
`cfg` when it walks the module tree.** Probe, per file: append
`fn   __p( )  ->i32{  1 }` and run `cargo fmt --all -- --check`. It exits 1 and
names the file for `settings_window.rs`, `autostart.rs`, `caps_hook.rs`,
`hotkey.rs`, `examples/settings_probe.rs` and `src/bin/beckon-serve.rs` —
cfg-gated modules, an example and a `[[bin]]` alike. Do not re-add the claim
without re-running that probe.

`rustfmt --edition 2021 --check <file>` is still worth knowing, because it is
the *fast* check on one file rather than a different one. It is not a stronger
gate, and a session that reaches for it believing `cargo fmt` is blind is about
to trust something it has not tested.

## Raising `rust-version` turns clippy lints ON

And a cross-target `cargo check` cannot see them. Measured 2026-08-16, twice in
one branch, both times misfiled as pre-existing. Clippy suppresses any lint
whose suggested API postdates the declared MSRV, so the floor is a lint gate as
well as a build gate: moving it 1.75 → 1.88 made `manual_is_multiple_of` (needs
1.87) and `manual_dangling_ptr` (needs 1.84) fire in **three files the branch
never touched**, and CI went red on the merge commit. A/B on one tree is the
probe — flip `rust-version`, `touch` the file, re-run clippy: `1.75 → 0 hits`,
`1.88 → 1 hit`.

Two rules follow:

- **A local gate must run `cargo clippy --target aarch64-pc-windows-msvc
  --all-targets -- -D warnings`, not just `cargo check --target …`.** `check`
  runs no lints at all, so a macOS host that only cross-*checks* the Windows
  crate is blind to every Windows-only clippy error CI will hit. That is
  exactly how this one shipped.
- **`manual_dangling_ptr`'s suggestion is WRONG for `MAKEINTRESOURCE` and must
  not be taken.** `PCWSTR(1 as *const u16)` is an integer resource id Win32
  packs into a pointer slot and never dereferences; clippy offers
  `std::ptr::dangling::<u16>()`, which returns the type's ALIGNMENT — measured
  as **2**, so the icon would load from resource id 2 instead of 1, silently.
  The right spelling is `std::ptr::without_provenance(1)`, measured as **1**,
  which also says out loud that the value is not a pointer.
