# `beckon check --resolve` Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make `beckon check` able to tell you, per binding, which tier each app
name matches at on the machine you are sitting at — so the knowledge currently
living in hand-written config comments becomes a command.

**Architecture:** One new `beckon-core` module (`certainty`) defines the
cross-OS vocabulary — `Certainty { Exact, Guess, NoMatch }` and `NameReport`.
Each OS crate maps its own private `MatchType` onto that vocabulary and gains a
single new public function, `resolve_reports(ids) -> Result<Vec<NameReport>>`,
which resolves N names against **one** catalog scan by calling the same resolver
the existing `beckon resolve` report calls. The CLI adds a `--resolve` flag to
the `check` subcommand and one table renderer shared by all three platforms.
`print_resolve_report` is not touched anywhere.

**Tech Stack:** Rust 2021 workspace; `clap` 4 derive (CLI); no new dependencies
in any crate.

**Spec:** `docs/superpowers/specs/2026-08-14-check-resolve-design.md`

## Global Constraints

- **CI runs four jobs** (`.github/workflows/ci.yml`): `rustfmt` on ubuntu, and
  `build & test` on `ubuntu-latest` / `macos-latest` / `windows-latest` with
  per-OS excludes. Every job must stay green.
- **Per-OS exclude flags, copied verbatim** — a local gate must use the same
  shape or it cannot pass:
  - ubuntu: `--exclude beckon-macos --exclude beckon-windows`
  - macos: `--exclude beckon-linux --exclude beckon-windows`
  - windows: `--exclude beckon-linux --exclude beckon-macos`
- **The three cargo commands CI runs**, with those flags interpolated:
  `cargo build --workspace <excl> --all-targets`,
  `cargo test --workspace <excl>`,
  `cargo clippy --workspace <excl> --all-targets -- -D warnings`.
  Note `cargo test` does **not** get `--all-targets`; build and clippy do.
- **`beckon-core` and `beckon-cli` are excluded on no runner.** Anything added
  there must compile *and* its unit tests must pass on Linux, macOS and Windows.
  There is no `cfg` escape hatch. This is why `Certainty` lives in
  `beckon-core`.
- **`beckon-core` stays dependency-light**: its only dependencies are
  `thiserror`, `toml`, `toml_edit`. Do not add `anyhow` or `clap` to it.
- **`cargo test -p beckon-linux` on a macOS host runs 0 tests and prints `ok`.**
  Green there is not evidence. Linux code is verified by CI only.
- **Cross-check Windows from macOS** with
  `cargo check --target aarch64-pc-windows-msvc --all-targets` (works without
  MSVC installed; `--all-targets` is what compiles `examples/` at all).
- **`beckon-cli` integration tests occasionally die with `signal: 9` locally**
  with empty stderr and a rotating victim; each passes when run alone. It is
  environmental — re-run before believing a failure, and treat CI as the
  authority.
- **Growth rule (CLAUDE.md):** new capabilities are flags on an existing verb,
  never a new top-level verb, and **no aliases, ever**.
- **`beckon check`'s existing output must not change when `--resolve` is
  absent.** `crates/beckon-cli/tests/check.rs` asserts the exact string
  `ok: 1 shortcuts`, and one of its tests exists specifically to pin that the
  bare verb consults nothing.
- **Emoji policy:** allowed in terminal output such as `resolve` / `doctor`;
  forbidden in `serve` log messages. The `--resolve` table is columnar, so it
  uses ASCII marks regardless — a wide glyph breaks column alignment.
- **Commit messages carry no `Co-Authored-By` line.**
- **`print_resolve_report` is out of scope in every task.** If a step seems to
  want to edit it, the step is wrong.

---

### Task 1: `Certainty` vocabulary in `beckon-core`

**Files:**
- Create: `crates/beckon-core/src/certainty.rs`
- Modify: `crates/beckon-core/src/lib.rs` (the `pub mod` block at the bottom of
  the file, lines 73-79, alphabetically sorted)
- Test: inline `#[cfg(test)] mod tests` at the bottom of
  `crates/beckon-core/src/certainty.rs` (beckon-core has no `[dev-dependencies]`
  and no `tests/` directory; every test in this crate is inline)

**Interfaces:**
- Consumes: nothing.
- Produces:
  - `beckon_core::certainty::Certainty` — `enum { Exact, Guess, NoMatch }`,
    `#[derive(Debug, Clone, Copy, PartialEq, Eq)]`
  - `Certainty::mark(self) -> char`
  - `Certainty::word(self) -> &'static str`
  - `beckon_core::certainty::NameReport` — struct with fields
    `id: String`, `certainty: Certainty`, `target: Option<String>`,
    `tier: Option<&'static str>`, `consequence: String`,
    `suggestions: Vec<String>`; `#[derive(Debug, Clone, PartialEq, Eq)]`
  - `beckon_core::certainty::Summary` — struct with fields
    `exact: usize`, `guess: usize`, `no_match: usize`
  - `Summary::line(&self) -> String`
  - `beckon_core::certainty::summarize(reports: &[NameReport]) -> Summary`

- [ ] **Step 1: Branch off main**

The repo's convention is not to commit feature work directly to the default
branch.

```bash
cd /Users/lenamkhanh/Documents/dev/beckon
git switch -c check-resolve
```

- [ ] **Step 2: Write the failing test**

Create `crates/beckon-core/src/certainty.rs` containing **only** this test
module. The types it names do not exist yet, which is the failure.

```rust
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
        }
    }

    // ---------- marks ----------

    /// The mark is what the eye scans down the left edge, so `Exact` must be
    /// blank rather than a tick: twenty ticks and two warnings reads as noise,
    /// two warnings against blank rows reads as two warnings.
    #[test]
    fn only_the_problems_carry_a_mark() {
        assert_eq!(Certainty::Exact.mark(), ' ');
        assert_eq!(Certainty::Guess.mark(), '!');
        assert_eq!(Certainty::NoMatch.mark(), 'x');
    }

    /// ASCII, exhaustively. The table is columnar and a wide glyph shifts
    /// every following column on the row it appears in.
    #[test]
    fn marks_and_words_are_ascii() {
        for c in [Certainty::Exact, Certainty::Guess, Certainty::NoMatch] {
            assert!(c.mark().is_ascii(), "{c:?}");
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

    /// When something is wrong the tail must carry the whole picture, because
    /// the table above it prints only the problem rows — without the exact
    /// count the reader cannot tell twenty bindings from two.
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
```

- [ ] **Step 3: Run the test to verify it fails**

Run: `cargo test -p beckon-core certainty`
Expected: FAIL — compile errors, `cannot find type 'Certainty' in this scope`
and the same for `NameReport`, `Summary`, `summarize`. (`certainty` is not yet
declared as a module, so the file is not even compiled; add the module
declaration in Step 4 and the errors appear.)

- [ ] **Step 4: Declare the module**

In `crates/beckon-core/src/lib.rs`, the `pub mod` block sits at the **bottom**
of the file and is alphabetically sorted. Insert `certainty` between `capture`
and `config_write`:

```rust
pub mod caps;
pub mod capture;
pub mod certainty;
pub mod config_write;
pub mod menu;
pub mod settings;
pub mod shortcuts;
pub mod theme;
```

- [ ] **Step 5: Write the implementation**

Prepend this to `crates/beckon-core/src/certainty.rs`, above the test module
written in Step 2.

```rust
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
    /// The leading glyph of a `check --resolve` row.
    ///
    /// `Exact` is blank on purpose. The mark is what the eye scans down the
    /// left edge; twenty ticks with two warnings among them reads as noise,
    /// while two marks against blank rows reads as two problems.
    ///
    /// ASCII, like every other mark in a columnar beckon output: a wide glyph
    /// shifts every column after it on the row it appears in.
    pub fn mark(self) -> char {
        match self {
            Certainty::Exact => ' ',
            Certainty::Guess => '!',
            Certainty::NoMatch => 'x',
        }
    }

    /// The word for this grade, for the summary line and for a row that has no
    /// tier to name.
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
}

/// Counts for the one-line tail of a `check --resolve` run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Summary {
    pub exact: usize,
    pub guess: usize,
    pub no_match: usize,
}

impl Summary {
    /// The closing line.
    ///
    /// The table above prints only the problem rows, so this line carries the
    /// whole picture — without the exact count a reader cannot tell twenty
    /// bindings from two. When nothing is wrong it says so in three words
    /// rather than making anyone count blank rows.
    pub fn line(&self) -> String {
        let total = self.exact + self.guess + self.no_match;
        if total == 0 {
            return "nothing to resolve".to_string();
        }
        if self.guess == 0 && self.no_match == 0 {
            return format!("all {} exact", self.exact);
        }
        let mut parts = vec![format!("{} exact", self.exact)];
        if self.guess > 0 {
            parts.push(format!("{} guess", self.guess));
        }
        if self.no_match > 0 {
            parts.push(format!("{} no match", self.no_match));
        }
        parts.join(", ")
    }
}

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
```

