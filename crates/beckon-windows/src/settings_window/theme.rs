//! The OS's answer to "which theme", and the GDI objects that answer costs.

use beckon_core::theme::{Backdrop, BackdropInputs, Palette, Theme, ThemeInputs};
use std::collections::HashMap;
use windows::core::{w, BOOL, PCWSTR};
// `RtlGetVersion` is generated under the driver-kit namespace -- see the
// Cargo.toml comment on `Wdk_System_SystemServices` for why.
use windows::Wdk::System::SystemServices::RtlGetVersion;
use windows::Win32::Foundation::{COLORREF, HWND};
use windows::Win32::Graphics::Dwm::{
    DwmExtendFrameIntoClientArea, DwmSetWindowAttribute, DWMWINDOWATTRIBUTE,
};
use windows::Win32::Graphics::Gdi::{
    CreateSolidBrush, DeleteObject, GetSysColor, COLOR_BTNFACE, HBRUSH, HGDIOBJ, SYS_COLOR_INDEX,
};
use windows::Win32::System::Registry::{
    RegCloseKey, RegOpenKeyExW, RegQueryValueExW, HKEY, HKEY_CURRENT_USER, KEY_READ,
};
use windows::Win32::System::SystemInformation::OSVERSIONINFOW;
use windows::Win32::UI::Accessibility::{HCF_HIGHCONTRASTON, HIGHCONTRASTW};
// `MARGINS` -- despite feeding `DwmExtendFrameIntoClientArea`, a Dwm
// function -- is generated under `Win32::UI::Controls`, not `Win32::Graphics::Dwm`.
use windows::Win32::UI::Controls::MARGINS;
use windows::Win32::UI::WindowsAndMessaging::{
    GetSystemMetrics, GetWindowLongW, SetLayeredWindowAttributes, SetWindowLongW,
    SystemParametersInfoW, GWL_EXSTYLE, LWA_ALPHA, SM_REMOTESESSION, SPI_GETHIGHCONTRAST,
    SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, WS_EX_LAYERED,
};

/// `0xRRGGBB` to Win32's `0x00BBGGRR`.
///
/// Called from `ThemeCache::col` below, which the drawing code in `paint.rs`
/// and `mod.rs` now calls directly.
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

/// `EnableTransparency` from the same key, for the backdrop tier -- absent
/// means on, since transparency is the Windows default.
///
/// Wired into the Mica/alpha/opaque decision by `read_backdrop_inputs`
/// below, its only caller.
pub(super) fn read_transparency_enabled() -> bool {
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

/// Tint the hairline DWM draws around the window to match the window's own
/// ground.
///
/// **REVERSED 2026-08-14: the "cheaper answer" this doc argued for is not
/// the one that shipped, and this attribute is not what fixed the black
/// box.** The block used to read:
///
/// > **`WM_NCCALCSIZE` reclaims only `.top`, so the left, right and bottom
/// > resize borders stay non-client and DWM paints them — and with no
/// > `WS_CAPTION` it paints them PURE BLACK.** Measured on a14 2026-08-13: a
/// > 10 px band of `(0,0,0)` on three sides of a `#15171C` window, which
/// > reads as the window sitting inside a black box.
/// >
/// > Reclaiming the whole frame instead would work, but it moves resize
/// > hit-testing out of `DefWindowProc` and into `nchittest` — eight
/// > directions and four corners to get right, for a border. One attribute
/// > is the cheaper answer, and it keeps `DefWindowProc` owning the resize
/// > behaviour that already works.
///
/// The measurement is real and the diagnosis was right. The prescription was
/// not: `DWMWA_BORDER_COLOR` was tried first and **does not reach the sizing
/// border at all** — it tints the hairline around the window, nothing wider.
/// That is `c523e8e`'s own finding, in its message ("reclaim the whole frame,
/// and hit-test the eight resize edges", 2026-08-13), and it is why the
/// expensive path was taken the same evening: `chrome::nccalcsize` now
/// returns `LRESULT(0)` without calling `DefWindowProcW`, so **client ==
/// window on all four edges** and there is no sizing border left for DWM to
/// paint black. `chrome::nchittest` pays the price the paragraph above
/// quoted — all eight directions, corners first.
///
/// **The call stays, and it is not vestigial.** DWM still owns the 1 px
/// border it draws around the window (see `apply_dwm_dark` above, same
/// point), and that hairline is exactly what this attribute colours. Left
/// alone it does not match a `#15171C` client.
///
/// Windows 11 22H2+. The call fails harmlessly on anything older, which
/// leaves an unthemed hairline — a cosmetic fault on an OS this window
/// already treats as second class (no Mica, no rounded corners there
/// either).
pub(super) fn apply_dwm_border(hwnd: HWND, t: Theme) {
    const DWMWA_BORDER_COLOR: DWMWINDOWATTRIBUTE = DWMWINDOWATTRIBUTE(34);
    // Resolved here, not at the call site, so the high-contrast branch cannot
    // be forgotten -- the same reason `ThemeCache::col` takes both a token and
    // a `GetSysColor` index. A call site that read `palette()` directly would
    // get `None` under high contrast and fall back to black, which is the
    // exact fault this function exists to remove.
    let c = match t.palette() {
        Some(p) => colorref(p.bg),
        None => COLORREF(unsafe { GetSysColor(COLOR_BTNFACE) }),
    };
    unsafe {
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_BORDER_COLOR,
            &c as *const _ as *const _,
            std::mem::size_of::<COLORREF>() as u32,
        );
    }
}

