# KDE Plasma (X11)

KWin on X11 supports EWMH. The **Wayland** session of KDE blocks
external focus — beckon can't work there. Pick "Plasma (X11)" at the
login screen.

> Verify session type:
> ```sh
> echo $XDG_SESSION_TYPE     # must say "x11"
> ```

## Wire bindings via System Settings

KDE doesn't have a stable command-line API for global shortcuts that
matches the Settings UI well, so the manual route is what's documented
here. (Power users can poke `kwriteconfig5 --file kglobalshortcutsrc`
but the schema changes between Plasma versions.)

1. **System Settings → Shortcuts → Custom Shortcuts**
2. **Edit → New → Global Shortcut → Command/URL**
3. Trigger tab: press `Meta+C` (Meta = Super = the Windows key).
4. Action tab: command = `beckon Claude` (use `which beckon` if `beckon`
   isn't on the system-wide PATH KDE inherits).
5. Click **Apply**.
6. Repeat for each binding you want.

Recommended bindings (matching the rest of the examples):

| Trigger | Action |
|---|---|
| `Meta+Space` | `beckon kitty` |
| `Meta+C`     | `beckon Claude` |
| `Meta+B`     | `beckon Brave` |
| `Meta+E`     | `beckon Cursor` |
| `Meta+D`     | `beckon Discord` |

KDE will warn if a chosen shortcut conflicts with an existing one (it
binds `Meta+Space` to KRunner by default). Either accept the
override or pick a different letter — pressing the same shortcut
opens the previous owner's UI for confirmation.

## Names

Run `beckon -L` first to see the exact Names KDE's `.desktop` files
expose. KDE-built apps (Konsole, Dolphin, Kate) have stable Names;
Brave PWAs and Flatpaks have whatever Name the install put into their
`.desktop` file.

## Troubleshooting

```sh
beckon -d            # session type + EWMH detection
beckon -l            # what KWin currently exposes
xprop -root _NET_SUPPORTED   # confirm EWMH atoms are advertised
```

If pressing the hotkey does nothing and `beckon -d` looks healthy,
KDE may have grabbed the key for itself — try a different letter.
