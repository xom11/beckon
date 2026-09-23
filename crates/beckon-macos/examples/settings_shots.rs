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
    use beckon_core::settings::{
        control_state, page_label, Callbacks, Model, Page, PageLabels, Paths, RuntimeStatus,
    };
    use beckon_macos::settings_window as win;
    use objc2::rc::Retained;
    use objc2::runtime::AnyObject;
    use objc2_app_kit::{NSApplication, NSWindow};
    use objc2_foundation::MainThreadMarker;
    use std::cell::RefCell;
    use std::io::Write;
    use std::rc::Rc;

    fn say(l: &str) {
        println!("{l}");
        let _ = std::io::stdout().flush();
    }

    /// This probe's own `Paths::config`, below -- and, since `open()` writes
    /// the file-name half of it into the window's SUBTITLE unchanged, also
    /// how `our_window` picks the real settings window out of every window
    /// `app.windows()` returns. **Not the title**: since a real `NSToolbar`
    /// replaced the tab strip, the title is the open page's own caption and
    /// changes on every door this probe switches to, where the old
    /// `starts_with("beckon")` check assumed a fixed literal that no longer
    /// exists — it would now match nothing, on every window, every tick.
    const CONFIG_LABEL: &str = "apps.toml";

    /// The four doors, in the same order as `names` below, so an index `i`
    /// used for one can always be used for the other.
    const PAGES: [Page; 4] = [Page::Shortcuts, Page::Keyboard, Page::System, Page::About];

    fn our_window(mtm: MainThreadMarker) -> Option<Retained<NSWindow>> {
        NSApplication::sharedApplication(mtm)
            .windows()
            .iter()
            .find(|w| w.subtitle().to_string() == CONFIG_LABEL)
    }

    /// Select a door through the toolbar's own dispatch -- see
    /// `settings_drive.rs`'s `click_toolbar_page`, which this mirrors.
    /// `NSToolbarItem::view()` is `nil` for every item here (AppKit's
    /// automatically-generated kind, per the objc2 binding's own doc), so
    /// there is no view left in `contentView`'s tree to click; the four
    /// doors used to be equal-width segments of one `NSSegmentedControl`
    /// there, which a posted click could aim at directly. `sendAction:to:
    /// from:` is the public API AppKit itself runs for a real click on an
    /// item, so this raises the exact same call with no view needed.
    fn click_toolbar_page(w: &NSWindow, label: &str, mtm: MainThreadMarker) {
        let Some(toolbar) = w.toolbar() else {
            say("      no toolbar on window");
            return;
        };
        let Some(item) = toolbar
            .items()
            .iter()
            .find(|it| it.label().to_string() == label)
        else {
            say(&format!("      no toolbar item labeled {label:?}"));
            return;
        };
        let Some(action) = item.action() else {
            say(&format!("      toolbar item {label:?} has no action"));
            return;
        };
        let target = item.target();
        let app = NSApplication::sharedApplication(mtm);
        let ok = unsafe {
            app.sendAction_to_from(action, target.as_deref(), Some(&*item as &AnyObject))
        };
        say(&format!("      sendAction({label:?}) -> {ok}"));
    }

    /// The caption of whichever door the toolbar currently has selected, by
    /// asking the toolbar for its `selectedItemIdentifier` and reading that
    /// item's own label back -- there is no `page_identifier` reachable from
    /// an example (it is not `pub`), so the label read for the click above
    /// doubles as the read for "which door is actually open".
    fn open_door_label(w: &NSWindow) -> Option<String> {
        let toolbar = w.toolbar()?;
        let sel = toolbar.selectedItemIdentifier()?.to_string();
        toolbar
            .items()
            .iter()
            .find(|it| it.itemIdentifier().to_string() == sel)
            .map(|it| it.label().to_string())
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
            config: CONFIG_LABEL.into(),
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
                    say(&format!("no window with subtitle `{CONFIG_LABEL}`"));
                    std::process::exit(1);
                };
                // Even steps switch, odd steps shoot: the door has to have
                // been laid out before it is photographed, and a switch and a
                // capture in the same turn of the loop photographs the door
                // that was open a moment ago.
                if step.is_multiple_of(2) {
                    let i = step / 2;
                    if i < 4 {
                        click_toolbar_page(&w, page_label(PAGES[i], PageLabels::MAC), mtm);
                    }
                } else {
                    // Raise it only on the SHOOT tick, one full tick after the
                    // switch. `click_toolbar_page` fires `show_page`
                    // synchronously -- there is no posted event to reorder
                    // any more, `sendAction:to:from:` is a direct call -- but
                    // `show_page` still starts an ANIMATED resize
                    // (`setFrame:display:animate:`), and photographing before
                    // it settles captures the window mid-frame. The `!!`
                    // guard below is what would catch either failure mode.
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
                        let want = page_label(PAGES[i], PageLabels::MAC);
                        let open_now = open_door_label(&w);
                        if open_now.as_deref() != Some(want) {
                            say(&format!(
                                "  !! wanted door {i} ({}) but toolbar shows {open_now:?}",
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
