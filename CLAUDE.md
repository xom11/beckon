# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with
code in this repository.

**Long-form measurements live in `docs/notes/`, not here.** This file holds the
rules and the shape of the thing; each note carries the evidence behind a rule,
including the entries that were later refuted or narrowed. When a rule below
looks arbitrary, the note says why — read it before "simplifying" anything.

| note | covers |
|---|---|
| [`docs/notes/worktrees-and-git.md`](docs/notes/worktrees-and-git.md) | why one worktree per session, and the git commands whose output cannot distinguish two opposite states |
| [`docs/notes/linux-backends.md`](docs/notes/linux-backends.md) | sway/i3, X11, GNOME, KDE, Hyprland; the shared algorithm; the Wayland-hotkey survey |
| [`docs/notes/macos-backend.md`](docs/notes/macos-backend.md) | `[NSApp run]`, the Caps tap, AX/probe traps, hot-path cost |
| [`docs/notes/windows-backend.md`](docs/notes/windows-backend.md) | window filters, catalog cost, `--log`, `beckon-serve.exe`, the LLHOOK exception |
| [`docs/notes/settings-window.md`](docs/notes/settings-window.md) | the four doors, the status vocabulary, chord capture, what must not be re-implemented |
| [`docs/notes/distribution.md`](docs/notes/distribution.md) | `--version` sha, the nix build breakage, packager token, the user's nix hosts |
| [`docs/notes/site-and-assets.md`](docs/notes/site-and-assets.md) | `site/`, the share card, the README animation recorder |

Design specs and dated measurement sessions are in `docs/superpowers/`.

## Project: beckon

Cross-platform focus-or-launch app switcher for macOS, Windows, and Linux. A
thin CLI wrapper around per-OS native backends — invoked by the existing
dotfiles (sway/AHK/Hammerspoon) instead of replacing them.

**Behavior**: press hotkey → if app not running, launch it. If running but not
focused, focus it. If already focused, cycle to next window of the same app, or
hide.

**No config file on the hot path.** `beckon <id>` resolves a user-supplied id
at runtime against the OS's own metadata (Linux `.desktop` files, macOS
LaunchServices, Windows Start menu). The dotfile per OS holds the id. beckon
ships discovery commands (`list`, `search`, `resolve`) so users don't have to
dig the id out of the OS by hand. The one file beckon does read is the
resident-mode shortcuts TOML (`serve` / `check`, macOS + Windows) — a
hotkey→Name table, not an id-alias layer.

**Name-first identifiers.** The id can be a human-readable Name (e.g. `Claude`,
`Brave`) or a canonical OS-level id (e.g. sway `app_id`, macOS `bundle_id`).
beckon resolves Names against installed-app metadata (`.desktop` `Name=` on
Linux). Names are stable across machines; OS-level ids often are not (Brave PWA
hashes vary per install). Bindings should prefer Names; canonical ids are a
fallback for ambiguity.

## One worktree per session

**Every session that is going to change anything works in its own git worktree.
Never in the primary checkout, and never directly on `main`.**

```sh
cd ~/Documents/dev/beckon
git worktree add .worktrees/<branch> -b <branch> origin/main
cd .worktrees/<branch>
```

Measured 2026-08-14: the primary checkout held 970 uncommitted lines from two
unrelated workstreams while three Claude sessions had it open. One session's
`git switch` silently re-homed another's next five commits onto `main`, and
every command involved reported success — `git log --oneline -1` never names
the branch you are on. Full account in
[`docs/notes/worktrees-and-git.md`](docs/notes/worktrees-and-git.md).

**A worktree prevents two sessions colliding on a FILE. It does nothing about
two sessions building the same THING** — that failure happened with both sides
committed on their own branches. Only the last rule below catches it.

- **Verify state, do not read output.** `git branch --show-current` *before*
  committing; `git branch -vv` or `git ls-remote --heads origin <branch>`
  *after* pushing. An empty push and a real one print the same line.
- **Share `target/` for `check`, `clippy` and `fmt`. Do not trust it for
  `build` and `run`.** Export
  `CARGO_TARGET_DIR=~/Documents/dev/beckon/target` (it is ~7.4 GB and this
  workspace also cross-compiles to `aarch64-pc-windows-msvc`), but:
  - **an error naming a symbol you can grep is a stale artifact, not a bug**;
  - `cargo clean -p <pkg>` misses cross-target artifacts — pass
    `--target aarch64-pc-windows-msvc` too, or the clean appears to disprove
    the diagnosis;
  - `target/debug/beckon` is one path shared by every worktree, so **build into
    a private `CARGO_TARGET_DIR` whenever you intend to run the binary and
    believe its output.** This one reports nothing at all when it goes wrong.
- **The first exec of a freshly linked binary is killed on this machine** (exit
  137, empty output); the second succeeds. A fresh `--help | grep` returning
  nothing is not evidence. Re-run before believing it.
- **The primary checkout stays on `main` and stays clean.** It is for reading,
  for `git log`, and for owning the shared `target/`.
- **Clean up when the branch merges, in this order**:
  `git worktree remove .worktrees/<branch>`, then `git branch -d <branch>` and
  `git push origin --delete <branch>`. The order is load-bearing: a branch
  cannot be deleted while any worktree holds it. `git worktree remove` itself
  does not care where your shell is; its only gate is `--force`, and a worktree
  with modified files is somebody mid-task — look at
  `git -C <worktree> status --porcelain` first.
  **Claude Code makes its own worktrees under `.claude/worktrees/`**, so
  `git worktree list` is the only complete inventory.
- **Before deleting a branch, check with `git cherry`, never a commit count.**
  After a rebase-merge the SHA changes while the patch does not, so every
  ref-counting command reports work that is not missing. `git ls-remote --heads
  origin` asks the server and cannot be stale; `origin/<b>` in your clone can.
