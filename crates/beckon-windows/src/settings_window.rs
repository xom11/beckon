//! The settings window: horizontal bands stacked top to bottom — an
//! external-change banner, a section head, the shortcut list, an editor
//! strip, the keyboard group, and a command bar. Win32 only — every
//! decision it draws comes from `beckon_core::settings::ControlState`, and
//! every edit it collects goes back out through `Callbacks`. This file
//! holds no policy.
//!
//! **Bands, not a split pane.** The 45/55 column split this replaced put
//! three fixed-width columns (34 + 190 + 150 = 561 px at 150 %) inside a
//! list pane 482 px wide, so beckon shipped a horizontal scroll bar and a
//! clipped App column. Widths are now a proportion of the live list width,
//! computed in `layout`, so that cannot recur — see the comment on
//! `LIST_COLUMNS`.
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
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM};
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Controls::*;
use windows::Win32::UI::HiDpi::{
    GetDpiForMonitor, GetDpiForWindow, GetSystemMetricsForDpi, SystemParametersInfoForDpi,
    MDT_EFFECTIVE_DPI,
};
use windows::Win32::UI::Input::KeyboardAndMouse::EnableWindow;
use windows::Win32::UI::WindowsAndMessaging::*;

/// `SS_LEFT` is 0 and `windows` 0.61 does not export it as a constant.
const SS_LEFT_STYLE: WINDOW_STYLE = WINDOW_STYLE(0);

/// `SS_CENTERIMAGE` (0x0200), which `windows` 0.61 does not export either.
/// On a STATIC holding text it centres that text vertically in the control
/// rect and clips it to one line — which is what lets a label share a band
/// line with controls taller than its own text instead of floating against
/// the top edge of it. Never on `IDC_NOTES`, which is deliberately several
/// lines tall.
const SS_CENTERIMAGE_STYLE: WINDOW_STYLE = WINDOW_STYLE(0x0200);

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
/// The `Shortcuts` heading in band 2. New ids go ABOVE the existing range:
/// 1001-1007 and the class name are hard-coded in
/// `examples/settings_probe.rs` and are fixed points.
const IDC_LBL_SECTION: i32 = 1020;

/// Layout tokens, at 96 DPI. Every one of them goes through `scale`.
///
/// Two need their reasoning, because they look like they contradict the
/// a14 measurements (`docs/superpowers/measurements/2026-08-11-landing-1-a14.md`)
/// and do not:
///
/// - **`CTL` is 32, not the measured 22.** `BCM_GETIDEALSIZE` returns the
///   smallest box the theme can draw a caption in — a floor, not a layout
///   recommendation. The measurement's job was to prove 32 does not clip,
///   and it does not.
/// - **There is no list-row token.** 29 px measured at 144 DPI is 19.33 at
///   96, and a non-integer is the tell that comctl32 derives the row
///   height from the font at the live DPI. A 96-DPI token pushed through
///   `scale` would be wrong at every non-integer scale and would break
///   again the moment the font changes, so `list_row_height` asks the
///   control instead.
mod tok {
    /// Surface padding — the margin between the client rect and content.
    pub const PAD: i32 = 16;
    /// Between two bands.
    pub const BAND: i32 = 14;
    /// Between two controls inside one band.
    pub const GAP: i32 = 8;
    /// A label and the control it names.
    pub const LABEL: i32 = 12;
    /// Height of one band line, and of every button on it.
    pub const CTL: i32 = 32;
    /// A button is never narrower than this, nor than its own caption.
    pub const BTN: i32 = 88;
    /// The right-aligned `Shortcut` column, and the editor field under it.
    pub const SHORTCUT_COL: i32 = 200;
    /// List rows visible without scrolling.
    pub const ROWS: i32 = 8;
}

/// ListView columns, in order: title and text alignment.
///
/// **Widths are deliberately absent.** They are a proportion of the live
/// list width, computed once per `layout` from the control's own client
/// rect minus a scroll bar — which is what makes the §A.3 overflow
/// (561 px of columns inside a 482 px list) structurally impossible rather
/// than merely unlikely. Putting a width back here would reintroduce it.
///
/// `App` is column 0 and must stay left-aligned: comctl32 forces
/// `LVCFMT_LEFT` on subitem 0 of a report view whatever is asked for, so
/// only a later column can carry `LVCFMT_RIGHT`. Column 0 is also where
/// `LVS_EX_CHECKBOXES` puts the tick, which is a state image and not a
/// column — it survived the status column's deletion untouched.
const LIST_COLUMNS: [(&str, LVCOLUMNW_FORMAT); 2] =
    [("App", LVCFMT_LEFT), ("Shortcut", LVCFMT_RIGHT)];

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
// 860 is spec B.2's stated width. 640 is the height raised from 560 so the
// notes band (the flex band -- see `layout`) fits four lines at 96 DPI: the
// band gets `kb_y`'s leftover directly, so every pixel added here becomes a
// pixel of notes room. Worked through in the task-6 report, section on
// fix 2: at the default size the notes band comes out to ~173 px against a
// ~19-21 px line height, i.e. 8+ lines against a 4-line requirement.
const WINDOW_WIDTH: i32 = 860;
const WINDOW_HEIGHT: i32 = 640;

/// Minimum resize size, at 96 DPI, enforced in `WM_GETMINMAXINFO`. Smaller
/// than `WINDOW_WIDTH`/`WINDOW_HEIGHT` so the window can be shrunk, but not
/// below the point where `layout` starts overlapping controls.
const MIN_WIDTH: i32 = 720;
const MIN_HEIGHT: i32 = 460;

/// One of §B.3's three type roles. There is no fourth: the `Keys` role the
/// spec table also lists belongs to keycap rendering, which this window
/// does not do -- combos are typed as text into an ordinary EDIT.
#[derive(Clone, Copy)]
enum Role {
    Subtitle,
    Body,
    Caption,
}

/// Which role a control takes, keyed on its id.
///
/// **This is the single mapping**, consulted by the creation path (`child`,
/// which every control in the window goes through) and by the
/// `WM_DPICHANGED` rebroadcast (which walks `GW_HWNDNEXT` and asks
/// `GetDlgCtrlID`). Those two must not each hold an opinion, for the same
/// reason `cells()` is the one funnel for column text: the second copy is
/// the one that drifts.
fn role_of(id: i32) -> Role {
    match id {
        // The one band heading. Subtitle exists so the list reads as a
        // section of the window rather than as the whole of it.
        IDC_LBL_SECTION => Role::Subtitle,
        // Secondary prose, and the only thing at Caption size. The banner
        // is deliberately NOT here: it announces that the file moved under
        // us, which is the least appropriate text in the window to shrink.
        IDC_NOTES => Role::Caption,
        // Everything the user reads or operates: the ListView, the shortcut
        // EDIT, the App COMBOBOX, their labels, every BUTTON (push, check,
        // radio, and the group box), the banner -- and anything added later
        // that does not say otherwise.
        _ => Role::Body,
    }
}

