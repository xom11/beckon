# macOS via Hammerspoon

[Hammerspoon](https://www.hammerspoon.org/) is the practical default for
global hotkeys on macOS. beckon is a CLI; Hammerspoon binds the keys
and shells out.

## Install

```sh
# beckon
cargo install --git https://github.com/xom11/beckon

# Hammerspoon (Homebrew, or download from hammerspoon.org)
brew install --cask hammerspoon
```

Launch Hammerspoon once and grant it the Accessibility permission it
asks for.

## Wire the bindings

Append the contents of [`init.lua`](init.lua) to `~/.hammerspoon/init.lua`,
then reload Hammerspoon:

- Click the Hammerspoon menu bar icon → **Reload Config**, or
- press `Cmd+Ctrl+R`.

## Grant beckon Accessibility permission too

beckon needs its **own** Accessibility grant — separate from
Hammerspoon's — to cycle between windows of the same app (step 5a).

1. **System Settings → Privacy & Security → Accessibility**
2. Click **+**, navigate to the beckon binary (the path `which beckon`
   prints — typically `~/.cargo/bin/beckon`).
3. Add it. Toggle it on.

Without this, beckon still launches / focuses / hides apps. Only
multi-window cycling on the same app falls back to the toggle-back
path.

> macOS binds the Accessibility grant to the binary's code signature.
> A fresh `cargo build` produces a new unsigned binary with a new
> identity → permission resets and you have to re-add. Production
> users via Nix get a stable `/etc/profiles/per-user/<user>/bin/beckon`
> wrapper path that survives rebuilds.

Run `beckon -d` to confirm the trust state.

## App Names on macOS

Most macOS apps surface their `localizedName` ("Claude", "Brave Browser",
"Cursor"). PWAs installed via Brave/Chrome land in
`~/Applications/<Browser> Apps.localized/<Name>.app` and beckon scans
that subdir too, so PWAs work the same way as native apps.

```sh
beckon -L            # all installed apps
beckon -l            # currently running apps
beckon -r Claude     # validate one id
```

## Troubleshooting

If a hotkey fires but nothing happens, check Hammerspoon's console
(menu bar icon → Console) and look for `beckon: ...` lines. Pass
`-v` for more diagnostics:

```lua
hs.task.new(BECKON, callback, { "-v", name })
```

`beckon -d` reports whether macOS Accessibility is granted, plus
NSWorkspace health.
