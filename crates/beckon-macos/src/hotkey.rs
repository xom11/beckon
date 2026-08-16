//! Global hotkeys via Carbon `RegisterEventHotKey` + a CFRunLoop timer.
//! Hand-rolled FFI (house style — the surface is 9 functions). Everything
//! runs on the main thread's run loop: the hotkey callback and every tick
//! callback are never concurrent.

use std::ffi::c_void;

pub const MOD_CMD: u32 = 0x100; // cmdKey
pub const MOD_SHIFT: u32 = 0x200; // shiftKey
pub const MOD_OPT: u32 = 0x800; // optionKey
pub const MOD_CTRL: u32 = 0x1000; // controlKey

const CLASS_KEYBOARD: u32 = u32::from_be_bytes(*b"keyb"); // kEventClassKeyboard
const HOTKEY_PRESSED: u32 = 5; // kEventHotKeyPressed
const SIG: u32 = u32::from_be_bytes(*b"BKON");
const PARAM_DIRECT_OBJECT: u32 = u32::from_be_bytes(*b"----"); // kEventParamDirectObject
const TYPE_EVENT_HOTKEY_ID: u32 = u32::from_be_bytes(*b"hkid"); // typeEventHotKeyID

#[repr(C)]
struct EventTypeSpec {
    event_class: u32,
    event_kind: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct EventHotKeyID {
    signature: u32,
    id: u32,
}

type HandlerFn = extern "C" fn(*mut c_void, *mut c_void, *mut c_void) -> i32;
type TimerFn = extern "C" fn(*mut c_void, *mut c_void);

#[repr(C)]
struct CFRunLoopTimerContext {
    version: isize,
    info: *mut c_void,
    retain: Option<extern "C" fn(*const c_void) -> *const c_void>,
    release: Option<extern "C" fn(*const c_void)>,
    copy_description: Option<extern "C" fn(*const c_void) -> *const c_void>,
}

#[link(name = "Carbon", kind = "framework")]
extern "C" {
    fn GetApplicationEventTarget() -> *mut c_void;
    fn InstallEventHandler(
        target: *mut c_void,
        handler: HandlerFn,
        num_types: usize, // ItemCount = unsigned long (64-bit)
        types: *const EventTypeSpec,
        user_data: *mut c_void,
        out_ref: *mut *mut c_void,
    ) -> i32;
    fn RegisterEventHotKey(
        key_code: u32,
        modifiers: u32,
        id: EventHotKeyID,
        target: *mut c_void,
        options: u32,
        out_ref: *mut *mut c_void,
    ) -> i32;
    fn UnregisterEventHotKey(hotkey: *mut c_void) -> i32;
    fn GetEventParameter(
        event: *mut c_void,
        name: u32,
        ty: u32,
        actual_type: *mut u32,
        size: usize,
        actual_size: *mut usize,
        out: *mut c_void,
    ) -> i32;
    fn RunApplicationEventLoop();
}

#[repr(C)]
struct ProcessSerialNumber {
    high: u32,
    low: u32,
}

const CURRENT_PROCESS: u32 = 2; // kCurrentProcess
const TRANSFORM_TO_UIELEMENT: u32 = 4; // kProcessTransformToUIElementApplication

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn TransformProcessType(psn: *const ProcessSerialNumber, transform_state: u32) -> i32;
}

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFAbsoluteTimeGetCurrent() -> f64;
    fn CFRunLoopTimerCreate(
        allocator: *const c_void,
        fire_date: f64,
        interval: f64,
        flags: u64,
        order: i64,
        callout: TimerFn,
        context: *mut CFRunLoopTimerContext,
    ) -> *mut c_void;
    fn CFRunLoopGetCurrent() -> *mut c_void;
    fn CFRunLoopAddTimer(rl: *mut c_void, timer: *mut c_void, mode: *const c_void);
    static kCFRunLoopCommonModes: *const c_void;
}

extern "C" fn hotkey_trampoline(
    _handler: *mut c_void,
    event: *mut c_void,
    user: *mut c_void,
) -> i32 {
    let mut hk = EventHotKeyID {
        signature: 0,
        id: 0,
    };
    let err = unsafe {
        GetEventParameter(
            event,
            PARAM_DIRECT_OBJECT,
            TYPE_EVENT_HOTKEY_ID,
            std::ptr::null_mut(),
            std::mem::size_of::<EventHotKeyID>(),
            std::ptr::null_mut(),
            &mut hk as *mut _ as *mut c_void,
        )
    };
    if err == 0 && hk.signature == SIG {
        let cb = unsafe { &mut *(user as *mut Box<dyn FnMut(u32)>) };
        cb(hk.id);
    }
    0 // noErr
}

