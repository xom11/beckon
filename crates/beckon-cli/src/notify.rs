//! Everything that decides whether a failure reaches the desktop.
//!
//! It lives in one module because the alternative was tried and lost: the
//! decision used to sit inline in `main`, and `serve` — which notifies from
//! its own long-running loop — called the poster directly, bypassing all of
//! it. That is how a reload loop could have posted one notification per
//! second while the supervised-restart case next door was carefully throttled.
//! Every notification in the program goes through `report` or `decide`.

use std::ffi::OsStr;
use std::time::Duration;

/// Silences every notification. Set by the integration tests, and available
/// to anyone scripting beckon who does not want their screen interrupted.
pub const MUTE_ENV: &str = "BECKON_NO_NOTIFY";

/// Test seam: when set to a path, notifications are appended there as lines
/// instead of being posted.
///
/// There is no portable way to read back a desktop notification — macOS,
/// Linux and Windows each answer to a different daemon and CI runners have
/// none of them. Shimming the spawned helper through `PATH` works on macOS
/// and Linux but not on Windows, where `std`'s `Command` resolves only
/// `.exe`. Without this seam the composition below is untestable, and an
/// untested composition is precisely what let the first fix be silently
/// revertible.
pub const LOG_ENV: &str = "BECKON_NOTIFY_LOG";

/// How long an identical throttled message stays "already reported".
///
/// Long enough that a restart loop nags rather than screams, short enough
/// that a fault left unfixed keeps reminding. Measured on macOS: launchd's
/// `ThrottleInterval` of 60 turned one unreadable config into a notification
/// every minute — 1440 a day; the Windows watchdog's five-minute repetition
/// would give 288.
pub const REPEAT_WINDOW: Duration = Duration::from_secs(60 * 60);

/// Who caused this message to exist.
///
/// The question is not how serious the failure is, but whether it can repeat
/// with nobody asking. A human pressing a hotkey that fails should be told
/// every single time — including the fifth time, because they pressed it a
/// fifth time. A file watcher re-reading a config that will not parse, or a
/// supervisor restarting a serve that cannot start, repeats on a timer for as
/// long as the fault lasts; identical messages from those get throttled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cause {
    HumanAction,
    MachineRepeat,
}

/// What the policy decided, and why. The variants exist so tests can assert
/// on the reason rather than on a bare bool.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Show,
    /// stderr is a terminal — the message is already on screen.
    AlreadyOnScreen,
    /// `BECKON_NO_NOTIFY` is set.
    Muted,
    /// A designed outcome wearing an error's clothes (see `is_expected`).
    Expected,
    /// An identical machine-driven message was reported within the window.
    RepeatWithinWindow,
}

/// The whole policy. Free of I/O — `claim_slot` is passed in — so it can be
/// tested without a terminal, a temp directory or a notification daemon.
pub fn decide(
    stderr_is_terminal: bool,
    muted: bool,
    expected: bool,
    cause: Cause,
    claim_slot: impl FnOnce() -> bool,
) -> Verdict {
    if muted {
        return Verdict::Muted;
    }
    if expected {
        return Verdict::Expected;
    }
    // Without a terminal — a hotkey binding, a launchd agent, a scheduled
    // task — stderr goes to a log or to /dev/null and the failure would
    // otherwise be invisible. With one, a notification only duplicates it.
    if stderr_is_terminal {
        return Verdict::AlreadyOnScreen;
    }
    match cause {
        Cause::HumanAction => Verdict::Show,
        Cause::MachineRepeat => {
            if claim_slot() {
                Verdict::Show
            } else {
                Verdict::RepeatWithinWindow
            }
        }
    }
}

/// Apply the policy to `message` and post it if it survives.
///
/// This is the entry point for everything except `main`'s top-level error
/// handler, which needs to pass its own `expected` flag.
///
/// Its only callers live in `serve`, which is compiled on macOS and Windows
/// alone — so everywhere else this is genuinely unused rather than
/// accidentally so.
#[cfg_attr(not(any(target_os = "macos", target_os = "windows")), allow(dead_code))]
pub fn report(message: &str, cause: Cause) {
    report_expected(message, cause, false);
}

/// As `report`, for the one caller that can recognise a designed outcome:
/// `main`, which owns `is_expected`.
pub fn report_expected(message: &str, cause: Cause, expected: bool) {
    let verdict = decide(
        std::io::IsTerminal::is_terminal(&std::io::stderr()),
        muted(),
        expected,
        cause,
        || claim_repeat_slot(message),
    );
    if verdict == Verdict::Show {
        show(message);
    }
}

pub fn muted() -> bool {
    muted_by(std::env::var_os(MUTE_ENV).as_deref())
}

/// Any non-empty value mutes. Empty counts as unset, so an exported but blank
/// variable does not silently disable notifications.
pub fn muted_by(value: Option<&OsStr>) -> bool {
    value.is_some_and(|v| !v.is_empty())
}

/// Claim the right to report `message` now, or decline if an identical one
/// was reported within `REPEAT_WINDOW`.
///
/// The state lives on disk because every restart in a supervised loop is a
/// fresh process — an in-memory guard would reset on exactly the event it
/// exists to count.
///
/// If the temp directory cannot be written, this returns `true` every time
/// and the throttle is simply absent: under a supervisor that means the
/// original once-a-minute storm, not "one duplicate". Nothing announces that,
/// so treat an unwritable temp directory as losing the guard entirely.
pub fn claim_repeat_slot(message: &str) -> bool {
    let stamp = std::env::temp_dir().join(format!(
        "beckon-notify-{}.stamp",
        crate::stable_id::fnv1a64(message.as_bytes())
    ));
    let recent = std::fs::metadata(&stamp)
        .and_then(|m| m.modified())
        .is_ok_and(|t| t.elapsed().is_ok_and(|age| age < REPEAT_WINDOW));
    if recent {
        return false;
    }
    // Rewriting refreshes mtime, which is the clock this reads back.
    let _ = std::fs::write(&stamp, message);
    true
}