/// The three live `HFONT`s. `Copy`, so `LayoutHandles` stays `Copy` and the
/// abort-class rule below keeps holding.
#[derive(Clone, Copy)]
struct Fonts {
    subtitle: HFONT,
    body: HFONT,
    caption: HFONT,
}

impl Fonts {
    fn get(self, role: Role) -> HFONT {
        match role {
            Role::Subtitle => self.subtitle,
            Role::Body => self.body,
            Role::Caption => self.caption,
        }
    }

    fn for_id(self, id: i32) -> HFONT {
        self.get(role_of(id))
    }

    /// Release all three.
    ///
    /// Only ever called AFTER the controls have been told about their
    /// replacements -- deleting a font that is still selected into a DC is
    /// undefined. Landing 1 established this discipline for one font
    /// because one `HFONT` was leaking per window open; three roles means
    /// three leaks if only one of them is freed.
    ///
    /// Deduplicated because the total-failure path hands every role the
    /// same stock handle. `DeleteObject` on a stock object is documented
    /// harmless, but "harmless twice" is not a property worth relying on.
    unsafe fn delete(self) {
        let all = [self.subtitle, self.body, self.caption];
        for (i, f) in all.iter().enumerate() {
            if f.is_invalid() || all[..i].iter().any(|p| p.0 == f.0) {
                continue;
            }
            let _ = DeleteObject(HGDIOBJ(f.0));
        }
    }
}

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
    /// The three type roles, rebuilt on every `WM_DPICHANGED` and freed on
    /// `WM_DESTROY`. Which control uses which is `role_of`'s answer, never
    /// a decision taken at a call site.
    fonts: Fonts,
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

/// Everything `layout` needs out of `Ui`, copied in ONE borrow that is
/// dropped before a single `SendMessageW` or `SetWindowPos` runs.
///
/// This is not tidiness. A second `RefCell` borrow taken across an
/// `extern "system"` boundary — and every one of those calls can re-enter
/// this window's wndproc — ABORTS the process rather than unwinding, so it
/// shows up as neither a panic nor a test failure nor anything a
/// cross-compile can catch. Copying the handles out first makes it
/// unrepresentable.
#[derive(Clone, Copy)]
struct LayoutHandles {
    list: HWND,
    combo: HWND,
    app: HWND,
    notes: HWND,
    banner: HWND,
    reload: HWND,
    keep: HWND,
    fonts: Fonts,
    external_change: bool,
}

impl LayoutHandles {
    fn of(ui: &Ui) -> Self {
        Self {
            list: ui.list,
            combo: ui.combo,
            app: ui.app,
            notes: ui.notes,
            banner: ui.banner,
            reload: ui.reload,
            keep: ui.keep,
            fonts: ui.fonts,
            external_change: ui.external_change,
        }
    }
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

/// The severity prefix on a line of the notes STATIC. Not the list: rows
/// carry `ListItem::flag` beside the app name now (see `app_cell`), and a
/// healthy row says nothing at all rather than `OK`.
fn mark_glyph(m: Mark) -> &'static str {
    // ASCII on purpose: the notes carry a Segoe UI Variable text face, or
    // the shell's own on the fallback path, and neither is a symbol font --
    // a missing glyph shows as a box that reads like a rendering bug rather
    // than a status. (Segoe Fluent Icons IS installed, measured on a14, but
    // spec B.5 defers those glyphs to the NM_CUSTOMDRAW pass that can give
    // them their own font.) All four are two columns wide so the note lines
    // line up -- the trailing space on `Warn` is load-bearing, not a typo.
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

/// The shell's own `lfMessageFont` at a specific DPI: the base every role's
/// `LOGFONT` is derived from, and the face all three fall back to.
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
///
/// Measured on a14 2026-08-11: this returns plain `Segoe UI`, weight 400,
/// at `lfHeight = -12`. NOT Segoe UI Variable -- that reaches the shell
/// through DirectWrite and XAML, never through `NONCLIENTMETRICS`, so a
/// Win32 app has to ask for it by name (`build_fonts`).
unsafe fn message_logfont(dpi: u32) -> LOGFONTW {
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
        return ncm.lfMessageFont;
    }
    // Describe the stock GUI font rather than hand back a zeroed LOGFONT: a
    // zeroed one asks the mapper for "any face at any size", which is how
    // you land on the 1995 bitmap font this path exists to avoid. GetObjectW
    // can itself fail -- unreached today, since both SystemParametersInfo
    // calls above already have to fail first -- and an unchecked failure
    // here hands back exactly the zeroed LOGFONTW this comment says never
    // to return, silently. Check it and say so, rather than let the two
    // disagree.
    let mut lf = LOGFONTW::default();
    let got = GetObjectW(
        GetStockObject(DEFAULT_GUI_FONT),
        std::mem::size_of::<LOGFONTW>() as i32,
        Some(&mut lf as *mut _ as *mut _),
    );
    if got == 0 && beckon_core::verbose() {
        eprintln!(
            "verbose: settings window: GetObjectW(DEFAULT_GUI_FONT) failed \
             -- both SystemParametersInfo calls already failed too, so this \
             LOGFONTW is zeroed and every role will ask the mapper for \
             \"any face at any size\""
        );
    }
    lf
}

/// Write `face` into `lf.lfFaceName`, or report that it does not fit.
///
/// `lfFaceName` is 32 wchars INCLUDING the NUL, so 31 characters is the
/// ceiling -- and a silent truncation there is the exact trap §B.3 records.
/// `Segoe UI Variable Display Semibold` truncated to `Segoe UI Variable
/// Display Semib` names nothing, and GDI answers a name that names nothing
/// with **Arial**, not with an error. Refusing to truncate means no future
/// edit can reintroduce that by accident; `face_matches` is the second net
/// under it.
fn set_face(lf: &mut LOGFONTW, face: &str) -> bool {
    let name: Vec<u16> = face.encode_utf16().collect();
    if name.len() >= lf.lfFaceName.len() {
        return false;
    }
    lf.lfFaceName = [0; 32];
    lf.lfFaceName[..name.len()].copy_from_slice(&name);
    true
}

