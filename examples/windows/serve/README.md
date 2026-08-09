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
$exe = (Get-Command beckon).Source
$cfg = "$env:USERPROFILE\.config\beckon\apps.toml"
$log = "$env:USERPROFILE\AppData\Local\beckon\serve.log"
$sid = ([Security.Principal.WindowsIdentity]::GetCurrent()).User.Value

(Get-Content -Raw beckon-serve.xml).
    Replace('C:\Users\YOUR_USERNAME\.cargo\bin\beckon.exe', $exe).
    Replace('C:\Users\YOUR_USERNAME\.config\beckon\apps.toml', $cfg).
    Replace('C:\Users\YOUR_USERNAME\AppData\Local\beckon\serve.log', $log).
    Replace('YOUR_USER_SID', $sid) |
  ForEach-Object { Register-ScheduledTask -TaskName "beckon-serve" -Xml $_ -Force }
```

Two things in there look like overkill and are not:

- **The principal is a SID, not `DOMAIN\user`.** On a machine that isn't
  domain-joined `$env:USERDOMAIN` is `WORKGROUP`, and registering
  `WORKGROUP\you` fails with *"No mapping between account names and
  security IDs was done."*
- **`beckon-serve.xml` declares `encoding="UTF-16"` even though the file
  is UTF-8.** Change it to UTF-8 and Task Scheduler rejects the import
  with *"The task XML is malformed. (1,40)::ERROR: unable to switch the
  encoding."* Every task Windows exports declares UTF-16 for the same
  reason.

Both were hit and fixed against a real machine (Windows 11 ARM64, build
26200); the file as shipped registers cleanly.

Verify, and start it without logging out:

```powershell
Get-ScheduledTask beckon-serve | Get-ScheduledTaskInfo
Start-ScheduledTask beckon-serve
```

Remove it again:

```powershell
Unregister-ScheduledTask beckon-serve -Confirm:$false
```

## The log

The task action passes `--log`. Task Scheduler throws a process's stderr
away, and stderr is the only place beckon reports **how many hotkeys
actually registered**. Without the log you cannot tell `20 shortcuts
registered` from `20 parsed, 0 registered` — which is exactly the failure
that went unnoticed for hours on 2026-08-09.

beckon creates the log's parent directory itself, and **appends** rather
than truncating. The `cmd.exe` redirect this replaces truncated on every
start, so `RestartOnFailure` destroyed the log explaining the failure it
was restarting from.

It is bounded: past **5 MiB** the file is renamed to `serve.log.1` and a
fresh one starts, so the pair never exceeds 10 MiB. Exactly one generation
is kept — the next roll overwrites `.1`. The check runs when the log is
opened, which needs no timer and lands where the growth is by itself: the
daemon opens its log once per logon and writes a couple of lines per boot,
while a 5-minute watchdog opens *its* log 288 times a day and is the only
writer producing a line on a schedule (~55 KB/day, measured).

Read it with `Get-Content -Tail`. beckon keeps its `--serve` messages
ASCII, so the default `Get-Content` encoding of Windows PowerShell 5.1
renders them correctly without `-Encoding utf8`.

```powershell
Get-Content "$env:USERPROFILE\AppData\Local\beckon\serve.log" -Tail 20 -Wait
```

## The console window

There isn't one. `--log` sends stderr to the file and calls
`FreeConsole()` in the same step, so the task runs `beckon.exe` directly
and the console Windows allocates for it closes immediately.

Earlier versions needed two extra hops for this — `cmd.exe` for the `2>`
redirect, then a `wscript.exe` VBScript shim to hide the window `cmd.exe`
left behind. VBScript is a deprecated feature-on-demand, so that install
was on a clock. Both are gone.

What remains is a **brief flash**: Task Scheduler has no way to start a
console-subsystem process without allocating a console first, and
`<Hidden>` in the task XML hides the task from the Task Scheduler UI, not
the window. `FreeConsole()` can only close the console *after* `main`
starts, never prevent it.

Measured on Windows 11 ARM64 (build 26200), from inside session 1, 25 ms
sampling, with a control:

| action | windows |
|---|---|
| `beckon.exe --serve <cfg>` (no `--log`) | console + `PseudoConsoleWindow`, **both persist for the life of the daemon** |
| `beckon.exe --serve <cfg> --log <log>` | one window at ~150 ms, **gone by ~210 ms** — a ~60 ms flash, nothing lingers |
| `conhost.exe --headless beckon.exe --serve <cfg> --log <log>` | **none at all** |

So `--log` is enough to stop anything *staying* on screen, and that is what
the shipped XML does. If even the flash is unacceptable, put
`conhost.exe --headless` in front of the same command — `--log` still
works, and nothing appears at any point. `--headless` is undocumented, so
treat it as a local preference rather than the default.

**The flash is worse than it sounds if Windows Terminal is your default
terminal.** The console then arrives as a *new tab in your existing WT
window* rather than a standalone window, so a long-lived one looks exactly
like a tab you opened yourself — and closing it sends `CTRL_CLOSE_EVENT`
and kills the daemon.

**Two caveats about what you point the action at.** The command must be
the real `beckon.exe`, not a wrapper that stays alive: a launcher which
remains as a live parent (a Scoop shim, `cmd /c`) holds the console open,
so beckon detaching does not close it. And a whole-binary
`windows_subsystem = "windows"` is **not** an option — it would silently
swallow the output of `beckon -l`, `-L`, `-s`, `-r` and `-d`. A separate
GUI-subsystem `beckon-serve.exe` is the escalation if one is ever needed.

## The tray icon

`--serve` puts an icon in the notification area. It is a **one-way**
liveness signal:

- icon present → the daemon is alive
- icon absent → the daemon is dead **or** the tray just isn't ready
  (logon race, or Explorer restarting)

Hotkeys register and fire independently of the icon, so don't diagnose
from the icon alone — read the log.

## Optional: a watchdog that costs nothing

A logon trigger fires once. If the daemon dies mid-session — crash,
`taskkill`, a botched upgrade — nothing brings it back until you log out
and in again. `<RestartOnFailure>` only covers what the task engine
itself considers a failure, which is not the same thing.

A second task that simply *tries* to start beckon every 5 minutes fixes
this with no extra tooling, because `--serve` already takes a per-config
lock: when one is healthy the redundant instance exits immediately with

```
beckon: another `beckon --serve` is already running for `...apps.windows.toml` (lock `...`)
```

Register the same XML under a second name, replacing the `<LogonTrigger>`
block with:

```xml
<TimeTrigger>
  <StartBoundary>2020-01-01T00:00:00</StartBoundary>
  <Repetition>
    <Interval>PT5M</Interval>
  </Repetition>
</TimeTrigger>
```

Point its `--log` at a *different* file. The daemon and the watchdog both
append, so a shared file still works — but it interleaves an "already
running" line into the daemon's log every 5 minutes, burying the
registration count you actually want to read.

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

**"another `beckon --serve` is already running for `...`"** — one
instance per config path, enforced with a lock file. Look for a leftover
process:

```powershell
Get-Process beckon -ErrorAction SilentlyContinue | Format-Table Id, Path
```

**Nothing fires and there's no window to read.** Read the log — that is
what `--log` in the task action is for. If the log is empty,
the task never started: check `Get-ScheduledTaskInfo beckon-serve`
(`LastTaskResult` 267009 = `SCHED_S_TASK_RUNNING`, which is healthy).

**A Name doesn't resolve.** `beckon -r "Windows Terminal"` shows the
match type. Prefer the exact friendly name from `beckon -L`; `Explorer`
is ambiguous, use `File Explorer`.
