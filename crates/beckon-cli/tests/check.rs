mod common;

use common::beckon;
use std::process::Output;

fn run_check(content: &str) -> Output {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("apps.toml");
    std::fs::write(&path, content).expect("write config");
    beckon()
        .arg("--check")
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

#[test]
fn check_missing_file_exits_nonzero() {
    let out = beckon()
        .args(["--check", "/nonexistent/beckon-test-apps.toml"])
        .output()
        .expect("run beckon");
    assert!(!out.status.success());
}
