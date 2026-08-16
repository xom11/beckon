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
    NSLayoutConstraint, NSLayoutConstraintOrientation, NSLayoutRelation, NSSlider, NSStackView,
    NSStackViewDistribution, NSSwitch, NSTextAlignment, NSTextField,
    NSUserInterfaceLayoutOrientation, NSView,
};
use objc2_foundation::{MainThreadMarker, NSEdgeInsets, NSString};

/// A plain, non-editable label.
pub(super) fn label(text: &str, mtm: MainThreadMarker) -> Retained<NSTextField> {
    NSTextField::labelWithString(&NSString::from_str(text), mtm)
}

/// A label in the dimmed weight the design uses for a row's *name* beside a
/// value (`Build`, `Location`, `Licence`) and for a value slot that is
/// reporting rather than offering (`Off in system settings`).
pub(super) fn secondary(text: &str, mtm: MainThreadMarker) -> Retained<NSTextField> {
    let l = label(text, mtm);
    unsafe { l.setTextColor(Some(&NSColor::secondaryLabelColor())) };
    l
}

/// A card's own heading — `Keyboard`, `Shortcuts`.
pub(super) fn heading(text: &str, mtm: MainThreadMarker) -> Retained<NSTextField> {
    let l = label(text, mtm);
    l.setFont(Some(&NSFont::boldSystemFontOfSize(13.0)));
    l
}

/// Body text that wraps — the About door's disclosure is the only caller,
/// and it is the only string in the window long enough to need it.
pub(super) fn wrapping(text: &str, mtm: MainThreadMarker) -> Retained<NSTextField> {
    let l = label(text, mtm);
    unsafe {
        l.setLineBreakMode(objc2_app_kit::NSLineBreakMode::ByWordWrapping);
        l.cell().unwrap().setWraps(true);
    }
    l.setFont(Some(&NSFont::systemFontOfSize(11.0)));
    l
}

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
    unsafe { b.setToolTip(Some(&NSString::from_str(tip))) };
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
    unsafe { s.setAlignment(NSLayoutAttribute::Leading) };
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
    unsafe {
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
pub(super) fn card(inner: &NSView, mtm: MainThreadMarker) -> Retained<NSBox> {
    let b = NSBox::new(mtm);
    unsafe {
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
    if let Some(stack) = inner.downcast_ref::<NSStackView>() {
        stack.setEdgeInsets(NSEdgeInsets {
            top: 14.0,
            left: 16.0,
            bottom: 14.0,
            right: 16.0,
        });
    }
    b
}

/// A horizontal hairline between two groups inside one card.
pub(super) fn divider(mtm: MainThreadMarker) -> Retained<NSBox> {
    let b = NSBox::new(mtm);
    unsafe {
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
    unsafe { l.setAlignment(NSTextAlignment::Right) };
    l
}
