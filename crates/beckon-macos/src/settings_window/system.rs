//! Door 3 — **System**.
//!
//! Design §3.3. This door writes neither `apps.toml` nor anything that a
//! `Save` later commits: every control here acts *now*, on the running
//! service or on this window. That is design §1's split by STORE, and it is
//! why `command_bar_shown` draws no `Save` on this page — see
//! `chrome::command_bar`.
//!
//! ## Two rows the Windows twin has and this one does not
//!
//! **`Dark mode` is omitted.** Not greyed, not stubbed — absent. The Win32
//! twin needs it because Win32 has no appearance to follow: it carries
//! `theme::pairs`, a `prefs.rs` registry value and a repaint path to do it.
//! Every colour in this window is an AppKit *semantic* colour, so the whole
//! thing follows the system between light and dark with no control, no
//! stored preference and no code. `SystemState::dark` is therefore read and
//! discarded here, which is the honest thing for a field whose question this
//! platform already answers.
//!
//! **`Start at login` is omitted**, and the reasoning is the design's own,
//! copied rather than re-derived: *a capability this process does not have
//! is left out, because a greyed row asks "why is this greyed?" with no
//! answer available in the row itself.* beckon does not own the macOS
//! launch item — the Homebrew formula's `service do` block does, and
//! `brew services start beckon` is the whole install. A switch here would
//! be a second writer for a file beckon did not create, and the two would
//! disagree the first time a user ran `brew services stop`.
//!
//! `SystemState::autostart` is `None` on this platform, which is the field's
//! documented way of saying exactly that — so the omission is expressed in
//! the crate all three CI jobs compile, not in this file's layout.

use beckon_core::settings::{SystemState, Transparency};
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::sel;
use objc2_app_kit::{NSButton, NSSlider, NSStackView, NSSwitch, NSTextField, NSView};
use objc2_foundation::{MainThreadMarker, NSString};

use super::widgets as w;

/// Every control on this door.
///
/// `Clone` for the reason `Controls` is: a handle is reached only through a
/// function that has already dropped the `RefCell` borrow, and a `Retained`
/// clone is one retain.
#[derive(Clone)]
pub(super) struct SystemControls {
    pub(super) pause: Retained<NSSwitch>,
    pub(super) opacity: Retained<NSSlider>,
    pub(super) opacity_value: Retained<NSTextField>,
    /// The whole transparency line, hidden when the OS refuses transparency
    /// outright — see `apply`.
    pub(super) opacity_row: Retained<NSStackView>,
    pub(super) config_value: Retained<NSTextField>,
    pub(super) log_name: Retained<NSTextField>,
    pub(super) log_value: Retained<NSTextField>,
    /// `serve` ran without `--log`, so there is no log file and a path would
    /// be a lie. Hidden rather than blank.
    pub(super) log_row: Retained<NSStackView>,
    pub(super) log_open: Retained<NSButton>,
    pub(super) log_reveal: Retained<NSButton>,
}

