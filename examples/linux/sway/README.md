# sway

sway is a Wayland tiling compositor. beckon talks to it through the
shared i3-IPC protocol via the `SWAYSOCK` env var that sway sets in
its session.

## Install

```sh
# 1. install beckon
cargo install --git https://github.com/xom11/beckon

# 2. drop the binding file into sway's config dir
cp beckon.conf ~/.config/sway/beckon.conf

# 3. tell sway to read it
echo "include ~/.config/sway/beckon.conf" >> ~/.config/sway/config

# 4. reload
swaymsg reload
```

## Customizing

Edit `~/.config/sway/beckon.conf` and change the Name on each line.
Run `beckon -L` first to see the exact Names available on your
machine — Brave PWAs, Flatpaks and snap-packaged apps all have
their own `.desktop` entries.

## Troubleshooting

If a hotkey doesn't do anything, the failure goes to stderr — which
sway sends to its log. Check it:

```sh
journalctl --user -t sway -e | grep beckon
```

beckon also fires a `notify-send` desktop notification on errors,
provided your session has a notification daemon (mako, dunst, etc.).

`beckon -d` diagnoses environment problems (missing `SWAYSOCK`,
notification daemon down, etc.) without you having to press a hotkey.
