//! The settings window: a list of shortcuts on the left, a detail panel on
//! the right, and the keyboard group below. Win32 only — every decision it
//! draws comes from `beckon_core::settings::ControlState`, and every edit
//! it collects goes back out through `Callbacks`. This file holds no policy.
//!
//! **Modeless, and created on the `serve` thread.** Hotkeys must keep
//! firing while it is open, so it cannot be a dialog box with its own modal
//! loop; `hotkey::run_forever` dispatches its messages like any others and
//! calls `filter_dialog_message` first so Tab/Esc/arrows work inside it.
//!
//! A deliberate non-feature: there is no "press a key to capture the
//! shortcut" field. `msctls_hotkey32` cannot capture the Windows key and
//! Explorer eats `Win+T` and its siblings before a normal window sees them,
//! so combos are typed as text and validated by the same parser `serve`
//! uses.

use crate::shell;
use beckon_core::settings::{ControlState, Mark};
use beckon_core::shortcuts::CapsTap;
use std::cell::RefCell;
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Controls::*;
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::Input::KeyboardAndMouse::EnableWindow;
use windows::Win32::UI::WindowsAndMessaging::*;

/// `SS_LEFT` is 0 and `windows` 0.61 does not export it as a constant.
const SS_LEFT_STYLE: WINDOW_STYLE = WINDOW_STYLE(0);

/// Posted by the catalog worker thread with the scanned app names.
pub const WM_CATALOG: u32 = WM_APP + 2;

const IDC_LIST: i32 = 1001;
const IDC_COMBO: i32 = 1002;
const IDC_APP: i32 = 1003;
const IDC_NOTES: i32 = 1004;
const IDC_ADD: i32 = 1005;
const IDC_REMOVE: i32 = 1006;
const IDC_APPLY: i32 = 1007;
const IDC_CAPS: i32 = 1008;
const IDC_TAP_CAPSLOCK: i32 = 1009;
const IDC_TAP_ESCAPE: i32 = 1010;
const IDC_TAP_NONE: i32 = 1011;
const IDC_OPENFILE: i32 = 1012;
const IDC_CLOSE: i32 = 1013;
const IDC_BANNER: i32 = 1014;
const IDC_RELOAD: i32 = 1015;
const IDC_KEEPMINE: i32 = 1016;
// Labels and the group box need real ids, not -1: `layout` positions
// controls through `GetDlgItem`, and every -1 resolves to the same first
// match, so sharing one id left all but the first stacked at the origin.
const IDC_LBL_SHORTCUT: i32 = 1017;
const IDC_LBL_APP: i32 = 1018;
const IDC_GRP_KEYBOARD: i32 = 1019;

/// Everything the window reports back. The caller owns all policy: what an
/// edit means, whether a close is allowed, what Apply writes.
pub struct Callbacks {
    pub on_select: Box<dyn FnMut(usize)>,
    pub on_edit_combo: Box<dyn FnMut(String)>,
    pub on_edit_app: Box<dyn FnMut(String)>,
    pub on_add: Box<dyn FnMut()>,
    pub on_remove: Box<dyn FnMut()>,
    pub on_apply: Box<dyn FnMut()>,
    pub on_caps: Box<dyn FnMut(bool)>,
    pub on_caps_tap: Box<dyn FnMut(CapsTap)>,
    pub on_open_file: Box<dyn FnMut()>,
    /// The installed-app catalog finished scanning.
    pub on_catalog: Box<dyn FnMut(Vec<String>)>,
    /// Reload the model from disk, discarding in-memory edits.
    pub on_reload_from_disk: Box<dyn FnMut()>,
    /// Keep the in-memory edits and dismiss the external-change banner.
    pub on_keep_mine: Box<dyn FnMut()>,
    /// `true` if the window may close. The caller shows any save prompt.
    pub on_close_request: Box<dyn FnMut() -> bool>,
}

struct Ui {
    hwnd: HWND,
    list: HWND,
    combo: HWND,
    app: HWND,
    notes: HWND,
    banner: HWND,
    reload: HWND,
    keep: HWND,
    font: HFONT,
    /// Set while `apply_state` is writing control contents, so the
    /// `EN_CHANGE`/`CBN_EDITCHANGE` those writes generate are not mistaken
    /// for the user typing. Without it, every repaint would feed the old
    /// text straight back into the model and mark it dirty.
    suppress: bool,
    /// Last state pushed, so the banner's visibility can be recomputed
    /// without asking the caller again.
    external_change: bool,
}

