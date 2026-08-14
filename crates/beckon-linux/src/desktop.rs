//! Minimal .desktop file parser. Hand-rolled to avoid pulling in a crate
//! for ~80 lines of work. Reads the `[Desktop Entry]` section only and
//! pulls Name / Exec / StartupWMClass.
//!
//! Field-code stripping in Exec follows the XDG Desktop Entry Spec:
//! `%f %F %u %U %d %D %n %N %i %c %k %v %m` are removed; `%%` becomes `%`.

use beckon_core::certainty::{Certainty, NameReport};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

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
        collect_dir(&dir, &dir, &mut by_id);
    }

    // Sorted, not HashMap order. `resolve_detailed_in`'s first three tiers
    // take the first entry that matches, so an unordered vector makes the
    // winner depend on HashMap's per-process random seed: with two entries
    // sharing a `Name=` (deb + snap Firefox, a user override under a new
    // filename, two PWAs with the same display name) the same keypress
    // resolves differently from one run to the next, and beckon alternates
    // between focusing the window and launching a second copy. Sorting by
    // id gives every tier the same "alphabetically first .desktop id wins"
    // rule the substring tier already documents.
    let mut out: Vec<DesktopEntry> = by_id.into_values().collect();
    out.sort_by(|a, b| a.id.cmp(&b.id));
    out
}

/// Walk one `applications/` root. The XDG menu spec builds the *desktop
/// file id* from the path relative to that root with `/` replaced by `-`,
/// so `applications/kde4/konsole.desktop` is `kde4-konsole`, not `konsole`.
/// Wine (`applications/wine/Programs/…`) and KDE install this way, and a
/// flat `read_dir` misses them entirely.
fn collect_dir(root: &Path, dir: &Path, by_id: &mut HashMap<String, DesktopEntry>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    // Sort so a directory that somehow yields two entries with the same id
    // resolves the same way on every run.
    let mut paths: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
    paths.sort();
    for path in paths {
        if path.is_dir() {
            // Don't follow symlinked directories: `applications/foo -> /`
            // would walk the whole filesystem.
            if path
                .symlink_metadata()
                .map(|m| m.is_symlink())
                .unwrap_or(true)
            {
                continue;
            }
            collect_dir(root, &path, by_id);
            continue;
        }
        if path.extension().and_then(|e| e.to_str()) != Some("desktop") {
            continue;
        }
        let Some(id) = desktop_file_id(root, &path) else {
            continue;
        };
        if let Some(d) = parse(&path, &id) {
            if d.no_display {
                continue;
            }
            by_id.insert(d.id.clone(), d);
        }
    }
}

