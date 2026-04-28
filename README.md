# beckon

Cross-platform focus-or-launch app switcher. A thin CLI invoked by your existing
hotkey dotfile (sway, i3, Hammerspoon, AHK).

```
press hotkey
  └── if app not running   → launch it
      if running, unfocused → focus it
      if already focused    → cycle windows / toggle to previous app / hide
```

## Status

| Platform | Status |
|----------|--------|
| Linux / sway (Wayland) | ✅ Phase 1a |
| Linux / i3 (X11) | ✅ Phase 1b — same backend (shared protocol) |
| Linux / X11 generic (GNOME-X11, KDE-X11, ...) | ⏳ Deferred |
| Linux / Hyprland | ⏳ Pending |
| macOS | ✅ Phase 2 — NSWorkspace + AX + CGWindowList |
| Windows | ✅ Phase 3 — Win32 EnumWindows + COM IShellLinkW |
| GNOME / KDE Wayland | ❌ Out of scope (compositor blocks external focus) |

## Build

### Cargo

```sh
cargo build --release
# binary: ./target/release/beckon
```

Requirements: Rust 1.75+. On Linux: a sway or i3 session (other compositors
TBD — `beckon -d` will tell you). On Windows: VS Build Tools 2022 with the
C++ ARM64/x64 component and Windows SDK.

### cargo install (from GitHub)

```sh
cargo install --git https://github.com/xom11/beckon
# update to latest:
cargo install --git https://github.com/xom11/beckon --force
```

Binary lands in `~/.cargo/bin/beckon` (already in PATH).

### Nix flake

```sh
nix run github:xom11/beckon -- -l
nix build .#beckon          # binary at ./result/bin/beckon
nix develop                 # dev shell with rustfmt / clippy / rust-analyzer
```

To pull beckon into your own flake, add the overlay:

```nix
{
  inputs.beckon.url = "github:xom11/beckon";

  outputs = { nixpkgs, beckon, ... }: {
    # ...
    nixpkgs.overlays = [ beckon.overlays.default ];
    # then `pkgs.beckon` resolves
  };
}
```

## Usage

The hot path is `beckon <id>` — invoke from a hotkey binding:

```sh
beckon Claude            # focus / launch / cycle Claude
```

`<id>` resolves against installed-app metadata. Priority per OS:

**Linux** (`.desktop` files in `$XDG_DATA_DIRS/applications/`):

1. `Name=` exact (case-insensitive, normalized) — **recommended for dotfiles**
2. `.desktop` filename
3. `StartupWMClass=`
4. `Name=` substring (alphabetical first wins, like rofi)

**macOS** (`NSWorkspace.runningApplications` + scan of `/Applications`,
`/System/Applications`, `~/Applications` — including one level into
non-.app subdirs to catch `~/Applications/{Brave,Chrome,Vivaldi}
Apps.localized/*.app`):

1. Running app — `localizedName` exact (case-insensitive, normalized)
2. Running app — `bundleIdentifier`
3. Installed app — `CFBundleDisplayName`/`CFBundleName` exact
4. Installed app — `CFBundleIdentifier`
5. Installed app — name substring (alphabetical first wins)

