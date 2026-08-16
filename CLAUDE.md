# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project: beckon

Cross-platform focus-or-launch app switcher for macOS, Windows, and Linux. A thin CLI wrapper around per-OS native backends — invoked by the existing dotfiles (sway/AHK/Hammerspoon) instead of replacing them.

**Behavior**: press hotkey → if app not running, launch it. If running but not focused, focus it. If already focused, cycle to next window of the same app, or hide.

**No config file on the hot path.** `beckon <id>` resolves a user-supplied id at runtime against the OS's own metadata (Linux `.desktop` files, macOS LaunchServices, Windows Start menu). The dotfile per OS holds the id. beckon ships discovery commands (`list`, `search`, `resolve`) so users don't have to dig the id out of the OS by hand. The one file beckon does read is the resident-mode shortcuts TOML (`serve` / `check`, macOS + Windows) — a hotkey→Name table, not an id-alias layer.

**Name-first identifiers.** The id can be a human-readable Name (e.g. `Claude`, `Brave`) or a canonical OS-level id (e.g. sway `app_id`, macOS `bundle_id`). beckon resolves Names against installed-app metadata (`.desktop` `Name=` on Linux). Names are stable across machines; OS-level ids often are not (Brave PWA hashes vary per install). Bindings should prefer Names; canonical ids are a fallback for ambiguity.

## One worktree per session

**Every session that is going to change anything works in its own git
worktree. Never in the primary checkout, and never directly on `main`.**
`.worktrees/` has been in `.gitignore` since before this entry existed — the
rule was implicit, and being implicit is exactly how it got broken.

```sh
cd ~/Documents/dev/beckon
git worktree add .worktrees/<branch> -b <branch> origin/main
cd .worktrees/<branch>
```

**Why this is a rule and not a preference.** Measured on 2026-08-14: the
primary checkout held **970 uncommitted lines belonging to two unrelated
workstreams at once** — a Hyprland-parity change (`hyprland.rs`, `CLAUDE.md`,
`testing/linux_live_test.py`) and a `check --resolve` implementation
(`beckon-cli/src/lib.rs`, two test files) — while `ListAgents` showed three
Claude sessions with that directory open. The failure modes are not
hypothetical and not visible from inside any one session:

- `git status` cannot say which change belongs to whom, so nobody can commit
  without either sweeping in a stranger's work or hand-picking hunks.
- **`git switch` in one session silently re-homes every commit another
  session makes next.** One did, mid-edit. The victim is not warned at any
  point, and the obvious defence does not exist. Committing to `main` is not
  an error, so git has nothing to say: the most it would print is the ordinary
  `[main abc1234] …` — and that line was not printed either, because the
  commits went through `git commit -q`, which suppresses it. The check that
  followed looked conclusive and was not: **`git log --oneline -1` never names
  the branch you are on.** Five commits landed on `main` while every command
  involved reported success. This is why the rule below is *run
  `git branch --show-current`*, not *read the output more carefully* — on this
  path there is no output to read.
  - And the push that should have caught it does not. `git push -u origin
    <branch>` pushed that untouched branch and printed
    `remote: Create a pull request for '<branch>' …` — **an empty push and a
    real one print the same thing**, and `-q` does not suppress it because it
    is a remote message. It surfaced only when `gh pr create` refused with
    *"you must first push the current branch"*, which is a different tool,
    much later. Verify instead of reading output: `git branch --show-current`
    **before** committing, and `git branch -vv` (it prints
    `[origin/<branch>: ahead N]`) or `git ls-remote --heads origin <branch>`
    against your local SHA **after** pushing.
- A `CLAUDE.md` edit from either session lands on top of the other's
  uncommitted text and is swept into whichever commit is made first.
- **Two sessions independently designed the *same* flag with *opposite*
  semantics** — `--resolve` exiting non-zero versus never changing the exit
  code. Note what did *not* cause this: one side's design was **committed**,
  on its own branch, the whole time. It was invisible because nobody fetched
  or listed branches, not because it was unwritten.

**So: a worktree prevents two sessions colliding on a FILE. It does nothing
about two sessions building the same THING.** Two spotless worktrees produce
the duplicate-design failure just as readily. Only the last rule below catches
that one.

Rules that follow from that:

- **Share the build directory for `check`, `clippy` and `fmt`. Do not trust it
  for `build` and `run`.** `target/` is ~7.4 GB and this workspace also
  cross-compiles to `aarch64-pc-windows-msvc`, so a fresh worktree rebuilds
  all of it: export `CARGO_TARGET_DIR=~/Documents/dev/beckon/target` and take
  the saving. Cargo locks the directory, so concurrent builds serialise rather
  than interleave — but "they do not corrupt each other" is where the useful
  half of that sentence stops. Three failure modes, all measured 2026-08-15
  with several worktrees live, in ascending order of how long they cost:

  1. **A stale rlib produces a compile error naming a symbol that is plainly
     in your source.** Ours said `no variant named 'Reset' for enum
     DefaultButton` about a file the task had never touched, while `Reset` sat
     in the enum three lines from where `grep` found it. **Rule: an error
     about code you can grep is a stale artifact, not a bug.**
  2. **`cargo clean -p <pkg>` does not clean cross-target artifacts**, so the
     obvious fix appears to disprove the diagnosis: the clean runs, removes
     ~99 MB, and the build fails *identically*, which reads as "so it is not
     the cache" and sends you hunting a real bug. Pass the flag —
     `cargo clean -p beckon-core -p beckon-windows --target
     aarch64-pc-windows-msvc` removed a further 882 MB and the check then
     passed in 0.8 s.
  3. **`target/debug/beckon` is one path shared by every worktree, so the
     binary you run may be another branch's** — and this one reports nothing
     at all. `cargo build` said `Finished in 0.08s` and the binary at that
     path had no `--resolve` flag, i.e. it predated `origin/main`. There is no
     error to notice; you simply measure the wrong program. **Build into a
     private `CARGO_TARGET_DIR` whenever you intend to run the binary and
     believe its output.**

  Unrelated to worktrees but it compounds all three: **the first exec of a
  freshly linked binary is killed on this machine** (exit 137, empty output),
  and the second succeeds. It makes a fresh `--help | grep` return nothing and
  a fresh test binary report a failure, neither of which is true. Re-run
  before believing either.
- **The primary checkout stays on `main` and stays clean.** It is for reading,
  for `git log`, and for owning the shared `target/`.
- **Clean up when the branch merges**: `git worktree remove .worktrees/<branch>`,
  then `git branch -d <branch>` and `git push origin --delete <branch>`.
  `git worktree list` is the inventory. One stray predates this rule and is
  not covered by it — `~/Documents/dev/beckon-fix-linux`.

  **Two things about the removal, both learned the hard way 2026-08-16.**

  A session **cannot remove its own worktree**: git refuses while the branch is
  checked out there, and a Claude session is `cd`'d inside it. So the last
  session on a branch does the merge, deletes the REMOTE branch, and leaves the
  worktree and the local branch for someone standing in the primary checkout —
  which means saying so out loud rather than reporting "cleaned up".

  **`.claude/worktrees/` is a second home for these and the rule above does not
  name it.** Claude Code makes its own worktrees there rather than in
  `.worktrees/`, so `git worktree list` shows both shapes and a cleanup that
  only looks at `.worktrees/` misses half the inventory. `four-doors-phase-0`
  lived at `.claude/worktrees/four-doors-phase-0` for its whole life and was
  recorded here as a "stray" for being in the wrong directory, when it was
  simply made by a different tool.
- **Before starting, look for company — and look at branches, not just at the
  working tree.** This is the only rule here that catches duplicate *design*,
  and the three obvious checks are all blind to it:

  ```sh
  git worktree list                 # other checkouts
  git status                        # in the PRIMARY checkout: someone mid-task
  ListAgents                        # other sessions, but not what they are building
  git fetch --all && git branch -a  # committed work you would otherwise never see
  git branch -vv                    # and how stale YOUR OWN refs are
  git log --all --oneline -20       # including branches nobody has merged
  ```

  **`git branch -vv` was added 2026-08-16, after the list above sent a session
  to the wrong conclusion.** It ran every other line, saw a branch called
  `four-doors-phase-0` it did not recognise, and measured it with
  `git rev-list --left-right --count main...four-doors-phase-0`, which
  returned `0 55`. It read that as *"an unmerged branch, 55 commits ahead of
  main"* and told the user that the settings-window design was about to be
  replaced. **The truth was the mirror image: the branch was already merged,
  and the primary checkout's local `main` was 55 commits behind.**
  `origin/main`, `four-doors-phase-0` and the `v0.9.4` tag were all one
  commit.

  The trap is that `git fetch --all` *had* been run, and it does exactly what
  it says: it updates `origin/main` and does **not** touch `main`. Every
  command in the old list reads a ref, and none of them compares a local
  branch to its upstream — so **the same number supports both readings and
  nothing in that output distinguishes them.** That is why the fix is a
  different command rather than closer reading: `git branch -vv` prints
  `[origin/main: behind 55]` and says which side is stale.

  It is the same shape as the push trap two bullets above — an empty push and
  a real one print the same line — and it has the same escape: ask git for
  the state, do not squint at output that does not carry it.

  Uncommitted work in the shared checkout means somebody is mid-task. A
  *branch* carrying a spec or a design doc means somebody has already decided
  something — and other people's work is far more often committed-but-unmerged
  than uncommitted. Either way the answer is the same: reconcile the plan
  before executing it, and talk to the other session if there is one.

## Architecture

### Workspace layout (Rust)

```
beckon/
├── Cargo.toml                # workspace root
├── crates/
│   ├── beckon-core/          # Backend trait, shared types (RunningApp, WindowId)
│   ├── beckon-macos/         # NSWorkspace + AX + CGWindowList — phase 2 done
│   │   └── src/
│   │       ├── lib.rs        # pick_backend, doctor, is_accessibility_trusted
│   │       ├── backend.rs    # Backend trait impl: focus / launch / cycle / hide
│   │       ├── apps.rs       # LaunchServices + .app bundle catalog, Name resolution
│   │       ├── windows.rs    # AX window list + AXRaise (the step-5a cycle)
│   │       ├── ffi.rs        # hand-rolled AX / CGWindowList extern "C"
│   │       ├── hotkey.rs     # RegisterEventHotKey + [NSApp run] (see the note below)
│   │       ├── tray.rs       # NSStatusItem menu (serve)
│   │       ├── shell.rs      # /usr/bin/open: open / reveal / https-only URL
│   │       └── settings_window/
│   │           ├── mod.rs      # window, tab strip, state, apply_state
│   │           ├── widgets.rs  # AppKit builders; semantic NSColors only
│   │           ├── keyboard.rs # door 2
│   │           ├── system.rs   # door 3
│   │           └── about.rs    # door 4
│   ├── beckon-windows/       # Win32 API (EnumWindows + COM IShellLinkW) — phase 3 done
│   │   └── src/
│   │       ├── lib.rs        # pick_backend, resolve report
│   │       ├── backend.rs    # Backend trait impl: focus / launch / cycle / hide
│   │       ├── apps.rs       # Start Menu .lnk + AppsFolder catalog, Name resolution
│   │       ├── window_ops.rs # EnumWindows / SetForegroundWindow / AttachThreadInput
│   │       ├── hotkey.rs     # RegisterHotKey + tray icon/menu (serve, beckon-serve)
│   │       ├── autostart.rs  # HKCU…Run value ("Start with Windows")
│   │       ├── shell.rs      # ShellExecuteW open path + MessageBoxW dialogs
│   │       └── logfile.rs    # --log redirect + console detach, size-capped
│   ├── beckon-linux/         # multi-backend, dispatch by env at runtime
│   │   └── src/
│   │       ├── lib.rs        # detect compositor/DE, return Box<dyn Backend>
│   │       ├── algorithm.rs  # neutral focus algorithm shared by every backend
│   │       ├── desktop.rs    # .desktop parser + Name resolution
│   │       ├── state.rs      # single-app MRU state at $XDG_RUNTIME_DIR/beckon-mru
│   │       ├── i3ipc.rs      # swayipc — handles BOTH sway and i3 (shared protocol)
│   │       ├── hyprland.rs   # native Unix-socket IPC — Hyprland
│   │       ├── x11.rs        # x11rb / EWMH — non-i3 X11 DEs
│   │       └── gnome.rs      # zbus client → bundled GNOME Shell extension
│   └── beckon-cli/           # command surface as a lib.rs, shared by two Windows binaries
│       ├── build.rs          # embeds ../../assets/beckon.ico into every binary (MSVC only)
│       ├── beckon.rc         # Windows resource script; resource id 1 is the icon
│       └── src/
│           ├── lib.rs        # cli_main(), clap Args/Command, RESERVED
│           ├── main.rs       # beckon.exe: console-subsystem shim -> cli_main()
│           ├── serve.rs      # resident-mode loop + tray menu (macOS + Windows)
│           ├── serve_app.rs  # beckon-serve.exe front door: defaults, starter config
│           ├── lockfile.rs   # one `serve` per config path
│           ├── notify.rs     # desktop notification policy
│           ├── stable_id.rs  # per-config lock hash
│           └── bin/
│               └── beckon-serve.rs   # beckon-serve.exe: GUI-subsystem shim (Windows only)
├── assets/
│   └── beckon.ico             # tray / Explorer / Alt-Tab icon for both Windows binaries
├── extensions/
│   └── beckon@xom11.github.io/   # GNOME Shell extension (GJS, ESM)
│       ├── metadata.json
│       └── extension.js          # exports D-Bus org.gnome.Shell.Extensions.Beckon
├── testing/
│   ├── linux_live_test.py    # live end-to-end suite, run inside a session
│   └── README.md             # how to bring up each compositor headless
├── test-i3-env.sh            # Xephyr+i3 dev sandbox (start/stop/xterm)
└── README.md
```

### Live Windows tests

`crates/beckon-windows/examples/` holds three probes that drive the real
binary on real hardware. They exist for the same reason
`testing/linux_live_test.py` does: they are the **only** layer that can
reach a tray icon, a message loop or a keyboard hook, and every defect
listed below was invisible to 159 green unit tests and to both `WINCHECK`
commands.

| Probe | Answers |
|---|---|
| `caps_probe` | Does an injected chord fire our own `RegisterHotKey`? Does the burst open Start (with a control that proves the detector works)? Does an injected `VK_CAPITAL` toggle? What does `SendInput` cost? |
| `caps_live` | End-to-end `Caps+<key>`, run once without `serve` and once with it — the difference is the result |
| `settings_probe` | Opens the settings window via the tray's own double-click notification, reads every control back with `EnumChildWindows`, drives an edit and an Apply |
| `combo_probe` | Does a populated `CBS_DROPDOWN` rewrite its own edit text as you type? (No.) Builds the control in-process, subclasses its child EDIT, and runs an empty combo, a plain EDIT, comctl32 v5-vs-v6 and `SendInput` as controls |

Defects they caught, none reachable from a unit test:

- Three settings labels shared control id `-1`, and `layout` positions
  through `GetDlgItem`, which resolves every `-1` to the same first match —
  so two controls were never placed.
- Typing "Notepad" into the App combo wrote `"d"` to the model while the
  screen said "Debuggable Package Manager". **The cause is not the combo
  box.** `apply_state` runs on every keystroke and ends by calling `layout`,
  whose `SetWindowPos` makes a *populated* combo re-synchronise its edit to
  the closest matching item and select the whole string — so the next
  character replaced all of it. A `CBS_DROPDOWN` does **not** autocomplete
  while you type; `combo_probe` measured that under comctl32 6.16 with real
  keystrokes, and the first fix failed on hardware precisely because it
  assumed otherwise. Guarded by `Ui::shown_external`; see
  `docs/superpowers/measurements/2026-08-11-landing-1-a14.md` §24–26.

Running them: **SSH into a14 lands in session 0**, which has no desktop and
no keyboard, so every result there is a confident false negative. Go through
a scheduled task in session 1, registered with `New-ScheduledTaskSettingsSet
-AllowStartIfOnBatteries -Priority 4`. **Both flags, not one.** `schtasks`'
defaults refuse to start on battery and leave the task `Queued` forever on a
laptop; separately, `New-ScheduledTask*` defaults to **priority 7**, and a
task left there on battery produces no diagnostic of any kind — it looks
exactly like the thing under test hanging, which is unfalsifiable when the
thing under test is a GUI you cannot see. Use `-EncodedCommand` for
the PowerShell, and a `.bat` for anything with a redirect, or the quoting is
eaten. **`cargo build --examples` does not build `[[bin]]` targets** — use
`--all-targets`, or you will test a stale `beckon-serve.exe`.

**REFUTED 2026-08-12: "`cargo fmt --all -- --check` does not cover
`crates/beckon-windows/src/*`."** Landing 2a lost time to this belief and it
was about to be written down here as fact. The reasoning was plausible —
`lib.rs` gates nine modules behind `#[cfg(target_os = "windows")]`, so on a
macOS host those `mod` items are not compiled, and CI's `fmt` job runs on
`ubuntu-latest`, meaning nothing anywhere would ever have looked at them.
**Measured on rustfmt 1.9.0-stable, and it is wrong: rustfmt does not
evaluate `cfg` when it walks the module tree.** Probe, per file: append
`fn   __p( )  ->i32{  1 }` and run `cargo fmt --all -- --check`. It exits 1
and names the file for `settings_window.rs`, `autostart.rs`, `caps_hook.rs`,
`hotkey.rs`, `examples/settings_probe.rs` and `src/bin/beckon-serve.rs` —
cfg-gated modules, an example and a `[[bin]]` alike. Do not re-add the claim
without re-running that probe.

`rustfmt --edition 2021 --check <file>` is still worth knowing, because it is
the *fast* check on one file rather than a different one. It is not a stronger
gate, and a session that reaches for it believing `cargo fmt` is blind is
about to trust something it has not tested — which is how the two `--examples`
and `WINCHECK` traps above actually work, and why this one is written up
despite turning out not to be one.

Reading control text across processes needs `SendMessage(WM_GETTEXT)`;
`GetWindowText` returns the kernel-side caption instead and reads back empty
for an EDIT or COMBOBOX.

### Live backend tests

