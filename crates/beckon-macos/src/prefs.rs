//! The settings window's own look, in `NSUserDefaults`.
//!
//! The macOS counterpart of `beckon_windows::prefs`, and it exists for the
//! same reason design §1 gives: Shortcuts and Keyboard write `apps.toml`,
//! System writes *here* or nothing. The split is what keeps a look preference
//! working when `apps.toml` does not parse -- a transparency slider has
//! nothing to do with a TOML error and must not be hostage to one.
//!
//! **It is deliberately SMALLER than the Windows twin, and the missing value
//! is not an oversight.** `DarkMode` has no counterpart because every colour
//! in this window is a semantic `NSColor` and follows the system on its own;
//! there is no control to store the state of, and storing one would create a
//! second answer to a question AppKit already answers.
//!
//! | Key | Meaning |
//! |---|---|
//! | `Opacity` | 85..=100, the transparency slider. Absent means `OPACITY_DEFAULT` |
//! | `CapsView` | Write a bound chord as `Caps` in the list. Absent means false |
//!
//! **Absent is not zero.** Both readers say so explicitly through
//! `Option`, because a fresh profile has neither key and a missing `Opacity`
//! read as 0 would ship a fully transparent window to everyone who never
//! touched the slider.
//!
//! `settings_window::mod`'s `opacity` field used to carry a comment saying
//! persisting it "is `NSUserDefaults` and one line, deliberately not taken in
//! this pass -- a preference that outlives the window should be introduced
//! with the reload path that has to honour it, not before". This module is
//! that pass, and `Ui::new` reading `opacity()` at construction is the reload
//! path: the window is built fresh on every open, so there is exactly one
//! place the stored value has to be honoured and it cannot be missed.

use beckon_core::settings::{clamp_opacity, OPACITY_DEFAULT};
use objc2_foundation::{NSString, NSUserDefaults};

const OPACITY: &str = "Opacity";
const CAPS_VIEW: &str = "CapsView";

/// What a stored `Opacity` means, separated from where it is stored.
///
/// Pure so the decision is testable without touching the user's defaults
/// database -- the same split `beckon_windows::prefs` gets for free by
/// returning `Option<u32>` from `read` and letting each caller supply its own
/// default.
///
/// **`None` and an out-of-range number are different inputs with the same
/// safe answer, and both are real.** `None` is a fresh profile. A number
/// outside 85..=100 is a hand-edited `defaults write`, or a value stored by a
/// future version with a wider range; `clamp_opacity` is what stops either
/// from producing a window nobody can see.
pub(crate) fn opacity_from_stored(raw: Option<isize>) -> u8 {
    match raw {
        None => OPACITY_DEFAULT,
        Some(n) => clamp_opacity(n.clamp(0, u8::MAX as isize) as u8),
    }
}

/// Read an integer, or `None` when the key was never written.
///
/// `objectForKey` first: `integerForKey` returns 0 for both "absent" and
/// "stored zero", and this module's whole contract is that those differ.
fn read_int(key: &str) -> Option<isize> {
    let defaults = NSUserDefaults::standardUserDefaults();
    let k = NSString::from_str(key);
    defaults.objectForKey(&k)?;
    Some(defaults.integerForKey(&k))
}

fn write_int(key: &str, value: isize) {
    let defaults = NSUserDefaults::standardUserDefaults();
    defaults.setInteger_forKey(value, &NSString::from_str(key));
}

pub fn opacity() -> u8 {
    opacity_from_stored(read_int(OPACITY))
}

pub fn set_opacity(percent: u8) {
    write_int(OPACITY, clamp_opacity(percent) as isize);
}

pub fn caps_view() -> bool {
    read_int(CAPS_VIEW).is_some_and(|n| n != 0)
}

pub fn set_caps_view(on: bool) {
    write_int(CAPS_VIEW, isize::from(on));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unwritten_key_means_the_default_and_not_zero() {
        assert_eq!(
            opacity_from_stored(None),
            OPACITY_DEFAULT,
            "a fresh profile has no key; reading that as 0 would ship an \
             invisible window to everyone who never touched the slider"
        );
    }

    #[test]
    fn a_stored_value_survives_the_round_trip() {
        assert_eq!(opacity_from_stored(Some(92)), 92);
    }

    /// `defaults write com.beckon Opacity 3` is one command away, and a
    /// future version could widen the range and store something this one
    /// cannot draw.
    #[test]
    fn a_value_outside_the_range_is_clamped_rather_than_trusted() {
        assert_eq!(opacity_from_stored(Some(3)), clamp_opacity(3));
        assert_eq!(opacity_from_stored(Some(999)), 100);
        assert_eq!(
            opacity_from_stored(Some(-5)),
            clamp_opacity(0),
            "a negative reads as the floor, not as a huge u8 through a cast"
        );
    }
}