/// `<root>/kde4/konsole.desktop` → `kde4-konsole`.
fn desktop_file_id(root: &Path, path: &Path) -> Option<String> {
    let rel = path.strip_prefix(root).ok()?;
    let stem = rel.with_extension("");
    let s = stem.to_str()?;
    Some(s.replace(std::path::MAIN_SEPARATOR, "-"))
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
///      Useful when the user copy-pastes a runtime app_id from `beckon list`.
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
/// `beckon resolve` debug command explain its reasoning.
pub fn resolve_detailed(id: &str) -> Option<ResolvedMatch> {
    resolve_detailed_in(&scan(), id)
}

/// Pure resolution against a caller-supplied entry list. Lets tests cover
/// the priority ladder without touching the filesystem.
pub fn resolve_detailed_in(entries: &[DesktopEntry], id: &str) -> Option<ResolvedMatch> {
    let needle = normalize(id);
    // An empty needle is a substring of every Name, so without this guard
    // tier 4 would resolve `beckon ""` (an unset `$APP` in a dotfile) to
    // the alphabetically first installed app and launch it.
    if needle.is_empty() {
        return None;
    }

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

/// Every window class that should count as "the app the user asked for".
///
/// A resolved `.desktop` entry gives us two independent strings, and which
/// one the running window advertises depends on the client:
///   - Wayland-native clients report the `.desktop` filename stem as their
///     `app_id` — that is `entry.id`;
///   - the same app under X11 / XWayland reports its `WM_CLASS`, which is
///     exactly what `StartupWMClass=` records. `debian-xterm.desktop` is
///     the extreme case: stem `debian-xterm`, `WM_CLASS` `XTerm`.
///
/// Matching on `entry.id` alone means an X11 app is never recognised as
/// running, so every keypress launches another copy. When nothing resolved,
/// the raw id is the only candidate — that is what lets beckon focus ad-hoc
/// apps that have no `.desktop` file at all.
pub fn target_classes(entry: Option<&DesktopEntry>, raw_id: &str) -> crate::algorithm::Target {
    match entry {
        Some(e) => crate::algorithm::Target::new(
            [Some(e.id.clone()), e.startup_wm_class.clone()]
                .into_iter()
                .flatten(),
        ),
        None => crate::algorithm::Target::new([raw_id]),
    }
}

/// All entries whose Name contains `id` as a case-insensitive substring,
/// sorted alphabetically by `.desktop` filename. Used by `beckon resolve`
/// to flag ambiguity (multiple substring matches) and to suggest "did you
/// mean".
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

fn parse(path: &PathBuf, id: &str) -> Option<DesktopEntry> {
    let content = fs::read_to_string(path).ok()?;
    parse_str(&content, id)
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

/// Per the XDG basedir spec, an environment variable that is set but empty
/// "should be considered unset" — and a relative path is invalid and must
/// be ignored. Both matter here: with `XDG_DATA_HOME=` exported, taking the
/// value literally makes beckon read `./applications/*.desktop` relative to
/// whatever directory it was invoked from, so a `.desktop` file dropped in
/// any directory the user happens to `cd` into becomes a launch target —
/// while their real `~/.local/share/applications` overrides are ignored.
fn absolute_or_none(value: std::ffi::OsString) -> Option<PathBuf> {
    let path = PathBuf::from(value);
    if path.as_os_str().is_empty() || !path.is_absolute() {
        return None;
    }
    Some(path)
}

fn user_app_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    let xdg_data_home = std::env::var_os("XDG_DATA_HOME")
        .and_then(absolute_or_none)
        .or_else(|| {
            std::env::var_os("HOME")
                .and_then(absolute_or_none)
                .map(|h| h.join(".local/share"))
        });
    if let Some(d) = xdg_data_home {
        dirs.push(d.join("applications"));
    }
    dirs
}

fn system_app_dirs() -> Vec<PathBuf> {
    const DEFAULT: &str = "/usr/local/share:/usr/share";
    let raw = std::env::var("XDG_DATA_DIRS").unwrap_or_default();
    let raw = if raw.trim().is_empty() { DEFAULT } else { &raw };
    raw.split(':')
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .map(|d| d.join("applications"))
        .collect()
}

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

    // ---------- resolution edge cases ----------

    /// An empty needle is a substring of every Name. Before the guard,
    /// `beckon "$APP"` with `$APP` unset launched the alphabetically-first
    /// installed app.
    #[test]
    fn empty_id_resolves_to_nothing() {
        let entries = vec![entry("alacritty", "Alacritty"), entry("kitty", "Kitty")];
        assert!(resolve_detailed_in(&entries, "").is_none());
        assert!(resolve_detailed_in(&entries, "   ").is_none());
    }

    /// `scan()` sorts by id, so ties in the Name-exact tier resolve to the
    /// alphabetically-first `.desktop` id — the same rule the substring
    /// tier already documents. Without the sort this depended on HashMap's
    /// per-process random seed and flipped between runs.
    #[test]
    fn name_tie_resolves_to_alphabetically_first_id() {
        let entries = vec![
            entry("firefox", "Firefox"),
            entry("firefox_firefox", "Firefox"),
        ];
        let m = resolve_detailed_in(&entries, "Firefox").expect("resolves");
        assert_eq!(m.entry.id, "firefox");
        assert_eq!(m.match_type, MatchType::NameExact);
    }

    /// XDG menu spec: the desktop file id is the path relative to the
    /// `applications/` root with `/` replaced by `-`. Wine and KDE install
    /// into subdirectories, and a flat scan misses them entirely.
    #[test]
    fn desktop_file_id_joins_subdirectories_with_dashes() {
        let root = PathBuf::from("/usr/share/applications");
        assert_eq!(
            desktop_file_id(&root, &root.join("kitty.desktop")).unwrap(),
            "kitty"
        );
        assert_eq!(
            desktop_file_id(&root, &root.join("kde4/konsole.desktop")).unwrap(),
            "kde4-konsole"
        );
        assert_eq!(
            desktop_file_id(&root, &root.join("wine/Programs/Acme/App.desktop")).unwrap(),
            "wine-Programs-Acme-App"
        );
    }

    // ---------- target candidates ----------

    #[test]
    fn target_classes_covers_stem_and_startup_wm_class() {
        let e = entry_with_wm("debian-xterm", "XTerm", "XTerm");
        let t = target_classes(Some(&e), "XTerm");
        assert!(t.matches("debian-xterm"), "Wayland app_id candidate");
        assert!(t.matches("XTerm"), "X11 WM_CLASS candidate");
        assert!(t.matches("xterm"), "case-insensitive");
        assert!(!t.matches("kitty"));
    }

    /// With no `.desktop` entry the raw id is the only candidate — that is
    /// what lets beckon focus ad-hoc apps that ship no desktop file.
    #[test]
    fn target_classes_falls_back_to_raw_id() {
        let t = target_classes(None, "my-adhoc-app");
        assert!(t.matches("my-adhoc-app"));
        assert_eq!(t.primary(), "my-adhoc-app");
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
        assert_eq!(r.suggestions, vec!["Brave Web Browser Beta".to_string()]);
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
}
