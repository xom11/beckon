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
use std::cell::{Cell, RefCell};
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
    /// Whether the Caps feature is wanted at all -- `keyboard.caps && !paused`,
    /// as `sync_caps_hook` computes it.
    wanted: bool,
}

impl Default for Config {
    /// What `caps::decide` sees when the Caps feature is off: no bound keys,
    /// so no chord is ever injected, and `CapsTap::CapsLock`, so a Caps tap
    /// is still a Caps tap. Spelled once, here, because `clear_bindings`
    /// and the `thread_local!` below must not drift apart -- a `CapsTap`
    /// that stayed on the user's configured `escape` after they switched
    /// Caps off would keep remapping a key the config says is theirs again.
    fn default() -> Self {
        Config {
            bound: HashSet::new(),
            hold: Chord::default(),
            tap: CapsTap::default(),
            wanted: false,
        }
    }
}

thread_local! {
    static HOOK: RefCell<Option<HHOOK>> = const { RefCell::new(None) };
    static WANTED: Cell<bool> = const { Cell::new(false) };
    static STATE: RefCell<CapsState> = RefCell::new(CapsState::default());
    static CONFIG: RefCell<Config> = RefCell::new(Config::default());
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
            STATE.with(|s| {
                let mut st = s.borrow_mut();
                // `at_rest` is not optional: a reload or a pause can clear
                // `wanted` between a Caps-down and its up, and skipping then
                // would leak an unpaired key-up and abandon the defensive
                // `release_modifiers` burst. `decide` keeps running until it
                // has finished what it started, and only then goes quiet.
                if !c.wanted && st.at_rest() {
                    return Action::PassThrough;
                }
                decide(ev, &mut st, &c.bound, c.hold, c.tap)
            })
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
///
/// This function must NEVER touch `STATE`. Clearing `CapsState.consumed`
/// mid-stream leaks an unpaired key-up into whichever application has focus:
/// the key-down was swallowed, so the up must be too, and only the next
/// Caps-down may clear the set. A reload arriving while a key is held is
/// ordinary, not exceptional.
pub fn set_bindings(bound: HashSet<u32>, hold: Chord, tap: CapsTap) {
    CONFIG.with(|c| {
        *c.borrow_mut() = Config {
            bound,
            hold,
            tap,
            wanted: true,
        }
    });
}

/// Forget the key set, chord and tap behaviour. **Call this whenever the
/// Caps feature stops being wanted** -- switched off, or paused.
///
/// Like `set_bindings`, this must NEVER touch `STATE`: a key-down already
/// swallowed still needs its up swallowed, and only the next Caps-down may
/// clear that set. That is also why `Config::wanted` going false does not on
/// its own stop `hook_proc` calling `decide` -- it stops only once
/// `CapsState::at_rest` agrees there is nothing left owed.
pub fn clear_bindings() {
    CONFIG.with(|c| *c.borrow_mut() = Config::default());
}

/// Install the hook on the CURRENT thread — which must have a message loop.
/// Idempotent: a second call is a no-op rather than chaining a second
/// `WH_KEYBOARD_LL` hook.
pub fn install() -> Result<(), String> {
    if WANTED.with(|w| w.replace(true)) {
        return Ok(());
    }
    // A handle a previous `UnhookWindowsHookEx` failed to remove. Try once
    // more before installing, because chaining a second `WH_KEYBOARD_LL` on
    // top of a live one must never happen.
    let stale = HOOK.with(|s| s.borrow_mut().take());
    if let Some(stale) = stale {
        if let Err(e) = unsafe { UnhookWindowsHookEx(stale) } {
            eprintln!("beckon serve: caps hook: leftover hook not removed: {e}");
        }
    }
    // Before the OS call, not after: the callback can be dispatched the
    // instant `SetWindowsHookExW` returns, so resetting afterwards would
    // race a real keystroke and wipe it.
    STATE.with(|s| *s.borrow_mut() = CapsState::default());
    let h = match unsafe { SetWindowsHookExW(WH_KEYBOARD_LL, Some(hook_proc), None, 0) } {
        Ok(h) => h,
        Err(e) => {
            WANTED.with(|w| w.set(false));
            return Err(format!("SetWindowsHookExW(WH_KEYBOARD_LL) failed: {e}"));
        }
    };
    HOOK.with(|s| *s.borrow_mut() = Some(h));
    Ok(())
}

/// Remove the hook if installed. Safe when not installed.
pub fn uninstall() {
    if !WANTED.with(|w| w.replace(false)) {
        return;
    }
    let h = HOOK.with(|s| s.borrow_mut().take());
    if let Some(h) = h {
        match unsafe { UnhookWindowsHookEx(h) } {
            Ok(()) => STATE.with(|s| *s.borrow_mut() = CapsState::default()),
            Err(e) => {
                HOOK.with(|s| *s.borrow_mut() = Some(h));
                eprintln!("beckon serve: caps hook: UnhookWindowsHookEx failed: {e}");
            }
        }
    }
}

/// Whether an `HHOOK` is currently held.
pub fn is_installed() -> bool {
    HOOK.with(|h| h.borrow().is_some())
}
