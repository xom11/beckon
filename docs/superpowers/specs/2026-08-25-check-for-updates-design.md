# Check for updates — design

**Date**: 2026-08-25 · **Platforms**: macOS + Windows (`serve` only) · **Status**: designed, not implemented

A `Check for updates…` row in the tray menu and a status row on the About
page. beckon asks GitHub which release is newest, says whether this build is
behind, and prints the upgrade command for the channel it was actually
installed from. It downloads nothing and writes nothing.

---

## 1. The question, and the answer

Every desktop app has this row, which is a bad reason to add one. The reason
to add it here is specific and already recorded: **version confusion has been
a real incident in this project.** On a14 a watchdog-started beckon ran the
0.8.0 image for three hours while `beckon --version` said 0.9.0 and scoop's
`current` junction pointed at 0.9.0. The About page exists because of that —
it answers *"what am I running?"* by comparing `current_exe()`'s mtime against
this process's start time.

It answers only half the question. The other half is *"and is there anything
newer?"*, and today the only way to find out is to open a browser.

So: yes, build it. But **as a check, never as an updater** — §2.

## 2. What this is not

- **Not a self-updater.** On the author's own machines beckon lives in
  `/nix/store` (read-only) and `~/scoop/apps/beckon/current` (a junction scoop
  owns). A process that overwrites itself in either place breaks the install.
  All five distribution channels — Homebrew, Scoop, nix, `cargo install
  --git`, raw release binaries — have their own upgrade path, and beckon must
  not race any of them.
- **Not a background poll.** No timer, no `last_checked` timestamp, no
  preference, no state file, no badge on the tray icon. beckon reaches the
  network exactly when a human presses a button, and at no other time.
  This is what keeps the About page's *"beckon keeps no record of what you
  type"* posture coherent: a program that phones home on a schedule has a
  different relationship with its user than one that does not.
- **Not a CLI surface.** No new verb (the growth rule forbids it: each verb
  costs an app Name permanently) and no new `doctor` row — `doctor` must stay
  offline and fast.
- **Not on Linux.** There is no `serve`, no tray and no settings window there;
  the compositor dotfile binds the keys. Nothing to hang this off.

## 3. Measured facts this design rests on

Measured 2026-08-25 on this Mac (macOS 26, Darwin 25.5.0):

```
$ /usr/bin/curl --version | head -1
curl 8.7.1 (x86_64-apple-darwin25.0) libcurl/8.7.1 (SecureTransport) …

$ /usr/bin/curl -sS -I -m 6 -o /dev/null \
      -w 'redirect_url=%{redirect_url}\nhttp_code=%{http_code}\n' \
      https://github.com/xom11/beckon/releases/latest
redirect_url=https://github.com/xom11/beckon/releases/tag/v0.10.0
http_code=302

real 0m0.196s
```

Three things follow:

1. **`/usr/bin/curl` ships with macOS.** No install step, no dependency.
2. **`/releases/latest` answers with a 302 whose `Location` carries the tag.**
   No JSON parser is needed, and this is the *web* endpoint — it is not
   subject to `api.github.com`'s 60-requests-per-hour unauthenticated limit.
3. **196 ms warm.** A 3-second ceiling is 15× headroom, which is what makes
   §7's synchronous design tolerable.

**Scope of these measurements: macOS, this machine.** Per the rule at the top
of `CLAUDE.md`, none of the three is evidence about Windows. The Windows half
is a probe, not an assumption — §9.

Also measured, by reading: **the codebase contains no `thread::spawn`, no
`PostMessageW` fan-out to a worker, and no main-thread dispatch helper.**
Everything runs on the UI thread over `Rc<RefCell<…>>`.
`beckon_macos::settings_window::post_catalog` documents the deferral in so
many words — *"the day the scan does move to a worker, only this function
changes"*. This design does not make that day today; §7.

## 4. The fetch: system curl, and why not an HTTP crate

Three candidates were weighed.

| | Mechanism | Cost | Risk |
|---|---|---|---|
| **A** ✅ | spawn the system `curl`, read `%{redirect_url}` | **no new crate**, ~40 lines | `curl.exe` on Windows ARM64 is unmeasured |
| B | `ureq`/`reqwest` + rustls | +40–80 crates, TLS in the binary | `ring`/`aws-lc-rs` need a C toolchain for the target |
| C | `NSURLSession` + `WinHTTP` | no new crate (one `windows` feature) | ~140 lines of `unsafe` split across two OSes, none of it unit-testable |

**A is chosen.**

