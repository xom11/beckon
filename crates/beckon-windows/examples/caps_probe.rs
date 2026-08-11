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
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::*;
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

    /// Executable name of whatever currently owns the foreground window.
    /// The Start menu on Windows 11 is a window of `StartMenuExperienceHost`,
    /// so this is how "did Start open?" gets answered without a person.
    fn foreground_exe() -> String {
        unsafe {
            let hwnd = GetForegroundWindow();
            if hwnd.0.is_null() {
                return "(none)".into();
            }
            let mut pid = 0u32;
            GetWindowThreadProcessId(hwnd, Some(&mut pid));
            let Ok(h) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
                return format!("(pid {pid}, no access)");
            };
            let mut buf = [0u16; 260];
            let mut len = buf.len() as u32;
            let ok = QueryFullProcessImageNameW(
                h,
                PROCESS_NAME_FORMAT(0),
                windows::core::PWSTR(buf.as_mut_ptr()),
                &mut len,
            )
            .is_ok();
            let _ = CloseHandle(h);
            if !ok {
                return format!("(pid {pid})");
            }
            let full = String::from_utf16_lossy(&buf[..len as usize]);
            full.rsplit('\\').next().unwrap_or(&full).to_string()
        }
    }

    fn opened_start(before: &str) -> (bool, String) {
        std::thread::sleep(Duration::from_millis(700));
        let after = foreground_exe();
        let changed = after.eq_ignore_ascii_case("StartMenuExperienceHost.exe")
            || (after != before && after.eq_ignore_ascii_case("SearchHost.exe"));
        (changed, after)
    }

    /// Close the Start menu if the control opened it, so the machine is left
    /// as it was found.
    fn dismiss_start() {
        send(&[stroke(VK_ESCAPE.0, false), stroke(VK_ESCAPE.0, true)]);
        std::thread::sleep(Duration::from_millis(400));
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
        println!("== measurement 2: does the one-burst chord open the Start menu? ==");

        // CONTROL FIRST. A probe that cannot detect the Start menu at all
        // produces the same clean output as a design that never opens it,
        // so prove the detector fires before trusting it not to.
        let base = foreground_exe();
        println!("   foreground before: {base}");
        send(&[stroke(VK_LWIN.0, false), stroke(VK_LWIN.0, true)]);
        let (control_fired, seen) = opened_start(&base);
        println!(
            "   CONTROL (bare Win tap): {} -> foreground {seen}",
            if control_fired {
                "Start opened, detector works"
            } else {
                "Start did NOT open - DETECTOR IS BLIND, ignore RESULT 2 below"
            }
        );
        if control_fired {
            dismiss_start();
        }

        let base = foreground_exe();
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
        let (burst_opened, seen) = opened_start(&base);
        println!(
            "   RESULT 2: {} (foreground now {seen})",
            if burst_opened {
                "FAIL - the burst opens Start; caps::chord needs a filler key"
            } else {
                "PASS - the burst does not open Start"
            }
        );
        if burst_opened {
            dismiss_start();
        }

        println!();
        println!("== measurements 3 and 5: not answerable here ==");
        println!("   3 (does swallowing Caps stop the toggle) needs the hook itself,");
        println!("     so it is checked on hardware once the hook exists.");
        println!("   5 (UIPI) needs an elevated window focused, which needs a UAC");
        println!("     consent a scheduled task cannot give. Documented, not measured.");
    }
}
