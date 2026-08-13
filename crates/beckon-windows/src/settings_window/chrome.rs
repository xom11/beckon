//! The client-drawn title bar: a 40 px band carrying the app icon, `beckon`,
//! the version, and two caption buttons (minimize, close).
//!
//! **No `use super::*;`.** This file names exactly the handful of items it
//! needs from the parent module; a blanket glob left unused here (as it was
//! while this file was an empty stub) fails the Windows CI job under
//! `-D warnings`.
//!
//! **Nothing here touches `UI`.** Every function takes what it needs as a
//! parameter -- `hwnd`, `hdc`, a `&mut ThemeCache`, `&Fonts`, `dpi`, the hot
//! button -- the same rule `paint.rs` follows and for the same reason: a
//! paint can arrive while `UI` is already borrowed, and it does.
//!
//! **Clicking Close/Minimize needs no `WM_COMMAND` arm in this window at
//! all.** `nchittest` reports `HTCLOSE` / `HTMINBUTTON` for the two button
//! rects, exactly the codes a REAL system caption's buttons would report --
//! and `DefWindowProc` already owns the press/release state machine for
//! those codes, generating `WM_SYSCOMMAND(SC_CLOSE)` / `SC_MINIMIZE` on its
//! own the moment `WM_NCLBUTTONUP` arrives over one of them. That machinery
//! does not care whether a real non-client caption exists to draw feedback
//! into; it only cares what `WM_NCHITTEST` answered. So the window's
//! existing `WM_CLOSE` handling (the save prompt) fires exactly as it did
//! when the OS drew the X button -- nothing about that path changes here.
//! This file owns the pixel and the hit-test region; it does not own what a
//! click on either DOES.
//!
//! **`button_rects` is the one geometry function**, read by both `paint`
//! (client coordinates) and `hit_button` (screen coordinates, via its own
//! `GetWindowRect`) so the drawn pixel and the hit-tested pixel cannot
//! disagree -- each caller supplies its own right edge and top, in its own
//! coordinate space, and the two 46 px-wide rects fall out the same way
//! either time.

use super::theme::ThemeCache;
use super::{high_contrast, scale, text_size, wide, Fonts, Role};
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    DrawTextW, FillRect, SelectObject, SetBkMode, SetTextColor, COLOR_BTNFACE, COLOR_BTNTEXT,
    COLOR_GRAYTEXT, COLOR_HIGHLIGHT, COLOR_HIGHLIGHTTEXT, DT_CENTER, DT_LEFT, DT_NOPREFIX,
    DT_SINGLELINE, DT_VCENTER, HDC, HGDIOBJ, TRANSPARENT,
};
use windows::Win32::UI::HiDpi::{GetDpiForWindow, GetSystemMetricsForDpi};
use windows::Win32::UI::WindowsAndMessaging::{
    DefWindowProcW, DrawIconEx, GetClassLongPtrW, GetClientRect, GetWindowRect, DI_NORMAL,
    GCLP_HICONSM, HICON, HTCAPTION, HTCLOSE, HTMINBUTTON, NCCALCSIZE_PARAMS, SM_CYSIZEFRAME,
    WM_NCCALCSIZE,
};

/// The bar's height, at 96 DPI. `layout` offsets every band's starting `y`
/// by `scale(TITLEBAR_H, dpi)` so the body draws below it rather than under
/// it -- see the comment on `layout`'s `y` in `layout.rs`.
pub(super) const TITLEBAR_H: i32 = 40;

/// One caption button's width, at 96 DPI. Two of them, right-aligned flush
/// against the client's own right edge -- the same edge a real system
/// caption's buttons sit against.
const BUTTON_W: i32 = 46;

/// The minimize and close buttons' rects, right-aligned against `right` /
/// `top` -- whatever coordinate space the caller supplies, client for
/// `paint`, screen for `hit_button` -- paired with the hit-test code each
/// one answers. Close is outermost, matching every native Windows title
/// bar's own left-to-right order.
fn button_rects(right: i32, top: i32, dpi: u32) -> [(RECT, u32); 2] {
    let bw = scale(BUTTON_W, dpi);
    let bh = scale(TITLEBAR_H, dpi);
    let close = RECT {
        left: right - bw,
        top,
        right,
        bottom: top + bh,
    };
    let min = RECT {
        left: right - bw * 2,
        top,
        right: right - bw,
        bottom: top + bh,
    };
    [(min, HTMINBUTTON), (close, HTCLOSE)]
}

