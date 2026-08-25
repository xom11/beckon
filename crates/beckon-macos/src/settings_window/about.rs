//! Door 4 — **About**.
//!
//! Design §3.4. It writes nothing: four value rows with a copy button each
//! (`Build`, `Location`, `Licence`, and the update command), three links,
//! and one disclosure — plus the update check's own controls, which carry
//! no copy button of their own: a status line, `Check now`, and `Open
//! releases page`. `command_bar_shown` draws no `Save` here, and
//! `home(Page::About)` is `None`, so Enter does nothing until the reader
//! tabs onto a button.
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
//! **The disclosure is drawn, in this platform's words rather than core's.**
//! `HOOK_DISCLOSURE` reads *"The keyboard hook is installed only while Caps
//! Lock is on, or while you are recording a shortcut."* The first half is
//! now true here — `beckon_macos::caps_tap` installs a `CGEventTap` exactly
//! then — but the second is not: chord capture is not built on macOS, so
//! that clause would name a thing this program cannot do. A true sentence
//! with a false clause in it is worse on the one page whose job is
//! disclosure, so the string is local until capture lands and the two
//! converge. It keeps both halves core's version has, and the second is the
//! one no icon or control could draw: a negative claim about what beckon
//! does NOT keep.
//!
//! **An Accessibility row is drawn instead**, and it is not a substitute
//! chosen for symmetry. It is this platform's version of the same question —
//! *what does this program need, and what does it do with it* — and on macOS
//! the answer is the single largest cause of "beckon does nothing": the
//! grant is bound to the binary's code signature, so a rebuilt binary loses
//! it silently and every hotkey stops focusing anything with no error
//! anywhere. `beckon doctor` already reports it; About is where a person
//! who has not thought to run `doctor` will be standing.

