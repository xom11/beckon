//! The macOS settings window — the counterpart of
//! `beckon_windows::settings_window`, against the same contract.
//!
//! It renders a `beckon_core::settings::ControlState` and raises a
//! `beckon_core::settings::Callbacks`. It never reads `Model`, never decides
//! what a row means, and never decides whether Save is legal — every such
//! question is already answered in `beckon-core`, where all three CI jobs
//! compile the answer.
//!
//! ## Why this file is not five thousand lines
//!
//! The Win32 twin spends most of its size on layout arithmetic and on
//! defending against its own layout pass. `NSStackView` removes both:
//! bands stack, AppKit sizes them, and **there is no `layout` call on a data
//! push**. That matters more than it sounds. The measured data-loss bug on
//! Windows — typing "Notepad" into the App field and having the model
//! receive `"d"` — was not a combo box defect: `apply_state` ended by
//! calling `layout`, whose `SetWindowPos` made a populated combo
//! re-synchronise its edit and select the whole string, so the next
//! character replaced all of it. Nothing here calls a layout pass on a data
//! push, so that class of bug is structurally absent rather than guarded
//! against.
//!
//! ## What is still the same, and must stay the same
//!
//! - **The list maps view rows to model rows.** `NSTableView` only knows a
//!   position in the filtered list it was given; `on_select` and `on_mark`
//!   take model indices. Every callback goes through `items[i].row`.
//!   Without that, one filtered keystroke ticks one binding and deletes
//!   another.
//! - **The shortcut is check boxes plus a closed key list**, never a text
//!   field, and the key list is filled from `key_table()` in order and never
//!   sorted: `ComboView::key` IS the index, so a sorted list would silently
//!   write a key the user did not choose.
//! - **Edits are compared as `ComboView`s, not as strings.** `Combo::parse`
//!   accepts free modifier order while this window rebuilds canonically, so
//!   a string compare would make `"super+ctrl+alt+t"` look like an edit and
//!   mark a file dirty that nobody touched.
//!
//! ## Not verified
//!
//! Nothing in this file has been seen on screen. See `src/tray.rs`'s module
//! doc: AppKit hands back live objects in a session with no window server,
//! so compiling and even constructing successfully proves nothing about
//! what is drawn. `examples/settings_probe.rs` is the only thing that can
//! answer that, and it has to run in an Aqua session.

use beckon_core::settings::{
    command_bar_shown, copy_text, Callbacks, ControlState, Field, Mark, Page, Paths,
    SettingsCommand,
};
// `beckon_core::settings::Target` names a link destination; `Target` in this
// file is the Objective-C class every control sends its action to. Aliasing
// the import rather than renaming the class keeps the two `sel!` tables and
// the two macOS probes reading the way they already do.
use beckon_core::settings::Target as SettingsTarget;
use beckon_core::shortcuts::{combo_view, key_table, CapsTap, Chord, ComboView};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject, Sel};
use objc2::{define_class, msg_send, sel, MainThreadOnly};
use objc2_app_kit::{
    NSBackingStoreType, NSBezelStyle, NSButton, NSComboBox, NSControlTextEditingDelegate, NSFont,
    NSLayoutAttribute, NSLayoutConstraint, NSPasteboard, NSPasteboardTypeString, NSPopUpButton,
    NSScrollView, NSSegmentedControl, NSStackView, NSStackViewDistribution, NSTableColumn,
    NSTableView, NSTableViewDataSource, NSTableViewDelegate, NSTextField,
    NSUserInterfaceLayoutOrientation, NSView, NSWindow, NSWindowStyleMask,
};
use objc2_foundation::{
    MainThreadMarker, NSNotification, NSObject, NSObjectProtocol, NSPoint, NSRect, NSSize, NSString,
};
use std::cell::RefCell;

// Construction helpers shared by the four doors. Nothing in there decides
// anything — see its module doc.
#[allow(dead_code)]
mod about;
#[allow(dead_code)]
mod keyboard;
#[allow(dead_code)]
mod system;
#[allow(dead_code)]
mod widgets;

/// How many rows the list shows, at every scale. Fixed rather than grown
/// from the config, for the same reason the Win32 twin fixes it: a window
/// that changes height when a binding is added is a window that moves under
/// the pointer mid-edit.
/// The design's window, in points.
///
/// `MIN_HEIGHT` is the Win32 twin's derivation carried over unchanged, and
/// its subject is the About door rather than the Shortcuts one: card 1's
/// list gives room up before anything else moves, so the door that runs out
/// first is one of the three whose card is FIXED, and About is the only page
/// whose height depends on a text measurement.
const WINDOW_WIDTH: f64 = 640.0;
const WINDOW_HEIGHT: f64 = 500.0;
const MIN_HEIGHT: f64 = 480.0;

const ROWS: f64 = 8.0;
const ROW_HEIGHT: f64 = 20.0;

// Column identifiers. Strings rather than integers because that is what
// AppKit's data source callbacks carry, and matching on them keeps the
// column order a layout question rather than a protocol one.
const COL_TICK: &str = "tick";
const COL_APP: &str = "app";
const COL_COMBO: &str = "combo";
const COL_STATUS: &str = "status";

/// Every control handle, and nothing else.
///
/// Split out from `Ui` and `Clone` (a `Retained` clone is one retain) so
/// that `controls()` can hand them out with the `RefCell` borrow ALREADY
/// released. That is not tidiness -- see `controls()`.
#[derive(Clone)]
struct Controls {
    window: Retained<NSWindow>,

    /// The four doors' pill strip.
    ///
    /// **`NSSegmentedControl`, not four hand-drawn pills.** The Win32 twin
    /// draws its own because Win32 has no such control, and it pays for it:
    /// three `(fill, ink)` pairs, a hover ink swap that exists because
    /// `text_muted` on `strip_hover` measures 3.700 and fails WCAG, a focus
    /// ring drawn OUTSIDE the pill in a 3 px margin because the borrowed
    /// `accent_on` measured 1.360 on the lit pill and was invisible, and a
    /// fixed four-digit badge slot so the strip's width never becomes a
    /// function of the data. Every one of those is contrast or geometry
    /// AppKit already gets right, in both appearances, with a keyboard story
    /// and a focus ring included.
    ///
    /// It also closes a deviation rather than inheriting one: the design's
    /// own drawing shrink-wraps the trough around the four pills — the
    /// segmented-control look — and Windows fills the whole band instead,
    /// recorded as a deferred difference, because hugging needs a width only
    /// its layout pass computes.
    tabs: Retained<NSSegmentedControl>,
    /// One container per door, in `Page` order. Exactly one is unhidden.
    /// Hidden arranged subviews collapse in an `NSStackView`, so the door
    /// that is not open contributes no height — the AppKit spelling of
    /// `compute_card_rects` being page-dependent.
    pages: [Retained<NSView>; 4],
    /// `Serving · N of M` / `Paused` / `Not serving`.
    ///
    /// Lives in the command bar and is drawn on **all four** doors, which is
    /// the whole reason `show_page` hides the three BUTTONS rather than the
    /// band that holds them.
    service: Retained<NSTextField>,
    kbd: keyboard::KeyboardControls,
    sys: system::SystemControls,
    abt: about::AboutControls,

    table: Retained<NSTableView>,
    filter: Retained<NSTextField>,
    app: Retained<NSComboBox>,
    key: Retained<NSPopUpButton>,
    mod_ctrl: Retained<NSButton>,
    mod_super: Retained<NSButton>,
    mod_alt: Retained<NSButton>,
    mod_shift: Retained<NSButton>,
    /// `Record` / `Stop` -- one button wearing two captions, exactly as
    /// `IDC_RECORD` does on Windows. The caption IS the state, which is
    /// why `end_capture` has to restore it from every exit path.
    record: Retained<NSButton>,
    notes: Retained<NSTextField>,
    /// The row, not just its contents.
    ///
    /// **Hiding the three children is not enough**: the `NSStackView` that
    /// holds them kept 70 points of height with all of them hidden, so every
    /// Shortcuts screenshot had a band of empty window between the tab strip
    /// and the first card. Photographed 2026-08-16. `banner_row` was not in
    /// this struct at all, which is why nothing could hide it.
    banner_row: Retained<NSStackView>,
    banner: Retained<NSTextField>,
    banner_reload: Retained<NSButton>,
    banner_keep: Retained<NSButton>,
    save: Retained<NSButton>,
    close_btn: Retained<NSButton>,
    open_file: Retained<NSButton>,
    remove: Retained<NSButton>,
    add: Retained<NSButton>,
}

/// The four doors, in `Page` order.
///
/// `Page` is an exhaustive enum in core and `Page::next` / `Page::prev`
/// already spell the cycle, so this is only the index — never a second
/// spelling of the order.
fn page_index(p: Page) -> usize {
    match p {
        Page::Shortcuts => 0,
        Page::Keyboard => 1,
        Page::System => 2,
        Page::About => 3,
    }
}

fn page_at(i: usize) -> Page {
    match i {
        1 => Page::Keyboard,
        2 => Page::System,
        3 => Page::About,
        _ => Page::Shortcuts,
    }
}

struct Ui {
    c: Controls,
    _target: Retained<Target>,