`testing/linux_live_test.py` drives the real binary against a real
compositor and asserts on what that compositor reports afterwards. It is the
only layer that can catch what unit tests structurally cannot: `.desktop`
resolution against the machine's own metadata, the class a toolkit actually
advertises at runtime, and whether a focus/minimize request is honoured at
all. Every Linux bug fixed in the 2026-08 pass was found by it, and none of
them were visible to the 65 unit tests that were green the whole time.

It detects its environment the same way `pick_backend` does, so run it inside
the session under test. **All five backends now pass 19/19.** Hyprland was the
last to be brought up — 0.56.2 on 2026-08-15, nested inside a live GNOME
session rather than on its own tty, which costs nothing and leaves the host
desktop untouched (recipe in `testing/README.md`, config in
`testing/hypr-nested.conf`). Two of the three defects that run found were the
suite's own, and both looked like focus bugs: on NixOS `pkill -x <name>` never
matches a wrapped binary (`comm` is `.xterm-wrapped`), so the suite left its
own windows behind and step 5c skipped itself while 5b failed expecting a
launch it already had. The other four pass on Ubuntu
26.04 arm64 (GNOME Shell 50.1 headless, sway 1.11, i3 + Xvfb, openbox + Xvfb)
— see `testing/README.md` for the headless bring-up recipes, including the
D-Bus service-directory trick that keeps `gnome-shell --headless` from
deadlocking on `xdg-desktop-portal`. **The suite kills GUI apps to build its
preconditions; run it in a VM.**

### Backend trait (core abstraction)

`id: &str` is what the user typed: a Name, a canonical OS id, or anything in between. The backend is responsible for resolution against OS metadata (Linux `.desktop`, macOS LaunchServices, Windows Start menu) before acting.

```rust
pub trait Backend {
    fn list_running(&self) -> Result<Vec<RunningApp>>;
    fn list_installed(&self) -> Result<Vec<InstalledApp>>;

    /// Single entry point — backend implements the full algorithm:
    /// launch / focus / cycle-same-app / toggle-other-app / hide.
    fn beckon(&self, id: &str) -> Result<()>;
}
```

Why one entrypoint: focus / cycle / hide are intertwined per-OS (sway tree query is one IPC call that yields all the info; AppleScript activation is similar). Splitting into 5 trait methods would mean re-querying the window tree multiple times per invocation. One method = one query = simplest.

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

The hot path (`beckon <id>`) is positional with no subcommand verb — the user types this 99% of the time from a hotkey binding. Discovery/admin actions are subcommands.

`--log` belongs to `serve`, so the order is verb-then-operand-then-flag:
`beckon serve C --log P`. `beckon --log P serve C` is a usage error, and that
is structural — the argument is declared inside the `Serve` variant, so it is
rejected everywhere else without a `requires =` guard.

#### Reserved names are a closed list

Eight words — `list`, `installed`, `search`, `resolve`, `doctor`, `check`,
`serve`, `help` — and `RESERVED` in `crates/beckon-cli/src/main.rs` is the
list. `help` is in it because clap injects that subcommand whether or not we
declare it. An app whose Name is one of the eight is unreachable through the
bare positional and can only be beckoned as `beckon -- <name>`. Subcommand
matching is byte-exact while every beckon resolver is case-insensitive, so
capitalisation alone decides the reading: `beckon Resolve` reaches the id
path, `beckon resolve` does not.

**Growth rule: new capabilities are flags on an existing verb, never a new
top-level verb.** Each verb costs an app name permanently, and the cost is
paid by users who never touch the verb. **No aliases, ever** — an alias costs
a name and saves nothing.

**Never set `args_conflicts_with_subcommands`.** Measured on clap 4.6.1: that
flag makes clap stop looking for a subcommand once any argument has been
parsed (`clap_builder/src/parser/parser.rs:592`), so `beckon -v list`
silently binds `list` to the id positional and exits 0 — the 0.5.x defect
respelled — and `testing/linux_live_test.py:509` runs eight live focus tests
through exactly that `-v` shape. The id/subcommand conflict is instead
enforced by hand in `Args::parse_checked`, which must not be deleted: without
it clap accepts `beckon Claude list` silently and discards the id. Re-run
both cases before touching either half.

**`run <id>` was considered and dropped.** `--` escapes both a reserved name
and a leading dash; `run` escapes only the first, and `run -weird.id` is
itself a usage error (measured) that still needs `run -- -weird.id`. The
escape hatch that strictly dominates is the one that already exists.

Full measurements and the rejected alternatives are in
`docs/superpowers/specs/2026-08-10-cli-subcommands-design.md`.

#### `check` validates shape; `check --resolve` validates meaning

`beckon check` never consults the machine — that is what makes it usable in
CI, where none of the apps are installed, and it is pinned by
`check_without_resolve_says_nothing_about_whether_the_app_exists`.

`--resolve` grades every app name against this machine's catalog using
`beckon_core::certainty::Certainty`. Every backend already computed the tier
and threw it away on one line (`resolve_inner(..).is_some()` on macOS,
`resolve_detailed_in(..).is_none()` on Linux, `apps::resolve(..).is_none()`
on Windows); the grade is that projection removed.

**Only `NoMatch` changes the exit code.** A `Guess` — the single substring
tier every backend has — resolves, so it prints and exits 0. Two of the
author's own bindings depend on that tier deliberately (`Settings` matching
*System Settings*, `DeepSeek` matching *DeepSeek - Into the Unknown*), so
failing on `Guess` would turn a correct file red, which is how a check stops
being run. The scale is why the flag exists at all: measured on `rog`,
**14 of 18 shortcuts did not resolve** while `beckon check` reported
`ok: 18 shortcuts`.

A `Guess` reports **two different hazards** and says which: one candidate
means a later install can take the name; several means the winner is already
decided by sort order over `.desktop` ids or display names, not by anything
the user wrote. Before `desktop::scan()` sorted its output, 20 runs of
`beckon resolve` split 12/8 between two entries sharing a `Name=` — the same
keypress, two answers.

### Linux backend dispatch

"Linux" is not one backend — it depends on the compositor/DE the user is currently running. `beckon-linux` detects this at startup via env variables and dispatches to the right implementation. A user only ever runs one compositor at a time, so there is no "support both at once" — there is only "detect correctly".

```rust
// crates/beckon-linux/src/lib.rs
fn pick_backend() -> Result<Box<dyn Backend>> {
    if env::var("SWAYSOCK").is_ok()                       { return SwayBackend::new(); }
    if env::var("I3SOCK").is_ok()                         { return I3Backend::new(); }
    if env::var("HYPRLAND_INSTANCE_SIGNATURE").is_ok()    { return HyprlandBackend::new(); }
    if env::var("WAYLAND_DISPLAY").is_ok() {
        // Both Mutter (GNOME) and KWin (KDE) refuse external focus, so each
        // needs a collaborator running *inside* the compositor. Which one to
        // try is decided by XDG_CURRENT_DESKTOP; guessing wrong produces an
        // error message about the wrong desktop entirely.
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
|--------------|---------|--------|
| `SWAYSOCK` | sway (Wayland) — `i3ipc::I3IpcBackend` | ✅ Done |
| `I3SOCK` | i3 (X11) — same `I3IpcBackend` (shared protocol) | ✅ Done |
| `HYPRLAND_INSTANCE_SIGNATURE` | Hyprland | ✅ Done |
| `DISPLAY` (no i3, no Wayland) | X11 generic via `x11rb` / EWMH | ✅ Done (covers GNOME-X11, KDE-X11, openbox, awesome, XFCE, ...) |
| `WAYLAND_DISPLAY` + `XDG_CURRENT_DESKTOP=GNOME` | GNOME Wayland — `gnome::GnomeBackend` via zbus → bundled shell extension | ✅ Done |
| `WAYLAND_DISPLAY` + `XDG_CURRENT_DESKTOP=KDE` | KDE Wayland — `kde::KdeBackend` via zbus → `org.kde.kwin.Scripting` | ✅ Done |

### Focus algorithm

Single behavior, not configurable. The backend implements the full algorithm; CLI just passes the id.

```
1. id = argv[1]                                   (Name or canonical OS id)
2. resolve(id) → (target_app_id, optional exec)   (per-OS, see "Resolution priority")
3. windows-of-app = scan tree for app_id == target_app_id
4. if empty AND we have an exec  → launch via exec
   if empty AND no exec          → error: id matched nothing
5. if running, unfocused         → focus first window
6. if already focused on this app:
     a. same app has another window → focus next window (MRU cycle)
     b. else, another app exists    → switch to most-recent window of a DIFFERENT app
     c. else                        → hide / minimize current
