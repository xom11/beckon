//! Does `NSWindow::displayIfNeeded()` force drawing to happen SYNCHRONOUSLY
//! at the call site, or does AppKit defer it to the next run-loop turn?
//!
//! ```text
//! CARGO_TARGET_DIR=/tmp/beckon-flush cargo run -p beckon-macos --example flush_paint_probe
//! ```
//!
//! This is Task 6's `flush_paint` measurement. `settings_window::flush_paint`
//! calls `displayIfNeeded` on the window immediately before `serve` blocks
//! this thread on a network check (Task 7), and the entire point of the call
//! is that the pending frame reaches the drawing code BEFORE the block, not
//! after. This probe isolates exactly that question, two independent ways:
//!
//! 1. **`needsDisplay` before/after.** Mark a view dirty, call
//!    `displayIfNeeded`, and read `needsDisplay()` back IMMEDIATELY — no run
//!    loop is ever pumped in this probe, so the only way the flag can be
//!    `false` afterward is if `displayIfNeeded` itself cleared it
//!    synchronously within the call.
//! 2. **A `drawRect:` counter.** A custom `NSView` subclass increments a
//!    counter inside its own `drawRect:`. If `displayIfNeeded` is genuinely
//!    synchronous, the counter is non-zero the instant the call returns.
//!
//! Each has a CONTROL: the identical setup with the `displayIfNeeded` call
//! removed. A probe that only reports the test side and never runs the
//! control cannot distinguish "it worked" from "nothing happened" — this
//! repository has lost sessions to exactly that shape (see `CLAUDE.md`,
//! "A measurement on one OS is data about that OS, not about the design").
//!
//! **What this does NOT and cannot answer: whether the frame reaches a
//! physical display.** `drawRect:` running proves the view drew into the
//! window's backing store; it does not prove a Core Animation commit reached
//! the window server and got composited on screen. This host (`macmini`) has
//! no visible display for this session to photograph — see the narrowed
//! `macos-ssh-background-namespace` note: offscreen `cacheDisplay` pixels and
//! AppKit layout are both readable from the Background namespace, but an
//! ON-SCREEN window is not. That on-screen half is left unverified here.
//!
//! Deliberately NOT gated on Aqua, on `geom_probe`'s reasoning restated for
//! drawing rather than layout: `displayIfNeeded` operates on the window's own
//! dirty-region state machine, which is a live AppKit object in the
//! Background namespace the same way layout is. `launchctl managername` is
//! printed rather than enforced, and the TEST/CONTROL split is what turns a
//! null result in Background into a visible null result instead of a false
//! "it works".

