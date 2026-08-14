# XFCE

xfwm4 supports EWMH so beckon works. XFCE is X11-only, so there's no
Wayland session caveat to worry about.

## Wire bindings via the Settings UI

1. **Settings → Keyboard → Application Shortcuts → Add**
2. **Command**: `beckon Spotify` (or the absolute path from `which beckon`).
3. Press the hotkey (e.g. `Ctrl+Super+Alt+C`) when prompted.
4. Repeat for each app.

Recommended bindings (matching the rest of the examples):

| Trigger | Action |
|---|---|
| `Ctrl+Super+Alt+t`     | `beckon kitty` |
| `Ctrl+Super+Alt+c`     | `beckon "Google Chrome"` |
| `Ctrl+Super+Alt+v`     | `beckon "Visual Studio Code"` |
| `Ctrl+Super+Alt+f`     | `beckon Files` (Thunar on a stock XFCE — check `beckon search files`) |
| `Ctrl+Super+Alt+s`     | `beckon Spotify` |

## Or wire bindings via `xfconf-query`

Faster than clicking through the UI five times:

```sh
BECKON="$(command -v beckon)"

xfconf-query -c xfce4-keyboard-shortcuts -np "/commands/custom/<Primary><Alt><Super>t"     -t string -s "$BECKON kitty"
xfconf-query -c xfce4-keyboard-shortcuts -np "/commands/custom/<Primary><Alt><Super>c"     -t string -s "$BECKON Google Chrome"
xfconf-query -c xfce4-keyboard-shortcuts -np "/commands/custom/<Primary><Alt><Super>v"     -t string -s "$BECKON Visual Studio Code"
xfconf-query -c xfce4-keyboard-shortcuts -np "/commands/custom/<Primary><Alt><Super>f"     -t string -s "$BECKON Thunar"
xfconf-query -c xfce4-keyboard-shortcuts -np "/commands/custom/<Primary><Alt><Super>s"     -t string -s "$BECKON Spotify"
```

The property name **is** the accelerator, so it is written in GTK's own
order — `<Primary>` (Ctrl), `<Alt>`, `<Super>` — rather than in the
keyboard order the table above uses. Same three keys either way; this
spelling is what the Settings UI writes when you record the shortcut by
hand, so the two routes produce one entry instead of two.

The `-n` (create if missing) and `-p` (property path) flags together
add new entries idempotently. Re-run safely.

To remove a binding:

```sh
xfconf-query -c xfce4-keyboard-shortcuts -p "/commands/custom/<Primary><Alt><Super>c" -r
```

## Troubleshooting

```sh
beckon doctor
beckon list
```

If your hotkey conflicts with an existing xfwm4 shortcut, the
existing one wins. Run `xfconf-query -c xfce4-keyboard-shortcuts -lv`
to dump everything that's currently bound.
