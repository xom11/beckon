//! Manual smoke: registers ctrl+alt+shift+f, prints each press, exits after
//! 15 s. Run in a real desktop session on the Mac:
//!     cargo run -p beckon-macos --example hotkey_smoke
//! The cfg gate keeps `--all-targets` builds green on Linux/Windows CI.
#[cfg(target_os = "macos")]
fn main() {
    let mut mgr = beckon_macos::hotkey::HotkeyManager::install(Box::new(|id| {
        println!("hotkey pressed: id={id}");
    }))
    .expect("install handler");
    // f = kVK_ANSI_F = 0x03. ctrl+alt+shift, NO cmd — deliberately outside
    // the hyper layer Hammerspoon/kanata currently own.
    mgr.register(0, true, false, true, true, 0x03)
        .expect("register");
    println!("press ctrl+alt+shift+f — exiting in 15 s");
    beckon_macos::hotkey::add_tick(15.0, Box::new(|| std::process::exit(0)));
    beckon_macos::hotkey::HotkeyManager::run_forever();
}

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("hotkey_smoke is macOS-only");
}
