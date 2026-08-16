#!/bin/sh
# Does a Carbon hotkey still fire under `[NSApp run]`?
#
#   ./testing/macos_hotkey_loop_probe.sh            # runs BOTH modes
#   ./testing/macos_hotkey_loop_probe.sh nsapp      # one mode
#
# See `crates/beckon-macos/examples/hotkey_loop_probe.rs` for what is measured
# and how to read the pair. **Run both.** The `carbon` run is a known-good
# baseline for the whole chain -- chord, injector, key table, registration --
# because hotkeys demonstrably worked under that loop before the change. A
# silent `nsapp` run alone cannot tell a regression from a keystroke that
# never arrived.
#
# The keystroke goes through `System Events`, which needs Automation
# permission for whatever runs this. That is a different grant from the
# Accessibility one `hid_click` needs, and on this machine it is already
# present -- which is why this probe can run unattended where the mouse one
# cannot.

set -u

ROOT=$(cd "$(dirname "$0")/.." && pwd)
BIN="${CARGO_TARGET_DIR:-$ROOT/target}/debug/examples/hotkey_loop_probe"
KEYBIN="${CARGO_TARGET_DIR:-$ROOT/target}/debug/examples/hid_key"
[ -x "$KEYBIN" ] || { echo "not built: $KEYBIN" >&2; exit 2; }
[ -x "$BIN" ] || { echo "not built: $BIN" >&2; echo "run: cargo build -p beckon-macos --example hotkey_loop_probe" >&2; exit 2; }

one_mode() {
    MODE="$1"
    OUT="${TMPDIR:-/tmp}/beckon-hotkey-loop-$MODE.txt"
    rm -f "$OUT"
    echo "=== mode: $MODE ==="

    osascript >/dev/null 2>&1 <<OSA
tell application "Terminal"
    do script "'$BIN' $MODE > '$OUT' 2>&1; exit"
end tell
OSA

    # Wait for the first HEARTBEAT, not for READY. READY is printed before the
    # loop starts turning, and a hotkey is not delivered to a process that is
    # not pumping -- the same correction `macos_loop_probe.sh` records, for the
    # same reason.
    i=0
    while [ $i -lt 60 ]; do
        grep -q "HEARTBEAT 1" "$OUT" 2>/dev/null && break
        grep -q "REFUSING\|failed" "$OUT" 2>/dev/null && { cat "$OUT"; return 3; }
        i=$((i + 1)); /bin/sleep 0.25
    done
    if [ $i -ge 60 ]; then
        echo "--- never reached a turning loop ---"; cat "$OUT" 2>/dev/null
        /usr/bin/pkill -x hotkey_loop_probe 2>/dev/null; return 4
    fi

    # kVK_ANSI_F = 3, pressed through the window server by `hid_key`.
    #
    # **Not `System Events`.** That route needs Automation permission, and on
    # this machine it does not fail -- it HANGS, waiting on a consent dialog
    # nobody may be in front of. `CGEventPost` needs Accessibility instead,
    # and the injector must therefore run from a process that has BOTH a
    # window-server session and that grant, which is why it is launched
    # through Terminal rather than from the caller's own shell.
    KEYOUT="${TMPDIR:-/tmp}/beckon-hotkey-inject-$MODE.txt"
    rm -f "$KEYOUT"
    osascript >/dev/null 2>&1 <<OSA
tell application "Terminal"
    do script "'$KEYBIN' 3 ctrl opt shift > '$KEYOUT' 2>&1; exit"
end tell
OSA
    j=0
    while [ $j -lt 20 ]; do
        grep -q "POSTED\|REFUSING" "$KEYOUT" 2>/dev/null && break
        j=$((j + 1)); /bin/sleep 0.25
    done
    # The injector's own trust line, echoed here. Without it a silent probe
    # reads as a dead loop when it may only mean nothing was ever typed.
    sed 's/^/    inject: /' "$KEYOUT" 2>/dev/null || echo "    inject: (no output)"
    if ! grep -q POSTED "$KEYOUT" 2>/dev/null; then
        echo "    the keystroke was never sent -- this run measures nothing"
        /usr/bin/pkill -x hotkey_loop_probe 2>/dev/null
        return 5
    fi

    i=0
    while [ $i -lt 24 ]; do
        grep -q "HOTKEY FIRED\|NOT-FIRED" "$OUT" 2>/dev/null && break
        i=$((i + 1)); /bin/sleep 0.25
    done

    grep -E "MODE|HOTKEY FIRED|NOT-FIRED|VERDICT" "$OUT" 2>/dev/null
    /usr/bin/pkill -x hotkey_loop_probe 2>/dev/null
    grep -q "HOTKEY FIRED" "$OUT" 2>/dev/null && return 0 || return 1
}

if [ $# -ge 1 ]; then
    one_mode "$1"; exit $?
fi

# Baseline first, so a broken injector is caught before the run that matters
# is interpreted.
one_mode carbon; CARBON=$?
echo
one_mode nsapp;  NSAPP=$?

echo
echo "=== reading the pair ==="
if [ $CARBON -ne 0 ] && [ $NSAPP -ne 0 ]; then
    echo "BOTH SILENT -- the keystroke never landed. This measures nothing."
    echo "Check Automation permission for whatever ran this, and that no"
    echo "remapper owns ctrl+alt+shift+f."
    exit 5
elif [ $CARBON -eq 0 ] && [ $NSAPP -ne 0 ]; then
    echo "REGRESSION: hotkeys fire under Carbon and NOT under [NSApp run]."
    echo "Revert hotkey::run_forever."
    exit 1
elif [ $CARBON -ne 0 ] && [ $NSAPP -eq 0 ]; then
    echo "The baseline did not fire but the new loop did. The control is"
    echo "wrong, not the result -- re-read before concluding anything."
    exit 6
else
    echo "BOTH FIRED: the loop change is safe for hotkeys."
    exit 0
fi