    /// The last state pushed. Callbacks need `items[i].row` to map a view
    /// row to a model row, and the delegate has no other way to reach it.
    items: Vec<beckon_core::settings::ListItem>,
    /// The combo the fields were last *given*, so an edit can be compared
    /// against what is stored rather than against the previous keystroke.
    shown_combo: Option<String>,
    /// True while `apply_state` is writing controls. Every notification
    /// consults it: AppKit raises the same actions for a programmatic write
    /// as for a human one, and a push that came back as an edit would make
    /// every open mark the file dirty.
    pushing: bool,

    /// Which door is open. The caller keeps its own mirror (it decides where
    /// the NEXT open lands) and is told through `SettingsCommand::ShowPage`;
    /// this is the window's own copy, and it is the one the window acts on.
    page: Page,

    /// The config and log paths this window was opened for. The System
    /// door's two file rows are built from them, and `system_state` wants
    /// them by reference on every push.
    paths: Paths,
    /// This window's own transparency, 85..=100.
    ///
    /// Held here rather than in a preferences store because there is no
    /// macOS counterpart to the Win32 twin's `HKCU\Software\beckon` yet:
    /// the value survives a page switch and dies with the window. Persisting
    /// it is `NSUserDefaults` and one line, deliberately not taken in this
    /// pass — a preference that outlives the window should be introduced with
    /// the reload path that has to honour it, not before.
    opacity: u8,

    /// Group 3 of the Keyboard door: write a bound chord as `Caps` in the
    /// list instead of its three modifiers.
    ///
    /// A **view** preference, so it lives with the window rather than in
    /// `apps.toml` — that file stays byte-identical between a machine with
    /// it on and one without, which is the whole reason a person can share
    /// one config across machines.
    caps_view: bool,
    /// The two halves of the fold that are not this window's own preference.
    /// Mirrored out of the last `ControlState` so the table's data source can
    /// read them re-entrantly — see `caps_view_now` and friends.
    caps_checked: bool,
    caps_hold: Chord,

    /// The last `AboutState` pushed.
    ///
    /// Held because the copy buttons act **in the window** — design §3.4 —
    /// and so need the row's payload at click time. It cannot come back
    /// through a callback: `SettingsCommand` is `Copy + Eq` and carries no
    /// `String` on purpose, so a caller answering `Copy(Field)` would have to
    /// rebuild this page's state and become a second author for it.
    about_state: Option<beckon_core::settings::AboutState>,
}

thread_local! {
    static UI: RefCell<Option<Ui>> = const { RefCell::new(None) };
    static CB: RefCell<Option<Callbacks>> = const { RefCell::new(None) };
}

/// Run `f` with the callbacks taken out of the slot.
///
/// Taken out, not borrowed across: a handler reloads, saves and closes,
/// all of which re-enter this module. Holding the `RefCell` would panic on
/// the second borrow.
fn with_cb(f: impl FnOnce(&mut Callbacks)) {
    let Some(mut cb) = CB.with(|c| c.borrow_mut().take()) else {
        return;
    };
    f(&mut cb);
    CB.with(|c| {
        if c.borrow().is_none() {
            *c.borrow_mut() = Some(cb);
        }
    });
}

/// Every control handle, with the `RefCell` borrow already released.
///
/// **This is the fix for a measured panic, not a convenience.** AppKit
/// re-enters this module *synchronously*: `reloadData` calls the data source
/// before it returns, and setting the selection raises the delegate the same
/// way. `apply_state` used to hold a `borrow_mut` across `reloadData`, so
/// the data source's `borrow()` met an outstanding mutable borrow and the
/// window died on open with `RefCell already mutably borrowed` — found by
/// `examples/settings_probe.rs` on macmini, 2026-08-13, on its first run in
/// an Aqua session.
///
/// The module already stated this rule twice — `with_cb` takes the callbacks
/// OUT of the slot, and `tray.rs`'s `dispatch` does the same — and the
/// rendering path broke it anyway. Routing every control access through here
/// makes it structural rather than remembered: there is no way to reach a
/// control while holding the borrow, because the only way to reach one is a
/// function that has already dropped it.
///
/// Cloning is cheap: a `Retained` clone is one retain.
fn controls() -> Option<Controls> {
    UI.with(|u| u.borrow().as_ref().map(|x| x.c.clone()))
}

/// Is this notification a programmatic push rather than a human edit?
fn suppressed() -> bool {
    UI.with(|u| u.borrow().as_ref().map(|x| x.pushing).unwrap_or(true))
}

/// Raise one `SettingsCommand`.
///
/// **Until this existed, `on_command` was never raised on macOS and all
/// eleven variants were unreachable** — the window had no way to pause the
/// service, reload it, open a file or follow a link, because there was
/// nothing on this side to say so. It goes through `with_cb` for that
/// function's reason: a handler may reload, save or close, every one of
/// which re-enters this module.
fn cmd(c: SettingsCommand) {
    with_cb(|cb| (cb.on_command)(c));
}

/// Put an About row on the clipboard, then report that it happened.
///
/// **The row's bare payload, not the string on screen.** `AboutValue` splits
/// `shown` from `copy` precisely because `Location` shows a verdict clause
/// and is shortened for width, while a copied path is for pasting into a
/// terminal. `copy_text` is the one decision and it lives in core.
///
/// The window acts first and reports afterwards, unlike every other command
/// here. That is design §3.4's rule and it follows from the type:
/// `SettingsCommand` is `Copy + Eq` and deliberately carries no `String`, so
/// a caller asked to perform the copy would have to rebuild this page's state
/// and become a second author for it.
fn copy_field(f: Field) {
    let text = UI.with(|u| {
        u.borrow().as_ref().and_then(|x| {
            x.about_state
                .as_ref()
                .map(|st| copy_text(st, f).to_string())
        })
    });
    let Some(text) = text else { return };
    let pb = { NSPasteboard::generalPasteboard() };
    unsafe {
        pb.clearContents();
        pb.setString_forType(&NSString::from_str(&text), NSPasteboardTypeString);
    }
    cmd(SettingsCommand::Copy(f));
}

/// The window's own transparency.
///
/// `alphaValue` is the same mechanism the Win32 twin uses (a layered
/// window's alpha), including the same consequence: the text goes
/// translucent with the ground, because it is one surface being composited
/// rather than a ground tinted behind opaque ink. Matching the Windows
/// behaviour is the point — the slider means the same thing on both.
fn set_window_opacity(w: &NSWindow, pct: u8) {
    let a = (pct as f64 / 100.0).clamp(0.0, 1.0);
    w.setAlphaValue(a);
}

// ---------------------------------------------------------------------------
// Chord capture
// ---------------------------------------------------------------------------

/// Arm the tap and put the window into recording state.
fn start_recording() {
    let Some(c) = controls() else { return };
    let sink: crate::caps_tap::CaptureSink = Box::new(on_capture);
    match crate::caps_tap::begin_capture(sink) {
        Ok(()) => {
            c.record.setTitle(&NSString::from_str("Stop"));
            c.notes
                .setStringValue(&NSString::from_str(beckon_core::capture::HINT_ARMED));
        }
        Err(e) => {
            // Input Monitoring is the usual one, and it is per-BINARY on this
            // platform -- a fresh `cargo build` loses it. Saying so beats a
            // button that does nothing.
            c.notes.setStringValue(&NSString::from_str(&e));
        }
    }
}

/// **Idempotent, and called from every exit path.** The list below is the
/// whole safety argument for holding a tap that swallows every keystroke:
///
/// - the `Stop` button
/// - a page switch (`show_page`) -- **not** covered by any focus notification,
///   because a tab click is a child-to-child focus move inside one window,
///   and `Record` is a Shortcuts-page control, so switching doors takes the
///   only visible way out of a recording off the screen
/// - the window closing, and being destroyed
/// - `Quit` from the menu bar, which never reaches a close handler
///
/// Windows learned each of these separately; they are ported rather than
/// rediscovered.
pub(crate) fn stop_recording() {
    crate::caps_tap::end_capture();
    let Some(c) = controls() else { return };
    c.record.setTitle(&NSString::from_str("Record"));
    // The notes line is `apply_state`'s to own; clearing it is the honest
    // hand-back, and the next push rewrites it from the model. Leaving the
    // last hint up would have the window claiming a recording that ended.
    c.notes.setStringValue(&NSString::from_str(""));
}

/// One outcome from the tap, on the run loop's own thread.
///
/// Cheap on purpose: a `CGEventTap` whose callback overruns is disabled by
/// the system, and `on_event` has to notice and re-enable it. Setting a
/// caption and a hint is the whole budget.
fn on_capture(outcome: beckon_core::capture::Outcome) {
    use beckon_core::capture::Outcome;
    let Some(c) = controls() else { return };

    if let Some(text) = beckon_core::capture::hint(outcome, None) {
        c.notes.setStringValue(&NSString::from_str(&text));
    }

    match outcome {
        Outcome::Captured => {
            // The chord is complete. Write it into the four boxes and the key
            // list -- the typed path's own controls, so there is exactly one
            // place a shortcut is spelled.
            let combo = crate::caps_tap::captured_combo();
            if let Some(spelled) = combo {
                apply_combo_text(&spelled);
                with_cb(|cb| (cb.on_probe_shortcut)(spelled.clone()));
                with_cb(|cb| (cb.on_edit_combo)(spelled));
            }
            stop_recording();
        }
        Outcome::Cancelled | Outcome::Disarmed => stop_recording(),
        _ => {}
    }
}