- [ ] **Step 6: Run the tests to verify they pass**

Run: `cargo test -p beckon-core certainty`
Expected: PASS — 7 tests.

- [ ] **Step 7: Run the local gate**

Run:
```bash
cargo fmt --all -- --check
cargo clippy --workspace --exclude beckon-linux --exclude beckon-windows --all-targets -- -D warnings
```
Expected: both clean. The `--exclude` flags copy the macOS CI job; a bare
workspace clippy cannot pass on a macOS host.

- [ ] **Step 8: Commit**

```bash
git add crates/beckon-core/src/certainty.rs crates/beckon-core/src/lib.rs
git commit -m "core: a cross-OS word for how sure a name resolution is"
```

---

### Task 2: macOS — map `MatchType`, split the resolver, add `resolve_reports`

**Files:**
- Modify: `crates/beckon-macos/src/apps.rs` (add `MatchType::certainty`; split
  `resolve_inner` at line 255 into two pure halves; add the report builders)
- Modify: `crates/beckon-macos/src/lib.rs` (re-export, two `#[cfg]` arms,
  mirroring `print_resolve_report` at lines 109-122)
- Test: inline `#[cfg(test)] mod tests` in `crates/beckon-macos/src/apps.rs`
  (starts at line 337; reuse its existing `rref` / `installed` helpers)

**Interfaces:**
- Consumes: `beckon_core::certainty::{Certainty, NameReport}` from Task 1.
- Produces:
  - `beckon_macos::resolve_reports(ids: &[&str]) -> beckon_core::Result<Vec<NameReport>>`
  - internal, `pub(crate)`: `apps::MatchType::certainty(self) -> Certainty`,
    `apps::resolve_running_in`, `apps::resolve_installed_in`,
    `apps::resolve_reports_in`

**Background the implementer needs:**
`mod apps` is **private** in `crates/beckon-macos/src/lib.rs`
(`#[cfg(target_os = "macos")] mod apps;`), so everything in it is crate-visible
only and reaches the outside world through an explicit re-export in `lib.rs`.
`RunningAppInfo` holds a live `Retained<NSRunningApplication>` and cannot be
built in a test; that is why the resolver takes `RunningRef<'a>` — a
`pub(crate)` struct of two `&str` — and why any new pure function must do the
same.

- [ ] **Step 1: Write the failing tests**

Append to the existing `mod tests` in `crates/beckon-macos/src/apps.rs`, after
the last test in the file. `rref` and `installed` are the module's existing
helpers; do not redefine them.

```rust
    // ---------- certainty ----------

    /// Exactly one tier is a guess, and it is the substring one. Written as a
    /// list rather than a loop so that adding a `MatchType` variant makes this
    /// test fail to compile in `certainty()` itself — the wildcard-free match
    /// there is the real guard, and this pins the answer it gives.
    #[test]
    fn only_the_substring_tier_is_a_guess() {
        use beckon_core::certainty::Certainty;
        assert_eq!(MatchType::RunningName.certainty(), Certainty::Exact);
        assert_eq!(MatchType::RunningBundleId.certainty(), Certainty::Exact);
        assert_eq!(MatchType::InstalledName.certainty(), Certainty::Exact);
        assert_eq!(MatchType::InstalledBundleId.certainty(), Certainty::Exact);
        assert_eq!(
            MatchType::InstalledNameSubstring.certainty(),
            Certainty::Guess
        );
    }

    // ---------- resolve_reports_in ----------

    /// The whole point of the plural form: `installed_apps()` walks
    /// /Applications, /System/Applications and ~/Applications and parses an
    /// Info.plist per bundle. Twenty-one bindings must pay for that once.
    #[test]
    fn many_ids_load_the_installed_catalog_at_most_once() {
        use std::cell::Cell;
        let calls = Cell::new(0usize);
        let running = vec![rref("com.anthropic.claude", "Claude")];
        let reports = resolve_reports_in(
            &["Claude", "Brave Browser", "definitely-not-installed-zzz"],
            &running,
            || {
                calls.set(calls.get() + 1);
                vec![installed("com.brave.Browser", "Brave Browser")]
            },
        );
        assert_eq!(reports.len(), 3);
        assert_eq!(calls.get(), 1, "the catalog must be loaded exactly once");
    }

    /// And not at all when nothing needs it — a config whose every app is
    /// already running should never touch the filesystem.
    #[test]
    fn a_config_that_all_resolves_from_running_apps_never_scans() {
        use std::cell::Cell;
        let calls = Cell::new(0usize);
        let running = vec![
            rref("com.anthropic.claude", "Claude"),
            rref("com.brave.Browser", "Brave Browser"),
        ];
        let reports = resolve_reports_in(&["Claude", "Brave Browser"], &running, || {
            calls.set(calls.get() + 1);
            Vec::new()
        });
        assert_eq!(calls.get(), 0);
        assert!(reports.iter().all(|r| r.certainty
            == beckon_core::certainty::Certainty::Exact));
    }

    #[test]
    fn a_substring_hit_is_reported_as_a_guess_and_names_the_alternatives() {
        use beckon_core::certainty::Certainty;
        let reports = resolve_reports_in(&["brave"], &[], || {
            vec![
                installed("com.brave.Browser", "Brave Browser"),
                installed("com.brave.Browser.beta", "Brave Browser Beta"),
            ]
        });
        let r = &reports[0];
        assert_eq!(r.certainty, Certainty::Guess);
        assert_eq!(r.target.as_deref(), Some("com.brave.Browser"));
        assert_eq!(r.tier, Some("installed app name substring"));
        assert!(!r.consequence.is_empty(), "a guess must say what it costs");
        assert!(
            r.suggestions.iter().any(|s| s == "Brave Browser Beta"),
            "{:?}",
            r.suggestions
        );
    }

    /// A total miss has no suggestions to give and must not pretend otherwise:
    /// the substring tier IS the last tier, so if nothing matched by substring
    /// there is nothing close by any measure this crate owns.
    #[test]
    fn a_total_miss_carries_no_target_no_tier_and_no_suggestions() {
        use beckon_core::certainty::Certainty;
        let reports = resolve_reports_in(&["zalo"], &[], || {
            vec![installed("com.apple.finder", "Finder")]
        });
        let r = &reports[0];
        assert_eq!(r.certainty, Certainty::NoMatch);
        assert_eq!(r.target, None);
        assert_eq!(r.tier, None);
        assert!(r.suggestions.is_empty());
        assert!(!r.consequence.is_empty(), "a miss must say what happens");
    }

    /// The report echoes the id as written, so a `check` row can print the
    /// config's own spelling rather than a normalized one.
    #[test]
    fn the_report_echoes_the_id_verbatim() {
        let reports = resolve_reports_in(&["  Brave Browser "], &[], Vec::new);
        assert_eq!(reports[0].id, "  Brave Browser ");
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p beckon-macos`
Expected: FAIL — `no method named 'certainty' found for enum 'MatchType'` and
`cannot find function 'resolve_reports_in' in this scope`.

- [ ] **Step 3: Add the certainty mapping**

In `crates/beckon-macos/src/apps.rs`, extend the existing
`impl MatchType` block (line 47) — the one that already holds `describe` — with:

```rust
    /// How sure this tier is, in the cross-OS vocabulary.
    ///
    /// Exhaustive with no wildcard arm on purpose: a new `MatchType` variant
    /// must fail to compile here rather than default quietly into `Exact`.
    pub fn certainty(self) -> beckon_core::certainty::Certainty {
        use beckon_core::certainty::Certainty;
        match self {
            MatchType::RunningName => Certainty::Exact,
            MatchType::RunningBundleId => Certainty::Exact,
            MatchType::InstalledName => Certainty::Exact,
            MatchType::InstalledBundleId => Certainty::Exact,
            MatchType::InstalledNameSubstring => Certainty::Guess,
        }
    }
```

- [ ] **Step 4: Split `resolve_inner` into two pure halves**

Replace the whole of `resolve_inner` (`crates/beckon-macos/src/apps.rs:255`)
with the three functions below. This is a behaviour-preserving extraction: the
tier order, the `normalize` asymmetry (name tiers normalize, bundle-id tiers
compare the raw input byte-exactly) and the substring sort are unchanged, and
the module's existing `resolve_inner` tests are the regression gate.

