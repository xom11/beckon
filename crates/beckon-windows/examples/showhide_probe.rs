//! G2 (spec `2026-08-12-settings-keycaps-design.md`): does `ShowWindow` on a
//! populated `CBS_DROPDOWN` rewrite its edit text the way `SetWindowPos`
//! does?
//!
//! `Ui::shown_external` in `settings_window.rs` records the `SetWindowPos`
//! half as measured. The empty state in spec §A.2 wants to hide and re-show
//! the App combo instead, and nobody has run that.
//!
//! **Runs a control in the same pass.** `SetWindowPos` is the known-bad call;
//! if the control comes back clean too, the probe is blind and its verdict on
//! `ShowWindow` means nothing. Reported as `CONTROL_CORRUPTED`, which MUST be
//! `True` for the run to be worth reading.
//!
//! Build: `cargo build -p beckon-windows --example showhide_probe --all-targets`
//! Run from **session 1** (an SSH shell is session 0 and has no desktop).

fn main() {
    #[cfg(not(target_os = "windows"))]
    eprintln!("showhide_probe only does anything on Windows");
    #[cfg(target_os = "windows")]
    win::run();
}

#[cfg(target_os = "windows")]
mod win {
    use windows::core::*;
    use windows::Win32::Foundation::*;
    use windows::Win32::Graphics::Gdi::*;
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::WindowsAndMessaging::*;

    /// Typed into the edit. A strict prefix of `PREFIX_OF`, so a combo that
    /// re-synchronises has somewhere wrong to go.
    const TYPED: &str = "Note";
    const PREFIX_OF: &str = "Notepad";

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    /// `WM_GETTEXT`, never `GetWindowText`. `GetWindowText` returns the
    /// kernel-side caption and reads back EMPTY for an EDIT or COMBOBOX -- the
    /// trap recorded in CLAUDE.md.
    unsafe fn text_of(h: HWND) -> String {
        let mut buf = [0u16; 512];
        let n = SendMessageW(
            h,
            WM_GETTEXT,
            Some(WPARAM(buf.len())),
            Some(LPARAM(buf.as_mut_ptr() as isize)),
        );
        String::from_utf16_lossy(&buf[..n.0.max(0) as usize])
    }

    unsafe fn make_combo(parent: HWND) -> HWND {
        let c = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            w!("COMBOBOX"),
            w!(""),
            WS_CHILD
                | WS_VISIBLE
                | WS_VSCROLL
                | WS_TABSTOP
                | WINDOW_STYLE((CBS_DROPDOWN | CBS_AUTOHSCROLL | CBS_SORT) as u32),
            10,
            10,
            300,
            200,
            Some(parent),
            None,
            None,
            None,
        )
        .expect("CreateWindowExW COMBOBOX");
        // Same shape the real App combo has: a populated, sorted list where the
        // typed text is a strict prefix of an entry.
        for item in ["Narrator", PREFIX_OF, "Notes & To Do", "Paint"] {
            let t = wide(item);
            SendMessageW(
                c,
                CB_ADDSTRING,
                Some(WPARAM(0)),
                Some(LPARAM(t.as_ptr() as isize)),
            );
        }
        let t = wide(TYPED);
        SendMessageW(
            c,
            WM_SETTEXT,
            Some(WPARAM(0)),
            Some(LPARAM(t.as_ptr() as isize)),
        );
        c
    }

    unsafe fn pump() {
        let mut msg = MSG::default();
        while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }

    /// `WNDCLASSW::lpfnWndProc` needs an `extern "system"` fn pointer;
    /// `DefWindowProcW` itself is an ordinary (non-`extern`) safe wrapper
    /// around the linked symbol, so it can't be named there directly --
    /// same reason `combo_probe.rs` and `hotkey.rs` each define their own
    /// `wndproc` rather than pointing straight at `DefWindowProcW`.
    unsafe extern "system" fn wndproc(h: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
        DefWindowProcW(h, msg, wp, lp)
    }

    pub fn run() {
        if let Err(e) = try_run() {
            eprintln!("showhide_probe failed: {e}");
        }
    }

    fn try_run() -> Result<()> {
        unsafe {
            let hinst = GetModuleHandleW(None)?;
            let cls = w!("BeckonShowHideProbe");
            let wc = WNDCLASSW {
                lpfnWndProc: Some(wndproc),
                hInstance: hinst.into(),
                lpszClassName: cls,
                hbrBackground: HBRUSH((COLOR_WINDOW.0 + 1) as isize as *mut _),
                ..Default::default()
            };
            RegisterClassW(&wc);
            let parent = CreateWindowExW(
                WINDOW_EX_STYLE(0),
                cls,
                w!("showhide_probe"),
                WS_OVERLAPPEDWINDOW,
                100,
                100,
                420,
                160,
                None,
                None,
                Some(hinst.into()),
                None,
            )?;
            let _ = ShowWindow(parent, SW_SHOW);
            pump();

            // -- CONTROL: SetWindowPos, the call already measured as corrupting.
            let c1 = make_combo(parent);
            pump();
            let before_ctl = text_of(c1);
            let _ = SetWindowPos(c1, None, 10, 10, 300, 200, SWP_NOZORDER | SWP_NOACTIVATE);
            pump();
            let after_ctl = text_of(c1);
            let _ = DestroyWindow(c1);

            // -- SUBJECT: hide then show.
            let c2 = make_combo(parent);
            pump();
            let before_sub = text_of(c2);
            let _ = ShowWindow(c2, SW_HIDE);
            pump();
            let _ = ShowWindow(c2, SW_SHOW);
            pump();
            let after_sub = text_of(c2);
            let _ = DestroyWindow(c2);

            println!("TYPED={TYPED}");
            println!("CONTROL_BEFORE={before_ctl}");
            println!("CONTROL_AFTER={after_ctl}");
            println!("CONTROL_CORRUPTED={}", before_ctl != after_ctl);
            println!("SUBJECT_BEFORE={before_sub}");
            println!("SUBJECT_AFTER={after_sub}");
            println!("SUBJECT_CORRUPTED={}", before_sub != after_sub);
            println!();
            if before_ctl == after_ctl {
                println!("VERDICT=BLIND  the control did not reproduce the known-bad");
                println!("               SetWindowPos corruption, so this run says");
                println!("               nothing about ShowWindow. Fix the probe.");
            } else if before_sub == after_sub {
                println!("VERDICT=SAFE   ShowWindow does not corrupt. Task 8 ships as written.");
            } else {
                println!("VERDICT=UNSAFE ShowWindow corrupts too. Task 8 takes its fallback:");
                println!("               hide the GROUP and cover it with a STATIC, leaving");
                println!("               the children mapped underneath.");
            }

            let _ = DestroyWindow(parent);
            Ok(())
        }
    }
}
