//! Does a Carbon hotkey still fire under `[NSApp run]`?
//!
//! `hotkey::run_forever` stopped calling `RunApplicationEventLoop` on
//! 2026-08-16, because under it `NSApp` never runs and every control in the
//! settings window was decoration (see `loop_probe.rs`). That change is only
//! safe if the OTHER thing `serve` does still works, and hotkeys are the
//! whole feature. `run_forever`'s doc says the argument in principle —
//! `RegisterEventHotKey` installs on `GetApplicationEventTarget()` and
//! `[NSApp run]` pumps the same queue, which is the ordinary Cocoa
//! configuration rather than a clever one — and "in principle" is exactly
//! the kind of sentence this branch has had to retract before.
//!
//! ```text
//! cargo run -p beckon-macos --example hotkey_loop_probe -- nsapp
//! cargo run -p beckon-macos --example hotkey_loop_probe -- carbon
//! ```
//!
//! ## The control is the `carbon` run, and it is not optional
//!
//! Hotkeys demonstrably worked before the change — `serve` shipped and is in
//! daily use — so the Carbon run is a **known-good baseline for the whole
//! chain**: the chord, the injector, the key table, the registration. Read
//! the pair, never one alone:
//!
//! | carbon | nsapp | reading |
//! |---|---|---|
//! | FIRED | FIRED | the loop change is safe |
//! | FIRED | silent | **regression — revert `run_forever`** |
//! | silent | silent | the injector never landed; measures nothing |
//! | silent | FIRED | the baseline is wrong; re-read everything |
//!
//! Without the third row spelled out, a silent `nsapp` run reads as a
//! regression when it may only mean the keystroke never arrived — the
//! blind-detector trap, which on this branch has now cost four attempts
//! across two probes.
//!
//! ## The chord
//!
//! `ctrl+alt+shift+f`, copied from `hotkey_smoke.rs` and for its stated
//! reason: it deliberately omits Cmd, so it sits outside the hyper layer
//! kanata and Hammerspoon own on this user's machines. A probe that fought
//! the user's own remapper would produce a silent run and a wrong conclusion.

fn main() {
    #[cfg(not(target_os = "macos"))]
    {
        eprintln!("hotkey_loop_probe is macOS-only");
        std::process::exit(2);
    }
    #[cfg(target_os = "macos")]
    mac::run();
}

#[cfg(target_os = "macos")]
mod mac {
    use std::io::Write;

    fn say(line: &str) {
        println!("{line}");
        let _ = std::io::stdout().flush();
    }

    pub fn run() {
        let manager = std::process::Command::new("launchctl")
            .arg("managername")
            .output()
            .ok()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .unwrap_or_default();
        say(&format!("bootstrap namespace : {manager}"));
        if manager != "Aqua" {
            say("REFUSING: not an Aqua session. `hotkey.rs` records that");
            say("RegisterEventHotKey can return success in a process with no window-");
            say("server identity and never deliver, so a silent run here would be");
            say("about the wrong program.");
            std::process::exit(3);
        }

        let mode = std::env::args().nth(1).unwrap_or_default();
        if mode != "carbon" && mode != "nsapp" {
            say("usage: hotkey_loop_probe <carbon|nsapp>");
            std::process::exit(2);
        }
        say(&format!("MODE: {mode}"));

        let mut mgr = match beckon_macos::hotkey::HotkeyManager::install(Box::new(|id| {
            say(&format!("HOTKEY FIRED: id={id}"));
            say("VERDICT: Carbon hotkeys DO deliver under this loop");
            // Leave at once, so a hung exit is distinguishable from a loop
            // that simply never delivered.
            std::process::exit(0);
        })) {
            Ok(m) => m,
            Err(e) => {
                say(&format!("install failed: {e}"));
                std::process::exit(1);
            }
        };

        let f = beckon_core::shortcuts::lookup_key("f").expect("key table has f");
        if let Err(e) = mgr.register(0, true, false, true, true, f) {
            say(&format!("register failed: {e}"));
            std::process::exit(1);
        }
        // `TransformProcessType(→ UIElement)` has already run inside
        // `install`, so this process is in the same state `serve` is when it
        // listens. That matters: `hotkey.rs` documents that a process with no
        // window-server identity can register successfully and never fire,
        // and the transform is what gives it one.
        say("READY: press ctrl+alt+shift+f");

        let mut n = 0u32;
        beckon_macos::hotkey::add_tick(
            1.0,
            Box::new(move || {
                n += 1;
                say(&format!("HEARTBEAT {n}"));
                if n >= 20 {
                    say("NOT-FIRED: no press arrived in 20s");
                    say("VERDICT: read this against the OTHER mode -- alone it cannot");
                    say("tell a dead loop from a keystroke that never landed.");
                    std::process::exit(4);
                }
            }),
        );

        match mode.as_str() {
            "carbon" => beckon_macos::hotkey::HotkeyManager::run_carbon_event_loop_for_probe(),
            _ => beckon_macos::hotkey::HotkeyManager::run_forever(),
        }
    }
}