- **On any shared repo use `git commit --only <file>`**, not `git add` +
  `git commit` — the latter takes the whole index, and a peer staging files
  mid-race lands their work in your commit. Verify with `git show --stat HEAD`.
- **Before starting, look for company — and look at branches, not just at the
  working tree.** This is the only rule here that catches duplicate *design*:

  ```sh
  git worktree list                 # other checkouts, both directory shapes
  git status                        # in the PRIMARY checkout: someone mid-task
  ListAgents                        # other sessions, but not what they build
  git fetch --all && git branch -a  # committed work you would never see
  git branch -vv                    # and how stale YOUR OWN refs are
  git log --all --oneline -20
  ```

  Other people's work is far more often committed-but-unmerged than
  uncommitted. Either way: reconcile the plan before executing it, and talk to
  the other session if there is one.

## Architecture

### Workspace layout (Rust)

```
beckon/
├── Cargo.toml                # workspace root
├── crates/
│   ├── beckon-core/          # Backend trait, shared types, settings contract
│   ├── beckon-macos/         # NSWorkspace + AX + CGWindowList — phase 2 done
│   │   └── src/
│   │       ├── lib.rs        # pick_backend, doctor, is/request_accessibility
│   │       ├── backend.rs    # Backend trait impl: focus / launch / cycle / hide
│   │       ├── apps.rs       # LaunchServices + .app bundle catalog, Name resolution
│   │       ├── windows.rs    # AX window list + AXRaise (the step-5a cycle)
│   │       ├── ffi.rs        # hand-rolled AX / CGWindowList extern "C"
│   │       ├── hotkey.rs     # RegisterEventHotKey + [NSApp run]
│   │       ├── caps_tap.rs   # CGEventTap: Caps alias + chord capture
│   │       ├── tray.rs       # NSStatusItem menu (serve)
│   │       ├── shell.rs      # /usr/bin/open: open / reveal / https-only URL
│   │       └── settings_window/   # mod, widgets, keyboard, system, about
│   ├── beckon-windows/       # Win32 (EnumWindows + COM IShellLinkW) — phase 3 done
│   │   ├── build.rs          # stamps BECKON_TARGET; embeds examples' manifest
│   │   ├── examples.rc       # resource script for the probes
│   │   └── src/
│   │       ├── lib.rs        # pick_backend, resolve report
│   │       ├── backend.rs    # Backend trait impl
│   │       ├── apps.rs       # Start Menu .lnk + AppsFolder catalog
│   │       ├── window_ops.rs # EnumWindows / SetForegroundWindow / AttachThreadInput
│   │       ├── hotkey.rs     # RegisterHotKey + tray icon/menu
│   │       ├── caps_hook.rs  # WH_KEYBOARD_LL: Caps alias + chord capture
│   │       ├── autostart.rs  # HKCU…Run value ("Start with Windows")
│   │       ├── prefs.rs      # HKCU\Software\beckon: DarkMode/Opacity/CapsView
│   │       ├── shell.rs      # ShellExecuteW open path + MessageBoxW dialogs
│   │       ├── logfile.rs    # --log redirect + console detach, size-capped
│   │       └── settings_window/   # mod, layout, paint, theme, chrome, ids
│   ├── beckon-linux/         # multi-backend, dispatch by env at runtime
│   │   └── src/
│   │       ├── lib.rs        # detect compositor/DE, return Box<dyn Backend>
│   │       ├── algorithm.rs  # neutral focus algorithm shared by every backend
│   │       ├── desktop.rs    # .desktop parser + Name resolution
│   │       ├── state.rs      # single-app MRU at $XDG_RUNTIME_DIR/beckon-mru
│   │       ├── i3ipc.rs      # swayipc — handles BOTH sway and i3
│   │       ├── hyprland.rs   # native Unix-socket IPC
│   │       ├── niri.rs       # native socket IPC — NIRI_SOCKET, JSON lines
│   │       ├── x11.rs        # x11rb / EWMH — non-i3 X11 DEs
│   │       ├── gnome.rs      # zbus client → bundled GNOME Shell extension
│   │       └── kde.rs        # zbus → org.kde.kwin.Scripting
│   └── beckon-cli/           # command surface as a lib.rs, shared by two binaries
│       ├── build.rs          # BECKON_VERSION; embeds assets/beckon.ico (MSVC)
│       └── src/
│           ├── lib.rs        # cli_main(), clap Args/Command, RESERVED
│           ├── main.rs       # beckon.exe: console-subsystem shim
│           ├── serve.rs      # resident-mode loop + tray menu (macOS + Windows)
│           ├── serve_app.rs  # beckon-serve.exe front door
│           ├── lockfile.rs   # one `serve` per config path, plus the Caps flock
│           ├── notify.rs     # desktop notification policy
│           ├── stable_id.rs  # per-config lock hash
│           └── bin/beckon-serve.rs   # GUI-subsystem shim (Windows only)
├── assets/
│   ├── beckon.ico             # the SOURCE of the mark. Windows tray / Explorer /
│   │                          #   Alt-Tab icon, and what the two below derive from
│   ├── beckon-menubar.png     # macOS menu bar TEMPLATE, generated from beckon.ico
│   │                          #   by tools/make-menubar-mark.py — never hand-edited
│   └── beckon.icns            # macOS APP icon, generated from beckon.ico by
│                              #   tools/make-app-icon.py — never hand-edited
├── extensions/beckon@xom11.github.io/   # GNOME Shell extension (GJS, ESM)
├── testing/                   # linux_live_test.py, macos_*.sh/.lua, README
├── site/                      # landing page (GitHub Pages)
├── docs/notes/                # the measurement notes this file points at
└── docs/superpowers/          # specs, plans, dated measurement sessions
```

### Backend trait (core abstraction)

`id: &str` is what the user typed: a Name, a canonical OS id, or anything in
between. The backend is responsible for resolution against OS metadata before
acting.