/// Gate 01's answer for whether tier 1 (`Backdrop::Mica`) is asserted to
/// work on this build's rendering path.
///
/// **Mica under a fully GDI-painted client area is a hypothesis, not a
/// verified fact.** DWM composites the backdrop material BEHIND the window;
/// any opaque GDI fill drawn on top of that rect hides it exactly as
/// completely as if Mica had never been requested, and nothing running on
/// this machine can tell "compositing, but every pixel happens to be
/// covered" apart from "never took" -- that can only be judged by looking
/// at real hardware.
///
/// If Gate 01 measures that it does not composite cleanly, flip this ONE
/// constant to `false` and ship tier 2. Nothing else needs to change:
/// `read_backdrop_inputs` below has exactly one caller
/// (`apply_current_backdrop` in `mod.rs`), so both the window's first paint
/// and every later re-evaluation (`on_theme_changed`) see the demoted
/// answer, and `beckon_core::theme::backdrop` -- tested, not touched by this
/// flag -- is what turns it into `Backdrop::Alpha(TIER2_ALPHA)`. That
/// single-flag property is why the tier decision lives there and not here.
/// **FLIPPED 2026-08-13. Gate 01 was measured on a14 and Mica lost.** The
/// window came up fully opaque: `WS_EX_LAYERED` absent, nothing of the desktop
/// behind it visible anywhere. That is the outcome the tier design predicted —
/// DWM composites its backdrop *behind* the window, and this client is painted
/// edge to edge with GDI, so there is no unpainted region for it to show
/// through. The sheet-of-glass margins are set and simply have nothing to do.
///
/// Tier 2 is not a consolation prize here: a uniform alpha is the only one of
/// the three that a fully-painted client can actually wear.
pub(super) const MICA_SUPPORTED: bool = false;

/// Gather the OS's current answer to which backdrop tier this window may
/// use, for `beckon_core::theme::backdrop` to decide with.
///
/// `mica_supported` arrives as a parameter rather than being read here so
/// `MICA_SUPPORTED` above stays the one flag a hardware failure has to
/// flip -- see its doc comment.
pub(super) fn read_backdrop_inputs(mica_supported: bool) -> BackdropInputs {
    BackdropInputs {
        build: os_build(),
        high_contrast: read_inputs().high_contrast,
        remote_session: unsafe { GetSystemMetrics(SM_REMOTESESSION) != 0 },
        transparency_enabled: read_transparency_enabled(),
        mica_supported,
    }
}

