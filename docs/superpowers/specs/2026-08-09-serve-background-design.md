# `--serve` background running — design

Status: proposed (2026-08-09)
Owner: xom11

## Goal

Remove the two rough edges in how `beckon --serve` gets to run in the
background, without adding a daemon to beckon itself:

1. **Windows depends on a deprecated Windows feature.** The Scheduled Task
   runs `wscript.exe beckon-serve.vbs`, and that VBScript shim exists solely
   to hide the console window a task-launched console process leaves on the
   desktop. VBScript is a deprecated feature-on-demand; the shim's own
   comment says so. When Microsoft finishes removing it, resident mode on
   Windows stops starting at logon.
2. **macOS install is a hand-run `sed` + `launchctl bootstrap`.** Homebrew
   already has a formula DSL (`service do`) that generates and manages the
   LaunchAgent, and beckon already ships via a Homebrew tap.

## Non-goals

- **Self-daemonizing** (`fork`/`setsid`, `DETACHED_PROCESS`, PID files).
  Explicitly rejected — see "Why not daemonize" below.
- **`beckon --service install/uninstall/start/stop/status`** (the
  skhd/yabai/espanso pattern). A reasonable future step, but it is new code
  with its own failure surface (path escaping, Windows SIDs, clean
  uninstall). It waits until the Scheduled Task is proven to work with no
  shim at all.
- **Log rotation.** `--log` appends and does not rotate.
- **A GUI-subsystem Windows binary.** Held in reserve as the escalation path
  if the console flash (below) measures badly.
- **Linux.** `--serve` does not exist there; the compositor owns the keybind.

## Why not daemonize