```rust
pub trait Backend {
    fn list_running(&self) -> Result<Vec<RunningApp>>;
    fn list_installed(&self) -> Result<Vec<InstalledApp>>;

    /// Single entry point — backend implements the full algorithm:
    /// launch / focus / cycle-same-app / toggle-other-app / hide.
    fn beckon(&self, id: &str) -> Result<()>;
}
```

Why one entrypoint: focus / cycle / hide are intertwined per-OS (a sway tree
query is one IPC call that yields all the info). Splitting into five trait
methods would mean re-querying the window tree several times per invocation.
One method = one query.

### Focus algorithm

Single behavior, not configurable. The backend implements the full algorithm;
the CLI just passes the id.

```
1. id = argv[1]                                   (Name or canonical OS id)
2. resolve(id) → (target_app_id, optional exec)   (per-OS)
3. windows-of-app = scan tree for app_id == target_app_id
4. if empty AND we have an exec  → launch via exec
   if empty AND no exec          → error: id matched nothing
5. if running, unfocused         → focus first window
6. if already focused on this app:
     a. same app has another window → focus next window (MRU cycle)
     b. else, another app exists    → switch to most-recent window of a DIFFERENT app
     c. else                        → hide / minimize current
```

Step 5 subsumes both "multi-window cycle" and "alt-tab toggle" without a flag.

On Linux the decision lives in `beckon-linux/src/algorithm.rs` and every
backend feeds it a neutral `Vec<WindowSnapshot>`; that is the only place to
change focus / cycle / toggle / hide policy. Two rules there are easy to
reintroduce as bugs and are covered in the note: **target matching is a set,
not a string**, and **step 5a cycles over a ring ordered by address, not by
recency** (recency-ordered is a 2-cycle on every backend with real focus
history).

### CLI surface (bare positional + subcommands, since 0.6.0)

```
beckon <id>                          # focus-or-launch (default, hot path)
beckon list                          # list running apps with their ids
beckon installed                     # list installed apps with launch ids
beckon search <NAME>                 # fuzzy search across running + installed
beckon resolve <ID>                  # validate id, print metadata + suggestions
beckon doctor                        # check environment (permissions, IPC, etc.)
beckon check <CONFIG> [--resolve]    # validate a shortcuts TOML file (CI-friendly)
beckon serve <CONFIG> [--log PATH]   # resident hotkey service (macOS, Windows)
beckon -v, --verbose                 # debug logging (combine with any command)
beckon -h, --help
beckon -V, --version

# Edge case: id starting with `-`, or an app Name that shadows a subcommand
beckon -- -weird.id
beckon -- list
```

`--log` belongs to `serve`, so the order is verb-then-operand-then-flag:
`beckon serve C --log P`. `beckon --log P serve C` is a usage error, and that
is structural — the argument is declared inside the `Serve` variant.

#### Reserved names are a closed list

Eight words — `list`, `installed`, `search`, `resolve`, `doctor`, `check`,
`serve`, `help` — and `RESERVED` in `crates/beckon-cli/src/lib.rs` is the list.
`help` is in it because clap injects that subcommand whether or not we declare
it. An app whose Name is one of the eight is only reachable as
`beckon -- <name>`. Subcommand matching is byte-exact while every beckon
resolver is case-insensitive, so capitalisation alone decides the reading:
`beckon Resolve` reaches the id path, `beckon resolve` does not.

**Growth rule: new capabilities are flags on an existing verb, never a new
top-level verb.** Each verb costs an app name permanently, paid by users who
never touch the verb. **No aliases, ever** — an alias costs a name and saves
nothing.

**Never set `args_conflicts_with_subcommands`.** Measured on clap 4.6.1: that
flag makes clap stop looking for a subcommand once any argument has been parsed
(`clap_builder/src/parser/parser.rs:592`), so `beckon -v list` silently binds
`list` to the id positional and exits 0 — the 0.5.x defect respelled — and
`testing/linux_live_test.py:509` runs eight live focus tests through exactly
that `-v` shape. The id/subcommand conflict is instead enforced by hand in
`Args::parse_checked`, **which must not be deleted**: without it clap accepts
`beckon Claude list` silently and discards the id. Re-run both cases before
touching either half.

**`run <id>` was considered and dropped.** `--` escapes both a reserved name
and a leading dash; `run` escapes only the first, and `run -weird.id` is itself
a usage error (measured) that still needs `run -- -weird.id`.

Full measurements and rejected alternatives:
`docs/superpowers/specs/2026-08-10-cli-subcommands-design.md`.

#### `check` validates shape; `check --resolve` validates meaning

`beckon check` never consults the machine — that is what makes it usable in CI,
where none of the apps are installed, pinned by
`check_without_resolve_says_nothing_about_whether_the_app_exists`.

`--resolve` grades every app name against this machine's catalog using
`beckon_core::certainty::Certainty`. Every backend already computed the tier
and threw it away on one line; the grade is that projection removed.

**Only `NoMatch` changes the exit code.** A `Guess` — the single substring tier
every backend has — resolves, so it prints and exits 0. Two of the author's own
bindings depend on that tier deliberately (`Settings` matching *System
Settings*, `DeepSeek` matching *DeepSeek - Into the Unknown*), so failing on
`Guess` would turn a correct file red, which is how a check stops being run.
The scale is why the flag exists at all: measured on `rog`, **14 of 18
shortcuts did not resolve** while `beckon check` reported `ok: 18 shortcuts`.

A `Guess` reports **two different hazards** and says which: one candidate means
a later install can take the name; several means the winner is already decided
by sort order, not by anything the user wrote.

### Linux backend dispatch

"Linux" is not one backend — it depends on the compositor/DE currently running.
`beckon-linux` detects this at startup via env variables. A user only ever runs
one compositor at a time, so there is no "support both at once", only "detect
correctly".