use beckon_core::settings::{copy_text, grant_button_shown, AboutState, Field, FlagTone, ImageAge};
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::sel;
use objc2_app_kit::{NSBox, NSButton, NSStackView, NSTextField, NSView};
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
    /// Offered only while the grant is missing (`grant_button_shown`).
    ///
    /// Without it this page could state the largest single cause of "beckon
    /// does nothing" and give the reader nowhere to go: `AXIsProcessTrusted`
    /// only reads an answer, so a binary with no TCC row cannot acquire one
    /// through anything beckon calls, and the pane wants a path carrying a
    /// nix hash or a Homebrew version.
    pub(super) grant: Retained<NSButton>,

    /// The update check's own line. `None` in `UpdateRow::status` draws
    /// nothing at all -- see `apply` -- so this is hidden, not blanked, in
    /// that state.
    pub(super) update_status: Retained<NSTextField>,
    /// Enabled iff `state.update.can_check` -- disabled for the length of a
    /// check, which blocks this thread (see `UpdateRow::can_check`'s own
    /// doc).
    pub(super) check_now: Retained<NSButton>,
    /// The upgrade command's whole row, hidden unless `state.update.command`
    /// is `Some` -- there is one only once a check finds a newer release.
    pub(super) command_row: Retained<NSStackView>,
    /// `cmd.shown`, which may carry a caveat the bare `cmd.copy` on the
    /// clipboard must not -- see `copy_field(Field::UpdateCommand)` in
    /// `mod.rs`, which reads `cmd.copy` through
    /// `beckon_core::settings::copy_text`.
    pub(super) command_value: Retained<NSTextField>,
    /// Hidden until a check has produced ANY verdict, including a failure --
    /// what gives a user with no curl somewhere to go. Mirrors
    /// `state.update.status.is_some()`, the same gate `update_status` uses.
    pub(super) open_releases_row: Retained<NSStackView>,
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
    //
    // **Three measured corrections, and two of them only work together.**
    // Measured with `examples/geom_probe.rs -- BECKON_PROBE_PAGE=about` on
    // macmini 2026-08-17, which reads this box and its content view out of the
    // real window:
    //
    // ```text
    // tile 34.0 x 34.0
    // glyph field x 5.0 y 5.0 w 24.0 h 24.0
    // contentViewMargins 5.0 x 5.0
    // alignment NSTextAlignment(4)      <- .natural, i.e. LEFT for LTR
    // font 13.0 pt                      <- tile/font ratio 0.382
    // ```
    //
    // 1. `contentViewMargins` was the `NSBox` DEFAULT of 5x5, never zeroed --
    //    `widgets::card` zeroes it by hand and this box did not. So the letter
    //    could only ever occupy the middle 24 pt of a 34 pt tile.
    // 2. `alignment` was `.natural`. The box stretches its content view to
    //    fill, so a single glyph in a field wider than itself sits at the
    //    field's LEADING edge, and how far off centre that looks is a function
    //    of the field's width rather than of anything intended. The `beckon`
    //    label ten lines down already sets `Center` explicitly; this one is the
    //    same case and did not.
    //
    //    **Fixing 1 without 2 makes it worse**, which is why they are one
    //    change: zeroing the margins takes the field from 24 pt to 34 pt, and a
    //    leading-aligned glyph in a wider field is further from the middle, not
    //    nearer.
    // 3. The letter was 13 pt in a 34 pt tile, a ratio of 0.382. The design's
    //    ratio is 0.5 and the Win32 twin carries it as `18/36`, with the reason
    //    in `paint.rs`: below it "the tile reads as a letter adrift in a box".
    //    17 pt is 0.5 of 34.
    //
    // 4. **`NSTextField` does not centre text vertically** -- a single line in a
    //    taller frame draws at the TOP, and there is no alignment enum for the
    //    other axis. The old 5 pt margin was accidentally paying for part of
    //    that, so zeroing it made the vertical WORSE, which is the trap in this
    //    change and the reason all four items ship together. The glyph is now a
    //    centred child of a host view (`widgets::centred_both`) rather than the
    //    box's content view itself.
    //
    // **All four measured in PIXELS, offscreen.** `cacheDisplay(in:to:)` renders
    // through CoreGraphics and needs no window server, so the one thing layout
    // could not answer -- where the ink actually lands -- was measurable from a
    // Background-namespace session after all. `dy` negative means high:
    //
    // ```text
    //                                              dx      dy
    // shipped   margins 5, natural, 13pt        -5.75   -3.75
    // margins 0, center, 17pt, label is content +0.25   -7.25   <- nearly shipped
    // margins 0, center, 17pt, centred host     +0.75   -0.25   <- ships
    // ```
    //
    // The control on every one of those runs: the opaque pixel count came out
    // 4412 of 4624, i.e. the tile drew and the missing 212 are its four rounded
    // corners. A bitmap nothing drew into yields "no ink", which reads exactly
    // like a perfectly centred glyph.
    //
    // The residual +0.75 is `b`'s own side bearings being unequal, and the
    // -0.25 is the line box's ascender-without-descender asymmetry. Both are
    // under a point; neither is worth a hand-tuned offset.
    //
    // **The letter's COLOUR is left alone deliberately.** It has none set, so it
    // takes `labelColor`, which means the mark is dark-on-accent in light mode
    // and light-on-accent in dark. Measured contrast against
    // `controlAccentColor`: 5.23:1 light and 4.02:1 dark, versus a fixed white's
    // 4.02:1 in both -- so the semantic colour is no worse anywhere and better
    // in light mode. The Win32 twin pins `accent_on` because it has no semantic
    // colours to lean on, not because a pinned pair is the better answer.
    let glyph = w::heading_sized("b", 17.0, mtm);
    glyph.setAlignment(objc2_app_kit::NSTextAlignment::Center);
    let glyph_host = w::centred_both(&glyph, mtm);
    let mark = NSBox::new(mtm);
    {
        mark.setBoxType(objc2_app_kit::NSBoxType::Custom);
        mark.setTitlePosition(objc2_app_kit::NSTitlePosition::NoTitle);
        mark.setFillColor(&objc2_app_kit::NSColor::controlAccentColor());
        mark.setBorderWidth(0.0);
        mark.setCornerRadius(8.0);
        // Zero, like `widgets::card`. The default is 5x5 -- see the note above.
        mark.setContentViewMargins(objc2_foundation::NSSize::new(0.0, 0.0));
        mark.setContentView(Some(&glyph_host));
    }
    w::pin_height(&mark, 34.0);
    w::pin_exact_width(&mark, 34.0);
    let mark_row = w::centred(&mark, mtm);

    let name = w::heading("beckon", mtm);
    name.setAlignment(objc2_app_kit::NSTextAlignment::Center);

    let (build_row, build) = value_row("Build", sel!(beckonCopyBuild:), target, mtm);
    let (loc_row, location) = value_row("Location", sel!(beckonCopyLocation:), target, mtm);
    let (lic_row, licence) = value_row("Licence", sel!(beckonCopyLicence:), target, mtm);

    // The update check's own row, beside the `Build` row it is a verdict
    // about: the status line (tone-coloured in `apply`) and `Check now`.
    let update_status = w::label("", mtm);
    let check_now = w::push("Check now", sel!(beckonCheckForUpdates:), target, mtm);
    let update_row = w::hstack(
        &[&*update_status as &NSView, &*w::spring(mtm), &check_now],
        mtm,
    );

    // The upgrade command, shown only once a check finds one. `cmd.shown`
    // is drawn here, by `apply` below; the Copy button puts `cmd.copy` on
    // the clipboard instead, by way of `copy_field(Field::UpdateCommand)`
    // in `mod.rs`, which does not choose the half itself -- it calls
    // `beckon_core::settings::copy_text`, the one place in core that maps a
    // `Field` to `.shown` or `.copy`. Neither this file's `apply` nor that
    // core function may swap them.
    let command_value = w::label("", mtm);
    let command_copy = w::glyph(
        "Copy",
        "Copy to clipboard",
        sel!(beckonCopyUpdateCommand:),
        target,
        mtm,
    );
    let command_row = w::hstack(
        &[&*command_value as &NSView, &*w::spring(mtm), &command_copy],
        mtm,
    );

    // A way to the releases page for every verdict a check can reach,
    // including a failure -- the one case with no upgrade command at all.
    // `grant_row` just below is the same shape: one left-aligned button,
    // hidden as a whole row until it has something to say.
    let open_releases = w::push("Open releases page", sel!(beckonReleases:), target, mtm);
    let open_releases_row = w::hstack(&[&*open_releases as &NSView, &*w::spring(mtm)], mtm);

    let image = w::secondary("", mtm);
    let access = w::wrapping("", mtm);
    let grant = w::push(
        "Grant Accessibility…",
        sel!(beckonGrantAccess:),
        target,
        mtm,
    );
    let grant_row = w::hstack(&[&*grant as &NSView, &*w::spring(mtm)], mtm);

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
            &*mark_row as &NSView,
            &name,
            &w::divider(mtm),
            &build_row,
            &update_row,
            &command_row,
            &open_releases_row,
            &loc_row,
            &image,
            &lic_row,
            &w::divider(mtm),
            &access,
            &grant_row,
            &links,
        ],
        10.0,
        mtm,
    );

    // The disclosure is the one child a `Width`-aligned column does not
    // stretch on its own -- it came out indented a third of the way across
    // the card. Pinned to the column instead of argued with.
    w::pin_width_to(&access, &inner, 0.0);
    // The name is centred by its own text alignment, which only means
    // anything once the label is as wide as the card — a label sized to its
    // text has no room to centre in, and the column put it hard right.
    w::pin_width_to(&name, &inner, 0.0);

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
            grant,
            update_status,
            check_now,
            command_row,
            command_value,
            open_releases_row,
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

    // The update check's own line. `status` is `None` only in `Idle` --
    // `UpdateRow`'s own rule is to draw nothing at all then, not an empty
    // line, so the field is hidden rather than blanked.
    c.update_status.setStringValue(&NSString::from_str(
        st.update.status.as_deref().unwrap_or(""),
    ));
    c.update_status.setHidden(st.update.status.is_none());
    // Set on every push, not only for `Warn`: a line left orange from a
    // failed check must not stay orange once a later check succeeds.
    let tone = match st.update.tone {
        FlagTone::Warn => objc2_app_kit::NSColor::systemOrangeColor(),
        // `update_row` never produces `Bad` for this row -- see its own doc
        // -- but the match stays exhaustive rather than folding into a
        // wildcard, so a tone this page has no colour for is a compile
        // error here, not a silent default.
        FlagTone::Bad => objc2_app_kit::NSColor::systemRedColor(),
        FlagTone::Neutral => objc2_app_kit::NSColor::labelColor(),
    };
    c.update_status.setTextColor(Some(&tone));
    c.check_now.setEnabled(st.update.can_check);

    match &st.update.command {
        Some(cmd) => {
            c.command_value
                .setStringValue(&NSString::from_str(&cmd.shown));
            c.command_row.setHidden(false);
        }
        None => {
            c.command_value.setStringValue(&NSString::from_str(""));
            c.command_row.setHidden(true);
        }
    }

    // Whenever a check has reached ANY verdict, including a failure -- what
    // gives a user with no curl somewhere to go.
    c.open_releases_row.setHidden(st.update.status.is_none());

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

    // Offered only while it can do something -- see `grant_button_shown`.
    c.grant.setHidden(!grant_button_shown(ax_trusted));

    c.access.setStringValue(&NSString::from_str(if ax_trusted {
        // **`or while you are recording a shortcut` is not padding.** Chord
        // capture arms the same tap on a machine where the reader
        // deliberately left `keyboard.caps` off, so without this clause the
        // sentence is a false claim about when beckon can see keystrokes --
        // the one kind of wrong sentence this page exists to avoid. It is the
        // same wording `HOOK_DISCLOSURE` carries on Windows, for the same
        // widening.
        "beckon has Accessibility permission, which is what lets it focus and cycle \
         windows. It reads window lists and raises windows. The keyboard event tap is \
         installed only while Caps Lock is on as a shortcut key, or while you are \
         recording a shortcut; beckon keeps no record of what you type."
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