extern "C" fn timer_trampoline(_timer: *mut c_void, info: *mut c_void) {
    let cb = unsafe { &mut *(info as *mut Box<dyn FnMut()>) };
    cb();
}

pub struct HotkeyManager {
    hotkeys: Vec<*mut c_void>,
    _callback: *mut Box<dyn FnMut(u32)>, // leaked for daemon lifetime
}

impl HotkeyManager {
    pub fn install(cb: Box<dyn FnMut(u32)>) -> Result<Self, String> {
        // A launchd-spawned process has no window-server "application"
        // identity: RegisterEventHotKey returns noErr but hotkey events are
        // never delivered (measured 2026-08-08: terminal-launched receives
        // presses, the identical binary under `launchctl bootstrap gui/` is
        // silent, with Hammerspoon as a positive control on the same chord).
        // Becoming a UIElement app creates that identity — no Dock icon, no
        // menu bar — and is a no-op harmless when already terminal-launched.
        // Failure is non-fatal on purpose: in contexts that reject the
        // transform we are no worse off than before, so warn and continue.
        let psn = ProcessSerialNumber {
            high: 0,
            low: CURRENT_PROCESS,
        };
        let err = unsafe { TransformProcessType(&psn, TRANSFORM_TO_UIELEMENT) };
        if err != 0 {
            eprintln!("hotkey: TransformProcessType failed: OSStatus {err} (hotkeys may not fire under launchd)");
        }

        let user = Box::into_raw(Box::new(cb));
        let spec = EventTypeSpec {
            event_class: CLASS_KEYBOARD,
            event_kind: HOTKEY_PRESSED,
        };
        let mut handler = std::ptr::null_mut();
        let err = unsafe {
            InstallEventHandler(
                GetApplicationEventTarget(),
                hotkey_trampoline,
                1,
                &spec,
                user as *mut c_void,
                &mut handler,
            )
        };
        if err != 0 {
            return Err(format!("InstallEventHandler failed: OSStatus {err}"));
        }
        Ok(Self {
            hotkeys: Vec::new(),
            _callback: user,
        })
    }

    pub fn register(
        &mut self,
        id: u32,
        ctrl: bool,
        super_: bool,
        alt: bool,
        shift: bool,
        key: &beckon_core::shortcuts::KeyDef,
    ) -> Result<(), String> {
        let mut mods = 0u32;
        if ctrl {
            mods |= MOD_CTRL;
        }
        if super_ {
            mods |= MOD_CMD;
        }
        if alt {
            mods |= MOD_OPT;
        }
        if shift {
            mods |= MOD_SHIFT;
        }
        let mut out = std::ptr::null_mut();
        let err = unsafe {
            RegisterEventHotKey(
                u32::from(key.mac),
                mods,
                EventHotKeyID { signature: SIG, id },
                GetApplicationEventTarget(),
                0,
                &mut out,
            )
        };
        if err != 0 {
            return Err(format!("RegisterEventHotKey failed: OSStatus {err}"));
        }
        self.hotkeys.push(out);
        Ok(())
    }

    pub fn unregister_all(&mut self) {
        for h in self.hotkeys.drain(..) {
            unsafe {
                UnregisterEventHotKey(h);
            }
        }
    }

