mod common;

use common::beckon;

#[test]
fn serve_missing_file_exits_nonzero_and_does_not_hang() {
    let out = beckon()
        .args(["--serve", "/nonexistent/beckon-test-apps.toml"])
        .output()
        .expect("run beckon");
    assert!(!out.status.success());
}

#[test]
fn serve_conflicts_with_check() {
    let out = beckon()
        .args(["--serve", "/tmp/a.toml", "--check", "/tmp/a.toml"])
        .output()
        .expect("run beckon");
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("cannot be used with"));
}
