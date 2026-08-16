//! Does `[NSApp run]` still drain the **Carbon** event queue?
//!
//! This is the second half of the run-loop change, and the half that carries
//! the risk. `hotkey::run_forever` stopped calling
//! `RunApplicationEventLoop` on 2026-08-16 because under it `NSApp` never
//! runs and no Cocoa control ever sees an event (`loop_probe.rs`). But
//! beckon's whole feature is `RegisterEventHotKey`, which is Carbon: it
//! installs a handler on `GetApplicationEventTarget()` and the system posts
//! the press to the application's Carbon event queue. If `[NSApp run]` does
//! not pump that queue, every hotkey stops firing — silently, on a daemon
//! that is running on this machine right now.
//!
//! ```text
//! cargo run -p beckon-macos --example carbon_queue_probe -- nsapp
//! cargo run -p beckon-macos --example carbon_queue_probe -- carbon
//! ```
//!
//! ## Why this and not a keypress
//!
//! A keypress is the direct measurement and it is better. It also needs an
//! event injected into the Aqua session, which needs a permission no process
//! on this machine has (see `loop_probe.rs`'s table) or a person at the
//! keyboard. This probe needs neither, because it posts a Carbon event of
//! its **own** class into the same queue the system posts a hotkey press
//! into, and installs a handler for it on the same target
//! `RegisterEventHotKey` uses.
//!
//! **What that does and does not establish.** It shows that a queued Carbon
//! event reaches an application-target handler under the loop being tested —
//! which is the mechanism a hotkey press travels by, one step after the
//! window server has decided a press belongs to this app. It does not
//! exercise the window server's half, so it cannot catch a failure that
//! lives there. It is indirect evidence, deliberately labelled as such, and
//! it is strictly better than the alternative on offer, which is reasoning.
//!
//! ## Reading the output
//!
//! Run both. `carbon` is the baseline: that loop demonstrably delivers
//! hotkeys, because `serve` ships on it and is in daily use.
//!
//! | carbon | nsapp | reading |
//! |---|---|---|
//! | DISPATCHED | DISPATCHED | `[NSApp run]` pumps the Carbon queue — the change is safe by this evidence |
//! | DISPATCHED | silent | **`[NSApp run]` does not. Revert `run_forever`.** |
//! | silent | * | the probe is wrong, not the loop; nothing below it means anything |

fn main() {
    #[cfg(not(target_os = "macos"))]
    {
        eprintln!("carbon_queue_probe is macOS-only");
        std::process::exit(2);
    }
    #[cfg(target_os = "macos")]
    mac::run();
}

#[cfg(target_os = "macos")]
mod mac {
    use std::ffi::c_void;
    use std::io::Write;
    use std::sync::atomic::{AtomicBool, Ordering};

    fn say(l: &str) {
        println!("{l}");
        let _ = std::io::stdout().flush();
    }

    static DISPATCHED: AtomicBool = AtomicBool::new(false);

    #[repr(C)]
    struct EventTypeSpec {
        event_class: u32,
        event_kind: u32,
    }

    type HandlerFn = extern "C" fn(*mut c_void, *mut c_void, *mut c_void) -> i32;

    #[link(name = "Carbon", kind = "framework")]
    unsafe extern "C" {
        fn GetApplicationEventTarget() -> *mut c_void;
        fn InstallEventHandler(
            target: *mut c_void,
            handler: HandlerFn,
            num_types: usize,
            types: *const EventTypeSpec,
            user_data: *mut c_void,
            out_ref: *mut *mut c_void,
        ) -> i32;
        fn CreateEvent(
            allocator: *const c_void,
            class_id: u32,
            kind: u32,
            when: f64,
            attributes: u32,
            out_event: *mut *mut c_void,
        ) -> i32;
        fn GetMainEventQueue() -> *mut c_void;
        fn PostEventToQueue(queue: *mut c_void, event: *mut c_void, priority: i16) -> i32;
        fn ReleaseEvent(event: *mut c_void);
    }

