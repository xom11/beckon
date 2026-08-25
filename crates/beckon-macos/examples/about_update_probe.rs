//! Does the About door's update row draw the right text, in the right
//! colour, and hide the right things -- for every state `UpdateState` can
//! reach? And, since three new rows landed in one `NSStackView` that used to
//! hold none of them: does the door still FIT, and does growing it push
//! anything below out of the window or into overlap with anything else?
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
//! case too. It is ALSO the geometry control: `idle` is the shortest the
//! door gets (all three new rows collapsed), so `dump_geometry`'s `idle`
//! reading is the "before" number the other seven states are measured
//! against -- if two states report the same fitting height, the probe is not
//! seeing the new rows at all and the reading is worthless, not merely
//! optimistic. The geometry WORST case needs the same `~/.cargo/bin/`
//! relocation as the command row's positive case above: `available` under
//! `Channel::Unknown` (a private `CARGO_TARGET_DIR`) hides the command row
//! and reports the same height as every other non-idle state, not the
//! tallest one.

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
                    dump_geometry(name);
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
                dump_geometry("unreadable");
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
                let r = v.frame();
                println!(
                    "{:indent$}[button] {:?} hidden={} own_hidden={} enabled={} action={action} y={:.1} h={:.1}",
                    "",
                    b.title().to_string(),
                    effective,
                    v.isHidden(),
                    b.isEnabled(),
                    r.origin.y,
                    r.size.height,
                    indent = d * 2
                );
            } else if let Some(t) = v.downcast_ref::<NSTextField>() {
                let colour = t.textColor().map(colour_name).unwrap_or("<none>");
                let r = v.frame();
                println!(
                    "{:indent$}[text]   {:?} hidden={} own_hidden={} colour={colour} y={:.1} h={:.1}",
                    "",
                    t.stringValue().to_string(),
                    effective,
                    v.isHidden(),
                    r.origin.y,
                    r.size.height,
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

/// The geometry half of Finding 2's fix round: does the About door's new
/// content still fit, and does adding it push anything below it out of the
/// window or into overlap?
///
/// **`app.windows()` is not one window.** A run under `NSApplication::run()`
/// in this probe carries several -- this host measured four, including two
/// with a content view whose `frame` reports zero height and one direct
/// `NSScrollView` child that is not the shortcuts table's own (that one is
/// `scroll.setDocumentView(Some(&table))`-owned in `settings_window/mod.rs`
/// and is never a ROOT-level child). Blindly taking `windows().first()` reads
/// one of THOSE, and its `fittingSize` is 0 in every state -- which is
/// exactly the "probe is blind, not the window empty" failure `geom_probe.rs`
/// already warns about, just one level up: not a blind WALK this time, a
/// blind WINDOW pick. Found by first dumping `app.windows()` unfiltered
/// (kept as the `root direct subviews` diagnostic below) and comparing
/// against `dump_about`'s already-correct deep walk, which -- being
/// class-blind -- had been reading the right window all along.
///
/// The real settings window is identified the same way `geom_probe.rs`
/// identifies the real shortcuts table among several scroll views: a
/// property only it has. Here that is an `NSSegmentedControl` (the tab
/// strip) as a DIRECT child of the content view -- `> 1` direct subviews
/// would also work but is a coincidence of this window's current child
/// count, while the tab strip is structural.
///
/// **Forces the content size first, on `geom_probe.rs`'s own measured
/// reason**: in the Background namespace the window is never ordered front,
/// so nothing has applied a content size yet and every frame reads 0 --
/// `WINDOW_WIDTH`/`WINDOW_HEIGHT` (640x500, `settings_window/mod.rs`) is the
/// shipped default, forced here so `layoutSubtreeIfNeeded` has a real size to
/// solve against. `fittingSize` is independent of that forced frame -- it is
/// Auto Layout's answer to "how tall do you actually want to be", which is
/// the number that matters here, not the frame it was forced into.
///
/// Bands are root's direct, non-hidden children, sorted top-down (AppKit's
/// origin is bottom-left, so descending `y` reads top-to-bottom) -- the exact
/// technique `geom_probe.rs`'s own `dump_geometry` already uses. A negative
/// gap between two consecutive bands is two bands' frames overlapping, which
/// is what this function exists to catch; a positive gap is normal slack.
#[cfg(target_os = "macos")]
fn dump_geometry(tag: &str) {
    use objc2_app_kit::{NSApplication, NSSegmentedControl};
    use objc2_foundation::{MainThreadMarker, NSSize};

    let mtm = MainThreadMarker::new().unwrap();
    let app = NSApplication::sharedApplication(mtm);
    println!("  app.windows().count() = {}", app.windows().len());
    let mut picked = false;
    for w in app.windows().iter() {
        // 640x500: this window's own `WINDOW_WIDTH`/`WINDOW_HEIGHT`, copied
        // rather than imported -- `settings_window`'s constants are not
        // `pub`, and re-deriving two literals is cheaper than widening their
        // visibility for one probe.
        w.setContentSize(NSSize::new(640.0, 500.0));
        let Some(root) = w.contentView() else {
            continue;
        };
        root.layoutSubtreeIfNeeded();

        // Skip anything that is not THE settings window -- see the doc
        // above. `.any` on direct children only: the tab strip is one level
        // down, never buried, in every build of this window so far.
        let is_settings_window = root
            .subviews()
            .iter()
            .any(|v| v.downcast_ref::<NSSegmentedControl>().is_some());
        if !is_settings_window {
            println!(
                "  (skipping a non-settings window: {} direct subviews, frame h {:.1})",
                root.subviews().len(),
                root.frame().size.height
            );
            continue;
        }
        picked = true;

        let f = root.frame();
        let fit = root.fittingSize();
        println!(
            "GEOM [{tag}] contentView frame h {:.1}  fittingSize h {:.1}  ({:+.1} pt vs the forced 500 frame)",
            f.size.height,
            fit.height,
            fit.height - f.size.height
        );

        println!("  ROOT direct subviews: {}", root.subviews().len());
        for v in root.subviews().iter() {
            let r = v.frame();
            println!(
                "    child hidden={} y {:.1} h {:.1} {:?}",
                v.isHidden(),
                r.origin.y,
                r.size.height,
                v.class()
            );
        }
        let mut bands: Vec<(f64, f64, String)> = root
            .subviews()
            .iter()
            .filter(|v| !v.isHidden())
            .map(|v| {
                let r = v.frame();
                (r.origin.y, r.size.height, format!("{:?}", v.class()))
            })
            .collect();
        bands.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());
        let mut prev_bottom: Option<f64> = None;
        for (y, h, cls) in &bands {
            if let Some(pb) = prev_bottom {
                let gap = pb - (y + h);
                let flag = if gap < -0.5 { "  <<< OVERLAP" } else { "" };
                println!("  gap {gap:>7.1}{flag}");
            }
            println!("  band y {y:>7.1}  h {h:>7.1}  {cls}");
            prev_bottom = Some(*y);
        }
        if let Some(pb) = prev_bottom {
            println!("  gap {pb:>7.1}  (below the last band -- window bottom)");
        }

        // **The band above is not the number that can overflow.** `w::card`
        // pins `inner`'s top AND bottom edges to the box with
        // `constraintEqualToAnchor_constant` -- a REQUIRED (1000) constraint
        // -- so the About card's outer `NSBox` reports the same 416 pt in
        // EVERY state above, content or no content: it is stretched to fill
        // the space between the tab strip and the command bar, not sized to
        // its content. The question Finding 2 actually asks is whether
        // `inner` (the vstack every About row lives in) NEEDS more height
        // than that fixed pin gives it -- `inner.frame()` is the forced
        // number (always the same), `inner.fittingSize()` is what the vstack
        // actually asks for, and if the second exceeds the first, `NSStackView`
        // has to compress something to make its own required constraint hold,
        // which is exactly the "pushed outside / overlapping" failure mode
        // asked about, just quieter than a torn frame.
        //
        // Found by taking the one VISIBLE `NSBox` among root's direct
        // children -- the three hidden ones above are the other doors'
        // cards, and About is the only page open in this probe.
        use objc2_app_kit::NSBox;
        for v in root.subviews().iter() {
            if v.isHidden() {
                continue;
            }
            let Some(b) = v.downcast_ref::<NSBox>() else {
                continue;
            };
            let Some(inner) = b.contentView() else {
                continue;
            };
            let bf = b.frame();
            let inf = inner.frame();
            let fit = inner.fittingSize();
            println!(
                "  ABOUT CARD box h {:.1}  inner FORCED h {:.1}  inner NEEDS (fittingSize) h {:.1}{}",
                bf.size.height,
                inf.size.height,
                fit.height,
                if fit.height > inf.size.height + 0.5 {
                    "  <<< OVERFLOW: content wants more than the card gives it"
                } else {
                    ""
                }
            );
        }

        break; // found it; the rest of `app.windows()` is not it.
    }
    if !picked {
        // Loud, not a silently-empty report: this is the exact failure mode
        // Finding 2 itself was raised over -- a probe that observed nothing
        // and printed success anyway.
        println!(
            "  GEOM [{tag}] NO SETTINGS WINDOW FOUND -- this reading is not evidence of anything."
        );
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
