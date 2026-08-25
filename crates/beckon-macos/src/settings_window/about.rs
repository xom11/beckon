//! Door 4 — **About**.
//!
//! Design §3.4. It writes nothing. **Compacted 2026-08-25**: an identity
//! block (the mark, the name, and a build line with one copy button), the
//! update check's status line and `Check now`, the upgrade command's row
//! when there is one, `Location` with its own copy button, a two-part
//! disclosure, and three links. `command_bar_shown` draws no `Save` here,
//! and `home(Page::About)` is `None`, so Enter does nothing until the reader
//! tabs onto a button.
//!
//! **Three things went in that pass and each was earning nothing.** The
//! `Licence` row restated `MIT OR Apache-2.0`, which ships beside the binary
//! and is one click away in the repo. The `Build` label named nothing a
//! target triple did not. And `Open releases page` sent the reader where the
//! `Releases` link already sends them -- the same selector, two controls.
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
//! **CORRECTED 2026-08-25 — the old reason here had expired.** It read that
//! `HOOK_DISCLOSURE`'s *"or while you are recording a shortcut"* named a
//! thing macOS could not do, so the string had to stay local "until capture
//! lands". Chord capture landed; that clause is true here now, and `apply`'s
//! own comment had said so for some time while this paragraph went on giving
//! the opposite reason.
//!
//! Two reasons survive, and both are about wording rather than capability.
//! Core says *"the keyboard hook"* — Windows' word for Windows' mechanism,
//! where this platform installs a `CGEventTap`. And core says *"while Caps
//! Lock is on"*, which its own doc spends a paragraph explaining does not
//! mean the lock's LED; `as a shortcut key` removes the ambiguity on the
//! page instead of annotating it in a comment nobody reading the window can
//! see.
//!
//! **An Accessibility row is drawn instead**, and it is not a substitute
//! chosen for symmetry. It is this platform's version of the same question —
//! *what does this program need, and what does it do with it* — and on macOS
//! the answer is the single largest cause of "beckon does nothing": the
//! grant is bound to the binary's code signature, so a rebuilt binary loses
//! it silently and every hotkey stops focusing anything with no error
//! anywhere. `beckon doctor` already reports it; About is where a person
//! who has not thought to run `doctor` will be standing.

use beckon_core::settings::{
    accessibility_warning, copy_text, grant_button_shown, AboutState, Field, FlagTone, ImageAge,
};
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::sel;
use objc2_app_kit::{NSBox, NSButton, NSStackView, NSTextField, NSView};
use objc2_foundation::{MainThreadMarker, NSString};

use super::widgets as w;

#[derive(Clone)]
pub(super) struct AboutControls {
    pub(super) name: Retained<NSTextField>,
    /// The secondary line directly under `name`, with no label of its own:
    /// `aarch64-apple-darwin · 2026-08-25`. The word `Build` beside a target
    /// triple named nothing the triple did not already say, so the identity
    /// block is two lines and one button instead of a heading plus a
    /// labelled row.
    ///
    /// Its Copy sends `Field::Build`, which `copy_text` answers with the name
    /// PREPENDED -- the one place on this page where the clipboard carries
    /// more than the screen. See `about_state`'s own comment for why that is
    /// not the `Copy diagnostics` button design §3.3 deleted.
    pub(super) build: Retained<NSTextField>,
    pub(super) location: Retained<NSTextField>,
    /// The stale-image verdict, beside `Location` rather than inside it —
    /// `AboutValue` splits `shown` from `copy` precisely so the clipboard
    /// never receives a sentence.
    pub(super) image: Retained<NSTextField>,
    /// The Accessibility warning, and **nothing at all when the grant is
    /// present** -- `accessibility_warning` returns `None` there, on the rule
    /// the Shortcuts list already follows: a row in good order says nothing.
    ///
    /// Before the page was compacted this field carried a four-sentence
    /// paragraph in BOTH states, three of whose sentences described a
    /// permission that was working. It was the tallest block on the page for
    /// a reader with nothing wrong.
    pub(super) access: Retained<NSTextField>,
    /// The keyboard-tap disclosure. Local rather than core's
    /// `HOOK_DISCLOSURE` -- the module doc gives the two wording reasons.
    ///
    /// **Split out of `access` when the page was compacted, and the split is
    /// the point.** This is a claim about what beckon does with the keyboard,
    /// true whether or not Accessibility is granted, so it stays on screen in
    /// both states; the sentence above it is a report about a permission and
    /// disappears when there is nothing to report. Folding them back into one
    /// field would tie a permanent claim to a conditional one again.
    pub(super) hook: Retained<NSTextField>,
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
}

