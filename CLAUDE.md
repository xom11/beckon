# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project: beckon

Cross-platform focus-or-launch app switcher for macOS, Windows, and Linux. A thin CLI wrapper around per-OS native backends — invoked by the existing dotfiles (sway/AHK/Hammerspoon) instead of replacing them.

**Behavior**: press hotkey → if app not running, launch it. If running but not focused, focus it. If already focused, cycle to next window of the same app, or hide.

**No config file on the hot path.** `beckon <id>` resolves a user-supplied id at runtime against the OS's own metadata (Linux `.desktop` files, macOS LaunchServices, Windows Start menu). The dotfile per OS holds the id. beckon ships discovery commands (`-l`, `-s`, `-r`) so users don't have to dig the id out of the OS by hand. The one file beckon does read is the resident-mode shortcuts TOML (`--serve` / `--check`, macOS + Windows) — a hotkey→Name table, not an id-alias layer.

**Name-first identifiers.** The id can be a human-readable Name (e.g. `Claude`, `Brave`) or a canonical OS-level id (e.g. sway `app_id`, macOS `bundle_id`). beckon resolves Names against installed-app metadata (`.desktop` `Name=` on Linux). Names are stable across machines; OS-level ids often are not (Brave PWA hashes vary per install). Bindings should prefer Names; canonical ids are a fallback for ambiguity.

## Architecture

### Workspace layout (Rust)

```
beckon/
├── Cargo.toml                # workspace root
├── crates/
│   ├── beckon-core/          # Backend trait, shared types (RunningApp, WindowId)
│   ├── beckon-macos/         # NSWorkspace + AX + CGWindowList — phase 2 done
│   ├── beckon-windows/       # Win32 API (EnumWindows + COM IShellLinkW) — phase 3 done
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
│   └── beckon-cli/           # main binary, picks backend via cfg!(target_os)
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

### Live backend tests

`testing/linux_live_test.py` drives the real binary against a real
compositor and asserts on what that compositor reports afterwards. It is the
only layer that can catch what unit tests structurally cannot: `.desktop`
resolution against the machine's own metadata, the class a toolkit actually
advertises at runtime, and whether a focus/minimize request is honoured at
all. Every Linux bug fixed in the 2026-08 pass was found by it, and none of
them were visible to the 65 unit tests that were green the whole time.

It detects its environment the same way `pick_backend` does, so run it inside
the session under test. All four backends currently pass 19/19 on Ubuntu
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

### CLI surface (pure-flag style)

```
beckon <id>                  # focus-or-launch (default, hot path)
beckon -l, --list            # list running apps with their ids
beckon -L, --list-installed  # list installed apps with launch ids
beckon -s, --search <name>   # fuzzy search across running + installed
beckon -r, --resolve <id>    # validate id, print metadata + suggestions
beckon -d, --doctor          # check environment (permissions, IPC, etc.)
beckon --check <config>      # validate a shortcuts TOML file (CI-friendly)
beckon --serve <config>      # resident hotkey service (macOS, Windows)
beckon -v, --verbose         # debug logging (combine with any command)
beckon -h, --help
beckon -V, --version