thread_local! {
    static UI: RefCell<Option<Ui>> = const { RefCell::new(None) };
    static CB: RefCell<Option<Callbacks>> = const { RefCell::new(None) };
}

/// The window's handle, or `None` when it is closed.
pub fn hwnd() -> Option<HWND> {
    UI.with(|u| u.borrow().as_ref().map(|ui| ui.hwnd))
}

/// An `HWND` a worker thread may carry.
///
/// `HWND` is a raw pointer and therefore not `Send`, but a window handle is
/// a kernel-side id, not a pointer into this thread's memory, and
/// `PostMessageW` is explicitly documented as callable from any thread —
/// posting to another thread's queue is the whole point of it. The only
/// thing this wrapper must never be used for is calling a window API that
/// requires the owning thread; the catalog worker calls exactly one
/// function, and it is `PostMessageW`.
#[derive(Clone, Copy)]
pub struct WindowHandle(pub HWND);
unsafe impl Send for WindowHandle {}

/// Raise the window that is already open. Cheaper than `open` when the
/// caller has already established there is one.
pub fn open_existing() -> bool {
    match hwnd() {
        Some(h) => unsafe { SetForegroundWindow(h) }.as_bool(),
        None => false,
    }
}

/// Give the settings window first refusal on a message so Tab, Esc and
/// arrow navigation work inside it. Returns `true` when it consumed the
/// message and the caller must not dispatch it.
///
/// `WM_HOTKEY` is not a dialog message and is never consumed here, so
/// hotkeys keep firing while the window is open — which is the entire
/// reason this window is modeless.
pub fn filter_dialog_message(msg: &MSG) -> bool {
    match hwnd() {
        Some(h) => unsafe { IsDialogMessageW(h, msg) }.as_bool(),
        None => false,
    }
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn text_of(h: HWND) -> String {
    let len = unsafe { GetWindowTextLengthW(h) };
    if len <= 0 {
        return String::new();
    }
    let mut buf = vec![0u16; len as usize + 1];
    let n = unsafe { GetWindowTextW(h, &mut buf) };
    String::from_utf16_lossy(&buf[..n.max(0) as usize])
}

fn set_text(h: HWND, s: &str) {
    unsafe {
        let _ = SetWindowTextW(h, PCWSTR(wide(s).as_ptr()));
    }
}

fn enable(parent: HWND, id: i32, on: bool) {
    if let Ok(h) = unsafe { GetDlgItem(Some(parent), id) } {
        unsafe {
            let _ = EnableWindow(h, on);
        }
    }
}

fn show(h: HWND, on: bool) {
    unsafe {
        let _ = ShowWindow(h, if on { SW_SHOW } else { SW_HIDE });
    }
}

fn mark_glyph(m: Mark) -> &'static str {
    // ASCII on purpose: this window inherits the shell font, and a missing
    // glyph shows as a box that reads like a rendering bug rather than a
    // status.
    match m {
        Mark::Ok => "OK",
        Mark::Bad => "!!",
        Mark::Unknown => "..",
    }
}

// ---------------------------------------------------------------------------
// Creation
// ---------------------------------------------------------------------------

/// Open the window, or raise it if it is already open.
pub fn open(cb: Callbacks) -> Result<(), String> {
    if let Some(h) = hwnd() {
        unsafe {
            let _ = SetForegroundWindow(h);
        }
        // Keep the existing callbacks: they close over the caller's live
        // state, and the second set would be a duplicate of the first.
        return Ok(());
    }
    CB.with(|c| *c.borrow_mut() = Some(cb));
    unsafe { create() }
}