/// A dimmed name, the value, a copy button.
///
/// **One caller left.** `Build` was folded into the identity block above the
/// divider (no label -- see `AboutControls::build`) and `Licence` was removed
/// with the page's compaction, so this now serves `Location` alone. It stays a
/// helper rather than being inlined because the row it builds is the shape any
/// future labelled value would want, and because inlining it would bury the
/// 68 pt label pin that keeps the value column aligned.
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

    // **The identity block: the name, then this line, then one button.**
    // No `Licence` row -- `MIT OR Apache-2.0` is one click away in the repo
    // and the licence text ships beside the binary; restating it here cost a
    // label, a value and a copy button that nobody came to About to press.
    //
    // No `Build` label either. `value_row`'s dimmed name earns its place
    // beside a path, where `Location` says what the string IS; beside a
    // target triple and a date it named nothing the triple did not.
    let build = w::label("", mtm);
    let build_copy = w::glyph(
        "Copy",
        "Copy the version, target and build date",
        sel!(beckonCopyBuild:),
        target,
        mtm,
    );
    let build_row = w::hstack(&[&*build as &NSView, &*w::spring(mtm), &build_copy], mtm);
    let (loc_row, location) = value_row("Location", sel!(beckonCopyLocation:), target, mtm);

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

    // **No `Open releases page` row.** It went to the same destination as the
    // `Releases` button in the links row below -- `sel!(beckonReleases:)`,
    // the same selector -- so a reader with a failed check had two controls
    // for one place. The links row is always on screen, including for every
    // failure state, which is the reason the extra row existed.
    let image = w::secondary("", mtm);
    let access = w::wrapping("", mtm);
    let hook = w::wrapping("", mtm);
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
            // The build line joins the identity block above the divider
            // rather than sitting in the detail group below it: it is the
            // rest of the answer to "which beckon is this", which the name
            // starts and `Location` finishes.
            &build_row,
            &w::divider(mtm),
            &update_row,
            &command_row,
            &loc_row,
            &image,
            &w::divider(mtm),
            &access,
            &grant_row,
            &hook,
            &links,
        ],
        10.0,
        mtm,
    );

    // The disclosure is the one child a `Width`-aligned column does not
    // stretch on its own -- it came out indented a third of the way across
    // the card. Pinned to the column instead of argued with.
    w::pin_width_to(&access, &inner, 0.0);
    // Same reason for the hook line: a `Width`-aligned column does not
    // stretch a wrapping label on its own, and an unpinned one came out
    // indented a third of the way across the card.
    w::pin_width_to(&hook, &inner, 0.0);
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
            image,
            access,
            hook,
            grant,
            update_status,
            check_now,
            command_row,
            command_value,
        },
    )
}

pub(super) fn apply(c: &AboutControls, st: &AboutState, ax_trusted: bool) {
    c.name.setStringValue(&NSString::from_str(&st.name));
    c.build.setStringValue(&NSString::from_str(&st.build.shown));
    c.location
        .setStringValue(&NSString::from_str(&st.location.shown));

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

    // **Nothing at all when the grant is present.** `accessibility_warning`
    // is `None` there, on the rule the Shortcuts door already follows: a row
    // in good order says nothing. What this replaced was two sentences
    // reporting that a permission was working -- true, never news, and the
    // top two thirds of the tallest block on the page.
    //
    // `Some` exactly when `grant_button_shown` is true, so the sentence and
    // the button that fixes it cannot appear without each other; core pins
    // that with a test rather than leaving it to these two calls agreeing.
    let warning = accessibility_warning(ax_trusted);
    c.access
        .setStringValue(&NSString::from_str(warning.unwrap_or("")));
    c.access.setHidden(warning.is_none());

    // **The disclosure, and it is drawn in both states** -- unlike the
    // sentence above it, this is a claim about what beckon DOES with the
    // keyboard rather than a report about a permission, so it is as true
    // with the grant as without it.
    //
    // **`or while you are recording a shortcut` is not padding.** Chord
    // capture arms the same tap on a machine where the reader deliberately
    // left `keyboard.caps` off, so without this clause the sentence is a
    // false claim about when beckon can see keystrokes -- the one kind of
    // wrong sentence this page exists to avoid.
    //
    // **`as a shortcut key` is not padding either**, and it is why this
    // string is still local rather than core's `HOOK_DISCLOSURE`: that one
    // reads `while Caps Lock is on`, which its own doc has to spend a
    // paragraph explaining does NOT mean the lock's LED. Said out loud on
    // the page, the ambiguity is cheaper to remove than to annotate. The
    // other half of the divergence is the noun: this platform installs a
    // `CGEventTap`, and calling it a hook would be Windows' word for
    // Windows' mechanism.
    c.hook.setStringValue(&NSString::from_str(
        "The keyboard event tap is installed only while Caps Lock is on as a shortcut \
         key, or while you are recording a shortcut. beckon keeps no record of what \
         you type.",
    ));
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
