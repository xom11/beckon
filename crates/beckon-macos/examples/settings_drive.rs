//! Drive the real settings window and assert its controls answer.
//!
//! `settings_probe.rs` opens the window and waits for a person. This one
//! presses the buttons itself and checks what came back, so the four doors
//! have a regression test that runs unattended.
//!
//! ```text
//! cargo run -p beckon-macos --example settings_drive
//! ```
//!
//! ## Why this needs no permission, when nothing else did
//!
//! Every earlier attempt to press something went **through the window
//! server** — `CGEventPost`, or System Events — and every one hit a wall
//! that is invisible from inside the process:
//!
//! | | agent's shell | Terminal.app |
//! |---|---|---|
//! | Aqua session, i.e. can draw | no | yes |
//! | `AXIsProcessTrusted()`, i.e. `CGEventPost` lands | yes | no |
//! | Automation → System Events | yes | **hangs on a consent dialog** |
//!
//! Neither process can do both, and the third row does not even fail: it
//! hangs, waiting for a click on a screen with nobody in front of it.
//!
//! `NSApp.postEvent:atStart:` goes nowhere near any of that. It puts the
//! event on `NSApplication`'s own queue, which `[NSApp run]` drains, and
//! `sendEvent:` routes it to the window and thence to the view — the whole
//! path from `sendEvent:` down, which is exactly the part beckon writes.
//! `loop_probe.rs` measured that it reaches a real `NSButton` and runs its
//! action (`FIRED`, under `nsapp`), so the mechanism is not assumed here; it
//! is the earlier probe's result being spent.
//!
//! **What this therefore does NOT prove**, stated so nobody upgrades it
//! later: that a human's click reaches the window. That is the window
//! server's half, and it needs one of the grants above. What it proves is
//! that once an event arrives, every control in the four doors is wired to
//! something and the something fires — which is where the bugs live.
//!
//! ## Reading the output
//!
//! One line per step, `PASS` or `FAIL`, and a non-zero exit if any failed.
//! The first step is a **control**: it asserts the window was found and its
//! controls enumerated. Without it a wall of `FAIL` would read as "the
//! window is broken" when it may only mean the driver never found it.

fn main() {
    #[cfg(not(target_os = "macos"))]
    {
        eprintln!("settings_drive is macOS-only");
        std::process::exit(2);
    }
    #[cfg(target_os = "macos")]
    mac::run();
}

#[cfg(target_os = "macos")]
mod mac {
    use beckon_core::settings::{control_state, Callbacks, Model, Page, Paths, RuntimeStatus};
    use beckon_macos::settings_window as win;
    use objc2::rc::Retained;
    use objc2_app_kit::{
        NSApplication, NSButton, NSEvent, NSEventModifierFlags, NSEventType, NSSegmentedControl,
        NSView, NSWindow,
    };
    use objc2_foundation::{MainThreadMarker, NSPoint};
    use std::cell::RefCell;
    use std::io::Write;
    use std::rc::Rc;

    fn say(l: &str) {
        println!("{l}");
        let _ = std::io::stdout().flush();
    }

    thread_local! {
        /// Every callback the window raised, in order. The assertions read
        /// this rather than the model, because a control that mutates the
        /// model without raising its callback is still broken — `serve` only
        /// ever learns through the callback.
        static LOG: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
        static FAILED: RefCell<u32> = const { RefCell::new(0) };
    }

    fn note(s: String) {
        LOG.with(|l| l.borrow_mut().push(s));
    }
    fn drain() -> Vec<String> {
        LOG.with(|l| std::mem::take(&mut *l.borrow_mut()))
    }

    fn check(step: &str, ok: bool, saw: &[String]) {
        if ok {
            say(&format!("PASS  {step}"));
        } else {
            say(&format!("FAIL  {step}   saw: {saw:?}"));
            FAILED.with(|f| *f.borrow_mut() += 1);
        }
    }

    // --- finding things -----------------------------------------------------

    /// The settings window, found by TITLE.
    ///
    /// **Not `windows().next()`**, which is what the first version did and
    /// what the control caught: `NSApplication` owns more windows than the
    /// one this crate made — AppKit creates its own — and the first is not
    /// ours. That returned a four-view window with no strip and no `Save`,
    /// which reads exactly like "the settings window is broken".
    fn our_window(mtm: MainThreadMarker) -> Option<Retained<NSWindow>> {
        let app = NSApplication::sharedApplication(mtm);
        app.windows()
            .iter()
            .find(|w| w.title().to_string().starts_with("beckon"))
    }

