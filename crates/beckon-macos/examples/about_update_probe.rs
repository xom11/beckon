//! Does the About door's update row draw the right text, in the right
//! colour, and hide the right things -- for every state `UpdateState` can
//! reach?
//!
//! ```text
//! CARGO_TARGET_DIR=/tmp/beckon-about cargo run -p beckon-macos --example about_update_probe
//! ```
//!
//! `set_update_state` is the ONLY writer of `Ui::update` and
//! `apply_about_state` the only reader (see their own docs in
//! `settings_window/mod.rs`), so driving the first and reading the tree back
//! after each call exercises the same path `serve.rs`'s `check_for_updates`
//! and the tray's `Check for updates` row do -- this probe never touches
//! `about.rs`'s private fields, only the public AppKit tree every one of
//! them ends up in.
//!
//! Deliberately NOT gated on Aqua, on `geom_probe`'s own reasoning: every
//! fact this probe reads -- a control's string value, its `isHidden`, its
//! `isEnabled` -- is Auto Layout / AppKit object state, not a drawn pixel.
//! `launchctl managername` is printed rather than enforced, same as there.
//!
//! **One case needs a real filesystem location, not a flag.** `update_row`'s
//! `command` field depends on `detect_channel`, which reads THIS PROCESS's
//! own `current_exe()` -- there is no parameter to fake it. A probe run out
//! of a private `CARGO_TARGET_DIR` is `Channel::Unknown` and never sees the
//! command row populated at all. To see it, copy the built binary into
//! `~/.cargo/bin/` and run it from there:
//!
//! ```text
//! cp /tmp/beckon-about/debug/examples/about_update_probe ~/.cargo/bin/about_update_probe_temp
//! ~/.cargo/bin/about_update_probe_temp
//! rm ~/.cargo/bin/about_update_probe_temp
//! ```
//!
//! **The control for every "hidden" claim below is the `idle` case itself**:
//! it is the one state where `status`, `command` and the releases row are
//! documented to draw NOTHING, so if `idle` ever prints a status line or an
//! unhidden command row, the probe (or the code) is lying about every other
//! case too.

fn main() {
    let manager = std::process::Command::new("launchctl")
        .arg("managername")
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default();
    println!("bootstrap namespace : {manager}");
    println!(
        "current_exe         : {}",
        std::env::current_exe()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|_| "<unknown>".into())
    );

    #[cfg(not(target_os = "macos"))]
    {
        eprintln!("about_update_probe is macOS-only");
        std::process::exit(2);
    }

    #[cfg(target_os = "macos")]
    {
        use beckon_core::settings::{control_state, Callbacks, Model, Page, Paths, RuntimeStatus};
        use beckon_core::update::{CheckError, UpdateState, Verdict, Version};
        use beckon_macos::settings_window as win;
        use std::cell::RefCell;
        use std::rc::Rc;

        const SAMPLE: &str = r#"
"ctrl+super+alt+t" = "kitty"

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
                    catalog: Some(vec!["kitty".into()]),
                    paused: false,
                    probe: None,
                };
                let cs = control_state(&m.borrow(), &rt);
                win::apply_state(&cs, false, None);
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
            on_caps: Box::new(|_| {}),
            on_caps_tap: Box::new(|_| {}),
            on_caps_hold: Box::new(|_| {}),
            on_open_file: Box::new(|| {}),
            on_catalog: Box::new(|_| {}),
            on_reload_from_disk: Box::new(|| {}),
            on_keep_mine: Box::new(|| {}),
            on_close_request: Box::new(|| std::process::exit(0)),
            on_command: Box::new(|c| println!("command {c:?}")),
        };

        let paths = Paths {
            config: "about_update_probe (nothing is written)".into(),
            log: None,
        };
        if let Err(e) = win::open(cb, &paths, Page::About) {
            eprintln!("open failed: {e}");
            std::process::exit(1);
        }
        refresh();
        win::apply_about_state();

        beckon_macos::hotkey::add_tick(
            0.5,
            Box::new(move || {
                let v = |maj, min, pat| Version {
                    major: maj,
                    minor: min,
                    patch: pat,
                };
                let cases: [(&str, UpdateState); 7] = [
                    ("idle", UpdateState::Idle),
                    ("checking", UpdateState::Checking),
                    ("up_to_date", UpdateState::Done(Verdict::UpToDate)),
                    (
                        "available",
                        UpdateState::Done(Verdict::Available(v(0, 99, 0))),
                    ),
                    ("ahead", UpdateState::Done(Verdict::Ahead(v(0, 1, 0)))),
                    ("no_client", UpdateState::Failed(CheckError::NoClient)),
                    ("unreachable", UpdateState::Failed(CheckError::Unreachable)),
                ];
                for (name, state) in cases {
                    win::set_update_state(state);
                    dump_about(name);
                }
                // A second pass, separate from the loop above, for the two
                // states the task brief specifically forces without a
                // network: `Unreachable` (127.0.0.1:1) and `Unreadable`
                // (example.com) are both `CheckError` variants already
                // covered by directly-constructed `UpdateState`s above and
                // in `unreachable`/this one -- this line exists so
                // `Unreadable` is not the one variant this probe forgets.
                win::set_update_state(UpdateState::Failed(CheckError::Unreadable));
                dump_about("unreadable");
                std::process::exit(0);
            }),
        );

        use objc2_app_kit::NSApplication;
        use objc2_foundation::MainThreadMarker;
        let mtm = MainThreadMarker::new().expect("probe runs on the main thread");
        NSApplication::sharedApplication(mtm).run();
    }
}

