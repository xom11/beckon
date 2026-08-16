//! Can a `CGEventTap` record a chord the way `WH_KEYBOARD_LL` does?
//!
//! ```text
//! cargo run -p beckon-macos --example capture_probe             # observe only, safe
//! cargo run -p beckon-macos --example capture_probe -- swallow  # suppress -- read the warnings
//! ```
//!
//! Every line also goes to `~/beckon-test/capture_probe.log`, and that is not
//! a convenience: **the first run of this probe took the terminal down with
//! it and every measurement was lost.** stdout is not a safe place to keep a
//! result produced by a program that suppresses keystrokes.
//!
//! ## Input Monitoring is per-BINARY here, and that is the trap
//!
//! Accessibility is inherited from the responsible process -- `beckon doctor`
//! run inside kitty reports the grant kitty holds. **`kTCCServiceListenEvent`
//! is not.** Read on airm3 2026-08-16, `beckon` had its own row while this
//! probe had none, so the tap was created successfully and received nothing.
//! Every key went through, `Cmd+Q` closed the terminal, and the run looked
//! exactly like "macOS refuses to suppress `Cmd+Q`" while measuring nothing
//! at all.
//!
//! So this probe **refuses to run without the grant** rather than producing
//! that result, and calls `IOHIDRequestAccess` so the dialog appears at all
//! -- `IOHIDCheckAccess` only asks, it never prompts, which is why the grant
//! could never arrive on its own.
//!
//! ## The four questions
//!
//! 1. **Do ordinary keys arrive as `keyDown`/`keyUp` with a keycode
//!    `shortcuts::key_table()` knows?** If yes, `capture::step` is reusable
//!    through a projection: `KeyDef` carries BOTH `mac: u16` and `win: u32`,
//!    so a Carbon keycode maps to the Win32 vk `step` expects. If no, capture
//!    needs a `step_mac` and the state machine forks.
//!
//! 2. **Does `flagsChanged` say which EDGE a modifier just took?** No Windows
//!    counterpart and no safe assumption. Caps does not -- `caps_tap` tracks
//!    parity because suppression freezes the lock the flag reports.
//!    Ctrl/Cmd/Option/Shift are not locks, so their bit *should* follow the
//!    physical key. `step` needs an `Edge` and a live `Mods`; both come from
//!    here.
//!
//! 3. **Are the system chords visible, and can they be swallowed?**
//!    `Cmd+Space`, `Cmd+Tab`, `Ctrl+Up` are this platform's `Win+T`.
//!
//! 4. **Is there a chord seen but NOT suppressible?** On Windows that is
//!    `Win+L`, which is why `capture::is_reserved` is a block-list rather
//!    than blindness. `Ctrl+Cmd+Q` (lock screen) is the macOS candidate.
//!
//! **`Cmd+Q` is deliberately NOT in the list.** It quits the front app, the
//! front app is the terminal this runs in, and a probe that tells you to
//! close the window it lives in cannot report what happened next. Test it
//! last, from a different app, once the rest is known -- the log file
//! survives that.

fn main() {
    #[cfg(not(target_os = "macos"))]
    {
        eprintln!("capture_probe is macOS-only");
        std::process::exit(2);
    }
    #[cfg(target_os = "macos")]
    mac::run();
}

#[cfg(target_os = "macos")]
mod mac {
    use std::ffi::c_void;
    use std::io::Write;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::Mutex;

    static LOG: Mutex<Option<std::fs::File>> = Mutex::new(None);

    /// Print AND append to the log file. The log is the copy that survives
    /// the terminal being closed by the thing under test.
    fn say(l: &str) {
        println!("{l}");
        let _ = std::io::stdout().flush();
        if let Ok(mut g) = LOG.lock() {
            if let Some(f) = g.as_mut() {
                let _ = writeln!(f, "{l}");
                let _ = f.flush();
            }
        }
    }

    type TapCallback = extern "C" fn(*mut c_void, u32, *mut c_void, *mut c_void) -> *mut c_void;

