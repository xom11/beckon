//! The one cross-OS word for how sure a name resolution is.
//!
//! Every backend already owns its own `MatchType` — five variants on macOS,
//! four on Windows, four on Linux — and each has exactly **one** substring
//! variant. That is the line this enum draws, and the code draws it, not a
//! judgement call.
//!
//! It lives in `beckon-core` for two reasons. `beckon-core` is excluded from no
//! CI runner, so the per-OS mappings onto it are checked on all three
//! platforms. And this is the vocabulary a future per-binding `match` floor
//! consumes — `match = "exact"` will mean "refuse `Guess`" — so a second,
//! per-OS spelling of the same idea is exactly the thing that would drift.
//!
//! `NoMatch` rather than `None`: this enum is matched inside functions that
//! also match `Option`, and two `None` patterns a line apart is a reading trap
//! for no gain.
//!
//! `ColdPath` is here too and is deliberately **not** a fourth `Certainty`:
//! it answers a different question — not how sure one resolution is, but
//! whether there are two of them.

/// How sure a resolution is, in the only three grades that matter to a user.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Certainty {
    /// Equality — against a display name, a bundle id, an AUMID, an exe stem,
    /// a `.desktop` filename or a window class.
    Exact,
    /// A substring match. Often right, silently wrong, and the tier that
    /// forces a full catalog scan on every OS.
    Guess,
    /// Nothing in the installed-app catalog claims this id.
    NoMatch,
}

impl Certainty {
    /// The word for this grade. `Summary::line()` is the one caller today;
    /// `check --resolve`'s per-binding rows print `tier` instead, since a
    /// tier says which of an OS's several substring rules fired, which is
    /// strictly more than this three-way grade names.
    pub fn word(self) -> &'static str {
        match self {
            Certainty::Exact => "exact",
            Certainty::Guess => "guess",
            Certainty::NoMatch => "no match",
        }
    }
}

/// What one app name resolved to on this machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NameReport {
    /// The name exactly as the config spells it.
    pub id: String,
    pub certainty: Certainty,
    /// What it resolved to — bundle id, AUMID or exe, `.desktop` id. `None`
    /// when `certainty` is `NoMatch`.
    ///
    /// Nothing prints this today. It exists as the resolved identity itself
    /// — the thing `certainty` and `tier` are *about* — and each backend's
    /// tests assert it as half of the one-report-per-name contract: a given
    /// name resolves to exactly one target, or it does not resolve at all.
    pub target: Option<String>,
    /// The backend's own words: `MatchType::describe()`. Displayed, never
    /// parsed — which is why it is a borrowed `&'static str` and not an enum
    /// this crate would then have to keep in step with three others.
    pub tier: Option<&'static str>,
    /// What a keypress does given this certainty, **on this OS**. Free text
    /// because the answer genuinely differs: on a miss macOS errors, Windows
    /// falls through to exe-name and window-title matching, and Linux treats
    /// the raw id as a window class and can still focus a live window. One
    /// shared sentence would be wrong on two platforms out of three.
    ///
    /// Empty when there is nothing to warn about.
    pub consequence: String,
    /// Other names worth looking at, already truncated by whoever produced it.
    pub suggestions: Vec<String>,
    /// What the SAME name resolves to with the running apps taken away, when
    /// that is not what `target` says. `None` is "nothing to report".
    ///
    /// **On Linux and Windows that is every report, structurally.**
    /// `desktop::resolve_reports_in` takes only `.desktop` entries and
    /// `apps::resolve_reports_in` only the Start-menu catalog: neither
    /// resolver consults running processes at all, so there is no second
    /// answer for a cold pass to differ from. Only macOS starts its ladder at
    /// `NSWorkspace.runningApplications`, and only macOS fills this in.
    pub cold: Option<ColdPath>,
}

/// What a name resolves to when its app is NOT running, for the reports whose
/// answer came from a running app in the first place.
///
/// This is the launch half of focus-or-launch, and it is a **separate signal
/// from `Certainty`, not a fourth grade of it**. Both answers can be `Exact`
/// — two bundles sharing one display name each match a whole string — so the
/// enum that grades one resolution has nothing wrong to say. What is missing
/// is that there are two resolutions.
///
/// Measured on macmini 2026-09-08: `com.nousresearch.hermes.setup` (an
/// installer stub in `/Applications`) and `com.nousresearch.hermes` (the app
/// it installed) both answer to the name *Hermes*. While the app runs, tier 1
/// finds it; once it quits, tier 3 finds the installer. Same config, opposite
/// results, and nothing said so.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ColdPath {
    /// A different target answers when the app is not running. The key
    /// focuses one app and launches another, and which one it does is decided
    /// by whether the app happens to be up.
    Elsewhere {
        /// The cold answer's identity, in the same vocabulary as `target`.
        target: String,
        /// The tier that produced it — the backend's own `describe()`.
        tier: &'static str,
    },
    /// Nothing answers when the app is not running: the bundle lives
    /// somewhere the installed-app scan does not walk. The key works only
    /// while the app is already up, which is the one state in which the user
    /// is least likely to press it.
    Nowhere,
}

/// Counts of each `Certainty` across a batch of `NameReport`s.
///
/// Built and tested, but nothing prints this today — `check --resolve`
/// prints its problem blocks (`unresolved_report`, `guess_report`,
/// `elsewhere_report`, `running_only_report`) and stops there. Only the first
/// two are keyed on `Certainty` at all; the last two are keyed on `ColdPath`,
/// which is why this summary would not describe them even if it were printed.
/// `Summary` and `line()` exist for the per-binding `match`
/// floor the spec describes as the next step (`match = "exact"` refusing a
/// file with any `Guess` in it), which needs the count of each grade to
/// decide anything.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Summary {
    pub exact: usize,
    pub guess: usize,
    pub no_match: usize,
}

