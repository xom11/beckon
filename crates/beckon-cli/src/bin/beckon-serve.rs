//! `beckon serve` with no console: the Windows tray app.
//!
//! Only the subsystem attribute and a call into the library live here. The
//! attribute is the entire reason this binary exists — the subsystem is a
//! bit in the PE header, not a runtime switch, so `beckon.exe` cannot be
//! both this and a working CLI. Flipping the whole binary would break
//! `list`, `installed`, `search`, `resolve` and `doctor`, whose output the
//! shell prints after returning its prompt.
#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

#[cfg(target_os = "windows")]
fn main() {
    beckon_cli::serve_app_main();
}

// Cargo cannot gate a [[bin]] on target_os, so this target still builds on
// Linux and macOS. It is never packaged there: the release workflow's unix
// step copies `beckon` by name and nothing else.
#[cfg(not(target_os = "windows"))]
fn main() {
    eprintln!("beckon-serve is Windows-only; use `beckon serve <CONFIG>` instead");
    std::process::exit(1);
}
