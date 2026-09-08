//! App enumeration + Name → bundle_id resolution.
//!
//! Two sources of truth:
//!   - **Running apps**: `NSWorkspace.runningApplications` exposes pid, bundleId,
//!     localizedName, activationPolicy. We keep only `regular` apps (those that
//!     appear in the Dock); accessory/UIElement processes are not user-facing.
//!   - **Installed apps**: scan `/Applications`, `/System/Applications`,
//!     `~/Applications` (one level deep, plus `/System/Applications/Utilities`)
//!     for `*.app` bundles and read `Contents/Info.plist`.
//!
//! Resolution priority mirrors the Linux backend's `.desktop` rules:
//!   1. Running app — `localizedName` exact match (case-insensitive).
//!   2. Running app — `bundleIdentifier` exact match.
//!   3. Installed app — display/bundle name exact match (case-insensitive).
//!   4. Installed app — `CFBundleIdentifier` exact match.
//!   5. Installed app — name substring (alphabetical-first wins, like rofi).
//!
//! **Tiers 1-2 and tiers 3-5 are two ladders, and they can disagree.** Which
//! one answers is decided by whether the app happens to be running, so the
//! same config resolves two ways on the same machine. That is correct for
//! focus-or-launch — focusing the app you can see beats launching a second
//! copy of it — but it means a name can focus one bundle and launch another.
//! `cold_path_for` computes the second answer and `check --resolve` reports
//! it; the order itself is deliberate and stays as it is.

use beckon_core::certainty::{Certainty, ColdPath, NameReport};
use objc2::rc::Retained;
use objc2::Message;
use objc2_app_kit::{NSRunningApplication, NSWorkspace};
use objc2_foundation::NSString;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct RunningAppInfo {
    pub pid: i32,
    pub bundle_id: String,
    pub name: String,
    pub running: Retained<NSRunningApplication>,
}

#[derive(Debug, Clone)]
pub struct InstalledAppInfo {
    pub bundle_id: String,
    pub name: String,
    pub bundle_path: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchType {
    RunningName,
    RunningBundleId,
    InstalledName,
    InstalledBundleId,
    InstalledNameSubstring,
}

impl MatchType {
    pub fn describe(self) -> &'static str {
        match self {
            MatchType::RunningName => "running app localizedName (exact)",
            MatchType::RunningBundleId => "running app bundleIdentifier",
            MatchType::InstalledName => "installed app name (exact)",
            MatchType::InstalledBundleId => "installed app CFBundleIdentifier",
            MatchType::InstalledNameSubstring => "installed app name substring",
        }
    }

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
}

#[derive(Debug, Clone)]
pub struct ResolvedMatch {
    /// What we resolved to. Always a bundle id (the canonical macOS app
    /// identity that NSRunningApplication.activate / launchApplication accept).
    pub bundle_id: String,
    pub display_name: String,
    /// `Some` when we found a corresponding installed bundle on disk; `None`
    /// when the match came from a running-only app (e.g. CLI-launched binary
    /// not registered with LaunchServices).
    pub bundle_path: Option<PathBuf>,
    pub match_type: MatchType,
}

/// All running, regular (Dock-visible) apps. Accessory / UIElement apps and
/// apps that haven't finished launching are excluded.
pub fn running_apps() -> Vec<RunningAppInfo> {
    let workspace = NSWorkspace::sharedWorkspace();
    let array = workspace.runningApplications();
    let mut out = Vec::with_capacity(array.len());
    for app in array.iter() {
        // 0 = NSApplicationActivationPolicyRegular; 1 = .accessory;
        // 2 = .prohibited. Only regular apps are user-facing and dock-visible.
        if app.activationPolicy().0 != 0 {
            continue;
        }
        let Some(bundle_id) = app.bundleIdentifier() else {
            continue;
        };
        let name = app
            .localizedName()
            .map(|s| s.to_string())
            .unwrap_or_default();
        out.push(RunningAppInfo {
            pid: app.processIdentifier(),
            bundle_id: bundle_id.to_string(),
            name,
            running: app.retain(),
        });
    }
    out
}

