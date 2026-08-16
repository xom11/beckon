//! Caps Lock as the beckon key, on macOS.
//!
//! The counterpart of `beckon_windows::caps_hook`, against the same design
//! and a different mechanism. Caps is an **alias for the configured chord**,
//! not a fifth modifier: the tap swallows `Caps+T` and injects
//! `ctrl+cmd+opt+T`, which `RegisterEventHotKey` is already listening for.
//! So `Combo`, `parse_shortcuts` and the registration path are untouched and
//! the config file is identical on a machine with the tick and one without.
//!
//! ## Why the decisions are here and not in `beckon_core::caps`
//!
//! They are the same decisions and deliberately not the same code. Measured
//! 2026-08-16 with `examples/caps_probe.rs`:
//!
//! - **Caps arrives as `kCGEventFlagsChanged`, never `keyDown`/`keyUp`.**
//!   `caps::KeyEvent` is `{ vk: u32, edge }` with a down and an up; this
//!   platform has neither, and its `time_ms` is documented as
//!   `KBDLLHOOKSTRUCT.time` (milliseconds since boot) while
//!   `CGEventTimestamp` is nanoseconds of mach absolute time — a unit
//!   mismatch that would not fail to compile.
//! - **Nothing in the event says whether Caps went down or came up.** Both
//!   transitions arrive with identical flags (`0x20000000`,
//!   `alphaShift` clear), because suppression stops the lock from moving and
//!   the flag reports the lock. `CGEventSourceKeyState` does not help
//!   either: for an ordinary key it tracks the press, for a LOCK key it
//!   reports the lock, which is frozen for the same reason.
//!
//! So the edge is **parity**: transitions alternate, the first is a press.
//! That is what every remapper at this layer does, and it has exactly one
//! failure mode — a missed event inverts the phase — which is why
//! `resync()` exists and is called on every path that can drop one.
//!
//! ## What must not be simplified away
//!
//! - **The chord is injected as one burst.** Holding the modifiers across
//!   real time would make a bare Caps tap press and release Command alone.
//! - **Only keys that are BOUND are injected for.** Otherwise
//!   `Caps+<anything>` becomes a genuine `ctrl+cmd+opt` chord that some
//!   other app may act on.
//! - **The callback never calls `backend.beckon()`.** A tap whose callback
//!   overruns is disabled by the system with no error anywhere; the real
//!   work happens later, on the ordinary hotkey path. The callback is a set
//!   lookup and one burst of `CGEventPost`.
//! - **On any doubt, the event is passed through.** A lock this callback
//!   cannot take, a state it cannot read, an unbound key: every one of those
//!   returns the event. Swallowing wrongly eats a keystroke the user meant.

use beckon_core::caps::{Edge, KeyEvent};
use beckon_core::capture::{
    mac_modifier_vk, step_on, CaptureState, HookOwners, HookReason, Mods, Outcome, Platform,
};
use beckon_core::shortcuts::{CapsTap as TapAction, Chord};
use std::collections::HashSet;
use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

// `CFRelease` is declared once per crate, in `ffi.rs`. Redeclaring it here
// with `*mut c_void` instead of that module's `CFTypeRef` is a clippy error
// (`different signature`) and, worse, would be two sources for one ABI.
use crate::ffi::CFRelease;

// ---------------------------------------------------------------------------
// FFI
// ---------------------------------------------------------------------------

type TapCallback = extern "C" fn(*mut c_void, u32, *mut c_void, *mut c_void) -> *mut c_void;

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGEventTapCreate(
        tap: u32,
        place: u32,
        options: u32,
        events_of_interest: u64,
        callback: TapCallback,
        user_info: *mut c_void,
    ) -> *mut c_void;
    fn CGEventTapEnable(tap: *mut c_void, enable: bool);
    fn CGEventGetIntegerValueField(event: *mut c_void, field: u32) -> i64;
    fn CGEventGetFlags(event: *mut c_void) -> u64;
    fn CGEventCreateKeyboardEvent(
        source: *const c_void,
        virtual_key: u16,
        key_down: bool,
    ) -> *mut c_void;
    fn CGEventSetFlags(event: *mut c_void, flags: u64);
    fn CGEventPost(tap: u32, event: *mut c_void);
}
#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFMachPortCreateRunLoopSource(
        allocator: *const c_void,
        port: *mut c_void,
        order: isize,
    ) -> *mut c_void;
    fn CFRunLoopGetCurrent() -> *mut c_void;
    fn CFRunLoopAddSource(rl: *mut c_void, source: *mut c_void, mode: *const c_void);
    static kCFRunLoopCommonModes: *const c_void;
}
#[link(name = "IOKit", kind = "framework")]
unsafe extern "C" {
    fn IOHIDCheckAccess(request: u32) -> u32;
}