B is rejected on a recorded scar rather than on taste: `nix build` was broken
from v0.8.0 to v0.9.3 — a month — because of one ungated `mod`. Adding sixty
crates and a TLS stack to *that* build graph trades a real risk for a small
convenience. B has a second, sharper edge: `cargo clippy --target
aarch64-pc-windows-msvc --all-targets` from macOS is a required local gate leg
in `CLAUDE.md`, and a crypto backend needing a cross C toolchain is exactly
what stops that leg from resolving.

C loses to A because it spends 140 lines of `unsafe` on what a 40-line process
spawn already does, and because **shelling out is an established pattern
here** — `beckon_macos::shell` invokes `/usr/bin/open`, `beckon_windows::shell`
calls `ShellExecuteW`.

A also degrades honestly: no curl means *"could not check"*, never *"up to
date"* (§8).

### 4.1 The exact invocation

```
macOS    /usr/bin/curl                     -sS -I --connect-timeout 2 -m 3 \
Windows  %SystemRoot%\System32\curl.exe        -o <null> -w %{redirect_url} \
                                               https://github.com/xom11/beckon/releases/latest
```

- Absolute path on macOS, matching the `/usr/bin/open` convention already in
  `shell.rs`. On Windows, `%SystemRoot%\System32\curl.exe` first, bare `curl`
  as a fallback — a user may have Git-for-Windows' or scoop's curl on `PATH`
  and either is fine, but the system one is the predictable default.
- **Spawned with `Command::new`, never through a shell.** No quoting question
  arises for the `%{…}` format string because no `cmd.exe` sees it.
- `-o /dev/null` / `-o NUL` discards the body; `-I` sends HEAD; **no `-L`**, so
  curl reports the redirect instead of following it.
- `--connect-timeout 2 -m 3` caps the whole call at three seconds.
- **On Windows the child needs `CREATE_NO_WINDOW` (`0x0800_0000`)** via
  `std::os::windows::process::CommandExt::creation_flags`. `beckon-serve.exe`
  is GUI-subsystem, so without it a console flashes on every check. Whether it
  actually flashes without the flag is part of the a14 probe (§9) — the flag
  goes in either way, but the probe is what tells us the flag *worked*.
- **No custom `User-Agent`, deliberately.** curl sends its own; adding
  `beckon/0.10.0` would tell GitHub which build the user runs, and there is no
  reason to send more than the request needs.
- Proxies come free: curl honours `http_proxy` / `https_proxy` / `no_proxy`
  without beckon knowing they exist.

### 4.2 Mapping curl's exit to a verdict

| Outcome | Result |
|---|---|
| spawn fails (`ErrorKind::NotFound`) | `CheckError::NoClient` |
| non-zero exit (6 DNS, 7 connect, 28 timeout, anything else) | `CheckError::Unreachable` |
| exit 0, `redirect_url` empty or not `…/releases/tag/vX.Y.Z` | `CheckError::Unreadable` |
| exit 0, tag parses | `Verdict` from §5.1 |

`Unreadable` is not a pedantic case: a captive portal answering 200, or
redirecting to its own login page, lands here — and it must not be reported as
success.

## 5. `beckon-core/src/update.rs` — the pure decisions

Every decision below is a pure function over its inputs, so it lives in
`beckon-core` and is compiled and tested by all three CI jobs. This is the same
reason `settings`, `caps`, `capture`, `page_plan` and `theme` are there.

### 5.1 Version, and the third verdict

```rust
pub struct Version { major: u64, minor: u64, patch: u64 }   // Ord

/// "0.10.0 (95e5596)" -> 0.10.0.  Takes the first whitespace-separated token,
/// because BECKON_VERSION carries the short sha and core cannot read env!.
pub fn parse_current(version_string: &str) -> Option<Version>;

/// ".../releases/tag/v0.11.0" -> 0.11.0
pub fn parse_tag(redirect_url: &str) -> Option<Version>;

pub enum Verdict {
    UpToDate,
    Available(Version),
    Ahead(Version),
}
```

**`Ahead` is required, not fastidious.** `beckon --version` prints
`0.10.0 (95e5596)`, and a build from `main` between two releases is *newer*
than the newest release. Reporting `UpToDate` there is false; reporting
`Available` is worse, because it would offer an upgrade command that moves the
user backwards. The repo already keeps this shape of third answer apart on
purpose — `ImageOnDisk::Gone` versus `ImageOnDisk::Unknown`, kept separate
because *"one is a fact worth printing, the other is beckon declining to claim
anything"*.

`Ahead` shows the fact and offers **no** upgrade command.

### 5.2 Which channel installed this binary

A pure function over `current_exe()`'s path — a path this process already
reads for the About page's Location row.

