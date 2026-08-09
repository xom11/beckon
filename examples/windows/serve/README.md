# Windows resident mode (`--serve` + Scheduled Task)

`beckon --serve <config>` makes beckon host the hotkeys itself — no
AutoHotkey layer. Pick this **or** [`../ahk/`](../ahk/), not both:
`RegisterHotKey` gives a chord to the first registrant, so a second
daemon on the same chord just fails to register.

| | AutoHotkey | `--serve` |
|---|---|---|
| Extra dependency | AutoHotkey v2 | none |
| Config language | AHK script | flat TOML |
| Live reload | reload the script manually | automatic on file save |
| Also does other things | yes, it's a full scripting language | no, hotkeys only |

Registration uses `RegisterHotKey` — not a low-level keyboard hook — so
it doesn't sit in the input path of every keystroke and doesn't compete
with LLHOOK-based remappers for hook ordering.

## Install

```powershell
# 1. beckon
scoop bucket add xom11 https://github.com/xom11/scoop-bucket
scoop install xom11/beckon

# 2. your shortcuts file
mkdir -Force "$env:USERPROFILE\.config\beckon"
copy apps.toml "$env:USERPROFILE\.config\beckon\apps.toml"
notepad "$env:USERPROFILE\.config\beckon\apps.toml"

# 3. validate it — exit code 0 means every combo parsed
beckon --check "$env:USERPROFILE\.config\beckon\apps.toml"
```

Run it in the foreground first and press a hotkey:

```powershell
beckon --serve "$env:USERPROFILE\.config\beckon\apps.toml"
# beckon serve: 5 shortcuts registered from C:\Users\you\.config\beckon\apps.toml
```

`Ctrl+C` when you're satisfied, then install the task.

## Run at logon via Scheduled Task

`--serve` is a foreground process, not a Windows service. It **must**
run inside your interactive desktop session — `RegisterHotKey` needs a
desktop to bind to, so a task configured to "run whether user is logged
on or not" registers nothing and silently never fires.

```powershell
$exe = "$env:USERPROFILE\.cargo\bin\beckon.exe"   # or (Get-Command beckon).Source
$cfg = "$env:USERPROFILE\.config\beckon\apps.toml"

(Get-Content -Raw beckon-serve.xml).
    Replace('C:\Users\YOUR_USERNAME\.cargo\bin\beckon.exe', $exe).
    Replace('C:\Users\YOUR_USERNAME\.config\beckon\apps.toml', $cfg).
    Replace('YOUR_USERNAME', "$env:USERDOMAIN\$env:USERNAME") |
  ForEach-Object { Register-ScheduledTask -TaskName "beckon-serve" -Xml $_ -Force }
```

Verify, and start it without logging out:

```powershell
Get-ScheduledTask beckon-serve | Get-ScheduledTaskInfo
Start-ScheduledTask beckon-serve
```

Remove it again:

```powershell
Unregister-ScheduledTask beckon-serve -Confirm:$false
```

## The console window

beckon is a console application, so the task above leaves an **empty
console window** on screen for as long as the daemon runs. Three ways
out, in order of how much they depend on:

1. **Live with it** — minimize it. Nothing breaks.
2. **[`beckon-serve.vbs`](beckon-serve.vbs)** — a three-line shim that
   starts the exe hidden, exactly like the AHK example's
   `Run(..., "Hide")`. Point the task at
   `wscript.exe "...\beckon-serve.vbs"` (the alternative `<Exec>` block
   is already in the XML, commented out). Caveat: VBScript is a
   deprecated Windows feature-on-demand — present by default today, not
   forever.
3. **`conhost.exe --headless beckon.exe --serve <config>`** — no
   deprecated dependency, but an undocumented flag.

None of these are verified on Windows by the maintainer of this file;
option 1 is the one that cannot fail.

## The tray icon

`--serve` puts an icon in the notification area. It is a **one-way**
liveness signal:

- icon present → the daemon is alive
- icon absent → the daemon is dead **or** the tray just isn't ready
  (logon race, or Explorer restarting)

Hotkeys register and fire independently of the icon, so don't diagnose
from the icon alone — read the log.

## Editing shortcuts

Just save the file. beckon watches the **parent directory** (so editors
that write-then-rename don't break the watch) and reloads on a 1 Hz
tick.

A broken edit does **not** cost you your working keys: the parse
failure is logged and toasted, and the previous bindings stay
registered until the file parses again.

## Troubleshooting

**Read the registration count, not the shortcut count.** Startup and
reload report how many keys *actually registered*:

```
beckon serve: 5 shortcuts registered from ...          # all good
beckon serve: 3 of 5 shortcuts registered (2 failed)   # two chords lost
```

The second form means something else already owns those chords. A
failure wave is collapsed into a single toast listing up to 5 combos
rather than one toast per key, so check stderr for the full per-key
detail.

**"another `beckon --serve` is already running for this config"** — one
instance per config path, enforced with a lock file. Look for a leftover
process:

```powershell
Get-Process beckon -ErrorAction SilentlyContinue | Format-Table Id, Path
```

**Nothing fires and there's no window to read.** Under a Scheduled Task
stderr goes nowhere. Redirect it by pointing the task at a shim that
appends to a file, or just run `--serve` from a terminal to reproduce.

**A Name doesn't resolve.** `beckon -r "Windows Terminal"` shows the
match type. Prefer the exact friendly name from `beckon -L`; `Explorer`
is ambiguous, use `File Explorer`.
