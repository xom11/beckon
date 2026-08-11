//! `WH_KEYBOARD_LL` shim for Caps-as-beckon-key.
//!
//! Every decision lives in `beckon_core::caps::decide`, which is pure and
//! tested on all three CI jobs. This file only translates
//! `KBDLLHOOKSTRUCT` into a `KeyEvent` and an `Action` into `SendInput`.
//!
//! **The callback must stay far inside `LowLevelHooksTimeout`** (300 ms by
//! default) or Windows silently unhooks us with no error anywhere. It does
//! one hash lookup and at most one `SendInput`; measured on a14, an 8-stroke
//! `SendInput` costs 5–13 ms. `backend.beckon()` — 57 ms typical, 945 ms on
//! the miss path — is never reached from here: the hook only injects the
//! chord, and the real work happens later on the ordinary `WM_HOTKEY` path.

use beckon_core::caps::{decide, Action, CapsState, Edge, KeyEvent};
use beckon_core::shortcuts::{CapsTap, Chord};
use std::cell::RefCell;
use std::collections::HashSet;
use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::WindowsAndMessaging::*;

/// Stamped into everything we inject so the hook ignores its own output.
/// Without it the first synthesized stroke re-enters `decide` and the whole
/// thing spirals.
pub const MARK: usize = 0xBECC0DE;

struct Config {
    bound: HashSet<u32>,
    hold: Chord,
    tap: CapsTap,
}

thread_local! {
    static HOOK: RefCell<Option<HHOOK>> = const { RefCell::new(None) };
    static STATE: RefCell<CapsState> = RefCell::new(CapsState::default());
    static CONFIG: RefCell<Config> = RefCell::new(Config {
        bound: HashSet::new(),
        hold: Chord::default(),
        tap: CapsTap::CapsLock,
    });
}

unsafe extern "system" fn hook_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    // MSDN: when code < 0 the hook must pass the message on without
    // inspecting it.
    if code == HC_ACTION as i32 {
        let kb = &*(lparam.0 as *const KBDLLHOOKSTRUCT);
        let edge = match wparam.0 as u32 {
            WM_KEYDOWN | WM_SYSKEYDOWN => Edge::Down,
            WM_KEYUP | WM_SYSKEYUP => Edge::Up,
            _ => return CallNextHookEx(None, code, wparam, lparam),
        };
        let ev = KeyEvent {
            vk: kb.vkCode,
            edge,
            injected_by_us: kb.dwExtraInfo == MARK,
            // The event's own timestamp, not `Instant::now()`: it is the
            // time the keystroke happened rather than the time our callback
            // got around to it, so a slow hook cannot turn a tap into a hold.
            time_ms: kb.time,
        };
        let action = CONFIG.with(|c| {
            let c = c.borrow();
            STATE.with(|s| decide(ev, &mut s.borrow_mut(), &c.bound, c.hold, c.tap))
        });
        // Trace only what beckon acted on, plus Caps itself. A trace of
        // every event that merely passed through would be a log of
        // everything the user types, virtual-key by virtual-key -- a
        // keylogger written to disk. There is no diagnostic worth that.
        if debug() && (ev.vk == beckon_core::caps::VK_CAPITAL || action != Action::PassThrough) {
            eprintln!(
                "beckon serve: caps hook: vk=0x{:02X} {:?}{} -> {}",
                ev.vk,
                ev.edge,
                if ev.injected_by_us { " (ours)" } else { "" },
                match &action {
                    Action::PassThrough => "pass".to_string(),
                    Action::Swallow => "swallow".to_string(),
                    Action::SwallowAndInject(s) => format!("swallow+inject {} strokes", s.len()),
                }
            );
        }
        match action {
            Action::PassThrough => {}
            Action::Swallow => return LRESULT(1),
            Action::SwallowAndInject(strokes) => {
                inject(&strokes);
                return LRESULT(1);
            }
        }
    }
    CallNextHookEx(None, code, wparam, lparam)
}

fn inject(strokes: &[beckon_core::caps::Stroke]) {
    let inputs: Vec<INPUT> = strokes
        .iter()
        .map(|k| INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VIRTUAL_KEY(k.vk as u16),
                    wScan: 0,
                    dwFlags: match k.edge {
                        Edge::Down => KEYBD_EVENT_FLAGS(0),
                        Edge::Up => KEYEVENTF_KEYUP,
                    },
                    time: 0,
                    dwExtraInfo: MARK,
                },
            },
        })
        .collect();
    let sent = unsafe { SendInput(&inputs, std::mem::size_of::<INPUT>() as i32) } as usize;
    // A short insert is how a keyboard gets stuck: the chord's downs land
    // and its ups do not. `SendInput` reports this only through its return
    // value -- UIPI blocks it without setting an error, and another thread
    // holding the input queue makes it return zero. Say so rather than
    // leaving the user to discover it by typing.
    if sent != inputs.len() {
        eprintln!(
            "beckon serve: caps hook: SendInput inserted {sent} of {} events - \
             modifiers may be left down",
            inputs.len()
        );
    }
    if debug() {
        eprintln!("beckon serve: caps hook: injected {strokes:?} ({sent} inserted)");
    }
}

/// Per-event tracing, off unless `BECKON_CAPS_DEBUG` is set to something
/// other than `0`. Read once: this is consulted from the hook callback,
/// which has a 300 ms budget and no business touching the environment
/// repeatedly.
fn debug() -> bool {
    use std::sync::OnceLock;
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| {
        std::env::var("BECKON_CAPS_DEBUG")
            .map(|v| v != "0")
            .unwrap_or(false)
    })
}

/// Replace the key set, chord and tap behaviour without touching the hook
/// itself. Called on every reload; installing is a separate decision.
pub fn set_bindings(bound: HashSet<u32>, hold: Chord, tap: CapsTap) {
    CONFIG.with(|c| *c.borrow_mut() = Config { bound, hold, tap });
}

/// Install the hook on the CURRENT thread, which must have a message loop.
/// Idempotent — a second call is a no-op rather than a second hook.
pub fn install() -> Result<(), String> {
    if is_installed() {
        return Ok(());
    }
    // A fresh press cycle: anything left over from a previous install would
    // have the machine believing Caps is still held.
    STATE.with(|s| *s.borrow_mut() = CapsState::default());
    let h = unsafe { SetWindowsHookExW(WH_KEYBOARD_LL, Some(hook_proc), None, 0) }
        .map_err(|e| format!("SetWindowsHookExW(WH_KEYBOARD_LL) failed: {e}"))?;
    HOOK.with(|s| *s.borrow_mut() = Some(h));
    Ok(())
}

/// Remove the hook. Safe when it is not installed.
pub fn uninstall() {
    HOOK.with(|s| {
        if let Some(h) = s.borrow_mut().take() {
            unsafe {
                let _ = UnhookWindowsHookEx(h);
            }
        }
    });
    STATE.with(|s| *s.borrow_mut() = CapsState::default());
}

pub fn is_installed() -> bool {
    HOOK.with(|h| h.borrow().is_some())
}
