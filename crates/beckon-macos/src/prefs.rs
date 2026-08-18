//! The settings window's own look, in one **named** preferences domain.
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
//! ## The domain is NAMED, and that is the whole point of this module
//!
//! This used to be `NSUserDefaults::standardUserDefaults()`, which has no
//! explicit domain: it uses the main bundle's identifier when the process has
//! one, and the **process name** when it does not. Measured on macmini
//! 2026-08-18, one installed copy of beckon therefore had two stores:
//!
//! | invoked as | `CFBundleGetIdentifier` | domain it read |
//! |---|---|---|
//! | `beckon.app/Contents/MacOS/beckon` (the LaunchAgent) | `com.xom11.beckon` | `com.xom11.beckon` |
//! | `/opt/homebrew/bin/beckon` -- a symlink INTO that bundle | **NULL** | `beckon` |
//!
//! `CFBundleGetMainBundle` looks for `Contents/Info.plist` beside the path
//! **as invoked** and does not follow the symlink, so the CLI on `PATH` and
//! the agent disagreed about where a preference lives. Proof: `Opacity`
//! written through the bundle path read back `(absent)` through the symlink.
//! Both plists existed on the reporter's Mac.
//!
//! The same commit that introduced the bundle (0.9.18, for the Accessibility
//! grant) therefore **orphaned every existing user's `Opacity` and
//! `CapsView`** -- they were in `beckon`, and the agent had started reading
//! `com.xom11.beckon`. `LEGACY_DOMAIN` below is that value being fetched, once.
//!
//! This narrows, and does not contradict, `docs/notes/macos-backend.md`'s
//! "one file serve as both the CLI on `PATH` and the agent, under one
//! identity". True of TCC, which resolves the real path. **False of
//! preferences**, which do not.
//!
//! ## Why not `initWithSuiteName:`
//!
//! It is the obvious fix and it **returns nil in exactly our case**. Measured:
//! with `CFBundleIdentifier = com.xom11.samesuite`, asking for that same suite
//! logged
//!
//! ```text
//! Using your own bundle identifier as an NSUserDefaults suite name does not
//! make sense and will not work.
//! ```
//!
//! and handed back `nil` -- which is the LaunchAgent, i.e. the process that
//! matters most. The objc2 binding says so too (`initWithSuiteName:`:
//! "Passing the current application's bundle identifier ... is an error").
//! `CFPreferences*` takes the domain as a plain argument and has no such rule;
//! it was measured working in both directions, including the bundle-id ==
//! domain case that killed the suite API.
//!
//! `settings_window::mod`'s `opacity` field used to carry a comment saying
//! persisting it "is `NSUserDefaults` and one line, deliberately not taken in
//! this pass -- a preference that outlives the window should be introduced
//! with the reload path that has to honour it, not before". This module is
//! that pass, and `Ui::new` reading `opacity()` at construction is the reload
//! path: the window is built fresh on every open, so there is exactly one
//! place the stored value has to be honoured and it cannot be missed.

use beckon_core::settings::{clamp_opacity, OPACITY_DEFAULT};
use core_foundation::base::{CFType, CFTypeRef, TCFType};
use core_foundation::boolean::CFBoolean;
use core_foundation::number::CFNumber;
use core_foundation::string::{CFString, CFStringRef};

/// The one domain every beckon process reads and writes, whatever path it was
/// launched through. Spelled the same as `assets/macos/Info.plist`'s
/// `CFBundleIdentifier` **because that is where the agent's values already
/// are** -- not because the code asks the bundle for it. Nothing here reads
/// the running bundle; that is the defect this constant exists to close.
const DOMAIN: &str = "com.xom11.beckon";

/// Where a pre-0.9.18 beckon put the same two keys: the process name, because
/// a bare CLI binary has no bundle identifier. Read when `DOMAIN` has nothing,
/// then written forward so the fallback is paid once per key.
const LEGACY_DOMAIN: &str = "beckon";

const OPACITY: &str = "Opacity";
const CAPS_VIEW: &str = "CapsView";

// `CFPreferences` is not in the `core-foundation` crate (only in
// `core-foundation-sys`, which is not a direct dependency), and the surface
// used here is three functions and two globals -- the same trade `hotkey.rs`
// and `ffi.rs` already make.
#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    static kCFPreferencesCurrentUser: CFStringRef;
    static kCFPreferencesAnyHost: CFStringRef;

    fn CFPreferencesCopyValue(
        key: CFStringRef,
        application_id: CFStringRef,
        user_name: CFStringRef,
        host_name: CFStringRef,
    ) -> CFTypeRef;
    fn CFPreferencesSetValue(
        key: CFStringRef,
        value: CFTypeRef,
        application_id: CFStringRef,
        user_name: CFStringRef,
        host_name: CFStringRef,
    );
    fn CFPreferencesSynchronize(
        application_id: CFStringRef,
        user_name: CFStringRef,
        host_name: CFStringRef,
    ) -> u8;
}

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

