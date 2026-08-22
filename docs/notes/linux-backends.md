# Linux backends — per-compositor implementation notes

Extracted from `CLAUDE.md` 2026-08-17. The dispatch table and the shared
algorithm's contract live there; this file is the per-backend detail.

All five backends pass `testing/linux_live_test.py` 19/19.

## sway / i3 share one module

sway and i3 share the i3-IPC protocol exactly — same `swayipc` crate, same JSON
tree, same `[con_id=N] focus` command, same scratchpad. The only differences:

- **Window identity**: Wayland uses `node.app_id`; X11 uses
  `window_properties.class` (second token of `WM_CLASS`). `collect_windows`
  already falls back from one to the other.
- **Socket env var**: `SWAYSOCK` for sway, `I3SOCK` for i3. The dispatcher
  accepts either.

→ No separate i3 module. `crates/beckon-linux/src/i3ipc.rs` serves both.

## The shared focus algorithm

Every Linux backend feeds a neutral `Vec<algorithm::WindowSnapshot>` into
`algorithm::decide` and dispatches the resulting `Decision` (`Launch` / `Focus`
/ `Cycle` / `ToggleBack` / `Hide`). The algorithm lives in
`crates/beckon-linux/src/algorithm.rs` — that's the only place to change focus
/ cycle / toggle / hide policy.

Each backend owns the projection from native window data to `WindowSnapshot`
(the `snapshots_from` helper at the top of every backend file) and the
translation from `Decision` to native commands.

`recency` semantics in `WindowSnapshot`:

- Hyprland: `focusHistoryID` straight through (0 = currently focused).
- X11: inverted index into `_NET_CLIENT_LIST_STACKING` (top of stack → 0).
- sway / i3: tree traversal index — degenerates to "first match wins" since the
  tree carries no real focus history. The `algorithm::decide` ties on recency
  are broken by address, so the deterministic order matches what `i3ipc.rs` did
  before the refactor.

**Target matching is a set, not a string.** `decide` takes an
`algorithm::Target` — every class that counts as the requested app — and
compares case-insensitively. One id shows up under different strings depending
on the client, and the user has no say in which: `debian-xterm.desktop` is
reported as `debian-xterm` by a Wayland-native client and as `XTerm` by the
same app under X11/XWayland. Matching on the `.desktop` stem alone meant beckon
never recognised the running app and launched another copy on *every* keypress
— confirmed live on sway (5 presses, 5 xterms). `desktop::target_classes`
builds the set: `.desktop` filename stem plus `StartupWMClass=`, or the raw id
when nothing resolved (which is what lets beckon focus ad-hoc apps that ship no
`.desktop` file).

**Step 5a cycles over a ring ordered by address, not by recency.** Picking "the
least-recent other window of this app" looks right but is a 2-cycle on every
backend whose `recency` is real focus history: focusing a window promotes it
and demotes the one you just left, so the next press goes straight back and
windows 3..N are unreachable. Addresses are the compositor's own window ids
(con_id / stable_sequence / X11 id / Hyprland pointer) — stable for the
window's lifetime and ordered by creation — so rotating over them visits every
window exactly once per lap. Verified live on sway: three `foot` windows, seven
presses, `35 → 36 → 37 → 35 → …`.

## X11 generic (`x11.rs`)

Covers every EWMH-compliant X11 desktop — GNOME-X11, KDE-X11, XFCE, openbox,
awesome, fluxbox. (i3 has its own faster path through `i3ipc.rs`.)

- **Connection**: `x11rb::connect(None)` — pure-Rust, no `libxcb` link. The
  connection lives for the life of `X11Backend` (one beckon invocation is one
  connection — no daemon).
