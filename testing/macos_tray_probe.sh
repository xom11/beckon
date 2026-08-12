#!/bin/bash
# Does beckon's menu bar item appear, and under which run loop?
#
#   ./testing/macos_tray_probe.sh
#
# RUN THIS FROM Terminal.app ON THE MACHINE. Not over SSH: an SSH shell is in
# the "Background" bootstrap namespace, where `screencapture` refuses to run
# and `statusItemWithLength` returns a live object that draws nothing. The
# probe binary refuses to start there, so a mistake is loud rather than
# silent -- but the capture below would be blind either way.
#
# The question is whether an NSStatusItem survives Carbon's
# RunApplicationEventLoop, which is what `hotkey::run_forever` uses today. If
# it does, no run loop changes and the hotkey path is not put at risk at all.
#
# baseline.png is the CONTROL. If the system clock is not legible in it, the
# capture is blind and nothing else in the run means anything -- a blind
# camera and a missing icon produce the same empty menu bar.

set -u
cd "$(dirname "$0")/.." || exit 1

OUT="${TMPDIR:-/tmp}/beckon-tray-probe"
mkdir -p "$OUT"
BAR="-R0,0,1920,40" # full width, top 40 points

say() { printf '\n=== %s ===\n' "$1"; }

say "environment"
NS="$(launchctl managername 2>/dev/null)"
printf 'bootstrap namespace : %s\n' "$NS"
if [ "$NS" != "Aqua" ]; then
  printf '\nSTOP: need an Aqua session. Open Terminal.app on the Mac and run this there.\n'
  exit 3
fi

say "build"
# --examples, not --all-targets: only the probe is needed here, and the
# beckon-windows crate does not build on a macOS host at all.
cargo build -p beckon-macos --examples || exit 1
PROBE=target/debug/examples/tray_probe

say "control: menu bar with no probe running"
screencapture -x $BAR "$OUT/baseline.png"
printf 'screencapture exit=%s\n' "$?"

for mode in carbon nsapp; do
  say "mode: $mode"
  "$PROBE" "$mode" >"$OUT/stdout-$mode.txt" 2>&1 &
  pid=$!
  sleep 3
  if kill -0 "$pid" 2>/dev/null; then
    screencapture -x $BAR "$OUT/$mode.png"
    printf 'screencapture exit=%s\n' "$?"
    kill "$pid" 2>/dev/null
  else
    printf 'probe exited before the capture:\n'
  fi
  wait "$pid" 2>/dev/null
  cat "$OUT/stdout-$mode.txt"
done

say "results"
printf 'PNGs in %s\n' "$OUT"
ls -la "$OUT"/*.png 2>/dev/null
cat <<EOF

Read the three PNGs. Look for the word "beckon" in the menu bar.

  visible in BOTH carbon.png and nsapp.png
      -> keep RunApplicationEventLoop; nothing about hotkeys changes.
  visible in nsapp.png ONLY
      -> the run loop must change, and RegisterEventHotKey must then be
         re-measured under [NSApp run] before that swap is trusted.
  visible in NEITHER, clock visible in baseline.png
      -> a single-process status item is not possible; see the spec's
         rejected two-process alternative.
  clock NOT visible in baseline.png
      -> nothing was measured. Grant Terminal.app Screen Recording and rerun.

Then open the menu by hand and click a row: stdout must print "menu click".
The screenshot cannot show that, and a drawn-but-dead menu looks identical.
EOF