```rust
/// The two running-app tiers. Split out of `resolve_inner` so a caller with a
/// whole list of ids can run these without holding an installed-app scan.
pub(crate) fn resolve_running_in(
    id: &str,
    running: &[RunningRef<'_>],
    bundle_path_for: impl Fn(&str) -> Option<PathBuf>,
) -> Option<ResolvedMatch> {
    let needle = normalize(id);

    if let Some(app) = running.iter().find(|a| normalize(a.name) == needle) {
        return Some(ResolvedMatch {
            bundle_id: app.bundle_id.to_string(),
            display_name: app.name.to_string(),
            bundle_path: bundle_path_for(app.bundle_id),
            match_type: MatchType::RunningName,
        });
    }
    if let Some(app) = running.iter().find(|a| a.bundle_id == id) {
        return Some(ResolvedMatch {
            bundle_id: app.bundle_id.to_string(),
            display_name: app.name.to_string(),
            bundle_path: bundle_path_for(app.bundle_id),
            match_type: MatchType::RunningBundleId,
        });
    }
    None
}

/// The three installed-app tiers, against a caller-supplied catalog. Taking a
/// slice rather than a loader is what lets `resolve_reports_in` scan once and
/// resolve many.
pub(crate) fn resolve_installed_in(
    id: &str,
    installed: &[InstalledAppInfo],
) -> Option<ResolvedMatch> {
    let needle = normalize(id);

    if let Some(app) = installed.iter().find(|a| normalize(&a.name) == needle) {
        return Some(ResolvedMatch {
            bundle_id: app.bundle_id.clone(),
            display_name: app.name.clone(),
            bundle_path: Some(app.bundle_path.clone()),
            match_type: MatchType::InstalledName,
        });
    }
    if let Some(app) = installed.iter().find(|a| a.bundle_id == id) {
        return Some(ResolvedMatch {
            bundle_id: app.bundle_id.clone(),
            display_name: app.name.clone(),
            bundle_path: Some(app.bundle_path.clone()),
            match_type: MatchType::InstalledBundleId,
        });
    }

    let mut subs: Vec<&InstalledAppInfo> = installed
        .iter()
        .filter(|a| normalize(&a.name).contains(&needle))
        .collect();
    subs.sort_by(|a, b| a.bundle_id.cmp(&b.bundle_id));
    subs.first().map(|app| ResolvedMatch {
        bundle_id: app.bundle_id.clone(),
        display_name: app.name.clone(),
        bundle_path: Some(app.bundle_path.clone()),
        match_type: MatchType::InstalledNameSubstring,
    })
}

/// Pure resolution against caller-supplied snapshots. Closures isolate the
/// two NSWorkspace-touching operations (installed scan, bundle path lookup)
/// so tests can pass stubs.
pub(crate) fn resolve_inner(
    id: &str,
    running: &[RunningRef<'_>],
    installed_loader: impl FnOnce() -> Vec<InstalledAppInfo>,
    bundle_path_for: impl Fn(&str) -> Option<PathBuf>,
) -> Option<ResolvedMatch> {
    if let Some(m) = resolve_running_in(id, running, bundle_path_for) {
        return Some(m);
    }
    resolve_installed_in(id, &installed_loader())
}
```

- [ ] **Step 5: Add the report builders**

Append to `crates/beckon-macos/src/apps.rs`, after `name_substring_matches`
(which ends at line 326) and before the `#[cfg(test)]` module.

```rust
/// What a keypress does when a name only matched by substring, on macOS.
const GUESS_CONSEQUENCE: &str =
    "full installed-app scan on every press, and another app can take this name";

/// What a keypress does when nothing matched at all, on macOS.
const MISS_CONSEQUENCE: &str = "this key will error and launch nothing";

fn report_for(id: &str, m: &ResolvedMatch, installed: &[InstalledAppInfo]) -> NameReport {
    let certainty = m.match_type.certainty();
    // Suggestions exist only for a guess. The substring tier IS the last
    // tier, so a total miss has nothing close by any measure this crate owns
    // — there is no fuzzy matcher anywhere in beckon.
    let suggestions = if certainty == Certainty::Guess {
        let needle = normalize(id);
        let mut others: Vec<String> = installed
            .iter()
            .filter(|a| normalize(&a.name).contains(&needle) && a.bundle_id != m.bundle_id)
            .map(|a| a.name.clone())
            .collect();
        others.sort();
        others.truncate(3);
        others
    } else {
        Vec::new()
    };
    NameReport {
        id: id.to_string(),
        certainty,
        target: Some(m.bundle_id.clone()),
        tier: Some(m.match_type.describe()),
        consequence: if certainty == Certainty::Guess {
            GUESS_CONSEQUENCE.to_string()
        } else {
            String::new()
        },
        suggestions,
    }
}

/// One `NameReport` per id, against caller-supplied snapshots.
///
/// `installed_loader` runs **at most once** for the whole slice, and not at all
/// when every id resolves from the running-app tiers. That is the reason this
/// function exists rather than a loop over `resolve`: `installed_apps()` walks
/// three directories and parses an `Info.plist` per bundle, and a shortcuts
/// file has twenty-odd names in it.
pub(crate) fn resolve_reports_in(
    ids: &[&str],
    running: &[RunningRef<'_>],
    installed_loader: impl FnOnce() -> Vec<InstalledAppInfo>,
) -> Vec<NameReport> {
    let mut loader = Some(installed_loader);
    let mut installed: Option<Vec<InstalledAppInfo>> = None;
    let mut out = Vec::with_capacity(ids.len());

    for id in ids {
        // `|_| None` for the bundle path: the report names the bundle id and
        // never the path, so an NSWorkspace lookup per id would buy nothing.
        if let Some(m) = resolve_running_in(id, running, |_| None) {
            out.push(report_for(id, &m, &[]));
            continue;
        }
        if installed.is_none() {
            let load = loader.take().expect("loader is taken at most once");
            installed = Some(load());
        }
        let inst = installed.as_deref().expect("loaded on the line above");
        match resolve_installed_in(id, inst) {
            Some(m) => out.push(report_for(id, &m, inst)),
            None => out.push(NameReport {
                id: (*id).to_string(),
                certainty: Certainty::NoMatch,
                target: None,
                tier: None,
                consequence: MISS_CONSEQUENCE.to_string(),
                suggestions: Vec::new(),
            }),
        }
    }
    out
}

/// One `NameReport` per id, against this machine. See `resolve_reports_in`.
pub fn resolve_reports(ids: &[&str]) -> Vec<NameReport> {
    let running = running_apps();
    let refs: Vec<RunningRef<'_>> = running.iter().map(RunningRef::from).collect();
    resolve_reports_in(ids, &refs, installed_apps)
}
```

Add the imports this needs to the top of `crates/beckon-macos/src/apps.rs`,
beside the existing `use` lines:

```rust
use beckon_core::certainty::{Certainty, NameReport};
```

- [ ] **Step 6: Re-export from the crate root**

In `crates/beckon-macos/src/lib.rs`, after the existing `print_resolve_report`
pair (lines 109-122), add — same two-arm shape, because the crate must still
compile when it is not the target OS:

```rust
/// One resolution report per id, against a single scan of the installed-app
/// catalog. Feeds `beckon check --resolve`.
///
/// `beckon resolve <ID>` keeps its own, deeper report: this one is a line per
/// binding for a whole file, that one is everything known about one id.
/// They cannot disagree about which tier fired, because both go through
/// `apps`' resolver.
#[cfg(target_os = "macos")]
pub fn resolve_reports(ids: &[&str]) -> Result<Vec<beckon_core::certainty::NameReport>> {
    Ok(apps::resolve_reports(ids))
}

#[cfg(not(target_os = "macos"))]
pub fn resolve_reports(_ids: &[&str]) -> Result<Vec<beckon_core::certainty::NameReport>> {
    Err(BackendError::UnsupportedEnvironment(
        "beckon-macos only compiles on macOS".to_string(),
    ))
}
```

`BackendError` is already imported under `#[cfg(not(target_os = "macos"))]` at
`lib.rs:18-19`, which is exactly the arm that uses it here — no import change
is needed.

- [ ] **Step 7: Run the tests to verify they pass**

Run: `cargo test -p beckon-macos`
Expected: PASS — the six new tests, **and** every pre-existing `resolve_inner`
test still green. Those are the regression gate for the Step 4 split; if any of
them fails, the extraction changed behaviour and must be corrected rather than
the test.

- [ ] **Step 8: Run the local gate**

