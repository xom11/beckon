//! The settings window's own look, in `HKCU\Software\beckon`.
//!
//! **This is the only file beckon writes that is not the shortcuts TOML**, and
//! that is design §1's split by STORE rather than an accident: Shortcuts and
//! Keyboard write `apps.toml`, System writes here, the Run key, or nothing.
//! The split is what makes the System page keep working when `apps.toml` does
//! not parse -- a theme switch has nothing to do with a TOML error, and before
//! the split it was greyed out by one.
//!
//! Two values, both `REG_DWORD`:
//!
//! | Name | Meaning |
//! |---|---|
//! | `DarkMode` | 0 light, anything else dark. Absent means DARK -- design §3.3's stated behaviour change |
//! | `Opacity` | 85..=100, the transparency slider. Absent means `OPACITY_DEFAULT` |
//!
//! **Absent is not zero**, and every read here says so explicitly: a fresh
//! profile has neither value, and a missing `DarkMode` read as 0 would ship
//! light mode to everyone who never touched the switch -- the exact opposite
//! of the design. `read` returns `Option<u32>` for that reason and every
//! caller supplies its own default.
//!
//! No path or quoting policy lives here; `autostart.rs` is the neighbouring
//! module and owns the Run key.

use beckon_core::settings::{clamp_opacity, OPACITY_DEFAULT};
use beckon_core::theme::{transparency_block, TransparencyBlock};
use windows::core::{w, PCWSTR};
use windows::Win32::System::Registry::{
    RegCloseKey, RegCreateKeyW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW, HKEY,
    HKEY_CURRENT_USER, KEY_READ, REG_DWORD,
};

const KEY: PCWSTR = w!("Software\\beckon");
const DARK: PCWSTR = w!("DarkMode");
const OPACITY: PCWSTR = w!("Opacity");
const CAPS_VIEW: PCWSTR = w!("CapsView");

/// Read one DWORD, or `None` when the key or the value is absent.
///
/// Absence is never coerced to 0 here -- see the module header. It is also
/// never reported as an error: a profile that has never opened the settings
/// window has no key at all, which is the ordinary first-run state and not a
/// failure to log.
fn read(name: PCWSTR) -> Option<u32> {
    unsafe {
        let mut key = HKEY::default();
        if RegOpenKeyExW(HKEY_CURRENT_USER, KEY, Some(0), KEY_READ, &mut key).is_err() {
            return None;
        }
        let mut value: u32 = 0;
        let mut size = std::mem::size_of::<u32>() as u32;
        let ok = RegQueryValueExW(
            key,
            name,
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

/// Write one DWORD, creating `Software\beckon` if this is the first time.
///
/// **`RegCreateKeyW`, not `RegOpenKeyExW`**: `autostart.rs` opens because the
/// Run key is always there, and this one is not -- a first-run write through
/// `Open` would fail on every fresh profile, which is exactly the profile that
/// most needs the default recorded once the user changes it.
///
/// **The short form, not `RegCreateKeyExW`, and the reason is a dependency
/// rather than a preference.** The `Ex` variant takes a
/// `SECURITY_ATTRIBUTES`, so the `windows` crate gates it behind the
/// `Win32_Security` feature this crate does not enable -- and enabling a
/// feature to pass `None` to one parameter is a wider Win32 surface for
/// nothing. `RegCreateKeyW` opens an existing key or creates it with the
/// parent's inherited security and `KEY_ALL_ACCESS`, which under `HKCU` is
/// exactly what a per-user preference wants. It is documented as provided for
/// 16-bit compatibility; what that costs here is the option of a narrower
/// access mask, and `HKCU\Software\beckon` is a key this user already owns
/// outright.
///
/// Failure is swallowed by the callers below rather than surfaced: what is
/// lost is that the preference does not survive a restart, and a modal
/// dialog for that on every step of a slider drag would be worse than the
/// fault. The window itself has already applied the change.
fn write(name: PCWSTR, value: u32) -> Result<(), String> {
    unsafe {
        let mut key = HKEY::default();
        let rc = RegCreateKeyW(HKEY_CURRENT_USER, KEY, &mut key);
        if rc.is_err() {
            return Err(format!("cannot create HKCU\\Software\\beckon: {rc:?}"));
        }
        let bytes = value.to_ne_bytes();
        let rc = RegSetValueExW(key, name, Some(0), REG_DWORD, Some(&bytes));
        let _ = RegCloseKey(key);
        rc.ok().map_err(|e| format!("RegSetValueExW failed: {e}"))
    }
}

/// Is the settings window dark?
///
/// **Absent means DARK**, which is design §3.3's behaviour change stated as
/// code: beckon used to read
/// `Themes\Personalize\AppsUseLightTheme` and follow Windows, and it now
/// defaults to dark and does not ask. A user on light Windows gets a dark
/// window until they turn this off. High contrast still outranks both --
/// `theme::resolve` returns `Theme::HighContrast` before it looks at this at
/// all, and that is the OS enforcing a choice rather than expressing one.
pub fn dark() -> bool {
    read(DARK) != Some(0)
}

pub fn set_dark(on: bool) -> Result<(), String> {
    write(DARK, u32::from(on))
}

/// Does the Shortcuts list fold the caps chord into one `Caps` cap?
///
/// Design §3.2's `Write shortcuts as [Caps] instead of [Ctrl][Win][Alt]`.
///
/// **Default OFF, which is the opposite of `dark()` above and is deliberate.**
/// `dark` reads absent-as-ON because §5.2 makes dark the default; this reads
/// absent-as-OFF because §3.2 says so, and because the fold hides what the
/// file actually says. A user who has never opened this window should see the
/// chord their config spells.
///
/// It is a **view** preference and lives here rather than in `apps.toml` for
/// the reason §1 splits the two stores: the file still says
/// `ctrl+super+alt+b`, so a machine with this on and a machine with it off
/// share a config byte for byte.
pub fn caps_view() -> bool {
    read(CAPS_VIEW) == Some(1)
}

pub fn set_caps_view(on: bool) -> Result<(), String> {
    write(CAPS_VIEW, u32::from(on))
}

/// The transparency percentage, clamped into the slider's own range.
///
/// Clamped on the way OUT rather than trusted: anything can write this value,
/// and an unclamped 0 would set an alpha the user could not reverse from the
/// control that set it. `clamp_opacity` is core's, so the range is stated
/// once.
pub fn opacity() -> u8 {
    match read(OPACITY) {
        Some(v) if v <= u8::MAX as u32 => clamp_opacity(v as u8),
        Some(_) => OPACITY_DEFAULT,
        None => OPACITY_DEFAULT,
    }
}

pub fn set_opacity(percent: u8) -> Result<(), String> {
    write(OPACITY, clamp_opacity(percent) as u32)
}

/// Why this machine may not be transparent, or `None` when it may.
///
/// A thin wrapper over `beckon_core::theme::transparency_block` and the
/// window's own `read_backdrop_inputs`, so the System page and the window's
/// backdrop tier ask the same question of the same inputs. It lives here
/// rather than in `settings_window::theme` because the page reads it as a
/// PREFERENCE-adjacent fact ("is the slider live") while the tier reads it as
/// a rendering one, and only this module is public.
pub fn transparency_block_now() -> Option<TransparencyBlock> {
    transparency_block(crate::settings_window::backdrop_inputs())
}