/// `kCGSessionEventTap`. Not `kCGHIDEventTap`: both deliver here, and the
/// session tap is the layer an ordinary application is meant to use.
const SESSION_TAP: u32 = 1;
const HEAD_INSERT: u32 = 0;
/// `kCGEventTapOptionDefault` — may modify and suppress. `ListenOnly` cannot
/// swallow, which is the whole feature.
const DEFAULT_TAP: u32 = 0;

const KEY_DOWN: u32 = 10;
const KEY_UP: u32 = 11;
const FLAGS_CHANGED: u32 = 12;
const TAP_DISABLED_BY_TIMEOUT: u32 = 0xFFFF_FFFE;
const TAP_DISABLED_BY_USER_INPUT: u32 = 0xFFFF_FFFF;

const FIELD_KEYCODE: u32 = 9;

const K_CAPSLOCK: u16 = 0x39;
const K_ESCAPE: u16 = 0x35;
const K_SHIFT: u16 = 0x38;
const K_CONTROL: u16 = 0x3B;
const K_OPTION: u16 = 0x3A;
const K_COMMAND: u16 = 0x37;

const FLAG_SHIFT: u64 = 0x0002_0000;
const FLAG_CONTROL: u64 = 0x0004_0000;
const FLAG_ALTERNATE: u64 = 0x0008_0000;
const FLAG_COMMAND: u64 = 0x0010_0000;

/// `kIOHIDRequestTypeListenEvent`.
const REQUEST_LISTEN: u32 = 1;

/// What a recording session reports back, once per event it consumed.
///
/// **Called on the run loop's own thread, from inside the tap callback.**
/// Windows posts a message instead, because a `WH_KEYBOARD_LL` callback that
/// overruns `LowLevelHooksTimeout` is unhooked silently; a `CGEventTap` is
/// disabled the same way (`kCGEventTapDisabledByTimeout`, which `on_event`
/// already re-enables). So the sink must stay cheap -- set a caption, redraw
/// a hint -- and must not open a modal loop or call back into `caps_tap`.
pub type CaptureSink = Box<dyn FnMut(Outcome) + Send>;

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

#[derive(Clone)]
struct Config {
    /// Carbon keycodes reachable through Caps — `caps::bound_keys_mac`.
    ///
    /// `None` and `Some(empty)` mean the same thing to the callback and the
    /// distinction is not worth a branch: both make it pass everything
    /// through. `Option` is here only because `HashSet::new` is not a `const
    /// fn` and this lives in a `static`.
    wanted: Option<HashSet<u16>>,
    hold: Chord,
    tap: TapAction,
}

impl Config {
    const fn empty() -> Config {
        Config {
            wanted: None,
            hold: Chord {
                ctrl: false,
                super_: false,
                alt: false,
            },
            tap: TapAction::CapsLock,
        }
    }

    /// Is any key reachable through Caps right now?
    ///
    /// **The off switch.** Pausing and unticking both arrive here, and a
    /// callback that answers `false` swallows nothing whatever else is true
    /// — which is what lets the tap stay installed across a pause without
    /// eating a keystroke.
    fn reaches_nothing(&self) -> bool {
        !matches!(&self.wanted, Some(w) if !w.is_empty())
    }

    fn has(&self, code: u16) -> bool {
        matches!(&self.wanted, Some(w) if w.contains(&code))
    }
}

static CONFIG: Mutex<Config> = Mutex::new(Config::empty());