unsafe fn create() -> Result<(), String> {
    let hinst = GetModuleHandleW(None).map_err(|e| format!("GetModuleHandleW: {e}"))?;

    // The common-controls DLL must be loaded before a SysListView32 is
    // created, or CreateWindowExW fails with "class not found".
    let icc = INITCOMMONCONTROLSEX {
        dwSize: std::mem::size_of::<INITCOMMONCONTROLSEX>() as u32,
        dwICC: ICC_LISTVIEW_CLASSES | ICC_STANDARD_CLASSES,
    };
    let _ = InitCommonControlsEx(&icc);

    let class = w!("BeckonSettingsWindow");
    let wc = WNDCLASSW {
        lpfnWndProc: Some(wndproc),
        hInstance: hinst.into(),
        lpszClassName: class,
        hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
        // WNDCLASS takes a system colour index PLUS ONE here, not a brush
        // and not the raw index -- 0 means "no background", so passing
        // COLOR_BTNFACE unshifted paints the window with COLOR_BTNSHADOW.
        hbrBackground: HBRUSH((COLOR_BTNFACE.0 + 1) as isize as *mut _),
        ..Default::default()
    };
    // Non-zero on success; a second call for an already-registered class
    // fails harmlessly, which is what happens when the window is reopened.
    RegisterClassW(&wc);

    let hwnd = CreateWindowExW(
        WINDOW_EX_STYLE(0),
        class,
        w!("beckon"),
        WS_OVERLAPPEDWINDOW,
        CW_USEDEFAULT,
        CW_USEDEFAULT,
        760,
        560,
        None,
        None,
        Some(hinst.into()),
        None,
    )
    .map_err(|e| format!("CreateWindowExW: {e}"))?;

    let _ = ShowWindow(hwnd, SW_SHOW);
    let _ = SetForegroundWindow(hwnd);
    Ok(())
}

/// The shell's UI font, so the window does not render in the 1995 bitmap
/// font Win32 defaults to.
unsafe fn ui_font() -> HFONT {
    let mut ncm = NONCLIENTMETRICSW {
        cbSize: std::mem::size_of::<NONCLIENTMETRICSW>() as u32,
        ..Default::default()
    };
    let ok = SystemParametersInfoW(
        SPI_GETNONCLIENTMETRICS,
        ncm.cbSize,
        Some(&mut ncm as *mut _ as *mut _),
        Default::default(),
    )
    .is_ok();
    if ok {
        let f = CreateFontIndirectW(&ncm.lfMessageFont);
        if !f.is_invalid() {
            return f;
        }
    }
    HFONT(GetStockObject(DEFAULT_GUI_FONT).0)
}

unsafe fn child(
    parent: HWND,
    class: PCWSTR,
    text: &str,
    style: WINDOW_STYLE,
    id: i32,
    font: HFONT,
) -> HWND {
    let h = CreateWindowExW(
        WINDOW_EX_STYLE(0),
        class,
        PCWSTR(wide(text).as_ptr()),
        style | WS_CHILD | WS_VISIBLE,
        0,
        0,
        10,
        10,
        Some(parent),
        Some(HMENU(id as isize as *mut _)),
        None,
        None,
    )
    .unwrap_or_default();
    SendMessageW(
        h,
        WM_SETFONT,
        Some(WPARAM(font.0 as usize)),
        Some(LPARAM(1)),
    );
    h
}

