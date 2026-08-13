//! The OS's answer to "which theme", and the GDI objects that answer costs.

use beckon_core::theme::{Palette, Theme, ThemeInputs};
use std::collections::HashMap;
use windows::core::{w, BOOL, PCWSTR};
use windows::Win32::Foundation::{COLORREF, HWND};
use windows::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWINDOWATTRIBUTE};
use windows::Win32::Graphics::Gdi::{
    CreateSolidBrush, DeleteObject, GetSysColor, HBRUSH, HGDIOBJ, SYS_COLOR_INDEX,
};
use windows::Win32::System::Registry::{
    RegCloseKey, RegOpenKeyExW, RegQueryValueExW, HKEY, HKEY_CURRENT_USER, KEY_READ,
};
use windows::Win32::UI::Accessibility::{HCF_HIGHCONTRASTON, HIGHCONTRASTW};
use windows::Win32::UI::WindowsAndMessaging::{
    SystemParametersInfoW, SPI_GETHIGHCONTRAST, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS,
};

/// `0xRRGGBB` to Win32's `0x00BBGGRR`.
///
/// Called from `ThemeCache::col` below, which is itself unreachable until
/// the drawing code (Task 5 on) starts calling it -- hence the allow. The
/// real Windows CI job runs this crate's own clippy under `-D warnings`
/// (unlike the local macOS-shaped gate, which excludes it), so leaving this
/// as ordinary dead code would fail that job the moment this commit lands,
/// the same way an unused `use super::*;` already did for Task 3.
#[allow(dead_code)]
pub(super) fn colorref(rgb: u32) -> COLORREF {
    COLORREF(((rgb & 0xFF) << 16) | (rgb & 0xFF00) | ((rgb >> 16) & 0xFF))
}

/// Ask Windows what it wants, as plain data for `core_theme::resolve`.
pub(super) fn read_inputs() -> ThemeInputs {
    let mut hc = HIGHCONTRASTW {
        cbSize: std::mem::size_of::<HIGHCONTRASTW>() as u32,
        ..Default::default()
    };
    let high_contrast = unsafe {
        SystemParametersInfoW(
            SPI_GETHIGHCONTRAST,
            hc.cbSize,
            Some(&mut hc as *mut _ as *mut _),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        )
        .is_ok()
            && hc.dwFlags.0 & HCF_HIGHCONTRASTON.0 != 0
    };
    ThemeInputs {
        high_contrast,
        apps_use_light_theme: read_apps_use_light(),
    }
}

/// `HKCU\...\Themes\Personalize\AppsUseLightTheme`. `None` when absent, which
/// is a fresh profile and means light.
fn read_apps_use_light() -> Option<u32> {
    read_personalize_dword("AppsUseLightTheme")
}

/// `EnableTransparency` from the same key, for the backdrop tier.
///
/// Unreachable until Task 13 wires it into the Mica/alpha/opaque decision --
/// see the allow on `colorref` above for why that must be marked rather than
/// left as ordinary dead code.
#[allow(dead_code)]
pub(super) fn read_transparency_enabled() -> bool {
    // Absent means on: transparency is the Windows default.
    read_personalize_dword("EnableTransparency") != Some(0)
}

fn read_personalize_dword(name: &str) -> Option<u32> {
    unsafe {
        let mut key = HKEY::default();
        let path = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize");
        if RegOpenKeyExW(HKEY_CURRENT_USER, path, Some(0), KEY_READ, &mut key).is_err() {
            return None;
        }
        let mut value: u32 = 0;
        let mut size = std::mem::size_of::<u32>() as u32;
        let wide: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
        let ok = RegQueryValueExW(
            key,
            PCWSTR(wide.as_ptr()),
            None,
            None,
            Some(&mut value as *mut _ as *mut u8),
            Some(&mut size),
        )
        .is_ok();
        let _ = RegCloseKey(key);
        ok.then_some(value)
    }
}

/// Tell DWM which way the frame, border and shadow should lean. Needed even
/// with a client-drawn caption: the window BORDER is DWM's, not ours.
pub(super) fn apply_dwm_dark(hwnd: HWND, dark: bool) {
    const DWMWA_USE_IMMERSIVE_DARK_MODE: DWMWINDOWATTRIBUTE = DWMWINDOWATTRIBUTE(20);
    let on: BOOL = dark.into();
    unsafe {
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_USE_IMMERSIVE_DARK_MODE,
            &on as *const _ as *const _,
            std::mem::size_of::<BOOL>() as u32,
        );
    }
}

/// The current theme plus one solid brush per colour actually used.
///
/// Brushes are cached because a repaint of the list alone asks for the same
/// half-dozen colours once per row, and `CreateSolidBrush` per row per paint
/// is the kind of cost that only shows up on the slowest machine someone owns.
#[derive(Default)]
pub(super) struct ThemeCache {
    theme: Option<Theme>,
    brushes: HashMap<u32, HBRUSH>,
}

impl ThemeCache {
    /// Unreachable until later tasks read it back to decide what to draw --
    /// see the allow on `colorref` above.
    #[allow(dead_code)]
    pub(super) fn theme(&self) -> Theme {
        self.theme.unwrap_or(Theme::Light)
    }

    /// Swap the theme and drop every brush built for the old one.
    ///
    /// Returns true when the theme actually changed, so the caller can skip
    /// the invalidate. `WM_SETTINGCHANGE` fires for a great many things that
    /// are not the colour scheme.
    pub(super) fn rebuild(&mut self, t: Theme) -> bool {
        if self.theme == Some(t) {
            return false;
        }
        self.free();
        self.theme = Some(t);
        true
    }

    fn free(&mut self) {
        for (_, b) in self.brushes.drain() {
            unsafe {
                let _ = DeleteObject(HGDIOBJ(b.0));
            }
        }
    }

    /// A colour, named twice: once as a palette token and once as the
    /// `GetSysColor` index that stands in for it under high contrast.
    ///
    /// Both arguments are mandatory, which is what makes the third branch
    /// impossible to forget at a call site.
    ///
    /// Unreachable until Task 5 (`Consumes: ThemeCache::col, ThemeCache::brush
    /// from Task 4`) calls it from the card/button drawing -- see the allow
    /// on `colorref` above.
    #[allow(dead_code)]
    pub(super) fn col(&self, pick: impl Fn(&Palette) -> u32, sys: SYS_COLOR_INDEX) -> COLORREF {
        match self.theme().palette() {
            Some(p) => colorref(pick(p)),
            None => COLORREF(unsafe { GetSysColor(sys) }),
        }
    }

    /// A cached solid brush for a resolved `COLORREF`.
    ///
    /// Never returns a system brush, so every handle here is ours to delete
    /// and `free` cannot leak or double-free one of Windows'.
    ///
    /// Unreachable until Task 5 calls it -- see the allow on `colorref` above.
    #[allow(dead_code)]
    pub(super) fn brush(&mut self, c: COLORREF) -> HBRUSH {
        *self
            .brushes
            .entry(c.0)
            .or_insert_with(|| unsafe { CreateSolidBrush(c) })
    }
}

impl Drop for ThemeCache {
    fn drop(&mut self) {
        self.free();
    }
}
