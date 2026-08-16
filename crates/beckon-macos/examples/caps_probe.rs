//! What does Caps Lock look like to a `CGEventTap`, and can it be taken?
//!
//! Phase B's design 5 — the question to answer **before** writing the
//! feature, because the last time that order was skipped on this platform the
//! settings window was written against a run loop that delivered it no
//! events.
//!
//! ```text
//! cargo run -p beckon-macos --example caps_probe
//! ```
//!
//! ## What is actually in doubt
//!
//! The Windows design is an ALIAS: a `WH_KEYBOARD_LL` hook sees `Caps+T`,
//! swallows it, and injects `ctrl+win+alt+T` as one burst, so
//! `RegisterHotKey` — which cannot bind Caps — fires on a chord it already
//! listens for. Everything in `beckon_core::caps` is that decision.
//!
//! Porting it needs five facts about this platform, and only the first is
//! documented clearly:
//!
//! 1. **Does the tap see Caps Lock as a key at all?** macOS reports Caps
//!    through `kCGEventFlagsChanged`, not `keyDown`/`keyUp` — so the Windows
//!    state machine's `edge` (down/up) may not exist here in the same shape.
//! 2. **Can it be suppressed?** Returning NULL from the callback drops an
//!    event. Whether that also stops the LOCK STATE from toggling is a
//!    different question, and the answer decides whether `caps_tap =
//!    "capslock"` is implementable or whether the lock is unavoidable.
//! 3. **Which grant does the tap need** — Accessibility, Input Monitoring, or
//!    both? They are separate panes and separate answers.
//! 4. **Is a key pressed while Caps is held visible and swallowable?**
//! 5. **Can the alias chord be posted from inside the callback**, and does
//!    `RegisterEventHotKey` then fire?
//!
//! This probe answers 1–4. 5 needs the hotkey half and gets its own probe.
//!
//! ## Every question carries a control
//!
//! A tap that was never installed and a Caps key that is never seen produce
//! the same silence. So the probe first proves it can see an ORDINARY key
//! (`F19`, chosen because nothing binds it), and refuses to report anything
//! about Caps until that has happened. This branch has now been caught four
//! times by a detector that was simply blind, and every one of those was
//! caught by a control rather than by care.

fn main() {
    #[cfg(not(target_os = "macos"))]
    {
        eprintln!("caps_probe is macOS-only");
        std::process::exit(2);
    }
    #[cfg(target_os = "macos")]
    mac::run();
}

#[cfg(target_os = "macos")]
mod mac {
    use std::ffi::c_void;
    use std::io::Write;
    use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

    fn say(l: &str) {
        println!("{l}");
        let _ = std::io::stdout().flush();
    }

