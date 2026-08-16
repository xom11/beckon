//! Photograph all four doors.
//!
//! ```text
//! cargo run -p beckon-macos --example settings_shots -- <output-dir>
//! ```
//!
//! The Win32 twin has `docs/superpowers/measurements/fd-*.png`, and every
//! large design lesson on that side came out of them: four visual gaps, a
//! third of the window empty on two doors, three labels sharing one control
//! id. None of those is reachable from a unit test or from an assertion —
//! `settings_drive.rs` will happily report `ALL PASS` on a window that is
//! ugly, and did, while a card's content sat off the top of it.
//!
//! So this is the other half of the same job: `settings_drive` asks whether
//! the controls answer, this asks what a person sees.
//!
//! ## `screencapture -l<windowNumber>`, not a region
//!
//! `NSWindow::windowNumber` IS the `CGWindowID`, so the window can be
//! captured by identity rather than by coordinates. That matters twice: a
//! region has to be computed from a frame in AppKit's bottom-left space and
//! flipped, which is the arithmetic that has already gone wrong twice on this
//! branch; and `-R` rejects a rect it does not like by naming neither the
//! offending number nor the bounds.
//!
//! Requires Screen Recording for whatever runs this — Terminal.app, in
//! practice, since the window must be drawn in an Aqua session anyway.

