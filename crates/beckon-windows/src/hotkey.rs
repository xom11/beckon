//! Global hotkeys on Windows: RegisterHotKey against the tray window's HWND
//! (not the thread queue), WM_HOTKEY/WM_TIMER dispatched by run_forever on
//! this thread as the fast path, with wndproc as a second dispatcher for
//! exactly the case run_forever can't be pumping: while a hotkey callback is
//! itself inside a nested message loop (ShellExecuteW, or an out-of-process
//! COM call such as IApplicationActivationManager::ActivateApplication,
//! which pumps this thread's queue internally to avoid an RPC deadlock). A
//! message posted with hwnd == NULL is a *thread* message; DispatchMessage
//! silently drops it (its one documented exception is a WM_TIMER carrying a
//! TIMERPROC, which we don't use). Registering against a real top-level
//! window instead makes WM_HOTKEY/WM_TIMER *window* messages, which
//! DispatchMessage delivers to wndproc no matter which loop is pumping — so
//! a hotkey pressed mid-launch is no longer silently lost.
//!
//! Unlike the mac twin, the callback here is therefore explicitly ALLOWED to
//! enter a nested pump — that's the whole point. The cost is that dispatch
//! must tolerate re-entering itself: everything is single-threaded and
//! callbacks live in thread_local slots, so dispatch takes the callback out
//! of its slot before invoking it. A message that reaches wndproc while
//! we're already inside a callback (the reentrant case above) can't then
//! double-borrow the RefCell and panic across the extern "system" boundary
//! — it goes on a small pending queue instead and runs immediately after the
//! in-flight one returns.

use beckon_core::shortcuts::KeyDef;
use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use windows::core::w;
use windows::core::PCWSTR;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::RemoteDesktop::ProcessIdToSessionId;
use windows::Win32::System::Threading::GetCurrentProcessId;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    RegisterHotKey, UnregisterHotKey, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT, MOD_WIN,
};
use windows::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_MODIFY,
    NOTIFYICONDATAW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu, DispatchMessageW,
    GetCursorPos, GetMessageW, LoadIconW, PeekMessageW, PostMessageW, PostQuitMessage,
    RegisterClassW, RegisterWindowMessageW, SetForegroundWindow, SetTimer, TrackPopupMenu,
    TranslateMessage, CW_USEDEFAULT, IDI_APPLICATION, MF_CHECKED, MF_GRAYED, MF_SEPARATOR,
    MF_STRING, MSG, PM_REMOVE, TPM_RIGHTBUTTON, WINDOW_EX_STYLE, WM_APP, WM_COMMAND,
    WM_CONTEXTMENU, WM_HOTKEY, WM_LBUTTONDBLCLK, WM_NULL, WM_RBUTTONUP, WM_TIMER, WNDCLASSW,
    WS_OVERLAPPED,
};

// Named so the thread_locals below (and install()'s identical parameter
// type) don't repeat the full `Box<dyn FnMut(...)>`/`Vec<(usize, ...)>`
// spelling — clippy::type_complexity flags the inline form. No behavior
// change: these are exactly the types that were written out before.
type HotkeyCallback = Box<dyn FnMut(u32)>;
type TickCallbacks = Vec<(usize, Box<dyn FnMut()>)>;

/// One row of the tray context menu. `hotkey.rs` draws it and reports the
/// click; what any row *means* is entirely the caller's business, which is
/// why there is no enum of actions here.
pub struct MenuEntry {
    pub id: u32,
    pub label: String,
    /// `None` for a plain item, `Some(bool)` for a check box.
    pub checked: Option<bool>,
    pub enabled: bool,
}

impl MenuEntry {
    /// A horizontal rule. Recognised by its empty label.
    pub fn separator() -> Self {
        Self {
            id: 0,
            label: String::new(),
            checked: None,
            enabled: false,
        }
    }
}

