mod common;

use common::beckon;

/// Pinned to exit 1, not merely to "non-zero".
///
/// The original assertion was `assert!(!out.status.success())`, which a clap
/// usage error satisfies as readily as a real failure — so when `--serve`
/// became `serve` this test would have stayed green while proving nothing
/// about `serve`. Exit 1 means beckon's own handler ran; exit 2 would mean the
/// subcommand was never recognised.
#[test]
fn serve_missing_file_exits_nonzero_and_does_not_hang() {
    let out = beckon()
        .args(["serve", "/nonexistent/beckon-test-apps.toml"])
        .output()
        .expect("run beckon");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(1), "stderr: {stderr}");
    assert!(
        !stderr.contains("unexpected argument"),
        "`serve` was not recognised as a subcommand: {stderr}",
    );
}

/// Two commands on one line are refused by construction now.
///
/// This used to assert `cannot be used with`, the wording clap produces for a
/// hand-declared `conflicts_with_all`. There is no such declaration any more —
/// clap permits exactly one subcommand, so the second word is simply
/// unexpected. Asserting the usage line as well is what proves the rejection
/// came from inside `serve` rather than from the top level.
#[test]
fn serve_conflicts_with_check() {
    let out = beckon()
        .args(["serve", "/tmp/a.toml", "check", "/tmp/a.toml"])
        .output()
        .expect("run beckon");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(2), "stderr: {stderr}");
    assert!(
        stderr.contains("unexpected argument 'check' found"),
        "stderr: {stderr}",
    );
    assert!(
        stderr.contains("Usage: beckon serve"),
        "the rejection did not come from the serve subcommand: {stderr}",
    );
}
