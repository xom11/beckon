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
//! ## Three ways to press it, and why there are three
//!
//! `PRESS=` selects one. They are not redundant; each answers a different
//! question, and the first two were each believed to be the whole answer
//! until a control said otherwise.
//!
//! - **`post`** (default) — `NSApp.postEvent:atStart:`, in-process. Cheapest
//!   and needs no permission, but it answers a *narrower* question than it
//!   looks: it enqueues onto `NSApplication`'s own queue, which only
//!   `[NSApp nextEventMatchingMask:]` inside `[NSApp run]` drains. Under a
//!   loop where `isRunning` is false it is undelivered by construction, so a
//!   negative here proves `NSApp` is not running — not that a real click
//!   fails.
//! - **`hid`** — `CGEventPost(kCGHIDEventTap)` from inside the probe: the
//!   path a real click takes. Currently useless from a Terminal-launched
//!   probe, which is not Accessibility-trusted, and an untrusted
//!   `CGEventPost` is a **silent** no-op.
//! - **`external`** — the probe publishes `CLICK-AT` and a separate,
//!   trusted process (`examples/hid_click.rs`) posts. This is the shape that
//!   can eventually give every door an automated click-and-assert loop.
//!
//! ## The permission split that makes this awkward
//!
//! Measured 2026-08-16, and invisible from inside any one process:
//!
//! | | agent's shell | Terminal.app |
//! |---|---|---|
//! | Aqua session, i.e. can draw | no (`Background`) | yes |
//! | `AXIsProcessTrusted()`, i.e. can post | yes | no |
//!
//! Neither can do both, so the probe draws where it cannot inject and the
//! injector is trusted where there is no session to inject into. One
//! Accessibility grant for Terminal.app collapses the table into one usable
//! process.
//!
//! The Accessibility *inspection* route — System Events, `click button …` —
//! was tried before any of this and abandoned: it reported
//! `count of windows` = 0 for the probe, and, asked as a control, 0 for
//! Terminal and 0 for Finder. The observer was blind, so an AX press would
//! have measured the grant rather than the loop.
//!
//! ## Reading the output
//!
//! - `HEARTBEAT` proves the run loop is turning at all. Without it, a silent
//!   result means "nothing is running", not "Cocoa gets no events" — opposite
//!   conclusions.
//! - `AXIsProcessTrusted` from the injector proves the press was actually
//!   sent. **A test with no positive control cannot tell a clean negative
//!   from a blind detector**; that rule is written into this repo three times
//!   over, and it caught two false leads here.
//! - `FIRED` / `NOT-FIRED` is the answer, and it is only an answer about a
//!   *real* click when the press method was `hid` or `external`.
//!
//! ## The default mode cannot produce the strong claim, so it says so
//!
//! Under `PRESS=post` a `carbon` / `NOT-FIRED` pair is **true by
//! construction**: the event goes on `NSApplication`'s own queue, only
//! `[NSApp run]` drains that queue, and `isRunning` is printed as `false` two
//! lines earlier. That run restates `isRunning`; it does not observe a click.
//! It is still worth having — it is the cheapest possible demonstration that
//! `NSApp` is not running under the Carbon loop, needs no permission and no
//! second process, and the `nsapp` leg of the same pair is a real positive
//! control — but the strong claim, *every control in the settings window was
//! decoration*, is about a **real click** and needs `PRESS=hid` or
//! `PRESS=external`.
//!
//! So `PRESS` is printed with the mode's reach beside it, and every `VERDICT`
//! line is worded for the mode that produced it. A reader who takes a `post`
//! verdict for the strong one has to ignore a sentence to do it.

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

    /// Which press method this run used. Read from the environment in each
    /// place that needs it rather than threaded through, because one of them
    /// is an Objective-C method the probe does not get to pass arguments to.
    fn press_mode() -> String {
        match std::env::var("PRESS").unwrap_or_default().as_str() {
            "hid" => "hid".into(),
            "external" => "external".into(),
            // Anything else, including unset, is the default.
            _ => "post".into(),
        }
    }

    /// How far a verdict from this press method reaches.
    ///
    /// **`post` is narrower than it looks, and this is the whole reason the
    /// mode is printed.** It enqueues onto `NSApplication`'s own queue, which
    /// only `[NSApp nextEventMatchingMask:]` inside `[NSApp run]` drains — so
    /// under a loop where `isRunning` is false it is undelivered *by
    /// construction*, and a `carbon` / `NOT-FIRED` pair from it restates the
    /// `isRunning=false` printed two lines earlier rather than observing
    /// anything about a click. `hid` and `external` go through the window
    /// server, which is the path a real click takes, and only those two can
    /// carry the strong claim.
    fn reach(mode: &str) -> &'static str {
        match mode {
            "hid" | "external" => "a real window-server click",
            _ => "NSApplication's own event queue, NOT a real click",
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
                let mode = press_mode();
                say(&match mode.as_str() {
                    "hid" | "external" => "VERDICT: Cocoa controls DO receive a real \
                                           window-server click under this loop"
                        .to_string(),
                    _ => "VERDICT: this loop DOES drain NSApplication's own event queue. \
                          A real click was not tested -- re-run with PRESS=hid or \
                          PRESS=external for that."
                        .to_string(),
                });
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
        // Printed beside the mode, not left to the reader to remember: the
        // whole difference between "this loop delivers no clicks" and "this
        // loop does not run NSApp" is which press method produced the line
        // below, and the two readings support different decisions.
        let press = press_mode();
        say(&format!("PRESS: {press} -- measures {}", reach(&press)));

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
        {
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
        window.setLevel(objc2_app_kit::NSStatusWindowLevel);
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
                    let at = { btn.convertPoint_toView(centre, None) };
                    let wnum = win.windowNumber();

                    let how = press_mode();

                    if how == "external" {
                        // Publish where to click and wait. The injector is a
                        // separate process because the two capabilities live
                        // in different ones on this machine — see
                        // `examples/hid_click.rs`.
                        let scr = { win.convertPointToScreen(at) };
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
                        let scr = { win.convertPointToScreen(at) };
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
                        let ev = {
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
                            Some(ev) => app.postEvent_atStart(&ev, false),
                            None => say("SELF-PRESS: NSEvent constructor returned nil"),
                        }
                    }
                }
                // `external` has to wait for a second process to be launched,
                // read the coordinates and post; the in-process routes fire
                // on the next turn or never.
                let budget = if press_mode() == "external" { 20 } else { 8 };
                if n >= budget {
                    say("NOT-FIRED: the press never reached the button");
                    let mode = press_mode();
                    say(&match mode.as_str() {
                        "hid" | "external" => "VERDICT: this loop does NOT deliver a real \
                                               window-server click to Cocoa controls"
                            .to_string(),
                        // Deliberately weaker than the line it replaced. Under
                        // `post` this is a restatement of `isRunning=false`,
                        // and reading it as "a click fails" is the one wrong
                        // conclusion this probe can be made to support.
                        _ => "VERDICT: this loop does NOT drain NSApplication's own event \
                              queue, which is what isRunning above already says. That is \
                              NOT yet evidence that a real click fails -- re-run with \
                              PRESS=hid or PRESS=external for that."
                            .to_string(),
                    });
                    std::process::exit(4);
                }
            }),
        );

        match mode.as_str() {
            // The loop `serve` actually runs today.
            "carbon" => beckon_macos::hotkey::HotkeyManager::run_carbon_event_loop_for_probe(),
            // The ordinary Cocoa loop, as the control.
            _ => {
                app.run();
                unreachable!("NSApplication::run returned");
            }
        }
    }
}
