//! Door 2 — **Keyboard**.
//!
//! Design §3.2, three groups in one card, hairlines between:
//!
//! ```text
//! Use Caps Lock as a shortcut key                       ( ●══)
//! ──────────────────────────────────────────────────────────
//! Hold [ ]Ctrl [ ]Cmd [ ]Option          Tap [ Caps Lock  ▾ ]
//! ──────────────────────────────────────────────────────────
//! Write shortcuts as Caps instead of Ctrl + Cmd + Option ( ●══)
//! ```
//!
//! **No card heading.** The Win32 twin's `Keyboard` group label was deleted
//! and its id retired: it drew the word `Keyboard` directly beneath a tab
//! captioned `Keyboard`. Design §7's rule — *a group heading is the word
//! every row beneath it does not repeat* — and the same reason the Shortcuts
//! door lost its own heading.
//!
//! ## What this door does NOT do on macOS yet
//!
//! Nothing here is wired to a key. The Caps Lock shorthand needs a
//! `CGEventTap` — macOS's counterpart to the `WH_KEYBOARD_LL` hook — and it
//! is not built. So group 1 and group 2 edit real settings in `apps.toml`
//! that **this platform currently ignores**, while group 3 is live, because
//! it is a view preference about the list and nothing else.
//!
//! That asymmetry is stated rather than hidden: `caps_note()` is drawn under
//! the first group and says so. The alternative — omitting the two groups
//! until the tap lands — was rejected because the file is shared across
//! machines: a person editing on a Mac and running on a PC is a real case,
//! and a window that silently refuses to show settings the file already
//! contains is worse than one that shows them and says where they take
//! effect.

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

/// Why the first two groups do not do anything on this platform yet.
///
/// One sentence, and it names the cause rather than the effect: a reader who
/// ticks a box and sees nothing happen needs to know whether they are looking
/// at a bug, a missing permission, or an unbuilt feature — and those have
/// three different next actions.
fn caps_note() -> &'static str {
    "Caps Lock shorthand is not active on macOS yet -- it needs a keyboard event tap, \
     which beckon does not install. These two settings are saved to the config file and \
     honoured by beckon on Windows."
}

pub(super) fn build(
    target: &AnyObject,
    mtm: MainThreadMarker,
) -> (Retained<NSView>, KeyboardControls) {
    // --- group 1: the arming switch ---------------------------------------
    //
    // A label plus `NSSwitch`, full width with the track on the card's right
    // edge, so this row and group 3's line up with each other and with the
    // System door's. It is a SENTENCE, not a key, which is why it is not
    // drawn as a keycap on either platform.
    let caps = w::switch(sel!(beckonCaps:), target, mtm);
    let caps_row = w::hstack(
        &[
            &*w::label("Use Caps Lock as a shortcut key", mtm) as &NSView,
            &*w::spring(mtm),
            &caps,
        ],
        mtm,
    );

    let note = w::wrapping(caps_note(), mtm);

    // --- group 2: Hold chips and the Tap list, one line --------------------
    //
    // **Three chips and there must never be a fourth.** `Chord` has exactly
    // `ctrl` / `super_` / `alt`, because the alias has to RELEASE whatever it
    // presses, and releasing Shift under the user's fingers makes everything
    // they type next arrive lowercase. The spec sketches four; the type is
    // right and the sketch is wrong.
    let hold_ctrl = w::check("Ctrl", sel!(beckonHold:), target, mtm);
    let hold_super = w::check("Cmd", sel!(beckonHold:), target, mtm);
    let hold_alt = w::check("Option", sel!(beckonHold:), target, mtm);

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

    let hold_row = w::hstack(
        &[
            &*w::label("Hold", mtm) as &NSView,
            &hold_ctrl,
            &hold_super,
            &hold_alt,
            &*w::spring(mtm),
            &*w::label("Tap", mtm),
            &tap,
        ],
        mtm,
    );

    // --- group 3: the view preference --------------------------------------
    let shorthand = w::switch(sel!(beckonShorthand:), target, mtm);
    let shorthand_row = w::hstack(
        &[
            &*w::label(
                "Write shortcuts as Caps instead of Ctrl + Cmd + Option",
                mtm,
            ) as &NSView,
            &*w::spring(mtm),
            &shorthand,
        ],
        mtm,
    );

    let inner = w::vstack(
        &[
            &*caps_row as &NSView,
            &note,
            &w::divider(mtm),
            &hold_row,
            &w::divider(mtm),
            &shorthand_row,
        ],
        10.0,
        mtm,
    );

    // Same as About's disclosure: a wrapping label is the child a
    // `Width`-aligned column leaves at its own width.
    w::pin_width_to(&note, &inner, 0.0);

    let view: Retained<NSView> = w::card(&inner, mtm).into_super();

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