/// Caps is currently held, by parity of the transitions seen.
static CAPS_DOWN: AtomicBool = AtomicBool::new(false);
/// A bound key fired during this hold, so the release must NOT emit the tap
/// action. Exactly the Windows rule: a tap is a press with nothing in it.
static USED: AtomicBool = AtomicBool::new(false);
/// True while this module is posting its own events, so the callback lets
/// them through instead of reading them as user input. Without it the
/// injected chord's own modifier events re-enter and the alias eats itself.
static INJECTING: AtomicBool = AtomicBool::new(false);

/// The live tap, kept so it can be disabled again. Never dereferenced off
/// the main thread.
static TAP_PORT: Mutex<usize> = Mutex::new(0);

/// Who wants the tap held, so neither reason can take it from the other.
///
/// Caps is resident for the session; a recording lasts seconds. Without the
/// refcount, ending a capture would tear down the tap under a machine with
/// `keyboard.caps = true`, and turning Caps off mid-recording would do the
/// same in the other direction.
///
/// **One tap, two reasons -- never two taps.** A second `CGEventTapCreate`
/// chains, and the one inserted later would record the chord the Caps arm
/// INJECTS rather than the key the user pressed.
static OWNERS: Mutex<HookOwners> = Mutex::new(HookOwners::new());

/// `Some` exactly while a shortcut field is recording.
static CAPTURE: Mutex<Option<CaptureState>> = Mutex::new(None);
static CAPTURE_SINK: Mutex<Option<CaptureSink>> = Mutex::new(None);

// ---------------------------------------------------------------------------
// Public surface — mirrors `beckon_windows::caps_hook`
// ---------------------------------------------------------------------------

/// Load the key set the alias should act on.
pub fn set_bindings(wanted: HashSet<u16>, hold: Chord, tap: TapAction) {
    if let Ok(mut c) = CONFIG.lock() {
        *c = Config {
            wanted: Some(wanted),
            hold,
            tap,
        };
    }
    resync();
}

/// Forget every binding.
///
/// **Called before the tap is dropped, and unconditionally**, for the reason
/// the Win32 twin records: dropping the reason is not enough to stop Caps
/// aliasing, because the tap may be held for some other purpose later. An
/// empty `wanted` makes the callback pass everything through whatever else
/// is true, so a paused or switched-off feature eats nothing.
///
/// It resets `tap` to `CapsLock` as well, so a configured `caps_tap =
/// "escape"` stops remapping the key the moment the feature it belongs to is
/// off.
pub fn clear_bindings() {
    if let Ok(mut c) = CONFIG.lock() {
        *c = Config::empty();
    }
    resync();
}

/// Forget which half of a press we are in.
///
/// The edge is parity, so a dropped transition inverts it and Caps would
/// then look held when it is not. Every path that can drop one calls this:
/// the tap being disabled by timeout or by the user, and any change of
/// configuration, which is also the moment nobody is holding a key.
pub fn resync() {
    CAPS_DOWN.store(false, Ordering::SeqCst);
    USED.store(false, Ordering::SeqCst);
}

pub fn is_installed() -> bool {
    TAP_PORT.lock().map(|p| *p != 0).unwrap_or(false)
}

/// Is Input Monitoring granted?
///
/// **Asked separately because `CGEventTapCreate` does not report it.**
/// Measured 2026-08-16: without the grant the create call still returns a
/// non-NULL port, the tap enables, and then no event is ever delivered —
/// silently. Checking first is what turns that into a message.
pub fn input_monitoring_granted() -> bool {
    unsafe { IOHIDCheckAccess(REQUEST_LISTEN) == 0 }
}

