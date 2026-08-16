# Hyprland

Hyprland is a Wayland tiling compositor. beckon talks to it through
its native Unix-socket IPC at
`$XDG_RUNTIME_DIR/hypr/$HYPRLAND_INSTANCE_SIGNATURE/.socket.sock`
(falling back to `/tmp/hypr/...` on Hyprland < 0.40).

## Install

```sh
# 1. install beckon
cargo install --git https://github.com/xom11/beckon

# 2. drop the binding file into hyprland's config dir
cp beckon.conf ~/.config/hypr/beckon.conf

# 3. tell hyprland to read it
echo "source = ~/.config/hypr/beckon.conf" >> ~/.config/hypr/hyprland.conf
```

Hyprland watches its config file and reloads automatically. No reload
command needed.

## Hide / restore on Hyprland

Hyprland has no minimize concept the way X11 does. beckon's "hide"
step (5c) parks the window on a special workspace called
`special:beckon`. The next time you press the same hotkey, beckon
finds the window there, moves it back onto the workspace you are
looking at, and only then focuses it.

Un-parking it first is beckon's job, not the compositor's. This page
used to say `dispatch focuswindow` was enough because Hyprland surfaces
the special workspace on focus; measured on 0.56.0, it surfaces it as an
*overlay* while the window still belongs to `special:beckon`, so the
moment focus moves elsewhere the window vanishes again and `$mod+1..4`,
`movefocus` and `movetoworkspace` all behave as if it does not exist —
only `beckon <Name>` could ever bring it back. Only `special:beckon` is
un-parked; a `special:*` workspace of your own is left where you put it.

If you want to inspect what beckon parked, ask for the shape beckon
itself reads — the workspace is an object, so the name lives at
`.workspace.name`:

```sh
hyprctl -j clients | jq -r '.[] | select(.workspace.name == "special:beckon") | .class'
```

## Customizing

Edit `~/.config/hypr/beckon.conf` and change the Name on each `bind = ...,
exec, beckon <Name>` line. Run `beckon installed` first to see what's
installed.

## Troubleshooting

```sh
beckon doctor        # check $HYPRLAND_INSTANCE_SIGNATURE + socket reachability
hyprctl clients      # see what classes the live tree exposes
```

beckon falls back to a `notify-send` notification on hotkey failure if
your session has a notification daemon.
