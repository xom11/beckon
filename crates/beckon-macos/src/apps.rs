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
pub fn resolve(id: &str) -> Option<ResolvedMatch> {
    resolve_with_running(id, &running_apps())
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

/// Resolve, reusing a `running_apps()` snapshot the caller already has.
/// Used by `beckon()` to avoid querying NSWorkspace twice in the hot path.
/// Calls `bundle_path_for` for running matches (NSWorkspace lookup) so the
/// `-r` debug output can show a path; `installed_apps()` is queried lazily.
pub fn resolve_with_running(id: &str, running: &[RunningAppInfo]) -> Option<ResolvedMatch> {
    let refs: Vec<RunningRef> = running.iter().map(RunningRef::from).collect();
    resolve_inner(id, &refs, installed_apps, bundle_path_for)
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

    let installed = installed_loader();
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

/// Substring matches across installed apps, sorted by bundle id. Used by
/// `-r` to flag ambiguity ("4 other entries also match by name substring").
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
            bundle_path: PathBuf::from(format!("/Applications/{}.app", name)),
        }
    }

    fn resolve_test(
        id: &str,
        running: &[RunningRef],
        installed: Vec<InstalledAppInfo>,
    ) -> Option<ResolvedMatch> {
        resolve_inner(id, running, move || installed, |_| None)
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
}
