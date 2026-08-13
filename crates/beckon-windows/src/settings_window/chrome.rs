//! The client-drawn title bar: a 34 px band carrying the app icon, `beckon`,
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
//! **`button_rects` is the one geometry function**, read by both `paint` and
//! `hit_button` -- and both now read it from the SAME rect: `GetClientRect`.
//! `hit_button` receives `pt` in screen coordinates (as `WM_NCHITTEST`
//! delivers them) and converts with `ScreenToClient` before calling
//! `button_rects`, rather than reading `GetWindowRect` on its own. That
//! matters because after `nccalcsize` restores only `.top`, the window rect
//! and the client rect are NOT the same physical edge on the horizontal
//! axis: the client stays inset left/right/bottom by the resize-frame
//! metrics (`SM_CXSIZEFRAME + SM_CXPADDEDBORDER`, ~8 px at 96 DPI). An
//! earlier version had `hit_button` read `GetWindowRect` while `paint` read
//! `GetClientRect` and called that "each caller supplies its own right edge,
//! in its own coordinate space" -- which is exactly backwards: two different
//! rulers were being compared as if they were one, so the drawn pixel and
//! the hit-tested pixel disagreed by that same inset. Reading the same rect
//! in the same space is what actually makes them agree, not "each caller
//! owns its own space."

use super::theme::ThemeCache;
use super::{scale, text_size, wide, Fonts, Role};
use beckon_core::theme::Theme;
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    DrawTextW, FillRect, ScreenToClient, SelectObject, SetBkMode, SetTextColor, COLOR_BTNFACE,
    COLOR_BTNTEXT, COLOR_GRAYTEXT, COLOR_HIGHLIGHT, COLOR_HIGHLIGHTTEXT, DT_CENTER, DT_LEFT,
    DT_NOPREFIX, DT_SINGLELINE, DT_VCENTER, HDC, HGDIOBJ, TRANSPARENT,
};
use windows::Win32::UI::HiDpi::{GetDpiForWindow, GetSystemMetricsForDpi};
use windows::Win32::UI::WindowsAndMessaging::{
    DrawIconEx, GetClassLongPtrW, GetClientRect, GetWindowRect, DI_NORMAL, GCLP_HICONSM, HICON,
    HTBOTTOM, HTBOTTOMLEFT, HTBOTTOMRIGHT, HTCAPTION, HTCLIENT, HTCLOSE, HTLEFT, HTMINBUTTON,
    HTRIGHT, HTTOP, HTTOPLEFT, HTTOPRIGHT, SM_CXPADDEDBORDER, SM_CYSIZEFRAME,
};

/// The bar's height, at 96 DPI. `layout` offsets every band's starting `y`
/// by `scale(TITLEBAR_H, dpi)` so the body draws below it rather than under
/// it -- see the comment on `layout`'s `y` in `layout.rs`.
pub(super) const TITLEBAR_H: i32 = 34;

/// One caption button's width, at 96 DPI. Two of them, right-aligned flush
/// against the client's own right edge -- the same edge a real system
/// caption's buttons sit against.
const BUTTON_W: i32 = 46;

