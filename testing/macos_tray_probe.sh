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
# MEASURED 2026-08-13, and it decides how this script works: a status item
# that is plainly visible on the menu bar is NOT listed by
# CGWindowListCopyWindowInfo. The window-server report therefore cannot
# answer the tray question at all -- it can only confirm ordinary windows,
# which is why the probe opens one as an enumeration control. The instrument
# that works is a person looking at the menu bar, so this script asks and
# records the answer rather than inferring it.
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

run_probe() {  # $1 = binary, $2 = arg or "", $3 = name
  local bin="$1" arg="$2" name="$3" try
  for try in 1 2; do
    # Output goes to the terminal AND a file. Live is what matters: a menu
    # click prints when you click, and a file you read afterwards cannot
    # tell you whether the click or the drawing was the thing that worked.
    if [ -n "$arg" ]; then
      "$bin" "$arg" > >(tee "$OUT/stdout-$name.txt") 2>&1 &
    else
      "$bin" > >(tee "$OUT/stdout-$name.txt") 2>&1 &
    fi
    local pid=$!
    sleep 3
    if kill -0 "$pid" 2>/dev/null; then
      capture "$name"
      ask_about "$name"
      kill "$pid" 2>/dev/null
      wait "$pid" 2>/dev/null
      return 0
    fi
    wait "$pid" 2>/dev/null
    printf 'probe died before it could be observed (attempt %s). Output above.\n' "$try"
  done
  printf 'GIVING UP on %s after two attempts.\n' "$name"
  return 1
}

# Ask the operator what they can see, and RECORD it.
#
# This exists because the previous design asked someone to watch two modes
# run back to back and remember which was which. They could not, reasonably,
# and the answer decides whether `run_forever` changes -- so the script asks
# per mode instead of hoping.
#
# A person looking at the menu bar is not a fallback here, it is the only
# instrument that works: measured on macmini 2026-08-13, a visible status
# item is NOT listed by CGWindowListCopyWindowInfo, so the window-server
# report structurally cannot answer this question.
ANSWERS=""
ask_about() {
  local name="$1" a b
  if [ ! -t 0 ]; then
    printf '\n(no terminal on stdin -- holding %ss instead of asking)\n' "$HOLD"
    sleep "$HOLD"
    ANSWERS="$ANSWERS\n  $name: not asked (non-interactive)"
    return
  fi
  printf '\n'
  printf '  >>> Is there an item reading "beckon" in the menu bar RIGHT NOW?\n'
  printf '      (a window titled ENUMERATION CONTROL should also be visible;\n'
  printf '       if you see neither, say n)\n'
  read -r -p "      [y/n] " a
  if [ "$a" = y ] || [ "$a" = Y ]; then
    printf '  >>> Now click it and choose "PROBE - click me".\n'
    printf '      Does a line "menu click: id=2" appear above?\n'
    read -r -p "      [y/n] " b
  else
    b="-"
  fi
  ANSWERS="$ANSWERS\n  $name: visible=$a  click_dispatched=$b"
}

say "what you reported"
printf '%b\n' "$ANSWERS"

say "results"
ls -la "$OUT"/*.png 2>/dev/null || printf '(no screenshots -- not needed, see the reports above)\n'
cat <<'EOF'

THE ANSWERS YOU GAVE ARE THE MEASUREMENT -- see the block above.

  visible in BOTH carbon and nsapp
      -> keep RunApplicationEventLoop; nothing about hotkeys changes.
  visible in nsapp ONLY
      -> run_forever must change, and RegisterEventHotKey must then be
         re-measured under [NSApp run] before that swap is trusted.
  visible in carbon ONLY
      -> keep the loop, and record why [NSApp run] fails so nobody swaps
         to it later.
  visible in NEITHER
      -> a single-process status item is not possible; see the spec's
         rejected two-process alternative.

The "window server report" blocks are NOT the answer for the tray -- a
visible status item does not appear in them. They are there for the
enumeration control window and for the settings window, both of which are
ordinary windows and do appear.

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
