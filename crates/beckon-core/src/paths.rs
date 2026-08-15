//! The spelling a path is shown in, and handed to the shell in.
//!
//! One job: undo Windows' **extended-length prefix**. `Path::canonicalize` on
//! Windows is `GetFinalPathNameByHandleW`, which always returns the verbatim
//! form — `\\?\C:\Users\kln\.config\beckon\apps.toml` — and beckon canonicalises
//! the `serve` config path once, at `serve.rs`'s `cmd_serve_app`. Everything
//! downstream inherits that spelling: the startup log line, the `Open config
//! file` tooltip, the System page's config row, and the two shell calls behind
//! that row's glyphs.
//!
//! **Why this is not a paint-time fix.** The prefix reached the screen through
//! four hops from one origin; stripping it at the painter leaves the log, the
//! tooltip and `ShellExecuteW` still carrying it, and puts the next person who
//! prints a path back where this started.
//!
//! **It is not only cosmetic.** `ShellExecuteW` and `explorer.exe /select,` are
//! the classic non-acceptors of `\\?\` — the shell resolves display names, not
//! raw NT paths — so the System page's two glyph buttons were handing the shell
//! a spelling it is documented not to take.
//!
//! **Long paths still work.** `crates/beckon-cli/beckon.exe.manifest` declares
//! `longPathAware`, so a plain `C:\...` over 260 characters is served by the
//! same Win32 calls that served the verbatim form. Dropping the prefix costs
//! nothing there.
//!
//! Everything below is `&str` arithmetic on purpose: `std::path`'s component
//! rules are the HOST's, so a `\`-separated test would parse as one component
//! on macOS and this file would be untestable on two of the three CI jobs.

use std::path::PathBuf;

/// The extended-length prefix itself.
const VERBATIM: &str = r"\\?\";
/// Its UNC form. `\\?\UNC\server\share` is `\\server\share`.
const VERBATIM_UNC: &str = r"\\?\UNC\";

