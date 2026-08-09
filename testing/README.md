# Live backend tests

`linux_live_test.py` drives the real `beckon` binary against a real
compositor and asserts on what that compositor reports afterwards. The unit
tests cover `algorithm::decide` against synthetic window lists; this covers
everything they structurally cannot — `.desktop` resolution against the
machine's own metadata, the class string a toolkit actually advertises at
runtime, and whether a focus / minimize request is honoured at all.

```sh
cargo build --release
./testing/linux_live_test.py --beckon ./target/release/beckon
```

It picks its probe the same way `beckon_linux::pick_backend` picks its
backend, so run it inside the session you want to test:

| environment | probe | notes |
|---|---|---|
| `SWAYSOCK` / `I3SOCK` | `swaymsg` / `i3-msg` tree | covers sway *and* i3 |
| `WAYLAND_DISPLAY` (GNOME) | the beckon shell extension over `busctl` | extension must be installed and enabled |
| `DISPLAY` | `xprop` / `xdotool` (EWMH) | any EWMH window manager |

**It is destructive.** To build its preconditions it kills every GUI app it
knows how to start (`Suite.KILLABLE`, plus `Env.extra_kill`). Run it in a VM
or a nested compositor, never in your daily desktop.

Useful flags: `--multi` / `--other` pick the two apps (one must be able to
open several windows), `--only <substring>` runs a single test, `-v` echoes
every `beckon` invocation and its action. Every failure prints the window
list, the focused window and the MRU file, so a red line is actionable
without a second run.

## Bringing up test compositors in a headless VM

All four Linux backends were exercised this way on Ubuntu 26.04 arm64 under
Lima (`vmType: vz`), with no display attached.

### GNOME Wayland (GNOME Shell 50.1)

`gnome-shell --headless` works, but **not** under a bare `dbus-run-session`:
the shell's JS side hangs before it claims `org.gnome.Shell`, because
`xdg-desktop-portal` and `xdg-desktop-portal-gnome` activate each other in a
cycle and the GTK settings-portal probe blocks on it. Give the private bus a
service directory that is empty, so every unavailable name fails instantly
with `ServiceUnknown` instead of a 25 s timeout:

```sh
mkdir -p /tmp/beckon-gnome/services
cat > /tmp/beckon-gnome/bus.conf <<'XML'
<!DOCTYPE busconfig PUBLIC "-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN"
 "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd">
<busconfig>
  <type>session</type>
  <listen>unix:tmpdir=/tmp</listen>
  <servicedir>/tmp/beckon-gnome/services</servicedir>
  <policy context="default">
    <allow send_destination="*" eavesdrop="true"/>
    <allow eavesdrop="true"/>
    <allow own="*"/>
  </policy>
</busconfig>
XML

export XDG_RUNTIME_DIR=/run/user/$(id -u)
export XDG_SESSION_TYPE=wayland XDG_CURRENT_DESKTOP=GNOME
export NO_AT_BRIDGE=1 GTK_A11Y=none GTK_USE_PORTAL=0
unset WAYLAND_DISPLAY SWAYSOCK I3SOCK DISPLAY
ADDR=$(dbus-daemon --config-file=/tmp/beckon-gnome/bus.conf --print-address --fork)
DBUS_SESSION_BUS_ADDRESS="$ADDR" \
  gnome-shell --headless --virtual-monitor 1920x1080 --wayland-display wayland-gnome &
```

Install the extension **before** starting the shell — Wayland cannot reload
it live:

```sh
cd extensions
gnome-extensions pack --force beckon@xom11.github.io
gnome-extensions install --force beckon@xom11.github.io.shell-extension.zip
gsettings set org.gnome.shell enabled-extensions "['beckon@xom11.github.io']"
```

Then point the suite at that bus with `WAYLAND_DISPLAY=wayland-gnome`.

Two things to know about this session:

- Nothing has focus until something is activated — a headless GNOME has no
  seat, so a freshly mapped window is listed with `focused = false`. The
  suite establishes focus through the extension's `ActivateWindow` rather
  than assuming it.
- **Do not kill Xwayland.** gnome-shell 50 dies with it
  (`Gjs-CRITICAL: JS callback during garbage collection`). `Env.extra_kill`
  is per-environment for exactly this reason.

### sway

The existing headless sway session works as-is
(`sway -c` with `output HEADLESS-1`). Set `DISPLAY` to Xwayland's display so
X11 clients can be used as test apps.

GTK4 apps do **not** map a window on a headless sway output here — no
`GSK_RENDERER` (cairo / ngl / gl / vulkan) helps, and `swaymsg exec` shows
the same thing with beckon out of the picture. Use `foot` (Wayland) and
`xterm` (XWayland) instead; between them they cover both window-identity
paths.

### i3 and generic X11

```sh
Xvfb :5 -screen 0 1920x1080x24 &   # generic EWMH
Xvfb :6 -screen 0 1920x1080x24 &   # i3
DISPLAY=:5 openbox &
DISPLAY=:6 i3 -c ~/.config/i3/config &
```

For i3, export `I3SOCK=$(DISPLAY=:6 i3 --get-socketpath)`. `xterm` and
`uxterm` make a good app pair: their `.desktop` ids (`debian-xterm`,
`debian-uxterm`) differ from the `WM_CLASS` they advertise (`XTerm`,
`UXTerm`), which is precisely the mismatch beckon has to handle.
