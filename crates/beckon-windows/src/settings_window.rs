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
use beckon_core::settings::{ControlState, ListItem, Mark};
use beckon_core::shortcuts::CapsTap;
use std::cell::RefCell;
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Controls::*;
use windows::Win32::UI::HiDpi::{
    GetDpiForMonitor, GetDpiForWindow, SystemParametersInfoForDpi, MDT_EFFECTIVE_DPI,
};
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

/// ListView header (title, width-at-96-DPI) pairs. Shared between creation
/// (`LVM_INSERTCOLUMNW`, in `build_children`) and `WM_DPICHANGED`
/// (`LVM_SETCOLUMNWIDTH`), so the two widths cannot drift apart.
const LIST_COLUMNS: [(&str, i32); 3] = [("", 34), ("Shortcut", 190), ("App", 150)];

/// A row's tick, as `LVIS_STATEIMAGEMASK` bits: the one-based index of the
/// state image, shifted up by 12. Image 1 is the empty box and image 2 the
/// ticked one; **0 means "no state image at all"**, which is what an item
/// inserted without `LVIF_STATE` gets -- and the `LVN_ITEMCHANGED` that
/// comctl32 then fires as it paints the first box (0 -> 1) is
/// indistinguishable from a user clicking a tick off.
///
/// This pair is also why `ListView_GetCheckState` is not ported: it is
/// `(state >> 12) - 1` on an *unsigned* value, so an item that never got a
/// state image reads back `0xFFFFFFFF` rather than `0`. Read
/// `LVM_GETITEMSTATE` masked by `LVIS_STATEIMAGEMASK` and compare against
/// these instead.
const LVIS_UNCHECKED: u32 = 1 << 12; // 0x1000
const LVIS_CHECKED: u32 = 2 << 12; // 0x2000

/// Window creation size, at 96 DPI. Shared between the initial
/// `CreateWindowExW` and the post-creation `SetWindowPos` correction (the
/// window is born on whichever monitor `CW_USEDEFAULT` picked, which
/// `GetDpiForWindow` can then reveal was guessed wrong) -- both must agree
/// on the un-scaled size or the correction would resize to the wrong target.
const WINDOW_WIDTH: i32 = 760;
const WINDOW_HEIGHT: i32 = 560;

/// Minimum resize size, at 96 DPI, enforced in `WM_GETMINMAXINFO`. Smaller
/// than `WINDOW_WIDTH`/`WINDOW_HEIGHT` so the window can be shrunk, but not
/// below the point where `layout` starts overlapping controls.
const MIN_WIDTH: i32 = 720;
const MIN_HEIGHT: i32 = 460;

/// Scales a 96-DPI value to `dpi`. The only scaling rule in this file --
/// `MulDiv` (round-half-up) was tried for the creation size and the list
/// columns and dropped, because it quietly disagrees with this truncating
/// formula at in-between DPIs (at 125%: `10 * 120 / 96 == 12` here, but
/// `MulDiv(10, 120, 96) == 13`). `layout`'s own `s` closure computes the
/// same thing inline, for a value it already has in scope.
fn scale(v: i32, dpi: u32) -> i32 {
    v * dpi as i32 / 96
}