/// Escape a string for embedding inside a double-quoted AppleScript literal.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn applescript_escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

/// Hand a message to the platform's notification daemon. Best-effort: silent
/// if the helper is missing or the daemon is unreachable.
///
/// Private on purpose. `report` and `report_expected` are the only ways out
/// of this module, so posting without a verdict is not expressible. A
/// mutation test found that `serve` calling the poster directly was the one
/// regression no test could catch; the compiler catches it instead.
fn post(message: &str) {
    #[cfg(target_os = "linux")]
    {
        let _ = std::process::Command::new("notify-send")
            .args([
                "--app-name=beckon",
                "--urgency=critical",
                "--icon=dialog-error",
                "beckon error",
                message,
            ])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn();
    }
    #[cfg(target_os = "windows")]
    {
        // Best-effort toast notification via PowerShell.
        let _ = std::process::Command::new("powershell")
            .args([
                "-NoProfile",
                "-Command",
                &format!(
                    "[Windows.UI.Notifications.ToastNotificationManager, Windows.UI.Notifications, ContentType = WindowsRuntime] > $null; \
                     $xml = [Windows.UI.Notifications.ToastNotificationManager]::GetTemplateContent(0); \
                     $text = $xml.GetElementsByTagName('text'); \
                     $text.Item(0).AppendChild($xml.CreateTextNode('beckon: {}')) > $null; \
                     [Windows.UI.Notifications.ToastNotificationManager]::CreateToastNotifier('beckon').Show([Windows.UI.Notifications.ToastNotification]::new($xml))",
                    message.replace('\'', "''")
                ),
            ])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn();
    }
    #[cfg(target_os = "macos")]
    {
        let script = format!(
            r#"display notification "{}" with title "beckon""#,
            applescript_escape(message)
        );
        let _ = std::process::Command::new("osascript")
            .args(["-e", &script])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
    }
    #[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
    {
        let _ = message;
    }
}

fn show(message: &str) {
    if log_instead(message) {
        return;
    }
    post(message);
}

/// Returns true when `LOG_ENV` diverted the message.
fn log_instead(message: &str) -> bool {
    let Some(path) = std::env::var_os(LOG_ENV) else {
        return false;
    };
    if path.is_empty() {
        return false;
    }
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    {
        let _ = writeln!(f, "{message}");
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    const NEVER: fn() -> bool = || panic!("claim_slot must not be consulted here");

    #[test]
    fn applescript_escape_quotes_and_backslashes() {
        assert_eq!(
            applescript_escape(r#"say "hi" \ bye"#),
            r#"say \"hi\" \\ bye"#
        );
    }

    #[test]
    fn mute_env_off_when_unset_or_empty() {
        assert!(!muted_by(None));
        assert!(!muted_by(Some(OsStr::new(""))));
    }

    #[test]
    fn mute_env_on_for_any_non_empty_value() {
        for v in ["1", "0", "yes", "false"] {
            assert!(muted_by(Some(OsStr::new(v))), "{v} should mute");
        }
    }

    #[test]
    fn muted_wins_over_everything() {
        assert_eq!(
            decide(false, true, false, Cause::HumanAction, NEVER),
            Verdict::Muted
        );
    }

    #[test]
    fn expected_outcomes_never_show() {
        assert_eq!(
            decide(false, false, true, Cause::HumanAction, NEVER),
            Verdict::Expected
        );
    }

    #[test]
    fn a_terminal_already_showed_it() {
        assert_eq!(
            decide(true, false, false, Cause::HumanAction, NEVER),
            Verdict::AlreadyOnScreen
        );
        assert_eq!(
            decide(true, false, false, Cause::MachineRepeat, NEVER),
            Verdict::AlreadyOnScreen
        );
    }

    #[test]
    fn human_actions_are_never_throttled() {
        // Told a fifth time, because they asked a fifth time.
        for _ in 0..5 {
            assert_eq!(
                decide(false, false, false, Cause::HumanAction, NEVER),
                Verdict::Show
            );
        }
    }

    #[test]
    fn machine_repeats_go_through_the_slot() {
        assert_eq!(
            decide(false, false, false, Cause::MachineRepeat, || true),
            Verdict::Show
        );
        assert_eq!(
            decide(false, false, false, Cause::MachineRepeat, || false),
            Verdict::RepeatWithinWindow
        );
    }

    /// Uniqueness comes from a nanosecond token, not the pid: macOS burns
    /// through its 99999 pids in well under `REPEAT_WINDOW`, so a pid-keyed
    /// message could collide with a stamp left by an earlier test run and
    /// fail the first assertion.
    fn unique_message(tag: &str) -> String {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or_default();
        format!("beckon-selftest-{tag}-{}-{nanos}", std::process::id())
    }

    #[test]
    fn repeat_slot_opens_once_then_closes() {
        let msg = unique_message("repeat");
        assert!(claim_repeat_slot(&msg), "first sighting must report");
        assert!(
            !claim_repeat_slot(&msg),
            "a supervisor restarting every minute must not report again"
        );
    }

    #[test]
    fn repeat_slot_is_per_message() {
        let a = unique_message("a");
        let b = unique_message("b");
        assert!(claim_repeat_slot(&a));
        assert!(
            claim_repeat_slot(&b),
            "a different failure is news even inside the window"
        );
    }
}