/// Delivered to `on_click` when the tray icon is double-clicked. Callers
/// must number their real entries below this.
pub const MENU_ID_DOUBLE_CLICK: u32 = u32::MAX;

/// Our tray icon's callback message. WM_APP+1 rather than WM_USER+n: WM_USER
/// is only private to a window *class*, and this window's class is shared
/// with nothing, but WM_APP is private to the application, which is the
/// guarantee actually wanted here.
const WM_TRAY: u32 = WM_APP + 1;

type MenuBuilder = Box<dyn Fn() -> Vec<MenuEntry>>;
type MenuHandler = Box<dyn FnMut(u32)>;

thread_local! {
    static HOTKEY_CB: RefCell<Option<HotkeyCallback>> = const { RefCell::new(None) };
    // Ids that arrived at wndproc/run_forever while HOTKEY_CB was already
    // out being run (a reentrant nested-pump delivery) — drained right
    // after the in-flight callback returns, so nothing is skipped.
    static HOTKEY_PENDING: RefCell<VecDeque<u32>> = const { RefCell::new(VecDeque::new()) };
    static TICK_CBS: RefCell<TickCallbacks> = const { RefCell::new(Vec::new()) };
    static MENU_BUILD: RefCell<Option<MenuBuilder>> = const { RefCell::new(None) };
    static MENU_CB: RefCell<Option<MenuHandler>> = const { RefCell::new(None) };
    // Same role as HOTKEY_PENDING: TrackPopupMenu runs its own modal message
    // pump, so a second click can reach dispatch_menu while the first
    // callback is still on the stack. Queue rather than drop.
    static MENU_PENDING: RefCell<VecDeque<u32>> = const { RefCell::new(VecDeque::new()) };
    static TICK_NEXT_ID: Cell<usize> = const { Cell::new(1) }; // 0 is SetTimer's failure sentinel
    // Set once in install(); add_tick (a free fn with no `self`) needs it to
    // register window timers against the same hwnd hotkeys use.
    static TRAY_HWND: Cell<HWND> = const { Cell::new(HWND(std::ptr::null_mut())) };
    // RegisterWindowMessageW("TaskbarCreated") result, or 0 if that call
    // failed. wndproc re-adds the tray icon whenever Explorer sends it.
    static TASKBAR_CREATED_MSG: Cell<u32> = const { Cell::new(0) };
    // Mirrors HotkeyManager::ids outside the instance: run_forever's WM_QUIT
    // branch has no `self` (it's an associated fn with no receiver) but
    // still needs the live id list to unregister everything for an orderly
    // exit, since std::process::exit skips Drop entirely.
    static REGISTERED_IDS: RefCell<Vec<u32>> = const { RefCell::new(Vec::new()) };
    // The tooltip text currently displayed. Kept so a TaskbarCreated re-add
    // (Explorer restart, logon race) restores the live status instead of
    // reverting to the startup placeholder — the icon coming back with a
    // stale tooltip would be a worse signal than no icon at all.
    static TRAY_TIP: RefCell<String> = RefCell::new(String::from("beckon serve"));
}

fn dispatch_hotkey(id: u32) {
    let Some(mut cb) = HOTKEY_CB.with(|slot| slot.borrow_mut().take()) else {
        // Already dispatching one level up (nested pump reentered while that
        // callback was running) — queue it rather than drop it.
        HOTKEY_PENDING.with(|p| p.borrow_mut().push_back(id));
        return;
    };
    cb(id);
    HOTKEY_CB.with(|slot| *slot.borrow_mut() = Some(cb));
    while let Some(next) = HOTKEY_PENDING.with(|p| p.borrow_mut().pop_front()) {
        dispatch_hotkey(next);
    }
}