/// The running build number, via `RtlGetVersion` -- **not** `GetVersionEx`,
/// which reports whatever version is named in the binary's application
/// manifest (or a stale Windows 8 answer, absent one) to any process that
/// does not assert compatibility with the version actually running. This
/// binary ships no such manifest, so `GetVersionEx` would under-report the
/// build on every real Windows 10/11 machine it ran on -- exactly the
/// failure `MICA_MIN_BUILD` exists to gate on correctly. `RtlGetVersion`
/// does not consult the compatibility shim at all.
fn os_build() -> u32 {
    let mut info = OSVERSIONINFOW {
        dwOSVersionInfoSize: std::mem::size_of::<OSVERSIONINFOW>() as u32,
        ..Default::default()
    };
    unsafe {
        // NTSTATUS 0 is STATUS_SUCCESS. Observed to always succeed -- ntdll
        // being unreachable would mean nothing else in the process works
        // either -- but a failure here must fail TOWARD the safe tier, not
        // toward Mica, so an error reports build 0, which is always below
        // `MICA_MIN_BUILD`.
        if RtlGetVersion(&mut info).0 == 0 {
            info.dwBuildNumber
        } else {
            0
        }
    }
}

thread_local! {
    /// The tier `apply_backdrop` last set, for `WM_ERASEBKGND` (`mod.rs`) to
    /// consult without re-deriving the answer from the registry and
    /// `RtlGetVersion` on every message -- `WM_ERASEBKGND` fires on every
    /// step of a resize drag, and `read_backdrop_inputs` is not that cheap.
    static CURRENT_TIER: std::cell::Cell<Backdrop> =
        const { std::cell::Cell::new(Backdrop::Opaque) };
}

/// The tier last applied by `apply_backdrop`. `Opaque` before the first
/// call, which is also the safe answer: nothing is painted transparently
/// before this window has decided it may be.
pub(super) fn current_tier() -> Backdrop {
    CURRENT_TIER.with(|c| c.get())
}

