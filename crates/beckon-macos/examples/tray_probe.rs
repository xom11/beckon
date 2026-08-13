//! Does the menu bar item in `src/tray.rs` actually appear, and under which
//! run loop?
//!
//! This is the macOS counterpart of `beckon-windows/examples/*_probe.rs`: it
//! reaches a layer no unit test can, because a status item only exists when
//! there is a window server to put it on.
//!
//! ```text
//! cargo run -p beckon-macos --example tray_probe -- carbon
//! cargo run -p beckon-macos --example tray_probe -- nsapp
//! ```
//!
//! `carbon` is what `hotkey::run_forever` does today; `nsapp` is the swap
//! that would be needed if a status item cannot live under the Carbon loop.
//! Driven by `testing/macos_tray_probe.sh`, which supplies the screenshots
//! and — importantly — the control frame.
//!
//! **Run it from Terminal.app.** Measured on macmini 2026-08-12: an SSH
//! shell is in the "Background" bootstrap namespace, where
//! `statusItemWithLength` hands back a live object with a non-nil button and
//! nothing is drawn, and `screencapture` refuses to run at all. A result
//! from there is a confident false negative. The probe checks for this
//! itself rather than trusting the operator to remember.

fn main() {
    let mode = std::env::args().nth(1).unwrap_or_else(|| "carbon".into());
    if mode != "carbon" && mode != "nsapp" {
        eprintln!("usage: tray_probe [carbon|nsapp]");
        std::process::exit(2);
    }

    // The self-guard. Without it a run from the wrong session looks exactly
    // like the feature being broken.
    let manager = std::process::Command::new("launchctl")
        .arg("managername")
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default();
    println!("bootstrap namespace : {manager}");
    if manager != "Aqua" {
        println!();
        println!("REFUSING: this is not an Aqua session, so nothing can be drawn and");
        println!("a negative result here would prove nothing. Run from Terminal.app on");
        println!("the machine itself -- not over SSH.");
        std::process::exit(3);
    }

    #[cfg(not(target_os = "macos"))]
    {
        eprintln!("tray_probe is macOS-only");
        std::process::exit(2);
    }

    #[cfg(target_os = "macos")]
    {
        use beckon_core::menu::MenuEntry;
        use beckon_macos::{hotkey, tray};

        // Mirrors `serve`: the UIElement transform happens inside
        // `HotkeyManager::install`, and it is a precondition for the status
        // item behaving the way it will in production.
        let mgr = match hotkey::HotkeyManager::install(Box::new(|id| {
            println!("hotkey fired: id={id}");
        })) {
            Ok(m) => m,
            Err(e) => {
                eprintln!("HotkeyManager::install failed: {e}");
                std::process::exit(1);
            }
        };
        // Held so the manager is not dropped out from under the loop.
        std::mem::forget(mgr);

        let build = Box::new(|| {
            vec![
                MenuEntry {
                    id: 1,
                    label: "PROBE - status row".into(),
                    checked: None,
                    enabled: false,
                },
                MenuEntry::separator(),
                MenuEntry::item(2, "PROBE - click me"),
                MenuEntry {
                    id: 3,
                    label: "PROBE - checkable".into(),
                    checked: Some(true),
                    enabled: true,
                },
                MenuEntry::separator(),
                MenuEntry::item(4, "PROBE - quit"),
            ]
        });
        let on_click = Box::new(|id: u32| {
            // Printing is the whole point: it is the only evidence that a
            // click reached Rust rather than merely dismissing the menu.
            println!("menu click: id={id}");
            if id == 4 {
                println!("quitting on request");
                std::process::exit(0);
            }
        });

        match tray::set_menu(build, on_click) {
            Ok(()) => println!("set_menu ok (NOT proof the icon is visible -- see the module doc)"),
            Err(e) => {
                eprintln!("set_menu failed: {e}");
                std::process::exit(1);
            }
        }
        tray::set_status("beckon - tray_probe");

        // Ask the window server directly. A screenshot needs Screen
        // Recording -- a permission about something else, granted per
        // terminal app -- and three probe runs were lost to it. A status
        // item is a real window at a high layer owned by this process, so
        // the question can be asked without any grant at all.
        //
        // The report is deferred to a run-loop tick because the window is
        // not published until the loop has turned once; asking here, before
        // the loop starts, would report an absence that means nothing.
        // THE ENUMERATION CONTROL, and the reason the first run of this
        // probe proved nothing.
        //
        // "0 windows owned by us" only means "the status item is not on
        // screen" if a status item WOULD have been enumerable. That was
        // assumed, not measured -- and the assumption is load-bearing,
        // because `CGWindowListCopyWindowInfo` is restricted without Screen
        // Recording (the first run saw exactly ONE window at menu-bar layers
        // across every process, which is not what a live menu bar looks
        // like). An ordinary window from THIS process is known to be
        // listed -- settings_probe demonstrated that on macmini 2026-08-13.
        // So open one here: if it appears and the status item does not, the
        // negative is about the status item rather than about what this
        // process can see.
        let control_window = {
            use objc2::MainThreadOnly;
            use objc2_app_kit::{NSBackingStoreType, NSWindow, NSWindowStyleMask};
            use objc2_foundation::{MainThreadMarker, NSPoint, NSRect, NSSize, NSString};
            let mtm = MainThreadMarker::new().expect("probe runs on the main thread");
            // SAFETY: a fresh allocation, initialised exactly once.
            let w = unsafe {
                NSWindow::initWithContentRect_styleMask_backing_defer(
                    NSWindow::alloc(mtm),
                    NSRect::new(NSPoint::new(60.0, 60.0), NSSize::new(360.0, 120.0)),
                    NSWindowStyleMask::Titled | NSWindowStyleMask::Closable,
                    NSBackingStoreType::Buffered,
                    false,
                )
            };
            w.setTitle(&NSString::from_str(
                "beckon tray_probe - ENUMERATION CONTROL",
            ));
            w.makeKeyAndOrderFront(None);
            w
        };

        let me = std::process::id() as i32;
        let mut reported = false;
        hotkey::add_tick(
            1.0,
            Box::new(move || {
                // The tick repeats; the report does not need to.
                if reported {
                    return;
                }
                reported = true;
                let all = beckon_macos::window_server_windows();
                let mine: Vec<_> = all.iter().filter(|w| w.pid == me).collect();
                let bar: Vec<_> = all.iter().filter(|w| w.layer >= 20).collect();
                println!("--- window server report (pid {me}) ---");
                println!(
                    "windows visible to the server, all processes : {}",
                    all.len()
                );
                // The CONTROL. Other applications' menu bar extras sit on
                // the same high layers. If this is 0, the enumeration is
                // blind and our own absence below proves nothing; if it is
                // healthy and ours is empty, that is a real negative.
                println!(
                    "  of those, at menu-bar layers (>=20)        : {}",
                    bar.len()
                );
                println!(
                    "windows owned by THIS process                : {}",
                    mine.len()
                );
                for w in &mine {
                    println!("  layer={:<4} {:.0}x{:.0}", w.layer, w.width, w.height);
                }
                let _ = bar;
                let status = mine.iter().any(|w| w.layer >= 20 && w.width > 0.0);
                // The control: an ordinary window from this same process.
                let control = mine.iter().any(|w| w.layer == 0 && w.width > 100.0);
                if status {
                    println!("VERDICT: the status item has a real window on screen.");
                } else if !control {
                    println!("VERDICT: INCONCLUSIVE. Even the plain control window of this");
                    println!("         process is not listed, so this process cannot see its");
                    println!("         own windows and the absence above means nothing.");
                } else {
                    println!("VERDICT: the control window IS listed and no status-item window");
                    println!("         is. That narrows it to two possibilities, and this");
                    println!("         report cannot separate them:");
                    println!("           (a) the item is not on screen, or");
                    println!("           (b) NSStatusItem does not produce a window this API");
                    println!("               enumerates on this macOS version.");
                    println!("         LOOK AT THE MENU BAR -- that settles it, and nothing");
                    println!("         else here can.");
                }
                println!("--- end report ---");
                println!();
                println!(">>> LOOK AT YOUR MENU BAR NOW. Is there an item reading \"beckon\"?");
                println!(">>> A control window is also open; if you can see THAT but not the");
                println!(">>> menu bar item, the menu bar item is genuinely missing.");
            }),
        );

        // Kept alive for the whole run: releasing it would remove the very
        // control this probe depends on.
        std::mem::forget(control_window);

        println!("entering {mode} loop; report follows in ~1s");
        if mode == "nsapp" {
            use objc2_app_kit::NSApplication;
            use objc2_foundation::MainThreadMarker;
            let mtm = MainThreadMarker::new().expect("probe runs on the main thread");
            let app = NSApplication::sharedApplication(mtm);
            app.run();
        } else {
            hotkey::HotkeyManager::run_forever();
        }
    }
}