/// Did GDI actually hand back the face we asked for?
///
/// **`CreateFontIndirectW` never fails on an unknown name** -- the font
/// mapper substitutes silently, so a successful create proves nothing.
/// Measured on a14 2026-08-11: asking for `Segoe UI Variable Text Semib`
/// returned `Arial`, exactly as a `This Font Does Not Exist` control did.
/// The only way to know is to select the font into a DC and read back what
/// the DC now holds.
unsafe fn face_matches(hwnd: HWND, font: HFONT, want: &str) -> bool {
    let dc = GetDC(Some(hwnd));
    if dc.is_invalid() {
        // `make_font` treats this exactly like the face genuinely being
        // absent -- same fallback, same silence otherwise. Log it so a
        // transient GetDC failure and a missing face read differently in a
        // log instead of both showing up as "role fell back" with no trace
        // of which cause it was.
        if beckon_core::verbose() {
            eprintln!(
                "verbose: settings window: GetDC failed while checking for \
                 {want} -- falling back to the shell face this time, not \
                 because it is absent"
            );
        }
        return false;
    }
    let prev = SelectObject(dc, HGDIOBJ(font.0));
    // LF_FACESIZE is 32; the slack costs nothing and removes the question of
    // whether the returned count includes the terminator.
    let mut buf = [0u16; 64];
    let n = GetTextFaceW(dc, Some(&mut buf));
    if !prev.is_invalid() {
        SelectObject(dc, prev);
    }
    ReleaseDC(Some(hwnd), dc);
    if n <= 0 {
        return false;
    }
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..end]).eq_ignore_ascii_case(want)
}

/// One role's font: the shell's `lfMessageFont` with this role's face,
/// pixel height and weight -- and the requested face only when GDI really
/// produced it.
///
/// **`px` is a PIXEL height**, applied as a negative `lfHeight` (character
/// height, the usual GDI convention). Our own measurement is the
/// corroboration: a14 reported `lfMessageFont.lfHeight = -12`, so the shell
/// font is exactly the Caption size. Read as points, Body would be 14 pt --
/// larger than any shell UI and inconsistent with that baseline.
///
/// The fallback keeps this role's SIZE and WEIGHT and gives up only the
/// face, because size and weight are the hierarchy. Segoe UI ships a
/// Semibold, so even a fallen-back Subtitle stays heavier than the Body
/// around it.
unsafe fn make_font(
    hwnd: HWND,
    base: &LOGFONTW,
    face: &str,
    px: i32,
    weight: i32,
    dpi: u32,
) -> HFONT {
    let mut spec = *base;
    spec.lfHeight = -scale(px, dpi);
    // The height is ours now, so the base's paired width would stretch the
    // glyphs; 0 asks the mapper for the face's own aspect ratio.
    spec.lfWidth = 0;
    spec.lfWeight = weight;

    let mut want = spec;
    if set_face(&mut want, face) {
        let f = CreateFontIndirectW(&want);
        if !f.is_invalid() {
            if face_matches(hwnd, f, face) {
                return f;
            }
            // A real handle, ours to free -- just the wrong font in it.
            let _ = DeleteObject(HGDIOBJ(f.0));
        }
    }

    let f = CreateFontIndirectW(&spec);
    if !f.is_invalid() {
        return f;
    }
    HFONT(GetStockObject(DEFAULT_GUI_FONT).0)
}

/// The three type roles of §B.3, built for `dpi`.
///
/// | Role | Size | Weight | Used for |
/// |---|---|---|---|
/// | Subtitle | 20 px | semibold | band headings |
/// | Body | 14 px | regular | list, fields, buttons |
/// | Caption | 12 px | regular | notes |
///
/// **The face names are spelled in full, from the a14 measurement.**
/// `Segoe UI Variable Text Semibold` is exactly 31 characters and survives
/// `lfFaceName` intact; the Display and Small semibolds do not, which is
/// why the family here is Text rather than whichever optical size a naive
/// truncation happens to leave valid. `Segoe UI Variable Text` / `Small` /
/// `Display` were all confirmed present and exact.
///
/// Optical size is why Body and Caption differ at all: Segoe UI Variable
/// ships Small for caption sizes, Text for body and headings up to ~30 px,
/// Display above that. 20 px is Text territory, not Display's.
unsafe fn build_fonts(hwnd: HWND, dpi: u32) -> Fonts {
    let base = message_logfont(dpi);
    Fonts {
        subtitle: make_font(
            hwnd,
            &base,
            "Segoe UI Variable Text Semibold",
            20,
            FW_SEMIBOLD.0 as i32,
            dpi,
        ),
        body: make_font(
            hwnd,
            &base,
            "Segoe UI Variable Text",
            14,
            FW_NORMAL.0 as i32,
            dpi,
        ),
        caption: make_font(
            hwnd,
            &base,
            "Segoe UI Variable Small",
            12,
            FW_NORMAL.0 as i32,
            dpi,
        ),
    }
}

unsafe fn child(
    parent: HWND,
    class: PCWSTR,
    text: &str,
    style: WINDOW_STYLE,
    id: i32,
    fonts: &Fonts,
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
    // The role comes from the id, through the SAME `role_of` the
    // `WM_DPICHANGED` rebroadcast consults. One mapping, two call sites --
    // if creation and that broadcast each carried their own idea of which
    // control gets which font, a walk across monitors would silently
    // re-role half the window.
    let font = fonts.for_id(id);
    SendMessageW(
        h,
        WM_SETFONT,
        Some(WPARAM(font.0 as usize)),
        Some(LPARAM(1)),
    );
    h
}