```rust
// crates/beckon-linux/src/lib.rs
fn pick_backend() -> Result<Box<dyn Backend>> {
    if env::var("SWAYSOCK").is_ok()                       { return SwayBackend::new(); }
    if env::var("I3SOCK").is_ok()                         { return I3Backend::new(); }
    if env::var("HYPRLAND_INSTANCE_SIGNATURE").is_ok()    { return HyprlandBackend::new(); }
    if env::var("NIRI_SOCKET").is_ok()                    { return NiriBackend::new(); }
    if env::var("WAYLAND_DISPLAY").is_ok() {
        // Mutter and KWin both refuse external focus, so each needs a
        // collaborator running INSIDE the compositor. XDG_CURRENT_DESKTOP
        // decides which; guessing wrong produces an error about the wrong
        // desktop entirely.
        return match wayland_desktop() {
            Kde     => KdeBackend::new(),      // probes org.kde.kwin.Scripting
            Gnome   => GnomeBackend::new(),    // probes the shell extension
            Unknown => GnomeBackend::new().or_else(|_| KdeBackend::new()),
        };
    }
    if env::var("DISPLAY").is_ok()                        { return X11Backend::new(); }
    bail!("No supported display server detected.");
}
```

| Detected env | Backend | Status |
|---|---|---|
| `SWAYSOCK` | sway (Wayland) — `i3ipc::I3IpcBackend` | ✅ |
| `I3SOCK` | i3 (X11) — same `I3IpcBackend` (shared protocol) | ✅ |
| `HYPRLAND_INSTANCE_SIGNATURE` | Hyprland — native socket IPC | ✅ |
| `NIRI_SOCKET` | niri — native socket IPC (`niri.rs`) | ✅ |
| `DISPLAY` (no i3, no Wayland) | X11 generic via `x11rb` / EWMH | ✅ covers GNOME-X11, KDE-X11, openbox, awesome, XFCE |
| `WAYLAND_DISPLAY` + `XDG_CURRENT_DESKTOP=GNOME` | zbus → bundled shell extension | ✅ |
| `WAYLAND_DISPLAY` + `XDG_CURRENT_DESKTOP=KDE` | zbus → `org.kde.kwin.Scripting` | ✅ |

Per-backend detail: [`docs/notes/linux-backends.md`](docs/notes/linux-backends.md).

### Resolution priority (Linux)

Linux scans `.desktop` files in `$XDG_DATA_DIRS/applications/` and tries:

1. **`Name=` exact** (case-insensitive, normalized to drop bidi/format marks).
   Recommended for dotfiles — Names are stable across machines.
2. **Filename stem** (`kitty.desktop` → `kitty`). Useful when copy-pasting an
   id from `beckon list`.
3. **`StartupWMClass=`**. Often wrong on Wayland (Brave ignores it) but
   harmless to try.
4. **`Name=` substring** (case-insensitive). Multiple matches → alphabetical
   first wins ("first wins", like rofi).

If 1–4 all fail, fall back to treating `id` as a literal `app_id`. This still
allows focusing apps that aren't in any `.desktop` file; *launching* such an
unknown id is an error with a "run `beckon installed` / `beckon search`" hint.

**An empty id is rejected at the CLI boundary.** It used to reach tier 4, where
it is a substring of every `Name`, so a dotfile doing `beckon "$APP"` with
`$APP` unset silently launched whatever sorted first.

Scanning rules the tiers depend on:

- **`scan()` returns entries sorted by id.** All four tiers take the first
  match, so an unordered vector made the winner depend on `HashMap`'s
  per-process random seed. With two entries sharing a `Name=` (deb + snap
  Firefox, a user override, two PWAs with the same display name) the *same*
  keypress resolved differently from run to run and beckon alternated between
  focusing the window and launching a second copy. Measured before the fix: 20
  runs of `beckon resolve Dup` split 12/8.
- **Subdirectories are scanned, and the id is the relative path with `/`
  replaced by `-`** — the XDG menu spec's *desktop file id*. Wine
  (`applications/wine/Programs/…`) and KDE install that way; a flat `read_dir`
  made those apps invisible to `installed` and unlaunchable. Symlinked
  directories are not followed (`applications/foo -> /` would walk the whole
  filesystem).
- **`XDG_DATA_HOME` / `XDG_DATA_DIRS` set-but-empty is treated as unset, and
  relative paths are ignored**, per the basedir spec. Taking `XDG_DATA_HOME=`
  literally made beckon read `./applications/*.desktop` relative to the working
  directory — so a `.desktop` file in any directory the user happened to `cd`
  into became a launch target, while their real overrides were skipped.

macOS and Windows follow the same shape with their own lists (LaunchServices
localized + canonical names; Start menu shortcut name + AppUserModelID).

### Dotfile integration (hotkeys stay where they are)

On Linux the tool does NOT register global hotkeys — the compositor / WM
dotfile does, and `exec beckon`s. On macOS and Windows `serve` hosts them
itself, because no compositor binds keys there.

```
# sway (Linux) — Names from .desktop `Name=`
bindsym $cap+c exec beckon Claude

# AHK (Windows) — Names from Start menu / shortcut display name
^#!c:: Run("beckon Claude")

# Hammerspoon (macOS) — Names from app display name
hs.hotkey.bind(hyper, "c", function() hs.execute("beckon Claude") end)
```

Names are typically the same on every OS — `Claude` resolves correctly on all
three. Where Names collide, fall back to a canonical OS id and document the
disambiguation in a comment.

## Phase plan