/// Walk every window's content view and print each button/text field's
/// payload -- title or string value, `hidden`, `enabled` -- indented by
/// depth. Unlike `geom_probe`'s `walk`, this one does NOT skip hidden views:
/// seeing a row PRESENT but `hidden=true` is exactly the fact this probe
/// exists to check.
///
/// **`hidden` printed here is EFFECTIVE, not the view's own flag.**
/// `command_row` and `open_releases_row` are hidden by calling `setHidden`
/// on the `NSStackView` itself -- the same idiom `system.rs`'s `opacity_row`
/// / `log_row` already use -- and AppKit correctly excludes a hidden
/// arranged subview from its parent's layout. But a CHILD inside that row
/// (the text field, the Copy button) keeps its OWN `isHidden() == false`;
/// only the ancestor is flagged. A probe that read `isHidden()` per leaf
/// without threading the ancestor's flag down would print `hidden=false` for
/// a row the window is not drawing at all, which is a probe bug pretending
/// to be a rendering bug -- caught by comparing this reading against a
/// second one (`update_row_hidden`, printed separately) that reads the row
/// container directly.
#[cfg(target_os = "macos")]
fn dump_about(tag: &str) {
    use objc2::rc::Retained;
    use objc2_app_kit::{NSApplication, NSButton, NSTextField, NSView};
    use objc2_foundation::MainThreadMarker;

    println!("=== {tag} ===");
    let mtm = MainThreadMarker::new().unwrap();
    let app = NSApplication::sharedApplication(mtm);
    for w in app.windows().iter() {
        let Some(root) = w.contentView() else {
            continue;
        };
        root.layoutSubtreeIfNeeded();

        fn walk(v: &NSView, d: usize, ancestor_hidden: bool) {
            let effective = ancestor_hidden || v.isHidden();
            if let Some(b) = v.downcast_ref::<NSButton>() {
                let action = b
                    .action()
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| "<none>".into());
                println!(
                    "{:indent$}[button] {:?} hidden={} own_hidden={} enabled={} action={action}",
                    "",
                    b.title().to_string(),
                    effective,
                    v.isHidden(),
                    b.isEnabled(),
                    indent = d * 2
                );
            } else if let Some(t) = v.downcast_ref::<NSTextField>() {
                let colour = t.textColor().map(colour_name).unwrap_or("<none>");
                println!(
                    "{:indent$}[text]   {:?} hidden={} own_hidden={} colour={colour}",
                    "",
                    t.stringValue().to_string(),
                    effective,
                    v.isHidden(),
                    indent = d * 2
                );
            }
            for s in v.subviews().iter() {
                walk(&s, d + 1, effective);
            }
        }
        let root: Retained<NSView> = root;
        walk(&root, 0, false);
    }
}

/// Which of the four semantic colours `about::apply` can pick this one is,
/// by identity comparison against the same class methods it calls. Dynamic
/// system colours resolve per-appearance but compare equal to themselves
/// regardless of which call produced them, so this does not need a live
/// window or a particular `NSAppearance` to be meaningful.
#[cfg(target_os = "macos")]
fn colour_name(c: objc2::rc::Retained<objc2_app_kit::NSColor>) -> &'static str {
    use objc2_app_kit::NSColor;
    if c == NSColor::labelColor() {
        "label"
    } else if c == NSColor::secondaryLabelColor() {
        "secondaryLabel"
    } else if c == NSColor::systemOrangeColor() {
        "systemOrange"
    } else if c == NSColor::systemRedColor() {
        "systemRed"
    } else {
        "other"
    }
}
