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

        println!("entering {mode} loop; screenshot the menu bar now");
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