/// Names the Win32 layer still resolves as DOS devices rather than as files,
/// in any directory. A path containing one is reachable ONLY through the
/// verbatim form, so simplifying it would change which object it names.
const DEVICES: [&str; 22] = [
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// The plain spelling of `p`, or `p` unchanged when it has none.
///
/// Total and lossless in the direction that matters: a path this refuses to
/// simplify keeps working, because the verbatim form is what every Win32 call
/// accepts. The reverse is not true, which is why `simplified` is conservative.
pub fn plain(p: PathBuf) -> PathBuf {
    match p.to_str().and_then(simplified) {
        Some(s) => PathBuf::from(s),
        // Two arms land here and both must: a path that is already plain, and
        // a path whose bytes are not UTF-8 (`to_str` is `None`). Neither is a
        // failure, so neither reports one.
        None => p,
    }
}

/// `None` when `s` is already plain, or when its only spelling is verbatim.
///
/// Split out from `plain` so the decision is `&str -> Option<String>` and can
/// be tested with literals on a host whose path separator is `/`.
fn simplified(s: &str) -> Option<String> {
    // Order matters: the UNC prefix starts with the plain one, so testing the
    // plain one first would take `\\?\UNC\srv\share` down the drive arm, find
    // `UNC` where a drive letter belongs, and return `None` — the right answer
    // by accident, and one that stops being right if the drive test is ever
    // loosened.
    let candidate = if let Some(rest) = s.strip_prefix(VERBATIM_UNC) {
        // `\\` + `server\share\...`. A UNC path with no share is not a path.
        if rest.split('\\').take(2).filter(|c| !c.is_empty()).count() < 2 {
            return None;
        }
        format!(r"\\{rest}")
    } else {
        // `?` rather than a third `else`: a path with no verbatim prefix at
        // all is already plain, which is the same answer as "no plain
        // spelling" to every caller and is why both are `None`.
        let rest = s.strip_prefix(VERBATIM)?;
        // A drive path and nothing else: `C:\`. `\\?\Volume{...}` and the
        // device namespace have no plain spelling at all, so they keep theirs.
        let mut c = rest.chars();
        match (c.next(), c.next(), c.next()) {
            (Some(d), Some(':'), Some('\\')) if d.is_ascii_alphabetic() => rest.to_string(),
            _ => return None,
        }
    };

    // The verbatim form is also the only way to NAME certain files, because it
    // is the one form Win32 passes through without rewriting. If any component
    // needs that rewrite suppressed, the prefix is load-bearing and stays.
    if candidate.split('\\').any(needs_verbatim) {
        return None;
    }
    Some(candidate)
}

/// Whether a single path component can only be reached verbatim.
fn needs_verbatim(component: &str) -> bool {
    if component.is_empty() {
        // The two leading empties of `\\server\share` and nothing else: a
        // doubled separator inside a path would already have been collapsed by
        // `canonicalize`, which is this module's only caller.
        return false;
    }
    // Win32 strips trailing dots and spaces from every component, so
    // `C:\a. \b` and `C:\a\b` name the same file once the prefix is gone.
    if component.ends_with('.') || component.ends_with(' ') {
        return true;
    }
    // `.` and `..` are resolved by Win32 and taken literally by the verbatim
    // form. `canonicalize` does not emit them; a caller that hand-built a
    // verbatim path might.
    if component == "." || component == ".." {
        return true;
    }
    // A device name is a device wherever it appears, with or without an
    // extension: `C:\dir\NUL.txt` is the null device.
    let stem = component.split('.').next().unwrap_or(component);
    DEVICES.iter().any(|d| stem.eq_ignore_ascii_case(d))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_drive_path_loses_the_prefix() {
        assert_eq!(
            simplified(r"\\?\C:\Users\kln\.config\beckon\apps.toml").as_deref(),
            Some(r"C:\Users\kln\.config\beckon\apps.toml"),
        );
    }

    #[test]
    fn the_bare_root_survives_the_drive_test() {
        assert_eq!(simplified(r"\\?\C:\").as_deref(), Some(r"C:\"));
    }

    #[test]
    fn a_lowercase_drive_letter_is_still_a_drive_letter() {
        assert_eq!(simplified(r"\\?\d:\tmp").as_deref(), Some(r"d:\tmp"));
    }

    #[test]
    fn a_unc_path_becomes_a_double_backslash() {
        assert_eq!(
            simplified(r"\\?\UNC\server\share\beckon\apps.toml").as_deref(),
            Some(r"\\server\share\beckon\apps.toml"),
        );
    }

    /// The UNC arm must be tried first. Were it not, `UNC` would be read as a
    /// drive letter candidate and the answer would be right for the wrong
    /// reason -- so this asserts the OUTPUT, which only the UNC arm produces.
    #[test]
    fn the_unc_arm_wins_over_the_drive_arm() {
        assert!(simplified(r"\\?\UNC\srv\pub\x").is_some_and(|s| s.starts_with(r"\\srv\pub")));
    }

    #[test]
    fn a_unc_prefix_with_no_share_is_refused() {
        assert_eq!(simplified(r"\\?\UNC\server"), None);
        assert_eq!(simplified(r"\\?\UNC\"), None);
    }

    /// A volume GUID path has no drive letter, so there is no plain spelling
    /// to fall back to and the prefix must stay.
    #[test]
    fn a_volume_guid_path_keeps_its_prefix() {
        assert_eq!(
            simplified(r"\\?\Volume{b75e2c83-0000-0000-0000-602f00000000}\beckon"),
            None,
        );
    }

    #[test]
    fn an_already_plain_path_is_left_alone() {
        assert_eq!(simplified(r"C:\Users\kln\apps.toml"), None);
        assert_eq!(simplified(r"\\server\share\apps.toml"), None);
        assert_eq!(simplified("/home/kln/apps.toml"), None);
    }

    /// The three cases where the prefix is doing work rather than decorating.
    /// Strip it and Win32 rewrites the component, so the path names a
    /// different object -- or a device rather than a file.
    #[test]
    fn a_component_that_needs_the_prefix_keeps_it() {
        assert_eq!(simplified(r"\\?\C:\weird.\apps.toml"), None);
        assert_eq!(simplified(r"\\?\C:\weird \apps.toml"), None);
        assert_eq!(simplified(r"\\?\C:\dir\NUL.txt"), None);
        assert_eq!(simplified(r"\\?\C:\dir\con\apps.toml"), None);
        assert_eq!(simplified(r"\\?\C:\dir\..\apps.toml"), None);
    }

    /// `needs_verbatim` matches a device name only when it IS the component's
    /// stem -- a file merely starting with those letters is an ordinary file,
    /// and refusing it would leave prefixes on paths that do not need them.
    #[test]
    fn a_name_that_merely_starts_like_a_device_is_ordinary() {
        assert_eq!(
            simplified(r"\\?\C:\dir\console\apps.toml").as_deref(),
            Some(r"C:\dir\console\apps.toml"),
        );
        assert_eq!(
            simplified(r"\\?\C:\dir\com10\apps.toml").as_deref(),
            Some(r"C:\dir\com10\apps.toml"),
        );
    }

    /// `plain` is total: everything `simplified` refuses comes back unchanged
    /// rather than empty, because the verbatim form is a WORKING path and the
    /// alternative to simplifying it is leaving it be.
    #[test]
    fn plain_returns_the_input_when_it_cannot_simplify() {
        let p = PathBuf::from(r"\\?\Volume{0}\beckon");
        assert_eq!(plain(p.clone()), p);
        let q = PathBuf::from("/home/kln/apps.toml");
        assert_eq!(plain(q.clone()), q);
    }

    #[test]
    fn plain_applies_the_simplification_it_finds() {
        assert_eq!(
            plain(PathBuf::from(r"\\?\C:\a\b.toml")),
            PathBuf::from(r"C:\a\b.toml"),
        );
    }
}
