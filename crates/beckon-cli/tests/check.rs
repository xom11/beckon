mod common;

use common::beckon;
use std::process::Output;

fn run_check(content: &str) -> Output {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("apps.toml");
    std::fs::write(&path, content).expect("write config");
    beckon()
        .arg("check")
        .arg(&path)
        .output()
        .expect("run beckon")
}

#[test]
fn check_valid_file_exits_zero() {
    let out = run_check("\"ctrl+super+alt+t\" = \"kitty\"\n");
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(String::from_utf8_lossy(&out.stdout).contains("ok: 1 shortcuts"));
}

/// Without `--resolve`, `check` must not consult the machine at all, so a
/// name nothing anywhere could resolve still exits 0. That is what makes the
/// bare verb usable in CI, where none of the apps are installed.
#[test]
fn check_without_resolve_says_nothing_about_whether_the_app_exists() {
    let out = run_check("\"ctrl+super+alt+t\" = \"beckon-selftest-no-such-app\"\n");
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(String::from_utf8_lossy(&out.stdout).contains("ok: 1 shortcuts"));
}

#[test]
fn check_invalid_key_exits_nonzero_with_message() {
    let out = run_check("\"ctrl+banana\" = \"kitty\"\n");
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("unknown key `banana`"));
}

#[test]
fn check_duplicate_exits_nonzero() {
    let out = run_check("\"ctrl+alt+a\" = \"X\"\n\"alt+ctrl+a\" = \"Y\"\n");
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("duplicates"));
}

/// Pinned to exit 1 and to the message, not merely to "non-zero".
///
/// `assert!(!out.status.success())` used to be the whole test, and a clap
/// usage error satisfies that too — so when `--check` became `check` this test
/// stayed green while asserting nothing about `check` at all. Exit 1 is
/// beckon's own handler; exit 2 would mean the subcommand never ran.
#[test]
fn check_missing_file_exits_nonzero() {
    let out = beckon()
        .args(["check", "/nonexistent/beckon-test-apps.toml"])
        .output()
        .expect("run beckon");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(1), "stderr: {stderr}");
    assert!(stderr.contains("cannot read"), "stderr: {stderr}");
}

fn run_check_resolve(content: &str) -> Output {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("apps.toml");
    std::fs::write(&path, content).expect("write config");
    beckon()
        .arg("check")
        .arg(&path)
        .arg("--resolve")
        .output()
        .expect("run beckon")
}

/// End to end, against this machine's real catalog: a name nothing can resolve
/// still exits 1, and says which key is dead.
#[test]
fn resolve_still_fails_on_a_name_this_machine_cannot_find() {
    let out = run_check_resolve("\"ctrl+super+alt+t\" = \"beckon-selftest-no-such-app\"\n");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(1), "stderr: {stderr}");
    assert!(stdout.contains("ctrl+super+alt+t"), "stdout: {stdout}");
}
