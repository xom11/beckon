//! Does `prefs.rs` survive the process it was written in?
//!
//! The System page's opacity slider moved on screen and `NSUserDefaults`
//! showed no domain at all afterwards. Two explanations fit that equally
//! well and they need opposite fixes:
//!
//! - the UI never called `set_opacity` (an AX `setAttributeValue` on an
//!   `NSSlider` may not fire its target/action), or
//! - `prefs.rs` cannot persist, because a CLI binary has no bundle
//!   identifier and `standardUserDefaults` has no domain to write to.
//!
//! This probe removes the UI from the question. Run it twice:
//!
//! ```text
//! prefs_probe write 73     # writes, then reads back IN THIS PROCESS
//! prefs_probe read         # a FRESH process -- the real question
//! ```
//!
//! The second run is the measurement. A value that reads back only in the
//! writing process is a value that is lost at exit, which for a preference
//! is the same as not being stored.

fn main() {
    #[cfg(target_os = "macos")]
    mac::run();
    #[cfg(not(target_os = "macos"))]
    println!("macOS only");
}

#[cfg(target_os = "macos")]
mod mac {
    use beckon_macos::prefs;

    pub fn run() {
        let args: Vec<String> = std::env::args().skip(1).collect();
        let mode = args.first().map(String::as_str).unwrap_or("read");

        println!("=== prefs_probe ===");
        println!("bundle id : {}", bundle_id());

        match mode {
            "write" => {
                let want: u8 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(73);
                println!("writing   : opacity={want} caps_view=true");
                prefs::set_opacity(want);
                prefs::set_caps_view(true);
                println!(
                    "same proc : opacity={} caps_view={}",
                    prefs::opacity(),
                    prefs::caps_view()
                );
                println!();
                println!("now run `prefs_probe read` -- a FRESH process is the question.");
            }
            _ => {
                println!(
                    "this proc : opacity={} caps_view={}",
                    prefs::opacity(),
                    prefs::caps_view()
                );
                println!();
                println!("96 and false are the DEFAULTS, i.e. nothing was stored.");
            }
        }
    }

    /// A CLI binary usually has none, and that is the suspicion under test.
    fn bundle_id() -> String {
        use objc2_foundation::NSBundle;
        let main = NSBundle::mainBundle();
        main.bundleIdentifier()
            .map(|s| s.to_string())
            .unwrap_or_else(|| "(none -- not a bundle)".to_string())
    }
}
