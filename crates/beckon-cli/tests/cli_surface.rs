//! The shape of the command line itself: what beckon accepts, what it
//! refuses, and which refusals must stay refusals.
//!
//! These tests are deliberately about *parsing*, not about what any command
//! goes on to do, so every one of them is platform-independent — a usage
//! error is decided before a backend is ever picked.

mod common;

use common::beckon;
use std::process::Output;

/// A shortcuts file that parses, for the tests that need a verb which does no
/// backend work. `check` is the only command that touches neither a display
/// server nor a window manager, which is what makes it usable as a parsing
/// probe on a headless CI runner.
fn with_valid_config<T>(f: impl FnOnce(&std::path::Path) -> T) -> T {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("apps.toml");
    std::fs::write(&path, "\"ctrl+super+alt+t\" = \"kitty\"\n").expect("write config");
    f(&path)
}

fn stderr_of(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

/// The defect that motivated 0.6.0: an app id combined with a query command
/// used to be accepted, and the id silently discarded.
///
/// Measured on v0.5.4 (`866a0b3`), all four of these exited **0** and printed
/// a table. Mutual exclusion was declared by hand in 33 `conflicts_with_all`
/// entries, and five of the seven commands forgot to name `"id"`; `run()` was
/// an if-ladder that tested the flags before `args.id`. Nothing caught it:
/// exit 0 is invisible to scripts and to CI, which is the whole reason the
/// surface moved to subcommands.
///
/// Do not delete this test. It is the only one that pins the original bug,
/// and the flags it names no longer exist, so it can only ever be satisfied
/// by a hard rejection.
#[test]
fn flag_style_invocation_is_rejected() {
    for argv in [
        vec!["ThisAppDoesNotExist", "-l"],
        vec!["ThisAppDoesNotExist", "-d"],
        vec!["ThisAppDoesNotExist", "-r", "Finder"],
        vec!["ThisAppDoesNotExist", "-s", "Finder"],
        vec!["ThisAppDoesNotExist", "-L"],
    ] {
        let out = beckon().args(&argv).output().expect("run beckon");
        assert_eq!(
            out.status.code(),
            Some(2),
            "`beckon {}` must be a usage error, got {:?}\nstdout: {}\nstderr: {}",
            argv.join(" "),
            out.status.code(),
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr),
        );
        assert!(
            out.stdout.is_empty(),
            "`beckon {}` printed to stdout instead of refusing: {}",
            argv.join(" "),
            String::from_utf8_lossy(&out.stdout),
        );
    }
}

/// The 99% path. Every sway / AutoHotkey / Hammerspoon binding is
/// `beckon <Name>` with no verb, so this is the one thing the migration is
/// not allowed to move.
///
/// A green assertion, not a red one — it passed before the migration and must
/// keep passing. The id is deliberately one no machine has installed, so the
/// interesting part is the *shape* of the failure: exit 1 from beckon's own
/// error handler, never exit 2 from clap. If this ever reports
/// `unexpected argument`, the positional stopped being a positional.
#[test]
fn bare_positional_hot_path_survives() {
    let out = beckon()
        .arg("definitely-not-installed-zzz")
        .output()
        .expect("run beckon");
    let stderr = String::from_utf8_lossy(&out.stderr);

    assert_eq!(
        out.status.code(),
        Some(1),
        "the hot path must fail through beckon's handler (exit 1), not clap (exit 2)\nstderr: {stderr}",
    );
    assert!(
        stderr.starts_with("beckon:"),
        "expected beckon's own error prefix, got: {stderr}",
    );
    assert!(
        !stderr.contains("unexpected argument"),
        "the bare positional was parsed as something else: {stderr}",
    );
}

/// `require_id` must survive the move from an if-ladder to a match.
///
/// clap enforces an operand's *presence*, never its *non-emptiness*, so
/// making `search`/`resolve` take `String` instead of `Option<String>` does
/// not subsume this check. Losing it reinstates the bug CLAUDE.md records
/// under "Resolution priority": an empty id is a substring of every `Name`,
/// so a dotfile running `beckon "$APP"` with `$APP` unset launches whatever
/// sorts first.
#[test]
fn empty_id_is_still_rejected() {
    let out = beckon().arg("").output().expect("run beckon");
    let stderr = String::from_utf8_lossy(&out.stderr);

    assert_eq!(out.status.code(), Some(1), "stderr: {stderr}");
    assert!(
        stderr.contains("empty id: expected an app Name or id"),
        "expected the empty-id refusal, got: {stderr}",
    );
}

