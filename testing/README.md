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
| `WAYLAND_DISPLAY` (KDE) | KWin scripting over `busctl` | nothing to install; KWin ships the engine |
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

**Restart it with a script, not an inline command.** Repeated restarts leave
stale `dbus-daemon`s and a stale `at-spi2-registryd` behind, and once they
accumulate the next gnome-shell comes up owning `org.gnome.Shell` but with
its JS side wedged: Mutter's D-Bus interfaces answer, `org.gnome.Shell.Extensions`
and the beckon object time out, and the process sits in `poll` at 0% CPU.
Killing the leftovers first fixes it:

```sh
pkill -9 -x gnome-shell
pkill -9 -f 'dbus-daemon --config-file=/tmp/beckon-gnome'
pkill -9 -f at-spi2-registryd
pkill -9 -f at-spi-bus-launcher
```

Put that in a file and run `bash the-file.sh`. Typed inline, `pkill -f` matches
the very shell that is running it and kills your own session before it gets to
the restart — which looks exactly like the VM dropping the connection.

Two more things to know about this session:

- Nothing has focus until something is activated — a headless GNOME has no
  seat, so a freshly mapped window is listed with `focused = false`. The
  suite establishes focus through the extension's `ActivateWindow` rather
  than assuming it.
- **Do not kill Xwayland.** gnome-shell 50 dies with it
  (`Gjs-CRITICAL: JS callback during garbage collection`). `Env.extra_kill`
  is per-environment for exactly this reason.

### KDE Wayland (KWin 6)

Only `kwin-wayland` is needed — not a full Plasma session — because beckon
talks to KWin's scripting engine, not to plasmashell.

```sh
sudo apt-get install -y --no-install-recommends kwin-wayland kwin-common
```

Same isolated-bus config as the GNOME rig (reuse `bus.conf`, pointing
`<servicedir>` at an empty directory), then:

```sh
export XDG_RUNTIME_DIR=/run/user/$(id -u)
export XDG_SESSION_TYPE=wayland XDG_CURRENT_DESKTOP=KDE
export KWIN_COMPOSE=Q          # QPainter compositor — see below
unset WAYLAND_DISPLAY SWAYSOCK I3SOCK DISPLAY
ADDR=$(dbus-daemon --config-file=/tmp/beckon-kde/bus.conf --print-address --fork)
DBUS_SESSION_BUS_ADDRESS="$ADDR" \
  kwin_wayland --virtual --width 1920 --height 1080 --xwayland --socket wayland-kde &
```

`KWIN_COMPOSE=Q` is load-bearing on a VM with no `/dev/dri`: it selects the
QPainter compositor so KWin never asks for OpenGL. With it, KWin 6.6.6 comes
up headless on software rendering in about ten seconds. The startup warnings
about missing `org.kde.breeze` decorations and missing portals are cosmetic —
neither is installed, and neither is needed.

Then point the suite at that session with `WAYLAND_DISPLAY=wayland-kde` and
`XDG_CURRENT_DESKTOP=KDE`.

Two things worth knowing before debugging a KDE failure:

- KWin advertises **neither** `zwlr_foreign_toplevel_management_v1` nor
  `org_kde_plasma_window_management` (check with `wayland-info`). A
  Wayland-protocol client therefore cannot enumerate windows here, which is
  why the backend goes through `org.kde.kwin.Scripting` instead.
- A KWin script's `print()` goes to KWin's own stderr. When KWin is started
  as above that lands in the redirect target, which makes it the quickest way
  to see what a script actually did.

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