# Edge case: id starting with `-`
beckon -- -weird.id
```

The hot path (`beckon <id>`) is positional with no subcommand verb — the user types this 99% of the time from a hotkey binding. Discovery/admin actions are flags.

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
2. **Filename stem** (`kitty.desktop` → `kitty`). Useful when copy-pasting an id from `beckon -l`.
3. **`StartupWMClass=`**. Often wrong on Wayland (Brave ignores it) but harmless to try.
4. **`Name=` substring** (case-insensitive). Multiple matches → alphabetical first wins ("first wins" like rofi).

If priorities 1-4 all fail, fall back to treating `id` as a literal `app_id`. This still allows focusing apps that aren't in any `.desktop` file (ad-hoc programs); launching such an unknown id is an error with a "run `beckon -L` / `-s`" hint.

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
  Measured before the fix: 20 runs of `beckon -r Dup` split 12/8 between two
  entries. Sorting gives every tier the "alphabetically first `.desktop` id
  wins" rule tier 4 already documented.
- **Subdirectories are scanned, and the id is the relative path with `/`
  replaced by `-`** — the XDG menu spec's *desktop file id*. Wine
  (`applications/wine/Programs/…`) and KDE (`applications/kde4/…`) install
  that way; a flat `read_dir` made those apps invisible to `-L` and
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
  (median ~15), `beckon -l` 5–6 ms. Comfortably inside the 50 ms budget, and
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
- **Cycle order (5a)**: pick the same-app window with the lowest non-current
  `focusHistoryID`. Two-window apps end up oscillating between the most-recent
  pair, mirroring the practical i3ipc behaviour.
- **Hide (5c)**: `dispatch movetoworkspacesilent special:beckon,address:0xN`.
  All apps that beckon hides land on the same shared `special:beckon`
  workspace; the next `beckon <id>` finds the window in `j/clients`, sees
  it's not focused, and `dispatch focuswindow` brings the special workspace
  back into view automatically (Hyprland surfaces the window's workspace on
  focus). No state file or per-app special workspaces required.
- **MRU (5b)**: reuses the same `state.rs` file at
  `$XDG_RUNTIME_DIR/beckon-mru` as the i3ipc backend. Sharing is safe — a
  user runs only one Linux compositor at a time.
- **Decision logic** is split into a pure `decide(clients, active, target,
  previous_app) -> Decision` function that the IPC layer then translates
  into dispatch commands. This is what makes the algorithm unit-testable
  without a live Hyprland session (19 tests in `hyprland::tests`).
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
- **Accessibility permission required**. Detect via `AXIsProcessTrusted()` and surface a clear message in `beckon -d` if missing.

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
Wayland has no standard global hotkey API. On sway/Hyprland the compositor itself must bind the key and `exec beckon`. There is no app-level workaround.

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

### macOS Accessibility permission
Required to focus arbitrary apps. Permission is bound to the codesigned binary identity — rebuilding the binary may invalidate it and require re-granting in System Settings.

### PWA handling
PWAs must be **installed as standalone apps** (Brave/Chrome → "Install this site as an app") so each gets a stable bundle ID / `.desktop` / `WM_CLASS`. beckon does NOT handle `--app=URL` invocations — that approach is too brittle to detect/focus reliably.

### Per-OS identifier asymmetry
Names typically resolve consistently across OSes (`Claude` works on Linux/macOS/Windows). Where they don't — e.g. macOS app display name is localized, or two apps share a `Name=` on Linux — users fall back to a canonical OS id (bundle_id / .desktop filename / exe). Discovery via `beckon -s <name>` per machine.

### PWA hash drift (Brave / Chrome)
PWAs installed via Brave/Chrome get an extension hash inside their `.desktop` filename (Linux) or bundle_id (macOS) — e.g. `brave-fmpnliohjhemenmnlpbfagaolkdacoja-Default`. **The hash is generated locally during install and differs across machines**, so canonical ids can't be synced via dotfile copy. The Name field, however, is stable: `Name=Claude` on every machine. **This is the primary reason Name-based resolution is the recommended id format.** `beckon -r <id>` reports "no match" with fuzzy suggestions when a stale canonical id appears in a dotfile.

## Open questions (decide in implementation session)

1. **Daemon vs one-shot CLI**
   Decided: **one-shot for the hot path, plus an opt-in resident mode.**
   `beckon <id>` stays a one-shot CLI (~10ms cold start) for compositor-bound
   hotkeys (sway/GNOME). `--serve <config>` (2026-08) additionally hosts the
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
   Scheduler afterwards. The ergonomic step that *is* open is a `--service
   install/start/stop` subcommand (the skhd / espanso pattern); not built.

2. **MRU tracking source per backend**
   Step 5b (toggle-back) on Linux uses a single-app state file at
   `$XDG_RUNTIME_DIR/beckon-mru` containing the `app_id` focused before
   the most recent beckon action. Each invocation reads the live focus
   from IPC, so transitions made by mouse / native hotkeys reconcile on
   the next beckon call. Limitation: only beckon-mediated focus changes
   are recorded; a sequence of mouse-only switches between beckon calls
   produces a stale "previous". Acceptable for the hotkey-driven workflow.
   macOS / Windows can read z-order directly (`CGWindowList` /
   `EnumWindows`) so they likely won't need a state file at all.

3. **Notification on errors**
   Decided: **auto-detect TTY**. If stderr is not a terminal (typical hotkey-bound invocation), beckon fires a desktop notification in addition to the stderr line. Linux uses `notify-send` (best-effort: silent if absent). macOS will use `osascript display notification`; Windows will use a toast — both pending phase 2/3.

4. **`-s` search scope and ranking**
   Should `beckon -s claude` match against window titles too, or only app id / app name? Title match is more forgiving but volatile. Default likely: id + name only, `--include-titles` opt-in.

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

# resident mode (--check / --serve, since 2026-08)
toml   = "0.8"    # beckon-core: parse the shortcuts file
notify = "6"      # beckon-cli:  watch it for live reload
fs4    = "0.8"    # beckon-cli:  flock, one --serve per config path
```