unsafe fn build_children(hwnd: HWND) {
    let font = ui_font();

    let list = child(
        hwnd,
        w!("SysListView32"),
        "",
        WINDOW_STYLE(LVS_REPORT | LVS_SINGLESEL | LVS_SHOWSELALWAYS | LVS_NOSORTHEADER)
            | WS_BORDER
            | WS_TABSTOP,
        IDC_LIST,
        font,
    );
    SendMessageW(
        list,
        LVM_SETEXTENDEDLISTVIEWSTYLE,
        Some(WPARAM(0)),
        Some(LPARAM(
            (LVS_EX_FULLROWSELECT | LVS_EX_DOUBLEBUFFER) as isize,
        )),
    );
    for (i, (title, cx)) in [("", 34), ("Shortcut", 190), ("App", 150)]
        .iter()
        .enumerate()
    {
        let mut t = wide(title);
        let col = LVCOLUMNW {
            mask: LVCF_TEXT | LVCF_WIDTH | LVCF_SUBITEM,
            cx: *cx,
            pszText: windows::core::PWSTR(t.as_mut_ptr()),
            iSubItem: i as i32,
            ..Default::default()
        };
        SendMessageW(
            list,
            LVM_INSERTCOLUMNW,
            Some(WPARAM(i)),
            Some(LPARAM(&col as *const _ as isize)),
        );
    }

    child(
        hwnd,
        w!("STATIC"),
        "Shortcut",
        SS_LEFT_STYLE,
        IDC_LBL_SHORTCUT,
        font,
    );
    let combo = child(
        hwnd,
        w!("EDIT"),
        "",
        WINDOW_STYLE(ES_AUTOHSCROLL as u32) | WS_BORDER | WS_TABSTOP,
        IDC_COMBO,
        font,
    );
    child(hwnd, w!("STATIC"), "App", SS_LEFT_STYLE, IDC_LBL_APP, font);
    // CBS_DROPDOWN, not CBS_DROPDOWNLIST: beckon deliberately supports apps
    // with no Start Menu entry, so free typing must stay possible even once
    // the catalog has loaded.
    let app = child(
        hwnd,
        w!("COMBOBOX"),
        "",
        WINDOW_STYLE((CBS_DROPDOWN | CBS_AUTOHSCROLL | CBS_SORT) as u32) | WS_VSCROLL | WS_TABSTOP,
        IDC_APP,
        font,
    );
    let notes = child(hwnd, w!("STATIC"), "", SS_LEFT_STYLE, IDC_NOTES, font);

    child(
        hwnd,
        w!("BUTTON"),
        "Add",
        WINDOW_STYLE(BS_PUSHBUTTON as u32) | WS_TABSTOP,
        IDC_ADD,
        font,
    );
    child(
        hwnd,
        w!("BUTTON"),
        "Remove",
        WINDOW_STYLE(BS_PUSHBUTTON as u32) | WS_TABSTOP,
        IDC_REMOVE,
        font,
    );
    child(
        hwnd,
        w!("BUTTON"),
        "Apply",
        WINDOW_STYLE(BS_DEFPUSHBUTTON as u32) | WS_TABSTOP,
        IDC_APPLY,
        font,
    );

    child(
        hwnd,
        w!("BUTTON"),
        "Keyboard",
        WINDOW_STYLE(BS_GROUPBOX as u32),
        IDC_GRP_KEYBOARD,
        font,
    );
    child(
        hwnd,
        w!("BUTTON"),
        "Use Caps Lock as the beckon key",
        WINDOW_STYLE(BS_AUTOCHECKBOX as u32) | WS_TABSTOP,
        IDC_CAPS,
        font,
    );
    child(
        hwnd,
        w!("BUTTON"),
        "Tapping Caps alone: Caps Lock",
        WINDOW_STYLE(BS_AUTORADIOBUTTON as u32) | WS_GROUP | WS_TABSTOP,
        IDC_TAP_CAPSLOCK,
        font,
    );
    child(
        hwnd,
        w!("BUTTON"),
        "Esc",
        WINDOW_STYLE(BS_AUTORADIOBUTTON as u32),
        IDC_TAP_ESCAPE,
        font,
    );
    child(
        hwnd,
        w!("BUTTON"),
        "nothing",
        WINDOW_STYLE(BS_AUTORADIOBUTTON as u32),
        IDC_TAP_NONE,
        font,
    );

    let banner = child(
        hwnd,
        w!("STATIC"),
        "This file changed on disk.",
        SS_LEFT_STYLE,
        IDC_BANNER,
        font,
    );
    let reload = child(
        hwnd,
        w!("BUTTON"),
        "Reload",
        WINDOW_STYLE(BS_PUSHBUTTON as u32) | WS_TABSTOP,
        IDC_RELOAD,
        font,
    );
    let keep = child(
        hwnd,
        w!("BUTTON"),
        "Keep mine",
        WINDOW_STYLE(BS_PUSHBUTTON as u32) | WS_TABSTOP,
        IDC_KEEPMINE,
        font,
    );
    show(banner, false);
    show(reload, false);
    show(keep, false);

    child(
        hwnd,
        w!("BUTTON"),
        "Open config file",
        WINDOW_STYLE(BS_PUSHBUTTON as u32) | WS_TABSTOP,
        IDC_OPENFILE,
        font,
    );
    child(
        hwnd,
        w!("BUTTON"),
        "Close",
        WINDOW_STYLE(BS_PUSHBUTTON as u32) | WS_TABSTOP,
        IDC_CLOSE,
        font,
    );

    UI.with(|u| {
        *u.borrow_mut() = Some(Ui {
            hwnd,
            list,
            combo,
            app,
            notes,
            banner,
            reload,
            keep,
            font,
            suppress: false,
            external_change: false,
        })
    });
}

// ---------------------------------------------------------------------------
// Layout
// ---------------------------------------------------------------------------