fn dispatch_tick(id: usize) {
    // Take the whole table out before calling anything in it: a tick
    // callback that triggers a nested pump (unlikely for a 1s reload poll,
    // but not ruled out) must not re-enter this RefCell while it's borrowed.
    let mut cbs = TICK_CBS.with(|c| std::mem::take(&mut *c.borrow_mut()));
    // Load-bearing, not a defect to fix: beckon-cli's serve.rs relies on
    // this take-then-run order to make tick delivery non-reentrant.
    // reload() (serve.rs) holds RefCell borrows live across register_all()
    // on the strength of exactly this guarantee — a reentrant tick, should
    // one ever land here mid-callback, must see an empty TICK_CBS and be a
    // no-op rather than re-enter reload() and hit an already-held borrow
    // (BorrowMutError -> panic -> abort across the extern "system"
    // boundary, the same class of bug serve.rs's module doc describes for
    // on_hotkey/backend.beckon()). Currently unreachable in practice —
    // only one tick is ever registered, the reload poll — but if that ever
    // changes (a second tick, or a callback here triggering a nested
    // pump), restructure serve.rs's reload() first, the same way on_hotkey
    // was restructured, before changing this ordering. Until then, a
    // reentrant dispatch_tick for a *different* id, triggered by a nested
    // pump inside one of these callbacks, finds TICK_CBS empty and
    // silently skips that other tick — a known, accepted side effect of
    // the same take-then-run that keeps serve.rs safe.
    for (tick_id, cb) in cbs.iter_mut() {
        if *tick_id == id {
            cb();
        }
    }
    TICK_CBS.with(|c| {
        let mut slot = c.borrow_mut();
        cbs.extend(slot.drain(..)); // keep anything added reentrantly, in order
        *slot = cbs;
    });
}

fn dispatch_menu(id: u32) {
    // Take-then-run, exactly as dispatch_hotkey and dispatch_tick do: a
    // menu action may itself pump (ShellExecuteW does), and re-entering a
    // live RefCell borrow would panic across the extern "system" boundary,
    // which aborts the process rather than the callback.
    let Some(mut cb) = MENU_CB.with(|slot| slot.borrow_mut().take()) else {
        MENU_PENDING.with(|p| p.borrow_mut().push_back(id));
        return;
    };
    cb(id);
    MENU_CB.with(|slot| *slot.borrow_mut() = Some(cb));
    while let Some(next) = MENU_PENDING.with(|p| p.borrow_mut().pop_front()) {
        dispatch_menu(next);
    }
}

fn show_menu(hwnd: HWND) {
    let Some(entries) = MENU_BUILD.with(|b| b.borrow().as_ref().map(|f| f())) else {
        return;
    };
    unsafe {
        let Ok(menu) = CreatePopupMenu() else {
            eprintln!("hotkey: CreatePopupMenu failed - no tray menu this time");
            return;
        };
        for e in &entries {
            if e.label.is_empty() {
                let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
                continue;
            }
            let mut flags = MF_STRING;
            if !e.enabled {
                flags |= MF_GRAYED;
            }
            if e.checked == Some(true) {
                flags |= MF_CHECKED;
            }
            let label: Vec<u16> = e.label.encode_utf16().chain(std::iter::once(0)).collect();
            let _ = AppendMenuW(menu, flags, e.id as usize, PCWSTR(label.as_ptr()));
        }
        let mut pt = POINT::default();
        let _ = GetCursorPos(&mut pt);
        // Both of these are required, and neither is folklore: without the
        // SetForegroundWindow the menu never dismisses when the user clicks
        // away, and without the trailing PostMessage the *next* menu fails
        // to appear. Documented in Microsoft KB135788.
        let _ = SetForegroundWindow(hwnd);
        let _ = TrackPopupMenu(menu, TPM_RIGHTBUTTON, pt.x, pt.y, Some(0), hwnd, None);
        let _ = PostMessageW(Some(hwnd), WM_NULL, WPARAM(0), LPARAM(0));
        let _ = DestroyMenu(menu);
    }
}