fn main() {
    let manager = std::process::Command::new("launchctl")
        .arg("managername")
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default();
    println!("bootstrap namespace : {manager}");

    #[cfg(not(target_os = "macos"))]
    {
        eprintln!("flush_paint_probe is macOS-only");
        std::process::exit(2);
    }

    #[cfg(target_os = "macos")]
    {
        use objc2::rc::Retained;
        use objc2::{define_class, msg_send, MainThreadOnly};
        use objc2_app_kit::{
            NSApplication, NSBackingStoreType, NSView, NSWindow, NSWindowStyleMask,
        };
        use objc2_foundation::{MainThreadMarker, NSPoint, NSRect, NSSize};
        use std::sync::atomic::{AtomicUsize, Ordering};

        static DRAW_COUNT: AtomicUsize = AtomicUsize::new(0);

        define_class!(
            // SAFETY:
            // - NSView's subclassing requirements (none beyond what NSObject
            //   needs) are met.
            // - ProbeView does not implement Drop.
            #[unsafe(super(NSView))]
            #[thread_kind = MainThreadOnly]
            #[name = "BeckonFlushPaintProbeView"]
            struct ProbeView;

            impl ProbeView {
                #[unsafe(method(drawRect:))]
                fn draw_rect(&self, _rect: NSRect) {
                    DRAW_COUNT.fetch_add(1, Ordering::SeqCst);
                    eprintln!("  drawRect: fired");
                }
            }
        );

        let mtm = MainThreadMarker::new().expect("examples run on the main thread");
        // Same as `open_existing`'s `raise`: a shared NSApplication is what a
        // window normally exists inside of.
        let _app = NSApplication::sharedApplication(mtm);

        fn make_window(mtm: MainThreadMarker) -> (Retained<NSWindow>, Retained<ProbeView>) {
            let rect = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(200.0, 200.0));
            let window = unsafe {
                NSWindow::initWithContentRect_styleMask_backing_defer(
                    NSWindow::alloc(mtm),
                    rect,
                    NSWindowStyleMask::Titled,
                    NSBackingStoreType::Buffered,
                    false,
                )
            };
            let view: Retained<ProbeView> =
                unsafe { msg_send![ProbeView::alloc(mtm), initWithFrame: rect] };
            window.setContentView(Some(&view));
            // Diagnostic addition: probe 2 (below) came back 0/0 without
            // this -- ordering the window is what gives it a backing store
            // to paint into at all. `orderFrontRegardless` rather than
            // `orderFront:`/`makeKeyAndOrderFront:` because this session has
            // no screen to become key on.
            window.orderFrontRegardless();
            (window, view)
        }

        println!();
        println!("-- probe 1: needsDisplay before/after --");
        let (test_win, test_view) = make_window(mtm);
        test_view.setNeedsDisplay(true);
        let before = test_view.needsDisplay();
        test_win.displayIfNeeded();
        let after = test_view.needsDisplay();
        println!("  TEST    needsDisplay before={before} after={after} (call MADE)");

        let (_ctrl_win, ctrl_view) = make_window(mtm);
        ctrl_view.setNeedsDisplay(true);
        let c_before = ctrl_view.needsDisplay();
        let c_after = ctrl_view.needsDisplay(); // no displayIfNeeded call in between
        println!("  CONTROL needsDisplay before={c_before} after={c_after} (call REMOVED)");

        println!();
        println!("-- probe 2a: sanity check -- does the override even fire when SENT directly? --");
        DRAW_COUNT.store(0, Ordering::SeqCst);
        let (_sanity_win, sanity_view) = make_window(mtm);
        let zero_rect = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(0.0, 0.0));
        let _: () = unsafe { msg_send![&*sanity_view, drawRect: zero_rect] };
        println!(
            "  direct drawRect: send -> counter = {} (must be 1 for the rest of probe 2 to mean anything)",
            DRAW_COUNT.load(Ordering::SeqCst)
        );

        println!();
        println!("-- probe 2: drawRect: counter, via displayIfNeeded --");
        DRAW_COUNT.store(0, Ordering::SeqCst);
        let (test_win2, test_view2) = make_window(mtm);
        // Force the traditional drawRect: path off the CALayer one: recent
        // AppKit defaults many views to layer-backed, where the real draw
        // hook is `updateLayer`, not `drawRect:` -- the deprecation notice
        // on `canDraw` below says as much ("-drawRect: or -updateLayer").
        test_view2.setWantsLayer(false);
        test_view2.setNeedsDisplay(true);
        // `canDraw` is deprecated in favour of checking `-window` /
        // `-isHiddenOrHasHiddenAncestor` -- kept here anyway, allowed
        // explicitly, because this is throwaway diagnostic output for a
        // one-off measurement, not code this repository ships.
        #[allow(deprecated)]
        let can_draw = test_view2.canDraw();
        println!(
            "  (diagnostic) isVisible={} canDraw={} wantsLayer={}",
            test_win2.isVisible(),
            can_draw,
            test_view2.wantsLayer()
        );
        test_win2.displayIfNeeded();
        let test_draws = DRAW_COUNT.load(Ordering::SeqCst);
        println!("  TEST    drawRect count = {test_draws} (call MADE)");

        DRAW_COUNT.store(0, Ordering::SeqCst);
        let (_ctrl_win2, ctrl_view2) = make_window(mtm);
        ctrl_view2.setWantsLayer(false);
        ctrl_view2.setNeedsDisplay(true);
        // deliberately NOT calling displayIfNeeded here -- this is the control
        let ctrl_draws = DRAW_COUNT.load(Ordering::SeqCst);
        println!("  CONTROL drawRect count = {ctrl_draws} (call REMOVED)");

        println!();
        // The needsDisplay probe is decisive only if the flag was actually
        // set beforehand (before == true) -- otherwise there was nothing to
        // clear and a `false` afterward proves nothing.
        let needs_display_synchronous = before && !after;
        let needs_display_control_ok = c_before && c_after;
        let draw_synchronous = test_draws > 0;
        let draw_control_ok = ctrl_draws == 0;
        println!(
            "needsDisplay probe : synchronous={needs_display_synchronous} control_ok={needs_display_control_ok}"
        );
        println!(
            "drawRect probe     : synchronous={draw_synchronous} control_ok={draw_control_ok}"
        );
        if needs_display_synchronous
            && needs_display_control_ok
            && draw_synchronous
            && draw_control_ok
        {
            println!(
                "RESULT: displayIfNeeded forces synchronous drawing at the call site (both probes agree, control clean)."
            );
        } else if !before && test_draws == 0 && ctrl_draws == 0 {
            println!(
                "RESULT: NULL -- neither probe drew anything, in test OR control. Likely this \
                 session's Background namespace refuses to display at all, even offscreen. \
                 Cannot confirm OR refute displayIfNeeded's synchronicity from here."
            );
        } else {
            println!("RESULT: MIXED -- read the raw before/after/count numbers above by hand.");
        }
    }
}
