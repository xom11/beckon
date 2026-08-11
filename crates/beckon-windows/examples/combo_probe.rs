//! `combo_probe` -- what actually rewrites a `CBS_DROPDOWN` combo box's edit
//! field, and in what order relative to the notifications beckon listens for.
//!
//! Two hardware runs said the App field and the model disagree (2026-08-11:
//! typing `Notepad` wrote `d`; 2026-08-12 at `05db60b`: the field showed
//! `Narrator` while the model held `N`). Both were measured from OUTSIDE the
//! process, so neither could see *which* message rewrote the text or where it
//! sat in the sequence. This probe builds the same control in-process and
//! prints, per keystroke:
//!
//! - every `WM_COMMAND` the parent receives, named, with the text sampled;
//! - every text-mutating message the combo's child EDIT receives, sampled
//!   before and after the control handles it -- which is the only way to see
//!   the rewrite itself rather than infer it;
//! - what a POSTED message sees when it is finally dispatched (beckon's
//!   `WM_APP_EDITED` route);
//! - `GetWindowTextW` on the COMBOBOX beside `WM_GETTEXT` on its child EDIT.
//!   beckon reads the first, `settings_probe` reads the second. Nobody has
//!   checked that they agree.
//!
//! FOUR CONTROLS, because this landing has already produced several checks
//! that could not tell a finding from a broken detector:
//!
//! 1. **An empty combo box.** No items means no completion is possible, so
//!    every sample must equal exactly what was typed. If it does not, the
//!    probe is wrong and nothing else in the run means anything.
//! 2. **A plain EDIT.** Same injection, same sampling, no combo box.
//! 3. **comctl32 v5 vs v6**, run from outside: the caller runs this binary
//!    once as `cargo` built it and once with beckon's own manifest stamped in
//!    by `mt.exe`. Every run prints `DllGetVersion`, so which one it got is
//!    read off the output rather than assumed. **Not the WinSxS path** -- both
//!    versions live there, and reading the path as the answer is how the first
//!    run of this probe misreported its own control.
//! 4. **Two ways to read the same text.** `GetWindowTextLengthW`-then-
//!    `GetWindowTextW` (what `settings_window.rs::text_of` does) beside a
//!    fixed 512-word `GetWindowTextW`. If a length-then-read can truncate,
//!    these two disagree.
//!
//! Injection is by `WM_CHAR`, which is what `settings_probe` does. Pass
//! `--sendinput` to add a real-keyboard pass; that one needs an interactive
//! session AND the foreground, and it reports whether it actually got the
//! foreground so a false negative cannot pass for a result.
//!
//! Throwaway diagnostic. Nothing in the library depends on it.

fn main() {
    #[cfg(not(target_os = "windows"))]
    eprintln!("combo_probe only does anything on Windows");
    #[cfg(target_os = "windows")]
    win::run();
}

