//! Does the settings window in `src/settings_window.rs` draw, and do its
//! controls report back?
//!
//! ```text
//! cargo run -p beckon-macos --example settings_probe
//! ```
//!
//! It drives the real window against a real `beckon_core::settings::Model`,
//! with callbacks that print. Nothing is written to disk: `on_apply` prints
//! the TOML it would have rendered, so a Save can be exercised without a
//! config file to lose.
//!
//! **Run it from Terminal.app**, for the reason `tray_probe` states: an SSH
//! shell is in the "Background" namespace where AppKit hands back live
//! objects and draws nothing.
//!
//! What to look for, in order — each is something no unit test can reach:
//!
//! 1. The window appears at all, and the bands are stacked in the documented
//!    order: banner (hidden), Shortcuts head, list, editor, notes, keyboard,
//!    command bar.
//! 2. The list shows four rows with App leading and Shortcut following.
//! 3. Clicking a row prints `select row=N` with the MODEL index — type into
//!    the filter first so the view and model indices differ, which is the
//!    only way to see the mapping is real.
//! 4. Ticking a box prints `mark row=N on=true`, again a model index, and
//!    the tick survives filtering it out of view.
//! 5. Typing a full app name into the App field leaves the whole name in the
//!    model, not its last character. That is the Windows data-loss bug this
//!    window is structurally supposed to be immune to; this is where the
//!    claim is either confirmed or it is not.
//! 6. Changing a modifier check box prints a probe line THEN an edit line,
//!    in that order.

