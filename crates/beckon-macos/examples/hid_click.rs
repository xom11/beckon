//! Post a left click at a point, through the window server.
//!
//! ```text
//! cargo run -p beckon-macos --example hid_click -- <x> <y>
//! ```
//!
//! Coordinates are **CoreGraphics global display coordinates**: origin
//! top-left, y growing downward. That is the opposite of every AppKit screen
//! coordinate, and the caller is expected to have flipped already —
//! `loop_probe` prints the flipped pair on its `CLICK-AT` line for exactly
//! this reason.
//!
//! ## Why this is a separate binary
//!
//! Because the two capabilities it needs live in different processes on this
//! machine, and that split is measured rather than assumed:
//!
//! - **Drawing** needs the `Aqua` bootstrap namespace. A shell under an SSH
//!   session or a coding agent is in `Background`, where AppKit hands back
//!   live objects and draws nothing — so `loop_probe` has to be launched
//!   through Terminal.app.
//! - **Posting an event** needs an Accessibility grant, and TCC attributes
//!   it to the *responsible* process. Measured 2026-08-16: a `CGEventPost`
//!   from the Terminal-launched probe did not move the button even in the
//!   `nsapp` mode where an in-process `postEvent:atStart:` did — i.e. the
//!   injector was unprivileged, silently, with no error returned anywhere.
//!   The agent's own shell answers `AXIsProcessTrusted() == true`
//!   (`beckon doctor` reports it), so it can post.
//!
//! One process cannot currently be both. Splitting the roles is what makes
//! the measurement possible at all: the probe draws and waits, this posts.
//!
//! A silent no-op is this API's failure mode — `CGEventPost` returns `void`
//! and reports nothing when the caller is untrusted — so this binary prints
//! `AXIsProcessTrusted()` first. **Read that line before believing a
//! negative result**; without it, "not trusted" and "the click missed" look
//! identical.

fn main() {
    #[cfg(not(target_os = "macos"))]
    {
        eprintln!("hid_click is macOS-only");
        std::process::exit(2);
    }
    #[cfg(target_os = "macos")]
    mac::run();
}

#[cfg(target_os = "macos")]
mod mac {
    use std::ffi::c_void;

    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct CGPoint {
        pub x: f64,
        pub y: f64,
    }

    #[link(name = "CoreGraphics", kind = "framework")]
    unsafe extern "C" {
        fn CGEventCreateMouseEvent(
            source: *const c_void,
            mouse_type: u32,
            pos: CGPoint,
            button: u32,
        ) -> *mut c_void;
        fn CGEventPost(tap: u32, event: *mut c_void);
        fn CGWarpMouseCursorPosition(pos: CGPoint) -> i32;
    }
    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CFRelease(cf: *mut c_void);
    }
    #[link(name = "ApplicationServices", kind = "framework")]
    unsafe extern "C" {
        fn AXIsProcessTrusted() -> bool;
    }

    const LEFT_DOWN: u32 = 1;
    const LEFT_UP: u32 = 2;
    const HID_TAP: u32 = 0;

    pub fn run() {
        let args: Vec<String> = std::env::args().skip(1).collect();
        if args.len() != 2 {
            eprintln!("usage: hid_click <x> <y>   (CoreGraphics display coords, y from top)");
            std::process::exit(2);
        }
        let (x, y) = match (args[0].parse::<f64>(), args[1].parse::<f64>()) {
            (Ok(a), Ok(b)) => (a, b),
            _ => {
                eprintln!("both arguments must be numbers");
                std::process::exit(2);
            }
        };

        let trusted = unsafe { AXIsProcessTrusted() };
        println!("AXIsProcessTrusted : {trusted}");
        if !trusted {
            println!("REFUSING: an untrusted CGEventPost is a silent no-op, and a silent");
            println!("no-op is indistinguishable from a click that missed. Grant this");
            println!("process's terminal Accessibility, or the result proves nothing.");
            std::process::exit(3);
        }

        let p = CGPoint { x, y };
        unsafe {
            // Move the pointer first. A synthesised down/up at a location the
            // cursor is not at still reaches the window under that point, but
            // moving makes what happened visible to anyone watching, and it
            // matches what a real click does.
            CGWarpMouseCursorPosition(p);
            for kind in [LEFT_DOWN, LEFT_UP] {
                let ev = CGEventCreateMouseEvent(std::ptr::null(), kind, p, 0);
                if ev.is_null() {
                    println!("CGEventCreateMouseEvent returned null");
                    std::process::exit(1);
                }
                CGEventPost(HID_TAP, ev);
                CFRelease(ev);
            }
        }
        println!("POSTED: left click at CG ({x:.0},{y:.0})");
    }
}
