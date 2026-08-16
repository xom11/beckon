//! Does `Caps+T` reach a hotkey bound to `ctrl+cmd+opt+T`?
//!
//! The end-to-end test of the whole alias, and the macOS twin of the Windows
//! probe of the same name — which is described there as *"run once without
//! `serve` and once with it; the difference is the result"*. Same shape:
//!
//! ```text
//! cargo run -p beckon-macos --example caps_live -- off   # control
//! cargo run -p beckon-macos --example caps_live -- on    # the tap installed
//! ```
//!
//! | off | on | reading |
//! |---|---|---|
//! | silent | FIRED | the alias works, and nothing else was doing it |
//! | silent | silent | the alias does not work |
//! | **FIRED** | * | **something else on this machine already maps Caps** — the result says nothing about beckon |
//!
//! The third row is not hypothetical here. This machine runs kanata, and
//! `~/.nix/configs/kanata/main.kbd` maps `caps` to
//! `tap-hold 200 200 esc (multi lmet lctl lalt)` — Caps held IS
//! Cmd+Ctrl+Option, which is beckon's own hold chord. With kanata running,
//! the `off` run fires and the probe is measuring kanata. Stop
//! `org.nixos.kanata` first.
//!
//! ## Why the injection is in-process
//!
//! The probe posts its own `Caps`, `T`, `Caps` through `CGEventPost`, which
//! is the same path a real key takes. `caps_tap` marks only its OWN burst as
//! injected, so the probe's events are seen as user input — which is the
//! point. Nothing here needs a second process or a person.

fn main() {
    #[cfg(not(target_os = "macos"))]
    {
        eprintln!("caps_live is macOS-only");
        std::process::exit(2);
    }
    #[cfg(target_os = "macos")]
    mac::run();
}

#[cfg(target_os = "macos")]
mod mac {
    use beckon_core::shortcuts::{CapsTap, Chord};
    use std::collections::HashSet;
    use std::ffi::c_void;
    use std::io::Write;
    use std::sync::atomic::{AtomicBool, Ordering};

    fn say(l: &str) {
        println!("{l}");
        let _ = std::io::stdout().flush();
    }

    #[link(name = "CoreGraphics", kind = "framework")]
    unsafe extern "C" {
        fn CGEventCreateKeyboardEvent(
            source: *const c_void,
            virtual_key: u16,
            key_down: bool,
        ) -> *mut c_void;
        fn CGEventPost(tap: u32, event: *mut c_void);
    }
    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CFRelease(cf: *const c_void);
    }

    const SESSION_TAP: u32 = 1;
    const K_CAPSLOCK: u16 = 0x39;
    const K_T: u16 = 0x11;

    static FIRED: AtomicBool = AtomicBool::new(false);

    fn post(code: u16, down: bool) {
        unsafe {
            let ev = CGEventCreateKeyboardEvent(std::ptr::null(), code, down);
            if ev.is_null() {
                say("CGEventCreateKeyboardEvent returned null");
                return;
            }
            CGEventPost(SESSION_TAP, ev);
            CFRelease(ev as *const c_void);
        }
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
            say("REFUSING: not an Aqua session.");
            std::process::exit(3);
        }

        let mode = std::env::args().nth(1).unwrap_or_default();
        if mode != "on" && mode != "off" {
            say("usage: caps_live <on|off>");
            std::process::exit(2);
        }
        say(&format!("MODE: {mode}"));

        // Anything else that owns Caps makes this measurement about that
        // thing instead, so it is reported before the result rather than
        // guessed at afterwards.
        let others = std::process::Command::new("/usr/bin/pgrep")
            .args(["-l", "kanata|karabiner_grabber"])
            .arg("-f")
            .output()
            .ok()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .unwrap_or_default();
        say(&format!(
            "other keyboard remappers: {}",
            if others.is_empty() {
                "none seen"
            } else {
                &others
            }
        ));

        let hold = Chord {
            ctrl: true,
            super_: true,
            alt: true,
        };

        let mut mgr = match beckon_macos::hotkey::HotkeyManager::install(Box::new(|id| {
            FIRED.store(true, Ordering::SeqCst);
            say(&format!("HOTKEY FIRED: id={id}"));
        })) {
            Ok(m) => m,
            Err(e) => {
                say(&format!("hotkey install failed: {e}"));
                std::process::exit(1);
            }
        };
        let t = beckon_core::shortcuts::lookup_key("t").expect("key table has t");
        if let Err(e) = mgr.register(0, hold.ctrl, hold.super_, hold.alt, false, t) {
            say(&format!("register failed: {e}"));
            std::process::exit(1);
        }
        say("registered ctrl+cmd+opt+T");

        if mode == "on" {
            beckon_macos::caps_tap::set_bindings(HashSet::from([K_T]), hold, CapsTap::CapsLock);
            match beckon_macos::caps_tap::install() {
                Ok(()) => say("caps tap installed, T reachable through Caps"),
                Err(e) => {
                    say(&format!("caps tap install failed: {e}"));
                    std::process::exit(4);
                }
            }
        } else {
            say("caps tap NOT installed (control run)");
        }
        say("READY");

        let mut n = 0u32;
        beckon_macos::hotkey::add_tick(
            0.8,
            Box::new(move || {
                n += 1;
                match n {
                    2 => {
                        say("press Caps");
                        post(K_CAPSLOCK, true);
                    }
                    3 => {
                        say("press T while Caps is held");
                        post(K_T, true);
                        post(K_T, false);
                    }
                    4 => {
                        say("release Caps");
                        post(K_CAPSLOCK, false);
                    }
                    6 => {
                        say("");
                        let fired = FIRED.load(Ordering::SeqCst);
                        say(&format!("RESULT: hotkey fired = {fired}"));
                        say(match (mode.as_str(), fired) {
                            ("on", true) => {
                                "VERDICT: Caps+T reached the chord. Read against the `off` run."
                            }
                            ("on", false) => "VERDICT: the alias did NOT work",
                            ("off", true) => {
                                "VERDICT: something ELSE maps Caps here -- this machine cannot \
                                 measure beckon's alias until it is stopped"
                            }
                            ("off", false) => {
                                "VERDICT: nothing else maps Caps, so an `on` run that fires is \
                                 beckon's doing"
                            }
                            _ => unreachable!(),
                        });
                        beckon_macos::caps_tap::uninstall();
                        std::process::exit(if fired { 0 } else { 1 });
                    }
                    _ if n > 14 => {
                        beckon_macos::caps_tap::uninstall();
                        say("TIMEOUT");
                        std::process::exit(5);
                    }
                    _ => {}
                }
            }),
        );

        beckon_macos::hotkey::HotkeyManager::run_forever();
    }
}