/// Open a door.
///
/// Three things happen and none is optional:
///
/// 1. the strip's selection follows, so a page opened from anywhere else —
///    the caller's stored page, a keyboard shortcut — lights the right pill;
/// 2. exactly one container is unhidden, and a hidden arranged subview
///    contributes no height, so the door that is shut costs nothing;
/// 3. the command bar appears only on the doors that WRITE the config
///    (`command_bar_shown`), while the band itself stays on all four — an
///    empty bar is indistinguishable from the window ground, and reserving
///    it keeps one meaning for the content's bottom edge.
fn show_page(p: Page) {
    // **Before the unchanged-door guard would have been wrong**: a recording
    // must end even when `show_page` is called for the door already open,
    // because `Add` and the tab strip both route through here.
    stop_recording();
    let Some(c) = controls() else { return };
    let now = UI.with(|u| u.borrow().as_ref().map(|x| x.page));
    UI.with(|u| {
        if let Some(x) = u.borrow_mut().as_mut() {
            x.page = p;
        }
    });
    c.tabs.setSelectedSegment(page_index(p) as isize);
    for (i, v) in c.pages.iter().enumerate() {
        v.setHidden(i != page_index(p));
    }
    // **The buttons, not the band.** The band carries the service line on
    // every door; hiding it would take a status that belongs on all four off
    // three of them. `command_bar_shown` answers only "does this door write
    // the config", which is what decides the buttons.
    let buttons = command_bar_shown(p);
    for b in [&c.save, &c.close_btn, &c.open_file] {
        b.setHidden(!buttons);
    }
    if now != Some(p) {
        cmd(SettingsCommand::ShowPage(p));
    }
}

define_class!(
    // SAFETY:
    // - NSObject has no subclassing requirements.
    // - Target does not implement Drop.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "BeckonSettingsTarget"]
    struct Target;

    unsafe impl NSObjectProtocol for Target {}

    unsafe impl NSTableViewDataSource for Target {
        #[unsafe(method(numberOfRowsInTableView:))]
        fn number_of_rows(&self, _t: &NSTableView) -> isize {
            UI.with(|u| u.borrow().as_ref().map(|x| x.items.len()).unwrap_or(0)) as isize
        }

        #[unsafe(method_id(tableView:objectValueForTableColumn:row:))]
        fn object_value(
            &self,
            _t: &NSTableView,
            col: Option<&NSTableColumn>,
            row: isize,
        ) -> Option<Retained<AnyObject>> {
            let id = col.map(|c| c.identifier().to_string()).unwrap_or_default();
            UI.with(|u| {
                let b = u.borrow();
                let it = b.as_ref()?.items.get(row as usize)?;
                let s = match id.as_str() {
                    COL_TICK => {
                        // A check box column's value is a number, not text.
                        let n = if it.marked { 1i64 } else { 0 };
                        let obj: Retained<AnyObject> = unsafe {
                            Retained::cast_unchecked(objc2_foundation::NSNumber::new_i64(n))
                        };
                        return Some(obj);
                    }
                    COL_APP => it.app.clone(),
                    // **Not `it.combo` raw.** That field holds the config
                    // file's own token — `ctrl+super+alt+t` — which is one
                    // language on every platform so a dotfile can be copied
                    // between machines. It is not what a person reads. The
                    // window is the layer that turns it into this keyboard's
                    // words, and on this keyboard `super` is Command.
                    COL_COMBO => {
                        let fold = beckon_core::settings::caps_view_fold(
                            caps_view_now(),
                            caps_checked_now(),
                            caps_hold_now(),
                        );
                        beckon_core::shortcuts::combo_display_folded_with(
                            &it.combo,
                            fold,
                            beckon_core::shortcuts::ModifierLabels::MAC,
                        )
                    }
                    COL_STATUS => it.flag.clone().unwrap_or_default(),
                    _ => String::new(),
                };
                let obj: Retained<AnyObject> =
                    unsafe { Retained::cast_unchecked(NSString::from_str(&s)) };
                Some(obj)
            })
        }

        #[unsafe(method(tableView:setObjectValue:forTableColumn:row:))]
        fn set_object_value(
            &self,
            _t: &NSTableView,
            value: Option<&AnyObject>,
            col: Option<&NSTableColumn>,
            row: isize,
        ) {
            if suppressed() {
                return;
            }
            let id = col.map(|c| c.identifier().to_string()).unwrap_or_default();
            if id != COL_TICK {
                return;
            }
            let on = value
                .map(|v| {
                    let n: i64 = unsafe { msg_send![v, longLongValue] };
                    n != 0
                })
                .unwrap_or(false);
            // View row -> MODEL row. The table only knows the filtered
            // position; every callback below the window takes model
            // indices. Getting this wrong ticks one binding and deletes
            // another.
            let Some(model_row) = UI.with(|u| {
                u.borrow()
                    .as_ref()
                    .and_then(|x| x.items.get(row as usize).map(|i| i.row))
            }) else {
                return;
            };
            with_cb(|cb| (cb.on_mark)(model_row, on));
        }
    }

    unsafe impl NSControlTextEditingDelegate for Target {}

    unsafe impl NSTableViewDelegate for Target {
        #[unsafe(method(tableViewSelectionDidChange:))]
        fn selection_changed(&self, _n: &NSNotification) {
            if suppressed() {
                return;
            }
            // The table is read with no borrow outstanding (see
            // `controls()`), and only then is the borrow taken to map the
            // view row to a MODEL row.
            let Some(c) = controls() else { return };
            let r = c.table.selectedRow();
            if r < 0 {
                return;
            }
            let sel = UI.with(|u| {
                u.borrow()
                    .as_ref()
                    .and_then(|x| x.items.get(r as usize).map(|i| i.row))
            });
            if let Some(model_row) = sel {
                with_cb(|cb| (cb.on_select)(model_row));
            }
        }
    }

    impl Target {
        #[unsafe(method(beckonFilter:))]
        fn on_filter(&self, _s: &AnyObject) {
            if suppressed() {
                return;
            }
            let Some(c) = controls() else { return };
            let t = c.filter.stringValue().to_string();
            with_cb(|cb| (cb.on_filter)(t));
        }

        #[unsafe(method(beckonAdd:))]
        fn on_add(&self, _s: &AnyObject) {
            with_cb(|cb| (cb.on_add)());
        }

        #[unsafe(method(beckonRemove:))]
        fn on_remove(&self, _s: &AnyObject) {
            with_cb(|cb| (cb.on_remove)());
        }

        /// One button, two captions. `Stop` is `Record` wearing the other
        /// one, which is why the caption is read back rather than a flag
        /// kept beside it -- two spellings of one state is how they drift.
        #[unsafe(method(beckonRecord:))]
        fn on_record(&self, _s: &AnyObject) {
            if crate::caps_tap::is_capturing() {
                stop_recording();
            } else {
                start_recording();
            }
        }

        #[unsafe(method(beckonSave:))]
        fn on_save(&self, _s: &AnyObject) {
            commit_fields();
            with_cb(|cb| (cb.on_apply)());
        }

        #[unsafe(method(beckonReload:))]
        fn on_reload(&self, _s: &AnyObject) {
            with_cb(|cb| (cb.on_reload_from_disk)());
        }

        #[unsafe(method(beckonKeepMine:))]
        fn on_keep_mine(&self, _s: &AnyObject) {
            with_cb(|cb| (cb.on_keep_mine)());
        }

        #[unsafe(method(beckonClose:))]
        fn on_close(&self, _s: &AnyObject) {
            let mut may = true;
            with_cb(|cb| may = (cb.on_close_request)());
            if may {
                close();
            }
        }

        #[unsafe(method(beckonOpenFile:))]
        fn on_open_file(&self, _s: &AnyObject) {
            with_cb(|cb| (cb.on_open_file)());
        }

        // --- the tab strip -------------------------------------------------

        /// Group 3 of the Keyboard door.
        ///
        /// A view preference: it changes what the Shortcuts list CELL says
        /// and touches nothing in `apps.toml`. So it is stored with the
        /// window, reported to the caller for its own records, and answered
        /// by redrawing the list.
        #[unsafe(method(beckonShorthand:))]
        fn on_shorthand(&self, _s: &AnyObject) {
            if suppressed() {
                return;
            }
            let Some(c) = controls() else { return };
            let on = c.kbd.shorthand.state() == 1;
            UI.with(|u| {
                if let Some(x) = u.borrow_mut().as_mut() {
                    x.caps_view = on;
                }
            });
            c.table.reloadData();
            // A VIEW preference, so it belongs beside `Opacity` in
            // `NSUserDefaults` and never in `apps.toml` -- that file stays
            // byte-identical between a machine with this ticked and one
            // without, which is what lets one config be shared across
            // machines. See the field's own doc.
            crate::prefs::set_caps_view(on);
            cmd(SettingsCommand::SetCapsShorthand(on));
        }

        #[unsafe(method(beckonPage:))]
        fn on_page(&self, _s: &AnyObject) {
            let Some(c) = controls() else { return };
            let i = { c.tabs.selectedSegment() };
            if i < 0 {
                return;
            }
            show_page(page_at(i as usize));
        }

        // --- door 3, System ------------------------------------------------
        //
        // Every one of these is a command that acts NOW, on the running
        // service or on this window, and none of them touches the config —
        // design §1's split by store. They go out as `SettingsCommand`, which
        // is why `on_command` had to start being raised at all: on macOS it
        // never was, so all eleven variants were unreachable.

        #[unsafe(method(beckonPause:))]
        fn on_pause(&self, _s: &AnyObject) {
            if suppressed() {
                return;
            }
            let Some(c) = controls() else { return };
            let on = c.sys.pause.state() == 1;
            cmd(SettingsCommand::SetPaused(on));
        }

        /// The System door's Reload — the tray's own, NOT the banner's
        /// "reload from disk". They answer different questions and the
        /// banner's is `on_reload_from_disk`.
        #[unsafe(method(beckonReloadNow:))]
        fn on_reload_now(&self, _s: &AnyObject) {
            cmd(SettingsCommand::ReloadNow);
        }

        #[unsafe(method(beckonOpacity:))]
        fn on_opacity(&self, _s: &AnyObject) {
            if suppressed() {
                return;
            }
            let Some(c) = controls() else { return };
            let v = { c.sys.opacity.doubleValue() };
            // The window clamps before sending; the caller may assume it.
            let pct = v.round().clamp(
                beckon_core::settings::OPACITY_MIN as f64,
                beckon_core::settings::OPACITY_MAX as f64,
            ) as u8;
            UI.with(|u| {
                if let Some(x) = u.borrow_mut().as_mut() {
                    x.opacity = pct;
                }
            });
            set_window_opacity(&c.window, pct);
            c.sys
                .opacity_value
                .setStringValue(&NSString::from_str(&format!("{pct}%")));
            // Stored HERE, not by the caller. `SettingsCommand::SetOpacity`
            // is a notification -- `serve.rs`'s arm for it is empty on both
            // platforms and says why: answering it there would make the
            // caller a second author for a value the window already holds.
            // The Win32 twin writes its registry value from the window for
            // the same reason.
            crate::prefs::set_opacity(pct);
            cmd(SettingsCommand::SetOpacity(pct));
        }

        #[unsafe(method(beckonOpenConfig:))]
        fn on_open_config(&self, _s: &AnyObject) {
            cmd(SettingsCommand::Open(SettingsTarget::Config));
        }

        #[unsafe(method(beckonRevealConfig:))]
        fn on_reveal_config(&self, _s: &AnyObject) {
            cmd(SettingsCommand::Reveal(SettingsTarget::Config));
        }

        #[unsafe(method(beckonOpenLog:))]
        fn on_open_log(&self, _s: &AnyObject) {
            cmd(SettingsCommand::Open(SettingsTarget::Log));
        }

        #[unsafe(method(beckonRevealLog:))]
        fn on_reveal_log(&self, _s: &AnyObject) {
            cmd(SettingsCommand::Reveal(SettingsTarget::Log));
        }

        // --- door 4, About -------------------------------------------------

        #[unsafe(method(beckonCopyBuild:))]
        fn on_copy_build(&self, _s: &AnyObject) {
            copy_field(Field::Build);
        }

        #[unsafe(method(beckonCopyLocation:))]
        fn on_copy_location(&self, _s: &AnyObject) {
            copy_field(Field::Location);
        }

        #[unsafe(method(beckonCopyLicence:))]
        fn on_copy_licence(&self, _s: &AnyObject) {
            copy_field(Field::Licence);
        }

        #[unsafe(method(beckonGithub:))]
        fn on_github(&self, _s: &AnyObject) {
            cmd(SettingsCommand::Open(SettingsTarget::Github));
        }

        #[unsafe(method(beckonReleases:))]
        fn on_releases(&self, _s: &AnyObject) {
            cmd(SettingsCommand::Open(SettingsTarget::Releases));
        }

        #[unsafe(method(beckonBugReport:))]
        fn on_bug_report(&self, _s: &AnyObject) {
            cmd(SettingsCommand::Open(SettingsTarget::BugReport));
        }

        /// Any of the four shortcut check boxes or the key list moved.
        ///
        /// The probe goes FIRST, while the model still holds the row's
        /// previous chord: `probe_plan`'s "Unchanged - this row already
        /// uses it" compares the typed chord against the row's own, so a
        /// probe asked after the edit would find every chord unchanged and
        /// never ask the OS anything.
        #[unsafe(method(beckonShortcut:))]
        fn on_shortcut(&self, _s: &AnyObject) {
            if suppressed() {
                return;
            }
            let Some(spelled) = shortcut_shown() else {
                // A modifier set with no key is not a half-combo to repair.
                return;
            };
            with_cb(|cb| (cb.on_probe_shortcut)(spelled.clone()));
            with_cb(|cb| (cb.on_edit_combo)(spelled));
        }

        #[unsafe(method(beckonApp:))]
        fn on_app(&self, _s: &AnyObject) {
            if suppressed() {
                return;
            }
            let Some(c) = controls() else { return };
            let t = c.app.stringValue().to_string();
            with_cb(|cb| (cb.on_edit_app)(t));
        }

        #[unsafe(method(beckonCaps:))]
        fn on_caps(&self, _s: &AnyObject) {
            if suppressed() {
                return;
            }
            let Some(c) = controls() else { return };
            let on = c.kbd.caps.state() == 1;
            with_cb(|cb| (cb.on_caps)(on));
        }

        /// All three Hold chips are one value, so they are sent together.
        /// There is no Shift chip and there must never be one: the hook has
        /// to release whatever it presses, and releasing Shift under the
        /// user's fingers makes everything they type next arrive lowercase.
        #[unsafe(method(beckonHold:))]
        fn on_hold(&self, _s: &AnyObject) {
            if suppressed() {
                return;
            }
            let Some(c) = controls() else { return };
            let chord = Chord {
                ctrl: c.kbd.hold_ctrl.state() == 1,
                super_: c.kbd.hold_super.state() == 1,
                alt: c.kbd.hold_alt.state() == 1,
            };
            with_cb(|cb| (cb.on_caps_hold)(chord));
        }

        /// Read BY INDEX, never by text: even a closed pop-up has
        /// typeahead, which moves the selection.
        #[unsafe(method(beckonTap:))]
        fn on_tap(&self, _s: &AnyObject) {
            if suppressed() {
                return;
            }
            let Some(c) = controls() else { return };
            let t = match c.kbd.tap.indexOfSelectedItem() {
                0 => CapsTap::CapsLock,
                1 => CapsTap::Escape,
                2 => CapsTap::None,
                _ => return,
            };
            with_cb(|cb| (cb.on_caps_tap)(t));
        }
    }
);

