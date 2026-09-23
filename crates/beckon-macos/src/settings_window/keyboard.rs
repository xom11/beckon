//! Door 2 — **Keyboard**.
//!
//! Design §5.2/§5.6: a grouped form, three rounded cards rather than the one
//! card three groups used to share:
//!
//! ```text
//! ┌ Use Caps Lock as a shortcut key ─────────────────────── ( ●══) ┐
//! │ While held, Caps Lock presses ────────── [ ]Ctrl [ ]Cmd [ ]Opt │
//! │ When tapped alone ───────────────────────── [ Caps Lock    ▾ ] │
//! └──────────────────────────────────────────────────────────────┘
//! ┌ Show shortcuts as Caps ─────────────────────────────── ( ●══) ┐
//! └──────────────────────────────────────────────────────────────┘
//! ┌ Input Monitoring ──────────────── [ Open Input Monitoring    ] │
//! └──────────────────────────────────────────────────────────────┘
//! ```
//!
//! **No card heading.** The Win32 twin's `Keyboard` group label was deleted
//! and its id retired: it drew the word `Keyboard` directly beneath a tab
//! captioned `Keyboard`. Design §7's rule — *a group heading is the word
//! every row beneath it does not repeat* — and the same reason the Shortcuts
//! door lost its own heading.
//!
//! ## All three groups are live on macOS
//!
//! Groups 1 and 2 drive `beckon_macos::caps_tap`, the `CGEventTap` twin of
//! the Windows `WH_KEYBOARD_LL` hook — measured end to end in
//! `examples/caps_live.rs`, with the tap uninstalled as the control:
//!
//! ```text
//! off : hotkey fired = false
//! on  : HOTKEY FIRED
//! ```
//!
//! Group 3 is a view preference about the list and touches nothing else.
//!
//! The note under group 1 is about the PERMISSION, which is the one thing a
//! reader cannot discover by trying: Input Monitoring is separate from
//! Accessibility and its absence is silent.

use beckon_core::settings::ControlState;
use beckon_core::shortcuts::CapsTap;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::sel;
use objc2_app_kit::{NSButton, NSPopUpButton, NSStackView, NSSwitch, NSTextField, NSView};
use objc2_foundation::{MainThreadMarker, NSString};

use super::widgets as w;

#[derive(Clone)]
pub(super) struct KeyboardControls {
    pub(super) caps: Retained<NSSwitch>,
    pub(super) hold_ctrl: Retained<NSButton>,
    pub(super) hold_super: Retained<NSButton>,
    pub(super) hold_alt: Retained<NSButton>,
    pub(super) tap: Retained<NSPopUpButton>,
    /// Group 3. A **view** preference: it changes the list cell and nothing
    /// else. `apps.toml` is byte-identical between a machine with it on and
    /// one without, which is why it is stored beside the window's own look
    /// rather than in the config.
    pub(super) shorthand: Retained<NSSwitch>,
    /// The row of controls group 2 owns, disabled together with the switch.
    pub(super) hold_row: Retained<NSStackView>,
    pub(super) note: Retained<NSTextField>,
}

/// What ticking the box actually costs, in one sentence — **or nothing at
/// all when the grant is already there.**
///
/// **It replaced a note saying the feature did not exist here.** It does now
/// (`beckon_macos::caps_tap`, end-to-end in `examples/caps_live.rs`), and
/// what a reader needs instead is the permission it asks for — because
/// Input Monitoring is a *separate* grant from Accessibility, in a separate
/// System Settings pane, and without it the tap is created successfully and
/// then receives nothing at all. That failure is silent, so the sentence
/// exists to make it findable before it happens rather than after.
///
/// **It used to say that unconditionally, and that is the bug this asks the
/// OS to avoid.** Measured on macmini 2026-09-18, fresh Homebrew install,
/// Accessibility granted and no `kTCCServiceListenEvent` row in TCC at all:
/// chord capture recorded `ctrl+shift+F9` on the first attempt, so
/// `install_for` had already passed the `input_monitoring_granted()` gate —
/// while this sentence three rows above the switch still said the Caps key
/// could not be read. A warning that is always on screen is read once and
/// then stops being read, and this one was false exactly when someone was
/// deciding whether to turn the feature on.
///
/// The wording lives in `beckon_core::settings::input_monitoring_warning`,
/// with the rest of the status vocabulary; this asks the OS and hands the
/// answer over.
fn caps_note() -> &'static str {
    beckon_core::settings::input_monitoring_warning(crate::caps_tap::input_monitoring_granted())
        .unwrap_or("")
}