    // --- CoreGraphics event taps, hand-rolled like `src/ffi.rs` ------------

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
        fn CGEventTapIsEnabled(tap: *mut c_void) -> bool;
        fn CGEventGetIntegerValueField(event: *mut c_void, field: u32) -> i64;
        fn CGEventGetFlags(event: *mut c_void) -> u64;
        fn CGEventSourceKeyState(state_id: i32, key: u16) -> bool;
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
        static kCFRunLoopCommonModes: *const c_void;
    }
    #[link(name = "ApplicationServices", kind = "framework")]
    unsafe extern "C" {
        fn AXIsProcessTrusted() -> bool;
    }
    // **Input Monitoring is a DIFFERENT grant from Accessibility**, with its
    // own System Settings pane and its own answer, and it is the one a
    // keyboard event tap needs. `IOHIDCheckAccess` is how to ask without
    // prompting.
    #[link(name = "IOKit", kind = "framework")]
    unsafe extern "C" {
        fn IOHIDCheckAccess(request: u32) -> u32;
    }
    /// `kIOHIDRequestTypeListenEvent`.
    const REQUEST_LISTEN: u32 = 1;

    fn input_monitoring() -> &'static str {
        match unsafe { IOHIDCheckAccess(REQUEST_LISTEN) } {
            0 => "granted",
            1 => "DENIED",
            _ => "unknown (never asked)",
        }
    }

    // `CGEventTapLocation`
    const HID_EVENT_TAP: u32 = 0;
    // `CGEventTapPlacement`
    const HEAD_INSERT: u32 = 0;
    // `CGEventTapOptions`
    const DEFAULT_TAP: u32 = 0; // may modify and suppress
                                // Event types.
    const KEY_DOWN: u32 = 10;
    const KEY_UP: u32 = 11;
    const FLAGS_CHANGED: u32 = 12;
    const TAP_DISABLED_BY_TIMEOUT: u32 = 0xFFFF_FFFE;
    const TAP_DISABLED_BY_USER_INPUT: u32 = 0xFFFF_FFFF;
    // `CGEventField::keyboardEventKeycode`
    const FIELD_KEYCODE: u32 = 9;

    const K_CAPSLOCK: u16 = 0x39;
    /// The control key, and it is **not** `F19`.
    ///
    /// F19 was the first choice, on the reasoning that nothing binds it. It
    /// produced **zero events of any type** while a `ctrl+opt+shift+f`
    /// injection through the same injector, in the same session, produced
    /// eight — so the tap was live and the control was the thing that was
    /// wrong. A keycode no physical key on this machine carries does not
    /// survive the trip. Measured 2026-08-16, and it cost a false suspicion
    /// of kanata, which was stopped for nothing.
    ///
    /// `kVK_ANSI_F`, injected with `ctrl+opt+shift` so it types nothing into
    /// whatever has focus, and because that exact chord is already proven to
    /// reach the window server: it fired a real `RegisterEventHotKey`
    /// binding in `hotkey_loop_probe`.
    const K_CONTROL_KEY: u16 = 0x03;
    const FLAG_ALPHA_SHIFT: u64 = 0x0001_0000;

    fn mask(t: u32) -> u64 {
        1u64 << t
    }

    // What the callback saw, read after the fact by the main thread.
    static SAW_CONTROL: AtomicBool = AtomicBool::new(false);
    static SAW_CAPS: AtomicBool = AtomicBool::new(false);
    static CAPS_EVENT_TYPE: AtomicU32 = AtomicU32::new(0);
    static CAPS_FLAGS: AtomicU32 = AtomicU32::new(0);
    static SUPPRESSED_CAPS: AtomicBool = AtomicBool::new(false);
    static TAP_DIED: AtomicBool = AtomicBool::new(false);
    /// Every event the callback saw, of any type.
    ///
    /// **The control's own control.** `SAW_CONTROL` false can mean the tap is
    /// dead OR that the matching is wrong, and those have opposite fixes. A
    /// raw count separates them in one number.
    static SEEN_ANY: AtomicU32 = AtomicU32::new(0);
    static LAST_TYPE: AtomicU32 = AtomicU32::new(999);
    static LAST_CODE: AtomicU32 = AtomicU32::new(999);

    /// Whether the probe is currently trying to SWALLOW Caps.
    static SWALLOW: AtomicBool = AtomicBool::new(false);

    extern "C" fn on_event(
        _proxy: *mut c_void,
        etype: u32,
        event: *mut c_void,
        _ud: *mut c_void,
    ) -> *mut c_void {
        // **The two ways a tap dies, and both must be handled or the probe
        // reports a silence it caused itself.** Past the system's timeout the
        // tap is disabled and simply stops delivering; the callback is told
        // once, and re-enabling is the caller's job.
        if etype == TAP_DISABLED_BY_TIMEOUT || etype == TAP_DISABLED_BY_USER_INPUT {
            TAP_DIED.store(true, Ordering::SeqCst);
            return event;
        }

        SEEN_ANY.fetch_add(1, Ordering::SeqCst);
        LAST_TYPE.store(etype, Ordering::SeqCst);
        let code = unsafe { CGEventGetIntegerValueField(event, FIELD_KEYCODE) } as u16;
        let flags = unsafe { CGEventGetFlags(event) };
        LAST_CODE.store(code as u32, Ordering::SeqCst);

        if code == K_CONTROL_KEY {
            SAW_CONTROL.store(true, Ordering::SeqCst);
            return event;
        }
        if code == K_CAPSLOCK || (etype == FLAGS_CHANGED && flags & FLAG_ALPHA_SHIFT != 0) {
            SAW_CAPS.store(true, Ordering::SeqCst);
            CAPS_EVENT_TYPE.store(etype, Ordering::SeqCst);
            CAPS_FLAGS.store((flags & 0xFFFF_FFFF) as u32, Ordering::SeqCst);
            if SWALLOW.load(Ordering::SeqCst) {
                SUPPRESSED_CAPS.store(true, Ordering::SeqCst);
                // NULL drops the event.
                return std::ptr::null_mut();
            }
        }
        event
    }

    fn type_name(t: u32) -> &'static str {
        match t {
            KEY_DOWN => "kCGEventKeyDown",
            KEY_UP => "kCGEventKeyUp",
            FLAGS_CHANGED => "kCGEventFlagsChanged",
            _ => "other",
        }
    }

    /// Is the Caps Lock LOCK currently on? Read from the HID system state, so
    /// it is the lock itself rather than any one event's flags.
    fn caps_locked() -> bool {
        // `kCGEventSourceStateHIDSystemState` = 1.
        unsafe { CGEventSourceKeyState(1, K_CAPSLOCK) }
    }

    pub fn run() {
        let manager = std::process::Command::new("launchctl")
            .arg("managername")
            .output()
            .ok()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .unwrap_or_default();
        say(&format!("bootstrap namespace : {manager}"));
        say(&format!("AXIsProcessTrusted   : {}", unsafe {
            AXIsProcessTrusted()
        }));
        say(&format!("Input Monitoring     : {}", input_monitoring()));
        if manager != "Aqua" {
            say("REFUSING: not an Aqua session; a tap here would see nothing and say so.");
            std::process::exit(3);
        }

        // **The tap LOCATION is a variable, not a constant.** `kCGHIDEventTap`
        // is the lowest layer and is documented as needing root on some
        // releases; `kCGSessionEventTap` is what an ordinary application
        // uses. Both return a non-NULL port here, so the difference does not
        // show up until events either arrive or do not -- which is the whole
        // reason this is selectable rather than chosen.
        let where_ = std::env::args().nth(1).unwrap_or_else(|| "session".into());
        let location = match where_.as_str() {
            "hid" => HID_EVENT_TAP,
            "session" => 1,
            "annotated" => 2,
            other => {
                say(&format!(
                    "unknown tap location `{other}`; use hid|session|annotated"
                ));
                std::process::exit(2);
            }
        };
        say(&format!("tap location         : {where_} ({location})"));

        let events = mask(KEY_DOWN) | mask(KEY_UP) | mask(FLAGS_CHANGED);
        let tap = unsafe {
            CGEventTapCreate(
                location,
                HEAD_INSERT,
                DEFAULT_TAP,
                events,
                on_event,
                std::ptr::null_mut(),
            )
        };
        if tap.is_null() {
            say("");
            say("TAP REFUSED: CGEventTapCreate returned NULL.");
            say("That is what an ungranted process gets. `AXIsProcessTrusted` above says");
            say("whether Accessibility is the missing one; if it is true, the grant this");
            say("needs is Input Monitoring, which is a SEPARATE pane and a separate");
            say("answer -- and that is itself the finding.");
            std::process::exit(4);
        }
        say("TAP CREATED: CGEventTapCreate returned a port");

        unsafe {
            let src = CFMachPortCreateRunLoopSource(std::ptr::null(), tap, 0);
            // Checked, because a NULL source makes `CFRunLoopAddSource` a
            // silent no-op and the tap then simply never fires -- which is
            // exactly the symptom being chased.
            say(&format!(
                "run loop source      : {}",
                if src.is_null() { "NULL" } else { "ok" }
            ));
            if src.is_null() {
                say("CFMachPortCreateRunLoopSource returned NULL; nothing can be delivered.");
                std::process::exit(7);
            }
            CFRunLoopAddSource(CFRunLoopGetCurrent(), src, kCFRunLoopCommonModes);
            CGEventTapEnable(tap, true);
            say(&format!(
                "CGEventTapIsEnabled  : {}",
                CGEventTapIsEnabled(tap)
            ));
        }
        say(&format!("caps lock is currently: {}", caps_locked()));
        say("READY");

        let mut n = 0u32;
        let mut caps_before = false;
        beckon_macos::hotkey::add_tick(
            1.0,
            Box::new(move || {
                n += 1;
                match n {
                    // The driver injects F19 here.
                    2 => {}
                    3 => {
                        if !SAW_CONTROL.load(Ordering::SeqCst) {
                            say("");
                            say("CONTROL FAILED: an ordinary key never reached the tap.");
                            say(&format!("  Input Monitoring : {}", input_monitoring()));
                            say(&format!(
                                "  events seen, ANY type : {}   last type={} code={}",
                                SEEN_ANY.load(Ordering::SeqCst),
                                LAST_TYPE.load(Ordering::SeqCst),
                                LAST_CODE.load(Ordering::SeqCst)
                            ));
                            if SEEN_ANY.load(Ordering::SeqCst) > 0 {
                                say("  -> the tap IS live; the MATCHING is what is wrong.");
                            } else {
                                say("  -> the tap is receiving nothing at all.");
                            }
                            say("");
                            say(
                                "**A non-NULL port from `CGEventTapCreate` is not evidence the tap",
                            );
                            say("will receive anything.** The call succeeds without Input");
                            say("Monitoring and then delivers nothing, silently -- which is a");
                            say("different failure from the one the NULL return describes, and");
                            say("the one that costs an afternoon.");
                            std::process::exit(5);
                        }
                        say("CONTROL OK: an ordinary key reaches the tap");
                        caps_before = caps_locked();
                        say(&format!("caps lock before the test press: {caps_before}"));
                        SWALLOW.store(true, Ordering::SeqCst);
                        say("swallow armed -- driver may press Caps");
                    }
                    // The driver injects Caps here.
                    5 => {
                        say("");
                        say(&format!(
                            "SAW CAPS          : {}",
                            SAW_CAPS.load(Ordering::SeqCst)
                        ));
                        if SAW_CAPS.load(Ordering::SeqCst) {
                            let t = CAPS_EVENT_TYPE.load(Ordering::SeqCst);
                            say(&format!("  arrives as      : {} ({t})", type_name(t)));
                            say(&format!(
                                "  flags           : {:#010x}  alphaShift={}",
                                CAPS_FLAGS.load(Ordering::SeqCst),
                                CAPS_FLAGS.load(Ordering::SeqCst) as u64 & FLAG_ALPHA_SHIFT != 0
                            ));
                        }
                        say(&format!(
                            "RETURNED NULL     : {}",
                            SUPPRESSED_CAPS.load(Ordering::SeqCst)
                        ));
                        let after = caps_locked();
                        say(&format!(
                            "caps lock after   : {after}  (before: {caps_before})"
                        ));
                        say(&format!(
                            "SUPPRESSION STOPS THE LOCK: {}",
                            if SUPPRESSED_CAPS.load(Ordering::SeqCst) {
                                if after == caps_before {
                                    "YES -- the lock did not move"
                                } else {
                                    "NO -- the lock toggled anyway"
                                }
                            } else {
                                "untested, nothing was swallowed"
                            }
                        ));
                        say(&format!(
                            "TAP DIED          : {}",
                            TAP_DIED.load(Ordering::SeqCst)
                        ));
                        say("");
                        say("DONE");
                        std::process::exit(0);
                    }
                    _ if n > 12 => {
                        say("TIMEOUT: the driver never pressed anything");
                        std::process::exit(6);
                    }
                    _ => {}
                }
            }),
        );

        beckon_macos::hotkey::HotkeyManager::run_forever();
    }
}