    /// Run the main event loop, forever.
    ///
    /// **`[NSApp run]`, not Carbon's `RunApplicationEventLoop` — changed
    /// 2026-08-16 on a measurement, and this is design §5 of the macOS tray
    /// spec finally being answered.** That spec said to settle it before any
    /// view code was written; it was not settled, and `tray.rs` and the
    /// settings window were written anyway.
    ///
    /// What was measured (`examples/loop_probe.rs`, one view hierarchy and
    /// two loops chosen by argv, so the difference in the output IS the
    /// result):
    ///
    /// ```text
    /// nsapp  : isRunning=true   the button's action ran
    /// carbon : isRunning=false  it never ran
    /// ```
    ///
    /// Under `RunApplicationEventLoop`, `NSApplication` is instantiated —
    /// `NSStatusBar` requires it to exist — but never enters its own loop, so
    /// nothing calls `[NSApp sendEvent:]` and nothing drains the queue that
    /// routes a mouse event to a window and thence to a view. Every control
    /// in the four doors would have been decoration.
    ///
    /// **The Carbon hotkeys are unaffected, and that is the load-bearing
    /// half of this change.** `RegisterEventHotKey` installs a handler on
    /// `GetApplicationEventTarget()`, and `[NSApp run]` pumps the same event
    /// queue Carbon's loop did — `nextEventMatchingMask:` calls
    /// `ReceiveNextEvent` underneath, and `[NSApp sendEvent:]` forwards what
    /// Cocoa does not claim to the Carbon target. This is the ordinary
    /// configuration rather than a clever one: it is what every Cocoa global-
    /// hotkey library on this platform does, and the Carbon-loop shape beckon
    /// had is the unusual one.
    ///
    /// **Measured, 2026-08-16** — `examples/carbon_queue_probe.rs`, with the
    /// Carbon loop as the baseline in the same run:
    ///
    /// ```text
    /// carbon : DISPATCHED
    /// nsapp  : DISPATCHED
    /// ```
    ///
    /// It installs a handler on `GetApplicationEventTarget()` — the same
    /// target `RegisterEventHotKey` installs on — posts an event of its own
    /// class to the main event queue, and asks whether the handler ran. Under
    /// both loops it did, so `[NSApp run]` pumps the Carbon application event
    /// queue and the path a hotkey press travels after the window server has
    /// decided the press belongs to this app is intact.
    ///
    /// **And directly, the same day**, once an Accessibility grant made a
    /// synthetic keystroke possible: `examples/hotkey_loop_probe.rs` driven
    /// by `examples/hid_key.rs`, a real chord posted through the window
    /// server, again with the Carbon loop as the baseline —
    ///
    /// ```text
    /// carbon : HOTKEY FIRED
    /// nsapp  : HOTKEY FIRED
    /// ```
    ///
    /// The queue probe therefore stands as the explanation rather than as the
    /// evidence, which is the better shape for it.
    ///
    /// `TransformProcessType` is untouched, so the window-server identity the
    /// hotkeys depend on has not moved either.
    pub fn run_forever() -> ! {
        let mtm = objc2_foundation::MainThreadMarker::new()
            .expect("run_forever must be called on the main thread");
        let app = objc2_app_kit::NSApplication::sharedApplication(mtm);
        app.run();
        // `[NSApp run]` returns only after `stop:` or `terminate:`. beckon
        // reaches neither — Quit from the tray calls `exit` — so this is the
        // clean-shutdown path rather than a case that happens today.
        // `unreachable!()` here would turn an orderly quit into a panic.
        std::process::exit(0);
    }

    /// Carbon's `RunApplicationEventLoop`, which `run_forever` used until
    /// 2026-08-16.
    ///
    /// **Kept solely so the measurement that replaced it can be re-run.**
    /// `examples/loop_probe.rs` needs both loops behind one argv flag, or its
    /// result stops being a comparison and becomes an assertion — and this
    /// repository's rule is that a claim is not re-added without re-running
    /// the probe that settled it. Deleting this function would make the
    /// finding in `run_forever`'s doc unfalsifiable, which is the failure
    /// mode three separate entries in `CLAUDE.md` are about.
    ///
    /// Nothing in beckon proper calls it, and nothing should.
    #[doc(hidden)]
    pub fn run_carbon_event_loop_for_probe() -> ! {
        unsafe { RunApplicationEventLoop() };
        unreachable!("RunApplicationEventLoop returned");
    }
}

/// Schedule a repeating callback on the CURRENT thread's run loop. Call
/// before `run_forever` on the same thread. The timer and callback leak —
/// they live as long as the daemon.
pub fn add_tick(seconds: f64, cb: Box<dyn FnMut()>) {
    let info = Box::into_raw(Box::new(cb)) as *mut c_void;
    let mut ctx = CFRunLoopTimerContext {
        version: 0,
        info,
        retain: None,
        release: None,
        copy_description: None,
    };
    unsafe {
        let timer = CFRunLoopTimerCreate(
            std::ptr::null(),
            CFAbsoluteTimeGetCurrent() + seconds,
            seconds,
            0,
            0,
            timer_trampoline,
            &mut ctx,
        );
        CFRunLoopAddTimer(CFRunLoopGetCurrent(), timer, kCFRunLoopCommonModes);
    }
}