- **Window list**: `_NET_CLIENT_LIST_STACKING` on root, reversed so index 0 is
  the topmost window (≈ most-recently focused). Windows without a `WM_CLASS`
  are filtered out — they're typically transient chrome (notifications, menus)
  that beckon shouldn't surface as "apps". So are windows whose
  `_NET_WM_WINDOW_TYPE` is neither NORMAL, DIALOG nor UTILITY: panels and docks
  (tint2, xfce4-panel) do carry a `WM_CLASS`, and letting one through makes
  step 5b "toggle back" to a panel the WM then refuses to focus — beckon
  reports success and nothing moves. A window with no `_NET_WM_WINDOW_TYPE` at
  all is treated as NORMAL, per EWMH.
- **Class matching**: `WM_CLASS[1]` (the second NUL-separated token, the
  "class" component), matched case-insensitively against the candidate set
  (`StartupWMClass=` first, then the `.desktop` filename stem). Case matters in
  practice: `xterm.desktop` has no `StartupWMClass` and the window advertises
  `XTerm`, so a byte-wise compare launched a new xterm on every press.
- **Active window**: `_NET_ACTIVE_WINDOW` root property; treats `0` as None.
- **Focus**: `_NET_ACTIVE_WINDOW` ClientMessage to root with source = 2
  (pager/taskbar). Source 2 is what `wmctrl -a` sends and what most WMs treat
  as a legitimate user action — bypasses focus-stealing prevention.
- **Hide**: ICCCM `WM_CHANGE_STATE` ClientMessage with `IconicState` (3).
  Universal across X11 WMs. We deliberately don't toggle
  `_NET_WM_STATE_HIDDEN` — that's spec'd as a hint the WM sets, not a
  client-driven toggle.
- **Restore from hidden**: an explicit map, then a wait, then the focus request
  — `ensure_mapped`. The old claim here was that EWMH's "the WM SHOULD bring
  the window forward" means every WM de-iconifies on a focus request.
  **openbox does not.** Measured on Ubuntu 26.04 + Xvfb + openbox: after
  beckon's own step-5c hide, `_NET_ACTIVE_WINDOW` alone left the window at
  `WM_STATE = Iconic` indefinitely — the hotkey could never bring it back and
  the window was stranded for good. ICCCM §4.1.4 is the portable answer: map
  the window to return it to `NormalState`; the WM holds SubstructureRedirect
  on the root, so the MapRequest reaches it.

  The wait is the other half and is not optional: the WM is just another
  client, so flushing the MapRequest only proves the *server* saw it. Sending
  the activation in the same breath lost the race every time, while the same
  map-then-activate pair issued as two separate `xdotool` calls always worked.
  `ensure_mapped` polls `map_state` (server state, unlike the WM-owned
  `WM_STATE`) for up to 400 ms. Only the restore path pays it; a normal focus
  costs one round-trip and no sleep.
- **Launch**: `/bin/sh -c "setsid -f <Exec> >/dev/null 2>&1"`. `setsid -f`
  detaches from beckon's process group so the launched app survives beckon
  exiting. Stdout/stderr nulled to prevent stale fds keeping the parent
  terminal alive when invoked from a hotkey.
- **No focus-history MRU on X11**: `_NET_CLIENT_LIST_STACKING` already reflects
  z-order, the closest standardised proxy for MRU. No state file is needed for
  step 5a cycling. Step 5b still consults the cross-backend MRU file at
  `$XDG_RUNTIME_DIR/beckon-mru` so toggle-back lands on the same app the user
  actually came from across multiple beckon invocations.

## GNOME Wayland (`gnome.rs` + shell extension)

`crates/beckon-linux/src/gnome.rs` is a thin zbus client. The actual window
work happens inside `extensions/beckon@xom11.github.io/extension.js`, which
runs as a GNOME Shell extension (so it has direct access to Mutter via
`global.display`, `global.get_window_actors()`, `Main.activateWindow`). Without
an in-process collaborator there's no path at all on GNOME Wayland — Mutter has
no public protocol for external focus.

