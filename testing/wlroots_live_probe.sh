#!/usr/bin/env bash
#
# Live probe for the wlroots backend (crates/beckon-linux/src/wlroots.rs).
#
#   ./testing/wlroots_live_probe.sh ./target/debug/beckon
#
# Runs inside a NESTED HEADLESS labwc, so it is safe on a machine somebody is
# using -- unlike linux_live_test.py, which kills every GUI app it knows.
#
# Requirements: `labwc` and `wlrctl` on PATH, and an outer Wayland session to
# nest inside (any compositor: the nested labwc is what gets tested).
#
#   nix shell nixpkgs#wlrctl -c ./testing/wlroots_live_probe.sh ./target/debug/beckon
#
# THE ORACLE IS THE TRAP. There is no `swaymsg` or `hyprctl` for labwc -- that
# is why this backend exists -- so the oracle is a different zwlr client, and
# the wrong one is silently blind:
#
#   lswt 2.0.0   speaks ext_foreign_toplevel_list_v1, which carries NO state.
#                Every window reports activated=false, minimized=false in
#                every state, so a WORKING backend fails five of seven checks
#                and each failure reads like beckon not focusing anything.
#   wlrctl 0.2.2 speaks zwlr_foreign_toplevel_manager_v1, matches
#                `state:active` / `state:minimized`, and can focus and
#                minimize a window itself -- so it also sets the
#                preconditions, and the suite never asserts beckon against
#                beckon.
#
# `--control` runs the oracle-sight check on its own: focus A, ask, focus B,
# ask, minimize, ask. Run it whenever a result here surprises you.
set -u

BECKON="${1:-beckon}"
CONTROL_ONLY=0
[ "${1:-}" = "--control" ] && { CONTROL_ONLY=1; BECKON="${2:-beckon}"; }

RUNTIME="${XDG_RUNTIME_DIR:?XDG_RUNTIME_DIR must be set}"
# An EMPTY config dir, and that is load-bearing: without it labwc runs the real
# session's autostart inside the nested instance, starting a second copy of the
# user's bar and IME -- and tearing the nested session down then took the outer
# session's IME with it (measured, rog, 2026-09-07).
CFG=$(mktemp -d /tmp/beckon-wlr-cfg.XXXXXX)

pass=0; fail=0
ok()  { echo "  PASS  $1"; pass=$((pass+1)); }
bad() { echo "  FAIL  $1"; echo "        $2"; fail=$((fail+1)); }

for tool in labwc wlrctl foot; do
  command -v "$tool" >/dev/null || { echo "$tool not on PATH"; exit 2; }
done

before=$(ls "$RUNTIME" | grep -E '^wayland-[0-9]+$' | sort)
env -u WAYLAND_DISPLAY -u DISPLAY XDG_RUNTIME_DIR="$RUNTIME" \
    XDG_CONFIG_HOME="$CFG" WLR_BACKENDS=headless \
    setsid -f labwc >"$CFG/labwc.log" 2>&1
sleep 3
after=$(ls "$RUNTIME" | grep -E '^wayland-[0-9]+$' | sort)
NEW=$(comm -13 <(echo "$before") <(echo "$after") | head -1)
[ -z "$NEW" ] && { echo "no nested socket appeared; log:"; tail -20 "$CFG/labwc.log"; exit 1; }
export XDG_RUNTIME_DIR="$RUNTIME" WAYLAND_DISPLAY="$NEW"; unset DISPLAY

# Identify OUR labwc by the config dir we handed it. `pgrep labwc` also matches
# the real session and anyone else's nested instance on a shared machine.
NESTED=""
for p in $(pgrep labwc); do
  if tr '\0' '\n' < "/proc/$p/environ" 2>/dev/null | grep -qx "XDG_CONFIG_HOME=$CFG"; then NESTED=$p; fi
done
echo "nested labwc on $WAYLAND_DISPLAY pid=${NESTED:-?}"

PIDS=""
cleanup() {
  for p in $PIDS; do kill "$p" 2>/dev/null; done
  [ -n "$NESTED" ] && kill "$NESTED" 2>/dev/null
  rm -rf "$CFG"
}
trap cleanup EXIT

spawn()     { foot --app-id="$1" --title="$2" -- sleep 3600 >/dev/null 2>&1 & PIDS="$PIDS $!"; }
active()    { wlrctl toplevel find "$@" state:active    >/dev/null 2>&1; }
minimized() { wlrctl toplevel find "$@" state:minimized >/dev/null 2>&1; }
exists()    { wlrctl toplevel find "$@"                 >/dev/null 2>&1; }
ntop()      { wlrctl toplevel list 2>/dev/null | wc -l; }
waitfor()   { local n=$1; shift; for _ in $(seq 1 $((n*4))); do eval "$@" && return 0; sleep 0.25; done; return 1; }