pub(super) fn build(
    target: &AnyObject,
    mtm: MainThreadMarker,
) -> (Retained<NSView>, SystemControls) {
    // --- group 1: the service ------------------------------------------------
    let pause = w::switch(sel!(beckonPause:), target, mtm);
    let pause_row = w::form_row(
        &w::labelled(
            "Shortcuts on",
            Some("Option-click the menu bar icon to toggle."),
            mtm,
        ),
        &pause,
        mtm,
    );

    let reload = w::push("Reload", sel!(beckonReloadNow:), target, mtm);
    let reload_row = w::form_row(&w::labelled("Configuration", None, mtm), &reload, mtm);

    let g_service = w::group(&[&*pause_row, &*reload_row], mtm);

    // --- group 2: the files ----------------------------------------------------
    let config_value = w::value("", mtm);
    let config_open = w::glyph(
        "Open",
        "Open this file",
        sel!(beckonOpenConfig:),
        target,
        mtm,
    );
    let config_reveal = w::glyph(
        "Reveal",
        "Show in Finder",
        sel!(beckonRevealConfig:),
        target,
        mtm,
    );
    let config_tail = w::hstack(
        &[&*config_value as &NSView, &config_open, &config_reveal],
        mtm,
    );
    let config_row = w::form_row(&w::labelled("Config file", None, mtm), &config_tail, mtm);

    // **The row's title stays the live filename, not a static caption.**
    // `beckon_core::settings::FileRow`'s own doc says "the name IS the
    // label" -- design §3.3 -- so unlike `Config file` above (which was
    // already a fixed `"apps.toml"` on this platform, never `st.config`),
    // this title is `log_name`, the same field `apply` has always pushed
    // `st.log.name` into. Wrapping it in a static `Log` caption would orphan
    // that push onto a field the window no longer draws.
    let log_name = w::label("", mtm);
    let log_value = w::value("", mtm);
    let log_open = w::glyph("Open", "Open this file", sel!(beckonOpenLog:), target, mtm);
    let log_reveal = w::glyph(
        "Reveal",
        "Show in Finder",
        sel!(beckonRevealLog:),
        target,
        mtm,
    );
    let log_tail = w::hstack(&[&*log_value as &NSView, &log_open, &log_reveal], mtm);
    let log_row = w::form_row(&log_name, &log_tail, mtm);
    // Hidden until a push says otherwise. `apply` hides it whenever `serve`
    // ran without `--log`, but the window is on screen before the first push
    // — and an empty row carrying `Open` and `Reveal` beside no file name is
    // exactly what that gap looked like. Photographed 2026-08-16,
    // `macos-door-system.png`.
    log_row.setHidden(true);

    let g_files = w::group(&[&*config_row, &*log_row], mtm);

    // --- group 3: this window --------------------------------------------------
    // **Built from `OPACITY_DEFAULT`, never spelled.** The literal here was
    // `"100%"`, and it is the string a row that never got a push keeps —
    // which is how "beckon defaults to 100%" became a thing two readers
    // believed and grep confirmed, while the actual default is 96 and an
    // unpushed slider reads 85 (measured: `setMaxValue(100)` then
    // `setMinValue(85)` leaves `doubleValue` at 85, knob hard left). A
    // placeholder that cannot disagree with the default cannot tell that lie.
    let opacity_value = w::value(
        &beckon_core::settings::opacity_label(beckon_core::settings::OPACITY_DEFAULT),
        mtm,
    );
    w::pin_min_width(&opacity_value, 44.0);
    let opacity = w::slider(
        beckon_core::settings::OPACITY_MIN as f64,
        beckon_core::settings::OPACITY_MAX as f64,
        sel!(beckonOpacity:),
        target,
        mtm,
    );
    w::pin_min_width(&opacity, 160.0);
    let opacity_tail = w::hstack(&[&*opacity_value as &NSView, &opacity], mtm);
    let opacity_row = w::form_row(
        &w::labelled("Window transparency", None, mtm),
        &opacity_tail,
        mtm,
    );

    let g_look = w::group(&[&*opacity_row], mtm);

    let page = w::vstack(&[&*g_service, &*g_files, &*g_look], 12.0, mtm);
    let view: Retained<NSView> = page.into_super();

    (
        view,
        SystemControls {
            pause,
            opacity,
            opacity_value,
            opacity_row,
            config_value,
            log_name,
            log_value,
            log_row,
            log_open,
            log_reveal,
        },
    )
}

/// Draw `st`.
///
/// **Called with the `RefCell` borrow already dropped**, like every other
/// rendering path in this module — the controls arrive as a clone. See
/// `super::controls`.
pub(super) fn apply(c: &SystemControls, st: &SystemState) {
    c.pause.setState(if st.paused { 1 } else { 0 });

    match st.transparency {
        Transparency::On(pct) => {
            c.opacity_row.setHidden(false);
            c.opacity.setEnabled(true);
            c.opacity.setDoubleValue(pct as f64);
            c.opacity_value.setStringValue(&NSString::from_str(
                &beckon_core::settings::opacity_label(pct),
            ));
        }
        Transparency::Off(block) => {
            // The row stays on screen and says WHY. `reason()` names the
            // cause rather than the effect, so the reader is not sent
            // looking for a switch that this window does not own.
            c.opacity_row.setHidden(false);
            c.opacity.setEnabled(false);
            c.opacity_value.setStringValue(&NSString::from_str(
                block.reason_with(beckon_core::theme::BlockReasons::MAC),
            ));
        }
    }

    c.config_value
        .setStringValue(&NSString::from_str(&st.config.value));

    match &st.log {
        Some(f) => {
            c.log_row.setHidden(false);
            c.log_name.setStringValue(&NSString::from_str(&f.name));
            c.log_value.setStringValue(&NSString::from_str(&f.value));
            c.log_open.setEnabled(true);
            c.log_reveal.setEnabled(true);
        }
        None => c.log_row.setHidden(true),
    }
}