/// Install the tray context menu.
///
/// `build` runs every time the menu opens rather than once at install, so
/// check marks reflect state at the moment of the click instead of at
/// startup. `on_click` receives the `MenuEntry::id` that was chosen, or
/// `MENU_ID_DOUBLE_CLICK` for a double-click on the icon itself.
///
/// Call after `HotkeyManager::install`, which creates the window this needs.
pub fn set_menu(build: MenuBuilder, on_click: MenuHandler) {
    MENU_BUILD.with(|b| *b.borrow_mut() = Some(build));
    MENU_CB.with(|c| *c.borrow_mut() = Some(on_click));
}

/// Ask the message loop to exit. `run_forever`'s WM_QUIT arm already
/// unregisters every hotkey and removes the tray icon, so this is the whole
/// of an orderly shutdown.
pub fn request_quit() {
    unsafe { PostQuitMessage(0) };
}

extern "system" fn wndproc(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    if msg == WM_HOTKEY {
        dispatch_hotkey(w.0 as u32);
        return LRESULT(0);
    }
    if msg == WM_TIMER {
        dispatch_tick(w.0);
        return LRESULT(0);
    }
    if msg == WM_TRAY {
        // lParam carries the mouse event; wParam is the icon's uID.
        match l.0 as u32 {
            WM_RBUTTONUP | WM_CONTEXTMENU => show_menu(hwnd),
            WM_LBUTTONDBLCLK => dispatch_menu(MENU_ID_DOUBLE_CLICK),
            _ => {}
        }
        return LRESULT(0);
    }
    if msg == WM_COMMAND {
        // TrackPopupMenu without TPM_RETURNCMD posts the chosen id here.
        // The high word is the notification code and is 0 for a menu.
        dispatch_menu((w.0 & 0xFFFF) as u32);
        return LRESULT(0);
    }
    let taskbar_created = TASKBAR_CREATED_MSG.with(|c| c.get());
    if taskbar_created != 0 && msg == taskbar_created {
        // Explorer (re)created the notification area — logon race or an
        // Explorer restart both land here. Re-add; hotkeys were never
        // affected, only the icon.
        tray_add(hwnd);
        return LRESULT(0);
    }
    unsafe { DefWindowProcW(hwnd, msg, w, l) }
}

/// Copy `text` into a fixed-size UTF-16 tip buffer, always NUL-terminated
/// and always clearing whatever the buffer held before.
///
/// `szTip` is a fixed array, not a pointer: a shorter second call would
/// otherwise leave the tail of the first call's text in place and the tray
/// would show a concatenation of both.
fn fill_tip(dst: &mut [u16; 128], text: &str) {
    let utf16: Vec<u16> = text.encode_utf16().collect();
    let max = dst.len() - 1; // leave room for the NUL terminator
    let n = utf16.len().min(max);
    dst[..n].copy_from_slice(&utf16[..n]);
    for slot in dst[n..].iter_mut() {
        *slot = 0;
    }
}

fn tray_add(hwnd: HWND) {
    let mut nid = NOTIFYICONDATAW {
        cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: hwnd,
        uID: 1,
        // NIF_MESSAGE is what turns the icon from a lamp into a control:
        // Shell_NotifyIcon posts WM_TRAY to this hwnd for every mouse event
        // on the icon, and wndproc turns the right-click into the menu.
        uFlags: NIF_ICON | NIF_TIP | NIF_MESSAGE,
        uCallbackMessage: WM_TRAY,
        ..Default::default()
    };
    nid.hIcon = unsafe { LoadIconW(None, IDI_APPLICATION) }.unwrap_or_default();
    TRAY_TIP.with(|t| fill_tip(&mut nid.szTip, &t.borrow()));
    // Best effort: a missing tray icon must not take the hotkeys down — but
    // it must not go silent either, or "no icon" reads as "daemon is dead"
    // and the user starts a second instance. Two known-benign causes: no
    // taskbar exists yet (logon race, explorer.exe not up), or no taskbar
    // exists at all (non-interactive window station — see the session check
    // in `install`).
    if !unsafe { Shell_NotifyIconW(NIM_ADD, &nid) }.as_bool() {
        eprintln!(
            "hotkey: Shell_NotifyIconW(NIM_ADD) failed - no tray icon; hotkeys are unaffected"
        );
    }
}

