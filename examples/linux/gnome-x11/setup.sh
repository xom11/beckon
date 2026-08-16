#!/usr/bin/env bash
# Wire beckon hotkeys into a GNOME X11 session via gsettings custom
# keybindings. Idempotent — re-running replaces the same five entries, and
# leaves every custom shortcut you wired by hand where it was.
#
# Requires:
#   - GNOME on X11 (run `echo $XDG_SESSION_TYPE` — must say `x11`)
#   - beckon on PATH (or an absolute path you set in BECKON_BIN below)
#
# Verify: open Settings → Keyboard → View and Customize Shortcuts →
# Custom Shortcuts. You should see the five entries this script writes, plus
# whatever was already there.
#
# `--self-test` checks this script's own list handling against the strings
# gsettings really prints, and touches no dconf key. It needs no GNOME.

set -euo pipefail

GS_SCHEMA=org.gnome.settings-daemon.plugins.media-keys
GS_KEY=custom-keybindings
BASE=/org/gnome/settings-daemon/plugins/media-keys/custom-keybindings

# `gsettings get` prints a GVariant array — `['/a/', '/b/']` — or `@as []`
# when the list is empty, which is the type-annotated spelling GLib uses for
# an empty `as`; a plain `[]` also turns up. Print one path per line, and
# nothing at all for either empty spelling: the `sed` prints only lines that
# really held a quoted string, so neither annotation can leak through as a
# path. A dconf path holds no comma and no quote, so splitting on `,` is safe.
parse_list() {
    printf '%s\n' "$1" | tr ',' '\n' | sed -n "s/^[^']*'\([^']*\)'.*$/\1/p"
}

# The list to write: every path already in the key that is not beckon's, then
# beckon's own. Usage: merged_list <current-gsettings-value> <path>...
#
# Writing only beckon's paths — which this script did until 2026-08-16 — drops
# every hand-wired custom shortcut out of the list gnome-settings-daemon reads,
# so they stop working the moment someone runs the example. The per-path dconf
# data survives, so it is recoverable, but nothing on screen says so.
#
# Dropping `$BASE/beckon-*` before appending is what keeps a re-run idempotent
# in both directions: the same five paths cannot accumulate, and shrinking
# ENTRIES cannot leave an orphan pointing at a binding nobody writes any more.
merged_list() {
    local current=$1 out="" p
    shift
    while IFS= read -r p; do
        [[ -z "$p" ]] && continue
        case "$p" in
        "$BASE"/beckon-*) continue ;;
        esac
        out+="'$p', "
    done < <(parse_list "$current")
    for p in "$@"; do
        out+="'$p', "
    done
    printf '[%s]' "${out%, }"
}

self_test() {
    local fails=0
    local mine=("$BASE/beckon-0/" "$BASE/beckon-1/")
    local ours="'$BASE/beckon-0/', '$BASE/beckon-1/'"
    local theirs="$BASE/custom0/"

    check() { # check <what> <want> <got>
        if [[ "$2" == "$3" ]]; then
            printf '  PASS %s\n' "$1"
        else
            printf '  FAIL %s\n       want %s\n       got  %s\n' "$1" "$2" "$3"
            fails=$((fails + 1))
        fi
    }

    check "an empty annotated list parses to nothing" "" "$(parse_list '@as []')"
    check "a bare empty list parses to nothing" "" "$(parse_list '[]')"
    check "two paths parse to one per line" \
        "/a/
/b/" "$(parse_list "['/a/', '/b/']")"

    check "no shortcuts yet: beckon's five stand alone" \
        "[$ours]" "$(merged_list '@as []' "${mine[@]}")"
    check "a bare empty list reads the same way" \
        "[$ours]" "$(merged_list '[]' "${mine[@]}")"

    local with_user
    with_user="$(merged_list "['$theirs']" "${mine[@]}")"
    check "the user's own shortcut survives" "['$theirs', $ours]" "$with_user"
    check "re-running writes the same list" \
        "$with_user" "$(merged_list "$with_user" "${mine[@]}")"
    check "a stale beckon-N from an older run is dropped" \
        "['$theirs', $ours]" \
        "$(merged_list "['$BASE/beckon-9/', '$theirs']" "${mine[@]}")"

    if [[ $fails -eq 0 ]]; then
        echo "self-test: all checks passed"
    else
        echo "self-test: $fails check(s) failed" >&2
        return 1
    fi
}

if [[ "${1:-}" == "--self-test" ]]; then
    self_test
    exit $?
fi

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

# Each binding lives at /org/gnome/.../custom-keybindings/beckon-N/.
MINE=()
for i in "${!ENTRIES[@]}"; do
    MINE+=("$BASE/beckon-$i/")
done

gsettings set "$GS_SCHEMA" "$GS_KEY" \
    "$(merged_list "$(gsettings get "$GS_SCHEMA" "$GS_KEY")" "${MINE[@]}")"

# Populate each binding's name / binding / command.
for i in "${!ENTRIES[@]}"; do
    IFS='|' read -r name binding app <<<"${ENTRIES[$i]}"
    schema="$GS_SCHEMA.custom-keybinding:$BASE/beckon-$i/"
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
