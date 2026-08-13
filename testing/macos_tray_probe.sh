#!/bin/bash
# Does beckon's menu bar item appear, and under which run loop? And does the
# settings window draw?
#
#   ./testing/macos_tray_probe.sh
#
# RUN THIS FROM Terminal.app ON THE MACHINE. Not over SSH: an SSH shell is in
# the "Background" bootstrap namespace, where `statusItemWithLength` returns a
# live object that draws nothing. Both probe binaries refuse to start there,
# so that mistake is loud rather than silent.
#
# The question is whether an NSStatusItem survives Carbon's
# RunApplicationEventLoop, which is what `hotkey::run_forever` uses today. If
# it does, no run loop changes and the hotkey path is not put at risk at all.
#
# baseline.png is the CONTROL. If the system clock is not legible in it, the
# capture is blind and nothing else in the run means anything -- a blind
# camera and a missing icon produce the same empty menu bar.
#
# Two harness hazards this script now handles, both measured on macmini
# 2026-08-13 by getting them wrong:
#
#   * `screencapture -R x,y,w,h` REJECTS a rect it does not like with
#     "could not create image from rect" and exit 1, naming neither which
#     number was wrong nor what the bounds are. A hard-coded rect guessed
#     from "UI Looks like 1920x1080" was refused outright. So the full
#     screen is always captured first -- that form takes no arguments to get
#     wrong -- and the menu bar strip is cropped afterwards, as a bonus
#     rather than as the artifact everything depends on.
#   * The FIRST exec of a freshly linked binary is SIGKILLed on this machine
#     (empty stderr, no message). Building immediately before running walked
#     straight into it and looked exactly like the probe crashing. Every
#     probe is now warmed once and retried once.

set -u
cd "$(dirname "$0")/.." || exit 1

OUT="${TMPDIR:-/tmp}/beckon-tray-probe"
rm -rf "$OUT"
mkdir -p "$OUT"

say() { printf '\n=== %s ===\n' "$1"; }

say "environment"
NS="$(launchctl managername 2>/dev/null)"
printf 'bootstrap namespace : %s\n' "$NS"
if [ "$NS" != "Aqua" ]; then
  printf '\nSTOP: need an Aqua session. Open Terminal.app on the Mac and run this there.\n'
  exit 3
fi
system_profiler SPDisplaysDataType 2>/dev/null | grep -iE 'Resolution|UI Looks like' | sed 's/^ */display : /'

say "build"
cargo build -p beckon-macos --examples || exit 1
PROBE=target/debug/examples/tray_probe
PROBE_SETTINGS=target/debug/examples/settings_probe

# Spend the SIGKILL-on-first-exec on a run whose result nobody reads.
warm() {
  "$1" nsapp >/dev/null 2>&1 &
  local p=$!
  sleep 1
  kill "$p" 2>/dev/null
  wait "$p" 2>/dev/null
}
warm "$PROBE"
warm "$PROBE_SETTINGS"

# Capture the whole screen, then crop the menu bar strip if sips can. The
# full frame is the artifact; the crop is only for legibility.
capture() {
  local name="$1"
  screencapture -x "$OUT/$name-full.png"
  local rc=$?
  if [ $rc -ne 0 ]; then
    # Not fatal any more. The probes ask the window server directly, which
    # needs no grant; a screenshot is now corroboration, not the evidence.
    printf 'screencapture(%s): no image (Screen Recording not granted to this\n' "$name"
    printf '  terminal). The window server report below is the real answer.\n'
    return 1
  fi
  printf 'screencapture(%s) ok\n' "$name"
  local w
  w=$(sips -g pixelWidth "$OUT/$name-full.png" 2>/dev/null | awk '/pixelWidth/{print $2}')
  if [ -n "$w" ]; then
    sips -c 120 "$w" --cropOffset 0 0 "$OUT/$name-full.png" --out "$OUT/$name-bar.png" >/dev/null 2>&1 \
      && printf '  bar strip: %s-bar.png\n' "$name"
  fi
  return 0
}