Surveyed comparable tools: skhd, yabai, espanso, kanata, AutoHotkey, caddy.
Effectively none of the hotkey daemons fork; they either delegate entirely
(kanata, AHK — beckon's current position) or add service-management
subcommands that generate the OS unit (skhd, yabai, espanso). The reasons
are technical, not stylistic:

- **macOS.** A forked, detached process loses the login session's bootstrap
  namespace. beckon already has to call `TransformProcessType(→ UIElement)`
  (`crates/beckon-macos/src/hotkey.rs:159`) because a launchd-spawned process
  has no window-server identity, and without one `RegisterEventHotKey`
  returns success while silently never delivering a press. A self-forked
  process detached from a terminal is *more* exposed to that failure, not
  less.
- **Windows.** There is no `fork`. "Daemonize" reduces to re-execing yourself
  with `DETACHED_PROCESS`/`CREATE_NO_WINDOW` — which is what the VBS shim
  already does, one layer out.
- **It solves the wrong problem.** Forking buys "survives closing the
  terminal". What users actually need is "starts at login" and "restarts if
  it dies", and only launchd / Task Scheduler provide those. After forking
  you still install a LaunchAgent, so the fork earns nothing.

## Constraints

- `--serve` must stay a foreground process. Both supervisors require it, and
  the Windows `RegisterHotKey` path additionally requires an interactive
  desktop session (`crates/beckon-windows/src/hotkey.rs:195` warns on
  session 0).
- The notification policy must not change. `notify::decide` branches on
  `stderr_is_terminal`; under both the old (`cmd /c … 2> log`) and new
  (`--log`) arrangements stderr is a file, so the verdict is identical.
- The Homebrew formula template lives in this repo
  (`packaging/homebrew/beckon.rb.template`). `.github/workflows/bump-packagers.yml`
  only substitutes `{{VERSION}}` and the four sha256 placeholders before
  pushing to `xom11/homebrew-tap`, so anything else added to the template
  propagates automatically and survives every future bump.

## Architecture

Four independent changes. Nothing touches the `beckon <id>` hot path, the
focus algorithm, or any backend.

### 1. `--log <PATH>` (Windows only)

```rust
#[cfg(windows)]
#[arg(long, value_name = "PATH", requires = "serve")]
log: Option<std::path::PathBuf>,
```

Applied at the top of `cmd_serve`, before `acquire_lock`: `create_dir_all`
the parent directory, open the file with `.append(true).create(true)`, then
`SetStdHandle` it over `STD_OUTPUT_HANDLE` and `STD_ERROR_HANDLE`. Doing it
before the lock means the "already running" refusal is logged too.

Creating the parent directory is not a convenience — it removes the most
likely way the redirect fails (see "Error handling"), and it deletes the
`mkdir -Force (Split-Path $log)` step from the Windows README.

This works because `std`'s Windows `Stderr` is a zero-sized type that calls
`GetStdHandle` on **every** write rather than caching a handle at startup.
So every later `eprintln!` — including those inside `serve.rs`, the hotkey
manager, and the Windows backend — lands in the file with no plumbing and no
change to any call site.

**Windows only, on purpose.** macOS gets the same result from the plist's
`StandardErrorPath`. A Unix implementation needs `dup2`, i.e. a new `libc`
dependency in `beckon-cli`, in exchange for nothing.

**Append, not truncate — a deliberate behavior change.** `cmd /c … 2> log`
truncates on every start, so under `RestartOnFailure` the log describing why
the daemon died is destroyed by the restart that follows it. Appending keeps
it. Unbounded in principle; in practice the worst case is a supervisor
retrying a broken config, which writes a few lines per restart attempt and
is capped at 3 attempts by the Scheduled Task.

### 2. `FreeConsole()` on the serve path (Windows only)

Called immediately **after** the `SetStdHandle` redirect above. Ordering is
load-bearing: `FreeConsole` invalidates the console handles, but the log
file handle is unaffected, so redirect-then-detach keeps stderr working
while detach-then-redirect would briefly have nowhere to write.

With the `cmd.exe` wrapper gone, beckon is the only process attached to the
console Windows allocated for it, so detaching destroys the console and its
window closes.

**Known trade-off: a possible ~10–50 ms window flash at logon.** Task
Scheduler has no way to launch a console-subsystem process without allocating
a console — the `<Hidden>` element in the task XML hides the *task* from the
Task Scheduler UI listing, not the window. This must be measured on real
hardware. If the flash is unacceptable, escalate to a separate
GUI-subsystem `beckon-serve.exe` (`#![windows_subsystem = "windows"]`),
accepting one more artifact per Windows release.

A whole-binary `#![windows_subsystem = "windows"]` is **not** an option:
it would silently break `beckon -l` / `-L` / `-s` / `-r` / `-d`, which exist
to print to the terminal.

### 3. Scheduled Task reduced to one action

`examples/windows/serve/beckon-serve.xml` gets a single `<Exec>`:

```
<Command>C:\...\beckon.exe</Command>
<Arguments>--serve "C:\...\apps.toml" --log "C:\...\serve.log"</Arguments>
```

No `cmd.exe`, no `wscript.exe`. `beckon-serve.vbs` is deleted.
`LogonTrigger`, `RestartOnFailure` (PT1M × 3), `MultipleInstancesPolicy`,
and the session-0 warning in the README all stay exactly as they are.

### 4. `service do` in the Homebrew formula template

Added to `packaging/homebrew/beckon.rb.template`, inside `on_macos do` —
`--serve` does not exist on Linux, and `brew services start beckon` there
must mean "this formula has no service", not "a service that dies on
startup".

```ruby
service do
  run [opt_bin/"beckon", "--serve", "#{Dir.home}/.config/beckon/apps.toml"]
  keep_alive true
  process_type :interactive
  log_path "#{var}/log/beckon.log"
  error_log_path "#{var}/log/beckon.log"
end
```

`keep_alive` and `process_type :interactive` mirror the hand-written plist in
`examples/macos/serve/`, which stays as the reference for people who did not
install via Homebrew.

## Data flow

Unchanged from today except for where stderr points:

```
logon
  └── Task Scheduler (Windows) / launchd (macOS)
        └── beckon --serve <cfg> [--log <file>]
              ├── redirect stderr → log file        (Windows: SetStdHandle)
              ├── detach console                    (Windows: FreeConsole)
              ├── acquire single-instance flock     (unchanged)
              ├── register hotkeys                  (unchanged)
              └── run_forever                       (unchanged)
```

## Error handling

- **Log file cannot be opened** (no permission, disk full — the missing-parent
  case is handled by `create_dir_all`): fail and exit non-zero rather than
  continue with a console that is about to be detached.

  **Named regression, accepted.** Today `cmd /c … 2> log` redirects stderr
  before beckon starts, so any startup failure has `stderr_is_terminal ==
  false` and `notify::decide` returns `Show`. Under the new arrangement, a
  failure that happens *before* the redirect succeeds writes to the console
  Task Scheduler allocated — and a console is a terminal, so the same policy
  returns `AlreadyOnScreen` and posts nothing, to a window no human is
  watching. This affects exactly one message: the log-open failure itself.
  Everything after the redirect is unaffected.

  Not fixed by reordering: detaching the console first would leave that
  message with nowhere to go at all, and writing to a freed console handle
  has unverified behavior in `std`. Not fixed by bypassing the policy either
  — `cce3256` deliberately made that unrepresentable. Mitigations instead:
  `create_dir_all` removes the common cause, the task's `RestartOnFailure`
  makes it visible in Event Viewer as repeated non-zero exits, and the
  README's existing "run it in the foreground once first" step catches it at
  install time with a real terminal attached.
- **`FreeConsole` fails**: non-fatal, matching every other best-effort path
  in the Windows hotkey module (tray-add failure, session 0, `SetTimer`
  failure). Warn on stderr — which by then is the log file — and continue.
  A visible console window is cosmetic; losing the hotkeys is not.
- **No behavioral change to `notify`**, `lockfile`, reload, or registration
  reporting.

## Testing

| What | How | Where |
|---|---|---|
| `--log` actually redirects | Integration test: run `beckon --serve <invalid config> --log <tmp>`, assert exit code 1 **and** that the error line is in the file. Pins both the redirect and that it happens before the first error is printed. | CI, `windows-latest` (already in the matrix) |
| `--log` rejects without `--serve` | clap `requires` — assert non-zero exit | CI, `windows-latest` |
| Append, not truncate | Pre-write a marker line into the log, run the failing serve, assert both the marker and the new error are present | CI, `windows-latest` |
| Parent directory is created | Point `--log` at `<tmp>/does/not/exist/serve.log`, assert the file exists afterwards | CI, `windows-latest` |
| Console window is gone / flash duration | Manual, via a Scheduled Task probe. SSH to the test machine lands in session 0, so the window cannot be observed directly from a remote shell. | a14 (ARM64 Windows 11) |
| Formula syntax | `brew style` / `brew audit --strict` on the rendered formula | macOS, manual |
| `brew services start beckon` | Manual end-to-end: start, press a hotkey, check `$(brew --prefix)/var/log/beckon.log` | macOS, manual |

The console-detach behavior is not CI-verifiable — GitHub's Windows runners
do not give a task-launched process an interactive desktop. That gap is
accepted and named here rather than papered over with a test that would pass
for the wrong reason.

## Documentation

- `examples/windows/serve/README.md` — rewrite the install section around a
  single-action task; drop every mention of the VBS shim and the `mkdir` for
  the log directory; state that the log now appends.
- `examples/macos/serve/README.md` — add the `brew services` path as the
  recommended install, keep the manual plist as the fallback for non-Homebrew
  installs.
- `README.md` — resident-mode section: mention `brew services start beckon`.
- `CLAUDE.md` — record why self-daemonizing was rejected, so it is not
  re-proposed, and note the `SetStdHandle`-works-because-`Stderr`-is-a-ZST
  detail, which is non-obvious and would look like a bug to a reader.