/// Install the tap on the CURRENT thread's run loop.
///
/// Idempotent. Must be called on the thread that runs the loop — `serve`'s
/// main thread — because the source is added to `CFRunLoopGetCurrent()`.
pub fn install() -> Result<(), String> {
    if is_installed() {
        return Ok(());
    }
    if !input_monitoring_granted() {
        return Err(
            "Input Monitoring is not granted. System Settings > Privacy & Security > \
             Input Monitoring. It is a different permission from Accessibility, and \
             without it an event tap is created successfully and then receives nothing."
                .into(),
        );
    }

    let events = (1u64 << KEY_DOWN) | (1u64 << KEY_UP) | (1u64 << FLAGS_CHANGED);
    let port = unsafe {
        CGEventTapCreate(
            SESSION_TAP,
            HEAD_INSERT,
            DEFAULT_TAP,
            events,
            on_event,
            std::ptr::null_mut(),
        )
    };
    if port.is_null() {
        return Err("CGEventTapCreate refused to make an event tap".into());
    }

    let source = unsafe { CFMachPortCreateRunLoopSource(std::ptr::null(), port, 0) };
    if source.is_null() {
        unsafe { CFRelease(port as *const c_void) };
        return Err("CFMachPortCreateRunLoopSource returned nothing".into());
    }
    unsafe {
        CFRunLoopAddSource(CFRunLoopGetCurrent(), source, kCFRunLoopCommonModes);
        // The source is retained by the run loop; this balances the create.
        CFRelease(source as *const c_void);
        CGEventTapEnable(port, true);
    }
    if let Ok(mut p) = TAP_PORT.lock() {
        *p = port as usize;
    }
    resync();
    Ok(())
}

/// Take the tap down.
///
/// **`clear_bindings` first, always.** Between disabling and the run loop
/// noticing there is a window in which the callback can still run, and a
/// callback with no bindings swallows nothing.
pub fn uninstall() {
    clear_bindings();
    let port = TAP_PORT.lock().map(|mut p| std::mem::replace(&mut *p, 0));
    if let Ok(port) = port {
        if port != 0 {
            unsafe {
                CGEventTapEnable(port as *mut c_void, false);
                CFRelease(port as *const c_void);
            }
        }
    }
}

/// Hold the tap for one reason, installing it if nobody held it yet.
pub fn install_for(reason: HookReason) -> Result<(), String> {
    let need = match OWNERS.lock() {
        Ok(mut o) => o.add(reason),
        Err(_) => return Err("the tap's owner set is poisoned".into()),
    };
    if need {
        if let Err(e) = install() {
            // Give the reason back. Leaving it recorded would make the next
            // `uninstall_for` believe a tap exists and the one after that
            // believe nothing needs installing.
            if let Ok(mut o) = OWNERS.lock() {
                o.remove(reason);
            }
            return Err(e);
        }
    }
    Ok(())
}

/// Drop one reason. The tap comes down only when nobody is left.
///
/// **This is why ending a recording does not disarm Caps**, and why
/// `sync_caps_hook` turning Caps off does not stop a recording in progress.
pub fn uninstall_for(reason: HookReason) {
    let last = match OWNERS.lock() {
        Ok(mut o) => o.remove(reason),
        Err(_) => return,
    };
    if last {
        uninstall();
    }
}

// ---------------------------------------------------------------------------
// Chord capture
// ---------------------------------------------------------------------------

/// Arm a recording. Idempotent in the sense that a second call replaces the
/// sink and restarts the state -- it never stacks.
///
/// **This can arm the tap on a machine where the user left
/// `keyboard.caps = false`**, which is exactly the widening the Windows
/// entry records for `WH_KEYBOARD_LL`: the exception is no longer one
/// feature. `end_capture` is what bounds it, and it is called from every
/// exit path the window has.
pub fn begin_capture(sink: CaptureSink) -> Result<(), String> {
    install_for(HookReason::Capture)?;
    if let Ok(mut s) = CAPTURE_SINK.lock() {
        *s = Some(sink);
    }
    if let Ok(mut c) = CAPTURE.lock() {
        *c = Some(CaptureState::armed());
    }
    Ok(())
}

/// Stop recording. **Idempotent**, because it is called from seven places
/// and most of them do not know whether one of the others ran first.
pub fn end_capture() {
    if let Ok(mut c) = CAPTURE.lock() {
        *c = None;
    }
    if let Ok(mut s) = CAPTURE_SINK.lock() {
        *s = None;
    }
    uninstall_for(HookReason::Capture);
}