/// The combo the shortcut controls currently spell, or `None` when no key
/// is chosen. Spelled through core so it is the exact inverse of
/// `combo_view` — see `ComboView::spell`.
fn shortcut_shown() -> Option<String> {
    combo_view_of().spell()
}

/// The shortcut controls as a `ComboView`, the same shape `combo_view`
/// derives from a stored string.
fn combo_view_of() -> ComboView {
    let Some(c) = controls() else {
        return ComboView::default();
    };
    let i = c.key.indexOfSelectedItem();
    ComboView {
        ctrl: c.mod_ctrl.state() == 1,
        super_: c.mod_super.state() == 1,
        alt: c.mod_alt.state() == 1,
        shift: c.mod_shift.state() == 1,
        key: if i < 0 { None } else { Some(i as usize) },
    }
}

/// Push whatever the fields hold into the model, before an action that
/// depends on it (Save).
///
/// Sends nothing when the controls already agree with what is stored,
/// compared as `ComboView`s rather than as strings — see the module doc.
fn commit_fields() {
    let Some(c) = controls() else { return };
    let stored = UI.with(|u| u.borrow().as_ref().and_then(|x| x.shown_combo.clone()));
    let app_text = c.app.stringValue().to_string();
    if Some(combo_view_of()) != stored.as_deref().map(combo_view) {
        if let Some(c) = shortcut_shown() {
            with_cb(|cb| (cb.on_edit_combo)(c));
        }
    }
    with_cb(|cb| (cb.on_edit_app)(app_text));
}

// ---------------------------------------------------------------------------
// Construction
// ---------------------------------------------------------------------------

fn label(text: &str, mtm: MainThreadMarker) -> Retained<NSTextField> {
    NSTextField::labelWithString(&NSString::from_str(text), mtm)
}

fn check(title: &str, action: Sel, target: &Target, mtm: MainThreadMarker) -> Retained<NSButton> {
    unsafe {
        NSButton::checkboxWithTitle_target_action(
            &NSString::from_str(title),
            Some(target as &AnyObject),
            Some(action),
            mtm,
        )
    }
}

fn push(title: &str, action: Sel, target: &Target, mtm: MainThreadMarker) -> Retained<NSButton> {
    let b = unsafe {
        NSButton::buttonWithTitle_target_action(
            &NSString::from_str(title),
            Some(target as &AnyObject),
            Some(action),
            mtm,
        )
    };
    b.setBezelStyle(NSBezelStyle::Push);
    b
}

fn hstack(views: &[&NSView], mtm: MainThreadMarker) -> Retained<NSStackView> {
    let s = NSStackView::new(mtm);
    {
        s.setOrientation(NSUserInterfaceLayoutOrientation::Horizontal);
        s.setSpacing(8.0);
        s.setDistribution(NSStackViewDistribution::Fill);
    }
    for v in views {
        s.addArrangedSubview(v);
    }
    s
}

