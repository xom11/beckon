//! Windows installed-app scanning and Name -> launch-target resolution.
//!
//! Scan paths:
//!   - `%APPDATA%\Microsoft\Windows\Start Menu\Programs\` (per-user)
//!   - `%ProgramData%\Microsoft\Windows\Start Menu\Programs\` (system-wide)
//!   - Shell application registrations exposed for the current user
//!
//! Resolution priority (mirrors Linux .desktop / macOS LaunchServices):
//!   1. Installed name exact match (case-insensitive, normalised).
//!   2. AppUserModelID exact match for packaged/system shell apps.
//!   3. Exe filename stem/name match (e.g. `brave` matches `brave.exe`).
//!   4. Installed name substring (alphabetical-first wins).

use beckon_core::certainty::{Certainty, NameReport};
use std::path::{Path, PathBuf};
use windows::core::{Interface, GUID, PCWSTR, PWSTR};
use windows::Win32::Foundation::PROPERTYKEY;
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoTaskMemFree, IBindCtx, IPersistFile, CLSCTX_INPROC_SERVER,
    COINIT_APARTMENTTHREADED, STGM,
};
use windows::Win32::UI::Shell::{
    BHID_EnumItems, FOLDERID_AppsFolder, IEnumShellItems, IShellItem, IShellItem2, IShellLinkW,
    SHGetKnownFolderItem, KNOWN_FOLDER_FLAG, SIGDN_NORMALDISPLAY,
};

/// CLSID for ShellLink COM class: {00021401-0000-0000-C000-000000000046}
const CLSID_SHELL_LINK: GUID = GUID::from_u128(0x00021401_0000_0000_c000_000000000046);
const PKEY_APP_USER_MODEL_ID: PROPERTYKEY = PROPERTYKEY {
    fmtid: GUID::from_u128(0x9f4c2855_9f79_4b39_a8d0_e1d42de1d5f3),
    pid: 5,
};

#[derive(Debug, Clone)]
pub struct InstalledAppInfo {
    /// Display name from shortcut filename (sans `.lnk`).
    pub name: String,
    /// Target exe path resolved from the shortcut.
    pub exe_path: String,
    /// Exe filename, lowercased (e.g. `brave.exe`).
    pub exe_name: String,
    /// Arguments from the shortcut.
    pub arguments: String,
    /// Path to the `.lnk` file itself (used for launching).
    pub shortcut_path: PathBuf,
    /// AppUserModelID for packaged/system shell apps; empty for classic shortcuts.
    pub aumid: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchType {
    InstalledName,
    InstalledAumid,
    InstalledExeStem,
    InstalledNameSubstring,
}

impl MatchType {
    pub fn describe(self) -> &'static str {
        match self {
            MatchType::InstalledName => "Start Menu/app display name (exact)",
            MatchType::InstalledAumid => "AppUserModelID",
            MatchType::InstalledExeStem => "exe filename stem",
            MatchType::InstalledNameSubstring => "Start Menu/app display name (substring)",
        }
    }

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
}

#[derive(Debug, Clone)]
pub struct ResolvedMatch {
    pub name: String,
    pub exe_path: String,
    pub exe_name: String,
    pub arguments: String,
    pub shortcut_path: PathBuf,
    pub aumid: Option<String>,
    pub match_type: MatchType,
}

/// Lowercase, drop bidi/format marks, collapse whitespace.
/// Mirrors `beckon_linux::desktop::normalize` and `beckon_macos::apps::normalize`.
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

/// Start Menu roots, per-user first so its shortcuts win the name dedupe.
fn start_menu_roots() -> Vec<PathBuf> {
    let mut roots: Vec<PathBuf> = Vec::new();

    // Per-user shortcuts.
    if let Ok(appdata) = std::env::var("APPDATA") {
        roots.push(
            PathBuf::from(&appdata)
                .join("Microsoft")
                .join("Windows")
                .join("Start Menu")
                .join("Programs"),
        );
    }

    // System-wide shortcuts.
    if let Ok(progdata) = std::env::var("ProgramData") {
        roots.push(
            PathBuf::from(&progdata)
                .join("Microsoft")
                .join("Windows")
                .join("Start Menu")
                .join("Programs"),
        );
    }

    roots
}

