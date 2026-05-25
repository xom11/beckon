//! Window enumeration, focus, and minimize via Win32 API.
//!
//! `enum_visible_windows()` returns windows in z-order (front-to-back),
//! which is inherently MRU — the foreground window is first.

use anyhow::{Context, Result};
use std::collections::HashMap;
use windows::core::{BOOL, GUID, PWSTR};
use windows::Win32::Foundation::{CloseHandle, HANDLE, HWND, LPARAM, PROPERTYKEY};
use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED};
use windows::Win32::Storage::Packaging::Appx::GetApplicationUserModelId;
use windows::Win32::System::Com::StructuredStorage::{PropVariantClear, PropVariantToString};
use windows::Win32::System::Threading::{
    AttachThreadInput, GetCurrentThreadId, OpenProcess, QueryFullProcessImageNameW,
    PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::Shell::PropertiesSystem::{IPropertyStore, SHGetPropertyStoreForWindow};
use windows::Win32::UI::WindowsAndMessaging::*;

const PKEY_APP_USER_MODEL_ID: PROPERTYKEY = PROPERTYKEY {
    fmtid: GUID::from_u128(0x9f4c2855_9f79_4b39_a8d0_e1d42de1d5f3),
    pid: 5,
};

#[derive(Debug, Clone)]
pub struct WindowInfo {
    pub hwnd: HWND,
    pub pid: u32,
    pub title: String,
    pub class_name: String,
    /// Full path to the exe, e.g. `C:\Program Files\...\app.exe`.
    pub exe_path: String,
    /// Just the filename, lowercased: `app.exe`.
    pub exe_name: String,
    /// AppUserModelID for packaged applications, when supplied by the process.
    pub aumid: Option<String>,
}

#[derive(Debug, Clone)]
struct ProcessInfo {
    exe_path: String,
    exe_name: String,
    aumid: Option<String>,
}

/// Enumerate all visible, non-cloaked, titled top-level windows.
/// Returned in z-order (front-to-back = MRU).
pub fn enum_visible_windows() -> Result<Vec<WindowInfo>> {
    let mut hwnds: Vec<HWND> = Vec::new();
    unsafe {
        let _ = EnumWindows(
            Some(enum_callback),
            LPARAM(&mut hwnds as *mut Vec<HWND> as isize),
        );
    }

    // Cache pid -> process identity to avoid opening the same process repeatedly.
    let mut process_cache: HashMap<u32, Option<ProcessInfo>> = HashMap::new();
    let mut windows = Vec::new();

    for hwnd in hwnds {
        if let Some(info) = build_window_info(hwnd, &mut process_cache) {
            windows.push(info);
        }
    }
    Ok(windows)
}

unsafe extern "system" fn enum_callback(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let hwnds = &mut *(lparam.0 as *mut Vec<HWND>);
    hwnds.push(hwnd);
    BOOL(1) // continue
}

fn build_window_info(
    hwnd: HWND,
    process_cache: &mut HashMap<u32, Option<ProcessInfo>>,
) -> Option<WindowInfo> {
    unsafe {
        // Must be visible.
        if !IsWindowVisible(hwnd).as_bool() {
            return None;
        }

        // Skip cloaked windows (hidden UWP, other virtual desktops).
        let mut cloaked: u32 = 0;
        let _ = DwmGetWindowAttribute(
            hwnd,
            DWMWA_CLOAKED,
            &mut cloaked as *mut u32 as *mut _,
            std::mem::size_of::<u32>() as u32,
        );
        if cloaked != 0 {
            return None;
        }

        // Must have a title.
        let mut title_buf = [0u16; 512];
        let title_len = GetWindowTextW(hwnd, &mut title_buf);
        if title_len == 0 {
            return None;
        }
        let title = String::from_utf16_lossy(&title_buf[..title_len as usize]);

        // Skip tool windows (floating toolbars etc.).
        let ex_style = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;
        if ex_style & WS_EX_TOOLWINDOW.0 != 0 {
            return None;
        }

        // Owner-less windows only (top-level app windows).
        // Windows with an owner are typically dialogs/popups.
        let owner = GetWindow(hwnd, GW_OWNER);
        if let Ok(o) = owner {
            if o != HWND::default() {
                return None;
            }
        }

        // Get PID.
        let mut pid: u32 = 0;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == 0 {
            return None;
        }

        // Get class name.
        let mut class_buf = [0u16; 256];
        let class_len = GetClassNameW(hwnd, &mut class_buf);
        let class_name = String::from_utf16_lossy(&class_buf[..class_len as usize]);

        // Get executable and packaged-app identity (cached by pid).
        let process = process_cache
            .entry(pid)
            .or_insert_with(|| get_process_info(pid))
            .clone()?;

        Some(WindowInfo {
            hwnd,
            pid,
            title,
            aumid: get_window_aumid(hwnd)
                .or(process.aumid)
                .or_else(|| built_in_window_aumid(&class_name)),
            class_name,
            exe_path: process.exe_path,
            exe_name: process.exe_name,
        })
    }
}

fn get_process_info(pid: u32) -> Option<ProcessInfo> {
    unsafe {
        let process: HANDLE = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = [0u16; 1024];
        let mut size = buf.len() as u32;
        let result = QueryFullProcessImageNameW(
            process,
            PROCESS_NAME_WIN32,
            windows::core::PWSTR(buf.as_mut_ptr()),
            &mut size,
        );
        if result.is_err() {
            let _ = CloseHandle(process);
            return None;
        }
        let path = String::from_utf16_lossy(&buf[..size as usize]);
        let name = path.rsplit('\\').next().unwrap_or(&path).to_lowercase();
        let aumid = get_aumid(process);
        let _ = CloseHandle(process);
        Some(ProcessInfo {
            exe_path: path,
            exe_name: name,
            aumid,
        })
    }
}

fn get_aumid(process: HANDLE) -> Option<String> {
    let mut buf = [0u16; 512];
    let mut size = buf.len() as u32;
    let result =
        unsafe { GetApplicationUserModelId(process, &mut size, Some(PWSTR(buf.as_mut_ptr()))) };
    if result.0 != 0 {
        return None;
    }
    let len = buf.iter().position(|&ch| ch == 0).unwrap_or(buf.len());
    Some(String::from_utf16_lossy(&buf[..len]))
}

fn get_window_aumid(hwnd: HWND) -> Option<String> {
    unsafe {
        let store: IPropertyStore = SHGetPropertyStoreForWindow(hwnd).ok()?;
        let mut value = store.GetValue(&PKEY_APP_USER_MODEL_ID).ok()?;
        let mut buf = [0u16; 512];
        let result = PropVariantToString(&value, &mut buf);
        let _ = PropVariantClear(&mut value);
        result.ok()?;
        let len = buf.iter().position(|&ch| ch == 0).unwrap_or(buf.len());
        if len == 0 {
            None
        } else {
            Some(String::from_utf16_lossy(&buf[..len]))
        }
    }
}

fn built_in_window_aumid(class_name: &str) -> Option<String> {
    if class_name.eq_ignore_ascii_case("CabinetWClass") {
        Some("Microsoft.Windows.Explorer".to_string())
    } else {
        None
    }
}

/// HWND of the current foreground window.
pub fn get_foreground_hwnd() -> HWND {
    unsafe { GetForegroundWindow() }
}

/// RAII guard for `AttachThreadInput` — guarantees the paired detach runs
/// on every exit path, including early returns from `?`.
struct ThreadInputDetach {
    our: u32,
    fg: u32,
}

impl Drop for ThreadInputDetach {
    fn drop(&mut self) {
        unsafe {
            let _ = AttachThreadInput(self.our, self.fg, false);
        }
    }
}

/// Focus a window with the `AttachThreadInput` trick to bypass
/// Win10+ anti-focus-stealing.
pub fn focus_window(hwnd: HWND) -> Result<()> {
    unsafe {
        let fg = GetForegroundWindow();
        let fg_thread = GetWindowThreadProcessId(fg, None);
        let our_thread = GetCurrentThreadId();

        let _detach = if fg_thread != 0
            && fg_thread != our_thread
            && AttachThreadInput(our_thread, fg_thread, true).as_bool()
        {
            Some(ThreadInputDetach {
                our: our_thread,
                fg: fg_thread,
            })
        } else {
            None
        };

        // Restore if minimised.
        if IsIconic(hwnd).as_bool() {
            let _ = ShowWindow(hwnd, SW_RESTORE);
        }

        let sfw_ok = SetForegroundWindow(hwnd).as_bool();
        if !sfw_ok && beckon_core::verbose() {
            eprintln!(
                "verbose: SetForegroundWindow returned false (BringWindowToTop will follow up; \
                 if focus stays put, another foreground-lock holder is blocking us)"
            );
        }
        BringWindowToTop(hwnd).ok().context("BringWindowToTop")?;
    }
    Ok(())
}

/// Minimise a window.
pub fn minimize_window(hwnd: HWND) -> Result<()> {
    unsafe {
        let _ = ShowWindow(hwnd, SW_MINIMIZE);
    }
    Ok(())
}
