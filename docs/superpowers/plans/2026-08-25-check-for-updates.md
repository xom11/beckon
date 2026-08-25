# Check for updates — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a `Check for updates...` row to the tray menu and an update-status
row to the About page on macOS and Windows, which reports whether a newer
beckon release exists and prints the upgrade command for the channel this
binary was installed from.

**Architecture:** All decisions are pure functions in a new
`beckon-core/src/update.rs`, tested by all three CI jobs. The single impure
part is a `curl` process spawn in `beckon-cli/src/update.rs` that reads the
`Location` header of GitHub's `/releases/latest` 302. The check runs
synchronously on the UI thread behind a 3-second ceiling; there is no worker
thread and no background poll.

**Tech Stack:** Rust, no new crates. System `curl`. Existing `MenuEntry`,
`SettingsCommand`, `AboutInputs`/`AboutState`/`AboutValue`, `FlagTone`.

**Spec:** `docs/superpowers/specs/2026-08-25-check-for-updates-design.md` —
read it before Task 1. Every "why" below is argued there.

**Worktree:** `.worktrees/check-updates`, branch `feat/check-for-updates`,
already created from `origin/main`. Do all work there. Never in the primary
checkout, never on `main`.

## Global Constraints

- **No new crate dependencies.** Not in any `Cargo.toml`. If a step seems to
  need one, stop and re-read spec §4.
- **ASCII only in every display string.** No `—`, no `…`, no smart quotes.
  Write `-` and `...`. This is a standing rule for this window.
- **`Up to date` is reachable from exactly one state**, `Done(Verdict::UpToDate)`.
  A failed check must never produce it. Pinned by a test in Task 3.
- **No network call outside the manual check.** No timer, no persisted
  timestamp, no preference, no state file, no tray badge.
- **No new CLI verb and no new `doctor` row.** The growth rule spends an app
  Name on every verb.
- **The tray row is gated on `m.settings`** — it opens the settings window and
  would be a lie without one.
- **Borrow discipline:** mutate under a short `borrow_mut`, drop it, *then*
  call anything that reaches the OS or re-enters `ServeState`. Never hold a
  `RefCell` borrow across `swin::*`, `open_path`, or a process spawn.
- **Linux compiles but gets nothing.** `mod serve` is
  `#[cfg(any(target_os = "macos", target_os = "windows"))]`; the new
  `beckon-cli` module carries the same gate. `beckon-core::update` is
  ungated — that is what puts its tests on all three CI jobs.

### The gate — run before every commit that touches Rust

```sh
export CARGO_TARGET_DIR=~/Documents/dev/beckon/target   # shared: fine for these
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo clippy --target aarch64-pc-windows-msvc --all-targets -- -D warnings
cargo check --workspace --all-targets
cargo test -p beckon-core
cargo test -p beckon-cli
```

Six legs, and none is redundant:

- `cargo check --target …` runs **no lints at all**, so the cross-*clippy* leg
  is the only thing that sees Windows-only clippy errors from this Mac.
- The bare `cargo check --workspace --all-targets` is the "unexcluded" leg CI
  added; it is the only one that compiles `beckon-windows` off Windows.
- `cargo fmt --all -- --check` **does** cover `cfg`-gated Windows modules —
  rustfmt does not evaluate `cfg` when walking the module tree.

**`cargo test -p beckon-cli` is SIGKILLed on its first run on this machine**
(exit 137, empty output); the second run passes. Warm it with one direct exec
of the freshly linked binary before believing a failure.

An error naming a symbol you can grep is a **stale artifact**, not a bug. When
cleaning, `cargo clean -p <pkg>` misses cross-target artifacts — pass
`--target aarch64-pc-windows-msvc` too.

---

## File Structure

| File | Responsibility |
|---|---|
| `crates/beckon-core/src/update.rs` | **new.** Version math, channel detection, upgrade commands, the state machine, the row projection. Every decision, all pure, all tested. |
| `crates/beckon-core/src/lib.rs` | `pub mod update;` |
| `crates/beckon-core/src/settings.rs` | `AboutInputs.update`, `AboutState.update`, `SettingsCommand::CheckForUpdates` |
| `crates/beckon-core/src/menu.rs` | `update_label(macos: bool)` — the one place the two labels live |
| `crates/beckon-cli/src/update.rs` | **new.** The `curl` spawn and nothing else. No decisions. |
| `crates/beckon-cli/src/serve.rs` | `MENU_UPDATE`, the menu row, `ServeState` fields, the `on_command` arm, `open_settings` |
| `crates/beckon-macos/src/settings_window/{mod,about}.rs` | `flush_paint`, the About rows |
| `crates/beckon-windows/src/settings_window/*` | `flush_paint`, the About rows |

---

## Task 1: Version math

**Files:**
- Create: `crates/beckon-core/src/update.rs`
- Modify: `crates/beckon-core/src/lib.rs`
- Test: inline `#[cfg(test)] mod tests` at the bottom of `update.rs` (the
  convention in every other `beckon-core` module)

**Interfaces:**
- Consumes: nothing
- Produces: `Version { major: u64, minor: u64, patch: u64 }` (`Copy + Ord +
  Display`), `parse_current(&str) -> Option<Version>`,
  `parse_tag(&str) -> Option<Version>`,
  `Verdict { UpToDate, Available(Version), Ahead(Version) }` (`Copy + Eq`),
  `compare(current: Version, latest: Version) -> Verdict`

- [ ] **Step 1: Write the failing tests**

Create `crates/beckon-core/src/update.rs` containing only this test module for
now:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn v(major: u64, minor: u64, patch: u64) -> Version {
        Version { major, minor, patch }
    }

    /// `BECKON_VERSION` carries the short sha, and core cannot read `env!`.
    #[test]
    fn the_running_version_is_the_first_token_sha_and_all() {
        assert_eq!(parse_current("0.10.0 (95e5596)"), Some(v(0, 10, 0)));
        assert_eq!(parse_current("0.10.0"), Some(v(0, 10, 0)));
        assert_eq!(parse_current(""), None);
        assert_eq!(parse_current("not a version"), None);
        assert_eq!(parse_current("0.10"), None);
        assert_eq!(parse_current("0.10.0.1"), None);
    }

    #[test]
    fn the_latest_version_comes_out_of_the_redirect_url() {
        assert_eq!(
            parse_tag("https://github.com/xom11/beckon/releases/tag/v0.10.0"),
            Some(v(0, 10, 0))
        );
        // A tag without the `v` still parses -- the prefix is stripped when
        // present, not required.
        assert_eq!(
            parse_tag("https://github.com/xom11/beckon/releases/tag/0.11.2"),
            Some(v(0, 11, 2))
        );
    }

    /// curl prints an empty `%{redirect_url}` when nothing redirected, and a
    /// captive portal redirects somewhere that is not a release tag. Neither
    /// may be read as success.
    #[test]
    fn a_non_release_redirect_is_not_a_version() {
        assert_eq!(parse_tag(""), None);
        assert_eq!(parse_tag("   "), None);
        assert_eq!(parse_tag("https://portal.example/login"), None);
        assert_eq!(parse_tag("https://github.com/xom11/beckon/releases"), None);
        // The last segment IS a valid triple, but the path is not a release
        // tag -- what a captive portal or a CDN error page produces. Reading
        // it as a release would report a version that does not exist.
        assert_eq!(parse_tag("https://portal.example/1.2.3"), None);
        assert_eq!(
            parse_tag("https://github.com/xom11/beckon/releases/tag/v0.11.0/extra"),
            None
        );
    }

    #[test]
    fn ordering_is_major_then_minor_then_patch() {
        assert!(v(0, 10, 0) < v(0, 11, 0));
        assert!(v(0, 9, 9) < v(0, 10, 0));
        assert!(v(1, 0, 0) > v(0, 99, 99));
        assert!(v(0, 10, 1) > v(0, 10, 0));
    }

    /// The third verdict is the whole reason this is an enum and not a bool.
    /// A build from `main` between two releases is NEWER than the newest
    /// release; `UpToDate` would be false and `Available` would offer an
    /// upgrade that moves the user backwards.
    #[test]
    fn a_build_ahead_of_the_latest_release_is_neither_up_to_date_nor_behind() {
        assert_eq!(compare(v(0, 11, 0), v(0, 10, 0)), Verdict::Ahead(v(0, 10, 0)));
        assert_eq!(compare(v(0, 10, 0), v(0, 10, 0)), Verdict::UpToDate);
        assert_eq!(
            compare(v(0, 10, 0), v(0, 11, 0)),
            Verdict::Available(v(0, 11, 0))
        );
    }

    #[test]
    fn a_version_displays_as_a_bare_triple() {
        assert_eq!(v(0, 10, 0).to_string(), "0.10.0");
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

```sh
cargo test -p beckon-core update::
```

Expected: the crate does not compile — `update` is not a module of
`beckon-core` yet, and none of `Version`, `parse_current`, `parse_tag`,
`compare`, `Verdict` exists.

- [ ] **Step 3: Declare the module**

In `crates/beckon-core/src/lib.rs`, add `pub mod update;` beside the other
`pub mod` declarations, in alphabetical position.

- [ ] **Step 4: Write the implementation**

Prepend to `crates/beckon-core/src/update.rs`, above the test module:

```rust
//! Is there a newer beckon than this one, and what would upgrade it.
//!
//! Every decision on this page is a pure function over its inputs, which is
//! why it lives here rather than in a window: all three CI jobs compile and
//! test it, the way they do `settings`, `caps`, `capture` and `theme`.
//!
//! **beckon checks; it never updates.** On the machines this ships to, the
//! binary lives in `/nix/store` (read-only) or under a junction scoop owns,
//! and a process that overwrites itself in either place breaks the install.
//! The deliverable of a check is the upgrade command for the channel that
//! actually installed this binary -- see `upgrade_command`.

/// A release version. Field order IS the comparison order, which is what the
/// derived `Ord` gives us for free: major, then minor, then patch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
}

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

/// Exactly three dot-separated integers. A fourth component is rejected
/// rather than ignored: `0.10.0.1` is not a shape this project publishes, so
/// reading it as `0.10.0` would be inventing an answer.
fn parse_triple(s: &str) -> Option<Version> {
    let mut parts = s.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch = parts.next()?.parse().ok()?;
    if parts.next().is_some() {
        return None;
    }
    Some(Version {
        major,
        minor,
        patch,
    })
}

/// `"0.10.0 (95e5596)"` -> `0.10.0`.
///
/// The caller passes `env!("BECKON_VERSION")`, which carries the short sha so
/// that two builds between releases can be told apart. This crate cannot read
/// that `env!` itself -- it is set by `beckon-cli`'s build script -- so the
/// string arrives as a parameter and the sha is dropped here.
pub fn parse_current(version_string: &str) -> Option<Version> {
    parse_triple(version_string.split_whitespace().next()?)
}

/// `".../releases/tag/v0.11.0"` -> `0.11.0`.
///
/// **The `/releases/tag/` segment is required, not merely the shape of the
/// last component.** Without it a redirect to `https://portal.example/1.2.3`
/// -- a captive portal, a CDN error page -- parses as a release and beckon
/// reports a version that does not exist. Spec §4.2 makes this the boundary
/// between a verdict and `CheckError::Unreadable`.
///
/// A leading `v` is stripped when present, not required. Everything that is
/// not a release tag -- an empty string (curl printed no redirect), a login
/// page, the releases index itself -- falls out as `None`, which the caller
/// must report as a failed check and never as success.
///
/// **No host check, deliberately.** The spec names the path shape and
/// nothing more; asserting `github.com` here would be scope this function
/// does not have.
pub fn parse_tag(redirect_url: &str) -> Option<Version> {
    let tag = redirect_url.trim().split_once("/releases/tag/")?.1;
    // A further separator means the tag is not the final component -- treat
    // it as unreadable rather than guessing which part is the version.
    if tag.contains('/') {
        return None;
    }
    parse_triple(tag.strip_prefix('v').unwrap_or(tag))
}

/// What the comparison found.
///
/// **`Ahead` is required, not fastidious.** A build from `main` between two
/// releases is newer than the newest release; reporting `UpToDate` there is
/// false, and reporting `Available` is worse because the upgrade command
/// would move the user backwards. This is the same shape of third answer
/// `ImageOnDisk` keeps apart -- one is a fact worth printing, the other is
/// beckon declining to claim anything.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    UpToDate,
    Available(Version),
    Ahead(Version),
}