The **only** file beckon reads is the `--serve` shortcuts TOML. There is still
no config for `beckon <id>` itself and no resolve cache — ids resolve against
OS metadata on every call.

## Out of scope (explicitly)

- **Config for the hot path / app aliases** — `beckon <id>` resolves against OS metadata (`.desktop` / LaunchServices / Start menu) directly. No `[apps.claude]` mapping, no resolve cache. The `--serve` TOML is a *hotkey table*, not a place to alias ids.
- **Global hotkey registration on Linux** — handled by the compositor / WM dotfile (sway config, Hyprland, GNOME Settings). Wayland has no app-level hotkey API, so there is nothing to implement. On macOS / Windows this is *in* scope and shipped: `--serve` registers via RegisterEventHotKey / RegisterHotKey.
- **GUI / TUI** — CLI only.
- **Fuzzy app launchers à la Rofi/Alfred** — beckon is for *known* hotkey-bound apps invoked by raw id. `-s` is for ad-hoc id discovery during setup, not interactive launching.
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
  `service?` true on Linux — where `--serve` does not exist — so
  `brew services start` fails there instead of the formula simply having no
  service.
- **Scoop bucket** (Windows, x86_64 + arm64): `scoop bucket add xom11 https://github.com/xom11/scoop-bucket && scoop install xom11/beckon` — bucket repo `xom11/scoop-bucket`. Manifest auto-bumped by the same workflow.
- **Cargo (from git)**: `cargo install --git https://github.com/xom11/beckon beckon-cli`. Requires rustup + a system C/MSVC toolchain.
- **Nix flake**: `nix run github:xom11/beckon -- -l` or pull `inputs.beckon.overlays.default` into your nixpkgs.

The auto-bump workflow needs a fine-grained PAT in repo secret `PACKAGER_TOKEN` with `Contents: write` on `xom11/homebrew-tap` and `xom11/scoop-bucket` only. Default expiry 90 days — renewal procedure documented in the tap repo's README.

User's nix integration (flake-input pattern, no hand-rolled overlay):

