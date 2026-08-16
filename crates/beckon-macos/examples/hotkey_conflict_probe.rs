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
//! Four cases, and case 2 is the control for all of them:
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
//! 4. **The same free chord, held by ANOTHER PROCESS.**
//!
//! ## Case 4 is the one the conclusion is actually about
//!
//! The probe shipped without it, and the finding it carries —
//! *`RegisterEventHotKey` does not report a chord another application holds,
//! so macOS has no availability probe* — is a statement about **another
//! application**. What was measured was a duplicate inside one process
//! (case 3, which Carbon refuses) and two chords the **system** holds
//! (case 1, which it accepts). Neither is an ordinary Carbon client holding a
//! hotkey, and the system's own chords are not registered through
//! `RegisterEventHotKey` at all — so case 1 accepting them was never evidence
//! about the API's conflict handling between two clients of it.
//!
//! So the probe re-executes **itself** as a `holder`, which registers
//! `Ctrl+Cmd+Opt+F19` and prints `HOLDER READY` before parking on the event
//! loop. The parent then unregisters everything of its own — otherwise a
//! refusal is case 3 again wearing a different label — and attempts the same
//! chord. It kills the holder and attempts once more: that closing attempt
//! **must** be accepted, which is what separates *refused because someone
//! else held it* from *refused for some reason that outlived the holder*.
//!
//! ```text
//! cargo run -p beckon-macos --example hotkey_conflict_probe -- holder   # not run by hand
//! ```
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

    fn managername() -> String {
        std::process::Command::new("launchctl")
            .arg("managername")
            .output()
            .ok()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .unwrap_or_default()
    }

    /// The other half of case 4: a SECOND process that holds the chord.
    ///
    /// It prints `HOLDER READY` only after a successful registration, so the
    /// parent's `while held` attempt cannot be made against a holder that
    /// silently failed — the same rule as every other control in this file.
    /// The watchdog is not tidiness: a holder that outlived a crashed parent
    /// would sit on `Ctrl+Cmd+Opt+F19` for the rest of the login session, and
    /// the next run of this probe would see its own leftovers and call them a
    /// cross-process conflict.
    fn holder() -> ! {
        println!("HOLDER namespace: {}", managername());
        let mut m = match beckon_macos::hotkey::HotkeyManager::install(Box::new(|_| {})) {
            Ok(m) => m,
            Err(e) => {
                println!("HOLDER FAILED: install: {e}");
                std::process::exit(1);
            }
        };
        if let Err(e) = m.register(1, true, true, true, false, key("f19")) {
            println!("HOLDER FAILED: register: {e}");
            std::process::exit(1);
        }
        println!("HOLDER READY");
        let _ = std::io::Write::flush(&mut std::io::stdout());

        let mut n = 0u32;
        beckon_macos::hotkey::add_tick(
            1.0,
            Box::new(move || {
                n += 1;
                if n > 30 {
                    std::process::exit(0);
                }
            }),
        );
        beckon_macos::hotkey::HotkeyManager::run_forever();
    }

    /// Spawn the holder and wait for it to say it has the chord.
    /// `None` means it never got there, and the caller must then report
    /// case 4 as unmeasured rather than as an answer.
    fn spawn_holder() -> Option<std::process::Child> {
        use std::io::BufRead;
        let exe = std::env::current_exe().ok()?;
        let mut child = std::process::Command::new(exe)
            .arg("holder")
            .stdout(std::process::Stdio::piped())
            .spawn()
            .ok()?;
        // Not `?`: dropping a `Child` neither kills nor reaps it, so an early
        // return here would leak a process still holding the chord — which is
        // the one thing this whole case must not do.
        let Some(out) = child.stdout.take() else {
            let _ = child.kill();
            let _ = child.wait();
            return None;
        };
        let mut r = std::io::BufReader::new(out);
        let mut line = String::new();
        loop {
            line.clear();
            // `Ok(0)` is EOF, i.e. the holder exited without ever being
            // ready — which is why this loop cannot spin forever waiting on a
            // process that is already gone.
            match r.read_line(&mut line) {
                Ok(0) | Err(_) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return None;
                }
                Ok(_) => {
                    print!("  [holder] {line}");
                    if line.trim() == "HOLDER READY" {
                        return Some(child);
                    }
                }
            }
        }
    }

    pub fn run() {
        if std::env::args().nth(1).as_deref() == Some("holder") {
            holder();
        }

        let manager = managername();
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
        println!("case 4 -- the SAME chord, held by ANOTHER PROCESS");
        // **This process must hold nothing first.** Cases 2 and 3 left
        // `Ctrl+Cmd+Opt+F19` registered here, and Carbon refuses a duplicate
        // within one process (case 3) -- so without this line a refusal below
        // would be case 3 again under a label that says "another process",
        // which is precisely the confusion this case exists to end.
        m.unregister_all();
        let cross = match spawn_holder() {
            None => {
                println!("  holder never became ready -- case 4 UNMEASURED");
                None
            }
            Some(mut child) => {
                let held = attempt(
                    &mut m,
                    5,
                    "Ctrl+Cmd+Opt+F19 (holder has it)",
                    [true, true, true, false],
                    key("f19"),
                );
                m.unregister_all();
                let _ = child.kill();
                let _ = child.wait();
                // The holder's registration dies with the process, but not
                // necessarily before the next call reaches Carbon.
                std::thread::sleep(std::time::Duration::from_millis(300));
                let freed = attempt(
                    &mut m,
                    6,
                    "Ctrl+Cmd+Opt+F19 (holder gone)",
                    [true, true, true, false],
                    key("f19"),
                );
                m.unregister_all();
                Some((held, freed))
            }
        };

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
        // Case 4 leads, because it is the case the conclusion is about: an
        // ordinary Carbon client holding a chord is what a beckon user has,
        // and the system's own chords are not registered through this API at
        // all.
        match cross {
            None => {
                println!("CROSS-PROCESS: UNMEASURED. Nothing below can stand in for it --");
                println!("a duplicate within one process and a chord the SYSTEM owns are");
                println!("different questions from `another application holds this`.");
            }
            Some((_, false)) => {
                println!("CROSS-PROCESS: INCONCLUSIVE. The chord was still refused after the");
                println!("holder was killed, so the refusal was not about the holder.");
            }
            Some((false, true)) => {
                println!("CROSS-PROCESS: RegisterEventHotKey DOES refuse a chord another");
                println!("process holds, and accepts it again once that process is gone.");
                println!("macOS CAN have an availability probe: register on a fixed id, read");
                println!("the status, unregister on every exit path -- the Windows shape.");
            }
            Some((true, true)) => {
                println!("CROSS-PROCESS: RegisterEventHotKey ACCEPTED a chord another process");
                println!("was holding. So it does not report conflicts between two clients of");
                println!("it either, and `serve.rs`'s AskTheOs arm is right to stay silent:");
                println!("answering `Free` from a successful registration would be a guess");
                println!("dressed as a measurement.");
            }
        }
        println!();
        match (spotlight, mission) {
            (false, _) | (_, false) => {
                println!("SYSTEM-HELD: RegisterEventHotKey refuses at least one system-held");
                println!("chord. Note this is a WEAKER fact than the line above: the system's");
                println!("chords do not go through this API, so it cannot answer for what two");
                println!("clients of it do to each other.");
            }
            (true, true) => {
                println!("SYSTEM-HELD: RegisterEventHotKey ACCEPTED both system-held chords,");
                println!("so a green verdict for `Cmd+Space` proves nothing about Spotlight.");
            }
        }
        m.unregister_all();
    }
}