/// Every `.lnk` path under the Start Menu roots, in traversal order. Pure
/// filesystem walk — no COM, so this is orders of magnitude cheaper than
/// parsing the shortcuts it finds.
fn collect_lnk_paths() -> Vec<PathBuf> {
    let mut out = Vec::new();
    for root in start_menu_roots() {
        walk_lnk_paths(&root, &mut out, 0);
    }
    out
}

/// Scan Start Menu directories for `.lnk` files and parse each.
pub fn scan_start_menu() -> Vec<InstalledAppInfo> {
    // Initialise COM for this thread (best-effort; may already be initialised).
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
    }

    let mut out: Vec<InstalledAppInfo> = Vec::new();
    let mut seen_names = std::collections::HashSet::<String>::new();

    for path in collect_lnk_paths() {
        let Some(info) = parse_lnk(&path) else {
            continue;
        };
        // Deduplicate by normalised name — keep per-user over system.
        if seen_names.insert(normalize(&info.name)) {
            out.push(info);
        }
    }

    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// Resolve `id` against Start Menu shortcut *filenames*, parsing at most the
/// handful of shortcuts whose stem matches.
///
/// A shortcut's display name **is** its filename stem — `parse_lnk` never reads
/// a name out of the `.lnk` body — so `resolve`'s top tier can be decided from
/// the directory listing alone. That turns the common hotkey case from "COM-parse
/// every shortcut on the machine" into one filesystem walk plus a single parse.
///
/// Equivalent to `resolve(id, &scan_start_menu())` returning
/// `MatchType::InstalledName`: `scan_start_menu` walks the same order and keeps
/// the first shortcut per normalised name that parses, so the first parsable
/// stem match here is exactly the entry that would have survived its dedupe.
/// Returns `None` when nothing matches by name, or when every stem match is a
/// shortcut `parse_lnk` rejects (URL/folder targets) — the caller must fall back
/// to the full catalog in both cases.
pub fn resolve_start_menu_by_name(id: &str) -> Option<ResolvedMatch> {
    let needle = normalize(id);
    if needle.is_empty() {
        return None;
    }
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
    }
    stem_matches(collect_lnk_paths(), &needle)
        .into_iter()
        .find_map(|p| parse_lnk(&p))
        .map(|app| to_match(&app, MatchType::InstalledName))
}

/// Paths whose filename stem normalises to `needle`, in the order given.
fn stem_matches(paths: Vec<PathBuf>, needle: &str) -> Vec<PathBuf> {
    paths
        .into_iter()
        .filter(|p| {
            p.file_stem()
                .and_then(|s| s.to_str())
                .is_some_and(|stem| normalize(stem) == needle)
        })
        .collect()
}

/// Scan classic Start Menu shortcuts together with registered shell apps.
pub fn scan_installed_apps() -> Vec<InstalledAppInfo> {
    let mut out = scan_start_menu();
    merge_shell_apps(&mut out, scan_shell_apps());
    out
}

/// Enumerate launchable shell apps through the native AppsFolder namespace.
/// Packaged app AUMIDs contain `!`; Explorer is a built-in registered shell
/// application with the stable AUMID `Microsoft.Windows.Explorer`.
pub fn scan_shell_apps() -> Vec<InstalledAppInfo> {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
    }

    let Ok(folder) = (unsafe {
        SHGetKnownFolderItem::<IShellItem>(&FOLDERID_AppsFolder, KNOWN_FOLDER_FLAG(0), None)
    }) else {
        return Vec::new();
    };
    let Ok(items) =
        (unsafe { folder.BindToHandler::<_, IEnumShellItems>(None::<&IBindCtx>, &BHID_EnumItems) })
    else {
        return Vec::new();
    };

    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    loop {
        let mut next = [None];
        let mut fetched = 0;
        if unsafe { items.Next(&mut next, Some(&mut fetched)) }.is_err() || fetched == 0 {
            break;
        }
        let Some(app) = next[0].as_ref().and_then(shell_app_from_shell_item) else {
            continue;
        };
        if seen.insert(app.aumid.as_deref().unwrap_or_default().to_lowercase()) {
            out.push(app);
        }
    }
    out
}

