//! How wide is each column of the shortcut list, and how much of it can the
//! reader actually see?
//!
//! ```text
//! cargo run -p beckon-macos --example geom_probe
//! ```
//!
//! It opens the real settings window against a real
//! `beckon_core::settings::Model` — `settings_probe`'s setup, kept so the
//! geometry is the shipped window's and not a replica — then walks to the
//! four-column table and prints, per column, its declared width, the rect
//! AppKit tiles it into, and how much of that rect falls inside the clip view.
//!
//! **The last number is the one worth having.** A column's declared width is
//! not what the reader sees: the table tiles to the sum of the columns plus
//! `intercellSpacing` per column, and when that exceeds the clip view the
//! overflow lands entirely on the LAST column, which no horizontal scroller
//! can reach. So the width a status word has is decided by the three columns
//! before it and by the gutters, and not at all by its own number. Widening it
//! was tried first and changed nothing, twice.
//!
//! **Unlike every other window probe here, this one does NOT need an Aqua
//! session**, and the distinction is the point: Auto Layout is arithmetic and
//! runs anywhere, while DRAWING is what the Background namespace cannot do.
//! `launchctl managername` is printed rather than enforced.
//!
//! Two things it therefore cannot answer, so do not read them off it: whether
//! the text is ELIDED or hard-clipped at that boundary, and whether a scroller
//! is drawn over anything. Both need pixels.
//!
//! The control that these numbers describe the real window: the Shortcut
//! column came out 267 pt wide at the shipped 250 + 17 pt default spacing, and
//! the same column measured 267.5 pt off an Aqua screenshot taken on airm3.
//! Two independent methods, one number.
//!
//! Also note it must FORCE the content size. In the Background namespace the
//! window is never ordered front, so nothing has applied its content size and
//! every frame reads 0 — a blind probe that looks exactly like a broken
//! window.