pub fn compare(current: Version, latest: Version) -> Verdict {
    match latest.cmp(&current) {
        std::cmp::Ordering::Greater => Verdict::Available(latest),
        std::cmp::Ordering::Equal => Verdict::UpToDate,
        std::cmp::Ordering::Less => Verdict::Ahead(latest),
    }
}
```

- [ ] **Step 5: Run the tests to verify they pass**

```sh
cargo test -p beckon-core update::
```

Expected: 6 tests pass.

- [ ] **Step 6: Run the gate**

Run the six-leg gate from Global Constraints. All must pass.

- [ ] **Step 7: Commit**

```sh
git commit --only crates/beckon-core/src/update.rs crates/beckon-core/src/lib.rs \
  -m "core(update): version math, and the third verdict a dev build needs"
git show --stat HEAD
```

`--only`, not `git add` + `git commit`: on this repo a peer session staging
files mid-race would otherwise land their work in your commit. `git show
--stat` is how you verify what actually went in.

---

## Task 2: Which channel installed this binary

**Files:**
- Modify: `crates/beckon-core/src/update.rs`
- Test: the same inline `mod tests`

**Interfaces:**
- Consumes: `crate::settings::AboutValue { shown: String, copy: String }`
- Produces: `Channel { Nix, Scoop, Homebrew, Cargo, Unknown }` (`Copy + Eq`),
  `detect_channel(exe: Option<&std::path::Path>) -> Channel`,
  `upgrade_command(channel: Channel) -> Option<AboutValue>`

- [ ] **Step 1: Write the failing tests**

Add inside the existing `mod tests`:

```rust
    use std::path::Path;

    /// The five outcomes, in both separator styles. A Windows path reaches
    /// this function as ONE `Path` component on the two CI jobs that are not
    /// Windows -- which is exactly why the function works on the string form
    /// and normalises separators itself.
    #[test]
    fn the_channel_comes_out_of_the_path_the_binary_was_invoked_as() {
        let cases: &[(&str, Channel)] = &[
            ("/nix/store/abc123-beckon-0.10.0/bin/beckon", Channel::Nix),
            (r"C:\Users\me\scoop\apps\beckon\current\beckon-serve.exe", Channel::Scoop),
            ("/home/me/scoop/apps/beckon/current/beckon-serve.exe", Channel::Scoop),
            ("/opt/homebrew/bin/beckon", Channel::Homebrew),
            ("/usr/local/Cellar/beckon/0.10.0/bin/beckon", Channel::Homebrew),
            ("/home/linuxbrew/.linuxbrew/bin/beckon", Channel::Homebrew),
            ("/Users/me/.cargo/bin/beckon", Channel::Cargo),
        ];
        for (path, want) in cases {
            assert_eq!(detect_channel(Some(Path::new(path))), *want, "{path}");
        }
    }

    /// Windows paths are case-insensitive, so the needles must be too.
    #[test]
    fn channel_detection_ignores_case() {
        assert_eq!(
            detect_channel(Some(Path::new(r"C:\Users\Me\Scoop\Apps\Beckon\current\beckon-serve.exe"))),
            Channel::Scoop
        );
    }

    /// `/usr/local/bin` is deliberately NOT a Homebrew needle: it is far too
    /// broad and would claim a hand-copied binary for a package manager that
    /// never saw it.
    #[test]
    fn an_unrecognised_location_is_unknown_not_a_guess() {
        assert_eq!(detect_channel(Some(Path::new("/usr/local/bin/beckon"))), Channel::Unknown);
        assert_eq!(detect_channel(Some(Path::new("/home/me/Downloads/beckon"))), Channel::Unknown);
        assert_eq!(detect_channel(None), Channel::Unknown);
    }

    /// `shown` may carry a caveat; `copy` is the bare payload, because what a
    /// user does with it is paste it into a terminal. This is the whole
    /// reason `AboutValue` has two fields.
    #[test]
    fn the_clipboard_half_is_a_command_and_nothing_else() {
        for ch in [Channel::Nix, Channel::Scoop, Channel::Homebrew, Channel::Cargo] {
            let cmd = upgrade_command(ch).expect("every known channel has a command");
            assert!(!cmd.copy.contains(" - "), "{ch:?}: copy carries a caveat");
            assert!(!cmd.copy.contains('('), "{ch:?}: copy carries a parenthetical");
            assert!(cmd.shown.starts_with(cmd.copy.as_str()), "{ch:?}: shown must lead with the command");
            assert!(cmd.copy.is_ascii(), "{ch:?}: display strings are ASCII here");
            assert!(cmd.shown.is_ascii(), "{ch:?}: display strings are ASCII here");
        }
    }

    /// The two caveats that would otherwise be lost. nix updates the wrong
    /// thing outside the flake repo; brew leaves the LaunchAgent holding the
    /// old binary, which is the a14 incident on the other platform.
    #[test]
    fn the_two_channels_with_a_caveat_carry_it_in_shown() {
        let nix = upgrade_command(Channel::Nix).unwrap();
        assert_eq!(nix.copy, "nix flake update beckon");
        assert!(nix.shown.contains("flake repo"));

        let brew = upgrade_command(Channel::Homebrew).unwrap();
        assert_eq!(brew.copy, "brew upgrade beckon");
        assert!(brew.shown.contains("brew services restart beckon"));
    }

    /// No command at all rather than a guessed one. The Releases link is the
    /// whole answer for a binary beckon cannot place.
    #[test]
    fn an_unknown_channel_gets_no_command() {
        assert_eq!(upgrade_command(Channel::Unknown), None);
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

```sh
cargo test -p beckon-core update::
```

Expected: compile error — `Channel`, `detect_channel`, `upgrade_command` do
not exist.

- [ ] **Step 3: Write the implementation**

Append to `crates/beckon-core/src/update.rs`, above `mod tests`:

```rust
use crate::settings::AboutValue;

/// How this binary got onto the machine, as far as its own path can say.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Channel {
    Nix,
    Scoop,
    Homebrew,
    Cargo,
    /// A hand-placed binary, a build tree, or a packaging route beckon does
    /// not know. The Releases link is the whole answer for these.
    Unknown,
}

/// Which channel installed the binary at `exe`.
///
/// **The path is used UNRESOLVED**, and that is inherited rather than
/// incidental: `AboutState` deliberately does not push `current_exe()`
/// through `GetFinalPathNameByHandleW`, because resolving reports today's
/// junction target -- the surface that lied during the a14 incident.
/// `~/scoop/apps/beckon/current/beckon-serve.exe` says Scoop whether or not
/// `current` points anywhere sensible today.
///
/// Matching is on the STRING form with `\` folded to `/` and the whole thing
/// lowercased. Two reasons, and both are load-bearing: Windows paths are
/// case-insensitive and use the other separator, and a `\`-spelled literal is
/// a single `Path` component on the two CI jobs that are not Windows -- so a
/// component-wise match would be untestable everywhere it matters most.
pub fn detect_channel(exe: Option<&std::path::Path>) -> Channel {
    let Some(exe) = exe else {
        return Channel::Unknown;
    };
    let p = exe.to_string_lossy().replace('\\', "/").to_lowercase();
    if p.contains("/nix/store/") {
        Channel::Nix
    } else if p.contains("/scoop/apps/") {
        Channel::Scoop
    } else if p.contains("/cellar/") || p.contains("/homebrew/") || p.contains("/linuxbrew/") {
        // Three needles because the same formula presents as
        // `/opt/homebrew/bin` (ARM), `/usr/local/…/Cellar/…` (Intel) and
        // `/home/linuxbrew/…`. `/usr/local/bin` alone is NOT a needle: it
        // would claim every hand-copied binary on a Mac.
        Channel::Homebrew
    } else if p.contains("/.cargo/bin/") {
        Channel::Cargo
    } else {
        Channel::Unknown
    }
}

/// What would upgrade this install, as a command plus the caveat that does
/// not belong on the clipboard.
///
/// `shown` leads with the command and may add a caveat; `copy` is the command
/// alone. `AboutValue`'s own doc says why they are two fields: a user pastes
/// the copied half into a terminal, where ` - run in your flake repo` is a
/// syntax error.
///
/// `brew services restart beckon` stays OUT of `copy` on purpose. The
/// Homebrew formula ships a LaunchAgent, and the running agent holds the old
/// binary until the service restarts -- but beckon cannot know whether this
/// install is service-managed, and a command that errors for half the users
/// is worse on the clipboard than in a sentence.
pub fn upgrade_command(channel: Channel) -> Option<AboutValue> {
    let (copy, caveat) = match channel {
        Channel::Nix => ("nix flake update beckon", " - run in your flake repo"),
        Channel::Scoop => ("scoop update beckon", ""),
        Channel::Homebrew => (
            "brew upgrade beckon",
            " - then: brew services restart beckon",
        ),
        Channel::Cargo => (
            "cargo install --git https://github.com/xom11/beckon beckon-cli --force",
            "",
        ),
        Channel::Unknown => return None,
    };
    Some(AboutValue {
        shown: format!("{copy}{caveat}"),
        copy: copy.to_string(),
    })
}
```

- [ ] **Step 4: Run the tests to verify they pass**

```sh
cargo test -p beckon-core update::
```

Expected: 12 tests pass.

- [ ] **Step 5: Run the gate**

- [ ] **Step 6: Commit**

```sh
git commit --only crates/beckon-core/src/update.rs \
  -m "core(update): read the channel off the unresolved exe path"
git show --stat HEAD
```

---

## Task 3: The state machine and the row it draws

**Files:**
- Modify: `crates/beckon-core/src/update.rs`
- Test: the same inline `mod tests`

**Interfaces:**
- Consumes: `Version`, `Verdict`, `compare`, `parse_tag`, `Channel`,
  `upgrade_command` (Tasks 1-2); `crate::settings::FlagTone`
- Produces: `CheckError { NoClient, Unreachable, Unreadable }` (`Copy + Eq`),
  `UpdateState { Idle, Checking, Done(Verdict), Failed(CheckError) }`
  (`Copy + Eq`), `CurlOutcome<'a> { NotSpawned, Failed, Ok(&'a str) }`,
  `interpret(outcome: CurlOutcome, current: Version) -> UpdateState`,
  `UpdateRow { status: Option<String>, tone: FlagTone, command:
  Option<AboutValue>, can_check: bool }` (`Clone + Eq`),
  `update_row(state: UpdateState, channel: Channel) -> UpdateRow`,
  `pub const UP_TO_DATE: &str`

**`UpdateState` must be `Copy`.** `AboutInputs` derives `Copy` and Task 4 adds
this to it; every payload here is `Copy`, so keep it that way.

- [ ] **Step 1: Write the failing tests**

Add inside the existing `mod tests`:

```rust
    /// The three ways a spawn can end, mapped to the three failures a user
    /// can act on differently.
    #[test]
    fn each_way_the_spawn_can_fail_gets_its_own_error() {
        let cur = v(0, 10, 0);
        assert_eq!(
            interpret(CurlOutcome::NotSpawned, cur),
            UpdateState::Failed(CheckError::NoClient)
        );
        assert_eq!(
            interpret(CurlOutcome::Failed, cur),
            UpdateState::Failed(CheckError::Unreachable)
        );
        // Exit zero, but nothing that names a release: a captive portal, or
        // no redirect at all.
        assert_eq!(
            interpret(CurlOutcome::Ok("https://portal.example/login"), cur),
            UpdateState::Failed(CheckError::Unreadable)
        );
        assert_eq!(
            interpret(CurlOutcome::Ok(""), cur),
            UpdateState::Failed(CheckError::Unreadable)
        );
    }

    /// The measured redirect from the design doc, end to end.
    #[test]
    fn the_measured_redirect_produces_a_verdict() {
        assert_eq!(
            interpret(
                CurlOutcome::Ok("https://github.com/xom11/beckon/releases/tag/v0.10.0"),
                v(0, 10, 0)
            ),
            UpdateState::Done(Verdict::UpToDate)
        );
        assert_eq!(
            interpret(
                CurlOutcome::Ok("https://github.com/xom11/beckon/releases/tag/v0.11.0"),
                v(0, 10, 0)
            ),
            UpdateState::Done(Verdict::Available(v(0, 11, 0)))
        );
    }

    /// **The invariant.** A blind detector and a clean result print the same
    /// thing, and a check that silently fails while saying `Up to date`
    /// converts "I don't know" into a confident false assurance. This is the
    /// single most important test in the module.
    #[test]
    fn a_failed_check_never_says_up_to_date_and_never_offers_a_command() {
        for e in [
            CheckError::NoClient,
            CheckError::Unreachable,
            CheckError::Unreadable,
        ] {
            let row = update_row(UpdateState::Failed(e), Channel::Scoop);
            assert_ne!(row.status.as_deref(), Some(UP_TO_DATE), "{e:?}");
            assert!(row.command.is_none(), "{e:?}");
            assert_eq!(row.tone, FlagTone::Warn, "{e:?}");
            assert!(row.can_check, "{e:?}: a failed check must be retryable");
        }
    }

    /// `Idle` has no line at all, which is a different instruction to the
    /// drawing code than "a line that says nothing" -- the reason `status` is
    /// an `Option` rather than a possibly-empty `String`.
    #[test]
    fn before_any_check_there_is_no_line() {
        let row = update_row(UpdateState::Idle, Channel::Scoop);
        assert_eq!(row.status, None);
        assert!(row.command.is_none());
        assert!(row.can_check);
    }

    /// While a check is in flight the button must not be pressable again --
    /// the call blocks this thread, so a second press could only queue behind
    /// the first.
    #[test]
    fn a_check_in_flight_disables_its_own_button() {
        let row = update_row(UpdateState::Checking, Channel::Scoop);
        assert_eq!(row.status.as_deref(), Some("Checking..."));
        assert!(!row.can_check);
    }

    /// The command appears for exactly one state, and carries the channel.
    #[test]
    fn only_an_available_update_carries_an_upgrade_command() {
        let avail = update_row(UpdateState::Done(Verdict::Available(v(0, 11, 0))), Channel::Scoop);
        assert_eq!(avail.status.as_deref(), Some("0.11.0 available"));
        assert_eq!(avail.command.map(|c| c.copy), Some("scoop update beckon".into()));

        assert!(update_row(UpdateState::Done(Verdict::UpToDate), Channel::Scoop).command.is_none());
        assert!(update_row(UpdateState::Done(Verdict::Ahead(v(0, 9, 9))), Channel::Scoop).command.is_none());
    }

    /// An unknown channel still gets the news, just not a command.
    #[test]
    fn an_available_update_on_an_unknown_channel_still_reports_the_version() {
        let row = update_row(UpdateState::Done(Verdict::Available(v(0, 11, 0))), Channel::Unknown);
        assert_eq!(row.status.as_deref(), Some("0.11.0 available"));
        assert!(row.command.is_none());
    }

    /// `Ahead` says what is true and offers nothing -- an upgrade command
    /// here would move the user backwards.
    #[test]
    fn a_build_ahead_of_the_release_says_so_plainly() {
        let row = update_row(UpdateState::Done(Verdict::Ahead(v(0, 9, 9))), Channel::Homebrew);
        assert_eq!(
            row.status.as_deref(),
            Some("Newer than the latest release (0.9.9)")
        );
        assert!(row.command.is_none());
        assert_eq!(row.tone, FlagTone::Neutral);
    }

    /// Every string this module can put on screen is ASCII, like every other
    /// display string in this window.
    #[test]
    fn every_status_line_is_ascii() {
        let states = [
            UpdateState::Idle,
            UpdateState::Checking,
            UpdateState::Done(Verdict::UpToDate),
            UpdateState::Done(Verdict::Available(v(0, 11, 0))),
            UpdateState::Done(Verdict::Ahead(v(0, 9, 9))),
            UpdateState::Failed(CheckError::NoClient),
            UpdateState::Failed(CheckError::Unreachable),
            UpdateState::Failed(CheckError::Unreadable),
        ];
        for s in states {
            let row = update_row(s, Channel::Homebrew);
            if let Some(line) = &row.status {
                assert!(line.is_ascii(), "{s:?}: {line}");
            }
        }
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

```sh
cargo test -p beckon-core update::
```

Expected: compile error — `CurlOutcome`, `interpret`, `UpdateState`,
`CheckError`, `update_row`, `UpdateRow`, `UP_TO_DATE` do not exist.

- [ ] **Step 3: Write the implementation**

Append to `crates/beckon-core/src/update.rs`, above `mod tests`:

```rust
use crate::settings::FlagTone;

/// Why a check produced no answer.
///
/// Three variants rather than one, because a reader acts differently on each:
/// `NoClient` is a machine that cannot check at all, `Unreachable` is worth
/// retrying, and `Unreadable` means something answered but it was not GitHub.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckError {
    /// No `curl` to spawn.
    NoClient,
    /// curl ran and failed -- DNS, connection refused, timeout.
    Unreachable,
    /// curl succeeded but what came back does not name a release. A captive
    /// portal answering 200, or redirecting to its own login page, lands
    /// here -- and must not be reported as success.
    Unreadable,
}