fn shell_app_from_shell_item(item: &IShellItem) -> Option<InstalledAppInfo> {
    let item2: IShellItem2 = item.cast().ok()?;
    let aumid = taskmem_string(unsafe { item2.GetString(&PKEY_APP_USER_MODEL_ID).ok()? })?;
    if !is_catalog_shell_aumid(&aumid) {
        return None;
    }
    let name = taskmem_string(unsafe { item.GetDisplayName(SIGDN_NORMALDISPLAY).ok()? })?;

    Some(InstalledAppInfo {
        name,
        exe_path: String::new(),
        exe_name: String::new(),
        arguments: String::new(),
        shortcut_path: PathBuf::new(),
        aumid: Some(aumid),
    })
}

fn is_catalog_shell_aumid(aumid: &str) -> bool {
    aumid.contains('!') || aumid.eq_ignore_ascii_case("Microsoft.Windows.Explorer")
}

fn taskmem_string(value: PWSTR) -> Option<String> {
    let text = unsafe { value.to_string().ok() };
    unsafe {
        CoTaskMemFree(Some(value.0 as *const std::ffi::c_void));
    }
    text
}

/// Maximum directory depth to descend when scanning Start Menu. Real Start
/// Menu trees are ≤4 deep; the cap is just a guardrail against junction
/// loops or pathological structures that would otherwise hang the scan.
const MAX_LNK_DEPTH: u8 = 8;

/// Recursively collect `.lnk` paths from `dir`, bounded by `MAX_LNK_DEPTH`.
fn walk_lnk_paths(dir: &Path, out: &mut Vec<PathBuf>, depth: u8) {
    if depth > MAX_LNK_DEPTH {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk_lnk_paths(&path, out, depth + 1);
            continue;
        }
        if path.extension().and_then(|e| e.to_str()) == Some("lnk") {
            out.push(path);
        }
    }
}

/// Parse a single `.lnk` file via COM `IShellLinkW`.
fn parse_lnk(path: &Path) -> Option<InstalledAppInfo> {
    unsafe {
        // Create ShellLink COM object.
        let link: IShellLinkW =
            CoCreateInstance(&CLSID_SHELL_LINK, None, CLSCTX_INPROC_SERVER).ok()?;

        // Load the .lnk file.
        let persist: IPersistFile = link.cast().ok()?;
        let wide_path = to_wide_path(path);
        persist.Load(PCWSTR(wide_path.as_ptr()), STGM(0)).ok()?;

        // Read target path.
        let mut target_buf = [0u16; 1024];
        link.GetPath(&mut target_buf, std::ptr::null_mut(), 0)
            .ok()?;
        let target = wstr_to_string(&target_buf);

        // Skip shortcuts that don't point to an exe (e.g. URLs, folders).
        if target.is_empty() || !target.to_lowercase().ends_with(".exe") {
            return None;
        }

        // Read arguments.
        let mut args_buf = [0u16; 2048];
        let _ = link.GetArguments(&mut args_buf);
        let arguments = wstr_to_string(&args_buf);

        // Display name = filename without `.lnk`.
        let name = path.file_stem()?.to_str()?.to_string();

        // Exe name from target path.
        let exe_name = target.rsplit('\\').next().unwrap_or(&target).to_lowercase();

        Some(InstalledAppInfo {
            name,
            exe_path: target,
            exe_name,
            arguments,
            shortcut_path: path.to_path_buf(),
            aumid: None,
        })
    }
}

/// Resolve a user-supplied id against installed Windows apps.
pub fn resolve(id: &str, installed: &[InstalledAppInfo]) -> Option<ResolvedMatch> {
    let needle = normalize(id);

    // 1. Name exact match.
    if let Some(app) = installed.iter().find(|a| normalize(&a.name) == needle) {
        return Some(to_match(app, MatchType::InstalledName));
    }

    // 2. AppUserModelID exact match.
    if let Some(app) = installed
        .iter()
        .find(|a| a.aumid.as_deref().is_some_and(|v| normalize(v) == needle))
    {
        return Some(to_match(app, MatchType::InstalledAumid));
    }

    // 3. Exe stem/name match (e.g. `brave` or `brave.exe`).
    let needle_exe = if needle.ends_with(".exe") {
        needle.clone()
    } else {
        format!("{}.exe", needle)
    };
    if let Some(app) = installed.iter().find(|a| a.exe_name == needle_exe) {
        return Some(to_match(app, MatchType::InstalledExeStem));
    }

    // 4. Name substring (alphabetical-first wins).
    let mut subs: Vec<&InstalledAppInfo> = installed
        .iter()
        .filter(|a| normalize(&a.name).contains(&needle))
        .collect();
    subs.sort_by(|a, b| a.name.cmp(&b.name));
    subs.first()
        .map(|app| to_match(app, MatchType::InstalledNameSubstring))
}