- **Bus surface** (`org.gnome.Shell` / `/com/github/xom11/beckon` /
  `org.gnome.Shell.Extensions.Beckon`):
  - `ListWindows() → a(tssbu)` — `(stable_seq, class, title, focused,
    monitor)`, MRU-ordered (`Meta.TabList.NORMAL_ALL`).
  - `GetFocusedWindow() → t` — `0` when no focus.
  - `ActivateWindow(t) → b` — calls `Main.activateWindow`, which switches
    workspace, unminimizes, raises and focuses in one shot. Mutter's own
    timestamp is used so focus-stealing prevention doesn't reject it.
  - `MinimizeWindow(t) → b` — `meta_window.minimize()`.
  - property `Version` — read at startup by the Rust client to verify the
    extension is loaded before trusting any other call.
- **Window identity**: `MetaWindow.get_stable_sequence()`. `uint32` that fits
  in the `t` (uint64) D-Bus type, stable for the window's lifetime, available
  on every supported GNOME version (no need for the newer `get_id()` API).
- **Class fallback ladder**: `get_wm_class()` → `get_gtk_application_id()` →
  `get_sandboxed_app_id()`. Wayland-native GTK apps frequently lack `WM_CLASS`
  and only set the GTK app id (`org.gnome.Console` etc.).
- **Recency**: `Meta.TabList.NORMAL_ALL` is exactly the order alt-tab walks,
  i.e. real focus history.
- **MRU file**: shares `$XDG_RUNTIME_DIR/beckon-mru` with the other Linux
  backends. Cross-backend sharing is safe — only one compositor runs at a time.
- **Launch path**: same `/bin/sh -c "setsid -f <Exec>"` recipe as the X11
  backend. Doesn't need to go through the extension because spawning a new
  process isn't what Mutter is gating.
- **Hot path cost**: 1 D-Bus connection (~10 ms) + 1 `ListWindows` round-trip +
  1 `ActivateWindow`/`MinimizeWindow` round-trip. Each call is ~1 ms over the
  session bus, well under the 50 ms budget.

### Installing / updating the extension

**Declarative (recommended, nix users)**: the flake exposes
`packages.<system>.beckon-gnome-extension` and the same name on
`overlays.default`. The package puts the extension at
`$out/share/gnome-shell/extensions/beckon@xom11.github.io/`. Drop it into
home-manager via `xdg.dataFile`:

```nix
# in your home-manager config (only needed on GNOME hosts)
xdg.dataFile."gnome-shell/extensions/beckon@xom11.github.io".source =
  "${pkgs.beckon-gnome-extension}/share/gnome-shell/extensions/beckon@xom11.github.io";
```