# ---- the control: is the oracle sighted at all? -------------------------
spawn ctlA ctlA; sleep 2; spawn ctlB ctlB
waitfor 20 'exists app_id:ctlB' || { echo "control setup failed"; exit 1; }
sight=0
wlrctl toplevel focus app_id:ctlA; sleep 1
active app_id:ctlA && ! active app_id:ctlB && sight=$((sight+1))
wlrctl toplevel focus app_id:ctlB; sleep 1
active app_id:ctlB && ! active app_id:ctlA && sight=$((sight+1))
wlrctl toplevel minimize app_id:ctlA; sleep 1
minimized app_id:ctlA && sight=$((sight+1))
if [ "$sight" = "3" ]; then
  ok "CONTROL: the oracle can see focus and minimize (3/3)"
else
  bad "CONTROL: the oracle is blind ($sight/3)" \
      "every result below would be meaningless. Is this wlrctl, or lswt?"
  echo; echo "=== $pass passed, $fail failed ==="; exit 1
fi
for p in $PIDS; do kill "$p" 2>/dev/null; done; PIDS=""; sleep 1.5
[ "$CONTROL_ONLY" = "1" ] && { echo; echo "=== $pass passed, $fail failed ==="; exit 0; }

# ---- 0. an empty session ------------------------------------------------
[ "$(ntop)" = "0" ] && ok "nested session starts empty" \
                    || bad "nested session starts empty" "$(wlrctl toplevel list)"

# ---- 1. step 3: launch --------------------------------------------------
"$BECKON" foot >"$CFG/b1" 2>&1; rc=$?
if waitfor 15 'exists app_id:foot'; then ok "step 3 launch: \`beckon foot\` opened a window (rc=$rc)"
else bad "step 3 launch" "rc=$rc $(cat "$CFG/b1")"; fi
PIDS="$PIDS $(pgrep -n foot)"

# ---- 2. step 4: focus an unfocused running app --------------------------
spawn other other
waitfor 15 'exists app_id:other' || bad setup "'other' never appeared"
wlrctl toplevel focus app_id:other; sleep 0.6
active app_id:other || bad setup "precondition: 'other' is not focused"
"$BECKON" foot >"$CFG/b2" 2>&1; rc=$?
if waitfor 6 'active app_id:foot'; then ok "step 4 focus: an unfocused running app is focused (rc=$rc)"
else bad "step 4 focus" "rc=$rc list=$(wlrctl toplevel list) $(cat "$CFG/b2")"; fi

# ---- 3. step 5a: cycle over three windows -------------------------------
# The property under test is the one `algorithm::decide` guarantees for a
# backend with no focus history: the ring is ordered by address, so a lap
# reaches every window rather than ping-ponging between two.
spawn cyc cycOne; sleep 1; spawn cyc cycTwo; sleep 1; spawn cyc cycThree
waitfor 20 '[ "$(wlrctl toplevel list | grep -c "^cyc:")" = 3 ]' || bad setup "3 cyc windows never appeared"
wlrctl toplevel focus app_id:other; sleep 0.6
seen=""
for _ in 1 2 3 4 5 6 7; do
  "$BECKON" cyc >/dev/null 2>&1; sleep 0.5
  for t in cycOne cycTwo cycThree; do
    if active app_id:cyc title:"$t"; then
      case " $seen " in *" $t "*) :;; *) seen="$seen $t";; esac
    fi
  done
done
n=$(echo $seen | wc -w)
if [ "$n" -ge 3 ]; then ok "step 5a cycle: seven presses visited all three windows ($seen)"
else bad "step 5a cycle" "reached only $n distinct windows:$seen"; fi

# ---- 4. step 5b: toggle back --------------------------------------------
wlrctl toplevel focus app_id:other; sleep 0.6
"$BECKON" other >"$CFG/b4" 2>&1; rc=$?; sleep 0.6
if ! active app_id:other; then ok "step 5b toggle-back: pressing a focused lone window left it (rc=$rc)"
else bad "step 5b toggle-back" "rc=$rc still on 'other'; $(cat "$CFG/b4")"; fi

# ---- 5. step 5c: hide, and come back ------------------------------------
for p in $PIDS; do kill "$p" 2>/dev/null; done; PIDS=""; sleep 1.5
spawn alone alone
waitfor 15 'exists app_id:alone' || bad setup "'alone' never appeared"
left=$(ntop)
if [ "$left" != "1" ]; then
  echo "  SKIP  step 5c: $left toplevels left, decide() would toggle back, not hide"
else
  wlrctl toplevel focus app_id:alone; sleep 0.6
  "$BECKON" alone >"$CFG/b5" 2>&1; rc=$?; sleep 0.8
  if minimized app_id:alone; then ok "step 5c hide: the lone window minimized (rc=$rc)"
  else bad "step 5c hide" "rc=$rc $(cat "$CFG/b5"); list=$(wlrctl toplevel list)"; fi
  # And the next press must bring it back, or step 5c strands the window
  # somewhere only beckon can reach.
  "$BECKON" alone >"$CFG/b6" 2>&1; rc=$?; sleep 0.8
  if active app_id:alone && ! minimized app_id:alone; then
    ok "step 5c restore: the next press un-minimized and focused it (rc=$rc)"
  else bad "step 5c restore" "rc=$rc $(cat "$CFG/b6")"; fi
fi

echo; echo "=== $pass passed, $fail failed ==="
[ "$fail" = "0" ]
