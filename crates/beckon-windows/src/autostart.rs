//! "Start with Windows" as an `HKCU\…\Run` value.
//!
//! Chosen over a Scheduled Task (which needs ~200 lines of COM and appears
//! in no user-facing list) and over a Startup-folder shortcut. A Run value
//! shows up in Task Manager -> Startup apps and in Settings -> Apps ->
//! Startup, so the user can turn it off the way they turn off every other
//! app. The `RestartOnFailure` it gives up was mostly guarding against the
//! Windows Terminal tab that CTRL_CLOSE_EVENTs a console-hosted serve --
//! a cause a GUI-subsystem binary does not have.
//!
//! No path or quoting policy lives here; see
//! `beckon-cli/src/serve_app.rs::run_key_command_line`.

use windows::core::{HSTRING, PCWSTR};
use windows::Win32::System::Registry::{
    RegCloseKey, RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW, HKEY,
    HKEY_CURRENT_USER, KEY_READ, KEY_WRITE, REG_SZ,
};

const RUN_KEY: PCWSTR = windows::core::w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
const VALUE: PCWSTR = windows::core::w!("beckon");

fn open(access: windows::Win32::System::Registry::REG_SAM_FLAGS) -> Option<HKEY> {
    let mut key = HKEY::default();
    let rc = unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, RUN_KEY, Some(0), access, &mut key) };
    rc.is_ok().then_some(key)
}

/// Is the autostart value present? Its contents are deliberately not parsed
/// -- the tick box answers "will this start at logon", nothing finer.
pub fn is_enabled() -> bool {
    let Some(key) = open(KEY_READ) else {
        return false;
    };
    let rc = unsafe { RegQueryValueExW(key, VALUE, None, None, None, None) };
    let _ = unsafe { RegCloseKey(key) };
    rc.is_ok()
}

pub fn enable(command: &str) -> Result<(), String> {
    let key = open(KEY_WRITE).ok_or_else(|| "cannot open the Run key for writing".to_string())?;
    let wide = HSTRING::from(command);
    // REG_SZ wants the byte length INCLUDING the NUL terminator.
    let bytes = unsafe {
        std::slice::from_raw_parts(
            wide.as_ptr() as *const u8,
            (wide.len() + 1) * std::mem::size_of::<u16>(),
        )
    };
    let rc = unsafe { RegSetValueExW(key, VALUE, Some(0), REG_SZ, Some(bytes)) };
    let _ = unsafe { RegCloseKey(key) };
    rc.ok().map_err(|e| format!("RegSetValueExW failed: {e}"))
}

pub fn disable() -> Result<(), String> {
    let key = open(KEY_WRITE).ok_or_else(|| "cannot open the Run key for writing".to_string())?;
    let rc = unsafe { RegDeleteValueW(key, VALUE) };
    let _ = unsafe { RegCloseKey(key) };
    rc.ok().map_err(|e| format!("RegDeleteValueW failed: {e}"))
}
