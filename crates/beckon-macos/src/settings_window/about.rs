//! Door 4 — **About**.
//!
//! Design §3.4. It writes nothing: three value rows with a copy button
//! each, three links, and one disclosure. `command_bar_shown` draws no
//! `Save` here, and `home(Page::About)` is `None`, so Enter does nothing
//! until the reader tabs onto a button.
//!
//! ## `Location` is the row this page exists for
//!
//! It shows the RUNNING IMAGE's path and, when there is one worth printing,
//! a verdict about its age. The incident behind it is recorded in
//! `CLAUDE.md`: on a14 a watchdog-started beckon ran the 0.8.0 image for
//! three hours while `beckon --version` said 0.9.0 and scoop's `current`
//! junction pointed at 0.9.0 — every obvious surface agreed, and every one
//! of them was wrong. The path is deliberately not resolved through the
//! platform's "give me the real file" call, because resolving reports
//! today's link target, which is the surface that lied.
//!
//! ## Two divergences from the Windows twin, both deliberate
//!
//! **The hook disclosure is not drawn yet.** `HOOK_DISCLOSURE` reads *"The
//! keyboard hook is installed only while Caps Lock is on, or while you are
//! recording a shortcut."* On macOS neither of those exists: the Caps Lock
//! shorthand and chord capture both need a `CGEventTap`, and neither is
//! built. The sentence would be *vacuously* true — no hook is ever installed
//! — while telling a reader that a keyboard hook is part of this program.
//! A true sentence that leaves a false impression is worse than no sentence,
//! and this one is on the page whose whole job is disclosure. It comes back,
//! reworded for the tap, in the same change that adds it.
//!
//! **An Accessibility row is drawn instead**, and it is not a substitute
//! chosen for symmetry. It is this platform's version of the same question —
//! *what does this program need, and what does it do with it* — and on macOS
//! the answer is the single largest cause of "beckon does nothing": the
//! grant is bound to the binary's code signature, so a rebuilt binary loses
//! it silently and every hotkey stops focusing anything with no error
//! anywhere. `beckon doctor` already reports it; About is where a person
//! who has not thought to run `doctor` will be standing.

use beckon_core::settings::{copy_text, AboutState, Field, ImageAge};
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::sel;
use objc2_app_kit::{NSBox, NSStackView, NSTextField, NSView};
use objc2_foundation::{MainThreadMarker, NSString};

use super::widgets as w;

#[derive(Clone)]
pub(super) struct AboutControls {
    pub(super) name: Retained<NSTextField>,
    pub(super) build: Retained<NSTextField>,
    pub(super) location: Retained<NSTextField>,
    pub(super) licence: Retained<NSTextField>,
    /// The stale-image verdict, beside `Location` rather than inside it —
    /// `AboutValue` splits `shown` from `copy` precisely so the clipboard
    /// never receives a sentence.
    pub(super) image: Retained<NSTextField>,
    pub(super) access: Retained<NSTextField>,
}

/// `Build` / `Location` / `Licence`: a dimmed name, the value, a copy button.
fn value_row(
    name: &str,
    action: objc2::runtime::Sel,
    target: &AnyObject,
    mtm: MainThreadMarker,
) -> (Retained<NSStackView>, Retained<NSTextField>) {
    let label = w::secondary(name, mtm);
    w::pin_min_width(&label, 68.0);
    let value = w::label("", mtm);
    let copy = w::glyph("Copy", "Copy to clipboard", action, target, mtm);
    let spring = w::spring(mtm);
    let row = w::hstack(&[&*label as &NSView, &value, &spring, &copy], mtm);
    (row, value)
}

