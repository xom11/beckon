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

use beckon_core::settings::{Callbacks, ControlState, Mark, Page, Paths};
use beckon_core::shortcuts::{combo_view, key_table, CapsTap, Chord, ComboView};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject, Sel};
use objc2::{define_class, msg_send, sel, MainThreadOnly};
use objc2_app_kit::{
    NSBackingStoreType, NSBezelStyle, NSButton, NSComboBox, NSControlTextEditingDelegate, NSFont,
    NSLayoutAttribute, NSLayoutConstraint, NSPopUpButton, NSScrollView, NSStackView,
    NSStackViewDistribution, NSTableColumn, NSTableView, NSTableViewDataSource,
    NSTableViewDelegate, NSTextField, NSUserInterfaceLayoutOrientation, NSView, NSWindow,
    NSWindowStyleMask,
};
use objc2_foundation::{
    MainThreadMarker, NSNotification, NSObject, NSObjectProtocol, NSPoint, NSRect, NSSize, NSString,
};
use std::cell::RefCell;

/// How many rows the list shows, at every scale. Fixed rather than grown
/// from the config, for the same reason the Win32 twin fixes it: a window
/// that changes height when a binding is added is a window that moves under
/// the pointer mid-edit.
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
    table: Retained<NSTableView>,
    filter: Retained<NSTextField>,
    app: Retained<NSComboBox>,
    key: Retained<NSPopUpButton>,
    mod_ctrl: Retained<NSButton>,
    mod_super: Retained<NSButton>,
    mod_alt: Retained<NSButton>,
    mod_shift: Retained<NSButton>,
    notes: Retained<NSTextField>,
    banner: Retained<NSTextField>,
    banner_reload: Retained<NSButton>,
    banner_keep: Retained<NSButton>,
    caps: Retained<NSButton>,
    hold_ctrl: Retained<NSButton>,
    hold_super: Retained<NSButton>,
    hold_alt: Retained<NSButton>,
    tap: Retained<NSPopUpButton>,
    save: Retained<NSButton>,
    remove: Retained<NSButton>,
    add: Retained<NSButton>,
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
                    COL_COMBO => it.combo.clone(),
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
            let on = c.caps.state() == 1;
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
                ctrl: c.hold_ctrl.state() == 1,
                super_: c.hold_super.state() == 1,
                alt: c.hold_alt.state() == 1,
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
            let t = match c.tap.indexOfSelectedItem() {
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
    c.window.makeKeyAndOrderFront(None);
    true
}

