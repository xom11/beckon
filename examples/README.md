# beckon examples

On Linux, beckon doesn't register hotkeys itself — your compositor or
window manager dotfile does. On macOS and Windows you get a choice:
the same dotfile approach (Hammerspoon / AutoHotkey), or beckon's own
resident mode, which hosts the hotkeys from a TOML file. These examples
cover both, with a consistent set of bindings so you only need to learn
one mental model.

```
press hotkey         (registered by your OS/WM dotfile,
   │                  or by `beckon serve` on macOS / Windows)
   │
   └── invokes:  beckon <Name>
                   │
                   ├── if <Name> isn't running     → launch it
                   │   if running, not focused     → focus it
                   │   if focused, more windows    → cycle to next of same app
                   │   if focused, lone window     → toggle to last-used app
                   │   if nothing else exists      → hide it
```

## Pick your setup

### Linux

| Compositor / DE | Backend | Config |
|---|---|---|
| sway (Wayland) | i3-IPC (shared) | [`linux/sway/`](linux/sway/) |
| i3 (X11) | i3-IPC (shared) | [`linux/i3/`](linux/i3/) |
| Hyprland (Wayland) | Hyprland Unix-socket IPC | [`linux/hyprland/`](linux/hyprland/) |
| GNOME on X11 | EWMH (`x11rb`) | [`linux/gnome-x11/`](linux/gnome-x11/) |
| KDE Plasma on X11 | EWMH (`x11rb`) | [`linux/kde/`](linux/kde/) |
| XFCE | EWMH (`x11rb`) | [`linux/xfce/`](linux/xfce/) |
| openbox / awesome / fluxbox | EWMH (`x11rb`) | [`linux/openbox/`](linux/openbox/) |
| GNOME on Wayland | bundled shell extension over D-Bus | [`../extensions/`](../extensions/) — install it, then bind keys in GNOME Settings |
| KDE Plasma on Wayland | KWin scripting over D-Bus | [`linux/kde/`](linux/kde/) — same Custom Shortcuts UI as the X11 session |

> Both Wayland desktops block external focus by design, and each needs a
> collaborator running *inside* the compositor. The difference is what
> that costs you: **KDE** needs nothing — KWin ships its own scripting
> engine, so beckon loads a script, reads the answer back, and unloads
> it. **GNOME** needs the bundled `beckon@xom11.github.io` extension
> installed, plus a logout and login, before anything works.

### macOS

| Hotkey source | Config |
|---|---|
| Hammerspoon | [`macos/hammerspoon/`](macos/hammerspoon/) |
| beckon itself (`serve` + launchd) | [`macos/serve/`](macos/serve/) |

### Windows

| Hotkey source | Config |
|---|---|
| AutoHotkey v2 | [`windows/ahk/`](windows/ahk/) |
| beckon itself (`serve` + Scheduled Task) | [`windows/serve/`](windows/serve/) |

Pick one per machine, not both — a hotkey chord goes to whichever
daemon registers it first, so running two just makes the second one
lose keys.

## Common app set used in every example

Every config wires the same five hotkeys so you only need to remember
the letter, not the modifier:

| Letter | App | Notes |
|---|---|---|
| `T` | Terminal | `Terminal` on macOS and Windows, `kitty` in the Linux examples — change to yours |
| `C` | Chrome | `Google Chrome` everywhere; swap for Firefox / Brave / Vivaldi |
| `V` | VS Code | `Visual Studio Code`, not `Code`; swap for Cursor, Zed, Sublime |
| `F` | Files | `Finder` on macOS, `File Explorer` on Windows, `Files` or `Dolphin` on Linux |
| `S` | Spotify | `Spotify` everywhere; swap for whatever plays your music |

The file manager is the one every desktop ships and no two agree on the
name of, which is exactly the case `beckon search files` exists for.

Modifier keys vary because each OS picks something idiomatic:

| OS | Modifier |
|---|---|
| Linux | `Super` (Mod4 — the Windows key) |
| macOS | Hyper (`cmd + ctrl + alt`) |
| Windows | `Ctrl + Win + Alt` |

## Discovering ids on your machine

Names in the examples (`Google Chrome`, `Spotify`, `kitty`, ...) are what
beckon resolves against your installed-app metadata. Some apps have
slightly different display names (e.g. `Visual Studio Code` instead
of `Code`). Always check before binding:

```sh
beckon installed         # list installed apps with their Name
beckon list              # list currently running apps
beckon search files      # search by partial name
beckon resolve Spotify   # validate one id — shows match type + Exec
beckon doctor            # diagnose your environment
```

If `beckon resolve Spotify` reports `❌ no match`, copy the actual Name
from `beckon installed` into your hotkey binding instead.

## Why one tool, many configs?

beckon is intentionally a thin CLI. On Linux there is no app-level way
to grab a global hotkey — Wayland has no such API and the compositor
already owns keybinds — so beckon leaves that job to the config that
does it well. The examples here just plug `beckon <Name>` into the
right place in each tool's config language.

macOS and Windows do expose a hotkey API that doesn't need a
permission prompt (`RegisterEventHotKey` / `RegisterHotKey`), so
there `serve` can skip the middleman entirely. The Hammerspoon and
AutoHotkey examples remain first-class — keep them if you already run
those tools for other automation.

Either way: same Names everywhere, and zero alias mapping. The only
config file beckon ever reads is the `serve` shortcuts TOML, which
maps keys to Names — `beckon <Name>` itself stays config-free and
resolves against your OS's own app metadata.
