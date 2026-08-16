//! What does the settings window's catalog actually contain?
//!
//! ```text
//! cargo run -p beckon-macos --example catalog_probe -- Finder kitty
//! ```
//!
//! `row_condition` prints `missing` beside any binding whose app is not in
//! this list, so a name that `beckon resolve` finds and this list does not is
//! the window calling a working binding broken. That happened: `Finder` lives
//! in `/System/Library/CoreServices`, which the installed scan does not walk,
//! while `resolve` matched it as a running app.
//!
//! Needs no Aqua session and no permission — it reads `NSWorkspace` and the
//! application directories, nothing more.

fn main() {
    #[cfg(not(target_os = "macos"))]
    {
        eprintln!("catalog_probe is macOS-only");
        std::process::exit(2);
    }
    #[cfg(target_os = "macos")]
    {
        let names = beckon_macos::installed_app_names();
        println!("catalog: {} names", names.len());
        let mut missing = 0;
        for want in std::env::args().skip(1) {
            let hit = names.iter().any(|n| n.eq_ignore_ascii_case(&want));
            println!("  {:<24} {}", want, if hit { "present" } else { "MISSING" });
            if !hit {
                missing += 1;
            }
        }
        std::process::exit(if missing == 0 { 0 } else { 1 });
    }
}
