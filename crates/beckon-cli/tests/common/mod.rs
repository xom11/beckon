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
pub fn beckon() -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_beckon"));
    cmd.env("BECKON_NO_NOTIFY", "1");
    cmd
}
