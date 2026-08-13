# KDE Plasma (X11 and Wayland)

Both sessions work, through different backends — beckon picks the right
one on its own, and the hotkey setup below is identical either way.

| Session | Backend | Needs installing |
|---|---|---|
| Plasma on X11 | EWMH via `x11rb` | nothing |
| Plasma on Wayland | KWin's own scripting engine over D-Bus | nothing |

KWin on Wayland does block external processes from focusing windows —
that part of the Wayland security model is real. beckon gets around it
the way KWin itself sanctions: it loads a small generated script into
KWin, reads the window list back out over D-Bus, and unloads it. Unlike
the GNOME Wayland path there is no extension to install and no logout.

> Check which one you're on:
> ```sh
> echo $XDG_SESSION_TYPE     # "x11" or "wayland"
> beckon doctor              # prints the backend beckon picked
> ```

## Wire bindings via System Settings

KDE doesn't have a stable command-line API for global shortcuts that
matches the Settings UI well, so the manual route is what's documented
here. (Power users can poke `kwriteconfig5 --file kglobalshortcutsrc`
but the schema changes between Plasma versions.)

1. **System Settings → Shortcuts → Custom Shortcuts**
2. **Edit → New → Global Shortcut → Command/URL**
3. Trigger tab: press `Meta+C` (Meta = Super = the Windows key).
4. Action tab: command = `beckon "Google Chrome"` (use `which beckon` if `beckon`
   isn't on the system-wide PATH KDE inherits).
5. Click **Apply**.
6. Repeat for each binding you want.

Recommended bindings (matching the rest of the examples):

| Trigger | Action |
|---|---|
| `Meta+T`     | `beckon kitty` |
| `Meta+C`     | `beckon "Google Chrome"` |
| `Meta+V`     | `beckon "Visual Studio Code"` |
| `Meta+F`     | `beckon Dolphin` (Plasma's file manager) |
| `Meta+S`     | `beckon Spotify` |

KDE will warn if a chosen shortcut conflicts with an existing one.
Either accept the override or pick a different letter — pressing the
same shortcut opens the previous owner's UI for confirmation. This is
worth knowing on Plasma in particular, which ships more Meta bindings
than most desktops: `Meta+Space` is KRunner, which is one reason the
terminal is on `T` here rather than on the space bar.

## Names

Run `beckon installed` first to see the exact Names KDE's `.desktop`
files expose. KDE-built apps (Konsole, Dolphin, Kate) have stable
Names; Brave PWAs and Flatpaks have whatever Name the install put into
their `.desktop` file.

## Troubleshooting

```sh
beckon doctor        # session type + which backend was picked
beckon list          # what KWin currently exposes

# X11 session only — confirm EWMH atoms are advertised:
xprop -root _NET_SUPPORTED

# Wayland session only — confirm KWin's scripting service is reachable:
busctl --user introspect org.kde.KWin /Scripting | grep -i loadScript
```

If pressing the hotkey does nothing and `beckon doctor` looks healthy,
KDE may have grabbed the key for itself — try a different letter.
