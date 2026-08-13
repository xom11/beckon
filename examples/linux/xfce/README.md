# XFCE

xfwm4 supports EWMH so beckon works. XFCE is X11-only, so there's no
Wayland session caveat to worry about.

## Wire bindings via the Settings UI

1. **Settings → Keyboard → Application Shortcuts → Add**
2. **Command**: `beckon Spotify` (or the absolute path from `which beckon`).
3. Press the hotkey (e.g. `Super+C`) when prompted.
4. Repeat for each app.

Recommended bindings (matching the rest of the examples):

| Trigger | Action |
|---|---|
| `Super+t`     | `beckon kitty` |
| `Super+c`     | `beckon "Google Chrome"` |
| `Super+v`     | `beckon "Visual Studio Code"` |
| `Super+f`     | `beckon Files` (Thunar on a stock XFCE — check `beckon search files`) |
| `Super+s`     | `beckon Spotify` |

## Or wire bindings via `xfconf-query`

Faster than clicking through the UI five times:

```sh
BECKON="$(command -v beckon)"

xfconf-query -c xfce4-keyboard-shortcuts -np "/commands/custom/<Super>t"     -t string -s "$BECKON kitty"
xfconf-query -c xfce4-keyboard-shortcuts -np "/commands/custom/<Super>c"     -t string -s "$BECKON Google Chrome"
xfconf-query -c xfce4-keyboard-shortcuts -np "/commands/custom/<Super>v"     -t string -s "$BECKON Visual Studio Code"
xfconf-query -c xfce4-keyboard-shortcuts -np "/commands/custom/<Super>f"     -t string -s "$BECKON Thunar"
xfconf-query -c xfce4-keyboard-shortcuts -np "/commands/custom/<Super>s"     -t string -s "$BECKON Spotify"
```

The `-n` (create if missing) and `-p` (property path) flags together
add new entries idempotently. Re-run safely.

To remove a binding:

```sh
xfconf-query -c xfce4-keyboard-shortcuts -p "/commands/custom/<Super>c" -r
```

## Troubleshooting

```sh
beckon doctor
beckon list
```

If your hotkey conflicts with an existing xfwm4 shortcut, the
existing one wins. Run `xfconf-query -c xfce4-keyboard-shortcuts -lv`
to dump everything that's currently bound.
