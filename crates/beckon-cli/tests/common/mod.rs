use std::process::Command;

/// Spawn the real `beckon` binary with desktop notifications muted.
///
/// Every integration test here runs a deliberately broken invocation with
/// stderr captured -- which is precisely the condition under which `beckon`
/// decides a human would otherwise never see the failure, and posts a desktop
/// notification. Without the mute, `cargo test` throws four real notifications
/// at whoever ran it; on macOS 2026-08-09 that had displaced the machine's
/// entire retained notification history with beckon's own test fixtures.
///
/// Go through this helper rather than `Command::new` directly, so a test added
/// later inherits the mute instead of quietly reintroducing the noise.
/// `#[allow(dead_code)]` on both helpers: this module is compiled once into
/// every integration test binary, and no single binary uses both.
#[allow(dead_code)]
pub fn beckon() -> Command {
    let mut cmd = beckon_unmuted();
    cmd.env("BECKON_NO_NOTIFY", "1");
    cmd
}

/// The binary with notifications left switched on.
///
/// Only `notify_policy.rs` wants this — it is testing the notification
/// decision itself, and diverts the output with `BECKON_NOTIFY_LOG` so
/// nothing reaches a real desktop. Every other test should use `beckon`.
#[allow(dead_code)] // not every integration test binary uses this
pub fn beckon_unmuted() -> Command {
    Command::new(env!("CARGO_BIN_EXE_beckon"))
}
