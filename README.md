# beckon

Cross-platform focus-or-launch app switcher. A thin CLI invoked by your existing
hotkey dotfile (sway, i3, Hammerspoon, AHK).

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

# 4. press the hotkey. failures fire a desktop notification — you'll see them.
```

## Status

| Platform | Status |
|----------|--------|
| Linux / sway (Wayland) | ✅ Phase 1a — i3-IPC |
| Linux / i3 (X11) | ✅ Phase 1b — same backend (shared protocol) |
| Linux / X11 generic (GNOME-X11, KDE-X11, XFCE, openbox, awesome) | ✅ Phase 1b.x11 — `x11rb` + EWMH |
| Linux / Hyprland (Wayland) | ✅ Phase 1c — native Unix-socket IPC |
| macOS | ✅ Phase 2 — NSWorkspace + AX + CGWindowList |
| Windows | ✅ Phase 3 — Win32 EnumWindows + COM IShellLinkW |
| GNOME / KDE Wayland | ❌ Out of scope (compositor blocks external focus) |

## Build

### Cargo

```sh
cargo build --release
# binary: ./target/release/beckon
```

Requirements: Rust 1.75+. Linux supports sway, i3, Hyprland and any
EWMH-compliant X11 desktop (GNOME-X11, KDE-X11, XFCE, openbox, awesome).
GNOME and KDE on Wayland are unsupported — `beckon -d` reports it.
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

**Windows** (Start Menu `.lnk` shortcuts in `%APPDATA%\...\Start Menu\Programs\`
and `%ProgramData%\...\Start Menu\Programs\`, parsed via COM `IShellLinkW`):

1. Shortcut display name exact (case-insensitive, normalized)
2. Exe filename stem (e.g. `brave` matches `brave.exe`)
3. Shortcut display name substring (alphabetical first wins)

When the resolved exe is a launcher stub (e.g. Brave PWA `chrome_proxy.exe` →
`brave.exe`), beckon falls back to title matching against running windows.

Names are stable across machines. Brave PWA hashes are not — bind to `Claude`,
not `brave-fmpnliohj...-Default` or `com.vivaldi.Vivaldi.app.<hash>`.

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
├── beckon-windows/   # Win32 EnumWindows + COM IShellLinkW (.lnk parsing)
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

No config file. No alias mapping. No global hotkey registration — that lives
in your existing dotfile. No interactive launcher (use rofi for that).

## License

MIT OR Apache-2.0