Run:
```bash
cargo fmt --all -- --check
cargo build --workspace --exclude beckon-linux --exclude beckon-windows --all-targets
cargo clippy --workspace --exclude beckon-linux --exclude beckon-windows --all-targets -- -D warnings
```
Expected: all clean.

- [ ] **Step 9: Commit**

```bash
git add crates/beckon-macos/src/apps.rs crates/beckon-macos/src/lib.rs
git commit -m "macos: resolve a whole list of names against one catalog scan"
```

---

### Task 3: Windows — map `MatchType`, add `resolve_reports`

**Files:**
- Modify: `crates/beckon-windows/src/apps.rs` (add `MatchType::certainty` and
  the report builders)
- Modify: `crates/beckon-windows/src/lib.rs` (re-export, two `#[cfg]` arms,
  mirroring `print_resolve_report` at lines 61-72)
- Test: inline `#[cfg(test)] mod tests` in `crates/beckon-windows/src/apps.rs`
  (starts at line 470; reuse its existing `app` / `appx` helpers)

**Interfaces:**
- Consumes: `beckon_core::certainty::{Certainty, NameReport}` from Task 1.
- Produces:
  - `beckon_windows::resolve_reports(ids: &[&str]) -> beckon_core::Result<Vec<NameReport>>`
  - internal: `apps::MatchType::certainty(self) -> Certainty`,
    `apps::resolve_reports_in(ids: &[&str], installed: &[InstalledAppInfo]) -> Vec<NameReport>`

**Background the implementer needs:**
Windows is simpler than macOS here: `apps::resolve(id, installed)` already takes
a pre-scanned slice, and there are no running-app tiers in the resolver at all —
so one `scan_installed_apps()` plus a loop is the whole story, with no lazy
loader to thread. Use the **full** `scan_installed_apps()`, not `resolve_lazy`:
`resolve_lazy` is a hot-path optimisation that can return `InstalledName`
without ever enumerating AppsFolder, and a report built on it would disagree
with what `beckon <id>` does on the miss path. Discovery commands already follow
this rule.

- [ ] **Step 1: Write the failing tests**

Append to the existing `mod tests` in `crates/beckon-windows/src/apps.rs`.
`app` and `appx` are the module's existing helpers; do not redefine them.

```rust
    // ---------- certainty ----------

    /// Exactly one tier is a guess. `InstalledExeStem` looks fuzzy and is not:
    /// it is equality against `exe_name`, so it belongs with the exact tiers.
    #[test]
    fn only_the_substring_tier_is_a_guess() {
        use beckon_core::certainty::Certainty;
        assert_eq!(MatchType::InstalledName.certainty(), Certainty::Exact);
        assert_eq!(MatchType::InstalledAumid.certainty(), Certainty::Exact);
        assert_eq!(MatchType::InstalledExeStem.certainty(), Certainty::Exact);
        assert_eq!(
            MatchType::InstalledNameSubstring.certainty(),
            Certainty::Guess
        );
    }

    // ---------- resolve_reports_in ----------

    #[test]
    fn every_id_gets_exactly_one_report_in_order() {
        let installed = vec![app("Claude", "claude.exe"), app("Brave", "brave.exe")];
        let reports = resolve_reports_in(&["Brave", "Claude", "nope-zzz"], &installed);
        let ids: Vec<&str> = reports.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(ids, vec!["Brave", "Claude", "nope-zzz"]);
    }

    #[test]
    fn an_exact_name_carries_its_tier_and_nothing_to_warn_about() {
        use beckon_core::certainty::Certainty;
        let installed = vec![app("Claude", "claude.exe")];
        let r = &resolve_reports_in(&["Claude"], &installed)[0];
        assert_eq!(r.certainty, Certainty::Exact);
        assert_eq!(r.tier, Some("Start Menu/app display name (exact)"));
        assert!(r.consequence.is_empty());
        assert!(r.suggestions.is_empty());
    }

    /// The measured reason this whole command exists: a substring name pays a
    /// full catalog scan on every keypress. The report has to say so.
    ///
    /// The exe names are deliberately NOT `brave.exe`: tier 3 is
    /// `a.exe_name == "brave.exe"`, which would match the id `brave` exactly
    /// and make this an `InstalledExeStem` hit — an exact tier — before the
    /// substring tier is ever reached.
    #[test]
    fn a_substring_hit_is_a_guess_and_names_the_alternatives() {
        use beckon_core::certainty::Certainty;
        let installed = vec![
            app("Brave Browser", "bravebrowser.exe"),
            app("Brave Browser Beta", "bravebeta.exe"),
        ];
        let r = &resolve_reports_in(&["brave"], &installed)[0];
        assert_eq!(r.certainty, Certainty::Guess);
        assert_eq!(r.tier, Some("Start Menu/app display name (substring)"));
        assert!(!r.consequence.is_empty());
        assert!(
            r.suggestions.iter().any(|s| s == "Brave Browser Beta"),
            "{:?}",
            r.suggestions
        );
    }

    /// On Windows a miss is not the end of the story — `windows_by_literal_id`
    /// still tries exe name and window title — so the sentence must not claim
    /// the key does nothing.
    #[test]
    fn a_miss_says_what_windows_actually_does_next() {
        use beckon_core::certainty::Certainty;
        let installed = vec![app("Claude", "claude.exe")];
        let r = &resolve_reports_in(&["zalo"], &installed)[0];
        assert_eq!(r.certainty, Certainty::NoMatch);
        assert_eq!(r.target, None);
        assert!(r.consequence.contains("title"), "{}", r.consequence);
    }

    /// A packaged app reports its AUMID as the target, because that is what
    /// launching it actually uses.
    #[test]
    fn a_packaged_app_reports_its_aumid_as_the_target() {
        let installed = vec![appx(
            "Windows Terminal",
            "Microsoft.WindowsTerminal_8wekyb3d8bbwe!App",
            "wt.exe",
        )];
        let r = &resolve_reports_in(&["Windows Terminal"], &installed)[0];
        assert_eq!(
            r.target.as_deref(),
            Some("Microsoft.WindowsTerminal_8wekyb3d8bbwe!App")
        );
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

On a Windows machine: `cargo test -p beckon-windows`
From the macOS dev host, the compile-only equivalent:
`cargo check -p beckon-windows --target aarch64-pc-windows-msvc --all-targets`
Expected: FAIL — `no method named 'certainty'`, `cannot find function
'resolve_reports_in'`.

- [ ] **Step 3: Add the certainty mapping**

Extend the existing `impl MatchType` block in
`crates/beckon-windows/src/apps.rs` (line 57, the one holding `describe`):

```rust
    /// How sure this tier is, in the cross-OS vocabulary.
    ///
    /// Exhaustive with no wildcard arm on purpose. Note `InstalledExeStem` is
    /// `Exact`: it compares `a.exe_name == needle_exe`, whole-string equality,
    /// not a substring.
    pub fn certainty(self) -> beckon_core::certainty::Certainty {
        use beckon_core::certainty::Certainty;
        match self {
            MatchType::InstalledName => Certainty::Exact,
            MatchType::InstalledAumid => Certainty::Exact,
            MatchType::InstalledExeStem => Certainty::Exact,
            MatchType::InstalledNameSubstring => Certainty::Guess,
        }
    }
```

- [ ] **Step 4: Add the report builders**

Append to `crates/beckon-windows/src/apps.rs`, after `name_substring_matches`
(ends around line 453) and before the `#[cfg(test)]` module.

