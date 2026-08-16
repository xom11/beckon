//! What does the settings window's catalog actually contain?
//!
//! ```text
//! cargo run -p beckon-macos --example catalog_probe -- Finder kitty
//! ```
//!
//! `row_condition` prints `missing` beside any binding whose app the catalog
//! cannot account for, so a name that `beckon resolve` finds and the window
//! flags is one program answering a question two ways. That happened:
//! `Finder` lives in `/System/Library/CoreServices`, which the installed scan
//! does not walk, while `resolve` matched it as a running app.
//!
//! ## The verdict comes from `control_state`, never from a rule written here
//!
//! This probe used to answer with its own `eq_ignore_ascii_case` over
//! `installed_app_names()`, and that was a copy of a rule the window had
//! stopped using. `row_condition` gained a **substring tier** — the same
//! `Certainty::Guess` tier `check --resolve` passes deliberately — and the
//! copy here did not. Measured on airm3 2026-08-17, both rules run over the
//! same 109-name catalog in the same session:
//!
//! ```text
//! old rule (eq_ignore_ascii_case)   Settings   MISSING
//! what the window draws             Settings   present, "Matches \"System
//!                                              Settings\" by substring"
//! ```
//!
//! **The control that exists to catch the window over-claiming was itself
//! over-claiming**, in the same direction, which is the one failure a control
//! must not have.
//!
//! So the probe now builds a one-row `Model` per name and reads
//! `ControlState::items[0].flag` — the literal word the window would draw in
//! that row's App cell. The combo is `ctrl+super+alt+f1`, which is the
//! default `keyboard.caps_hold`, so the row cannot earn `other chord` and
//! push the word this probe is about out of a cell that holds one.
//!
//! Needs no Aqua session and no permission — it reads `NSWorkspace` and the
//! application directories, nothing more.

fn main() {
    #[cfg(not(target_os = "macos"))]
    {
        eprintln!("catalog_probe is macOS-only");
        std::process::exit(2);
    }
    #[cfg(target_os = "macos")]
    mac::run();
}

#[cfg(target_os = "macos")]
mod mac {
    use beckon_core::settings::{control_state, Mark, Model, RuntimeStatus};

    pub fn run() {
        let names = beckon_macos::installed_app_names();
        println!("catalog: {} names", names.len());

        // Everything else left at its default: no registration pass, not
        // paused, no probe verdict. Those decide the words that OUTRANK
        // `missing` in the App cell, and a probe about the catalog must not
        // let one of them answer for it.
        let rt = RuntimeStatus {
            catalog: Some(names),
            ..RuntimeStatus::default()
        };

        let mut missing = 0;
        for want in std::env::args().skip(1) {
            let mut m = Model::from_text("").expect("the empty config parses");
            m.add_row();
            m.set_combo(0, "ctrl+super+alt+f1");
            m.set_app(0, &want);
            let st = control_state(&m, &rt);

            let flag = st.items[0].flag.clone();
            // The substring tier says its piece in a note rather than in the
            // cell, so reading the flag alone would report a `Guess` and an
            // exact hit identically -- and the difference between them is the
            // whole reason this probe stopped comparing for equality.
            let hazard = st
                .detail
                .as_ref()
                .and_then(|d| d.notes.iter().find(|n| n.mark == Mark::Warn))
                .map(|n| n.text.clone());

            match (flag.as_deref(), hazard) {
                (Some("missing"), _) => {
                    println!("  {want:<24} MISSING");
                    missing += 1;
                }
                (Some(other), _) => println!("  {want:<24} present  ({other})"),
                (None, Some(note)) => println!("  {want:<24} present  ({note})"),
                (None, None) => println!("  {want:<24} present"),
            }
        }
        std::process::exit(if missing == 0 { 0 } else { 1 });
    }
}
