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
/// Takes the last path segment and strips a leading `v` when there is one.
/// Everything that is not a release tag -- an empty string (curl printed no
/// redirect), a captive portal's login page, the releases index itself --
/// falls out as `None`, which the caller must report as a failed check and
/// never as success.
pub fn parse_tag(redirect_url: &str) -> Option<Version> {
    let tag = redirect_url.trim().rsplit('/').next()?;
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
