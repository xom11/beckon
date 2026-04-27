//! Minimal .desktop file parser. Hand-rolled to avoid pulling in a crate
//! for ~80 lines of work. Reads the `[Desktop Entry]` section only and
//! pulls Name / Exec / StartupWMClass.
//!
//! Field-code stripping in Exec follows the XDG Desktop Entry Spec:
//! `%f %F %u %U %d %D %n %N %i %c %k %v %m` are removed; `%%` becomes `%`.

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct DesktopEntry {
    /// Filename without `.desktop` extension (e.g. `brave-fmpnliohjh...-Default`).
    pub id: String,
    pub name: String,
    pub exec: String,
    /// Equal to sway's `app_id` for Wayland apps that set this hint.
    pub startup_wm_class: Option<String>,
    pub no_display: bool,
}

pub fn scan() -> Vec<DesktopEntry> {
    let mut by_id: HashMap<String, DesktopEntry> = HashMap::new();

    // Spec precedence: $XDG_DATA_HOME wins over $XDG_DATA_DIRS. We scan
    // user dir last so it overwrites system entries with the same id.
    let mut dirs = system_app_dirs();
    dirs.extend(user_app_dirs());

    for dir in dirs {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("desktop") {
                continue;
            }
            if let Some(d) = parse(&path) {
                if d.no_display {
                    continue;
                }
                by_id.insert(d.id.clone(), d);
            }
        }
    }

    by_id.into_values().collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchType {
    NameExact,
    Filename,
    StartupWmClass,
    NameSubstring,
}

impl MatchType {
    pub fn describe(self) -> &'static str {
        match self {
            MatchType::NameExact => "Name= exact (case-insensitive)",
            MatchType::Filename => ".desktop filename",
            MatchType::StartupWmClass => "StartupWMClass=",
            MatchType::NameSubstring => "Name= substring (alphabetical first wins)",
        }
    }
}

#[derive(Debug, Clone)]
pub struct ResolvedMatch {
    pub entry: DesktopEntry,
    pub match_type: MatchType,
}

/// Resolve a user-supplied id to a desktop entry. Tries four matches in
/// priority order, all against the same `scan()` enumeration:
///
///   1. `Name` exact (case-insensitive, normalized — strips bidi marks).
///      This is the recommended way to reference apps in dotfiles because
///      Name is stable across machines (Brave PWA hashes are not).
///   2. `.desktop` filename stem (`kitty.desktop` → id `kitty`).
///      Useful when the user copy-pastes a runtime app_id from `beckon -l`.
///   3. `StartupWMClass` (rarely correct on Wayland because clients like
///      Brave ignore it, but harmless to try).
///   4. `Name` substring (case-insensitive). Multiple matches resolve to
///      the alphabetically first `.desktop` filename — same "first wins"
///      rule as rofi.
///
/// Returns `None` if nothing matches.
pub fn resolve(id: &str) -> Option<DesktopEntry> {
    resolve_detailed(id).map(|m| m.entry)
}

/// Same as [`resolve`] but reports which priority matched and lets the
/// `-r` debug command explain its reasoning.
pub fn resolve_detailed(id: &str) -> Option<ResolvedMatch> {
    let entries = scan();
    let needle = normalize(id);

    if let Some(e) = entries.iter().find(|e| normalize(&e.name) == needle) {
        return Some(ResolvedMatch {
            entry: e.clone(),
            match_type: MatchType::NameExact,
        });
    }
    if let Some(e) = entries.iter().find(|e| e.id == id) {
        return Some(ResolvedMatch {
            entry: e.clone(),
            match_type: MatchType::Filename,
        });
    }
    if let Some(e) = entries
        .iter()
        .find(|e| e.startup_wm_class.as_deref() == Some(id))
    {
        return Some(ResolvedMatch {
            entry: e.clone(),
            match_type: MatchType::StartupWmClass,
        });
    }
    let mut subs: Vec<&DesktopEntry> = entries
        .iter()
        .filter(|e| normalize(&e.name).contains(&needle))
        .collect();
    subs.sort_by(|a, b| a.id.cmp(&b.id));
    subs.first().map(|e| ResolvedMatch {
        entry: (*e).clone(),
        match_type: MatchType::NameSubstring,
    })
}

/// All entries whose Name contains `id` as a case-insensitive substring,
/// sorted alphabetically by `.desktop` filename. Used by `-r` to flag
/// ambiguity (multiple substring matches) and to suggest "did you mean".
pub fn name_substring_matches(id: &str) -> Vec<DesktopEntry> {
    let needle = normalize(id);
    if needle.is_empty() {
        return Vec::new();
    }
    let mut matches: Vec<DesktopEntry> = scan()
        .into_iter()
        .filter(|e| normalize(&e.name).contains(&needle))
        .collect();
    matches.sort_by(|a, b| a.id.cmp(&b.id));
    matches
}

/// Lowercase, drop Unicode bidi/format marks, collapse whitespace.
/// Brave PWAs sometimes prefix Name with U+200E LEFT-TO-RIGHT MARK
/// (e.g. "‎Google Gemini") which would otherwise break exact match.
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
        '\u{200E}' | '\u{200F}'                // LRM, RLM
            | '\u{202A}'..='\u{202E}'          // bidi embeddings/overrides
            | '\u{2066}'..='\u{2069}'          // bidi isolates
            | '\u{FEFF}'                       // zero-width no-break space
    )
}

fn parse(path: &PathBuf) -> Option<DesktopEntry> {
    let content = fs::read_to_string(path).ok()?;
    let mut name = None;
    let mut exec = None;
    let mut wm_class = None;
    let mut no_display = false;
    let mut entry_type = None;
    let mut in_section = false;

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            in_section = line == "[Desktop Entry]";
            continue;
        }
        if !in_section {
            continue;
        }
        if let Some(v) = line.strip_prefix("Name=") {
            if name.is_none() {
                name = Some(v.to_string());
            }
        } else if let Some(v) = line.strip_prefix("Exec=") {
            exec = Some(v.to_string());
        } else if let Some(v) = line.strip_prefix("StartupWMClass=") {
            wm_class = Some(v.to_string());
        } else if let Some(v) = line.strip_prefix("NoDisplay=") {
            no_display = v.eq_ignore_ascii_case("true");
        } else if let Some(v) = line.strip_prefix("Type=") {
            entry_type = Some(v.to_string());
        }
    }

    if entry_type.as_deref() != Some("Application") {
        return None;
    }

    let id = path.file_stem()?.to_str()?.to_string();
    Some(DesktopEntry {
        id,
        name: name?,
        exec: strip_field_codes(&exec?),
        startup_wm_class: wm_class,
        no_display,
    })
}

fn strip_field_codes(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '%' {
            match chars.next() {
                Some('%') => out.push('%'),
                Some(_) => {}
                None => {}
            }
        } else {
            out.push(c);
        }
    }
    // Collapse multi-spaces left behind by removed field codes.
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn user_app_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    let xdg_data_home = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")));
    if let Some(d) = xdg_data_home {
        dirs.push(d.join("applications"));
    }
    dirs
}

fn system_app_dirs() -> Vec<PathBuf> {
    let raw = std::env::var("XDG_DATA_DIRS")
        .unwrap_or_else(|_| "/usr/local/share:/usr/share".to_string());
    raw.split(':')
        .filter(|s| !s.is_empty())
        .map(|d| PathBuf::from(d).join("applications"))
        .collect()
}
