//! Hand a file or an address to the Finder, the way
//! `beckon_windows::shell` hands one to Explorer.
//!
//! Three calls, all through `/usr/bin/open`. That is the same shell-out the
//! backend's launch path already uses, and it is deliberate rather than
//! lazy: `NSWorkspace`'s equivalents are `@MainActor` and, on modern macOS,
//! completion-handler-only, so reaching them from a settings-window callback
//! would mean either a main-queue hop this crate does not yet have or
//! spinning a run loop inside a click handler. `open` returns in ~10-20 ms
//! and none of these three is on a hot path — a person clicked a button.

use std::path::Path;

/// Open a file with whatever the user has associated with it.
pub fn open_path(p: &Path) -> Result<(), String> {
    run(&[p.as_os_str()], &format!("open {}", p.display()))
}

/// Show a file in the Finder, selected in its containing folder.
///
/// `-R` is `open`'s spelling of `NSWorkspace`'s
/// `selectFile:inFileViewerRootedAtPath:`, and it is the second glyph on
/// each of the System door's file rows. It differs from `open_path` in the
/// way that matters for a config file: it puts the reader *next to* the
/// file, where they can rename or duplicate it, instead of handing it to
/// whichever editor claimed `.toml`.
pub fn reveal_path(p: &Path) -> Result<(), String> {
    run(
        &["-R".as_ref(), p.as_os_str()],
        &format!("reveal {}", p.display()),
    )
}

/// Open a web address in the default browser.
///
/// **`https://` only, and the check is not decoration.** `open` will happily
/// launch a `file://` URL, an `x-apple-...` scheme, or anything else with a
/// registered handler, so an unchecked pass-through turns three About-page
/// buttons into a general "make this Mac do a thing" surface. The Win32 twin
/// refuses on exactly the same rule; the addresses themselves are
/// `Target::url`'s, in core, where a test can read them.
pub fn open_url(url: &str) -> Result<(), String> {
    if !url.starts_with("https://") {
        return Err(format!("refusing to open `{url}`: not an https URL"));
    }
    run(&[url.as_ref()], &format!("open {url}"))
}

/// The System Settings pane beckon's own event tap needs.
///
/// **A separate function rather than a relaxed `open_url`, and that is the
/// whole point.** `open_url`'s `https://` check exists because `/usr/bin/open`
/// will launch any scheme with a registered handler, so letting an
/// `x-apple.systempreferences:` URL through there would reopen exactly the
/// hole that check closes -- for every caller, not just this one. Here the
/// address is a constant no caller can influence, so there is nothing to
/// validate and nothing to widen.
///
/// The anchor is `Privacy_ListenEvent`, which is the same TCC service name
/// `IOHIDCheckAccess(kIOHIDRequestTypeListenEvent)` asks about, so the pane
/// that opens is the one whose switch changes that answer.
pub const INPUT_MONITORING_PANE: &str =
    "x-apple.systempreferences:com.apple.preference.security?Privacy_ListenEvent";

/// Open System Settings at Privacy & Security > Input Monitoring.
///
/// A button rather than a sentence, because the sentence names a pane four
/// clicks deep and every one of those clicks is a chance to land in
/// Accessibility instead -- the neighbouring row, the permission this is
/// most often confused with, and the one that is already granted.
pub fn open_input_monitoring() -> Result<(), String> {
    run(
        &[INPUT_MONITORING_PANE.as_ref()],
        "open Input Monitoring settings",
    )
}

fn run(args: &[&std::ffi::OsStr], what: &str) -> Result<(), String> {
    std::process::Command::new("/usr/bin/open")
        .args(args)
        .status()
        .map_err(|e| format!("cannot {what}: {e}"))
        .and_then(|s| {
            if s.success() {
                Ok(())
            } else {
                Err(format!("{what} exited {s}"))
            }
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **The pane anchor must be the one TCC service the tap asks about.**
    /// `Privacy_Accessibility` is the neighbouring row and the permission this
    /// is most often confused with -- opening it would send a reader to a
    /// switch that is already on, which is worse than the sentence this button
    /// replaces.
    #[test]
    fn the_pane_is_input_monitoring_and_not_accessibility() {
        assert!(INPUT_MONITORING_PANE.contains("Privacy_ListenEvent"));
        assert!(!INPUT_MONITORING_PANE.contains("Accessibility"));
    }

    /// It is deliberately NOT an https URL, which is why it cannot go through
    /// `open_url`. This pins the pair: the constant is refused there, so a
    /// later "simplification" that routes it through `open_url` fails loudly
    /// instead of quietly widening that function's guard.
    #[test]
    fn open_url_still_refuses_the_settings_scheme() {
        assert!(open_url(INPUT_MONITORING_PANE).is_err());
    }
}
