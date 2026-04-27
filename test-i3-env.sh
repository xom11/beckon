#!/usr/bin/env bash
# Bring up a nested X11 + i3 environment on DISPLAY=:2 for testing
# the i3 / X11 backends without disrupting sway.
#
# Architecture:
#   sway (Wayland host)
#   └── Xwayland :3   ← standalone X server (fed by Wayland)
#       └── Xephyr :2 ← nested X server (real root window for i3)
#           └── i3    ← actual WM under test
#
# Usage:
#   ./test-i3-env.sh start   # start everything, leaves running
#   ./test-i3-env.sh stop    # tear down
#   ./test-i3-env.sh test    # run beckon test commands inside :2
#   ./test-i3-env.sh xterm   # spawn an xterm in :2
#
# After `start`, run beckon manually with the right env:
#   env -u SWAYSOCK -u WAYLAND_DISPLAY DISPLAY=:2 ./target/release/beckon -d

set -e

XEPHYR=/nix/store/91cyn31jfn9kv5gb57mwi7ld5bmxmdxq-xorg-server-21.1.22/bin/Xephyr
XTERM=/nix/store/czdyqd1dixafblmqwf2a896a5dp04bfx-xterm-407/bin/xterm
I3CONF=/tmp/i3-test.conf

case "${1:-}" in
start)
    # 1. Standalone Xwayland on :3 — gives Xephyr a host display.
    if ! [ -e /tmp/.X11-unix/X3 ]; then
        setsid Xwayland :3 -rootless > /tmp/xwayland.log 2>&1 < /dev/null &
        sleep 1.5
    fi

    # 2. Xephyr nested X server on :2 — real root window so i3 can manage.
    if ! [ -e /tmp/.X11-unix/X2 ]; then
        DISPLAY=:3 setsid "$XEPHYR" -screen 1280x800 -resizeable :2 \
            > /tmp/xephyr.log 2>&1 < /dev/null &
        sleep 1.5
    fi

    # 3. Minimal i3 config (font is required, scratchpad supported by default).
    cat > "$I3CONF" <<'EOF'
font pango:monospace 10
floating_modifier Mod1
bindsym Mod1+Return exec xterm
bindsym Mod1+q kill
EOF

    # 4. i3 inside Xephyr. Must isolate from host sway sockets.
    if ! pgrep -af "i3 -c $I3CONF" >/dev/null; then
        env -u SWAYSOCK -u I3SOCK -u WAYLAND_DISPLAY DISPLAY=:2 \
            setsid /usr/bin/i3 -c "$I3CONF" > /tmp/i3.log 2>&1 < /dev/null &
        sleep 1.5
    fi

    echo "Up: Xwayland :3, Xephyr :2, i3"
    echo "Run: env -u SWAYSOCK -u WAYLAND_DISPLAY DISPLAY=:2 ./target/release/beckon ..."
    ;;
stop)
    pkill -f "i3 -c $I3CONF" 2>/dev/null || true
    pkill -f "Xephyr.*:2" 2>/dev/null || true
    pkill -f "Xwayland :3" 2>/dev/null || true
    rm -f "$I3CONF"
    echo "Down."
    ;;
xterm)
    env -u SWAYSOCK -u WAYLAND_DISPLAY DISPLAY=:2 \
        setsid "$XTERM" > /dev/null 2>&1 < /dev/null &
    echo "xterm spawned in :2"
    ;;
*)
    echo "usage: $0 {start|stop|xterm}" >&2
    exit 1
    ;;
esac