/// Everything is placed from the client rect at the current DPI, so a
/// 150 % display is not an afterthought — `GetDpiForWindow` scales the
/// constants rather than the constants assuming 96.
unsafe fn layout(hwnd: HWND) {
    let mut rc = RECT::default();
    if GetClientRect(hwnd, &mut rc).is_err() {
        return;
    }
    let dpi = GetDpiForWindow(hwnd).max(96);
    let s = |v: i32| v * dpi as i32 / 96;

    let pad = s(10);
    let row = s(24);
    let w = rc.right - rc.left;
    let h = rc.bottom - rc.top;

    let kb_h = s(72);
    let btn_h = s(26);
    let bottom_h = btn_h + pad;
    let banner_h = if UI.with(|u| {
        u.borrow()
            .as_ref()
            .map(|x| x.external_change)
            .unwrap_or(false)
    }) {
        row + pad
    } else {
        0
    };

    let top = pad + banner_h;
    let mid_h = h - top - kb_h - bottom_h - pad * 2;
    let list_w = (w - pad * 3) * 45 / 100;

    let place = |id: i32, x: i32, y: i32, cx: i32, cy: i32| {
        if let Ok(c) = GetDlgItem(Some(hwnd), id) {
            let _ = SetWindowPos(c, None, x, y, cx, cy, SWP_NOZORDER | SWP_NOACTIVATE);
        }
    };
    let place_h = |h_: HWND, x: i32, y: i32, cx: i32, cy: i32| {
        let _ = SetWindowPos(h_, None, x, y, cx, cy, SWP_NOZORDER | SWP_NOACTIVATE);
    };

    let (banner, reload, keep, list, combo, app, notes) = match UI.with(|u| {
        u.borrow()
            .as_ref()
            .map(|x| (x.banner, x.reload, x.keep, x.list, x.combo, x.app, x.notes))
    }) {
        Some(t) => t,
        None => return,
    };

    if banner_h > 0 {
        let bw = w - pad * 2 - s(180);
        place_h(banner, pad, pad + s(4), bw, row);
        place_h(reload, pad + bw + s(4), pad, s(84), row);
        place_h(keep, pad + bw + s(92), pad, s(84), row);
    }

    place_h(list, pad, top, list_w, mid_h - btn_h - s(6));
    place(IDC_ADD, pad, top + mid_h - btn_h, s(70), btn_h);
    place(IDC_REMOVE, pad + s(76), top + mid_h - btn_h, s(80), btn_h);

    let rx = pad * 2 + list_w;
    let rw = w - rx - pad;
    let mut y = top;
    place(IDC_LBL_SHORTCUT, rx, y, rw, row);
    y += row - s(6);
    place_h(combo, rx, y, rw, row);
    y += row + s(10);
    place(IDC_LBL_APP, rx, y, rw, row);
    y += row - s(6);
    // A combo box's height is the height of its dropped-down list, not of
    // the closed control; the closed control is sized by the system.
    place_h(app, rx, y, rw, row * 8);
    y += row + s(10);
    place_h(notes, rx, y, rw, mid_h - (y - top) - btn_h - s(6));
    place(
        IDC_APPLY,
        rx + rw - s(84),
        top + mid_h - btn_h,
        s(84),
        btn_h,
    );

    let ky = top + mid_h + pad;
    place(IDC_GRP_KEYBOARD, pad, ky, w - pad * 2, kb_h);
    place(IDC_CAPS, pad + s(12), ky + row, w - pad * 2 - s(24), row);
    let tx = pad + s(24);
    place(IDC_TAP_CAPSLOCK, tx, ky + row * 2, s(190), row);
    place(IDC_TAP_ESCAPE, tx + s(196), ky + row * 2, s(70), row);
    place(IDC_TAP_NONE, tx + s(270), ky + row * 2, s(90), row);

    let by = h - btn_h - pad;
    place(IDC_OPENFILE, w - pad - s(190), by, s(120), btn_h);
    place(IDC_CLOSE, w - pad - s(64), by, s(64), btn_h);
}

// ---------------------------------------------------------------------------
// Drawing
// ---------------------------------------------------------------------------