Normalise first: `\` → `/`, then lowercase the whole string. Windows paths are
case-insensitive and use the other separator, and one normalised needle list
keeps the function testable on all three CI jobs rather than only on Windows.

First match wins, in this order:

| normalised path contains | `Channel` |
|---|---|
| `/nix/store/` | `Nix` |
| `/scoop/apps/` | `Scoop` |
| `/cellar/`, `/homebrew/`, `/linuxbrew/` | `Homebrew` |
| `/.cargo/bin/` | `Cargo` |
| — | `Unknown` |

**The path is used unresolved.** `AboutState` deliberately does not push
`current_exe()` through `GetFinalPathNameByHandleW`, because *resolving reports
today's junction target, which is the surface that lied* in the a14 incident.
This function inherits that: it reads the path beckon was invoked as, which is
the one that names the channel. `~/scoop/apps/beckon/current/beckon-serve.exe`
says Scoop whether or not `current` currently points anywhere sensible.

Homebrew needs the three needles because the binary may present as
`/opt/homebrew/bin/beckon` (ARM), `/usr/local/…/Cellar/…` (Intel) or
`/home/linuxbrew/…`. `/usr/local/bin` alone is **not** a needle — far too
broad, and it would claim a hand-copied binary for Homebrew.

### 5.3 The upgrade command, and why `shown` ≠ `copy`

```rust
pub fn upgrade_command(channel: Channel) -> Option<AboutValue>;
```

| Channel | `copy` (the clipboard payload) | `shown` adds |
|---|---|---|
| `Nix` | `nix flake update beckon` | ` — run in your flake repo` |
| `Scoop` | `scoop update beckon` | — |
| `Homebrew` | `brew upgrade beckon` | ` — then: brew services restart beckon` |
| `Cargo` | `cargo install --git https://github.com/xom11/beckon beckon-cli --force` | — |
| `Unknown` | `None` — the Releases link is the whole answer | — |

`AboutValue { shown, copy }` already exists and exists **for exactly this**:
its doc says a single `String` would have made two jobs one field and the
clipboard would have got whichever won. A user pastes the `copy` half into a
terminal, where `— run in your flake repo` is a syntax error.

Two caveats are carried in `shown` rather than dropped:

- **nix**: `nix flake update beckon` only means anything inside the flake repo
  that has beckon as an input. Run anywhere else it fails or updates the wrong
  thing.
- **Homebrew on macOS**: the formula ships a LaunchAgent, and the running
  agent holds the old binary until the service restarts. `brew upgrade` alone
  leaves the user upgraded on disk and unchanged in memory — which is the a14
  failure again, on the other platform.

`brew services restart beckon` stays out of `copy` because beckon cannot know
whether this install is service-managed, and a command that errors for half
the users is worse on the clipboard than in a sentence.

### 5.4 What About draws

```rust
pub enum UpdateState {        // what the caller knows
    Idle,                     // no check this session
    Checking,
    Done(Verdict),
    Failed(CheckError),
}

pub struct UpdateRow {        // what the page draws — one function decides it
    pub status: Option<String>,   // None in `Idle`: there is no line yet
    pub command: Option<AboutValue>,
    pub can_check: bool,          // false while Checking
}

pub fn update_row(state: &UpdateState, channel: Channel) -> UpdateRow;
```

`status` is `Option<String>` rather than an empty `String` for the reason
`ImageOnDisk` splits `Gone` from `Unknown`: *"no line yet"* and *"a line that
says nothing"* are different instructions to the drawing code, and an empty
string makes the page decide which one it got.

`Channel` is **not** a new field on `AboutInputs`. `about_state` derives it
from the executable path `AboutInputs` already carries for the Location row,
so there is one source for "where is this binary" rather than two that can
drift.

One function produces the status word **and** the command, so the two cannot
disagree by construction — the same discipline `row_condition` already
enforces for the Shortcuts list's status vocabulary.

`UpdateState` is session state on `ServeState`, not persisted. Closing the
settings window and reopening it shows `Idle` again, which is correct: the
answer was a fact about a moment, and nothing refreshes it.

## 6. Where it appears

### 6.1 The tray row

`MENU_UPDATE: u32 = 8`, built in `serve::build_entries` — the one place the
menu is composed for both platforms — and **gated on `m.settings`**, because
the row opens the settings window and would be a lie without one.

```
beckon - 19 shortcuts          (disabled)
───────────────────────────
Settings...
Check for Updates...           ← new
Reload now
Open log                       (Windows always; macOS only with --log)
───────────────────────────
Pause hotkeys
Start with Windows             (Windows only)
───────────────────────────
Quit
```

