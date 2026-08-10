//! `--log`: the flag a Scheduled Task uses instead of a `cmd.exe` redirect.
//!
//! Windows-only, because the flag is. Every test drives `beckon serve` at a
//! config that cannot be read, which is the only way to make a resident
//! service exit on its own — and, conveniently, produces the error line we
//! then look for in the log.
//!
//! Since 0.6.0 `--log` is an option *of* `serve` rather than a top-level flag
//! with `requires = "serve"`, so it must follow the subcommand.
//!
//! These cannot run on a macOS or Linux host: `cargo test` for a Windows
//! target fails at link. CI's `windows-latest` job is what executes them.

#![cfg(target_os = "windows")]

mod common;

use common::beckon;
use std::path::Path;

/// Run `serve` against a config that does not exist, logging to `log`.
///
/// The config path is derived from `log`'s own temp directory rather than
/// being a shared constant. `serve` takes a lock keyed on the config path,
/// and cargo runs these tests in parallel: a shared path would mean two of
/// them contend, one exits with "already running", and the assertions below
/// still pass — for entirely the wrong reason.
fn serve_failing(log: &Path) -> std::process::Output {
    let config = log
        .parent()
        .expect("log path has a parent")
        .join("no-such-config.toml");
    assert!(!config.exists(), "the config must be unreadable");
    beckon()
        .arg("serve")
        .arg(&config)
        .arg("--log")
        .arg(log)
        .output()
        .expect("run beckon")
}

#[test]
fn log_captures_stderr_that_would_otherwise_be_lost() {
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("serve.log");

    let out = serve_failing(&log);

    assert!(!out.status.success(), "a missing config must exit non-zero");
    let body = std::fs::read_to_string(&log).expect("log file must exist");
    assert!(
        body.contains("beckon:"),
        "the error belongs in the log, not on a console nobody reads: {body:?}"
    );
    assert!(
        out.stderr.is_empty(),
        "stderr was redirected, so the pipe must be empty: {:?}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn log_creates_missing_parent_directories() {
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("does/not/exist/serve.log");

    serve_failing(&log);

    assert!(
        log.exists(),
        "a Scheduled Task cannot run mkdir first, so beckon must"
    );
}

#[test]
fn log_appends_rather_than_truncating() {
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("serve.log");
    std::fs::write(&log, "evidence from the previous run\n").unwrap();

    serve_failing(&log);

    let body = std::fs::read_to_string(&log).unwrap();
    assert!(
        body.contains("evidence from the previous run"),
        "RestartOnFailure must not destroy the log explaining the failure: {body:?}"
    );
    assert!(
        body.contains("beckon:"),
        "and this run must be there too: {body:?}"
    );
}

/// `--log` exists only under `serve`, and now that is structural.
///
/// It used to be a top-level flag carrying `requires = "serve"`, so the old
/// assertion looked for `--serve` in clap's "missing requirement" message.
/// That requirement is gone: the option is scoped to the subcommand, so
/// anywhere else it is simply an unknown argument. Asserting the old wording
/// would fail; asserting only "non-zero" would pass without testing anything.
#[test]
fn log_is_unknown_outside_serve() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = dir.path().join("apps.toml");
    std::fs::write(&cfg, "\"ctrl+super+alt+t\" = \"kitty\"\n").unwrap();
    let log = "C:\\Windows\\Temp\\beckon-test.log";

    let cases: [Vec<String>; 3] = [
        vec!["--log".into(), log.into()],
        vec!["list".into(), "--log".into(), log.into()],
        vec![
            "check".into(),
            cfg.to_string_lossy().into_owned(),
            "--log".into(),
            log.into(),
        ],
    ];

    for argv in cases {
        let out = beckon().args(&argv).output().expect("run beckon");
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert_eq!(
            out.status.code(),
            Some(2),
            "`beckon {}` must be a usage error\nstderr: {stderr}",
            argv.join(" "),
        );
        assert!(
            stderr.contains("'--log'"),
            "clap must name the offending argument\nstderr: {stderr}",
        );
    }
}