| Phase | Target | Status |
|---|---|---|
| 1a | Linux / sway (Wayland) | ✅ `i3ipc::I3IpcBackend` via swayipc |
| 1b.i3 | Linux / i3 (X11) | ✅ same `I3IpcBackend` |
| 1b.x11 | Linux / X11 generic via x11rb | ✅ `x11::X11Backend`, EWMH ClientMessages |
| 1c | Linux / Hyprland | ✅ `hyprland::HyprlandBackend` |
| 1f | Linux / niri | ✅ `niri::NiriBackend`, native socket IPC |
| 1d | Linux / GNOME Wayland | ✅ `gnome::GnomeBackend` + shell extension |
| 1e | Linux / KDE Wayland | ✅ `kde::KdeBackend` via KWin scripting |
| 2 | macOS | ✅ `objc2-app-kit` + AX + CGWindowList |
| 3 | Windows | ✅ Win32 EnumWindows + COM IShellLinkW |

## A measurement on one OS is data about that OS, not about the design

Read this before porting anything. The same mistake has been made three times
in one day, always in the same shape: a correct, measured sentence in this
repository is carried across a platform boundary as a premise rather than
re-run as a question.

| carried across | what the port assumed | what measuring said |
|---|---|---|
| *"an injected `VK_CAPITAL` flips the toggle, so `caps_tap = "capslock"` is implementable"* | the macOS arm posts `kVK_CapsLock` the same way | `CGEventPost` does **not** move the lock on macOS, at either tap level, with `AXIsProcessTrusted = 1`. The option did nothing at all until it moved to `IOHIDSetModifierLockState`. |
| `banner_shown` / `warn_dot_shown` partition `external_change` | taking `banner_shown` is taking the pair | `warn_dot_shown` had **zero callers** in `beckon-macos`, so three doors out of four carried the fact nowhere — while the core test asserting the partition passed the whole time. |
| a tap cannot reach the brightness/volume keys | so `NX_SYSDEFINED` need not be considered | registering it **swallows** them for the length of a recording. The guess was backwards. |

None of the three original sentences was wrong. Each was true of the platform
it was measured on, and each stopped being a measurement the moment it crossed
over — which is exactly what makes a well-recorded fact a hazard, because it
reads as settled and a reader has no reason to re-open it.

**The rule is about WHERE the sentence came from, not whether it is
believable.** Treat every "measured on a14" / "measured on airm3" claim as
scoped to that machine's OS, and re-run the probe rather than the reasoning.
All three above were caught by running something; none was caught by reading,
and two had already survived adversarial review.

The same rule applies inside this repository's own history: several entries in
`docs/notes/` are marked REFUTED, CORRECTED, NARROWED or WITHDRAWN. Those
markers are the point — do not restore a deleted claim without re-running the
probe named beside it.

## Known constraints

**Wayland global hotkeys.** On every Linux target the compositor / DE binds the
key and `exec beckon`s; `serve` is not offered there. This is a choice, not a
missing API — routes exist on X11, KDE, Hyprland and GNOME, and sway is the one
real gap. The survey and the three reasons it stays out of scope are in
[`docs/notes/linux-backends.md`](docs/notes/linux-backends.md).