/// What the caller knows about the check right now.
///
/// Session state, deliberately not persisted: the answer was a fact about a
/// moment, and nothing refreshes it. Closing the window and reopening it
/// shows `Idle` again, which is correct.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateState {
    /// No check has run this session.
    Idle,
    Checking,
    Done(Verdict),
    Failed(CheckError),
}

/// What the spawn actually produced, as the three cases this crate can reason
/// about. Keeping the process handling in `beckon-cli` and the meaning here
/// is what puts the meaning under all three CI jobs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CurlOutcome<'a> {
    /// The binary could not be spawned at all.
    NotSpawned,
    /// It ran and exited non-zero.
    Failed,
    /// It exited zero; this is `%{redirect_url}` as printed.
    Ok(&'a str),
}

pub fn interpret(outcome: CurlOutcome<'_>, current: Version) -> UpdateState {
    match outcome {
        CurlOutcome::NotSpawned => UpdateState::Failed(CheckError::NoClient),
        CurlOutcome::Failed => UpdateState::Failed(CheckError::Unreachable),
        CurlOutcome::Ok(url) => match parse_tag(url) {
            Some(latest) => UpdateState::Done(compare(current, latest)),
            None => UpdateState::Failed(CheckError::Unreadable),
        },
    }
}

/// The one string that means "checked, and there is nothing newer".
///
/// A constant rather than a literal so the invariant test cannot drift out of
/// agreement with the code by a wording change.
pub const UP_TO_DATE: &str = "Up to date";