pub(super) fn build(
    target: &AnyObject,
    mtm: MainThreadMarker,
) -> (Retained<NSView>, KeyboardControls) {
    // --- group 1: Caps Lock as the beckon key -------------------------------
    let caps = w::switch(sel!(beckonCaps:), target, mtm);
    let caps_row = w::form_row(
        &w::labelled(
            "Use Caps Lock as a shortcut key",
            Some("Hold Caps Lock and press a key instead of the chord below."),
            mtm,
        ),
        &caps,
        mtm,
    );

    // **Three chips and there must never be a fourth.** `Chord` has exactly
    // `ctrl` / `super_` / `alt`, because the alias has to RELEASE whatever it
    // presses, and releasing Shift under the user's fingers makes everything
    // they type next arrive lowercase. The spec sketches four; the type is
    // right and the sketch is wrong.
    let hold_ctrl = w::check("Ctrl", sel!(beckonHold:), target, mtm);
    let hold_super = w::check("Cmd", sel!(beckonHold:), target, mtm);
    let hold_alt = w::check("Option", sel!(beckonHold:), target, mtm);
    let mods_row = w::hstack(&[&*hold_ctrl as &NSView, &hold_super, &hold_alt], mtm);
    let hold_row = w::form_row(
        &w::labelled("While held, Caps Lock presses", None, mtm),
        &mods_row,
        mtm,
    );

    let tap = NSPopUpButton::new(mtm);
    unsafe {
        tap.setTarget(Some(target));
        tap.setAction(Some(sel!(beckonTap:)));
    }
    // **Order IS the `CapsTap` mapping, and it is read and written by INDEX,
    // never by text.** Even a closed list has typeahead, which moves the
    // selection; matching on the visible string would then write whatever the
    // user's last keystroke happened to land on.
    for t in ["Caps Lock", "Escape", "Nothing"] {
        tap.addItemWithTitle(&NSString::from_str(t));
    }
    let tap_row = w::form_row(&w::labelled("When tapped alone", None, mtm), &tap, mtm);

    let g_caps = w::group(&[&*caps_row, &*hold_row, &*tap_row], mtm);

    // --- group 2: how the list writes a bound chord -------------------------
    let shorthand = w::switch(sel!(beckonShorthand:), target, mtm);
    let shorthand_row = w::form_row(
        &w::labelled("Show shortcuts as Caps", None, mtm),
        &shorthand,
        mtm,
    );
    let g_view = w::group(&[&*shorthand_row], mtm);

    // --- group 3: the grant this page's feature needs -----------------------
    //
    // **A button, because the sentence names a pane four clicks deep.**
    // Every one of those clicks is a chance to land in Accessibility instead
    // -- the neighbouring row, the permission this is most often confused
    // with, and the one that is usually already granted. The sentence stays:
    // it is the half that says *why*, and a reader who has the pane open
    // still has to know which switch and that it is not Accessibility.
    //
    // It is drawn unconditionally rather than only while the grant is
    // missing. Revoking Input Monitoring does not notify this process, so a
    // button that appears on a state we cannot observe would be absent
    // exactly when it is needed; and a reader who wants to CHECK the switch
    // has the same errand as one who needs to grant it.
    //
    // **Built by hand rather than through `widgets::labelled`.** `labelled`
    // manufactures its OWN secondary `NSTextField` and hands back only the
    // assembled stack, so there is no way to reach back into it from here --
    // and `apply` has to keep re-asking `IOHIDCheckAccess` into this exact
    // field every time the window redraws (the grant can arrive mid-session).
    // `labelled`'s secondary line is also a plain non-wrapping `secondary()`
    // label; this sentence runs past 250 characters, and a non-wrapping field
    // that long either clips at the card edge or drags the window wide --
    // the same class of defect `widgets::wrapping`'s own doc records on the
    // other axis. So this row keeps building its note with `w::wrapping`,
    // arranged in the same title-then-note shape `labelled` uses, instead of
    // calling `labelled` itself.
    let note = w::wrapping(caps_note(), mtm);
    let im_label = w::vstack(
        &[&*w::label("Input Monitoring", mtm) as &NSView, &*note],
        2.0,
        mtm,
    );
    let open_im = w::push(
        "Open Input Monitoring",
        sel!(beckonOpenInputMonitoring:),
        target,
        mtm,
    );
    let im_row = w::form_row(&im_label, &open_im, mtm);
    let g_grant = w::group(&[&*im_row], mtm);

    let page = w::vstack(&[&*g_caps, &*g_view, &*g_grant], 12.0, mtm);
    // **Every direct child of a `Width`-aligned column needs its own pin.**
    // `vstack`'s `setAlignment(Width)` does not stretch children to the
    // column's width -- see `widgets::pin_width_to`'s doc, and `mod.rs`'s
    // own root loop, which pins each door to `root` for the identical
    // reason. Without this the three cards here came out ragged: `g_caps`
    // (the one card whose first row is a two-line label) sized itself to
    // that label's intrinsic width and sat trailing-aligned around x=427 in
    // a 640pt window, while `g_view` and `g_grant` (whose only rows are one
    // line) happened to size close enough to full width to look right by
    // accident. Photographed 2026-09-23, `Keyboard.png`.
    for grp in [&g_caps, &g_view, &g_grant] {
        w::pin_width_to(grp, &page, 0.0);
    }
    let view: Retained<NSView> = page.into_super();

    (
        view,
        KeyboardControls {
            caps,
            hold_ctrl,
            hold_super,
            hold_alt,
            tap,
            shorthand,
            hold_row,
            note,
        },
    )
}

