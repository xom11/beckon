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

/// `%USERPROFILE%\.config\beckon\apps.toml`.
///
/// `.config` rather than `%APPDATA%`: it is the path the README already
/// tells Windows users to create, and it is the path macOS uses. The
/// shortcuts file is designed to validate on every platform, so one
/// location across all three beats one platform's idiom.
///
/// Called only from the Windows-only `mod app` below, so non-Windows builds
/// see it as unused outside its own tests -- same reasoning as
/// `run_key_command_line` above.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub fn default_config_path(home: &Path) -> PathBuf {
    home.join(".config").join("beckon").join("apps.toml")
}

/// `%LOCALAPPDATA%\beckon\serve.log` — the path the Scheduled Task example
/// already uses, so an existing install's log does not move.
///
/// Called only from the Windows-only `mod app` below, so non-Windows builds
/// see it as unused outside its own tests -- same reasoning as
/// `default_config_path` above.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub fn default_log_path(local_appdata: &Path) -> PathBuf {
    local_appdata.join("beckon").join("serve.log")
}

/// The file a brand-new user gets. Every binding here must parse, because
/// this is the first thing beckon ever shows them.
///
/// ASCII only: this text can be echoed into the log, and Windows
/// PowerShell 5.1's Get-Content defaults to ANSI.
///
/// Called only from the Windows-only `mod app` below, so non-Windows builds
/// see it as unused outside its own tests -- same reasoning as
/// `default_config_path` above.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub fn starter_template() -> &'static str {
    r#"# beckon shortcuts. Edit and save -- beckon reloads automatically.
#
#   "<modifiers>+<key>" = "<app Name>"
#
# Modifiers: ctrl, super (the Windows key), alt, shift -- any order.
# Keys are lowercase: a-z, 0-9, f1-f20, and names like space, comma, pageup.
#
# Find the Name to use on the right-hand side with:
#   beckon installed
#   beckon search <part of the name>
#
# Check a file without starting anything:
#   beckon check "%USERPROFILE%\.config\beckon\apps.toml"

"ctrl+super+alt+t" = "Terminal"
"ctrl+super+alt+e" = "File Explorer"
"#
}

/// Create `path` with the starter template if it is not there.
///
/// Returns `true` when it created the file. Never overwrites: a user whose
/// config exists must keep it, whatever else goes wrong.
///
/// Called only from the Windows-only `mod app` below, so non-Windows builds
/// see it as unused outside its own tests -- same reasoning as
/// `default_config_path` above.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub fn ensure_config(path: &Path) -> std::io::Result<bool> {
    if path.exists() {
        return Ok(false);
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, starter_template())?;
    Ok(true)
}

#[cfg(target_os = "windows")]
mod app {
    use super::*;
    use clap::Parser;

