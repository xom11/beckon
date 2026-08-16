//! Small AppKit builders shared by the four doors.
//!
//! Everything here is construction only. Nothing in this file reads
//! `beckon_core`, holds the `UI` borrow, or raises a callback — a widget
//! that did any of those would put a decision in the one crate that two of
//! the three CI jobs cannot compile.
//!
//! ## Semantic colours, not literals
//!
//! Every colour below is one of AppKit's *semantic* colours
//! (`controlBackgroundColor`, `separatorColor`, `secondaryLabelColor`, …).
//! They resolve against the effective `NSAppearance` at draw time, so the
//! whole window follows the system between light and dark with no code and
//! no stored preference. That is the payoff for omitting the Windows
//! design's `Dark mode` row rather than porting it: the Win32 twin needs
//! `theme::pairs` and a `prefs.rs` registry value to do what
//! `NSColor::controlBackgroundColor()` does for free here.
//!
//! It is also why there are no hex literals in this crate. A literal is a
//! colour that is right in exactly one appearance, and the measured
//! high-contrast defect on the Windows side — a fill and its ink resolving
//! to one `GetSysColor` index, i.e. invisible text — is the failure a
//! semantic pair cannot produce.

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Sel};
use objc2_app_kit::{
    NSBezelStyle, NSBox, NSBoxType, NSButton, NSColor, NSControlSize, NSFont, NSLayoutAttribute,
    NSLayoutConstraint, NSLayoutConstraintOrientation, NSLayoutPriorityDefaultHigh,
    NSLayoutRelation, NSSlider, NSStackView, NSStackViewDistribution, NSSwitch, NSTextAlignment,
    NSTextField, NSUserInterfaceLayoutOrientation, NSView,
};
use objc2_foundation::{MainThreadMarker, NSString};

/// A plain, non-editable label.
pub(super) fn label(text: &str, mtm: MainThreadMarker) -> Retained<NSTextField> {
    NSTextField::labelWithString(&NSString::from_str(text), mtm)
}

/// A label in the dimmed weight the design uses for a row's *name* beside a
/// value (`Build`, `Location`, `Licence`) and for a value slot that is
/// reporting rather than offering (`Off in system settings`).
pub(super) fn secondary(text: &str, mtm: MainThreadMarker) -> Retained<NSTextField> {
    let l = label(text, mtm);
    l.setTextColor(Some(&NSColor::secondaryLabelColor()));
    l
}

/// A card's own heading — `Keyboard`, `Shortcuts`.
pub(super) fn heading(text: &str, mtm: MainThreadMarker) -> Retained<NSTextField> {
    let l = label(text, mtm);
    l.setFont(Some(&NSFont::boldSystemFontOfSize(13.0)));
    l
}

/// Body text that wraps.
///
/// **`preferredMaxLayoutWidth` is the whole function.** Without it a
/// word-wrapping `NSTextField` still reports its intrinsic size as ONE LINE,
/// however long — and under autolayout a window sizes itself to its content's
/// minimum, so one 250-character sentence dragged the whole settings window
/// to **1072x1048** when it is meant to be 640x500. Measured 2026-08-16 with
/// `examples/settings_drive.rs`, immediately after the card-constraint fix
/// that made the window's geometry trustworthy enough to read at all.
///
/// The width is the card's interior at the design's window width: 640 less
/// the root stack's 12-a-side inset and the card's 16-a-side padding. It is a
/// *preferred* maximum, so a wider window rewraps.
pub(super) fn wrapping(text: &str, mtm: MainThreadMarker) -> Retained<NSTextField> {
    let l = label(text, mtm);
    {
        l.setLineBreakMode(objc2_app_kit::NSLineBreakMode::ByWordWrapping);
        l.cell().unwrap().setWraps(true);
    }
    l.setFont(Some(&NSFont::systemFontOfSize(11.0)));
    l.setPreferredMaxLayoutWidth(CARD_TEXT_WIDTH);
    // A paragraph must yield horizontally before anything else does: it can
    // answer a narrower window by growing taller, and a check box or a button
    // cannot.
    l.setContentCompressionResistancePriority_forOrientation(
        249.0,
        NSLayoutConstraintOrientation::Horizontal,
    );
    l
}

/// 640 (the design's window width) − 12 × 2 (root inset) − 16 × 2 (card pad).
const CARD_TEXT_WIDTH: f64 = 640.0 - 24.0 - 32.0;

pub(super) fn check(
    title: &str,
    action: Sel,
    target: &AnyObject,
    mtm: MainThreadMarker,
) -> Retained<NSButton> {
    unsafe {
        NSButton::checkboxWithTitle_target_action(
            &NSString::from_str(title),
            Some(target),
            Some(action),
            mtm,
        )
    }
}