pub(super) fn apply(c: &KeyboardControls, st: &ControlState, shorthand_on: bool) {
    c.caps.setState(if st.caps_checked { 1 } else { 0 });
    super::set_check(&c.hold_ctrl, st.caps_hold.ctrl);
    super::set_check(&c.hold_super, st.caps_hold.super_);
    super::set_check(&c.hold_alt, st.caps_hold.alt);
    c.tap.selectItemAtIndex(match st.caps_tap {
        CapsTap::CapsLock => 0,
        CapsTap::Escape => 1,
        CapsTap::None => 2,
    });
    c.shorthand.setState(if shorthand_on { 1 } else { 0 });

    // **Re-asked here, not just at `build`.** The grant can arrive while this
    // window is open -- `Open Input Monitoring` is one click away and the
    // whole point of it -- and `build` runs once per window. Without this the
    // sentence a user just acted on stays on screen contradicting them until
    // beckon is restarted, which reads as the click having done nothing.
    //
    // `input_monitoring_granted` is `IOHIDCheckAccess`, which only READS an
    // answer: it never raises a dialog, so calling it on every `apply` costs
    // a syscall and cannot surprise anybody with a panel.
    c.note
        .setStringValue(&objc2_foundation::NSString::from_str(caps_note()));

    // Group 2 follows group 1: `Hold` and `Tap` are answers to the question
    // group 1 asks, and they mean nothing while it is off.
    //
    // The note does NOT follow it. It explains why the whole card is inert on
    // this platform, which is exactly the thing a reader needs while the
    // switch is off and they are wondering whether turning it on would help.
    let live = st.caps_checked;
    for v in [&c.hold_ctrl, &c.hold_super, &c.hold_alt] {
        v.setEnabled(live);
    }
    c.tap.setEnabled(live);
    let _ = &c.hold_row;
}