Placed directly after `Settings...`: both rows open the same window, and
`Reload now` is about the config file rather than about beckon.

Clicking it sets `ServeState::pending_update_check = true` and calls
`open_settings`, which lands on About, consumes the flag and runs the check.
Opening About from the tab strip does **not** check — manual means the button
or the menu row, not merely arriving at the page.

`install_tray_menu`'s macOS doc comment currently opens *"Four rows against
Windows' seven"*. It becomes five against eight, and the comment must be
updated with it — that sentence is load-bearing for the next reader deciding
whether an omission is structural.

**The two labels go in the platform-strings table, not inline.** macOS writes
`Check for Updates…` (title case, real ellipsis, Apple's own wording);
Windows writes `Check for updates...`. `docs/notes/settings-window.md` records
that platform strings are tables here, not literals.

### 6.2 The About row

```
About
──────────────────────────────────────────────
 Version    0.10.0 (95e5596)          [Copy]
            0.11.0 available          [Check now]

            scoop update beckon       [Copy]
            [ Open releases page ]
──────────────────────────────────────────────
```

The second line is `UpdateRow::status`; the third is
`UpdateRow::command.shown` with its own Copy button bound to `.copy`, drawn
only when `command.is_some()`. `Open releases page` is the existing
`Target::Releases` — already wired, already opening
`https://github.com/xom11/beckon/releases`.

Status line by state:

| state | line |
|---|---|
| `Idle` | *(the line is absent; only `[Check now]` shows)* |
| `Checking` | `Checking…` and `[Check now]` disabled |
| `Done(UpToDate)` | `Up to date` |
| `Done(Available(v))` | `<v> available` + the command row |
| `Done(Ahead(v))` | `Newer than the latest release (<v>)` |
| `Failed(_)` | §8 |

## 7. Synchronous, and why there is no worker thread

```rust
SettingsCommand::CheckForUpdates => {
    st.borrow_mut().update = UpdateState::Checking;
    refresh_settings(&st);
    swin::flush_paint();               // force the frame BEFORE blocking
    let outcome = crate::update::fetch();   // ≤ 3 s, measured 196 ms
    st.borrow_mut().update = outcome;
    refresh_settings(&st);
}
```

The borrow is taken, mutated and dropped before the call that reaches the OS —
the discipline `on_probe_shortcut` already states in this file (*"must run
OUTSIDE a `settings` borrow"*) and the same one `MENU_LOG` follows before
`open_path`.

**Why not a worker thread.** `ServeState` is `Rc<RefCell<…>>` and cannot cross
a thread boundary, so a worker means `Arc`, a channel, and a per-OS wake
mechanism. On Windows that wake lands in the hazard already documented at
`beckon-windows/src/settings_window/mod.rs:8296`: the chain `apply_state →
on_select → refresh_settings → apply_state` recurses across an `extern
"system"` boundary where **a second `RefCell` borrow aborts the process rather
than unwinding**. A `PostMessageW` arriving mid-`apply_state` is that exact
shape. Three seconds of worst-case block on a button the user just pressed is
the cheaper trade, and `post_catalog`'s comment already names the function that
would change if this is ever revisited.

**`flush_paint()` is what makes the block acceptable.** `refresh_settings`
sets control text, but the frame is painted by the message pump — which is
about to be blocked. Without an explicit flush the window shows the *old*
frame for up to three seconds and reads as frozen; with it, `Checking…` is on
screen before the call starts. Roughly three lines per platform
(`UpdateWindow` on Windows; the AppKit equivalent — §10 flags this as
unverified).

## 8. Failure states — never claim up to date

The repo's standing rule is *"a blind detector and a clean result print the
same thing"*, and this feature is a textbook place to violate it: a check that
silently fails and prints `Up to date` is worse than no feature, because it
converts *"I don't know"* into a confident false assurance.

| `CheckError` | shown |
|---|---|
| `NoClient` | `Could not check — no HTTP client found` |
| `Unreachable` | `Could not reach github.com` |
| `Unreadable` | `Could not read the latest version` |

All three keep `[ Open releases page ]` and `[Check now]`, and none of them
emits an upgrade command. **`Up to date` is reachable from exactly one place:
`Done(UpToDate)`.** This is the invariant §9 pins with a test.

## 9. Testing

**Unit tests in `beckon-core`** — every one of these is a pure function, so
all three CI jobs run them:

- `parse_current` over `0.10.0 (95e5596)`, bare `0.10.0`, and garbage.
- `parse_tag` over the measured redirect, a tag without `v`, a captive-portal
  URL, and an empty string.
- `compare` in all three directions, **including `Ahead`**.
- `detect_channel` for all five outcomes in **both** separator styles and
  mixed case — `C:\Users\x\scoop\apps\beckon\current\beckon-serve.exe` and
  `/nix/store/abc-beckon-0.10.0/bin/beckon` are the two that matter most.
- `/usr/local/bin/beckon` resolves to `Unknown`, not `Homebrew` (§5.2).
- `upgrade_command` returns `None` for `Unknown`, and for every other channel
  `copy` contains no `—`, no parenthetical and no second sentence.
- **The invariant**: for every `CheckError`, `update_row(...).status` is not
  `Up to date` and `.command` is `None`.
- `build_entries` grows the row when `settings` is true and omits it when
  false.

**Probe on a14 (ARM64 Windows 11)** — required, because §3's measurements are
data about macOS and nothing else:

1. Does `C:\Windows\System32\curl.exe` exist?
2. Does it return the same 302 through a Schannel-backed build?
3. Spawned from `beckon-serve.exe` (GUI subsystem): does a console flash
   without `CREATE_NO_WINDOW`, and is it silent with it? **Run the negative
   control** — a check that never flashes because the spawn silently failed
   looks identical to one that never flashes because the flag worked.

Per the memory note, SSH to a14 lands in session 0 and has no desktop, so (3)
must go through a scheduled task in session 1; (1) and (2) are fine over SSH.

If (1) fails, the feature degrades to `NoClient` — already designed, already
tested, no code change. That is the point of §4's honest-degradation property.

**macOS**: §3 is the measurement. The remaining live check is that
`flush_paint` actually puts `Checking…` on screen before the block — verifiable
with `testing/macos_settings_drive.lua`.

## 10. Unverified at design time

Stated plainly so the implementation does not inherit them as premises:

1. **`curl.exe` on Windows ARM64.** §9's probe. Everything downstream is
   designed for it being absent.
2. **The AppKit primitive behind `flush_paint`.** `displayIfNeeded` on the
   content view is the candidate; whether AppKit will paint without a run-loop
   turn has not been measured here. If it will not, the macOS arm needs a
   single `NSRunLoop` spin instead — measure before choosing.
3. **Whether About has a colour/tone vocabulary** the status line should join.
   `FlagTone` exists in `settings.rs` but was read only through doc comments.
   If nothing fits, the line is plain text; inventing a fifth status colour is
   out of scope.
4. **Whether `refresh_settings` currently repaints About at all.** About is
   near-static today, so the push path may exist without ever having been
   exercised on that page.

## 11. Files

| crate | file | change |
|---|---|---|
| `beckon-core` | `src/update.rs` | **new** — §5 in full |
| | `src/lib.rs` | `pub mod update;` |
| | `src/settings.rs` | `AboutState.update: UpdateRow`, `AboutInputs` gains `UpdateState` (only — `Channel` is derived, §5.4), `SettingsCommand::CheckForUpdates`, two labels into the platform-strings table |
| `beckon-cli` | `src/update.rs` | **new** — §4, the spawn and the exit mapping |
| | `src/serve.rs` | `MENU_UPDATE`, the `build_entries` row, both `install_tray_menu` arms, `ServeState::{update, pending_update_check}`, the `on_command` arm, `open_settings` consuming the flag, the macOS row-count comment |
| `beckon-macos` | `src/settings_window/about.rs`, `mod.rs` | the row, the two buttons, `flush_paint` |
| `beckon-windows` | `src/settings_window/*` | the same |

`SettingsCommand`'s `on_command` match is exhaustive on purpose — *"every
variant added later is a compile error at this one site"* — so adding
`CheckForUpdates` makes the compiler name every place that must handle it.
That is the design working, not friction.

## 12. Rejected alternatives

- **`api.github.com/repos/xom11/beckon/releases/latest` + a JSON parser.**
  Needs `serde_json` in `beckon-cli` (today it is a dev-dependency only) and
  buys a rate limit of 60/hour per IP. The 302 gives the same answer with
  neither.
- **A tray badge / red dot when an update exists.** Requires the background
  poll this design rejects in §2; with a manual check the dot would only ever
  appear after the user had already read the answer.
- **Running the upgrade command from the window.** beckon would be killing
  itself mid-command; nix needs the right working directory; brew needs a
  service restart afterwards. The command is the deliverable, the clipboard is
  the handoff.
- **A `beckon update` verb, or `--check-updates` on an existing one.** The
  growth rule spends an app Name on every verb, and a flag would need the
  network code compiled into the ~10 ms hot-path binary for a feature only
  `serve` users can reach.