/// Push a snapshot into the controls. The only path that changes what is on
/// screen; the window never reads the model.
pub fn apply_state(st: &ControlState, external_change: bool, catalog: Option<&[String]>) {
    let Some((hwnd, list, combo, app, notes, banner, reload, keep)) = UI.with(|u| {
        u.borrow().as_ref().map(|x| {
            (
                x.hwnd, x.list, x.combo, x.app, x.notes, x.banner, x.reload, x.keep,
            )
        })
    }) else {
        return;
    };
    // Writing control text raises EN_CHANGE / CBN_EDITCHANGE. Without this
    // guard every repaint would feed the control's own text back into the
    // model and mark it dirty.
    UI.with(|u| {
        if let Some(ui) = u.borrow_mut().as_mut() {
            ui.suppress = true;
            ui.external_change = external_change;
        }
    });

    unsafe {
        SendMessageW(list, LVM_DELETEALLITEMS, Some(WPARAM(0)), Some(LPARAM(0)));
        for (i, it) in st.items.iter().enumerate() {
            let mut mark = wide(mark_glyph(it.mark));
            let item = LVITEMW {
                mask: LVIF_TEXT,
                iItem: i as i32,
                iSubItem: 0,
                pszText: windows::core::PWSTR(mark.as_mut_ptr()),
                ..Default::default()
            };
            SendMessageW(
                list,
                LVM_INSERTITEMW,
                Some(WPARAM(0)),
                Some(LPARAM(&item as *const _ as isize)),
            );
            for (sub, text) in [(1, &it.combo), (2, &it.app)] {
                let mut t = wide(text);
                let si = LVITEMW {
                    mask: LVIF_TEXT,
                    iItem: i as i32,
                    iSubItem: sub,
                    pszText: windows::core::PWSTR(t.as_mut_ptr()),
                    ..Default::default()
                };
                SendMessageW(
                    list,
                    LVM_SETITEMW,
                    Some(WPARAM(0)),
                    Some(LPARAM(&si as *const _ as isize)),
                );
            }
        }

        if let Some(names) = catalog {
            // Repopulating on every repaint would fight the user's typing;
            // only fill an empty list.
            let count = SendMessageW(app, CB_GETCOUNT, Some(WPARAM(0)), Some(LPARAM(0))).0;
            if count <= 0 {
                for n in names {
                    let t = wide(n);
                    SendMessageW(
                        app,
                        CB_ADDSTRING,
                        Some(WPARAM(0)),
                        Some(LPARAM(t.as_ptr() as isize)),
                    );
                }
            }
        }

        match &st.detail {
            Some(d) => {
                enable(hwnd, IDC_COMBO, true);
                enable(hwnd, IDC_APP, true);
                if text_of(combo) != d.combo {
                    set_text(combo, &d.combo);
                }
                if text_of(app) != d.app {
                    set_text(app, &d.app);
                }
                let body: Vec<String> = d
                    .notes
                    .iter()
                    .map(|n| format!("{}  {}", mark_glyph(n.mark), n.text))
                    .collect();
                set_text(notes, &body.join("\r\n"));
            }
            None => {
                enable(hwnd, IDC_COMBO, false);
                enable(hwnd, IDC_APP, false);
                set_text(combo, "");
                set_text(app, "");
                set_text(notes, "Select a shortcut, or press Add.");
            }
        }

        enable(hwnd, IDC_APPLY, st.apply_enabled);
        enable(hwnd, IDC_REMOVE, st.remove_enabled);
        check(hwnd, IDC_CAPS, st.caps_checked);
        check(hwnd, IDC_TAP_CAPSLOCK, st.caps_tap == CapsTap::CapsLock);
        check(hwnd, IDC_TAP_ESCAPE, st.caps_tap == CapsTap::Escape);
        check(hwnd, IDC_TAP_NONE, st.caps_tap == CapsTap::None);
        // The tap choice only means anything when Caps is on.
        for id in [IDC_TAP_CAPSLOCK, IDC_TAP_ESCAPE, IDC_TAP_NONE] {
            enable(hwnd, id, st.caps_checked);
        }

        show(banner, external_change);
        show(reload, external_change);
        show(keep, external_change);
        layout(hwnd);
    }

    UI.with(|u| {
        if let Some(ui) = u.borrow_mut().as_mut() {
            ui.suppress = false;
        }
    });
}

unsafe fn check(parent: HWND, id: i32, on: bool) {
    if let Ok(h) = GetDlgItem(Some(parent), id) {
        SendMessageW(
            h,
            BM_SETCHECK,
            Some(WPARAM(
                if on { BST_CHECKED.0 } else { BST_UNCHECKED.0 } as usize
            )),
            Some(LPARAM(0)),
        );
    }
}

