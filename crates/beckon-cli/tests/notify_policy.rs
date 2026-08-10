//! What the real binary actually decides to notify about.
//!
//! The unit tests in `notify` cover the policy; these cover the *wiring* —
//! the handful of lines in `fn main` that pick a `Cause`, ask for a verdict
//! and act on it. Nothing else can reach those lines, and an earlier version
//! of this fix could be deleted wholesale with the whole suite still green.
//!
//! Notifications are diverted to a file by `BECKON_NOTIFY_LOG` rather than
//! posted, because no CI runner has a notification daemon and there is no
//! portable way to read one back.

mod common;

use common::beckon_unmuted;
use std::path::Path;

/// Every child gets its own `TMPDIR`, so the repeat-stamps one test writes
/// cannot silence another — or be silenced by a real beckon on this machine.
fn run(dir: &Path, log: &Path, args: &[&str]) -> std::process::Output {
    let mut cmd = beckon_unmuted();
    cmd.args(args)
        .env("BECKON_NOTIFY_LOG", log)
        .env("TMPDIR", dir) // unix
        .env("TEMP", dir) // windows
        .env("TMP", dir);
    cmd.output().expect("run beckon")
}

fn notifications(log: &Path) -> Vec<String> {
    match std::fs::read_to_string(log) {
        Ok(s) => s.lines().map(str::to_owned).collect(),
        Err(_) => Vec::new(),
    }
}

#[test]
fn a_failure_nobody_can_see_is_notified() {
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("notifications.txt");
    let out = run(
        dir.path(),
        &log,
        &["check", "/nonexistent/beckon-test.toml"],
    );

    assert!(!out.status.success());
    assert_eq!(
        notifications(&log).len(),
        1,
        "stderr was captured, so the only way to surface this is a notification"
    );
}

#[test]
fn muting_wins_over_everything() {
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("notifications.txt");
    let mut cmd = beckon_unmuted();
    let out = cmd
        .args(["check", "/nonexistent/beckon-test.toml"])
        .env("BECKON_NOTIFY_LOG", &log)
        .env("BECKON_NO_NOTIFY", "1")
        .env("TMPDIR", dir.path())
        .env("TEMP", dir.path())
        .env("TMP", dir.path())
        .output()
        .expect("run beckon");

    assert!(!out.status.success());
    assert!(
        notifications(&log).is_empty(),
        "BECKON_NO_NOTIFY must reach the decision, not just the poster"
    );
}

/// The storm guard. A supervisor re-runs the identical `serve` command on a
/// timer; before this, each restart posted. Measured on macOS: 1440 a day.
#[test]
fn repeated_serve_startup_failures_notify_once() {
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("notifications.txt");
    let config = "/nonexistent/beckon-test-serve.toml";

    for _ in 0..3 {
        let out = run(dir.path(), &log, &["serve", config]);
        assert!(!out.status.success(), "a missing config must still fail");
    }

    assert_eq!(
        notifications(&log).len(),
        1,
        "three identical supervised restarts, one notification"
    );
}

/// The other half of the same rule: a person running a command that fails is
/// told every time, because they asked every time.
#[test]
fn repeated_human_invocations_notify_every_time() {
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("notifications.txt");

    for _ in 0..3 {
        run(
            dir.path(),
            &log,
            &["check", "/nonexistent/beckon-test.toml"],
        );
    }

    assert_eq!(
        notifications(&log).len(),
        3,
        "`check` is not supervised; throttling it would swallow real answers"
    );
}

/// Two different faults are two pieces of news, even inside the window.
///
/// macOS and Windows only: elsewhere `serve` is unimplemented and both runs
/// fail with the same sentence regardless of which config was asked for, so
/// the throttle collapses them — correctly, and this test would be asserting
/// that two identical messages are distinct.
#[cfg(any(target_os = "macos", target_os = "windows"))]
#[test]
fn distinct_serve_failures_are_not_collapsed() {
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("notifications.txt");

    run(dir.path(), &log, &["serve", "/nonexistent/one.toml"]);
    run(dir.path(), &log, &["serve", "/nonexistent/two.toml"]);

    assert_eq!(
        notifications(&log).len(),
        2,
        "the stamp is keyed by message, and these two messages differ"
    );
}
