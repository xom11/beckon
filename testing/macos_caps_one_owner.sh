#!/usr/bin/env bash
# One CGEventTap per machine: does the SECOND serve decline, and say so?
#
# This is `macos_two_caps_taps.sh` (which measured the defect) turned into the
# check for the fix, and the middle arm means the OPPOSITE thing now.
#
#   before the fix    two serves, two taps, both logging success, Caps dead
#   after the fix     two serves, ONE tap, one refusal ON STDERR, Caps alive
#
# So this script does NOT merely relax the tap count. It asserts the refusal
# line, because silence was the whole defect: a user with two serves running
# saw Caps stop working and had nothing to read. A version of this test that
# only counted taps would pass against a beckon that declined quietly, which
# is the bug one level down.
#
# The controls run either side of the experiment, same measurement, only the
# number of serves differing -- without them "broken with two" cannot be told
# from "broken at that moment".
set -uo pipefail
B="$HOME/beckon-test/beckon"
P="$HOME/beckon-test/caps_synth_probe"
PAT="beckon-test/beckon serve"
C1="$HOME/beckon-test/caps.toml"
C2="$HOME/beckon-test/caps2.toml"
LOG=/tmp/one_owner.log

frontmost() { osascript -e 'tell application "System Events" to name of first application process whose frontmost is true' 2>/dev/null; }
start() { nohup sudo -n launchctl asuser "$(id -u)" sudo -n -u "$USER" "$B" serve "$1" >>"$LOG" 2>&1 & sleep 5; }
stop() { pkill -f "$PAT" 2>/dev/null; sleep 2; }

score() {
  ok=0; s=""
  for _ in $(seq 1 8); do
    osascript -e 'tell application "kitty" to activate' 2>/dev/null; sleep 1
    sudo -n launchctl asuser "$(id -u)" sudo -n -u "$USER" "$P" 12 250 >/dev/null 2>&1
    sleep 2
    if [ "$(frontmost)" = "Finder" ]; then ok=$((ok+1)); s="$s+"; else s="$s."; fi
  done
  echo "$ok/8  [$s]"
}

# want_taps / want_refusals are the whole point: each arm states what it
# expects to SEE before it measures anything, so a premise that did not hold
# is reported as a broken premise rather than as a result.
run() {
  label="$1"; want_taps="$2"; want_ref="$3"; shift 3
  stop
  : >"$LOG"
  for c in "$@"; do start "$c" ; done
  taps=$(grep -c "caps event tap active" "$LOG")
  refs=$(grep -c "another beckon owns Caps" "$LOG")
  # Count the REAL processes, not the wrappers. `start` goes through
  # `sudo launchctl asuser ... sudo -u ...`, so `pgrep -f` matches three
  # lines per serve and every premise check fails at three-times the truth.
  # `comm` is `beckon` only for the process that actually runs it.
  live=$(pgrep -f "$PAT" | while read -r pid; do ps -o comm= -p "$pid"; done \
         | grep -c '/beckon$' || true)
  if [ "$live" != "$#" ]; then
    echo "  $label: PREMISE BROKEN -- wanted $# serves, $live alive"; return
  fi
  if [ "$taps" != "$want_taps" ] || [ "$refs" != "$want_ref" ]; then
    echo "  $label: PREMISE BROKEN -- wanted ${want_taps} tap / ${want_ref} refusal, saw ${taps} / ${refs}"
    sed 's/^/    /' "$LOG"; return
  fi
  echo "  $label ($taps tap, $refs refusal): $(score)"
}

cat > "$C2" <<'TOML'
# A second config, only so a second serve exists that wants Caps too.
# Different key from caps.toml, so the two cannot contend for a chord --
# the tap is what is under test, not hotkey arbitration.
"ctrl+super+alt+w" = "Safari"

[keyboard]
caps = true
caps_hold = "ctrl+super+alt"
caps_tap = "capslock"
TOML

echo '=== CONTROL BEFORE: one serve, one tap ==='
run 'one serve ' 1 0 "$C1"
echo '=== EXPERIMENT: two serves -- one tap, one refusal ==='
run 'two serves' 1 1 "$C1" "$C2"
echo '=== CONTROL AFTER: one serve again ==='
run 'one serve ' 1 0 "$C1"

stop
rm -f "$C2"
hs -c "hs.hid.capslock.set(false)" >/dev/null 2>&1
launchctl bootstrap "gui/$(id -u)" "$HOME/Library/LaunchAgents/com.xom11.beckon-serve.plist" 2>/dev/null
sleep 2
pgrep -f 'nix/store.*beckon serve' >/dev/null && echo 'real agent is back' || echo 'WARNING: real agent did not come back'
