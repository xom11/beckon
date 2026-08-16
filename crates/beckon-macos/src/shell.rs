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
