//! Live end-to-end check of the Caps hook. Throwaway, like `caps_probe`.
//!
//! Run it once with no `beckon-serve` running (the baseline) and once with
//! one running against a config that has `keyboard.caps = true`. The
//! difference between the two runs is the whole result — a single run
//! cannot tell "the hook works" from "Windows would have done that anyway".
//!
//! Must run in an INTERACTIVE session. The keys it sends carry
//! `dwExtraInfo = 0`, deliberately: beckon's hook only ignores strokes
//! stamped with its own marker, so anything from here is indistinguishable
//! from a person typing.
//!
//!     cargo run --example caps_live -- <vk-hex> <expected-exe>
//!
//! e.g. `caps_live -- 4E notepad.exe` for Caps+N.

fn main() {
    #[cfg(not(target_os = "windows"))]
    eprintln!("caps_live only does anything on Windows");
    #[cfg(target_os = "windows")]
    win::run();
}

#[cfg(target_os = "windows")]
mod win {
    use std::time::Duration;
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::*;
    use windows::Win32::UI::Input::KeyboardAndMouse::*;
    use windows::Win32::UI::WindowsAndMessaging::*;

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
                    // NOT beckon's marker: the hook must treat this as real
                    // input, which is the entire point of this program.
                    dwExtraInfo: 0,
                },
            },
        }
    }

    fn send(s: &[INPUT]) {
        unsafe {
            SendInput(s, std::mem::size_of::<INPUT>() as i32);
        }
    }

    fn caps_state() -> i16 {
        (unsafe { GetKeyState(VK_CAPITAL.0 as i32) }) & 1
    }

    fn foreground_exe() -> String {
        unsafe {
            let hwnd = GetForegroundWindow();
            if hwnd.0.is_null() {
                return "(none)".into();
            }
            let mut pid = 0u32;
            GetWindowThreadProcessId(hwnd, Some(&mut pid));
            let Ok(h) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
                return format!("(pid {pid})");
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

    pub fn run() {
        let args: Vec<String> = std::env::args().collect();
        let vk = u16::from_str_radix(args.get(1).map(|s| s.as_str()).unwrap_or("4E"), 16)
            .expect("first argument must be a hex VK code");
        let want = args
            .get(2)
            .cloned()
            .unwrap_or_else(|| "notepad.exe".to_string());

        println!("caps_live: chord key VK 0x{vk:02X}, expecting {want}");
        println!("foreground at start: {}", foreground_exe());

        // --- A bare Caps tap ---
        let before = caps_state();
        send(&[stroke(VK_CAPITAL.0, false), stroke(VK_CAPITAL.0, true)]);
        std::thread::sleep(Duration::from_millis(400));
        let after = caps_state();
        println!("TAP: caps lock {before} -> {after} ({})", {
            if before != after {
                "toggled"
            } else {
                "did NOT toggle"
            }
        });
        if before != after {
            // Leave the machine as it was found.
            send(&[stroke(VK_CAPITAL.0, false), stroke(VK_CAPITAL.0, true)]);
            std::thread::sleep(Duration::from_millis(300));
        }

        // --- Caps held, then the chord key ---
        let base = foreground_exe();
        send(&[stroke(VK_CAPITAL.0, false)]);
        std::thread::sleep(Duration::from_millis(120));
        send(&[stroke(vk, false)]);
        std::thread::sleep(Duration::from_millis(120));
        send(&[stroke(vk, true)]);
        std::thread::sleep(Duration::from_millis(120));
        send(&[stroke(VK_CAPITAL.0, true)]);

        // Launching an app is slower than focusing one; give it room.
        let mut seen = base.clone();
        for _ in 0..30 {
            std::thread::sleep(Duration::from_millis(300));
            seen = foreground_exe();
            if seen.eq_ignore_ascii_case(&want) {
                break;
            }
        }
        println!(
            "CHORD: foreground {base} -> {seen} ({})",
            if seen.eq_ignore_ascii_case(&want) {
                "HIT"
            } else {
                "MISS"
            }
        );

        // Caps Lock may have been left on by the chord if the hook is not
        // running; report the final state so a dirty exit is visible.
        println!("caps lock at exit: {}", caps_state());
    }
}
