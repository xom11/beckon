# macOS resident mode (`--serve` + launchd)

`beckon --serve <config>` makes beckon host the hotkeys itself — no
Hammerspoon layer. Pick this **or** [`../hammerspoon/`](../hammerspoon/),
not both: two daemons registering the same chord means the second one
loses the registration.

| | Hammerspoon | `--serve` |
|---|---|---|
| Extra dependency | Hammerspoon | none |
| Config language | Lua | flat TOML |
| Live reload | reload config manually | automatic on file save |
| Also does other things | yes, it's a full automation tool | no, hotkeys only |

Registration uses Carbon `RegisterEventHotKey` — no event tap, no input
monitoring, so it triggers **no new TCC prompt** and does not interfere
with kanata-style key remappers.

## Install

```sh
# 1. beckon
cargo install --git https://github.com/xom11/beckon

# 2. your shortcuts file
mkdir -p ~/.config/beckon
cp apps.toml ~/.config/beckon/apps.toml
$EDITOR ~/.config/beckon/apps.toml

# 3. validate it — exit 0 means every combo parsed
beckon --check ~/.config/beckon/apps.toml
```

Run it in the foreground first to confirm the keys actually fire:

```sh
beckon --serve ~/.config/beckon/apps.toml
# beckon serve: 5 shortcuts registered from /Users/you/.config/beckon/apps.toml
```

Press a hotkey. If it works, `Ctrl+C` and move on to launchd.

## Load at login via `brew services` (Homebrew installs)

If you installed with `brew install xom11/tap/beckon`, the formula ships
the LaunchAgent already:

```sh
brew services start beckon
brew services list                       # beckon should be `started`
tail -f "$(brew --prefix)/var/log/beckon.log"
```

It reads `~/.config/beckon/apps.toml`. Create and `beckon --check` it
first — `keep_alive` restarts a serve that cannot read its config every
~10 seconds, forever.

Two ways this ships broken, both silent:

- **`sudo brew services start`** installs a LaunchDaemon instead of a
  per-user LaunchAgent. A daemon has no window-server session, so
  `RegisterEventHotKey` returns success and no key ever fires.
- **Starting it over SSH** can drop the agent out of the `gui/<uid>`
  domain for the same reason. Start it from a terminal in the desktop
  session.

Confirm you got the right domain:

```sh
launchctl print gui/$(id -u)/homebrew.mxcl.beckon | head -20
```

The hand-written plist below is the fallback for non-Homebrew installs,
and still the reference for what the agent actually does.

## Load at login via launchd (manual install)

```sh
mkdir -p ~/Library/LaunchAgents
sed "s|/Users/YOUR_USERNAME|$HOME|g" com.github.xom11.beckon.plist \
    > ~/Library/LaunchAgents/com.github.xom11.beckon.plist

launchctl bootstrap gui/$(id -u) ~/Library/LaunchAgents/com.github.xom11.beckon.plist
```

Verify and watch the log:

```sh
launchctl print gui/$(id -u)/com.github.xom11.beckon | head -20
tail -f /tmp/beckon-serve.log
```

Reload after editing the plist (editing `apps.toml` needs none — it
reloads itself):

```sh
launchctl bootout  gui/$(id -u)/com.github.xom11.beckon
launchctl bootstrap gui/$(id -u) ~/Library/LaunchAgents/com.github.xom11.beckon.plist
```

> A launchd-spawned process has no window-server application identity,
> and without one `RegisterEventHotKey` returns success while silently
> never delivering a press. beckon handles this by transforming itself
> into a UIElement app at startup (no Dock icon, no menu bar). Nothing
> for you to configure — but it's why `--serve` under launchd works at
> all, and why the log line to trust is the registration count, not the
> absence of errors.

## Grant Accessibility permission

Same requirement as the Hammerspoon path, and for the same reason:
cycling between windows of one app (step 5a) goes through the
Accessibility API.

1. **System Settings → Privacy & Security → Accessibility**
2. **+** → the binary `which beckon` prints (typically
   `~/.cargo/bin/beckon`) → toggle on.

Without it, launch / focus / toggle / hide still work; only same-app
window cycling degrades. `beckon -d` reports the trust state.

> macOS binds the grant to the binary's code signature, so a fresh
> `cargo build` invalidates it. Nix users get a stable
> `/etc/profiles/per-user/<user>/bin/beckon` wrapper that survives
> rebuilds — point the plist at that path instead.

## Editing shortcuts

Just save the file. beckon watches the **parent directory** (so
editors that write-then-rename don't break the watch) and reloads on a
1 Hz tick.

A broken edit does **not** cost you your working keys: the parse
failure is logged and notified, and the previous bindings stay
registered until the file parses again.

```sh
# safe loop while experimenting
beckon --check ~/.config/beckon/apps.toml && tail -f /tmp/beckon-serve.log
```

## Troubleshooting

**Read the registration count, not the shortcut count.** The startup
and reload lines report how many keys *actually registered*:

```
beckon serve: 5 shortcuts registered from ...          # all good
beckon serve: 3 of 5 shortcuts registered (2 failed)   # two chords lost
```

The second form means another app already owns those chords — macOS
gives a hotkey to the first registrant. Check System Settings →
Keyboard → Keyboard Shortcuts, and any other hotkey daemon you have
running (including a stale `beckon --serve`).

**"another `beckon --serve` is already running for `...`"** — one
instance per config path is enforced with a lock file. Find the other
one with `pgrep -fl "beckon --serve"`.

**Nothing fires at all, but the log says everything registered** — the
UIElement transform likely failed; the log has
`hotkey: TransformProcessType failed`. Run `--serve` from a terminal to
confirm the config itself is fine.
