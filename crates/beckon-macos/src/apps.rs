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
/// We do **not** recurse arbitrarily — only one level inside the search root,
/// plus `/System/Applications/Utilities`. Going deeper would pick up nested
/// helper bundles like `Foo.app/Contents/Library/Bar.app` which are not
/// user-launchable. Matches what `mdfind kMDItemContentType==com.apple.application-bundle`
/// would return at the top level, without depending on Spotlight indexing.
pub fn installed_apps() -> Vec<InstalledAppInfo> {
    let mut roots: Vec<PathBuf> = vec![
        PathBuf::from("/Applications"),
        PathBuf::from("/System/Applications"),
        PathBuf::from("/System/Applications/Utilities"),
    ];
    if let Some(home) = std::env::var_os("HOME") {
        roots.push(PathBuf::from(&home).join("Applications"));
    }

    let mut out: Vec<InstalledAppInfo> = Vec::new();
    let mut seen_bundles = std::collections::HashSet::<String>::new();

    for root in &roots {
        let Ok(entries) = std::fs::read_dir(root) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("app") {
                continue;
            }
            let Some(info) = read_bundle_info(&path) else {
                continue;
            };
            // Multiple roots can list the same bundle (e.g. /Applications
            // shadowing a /System default). Keep the first occurrence,
            // which matches our root order: user → system.
            if seen_bundles.insert(info.bundle_id.clone()) {
                out.push(info);
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
    let needle = normalize(id);
    let running = running_apps();

    if let Some(app) = running.iter().find(|a| normalize(&a.name) == needle) {
        return Some(ResolvedMatch {
            bundle_id: app.bundle_id.clone(),
            display_name: app.name.clone(),
            bundle_path: bundle_path_for(&app.bundle_id),
            match_type: MatchType::RunningName,
        });
    }
    if let Some(app) = running.iter().find(|a| a.bundle_id == id) {
        return Some(ResolvedMatch {
            bundle_id: app.bundle_id.clone(),
            display_name: app.name.clone(),
            bundle_path: bundle_path_for(&app.bundle_id),
            match_type: MatchType::RunningBundleId,
        });
    }

    let installed = installed_apps();
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

/// All running processes for a bundle id. Different PIDs but same logical app.
pub fn all_running_for_bundle(bundle_id: &str) -> Vec<RunningAppInfo> {
    running_apps()
        .into_iter()
        .filter(|a| a.bundle_id == bundle_id)
        .collect()
}
