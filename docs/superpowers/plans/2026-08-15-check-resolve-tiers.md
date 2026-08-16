# `check --resolve` grows three grades — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Widen `beckon check --resolve` from "does this name resolve, yes or
no" to three grades — `Exact`, `Guess`, `NoMatch` — so a binding that resolves
only by substring is reported as the latent hazard it is, without changing what
already exits 1.

**Architecture:** The grade already exists in every backend and is thrown away
on exactly one line each. Each backend maps its own `MatchType` onto
`beckon_core::certainty::Certainty`, and its `unresolved_names` becomes
`resolve_reports`, returning one `NameReport` per name instead of a miss list.
`check_resolution` in the CLI keeps its one-call batching and splits its single
pass/fail branch into two buckets: `NoMatch` still gates the exit code,
`Guess` only prints.

**Tech Stack:** Rust 2021 workspace; no new dependencies anywhere.

**Spec:** `docs/superpowers/specs/2026-08-14-check-resolve-design.md`
(§6 was reversed on 2026-08-15 — read that section, it is the exit-code rule
this plan implements)

**Supersedes:** `docs/superpowers/plans/2026-08-14-check-resolve.md`. That plan
was written before `check --resolve` existed and would have built a second,
parallel implementation of it. Its **Task 1 is done and still stands** —
`beckon_core::certainty` shipped in `e8f9431` + `7dc438f`. Its Tasks 2-5 are
replaced by this file. Its Task 7 (`~/.nix` flake pin) is untouched and still
outstanding; do it from that plan.

## Global Constraints

- **CI runs four jobs** (`.github/workflows/ci.yml`): `rustfmt` on ubuntu, and
  `build & test` on `ubuntu-latest` / `macos-latest` / `windows-latest`.
- **Per-OS exclude flags, verbatim:**
  - ubuntu: `--exclude beckon-macos --exclude beckon-windows`
  - macos: `--exclude beckon-linux --exclude beckon-windows`
  - windows: `--exclude beckon-linux --exclude beckon-macos`
- **The cargo commands CI runs:** `cargo build --workspace <excl> --all-targets`,
  `cargo test --workspace <excl>`,
  `cargo clippy --workspace <excl> --all-targets -- -D warnings`,
  `cargo fmt --all -- --check`. `cargo test` does **not** get `--all-targets`.
- **There is also a `cargo check --workspace --all-targets` step with no
  excludes**, on the Linux and macOS legs. Since `nix/package.nix` now passes
  `-p beckon-cli --bin beckon`, **nix no longer compiles `beckon-windows` at
  all**, and that unexcluded step is the only thing in the project that
  compiles the Windows crate off-Windows. Do not break it and do not assume
  nix covers it.
- **`beckon-core` and `beckon-cli` are excluded on no runner** — anything added
  there must compile and pass tests on all three platforms.
- **`beckon-core` stays dependency-light**: `thiserror`, `toml`, `toml_edit`
  only.
- **Export `CARGO_TARGET_DIR=/Users/lenamkhanh/Documents/dev/beckon/target`**
  before any cargo command. `target/` is ~7.4 GB and this is a worktree; a cold
  rebuild costs many minutes.
- **`cargo test -p beckon-cli` produces FALSE FAILURES on this macOS host.**
  Measured by another session: the same test binary run directly passes 5/5,
  run through `cargo test` fails 5/5 with empty stderr, because the tests spawn
  `beckon` and the grandchild is killed. **Linux is the arbiter.** Do not
  "fix" a beckon-cli test failure seen only on this Mac without reproducing it
  on Linux first. Unit tests inside `beckon-cli/src/lib.rs` are unaffected —
  they spawn nothing.
- **This work happens in the worktree**
  `/Users/lenamkhanh/Documents/dev/beckon/.worktrees/check-resolve`, branch
  `check-resolve`. Never write under `/Users/lenamkhanh/Documents/dev/beckon`
  itself; other sessions share it.
- **Commit messages carry no `Co-Authored-By` line.**
- **Run `cargo fmt --all` before every commit — the code blocks in this plan
  are NOT rustfmt-normalised.** Two of them were transcribed faithfully into
  Task 3 and broke the `rustfmt` CI job: a `const` the author split across two
  lines that fits on one, and a signature one character over the 100-column
  limit. rustfmt does **not** evaluate `cfg` when it walks the module tree
  (measured, and recorded in `CLAUDE.md` as a refutation of the opposite
  belief), so a `#[cfg(target_os = "windows")]`-gated file is not exempt on a
  Linux runner. Let rustfmt decide the layout rather than hand-editing to
  match; CI compares against what rustfmt computes.

### Behaviour that is pinned by a test and must survive

Another session owns these tests and asked that they not be lost. Each names
the property, not just the assertion:

- `check_without_resolve_says_nothing_about_whether_the_app_exists`
  (`tests/check.rs`) — bare `check` consults the machine **not at all** and
  exits 0 on a name nothing can resolve. This is what keeps CI green on a
  runner with none of the apps installed.
- `resolve_belongs_to_check_and_nowhere_else` (`tests/cli_surface.rs`) —
  `--resolve` is a usage error (exit 2) anywhere but inside `check`.
- `resolution_asks_the_resolver_once_for_each_distinct_name`
  (`beckon-cli/src/lib.rs` unit tests) — the resolver closure is called
  **exactly once**, with the sorted deduplicated name list. The batching is the
  reason a shortcuts file costs one catalog scan and not eighteen.
- `resolution_propagates_a_resolver_that_could_not_answer` — a resolver `Err`
  propagates verbatim; it is never reinterpreted as "nothing resolved".
- These exact strings are asserted somewhere and must keep their current
  meaning: `ok: {} shortcuts`, `ok: every app name resolves on this machine`,
  `{} of {} shortcuts name an app that does not resolve on this machine`, and
  `unresolved_report`'s header and `beckon resolve <ID>` hint.

### The exit-code rule this plan implements

| Grade | Exit code | Printed |
|---|---|---|
| `NoMatch` | **non-zero**, unchanged | yes, in the existing block |
| `Guess` | **no effect** | yes, in a new block |
| `Exact` | no effect | no |

`Guess` not failing is deliberate and was decided by the user: two of their own
bindings live on the substring tier on purpose (`Settings` matching *System
Settings*, `DeepSeek` matching *DeepSeek - Into the Unknown*), so failing on
`Guess` would turn a correct file red.

---

### Task 2: macOS — grade the match instead of discarding it

**Files:**
- Modify: `crates/beckon-macos/src/apps.rs` (add `MatchType::certainty`;
  replace `resolves_in` with a grading function)
- Modify: `crates/beckon-macos/src/lib.rs` (replace `unresolved_names` with
  `resolve_reports`, both cfg arms)