    /// Every window this process owns, for the control step to print when it
    /// cannot find the right one.
    fn window_report(mtm: MainThreadMarker) -> String {
        let app = NSApplication::sharedApplication(mtm);
        app.windows()
            .iter()
            .map(|w| {
                let f = w.frame();
                format!(
                    "[{:?} {:.0}x{:.0} vis={}]",
                    w.title().to_string(),
                    f.size.width,
                    f.size.height,
                    w.isVisible()
                )
            })
            .collect::<Vec<_>>()
            .join(" ")
    }

    fn walk(v: &NSView, out: &mut Vec<Retained<NSView>>) {
        for sub in v.subviews().iter() {
            out.push(sub.clone());
            walk(&sub, out);
        }
    }

    fn all_views(w: &NSWindow) -> Vec<Retained<NSView>> {
        let mut out = Vec::new();
        if let Some(root) = w.contentView() {
            walk(&root, &mut out);
        }
        out
    }

    fn button_titled(views: &[Retained<NSView>], title: &str) -> Option<Retained<NSButton>> {
        views.iter().find_map(|v| {
            let b = v.downcast_ref::<NSButton>()?;
            (b.title().to_string() == title).then(|| Retained::from(b))
        })
    }

    fn segmented(views: &[Retained<NSView>]) -> Option<Retained<NSSegmentedControl>> {
        views
            .iter()
            .find_map(|v| v.downcast_ref::<NSSegmentedControl>().map(Retained::from))
    }

    // --- pressing things ----------------------------------------------------

    /// Post a click at a point given in `view`'s own coordinates.
    ///
    /// `windowNumber` is carried on the event, so this reaches our window
    /// whatever is in front of it — which is not a shortcut but the reason
    /// this is a better test harness than a HID click: no z-order to lose to,
    /// and no cursor moved under the user.
    fn click_in(view: &NSView, at: NSPoint, w: &NSWindow, mtm: MainThreadMarker) {
        let app = NSApplication::sharedApplication(mtm);
        let in_window = view.convertPoint_toView(at, None);
        // **Ask the window what is actually at that point before posting.**
        // A click that lands on a view in FRONT of the target produces an
        // empty log that reads exactly like a control wired to nothing, and
        // this is the one line that separates the two. `hitTest:` takes a
        // point in the receiver's SUPERVIEW space, so the content view is
        // asked in window coordinates -- which is what `in_window` is.
        if let Some(root) = w.contentView() {
            let hit = root.hitTest(in_window);
            let name = hit
                .as_ref()
                .map(|h| h.class().name().to_string_lossy().into_owned())
                .unwrap_or_else(|| "<nothing>".into());
            let want = view.class().name().to_string_lossy().into_owned();
            let same = hit
                .as_ref()
                .map(|h| std::ptr::eq(&**h as *const NSView, view as *const NSView))
                .unwrap_or(false);
            say(&format!(
                "      hit ({:.0},{:.0}) -> {name}{}  (target {want})",
                in_window.x,
                in_window.y,
                if same { " ==TARGET" } else { "" }
            ));
        }
        let wnum = w.windowNumber();
        for kind in [NSEventType::LeftMouseDown, NSEventType::LeftMouseUp] {
            let ev = {
                NSEvent::mouseEventWithType_location_modifierFlags_timestamp_windowNumber_context_eventNumber_clickCount_pressure(
                    kind,
                    in_window,
                    NSEventModifierFlags::empty(),
                    0.0,
                    wnum,
                    None,
                    0,
                    1,
                    1.0,
                )
            };
            if let Some(ev) = ev {
                app.postEvent_atStart(&ev, false);
            }
        }
    }

