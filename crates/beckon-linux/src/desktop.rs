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
    resolve_detailed_in(&scan(), id)
}

/// Pure resolution against a caller-supplied entry list. Lets tests cover
/// the priority ladder without touching the filesystem.
pub fn resolve_detailed_in(entries: &[DesktopEntry], id: &str) -> Option<ResolvedMatch> {
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
    let id = path.file_stem()?.to_str()?.to_string();
    parse_str(&content, &id)
}

/// Parse the textual contents of a `.desktop` file into an entry. Pure —
/// the caller supplies the id (filename stem) so tests don't need files.
pub fn parse_str(content: &str, id: &str) -> Option<DesktopEntry> {
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

    Some(DesktopEntry {
        id: id.to_string(),
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

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: &str, name: &str) -> DesktopEntry {
        DesktopEntry {
            id: id.to_string(),
            name: name.to_string(),
            exec: format!("{} %U", id),
            startup_wm_class: None,
            no_display: false,
        }
    }

    fn entry_with_wm(id: &str, name: &str, wm: &str) -> DesktopEntry {
        let mut e = entry(id, name);
        e.startup_wm_class = Some(wm.to_string());
        e
    }

    // ---------- normalize ----------

    #[test]
    fn normalize_lowercases_and_trims() {
        assert_eq!(normalize("Brave Browser"), "brave browser");
        assert_eq!(normalize("  Kitty  "), "kitty");
    }

    #[test]
    fn normalize_collapses_internal_whitespace() {
        assert_eq!(normalize("Visual   Studio   Code"), "visual studio code");
        assert_eq!(normalize("Foo\tBar"), "foo bar");
    }

    #[test]
    fn normalize_strips_bidi_marks() {
        // U+200E LEFT-TO-RIGHT MARK that Brave PWAs prepend.
        assert_eq!(normalize("\u{200E}Google Gemini"), "google gemini");
        // BOM and isolates too.
        assert_eq!(normalize("\u{FEFF}Claude"), "claude");
        assert_eq!(normalize("\u{2068}Foo\u{2069}"), "foo");
    }

    #[test]
    fn normalize_preserves_non_format_unicode() {
        assert_eq!(normalize("café"), "café");
        assert_eq!(normalize("日本語"), "日本語");
    }

    // ---------- strip_field_codes ----------

    #[test]
    fn strip_field_codes_drops_known_codes() {
        assert_eq!(strip_field_codes("brave %U"), "brave");
        assert_eq!(strip_field_codes("kitty %f"), "kitty");
        assert_eq!(strip_field_codes("foo %i %c %k --bar"), "foo --bar");
    }

    #[test]
    fn strip_field_codes_handles_double_percent() {
        assert_eq!(strip_field_codes("echo 100%%"), "echo 100%");
    }

    #[test]
    fn strip_field_codes_collapses_left_over_spaces() {
        // Removing field codes between args must not leave double spaces.
        assert_eq!(strip_field_codes("vlc %U --intf qt"), "vlc --intf qt");
    }

    #[test]
    fn strip_field_codes_handles_trailing_percent() {
        assert_eq!(strip_field_codes("foo %"), "foo");
    }

    // ---------- parse_str ----------

    #[test]
    fn parse_basic_application() {
        let s = "[Desktop Entry]\n\
                 Type=Application\n\
                 Name=Kitty\n\
                 Exec=kitty %U\n";
        let e = parse_str(s, "kitty").unwrap();
        assert_eq!(e.id, "kitty");
        assert_eq!(e.name, "Kitty");
        assert_eq!(e.exec, "kitty");
        assert_eq!(e.startup_wm_class, None);
        assert!(!e.no_display);
    }

    #[test]
    fn parse_skips_link_type() {
        let s = "[Desktop Entry]\nType=Link\nName=foo\nURL=https://x\n";
        assert!(parse_str(s, "foo").is_none());
    }

    #[test]
    fn parse_requires_application_type() {
        // No Type= at all -> still rejected.
        let s = "[Desktop Entry]\nName=Foo\nExec=foo\n";
        assert!(parse_str(s, "foo").is_none());
    }

    #[test]
    fn parse_no_display_true() {
        let s = "[Desktop Entry]\nType=Application\nName=Hidden\nExec=hidden\nNoDisplay=true\n";
        assert!(parse_str(s, "hidden").unwrap().no_display);
    }

    #[test]
    fn parse_no_display_case_insensitive() {
        let s = "[Desktop Entry]\nType=Application\nName=H\nExec=h\nNoDisplay=TRUE\n";
        assert!(parse_str(s, "h").unwrap().no_display);
    }

    #[test]
    fn parse_picks_first_name_only() {
        // Spec says localized Name[xx]= entries follow; we ignore them but
        // also guard against a duplicate plain Name= overwriting the first.
        let s = "[Desktop Entry]\n\
                 Type=Application\n\
                 Name=First\n\
                 Name=Second\n\
                 Exec=foo\n";
        assert_eq!(parse_str(s, "f").unwrap().name, "First");
    }

    #[test]
    fn parse_ignores_other_sections() {
        let s = "[Desktop Action New]\n\
                 Name=ShouldNotWin\n\
                 Exec=should-not-win\n\
                 [Desktop Entry]\n\
                 Type=Application\n\
                 Name=Real\n\
                 Exec=real\n";
        let e = parse_str(s, "real").unwrap();
        assert_eq!(e.name, "Real");
        assert_eq!(e.exec, "real");
    }

    #[test]
    fn parse_skips_comments_and_blanks() {
        let s = "# comment at top\n\
                 \n\
                 [Desktop Entry]\n\
                 # inside\n\
                 Type=Application\n\
                 \n\
                 Name=X\n\
                 Exec=x\n";
        assert_eq!(parse_str(s, "x").unwrap().name, "X");
    }

    #[test]
    fn parse_picks_up_startup_wm_class() {
        let s = "[Desktop Entry]\n\
                 Type=Application\n\
                 Name=Foot\n\
                 Exec=foot\n\
                 StartupWMClass=foot\n";
        assert_eq!(
            parse_str(s, "foot").unwrap().startup_wm_class.as_deref(),
            Some("foot")
        );
    }

    #[test]
    fn parse_missing_required_returns_none() {
        // Name missing.
        let s = "[Desktop Entry]\nType=Application\nExec=foo\n";
        assert!(parse_str(s, "foo").is_none());
        // Exec missing.
        let s = "[Desktop Entry]\nType=Application\nName=Foo\n";
        assert!(parse_str(s, "foo").is_none());
    }

    // ---------- resolve_detailed_in priority ----------

    #[test]
    fn resolve_prefers_name_exact_over_filename() {
        // Two entries: one whose filename matches "Foo" exactly, one whose
        // Name= matches. NameExact should win.
        let entries = vec![
            entry("Foo", "Other"), // filename hit
            entry("bar", "Foo"),   // Name=Foo
        ];
        let m = resolve_detailed_in(&entries, "Foo").unwrap();
        assert_eq!(m.match_type, MatchType::NameExact);
        assert_eq!(m.entry.id, "bar");
    }

    #[test]
    fn resolve_name_exact_is_case_insensitive() {
        let entries = vec![entry("kitty", "Kitty")];
        let m = resolve_detailed_in(&entries, "KITTY").unwrap();
        assert_eq!(m.match_type, MatchType::NameExact);
    }

    #[test]
    fn resolve_falls_through_to_filename() {
        let entries = vec![entry("kitty", "Kitty Terminal")];
        let m = resolve_detailed_in(&entries, "kitty").unwrap();
        // "kitty" doesn't equal Name="Kitty Terminal" exactly, so falls to
        // filename. (Note this is actually reachable because normalize
        // collapses but doesn't strip; "kitty" != "kitty terminal".)
        assert_eq!(m.match_type, MatchType::Filename);
    }

    #[test]
    fn resolve_falls_through_to_wm_class() {
        let entries = vec![entry_with_wm("foot-app", "FooApp", "foot")];
        let m = resolve_detailed_in(&entries, "foot").unwrap();
        assert_eq!(m.match_type, MatchType::StartupWmClass);
    }

    #[test]
    fn resolve_falls_through_to_substring_alphabetical_first() {
        let entries = vec![
            entry("zeta", "Zeta Browser"),
            entry("alpha", "Alpha Browser"),
        ];
        let m = resolve_detailed_in(&entries, "browser").unwrap();
        assert_eq!(m.match_type, MatchType::NameSubstring);
        // "alpha" sorts before "zeta" by .desktop filename.
        assert_eq!(m.entry.id, "alpha");
    }

    #[test]
    fn resolve_substring_handles_bidi_prefixed_name() {
        // PWA installed with U+200E in Name. User types ASCII; should match.
        let mut e = entry("brave-claude-Default", "Claude");
        e.name = "\u{200E}Claude".to_string();
        let m = resolve_detailed_in(&[e], "Claude").unwrap();
        assert_eq!(m.match_type, MatchType::NameExact);
    }

    #[test]
    fn resolve_returns_none_on_total_miss() {
        let entries = vec![entry("kitty", "Kitty")];
        assert!(resolve_detailed_in(&entries, "thunderbird").is_none());
    }

    #[test]
    fn resolve_empty_entries_returns_none() {
        assert!(resolve_detailed_in(&[], "anything").is_none());
    }
}
