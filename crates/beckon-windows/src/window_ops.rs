//! Window enumeration, focus, and minimize via Win32 API.
//!
//! `enum_visible_windows()` returns windows in z-order (front-to-back),
//! which is inherently MRU — the foreground window is first.

use anyhow::{Context, Result};
use std::collections::HashMap;
use windows::core::BOOL;
use windows::Win32::Foundation::{CloseHandle, HANDLE, HWND, LPARAM};
use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED};
use windows::Win32::System::Threading::{
    AttachThreadInput, GetCurrentThreadId, OpenProcess, QueryFullProcessImageNameW,
    PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::WindowsAndMessaging::*;

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

    // Cache pid -> exe path to avoid opening the same process repeatedly.
    let mut exe_cache: HashMap<u32, Option<(String, String)>> = HashMap::new();
    let mut windows = Vec::new();

    for hwnd in hwnds {
        if let Some(info) = build_window_info(hwnd, &mut exe_cache) {
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
    exe_cache: &mut HashMap<u32, Option<(String, String)>>,
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

        // Get exe path (cached by pid).
        let (exe_path, exe_name) = exe_cache
            .entry(pid)
            .or_insert_with(|| get_exe_info(pid))
            .clone()?;

        Some(WindowInfo {
            hwnd,
            pid,
            title,
            class_name,
            exe_path,
            exe_name,
        })
    }
}

fn get_exe_info(pid: u32) -> Option<(String, String)> {
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
        let _ = CloseHandle(process);
        result.ok()?;
        let path = String::from_utf16_lossy(&buf[..size as usize]);
        let name = path.rsplit('\\').next().unwrap_or(&path).to_lowercase();
        Some((path, name))
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