fn main() {
    let manager = std::process::Command::new("launchctl")
        .arg("managername")
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default();
    println!("bootstrap namespace : {manager}");
    if manager != "Aqua" {
        println!();
        println!("REFUSING: not an Aqua session, so nothing can be drawn and a negative");
        println!("result would prove nothing. Run from Terminal.app on the machine.");
        std::process::exit(3);
    }

    #[cfg(not(target_os = "macos"))]
    {
        eprintln!("settings_probe is macOS-only");
        std::process::exit(2);
    }

    #[cfg(target_os = "macos")]
    {
        use beckon_core::settings::{control_state, Callbacks, Model, RuntimeStatus};
        use beckon_macos::settings_window as win;
        use std::cell::RefCell;
        use std::rc::Rc;

        const SAMPLE: &str = r#"
"ctrl+super+alt+t" = "kitty"
"ctrl+super+alt+c" = "Claude"
"ctrl+super+alt+b" = "Brave"
"ctrl+super+alt+f" = "Finder"

[keyboard]
caps = false
caps_tap = "capslock"
caps_hold = "ctrl+super+alt"
"#;

        let model = Rc::new(RefCell::new(
            Model::from_text(SAMPLE).expect("the sample parses"),
        ));

        // Projected fresh after every callback, exactly as `serve` does.
        let refresh = {
            let m = Rc::clone(&model);
            move || {
                let rt = RuntimeStatus {
                    registered: Default::default(),
                    catalog: Some(vec![
                        "Brave Browser".into(),
                        "Claude".into(),
                        "Finder".into(),
                        "Notes".into(),
                        "kitty".into(),
                    ]),
                    paused: false,
                    probe: None,
                };
                let cs = control_state(&m.borrow(), &rt);
                win::apply_state(&cs, false, None);
            }
        };

        macro_rules! edit {
            ($body:expr) => {{
                let m = Rc::clone(&model);
                let r = refresh.clone();
                move |arg| {
                    #[allow(clippy::redundant_closure_call)]
                    ($body)(&mut *m.borrow_mut(), arg);
                    r();
                }
            }};
        }

        let cb = Callbacks {
            on_select: Box::new(edit!(|m: &mut Model, i: usize| {
                println!("select row={i}");
                m.selected = Some(i);
            })),
            on_mark: Box::new({
                let m = Rc::clone(&model);
                let r = refresh.clone();
                move |i: usize, on: bool| {
                    println!("mark row={i} on={on}");
                    if i < m.borrow().rows.len() {
                        m.borrow_mut().set_marked(i, on);
                    }
                    r();
                }
            }),
            on_edit_combo: Box::new(edit!(|m: &mut Model, t: String| {
                println!("edit combo={t:?}");
                if let Some(i) = m.selected {
                    m.set_combo(i, &t);
                }
            })),
            on_probe_shortcut: Box::new(|t: String| println!("probe combo={t:?}")),
            on_edit_app: Box::new(edit!(|m: &mut Model, t: String| {
                println!("edit app={t:?}");
                if let Some(i) = m.selected {
                    m.set_app(i, &t);
                }
            })),
            on_filter: Box::new(edit!(|m: &mut Model, t: String| {
                println!("filter={t:?}");
                m.set_filter(&t);
            })),
            on_add: Box::new({
                let m = Rc::clone(&model);
                let r = refresh.clone();
                move || {
                    println!("add");
                    m.borrow_mut().add_row();
                    r();
                }
            }),
            on_remove: Box::new({
                let m = Rc::clone(&model);
                let r = refresh.clone();
                move || {
                    println!("remove");
                    m.borrow_mut().remove_pressed();
                    r();
                }
            }),
            on_apply: Box::new({
                let m = Rc::clone(&model);
                move || match m.borrow().render() {
                    // Printed, never written: a probe must not be able to
                    // destroy a config file.
                    Ok(t) => println!("--- would write ---\n{t}--- end ---"),
                    Err(e) => println!("render refused: {e}"),
                }
            }),
            on_caps: Box::new(edit!(|m: &mut Model, on: bool| {
                println!("caps={on}");
                m.set_caps(on);
            })),
            on_caps_tap: Box::new(edit!(|m: &mut Model, t| {
                println!("caps_tap={t:?}");
                m.set_caps_tap(t);
            })),
            on_caps_hold: Box::new(edit!(|m: &mut Model, c| {
                println!("caps_hold={c:?}");
                m.set_caps_hold(c);
            })),
            on_open_file: Box::new(|| println!("open file (probe does not)")),
            on_catalog: Box::new(|n: Vec<String>| println!("catalog: {} names", n.len())),
            on_reload_from_disk: Box::new(|| println!("reload from disk (probe does not)")),
            on_keep_mine: Box::new(|| println!("keep mine")),
            on_close_request: Box::new(|| {
                println!("close requested; probe always allows it");
                std::process::exit(0);
            }),
        };

        if let Err(e) = win::open(cb, "settings_probe (nothing is written)") {
            eprintln!("open failed: {e}");
            std::process::exit(1);
        }
        refresh();
        println!("window opened (constructing one proves nothing -- see below)");

        // Same window-server question the tray probe asks, and for the same
        // reason: it needs no Screen Recording grant. A settings window is
        // an ordinary layer-0 window, so this also acts as the control for
        // the tray probe's high-layer query -- if THIS is not listed either,
        // the enumeration is blind rather than the UI missing.
        let me = std::process::id() as i32;
        let mut reported = false;
        beckon_macos::hotkey::add_tick(
            1.0,
            Box::new(move || {
                if reported {
                    return;
                }
                reported = true;
                let all = beckon_macos::window_server_windows();
                let mine: Vec<_> = all.iter().filter(|w| w.pid == me).collect();
                println!("--- window server report (pid {me}) ---");
                println!("windows, all processes       : {}", all.len());
                println!("windows owned by THIS process: {}", mine.len());
                for w in &mine {
                    println!("  layer={:<4} {:.0}x{:.0}", w.layer, w.width, w.height);
                }
                if mine.iter().any(|w| w.width > 200.0 && w.height > 200.0) {
                    println!("VERDICT: the settings window is on screen.");
                } else if all.is_empty() {
                    println!("VERDICT: INCONCLUSIVE -- the server listed no windows at all.");
                } else {
                    println!("VERDICT: NO settings window. Other processes ARE listed, so");
                    println!("         this is a real negative.");
                }
                println!("--- end report ---");
            }),
        );

        use objc2_app_kit::NSApplication;
        use objc2_foundation::MainThreadMarker;
        let mtm = MainThreadMarker::new().expect("probe runs on the main thread");
        NSApplication::sharedApplication(mtm).run();
    }
}