/// All installed `.app` bundles in the standard search paths.
///
/// We descend at most one level into non-.app subdirectories of each root,
/// which catches:
///   - Browser PWA folders: `~/Applications/{Brave Browser,Chrome,Vivaldi}
///     Apps.localized/*.app` — these contain the user's Chrome/Brave/Vivaldi
///     PWAs (Discord, Gmail, YouTube, ...).
///   - `/System/Applications/Utilities/*.app` — the standard utilities folder.
///
/// We do NOT recurse beyond one level — that would pick up nested helper
/// bundles like `Foo.app/Contents/Library/Bar.app` which are not
/// user-launchable.
pub fn installed_apps() -> Vec<InstalledAppInfo> {
    let mut roots: Vec<PathBuf> = vec![
        PathBuf::from("/Applications"),
        PathBuf::from("/System/Applications"),
    ];
    if let Some(home) = std::env::var_os("HOME") {
        roots.push(PathBuf::from(&home).join("Applications"));
    }

    let mut out: Vec<InstalledAppInfo> = Vec::new();
    let mut seen_bundles = std::collections::HashSet::<String>::new();
    let mut process = |path: &Path, out: &mut Vec<InstalledAppInfo>| {
        let Some(info) = read_bundle_info(path) else {
            return;
        };
        // Multiple roots can list the same bundle (e.g. /Applications
        // shadowing a /System default). Keep the first occurrence, which
        // matches our root order: /Applications → /System → ~/Applications.
        if seen_bundles.insert(info.bundle_id.clone()) {
            out.push(info);
        }
    };

    for root in &roots {
        let Ok(entries) = std::fs::read_dir(root) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let is_app = path.extension().and_then(|e| e.to_str()) == Some("app");
            if is_app {
                process(&path, &mut out);
                continue;
            }
            // Non-.app entry — descend one level if it's a directory. This
            // catches `Vivaldi Apps.localized/*.app`, `Utilities/*.app`, etc.
            let Ok(meta) = entry.metadata() else { continue };
            if !meta.is_dir() {
                continue;
            }
            let Ok(sub_entries) = std::fs::read_dir(&path) else {
                continue;
            };
            for sub in sub_entries.flatten() {
                let sub_path = sub.path();
                if sub_path.extension().and_then(|e| e.to_str()) == Some("app") {
                    process(&sub_path, &mut out);
                }
            }
        }
    }
    out
}

fn read_bundle_info(app_path: &Path) -> Option<InstalledAppInfo> {
    let plist_path = app_path.join("Contents").join("Info.plist");
    let value = plist::Value::from_file(&plist_path).ok()?;
    let dict = value.as_dictionary()?;

    let bundle_id = dict.get("CFBundleIdentifier")?.as_string()?.to_string();

    // Prefer CFBundleDisplayName (what the Finder shows), fall back to
    // CFBundleName, then to the bundle's own filename without `.app`.
    let name = dict
        .get("CFBundleDisplayName")
        .and_then(|v| v.as_string())
        .map(|s| s.to_string())
        .or_else(|| {
            dict.get("CFBundleName")
                .and_then(|v| v.as_string())
                .map(|s| s.to_string())
        })
        .unwrap_or_else(|| {
            app_path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("?")
                .to_string()
        });

    Some(InstalledAppInfo {
        bundle_id,
        name,
        bundle_path: app_path.to_path_buf(),
    })
}

/// Lowercase, drop bidi/format marks, collapse whitespace.
/// Mirrors `beckon_linux::desktop::normalize` so the same Names resolve
/// consistently across OSes (Brave PWAs sometimes prefix with U+200E).
pub fn normalize(s: &str) -> String {
    s.chars()
        .filter(|c| !is_format_mark(*c))
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

fn is_format_mark(c: char) -> bool {
    matches!(
        c,
        '\u{200E}' | '\u{200F}'
            | '\u{202A}'..='\u{202E}'
            | '\u{2066}'..='\u{2069}'
            | '\u{FEFF}'
    )
}

/// Resolve a user-supplied id. See module docs for priority order.
///
/// The discovery entry point (`beckon resolve`, `beckon search`): it pays
/// `bundle_path_for` on a running match so the report can print a path. The
/// hot path takes `resolve_with_running`, which does not — see there.
pub fn resolve(id: &str) -> Option<ResolvedMatch> {
    let running = running_apps();
    let refs: Vec<RunningRef> = running.iter().map(RunningRef::from).collect();
    resolve_inner(id, &refs, installed_apps, bundle_path_for)
}

/// Subset of `RunningAppInfo` that the resolver actually consults — no
/// `Retained<NSRunningApplication>` so tests can build it on any host.
#[derive(Debug, Clone)]
pub(crate) struct RunningRef<'a> {
    pub bundle_id: &'a str,
    pub name: &'a str,
}

impl<'a> From<&'a RunningAppInfo> for RunningRef<'a> {
    fn from(a: &'a RunningAppInfo) -> Self {
        RunningRef {
            bundle_id: &a.bundle_id,
            name: &a.name,
        }
    }
}