#[cfg(target_os = "windows")]
mod win {
    use std::cell::{Cell, RefCell};
    use std::ffi::c_void;
    use std::time::Duration;
    use windows::core::{w, PCWSTR};
    use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use windows::Win32::System::LibraryLoader::{
        GetModuleFileNameW, GetModuleHandleW, GetProcAddress,
    };
    use windows::Win32::UI::Controls::{
        InitCommonControlsEx, ICC_STANDARD_CLASSES, INITCOMMONCONTROLSEX,
    };
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        GetFocus, SendInput, SetFocus, INPUT, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP,
        KEYEVENTF_UNICODE,
    };
    use windows::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};
    use windows::Win32::UI::WindowsAndMessaging::*;

    /// The word typed in every scenario. The two hardware reports are about
    /// this exact string, so nothing else is worth typing.
    const WORD: &str = "Notepad";

    /// Names taken from the a14 report's own output, so the completions this
    /// probe can produce are the ones that were actually observed there:
    /// `N` -> Narrator, `o` -> Obsidian, `t` -> Tailscale, and so on.
    const CATALOG: &[&str] = &[
        "Access",
        "Debuggable Package Manager",
        "Excel",
        "Narrator",
        "Notepad",
        "Obsidian",
        "Paint",
        "Tailscale",
    ];

    const IDC_COMBO: i32 = 3001;
    const IDC_PLAIN: i32 = 3002;

    /// Spelled as the numbers winuser.h/commctrl.h give them, the way
    /// `settings_probe` already spells `VK_DOWN`. `windows` 0.61 files these
    /// under a different module from the `WM_*` glob above, and a `use` that
    /// pulls in half a second module for five integers is worse than the
    /// integers.
    const EM_GETSEL: u32 = 0x00B0;
    const EM_SETSEL: u32 = 0x00B1;
    const EM_REPLACESEL: u32 = 0x00C2;
    const EM_UNDO: u32 = 0x00C7;
    const CB_SETMINVISIBLE: u32 = 0x1701;
    /// beckon's own deferred-read message, respelled here so the ordering
    /// this probe measures is the ordering beckon actually depends on.
    const WM_PROBE_READ: u32 = WM_APP + 3;

    thread_local! {
        static COMBO: Cell<isize> = const { Cell::new(0) };
        static ECHILD: Cell<isize> = const { Cell::new(0) };
        static PLAIN: Cell<isize> = const { Cell::new(0) };
        /// Suppresses the subclass log while the scenario is being built --
        /// otherwise the setup's own `WM_SETTEXT`s bury the measurement.
        static WATCH: Cell<bool> = const { Cell::new(false) };
        /// `Kind::ModelLoop` only: beckon's model cell, and beckon's
        /// `Ui::suppress`.
        static MODEL: RefCell<String> = const { RefCell::new(String::new()) };
        static SUPPRESS: Cell<bool> = const { Cell::new(false) };
        static LOOPING: Cell<bool> = const { Cell::new(false) };
        /// `Kind::ModelLoopWithLayout` only: re-place the combo box after the
        /// model push, the way `apply_state` -> `layout` does.
        static RELAYOUT: Cell<bool> = const { Cell::new(false) };
    }

    fn hwnd_of(v: isize) -> HWND {
        HWND(v as *mut c_void)
    }
    fn combo() -> HWND {
        hwnd_of(COMBO.with(|c| c.get()))
    }
    fn echild() -> HWND {
        hwnd_of(ECHILD.with(|c| c.get()))
    }
    fn plain() -> HWND {
        hwnd_of(PLAIN.with(|c| c.get()))
    }
    fn live(h: HWND) -> bool {
        !h.0.is_null()
    }

    // ---------------------------------------------------------------- reads

    /// `settings_window.rs::text_of`, copied verbatim in shape: ask for the
    /// length, size the buffer to it, then read. Control 4's first half.
    fn text_of_len_first(h: HWND) -> String {
        let len = unsafe { GetWindowTextLengthW(h) };
        if len <= 0 {
            return String::new();
        }
        let mut buf = vec![0u16; len as usize + 1];
        let n = unsafe { GetWindowTextW(h, &mut buf) };
        String::from_utf16_lossy(&buf[..n.max(0) as usize])
    }

    /// The same read with a buffer that cannot be too small. Control 4's
    /// second half: if these two ever disagree, a length-then-read truncates.
    fn text_of_big(h: HWND) -> String {
        let mut buf = [0u16; 512];
        let n = unsafe { GetWindowTextW(h, &mut buf) };
        String::from_utf16_lossy(&buf[..n.max(0) as usize])
    }

    /// What `settings_probe` reads: an explicit `WM_GETTEXT`.
    fn wm_text(h: HWND) -> String {
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

    fn selection(h: HWND) -> (i32, i32) {
        let mut s: u32 = 0;
        let mut e: u32 = 0;
        unsafe {
            SendMessageW(
                h,
                EM_GETSEL,
                Some(WPARAM(&mut s as *mut u32 as usize)),
                Some(LPARAM(&mut e as *mut u32 as isize)),
            );
        }
        (s as i32, e as i32)
    }

    /// One line describing everything that could be the answer, at one
    /// instant. Every observation in this probe is one of these.
    fn sample(tag: &str) {
        let c = combo();
        if live(c) {
            let e = echild();
            let (ss, se) = if live(e) { selection(e) } else { (-1, -1) };
            let cur = unsafe { SendMessageW(c, CB_GETCURSEL, Some(WPARAM(0)), Some(LPARAM(0))) }.0;
            let len_first = text_of_len_first(c);
            let big = text_of_big(c);
            println!(
                "        [{tag}] combo.GetWindowText={:?} combo.big={:?} combo.WM_GETTEXT={:?} edit.WM_GETTEXT={:?} sel={ss}..{se} cursel={cur}{}",
                len_first,
                big,
                wm_text(c),
                if live(e) { wm_text(e) } else { String::from("<none>") },
                if len_first == big { "" } else { "   <<< TRUNCATED READ" },
            );
        } else if live(plain()) {
            let p = plain();
            let (ss, se) = selection(p);
            println!(
                "        [{tag}] edit.GetWindowText={:?} edit.big={:?} edit.WM_GETTEXT={:?} sel={ss}..{se}",
                text_of_len_first(p),
                text_of_big(p),
                wm_text(p),
            );
        }
    }

    // ------------------------------------------------------------ msg names

    fn combo_notify_name(c: u32) -> &'static str {
        match c {
            1 => "CBN_SELCHANGE",
            2 => "CBN_DBLCLK",
            3 => "CBN_SETFOCUS",
            4 => "CBN_KILLFOCUS",
            5 => "CBN_EDITCHANGE",
            6 => "CBN_EDITUPDATE",
            7 => "CBN_DROPDOWN",
            8 => "CBN_CLOSEUP",
            9 => "CBN_SELENDOK",
            10 => "CBN_SELENDCANCEL",
            0xFFFF_FFFF => "CBN_ERRSPACE",
            _ => "CBN_?",
        }
    }

    fn edit_notify_name(c: u32) -> &'static str {
        match c {
            0x0100 => "EN_SETFOCUS",
            0x0200 => "EN_KILLFOCUS",
            0x0300 => "EN_CHANGE",
            0x0400 => "EN_UPDATE",
            0x0500 => "EN_ERRSPACE",
            0x0501 => "EN_MAXTEXT",
            _ => "EN_?",
        }
    }

    /// Only the messages that can change an EDIT's text. Logging every
    /// message would bury the two lines that matter under paint traffic.
    fn text_msg_name(m: u32) -> Option<&'static str> {
        match m {
            WM_SETTEXT => Some("WM_SETTEXT"),
            WM_CHAR => Some("WM_CHAR"),
            WM_KEYDOWN => Some("WM_KEYDOWN"),
            WM_PASTE => Some("WM_PASTE"),
            WM_CUT => Some("WM_CUT"),
            WM_CLEAR => Some("WM_CLEAR"),
            WM_UNDO => Some("WM_UNDO"),
            EM_REPLACESEL => Some("EM_REPLACESEL"),
            EM_SETSEL => Some("EM_SETSEL"),
            EM_UNDO => Some("EM_UNDO"),
            _ => None,
        }
    }

    // -------------------------------------------------------------- wndprocs

    /// The instrument for "what actually rewrites the text". Sits on the
    /// combo box's own child EDIT and prints the text before and after the
    /// control handles anything that could change it -- so a rewrite shows up
    /// as a named message with a visible before/after, not as an inference.
    unsafe extern "system" fn edit_sub(
        h: HWND,
        msg: u32,
        wp: WPARAM,
        lp: LPARAM,
        _id: usize,
        _data: usize,
    ) -> LRESULT {
        let watching = WATCH.with(|w| w.get());
        let named = text_msg_name(msg);
        if watching {
            if let Some(n) = named {
                let arg = if msg == WM_SETTEXT && lp.0 != 0 {
                    let p = lp.0 as *const u16;
                    let mut len = 0usize;
                    while len < 512 && *p.add(len) != 0 {
                        len += 1;
                    }
                    format!(
                        " arg={:?}",
                        String::from_utf16_lossy(std::slice::from_raw_parts(p, len))
                    )
                } else if msg == WM_CHAR {
                    format!(" arg={:?}", char::from_u32(wp.0 as u32).unwrap_or('?'))
                } else {
                    String::new()
                };
                println!("      EDIT <- {n}{arg}   before={:?}", wm_text(h));
            }
        }
        let r = DefSubclassProc(h, msg, wp, lp);
        if watching && named.is_some() {
            println!(
                "      EDIT    {} done         after ={:?}",
                text_msg_name(msg).unwrap_or(""),
                wm_text(h)
            );
        }
        r
    }

    unsafe extern "system" fn wndproc(h: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
        match msg {
            WM_COMMAND if WATCH.with(|w| w.get()) => {
                let id = (wp.0 & 0xFFFF) as i32;
                let code = ((wp.0 >> 16) & 0xFFFF) as u32;
                if id == IDC_COMBO {
                    println!("      NOTIFY {} (code {code})", combo_notify_name(code));
                    sample("at notify");
                    // beckon's route: on CBN_EDITCHANGE it does NOT read
                    // here, it posts and reads when the post is dispatched.
                    if code == CBN_EDITCHANGE {
                        if SUPPRESS.with(|s| s.get()) {
                            println!("      (SUPPRESSED -- this notification is apply_state's own write)");
                        } else {
                            let _ = PostMessageW(Some(h), WM_PROBE_READ, WPARAM(0), LPARAM(0));
                            println!("      (posted WM_PROBE_READ, beckon's deferred route)");
                        }
                    }
                } else if id == IDC_PLAIN {
                    println!("      NOTIFY {} (code {code})", edit_notify_name(code));
                    sample("at notify");
                }
                LRESULT(0)
            }
            WM_PROBE_READ => {
                println!("      DISPATCH WM_PROBE_READ");
                sample("at posted read");
                if LOOPING.with(|l| l.get()) {
                    // What beckon records: `text_of(app)` on the COMBOBOX.
                    let recorded = text_of_len_first(combo());
                    MODEL.with(|m| *m.borrow_mut() = recorded.clone());
                    println!("      MODEL := {recorded:?}");
                    // What `apply_state` then does with it, verbatim: the
                    // suppression flag, the `!=` guard, and `SetWindowTextW`.
                    SUPPRESS.with(|s| s.set(true));
                    let shown = text_of_len_first(combo());
                    if shown != recorded {
                        println!(
                            "      apply_state: field {shown:?} != model {recorded:?} -> SetWindowTextW"
                        );
                        let wide: Vec<u16> =
                            recorded.encode_utf16().chain(std::iter::once(0)).collect();
                        let _ = SetWindowTextW(combo(), PCWSTR(wide.as_ptr()));
                    } else {
                        println!("      apply_state: field already equals the model, no write");
                    }
                    SUPPRESS.with(|s| s.set(false));
                    sample("after apply_state");
                    if RELAYOUT.with(|r| r.get()) {
                        // `layout`'s `place_h(ui.app, ...)`, same flags, same
                        // "cy is the dropped-down height" convention.
                        let _ = SetWindowPos(
                            combo(),
                            None,
                            10,
                            10,
                            400,
                            240,
                            SWP_NOZORDER | SWP_NOACTIVATE,
                        );
                        sample("after layout SetWindowPos");
                    }
                }
                LRESULT(0)
            }
            WM_DESTROY => {
                PostQuitMessage(0);
                LRESULT(0)
            }
            _ => DefWindowProcW(h, msg, wp, lp),
        }
    }

    // ----------------------------------------------------------------- pump

    fn pump() {
        unsafe {
            let mut m = MSG::default();
            let mut guard = 0;
            while PeekMessageW(&mut m, None, 0, 0, PM_REMOVE).as_bool() {
                let _ = TranslateMessage(&m);
                DispatchMessageW(&m);
                guard += 1;
                if guard > 500 {
                    break;
                }
            }
        }
    }

    // ------------------------------------------------------------- scenarios

    #[derive(Clone, Copy, PartialEq)]
    enum Kind {
        ComboFull,
        ComboEmpty,
        PlainEdit,
        /// What `apply_state` does: write the model's string back into the
        /// COMBOBOX with `SetWindowTextW`. No typing at all -- this asks one
        /// question, which nobody has asked: does a populated combo box keep
        /// the string it is handed?
        SetTextOnly,
        /// beckon's whole loop, with no beckon in it: type a character, defer
        /// the read exactly as `WM_APP_EDITED` does, take the text as the
        /// model, then push the model back with `SetWindowTextW` behind a
        /// suppression flag -- `apply_state`'s `if text_of(app) != d.app`.
        /// If the hardware defect is reachable from these three steps alone
        /// then it is not a comctl32 timing problem.
        ModelLoop,
        /// `ModelLoop` plus the ONE call that `apply_state` makes after it
        /// and this probe originally did not: `layout`'s `SetWindowPos` on
        /// the COMBOBOX. Everything else is identical to `ModelLoop`, so a
        /// divergence here and agreement there isolates the writer to that
        /// single call.
        ModelLoopWithLayout,
    }

    #[derive(Clone, Copy, PartialEq)]
    enum Inject {
        CharToEditChild,
        CharToCombo,
        SendInput,
    }

    fn inject_label(i: Inject) -> &'static str {
        match i {
            Inject::CharToEditChild => "WM_CHAR -> combo's child EDIT (what settings_probe does)",
            Inject::CharToCombo => "WM_CHAR -> the COMBOBOX itself",
            Inject::SendInput => "SendInput (real keyboard, needs the foreground)",
        }
    }

    /// Build a fresh window with one control, type `WORD` into it one
    /// character at a time, and print everything observable at each step.
    fn scenario(kind: Kind, inject: Inject) {
        let title = match kind {
            Kind::ComboFull => "CBS_DROPDOWN with items",
            Kind::ComboEmpty => "CBS_DROPDOWN with NO items (CONTROL 1)",
            Kind::PlainEdit => "plain EDIT (CONTROL 2)",
            Kind::SetTextOnly => "SetWindowTextW into a POPULATED combo, no typing",
            Kind::ModelLoop => "beckon's read-then-write-back loop, with no beckon in it",
            Kind::ModelLoopWithLayout => "the same loop PLUS layout's SetWindowPos on the combo",
        };
        println!();
        println!("  == {title} == via {}", inject_label(inject));

        COMBO.with(|c| c.set(0));
        ECHILD.with(|c| c.set(0));
        PLAIN.with(|c| c.set(0));
        WATCH.with(|w| w.set(false));

        unsafe {
            let hinst = GetModuleHandleW(None).unwrap_or_default();
            let Ok(top) = CreateWindowExW(
                WINDOW_EX_STYLE(0),
                w!("BeckonComboProbe"),
                w!("combo probe"),
                WS_OVERLAPPEDWINDOW,
                100,
                100,
                520,
                240,
                None,
                None,
                Some(hinst.into()),
                None,
            ) else {
                println!("    FAIL: could not create the probe window");
                return;
            };

            match kind {
                Kind::PlainEdit => {
                    let e = CreateWindowExW(
                        WINDOW_EX_STYLE(0),
                        w!("EDIT"),
                        w!(""),
                        WINDOW_STYLE(ES_AUTOHSCROLL as u32)
                            | WS_CHILD
                            | WS_VISIBLE
                            | WS_BORDER
                            | WS_TABSTOP,
                        10,
                        10,
                        400,
                        28,
                        Some(top),
                        Some(HMENU(IDC_PLAIN as isize as *mut _)),
                        Some(hinst.into()),
                        None,
                    )
                    .unwrap_or_default();
                    PLAIN.with(|c| c.set(e.0 as isize));
                    let _ = SetWindowSubclass(e, Some(edit_sub), 1, 0);
                }
                _ => {
                    // The EXACT styles `settings_window.rs` uses.
                    let c = CreateWindowExW(
                        WINDOW_EX_STYLE(0),
                        w!("COMBOBOX"),
                        w!(""),
                        WINDOW_STYLE((CBS_DROPDOWN | CBS_AUTOHSCROLL | CBS_SORT) as u32)
                            | WS_CHILD
                            | WS_VISIBLE
                            | WS_VSCROLL
                            | WS_TABSTOP,
                        10,
                        10,
                        400,
                        240,
                        Some(top),
                        Some(HMENU(IDC_COMBO as isize as *mut _)),
                        Some(hinst.into()),
                        None,
                    )
                    .unwrap_or_default();
                    COMBO.with(|x| x.set(c.0 as isize));
                    SendMessageW(c, CB_SETMINVISIBLE, Some(WPARAM(8)), Some(LPARAM(0)));
                    if kind != Kind::ComboEmpty {
                        for n in CATALOG {
                            let t: Vec<u16> = n.encode_utf16().chain(std::iter::once(0)).collect();
                            SendMessageW(
                                c,
                                CB_ADDSTRING,
                                Some(WPARAM(0)),
                                Some(LPARAM(t.as_ptr() as isize)),
                            );
                        }
                    }
                    let count = SendMessageW(c, CB_GETCOUNT, Some(WPARAM(0)), Some(LPARAM(0))).0;
                    println!("    items in the list: {count}");
                    // The child EDIT is where the text really lives, and the
                    // only place a rewrite can be caught in the act.
                    let e = FindWindowExW(Some(c), None, w!("EDIT"), None).unwrap_or_default();
                    if live(e) {
                        ECHILD.with(|x| x.set(e.0 as isize));
                        let _ = SetWindowSubclass(e, Some(edit_sub), 1, 0);
                    } else {
                        println!("    NOTE: no child EDIT found on the combo box");
                    }
                }
            }

            let _ = ShowWindow(top, SW_SHOW);
            pump();

            let target = match (kind, inject) {
                (Kind::PlainEdit, _) => plain(),
                (_, Inject::CharToCombo) => combo(),
                _ => echild(),
            };
            if !live(target) {
                println!("    FAIL: no control to type into");
                let _ = DestroyWindow(top);
                pump();
                return;
            }

            // ALWAYS tried, ALWAYS reported, for every scenario -- not just
            // the `SendInput` one.
            //
            // A comctl32 control with no caret and no focus may simply not do
            // the thing being looked for, and this probe's first run was in
            // session 0 where focus is unobtainable. A clean result there and
            // a control that never ran look identical unless the line below
            // is printed, so it is printed even when nothing needs it.
            let _ = SetForegroundWindow(top);
            let _ = SetFocus(Some(target));
            pump();
            std::thread::sleep(Duration::from_millis(200));
            pump();
            let fg = GetForegroundWindow();
            let focused = GetFocus();
            println!(
                "    FOCUS: foreground is ours: {}   focus is on the target: {}",
                fg == top,
                focused == target
            );
            if fg != top || focused != target {
                println!("    FOCUS: NOT FOCUSED -- a clean result below may be a FALSE NEGATIVE");
            }
            if inject == Inject::SendInput && fg != top {
                println!("    SKIPPED: SendInput needs the foreground and never got it");
                let _ = DestroyWindow(top);
                pump();
                return;
            }

            // Clear, exactly as `settings_probe::type_into` does.
            let empty: [u16; 1] = [0];
            SendMessageW(
                target,
                WM_SETTEXT,
                Some(WPARAM(0)),
                Some(LPARAM(empty.as_ptr() as isize)),
            );
            pump();

            WATCH.with(|w| w.set(true));

            // The two scenarios that do not type. Both return early.
            if kind == Kind::SetTextOnly {
                for n in 1..=WORD.len() {
                    let s = &WORD[..n];
                    println!("    --- SetWindowTextW({s:?}) ---");
                    let wide: Vec<u16> = s.encode_utf16().chain(std::iter::once(0)).collect();
                    SetWindowTextW(combo(), PCWSTR(wide.as_ptr())).ok();
                    pump();
                    sample("after SetWindowTextW");
                }
                println!("    --- now one WM_CHAR 'X' on top of whatever is there ---");
                SendMessageW(target, WM_CHAR, Some(WPARAM('X' as usize)), Some(LPARAM(1)));
                pump();
                sample("after one more char");
                WATCH.with(|w| w.set(false));
                if live(echild()) {
                    let _ = RemoveWindowSubclass(echild(), Some(edit_sub), 1);
                }
                let _ = DestroyWindow(top);
                pump();
                return;
            }

            if kind == Kind::ModelLoop || kind == Kind::ModelLoopWithLayout {
                MODEL.with(|m| m.borrow_mut().clear());
                LOOPING.with(|l| l.set(true));
                RELAYOUT.with(|r| r.set(kind == Kind::ModelLoopWithLayout));
            }

            for (i, ch) in WORD.encode_utf16().enumerate() {
                println!(
                    "    --- char {i} {:?} ---",
                    char::from_u32(ch as u32).unwrap_or('?')
                );
                if inject == Inject::SendInput {
                    let down = INPUT {
                        r#type: INPUT_KEYBOARD,
                        Anonymous: windows::Win32::UI::Input::KeyboardAndMouse::INPUT_0 {
                            ki: KEYBDINPUT {
                                wVk: windows::Win32::UI::Input::KeyboardAndMouse::VIRTUAL_KEY(0),
                                wScan: ch,
                                dwFlags: KEYEVENTF_UNICODE,
                                time: 0,
                                dwExtraInfo: 0,
                            },
                        },
                    };
                    let mut up = down;
                    up.Anonymous.ki.dwFlags = KEYEVENTF_UNICODE | KEYEVENTF_KEYUP;
                    SendInput(&[down, up], std::mem::size_of::<INPUT>() as i32);
                    std::thread::sleep(Duration::from_millis(60));
                } else {
                    SendMessageW(target, WM_CHAR, Some(WPARAM(ch as usize)), Some(LPARAM(1)));
                }
                pump();
                sample("after pump");
                std::thread::sleep(Duration::from_millis(150));
                pump();
                sample("after 150ms");
            }
            WATCH.with(|w| w.set(false));
            if kind == Kind::ModelLoop || kind == Kind::ModelLoopWithLayout {
                LOOPING.with(|l| l.set(false));
                RELAYOUT.with(|r| r.set(false));
                let m = MODEL.with(|m| m.borrow().clone());
                let f = text_of_len_first(combo());
                println!(
                    "    RESULT: typed {WORD:?} -> model {m:?}, field {f:?}   {}",
                    if m == f {
                        "AGREE"
                    } else {
                        "<<< DISAGREE -- the hardware defect, reproduced"
                    }
                );
            }

            if live(echild()) {
                let _ = RemoveWindowSubclass(echild(), Some(edit_sub), 1);
            }
            if live(plain()) {
                let _ = RemoveWindowSubclass(plain(), Some(edit_sub), 1);
            }
            let _ = DestroyWindow(top);
            pump();
        }
    }

    // ------------------------------------------------------------- comctl32

    fn comctl_path() -> String {
        unsafe {
            match GetModuleHandleW(w!("comctl32.dll")) {
                Ok(m) if !m.is_invalid() => {
                    let mut buf = [0u16; 512];
                    let n = GetModuleFileNameW(Some(m), &mut buf);
                    String::from_utf16_lossy(&buf[..n as usize])
                }
                _ => String::from("<not loaded>"),
            }
        }
    }

    /// `DLLVERSIONINFO`, spelled out rather than imported: `DllGetVersion` is
    /// an ordinal-free export every common-control DLL carries, and its
    /// struct is four `DWORD`s.
    #[repr(C)]
    #[derive(Default)]
    struct DllVersionInfo {
        cb_size: u32,
        major: u32,
        minor: u32,
        build: u32,
        platform_id: u32,
    }

    /// Which comctl32 this process actually got.
    ///
    /// **The path alone is not the answer, and reading it as one is how the
    /// first run of this probe misreported itself.** Both versions live under
    /// `WinSxS`; the discriminator is the version, where 5.82 is v5 and 6.0
    /// is v6. `DllGetVersion` is asked directly so the answer does not depend
    /// on parsing a directory name.
    fn report_comctl(tag: &str) {
        let path = comctl_path();
        let mut ver = String::from("<no DllGetVersion>");
        unsafe {
            if let Ok(m) = GetModuleHandleW(w!("comctl32.dll")) {
                if !m.is_invalid() {
                    if let Some(p) = GetProcAddress(m, windows::core::s!("DllGetVersion")) {
                        type F = unsafe extern "system" fn(*mut DllVersionInfo) -> i32;
                        let f: F = std::mem::transmute(p);
                        let mut v = DllVersionInfo {
                            cb_size: std::mem::size_of::<DllVersionInfo>() as u32,
                            ..Default::default()
                        };
                        if f(&mut v) == 0 {
                            ver = format!("{}.{}.{}", v.major, v.minor, v.build);
                        }
                    }
                }
            }
        }
        let v6 = ver.starts_with("6.");
        println!("  comctl32 [{tag}]: {path}");
        println!("  comctl32 [{tag}]: DllGetVersion = {ver}   -> version 6: {v6}");
    }

    pub fn run() {
        let sendinput = std::env::args().any(|a| a == "--sendinput");
        println!("== combo_probe ==");
        println!("typing {WORD:?} into each control, one WM_CHAR at a time");
        // CONTROL 3 is run from OUTSIDE this binary: the caller runs it once
        // as `cargo` built it and once with beckon's own manifest stamped in
        // by `mt.exe`. An activation context created here would come too
        // late -- comctl32 is already loaded by the time `main` runs, because
        // this file links it for `SetWindowSubclass`.
        report_comctl("this process");

        unsafe {
            let hinst = GetModuleHandleW(None).unwrap_or_default();
            let icc = INITCOMMONCONTROLSEX {
                dwSize: std::mem::size_of::<INITCOMMONCONTROLSEX>() as u32,
                dwICC: ICC_STANDARD_CLASSES,
            };
            let _ = InitCommonControlsEx(&icc);
            let wc = WNDCLASSW {
                lpfnWndProc: Some(wndproc),
                hInstance: hinst.into(),
                lpszClassName: w!("BeckonComboProbe"),
                hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
                ..Default::default()
            };
            RegisterClassW(&wc);
        }

        scenario(Kind::ComboFull, Inject::CharToEditChild);
        scenario(Kind::ComboEmpty, Inject::CharToEditChild);
        scenario(Kind::PlainEdit, Inject::CharToEditChild);
        scenario(Kind::ComboFull, Inject::CharToCombo);
        scenario(Kind::SetTextOnly, Inject::CharToEditChild);
        scenario(Kind::ModelLoop, Inject::CharToEditChild);
        scenario(Kind::ModelLoopWithLayout, Inject::CharToEditChild);
        if sendinput {
            scenario(Kind::ComboFull, Inject::SendInput);
            scenario(Kind::ModelLoop, Inject::SendInput);
        }

        println!();
        println!("== done ==");
    }
}
