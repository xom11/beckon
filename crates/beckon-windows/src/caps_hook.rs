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
use beckon_core::capture::{CaptureState, HookOwners, Outcome};
use beckon_core::shortcuts::{CapsTap, Chord};
use std::cell::{Cell, RefCell};
use std::collections::HashSet;

/// Why the hook is installed. Re-exported so callers name one thing:
/// `caps_hook::HookReason::Caps`.
pub use beckon_core::capture::HookReason;
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
        }
    }
}

thread_local! {
    static HOOK: RefCell<Option<HHOOK>> = const { RefCell::new(None) };
    /// Which reasons want the hook. The decision logic is
    /// `beckon_core::capture::HookOwners`; this is only where it lives.
    static OWNERS: RefCell<HookOwners> = const { RefCell::new(HookOwners::new()) };
    static STATE: RefCell<CapsState> = RefCell::new(CapsState::default());
    static CONFIG: RefCell<Config> = RefCell::new(Config::default());
    /// Whether a shortcut field is recording. A `Cell`, not a `RefCell`:
    /// this is the first thing the callback reads on EVERY keystroke, and a
    /// `Cell<bool>` cannot be borrowed and therefore cannot be the second
    /// borrow that aborts the process.
    static CAP_ARMED: Cell<bool> = const { Cell::new(false) };
    /// The recording session. Meaningless while `CAP_ARMED` is false --
    /// `arm_capture` replaces it wholesale, so nothing carries over from the
    /// previous recording.
    static CAP: RefCell<CaptureState> = RefCell::new(CaptureState::armed());
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
        // Capture mode is consulted BEFORE `caps::decide`, and that order is
        // the whole of spec F.2. A second `WH_KEYBOARD_LL` hook is not an
        // option -- they chain -- so the one hook has two modes, and capture
        // has to be the first of them: otherwise pressing `Caps+T` in order
        // to BIND it is swallowed by the Caps arm and injected as
        // `ctrl+super+alt+t`, and the field records the alias instead of the
        // key the user pressed.
        if capture_armed() {
            // The per-event foreground gate: the third of spec F.4's three
            // focus layers, and the only one that fires when a UAC prompt or
            // an elevated window takes foreground WITHOUT sending
            // `WM_KILLFOCUS` or `WM_ACTIVATE`. Per event, because that is the
            // only granularity at which "the settings window is still
            // frontmost" is a fact rather than a memory.
            //
            // Failing the gate falls through to `caps::decide` rather than
            // cancelling: cancelling is a window-side decision, and posting
            // one from here without stepping the state machine would leave
            // `draining` disagreeing with what the window believes.
            //
            // `settings_window::hwnd()` takes a SHARED borrow of a
            // `thread_local` owned by this same thread. Audited 2026-08-12:
            // no `borrow_mut` of `UI` in that file is held across a call that
            // pumps the message queue -- which is the only way this callback,
            // dispatched as a sent message, could re-enter one.
            let fg = GetForegroundWindow();
            if let Some(hwnd) = crate::settings_window::hwnd().filter(|h| *h == fg) {
                // The borrow ends with the closure, before `PostMessageW`: a
                // second borrow taken across an `extern "system"` boundary
                // aborts the process rather than unwinding, and nothing
                // catches that.
                let outcome = CAP.with(|c| beckon_core::capture::step(ev, &mut c.borrow_mut()));
                // No trace here, deliberately, and not only for the budget:
                // while recording, EVERY keystroke reaches this arm, so a
                // per-event line would be the keylogger the trace below is
                // written to avoid.
                if outcome.post() {
                    let _ = PostMessageW(
                        Some(hwnd),
                        crate::settings_window::WM_CAPTURE,
                        WPARAM(outcome.code()),
                        LPARAM(0),
                    );
                }
                return match outcome {
                    // The ONE outcome that must reach the system. Its
                    // key-down was never swallowed -- the user was holding
                    // the key before recording began, or the key was refused
                    // and never entered the held set -- so swallowing its
                    // key-up would leave the system believing a modifier is
                    // held with no up ever coming. Nothing short of killing
                    // beckon gets it back. See `Outcome::PassThrough`.
                    Outcome::PassThrough => CallNextHookEx(None, code, wparam, lparam),
                    // Everything else is swallowed, down and up alike: that
                    // is what makes `alt+tab` recordable without the system
                    // seeing a bare Alt-up.
                    _ => LRESULT(1),
                };
            }
        }
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
///
/// This function must NEVER touch `STATE`. Clearing `CapsState.consumed`
/// mid-stream leaks an unpaired key-up into whichever application has focus:
/// the key-down was swallowed, so the up must be too, and only the next
/// Caps-down may clear the set. A reload arriving while a key is held is
/// ordinary, not exceptional.
pub fn set_bindings(bound: HashSet<u32>, hold: Chord, tap: CapsTap) {
    CONFIG.with(|c| *c.borrow_mut() = Config { bound, hold, tap });
}

/// Forget the key set, chord and tap behaviour. **Call this whenever the
/// Caps feature stops being wanted** -- switched off, or paused.
///
/// Giving up the Caps *reason* is not enough, and this is the whole reason
/// the function exists. The hook is shared: a chord capture installs it too,
/// and `hook_proc`'s capture arm is gated on `armed() && GetForegroundWindow()
/// == hwnd`, so a capture armed while the settings window is not frontmost
/// falls straight through to `caps::decide` -- with whatever `CONFIG` was
/// last handed. Leaving the old set in place means a config that says Caps
/// is off can still alias `Caps+T`, seconds at a time, whenever the user
/// records a shortcut. Clearing it leaves `decide` nothing to act on.
///
/// Like `set_bindings`, this must NEVER touch `STATE`: a key-down already
/// swallowed still needs its up swallowed, and only the next Caps-down may
/// clear that set.
pub fn clear_bindings() {
    CONFIG.with(|c| *c.borrow_mut() = Config::default());
}

/// Take a reason to hold the hook, installing it on the CURRENT thread —
/// which must have a message loop — if this is the first reason.
///
/// Idempotent per reason and across reasons: a second call is a no-op rather
/// than a second hook. **There must never be two `WH_KEYBOARD_LL` hooks**;
/// spec F.2 spells out both failures, and both are silent.
///
/// On failure the reason is handed back before returning, so nothing is left
/// believing it holds a hook that was never installed — which would make the
/// *next* reason's `install_for` a successful no-op over nothing.
pub fn install_for(reason: HookReason) -> Result<(), String> {
    // Never hold this borrow across the OS call: a re-entrant borrow across
    // an `extern "system"` boundary aborts the process rather than unwinding.
    let need = OWNERS.with(|o| o.borrow_mut().add(reason));
    if !need {
        return Ok(());
    }
    // A handle a previous `UnhookWindowsHookEx` failed to remove. Try once
    // more before installing, because chaining a second `WH_KEYBOARD_LL` on
    // top of a live one is exactly what spec F.2 forbids and what the
    // refcount exists to prevent.
    //
    // One attempt, on the install path, on the UI thread -- never a loop and
    // never on the callback path. The slot is cleared either way: past this
    // point a handle we have now failed twice to remove is not worth aiming
    // at again, and keeping it across the install below is the one ordering
    // that could turn a retry into a double-unhook -- `SetWindowsHookExW`
    // may hand back the same `HHOOK` value, and unhooking the stale copy
    // would then kill the hook we just installed.
    //
    // Bound on its own line, like `uninstall_for` does it: the borrow must
    // be released before the OS call, and an `if let` scrutinee keeps its
    // temporaries alive for the whole body.
    let stale = HOOK.with(|s| s.borrow_mut().take());
    if let Some(stale) = stale {
        if let Err(e) = unsafe { UnhookWindowsHookEx(stale) } {
            eprintln!("beckon serve: caps hook: leftover hook not removed: {e}");
        }
    }
    // A fresh press cycle: anything left over from a previous install would
    // have the machine believing Caps is still held. Only reached when the
    // hook is genuinely about to be installed -- a second reason arriving
    // later returns above, which is the whole point of the refcount.
    //
    // Before the OS call, not after: the callback can be dispatched the
    // instant `SetWindowsHookExW` returns, so resetting afterwards would
    // race a real keystroke and wipe it. If the call then fails, the reset
    // is harmless -- no hook exists to have observed anything.
    STATE.with(|s| *s.borrow_mut() = CapsState::default());
    let h = match unsafe { SetWindowsHookExW(WH_KEYBOARD_LL, Some(hook_proc), None, 0) } {
        Ok(h) => h,
        Err(e) => {
            OWNERS.with(|o| {
                o.borrow_mut().remove(reason);
            });
            return Err(format!("SetWindowsHookExW(WH_KEYBOARD_LL) failed: {e}"));
        }
    };
    HOOK.with(|s| *s.borrow_mut() = Some(h));
    Ok(())
}

/// Give up a reason, unhooking only when it was the last one. Safe when the
/// reason never held it, and safe when the hook is not installed.
///
/// **Only a real unhook resets `CapsState`.** Dropping the capture reason
/// while Caps still holds the hook must leave the Caps state machine exactly
/// as it was: clearing `consumed` mid-stream leaks an unpaired key-up into
/// whichever application has focus, and clearing `held` makes the next
/// Caps-up look like a tap. Same rule `set_bindings` documents, arrived at
/// from the other direction.
pub fn uninstall_for(reason: HookReason) {
    let unhook = OWNERS.with(|o| o.borrow_mut().remove(reason));
    if !unhook {
        return;
    }
    // Take the handle out first and drop the borrow, then call the OS: see
    // `install_for` on why no borrow may be live across the boundary.
    let h = HOOK.with(|s| s.borrow_mut().take());
    if let Some(h) = h {
        match unsafe { UnhookWindowsHookEx(h) } {
            Ok(()) => STATE.with(|s| *s.borrow_mut() = CapsState::default()),
            Err(e) => {
                // Put the handle back. Dropping it on the floor is what let
                // the next `install_for` chain a SECOND `WH_KEYBOARD_LL`
                // over a hook that may still be delivering -- the one thing
                // spec F.2 forbids, and the reason capture shares this hook
                // rather than installing its own. `install_for` makes one
                // further attempt before it installs anything.
                //
                // `CapsState` is deliberately NOT reset here: if the hook is
                // still live it is still swallowing, and clearing `consumed`
                // mid-stream leaks an unpaired key-up into whichever
                // application has focus -- the hazard `set_bindings`
                // documents. Only a real unhook resets it.
                //
                // Most plausibly this means Windows already removed the hook
                // itself past `LowLevelHooksTimeout`, in which case there is
                // nothing live and the handle is merely stale. There is no
                // API to tell the two apart (F.2, recorded not fixed), so
                // this takes the branch that cannot leave two hooks running.
                HOOK.with(|s| *s.borrow_mut() = Some(h));
                eprintln!("beckon serve: caps hook: UnhookWindowsHookEx failed: {e}");
            }
        }
    }
}

/// Whether a shortcut field is recording, i.e. whether the capture arm of
/// `hook_proc` runs at all. **Not** "is the hook installed": Caps may hold
/// it while nothing is recording, and `arm_capture` refuses to set this when
/// the install failed.
pub fn capture_armed() -> bool {
    CAP_ARMED.with(|a| a.get())
}

/// Start recording into a fresh `CaptureState`, taking the hook for as long
/// as it lasts.
///
/// **Returns whether the hook is actually installed.** `false` means
/// `SetWindowsHookExW` failed and nothing was armed, which the caller must
/// surface (`capture::HINT_UNAVAILABLE`) rather than showing a recording
/// field that can never record. Spec F.3 is explicit that there is no
/// fallback to message-queue capture: that path cannot see the Windows key,
/// so it would fail on precisely the chords beckon recommends, and it would
/// fail by recording the WRONG chord rather than by refusing.
///
/// The order is load-bearing in both directions. The state is replaced
/// *before* the install, so a hook that starts delivering the instant
/// `SetWindowsHookExW` returns cannot be met with the previous session's
/// held keys; and the flag is set *after* it, so a failed install leaves
/// nothing armed over a hook that does not exist. The gap in between is the
/// pre-arm behaviour -- events reach `caps::decide`, which is where they
/// were going a moment ago anyway.
pub fn arm_capture() -> bool {
    CAP.with(|c| *c.borrow_mut() = CaptureState::armed());
    if let Err(e) = install_for(HookReason::Capture) {
        // The caller shows the user one fixed sentence; this is the only
        // place the actual failure is ever named. ASCII, like every other
        // `serve` log line -- see the `--log` notes in CLAUDE.md.
        eprintln!("beckon serve: capture: cannot record: {e}");
        return false;
    }
    CAP_ARMED.with(|a| a.set(true));
    true
}

/// Stop recording and give the hook back. Caps keeps it if Caps wants it;
/// `uninstall_for` is what decides, and only a real unhook resets
/// `CapsState`.
///
/// The flag is cleared first so that no further keystroke can enter the
/// capture arm while the hook is being torn down. Safe to call when nothing
/// is armed -- `WM_CLOSE`, `WM_DESTROY` and the watchdog all call it without
/// asking, which is what spec F.4 means by "there is no path where the
/// window dies with the hook armed".
pub fn disarm_capture() {
    CAP_ARMED.with(|a| a.set(false));
    uninstall_for(HookReason::Capture);
}

/// Whether an `HHOOK` is currently held. **Not** "does anyone want one":
/// ask `HookOwners` for that. Note it can lie — past
/// `LowLevelHooksTimeout` Windows removes the hook silently and there is no
/// API to ask (spec F.2, recorded not fixed).
pub fn is_installed() -> bool {
    HOOK.with(|h| h.borrow().is_some())
}