/// Resolve, reusing a `running_apps()` snapshot the caller already has — the
/// hot path (`beckon <id>` → `MacBackend::beckon_inner`). `installed_apps()` is
/// queried lazily, and the bundle path is not queried at all.
///
/// `|_| None` for the path, the same call `resolve_reports_in` makes and for
/// the same reason: the only reader of `ResolvedMatch::bundle_path` is
/// `print_resolve_report`, and every hot-path consumer — `launch_bundle`,
/// `focus_running` — acts on the bundle id. The lookup is a LaunchServices
/// round trip and it is **cold** on this path, because a keypress is a whole
/// process: measured on airm3 2026-08-16, 13 fresh processes each taking one
/// sample, the first `resolve_with_running` on a running-tier match cost
/// **6.3–8.8 ms (median 6.6)** with the lookup and **0.011–0.035 ms
/// (median 0.014)** without it. A warm loop reports ~0.013 ms either way and
/// is measuring the LaunchServices cache, not this path.
pub fn resolve_with_running(id: &str, running: &[RunningAppInfo]) -> Option<ResolvedMatch> {
    let refs: Vec<RunningRef> = running.iter().map(RunningRef::from).collect();
    resolve_inner(id, &refs, installed_apps, |_| None)
}

/// What a keypress costs when a name matched only by substring and exactly one
/// app answered it.
const GUESS_LONE: &str = "substring match, so an app installed later can quietly take this name";

/// What a miss means on macOS.
const MISS_CONSEQUENCE: &str = "no match; this key will error and launch nothing";

/// Bundle ids, other than the winner's, of installed apps whose name IS this
/// id. Empty for every tier that matched a bundle id, which is unique.
///
/// This is the state-independent half of the two-answers problem. `cold`
/// only fires while the app is running, so on its own it would report the
/// hazard exactly when the user is least likely to be bitten by it and stay
/// silent on the day the key launches the wrong thing. The catalog does not
/// care what is running, and neither does this.
fn same_name_rivals(id: &str, winner: &str, installed: &[InstalledAppInfo]) -> Vec<String> {
    let needle = normalize(id);
    let mut out: Vec<String> = installed
        .iter()
        .filter(|a| normalize(&a.name) == needle && a.bundle_id != winner)
        .map(|a| a.bundle_id.clone())
        .collect();
    out.sort();
    out.dedup();
    out
}

/// The report for one already-resolved name.
///
/// `installed` is passed so a guess can name its rivals — and, since the
/// exact-name tiers can have rivals too, so can an exact match. The tiers
/// that match a bundle id cannot: an id identifies one bundle.
fn report_for(id: &str, m: &ResolvedMatch, installed: &[InstalledAppInfo]) -> NameReport {
    let certainty = m.match_type.certainty();
    let rivals = match m.match_type {
        MatchType::RunningName | MatchType::InstalledName => {
            same_name_rivals(id, &m.bundle_id, installed)
        }
        MatchType::RunningBundleId
        | MatchType::InstalledBundleId
        | MatchType::InstalledNameSubstring => Vec::new(),
    };
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
    } else if !rivals.is_empty() {
        // Exact, and still not one app. Which of them the key opens is
        // decided by whether one is running — tiers 1-2 — and otherwise by
        // bundle-id order in tier 3. Neither is anything the config says.
        (
            format!(
                "{} installed apps answer to this Name; which one this key opens \
                 depends on which of them is running",
                rivals.len() + 1
            ),
            Vec::new(),
        )
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
        rivals,
        // Filled by `resolve_reports_in` for the running tiers only. A match
        // that already came from the installed catalog IS the cold answer and
        // has nothing to compare itself against.
        cold: None,
    }
}

/// What the same name resolves to once the running tiers are taken away, for
/// a report that came from one of them.
///
/// **This is the launch half of focus-or-launch.** Tiers 1-2 answer from
/// `NSWorkspace.runningApplications` and tiers 3-5 from the installed
/// catalog, and nothing makes the two agree. Two bundles can carry one
/// `localizedName` — an installer stub beside the app it installed, a dev
/// build beside the release, an Electron app built from source — and a bundle
/// can be installed where `installed_apps()` does not walk at all.
///
/// The consequence is that resolution depends on process state while the
/// config does not: validate while the app is up and the answer is one
/// bundle, press the key on the day it is down and beckon launches the other.
fn cold_path_for(
    id: &str,
    hot: &ResolvedMatch,
    installed: &[InstalledAppInfo],
) -> Option<ColdPath> {
    match resolve_installed_in(id, installed) {
        None => Some(ColdPath::Nowhere),
        Some(cold) if cold.bundle_id != hot.bundle_id => Some(ColdPath::Elsewhere {
            target: cold.bundle_id,
            tier: cold.match_type.describe(),
        }),
        Some(_) => None,
    }
}