/// Create every child, **in the order it is drawn**.
///
/// Creation order is Tab order, and that is the whole reason this function
/// reads top-to-bottom. The banner's `Reload` / `Keep mine` used to be
/// created last: the one pair that answers an urgent event — the file moved
/// under us — sat at the end of the Tab order, behind everything it
/// interrupts.
///
/// Every control leaves here already carrying its role's font, which is why
/// `WM_CREATE` can call `layout` immediately afterwards: comctl32 derives
/// the ListView row height from the control's font, and `layout` QUERIES
/// that height rather than assuming it.
unsafe fn build_children(hwnd: HWND) {
    let dpi = GetDpiForWindow(hwnd).max(96);
    let fonts = build_fonts(hwnd, dpi);

    // -- Band 1: the external-change banner. Hidden until `apply_state`
    // says the file moved; `layout` gives it no height at all while it is
    // hidden, so the bands below close up rather than leaving a gap.
    let banner = child(
        hwnd,
        w!("STATIC"),
        "This file changed on disk.",
        SS_CENTERIMAGE_STYLE,
        IDC_BANNER,
        &fonts,
    );
    let reload = child(
        hwnd,
        w!("BUTTON"),
        "Reload",
        WINDOW_STYLE(BS_PUSHBUTTON as u32) | WS_TABSTOP,
        IDC_RELOAD,
        &fonts,
    );
    let keep = child(
        hwnd,
        w!("BUTTON"),
        "Keep mine",
        WINDOW_STYLE(BS_PUSHBUTTON as u32) | WS_TABSTOP,
        IDC_KEEPMINE,
        &fonts,
    );
    show(banner, false);
    show(reload, false);
    show(keep, false);

    // -- Band 2: the section head.
    //
    // **No filter control, and no placeholder for one.** `on_select(i)` and
    // `on_mark(i)` index `m.rows` DIRECTLY, so the moment the list shows a
    // filtered subset those callbacks address the wrong row -- ticking one
    // binding and deleting another. It lands together with the
    // view-index-to-model-index mapping that makes it safe, not before.
    child(
        hwnd,
        w!("STATIC"),
        "Shortcuts",
        SS_CENTERIMAGE_STYLE,
        IDC_LBL_SECTION,
        &fonts,
    );
    child(
        hwnd,
        w!("BUTTON"),
        "Remove",
        WINDOW_STYLE(BS_PUSHBUTTON as u32) | WS_TABSTOP,
        IDC_REMOVE,
        &fonts,
    );
    child(
        hwnd,
        w!("BUTTON"),
        "Add",
        WINDOW_STYLE(BS_PUSHBUTTON as u32) | WS_TABSTOP,
        IDC_ADD,
        &fonts,
    );

    // -- Band 3: the list.
    let list = child(
        hwnd,
        w!("SysListView32"),
        "",
        WINDOW_STYLE(LVS_REPORT | LVS_SINGLESEL | LVS_SHOWSELALWAYS | LVS_NOSORTHEADER)
            | WS_BORDER
            | WS_TABSTOP,
        IDC_LIST,
        &fonts,
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
    // No LVCF_WIDTH: `layout` owns every column width, so there is exactly
    // one place a column can be made too wide for its list.
    for (i, (title, fmt)) in LIST_COLUMNS.iter().enumerate() {
        let mut t = wide(title);
        let col = LVCOLUMNW {
            mask: LVCF_TEXT | LVCF_FMT | LVCF_SUBITEM,
            fmt: *fmt,
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

    // -- Band 4: the editor strip. App first, then the shortcut, mirroring
    // the row above it (B.1: "laid out to mirror a row").
    child(
        hwnd,
        w!("STATIC"),
        "App",
        SS_CENTERIMAGE_STYLE,
        IDC_LBL_APP,
        &fonts,
    );
    // CBS_DROPDOWN, not CBS_DROPDOWNLIST: beckon deliberately supports apps
    // with no Start Menu entry, so free typing must stay possible even once
    // the catalog has loaded.
    let app = child(
        hwnd,
        w!("COMBOBOX"),
        "",
        WINDOW_STYLE((CBS_DROPDOWN | CBS_AUTOHSCROLL | CBS_SORT) as u32) | WS_VSCROLL | WS_TABSTOP,
        IDC_APP,
        &fonts,
    );
    // Under comctl32 v6 the `cy` passed to SetWindowPos no longer decides
    // how tall the drop-down is; this does. Without it the list opens at
    // the default 30 items regardless of the height layout computes.
    SendMessageW(app, CB_SETMINVISIBLE, Some(WPARAM(8)), Some(LPARAM(0)));
    child(
        hwnd,
        w!("STATIC"),
        "Shortcut",
        SS_CENTERIMAGE_STYLE,
        IDC_LBL_SHORTCUT,
        &fonts,
    );
    let combo = child(
        hwnd,
        w!("EDIT"),
        "",
        WINDOW_STYLE(ES_AUTOHSCROLL as u32) | WS_BORDER | WS_TABSTOP,
        IDC_COMBO,
        &fonts,
    );
    // On its own line directly beneath the strip, which is where B.1's
    // mock-up draws it. Several lines tall, so no SS_CENTERIMAGE.
    let notes = child(hwnd, w!("STATIC"), "", SS_LEFT_STYLE, IDC_NOTES, &fonts);

    // -- Band 5: the suggestion row. Nothing is created for it and it
    // contributes zero height. A placeholder would be a control to keep in
    // sync with a feature that does not exist yet.

    // -- Band 6: the keyboard group, directly above the command bar. F.8
    // replaces this with a one-line Caps row at the TOP of the window, but
    // that is the next landing and it is gated on measurements not taken.
    child(
        hwnd,
        w!("BUTTON"),
        "Keyboard",
        WINDOW_STYLE(BS_GROUPBOX as u32),
        IDC_GRP_KEYBOARD,
        &fonts,
    );
    child(
        hwnd,
        w!("BUTTON"),
        "Use Caps Lock as the beckon key",
        WINDOW_STYLE(BS_AUTOCHECKBOX as u32) | WS_TABSTOP,
        IDC_CAPS,
        &fonts,
    );
    child(
        hwnd,
        w!("BUTTON"),
        "Tapping Caps alone: Caps Lock",
        WINDOW_STYLE(BS_AUTORADIOBUTTON as u32) | WS_GROUP | WS_TABSTOP,
        IDC_TAP_CAPSLOCK,
        &fonts,
    );
    child(
        hwnd,
        w!("BUTTON"),
        "Esc",
        WINDOW_STYLE(BS_AUTORADIOBUTTON as u32),
        IDC_TAP_ESCAPE,
        &fonts,
    );
    child(
        hwnd,
        w!("BUTTON"),
        "nothing",
        WINDOW_STYLE(BS_AUTORADIOBUTTON as u32),
        IDC_TAP_NONE,
        &fonts,
    );

    // -- Band 7: the command bar. `Open config file` far left, then Close
    // and Apply on the right.
    //
    // WS_GROUP terminates the radio group above it. IDC_TAP_CAPSLOCK opens
    // a group and nothing closed it, so Right/Down from `nothing` used to
    // walk focus straight out of the group and into whatever was created
    // next -- which, before this reordering, was the hidden banner.
    //
    // Captions are unchanged on purpose: renaming Apply to Save, the
    // mnemonics and the accelerator table are the next task.
    child(
        hwnd,
        w!("BUTTON"),
        "Open config file",
        WINDOW_STYLE(BS_PUSHBUTTON as u32) | WS_GROUP | WS_TABSTOP,
        IDC_OPENFILE,
        &fonts,
    );
    child(
        hwnd,
        w!("BUTTON"),
        "Close",
        WINDOW_STYLE(BS_PUSHBUTTON as u32) | WS_TABSTOP,
        IDC_CLOSE,
        &fonts,
    );
    child(
        hwnd,
        w!("BUTTON"),
        "Apply",
        WINDOW_STYLE(BS_DEFPUSHBUTTON as u32) | WS_TABSTOP,
        IDC_APPLY,
        &fonts,
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
            fonts,
            suppress: false,
            external_change: false,
            items: Vec::new(),
        })
    });
}

