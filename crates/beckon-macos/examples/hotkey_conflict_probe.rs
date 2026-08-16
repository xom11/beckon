//! Does `RegisterEventHotKey` refuse a chord somebody else already holds?
//!
//! This is the one unmeasured fact behind the macOS availability probe.
//! `beckon_core::settings::probe_plan` asks the OS **last**, after five
//! questions the OS cannot answer — parse, the F12 guard, the row's own
//! chord, other rows in the file, the row's saved chord — and on Windows the
//! sixth step is `RegisterHotKey`, which reports a conflict plainly. On macOS
//! `serve.rs`'s `ProbePlan::AskTheOs` arm simply returns, with a comment
//! saying so:
//!
//! > Whether `RegisterEventHotKey` even refuses a chord another app holds is
//! > unmeasured [...] assuming the Carbon API behaves the same way is exactly
//! > the kind of claim this repo has had to retract before.
//!
//! That caution is right, and this probe is how it stops being permanent.
//!
//! ```text
//! cargo run -p beckon-macos --example hotkey_conflict_probe
//! ```
//!
//! **Run it from Terminal.app.** Registration is not delivery, but a process
//! with no window-server identity is a different program from `serve`, and
//! `hotkey.rs` already documents that `RegisterEventHotKey` can return
//! success there and never fire. Measuring the refusal in the same kind of
//! session `serve` runs in is the only reading worth having.
//!
//! ## Reading the output
//!
//! Three cases, and the middle one is the control:
//!
//! 1. **A chord the system holds.** `Cmd+Space` is Spotlight and
//!    `Ctrl+Up` is Mission Control on a stock Mac. A non-zero `OSStatus`
//!    here means the API DOES report conflicts, and the availability probe
//!    can be implemented exactly like the Windows one.
//! 2. **A chord nothing plausibly holds** (`Ctrl+Cmd+Opt+F19`). This must
//!    SUCCEED. Without it, a failure in case 1 could just as well mean
//!    "registration never works from here", which is the opposite
//!    conclusion — the blind-detector trap this repository has hit three
//!    times.
//! 3. **The same free chord twice, from this process.** Windows keeps a
//!    duplicate `(hWnd, id)` pair alongside the original and then frees an
//!    unspecified one, which is why the Windows probe registers on one fixed
//!    id and unregisters on every exit path. Whether Carbon does the same is
//!    worth knowing before writing the macOS twin.
//!
//! Whatever the answer, write it into `serve.rs`'s `AskTheOs` arm and into
//! CLAUDE.md. An unmeasured comment that stays unmeasured becomes folklore.

fn main() {
    #[cfg(not(target_os = "macos"))]
    {
        eprintln!("hotkey_conflict_probe is macOS-only");
        std::process::exit(2);
    }
    #[cfg(target_os = "macos")]
    mac::run();
}

#[cfg(target_os = "macos")]
mod mac {
    use beckon_core::shortcuts::{key_table, KeyDef};

    fn key(name: &str) -> &'static KeyDef {
        key_table()
            .iter()
            .find(|k| k.name == name)
            .unwrap_or_else(|| panic!("no key named `{name}` in key_table()"))
    }

    /// One registration attempt, reported whichever way it goes.
    /// `[ctrl, cmd, opt, shift]` rather than four `bool` parameters: with
    /// the manager, the id, the label and the key that is eight arguments,
    /// which clippy refuses at `-D warnings`, and four adjacent bare bools
    /// at a call site are four chances to transpose two of them anyway.
    fn attempt(
        m: &mut beckon_macos::hotkey::HotkeyManager,
        id: u32,
        label: &str,
        mods: [bool; 4],
        k: &'static KeyDef,
    ) -> bool {
        let [ctrl, cmd, opt, shift] = mods;
        match m.register(id, ctrl, cmd, opt, shift, k) {
            Ok(()) => {
                println!("  {label:<34} ACCEPTED");
                true
            }
            Err(e) => {
                println!("  {label:<34} REFUSED  ({e})");
                false
            }
        }
    }

    pub fn run() {
        let manager = std::process::Command::new("launchctl")
            .arg("managername")
            .output()
            .ok()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .unwrap_or_default();
        println!("bootstrap namespace : {manager}");
        if manager != "Aqua" {
            println!();
            println!("REFUSING: not an Aqua session. `hotkey.rs` already records that");
            println!("RegisterEventHotKey can return success in a process with no window-");
            println!("server identity, so a result here would be about the wrong program.");
            std::process::exit(3);
        }

        let mut m = match beckon_macos::hotkey::HotkeyManager::install(Box::new(|_| {})) {
            Ok(m) => m,
            Err(e) => {
                println!("install failed: {e}");
                std::process::exit(1);
            }
        };

        println!();
        println!("case 2 -- CONTROL: a chord nothing plausibly holds");
        let control = attempt(
            &mut m,
            1,
            "Ctrl+Cmd+Opt+F19",
            [true, true, true, false],
            key("f19"),
        );
        if !control {
            println!();
            println!("VERDICT: inconclusive. Registration does not work from here at all,");
            println!("so a refusal below would say nothing about conflicts.");
            std::process::exit(4);
        }

        println!();
        println!("case 3 -- the SAME chord again, from this same process");
        attempt(
            &mut m,
            2,
            "Ctrl+Cmd+Opt+F19 (again)",
            [true, true, true, false],
            key("f19"),
        );

        println!();
        println!("case 1 -- chords the SYSTEM holds on a stock Mac");
        let spotlight = attempt(
            &mut m,
            3,
            "Cmd+Space (Spotlight)",
            [false, true, false, false],
            key("space"),
        );
        let mission = attempt(
            &mut m,
            4,
            "Ctrl+Up (Mission Control)",
            [true, false, false, false],
            key("up"),
        );

        println!();
        println!("--- verdict ---");
        match (spotlight, mission) {
            (false, _) | (_, false) => {
                println!("RegisterEventHotKey DOES refuse at least one system-held chord.");
                println!("The macOS availability probe can be built on it: register on a");
                println!("fixed id, read the status, unregister on every exit path.");
            }
            (true, true) => {
                println!("RegisterEventHotKey ACCEPTED both system-held chords.");
                println!("So it does NOT report conflicts, and `serve.rs`'s AskTheOs arm is");
                println!("right to stay silent: answering `Free` from a successful");
                println!("registration would be a guess dressed as a measurement.");
                println!("Record this and stop re-opening the question.");
            }
        }
        m.unregister_all();
    }
}