pub struct HotkeyManager {
    ids: Vec<u32>,
    tray_hwnd: HWND,
}

impl HotkeyManager {
    pub fn install(cb: HotkeyCallback) -> Result<Self, String> {
        HOTKEY_CB.with(|slot| *slot.borrow_mut() = Some(cb));

        // A Scheduled Task set to "Run whether user is logged on or not"
        // runs in session 0, on a non-interactive window station: every call
        // below still returns success (CreateWindowExW, RegisterHotKey; a
        // Shell_NotifyIconW failure is reported but non-fatal), yet no
        // keyboard input and no taskbar ever reach this process. Non-fatal
        // on purpose here too — same posture as the mac twin's
        // TransformProcessType check — but staying silent would leave a
        // dead daemon with a clean bill of health, so warn once up front.
        let mut session_id = 0u32;
        let pid = unsafe { GetCurrentProcessId() };
        if unsafe { ProcessIdToSessionId(pid, &mut session_id) }.is_ok() && session_id == 0 {
            eprintln!(
                "hotkey: running in session 0 (no interactive desktop) — hotkeys will never fire; \
                 use an 'At log on' trigger, not 'Run whether user is logged on or not'"
            );
        }

        // Hidden window: hosts the tray icon AND gives RegisterHotKey a real
        // HWND, so WM_HOTKEY/WM_TIMER become window messages instead of
        // thread messages (see module doc). Never shown.
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
        TRAY_HWND.with(|c| c.set(hwnd));
        TASKBAR_CREATED_MSG.with(|c| {
            c.set(unsafe { RegisterWindowMessageW(w!("TaskbarCreated")) });
        });
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
        // Against tray_hwnd, not None: WM_HOTKEY is posted as a WINDOW
        // message (msg.hwnd == tray_hwnd), which DispatchMessage delivers to
        // wndproc from ANY pump on this thread — including one a hotkey
        // callback enters via ShellExecuteW/COM. run_forever's own loop
        // still short-circuits it before DispatchMessageW on the normal
        // path (see run_forever); the wndproc branch only fires when some
        // other pump got to the message first.
        unsafe { RegisterHotKey(Some(self.tray_hwnd), id as i32, mods, key.win) }
            .map_err(|e| format!("RegisterHotKey failed: {e}"))?;
        self.ids.push(id);
        REGISTERED_IDS.with(|c| c.borrow_mut().push(id));
        Ok(())
    }

    pub fn unregister_all(&mut self) {
        for id in self.ids.drain(..) {
            if let Err(e) = unsafe { UnregisterHotKey(Some(self.tray_hwnd), id as i32) } {
                eprintln!("hotkey: UnregisterHotKey({id}) failed: {e}");
            }
        }
        REGISTERED_IDS.with(|c| c.borrow_mut().clear());
        // UnregisterHotKey stops FUTURE WM_HOTKEY generation; it does not
        // touch one already sitting in the queue. Without this, a press
        // queued right before a config reload gets delivered after
        // register_all() has installed the new table and is reinterpreted
        // against it by positional id — silently focusing the wrong app.
        let mut msg = MSG::default();
        while unsafe {
            PeekMessageW(
                &mut msg,
                Some(self.tray_hwnd),
                WM_HOTKEY,
                WM_HOTKEY,
                PM_REMOVE,
            )
        }
        .as_bool()
        {}
        // A press that already reached dispatch_hotkey but is waiting on
        // HOTKEY_PENDING (a nested-pump reentrant delivery queued while an
        // in-flight callback — e.g. launch_appx()'s blocking
        // ActivateApplication call — is still running) is dropped for the
        // same reason as the PeekMessageW drain above: on_hotkey resolves
        // the id against whatever shortcuts table is live when it finally
        // runs, which will be the table this reload is about to install,
        // not the one live when the key was actually pressed — so
        // delivering it could silently focus the wrong app. Unlike the
        // OS-queue drain, though, this discards a keypress that has
        // unambiguously already happened rather than a duplicate still
        // sitting unprocessed, so — matching every other best-effort path
        // in this file (tray-add failure, session-0, SetTimer failure) —
        // say so instead of dropping it in silence.
        HOTKEY_PENDING.with(|p| {
            let dropped: Vec<u32> = p.borrow_mut().drain(..).collect();
            if !dropped.is_empty() {
                eprintln!(
                    "hotkey: dropping {} in-flight hotkey press(es) ({dropped:?}) that raced \
                     this config reload — press again if still needed",
                    dropped.len()
                );
            }
        });
    }

