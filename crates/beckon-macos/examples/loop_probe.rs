//! Does an AppKit control receive events under the loop `serve` actually
//! runs?
//!
//! This is design §5 of `docs/superpowers/specs/2026-08-12-macos-tray-design.md`,
//! the question that spec told the implementer to answer BEFORE writing any
//! view code. It was not answered, and both `tray.rs` and
//! `settings_window.rs` were written anyway. Everything in the settings
//! window rests on the answer: `hotkey::run_forever` calls Carbon's
//! `RunApplicationEventLoop`, and `NSApp` is *instantiated* (because
//! `NSStatusBar` requires it to exist) but never `run`. If a Cocoa control
//! never sees a mouse-down under that loop, every button in the four doors
//! is decoration.
//!
//! ## The shape of the measurement
//!
//! One view hierarchy, two loops, selected by argv, so the difference in the
//! output IS the result and cannot be a difference in the thing under test:
//!
//! ```text
//! cargo run -p beckon-macos --example loop_probe -- carbon
//! cargo run -p beckon-macos --example loop_probe -- nsapp
//! ```
//!
//! **Run it from Terminal.app.** An SSH shell — and a shell under a coding
//! agent — is in the `Background` bootstrap namespace, where AppKit hands
//! back live objects and draws nothing, so a negative result there would
//! prove nothing at all. The probe refuses rather than lie: it checks
//! `launchctl managername` for `Aqua` first, the same guard
//! `settings_probe.rs` and `tray_probe.rs` use.
//!
//! ## It does not need a person
//!
//! The button is pressed from *outside* the process, over the Accessibility
//! API, by the driver in `testing/macos_loop_probe.sh`. That is the whole
//! point: a probe a human has to click can only be run when a human is
//! there, which is exactly why §5 sat open. Pressing it through AX also
//! measures something a human click would not — that the window is a real
//! accessibility citizen, which is what any later automated UI test needs.
//!
//! ## Reading the output
//!
//! - `HEARTBEAT` proves the run loop is turning at all. Without it, a silent
//!   `PRESS` result means "nothing is running", not "Cocoa gets no events".
//! - `AX-VISIBLE` proves the driver could see the button before it tried to
//!   press it, so a missing `FIRED` is about event delivery rather than
//!   about the probe failing to find its target. **A test with no positive
//!   control cannot tell a clean negative from a broken detector** — that
//!   rule is written into this repo three times over, and this line is it.
//! - `FIRED` is the answer.

fn main() {
    #[cfg(not(target_os = "macos"))]
    {
        eprintln!("loop_probe is macOS-only");
        std::process::exit(2);
    }
    #[cfg(target_os = "macos")]
    mac::run();
}

// An unconditional `fn main` that dispatches into a cfg-gated module, never
// `#![cfg(target_os = "macos")]` at the top of the file. That inner attribute
// applies to the whole CRATE: off macOS the file becomes empty, `main` goes
// with it, and the example fails E0601 rather than compiling to a no-op. It
// is not hypothetical — `beckon-windows/examples/pill_probe.rs` did exactly
// that and CI was red from the merge through the v0.9.4 tag, because the one
// step that can see it (`cargo check --workspace --all-targets`, unexcluded)
// and the file itself lived on different branches until they met.
#[cfg(target_os = "macos")]
mod mac {
    use objc2::rc::Retained;
    use objc2::runtime::{AnyObject, NSObject, NSObjectProtocol};
    use objc2::{define_class, msg_send, sel, MainThreadOnly};
    use objc2_app_kit::{
        NSApplication, NSApplicationActivationPolicy, NSBackingStoreType, NSButton, NSStackView,
        NSTextField, NSUserInterfaceLayoutOrientation, NSWindow, NSWindowStyleMask,
    };
    use objc2_foundation::{MainThreadMarker, NSPoint, NSRect, NSSize, NSString};
    use std::io::Write;

    fn say(line: &str) {
        println!("{line}");
        let _ = std::io::stdout().flush();
    }