/// Hand the scanned catalog to the window, from the worker thread.
///
/// The `Vec` is leaked into the message and reclaimed by the `WM_CATALOG`
/// arm of `wndproc`. If the post fails — the window closed while the scan
/// was running — this reclaims it here instead, so the failure costs
/// nothing but the scan.
pub fn post_catalog(target: WindowHandle, names: Vec<String>) {
    let boxed = Box::into_raw(Box::new(names));
    let posted = unsafe {
        PostMessageW(
            Some(target.0),
            WM_CATALOG,
            WPARAM(0),
            LPARAM(boxed as isize),
        )
    };
    if posted.is_err() {
        drop(unsafe { Box::from_raw(boxed) });
    }
}

// ---------------------------------------------------------------------------
// Message handling
// ---------------------------------------------------------------------------

fn with_cb(f: impl FnOnce(&mut Callbacks)) {
    // Take-then-run, matching `hotkey.rs`: a handler that pumps (open_path's
    // ShellExecuteW, a MessageBox) can re-enter this window's wndproc, and a
    // second borrow of the same RefCell would panic across an
    // `extern "system"` boundary, which aborts the process rather than
    // unwinding.
    let taken = CB.with(|c| c.borrow_mut().take());
    if let Some(mut cb) = taken {
        f(&mut cb);
        CB.with(|c| {
            if c.borrow().is_none() {
                *c.borrow_mut() = Some(cb);
            }
        });
    }
}

fn suppressed() -> bool {
    UI.with(|u| u.borrow().as_ref().map(|x| x.suppress).unwrap_or(true))
}

extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_CREATE => {
                build_children(hwnd);
                layout(hwnd);
                LRESULT(0)
            }
            WM_SIZE | WM_DPICHANGED => {
                layout(hwnd);
                LRESULT(0)
            }
            WM_CATALOG => {
                // Reclaims what `post_catalog` leaked into the message.
                let names = *Box::from_raw(lp.0 as *mut Vec<String>);
                // The caller stores it and calls back into `apply_state`,
                // which is what actually fills the combo box -- one path
                // for putting things on screen, not two.
                with_cb(|cb| (cb.on_catalog)(names));
                LRESULT(0)
            }
            WM_NOTIFY => {
                let nm = &*(lp.0 as *const NMHDR);
                if nm.idFrom == IDC_LIST as usize && nm.code == LVN_ITEMCHANGED {
                    let lv = &*(lp.0 as *const NMLISTVIEW);
                    if (lv.uNewState & LVIS_SELECTED.0) != 0
                        && (lv.uOldState & LVIS_SELECTED.0) == 0
                    {
                        let i = lv.iItem as usize;
                        with_cb(|cb| (cb.on_select)(i));
                    }
                }
                LRESULT(0)
            }
            WM_COMMAND => {
                let id = (wp.0 & 0xFFFF) as i32;
                let code = ((wp.0 >> 16) & 0xFFFF) as u32;
                handle_command(hwnd, id, code);
                LRESULT(0)
            }
            WM_CLOSE => {
                let mut may = true;
                with_cb(|cb| may = (cb.on_close_request)());
                if may {
                    let _ = DestroyWindow(hwnd);
                }
                LRESULT(0)
            }
            WM_DESTROY => {
                UI.with(|u| {
                    if let Some(ui) = u.borrow_mut().take() {
                        let _ = DeleteObject(HGDIOBJ(ui.font.0));
                    }
                });
                CB.with(|c| *c.borrow_mut() = None);
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wp, lp),
        }
    }
}

/// Push whatever the two edit fields currently show into the model.
///
/// Separate from the per-keystroke notifications on purpose: those tell us
/// *that* something changed, but a control is free to rewrite its own text
/// afterwards without saying so, and a combo box with a populated list does
/// exactly that. This reads the final state.
fn commit_fields() {
    if suppressed() {
        return;
    }
    let Some((combo, app)) = UI.with(|u| u.borrow().as_ref().map(|x| (x.combo, x.app))) else {
        return;
    };
    let c = text_of(combo);
    let a = text_of(app);
    with_cb(|cb| (cb.on_edit_combo)(c));
    with_cb(|cb| (cb.on_edit_app)(a));
}

