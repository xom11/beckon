# beckon

> *beckon* (v.) — to call someone toward you with a gesture.
> Press a key, the app comes to you.

Cross-platform focus-or-launch app switcher. A thin CLI invoked by your existing
hotkey dotfile (sway, i3, Hammerspoon, AHK) — or, on macOS and Windows,
hosting the hotkeys itself via [resident mode](#resident-mode-macos--windows).

```
press hotkey
  └── if app not running   → launch it
      if running, unfocused → focus it
      if already focused    → cycle windows / toggle to previous app / hide
```

## Quickstart

```sh
# 1. install (binary lands at ~/.cargo/bin/beckon)
cargo install --git https://github.com/xom11/beckon

# 2. discover the Names beckon sees on your machine
beckon -L | grep -i claude     # is "Claude" the right Name?
beckon -r Claude               # confirm: shows match type + Exec
beckon -d                      # diagnose your environment

# 3. wire a hotkey via your existing dotfile — pick yours from
#    examples/ and follow its README:
#       examples/linux/sway/         examples/linux/i3/
#       examples/linux/hyprland/     examples/linux/gnome-x11/
#       examples/linux/kde-x11/      examples/linux/xfce/
#       examples/linux/openbox/      examples/macos/hammerspoon/
#       examples/windows/ahk/
#
#    macOS / Windows alternative — let beckon host the hotkeys itself,
#    no Hammerspoon or AHK needed:
#       examples/macos/serve/        examples/windows/serve/

# 4. press the hotkey. failures fire a desktop notification — you'll see them.
```

Notifications fire only when nothing else would show the error: with stderr on a
terminal beckon just prints. Two adjustments:

- `BECKON_NO_NOTIFY=1` silences them entirely — for scripts and test harnesses,
  which capture stderr and would otherwise look like a hotkey to beckon.
- A `--serve` that fails to start reports once an hour, not once per restart.
  Supervisors (launchd `KeepAlive`, a Task Scheduler repetition) relaunch a
  failing service on a timer, and one broken config should not mean one
  notification a minute.

## Status

| Platform | Status |
|----------|--------|
| Linux / sway (Wayland) | ✅ Phase 1a — i3-IPC |
| Linux / i3 (X11) | ✅ Phase 1b — same backend (shared protocol) |
| Linux / X11 generic (GNOME-X11, KDE-X11, XFCE, openbox, awesome) | ✅ Phase 1b.x11 — `x11rb` + EWMH |
| Linux / Hyprland (Wayland) | ✅ Phase 1c — native Unix-socket IPC |
| Linux / GNOME Wayland | ✅ Phase 1d — bundled GNOME Shell extension over D-Bus |
| macOS | ✅ Phase 2 — NSWorkspace + AX + CGWindowList |
| Windows | ✅ Phase 3 — Win32 EnumWindows + COM IShellLinkW |
| KDE Wayland | ❌ Out of scope (KWin blocks external focus, no bridge to ride on) |

Resident hotkey mode (`--serve`) is available on macOS and Windows. Linux
stays compositor-bound by design — the compositor already owns the keybind.

## Install

### Homebrew (macOS / Linux)

```sh
brew install xom11/tap/beckon
```

### Scoop (Windows)

```sh
scoop bucket add xom11 https://github.com/xom11/scoop-bucket
scoop install xom11/beckon
```

### Cargo (build from source)

```sh
cargo build --release
# binary: ./target/release/beckon
```

Requirements: Rust 1.75+. Linux supports sway, i3, Hyprland, any
EWMH-compliant X11 desktop (GNOME-X11, KDE-X11, XFCE, openbox, awesome),
and GNOME Wayland via the bundled shell extension in
[`extensions/`](./extensions/) — install it with `gnome-extensions install`
and log back in. KDE Wayland is unsupported; `beckon -d` reports which
backend it picked.
On Windows: VS Build Tools 2022 with the C++ ARM64/x64 component and
Windows SDK.

### cargo install (from GitHub)

```sh
cargo install --git https://github.com/xom11/beckon
# update to latest:
cargo install --git https://github.com/xom11/beckon --force
```

Binary lands in `~/.cargo/bin/beckon` (already in PATH).

### Nix flake

```sh
nix run github:xom11/beckon -- -l
nix build .#beckon          # binary at ./result/bin/beckon
nix develop                 # dev shell with rustfmt / clippy / rust-analyzer
```

To pull beckon into your own flake, add the overlay:

```nix
{
  inputs.beckon.url = "github:xom11/beckon";

  outputs = { nixpkgs, beckon, ... }: {
    # ...
    nixpkgs.overlays = [ beckon.overlays.default ];
    # then `pkgs.beckon` resolves
  };
}
```

## Usage

The hot path is `beckon <id>` — invoke from a hotkey binding:

```sh
beckon Claude            # focus / launch / cycle Claude
```

`<id>` resolves against installed-app metadata. Priority per OS:

**Linux** (`.desktop` files in `$XDG_DATA_DIRS/applications/`):

1. `Name=` exact (case-insensitive, normalized) — **recommended for dotfiles**
2. `.desktop` filename
3. `StartupWMClass=`
4. `Name=` substring (alphabetical first wins, like rofi)

**macOS** (`NSWorkspace.runningApplications` + scan of `/Applications`,
`/System/Applications`, `~/Applications` — including one level into
non-.app subdirs to catch `~/Applications/{Brave,Chrome,Vivaldi}
Apps.localized/*.app`):

1. Running app — `localizedName` exact (case-insensitive, normalized)
2. Running app — `bundleIdentifier`
3. Installed app — `CFBundleDisplayName`/`CFBundleName` exact
4. Installed app — `CFBundleIdentifier`
5. Installed app — name substring (alphabetical first wins)

**Windows** (Start Menu `.lnk` shortcuts plus registered shell/MSIX/AppX apps):

1. Display name exact (case-insensitive, normalized)
2. AppUserModelID exact for shell/MSIX/AppX apps
3. Exe filename stem or filename (e.g. `brave` / `brave.exe`)
4. Display name substring (alphabetical first wins)

When the resolved exe is a launcher stub (e.g. Brave PWA `chrome_proxy.exe` →
`brave.exe`), beckon falls back to title matching against running windows.

Names are stable across machines. Brave PWA hashes are not — bind to `Claude`,
not `brave-fmpnliohj...-Default` or `com.vivaldi.Vivaldi.app.<hash>`.
On Windows, prefer exact friendly names such as `Terminal`, `Settings`, and
`File Explorer`; shortened `Explorer` can collide with shortcuts that launch
through `explorer.exe`.

### Discovery

```sh
beckon -l           # list running apps with their app_ids
beckon -L           # list installed apps (parsed from .desktop)
beckon -s claude    # fuzzy-search ids matching "claude"
beckon -r Claude    # show how an id resolves (match type, exec, status)
beckon -d           # check environment (compositor / IPC / notification daemon)
```

### Dotfile examples — see [`examples/`](./examples/)

Drop-in configs for every supported setup (sway, i3, Hyprland,
GNOME-X11, KDE-X11, XFCE, openbox / awesome / fluxbox, macOS
Hammerspoon, Windows AHK) live under [`examples/`](./examples/) with
short READMEs explaining where to place each file and how to reload.

The examples wire the same five hotkeys everywhere so you only have to
remember the letter, not the modifier:

| Letter | App |
|---|---|
| `Space` | terminal |
| `C` | Claude |
| `B` | Brave |
| `E` | Cursor |
| `D` | Discord |

Modifier defaults: `Super` on Linux, Hyper (`cmd+ctrl+alt`) on macOS,
`Ctrl+Win+Alt` on Windows. Replace the Names with whatever
`beckon -L` reports on your machine.

## Resident mode (macOS & Windows)

`beckon --serve shortcuts.toml` turns beckon into the hotkey host itself —
no Hammerspoon/AHK layer needed. The file is flat TOML, one combo per line:

    "ctrl+super+alt+t" = "kitty"
    "ctrl+super+alt+shift+t" = "Telegram Web"

Ready-to-use setups, including the launchd agent and the Scheduled Task
definition: [`examples/macos/serve/`](examples/macos/serve/) and
[`examples/windows/serve/`](examples/windows/serve/).

Modifiers: `ctrl`, `super` (Cmd / Win key), `alt` (Option), `shift` — order
is free. Keys are lowercase only (`a`-`z`, `0`-`9`, `f1`-`f20`, plus named
specials like `space` / `comma` / `pageup`); a shifted binding is written as
the base key plus an explicit `shift`. `f20` is the ceiling because macOS
has no keycode above it, and every key must exist on both OSes so a config
validates anywhere.

`beckon --check shortcuts.toml` validates a file (exit 0/1) without touching
the OS — it runs on Linux too, so it works in CI. The file is watched: edits
apply live, and a broken edit keeps the current bindings and fires a
notification instead of dropping your keys. One `--serve` per config path is
enforced with a lock file.

**Trust the registration count, not the shortcut count.** Startup and reload
report `5 shortcuts registered` when clean and `3 of 5 shortcuts registered
(2 failed)` when another app already owns a chord — a config can parse
perfectly and still register nothing.

On Windows, run it via a Scheduled Task (foreground process in your
interactive session, not a service — `RegisterHotKey` needs a desktop). The
tray icon is a liveness signal, but only in one direction: icon present
means the daemon is alive, icon absent means either the daemon is dead OR
the tray just isn't ready yet (a logon race, or Explorer restarting) —
hotkeys register and fire independently of the icon, so check stderr/the
log to tell those two apart rather than trusting the icon alone. Linux
stays compositor-bound by design.

## What `beckon <id>` actually does

Single algorithm, not configurable:

```
1. resolve id → app metadata (.desktop / Info.plist / .lnk)
2. if no window of this app  → launch
3. if running but unfocused  → focus first window
4. if focused, more windows  → cycle to next window of same app
5. if focused, sole window   → toggle to the previously focused app
6. if nothing else exists    → hide / minimize
```

When a hotkey-bound invocation fails (id not found, IPC error), beckon fires
a desktop notification (`notify-send` on Linux, toast on Windows). Run from a
terminal to see errors on stderr instead.

## Project layout

```
crates/
├── beckon-core/      # Backend trait, shared types
├── beckon-linux/     # algorithm.rs (shared) + i3-IPC + Hyprland + EWMH
├── beckon-macos/     # NSWorkspace + AX (cycle) + CGWindowList (z-order)
├── beckon-windows/   # Win32 EnumWindows + .lnk and MSIX/AppX catalog
└── beckon-cli/       # binary, clap CLI, doctor / search / resolve
examples/             # ready-to-use configs for every supported OS / WM
```

See [`CLAUDE.md`](./CLAUDE.md) for the full design rationale.

## Testing on i3 without leaving sway

```sh
./test-i3-env.sh start    # Xwayland :3 → Xephyr :2 → i3
./test-i3-env.sh xterm    # spawn xterm in :2 to play with
./test-i3-env.sh stop     # tear down
```

Then inside the i3 sandbox:

```sh
env -u SWAYSOCK -u WAYLAND_DISPLAY \
    I3SOCK=$(ls /run/user/1000/i3/ipc-socket.* | head -1) DISPLAY=:2 \
    ./target/release/beckon -l
```

## Out of scope

No alias mapping and no resolve cache — ids resolve against OS metadata
(`.desktop` / LaunchServices / Start Menu) on every call. No interactive
launcher (use rofi for that). No window tiling or layout management.

The one config file beckon reads is the `--serve` shortcuts TOML, and it maps
hotkeys to Names — it is not a place to alias or configure `beckon <id>`
itself, which stays config-free. Hotkey registration is still the dotfile's
job everywhere except macOS/Windows resident mode.

## License

Licensed under either of

* Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
* MIT License ([LICENSE-MIT](LICENSE-MIT))

at your option.

### Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in this crate by you, as defined in the Apache-2.0 license, shall
be dual licensed as above, without any additional terms or conditions.
