//! G3 (spec `2026-08-12-settings-keycaps-design.md` §B.6): does
//! `CDRF_SKIPDEFAULT` on subitem 0 of a `LVS_EX_CHECKBOXES` report-view
//! ListView remove the per-row tick?
//!
//! The tick is a state image in column 0 and it is what makes `Remove` a
//! multi-delete. Losing it is not cosmetic.
//!
//! **Reads pixels, not intentions.** The only honest answer comes from what
//! is on the screen, so the probe screen-captures the state-image rectangle
//! (`LVIR_ICON` on subitem 0 is the state image's rect under
//! `LVS_EX_CHECKBOXES`) and counts non-background pixels. A drawn tick box is
//! tens of dark pixels; an absent one is zero.
//!
//! **Carries a control:** row 0 is skipped, row 1 is default-drawn. Row 1 MUST
//! come back with ink. If it does not, the capture is broken and the verdict
//! on row 0 means nothing.
//!
//! Build: `cargo build -p beckon-windows --example customdraw_probe --all-targets`
//! Run from **session 1** (an SSH shell is session 0 and has no desktop).

fn main() {
    #[cfg(not(target_os = "windows"))]
    eprintln!("customdraw_probe only does anything on Windows");
    #[cfg(target_os = "windows")]
    win::run();
}

#[cfg(target_os = "windows")]
mod win {
    use windows::core::*;
    use windows::Win32::Foundation::*;
    use windows::Win32::Graphics::Gdi::*;
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::Controls::*;
    use windows::Win32::UI::WindowsAndMessaging::*;