```rust
/// What a keypress costs when a name only matched by substring, on Windows.
/// The numbers are CLAUDE.md's, measured on ARM64 Windows 11.
const GUESS_CONSEQUENCE: &str =
    "full catalog scan on every press (~400 ms), and another app can take this name";

/// What happens on a miss. Not "nothing": `windows_by_literal_id` still tries
/// the exe name and then the window title, so a miss can still focus something
/// — it just can never launch.
const MISS_CONSEQUENCE: &str =
    "no installed app; focus may still match by exe or window title, launch will fail";

fn report_for(id: &str, m: &ResolvedMatch, installed: &[InstalledAppInfo]) -> NameReport {
    let certainty = m.match_type.certainty();
    let suggestions = if certainty == Certainty::Guess {
        let needle = normalize(id);
        let mut others: Vec<String> = installed
            .iter()
            .filter(|a| normalize(&a.name).contains(&needle) && normalize(&a.name) != normalize(&m.name))
            .map(|a| a.name.clone())
            .collect();
        others.sort();
        others.truncate(3);
        others
    } else {
        Vec::new()
    };
    // An AUMID is what activation actually uses for a packaged app; for a
    // classic shortcut the exe path is the honest answer.
    let target = match &m.aumid {
        Some(aumid) => aumid.clone(),
        None => m.exe_path.clone(),
    };
    NameReport {
        id: id.to_string(),
        certainty,
        target: Some(target),
        tier: Some(m.match_type.describe()),
        consequence: if certainty == Certainty::Guess {
            GUESS_CONSEQUENCE.to_string()
        } else {
            String::new()
        },
        suggestions,
    }
}

/// One `NameReport` per id, against a caller-supplied catalog.
pub(crate) fn resolve_reports_in(ids: &[&str], installed: &[InstalledAppInfo]) -> Vec<NameReport> {
    ids.iter()
        .map(|id| match resolve(id, installed) {
            Some(m) => report_for(id, &m, installed),
            None => NameReport {
                id: (*id).to_string(),
                certainty: Certainty::NoMatch,
                target: None,
                tier: None,
                consequence: MISS_CONSEQUENCE.to_string(),
                suggestions: Vec::new(),
            },
        })
        .collect()
}

/// One `NameReport` per id, against this machine, with a single catalog scan.
///
/// Deliberately the full `scan_installed_apps` rather than `resolve_lazy`:
/// the lazy path can answer `InstalledName` without ever enumerating
/// AppsFolder, so a report built on it would disagree with what `beckon <id>`
/// does on the miss path. Discovery commands buy completeness with latency;
/// this is one of them.
pub fn resolve_reports(ids: &[&str]) -> Vec<NameReport> {
    let installed = scan_installed_apps();
    resolve_reports_in(ids, &installed)
}
```

Add the import at the top of `crates/beckon-windows/src/apps.rs`:

```rust
use beckon_core::certainty::{Certainty, NameReport};
```

- [ ] **Step 5: Re-export from the crate root**

In `crates/beckon-windows/src/lib.rs`, after the existing
`print_resolve_report` pair (lines 61-72):

```rust
/// One resolution report per id, against a single scan of the installed-app
/// catalog. Feeds `beckon check --resolve`; `beckon resolve <ID>` keeps its own
/// deeper report, and the two cannot disagree because both go through `apps`.
#[cfg(target_os = "windows")]
pub fn resolve_reports(ids: &[&str]) -> Result<Vec<beckon_core::certainty::NameReport>> {
    Ok(apps::resolve_reports(ids))
}

#[cfg(not(target_os = "windows"))]
pub fn resolve_reports(_ids: &[&str]) -> Result<Vec<beckon_core::certainty::NameReport>> {
    Err(BackendError::UnsupportedEnvironment(
        "beckon-windows only runs on Windows".to_string(),
    ))
}
```

`BackendError` is already imported under `#[cfg(not(target_os = "windows"))]` at
`lib.rs:14-15`, which is the arm that uses it — no import change needed.
Note `mod apps` **is** `pub` in this crate (unlike beckon-macos), so
`apps::resolve_reports` is reachable; the wrapper exists for the non-Windows arm
and for signature parity with the other two crates.

- [ ] **Step 6: Verify from the macOS dev host**

Run:
```bash
cargo check -p beckon-windows --target aarch64-pc-windows-msvc --all-targets
```
Expected: clean. This compiles the code and the test module without MSVC
installed. It does **not** run the tests — those run on the Windows CI job.
If a cold cache produces a `SIGKILL` with empty output, re-run; it converges
after a few attempts and is environmental.

- [ ] **Step 7: Run the local gate**