/// Bring the window to the front and give the app the keyboard.
///
/// **`makeKeyAndOrderFront` alone is not enough here, and the reason is what
/// `serve` is.** `hotkey::install` puts the process in the Accessory
/// activation policy — no Dock tile, no menu bar — which is what a
/// background hotkey daemon should be. An Accessory app is never the active
/// application on its own, so a window it orders front appears BEHIND
/// whatever the user was in, with its close/minimise buttons greyed and its
/// fields not taking keys. Photographed 2026-08-16: the settings window came
/// up whole and unfocused underneath System Settings, which reads exactly
/// like "clicking Settings did nothing".
///
/// `activate` asks for the application; `makeKeyAndOrderFront` asks for the
/// window. Both, in that order, and neither on its own.
fn raise(w: &NSWindow) {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    objc2_app_kit::NSApplication::sharedApplication(mtm).activate();
    w.makeKeyAndOrderFront(None);
}

/// Is the window open?
pub fn is_open() -> bool {
    UI.with(|u| u.borrow().is_some())
}

/// Raise the window that is already open. `false` when there is none.
pub fn open_existing() -> bool {
    let Some(mtm) = MainThreadMarker::new() else {
        return false;
    };
    let _ = mtm;
    let Some(c) = controls() else {
        return false;
    };
    // Borrow released first: ordering a window front can raise its delegate
    // before returning, and anything that re-enters here would find the
    // RefCell held. Same rule as `controls()`.
    raise(&c.window);
    true
}

fn close() {
    // **Before the state is taken**, because `stop_recording` reads
    // `controls()` to put the caption back -- and because a tap that
    // swallows every keystroke must not outlive the window that armed it.
    // `end_capture` is idempotent, so the other six callers cost nothing.
    stop_recording();
    // Take the state out FIRST, then close. `NSWindow::close` runs the
    // window delegate synchronously, and anything that re-enters must find
    // the slot empty rather than borrowed.
    let ui = UI.with(|u| u.borrow_mut().take());
    CB.with(|c| *c.borrow_mut() = None);
    if let Some(ui) = ui {
        ui.c.window.close();
    }
}

/// Hand the installed-app catalog to the window.
///
/// The Windows twin posts a message because its scan runs on a worker
/// thread. This one is called inline from the main thread, so it forwards
/// straight to the callback — but it keeps the same name and shape, so the
/// day the scan does move to a worker, only this function changes.
pub fn post_catalog(names: Vec<String>) {
    with_cb(|cb| (cb.on_catalog)(names));
}

/// What the user chose when asked about unsaved edits on close.
///
/// Mirrors `beckon_windows::shell::SaveChoice`. Three answers and not two:
/// Cancel means "do not close", which a bool cannot say without conflating
/// it with "close and discard".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaveChoice {
    Save,
    Discard,
    Cancel,
}

/// Ask about unsaved edits.
///
/// Button order is Save / Cancel / Discard because AppKit gives the FIRST
/// button the return key, and Save is the safe default here — the same
/// place the window's own default ring rests.
pub fn ask_save(title: &str, body: &str) -> SaveChoice {
    let Some(mtm) = MainThreadMarker::new() else {
        // No way to ask, so do not guess in the destructive direction.
        return SaveChoice::Cancel;
    };
    let alert = objc2_app_kit::NSAlert::new(mtm);
    alert.setMessageText(&NSString::from_str(title));
    alert.setInformativeText(&NSString::from_str(body));
    alert.addButtonWithTitle(&NSString::from_str("Save"));
    alert.addButtonWithTitle(&NSString::from_str("Cancel"));
    alert.addButtonWithTitle(&NSString::from_str("Discard"));
    // NSAlertFirstButtonReturn is 1000, and the rest count up from there.
    match alert.runModal() {
        1000 => SaveChoice::Save,
        1002 => SaveChoice::Discard,
        _ => SaveChoice::Cancel,
    }
}

/// Open the config file in whatever the user has set for `.toml`.
///
/// `/usr/bin/open` rather than `NSWorkspace`: the backend already shells
/// out to it for launching, it never blocks, and it needs no main thread.
pub fn open_path(p: &std::path::Path) -> Result<(), String> {
    std::process::Command::new("/usr/bin/open")
        .arg(p)
        .status()
        .map_err(|e| format!("cannot open {}: {e}", p.display()))
        .and_then(|s| {
            if s.success() {
                Ok(())
            } else {
                Err(format!("open {} exited {s}", p.display()))
            }
        })
}

/// Report a problem the window itself could not handle.
///
/// A dialog rather than stderr: this window exists because someone is not
/// reading the log.
pub fn error(body: &str) {
    let Some(mtm) = MainThreadMarker::new() else {
        eprintln!("beckon: {body}");
        return;
    };
    let alert = objc2_app_kit::NSAlert::new(mtm);
    {
        alert.setMessageText(&NSString::from_str("beckon"));
        alert.setInformativeText(&NSString::from_str(body));
        alert.runModal();
    }
}

