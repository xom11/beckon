//! Cold-launch policy: launch immediately (`Always`), require a second press
//! within [`CONFIRM_WINDOW_MS`] (`Confirm`), or never launch at all (`Never`).
//!
//! Only step 3 (no running window of the target app, after `NoMatch` has been
//! ruled out) consults this module. Steps 4 and 5 — focus, cycle, toggle-back,
//! hide — always act on the first press regardless of [`LaunchMode`], and any
//! press that acts on another target clears a pending confirmation so "twice"
//! means two *consecutive* presses of the same target rather than two presses
//! separated by ordinary window switching.

use crate::{BeckonAction, Result};
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// How long a first press on a `Confirm` binding stays armed for the second
/// press, in milliseconds.
///
/// Deliberately wider than a mouse double-click (~300-500 ms): when a user
/// genuinely wants to open the app, they press the hotkey once, notice nothing
/// came forward (~400-700 ms), and press it a second time. Because any
/// intervening beckon action on another app clears the armed state immediately,
/// a comfortable 2-second window does not weaken misclick protection.
pub const CONFIRM_WINDOW_MS: u64 = 2_000;

/// Cold-launch policy for a hotkey invocation or a shortcuts-file binding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LaunchMode {
    /// Default: start the app immediately when it is not running.
    #[default]
    Always,
    /// Require two consecutive presses within [`CONFIRM_WINDOW_MS`] before
    /// starting a closed app.
    Confirm,
    /// Never start the app; only focus, cycle, toggle-back, or hide it while
    /// it is already running.
    Never,
}

impl LaunchMode {
    pub fn parse_str(s: &str) -> std::result::Result<Self, String> {
        match s {
            "always" => Ok(Self::Always),
            "confirm" => Ok(Self::Confirm),
            "never" => Ok(Self::Never),
            other => Err(format!(
                "unknown `launch` mode `{other}` \
                 (expected \"always\", \"confirm\", \"never\", true or false)"
            )),
        }
    }

    pub fn from_bool(b: bool) -> Self {
        if b {
            Self::Always
        } else {
            Self::Never
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Always => "always",
            Self::Confirm => "confirm",
            Self::Never => "never",
        }
    }
}

/// One armed `Confirm` press recorded on disk for the next invocation to read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingLaunch {
    pub target: String,
    pub armed_at_ms: u64,
}

/// Pure verdict for a cold-launch attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GateOutcome {
    /// Start the app now (and clear any stored [`PendingLaunch`]).
    Proceed,
    /// Do not start yet; persist the returned [`PendingLaunch`] and return
    /// [`BeckonAction::LaunchPending`].
    Arm,
    /// Do not start at all; clear any stored [`PendingLaunch`] and return
    /// [`BeckonAction::LaunchSkipped`].
    Skip,
}

/// Pure state transition for step 3 (app resolved, no window running).
///
/// Returns `(outcome, next_pending)` where `next_pending = None` means any
/// persisted pending record must be removed, and `Some(p)` means `p` replaces
/// it. Free of I/O and wall-clock reads so every branch is unit-tested on all
/// three CI platforms.
pub fn decide_cold_launch(
    mode: LaunchMode,
    target: &str,
    pending: Option<&PendingLaunch>,
    now_ms: u64,
) -> (GateOutcome, Option<PendingLaunch>) {
    match mode {
        LaunchMode::Always => (GateOutcome::Proceed, None),
        LaunchMode::Never => (GateOutcome::Skip, None),
        LaunchMode::Confirm => {
            let confirmed = pending.is_some_and(|p| {
                p.target == target
                    && now_ms >= p.armed_at_ms
                    && now_ms - p.armed_at_ms <= CONFIRM_WINDOW_MS
            });
            if confirmed {
                (GateOutcome::Proceed, None)
            } else {
                (
                    GateOutcome::Arm,
                    Some(PendingLaunch {
                        target: target.to_string(),
                        armed_at_ms: now_ms,
                    }),
                )
            }
        }
    }
}

/// Run the cold-launch gate around `do_launch` using the runtime state file.
///
/// When `do_launch` is called and returns `Err`, any existing pending record is
/// left untouched rather than cleared ahead of time; when it succeeds, the
/// pending record is cleared so a hurried third press while a slow app is still
/// starting cannot immediately pass a stale confirmation.
pub fn cold_launch(
    mode: LaunchMode,
    canonical_target: &str,
    do_launch: impl FnOnce() -> Result<()>,
) -> Result<BeckonAction> {
    let path = confirm_state_path();
    cold_launch_at(&path, mode, canonical_target, now_epoch_ms(), do_launch)
}