/// Which of the two domains answers, when the process identity changed under
/// a user who never touched the slider again.
///
/// **`current` wins even when it disagrees**, because the only writer of
/// `current` is this program obeying a drag; a `legacy` value is by
/// construction older than the bundle switch. The pair is a function rather
/// than an `unwrap_or` at the call site so the rule is one place and has a
/// test, exactly like `opacity_from_stored`.
pub(crate) fn pick_stored(current: Option<isize>, legacy: Option<isize>) -> Option<isize> {
    current.or(legacy)
}

/// Read a value from one named domain, or `None` when that domain has no such
/// key.
///
/// **Three plist types, not one.** The old `integerForKey:` coerced, and the
/// coercion was load-bearing: `defaults write com.xom11.beckon Opacity 90`
/// with no type flag stores a **string**, which is the spelling this module's
/// own test comment reaches for. Reading only `CFNumber` would have made that
/// command silently do nothing.
fn read_int_in(domain: &str, key: &str) -> Option<isize> {
    let k = CFString::new(key);
    let d = CFString::new(domain);
    let raw = unsafe {
        CFPreferencesCopyValue(
            k.as_concrete_TypeRef(),
            d.as_concrete_TypeRef(),
            kCFPreferencesCurrentUser,
            kCFPreferencesAnyHost,
        )
    };
    if raw.is_null() {
        return None;
    }
    // `Copy` in the name: this is a create rule, so the wrapper owns it.
    let v = unsafe { CFType::wrap_under_create_rule(raw) };
    if let Some(n) = v.downcast::<CFNumber>() {
        return n.to_i64().map(|n| n as isize);
    }
    if let Some(b) = v.downcast::<CFBoolean>() {
        return Some(isize::from(bool::from(b)));
    }
    v.downcast::<CFString>()
        .and_then(|s| s.to_string().trim().parse::<isize>().ok())
}

/// Read `DOMAIN`, falling back to `LEGACY_DOMAIN` **and writing the answer
/// forward** so the fallback is paid at most once per key.
///
/// The write-forward is what makes this a migration rather than a permanent
/// second lookup. Its one visible consequence: `defaults delete
/// com.xom11.beckon Opacity` expecting the default will resurrect the legacy
/// value on the next read, once. Deleting the key from both domains is the
/// clean reset, and that pairing is why `LEGACY_DOMAIN` is named rather than
/// inlined.
fn read_int(key: &str) -> Option<isize> {
    let current = read_int_in(DOMAIN, key);
    if current.is_some() {
        return current;
    }
    let migrated = pick_stored(current, read_int_in(LEGACY_DOMAIN, key));
    if let Some(v) = migrated {
        write_int(key, v);
    }
    migrated
}

fn write_int(key: &str, value: isize) {
    let k = CFString::new(key);
    let d = CFString::new(DOMAIN);
    let n = CFNumber::from(value as i64);
    unsafe {
        CFPreferencesSetValue(
            k.as_concrete_TypeRef(),
            n.as_CFTypeRef(),
            d.as_concrete_TypeRef(),
            kCFPreferencesCurrentUser,
            kCFPreferencesAnyHost,
        );
        // **Not tidiness.** `standardUserDefaults` had a flush of its own;
        // `CFPreferencesSetValue` is documented to need this, and beckon can
        // have two processes against one domain (the LaunchAgent and a
        // `beckon serve` typed in a terminal), so a value that only exists in
        // this process's cfprefsd cache is a value the other one cannot see.
        CFPreferencesSynchronize(
            d.as_concrete_TypeRef(),
            kCFPreferencesCurrentUser,
            kCFPreferencesAnyHost,
        );
    }
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

    /// `defaults write com.xom11.beckon Opacity 3` is one command away, and a
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

    #[test]
    fn the_current_domain_wins_over_the_legacy_one() {
        assert_eq!(
            pick_stored(Some(88), Some(100)),
            Some(88),
            "the only writer of the current domain is a drag in this version; \
             a legacy value is older than the bundle switch by construction"
        );
    }

    /// The 0.9.18 bundle switch moved the domain out from under everyone who
    /// had ever touched the slider. Without this arm they all read as a fresh
    /// profile.
    #[test]
    fn a_legacy_value_is_adopted_when_the_current_domain_is_empty() {
        assert_eq!(pick_stored(None, Some(91)), Some(91));
        assert_eq!(
            opacity_from_stored(pick_stored(None, Some(91))),
            91,
            "and it survives the grading, rather than being adopted and then \
             thrown away by the default"
        );
    }

    #[test]
    fn two_empty_domains_are_still_a_fresh_profile() {
        assert_eq!(pick_stored(None, None), None);
        assert_eq!(
            opacity_from_stored(pick_stored(None, None)),
            OPACITY_DEFAULT
        );
    }

    /// `DOMAIN` is spelled out here on purpose: the bug this module closes was
    /// the domain being *derived* at runtime from how the process was
    /// launched. A test that recomputed it would re-introduce the defect it is
    /// meant to pin.
    #[test]
    fn the_domain_is_a_literal_and_matches_the_shipped_bundle_identifier() {
        assert_eq!(DOMAIN, "com.xom11.beckon");
        assert_eq!(LEGACY_DOMAIN, "beckon");
        assert_ne!(
            DOMAIN, LEGACY_DOMAIN,
            "if these ever converge the migration read is a no-op and the \
             write-forward loops on itself"
        );
    }
}
