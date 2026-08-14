# openbox / awesome / fluxbox

These are EWMH-compliant minimal X11 window managers. beckon talks to
all of them through `_NET_CLIENT_LIST_STACKING` + `_NET_ACTIVE_WINDOW`
atoms — no WM-specific code path.

All three bind `Ctrl + Super + Alt` + a letter, the same chord as every
other example. Each spells it differently: openbox writes `C-W-A-`,
awesome and fluxbox both use X11's own names, where `Mod4` is Super and
`Mod1` is Alt.

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
awful.key({ "Control", "Mod4", "Mod1" }, "t", function () awful.spawn("beckon kitty") end),
awful.key({ "Control", "Mod4", "Mod1" }, "c", function () awful.spawn({"beckon", "Google Chrome"}) end),
awful.key({ "Control", "Mod4", "Mod1" }, "v", function () awful.spawn({"beckon", "Visual Studio Code"}) end),
awful.key({ "Control", "Mod4", "Mod1" }, "f", function () awful.spawn("beckon Files") end),
awful.key({ "Control", "Mod4", "Mod1" }, "s", function () awful.spawn("beckon Spotify") end),
```

Plug those into your `globalkeys` table. Reload with `Mod4+Ctrl+r`.

## fluxbox

Add to `~/.fluxbox/keys`:

```
Control Mod4 Mod1 t     :Exec beckon kitty
Control Mod4 Mod1 c     :Exec beckon "Google Chrome"
Control Mod4 Mod1 v     :Exec beckon "Visual Studio Code"
Control Mod4 Mod1 f     :Exec beckon Files
Control Mod4 Mod1 s     :Exec beckon Spotify
```

Reload: `Reconfigure` from the root menu, or `Restart`.

## Troubleshooting

```sh
beckon doctor
xprop -root _NET_SUPPORTED   # must list _NET_ACTIVE_WINDOW
```

If `_NET_ACTIVE_WINDOW` isn't advertised, your WM doesn't speak
EWMH and beckon can't focus windows on it. Check whether your WM has
an "EWMH compliance" config option (older fluxbox versions need
`session.screen0.fullMaximization: true` and a recent build).