- Test: inline `#[cfg(test)] mod tests` in `crates/beckon-macos/src/apps.rs`
  (reuse its existing `rref` / `installed` helpers)

**Interfaces:**
- Consumes: `beckon_core::certainty::{Certainty, NameReport}`.
- Produces: `beckon_macos::resolve_reports(names: &[&str]) -> beckon_core::Result<Vec<NameReport>>`,
  one report per name, **in the order given**.

**Background:** `apps::resolves_in` (`apps.rs`) already calls the full
`resolve_inner`, which returns `Option<ResolvedMatch>` with `match_type`
populated on every `Some` arm — and then throws all of it away with
`.is_some()`. That one projection is what this task removes. `mod apps` is
**private** in `lib.rs`, so anything public must be re-exported there.
`RunningAppInfo` holds a live `Retained<NSRunningApplication>` and cannot be
built in a test, which is why the resolver takes `RunningRef`.

- [ ] **Step 1: Write the failing tests**

Append to the existing `mod tests` in `crates/beckon-macos/src/apps.rs`.

```rust
    // ---------- certainty ----------

    /// Exactly one tier is a guess. Listed rather than looped so that adding a
    /// `MatchType` variant fails to compile in `certainty()` itself — the
    /// wildcard-free match there is the real guard; this pins its answer.
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

    // ---------- reports ----------

    #[test]
    fn every_name_gets_one_report_in_the_order_asked() {
        let reports = reports_test(
            &["Claude", "nope-zzz", "Brave Browser"],
            &[],
            vec![
                installed("com.anthropic.claude", "Claude"),
                installed("com.brave.Browser", "Brave Browser"),
            ],
        );
        let ids: Vec<&str> = reports.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(ids, vec!["Claude", "nope-zzz", "Brave Browser"]);
    }

    #[test]
    fn an_exact_name_has_nothing_to_warn_about() {
        use beckon_core::certainty::Certainty;
        let reports = reports_test(&["Claude"], &[], vec![installed("com.anthropic.claude", "Claude")]);
        let r = &reports[0];
        assert_eq!(r.certainty, Certainty::Exact);
        assert_eq!(r.target.as_deref(), Some("com.anthropic.claude"));
        assert_eq!(r.tier, Some("installed app name (exact)"));
        assert!(r.consequence.is_empty());
        assert!(r.suggestions.is_empty());
    }

    /// A running app grades `Exact` too. `Finder` lives in
    /// /System/Library/CoreServices, which `installed_apps()` does not walk,
    /// so the running tier is the only thing that finds it — and it is an
    /// exact name match, not a guess.
    #[test]
    fn a_running_only_app_is_exact_not_a_guess() {
        use beckon_core::certainty::Certainty;
        let running = vec![rref("com.apple.finder", "Finder")];
        let reports = reports_test(&["Finder"], &running, Vec::new());
        assert_eq!(reports[0].certainty, Certainty::Exact);
        assert_eq!(reports[0].tier, Some("running app localizedName (exact)"));
    }

    /// One candidate: the hazard is that a future install can take the name.
    #[test]
    fn a_lone_substring_match_says_a_new_install_could_take_it() {
        use beckon_core::certainty::Certainty;
        let reports = reports_test(
            &["brave"],
            &[],
            vec![installed("com.brave.Browser", "Brave Browser")],
        );
        let r = &reports[0];
        assert_eq!(r.certainty, Certainty::Guess);
        assert_eq!(r.tier, Some("installed app name substring"));
        assert!(r.suggestions.is_empty(), "{:?}", r.suggestions);
        assert!(
            r.consequence.contains("install"),
            "consequence was {:?}",
            r.consequence
        );
    }

    /// Several candidates is the worse case and must read differently: the
    /// winner is decided by sort order, so which app the key opens is a
    /// property of the catalog, not of the config. Measured on another
    /// machine before `desktop::scan()` was sorted: 20 runs split 12/8
    /// between two entries.
    #[test]
    fn several_substring_candidates_name_the_winner_and_the_runners_up() {
        use beckon_core::certainty::Certainty;
        let reports = reports_test(
            &["brave"],
            &[],
            vec![
                installed("com.brave.Browser", "Brave Browser"),
                installed("com.brave.Browser.beta", "Brave Browser Beta"),
            ],
        );
        let r = &reports[0];
        assert_eq!(r.certainty, Certainty::Guess);
        assert_eq!(r.target.as_deref(), Some("com.brave.Browser"));
        assert!(
            r.consequence.contains('2'),
            "the count must be in the sentence: {:?}",
            r.consequence
        );
        assert_eq!(r.suggestions, vec!["Brave Browser Beta".to_string()]);
    }

    /// A total miss has no suggestions to give and must not invent any: the
    /// substring tier IS the last tier, so nothing matched by any measure
    /// this crate owns.
    #[test]
    fn a_total_miss_carries_no_target_no_tier_and_no_suggestions() {
        use beckon_core::certainty::Certainty;
        let reports = reports_test(&["zalo"], &[], vec![installed("com.apple.finder", "Finder")]);
        let r = &reports[0];
        assert_eq!(r.certainty, Certainty::NoMatch);
        assert_eq!(r.target, None);
        assert_eq!(r.tier, None);
        assert!(r.suggestions.is_empty());
        assert!(!r.consequence.is_empty());
    }

    /// The catalog is walked once for the whole batch, not once per name.
    /// `installed_apps()` reads three roots and one Info.plist per bundle;
    /// an eighteen-binding file is ordinary.
    #[test]
    fn the_installed_catalog_is_loaded_at_most_once_for_a_batch() {
        use std::cell::Cell;
        let calls = Cell::new(0usize);
        let reports = resolve_reports_in(&["a-zzz", "b-zzz", "c-zzz"], &[], || {
            calls.set(calls.get() + 1);
            Vec::new()
        });
        assert_eq!(reports.len(), 3);
        assert_eq!(calls.get(), 1);
    }
```

And add this helper beside the module's existing `resolve_test`, so the tests
above read as one line each:

```rust
    fn reports_test(
        names: &[&str],
        running: &[RunningRef],
        installed: Vec<InstalledAppInfo>,
    ) -> Vec<beckon_core::certainty::NameReport> {
        resolve_reports_in(names, running, move || installed)
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p beckon-macos`
Expected: FAIL — `no method named 'certainty'`, `cannot find function
'resolve_reports_in'`.

- [ ] **Step 3: Add the certainty mapping**

Extend the existing `impl MatchType` block in `crates/beckon-macos/src/apps.rs`
— the one that already holds `describe`:

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

- [ ] **Step 4: Replace `resolves_in` with the report builder**

Delete `pub(crate) fn resolves_in` entirely — its only caller is
`unresolved_names`, which Step 5 replaces — and put this in its place:

```rust
/// What a keypress costs when a name matched only by substring and exactly one
/// app answered it.
const GUESS_LONE: &str =
    "substring match, so an app installed later can quietly take this name";

/// What a miss means on macOS.
const MISS_CONSEQUENCE: &str = "no match; this key will error and launch nothing";

/// The report for one already-resolved name.
///
/// `installed` is passed so a guess can name its rivals. It is empty when the
/// match came from the running tiers, which is correct: those are exact, and
/// an exact match has no rivals worth printing.
fn report_for(id: &str, m: &ResolvedMatch, installed: &[InstalledAppInfo]) -> NameReport {
    let certainty = m.match_type.certainty();
    let (consequence, suggestions) = if certainty == Certainty::Guess {
        let needle = normalize(id);
        let mut others: Vec<String> = installed
            .iter()
            .filter(|a| normalize(&a.name).contains(&needle) && a.bundle_id != m.bundle_id)
            .map(|a| a.name.clone())
            .collect();
        others.sort();
        // Several candidates is a different hazard from one, and the worse of
        // the two: the winner is whichever sorts first, so which app the key
        // opens is a property of the catalog rather than of the config, and
        // one install can reverse it.
        let sentence = if others.is_empty() {
            GUESS_LONE.to_string()
        } else {
            format!(
                "substring match with {} candidates; \"{}\" wins only because it sorts first",
                others.len() + 1,
                m.display_name
            )
        };
        others.truncate(3);
        (sentence, others)
    } else {
        (String::new(), Vec::new())
    };
    NameReport {
        id: id.to_string(),
        certainty,
        target: Some(m.bundle_id.clone()),
        tier: Some(m.match_type.describe()),
        consequence,
        suggestions,
    }
}

/// One `NameReport` per name, in the order given, against caller-supplied
/// snapshots. `installed_loader` runs at most once for the whole batch, and
/// not at all when every name resolves from the running tiers.
pub(crate) fn resolve_reports_in(
    names: &[&str],
    running: &[RunningRef<'_>],
    installed_loader: impl FnOnce() -> Vec<InstalledAppInfo>,
) -> Vec<NameReport> {
    let mut loader = Some(installed_loader);
    let mut installed: Option<Vec<InstalledAppInfo>> = None;
    let mut out = Vec::with_capacity(names.len());

    for id in names {
        // `|_| None` for the bundle path: the report names the bundle id and
        // never the path, and `bundle_path_for` is an NSWorkspace round trip
        // on every running match.
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

/// One `NameReport` per name, against this machine.
pub fn resolve_reports(names: &[&str]) -> Vec<NameReport> {
    let running = running_apps();
    let refs: Vec<RunningRef<'_>> = running.iter().map(RunningRef::from).collect();
    resolve_reports_in(names, &refs, installed_apps)
}
```

This needs `resolve_running_in` and `resolve_installed_in`, which do not exist
yet. Split `resolve_inner` into them — a behaviour-preserving extraction; the
module's existing `resolve_inner` tests are the regression gate:

```rust
/// The two running-app tiers, split out so a batch caller can run them
/// without holding an installed-app scan.
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

/// The three installed-app tiers, against a caller-supplied catalog.
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

Add the import at the top of `crates/beckon-macos/src/apps.rs`:

```rust
use beckon_core::certainty::{Certainty, NameReport};
```

- [ ] **Step 5: Swap the crate's public entry point**

In `crates/beckon-macos/src/lib.rs`, replace **both** cfg arms of
`unresolved_names` with:

```rust
/// One resolution report per name, for `beckon check --resolve`.
///
/// A batch rather than a loop over `apps::resolve`, because the scans are the
/// expensive half and they are per-call there: `installed_apps()` walks three
/// roots one level deep and reads one `Info.plist` per bundle, and a shortcuts
/// file with eighteen bindings is an ordinary one.
///
/// `running_apps()` is part of the answer because it is tiers 1 and 2 of the
/// ladder — an app running but installed somewhere this scan does not reach
/// still resolves, and resolves *exactly*. `Finder` is the everyday case: its
/// bundle is `/System/Library/CoreServices/Finder.app`, under none of the
/// three roots. That is the one place this answer depends on the session
/// rather than on the disk, and it matches what `beckon resolve` reports.
#[cfg(target_os = "macos")]
pub fn resolve_reports(names: &[&str]) -> Result<Vec<beckon_core::certainty::NameReport>> {
    Ok(apps::resolve_reports(names))
}

/// Returns an error rather than an empty vector: an empty one reads as
/// "every name resolved", which is the one answer this cannot know.
#[cfg(not(target_os = "macos"))]
pub fn resolve_reports(_names: &[&str]) -> Result<Vec<beckon_core::certainty::NameReport>> {
    Err(BackendError::UnsupportedEnvironment(
        "beckon-macos only compiles on macOS".to_string(),
    ))
}
```

The CLI still calls `unresolved_names` at this point, so the workspace will not
build until Task 5. That is expected and is why Task 5 exists; do not add a
compatibility shim.

- [ ] **Step 6: Run the tests**

Run: `cargo test -p beckon-macos`
Expected: PASS — the seven new tests **and** every pre-existing `resolve_inner`
test. Those are the regression gate for the Step 4 extraction; if one fails, the
extraction changed behaviour and the extraction is what to fix.

- [ ] **Step 7: Commit**

```bash
git add crates/beckon-macos/src/apps.rs crates/beckon-macos/src/lib.rs
git commit -m "macos: report which tier a name matched at, not just whether it did"
```

---

### Task 3: Windows — the same change, one crate over

**Files:**
- Modify: `crates/beckon-windows/src/apps.rs` (add `MatchType::certainty` and
  the report builder)
- Modify: `crates/beckon-windows/src/lib.rs` (replace `unresolved_names`)
- Test: inline `#[cfg(test)] mod tests` in `crates/beckon-windows/src/apps.rs`
  (reuse its existing `app` / `appx` helpers and its `use std::cell::Cell;`)

**Interfaces:**
- Consumes: `beckon_core::certainty::{Certainty, NameReport}`.
- Produces: `beckon_windows::resolve_reports(names: &[&str]) -> beckon_core::Result<Vec<NameReport>>`,
  one report per name, in the order given.

**Background:** simpler than macOS — `apps::resolve(id, &installed)` already
takes a pre-scanned slice and there are no running-app tiers, so one
`scan_installed_apps()` plus a loop is the whole story. Keep the **full**
scan: `resolve_lazy` can answer `InstalledName` without ever enumerating
AppsFolder, and the landed code's own doc comment records that completeness is
deliberate here. `mod apps` is `pub` in this crate, unlike beckon-macos.

- [ ] **Step 1: Write the failing tests**

Append to `mod tests` in `crates/beckon-windows/src/apps.rs`:

```rust
    // ---------- certainty ----------

    /// Exactly one tier is a guess. `InstalledExeStem` looks fuzzy and is not:
    /// it is `a.exe_name == needle_exe`, whole-string equality.
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

    // ---------- reports ----------

    #[test]
    fn every_name_gets_one_report_in_the_order_asked() {
        let installed = vec![app("Claude", "claude.exe"), app("Brave", "brave.exe")];
        let reports = resolve_reports_in(&["Brave", "Claude", "nope-zzz"], &installed);
        let ids: Vec<&str> = reports.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(ids, vec!["Brave", "Claude", "nope-zzz"]);
    }

    #[test]
    fn an_exact_name_has_nothing_to_warn_about() {
        use beckon_core::certainty::Certainty;
        let installed = vec![app("Claude", "claude.exe")];
        let r = &resolve_reports_in(&["Claude"], &installed)[0];
        assert_eq!(r.certainty, Certainty::Exact);
        assert_eq!(r.tier, Some("Start Menu/app display name (exact)"));
        assert!(r.consequence.is_empty());
        assert!(r.suggestions.is_empty());
    }

    /// The exe names are deliberately not `brave.exe`: tier 3 is
    /// `a.exe_name == "brave.exe"`, which would match the id `brave` exactly
    /// and grade `Exact` before the substring tier is reached.
    #[test]
    fn several_substring_candidates_name_the_winner_and_the_runners_up() {
        use beckon_core::certainty::Certainty;
        let installed = vec![
            app("Brave Browser", "bravebrowser.exe"),
            app("Brave Browser Beta", "bravebeta.exe"),
        ];
        let r = &resolve_reports_in(&["brave"], &installed)[0];
        assert_eq!(r.certainty, Certainty::Guess);
        assert_eq!(r.tier, Some("Start Menu/app display name (substring)"));
        assert!(r.consequence.contains('2'), "{:?}", r.consequence);
        assert_eq!(r.suggestions, vec!["Brave Browser Beta".to_string()]);
    }

    #[test]
    fn a_lone_substring_match_says_a_new_install_could_take_it() {
        use beckon_core::certainty::Certainty;
        let installed = vec![app("Brave Browser", "bravebrowser.exe")];
        let r = &resolve_reports_in(&["brave"], &installed)[0];
        assert_eq!(r.certainty, Certainty::Guess);
        assert!(r.suggestions.is_empty());
        assert!(r.consequence.contains("install"), "{:?}", r.consequence);
    }

    /// On Windows a miss is not the end of the story — the window-matching
    /// layer still tries exe name and window title — so the sentence must not
    /// claim the key does nothing.
    #[test]
    fn a_miss_says_what_windows_actually_does_next() {
        use beckon_core::certainty::Certainty;
        let installed = vec![app("Claude", "claude.exe")];
        let r = &resolve_reports_in(&["zalo"], &installed)[0];
        assert_eq!(r.certainty, Certainty::NoMatch);
        assert_eq!(r.target, None);
        assert!(r.consequence.contains("title"), "{}", r.consequence);
    }

    /// A packaged app reports its AUMID, because that is what activation uses.
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

From this macOS host the compile-only equivalent is:
`cargo check -p beckon-windows --target aarch64-pc-windows-msvc --all-targets`
Expected: FAIL — `no method named 'certainty'`, `cannot find function
'resolve_reports_in'`.

- [ ] **Step 3: Add the certainty mapping**

Extend the existing `impl MatchType` block in
`crates/beckon-windows/src/apps.rs`:

```rust
    /// How sure this tier is, in the cross-OS vocabulary.
    ///
    /// Exhaustive with no wildcard arm on purpose. `InstalledExeStem` is
    /// `Exact`: it compares `a.exe_name == needle_exe`, whole-string
    /// equality, not a substring.
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

- [ ] **Step 4: Add the report builder**

Append to `crates/beckon-windows/src/apps.rs`, before the `#[cfg(test)]` module:

```rust
/// What a keypress costs when a name matched only by substring and exactly one
/// app answered it.
const GUESS_LONE: &str =
    "substring match, so an app installed later can quietly take this name";

/// What happens on a miss. Not "nothing": the window-matching layer still
/// tries the exe name and then the window title, so a miss can still focus
/// something — it just can never launch.
const MISS_CONSEQUENCE: &str =
    "no installed app; focus may still match by exe or window title, launch will fail";

fn report_for(id: &str, m: &ResolvedMatch, installed: &[InstalledAppInfo]) -> NameReport {
    let certainty = m.match_type.certainty();
    let (consequence, suggestions) = if certainty == Certainty::Guess {
        let needle = normalize(id);
        let mut others: Vec<String> = installed
            .iter()
            .filter(|a| {
                normalize(&a.name).contains(&needle) && normalize(&a.name) != normalize(&m.name)
            })
            .map(|a| a.name.clone())
            .collect();
        others.sort();
        let sentence = if others.is_empty() {
            GUESS_LONE.to_string()
        } else {
            format!(
                "substring match with {} candidates; \"{}\" wins only because it sorts first",
                others.len() + 1,
                m.name
            )
        };
        others.truncate(3);
        (sentence, others)
    } else {
        (String::new(), Vec::new())
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
        consequence,
        suggestions,
    }
}

/// One `NameReport` per name, in the order given, against a caller-supplied
/// catalog.
pub(crate) fn resolve_reports_in(names: &[&str], installed: &[InstalledAppInfo]) -> Vec<NameReport> {
    names
        .iter()
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

/// One `NameReport` per name, against this machine, with a single catalog scan.
pub fn resolve_reports(names: &[&str]) -> Vec<NameReport> {
    let installed = scan_installed_apps();
    resolve_reports_in(names, &installed)
}
```

Add the import at the top of the file:

```rust
use beckon_core::certainty::{Certainty, NameReport};
```

- [ ] **Step 5: Swap the crate's public entry point**

In `crates/beckon-windows/src/lib.rs`, replace **both** cfg arms of
`unresolved_names` with:

```rust
/// One resolution report per name, for `beckon check --resolve`.
///
/// A batch rather than a loop, and deliberately over the full
/// `scan_installed_apps()` rather than `resolve_lazy`: the AppsFolder half of
/// that scan costs several hundred milliseconds, so paying it eighteen times
/// for an eighteen-binding file is the thing to avoid, while paying it once
/// buys the same completeness `installed` / `resolve` are given.
#[cfg(target_os = "windows")]
pub fn resolve_reports(names: &[&str]) -> Result<Vec<beckon_core::certainty::NameReport>> {
    Ok(apps::resolve_reports(names))
}

/// Returns an error rather than an empty vector: an empty one reads as
/// "every name resolved", which is the one answer this cannot know.
#[cfg(not(target_os = "windows"))]
pub fn resolve_reports(_names: &[&str]) -> Result<Vec<beckon_core::certainty::NameReport>> {
    Err(BackendError::UnsupportedEnvironment(
        "beckon-windows only runs on Windows".to_string(),
    ))
}
```

