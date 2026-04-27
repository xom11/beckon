# beckon

Cross-platform focus-or-launch app switcher. A thin CLI invoked by your existing
hotkey dotfile (sway, i3, Hammerspoon, AHK).

```
press hotkey
  └── if app not running   → launch it
      if running, unfocused → focus it
      if already focused    → cycle windows / toggle to previous app / hide
```

## Status

| Platform | Status |
|----------|--------|
| Linux / sway (Wayland) | ✅ Phase 1a |
| Linux / i3 (X11) | ✅ Phase 1b — same backend (shared protocol) |
| Linux / X11 generic (GNOME-X11, KDE-X11, ...) | ⏳ Deferred |
| Linux / Hyprland | ⏳ Pending |
| macOS | ⏳ Pending |
| Windows | ⏳ Pending |
| GNOME / KDE Wayland | ❌ Out of scope (compositor blocks external focus) |

## Build

```sh
cargo build --release
# binary: ./target/release/beckon
```

Requirements: Rust 1.75+. On Linux: a sway or i3 session (other compositors
TBD — `beckon -d` will tell you).

## Usage

The hot path is `beckon <id>` — invoke from a hotkey binding:

```sh
beckon Claude            # focus / launch / cycle Claude
```

`<id>` resolves against installed-app metadata. On Linux this means
`.desktop` files. Priority:

1. `Name=` exact (case-insensitive, normalized) — **recommended for dotfiles**
2. `.desktop` filename
3. `StartupWMClass=`
4. `Name=` substring (alphabetical first wins, like rofi)

Names are stable across machines. Brave PWA hashes are not — bind to `Claude`,
not `brave-fmpnliohj...-Default`.

### Discovery

```sh
beckon -l           # list running apps with their app_ids
beckon -L           # list installed apps (parsed from .desktop)
beckon -s claude    # fuzzy-search ids matching "claude"
beckon -r Claude    # show how an id resolves (match type, exec, status)
beckon -d           # check environment (compositor / IPC / notification daemon)
```

### Sway dotfile example

```
set $focus exec /path/to/beckon

bindsym $mod+space $focus kitty
bindsym $mod+c     $focus Claude
bindsym $mod+g     $focus Gemini
bindsym $mod+t     $focus "Telegram Web"
```

If `beckon -L` shows two apps with the same Name (e.g. native Telegram +
Telegram Web PWA), bind to the more specific Name to disambiguate.

## What `beckon <id>` actually does

Single algorithm, not configurable:

```
1. resolve id → desktop entry (Name match, etc.)
2. if no window of this app  → launch via Exec line
3. if running but unfocused  → focus first window
4. if focused, more windows  → cycle to next window of same app
5. if focused, sole window   → toggle to the previously focused app
                               (tracked at $XDG_RUNTIME_DIR/beckon-mru)
6. if nothing else exists    → hide via scratchpad
```

When a hotkey-bound invocation fails (id not found, IPC error), beckon fires
a desktop notification via `notify-send`. Run from a terminal to see errors
on stderr instead.

## Project layout

```
crates/
├── beckon-core/    # Backend trait, shared types
├── beckon-linux/   # sway + i3 (i3-IPC), .desktop parser, MRU state
└── beckon-cli/     # binary, clap CLI, doctor / search / resolve
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
