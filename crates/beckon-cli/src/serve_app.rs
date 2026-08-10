//! The Windows app front door: where its config and log live by default,
//! what a fresh config looks like, and what goes in the autostart value.
//!
//! Deliberately in `beckon-cli` and free of `cfg(windows)` for everything
//! but the glue at the bottom. CI excludes `beckon-windows` from the Linux
//! and macOS jobs, so logic placed there is only ever tested on one runner;
//! placed here, these tests run on all three.

use std::path::{Path, PathBuf};

/// Build the `HKCU\…\Run` value.
///
/// `config` and `log` are passed as `Some` **only when they differ from the
/// defaults**. Ticking "Start with Windows" while running against a
/// non-default config must not silently hand the user the default config at
/// next logon; omitting the defaults keeps the common value short enough to
/// read in regedit.
///
/// Called only from the Windows-only tray menu in `serve.rs`, so non-Windows
/// builds see it as unused outside its own tests -- same reasoning as
/// `ServeState::log` there.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub fn run_key_command_line(exe: &Path, config: Option<&Path>, log: Option<&Path>) -> String {
    let mut s = format!("\"{}\"", exe.display());
    if let Some(c) = config {
        s.push_str(&format!(" \"{}\"", c.display()));
    }
    if let Some(l) = log {
        s.push_str(&format!(" --log \"{}\"", l.display()));
    }
    s
}

/// Rewrite a Scoop versioned install path to the `current` junction.
///
/// Scoop lays out `…\scoop\apps\<name>\<version>\` and keeps a `current`
/// junction pointing at the active one. A Run value naming the versioned
/// directory dies at the next `scoop update`, which deletes it — and
/// because it no longer launches, it can never repair itself. Anything
/// that is not that exact shape is returned unchanged.
///
/// Splits on `/` and `\` explicitly rather than going through
/// `Path::components()`. On a Unix host `Path::components()` does not
/// split a Windows-style `C:\a\b` string at all — it is one opaque
/// component — so four of this module's six tests would pass vacuously
/// off-Windows while still looking green. Working on the raw string
/// instead means every test here genuinely exercises the logic on macOS,
/// Linux and Windows alike, not just on the one runner that happens to
/// use backslashes natively.
///
/// Called only from the Windows-only tray menu in `serve.rs`, so non-Windows
/// builds see it as unused outside its own tests -- same reasoning as
/// `ServeState::log` there.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub fn scoop_current_path(exe: &Path) -> PathBuf {
    let Some(s) = exe.to_str() else {
        return exe.to_path_buf();
    };

    // Manually split on both separators, remembering which separator sat
    // at each boundary so the path can be rebuilt byte-for-byte except for
    // the one segment being rewritten.
    let mut segments: Vec<&str> = Vec::new();
    let mut seps: Vec<char> = Vec::new();
    let mut start = 0;
    for (i, c) in s.char_indices() {
        if c == '/' || c == '\\' {
            segments.push(&s[start..i]);
            seps.push(c);
            start = i + c.len_utf8();
        }
    }
    segments.push(&s[start..]);

    // Need at least: … scoop, apps, <name>, <version>, <file>
    for i in 0..segments.len().saturating_sub(4) {
        if !segments[i].eq_ignore_ascii_case("scoop")
            || !segments[i + 1].eq_ignore_ascii_case("apps")
        {
            continue;
        }
        if segments[i + 3].eq_ignore_ascii_case("current") {
            return exe.to_path_buf();
        }
        let mut out = String::new();
        for (n, seg) in segments.iter().enumerate() {
            if n > 0 {
                out.push(seps[n - 1]);
            }
            if n == i + 3 {
                out.push_str("current");
            } else {
                out.push_str(seg);
            }
        }
        return PathBuf::from(out);
    }
    exe.to_path_buf()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn command_line_is_just_the_quoted_exe_when_everything_is_default() {
        let exe = PathBuf::from(r"C:\Program Files\beckon\beckon-serve.exe");
        assert_eq!(
            run_key_command_line(&exe, None, None),
            r#""C:\Program Files\beckon\beckon-serve.exe""#
        );
    }

    #[test]
    fn command_line_carries_a_non_default_config_and_log() {
        let exe = PathBuf::from(r"C:\bin\beckon-serve.exe");
        let cfg = PathBuf::from(r"D:\my keys.toml");
        let log = PathBuf::from(r"D:\logs\b.log");
        assert_eq!(
            run_key_command_line(&exe, Some(&cfg), Some(&log)),
            r#""C:\bin\beckon-serve.exe" "D:\my keys.toml" --log "D:\logs\b.log""#
        );
    }

    #[test]
    fn scoop_versioned_path_is_rewritten_to_current() {
        let p = PathBuf::from(r"C:\Users\me\scoop\apps\beckon\0.7.0\beckon-serve.exe");
        assert_eq!(
            scoop_current_path(&p),
            PathBuf::from(r"C:\Users\me\scoop\apps\beckon\current\beckon-serve.exe")
        );
    }

    #[test]
    fn scoop_current_path_is_left_alone() {
        let p = PathBuf::from(r"C:\Users\me\scoop\apps\beckon\current\beckon-serve.exe");
        assert_eq!(scoop_current_path(&p), p);
    }

    #[test]
    fn a_path_merely_containing_the_word_scoop_is_untouched() {
        let p = PathBuf::from(r"C:\scoop-backups\beckon\0.7.0\beckon-serve.exe");
        assert_eq!(scoop_current_path(&p), p);
    }

    #[test]
    fn non_scoop_paths_are_untouched() {
        let p = PathBuf::from(r"C:\Program Files\beckon\beckon-serve.exe");
        assert_eq!(scoop_current_path(&p), p);
    }
}