/// What the About page draws for the update row.
///
/// One function produces the status line AND the command, so the two cannot
/// disagree by construction rather than by discipline -- the rule
/// `row_condition` already follows for the Shortcuts list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateRow {
    /// `None` in `Idle`: there is no line yet. Not an empty `String`, because
    /// "no line" and "a line that says nothing" are different instructions to
    /// the drawing code and an empty string makes the page decide which it
    /// got -- the same reason `ImageOnDisk` splits `Gone` from `Unknown`.
    pub status: Option<String>,
    /// `Warn` only for a failure. An available update is news, not a problem,
    /// so it stays `Neutral` and draws no pill.
    ///
    /// Computed here rather than through `flag_tone`, which owns a CLOSED
    /// four-word vocabulary for the Shortcuts list. These words are not in it
    /// and must not be added to it.
    pub tone: FlagTone,
    pub command: Option<AboutValue>,
    /// `false` only while a check is in flight -- the call blocks this
    /// thread, so a second press could only queue behind the first.
    pub can_check: bool,
}

pub fn update_row(state: UpdateState, channel: Channel) -> UpdateRow {
    let plain = |status: Option<String>| UpdateRow {
        status,
        tone: FlagTone::Neutral,
        command: None,
        can_check: true,
    };
    match state {
        UpdateState::Idle => plain(None),
        UpdateState::Checking => UpdateRow {
            can_check: false,
            ..plain(Some("Checking...".into()))
        },
        UpdateState::Done(Verdict::UpToDate) => plain(Some(UP_TO_DATE.into())),
        UpdateState::Done(Verdict::Available(v)) => UpdateRow {
            command: upgrade_command(channel),
            ..plain(Some(format!("{v} available")))
        },
        UpdateState::Done(Verdict::Ahead(v)) => {
            plain(Some(format!("Newer than the latest release ({v})")))
        }
        UpdateState::Failed(e) => UpdateRow {
            tone: FlagTone::Warn,
            ..plain(Some(
                match e {
                    CheckError::NoClient => "Could not check - no HTTP client found",
                    CheckError::Unreachable => "Could not reach github.com",
                    CheckError::Unreadable => "Could not read the latest version",
                }
                .into(),
            ))
        },
    }
}
```

- [ ] **Step 4: Run the tests to verify they pass**

```sh
cargo test -p beckon-core update::
```

Expected: 21 tests pass.

- [ ] **Step 5: Run the gate**

- [ ] **Step 6: Commit**

```sh
git commit --only crates/beckon-core/src/update.rs \
  -m "core(update): the state machine, and the invariant that a failed check cannot claim success"
git show --stat HEAD
```

---

## Task 4: Wire the row into About, and the command into the channel

This task adds a field to `AboutInputs` and a variant to `SettingsCommand`,
which **breaks every call site on purpose**. Repairing them minimally is part
of this task; the tree must compile on all six gate legs when it ends.

**Files:**
- Modify: `crates/beckon-core/src/settings.rs`
- Modify: `crates/beckon-macos/src/settings_window/mod.rs` (the `AboutInputs`
  construction)
- Modify: `crates/beckon-windows/src/settings_window/mod.rs` (the same)
- Modify: `crates/beckon-cli/src/serve.rs` (the exhaustive `on_command` match)
- Test: `crates/beckon-core/src/settings.rs`'s existing `mod tests`

**Interfaces:**
- Consumes: `update::{UpdateState, UpdateRow, update_row, detect_channel}`
- Produces: `AboutInputs.update: UpdateState`, `AboutState.update: UpdateRow`,
  `SettingsCommand::CheckForUpdates`

- [ ] **Step 1: Write the failing test**

Add inside `settings.rs`'s `mod tests`, beside the existing `about_with`
helper:

```rust
    /// `Channel` is NOT a new input. `about_state` derives it from the
    /// executable path `AboutInputs` already carries for the Location row, so
    /// there is one source for "where is this binary" rather than two that
    /// can drift.
    #[test]
    fn about_derives_the_channel_from_the_exe_path_it_already_has() {
        let page = about_update(&exe_path(), UpdateState::Done(Verdict::Available(
            crate::update::Version { major: 0, minor: 11, patch: 0 },
        )));
        assert_eq!(page.update.status.as_deref(), Some("0.11.0 available"));
        // exe_path() is the scoop shape.
        assert_eq!(
            page.update.command.map(|c| c.copy),
            Some("scoop update beckon".into())
        );
    }

    #[test]
    fn about_shows_no_update_line_before_a_check() {
        let page = about_update(&exe_path(), UpdateState::Idle);
        assert_eq!(page.update.status, None);
    }
```

and this helper beside `about_with`:

```rust
    fn about_update(exe: &std::path::Path, update: UpdateState) -> AboutState {
        about_state(AboutInputs {
            version: "0.9.3",
            target: "aarch64-pc-windows-msvc",
            exe: Some(exe),
            started: None,
            disk: ImageOnDisk::Unknown,
            identity: ImageIdentity::Same,
            licence: "MIT OR Apache-2.0",
            update,
        })
    }
```

Add `use crate::update::{UpdateState, Verdict};` to the test module's imports.

- [ ] **Step 2: Run the test to verify it fails**

```sh
cargo test -p beckon-core settings::
```

Expected: compile error — `AboutInputs` has no field `update`, and
`AboutState` has no field `update`.

- [ ] **Step 3: Add the field to `AboutInputs`**

In `crates/beckon-core/src/settings.rs`, add as the last field of
`AboutInputs`:

```rust
    /// What the caller knows about the update check right now.
    ///
    /// `Copy`, like every other field here -- `AboutInputs` derives `Copy`
    /// and `UpdateState`'s payloads are all `Copy`, so keep it that way.
    ///
    /// The CHANNEL is not a field: `about_state` derives it from `exe` above,
    /// so there is one source for "where is this binary" rather than two.
    pub update: crate::update::UpdateState,
```

- [ ] **Step 4: Add the field to `AboutState` and fill it in `about_state`**

Add as the last field of `AboutState`:

```rust
    /// The update check's line, its tone, and the upgrade command when there
    /// is one. Decided entirely by `crate::update::update_row`.
    pub update: crate::update::UpdateRow,
