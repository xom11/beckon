# beckon examples

beckon doesn't register hotkeys itself — your existing OS / window
manager dotfile does. These examples show you how to wire that dotfile
on every supported platform, with a consistent set of bindings so you
only need to learn one mental model.

```
press hotkey         (registered by your OS/WM dotfile)
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
| KDE Plasma on X11 | EWMH (`x11rb`) | [`linux/kde-x11/`](linux/kde-x11/) |
| XFCE | EWMH (`x11rb`) | [`linux/xfce/`](linux/xfce/) |
| openbox / awesome / fluxbox | EWMH (`x11rb`) | [`linux/openbox/`](linux/openbox/) |

> GNOME and KDE on **Wayland** block external focus by design — beckon
> doesn't work there. Switch to the X11 session, or use sway / Hyprland.

### macOS

| Hotkey daemon | Config |
|---|---|
| Hammerspoon | [`macos/hammerspoon/`](macos/hammerspoon/) |

### Windows

| Hotkey daemon | Config |
|---|---|
| AutoHotkey v2 | [`windows/ahk/`](windows/ahk/) |

## Common app set used in every example

Every config wires the same five hotkeys so you only need to remember
the letter, not the modifier:

| Letter | App | Notes |
|---|---|---|
| `Space` | terminal | kitty / Alacritty / Windows Terminal — change to your terminal |
| `C` | Claude | the desktop app or the [Claude.ai](https://claude.ai) PWA |
| `B` | Brave | swap for Firefox / Chrome / Vivaldi if you don't use Brave |
| `E` | Cursor | swap for VS Code, Zed, Sublime, etc. |
| `D` | Discord | comms — swap for Slack, Telegram, etc. |

Modifier keys vary because each OS picks something idiomatic:

| OS | Modifier |
|---|---|
| Linux | `Super` (Mod4 — the Windows key) |
| macOS | Hyper (`cmd + ctrl + alt`) |
| Windows | `Ctrl + Win + Alt` |

## Discovering ids on your machine

Names in the examples (`Claude`, `Brave`, `kitty`, ...) are what
beckon resolves against your installed-app metadata. Some apps have
slightly different display names (e.g. `Visual Studio Code` instead
of `Code`). Always check before binding:

```sh
beckon -L                # list installed apps with their Name
beckon -l                # list currently running apps
beckon -s claude         # search by partial name
beckon -r Claude         # validate one id — shows match type + Exec
beckon -d                # diagnose your environment
```

If `beckon -r Claude` reports `❌ no match`, copy the actual Name from
`beckon -L` into your hotkey binding instead.

## Why one tool, many configs?

beckon is intentionally a thin CLI. It doesn't know how to grab a
global hotkey on each platform, so it leaves that to the tools that
already do it well — the compositor on Linux, Hammerspoon on macOS,
AutoHotkey on Windows. The examples here just plug `beckon <Name>`
into the right place in each tool's config language.

This means: same Names everywhere, three different hotkey daemons,
zero alias mapping or config file inside beckon itself.
