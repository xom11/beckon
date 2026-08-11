//! Throwaway probe for the Caps-as-beckon-key design. Delete once its
//! answers are recorded in the plan.
//!
//! Run in an INTERACTIVE session:
//!
//!     cargo run --example caps_probe
//!
//! Session 0 has no desktop and no keyboard; hotkeys never fire there and
//! every answer below would be a confident false negative. SSH into a14
//! lands in session 0 -- go through a scheduled task instead, with
//! `-EncodedCommand` to avoid quoting damage.
//!
//! Answers measurements 1, 4 and 6 from the spec programmatically.
//! Measurements 2 (Start menu), 3 (does swallowing Caps stop the toggle)
//! and 5 (elevated window focus) need a person looking at the screen; the
//! probe prints what to look for.

fn main() {
    #[cfg(not(target_os = "windows"))]
    eprintln!("caps_probe only does anything on Windows");
    #[cfg(target_os = "windows")]
    win::run();
}

#[cfg(target_os = "windows")]
mod win {
    use std::time::{Duration, Instant};
    use windows::Win32::UI::Input::KeyboardAndMouse::*;
    use windows::Win32::UI::WindowsAndMessaging::*;

    const ID: i32 = 0xBEC0;
    const MARK: usize = 0xBECC0DE;
    /// f19 is in beckon's key table and is not a shell hotkey, so nothing
    /// else on the machine is likely to be holding it.
    const VK_F19: u16 = 0x82;

    fn stroke(vk: u16, up: bool) -> INPUT {
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VIRTUAL_KEY(vk),
                    wScan: 0,
                    dwFlags: if up {
                        KEYEVENTF_KEYUP
                    } else {
                        KEYBD_EVENT_FLAGS(0)
                    },
                    time: 0,
                    dwExtraInfo: MARK,
                },
            },
        }
    }

    fn send(strokes: &[INPUT]) {
        unsafe {
            SendInput(strokes, std::mem::size_of::<INPUT>() as i32);
        }
    }

    fn drain_for(ms: u64, id: i32) -> bool {
        let mut fired = false;
        let deadline = Instant::now() + Duration::from_millis(ms);
        while Instant::now() < deadline {
            let mut msg = MSG::default();
            while unsafe { PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE) }.as_bool() {
                if msg.message == WM_HOTKEY && msg.wParam.0 == id as usize {
                    fired = true;
                }
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        fired
    }

    pub fn run() {
        println!("beckon caps_probe");
        println!("=================");
        println!();
        println!("== measurement 1: does a SendInput chord trigger our OWN RegisterHotKey? ==");
        println!("   This is the load-bearing assumption of the whole alias design.");

        unsafe {
            RegisterHotKey(None, ID, MOD_CONTROL | MOD_WIN | MOD_ALT, VK_F19 as u32)
                .expect("RegisterHotKey(ctrl+win+alt+f19) failed");
        }

        let t0 = Instant::now();
        send(&[
            stroke(VK_LCONTROL.0, false),
            stroke(VK_LWIN.0, false),
            stroke(VK_LMENU.0, false),
            stroke(VK_F19, false),
            stroke(VK_F19, true),
            stroke(VK_LMENU.0, true),
            stroke(VK_LWIN.0, true),
            stroke(VK_LCONTROL.0, true),
        ]);
        let inject_us = t0.elapsed().as_micros();
        let fired = drain_for(1000, ID);

        unsafe {
            let _ = UnregisterHotKey(None, ID);
        }

        println!(
            "   RESULT 1: {}",
            if fired {
                "PASS - the injected chord fired our hotkey"
            } else {
                "FAIL - DESIGN CHANGE REQUIRED, see the plan's Task 6 step 5"
            }
        );
        println!("   RESULT 6: SendInput of 8 strokes took {inject_us} us");
        println!("             (LowLevelHooksTimeout budget is 300000 us)");

        println!();
        println!("== measurement 4: does an injected VK_CAPITAL toggle Caps Lock? ==");
        let before = unsafe { GetKeyState(VK_CAPITAL.0 as i32) } & 1;
        send(&[stroke(VK_CAPITAL.0, false), stroke(VK_CAPITAL.0, true)]);
        std::thread::sleep(Duration::from_millis(150));
        let after = unsafe { GetKeyState(VK_CAPITAL.0 as i32) } & 1;
        println!(
            "   RESULT 4: {} (state before={before}, after={after})",
            if before != after {
                "PASS - flipped, so caps_tap = \"capslock\" is implementable"
            } else {
                "FAIL - caps_tap = \"capslock\" cannot be implemented this way"
            }
        );
        if before != after {
            // Put it back the way we found it.
            send(&[stroke(VK_CAPITAL.0, false), stroke(VK_CAPITAL.0, true)]);
        }

        println!();
        println!("== measurements 2, 3 and 5: watch the screen, then write down what happened ==");
        println!();
        println!("   2 (Start menu): press ctrl+win+alt+f19 BY HAND. If the Start menu");
        println!("     opens, the one-burst chord is not enough on its own and");
        println!("     caps::chord needs a filler key.");
        println!();
        println!("   3 (Caps swallowed): needs the hook itself, so it is checked on");
        println!("     hardware in Task 7, not here.");
        println!();
        println!("   5 (UIPI): open Task Manager AS ADMIN, click into it, then repeat");
        println!("     measurement 2. Expect nothing to happen -- that confirms the");
        println!("     documented gap rather than assuming it.");
    }
}
