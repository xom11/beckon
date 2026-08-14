#!/usr/bin/env python3
"""Live end-to-end tests for beckon's Linux backends.

Unlike the unit tests (which exercise `algorithm::decide` against synthetic
window lists), this drives the *real* binary against a *real* compositor and
asserts on what the compositor reports afterwards. It is the only way to catch
the parts that unit tests structurally cannot reach: `.desktop` resolution
against the machine's own metadata, the class string a toolkit actually
advertises at runtime, and whether a focus/minimize request is honoured.

Run it inside the session you want to test — it picks its probe the same way
`beckon_linux::pick_backend` picks its backend:

    SWAYSOCK / I3SOCK               -> swaymsg / i3-msg tree probe
    HYPRLAND_INSTANCE_SIGNATURE     -> hyprctl probe
    WAYLAND_DISPLAY  (GNOME)        -> the beckon shell extension over D-Bus
    WAYLAND_DISPLAY  (KDE)          -> KWin scripting over D-Bus
    DISPLAY                         -> xprop / EWMH probe

    ./testing/linux_live_test.py --beckon ./target/release/beckon

Apps: the suite needs one app that can open several windows (`--multi`) and one
other app to toggle back to (`--other`). Defaults are picked per environment;
override them when testing a different toolkit.

DESTRUCTIVE: to build its preconditions the suite kills every GUI app it knows
how to start (see `Suite.KILLABLE`, which includes Xwayland). Run it in a
disposable session — a VM or a nested compositor — not in your daily desktop.
"""

from __future__ import annotations

import argparse
import json
import os
import re
import shutil
import subprocess
import sys
import time
from dataclasses import dataclass, field

# --------------------------------------------------------------------------
# tiny test harness
# --------------------------------------------------------------------------

RESET, RED, GREEN, YELLOW, DIM = "\033[0m", "\033[31m", "\033[32m", "\033[33m", "\033[2m"


@dataclass
class Report:
    passed: list[str] = field(default_factory=list)
    failed: list[tuple[str, str]] = field(default_factory=list)
    skipped: list[tuple[str, str]] = field(default_factory=list)

    def ok(self, name: str) -> None:
        self.passed.append(name)
        print(f"  {GREEN}PASS{RESET} {name}")

    def fail(self, name: str, why: str) -> None:
        self.failed.append((name, why))
        print(f"  {RED}FAIL{RESET} {name}\n       {why}")

    def skip(self, name: str, why: str) -> None:
        self.skipped.append((name, why))
        print(f"  {YELLOW}SKIP{RESET} {name} {DIM}({why}){RESET}")


class Skip(Exception):
    """Raised by a test when its precondition can't be met in this session."""


def run(cmd: list[str], env: dict | None = None, timeout: int = 20) -> subprocess.CompletedProcess:
    return subprocess.run(
        cmd, capture_output=True, text=True, timeout=timeout, env=env or os.environ.copy()
    )


def wait_for(predicate, timeout: float = 10.0, interval: float = 0.25):
    """Poll `predicate` until it returns something truthy. Returns it, or None."""
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        value = predicate()
        if value:
            return value
        time.sleep(interval)
    return None


# --------------------------------------------------------------------------
# environment probes — one per compositor family
# --------------------------------------------------------------------------


@dataclass
class Win:
    wid: str
    cls: str
    title: str = ""


class Env:
    """A compositor we can ask 'which windows exist and which one has focus'."""

    name = "?"
    default_multi = ""
    default_other = ""
    # Extra processes this environment allows the suite to kill on top of the
    # test apps. Deliberately per-environment: sway respawns Xwayland on
    # demand, whereas killing GNOME's Xwayland takes gnome-shell 50 down with
    # it (observed: Gjs-CRITICAL "JS callback during garbage collection",
    # then the session dies).
    extra_kill: tuple[str, ...] = ()

    def windows(self) -> list[Win]:
        raise NotImplementedError

    def focused(self) -> Win | None:
        raise NotImplementedError

    def activate(self, wid: str) -> None:
        """Focus a window *without* going through beckon.

        Preconditions have to be established by the compositor itself,
        otherwise the suite would be asserting beckon against beckon.
        """
        raise NotImplementedError

    def spawn(self, argv: list[str]) -> None:
        subprocess.Popen(
            ["setsid", "-f", *argv],
            stdin=subprocess.DEVNULL,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )

    # A window counts as "hidden" when the compositor no longer lists it as a
    # normal window, or lists it but not focused. Each backend refines this.
    def is_hidden(self, wid: str) -> bool:
        return all(w.wid != wid for w in self.windows())

    def of_class(self, cls: str) -> list[Win]:
        return [w for w in self.windows() if w.cls == cls]


