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
//! ## The `off` run needs a control of its own, and now has two
//!
//! A silent `off` run reads as *nothing else maps Caps*. It has a second
//! reading that is far more common and looks identical: **this process cannot
//! type at all.** `CGEventPost` returns `void` and does nothing whatever when
//! the caller is not Accessibility-trusted, which is the state a
//! freshly-`cargo build`-ed binary is always in.
//!
//! 1. `AXIsProcessTrusted()` is printed first and the probe REFUSES when it
//!    is false — `hid_key.rs`'s rule, for the same reason and in both modes:
//!    an `on` run that cannot inject reports *the alias did NOT work*.
//! 2. After the Caps sequence, every run posts the chord **directly** —
//!    `ctrl+cmd+opt+T` with the modifiers as real key events — and that must
//!    fire. It is the positive control for the whole apparatus: injector,
//!    registration and hotkey delivery, measured in the same session as the
//!    result they carry. `DIRECT CONTROL: did not fire` means the run proved
//!    nothing, in either mode.
//!
//! The third row of the table is not hypothetical here. This machine runs kanata, and
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
        fn CGEventSetFlags(event: *mut c_void, flags: u64);
        fn CGEventPost(tap: u32, event: *mut c_void);
    }
    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CFRelease(cf: *const c_void);
    }
    #[link(name = "ApplicationServices", kind = "framework")]
    unsafe extern "C" {
        fn AXIsProcessTrusted() -> bool;
    }

    const SESSION_TAP: u32 = 1;
    const K_CAPSLOCK: u16 = 0x39;
    const K_T: u16 = 0x11;
    // The modifier keys themselves, from HIToolbox Events.h.
    const K_CONTROL: u16 = 0x3B;
    const K_OPTION: u16 = 0x3A;
    const K_COMMAND: u16 = 0x37;
    // `CGEventFlags`, from CGEventTypes.h.
    const F_CONTROL: u64 = 0x0004_0000;
    const F_ALTERNATE: u64 = 0x0008_0000;
    const F_COMMAND: u64 = 0x0010_0000;

    static FIRED: AtomicBool = AtomicBool::new(false);

    fn post(code: u16, down: bool) {
        post_with(code, down, 0);
    }

    fn post_with(code: u16, down: bool, flags: u64) {
        unsafe {
            let ev = CGEventCreateKeyboardEvent(std::ptr::null(), code, down);
            if ev.is_null() {
                say("CGEventCreateKeyboardEvent returned null");
                return;
            }
            CGEventSetFlags(ev, flags);
            CGEventPost(SESSION_TAP, ev);
            CFRelease(ev as *const c_void);
        }
    }

    /// Press `ctrl+cmd+opt+T` the way a hand does — the positive control.
    ///
    /// **The modifiers are pressed as REAL KEYS, not as flags on `T`.**
    /// Measured 2026-08-16 and written up in CLAUDE.md: a single event
    /// carrying `CGEventSetFlags(ctrl|opt|shift)` posts successfully and fires
    /// no `RegisterEventHotKey` chord under EITHER loop. The system tracks
    /// modifier state from the modifier keys' own events; the flags field
    /// describes an event, it does not hold a key down.
    fn post_chord_directly() {
        let pressed = [
            (K_CONTROL, F_CONTROL),
            (K_OPTION, F_ALTERNATE),
            (K_COMMAND, F_COMMAND),
        ];
        let all = F_CONTROL | F_ALTERNATE | F_COMMAND;
        let mut acc = 0u64;
        for (k, f) in pressed {
            acc |= f;
            post_with(k, true, acc);
        }
        post_with(K_T, true, all);
        post_with(K_T, false, all);
        for (k, f) in pressed.iter().rev() {
            acc &= !f;
            post_with(*k, false, acc);
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

        // **Both modes, not just `off`.** An untrusted `CGEventPost` does
        // nothing and says nothing, so an `on` run reports *the alias did NOT
        // work* and an `off` run reports *nothing else maps Caps* -- two
        // confident conclusions from one process that never typed anything.
        // A fresh `cargo build` is always in this state, because the grant is
        // bound to the binary's code signature.
        let trusted = unsafe { AXIsProcessTrusted() };
        say(&format!("AXIsProcessTrusted  : {trusted}"));
        if !trusted {
            say("REFUSING: this process cannot post events, so neither mode would be");
            say("measuring beckon. Grant Accessibility to whatever launched it.");
            std::process::exit(6);
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
        // The Caps result, read and latched before the positive control is
        // allowed to touch `FIRED`.
        let mut via_caps = false;
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
                        // Latch the answer, then reuse `FIRED` for the
                        // control. Two flags would drift; one flag read at a
                        // known point cannot.
                        via_caps = FIRED.swap(false, Ordering::SeqCst);
                        say(&format!("via Caps: hotkey fired = {via_caps}"));
                        say("DIRECT CONTROL: pressing ctrl+cmd+opt+T with no Caps at all");
                        post_chord_directly();
                    }
                    8 => {
                        say("");
                        let control = FIRED.load(Ordering::SeqCst);
                        say(&format!("DIRECT CONTROL: hotkey fired = {control}"));
                        if !control {
                            say("");
                            say("INCONCLUSIVE: the chord this probe registered does");
                            say("not fire even when pressed directly, so nothing above");
                            say("is about Caps. Either the injection is not reaching the");
                            say("window server or the registration never took -- and a");
                            say("silent Caps result looks the same either way.");
                            beckon_macos::caps_tap::uninstall();
                            std::process::exit(7);
                        }
                        let fired = via_caps;
                        say(&format!("RESULT: hotkey fired via Caps = {fired}"));
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
                                "VERDICT: nothing else maps Caps -- and the direct control DID \
                                 fire, so this is silence rather than blindness. An `on` run \
                                 that fires is beckon's doing"
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