```

and in `about_state`'s constructed `AboutState`, add:

```rust
        update: crate::update::update_row(
            inputs.update,
            crate::update::detect_channel(inputs.exe),
        ),
```

- [ ] **Step 5: Add the `SettingsCommand` variant**

In `SettingsCommand`, after `ReloadNow`:

```rust
    /// About's `Check now`, and the tray row that opens this window on About.
    ///
    /// Carries nothing, which is what lets it stay in a `Copy + Eq` enum: the
    /// ANSWER travels back through `ServeState` and `refresh_settings`, the
    /// way pause and autostart already do.
    CheckForUpdates,
```

- [ ] **Step 6: Repair the call sites**

Three places stop compiling. Each gets the minimal correct change:

1. `crates/beckon-macos/src/settings_window/mod.rs` — find the `AboutInputs {
   … }` construction and add `update: <the state the caller holds>`. The
   caller does not hold one yet, so pass `beckon_core::update::UpdateState::Idle`
   with the comment `// Task 7 threads the real state through here.`
2. `crates/beckon-windows/src/settings_window/mod.rs` — the same.
3. `crates/beckon-cli/src/serve.rs` — the `on_command` match is exhaustive by
   design (*"every variant added later is a compile error at this one site"*).
   Add, for now:

```rust
                // Wired in Task 7. Kept as its own arm rather than folded
                // into a `_` so the compiler goes on naming this site.
                SettingsCommand::CheckForUpdates => {}
```

Also add `update: UpdateState::Idle` to the `about_with` helper in
`settings.rs`'s test module, which constructs `AboutInputs` too.

- [ ] **Step 7: Run the tests to verify they pass**

```sh
cargo test -p beckon-core
```

Expected: the two new tests pass, and every pre-existing `settings::` test
still passes.

- [ ] **Step 8: Run the gate**

All six legs. The cross-target clippy leg is the one that proves the Windows
call site was repaired — **`cargo check --target …` runs no lints and would
not**.

- [ ] **Step 9: Commit**

```sh
git commit --only crates/beckon-core/src/settings.rs \
  crates/beckon-macos/src/settings_window/mod.rs \
  crates/beckon-windows/src/settings_window/mod.rs \
  crates/beckon-cli/src/serve.rs \
  -m "core(settings): About carries the update row; SettingsCommand gains CheckForUpdates"
git show --stat HEAD
```

---

## Task 5: The spawn — the one place beckon reaches the network

**Files:**
- Create: `crates/beckon-cli/src/update.rs`
- Modify: `crates/beckon-cli/src/lib.rs`

**Interfaces:**
- Consumes: `beckon_core::update::{CurlOutcome, UpdateState, Version, interpret}`
- Produces: `pub fn fetch(current: Version) -> UpdateState`

There is no unit test here on purpose: every decision this module could make
already lives in `beckon_core::update::interpret`, which Task 3 tested. What
remains is a process spawn, and a test that spawns `curl` would be a network
test in CI. The Windows half is covered by the a14 probe in Task 10.

- [ ] **Step 1: Write the module**

Create `crates/beckon-cli/src/update.rs`:

```rust
//! The one place beckon reaches the network, and only when a person presses a
//! button.
//!
//! **No HTTP crate, deliberately.** `beckon-core` depends on `thiserror` and
//! `toml`; this crate adds `anyhow`, `clap`, `fs4` and `notify`. Adding a TLS
//! stack would put sixty crates into a build graph that was broken for a
//! month (v0.8.0 to v0.9.3) by one ungated `mod` -- and a crypto backend
//! needing a cross C toolchain is what stops
//! `cargo clippy --target aarch64-pc-windows-msvc` from resolving on the
//! author's Mac, which is a required gate leg.
//!
//! Shelling out is already the pattern here: `beckon_macos::shell` invokes
//! `/usr/bin/open`.
//!
//! **The 302 is the whole trick.** `github.com/xom11/beckon/releases/latest`
//! answers 302 with the tag in `Location`, so there is no JSON to parse and
//! no `api.github.com` rate limit (60/hour, unauthenticated) to hit.
//! Measured 2026-08-25 on macOS: 196 ms.

use beckon_core::update::{self, CurlOutcome, UpdateState, Version};
use std::process::Command;

const LATEST: &str = "https://github.com/xom11/beckon/releases/latest";

#[cfg(target_os = "windows")]
const NULL_SINK: &str = "NUL";
#[cfg(not(target_os = "windows"))]
const NULL_SINK: &str = "/dev/null";

/// `CREATE_NO_WINDOW`. `beckon-serve.exe` is GUI-subsystem, so without this a
/// console flashes on every check.
#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Where to look for curl, in order.
///
/// macOS: the absolute path only, matching the `/usr/bin/open` convention in
/// `beckon_macos::shell`. `/usr/bin/curl` ships with the OS.
///
/// Windows: the system copy first (predictable), then bare `curl` so a
/// Git-for-Windows or scoop curl on `PATH` still works. **Whether the system
/// copy exists on ARM64 Windows 11 is unmeasured** -- see the a14 probe. If
/// it does not, `fetch` returns `NoClient` and the About page says so, which
/// is designed for and tested.
fn candidates() -> Vec<std::ffi::OsString> {
    #[cfg(target_os = "windows")]
    {
        let root = std::env::var_os("SystemRoot")
            .unwrap_or_else(|| std::ffi::OsString::from(r"C:\Windows"));
        let mut system = std::path::PathBuf::from(root);
        system.push("System32");
        system.push("curl.exe");
        vec![system.into_os_string(), std::ffi::OsString::from("curl")]
    }
    #[cfg(not(target_os = "windows"))]
    {
        vec![std::ffi::OsString::from("/usr/bin/curl")]
    }
}

/// Ask GitHub which release is newest and compare it against `current`.
///
/// Blocks the calling thread for at most three seconds. Every failure mode
/// resolves to an `UpdateState::Failed`; none of them can produce
/// `Up to date` -- see `beckon_core::update::update_row`'s invariant test.
pub fn fetch(current: Version) -> UpdateState {
    for exe in candidates() {
        let mut cmd = Command::new(&exe);
        cmd.args([
            "-sS",
            // HEAD, and no -L: report the redirect instead of following it.
            "-I",
            "--connect-timeout",
            "2",
            "-m",
            "3",
            "-o",
            NULL_SINK,
            "-w",
            "%{redirect_url}",
            LATEST,
        ]);
        // No custom User-Agent, deliberately: curl sends its own, and
        // `beckon/0.10.0` would tell GitHub which build this user runs for no
        // reason the request needs. Proxies come free -- curl honours
        // http_proxy / https_proxy / no_proxy without beckon knowing they
        // exist.
        //
        // Spawned with `Command::new`, never through a shell, so the `%{...}`
        // format string raises no quoting question: no cmd.exe sees it.
        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }
        match cmd.output() {
            // Spawned. Whatever it did is now the answer -- do NOT fall
            // through to the next candidate, or a real network failure would
            // be retried as though the binary were missing.
            Ok(out) => {
                if !out.status.success() {
                    return update::interpret(CurlOutcome::Failed, current);
                }
                let url = String::from_utf8_lossy(&out.stdout);
                return update::interpret(CurlOutcome::Ok(url.trim()), current);
            }
            Err(_) => continue,
        }
    }
    update::interpret(CurlOutcome::NotSpawned, current)
}
```

- [ ] **Step 2: Declare the module**

In `crates/beckon-cli/src/lib.rs`, beside `mod serve;` (which carries the same
gate):

```rust
#[cfg(any(target_os = "macos", target_os = "windows"))]
mod update;
```

- [ ] **Step 3: Verify it compiles on both platforms**

```sh
cargo clippy --workspace --all-targets -- -D warnings
cargo clippy --target aarch64-pc-windows-msvc --all-targets -- -D warnings
```

Expected: clean. `fetch` is unused until Task 7 — if clippy warns
`dead_code`, add `#[allow(dead_code)]` **with a comment naming Task 7**, and
remove it in Task 7.

- [ ] **Step 4: Verify the invocation by hand**

```sh
/usr/bin/curl -sS -I --connect-timeout 2 -m 3 -o /dev/null \
  -w '%{redirect_url}\n' https://github.com/xom11/beckon/releases/latest
```

Expected: `https://github.com/xom11/beckon/releases/tag/v0.10.0`

- [ ] **Step 5: Run the gate**

- [ ] **Step 6: Commit**

```sh
git commit --only crates/beckon-cli/src/update.rs crates/beckon-cli/src/lib.rs \
  -m "cli(update): ask GitHub over the system curl, no HTTP crate"
git show --stat HEAD
```

---

## Task 6: `flush_paint`, and the push that carries `UpdateState` to About

Two small cross-platform primitives, in the two files that already hold the
window's public surface.

**`flush_paint`** — `refresh_settings` sets control text, but the frame is
painted by the message pump, which Task 7 is about to block for up to three
seconds. Without an explicit flush the window shows the *old* frame for that
whole time and reads as frozen.

**`set_update_state`** — and this one is a plan correction found in
pre-flight, not an original step. **`apply_about_state()` takes no arguments
and builds `AboutInputs` entirely from local sources** (`current_exe()`,
`fs::metadata`, `env!`), while `refresh_settings` pushes only `ControlState`
plus the System page's own second push. **There is no existing path from
`ServeState` to the About page**, so Tasks 8 and 9 as originally written
assumed plumbing that does not exist. It is built here.

