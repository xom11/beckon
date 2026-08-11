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
    use windows::core::{w, BOOL, PWSTR};
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
        BCM_GETIDEALSIZE, LVIF_TEXT, LVIR_BOUNDS, LVITEMW, LVM_GETCOUNTPERPAGE, LVM_GETHEADER,
        LVM_GETITEMCOUNT, LVM_GETITEMRECT, LVM_GETITEMTEXTW,
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

    /// `GetWindowRect` failing (window destroyed mid-probe, access denied)
    /// must not look like a valid zero-size control -- printing `(0,0,0,0)`
    /// silently was exactly that failure mode, since a genuine zero-size
    /// control also prints `(0,0,0,0)`. Every field carries this
    /// out-of-range sentinel so a caller can test one field and know the
    /// read failed rather than mistake it for real geometry.
    const RECT_FAIL: i32 = i32::MIN;

    /// A child's on-screen box, expressed in the settings window's own client
    /// coordinates -- which is the frame the layout code works in, so the
    /// numbers can be compared against the tokens directly.
    fn box_in_client(parent: HWND, child: HWND) -> (i32, i32, i32, i32) {
        let mut rc = RECT::default();
        if unsafe { GetWindowRect(child, &mut rc) }.is_err() {
            return (RECT_FAIL, RECT_FAIL, RECT_FAIL, RECT_FAIL);
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

    /// Render a `box_in_client` result, printing an unmistakable marker
    /// instead of a coordinate when the read failed.
    fn fmt_box(x: i32, y: i32, w: i32, h: i32) -> String {
        if w == RECT_FAIL {
            "RECTFAIL".to_string()
        } else {
            format!("{w}x{h} at ({x},{y})")
        }
    }

    /// Same marker, for call sites that only print width/height.
    fn fmt_wh(w: i32, h: i32) -> String {
        if w == RECT_FAIL {
            "RECTFAIL".to_string()
        } else {
            format!("{w}x{h}")
        }
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
    // Above the range the probe pins (1001-1007). Read only, never used to
    // drive anything -- they say whether the window is in read-only mode.
    const IDC_CAPS: i32 = 1008;
    const IDC_OPENFILE: i32 = 1012;
    const IDC_CLOSE: i32 = 1013;

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

    /// How big a remote block `Remote::open` reserves. Big enough for the
    /// largest struct sent through it (`LVITEMW`) at offset 0 AND for a
    /// cell's text at `CELL_TEXT_OFF`, because `LVM_GETITEMTEXT` takes a
    /// struct that itself CONTAINS a pointer -- and that inner pointer has
    /// to be an address in the target process too, not just the outer one.
    const REMOTE_SIZE: usize = CELL_TEXT_OFF + CELL_TEXT_CHARS * 2;
    /// Where the text buffer starts inside that block. Comfortably past
    /// `size_of::<LVITEMW>()` on both 32- and 64-bit.
    const CELL_TEXT_OFF: usize = 256;
    const CELL_TEXT_CHARS: usize = 256;

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
            let addr = unsafe {
                VirtualAllocEx(
                    proc,
                    None,
                    REMOTE_SIZE,
                    MEM_COMMIT | MEM_RESERVE,
                    PAGE_READWRITE,
                )
            };
            if addr.is_null() {
                unsafe {
                    let _ = CloseHandle(proc);
                }
                return None;
            }
            Some(Remote { proc, addr })
        }

        /// An address inside the remote block, for a message that wants a
        /// pointer the TARGET can dereference.
        fn at(&self, off: usize) -> *mut c_void {
            (self.addr as usize + off) as *mut c_void
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

        /// Read a NUL-terminated wide string back out of the remote block.
        /// Not `get::<[u16; N]>`: arrays only implement `Default` up to 32
        /// elements, and a 32-character cell is not a cell.
        fn read_utf16(&self, off: usize, chars: usize) -> Option<String> {
            let mut buf = vec![0u16; chars];
            unsafe {
                ReadProcessMemory(
                    self.proc,
                    self.at(off),
                    buf.as_mut_ptr() as *mut c_void,
                    chars * 2,
                    None,
                )
            }
            .ok()?;
            let n = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
            Some(String::from_utf16_lossy(&buf[..n]))
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

    /// One cell of the ListView, read out of the target process.
    ///
    /// **This is the model, not the screen.** The list is rendered from
    /// `ControlState::items`, which `serve` projects out of the `Model` on
    /// every push -- so a cell is the cheapest witness there is to what the
    /// window actually RECORDED, as opposed to what the control it was typed
    /// into currently displays. Comparing the two after every character is
    /// how the App combo box's lying is either caught or ruled out; nothing
    /// reachable from a unit test can see both at once.
    fn list_cell(list: HWND, row: i32, sub: i32) -> Option<String> {
        let r = Remote::open(list)?;
        let item = LVITEMW {
            mask: LVIF_TEXT,
            iItem: row,
            iSubItem: sub,
            pszText: PWSTR(r.at(CELL_TEXT_OFF) as *mut u16),
            cchTextMax: CELL_TEXT_CHARS as i32,
            ..Default::default()
        };
        if !r.put(&item) {
            return None;
        }
        // Returns the character count, and 0 is a legitimate answer (an
        // empty cell), so only the struct write above can fail here.
        send(list, LVM_GETITEMTEXTW, row as usize, r.addr as isize);
        r.read_utf16(CELL_TEXT_OFF, CELL_TEXT_CHARS)
    }

    /// Does this list cell agree with what the field it mirrors says?
    ///
    /// The App column appends the row's flag after three spaces
    /// (`Notepad   not installed`), so the field's text is a PREFIX of the
    /// cell rather than the whole of it. The Shortcut column is the combo
    /// verbatim.
    fn cell_agrees(cell: &str, field: &str) -> bool {
        cell == field
            || cell
                .strip_prefix(field)
                .is_some_and(|r| r.starts_with("   "))
    }

    /// The number every spacing token is measured against: one report-mode
    /// row, as comctl32 v6 sizes it for the window's font and DPI.
    fn measure_listview(parent: HWND, when: &str) {
        let Some(list) = dlg_item(parent, IDC_LIST) else {
            println!("    listview {when}: MISSING -- probe never reached IDC_LIST");
            return;
        };
        let (x, y, w, h) = box_in_client(parent, list);
        println!(
            "    SysListView32 IDC_LIST ({when}): {}",
            fmt_box(x, y, w, h)
        );

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
            if hh == RECT_FAIL {
                println!("      header SysHeader32:   RECTFAIL  => HEADER HEIGHT RECTFAIL");
            } else {
                println!("      header SysHeader32:   {hw}x{hh}  => HEADER HEIGHT {hh}");
            }
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
            let wh = fmt_wh(w, h);
            let ideal = Remote::open(ctl).and_then(|r| {
                let zero = SIZE { cx: 0, cy: 0 };
                if !r.put(&zero) || send(ctl, BCM_GETIDEALSIZE, 0, r.addr as isize) == 0 {
                    return None;
                }
                r.get::<SIZE>()
            });
            match ideal {
                Some(s) => println!("    {label}: {wh}   BCM_GETIDEALSIZE {}x{}", s.cx, s.cy),
                None => println!("    {label}: {wh}   BCM_GETIDEALSIZE unavailable"),
            }
        }

        if let Some(ctl) = dlg_item(parent, IDC_COMBO) {
            let (_, _, w, h) = box_in_client(parent, ctl);
            println!("    EDIT     IDC_COMBO:   {}", fmt_wh(w, h));
        } else {
            println!("    EDIT     IDC_COMBO:   MISSING");
        }

        if let Some(ctl) = dlg_item(parent, IDC_APP) {
            // Closed height: the drop-down list is a separate popup under v6,
            // so the control's own rect IS the closed height.
            let (_, _, w, h) = box_in_client(parent, ctl);
            let item = send(ctl, CB_GETITEMHEIGHT, usize::MAX, 0);
            println!(
                "    COMBOBOX IDC_APP:     {} closed   CB_GETITEMHEIGHT(-1) {item}",
                fmt_wh(w, h)
            );
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
    ///
    /// `witness` is `(list, row, subitem)`: the list cell this field feeds.
    /// After every character the cell is read back and compared with the
    /// field, and the count of disagreements is returned. **That comparison
    /// is the only thing that can settle the App combo box defect.** The
    /// control rewriting its own text is not itself a bug — comctl32 is
    /// entitled to autocomplete — the bug is beckon recording something
    /// other than what the field ends up showing. One number, per keystroke,
    /// with both sides printed.
    fn type_into(h: HWND, s: &str, witness: Option<(HWND, i32, i32)>) -> usize {
        let mut empty: Vec<u16> = vec![0];
        let mut disagreements = 0usize;
        unsafe {
            SendMessageW(
                h,
                WM_SETTEXT,
                Some(WPARAM(0)),
                Some(LPARAM(empty.as_mut_ptr() as isize)),
            );
            for ch in s.encode_utf16() {
                SendMessageW(h, WM_CHAR, Some(WPARAM(ch as usize)), Some(LPARAM(1)));
                // Long enough for the window to have pumped the POSTED
                // message the App field's read now rides on. A synchronous
                // read needs no wait, so a probe with no wait here would
                // report the fix as a regression.
                std::thread::sleep(Duration::from_millis(120));
                let field = ctl_text(h);
                let verdict = match witness {
                    Some((list, row, sub)) => match list_cell(list, row, sub) {
                        Some(cell) if cell_agrees(&cell, &field) => format!("list {cell:?} MATCH"),
                        Some(cell) => {
                            disagreements += 1;
                            format!("list {cell:?} <<< DISAGREES with the field")
                        }
                        None => "list UNREADABLE".to_string(),
                    },
                    None => "(no witness)".to_string(),
                };
                println!(
                    "      typed {:?} -> field {:?}   {verdict}",
                    char::from_u32(ch as u32).unwrap_or('?'),
                    field,
                );
            }
        }
        std::thread::sleep(Duration::from_millis(300));
        disagreements
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

    /// Is the window in read-only mode, and does it look like it?
    ///
    /// Reachable only by pointing `beckon-serve` at a config file that does
    /// not parse, which the probe cannot arrange for itself -- it does not
    /// own the path. So this REPORTS rather than asserts: run the probe once
    /// against a good file and once against a deliberately broken one, and
    /// the two blocks are the before and after.
    ///
    /// Returns true when editing is off, so the caller can skip the edit
    /// drive instead of reporting a disabled Add as a failure.
    fn report_read_only(h: HWND) -> bool {
        let on = |id: i32| {
            dlg_item(h, id)
                .map(|c| unsafe { IsWindowEnabled(c) }.as_bool())
                .unwrap_or(false)
        };
        let read_only = !on(IDC_ADD);
        println!("  -- editing state --");
        println!(
            "    enabled: Add={} List={} Caps={} Save={} | escape routes: OpenFile={} Close={}",
            on(IDC_ADD),
            on(IDC_LIST),
            on(IDC_CAPS),
            on(IDC_APPLY),
            on(IDC_OPENFILE),
            on(IDC_CLOSE),
        );
        if read_only {
            println!("    READ ONLY -- the file did not parse. The contract is:");
            println!(
                "      every mutating control off, both escape routes on, \
                 and the notes say why"
            );
            let bad = [
                ("Add", on(IDC_ADD)),
                ("List", on(IDC_LIST)),
                ("Caps", on(IDC_CAPS)),
                ("Save", on(IDC_APPLY)),
            ]
            .iter()
            .filter(|(_, e)| *e)
            .map(|(n, _)| *n)
            .collect::<Vec<_>>();
            if bad.is_empty() && on(IDC_OPENFILE) && on(IDC_CLOSE) {
                println!("      PASS");
            } else {
                println!(
                    "      FAIL: still enabled {bad:?}, escape routes OpenFile={} Close={}",
                    on(IDC_OPENFILE),
                    on(IDC_CLOSE)
                );
            }
            dump(h, "read only");
        }
        read_only
    }

    fn drive_an_edit(h: HWND) {
        println!("  -- driving an edit --");
        dump(h, "start");
        click(h, IDC_ADD);
        dump(h, "after Add");
        // A row exists now even if the machine's config was empty, so this is
        // the run that always has a row height in it.
        measure_listview(h, "after Add");

        // Add appends and selects, so the new row is the last one. Its cells
        // are what the two fields below are compared against.
        let list = dlg_item(h, IDC_LIST);
        let row = list
            .map(|l| send(l, LVM_GETITEMCOUNT, 0, 0) as i32 - 1)
            .filter(|r| *r >= 0);
        match row {
            Some(r) => println!("    witness row: {r}"),
            None => println!("    NOTE: no list row to witness against; typing is unchecked"),
        }
        let witness = |sub: i32| match (list, row) {
            (Some(l), Some(r)) => Some((l, r, sub)),
            _ => None,
        };

        let Some(combo_edit) = dlg_item(h, IDC_COMBO) else {
            println!("    FAIL: no shortcut field");
            return;
        };
        // Column 1 is `Shortcut`; column 0 is `App`. A plain EDIT does not
        // rewrite itself, so this half is the CONTROL for the App half: if
        // it disagrees too, the probe's own timing is wrong and neither
        // result means anything.
        let combo_lies = type_into(combo_edit, "ctrl+super+alt+j", witness(1));
        dump(h, "after shortcut text");

        // The App control is a COMBOBOX; its text lives in a child EDIT, and
        // only that child raises the change notification the window listens
        // for. Setting the combo itself is silent.
        let app_edit = dlg_item(h, IDC_APP).and_then(|c| first_child_of_class(c, "Edit"));
        let app_lies = match app_edit {
            Some(e) => type_into(e, "Notepad", witness(0)),
            None => {
                println!("    FAIL: combo box has no edit child");
                usize::MAX
            }
        };
        dump(h, "after app text");
        println!(
            "    per-keystroke agreement: Shortcut field {} ({combo_lies} disagreements), \
             App field {} ({app_lies} disagreements)",
            if combo_lies == 0 { "PASS" } else { "FAIL" },
            if app_lies == 0 { "PASS" } else { "FAIL" },
        );
        if combo_lies > 0 {
            println!(
                "      the control field disagreed too -- suspect the probe's own \
                 timing before believing the App result"
            );
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
        // Don't guess why the call failed: "already set by a manifest" is
        // only what ERROR_ACCESS_DENIED implies, and it's one of several
        // ways `SetProcessDpiAwarenessContext` can fail. Print the real
        // error and let it speak for itself.
        let set_result = match &set {
            Ok(()) => "ok".to_string(),
            Err(e) => format!("FAILED: {e}"),
        };
        println!(
            "probe DPI awareness: {} (SetProcessDpiAwarenessContext: {})",
            match awareness.0 {
                0 => "UNAWARE -- every rect below is virtualized, do not trust it",
                1 => "SYSTEM_AWARE",
                2 => "PER_MONITOR_AWARE (v2 requested)",
                _ => "INVALID",
            },
            set_result
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
                    "    {:<18} vis={} {:?}  id={} {}",
                    kid.cls,
                    if kid.vis { "y" } else { "n" },
                    kid.txt,
                    kid.id,
                    fmt_box(kid.x, kid.y, kid.w, kid.h)
                );
            }
        });

        measure_geometry(h);

        // A read-only window has nothing to drive, and nothing to save on
        // the way out either -- the close below must NOT produce a prompt.
        if !report_read_only(h) {
            drive_an_edit(h);
        }

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