class SwayEnv(Env):
    name = "sway/i3 (i3-IPC)"
    default_multi = "foot"
    # xterm rather than a GTK4 app on purpose: it exercises the
    # XWayland/`WM_CLASS` identity path (`debian-xterm.desktop` ⇒ class
    # `XTerm`), and GTK4 apps do not map a window at all under a headless
    # sway output — no GSK renderer (cairo/ngl/gl/vulkan) helps.
    default_other = "xterm"
    # A stray "Xwayland on :N" toplevel would turn step 5c's hide into a
    # (correct, but untested) toggle-back. sway starts it again on demand.
    extra_kill = ("Xwayland",)

    def __init__(self) -> None:
        self.msg = "swaymsg" if os.environ.get("SWAYSOCK") else "i3-msg"
        if not shutil.which(self.msg):
            raise RuntimeError(f"{self.msg} not on PATH")

    def spawn(self, argv: list[str]) -> None:
        # Go through the compositor rather than fork from here: sway hands
        # its children the right DISPLAY/WAYLAND_DISPLAY, which matters
        # after Xwayland has been restarted mid-run.
        run([self.msg, "exec", " ".join(argv)])

    def _tree(self) -> dict:
        out = run([self.msg, "-t", "get_tree"]).stdout
        return json.loads(out)

    def _walk(self, node: dict, acc: list[tuple[dict, bool]]) -> None:
        props = node.get("window_properties") or {}
        cls = node.get("app_id") or props.get("class")
        if cls:
            acc.append((node, node.get("focused", False)))
        for child in node.get("nodes", []) + node.get("floating_nodes", []):
            self._walk(child, acc)

    def windows(self) -> list[Win]:
        acc: list[tuple[dict, bool]] = []
        self._walk(self._tree(), acc)
        out = []
        for node, _ in acc:
            props = node.get("window_properties") or {}
            cls = node.get("app_id") or props.get("class") or ""
            out.append(Win(str(node["id"]), cls, node.get("name") or ""))
        return out

    def focused(self) -> Win | None:
        acc: list[tuple[dict, bool]] = []
        self._walk(self._tree(), acc)
        for node, focused in acc:
            if focused:
                props = node.get("window_properties") or {}
                cls = node.get("app_id") or props.get("class") or ""
                return Win(str(node["id"]), cls, node.get("name") or "")
        return None

    def activate(self, wid: str) -> None:
        run([self.msg, f"[con_id={wid}] focus"])

    def is_hidden(self, wid: str) -> bool:
        # sway hide == move to the `special:beckon`-style scratchpad; the node
        # still exists in the tree but hangs off __i3_scratch.
        tree = self._tree()

        def in_scratch(node: dict, inside: bool) -> bool:
            inside = inside or node.get("name") == "__i3_scratch"
            if str(node.get("id")) == wid:
                return inside
            for child in node.get("nodes", []) + node.get("floating_nodes", []):
                if in_scratch(child, inside):
                    return True
            return False

        if in_scratch(tree, False):
            return True
        return all(w.wid != wid for w in self.windows())


class HyprEnv(Env):
    """Hyprland, probed through `hyprctl`.

    Deliberately a different client from the backend under test: beckon opens
    `$XDG_RUNTIME_DIR/hypr/<sig>/.socket.sock` itself, this shells out to
    `hyprctl`. Same compositor, independent path — so a bug in beckon's own
    socket plumbing cannot make the oracle agree with it.
    """

    name = "Hyprland (hyprctl)"
    default_multi = "foot"
    # The same pair as sway, for the same reason: `foot` is Wayland-native
    # (class = app_id = `foot`) while `xterm` arrives through XWayland, where
    # the class is its `WM_CLASS` (`XTerm`) rather than the `.desktop` stem
    # (`debian-xterm`) — the identity mismatch beckon has to survive.
    default_other = "xterm"
    # `extra_kill` is deliberately left at the default. sway kills Xwayland
    # because a stray "Xwayland on :N" toplevel turns step 5c's hide into a
    # toggle-back, and sway starts it again on demand; neither half of that
    # has been measured on Hyprland. An unexpected extra window surfaces as
    # the skip in `_hide_alone`, which names every class it found, so guessing
    # here would trade a legible skip for a session that may not come back.

    # Must match `HIDE_WORKSPACE` in crates/beckon-linux/src/hyprland.rs.
    # Step 5c parks the window on a special workspace instead of unmapping
    # it, so a hidden window is still in `j/clients` and the workspace name
    # is the only thing that says it is hidden.
    HIDE_WORKSPACE = "special:beckon"

    def __init__(self) -> None:
        if not shutil.which("hyprctl"):
            raise RuntimeError("hyprctl not on PATH")
        if self._json("activeworkspace") is None:
            raise RuntimeError(
                "hyprctl did not answer — is HYPRLAND_INSTANCE_SIGNATURE pointing at a "
                "live Hyprland instance?"
            )

    def _json(self, kind: str):
        """`hyprctl -j <kind>`, or None when the query failed.

        Degrading to None rather than raising is what keeps the suite's
        `wait_for` polls working the way they do on every other backend, where
        a failed probe reads as "nothing yet". `__init__` is the loud gate.

        `is_hidden` is the one poll that reads the other way — an empty client
        list means "gone", so a failed query there passes instead of retrying.
        Same shape as `KdeEnv.is_hidden`, and unmeasured on either.
        """
        res = run(["hyprctl", "-j", kind], timeout=15)
        if res.returncode != 0:
            return None
        try:
            return json.loads(res.stdout)
        except ValueError:
            return None

    def _clients(self) -> list[dict]:
        data = self._json("clients")
        return data if isinstance(data, list) else []

    def _dispatch(self, *args: str) -> None:
        """`hyprctl dispatch …`, loud when the compositor refuses.

        Hyprland answers a dispatch with `ok` or with the reason it declined
        — the same check `hyprland::dispatch` makes — and nothing in the suite
        polls these for success. A silently dropped `focuswindow` would come
        back as an unexplained skip out of `force_focus` instead.
        """
        res = run(["hyprctl", "dispatch", *args], timeout=15)
        body = (res.stdout or "").strip()
        if res.returncode != 0 or body.lower() != "ok":
            raise RuntimeError(
                f"hyprctl dispatch {' '.join(args)} -> rc={res.returncode} {body!r}"
            )

    def spawn(self, argv: list[str]) -> None:
        # Through the compositor rather than forking from here: Hyprland hands
        # its children WAYLAND_DISPLAY and the DISPLAY of its own Xwayland,
        # neither of which is necessarily in the environment the suite was
        # started from (an ssh shell, say).
        self._dispatch("exec", " ".join(argv))

    def windows(self) -> list[Win]:
        # The address is Hyprland's own `0x…` string, used verbatim: beckon
        # matches the `j/activewindow` address against the `j/clients` ones by
        # exact string compare (`algorithm::decide`, fed by `snapshots_from`),
        # so normalising it here would test something beckon does not do.
        out = []
        for c in self._clients():
            cls = c.get("class") or ""
            if cls:
                out.append(Win(c.get("address") or "", cls, c.get("title") or ""))
        return out

    def focused(self) -> Win | None:
        data = self._json("activewindow")
        if not isinstance(data, dict):
            return None
        # Hyprland answers `{}` when nothing is focused; `hyprland::parse_active`
        # also treats an empty or `0x0` address as nothing, so this does too.
        addr = (data.get("address") or "").strip()
        if not addr or addr == "0x0":
            return None
        return Win(addr, data.get("class") or "", data.get("title") or "")

    def activate(self, wid: str) -> None:
        self._dispatch("focuswindow", f"address:{wid}")

    def is_hidden(self, wid: str) -> bool:
        # Hyprland hide == parked on `special:beckon`, so hidden-ness is a
        # property of where the window lives, not of whether it is still
        # listed — the same shape as SwayEnv, where the node stays in the tree
        # but hangs off `__i3_scratch`.
        for c in self._clients():
            if c.get("address") == wid:
                return (c.get("workspace") or {}).get("name") == self.HIDE_WORKSPACE
        return True  # gone from the list entirely