pub(crate) fn cold_launch_at(
    path: &Path,
    mode: LaunchMode,
    canonical_target: &str,
    now_ms: u64,
    do_launch: impl FnOnce() -> Result<()>,
) -> Result<BeckonAction> {
    // Avoid reading the file at all on the 99% `Always` and `Never` paths.
    let current = match mode {
        LaunchMode::Confirm => read_pending_at(path),
        LaunchMode::Always | LaunchMode::Never => None,
    };
    let (outcome, next) = decide_cold_launch(mode, canonical_target, current.as_ref(), now_ms);
    match outcome {
        GateOutcome::Proceed => {
            do_launch()?;
            clear_pending_at(path);
            Ok(BeckonAction::Launched)
        }
        GateOutcome::Arm => {
            if let Some(p) = next {
                write_pending_at(path, &p);
            }
            Ok(BeckonAction::LaunchPending)
        }
        GateOutcome::Skip => {
            clear_pending_at(path);
            Ok(BeckonAction::LaunchSkipped)
        }
    }
}

/// Reconcile pending-launch state with the final `BeckonAction` of a press.
///
/// Any press that focused, cycled, toggled back, or hid a window represents an
/// intervening user action and disarms a prior misclicked `Confirm` press.
/// (`Launched` and `LaunchSkipped` already disarm inside [`cold_launch`];
/// `LaunchPending` is the arm itself and must keep its file.)
pub fn complete_action(action: BeckonAction) -> BeckonAction {
    if matches!(
        action,
        BeckonAction::Focused
            | BeckonAction::Cycled
            | BeckonAction::ToggledBack
            | BeckonAction::Hidden
    ) {
        clear_pending();
    }
    action
}

/// Remove any armed confirmation record (best-effort).
pub fn clear_pending() {
    clear_pending_at(&confirm_state_path());
}

fn now_epoch_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(0)
}

fn confirm_state_path() -> PathBuf {
    confirm_state_path_in(
        std::env::var_os("XDG_RUNTIME_DIR"),
        std::env::temp_dir(),
        std::env::var_os("HOME"),
        cfg!(target_os = "linux"),
    )
}

/// Pure path resolver.
///
/// - An absolute `XDG_RUNTIME_DIR` wins (standard on Linux tmpfs sessions and
///   used by the Linux e2e harness to isolate each test). Set-but-empty or
///   relative values are ignored per the XDG basedir spec so beckon never reads
///   or writes `./beckon-confirm` in the caller's working directory.
/// - On macOS and Windows `std::env::temp_dir()` is already per-user
///   (`$TMPDIR` under `/var/folders/...`, `%TEMP%` under `AppData\Local\Temp`).
/// - On Linux without `XDG_RUNTIME_DIR`, `temp_dir()` is typically `/tmp`
///   (shared across local users with sticky-bit semantics), so `HOME` is hashed
///   into the filename to avoid cross-user permission collisions.
fn confirm_state_path_in(
    xdg_runtime_dir: Option<OsString>,
    temp_dir: PathBuf,
    home: Option<OsString>,
    shared_tmp_fallback: bool,
) -> PathBuf {
    if let Some(raw) = xdg_runtime_dir {
        let dir = PathBuf::from(raw);
        if dir.is_absolute() {
            return dir.join("beckon-confirm");
        }
    }
    if shared_tmp_fallback {
        let tag = home
            .as_deref()
            .map(|h| fnv1a64(h.as_encoded_bytes()))
            .unwrap_or(0);
        return temp_dir.join(format!("beckon-confirm-{tag:016x}"));
    }
    temp_dir.join("beckon-confirm")
}

fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h
}

pub(crate) fn encode_pending(p: &PendingLaunch) -> String {
    format!("{}\t{}\n", p.target, p.armed_at_ms)
}

pub(crate) fn decode_pending(raw: &str) -> Option<PendingLaunch> {
    let line = raw.trim_end_matches(['\r', '\n']);
    let (target, ts) = line.rsplit_once('\t')?;
    if target.is_empty() {
        return None;
    }
    let armed_at_ms = ts.parse::<u64>().ok()?;
    Some(PendingLaunch {
        target: target.to_string(),
        armed_at_ms,
    })
}

pub(crate) fn read_pending_at(path: &Path) -> Option<PendingLaunch> {
    let raw = fs::read_to_string(path).ok()?;
    decode_pending(&raw)
}

