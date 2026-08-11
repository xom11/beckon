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

    /// Type into a control the way a person does: clear it, then one
    /// `WM_CHAR` per character.
    ///
    /// `WM_SETTEXT` alone is not enough and the difference is the point. An
    /// EDIT raises `EN_CHANGE` for a programmatic set, but a COMBOBOX only
    /// forwards `CBN_EDITCHANGE` for input it processed itself — so a
    /// `WM_SETTEXT` test would pass on the Shortcut field and fail on the
    /// App field for a reason that has nothing to do with beckon.
    fn type_into(h: HWND, s: &str) {
        let mut empty: Vec<u16> = vec![0];
        unsafe {
            SendMessageW(
                h,
                WM_SETTEXT,
                Some(WPARAM(0)),
                Some(LPARAM(empty.as_mut_ptr() as isize)),
            );
            for ch in s.encode_utf16() {
                SendMessageW(h, WM_CHAR, Some(WPARAM(ch as usize)), Some(LPARAM(1)));
            }
        }
        std::thread::sleep(Duration::from_millis(300));
    }

    /// Dismiss a modal dialog by clicking one of its buttons.
    /// `#32770` is the system dialog class, which is what `MessageBox` uses.
    fn dismiss_dialog(button: i32) -> bool {
        for _ in 0..20 {
            std::thread::sleep(Duration::from_millis(250));
            if let Ok(dlg) = unsafe { FindWindowW(w!("#32770"), None) } {
                if !dlg.0.is_null() {
                    println!("    modal dialog present: {:?}", text_of(dlg));
                    unsafe {
                        let _ = PostMessageW(
                            Some(dlg),
                            WM_COMMAND,
                            WPARAM(button as usize),
                            LPARAM(0),
                        );
                    }
                    return true;
                }
            }
        }
        false
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
    const IDC_NOTES: i32 = 1004;

    /// The notes pane is rendered from the model, so its text is the
    /// cheapest window into whether an event actually landed.
    fn dump(h: HWND, label: &str) {
        // Only the notes pane is readable from here. GetWindowText on a
        // control in ANOTHER process returns the window caption from the
        // kernel structure rather than sending WM_GETTEXT, so an EDIT or a
        // COMBOBOX always reads back empty -- a STATIC does not, because
        // its text IS its caption.
        let notes = dlg_item(h, IDC_NOTES).map(text_of).unwrap_or_default();
        let apply = dlg_item(h, IDC_APPLY)
            .map(|a| unsafe { IsWindowEnabled(a) }.as_bool())
            .unwrap_or(false);
        println!("    [{label}] apply={apply}");
        println!("      notes: {}", notes.replace('\r', "").replace('\n', " | "));
    }

    fn drive_an_edit(h: HWND) {
        println!("  -- driving an edit --");
        dump(h, "start");
        click(h, IDC_ADD);
        dump(h, "after Add");

        let Some(combo_edit) = dlg_item(h, IDC_COMBO) else {
            println!("    FAIL: no shortcut field");
            return;
        };
        type_into(combo_edit, "ctrl+super+alt+j");
        dump(h, "after shortcut text");

        // The App control is a COMBOBOX; its text lives in a child EDIT, and
        // only that child raises the change notification the window listens
        // for. Setting the combo itself is silent.
        let app_edit = dlg_item(h, IDC_APP).and_then(|c| first_child_of_class(c, "Edit"));
        match app_edit {
            Some(e) => type_into(e, "Notepad"),
            None => println!("    FAIL: combo box has no edit child"),
        }
        dump(h, "after app text");

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

        // Leave the machine as it was found. If the model is still dirty
        // the window asks before closing -- that prompt is a feature, so
        // answer it rather than treating it as a hang.
        std::thread::sleep(Duration::from_millis(500));
        unsafe {
            let _ = PostMessageW(Some(h), WM_CLOSE, WPARAM(0), LPARAM(0));
        }
        std::thread::sleep(Duration::from_millis(600));
        if find_settings().is_some() {
            // IDNO = 7: discard, so the probe never leaves an edit behind.
            if dismiss_dialog(7) {
                println!("    (answered the unsaved-changes prompt with Discard)");
            }
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
