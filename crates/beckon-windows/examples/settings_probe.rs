//! Live check that the settings window actually builds. Throwaway.
//!
//! Nothing in `settings_window.rs` is reachable from a unit test — it needs
//! a message loop and a tray icon — so this drives it the way a person
//! does: it posts the tray icon's own double-click notification to the
//! running `beckon-serve`, then reads the resulting window back out with
//! `EnumChildWindows`. Reading another process's controls is the only way
//! to prove the layout was built rather than merely that a window appeared.
//!
//! Must run in an INTERACTIVE session, with `beckon-serve` already running.

fn main() {
    #[cfg(not(target_os = "windows"))]
    eprintln!("settings_probe only does anything on Windows");
    #[cfg(target_os = "windows")]
    win::run();
}

#[cfg(target_os = "windows")]
mod win {
    use std::cell::RefCell;
    use std::time::Duration;
    use windows::core::{w, BOOL};
    use windows::Win32::Foundation::{HWND, LPARAM, RECT, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::*;

    const WM_TRAY: u32 = WM_APP + 1;

    thread_local! {
        static KIDS: RefCell<Vec<(String, String, bool)>> = const { RefCell::new(Vec::new()) };
    }

    fn class_of(h: HWND) -> String {
        let mut buf = [0u16; 128];
        let n = unsafe { GetClassNameW(h, &mut buf) };
        String::from_utf16_lossy(&buf[..n.max(0) as usize])
    }

    fn text_of(h: HWND) -> String {
        let mut buf = [0u16; 256];
        let n = unsafe { GetWindowTextW(h, &mut buf) };
        String::from_utf16_lossy(&buf[..n.max(0) as usize])
    }

    unsafe extern "system" fn on_child(h: HWND, _l: LPARAM) -> BOOL {
        let visible = IsWindowVisible(h).as_bool();
        KIDS.with(|k| k.borrow_mut().push((class_of(h), text_of(h), visible)));
        true.into()
    }

    fn find_settings() -> Option<HWND> {
        let h = unsafe { FindWindowW(w!("BeckonSettingsWindow"), None) };
        match h {
            Ok(h) if !h.0.is_null() => Some(h),
            _ => None,
        }
    }

    pub fn run() {
        let tray = match unsafe { FindWindowW(w!("beckon-serve-tray"), None) } {
            Ok(h) if !h.0.is_null() => h,
            _ => {
                println!("FAIL: no beckon-serve-tray window -- is beckon-serve running?");
                return;
            }
        };
        println!("tray window found: {:?}", tray.0);

        if find_settings().is_some() {
            println!("note: a settings window was already open");
        }

        // Exactly what Shell_NotifyIcon posts when the icon is
        // double-clicked.
        unsafe {
            let _ = PostMessageW(
                Some(tray),
                WM_TRAY,
                WPARAM(1),
                LPARAM(WM_LBUTTONDBLCLK as isize),
            );
        }

        let mut win = None;
        for _ in 0..40 {
            std::thread::sleep(Duration::from_millis(250));
            if let Some(h) = find_settings() {
                win = Some(h);
                break;
            }
        }
        let Some(h) = win else {
            println!("FAIL: no BeckonSettingsWindow appeared within 10s");
            return;
        };
        println!("PASS: settings window opened: {:?}", h.0);
        println!("  title:   {:?}", text_of(h));
        println!("  visible: {}", unsafe { IsWindowVisible(h) }.as_bool());
        let mut rc = RECT::default();
        if unsafe { GetWindowRect(h, &mut rc) }.is_ok() {
            println!(
                "  rect:    {}x{} at ({}, {})",
                rc.right - rc.left,
                rc.bottom - rc.top,
                rc.left,
                rc.top
            );
        }

        KIDS.with(|k| k.borrow_mut().clear());
        unsafe {
            let _ = EnumChildWindows(Some(h), Some(on_child), LPARAM(0));
        }
        KIDS.with(|k| {
            let kids = k.borrow();
            println!("  {} child controls:", kids.len());
            for (cls, txt, vis) in kids.iter() {
                println!(
                    "    {:<18} vis={} {:?}",
                    cls,
                    if *vis { "y" } else { "n" },
                    txt
                );
            }
        });

        // Leave the machine as it was found.
        std::thread::sleep(Duration::from_millis(500));
        unsafe {
            let _ = PostMessageW(Some(h), WM_CLOSE, WPARAM(0), LPARAM(0));
        }
        let mut gone = false;
        for _ in 0..20 {
            std::thread::sleep(Duration::from_millis(250));
            if find_settings().is_none() {
                gone = true;
                break;
            }
        }
        println!(
            "  close:   {}",
            if gone {
                "PASS - window closed"
            } else {
                "FAIL - still open after WM_CLOSE"
            }
        );
    }
}