    /// A button's state at the moment it is clicked.
    ///
    /// **Printed on every button step, not only on failure.** A click that
    /// does nothing because the control is DISABLED is correct behaviour, and
    /// a click that does nothing because the wiring is broken is a defect;
    /// they produce the same empty log. This is the line that tells them
    /// apart, and adding it only after a failure would mean re-running to
    /// learn what the run just saw.
    fn state_of(b: &NSButton) -> String {
        format!(
            "enabled={} hidden={} frame={:.0}x{:.0}",
            b.isEnabled(),
            b.isHidden(),
            b.frame().size.width,
            b.frame().size.height
        )
    }

    fn click_centre(v: &NSView, w: &NSWindow, mtm: MainThreadMarker) {
        let b = v.bounds();
        click_in(
            v,
            NSPoint::new(b.size.width / 2.0, b.size.height / 2.0),
            w,
            mtm,
        );
    }

    /// Click segment `i` of `n`. `NSSegmentedControl` exposes no per-segment
    /// rect, so the point is derived from the control's own width — which is
    /// exact for the equal-width segments this strip uses, and is why the
    /// Shortcuts segment's width is PINNED rather than left to the badge.
    fn click_segment(
        sc: &NSSegmentedControl,
        i: usize,
        n: usize,
        w: &NSWindow,
        mtm: MainThreadMarker,
    ) {
        let b = sc.bounds();
        let x = b.size.width * (i as f64 + 0.5) / n as f64;
        click_in(sc, NSPoint::new(x, b.size.height / 2.0), w, mtm);
    }

    // --- the run ------------------------------------------------------------

