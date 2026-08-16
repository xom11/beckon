//! What does Caps Lock look like to a `CGEventTap`, and can it be taken?
//!
//! Phase B's design 5 — the question to answer **before** writing the
//! feature, because the last time that order was skipped on this platform the
//! settings window was written against a run loop that delivered it no
//! events.
//!
//! ```text
//! cargo run -p beckon-macos --example caps_probe -- session swallow   # the test
//! cargo run -p beckon-macos --example caps_probe -- session pass      # the control
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
//!
//! **Question 2 shipped without one, and it is the question the whole Caps
//! feature rests on.** Two separate faults made
//! `SUPPRESSION STOPS THE LOCK: YES` unfalsifiable:
//!
//! 1. The lock was read with `CGEventSourceKeyState(_, kVK_CapsLock)`, which
//!    answers *is that KEY down* — momentary, and long up again by the time
//!    the probe samples a tick later. It reads `false` before and `false`
//!    after whether suppression works or not. `CGEventSourceFlagsState`'s
//!    `alphaShift` bit is the lock, and it is level rather than momentary.
//! 2. The driver presses Caps **twice**, so a before/after pair is equal even
//!    for a lock that toggled both times. The probe now samples the lock once
//!    per tick and asks whether it moved at ANY point.
//!
//! And the missing control is the `pass` arm: the same run with suppression
//! disarmed, which **must** show the lock moving. A `swallow` verdict read
//! without a `pass` run beside it says nothing at all — it is the same shape
//! as the F19 false negative recorded below, and the same shape as the
//! Windows `caps_probe`'s bare-Win-tap control.

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
        fn CGEventSourceFlagsState(state_id: i32) -> u64;
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
    /// A ring of every event, so the SEQUENCE can be read rather than only
    /// the last one. Caps is a LOCK key: whether macOS reports a press and a
    /// release as two events, and whether the two can be told apart once
    /// suppression has stopped the lock from moving, is what decides whether
    /// "hold Caps and press T" is detectable at all.
    /// One traced event: `(type, keycode, flags, keyState combined, keyState hid)`.
    type Traced = (u32, u16, u64, bool, bool);
    static TRACE: std::sync::Mutex<Vec<Traced>> = std::sync::Mutex::new(Vec::new());

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
        {
            let c = unsafe { CGEventGetIntegerValueField(event, FIELD_KEYCODE) } as u16;
            let f = unsafe { CGEventGetFlags(event) };
            if let Ok(mut t) = TRACE.lock() {
                if t.len() < 64 {
                    // `CGEventSourceKeyState` for the Caps KEYCODE, from two
                    // different source states. If either flips between the
                    // press and the release, it is the discriminator the
                    // flags cannot provide.
                    let combined = unsafe { CGEventSourceKeyState(0, c) };
                    let hid = unsafe { CGEventSourceKeyState(1, c) };
                    t.push((etype, c, f, combined, hid));
                }
            }
        }
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

    /// Is the Caps Lock LOCK currently on?
    ///
    /// **`CGEventSourceKeyState(_, kVK_CapsLock)` is NOT this, and reading
    /// the lock through it is what made this probe's headline verdict
    /// unfalsifiable.** `CGEventSourceKeyState` answers *is that KEY down*,
    /// and Caps is a momentary key: it is down for the instant of the press
    /// and up again long before the probe samples it a tick later. So it read
    /// `false` before and `false` after **whatever suppression did**, and
    /// `after == caps_before` -- the whole test -- was a tautology. The trace
    /// columns below say the same thing from the other side: neither keyState
    /// column ever flipped, which was recorded as "no discriminator" when it
    /// was really "wrong instrument".
    ///
    /// The lock is the `alphaShift` bit of the source's FLAGS state, which is
    /// level rather than momentary. `kCGEventSourceStateHIDSystemState` = 1.
    fn caps_locked() -> bool {
        unsafe { CGEventSourceFlagsState(1) & FLAG_ALPHA_SHIFT != 0 }
    }

    /// Which arm this run is: does it swallow the Caps press, or let it
    /// through?
    ///
    /// **The `pass` arm is the missing control**, and without it the `swallow`
    /// arm cannot fail: a reader that never moves and a lock that never moves
    /// print the same line. `pass` must show the lock CHANGING; a `pass` run
    /// where it does not means the reader is blind and the `swallow` verdict
    /// beside it is worth nothing.
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Arm {
        Swallow,
        Pass,
    }

    /// The lock, sampled once per tick from the arming tick onward.
    ///
    /// **A before/after pair is not enough, and this is the second reason the
    /// old verdict could not fail:** the driver presses Caps TWICE, so a lock
    /// that toggled on both presses is back where it started by the time an
    /// "after" sample is taken. Only the sequence can tell "never moved" from
    /// "moved and came back".
    static LOCK_SAMPLES: std::sync::Mutex<Vec<bool>> = std::sync::Mutex::new(Vec::new());

    fn sample_lock() {
        if let Ok(mut s) = LOCK_SAMPLES.lock() {
            s.push(caps_locked());
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

        let want_arm = std::env::args().nth(2).unwrap_or_else(|| "swallow".into());
        let arm = match want_arm.as_str() {
            "swallow" => Arm::Swallow,
            "pass" => Arm::Pass,
            other => {
                say(&format!("unknown arm `{other}`; use swallow|pass"));
                std::process::exit(2);
            }
        };
        say(&format!(
            "arm                  : {}",
            match arm {
                Arm::Swallow => "swallow -- return NULL for Caps (the thing under test)",
                Arm::Pass => "pass -- let Caps through (the CONTROL; the lock MUST move)",
            }
        ));

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
                        sample_lock();
                        say(&format!("caps lock before the test press: {caps_before}"));
                        SWALLOW.store(arm == Arm::Swallow, Ordering::SeqCst);
                        say(match arm {
                            Arm::Swallow => "swallow armed -- driver may press Caps",
                            Arm::Pass => "pass armed (control) -- driver may press Caps",
                        });
                    }
                    // The driver injects Caps twice, at ticks 4 and 5. The
                    // lock is sampled after each, because two presses put a
                    // toggling lock back where it started.
                    4 | 5 => sample_lock(),
                    6 => {
                        sample_lock();
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
                        let samples = LOCK_SAMPLES.lock().map(|s| s.clone()).unwrap_or_default();
                        let moved = samples.iter().any(|&s| s != caps_before);
                        say(&format!(
                            "caps lock samples : {:?}  (before: {caps_before})",
                            samples
                        ));
                        say(&format!("lock MOVED at any point: {moved}"));
                        let mut exit = 0;
                        match arm {
                            Arm::Swallow => {
                                say(&format!(
                                    "SUPPRESSION STOPS THE LOCK: {}",
                                    if !SUPPRESSED_CAPS.load(Ordering::SeqCst) {
                                        "untested, nothing was swallowed"
                                    } else if moved {
                                        "NO -- the lock toggled anyway"
                                    } else {
                                        "YES -- the lock never moved"
                                    }
                                ));
                                say("  Read against a `pass` run in the same session.");
                                say("  On its own this line cannot fail: a lock that");
                                say("  never moves and a reader that cannot see it");
                                say("  print the same words.");
                            }
                            Arm::Pass => {
                                say(&format!(
                                    "CONTROL -- the lock moves when Caps is NOT swallowed: {}",
                                    if moved { "YES" } else { "NO" }
                                ));
                                if !moved {
                                    say("  The reader is blind, or the driver never pressed Caps.");
                                    say("  Any `swallow` verdict measured with it says nothing.");
                                    exit = 8;
                                }
                            }
                        }
                        say(&format!(
                            "TAP DIED          : {}",
                            TAP_DIED.load(Ordering::SeqCst)
                        ));
                        say("");
                        say("--- every event, in order ---");
                        if let Ok(t) = TRACE.lock() {
                            for (idx, (ty, c, f, comb, hid)) in t.iter().enumerate() {
                                say(&format!(
                                    "  {idx:>2}  {:<22} code={:<3} flags={:#010x} alpha={} keyState comb={} hid={}",
                                    type_name(*ty),
                                    c,
                                    f,
                                    f & FLAG_ALPHA_SHIFT != 0,
                                    comb,
                                    hid
                                ));
                            }
                            say(&format!("  ({} events traced)", t.len()));
                        }
                        say("");
                        say("DONE");
                        std::process::exit(exit);
                    }
                    _ if n > 14 => {
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