    // CoreGraphics event posting, hand-rolled the way `src/ffi.rs` does the
    // AX surface: four symbols is not worth a dependency.
    //
    // **`PRESS=hid` exists because `postEvent:atStart:` answers a narrower
    // question than it appears to.** That call puts an event on
    // `NSApplication`'s OWN queue, and the only thing that drains that queue
    // is `[NSApp nextEventMatchingMask:]` from inside `[NSApp run]`. So under
    // a loop where `isRunning` is false it is *necessarily* undelivered, and
    // measuring it proves the tautology rather than the thing anyone cares
    // about: whether a real click works. A real click arrives from the window
    // server, and `CGEventPost(kCGHIDEventTap, ...)` is that same path —
    // injected below the application, not inside it.
    #[link(name = "CoreGraphics", kind = "framework")]
    unsafe extern "C" {
        fn CGEventCreateMouseEvent(
            source: *const std::ffi::c_void,
            mouse_type: u32,
            pos: CGPoint,
            button: u32,
        ) -> *mut std::ffi::c_void;
        fn CGEventPost(tap: u32, event: *mut std::ffi::c_void);
    }
    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CFRelease(cf: *mut std::ffi::c_void);
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct CGPoint {
        pub x: f64,
        pub y: f64,
    }

    const K_CG_EVENT_LEFT_MOUSE_DOWN: u32 = 1;
    const K_CG_EVENT_LEFT_MOUSE_UP: u32 = 2;
    const K_CG_HID_EVENT_TAP: u32 = 0;

    /// Click at a point in **CoreGraphics global display coordinates**:
    /// origin top-left, y growing downward — the opposite of every AppKit
    /// coordinate in this file, which is the mistake this comment exists to
    /// stop. The caller flips.
    fn hid_click(p: CGPoint) {
        unsafe {
            for kind in [K_CG_EVENT_LEFT_MOUSE_DOWN, K_CG_EVENT_LEFT_MOUSE_UP] {
                let ev = CGEventCreateMouseEvent(std::ptr::null(), kind, p, 0);
                if ev.is_null() {
                    say("HID: CGEventCreateMouseEvent returned null");
                    return;
                }
                CGEventPost(K_CG_HID_EVENT_TAP, ev);
                CFRelease(ev);
            }
        }
    }

    define_class!(
        // SAFETY:
        // - NSObject has no subclassing requirements.
        // - Probe does not implement Drop.
        #[unsafe(super(NSObject))]
        #[thread_kind = MainThreadOnly]
        #[name = "BeckonLoopProbe"]
        struct Probe;

        unsafe impl NSObjectProtocol for Probe {}

        impl Probe {
            #[unsafe(method(beckonHit:))]
            fn hit(&self, _s: Option<&AnyObject>) {
                say("FIRED: the button's action ran");
                say("VERDICT: Cocoa controls DO receive events under this loop");
                // Leave promptly so the driver never has to kill it, and so a
                // hung exit is distinguishable from a hung loop.
                std::process::exit(0);
            }
        }
    );