/// Resolve `id` against the Start Menu, consulting the packaged-app
/// (AppsFolder) catalog only when the Start Menu alone cannot settle it.
///
/// `shell_loader` is `FnOnce` and is **not** called when a Start Menu shortcut
/// matches `id` by exact display name. That is the top tier of `resolve`, and
/// a shortcut (`aumid: None`) sorts ahead of a packaged app of the same name,
/// so enumerating AppsFolder could not change the answer — see
/// `resolve_lazy_agrees_with_one_shot_resolve`. Every weaker tier (AUMID, exe
/// stem, name substring) can be beaten by a packaged app's exact name, so
/// those fall through to the full scan.
///
/// This matters because the two scans are not remotely equal in cost: parsing
/// the Start Menu `.lnk` tree takes tens of milliseconds, while enumerating
/// `FOLDERID_AppsFolder` costs several hundred. Deferring it keeps the common
/// hot-path case — a hotkey aimed at an app that has a Start Menu entry — off
/// the slow path. Mirrors `beckon_macos::apps::resolve_inner`, which defers
/// `installed_apps()` the same way.
pub fn resolve_lazy(
    id: &str,
    start_menu: &[InstalledAppInfo],
    shell_loader: impl FnOnce() -> Vec<InstalledAppInfo>,
) -> Option<ResolvedMatch> {
    if let Some(m) = resolve(id, start_menu) {
        if m.match_type == MatchType::InstalledName {
            return Some(m);
        }
    }

    let mut all = start_menu.to_vec();
    merge_shell_apps(&mut all, shell_loader());
    resolve(id, &all)
}

/// Append packaged apps to `out` and restore the canonical catalog order:
/// by display name, then AUMID — so a Start Menu shortcut (`aumid: None`)
/// precedes a packaged app sharing its name.
fn merge_shell_apps(out: &mut Vec<InstalledAppInfo>, shell_apps: Vec<InstalledAppInfo>) {
    out.extend(shell_apps);
    out.sort_by(|a, b| a.name.cmp(&b.name).then_with(|| a.aumid.cmp(&b.aumid)));
}

fn to_match(app: &InstalledAppInfo, match_type: MatchType) -> ResolvedMatch {
    ResolvedMatch {
        name: app.name.clone(),
        exe_path: app.exe_path.clone(),
        exe_name: app.exe_name.clone(),
        arguments: app.arguments.clone(),
        shortcut_path: app.shortcut_path.clone(),
        aumid: app.aumid.clone(),
        match_type,
    }
}

/// Substring matches across installed apps (for `resolve` ambiguity warnings).
pub fn name_substring_matches(id: &str, installed: &[InstalledAppInfo]) -> Vec<InstalledAppInfo> {
    let needle = normalize(id);
    if needle.is_empty() {
        return Vec::new();
    }
    let mut matches: Vec<InstalledAppInfo> = installed
        .iter()
        .filter(|a| normalize(&a.name).contains(&needle))
        .cloned()
        .collect();
    matches.sort_by(|a, b| a.name.cmp(&b.name));
    matches
}

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

// -- helpers --

fn to_wide_path(path: &Path) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    path.as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