/// Open the settings window.
pub fn open(cb: Callbacks, paths: &Paths, page: Page) -> Result<(), String> {
    let Some(mtm) = MainThreadMarker::new() else {
        return Err("the settings window must be opened on the main thread".into());
    };
    if is_open() {
        open_existing();
        return Ok(());
    }

    let target: Retained<Target> = unsafe { msg_send![Target::alloc(mtm), init] };

    // --- banner: the file changed under unsaved edits ---------------------
    let banner = label("This file changed on disk since you opened it.", mtm);
    let banner_reload = push("Reload from disk", sel!(beckonReload:), &target, mtm);
    let banner_keep = push("Keep my edits", sel!(beckonKeepMine:), &target, mtm);
    let banner_row = hstack(&[&banner, &banner_reload, &banner_keep], mtm);

    // --- head: filter + Remove + Add --------------------------------------
    let head = label("Shortcuts", mtm);
    let filter = NSTextField::new(mtm);
    unsafe {
        filter.setTarget(Some(&*target));
        filter.setAction(Some(sel!(beckonFilter:)));
        let ph = NSString::from_str("Filter");
        let cell = filter.cell().unwrap();
        let _: () = msg_send![&*cell, setPlaceholderString: &*ph];
    }
    let remove = push("Remove", sel!(beckonRemove:), &target, mtm);
    let add = push("Add", sel!(beckonAdd:), &target, mtm);
    let head_row = hstack(&[&head, &filter, &remove, &add], mtm);

    // --- the list ---------------------------------------------------------
    let table = NSTableView::new(mtm);
    // App leads, Shortcut follows: the app is what the user is looking for.
    for (id, title, width) in [
        (COL_TICK, "", 20.0),
        (COL_APP, "App", 170.0),
        // Wide enough for the longest chord this window can show:
        // `Ctrl + Cmd + Option + Shift + PgDn`. At 160 it truncated a plain
        // four-modifier binding to `Ctrl + Cmd + Option + S...`, which hides
        // exactly the character that says WHICH shortcut it is.
        (COL_COMBO, "Shortcut", 250.0),
        (COL_STATUS, "", 80.0),
    ] {
        let col = {
            NSTableColumn::initWithIdentifier(NSTableColumn::alloc(mtm), &NSString::from_str(id))
        };
        {
            col.setWidth(width);
            col.headerCell().setStringValue(&NSString::from_str(title));
        }
        if id == COL_TICK {
            // The tick lives in the column's data cell, which is what makes
            // Remove a multi-delete.
            let cell = objc2_app_kit::NSButtonCell::new(mtm);
            unsafe {
                cell.setButtonType(objc2_app_kit::NSButtonType::Switch);
                cell.setTitle(Some(&NSString::from_str("")));
                col.setDataCell(&cell);
            }
        }
        table.addTableColumn(&col);
    }
    unsafe {
        table.setRowHeight(ROW_HEIGHT);
        table.setDataSource(Some(ProtocolObject::from_ref(&*target)));
        table.setDelegate(Some(ProtocolObject::from_ref(&*target)));
        table.setUsesAlternatingRowBackgroundColors(true);
        table.setAllowsMultipleSelection(false);
    }
    let scroll = NSScrollView::new(mtm);
    {
        scroll.setHasVerticalScroller(true);
        scroll.setDocumentView(Some(&table));
        scroll.setTranslatesAutoresizingMaskIntoConstraints(false);
    }
    // A fixed eight rows at every scale, measured rather than scaled from a
    // token: a window that changes height when a binding is added is a
    // window that moves under the pointer mid-edit.
    unsafe {
        NSLayoutConstraint::constraintWithItem_attribute_relatedBy_toItem_attribute_multiplier_constant(
            &scroll,
            NSLayoutAttribute::Height,
            objc2_app_kit::NSLayoutRelation::Equal,
            None,
            NSLayoutAttribute::NotAnAttribute,
            1.0,
            ROWS * ROW_HEIGHT + 24.0,
        )
        .setActive(true);
    }

    // --- editor strip -----------------------------------------------------
    // No `&` mnemonics anywhere in this window: `Hold` already claims t, w
    // and l among the chips, and AppKit has no mnemonic table to arbitrate.
    let mod_ctrl = check("Ctrl", sel!(beckonShortcut:), &target, mtm);
    let mod_super = check("Cmd", sel!(beckonShortcut:), &target, mtm);
    let mod_alt = check("Option", sel!(beckonShortcut:), &target, mtm);
    let mod_shift = check("Shift", sel!(beckonShortcut:), &target, mtm);
    let key = NSPopUpButton::new(mtm);
    unsafe {
        key.setTarget(Some(&*target));
        key.setAction(Some(sel!(beckonShortcut:)));
    }
    // Filled from key_table() IN ORDER and never sorted: `ComboView::key`
    // is an index into that slice and this control's selection index is
    // handed straight back to it.
    for k in key_table() {
        key.addItemWithTitle(&NSString::from_str(&k.name));
    }
    let record = push("Record", sel!(beckonRecord:), &target, mtm);
    let app = NSComboBox::new(mtm);
    unsafe {
        app.setTarget(Some(&*target));
        app.setAction(Some(sel!(beckonApp:)));
        app.setCompletes(true);
    }
    let editor_row = hstack(
        &[
            &label("Shortcut", mtm),
            &mod_ctrl,
            &mod_super,
            &mod_alt,
            &mod_shift,
            &key,
            &record,
            &label("App", mtm),
            &app,
        ],
        mtm,
    );

    // --- notes ------------------------------------------------------------
    let notes = label("", mtm);
    notes.setFont(Some(&NSFont::systemFontOfSize(11.0)));

    // --- command bar ------------------------------------------------------
    let save = push("Save", sel!(beckonSave:), &target, mtm);
    let open_file = push("Open config file", sel!(beckonOpenFile:), &target, mtm);
    let close_btn = push("Close", sel!(beckonClose:), &target, mtm);
    // **The service line leads the bar, and it is on ALL FOUR doors.**
    // Design §6.4: it is chrome, not a page control. That is also why the
    // BAND survives on the two doors that draw no buttons — `compute_card_rects`
    // reserves it whatever the page says, so the content's bottom edge has one
    // meaning, and an empty bar is indistinguishable from the window ground it
    // is painted on.
    let service = widgets::secondary("", mtm);
    // `Open config file` then `Close` and `Save`: the pair that ends the
    // session sits where the eye finishes. `Reload` is NOT here — the System
    // door owns it now, and the banner owns the other one.
    let bar = hstack(
        &[
            &*service as &NSView,
            &*widgets::spring(mtm),
            &*open_file,
            &close_btn,
            &save,
        ],
        mtm,
    );

    // --- door 1: Shortcuts -------------------------------------------------
    // Two cards, as design §3.1 draws them: the list with its head, then the
    // editor with its notes. The banner rides above both and contributes no
    // height while hidden.
    let list_card = widgets::card(
        &widgets::vstack(&[&*head_row as &NSView, &*scroll], 8.0, mtm),
        mtm,
    );
    let editor_card = widgets::card(
        &widgets::vstack(&[&*editor_row as &NSView, &*notes], 8.0, mtm),
        mtm,
    );
    let page_shortcuts: Retained<NSView> = widgets::vstack(
        &[&*banner_row as &NSView, &*list_card, &*editor_card],
        10.0,
        mtm,
    )
    .into_super();

    // --- door 2: Keyboard --------------------------------------------------
    let (page_keyboard, kbd) = keyboard::build(&target, mtm);

    // --- doors 3 and 4 -----------------------------------------------------
    let (page_system, sys) = system::build(&target, mtm);
    let (page_about, abt) = about::build(&target, mtm);

    // --- the strip ---------------------------------------------------------
    let tabs = NSSegmentedControl::new(mtm);
    unsafe {
        tabs.setSegmentCount(4);
        tabs.setTrackingMode(objc2_app_kit::NSSegmentSwitchTracking::SelectOne);
        tabs.setTarget(Some(&*target));
        tabs.setAction(Some(sel!(beckonPage:)));
        for (i, cap) in ["Shortcuts", "Keyboard", "System", "About"]
            .iter()
            .enumerate()
        {
            tabs.setLabel_forSegment(&NSString::from_str(cap), i as isize);
        }
        // The Shortcuts segment carries the binding count and therefore has
        // text whose width changes with the data. Pinning its width is the
        // rule the Win32 twin spends a measured four-digit slot on: **the
        // badge must never make the strip's geometry a function of the
        // config**, or the other three pills move when a binding is added.
        tabs.setWidth_forSegment(120.0, 0);
    }

    // --- stack them -------------------------------------------------------
    let root = NSStackView::new(mtm);
    {
        root.setOrientation(NSUserInterfaceLayoutOrientation::Vertical);
        // Explicit, not relied upon: a hidden door must contribute NO height.
        // The default is already true, but this stack is the one place where
        // the whole four-door illusion rests on it.
        root.setDetachesHiddenViews(true);
        root.setSpacing(10.0);
        // **Before the children are added, not after.** Setting `alignment`
        // on an `NSStackView` that already has arranged subviews does not
        // re-apply to them: the strip, every door and the command bar came out
        // TRAILING-aligned, each ending at x=628 with a ragged left edge —
        // 322 wide at 306, 494 at 134, 616 at 12. Photographed 2026-08-16
        // (`macos-door-shortcuts.png`, first set) and confirmed against the
        // frame dump in `settings_drive`, where the three trailing edges
        // agreeing to the pixel is what named the cause.
        //
        // The inner `vstack`s never had the bug because `widgets::vstack`
        // sets alignment before it adds anything, which is why only the
        // outermost column was wrong and the cards inside each door were not.
        root.setAlignment(NSLayoutAttribute::Width);
        root.setEdgeInsets(objc2_foundation::NSEdgeInsets {
            top: 12.0,
            left: 12.0,
            bottom: 12.0,
            right: 12.0,
        });
        for v in [
            &*tabs as &NSView,
            &page_shortcuts,
            &page_keyboard,
            &page_system,
            &page_about,
            &*bar,
        ] {
            root.addArrangedSubview(v);
            // The stack's own `alignment` does not do this -- see
            // `widgets::pin_width_to`, which carries the measurement.
            widgets::pin_width_to(v, &root, 12.0);
        }
    }

    let window = unsafe {
        NSWindow::initWithContentRect_styleMask_backing_defer(
            NSWindow::alloc(mtm),
            NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(640.0, 500.0)),
            NSWindowStyleMask::Titled
                | NSWindowStyleMask::Closable
                | NSWindowStyleMask::Miniaturizable
                | NSWindowStyleMask::Resizable,
            NSBackingStoreType::Buffered,
            false,
        )
    };
    unsafe {
        // **The file NAME, not the path.** A full path in a title bar is
        // truncated from the right by every window menu and app switcher
        // there is — i.e. it loses precisely the file name it was there to
        // show. This window used to set `paths.config.display()`, which is
        // the whole path.
        let name = paths
            .config
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| paths.config.display().to_string());
        window.setTitle(&NSString::from_str(&format!("beckon - {name}")));
        window.setContentView(Some(&root));
        // **Set the size deliberately, and do not let AppKit restore one.**
        //
        // Measured 2026-08-16 with `examples/settings_drive.rs`: the window
        // came up **640x1080** while `root.fittingSize()` was 506x430 — the
        // content was content, the WINDOW was too big, and every row inside
        // stretched to fill it. The 1080 came from an earlier run of the
        // same binary, back when an unconstrained wrapping label demanded
        // 1072 points of width; `NSWindow.isRestorable` defaults to true, so
        // macOS had saved that frame and handed it back on every launch
        // afterwards. Nothing reported anything: a window opening at a stale
        // size looks exactly like a window laid out wrongly.
        //
        // A settings window with fixed geometry gains nothing from
        // restoration and can inherit a bad frame from any single bad build,
        // which is a defect that survives the fix that caused it.
        window.setRestorable(false);
        window.setContentMinSize(NSSize::new(560.0, MIN_HEIGHT));
        // Save rests here, but the ring migrates to whichever push button
        // has focus, so Enter on a tabbed-to Close closes.
        let save_cell = save.cell().unwrap();
        let _: () = msg_send![&*window, setDefaultButtonCell: &*save_cell];
    }
    raise(&window);

    UI.with(|u| {
        *u.borrow_mut() = Some(Ui {
            c: Controls {
                window,
                tabs,
                pages: [page_shortcuts, page_keyboard, page_system, page_about],
                service,
                kbd,
                sys,
                abt,
                table,
                filter,
                app,
                key,
                mod_ctrl,
                mod_super,
                mod_alt,
                mod_shift,
                record,
                notes,
                banner_row,
                banner,
                banner_reload,
                banner_keep,
                save,
                close_btn,
                open_file,
                remove,
                add,
            },
            _target: target,
            items: Vec::new(),
            shown_combo: None,
            pushing: false,
            // Set below by `show_page`, which is also what tells the caller.
            page: Page::Shortcuts,
            paths: paths.clone(),
            // **This IS the reload path** the `opacity` field's old comment
            // said a stored preference should arrive with. The window is
            // built fresh on every open, so there is exactly one place the
            // stored value has to be honoured and it cannot be missed by a
            // later edit. `100` was the hard-coded default that made the
            // slider forget itself the moment the window closed.
            opacity: crate::prefs::opacity(),
            caps_view: crate::prefs::caps_view(),
            caps_checked: false,
            caps_hold: Chord::default(),
            about_state: None,
        });
    });
    CB.with(|c| *c.borrow_mut() = Some(cb));
    // Open on the door the caller remembered. `page` is no longer accepted
    // and discarded — that line (`let _ = page;`) was this window's whole
    // relationship with the four-door design.
    show_page(page);

    // **Size the window AFTER `show_page`, never before, and this ordering is
    // the whole bug.** Until `show_page` runs, all four doors are visible and
    // the content genuinely needs about 1048 points of height; a
    // `setContentSize` there is fought by the constraints and loses. Three
    // doors then hide, `fittingSize` drops to about 430 — and the window does
    // NOT shrink back on its own, so it sat at 640x1080 with every row inside
    // stretched to fill it.
    //
    // Nothing reported anything. The window was on screen at a plausible
    // size, the root stack was laid out correctly for the size it had, and
    // `fittingSize` said 506x430 while the frame said 640x1080 — the two
    // numbers that had to be compared were never printed side by side until
    // `examples/settings_drive.rs` printed them. It is now an assertion
    // there.
    if let Some(c) = controls() {
        c.window
            .setContentSize(NSSize::new(WINDOW_WIDTH, WINDOW_HEIGHT));
        c.window.center();

        // **Reading the stored opacity into `Ui` is only half of honouring
        // it.** `set_window_opacity` had exactly one caller -- the slider's
        // own handler -- so a value restored at construction would sit in the
        // struct while the window drew fully opaque and the slider pointed at
        // the wrong number. Three surfaces, one value; the handler updates
        // the same three in the same order.
        let pct = crate::prefs::opacity();
        set_window_opacity(&c.window, pct);
        c.sys.opacity.setDoubleValue(pct as f64);
        c.sys
            .opacity_value
            .setStringValue(&NSString::from_str(&format!("{pct}%")));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------

/// Write a chord into the four boxes and the key list.
///
/// **The same four `set_check` calls and the same `selectItemAtIndex`
/// `apply_state` uses**, deliberately: `ComboView::key` is an INDEX into
/// `key_table()`, and a second way of writing it is a second place that
/// index can be got wrong. `CBS_SORT`'s macOS equivalent does not exist
/// here, but the index discipline is the same.
fn apply_combo_text(spelled: &str) {
    let Some(c) = controls() else { return };
    let v = combo_view(spelled);
    set_check(&c.mod_ctrl, v.ctrl);
    set_check(&c.mod_super, v.super_);
    set_check(&c.mod_alt, v.alt);
    set_check(&c.mod_shift, v.shift);
    match v.key {
        Some(i) => c.key.selectItemAtIndex(i as isize),
        None => c.key.selectItemAtIndex(-1),
    }
}

fn set_check(b: &NSButton, on: bool) {
    b.setState(if on { 1 } else { 0 });
}

const TARGET_TRIPLE: &str = env!("BECKON_TARGET");

/// When did THIS process start?
///
/// The About door's stale-image verdict is the comparison of this against the
/// modification time of the file at `current_exe()`, and without it the row
/// can only print a path. The incident it exists for is in `CLAUDE.md`: a
/// beckon on a14 ran a three-hour-old image while `--version`, the package
/// manager's `current` junction and the install directory all agreed on a
/// newer one.
///
/// `proc_pidinfo(PROC_PIDTBSDINFO)` rather than `sysctl(KERN_PROC_PID)`:
/// both are FFI, but `kinfo_proc` is a large nested structure whose layout
/// this file would have to restate exactly, while `proc_bsdinfo` is flat and
/// ends in the two fields actually wanted. Getting a large `#[repr(C)]`
/// wrong reads back plausible garbage rather than failing.
///
/// Returns `None` rather than guessing: `about_state` renders that as
/// `ImageAge::Unknown`, which draws no verdict at all — silence, never a
/// false alarm.
#[cfg(target_os = "macos")]
fn process_start_time() -> Option<std::time::SystemTime> {
    // `struct proc_bsdinfo` from `<libproc.h>`, in order. Only the last two
    // fields are read; the rest is padding that must be the right size.
    #[repr(C)]
    struct ProcBsdInfo {
        pbi_flags: u32,
        pbi_status: u32,
        pbi_xstatus: u32,
        pbi_pid: u32,
        pbi_ppid: u32,
        pbi_uid: u32,
        pbi_gid: u32,
        pbi_ruid: u32,
        pbi_rgid: u32,
        pbi_svuid: u32,
        pbi_svgid: u32,
        rfu_1: u32,
        pbi_comm: [u8; 16],
        pbi_name: [u8; 32],
        pbi_nfiles: u32,
        pbi_pgid: u32,
        pbi_pjobc: u32,
        e_tdev: u32,
        e_tpgid: u32,
        pbi_nice: i32,
        pbi_start_tvsec: u64,
        pbi_start_tvusec: u64,
    }

    #[link(name = "proc")]
    unsafe extern "C" {
        fn proc_pidinfo(
            pid: i32,
            flavor: i32,
            arg: u64,
            buffer: *mut std::ffi::c_void,
            buffersize: i32,
        ) -> i32;
    }
    const PROC_PIDTBSDINFO: i32 = 3;

    let mut info: ProcBsdInfo = unsafe { std::mem::zeroed() };
    let size = std::mem::size_of::<ProcBsdInfo>() as i32;
    let got = unsafe {
        proc_pidinfo(
            std::process::id() as i32,
            PROC_PIDTBSDINFO,
            0,
            &mut info as *mut _ as *mut std::ffi::c_void,
            size,
        )
    };
    // The call returns the number of bytes written, so a short read is a
    // layout mismatch and must not be trusted.
    if got != size {
        return None;
    }
    Some(
        std::time::UNIX_EPOCH
            + std::time::Duration::new(info.pbi_start_tvsec, info.pbi_start_tvusec as u32 * 1000),
    )
}

/// Push the System door.
///
/// **Two arguments, not a `SystemState`**, mirroring the Win32 twin: the rest
/// of what the page draws is something only this process can look up — the
/// log's size on disk, whether the OS is refusing transparency — and
/// `system_state` is where those become the row a reader sees.
///
/// **A second push, separate from `apply_state`, and design §1's split by
/// store is why.** This door writes the running service or this window,
/// never `apps.toml` — so it has to keep working in the one state where
/// there is no `Model` to project a `ControlState` out of at all, which is a
/// config file that does not parse. Riding on that projection would have made
/// pausing the hotkeys hostage to a TOML error.
pub fn apply_system_state(paused: bool, autostart: Option<bool>) {
    let Some(_mtm) = MainThreadMarker::new() else {
        return;
    };
    let Some(c) = controls() else { return };
    let Some((paths, opacity)) =
        UI.with(|u| u.borrow().as_ref().map(|x| (x.paths.clone(), x.opacity)))
    else {
        return;
    };

    // `Written`/`Gone`/`Unknown` matter here for the same reason they do on
    // About: a log that is not there is a fact, and `system_state` renders it
    // as `not found` rather than as `0 bytes`.
    let log_bytes = paths
        .log
        .as_ref()
        .and_then(|p| std::fs::metadata(p).ok())
        .map(|m| m.len());

    let st = beckon_core::settings::system_state(beckon_core::settings::SystemInputs {
        paused,
        // `None` OMITS the row. On macOS that is not a shortfall being
        // papered over: Homebrew's formula owns the launch agent, so a switch
        // here would be a second writer for a file beckon did not create.
        autostart,
        // Read and discarded — this window follows the system appearance.
        dark: false,
        opacity,
        block: transparency_block(),
        paths: &paths,
        log_bytes,
    });
    system::apply(&c.sys, &st);
}

/// May this machine be transparent at all, and if not, why not?
///
/// The macOS analogues of the Win32 twin's three refusals, in the same
/// precedence. `RemoteSession` has no reading here — a Screen Sharing client
/// is not a separate session from the app's point of view — so it is never
/// returned rather than being guessed at.
#[cfg(target_os = "macos")]
fn transparency_block() -> Option<beckon_core::theme::TransparencyBlock> {
    use beckon_core::theme::TransparencyBlock as B;
    let mtm = MainThreadMarker::new()?;
    let ws = { objc2_app_kit::NSWorkspace::sharedWorkspace() };
    let _ = mtm;
    if ws.accessibilityDisplayShouldIncreaseContrast() {
        return Some(B::HighContrast);
    }
    if ws.accessibilityDisplayShouldReduceTransparency() {
        return Some(B::SystemSetting);
    }
    None
}

/// Push the About door.
///
/// **It takes no arguments at all.** Every string on this page is something
/// only the settings window's own process can know — its compiled-in version,
/// its target triple, its `current_exe()` and the two timestamps behind the
/// stale-image verdict — so there is nothing for a caller to hand over, and
/// anything passed would be a copy of a fact this crate reads directly.
///
/// Called on every refresh rather than once at open, because one of those
/// strings genuinely moves: the file at the launch path can be replaced while
/// the window is up, which is the whole subject of the `Location` row.
pub fn apply_about_state() {
    let Some(_mtm) = MainThreadMarker::new() else {
        return;
    };
    let Some(c) = controls() else { return };

    let exe = std::env::current_exe().ok();
    let disk = match exe.as_ref().map(std::fs::metadata) {
        Some(Ok(m)) => match m.modified() {
            Ok(t) => beckon_core::settings::ImageOnDisk::Written(t),
            Err(_) => beckon_core::settings::ImageOnDisk::Unknown,
        },
        Some(Err(e)) if e.kind() == std::io::ErrorKind::NotFound => {
            beckon_core::settings::ImageOnDisk::Gone
        }
        _ => beckon_core::settings::ImageOnDisk::Unknown,
    };

    // **`exe` goes to the row UNRESOLVED.** Resolving it through
    // `canonicalize` would report where a symlink points TODAY, and on this
    // project's own macOS install that is `/etc/profiles/per-user/<user>/bin/
    // beckon` — a stable wrapper over a Nix store path that changes on every
    // rebuild. The unresolved launch path is the string the row must show;
    // resolving it is the exact simplification design §3.4 forbids.
    let st = beckon_core::settings::about_state(beckon_core::settings::AboutInputs {
        version: env!("CARGO_PKG_VERSION"),
        target: TARGET_TRIPLE,
        exe: exe.as_deref(),
        started: process_start_time(),
        disk,
        // macOS has no second path to compare against: there is no launcher
        // junction between the caller and the image the way scoop's
        // `current\` sits in front of a Windows install. `Unknown` is the
        // documented answer for "one of the two could not be resolved", and
        // it costs silence rather than a false alarm.
        identity: beckon_core::settings::ImageIdentity::Unknown,
        licence: env!("CARGO_PKG_LICENSE"),
    });

    about::apply(&c.abt, &st, crate::is_accessibility_trusted());
    UI.with(|u| {
        if let Some(x) = u.borrow_mut().as_mut() {
            x.about_state = Some(st);
        }
    });
}

/// Draw `st`. Called on every keystroke, so it must be cheap and must not
/// raise the notifications it causes — hence the `pushing` flag.
///
/// **It does not call a layout pass.** See the module doc: the Win32 twin's
/// measured data-loss bug came from exactly that.
pub fn apply_state(st: &ControlState, external_change: bool, catalog: Option<&[String]>) {
    let Some(_mtm) = MainThreadMarker::new() else {
        return;
    };
    // PHASE 1 -- model-side fields, under a borrow released before any
    // AppKit call. `items` must be in place BEFORE `reloadData`, because
    // that call asks the data source for them synchronously.
    let ok = UI.with(|u| {
        let mut b = u.borrow_mut();
        let Some(x) = b.as_mut() else {
            return false;
        };
        x.pushing = true;
        x.items = st.items.clone();
        x.shown_combo = st.detail.as_ref().map(|d| d.combo.clone());
        // Kept for the DATA SOURCE, which needs them while `reloadData` is
        // running and cannot be handed arguments: the list cell folds a
        // bound chord into `Caps` only when the preference is on AND Caps is
        // actually acting as a shortcut key, and `caps_view_fold` is where
        // that AND lives.
        x.caps_checked = st.caps_checked;
        x.caps_hold = st.caps_hold;
        true
    });
    if !ok {
        return;
    }

    // PHASE 2 -- every AppKit call, with no borrow outstanding. This is the
    // whole point of the split: `reloadData` re-enters the data source and
    // the selection call re-enters the delegate, both before returning.
    let Some(x) = controls() else {
        return;
    };
    // Read and the borrow dropped in one expression, before any AppKit call
    // — the rule `controls()` exists to make structural.
    let caps_view = UI.with(|u| u.borrow().as_ref().map(|y| y.caps_view).unwrap_or(false));
    {
        // The count badge, on the Shortcuts pill and nowhere else.
        //
        // **`binding_count`, never `items.len()`.** The two differ on purpose:
        // `items` is what is on SCREEN, which is filter-dependent and
        // additionally exempts the selected row from the filter — and this
        // badge is read from three doors that have no filter box at all,
        // where "the rows matching a filter you cannot see" is not a number
        // that means anything. It is also the only count in the window; a
        // second one beside the heading was retired precisely because the two
        // could disagree under a filter while both were right.
        //
        // Shown at `0` as well. `unreadable_state` sets it to zero, and zero
        // is true of a file beckon cannot read: it has no bindings beckon can
        // act on. A row added and not yet saved counts, for the same reason
        // the title bar's dirty mark follows the model rather than the disk.
        {
            x.tabs.setLabel_forSegment(
                &NSString::from_str(&format!("Shortcuts  {}", st.binding_count)),
                0,
            );
        }
        // The dirty mark. `setDocumentEdited:` is AppKit's own — it puts a
        // dot in the close button — so the Win32 twin's `*` title prefix does
        // NOT port: both at once would be two marks for one fact.
        x.window.setDocumentEdited(st.dirty);

        // The service line, on every door.
        //
        // **No glyph in front of it.** The Win32 twin owner-draws one because
        // it also owner-draws the colour; here the colour IS the signal and
        // AppKit resolves it against the appearance, so a leading `ok` / `!`
        // / `x` would be a second encoding of the same fact. `Ok` is
        // deliberately quiet — `secondaryLabelColor`, the same weight as the
        // rest of the chrome — because a healthy state announcing itself is
        // the noise the Shortcuts door's status vocabulary already refuses to
        // make.
        x.service
            .setStringValue(&NSString::from_str(&st.service.text));
        let tone = match st.service.mark {
            Mark::Bad => objc2_app_kit::NSColor::systemRedColor(),
            Mark::Warn => objc2_app_kit::NSColor::systemOrangeColor(),
            Mark::Ok | Mark::Unknown => objc2_app_kit::NSColor::secondaryLabelColor(),
        };
        x.service.setTextColor(Some(&tone));

        x.table.reloadData();
        if let Some(i) = st.selected {
            let set = objc2_foundation::NSIndexSet::indexSetWithIndex(i);
            x.table.selectRowIndexes_byExtendingSelection(&set, false);
        } else {
            unsafe { x.table.deselectAll(None) };
        }

        // Written back only when it differs from what the control holds:
        // an unconditional write raises the action on every push and would
        // fight the user's typing.
        if x.filter.stringValue().to_string() != st.filter {
            x.filter.setStringValue(&NSString::from_str(&st.filter));
        }

        match &st.detail {
            Some(d) => {
                let v = combo_view(&d.combo);
                set_check(&x.mod_ctrl, v.ctrl);
                set_check(&x.mod_super, v.super_);
                set_check(&x.mod_alt, v.alt);
                set_check(&x.mod_shift, v.shift);
                match v.key {
                    Some(i) => x.key.selectItemAtIndex(i as isize),
                    None => x.key.selectItemAtIndex(-1),
                }
                if x.app.stringValue().to_string() != d.app {
                    x.app.setStringValue(&NSString::from_str(&d.app));
                }
                let text = d
                    .notes
                    .iter()
                    .map(|n| format!("{} {}", mark_glyph(n.mark), n.text))
                    .collect::<Vec<_>>()
                    .join("\n");
                x.notes.setStringValue(&NSString::from_str(&text));
            }
            None => {
                set_check(&x.mod_ctrl, false);
                set_check(&x.mod_super, false);
                set_check(&x.mod_alt, false);
                set_check(&x.mod_shift, false);
                x.key.selectItemAtIndex(-1);
                x.app.setStringValue(&NSString::from_str(""));
                x.notes.setStringValue(&NSString::from_str(""));
            }
        }

        // The whole Keyboard door, including its own enablement -- see
        // `keyboard::apply`. The `caps_view` preference rides along because
        // it is the one control on that door with no home in `ControlState`:
        // it changes this list's cells and nothing in the file.
        keyboard::apply(&x.kbd, st, caps_view);

        // `editable` is ANDed at every enable site rather than branched on:
        // the window is not allowed to know WHY it is read only.
        let edit = st.editable;
        x.save.setEnabled(st.apply_enabled && edit);
        x.remove.setEnabled(st.remove_enabled && edit);
        x.add.setEnabled(edit);
        x.kbd.caps.setEnabled(edit);
        x.kbd.shorthand.setEnabled(edit);
        let has_row = st.detail.is_some() && edit;
        x.mod_ctrl.setEnabled(has_row);
        x.mod_super.setEnabled(has_row);
        x.mod_alt.setEnabled(has_row);
        x.mod_shift.setEnabled(has_row);
        x.key.setEnabled(has_row);
        x.app.setEnabled(has_row);

        // The banner contributes no height when hidden -- which needs the
        // ROW hidden, not only what is in it.
        x.banner_row.setHidden(!external_change);
        x.banner.setHidden(!external_change);
        x.banner_reload.setHidden(!external_change);
        x.banner_keep.setHidden(!external_change);

        if let Some(names) = catalog {
            unsafe {
                x.app.removeAllItems();
                for n in names {
                    x.app.addItemWithObjectValue(&NSString::from_str(n));
                }
            }
        }

        // The dirty marker rides on every push because the title has to
        // follow every keystroke.
        let t = x.window.title().to_string();
        let base = t.trim_end_matches(" *").to_string();
        let want = if st.dirty { format!("{base} *") } else { base };
        if want != t {
            x.window.setTitle(&NSString::from_str(&want));
        }
    }

    // PHASE 3 -- reopen the gate. Until this runs, every notification the
    // writes above provoked has been discarded by `suppressed`, which is
    // what keeps a push from coming back as a user edit.
    UI.with(|u| {
        if let Some(x) = u.borrow_mut().as_mut() {
            x.pushing = false;
        }
    });
}

fn mark_glyph(m: Mark) -> &'static str {
    match m {
        Mark::Ok => "ok",
        Mark::Warn => "!",
        Mark::Bad => "x",
        Mark::Unknown => "?",
    }
}

/// The three inputs `caps_view_fold` needs, read one at a time with the
/// borrow taken and released each time.
///
/// Split into three tiny reads rather than one struct because the data
/// source calls them from inside `reloadData`, i.e. re-entrantly, where an
/// outstanding borrow is the measured panic `controls()` exists to prevent.
fn caps_view_now() -> bool {
    UI.with(|u| u.borrow().as_ref().map(|x| x.caps_view).unwrap_or(false))
}
fn caps_checked_now() -> bool {
    UI.with(|u| u.borrow().as_ref().map(|x| x.caps_checked).unwrap_or(false))
}
fn caps_hold_now() -> Chord {
    UI.with(|u| u.borrow().as_ref().map(|x| x.caps_hold).unwrap_or_default())
}