Run:
```bash
cargo fmt --all -- --check
cargo clippy --workspace --exclude beckon-linux --exclude beckon-windows --all-targets -- -D warnings
```
Expected: clean. (The Windows crate's own clippy runs on the Windows CI job.)

- [ ] **Step 8: Commit**

```bash
git add crates/beckon-windows/src/apps.rs crates/beckon-windows/src/lib.rs
git commit -m "windows: resolve a whole list of names against one catalog scan"
```

---

### Task 4: Linux — map `MatchType`, add `resolve_reports`

**Files:**
- Modify: `crates/beckon-linux/src/desktop.rs` (add `MatchType::certainty` and
  the report builders)
- Modify: `crates/beckon-linux/src/lib.rs` (re-export, two `#[cfg]` arms)
- Test: inline `#[cfg(test)] mod tests` in `crates/beckon-linux/src/desktop.rs`
  (starts at line 372; reuse its existing `entry` / `entry_with_wm` helpers)

**Interfaces:**
- Consumes: `beckon_core::certainty::{Certainty, NameReport}` from Task 1.
- Produces:
  - `beckon_linux::resolve_reports(ids: &[&str]) -> beckon_core::Result<Vec<NameReport>>`
  - internal: `desktop::MatchType::certainty(self) -> Certainty`,
    `desktop::resolve_reports_in(ids: &[&str], entries: &[DesktopEntry]) -> Vec<NameReport>`

**Background the implementer needs:**

1. **These tests will not run on a macOS dev host.** `mod desktop` is
   `#[cfg(target_os = "linux")]` in `crates/beckon-linux/src/lib.rs`, and
   `cargo test -p beckon-linux` on macOS prints `ok` while running **zero**
   tests. The Linux CI job is the only evidence. Do not read a local green as a
   pass.
2. **Do not call `name_substring_matches` for suggestions.** Unlike its macOS
   and Windows namesakes it takes no catalog argument and calls `scan()` itself
   (`desktop.rs:217`), so using it inside the loop would walk every XDG
   applications directory once per id. Inline the three-line filter over the
   `entries` slice instead, exactly as the code below does.
3. **`Certainty::NoMatch` on Linux does not mean "will not work".** When nothing
   resolves, `target_classes` falls back to `Target::new([raw_id])`
   (`desktop.rs:213`) and `Target::matches` is case-insensitive **equality** —
   the same strength as the `Filename` tier. That is what lets beckon focus an
   ad-hoc app that ships no `.desktop` file. The consequence sentence must say
   this; a shared "will error and launch nothing" line would be a lie here.

- [ ] **Step 1: Write the failing tests**

Append to the existing `mod tests` in `crates/beckon-linux/src/desktop.rs`.
`entry` and `entry_with_wm` are the module's existing helpers.

```rust
    // ---------- certainty ----------

    /// Exactly one tier is a guess. `Filename` and `StartupWmClass` are
    /// byte-exact comparisons, so they sit with the exact tiers.
    #[test]
    fn only_the_substring_tier_is_a_guess() {
        use beckon_core::certainty::Certainty;
        assert_eq!(MatchType::NameExact.certainty(), Certainty::Exact);
        assert_eq!(MatchType::Filename.certainty(), Certainty::Exact);
        assert_eq!(MatchType::StartupWmClass.certainty(), Certainty::Exact);
        assert_eq!(MatchType::NameSubstring.certainty(), Certainty::Guess);
    }

    // ---------- resolve_reports_in ----------

    #[test]
    fn every_id_gets_exactly_one_report_in_order() {
        let entries = vec![entry("kitty", "kitty"), entry("brave", "Brave")];
        let reports = resolve_reports_in(&["Brave", "kitty", "nope-zzz"], &entries);
        let ids: Vec<&str> = reports.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(ids, vec!["Brave", "kitty", "nope-zzz"]);
    }

    #[test]
    fn an_exact_name_reports_the_desktop_id_as_its_target() {
        use beckon_core::certainty::Certainty;
        let entries = vec![entry("org.telegram.desktop", "Telegram")];
        let r = &resolve_reports_in(&["Telegram"], &entries)[0];
        assert_eq!(r.certainty, Certainty::Exact);
        assert_eq!(r.target.as_deref(), Some("org.telegram.desktop"));
        assert_eq!(r.tier, Some("Name= exact (case-insensitive)"));
        assert!(r.consequence.is_empty());
    }

    /// The `.desktop` ids matter: tier 4 sorts candidates by `id`, so
    /// `brave-beta` would win over `brave-browser` and the assertions below
    /// would be inverted. Name them so the intended winner sorts first.
    #[test]
    fn a_substring_hit_is_a_guess_and_names_the_alternatives() {
        use beckon_core::certainty::Certainty;
        let entries = vec![
            entry("brave-browser", "Brave Web Browser"),
            entry("brave-browser-beta", "Brave Web Browser Beta"),
        ];
        let r = &resolve_reports_in(&["brave"], &entries)[0];
        assert_eq!(r.certainty, Certainty::Guess);
        assert_eq!(r.tier, Some("Name= substring (alphabetical first wins)"));
        assert!(!r.consequence.is_empty());
        assert!(
            r.suggestions.iter().any(|s| s == "Brave Web Browser Beta"),
            "{:?}",
            r.suggestions
        );
    }

    /// A miss on Linux is not fatal: the raw id becomes the window class, and
    /// `Target::matches` is equality, so an ad-hoc app with no `.desktop` file
    /// is still focusable. Saying "this key does nothing" would be wrong.
    #[test]
    fn a_miss_says_focus_can_still_work_and_launch_cannot() {
        use beckon_core::certainty::Certainty;
        let entries = vec![entry("kitty", "kitty")];
        let r = &resolve_reports_in(&["some-adhoc-app"], &entries)[0];
        assert_eq!(r.certainty, Certainty::NoMatch);
        assert_eq!(r.target, None);
        assert!(r.consequence.contains("focus"), "{}", r.consequence);
        assert!(r.consequence.contains("launch"), "{}", r.consequence);
    }

    /// The tier ladder is unchanged by this task — a StartupWMClass hit is
    /// still exact, and still reports the `.desktop` id as the target.
    #[test]
    fn a_startup_wm_class_hit_is_exact() {
        use beckon_core::certainty::Certainty;
        let entries = vec![entry_with_wm("debian-xterm", "XTerm session", "XTerm")];
        let r = &resolve_reports_in(&["XTerm"], &entries)[0];
        assert_eq!(r.certainty, Certainty::Exact);
        assert_eq!(r.tier, Some("StartupWMClass="));
        assert_eq!(r.target.as_deref(), Some("debian-xterm"));
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo check -p beckon-linux --target x86_64-unknown-linux-gnu` if that
target is installed, otherwise push the branch and read the Linux CI job.
Expected: FAIL — `no method named 'certainty'`, `cannot find function
'resolve_reports_in'`.

**Do not** run `cargo test -p beckon-linux` on macOS and read `ok` as a result;
it runs zero tests.

- [ ] **Step 3: Add the certainty mapping**

Extend the existing `impl MatchType` block in
`crates/beckon-linux/src/desktop.rs` (line 107, the one holding `describe`):

```rust
    /// How sure this tier is, in the cross-OS vocabulary.
    ///
    /// Exhaustive with no wildcard arm on purpose. `Filename` and
    /// `StartupWmClass` are byte-exact comparisons against the raw id, so they
    /// are `Exact` despite being weaker tiers than `NameExact`.
    pub fn certainty(self) -> beckon_core::certainty::Certainty {
        use beckon_core::certainty::Certainty;
        match self {
            MatchType::NameExact => Certainty::Exact,
            MatchType::Filename => Certainty::Exact,
            MatchType::StartupWmClass => Certainty::Exact,
            MatchType::NameSubstring => Certainty::Guess,
        }
    }
```

- [ ] **Step 4: Add the report builders**

Append to `crates/beckon-linux/src/desktop.rs`, after `name_substring_matches`
(ends at line 232) and before the `#[cfg(test)]` module.

```rust
/// What a keypress costs when a name only matched by substring, on Linux.
const GUESS_CONSEQUENCE: &str =
    "alphabetically first .desktop wins, so a new install can quietly take this name";

/// What a miss means on Linux. Not "nothing happens": `target_classes` falls
/// back to the raw id as a window class, and that comparison is equality — so
/// an ad-hoc app with no `.desktop` file is still focusable.
const MISS_CONSEQUENCE: &str =
    "no .desktop entry; focus still works if a window's class equals this id, launch will fail";

fn report_for(id: &str, m: &ResolvedMatch, entries: &[DesktopEntry]) -> NameReport {
    let certainty = m.match_type.certainty();
    // Deliberately not `name_substring_matches`: that one calls `scan()`
    // itself, which would walk every XDG applications directory again, once
    // per id.
    let suggestions = if certainty == Certainty::Guess {
        let needle = normalize(id);
        let mut others: Vec<String> = entries
            .iter()
            .filter(|e| normalize(&e.name).contains(&needle) && e.id != m.entry.id)
            .map(|e| e.name.clone())
            .collect();
        others.sort();
        others.truncate(3);
        others
    } else {
        Vec::new()
    };
    NameReport {
        id: id.to_string(),
        certainty,
        target: Some(m.entry.id.clone()),
        tier: Some(m.match_type.describe()),
        consequence: if certainty == Certainty::Guess {
            GUESS_CONSEQUENCE.to_string()
        } else {
            String::new()
        },
        suggestions,
    }
}

/// One `NameReport` per id, against a caller-supplied entry list.
pub fn resolve_reports_in(ids: &[&str], entries: &[DesktopEntry]) -> Vec<NameReport> {
    ids.iter()
        .map(|id| match resolve_detailed_in(entries, id) {
            Some(m) => report_for(id, &m, entries),
            None => NameReport {
                id: (*id).to_string(),
                certainty: Certainty::NoMatch,
                target: None,
                tier: None,
                consequence: MISS_CONSEQUENCE.to_string(),
                suggestions: Vec::new(),
            },
        })
        .collect()
}

/// One `NameReport` per id, against this machine, with a single `scan()`.
pub fn resolve_reports(ids: &[&str]) -> Vec<NameReport> {
    resolve_reports_in(ids, &scan())
}
```

Add the import at the top of `crates/beckon-linux/src/desktop.rs`:

```rust
use beckon_core::certainty::{Certainty, NameReport};
```

- [ ] **Step 5: Re-export from the crate root**

In `crates/beckon-linux/src/lib.rs`, after the `pub mod kde;` declaration
(line 32) and before the `WaylandDesktop` enum:

```rust
/// One resolution report per id, against a single `desktop::scan()`.
/// Feeds `beckon check --resolve`.
#[cfg(target_os = "linux")]
pub fn resolve_reports(ids: &[&str]) -> Result<Vec<beckon_core::certainty::NameReport>> {
    Ok(desktop::resolve_reports(ids))
}

#[cfg(not(target_os = "linux"))]
pub fn resolve_reports(_ids: &[&str]) -> Result<Vec<beckon_core::certainty::NameReport>> {
    Err(BackendError::UnsupportedEnvironment(
        "beckon-linux only runs on Linux".to_string(),
    ))
}
```

`beckon-linux/src/lib.rs` imports `Backend`, `BackendError` and `Result`
unconditionally at line 8, so both arms compile with no import change.

- [ ] **Step 6: Verify on the Linux CI job**

```bash
git add crates/beckon-linux/src/desktop.rs crates/beckon-linux/src/lib.rs
git commit -m "linux: resolve a whole list of names against one desktop scan"
git push -u origin check-resolve
gh run watch
```
Expected: the `build & test (ubuntu-latest)` job green, with the six new tests
in its output. **This is the only evidence that this task works** — see the
background note above.

---

### Task 5: The CLI — `check --resolve`

**Files:**
- Modify: `crates/beckon-cli/src/lib.rs` (the `Check` variant at line 106; the
  dispatch arm at line 286; `cmd_check` at line 360; two new private functions)
- Test: `crates/beckon-cli/tests/check.rs` (extend its existing `run_check`
  helper)

**Interfaces:**
- Consumes: `beckon_{macos,windows,linux}::resolve_reports(ids: &[&str]) ->
  beckon_core::Result<Vec<NameReport>>` from Tasks 2-4;
  `beckon_core::certainty::{summarize, Certainty}` from Task 1.
- Produces: the `beckon check --resolve <CONFIG>` command.

**Why this task is last:** the three backend functions must all exist before the
CLI can name them, because `beckon-cli` is compiled on every runner and its
per-OS dependencies are declared per target. Landing the CLI earlier would mean
either a half-implemented flag in the tree or a `#[cfg]` gate on the integration
test that two later tasks would have to keep editing.

- [ ] **Step 1: Write the failing tests**

Append to `crates/beckon-cli/tests/check.rs`. Note the existing `run_check`
helper takes only the file content; add a second helper beside it rather than
changing the first, so the five existing tests are untouched.

```rust
fn run_check_resolve(content: &str) -> Output {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("apps.toml");
    std::fs::write(&path, content).expect("write config");
    beckon()
        .arg("check")
        .arg(&path)
        .arg("--resolve")
        .output()
        .expect("run beckon")
}

/// `--resolve` is a report, not a gate. A name that resolves to nothing is a
/// finding the user may already know about — `Zalo` is genuinely not installed
/// on the author's Mac — and a check that goes red on a file its author
/// considers correct is a check people stop running. Pinned to exit 0, not to
/// "not 2": exit 1 would mean beckon's own error handler ran.
#[test]
fn resolve_reports_a_miss_without_failing() {
    let out = run_check_resolve("\"ctrl+super+alt+t\" = \"beckon-selftest-no-such-app\"\n");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(0), "stderr: {stderr}");
    assert!(stdout.contains("ok: 1 shortcuts"), "stdout: {stdout}");
    assert!(stdout.contains("no match"), "stdout: {stdout}");
}

/// The bare verb's output is the first line of the `--resolve` output too, so
/// scripts reading `ok: N shortcuts` keep working with the flag on.
#[test]
fn resolve_keeps_the_bare_verbs_first_line() {
    let out = run_check_resolve("\"ctrl+super+alt+t\" = \"beckon-selftest-no-such-app\"\n");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.starts_with("ok: 1 shortcuts"),
        "stdout: {stdout}"
    );
}

/// An empty file parses, so this path is reachable, and "all 0 exact" would be
/// a true sentence that reads like a bug.
#[test]
fn resolve_on_an_empty_file_says_there_was_nothing_to_do() {
    let out = run_check_resolve("");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(out.status.code(), Some(0));
    assert!(stdout.contains("nothing to resolve"), "stdout: {stdout}");
}

/// A parse failure must still fail, and must fail BEFORE anything touches the
/// machine — otherwise `--resolve` would turn a fast syntax error into a
/// several-hundred-millisecond catalog scan first.
#[test]
fn resolve_does_not_rescue_a_broken_file() {
    let out = run_check_resolve("\"ctrl+banana\" = \"kitty\"\n");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(1), "stderr: {stderr}");
    assert!(stderr.contains("unknown key `banana`"), "stderr: {stderr}");
}
```

And in `crates/beckon-cli/tests/cli_surface.rs`, append this parsing test —
`cli_surface.rs` is where flag *shape* is pinned, and it must hold on every
runner including a headless one:

```rust
/// Both orders parse. The documented form is verb-operand-flag, matching
/// `beckon serve C --log P`, but clap accepts the flag before the operand too
/// and a user who types it that way should not get a usage error.
#[test]
fn check_accepts_resolve_in_either_position() {
    with_valid_config(|cfg| {
        for argv in [
            vec!["check", cfg.to_str().unwrap(), "--resolve"],
            vec!["check", "--resolve", cfg.to_str().unwrap()],
        ] {
            let out = beckon().args(&argv).output().expect("run beckon");
            assert_eq!(
                out.status.code(),
                Some(0),
                "`beckon {}` must parse and run\nstderr: {}",
                argv.join(" "),
                stderr_of(&out),
            );
        }
    });
}

/// `--resolve` belongs to `check` and nowhere else — it is declared inside the
/// variant, so this is structural rather than a `conflicts_with` rule that
/// could be forgotten. Exit 2 is clap's usage error.
#[test]
fn resolve_is_not_a_flag_on_anything_else() {
    let out = beckon()
        .args(["list", "--resolve"])
        .output()
        .expect("run beckon");
    assert_eq!(out.status.code(), Some(2), "stderr: {}", stderr_of(&out));
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p beckon-cli --test check --test cli_surface`
Expected: FAIL — `unexpected argument '--resolve' found`, and the four
`check.rs` tests failing on exit code 2.

If a run dies with `signal: 9` and empty output, re-run — that is the known
local flake, not a result.

- [ ] **Step 3: Add the flag to the `Check` variant**

In `crates/beckon-cli/src/lib.rs`, replace the `Check` variant (line 106):

```rust
    /// Validate a shortcuts TOML file (see `beckon serve`) and exit; 0 = valid.
    Check {
        #[arg(value_name = "CONFIG")]
        config: std::path::PathBuf,

        /// Also resolve every app name against this machine's installed apps,
        /// and report which tier each one matches at.
        ///
        /// Without it, `check` validates shape and never meaning: it does not
        /// consult the machine at all, which is what makes the bare verb
        /// usable in CI. With it, a name that only matches by substring shows
        /// up — that costs a full catalog scan on every keypress and can
        /// silently switch to a different app the day something else takes the
        /// name.
        ///
        /// Local only, by nature: a Linux CI runner cannot resolve a macOS or
        /// Windows app name, so this flag does not belong in CI. It never
        /// changes the exit code either — a substring match is a finding, not
        /// a failure.
        #[arg(long)]
        resolve: bool,
    },
```

- [ ] **Step 4: Update the dispatch arm**

In `fn run` (line 286), the arm must now destructure both fields:

```rust
        Some(Command::Check { config, resolve }) => cmd_check(config, *resolve),
```

- [ ] **Step 5: Implement the command**

Replace `cmd_check` (line 360) and add the two functions after it:

```rust
fn cmd_check(path: &std::path::Path, resolve: bool) -> Result<()> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("cannot read `{}`", path.display()))?;
    let shortcuts = beckon_core::shortcuts::parse_shortcuts(&text)
        .map_err(|e| anyhow!("{}: {}", path.display(), e))?;
    println!("ok: {} shortcuts", shortcuts.len());
    if resolve {
        print_resolve_table(&shortcuts)?;
    }
    Ok(())
}

/// The `--resolve` half of `check`: one line per binding that is not already
/// exact, then a tail that carries the whole count.
///
/// Only the problem rows are printed. Twenty ticks with two warnings among
/// them reads as noise; two marks against nothing reads as two problems, and
/// the summary line is what tells the reader the other eighteen were fine.
///
/// Combos are printed through `Combo::canonical`, never `combo_display` — the
/// display form spells `super` as `Win`, and a row a user might paste back
/// into their config must carry the spelling the parser accepts.
fn print_resolve_table(shortcuts: &[beckon_core::shortcuts::Shortcut]) -> Result<()> {
    use beckon_core::certainty::{summarize, Certainty};

    let ids: Vec<&str> = shortcuts.iter().map(|s| s.app.as_str()).collect();
    let reports = resolve_reports(&ids)?;

    let combos: Vec<String> = shortcuts.iter().map(|s| s.combo.canonical()).collect();
    let width = combos.iter().map(|c| c.chars().count()).max().unwrap_or(0);

    println!();
    // `reports` is one-per-id in the order the ids were passed, so zipping it
    // back against `combos` is index-safe. The app name comes from the report
    // rather than the shortcut so the two can never fall out of step.
    for (combo, r) in combos.iter().zip(&reports) {
        if r.certainty == Certainty::Exact {
            continue;
        }
        println!(
            "  {}  {:<width$}  {:<28}  {}",
            r.certainty.mark(),
            combo,
            format!("\"{}\"", r.id),
            r.tier.unwrap_or_else(|| r.certainty.word()),
        );
        if !r.consequence.is_empty() {
            println!("     {:<width$}  {}", "", r.consequence);
        }
        for name in &r.suggestions {
            println!("     {:<width$}  also matches: {}", "", name);
        }
    }
    println!("{}", summarize(&reports).line());
    Ok(())
}

/// Per-OS dispatch for the report, mirroring `cmd_resolve`'s shape.
///
/// `beckon-cli` declares each backend as a per-target dependency, so only one
/// of these arms exists in any given build.
fn resolve_reports(ids: &[&str]) -> Result<Vec<beckon_core::certainty::NameReport>> {
    #[cfg(target_os = "linux")]
    {
        beckon_linux::resolve_reports(ids).map_err(|e| anyhow!("{e}"))
    }
    #[cfg(target_os = "macos")]
    {
        beckon_macos::resolve_reports(ids).map_err(|e| anyhow!("{e}"))
    }
    #[cfg(target_os = "windows")]
    {
        beckon_windows::resolve_reports(ids).map_err(|e| anyhow!("{e}"))
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        let _ = ids;
        Err(anyhow!(
            "`check --resolve` is not implemented on this platform"
        ))
    }
}
```

- [ ] **Step 6: Run the tests to verify they pass**

Run: `cargo test -p beckon-cli --test check --test cli_surface`
Expected: PASS — the four new `check.rs` tests, the two new `cli_surface.rs`
tests, and all five pre-existing `check.rs` tests still green (they are the
regression gate for "the bare verb did not change").

- [ ] **Step 7: See it work on real data**

Run:
```bash
cargo run -p beckon-cli --bin beckon -- check --resolve ~/.nix/configs/shortcuts/apps.macos.toml
```
Expected: `ok: 20 shortcuts`, then a row for each non-exact binding, then a
summary line. On the author's Mac this should surface at least `Brave` matching
by substring and `Zalo` not matching at all — the two findings that motivated
the whole design. If every row is exact, try
`~/.nix/configs/shortcuts/apps.windows.toml`, whose names are chosen for a
different OS and should mostly miss.

- [ ] **Step 8: Run the full local gate**

Run:
```bash
cargo fmt --all -- --check
cargo build --workspace --exclude beckon-linux --exclude beckon-windows --all-targets
cargo test --workspace --exclude beckon-linux --exclude beckon-windows
cargo clippy --workspace --exclude beckon-linux --exclude beckon-windows --all-targets -- -D warnings
cargo check -p beckon-windows --target aarch64-pc-windows-msvc --all-targets
```
Expected: all clean.

- [ ] **Step 9: Commit**

```bash
git add crates/beckon-cli/src/lib.rs crates/beckon-cli/tests/check.rs crates/beckon-cli/tests/cli_surface.rs
git commit -m "cli: check --resolve reports the tier every app name matches at"
```

---

### Task 6: Documentation

**Files:**
- Modify: `README.md` (the CLI surface section)
- Modify: `CLAUDE.md` (the "CLI surface" section, which lists every command
  verbatim, and the "Open questions" item 4 about `search` scope)

**Interfaces:**
- Consumes: the finished command from Task 5.
- Produces: nothing code depends on.

- [ ] **Step 1: Update the CLI surface listing in `CLAUDE.md`**

Find the fenced block under `### CLI surface (bare positional + subcommands,
since 0.6.0)` and change the `check` line, keeping the column alignment of the
surrounding lines:

```
beckon check <CONFIG>                # validate a shortcuts TOML file (CI-friendly)
beckon check <CONFIG> --resolve      # ...and resolve every name against this machine
```

- [ ] **Step 2: Add the rule the flag establishes**

Immediately after the `#### Reserved names are a closed list` subsection in
`CLAUDE.md`, add:

```markdown
#### `check` validates shape; `check --resolve` validates meaning

`beckon check` never consults the machine — that is what makes it usable in
CI, where none of the apps are installed, and it is pinned by
`check_without_resolve_says_nothing_about_whether_the_app_exists`.

`--resolve` adds the other half: every app name is resolved against this
machine's catalog and reported with the tier it matched at, drawn from
`beckon_core::certainty::Certainty` — `Exact`, `Guess` (the one substring
tier every backend has) or `NoMatch`. It exists because three comments in the
author's own shortcut files are hand-written results of this exact experiment,
and because a substring match is both slow (a full catalog scan per keypress)
and silently wrong (`Terminal` matches Apple's Terminal.app at tier 1).

**It never changes the exit code, and it does not belong in CI.** A
`ubuntu-latest` runner cannot resolve a macOS or Windows app name — the only
catalog it can consult is its own — so a red here would mean nothing. A gate,
if one is ever wanted, is a second flag.

`Certainty` lives in `beckon-core` rather than in a backend because
beckon-core is excluded from no CI job, and because it is the vocabulary a
per-binding `match` floor would consume.
```

- [ ] **Step 3: Update `README.md`**

In the command list, under the existing `beckon check` entry, add the flag with
one line of purpose. Match the surrounding formatting exactly — read the two
neighbouring entries before writing.

- [ ] **Step 4: Verify the docs did not break the site check**

Run: `./tools/check-site.sh`
Expected: PASS. It asserts the landing page's install commands byte-match
`README.md`, so a careless edit to the wrong part of that file fails here.

- [ ] **Step 5: Commit**

```bash
git add README.md CLAUDE.md
git commit -m "docs: check validates shape, check --resolve validates meaning"
```

---

### Task 7: `~/.nix` — bump the pin (separate repository)

This task is in `/Users/lenamkhanh/.nix`, not in the beckon repo. It is
independent of Tasks 1-6 and can be done before, after or alongside them.

**Files:**
- Modify: `/Users/lenamkhanh/.nix/flake.lock` (via `nix flake update`)

**Why:** `flake.lock` pins beckon at `ad1d0ce` — v0.6.0, 330 commits behind
HEAD — and that build contains **zero** occurrences of `KEYBOARD_KEY`: it does
not know the `keyboard` block exists and rejects any file carrying one. The CI
job at `.github/workflows/eval.yml:118-124` validates every `apps.*.toml` with
**that** binary. It is green only because no file currently has a `keyboard`
line, and the Windows settings window can write one at any time — beckon 0.8.0
already did once.

- [ ] **Step 1: Confirm the problem still exists**

```bash
cd ~/.nix
nix eval --raw --impure --expr \
  '(builtins.fromJSON (builtins.readFile ./flake.lock)).nodes.beckon.locked.rev'
```
Expected: a rev; check it against beckon's history with
`git -C ~/Documents/dev/beckon describe --tags <rev>`. If it already names a
recent tag, this task is done — skip to Step 4.

- [ ] **Step 2: Bump**

```bash
cd ~/.nix
nix flake update beckon
```

- [ ] **Step 3: Verify the new pin can read the current files**

```bash
cd ~/.nix
rev="$(nix eval --raw --impure --expr \
  '(builtins.fromJSON (builtins.readFile ./flake.lock)).nodes.beckon.locked.rev')"
for f in configs/shortcuts/apps.*.toml; do
  nix run "github:xom11/beckon/$rev" -- check "$f"
done
```
Expected: three `ok: N shortcuts` lines. This is the same loop CI runs, so a
green here predicts a green there.

- [ ] **Step 4: Prove the hazard is actually closed**

Append a `keyboard` line to a scratch copy and check it with the new pin — the
old pin rejects this outright, so it is the discriminating test:

```bash
cd ~/.nix
tmp="$(mktemp -d)/apps.toml"
cat configs/shortcuts/apps.macos.toml > "$tmp"
printf '\nkeyboard.caps = false\n' >> "$tmp"
rev="$(nix eval --raw --impure --expr \
  '(builtins.fromJSON (builtins.readFile ./flake.lock)).nodes.beckon.locked.rev')"
nix run "github:xom11/beckon/$rev" -- check "$tmp"
```
Expected: `ok: 20 shortcuts`. Under the old v0.6.0 pin this exits non-zero.

- [ ] **Step 5: Commit**

```bash
cd ~/.nix
git add flake.lock
git commit -m "flake: bump beckon off v0.6.0, which cannot read a keyboard block"
```

---

## Self-Review

**Spec coverage.** Every section of
`docs/superpowers/specs/2026-08-14-check-resolve-design.md` maps to a task:
§3 `Certainty` → Task 1 plus the mapping in Tasks 2-4; §4 reports-as-values →
Tasks 2-4 (`resolve_reports`), and the spec's explicit decision *not* to touch
`print_resolve_report` is carried into this plan's Global Constraints; §5 CLI
surface → Task 5 Steps 3-4; §6 output and exit code → Task 5 Steps 1 and 5;
§7 "CI cannot do this" → Task 5's flag doc comment and Task 6 Step 2; §8.1 the
pin → Task 7; §10 testing → the test steps of Tasks 1-5.

**Two spec items deliberately not in this plan**, both recorded here so the gap
is a decision and not an oversight:

- **§8.2, generating the README table in `~/.nix` with `builtins.fromTOML`.**
  It is a separate repository, a separate language, and it shares no code with
  anything here. It deserves its own short plan. The drift it fixes is real
  (`configs/shortcuts/README.md` lists `d = DeepSeek` for Windows;
  `apps.windows.toml` has no such row) and it is not urgent.
- **§8.3, the `apps.*.toml` CI glob tripwire.** Recorded in the spec as a note,
  not a change — there is nothing to do while the three-file layout stands.

**Placeholder scan.** No "TBD", no "handle errors appropriately", no "similar to
Task N". Every code step carries the code. Task 6 Step 3 is the one step that
says "match the surrounding formatting" rather than quoting the target text —
that is because the README section it edits was not read while writing this
plan, and inventing its exact shape would be worse than naming the constraint.

**Type consistency.** `Certainty`, `NameReport`, `Summary`, `summarize`,
`mark()`, `word()`, `line()` are defined in Task 1 and used with those exact
names and signatures in Tasks 2, 3, 4 and 5. `resolve_reports` has the same
signature — `(ids: &[&str]) -> beckon_core::Result<Vec<NameReport>>` — at the
crate root of all three backends, which is what lets Task 5's dispatch have one
body per arm. The internal pure functions differ per backend on purpose and are
named in each task's **Interfaces** block: macOS takes a lazy loader because its
running tiers can avoid the scan entirely, Windows and Linux take a slice
because they have no running tier in the resolver.

**One risk this plan does not remove.** Tasks 3 and 4 are verified by CI, not
locally: `cargo test -p beckon-linux` on a macOS host runs zero tests while
printing `ok`, and the Windows tests need a Windows machine. The plan states
this in each task rather than pretending the local gate covers it.