    pub fn run() {
        let manager = std::process::Command::new("launchctl")
            .arg("managername")
            .output()
            .ok()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .unwrap_or_default();
        say(&format!("bootstrap namespace : {manager}"));
        if manager != "Aqua" {
            say("REFUSING: not an Aqua session. AppKit would hand back live objects and");
            say("draw nothing, so a negative result here would prove nothing at all.");
            std::process::exit(3);
        }

        let mode = std::env::args().nth(1).unwrap_or_default();
        if mode != "carbon" && mode != "nsapp" {
            say("usage: loop_probe <carbon|nsapp>");
            std::process::exit(2);
        }
        say(&format!("MODE: {mode}"));

        let mtm = MainThreadMarker::new().expect("main thread");
        let app = NSApplication::sharedApplication(mtm);
        // What `tray.rs` achieves with TransformProcessType(→ kProcessTransformToUIElementApplication).
        // Accessory is the Cocoa spelling of the same state: no Dock tile, no
        // menu bar, windows still real. `serve` is in that state when the
        // settings window opens, so it is the default here — a probe that
        // measured a Regular app would be measuring a different program.
        //
        // `POLICY=regular` isolates it, because the activation policy turned
        // out to be a variable and not a constant: with Accessory, System
        // Events reported the process but `count of windows` = 0 on a window
        // that had already been ordered front under a turning loop. Two
        // variables (loop, policy) and one observation is not a measurement,
        // so the policy gets its own switch.
        let policy = std::env::var("POLICY").unwrap_or_default();
        let regular = policy == "regular";
        app.setActivationPolicy(if regular {
            NSApplicationActivationPolicy::Regular
        } else {
            NSApplicationActivationPolicy::Accessory
        });
        say(&format!(
            "POLICY: {}",
            if regular { "Regular" } else { "Accessory" }
        ));

        let target: Retained<Probe> = unsafe { msg_send![Probe::alloc(mtm), init] };

        let button = unsafe {
            NSButton::buttonWithTitle_target_action(
                &NSString::from_str("PressMe"),
                Some(&*target as &AnyObject),
                Some(sel!(beckonHit:)),
                mtm,
            )
        };
        let caption = NSTextField::labelWithString(&NSString::from_str("beckon loop probe"), mtm);

        let stack = NSStackView::new(mtm);
        stack.setOrientation(NSUserInterfaceLayoutOrientation::Vertical);
        stack.setSpacing(12.0);
        stack.addArrangedSubview(&caption);
        stack.addArrangedSubview(&button);

        let window = unsafe {
            NSWindow::initWithContentRect_styleMask_backing_defer(
                NSWindow::alloc(mtm),
                NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(320.0, 140.0)),
                NSWindowStyleMask::Titled | NSWindowStyleMask::Closable,
                NSBackingStoreType::Buffered,
                false,
            )
        };
        unsafe {
            window.setTitle(&NSString::from_str("beckon loop probe"));
            window.setContentView(Some(&stack));
        }
        window.center();
        // Above ordinary windows.
        //
        // **Measured, not decorative.** A window-server click lands on
        // whatever window is at that point, and the probe is launched through
        // Terminal.app, whose window is centred over the same pixels. The
        // control run that exposed it: the injector reported
        // `AXIsProcessTrusted: true` and `POSTED`, and the button still did
        // not fire — in `nsapp` mode, where the in-process route had already
        // succeeded, so the loop was not the variable. An in-process
        // `postEvent:` carries a `windowNumber` and ignores z-order
        // entirely; a HID click cannot.
        unsafe { window.setLevel(objc2_app_kit::NSStatusWindowLevel) };
        window.makeKeyAndOrderFront(None);
        // An Accessory app is not frontmost by default, and an AX press wants
        // a window that is actually on screen.
        app.activate();

        say(&format!("isRunning (before loop) : {}", app.isRunning()));
        say("WINDOW: up.");

