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
    use windows::Win32::UI::Input::KeyboardAndMouse::IsWindowEnabled;
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

    const IDC_COMBO: i32 = 1002;
    const IDC_APP: i32 = 1003;
    const IDC_ADD: i32 = 1005;
    const IDC_APPLY: i32 = 1007;

    fn dlg_item(parent: HWND, id: i32) -> Option<HWND> {
        match unsafe { GetDlgItem(Some(parent), id) } {
            Ok(h) if !h.0.is_null() => Some(h),
            _ => None,
        }
    }

    fn click(parent: HWND, id: i32) {
        let Some(ctl) = dlg_item(parent, id) else {
            println!("    (no control {id})");
            return;
        };
        unsafe {
            let _ = PostMessageW(
                Some(parent),
                WM_COMMAND,
                WPARAM(((BN_CLICKED as usize) << 16) | (id as usize & 0xFFFF)),
                LPARAM(ctl.0 as isize),
            );
        }
        std::thread::sleep(Duration::from_millis(400));
    }

    /// `WM_SETTEXT` is one of the few messages USER32 marshals across a
    /// process boundary, which is what makes driving another process's edit
    /// fields possible at all.
    fn set_text(h: HWND, s: &str) {
        let mut buf: Vec<u16> = s.encode_utf16().chain(std::iter::once(0)).collect();
        unsafe {
            SendMessageW(
                h,
                WM_SETTEXT,
                Some(WPARAM(0)),
                Some(LPARAM(buf.as_mut_ptr() as isize)),
            );
        }
        std::thread::sleep(Duration::from_millis(250));
    }

    fn first_child_of_class(parent: HWND, class: &str) -> Option<HWND> {
        KIDS.with(|k| k.borrow_mut().clear());
        let mut found: Option<HWND> = None;
        let mut child = unsafe { GetWindow(parent, GW_CHILD) }.ok();
        while let Some(c) = child {
            if c.0.is_null() {
                break;
            }
            if class_of(c).eq_ignore_ascii_case(class) {
                found = Some(c);
                break;
            }
            child = unsafe { GetWindow(c, GW_HWNDNEXT) }.ok();
        }
        found
    }

    /// Add a row, fill it in, and press Apply. Whether the file changed is
    /// checked by the caller — this only proves the events reach the model.
    fn drive_an_edit(h: HWND) {
        println!("  -- driving an edit --");
        click(h, IDC_ADD);

        let Some(combo_edit) = dlg_item(h, IDC_COMBO) else {
            println!("    FAIL: no shortcut field");
            return;
        };
        set_text(combo_edit, "ctrl+super+alt+j");

        // The App control is a COMBOBOX; its text lives in a child EDIT, and
        // only that child raises the change notification the window listens
        // for. Setting the combo itself is silent.
        let app_edit = dlg_item(h, IDC_APP).and_then(|c| first_child_of_class(c, "Edit"));
        match app_edit {
            Some(e) => set_text(e, "Notepad"),
            None => println!("    FAIL: combo box has no edit child"),
        }

        let apply = dlg_item(h, IDC_APPLY);
        let enabled = apply.map(|a| unsafe { IsWindowEnabled(a) }.as_bool());
        println!("    Apply enabled after typing: {enabled:?}");
        if enabled == Some(true) {
            click(h, IDC_APPLY);
            std::thread::sleep(Duration::from_millis(900));
            println!("    Apply clicked");
        } else {
            println!("    FAIL: Apply stayed disabled -- the edits never reached the model");
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

        drive_an_edit(h);

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