/// The minimize and close buttons' rects, right-aligned against `right` /
/// `top`. Both callers now pass CLIENT coordinates -- `paint` reads them
/// straight from `GetClientRect`, and `hit_button` converts its
/// screen-coordinate `pt` with `ScreenToClient` before reading the same
/// `GetClientRect` -- so `right`/`top` name the same physical edge either
/// time. Paired with the hit-test code each rect answers. Close is
/// outermost, matching every native Windows title bar's own left-to-right
/// order.
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
/// **This window receives `WM_NCCALCSIZE` with `wParam == FALSE`, and only
/// that form.** Measured on a14 2026-08-13 by logging the first line of this
/// function: exactly one call, `wparam=0`. Handling only the `TRUE` form —
/// which every published sample of this technique does, and which this
/// function did — means the caption is never reclaimed: the settings window
/// showed the system caption AND its own drawn bar underneath, with
/// `ClientToScreen(0,0).y - GetWindowRect().top == 45` (= `SM_CYCAPTION` 34 +
/// `SM_CYSIZEFRAME` 5 + `SM_CXPADDEDBORDER` 6, the untouched default) where a
/// working handler gives 0.
///
/// **CORRECTED: an earlier fix blamed aliasing and was wrong.** The reasoning
/// was that holding a `&mut NCCALCSIZE_PARAMS` across `DefWindowProcW` lets
/// the compiler treat the read-back as dead. Plausible, and it is still why
/// this function uses raw pointers throughout — but it was not the cause.
/// Rewriting it that way changed nothing on hardware, which is how the real
/// cause was found. Do not re-derive the aliasing story and stop there.
///
/// The two forms differ only in how `lParam` is shaped: `FALSE` gives a bare
/// `RECT`, `TRUE` gives `NCCALCSIZE_PARAMS` whose `rgrc[0]` plays the same
/// role. Both are window-rect-in, client-rect-out.
pub(super) fn nccalcsize(_hwnd: HWND, _wparam: WPARAM, _lparam: LPARAM) -> LRESULT {
    // Returning 0 without calling `DefWindowProcW` leaves the rect exactly as
    // it arrived -- the proposed WINDOW rect -- so the client becomes the whole
    // window and `paint` fills every pixel of it. Both `wParam` forms are
    // handled by doing nothing to either, which is why neither parameter is
    // read: `FALSE` hands over a bare `RECT` and `TRUE` an `NCCALCSIZE_PARAMS`
    // whose `rgrc[0]` plays the same role, and leaving each untouched says the
    // same thing.
    LRESULT(0)
}

