# GNOME (X11)

GNOME on X11 uses Mutter, which exposes the EWMH atoms beckon needs, so
there is nothing to install beyond beckon itself. That session is what
this page is about.

**The Wayland session works too**, through a different backend, and the
hotkey setup below is identical either way — but it costs an install and
a logout. Mutter does block external processes from focusing windows
there; beckon gets around it with a collaborator running *inside*
gnome-shell, the bundled extension at
[`extensions/beckon@xom11.github.io/`](../../../extensions/):

```sh
cd extensions
gnome-extensions pack beckon@xom11.github.io
gnome-extensions install --force beckon@xom11.github.io.shell-extension.zip
gnome-extensions enable beckon@xom11.github.io
# then log out and back in — Wayland can't reload the shell live
```

(Unlike KDE Wayland, which needs nothing installed: KWin ships its own
scripting engine. See [`../kde/`](../kde/).)

> Check which one you're on:
> ```sh
> echo $XDG_SESSION_TYPE     # "x11" or "wayland"
> beckon doctor              # prints the five env vars it detects on, and
>                            # whether a backend was selected at all
> ```

## Option A — automated via the included script

```sh
cargo install --git https://github.com/xom11/beckon
./setup.sh
```

The script writes five custom keybindings using `gsettings`. Open
**Settings → Keyboard → View and Customize Shortcuts → Custom
Shortcuts** to confirm they appeared. Re-running the script
overwrites the same five entries; it doesn't accumulate.

To remove every entry the script created:

```sh
gsettings reset org.gnome.settings-daemon.plugins.media-keys custom-keybindings
```

(That clears all custom keybindings, not just beckon's. Combine with
the per-path `reset` if you have other custom shortcuts to preserve.)

## Option B — manual via Settings UI

1. **Settings → Keyboard → View and Customize Shortcuts → Custom Shortcuts → +**
2. Fill in:
   - **Name**: `beckon Chrome`
   - **Command**: `beckon "Google Chrome"` (or the absolute path printed by `which beckon`)
   - **Shortcut**: hold `Ctrl + Super + Alt` and press `C`
3. Repeat for each app you want a hotkey for.

Names must match what `beckon installed` reports. Run that first.

## Troubleshooting

```sh
beckon doctor             # check DISPLAY + EWMH support
beckon list               # list windows beckon can see
beckon resolve Files      # validate that your file manager resolves
```

If the hotkey works but focus doesn't change, `xprop _NET_SUPPORTED -root`
should list `_NET_ACTIVE_WINDOW`. If it doesn't, your WM doesn't speak
EWMH — beckon can't help there.
