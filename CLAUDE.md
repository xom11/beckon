# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project: beckon

Cross-platform focus-or-launch app switcher for macOS, Windows, and Linux. A thin CLI wrapper around per-OS native backends — invoked by the existing dotfiles (sway/AHK/Hammerspoon) instead of replacing them.

**Behavior**: press hotkey → if app not running, launch it. If running but not focused, focus it. If already focused, cycle to next window of the same app, or hide.

**No config file.** beckon resolves a user-supplied id at runtime against the OS's own metadata (Linux `.desktop` files, macOS LaunchServices, Windows Start menu). The dotfile per OS holds the id. beckon ships discovery commands (`-l`, `-s`, `-r`) so users don't have to dig the id out of the OS by hand.

**Name-first identifiers.** The id can be a human-readable Name (e.g. `Claude`, `Brave`) or a canonical OS-level id (e.g. sway `app_id`, macOS `bundle_id`). beckon resolves Names against installed-app metadata (`.desktop` `Name=` on Linux). Names are stable across machines; OS-level ids often are not (Brave PWA hashes vary per install). Bindings should prefer Names; canonical ids are a fallback for ambiguity.

## Architecture

### Workspace layout (Rust)

```
beckon/
├── Cargo.toml                # workspace root
├── crates/
│   ├── beckon-core/          # Backend trait, shared types (RunningApp, WindowId)
│   ├── beckon-macos/         # Cocoa/AppKit (planned, phase 2)
│   ├── beckon-windows/       # windows-rs (planned, phase 3)
│   ├── beckon-linux/         # multi-backend, dispatch by env at runtime
│   │   └── src/
│   │       ├── lib.rs        # detect compositor/DE, return Box<dyn Backend>
│   │       ├── desktop.rs    # .desktop parser + Name resolution
│   │       ├── state.rs      # single-app MRU state at $XDG_RUNTIME_DIR/beckon-mru
│   │       ├── i3ipc.rs      # swayipc — handles BOTH sway and i3 (shared protocol)
│   │       ├── hyprland.rs   # hyprctl — Hyprland (planned, phase 1c)
│   │       └── x11.rs        # x11rb / EWMH — non-i3 X11 DEs (deferred)
│   └── beckon-cli/           # main binary, picks backend via cfg!(target_os)
├── test-i3-env.sh            # Xephyr+i3 dev sandbox (start/stop/xterm)
└── README.md
```

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
        // GNOME/KDE Wayland — Mutter/KWin block external focus
        bail!("Unsupported Wayland compositor. Run `beckon -d` for details.");
    }
    if env::var("DISPLAY").is_ok()                        { return X11Backend::new(); }
    bail!("No supported display server detected.");
}
```

| Detected env | Backend | Status |
|--------------|---------|--------|
| `SWAYSOCK` | sway (Wayland) — `i3ipc::I3IpcBackend` | ✅ Done |
| `I3SOCK` | i3 (X11) — same `I3IpcBackend` (shared protocol) | ✅ Done |
| `HYPRLAND_INSTANCE_SIGNATURE` | Hyprland | ⏳ Phase 1c |
| `DISPLAY` (no i3, no Wayland) | X11 generic via `x11rb` / EWMH | ⏳ Deferred (covers GNOME-X11, KDE-X11, openbox, ...) |
| `WAYLAND_DISPLAY` w/o sway/Hyprland | GNOME / KDE Wayland | ❌ Out of scope (compositor blocks external focus) |

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
| 1b.x11 | Linux / X11 generic via x11rb (GNOME-X11, KDE-X11, openbox, awesome, XFCE) | ⏳ Deferred |
| 1c | Linux / Hyprland | ⏳ Pending |
| 2 | macOS | ⏳ Pending — must handle Accessibility permission UX |
| 3 | Windows | ⏳ Pending — anti-focus-stealing has quirks |
| — | GNOME / KDE Wayland | ❌ Out of scope — compositor blocks external focus |

### Phase 1b.i3 implementation note

sway and i3 share the i3-IPC protocol exactly — same `swayipc` crate, same JSON tree, same `[con_id=N] focus` command, same scratchpad. The only differences across compositors:
- **Window identity**: Wayland uses `node.app_id`; X11 uses `window_properties.class` (second token of `WM_CLASS`). `collect_windows` already falls back from one to the other.
- **Socket env var**: `SWAYSOCK` for sway, `I3SOCK` for i3. The dispatcher accepts either.

→ No separate i3 module. `crates/beckon-linux/src/i3ipc.rs` serves both.

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
Mutter (GNOME) and KWin (KDE) block external processes from focusing arbitrary windows on Wayland — this is by design (Wayland security model). beckon explicitly does not support these compositors. `beckon -d` detects and reports this. Users on GNOME/KDE Wayland either switch to X11 session, use a supported compositor (sway/Hyprland), or rely on per-DE extensions (out of beckon's scope).

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
   Decided: **one-shot**. Rust cold start is ~10ms, sway/AHK invoke per keypress anyway, and a daemon adds IPC complexity without clear benefit.

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

## Crate dependencies (planned)

```toml
# core / cli
anyhow    = "1"
thiserror = "2"
clap      = { version = "4", features = ["derive"] }