fn main() {
    #[cfg(not(target_os = "macos"))]
    {
        eprintln!("settings_shots is macOS-only");
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
        NSApplication, NSEvent, NSEventModifierFlags, NSEventType, NSSegmentedControl, NSView,
        NSWindow,
    };
    use objc2_foundation::{MainThreadMarker, NSPoint};
    use std::cell::RefCell;
    use std::io::Write;
    use std::rc::Rc;

    fn say(l: &str) {
        println!("{l}");
        let _ = std::io::stdout().flush();
    }

    fn our_window(mtm: MainThreadMarker) -> Option<Retained<NSWindow>> {
        NSApplication::sharedApplication(mtm)
            .windows()
            .iter()
            .find(|w| w.title().to_string().starts_with("beckon"))
    }

    fn walk(v: &NSView, out: &mut Vec<Retained<NSView>>) {
        for sub in v.subviews().iter() {
            out.push(sub.clone());
            walk(&sub, out);
        }
    }

    fn segmented(w: &NSWindow) -> Option<Retained<NSSegmentedControl>> {
        let mut all = Vec::new();
        if let Some(root) = w.contentView() {
            walk(&root, &mut all);
        }
        all.iter()
            .find_map(|v| v.downcast_ref::<NSSegmentedControl>().map(Retained::from))
    }

    fn click_segment(sc: &NSSegmentedControl, i: usize, w: &NSWindow, mtm: MainThreadMarker) {
        let app = NSApplication::sharedApplication(mtm);
        let b = sc.bounds();
        let at = sc.convertPoint_toView(
            NSPoint::new(b.size.width * (i as f64 + 0.5) / 4.0, b.size.height / 2.0),
            None,
        );
        for kind in [NSEventType::LeftMouseDown, NSEventType::LeftMouseUp] {
            if let Some(ev) = NSEvent::mouseEventWithType_location_modifierFlags_timestamp_windowNumber_context_eventNumber_clickCount_pressure(
                kind, at, NSEventModifierFlags::empty(), 0.0, w.windowNumber(), None, 0, 1, 1.0,
            ) {
                app.postEvent_atStart(&ev, false);
            }
        }
    }

    /// One PNG of one window, by `CGWindowID`.
    ///
    /// `-x` silences the shutter, `-o` drops the drop-shadow so the image is
    /// the window and not a soft grey margin around it.
    fn shoot(window_number: isize, path: &str) -> bool {
        let ok = std::process::Command::new("/usr/sbin/screencapture")
            .args(["-x", "-o", &format!("-l{window_number}"), path])
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        let bytes = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
        // A `screencapture` that is refused still exits 0 on some paths, so
        // the file's existence and size is the check, not the status.
        say(&format!(
            "  {} {path} ({bytes} bytes)",
            if ok && bytes > 0 { "OK  " } else { "FAIL" }
        ));
        ok && bytes > 0
    }

    pub fn run() {
        let out_dir = std::env::args().nth(1).unwrap_or_else(|| ".".into());

        let manager = std::process::Command::new("launchctl")
            .arg("managername")
            .output()
            .ok()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .unwrap_or_default();
        say(&format!("bootstrap namespace : {manager}"));
        if manager != "Aqua" {
            say("REFUSING: not an Aqua session; there would be nothing to photograph.");
            std::process::exit(3);
        }

        const SAMPLE: &str = r#"
"ctrl+super+alt+t" = "kitty"
"ctrl+super+alt+c" = "Claude"
"ctrl+super+alt+b" = "Brave"
"ctrl+super+alt+shift+n" = "Notes"

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
                // **All three pushes, exactly as `serve` does them.** The
                // first version of this probe pushed only `apply_state`, so
                // the System and About doors were photographed holding the
                // values they were BUILT with — which is how an empty log row
                // and a 100%-labelled slider at its left stop got into the
                // first set of images and read as window defects.
                win::apply_system_state(false, None);
                win::apply_about_state();
            }
        };
        let cb = Callbacks {
            on_select: Box::new(|_| {}),
            on_mark: Box::new(|_, _| {}),
            on_edit_combo: Box::new(|_| {}),
            on_probe_shortcut: Box::new(|_| {}),
            on_edit_app: Box::new(|_| {}),
            on_filter: Box::new(|_| {}),
            on_add: Box::new(|| {}),
            on_remove: Box::new(|| {}),
            on_apply: Box::new(|| {}),
            on_reload_from_disk: Box::new(|| {}),
            on_keep_mine: Box::new(|| {}),
            on_open_file: Box::new(|| {}),
            on_close_request: Box::new(|| false),
            on_caps: Box::new(|_| {}),
            on_caps_hold: Box::new(|_| {}),
            on_caps_tap: Box::new(|_| {}),
            on_command: Box::new(|_| {}),
            on_catalog: Box::new(|_| {}),
        };
        let paths = Paths {
            config: "apps.toml".into(),
            log: None,
        };
        if let Err(e) = win::open(cb, &paths, Page::Shortcuts) {
            say(&format!("open failed: {e}"));
            std::process::exit(1);
        }
        refresh();

        let names = ["shortcuts", "keyboard", "system", "about"];
        let mut step = 0usize;
        let mut failures = 0u32;
        beckon_macos::hotkey::add_tick(
            0.7,
            Box::new(move || {
                let mtm = MainThreadMarker::new().expect("main thread");
                let Some(w) = our_window(mtm) else {
                    say("no beckon window");
                    std::process::exit(1);
                };
                // Even steps switch, odd steps shoot: the door has to have
                // been laid out before it is photographed, and a switch and a
                // capture in the same turn of the loop photographs the door
                // that was open a moment ago.
                if step.is_multiple_of(2) {
                    let i = step / 2;
                    if i < 4 {
                        if let Some(sc) = segmented(&w) {
                            click_segment(&sc, i, &w, mtm);
                        }
                    }
                } else {
                    // Raise it only on the SHOOT tick. Doing it on every tick
                    // -- including the one that posts the door click -- left
                    // the strip on segment 0 while the probe photographed
                    // what it believed was the Keyboard door: `makeKey` and
                    // `activate` re-order the event queue the click was just
                    // posted into. The `!!` guard is what caught it.
                    w.makeKeyAndOrderFront(None);
                    NSApplication::sharedApplication(mtm).activate();
                    let i = step / 2;
                    if i < 4 {
                        // **Say which door is actually open.** The first run
                        // produced a byte-identical `keyboard` and
                        // `shortcuts`, and nothing in the output said so —
                        // only comparing the two files afterwards did. A
                        // capture that cannot name its own subject is not a
                        // measurement.
                        let open_now = segmented(&w).map(|sc| sc.selectedSegment()).unwrap_or(-1);
                        if open_now != i as isize {
                            say(&format!(
                                "  !! wanted door {i} ({}) but segment {open_now} is lit",
                                names[i]
                            ));
                            failures += 1;
                        }
                        let path = format!("{out_dir}/macos-door-{}.png", names[i]);
                        if !shoot(w.windowNumber(), &path) {
                            failures += 1;
                        }
                    }
                }
                step += 1;
                if step >= 8 {
                    say("");
                    if failures == 0 {
                        say("ALL FOUR DOORS PHOTOGRAPHED");
                        std::process::exit(0);
                    }
                    say(&format!("{failures} capture(s) failed"));
                    std::process::exit(1);
                }
            }),
        );

        beckon_macos::hotkey::HotkeyManager::run_forever();
    }
}
