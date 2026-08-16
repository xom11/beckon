//! Stamp the target triple into the binary for the About door's `Build` row.
//!
//! The Windows twin does the same thing (`BECKON_TARGET`), and the reasoning
//! recorded there applies unchanged: **a stamped DATE would be wrong.** What
//! a build script can report is when the script last ran, cargo caches that
//! aggressively, and the version on the row above already answers "how old is
//! this" without being able to drift from the running process. A triple
//! cannot drift — it is a property of the compilation, not of when it
//! happened.
//!
//! This runs on every platform, not just macOS: `beckon-macos` is a workspace
//! member, and CI's `cargo check --workspace --all-targets` compiles it with
//! no `--exclude` on the Linux and macOS legs. `TARGET` is always set for a
//! build script, so there is nothing to guard.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!(
        "cargo:rustc-env=BECKON_TARGET={}",
        std::env::var("TARGET").unwrap_or_else(|_| "unknown".into())
    );
}