The shape is the one `refresh_settings` already documents for the System
page — *"A second push, and design §1's split by store is why"* — and it
earns its place for the same reason: About must keep working in the
`unreadable_state` case, where there is no `Model` to project a
`ControlState` out of at all.

**Files:**
- Modify: `crates/beckon-macos/src/settings_window/mod.rs`
- Modify: `crates/beckon-windows/src/settings_window/mod.rs`

**Interfaces:**
- Consumes: `beckon_core::update::UpdateState` (Task 3),
  `AboutInputs.update` (Task 4)
- Produces, in **both** crates' `settings_window`, with identical names and
  signatures because `serve.rs` calls them through the `swin` alias:
  - `pub fn flush_paint()`
  - `pub fn set_update_state(update: beckon_core::update::UpdateState)`

- [ ] **Step 1: Add the Windows one**

In `crates/beckon-windows/src/settings_window/mod.rs`, beside the other
`pub fn`s that reach the window handle (`is_open`, `open_existing`):

```rust
/// Paint the pending frame NOW, rather than on the next pump turn.
///
/// Called immediately before `serve` blocks this thread on a network check.
/// `apply_state` has already invalidated; `UpdateWindow` is what makes the
/// `Checking...` line reach the screen before the block instead of after it.
///
/// A no-op when the window is closed, which is the right answer rather than
/// an error: the caller does not check first.
pub fn flush_paint() {
    // `hwnd()` takes and releases the `UI` borrow before we return, so
    // nothing is held across the paint -- the rule `open_existing` follows
    // one function above.
    if let Some(h) = hwnd() {
        unsafe {
            let _ = UpdateWindow(h);
        }
    }
}
```

`hwnd() -> Option<HWND>` already exists at `mod.rs:2337` and is what
`is_open` and `open_existing` both use — do not add a second handle store.

`UpdateWindow` comes from `windows::Win32::Graphics::Gdi`. If
`Win32_Graphics_Gdi` is not already in `beckon-windows`'s `windows` features,
add it: a feature on a crate that is already a dependency is not a new
dependency.

- [ ] **Step 2: Add the macOS one — and MEASURE it**

**This primitive is unverified.** `displayIfNeeded` on the content view is the
candidate; whether AppKit will paint without a run-loop turn has not been
measured. Do not assume it.

In `crates/beckon-macos/src/settings_window/mod.rs`:

```rust
/// Paint the pending frame NOW, rather than on the next run-loop turn.
///
/// The macOS half of the Windows `UpdateWindow` call: `serve` is about to
/// block this thread on a network check, and without this the window shows
/// the frame from BEFORE `Checking...` for the whole call.
pub fn flush_paint() {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let _ = mtm;
    // `controls()` releases the `UI` borrow before returning -- the rule
    // `open_existing` states two functions above, and painting can re-enter
    // this module through the delegate.
    let Some(c) = controls() else {
        return;
    };
    c.window.displayIfNeeded();
}
```

This mirrors `open_existing` (`mod.rs:1221`) exactly, down to the
`MainThreadMarker` guard and the released borrow — the only difference is
`displayIfNeeded` where it calls `raise`.

**`displayIfNeeded` is the candidate, not the answer.** Step 3 measures it. If
AppKit will not paint without a run-loop turn, spin the run loop once here
instead — and rewrite the doc comment above to say which it was and when it
was measured.

- [ ] **Step 3: Measure the macOS arm**

Build `beckon serve` into a **private** `CARGO_TARGET_DIR` — `target/debug/beckon`
is one path shared by every worktree, and this is a step whose output you must
believe:

```sh
CARGO_TARGET_DIR=/tmp/beckon-flush cargo build -p beckon-cli --bin beckon
/tmp/beckon-flush/debug/beckon --version   # warm it: the FIRST exec of a
/tmp/beckon-flush/debug/beckon --version   # freshly linked binary is killed
```

Then, with the settings window open on About, call `flush_paint` with a
deliberate `std::thread::sleep(Duration::from_secs(2))` after it and confirm
the new text is on screen *during* the sleep, not after.

**Run the control.** Comment the `flush_paint` call out and repeat: the text
must appear only after the sleep. A flush that does nothing and a flush that
works look identical without it — this repo has lost sessions to exactly that
shape.

Remove the temporary sleep before committing.

- [ ] **Step 4: Store the update state in each window's `UI`**

Both crates keep their live widgets in a thread-local `UI` (`UI.with(|u| …)`,
reached by `hwnd()` on Windows and `controls()` on macOS). Add one field to
each crate's `Ui` struct:

```rust
    /// What About should say about the update check.
    ///
    /// Pushed in by `serve` through `set_update_state` and read by
    /// `apply_about_state`, which builds every OTHER `AboutInputs` field
    /// from local sources -- `current_exe()`, `fs::metadata`, `env!`. This
    /// one cannot be local: the check runs in `serve`, not in the window.
    update: beckon_core::update::UpdateState,
```

Initialise it to `beckon_core::update::UpdateState::Idle` wherever `Ui` is
constructed.

- [ ] **Step 5: Add `set_update_state` to both crates**

```rust
/// Take the update check's latest answer and redraw About with it.
///
/// **A second push, and for the reason `refresh_settings` already gives for
/// the System page's:** About must keep working in the `unreadable_state`
/// case, where there is no `Model` to project a `ControlState` out of, so
/// riding on `apply_state` would make the update row hostage to a TOML
/// error.
///
/// A no-op when the window is closed -- the caller does not check first, the
/// way `refresh_settings` does not.
pub fn set_update_state(update: beckon_core::update::UpdateState) {
    // <Write the field through the same `UI.with(..)` accessor the file
    //  already uses, RELEASING the borrow before the redraw below --
    //  `apply_about_state` reaches `controls()` / `hwnd()` itself.>
    apply_about_state();
}
```

The borrow must be released before `apply_about_state()`. Mirror how a
neighbouring setter in the same file writes `UI` and then redraws; do not
hold the `RefCell` across the redraw.

- [ ] **Step 6: Read it in `apply_about_state`**

In both crates, `apply_about_state` currently passes Task 4's
`UpdateState::Idle` placeholder into `AboutInputs`. Replace it with the
stored field, read from `UI` in the same place the function already reads
`controls()` / the `Ui`.

Delete Task 4's `// Task 7 threads the real state through here.` comment — it
named the wrong task, and the plumbing is here.

- [ ] **Step 7: Run the gate**

Both clippy legs. The cross-target leg is what proves the Windows twin
compiles, and `cargo check --target …` would not — it runs no lints.

- [ ] **Step 8: Commit**

```sh
git commit --only crates/beckon-macos/src/settings_window/mod.rs \
  crates/beckon-windows/src/settings_window/mod.rs \
  -m "settings: flush_paint, and the second push that carries UpdateState to About"
git show --stat HEAD
```

---

## Task 7: The tray row and the serve wiring

**Files:**
- Modify: `crates/beckon-core/src/menu.rs`
- Modify: `crates/beckon-cli/src/serve.rs`
- Test: `crates/beckon-core/src/menu.rs` inline tests; `serve.rs`'s existing
  `mod tests` (which already has `build_entries` tests around line 2911)

**Interfaces:**
- Consumes: `update::{UpdateState, parse_current}` (Tasks 1, 3),
  `crate::update::fetch` (Task 5), `swin::flush_paint` (Task 6),
  `SettingsCommand::CheckForUpdates` (Task 4)
- Produces: `beckon_core::menu::update_label(macos: bool) -> &'static str`,
  `MENU_UPDATE: u32 = 8`, `MenuModel.macos: bool`,
  `ServeState.update: UpdateState`, `ServeState.pending_update_check: bool`

- [ ] **Step 1: Write the failing tests**

