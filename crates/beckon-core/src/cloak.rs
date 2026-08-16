//! The Windows enumeration rule for a **cloaked** window: drop it, or rescue
//! it because the only thing wrong with it is that it lives on another
//! virtual desktop.
//!
//! Lives here rather than in `beckon-windows` for the reason `caps::decide`,
//! `settings::row_condition` and `page_plan` do: it is pure integer/enum
//! logic that decides real behaviour, and inside a `cfg(target_os =
//! "windows")` module it would be untestable on two of the three CI jobs and
//! unrunnable on the machine most of this is written on. Nothing here names
//! the `windows` crate.
//!
//! ## Why `DwmGetWindowAttribute(DWMWA_CLOAKED)` alone is not enough
//!
//! `enum_visible_windows` drops every cloaked window, which is right for the
//! ghosts it was added to remove and wrong for a whole virtual desktop: a
//! window sitting on desktop 2 is cloaked, so beckon never sees it, so the
//! hotkey **launches a second copy of an app that is already running**.
//!
//! **The tempting fix is wrong and is the reason this module exists.** The
//! cloak word has three documented bits -- `DWM_CLOAKED_APP` (`0x1`), the app
//! cloaked itself; `DWM_CLOAKED_SHELL` (`0x2`), the shell cloaked it;
//! `DWM_CLOAKED_INHERITED` (`0x4`) -- and reading `0x2` as "on another
//! virtual desktop" looks like it separates the two cases. It does not:
//! **`0x2` is also what a suspended UWP app reports.** Keeping `0x2` would
//! re-admit exactly the ghost windows the filter exists to remove, and beckon
//! would start "focusing" windows that never come forward, reporting success
//! while nothing moves.
//!
//! So the bits are **not consulted at all** here. The discriminator is a
//! different question asked of a different API --
//! `IVirtualDesktopManager::IsWindowOnCurrentVirtualDesktop` -- which answers
//! the thing we actually want to know. A suspended UWP app on the desktop you
//! are looking at is on the *current* desktop, cloaked or not, and so stays
//! out.

/// Which virtual desktop a window is on, as far as
/// `IsWindowOnCurrentVirtualDesktop` was able to say.
///
/// **`Unknown` is a third answer, not a spelling of either other one.** That
/// call returns an `HRESULT` and it really does fail -- for a window that is
/// not top-level, for one caught mid-creation, and for a window the shell has
/// not assigned to a desktop. Folding a failure into `Other` would let every
/// failure re-admit a window, which is the direction that breaks things;
/// folding it into `Current` would claim more than was measured. It gets its
/// own variant so [`admit_window`] can route it deliberately.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Desktop {
    /// The window is on the virtual desktop the user is looking at.
    Current,
    /// The window is on some other virtual desktop.
    Other,
    /// The COM object could not be created, or the call returned an error.
    Unknown,
}

/// Does this window survive enumeration?
///
/// `cloaked` is the raw word `DwmGetWindowAttribute(DWMWA_CLOAKED)` wrote;
/// `locate` asks the virtual-desktop manager where the window is.
///
/// `locate` is a closure rather than a value **so that the cost is provably
/// not paid on the hot path's common case.** `beckon <id>` is budgeted at
/// 50 ms and measured at ~57 ms already; nearly every window on the machine
/// is uncloaked, and for those this returns before `locate` is ever called,
/// so no COM round-trip happens for them at all.
/// `an_uncloaked_window_never_asks_the_virtual_desktop_manager` pins that.
///
/// The rule, in one line: **a cloaked window is admitted only when something
/// positively said it is on another desktop.** `Unknown` therefore keeps it
/// out -- that is today's behaviour, and degrading toward the known state is
/// the safe direction. The failure mode of getting this backwards is not
/// symmetric: a missed rescue means beckon behaves as it does today, while a
/// wrong rescue means beckon "focuses" a ghost and reports success while the
/// screen does not change.
pub fn admit_window(cloaked: u32, locate: impl FnOnce() -> Desktop) -> bool {
    if cloaked == 0 {
        // Not cloaked. Nothing to rescue and nothing to ask.
        return true;
    }
    locate() == Desktop::Other
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    /// The DWM values, spelled out so the tests read like the docs. Not
    /// re-exported: nothing in beckon branches on them, and this module's
    /// whole point is that they are the wrong thing to branch on.
    const DWM_CLOAKED_APP: u32 = 0x1;
    const DWM_CLOAKED_SHELL: u32 = 0x2;
    const DWM_CLOAKED_INHERITED: u32 = 0x4;

    #[test]
    fn an_uncloaked_window_never_asks_the_virtual_desktop_manager() {
        let asked = Cell::new(false);
        let kept = admit_window(0, || {
            asked.set(true);
            Desktop::Other
        });
        assert!(kept);
        assert!(
            !asked.get(),
            "the COM call must not be made for an uncloaked window -- that is \
             almost every window on the machine, and this is the hot path"
        );
    }

    #[test]
    fn a_window_on_another_virtual_desktop_is_rescued() {
        // The whole defect: this used to return false, so the hotkey launched
        // a second copy of an app that was already running on desktop 2.
        assert!(admit_window(DWM_CLOAKED_SHELL, || Desktop::Other));
    }

    #[test]
    fn a_suspended_uwp_app_on_this_desktop_stays_out() {
        // The regression guard. A suspended UWP app reports the SAME cloak
        // word as a window on another desktop -- `0x2` -- which is why the
        // bits cannot be the discriminator. It is on the current desktop, so
        // it stays out.
        assert!(!admit_window(DWM_CLOAKED_SHELL, || Desktop::Current));
    }

    #[test]
    fn a_failed_com_call_falls_back_to_todays_behaviour() {
        // No object, or a failing HRESULT. Today beckon drops every cloaked
        // window; a failure must land exactly there and never on "include
        // everything".
        for cloaked in [DWM_CLOAKED_APP, DWM_CLOAKED_SHELL, DWM_CLOAKED_INHERITED] {
            assert!(!admit_window(cloaked, || Desktop::Unknown));
        }
    }

    #[test]
    fn the_cloak_bits_are_not_the_discriminator() {
        // Every cloak value behaves identically; only the desktop answer
        // moves the outcome. If someone "simplifies" this by testing for
        // `0x2`, these four assertions are what break.
        for cloaked in [
            DWM_CLOAKED_APP,
            DWM_CLOAKED_SHELL,
            DWM_CLOAKED_INHERITED,
            DWM_CLOAKED_SHELL | DWM_CLOAKED_INHERITED,
        ] {
            assert!(admit_window(cloaked, || Desktop::Other), "{cloaked:#x}");
            assert!(!admit_window(cloaked, || Desktop::Current), "{cloaked:#x}");
        }
    }

    #[test]
    fn an_unknown_cloak_bit_is_still_a_cloaked_window() {
        // DWM could grow a fourth bit. It is still cloaked, so it is still
        // subject to the rescue rule rather than waved through.
        assert!(!admit_window(0x8, || Desktop::Current));
        assert!(admit_window(0x8, || Desktop::Other));
    }
}