/// Extend the client area over the caption, keeping the resize borders.
///
/// The maximized correction every other implementation of this needs is
/// absent because the state is unreachable: `WS_MAXIMIZEBOX` is off, so
/// neither the button nor Win+Up can produce it.
pub(super) fn nccalcsize(hwnd: HWND, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if wparam.0 == 0 {
        return unsafe { DefWindowProcW(hwnd, WM_NCCALCSIZE, wparam, lparam) };
    }
    let params = unsafe { &mut *(lparam.0 as *mut NCCALCSIZE_PARAMS) };
    let before = params.rgrc[0];
    let _ = unsafe { DefWindowProcW(hwnd, WM_NCCALCSIZE, wparam, lparam) };
    // Give the caption band back to the client. The side and bottom borders
    // stay whatever DefWindowProc made them, so resizing is untouched.
    params.rgrc[0].top = before.top;
    LRESULT(0)
}

/// Which caption button, if any, `pt` (screen coordinates) is over.
///
/// One `GetWindowRect` call, then two rect tests against `button_rects` --
/// the same geometry `paint` fills, so the drawn pixel and the hit-tested
/// pixel cannot disagree.
pub(super) fn hit_button(hwnd: HWND, pt: POINT, dpi: u32) -> Option<i32> {
    let mut rc = RECT::default();
    if unsafe { GetWindowRect(hwnd, &mut rc) }.is_err() {
        return None;
    }
    for (r, code) in button_rects(rc.right, rc.top, dpi) {
        if pt.x >= r.left && pt.x < r.right && pt.y >= r.top && pt.y < r.bottom {
            return Some(code as i32);
        }
    }
    None
}

/// `None` means "let DefWindowProc answer" -- which is what resolves the
/// eight resize borders, so they keep working without being restated here.
pub(super) fn nchittest(hwnd: HWND, pt: POINT) -> Option<LRESULT> {
    let dpi = unsafe { GetDpiForWindow(hwnd) }.max(96);
    let mut rc = RECT::default();
    if unsafe { GetWindowRect(hwnd, &mut rc) }.is_err() {
        return None;
    }
    let bar_h = rc.top + TITLEBAR_H * dpi as i32 / 96;
    if pt.y >= bar_h {
        return None;
    }
    // The resize border wins over the caption along the very top edge,
    // otherwise the window cannot be resized upward at all.
    let border = unsafe { GetSystemMetricsForDpi(SM_CYSIZEFRAME, dpi) };
    if pt.y < rc.top + border {
        return None;
    }
    if let Some(ht) = hit_button(hwnd, pt, dpi) {
        return Some(LRESULT(ht as isize));
    }
    Some(LRESULT(HTCAPTION as isize))
}

