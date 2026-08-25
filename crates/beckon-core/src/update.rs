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

use crate::settings::{AboutValue, FlagTone};

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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn v(major: u64, minor: u64, patch: u64) -> Version {
        Version {
            major,
            minor,
            patch,
        }
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
        // The last segment is a valid triple, but the path is not a release
        // tag. This is the case a captive portal or a CDN error page
        // produces, and reading it as a release would report a version that
        // does not exist.
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
        assert_eq!(
            compare(v(0, 11, 0), v(0, 10, 0)),
            Verdict::Ahead(v(0, 10, 0))
        );
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

    /// The five outcomes, in both separator styles. A Windows path reaches
    /// this function as ONE `Path` component on the two CI jobs that are not
    /// Windows -- which is exactly why the function works on the string form
    /// and normalises separators itself.
    #[test]
    fn the_channel_comes_out_of_the_path_the_binary_was_invoked_as() {
        let cases: &[(&str, Channel)] = &[
            ("/nix/store/abc123-beckon-0.10.0/bin/beckon", Channel::Nix),
            (
                r"C:\Users\me\scoop\apps\beckon\current\beckon-serve.exe",
                Channel::Scoop,
            ),
            (
                "/home/me/scoop/apps/beckon/current/beckon-serve.exe",
                Channel::Scoop,
            ),
            ("/opt/homebrew/bin/beckon", Channel::Homebrew),
            (
                "/usr/local/Cellar/beckon/0.10.0/bin/beckon",
                Channel::Homebrew,
            ),
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
            detect_channel(Some(Path::new(
                r"C:\Users\Me\Scoop\Apps\Beckon\current\beckon-serve.exe"
            ))),
            Channel::Scoop
        );
    }

    /// `/usr/local/bin` is deliberately NOT a Homebrew needle: it is far too
    /// broad and would claim a hand-copied binary for a package manager that
    /// never saw it.
    #[test]
    fn an_unrecognised_location_is_unknown_not_a_guess() {
        assert_eq!(
            detect_channel(Some(Path::new("/usr/local/bin/beckon"))),
            Channel::Unknown
        );
        assert_eq!(
            detect_channel(Some(Path::new("/home/me/Downloads/beckon"))),
            Channel::Unknown
        );
        assert_eq!(detect_channel(None), Channel::Unknown);
    }

    /// `shown` may carry a caveat; `copy` is the bare payload, because what a
    /// user does with it is paste it into a terminal. This is the whole
    /// reason `AboutValue` has two fields.
    #[test]
    fn the_clipboard_half_is_a_command_and_nothing_else() {
        for ch in [
            Channel::Nix,
            Channel::Scoop,
            Channel::Homebrew,
            Channel::Cargo,
        ] {
            let cmd = upgrade_command(ch).expect("every known channel has a command");
            assert!(!cmd.copy.contains(" - "), "{ch:?}: copy carries a caveat");
            assert!(
                !cmd.copy.contains('('),
                "{ch:?}: copy carries a parenthetical"
            );
            assert!(
                cmd.shown.starts_with(cmd.copy.as_str()),
                "{ch:?}: shown must lead with the command"
            );
            assert!(
                cmd.copy.is_ascii(),
                "{ch:?}: display strings are ASCII here"
            );
            assert!(
                cmd.shown.is_ascii(),
                "{ch:?}: display strings are ASCII here"
            );
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
        let avail = update_row(
            UpdateState::Done(Verdict::Available(v(0, 11, 0))),
            Channel::Scoop,
        );
        assert_eq!(avail.status.as_deref(), Some("0.11.0 available"));
        assert_eq!(
            avail.command.map(|c| c.copy),
            Some("scoop update beckon".into())
        );

        assert!(
            update_row(UpdateState::Done(Verdict::UpToDate), Channel::Scoop)
                .command
                .is_none()
        );
        assert!(update_row(
            UpdateState::Done(Verdict::Ahead(v(0, 9, 9))),
            Channel::Scoop
        )
        .command
        .is_none());
    }

    /// An unknown channel still gets the news, just not a command.
    #[test]
    fn an_available_update_on_an_unknown_channel_still_reports_the_version() {
        let row = update_row(
            UpdateState::Done(Verdict::Available(v(0, 11, 0))),
            Channel::Unknown,
        );
        assert_eq!(row.status.as_deref(), Some("0.11.0 available"));
        assert!(row.command.is_none());
    }

    /// `Ahead` says what is true and offers nothing -- an upgrade command
    /// here would move the user backwards.
    #[test]
    fn a_build_ahead_of_the_release_says_so_plainly() {
        let row = update_row(
            UpdateState::Done(Verdict::Ahead(v(0, 9, 9))),
            Channel::Homebrew,
        );
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
}