- [ ] **Step 6: Verify from this host**

Run: `cargo check -p beckon-windows --target aarch64-pc-windows-msvc --all-targets`
Expected: clean. This compiles the code and the test module without MSVC; the
tests themselves run on the Windows CI job. If a cold cache produces a SIGKILL
with empty output, re-run — it converges.

- [ ] **Step 7: Commit**

```bash
git add crates/beckon-windows/src/apps.rs crates/beckon-windows/src/lib.rs
git commit -m "windows: report which tier a name matched at, not just whether it did"
```

---

### Task 4: Linux — the same change, and the tier it already computes

**Files:**
- Modify: `crates/beckon-linux/src/desktop.rs` (add `MatchType::certainty` and
  the report builder)
- Modify: `crates/beckon-linux/src/lib.rs` (replace `unresolved_names`)
- Test: inline `#[cfg(test)] mod tests` in `crates/beckon-linux/src/desktop.rs`
  (reuse its `entry` / `entry_with_wm` helpers)

**Interfaces:**
- Consumes: `beckon_core::certainty::{Certainty, NameReport}`.
- Produces: `beckon_linux::resolve_reports(names: &[&str]) -> beckon_core::Result<Vec<NameReport>>`.

**Background the implementer needs:**

1. **These tests will not run on this macOS host.** `mod desktop` is
   `#[cfg(target_os = "linux")]`, and `cargo test -p beckon-linux` on macOS
   prints `ok` while running **zero** tests. The Linux CI job is the only
   evidence. Do not read a local green as a pass.
2. **Do not call `name_substring_matches` for suggestions.** Unlike its macOS
   and Windows namesakes it takes no catalog argument and calls `scan()`
   itself, so using it in the loop would walk every XDG applications directory
   once per name. Inline the filter over the `entries` slice, as the code below
   does.
3. **`NoMatch` on Linux does not mean "will not work".** When nothing resolves,
   `target_classes` falls back to `Target::new([raw_id])` and `Target::matches`
   is case-insensitive **equality** — the same strength as the `Filename` tier.
   That is what lets beckon focus an ad-hoc app with no `.desktop` file. The
   consequence sentence must say so.
4. This backend is where the non-determinism was measured: before `scan()`
   sorted its output, 20 runs of `beckon resolve` split 12/8 between two
   entries sharing a `Name=`. The multi-candidate sentence exists for this.

- [ ] **Step 1: Write the failing tests**

Append to `mod tests` in `crates/beckon-linux/src/desktop.rs`:

```rust
    // ---------- certainty ----------

    /// Exactly one tier is a guess. `Filename` and `StartupWmClass` are
    /// byte-exact comparisons against the raw id.
    #[test]
    fn only_the_substring_tier_is_a_guess() {
        use beckon_core::certainty::Certainty;
        assert_eq!(MatchType::NameExact.certainty(), Certainty::Exact);
        assert_eq!(MatchType::Filename.certainty(), Certainty::Exact);
        assert_eq!(MatchType::StartupWmClass.certainty(), Certainty::Exact);
        assert_eq!(MatchType::NameSubstring.certainty(), Certainty::Guess);
    }

    // ---------- reports ----------

    #[test]
    fn every_name_gets_one_report_in_the_order_asked() {
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

    /// The ids matter: tier 4 sorts candidates by `id`, so `brave-beta` would
    /// win over `brave-browser` and invert these assertions. Name them so the
    /// intended winner sorts first.
    #[test]
    fn several_substring_candidates_name_the_winner_and_the_runners_up() {
        use beckon_core::certainty::Certainty;
        let entries = vec![
            entry("brave-browser", "Brave Web Browser"),
            entry("brave-browser-beta", "Brave Web Browser Beta"),
        ];
        let r = &resolve_reports_in(&["brave"], &entries)[0];
        assert_eq!(r.certainty, Certainty::Guess);
        assert_eq!(r.tier, Some("Name= substring (alphabetical first wins)"));
        assert!(r.consequence.contains('2'), "{:?}", r.consequence);
        assert_eq!(
            r.suggestions,
            vec!["Brave Web Browser Beta".to_string()]
        );
    }

    #[test]
    fn a_lone_substring_match_says_a_new_install_could_take_it() {
        use beckon_core::certainty::Certainty;
        let entries = vec![entry("brave-browser", "Brave Web Browser")];
        let r = &resolve_reports_in(&["brave"], &entries)[0];
        assert_eq!(r.certainty, Certainty::Guess);
        assert!(r.suggestions.is_empty());
        assert!(r.consequence.contains("install"), "{:?}", r.consequence);
    }

    /// A miss on Linux is not fatal: the raw id becomes the window class and
    /// `Target::matches` is equality, so an ad-hoc app with no `.desktop`
    /// file is still focusable. Saying "this key does nothing" would be wrong.
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

If a Linux target is installed:
`cargo check -p beckon-linux --target x86_64-unknown-linux-gnu --all-targets`
Otherwise push the branch and read the ubuntu CI job.
Expected: FAIL — `no method named 'certainty'`, `cannot find function
'resolve_reports_in'`.

**Do not** run `cargo test -p beckon-linux` on macOS and read `ok` as a result.

- [ ] **Step 3: Add the certainty mapping**

Extend the existing `impl MatchType` block in
`crates/beckon-linux/src/desktop.rs`:

```rust
    /// How sure this tier is, in the cross-OS vocabulary.
    ///
    /// Exhaustive with no wildcard arm on purpose. `Filename` and
    /// `StartupWmClass` are byte-exact comparisons against the raw id, so
    /// they are `Exact` despite being weaker tiers than `NameExact`.
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

- [ ] **Step 4: Add the report builder**

Append to `crates/beckon-linux/src/desktop.rs`, before the `#[cfg(test)]`
module:

```rust
/// What a keypress costs when a name matched only by substring and exactly one
/// entry answered it.
const GUESS_LONE: &str = "substring match, so an app installed later can quietly take this name";

/// What a miss means on Linux. Not "nothing happens": `target_classes` falls
/// back to the raw id as a window class, and that comparison is equality — so
/// an ad-hoc app with no `.desktop` file is still focusable.
const MISS_CONSEQUENCE: &str =
    "no .desktop entry; focus still works if a window's class equals this id, launch will fail";

fn report_for(id: &str, m: &ResolvedMatch, entries: &[DesktopEntry]) -> NameReport {
    let certainty = m.match_type.certainty();
    let (consequence, suggestions) = if certainty == Certainty::Guess {
        let needle = normalize(id);
        // Deliberately not `name_substring_matches`: that one calls `scan()`
        // itself, which would walk every XDG applications directory again,
        // once per name.
        let mut others: Vec<String> = entries
            .iter()
            .filter(|e| normalize(&e.name).contains(&needle) && e.id != m.entry.id)
            .map(|e| e.name.clone())
            .collect();
        others.sort();
        // The multi-candidate sentence exists because the winner is decided by
        // sort order over `.desktop` ids: which app the key opens is a
        // property of the catalog, not of the config, and one install can
        // reverse it. Before `scan()` sorted its output the same keypress
        // resolved two different ways across runs.
        let sentence = if others.is_empty() {
            GUESS_LONE.to_string()
        } else {
            format!(
                "substring match with {} candidates; \"{}\" wins only because it sorts first",
                others.len() + 1,
                m.entry.name
            )
        };
        others.truncate(3);
        (sentence, others)
    } else {
        (String::new(), Vec::new())
    };
    NameReport {
        id: id.to_string(),
        certainty,
        target: Some(m.entry.id.clone()),
        tier: Some(m.match_type.describe()),
        consequence,
        suggestions,
    }
}

/// One `NameReport` per name, in the order given, against a caller-supplied
/// entry list.
pub fn resolve_reports_in(names: &[&str], entries: &[DesktopEntry]) -> Vec<NameReport> {
    names
        .iter()
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

/// One `NameReport` per name, against this machine, with a single `scan()`.
pub fn resolve_reports(names: &[&str]) -> Vec<NameReport> {
    resolve_reports_in(names, &scan())
}
```

Add the import at the top of the file:

```rust
use beckon_core::certainty::{Certainty, NameReport};
```

- [ ] **Step 5: Swap the crate's public entry point**

In `crates/beckon-linux/src/lib.rs`, replace **both** cfg arms of
`unresolved_names` with:

```rust
/// One resolution report per name, for `beckon check --resolve`.
///
/// A batch rather than a loop over `desktop::resolve_detailed`, which re-runs
/// `scan()` — every `applications/` directory in `$XDG_DATA_DIRS`, recursively
/// — on every call.
///
/// Takes no backend: this is the resolution half of step 2, and `.desktop`
/// files are on disk whether or not a compositor is running.
#[cfg(target_os = "linux")]
pub fn resolve_reports(names: &[&str]) -> Result<Vec<beckon_core::certainty::NameReport>> {
    Ok(desktop::resolve_reports(names))
}

/// Returns an error rather than an empty vector: an empty one reads as
/// "every name resolved", which is the one answer this cannot know.
#[cfg(not(target_os = "linux"))]
pub fn resolve_reports(_names: &[&str]) -> Result<Vec<beckon_core::certainty::NameReport>> {
    Err(BackendError::UnsupportedEnvironment(
        "beckon-linux only compiles on Linux".to_string(),
    ))
}
```

- [ ] **Step 6: Commit and verify on CI**

```bash
git add crates/beckon-linux/src/desktop.rs crates/beckon-linux/src/lib.rs
git commit -m "linux: report which tier a name matched at, not just whether it did"
```

The workspace does not build until Task 5 lands, so CI cannot be green yet.
Verify the syntax locally with the `cargo check --target` command from Step 2
if the target is installed; otherwise this task's evidence arrives with Task 5's
CI run.

---

### Task 5: The CLI — two buckets, one exit gate

**Files:**
- Modify: `crates/beckon-cli/src/lib.rs` (`unresolved_names` →
  `name_reports`; `check_resolution`; `unresolved_report`; the unit tests in
  its `#[cfg(test)] mod tests`)
- Test: the same inline `mod tests`, plus `crates/beckon-cli/tests/check.rs`

**Interfaces:**
- Consumes: `beckon_{macos,windows,linux}::resolve_reports(names: &[&str]) ->
  beckon_core::Result<Vec<NameReport>>` from Tasks 2-4, one report per name in
  the order given.
- Produces: the shipped behaviour.

**Background:** `check_resolution` currently has one failure branch —
`if dead.is_empty() { ok } else { print + Err }` — and the exit code is nothing
more than that `Err` reaching `cli_main`'s `exit(1)`. Splitting it into two
buckets is the whole task. The one-call batching must survive; a test pins the
exact deduplicated, sorted list the closure receives.

- [ ] **Step 1: Write the failing tests**

Replace the resolver stubs in `crates/beckon-cli/src/lib.rs`'s `mod tests` so
they return reports, and add the two new cases. Keep the existing test names —
they are cited elsewhere.

```rust
    fn report(id: &str, certainty: Certainty) -> NameReport {
        NameReport {
            id: id.to_string(),
            certainty,
            target: None,
            tier: None,
            consequence: if certainty == Certainty::Exact {
                String::new()
            } else {
                "because".to_string()
            },
            suggestions: Vec::new(),
        }
    }

    /// A guess resolves. It is slow and fragile, not dead — so it is printed
    /// and the exit code is untouched. This is the rule the whole three-grade
    /// change exists to express, and the one that separates it from the
    /// boolean it replaces.
    #[test]
    fn a_guess_is_reported_and_does_not_fail_the_check() {
        let shortcuts = parse_shortcuts("\"ctrl+alt+a\" = \"Brave\"\n").unwrap();
        let out = check_resolution(&shortcuts, |names| {
            Ok(names.iter().map(|n| report(n, Certainty::Guess)).collect())
        });
        assert!(out.is_ok(), "a guess must not fail the check: {out:?}");
    }

    /// And a guess alongside a miss must not soften the miss.
    #[test]
    fn a_miss_still_fails_even_when_another_row_is_only_a_guess() {
        let shortcuts =
            parse_shortcuts("\"ctrl+alt+a\" = \"Brave\"\n\"ctrl+alt+b\" = \"Zalo\"\n").unwrap();
        let err = check_resolution(&shortcuts, |names| {
            Ok(names
                .iter()
                .map(|n| {
                    if *n == "Zalo" {
                        report(n, Certainty::NoMatch)
                    } else {
                        report(n, Certainty::Guess)
                    }
                })
                .collect())
        })
        .unwrap_err();
        assert!(
            format!("{err}").contains("1 of 2 shortcuts"),
            "the count must name only the dead ones: {err}"
        );
    }
```

Every other test in that module keeps its name and its assertions; only the
closure changes from returning `Vec<&str>` to returning `Vec<NameReport>`. In
particular `resolution_asks_the_resolver_once_for_each_distinct_name` must keep
asserting the exact `["Claude", "Terminal"]` vector the closure receives.

And add one integration test to `crates/beckon-cli/tests/check.rs`, beside the
existing `run_check` helper:

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