    pub fn run_forever() -> ! {
        let mut msg = MSG::default();
        loop {
            // GetMessageW's BOOL return is really three states, not two:
            // -1 means the call itself failed (a broken queue — e.g. an
            // invalid hwnd got in somehow — is unrecoverable), 0 is
            // WM_QUIT, anything else is a real message with `msg` filled
            // in. `.as_bool()` treats -1 the same as "got a message" (it's
            // nonzero), which would have looped forever re-processing
            // stale `msg` contents instead of surfacing the failure — match
            // the raw i32 instead.
            let ret = unsafe { GetMessageW(&mut msg, None, 0, 0) }.0;
            match ret {
                -1 => {
                    eprintln!("hotkey: GetMessageW failed - message queue is broken, exiting");
                    std::process::exit(1);
                }
                0 => {
                    // WM_QUIT. An orderly exit, not an `unreachable!()`
                    // panic: this is the only exit path a Task
                    // Scheduler-run daemon has, and std::process::exit
                    // skips Drop entirely, so the cleanup Drop would have
                    // done has to happen here explicitly.
                    let tray_hwnd = TRAY_HWND.with(|c| c.get());
                    for id in REGISTERED_IDS.with(|c| std::mem::take(&mut *c.borrow_mut())) {
                        let _ = unsafe { UnregisterHotKey(Some(tray_hwnd), id as i32) };
                    }
                    let nid = NOTIFYICONDATAW {
                        cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
                        hWnd: tray_hwnd,
                        uID: 1,
                        ..Default::default()
                    };
                    let _ = unsafe { Shell_NotifyIconW(NIM_DELETE, &nid) };
                    std::process::exit(0);
                }
                _ => {
                    // Fast path: handle WM_HOTKEY/WM_TIMER here before
                    // DispatchMessageW — but only when the message is
                    // addressed to OUR window. This thread becomes an STA
                    // once the backend calls CoInitializeEx, and COM/shell
                    // machinery (ShellExecuteW, an out-of-process
                    // IApplicationActivationManager call) can create hidden
                    // windows on this same thread whose own WM_TIMERs must
                    // still reach their wndprocs via DispatchMessageW —
                    // matching on msg.message alone would silently steal
                    // those instead of routing them. The wndproc branches
                    // (module doc) exist for exactly the case this loop
                    // ISN'T the one pumping.
                    let ours = msg.hwnd == TRAY_HWND.with(|c| c.get());
                    match msg.message {
                        WM_HOTKEY if ours => dispatch_hotkey(msg.wParam.0 as u32),
                        WM_TIMER if ours => dispatch_tick(msg.wParam.0),
                        _ => {
                            let _ = unsafe { TranslateMessage(&msg) };
                            unsafe { DispatchMessageW(&msg) };
                        }
                    }
                }
            }
        }
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

/// Repeating tick on the tray window's message queue (SetTimer, a window
/// timer — not a thread timer; see module doc for why that distinction
/// matters). Call after `HotkeyManager::install`, which is what sets the
/// hwnd this needs.
pub fn add_tick(seconds: f64, cb: Box<dyn FnMut()>) {
    let hwnd = TRAY_HWND.with(|c| c.get());
    let elapse_ms = (seconds * 1000.0) as u32;
    let requested_id = TICK_NEXT_ID.with(|n| {
        let id = n.get();
        n.set(id + 1);
        id
    });
    // SetTimer returns 0 on failure (e.g. the process's timer quota is
    // exhausted) and, for a window timer, echoes back the id we asked for on
    // success — which starts at 1, so 0 is never a legitimate id here. A
    // silently discarded failure here means this tick would never fire
    // again with no diagnostic at all (beckon-cli's config-reload poll,
    // specifically).
    let id = unsafe { SetTimer(Some(hwnd), requested_id, elapse_ms, None) };
    if id == 0 {
        eprintln!(
            "hotkey: SetTimer failed (requested id {requested_id}) - this tick will never fire"
        );
        return;
    }
    TICK_CBS.with(|cbs| cbs.borrow_mut().push((id, cb)));
}

/// Update the tray tooltip. Best effort: a tooltip that will not update must
/// not take the hotkeys down, but it must not be silent either — the whole
/// point of the tooltip is that it is the honest answer to "is this alive and
/// how many keys does it hold".
pub fn set_status(text: &str) {
    TRAY_TIP.with(|t| *t.borrow_mut() = text.to_string());
    let hwnd = TRAY_HWND.with(|c| c.get());
    if hwnd.0.is_null() {
        return; // install() has not run yet; tray_add will pick the text up
    }
    let mut nid = NOTIFYICONDATAW {
        cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: hwnd,
        uID: 1,
        uFlags: NIF_TIP,
        ..Default::default()
    };
    fill_tip(&mut nid.szTip, text);
    if !unsafe { Shell_NotifyIconW(NIM_MODIFY, &nid) }.as_bool() {
        eprintln!("hotkey: Shell_NotifyIconW(NIM_MODIFY) failed - tooltip is stale");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fill_tip_writes_text_and_terminates() {
        let mut buf = [0xFFFFu16; 128];
        fill_tip(&mut buf, "beckon - 5 shortcuts");
        let text: String = String::from_utf16(&buf[..20]).unwrap();
        assert_eq!(text, "beckon - 5 shortcuts");
        assert_eq!(buf[20], 0, "must be NUL-terminated right after the text");
    }

    #[test]
    fn fill_tip_truncates_and_still_terminates() {
        let mut buf = [0xFFFFu16; 128];
        let long = "x".repeat(500);
        fill_tip(&mut buf, &long);
        assert_eq!(buf[127], 0, "the last slot must always be the NUL");
        assert!(buf[..127].iter().all(|&c| c == b'x' as u16));
    }

    #[test]
    fn fill_tip_clears_the_tail_of_a_reused_buffer() {
        let mut buf = [0u16; 128];
        fill_tip(&mut buf, "a long previous tooltip");
        fill_tip(&mut buf, "hi");
        assert_eq!(String::from_utf16(&buf[..2]).unwrap(), "hi");
        assert!(
            buf[2..].iter().all(|&c| c == 0),
            "stale text from the previous call must not survive"
        );
    }

    #[test]
    fn separator_is_recognisable_by_its_empty_label() {
        let sep = MenuEntry::separator();
        assert!(sep.label.is_empty());
        assert_eq!(sep.checked, None);
    }

    #[test]
    // MENU_ID_DOUBLE_CLICK and 1000 are both compile-time constants, so
    // clippy folds the comparison and flags it; the assertion still earns
    // its place as a readable, enforced doc of the "far outside any
    // plausible menu" invariant.
    #[allow(clippy::assertions_on_constants)]
    fn double_click_id_cannot_collide_with_a_real_entry() {
        // serve.rs numbers its entries from 1 upward; the reserved id must
        // sit far outside any plausible menu.
        assert_eq!(MENU_ID_DOUBLE_CLICK, u32::MAX);
        assert!(MENU_ID_DOUBLE_CLICK > 1000);
    }
}