/// Everything the window reports back. The caller owns all policy: what an
/// edit means, whether a close is allowed, what Apply writes.
pub struct Callbacks {
    pub on_select: Box<dyn FnMut(usize)>,
    /// A row's tick changed: `(index, ticked)`. Independent of `on_select`
    /// -- one click can raise both, and neither implies the other.
    pub on_mark: Box<dyn FnMut(usize, bool)>,
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
    /// The rows currently in the ListView, exactly as `apply_state` last
    /// pushed them. `apply_state` diffs the next snapshot against this
    /// instead of deleting and reinserting, which is what stops a
    /// keystroke from wiping the ticks and scrolling back to the top.
    ///
    /// Never read while a message is in flight: every use takes it out of
    /// the `RefCell` first (`mem::take`), so an empty vector means "the
    /// control's contents are unknown" and the next push rebuilds -- which
    /// is always correct, just slower.
    items: Vec<ListItem>,
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
    // status. All four are two columns wide so the notes below the list line
    // up -- the trailing space on `Warn` is load-bearing, not a typo.
    match m {
        Mark::Ok => "OK",
        Mark::Warn => "! ",
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
    // Resource id 1, the same icon beckon.rc embeds and the tray already
    // uses. hIcon wants the large (SM_CXICON, 32x32) variant LoadIconW
    // returns; hIconSm wants the small (SM_CXSMICON, typically 16x16) one,
    // loaded explicitly via LoadImageW exactly like the tray's own
    // tray_add -- letting the shell downsample the large icon to 16x16 on
    // the fly is what tray_add's comment says blurs an icon that is crisp
    // at 16x16 in the .ico itself. Both fall back to the stock
    // IDI_APPLICATION icon, matching tray_add, so a build without the .rc
    // resource still shows an icon instead of none.
    let icon = LoadIconW(Some(hinst.into()), PCWSTR(1 as *const u16))
        .or_else(|_| LoadIconW(None, IDI_APPLICATION))
        .unwrap_or_default();
    let icon_sm = LoadImageW(
        Some(hinst.into()),
        PCWSTR(1 as *const u16),
        IMAGE_ICON,
        GetSystemMetrics(SM_CXSMICON),
        GetSystemMetrics(SM_CYSMICON),
        LR_DEFAULTCOLOR,
    )
    .map(|h| HICON(h.0))
    .or_else(|_| LoadIconW(None, IDI_APPLICATION))
    .unwrap_or_default();
    // WNDCLASSEXW, not WNDCLASSW: the brief called for hIconSm, but that
    // field only exists on the Ex struct (paired with RegisterClassExW) --
    // WNDCLASSW has no small-icon slot at all. Same feature flag either way
    // (Win32_UI_WindowsAndMessaging), so this is not a new dependency.
    let wc = WNDCLASSEXW {
        cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
        lpfnWndProc: Some(wndproc),
        hInstance: hinst.into(),
        lpszClassName: class,
        hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
        // WNDCLASS takes a system colour index PLUS ONE here, not a brush
        // and not the raw index -- 0 means "no background", so passing
        // COLOR_BTNFACE unshifted paints the window with COLOR_BTNSHADOW.
        hbrBackground: HBRUSH((COLOR_BTNFACE.0 + 1) as isize as *mut _),
        hIcon: icon,
        hIconSm: icon_sm,
        ..Default::default()
    };
    // Non-zero on success; a second call for an already-registered class
    // fails harmlessly, which is what happens when the window is reopened.
    RegisterClassExW(&wc);

    // CW_USEDEFAULT for position, but the SIZE must be scaled by hand:
    // under per-monitor-v2 these are physical pixels, and no WM_DPICHANGED
    // arrives to correct a window that was born the wrong size.
    let dpi = {
        let mut pt = POINT::default();
        let _ = GetCursorPos(&mut pt);
        let mon = MonitorFromPoint(pt, MONITOR_DEFAULTTOPRIMARY);
        let (mut x, mut y) = (96u32, 96u32);
        let _ = GetDpiForMonitor(mon, MDT_EFFECTIVE_DPI, &mut x, &mut y);
        x.max(96)
    };
    let w = scale(WINDOW_WIDTH, dpi);
    let h = scale(WINDOW_HEIGHT, dpi);

    let hwnd = CreateWindowExW(
        WINDOW_EX_STYLE(0),
        class,
        w!("beckon"),
        WS_OVERLAPPEDWINDOW,
        CW_USEDEFAULT,
        CW_USEDEFAULT,
        w,
        h,
        None,
        None,
        Some(hinst.into()),
        None,
    )
    .map_err(|e| format!("CreateWindowExW: {e}"))?;

    // Position was CW_USEDEFAULT, so Windows -- not the cursor position
    // used above -- decided which monitor the window actually landed on.
    // GetDpiForWindow(hwnd) is authoritative now that hwnd exists; correct
    // the size once, before anything is shown, if the guess was wrong. No
    // WM_DPICHANGED arrives to do this for us: the window was already born
    // on its final monitor, so nothing "changed" from Windows' point of view.
    let real_dpi = GetDpiForWindow(hwnd).max(96);
    if real_dpi != dpi {
        let _ = SetWindowPos(
            hwnd,
            None,
            0,
            0,
            scale(WINDOW_WIDTH, real_dpi),
            scale(WINDOW_HEIGHT, real_dpi),
            SWP_NOZORDER | SWP_NOACTIVATE | SWP_NOMOVE,
        );
    }

    let _ = ShowWindow(hwnd, SW_SHOW);
    let _ = SetForegroundWindow(hwnd);
    Ok(())
}

/// The shell's UI font at a specific DPI.
///
/// `SystemParametersInfoForDpi` first: `SystemParametersInfoW` answers for
/// the system DPI, which is the wrong number for a per-monitor-v2 process on
/// a secondary display. But `SystemParametersInfoForDpi` is documented as
/// valid only for a DPI-aware process, and can fail where the old call never
/// did -- `build.rs` embeds the manifest from Task 6 only for `-msvc`, so a
/// `-gnu` build, or `cargo install --git` on a host with no resource
/// compiler, is still DPI-unaware. Falling back to `SystemParametersInfoW`
/// there keeps the real shell font instead of dropping straight to the
/// stock 1995 bitmap font this whole function exists to avoid. Whether
/// `SystemParametersInfoForDpi` actually returns FALSE on a non-PM process,
/// rather than silently answering for the system DPI, is not something a
/// cross-compile can confirm -- unverified, flagged for the hardware pass.
unsafe fn ui_font(dpi: u32) -> HFONT {
    let mut ncm = NONCLIENTMETRICSW {
        cbSize: std::mem::size_of::<NONCLIENTMETRICSW>() as u32,
        ..Default::default()
    };
    let ok = SystemParametersInfoForDpi(
        SPI_GETNONCLIENTMETRICS.0,
        ncm.cbSize,
        Some(&mut ncm as *mut _ as *mut _),
        0,
        dpi,
    )
    .is_ok()
        || SystemParametersInfoW(
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
    let dpi = GetDpiForWindow(hwnd).max(96);
    let font = ui_font(dpi);

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
    // LVS_EX_CHECKBOXES rides in column 0's state image, beside its text --
    // it is not a column, so deleting the status column later is compatible.
    // The window style above deliberately keeps LVS_SINGLESEL: ticks are
    // independent of the highlight, so several rows can be marked for
    // deletion while the editor strip still has exactly one current row.
    // LVS_EX_AUTOCHECKSELECT is the opposite of that and must never appear.
    SendMessageW(
        list,
        LVM_SETEXTENDEDLISTVIEWSTYLE,
        Some(WPARAM(0)),
        Some(LPARAM(
            (LVS_EX_FULLROWSELECT | LVS_EX_DOUBLEBUFFER | LVS_EX_CHECKBOXES) as isize,
        )),
    );
    let sx = |v: i32| scale(v, dpi);
    for (i, (title, cx)) in LIST_COLUMNS.iter().enumerate() {
        let mut t = wide(title);
        let col = LVCOLUMNW {
            mask: LVCF_TEXT | LVCF_WIDTH | LVCF_SUBITEM,
            cx: sx(*cx),
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
    // Under comctl32 v6 the `cy` passed to SetWindowPos no longer decides
    // how tall the drop-down is; this does. Without it the list opens at
    // the default 30 items regardless of the height layout computes.
    SendMessageW(app, CB_SETMINVISIBLE, Some(WPARAM(8)), Some(LPARAM(0)));
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
            items: Vec::new(),
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
    // Independent of WM_GETMINMAXINFO: the floor is about the frame, and a
    // clamp is about the arithmetic. Either alone leaves a negative cy
    // reachable -- SetWindowPos with one produces a control the user can
    // never see or focus again.
    let clamp = |v: i32| v.max(0);

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
    let mid_h = clamp(h - top - kb_h - bottom_h - pad * 2);
    // Widths need the same guard as heights: WM_SIZE fires with a 0x0
    // client rect on minimize (ptMinTrackSize only constrains dragging,
    // not that), so w == 0 here on every minimize, on every machine, and
    // every subtraction below goes negative without this.
    let list_w = clamp((w - pad * 3) * 45 / 100);

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
        let bw = clamp(w - pad * 2 - s(180));
        place_h(banner, pad, pad + s(4), bw, row);
        place_h(reload, pad + bw + s(4), pad, s(84), row);
        place_h(keep, pad + bw + s(92), pad, s(84), row);
    }

    place_h(list, pad, top, list_w, clamp(mid_h - btn_h - s(6)));
    place(IDC_ADD, pad, top + mid_h - btn_h, s(70), btn_h);
    place(IDC_REMOVE, pad + s(76), top + mid_h - btn_h, s(80), btn_h);

    let rx = pad * 2 + list_w;
    let rw = clamp(w - rx - pad);
    let mut y = top;
    place(IDC_LBL_SHORTCUT, rx, y, rw, row);
    y += row - s(6);
    place_h(combo, rx, y, rw, row);
    y += row + s(10);
    place(IDC_LBL_APP, rx, y, rw, row);
    y += row - s(6);
    // A combo box's height is the height of its dropped-down list, not of
    // the closed control; the closed control is sized by the system. That
    // was the whole story under comctl32 v5, but not v6: there, `cy` here
    // is capped by the minimum-visible-items count, and `build_children`'s
    // CB_SETMINVISIBLE(app, 8) is what actually governs the drop-down
    // height now. Changing `row * 8` alone, without touching that call,
    // does nothing on a v6 box.
    place_h(app, rx, y, rw, row * 8);
    y += row + s(10);
    place_h(notes, rx, y, rw, clamp(mid_h - (y - top) - btn_h - s(6)));
    place(
        IDC_APPLY,
        rx + rw - s(84),
        top + mid_h - btn_h,
        s(84),
        btn_h,
    );

    let ky = top + mid_h + pad;
    // Not named in the review that asked for this guard, but the same
    // formula minus one term (w - pad * 2, vs. IDC_CAPS's w - pad * 2 -
    // s(24) right below) -- same 0x0-on-minimize hazard, same fix.
    place(IDC_GRP_KEYBOARD, pad, ky, clamp(w - pad * 2), kb_h);
    place(
        IDC_CAPS,
        pad + s(12),
        ky + row,
        clamp(w - pad * 2 - s(24)),
        row,
    );
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
    // Taken, not cloned, and taken HERE: every `SendMessageW` below can
    // re-enter this window's wndproc, `suppressed()` takes a fresh
    // `UI.borrow()` when it does, and a second borrow across an
    // `extern "system"` boundary aborts the process instead of unwinding.
    // So no borrow may be alive once the sending starts. Taking also makes
    // the failure mode safe: a lost cache means the next push rebuilds.
    let prev: Vec<ListItem> = UI.with(|u| {
        u.borrow_mut()
            .as_mut()
            .map(|x| std::mem::take(&mut x.items))
            .unwrap_or_default()
    });
    // Writing control text raises EN_CHANGE / CBN_EDITCHANGE. Without this
    // guard every repaint would feed the control's own text back into the
    // model and mark it dirty. It is also what swallows the LVN_ITEMCHANGED
    // that `sync_list`'s own `LVM_SETITEMSTATE` fires synchronously.
    UI.with(|u| {
        if let Some(ui) = u.borrow_mut().as_mut() {
            ui.suppress = true;
            ui.external_change = external_change;
        }
    });

    unsafe {
        sync_list(list, &prev, st);

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

    // Nothing is sent from here on, so this borrow is safe to hold while it
    // records what the control now shows.
    UI.with(|u| {
        if let Some(ui) = u.borrow_mut().as_mut() {
            ui.suppress = false;
            ui.items = st.items.clone();
        }
    });
}

/// The column texts for one row, in `LIST_COLUMNS` order.
///
/// Both the rebuild and the diff go through here, so they cannot disagree
/// about what a cell says -- and the column set is one edit, in one place,
/// when it changes.
fn cells(it: &ListItem) -> Vec<String> {
    vec![
        mark_glyph(it.mark).to_string(),
        it.combo.clone(),
        it.app.clone(),
    ]
}

/// Push `st.items` into the ListView, rebuilding only when it has to.
///
/// **The row count is the whole discriminator.** Every text edit leaves it
/// unchanged, so every text edit takes the diff, where nothing is deleted
/// and therefore neither the scroll position nor a tick nor the highlight
/// can be disturbed. Only Add, Remove and a reload change it, and only they
/// pay for a rebuild. That is what keeps this trivial: no keyed
/// reconciliation and no ids in `LVITEM.lParam`.
///
/// The control's own count is consulted as well as the cache, so a list
/// emptied by anything other than `apply_state` rebuilds rather than being
/// written past the end.
unsafe fn sync_list(list: HWND, prev: &[ListItem], st: &ControlState) {
    let live = SendMessageW(list, LVM_GETITEMCOUNT, Some(WPARAM(0)), Some(LPARAM(0))).0;
    if prev.len() != st.items.len() || live != st.items.len() as isize {
        rebuild_list(list, st);
        return;
    }
    for (i, it) in st.items.iter().enumerate() {
        let now = cells(it);
        let was = cells(&prev[i]);
        for (sub, text) in now.iter().enumerate() {
            if was.get(sub) != Some(text) {
                set_item_text(list, i, sub as i32, text);
            }
        }
        set_item_state(list, i, it.marked, st.selected == Some(i));
    }
}

/// Delete and reinsert every row. Only for a changed row count.
unsafe fn rebuild_list(list: HWND, st: &ControlState) {
    // Read the scroll position while it still means something.
    let top = SendMessageW(list, LVM_GETTOPINDEX, Some(WPARAM(0)), Some(LPARAM(0))).0;
    let per = SendMessageW(list, LVM_GETCOUNTPERPAGE, Some(WPARAM(0)), Some(LPARAM(0))).0;

    SendMessageW(list, WM_SETREDRAW, Some(WPARAM(0)), Some(LPARAM(0)));
    SendMessageW(list, LVM_DELETEALLITEMS, Some(WPARAM(0)), Some(LPARAM(0)));
    for (i, it) in st.items.iter().enumerate() {
        let texts = cells(it);
        let mut first = wide(&texts[0]);
        // The state goes in with the insert, not after it: an item that is
        // inserted without LVIF_STATE has no state image, and the
        // LVN_ITEMCHANGED comctl32 fires when it paints the first empty box
        // looks exactly like the user clicking a tick off.
        //
        // LVIS_FOCUSED is deliberately absent. Setting it scrolls the item
        // into view, which would fight the scroll restore below.
        let item = LVITEMW {
            mask: LVIF_TEXT | LVIF_STATE,
            iItem: i as i32,
            iSubItem: 0,
            pszText: windows::core::PWSTR(first.as_mut_ptr()),
            stateMask: LIST_VIEW_ITEM_STATE_FLAGS(LVIS_STATEIMAGEMASK.0 | LVIS_SELECTED.0),
            state: LIST_VIEW_ITEM_STATE_FLAGS(
                check_bits(it.marked) | selected_bits(st.selected == Some(i)),
            ),
            ..Default::default()
        };
        SendMessageW(
            list,
            LVM_INSERTITEMW,
            Some(WPARAM(0)),
            Some(LPARAM(&item as *const _ as isize)),
        );
        for (sub, text) in texts.iter().enumerate().skip(1) {
            set_item_text(list, i, sub as i32, text);
        }
    }

    SendMessageW(list, WM_SETREDRAW, Some(WPARAM(1)), Some(LPARAM(0)));

    // A rebuild leaves the view at the top, so a lone ENSUREVISIBLE(top)
    // does nothing at all -- `top` is already on screen. Ensuring the
    // BOTTOM of the page that used to be showing is what scrolls; ensuring
    // `top` afterwards stops it overshooting by a row.
    //
    // After WM_SETREDRAW TRUE on purpose, so the scroll is not asked of a
    // control that has been told not to draw. It costs no flicker: lifting
    // the block does not paint, it only marks the control dirty, and
    // nothing reaches the screen until the WM_PAINT that follows this
    // whole refresh.
    let count = st.items.len() as isize;
    if count > 0 && top > 0 {
        let top = top.min(count - 1);
        let bottom = (top + per.max(1) - 1).min(count - 1);
        ensure_visible(list, bottom);
        ensure_visible(list, top);
    }

    let _ = InvalidateRect(Some(list), None, true);
}

unsafe fn ensure_visible(list: HWND, i: isize) {
    SendMessageW(
        list,
        LVM_ENSUREVISIBLE,
        Some(WPARAM(i as usize)),
        // fPartialOK = FALSE: the row must be fully on screen, or the pair
        // above can land half a row short.
        Some(LPARAM(0)),
    );
}

unsafe fn set_item_text(list: HWND, i: usize, sub: i32, text: &str) {
    let mut t = wide(text);
    let it = LVITEMW {
        iSubItem: sub,
        pszText: windows::core::PWSTR(t.as_mut_ptr()),
        ..Default::default()
    };
    SendMessageW(
        list,
        LVM_SETITEMTEXTW,
        Some(WPARAM(i)),
        Some(LPARAM(&it as *const _ as isize)),
    );
}

fn check_bits(on: bool) -> u32 {
    if on {
        LVIS_CHECKED
    } else {
        LVIS_UNCHECKED
    }
}

fn selected_bits(on: bool) -> u32 {
    if on {
        LVIS_SELECTED.0
    } else {
        0
    }
}

/// Set a row's tick and highlight, but only when they are not already
/// right. Reading first keeps the diff from firing an `LVN_ITEMCHANGED`
/// per row per keystroke, which the suppression guard would swallow but
/// which comctl32 still has to raise.
unsafe fn set_item_state(list: HWND, i: usize, marked: bool, selected: bool) {
    let mask = LVIS_STATEIMAGEMASK.0 | LVIS_SELECTED.0;
    let want = check_bits(marked) | selected_bits(selected);
    let cur = SendMessageW(
        list,
        LVM_GETITEMSTATE,
        Some(WPARAM(i)),
        Some(LPARAM(mask as isize)),
    )
    .0 as u32
        & mask;
    if cur == want {
        return;
    }
    let it = LVITEMW {
        state: LIST_VIEW_ITEM_STATE_FLAGS(want),
        stateMask: LIST_VIEW_ITEM_STATE_FLAGS(mask),
        ..Default::default()
    };
    SendMessageW(
        list,
        LVM_SETITEMSTATE,
        Some(WPARAM(i)),
        Some(LPARAM(&it as *const _ as isize)),
    );
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
            WM_GETMINMAXINFO => {
                // A frame promise, not an arithmetic one -- Step 2 clamps
                // independently, because a floor does not make subtraction
                // safe, it only makes it unlikely.
                let dpi = GetDpiForWindow(hwnd).max(96);
                let mm = &mut *(lp.0 as *mut MINMAXINFO);
                mm.ptMinTrackSize.x = scale(MIN_WIDTH, dpi);
                mm.ptMinTrackSize.y = scale(MIN_HEIGHT, dpi);
                LRESULT(0)
            }
            WM_SIZE => {
                layout(hwnd);
                LRESULT(0)
            }
            WM_DPICHANGED => {
                // HIWORD(wParam) is the new DPI; lParam is a RECT with the
                // position and size Windows wants. Ignoring lParam leaves
                // the window the wrong size on the new monitor, and no
                // second message arrives to correct it.
                let dpi = ((wp.0 >> 16) & 0xFFFF) as u32;
                let font = ui_font(dpi);
                let old = UI.with(|u| {
                    u.borrow_mut().as_mut().map(|ui| {
                        let prev = ui.font;
                        ui.font = font;
                        prev
                    })
                });
                // Every child must be told, including ones `layout` places
                // through GetDlgItem rather than a stored handle.
                let mut child = GetWindow(hwnd, GW_CHILD).unwrap_or_default();
                while !child.is_invalid() {
                    SendMessageW(
                        child,
                        WM_SETFONT,
                        Some(WPARAM(font.0 as usize)),
                        Some(LPARAM(1)),
                    );
                    child = GetWindow(child, GW_HWNDNEXT).unwrap_or_default();
                }
                // If `UI` is somehow absent, `font` was never stored above,
                // so free it here instead of leaking it -- practically
                // unreachable (`UI` is populated in WM_CREATE before any
                // other message can arrive), but cheap to close.
                let _ = DeleteObject(HGDIOBJ(old.unwrap_or(font).0));
                // The ListView column widths are set once at creation
                // (LVM_INSERTCOLUMNW) and never touched again otherwise --
                // without this the headers stay at the old DPI's physical
                // width forever, clipping as soon as the font grows.
                let list = UI.with(|u| u.borrow().as_ref().map(|ui| ui.list));
                if let Some(list) = list {
                    for (i, (_, cx)) in LIST_COLUMNS.iter().enumerate() {
                        SendMessageW(
                            list,
                            LVM_SETCOLUMNWIDTH,
                            Some(WPARAM(i)),
                            Some(LPARAM(scale(*cx, dpi) as isize)),
                        );
                    }
                }
                let rc = &*(lp.0 as *const RECT);
                let _ = SetWindowPos(
                    hwnd,
                    None,
                    rc.left,
                    rc.top,
                    rc.right - rc.left,
                    rc.bottom - rc.top,
                    SWP_NOZORDER | SWP_NOACTIVATE,
                );
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
                // Every WM_COMMAND arm asks this; this one did not, and that
                // becomes fatal the moment `apply_state` writes item state.
                // `LVM_SETITEMSTATE` makes comctl32 fire LVN_ITEMCHANGED
                // SYNCHRONOUSLY, inside `apply_state` -- so the chain
                // apply_state -> on_select -> refresh_settings -> apply_state
                // recurses without bound across an `extern "system"`
                // boundary, where a second RefCell borrow ABORTS the process
                // instead of unwinding. Landing 2a writes item state for the
                // first time, so this guard has to exist before any of it.
                if suppressed() {
                    return LRESULT(0);
                }
                let nm = &*(lp.0 as *const NMHDR);
                if nm.idFrom == IDC_LIST as usize && nm.code == LVN_ITEMCHANGED {
                    let lv = &*(lp.0 as *const NMLISTVIEW);
                    // iItem is -1 on the notifications that speak for the
                    // whole list rather than one row; `as usize` would turn
                    // that into an index no model has, and `set_marked`
                    // indexes straight into `rows`.
                    if lv.iItem >= 0 {
                        let i = lv.iItem as usize;
                        // A tick and a selection both arrive as LVIF_STATE
                        // and `uChanged` cannot tell them apart, so the two
                        // bits are tested independently. Never `else if`:
                        // clicking an unselected row's box changes both in
                        // ONE message, and an `else if` drops whichever the
                        // arm did not reach.
                        let changed = lv.uOldState ^ lv.uNewState;
                        if changed & LVIS_STATEIMAGEMASK.0 != 0 {
                            let on = (lv.uNewState & LVIS_STATEIMAGEMASK.0) == LVIS_CHECKED;
                            with_cb(|cb| (cb.on_mark)(i, on));
                        }
                        if changed & LVIS_SELECTED.0 != 0 && lv.uNewState & LVIS_SELECTED.0 != 0 {
                            with_cb(|cb| (cb.on_select)(i));
                        }
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