/// Draw the bar: fill, icon, name, version, then the two buttons.
///
/// `cache` is the caller's own borrow of the paint-safe theme mirror
/// (`PAINT_THEME`), taken once and passed down -- not re-read here through
/// `theme_col`/`theme_brush`, which would try to borrow that same
/// `RefCell` a second time and panic. `hot` is `Ui::hot`, read by the
/// caller before this call and passed by value for the same reason this
/// file never touches `UI` directly.
pub(super) fn paint(
    hwnd: HWND,
    hdc: HDC,
    cache: &mut ThemeCache,
    fonts: &Fonts,
    dpi: u32,
    hot: Option<i32>,
) {
    let mut rc = RECT::default();
    if unsafe { GetClientRect(hwnd, &mut rc) }.is_err() {
        return;
    }
    let bar = RECT {
        left: 0,
        top: 0,
        right: rc.right,
        bottom: scale(TITLEBAR_H, dpi),
    };

    let bg = cache.col(|p| p.bg, COLOR_BTNFACE);
    let bg_brush = cache.brush(bg);
    unsafe { FillRect(hdc, &bar, bg_brush) };
    unsafe { SetBkMode(hdc, TRANSPARENT) };

    // The app icon: 18 px, scaled, 14 px in from the left, centred in the
    // bar. Read from the CLASS rather than loaded again here -- `create`
    // already resolved it once, with its own `IDI_APPLICATION` fallback,
    // and registered it on the window class as `hIconSm`.
    let icon_size = scale(18, dpi);
    let icon_x = scale(14, dpi);
    let icon = unsafe { HICON(GetClassLongPtrW(hwnd, GCLP_HICONSM) as *mut _) };
    if !icon.is_invalid() {
        let icon_y = (bar.bottom - icon_size) / 2;
        unsafe {
            let _ = DrawIconEx(
                hdc, icon_x, icon_y, icon, icon_size, icon_size, 0, None, DI_NORMAL,
            );
        }
    }

    // `beckon`, in the accent colour -- the one piece of coloured text in
    // the window's own chrome.
    let title_x = icon_x + icon_size + scale(8, dpi);
    let prev_font = unsafe { SelectObject(hdc, HGDIOBJ(fonts.get(Role::Title).0)) };
    let title_col = cache.col(|p| p.accent, COLOR_HIGHLIGHT);
    unsafe { SetTextColor(hdc, title_col) };
    let mut title = wide("beckon");
    let title_n = title.len() - 1;
    let mut title_rc = RECT {
        left: title_x,
        top: bar.top,
        right: bar.right,
        bottom: bar.bottom,
    };
    unsafe {
        DrawTextW(
            hdc,
            &mut title[..title_n],
            &mut title_rc,
            DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX,
        );
    }

    // The version, faint, directly after the name.
    let (title_w, _) = unsafe { text_size(hwnd, fonts.get(Role::Title), dpi, "beckon") };
    let ver_x = title_x + title_w + scale(8, dpi);
    unsafe { SelectObject(hdc, HGDIOBJ(fonts.get(Role::Caption).0)) };
    let ver_col = cache.col(|p| p.text_faint, COLOR_GRAYTEXT);
    unsafe { SetTextColor(hdc, ver_col) };
    let mut ver = wide(env!("CARGO_PKG_VERSION"));
    let ver_n = ver.len() - 1;
    let mut ver_rc = RECT {
        left: ver_x,
        top: bar.top,
        right: bar.right,
        bottom: bar.bottom,
    };
    unsafe {
        DrawTextW(
            hdc,
            &mut ver[..ver_n],
            &mut ver_rc,
            DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX,
        );
    }

    // The two caption buttons, right-aligned.
    unsafe { SelectObject(hdc, HGDIOBJ(fonts.get(Role::Chrome).0)) };
    for (r, code) in button_rects(bar.right, bar.top, dpi) {
        let is_hot = hot == Some(code as i32);
        if is_hot {
            let fill = if code == HTCLOSE {
                if high_contrast() {
                    cache.col(|p| p.accent_fill, COLOR_HIGHLIGHT)
                } else {
                    // Windows' own close-button red, exact regardless of the
                    // user's accent colour -- the ONE literal this file
                    // carries, and it is behind the high-contrast branch
                    // above on purpose: #C42B1C in BGR.
                    COLORREF(0x001C2BC4)
                }
            } else {
                cache.col(|p| p.accent_soft, COLOR_HIGHLIGHT)
            };
            let fill_brush = cache.brush(fill);
            unsafe { FillRect(hdc, &r, fill_brush) };
        }
        let ink = if is_hot && code == HTCLOSE {
            cache.col(|p| p.accent_on, COLOR_HIGHLIGHTTEXT)
        } else {
            cache.col(|p| p.text, COLOR_BTNTEXT)
        };
        unsafe { SetTextColor(hdc, ink) };
        // Segoe Fluent Icons, confirmed present on the target hardware.
        // `make_font`'s `GetTextFace` round-trip already handles a silent
        // substitution if it is ever missing.
        let glyph = if code == HTMINBUTTON {
            "\u{E921}"
        } else {
            "\u{E8BB}"
        };
        let mut g = wide(glyph);
        let g_n = g.len() - 1;
        let mut grc = r;
        unsafe {
            DrawTextW(
                hdc,
                &mut g[..g_n],
                &mut grc,
                DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX,
            );
        }
    }

    if !prev_font.is_invalid() {
        unsafe { SelectObject(hdc, prev_font) };
    }
}