```

### Resolution priority (Linux)

Each backend resolves its OS's installed-app metadata. Linux scans `.desktop` files in `$XDG_DATA_DIRS/applications/` and tries:

1. **`Name=` exact** (case-insensitive, normalized to drop bidi/format marks). Recommended for dotfiles — Names are stable across machines.
2. **Filename stem** (`kitty.desktop` → `kitty`). Useful when copy-pasting an id from `beckon list`.
3. **`StartupWMClass=`**. Often wrong on Wayland (Brave ignores it) but harmless to try.
4. **`Name=` substring** (case-insensitive). Multiple matches → alphabetical first wins ("first wins" like rofi).

If priorities 1-4 all fail, fall back to treating `id` as a literal `app_id`. This still allows focusing apps that aren't in any `.desktop` file (ad-hoc programs); launching such an unknown id is an error with a "run `beckon installed` / `beckon search`" hint.

An empty id is rejected at the CLI boundary. It used to reach tier 4, where
it is a substring of every `Name`, so a dotfile doing `beckon "$APP"` with
`$APP` unset silently launched whatever sorted first.

Scanning rules that the tiers depend on:

- **`scan()` returns entries sorted by id.** All four tiers take the first
  match, so an unordered vector made the winner depend on `HashMap`'s
  per-process random seed. With two entries sharing a `Name=` (deb + snap
  Firefox, a user override under a new filename, two PWAs with the same
  display name) the *same* keypress resolved differently from run to run and
  beckon alternated between focusing the window and launching a second copy.
  Measured before the fix: 20 runs of `beckon resolve Dup` split 12/8 between two
  entries. Sorting gives every tier the "alphabetically first `.desktop` id
  wins" rule tier 4 already documented.
- **Subdirectories are scanned, and the id is the relative path with `/`
  replaced by `-`** — the XDG menu spec's *desktop file id*. Wine
  (`applications/wine/Programs/…`) and KDE (`applications/kde4/…`) install
  that way; a flat `read_dir` made those apps invisible to `installed` and
  unlaunchable. Symlinked directories are not followed (`applications/foo -> /`
  would walk the whole filesystem).
- **`XDG_DATA_HOME` / `XDG_DATA_DIRS` set-but-empty is treated as unset, and
  relative paths are ignored**, per the basedir spec. Taking `XDG_DATA_HOME=`
  literally made beckon read `./applications/*.desktop` relative to the
  working directory — so a `.desktop` file in any directory the user happened
  to `cd` into became a launch target, while their real
  `~/.local/share/applications` overrides were skipped.

macOS / Windows backends will follow the same shape with their own priority lists (LaunchServices localized + canonical names; Start menu shortcut name + AppUserModelID).

Step 5 is "smart" — cycles within the same app first, then falls back to toggling to the previous app, then hides if nothing else exists. This subsumes both "multi-window cycle" and "alt-tab toggle" behaviors without a flag.

Reference implementation: `~/.nix/home-manager/dotfiles/hammerspoon/MySpoons/LaunchApp.spoon/init.lua` does step 5b+5c today (toggle + hide). beckon adds 5a on top.

### Dotfile integration (hotkeys stay where they are)

The tool does NOT register global hotkeys itself — that's left to each OS's native mechanism. beckon is a thin CLI invoked by the existing dotfiles. Each dotfile holds the **raw OS identifier** for that platform.

```
# sway (Linux) — Names from .desktop `Name=` (stable across machines)
bindsym $cap+c exec beckon Claude
bindsym $cap+t exec beckon kitty

# AHK (Windows) — Names from Start menu / shortcut display name
^#!c:: Run("beckon Claude")
^#!t:: Run("beckon Windows Terminal")

# Hammerspoon (macOS) — Names from app display name
hs.hotkey.bind(hyper, "c", function() hs.execute("beckon Claude") end)
hs.hotkey.bind(hyper, "t", function() hs.execute("beckon kitty") end)
```

The dotfiles are inherently per-OS already (sway runs only on Linux, AHK only on Windows, Hammerspoon only on macOS), but Names are typically the same on every OS — `Claude` resolves correctly on all three. Where Names collide (two apps with the same `Name=`, or different platforms exposing different display names), users fall back to a canonical OS id and document the disambiguation in a comment.

## Phase plan

| Phase | Target | Status |
|-------|--------|--------|
| 1a | Linux / sway (Wayland) | ✅ Done — `i3ipc::I3IpcBackend` via swayipc |
| 1b.i3 | Linux / i3 (X11) | ✅ Done — same `I3IpcBackend` (shared protocol) |
| 1b.x11 | Linux / X11 generic via x11rb (GNOME-X11, KDE-X11, openbox, awesome, XFCE) | ✅ Done — `x11::X11Backend` via EWMH ClientMessages |
| 1d | Linux / GNOME Wayland via bundled shell extension + zbus | ✅ Done — `gnome::GnomeBackend` |
| 1c | Linux / Hyprland | ✅ Done — `hyprland::HyprlandBackend` via Unix-socket IPC |
| 2 | macOS | ✅ Done — `beckon-macos` via `objc2-app-kit` + AX + CGWindowList |
| 3 | Windows | ✅ Done — `beckon-windows` via Win32 EnumWindows + COM IShellLinkW |
| 1e | Linux / KDE Wayland via KWin scripting + zbus | ✅ Done — `kde::KdeBackend` |

### Phase 1b.i3 implementation note

sway and i3 share the i3-IPC protocol exactly — same `swayipc` crate, same JSON tree, same `[con_id=N] focus` command, same scratchpad. The only differences across compositors:
- **Window identity**: Wayland uses `node.app_id`; X11 uses `window_properties.class` (second token of `WM_CLASS`). `collect_windows` already falls back from one to the other.
- **Socket env var**: `SWAYSOCK` for sway, `I3SOCK` for i3. The dispatcher accepts either.

→ No separate i3 module. `crates/beckon-linux/src/i3ipc.rs` serves both.

### Shared focus algorithm

Every Linux backend (sway/i3, Hyprland, X11 generic) feeds a neutral
`Vec<algorithm::WindowSnapshot>` into `algorithm::decide` and dispatches
the resulting `Decision` (`Launch` / `Focus` / `Cycle` / `ToggleBack` /
`Hide`). The algorithm itself lives in `crates/beckon-linux/src/algorithm.rs`
— that's the only place to change focus / cycle / toggle / hide policy.

Each backend owns:
- the projection from native window data to `WindowSnapshot` (the
  `snapshots_from` helper at the top of every backend file), and
- the translation from `Decision` to native commands.

`recency` semantics in `WindowSnapshot`:
- Hyprland: `focusHistoryID` straight through (0 = currently focused).
- X11: inverted index into `_NET_CLIENT_LIST_STACKING` (top of stack → 0).
- sway / i3: tree traversal index — degenerates to "first match wins" since
  the tree carries no real focus history. The `algorithm::decide` ties on
  recency are broken by address, so the deterministic order matches what
  `i3ipc.rs` did before the refactor.

**Target matching is a set, not a string.** `decide` takes an
`algorithm::Target` — every class that counts as the requested app — and
compares case-insensitively. One id shows up under different strings
depending on the client, and the user has no say in which:
`debian-xterm.desktop` is reported as `debian-xterm` by a Wayland-native
client and as `XTerm` by the same app under X11/XWayland. Matching on the
`.desktop` stem alone meant beckon never recognised the running app and
launched another copy on *every* keypress — confirmed live on sway (5 presses,
5 xterms). `desktop::target_classes` builds the set: `.desktop` filename stem
plus `StartupWMClass=`, or the raw id when nothing resolved (which is what
lets beckon focus ad-hoc apps that ship no `.desktop` file).

**Step 5a cycles over a ring ordered by address, not by recency.** Picking
"the least-recent other window of this app" looks right but is a 2-cycle on
every backend whose `recency` is real focus history: focusing a window
promotes it and demotes the one you just left, so the next press goes
straight back and windows 3..N are unreachable. Addresses are the
compositor's own window ids (con_id / stable_sequence / X11 id / Hyprland
pointer) — stable for the window's lifetime and ordered by creation — so
rotating over them visits every window exactly once per lap. Verified live
on sway: three `foot` windows, seven presses, `35 → 36 → 37 → 35 → …`.

### Phase 1b.x11 X11 generic implementation note

`crates/beckon-linux/src/x11.rs` covers every EWMH-compliant X11 desktop —
GNOME-X11, KDE-X11, XFCE, openbox, awesome, fluxbox. (i3 has its own faster
path through `i3ipc.rs`.)

- **Connection**: `x11rb::connect(None)` — pure-Rust, no `libxcb` link.
  The connection lives for the life of `X11Backend` (one beckon invocation
  is one connection — no daemon).
- **Window list**: `_NET_CLIENT_LIST_STACKING` on root, reversed so index 0
  is the topmost window (≈ most-recently focused). Windows without a
  `WM_CLASS` are filtered out — they're typically transient chrome
  (notifications, menus) that beckon shouldn't surface as "apps". So are
  windows whose `_NET_WM_WINDOW_TYPE` is neither NORMAL, DIALOG nor UTILITY:
  panels and docks (tint2, xfce4-panel) do carry a `WM_CLASS`, and letting
  one through makes step 5b "toggle back" to a panel the WM then refuses to
  focus — beckon reports success and nothing moves. A window with no
  `_NET_WM_WINDOW_TYPE` at all is treated as NORMAL, per EWMH.
- **Class matching**: `WM_CLASS[1]` (the second NUL-separated token, the
  "class" component), matched case-insensitively against the candidate set
  (`StartupWMClass=` first, then the `.desktop` filename stem). Case matters
  in practice: `xterm.desktop` has no `StartupWMClass` and the window
  advertises `XTerm`, so a byte-wise compare launched a new xterm on every
  press.
- **Active window**: `_NET_ACTIVE_WINDOW` root property; treats `0` as None.
- **Focus**: `_NET_ACTIVE_WINDOW` ClientMessage to root with source = 2
  (pager/taskbar). Source 2 is what `wmctrl -a` sends and what most WMs
  treat as a legitimate user action — bypasses focus-stealing prevention.
- **Hide**: ICCCM `WM_CHANGE_STATE` ClientMessage with `IconicState` (3).
  Universal across X11 WMs. We deliberately don't toggle
  `_NET_WM_STATE_HIDDEN` — that's spec'd as a hint the WM sets, not a
  client-driven toggle.
- **Restore from hidden**: an explicit map, then a wait, then the focus
  request — `ensure_mapped` in `x11.rs`. The old claim here was that EWMH's
  "the WM SHOULD bring the window forward" means every WM de-iconifies on a
  focus request. **openbox does not.** Measured on Ubuntu 26.04 + Xvfb +
  openbox: after beckon's own step-5c hide, `_NET_ACTIVE_WINDOW` alone left
  the window at `WM_STATE = Iconic` indefinitely — the hotkey could never
  bring it back and the window was stranded for good. ICCCM §4.1.4 is the
  portable answer: map the window to return it to `NormalState`; the WM holds
  SubstructureRedirect on the root, so the MapRequest reaches it.
  The wait is the other half and is not optional: the WM is just another
  client, so flushing the MapRequest only proves the *server* saw it. Sending
  the activation in the same breath lost the race every time, while the same
  map-then-activate pair issued as two separate `xdotool` calls always worked.
  `ensure_mapped` polls `map_state` (server state, unlike the WM-owned
  `WM_STATE`) for up to 400 ms. Only the restore path pays it; a normal focus
  costs one round-trip and no sleep.
- **Launch**: `/bin/sh -c "setsid -f <Exec> >/dev/null 2>&1"`. `setsid -f`
  detaches from beckon's process group so the launched app survives beckon
  exiting. Stdout/stderr nulled to prevent stale fds keeping the parent
  terminal alive when invoked from a hotkey.
- **No focus-history MRU on X11**: `_NET_CLIENT_LIST_STACKING` already
  reflects z-order, which is the closest standardised proxy for MRU
  (focused windows rise to the top). No state file is needed for step 5a
  cycling. Step 5b still consults the cross-backend MRU file at
  `$XDG_RUNTIME_DIR/beckon-mru` so toggle-back lands on the same app the
  user actually came from across multiple beckon invocations.

### Phase 1d GNOME Wayland implementation note

`crates/beckon-linux/src/gnome.rs` is a thin zbus client. The actual window
work happens inside `extensions/beckon@xom11.github.io/extension.js`, which
runs as a GNOME Shell extension (so it has direct access to Mutter via
`global.display`, `global.get_window_actors()`, `Main.activateWindow`).
Without an in-process collaborator there's no path at all on GNOME Wayland —
Mutter has no public protocol for external focus.

- **Bus surface** (`org.gnome.Shell` / `/com/github/xom11/beckon` /
  `org.gnome.Shell.Extensions.Beckon`):
    - `ListWindows() → a(tssbu)` — `(stable_seq, class, title, focused, monitor)`,
      MRU-ordered (`Meta.TabList.NORMAL_ALL`).
    - `GetFocusedWindow() → t` — `0` when no focus.
    - `ActivateWindow(t) → b` — calls `Main.activateWindow`, which switches
      workspace, unminimizes, raises and focuses in one shot. Mutter's own
      timestamp is used so focus-stealing prevention doesn't reject it.
    - `MinimizeWindow(t) → b` — `meta_window.minimize()`.
    - property `Version` — read at startup by the Rust client to verify the
      extension is loaded before trusting any other call.
- **Window identity**: `MetaWindow.get_stable_sequence()`. `uint32` that
  fits in the `t` (uint64) D-Bus type, stable for the window's lifetime,
  available on every supported GNOME version (no need for the newer
  `get_id()` API).
- **Class fallback ladder**: `get_wm_class()` → `get_gtk_application_id()`
  → `get_sandboxed_app_id()`. Wayland-native GTK apps frequently lack
  `WM_CLASS` and only set the GTK app id (`org.gnome.Console` etc.).
- **Recency**: `Meta.TabList.NORMAL_ALL` is exactly the order alt-tab walks,
  i.e. real focus history. The shared algorithm reads it via
  `WindowSnapshot.recency` (lower = more recent), so step 5a/5b behave the
  same as on Hyprland.
- **MRU file**: shares `$XDG_RUNTIME_DIR/beckon-mru` with the other Linux
  backends. Cross-backend sharing is safe — only one compositor runs at
  a time.
- **Launch path**: same `/bin/sh -c "setsid -f <Exec>"` recipe as the X11
  backend. Doesn't need to go through the extension because spawning a
  new process isn't what Mutter is gating.
- **Hot path cost**: 1 D-Bus connection (~10 ms) + 1 `ListWindows` round-
  trip + 1 `ActivateWindow`/`MinimizeWindow` round-trip. Each call is
  ~1 ms over the session bus, well under the 50 ms budget.

#### Installing / updating the extension

**Declarative (recommended, nix users)**: the flake exposes
`packages.<system>.beckon-gnome-extension` and the same name on
`overlays.default`. The package puts the extension at
`$out/share/gnome-shell/extensions/beckon@xom11.github.io/`. Drop it into
home-manager via `xdg.dataFile`:

```nix
# in your home-manager config (only needed on GNOME hosts)
xdg.dataFile."gnome-shell/extensions/beckon@xom11.github.io".source =
  "${pkgs.beckon-gnome-extension}/share/gnome-shell/extensions/beckon@xom11.github.io";
```

Plus add `"beckon@xom11.github.io"` to dconf `org/gnome/shell.enabled-extensions`
so gnome-shell turns it on at session start. After the first
`home-manager switch`: log out and back in (Wayland can't reload shell
live). Subsequent updates that change the extension code also need a
relogin; updates that change only beckon-cli do not.

**Manual (one-shot, useful for testing extension changes)**:

```sh
cd extensions
gnome-extensions pack beckon@xom11.github.io
gnome-extensions install --force beckon@xom11.github.io.shell-extension.zip
gnome-extensions enable beckon@xom11.github.io
# Wayland: log out and back in. (`busctl ... ReloadExtension` is gated on
# unsafe-mode and not available in normal sessions.)
```

`gnome-extensions install` writes a real directory under
`~/.local/share/gnome-shell/extensions/`. If you later switch to the
declarative path, remove that directory first — home-manager's symlink
activation refuses to clobber an unmanaged file.

### Phase 1e KDE Wayland implementation note

`crates/beckon-linux/src/kde.rs` is the KDE counterpart of `gnome.rs`, with
one big difference: **there is nothing for the user to install.** KWin ships
its own scripting engine and exposes it on the session bus, so beckon loads
a generated script, gets the answer back, and unloads it.

- **Bus surface** (`org.kde.KWin` / `/Scripting` /
  `org.kde.kwin.Scripting`): `loadScript(path, pluginName) → i`, `start()`,
  `unloadScript(pluginName) → b`, `isScriptLoaded(pluginName) → b`.
  `isScriptLoaded` is the startup probe — read-only, and it proves both that
  KWin owns the name and that the scripting object is at the expected path.
- **Why not a Wayland protocol.** KWin advertises neither
  `zwlr_foreign_toplevel_management_v1` (wlroots-only) nor its own
  `org_kde_plasma_window_management`. Confirmed with `wayland-info` against
  `kwin_wayland 6.6.6`: the latter is simply not in the registry, so a
  protocol client cannot enumerate windows at all. Scripting is the only
  surface that exists in practice.
- **Getting data back out.** KWin scripts have no file I/O; `callDBus` is
  the only escape hatch. beckon therefore serves a one-method interface
  (`com.github.xom11.beckon.KWin.Windows`) on its own connection, bakes its
  unique bus name (`:1.42`) into the generated script, and blocks on an
  `mpsc` channel until the script calls back. Baking in the *unique* name
  rather than a well-known one is what keeps two concurrent beckon
  invocations from reading each other's replies.
- **Two script round trips per invocation**: one to read the window list,
  one to act. The read cannot be merged with the act because the decision
  needs the list first, and a script is fire-and-forget — it cannot wait for
  a reply from us.
- **Window identity**: `Window.internalId`, a QUuid rendered `{xxxxxxxx-…}`.
  Stable for the window's lifetime, which is all the algorithm needs — but
  unlike every other backend's address it is **not numeric**, so
  `algorithm::cmp_address` falls back to byte ordering. The step-5a cycle
  ring is therefore stable but not in window-creation order on KDE.
- **Recency**: `workspace.stackingOrder` reversed (topmost first), falling
  back to `workspace.windowList()` on builds without the property. Same
  stacking-as-MRU proxy the X11 backend uses.
- **Window filter**: `normalWindow && !skipTaskbar && resourceClass != ""`.
  Plasma's panels and desktop are windows KWin refuses to activate, so
  letting one through would make step 5b toggle to something that never
  takes focus — the same class of bug the X11 backend's
  `_NET_WM_WINDOW_TYPE` filter prevents.
- **Restore before focus**: the act script sets `w.minimized = false` before
  assigning `workspace.activeWindow`. Assigning active on a minimized window
  is not documented to restore it, and the X11 backend already taught us not
  to assume a focus request de-iconifies.
- **Script source is generated, so values are escaped** (`js_quote`). Window
  ids are KWin-minted UUIDs and the bus name is a unique name, so neither can
  realistically contain a quote today — but building source by concatenation
  without escaping is how that stops being true.
- **Hot-path cost, measured on the headless VM**: `beckon <id>` 7–41 ms
  (median ~15), `beckon list` 5–6 ms. Comfortably inside the 50 ms budget, and
  cheaper than both macOS (~95–105 ms) and Windows (~57 ms).

Testing: `kwin_wayland --virtual` runs headless with no GPU at all — see
`testing/README.md`. All 19 live tests pass there.

### Phase 1c Hyprland implementation note

`crates/beckon-linux/src/hyprland.rs` talks to the compositor via the request
socket directly — no `hyprctl` shell-out, no `hyprland-rs` dep. Two queries
(`j/clients`, `j/activewindow`) per invocation, parsed with `serde_json`.
Window identity uses Hyprland's `class` field, which is set from Wayland
`app_id` for native clients and from `WM_CLASS` for XWayland — one field, no
fallback ladder.

- **Socket path**: `$XDG_RUNTIME_DIR/hypr/$HYPRLAND_INSTANCE_SIGNATURE/.socket.sock`
  (Hyprland 0.40+) with `/tmp/hypr/<sig>/.socket.sock` as fallback for older
  installs. Each request opens a fresh `UnixStream` — Hyprland closes the
  socket after responding.
- **Cycle order (5a)** is the shared address-ordered ring in
  `algorithm::decide`, *not* `focusHistoryID`. This entry used to say
  "pick the same-app window with the lowest non-current `focusHistoryID`",
  which describes code that was deleted: because `focusHistoryID` is real
  focus history, focusing a window promotes it to 0 and demotes the one just
  left, so that ring is a 2-cycle and windows 3..N are unreachable. Verified
  live on Hyprland 0.56.0 — three `foot` windows, six presses, the ring walks
  all three and laps.
- **Hide (5c)**: `dispatch movetoworkspacesilent special:beckon,address:0xN`,
  and **coming back out is beckon's job** — `focus_window` moves a window off
  `special:beckon` before focusing it. This entry used to claim `dispatch
  focuswindow` alone was enough because "Hyprland surfaces the window's
  workspace on focus". Measured on 0.56.0, that is wrong in the way that
  matters: the special workspace is *shown* as an overlay, but the window
  keeps belonging to it, so the moment focus moves elsewhere it disappears
  and `$mod+1..4`, `movefocus` and `movetoworkspace` all behave as if it does
  not exist — only `beckon <id>` could surface it again. sway does not have
  this problem because `focus` on a scratchpad container runs
  `root_scratchpad_show`, which re-parents it onto the workspace the user is
  looking at. The same bug also made the *second* hide a silent no-op
  (`movetoworkspace` early-returns when the window is already there) while
  beckon reported `Hidden`. Only `special:beckon` is unparked; a user's own
  `special:*` workspace is left where they put it.
- **No MRU state file (5b)**: unlike every other Linux backend, this one
  passes `previous_app = None` to `decide`. `$XDG_RUNTIME_DIR/beckon-mru`
  exists because the sway tree carries no focus history; `focusHistoryID` is
  real MRU and — measured on 0.56.0 — reorders on focus changes beckon never
  made, including mouse clicks and native binds. Consulting a file that only
  records beckon's own actions could only make step 5b less accurate.
- **Window filter**: `list_clients` drops clients with an empty `class` and
  those with `hidden = true` (Hyprland sets it on windows it deliberately
  keeps off screen, e.g. terminal swallowing). It must **never** filter on
  `visible`: measured on 0.56.0, a group tab that is not on top reports
  `hidden=false, visible=false`, so filtering there would hide every tab but
  the front one and break step 5a through a tabbed group. Windows parked on
  `special:beckon` stay in the list on purpose — drop them and the next
  keypress launches a duplicate instead of bringing the window back.
- **Decision logic** lives in `algorithm::decide`, shared with every other
  Linux backend; `hyprland.rs` owns only the `Client` → `WindowSnapshot`
  projection and the `Decision` → dispatch translation.
- **No `hyprctl` dep**: keeps the hot path at a single short-lived socket
  connection per query, and works in containers/Nix builds where `hyprctl`
  may not be on PATH.

## Reference implementations to port from (phase 2 / 3)

When porting beckon to macOS and Windows, mirror the logic in the existing
hand-rolled scripts. Both already handle the "is the app open?" → focus / launch
flow; beckon's job is to add Name resolution against OS metadata, plus the
cycle / toggle-back / hide algorithm.

### macOS — Hammerspoon spoon

`~/.nix/home-manager/dotfiles/hammerspoon/MySpoons/LaunchApp.spoon/init.lua`

What it does today:
- Takes app **display name** (e.g. `"Claude"`).
- `hs.osascript.applescript('id of app "Claude"')` → bundle_id (free name resolution!).
- `hs.application.launchOrFocusByBundleID(bundleID)` to focus / launch.
- If already on this app: walk `hs.window.orderedWindows()` (MRU), focus first window of a *different* app; else hide.

What beckon should add:
- Replace `osascript` shell-out (~50ms) with native `objc2-app-kit` (`NSWorkspace.runningApplications`, `NSRunningApplication.activate`).
- Add step 5a (cycle within same app) — Hammerspoon skipped this.
- Use `CGWindowListCopyWindowInfo(.optionOnScreenOnly)` for z-order → free MRU, no state file needed (unlike Linux).
- **Accessibility permission required**. Detect via `AXIsProcessTrusted()` and surface a clear message in `beckon doctor` if missing.

### Windows — AHK script

`~/.nix/windows/ahk/launch-app.ahk`

What it does today:
- Takes a **window title** (e.g. `"Claude"`) plus an **exe path / shortcut path** as separate args:
  ```
  Launch(browser, "Claude", " --app=https://claude.ai/new")
  ```
- `WinExist(winTitle)` to check, `WinActivate` to focus, `Send("!{Esc}")` to hide.
- Browser PWAs: launches via `--app=URL` against Vivaldi.

Pain points beckon should fix:
- Title-based matching is brittle: PWAs are titled after the page, not the app — unloaded tabs break it.
- Two arguments (winTitle + launch cmd) means each binding repeats the launch URL.

What beckon should do:
- Resolve Names against Start Menu shortcuts (`%APPDATA%\Microsoft\Windows\Start Menu\Programs\*.lnk`) — read the `.lnk` target to get exe + args. This mirrors Linux `.desktop` resolution.
- For PWAs: detect via shortcut argument pattern (`--app=URL` or `--app-id=`) and match by AppUserModelID once running.
- Match running windows by AppUserModelID (preferred) or `WM_CLASS` equivalent via `GetClassName()`.
- z-order from `EnumWindows` gives MRU directly → no state file needed.
- **Anti-focus-stealing**: Win10+ requires `AllowSetForegroundWindow(GetCurrentProcessId())` or a foreground-lock workaround (the `AttachThreadInput` trick) before `SetForegroundWindow`. Search nixpkgs / GitHub for "windows allow set foreground rust" — this is well-trodden.

### Cross-OS dotfile shape after phase 2/3

Same Name everywhere, OS-canonical id only when Names collide:

```
# sway      (Linux)
bindsym $cap+c exec beckon Claude
# Hammerspoon (macOS)
hs.hotkey.bind(hyper, "c", function() hs.execute("beckon Claude") end)
# AHK         (Windows)
^#!c:: Run("beckon Claude")
```

## Known constraints

### Wayland hotkey
On every Linux target, the compositor / DE binds the key and `exec beckon`s.
That is the shape of the integration, and `serve` is not offered here.

This entry used to read *"Wayland has no standard global hotkey API […]
There is no app-level workaround."* That is not accurate and was leading
sessions to conclude Linux resident mode is technically impossible. There
**is** a standard — `org.freedesktop.portal.GlobalShortcuts` — plus
per-desktop routes that predate it. Surveyed 2026-08, **from documentation
only; none of this has been built or run against beckon**:

| Environment | Route | State |
|---|---|---|
| X11 (i3, openbox, XFCE, GNOME-X11, KDE-X11) | `XGrabKey` on root — what sxhkd / xbindkeys do; beckon already links `x11rb`, which exposes `grab_key` | available |
| KDE Wayland | KWin script `registerShortcut`, same engine `kde.rs` already drives via `loadScript`; or the portal | available, two routes |
| Hyprland | GlobalShortcuts portal via `xdg-desktop-portal-hyprland` | available |
| GNOME Wayland | the bundled extension could `addKeybinding` / `grab_accelerator`; the portal route is unreliable (Mutter ≥ 49 dropped XWayland-side key grabs) | awkward |
| **sway** | wlroots has not implemented the GlobalShortcuts portal — still under discussion | **no route** |

So "impossible" holds for exactly one compositor, and it is the one where
`bindsym` is easiest. The reasons this stays out of scope are different,
and they are what to re-read before anyone reopens this:

1. **No single API.** macOS is one call, Windows is one call, Linux would be
   four separate implementations — comparable to the cost of the entire
   existing Linux backend layer, for one feature.
2. **The portal model does not carry the shortcuts TOML.** An app asks for a
   shortcut *by name* and the **user** assigns the keys in the compositor's
   own UI — deliberate, per the Wayland security model. `"ctrl+super+alt+t" =
   "kitty"` has nowhere to go, so a Wayland `serve` would be a different
   feature wearing the same name.
3. **Negative value.** Every environment in the table already ships a place
   to bind a key to a command. `serve` exists because macOS and Windows do
   not.

### GNOME / KDE Wayland focus restrictions
Mutter (GNOME) and KWin (KDE) block external processes from focusing arbitrary windows on Wayland — this is by design (Wayland security model).

- **GNOME Wayland**: supported via the bundled shell extension at
  `extensions/beckon@xom11.github.io/`. The extension runs inside
  gnome-shell, so it bypasses the external-focus restriction by being
  internal. The Rust client talks to it over the session bus. Install once
  with `gnome-extensions install --force` + `enable`, then log out / log
  back in (Wayland can't reload shell live).
- **KDE Wayland**: supported via KWin's own scripting engine — see the
  Phase 1e note. Nothing to install: `org.kde.kwin.Scripting` is part of
  KWin, so beckon loads a generated script, gets the answer back through
  `callDBus`, and unloads it.

  This entry used to read *"KWin doesn't have an equivalent extension API
  surface that we can ride on"*. That was wrong, and was falsified by
  running the thing: on `kwin_wayland 6.6.6` a loaded script enumerated
  every window with `resourceClass` / `caption` / `minimized` and moved
  focus by assigning `workspace.activeWindow`. Do not re-add the claim
  without re-testing it.

  Note what is *not* available and why the script route is the only one:
  KWin advertises neither `zwlr_foreign_toplevel_management_v1` (wlroots
  only) nor its own `org_kde_plasma_window_management` — the latter is
  absent from the registry on a plain `kwin_wayland`, so a Wayland-protocol
  client cannot enumerate windows even though the protocol exists on paper.

### Caps Lock as the beckon key (Windows) — the LLHOOK exception

`keyboard.caps = true` installs a `WH_KEYBOARD_LL` hook. That **reverses**
the decision recorded under *Open questions → 1* that beckon uses
"RegisterEventHotKey / RegisterHotKey: no event tap, no LLHOOK". The reversal
is deliberate and narrow: off by default, on one OS. **Not "one opt-in
feature" any more** — that was true until 2026-08-12; the next paragraph is
why it no longer is.

**Since 2026-08-12 there are TWO reasons to hold that hook, not one**: Caps,
and a settings-window chord capture (see *Out of scope → GUI/TUI*). There is
still exactly one hook — `capture::HookOwners` refcounts the two reasons, and
`hook_proc` consults the capture arm **first** — but the exception is now
reachable on a machine where the user left `keyboard.caps = false`, for the
seconds a recording lasts. Everything below still holds for the Caps arm; the
capture arm's own rules are in the *Out of scope* entry.

Caps is an **alias for the configured chord** — `ctrl+super+alt` by
default, `keyboard.caps_hold` to change it — not a fifth modifier. The hook
injects the chord `RegisterHotKey` already listens for, so `Combo`,
`parse_shortcuts` and `register_all` are untouched and the config file is
identical on a machine with the tick and one without. Decisions live in
`beckon_core::caps::decide` (pure, tested on all three CI jobs);
`beckon-windows/src/caps_hook.rs` only translates `KBDLLHOOKSTRUCT` to
`SendInput`.

Two hazards are removed by construction, not guarded against, and both are
easy to reintroduce by "simplifying":

- **The chord is injected as one burst.** Holding `ctrl+win+alt` down across
  real time would make a bare Caps tap press and release Win alone — the
  gesture that opens the Start menu.
- **Only keys bound to the chord are injected for.** Otherwise
  `Caps+<anything>` becomes a genuine `ctrl+win+alt` chord the shell may act
  on.

**The hook must never call `backend.beckon()`.** A callback that outruns
`LowLevelHooksTimeout` (300 ms default) is silently unhooked by Windows with
no error anywhere, and `backend.beckon()` measured ~57 ms typical / ~945 ms
on the miss path. The alias design keeps the callback at a hash lookup plus
one `SendInput` — **13 ms cold, 5.2 ms warm, measured on a14**, so 2–4 % of
budget. (An earlier estimate of "microseconds" was wrong by three orders of
magnitude; the headroom is real but it is not unlimited, so nothing else
belongs in that callback.) The real work happens later on the ordinary
`WM_HOTKEY` path.

**Measured on a14 2026-08-11, not reasoned:** an injected chord does fire
our own `RegisterHotKey`; the one-burst chord does not open the Start menu
(verified against a control that proved a bare Win tap does — without that
control a blind detector and a clean result are indistinguishable); an
injected `VK_CAPITAL` flips the toggle, so `caps_tap = "capslock"` is
implementable; and end-to-end, `Caps+N` focused Notepad with `serve` running
and did nothing without it.

Known gaps, documented in the README rather than hidden:

- **UIPI.** beckon runs at normal integrity, so the hook never sees keys
  while an elevated window has focus; Caps silently does nothing there. The
  typed `ctrl+super+alt+t` chord **does** still work, because `RegisterHotKey`
  is not subject to UIPI — there is always a fallback. Both halves measured
  by hand on a14 2026-08-11 with Task Manager elevated and focused, against
  a normal-window control run first. This was documentation-only for a day
  and is now not.
- **Other remappers.** kanata / PowerToys / AHK claiming Caps means beckon
  never sees it. Detection is unreliable; documented, not guessed.
- **EDR.** A low-level keyboard hook is the classic keylogger signature.

Pausing must never leave Caps able to swallow a keystroke. That used to mean
`set_paused(true)` unhooks outright — true while Caps was the only reason to
hold the hook, no longer guaranteed now that capture can hold it too:
`sync_caps_hook`'s `uninstall_for(HookReason::Caps)` (`serve.rs:869`) drops
only the Caps reason, and the HHOOK survives if a capture also owns it. What
actually makes pausing safe is `clear_bindings()`, called first: it zeroes
`Config::wanted`, and `hook_proc`'s `!c.wanted && st.at_rest()` arm passes
every event straight through once nothing swallowed is still owed a matching
up — installed or not, a paused hook eats nothing.

### macOS Accessibility permission
Required to focus arbitrary apps. Permission is bound to the codesigned binary identity — rebuilding the binary may invalidate it and require re-granting in System Settings.

### PWA handling
PWAs must be **installed as standalone apps** (Brave/Chrome → "Install this site as an app") so each gets a stable bundle ID / `.desktop` / `WM_CLASS`. beckon does NOT handle `--app=URL` invocations — that approach is too brittle to detect/focus reliably.

### Per-OS identifier asymmetry
Names typically resolve consistently across OSes (`Claude` works on Linux/macOS/Windows). Where they don't — e.g. macOS app display name is localized, or two apps share a `Name=` on Linux — users fall back to a canonical OS id (bundle_id / .desktop filename / exe). Discovery via `beckon search <name>` per machine.

### PWA hash drift (Brave / Chrome)
PWAs installed via Brave/Chrome get an extension hash inside their `.desktop` filename (Linux) or bundle_id (macOS) — e.g. `brave-fmpnliohjhemenmnlpbfagaolkdacoja-Default`. **The hash is generated locally during install and differs across machines**, so canonical ids can't be synced via dotfile copy. The Name field, however, is stable: `Name=Claude` on every machine. **This is the primary reason Name-based resolution is the recommended id format.** `beckon resolve <id>` reports "no match" with fuzzy suggestions when a stale canonical id appears in a dotfile.

## Open questions (decide in implementation session)

1. **Daemon vs one-shot CLI**
   Decided: **one-shot for the hot path, plus an opt-in resident mode.**
   `beckon <id>` stays a one-shot CLI (~10ms cold start) for compositor-bound
   hotkeys (sway/GNOME). `serve <config>` (2026-08) additionally hosts the
   hotkeys itself on macOS/Windows — where no compositor binds keys for us —
   reading a flat TOML (`"ctrl+super+alt+t" = "kitty"`), watching it for
   reloads. Hotkey registration uses RegisterEventHotKey / RegisterHotKey:
   no event tap, no LLHOOK, so no new TCC prompt for the hotkey half and no
   interference with kanata's hook ordering.

   beckon itself never daemonizes, and that is a decision, not an omission.
   Surveyed skhd, yabai, espanso, kanata, AutoHotkey and caddy: effectively
   no hotkey daemon forks. On macOS a detached process loses the login
   session's bootstrap namespace — beckon already needs
   `TransformProcessType(→ UIElement)` because a launchd-spawned process has
   no window-server identity, and without one `RegisterEventHotKey` returns
   success while never delivering a press. On Windows there is no `fork` at
   all. Above all it solves the wrong problem: forking buys "survives
   closing the terminal", while what users need is "starts at login" and
   "restarts if it dies" — both of which still require launchd / Task
   Scheduler afterwards. The ergonomic step that *is* open is an
   install/start/stop lifecycle (the skhd / espanso pattern); not built, and
   its shape is not decided. The growth rule in *CLI surface* rules out a
   top-level `service` verb, which leaves two candidates — flags on the
   existing verb (`serve --install`, which reads oddly for an operation that
   installs a launchd agent rather than serving) or subcommands nested under
   it (`serve install`, which reads correctly and costs no top-level name,
   but puts a positional `<CONFIG>` and a subcommand on the same level and so
   inherits the `(Some, Some)` problem documented above, one level down).
   Pick one when it is actually built.

   **Windows answer, 2026-08 (`beckon-serve.exe`):** neither. A checkable
   "Start with Windows" row in the tray menu writes the
   `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` value directly —
   no new verb, no new flag — so the growth rule in *CLI surface* never had
   to be spent. `beckon.exe serve <CONFIG>` now shares the tray menu too,
   minus this row; the autostart lifecycle still lives entirely inside the
   GUI-subsystem binary. See
   `docs/superpowers/specs/2026-08-10-windows-serve-app-design.md` §6.

2. **MRU tracking source per backend**
   Step 5b (toggle-back) on Linux uses a single-app state file at
   `$XDG_RUNTIME_DIR/beckon-mru` containing the `app_id` focused before
   the most recent beckon action — **except on Hyprland**, which has real
   focus history in `focusHistoryID` and therefore reads and writes nothing
   (see the Phase 1c note). Each invocation reads the live focus
   from IPC, so transitions made by mouse / native hotkeys reconcile on
   the next beckon call. Limitation: only beckon-mediated focus changes
   are recorded; a sequence of mouse-only switches between beckon calls
   produces a stale "previous". Acceptable for the hotkey-driven workflow.
   macOS / Windows can read z-order directly (`CGWindowList` /
   `EnumWindows`) so they likely won't need a state file at all.

3. **Notification on errors**
   Decided: **auto-detect TTY**. If stderr is not a terminal (typical hotkey-bound invocation), beckon fires a desktop notification in addition to the stderr line. Linux uses `notify-send` (best-effort: silent if absent). macOS will use `osascript display notification`; Windows will use a toast — both pending phase 2/3.

4. **`search` scope and ranking**
   Should `beckon search claude` match against window titles too, or only app id / app name? Title match is more forgiving but volatile. Default likely: id + name only, `--include-titles` opt-in.

## Crate dependencies

```toml
# core / cli
anyhow    = "1"
thiserror = "2"
clap      = { version = "4", features = ["derive"] }

# macOS (in use as of phase 2)
objc2            = "0.6"
objc2-foundation = "0.3"   # NSString / NSURL / NSArray / NSDictionary
objc2-app-kit    = "0.3"   # NSWorkspace / NSRunningApplication
core-foundation  = "0.10"  # CF lifetime wrappers (CFType / CFArray / CFString)
plist            = "1"     # parse .app/Contents/Info.plist
# AX (Accessibility API) and CGWindowList — hand-rolled `extern "C"`
# bindings in `crates/beckon-macos/src/ffi.rs`. Surface is ~6 functions, not
# worth dragging in objc2-application-services.

# windows (in use as of phase 3)
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

# linux (in use as of phase 1)
swayipc    = "3"      # sway + i3 (same protocol)
serde      = "1"      # serde_json for Hyprland JSON IPC payloads
serde_json = "1"
x11rb      = "0.13"   # any EWMH-compliant X11 DE (GNOME-X11, KDE-X11, XFCE, ...)
zbus       = "4"      # session bus client for the GNOME Shell extension bridge
# Future:
# freedesktop-desktop-entry = "0.7"    # currently we parse .desktop ourselves

# resident mode (check / serve, since 2026-08)
toml      = "0.8"    # beckon-core: parse the shortcuts file
toml_edit = "0.22"   # beckon-core: WRITE it back, keeping comments. Already
                     #   a transitive dep of toml 0.8, so it costs nothing.
notify    = "6"      # beckon-cli:  watch it for live reload
fs4       = "0.8"    # beckon-cli:  flock, one serve per config path
```

**The config path is canonicalised once and then SIMPLIFIED**, in
`cmd_serve_app` (`serve.rs`). `Path::canonicalize` on Windows is
`GetFinalPathNameByHandleW` and always returns `\\?\C:\…`; that spelling
reached the startup log, the `Open config file` tooltip, the System page's
config row — where `SS_PATHELLIPSIS` elides from the MIDDLE, so the prefix is
the part that never shortens — and, less visibly, `ShellExecuteW` and
`explorer.exe /select,`, which are the classic non-acceptors of it.
`beckon_core::paths::plain` undoes it at the origin, conservatively: a volume
GUID path, a UNC with no share, or any component Win32 would rewrite once
unprotected (trailing dot or space, `.`/`..`, a DOS device name) keeps the
prefix, because the verbatim form is what every Win32 call accepts.
**Deliberately NOT applied inside `lockfile::acquire`**, which canonicalises
independently one line earlier and hashes the result into the lock file's
NAME — renaming the lock would let an old and a new binary both serve. Long
paths are unaffected: the manifest declares `longPathAware`.

The **only** file beckon reads is the `serve` shortcuts TOML — and since the
settings window, the only file it writes. There is still
no config for `beckon <id>` itself and no resolve cache — ids resolve against
OS metadata on every call.

**CORRECTED 2026-08-15: there is now a second store, and it is deliberately
not a file.** The settings window's System page (design §3.3) keeps the
window's own look — `DarkMode` and `Opacity` — in
`HKCU\Software\beckon`, two `REG_DWORD`s written by
`crates/beckon-windows/src/prefs.rs`. That is the split the Four Doors design
asks for: Shortcuts and Keyboard write `apps.toml`, System writes the
registry, the Run key, or nothing. The split is what makes a theme switch keep
working when `apps.toml` does not parse, which is the one state a user most
needs a GUI in. The sentence above still holds as written — a *file* is still
only ever the shortcuts TOML — and the hot path (`beckon <id>`) reads neither.

**One more read, added 2026-08-15 and not a config: `current_exe()` and a
`stat` of it.** The About page (design §3.4) shows the RUNNING IMAGE's path
and compares the file's modification time against this process's own start
time. That is a read of beckon's own binary, not of anything a user writes,
and it exists because a recorded failure had every obvious surface lying: a
watchdog-started beckon on a14 ran the 0.8.0 image for three hours while
`beckon --version` said 0.9.0 and scoop's `current` junction pointed at 0.9.0.
The path is deliberately **not** resolved through `GetFinalPathNameByHandleW`
— resolving reports today's junction target, which is the surface that lied.

## Out of scope (explicitly)

- **Config for the hot path / app aliases** — `beckon <id>` resolves against OS metadata (`.desktop` / LaunchServices / Start menu) directly. No `[apps.claude]` mapping, no resolve cache. The `serve` TOML is a *hotkey table*, not a place to alias ids.
- **Global hotkey registration on Linux** — handled by the compositor / WM dotfile (sway config, Hyprland, GNOME/KDE Settings → Custom Shortcuts). Out of scope by choice, *not* for lack of an API: routes exist on X11, KDE, Hyprland and GNOME (sway is the one gap) — see *Known constraints → Wayland hotkey* for the survey and the three reasons. On macOS / Windows this is *in* scope and shipped: `serve` registers via RegisterEventHotKey / RegisterHotKey.
- **GUI / TUI** — CLI only, with one exception, which is `serve`'s control
  surface rather than a launcher: the tray context menu (reload, pause, open
  the log, toggle autostart, quit) and the settings window it opens.

  **CORRECTED 2026-08-16: no longer "Windows-only".** The macOS window has
  all four doors as of the four-doors port, against the same
  `beckon_core::settings` contract — `Page`, `ControlState`, `Callbacks`,
  `SettingsCommand`, `row_condition`, `probe_plan`, `command_bar_shown`,
  `caps_view_fold`, `page_plan`. Everything below about *what the window
  decides* is therefore two implementations of one design, and the place to
  change a decision is still `beckon-core`.

  Where macOS deliberately differs, and why each is a platform fact rather
  than a shortfall:

  - **No `Dark mode` row.** Every colour in the AppKit window is a *semantic*
    `NSColor`, so it follows the system between light and dark with no
    control, no stored preference and no repaint path. The Win32 row exists
    because Win32 has no appearance to follow — it needs `theme::pairs` and a
    `prefs.rs` registry value to do what `controlBackgroundColor()` does for
    free. `SystemState::dark` is read and discarded there.
  - **No `Start at login` row**, by design §3.3's own rule that a capability
    this process does not have is omitted rather than greyed: the Homebrew
    formula's `service do` block owns the launch agent, and a switch here
    would be a second writer for a file beckon did not create.
    `SystemState::autostart` is `None`, which is the field's documented way
    of saying exactly that.
  - **`NSSegmentedControl` for the tab strip**, not four hand-drawn pills.
    This closes a deviation rather than inheriting one — the design's own
    drawing shrink-wraps the trough around the pills, and Windows fills the
    band instead because hugging needs a width only its layout pass computes
    — and it brings the contrast, focus ring and keyboard story each Win32
    pill state needed its own measurement to get right.
  - **About draws an Accessibility row where Windows draws
    `HOOK_DISCLOSURE`.** With no `CGEventTap` built, *"the keyboard hook is
    installed only while…"* is vacuously true while telling the reader a
    keyboard hook is part of the program — worse than silence, on the one
    page whose job is disclosure. The Accessibility grant is this platform's
    version of the same question, and losing it silently on a rebuild is the
    largest single cause of "beckon does nothing" here.
  - **The Keyboard door's first two groups edit settings macOS does not yet
    honour**, because the Caps alias needs the event tap. The door says so in
    a sentence rather than letting a reader discover it by ticking a box.
    Omitting them was rejected: the config file is shared across machines,
    and hiding settings the file already contains is worse than showing them
    and naming where they take effect.

  **Modifier names are a table now, not literals.**
  `beckon_core::shortcuts::ModifierLabels` — `WINDOWS` is `Ctrl/Win/Alt/Shift`
  and `MAC` is `Ctrl/Cmd/Option/Shift`. `combo_caps`, `combo_caps_folded` and
  `combo_display_folded` keep their signatures and delegate to `*_with(..,
  WINDOWS)`, so no Windows string moved;
  `the_default_labels_are_what_combo_caps_always_produced` pins that. Words
  rather than glyphs (`Cmd`, not `⌘`): the editor's own check boxes read
  `Cmd`, and a cell showing a symbol beside a box showing a word is two names
  for one key on one screen. `key_label` was already neutral —
  **`theme::TransparencyBlock::reason`'s `"Off in Windows settings"` is the
  one string left in core that names a platform**, worked around locally by
  `beckon-macos`'s `block_reason` and wanting the same treatment.

  The macOS window is also where `on_command` is raised for the first time on
  that platform: before the System and About doors, all eleven
  `SettingsCommand` variants were unreachable there, and `open_target` /
  `reveal_target` were `{}` stubs.

  The window shows the shortcut table with per-row registration state,
  edits it, and writes the same TOML back through `toml_edit` so hand
  edits and window edits stay interchangeable. It lists installed apps
  only to fill in a Name while authoring a binding — the job `beckon
  search` already has — and never focuses or launches anything. Design:
  `docs/superpowers/specs/2026-08-11-windows-settings-window-and-caps-design.md`.

  **WIDENED 2026-08-15 — the window is `serve`'s control surface as well as
  its editor.** Design §3.3's System page pauses and resumes the hotkeys,
  reloads the config, toggles `Start with Windows`, sets the window's own
  theme and transparency, and opens or reveals the config and log files. The
  paragraph above is still true of the SHORTCUT table — nothing there
  focuses or launches — and the additions are the tray menu's own commands
  reached from a page rather than a menu: `Pause shortcuts` and `Reload` call
  `serve.rs`'s `set_paused` and `reload`, the same two functions the tray
  calls, through `SettingsCommand`. **They must never be re-implemented in the
  window**: `set_paused` does five ordered things, one of which is CLEARING
  the registration map, and that cleared map is what makes the `paused` status
  word load-bearing on every Shortcuts row.

  `Start with Windows` is **omitted, not greyed**, under `beckon.exe serve`,
  copying the tray's own reasoning — a capability this process does not have
  asks "why is this greyed?" with no answer in the row. The log row is omitted
  the same way when `serve` ran without `--log`. Both decisions live in
  `beckon_core::settings::system_state`, so all three CI jobs test them.

  **WIDENED AGAIN 2026-08-15 — the window also puts text on the clipboard and
  opens a browser.** Design §3.4's About page has three copy buttons and three
  links (`GitHub`, `Releases`, `Report a bug`). Two rules keep those from
  growing into a second control surface:

  - **The copy buttons act in the window and report afterwards**, like the
    theme switch and unlike everything else: `SettingsCommand` is `Copy + Eq`
    and carries no `String` by design, so a caller answering `Copy(Field)`
    would have to rebuild the page's state and be a second author for it.
    `beckon_core::settings::copy_text` is the one decision — **the row's bare
    payload, not the string on screen**, because `Location` shows a verdict
    clause and is shortened by `SS_PATHELLIPSIS`, and a copied path is for
    pasting into Explorer.
  - **The links go through `SettingsCommand::Open(Target::…)`, never a new
    `Callbacks` field**: `beckon-macos/examples/settings_probe.rs` builds
    `Callbacks` as a complete literal with no `..`, so a new field is a hard
    E0063 on a CI job that has nothing to do with this page. The three
    addresses live in `Target::url` in core, where a test can read them, and
    `shell::open_url` refuses anything that is not `https://`.

  **The hook disclosure lives on About**, moved off Keyboard by §3.4: *"The
  keyboard hook is installed only while Caps Lock is on, or while you are
  recording a shortcut. beckon keeps no record of what you type."* An unsigned
  process that holds `WH_KEYBOARD_LL`, calls `SendInput` and writes an autorun
  key owes the reader both halves, and **the second is a negative claim that
  no icon, colour or control state can draw** — which is why it is a sentence
  and why `HOOK_DISCLOSURE` has a test pinning both halves. "While Caps Lock
  is on" means the SETTING, not the lock's LED.

  **Shape: bands stacked top to bottom, not a split pane** (landing 2a,
  `settings_window.rs::layout`). The 45/55 column split it replaced put 561 px of fixed
  columns inside a 482 px pane, so beckon shipped a horizontal scroll bar
  and a clipped App column; widths are now a proportion of the live list
  width, which is why that cannot recur. **App leads, Shortcut follows** —
  the app is what the user is looking for. Per-row `LVS_EX_CHECKBOXES`
  ride in column 0's state image and make Remove a multi-delete: the whole
  decision is `Model::remove_pressed` — **ticks win, the selection is the
  fallback** — because clicking a tick also moves the highlight, so a
  selection-only Remove would delete a row the user never ticked and leave
  the ticked ones behind. `remove_enabled` is `selected.is_some() ||
  marked_count() > 0`. The caption stays the constant `Remove` rather than
  `Remove N`: `layout` sizes buttons from `text_size` of their caption, so a
  live count would be a further `layout` input, and calling `layout` on a
  data push means `SetWindowPos` on the App combo — the measured data-loss
  path. That is not the only route to a live count — reserving width for the
  widest caption at `layout` time and driving the count with
  `SetWindowTextW` alone on pushes would honour it without `layout` or
  `SetWindowPos` — just one not taken this pass, cosmetic gain against no
  hardware time left.
  `Save` (was `Apply`; the id is still `IDC_APPLY`, because
  `examples/settings_probe.rs` hard-codes 1002-1007) is `BS_DEFPUSHBUTTON`
  and is where the default ring RESTS — **not where it stays**.
  `default_button_of` migrates the ring onto whichever push button has
  focus, so Enter saves from the fields, the list and the check boxes, but
  Enter on a tabbed-to `Close` closes and on `Reload` reloads. That is the
  point of two earlier fixes: Enter on a focused `Reload` used to save and
  overwrite the external change the banner existed to protect.

  **CORRECTED 2026-08-15: "Only `Ctrl+S` is unconditional" is no longer true,
  and it was the defect rather than the design.** `Ctrl+S` is an accelerator
  on the WINDOW, so from the System or About door it reached
  `handle_command`'s `IDC_APPLY` arm and wrote `apps.toml` with no Save on
  screen — two doors that write no part of that file. `enabled()` could not
  have stopped it: `apply_enabled` is `dirty && no errors` with no page term,
  and a hidden button is not a disabled one. It is inert on those two doors
  now; the model stays dirty, so the keystroke is waiting one `Ctrl+1` away
  rather than lost.

  **The command bar follows design §1's split by STORE**
  (`beckon_core::settings::command_bar_shown`, from `Page::writes_config`):
  `Save` / `Close` / `Open config file` are drawn on Shortcuts and Keyboard,
  and on neither of the other two. `Close` and `Open config file` go with
  `Save` rather than staying — `Close` is where "discard" lives (the dirty
  prompt is in `WM_CLOSE`) and `Open config file` is a second route to a file
  the System page already lists with its own two glyphs. **The BAND stays on
  all four**: `compute_card_rects` reserves `pad + ctl` whatever the page
  says, so `content_bottom` is one expression with one meaning, and an empty
  bar carried no buttons and nothing else. **Since 2026-08-16 it is not
  empty**: design §6.4's service line (`IDC_SERVICE_LINE`, 1044) is chrome and
  is drawn on all four doors, so the band says whether the hotkeys are working
  from every page. What §6.4 still owes is its right half -- the `Saved`
  readout and `Undo`, both of which belong to §6's auto-save.

  Two consequences that are not obvious and are load-bearing:

  - **`DefaultButton::HOME` is gone, replaced by `home(page) ->
    Option<DefaultButton>`**, and `default_button` returns `Option`. The old
    constant's own doc named this: *"`Save` is on every page — if it ever
    stops being, this early return is the line that breaks."* `None` is a real
    answer, not a missing one: System and About have no primary action, so
    Enter does nothing there until the user tabs onto a button. `NO_DEFAULT`
    (0) is the id that carries "nowhere" through `Ui::defid`.
  - **`repair_default_button`'s successor is page-dependent.** It named
    `IDC_CLOSE` unconditionally — "always present" — which is now a hidden
    control on half the doors, and a repair that moves focus onto a hidden
    control is the exact fault it exists for. It falls back to the open door's
    own pill, which is `show_page`'s successor and is chrome.

  Every door keeps two ways out: the caption `X` is chrome, and Escape arrives
  as `IDCANCEL` from the dialog manager rather than from the button, so hiding
  `Close` does not disarm the key.

  **CORRECTED 2026-08-15 (branch `four-doors-phase-0`): the band list above
  and the list's row count were both wrong, and each was wrong in its own
  way.**

  The band list read *"Banner (external change; contributes no height when
  hidden) / `Shortcuts` head with the filter, Remove and Add / the list /
  editor strip / suggestion row (nothing built for it yet) / keyboard group /
  command bar."* Two things falsify it. **The stack is page-dependent**:
  `compute_card_rects` used to reserve the keyboard card's height on every
  page, so Shortcuts carried a card-shaped hole above the command bar; the
  Shortcuts door now stacks banner / list card / editor card down to the
  command bar and the Keyboard door puts its own card at the same origin and
  nothing else. And the **`Shortcuts` head has no heading in it** — the STATIC
  that said the word (`IDC_LBL_SECTION`, 1020) sat in Subtitle directly beneath
  a tab pill captioned `Shortcuts`, and design §3.1's drawing has no such
  heading. The row survives; the filter leads it and Remove/Add close it.

  The list read *"a **fixed eight rows** (`tok::ROWS`) at every DPI, measured
  rather than scaled from a token, so it does not grow with the config."*
  **`tok::ROWS` is deleted.** `list_h` was `want.min(room)` with `want =
  list_header_height + row_h * ROWS`, i.e. a cap, and design §4 makes the list
  take the room the page leaves and scroll. The cap had to go in the same
  commit as the four deletions above it: they return 110 px at 96 DPI, and with
  the cap in place every one of those pixels would have re-appeared as empty
  space *below* the editor card — the same void, moved down the window. What
  survives of it is the whole-row **snap** (`list_h = avail − avail % row_h`),
  which is what keeps `Ui::shown_empty` guarding a real transition.

  Neither correction touches `MIN_HEIGHT` or `MIN_WIDTH`; both were frozen for
  reasons of their own, recorded at the constants.

  **CORRECTED 2026-08-15: `MIN_HEIGHT` is 480, `WINDOW_HEIGHT` is 500, and
  `MIN_HEIGHT`'s freeze was lifted by arithmetic rather than by preference.**
  The first photographs of all four doors show the System card ending 224 px
  above the command bar and the About card 210 — a third of the window, on two
  doors out of four. Two causes:

  - **The window was 103 px taller than the drawing.** Measured in headless
    Chrome at the mock-up's own 680 px, `.win` is **496.9** — not the 600 its
    hint line claims. Design §2's table says 600 and derives only the width;
    the height came across from the pre-Four-Doors window unexamined.
  - **The setting-row pitch was 32 where the drawing is 46.** `tok::ROW_GAP`
    (20) and `tok::DIV_GAP` (10) are that rhythm, for the System and About
    cards only. **Not** a regrid of `CTL` / `ROW_H` / `CARD_PAD` — design §10
    rules those out because `ROW_H` feeds `ImageList_Create` and so moves the
    tick's cell.

  `MIN_HEIGHT` also changed SUBJECT. Every earlier derivation solved the
  Shortcuts page for a row count, which cannot be the binding constraint —
  card 1's list gives room up before anything else moves, so the door that
  runs out of room first is one of the three whose card is FIXED. It is About,
  the only page whose height depends on a text measurement, at a three-line
  disclosure: `78 + 356 + 44` = 478, rounded to 480. The list's rows are a
  consequence now (eight at 500, seven at the floor), not the derivation.
  `MIN_WIDTH` is untouched and still waits on gate G1.

  **`system_plan` and `about_plan` now live in `beckon_core::page_plan`.**
  They were pure integer arithmetic inside a `cfg(windows)` module that had
  **zero tests** — so the whole vertical geometry of four doors was untestable
  on two of the three CI jobs and unrunnable on the dev machine. The evidence
  that this was the cause and not a coincidence: `layout.rs` claimed the System
  card is "262 px of interior" when the figure was 232, and no reading of the
  code produced 262. `layout.rs` now has its first five tests (Windows job
  only); the seven in core run everywhere.

  **The filter box is a view, and the mapping is the feature.** `IDC_FILTER`
  (1021, cue banner `Filter`, no label) matches case-insensitively against
  the **app column only** (`crates/beckon-core/src/settings.rs:544`; see the
  correction below). It lives in `Model`, not in `Ui`, because
  `Model::remove_pressed`, `marked_count`, `ControlState::selected` and
  `remove_enabled` all depend on what is visible — decisions that belong in
  the crate all three CI jobs compile. **`ListItem` carries its model row,
  and `LVN_ITEMCHANGED` maps `items[i].row` before calling `on_select` /
  `on_mark`** — those callbacks take model indices, and a ListView only ever
  knows view positions. Without that, one filtered keystroke ticks one
  binding and deletes another.

  **CORRECTED 2026-08-14 (`8d03d56`): the filter matched BOTH columns until
  this branch.** That is where the feature started, and the argument for it
  was that both columns is the rule `beckon search` already uses, so the
  program would have no third matching dialect. What falsified it: **every
  beckon chord contains `alt`**, so a filter of `a` — a plausible first
  keystroke of "brave" — matched every row while the box looked as though it
  had narrowed the list, and `Remove` takes the ticked rows. Measured with
  four bindings (`Brave` / `Kitty` / `Firefox` / `Discord`, all
  `ctrl+alt+<key>`) and filter `a`: `visible` returned all four, so ticking
  what was on screen and pressing Remove deleted the whole table. It now
  matches the app name only, pinned by
  `the_filter_does_not_match_the_shortcut_column`. The two dialects differ on
  purpose: `search`'s worst outcome is a long list, this window's is a
  deleted binding.

  **What that gives up is real, and is pinned rather than left to be
  rediscovered.** The window can no longer answer "what already owns this
  chord?" by filtering — that is what
  `filtering_by_a_key_name_finds_nothing` asserts. If it bites, the way back
  is to match the chord's **key** (`f2`, `b`) — the half a person searches
  for, and the half that is not `alt` on every row — and never the whole
  chord as a substring again.

  Two rules keep it safe, and both are functions rather than discipline.
  **Remove never deletes a row you cannot see:** ticks survive being
  filtered out but are inert while off screen, and `marked_count` /
  `remove_enabled` are scoped to the visible set too — otherwise the window
  says four are ticked while Remove takes one. **`visible()` exempts the
  selected row from the filter:** without it, editing a row until it stops
  matching drops it from the view, and `apply_state`'s `None` arm then
  disables the field that has keyboard focus and blanks it, mid-word. That
  exemption also means the list cannot empty while a row is selected, so
  `Ui::shown_empty` never flips on a filter keystroke and `layout` never
  resizes the App combo there — the §7.15 path, closed rather than argued
  about. `Add` still clears the filter, which is a different question: a new
  row is empty and would match nothing.

  **The status vocabulary is four words, and a healthy row says nothing.**
  `paused` > `in use` > `missing` > `other chord`, and that order IS
  the precedence — a row can be several at once while the cell holds one
  word. `paused` sits above the registration map deliberately: `serve`
  CLEARS that map when it pauses, so consulting the map first would render
  every row "not registered yet" and never say why. **One function,
  `beckon_core::settings::row_condition`, produces the list flag AND the
  editor's notes**, and derives `mark` at the end rather
  than assigning it along the way — so "the cell and the note cannot
  disagree" is true by construction rather than by discipline. It was not:
  `items` used to read only the registration map while `detail` read the
  catalog too, and they contradicted each other.

  **CORRECTED 2026-08-15 (branch `four-doors-phase-0`), twice over.** The four
  words were `paused` > `key in use` > `not installed` > `custom`; design §3.1
  reworded three of them to the shorter forms above, and the precedence did not
  move. All three renames are shorter than what they replace on purpose — the
  word rides *inside* the App cell (`app_cell`), so every character it spends
  is one the app name does not get.

  And *"derives `mark` from the notes at the end"* stopped being the whole
  rule: design §3.1 also deleted the note that merely repeated each word, so
  three of the four words now say their piece in the cell and nowhere else, and
  `mark` folds **the notes and every condition the row earned**. Fold only the
  word that WON the cell and a paused row whose app is missing reports `Warn`
  where it used to report `Bad` — the precedence is for the cell, not a claim
  that the outranked problem stopped existing.
  `a_paused_row_whose_app_is_missing_is_still_bad` is the pin.

  **`beckon-serve.exe` starts on a config that does not parse** (commit
  `4f82b94`). It installs the tray, registers no hotkeys, arms no Caps
  hook — the parsed `keyboard` block is discarded along with the shortcuts,
  because a half-parsed file must not decide whether to install a
  `WH_KEYBOARD_LL` hook — and writes nothing. The settings window then
  opens read-only with the parse error as ordinary notes. Refusing was
  measured on a14 to end in a modal dialog with *no tray icon*, which made
  the one window built for exactly this file unreachable from the one
  starting condition that most needs it. **`beckon.exe serve` still refuses
  and exits non-zero** (`BrokenConfig::Refuse`): it has a console to print
  to and callers that check the code. macOS `serve` refuses too — no tray,
  no window, nothing for a tolerant start to rescue — and `beckon check` is
  untouched. Note the interaction the old behaviour had with
  `examples/windows/serve/beckon-serve.xml`: `<RestartOnFailure>` there is
  `PT1M` x 3, and pairing it with a deterministic exit 1 spends all three
  restarts on a file only a human can fix, then gives up — leaving no
  hotkeys and, before `4f82b94`, no tray to say so.

  **The availability probe asks the OS last, and always gives the chord
  back.** Order, from `beckon_core::settings::probe_plan`: parse, the F12
  guard, the row's own chord, other rows in the file, the row's *saved*
  chord, and only then `RegisterHotKey`. Every step before the last is a fact
  the OS cannot report, and asking it first lets a reserved or already-
  duplicated chord come back green. **`VK_F12` is reserved for debuggers at
  all times**, so a successful registration on it proves nothing -- and the
  F12 guard does **not** commute with the own-row check: below it, a row
  bound to `ctrl+alt+f12` probing its own chord answers `Unchanged` with
  `Mark::Ok`, a green tick on the one key the guard exists for.

  The probe registers on the **settings window's** `HWND` with one fixed id,
  never `tray_hwnd`: a hotkey is `(hWnd, id)`, and MSDN keeps a duplicate
  pair *alongside* the original, after which `UnregisterHotKey` frees an
  unspecified one of the two -- a silently dead hotkey. It unregisters on
  every exit path; measurements §60 proves it does, with a control that shows
  the test can see a held chord. The verdict rides on `RuntimeStatus`, never
  `Model::problems`, which is what keeps `apply_enabled` testable on the two
  CI jobs that are not Windows -- and **`RuntimeStatus.registered` never
  decides availability**, because pausing clears it and beckon's own chord
  would read as free.

  **The shortcut is four check boxes and a closed key list, not a text
  field.** Spec §C.4's typed path, which it calls primary: it makes an
  invalid combo unrepresentable, it is the only path for someone who cannot
  physically produce a chord, and a `CBS_DROPDOWNLIST` has no edit control,
  so §7.15's resize defect is structurally impossible there. `IDC_COMBO`
  **kept its number (1002) and changed class** -- the id `settings_probe`
  pins still names the shortcut control.

  Two things hold it together and neither is visible to a unit test.
  **`ComboView::key` is an index into `shortcuts::key_table()` and the window
  passes the same integer to `CB_SETCURSEL`** -- so the list must be filled
  from `key_table()` in order and **`CBS_SORT` must never be set**; sorted,
  `f10` moves ahead of `f2`, every index shifts, and the window writes a key
  the user did not choose, silently. `examples/settings_probe.rs` reads the
  style and the count on hardware because nothing in `beckon-core` can see
  either. And **`commit_fields` compares `ComboView`s, not strings**:
  `Combo::parse` accepts free modifier order while the window rebuilds
  canonically, so a string compare made `"super+ctrl+alt+t"` look like an
  edit and lit up Save on a file nobody had touched.

  The four boxes carry **no `&` mnemonic** -- `Hold` already claimed `t`,
  `w` and `l`, and the table in `mod cap` is the only guard there is.

  **The Caps Lock row is one line, and `Hold` has three chips, not four.**
  `[x] Use Caps Lock as a shortcut key   Hold [Ctrl][Win][Alt]   Tap [v]`.
  It replaced a check box plus three radios whose first caption embedded the
  question governing all three, so the other two did not read as answers to
  it. **There is no Shift chip and there must never be one**: `Chord` has
  exactly `ctrl`/`super_`/`alt`, because the hook has to release whatever it
  presses, and releasing Shift under the user's fingers makes everything they
  type next arrive lowercase. Spec §F.8 sketches four chips; the type is
  right and the sketch is wrong. `Tap` is a `CBS_DROPDOWNLIST` read and
  written **by index**, never by text -- even a `DROPDOWNLIST` has typeahead,
  which moves the selection. Enablement follows the check box, and note that
  a **disabled `CBS_DROPDOWNLIST` still renders white with dark text**, so it
  looks live beside greyed labels: measurements §56, and do not "fix" it.

  **REVERSED 2026-08-12: chord capture is in, as `Record` / `Stop`.** This
  entry used to read *"Chord capture stays out. Combos are typed as text.
  `msctls_hotkey32` cannot capture the Windows key, and `Win+T` and its
  siblings are shell hotkeys Explorer consumes before a normal window sees
  them — so a capture field would fail on precisely the chords beckon
  recommends."* **Both facts are true and both are about a window receiving
  `WM_KEYDOWN`, which is not the layer capture uses.** A `WH_KEYBOARD_LL`
  callback runs before the keystroke reaches any queue and before shell
  hotkey processing, sees `VK_LWIN` as an ordinary `vkCode`, and suppresses
  the key by returning 1 — and beckon already owns that hook for the Caps
  feature. Measured on a14 2026-08-12 with a person at the keyboard: `Win+T`,
  `Win+X`, `Win+D`, `Win+E`, `Win+R`, `Win+Tab`, `Alt+Tab` and
  `Ctrl+Shift+Esc` all came back `SEEN=True SWALLOWED=True ACTED=False`, with
  `Win+R` appearing twice in one run — passed through it opened the Run
  dialog, swallowed it did not — as the control that carries the claim. Do
  not re-add the old entry without re-running that probe.

  **This widens the LLHOOK exception from one feature to two**, because
  capture arms the hook on machines where the user deliberately left
  `keyboard.caps = false`. Three things keep that narrow and none may be
  "simplified" away: there is exactly **one** hook with a two-reason refcount
  (`capture::HookOwners`) — a second `WH_KEYBOARD_LL` chains and would record
  the alias `Caps+T` injects instead of the key pressed; the capture arm of
  `hook_proc` is consulted **before** `caps::decide` for that same reason; and
  the `caps::decide` arm is **skipped entirely** when Caps is not wanted and
  `CapsState::at_rest()` agrees nothing is owed, so a capture on a Caps-off
  machine cannot make a Caps tap toggle the lock through a synthesized stroke.
  The `at_rest` half is not optional: skipping while a swallowed key-down is
  still owed its swallowed key-up leaks an unpaired up into whatever has
  focus.

  **What is refused rather than recorded**, in `capture::is_reserved`:
  `Win+L` and `Ctrl+Alt+Del`, and the three lock keys as main keys. `Win+L`
  is a **block-list, not blindness** — measured, the hook *does* see it, and
  returning 1 does not stop the lock, so without the list beckon would
  cheerfully write a binding that can never fire.

  **The hook must never outlive the window**, and it does not: `end_capture`
  is idempotent and is called by the `Stop` button, all three of §F.4's focus
  layers, a 10 s watchdog, `WM_CLOSE` (before the save prompt — that prompt
  is a modal loop on the hook's own thread), `WM_DESTROY`, both
  `std::process::exit` arms of `hotkey::run_forever` (Quit from the tray
  never reaches a `WM_DESTROY`) — and, since the tab strip landed
  (2026-08-14, `fa16bf3`), **a page switch**: `settings_window::show_page`
  calls it after the unchanged-door guard and before anything is hidden.
  That one is not redundant with the three focus layers, and this is why it
  had to be added rather than assumed: **`WM_KILLFOCUS`, `WM_ACTIVATE` and
  `WM_ACTIVATEAPP` are all about the WINDOW losing focus, and a pill click is
  a child-to-child focus move inside one window** — none of the three fires.
  `Stop` is `IDC_RECORD` wearing another caption and `IDC_RECORD` is a
  Shortcuts-page control, so the switch takes the only visible way out of a
  recording off the screen while the hook is still swallowing every
  keystroke; the mouse reaches the pills freely because the hook swallows the
  keyboard only. The watchdog is a weak bound on that, not a substitute:
  `CAPTURE_TIMEOUT_MS` bounds SILENCE and `on_capture` re-arms the timer for
  every outcome the hook posts, so a held modifier keeps the clock running.
  Worse, a chord completed behind another door still ran `Outcome::Captured`
  all the way into `push_shortcut`. The watchdog itself is not
  belt-and-braces either: `is_installed()` can lie, because past
  `LowLevelHooksTimeout` Windows removes the hook silently and there is no
  API to ask.

  The typed path stays primary — capture is an accelerator, not a
  replacement. Someone who cannot physically produce a chord still has the
  four check boxes and the key list, and keys capture can never see (bare
  `escape`, bare `tab`) remain selectable there.
- **Fuzzy app launchers à la Rofi/Alfred** — beckon is for *known* hotkey-bound apps invoked by raw id. `search` is for ad-hoc id discovery during setup, not interactive launching.
- **Window tiling / layout management** — beckon only focuses/launches, never moves or resizes.
- **PWA install helper** — user installs PWAs manually via Brave/Chrome's "Install this site as an app". beckon does not wrap this.

## Distribution

- **GitHub**: https://github.com/xom11/beckon (source + tagged release artifacts; 6 prebuilt binaries per release: x86_64 + aarch64 × linux-gnu / apple-darwin / pc-windows-msvc).
- **Homebrew tap** (macOS / Linux): `brew install xom11/tap/beckon` — tap repo `xom11/homebrew-tap`. Formula auto-bumped by `.github/workflows/bump-packagers.yml` on every release.
  The formula ships a **macOS LaunchAgent** (`service do` in
  `packaging/homebrew/beckon.rb.template`), so `brew services start beckon`
  is the whole resident-mode install. Guarded by a top-level `if OS.mac?`:
  `brew style` rejects a `service` block nested in `on_macos do`
  (`FormulaAudit/ComponentsOrder`), and the `run macos:` form leaves
  `service?` true on Linux — where `serve` does not exist — so
  `brew services start` fails there instead of the formula simply having no
  service.
- **Scoop bucket** (Windows, x86_64 + arm64): `scoop bucket add xom11 https://github.com/xom11/scoop-bucket && scoop install xom11/beckon` — bucket repo `xom11/scoop-bucket`. Manifest auto-bumped by the same workflow.
- **Cargo (from git)**: `cargo install --git https://github.com/xom11/beckon beckon-cli`. Requires rustup + a system C/MSVC toolchain.
- **Nix flake**: `nix run github:xom11/beckon -- list` or pull `inputs.beckon.overlays.default` into your nixpkgs.

  **`beckon --version` carries the short sha, and the nix path is why.** It
  prints `beckon <version> (<short sha>)` — `beckon 0.9.4 (400b452)` in the
  measurements below, which is where the 0.9.4 in them comes from; the
  version half tracks `[workspace.package]` as it always did. A flake input
  pins a *rev*, so every rev
  between two releases reports the identical Cargo version: after `nix flake
  update beckon` there was no way to ask a machine which commit it had, and
  the only answer available was to read `flake.lock` on whatever built it.

  Three pieces, and each one exists because the obvious shorter version does
  not work:

  - `crates/beckon-cli/build.rs::emit_version` composes the string and
    `#[command(version = env!("BECKON_VERSION"))]` prints it. **Not the bare
    `version` attribute**, which is `CARGO_PKG_VERSION` alone.
  - It reads `BECKON_GIT_REV` FIRST and falls back to `git rev-parse`.
    The env var is not a convenience: `nix/package.nix` filters `.git` out of
    `src`, so a nix build has no repository to ask and may have no `git` on
    `$PATH` either. `flake.nix` passes `self.shortRev or (self.dirtyShortRev
    or null)` into both `packages.beckon` and `overlays.default` — **the
    overlay is the one that matters**, since every Linux and macOS host here
    installs through it.
  - The `rerun-if-changed` lines in `git_rev` name paths git reports rather
    than a hardcoded `.git/HEAD`, because in a worktree `.git` is a *file*,
    HEAD lives under `.git/worktrees/<name>/` and the branch ref lives in the
    common dir. Verified by measurement, not by reading: from a clean build,
    one `git commit --allow-empty` and a plain `cargo build` — no `clean`, no
    `touch` — moved the printed sha from `400b452` to `f65e6d3`.

  Both CI assertions on this output match a **substring** (`-notmatch
  "beckon"` on Windows; `*"$want"*` against `nix eval .#beckon.version`), so
  the suffix is safe by construction. `nix eval .#beckon.version` is still the
  bare `0.9.4` — `package.nix`'s `version` did not change, only the build
  env. A future check that compares for EQUALITY would break.

  **`-dirty` can appear, and only from nix.** `dirtyShortRev` answered
  `400b452-dirty` on Nix 2.34.8 in a worktree with four modified files, and it
  is passed through verbatim: nix evaluates the whole tree at one instant, so
  it can honestly say the build did not come from a commit. `build.rs`'s git
  fallback deliberately makes no such claim — the suffix is baked when the
  build script runs and `rerun-if-changed` cannot name "any file in the tree",
  so a dirty flag computed there would go stale in the one direction that
  matters: claiming clean while it is not.

  **This closes half a problem; do not read it as the whole one.** The other
  half is that the RUNNING PROCESS may not be the image on disk — on a14 a
  watchdog-started beckon ran the 0.8.0 image for three hours while
  `--version`, a *fresh* process started from whatever is on disk today, said
  0.9.0. No version string can fix that, which is why the settings window's
  About page compares `current_exe()`'s mtime against this process's start
  time instead.

  **`beckon-windows` still prints its own `env!("CARGO_PKG_VERSION")`** in the
  About page and `chrome.rs` — untouched, because that crate has no build
  script and the About page already answers the identity question a better
  way. If the sha is ever wanted there, it needs its own `build.rs`, not a
  reach into beckon-cli's.

  **`nix build` was broken from v0.8.0 to v0.9.3 and nobody noticed for a
  month.** `c33fcf6` inserted `pub mod settings_window;` between
  `#[cfg(target_os = "windows")]` and `pub mod shell;` in
  `crates/beckon-windows/src/lib.rs`, leaving `shell` ungated; `package.nix`
  built the whole workspace, so every Linux/macOS `nix build` hit
  `E0433: unresolved module \`windows\``. The user's Hyprland laptop sat on
  0.6.0 the whole time because `nix flake update beckon` could not succeed.
  Nothing in CI could see it: the build matrix passes
  `--exclude beckon-windows` off Windows and `release.yml` builds
  `-p beckon-cli`. Two guards now exist and they cover *different* halves —
  `package.nix` passes `-p beckon-cli --bin beckon`, so **nix no longer
  compiles `beckon-windows` at all** and the `nix` CI job cannot catch a
  future ungated `mod`; the step that can is `the whole workspace still
  compiles, unexcluded` (`cargo check --workspace --all-targets`, Linux and
  macOS legs) in the `build` matrix. Do not delete one believing the other
  covers it — and note the trap in that step's own history: it was written
  as "mirroring what nix does", which stopped being true in the same commit
  range that made it load-bearing.

  **It earned its keep on 2026-08-16, one commit after landing.**
  `crates/beckon-windows/examples/pill_probe.rs` opened with
  `#![cfg(target_os = "windows")]` — an inner attribute, so it applies to the
  CRATE: off Windows the whole file disappears, `main` with it, and the
  example fails **E0601** rather than compiling to a no-op. Every other probe
  in that directory carries an unconditional `fn main` that dispatches into a
  `#[cfg(target_os = "windows")] mod win`, which is the shape to copy.
  **Neither branch could have caught it alone** — the file lived on
  `four-doors-phase-0`, whose gate excludes the crate, and the step lived on
  `main`, where the file did not exist — so the merge was the first tree that
  had both, and CI went red on the merge commit and stayed red through the
  v0.9.4 tag. A local gate built from the `--exclude` shape alone will not see
  this class; add a bare `cargo check --workspace --all-targets` to it.

**Landing page**: `site/`, deployed by `.github/workflows/pages.yml` (Pages
source = **GitHub Actions**, set by hand in repo settings — left at *Deploy
from a branch* the workflow goes green and publishes nothing). Not `docs/`:
that directory holds internal specs, plans and measurements, and serving
Pages from `/docs` would publish them. `tools/check-site.sh` is the page's
test suite and runs in CI; it asserts the install commands still byte-match
`README.md`, that the letter→app table still matches it, and that the version
matches `Cargo.toml`, so a release bump that forgets the page fails CI rather
than shipping a stale command.

The auto-bump workflow needs a fine-grained PAT in repo secret `PACKAGER_TOKEN` with `Contents: write` on `xom11/homebrew-tap` and `xom11/scoop-bucket` only. Renewal procedure is documented in the tap repo's README. **Rotated 2026-08-11; expires 2027-08-12.**

**CORRECTED 2026-08-13: `Bump packagers` DOES fire on its own, and has since
the `workflow_call` fix landed.** `release.yml` ends with a job that does
`needs: release` and `uses: ./.github/workflows/bump-packagers.yml`, so the
bump is a step of the release rather than a reaction to it. Verified from the
bucket rather than from the workflow file: `xom11/scoop-bucket` carries
`beckon 0.9.0` at 2026-08-12T22:38Z and `beckon 0.9.1` at 2026-08-13T02:41Z,
both minutes after their tags and neither dispatched by hand. **No manual
`gh workflow run` is needed.**

This entry used to say the opposite, and the reasoning was sound for the code
at the time: `bump-packagers.yml` also listens for `release: published`, the
release is created by `release.yml` with `GITHUB_TOKEN`, and GitHub raises no
workflow events for that token — the recursion guard. That was measured at
v0.8.0, when the last `Bump packagers` run really was months old. The
`workflow_call` chain was the fix named there and it was taken; the entry was
not updated. If a release ever does go out without a bump, that path is still
the fallback:

```sh
gh workflow run "Bump packagers" -f tag=vX.Y.Z
```

**Re-running the bump does not test the token.** Both packager repos are
public, so `git clone` with a dead token still succeeds, and both push
steps in `bump-packagers.yml` `exit 0` before reaching `git push` whenever
the rendered manifest is unchanged — so a backfill of an already-published
tag is green whether the token works or not. To actually check it, ask
GitHub what the token may do:

```yaml
env: { GH_TOKEN: "${{ secrets.PACKAGER_TOKEN }}" }
run: gh api repos/xom11/homebrew-tap --jq .permissions.push   # must be true
```

At v0.8.0 the manifest genuinely changed (0.7.0 to 0.8.0), so `git push`
actually ran and the token was exercised for real rather than skipped.

A fine-grained PAT cannot even read a repo it was not granted, so a 404
there means the repo is missing from the token's list. `gh api rate_limit
--include` also returns a `github-authentication-token-expiration` header,
which is where the expiry above came from.

User's nix integration (flake-input pattern, no hand-rolled overlay):

- `~/.nix/flake.nix` — `inputs.beckon.url = "github:xom11/beckon"; inputs.beckon.inputs.nixpkgs.follows = "nixpkgs";`
- `~/.nix/lib/mkConfigs.nix` — `mkArgs` does `args = inputs // { ... }`, which **spreads inputs flat at the top level of specialArgs**. So inside any host's `home.nix` the input is referenced directly as `beckon`, not `inputs.beckon`.
- **CORRECTED 2026-08-16: `rog` is a NixOS host, not a standalone HM one, and
  `zenbook-a14` is not a nix host at all.** The old list here said *"Standalone
  HM hosts (`mkHomeManager`, e.g. `rog`, `desktop`, `zenbook-a14`)"* and two of
  its three examples were wrong. Read out of `~/.nix/flake.nix` and confirmed
  against `nix flake show`:

  | builder | hosts | flake output |
  |---|---|---|
  | `lib.mkNixos` | `x1g6`, `vm`, **`rog`** | `nixosConfigurations` |
  | `lib.mkDarwin` | `macmini`, `airm3` | `darwinConfigurations` |
  | `lib.mkHomeManager` | `server`, `desktop`, `minimal` | `homeConfigurations` |

  `zenbook-a14` appears nowhere — "a14" is the **Windows** laptop, and a
  session reading this entry can spend a while looking for its nix host.

  **The cost of the error is a command that cannot work.** `nix flake show`
  lists no `homeConfigurations.rog`, so `home-manager switch --flake .#rog` —
  the obvious thing to reach for after `nix flake update beckon` — fails on a
  host where beckon is very much installed. `mkNixos` pulls home-manager in as
  a NixOS module (`inputs.home-manager.nixosModules.home-manager`, then
  `home-manager.users.${username}.imports = hmModules ++ [../hosts/<device>/home.nix]`),
  so on `rog` the whole HM layer ships inside the system closure and
  **`sudo nixos-rebuild switch --impure --flake ~/.nix#rog` is the one command
  that applies it**. There is no separate HM step to run, and running one is
  not merely redundant — it errors.

- **Standalone HM hosts** (`mkHomeManager` — `server`, `desktop`, `minimal`) — `pkgs` is constructed with `overlays = [ (import ../overlays) inputs.beckon.overlays.default ]`, so `pkgs.beckon` works without further wiring.
- **nix-darwin / NixOS hosts** (`mkDarwin`, `mkNixos` — `macmini`, `airm3`, `x1g6`, `vm`, `rog`) — overlay is **not** pre-baked. The host's `home.nix` adds it explicitly:
  ```nix
  {pkgs, beckon, ...}: {
    nixpkgs.overlays = [
      (import ../../overlays)
      beckon.overlays.default
    ];
    home.packages = [ pkgs.beckon ];
  }
  ```
- Linux/sway:
  - `~/.nix/home-manager/environments/sway/default.nix` — `home.packages` includes `beckon`.
  - `~/.nix/home-manager/environments/sway/sway.d/conf.d/launch-app.conf` — `set $focus exec beckon` (no path), bindings use Names.
- macOS/Hammerspoon:
  - `~/.nix/hosts/airm3/home.nix` — overlay + `pkgs.beckon` wired as above.
  - `~/.nix/home-manager/dotfiles/macos/hammerspoon/MySpoons/LaunchApp.spoon/init.lua` — beckon-backed spoon. Uses `hs.task.new("/etc/profiles/per-user/$USER/bin/beckon", cb, {name}):start()`. **Do NOT use `hs.execute(cmd, true)`** — the second arg sources the user login shell, which on this user's setup runs >10s and was the source of the original "delay" perceived from hotkey presses.
  - `~/.nix/home-manager/dotfiles/macos/hammerspoon/MySpoons/LaunchApp.spoon/init.lua.backup` — preserved original Lua impl for reference.

To bump beckon to latest commit on `main`:

```sh
cd ~/.nix
nix flake update beckon
git commit --only flake.lock -m 'flake: bump beckon <old> -> <new>'

# NixOS (rog, x1g6, vm) — home-manager rides inside the system closure,
# so this ONE command is the whole deploy. There is no HM step to add.
sudo nixos-rebuild switch --impure --flake ~/.nix#rog

# standalone HM (server, desktop, minimal) — only these three
home-manager switch --flake .#<host>

# macOS / nix-darwin (macmini, airm3)
sudo darwin-rebuild switch --flake .#airm3 --impure
hs -c "hs.reload()"   # reload Hammerspoon to pick up spoon changes
```

**Use `git commit --only flake.lock`, not `git add` + `git commit`.** Measured
2026-08-16: several Claude sessions share `~/.nix`, and `git commit` takes the
whole INDEX, not the file just added — a peer staged three files between the
`git status` read and the commit, and 110 lines of their work landed inside a
commit whose message was about bumping this pin. `--only` ignores the index
entirely, so a race cannot widen the commit; verify with `git show --stat HEAD`
rather than trusting the commit summary.

The bump itself is all there is to it — no manual rev / hash / Cargo.lock copy.
flake.lock records the pinned rev for reproducibility across machines, and
since 2026-08-16 `beckon --version` prints that rev's short sha, so a machine
can be asked directly instead of by reading the lock file that built it.

## Picking up next session

State at session close:
- ✅ Phase 1a (sway), 1b.i3 done — name-based MRU toggle, `.desktop` launch, `notify-send` on hotkey error, Nix flake + overlay.
- ✅ Phase 2 (macOS) done **and deployed on `airm3`** — `crates/beckon-macos/` ships full focus / launch / cycle / toggle / hide via `objc2-app-kit` (NSWorkspace, NSRunningApplication), AX (`AXUIElementCreateApplication`, `AXWindows`, `AXRaise`), and CGWindowListCopyWindowInfo for z-order. Launch shells out to `/usr/bin/open -b <bundle_id>`. `beckon doctor` reports Accessibility trust state. Hammerspoon spoon ported and live.
- ✅ Phase 3 (Windows) done — `crates/beckon-windows/` ships full focus / launch / cycle / toggle / hide via Win32 `EnumWindows` (z-order = MRU), COM `IShellLinkW` for Start Menu `.lnk` parsing, native MSIX/AppX catalog resolution and activation through AUMIDs, and `SetForegroundWindow` + `AttachThreadInput` for anti-focus-stealing. Toast notification on hotkey errors. Tested on ARM64 Windows 11.

Reasonable next-session order:
1. **AHK integration** — wire beckon into `~/.nix/windows/ahk/launch-app.ahk` replacing the old title-match approach. Each binding becomes `Run("beckon <Name>")`.
2. **PWA AUMID matching** — MSIX/AppX identity is handled natively; browser PWAs still need validation because their window ownership and AUMID behavior varies by browser.
3. **Polish** (when needed): X11 generic backend, Hyprland, integration tests on CI, fuzzy match for `resolve` typos. Maybe `--include-titles` for `search` (open question 4).

### Phase 3 Windows notes (for future maintenance)

- **Window enumeration**: `EnumWindows` returns windows in z-order (front-to-back), which gives us MRU order for free — no state file needed (mirrors macOS `CGWindowListCopyWindowInfo`). We filter out invisible, cloaked (via `DwmGetWindowAttribute(DWMWA_CLOAKED)`), tool windows (`WS_EX_TOOLWINDOW`), and owner windows.
- **Anti-focus-stealing**: Win10+ blocks `SetForegroundWindow` from background processes. We use the `AttachThreadInput` trick: attach our thread input to the foreground thread, call `SetForegroundWindow` + `BringWindowToTop`, then detach. This works because beckon is invoked from AHK which holds the foreground.
- **Name resolution**: Start Menu `.lnk` files are parsed via COM `IShellLinkW` + `IPersistFile::Load`; MSIX/AppX entries and the built-in `File Explorer` shell app are enumerated natively from shell `AppsFolder` with friendly name and AUMID. Priority: display name (exact) > AUMID > exe stem/name > display name (substring). Use the exact name `File Explorer`, since `Explorer` may collide with a shortcut targeting `explorer.exe`.
- **Hot-path catalog cost (three layers, measured on ARM64 Windows 11)**. The
  naive `beckon <id>` cost was ~443 ms because it built the whole installed-app
  catalog on every keypress. It is now ~57 ms. Do not undo these in the name of
  simplification:
    1. **Name tier resolves from filenames, no COM** — a shortcut's display name
       *is* its filename stem (`parse_lnk` never reads a name from the `.lnk`
       body), so `apps::resolve_start_menu_by_name` walks the tree and parses
       only the stem matches: one parse instead of ~120. This is the whole
       reason the hot path is fast; 186 ms → 57 ms on its own.
    2. **AppsFolder stays lazy** — `apps::resolve_lazy` reaches for
       `scan_shell_apps()` only when no shortcut matches by exact display name.
       That top tier can't be beaten by a packaged app (a shortcut sorts ahead
       of an AppX entry of the same name), so skipping it cannot change the
       answer; `resolve_lazy_agrees_with_one_shot_resolve` pins the equivalence.
       Weaker tiers (AUMID, exe stem, name substring) all lose to a packaged
       app's exact name, so those still pay for the full scan.
    3. **The two scans overlap on the fallback path** — by then the name tier is
       already ruled out, so `resolve_lazy` is guaranteed to call its loader and
       the AppsFolder enumeration can start eagerly. Worth ~60 ms on the miss
       path (1005 ms → 945 ms, of which ~700 ms is the error toast, not scanning).
  **Do not parallelise the `.lnk` parse.** After (3), `scan_start_menu` (~150 ms)
  runs alongside `scan_shell_apps` (~370 ms) and is no longer the critical path,
  so a thread pool there buys zero wall-clock — while costing per-thread STA
  `CoInitializeEx` (an MTA worker would get a marshalling proxy back to the host
  STA and serialise anyway) plus a two-phase walk to keep the traversal-order
  dedupe intact. Measured, not assumed.

  Discovery commands (`list`, `installed`, `resolve`, `search`) deliberately
  keep using the full `scan_installed_apps` — correctness and completeness
  beat latency there.
- **Matching running windows**: Packaged apps match by HWND `PKEY_AppUserModel_ID`, falling back to process AUMID from `GetApplicationUserModelId`; `CabinetWClass` windows map to the built-in `Microsoft.Windows.Explorer` AUMID; classic applications retain exe filename and title fallback matching. Browser PWAs sharing an exe still require browser-specific validation.
- **UWP/Store apps**: Apps installed via Microsoft Store (e.g. Windows Terminal) are cataloged by friendly name and AUMID; launch uses `IApplicationActivationManager::ActivateApplication`.
- **Launch path**: Classic shortcut entries use `ShellExecuteW` with the exe path and arguments extracted from the `.lnk`; MSIX/AppX entries use `IApplicationActivationManager::ActivateApplication` with the AUMID. `Microsoft.Windows.Explorer` is identified by AUMID/class but launches through `explorer.exe`, since activation manager rejects that built-in shell AppID.
- **COM initialization**: `CoInitializeEx(COINIT_APARTMENTTHREADED)` is called for catalog and activation threads. The call is idempotent (returns `S_FALSE` if already initialized on the thread).
- **Toast notifications**: When stderr is not a terminal (hotkey invocation), errors are surfaced via PowerShell-spawned Windows toast notifications (best-effort, same pattern as Linux `notify-send`).
- **`--log <PATH>` (with `serve`) redirects stderr and detaches the
  console** — `crates/beckon-windows/src/logfile.rs`. It exists so a
  Scheduled Task can run `beckon.exe` directly: Task Scheduler cannot
  redirect stderr, so the task used to go through `cmd.exe` for a `2>`,
  which left a console window, which needed a `wscript.exe` VBScript shim
  to hide. VBScript is a deprecated feature-on-demand; both hops are gone.
    - **Why no call site changed.** std's Windows stdio resolves
      `GetStdHandle` on *every* write instead of caching it, with a comment
      naming `SetStdHandle` as the reason (rust-lang/rust#40490), and std
      pins it with `library/std/tests/switch-stdout.rs`. One swap redirects
      every print site. Verified identical at the 1.75 floor and at 1.97.
    - **Redirect and detach are one flag on purpose.** Detaching without
      redirecting leaves stderr pointing at a destroyed console, and
      `print_to` panics rather than returning on a write error that is not
      `ERROR_INVALID_HANDLE`. Fusing them makes that state unrepresentable.
    - **Everything fallible runs before `FreeConsole`**, because `main`
      reports errors with `eprintln!` — an `Err` returned from after the
      detach turns `exit(1)` into a silent panic.
    - **Append, not truncate.** `2>` truncated on every start, so
      `RestartOnFailure` destroyed the log explaining the failure it was
      restarting from.
    - **Bounded at 5 MiB, one generation** (`roll_if_oversized`): past the
      limit the file becomes `<name>.1` and a fresh one starts, so the pair
      caps at 10 MiB. The check runs *when the log is opened*, which is why
      there is no timer and no background thread — and the frequency lands
      where the growth is on its own: the daemon opens its log once per
      logon and writes a couple of lines per boot, while a 5-minute watchdog
      opens its log 288 times a day and is the only writer producing a line
      on a schedule (~55 KB/day, measured on a14). `beckon <id>` never
      reaches this code.
      Owning the file is *why* this is beckon's job: on macOS launchd owns
      it via `StandardErrorPath` and on Linux journald owns it, but Task
      Scheduler discards stderr entirely, so on Windows nobody else can.
    - **`serve` log messages stay ASCII.** Windows PowerShell 5.1's
      `Get-Content` defaults to ANSI, so a UTF-8 em-dash came back as
      `�?"` in the log. The doctor/resolve output keeps its emoji — those
      go to a terminal, never to `--log`.
    - **Pre-existing hazard this does not fix**: whenever stderr is a file
      (already true under `cmd /c … 2>`), a write failure — full disk,
      disconnected network share — panics the printing thread rather than
      returning an error. In `serve` that surfaces as "hotkeys silently
      stop", not a crash.
    - **The toast spawn needs `CREATE_NO_WINDOW` because of this.** After
      `FreeConsole`, `CreateProcess` hands a console-subsystem child of a
      console-less parent a brand-new console, *shown* — std passes only
      `CREATE_UNICODE_ENVIRONMENT` and never sets `STARTF_USESHOWWINDOW`. So
      the PowerShell toast in `notify.rs` would flash a black window on every
      post, including once per keypress (`on_hotkey` uses
      `Cause::HumanAction`, which is never throttled), undoing the entire
      point of `--log`. The flag is invisible from the call site; do not
      "clean it up".
    - **A ~60 ms flash remains, measured.** Task Scheduler cannot start a
      console-subsystem process without allocating a console first;
      `FreeConsole` only closes it afterwards. On Windows 11 ARM64 (build
      26200), inside session 1, 25 ms sampling, with a control: bare
      `serve` leaves a console **and** a `PseudoConsoleWindow` up for the
      life of the daemon; `serve --log` shows one window at ~150 ms that
      is gone by ~210 ms and leaves nothing; `conhost.exe --headless` in
      front of the same command shows nothing at any point. Worse than it
      sounds where Windows Terminal is the default terminal: the console
      arrives as a new WT *tab*, and closing that tab sends
      `CTRL_CLOSE_EVENT` and kills the daemon.
    - **Point the task at the real exe.** A launcher that stays alive as a
      parent — a Scoop shim, `cmd /c` — holds the console, so beckon's
      `FreeConsole` does not close it. Verified: the shim's pid is the
      `ParentProcessId` of the real beckon process.
    - **That escalation was taken: `beckon-serve.exe`.** A second binary
      (`crates/beckon-cli/src/bin/beckon-serve.rs`), GUI-subsystem on just
      that `[[bin]]` target — never the whole crate, which would swallow the
      output of `list`, `installed`, `search`, `resolve`, `doctor`. It has
      no console at any point, so it has none of the flash measured above:
      there is no PE console subsystem for `CreateProcess` to allocate one
      against in the first place. **Confirmed on hardware** — see the
      verification bullet below. It calls `redirect_to_log` before anything else in `main` tries
      to print — the only step ahead of it is argument parsing, which
      already reports its own errors through a dialog, not `eprintln!` — and
      reports its own startup failures the same way, with `MessageBoxW`
      rather than stderr, since there is no console to fall back to even
      before the redirect runs.
      **`CREATE_NO_WINDOW` on the toast spawn stays load-bearing here for
      the same reason it matters after `FreeConsole`**: `CreateProcess`
      gives a console-subsystem child (PowerShell) of a console-less parent
      a brand-new *visible* console, and a GUI-subsystem parent is
      console-less from the start, not just after detaching. See
      `docs/superpowers/specs/2026-08-10-windows-serve-app-design.md` for
      the tray menu, autostart Run-key and first-run design this binary
      adds on top of `logfile.rs`.
    - **Verified on a14, 2026-08-11** (Windows 11 Home build 26200, ARM64),
      built natively and driven from **session 1** — an SSH shell is session
      0 and cannot see the desktop at all, so every observation went through
      a one-shot scheduled task. Registering that task hit the
      SID-not-`DOMAIN\user` failure `examples/windows/serve/README.md`
      documents, which is a live confirmation that note is still accurate.
        - PE subsystem read from the header: `beckon.exe` = 3 (console),
          `beckon-serve.exe` = 2 (GUI).
        - **No window of any kind** from `beckon-serve.exe`, `EnumWindows`
          sampled at 25 ms for 4 s. The control fired as expected in the
          same run: `beckon.exe serve --log` produced
          `CASCADIA_HOSTING_WINDOW_CLASS` (a Windows Terminal tab) at 243 ms,
          gone by 245 ms. Always run that control — a broken probe and a
          clean result look identical without one.
        - Tray icon is real: `Shell_NotifyIconGetRect` returns `hr=0` with a
          screen rect while running, and `0x80004005` after Quit, which is
          how you prove `NIM_DELETE` actually ran.
        - Menu contents read out of the live process with `MN_GETHMENU` on
          the `#32768` popup, then `GetMenuStringW`. That is the only way to
          see another process's menu text, and it is what proved
          **"Start with Windows" is present for `beckon-serve.exe` and absent
          for `beckon.exe serve`** — the row is omitted where a Run value
          could never work.
        - Quit from the menu exits in under 500 ms with code 0. This was the
          risk `TPM_RETURNCMD` was adopted to remove: without it `WM_COMMAND`
          arrives inside the menu's own modal loop, where a `PostQuitMessage`
          that failed to break out would look exactly like a freeze.
        - `--version` / `--help` show a dialog and exit 0; an unknown flag
          shows a dialog and exits 2, matching `beckon.exe`'s usage-error code.
        - First run wrote the starter config and `beckon check` accepted it.
        - **Autostart survives a reboot.** Ticked through the tray menu, then
          rebooted a14: boot 09:15:34, logon 09:15:48, `beckon-serve` up at
          09:16:01 — 13 s after logon — with a fresh pid, the exact Run
          command line, `18 shortcuts registered` in the log, and **parent
          process `explorer.exe`**, which is what a Run-key launch looks like
          and is the part that distinguishes it from a leftover process.
        - The Run value it wrote names the scoop **`current` junction**, not
          the version directory. That mitigation was reasoned from how scoop
          lays out its store; it is now observed.
        - **Still unverified, because it needs a human at the keyboard**:
          the hover tooltip's text, the menu dismissing on click-away (the
          `SetForegroundWindow` half of KB135788), menu placement on a
          high-DPI display, whether Pause actually swallows a physical
          keypress (only the unregister-and-report half was checked), a
          hotkey pressed while the menu is open, and config-edit-to-tooltip
          latency.
        - **a14 cannot be rebooted unattended into a signed-in state.**
          `AutoAdminLogon` is 0 and `shutdown /g` (restart + auto sign-on) is
          rejected with error 87, so the machine stops at the sign-in screen
          and a Run value does not fire until someone signs in. Enabling
          autologon would mean storing the password in the registry in the
          clear — don't. Plan on a person being present.
- **Build requirements**: `aarch64-pc-windows-msvc` target requires VS Build Tools 2022 with the ARM64 component (`Microsoft.VisualStudio.Component.VC.Tools.ARM64`) and Windows SDK. The `.cargo/config.toml` is NOT committed — each machine uses its own MSVC/linker setup.

### Phase 2 macOS notes (for future maintenance)

- **`serve` runs `[NSApp run]`, not Carbon's `RunApplicationEventLoop` —
  changed 2026-08-16, and it is why the macOS settings window had never
  worked.** The macOS tray design's §5 told the implementer to settle this
  before writing any view code. It was not settled; `tray.rs` and
  `settings_window.rs` were written anyway, and the file's own module doc
  said "nothing in this file has been seen on screen" for four days without
  anyone asking whether it *could* be.

  Measured with `crates/beckon-macos/examples/loop_probe.rs` — one view
  hierarchy, two loops chosen by argv, so the difference in the output IS the
  result and cannot be a difference in the thing under test:

  ```text
  nsapp  : isRunning=true   the button's action ran
  carbon : isRunning=false  it never ran
  ```

  `NSApplication` is *instantiated* under the Carbon loop, because
  `NSStatusBar` requires it to exist — and that is exactly what made this
  invisible: the tray drew, so the window was assumed to be fine. But nothing
  ever calls `[NSApp run]`, so nothing calls `[NSApp sendEvent:]`, so nothing
  drains the queue that routes a mouse event to a window and thence to a
  view. **Every control in the window was decoration.**

  The Carbon loop survives as `HotkeyManager::run_carbon_event_loop_for_probe`
  and nothing in beckon calls it. Deleting it would make the finding above
  unfalsifiable, which is the failure mode three other entries in this file
  are about.

  **The hotkey half is measured too, indirectly**, by
  `examples/carbon_queue_probe.rs` with the Carbon loop as the baseline in
  the same run: `carbon : DISPATCHED` / `nsapp : DISPATCHED`. It installs a
  handler on `GetApplicationEventTarget()` — the target `RegisterEventHotKey`
  installs on — posts an event of its own class to the main event queue, and
  asks whether the handler ran. Both loops deliver, so `[NSApp run]` pumps
  the Carbon application event queue.

  **And directly.** Once Terminal.app was granted Accessibility,
  `examples/hotkey_loop_probe.rs` driven by `examples/hid_key.rs` posted a
  real chord through the window server, baseline first:
  `carbon : HOTKEY FIRED` / `nsapp : HOTKEY FIRED`. The queue probe now
  stands as the *explanation* rather than as the evidence.

- **A synthetic chord needs the modifiers posted as REAL KEY EVENTS.**
  `CGEventCreateKeyboardEvent(code)` + `CGEventSetFlags(ctrl|opt|shift)` is
  the obvious spelling and it does **not** fire a `RegisterEventHotKey`
  chord: measured 2026-08-16, it posted successfully — `AXIsProcessTrusted:
  true`, no error anywhere — and nothing happened under EITHER loop. The
  flags field *describes* an event; it does not hold a key down, and the
  system tracks modifier state from `kVK_Control` &co. key events. The
  working sequence is the one a hand makes: each modifier down carrying the
  flags accumulated so far, then the key down and up, then the modifiers up
  in reverse (`examples/hid_key.rs`).

  **The baseline is what caught it.** A flags-only injection failing under
  `nsapp` alone would have read as "the loop change broke hotkeys" and got
  `run_forever` reverted for no reason; it failed under the Carbon loop too,
  which is the one that demonstrably delivers hotkeys in production, and that
  is what said the injector was wrong rather than the thing under test.

- **Phase B is built and measured end to end.** `beckon_macos::caps_tap` is
  the `CGEventTap` twin of `beckon_windows::caps_hook`, against the same
  design: Caps is an **alias for the configured chord**, so the tap swallows
  `Caps+T` and injects `ctrl+cmd+opt+T`, which `RegisterEventHotKey` already
  listens for. `examples/caps_live.rs`, with the tap uninstalled as the
  control:

  ```text
  off : hotkey fired = false     <- nothing else on this machine maps Caps
  on  : HOTKEY FIRED             <- beckon's alias did it
  ```

  The control is not ceremony here: with kanata running the `off` run FIRES,
  and the probe would be measuring kanata.

  **The edge is parity, not a flag.** Nothing in a Caps event says whether it
  is a press or a release — both arrive with identical flags because
  suppression freezes the lock the flag reports, and
  `CGEventSourceKeyState` reports that same frozen lock for a lock key. So
  transitions alternate and the first is a press, which has exactly one
  failure mode: a dropped event inverts the phase. `caps_tap::resync()` is
  the answer and is called from every path that can drop one — the tap being
  disabled by timeout or by user input, and any configuration change, which
  is also a moment nobody is holding a key.

  **`caps::decide` is NOT shared, and this is why**, beyond the edge: its
  `KeyEvent` is `{ vk: u32, edge }` with a down and an up, and macOS has
  neither; its `time_ms` is documented as `KBDLLHOOKSTRUCT.time` in
  milliseconds since boot while `CGEventTimestamp` is nanoseconds of mach
  absolute time. What IS shared is `caps::bound_keys_mac`, the Carbon-keycode
  sibling of `bound_keys`, pinned to select the same bindings by
  `the_two_projections_select_the_same_bindings`.

  **Input Monitoring, not Accessibility.** It is a separate grant in a
  separate pane, and `caps_tap::install` refuses with that sentence rather
  than installing a tap that receives nothing. The Keyboard door says the
  same thing under its first group, because it is the one thing a reader
  cannot discover by trying.

- **Phase B, measured before it was written** (`examples/caps_probe.rs`,
  2026-08-16). The Windows Caps feature is an ALIAS: the hook swallows
  `Caps+T` and injects `ctrl+win+alt+T`, because `RegisterHotKey` cannot bind
  Caps. Four facts had to hold for that shape to port, and all four do:

  | | |
  |---|---|
  | a `CGEventTap` sees Caps | yes -- as **`kCGEventFlagsChanged`**, never `keyDown`/`keyUp` |
  | returning NULL suppresses it | yes |
  | **suppression also stops the LOCK** | **yes** -- `caps_locked()` read `false` before and `false` after a swallowed press |
  | the tap survived | no timeout in that run |

  The third is the one the feature rests on: beckon can take the key without
  the lock engaging, so `caps_tap` can offer `capslock` / `escape` / `none`
  the way Windows does rather than being stuck with whatever the OS did.

  **Caps arriving as `flagsChanged` is the one structural difference from
  Windows** and it is why `beckon_core::caps` is not shared: that state
  machine is written against `KeyEvent { vk, edge }` with a down and an up,
  and this platform has neither. It also carries `time_ms` documented as
  `KBDLLHOOKSTRUCT.time` (ms since boot) while `CGEventTimestamp` is
  nanoseconds of mach absolute time -- a unit mismatch that would not fail to
  compile.

- **`CGEventTapCreate` returning a non-NULL port is NOT evidence the tap will
  receive anything**, and the probe now prints `IOHIDCheckAccess` because of
  it. Input Monitoring is a *separate* grant from Accessibility, with its own
  System Settings pane; without it the create call still succeeds and then
  delivers nothing, silently.

- **A synthetic keycode no physical key carries does not survive the trip.**
  The probe's control was `F19` (`kVK_F19`, 0x50), chosen because nothing
  binds it. It produced **zero events of any type** while
  `ctrl+opt+shift+f` through the same injector in the same session produced
  eight -- so the tap was live the whole time and the CONTROL was the broken
  part. That false negative cost a wrong suspicion of kanata, which was
  stopped for nothing. Use a key the keyboard has, and chord it so it types
  nothing.

- **This machine runs kanata, and kanata already implements beckon's Caps
  feature.** `~/.nix/configs/kanata/main.kbd:52` is
  `caps (tap-hold 200 200 esc @cap_alias)` and `kanata_macos.kbd:19` is
  `cap_alias (multi lmet lctl lalt)` -- Caps tapped sends Escape, Caps held
  sends Cmd+Ctrl+Option, which is beckon's own hyper chord and its
  `caps_tap = "escape"` option. Anyone testing beckon's Caps support here
  must stop `org.nixos.kanata` first or they are measuring kanata.

- **`RegisterEventHotKey` does NOT report a chord another application holds,
  so macOS has no availability probe and that is a finding rather than a
  gap.** Measured 2026-08-16 with `examples/hotkey_conflict_probe.rs`, in an
  Aqua session, control first:

  ```text
  Ctrl+Cmd+Opt+F19            ACCEPTED   <- control: registration works here
  Ctrl+Cmd+Opt+F19 (again)    REFUSED    <- OSStatus -9878, same process
  Cmd+Space   (Spotlight)     ACCEPTED
  Ctrl+Up     (Mission Ctrl)  ACCEPTED
  ```

  Carbon refuses a duplicate **within one process**
  (`eventHotKeyExistsErr`) and happily accepts a chord the system owns. So
  the sixth step of `probe_plan` — *ask the OS* — has nothing to ask on this
  platform, and `serve.rs`'s `AskTheOs` arm returning without a verdict is
  correct: a successful registration would be a guess dressed as a
  measurement. The five steps before it all still run, and they are the ones
  that catch real mistakes. The same-process refusal is not a fallback
  signal either — "another row in this file already uses it" is step four,
  which core answers before that arm is reached.

  This entry used to be a hedge in a code comment (*"whether it even refuses
  … is unmeasured"*). The hedge was right; it is now a result. Do not
  re-open it without re-running that probe.

- **Two capabilities live in different processes on this machine, and neither
  one can do both.** Measured 2026-08-16; this is why the macOS UI probes are
  awkward and it is not a thing any single process can discover:

  | | agent's shell | Terminal.app |
  |---|---|---|
  | `launchctl managername` is `Aqua`, i.e. AppKit can draw | **no** (`Background`) | yes |
  | `AXIsProcessTrusted()`, i.e. `CGEventPost` is not a no-op | **yes** | no |

  So a probe launched through Terminal draws but cannot inject, and an
  injector run from the agent's shell is trusted but has no session to inject
  into. `examples/hid_click.rs` exists to be the second half of that split
  and **prints `AXIsProcessTrusted` before posting**, because an untrusted
  `CGEventPost` returns `void` and does nothing — silently, which is
  indistinguishable from a click that missed.

  One Accessibility grant for Terminal.app collapses the table into one
  usable process and gives every door an automated click-and-assert loop.

  **The Accessibility *inspection* route is a dead end and was tried first.**
  System Events reported `count of windows` = 0 for the probe — and, asked as
  a control, 0 for Terminal and 0 for Finder, on a machine where System
  Events' own `UI elements enabled` is true. The observer was blind, so an AX
  press would have measured the grant rather than the thing under test.

  What DOES work from the agent's shell with no grant at all:
  `beckon_macos::window_server_windows()` (`CGWindowListCopyWindowInfo`),
  which is how "the settings window is on screen, 640x532, layer 0" was
  confirmed without a screenshot.

- **`WINDOW: up` is not a precondition; the first heartbeat is.** A probe
  that prints a line after `makeKeyAndOrderFront` has not yet proved
  anything: an AppKit window is not an accessibility citizen, and does not
  answer to anything, until its process is pumping events. Wait for a line
  only a turning run loop can emit.

- **Hot-path cost (measured on airm3, ~95-105 ms total)**. Unlike Windows there
  is no structural win left here — most of the time is Apple's, not ours:
    - `open -b <bundle>` is **55-75 ms** and is 92% of the focus path. Of that,
      only ~13 ms is spawning `/usr/bin/open` (bare spawn floor is 2.8 ms; `open`
      with no args, i.e. spawn + dyld of Cocoa/AppKit/CoreServices, is 12.9 ms).
      The rest is the LaunchServices + reopen-Apple-Event round-trip, which no
      API avoids: a native `NSWorkspace.openApplication(at:configuration:)` probe
      measured 50-60 ms to its completion handler. Swapping to it would buy ~13 ms
      in exchange for block/runloop plumbing in the one area that has already
      produced two focus bugs (`82c210a`, `61bf656`) — not currently worth it.
    - `AXIsProcessTrusted()` is **~20 ms**, which is why the step-4.5 guard tests
      the window count first (see the comment there — the order is load-bearing).
      A/B on the cycle/toggle path: 53.8 ms → 44.7 ms median.
    - AX cost is **per-process setup, not per-call**: the first
      `collect_app_windows` for a pid is ~38 ms, the second ~0.25 ms. So
      de-duplicating the `visible_standard_window_count` /
      `cycle_to_next_window` pair buys nothing — measured, don't bother.
    - Everything else is noise: `running_apps()` 8-9 ms, process start ~5 ms,
      MRU write ~0.4 ms.
- **Accessibility permission**: bound to the binary's code signature. Each fresh `cargo build` produces a new unsigned binary with a different identity → permission resets. For development, sign the binary or use a stable wrapper. Production users via Nix get a stable `/etc/profiles/per-user/<user>/bin/beckon` path that survives rebuilds (the Nix-store hash changes but the wrapper symlink does not, and macOS appears to accept that).
- **`activate()` vs `activateWithOptions:`**: objc2-app-kit 0.3 only exposes `activateWithOptions:`. We pass empty options (no `ActivateAllWindows`) so step 5a's window-cycle decision survives the activation.
- **Launch path**: We shell out to `/usr/bin/open -b <bundle_id>` instead of `NSWorkspace.openApplicationAtURL:configuration:completionHandler:` because the latter is async-only on modern macOS and would force us to spin a runloop. `open` returns in ~10–20 ms.
- **Cycle algorithm**: `AXUIElementCopyAttributeValue(app, "AXWindows")` gives us a `CFArray<AXUIElement>`. We find the element with `AXMain == true` and `AXRaise` the next one (wrap-around). Returns `false` (falls through to step 5b) if there are <2 windows OR if the process is not AX-trusted — we can't distinguish those reliably.
- **z-order other-app pick (5b)**: `CGWindowListCopyWindowInfo(.onScreenOnly | .excludeDesktopElements, kCGNullWindowID)` returns front-to-back layer-0 windows. Filter to those with PIDs not in the target's bundle PID set; first hit is the most-recent OTHER app.
- **PWA scan recursion**: macOS browsers (Brave/Chrome/Vivaldi) install PWAs into `~/Applications/<Browser> Apps.localized/<Name>.app`, which is one level deeper than a flat `read_dir` of `~/Applications` reaches. `installed_apps()` therefore descends one extra level into any non-`.app` directory child of each root, but stops there (going inside a `.app` would surface nested helper bundles like `Foo.app/Contents/Library/Bar.app` which are not user-launchable). PWAs ship with `CFBundleDisplayName=Discord` (etc.) — beckon's Name match works directly; the bundle ids contain a per-install hash and are not portable across machines (same caveat as Linux Brave PWAs).
- **Hammerspoon spoon avoid `hs.execute(cmd, true)`**: the `true` second arg makes Hammerspoon source the user's login shell (`~/.zshrc`) before each invocation. On a typical setup that's hundreds of ms; on a heavily customized zsh (this user) it can exceed 10 s — fully swamping beckon's own ~50 ms hot path. The spoon uses `hs.task.new("/etc/profiles/per-user/$USER/bin/beckon", cb, {name}):start()` instead — non-blocking, no shell startup. Deliberately chosen over `hs.execute` even with `false`, because `hs.task` also gives us `exitCode` and `stderr` in the callback for clean error surfacing.
- **AX-cycle ref counting in `windows.rs`**: `AXUIElementCopyAttributeValue` returns CF refs under the create rule. We wrap the outer `AXWindows` array via `CFArray::wrap_under_create_rule` (from `windows_value`), then for each window AXUIElement we `wrap_under_get_rule` to take an extra retain so the per-window CF lifetime extends past the array. The `AxElement::from_borrowed` constructor is `unsafe` and must be paired with `mem::forget` — see the inline comment in `windows.rs` if changing this code.