pub(super) fn build(
    target: &AnyObject,
    mtm: MainThreadMarker,
) -> (Retained<NSView>, AboutControls) {
    // The mark. A tinted rounded square with the wordmark's letter, rather
    // than the app icon: `beckon` has no `.icns` and an `NSImage` that fails
    // to load draws nothing, which is a blank hole rather than a fallback.
    let glyph = w::heading("b", mtm);
    let mark = NSBox::new(mtm);
    {
        mark.setBoxType(objc2_app_kit::NSBoxType::Custom);
        mark.setTitlePosition(objc2_app_kit::NSTitlePosition::NoTitle);
        mark.setFillColor(&objc2_app_kit::NSColor::controlAccentColor());
        mark.setBorderWidth(0.0);
        mark.setCornerRadius(8.0);
        mark.setContentView(Some(&glyph));
    }
    w::pin_height(&mark, 34.0);
    w::pin_min_width(&mark, 34.0);

    let name = w::heading("beckon", mtm);

    let (build_row, build) = value_row("Build", sel!(beckonCopyBuild:), target, mtm);
    let (loc_row, location) = value_row("Location", sel!(beckonCopyLocation:), target, mtm);
    let (lic_row, licence) = value_row("Licence", sel!(beckonCopyLicence:), target, mtm);

    let image = w::secondary("", mtm);
    let access = w::wrapping("", mtm);

    let github = w::push("GitHub", sel!(beckonGithub:), target, mtm);
    let releases = w::push("Releases", sel!(beckonReleases:), target, mtm);
    let bug = w::push("Report a bug", sel!(beckonBugReport:), target, mtm);
    let links = w::hstack(
        &[
            &*w::spring(mtm) as &NSView,
            &github,
            &releases,
            &bug,
            &w::spring(mtm),
        ],
        mtm,
    );

    let inner = w::vstack(
        &[
            &*mark as &NSView,
            &name,
            &w::divider(mtm),
            &build_row,
            &loc_row,
            &image,
            &lic_row,
            &w::divider(mtm),
            &access,
            &links,
        ],
        10.0,
        mtm,
    );
    inner.setAlignment(objc2_app_kit::NSLayoutAttribute::Width);

    let card = w::card(&inner, mtm);
    let view: Retained<NSView> = card.into_super();

    (
        view,
        AboutControls {
            name,
            build,
            location,
            licence,
            image,
            access,
        },
    )
}

pub(super) fn apply(c: &AboutControls, st: &AboutState, ax_trusted: bool) {
    c.name.setStringValue(&NSString::from_str(&st.name));
    c.build.setStringValue(&NSString::from_str(&st.build.shown));
    c.location
        .setStringValue(&NSString::from_str(&st.location.shown));
    c.licence
        .setStringValue(&NSString::from_str(&st.licence.shown));

    // The verdict, only when there is one. `Current` says nothing: a healthy
    // row saying "healthy" is the noise the Shortcuts door's status
    // vocabulary already refuses to make.
    let verdict = match st.image {
        ImageAge::Current => "",
        ImageAge::Replaced => "This file changed after beckon started. Restart to run it.",
        ImageAge::Missing => "This file is gone. beckon is running from a deleted image.",
        ImageAge::Unknown => "",
    };
    c.image.setStringValue(&NSString::from_str(verdict));
    c.image.setHidden(verdict.is_empty());

    c.access.setStringValue(&NSString::from_str(if ax_trusted {
        "beckon has Accessibility permission, which is what lets it focus and cycle \
         windows. It reads window lists and raises windows; it records nothing."
    } else {
        "beckon does NOT have Accessibility permission. Hotkeys will launch apps but \
         cannot focus or cycle windows. Grant it in System Settings > Privacy & \
         Security > Accessibility. The grant is bound to this exact binary, so a \
         rebuilt or replaced beckon has to be granted again."
    }));
}

/// What the clipboard gets for a row.
///
/// **The row's bare payload, never the string on screen.** `Location` shows a
/// path the OS may have shortened for width and, in the row beneath it, a
/// verdict clause — and a copied path is for pasting into a file manager or
/// a terminal, where neither belongs. `copy_text` is the one decision and it
/// lives in core, where a test can read it.
pub(super) fn clipboard_text(st: &AboutState, f: Field) -> String {
    copy_text(st, f).to_string()
}
