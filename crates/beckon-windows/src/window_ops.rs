//! Window enumeration, focus, and minimize via Win32 API.
//!
//! `enum_visible_windows()` returns windows in z-order (front-to-back),
//! which is inherently MRU — the foreground window is first.

use anyhow::{Context, Result};
use beckon_core::cloak::{self, Desktop};
use std::collections::HashMap;
use windows::core::{BOOL, GUID, PWSTR};
use windows::Win32::Foundation::{CloseHandle, HANDLE, HWND, LPARAM, PROPERTYKEY};
use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED};
use windows::Win32::Storage::Packaging::Appx::GetApplicationUserModelId;
use windows::Win32::System::Com::StructuredStorage::{PropVariantClear, PropVariantToString};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_APARTMENTTHREADED,
};
use windows::Win32::System::Threading::{
    AttachThreadInput, GetCurrentThreadId, OpenProcess, QueryFullProcessImageNameW,
    PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::Shell::PropertiesSystem::{IPropertyStore, SHGetPropertyStoreForWindow};
use windows::Win32::UI::Shell::{IVirtualDesktopManager, VirtualDesktopManager};
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

/// Answers "which virtual desktop is this window on?" — the second test that
/// can rescue a window `DWMWA_CLOAKED` would drop. See
/// `beckon_core::cloak` for *why* the cloak word cannot answer it alone.
///
/// **The COM object is created at most once per `enum_visible_windows` call,
/// never once per window**, and lazily even then: the constructor does not
/// run until the first cloaked window actually asks. On a machine with one
/// virtual desktop and no suspended UWP apps it therefore costs nothing at
/// all. The hot path (`beckon <id>`) is budgeted at 50 ms and already
/// measured at ~57 ms, so a per-window `CoCreateInstance` is not affordable —
/// see the *Hot-path catalog cost* note in `CLAUDE.md`, which this is the
/// same class of mistake as.
///
/// The failure is memoised alongside the success (`Some(None)`): a machine
/// where the object cannot be created must pay one failed
/// `CoCreateInstance`, not one per cloaked window.
struct DesktopOracle {
    /// `None` = not asked yet. `Some(None)` = asked, and it failed.
    manager: Option<Option<IVirtualDesktopManager>>,
}

impl DesktopOracle {
    fn new() -> Self {
        Self { manager: None }
    }

    fn manager(&mut self) -> Option<&IVirtualDesktopManager> {
        self.manager
            .get_or_insert_with(|| unsafe {
                // Idempotent — returns S_FALSE when the thread is already in
                // an STA, which it often is by the time we get here (`apps.rs`
                // and `backend.rs` both do this). `CoCreateInstance` returns
                // CO_E_NOTINITIALIZED without it, so it is not optional, and
                // the result is deliberately discarded exactly as at the four
                // other call sites in this crate.
                let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
                // `CLSCTX_ALL` rather than the `CLSCTX_INPROC_SERVER` used
                // elsewhere here: this CLSID is served by the shell, and
                // pinning the context would turn a working machine into a
                // silent fallback to today's behaviour.
                CoCreateInstance(&VirtualDesktopManager, None, CLSCTX_ALL).ok()
            })
            .as_ref()
    }

    /// **Never returns `Current` on failure.** An error is `Unknown`, which
    /// `cloak::admit_window` sends back to today's behaviour — see its doc
    /// for why the three-way answer is not collapsible to a bool.
    fn locate(&mut self, hwnd: HWND) -> Desktop {
        let Some(manager) = self.manager() else {
            return Desktop::Unknown;
        };
        // Fails for a window that is not top-level or is mid-creation, and
        // returns TYPE_E_ELEMENTNOTFOUND for one the shell has not assigned
        // to a desktop. All of those are `Unknown`, not "no".
        match unsafe { manager.IsWindowOnCurrentVirtualDesktop(hwnd) } {
            Ok(on_current) if on_current.as_bool() => Desktop::Current,
            Ok(_) => Desktop::Other,
            Err(e) => {
                if beckon_core::verbose() {
                    eprintln!(
                        "verbose: IsWindowOnCurrentVirtualDesktop failed for {:?} ({}); \
                         treating the window as cloaked, i.e. dropping it",
                        hwnd, e
                    );
                }
                Desktop::Unknown
            }
        }
    }
}

/// Enumerate all visible, titled top-level windows that are not the shell's
/// own (`is_shell_window`), and not cloaked — **except** for windows whose
/// only reason for being cloaked is that they sit on another virtual desktop,
/// which are kept (`beckon_core::cloak::admit_window`).
///
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
    // One COM object for the whole enumeration, and only if a cloaked window
    // asks for it. See `DesktopOracle`.
    let mut oracle = DesktopOracle::new();
    let mut windows = Vec::new();

    for hwnd in hwnds {
        if let Some(info) = build_window_info(hwnd, &mut process_cache, &mut oracle) {
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
    oracle: &mut DesktopOracle,
) -> Option<WindowInfo> {
    unsafe {
        // Must be visible.
        if !IsWindowVisible(hwnd).as_bool() {
            return None;
        }

        // NOTE: the cloak test used to sit HERE, immediately after the
        // visibility test. It now runs after the four cheap tests below,
        // because it grew a COM call and those tests are pure reads. Every
        // test in this function is an independent predicate of `hwnd`
        // ANDed with the others, so reordering them cannot change which
        // windows survive — only how much is paid for the ones that do not.
        //
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

        let mut pid: u32 = 0;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == 0 {
            return None;
        }

        // Get class name.
        let mut class_buf = [0u16; 256];
        let class_len = GetClassNameW(hwnd, &mut class_buf);
        let class_name = String::from_utf16_lossy(&class_buf[..class_len as usize]);

        // Skip the shell's own windows.
        if is_shell_window(&class_name) {
            return None;
        }

        // Cloaked windows are dropped -- unless the ONLY thing wrong with
        // them is that they are parked on another virtual desktop, in which
        // case they are the app the user is asking for and dropping them
        // makes the hotkey launch a second copy. `DWMWA_CLOAKED` cannot tell
        // those two apart (a suspended UWP app reports the same `0x2`), so a
        // second, different question decides it. All of that lives in
        // `beckon_core::cloak` -- read its module doc before touching this.
        let mut cloaked: u32 = 0;
        let _ = DwmGetWindowAttribute(
            hwnd,
            DWMWA_CLOAKED,
            &mut cloaked as *mut u32 as *mut _,
            std::mem::size_of::<u32>() as u32,
        );
        // The closure is what keeps the COM round-trip off the uncloaked
        // majority: `admit_window` returns before calling it when `cloaked`
        // is 0. Do not hoist `oracle.locate(hwnd)` out to a local.
        if !cloak::admit_window(cloaked, || oracle.locate(hwnd)) {
            return None;
        }

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

/// Is this class one of the shell's own windows rather than an app's?
///
/// **The desktop passes every other filter in `build_window_info`** — class
/// `Progman`, caption "Program Manager", visible, uncloaked, not a tool
/// window, unowned — so without this it sat in `enum_visible_windows`
/// permanently. Step 5b then always found an "other app" to toggle to, step
/// 5c (minimize) was unreachable, and `beckon list` printed a running app
/// called "Program Manager". It is the same role `_NET_WM_WINDOW_TYPE` plays
/// in `beckon-linux`'s X11 backend, added for the same failure: beckon
/// focuses a shell window, reports success, and nothing moves.
///
/// `Windows.UI.Core.CoreWindow` was proposed alongside these and is
/// deliberately left out. A UWP app that is not hosted by
/// ApplicationFrameHost presents one as its own top-level window, so denying
/// it would make beckon launch a second copy on every keypress — the more
/// expensive of the two failures, and the one CLAUDE.md records Hyprland's
/// `visible` filter causing.
fn is_shell_window(class_name: &str) -> bool {
    ["Progman", "WorkerW", "Shell_TrayWnd"]
        .iter()
        .any(|c| class_name.eq_ignore_ascii_case(c))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_desktop_is_not_an_app_beckon_can_toggle_back_to() {
        assert!(is_shell_window("Progman"));
        assert!(is_shell_window("WorkerW"));
        assert!(is_shell_window("Shell_TrayWnd"));
        // GetClassNameW reports the registered spelling, but every other
        // class comparison in this file is case-insensitive.
        assert!(is_shell_window("progman"));
    }

    #[test]
    fn an_ordinary_app_window_is_not_a_shell_window() {
        assert!(!is_shell_window("CabinetWClass"));
        assert!(!is_shell_window("Notepad"));
        assert!(!is_shell_window("ApplicationFrameWindow"));
        // Left out on purpose -- see `is_shell_window`'s own comment.
        assert!(!is_shell_window("Windows.UI.Core.CoreWindow"));
    }
}
