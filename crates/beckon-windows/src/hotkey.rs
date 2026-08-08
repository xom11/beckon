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
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::RemoteDesktop::ProcessIdToSessionId;
use windows::Win32::System::Threading::GetCurrentProcessId;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    RegisterHotKey, UnregisterHotKey, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT, MOD_WIN,
};
use windows::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_ICON, NIF_TIP, NIM_ADD, NIM_DELETE, NOTIFYICONDATAW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, LoadIconW, PeekMessageW,
    RegisterClassW, RegisterWindowMessageW, SetTimer, TranslateMessage, CW_USEDEFAULT,
    IDI_APPLICATION, MSG, PM_REMOVE, WINDOW_EX_STYLE, WM_HOTKEY, WM_TIMER, WNDCLASSW,
    WS_OVERLAPPED,
};

thread_local! {
    static HOTKEY_CB: RefCell<Option<Box<dyn FnMut(u32)>>> = const { RefCell::new(None) };
    // Ids that arrived at wndproc/run_forever while HOTKEY_CB was already
    // out being run (a reentrant nested-pump delivery) — drained right
    // after the in-flight callback returns, so nothing is skipped.
    static HOTKEY_PENDING: RefCell<VecDeque<u32>> = const { RefCell::new(VecDeque::new()) };
    static TICK_CBS: RefCell<Vec<(usize, Box<dyn FnMut()>)>> = const { RefCell::new(Vec::new()) };
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

extern "system" fn wndproc(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    if msg == WM_HOTKEY {
        dispatch_hotkey(w.0 as u32);
        return LRESULT(0);
    }
    if msg == WM_TIMER {
        dispatch_tick(w.0);
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

fn tray_add(hwnd: HWND) {
    let mut nid = NOTIFYICONDATAW {
        cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: hwnd,
        uID: 1,
        // No NIF_MESSAGE/uCallbackMessage: the tray icon is a liveness
        // indicator only (no menu, no click handling in scope), and wndproc
        // has nothing to do with a callback message — requesting one would
        // just be a dead channel that reads as noise, not a feature.
        uFlags: NIF_ICON | NIF_TIP,
        ..Default::default()
    };
    nid.hIcon = unsafe { LoadIconW(None, IDI_APPLICATION) }.unwrap_or_default();
    // szTip is a fixed-size buffer; bound the copy instead of trusting the
    // source string to always fit, and NUL-terminate explicitly so
    // truncation (if the string ever changes) can't cut off the only NUL.
    let tip: Vec<u16> = "beckon serve".encode_utf16().collect();
    let max = nid.szTip.len() - 1; // leave room for the NUL terminator
    let n = tip.len().min(max);
    nid.szTip[..n].copy_from_slice(&tip[..n]);
    nid.szTip[n] = 0;
    // Best effort: a missing tray icon must not take the hotkeys down — but
    // it must not go silent either, or "no icon" reads as "daemon is dead"
    // and the user starts a second instance. Two known-benign causes: no
    // taskbar exists yet (logon race, explorer.exe not up), or no taskbar
    // exists at all (non-interactive window station — see the session check
    // in `install`).
    if !unsafe { Shell_NotifyIconW(NIM_ADD, &nid) }.as_bool() {
        eprintln!(
            "hotkey: Shell_NotifyIconW(NIM_ADD) failed — no tray icon; hotkeys are unaffected"
        );
    }
}

pub struct HotkeyManager {
    ids: Vec<u32>,
    tray_hwnd: HWND,
}

impl HotkeyManager {
    pub fn install(cb: Box<dyn FnMut(u32)>) -> Result<Self, String> {
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
                    eprintln!("hotkey: GetMessageW failed — message queue is broken, exiting");
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
            "hotkey: SetTimer failed (requested id {requested_id}) — this tick will never fire"
        );
        return;
    }
    TICK_CBS.with(|cbs| cbs.borrow_mut().push((id, cb)));
}