class GnomeEnv(Env):
    name = "GNOME Wayland (shell extension)"
    default_multi = "foot"
    default_other = "org.gnome.Calculator"

    DEST = "org.gnome.Shell"
    PATH = "/com/github/xom11/beckon"
    IFACE = "org.gnome.Shell.Extensions.Beckon"

    def __init__(self) -> None:
        if not shutil.which("busctl"):
            raise RuntimeError("busctl not on PATH")
        if self._call("ListWindows") is None:
            raise RuntimeError("beckon GNOME extension not reachable on the session bus")

    def _call(self, method: str) -> list | None:
        # `--json=short` gives real JSON. Do NOT parse gdbus' GVariant text
        # instead: it annotates types only on the first element of an array
        # (`uint64 22, ... uint32 0` then bare `21, ..., 0`), which is very
        # easy to under-match and then silently see fewer windows than exist.
        res = run(
            ["busctl", "--user", "--json=short", "call",
             self.DEST, self.PATH, self.IFACE, method],
            timeout=15,
        )
        if res.returncode != 0:
            return None
        try:
            return json.loads(res.stdout)["data"]
        except (ValueError, KeyError):
            return None

    def windows(self) -> list[Win]:
        data = self._call("ListWindows")
        if not data:
            return []
        return [Win(str(row[0]), row[1], row[2]) for row in data[0]]

    def focused(self) -> Win | None:
        data = self._call("GetFocusedWindow")
        if not data or not data[0]:
            return None
        wid = str(data[0])
        for w in self.windows():
            if w.wid == wid:
                return w
        return Win(wid, "", "")

    def activate(self, wid: str) -> None:
        run(["busctl", "--user", "call", self.DEST, self.PATH, self.IFACE,
             "ActivateWindow", "t", wid], timeout=15)

    def is_hidden(self, wid: str) -> bool:
        # Minimized windows stay in the list; "hidden" here means it lost focus
        # and nothing of that app is focused any more.
        f = self.focused()
        return f is None or f.wid != wid