fn close() {
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

/// Open a door from outside the window's own event handling.
///
/// **Accepted and ignored, the same way `open` ignores the page it is
/// handed**, and for the same reason: this signature is shared, not
/// per-platform. macOS has no tab strip, so there is no door to open — and
/// nothing to open one FOR, because the banner it exists to reveal is drawn
/// here on `external_change` alone (`apply_state`), whatever page the caller
/// thinks it is on.
///
/// It is unreachable today rather than merely harmless: nothing on macOS
/// raises `SettingsCommand::ShowPage`, so `ServeState::settings_page` never
/// leaves `Shortcuts`, so `save_press` never returns the arm that calls this.
/// It exists so `apply_settings` has one shape on both platforms instead of a
/// `cfg` around the one branch that must not go quiet.
pub fn switch_to_page(page: Page) {
    let _ = page;
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

    // Same as the Windows side: accepted and ignored. macOS has no tab strip
    // and this signature is shared, not per-platform.
    let _ = page;

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
        (COL_APP, "App", 180.0),
        (COL_COMBO, "Shortcut", 160.0),
        (COL_STATUS, "", 90.0),
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
            &label("App", mtm),
            &app,
        ],
        mtm,
    );

    // --- notes ------------------------------------------------------------
    let notes = label("", mtm);
    notes.setFont(Some(&NSFont::systemFontOfSize(11.0)));

    // --- keyboard group ---------------------------------------------------
    // One line: [x] Use Caps Lock as a shortcut key  Hold [Ctrl][Cmd][Option]
    // Tap [v]. It replaced a check box plus three radios whose first caption
    // embedded the question governing all three.
    let caps = check(
        "Use Caps Lock as a shortcut key",
        sel!(beckonCaps:),
        &target,
        mtm,
    );
    let hold_ctrl = check("Ctrl", sel!(beckonHold:), &target, mtm);
    let hold_super = check("Cmd", sel!(beckonHold:), &target, mtm);
    let hold_alt = check("Option", sel!(beckonHold:), &target, mtm);
    let tap = NSPopUpButton::new(mtm);
    unsafe {
        tap.setTarget(Some(&*target));
        tap.setAction(Some(sel!(beckonTap:)));
        // Order IS the CapsTap mapping in `beckonTap:`; do not reorder.
        tap.addItemWithTitle(&NSString::from_str("Caps Lock"));
        tap.addItemWithTitle(&NSString::from_str("Escape"));
        tap.addItemWithTitle(&NSString::from_str("Nothing"));
    }
    let keyboard_row = hstack(
        &[
            &caps,
            &label("Hold", mtm),
            &hold_ctrl,
            &hold_super,
            &hold_alt,
            &label("Tap", mtm),
            &tap,
        ],
        mtm,
    );

    // --- command bar ------------------------------------------------------
    let save = push("Save", sel!(beckonSave:), &target, mtm);
    let reload = push("Reload", sel!(beckonReload:), &target, mtm);
    let open_file = push("Open file", sel!(beckonOpenFile:), &target, mtm);
    let close_btn = push("Close", sel!(beckonClose:), &target, mtm);
    let command_row = hstack(&[&open_file, &reload, &close_btn, &save], mtm);

    // --- stack them -------------------------------------------------------
    let root = NSStackView::new(mtm);
    {
        root.setOrientation(NSUserInterfaceLayoutOrientation::Vertical);
        root.setSpacing(10.0);
        root.setEdgeInsets(objc2_foundation::NSEdgeInsets {
            top: 12.0,
            left: 12.0,
            bottom: 12.0,
            right: 12.0,
        });
        for v in [
            &*banner_row as &NSView,
            &*head_row,
            &*scroll,
            &*editor_row,
            &*notes,
            &*keyboard_row,
            &*command_row,
        ] {
            root.addArrangedSubview(v);
        }
    }

    let window = unsafe {
        NSWindow::initWithContentRect_styleMask_backing_defer(
            NSWindow::alloc(mtm),
            NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(640.0, 460.0)),
            NSWindowStyleMask::Titled
                | NSWindowStyleMask::Closable
                | NSWindowStyleMask::Miniaturizable
                | NSWindowStyleMask::Resizable,
            NSBackingStoreType::Buffered,
            false,
        )
    };
    unsafe {
        window.setTitle(&NSString::from_str(&format!(
            "beckon - {}",
            paths.config.display()
        )));
        window.setContentView(Some(&root));
        window.center();
        // Save rests here, but the ring migrates to whichever push button
        // has focus, so Enter on a tabbed-to Close closes.
        let save_cell = save.cell().unwrap();
        let _: () = msg_send![&*window, setDefaultButtonCell: &*save_cell];
    }
    window.makeKeyAndOrderFront(None);

    UI.with(|u| {
        *u.borrow_mut() = Some(Ui {
            c: Controls {
                window,
                table,
                filter,
                app,
                key,
                mod_ctrl,
                mod_super,
                mod_alt,
                mod_shift,
                notes,
                banner,
                banner_reload,
                banner_keep,
                caps,
                hold_ctrl,
                hold_super,
                hold_alt,
                tap,
                save,
                remove,
                add,
            },
            _target: target,
            items: Vec::new(),
            shown_combo: None,
            pushing: false,
        });
    });
    CB.with(|c| *c.borrow_mut() = Some(cb));
    Ok(())
}

// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------

fn set_check(b: &NSButton, on: bool) {
    b.setState(if on { 1 } else { 0 });
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
    {
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

        set_check(&x.caps, st.caps_checked);
        set_check(&x.hold_ctrl, st.caps_hold.ctrl);
        set_check(&x.hold_super, st.caps_hold.super_);
        set_check(&x.hold_alt, st.caps_hold.alt);
        // Written BY INDEX, never by text, and the order here is the same
        // order `beckonTap:` reads back. Do not reorder one without the
        // other.
        x.tap.selectItemAtIndex(match st.caps_tap {
            CapsTap::CapsLock => 0,
            CapsTap::Escape => 1,
            CapsTap::None => 2,
        });

        // `editable` is ANDed at every enable site rather than branched on:
        // the window is not allowed to know WHY it is read only.
        let edit = st.editable;
        x.save.setEnabled(st.apply_enabled && edit);
        x.remove.setEnabled(st.remove_enabled && edit);
        x.add.setEnabled(edit);
        x.caps.setEnabled(edit);
        // A disabled pop-up still renders light with dark text on macOS, so
        // it looks live beside greyed labels. Enablement follows the check
        // box regardless; do not "fix" the appearance.
        let caps_on = st.caps_checked && edit;
        x.hold_ctrl.setEnabled(caps_on);
        x.hold_super.setEnabled(caps_on);
        x.hold_alt.setEnabled(caps_on);
        x.tap.setEnabled(caps_on);
        let has_row = st.detail.is_some() && edit;
        x.mod_ctrl.setEnabled(has_row);
        x.mod_super.setEnabled(has_row);
        x.mod_alt.setEnabled(has_row);
        x.mod_shift.setEnabled(has_row);
        x.key.setEnabled(has_row);
        x.app.setEnabled(has_row);

        // The banner contributes no height when hidden.
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
