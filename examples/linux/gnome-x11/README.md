# GNOME (X11)

GNOME on X11 uses Mutter, which exposes EWMH atoms beckon needs. The
**Wayland** session of GNOME blocks external focus by design — beckon
can't work there. Switch to "GNOME on Xorg" at the login screen.

> Verify your session type:
> ```sh
> echo $XDG_SESSION_TYPE     # must say "x11"
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
   - **Name**: `beckon Claude`
   - **Command**: `beckon Claude` (or the absolute path printed by `which beckon`)
   - **Shortcut**: press `Super+C`
3. Repeat for each app you want a hotkey for.

Names must match what `beckon -L` reports. Run that first.

## Troubleshooting

```sh
beckon -d            # check DISPLAY + EWMH support
beckon -l            # list windows beckon can see
beckon -r Claude     # validate that "Claude" resolves
```

If the hotkey works but focus doesn't change, `xprop _NET_SUPPORTED -root`
should list `_NET_ACTIVE_WINDOW`. If it doesn't, your WM doesn't speak
EWMH — beckon can't help there.