/// Apply one of the three backdrop tiers `beckon_core::theme::backdrop`
/// decided on. See `MICA_SUPPORTED` above for why tier 1 is a hypothesis
/// rather than an assertion this function makes true by calling it.
pub(super) fn apply_backdrop(hwnd: HWND, b: Backdrop) {
    CURRENT_TIER.with(|c| c.set(b));
    const DWMWA_SYSTEMBACKDROP_TYPE: DWMWINDOWATTRIBUTE = DWMWINDOWATTRIBUTE(38);
    const DWMSBT_NONE: u32 = 1;
    const DWMSBT_MAINWINDOW: u32 = 2;
    unsafe {
        match b {
            Backdrop::Mica => {
                let ty = DWMSBT_MAINWINDOW;
                let _ = DwmSetWindowAttribute(
                    hwnd,
                    DWMWA_SYSTEMBACKDROP_TYPE,
                    &ty as *const _ as *const _,
                    std::mem::size_of::<u32>() as u32,
                );
                // Sheet of glass: extending the frame all the way in is what
                // lets the Mica material fill the whole client rect instead
                // of just the usual few pixels of non-client border. "The
                // usual" is the generic case, not this window's -- since
                // `c523e8e` `chrome::nccalcsize` leaves no non-client area at
                // all, so the extension is the only way any of it is reached
                // here rather than merely the way to reach more of it. Its
                // documented hazard -- GDI text drawn straight onto glass
                // loses its alpha channel and fringes black -- does not
                // apply here, because every string this window draws lands
                // on an opaque surface first; the glass is only ever visible
                // where nothing is drawn at all. The next change that puts
                // text, an icon or any other GDI ink onto the window
                // background WITHOUT filling its own rect first reopens that
                // hazard and needs to know this.
                //
                // **CORRECTED 2026-08-14, Task 7.** That reason used to read
                // "every string this window draws lives inside an opaque card
                // (Task 8); the glass is only ever visible through the gaps
                // BETWEEN cards". System's and About's waiting lines are the
                // first strings drawn outside a card -- neither page has one
                // -- so the card half of the claim is no longer true and the
                // conclusion had to be re-earned rather than inherited. It is:
                // their `WM_CTLCOLORSTATIC` branch returns a `bg` brush and
                // sets `OPAQUE`, so each line's own rect is filled before a
                // glyph is drawn. What that costs is one line's worth of
                // opaque `bg` on a page that would otherwise be glass, which
                // is the trade this window has already made ~30 times over
                // (Mica is measured dead here for exactly that reason).
                let m = MARGINS {
                    cxLeftWidth: -1,
                    cxRightWidth: -1,
                    cyTopHeight: -1,
                    cyBottomHeight: -1,
                };
                let _ = DwmExtendFrameIntoClientArea(hwnd, &m);
                set_layered(hwnd, None);
            }
            Backdrop::Alpha(a) => {
                let ty = DWMSBT_NONE;
                let _ = DwmSetWindowAttribute(
                    hwnd,
                    DWMWA_SYSTEMBACKDROP_TYPE,
                    &ty as *const _ as *const _,
                    std::mem::size_of::<u32>() as u32,
                );
                // Reset frame margins set by Mica. Unconditional: when not
                // coming from Mica, the call is idempotent; when we are, it
                // prevents the -1 margins from persisting and causing visual
                // corruption. The call runs before `set_layered` so the two
                // mechanisms (DWM backdrop and WS_EX_LAYERED) don't interact.
                let m = MARGINS {
                    cxLeftWidth: 0,
                    cxRightWidth: 0,
                    cyTopHeight: 0,
                    cyBottomHeight: 0,
                };
                let _ = DwmExtendFrameIntoClientArea(hwnd, &m);
                set_layered(hwnd, Some(a));
            }
            Backdrop::Opaque => {
                let ty = DWMSBT_NONE;
                let _ = DwmSetWindowAttribute(
                    hwnd,
                    DWMWA_SYSTEMBACKDROP_TYPE,
                    &ty as *const _ as *const _,
                    std::mem::size_of::<u32>() as u32,
                );
                // Reset frame margins set by Mica. Unconditional: when not
                // coming from Mica, the call is idempotent; when we are, it
                // prevents the -1 margins from persisting and causing visual
                // corruption. The call runs before `set_layered` so the two
                // mechanisms (DWM backdrop and WS_EX_LAYERED) don't interact.
                let m = MARGINS {
                    cxLeftWidth: 0,
                    cxRightWidth: 0,
                    cyTopHeight: 0,
                    cyBottomHeight: 0,
                };
                let _ = DwmExtendFrameIntoClientArea(hwnd, &m);
                set_layered(hwnd, None);
            }
        }
    }
}

/// Add or remove `WS_EX_LAYERED` to match whether an alpha is wanted, and
/// set the alpha when one is.
///
/// **Removing the style matters, not just no longer setting an alpha.** A
/// window left `WS_EX_LAYERED` after leaving `Alpha` still gets composited
/// through an off-screen surface by DWM on every frame for a blend nothing
/// asks for any more -- paid for nothing, on the tier (`Opaque`, forced by
/// high contrast or a remote session) that can least afford it.
fn set_layered(hwnd: HWND, alpha: Option<u8>) {
    unsafe {
        let ex = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;
        let wanted = alpha.is_some();
        let has = ex & WS_EX_LAYERED.0 != 0;
        if wanted != has {
            let new_ex = if wanted {
                ex | WS_EX_LAYERED.0
            } else {
                ex & !WS_EX_LAYERED.0
            };
            SetWindowLongW(hwnd, GWL_EXSTYLE, new_ex as i32);
        }
        if let Some(a) = alpha {
            let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), a, LWA_ALPHA);
        }
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
    /// The resolved theme, read back by `col` below to decide what to draw.
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
