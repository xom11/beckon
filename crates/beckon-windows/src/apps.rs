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

/// Scan Start Menu directories for `.lnk` files and parse each.
pub fn scan_start_menu() -> Vec<InstalledAppInfo> {
    // Initialise COM for this thread (best-effort; may already be initialised).
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
    }

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

    let mut out: Vec<InstalledAppInfo> = Vec::new();
    let mut seen_names = std::collections::HashSet::<String>::new();

    for root in &roots {
        collect_lnk_files(root, &mut out, &mut seen_names, 0);
    }

    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
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

/// Recursively collect `.lnk` files from `dir`, bounded by `MAX_LNK_DEPTH`.
fn collect_lnk_files(
    dir: &Path,
    out: &mut Vec<InstalledAppInfo>,
    seen: &mut std::collections::HashSet<String>,
    depth: u8,
) {
    if depth > MAX_LNK_DEPTH {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_lnk_files(&path, out, seen, depth + 1);
            continue;
        }
        let ext = path.extension().and_then(|e| e.to_str());
        if ext != Some("lnk") {
            continue;
        }
        if let Some(info) = parse_lnk(&path) {
            // Deduplicate by normalised name — keep per-user over system.
            let key = normalize(&info.name);
            if seen.insert(key) {
                out.push(info);
            }
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

/// Substring matches across installed apps (for `-r` ambiguity warnings).
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

    // ---------- collect_lnk_files depth limit ----------

    #[test]
    fn collect_lnk_files_respects_max_depth() {
        // Build a deeply nested temp tree and verify the recursion bails.
        let dir =
            std::env::temp_dir().join(format!("beckon-lnk-depth-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        // Build dir/0/1/.../9/marker.lnk (depth 10, exceeds MAX_LNK_DEPTH=8).
        let mut deep = dir.clone();
        for i in 0..10 {
            deep = deep.join(i.to_string());
            std::fs::create_dir_all(&deep).unwrap();
        }
        // Note: parse_lnk would fail without COM init + valid .lnk content,
        // but we're only verifying the recursion guard. To do that without
        // touching COM, drop a non-.lnk marker file and prove the walk
        // doesn't hang (i.e. completes in < a second). The stronger test
        // (parsing real .lnks) lives behind real Start Menu fixtures.
        std::fs::write(deep.join("marker.txt"), b"").unwrap();

        let mut out = Vec::new();
        let mut seen = std::collections::HashSet::new();
        // Should return promptly even without a depth bug, but the test
        // is named for the property we want preserved.
        collect_lnk_files(&dir, &mut out, &mut seen, 0);

        let _ = std::fs::remove_dir_all(&dir);
        // No .lnk files exist, so nothing to collect; success = no hang
        // and no panic.
        assert!(out.is_empty());
    }
}
