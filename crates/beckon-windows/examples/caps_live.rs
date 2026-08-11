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
        //
        // `fast` releases Caps in the same breath as the key, which is what
        // a person actually does. It matters because `backend.beckon()`
        // takes 57 ms typically and up to 945 ms on the miss path, and it
        // pumps this thread's message queue while it runs — so a quick
        // release lands *inside* that window, where a leisurely synthetic
        // one never does.
        let fast = std::env::args().any(|a| a == "--fast");
        let gap = if fast { 15 } else { 120 };
        println!("chord timing: {}ms between strokes", gap);
        let base = foreground_exe();
        send(&[stroke(VK_CAPITAL.0, false)]);
        std::thread::sleep(Duration::from_millis(gap));
        send(&[stroke(vk, false)]);
        std::thread::sleep(Duration::from_millis(gap));
        send(&[stroke(vk, true)]);
        std::thread::sleep(Duration::from_millis(gap));
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

        // --- Did the sequence leave anything held? ---
        //
        // Two failure modes look identical from the keyboard but need
        // opposite fixes, so name them apart here:
        //   (a) the injected modifier key-ups were lost, so the system still
        //       believes ctrl+win+alt are down; or
        //   (b) the hook's own `held` flag is stuck, so it keeps treating
        //       every key as if Caps were down.
        std::thread::sleep(Duration::from_millis(500));
        let stuck = modifiers_down();
        println!(
            "STUCK MODIFIERS: {}",
            if stuck.is_empty() {
                "none".to_string()
            } else {
                stuck.join(", ")
            }
        );

        // Now press the chord key ALONE. If it still beckons, the hook
        // thinks Caps is held (b). If nothing happens and no modifier is
        // stuck, the sequence was clean.
        //
        // The target is in front after a successful chord, which would make
        // "did the foreground become the target" unanswerable — so get it
        // out of the way first. Minimising is enough and needs no rights
        // over another process's focus.
        if foreground_exe().eq_ignore_ascii_case(&want) {
            unsafe {
                let fg = GetForegroundWindow();
                let _ = ShowWindow(fg, SW_MINIMIZE);
            }
            std::thread::sleep(Duration::from_millis(800));
        }
        let base = foreground_exe();
        if base.eq_ignore_ascii_case(&want) {
            println!("BARE KEY: skipped, could not move {want} out of the way");
        } else {
            send(&[stroke(vk, false), stroke(vk, true)]);
            let mut seen = base.clone();
            for _ in 0..12 {
                std::thread::sleep(Duration::from_millis(300));
                seen = foreground_exe();
                if seen.eq_ignore_ascii_case(&want) {
                    break;
                }
            }
            println!(
                "BARE KEY (no Caps): foreground {base} -> {seen} ({})",
                if seen.eq_ignore_ascii_case(&want) {
                    "BUG REPRODUCED - a bare key still beckons"
                } else {
                    "clean - a bare key does nothing"
                }
            );
        }

        println!("caps lock at exit: {}", caps_state());
    }

    /// Which modifiers the system currently believes are held.
    fn modifiers_down() -> Vec<String> {
        [
            (VK_CONTROL.0, "ctrl"),
            (VK_LCONTROL.0, "lctrl"),
            (VK_MENU.0, "alt"),
            (VK_LMENU.0, "lalt"),
            (VK_LWIN.0, "lwin"),
            (VK_SHIFT.0, "shift"),
        ]
        .iter()
        .filter(|(vk, _)| (unsafe { GetAsyncKeyState(*vk as i32) } as u16 & 0x8000) != 0)
        .map(|(_, n)| n.to_string())
        .collect()
    }
}
