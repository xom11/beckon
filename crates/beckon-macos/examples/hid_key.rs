//! Press a chord, through the window server.
//!
//! ```text
//! cargo run -p beckon-macos --example hid_key -- <keycode> [ctrl] [cmd] [opt] [shift]
//! cargo run -p beckon-macos --example hid_key -- 3 ctrl opt shift   # ctrl+alt+shift+f
//! ```
//!
//! `hid_click`'s twin, and it exists for the same reason: the direct test of
//! a hotkey is a real press, and a real press comes from the window server.
//! `CGEventPost(kCGHIDEventTap, …)` is that path.
//!
//! ## Keycode, never a character
//!
//! The argument is a **Carbon virtual keycode** (`kVK_ANSI_F` = 3), the same
//! number `beckon_core::shortcuts::KeyDef::mac` carries and the same number
//! `RegisterEventHotKey` was given. Asking for a character instead — the way
//! `System Events`' `keystroke` does — makes the OS find whichever key
//! produces that character on the *current layout*, which on a non-US layout
//! is a different key entirely, and the hotkey was registered against a key.
//!
//! ## It prints its own trust state first
//!
//! `CGEventPost` returns `void` and does nothing at all when the calling
//! process is not Accessibility-trusted. A silent no-op is indistinguishable
//! from a press that landed somewhere else, which is how four earlier
//! attempts on this branch produced confident wrong readings. **Read the
//! `AXIsProcessTrusted` line before believing a negative result.**

fn main() {
    #[cfg(not(target_os = "macos"))]
    {
        eprintln!("hid_key is macOS-only");
        std::process::exit(2);
    }
    #[cfg(target_os = "macos")]
    mac::run();
}

#[cfg(target_os = "macos")]
mod mac {
    use std::ffi::c_void;

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
        fn CFRelease(cf: *mut c_void);
    }
    #[link(name = "ApplicationServices", kind = "framework")]
    unsafe extern "C" {
        fn AXIsProcessTrusted() -> bool;
    }

    const HID_TAP: u32 = 0;
    // The modifier keys themselves, from HIToolbox Events.h.
    const K_SHIFT: u16 = 0x38;
    const K_CONTROL: u16 = 0x3B;
    const K_OPTION: u16 = 0x3A;
    const K_COMMAND: u16 = 0x37;

    /// One key event, posted where the window server can see it.
    unsafe fn post(code: u16, down: bool, flags: u64) {
        unsafe {
            let ev = CGEventCreateKeyboardEvent(std::ptr::null(), code, down);
            if ev.is_null() {
                println!("CGEventCreateKeyboardEvent returned null");
                std::process::exit(1);
            }
            CGEventSetFlags(ev, flags);
            CGEventPost(HID_TAP, ev);
            CFRelease(ev);
        }
    }
    // `CGEventFlags`, from CGEventTypes.h.
    const SHIFT: u64 = 0x0002_0000;
    const CONTROL: u64 = 0x0004_0000;
    const ALTERNATE: u64 = 0x0008_0000;
    const COMMAND: u64 = 0x0010_0000;

    pub fn run() {
        let args: Vec<String> = std::env::args().skip(1).collect();
        if args.is_empty() {
            eprintln!("usage: hid_key <keycode> [ctrl] [cmd] [opt] [shift]");
            std::process::exit(2);
        }
        let Ok(code) = args[0].parse::<u16>() else {
            eprintln!("the first argument must be a Carbon virtual keycode");
            std::process::exit(2);
        };
        let mut flags = 0u64;
        for m in &args[1..] {
            match m.as_str() {
                "ctrl" => flags |= CONTROL,
                "cmd" | "super" => flags |= COMMAND,
                "opt" | "alt" => flags |= ALTERNATE,
                "shift" => flags |= SHIFT,
                other => {
                    eprintln!("unknown modifier `{other}`");
                    std::process::exit(2);
                }
            }
        }

        let trusted = unsafe { AXIsProcessTrusted() };
        println!("AXIsProcessTrusted : {trusted}");
        if !trusted {
            println!("REFUSING: an untrusted CGEventPost does nothing and says nothing,");
            println!("so a silent result would prove only that this process cannot type.");
            std::process::exit(3);
        }

        // **The modifiers are pressed as REAL KEYS, not just as flags on the
        // main key.** Measured 2026-08-16: a `CGEventCreateKeyboardEvent`
        // carrying `CGEventSetFlags(ctrl|opt|shift)` and nothing else was
        // posted successfully — `AXIsProcessTrusted: true`, `POSTED` — and
        // did **not** fire a `RegisterEventHotKey` chord under EITHER loop,
        // including the Carbon one that demonstrably delivers hotkeys in
        // production. The baseline failing is what said the injector was
        // wrong rather than the thing under test.
        //
        // The system tracks modifier state from `kVK_Control` &co. key
        // events; the flags field describes an event, it does not hold a key
        // down. So the sequence is the one a hand makes: modifiers down (each
        // carrying the flags accumulated so far), the key down and up, then
        // the modifiers up in reverse.
        let mut pressed: Vec<(u16, u64)> = Vec::new();
        if flags & CONTROL != 0 {
            pressed.push((K_CONTROL, CONTROL));
        }
        if flags & ALTERNATE != 0 {
            pressed.push((K_OPTION, ALTERNATE));
        }
        if flags & SHIFT != 0 {
            pressed.push((K_SHIFT, SHIFT));
        }
        if flags & COMMAND != 0 {
            pressed.push((K_COMMAND, COMMAND));
        }

        unsafe {
            let mut acc = 0u64;
            for (k, f) in &pressed {
                acc |= f;
                post(*k, true, acc);
            }
            post(code, true, flags);
            post(code, false, flags);
            for (k, f) in pressed.iter().rev() {
                acc &= !f;
                post(*k, false, acc);
            }
        }
        println!(
            "POSTED: keycode {code} flags {flags:#x} via {} real modifier keys",
            pressed.len()
        );
    }
}