// ---------------------------------------------------------------------------
// Layout
// ---------------------------------------------------------------------------

/// The size of `s` rendered in `font`, in physical pixels.
///
/// Widths are measured, never tabulated, so a button is never narrower
/// than its own caption and a label never overlaps the field beside it —
/// the defects B.2 records, all of which come from constants that were
/// right for one font at one DPI. B.3 has since given the window three
/// fonts, and this needed no change to survive it — but the CALLER now has
/// to pass the font of the role it is measuring FOR, not whichever handle
/// is nearest to hand, or a caption gets a box sized for a different face.
///
/// The estimate on the failure path is deliberately generous: too wide
/// costs a gap, too narrow clips.
unsafe fn text_size(hwnd: HWND, font: HFONT, dpi: u32, s: &str) -> (i32, i32) {
    let est = (
        scale(8, dpi) * s.chars().count() as i32,
        scale(16, dpi).max(1),
    );
    let dc = GetDC(Some(hwnd));
    if dc.is_invalid() {
        return est;
    }
    let prev = SelectObject(dc, HGDIOBJ(font.0));
    let text = wide(s);
    let mut sz = SIZE::default();
    // `wide` appends a NUL and this API takes a length, so the NUL would
    // be measured as a character.
    let ok = GetTextExtentPoint32W(dc, &text[..text.len() - 1], &mut sz).as_bool();
    if !prev.is_invalid() {
        SelectObject(dc, prev);
    }
    ReleaseDC(Some(hwnd), dc);
    if ok && sz.cy > 0 {
        (sz.cx, sz.cy)
    } else {
        est
    }
}

/// One ListView row, in physical pixels at the live DPI.
///
/// **Queried, never scaled from a token.** 29 px measured on a14 at 144 DPI
/// is 19.33 at 96, and a non-integer is the tell that comctl32 derives the
/// row height from the font rather than from a design constant — a 96-DPI
/// token pushed through `scale` would be wrong at every non-integer scale
/// and would go wrong again the moment B.3 changes the font.
///
/// `LVM_GETITEMRECT` needs a row to measure. When the list is empty there
/// is none, and the fallback barely matters: an empty list has no rows to
/// show, and `apply_state` calls `layout` again the instant it puts one in.
unsafe fn list_row_height(list: HWND, dpi: u32) -> i32 {
    let count = SendMessageW(list, LVM_GETITEMCOUNT, Some(WPARAM(0)), Some(LPARAM(0))).0;
    if count > 0 {
        // `left` is the input: which of the item's rectangles is wanted.
        let mut rc = RECT {
            left: LVIR_BOUNDS as i32,
            ..Default::default()
        };
        let got = SendMessageW(
            list,
            LVM_GETITEMRECT,
            Some(WPARAM(0)),
            Some(LPARAM(&mut rc as *mut RECT as isize)),
        )
        .0 != 0;
        let h = rc.bottom - rc.top;
        if got && h > 0 {
            return h;
        }
    }
    scale(20, dpi)
}

/// The ListView's header, in physical pixels at the live DPI. Measured 31
/// at 144 DPI, which is 20.67 at 96 — a non-integer for the same reason a
/// row is, so it is asked for rather than tabulated.
unsafe fn list_header_height(list: HWND, dpi: u32) -> i32 {
    let hdr = HWND(SendMessageW(list, LVM_GETHEADER, Some(WPARAM(0)), Some(LPARAM(0))).0 as *mut _);
    if !hdr.is_invalid() {
        let mut rc = RECT::default();
        if GetWindowRect(hdr, &mut rc).is_ok() {
            let h = rc.bottom - rc.top;
            if h > 0 {
                return h;
            }
        }
    }
    scale(21, dpi)
}

/// Set a column's width, but only when it is not already right.
///
/// `apply_state` calls `layout`, and `apply_state` runs on every keystroke,
/// so this write happens per keystroke — and a width write invalidates the
/// header whether or not the number changed. Reading first is the same
/// guard, for the same reason, as the one on `set_item_state`.
unsafe fn set_column_width(list: HWND, col: usize, cx: i32) {
    let cur = SendMessageW(list, LVM_GETCOLUMNWIDTH, Some(WPARAM(col)), Some(LPARAM(0))).0;
    if cur == cx as isize {
        return;
    }
    SendMessageW(
        list,
        LVM_SETCOLUMNWIDTH,
        Some(WPARAM(col)),
        Some(LPARAM(cx as isize)),
    );
}

