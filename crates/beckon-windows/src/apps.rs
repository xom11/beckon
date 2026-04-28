//! Start Menu shortcut (.lnk) scanning and Name -> exe resolution.
//!
//! Scan paths:
//!   - `%APPDATA%\Microsoft\Windows\Start Menu\Programs\` (per-user)
//!   - `%ProgramData%\Microsoft\Windows\Start Menu\Programs\` (system-wide)
//!
//! Resolution priority (mirrors Linux .desktop / macOS LaunchServices):
//!   1. Installed name exact match (case-insensitive, normalised).
//!   2. Exe filename stem match (e.g. `brave` matches `brave.exe`).
//!   3. Installed name substring (alphabetical-first wins).

use std::path::{Path, PathBuf};
use windows::core::{Interface, GUID, PCWSTR};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, IPersistFile, CLSCTX_INPROC_SERVER,
    COINIT_APARTMENTTHREADED, STGM,
};
use windows::Win32::UI::Shell::IShellLinkW;

/// CLSID for ShellLink COM class: {00021401-0000-0000-C000-000000000046}
const CLSID_SHELL_LINK: GUID = GUID::from_u128(0x00021401_0000_0000_c000_000000000046);

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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchType {
    InstalledName,
    InstalledExeStem,
    InstalledNameSubstring,
}

impl MatchType {
    pub fn describe(self) -> &'static str {
        match self {
            MatchType::InstalledName => "Start Menu shortcut name (exact)",
            MatchType::InstalledExeStem => "exe filename stem",
            MatchType::InstalledNameSubstring => "Start Menu shortcut name (substring)",
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
        link.GetPath(&mut target_buf, std::ptr::null_mut(), 0).ok()?;
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
        let exe_name = target
            .rsplit('\\')
            .next()
            .unwrap_or(&target)
            .to_lowercase();

        Some(InstalledAppInfo {
            name,
            exe_path: target,
            exe_name,
            arguments,
            shortcut_path: path.to_path_buf(),
        })
    }
}

/// Resolve a user-supplied id against installed Start Menu apps.
pub fn resolve(id: &str, installed: &[InstalledAppInfo]) -> Option<ResolvedMatch> {
    let needle = normalize(id);

    // 1. Name exact match.
    if let Some(app) = installed.iter().find(|a| normalize(&a.name) == needle) {
        return Some(to_match(app, MatchType::InstalledName));
    }

    // 2. Exe stem match (e.g. `brave` matches `brave.exe`).
    let needle_exe = format!("{}.exe", needle);
    if let Some(app) = installed.iter().find(|a| a.exe_name == needle_exe) {
        return Some(to_match(app, MatchType::InstalledExeStem));
    }

    // 3. Name substring (alphabetical-first wins).
    let mut subs: Vec<&InstalledAppInfo> = installed
        .iter()
        .filter(|a| normalize(&a.name).contains(&needle))
        .collect();
    subs.sort_by(|a, b| a.name.cmp(&b.name));
    subs.first().map(|app| to_match(app, MatchType::InstalledNameSubstring))
}

fn to_match(app: &InstalledAppInfo, match_type: MatchType) -> ResolvedMatch {
    ResolvedMatch {
        name: app.name.clone(),
        exe_path: app.exe_path.clone(),
        exe_name: app.exe_name.clone(),
        arguments: app.arguments.clone(),
        shortcut_path: app.shortcut_path.clone(),
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