/// What a divergent name does, spelled for the row that reports it.
fn cold_consequence(cold: &ColdPath, hot: &ResolvedMatch) -> String {
    match cold {
        ColdPath::Elsewhere { target, .. } => format!(
            "while it runs this focuses {}; when it does not, the same name launches {}",
            hot.bundle_id, target
        ),
        ColdPath::Nowhere => format!(
            "only the running {} claims this name; once it quits, this key will error \
             and launch nothing",
            hot.bundle_id
        ),
    }
}

/// One `NameReport` per name, in the order given, against caller-supplied
/// snapshots. `installed_loader` runs at most once for the whole batch, and
/// not at all for an empty one.
///
/// It used to be skipped entirely when every name matched a running app. The
/// cold pass below retired that: a running match is precisely the case that
/// now needs the catalog, because it is the only case with a second answer to
/// look up. The laziness would survive only in a batch where no name matched
/// running — and that batch loads the catalog for the installed tiers anyway,
/// one line down. `resolve_inner`, the hot path, is untouched and stays lazy;
/// `installed_loader_not_invoked_when_running_matches` still pins it.
pub(crate) fn resolve_reports_in(
    names: &[&str],
    running: &[RunningRef<'_>],
    installed_loader: impl FnOnce() -> Vec<InstalledAppInfo>,
) -> Vec<NameReport> {
    if names.is_empty() {
        return Vec::new();
    }
    let installed = installed_loader();
    let mut out = Vec::with_capacity(names.len());

    for id in names {
        // `|_| None` for the bundle path: the report names the bundle id and
        // never the path, and `bundle_path_for` is an NSWorkspace round trip
        // on every running match.
        if let Some(m) = resolve_running_in(id, running, |_| None) {
            let mut r = report_for(id, &m, &installed);
            if let Some(cold) = cold_path_for(id, &m, &installed) {
                // Both running tiers are `Exact`, so `report_for` left the
                // sentence empty and this is the whole of it. Joined rather
                // than assigned all the same: a running tier that ever grades
                // below exact would otherwise lose its own sentence here, and
                // silently, which is the failure this whole feature is about.
                let sentence = cold_consequence(&cold, &m);
                r.consequence = if r.consequence.is_empty() {
                    sentence
                } else {
                    format!("{}; {}", r.consequence, sentence)
                };
                r.cold = Some(cold);
            }
            out.push(r);
            continue;
        }
        match resolve_installed_in(id, &installed) {
            Some(m) => out.push(report_for(id, &m, &installed)),
            None => out.push(NameReport {
                id: (*id).to_string(),
                certainty: Certainty::NoMatch,
                target: None,
                tier: None,
                consequence: MISS_CONSEQUENCE.to_string(),
                suggestions: Vec::new(),
                rivals: Vec::new(),
                cold: None,
            }),
        }
    }
    out
}

/// The cold half of the answer for one already-resolved id, against this
/// machine. `beckon resolve`'s entry point; `check --resolve` goes through
/// `resolve_reports_in`, which already holds a catalog and must not scan a
/// second time.
pub(crate) fn cold_path(id: &str, hot: &ResolvedMatch) -> Option<ColdPath> {
    cold_path_for(id, hot, &installed_apps())
}

/// One `NameReport` per name, against this machine.
pub fn resolve_reports(names: &[&str]) -> Vec<NameReport> {
    let running = running_apps();
    let refs: Vec<RunningRef<'_>> = running.iter().map(RunningRef::from).collect();
    resolve_reports_in(names, &refs, installed_apps)
}

/// The two running-app tiers, split out so a batch caller can run them
/// without holding an installed-app scan.
pub(crate) fn resolve_running_in(
    id: &str,
    running: &[RunningRef<'_>],
    bundle_path_for: impl Fn(&str) -> Option<PathBuf>,
) -> Option<ResolvedMatch> {
    let needle = normalize(id);
    if needle.is_empty() {
        return None;
    }

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
    // An empty needle is a substring of every name, so without this guard the
    // substring tier resolves an id that is all format marks — a lone U+200E
    // survives every `trim()` at the CLI and config boundaries — to whichever
    // installed app sorts first, and launches it.
    if needle.is_empty() {
        return None;
    }

    // `min_by_key`, not `find`. Two bundles can carry one display name, and
    // `installed_apps()` yields them in `read_dir` order within each root —
    // an order nothing specifies, that changes when a neighbouring bundle is
    // added or removed, and that decides which app a keypress launches. The
    // substring tier below already sorts by bundle id for exactly this
    // reason; this tier was the one that did not. Linux hit the same shape
    // with `HashMap` iteration order and fixed it by sorting `scan()`.
    //
    // Which bundle wins is arbitrary either way — that is what `rivals`
    // reports. What this buys is that it is the SAME one every time.
    if let Some(app) = installed
        .iter()
        .filter(|a| normalize(&a.name) == needle)
        .min_by(|a, b| a.bundle_id.cmp(&b.bundle_id))
    {
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

/// Substring matches across installed apps, sorted by bundle id. Used by
/// `resolve` to flag ambiguity ("4 other entries also match by name substring").
pub fn name_substring_matches(id: &str) -> Vec<InstalledAppInfo> {
    let needle = normalize(id);
    if needle.is_empty() {
        return Vec::new();
    }
    let mut matches: Vec<InstalledAppInfo> = installed_apps()
        .into_iter()
        .filter(|a| normalize(&a.name).contains(&needle))
        .collect();
    matches.sort_by(|a, b| a.bundle_id.cmp(&b.bundle_id));
    matches
}

fn bundle_path_for(bundle_id: &str) -> Option<PathBuf> {
    let workspace = NSWorkspace::sharedWorkspace();
    let ns_id = NSString::from_str(bundle_id);
    let url = workspace.URLForApplicationWithBundleIdentifier(&ns_id)?;
    let path = url.path()?;
    Some(PathBuf::from(path.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rref<'a>(bundle_id: &'a str, name: &'a str) -> RunningRef<'a> {
        RunningRef { bundle_id, name }
    }

    fn installed(bundle_id: &str, name: &str) -> InstalledAppInfo {
        InstalledAppInfo {
            bundle_id: bundle_id.to_string(),
            name: name.to_string(),
            bundle_path: PathBuf::from(format!("/Applications/{name}.app")),
        }
    }

    fn resolve_test(
        id: &str,
        running: &[RunningRef],
        installed: Vec<InstalledAppInfo>,
    ) -> Option<ResolvedMatch> {
        resolve_inner(id, running, move || installed, |_| None)
    }

    fn reports_test(
        names: &[&str],
        running: &[RunningRef],
        installed: Vec<InstalledAppInfo>,
    ) -> Vec<beckon_core::certainty::NameReport> {
        resolve_reports_in(names, running, move || installed)
    }

    // ---------- normalize ----------

    #[test]
    fn normalize_lowercases_and_collapses_whitespace() {
        assert_eq!(normalize("Brave Browser"), "brave browser");
        assert_eq!(normalize("  Visual   Studio   Code "), "visual studio code");
    }

    #[test]
    fn normalize_strips_format_marks() {
        assert_eq!(normalize("\u{200E}Claude"), "claude");
        assert_eq!(normalize("\u{FEFF}Foo \u{2069}Bar"), "foo bar");
    }

    // ---------- priority: running over installed ----------

    #[test]
    fn running_name_beats_installed_name() {
        // Both match "Claude" by name, but running wins.
        let running = vec![rref("com.anthropic.claude", "Claude")];
        let installed = vec![installed("com.anthropic.claude", "Claude")];
        let m = resolve_test("Claude", &running, installed).unwrap();
        assert_eq!(m.match_type, MatchType::RunningName);
        assert_eq!(m.bundle_id, "com.anthropic.claude");
    }

    #[test]
    fn running_name_is_case_insensitive() {
        let running = vec![rref("com.x.kitty", "Kitty")];
        let m = resolve_test("KITTY", &running, vec![]).unwrap();
        assert_eq!(m.match_type, MatchType::RunningName);
    }

    #[test]
    fn running_bundle_id_is_exact_only() {
        // Bundle id matching is case-sensitive and exact, no normalization.
        let running = vec![rref("com.example.foo", "Foo")];
        // Wrong case -> doesn't match RunningBundleId; falls through.
        assert!(resolve_test("COM.EXAMPLE.FOO", &running, vec![]).is_none());
        // Exact -> matches.
        let m = resolve_test("com.example.foo", &running, vec![]).unwrap();
        assert_eq!(m.match_type, MatchType::RunningBundleId);
    }

    #[test]
    fn running_name_beats_running_bundle_id() {
        // If a Name match exists, prefer it over a bundle-id match on a
        // different running app.
        let running = vec![
            rref("com.example.bar", "Foo"),
            rref("com.example.foo", "Bar"),
        ];
        let m = resolve_test("Foo", &running, vec![]).unwrap();
        assert_eq!(m.match_type, MatchType::RunningName);
        assert_eq!(m.bundle_id, "com.example.bar");
    }

    // ---------- installed fallback ----------

    #[test]
    fn falls_through_to_installed_name_when_not_running() {
        let installed = vec![installed("com.anthropic.claude", "Claude")];
        let m = resolve_test("Claude", &[], installed).unwrap();
        assert_eq!(m.match_type, MatchType::InstalledName);
        assert_eq!(m.bundle_id, "com.anthropic.claude");
        // bundle_path comes from InstalledAppInfo, not bundle_path_for.
        assert!(m.bundle_path.is_some());
    }

    #[test]
    fn falls_through_to_installed_bundle_id() {
        let installed = vec![installed("com.example.foo", "Foo App")];
        // "Foo App" exact would hit InstalledName; the literal bundle id
        // should hit InstalledBundleId.
        let m = resolve_test("com.example.foo", &[], installed).unwrap();
        assert_eq!(m.match_type, MatchType::InstalledBundleId);
    }

    #[test]
    fn falls_through_to_installed_substring_alphabetical_first() {
        let installed = vec![
            installed("com.zeta.browser", "Zeta Browser"),
            installed("com.alpha.browser", "Alpha Browser"),
        ];
        let m = resolve_test("Browser", &[], installed).unwrap();
        assert_eq!(m.match_type, MatchType::InstalledNameSubstring);
        // Alphabetical first by bundle_id wins.
        assert_eq!(m.bundle_id, "com.alpha.browser");
    }

    // ---------- misses ----------

    #[test]
    fn miss_returns_none() {
        let installed = vec![installed("com.example.foo", "Foo")];
        assert!(resolve_test("nonexistent", &[], installed).is_none());
    }

    #[test]
    fn empty_inputs_return_none() {
        assert!(resolve_test("anything", &[], vec![]).is_none());
    }

    /// An id that normalizes to nothing must not resolve. It reaches here:
    /// U+200E is a format mark, not whitespace, so it survives every
    /// `trim()` at the CLI and config boundaries, and an empty needle is a
    /// substring of every name — so the substring tier used to launch
    /// whichever installed app sorted first.
    #[test]
    fn an_id_that_normalizes_to_nothing_resolves_to_nothing() {
        let running = vec![rref("com.example.alpha", "Alpha")];
        let inst = vec![
            installed("com.example.alpha", "Alpha"),
            installed("com.example.beta", "Beta"),
        ];
        assert!(resolve_test("\u{200E}", &running, inst.clone()).is_none());
        assert!(resolve_test("", &running, inst).is_none());
    }

    // ---------- bidi-prefixed Name (PWA case) ----------

    #[test]
    fn bidi_prefixed_running_name_matches_ascii_query() {
        // Brave PWAs sometimes prefix Name with U+200E. The user types
        // ASCII; normalize should strip the mark on both sides.
        let running = vec![rref(
            "brave-fmpnliohjhemenmnlpbfagaolkdacoja-Default",
            "\u{200E}Claude",
        )];
        let m = resolve_test("Claude", &running, vec![]).unwrap();
        assert_eq!(m.match_type, MatchType::RunningName);
    }

    // ---------- installed_loader laziness ----------

    #[test]
    fn installed_loader_not_invoked_when_running_matches() {
        // If running matches, the closure should never be called — that's
        // the whole point of lazy installed scan.
        use std::cell::Cell;
        let called = Cell::new(false);
        let running = vec![rref("com.x.kitty", "Kitty")];
        let _ = resolve_inner(
            "Kitty",
            &running,
            || {
                called.set(true);
                Vec::new()
            },
            |_| None,
        );
        assert!(
            !called.get(),
            "installed_loader was invoked despite running match"
        );
    }

    #[test]
    fn installed_loader_is_invoked_on_running_miss() {
        use std::cell::Cell;
        let called = Cell::new(false);
        let _ = resolve_inner(
            "anything",
            &[],
            || {
                called.set(true);
                Vec::new()
            },
            |_| None,
        );
        assert!(
            called.get(),
            "installed_loader should run when running miss"
        );
    }

    // ---------- the hot path pays for no bundle path ----------

    /// `beckon <id>` never reads `ResolvedMatch::bundle_path` — `launch_bundle`
    /// and `focus_running` both act on the bundle id — so the hot path must not
    /// pay `bundle_path_for` to fill it. That lookup is a LaunchServices round
    /// trip and it is COLD on this path, because a keypress is a whole process:
    /// measured on airm3 2026-08-16, 13 fresh processes each taking one sample,
    /// 6.3–8.8 ms (median 6.6) with it against 0.011–0.035 ms (median 0.014)
    /// without, on a ~95–105 ms path.
    ///
    /// `com.apple.finder` is the bundle id because it resolves on **every**
    /// macOS: put `bundle_path_for` back and this comes back
    /// `Some("/System/Library/CoreServices/Finder.app")` and the assertion
    /// fails. A made-up id would answer `None` either way and pin nothing.
    /// `NSRunningApplication::currentApplication` stands in for the snapshot
    /// entry — the resolver reads only `bundle_id` and `name`.
    #[test]
    fn the_hot_path_resolves_without_a_bundle_path_lookup() {
        let running = vec![RunningAppInfo {
            pid: std::process::id() as i32,
            bundle_id: "com.apple.finder".to_string(),
            name: "Finder".to_string(),
            running: NSRunningApplication::currentApplication(),
        }];
        let m = resolve_with_running("Finder", &running).unwrap();
        assert_eq!(m.match_type, MatchType::RunningName);
        assert_eq!(m.bundle_id, "com.apple.finder");
        assert_eq!(
            m.bundle_path, None,
            "the hot path asked LaunchServices for a path nothing downstream reads"
        );
    }

    /// The installed tiers take their path from the catalog entry, not from the
    /// closure, so dropping the lookup on the hot path cannot blank a path the
    /// launcher might have wanted. This is what makes the change safe rather
    /// than merely cheap.
    #[test]
    fn dropping_the_lookup_does_not_blank_the_installed_tiers_path() {
        let inst = vec![installed("com.anthropic.claude", "Claude")];
        let m = resolve_inner("Claude", &[], move || inst, |_| None).unwrap();
        assert_eq!(m.match_type, MatchType::InstalledName);
        assert_eq!(
            m.bundle_path,
            Some(PathBuf::from("/Applications/Claude.app"))
        );
    }

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
        let reports = reports_test(
            &["Claude"],
            &[],
            vec![installed("com.anthropic.claude", "Claude")],
        );
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
    ///
    /// It is also the `Nowhere` case, and the two halves belong in one test:
    /// the grade is right *and* incomplete. `beckon installed | grep -ci
    /// finder` is 0 on this machine, measured 2026-09-08.
    #[test]
    fn a_running_only_app_is_exact_not_a_guess() {
        use beckon_core::certainty::{Certainty, ColdPath};
        let running = vec![rref("com.apple.finder", "Finder")];
        let reports = reports_test(&["Finder"], &running, Vec::new());
        assert_eq!(reports[0].certainty, Certainty::Exact);
        assert_eq!(reports[0].tier, Some("running app localizedName (exact)"));
        assert_eq!(reports[0].cold, Some(ColdPath::Nowhere));
        assert!(
            reports[0].consequence.contains("quits"),
            "consequence was {:?}",
            reports[0].consequence
        );
    }

    // ---------- the cold ladder ----------

    /// Two bundles, one `localizedName`. Measured on macmini 2026-09-08:
    /// `com.nousresearch.hermes.setup` is a 12 MB installer stub in
    /// /Applications and `com.nousresearch.hermes` is the app it installed.
    /// While the app runs, tier 1 answers with the app; once it quits, tier 3
    /// answers with the installer — from the identical config string.
    ///
    /// The catalog here holds only the installer, which is the situation as
    /// first reported: the real app lives under ~/.hermes, outside every scan
    /// root, and became visible to `installed` only once it was symlinked
    /// into ~/Applications. Running or not, the name answers twice.
    #[test]
    fn a_running_match_whose_cold_ladder_lands_elsewhere_is_reported() {
        use beckon_core::certainty::{Certainty, ColdPath};
        let running = vec![rref("com.nousresearch.hermes", "Hermes")];
        let reports = reports_test(
            &["Hermes"],
            &running,
            vec![installed("com.nousresearch.hermes.setup", "Hermes")],
        );
        let r = &reports[0];
        assert_eq!(r.certainty, Certainty::Exact);
        assert_eq!(r.target.as_deref(), Some("com.nousresearch.hermes"));
        assert_eq!(
            r.cold,
            Some(ColdPath::Elsewhere {
                target: "com.nousresearch.hermes.setup".to_string(),
                tier: "installed app name (exact)",
            })
        );
        // Both ids, because either one alone leaves the reader guessing which
        // half of the sentence they are looking at.
        assert!(
            r.consequence.contains("com.nousresearch.hermes.setup")
                && r.consequence.contains("com.nousresearch.hermes;"),
            "consequence was {:?}",
            r.consequence
        );
    }

    /// The ordinary case, and the one that must stay silent: an app that is
    /// running and is also the only thing installed under that name. A
    /// warning here would fire on nearly every binding and stop being read.
    #[test]
    fn the_two_ladders_agreeing_reports_nothing() {
        let running = vec![rref("com.anthropic.claude", "Claude")];
        let reports = reports_test(
            &["Claude"],
            &running,
            vec![installed("com.anthropic.claude", "Claude")],
        );
        assert_eq!(reports[0].cold, None);
        assert!(reports[0].consequence.is_empty());
    }

    /// A report that came from the installed tiers IS the cold answer, so
    /// there is nothing for it to differ from — `cold` is correctly `None`.
    ///
    /// **And that is exactly why `cold` alone is not enough.** With nothing
    /// running this is the state a user validates in most often, and it is
    /// the state in which the key launches something; if the report went
    /// quiet here, the warning would appear only while the app was up and
    /// vanish on the day it matters. `rivals` is what does not go quiet.
    #[test]
    fn nothing_running_reports_the_rivals_even_though_cold_is_silent() {
        let reports = reports_test(
            &["Hermes"],
            &[],
            vec![
                installed("com.nousresearch.hermes.setup", "Hermes"),
                installed("com.nousresearch.hermes", "Hermes"),
            ],
        );
        let r = &reports[0];
        assert_eq!(r.cold, None);
        assert_eq!(r.rivals, vec!["com.nousresearch.hermes.setup".to_string()]);
        assert!(r.consequence.contains('2'), "{:?}", r.consequence);
    }

    /// Which of two same-named bundles wins tier 3 must not depend on the
    /// order the catalog happens to list them in — `installed_apps()` yields
    /// `read_dir` order within each root, which nothing specifies and which
    /// changes when a neighbouring bundle is added or removed.
    ///
    /// The assertion is that the two orders agree, not which one wins: the
    /// winner is arbitrary and `rivals` is what reports that. Before
    /// `min_by`, this test failed on the second call.
    #[test]
    fn the_exact_name_tier_does_not_depend_on_catalog_order() {
        let a = installed("com.nousresearch.hermes.setup", "Hermes");
        let b = installed("com.nousresearch.hermes", "Hermes");
        let one = resolve_installed_in("Hermes", &[a.clone(), b.clone()]).unwrap();
        let two = resolve_installed_in("Hermes", &[b, a]).unwrap();
        assert_eq!(one.bundle_id, two.bundle_id);
        assert_eq!(one.match_type, MatchType::InstalledName);
    }

    /// A bundle id names one bundle, so the tiers that match one have no
    /// rivals to report by construction. Without this the tier check in
    /// `report_for` could be dropped and every id-matched report would start
    /// listing every same-named app as a rival of something it did not match
    /// by name at all.
    #[test]
    fn a_bundle_id_match_reports_no_rivals() {
        let reports = reports_test(
            &["com.nousresearch.hermes"],
            &[],
            vec![
                installed("com.nousresearch.hermes.setup", "Hermes"),
                installed("com.nousresearch.hermes", "Hermes"),
            ],
        );
        assert_eq!(reports[0].tier, Some("installed app CFBundleIdentifier"));
        assert!(reports[0].rivals.is_empty(), "{:?}", reports[0].rivals);
    }

    /// `check --resolve` sorts bindings into four baskets and prints each one
    /// once. Nothing in the types stops a report being both a `Guess` and
    /// divergent; what stops it is that only the running tiers set `cold` and
    /// both of them grade `Exact`. This is where that holds.
    #[test]
    fn a_cold_divergence_is_always_on_an_exact_report() {
        use beckon_core::certainty::Certainty;
        let running = vec![
            rref("com.nousresearch.hermes", "Hermes"),
            rref("com.apple.finder", "Finder"),
        ];
        let reports = reports_test(
            &["Hermes", "Finder"],
            &running,
            vec![installed("com.nousresearch.hermes.setup", "Hermes")],
        );
        for r in &reports {
            assert!(r.cold.is_some(), "{r:?}");
            assert_eq!(r.certainty, Certainty::Exact, "{r:?}");
        }
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
        let reports = reports_test(
            &["zalo"],
            &[],
            vec![installed("com.apple.finder", "Finder")],
        );
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

    /// The pin that REPLACED "the loader is not invoked when every name
    /// matches a running app". That laziness was retired on purpose: a
    /// running match is precisely the case with a second answer to look up,
    /// so it is the case that needs the catalog most. `resolve_inner` — the
    /// hot path, a different function — is still lazy, and
    /// `installed_loader_not_invoked_when_running_matches` still pins it.
    #[test]
    fn the_catalog_is_loaded_even_when_every_name_matched_a_running_app() {
        use std::cell::Cell;
        let calls = Cell::new(0usize);
        let running = vec![rref("com.x.kitty", "Kitty")];
        let reports = resolve_reports_in(&["Kitty"], &running, || {
            calls.set(calls.get() + 1);
            Vec::new()
        });
        assert_eq!(reports.len(), 1);
        assert_eq!(calls.get(), 1);
    }

    /// An empty shortcuts file parses fine and reaches here. The catalog scan
    /// reads three roots and one Info.plist per bundle; there is nothing to
    /// spend it on.
    #[test]
    fn an_empty_batch_does_not_scan_the_catalog() {
        use std::cell::Cell;
        let called = Cell::new(false);
        let reports = resolve_reports_in(&[], &[], || {
            called.set(true);
            Vec::new()
        });
        assert!(reports.is_empty());
        assert!(!called.get(), "an empty batch scanned the catalog");
    }
}
