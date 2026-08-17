#!/usr/bin/env bash
# Hai `serve`, hai CGEventTap tren Caps -- co hop le, va co pha alias khong?
#
# `lockfile::acquire` bam duong dan CONFIG vao ten khoa, nen mot khoa moi
# config. Hai config khac nhau => hai serve cung song => hai tap cung nghe
# Caps. Tap thu hai khong biet gi ve co INJECTING cua tap thu nhat, nen chord
# tap 1 bom ra la DAU VAO doi voi tap 2 -- va tap 2 cung tin Caps dang giu,
# vi no thay cung nhung su kien Caps do.
#
# Doi chung chay TRUOC va sau, cung phep do, chi khac so tap. Khong co no thi
# "hong khi co hai tap" khong phan biet duoc voi "hong luc do trong ngay".
set -uo pipefail
B="$HOME/beckon-test/beckon"; P="$HOME/beckon-test/caps_synth_probe"
PAT="beckon-test/beckon serve"
C1="$HOME/beckon-test/caps.toml"; C2="$HOME/beckon-test/caps2.toml"

frontmost() { osascript -e 'tell application "System Events" to name of first application process whose frontmost is true' 2>/dev/null; }
start() { nohup sudo -n launchctl asuser "$(id -u)" sudo -n -u "$USER" "$B" serve "$1" >>"$2" 2>&1 & sleep 5; }
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

run() {
  label="$1"; shift
  stop
  : >/tmp/tt.log
  for c in "$@"; do start "$c" /tmp/tt.log; done
  taps=$(grep -c "caps event tap active" /tmp/tt.log)
  live=$(pgrep -f "$PAT" | grep -c . || true)
  if [ "$taps" != "$#" ]; then
    echo "  $label: TIEN DE HONG -- muon $# tap, thay $taps"; sed 's/^/    /' /tmp/tt.log; return
  fi
  echo "  $label ($taps tap active): $(score)"
}

cat > "$C2" <<'TOML'
# Config thu hai, chi de dung MOT serve thu hai co tap Caps rieng.
# Phim khac han caps.toml, nen khong tranh chord voi no.
"ctrl+super+alt+w" = "Safari"

[keyboard]
caps = true
caps_hold = "ctrl+super+alt"
caps_tap = "capslock"
TOML

echo '=== DOI CHUNG TRUOC: mot tap ==='
run 'mot tap ' "$C1"
echo '=== THU NGHIEM: hai tap ==='
run 'hai tap' "$C1" "$C2"
echo '=== DOI CHUNG SAU: mot tap lai ==='
run 'mot tap ' "$C1"

stop
rm -f "$C2"
hs -c "hs.hid.capslock.set(false)" >/dev/null 2>&1
launchctl bootstrap "gui/$(id -u)" "$HOME/Library/LaunchAgents/com.xom11.beckon-serve.plist" 2>/dev/null
sleep 2
pgrep -f 'nix/store.*beckon serve' >/dev/null && echo 'agent that da chay lai' || echo 'CANH BAO: agent that chua chay lai'
