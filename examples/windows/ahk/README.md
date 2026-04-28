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

beckon resolves Names against Start Menu `.lnk` shortcuts in
`%APPDATA%\...\Start Menu\Programs\` and
`%ProgramData%\...\Start Menu\Programs\`. The shortcut's display
name (the text under the icon, not the filename) is the canonical
Name.

```cmd
beckon -L            list installed shortcuts
beckon -l            list currently running apps
beckon -r Claude     validate an id
```

> **Microsoft Store apps**: apps installed via the Microsoft Store
> (Windows Terminal, Calculator, …) sometimes have no file-system
> `.lnk`. They show up in `beckon -l` once running and can be
> focused/cycled, but launching by Name from cold may not work.
> The example uses `Windows Terminal` because it ships with a Start
> Menu shortcut on most installs.

## Anti-focus-stealing

Windows 10+ blocks `SetForegroundWindow` from background processes.
beckon handles this with the standard `AttachThreadInput` trick: it
attaches to the foreground thread before raising. AHK is the
foreground process when it invokes beckon (the user just pressed a
key), so the trick succeeds.

## Troubleshooting

```cmd
beckon -d            check environment
beckon -l            see what beckon enumerates
```

If a hotkey runs (the AHK tray icon flashes) but nothing happens:

- Pass `-v` in the AHK call to see verbose stderr:
  ```ahk
  Beckon(name) {
      try RunWait('"' BeckonExe '" -v "' name '"')
  }
  ```
- Check the Windows event log for any process that crashed.
- Confirm the Name resolves: `beckon -r "<Name>"`.

beckon also fires a Windows toast notification on errors (best-effort
via PowerShell), so silent failures still surface.