/// End to end, against this machine's real catalog: a name nothing can resolve
/// still exits 1, and says which key is dead.
#[test]
fn resolve_still_fails_on_a_name_this_machine_cannot_find() {
    let out = run_check_resolve("\"ctrl+super+alt+t\" = \"beckon-selftest-no-such-app\"\n");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(1), "stderr: {stderr}");
    assert!(stdout.contains("ctrl+super+alt+t"), "stdout: {stdout}");
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p beckon-cli --lib`
Expected: FAIL to compile — the closures now return `Vec<NameReport>` while
`check_resolution` still expects `Vec<&str>`.

Use `--lib` here: the integration tests in `tests/` produce false failures on
this host (see Global Constraints). Their real result comes from CI.

- [ ] **Step 3: Rewire the per-OS dispatch**

Rename the CLI's helper and change its type. Keep every word of the existing
doc comment that is still true — the reasons it gives for not going through
`pick_backend` are unchanged and are the valuable part:

```rust
/// One resolution report per name, from whichever backend this OS has.
///
/// Deliberately does NOT go through `pick_backend`. Resolution reads
/// installed-app metadata — `.desktop` files, LaunchServices, the Start menu
/// — which is on disk whether or not a session is running, so `check
/// --resolve` runs over SSH, in a headless VM and on a CI runner. Taking a
/// backend would make the flag fail for a reason that has nothing to do with
/// the question it asks, and the only alternative to failing there would be to
/// pass silently, which is worse than not having the flag at all.
///
/// The answer is not invariant, though, and the flag must not claim to be:
/// the macOS ladder starts at the running apps, so a bundle installed where
/// `installed_apps()` does not walk — `/System/Library/CoreServices/Finder.app`,
/// measured — resolves only while it is running. That is `resolve`'s own
/// behaviour, which this has to agree with, not something to fix here.
///
/// An OS with no backend crate is the one case left, and it errors: an empty
/// list would read as "every name resolved".
fn name_reports(names: &[&str]) -> Result<Vec<NameReport>> {
    #[cfg(target_os = "linux")]
    {
        beckon_linux::resolve_reports(names)
            .map_err(|e| anyhow!("{e}"))
            .context("resolving app names failed")
    }
    #[cfg(target_os = "macos")]
    {
        beckon_macos::resolve_reports(names)
            .map_err(|e| anyhow!("{e}"))
            .context("resolving app names failed")
    }
    #[cfg(target_os = "windows")]
    {
        beckon_windows::resolve_reports(names)
            .map_err(|e| anyhow!("{e}"))
            .context("resolving app names failed")
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        let _ = names;
        Err(anyhow!(
            "`beckon check --resolve` needs a backend, and this OS is not supported"
        ))
    }
}
```

Update `cmd_check`'s last line to `check_resolution(&shortcuts, name_reports)`
and add the import `use beckon_core::certainty::{Certainty, NameReport};` at the
top of the file.

- [ ] **Step 4: Split `check_resolution` into two buckets**

```rust
/// The `--resolve` half of `check`: what grade does every binding's name earn
/// on this machine?
///
/// The resolver is passed in, so the whole flag — the batching, the counting
/// and the report — is testable on a machine that has none of the apps in
/// question, which is every CI runner and is the exact condition the flag
/// exists to describe.
///
/// **Only `NoMatch` gates the exit code.** A `Guess` resolves; it is slow and
/// fragile, not dead, and two of this user's own bindings live on that tier on
/// purpose. Failing on it would turn a correct file red, which is how a check
/// stops being run.
fn check_resolution<'a>(
    shortcuts: &'a [Shortcut],
    report: impl FnOnce(&[&'a str]) -> Result<Vec<NameReport>>,
) -> Result<()> {
    // Distinct names, asked in one call: several hotkeys aiming at one app is
    // the normal shape of a shortcuts file, and every backend answers a batch
    // with a single catalog scan.
    let mut names: Vec<&str> = shortcuts.iter().map(|s| s.app.as_str()).collect();
    names.sort_unstable();
    names.dedup();
    let reports = report(&names)?;
    let grade: std::collections::HashMap<&str, &NameReport> =
        reports.iter().map(|r| (r.id.as_str(), r)).collect();

    let pick = |want: Certainty| -> Vec<(&Shortcut, &NameReport)> {
        shortcuts
            .iter()
            .filter_map(|s| grade.get(s.app.as_str()).map(|r| (s, *r)))
            .filter(|(_, r)| r.certainty == want)
            .collect()
    };
    let dead = pick(Certainty::NoMatch);
    let guessed = pick(Certainty::Guess);

    if dead.is_empty() {
        println!("ok: every app name resolves on this machine");
    } else {
        print!("{}", unresolved_report(&dead));
    }
    if !guessed.is_empty() {
        print!("{}", guess_report(&guessed));
    }
    if dead.is_empty() {
        return Ok(());
    }
    // The counts live here and nowhere else: the block above lists the
    // bindings, `main` prints this to stderr, and the two do not repeat
    // each other.
    Err(anyhow!(
        "{} of {} shortcuts name an app that does not resolve on this machine",
        dead.len(),
        shortcuts.len()
    ))
}
```

`unresolved_report` keeps its wording and its "one line per binding" rule; only
its parameter type widens, so that it can print nothing new and stay pinned by
its two existing tests:

```rust
fn unresolved_report(dead: &[(&Shortcut, &NameReport)]) -> String {
    let mut s = String::from("\nThese shortcuts name an app this machine has no match for:\n");
    for (b, _) in dead {
        s.push_str(&format!("   {:<30} {}\n", b.combo.canonical(), b.app));
    }
    s.push_str(
        "\nHint: `beckon resolve <ID>` explains one of them; \
         `beckon installed` lists what is installed.\n",
    );
    s
}
```

And the new block, which is where the whole grade earns its keep:

```rust
/// The block for bindings that resolve, but only by substring.
///
/// Separate from `unresolved_report` because it says something different: not
/// "this key is dead" but "this key works today for a reason the config does
/// not state". Each line carries the reason, because the reason is what makes
/// it actionable — a lone substring match invites a future install to steal
/// the name, while several candidates means the winner is already decided by
/// sort order rather than by anything the user wrote.
fn guess_report(guessed: &[(&Shortcut, &NameReport)]) -> String {
    let mut s = String::from("\nThese shortcuts resolve, but only loosely:\n");
    for (b, r) in guessed {
        s.push_str(&format!("   {:<30} {}\n", b.combo.canonical(), b.app));
        if !r.consequence.is_empty() {
            s.push_str(&format!("   {:<30} {}\n", "", r.consequence));
        }
        for other in &r.suggestions {
            s.push_str(&format!("   {:<30} also matches: {}\n", "", other));
        }
    }
    s.push_str("\nThey do not fail this check. Naming the app exactly makes them exact.\n");
    s
}
```

- [ ] **Step 5: Run the tests**

Run:
```bash
cargo test -p beckon-cli --lib
cargo test -p beckon-core
```
Expected: PASS. The `beckon-cli` unit tests include the six pre-existing
`check_resolution` cases — they are the regression gate for this rewiring.

- [ ] **Step 6: See it work on real data**

```bash
cargo run -p beckon-cli --bin beckon -- check --resolve ~/.nix/configs/shortcuts/apps.macos.toml
echo "exit: $?"
```
Expected: `ok: 20 shortcuts`, a `resolve, but only loosely` block naming at
least `Brave` (which matches by substring on this machine — the reason the user
changed it to `Brave Browser` in the file on 10/08), a dead block naming `Zalo`,
and **exit 1** because of `Zalo` alone.

Then the control that proves `Guess` really does not gate:
```bash
printf '"ctrl+super+alt+b" = "Brave"\n' > /tmp/guess-only.toml
cargo run -p beckon-cli --bin beckon -- check --resolve /tmp/guess-only.toml
echo "exit: $?"
```
Expected: the loose block, and **exit 0**.

- [ ] **Step 7: Full local gate**

```bash
cargo fmt --all -- --check
cargo build --workspace --exclude beckon-linux --exclude beckon-windows --all-targets
cargo clippy --workspace --exclude beckon-linux --exclude beckon-windows --all-targets -- -D warnings
cargo check --workspace --all-targets
cargo check -p beckon-windows --target aarch64-pc-windows-msvc --all-targets
```
Expected: all clean. The unexcluded `cargo check --workspace --all-targets` is
the step that compiles `beckon-windows` off-Windows — do not skip it.

- [ ] **Step 8: Commit and push**

```bash
git add crates/beckon-cli/src/lib.rs crates/beckon-cli/tests/check.rs
git commit -m "cli: check --resolve grades every name, and only a miss fails it"
git push -u origin check-resolve
```

CI is the arbiter for the Linux and Windows tests and for the `beckon-cli`
integration tests, which fail falsely on this host.

---

### Task 6: Documentation

**Files:**
- Modify: `README.md`
- Modify: `CLAUDE.md`

- [ ] **Step 1: Update `CLAUDE.md`**

Add this after the `#### Reserved names are a closed list` subsection:

```markdown
#### `check` validates shape; `check --resolve` validates meaning

`beckon check` never consults the machine — that is what makes it usable in
CI, where none of the apps are installed, and it is pinned by
`check_without_resolve_says_nothing_about_whether_the_app_exists`.

`--resolve` grades every app name against this machine's catalog using
`beckon_core::certainty::Certainty`. Every backend already computed the tier
and threw it away on one line (`resolve_inner(..).is_some()` on macOS,
`resolve_detailed_in(..).is_none()` on Linux, `apps::resolve(..).is_none()`
on Windows); the grade is that projection removed.

**Only `NoMatch` changes the exit code.** A `Guess` — the single substring
tier every backend has — resolves, so it prints and exits 0. Two of the
author's own bindings depend on that tier deliberately (`Settings` matching
*System Settings*, `DeepSeek` matching *DeepSeek - Into the Unknown*), so
failing on `Guess` would turn a correct file red, which is how a check stops
being run. The scale is why the flag exists at all: measured on `rog`,
**14 of 18 shortcuts did not resolve** while `beckon check` reported
`ok: 18 shortcuts`.

A `Guess` reports **two different hazards** and says which: one candidate
means a later install can take the name; several means the winner is already
decided by sort order over `.desktop` ids or display names, not by anything
the user wrote. Before `desktop::scan()` sorted its output, 20 runs of
`beckon resolve` split 12/8 between two entries sharing a `Name=` — the same
keypress, two answers.
```

- [ ] **Step 2: Update `README.md`**

Find the `check --resolve` entry added by the earlier commit and extend it with
the three grades and the exit rule. Read the two neighbouring entries first and
match their formatting exactly.

- [ ] **Step 3: Verify the site check still passes**

Run: `./tools/check-site.sh`
Expected: PASS. It asserts the landing page's install commands byte-match
`README.md`.

- [ ] **Step 4: Commit**

```bash
git add README.md CLAUDE.md
git commit -m "docs: only a miss fails check --resolve, and a guess says why"
```

---

## Self-Review

**Spec coverage.** §3 (`Certainty`) shipped in Task 1 of the superseded plan.
§4's "reports become values" is Tasks 2-4, now expressed as widening the landed
`unresolved_names` rather than adding a parallel function. §5 (CLI surface)
needs nothing — the flag already exists and is already declared inside `Check`.
§6 (output and exit code), as reversed on 2026-08-15, is Task 5 Step 4. §7
("CI cannot resolve") is unchanged and is already stated in the landed doc
comment this plan preserves verbatim. §10 (testing) is the test steps.

**Deliberately not in this plan**, each a decision rather than an oversight:

- **§8.1, the `~/.nix` flake pin.** Still outstanding, still worth doing, and
  now more urgent than when it was written: `origin/main` has gained both
  `check --resolve` and the Hyprland fixes the user's `rog` needs, while the
  pin is still v0.6.0. It lives in the superseded plan as Task 7 and is
  unaffected by anything here.
- **§8.2, generating the README shortcut table.** Different repository,
  different language, no shared code.
- **A `--strict` flag that would also fail on `Guess`.** Named in the spec as a
  possible future second flag; there is no request for it.

**Type consistency.** `resolve_reports(names: &[&str]) ->
beckon_core::Result<Vec<NameReport>>` is identical at all three crate roots,
which is what lets Task 5's dispatch have one body per `#[cfg]` arm. The
internal pure functions differ on purpose and are named in each task's
Interfaces block: macOS takes a lazy loader because its running tiers can skip
the scan entirely, Windows and Linux take a slice because they have no running
tier. `NameReport`, `Certainty`, `Summary` and `summarize` are exactly as
shipped in `beckon-core`; nothing in this plan changes them.

**`Summary`/`summarize` go unused by this plan, and that is deliberate.** They
were built in Task 1 for a table that printed one line per binding including the
exact ones. The landed `check --resolve` prints only the problems and states its
own counts in the error line, which is the better shape — so a tail count would
duplicate what `unresolved_report` and the `Err` already say. They stay in
`beckon-core`, tested, unused, for the `match` floor that will want them. If a
reviewer flags them as dead code, that is a fair finding and the answer is this
paragraph, not a hasty caller.

**One risk this plan does not remove.** Tasks 3 and 4 are verified by CI, not
locally: `cargo test -p beckon-linux` on this macOS host runs zero tests while
printing `ok`, the Windows tests need a Windows machine, and `beckon-cli`'s
integration tests fail falsely here. Each task says so rather than pretending
the local gate covers it.
