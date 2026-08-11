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
    use std::ffi::c_void;
    use std::time::Duration;
    use windows::core::{w, BOOL};
    use windows::Win32::Foundation::{
        CloseHandle, HANDLE, HWND, LPARAM, POINT, RECT, SIZE, WPARAM,
    };
    use windows::Win32::Graphics::Gdi::MapWindowPoints;
    use windows::Win32::System::Diagnostics::Debug::{ReadProcessMemory, WriteProcessMemory};
    use windows::Win32::System::Memory::{
        VirtualAllocEx, VirtualFreeEx, MEM_COMMIT, MEM_RELEASE, MEM_RESERVE, PAGE_READWRITE,
    };
    use windows::Win32::System::Threading::{
        OpenProcess, PROCESS_VM_OPERATION, PROCESS_VM_READ, PROCESS_VM_WRITE,
    };
    use windows::Win32::UI::Controls::{
        BCM_GETIDEALSIZE, LVIR_BOUNDS, LVM_GETCOUNTPERPAGE, LVM_GETHEADER, LVM_GETITEMCOUNT,
        LVM_GETITEMRECT,
    };
    use windows::Win32::UI::HiDpi::{
        GetAwarenessFromDpiAwarenessContext, GetDpiForSystem, GetDpiForWindow,
        GetThreadDpiAwarenessContext, SetProcessDpiAwarenessContext,
        DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
    };
    use windows::Win32::UI::Input::KeyboardAndMouse::IsWindowEnabled;
    use windows::Win32::UI::WindowsAndMessaging::*;

    const WM_TRAY: u32 = WM_APP + 1;

    struct Kid {
        cls: String,
        txt: String,
        vis: bool,
        id: i32,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
    }

    thread_local! {
        static KIDS: RefCell<Vec<Kid>> = const { RefCell::new(Vec::new()) };
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

    /// Read a control's text from ANOTHER process.
    ///
    /// `GetWindowText` cannot: it returns the kernel-side caption instead of
    /// sending `WM_GETTEXT`, deliberately, so a hung target cannot hang the
    /// caller. An EDIT keeps its text in its own buffer, so that read comes
    /// back empty. `WM_GETTEXT` sent explicitly IS marshalled.
    fn ctl_text(h: HWND) -> String {
        let mut buf = [0u16; 512];
        let n = unsafe {
            SendMessageW(
                h,
                WM_GETTEXT,
                Some(WPARAM(buf.len())),
                Some(LPARAM(buf.as_mut_ptr() as isize)),
            )
        }
        .0;
        String::from_utf16_lossy(&buf[..n.max(0) as usize])
    }

    /// A child's on-screen box, expressed in the settings window's own client
    /// coordinates -- which is the frame the layout code works in, so the
    /// numbers can be compared against the tokens directly.
    fn box_in_client(parent: HWND, child: HWND) -> (i32, i32, i32, i32) {
        let mut rc = RECT::default();
        if unsafe { GetWindowRect(child, &mut rc) }.is_err() {
            return (0, 0, 0, 0);
        }
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
        // `None` for the source is HWND_DESKTOP, i.e. screen coordinates.
        unsafe { MapWindowPoints(None, Some(parent), &mut pts) };
        (pts[0].x, pts[0].y, pts[1].x - pts[0].x, pts[1].y - pts[0].y)
    }

    unsafe extern "system" fn on_child(h: HWND, l: LPARAM) -> BOOL {
        let parent = HWND(l.0 as *mut c_void);
        let visible = IsWindowVisible(h).as_bool();
        let (x, y, w, ht) = box_in_client(parent, h);
        KIDS.with(|k| {
            k.borrow_mut().push(Kid {
                cls: class_of(h),
                txt: text_of(h),
                vis: visible,
                id: GetDlgCtrlID(h),
                x,
                y,
                w,
                h: ht,
            })
        });
        true.into()
    }

    fn find_settings() -> Option<HWND> {
        let h = unsafe { FindWindowW(w!("BeckonSettingsWindow"), None) };
        match h {
            Ok(h) if !h.0.is_null() => Some(h),
            _ => None,
        }
    }

    const IDC_LIST: i32 = 1001;
    const IDC_COMBO: i32 = 1002;
    const IDC_APP: i32 = 1003;
    const IDC_ADD: i32 = 1005;
    const IDC_REMOVE: i32 = 1006;
    const IDC_APPLY: i32 = 1007;

    fn dlg_item(parent: HWND, id: i32) -> Option<HWND> {
        match unsafe { GetDlgItem(Some(parent), id) } {
            Ok(h) if !h.0.is_null() => Some(h),
            _ => None,
        }
    }

    /// A scratch buffer inside the window's OWN process.
    ///
    /// comctl32 messages that take a pointer (`LVM_GETITEMRECT`,
    /// `BCM_GETIDEALSIZE`) are not marshalled across a process boundary --
    /// only user32's own controls get that treatment, which is why
    /// `WM_GETTEXT` works above and `LVM_*` would not. Passing a local pointer
    /// makes the target dereference OUR address in ITS address space: the
    /// numbers that come back are meaningless and the target's memory is
    /// scribbled on. So the buffer is allocated where the control can reach
    /// it, and read back out afterwards.
    struct Remote {
        proc: HANDLE,
        addr: *mut c_void,
    }

    impl Remote {
        fn open(h: HWND) -> Option<Remote> {
            let mut pid = 0u32;
            unsafe { GetWindowThreadProcessId(h, Some(&mut pid)) };
            if pid == 0 {
                return None;
            }
            let proc = unsafe {
                OpenProcess(
                    PROCESS_VM_OPERATION | PROCESS_VM_READ | PROCESS_VM_WRITE,
                    false,
                    pid,
                )
            }
            .ok()?;
            let addr =
                unsafe { VirtualAllocEx(proc, None, 64, MEM_COMMIT | MEM_RESERVE, PAGE_READWRITE) };
            if addr.is_null() {
                unsafe {
                    let _ = CloseHandle(proc);
                }
                return None;
            }
            Some(Remote { proc, addr })
        }

        fn put<T: Copy>(&self, v: &T) -> bool {
            unsafe {
                WriteProcessMemory(
                    self.proc,
                    self.addr,
                    v as *const T as *const c_void,
                    std::mem::size_of::<T>(),
                    None,
                )
            }
            .is_ok()
        }

        fn get<T: Copy + Default>(&self) -> Option<T> {
            let mut out = T::default();
            unsafe {
                ReadProcessMemory(
                    self.proc,
                    self.addr,
                    &mut out as *mut T as *mut c_void,
                    std::mem::size_of::<T>(),
                    None,
                )
            }
            .ok()?;
            Some(out)
        }
    }

    impl Drop for Remote {
        fn drop(&mut self) {
            unsafe {
                let _ = VirtualFreeEx(self.proc, self.addr, 0, MEM_RELEASE);
                let _ = CloseHandle(self.proc);
            }
        }
    }

    fn send(h: HWND, msg: u32, wp: usize, lp: isize) -> isize {
        unsafe { SendMessageW(h, msg, Some(WPARAM(wp)), Some(LPARAM(lp))) }.0
    }

    /// The number every spacing token is measured against: one report-mode
    /// row, as comctl32 v6 sizes it for the window's font and DPI.
    fn measure_listview(parent: HWND, when: &str) {
        let Some(list) = dlg_item(parent, IDC_LIST) else {
            println!("    listview {when}: MISSING -- probe never reached IDC_LIST");
            return;
        };
        let (x, y, w, h) = box_in_client(parent, list);
        println!("    SysListView32 IDC_LIST ({when}): {w}x{h} at ({x},{y})");

        let count = send(list, LVM_GETITEMCOUNT, 0, 0);
        let per_page = send(list, LVM_GETCOUNTPERPAGE, 0, 0);
        println!("      LVM_GETITEMCOUNT:     {count}");
        println!("      LVM_GETCOUNTPERPAGE:  {per_page}");

        if count < 1 {
            println!("      LVM_GETITEMRECT(0):   n/a -- list is empty, not a zero-height row");
        } else if let Some(r) = Remote::open(list) {
            let want = RECT {
                left: LVIR_BOUNDS as i32,
                top: 0,
                right: 0,
                bottom: 0,
            };
            if !r.put(&want) {
                println!("      LVM_GETITEMRECT(0):   FAIL -- could not seed the remote buffer");
            } else if send(list, LVM_GETITEMRECT, 0, r.addr as isize) == 0 {
                println!("      LVM_GETITEMRECT(0):   FAIL -- message returned FALSE");
            } else if let Some(got) = r.get::<RECT>() {
                println!(
                    "      LVM_GETITEMRECT(0):   {}x{} at ({},{})  => ROW HEIGHT {}",
                    got.right - got.left,
                    got.bottom - got.top,
                    got.left,
                    got.top,
                    got.bottom - got.top
                );
            } else {
                println!("      LVM_GETITEMRECT(0):   FAIL -- could not read the remote buffer");
            }
        } else {
            println!("      LVM_GETITEMRECT(0):   FAIL -- no remote buffer (OpenProcess denied?)");
        }

        let hdr = HWND(send(list, LVM_GETHEADER, 0, 0) as *mut c_void);
        if hdr.0.is_null() {
            println!("      header:               MISSING -- LVM_GETHEADER returned null");
        } else {
            let (_, _, hw, hh) = box_in_client(parent, hdr);
            println!("      header SysHeader32:   {hw}x{hh}  => HEADER HEIGHT {hh}");
        }
    }

    /// Everything Landing 2a's spacing tokens are guesses without.
    fn measure_geometry(parent: HWND) {
        println!("  -- geometry (themed, comctl32 v6, inside the manifested process) --");
        println!("    GetDpiForWindow:      {}", unsafe {
            GetDpiForWindow(parent)
        });
        let mut rc = RECT::default();
        if unsafe { GetClientRect(parent, &mut rc) }.is_ok() {
            println!(
                "    client area:          {}x{}",
                rc.right - rc.left,
                rc.bottom - rc.top
            );
        }

        measure_listview(parent, "as built");

        // `BCM_GETIDEALSIZE` is what the THEME wants; the window rect is what
        // `layout` asked for. Printing both is the point -- a token that
        // disagrees with the ideal size is the thing to find.
        for (label, id) in [
            ("BUTTON   IDC_ADD", IDC_ADD),
            ("BUTTON   IDC_REMOVE", IDC_REMOVE),
            ("BUTTON   IDC_APPLY", IDC_APPLY),
        ] {
            let Some(ctl) = dlg_item(parent, id) else {
                println!("    {label}: MISSING");
                continue;
            };
            let (_, _, w, h) = box_in_client(parent, ctl);
            let ideal = Remote::open(ctl).and_then(|r| {
                let zero = SIZE { cx: 0, cy: 0 };
                if !r.put(&zero) || send(ctl, BCM_GETIDEALSIZE, 0, r.addr as isize) == 0 {
                    return None;
                }
                r.get::<SIZE>()
            });
            match ideal {
                Some(s) => println!("    {label}: {w}x{h}   BCM_GETIDEALSIZE {}x{}", s.cx, s.cy),
                None => println!("    {label}: {w}x{h}   BCM_GETIDEALSIZE unavailable"),
            }
        }

        if let Some(ctl) = dlg_item(parent, IDC_COMBO) {
            let (_, _, w, h) = box_in_client(parent, ctl);
            println!("    EDIT     IDC_COMBO:   {w}x{h}");
        } else {
            println!("    EDIT     IDC_COMBO:   MISSING");
        }

        if let Some(ctl) = dlg_item(parent, IDC_APP) {
            // Closed height: the drop-down list is a separate popup under v6,
            // so the control's own rect IS the closed height.
            let (_, _, w, h) = box_in_client(parent, ctl);
            let item = send(ctl, CB_GETITEMHEIGHT, usize::MAX, 0);
            println!("    COMBOBOX IDC_APP:     {w}x{h} closed   CB_GETITEMHEIGHT(-1) {item}");
        } else {
            println!("    COMBOBOX IDC_APP:     MISSING");
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
                std::thread::sleep(Duration::from_millis(60));
                println!(
                    "      typed {:?} -> control now {:?}",
                    char::from_u32(ch as u32).unwrap_or('?'),
                    ctl_text(h)
                );
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
                        let _ =
                            PostMessageW(Some(dlg), WM_COMMAND, WPARAM(button as usize), LPARAM(0));
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
        let notes = dlg_item(h, IDC_NOTES).map(ctl_text).unwrap_or_default();
        let shortcut = dlg_item(h, IDC_COMBO).map(ctl_text).unwrap_or_default();
        let appfld = dlg_item(h, IDC_APP).map(ctl_text).unwrap_or_default();
        let apply = dlg_item(h, IDC_APPLY)
            .map(|a| unsafe { IsWindowEnabled(a) }.as_bool())
            .unwrap_or(false);
        println!("    [{label}] apply={apply} shortcut={shortcut:?} app={appfld:?}");
        println!(
            "      notes: {}",
            notes.replace('\r', "").replace('\n', " | ")
        );
    }

    fn drive_an_edit(h: HWND) {
        println!("  -- driving an edit --");
        dump(h, "start");
        click(h, IDC_ADD);
        dump(h, "after Add");
        // A row exists now even if the machine's config was empty, so this is
        // the run that always has a row height in it.
        measure_listview(h, "after Add");

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

    /// Make THIS process per-monitor aware before it measures anything.
    ///
    /// Not cosmetic. A DPI-unaware caller gets virtualized answers from
    /// `GetWindowRect`: on a 150 % display a 1140 px window is reported as
    /// 760, silently divided by the scale factor. `LVM_GETITEMRECT` and
    /// `BCM_GETIDEALSIZE` come back through `ReadProcessMemory` instead and
    /// are never virtualized -- so an unaware probe prints logical pixels and
    /// physical pixels side by side, in the same block, unlabelled. That is
    /// how a 29 px row and a 21 px header end up in the same table when they
    /// are really 29 and 31. One awareness for the whole probe, so every
    /// number below is in physical pixels.
    fn go_dpi_aware() {
        let set =
            unsafe { SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
        let ctx = unsafe { GetThreadDpiAwarenessContext() };
        let awareness = unsafe { GetAwarenessFromDpiAwarenessContext(ctx) };
        println!(
            "probe DPI awareness: {} (SetProcessDpiAwarenessContext: {})",
            match awareness.0 {
                0 => "UNAWARE -- every rect below is virtualized, do not trust it",
                1 => "SYSTEM_AWARE",
                2 => "PER_MONITOR_AWARE (v2 requested)",
                _ => "INVALID",
            },
            if set.is_ok() {
                "ok"
            } else {
                "already set by a manifest"
            }
        );
        println!("GetDpiForSystem: {}", unsafe { GetDpiForSystem() });
    }

    pub fn run() {
        go_dpi_aware();

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
            let _ = EnumChildWindows(Some(h), Some(on_child), LPARAM(h.0 as isize));
        }
        KIDS.with(|k| {
            let kids = k.borrow();
            println!("  {} child controls:", kids.len());
            for kid in kids.iter() {
                println!(
                    "    {:<18} vis={} {:?}  id={} {}x{} at ({},{})",
                    kid.cls,
                    if kid.vis { "y" } else { "n" },
                    kid.txt,
                    kid.id,
                    kid.w,
                    kid.h,
                    kid.x,
                    kid.y
                );
            }
        });

        measure_geometry(h);

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
