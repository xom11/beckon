//! Does `caps_tap`'s caps-lock toggle actually move the lock?
//!
//! It exists because the answer used to be NO and nothing could see it. The
//! `caps_tap = "capslock"` option injected `kVK_CapsLock` with `CGEventPost`,
//! which macOS ignores — measured 2026-08-17 at BOTH tap levels, with
//! `AXIsProcessTrusted = 1` so it was not the silent no-op an untrusted post
//! gives. The user ticked the box, beckon swallowed the key, and the lock
//! never moved.
//!
//! This drives the real function rather than a copy of its FFI, because a
//! copy is exactly what would keep passing after the real one broke.
//!
//! ```text
//! cargo run -p beckon-macos --example capslock_probe
//! ```
//!
//! It reads the lock through `IOHIDGetModifierLockState`, toggles, reads
//! again, and puts it back. **The control is the read**: if before and after
//! are equal the toggle did nothing, and if the reader is stuck the two
//! reads are equal too — so it also asserts that a second toggle returns the
//! lock to where it started, which a stuck reader cannot fake.

fn main() {
    #[cfg(not(target_os = "macos"))]
    {
        eprintln!("capslock_probe is macOS-only");
        std::process::exit(2);
    }
    #[cfg(target_os = "macos")]
    mac::run();
}

#[cfg(target_os = "macos")]
mod mac {
    use std::ffi::c_void;

    #[link(name = "IOKit", kind = "framework")]
    unsafe extern "C" {
        fn IOServiceMatching(name: *const i8) -> *mut c_void;
        fn IOServiceGetMatchingService(main_port: u32, matching: *mut c_void) -> u32;
        fn IOServiceOpen(service: u32, owner: u32, typ: u32, connect: *mut u32) -> i32;
        fn IOServiceClose(connect: u32) -> i32;
        fn IOObjectRelease(object: u32) -> i32;
        fn IOHIDGetModifierLockState(handle: u32, selector: i32, state: *mut bool) -> i32;
        static mach_task_self_: u32;
    }

    /// Read-only, and deliberately a SEPARATE path from the one under test:
    /// the probe must not share the writer's mistake.
    fn read_lock() -> Option<bool> {
        unsafe {
            let matching = IOServiceMatching(c"IOHIDSystem".as_ptr());
            if matching.is_null() {
                return None;
            }
            let svc = IOServiceGetMatchingService(0, matching);
            if svc == 0 {
                return None;
            }
            let mut conn = 0u32;
            let kr = IOServiceOpen(svc, mach_task_self_, 1, &mut conn);
            IOObjectRelease(svc);
            if kr != 0 || conn == 0 {
                return None;
            }
            let mut st = false;
            let ok = IOHIDGetModifierLockState(conn, 1, &mut st) == 0;
            IOServiceClose(conn);
            if ok {
                Some(st)
            } else {
                None
            }
        }
    }

    pub fn run() {
        let Some(start) = read_lock() else {
            println!("FAIL  cannot read the lock at all — every verdict below would be vacuous");
            std::process::exit(1);
        };
        println!("lock at start      = {start}");

        beckon_macos::caps_tap::toggle_caps_lock_for_probe();
        let once = read_lock();
        println!("after one toggle   = {once:?}");

        beckon_macos::caps_tap::toggle_caps_lock_for_probe();
        let twice = read_lock();
        println!("after two toggles  = {twice:?}");

        let moved = once == Some(!start);
        let returned = twice == Some(start);
        println!();
        println!(
            "{}  the toggle moves the lock",
            if moved { "PASS " } else { "FAIL " }
        );
        println!(
            "{}  two toggles return it (a stuck reader cannot fake this)",
            if returned { "PASS " } else { "FAIL " }
        );
        if !(moved && returned) {
            std::process::exit(1);
        }
    }
}