    /// `beckon-serve.exe [CONFIG] [--log PATH]` — the same two operands
    /// `beckon serve` takes, both optional so that a double-click and a bare
    /// Run-key value are the normal invocation.
    #[derive(Parser, Debug)]
    #[command(
        name = "beckon-serve",
        version,
        about = "beckon resident hotkey service (tray app)"
    )]
    struct ServeAppArgs {
        #[arg(value_name = "CONFIG")]
        config: Option<PathBuf>,

        /// Send stderr to PATH instead of the default log.
        #[arg(long, value_name = "PATH")]
        log: Option<PathBuf>,
    }

    fn die(body: &str) -> ! {
        beckon_windows::shell::error_dialog("beckon serve", body);
        std::process::exit(1);
    }

    pub fn main() {
        // clap's own `Parser::parse()` prints to stderr and exits internally
        // on `--help`, `--version`, or a usage error -- fine for `beckon.exe`,
        // which is console-subsystem and has a terminal to print to. This
        // process has no console at any point, so that write would reach
        // nobody and the app would vanish with zero visible feedback -- the
        // exact failure mode this binary exists to eliminate. `try_parse`
        // lets us route both outcomes through a dialog instead.
        let args = match ServeAppArgs::try_parse() {
            Ok(a) => a,
            Err(e) => {
                let body = e.to_string();
                let informational = matches!(
                    e.kind(),
                    clap::error::ErrorKind::DisplayHelp
                        | clap::error::ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand
                        | clap::error::ErrorKind::DisplayVersion
                );
                if informational {
                    beckon_windows::shell::info_dialog("beckon serve", &body);
                    std::process::exit(0);
                } else {
                    beckon_windows::shell::error_dialog("beckon serve", &body);
                    std::process::exit(2);
                }
            }
        };

        // 1. The log, before anything can print. Every eprintln! in this
        //    process lands in the file after this returns; before it, there
        //    is nowhere for one to go.
        let log_default = std::env::var_os("LOCALAPPDATA")
            .map(|p| default_log_path(Path::new(&p)))
            .unwrap_or_else(|| PathBuf::from("beckon-serve.log"));
        let log = args.log.clone().unwrap_or(log_default.clone());
        if let Err(e) = beckon_windows::logfile::redirect_to_log(&log) {
            die(&format!(
                "Cannot open the log file:\n{}\n\n{e:#}",
                log.display()
            ));
        }

        // 2. The config, created on first run so a double-click works with
        //    nothing read beforehand.
        let cfg_default = std::env::var_os("USERPROFILE")
            .map(|p| default_config_path(Path::new(&p)))
            .unwrap_or_else(|| PathBuf::from("apps.toml"));
        let config = args.config.clone().unwrap_or(cfg_default.clone());
        match ensure_config(&config) {
            Err(e) => {
                eprintln!("beckon serve: cannot create {}: {e}", config.display());
                die(&format!(
                    "Cannot create the config file:\n{}\n\n{e}",
                    config.display()
                ))
            }
            Ok(true) => {
                eprintln!("beckon serve: created {}", config.display());
                if let Err(e) = beckon_windows::shell::open_path(&config) {
                    eprintln!("beckon serve: {e}");
                }
            }
            Ok(false) => {}
        }

        // 3. Only non-default values go into the autostart command line.
        // `Some` here (as opposed to `cmd_serve`'s `None`) is what tells
        // the tray menu this process's own exe is a valid autostart
        // target -- see `AutostartCapability`.
        let autostart = Some(crate::serve::AutostartCapability {
            config: (config != cfg_default).then(|| config.clone()),
            log: (log != log_default).then(|| log.clone()),
        });

        if let Err(e) = crate::serve::cmd_serve_app(&config, Some(log), autostart) {
            eprintln!("beckon serve: {e:#}");
            // The lock refusal is a designed outcome, not a fault -- but with
            // no console the user needs telling, or a double-click looks like
            // it did nothing at all.
            die(&format!("{e:#}"));
        }
        // cmd_serve_app only returns on error; run_forever exits the process.
    }
}

/// Entry point for `beckon-serve.exe`. Never returns normally.
#[cfg(target_os = "windows")]
pub fn serve_app_main() {
    app::main()
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

    #[test]
    fn default_paths_sit_where_the_readme_says() {
        assert_eq!(
            default_config_path(Path::new(r"C:\Users\me")),
            PathBuf::from(r"C:\Users\me")
                .join(".config")
                .join("beckon")
                .join("apps.toml")
        );
        assert_eq!(
            default_log_path(Path::new(r"C:\Users\me\AppData\Local")),
            PathBuf::from(r"C:\Users\me\AppData\Local")
                .join("beckon")
                .join("serve.log")
        );
    }

    #[test]
    fn the_starter_template_is_a_valid_shortcuts_file() {
        let parsed = beckon_core::shortcuts::parse_shortcuts(starter_template())
            .expect("the very first file a new user sees must not fail validation");
        assert!(
            !parsed.is_empty(),
            "an empty template teaches nothing and registers nothing"
        );
    }

    #[test]
    fn ensure_config_creates_once_then_leaves_it_alone() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = dir.path().join("nested").join("apps.toml");

        assert!(ensure_config(&cfg).unwrap(), "first call must create it");
        assert!(cfg.exists());
        std::fs::write(&cfg, "\"ctrl+alt+z\" = \"Zed\"\n").unwrap();

        assert!(!ensure_config(&cfg).unwrap(), "second call must not create");
        assert_eq!(
            std::fs::read_to_string(&cfg).unwrap(),
            "\"ctrl+alt+z\" = \"Zed\"\n",
            "an existing config must never be overwritten"
        );
    }
}