class KdeEnv(Env):
    """KWin, probed through its scripting engine.

    Deliberately a different transport from the backend under test: beckon
    gets its answer back via `callDBus` into its own Rust service, this reads
    what a script `print()`ed onto KWin's stderr. Same engine, independent
    path — so a bug in beckon's reply plumbing cannot make the oracle agree
    with it.
    """

    name = "KDE Wayland (KWin scripting)"
    default_multi = "foot"
    default_other = "org.gnome.Calculator"

    DEST, OBJ, IFACE = "org.kde.KWin", "/Scripting", "org.kde.kwin.Scripting"
    MARKER = "BECKON-TEST "
    PLUGIN = "beckon-live-test"

    QUERY_JS = r"""(function () {
    var wins = (typeof workspace.stackingOrder !== "undefined" && workspace.stackingOrder)
        ? workspace.stackingOrder.slice().reverse()
        : workspace.windowList();
    var active = workspace.activeWindow;
    var out = [];
    for (var i = 0; i < wins.length; i++) {
        var w = wins[i];
        if (!w.normalWindow) continue;
        if (w.skipTaskbar) continue;
        var c = w.resourceClass || w.resourceName || "";
        if (!c) continue;
        out.push({
            id: String(w.internalId),
            cls: String(c),
            title: String(w.caption || ""),
            active: (active !== null && active !== undefined && w === active),
            minimized: !!w.minimized
        });
    }
    print("BECKON-TEST " + JSON.stringify(out));
})();"""

    def __init__(self) -> None:
        if not shutil.which("busctl"):
            raise RuntimeError("busctl not on PATH")
        self.log = os.environ.get("BECKON_TEST_KWIN_LOG", "/tmp/beckon-kde-shell.log")
        if not os.path.exists(self.log):
            raise RuntimeError(
                f"KWin's stderr log not found at {self.log} — start kwin_wayland with its "
                "output redirected to a file, or set BECKON_TEST_KWIN_LOG"
            )
        if self._rows() is None:
            raise RuntimeError("KWin scripting did not answer; is this a KWin session?")

    def _busctl(self, method: str, *args: str) -> bool:
        res = run(["busctl", "--user", "call", self.DEST, self.OBJ, self.IFACE, method, *args],
                  timeout=15)
        return res.returncode == 0

    def _run_js(self, js: str, expect_output: bool) -> str | None:
        path = f"/tmp/{self.PLUGIN}.js"
        with open(path, "w") as fh:
            fh.write(js)
        try:
            offset = os.path.getsize(self.log)
        except OSError:
            offset = 0
        self._busctl("unloadScript", "s", self.PLUGIN)
        if not self._busctl("loadScript", "ss", path, self.PLUGIN):
            return None
        self._busctl("start")
        if not expect_output:
            time.sleep(0.4)
            self._busctl("unloadScript", "s", self.PLUGIN)
            return ""

        deadline = time.monotonic() + 5.0
        payload = None
        while time.monotonic() < deadline and payload is None:
            time.sleep(0.1)
            try:
                with open(self.log, errors="replace") as fh:
                    fh.seek(offset)
                    for line in fh:
                        idx = line.find(self.MARKER)
                        if idx != -1:
                            payload = line[idx + len(self.MARKER):].strip()
            except OSError:
                pass
        self._busctl("unloadScript", "s", self.PLUGIN)
        return payload

    def _rows(self) -> list[dict] | None:
        payload = self._run_js(self.QUERY_JS, expect_output=True)
        if payload is None:
            return None
        try:
            return json.loads(payload)
        except ValueError:
            return None

    def windows(self) -> list[Win]:
        rows = self._rows() or []
        return [Win(r["id"], r["cls"], r.get("title", "")) for r in rows]

    def focused(self) -> Win | None:
        for r in self._rows() or []:
            if r.get("active"):
                return Win(r["id"], r["cls"], r.get("title", ""))
        return None

    def activate(self, wid: str) -> None:
        js = (
            "(function () {\n"
            f"    var target = {json.dumps(wid)};\n"
            "    var wins = workspace.windowList();\n"
            "    for (var i = 0; i < wins.length; i++) {\n"
            "        if (String(wins[i].internalId) === target) {\n"
            "            wins[i].minimized = false;\n"
            "            workspace.activeWindow = wins[i];\n"
            "            return;\n"
            "        }\n"
            "    }\n"
            "})();"
        )
        self._run_js(js, expect_output=False)

    def is_hidden(self, wid: str) -> bool:
        for r in self._rows() or []:
            if r["id"] == wid:
                return bool(r.get("minimized"))
        return True  # gone from the list entirely


class X11Env(Env):
    name = "X11 (EWMH)"
    default_multi = "xterm"
    default_other = "xclock"

    def __init__(self) -> None:
        for tool in ("xprop", "xdotool"):
            if not shutil.which(tool):
                raise RuntimeError(f"{tool} not on PATH")
        if run(["xprop", "-root", "_NET_CLIENT_LIST_STACKING"]).returncode != 0:
            raise RuntimeError("no EWMH window manager on this DISPLAY")

    def _stacking(self) -> list[str]:
        out = run(["xprop", "-root", "_NET_CLIENT_LIST_STACKING"]).stdout
        return list(reversed(re.findall(r"0x[0-9a-fA-F]+", out)))

    def _class_of(self, wid: str) -> str:
        out = run(["xprop", "-id", wid, "WM_CLASS"]).stdout
        m = re.search(r'WM_CLASS\(STRING\) = "([^"]*)", "([^"]*)"', out)
        return m.group(2) if m else ""

    def _title_of(self, wid: str) -> str:
        out = run(["xprop", "-id", wid, "_NET_WM_NAME"]).stdout
        m = re.search(r'= "(.*)"', out)
        return m.group(1) if m else ""

    def windows(self) -> list[Win]:
        out = []
        for wid in self._stacking():
            cls = self._class_of(wid)
            if cls:
                out.append(Win(self._norm(wid), cls, self._title_of(wid)))
        return out

    @staticmethod
    def _norm(wid: str) -> str:
        # beckon prints window ids in decimal; xprop in hex. Normalise to decimal.
        return str(int(wid, 16))

    def focused(self) -> Win | None:
        out = run(["xprop", "-root", "_NET_ACTIVE_WINDOW"]).stdout
        m = re.search(r"0x[0-9a-fA-F]+", out)
        if not m or int(m.group(0), 16) == 0:
            return None
        hexid = m.group(0)
        cls = self._class_of(hexid)
        return Win(self._norm(hexid), cls, self._title_of(hexid))

    def activate(self, wid: str) -> None:
        run(["xdotool", "windowactivate", "--sync", str(int(wid))], timeout=15)

    def is_hidden(self, wid: str) -> bool:
        # Require the ICCCM state, not just "lost focus": the WM drops
        # `_NET_ACTIVE_WINDOW` to 0 before it finishes iconifying, so a
        # focus-only check reports success mid-transition and the next step
        # races the WM.
        out = run(["xprop", "-id", hex(int(wid)), "WM_STATE"]).stdout
        return "Iconic" in out