    const IDC_LIST: i32 = 1;
    /// Row 0 is the subject (subitem 0 skipped); row 1 is the control.
    const SUBJECT_ROW: i32 = 0;
    const CONTROL_ROW: i32 = 1;

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    /// `WNDCLASSW::lpfnWndProc` needs an `extern "system"` fn pointer;
    /// `DefWindowProcW` itself is an ordinary (non-`extern`) safe wrapper
    /// around the linked symbol, so it can't be named there directly -- same
    /// reason `combo_probe.rs` and `showhide_probe.rs` each define their own
    /// `wndproc`. This one also carries the `NM_CUSTOMDRAW` handling, since a
    /// real one is needed anyway to run the measurement.
    unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
        if msg == WM_NOTIFY {
            let nm = &*(lp.0 as *const NMHDR);
            if nm.idFrom == IDC_LIST as usize && nm.code == NM_CUSTOMDRAW {
                let cd = &*(lp.0 as *const NMLVCUSTOMDRAW);
                let stage = cd.nmcd.dwDrawStage;
                if stage == CDDS_PREPAINT {
                    return LRESULT(CDRF_NOTIFYITEMDRAW as isize);
                }
                if stage == CDDS_ITEMPREPAINT {
                    return LRESULT(CDRF_NOTIFYSUBITEMDRAW as isize);
                }
                // `NMCUSTOMDRAW_DRAW_STAGE` (the type of `dwDrawStage`) has no
                // `BitOr` impl in `windows` 0.61 -- unlike the flag types
                // (`WINDOW_STYLE`, `LVCOLUMNW_MASK`, ...) it is not generated
                // as a bitmask type, just a bare newtype. Compare the raw
                // `u32`s instead of `stage == CDDS_ITEMPREPAINT | CDDS_SUBITEM`.
                if stage.0 == CDDS_ITEMPREPAINT.0 | CDDS_SUBITEM.0 {
                    // Skip subitem 0 on the SUBJECT row only. Everything else
                    // draws normally, which is what makes the control a control.
                    if cd.nmcd.dwItemSpec == SUBJECT_ROW as usize && cd.iSubItem == 0 {
                        return LRESULT(CDRF_SKIPDEFAULT as isize);
                    }
                    return LRESULT(CDRF_DODEFAULT as isize);
                }
            }
        }
        DefWindowProcW(hwnd, msg, wp, lp)
    }

    /// Count pixels in `rc` (screen coords) that are not the window background.
    unsafe fn ink_in(rc: RECT) -> u32 {
        let w = rc.right - rc.left;
        let h = rc.bottom - rc.top;
        if w <= 0 || h <= 0 {
            return 0;
        }
        let screen = GetDC(None);
        let mem = CreateCompatibleDC(Some(screen));
        let bmp = CreateCompatibleBitmap(screen, w, h);
        let old = SelectObject(mem, bmp.into());
        let _ = BitBlt(mem, 0, 0, w, h, Some(screen), rc.left, rc.top, SRCCOPY);
        let bg = GetSysColor(COLOR_WINDOW) & 0x00FF_FFFF;
        let mut ink = 0u32;
        for y in 0..h {
            for x in 0..w {
                let px = GetPixel(mem, x, y).0 & 0x00FF_FFFF;
                if px != bg {
                    ink += 1;
                }
            }
        }
        SelectObject(mem, old);
        let _ = DeleteObject(bmp.into());
        let _ = DeleteDC(mem);
        ReleaseDC(None, screen);
        ink
    }

    /// The state-image rect for `row`, in SCREEN coordinates.
    unsafe fn state_rect(list: HWND, row: i32) -> RECT {
        let mut rc = RECT {
            // `LVIR_ICON` in `rc.left` before `LVM_GETITEMRECT` is the
            // documented calling convention for that message (it selects
            // which rect variant comes back), not a bug to remove.
            //
            // `LVIR_ICON` is a bare `u32` in `windows` 0.61 (unlike most of
            // the surrounding `LV*` constants, which are newtypes with a
            // `.0` field) -- `LVIR_ICON.0` does not compile.
            left: LVIR_ICON as i32,
            top: 0,
            right: 0,
            bottom: 0,
        };
        SendMessageW(
            list,
            LVM_GETITEMRECT,
            Some(WPARAM(row as usize)),
            Some(LPARAM(&mut rc as *mut RECT as isize)),
        );
        let mut pts = [
            POINT {
                x: rc.left,
                y: rc.top,
            },
            POINT {
                x: rc.right,
                y: rc.bottom,
            },
        ];
        MapWindowPoints(Some(list), None, &mut pts);
        RECT {
            left: pts[0].x,
            top: pts[0].y,
            right: pts[1].x,
            bottom: pts[1].y,
        }
    }

    /// Pumps the message queue for `ms` milliseconds so the ListView actually
    /// paints (and repaints, if the WM sends more than one pass) before the
    /// capture. `windows` 0.61's `GetTickCount64` lives behind the
    /// `Win32_System_SystemInformation` feature, which `beckon-windows` does
    /// not enable and this probe must not add -- `std::time::Instant` gives
    /// the identical fixed-duration pump with no new Win32 surface at all.
    fn pump_for(ms: u32) {
        let end = std::time::Instant::now() + std::time::Duration::from_millis(ms as u64);
        let mut msg = MSG::default();
        while std::time::Instant::now() < end {
            while unsafe { PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE) }.as_bool() {
                unsafe {
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            }
        }
    }

    pub fn run() {
        if let Err(e) = try_run() {
            eprintln!("customdraw_probe failed: {e}");
        }
    }

    fn try_run() -> Result<()> {
        unsafe {
            let icc = INITCOMMONCONTROLSEX {
                dwSize: std::mem::size_of::<INITCOMMONCONTROLSEX>() as u32,
                dwICC: ICC_LISTVIEW_CLASSES,
            };
            let _ = InitCommonControlsEx(&icc);

            let hinst = GetModuleHandleW(None)?;
            let cls = w!("BeckonCustomDrawProbe");
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
                w!("customdraw_probe"),
                WS_OVERLAPPEDWINDOW,
                100,
                100,
                520,
                260,
                None,
                None,
                Some(hinst.into()),
                None,
            )?;

            let list = CreateWindowExW(
                WINDOW_EX_STYLE(0),
                w!("SysListView32"),
                w!(""),
                // `LVS_REPORT` is already a bare `u32` in this crate version
                // (most sibling `LVS_*`/`LVS_EX_*` constants are not), so
                // `WINDOW_STYLE(LVS_REPORT as u32)` is a same-type cast --
                // `clippy::unnecessary_cast` flags it. Pass it straight in.
                WS_CHILD | WS_VISIBLE | WS_BORDER | WINDOW_STYLE(LVS_REPORT),
                10,
                10,
                480,
                180,
                Some(parent),
                Some(HMENU(IDC_LIST as *mut _)),
                None,
                None,
            )?;
            SendMessageW(
                list,
                LVM_SETEXTENDEDLISTVIEWSTYLE,
                Some(WPARAM(0)),
                Some(LPARAM(
                    (LVS_EX_FULLROWSELECT | LVS_EX_DOUBLEBUFFER | LVS_EX_CHECKBOXES) as isize,
                )),
            );

            for (i, title) in ["App", "Shortcut"].iter().enumerate() {
                let t = wide(title);
                let col = LVCOLUMNW {
                    mask: LVCF_TEXT | LVCF_WIDTH,
                    cx: 230,
                    pszText: PWSTR(t.as_ptr() as *mut u16),
                    ..Default::default()
                };
                SendMessageW(
                    list,
                    LVM_INSERTCOLUMNW,
                    Some(WPARAM(i)),
                    Some(LPARAM(&col as *const LVCOLUMNW as isize)),
                );
            }
            for (i, app) in ["Windows Terminal", "Claude"].iter().enumerate() {
                let t = wide(app);
                let it = LVITEMW {
                    mask: LVIF_TEXT,
                    iItem: i as i32,
                    pszText: PWSTR(t.as_ptr() as *mut u16),
                    ..Default::default()
                };
                SendMessageW(
                    list,
                    LVM_INSERTITEMW,
                    Some(WPARAM(0)),
                    Some(LPARAM(&it as *const LVITEMW as isize)),
                );
            }

            let _ = ShowWindow(parent, SW_SHOW);
            let _ = UpdateWindow(parent);
            pump_for(600);

            let subject = ink_in(state_rect(list, SUBJECT_ROW));
            let control = ink_in(state_rect(list, CONTROL_ROW));

            println!("SUBJECT_ROW={SUBJECT_ROW} (subitem 0 CDRF_SKIPDEFAULT)");
            println!("CONTROL_ROW={CONTROL_ROW} (default drawn)");
            println!("SUBJECT_INK={subject}");
            println!("CONTROL_INK={control}");
            println!();
            if control == 0 {
                println!("VERDICT=BLIND    the control row shows no tick either, so the");
                println!("                 capture is broken. Fix the probe before reading");
                println!("                 anything into SUBJECT_INK.");
            } else if subject == 0 {
                println!("VERDICT=TICK_LOST  subitem 0 stays default-drawn. Spec B.6 takes");
                println!("                   its second branch: app_cell keeps appending the");
                println!("                   flag in Body, and the IOU stays open.");
            } else {
                println!("VERDICT=TICK_SURVIVES  subitem 0 joins the custom-draw pass in 3b:");
                println!("                       app name in Body, flag in Caption, and the");
                println!("                       app_cell IOU closes.");
            }

            let _ = DestroyWindow(parent);
            Ok(())
        }
    }
}
