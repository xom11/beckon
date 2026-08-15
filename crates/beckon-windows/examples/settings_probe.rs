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
    use windows::Win32::Graphics::Gdi::{ClientToScreen, MapWindowPoints};
    use windows::Win32::System::Diagnostics::Debug::{ReadProcessMemory, WriteProcessMemory};
    use windows::Win32::System::Memory::{
        VirtualAllocEx, VirtualFreeEx, MEM_COMMIT, MEM_RELEASE, MEM_RESERVE, PAGE_READWRITE,
    };
    use windows::Win32::System::Threading::{
        OpenProcess, PROCESS_VM_OPERATION, PROCESS_VM_READ, PROCESS_VM_WRITE,
    };
    // `BST_CHECKED` is deliberately gone: nothing here asks `BM_GETCHECK`
    // any more. See `chip_armed`.
    use windows::Win32::UI::Controls::{
        BCM_GETIDEALSIZE, LVIF_TEXT, LVIR_BOUNDS, LVITEMW, LVM_GETCOLUMNWIDTH, LVM_GETCOUNTPERPAGE,
        LVM_GETHEADER, LVM_GETITEMCOUNT, LVM_GETITEMRECT, LVM_GETITEMTEXTW, LVS_NOCOLUMNHEADER,
    };
    use windows::Win32::UI::HiDpi::{
        GetAwarenessFromDpiAwarenessContext, GetDpiForSystem, GetDpiForWindow,
        GetSystemMetricsForDpi, GetThreadDpiAwarenessContext, SetProcessDpiAwarenessContext,
        DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
    };
    use windows::Win32::UI::Input::KeyboardAndMouse::IsWindowEnabled;
    use windows::Win32::UI::WindowsAndMessaging::*;

    const WM_TRAY: u32 = WM_APP + 1;
    /// Spelled as winuser.h gives it; `windows` 0.61 files `EM_*` under a
    /// different module from the `WM_*` glob this file imports.
    const EM_GETSEL: u32 = 0x00B0;

    thread_local! {
        /// The App COMBOBOX, while `type_into` is driving its child EDIT.
        /// Set by `drive_an_edit` around the App half only, so the Shortcut
        /// control run prints nothing extra and stays a clean comparison.
        static APP_COMBO: RefCell<isize> = const { RefCell::new(0) };
    }

    /// The four numbers that separate "the combo box rewrote itself" from
    /// "somebody wrote to it".
    ///
    /// `combo_probe` established on this machine (comctl32 6.16, session 1,
    /// real focus, real keystrokes) that a populated `CBS_DROPDOWN` does NOT
    /// autocomplete: `cursel` stays -1 and the child EDIT is sent nothing but
    /// `WM_CHAR`. So if the field here ever holds a catalog entry the user
    /// did not type, one of these says where it came from:
    ///
    /// - `cursel` other than -1 means the LIST selection moved, i.e. a
    ///   `CB_SETCURSEL` / `CB_SELECTSTRING` reached the control;
    /// - `sel` spanning the whole text means something selected it, which is
    ///   what a select-string does and what typing does not;
    /// - `combo` differing from the field means `GetWindowTextW` on the
    ///   COMBOBOX (what `settings_window::text_of` reads) and `WM_GETTEXT` on
    ///   its child EDIT (what this probe reads) are not the same string.
    fn combo_detail(edit: HWND) -> String {
        let c = APP_COMBO.with(|c| *c.borrow());
        if c == 0 {
            return String::new();
        }
        let combo = HWND(c as *mut c_void);
        unsafe {
            let count = SendMessageW(combo, CB_GETCOUNT, Some(WPARAM(0)), Some(LPARAM(0))).0;
            let cursel = SendMessageW(combo, CB_GETCURSEL, Some(WPARAM(0)), Some(LPARAM(0))).0;
            // Both pointers NULL: the packed result carries start in the low
            // word and end in the high word, which is the only form that
            // works across a process boundary without a remote buffer.
            let sel = SendMessageW(edit, EM_GETSEL, Some(WPARAM(0)), Some(LPARAM(0))).0;
            // An open drop-down is the one state in which comctl32 does
            // search the list as you type, so it has to be ruled out by
            // reading it rather than by assuming the probe never opened it.
            let dropped =
                SendMessageW(combo, CB_GETDROPPEDSTATE, Some(WPARAM(0)), Some(LPARAM(0))).0;
            format!(
                "  [items={count} cursel={cursel} sel={}..{} dropped={dropped} combo={:?}]",
                sel & 0xFFFF,
                (sel >> 16) & 0xFFFF,
                ctl_text(combo),
            )
        }
    }

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
    /// The shortcut control. It kept this number when it stopped being an
    /// EDIT and became the key `COMBOBOX` -- which is exactly why the id was
    /// reused rather than retired: this probe pins 1002, so a renumber would
    /// have left it reading a control that no longer exists while still
    /// printing numbers.
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
    /// The Caps-tap list -- the second `CBS_DROPDOWNLIST | CBS_OWNERDRAWFIXED`
    /// control Task 9 built, alongside `IDC_COMBO`. Unmoved since it was
    /// added; pinned here only so `measure_geometry` can read its style bits
    /// and its three fixed items.
    const IDC_TAP: i32 = 1025;
    /// The four modifier check boxes that spell a shortcut alongside
    /// `IDC_COMBO`. Driven, not merely read: `BM_CLICK` on one of these is
    /// the real path a mouse takes, unlike the synthesised `WM_COMMAND`
    /// `click` posts for a push button.
    const IDC_MOD_CTRL: i32 = 1028;
    const IDC_MOD_WIN: i32 = 1029;
    const IDC_MOD_ALT: i32 = 1030;
    const IDC_MOD_SHIFT: i32 = 1031;
    /// The four tab pills, in the order `settings_window::TABS` holds them --
    /// which is also the order `layout` places them left to right, so a rect
    /// that goes backwards down this list is a placement bug and not a
    /// transcription one.
    ///
    /// **Contiguous, and that is a requirement rather than tidiness.**
    /// `CheckRadioButton(hwnd, IDC_TAB_SHORTCUTS, IDC_TAB_ABOUT, id)` is what
    /// lights a pill, and it takes a FIRST and a LAST id and walks the range
    /// between them. Transcribed here like every other id in this file: the
    /// probe drives another process and cannot link the crate.
    const TAB_PILLS: [(i32, &str); 4] = [
        (1040, "Shortcuts"),
        (1041, "Keyboard"),
        (1042, "System"),
        (1043, "About"),
    ];
    /// `[(id, the word the config file spells it with, the word the SCREEN
    /// spells it with)]`, in the canonical order `Combo::canonical` prints.
    /// The probe reconstructs the whole combo from the controls, so it needs
    /// its own copy of that order -- which is the point: a copy that agrees
    /// is a check, and one derived from the window's own would only ever
    /// agree with itself.
    ///
    /// **Two spellings, because the window uses two.** `super` is what goes
    /// in the TOML; `Win` is what the Shortcut column shows, because nothing
    /// on a Windows keyboard is labelled `super`. `shortcut_shown` builds the
    /// first, `shortcut_caps` the second, and only the second is comparable
    /// to a list cell.
    const MODIFIERS: [(i32, &str, &str); 4] = [
        (IDC_MOD_CTRL, "ctrl", "Ctrl"),
        (IDC_MOD_WIN, "super", "Win"),
        (IDC_MOD_ALT, "alt", "Alt"),
        (IDC_MOD_SHIFT, "shift", "Shift"),
    ];
    /// How many keys `beckon_core::shortcuts::key_table()` holds, and the
    /// order it holds them in at seven fixed points. The probe cannot link
    /// the table (it drives another process), so this is the independent
    /// copy that makes `CB_SETCURSEL i == key_table()[i]` a CHECKED claim on
    /// hardware rather than a comment.
    const KEY_COUNT: isize = 81;
    const KEY_ORDER: [(isize, &str); 7] = [
        (0, "a"),
        (25, "z"),
        (26, "0"),
        (36, "f1"),
        (55, "f20"),
        (56, "comma"),
        (80, "down"),
    ];

    /// The window's own logical size at 96 DPI, and its floor -- transcribed
    /// from `settings_window::mod::{WINDOW_WIDTH, WINDOW_HEIGHT, MIN_WIDTH,
    /// MIN_HEIGHT}`. Same reasoning as `KEY_COUNT`/`KEY_ORDER`: the probe
    /// drives another process and cannot link the crate, so this is an
    /// independent copy that agrees with the source today, and a later
    /// resize that changes one without the other shows up as a
    /// disagreement on hardware rather than being absorbed silently.
    ///
    /// **The independence is real and it did not save us.** These four sat
    /// at 900/740/753/702 from the 2026-08-13 compaction pass until
    /// 2026-08-14 -- the probe would have printed `<<< FAIL` against a
    /// perfectly healthy window, and nobody saw it, because the mechanism
    /// only fires with a person at a14. The copy stays (it is what catches a
    /// probe driving an OLDER binary), and `geometry_matches_the_probe` in
    /// `settings_window::ids` now catches the source-level drift without
    /// leaving this machine.
    ///
    /// **That test earned its keep the same day it was written.** Task 8 took
    /// `WINDOW_WIDTH` 760 -> 680 and `geometry_matches_the_probe` failed on
    /// the unedited copy below, on the developer's own machine, with no
    /// hardware in the loop -- which is the exact drift the paragraph above
    /// says nobody would otherwise have seen until a person ran this probe.
    const WINDOW_WIDTH_96: i32 = 680;
    const WINDOW_HEIGHT_96: i32 = 600;
    /// Printed for reference only. What has to be checked at this floor
    /// needs a human to drag the corner, and this probe does not drive a
    /// resize.
    ///
    /// **CORRECTED 2026-08-14: that check is no longer "gate 09 (eight rows,
    /// no scrollbar)".** Two things were wrong with the sentence. Gate 09 of
    /// the redesign plan is eight rows at the SHIPPED size against a 20-row
    /// config, not at the floor -- the floor has never shown eight. And the
    /// tab strip's band (`tok::TABSTRIP_H`) takes 34 px out of the list, so
    /// the shipped 600 now caps it at seven rows and the floor at two; the
    /// derivation is under `MIN_HEIGHT` in `settings_window::mod`, which
    /// withdraws the four-row guarantee in the same landing. Eight rows is
    /// not a property of this window any more, at either size.
    const MIN_WIDTH_96: i32 = 660;
    const MIN_HEIGHT_96: i32 = 560;

    /// A 96-DPI value scaled to `dpi`, transcribed from
    /// `settings_window::mod::scale` -- truncating, not `MulDiv`'s
    /// round-half-up, because the source's own doc comment says the two
    /// disagree at in-between DPIs and this has to match the source, not
    /// the "nicer" rounding.
    fn scale96(v: i32, dpi: u32) -> i32 {
        v * dpi as i32 / 96
    }

    fn dlg_item(parent: HWND, id: i32) -> Option<HWND> {
        match unsafe { GetDlgItem(Some(parent), id) } {
            Ok(h) if !h.0.is_null() => Some(h),
            _ => None,
        }
    }

    /// `WM_APP + 5` -- `settings_window::WM_CHIP_STATE`. See `chip_armed`.
    const WM_CHIP_STATE: u32 = WM_APP + 5;

    /// Is this chip armed, in the other process?
    ///
    /// **`BM_GETCHECK` is not the question any more, and asking it would be
    /// the "measured a proxy" failure again.** The four modifier chips and
    /// the three `Hold` chips are `BS_OWNERDRAW`, which REPLACES
    /// `BS_AUTOCHECKBOX` rather than joining it, so those controls have no
    /// check state: `BM_GETCHECK` answers 0 forever and every chip would
    /// read as clear. `shortcut_shown` and `shortcut_caps` build a whole
    /// chord out of this function, so that would not have failed loudly --
    /// it would have reported a confident wrong shortcut on every run.
    ///
    /// The window answers `WM_CHIP_STATE` with its own bit instead: a bare
    /// integer message, so unlike the comctl32 messages `Remote` exists for
    /// it needs no marshalling across the process boundary.
    ///
    /// **Sent to the WINDOW, not to the chip**, which is the one thing about
    /// this that does not read like `BM_GETCHECK`. `BM_GETCHECK` asked the
    /// control because a check box owned its own state; `WM_CHIP_STATE` is
    /// answered by `wndproc`, and the id travels in `WPARAM`. Addressed to
    /// the button it reaches comctl32's BUTTON instead, which does not know
    /// the message and returns 0 -- indistinguishable from an old build.
    /// Measured on a14: the first run of this function did exactly that and
    /// reported every chip unreadable while the window was working.
    ///
    /// `dlg_item` still runs, and only as an existence check: a chip that is
    /// not there must read as `None` rather than as whatever the window says
    /// about an id it has no control for.
    ///
    /// **`0` means the window did not answer**, which is what an older
    /// `beckon-serve` returns through `DefWindowProcW`. Reported rather than
    /// folded into `false`: a probe that cannot see the chips must say so,
    /// not print a chord it guessed.
    fn chip_armed(parent: HWND, id: i32) -> Option<bool> {
        dlg_item(parent, id)?;
        match send(parent, WM_CHIP_STATE, id as usize, 0) {
            2 => Some(true),
            1 => Some(false),
            _ => None,
        }
    }

    /// The same question, collapsed for the callers that build a string out
    /// of it. An unanswered chip reads as clear -- `chips_readable` is what
    /// says the whole reading is worthless, once, rather than every caller
    /// carrying an `Option` it would only unwrap the same way.
    fn checked(parent: HWND, id: i32) -> bool {
        chip_armed(parent, id).unwrap_or(false)
    }

    /// Does this window answer `WM_CHIP_STATE` at all?
    ///
    /// Called once per run, before anything reads a chord, so a probe driving
    /// a `beckon-serve` older than `WM_CHIP_STATE` says so in one line
    /// instead of printing four unticked chips as if it had looked.
    fn chips_readable(parent: HWND) -> bool {
        MODIFIERS
            .iter()
            .all(|(id, _, _)| chip_armed(parent, *id).is_some())
    }

    /// One item's text out of a combo box in ANOTHER process.
    ///
    /// `CB_GETLBTEXT` takes a pointer, and unlike the comctl32 messages
    /// `Remote` exists for, it IS marshalled: a COMBOBOX is one of user32's
    /// own controls, which is the same reason `WM_GETTEXT` works above and
    /// `LVM_GETITEMTEXT` does not. `CB_ERR` (-1) comes back for an index the
    /// list does not have, and is reported as `None` rather than folded into
    /// an empty string -- an empty item and a failed read must not print the
    /// same.
    fn combo_item(combo: HWND, i: isize) -> Option<String> {
        let mut buf = [0u16; 128];
        let n = send(combo, CB_GETLBTEXT, i as usize, buf.as_mut_ptr() as isize);
        if n < 0 {
            return None;
        }
        Some(String::from_utf16_lossy(
            &buf[..(n as usize).min(buf.len())],
        ))
    }

    /// What the key list has selected: `(index, name)`, or `None` for
    /// nothing selected.
    ///
    /// **`CB_GETCURSEL` + `CB_GETLBTEXT`, never `WM_GETTEXT`.** A
    /// `CBS_DROPDOWNLIST` answers `WM_GETTEXT` with the selected item's
    /// text, which looks like the same answer and is not: it cannot tell
    /// "nothing selected" from "an item whose text is empty", and it is
    /// precisely the read the window itself is forbidden to make -- so a
    /// probe making it would be checking a contract nobody has to keep.
    fn key_sel(parent: HWND) -> Option<(isize, String)> {
        let combo = dlg_item(parent, IDC_COMBO)?;
        let i = send(combo, CB_GETCURSEL, 0, 0);
        if i < 0 {
            return None;
        }
        Some((i, combo_item(combo, i)?))
    }

    /// The whole shortcut the five controls currently show, spelled the way
    /// the config file spells it: `ctrl+super+alt+t`.
    ///
    /// **This is the MODEL's spelling, and it is not what any list cell
    /// says.** It is printed by `dump` beside the display spelling so the two
    /// can be read against each other; comparing it to a cell is what
    /// `shortcut_caps` is for. The key list holds the config names verbatim
    /// (the window fills it from `key_table()`, whose `name` is the TOML
    /// token), so the item text goes in here unchanged -- and that is not an
    /// assumption, it is what `KEY_ORDER` checks on hardware: it expects item
    /// 56 to read `comma`, not `,`. If the list ever showed labels instead,
    /// `KEY_ORDER` fails first and loudly, before this function can quietly
    /// start writing them into a config spelling.
    ///
    /// `""` when no key is selected -- which is a state the window is
    /// entitled to be in, and the one in which it must send the model
    /// nothing at all.
    fn shortcut_shown(parent: HWND) -> String {
        let Some((_, key)) = key_sel(parent) else {
            return String::new();
        };
        let mut parts: Vec<&str> = MODIFIERS
            .iter()
            .filter(|(id, _, _)| checked(parent, *id))
            .map(|(_, word, _)| *word)
            .collect();
        parts.push(&key);
        parts.join("+")
    }

    /// One key's cap as the Shortcut column draws it -- the probe's OWN copy
    /// of `beckon_core::shortcuts::key_label`.
    ///
    /// **The copy is the point, exactly as it is for `KEY_ORDER`.** The probe
    /// drives another process and cannot link the crate; calling
    /// `combo_display` here -- even if the linkage existed -- would compare
    /// the window's output against the same function that produced it, which
    /// agrees by construction and checks nothing.
    ///
    /// Independent at RUN time, which is the property that matters: this
    /// table was transcribed from `key_label`, not invented, but it does not
    /// resolve to it, so a later edit on either side shows up as a
    /// disagreement on hardware instead of being absorbed silently. The
    /// failure it exists to catch is `key_label` drifting -- or the window
    /// reverting the column to `Combo::canonical` -- while the probe goes on
    /// reporting MATCH.
    ///
    /// Exhaustive rather than "uppercase and hope": the punctuation and
    /// navigation keys are precisely where a display table drifts, and they
    /// are the ones no `assert_eq!` in `beckon-core` can see on a real
    /// ListView.
    fn key_cap(name: &str) -> String {
        match name {
            "space" => "Space".to_string(),
            "comma" => ",".to_string(),
            "period" => ".".to_string(),
            "slash" => "/".to_string(),
            "minus" => "-".to_string(),
            "equal" => "=".to_string(),
            "semicolon" => ";".to_string(),
            "quote" => "'".to_string(),
            "bracketleft" => "[".to_string(),
            "bracketright" => "]".to_string(),
            "backslash" => "\\".to_string(),
            "grave" => "`".to_string(),
            "tab" => "Tab".to_string(),
            "return" => "Enter".to_string(),
            "escape" => "Esc".to_string(),
            "backspace" => "Backspace".to_string(),
            "delete" => "Del".to_string(),
            "home" => "Home".to_string(),
            "end" => "End".to_string(),
            "pageup" => "PgUp".to_string(),
            "pagedown" => "PgDn".to_string(),
            "up" => "Up".to_string(),
            "down" => "Down".to_string(),
            "left" => "Left".to_string(),
            "right" => "Right".to_string(),
            // Letters, digits and f1-f20 uppercase whole: `t` -> `T`,
            // `f10` -> `F10`, `7` -> `7`.
            other => other.to_uppercase(),
        }
    }

    /// The whole shortcut the five controls currently show, spelled the way
    /// the SHORTCUT COLUMN spells it: `Ctrl + Win + Alt + T`.
    ///
    /// **This is the expectation `expect_shown` compares a cell against**, and
    /// it exists because the column stopped being the config string verbatim:
    /// the cell is `combo_display`'s output now, so an expectation built from
    /// `shortcut_shown` disagreed with a correct window at every step and
    /// turned this whole half -- the CONTROL for the App half -- into 5/5
    /// false alarms.
    ///
    /// Built from `MODIFIERS`' third column and `key_cap`, both of which are
    /// the probe's own; the only thing read out of the window is which boxes
    /// are ticked and which key is selected.
    ///
    /// `""` when no key is selected, for the same reason `shortcut_shown`
    /// returns it.
    fn shortcut_caps(parent: HWND) -> String {
        let Some((_, key)) = key_sel(parent) else {
            return String::new();
        };
        let mut parts: Vec<String> = MODIFIERS
            .iter()
            .filter(|(id, _, _)| checked(parent, *id))
            .map(|(_, _, cap)| (*cap).to_string())
            .collect();
        parts.push(key_cap(&key));
        parts.join(" + ")
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
    /// (`Notepad   missing`), so the field's text is a PREFIX of the
    /// cell rather than the whole of it -- which is what the second arm
    /// allows for. The separator is what this reads, never the word, so the
    /// 2026-08-15 rewording of the four flags did not touch the logic.
    ///
    /// **The Shortcut column is NOT the combo verbatim.** It used to be, and
    /// this comment used to say so; the column now shows the chord as a
    /// keyboard spells it (`Ctrl + Win + Alt + T`), never the TOML token
    /// (`ctrl+super+alt+t`). It carries no flag suffix, so it only ever takes
    /// the first arm -- and the expectation handed in must come from
    /// `shortcut_caps`, not `shortcut_shown`.
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

        // The two column widths, and the arithmetic Task 8 claims they come
        // from. `layout` sizes them off the list's OWN client width less
        // `SM_CXVSCROLL`, unconditionally -- so `App` is
        // `client - SM_CXVSCROLL - min(200, .)` and it loses a second
        // `SM_CXVSCROLL` once a scroll bar is actually up and the client
        // shrinks under it. Both figures are printed because the difference
        // between them is the whole point: the design's "~438 px for the app
        // name" is the client width less the Shortcut column and forgets both
        // subtractions.
        //
        // `LVM_GETCOLUMNWIDTH` returns a plain integer, so unlike
        // `LVM_GETITEMRECT` above it needs no remote buffer.
        // `w` is the control's WINDOW width, read at the top of this function;
        // the client width below is what is left of it once comctl32 has taken
        // a scroll bar. Comparing the two is how the last field says whether
        // one is up, and it is why both are printed rather than only the one
        // `layout` reads.
        let lw = w;
        let mut lrc = RECT::default();
        let client_w = if unsafe { GetClientRect(list, &mut lrc) }.is_ok() {
            lrc.right - lrc.left
        } else {
            RECT_FAIL
        };
        let sb = unsafe { GetSystemMetricsForDpi(SM_CXVSCROLL, GetDpiForWindow(parent)) };
        println!(
            "      columns:              App {}   Shortcut {}   (window {lw}, client \
             {client_w}, SM_CXVSCROLL {sb}, scroll bar {})",
            send(list, LVM_GETCOLUMNWIDTH, 0, 0),
            send(list, LVM_GETCOLUMNWIDTH, 1, 0),
            if client_w != RECT_FAIL && lw != RECT_FAIL && lw - client_w >= sb {
                "UP"
            } else {
                "down"
            }
        );

        // **The header is meant to be ABSENT** since 2026-08-15 (design 3.1),
        // and this block flipped from measuring it to checking that it is not
        // there. `LVS_NOCOLUMNHEADER` is the lever `build_children` pulls, so
        // the STYLE is the primary reading and the window is the corroboration
        // -- comctl32 is not documented to destroy the Header window when the
        // style is set, only to stop showing it, so a live `SysHeader32` HWND
        // here is not by itself a failure. A VISIBLE one is.
        //
        // Its height is still printed when there is one, because that is the
        // number `compute_card_rects` stopped subtracting: `list_header_height`
        // is deleted and `want`'s header term with it, so a header with a
        // non-zero height on screen means the list is overlapping the rows the
        // arithmetic thinks it has.
        let style = unsafe { GetWindowLongPtrW(list, GWL_STYLE) } as u32;
        println!(
            "      LVS_NOCOLUMNHEADER:   {}  {}",
            style & LVS_NOCOLUMNHEADER != 0,
            if style & LVS_NOCOLUMNHEADER != 0 {
                "ok"
            } else {
                "<<< FAIL -- the column headers are back"
            }
        );
        let hdr = HWND(send(list, LVM_GETHEADER, 0, 0) as *mut c_void);
        if hdr.0.is_null() {
            println!("      header:               none -- LVM_GETHEADER returned null");
        } else {
            let shown = unsafe { IsWindowVisible(hdr) }.as_bool();
            let (_, _, hw, hh) = box_in_client(parent, hdr);
            let size = if hh == RECT_FAIL {
                "RECTFAIL".to_string()
            } else {
                format!("{hw}x{hh}")
            };
            println!(
                "      header SysHeader32:   exists, visible {shown}, {size}  {}",
                if shown {
                    "<<< FAIL -- a header band is on screen"
                } else {
                    "ok -- window exists, draws nothing"
                }
            );
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

        // `WM_NCCALCSIZE` (`chrome::nccalcsize`, chrome.rs:142) returns 0
        // without calling `DefWindowProcW` at all, so the proposed rect is
        // handed back untouched and the client becomes the WHOLE window --
        // every edge, not just the top. The client's on-screen origin should
        // therefore sit EXACTLY at the window's own top edge: 0 px of inset.
        // That is a property of the code, not of the hardware, so the top
        // figure below is asserted; the bottom figure is only printed; see
        // the comment beside it for why.
        //
        // **CORRECTED 2026-08-14: the inset this replaces is a system
        // metric, never `chrome::TITLEBAR_H`.** The sentence used to read
        // "where an unmodified `WS_CAPTION` window would show
        // `chrome::TITLEBAR_H` (40 @96 DPI) plus a border", which quoted a
        // stale number and conflated two unrelated ones. `TITLEBAR_H` is
        // **34** -- chrome.rs:74, moved 40 -> 34 by the 2026-08-13
        // compaction pass `1f46335` -- and it is the band beckon paints
        // INSIDE the client (chrome.rs:292), a figure the OS has never been
        // told about and would never reserve. What `DefWindowProcW` would
        // reserve is `SM_CYSIZEFRAME + SM_CXPADDEDBORDER` for
        // `WS_THICKFRAME`, the same pair `chrome::nchittest` sizes its
        // resize strips from, plus `SM_CYCAPTION` on top for a window that
        // asked for a caption. This one does not ask: `WS_POPUP |
        // WS_SYSMENU | WS_THICKFRAME | WS_MINIMIZEBOX`, no `WS_CAPTION`
        // (mod.rs:1944).
        let mut wrc = RECT::default();
        let mut origin = POINT { x: 0, y: 0 };
        if unsafe { GetWindowRect(parent, &mut wrc) }.is_ok()
            && unsafe { ClientToScreen(parent, &mut origin) }.as_bool()
        {
            let top_inset = origin.y - wrc.top;
            println!(
                "    client top inset from window top: {top_inset}px   {}",
                if top_inset == 0 {
                    "MATCH -- the caption band was given back to the client"
                } else {
                    "<<< FAIL -- still inset as if a system caption were reserved"
                }
            );
            // Everything left over (window height minus client height minus
            // the top inset already accounted for) is whatever the OS still
            // reserves at the BOTTOM.
            //
            // **CORRECTED 2026-08-14: it reserves nothing there either.**
            // This text was written against `nccalcsize`'s old body, which
            // called `DefWindowProcW` and then wrote `rgrc[0].top =
            // before.top` back over the answer -- so the sides and the
            // bottom kept whatever `DefWindowProcW` made them, and "one
            // border or two" was a real question. `c523e8e` deleted that
            // body two hours later the same evening, because leaving those
            // three borders non-client had DWM painting them black once
            // `WS_CAPTION` was gone: `nccalcsize` now returns 0 without
            // calling `DefWindowProcW`, so the expected bottom inset is 0,
            // the same as the top.
            //
            // Still printed rather than asserted, for a different reason
            // than before: that expectation is read off the source, and
            // nobody has read this figure back on hardware since the frame
            // was reclaimed. A non-zero bottom is the first thing that would
            // say the reclaim did not take.
            let vertical_inset = (wrc.bottom - wrc.top) - (rc.bottom - rc.top);
            println!(
                "    total vertical inset (window - client): {vertical_inset}px \
                 ({top_inset}px top + {}px bottom)",
                vertical_inset - top_inset
            );
        } else {
            println!("    client/window top-inset check: FAIL -- could not read one of the rects");
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
            // Closed height, like IDC_APP below: under v6 the drop-down is a
            // separate popup, so the control's own rect is what is on screen
            // while it is shut.
            let (_, _, w, h) = box_in_client(parent, ctl);
            let st = unsafe { GetWindowLongPtrW(ctl, GWL_STYLE) } as u32;
            let count = send(ctl, CB_GETCOUNT, 0, 0);
            println!(
                "    COMBOBOX IDC_COMBO:   {} closed   CB_GETCOUNT {count}   style 0x{st:08X}",
                fmt_wh(w, h)
            );
            // Four claims, none of them checkable anywhere else.
            //
            // CBS_DROPDOWNLIST is what makes the resize defect structurally
            // impossible on this control: a CBS_DROPDOWN would have an edit
            // field for `SetWindowPos` to re-synchronise, which is what cost
            // this project a day. CBS_OWNERDRAWFIXED, added in Task 9, is
            // what makes the tick-centring and font-role gates (05, 08)
            // possible at all -- without it `paint::draw_combo_item` never
            // runs and the row falls back to comctl32's own default draw.
            // CBS_SORT absent, plus the fixed points below, ARE the index
            // contract -- `CB_SETCURSEL i` means `key_table()[i]`, and that
            // holds only while the list is in the table's own order.
            // Sorted, `f10` would come before `f2` and every selection would
            // name the wrong key, silently, and invisibly to every unit
            // test.
            println!(
                "      CBS_DROPDOWNLIST: {}   CBS_OWNERDRAWFIXED: {}   CBS_SORT: {}",
                if st & 0x3 == 3 { "yes" } else { "NO <<< FAIL" },
                if st & 0x0010 != 0 {
                    "yes"
                } else {
                    "NO <<< FAIL, Task 9's owner-draw row never paints"
                },
                if st & 0x0100 == 0 {
                    "absent"
                } else {
                    "PRESENT <<< FAIL, the index contract is broken"
                },
            );
            println!(
                "      CB_GETCOUNT == {KEY_COUNT}: {}",
                if count == KEY_COUNT {
                    "yes"
                } else {
                    "NO <<< FAIL"
                }
            );
            for (i, want) in KEY_ORDER {
                match combo_item(ctl, i) {
                    Some(got) if got == want => println!("      item {i:>3}: {got:?} ok"),
                    Some(got) => {
                        println!("      item {i:>3}: {got:?} <<< FAIL, expected {want:?}")
                    }
                    None => println!("      item {i:>3}: CB_ERR <<< FAIL, expected {want:?}"),
                }
            }
        } else {
            println!("    COMBOBOX IDC_COMBO:   MISSING");
        }

        // The other `CBS_DROPDOWNLIST | CBS_OWNERDRAWFIXED` list Task 9
        // built. Same three style claims as `IDC_COMBO` above, for the same
        // reasons -- a `CBS_SORT` here would move `Esc` ahead of `Caps Lock`
        // and `paint::draw_combo_item`'s `cap::TAP_ITEMS[di.itemID]` lookup
        // would silently draw the wrong caption for the row's own
        // selection -- but only three fixed items, not 81, so they are
        // checked inline rather than through `KEY_ORDER`.
        if let Some(ctl) = dlg_item(parent, IDC_TAP) {
            let (_, _, w, h) = box_in_client(parent, ctl);
            let st = unsafe { GetWindowLongPtrW(ctl, GWL_STYLE) } as u32;
            let count = send(ctl, CB_GETCOUNT, 0, 0);
            println!(
                "    COMBOBOX IDC_TAP:     {} closed   CB_GETCOUNT {count}   style 0x{st:08X}",
                fmt_wh(w, h)
            );
            println!(
                "      CBS_DROPDOWNLIST: {}   CBS_OWNERDRAWFIXED: {}   CBS_SORT: {}",
                if st & 0x3 == 3 { "yes" } else { "NO <<< FAIL" },
                if st & 0x0010 != 0 {
                    "yes"
                } else {
                    "NO <<< FAIL, Task 9's owner-draw row never paints"
                },
                if st & 0x0100 == 0 {
                    "absent"
                } else {
                    "PRESENT <<< FAIL, TAP_ITEMS order is broken"
                },
            );
            // Transcribed from `cap::TAP_ITEMS` -- the probe cannot link the
            // crate, so this is an independent copy, exactly like
            // `KEY_ORDER` above.
            const TAP_COUNT: isize = 3;
            const TAP_ITEMS: [&str; 3] = ["Caps Lock", "Esc", "Nothing"];
            println!(
                "      CB_GETCOUNT == {TAP_COUNT}: {}",
                if count == TAP_COUNT {
                    "yes"
                } else {
                    "NO <<< FAIL"
                }
            );
            for (i, want) in TAP_ITEMS.iter().enumerate() {
                let i = i as isize;
                match combo_item(ctl, i) {
                    Some(got) if got == *want => println!("      item {i:>3}: {got:?} ok"),
                    Some(got) => {
                        println!("      item {i:>3}: {got:?} <<< FAIL, expected {want:?}")
                    }
                    None => println!("      item {i:>3}: CB_ERR <<< FAIL, expected {want:?}"),
                }
            }
        } else {
            println!("    COMBOBOX IDC_TAP:     MISSING");
        }

        // `IDC_CAPS` -- the one toggle switch in the window -- is
        // deliberately UNCHANGED since before this redesign: it stays a
        // real `BS_AUTOCHECKBOX`, painted through `NM_CUSTOMDRAW`, so it
        // keeps both the check-box state machine and the UIA checkbox role
        // that `BS_OWNERDRAW` would drop. Seven OTHER controls in this
        // window traded that role away on purpose (the four modifier chips,
        // the three `Hold` chips, and neither `IDC_COMBO` nor `IDC_TAP`
        // count here since a `CBS_DROPDOWNLIST` was never a check box to
        // begin with) -- so this assertion exists to catch a future edit
        // that "simplifies" `IDC_CAPS` the same way, here, at a style-bit
        // read, rather than in a screen reader. `BS_TYPEMASK` is the low
        // nibble (0x0F); `BS_AUTOCHECKBOX` is 0x03.
        if let Some(ctl) = dlg_item(parent, IDC_CAPS) {
            let st = unsafe { GetWindowLongPtrW(ctl, GWL_STYLE) } as u32;
            println!(
                "    BUTTON   IDC_CAPS:     style 0x{st:08X}   BS_AUTOCHECKBOX: {}",
                if st & 0x0F == 0x03 {
                    "yes"
                } else {
                    "NO <<< FAIL, the UIA checkbox role would be gone"
                },
            );
        } else {
            println!("    BUTTON   IDC_CAPS:     MISSING");
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

    /// The tab strip, and the frame metrics the strip's own inset is measured
    /// against. Two gates read this block and they want different halves of
    /// it.
    ///
    /// **G-S2 wants the style bits, and it wants them for a reason a re-read
    /// of `WS_TABSTOP` alone cannot serve.** user32 migrates that bit onto
    /// whichever radio in a group is checked, so a group is ONE tab stop --
    /// measured on a14 2026-08-14 with `examples/pill_probe.rs`, and the FIRST
    /// run of that gate proved nothing because it read the styles once, on a
    /// pair it had created with different styles to begin with. This section
    /// prints the checked state on the same line as the bits so the two can
    /// be read against each other, and it prints all four pills so a second
    /// run after clicking a different door is a comparison rather than a
    /// fresh assertion. **One reading here is still not evidence of
    /// migration** -- run it, switch doors, run it again, and read the CHANGE.
    ///
    /// **G-S5 wants the frame metrics by name.** `chrome::nchittest` resolves
    /// all eight resize directions itself out of `SM_CYSIZEFRAME +
    /// SM_CXPADDEDBORDER`, and `strip_rect` insets the trough by `tok::PAD`
    /// so a pill cannot cover the left or right resize edge across the
    /// strip's whole band. That margin is 2-3 px by arithmetic and has never
    /// been read off a machine, which is what makes printing the metrics
    /// worth a section rather than a comment.
    ///
    /// **There is no `SM_CYPADDEDBORDER`.** `windows` 0.61.3 defines index 92
    /// as `SM_CXPADDEDBORDER` and nothing else, and `chrome::nchittest`
    /// spends that one X metric on both axes -- so the pair printed below is
    /// what the window itself computes, not a convenient substitute for it.
    fn measure_strip(parent: HWND) {
        println!("  -- tab strip (G-S2) and frame metrics (G-S5) --");
        let dpi = unsafe { GetDpiForWindow(parent) };

        let mut leftmost: Option<i32> = None;
        let mut prev_right: Option<i32> = None;
        for (id, caption) in TAB_PILLS {
            let Some(ctl) = dlg_item(parent, id) else {
                println!("    pill {id} {caption:<9}: MISSING <<< FAIL");
                continue;
            };
            let (x, y, w, h) = box_in_client(parent, ctl);
            let st = unsafe { GetWindowLongPtrW(ctl, GWL_STYLE) } as u32;
            // `BM_GETCHECK` on an auto-radio was gate G-S3, and it passed on
            // a14 2026-08-14: 1 for the checked radio, 0 for its sibling. It
            // is a bare integer message, so unlike the comctl32 messages
            // `Remote` exists for, it crosses the process boundary as it is.
            // This is also what `is_checked` reads, and what `paint::tab_pill`
            // takes selected-ness from instead of `CDIS_CHECKED` -- so a
            // reading here that disagrees with the lit pill on screen is the
            // painter's bug, not this probe's.
            let checked = send(ctl, BM_GETCHECK, 0, 0);
            println!(
                "    pill {id} {caption:<9}: {}   checked={checked}   style 0x{st:08X}",
                fmt_box(x, y, w, h)
            );
            println!(
                "        BS_AUTORADIOBUTTON: {}   BS_PUSHLIKE: {}   WS_TABSTOP: {}   \
                 WS_GROUP: {}",
                // `BS_TYPEMASK` is the low nibble; `BS_AUTORADIOBUTTON` is 9.
                // A 0 here means something ran `set_button_type` over a pill,
                // which is exactly why they are absent from `PUSH_BUTTONS`.
                if st & 0x0F == 0x09 {
                    "yes"
                } else {
                    "NO <<< FAIL, a pill was rewritten into a push button"
                },
                if st & 0x1000 != 0 {
                    "yes"
                } else {
                    "NO <<< FAIL"
                },
                // Neither of these two is a pass/fail. `WS_TABSTOP` is
                // user32's to move and `WS_GROUP` is set on the control AFTER
                // the last pill, never on a pill -- so both are printed as
                // facts to compare across runs.
                if st & 0x00010000 != 0 { "yes" } else { "no" },
                if st & 0x00020000 != 0 { "yes" } else { "no" },
            );
            if x != RECT_FAIL {
                leftmost = Some(leftmost.map_or(x, |l: i32| l.min(x)));
                if let Some(r) = prev_right {
                    if x < r {
                        println!(
                            "        <<< FAIL: this pill starts at {x}, left of the previous \
                             pill's right edge {r} -- the run is out of order or overlapping"
                        );
                    }
                }
                prev_right = Some(x + w);
            }
        }

        let (cxframe, cyframe, padded, vscroll) = unsafe {
            (
                GetSystemMetricsForDpi(SM_CXSIZEFRAME, dpi),
                GetSystemMetricsForDpi(SM_CYSIZEFRAME, dpi),
                GetSystemMetricsForDpi(SM_CXPADDEDBORDER, dpi),
                GetSystemMetricsForDpi(SM_CXVSCROLL, dpi),
            )
        };
        println!(
            "    GetSystemMetricsForDpi @{dpi}:  SM_CXSIZEFRAME {cxframe}   SM_CYSIZEFRAME \
             {cyframe}   SM_CXPADDEDBORDER {padded}   SM_CXVSCROLL {vscroll}"
        );
        // The pair `chrome::nchittest` actually spends -- the Y size-frame
        // metric on BOTH axes, since there is no `SM_CYPADDEDBORDER` to pair
        // an X one with. `SM_CXSIZEFRAME` is printed beside it only so a
        // machine where the two size-frame metrics disagree says so here
        // rather than in a resize that misses by a pixel.
        let border = cyframe + padded;
        match leftmost {
            Some(x) if x > border => println!(
                "    resize border {border}px vs leftmost pill at x={x}: clear by {}px -- \
                 HTLEFT survives across the strip band",
                x - border
            ),
            Some(x) => println!(
                "    resize border {border}px vs leftmost pill at x={x}: <<< FAIL, the pill \
                 covers the left resize edge and the band cannot be dragged"
            ),
            None => println!("    resize border {border}px: no pill rect to compare it against"),
        }
    }

    /// The System page (design §3.3), read back control by control.
    ///
    /// **Nothing on the machine this window is written on can display it**,
    /// so every figure on that page is a hand trace until this runs. It reads
    /// the fourteen ids by number -- transcribed, like every id in this file,
    /// because the probe drives another process and cannot link the crate --
    /// and prints what only hardware can answer: whether each control exists,
    /// where it landed, whether a row that should be OMITTED is on screen
    /// anyway, and what the transparency slot actually says.
    ///
    /// **Run it with the System door open.** Everything here is behind it, so
    /// on any other page every control reads `hidden` and the section says
    /// nothing -- which is itself the check that `show_page_controls` covers
    /// the new rows.
    ///
    /// Three things it can catch that no test on the build host can:
    ///
    /// - **A row that is omitted in the model but shown on screen.** The two
    ///   conditional rows (`Start with Windows`, and the log row's four) are
    ///   hidden by `sys_row_shown` through a thread-local the tests cannot
    ///   reach. A visible `Start with Windows` under `beckon.exe serve` is a
    ///   switch that writes a Run value which opens a console at every logon.
    /// - **The slider's range**, which `build_children` sets once and
    ///   `paint::slider_part` reads back to decide how much of the channel is
    ///   filled. A range of 0..=100 with a position of 96 would draw a
    ///   plausible bar at the wrong place for ever.
    /// - **The value slots' text**, which is the only place the forced-off
    ///   reason appears at all. A blank slot with a greyed slider is design
    ///   §7 rule 7 broken exactly as it warns.
    fn measure_system(parent: HWND) {
        println!("  -- System page (design 3.3) --");
        // Transcribed from `beckon_core::settings::CONTROL_IDS`' System block
        // (1070-1083). `SYS_PLACEHOLDER` (1084) is RETIRED and deliberately
        // absent: a probe that still looked for it would report a missing
        // control on every healthy build.
        const ROWS: [(i32, &str); 14] = [
            (1070, "PAUSE"),
            (1071, "AUTOSTART"),
            (1072, "SYS_RELOAD"),
            (1073, "DARK"),
            (1074, "OPACITY"),
            (1075, "OPACITY_VALUE"),
            (1076, "CONFIG_NAME"),
            (1077, "CONFIG_DIR"),
            (1078, "CONFIG_OPEN"),
            (1079, "CONFIG_SHOW"),
            (1080, "LOG_NAME"),
            (1081, "LOG_SIZE"),
            (1082, "LOG_OPEN"),
            (1083, "LOG_SHOW"),
        ];
        // `WM_USER + 0` and `TBM_GETRANGEMIN` / `TBM_GETRANGEMAX`. Bare
        // integer messages, so they cross the process boundary as they are --
        // unlike anything that would return a pointer.
        const TBM_GETPOS: u32 = 0x0400;
        const TBM_GETRANGEMIN: u32 = 0x0400 + 1;
        const TBM_GETRANGEMAX: u32 = 0x0400 + 2;
        // A `BS_AUTOCHECKBOX` answers `BM_GETCHECK` whatever paints it, which
        // is the whole reason the three switches stayed check boxes.
        const SWITCHES: [i32; 3] = [1070, 1071, 1073];

        let mut any_visible = false;
        for (id, name) in ROWS {
            let Some(ctl) = dlg_item(parent, id) else {
                println!("    {id} {name:<14}: MISSING <<< FAIL");
                continue;
            };
            let vis = unsafe { IsWindowVisible(ctl) }.as_bool();
            any_visible |= vis;
            let (x, y, w, h) = box_in_client(parent, ctl);
            let en = unsafe { IsWindowEnabled(ctl) }.as_bool();
            let text = ctl_text(ctl);
            let extra = if SWITCHES.contains(&id) {
                format!("   checked={}", send(ctl, BM_GETCHECK, 0, 0))
            } else if id == 1074 {
                format!(
                    "   pos={} range={}..={}",
                    send(ctl, TBM_GETPOS, 0, 0),
                    send(ctl, TBM_GETRANGEMIN, 0, 0),
                    send(ctl, TBM_GETRANGEMAX, 0, 0),
                )
            } else {
                String::new()
            };
            println!(
                "    {id} {name:<14}: {}   visible={vis} enabled={en}{extra}   text={text:?}",
                fmt_box(x, y, w, h)
            );
        }
        if !any_visible {
            println!(
                "    (every control is hidden -- this run is not on the System door, so \
                 nothing above is a verdict)"
            );
            return;
        }
        // The two conditional rows, stated as a verdict rather than left for
        // the reader to infer from four `visible=` flags. Neither is a FAIL
        // on its own: which way each should go depends on how this
        // `beckon-serve` was started, and only a person knows that.
        let shown = |id: i32| {
            dlg_item(parent, id)
                .map(|c| unsafe { IsWindowVisible(c) }.as_bool())
                .unwrap_or(false)
        };
        println!(
            "    conditional rows: AUTOSTART {}   log row {}",
            if shown(1071) {
                "shown (expect this only under beckon-serve.exe)"
            } else {
                "omitted (expect this under `beckon.exe serve`)"
            },
            if shown(1080) && shown(1081) && shown(1082) && shown(1083) {
                "shown (expect this only with --log)"
            } else if !shown(1080) && !shown(1081) && !shown(1082) && !shown(1083) {
                "omitted (expect this without --log)"
            } else {
                "HALF SHOWN <<< FAIL, sys_row_shown disagrees with itself"
            }
        );
        // The transparency slot, which is where rule 7 either holds or does
        // not. A disabled slider whose slot carries no reason is the failure
        // design 7 names by name -- and a tooltip cannot rescue it, because a
        // disabled Win32 control receives no mouse messages.
        let live = dlg_item(parent, 1074)
            .map(|c| unsafe { IsWindowEnabled(c) }.as_bool())
            .unwrap_or(false);
        let slot = dlg_item(parent, 1075).map(ctl_text).unwrap_or_default();
        if live {
            if slot.contains('%') {
                println!("    transparency: live, slot {slot:?}");
            } else {
                println!("    transparency: live but the slot shows no percentage <<< FAIL");
            }
        } else if slot.len() > "Window transparency".len() {
            println!("    transparency: forced off, slot {slot:?}");
        } else {
            println!(
                "    transparency: forced off and the slot says nothing beyond its label \
                 ({slot:?}) <<< FAIL, design 7 rule 7"
            );
        }
    }

    /// The About page (design §3.4), read back control by control.
    ///
    /// `measure_system`'s shape and its rules: transcribed ids, run it with
    /// the About door open, and everything hidden means the section is not a
    /// verdict.
    ///
    /// Four things it can catch that nothing on the build host can:
    ///
    /// - **The copy glyph.** `U+29C9 TWO JOINED SQUARES` is the third
    ///   non-ASCII string this window draws and the least certain of the
    ///   three -- the other two were argued to be in Segoe UI's own coverage,
    ///   while this is a mathematical symbol that may only arrive through font
    ///   linking. Reading the caption back does NOT prove it rendered (a font
    ///   that lacks it still reports the character and draws a box), but a
    ///   caption that comes back as `?` proves it did not survive the trip at
    ///   all, which is the failure that costs three buttons their meaning.
    /// - **The `Location` row's text**, which is the whole point of the page:
    ///   it must be the launch path with `\current\` still in it, NOT a
    ///   resolved version directory. A path containing a version number is
    ///   the tell that something started resolving it, which is exactly the
    ///   surface that lied on a14.
    /// - **Whether the verdict fires at all after an update.** Since
    ///   2026-08-15 the verdict has two producers and only one of them can
    ///   see a moved scoop junction (`beckon_core::settings::image_age`, and
    ///   its doc measures why the clock half cannot). The identity half rests
    ///   on `QueryFullProcessImageNameW` returning the RESOLVED image path
    ///   for a junction launch, which is read from documentation and has
    ///   never been run. **The run that settles it is: `scoop update beckon`
    ///   with an old `beckon-serve.exe` still running, then open About.** A
    ///   verdict means the documented reading holds; silence means it returns
    ///   the launch path instead, which is the reading `about_now` is built
    ///   to survive -- not a regression, but the answer, and worth writing
    ///   down either way.
    /// - **The name row's version**, which is the running IMAGE's. Compare it
    ///   against `beckon --version` typed at a shell: they are allowed to
    ///   disagree, and when they do, THIS one is the truth and an update is
    ///   waiting for a restart.
    /// - **The disclosure**, whose second sentence is a promise. A control
    ///   whose text comes back truncated is a promise half-made.
    fn measure_about(parent: HWND) {
        println!("  -- About page (design 3.4) --");
        // Transcribed from `beckon_core::settings::CONTROL_IDS`' About block
        // (1100-1114). `ABOUT_PLACEHOLDER` (1115) is RETIRED and deliberately
        // absent, exactly like `SYS_PLACEHOLDER` in `measure_system`.
        const ROWS: [(i32, &str); 15] = [
            (1100, "MARK"),
            (1101, "NAME"),
            (1102, "BUILD_LABEL"),
            (1103, "BUILD_VALUE"),
            (1104, "BUILD_COPY"),
            (1105, "LOCATION_LABEL"),
            (1106, "LOCATION_VALUE"),
            (1107, "LOCATION_COPY"),
            (1108, "LICENCE_LABEL"),
            (1109, "LICENCE_VALUE"),
            (1110, "LICENCE_COPY"),
            (1111, "DISCLOSURE"),
            (1112, "GITHUB"),
            (1113, "RELEASES"),
            (1114, "BUG"),
        ];
        let mut any_visible = false;
        for (id, name) in ROWS {
            let Some(ctl) = dlg_item(parent, id) else {
                println!("    {id} {name:<15}: MISSING <<< FAIL");
                continue;
            };
            let vis = unsafe { IsWindowVisible(ctl) }.as_bool();
            any_visible |= vis;
            let (x, y, w, h) = box_in_client(parent, ctl);
            let text = ctl_text(ctl);
            println!(
                "    {id} {name:<15}: {}   visible={vis}   text={text:?}",
                fmt_box(x, y, w, h)
            );
        }
        if !any_visible {
            println!(
                "    (every control is hidden -- this run is not on the About door, so \
                 nothing above is a verdict)"
            );
            return;
        }
        let text = |id: i32| dlg_item(parent, id).map(ctl_text).unwrap_or_default();
        // The three copy glyphs, as ONE verdict: they carry one caption, so
        // three separate lines would be three readings of the same fact.
        let glyphs: Vec<String> = [1104, 1107, 1110].iter().map(|id| text(*id)).collect();
        if glyphs.iter().all(|g| g == "\u{29C9}") {
            println!("    copy glyph: all three carry U+29C9 (a box on screen is still possible)");
        } else {
            println!("    copy glyph: <<< FAIL, captions came back {glyphs:?}");
        }
        // The row this page exists for. `\current\` is scoop's junction; a
        // machine that installed some other way has no such component, so its
        // absence is not a failure -- what IS reported is the raw string, so
        // a reader can see for themselves whether anything resolved it.
        let loc = text(1106);
        println!("    location: {loc:?}");
        if loc.contains("(updated on disk") || loc.contains("(no longer on disk") {
            println!("    location verdict: PRESENT -- the running image is not the file on disk");
        } else {
            println!(
                "    location verdict: silent (Current or Unknown -- see `image_age`). \
                 On a run made straight after `scoop update` with the OLD serve still \
                 alive, this line is the answer to whether QueryFullProcessImageNameW \
                 resolves a junction: silence there means it does not."
            );
        }
        // The disclosure's two halves, both of which design 3.4 requires. The
        // second is the negative claim, which nothing but the words can make.
        let disc = text(1111);
        let both = disc.contains("recording a shortcut") && disc.contains("keeps no record");
        println!(
            "    disclosure: {} ({} chars)",
            if both {
                "both halves present"
            } else {
                "<<< FAIL, a half is missing"
            },
            disc.chars().count()
        );
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
    /// `WM_SETTEXT` test would pass on a plain EDIT and fail on the App
    /// field for a reason that has nothing to do with beckon.
    ///
    /// `witness` is `(list, row, subitem)`: the list cell this field feeds.
    /// After every character the cell is read back and compared with the
    /// field. **That comparison is the only thing that can settle the App
    /// combo box defect.** The control rewriting its own text is not itself
    /// a bug — comctl32 is entitled to autocomplete — the bug is beckon
    /// recording something other than what the field ends up showing. Both
    /// sides are printed, per keystroke.
    ///
    /// Returns `(wrong, late)`: cells that never caught up, and cells that
    /// caught up only on the re-read. **The split exists because the
    /// control cannot catch a too-short wait on this path.** The shortcut
    /// half's reads are synchronous, so `drive_the_shortcut` passes at any
    /// sleep — only the App half is timing-sensitive, and a sleep that
    /// is simply too short therefore reads exactly like the defect. So every
    /// disagreement is re-read once after a further 300 ms and BOTH readings
    /// are printed: a cell that changes between them is the probe being
    /// impatient, and one that does not is the defect. Without this the two
    /// are indistinguishable in the output, which is how a broken detector
    /// gets mistaken for a finding.
    fn type_into(h: HWND, s: &str, witness: Option<(HWND, i32, i32)>) -> (usize, usize) {
        let mut empty: Vec<u16> = vec![0];
        let mut wrong = 0usize;
        let mut late = 0usize;
        unsafe {
            SendMessageW(
                h,
                WM_SETTEXT,
                Some(WPARAM(0)),
                Some(LPARAM(empty.as_mut_ptr() as isize)),
            );
            for ch in s.encode_utf16() {
                // WHEN the text changes, not just what it ends up as.
                //
                // `SendMessageW` returns only after the target has finished
                // handling the character, so anything visible at the FIRST
                // sample happened inside that handling -- comctl32, or a
                // notification beckon answered synchronously. Anything that
                // appears at a later sample happened on the message loop
                // afterwards, which is where the posted read and
                // `apply_state` run. The two are indistinguishable from a
                // single reading taken 120 ms later, which is all this probe
                // used to take.
                let t0 = std::time::Instant::now();
                let tracing = APP_COMBO.with(|c| *c.borrow()) != 0;
                let mut trail: Vec<(u128, String)> = Vec::new();
                if tracing {
                    // POSTED, not sent, and only on the traced field.
                    //
                    // `SendMessageW` does not return until the character has
                    // been fully handled, so the earliest possible reading is
                    // already after everything comctl32 did -- which makes an
                    // atomic rewrite and a two-step rewrite look identical.
                    // Posting hands the character to the target's own message
                    // loop and returns at once, so the polling below can see
                    // the intermediate state if there is one. If the field
                    // never reads as the bare character, there is no
                    // intermediate state to catch.
                    let _ = PostMessageW(Some(h), WM_CHAR, WPARAM(ch as usize), LPARAM(1));
                    let mut last = String::from("\u{0}unset");
                    for _ in 0..600 {
                        let now = ctl_text(h);
                        if now != last {
                            trail.push((t0.elapsed().as_micros(), now.clone()));
                            last = now;
                        }
                    }
                } else {
                    SendMessageW(h, WM_CHAR, Some(WPARAM(ch as usize)), Some(LPARAM(1)));
                }
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
                            // The one extra wait, and only where it can
                            // change the reading.
                            std::thread::sleep(Duration::from_millis(300));
                            let field2 = ctl_text(h);
                            match list_cell(list, row, sub) {
                                Some(c2) if cell_agrees(&c2, &field2) => {
                                    late += 1;
                                    format!(
                                        "list {cell:?} <<< disagreed, then {c2:?} AGREED \
                                         after +300ms -- SLOW, not wrong: the 120ms wait \
                                         is too short on this machine"
                                    )
                                }
                                Some(c2) => {
                                    wrong += 1;
                                    format!(
                                        "list {cell:?} <<< DISAGREES with the field, and \
                                         still {c2:?} vs field {field2:?} after +300ms -- \
                                         STILL WRONG, not slow"
                                    )
                                }
                                None => {
                                    wrong += 1;
                                    format!(
                                        "list {cell:?} <<< DISAGREES with the field; \
                                         re-read UNREADABLE"
                                    )
                                }
                            }
                        }
                        None => "list UNREADABLE".to_string(),
                    },
                    None => "(no witness)".to_string(),
                };
                println!(
                    "      typed {:?} -> field {:?}   {verdict}{}",
                    char::from_u32(ch as u32).unwrap_or('?'),
                    field,
                    combo_detail(h),
                );
                if APP_COMBO.with(|c| *c.borrow()) != 0 {
                    let t: Vec<String> = trail
                        .iter()
                        .map(|(us, s)| format!("+{}us {s:?}", us))
                        .collect();
                    println!("        transitions: {}", t.join("  ->  "));
                }
            }
        }
        std::thread::sleep(Duration::from_millis(300));
        (wrong, late)
    }

    /// `VK_DOWN` / `VK_RETURN`, spelled as the numbers winuser.h gives them.
    /// The `windows` crate types these as `VIRTUAL_KEY`, which is not what a
    /// `WM_KEYDOWN` wParam is.
    const VK_DOWN_CODE: usize = 0x28;
    const VK_RETURN_CODE: usize = 0x0D;

    /// Drive the App combo box's dropdown from the KEYBOARD and report what
    /// the model recorded.
    ///
    /// **A different notification sequence from typing, which is why it is a
    /// separate step.** An arrow key inside a dropped-down list raises
    /// `CBN_SELCHANGE`; Enter then raises `CBN_SELENDOK` and `CBN_CLOSEUP`,
    /// and the `CBN_CLOSEUP` arm commits the field SYNCHRONOUSLY. So this is
    /// the path where a deferred `CBN_SELCHANGE` read gets discarded by its
    /// own backstop, and no amount of typing exercises it.
    ///
    /// **It reports; it does not assert.** Driving a dropdown with posted
    /// keys across a process boundary is not guaranteed to reach the list at
    /// all, so the first thing printed is whether the field changed — a
    /// probe that never drove the control and a control that behaved
    /// correctly must not look the same, which is the trap this whole file
    /// keeps falling into.
    fn pick_from_dropdown(h: HWND, witness: Option<(HWND, i32, i32)>) {
        println!("  -- picking from the dropdown with the keyboard --");
        let Some(combo) = dlg_item(h, IDC_APP) else {
            println!("    SKIP: no App combo box");
            return;
        };
        let count = send(combo, CB_GETCOUNT, 0, 0);
        if count <= 0 {
            println!("    SKIP: the catalog is empty, so there is nothing to pick ({count} items)");
            return;
        }
        let before = ctl_text(combo);
        unsafe {
            SendMessageW(combo, CB_SHOWDROPDOWN, Some(WPARAM(1)), Some(LPARAM(0)));
        }
        std::thread::sleep(Duration::from_millis(200));
        // To the combo itself: comctl32 routes the list's key handling
        // through the combo's own wndproc, and the probe cannot take focus
        // in another process's thread without attaching to its input queue.
        for _ in 0..2 {
            send(combo, WM_KEYDOWN, VK_DOWN_CODE, 0);
            send(combo, WM_KEYUP, VK_DOWN_CODE, 0);
            std::thread::sleep(Duration::from_millis(150));
        }
        let after_arrows = ctl_text(combo);
        send(combo, WM_KEYDOWN, VK_RETURN_CODE, 0);
        send(combo, WM_KEYUP, VK_RETURN_CODE, 0);
        // Generous: the CBN_CLOSEUP commit and the push it triggers are both
        // synchronous, but the window still has to repaint the list.
        std::thread::sleep(Duration::from_millis(500));
        let after_enter = ctl_text(combo);
        println!("    field: before {before:?} -> after arrows {after_arrows:?} -> after Enter {after_enter:?}");
        if after_arrows == before && after_enter == before {
            println!(
                "    INCONCLUSIVE: the field never moved, so the posted keys did not \
                 reach the list. This is a probe limitation, NOT a beckon result -- \
                 a human must pick an entry by hand."
            );
            return;
        }
        match witness {
            Some((list, row, sub)) => match list_cell(list, row, sub) {
                Some(cell) if cell_agrees(&cell, &after_enter) => {
                    println!("    PASS: list {cell:?} carries the picked value");
                }
                Some(cell) => {
                    println!(
                        "    FAIL: list {cell:?} but the field shows {after_enter:?} -- \
                         the pick was recorded wrong, or CBN_CLOSEUP undid it"
                    );
                }
                None => println!("    list UNREADABLE"),
            },
            None => println!("    (no witness row; the field reading above is all there is)"),
        }
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
        // Reconstructed from the five controls, because there is no longer a
        // field to read. `""` means the key list has nothing selected, which
        // is a legitimate state and not a failed read.
        let shortcut = shortcut_shown(h);
        let caps = shortcut_caps(h);
        let appfld = dlg_item(h, IDC_APP).map(ctl_text).unwrap_or_default();
        let apply = dlg_item(h, IDC_APPLY)
            .map(|a| unsafe { IsWindowEnabled(a) }.as_bool())
            .unwrap_or(false);
        // Both spellings: `shortcut` is what the model carries and the file
        // would get, `caps` is what the Shortcut cell must say. They differ
        // on purpose -- see `shortcut_caps`.
        println!("    [{label}] apply={apply} shortcut={shortcut:?} caps={caps:?} app={appfld:?}");
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
            "    enabled: Add={} List={} Caps={} Save={} \
             ModCtrl={} ModWin={} ModAlt={} ModShift={} KeyList={} \
             | escape routes: OpenFile={} Close={}",
            on(IDC_ADD),
            on(IDC_LIST),
            on(IDC_CAPS),
            on(IDC_APPLY),
            on(IDC_MOD_CTRL),
            on(IDC_MOD_WIN),
            on(IDC_MOD_ALT),
            on(IDC_MOD_SHIFT),
            on(IDC_COMBO),
            on(IDC_OPENFILE),
            on(IDC_CLOSE),
        );
        if read_only {
            println!("    READ ONLY -- the file did not parse. The contract is:");
            println!(
                "      every mutating control off, both escape routes on, \
                 and the notes say why"
            );
            // Five more mutating controls since the shortcut field became
            // four check boxes and a key list -- all gated on `st.editable`
            // exactly like Add/List/Caps/Save, so a read-only window that
            // left any of them on is the same defect this block exists to
            // catch. Naming only the original four here would print a
            // stronger claim ("every mutating control off") than this
            // block actually checks.
            let bad = [
                ("Add", on(IDC_ADD)),
                ("List", on(IDC_LIST)),
                ("Caps", on(IDC_CAPS)),
                ("Save", on(IDC_APPLY)),
                ("ModCtrl", on(IDC_MOD_CTRL)),
                ("ModWin", on(IDC_MOD_WIN)),
                ("ModAlt", on(IDC_MOD_ALT)),
                ("ModShift", on(IDC_MOD_SHIFT)),
                ("KeyList", on(IDC_COMBO)),
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

    /// Click a chip the way a mouse does.
    ///
    /// `BM_CLICK` reaches the control's OWN wndproc, which is what makes the
    /// whole chain real: the button turns it into `BN_CLICKED`, the window's
    /// `handle_command` calls `toggle_chip` and only then reads every chip
    /// back. Posting a `WM_COMMAND` at the parent instead, the way `click`
    /// does for a push button, would skip the button entirely -- and now
    /// that these are `BS_OWNERDRAW` that is a bigger lie than it was: the
    /// window would still toggle, so the difference is no longer visible in
    /// the result, only in what was actually tested.
    fn tick(parent: HWND, id: i32) {
        let Some(ctl) = dlg_item(parent, id) else {
            println!("    (no control {id})");
            return;
        };
        send(ctl, BM_CLICK, 0, 0);
        std::thread::sleep(Duration::from_millis(250));
    }

    fn cell_now(witness: Option<(HWND, i32, i32)>) -> Option<String> {
        let (list, row, sub) = witness?;
        list_cell(list, row, sub)
    }

    /// Does the witness cell say `want`?
    ///
    /// With `type_into`'s one re-read, and for its reason: a cell that
    /// changes between the two readings is the probe being impatient, and
    /// one that does not is the defect. Without the split the two are
    /// indistinguishable in the output, which is how a broken detector gets
    /// mistaken for a finding.
    fn expect_cell(
        witness: Option<(HWND, i32, i32)>,
        step: &str,
        want: Option<&str>,
    ) -> (usize, usize) {
        let Some(want) = want else {
            println!("    {step}: no witness cell to check against");
            return (0, 0);
        };
        match cell_now(witness) {
            Some(cell) if cell_agrees(&cell, want) => {
                println!("    {step}: list {cell:?} MATCH");
                (0, 0)
            }
            Some(cell) => {
                std::thread::sleep(Duration::from_millis(300));
                match cell_now(witness) {
                    Some(c2) if cell_agrees(&c2, want) => {
                        println!(
                            "    {step}: list {cell:?} <<< disagreed with {want:?}, then \
                             {c2:?} AGREED after +300ms -- SLOW, not wrong"
                        );
                        (0, 1)
                    }
                    Some(c2) => {
                        println!(
                            "    {step}: list {cell:?} <<< DISAGREES with {want:?}, and \
                             still {c2:?} after +300ms -- STILL WRONG, not slow"
                        );
                        (1, 0)
                    }
                    None => {
                        println!("    {step}: list {cell:?} vs {want:?}; re-read UNREADABLE");
                        (1, 0)
                    }
                }
            }
            None => {
                println!("    {step}: list UNREADABLE");
                (0, 0)
            }
        }
    }

    /// The controls' own reading is the expectation: whatever they show, the
    /// model must carry.
    ///
    /// Read through `shortcut_caps`, because the witness is the **Shortcut
    /// cell** and that cell is the display spelling. Both spellings are
    /// printed: they are the input and the output of the mapping under test,
    /// and a run where they disagree in some *third* way is worth being able
    /// to see.
    fn expect_shown(h: HWND, witness: Option<(HWND, i32, i32)>, step: &str) -> (usize, usize) {
        let caps = shortcut_caps(h);
        println!(
            "    controls now show {caps:?} (config spelling: {:?})",
            shortcut_shown(h)
        );
        expect_cell(witness, step, Some(&caps))
    }

    /// Drive the five shortcut controls and check the model followed.
    ///
    /// **This is the CONTROL for the App half**, the role typing
    /// `ctrl+super+alt+j` into an EDIT used to have. Neither a check box nor
    /// a `CBS_DROPDOWNLIST` rewrites itself, and every read here is
    /// synchronous, so this half must agree at every step -- if it disagrees
    /// too, the probe's own timing is wrong and the App result means
    /// nothing.
    ///
    /// Returns `(wrong, late)` in `type_into`'s vocabulary, so the summary
    /// line reads the same either side of this change.
    ///
    /// Step 0 is the one that cannot be phrased as "the controls agree with
    /// the cell", and is the more interesting for it: with no key selected,
    /// ticking a modifier must send NOTHING, because `ctrl+` alone is not a
    /// combo and would flag the row for a mistake the user is halfway
    /// through not making. So its expectation is the cell's PREVIOUS value,
    /// not the controls' current one.
    fn drive_the_shortcut(h: HWND, witness: Option<(HWND, i32, i32)>) -> (usize, usize) {
        println!("  -- driving the shortcut: four check boxes and the key list --");
        let Some(combo) = dlg_item(h, IDC_COMBO) else {
            println!("    FAIL: no key list");
            return (usize::MAX, 0);
        };
        let mut wrong = 0usize;
        let mut late = 0usize;

        // Step 0: a modifier with no key selected must change nothing.
        match key_sel(h) {
            Some((i, name)) => println!(
                "    NOTE: the row already has key {i} = {name:?} selected, so the \
                 'a modifier alone sends nothing' step is skipped -- it needs the \
                 empty row Add makes"
            ),
            None => {
                let was = cell_now(witness);
                tick(h, IDC_MOD_SHIFT);
                let (w, l) = expect_cell(witness, "Shift with no key", was.as_deref());
                wrong += w;
                late += l;
                // Put it back, so the steps below start from the state Add
                // left behind rather than from this one.
                tick(h, IDC_MOD_SHIFT);
            }
        }

        // Step 1: give it a key. VK_DOWN on a CLOSED list is the real path
        // -- comctl32 moves the selection and raises CBN_SELCHANGE, with no
        // dropdown to open and no focus to steal -- so WHICH key it lands on
        // is the control's decision, and the expectation is read back rather
        // than assumed.
        send(combo, WM_KEYDOWN, VK_DOWN_CODE, 0);
        send(combo, WM_KEYUP, VK_DOWN_CODE, 0);
        std::thread::sleep(Duration::from_millis(250));
        if key_sel(h).is_none() {
            // Honest about being a fallback: `CB_SETCURSEL` is documented
            // NOT to notify, so the parent has to be told separately --
            // exactly the synthesis `click` performs for a push button, and
            // just as unable to prove comctl32 would have sent it.
            println!(
                "    NOTE: VK_DOWN did not move the selection. Falling back to \
                 CB_SETCURSEL plus a SYNTHESISED CBN_SELCHANGE -- the window's \
                 handling is still under test, comctl32's is not."
            );
            send(combo, CB_SETCURSEL, 0, 0);
            unsafe {
                let _ = PostMessageW(
                    Some(h),
                    WM_COMMAND,
                    WPARAM(((CBN_SELCHANGE as usize) << 16) | (IDC_COMBO as usize & 0xFFFF)),
                    LPARAM(combo.0 as isize),
                );
            }
            std::thread::sleep(Duration::from_millis(400));
        }
        match key_sel(h) {
            Some((i, name)) => println!("    key list: index {i} = {name:?}"),
            None => {
                println!(
                    "    INCONCLUSIVE: nothing ever selected in the key list, so the \
                     steps below could not mean anything. A human must pick a key by \
                     hand. This is a probe limitation, NOT a beckon result."
                );
                return (wrong, late);
            }
        }
        let (w, l) = expect_shown(h, witness, "a key alone");
        wrong += w;
        late += l;

        // Steps 2-5: one modifier at a time, in canonical order, so a
        // disagreement names the chip that caused it. The step is labelled
        // with the CONFIG word (`+super`), which names the chip
        // unambiguously; the cell it is checked against is the display
        // spelling, and `expect_shown` prints both.
        for (id, word, _) in MODIFIERS {
            tick(h, id);
            let (w, l) = expect_shown(h, witness, &format!("+{word}"));
            wrong += w;
            late += l;
        }
        (wrong, late)
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

        // Column 1 is `Shortcut`; column 0 is `App`.
        let (combo_lies, combo_late) = drive_the_shortcut(h, witness(1));
        dump(h, "after the shortcut");

        // The App control is a COMBOBOX; its text lives in a child EDIT, and
        // only that child raises the change notification the window listens
        // for. Setting the combo itself is silent.
        let app_combo = dlg_item(h, IDC_APP);
        // The style the control ACTUALLY has, not the one the source passes
        // to `CreateWindowExW`. `combo_probe` reproduced the documented style
        // and saw no rewrite, so the first thing to check on the live control
        // is whether its style is what the code says.
        if let Some(c) = app_combo {
            let st = unsafe { GetWindowLongPtrW(c, GWL_STYLE) } as u32;
            let kind = match st & 0x3 {
                1 => "CBS_SIMPLE",
                2 => "CBS_DROPDOWN",
                3 => "CBS_DROPDOWNLIST",
                _ => "CBS_?",
            };
            println!(
                "    App combo style: 0x{st:08X} ({kind}{}{})",
                if st & 0x0040 != 0 {
                    " CBS_AUTOHSCROLL"
                } else {
                    ""
                },
                if st & 0x0100 != 0 { " CBS_SORT" } else { "" },
            );
            let mut kid = unsafe { GetWindow(c, GW_CHILD) }.unwrap_or_default();
            while !kid.0.is_null() {
                println!(
                    "      combo child: class {:?} style 0x{:08X}",
                    class_of(kid),
                    unsafe { GetWindowLongPtrW(kid, GWL_STYLE) } as u32
                );
                kid = unsafe { GetWindow(kid, GW_HWNDNEXT) }.unwrap_or_default();
            }
        }
        let app_edit = app_combo.and_then(|c| first_child_of_class(c, "Edit"));
        APP_COMBO.with(|c| *c.borrow_mut() = app_combo.map(|x| x.0 as isize).unwrap_or(0));
        let (app_lies, app_late) = match app_edit {
            Some(e) => type_into(e, "Notepad", witness(0)),
            None => {
                println!("    FAIL: combo box has no edit child");
                (usize::MAX, 0)
            }
        };
        APP_COMBO.with(|c| *c.borrow_mut() = 0);
        dump(h, "after app text");
        println!(
            "    per-step agreement: shortcut controls {} ({combo_lies} wrong, \
             {combo_late} slow), App field {} ({app_lies} wrong, {app_late} slow)",
            if combo_lies == 0 { "PASS" } else { "FAIL" },
            if app_lies == 0 { "PASS" } else { "FAIL" },
        );
        if combo_lies > 0 {
            println!(
                "      the shortcut controls disagreed too -- suspect the probe's own \
                 timing before believing the App result"
            );
        }
        if combo_late + app_late > 0 {
            println!(
                "      NOTE: {} cell(s) agreed only on the +300ms re-read. The model \
                 did converge; the per-step wait is too short on this machine. Raise \
                 it and re-run before reading anything else here.",
                combo_late + app_late
            );
        }

        // AFTER the typing half, because it needs a row to witness against
        // and a populated catalog, and BEFORE Apply, because it changes what
        // gets saved.
        pick_from_dropdown(h, witness(0));
        dump(h, "after dropdown pick");

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
    /// `GetWindowRect`: on a 150 % display a 1020 px window is reported as
    /// 680, silently divided by the scale factor. `LVM_GETITEMRECT` and
    /// `BCM_GETIDEALSIZE` come back through `ReadProcessMemory` instead and
    /// are never virtualized -- so an unaware probe prints logical pixels and
    /// physical pixels side by side, in the same block, unlabelled. That is
    /// how a 29 px row and a 21 px header end up in the same table when they
    /// are really 29 and 31 -- two numbers that look 8 px apart and are 2.
    /// One awareness for the whole probe, so every number below is in
    /// physical pixels.
    ///
    /// **The header half of that example is history too** -- design 3.1
    /// deleted the column headers on 2026-08-15 and `measure_listview` now
    /// checks the band is absent rather than measuring it. The example is kept
    /// because the hazard it illustrates is about mixing coordinate spaces,
    /// not about that particular pair.
    ///
    /// **The 29 is history, not today's row.** It is the row measured while
    /// `tok::ROW_H` was 20 (see the twice-corrected `Ui::shown_empty`
    /// comment in `settings_window::mod`, which quotes it). The pair is kept
    /// because the arithmetic is what this paragraph is for. Since the
    /// 2026-08-13 compaction pass `1f46335` took `tok::ROW_H` to 22, the
    /// state image list floors the live row at `scale(tok::ROW_H, dpi)` --
    /// 33 px at a14's 144 DPI -- and comctl32 may pad above that, so do not
    /// read 29 as a figure to check the output against. The 1020/680 pair
    /// above is current: `WINDOW_WIDTH` is 680 since Task 8 (it was 760, and
    /// this sentence exists because the pair went stale once already).
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
            let (win_w, win_h) = (rc.right - rc.left, rc.bottom - rc.top);
            println!("  rect:    {win_w}x{win_h} at ({}, {})", rc.left, rc.top);
            let dpi = unsafe { GetDpiForWindow(h) };
            let (want_w, want_h) = (
                scale96(WINDOW_WIDTH_96, dpi),
                scale96(WINDOW_HEIGHT_96, dpi),
            );
            println!(
                "  size:    wanted {want_w}x{want_h} ({WINDOW_WIDTH_96}x{WINDOW_HEIGHT_96} @96 \
                 DPI, scaled to {dpi})   {}",
                if (win_w, win_h) == (want_w, want_h) {
                    "MATCH"
                } else {
                    "<<< FAIL, or the window was left resized by a human before this run"
                }
            );
            println!(
                "  floor:   {MIN_WIDTH_96}x{MIN_HEIGHT_96} @96 DPI -- not driven by this probe, \
                 a human must resize down to it by hand"
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
        // After `measure_geometry`, not inside it: the strip is chrome and is
        // present on every door, while everything that block reads belongs to
        // whichever page is open. Keeping them separate is what lets a second
        // run -- taken after a door change -- be read as a comparison of this
        // section alone.
        measure_strip(h);
        // After the strip, for the same reason the strip comes after
        // `measure_geometry`: this block reads one page's controls, so it
        // says something only on the run where that door is open. Cheap and
        // silent otherwise -- it prints one line saying so and returns.
        measure_system(h);
        // Same rule again, one door across.
        measure_about(h);

        // **Asked before anything reads a chord, and it is a control for the
        // probe itself.** Every shortcut this run prints is rebuilt from the
        // four modifier chips, and since they became `BS_OWNERDRAW` the only
        // way to read one is `WM_CHIP_STATE`. A `beckon-serve` that predates
        // that message answers 0 to all four, which `checked` collapses to
        // "clear" -- so without this line the probe would go on printing
        // chords, all of them wrong, all of them plausible.
        if chips_readable(h) {
            println!("  chip state: readable (WM_CHIP_STATE answered)");
        } else {
            println!(
                "  chip state: UNREADABLE -- this beckon-serve does not answer \
                 WM_CHIP_STATE (WM_APP+5). Every shortcut printed below is \
                 missing its modifiers. Rebuild and rerun; do not read the \
                 chords."
            );
        }

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