    pub fn run() {
        let manager = std::process::Command::new("launchctl")
            .arg("managername")
            .output()
            .ok()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .unwrap_or_default();
        say(&format!("bootstrap namespace : {manager}"));
        if manager != "Aqua" {
            say("REFUSING: not an Aqua session. AppKit hands back live objects and draws");
            say("nothing here, so every assertion below would be about a window nobody has.");
            std::process::exit(3);
        }

        const SAMPLE: &str = r#"
"ctrl+super+alt+t" = "kitty"
"ctrl+super+alt+c" = "Claude"
"ctrl+super+alt+b" = "Brave"

[keyboard]
caps = false
caps_tap = "capslock"
caps_hold = "ctrl+super+alt"
"#;
        let model = Rc::new(RefCell::new(
            Model::from_text(SAMPLE).expect("the sample parses"),
        ));

        let refresh = {
            let m = Rc::clone(&model);
            move || {
                let rt = RuntimeStatus {
                    registered: Default::default(),
                    catalog: Some(vec!["Brave".into(), "Claude".into(), "kitty".into()]),
                    paused: false,
                    probe: None,
                };
                let cs = control_state(&m.borrow(), &rt);
                win::apply_state(&cs, false, None);
            }
        };

        let cb = Callbacks {
            on_select: Box::new(|i| note(format!("select {i}"))),
            on_mark: Box::new(|i, on| note(format!("mark {i} {on}"))),
            on_edit_combo: Box::new(|t| note(format!("combo {t}"))),
            on_probe_shortcut: Box::new(|t| note(format!("probe {t}"))),
            on_edit_app: Box::new(|t| note(format!("app {t}"))),
            on_filter: Box::new(|t| note(format!("filter {t}"))),
            on_add: Box::new({
                let m = Rc::clone(&model);
                let r = refresh.clone();
                move || {
                    note("add".into());
                    m.borrow_mut().add_row();
                    r();
                }
            }),
            on_remove: Box::new(|| note("remove".into())),
            on_apply: Box::new(|| note("apply".into())),
            on_reload_from_disk: Box::new(|| note("reload".into())),
            on_keep_mine: Box::new(|| note("keep".into())),
            on_open_file: Box::new(|| note("openfile".into())),
            on_close_request: Box::new(|| {
                note("close".into());
                false // never actually close: the driver owns the lifetime
            }),
            on_caps: Box::new(|on| note(format!("caps {on}"))),
            on_caps_hold: Box::new(|_| note("caps_hold".into())),
            on_caps_tap: Box::new(|_| note("caps_tap".into())),
            on_command: Box::new(|c| note(format!("cmd {c:?}"))),
            on_catalog: Box::new(|c: Vec<String>| note(format!("catalog {}", c.len()))),
        };

        let paths = Paths {
            config: "settings_drive (nothing is written)".into(),
            log: None,
        };
        if let Err(e) = win::open(cb, &paths, Page::Shortcuts) {
            say(&format!("open failed: {e}"));
            std::process::exit(1);
        }
        refresh();
        drain(); // the opening `ShowPage` is not under test

        let mut step = 0u32;
        beckon_macos::hotkey::add_tick(
            0.6,
            Box::new(move || {
                let mtm = MainThreadMarker::new().expect("main thread");
                let Some(w) = our_window(mtm) else {
                    say(&format!(
                        "FAIL  control: no window titled `beckon`. NSApp has: {}",
                        window_report(mtm)
                    ));
                    std::process::exit(1);
                };
                let views = all_views(&w);
                step += 1;
                match step {
                    // The control. Everything below is meaningless if the
                    // driver cannot see the window's controls at all.
                    1 => {
                        // Where every named control actually IS, in window
                        // coordinates, against the window's own bounds. A
                        // control laid out beyond the frame is invisible to a
                        // click AND to a person, and is the class of defect
                        // the Windows twin only ever caught by photograph.
                        if let Some(root) = w.contentView() {
                            let rb = root.bounds();
                            say(&format!(
                                "      root fittingSize {:.0}x{:.0}   window frame {:.0}x{:.0}",
                                root.fittingSize().width,
                                root.fittingSize().height,
                                w.frame().size.width,
                                w.frame().size.height
                            ));
                            say(&format!(
                                "      content bounds {:.0}x{:.0}  root frame {:.0}x{:.0} at ({:.0},{:.0})",
                                rb.size.width, rb.size.height,
                                root.frame().size.width, root.frame().size.height,
                                root.frame().origin.x, root.frame().origin.y
                            ));
                            // The root IS the stack. Its arranged subviews are
                            // the strip, the four doors and the bar; if a
                            // hidden door is still contributing height the
                            // whole column overflows and everything inside a
                            // card lands above the window.
                            for (i, sub) in root.subviews().iter().enumerate() {
                                let f = sub.frame();
                                say(&format!(
                                    "        [{i}] {:<28} {:.0}x{:.0} at ({:.0},{:.0}) hidden={}",
                                    sub.class().name().to_string_lossy(),
                                    f.size.width,
                                    f.size.height,
                                    f.origin.x,
                                    f.origin.y,
                                    sub.isHidden()
                                ));
                                let fs = sub.fittingSize();
                                say(&format!(
                                    "            fitting {:.0}x{:.0}",
                                    fs.width, fs.height
                                ));
                                // One level deeper on the door that is OPEN:
                                // a column that is too tall is too tall
                                // because of one child, and naming it is the
                                // difference between a fix and a guess.
                                if !sub.isHidden() && i == 1 {
                                    for (j, k) in sub.subviews().iter().enumerate() {
                                        let g = k.frame();
                                        say(&format!(
                                            "          ({j}) {:<22} {:.0}x{:.0} at ({:.0},{:.0})",
                                            k.class().name().to_string_lossy(),
                                            g.size.width,
                                            g.size.height,
                                            g.origin.x,
                                            g.origin.y
                                        ));
                                        for (m, n) in k.subviews().iter().enumerate() {
                                            let h = n.frame();
                                            say(&format!(
                                                "             .{m} {:<20} {:.0}x{:.0}",
                                                n.class().name().to_string_lossy(),
                                                h.size.width,
                                                h.size.height
                                            ));
                                        }
                                    }
                                }
                            }
                            for name in [
                                "Add",
                                "Remove",
                                "Save",
                                "Close",
                                "Open config file",
                                "Reload",
                            ] {
                                if let Some(b) = button_titled(&views, name) {
                                    let r = b.convertRect_toView(b.bounds(), None);
                                    let out = r.origin.x < 0.0
                                        || r.origin.y < 0.0
                                        || r.origin.x + r.size.width > rb.size.width
                                        || r.origin.y + r.size.height > rb.size.height;
                                    say(&format!(
                                        "      {name:<18} x={:.0} y={:.0} {:.0}x{:.0}{}",
                                        r.origin.x,
                                        r.origin.y,
                                        r.size.width,
                                        r.size.height,
                                        if out { "   *** OUTSIDE ***" } else { "" }
                                    ));
                                }
                            }
                        }
                        let sc = segmented(&views);
                        let save = button_titled(&views, "Save");
                        check(
                            &format!(
                                "control: window found, {} views, strip={} Save={}",
                                views.len(),
                                sc.is_some(),
                                save.is_some()
                            ),
                            sc.is_some() && save.is_some(),
                            &[],
                        );
                        if sc.is_none() || save.is_none() {
                            say(&format!("      windows: {}", window_report(mtm)));
                            std::process::exit(1);
                        }
                    }
                    2 => {
                        // **The window opens at the size it is meant to be.**
                        // It once opened at 640x1080 because `setContentSize`
                        // ran while all four doors were still visible and the
                        // content really did need that much; three doors then
                        // hid and the window kept the frame. 532 = 500 of
                        // content plus the title bar.
                        let f = w.frame();
                        check(
                            &format!(
                                "window opens at 640x532, not stretched ({:.0}x{:.0})",
                                f.size.width, f.size.height
                            ),
                            (f.size.width - 640.0).abs() < 2.0
                                && (f.size.height - 532.0).abs() < 2.0,
                            &[],
                        );
                        if let Some(sc) = segmented(&views) {
                            click_segment(&sc, 2, 4, &w, mtm);
                        }
                    }
                    3 => {
                        let saw = drain();
                        check(
                            "clicking the System pill raises ShowPage(System)",
                            saw.iter().any(|s| s.contains("ShowPage(System)")),
                            &saw,
                        );
                    }
                    4 => {
                        if let Some(sc) = segmented(&views) {
                            click_segment(&sc, 3, 4, &w, mtm);
                        }
                    }
                    5 => {
                        let saw = drain();
                        check(
                            "clicking the About pill raises ShowPage(About)",
                            saw.iter().any(|s| s.contains("ShowPage(About)")),
                            &saw,
                        );
                    }
                    6 => {
                        if let Some(sc) = segmented(&views) {
                            click_segment(&sc, 0, 4, &w, mtm);
                        }
                    }
                    7 => {
                        let saw = drain();
                        check(
                            "and back to Shortcuts",
                            saw.iter().any(|s| s.contains("ShowPage(Shortcuts)")),
                            &saw,
                        );
                    }
                    8 => {
                        // Save is DISABLED on a model nobody has touched --
                        // `apply_enabled` is `dirty && no errors` -- so this
                        // asserts the enablement rather than the click.
                        if let Some(b) = button_titled(&views, "Save") {
                            say(&format!("      Save {}", state_of(&b)));
                            check(
                                "Save starts disabled: there is nothing to save yet",
                                !b.isEnabled(),
                                &[],
                            );
                        }
                        if let Some(b) = button_titled(&views, "Add") {
                            say(&format!("      Add  {}", state_of(&b)));
                            click_centre(&b, &w, mtm);
                        }
                    }
                    9 => {
                        let saw = drain();
                        check(
                            "clicking Add raises on_add",
                            saw.iter().any(|s| s == "add"),
                            &saw,
                        );
                    }
                    10 => {
                        // `on_add` added a row and refreshed, so the model is
                        // dirty now and Save has to have come alive. That
                        // transition is the point of the pair.
                        if let Some(b) = button_titled(&views, "Save") {
                            say(&format!("      Save {}", state_of(&b)));
                            check("adding a row enables Save", b.isEnabled(), &[]);
                            click_centre(&b, &w, mtm);
                        }
                    }
                    11 => {
                        let saw = drain();
                        check(
                            "clicking Save raises on_apply",
                            saw.iter().any(|s| s == "apply"),
                            &saw,
                        );
                    }
                    12 => {
                        if let Some(b) = button_titled(&views, "Open config file") {
                            click_centre(&b, &w, mtm);
                        }
                    }
                    13 => {
                        let saw = drain();
                        check(
                            "clicking Open config file raises on_open_file",
                            saw.iter().any(|s| s == "openfile"),
                            &saw,
                        );
                    }
                    _ => {
                        let bad = FAILED.with(|f| *f.borrow());
                        say("");
                        if bad == 0 {
                            say("ALL PASS: every control driven answered its callback.");
                            std::process::exit(0);
                        }
                        say(&format!("{bad} FAILED"));
                        std::process::exit(1);
                    }
                }
            }),
        );

        beckon_macos::hotkey::HotkeyManager::run_forever();
    }
}