# macOS
objc2         = "0.5"
objc2-app-kit = "0.2"

# windows
windows = { version = "0.58", features = [
    "Win32_UI_WindowsAndMessaging",
    "Win32_System_Threading",
] }

# linux
swayipc                    = "3"      # sway + i3 (same protocol)
x11rb                      = "0.13"   # any X11 DE (GNOME-X11, KDE-X11, i3, awesome, XFCE, ...)
freedesktop-desktop-entry  = "0.7"    # parse .desktop files for list-installed
```

No `serde` / `toml` — beckon does not read or write any config or cache file.

## Out of scope (explicitly)

- **Config file / app aliases** — beckon resolves against OS metadata (`.desktop` / LaunchServices / Start menu) directly. No TOML, no `[apps.claude]` mapping, no resolve cache.
- **Global hotkey registration** — handled by OS-native dotfiles (sway config / AHK / Hammerspoon).
- **GUI / TUI** — CLI only.
- **Fuzzy app launchers à la Rofi/Alfred** — beckon is for *known* hotkey-bound apps invoked by raw id. `-s` is for ad-hoc id discovery during setup, not interactive launching.
- **Window tiling / layout management** — beckon only focuses/launches, never moves or resizes.
- **PWA install helper** — user installs PWAs manually via Brave/Chrome's "Install this site as an app". beckon does not wrap this.

## Distribution

- GitHub: https://github.com/xom11/beckon
- Cargo build: `cargo build --release`
- Nix flake: `nix run github:xom11/beckon -- -l` or pull `inputs.beckon.overlays.default` into your nixpkgs.
- User's local nix overlay: `~/.nix/overlays/beckon/` — pinned to a commit via `fetchFromGitHub`. To bump:
  1. Push beckon changes
  2. `nix-shell -p nix-prefetch-github --run "nix-prefetch-github xom11 beckon --rev <commit>"`
  3. Update `rev` + `hash` in `~/.nix/overlays/beckon/default.nix`
  4. If `Cargo.lock` changed, copy it: `cp ~/beckon/Cargo.lock ~/.nix/overlays/beckon/`
  5. `home-manager switch --flake ~/.nix#<host>`

User's nix integration (already wired):
- `~/.nix/lib/mkConfigs.nix` — `mkHomeManager` imports nixpkgs with `overlays = [ ../overlays ]` (standalone HM ignores `nixpkgs.overlays` set in modules; this is the working alternative).
- `~/.nix/home-manager/environments/sway/default.nix` — `home.packages` includes `beckon`.
- `~/.nix/home-manager/environments/sway/sway.d/conf.d/launch-app.conf` — `set $focus exec beckon` (no path), bindings use Names.

## Picking up next session

State at session close:
- ✅ Phase 1a (sway), 1b.i3 done, name-based MRU toggle, .desktop launch, notify-send on hotkey error, Nix flake + overlay
- ⏳ Phase 2 (macOS) is the next big chunk

Reasonable next-session order:
1. **Phase 2 macOS** — port the Hammerspoon spoon. Reference impl already exists, OS APIs are clean. Plan to add `crates/beckon-macos/` with `objc2`/`objc2-app-kit` deps, mirror the i3ipc structure but use `NSWorkspace` + `CGWindowList`. Accessibility permission needs UX in `-d`.
2. **Phase 3 Windows** — port the AHK script. Adds `crates/beckon-windows/` with `windows` crate. Start Menu `.lnk` parsing for Name resolution (mirrors `.desktop` parsing). Anti-focus-stealing workaround (`AllowSetForegroundWindow` + `AttachThreadInput`).
3. **Polish** (when needed): X11 generic backend, Hyprland, integration tests on CI, fuzzy match for `-r` typos.
