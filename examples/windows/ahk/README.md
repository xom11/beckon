# Windows via AutoHotkey v2

Windows has no first-class global-hotkey API for end users, so
[AutoHotkey v2](https://www.autohotkey.com/) is the canonical layer.
beckon is a CLI; AHK binds the keys and shells out.

## Install

1. **AutoHotkey v2**: download the installer from
   <https://www.autohotkey.com/> and run it. Pick "Express install".
2. **beckon**:
   ```cmd
   cargo install --git https://github.com/xom11/beckon
   ```
   That puts `beckon.exe` at `%USERPROFILE%\.cargo\bin\beckon.exe`.

## Wire the bindings

Save [`beckon.ahk`](beckon.ahk) anywhere (e.g. `Documents\beckon.ahk`)
and double-click it to start. The system tray gets a green H icon.

To make it run on login, drop a shortcut in the Startup folder:

1. Press `Win+R`, type `shell:startup`, hit Enter.
2. Right-click → **New → Shortcut**.
3. Target: the path to `beckon.ahk` (Windows associates `.ahk`
   files with AutoHotkey automatically).

## App Names on Windows

beckon resolves Names against Start Menu `.lnk` shortcuts and registered
shell/MSIX/AppX apps. It uses friendly Start Menu names and AppUserModelIDs
(AUMIDs) for identity; packaged apps activate through AUMID, while File
Explorer launches through `explorer.exe`.

```cmd
beckon installed       list installed desktop and MSIX/AppX apps
beckon list            list currently running apps
beckon resolve Spotify validate an id
```

For example, Windows Terminal is commonly exposed as `Terminal`; verify the
local friendly name with `beckon installed | findstr /i terminal` before
binding it. `Settings` and `File Explorer` are also supported. Use the exact
name `File Explorer`, because `Explorer` can match another shortcut whose
target is `explorer.exe` (for example a cloud-storage promotion shortcut).

```ahk
^#!,:: Beckon("Settings")
^#!f:: Beckon("File Explorer")
```

## Anti-focus-stealing

Windows 10+ blocks `SetForegroundWindow` from background processes.
beckon handles this with the standard `AttachThreadInput` trick: it
attaches to the foreground thread before raising. AHK is the
foreground process when it invokes beckon (the user just pressed a
key), so the trick succeeds.

## Troubleshooting

```cmd
beckon doctor        check environment
beckon list          see what beckon enumerates
```

If a hotkey runs (the AHK tray icon flashes) but nothing happens:

- Pass `-v` in the AHK call to see verbose stderr:
  ```ahk
  Beckon(name) {
      try RunWait('"' BeckonExe '" -v "' name '"')
  }
  ```
- Check the Windows event log for any process that crashed.
- Confirm the Name resolves: `beckon resolve "<Name>"`.

beckon also fires a Windows toast notification on errors (best-effort
via PowerShell), so silent failures still surface.