        // Heartbeat, so a silent result is readable. Without it, "no FIRED"
        // covers both "Cocoa saw nothing" and "nothing is turning at all",
        // which are opposite conclusions.
        //
        // On beat 2 the probe presses its OWN button, by posting a synthetic
        // mouse pair into `NSApp`'s event queue.
        //
        // **That is the measurement, and it is deliberately not an
        // Accessibility press.** Driving it from outside was tried first and
        // the observer turned out to be blind: System Events reported
        // `count of windows` = 0 for this probe — and, when asked as a
        // control, 0 for Terminal and for Finder as well, on a machine where
        // `AXIsProcessTrusted()` answers true and System Events' own
        // `UI elements enabled` answers true. An AX press would therefore
        // have measured the grant, not the loop.
        //
        // `postEvent:atStart:` puts the event on the queue that
        // `NSApplication` drains. Whether anything drains that queue while
        // Carbon's `RunApplicationEventLoop` is the loop being run IS design
        // §5, restated exactly. Nothing about this needs a permission, a
        // second process, or a person.
        let btn = button.clone();
        let win = window.clone();
        let mut n = 0u32;
        beckon_macos::hotkey::add_tick(
            1.0,
            Box::new(move || {
                n += 1;
                let mtm = MainThreadMarker::new().expect("main thread");
                let app = NSApplication::sharedApplication(mtm);
                say(&format!("HEARTBEAT {n} isRunning={}", app.isRunning()));
                if n == 2 {
                    let b = btn.bounds();
                    let centre = NSPoint::new(b.size.width / 2.0, b.size.height / 2.0);
                    // `None` converts to the window's base coordinate system,
                    // which is what a `windowNumber`-targeted event wants.
                    let at = unsafe { btn.convertPoint_toView(centre, None) };
                    let wnum = win.windowNumber();

                    let how = std::env::var("PRESS").unwrap_or_default();

                    if how == "external" {
                        // Publish where to click and wait. The injector is a
                        // separate process because the two capabilities live
                        // in different ones on this machine — see
                        // `examples/hid_click.rs`.
                        let scr = unsafe { win.convertPointToScreen(at) };
                        let h = objc2_app_kit::NSScreen::mainScreen(mtm)
                            .map(|s| s.frame().size.height)
                            .unwrap_or(0.0);
                        win.makeKeyAndOrderFront(None);
                        app.activate();
                        say(&format!("CLICK-AT: {:.0} {:.0}", scr.x, h - scr.y));
                        return;
                    }

                    if how == "hid" {
                        // Window-server path: what a real click does.
                        let scr = unsafe { win.convertPointToScreen(at) };
                        let h = objc2_app_kit::NSScreen::mainScreen(mtm)
                            .map(|s| s.frame().size.height)
                            .unwrap_or(0.0);
                        let p = CGPoint {
                            x: scr.x,
                            // AppKit screen coords grow upward from the
                            // bottom-left; CG display coords grow downward
                            // from the top-left.
                            y: h - scr.y,
                        };
                        say(&format!(
                            "SELF-PRESS(hid): CG ({:.0},{:.0}) from AppKit ({:.0},{:.0}), screen h={h:.0}",
                            p.x, p.y, scr.x, scr.y
                        ));
                        // Front and key first: a HID click lands on whatever
                        // window is at that point, so this is both correctness
                        // and the reason the probe must not be run over a
                        // window the user cares about.
                        win.makeKeyAndOrderFront(None);
                        app.activate();
                        hid_click(p);
                        return;
                    }

                    say(&format!(
                        "SELF-PRESS(post): posting at ({:.0},{:.0}) window {}",
                        at.x, at.y, wnum
                    ));
                    for (kind, click) in [
                        (objc2_app_kit::NSEventType::LeftMouseDown, 1isize),
                        (objc2_app_kit::NSEventType::LeftMouseUp, 1isize),
                    ] {
                        let ev = unsafe {
                            objc2_app_kit::NSEvent::mouseEventWithType_location_modifierFlags_timestamp_windowNumber_context_eventNumber_clickCount_pressure(
                                kind,
                                at,
                                objc2_app_kit::NSEventModifierFlags::empty(),
                                0.0,
                                wnum,
                                None,
                                0,
                                click,
                                1.0,
                            )
                        };
                        match ev {
                            Some(ev) => unsafe { app.postEvent_atStart(&ev, false) },
                            None => say("SELF-PRESS: NSEvent constructor returned nil"),
                        }
                    }
                }
                // `external` has to wait for a second process to be launched,
                // read the coordinates and post; the in-process routes fire
                // on the next turn or never.
                let budget = if std::env::var("PRESS").as_deref() == Ok("external") {
                    20
                } else {
                    8
                };
                if n >= budget {
                    say("NOT-FIRED: the click never reached the button");
                    say("VERDICT: this loop does NOT deliver mouse events to Cocoa controls");
                    std::process::exit(4);
                }
            }),
        );

        match mode.as_str() {
            // The loop `serve` actually runs today.
            "carbon" => beckon_macos::hotkey::HotkeyManager::run_forever(),
            // The ordinary Cocoa loop, as the control.
            _ => {
                app.run();
                unreachable!("NSApplication::run returned");
            }
        }
    }
}