**Windows** (Start Menu `.lnk` shortcuts in `%APPDATA%\...\Start Menu\Programs\`
and `%ProgramData%\...\Start Menu\Programs\`, parsed via COM `IShellLinkW`):

1. Shortcut display name exact (case-insensitive, normalized)
2. Exe filename stem (e.g. `brave` matches `brave.exe`)
3. Shortcut display name substring (alphabetical first wins)

When the resolved exe is a launcher stub (e.g. Brave PWA `chrome_proxy.exe` →
`brave.exe`), beckon falls back to title matching against running windows.

Names are stable across machines. Brave PWA hashes are not — bind to `Claude`,
not `brave-fmpnliohj...-Default` or `com.vivaldi.Vivaldi.app.<hash>`.

### Discovery

```sh
beckon -l           # list running apps with their app_ids
beckon -L           # list installed apps (parsed from .desktop)
beckon -s claude    # fuzzy-search ids matching "claude"
beckon -r Claude    # show how an id resolves (match type, exec, status)
beckon -d           # check environment (compositor / IPC / notification daemon)
```

### Sway dotfile example

```
set $focus exec /path/to/beckon

bindsym $mod+space $focus kitty
bindsym $mod+c     $focus Claude
bindsym $mod+g     $focus Gemini
bindsym $mod+t     $focus "Telegram Web"
```

If `beckon -L` shows two apps with the same Name (e.g. native Telegram +
Telegram Web PWA), bind to the more specific Name to disambiguate.

### Hammerspoon (macOS) dotfile example

```lua
local hyper = { "cmd", "ctrl", "alt" }

-- Use hs.task with the absolute path. Do NOT use `hs.execute(cmd, true)` —
-- the `true` flag sources the user login shell (~/.zshrc) before each
-- invocation, which can run several hundred ms to several seconds and
-- swamps the actual focus latency.
local function beckon(name)
  hs.task.new("/etc/profiles/per-user/" .. os.getenv("USER") .. "/bin/beckon",
    function(exitCode, _, stderr)
      if exitCode ~= 0 then
        hs.alert.show("beckon " .. name .. ": " .. (stderr or ""), 3)
      end
    end, { name }):start()
end

hs.hotkey.bind(hyper, "space", function() beckon("kitty") end)
hs.hotkey.bind(hyper, "c",     function() beckon("Claude") end)
hs.hotkey.bind(hyper, "d",     function() beckon("Discord") end)
```

### AutoHotkey (Windows) dotfile example

```ahk
#Requires AutoHotkey v2.0

BeckonExe := A_UserProfile . "\.cargo\bin\beckon.exe"

Beckon(name) {
    try RunWait('"' BeckonExe '" "' name '"', , "Hide")
}

^#!c:: Beckon("Claude")
^#!d:: Beckon("Discord")
^#!b:: Beckon("Vivaldi")
^#!Space:: Beckon("windowsterminal.exe")
```

Installed PWAs (via Brave/Chrome "Install as app") get their own Start Menu
shortcut, so `Beckon("Claude")` resolves and launches correctly. The
`AttachThreadInput` trick handles Win10+ anti-focus-stealing since the AHK
process holds foreground when it invokes beckon.

beckon needs **Accessibility permission** to cycle between windows of the
same app (step 5a). Grant in System Settings → Privacy & Security →
Accessibility, adding the binary path that Hammerspoon invokes (typically
the Nix profile path above). Without it, beckon still focuses / launches /
hides — only the cycle step degrades to "toggle to other app". Run
`beckon -d` to check trust state.

## What `beckon <id>` actually does

Single algorithm, not configurable:

```
1. resolve id → app metadata (.desktop / Info.plist / .lnk)
2. if no window of this app  → launch
3. if running but unfocused  → focus first window
4. if focused, more windows  → cycle to next window of same app
5. if focused, sole window   → toggle to the previously focused app
6. if nothing else exists    → hide / minimize
```

When a hotkey-bound invocation fails (id not found, IPC error), beckon fires
a desktop notification (`notify-send` on Linux, toast on Windows). Run from a
terminal to see errors on stderr instead.

## Project layout

```
crates/
├── beckon-core/      # Backend trait, shared types
├── beckon-linux/     # sway + i3 (i3-IPC), .desktop parser, MRU state
├── beckon-macos/     # NSWorkspace + AX (cycle) + CGWindowList (z-order)
├── beckon-windows/   # Win32 EnumWindows + COM IShellLinkW (.lnk parsing)
└── beckon-cli/       # binary, clap CLI, doctor / search / resolve
```

See [`CLAUDE.md`](./CLAUDE.md) for the full design rationale.

## Testing on i3 without leaving sway

```sh
./test-i3-env.sh start    # Xwayland :3 → Xephyr :2 → i3
./test-i3-env.sh xterm    # spawn xterm in :2 to play with
./test-i3-env.sh stop     # tear down
```

Then inside the i3 sandbox:

```sh
env -u SWAYSOCK -u WAYLAND_DISPLAY \
    I3SOCK=$(ls /run/user/1000/i3/ipc-socket.* | head -1) DISPLAY=:2 \
    ./target/release/beckon -l
```

## Out of scope

No config file. No alias mapping. No global hotkey registration — that lives
in your existing dotfile. No interactive launcher (use rofi for that).

## License

MIT OR Apache-2.0
