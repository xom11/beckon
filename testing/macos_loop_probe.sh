#!/bin/sh
# Answer design §5: does an AppKit control receive events under the loop
# `serve` actually runs?
#
# See `crates/beckon-macos/examples/loop_probe.rs` for what is measured and
# why. This script is the half that makes the measurement possible without a
# person at the keyboard, which is the reason §5 stayed open: the probe's
# button is pressed from OUTSIDE the process, over the Accessibility API.
#
#   ./testing/macos_loop_probe.sh carbon      # the loop serve runs today
#   ./testing/macos_loop_probe.sh nsapp       # the control
#
# Run BOTH. One mode alone cannot tell "Cocoa gets no events under Carbon"
# from "this probe is broken" -- the second mode is the positive control, and
# this repo has been bitten three times by a clean negative from a blind
# detector.
#
# ## Why it goes through Terminal.app
#
# An SSH shell, and a shell under a coding agent, is in the `Background`
# bootstrap namespace: AppKit hands back live objects and draws nothing, and
# the probe refuses to run there rather than produce a confident false
# negative. `sudo launchctl asuser` reaches Aqua but wants a password.
# Driving Terminal.app over Automation reaches Aqua with neither, because
# Terminal is itself an Aqua app and `do script` runs as its child.
#
# Requires: Automation permission for whatever runs this (to control Terminal
# and System Events) and an Accessibility grant (to press the button).
# Neither is Screen Recording -- nothing here takes a screenshot.

set -u

MODE="${1:-}"
case "$MODE" in
  carbon|nsapp) ;;
  *) echo "usage: $0 <carbon|nsapp>" >&2; exit 2 ;;
esac

ROOT=$(cd "$(dirname "$0")/.." && pwd)
OUT="${TMPDIR:-/tmp}/beckon-loop-probe-$MODE.txt"
BIN="${CARGO_TARGET_DIR:-$ROOT/target}/debug/examples/loop_probe"

[ -x "$BIN" ] || { echo "not built: $BIN" >&2; echo "run: cargo build -p beckon-macos --example loop_probe" >&2; exit 2; }

rm -f "$OUT"
echo "=== mode: $MODE ==="
echo "binary : $BIN"
echo "output : $OUT"

# Terminal's `do script` runs in a login shell in the Aqua namespace.
# `exit` closes the tab afterwards so repeated runs do not pile up windows.
osascript >/dev/null 2>&1 <<OSA
tell application "Terminal"
    do script "POLICY='${POLICY:-}' PRESS='${PRESS:-}' '$BIN' $MODE > '$OUT' 2>&1; exit"
end tell
OSA

# Wait for the first HEARTBEAT, NOT for "WINDOW: up".
#
# **Measured 2026-08-16, and it cost a whole inconclusive run.** `WINDOW: up`
# is printed after `makeKeyAndOrderFront` and `activate` but BEFORE the run
# loop starts turning, and an AppKit window is not an accessibility citizen
# until the process is pumping events: System Events answered
# `(count of windows) is 0` on a process that had already ordered its window
# front. Waiting for a line that only the running loop can emit makes the
# precondition the same fact as the thing being waited for.
i=0
while [ $i -lt 80 ]; do
    grep -q "HEARTBEAT 1" "$OUT" 2>/dev/null && break
    grep -q "REFUSING" "$OUT" 2>/dev/null && { cat "$OUT"; exit 3; }
    i=$((i + 1))
    /bin/sleep 0.25
done
if [ $i -ge 80 ]; then
    echo "--- the probe never reached a turning run loop ---"
    cat "$OUT" 2>/dev/null
    /usr/bin/pkill -x loop_probe 2>/dev/null
    exit 4
fi

# `PRESS=external`: the probe draws, THIS shell posts.
#
# The split is not tidiness. Drawing needs the `Aqua` namespace, which this
# shell is not in; posting needs an Accessibility grant, which the
# Terminal-launched probe does not have — measured, and silently, because
# `CGEventPost` returns void and an untrusted post is a no-op with no error.
# The control that proved it: the same HID click failed in `nsapp` mode,
# where the in-process `postEvent:` route had already succeeded.
if [ "${PRESS:-}" = "external" ]; then
    i=0
    while [ $i -lt 40 ]; do
        grep -q "CLICK-AT" "$OUT" 2>/dev/null && break
        i=$((i + 1)); /bin/sleep 0.25
    done
    COORDS=$(sed -n 's/^CLICK-AT: //p' "$OUT" 2>/dev/null | head -1)
    if [ -z "$COORDS" ]; then
        echo "--- the probe never published a click target ---"
        cat "$OUT" 2>/dev/null; /usr/bin/pkill -x loop_probe 2>/dev/null; exit 4
    fi
    CLICKER="${CARGO_TARGET_DIR:-$ROOT/target}/debug/examples/hid_click"
    [ -x "$CLICKER" ] || { echo "not built: $CLICKER" >&2; /usr/bin/pkill -x loop_probe 2>/dev/null; exit 2; }
    echo "inject : $CLICKER $COORDS"
    # shellcheck disable=SC2086
    "$CLICKER" $COORDS || { echo "--- injector refused; result INCONCLUSIVE ---"; /usr/bin/pkill -x loop_probe 2>/dev/null; exit 5; }
fi

# Wait for the probe's own verdict. Everything after the loop starts is
# in-process: the probe posts a synthetic click into NSApp's own event queue
# and reports whether the button's action ran.
#
# **The Accessibility route was tried first and abandoned, measured.** System
# Events answered `count of windows` = 0 for this probe -- and, as a control,
# 0 for Terminal and 0 for Finder, on a machine where `AXIsProcessTrusted()`
# is true and System Events' `UI elements enabled` is true. An outside press
# would have measured that blindness, not the loop.
i=0
while [ $i -lt 60 ]; do
    grep -q "FIRED\|NOT-FIRED" "$OUT" 2>/dev/null && break
    i=$((i + 1))
    /bin/sleep 0.25
done

echo "--- probe output ---"
cat "$OUT"

# Leave nothing behind: a probe still holding a window would be pressed by
# the next run and report the previous mode's answer.
/usr/bin/pkill -x loop_probe 2>/dev/null

grep -q "^FIRED" "$OUT" 2>/dev/null && exit 0 || exit 1