/// Seven horizontal bands, top to bottom: the external-change banner (no
/// height when hidden), the section head, the list, the editor strip, the
/// suggestion row (no control, no height, in this landing), the keyboard
/// group and the command bar.
///
/// Everything is placed from the client rect at the current DPI, so a
/// 150 % display is not an afterthought — `GetDpiForWindow` scales the
/// tokens rather than the tokens assuming 96.
///
/// **Vertical shape.** The command bar is anchored to the bottom and the
/// keyboard group sits directly above it; the top bands stack downward.
/// The one thing that flexes is the notes STATIC between them, so a resize
/// lands there. The list wants `header + 8 rows` and gives that up rather
/// than let anything overlap when the window is short — a shrunk list
/// scrolls, an overlapped control is unreachable.
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
    // never see or focus again. Widths need it as much as heights: WM_SIZE
    // fires with a 0x0 client rect on minimize (ptMinTrackSize only
    // constrains dragging, not that), so `w` is 0 here on every minimize,
    // on every machine, and every subtraction below goes negative without
    // it.
    let clamp = |v: i32| v.max(0);

    // ONE borrow of UI, taken here and dropped on this line. Nothing below
    // may hold one: every SetWindowPos and SendMessageW that follows can
    // re-enter this window's wndproc, and a second borrow across an
    // `extern "system"` boundary aborts the process instead of unwinding.
    let Some(ui) = UI.with(|u| u.borrow().as_ref().map(LayoutHandles::of)) else {
        return;
    };

    let pad = s(tok::PAD);
    let band = s(tok::BAND);
    let gap = s(tok::GAP);
    let lblgap = s(tok::LABEL);
    let ctl = s(tok::CTL);

    let w = rc.right - rc.left;
    let h = rc.bottom - rc.top;
    let cx = pad;
    let cw = clamp(w - pad * 2);

    // Body, and only Body: every string measured in this function labels or
    // captions a Body control -- the three command-bar buttons, Add /
    // Remove / Reload / Keep mine, the two field labels, the three radios,
    // and the "Ag" that sizes the EDIT. The `Shortcuts` heading is the one
    // Subtitle in the window and its width is never measured; it takes
    // whatever Add and Remove leave it.
    let tw = |t: &str| text_size(hwnd, ui.fonts.get(Role::Body), dpi, t).0;
    let btn = |t: &str| s(tok::BTN).max(tw(t) + s(24));

    let place = |id: i32, x: i32, y: i32, cxx: i32, cy: i32| {
        if let Ok(c) = GetDlgItem(Some(hwnd), id) {
            let _ = SetWindowPos(c, None, x, y, cxx, cy, SWP_NOZORDER | SWP_NOACTIVATE);
        }
    };
    let place_h = |h_: HWND, x: i32, y: i32, cxx: i32, cy: i32| {
        let _ = SetWindowPos(h_, None, x, y, cxx, cy, SWP_NOZORDER | SWP_NOACTIVATE);
    };

    // The two bottom bands are anchored, not stacked, so the window's
    // bottom edge is where they stay however tall the content above is.
    let bar_y = clamp(h - pad - ctl);
    // Caption inset, then two control lines with a gap, then a bottom
    // inset the same size as the gap.
    let kb_h = s(24) + ctl * 2 + gap * 2;
    let kb_y = clamp(bar_y - band - kb_h);

    let mut y = pad;

    // -- Band 1: the banner. Contributes NO height when hidden.
    if ui.external_change {
        let bw_reload = btn("Reload");
        let bw_keep = btn("Keep mine");
        let buttons = bw_reload + gap + bw_keep;
        place_h(ui.banner, cx, y, clamp(cw - buttons - gap), ctl);
        place_h(ui.reload, cx + clamp(cw - buttons), y, bw_reload, ctl);
        place_h(ui.keep, cx + clamp(cw - bw_keep), y, bw_keep, ctl);
        y += ctl + band;
    }

    // -- Band 2: the section head. `Shortcuts` leading, Remove and Add
    // right-aligned. No filter control -- see `build_children`.
    let bw_add = btn("Add");
    let bw_remove = btn("Remove");
    place(IDC_ADD, cx + clamp(cw - bw_add), y, bw_add, ctl);
    place(
        IDC_REMOVE,
        cx + clamp(cw - bw_add - gap - bw_remove),
        y,
        bw_remove,
        ctl,
    );
    place(
        IDC_LBL_SECTION,
        cx,
        y,
        clamp(cw - bw_add - gap - bw_remove - gap),
        ctl,
    );
    // A control gap, not a band gap: the head labels the list directly
    // below it, so the two read as one group.
    y += ctl + gap;

    // -- Band 3: the list.
    let row_h = list_row_height(ui.list, dpi);
    // `want` is a WINDOW height (it feeds SetWindowPos below), but the list
    // carries WS_BORDER, so its client height -- where header_height + 8
    // rows actually get drawn -- is 2*SM_CYBORDER less than that. Without
    // this the 8th row was clipped by the border and comctl32 drew a sliver
    // of a 9th.
    let border = 2 * GetSystemMetricsForDpi(SM_CYBORDER, dpi);
    let want = list_header_height(ui.list, dpi) + row_h * tok::ROWS + border;
    // The editor strip below needs its own line plus at least one line of
    // notes; the list yields its fixed height before anything overlaps.
    let editor_min = ctl + gap + ctl;
    let room = clamp(kb_y - band - y);
    let list_h = clamp(want.min(clamp(room - band - editor_min)));
    place_h(ui.list, cx, y, cw, list_h);

    // Columns, sized from the list's OWN client width now that it has one,
    // minus a vertical scroll bar's width whether or not one is showing.
    // That subtraction is what makes overflow structurally impossible: a
    // scroll bar appearing later steals client width the columns have
    // already been told not to use. Measured before this change: 561 px of
    // columns inside a 482 px list, i.e. a horizontal scroll bar shipped.
    let mut lrc = RECT::default();
    let inner = if GetClientRect(ui.list, &mut lrc).is_ok() {
        clamp(lrc.right - lrc.left - GetSystemMetricsForDpi(SM_CXVSCROLL, dpi))
    } else {
        0
    };
    // `Shortcut` never takes more than half, so `App` -- which leads, and
    // carries the tick and the flag -- can never be squeezed out.
    let col_shortcut = s(tok::SHORTCUT_COL).min(inner / 2);
    let col_app = clamp(inner - col_shortcut);
    set_column_width(ui.list, 0, col_app);
    set_column_width(ui.list, 1, col_shortcut);
    y += list_h + band;
    // `list_h` clamps to 0 when `room` itself clamped negative -- reachable
    // only by an intermediate resize below MIN_HEIGHT that WM_DPICHANGED's
    // suggested rect can hand us without asking WM_GETMINMAXINFO (dragging
    // can't reach it; a 0x0 client rect clamps everything to 0 and is fine).
    // In that state `y` here can still land past `kb_y`, and unlike the
    // list and the notes STATIC below, the strip's height is the fixed
    // `ctl`, not something `clamp` already shrinks -- so without this it
    // draws over the keyboard group box.
    y = y.min(kb_y);

    // -- Band 4: the editor strip, one line, then the notes beneath it.
    //
    // A single-line EDIT draws its text at the TOP of its client rect --
    // Win32 gives it no vertical centring at all -- so stretching one to
    // the 32 px band line would park the text against the top edge. The
    // two text fields therefore take a height the font justifies and are
    // centred within the line; the buttons, which do honour `cy` and look
    // right at 32, take the token directly.
    let field_h = (text_size(hwnd, ui.fonts.get(Role::Body), dpi, "Ag").1 + s(10)).min(ctl);
    let fy = y + clamp(ctl - field_h) / 2;
    // A hair of slack past the measured width: a STATIC clips to its rect,
    // and SS_CENTERIMAGE clips harder because it also refuses to wrap.
    let lw_app = tw("App") + s(4);
    let lw_short = tw("Shortcut") + s(4);
    // The shortcut field sits under the Shortcut column so the strip
    // mirrors a row. A third of the width is its ceiling on a narrow one.
    let field_w = s(tok::SHORTCUT_COL).min(clamp(cw / 3));
    let edit_x = cx + clamp(cw - field_w);
    let lbl_short_x = (edit_x - lblgap - lw_short).max(cx);
    let app_x = cx + lw_app + lblgap;
    let app_w = clamp(lbl_short_x - gap - app_x);

    place(IDC_LBL_APP, cx, y, lw_app, ctl);
    // A COMBOBOX's `cy` is the height of its DROPPED-DOWN list, not of the
    // closed control -- and under comctl32 v6 even that is capped by
    // `build_children`'s CB_SETMINVISIBLE(8). The closed height is the
    // system's to choose from the font, so ask what it took and centre THAT
    // in the line, rather than guessing a chrome delta the next font change
    // would invalidate.
    place_h(ui.app, app_x, fy, app_w, field_h * 9);
    let mut arc = RECT::default();
    if GetWindowRect(ui.app, &mut arc).is_ok() {
        let ah = arc.bottom - arc.top;
        if ah > 0 && ah < ctl {
            place_h(ui.app, app_x, y + (ctl - ah) / 2, app_w, field_h * 9);
        }
    }
    place(IDC_LBL_SHORTCUT, lbl_short_x, y, lw_short, ctl);
    place_h(ui.combo, edit_x, fy, field_w, field_h);
    y += ctl + gap;
    place_h(ui.notes, cx, y, cw, clamp(kb_y - band - y));

    // -- Band 5: the suggestion row. No control, no height.

    // -- Band 6: the keyboard group.
    place(IDC_GRP_KEYBOARD, cx, kb_y, cw, kb_h);
    let inner_x = cx + gap;
    let caps_y = kb_y + s(24);
    place(IDC_CAPS, inner_x, caps_y, clamp(cw - gap * 2), ctl);
    // Radio widths come from the captions. The s(190)/s(70)/s(90) these
    // replace were sized for one font at one DPI and clipped the moment
    // either changed.
    let ry = caps_y + ctl + gap;
    let rx = inner_x + gap;
    // The radio's own circle, plus the gap it leaves before its caption.
    let glyph = s(24);
    let w_caps = tw("Tapping Caps alone: Caps Lock") + glyph;
    let w_esc = tw("Esc") + glyph;
    let w_none = tw("nothing") + glyph;
    place(IDC_TAP_CAPSLOCK, rx, ry, w_caps, ctl);
    place(IDC_TAP_ESCAPE, rx + w_caps + gap, ry, w_esc, ctl);
    place(
        IDC_TAP_NONE,
        rx + w_caps + gap + w_esc + gap,
        ry,
        w_none,
        ctl,
    );

    // -- Band 7: the command bar.
    let bw_open = btn("Open config file");
    let bw_apply = btn("Apply");
    let bw_close = btn("Close");
    place(IDC_OPENFILE, cx, bar_y, bw_open, ctl);
    place(IDC_APPLY, cx + clamp(cw - bw_apply), bar_y, bw_apply, ctl);
    place(
        IDC_CLOSE,
        cx + clamp(cw - bw_apply - gap - bw_close),
        bar_y,
        bw_close,
        ctl,
    );
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
    vec![app_cell(it), it.combo.clone()]
}