**GNOME / KDE Wayland refuse external focus** by design. GNOME is supported via
the bundled shell extension (install once, then log out and back in — Wayland
can't reload shell live); KDE via KWin's own scripting engine, with nothing for
the user to install. Neither compositor exposes a usable Wayland protocol for
window enumeration.

**Caps Lock as the beckon key** installs a `WH_KEYBOARD_LL` hook on Windows /
`CGEventTap` on macOS, reversing the "no event tap, no LLHOOK" decision. The
reversal is deliberate and narrow, and there are now **two** reasons to hold
the hook (Caps, and the settings window's chord capture) behind one refcount.
Caps is an **alias for the configured chord**, never a fifth modifier, so the
config file is identical with and without it. The rules that must not be
"simplified" — one-burst injection, injecting only for bound keys, never
calling `backend.beckon()` from the callback — are in
[`docs/notes/windows-backend.md`](docs/notes/windows-backend.md) and
[`docs/notes/macos-backend.md`](docs/notes/macos-backend.md).

**macOS Accessibility permission** is required to focus arbitrary apps and is
bound to the codesigned binary identity — every fresh `cargo build` invalidates
it. **Input Monitoring is a separate grant**, in a separate pane, per-binary,
and is what the Caps tap needs; without it the tap is created successfully and
receives nothing, silently.

Since 0.9.9 beckon **asks** rather than only reporting:
`beckon_macos::request_accessibility()` raises the system dialog, alongside the
read-only `is_accessibility_trusted()`. **macOS raises that panel only when no
answer is recorded** and returns the stored verdict silently afterwards, so a
caller must offer the Settings pane *as well as*, never *instead of*, the ask.

**PWAs must be installed as standalone apps** (Brave/Chrome → "Install this
site as an app") so each gets a stable bundle ID / `.desktop` / `WM_CLASS`.
beckon does NOT handle `--app=URL` invocations — too brittle to detect reliably.

**PWA hash drift.** Brave/Chrome PWAs get an extension hash inside their
`.desktop` filename or bundle_id — e.g.
`brave-fmpnliohjhemenmnlpbfagaolkdacoja-Default`. **The hash is generated
locally during install and differs across machines**, so canonical ids can't be
synced via dotfile copy. `Name=Claude` is stable everywhere. **This is the
primary reason Name-based resolution is the recommended id format.**

**Per-OS identifier asymmetry.** Where Names don't resolve consistently — a
localized macOS display name, two apps sharing a `Name=` on Linux — fall back
to a canonical OS id. Discovery via `beckon search <name>` per machine.

## Decisions already made

1. **Daemon vs one-shot CLI — one-shot for the hot path, plus an opt-in
   resident mode.** `beckon <id>` stays a one-shot CLI (~10 ms cold start) for
   compositor-bound hotkeys. `serve <config>` additionally hosts the hotkeys on
   macOS/Windows, reading a flat TOML (`"ctrl+super+alt+t" = "kitty"`) and
   watching it for reloads.

   **beckon never daemonizes, and that is a decision, not an omission.**
   Surveyed skhd, yabai, espanso, kanata, AutoHotkey and caddy: effectively no
   hotkey daemon forks. On macOS a detached process loses the login session's
   bootstrap namespace — beckon already needs `TransformProcessType(→
   UIElement)` because a launchd-spawned process has no window-server identity,
   and without one `RegisterEventHotKey` returns success while never delivering
   a press. On Windows there is no `fork` at all. Above all it solves the wrong
   problem: forking buys "survives closing the terminal", while what users need
   is "starts at login" and "restarts if it dies" — both of which still require
   launchd / Task Scheduler.

   **Windows answered this without spending a verb**: a checkable "Start with
   Windows" row in `beckon-serve.exe`'s tray menu writes the `HKCU\…\Run` value
   directly. macOS gets its launch agent from the Homebrew formula's
   `service do` block. An `install`/`start`/`stop` lifecycle is still open and
   its shape is not decided; the growth rule rules out a top-level `service`
   verb.

2. **MRU tracking source per backend.** Step 5b (toggle-back) on Linux uses a
   single-app state file at `$XDG_RUNTIME_DIR/beckon-mru` — **except on
   Hyprland**, which has real focus history in `focusHistoryID` and reads and
   writes nothing. Each invocation reads live focus from IPC, so mouse /
   native-hotkey transitions reconcile on the next beckon call. Limitation:
   only beckon-mediated focus changes are recorded, so a run of mouse-only
   switches produces a stale "previous". Acceptable for a hotkey workflow.
   macOS and Windows read z-order directly (`CGWindowList` / `EnumWindows`) and
   need no state file.

3. **Notification on errors — auto-detect TTY.** If stderr is not a terminal
   (the typical hotkey-bound invocation), beckon fires a desktop notification
   in addition to the stderr line: `notify-send` on Linux,
   `osascript display notification` on macOS, a PowerShell toast on Windows.
   All best-effort.

4. **`search` scope and ranking — still open.** Should `beckon search claude`
   match window titles too, or only app id / name? Title match is more
   forgiving but volatile. Default likely id + name only, `--include-titles`
   opt-in.

## Crate dependencies

```toml
# core / cli
anyhow    = "1"
thiserror = "2"
clap      = { version = "4", features = ["derive"] }

# macOS (phase 2)
objc2            = "0.6"
objc2-foundation = "0.3"   # NSString / NSURL / NSArray / NSDictionary
objc2-app-kit    = "0.3"   # NSWorkspace / NSRunningApplication
core-foundation  = "0.10"  # CF lifetime wrappers (CFType / CFArray / CFString)
plist            = "1"     # parse .app/Contents/Info.plist
# AX and CGWindowList are hand-rolled `extern "C"` in beckon-macos/src/ffi.rs.
# ~6 functions, not worth dragging in objc2-application-services.

# windows (phase 3)
windows = { version = "0.61", features = [
    "Win32_Foundation",
    "Win32_Graphics_Dwm",              # DwmGetWindowAttribute (cloaked detection)
    "Win32_Storage_FileSystem",        # WIN32_FIND_DATAW (IShellLinkW::GetPath)
    "Win32_System_Com",                # COM init + IPersistFile (.lnk parsing)
    "Win32_System_Threading",          # OpenProcess, AttachThreadInput
    "Win32_UI_Shell",                  # IShellLinkW, ShellExecuteW
    "Win32_UI_Shell_Common",           # ITEMIDLIST
    "Win32_UI_WindowsAndMessaging",    # EnumWindows, SetForegroundWindow, etc.
] }

# linux (phase 1)
swayipc    = "3"      # sway + i3 (same protocol)
serde      = "1"      # serde_json for Hyprland JSON IPC payloads
serde_json = "1"
x11rb      = "0.13"   # any EWMH-compliant X11 DE
zbus       = "4"      # session bus client for GNOME extension + KWin scripting

# resident mode (check / serve)
toml      = "0.8"    # beckon-core: parse the shortcuts file
toml_edit = "0.22"   # beckon-core: WRITE it back, keeping comments. Already a
                     #   transitive dep of toml 0.8, so it costs nothing.
notify    = "6"      # beckon-cli:  watch it for live reload
fs4       = "0.8"    # beckon-cli:  flock, one serve per config path
```

### What beckon reads and writes

**The only *file* beckon reads is the `serve` shortcuts TOML** — and, since the
settings window, the only file it writes. **That write resolves the path before it
renames onto it**, because a rename onto a symlink replaces the LINK, and both of
the author's Macs reach this file through one (`mkOutOfStoreSymlink`, itself forced
by a plain `home.file.source` putting a read-only store path there). Do not
"simplify" `write_config_text` back to renaming onto the path it was given —
`saving_through_a_symlink_writes_the_target_and_keeps_the_link` is what fails, and
only that one. There is no config for
`beckon <id>` itself and no resolve cache; ids resolve against OS metadata on
every call.

**There is a second store on Windows, and it is deliberately not a file.** The
settings window's System page keeps the window's own look — `DarkMode`,
`Opacity` and **`CapsView`** — as three `REG_DWORD`s in `HKCU\Software\beckon`
(`crates/beckon-windows/src/prefs.rs`). The count matters: **that table is the
list of what to DELETE**, so a profile reset built from a two-value list leaves
the caps fold behind and the Shortcuts list comes back folded on a machine the
user believes is clean. This split is what makes a theme switch keep working
when `apps.toml` does not parse — the one state a user most needs a GUI in.

**One more read, and it is not a config: `current_exe()` and a `stat` of it.**
The About page shows the RUNNING IMAGE's path and compares its mtime against
this process's start time. It exists because a recorded failure had every
obvious surface lying: a watchdog-started beckon on a14 ran the 0.8.0 image for
three hours while `beckon --version` said 0.9.0 and scoop's `current` junction
pointed at 0.9.0. The path is deliberately **not** resolved through
`GetFinalPathNameByHandleW` — resolving reports today's junction target, which
is the surface that lied.

**The config path is canonicalised once and then SIMPLIFIED**, in
`cmd_serve_app`. `Path::canonicalize` on Windows is
`GetFinalPathNameByHandleW` and always returns `\\?\C:\…`; that spelling
reached the startup log, the `Open config file` tooltip, the System page's
config row — where `SS_PATHELLIPSIS` elides from the MIDDLE, so the prefix
never shortens — and, less visibly, `ShellExecuteW` and `explorer.exe /select,`,
the classic non-acceptors of it. `beckon_core::paths::plain` undoes it at the
origin, conservatively: a volume GUID path, a UNC with no share, or any
component Win32 would rewrite once unprotected (trailing dot or space, `.`/`..`,
a DOS device name) keeps the prefix. **Deliberately NOT applied inside
`lockfile::acquire`**, which canonicalises independently one line earlier and
hashes the result into the lock file's NAME — renaming the lock would let an
old and a new binary both serve. Long paths are unaffected: the manifest
declares `longPathAware`.

**As of the check-for-updates feature, beckon also makes exactly one outbound
NETWORK request** — never a file read or write — and only on a person
pressing `Check for updates`, from `serve`'s settings window: the About
page's own button, or the tray row, which opens the window (landing on
About) and runs the SAME check rather than firing one of its own. There is
no background poll, no request on startup, and nothing downloaded:
`beckon_cli::update` spawns the system `curl` against
`github.com/.../releases/latest`, reads the redirect's tag out of the
`Location` header, and compares it to the running version. See
`docs/notes/distribution.md` for the mechanism and the measurements behind
it.

## Out of scope (explicitly)

- **Config for the hot path / app aliases.** `beckon <id>` resolves against OS
  metadata directly. No `[apps.claude]` mapping, no resolve cache. The `serve`
  TOML is a *hotkey table*, not a place to alias ids.
- **Global hotkey registration on Linux** — the compositor / WM dotfile owns
  it. Out of scope by choice, not for lack of an API.
- **Fuzzy app launchers à la Rofi/Alfred** — beckon is for *known* hotkey-bound
  apps invoked by raw id. `search` is for ad-hoc id discovery during setup, not
  interactive launching.
- **Window tiling / layout management** — beckon only focuses and launches,
  never moves or resizes.
- **PWA install helper** — the user installs PWAs manually via the browser.
- **Self-update and background update polling.** The Check for updates button
  (§ "What beckon reads and writes" above) only ever reports; it never
  downloads or replaces anything, and there is no timer or CLI verb that
  checks on its own. Reason: the running binary lives in a read-only nix
  store or under a package manager's own junction (scoop's `current`,
  Homebrew's Cellar symlink), and a process that overwrites itself there
  breaks the install the same package manager is supposed to own.
- **GUI / TUI — CLI only, with one exception**, which is `serve`'s control
  surface rather than a launcher: the tray context menu (reload, pause, open
  the log, toggle autostart, quit) and the settings window it opens. Four doors
  on both macOS and Windows, against one `beckon_core::settings` contract, so
  **the place to change a decision is `beckon-core`, never a window**.

  The window shows the shortcut table with per-row registration state, edits
  it, writes the same TOML back through `toml_edit` so hand edits and window
  edits stay interchangeable, and lists installed apps only to fill in a Name
  while authoring a binding. **Nothing in the shortcut table focuses or
  launches anything.** It also pauses/resumes hotkeys, reloads the config,
  toggles autostart, sets its own theme, opens or reveals files, copies three
  About fields and opens three `https://` links.

  Four rules that are load-bearing and cheap to break:

  - **`Pause shortcuts` and `Reload` call `serve.rs`'s own `set_paused` and
    `reload` through `SettingsCommand`, and must never be re-implemented.**
    `set_paused` does five ordered things, one of which is CLEARING the
    registration map — and that cleared map is what makes the `paused` status
    word load-bearing on every row.
  - **A capability this process does not have is omitted, not greyed** — the
    reasoning the tray already uses. `Start with Windows` under
    `beckon.exe serve`, the log row without `--log`, `Start at login` on macOS.
    All decided in `beckon_core::settings::system_state`.
  - **The status vocabulary is four words and a healthy row says nothing**:
    `paused` > `in use` > `missing` > `other chord`. One function,
    `row_condition`, produces the list flag AND the editor's notes, and derives
    `mark` at the end — so the cell and the note cannot disagree by
    construction rather than by discipline.
  - **The availability probe asks the OS last**, from `probe_plan`: parse, the
    F12 guard, the row's own chord, other rows, the row's saved chord, and only
    then `RegisterHotKey`. Everything before the last step is a fact the OS
    cannot report.

  Everything else — the shape, the filter's app-column-only rule, chord
  capture and its hook-lifetime rules, the geometry derivation — is in
  [`docs/notes/settings-window.md`](docs/notes/settings-window.md). Design:
  `docs/superpowers/specs/2026-08-11-windows-settings-window-and-caps-design.md`
  and `2026-08-14-four-doors-settings-window-design.md`.

## Distribution

- **GitHub**: https://github.com/xom11/beckon — source plus 6 prebuilt binaries
  per release (x86_64 + aarch64 × linux-gnu / apple-darwin / pc-windows-msvc).
- **Homebrew tap** (macOS / Linux): `brew install xom11/tap/beckon`. The
  formula ships a macOS LaunchAgent, so `brew services start beckon` is the
  whole resident-mode install.
- **Scoop bucket** (Windows, x86_64 + arm64):
  `scoop bucket add xom11 https://github.com/xom11/scoop-bucket && scoop install xom11/beckon`.
- **Cargo (from git)**:
  `cargo install --git https://github.com/xom11/beckon beckon-cli`.
- **Nix flake**: `nix run github:xom11/beckon -- list`, or pull
  `inputs.beckon.overlays.default` into your nixpkgs.

Both packager manifests are auto-bumped by
`.github/workflows/bump-packagers.yml`, which `release.yml` calls as a
`workflow_call` job — **it fires on its own; no manual `gh workflow run` is
needed.** It needs a fine-grained PAT in repo secret `PACKAGER_TOKEN`.
**Rotated 2026-08-11; expires 2027-08-12.**

**`beckon --version` prints `beckon <version> (<short sha>)`.** The sha exists
because a flake input pins a *rev*, so every rev between two releases would
otherwise report the identical Cargo version. It comes from `BECKON_GIT_REV`
(passed by `flake.nix`) with a `git rev-parse` fallback, because a nix build
has no `.git` to ask.

**Two CI guards cover different halves of the same class of breakage** and
neither replaces the other: `package.nix` passes `-p beckon-cli --bin beckon`
(so nix no longer compiles `beckon-windows` at all), and the `build` matrix
runs a bare `cargo check --workspace --all-targets` on the Linux and macOS
legs. An ungated `mod` inside `beckon-windows` broke `nix build` from v0.8.0 to
v0.9.3 and nobody noticed for a month. A local gate must include the bare
unexcluded check.

**Landing page**: `site/`, deployed by `.github/workflows/pages.yml` — Pages
source must stay **GitHub Actions** in repo settings, or the workflow goes
green and publishes nothing. Not `docs/`, which holds internal specs and these
notes. `tools/check-site.sh` runs in CI and asserts the install commands, the
letter→app table and the version still match `README.md` and `Cargo.toml`.

**One mark, three files, and the two derived ones are generated.** `beckon.ico`
is the source; `beckon-menubar.png` and `beckon.icns` are produced from it by
`tools/make-menubar-mark.py` and `tools/make-app-icon.py`. Neither is hand-edited,
and neither re-typesets the letter — the `b` in the `.ico` is the drawn brand
letterform, not a font glyph, so both tools lift it as an alpha mask by
luminance. Re-typesetting would put a third `b` in the program.

The two macOS files share **one corner ratio, 0.2353** — `cornerRadius(8.0)` on
the About door's 34 pt tile — so the app icon, the menu bar mark and the settings
window all carry the same shape. The `.icns` additionally sits on Apple's grid
(body 824 of a 1024 canvas): `beckon.ico` is full-bleed, which is right on
Windows where the shell applies a shape and wrong here where nothing does.

**The README animation goes stale in silence.** `assets/five-answers.webp` is a
photograph of `site/#how`, and nothing in CI compares the two — re-run
`tools/record-five-answers.mjs` in the same commit that changes that section.
**webp rather than gif is about GitHub's renderer, not the codec**: GitHub
wraps an animated GIF in an `<animated-image>` player whose still canvas is
backed in CSS pixels, so no source resolution can un-blur the paused state.
The share card is `site/og.png` (1200x630, `tools/make-og-card.mjs`); its
scrapers cache hard, so an unchanged preview after a deploy is not evidence the
change failed.

Detail for all of the above:
[`docs/notes/distribution.md`](docs/notes/distribution.md) and
[`docs/notes/site-and-assets.md`](docs/notes/site-and-assets.md).

## Testing

Unit tests run on all three CI jobs and are where every pure decision belongs —
that is the whole reason `beckon_core::settings`, `caps`, `capture`,
`page_plan` and `theme` are in `beckon-core` and not in a `cfg`-gated module.

Three layers exist that unit tests structurally cannot reach:

| layer | what only it can catch |
|---|---|
| `testing/linux_live_test.py` | `.desktop` resolution against a real machine, the class a toolkit actually advertises, whether a focus/minimize request is honoured. All five backends pass 19/19. **It kills GUI apps to build its preconditions — run it in a VM.** |
| `crates/beckon-windows/examples/` | a tray icon, a message loop, a keyboard hook. **SSH to a14 lands in session 0**, which has no desktop, so every result there is a confident false negative — go through a scheduled task in session 1. |
| `testing/macos_*.sh` / `macos_settings_drive.lua` | the real window driven by real events. `sudo launchctl asuser` draws, Hammerspoon injects and reads the AX tree. |

**Always run a control.** A blind detector and a clean result print the same
thing, and this repository has lost whole sessions to that shape more than once
— see the REFUTED / WITHDRAWN entries in `docs/notes/`.

Two gate facts worth knowing before CI tells you:

- **`cargo fmt --all -- --check` DOES cover the cfg-gated Windows modules.**
  rustfmt does not evaluate `cfg` when walking the module tree. Measured; do
  not re-add the opposite claim without re-running the probe in
  [`docs/notes/windows-backend.md`](docs/notes/windows-backend.md).
- **A local gate must run `cargo clippy --target aarch64-pc-windows-msvc
  --all-targets -- -D warnings`, not just `cargo check --target …`** — `check`
  runs no lints at all, so cross-*checking* the Windows crate from macOS is
  blind to every Windows-only clippy error CI will hit. Raising `rust-version`
  turns lints ON in files a branch never touched.

## Picking up next session

All three phases are done and deployed:

- **Linux** — five backends, all passing the live suite; nix flake + overlay.
- **macOS** — full focus / launch / cycle / toggle / hide, `serve` with tray and
  four settings doors, Caps tap. Deployed on `airm3` via Hammerspoon.
- **Windows** — same surface plus `beckon-serve.exe`, Caps hook, autostart.
  Tested on ARM64 Windows 11.

Reasonable next steps:

1. **AHK integration** — wire beckon into `~/.nix/windows/ahk/launch-app.ahk`,
   replacing the old title-match approach. Each binding becomes
   `Run("beckon <Name>")`.
2. **Browser-PWA AUMID matching** — MSIX/AppX identity is handled natively;
   browser PWAs still need validation because window ownership and AUMID
   behavior vary by browser.
3. **Polish** — fuzzy match for `resolve` typos; `--include-titles` for
   `search` (decision 4 above); the `serve install`/`start`/`stop` lifecycle
   (decision 1).
