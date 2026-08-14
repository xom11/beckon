#!/usr/bin/env bash
# Wire beckon hotkeys into a GNOME X11 session via gsettings custom
# keybindings. Idempotent — re-running replaces the same five entries.
#
# Requires:
#   - GNOME on X11 (run `echo $XDG_SESSION_TYPE` — must say `x11`)
#   - beckon on PATH (or an absolute path you set in BECKON_BIN below)
#
# Verify: open Settings → Keyboard → View and Customize Shortcuts →
# Custom Shortcuts. You should see the five entries this script writes.

set -euo pipefail

BECKON_BIN="${BECKON_BIN:-$(command -v beckon || true)}"
if [[ -z "$BECKON_BIN" ]]; then
    echo "error: beckon not found in PATH. Set BECKON_BIN=/abs/path or install beckon first." >&2
    exit 1
fi

# Each entry: name|binding|app
#
# Ctrl + Super + Alt, the same chord as every other example, written in the
# order the three keys sit in on the bottom row. GTK parses the modifiers as a
# set, so the order here is for the reader; `<Control>` and `<Primary>` are the
# same key and GNOME's own defaults use `<Control>`.
ENTRIES=(
    "beckon-terminal|<Control><Super><Alt>t|kitty"
    "beckon-chrome|<Control><Super><Alt>c|Google Chrome"
    "beckon-code|<Control><Super><Alt>v|Visual Studio Code"
    "beckon-files|<Control><Super><Alt>f|Files"
    "beckon-spotify|<Control><Super><Alt>s|Spotify"
)

# Build the path list gsettings expects. Each binding lives at
# /org/gnome/.../custom-keybindings/beckon-N/.
PATHS=""
for i in "${!ENTRIES[@]}"; do
    PATHS+="'/org/gnome/settings-daemon/plugins/media-keys/custom-keybindings/beckon-$i/'"
    [[ $i -lt $((${#ENTRIES[@]} - 1)) ]] && PATHS+=", "
done

gsettings set org.gnome.settings-daemon.plugins.media-keys custom-keybindings "[$PATHS]"

# Populate each binding's name / binding / command.
for i in "${!ENTRIES[@]}"; do
    IFS='|' read -r name binding app <<<"${ENTRIES[$i]}"
    schema="org.gnome.settings-daemon.plugins.media-keys.custom-keybinding:/org/gnome/settings-daemon/plugins/media-keys/custom-keybindings/beckon-$i/"
    gsettings set "$schema" name "$name"
    gsettings set "$schema" binding "$binding"
    gsettings set "$schema" command "$BECKON_BIN $app"
done

echo "Done. Five beckon shortcuts wired:"
for i in "${!ENTRIES[@]}"; do
    IFS='|' read -r _ binding app <<<"${ENTRIES[$i]}"
    printf "  %-15s → beckon %s\n" "$binding" "$app"
done
echo
echo "Test one: press the binding, or run \`beckon doctor\` to check the env."