    #[link(name = "CoreGraphics", kind = "framework")]
    unsafe extern "C" {
        fn CGEventTapCreate(
            tap: u32,
            place: u32,
            options: u32,
            events_of_interest: u64,
            callback: TapCallback,
            user_info: *mut c_void,
        ) -> *mut c_void;
        fn CGEventTapEnable(tap: *mut c_void, enable: bool);
        fn CGEventGetIntegerValueField(event: *mut c_void, field: u32) -> i64;
        fn CGEventGetFlags(event: *mut c_void) -> u64;
    }
    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CFMachPortCreateRunLoopSource(
            allocator: *const c_void,
            port: *mut c_void,
            order: isize,
        ) -> *mut c_void;
        fn CFRunLoopGetCurrent() -> *mut c_void;
        fn CFRunLoopAddSource(rl: *mut c_void, source: *mut c_void, mode: *const c_void);
        fn CFRunLoopRunInMode(mode: *const c_void, seconds: f64, return_after_source: bool) -> i32;
        fn CFRunLoopStop(rl: *mut c_void);
        static kCFRunLoopCommonModes: *const c_void;
        static kCFRunLoopDefaultMode: *const c_void;
    }
    #[link(name = "IOKit", kind = "framework")]
    unsafe extern "C" {
        fn IOHIDCheckAccess(request: u32) -> u32;
        /// The only call that raises the dialog. `IOHIDCheckAccess` asks and
        /// never prompts, so a binary with no TCC row can never acquire one
        /// through it -- which is exactly how the first run of this probe
        /// measured nothing while looking like it measured something.
        fn IOHIDRequestAccess(request: u32) -> bool;
    }
    const REQUEST_LISTEN: u32 = 1;

    const HID_EVENT_TAP: u32 = 0;
    const HEAD_INSERT: u32 = 0;
    const DEFAULT_TAP: u32 = 0;
    const KEY_DOWN: u32 = 10;
    const KEY_UP: u32 = 11;
    const FLAGS_CHANGED: u32 = 12;
    /// `NX_SYSDEFINED`. Media, brightness and the other fn-row keys arrive as
    /// this, **not** as `keyDown` -- so a tap that registers only the three
    /// types above is blind to them, and a chord built on one could never
    /// fire. Asking is the only way to know which side of that line a key is
    /// on.
    const SYSDEFINED: u32 = 14;
    const TAP_DISABLED_BY_TIMEOUT: u32 = 0xFFFF_FFFE;
    const TAP_DISABLED_BY_USER: u32 = 0xFFFF_FFFF;
    const KEYBOARD_EVENT_KEYCODE: u32 = 9;

    const FLAG_SHIFT: u64 = 0x0002_0000;
    const FLAG_CONTROL: u64 = 0x0004_0000;
    const FLAG_ALTERNATE: u64 = 0x0008_0000;
    const FLAG_COMMAND: u64 = 0x0010_0000;

    const KC_C: u16 = 0x08;
    const KC_ESCAPE: u16 = 0x35;

    fn modifier_name(code: u16) -> Option<(&'static str, u64)> {
        Some(match code {
            0x38 => ("shift", FLAG_SHIFT),
            0x3C => ("shift(R)", FLAG_SHIFT),
            0x3B => ("ctrl", FLAG_CONTROL),
            0x3E => ("ctrl(R)", FLAG_CONTROL),
            0x3A => ("option", FLAG_ALTERNATE),
            0x3D => ("option(R)", FLAG_ALTERNATE),
            0x37 => ("command", FLAG_COMMAND),
            0x36 => ("command(R)", FLAG_COMMAND),
            0x39 => ("capslock", 0),
            _ => return None,
        })
    }

    /// Question 1, answered per event.
    fn projected(code: u16) -> Option<(&'static str, u32)> {
        beckon_core::shortcuts::key_table()
            .iter()
            .find(|k| k.mac == code)
            .map(|k| (k.name.as_str(), k.win))
    }

    static SWALLOW: AtomicBool = AtomicBool::new(false);
    static SEEN_DOWN: AtomicUsize = AtomicUsize::new(0);
    static SEEN_UP: AtomicUsize = AtomicUsize::new(0);
    static UNPROJECTED: AtomicUsize = AtomicUsize::new(0);
    static EDGE_READABLE: AtomicUsize = AtomicUsize::new(0);
    static EDGE_FROZEN: AtomicUsize = AtomicUsize::new(0);
    static SEEN_SYS: AtomicUsize = AtomicUsize::new(0);

    fn mods_of(flags: u64) -> String {
        let mut s = String::new();
        for (bit, name) in [
            (FLAG_CONTROL, "ctrl"),
            (FLAG_COMMAND, "cmd"),
            (FLAG_ALTERNATE, "opt"),
            (FLAG_SHIFT, "shift"),
        ] {
            if flags & bit != 0 {
                if !s.is_empty() {
                    s.push('+');
                }
                s.push_str(name);
            }
        }
        if s.is_empty() {
            s.push('-');
        }
        s
    }

    extern "C" fn on_event(
        _proxy: *mut c_void,
        etype: u32,
        event: *mut c_void,
        _user: *mut c_void,
    ) -> *mut c_void {
        if etype == TAP_DISABLED_BY_TIMEOUT || etype == TAP_DISABLED_BY_USER {
            say("  !! TAP DISABLED -- everything during the gap was missed");
            return event;
        }

        let code = unsafe { CGEventGetIntegerValueField(event, KEYBOARD_EVENT_KEYCODE) } as u16;
        let flags = unsafe { CGEventGetFlags(event) };

        // **Two escape hatches, always passed through, never suppressed.**
        // The first run of this probe swallowed everything for 30 seconds
        // with no way out, so the only exit was Force Quit -- which also
        // destroyed the output. A probe that can trap you is a probe whose
        // results you cannot collect.
        let ctrl_c = etype == KEY_DOWN && code == KC_C && flags & FLAG_CONTROL != 0;
        // **BARE Escape only.** The first version matched every Escape
        // key-down, which made `Cmd+Option+Esc` -- the Force Quit chord this
        // probe now exists to measure -- stop the run instead of being
        // recorded. An escape hatch that eats the measurement is worse than
        // none, because the run still looks successful.
        //
        // It is also what `capture::step` does: bare Escape cancels, Escape
        // with a modifier is an ordinary chord.
        let bare = flags & (FLAG_CONTROL | FLAG_COMMAND | FLAG_ALTERNATE | FLAG_SHIFT) == 0;
        let escape = etype == KEY_DOWN && code == KC_ESCAPE && bare;
        if ctrl_c || escape {
            say(&format!(
                "  EXIT   {}  -- passed through, stopping",
                if ctrl_c { "ctrl+c " } else { "escape " }
            ));
            unsafe { CFRunLoopStop(CFRunLoopGetCurrent()) };
            return event;
        }

        match etype {
            KEY_DOWN | KEY_UP => {
                if etype == KEY_DOWN {
                    SEEN_DOWN.fetch_add(1, Ordering::Relaxed);
                } else {
                    SEEN_UP.fetch_add(1, Ordering::Relaxed);
                }
                let edge = if etype == KEY_DOWN { "down" } else { "up  " };
                match projected(code) {
                    Some((name, win)) => say(&format!(
                        "  key   {edge}  0x{code:02X}  mods {:<16} -> key_table \"{name}\" (win vk 0x{win:02X})",
                        mods_of(flags)
                    )),
                    None => {
                        UNPROJECTED.fetch_add(1, Ordering::Relaxed);
                        say(&format!(
                            "  key   {edge}  0x{code:02X}  mods {:<16} -> NOT IN key_table",
                            mods_of(flags)
                        ));
                    }
                }
            }
            FLAGS_CHANGED => match modifier_name(code) {
                Some((name, bit)) if bit != 0 => {
                    let down = flags & bit != 0;
                    EDGE_READABLE.fetch_add(1, Ordering::Relaxed);
                    say(&format!(
                        "  mod   {name:<9} 0x{code:02X}  mods {:<16} -> edge {}",
                        mods_of(flags),
                        if down { "DOWN" } else { "UP" }
                    ));
                }
                Some((name, _)) => {
                    EDGE_FROZEN.fetch_add(1, Ordering::Relaxed);
                    say(&format!(
                        "  mod   {name:<9} 0x{code:02X}  mods {:<16} -> no bit (Caps; caps_tap uses parity)",
                        mods_of(flags)
                    ));
                }
                None => say(&format!("  mod   ????      0x{code:02X}")),
            },
            SYSDEFINED => {
                SEEN_SYS.fetch_add(1, Ordering::Relaxed);
                say(&format!(
                    "  sys   ----      0x{code:02X}  mods {:<16} -> NX_SYSDEFINED (a media/fn key, not a keyDown)",
                    mods_of(flags)
                ));
            }
            _ => {}
        }

        if SWALLOW.load(Ordering::Relaxed) {
            std::ptr::null_mut()
        } else {
            event
        }
    }

    pub fn run() {
        if let Some(home) = std::env::var_os("HOME") {
            let p = std::path::Path::new(&home)
                .join("beckon-test")
                .join("capture_probe.log");
            let _ = std::fs::create_dir_all(p.parent().unwrap());
            if let Ok(f) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&p)
            {
                *LOG.lock().unwrap() = Some(f);
            }
            say(&format!("=== capture_probe === (log: {})", p.display()));
        } else {
            say("=== capture_probe === (no HOME; log disabled)");
        }

        let swallow = std::env::args().any(|a| a == "swallow");
        SWALLOW.store(swallow, Ordering::Relaxed);

        // **Refuse rather than measure nothing.** Without this row in TCC the
        // tap is created and receives no events, which is indistinguishable
        // from "the system let everything through" -- and that is exactly how
        // the first run produced a confident wrong answer about `Cmd+Q`.
        let access = unsafe { IOHIDCheckAccess(REQUEST_LISTEN) };
        say(&format!(
            "Input Monitoring : {}",
            match access {
                0 => "granted",
                1 => "DENIED",
                _ => "never asked",
            }
        ));
        if access != 0 {
            say("");
            say("STOPPING. This binary has no Input Monitoring grant of its own.");
            say("");
            say("Accessibility is inherited from the terminal; **this is not**.");
            say("kitty holding the grant does nothing for a child binary -- TCC");
            say("keeps a row per binary path, and this one has none. A tap");
            say("without it is created successfully and then receives nothing,");
            say("silently, which reads exactly like `the system ignored us`.");
            say("");
            say("Raising the dialog now (IOHIDCheckAccess only asks; it never");
            say("prompts, which is why the grant could not arrive on its own):");
            let asked = unsafe { IOHIDRequestAccess(REQUEST_LISTEN) };
            say(&format!("  IOHIDRequestAccess returned {asked}"));
            say("");
            say("If no dialog appeared, add it by hand:");
            say("  System Settings > Privacy & Security > Input Monitoring > +");
            say("  then Cmd+Shift+G and paste:");
            say("  ~/beckon-test/capture_probe");
            say("");
            say("Then run this again. It will not measure until the row exists.");
            std::process::exit(1);
        }

        say(&format!(
            "mode             : {}",
            if swallow {
                "SWALLOW -- keys are suppressed; ctrl+c and escape still work"
            } else {
                "OBSERVE -- nothing is suppressed (safe; run with `swallow` after)"
            }
        ));
        say(&format!(
            "key_table        : {} entries",
            beckon_core::shortcuts::key_table().len()
        ));
        say("");
        say("Press these, then wait. 25 seconds, or escape to stop early.");
        say("  1. a              -- an ordinary key");
        say("  2. F2             -- a function key");
        say("  3. hold Ctrl, hold Cmd, hold Option, release all");
        say("  4. Cmd+Space      -- Spotlight");
        say("  5. Cmd+Tab        -- app switcher");
        say("  6. Ctrl+Up        -- Mission Control");
        say("  7. Cmd+Option+Esc -- Force Quit. Bare Esc still stops the run;");
        say("                       THIS one is a chord and is measured.");
        say("  8. fn+F1 or F2    -- brightness (a media key)");
        say("  9. the volume keys, and `fn` on its own");
        say("");
        say("7 answers the last chord question: Force Quit is the remaining");
        say("`Win+L` candidate. 8 and 9 answer a different one -- whether a key");
        say("exists that a tap CANNOT see, which would make a binding on it");
        say("dead on arrival. They arrive as NX_SYSDEFINED if at all.");
        say("");
        say("NOT in the list, and not measurable this way:");
        say("  Touch ID / power  a tap sees KEYS. Those are not keys, and the");
        say("                    honest answer is that this probe cannot ask --");
        say("                    pressing one to find out risks sleeping or");
        say("                    shutting the machine mid-measurement.");
        say("");
        say("NOT in the list, on purpose:");
        say("  Cmd+Q      quits the front app, which is THIS terminal. The");
        say("             first run of this probe told you to press it and");
        say("             took the window down with the whole measurement.");
        say("  Ctrl+Cmd+Q locks the screen -- the `Win+L` candidate. Worth");
        say("             measuring, but do it last: the log file survives it");
        say("             and the terminal may not.");
        say("");
        say("The probe says SEEN and SWALLOWED. Only you can say ACTED --");
        say("whether Spotlight opened, whether the switcher appeared.");
        say("");

        let mask =
            (1u64 << KEY_DOWN) | (1u64 << KEY_UP) | (1u64 << FLAGS_CHANGED) | (1u64 << SYSDEFINED);
        let tap = unsafe {
            CGEventTapCreate(
                HID_EVENT_TAP,
                HEAD_INSERT,
                DEFAULT_TAP,
                mask,
                on_event,
                std::ptr::null_mut(),
            )
        };
        if tap.is_null() {
            say("TAP REFUSED: CGEventTapCreate returned NULL.");
            std::process::exit(1);
        }
        unsafe {
            let src = CFMachPortCreateRunLoopSource(std::ptr::null(), tap, 0);
            CFRunLoopAddSource(CFRunLoopGetCurrent(), src, kCFRunLoopCommonModes);
            CGEventTapEnable(tap, true);
            CFRunLoopRunInMode(kCFRunLoopDefaultMode, 25.0, false);
            CGEventTapEnable(tap, false);
        }

        let down = SEEN_DOWN.load(Ordering::Relaxed);
        let up = SEEN_UP.load(Ordering::Relaxed);
        let unproj = UNPROJECTED.load(Ordering::Relaxed);
        let readable = EDGE_READABLE.load(Ordering::Relaxed);

        say("");
        say("=== what this run measured ===");
        say(&format!("keyDown {down}   keyUp {up}"));
        say(&format!(
            "Q1  ordinary keys carry a key_table keycode : {}",
            if down == 0 {
                "NO EVENTS -- the tap saw nothing; every other line is void"
            } else if unproj == 0 {
                "yes, all of them -- capture::step is reusable through KeyDef"
            } else {
                "partly -- see NOT IN key_table above"
            }
        ));
        say(&format!(
            "Q2  a modifier's edge is readable from flags : {}",
            if readable == 0 {
                "no modifier events -- step 3 was not performed"
            } else {
                "yes -- unlike Caps, these are not locks"
            }
        ));
        say("Q3  system chords seen / swallowed          : read the lines above");
        say("Q4  seen but NOT suppressible               : you say -- what acted?");
        let sys = SEEN_SYS.load(Ordering::Relaxed);
        let q5 = if sys == 0 {
            "no NX_SYSDEFINED seen -- either none was pressed, or the tap is \
             not given them at all"
                .to_string()
        } else {
            format!(
                "{sys} NX_SYSDEFINED events. Those are NOT keyDown, so a tap \
                 registering only key events is blind to them"
            )
        };
        say(&format!(
            "Q5  keys a tap cannot see as keyDown        : {q5}"
        ));
        say("");
        if !swallow {
            say("Now the other half. A tap that receives nothing and a tap that");
            say("suppresses everything look identical from outside:");
            say("  ~/beckon-test/capture_probe swallow");
        }
    }
}