pub fn is_capturing() -> bool {
    CAPTURE.lock().map(|c| c.is_some()).unwrap_or(false)
}

/// The chord just recorded, canonically spelled, or `None`.
///
/// **Read it before `end_capture`**, which drops the state it lives in. The
/// sink is called while the recording is still armed precisely so this is
/// reachable from inside it.
pub fn captured_combo() -> Option<String> {
    let g = CAPTURE.lock().ok()?;
    g.as_ref()?.captured().map(|c| c.canonical())
}

/// The modifiers physically held, as this event reports them.
///
/// `step_on` takes these as a parameter rather than a snapshot precisely so a
/// modifier RELEASED mid-recording stops counting the moment it is let go.
fn live_mods(flags: u64) -> Mods {
    Mods {
        ctrl: flags & FLAG_CONTROL != 0,
        super_: flags & FLAG_COMMAND != 0,
        alt: flags & FLAG_ALTERNATE != 0,
        shift: flags & FLAG_SHIFT != 0,
    }
}

/// One tap event as `step_on` wants it, or `None` when it is not a key at all.
///
/// Two projections in one function because they answer one question:
///
/// - **an ordinary key** goes through `key_table()`, which carries `mac` and
///   `win` side by side -- measured on airm3, every keycode the probe saw was
///   in it, which is why `step` is reused rather than forked;
/// - **a modifier** goes through `mac_modifier_vk`, and its EDGE comes from
///   its own flag bit. That works here and not for Caps: suppression freezes
///   the lock the Caps flag reports, while these are not locks.
fn project(etype: u32, code: u16, flags: u64) -> Option<KeyEvent> {
    let edge = match etype {
        KEY_DOWN => Edge::Down,
        KEY_UP => Edge::Up,
        FLAGS_CHANGED => {
            let bit = match code {
                K_SHIFT | 0x3C => FLAG_SHIFT,
                K_CONTROL | 0x3E => FLAG_CONTROL,
                K_OPTION | 0x3D => FLAG_ALTERNATE,
                K_COMMAND | 0x36 => FLAG_COMMAND,
                // Caps, or a modifier with no bit. It reaches `step_on` as an
                // ordinary key-down so `refusal_for` can refuse it.
                _ => return ordinary(code, Edge::Down),
            };
            if flags & bit != 0 {
                Edge::Down
            } else {
                Edge::Up
            }
        }
        _ => return None,
    };
    if etype == FLAGS_CHANGED {
        return mac_modifier_vk(code).map(|vk| KeyEvent {
            vk,
            edge,
            injected_by_us: false,
            time_ms: 0,
        });
    }
    ordinary(code, edge)
}

/// Run one event through the recorder, or `None` when nothing is recording.
///
/// **Every lock is released before the sink is called.** The sink reaches
/// into the settings window, and the window's own handlers call
/// `end_capture` -- which takes `CAPTURE` and `CAPTURE_SINK`. Holding either
/// across the call is a deadlock on the run loop's own thread, and the tray's
/// `dispatch` takes the same precaution for the same reason.
fn capture_step(etype: u32, event: *mut c_void) -> Option<Outcome> {
    // `try_lock`: a contended lock means the window is mid-`end_capture`, and
    // the safe answer there is to let the key through rather than swallow it
    // into a recording that is being torn down.
    let mut guard = CAPTURE.try_lock().ok()?;
    let st = guard.as_mut()?;

    let code = unsafe { CGEventGetIntegerValueField(event, FIELD_KEYCODE) } as u16;
    let flags = unsafe { CGEventGetFlags(event) };
    let Some(ev) = project(etype, code, flags) else {
        drop(guard);
        return Some(Outcome::PassThrough);
    };

    let outcome = step_on(ev, st, live_mods(flags), Platform::Mac);
    drop(guard);

    // Taken out of the mutex, called, then put back -- so the sink may
    // re-enter this module without deadlocking. If it called `end_capture`,
    // the slot is `None` when we return and the sink is simply dropped.
    let taken = CAPTURE_SINK.lock().ok().and_then(|mut s| s.take());
    if let Some(mut sink) = taken {
        sink(outcome);
        if let Ok(mut s) = CAPTURE_SINK.lock() {
            if s.is_none() {
                *s = Some(sink);
            }
        }
    }
    Some(outcome)
}