In `crates/beckon-core/src/menu.rs`, add a `#[cfg(test)] mod tests`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    /// Two spellings, one table. Platform strings are tables here, not
    /// literals -- and this is the shape `menu_log_row` already uses: the
    /// platform arrives as a parameter so both readings are testable on every
    /// CI job, not just on the machine that ships them.
    #[test]
    fn the_update_row_is_title_case_on_macos_only() {
        assert_eq!(update_label(true), "Check for Updates...");
        assert_eq!(update_label(false), "Check for updates...");
    }

    /// ASCII dots, not an ellipsis -- like every other display string here.
    #[test]
    fn both_update_labels_are_ascii() {
        assert!(update_label(true).is_ascii());
        assert!(update_label(false).is_ascii());
    }
}
```

In `serve.rs`'s `mod tests`, beside the existing `build_entries` tests:

```rust
    #[test]
    fn the_update_row_appears_next_to_settings() {
        let rows = build_entries(&MenuModel {
            phrase: "19 shortcuts".into(),
            paused: false,
            autostart: None,
            log: None,
            settings: true,
            macos: true,
        });
        let edit = rows.iter().position(|r| r.id == MENU_EDIT).unwrap();
        let update = rows.iter().position(|r| r.id == MENU_UPDATE).unwrap();
        assert_eq!(update, edit + 1, "the two window-opening rows sit together");
        assert_eq!(rows[update].label, "Check for Updates...");
        assert!(rows[update].enabled);
        assert_eq!(rows[update].checked, None);
    }

    /// The row opens the settings window, so a build without one must not
    /// draw it -- it would be a lie, and clicking it would do nothing.
    #[test]
    fn no_settings_window_means_no_update_row() {
        let rows = build_entries(&MenuModel {
            phrase: "19 shortcuts".into(),
            paused: false,
            autostart: None,
            log: None,
            settings: false,
            macos: false,
        });
        assert!(rows.iter().all(|r| r.id != MENU_UPDATE));
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

```sh
cargo test -p beckon-core menu::
cargo test -p beckon-cli
```

Expected: compile errors — `update_label`, `MENU_UPDATE`, and `MenuModel.macos`
do not exist.

- [ ] **Step 3: Add the label table**

In `crates/beckon-core/src/menu.rs`:

```rust
/// The `Check for updates` row's label, which differs by platform in case
/// only.
///
/// macOS title-cases menu items and Windows does not. **ASCII dots, not an
/// ellipsis**, like every other display string this program draws.
///
/// The platform arrives as a parameter rather than as a `cfg!` inside, for
/// the reason `menu_log_row` takes one: both readings are then compiled and
/// tested by all three CI jobs, not only by the one that ships them.
pub fn update_label(macos: bool) -> &'static str {
    if macos {
        "Check for Updates..."
    } else {
        "Check for updates..."
    }
}
```

- [ ] **Step 4: Add the menu row**

In `serve.rs`, after `const MENU_QUIT: u32 = 7;`:

```rust
const MENU_UPDATE: u32 = 8;
```

Add `macos: bool` to `MenuModel` with a doc comment:

```rust
    /// Which spelling the update row gets. Passed in rather than read from
    /// `cfg!` here so `build_entries` can be tested for both -- the same
    /// reason `log` arrives already decided by `menu_log_row`.
    macos: bool,
```

In `build_entries`, change the settings row to:

```rust
    if m.settings {
        entries.push(MenuEntry::item(MENU_EDIT, "Settings..."));
        // Directly after Settings: both rows open the same window, while
        // `Reload now` below is about the config file rather than about
        // beckon itself.
        entries.push(MenuEntry::item(
            MENU_UPDATE,
            beckon_core::menu::update_label(m.macos),
        ));
    }
```

In **both** `install_tray_menu` arms, add `macos: cfg!(target_os = "macos"),`
to the `MenuModel` construction.

- [ ] **Step 5: Update the macOS row-count comment**

`install_tray_menu`'s macOS doc comment opens *"Four rows against Windows'
seven"*. It is now **five against eight**. Change it. That sentence is
load-bearing for the next reader deciding whether an omission is structural.

- [ ] **Step 6: Add the `ServeState` fields**

```rust
    /// The update check's answer, for as long as this process runs. Session
    /// state on purpose -- nothing persists it and nothing refreshes it, so a
    /// reopened window starts at `Idle` again.
    update: beckon_core::update::UpdateState,
    /// Set by the tray's `Check for updates...` row and consumed by
    /// `open_settings`, which is what makes that row land on About with a
    /// check already running. Not set by the About page's own button, which
    /// reaches `check_for_updates` directly.
    pending_update_check: bool,
```

Initialise both wherever `ServeState` is constructed:
`update: beckon_core::update::UpdateState::Idle,` and
`pending_update_check: false,`.

Neither field needs a `cfg_attr(…, allow(dead_code))`: both are read on macOS
and on Windows, and Linux never compiles this module.

- [ ] **Step 7: Add `check_for_updates`**

In `serve.rs`, beside `set_paused` and `reload`:

```rust
/// Ask GitHub, and put the answer on the About page.
///
/// **Synchronous, and that is a decision.** `ServeState` is `Rc<RefCell<..>>`
/// and cannot cross a thread boundary, so a worker would mean `Arc`, a
/// channel, and a per-OS wake. On Windows that wake lands in the hazard at
/// `beckon-windows/src/settings_window/mod.rs`: the chain
/// `apply_state -> on_select -> refresh_settings -> apply_state` recurses
/// across an `extern "system"` boundary where a second `RefCell` borrow
/// ABORTS the process instead of unwinding, and a `PostMessageW` arriving
/// mid-`apply_state` is that exact shape. Three seconds of worst case on a
/// button the user just pressed is the cheaper trade. Measured 196 ms.
///
/// Every borrow is dropped before the spawn -- the discipline
/// `on_probe_shortcut` states in this file and `MENU_LOG` follows before
/// `open_path`.
fn check_for_updates(state: &Rc<RefCell<ServeState>>) {
    use beckon_core::update::{self, CheckError, UpdateState};

    // A version string this build cannot parse is a fact about beckon, not
    // about the network -- so it never reaches curl.
    let Some(current) = update::parse_current(env!("BECKON_VERSION")) else {
        state.borrow_mut().update = UpdateState::Failed(CheckError::Unreadable);
        refresh_settings(state);
        return;
    };

    state.borrow_mut().update = UpdateState::Checking;
    refresh_settings(state);
    // The frame must reach the screen BEFORE the block, or the window shows
    // the previous one for the whole call and reads as frozen.
    swin::flush_paint();

    let outcome = crate::update::fetch(current);
    state.borrow_mut().update = outcome;
    refresh_settings(state);
}
```

- [ ] **Step 7b: Push the state to the window from `refresh_settings`**

`check_for_updates` calls `refresh_settings`, and that is what must reach
About. In `refresh_settings`, read the field in the SAME borrow as
`external`/`catalog`/`paused` above `drop(s)`:

```rust
    let update = s.update;
```

and push it after `drop(s)`, beside the System page's own second push:

```rust
    // The About page's second push, and the System page's reason exactly:
    // About must keep working in the `unreadable_state` case, where there is
    // no `Model` to project a `ControlState` out of.
    swin::set_update_state(update);
```

`UpdateState` is `Copy`, so this is a read rather than a clone.

- [ ] **Step 8: Fill the `on_command` arm**

Replace Task 4's empty arm:

```rust
                SettingsCommand::CheckForUpdates => check_for_updates(&st),
```

- [ ] **Step 9: Add the tray arm in both `install_tray_menu`s**

```rust
            // The row opens the window rather than answering here: the answer
            // needs a Copy button beside it, which a menu row cannot carry.
            MENU_UPDATE => {
                st.borrow_mut().pending_update_check = true;
                open_settings(&st, &mg);
            }
```

- [ ] **Step 10: Consume the flag in `open_settings`**

At the very top of `open_settings`, before the `swin::is_open()` check:

```rust
    // Hoisted out of the `if` on purpose. A `RefCell` borrow inside an `if`
    // CONDITION lives until the end of the whole `if` statement, so
    // `if std::mem::take(&mut state.borrow_mut().pending_update_check) { .. }`
    // would hold the borrow across `check_for_updates` and panic on its first
    // `borrow_mut`.
    let wanted_check = std::mem::take(&mut state.borrow_mut().pending_update_check);
    if wanted_check {
        // Where the NEXT open lands -- which is this one.
        state.borrow_mut().settings_page = beckon_core::settings::Page::About;
    }
```

and at **both** exits — the `is_open()` early return and the end of the
function after `swin::open(..)`:

```rust
    if wanted_check {
        check_for_updates(state);
    }
```

**Known limitation, and it is deliberate.** `settings_page` is *"where the
next open lands"*, so a window that is ALREADY open is raised without moving
to About. The check still runs and the About row still updates; the user may
have to click About to see it. Adding a page-switch API to both window crates
to close this is scope the tray row does not justify — the row exists mostly
for when the window is shut.

- [ ] **Step 11: Remove Task 5's `#[allow(dead_code)]`**

If Task 5 added one to `fetch`, delete it — `check_for_updates` is its caller
now.

- [ ] **Step 12: Run the tests to verify they pass**

```sh
cargo test -p beckon-core
cargo test -p beckon-cli   # run TWICE: the first exec is SIGKILLed here
```

- [ ] **Step 13: Run the gate**

- [ ] **Step 14: Commit**

```sh
git commit --only crates/beckon-core/src/menu.rs crates/beckon-cli/src/serve.rs \
  -m "serve: the tray's Check for updates row, and the synchronous check behind it"
git show --stat HEAD
```

---

## Task 8: The About page on macOS

**Files:**
- Modify: `crates/beckon-macos/src/settings_window/about.rs`
- Modify: `crates/beckon-macos/src/settings_window/mod.rs`

**Interfaces:**
- Consumes: `AboutState.update: UpdateRow` (Task 4),
  `SettingsCommand::{CheckForUpdates, Copy}` (Task 4)
- Produces: the drawn rows. Nothing downstream depends on this task.

**This plan deliberately does not invent widget calls for this file.** Step 1
is to read it; the steps after specify exactly what to add, not how the
toolkit spells it.

- [ ] **Step 1: Read the file and identify three things**

Read `crates/beckon-macos/src/settings_window/about.rs` in full and write down:

1. How a value row with a `Copy` button is constructed (the `location` row is
   the model to copy — it is the one with both an `AboutValue` and a button).
2. How a button press maps to a `SettingsCommand` (the existing Copy buttons
   raise `SettingsCommand::Copy(Field)`).
3. Where the page's vertical layout constants live, so a new row shifts what
   follows it rather than overlapping.

- [ ] **Step 2: Nothing to plumb — Task 6 did it**

`AboutState.update` already arrives populated: Task 6 added the `Ui` field,
`set_update_state`, and the read inside `apply_about_state`, and Task 7's
`refresh_settings` pushes into it. This task is rendering only.

Confirm it before drawing: `apply_about_state` must be passing the stored
field into `AboutInputs`, not `UpdateState::Idle`. If it still passes `Idle`,
that is a Task 6 defect — fix it here and say so in the report.

- [ ] **Step 3: Draw the status line**

Below the existing Version row, draw `state.update.status` when it is
`Some(_)` and nothing at all when it is `None`. Colour it by
`state.update.tone`: `FlagTone::Warn` uses whatever the page already uses for
a warning, `FlagTone::Neutral` is ordinary body text.

- [ ] **Step 4: Add the `Check now` button**

Beside the status line. Enabled iff `state.update.can_check`. Pressing it
raises `SettingsCommand::CheckForUpdates`.

- [ ] **Step 5: Draw the command row**

When `state.update.command` is `Some(cmd)`, draw `cmd.shown` with a `Copy`
button that puts `cmd.copy` — **not `cmd.shown`** — on the clipboard. The two
differ, and that is the entire reason `AboutValue` has two fields.

- [ ] **Step 6: Add the `Open releases page` button**

It raises `SettingsCommand::Open(Target::Releases)`, which is already wired
end to end (`serve.rs`'s `open_target` handles it, and `Target::Releases`
already resolves to `https://github.com/xom11/beckon/releases`). Draw it
whenever `state.update.status` is `Some(_)` — including every failure state,
which is what gives a user with no curl somewhere to go.

- [ ] **Step 7: Drive it live**

```sh
CARGO_TARGET_DIR=/tmp/beckon-about cargo build -p beckon-cli --bin beckon
/tmp/beckon-about/debug/beckon --version    # warm it (first exec is killed)
/tmp/beckon-about/debug/beckon --version
```

Open the settings window on About and press `Check now`. Confirm, in order:
`Checking...` appears **before** the pause, then a verdict, then the command
row with a working Copy.

Then force each failure: rename `/usr/bin/curl` out of the way is **not** an
option — instead temporarily point `LATEST` at `https://127.0.0.1:1/` to get
`Unreachable`, and at `https://example.com/` to get `Unreadable`. Confirm
neither says `Up to date`. Restore `LATEST` afterwards.

- [ ] **Step 8: Run the gate**

- [ ] **Step 9: Commit**

```sh
git commit --only crates/beckon-macos/src/settings_window/about.rs \
  crates/beckon-macos/src/settings_window/mod.rs \
  -m "macos(about): the update row, its command and its Copy"
git show --stat HEAD
```

---

## Task 9: The About page on Windows

**Files:**
- Modify: `crates/beckon-windows/src/settings_window/` — the About page's
  layout, paint and control-id modules

**Interfaces:**
- Consumes: the same as Task 8
- Produces: the drawn rows

- [ ] **Step 1: Read the About page's four concerns**

Read how the Windows settings window builds About: the control ids (`ids`),
the layout arithmetic (`layout`), the painting (`paint`), and the `WM_COMMAND`
routing in `mod.rs`. Identify the `location` row and its Copy button — the
model to mirror, for the same reason as Task 8.

- [ ] **Step 2: Nothing to plumb — Task 6 did it**

As in Task 8: `AboutState.update` arrives populated. Confirm
`apply_about_state` passes the stored `Ui` field rather than
`UpdateState::Idle`, then render.

- [ ] **Step 3-6: The four controls**

Same four as Task 8, in the Windows idiom: the status line coloured by
`tone`, a `Check now` button enabled iff `can_check`, the command row with a
Copy bound to `cmd.copy`, and `Open releases page` raising
`SettingsCommand::Open(Target::Releases)`.

Two Windows-specific traps this repo has already paid for:

- **A fill and its ink must not share one `GetSysColor` index.** Five such
  collisions were found on the settings redesign branch and no compiler
  catches any of them — they render as invisible text under High Contrast.
  Check the pair you pick for the `Warn` tone.
- **The paint handler must not borrow `UI`.** If the status line is drawn
  through custom draw, the existing handler's borrow discipline applies
  unchanged.

- [ ] **Step 7: Cross-check from this Mac**

```sh
cargo clippy --target aarch64-pc-windows-msvc --all-targets -- -D warnings
cargo check --workspace --all-targets
```

`cargo check --target …` alone is blind to every Windows-only clippy error CI
will hit — the clippy leg is the one that matters here.

- [ ] **Step 8: Run the gate**

- [ ] **Step 9: Commit**

```sh
git commit --only crates/beckon-windows/src/settings_window/ \
  -m "windows(about): the update row, its command and its Copy"
git show --stat HEAD
```

---

## Task 10: The a14 probe, and the documentation

**Files:**
- Modify: `CLAUDE.md`
- Modify: `docs/notes/distribution.md`
- Modify: `crates/beckon-cli/src/update.rs` (record the probe result in
  `candidates`' doc comment)

- [ ] **Step 1: Probe a14 over SSH — the two questions SSH can answer**

```powershell
Test-Path C:\Windows\System32\curl.exe
C:\Windows\System32\curl.exe -sS -I --connect-timeout 2 -m 3 -o NUL `
  -w '%{redirect_url}' https://github.com/xom11/beckon/releases/latest
```

Use `-EncodedCommand` to dodge quoting. Expected: `True`, then
`https://github.com/xom11/beckon/releases/tag/v0.10.0`.

If `Test-Path` is `False`, that is a **result, not a blocker**: `fetch`
returns `NoClient`, About says `Could not check - no HTTP client found`, and
no code changes. Record it and move on.

- [ ] **Step 2: Probe the console flash — this one needs a desktop**

SSH to a14 lands in **session 0**, which has no desktop, so a console-window
question answered there is a confident false negative. Run this through a
scheduled task in session 1.

Press `Check now` and watch for a console flash. Then **run the control**:
rebuild with `creation_flags` commented out and confirm a flash *does* appear.
A check that never flashes because the spawn silently failed looks identical
to one that never flashes because the flag worked.

- [ ] **Step 3: Record the result in the code**

Replace the "unmeasured" paragraph in `candidates`' doc comment in
`crates/beckon-cli/src/update.rs` with what the probe actually found, dated
and naming the machine — the convention every measured claim in this repo
follows.

- [ ] **Step 4: Update `CLAUDE.md`**

Two places:

1. **"What beckon reads and writes"** — add that beckon now makes exactly one
   outbound request, only on a button press, only from `serve`'s settings
   window, and that it downloads and writes nothing.
2. **"Out of scope (explicitly)"** — add self-update and background update
   polling, with the one-line reason (the binary lives in a read-only store or
   under a package manager's junction).

- [ ] **Step 5: Update `docs/notes/distribution.md`**

Add a section recording: the 302 trick and why it beats `api.github.com`; the
196 ms measurement with its date and platform; the channel-detection needle
table and why `/usr/local/bin` is not one of them; and the a14 probe result
from Step 1.

- [ ] **Step 6: Run the gate**

- [ ] **Step 7: Commit**

```sh
git commit --only CLAUDE.md docs/notes/distribution.md crates/beckon-cli/src/update.rs \
  -m "docs: the update check reaches the network once, on a press, and writes nothing"
git show --stat HEAD
```

- [ ] **Step 8: Verify the branch**

```sh
git branch --show-current          # feat/check-for-updates
git log --oneline origin/main..HEAD
```

Expected: ten commits. `git branch --show-current` before anything else — an
empty push and a real one print the same line, and `git log --oneline -1`
never names the branch you are on.

---

## Coverage against the spec

| Spec section | Task |
|---|---|
| §2 not an updater / not a poll / no CLI verb / not on Linux | Global Constraints; Task 10 Step 4 records it |
| §3 measured facts | Task 5 Step 4 re-verifies; Task 10 for the Windows half |
| §4 curl over an HTTP crate; §4.1 the invocation; §4.2 exit mapping | Task 5; mapping tested in Task 3 |
| §5.1 Version, `Ahead` | Task 1 |
| §5.2 channel detection, unresolved path | Task 2 |
| §5.3 upgrade commands, `shown` ≠ `copy` | Task 2 |
| §5.4 `UpdateState`, `UpdateRow`, `Option<String>`, derived `Channel` | Tasks 3, 4 |
| §6.1 tray row, gating, placement, label table, row-count comment | Task 7 |
| §6.2 About row, all six states | Tasks 8, 9 |
| §7 synchronous, borrow discipline, `flush_paint` | Tasks 6, 7 |
| §8 never claim up to date | Task 3 (the invariant test) |
| §9 unit tests; a14 probe with its control | Tasks 1-3, 7; Task 10 |
| §10.1 curl on ARM64 | Task 10 Step 1 |
| §10.2 the AppKit primitive | Task 6 Step 3, with a control |
| §10.3 does `FlagTone` fit | **Resolved**: it does. `UpdateRow.tone` uses `Warn`/`Neutral` directly and does NOT extend `flag_tone`'s closed four-word vocabulary |
| §10.4 does `refresh_settings` repaint About | Task 8 Step 7 answers it live; if it does not, that is a Task 8 defect to fix there |
| §11 files | matches, plus `menu.rs` for the label table |
| §12 rejected alternatives | recorded in module docs (Task 5) and `distribution.md` (Task 10) |
