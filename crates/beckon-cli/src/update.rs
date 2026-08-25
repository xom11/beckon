//! The one place beckon reaches the network, and only when a person presses a
//! button.
//!
//! **No HTTP crate, deliberately.** `beckon-core` depends on `thiserror` and
//! `toml`; this crate adds `anyhow`, `clap`, `fs4` and `notify`. Adding a TLS
//! stack would put sixty crates into a build graph that was broken for a
//! month (v0.8.0 to v0.9.3) by one ungated `mod` -- and a crypto backend
//! needing a cross C toolchain is what stops
//! `cargo clippy --target aarch64-pc-windows-msvc` from resolving on the
//! author's Mac, which is a required gate leg.
//!
//! Shelling out is already the pattern here: `beckon_macos::shell` invokes
//! `/usr/bin/open`.
//!
//! **The 302 is the whole trick.** `github.com/xom11/beckon/releases/latest`
//! answers 302 with the tag in `Location`, so there is no JSON to parse and
//! no `api.github.com` rate limit (60/hour, unauthenticated) to hit.
//! Measured 2026-08-25 on macOS: 196 ms.

use beckon_core::update::{self, CurlOutcome, UpdateState, Version};
use std::process::Command;

const LATEST: &str = "https://github.com/xom11/beckon/releases/latest";

#[cfg(target_os = "windows")]
const NULL_SINK: &str = "NUL";
#[cfg(not(target_os = "windows"))]
const NULL_SINK: &str = "/dev/null";

/// `CREATE_NO_WINDOW`. `beckon-serve.exe` is GUI-subsystem, so without this a
/// console flashes on every check.
#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Where to look for curl, in order.
///
/// macOS: the absolute path only, matching the `/usr/bin/open` convention in
/// `beckon_macos::shell`. `/usr/bin/curl` ships with the OS.
///
/// Windows: the system copy first (predictable), then bare `curl` so a
/// Git-for-Windows or scoop curl on `PATH` still works. **Whether the system
/// copy exists on ARM64 Windows 11 is unmeasured** -- see the a14 probe. If
/// it does not, `fetch` returns `NoClient` and the About page says so, which
/// is designed for and tested.
fn candidates() -> Vec<std::ffi::OsString> {
    #[cfg(target_os = "windows")]
    {
        let root = std::env::var_os("SystemRoot")
            .unwrap_or_else(|| std::ffi::OsString::from(r"C:\Windows"));
        let mut system = std::path::PathBuf::from(root);
        system.push("System32");
        system.push("curl.exe");
        vec![system.into_os_string(), std::ffi::OsString::from("curl")]
    }
    #[cfg(not(target_os = "windows"))]
    {
        vec![std::ffi::OsString::from("/usr/bin/curl")]
    }
}

/// Ask GitHub which release is newest and compare it against `current`.
///
/// Blocks the calling thread for at most three seconds. Every failure mode
/// resolves to an `UpdateState::Failed`; none of them can produce
/// `Up to date` -- see `beckon_core::update::update_row`'s invariant test.
pub fn fetch(current: Version) -> UpdateState {
    for exe in candidates() {
        let mut cmd = Command::new(&exe);
        cmd.args([
            "-sS",
            // HEAD, and no -L: report the redirect instead of following it.
            "-I",
            "--connect-timeout",
            "2",
            "-m",
            "3",
            "-o",
            NULL_SINK,
            "-w",
            "%{redirect_url}",
            LATEST,
        ]);
        // No custom User-Agent, deliberately: curl sends its own, and
        // `beckon/0.10.0` would tell GitHub which build this user runs for no
        // reason the request needs. Proxies come free -- curl honours
        // http_proxy / https_proxy / no_proxy without beckon knowing they
        // exist.
        //
        // Spawned with `Command::new`, never through a shell, so the `%{...}`
        // format string raises no quoting question: no cmd.exe sees it.
        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }
        match cmd.output() {
            // Spawned. Whatever it did is now the answer -- do NOT fall
            // through to the next candidate, or a real network failure would
            // be retried as though the binary were missing.
            Ok(out) => {
                if !out.status.success() {
                    return update::interpret(CurlOutcome::Failed, current);
                }
                let url = String::from_utf8_lossy(&out.stdout);
                return update::interpret(CurlOutcome::Ok(url.trim()), current);
            }
            Err(_) => continue,
        }
    }
    update::interpret(CurlOutcome::NotSpawned, current)
}