fn handle_command(hwnd: HWND, id: i32, code: u32) {
    let (combo, app) = match UI.with(|u| u.borrow().as_ref().map(|x| (x.combo, x.app))) {
        Some(t) => t,
        None => return,
    };
    match (id, code) {
        (IDC_COMBO, c) if c == EN_CHANGE => {
            if !suppressed() {
                let t = text_of(combo);
                with_cb(|cb| (cb.on_edit_combo)(t));
            }
        }
        (IDC_APP, c) if c == CBN_EDITCHANGE || c == CBN_SELCHANGE => {
            if !suppressed() {
                // On CBN_SELCHANGE the edit field has not been updated yet,
                // so read the selected item instead of the field.
                let t = if c == CBN_SELCHANGE {
                    selected_combo_text(app).unwrap_or_else(|| text_of(app))
                } else {
                    text_of(app)
                };
                with_cb(|cb| (cb.on_edit_app)(t));
            }
        }
        // Tabbing or clicking away commits what is in the field, so a value
        // the control rewrote without notifying is not silently lost.
        (IDC_COMBO, c) if c == EN_KILLFOCUS => commit_fields(),
        (IDC_APP, c) if c == CBN_KILLFOCUS || c == CBN_CLOSEUP => commit_fields(),
        (IDC_ADD, _) => with_cb(|cb| (cb.on_add)()),
        (IDC_REMOVE, _) => with_cb(|cb| (cb.on_remove)()),
        (IDC_APPLY, _) => {
            // The fields are the source of truth at the moment Apply is
            // pressed.
            //
            // Measured on a14: a COMBOBOX whose list is populated jumps to
            // the matching entry as you type -- 'N' leaves "Narrator" in
            // the field, 'o' leaves "Obsidian" -- and the CBN_EDITCHANGE
            // that reaches us carries the text from BEFORE that rewrite,
            // i.e. the single character just typed. Trusting the
            // incremental notifications alone therefore wrote "d" to the
            // file while the screen said "Debuggable Package Manager".
            // Incremental notifications still drive the enabled state; this
            // is what decides the content.
            commit_fields();
            with_cb(|cb| (cb.on_apply)())
        }
        (IDC_CAPS, _) => {
            let on = unsafe {
                GetDlgItem(Some(hwnd), IDC_CAPS)
                    .map(|h| {
                        SendMessageW(h, BM_GETCHECK, Some(WPARAM(0)), Some(LPARAM(0))).0
                            == BST_CHECKED.0 as isize
                    })
                    .unwrap_or(false)
            };
            with_cb(|cb| (cb.on_caps)(on));
        }
        (IDC_TAP_CAPSLOCK, _) => with_cb(|cb| (cb.on_caps_tap)(CapsTap::CapsLock)),
        (IDC_TAP_ESCAPE, _) => with_cb(|cb| (cb.on_caps_tap)(CapsTap::Escape)),
        (IDC_TAP_NONE, _) => with_cb(|cb| (cb.on_caps_tap)(CapsTap::None)),
        (IDC_OPENFILE, _) => with_cb(|cb| (cb.on_open_file)()),
        (IDC_RELOAD, _) => with_cb(|cb| (cb.on_reload_from_disk)()),
        (IDC_KEEPMINE, _) => with_cb(|cb| (cb.on_keep_mine)()),
        // Both the Close button and Esc (which IsDialogMessage turns into
        // IDCANCEL) go through WM_CLOSE, so the save prompt is asked once.
        (IDC_CLOSE, _) | (2 /* IDCANCEL */, _) => unsafe {
            let _ = PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
        },
        _ => {}
    }
}

fn selected_combo_text(app: HWND) -> Option<String> {
    unsafe {
        let i = SendMessageW(app, CB_GETCURSEL, Some(WPARAM(0)), Some(LPARAM(0))).0;
        if i < 0 {
            return None;
        }
        let len = SendMessageW(
            app,
            CB_GETLBTEXTLEN,
            Some(WPARAM(i as usize)),
            Some(LPARAM(0)),
        )
        .0;
        if len <= 0 {
            return None;
        }
        let mut buf = vec![0u16; len as usize + 1];
        SendMessageW(
            app,
            CB_GETLBTEXT,
            Some(WPARAM(i as usize)),
            Some(LPARAM(buf.as_mut_ptr() as isize)),
        );
        let n = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        Some(String::from_utf16_lossy(&buf[..n]))
    }
}

/// Report a save failure. The window has somewhere to put this, unlike
/// bare `serve`.
pub fn error(body: &str) {
    shell::error_dialog("beckon", body);
}