def detect_env() -> Env:
    if os.environ.get("SWAYSOCK") or os.environ.get("I3SOCK"):
        return SwayEnv()
    # Ahead of WAYLAND_DISPLAY, and for the reason `pick_backend` orders it
    # that way too: Hyprland sets WAYLAND_DISPLAY as well, so a later branch
    # reads a Hyprland session as GNOME and probes for an extension that will
    # never be there.
    if os.environ.get("HYPRLAND_INSTANCE_SIGNATURE"):
        return HyprEnv()
    if os.environ.get("WAYLAND_DISPLAY"):
        # Same rule beckon's own pick_backend uses: XDG_CURRENT_DESKTOP is a
        # colon-separated list, matched per component.
        parts = [p.strip().upper() for p in os.environ.get("XDG_CURRENT_DESKTOP", "").split(":")]
        if "KDE" in parts or "PLASMA" in parts:
            return KdeEnv()
        return GnomeEnv()
    if os.environ.get("DISPLAY"):
        return X11Env()
    raise RuntimeError("no supported display server in this environment")


# --------------------------------------------------------------------------
# the suite
# --------------------------------------------------------------------------


class Suite:
    def __init__(self, beckon: str, env: Env, multi: str, other: str, verbose: bool,
                 only: str | None = None) -> None:
        self.beckon = beckon
        self.env = env
        self.multi = multi
        self.other = other
        self.verbose = verbose
        self.only = only
        self.report = Report()
        self.multi_cls: str | None = None
        self.other_cls: str | None = None

    # ---- helpers ---------------------------------------------------------

    def call(self, *args: str) -> tuple[int, str, str]:
        res = run([self.beckon, "-v", *args], timeout=30)
        if self.verbose:
            print(f"    {DIM}$ beckon -v {' '.join(args)} -> rc={res.returncode}{RESET}")
            for line in (res.stderr or "").splitlines():
                print(f"      {DIM}{line}{RESET}")
        return res.returncode, res.stdout, res.stderr

    @staticmethod
    def action_of(stderr: str) -> str | None:
        m = re.search(r"action:\s*(\w+)", stderr)
        return m.group(1) if m else None

    def beckon_expect(self, app: str, expected: str) -> str:
        rc, _, err = self.call(app)
        if rc != 0:
            raise AssertionError(f"beckon {app} exited {rc}: {err.strip()[:400]}")
        got = self.action_of(err)
        if got != expected:
            raise AssertionError(f"beckon {app}: expected action {expected}, got {got}")
        return got

    # Every process the suite may have started, by the name it actually runs
    # under. `pkill -x` silently matches nothing for names over 15 chars
    # (the kernel comm limit), so anything longer has to go through `-f`.
    KILLABLE = (
        "foot", "xterm", "xclock",
        "gnome-calculator", "gnome-text-editor", "gnome-system-monitor",
    )

    def kill_apps(self) -> None:
        names = set(self.KILLABLE) | set(self.env.extra_kill)
        for app in (self.multi, self.other):
            names.add(os.path.basename(app.split()[0]))
        for name in names:
            flag = "-x" if len(name) <= 15 else "-f"
            subprocess.run(["pkill", flag, name], capture_output=True)
        if not wait_for(lambda: not self.env.windows(), timeout=10):
            leftover = [f"{w.cls}:{w.wid}" for w in self.env.windows()]
            print(f"    {YELLOW}note{RESET} could not clear windows: {leftover}")

    def reset_mru(self) -> None:
        runtime = os.environ.get("XDG_RUNTIME_DIR")
        if runtime:
            try:
                os.remove(os.path.join(runtime, "beckon-mru"))
            except FileNotFoundError:
                pass

    def clean(self) -> None:
        self.kill_apps()
        self.reset_mru()

    def launch_and_wait(self, app: str) -> Win:
        """beckon-launch `app`, wait for its window, return it."""
        before = {w.wid for w in self.env.windows()}
        self.beckon_expect(app, "Launched")
        new = wait_for(lambda: [w for w in self.env.windows() if w.wid not in before], timeout=25)
        if not new:
            raise AssertionError(f"`beckon {app}` reported Launched but no window appeared")
        self.settle()
        return new[0]

    def settle(self, timeout: float = 6.0) -> Win | None:
        """Wait until focus stops moving.

        A freshly mapped window takes focus asynchronously, so reading focus
        right after a launch races the compositor — and a racy precondition
        makes beckon look wrong when it is not.
        """
        last, stable_since = object(), 0.0
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            cur = self.env.focused()
            key = cur.wid if cur else None
            now = time.monotonic()
            if key == last:
                if now - stable_since > 0.7:
                    return cur
            else:
                last, stable_since = key, now
            time.sleep(0.15)
        return self.env.focused()

    def force_focus(self, win: Win) -> None:
        """Establish 'this window is focused' via the compositor, not beckon."""
        self.env.activate(win.wid)
        got = wait_for(lambda: (self.env.focused() or Win("", "")).wid == win.wid, timeout=8)
        self.settle()
        if not got:
            raise Skip(
                f"compositor would not focus {win.cls}#{win.wid} on request "
                "(headless session with no seat?)"
            )

    def snapshot(self) -> str:
        """What the compositor sees right now — printed on every failure so a
        red line is actionable without a second run."""
        try:
            wins = ", ".join(f"{w.cls}#{w.wid}({w.title!r})" for w in self.env.windows())
            f = self.env.focused()
            mru = "-"
            runtime = os.environ.get("XDG_RUNTIME_DIR")
            if runtime:
                try:
                    with open(os.path.join(runtime, "beckon-mru")) as fh:
                        mru = fh.read().strip()
                except OSError:
                    pass
            return (f"       windows: [{wins or 'none'}]\n"
                    f"       focused: {f.cls + '#' + f.wid if f else '(none)'}\n"
                    f"       mru:     {mru}")
        except Exception as e:  # noqa: BLE001
            return f"       (could not snapshot state: {e})"

    def case(self, name, fn) -> None:
        if self.only and self.only not in name:
            return
        try:
            self.clean()
            fn()
        except Skip as e:
            self.report.skip(name, str(e))
        except AssertionError as e:
            self.report.fail(name, f"{e}\n{self.snapshot()}")
        except Exception as e:  # noqa: BLE001 - surface the whole thing
            self.report.fail(name, f"{type(e).__name__}: {e}\n{self.snapshot()}")
        else:
            self.report.ok(name)

    # ---- discovery commands ---------------------------------------------

    def t_doctor(self) -> None:
        res = run([self.beckon, "doctor"])
        if res.returncode != 0:
            raise AssertionError(f"doctor exited {res.returncode}: {res.stderr[:300]}")
        if "Backend selected" not in res.stdout:
            raise AssertionError(f"doctor did not report a backend:\n{res.stdout[:500]}")

    def t_list_installed(self) -> None:
        res = run([self.beckon, "installed"], timeout=30)
        if res.returncode != 0:
            raise AssertionError(f"installed exited {res.returncode}: {res.stderr[:300]}")
        if len(res.stdout.splitlines()) < 5:
            raise AssertionError(f"installed listed almost nothing:\n{res.stdout[:300]}")

    def t_search(self) -> None:
        res = run([self.beckon, "search", self.multi[:4]], timeout=30)
        if res.returncode != 0:
            raise AssertionError(f"search exited {res.returncode}: {res.stderr[:300]}")

    def t_resolve(self) -> None:
        res = run([self.beckon, "resolve", self.multi], timeout=30)
        if res.returncode != 0:
            raise AssertionError(
                f"resolve {self.multi} exited {res.returncode}: {res.stderr[:300]}"
            )

    def t_resolve_unknown(self) -> None:
        res = run([self.beckon, "resolve", "definitely-not-installed-zzz"], timeout=30)
        if res.returncode == 0 and "no match" not in (res.stdout + res.stderr).lower():
            raise AssertionError(
                "resolve on an unknown id neither failed nor said 'no match':\n"
                f"{(res.stdout + res.stderr)[:300]}"
            )

    def t_unknown_id_fails(self) -> None:
        res = run([self.beckon, "definitely-not-installed-zzz"], timeout=30)
        if res.returncode == 0:
            raise AssertionError("beckon on an unknown id exited 0 (should be an error)")

    def t_dash_id(self) -> None:
        """`beckon -- -weird.id` must be treated as an id, not a flag."""
        res = run([self.beckon, "--", "-weird.id"], timeout=30)
        combined = res.stdout + res.stderr
        if "unexpected argument" in combined or "Usage:" in combined and res.returncode == 2:
            raise AssertionError(f"`beckon -- -weird.id` was parsed as a flag:\n{combined[:300]}")

    # ---- the focus algorithm --------------------------------------------

    def t_launch(self) -> None:
        self.multi_cls = self.launch_and_wait(self.multi).cls

    def t_focus(self) -> None:
        target = self.launch_and_wait(self.multi)
        self.multi_cls = target.cls
        other = self.launch_and_wait(self.other)
        self.other_cls = other.cls
        # precondition, set by the compositor: the OTHER app has focus
        self.force_focus(other)
        self.beckon_expect(self.multi, "Focused")
        got = wait_for(lambda: (self.env.focused() or Win("", "")).cls == self.multi_cls, timeout=8)
        if not got:
            raise AssertionError(
                "after Focused, compositor reports "
                f"{(self.env.focused() or Win('', '(none)')).cls!r}, want {self.multi_cls!r}"
            )

    def t_cycle(self) -> None:
        first = self.launch_and_wait(self.multi)
        self.multi_cls = first.cls
        # second window of the same app, spawned directly (not via beckon)
        self.env.spawn([self.multi])
        two = wait_for(lambda: len(self.env.of_class(self.multi_cls)) >= 2, timeout=20)
        if not two:
            raise Skip(f"{self.multi} would not open a second window")
        # precondition: the FIRST window of the app has focus
        self.force_focus(first)
        self.beckon_expect(self.multi, "Cycled")
        moved = wait_for(
            lambda: (self.env.focused() or Win("", first.wid)).wid != first.wid, timeout=8
        )
        if not moved:
            raise AssertionError(
                f"Cycled reported, but focus stayed on window {first.wid} ({first.title!r})"
            )
        now = self.env.focused()
        if now is None or now.cls != self.multi_cls:
            raise AssertionError(
                f"cycle left focus on {(now.cls if now else '(none)')!r}, want {self.multi_cls!r}"
            )

    def t_cycle_reaches_all_three(self) -> None:
        """Repeated presses must round-robin every window, not ping-pong.

        The old rule picked the globally least-recent other window of the
        app; on any backend with real focus history that is the window we
        just left, so windows 3..N were unreachable.
        """
        first = self.launch_and_wait(self.multi)
        self.multi_cls = first.cls
        for _ in range(2):
            self.env.spawn([self.multi])
        three = wait_for(lambda: len(self.env.of_class(self.multi_cls)) >= 3, timeout=25)
        if not three:
            raise Skip(f"{self.multi} would not open three windows")
        self.force_focus(first)
        seen = {first.wid}
        for i in range(6):
            self.beckon_expect(self.multi, "Cycled")
            cur = wait_for(lambda: self.env.focused(), timeout=8)
            if cur is None:
                raise Skip("compositor reports no focused window")
            self.settle()
            cur = self.env.focused()
            seen.add(cur.wid)
            if len(seen) >= 3:
                break
        if len(seen) < 3:
            raise AssertionError(
                f"after 6 presses the cycle only ever reached {len(seen)} of 3 windows "
                f"({sorted(seen)}) — it is stuck in a 2-cycle"
            )

    def t_toggle_back(self) -> None:
        target = self.launch_and_wait(self.multi)
        self.multi_cls = target.cls
        other = self.launch_and_wait(self.other)
        self.other_cls = other.cls
        if len(self.env.of_class(self.multi_cls)) != 1:
            raise Skip(f"{self.multi} opened more than one window; not a toggle-back scenario")
        # Teach the MRU file that `other` is where we came from: focus other,
        # then let beckon focus the target (that call records the previous app).
        self.force_focus(other)
        self.beckon_expect(self.multi, "Focused")
        if not wait_for(
            lambda: (self.env.focused() or Win("", "")).cls == self.multi_cls, timeout=8
        ):
            raise Skip("could not get the target focused to set up toggle-back")
        self.settle()
        self.beckon_expect(self.multi, "ToggledBack")
        landed = wait_for(
            lambda: (self.env.focused() or Win("", "")).cls == self.other_cls, timeout=8
        )
        if not landed:
            raise AssertionError(
                "ToggledBack landed on "
                f"{(self.env.focused() or Win('', '(none)')).cls!r}, want {self.other_cls!r}"
            )

    def _hide_alone(self) -> Win:
        """Launch the target as the only window and hide it. Returns the window."""
        win = self.launch_and_wait(self.multi)
        self.multi_cls = win.cls
        if len(self.env.windows()) != 1:
            raise Skip(
                f"session has extra windows ({[w.cls for w in self.env.windows()]}); "
                "hide needs the target to be the only one"
            )
        self.force_focus(win)
        self.beckon_expect(self.multi, "Hidden")
        if not wait_for(lambda: self.env.is_hidden(win.wid), timeout=8):
            raise AssertionError(f"Hidden reported, but window {win.wid} is still visible/focused")
        return win

    def t_hide(self) -> None:
        self._hide_alone()

    def t_restore_after_hide(self) -> None:
        """A hidden window must come back on the next beckon call."""
        self._hide_alone()
        rc, _, err = self.call(self.multi)
        if rc != 0:
            raise AssertionError(f"beckon after hide exited {rc}: {err.strip()[:300]}")
        act = self.action_of(err)
        if act == "Launched":
            raise AssertionError(
                "after Hidden, beckon launched a SECOND instance instead of restoring "
                "the hidden window"
            )
        back = wait_for(
            lambda: (self.env.focused() or Win("", "")).cls == self.multi_cls, timeout=10
        )
        if not back:
            raise AssertionError(
                f"hidden window was not restored (action={act}, focused="
                f"{(self.env.focused() or Win('', '(none)')).cls!r})"
            )

    def t_no_duplicate_launch(self) -> None:
        """Pressing the hotkey for a running app must never start a copy.

        This is where the `.desktop`-stem-only match used to fail: an X11 /
        XWayland app advertises its `WM_CLASS` (`XTerm`), not the desktop
        file stem (`debian-xterm`), so every press launched another window.
        """
        win = self.launch_and_wait(self.multi)
        self.multi_cls = win.cls
        before = len(self.env.of_class(self.multi_cls))
        for _ in range(3):
            rc, _, err = self.call(self.multi)
            if rc != 0:
                raise AssertionError(f"beckon {self.multi} exited {rc}: {err.strip()[:300]}")
            if self.action_of(err) == "Launched":
                raise AssertionError(
                    f"`beckon {self.multi}` launched a duplicate while the app was "
                    f"already running (class {self.multi_cls!r})"
                )
            time.sleep(1.0)
        after = len(self.env.of_class(self.multi_cls))
        if after > before:
            raise AssertionError(
                f"window count for {self.multi_cls!r} grew {before} -> {after} "
                "across repeated presses"
            )

    def t_empty_id_is_an_error(self) -> None:
        res = run([self.beckon, ""], timeout=20)
        if res.returncode == 0:
            raise AssertionError(
                "`beckon \"\"` exited 0 — an unset $APP in a dotfile would launch "
                "whatever app sorts first"
            )

    def t_resolve_is_deterministic(self) -> None:
        """Same input, same answer — `scan()` must not expose HashMap order."""
        outs = set()
        for _ in range(8):
            res = run([self.beckon, "resolve", self.multi], timeout=20)
            m = re.search(r"^\s*Runtime id:\s*(.+)$", res.stdout, re.M)
            outs.add(m.group(1).strip() if m else res.stdout)
        if len(outs) > 1:
            raise AssertionError(f"resolve {self.multi} resolved differently across runs: {outs}")

    def t_list_running(self) -> None:
        self.multi_cls = self.launch_and_wait(self.multi).cls
        res = run([self.beckon, "list"], timeout=20)
        if res.returncode != 0:
            raise AssertionError(f"list exited {res.returncode}: {res.stderr[:300]}")
        if self.multi_cls not in res.stdout:
            raise AssertionError(
                f"list does not list the running app {self.multi_cls!r}:\n{res.stdout[:400]}"
            )

    def t_beckon_by_name(self) -> None:
        """The documented happy path: bind by human-readable Name, not by id."""
        self.multi_cls = self.launch_and_wait(self.multi).cls
        res = run([self.beckon, "resolve", self.multi], timeout=20)
        m = re.search(r"^\s*name\s*[:=]\s*(.+)$", res.stdout, re.M | re.I)
        if not m:
            raise Skip(f"resolve output has no Name line to test with:\n{res.stdout[:300]}")
        name = m.group(1).strip()
        rc, _, err = self.call(name)
        if rc != 0:
            raise AssertionError(f"beckon by Name {name!r} exited {rc}: {err.strip()[:300]}")
        act = self.action_of(err)
        if act == "Launched":
            raise AssertionError(
                f"beckon {name!r} (the Name of an already-running app) launched a "
                "second instance instead of focusing the running one"
            )

    def run_all(self) -> Report:
        print(f"\n{DIM}environment:{RESET} {self.env.name}")
        print(f"{DIM}binary:     {RESET} {self.beckon}")
        print(f"{DIM}apps:       {RESET} multi={self.multi} other={self.other}\n")

        print("discovery commands")
        for name, fn in [
            ("doctor reports a backend", self.t_doctor),
            ("installed lists installed apps", self.t_list_installed),
            ("search runs", self.t_search),
            ("resolve resolves a known id", self.t_resolve),
            ("resolve on unknown id reports no match", self.t_resolve_unknown),
            ("unknown id is a hard error", self.t_unknown_id_fails),
            ("empty id is a hard error", self.t_empty_id_is_an_error),
            ("`-- -weird.id` parses as an id", self.t_dash_id),
            ("resolve resolves deterministically", self.t_resolve_is_deterministic),
        ]:
            self.case(name, fn)

        print("\nfocus algorithm")
        for name, fn in [
            ("step 2: launch when not running", self.t_launch),
            ("list lists the running app", self.t_list_running),
            ("running app is never launched twice", self.t_no_duplicate_launch),
            ("step 3: focus when running, unfocused", self.t_focus),
            ("step 5a: cycle within the same app", self.t_cycle),
            ("step 5a: cycle reaches all 3 windows", self.t_cycle_reaches_all_three),
            ("step 5b: toggle back to the previous app", self.t_toggle_back),
            ("step 5c: hide the lone window", self.t_hide),
            ("hidden window is restored, not relaunched", self.t_restore_after_hide),
            ("beckon by human-readable Name", self.t_beckon_by_name),
        ]:
            self.case(name, fn)

        return self.report


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--beckon", default="beckon", help="path to the beckon binary")
    ap.add_argument("--multi", help="app id that can open several windows")
    ap.add_argument("--other", help="a second app id to toggle back to")
    ap.add_argument("--only", help="run only tests whose name contains this substring")
    ap.add_argument("-v", "--verbose", action="store_true")
    args = ap.parse_args()

    try:
        env = detect_env()
    except Exception as e:  # noqa: BLE001
        print(f"{RED}cannot run:{RESET} {e}", file=sys.stderr)
        return 2

    suite = Suite(
        beckon=args.beckon,
        env=env,
        multi=args.multi or env.default_multi,
        other=args.other or env.default_other,
        verbose=args.verbose,
        only=args.only,
    )
    report = suite.run_all()
    suite.clean()

    print(
        f"\n{len(report.passed)} passed, {len(report.failed)} failed, "
        f"{len(report.skipped)} skipped"
    )
    for name, why in report.failed:
        print(f"{RED}  FAIL{RESET} {name}: {why}")
    return 1 if report.failed else 0


if __name__ == "__main__":
    sys.exit(main())