Plus add `"beckon@xom11.github.io"` to dconf
`org/gnome/shell.enabled-extensions` so gnome-shell turns it on at session
start. After the first `home-manager switch`: log out and back in (Wayland
can't reload shell live). Subsequent updates that change the extension code
also need a relogin; updates that change only beckon-cli do not.

**Manual (one-shot, useful for testing extension changes)**:

```sh
cd extensions
gnome-extensions pack beckon@xom11.github.io
gnome-extensions install --force beckon@xom11.github.io.shell-extension.zip
gnome-extensions enable beckon@xom11.github.io
# Wayland: log out and back in. (`busctl ... ReloadExtension` is gated on
# unsafe-mode and not available in normal sessions.)
```

`gnome-extensions install` writes a real directory under
`~/.local/share/gnome-shell/extensions/`. If you later switch to the
declarative path, remove that directory first — home-manager's symlink
activation refuses to clobber an unmanaged file.

## KDE Wayland (`kde.rs`)

The KDE counterpart of `gnome.rs`, with one big difference: **there is nothing
for the user to install.** KWin ships its own scripting engine and exposes it
on the session bus, so beckon loads a generated script, gets the answer back,
and unloads it.

- **Bus surface** (`org.kde.KWin` / `/Scripting` / `org.kde.kwin.Scripting`):
  `loadScript(path, pluginName) → i`, `start()`, `unloadScript(pluginName) → b`,
  `isScriptLoaded(pluginName) → b`. `isScriptLoaded` is the startup probe —
  read-only, and it proves both that KWin owns the name and that the scripting
  object is at the expected path.
- **Why not a Wayland protocol.** KWin advertises neither
  `zwlr_foreign_toplevel_management_v1` (wlroots-only) nor its own
  `org_kde_plasma_window_management`. Confirmed with `wayland-info` against
  `kwin_wayland 6.6.6`: the latter is simply not in the registry, so a protocol
  client cannot enumerate windows at all. Scripting is the only surface that
  exists in practice.

  This replaced an earlier claim that *"KWin doesn't have an equivalent
  extension API surface that we can ride on"*. That was wrong, and was
  falsified by running the thing: on `kwin_wayland 6.6.6` a loaded script
  enumerated every window with `resourceClass` / `caption` / `minimized` and
  moved focus by assigning `workspace.activeWindow`. Do not re-add the claim
  without re-testing it.
- **Getting data back out.** KWin scripts have no file I/O; `callDBus` is the
  only escape hatch. beckon therefore serves a one-method interface
  (`com.github.xom11.beckon.KWin.Windows`) on its own connection, bakes its
  unique bus name (`:1.42`) into the generated script, and blocks on an `mpsc`
  channel until the script calls back. Baking in the *unique* name rather than
  a well-known one is what keeps two concurrent beckon invocations from reading
  each other's replies.
- **Two script round trips per invocation**: one to read the window list, one
  to act. The read cannot be merged with the act because the decision needs the
  list first, and a script is fire-and-forget — it cannot wait for a reply from
  us.
- **Window identity**: `Window.internalId`, a QUuid rendered `{xxxxxxxx-…}`.
  Stable for the window's lifetime, which is all the algorithm needs — but
  unlike every other backend's address it is **not numeric**, so
  `algorithm::cmp_address` falls back to byte ordering. The step-5a cycle ring
  is therefore stable but not in window-creation order on KDE.
- **Recency**: `workspace.stackingOrder` reversed (topmost first), falling back
  to `workspace.windowList()` on builds without the property.
- **Window filter**: `normalWindow && !skipTaskbar && resourceClass != ""`.
  Plasma's panels and desktop are windows KWin refuses to activate, so letting
  one through would make step 5b toggle to something that never takes focus.
- **Restore before focus**: the act script sets `w.minimized = false` before
  assigning `workspace.activeWindow`. Assigning active on a minimized window is
  not documented to restore it, and the X11 backend already taught us not to
  assume a focus request de-iconifies.
- **Script source is generated, so values are escaped** (`js_quote`). Window
  ids are KWin-minted UUIDs and the bus name is a unique name, so neither can
  realistically contain a quote today — but building source by concatenation
  without escaping is how that stops being true.
- **Hot-path cost, measured on the headless VM**: `beckon <id>` 7–41 ms
  (median ~15), `beckon list` 5–6 ms. Comfortably inside the 50 ms budget, and
  cheaper than both macOS (~95–105 ms) and Windows (~57 ms).

Testing: `kwin_wayland --virtual` runs headless with no GPU at all — see
`testing/README.md`. All 19 live tests pass there.

## Hyprland (`hyprland.rs`)

Talks to the compositor via the request socket directly — no `hyprctl`
shell-out, no `hyprland-rs` dep. Two queries (`j/clients`, `j/activewindow`)
per invocation, parsed with `serde_json`. Window identity uses Hyprland's
`class` field, which is set from Wayland `app_id` for native clients and from
`WM_CLASS` for XWayland — one field, no fallback ladder.

- **Socket path**:
  `$XDG_RUNTIME_DIR/hypr/$HYPRLAND_INSTANCE_SIGNATURE/.socket.sock` (Hyprland
  0.40+) with `/tmp/hypr/<sig>/.socket.sock` as fallback for older installs.
  Each request opens a fresh `UnixStream` — Hyprland closes the socket after
  responding.
- **Cycle order (5a)** is the shared address-ordered ring in
  `algorithm::decide`, *not* `focusHistoryID`. This entry used to say "pick the
  same-app window with the lowest non-current `focusHistoryID`", which
  describes code that was deleted: because `focusHistoryID` is real focus
  history, focusing a window promotes it to 0 and demotes the one just left, so
  that ring is a 2-cycle and windows 3..N are unreachable. Verified live on
  Hyprland 0.56.0 — three `foot` windows, six presses, the ring walks all three
  and laps.
- **Hide (5c)**: `dispatch movetoworkspacesilent special:beckon,address:0xN`,
  and **coming back out is beckon's job** — `focus_window` moves a window off
  `special:beckon` before focusing it. This entry used to claim `dispatch
  focuswindow` alone was enough because "Hyprland surfaces the window's
  workspace on focus". Measured on 0.56.0, that is wrong in the way that
  matters: the special workspace is *shown* as an overlay, but the window keeps
  belonging to it, so the moment focus moves elsewhere it disappears and
  `$mod+1..4`, `movefocus` and `movetoworkspace` all behave as if it does not
  exist — only `beckon <id>` could surface it again. sway does not have this
  problem because `focus` on a scratchpad container runs
  `root_scratchpad_show`, which re-parents it onto the workspace the user is
  looking at. The same bug also made the *second* hide a silent no-op
  (`movetoworkspace` early-returns when the window is already there) while
  beckon reported `Hidden`. Only `special:beckon` is unparked; a user's own
  `special:*` workspace is left where they put it.
- **No MRU state file (5b)**: unlike every other Linux backend, this one passes
  `previous_app = None` to `decide`. `$XDG_RUNTIME_DIR/beckon-mru` exists
  because the sway tree carries no focus history; `focusHistoryID` is real MRU
  and — measured on 0.56.0 — reorders on focus changes beckon never made,
  including mouse clicks and native binds. Consulting a file that only records
  beckon's own actions could only make step 5b less accurate.
- **Window filter**: `list_clients` drops clients with an empty `class` and
  those with `hidden = true` (Hyprland sets it on windows it deliberately keeps
  off screen, e.g. terminal swallowing). It must **never** filter on `visible`:
  measured on 0.56.0, a group tab that is not on top reports
  `hidden=false, visible=false`, so filtering there would hide every tab but
  the front one and break step 5a through a tabbed group. Windows parked on
  `special:beckon` stay in the list on purpose — drop them and the next
  keypress launches a duplicate instead of bringing the window back.
- **No `hyprctl` dep**: keeps the hot path at a single short-lived socket
  connection per query, and works in containers/Nix builds where `hyprctl` may
  not be on PATH.

## niri (`niri.rs`)

Talks to the compositor over `NIRI_SOCKET` (exported by niri to children; the
env var is the only source of truth — nested instances are real, so the path
is never derived from `$XDG_RUNTIME_DIR` globbing). One JSON line per request,
one JSON line per reply, wrapper `{"Ok":…}` / `{"Err":"…"}`. Measured on niri
26.04, 2026-08-22.

- **No crate dep**: the official `niri-ipc` types exist and 26.4.0 matches
  this protocol, but it is GPL-3.0-or-later while beckon is MIT OR Apache-2.0
  and ships prebuilt binaries. The used surface is four requests, so framing
  is hand-rolled on serde_json (already a dependency).
- **Unit variants are sent as `null`** (`{"Windows":null}`); `{}` is rejected
  with `{"Err":"error parsing request"}`.
- **`FocusWindow` with an unknown id still answers `{"Ok":"Handled"}`** — a
  silent no-op. The reply is never treated as proof of focus; tests assert
  against server state (`is_focused`), not the reply.
- **No minimize/scratchpad action exists**: all three plausible spellings are
  parse errors. Hide (5c) is therefore
  `MoveWindowToWorkspace { reference: {Index: 1_000_000}, focus: false }` —
  the window leaves the view but stays alive, and a later `FocusWindow`
  navigates to its workspace, so retrieval is self-unparking: unlike Hyprland's
  `special:beckon`, no explicit move-back is needed before focusing.
- **MRU is real**: every window carries `focus_timestamp`, so snapshots are
  sorted newest-first before recency indices are assigned — step 4 and step 5b
  see genuine focus order, which sway cannot offer (tree order). The
  `$XDG_RUNTIME_DIR/beckon-mru` file is still written and read, as in i3ipc:
  mostly redundant here, but it keeps one contract across backends that share
  the file.
- Windows without an `app_id` are skipped, exactly like i3ipc skips windows
  with neither `app_id` nor `WM_CLASS`.

## Live backend tests

`testing/linux_live_test.py` drives the real binary against a real compositor
and asserts on what that compositor reports afterwards. It is the only layer
that can catch what unit tests structurally cannot: `.desktop` resolution
against the machine's own metadata, the class a toolkit actually advertises at
runtime, and whether a focus/minimize request is honoured at all. Every Linux
bug fixed in the 2026-08 pass was found by it, and none were visible to the 65
unit tests that were green the whole time.

It detects its environment the same way `pick_backend` does, so run it inside
the session under test. Hyprland was the last to be brought up — 0.56.2 on
2026-08-15, nested inside a live GNOME session rather than on its own tty,
which costs nothing and leaves the host desktop untouched (recipe in
`testing/README.md`, config in `testing/hypr-nested.conf`).

Two of the three defects that run found were the suite's own, and both looked
like focus bugs: on NixOS `pkill -x <name>` never matches a wrapped binary
(`comm` is `.xterm-wrapped`), so the suite left its own windows behind and step
5c skipped itself while 5b failed expecting a launch it already had.

The other four backends pass on Ubuntu 26.04 arm64 (GNOME Shell 50.1 headless,
sway 1.11, i3 + Xvfb, openbox + Xvfb) — see `testing/README.md` for the
headless bring-up recipes, including the D-Bus service-directory trick that
keeps `gnome-shell --headless` from deadlocking on `xdg-desktop-portal`.

**The suite kills GUI apps to build its preconditions; run it in a VM.**

## Wayland global hotkeys — the survey

`serve` is not offered on Linux. This entry used to read *"Wayland has no
standard global hotkey API […] There is no app-level workaround."* That is not
accurate and was leading sessions to conclude Linux resident mode is
technically impossible. There **is** a standard —
`org.freedesktop.portal.GlobalShortcuts` — plus per-desktop routes that predate
it. Surveyed 2026-08, **from documentation only; none of this has been built or
run against beckon**:

| Environment | Route | State |
|---|---|---|
| X11 (i3, openbox, XFCE, GNOME-X11, KDE-X11) | `XGrabKey` on root — what sxhkd / xbindkeys do; beckon already links `x11rb`, which exposes `grab_key` | available |
| KDE Wayland | KWin script `registerShortcut`, same engine `kde.rs` already drives via `loadScript`; or the portal | available, two routes |
| Hyprland | GlobalShortcuts portal via `xdg-desktop-portal-hyprland` | available |
| GNOME Wayland | the bundled extension could `addKeybinding` / `grab_accelerator`; the portal route is unreliable (Mutter ≥ 49 dropped XWayland-side key grabs) | awkward |
| **sway** | wlroots has not implemented the GlobalShortcuts portal — still under discussion | **no route** |

So "impossible" holds for exactly one compositor, and it is the one where
`bindsym` is easiest. The reasons this stays out of scope are different, and
they are what to re-read before anyone reopens this:

1. **No single API.** macOS is one call, Windows is one call, Linux would be
   four separate implementations — comparable to the cost of the entire
   existing Linux backend layer, for one feature.
2. **The portal model does not carry the shortcuts TOML.** An app asks for a
   shortcut *by name* and the **user** assigns the keys in the compositor's own
   UI — deliberate, per the Wayland security model.
   `"ctrl+super+alt+t" = "kitty"` has nowhere to go, so a Wayland `serve` would
   be a different feature wearing the same name.
3. **Negative value.** Every environment in the table already ships a place to
   bind a key to a command. `serve` exists because macOS and Windows do not.
