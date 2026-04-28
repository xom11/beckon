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
finds the window there and `dispatch focuswindow` brings it back —
Hyprland surfaces the special workspace automatically when a window
on it gets focus.

If you want to inspect what beckon parked, run:

```sh
hyprctl clients | grep -A1 'workspace: special:beckon'
```

## Customizing

Edit `~/.config/hypr/beckon.conf` and change the Name on each `bind = ...,
exec, beckon <Name>` line. Run `beckon -L` first to see what's
installed.

## Troubleshooting

```sh
beckon -d            # check $HYPRLAND_INSTANCE_SIGNATURE + socket reachability
hyprctl clients      # see what classes the live tree exposes
```

beckon falls back to a `notify-send` notification on hotkey failure if
your session has a notification daemon.