pub(crate) fn write_pending_at(path: &Path, pending: &PendingLaunch) {
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let tmp = path.with_extension(format!("tmp.{}", std::process::id()));
    let payload = encode_pending(pending);
    if fs::write(&tmp, payload).is_ok() && fs::rename(&tmp, path).is_err() {
        let _ = fs::remove_file(&tmp);
    }
}

pub(crate) fn clear_pending_at(path: &Path) {
    let _ = fs::remove_file(path);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::BackendError;
    use std::sync::atomic::{AtomicU32, Ordering};

    fn scratch_dir(label: &str) -> PathBuf {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "beckon-launch-test-{label}-{}-{}-{n}",
            std::process::id(),
            now_epoch_ms()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn launch_mode_parses_strings_and_booleans() {
        assert_eq!(LaunchMode::parse_str("always").unwrap(), LaunchMode::Always);
        assert_eq!(
            LaunchMode::parse_str("confirm").unwrap(),
            LaunchMode::Confirm
        );
        assert_eq!(LaunchMode::parse_str("never").unwrap(), LaunchMode::Never);
        assert!(LaunchMode::parse_str("sometimes").is_err());

        assert_eq!(LaunchMode::from_bool(true), LaunchMode::Always);
        assert_eq!(LaunchMode::from_bool(false), LaunchMode::Never);

        for m in [LaunchMode::Always, LaunchMode::Confirm, LaunchMode::Never] {
            assert_eq!(LaunchMode::parse_str(m.as_str()).unwrap(), m);
        }
    }

    #[test]
    fn always_proceeds_immediately_and_clears_any_prior_pending() {
        let prior = PendingLaunch {
            target: "discord".into(),
            armed_at_ms: 1_000,
        };
        let (out, next) = decide_cold_launch(LaunchMode::Always, "kitty", Some(&prior), 1_200);
        assert_eq!(out, GateOutcome::Proceed);
        assert_eq!(next, None);
    }

    #[test]
    fn never_skips_and_clears_any_prior_pending() {
        let prior = PendingLaunch {
            target: "discord".into(),
            armed_at_ms: 1_000,
        };
        let (out, next) = decide_cold_launch(LaunchMode::Never, "steam", Some(&prior), 1_200);
        assert_eq!(out, GateOutcome::Skip);
        assert_eq!(next, None);
    }

    #[test]
    fn confirm_arms_on_first_press_and_proceeds_on_second_within_window() {
        let (out1, next1) = decide_cold_launch(LaunchMode::Confirm, "discord", None, 10_000);
        assert_eq!(out1, GateOutcome::Arm);
        let armed = next1.expect("first press must arm");
        assert_eq!(armed.target, "discord");
        assert_eq!(armed.armed_at_ms, 10_000);

        // Exactly at the window boundary still counts.
        let (out2, next2) = decide_cold_launch(
            LaunchMode::Confirm,
            "discord",
            Some(&armed),
            10_000 + CONFIRM_WINDOW_MS,
        );
        assert_eq!(out2, GateOutcome::Proceed);
        assert_eq!(next2, None, "confirming consumes the armed token");
    }

    #[test]
    fn confirm_past_the_window_rearms_instead_of_launching() {
        let armed = PendingLaunch {
            target: "discord".into(),
            armed_at_ms: 10_000,
        };
        let late = 10_000 + CONFIRM_WINDOW_MS + 1;
        let (out, next) = decide_cold_launch(LaunchMode::Confirm, "discord", Some(&armed), late);
        assert_eq!(out, GateOutcome::Arm);
        let rearmed = next.expect("expired press starts a fresh window");
        assert_eq!(rearmed.armed_at_ms, late);

        // And a prompt follow-up to THAT press launches.
        let (out2, _) =
            decide_cold_launch(LaunchMode::Confirm, "discord", Some(&rearmed), late + 400);
        assert_eq!(out2, GateOutcome::Proceed);
    }

    #[test]
    fn confirm_for_a_different_target_replaces_pending_without_launching_either() {
        let armed_discord = PendingLaunch {
            target: "discord".into(),
            armed_at_ms: 10_000,
        };
        let (out, next) =
            decide_cold_launch(LaunchMode::Confirm, "steam", Some(&armed_discord), 10_300);
        assert_eq!(out, GateOutcome::Arm);
        assert_eq!(
            next,
            Some(PendingLaunch {
                target: "steam".into(),
                armed_at_ms: 10_300,
            })
        );
    }

    #[test]
    fn clock_skew_backwards_rearms_cleanly_without_underflow() {
        let future = PendingLaunch {
            target: "discord".into(),
            armed_at_ms: 50_000,
        };
        let (out, next) = decide_cold_launch(LaunchMode::Confirm, "discord", Some(&future), 40_000);
        assert_eq!(out, GateOutcome::Arm);
        assert_eq!(next.unwrap().armed_at_ms, 40_000);
    }

    #[test]
    fn encode_decode_preserves_targets_with_spaces_and_windows_paths() {
        for id in [
            "com.hnc.Discord",
            r"c:\program files\steam\steam.exe",
            "app with\ttab-in-name",
        ] {
            let p = PendingLaunch {
                target: id.to_string(),
                armed_at_ms: 123_456_789,
            };
            assert_eq!(decode_pending(&encode_pending(&p)), Some(p));
        }
        assert_eq!(decode_pending(""), None);
        assert_eq!(decode_pending("\t123"), None);
        assert_eq!(decode_pending("discord\tnotanumber"), None);
    }

    #[test]
    fn cold_launch_at_roundtrips_through_filesystem_and_guards_third_press() {
        let dir = scratch_dir("rt");
        let path = dir.join("beckon-confirm");
        let mut launches = 0usize;

        // 1st press: arms, does not launch.
        let a1 = cold_launch_at(&path, LaunchMode::Confirm, "discord", 1_000, || {
            launches += 1;
            Ok(())
        })
        .unwrap();
        assert_eq!(a1, BeckonAction::LaunchPending);
        assert_eq!(launches, 0);
        assert!(path.exists());

        // 2nd press within window: launches and removes the token.
        let a2 = cold_launch_at(&path, LaunchMode::Confirm, "discord", 1_600, || {
            launches += 1;
            Ok(())
        })
        .unwrap();
        assert_eq!(a2, BeckonAction::Launched);
        assert_eq!(launches, 1);
        assert!(!path.exists(), "token must be consumed on launch");

        // 3rd hurried press before any window appears: rearms instead of
        // spawning a second instance.
        let a3 = cold_launch_at(&path, LaunchMode::Confirm, "discord", 1_900, || {
            launches += 1;
            Ok(())
        })
        .unwrap();
        assert_eq!(a3, BeckonAction::LaunchPending);
        assert_eq!(launches, 1);
    }

    #[test]
    fn failed_launch_keeps_error_and_never_calls_do_launch_when_skipped() {
        let dir = scratch_dir("err");
        let path = dir.join("beckon-confirm");

        let skipped = cold_launch_at(&path, LaunchMode::Never, "steam", 1_000, || {
            panic!("Never mode must not call do_launch");
        })
        .unwrap();
        assert_eq!(skipped, BeckonAction::LaunchSkipped);

        write_pending_at(
            &path,
            &PendingLaunch {
                target: "discord".into(),
                armed_at_ms: 1_000,
            },
        );
        let err = cold_launch_at(&path, LaunchMode::Confirm, "discord", 1_200, || {
            Err(BackendError::LaunchFailed {
                id: "discord".into(),
                reason: "boom".into(),
            })
        })
        .unwrap_err();
        assert!(matches!(err, BackendError::LaunchFailed { .. }));
    }

    #[test]
    fn relative_xdg_runtime_dir_is_ignored() {
        let tmp = PathBuf::from("/tmp");
        assert_eq!(
            confirm_state_path_in(
                Some(OsString::from("relative/dir")),
                tmp.clone(),
                None,
                false,
            ),
            PathBuf::from("/tmp/beckon-confirm")
        );
        assert_eq!(
            confirm_state_path_in(
                Some(OsString::from("/run/user/1000")),
                tmp.clone(),
                None,
                true,
            ),
            PathBuf::from("/run/user/1000/beckon-confirm")
        );
        let a = confirm_state_path_in(None, tmp.clone(), Some(OsString::from("/home/alice")), true);
        let b = confirm_state_path_in(None, tmp, Some(OsString::from("/home/bob")), true);
        assert_ne!(a, b, "shared /tmp fallback must isolate users");
    }

    #[test]
    fn acted_excludes_non_acting_launch_outcomes() {
        assert!(BeckonAction::Launched.acted());
        assert!(BeckonAction::Focused.acted());
        assert!(BeckonAction::Cycled.acted());
        assert!(BeckonAction::ToggledBack.acted());
        assert!(BeckonAction::Hidden.acted());
        assert!(!BeckonAction::LaunchPending.acted());
        assert!(!BeckonAction::LaunchSkipped.acted());
    }
}