pub(super) fn push(
    title: &str,
    action: Sel,
    target: &AnyObject,
    mtm: MainThreadMarker,
) -> Retained<NSButton> {
    let b = unsafe {
        NSButton::buttonWithTitle_target_action(
            &NSString::from_str(title),
            Some(target),
            Some(action),
            mtm,
        )
    };
    b.setBezelStyle(NSBezelStyle::Push);
    b
}

/// The `↗` / `▤` pair beside a file row, and the `⧉` beside an About value.
///
/// Deliberately the same builder for all three: they are the same gesture
/// (act on the thing this row names) at the same size, and giving them one
/// constructor is what keeps them the same size after somebody edits one.
pub(super) fn glyph(
    sym: &str,
    tip: &str,
    action: Sel,
    target: &AnyObject,
    mtm: MainThreadMarker,
) -> Retained<NSButton> {
    let b = push(sym, action, target, mtm);
    b.setControlSize(NSControlSize::Small);
    b.setFont(Some(&NSFont::systemFontOfSize(11.0)));
    b.setToolTip(Some(&NSString::from_str(tip)));
    b
}

/// The System door's on/off control.
///
/// `NSSwitch` rather than a check box because design §3.3 draws a switch and
/// because these rows are *commands to the running service* — pause, and
/// (on Windows) autostart — rather than fields that a Save later commits.
/// A check box beside `Save`-less rows reads as something waiting to be
/// applied.
pub(super) fn switch(action: Sel, target: &AnyObject, mtm: MainThreadMarker) -> Retained<NSSwitch> {
    let s = NSSwitch::new(mtm);
    unsafe {
        s.setTarget(Some(target));
        s.setAction(Some(action));
    }
    s
}

pub(super) fn slider(
    min: f64,
    max: f64,
    action: Sel,
    target: &AnyObject,
    mtm: MainThreadMarker,
) -> Retained<NSSlider> {
    let s = NSSlider::new(mtm);
    unsafe {
        s.setMinValue(min);
        s.setMaxValue(max);
        s.setTarget(Some(target));
        s.setAction(Some(action));
        // Continuous: the window clamps and forwards on every change, and
        // `SetOpacity` is idempotent. A slider that only reported on mouse-up
        // would make the window's own transparency lag the control the user
        // is dragging, which is the one control where the preview IS the
        // feedback.
        s.setContinuous(true);
    }
    s
}

pub(super) fn hstack(views: &[&NSView], mtm: MainThreadMarker) -> Retained<NSStackView> {
    let s = NSStackView::new(mtm);
    s.setOrientation(NSUserInterfaceLayoutOrientation::Horizontal);
    s.setSpacing(8.0);
    s.setDistribution(NSStackViewDistribution::Fill);
    // **A row is as tall as its tallest control and no taller.** Without
    // this, a row inside a vertical `Fill` stack absorbs whatever height the
    // column has spare — measured 2026-08-16: the Shortcuts door's banner
    // row, whose three children are 16 to 24 points high, came back **618
    // points tall** and pushed the window to 1048. Nothing errors; the row is
    // simply enormous and invisible, because it draws nothing of its own.
    s.setHuggingPriority_forOrientation(
        NSLayoutPriorityDefaultHigh,
        NSLayoutConstraintOrientation::Vertical,
    );
    for v in views {
        s.addArrangedSubview(v);
    }
    s
}

pub(super) fn vstack(
    views: &[&NSView],
    spacing: f64,
    mtm: MainThreadMarker,
) -> Retained<NSStackView> {
    let s = NSStackView::new(mtm);
    s.setOrientation(NSUserInterfaceLayoutOrientation::Vertical);
    s.setSpacing(spacing);
    s.setDistribution(NSStackViewDistribution::Fill);
    s.setAlignment(NSLayoutAttribute::Leading);
    // Same rule one axis over: a column hugs its content vertically, so the
    // slack lands where a caller puts it rather than in whichever child
    // happens to resist least.
    s.setHuggingPriority_forOrientation(
        NSLayoutPriorityDefaultHigh,
        NSLayoutConstraintOrientation::Vertical,
    );
    for v in views {
        s.addArrangedSubview(v);
    }
    s
}

/// A view that eats whatever horizontal room is left, so the things after it
/// sit against the trailing edge.
///
/// The design puts a control hard right on most rows (`Pause shortcuts` ——
/// switch). `NSStackView` has no "spring", so this is one: an empty view
/// with a low hugging priority is what expands when the row does.
pub(super) fn spring(mtm: MainThreadMarker) -> Retained<NSView> {
    let v = NSView::new(mtm);
    {
        v.setContentHuggingPriority_forOrientation(1.0, NSLayoutConstraintOrientation::Horizontal);
        v.setContentCompressionResistancePriority_forOrientation(
            1.0,
            NSLayoutConstraintOrientation::Horizontal,
        );
    }
    v
}

