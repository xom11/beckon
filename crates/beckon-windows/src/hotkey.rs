//! Global hotkeys on Windows: RegisterHotKey against the thread queue (no
//! window, no hook — deliberately outside the VKey→kanata LLHOOK ordering),
//! WM_HOTKEY/WM_TIMER dispatched by run_forever on this thread. A hidden
//! window exists only to host the tray icon (liveness signal). Everything is
//! single-threaded; callbacks live in thread_local slots.

use beckon_core::shortcuts::KeyDef;
use std::cell::RefCell;
use windows::core::w;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    RegisterHotKey, UnregisterHotKey, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT, MOD_WIN,
};
use windows::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NOTIFYICONDATAW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, LoadIconW, RegisterClassW,
    SetTimer, TranslateMessage, CW_USEDEFAULT, IDI_APPLICATION, MSG, WINDOW_EX_STYLE, WM_APP,
    WM_HOTKEY, WM_TIMER, WNDCLASSW, WS_OVERLAPPED,
};

thread_local! {
    static HOTKEY_CB: RefCell<Option<Box<dyn FnMut(u32)>>> = const { RefCell::new(None) };
    static TICK_CBS: RefCell<Vec<(usize, Box<dyn FnMut()>)>> = const { RefCell::new(Vec::new()) };
}

extern "system" fn wndproc(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    unsafe { DefWindowProcW(hwnd, msg, w, l) }
}

fn tray_add(hwnd: HWND) {
    let mut nid = NOTIFYICONDATAW {
        cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: hwnd,
        uID: 1,
        uFlags: NIF_ICON | NIF_TIP | NIF_MESSAGE,
        uCallbackMessage: WM_APP,
        ..Default::default()
    };
    nid.hIcon = unsafe { LoadIconW(None, IDI_APPLICATION) }.unwrap_or_default();
    let tip: Vec<u16> = "beckon serve\0".encode_utf16().collect();
    nid.szTip[..tip.len()].copy_from_slice(&tip);
    // Best effort: a missing tray icon must not take the hotkeys down.
    let _ = unsafe { Shell_NotifyIconW(NIM_ADD, &nid) };
}

pub struct HotkeyManager {
    ids: Vec<u32>,
    tray_hwnd: HWND,
}

impl HotkeyManager {
    pub fn install(cb: Box<dyn FnMut(u32)>) -> Result<Self, String> {
        HOTKEY_CB.with(|slot| *slot.borrow_mut() = Some(cb));
        // Hidden window solely for the tray icon. Never shown.
        let hwnd = unsafe {
            let hinst = GetModuleHandleW(None).map_err(|e| e.to_string())?;
            let class = WNDCLASSW {
                lpfnWndProc: Some(wndproc),
                hInstance: hinst.into(),
                lpszClassName: w!("beckon-serve-tray"),
                ..Default::default()
            };
            RegisterClassW(&class); // 0 on re-register — harmless, single call per process
            CreateWindowExW(
                WINDOW_EX_STYLE(0),
                w!("beckon-serve-tray"),
                w!("beckon serve"),
                WS_OVERLAPPED,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                0,
                0,
                None,
                None,
                Some(hinst.into()),
                None,
            )
            .map_err(|e| format!("CreateWindowExW failed: {e}"))?
        };
        tray_add(hwnd);
        Ok(Self {
            ids: Vec::new(),
            tray_hwnd: hwnd,
        })
    }

    pub fn register(
        &mut self,
        id: u32,
        ctrl: bool,
        super_: bool,
        alt: bool,
        shift: bool,
        key: &KeyDef,
    ) -> Result<(), String> {
        let mut mods = MOD_NOREPEAT;
        if ctrl {
            mods |= MOD_CONTROL;
        }
        if super_ {
            mods |= MOD_WIN;
        }
        if alt {
            mods |= MOD_ALT;
        }
        if shift {
            mods |= MOD_SHIFT;
        }
        // hwnd = None: WM_HOTKEY lands on THIS THREAD's queue with the id in
        // wParam — run_forever picks it up before any window dispatch.
        unsafe { RegisterHotKey(None, id as i32, mods, key.win) }
            .map_err(|e| format!("RegisterHotKey failed: {e}"))?;
        self.ids.push(id);
        Ok(())
    }

    pub fn unregister_all(&mut self) {
        for id in self.ids.drain(..) {
            let _ = unsafe { UnregisterHotKey(None, id as i32) };
        }
    }

    pub fn run_forever() -> ! {
        let mut msg = MSG::default();
        unsafe {
            while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                match msg.message {
                    // Thread messages (hwnd == 0) never reach a wndproc —
                    // handle them here, before dispatch.
                    WM_HOTKEY => HOTKEY_CB.with(|slot| {
                        if let Some(cb) = slot.borrow_mut().as_mut() {
                            cb(msg.wParam.0 as u32);
                        }
                    }),
                    WM_TIMER => TICK_CBS.with(|cbs| {
                        for (id, cb) in cbs.borrow_mut().iter_mut() {
                            if *id == msg.wParam.0 {
                                cb();
                            }
                        }
                    }),
                    _ => {
                        let _ = TranslateMessage(&msg);
                        DispatchMessageW(&msg);
                    }
                }
            }
        }
        unreachable!("GetMessageW returned FALSE without WM_QUIT being expected");
    }
}

impl Drop for HotkeyManager {
    fn drop(&mut self) {
        let nid = NOTIFYICONDATAW {
            cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: self.tray_hwnd,
            uID: 1,
            ..Default::default()
        };
        let _ = unsafe { Shell_NotifyIconW(NIM_DELETE, &nid) };
    }
}

/// Repeating tick on this thread's message queue (SetTimer, thread timer).
pub fn add_tick(seconds: f64, cb: Box<dyn FnMut()>) {
    let elapse_ms = (seconds * 1000.0) as u32;
    let id = unsafe { SetTimer(None, 0, elapse_ms, None) };
    TICK_CBS.with(|cbs| cbs.borrow_mut().push((id, cb)));
}