impl Summary {
    /// One sentence naming every count, in `Certainty::word()` order.
    ///
    /// Not printed by anything shipped — see the struct doc. Calls
    /// `Certainty::word()` rather than repeating "exact" / "guess" /
    /// "no match" as literals, so the two cannot drift: a fourth variant
    /// changes `word()` once and this line follows without a second edit.
    pub fn line(&self) -> String {
        let total = self.exact + self.guess + self.no_match;
        if total == 0 {
            return "nothing to resolve".to_string();
        }
        if self.guess == 0 && self.no_match == 0 {
            return format!("all {} {}", self.exact, Certainty::Exact.word());
        }
        let mut parts = vec![format!("{} {}", self.exact, Certainty::Exact.word())];
        if self.guess > 0 {
            parts.push(format!("{} {}", self.guess, Certainty::Guess.word()));
        }
        if self.no_match > 0 {
            parts.push(format!("{} {}", self.no_match, Certainty::NoMatch.word()));
        }
        parts.join(", ")
    }
}

/// Tally a batch of reports into a `Summary`. The `match` here is exhaustive
/// on purpose: it is the compiler nudge that forces a decision in this
/// shared crate, rather than in only two of the three backends, if a fourth
/// `Certainty` variant is ever added.
pub fn summarize(reports: &[NameReport]) -> Summary {
    let mut s = Summary::default();
    for r in reports {
        match r.certainty {
            Certainty::Exact => s.exact += 1,
            Certainty::Guess => s.guess += 1,
            Certainty::NoMatch => s.no_match += 1,
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report(id: &str, certainty: Certainty) -> NameReport {
        NameReport {
            id: id.to_string(),
            certainty,
            target: None,
            tier: None,
            consequence: String::new(),
            suggestions: Vec::new(),
            cold: None,
        }
    }

    // ---------- word ----------

    /// ASCII, exhaustively. Every columnar beckon output assumes a narrow
    /// glyph; a wide one shifts every following column on the row it
    /// appears in.
    #[test]
    fn words_are_ascii() {
        for c in [Certainty::Exact, Certainty::Guess, Certainty::NoMatch] {
            assert!(c.word().is_ascii(), "{c:?}");
        }
    }

    #[test]
    fn no_two_certainties_share_a_word() {
        let words = [
            Certainty::Exact.word(),
            Certainty::Guess.word(),
            Certainty::NoMatch.word(),
        ];
        let mut sorted = words;
        sorted.sort_unstable();
        let mut deduped = sorted.to_vec();
        deduped.dedup();
        assert_eq!(deduped.len(), words.len(), "{words:?}");
    }

    // ---------- summary ----------

    /// `cold` is a second signal, not a worse grade. A binding that resolves
    /// exactly and diverges when the app is not running still counts as
    /// `exact` — which is what keeps `check --resolve` exiting 0 over it, the
    /// same call already made for `Guess`. Tally it as anything else and a
    /// warning becomes a failure, which is how a check stops being run.
    #[test]
    fn a_cold_divergence_does_not_change_the_grade() {
        let mut r = report("Hermes", Certainty::Exact);
        r.cold = Some(ColdPath::Elsewhere {
            target: "com.nousresearch.hermes.setup".to_string(),
            tier: "installed app name (exact)",
        });
        let s = summarize(&[r]);
        assert_eq!((s.exact, s.guess, s.no_match), (1, 0, 0));
    }

    #[test]
    fn summarize_counts_each_variant() {
        let rs = vec![
            report("a", Certainty::Exact),
            report("b", Certainty::Guess),
            report("c", Certainty::Exact),
            report("d", Certainty::NoMatch),
        ];
        let s = summarize(&rs);
        assert_eq!((s.exact, s.guess, s.no_match), (2, 1, 1));
    }

    /// If this line is ever printed, it has to carry the whole picture on
    /// its own — without the exact count a reader cannot tell twenty
    /// bindings from two.
    #[test]
    fn line_reports_every_count_when_something_is_wrong() {
        let s = Summary {
            exact: 18,
            guess: 2,
            no_match: 1,
        };
        assert_eq!(s.line(), "18 exact, 2 guess, 1 no match");
    }

    #[test]
    fn line_omits_a_category_with_no_members() {
        let s = Summary {
            exact: 5,
            guess: 0,
            no_match: 2,
        };
        assert_eq!(s.line(), "5 exact, 2 no match");
    }

    /// The complement of the test above. Without it, coupling the guess push
    /// to `no_match > 0` passes every other test in this module while
    /// silently dropping the guess count — which is the one number
    /// `check --resolve` exists to surface.
    #[test]
    fn line_omits_no_match_when_there_are_none() {
        let s = Summary {
            exact: 5,
            guess: 3,
            no_match: 0,
        };
        assert_eq!(s.line(), "5 exact, 3 guess");
    }

    #[test]
    fn line_says_so_plainly_when_nothing_is_wrong() {
        let s = Summary {
            exact: 20,
            guess: 0,
            no_match: 0,
        };
        assert_eq!(s.line(), "all 20 exact");
    }

    /// An empty shortcuts file parses fine, so this line is reachable.
    /// "all 0 exact" would be a true sentence that reads like a bug.
    #[test]
    fn line_on_an_empty_file_says_there_was_nothing_to_do() {
        assert_eq!(Summary::default().line(), "nothing to resolve");
        assert_eq!(summarize(&[]).line(), "nothing to resolve");
    }
}