/// The rounded ground a door's contents sit on.
///
/// `NSBox` in `Custom` mode rather than a layer-backed `NSView`: it gives
/// `setCornerRadius` / `setFillColor` / `setContentView` without adding
/// `objc2-quartz-core` as a direct dependency, and its fill is an `NSColor`,
/// so it follows the appearance like everything else here.
///
/// ## The content view is pinned by hand, and it has to be
///
/// **Measured 2026-08-16 with `examples/settings_drive.rs`.** Setting
/// `contentView` and turning off its autoresizing mask is not enough:
/// `NSBox` does not create constraints tying the content view's edges to
/// itself, so the content lays out in an unconstrained space and its children
/// keep their natural positions there. The window looked right — it was on
/// screen at the right size, the root stack was correct, and the three hidden
/// doors collapsed as they should — while every control INSIDE a card sat off
/// the top of it:
///
/// ```text
/// content bounds 640x500
/// Add     x=184 y=649   *** OUTSIDE ***
/// Save    x=574 y=12    (command bar: a direct child of the root, so fine)
/// ```
///
/// Nothing reported an error. A click posted at the button's own centre hit
/// `<nothing>`, which is what a control wired to nothing also looks like —
/// the two are distinguishable only by asking `hitTest:` what is actually at
/// the point, which is why the driver prints that on every step.
///
/// The insets live on these constraints rather than on the stack's
/// `edgeInsets` for the same reason: an `edgeInsets` on a stack that is not
/// itself positioned has nothing to inset from.
pub(super) fn card(inner: &NSView, mtm: MainThreadMarker) -> Retained<NSBox> {
    const PAD_X: f64 = 16.0;
    const PAD_Y: f64 = 14.0;

    let b = NSBox::new(mtm);
    {
        b.setBoxType(NSBoxType::Custom);
        b.setTitlePosition(objc2_app_kit::NSTitlePosition::NoTitle);
        b.setFillColor(&NSColor::controlBackgroundColor());
        b.setBorderColor(&NSColor::separatorColor());
        b.setBorderWidth(1.0);
        b.setCornerRadius(8.0);
        b.setContentViewMargins(objc2_foundation::NSSize::new(0.0, 0.0));
        b.setContentView(Some(inner));
    }
    inner.setTranslatesAutoresizingMaskIntoConstraints(false);
    NSLayoutConstraint::activateConstraints(&objc2_foundation::NSArray::from_retained_slice(&[
        inner
            .leadingAnchor()
            .constraintEqualToAnchor_constant(&b.leadingAnchor(), PAD_X),
        inner
            .trailingAnchor()
            .constraintEqualToAnchor_constant(&b.trailingAnchor(), -PAD_X),
        inner
            .topAnchor()
            .constraintEqualToAnchor_constant(&b.topAnchor(), PAD_Y),
        inner
            .bottomAnchor()
            .constraintEqualToAnchor_constant(&b.bottomAnchor(), -PAD_Y),
    ]));
    b
}

/// A horizontal hairline between two groups inside one card.
pub(super) fn divider(mtm: MainThreadMarker) -> Retained<NSBox> {
    let b = NSBox::new(mtm);
    {
        b.setBoxType(NSBoxType::Separator);
    }
    pin_height(&b, 1.0);
    b
}

/// Pin a view to an exact height.
///
/// Used for the hairline and for the list. Everything else is sized by
/// AppKit from its own content, which is the whole reason this file is not
/// the 2 044-line `layout.rs` its Win32 twin needs.
pub(super) fn pin_height(v: &NSView, h: f64) {
    v.setTranslatesAutoresizingMaskIntoConstraints(false);
    unsafe {
        NSLayoutConstraint::constraintWithItem_attribute_relatedBy_toItem_attribute_multiplier_constant(
            v,
            NSLayoutAttribute::Height,
            NSLayoutRelation::Equal,
            None,
            NSLayoutAttribute::NotAnAttribute,
            1.0,
            h,
        )
        .setActive(true);
    }
}

/// Pin a view to a minimum width, so a value slot does not collapse to its
/// text and drag the control after it left on every state change.
pub(super) fn pin_min_width(v: &NSView, w: f64) {
    v.setTranslatesAutoresizingMaskIntoConstraints(false);
    unsafe {
        NSLayoutConstraint::constraintWithItem_attribute_relatedBy_toItem_attribute_multiplier_constant(
            v,
            NSLayoutAttribute::Width,
            NSLayoutRelation::GreaterThanOrEqual,
            None,
            NSLayoutAttribute::NotAnAttribute,
            1.0,
            w,
        )
        .setActive(true);
    }
}

/// A right-aligned value slot — the `96%`, the `248 bytes`, the config path.
pub(super) fn value(text: &str, mtm: MainThreadMarker) -> Retained<NSTextField> {
    let l = secondary(text, mtm);
    l.setAlignment(NSTextAlignment::Right);
    l
}