- `~/.nix/flake.nix` — `inputs.beckon.url = "github:xom11/beckon"; inputs.beckon.inputs.nixpkgs.follows = "nixpkgs";`
- `~/.nix/lib/mkConfigs.nix` — `mkArgs` does `args = inputs // { ... }`, which **spreads inputs flat at the top level of specialArgs**. So inside any host's `home.nix` the input is referenced directly as `beckon`, not `inputs.beckon`.
- **Standalone HM hosts** (`mkHomeManager`, e.g. `rog`, `desktop`, `zenbook-a14`) — `pkgs` is constructed with `overlays = [ (import ../overlays) inputs.beckon.overlays.default ]`, so `pkgs.beckon` works without further wiring.
- **nix-darwin / NixOS hosts** (`mkDarwin`, `mkNixos`, e.g. `airm3`, `macmini`) — overlay is **not** pre-baked. The host's `home.nix` adds it explicitly:
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
# Linux / standalone HM:
home-manager switch --flake .#<host>
# macOS / nix-darwin (airm3):
sudo darwin-rebuild switch --flake .#airm3 --impure
hs -c "hs.reload()"   # reload Hammerspoon to pick up spoon changes
```

That's it — no manual rev / hash / Cargo.lock copy. flake.lock records the pinned rev for reproducibility across machines.

## Picking up next session

State at session close:
- ✅ Phase 1a (sway), 1b.i3 done — name-based MRU toggle, `.desktop` launch, `notify-send` on hotkey error, Nix flake + overlay.
- ✅ Phase 2 (macOS) done **and deployed on `airm3`** — `crates/beckon-macos/` ships full focus / launch / cycle / toggle / hide via `objc2-app-kit` (NSWorkspace, NSRunningApplication), AX (`AXUIElementCreateApplication`, `AXWindows`, `AXRaise`), and CGWindowListCopyWindowInfo for z-order. Launch shells out to `/usr/bin/open -b <bundle_id>`. `beckon -d` reports Accessibility trust state. Hammerspoon spoon ported and live.
- ✅ Phase 3 (Windows) done — `crates/beckon-windows/` ships full focus / launch / cycle / toggle / hide via Win32 `EnumWindows` (z-order = MRU), COM `IShellLinkW` for Start Menu `.lnk` parsing, native MSIX/AppX catalog resolution and activation through AUMIDs, and `SetForegroundWindow` + `AttachThreadInput` for anti-focus-stealing. Toast notification on hotkey errors. Tested on ARM64 Windows 11.

Reasonable next-session order:
1. **AHK integration** — wire beckon into `~/.nix/windows/ahk/launch-app.ahk` replacing the old title-match approach. Each binding becomes `Run("beckon <Name>")`.
2. **PWA AUMID matching** — MSIX/AppX identity is handled natively; browser PWAs still need validation because their window ownership and AUMID behavior varies by browser.
3. **Polish** (when needed): X11 generic backend, Hyprland, integration tests on CI, fuzzy match for `-r` typos. Maybe `--include-titles` for `-s` (open question 4).

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

  Discovery commands (`-l`, `-L`, `-r`, `-s`) deliberately keep using the full
  `scan_installed_apps` — correctness and completeness beat latency there.
- **Matching running windows**: Packaged apps match by HWND `PKEY_AppUserModel_ID`, falling back to process AUMID from `GetApplicationUserModelId`; `CabinetWClass` windows map to the built-in `Microsoft.Windows.Explorer` AUMID; classic applications retain exe filename and title fallback matching. Browser PWAs sharing an exe still require browser-specific validation.
- **UWP/Store apps**: Apps installed via Microsoft Store (e.g. Windows Terminal) are cataloged by friendly name and AUMID; launch uses `IApplicationActivationManager::ActivateApplication`.
- **Launch path**: Classic shortcut entries use `ShellExecuteW` with the exe path and arguments extracted from the `.lnk`; MSIX/AppX entries use `IApplicationActivationManager::ActivateApplication` with the AUMID. `Microsoft.Windows.Explorer` is identified by AUMID/class but launches through `explorer.exe`, since activation manager rejects that built-in shell AppID.
- **COM initialization**: `CoInitializeEx(COINIT_APARTMENTTHREADED)` is called for catalog and activation threads. The call is idempotent (returns `S_FALSE` if already initialized on the thread).
- **Toast notifications**: When stderr is not a terminal (hotkey invocation), errors are surfaced via PowerShell-spawned Windows toast notifications (best-effort, same pattern as Linux `notify-send`).
- **`--log <PATH>` (with `--serve`) redirects stderr and detaches the
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
    - **Pre-existing hazard this does not fix**: whenever stderr is a file
      (already true under `cmd /c … 2>`), a write failure — full disk,
      disconnected network share — panics the printing thread rather than
      returning an error. In `--serve` that surfaces as "hotkeys silently
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
    - A **console-window flash** at logon may remain: Task Scheduler cannot
      start a console-subsystem process without allocating a console first.
      The escalation is a separate GUI-subsystem `beckon-serve.exe` — never
      a whole-binary `windows_subsystem = "windows"`, which would swallow
      the output of `-l`, `-L`, `-s`, `-r`, `-d`.
- **Build requirements**: `aarch64-pc-windows-msvc` target requires VS Build Tools 2022 with the ARM64 component (`Microsoft.VisualStudio.Component.VC.Tools.ARM64`) and Windows SDK. The `.cargo/config.toml` is NOT committed — each machine uses its own MSVC/linker setup.

### Phase 2 macOS notes (for future maintenance)

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