fn main() {
    let manager = std::process::Command::new("launchctl")
        .arg("managername")
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default();
    println!("bootstrap namespace : {manager}");
    // Deliberately NOT gated on Aqua: this probe reads LAYOUT, not pixels.
    // Auto Layout is arithmetic and runs in the Background namespace; drawing
    // is what does not. The control that the numbers are real is printed at
    // the bottom -- the Shortcut column's drawn width, which was independently
    // measured at 267.5 pt off an Aqua screenshot.

    #[cfg(not(target_os = "macos"))]
    {
        eprintln!("geom_probe is macOS-only");
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
            on_command: Box::new(|c| println!("command {c:?}")),
        };

        let paths = beckon_core::settings::Paths {
            config: "settings_probe (nothing is written)".into(),
            log: None,
        };
        if let Err(e) = win::open(cb, &paths, beckon_core::settings::Page::Shortcuts) {
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
        beckon_macos::hotkey::add_tick(
            0.5,
            Box::new(move || {
                dump_geometry();
                std::process::exit(0);
            }),
        );

        use objc2_app_kit::NSApplication;
        use objc2_foundation::MainThreadMarker;
        let mtm = MainThreadMarker::new().expect("probe runs on the main thread");
        NSApplication::sharedApplication(mtm).run();
    }

    /// Every width that decides whether the last column's word fits.
    ///
    /// **The gate is not decoration.** This body names `objc2_app_kit` and
    /// `objc2_foundation`, which do not exist off macOS, and a nested `fn`
    /// inside `main` is compiled whatever the enclosing block's `cfg` says --
    /// so without it `cargo clippy --target aarch64-pc-windows-msvc
    /// --all-targets` fails to resolve five imports. Same shape as the ungated
    /// `mod` in `beckon-windows` that broke `nix build` from v0.8.0 to v0.9.3.
    #[cfg(target_os = "macos")]
    fn dump_geometry() {
        use objc2::rc::Retained;
        use objc2_app_kit::{NSScrollView, NSTableView, NSView};
        use objc2_foundation::MainThreadMarker;
        let mtm = MainThreadMarker::new().unwrap();
        let app = objc2_app_kit::NSApplication::sharedApplication(mtm);
        let windows = app.windows();
        let mut found = false;
        for w in windows.iter() {
            // In the Background namespace the window is never ordered front,
            // so nothing has forced it to its content size yet and every frame
            // reads 0. Auto Layout still works -- it just needs a size to
            // solve against, which is what `open` would have set on show.
            // `BECKON_PROBE_H` so the FLOOR can be measured too, not just the
            // default size. `MIN_HEIGHT` is `setContentMinSize`, so the height
            // the user can drag down to is a second layout case, and it is the
            // one where a too-tall page clips the command bar and nothing else.
            let h: f64 = std::env::var("BECKON_PROBE_H")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(500.0);
            w.setContentSize(objc2_foundation::NSSize::new(640.0, h));
            let Some(root) = w.contentView() else {
                continue;
            };
            root.layoutSubtreeIfNeeded();
            println!("window contentView width: {}", root.frame().size.width);
            {
                // The VERTICAL question, which is a different one: where does
                // the window's height actually go, and how much of it lands
                // nowhere? `fittingSize` is what the content asks for; the
                // frame is what it got. Bands are printed top-down in screen
                // order, so the gaps between them are readable by subtraction.
                let f = root.frame();
                let fit = root.fittingSize();
                println!(
                    "contentView height {} vs fittingSize {:.1}  -> {:.1} pt unclaimed",
                    f.size.height,
                    fit.height,
                    f.size.height - fit.height
                );
                // Every band's ASK, hidden ones included. Three of the four
                // doors are hidden at any moment and `fittingSize` still
                // answers for them, which is the only way to see which door
                // actually decides `WINDOW_HEIGHT` -- the visible one is not
                // necessarily the tallest.
                for (i, v) in root.subviews().iter().enumerate() {
                    println!(
                        "  ask[{i}] fitting h {:>6.1}  {}  {:?}",
                        v.fittingSize().height,
                        if v.isHidden() { "hidden " } else { "visible" },
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
                // AppKit's origin is bottom-left, so descending y is top-down.
                bands.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());
                let mut prev_bottom: Option<f64> = None;
                for (y, h, cls) in &bands {
                    if let Some(pb) = prev_bottom {
                        let gap = pb - (y + h);
                        if gap.abs() > 0.5 {
                            println!("      gap {gap:>6.1}");
                        }
                    }
                    println!("  band y {y:>6.1}  h {h:>6.1}  {cls}");
                    prev_bottom = Some(*y);
                }
                if let Some(pb) = prev_bottom {
                    println!("      gap {pb:>6.1}  (below the last band)");
                }
            }
            let mut stack: Vec<Retained<NSView>> = vec![root];
            while let Some(v) = stack.pop() {
                // `downcast` consumes the receiver and hands it back in `Err`,
                // so the walk has to take it apart rather than borrow it.
                let sv = match v.downcast::<NSScrollView>() {
                    Ok(sv) => sv,
                    Err(v) => {
                        for s in v.subviews().iter() {
                            stack.push(s);
                        }
                        continue;
                    }
                };
                {
                    let content = sv.contentSize();
                    println!("scrollView frame width : {}", sv.frame().size.width);
                    println!("clipView content width : {}", content.width);
                    if let Some(doc) = sv.documentView() {
                        if let Ok(t) = doc.downcast::<NSTableView>() {
                            // The shortcuts list is the FOUR-column one. Any
                            // other scroll view in the hierarchy is somebody
                            // else's, and reporting its geometry would answer a
                            // question nobody asked -- which is how a probe
                            // produces a confident wrong number.
                            if t.numberOfColumns() != 4 {
                                continue;
                            }
                            t.tile();
                            println!("table frame width      : {}", t.frame().size.width);
                            println!("intercellSpacing.width : {}", t.intercellSpacing().width);
                            let n = t.numberOfColumns();
                            for i in 0..n {
                                let col = t.tableColumns().objectAtIndex(i as usize);
                                let r = t.rectOfColumn(i);
                                let ident = col.identifier();
                                println!(
                                    "  col {} {:>7} declared {:6.1}  drawn x {:6.1} w {:6.1}  visible-in-clip {:6.1}",
                                    i,
                                    ident.to_string(),
                                    col.width(),
                                    r.origin.x,
                                    r.size.width,
                                    (content.width - r.origin.x).max(0.0).min(r.size.width),
                                );
                            }
                            found = true;
                        }
                    }
                }
            }
            if found {
                break;
            }
        }
        if !found {
            println!("NO TABLE FOUND -- the walk is blind, not the window empty");
        }
    }
}