/// Which caption button, if any, `pt` -- screen coordinates, exactly as
/// `WM_NCHITTEST` delivers them -- is over.
///
/// Converts to client coordinates with `ScreenToClient`, then reads
/// `GetClientRect` -- the same rect `paint` fills, not `GetWindowRect`.
/// `nccalcsize` restores only `.top`, so the window rect and the client
/// rect are inset from each other on the left/right/bottom by the
/// resize-frame metrics; comparing screen-space hits against a
/// client-space fill was the bug (see the module header). Working entirely
/// in client space is what makes the drawn pixel and the hit-tested pixel
/// unable to disagree, not "each caller owns its own space."
pub(super) fn hit_button(hwnd: HWND, pt: POINT, dpi: u32) -> Option<i32> {
    let mut client_pt = pt;
    if !unsafe { ScreenToClient(hwnd, &mut client_pt) }.as_bool() {
        return None;
    }
    let mut rc = RECT::default();
    if unsafe { GetClientRect(hwnd, &mut rc) }.is_err() {
        return None;
    }
    for (r, code) in button_rects(rc.right, 0, dpi) {
        if client_pt.x >= r.left
            && client_pt.x < r.right
            && client_pt.y >= r.top
            && client_pt.y < r.bottom
        {
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
    // With the whole frame reclaimed there is no non-client border left for
    // `DefWindowProc` to find a resize direction in, so every edge and corner
    // is answered here. Corners first: a point in the bottom-left corner is in
    // both the left strip and the bottom strip, and answering `HTLEFT` there
    // would cost the diagonal cursor.
    let border = unsafe {
        GetSystemMetricsForDpi(SM_CYSIZEFRAME, dpi) + GetSystemMetricsForDpi(SM_CXPADDEDBORDER, dpi)
    };
    let l = pt.x < rc.left + border;
    let r = pt.x >= rc.right - border;
    let t = pt.y < rc.top + border;
    let b = pt.y >= rc.bottom - border;
    let edge = match (l, r, t, b) {
        (true, _, true, _) => Some(HTTOPLEFT),
        (_, true, true, _) => Some(HTTOPRIGHT),
        (true, _, _, true) => Some(HTBOTTOMLEFT),
        (_, true, _, true) => Some(HTBOTTOMRIGHT),
        (true, ..) => Some(HTLEFT),
        (_, true, ..) => Some(HTRIGHT),
        (_, _, true, _) => Some(HTTOP),
        (_, _, _, true) => Some(HTBOTTOM),
        _ => None,
    };
    if let Some(e) = edge {
        return Some(LRESULT(e as isize));
    }

    let bar_h = rc.top + TITLEBAR_H * dpi as i32 / 96;
    if pt.y >= bar_h {
        // Below the bar and not on an edge: ordinary client, and the child
        // controls under it must keep getting their own mouse messages.
        return Some(LRESULT(HTCLIENT as isize));
    }
    // The resize border wins over the caption along the very top edge,
    // otherwise the window cannot be resized upward at all.
    //
    // **Both terms, not just `SM_CYSIZEFRAME`.** That is only half of what
    // Windows itself uses for the top resize border -- the module header's
    // own arithmetic for the horizontal metrics already names the pair
    // (`SM_CXSIZEFRAME + SM_CXPADDEDBORDER`, ~8 px at 96 DPI); the vertical
    // one is the same two constants under their Y names. Omitting
    // `SM_CXPADDEDBORDER` here left the top edge's grabbable strip half the
    // width of every other window on the machine.
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

    // The version, muted, directly after the name.
    //
    // **`text_muted`, not `text_faint`.** `text_faint` is a card-only token
    // (4.513:1 Light / 4.504:1 Dark against `card` -- 0.013 / 0.004 of
    // headroom over the 4.5 floor) and this bar fills with `bg`, not `card`;
    // measured there it drops to 4.10:1 Light, a real WCAG failure.
    // `text_muted` on `bg` is 5.58:1 / 7.33:1 and is already a CI-enforced
    // pair (`beckon_core::theme`'s `"muted text on window bg"` test) -- no
    // new token, no new test row.
    let (title_w, _) = unsafe { text_size(hwnd, fonts.get(Role::Title), dpi, "beckon") };
    let ver_x = title_x + title_w + scale(8, dpi);
    unsafe { SelectObject(hdc, HGDIOBJ(fonts.get(Role::Caption).0)) };
    let ver_col = cache.col(|p| p.text_muted, COLOR_GRAYTEXT);
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
    // Read once from the cache that is actually about to draw -- not the
    // separate `HIGH_CONTRAST` `Cell` in `mod.rs`. That `Cell` refreshes
    // only on `WM_SETTINGCHANGE(SPI_SETHIGHCONTRAST)`, while `WM_THEMECHANGED`
    // alone already rebuilds `ThemeCache` to `Theme::HighContrast` and
    // invalidates the window; a high-contrast toggle raises both messages,
    // and if `WM_THEMECHANGED` lands first this repaint runs with the cache
    // already in high contrast but the `Cell` not yet caught up. Gating on
    // `cache.theme()` removes that divergence instead of narrowing it.
    let hc = cache.theme() == Theme::HighContrast;
    for (r, code) in button_rects(bar.right, bar.top, dpi) {
        let is_hot = hot == Some(code as i32);
        if is_hot {
            let fill = if code == HTCLOSE {
                if hc {
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
        // Close's hot fill is always either the red literal or, under high
        // contrast, `COLOR_HIGHLIGHT` -- so its ink is always the matching
        // "on highlight" colour, white in Light/Dark and the system
        // `COLOR_HIGHLIGHTTEXT` pair under high contrast, either way. Minimize's
        // hot fill is `accent_soft`, a near-white TINT in Light (0xE8F0FF) --
        // `accent_on` (always 0xFFFFFF) would be white-on-near-white there
        // (measured contrast ~1.1:1), so it is used for minimize ONLY under
        // high contrast, where the fill switches to `COLOR_HIGHLIGHT` and
        // needs the same system pair. Outside high contrast, minimize keeps
        // `p.text`/`COLOR_BTNTEXT`, which is what it always used. Under high
        // contrast, `p.text`/`COLOR_BTNTEXT` paired against a
        // `COLOR_HIGHLIGHT` fill is the unreadable combination this branch
        // exists to avoid: black-on-#37006E in HC White, white-on-#1AEBFF in
        // HC Black.
        let ink = if is_hot && (code == HTCLOSE || hc) {
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
