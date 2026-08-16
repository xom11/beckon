//! Can a `CGEventTap` record a chord the way `WH_KEYBOARD_LL` does?
//!
//! ```text
//! cargo run -p beckon-macos --example capture_probe            # swallow (what capture needs)
//! cargo run -p beckon-macos --example capture_probe -- pass    # control: same tap, nothing suppressed
//! ```
//!
//! **Run it in kitty on airm3**, or anywhere with Input Monitoring. It prints
//! `IOHIDCheckAccess` first, because a tap without that grant is created
//! successfully and then receives nothing, silently.
//!
//! ## Why this exists before any UI code
//!
//! Windows' chord capture was written after `caps_probe` measured that the
//! hook sees `Win+T` and friends, with a control proving the detector worked.
//! The same four questions have to be asked here, and two of them have macOS
//! answers that cannot be guessed from the Windows side:
//!
//! 1. **Do ordinary keys arrive as `keyDown`/`keyUp` with a keycode
//!    `shortcuts::key_table()` knows?** If yes, `capture::step` is reusable
//!    through a projection: `KeyDef` already carries BOTH `mac: u16` and
//!    `win: u32`, so a Carbon keycode maps to the Win32 vk `step` expects.
//!    If no, capture needs a `step_mac` and the whole state machine forks.
//!
//! 2. **Does `flagsChanged` say which EDGE a modifier just took?** This is
//!    the one that has no Windows counterpart and no safe assumption. Caps
//!    does not: `caps_tap` tracks parity because suppression freezes the lock
//!    the flag reports. Ctrl/Cmd/Option/Shift are not locks, so their bit
//!    should follow the physical key -- **should**. `capture::step` takes a
//!    `KeyEvent { edge }` and a live `Mods`, and both come from here.
//!
//! 3. **Are the system chords visible, and can they be swallowed?**
//!    `Cmd+Space`, `Cmd+Tab`, `Ctrl+Up`, `Cmd+Q` are this platform's
//!    `Win+T` -- the chords a person would plausibly try to bind, and the
//!    ones the shell consumes first.
//!
//! 4. **Is there a chord seen but NOT suppressible?** On Windows that is
//!    `Win+L`: the hook sees it, returning 1 does not stop the lock, so
//!    `capture::is_reserved` block-lists it rather than pretending. macOS
//!    needs its own list and this is how to find out what goes on it.
//!
//! Question 4 is the only one a machine cannot answer alone -- "did the
//! system act?" is a thing a person watches. The probe says SEEN and
//! SWALLOWED; you say ACTED.

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

    fn say(l: &str) {
        println!("{l}");
        let _ = std::io::stdout().flush();
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
        static kCFRunLoopCommonModes: *const c_void;
        static kCFRunLoopDefaultMode: *const c_void;
    }
    #[link(name = "IOKit", kind = "framework")]
    unsafe extern "C" {
        fn IOHIDCheckAccess(request: u32) -> u32;
    }
    const REQUEST_LISTEN: u32 = 1;

    const HID_EVENT_TAP: u32 = 0;
    const HEAD_INSERT: u32 = 0;
    const DEFAULT_TAP: u32 = 0;
    const KEY_DOWN: u32 = 10;
    const KEY_UP: u32 = 11;
    const FLAGS_CHANGED: u32 = 12;
    const TAP_DISABLED_BY_TIMEOUT: u32 = 0xFFFF_FFFE;
    const TAP_DISABLED_BY_USER: u32 = 0xFFFF_FFFF;
    const KEYBOARD_EVENT_KEYCODE: u32 = 9;

    // `CGEventFlags`. Only the four capture can record -- Caps is
    // `caps_tap`'s subject and deliberately not this probe's.
    const FLAG_SHIFT: u64 = 0x0002_0000;
    const FLAG_CONTROL: u64 = 0x0004_0000;
    const FLAG_ALTERNATE: u64 = 0x0008_0000;
    const FLAG_COMMAND: u64 = 0x0010_0000;

    // Carbon keycodes for the modifier keys, which `key_table()` does NOT
    // carry -- it is a table of bindable keys, and a bare modifier is not
    // one. Left/right pairs, because `flagsChanged` reports the physical key.
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

    /// Question 1, answered per event: is this keycode one `capture::step`
    /// could be handed through the existing `KeyDef` projection?
    fn projected(code: u16) -> Option<(&'static str, u32)> {
        beckon_core::shortcuts::key_table()
            .iter()
            .find(|k| k.mac == code)
            .map(|k| (k.name.as_str(), k.win))
    }

    static SWALLOW: AtomicBool = AtomicBool::new(true);
    static SEEN_DOWN: AtomicUsize = AtomicUsize::new(0);
    static SEEN_UP: AtomicUsize = AtomicUsize::new(0);
    static SEEN_FLAGS: AtomicUsize = AtomicUsize::new(0);
    static UNPROJECTED: AtomicUsize = AtomicUsize::new(0);
    static EDGE_READABLE: AtomicUsize = AtomicUsize::new(0);
    static EDGE_UNREADABLE: AtomicUsize = AtomicUsize::new(0);

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
        // A tap that is disabled must be re-enabled by hand or it stays dead
        // and the rest of the run is a silent false negative. `caps_tap`
        // handles the same two types and calls `resync` after.
        if etype == TAP_DISABLED_BY_TIMEOUT || etype == TAP_DISABLED_BY_USER {
            say(&format!(
                "  !! TAP DISABLED ({}) -- re-enabling; everything after this \
                 is still measured, everything during the gap was missed",
                if etype == TAP_DISABLED_BY_TIMEOUT {
                    "timeout"
                } else {
                    "user"
                }
            ));
            return event;
        }

        let code = unsafe { CGEventGetIntegerValueField(event, KEYBOARD_EVENT_KEYCODE) } as u16;
        let flags = unsafe { CGEventGetFlags(event) };
        let swallow = SWALLOW.load(Ordering::Relaxed);

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
                        "  key   {edge}  keycode 0x{code:02X}  mods {:<18} -> key_table \"{name}\" (win vk 0x{win:02X})",
                        mods_of(flags)
                    )),
                    None => {
                        UNPROJECTED.fetch_add(1, Ordering::Relaxed);
                        say(&format!(
                            "  key   {edge}  keycode 0x{code:02X}  mods {:<18} -> NOT IN key_table",
                            mods_of(flags)
                        ));
                    }
                }
            }
            FLAGS_CHANGED => {
                SEEN_FLAGS.fetch_add(1, Ordering::Relaxed);
                match modifier_name(code) {
                    Some((name, bit)) if bit != 0 => {
                        // **Question 2.** The bit for THIS key in the flags
                        // this event carries: set means the key just went
                        // down, clear means it just came up -- if the flags
                        // track the physical key at all.
                        let down = flags & bit != 0;
                        EDGE_READABLE.fetch_add(1, Ordering::Relaxed);
                        say(&format!(
                            "  mod   {:<4}  keycode 0x{code:02X}  mods {:<18} -> edge readable: {}",
                            name,
                            mods_of(flags),
                            if down { "DOWN" } else { "UP" }
                        ));
                    }
                    Some((name, _)) => {
                        EDGE_UNREADABLE.fetch_add(1, Ordering::Relaxed);
                        say(&format!(
                            "  mod   {name:<4}  keycode 0x{code:02X}  mods {:<18} -> no bit of its own (this is Caps; caps_tap uses parity)",
                            mods_of(flags)
                        ));
                    }
                    None => say(&format!(
                        "  mod   ????  keycode 0x{code:02X}  mods {:<18} -> unknown modifier keycode",
                        mods_of(flags)
                    )),
                }
            }
            _ => {}
        }

        if swallow {
            std::ptr::null_mut()
        } else {
            event
        }
    }

    pub fn run() {
        let pass = std::env::args().any(|a| a == "pass");
        SWALLOW.store(!pass, Ordering::Relaxed);

        say("=== capture_probe ===");
        say(&format!(
            "mode              : {}",
            if pass {
                "PASS (control -- nothing suppressed; the system should act normally)"
            } else {
                "SWALLOW (what capture needs -- the system should NOT act)"
            }
        ));
        say(&format!(
            "Input Monitoring  : {}",
            match unsafe { IOHIDCheckAccess(REQUEST_LISTEN) } {
                0 => "granted",
                1 => "DENIED  <- the tap will receive nothing, silently",
                _ => "unknown (never asked)",
            }
        ));
        say(&format!(
            "key_table entries : {}",
            beckon_core::shortcuts::key_table().len()
        ));
        say("");
        say("Press these, in this order, then wait. 30 seconds.");
        say("  1. a            -- an ordinary key");
        say("  2. F2           -- a function key");
        say("  3. hold Ctrl, hold Cmd, hold Option, release all");
        say("  4. Cmd+Space    -- Spotlight");
        say("  5. Cmd+Tab      -- app switcher");
        say("  6. Ctrl+Up      -- Mission Control");
        say("  7. Cmd+Q        -- quit the front app");
        say("  8. Ctrl+Cmd+Q   -- lock screen (the Win+L candidate)");
        say("");
        say("The probe says SEEN and SWALLOWED. Only you can say ACTED --");
        say("watch whether Spotlight opens, whether the app switcher appears,");
        say("and above all whether the screen locks on 8.");
        say("");

        let mask = (1u64 << KEY_DOWN) | (1u64 << KEY_UP) | (1u64 << FLAGS_CHANGED);
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
            say("Without Input Monitoring this is what you get -- grant it and re-run.");
            std::process::exit(1);
        }
        unsafe {
            let src = CFMachPortCreateRunLoopSource(std::ptr::null(), tap, 0);
            CFRunLoopAddSource(CFRunLoopGetCurrent(), src, kCFRunLoopCommonModes);
            CGEventTapEnable(tap, true);
            CFRunLoopRunInMode(kCFRunLoopDefaultMode, 30.0, false);
            CGEventTapEnable(tap, false);
        }

        let down = SEEN_DOWN.load(Ordering::Relaxed);
        let up = SEEN_UP.load(Ordering::Relaxed);
        let fl = SEEN_FLAGS.load(Ordering::Relaxed);
        let unproj = UNPROJECTED.load(Ordering::Relaxed);
        let readable = EDGE_READABLE.load(Ordering::Relaxed);
        let unreadable = EDGE_UNREADABLE.load(Ordering::Relaxed);

        say("");
        say("=== what this run measured ===");
        say(&format!("keyDown {down}   keyUp {up}   flagsChanged {fl}"));
        say("");
        say(&format!(
            "Q1  ordinary keys reach the tap with a key_table keycode : {}",
            if down == 0 {
                "NO EVENTS AT ALL -- the tap saw nothing; treat every other line as void"
            } else if unproj == 0 {
                "yes, every one -- capture::step is reusable through KeyDef"
            } else {
                "PARTLY -- some keycodes are not in key_table; see NOT IN key_table above"
            }
        ));
        say(&format!(
            "Q2  a modifier's edge is readable from its own flag bit  : {}",
            if readable == 0 && unreadable == 0 {
                "no modifier events seen -- step 3 was not performed"
            } else if readable > 0 {
                "yes -- unlike Caps, these are not locks, so the bit follows the key"
            } else {
                "NO -- capture needs parity tracking like caps_tap"
            }
        ));
        say("Q3  system chords seen / swallowed                       : read the lines above");
        say("Q4  anything seen but NOT suppressible                   : YOU say -- did 8 lock the screen?");
        say("");
        if !pass {
            say("Now run the control, or the result above means nothing:");
            say("  cargo run -p beckon-macos --example capture_probe -- pass");
            say("A tap that receives nothing and a tap that suppresses everything");
            say("produce the same silence from the system.");
        }
    }
}