fn wstr_to_string(buf: &[u16]) -> String {
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..len])
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    fn app(name: &str, exe: &str) -> InstalledAppInfo {
        InstalledAppInfo {
            name: name.to_string(),
            exe_path: format!("C:\\Program Files\\{}", exe),
            exe_name: exe.to_lowercase(),
            arguments: String::new(),
            shortcut_path: PathBuf::from(format!(
                "C:\\Users\\test\\AppData\\Roaming\\Microsoft\\Windows\\Start Menu\\Programs\\{}.lnk",
                name
            )),
            aumid: None,
        }
    }

    fn appx(name: &str, aumid: &str, exe: &str) -> InstalledAppInfo {
        InstalledAppInfo {
            name: name.to_string(),
            exe_path: String::new(),
            exe_name: exe.to_lowercase(),
            arguments: String::new(),
            shortcut_path: PathBuf::new(),
            aumid: Some(aumid.to_string()),
        }
    }

    // ---------- normalize ----------

    #[test]
    fn normalize_lowercases_and_collapses() {
        assert_eq!(normalize("Visual Studio Code"), "visual studio code");
        assert_eq!(normalize("  Brave   Browser  "), "brave browser");
    }

    #[test]
    fn normalize_strips_format_marks() {
        assert_eq!(normalize("\u{200E}Claude"), "claude");
        assert_eq!(normalize("\u{FEFF}Foo \u{2069}Bar"), "foo bar");
    }

    // ---------- resolve priority ----------

    #[test]
    fn resolve_name_exact_wins() {
        let installed = vec![app("Brave", "brave.exe"), app("Brave Browser", "brave.exe")];
        let m = resolve("Brave", &installed).unwrap();
        assert_eq!(m.match_type, MatchType::InstalledName);
        assert_eq!(m.name, "Brave");
    }

    #[test]
    fn resolve_name_exact_is_case_insensitive() {
        let installed = vec![app("Claude", "claude.exe")];
        let m = resolve("CLAUDE", &installed).unwrap();
        assert_eq!(m.match_type, MatchType::InstalledName);
    }

    #[test]
    fn resolve_falls_through_to_exe_stem() {
        // No exact name match for "brave", but exe_name = "brave.exe".
        let installed = vec![app("Brave Browser", "brave.exe")];
        let m = resolve("brave", &installed).unwrap();
        assert_eq!(m.match_type, MatchType::InstalledExeStem);
    }

    #[test]
    fn resolve_appx_by_name_and_aumid() {
        let installed = vec![appx(
            "Terminal",
            "Microsoft.WindowsTerminal_8wekyb3d8bbwe!App",
            "WindowsTerminal.exe",
        )];
        let by_name = resolve("Terminal", &installed).unwrap();
        assert_eq!(by_name.match_type, MatchType::InstalledName);
        assert_eq!(
            by_name.aumid.as_deref(),
            Some("Microsoft.WindowsTerminal_8wekyb3d8bbwe!App")
        );

        let by_aumid = resolve("Microsoft.WindowsTerminal_8wekyb3d8bbwe!App", &installed).unwrap();
        assert_eq!(by_aumid.match_type, MatchType::InstalledAumid);
    }

    #[test]
    fn catalog_includes_packaged_apps_and_file_explorer_aumid() {
        assert!(is_catalog_shell_aumid(
            "Microsoft.WindowsTerminal_8wekyb3d8bbwe!App"
        ));
        assert!(is_catalog_shell_aumid("Microsoft.Windows.Explorer"));
        assert!(!is_catalog_shell_aumid("Some.Desktop.Application"));
    }

    #[test]
    fn resolve_falls_through_to_substring_alphabetical() {
        let installed = vec![
            app("Zeta Browser", "zeta.exe"),
            app("Alpha Browser", "alpha.exe"),
        ];
        let m = resolve("Browser", &installed).unwrap();
        assert_eq!(m.match_type, MatchType::InstalledNameSubstring);
        // Alphabetical-first by display name.
        assert_eq!(m.name, "Alpha Browser");
    }

    #[test]
    fn resolve_returns_none_on_total_miss() {
        let installed = vec![app("Brave", "brave.exe")];
        assert!(resolve("thunderbird", &installed).is_none());
    }

    #[test]
    fn resolve_empty_installed_returns_none() {
        assert!(resolve("anything", &[]).is_none());
    }

    #[test]
    fn resolve_bidi_prefixed_name_matches_ascii_query() {
        // PWA shortcut whose Name has a leading U+200E.
        let installed = vec![app("\u{200E}Claude", "brave.exe")];
        let m = resolve("Claude", &installed).unwrap();
        assert_eq!(m.match_type, MatchType::InstalledName);
    }

    #[test]
    fn resolve_accepts_full_exe_name() {
        let installed = vec![app("Brave", "brave.exe")];
        let m = resolve("brave.exe", &installed).unwrap();
        assert_eq!(m.match_type, MatchType::InstalledExeStem);
    }

    // ---------- name_substring_matches ----------

    #[test]
    fn name_substring_matches_returns_sorted_by_name() {
        let installed = vec![
            app("Zeta", "zeta.exe"),
            app("Beta", "beta.exe"),
            app("Alpha", "alpha.exe"),
        ];
        let names: Vec<_> = name_substring_matches("eta", &installed)
            .into_iter()
            .map(|a| a.name)
            .collect();
        assert_eq!(names, vec!["Beta", "Zeta"]);
    }

    #[test]
    fn name_substring_matches_empty_needle_returns_empty() {
        let installed = vec![app("Brave", "brave.exe")];
        assert!(name_substring_matches("", &installed).is_empty());
    }

    // ---------- resolve_lazy (deferred AppsFolder scan) ----------

    #[test]
    fn resolve_lazy_skips_shell_scan_on_start_menu_name_hit() {
        let start_menu = vec![app("Claude", "brave.exe")];
        let called = Cell::new(false);
        let m = resolve_lazy("Claude", &start_menu, || {
            called.set(true);
            Vec::new()
        })
        .unwrap();
        assert_eq!(m.match_type, MatchType::InstalledName);
        assert!(
            !called.get(),
            "AppsFolder scan must not run on an exact Start Menu hit"
        );
    }

    #[test]
    fn resolve_lazy_start_menu_shortcut_wins_exact_name_tie() {
        // Documented tie-break: a Start Menu shortcut beats a packaged app of
        // the same display name, so the scan can be skipped entirely.
        let start_menu = vec![app("Terminal", "wt.exe")];
        let called = Cell::new(false);
        let m = resolve_lazy("Terminal", &start_menu, || {
            called.set(true);
            vec![appx(
                "Terminal",
                "Microsoft.WindowsTerminal_8wekyb3d8bbwe!App",
                "WindowsTerminal.exe",
            )]
        })
        .unwrap();
        assert!(!called.get());
        assert_eq!(m.aumid, None);
        assert_eq!(m.exe_name, "wt.exe");
    }

    #[test]
    fn resolve_lazy_defers_when_start_menu_only_matches_substring() {
        // Substring is the weakest tier — a packaged app matching by exact
        // name outranks it, so the scan must still run.
        let start_menu = vec![app("Brave Browser", "bravebrowser.exe")];
        let called = Cell::new(false);
        let m = resolve_lazy("Brave", &start_menu, || {
            called.set(true);
            vec![appx("Brave", "Brave_8wekyb3d8bbwe!App", "brave.exe")]
        })
        .unwrap();
        assert!(called.get());
        assert_eq!(m.match_type, MatchType::InstalledName);
        assert_eq!(m.name, "Brave");
    }

    #[test]
    fn resolve_lazy_defers_when_start_menu_only_matches_exe_stem() {
        // Same reasoning one tier up: exe-stem loses to a packaged app's
        // exact name, so exe-stem must not short-circuit either.
        let start_menu = vec![app("Brave Browser", "brave.exe")];
        let called = Cell::new(false);
        let m = resolve_lazy("brave", &start_menu, || {
            called.set(true);
            vec![appx("Brave", "Brave_8wekyb3d8bbwe!App", "brave.exe")]
        })
        .unwrap();
        assert!(called.get());
        assert_eq!(m.match_type, MatchType::InstalledName);
        assert_eq!(m.name, "Brave");
    }

    #[test]
    fn resolve_lazy_finds_packaged_app_on_start_menu_miss() {
        let start_menu = vec![app("Brave", "brave.exe")];
        let called = Cell::new(false);
        let m = resolve_lazy("Terminal", &start_menu, || {
            called.set(true);
            vec![appx(
                "Terminal",
                "Microsoft.WindowsTerminal_8wekyb3d8bbwe!App",
                "WindowsTerminal.exe",
            )]
        })
        .unwrap();
        assert!(called.get());
        assert_eq!(
            m.aumid.as_deref(),
            Some("Microsoft.WindowsTerminal_8wekyb3d8bbwe!App")
        );
    }

    #[test]
    fn resolve_lazy_returns_none_when_nothing_matches() {
        let called = Cell::new(false);
        let m = resolve_lazy("thunderbird", &[app("Brave", "brave.exe")], || {
            called.set(true);
            Vec::new()
        });
        assert!(called.get());
        assert!(m.is_none());
    }

    #[test]
    fn resolve_lazy_agrees_with_one_shot_resolve() {
        // The whole point of deferring is that it changes cost, not answers.
        let start_menu = vec![app("Brave", "brave.exe"), app("Claude", "brave.exe")];
        let shell = vec![
            appx(
                "Terminal",
                "Microsoft.WindowsTerminal_8wekyb3d8bbwe!App",
                "WindowsTerminal.exe",
            ),
            appx(
                "File Explorer",
                "Microsoft.Windows.Explorer",
                "explorer.exe",
            ),
        ];
        let mut combined = start_menu.clone();
        combined.extend(shell.clone());
        combined.sort_by(|a, b| a.name.cmp(&b.name).then_with(|| a.aumid.cmp(&b.aumid)));

        for id in [
            "Brave",
            "Claude",
            "Terminal",
            "File Explorer",
            "brave",
            "brave.exe",
            "Microsoft.Windows.Explorer",
            "Term",
            "thunderbird",
        ] {
            let lazy = resolve_lazy(id, &start_menu, || shell.clone());
            let one_shot = resolve(id, &combined);
            assert_eq!(
                lazy.as_ref().map(|m| (m.name.clone(), m.match_type)),
                one_shot.as_ref().map(|m| (m.name.clone(), m.match_type)),
                "resolve_lazy diverged from resolve for id `{}`",
                id
            );
        }
    }

    // ---------- stem_matches (filename-only name tier) ----------

    fn paths(list: &[&str]) -> Vec<PathBuf> {
        list.iter().map(PathBuf::from).collect()
    }

    #[test]
    fn stem_matches_is_case_insensitive_and_keeps_order() {
        let found = stem_matches(
            paths(&[
                r"C:\sys\Claude.lnk",
                r"C:\user\CLAUDE.lnk",
                r"C:\user\Brave.lnk",
            ]),
            "claude",
        );
        // Traversal order is the dedupe rule, so it must survive filtering.
        assert_eq!(found, paths(&[r"C:\sys\Claude.lnk", r"C:\user\CLAUDE.lnk"]));
    }

    #[test]
    fn stem_matches_ignores_bidi_marks_in_filename() {
        // PWA shortcuts really do ship names with a leading U+200E.
        let found = stem_matches(paths(&["\u{200E}Claude.lnk"]), "claude");
        assert_eq!(found.len(), 1);
    }

    #[test]
    fn stem_matches_requires_whole_stem_not_substring() {
        // The name tier is exact — substring is a strictly lower tier that
        // only the full catalog scan can settle.
        assert!(stem_matches(paths(&["Brave Browser.lnk"]), "brave").is_empty());
    }

    #[test]
    fn stem_matches_returns_empty_on_miss() {
        assert!(stem_matches(paths(&["Brave.lnk"]), "thunderbird").is_empty());
    }

    // ---------- walk_lnk_paths depth limit ----------

    #[test]
    fn walk_lnk_paths_respects_max_depth() {
        let dir =
            std::env::temp_dir().join(format!("beckon-lnk-depth-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        // Shallow shortcut: must be collected.
        std::fs::write(dir.join("shallow.lnk"), b"").unwrap();

        // Build dir/0/1/.../9/deep.lnk (depth 10, exceeds MAX_LNK_DEPTH=8).
        let mut deep = dir.clone();
        for i in 0..10 {
            deep = deep.join(i.to_string());
            std::fs::create_dir_all(&deep).unwrap();
        }
        std::fs::write(deep.join("deep.lnk"), b"").unwrap();

        // The walk only lists paths — no COM, no parsing — so empty file
        // bodies are fine and the depth guard is directly observable now.
        let mut out = Vec::new();
        walk_lnk_paths(&dir, &mut out, 0);

        let _ = std::fs::remove_dir_all(&dir);
        let names: Vec<&str> = out
            .iter()
            .filter_map(|p| p.file_name().and_then(|n| n.to_str()))
            .collect();
        assert_eq!(names, vec!["shallow.lnk"]);
    }

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
}
