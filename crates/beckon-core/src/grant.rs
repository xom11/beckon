//! What `serve` on macOS does when a missing TCC grant appears while it is
//! running.
//!
//! macOS hands Accessibility to a NEW process at launch, so flipping the
//! switch in System Settings does not make an already-running `serve` start
//! working. Without a response to the transition, a new user allows beckon,
//! nothing happens, and the reasonable reading is that beckon is broken.
//!
//! The answer differs by who started beckon, on the same signal
//! `serve::macos_broken_config` reads:
//!
//! * **launchd** (stderr is not a terminal) -- exit non-zero so
//!   `KeepAlive { SuccessfulExit: false }` brings beckon straight back. Zero
//!   would NOT restart, deliberately: that is what makes the tray's Quit work.
//! * **a person in a terminal** -- say so and keep running. Killing a process
//!   somebody started by hand, to restart something they are not running under
//!   a supervisor, would lose them their beckon entirely.
//!
//! `Nothing` covers both the ordinary case (still missing, or granted all
//! along) and the one that matters for correctness: this must fire on the
//! TRANSITION only, or a beckon whose grant is simply present would exit on
//! every tick.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GrantRecovery {
    Nothing,
    TellAndStay,
    RestartUnderLaunchd,
}

pub fn grant_recovery(
    was_missing: bool,
    granted_now: bool,
    stderr_is_terminal: bool,
) -> GrantRecovery {
    if !(was_missing && granted_now) {
        return GrantRecovery::Nothing;
    }
    if stderr_is_terminal {
        GrantRecovery::TellAndStay
    } else {
        GrantRecovery::RestartUnderLaunchd
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **Only the transition acts.** A beckon whose grant was present all
    /// along must not exit on every tick, and one still waiting must not
    /// either -- the user may not have reached the switch yet.
    #[test]
    fn only_the_moment_the_grant_appears_does_anything() {
        use GrantRecovery::*;
        assert_eq!(
            grant_recovery(false, true, false),
            Nothing,
            "granted all along"
        );
        assert_eq!(grant_recovery(true, false, false), Nothing, "still waiting");
        assert_eq!(grant_recovery(false, false, false), Nothing);
        assert_eq!(grant_recovery(true, true, false), RestartUnderLaunchd);
    }

    /// **Who started beckon decides what happens**, on the same signal
    /// `macos_broken_config` reads. Exiting under a person's own terminal
    /// would take their beckon away with nothing to bring it back.
    #[test]
    fn a_hand_started_beckon_is_told_rather_than_killed() {
        assert_eq!(grant_recovery(true, true, true), GrantRecovery::TellAndStay);
        assert_eq!(
            grant_recovery(true, true, false),
            GrantRecovery::RestartUnderLaunchd
        );
    }
}