fn ordinary(code: u16, edge: Edge) -> Option<KeyEvent> {
    // `step_on` looks the vk up again through `lookup_win_vk`; handing it a
    // vk it cannot name is how `Refusal::UnknownKey` is reached, which is a
    // real answer rather than a hole. So an unknown keycode still has to
    // reach it -- passing `None` here would swallow the event with no verdict
    // and no hint.
    let vk = beckon_core::shortcuts::key_table()
        .iter()
        .find(|k| k.mac == code)
        .map(|k| k.win)
        .unwrap_or(u32::from(code) | 0x1_0000);
    Some(KeyEvent {
        vk,
        edge,
        injected_by_us: false,
        time_ms: 0,
    })
}

// ---------------------------------------------------------------------------
// The callback
// ---------------------------------------------------------------------------

extern "C" fn on_event(
    _proxy: *mut c_void,
    etype: u32,
    event: *mut c_void,
    _ud: *mut c_void,
) -> *mut c_void {
    // The system disables a tap whose callback overran, and it tells the
    // callback once. Re-enabling is the caller's job and the phase is gone,
    // so both halves happen here.
    if etype == TAP_DISABLED_BY_TIMEOUT || etype == TAP_DISABLED_BY_USER_INPUT {
        resync();
        if let Ok(p) = TAP_PORT.lock() {
            if *p != 0 {
                unsafe { CGEventTapEnable(*p as *mut c_void, true) };
            }
        }
        return event;
    }

    // Our own burst. Reading it as input would make the alias eat itself.
    if INJECTING.load(Ordering::SeqCst) {
        return event;
    }

    // **The capture arm is consulted FIRST, and the order is load-bearing.**
    //
    // Two reasons, both taken from the Windows entry. A recording must work
    // on a machine where the user left `keyboard.caps = false` -- and the
    // Caps arm below returns early on `reaches_nothing()`, so anything after
    // it would never run there. And if Caps ran first, a `Caps+T` during a
    // recording would inject `ctrl+cmd+opt+T` and the recorder would write
    // down the ALIAS instead of the key that was pressed.
    if let Some(out) = capture_step(etype, event) {
        // `PassThrough` is the one outcome that must not be swallowed: the
        // down was never taken, so taking the up would strand a modifier the
        // system believes is still held. See `Outcome::PassThrough`.
        return if out == Outcome::PassThrough {
            event
        } else {
            std::ptr::null_mut()
        };
    }

    // **`try_lock`, and pass through when it fails.** The callback runs on
    // the same thread as `set_bindings`, so a contended lock means something
    // re-entered — and the safe answer to "I cannot decide" is never to
    // swallow.
    let Ok(cfg) = CONFIG.try_lock() else {
        return event;
    };
    if cfg.reaches_nothing() {
        // Nothing is reachable through Caps, so nothing is taken — whatever
        // else is true. This is what makes pausing and switching off safe
        // without touching the tap.
        return event;
    }

    let code = unsafe { CGEventGetIntegerValueField(event, FIELD_KEYCODE) } as u16;

    if etype == FLAGS_CHANGED && code == K_CAPSLOCK {
        // Parity: transitions alternate and the first is a press.
        let now_down = !CAPS_DOWN.load(Ordering::SeqCst);
        CAPS_DOWN.store(now_down, Ordering::SeqCst);
        if now_down {
            USED.store(false, Ordering::SeqCst);
        } else if !USED.swap(false, Ordering::SeqCst) {
            // A press with nothing in it: the tap gesture.
            match cfg.tap {
                TapAction::CapsLock => {
                    // Give the key back. The lock did not move while it was
                    // swallowed, so this is the only way it ever toggles —
                    // which is what makes `capslock` the honest default: a
                    // person who ticks the box does not silently lose a key.
                    drop(cfg);
                    inject_plain(K_CAPSLOCK);
                }
                TapAction::Escape => {
                    drop(cfg);
                    inject_plain(K_ESCAPE);
                }
                TapAction::None => {}
            }
        }
        // Swallowed either way: the lock must not move under a hold.
        return std::ptr::null_mut();
    }

    if !CAPS_DOWN.load(Ordering::SeqCst) {
        return event;
    }
    // Caps is held.
    if !cfg.has(code) {
        // **Unbound keys pass through untouched.** Injecting the chord for
        // them would turn `Caps+<anything>` into a real `ctrl+cmd+opt` chord
        // that some other application may act on.
        return event;
    }
    if etype == KEY_DOWN {
        USED.store(true, Ordering::SeqCst);
        let hold = cfg.hold;
        drop(cfg);
        inject_chord(hold, code);
    }
    // Both the down and the up are swallowed. Letting the up through leaves
    // an unpaired release in whatever has focus.
    std::ptr::null_mut()
}