say "control: menu bar with no probe running"
capture baseline

# How long each probe stays up. Long enough for a person to LOOK at the menu
# bar, which is the only instrument that settles the tray question: the
# window-server report cannot distinguish "the item is not on screen" from
# "NSStatusItem produces no window this API enumerates", and a screenshot
# needs a grant that has nothing to do with the question.
HOLD="${HOLD:-12}"

run_probe() {  # $1 = binary, $2 = arg or "", $3 = capture name
  local bin="$1" arg="$2" name="$3" try
  for try in 1 2; do
    if [ -n "$arg" ]; then "$bin" "$arg" >"$OUT/stdout-$name.txt" 2>&1 &
    else "$bin" >"$OUT/stdout-$name.txt" 2>&1 & fi
    local pid=$!
    sleep 2
    if kill -0 "$pid" 2>/dev/null; then
      # The report has printed by now; show it BEFORE the hold, so the
      # instruction to look at the menu bar is on screen while there is
      # still something to look at.
      cat "$OUT/stdout-$name.txt"
      printf '\n... holding %ss. LOOK AT THE MENU BAR.\n' "$HOLD"
      sleep "$HOLD"
      capture "$name"
      kill "$pid" 2>/dev/null
      wait "$pid" 2>/dev/null
      return 0
    fi
    wait "$pid" 2>/dev/null
    printf 'probe died before the capture (attempt %s); stdout was:\n' "$try"
    cat "$OUT/stdout-$name.txt"
    # An empty stdout here means it never reached its first println -- the
    # SIGKILL, not a beckon failure. A non-empty one is a real refusal and
    # says so in words.
  done
  printf 'GIVING UP on %s after two attempts.\n' "$name"
  return 1
}

for mode in carbon nsapp; do
  say "mode: $mode"
  run_probe "$PROBE" "$mode" "$mode"
done

say "settings window"
run_probe "$PROBE_SETTINGS" "" settings

say "results"
ls -la "$OUT"/*.png 2>/dev/null || printf '(no screenshots -- not needed, see the reports above)\n'
cat <<'EOF'

READ THE "window server report" BLOCKS ABOVE. They are the measurement.
Each carries its own control, so a blind probe says so in words instead of
looking like a clean negative.

  VERDICT line in BOTH carbon and nsapp says the status item has a window
      -> keep RunApplicationEventLoop; nothing about hotkeys changes.
  only nsapp says it
      -> the run loop must change, and RegisterEventHotKey must then be
         re-measured under [NSApp run] before that swap is trusted.
  neither says it, but both report menu-bar windows for other processes
      -> a single-process status item is not possible; see the spec's
         rejected two-process alternative.
  INCONCLUSIVE
      -> the server could not see that layer at all; nothing was measured.

Each tray probe now also opens a plain titled window, "ENUMERATION CONTROL".
That window is the control for the report: if it is listed and no status-item
window is, the report has told you all it can, and the remaining question --
is the item on screen, or does NSStatusItem simply not produce an enumerable
window here? -- is settled by looking, not by this script.

So while each probe holds: LOOK AT THE MENU BAR. Then click the item if it is
there; stdout must print "menu click". A drawn-but-dead menu looks identical
to a working one until something is clicked.

settings-full.png is the settings window. Nothing it does touches a config
file -- Save prints the TOML it would have written. Worth doing by hand,
because no screenshot and no unit test can reach them:

  * type a filter, THEN click a row -- stdout must print the model index,
    which is only distinguishable from the view index once a filter is on.
  * type a whole app name into the App field. The model must receive the
    whole name, not its last character. That is the Windows data-loss bug
    this window claims to be structurally immune to, and this is the only
    place the claim gets tested.
  * change a modifier box: "probe" must print BEFORE "edit".
EOF
