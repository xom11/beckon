//! `beckon resolve` and the `||` chain.
//!
//! A chain is the syntax a shortcuts TOML is written in, and `resolve` is the
//! command a person runs to find out why a line does not work. Until this
//! test existed, `resolve` was the one caller that did not split it: it took
//! `"A || B"` as a single literal id, found nothing named that, and answered
//! `no match for \`A || B\`` — pointing at the whole line while the actual
//! answer was about one candidate inside it.
//!
//! Every test here uses ids that cannot exist on any machine, so the
//! assertions are about the SHAPE of the report and hold on all three CI
//! jobs. What a real name resolves to is a property of the machine and is
//! deliberately not asserted.

mod common;

use common::beckon;

/// Ids no catalog can contain, on any OS.
const A: &str = "NoSuchAppAlphaXyzzy";
const B: &str = "NoSuchAppBetaXyzzy";

fn resolve_stdout(id: &str) -> String {
    let out = beckon()
        .args(["resolve", id])
        .output()
        .expect("run beckon resolve");
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// The defect. `resolve` must not look for an app literally named
/// `"A || B"` — nothing can be, so the answer was guaranteed useless.
#[test]
fn resolve_does_not_treat_a_whole_chain_as_one_id() {
    let chain = format!("{A} || {B}");
    let stdout = resolve_stdout(&chain);

    assert!(
        !stdout.contains(&chain),
        "`resolve` still echoes the whole chain as one id -- it did not split \
         it.\nstdout:\n{stdout}"
    );
}

/// Splitting is not enough: a chain is a disjunction, and the reason a line
/// fails is usually about ONE candidate. Both must be reported, or the user
/// still cannot tell which half is wrong.
///
/// Asserting only that both names appear would pass **vacuously** on the
/// unfixed binary, because echoing the chain back as one id contains both by
/// construction -- that is exactly what this test did on its first run. It
/// therefore asserts the per-candidate heading instead, which only a real
/// split can produce.
#[test]
fn every_candidate_in_a_chain_gets_its_own_report() {
    let stdout = resolve_stdout(&format!("{A} || {B}"));

    assert!(
        stdout.contains("candidate 1 of 2"),
        "the first candidate has no report of its own\nstdout:\n{stdout}"
    );
    assert!(
        stdout.contains("candidate 2 of 2"),
        "the second candidate has no report of its own\nstdout:\n{stdout}"
    );
    assert!(
        stdout.contains(A) && stdout.contains(B),
        "both candidates must still be named\nstdout:\n{stdout}"
    );
}

/// The overwhelmingly common case is one candidate, and it must look exactly
/// as it always did. A person running `beckon resolve Finder` is not asking
/// about chains and must not be shown chain scaffolding.
#[test]
fn a_single_id_is_reported_exactly_as_before() {
    let stdout = resolve_stdout(A);

    assert!(
        !stdout.to_lowercase().contains("candidate"),
        "a plain id grew chain scaffolding it never had\nstdout:\n{stdout}"
    );
    assert!(
        stdout.contains(A),
        "the id itself vanished from its own report\nstdout:\n{stdout}"
    );
}

/// `resolve` answers a question; not finding the app is an answer, not a
/// failure of the command. Pinned because the chain work adds a new way for
/// this to regress: a candidate that misses must not abort the ones after it.
#[test]
fn a_chain_that_resolves_to_nothing_still_exits_zero() {
    for id in [A.to_string(), format!("{A} || {B}")] {
        let out = beckon()
            .args(["resolve", &id])
            .output()
            .expect("run beckon resolve");
        assert_eq!(
            out.status.code(),
            Some(0),
            "`beckon resolve {id}` must exit 0 even when nothing matches\n\
             stdout: {}\nstderr: {}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr),
        );
    }
}

/// The empty candidate is refused rather than skipped, exactly as the hot
/// path refuses it -- `candidates::split` is the one place that decides, and
/// `resolve` must not grow a second, softer opinion. A trailing separator is
/// a typo, and silently ignoring it is how `beckon "$APP"` with `$APP` unset
/// used to launch whatever sorted first.
#[test]
fn an_empty_candidate_is_refused_by_resolve_too() {
    let out = beckon()
        .args(["resolve", &format!("{A} || ")])
        .output()
        .expect("run beckon resolve");

    assert_ne!(
        out.status.code(),
        Some(0),
        "a trailing separator must not be accepted\nstdout: {}",
        String::from_utf8_lossy(&out.stdout),
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("empty candidate"),
        "the refusal must name the real problem\nstderr: {stderr}"
    );
}
