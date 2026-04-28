# openbox / awesome / fluxbox

These are EWMH-compliant minimal X11 window managers. beckon talks to
all of them through `_NET_CLIENT_LIST_STACKING` + `_NET_ACTIVE_WINDOW`
atoms — no WM-specific code path.

## Install (openbox)

```sh
cargo install --git https://github.com/xom11/beckon
```

Open `~/.config/openbox/rc.xml`, find the `<keyboard>` section, and
paste the contents of [`rc.xml.snippet`](rc.xml.snippet) inside it.
Then reload:

```sh
openbox --reconfigure
```

## awesome

Bindings live in your Lua config (`~/.config/awesome/rc.lua`). Add:

```lua
awful.key({ "Mod4" }, "space", function () awful.spawn("beckon kitty") end),
awful.key({ "Mod4" }, "c",     function () awful.spawn("beckon Claude") end),
awful.key({ "Mod4" }, "b",     function () awful.spawn("beckon Brave") end),
awful.key({ "Mod4" }, "e",     function () awful.spawn("beckon Cursor") end),
awful.key({ "Mod4" }, "d",     function () awful.spawn("beckon Discord") end),
```

Plug those into your `globalkeys` table. Reload with `Mod4+Ctrl+r`.

## fluxbox

Add to `~/.fluxbox/keys`:

```
Mod4 space :Exec beckon kitty
Mod4 c     :Exec beckon Claude
Mod4 b     :Exec beckon Brave
Mod4 e     :Exec beckon Cursor
Mod4 d     :Exec beckon Discord
```

Reload: `Reconfigure` from the root menu, or `Restart`.

## Troubleshooting

```sh
beckon -d
xprop -root _NET_SUPPORTED   # must list _NET_ACTIVE_WINDOW
```

If `_NET_ACTIVE_WINDOW` isn't advertised, your WM doesn't speak
EWMH and beckon can't focus windows on it. Check whether your WM has
an "EWMH compliance" config option (older fluxbox versions need
`session.screen0.fullMaximization: true` and a recent build).