/// The App column's text: the app name, and the row's flag beside it when
/// it has one.
///
/// **Appended to the cell, not a third column and not `NM_CUSTOMDRAW`.**
/// B.1 draws the flag inline beside the app name, B.2 names exactly two
/// columns, and B.5 is explicit that the Fluent glyphs come later "via
/// `NM_CUSTOMDRAW` as decoration over text that already works". This is
/// that text. It is produced inside the `cells` funnel so the rebuild path
/// and the diff path cannot disagree about it.
///
/// **The flag takes the list's Body font, and cannot take Caption.** B.3
/// puts flags at Caption size, but this text is part of the App CELL, and a
/// ListView draws a cell in the control's one font -- there is no
/// per-run font in a report view. Giving the flag its own would mean
/// `NM_CUSTOMDRAW`, which B.5 explicitly defers to a later pass. So this is
/// a deferral, not an oversight: it lands with the Fluent glyphs or not at
/// all.
///
/// ASCII, like `mark_glyph`, and for the same reason: the face here is a
/// text font, not a symbol one. A healthy row still says nothing at all --
/// `flag` is `None` and the name stands alone, which is the whole point of
/// deleting the status column that used to say `OK` on every row.
fn app_cell(it: &ListItem) -> String {
    match &it.flag {
        Some(f) => format!("{}   {}", it.app, f),
        None => it.app.clone(),
    }
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
        // into view, which would fight the scroll restore below. Consequence:
        // after any Add / Remove / reload, the first arrow key press jumps
        // to row 0 instead of continuing from the current selection.
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

    // The pair above restores the pre-rebuild scroll position, which is
    // right for a reload (`Model::from_text` leaves `st.selected` as
    // `None`, so this block does nothing there) but wrong for
    // `Model::add_row`, which always selects the newly appended last row:
    // on a list longer than one page, "restore the old top" leaves that
    // new, selected, empty row off-screen while the editor strip below is
    // already showing it -- and when the old top was 0 the pair above
    // skips entirely, so the row stays off-screen with nothing to fix it.
    // `LVM_ENSUREVISIBLE` is a no-op when the row is already fully on
    // screen, so this only moves the view when the restore above left the
    // selection outside it -- it never fights the restore for the reload
    // case.
    if let Some(sel) = st.selected {
        if (sel as isize) < count {
            ensure_visible(list, sel as isize);
        }
    }

    let _ = InvalidateRect(Some(list), None, false);
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

/// Shared tail for `WM_SYSCOLORCHANGE`, `WM_THEMECHANGED`, and
/// `WM_SETTINGCHANGE`(`SPI_SETHIGHCONTRAST`): forward the message verbatim
/// to every child, then invalidate and relayout.
///
/// The system delivers all three to TOP-LEVEL windows only. A themed common
/// control (the ListView, the group box) needs `WM_THEMECHANGED` itself to
/// reopen its theme handle, and none of them see it unless we pass it on --
/// so without this forwarding every control keeps rendering from stale
/// theme data after a theme switch or a high-contrast toggle, which is
/// exactly the path this window uses as its dark mode.
///
/// Same enumeration `WM_DPICHANGED` uses to rebroadcast `WM_SETFONT` -- one
/// funnel for "walk every child", not a second one invented here. Only
/// direct children: every control in this window is a sibling of `hwnd`,
/// same as the font rebroadcast relies on. Never sent to `hwnd` itself --
/// that would recurse into this wndproc.
///
/// No `UI` borrow is held across any of these sends: `GetWindow` /
/// `SendMessageW` / `InvalidateRect` don't touch the struct, and `layout`
/// takes and drops its own borrow before any of ITS sends (see the comment
/// at its top) -- the same discipline `WM_DPICHANGED` follows.
unsafe fn broadcast_theme_change(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) {
    let mut child = GetWindow(hwnd, GW_CHILD).unwrap_or_default();
    while !child.is_invalid() {
        SendMessageW(child, msg, Some(wp), Some(lp));
        child = GetWindow(child, GW_HWNDNEXT).unwrap_or_default();
    }
    let _ = InvalidateRect(Some(hwnd), None, true);
    // Relayout, not just repaint: high contrast and theme switches can
    // change the system metrics `layout` reads live -- ListView row height
    // (`list_row_height`, the same value WM_DPICHANGED's font swap already
    // invalidates), SM_CXVSCROLL / SM_CYBORDER, and control heights read
    // back through GetWindowRect. Those are exactly the metrics that move
    // when a user enters or leaves high contrast, and layout already
    // queries them at call time instead of assuming a constant -- staying
    // stale here would reintroduce the clipping bug that query was added to
    // fix. Rare, user-initiated events; a handful of extra SetWindowPos
    // calls is not a cost worth avoiding for it.
    layout(hwnd);
}

extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_CREATE => {
                // Font BEFORE geometry, and the order is load-bearing:
                // `build_children` leaves every control carrying its role's
                // font, and `layout` asks comctl32 for the ListView's row
                // height -- which comctl32 derives from that font. Placing
                // first would size the list against whatever the control
                // was born with.
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
                let fonts = build_fonts(hwnd, dpi);
                // The borrow is taken and dropped on these lines. Nothing
                // below may hold one: `WM_SETFONT` re-enters this wndproc,
                // and a second `RefCell` borrow across an `extern "system"`
                // boundary ABORTS the process rather than unwinding.
                let old = UI.with(|u| {
                    u.borrow_mut().as_mut().map(|ui| {
                        let prev = ui.fonts;
                        ui.fonts = fonts;
                        prev
                    })
                });
                // Every child must be told, including ones `layout` places
                // through GetDlgItem rather than a stored handle -- and
                // each must be told about ITS OWN role, read back from the
                // same `role_of` the creation path used. A single font
                // broadcast here would flatten the ramp on the first walk
                // between monitors.
                let mut child = GetWindow(hwnd, GW_CHILD).unwrap_or_default();
                while !child.is_invalid() {
                    let f = fonts.for_id(GetDlgCtrlID(child));
                    SendMessageW(
                        child,
                        WM_SETFONT,
                        Some(WPARAM(f.0 as usize)),
                        Some(LPARAM(1)),
                    );
                    child = GetWindow(child, GW_HWNDNEXT).unwrap_or_default();
                }
                // AFTER the broadcast, never before: the old handles were
                // selected into those controls until the loop above replaced
                // them, and deleting a font that is still selected is
                // undefined.
                //
                // If `UI` is somehow absent, `fonts` was never stored above,
                // so free THAT instead of leaking three -- practically
                // unreachable (`UI` is populated in WM_CREATE before any
                // other message can arrive), but cheap to close.
                old.unwrap_or(fonts).delete();
                // Font before geometry: the controls already carry the new
                // fonts by the time `SetWindowPos` (which raises WM_SIZE)
                // and the explicit `layout` below run, so the ListView's
                // row height is queried at the size it will actually draw.
                //
                // No column-width loop here any more. Widths used to be
                // fixed per-DPI constants that only this arm refreshed;
                // they are now a proportion of the live list width and
                // `layout`, called at the bottom of this arm, is the one
                // place that sets them.
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
            WM_SYSCOLORCHANGE => {
                // System palette changed (e.g. entering/leaving high
                // contrast). Every control on this window already reads
                // colours through GetSysColor / DefWindowProcW's own
                // COLOR_BTNFACE brush -- see the module-level colour audit
                // -- so there is no cached colour of ours to re-read here;
                // the forward+invalidate is what makes the CHILDREN's own
                // cached colours (edit control backgrounds, ListView text/
                // back colour) catch up.
                broadcast_theme_change(hwnd, msg, wp, lp);
                LRESULT(0)
            }
            WM_THEMECHANGED => {
                // Visual style changed. Themed common controls (the
                // ListView) open their theme handle once and keep it until
                // told otherwise; WM_THEMECHANGED is that notice, and it
                // only reaches top-level windows, hence the forward.
                broadcast_theme_change(hwnd, msg, wp, lp);
                LRESULT(0)
            }
            WM_SETTINGCHANGE => {
                // WM_SETTINGCHANGE fires for dozens of unrelated SPI_
                // actions (wallpaper, mouse trails, ...) -- wParam carries
                // the SPI_ action code when SystemParametersInfo was called
                // with SPIF_SENDCHANGE, which is how Windows reports a
                // high-contrast toggle. Only that one is this window's
                // concern; everything else must fall through to
                // DefWindowProcW untouched rather than relayout on every
                // unrelated settings change.
                if wp.0 == SPI_SETHIGHCONTRAST.0 as usize {
                    broadcast_theme_change(hwnd, msg, wp, lp);
                    LRESULT(0)
                } else {
                    DefWindowProcW(hwnd, msg, wp, lp)
                }
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
                // Taken out of the `RefCell` first, so all three
                // `DeleteObject` calls run with no borrow alive -- and so
                // all three run at all. One `HFONT` per window open was
                // already a leak Landing 1 had to close; three roles make
                // it three.
                let ui = UI.with(|u| u.borrow_mut().take());
                if let Some(ui) = ui {
                    ui.fonts.delete();
                }
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