/// clap will accept an app id *and* a subcommand in the same invocation and
/// report success. Measured on clap 4.6.1 against a probe of exactly this
/// surface: `beckon Claude list` parses to `id: Some("Claude"),
/// command: Some(List)` and exits 0 — the 0.5.4 defect, respelled.
///
/// The rejection is therefore hand-written, in `Args::parse_checked`. This
/// test is the only thing in the suite that notices if that guard is deleted,
/// so it asserts on the guard's own wording: a bare `exit 2` would also be
/// produced by clap refusing an unknown argument, and would pass vacuously.
#[test]
fn id_and_subcommand_are_mutually_exclusive() {
    for argv in [
        vec!["Claude", "list"],
        vec!["Claude", "doctor"],
        vec!["Claude", "resolve", "Finder"],
        vec!["Claude", "doctor", "-v"],
    ] {
        let out = beckon().args(&argv).output().expect("run beckon");
        let stderr = stderr_of(&out);
        assert_eq!(
            out.status.code(),
            Some(2),
            "`beckon {}` must be refused\nstderr: {stderr}",
            argv.join(" "),
        );
        assert!(
            stderr.contains("cannot be combined with a subcommand"),
            "`beckon {}` was refused, but by clap rather than by parse_checked \
             — the guard may be gone.\nstderr: {stderr}",
            argv.join(" "),
        );
    }
}

/// `arg_required_else_help` fires only on a genuinely empty argv, so
/// `beckon -v` parses clean to `(None, None)` and would otherwise be a silent
/// exit-0 no-op. The `(None, None)` arm of `parse_checked` closes that.
///
/// This changes behaviour: v0.5.4 exited **1** here, through beckon's own
/// handler, and posted a desktop notification. "No command given" is a usage
/// error, so it now exits 2 from clap — which also means no notification,
/// because clap exits before `main`'s handler runs.
#[test]
fn no_id_and_no_subcommand_is_rejected() {
    let out = beckon().arg("-v").output().expect("run beckon");
    let stderr = stderr_of(&out);
    assert_eq!(out.status.code(), Some(2), "stderr: {stderr}");
    assert!(
        stderr.contains("an app id or a subcommand is required"),
        "expected the parse_checked refusal, got: {stderr}",
    );

    // Empty argv is clap's own job, and it writes the help to stderr.
    let bare = beckon().output().expect("run beckon");
    assert_eq!(bare.status.code(), Some(2));
    assert!(bare.stdout.is_empty(), "help must not go to stdout");
    assert!(stderr_of(&bare).contains("Usage:"));
}

/// The tripwire for `args_conflicts_with_subcommands`.
///
/// That attribute is the obvious one-line fix for the conflict this file's
/// previous test guards by hand, and it is wrong. clap stops looking for
/// subcommands once any argument has been parsed
/// (`clap_builder/src/parser/parser.rs:592`), so with it set, `beckon -v list`
/// silently binds `"list"` to the id positional and exits 0.
///
/// That is not academic: `testing/linux_live_test.py:509` runs every
/// focus-algorithm case through `run([self.beckon, "-v", *args])`. Setting the
/// attribute fails eight live tests at once, and they look like a backend
/// regression rather than a parser one.
///
/// `check` is the probe verb because it needs no display server, so the
/// assertions hold on a headless runner.
#[test]
fn global_verbose_parses_in_every_position() {
    with_valid_config(|cfg| {
        for argv in [
            vec!["-v", "check", cfg.to_str().unwrap()],
            vec!["check", cfg.to_str().unwrap(), "-v"],
        ] {
            let out = beckon().args(&argv).output().expect("run beckon");
            assert!(
                out.status.success(),
                "`beckon {}` must parse and run\nstderr: {}",
                argv.join(" "),
                stderr_of(&out),
            );
        }
    });

    // And on the positional side, where `-v` must not turn the id into
    // something else. Exit 1 (beckon's handler), never 2 (clap).
    for argv in [
        vec!["-v", "definitely-not-installed-zzz"],
        vec!["definitely-not-installed-zzz", "-v"],
    ] {
        let out = beckon().args(&argv).output().expect("run beckon");
        let stderr = stderr_of(&out);
        assert_eq!(
            out.status.code(),
            Some(1),
            "`beckon {}` must reach the id path\nstderr: {stderr}",
            argv.join(" "),
        );
        assert!(!stderr.contains("unexpected argument"), "{stderr}");
    }
}

/// `--` is the whole escape story, which is why there is no `run` subcommand.
///
/// It covers both cases: an app whose Name equals one of the eight reserved
/// verbs, and an id that starts with a dash. A `run <id>` verb was considered
/// and dropped because it handles only the first — measured, `run -weird.id`
/// is itself a usage error and needs `run -- -weird.id` anyway — while costing
/// a ninth reserved name.
///
/// A pin, not a red test: `--` behaved this way before the migration too. What
/// is new is that the words on the right-hand side now mean something.
#[test]
fn dash_dash_escapes_reserved_names_and_leading_dashes() {
    for argv in [
        vec!["--", "list"],
        vec!["--", "doctor"],
        vec!["--", "help"],
        vec!["--", "-weird.id"],
        vec!["-v", "--", "list"],
    ] {
        let out = beckon().args(&argv).output().expect("run beckon");
        let stderr = stderr_of(&out);
        assert_ne!(
            out.status.code(),
            Some(2),
            "`beckon {}` must reach the id path, not clap's error\nstderr: {stderr}",
            argv.join(" "),
        );
        assert!(
            out.stdout.is_empty(),
            "`beckon {}` ran a command instead of treating it as an id: {}",
            argv.join(" "),
            String::from_utf8_lossy(&out.stdout),
        );
    }
}