// ---------------------------------------------------------------------------
// Injection
// ---------------------------------------------------------------------------

fn post(code: u16, down: bool, flags: u64) {
    unsafe {
        let ev = CGEventCreateKeyboardEvent(std::ptr::null(), code, down);
        if ev.is_null() {
            return;
        }
        CGEventSetFlags(ev, flags);
        CGEventPost(SESSION_TAP, ev);
        CFRelease(ev as *const c_void);
    }
}

/// One key, no modifiers — the tap gesture's replacement.
fn inject_plain(code: u16) {
    INJECTING.store(true, Ordering::SeqCst);
    post(code, true, 0);
    post(code, false, 0);
    INJECTING.store(false, Ordering::SeqCst);
}

/// The configured chord and one key, as ONE burst.
///
/// **The modifiers are real key events, not just flags.** Measured
/// 2026-08-16: a key carrying `CGEventSetFlags(ctrl|opt|cmd)` and nothing
/// else posts successfully and does **not** fire a `RegisterEventHotKey`
/// binding — under either run loop, including the one that demonstrably
/// delivers hotkeys. The flags field describes an event; it does not hold a
/// key down, and the system tracks modifier state from the `kVK_Control`
/// &co. events themselves.
///
/// **One burst, no real time held.** Pressing the modifiers and releasing
/// them across a gap would make a bare Caps tap press and release Command
/// alone, which on this platform opens nothing but on Windows opens the
/// Start menu — the rule is the same and the reason it exists is worth
/// keeping either way.
fn inject_chord(hold: Chord, code: u16) {
    let mut mods: Vec<(u16, u64)> = Vec::with_capacity(3);
    if hold.ctrl {
        mods.push((K_CONTROL, FLAG_CONTROL));
    }
    if hold.alt {
        mods.push((K_OPTION, FLAG_ALTERNATE));
    }
    if hold.super_ {
        mods.push((K_COMMAND, FLAG_COMMAND));
    }
    // There is no Shift arm and there must never be one: `Chord` has exactly
    // three fields, because the alias has to RELEASE whatever it presses and
    // releasing Shift under the user's fingers makes everything they type
    // next arrive lowercase.
    let _ = FLAG_SHIFT;
    let _ = K_SHIFT;

    INJECTING.store(true, Ordering::SeqCst);
    let mut acc = 0u64;
    for (k, f) in &mods {
        acc |= f;
        post(*k, true, acc);
    }
    post(code, true, acc);
    post(code, false, acc);
    for (k, f) in mods.iter().rev() {
        acc &= !f;
        post(*k, false, acc);
    }
    INJECTING.store(false, Ordering::SeqCst);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An ordinary key projects to the vk `step_on` looks up, both edges.
    #[test]
    fn an_ordinary_key_carries_its_win_vk_and_its_edge() {
        let down = project(KEY_DOWN, 0x00, 0).expect("`a` is in key_table");
        assert_eq!((down.vk, down.edge), (0x41, Edge::Down));
        let up = project(KEY_UP, 0x00, 0).expect("`a` is in key_table");
        assert_eq!((up.vk, up.edge), (0x41, Edge::Up));
    }

    /// **The edge comes from the modifier's OWN bit in the flags this event
    /// carries**, and getting it backwards is the quietest possible defect:
    /// nothing throws, the chord simply commits with a modifier the user let
    /// go of, or without one they are holding.
    ///
    /// Measured on airm3 2026-08-16 -- the flags carry state AFTER the
    /// transition, so the bit being set means the key just went down.
    #[test]
    fn a_modifier_takes_its_edge_from_its_own_flag_bit() {
        let down = project(FLAGS_CHANGED, K_CONTROL, FLAG_CONTROL).expect("ctrl projects");
        assert_eq!(down.edge, Edge::Down, "bit set means it just went down");

        let up = project(FLAGS_CHANGED, K_CONTROL, 0).expect("ctrl projects");
        assert_eq!(up.edge, Edge::Up, "bit clear means it just came up");

        // A DIFFERENT modifier's bit must not decide this one's edge. With
        // Command held and Control released, the flags are non-zero and a
        // naive `flags != 0` reads the release as a press.
        let up_while_cmd_held =
            project(FLAGS_CHANGED, K_CONTROL, FLAG_COMMAND).expect("ctrl projects");
        assert_eq!(
            up_while_cmd_held.edge,
            Edge::Up,
            "ctrl came up; Command being held says nothing about ctrl"
        );
    }

    /// Caps must reach `step_on` as an ORDINARY key, not as a modifier.
    ///
    /// `refusal_for` refuses it there -- the lock toggles before any tap
    /// runs, so swallowing cannot undo the light. Projected as a modifier it
    /// would enter the held set instead, and a chord could commit with a lock
    /// key silently counted as one of its modifiers.
    #[test]
    fn caps_projects_as_a_key_so_it_can_be_refused() {
        let ev = project(FLAGS_CHANGED, K_CAPSLOCK, 0).expect("caps still projects");
        assert_eq!(ev.edge, Edge::Down);
        assert_eq!(
            beckon_core::capture::mac_modifier_vk(K_CAPSLOCK),
            None,
            "and it is not in the modifier table at all"
        );
    }

    /// A keycode `key_table()` cannot name still has to REACH `step_on`.
    ///
    /// That is how `Refusal::UnknownKey` is produced, which is a real answer
    /// with a hint that sends the reader to the Key list. Returning `None`
    /// here would swallow the key with no verdict and no hint -- a recording
    /// that silently ignores a keypress.
    #[test]
    fn an_unnameable_keycode_still_reaches_the_state_machine() {
        // 0x6F is F12 on some layouts and absent from the 81-key table's mac
        // column on this one; whatever it is, the projection must not drop it.
        let ev = project(KEY_DOWN, 0x7F, 0);
        assert!(
            ev.is_some(),
            "an unknown keycode must still produce an event"
        );
    }

    #[test]
    fn live_mods_reads_all_four() {
        let m = live_mods(FLAG_CONTROL | FLAG_COMMAND);
        assert!(m.ctrl && m.super_ && !m.alt && !m.shift);
        let m = live_mods(FLAG_ALTERNATE | FLAG_SHIFT);
        assert!(!m.ctrl && !m.super_ && m.alt && m.shift);
    }

    /// `resync` is the whole answer to a dropped transition, so it has to
    /// clear BOTH halves — a stale `USED` would silently swallow the next
    /// tap gesture.
    #[test]
    fn resync_clears_both_halves() {
        CAPS_DOWN.store(true, Ordering::SeqCst);
        USED.store(true, Ordering::SeqCst);
        resync();
        assert!(!CAPS_DOWN.load(Ordering::SeqCst));
        assert!(!USED.load(Ordering::SeqCst));
    }

    /// An empty binding set is the off switch, and `clear_bindings` must
    /// reach it from any state — including one where a `caps_tap` was
    /// remapping the key.
    #[test]
    fn clearing_bindings_also_gives_the_key_back() {
        set_bindings(
            HashSet::from([0x11]),
            Chord {
                ctrl: true,
                super_: true,
                alt: true,
            },
            TapAction::Escape,
        );
        clear_bindings();
        let c = CONFIG.lock().expect("not poisoned");
        assert!(c.reaches_nothing());
        assert_eq!(c.tap, TapAction::CapsLock);
    }
}