    /// Our own event class and kind. A four-char code, like every Carbon
    /// class — `'BKPB'` for beckon-probe. It is deliberately not a system
    /// class: a probe that posted `kEventClassKeyboard` would be competing
    /// with the real input path rather than measuring the queue.
    const CLASS: u32 = u32::from_be_bytes(*b"BKPB");
    const KIND: u32 = 1;
    /// `kEventPriorityStandard`.
    const PRIORITY_STANDARD: i16 = 1;

    extern "C" fn on_event(_call: *mut c_void, _event: *mut c_void, _ud: *mut c_void) -> i32 {
        DISPATCHED.store(true, Ordering::SeqCst);
        // `eventNotHandledErr` would ask Carbon to keep looking; 0 says we
        // consumed it, which is what a hotkey handler does.
        0
    }

    pub fn run() {
        let manager = std::process::Command::new("launchctl")
            .arg("managername")
            .output()
            .ok()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .unwrap_or_default();
        say(&format!("bootstrap namespace : {manager}"));
        if manager != "Aqua" {
            say("REFUSING: not an Aqua session, which is where `serve` runs.");
            std::process::exit(3);
        }

        let mode = std::env::args().nth(1).unwrap_or_default();
        if mode != "carbon" && mode != "nsapp" {
            say("usage: carbon_queue_probe <carbon|nsapp>");
            std::process::exit(2);
        }
        say(&format!("MODE: {mode}"));

        // The same target `RegisterEventHotKey` installs on. That is the
        // whole point of the probe: not "does Carbon work" but "does THIS
        // loop deliver to THAT target".
        let spec = EventTypeSpec {
            event_class: CLASS,
            event_kind: KIND,
        };
        let mut handler_ref: *mut c_void = std::ptr::null_mut();
        let err = unsafe {
            InstallEventHandler(
                GetApplicationEventTarget(),
                on_event,
                1,
                &spec,
                std::ptr::null_mut(),
                &mut handler_ref,
            )
        };
        if err != 0 {
            say(&format!("InstallEventHandler failed: OSStatus {err}"));
            std::process::exit(1);
        }
        say("handler installed on the application event target");

        // `NSApp` has to exist for the same reason `tray.rs` needs it, and in
        // `nsapp` mode it is what will be running.
        let mtm = objc2_foundation::MainThreadMarker::new().expect("main thread");
        let app = objc2_app_kit::NSApplication::sharedApplication(mtm);
        app.setActivationPolicy(objc2_app_kit::NSApplicationActivationPolicy::Accessory);

        // Post from a timer rather than before the loop starts: an event
        // posted to a queue nobody is draining yet is a different experiment,
        // and `serve` receives its hotkeys while running, not before.
        let mut n = 0u32;
        beckon_macos::hotkey::add_tick(
            0.5,
            Box::new(move || {
                n += 1;
                if n == 2 {
                    let mut ev: *mut c_void = std::ptr::null_mut();
                    let e = unsafe { CreateEvent(std::ptr::null(), CLASS, KIND, 0.0, 0, &mut ev) };
                    if e != 0 || ev.is_null() {
                        say(&format!("CreateEvent failed: OSStatus {e}"));
                        std::process::exit(1);
                    }
                    let p = unsafe { PostEventToQueue(GetMainEventQueue(), ev, PRIORITY_STANDARD) };
                    unsafe { ReleaseEvent(ev) };
                    if p != 0 {
                        say(&format!("PostEventToQueue failed: OSStatus {p}"));
                        std::process::exit(1);
                    }
                    say("posted one BKPB event to the main event queue");
                }
                if n >= 4 {
                    if DISPATCHED.load(Ordering::SeqCst) {
                        say("DISPATCHED: the handler ran");
                        say("VERDICT: this loop DOES pump the Carbon application event queue");
                        std::process::exit(0);
                    }
                    say("SILENT: the handler never ran");
                    say("VERDICT: this loop does NOT pump it -- read against the other mode");
                    std::process::exit(4);
                }
            }),
        );

        match mode.as_str() {
            "carbon" => beckon_macos::hotkey::HotkeyManager::run_carbon_event_loop_for_probe(),
            _ => beckon_macos::hotkey::HotkeyManager::run_forever(),
        }
    }
}
