# XFCE

xfwm4 supports EWMH so beckon works. XFCE is X11-only, so there's no
Wayland session caveat to worry about.

## Wire bindings via the Settings UI

1. **Settings → Keyboard → Application Shortcuts → Add**
2. **Command**: `beckon Claude` (or the absolute path from `which beckon`).
3. Press the hotkey (e.g. `Super+C`) when prompted.
4. Repeat for each app.

Recommended bindings (matching the rest of the examples):

| Trigger | Action |
|---|---|
| `Super+space` | `beckon kitty` |
| `Super+c`     | `beckon Claude` |
| `Super+b`     | `beckon Brave` |
| `Super+e`     | `beckon Cursor` |
| `Super+d`     | `beckon Discord` |

## Or wire bindings via `xfconf-query`

Faster than clicking through the UI five times:

```sh
BECKON="$(command -v beckon)"

xfconf-query -c xfce4-keyboard-shortcuts -np "/commands/custom/<Super>space" -t string -s "$BECKON kitty"
xfconf-query -c xfce4-keyboard-shortcuts -np "/commands/custom/<Super>c"     -t string -s "$BECKON Claude"
xfconf-query -c xfce4-keyboard-shortcuts -np "/commands/custom/<Super>b"     -t string -s "$BECKON Brave"
xfconf-query -c xfce4-keyboard-shortcuts -np "/commands/custom/<Super>e"     -t string -s "$BECKON Cursor"
xfconf-query -c xfce4-keyboard-shortcuts -np "/commands/custom/<Super>d"     -t string -s "$BECKON Discord"
```

The `-n` (create if missing) and `-p` (property path) flags together
add new entries idempotently. Re-run safely.

To remove a binding:

```sh
xfconf-query -c xfce4-keyboard-shortcuts -p "/commands/custom/<Super>c" -r
```

## Troubleshooting

```sh
beckon -d
beckon -l
```

If your hotkey conflicts with an existing xfwm4 shortcut, the
existing one wins. Run `xfconf-query -c xfce4-keyboard-shortcuts -lv`
to dump everything that's currently bound.
