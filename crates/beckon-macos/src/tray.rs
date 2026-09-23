//! The macOS menu bar item for `serve` — the counterpart of the Windows
//! tray icon in `beckon_windows::hotkey`.
//!
//! It deliberately exposes the same four things that module does —
//! `set_menu`, `set_status`, `request_quit`, and `beckon_core::menu`'s
//! `MenuEntry` — so `serve::build_entries` composes one menu for both
//! platforms and neither side owns a private notion of what a row is.
//!
//! **Nothing here proves an icon appeared, and no API in this process can.**
//! Two things were measured on macmini, and both are load-bearing:
//!
//! - 2026-08-12, in the "Background" bootstrap namespace an SSH shell gets:
//!   `TransformProcessType` returns `OSStatus 0` and `statusItemWithLength`
//!   returns a live `NSStatusItem` with a non-nil `button`, while the screen
//!   shows nothing and `screencapture` refuses to run at all.
//! - 2026-08-13, in a real Aqua session: a status item that a person could
//!   plainly SEE on the menu bar was **not listed by
//!   `CGWindowListCopyWindowInfo`** — not at any layer, while an ordinary
//!   window of the same process was listed in the same call. So the window
//!   server cannot be asked either; "no window" is not "no icon".
//!
//! Between them: constructing successfully proves nothing, screenshotting
//! needs a grant about something else, and enumerating windows structurally
//! misses this one. The instrument is a person looking at the menu bar, and
//! `testing/macos_tray_probe.sh` asks one rather than inferring.
//!
//! ## Main thread
//!
//! `NSStatusItem::button` and `::menu` take a `MainThreadMarker`, so objc2
//! enforces the requirement in the type system. The public functions here
//! deliberately do NOT take a marker: they acquire it themselves and report
//! rather than act if they are off the main thread. That keeps the marker
//! out of every call site, which is what lets the settings-window work add
//! a real main-queue hop later without touching any caller.

use beckon_core::menu::{Dot, EntryKind, Header, MenuEntry, MENU_ID_DOUBLE_CLICK};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2::{define_class, msg_send, sel, AnyThread, MainThreadOnly};
use objc2_app_kit::{
    NSAccessibility, NSApplication, NSAttributedStringNSStringDrawing, NSAutoresizingMaskOptions,
    NSBox, NSBoxType, NSButton, NSColor, NSControl, NSFont, NSFontAttributeName,
    NSForegroundColorAttributeName, NSImage, NSMenu, NSMenuDelegate, NSMenuItem,
    NSMutableParagraphStyle, NSParagraphStyleAttributeName, NSStatusBar, NSStatusItem,
    NSTextAlignment, NSTextField, NSTextTab, NSTitlePosition, NSVariableStatusItemLength, NSView,
    NSWorkspace,
};
use objc2_foundation::{
    MainThreadMarker, NSArray, NSAttributedString, NSAttributedStringKey, NSData, NSDictionary,
    NSMutableAttributedString, NSObject, NSObjectProtocol, NSOperatingSystemVersion, NSPoint,
    NSProcessInfo, NSRect, NSSize, NSString,
};
use std::cell::RefCell;
use std::collections::HashMap;

type MenuBuilder = Box<dyn Fn() -> Vec<MenuEntry>>;
type MenuHandler = Box<dyn FnMut(u32)>;

struct Tray {
    /// Held for the life of the process. Releasing an `NSStatusItem`
    /// removes the icon, so this is not an idle field. Read in `pop_menu`
    /// and `set_dimmed`, so it is no longer prefixed `_`.
    item: Retained<NSStatusItem>,
    /// Kept alive because `NSMenu`'s delegate reference is weak/unowned;
    /// dropping this makes `menuNeedsUpdate:` stop arriving and the menu
    /// silently freeze at whatever it last showed.
    _target: Retained<MenuTarget>,
    /// The menu, shown on demand by `pop_menu` rather than attached for
    /// good -- attached, AppKit opens it on every click and no modifier can
    /// be seen (spec §3.3).
    menu: Retained<NSMenu>,
    build: MenuBuilder,
    on_click: MenuHandler,
    /// App icons by bundle id, fetched once. `iconForFile` goes to disk.
    icons: HashMap<String, Retained<NSImage>>,
    /// The header's live parts, so flipping its switch can update the
    /// subtitle in place while the menu stays open.
    header: Option<HeaderParts>,
}

struct HeaderParts {
    dot: Retained<NSTextField>,
    subtitle: Retained<NSTextField>,
    /// The drawn track, knob and hit target that replace `NSSwitch` --
    /// `header_item`'s doc comment says why.
    track: Retained<NSBox>,
    knob: Retained<NSBox>,
    button: Retained<NSButton>,
}

thread_local! {
    static TRAY: RefCell<Option<Tray>> = const { RefCell::new(None) };
}

define_class!(
    // SAFETY:
    // - NSObject has no subclassing requirements.
    // - MenuTarget does not implement Drop.
    #[unsafe(super(NSObject))]
    // NSMenuDelegate is main-thread-only, and so is everything this class
    // reaches for through TRAY.
    #[thread_kind = MainThreadOnly]
    #[name = "BeckonMenuTarget"]
    struct MenuTarget;

    unsafe impl NSObjectProtocol for MenuTarget {}

    unsafe impl NSMenuDelegate for MenuTarget {
        /// Rebuild immediately before the menu is shown, which is what
        /// makes `build` a snapshot of live state rather than of whatever
        /// was true at install time. The Windows side gets this for free
        /// because it builds the popup inside `WM_TRAY`.
        #[unsafe(method(menuNeedsUpdate:))]
        fn menu_needs_update(&self, menu: &NSMenu) {
            let Some(mtm) = MainThreadMarker::new() else {
                return;
            };
            let Some(entries) = built() else { return };
            populate(menu, &entries, self, mtm);
        }
    }

    impl MenuTarget {
        /// Every row points here; the row's identity rides in its `tag`.
        #[unsafe(method(beckonMenuAction:))]
        fn beckon_menu_action(&self, sender: &NSMenuItem) {
            let id = sender.tag() as u32;
            dispatch(id);
        }

        /// The header's switch. Its tag is the header entry's id.
        #[unsafe(method(beckonControlAction:))]
        fn beckon_control_action(&self, sender: &NSControl) {
            let id = sender.tag() as u32;
            dispatch(id);
            refresh_header();
        }

        /// Every click on the icon. ⌥ + left click is the pause toggle and
        /// opens nothing; anything else opens the menu.
        #[unsafe(method(beckonStatusClick:))]
        fn beckon_status_click(&self, _sender: &AnyObject) {
            let Some(mtm) = MainThreadMarker::new() else {
                return;
            };
            let ev = NSApplication::sharedApplication(mtm).currentEvent();
            let option = ev.as_ref().is_some_and(|e| {
                e.modifierFlags()
                    .contains(objc2_app_kit::NSEventModifierFlags::Option)
            });
            let right = ev
                .as_ref()
                .is_some_and(|e| e.r#type() == objc2_app_kit::NSEventType::RightMouseUp);
            if option && !right {
                dispatch(beckon_core::menu::MENU_ID_ALT_CLICK);
                return;
            }
            pop_menu(mtm);
        }
    }
);

/// Show the menu under the icon. Attaching it for the length of one
/// `performClick` is what gives it the system's own placement and highlight;
/// detaching it again is what lets the next click reach
/// `beckonStatusClick:`.
fn pop_menu(mtm: MainThreadMarker) {
    let parts = TRAY.with(|t| {
        t.borrow()
            .as_ref()
            .map(|x| (x.item.clone(), x.menu.clone()))
    });
    let Some((item, menu)) = parts else {
        return;
    };
    item.setMenu(Some(&menu));
    if let Some(b) = item.button(mtm) {
        unsafe { b.performClick(None) };
    }
    item.setMenu(None);
}

/// Run `build` with the tray borrow released -- the rule `dispatch` states.
///
/// The builder is taken OUT of the slot, not borrowed across, for the reason
/// `dispatch` states and the settings window learned the hard way
/// (`settings_window::controls`): a callback that re-enters this module
/// while a borrow is live panics. Today's builder only reads `ServeState`,
/// so this is insurance rather than a fix -- but it is the same shape as the
/// bug that actually happened, and the cost is one take-and-restore.
fn built() -> Option<Vec<MenuEntry>> {
    let build = TRAY.with(|t| {
        t.borrow_mut()
            .as_mut()
            .map(|x| std::mem::replace(&mut x.build, Box::new(Vec::new)))
    })?;
    let entries = build();
    TRAY.with(|t| {
        if let Some(x) = t.borrow_mut().as_mut() {
            x.build = build;
        }
    });
    Some(entries)
}

/// Run `on_click` with the tray borrow released.
///
/// The handler reloads the config, toggles pause and quits — all of which
/// re-enter this module. Holding the `RefCell` across the call would panic
/// on the second borrow, which is the same rule `serve.rs`'s module doc
/// states for `backend.beckon()`.
fn dispatch(id: u32) {
    let mut handler = match TRAY.with(|t| {
        t.borrow_mut()
            .as_mut()
            .map(|t| std::mem::replace(&mut t.on_click, Box::new(|_| {})))
    }) {
        Some(h) => h,
        None => return,
    };
    handler(id);
    TRAY.with(|t| {
        if let Some(tray) = t.borrow_mut().as_mut() {
            tray.on_click = handler;
        }
    });
}

/// Replace `menu`'s rows with `entries`, recursing into submenus.
fn populate(menu: &NSMenu, entries: &[MenuEntry], target: &MenuTarget, mtm: MainThreadMarker) {
    menu.removeAllItems();
    let tab = tab_stop(entries);
    for e in entries {
        let item = match &e.kind {
            EntryKind::SectionHeader => section_header(&e.label, mtm),
            EntryKind::Header(h) => header_item(e.id, h, target, mtm),
            EntryKind::Submenu => {
                let item = plain_item(e, target, tab, mtm);
                let sub = NSMenu::new(mtm);
                sub.setAutoenablesItems(false);
                populate(&sub, &e.children, target, mtm);
                item.setSubmenu(Some(&sub));
                item
            }
            EntryKind::Item if e.is_separator() => NSMenuItem::separatorItem(mtm),
            EntryKind::Item => plain_item(e, target, tab, mtm),
        };
        menu.addItem(&item);
    }
}

/// A plain, clickable row: label, optional flag/chord, optional icon.
fn plain_item(
    e: &MenuEntry,
    target: &MenuTarget,
    tab: f64,
    mtm: MainThreadMarker,
) -> Retained<NSMenuItem> {
    let key = e.key.map(String::from).unwrap_or_default();
    let item = unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            NSMenuItem::alloc(mtm),
            &NSString::from_str(&e.label),
            // A disabled row still gets the action: AppKit decides
            // enablement from `setEnabled:` below, and leaving the selector
            // off would ALSO grey the row, making `enabled` impossible to
            // observe separately.
            Some(sel!(beckonMenuAction:)),
            &NSString::from_str(&key),
        )
    };
    unsafe {
        item.setTarget(Some(target as &AnyObject));
        item.setTag(e.id as isize);
    }
    item.setEnabled(e.enabled);
    if let Some(checked) = e.checked {
        item.setState(if checked { 1 } else { 0 });
    }
    if e.detail.is_some() || e.flag.is_some() {
        item.setAttributedTitle(Some(&row_title(e, tab)));
    }
    // Every binding row gets an image, so the column lines up even when a
    // bundle is unknown -- a gap would read as a rendering fault.
    if e.detail.is_some() {
        let img = e
            .icon
            .as_deref()
            .and_then(app_icon)
            .or_else(unknown_app_icon);
        item.setImage(img.as_deref());
    }
    if let Some(t) = &e.tooltip {
        item.setToolTip(Some(&NSString::from_str(t)));
    }
    item
}

/// Label, then the flag in orange, then a tab to a right-aligned chord.
///
/// **The label carries no colour attribute on purpose**, so AppKit still
/// inverts it on the highlighted row. The flag and the chord are coloured and
/// will NOT invert -- that is Probe P2 below.
fn row_title(e: &MenuEntry, tab: f64) -> Retained<NSAttributedString> {
    let font = NSFont::menuFontOfSize(0.0);
    let para = NSMutableParagraphStyle::new();
    let stop = unsafe {
        NSTextTab::initWithTextAlignment_location_options(
            NSTextTab::alloc(),
            NSTextAlignment::Right,
            tab,
            &NSDictionary::new(),
        )
    };
    para.setTabStops(Some(&NSArray::from_retained_slice(&[stop])));
    let out = NSMutableAttributedString::new();
    append(&out, &e.label, &font, None, &para);
    if let Some(f) = e.flag {
        append(
            &out,
            &format!("  {f}"),
            &NSFont::menuFontOfSize(11.0),
            Some(&NSColor::systemOrangeColor()),
            &para,
        );
    }
    if let Some(d) = &e.detail {
        append(
            &out,
            &format!("\t{d}"),
            &font,
            Some(&NSColor::secondaryLabelColor()),
            &para,
        );
    }
    Retained::into_super(out)
}

fn append(
    out: &NSMutableAttributedString,
    text: &str,
    font: &NSFont,
    color: Option<&NSColor>,
    para: &NSMutableParagraphStyle,
) {
    let mut keys: Vec<&NSAttributedStringKey> = vec![unsafe { NSFontAttributeName }, unsafe {
        NSParagraphStyleAttributeName
    }];
    let mut vals: Vec<&AnyObject> = vec![font.as_ref(), para.as_ref()];
    if let Some(c) = color {
        keys.push(unsafe { NSForegroundColorAttributeName });
        vals.push(c.as_ref());
    }
    let attrs = NSDictionary::from_slices(&keys, &vals);
    let piece = unsafe {
        NSAttributedString::initWithString_attributes(
            NSAttributedString::alloc(),
            &NSString::from_str(text),
            Some(&attrs),
        )
    };
    out.appendAttributedString(&piece);
}

/// Where the right-aligned chord column ends: the widest label plus flag,
/// a gap, and the widest chord, all in the menu font. Measured per menu
/// level, so the submenu's column is its own.
fn tab_stop(entries: &[MenuEntry]) -> f64 {
    const GAP: f64 = 28.0;
    let font = NSFont::menuFontOfSize(0.0);
    let small = NSFont::menuFontOfSize(11.0);
    let rows = entries.iter().filter(|e| e.detail.is_some());
    let label = rows
        .clone()
        .map(|e| width(&e.label, &font) + e.flag.map_or(0.0, |f| width(&format!("  {f}"), &small)))
        .fold(0.0, f64::max);
    let chord = rows
        .filter_map(|e| e.detail.as_deref())
        .map(|d| width(d, &font))
        .fold(0.0, f64::max);
    label + GAP + chord
}

fn width(s: &str, font: &NSFont) -> f64 {
    let attrs = NSDictionary::from_slices(
        &[unsafe { NSFontAttributeName }],
        &[font.as_ref() as &AnyObject],
    );
    let a = unsafe {
        NSAttributedString::initWithString_attributes(
            NSAttributedString::alloc(),
            &NSString::from_str(s),
            Some(&attrs),
        )
    };
    a.size().width
}

/// macOS 14's real section header. Below 14 -- `Info.plist` allows 11 -- a
/// disabled row reads as a heading.
fn section_header(title: &str, mtm: MainThreadMarker) -> Retained<NSMenuItem> {
    let t = NSString::from_str(title);
    let v14 = NSOperatingSystemVersion {
        majorVersion: 14,
        minorVersion: 0,
        patchVersion: 0,
    };
    if NSProcessInfo::processInfo().isOperatingSystemAtLeastVersion(v14) {
        return NSMenuItem::sectionHeaderWithTitle(&t, mtm);
    }
    let item = unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            NSMenuItem::alloc(mtm),
            &t,
            None,
            &NSString::from_str(""),
        )
    };
    item.setEnabled(false);
    item
}

/// A bundle's icon at menu size, cached by id.
fn app_icon(bundle_id: &str) -> Option<Retained<NSImage>> {
    let cached = TRAY.with(|t| {
        t.borrow()
            .as_ref()
            .and_then(|x| x.icons.get(bundle_id).cloned())
    });
    if cached.is_some() {
        return cached;
    }
    let ws = NSWorkspace::sharedWorkspace();
    let url = ws.URLForApplicationWithBundleIdentifier(&NSString::from_str(bundle_id))?;
    let path = url.path()?;
    let img = ws.iconForFile(&path);
    img.setSize(NSSize::new(16.0, 16.0));
    TRAY.with(|t| {
        if let Some(x) = t.borrow_mut().as_mut() {
            x.icons.insert(bundle_id.to_string(), img.clone());
        }
    });
    Some(img)
}

/// The placeholder for a binding whose app did not resolve. It is a template
/// SF Symbol, so it tints with the menu.
fn unknown_app_icon() -> Option<Retained<NSImage>> {
    let img = NSImage::imageWithSystemSymbolName_accessibilityDescription(
        &NSString::from_str("app.dashed"),
        None,
    )?;
    img.setSize(NSSize::new(16.0, 16.0));
    Some(img)
}

fn dot_color(d: Dot) -> Retained<NSColor> {
    match d {
        Dot::Ok => NSColor::systemGreenColor(),
        Dot::Warn => NSColor::systemOrangeColor(),
        Dot::Off => NSColor::tertiaryLabelColor(),
    }
}

const HEADER_WIDTH: f64 = 260.0;
const HEADER_HEIGHT: f64 = 44.0;

/// Hand-set geometry for the header row. Every one of these is a frame
/// number, not a constraint constant -- `header_item`'s doc comment says why
/// that distinction is the whole point. Origins are bottom-left: an
/// `NSView` is unflipped, so `SUBTITLE_Y` below `TITLE_Y` is the lower line.
const HEADER_PAD_X: f64 = 14.0;
const DOT_X: f64 = 14.0;
const DOT_Y: f64 = 15.0;
const TEXT_X: f64 = 30.0;
const TITLE_Y: f64 = 22.0;
const SUBTITLE_Y: f64 = 6.0;
/// Centres the 20pt toggle in the 44pt row: `(44 - 20) / 2`.
const TOGGLE_Y: f64 = 12.0;

/// Track/knob geometry for the drawn switch (`header_item`'s doc comment
/// says why it is drawn rather than an `NSSwitch`).
const TRACK_W: f64 = 34.0;
const TRACK_H: f64 = 20.0;
const KNOB_SIZE: f64 = 16.0;
const KNOB_MARGIN: f64 = 2.0;

fn track_color(on: bool) -> Retained<NSColor> {
    if on {
        NSColor::controlAccentColor()
    } else {
        NSColor::tertiaryLabelColor()
    }
}

fn knob_x(on: bool) -> f64 {
    if on {
        TRACK_W - KNOB_SIZE - KNOB_MARGIN
    } else {
        KNOB_MARGIN
    }
}

fn switch_accessibility_label(on: bool) -> Retained<NSString> {
    NSString::from_str(if on { "Shortcuts on" } else { "Shortcuts off" })
}

/// A borderless, custom-drawn `NSBox`: the shared shape behind the track and
/// the knob, and the same style `settings_window::widgets::card` uses for
/// `NSBox`-as-drawn-shape rather than as a titled group box.
///
/// **The zero content-view margins are load-bearing**, the same call and for
/// the same reason `widgets::card` makes it: an `NSBox` insets its content
/// view by `contentViewMargins`, so a subview's frame is not in the box's own
/// coordinates unless those margins are zero. The knob is positioned by hand
/// inside the track, and a non-zero inset moves and clips it.
fn plain_box(mtm: MainThreadMarker) -> Retained<NSBox> {
    let b = NSBox::new(mtm);
    b.setBoxType(NSBoxType::Custom);
    b.setTitlePosition(NSTitlePosition::NoTitle);
    b.setBorderWidth(0.0);
    b.setContentViewMargins(NSSize::new(0.0, 0.0));
    b
}

/// The header: dot, title over subtitle, and the switch hard right.
///
/// **The switch is drawn, not an `NSSwitch`.** Measured on airm3 (Darwin
/// 25.6) against the real `serve`: beckon is a `UIElement` accessory app and
/// is not the active application while its menu is open, so AppKit draws
/// every `NSControl` inside it in its INACTIVE appearance -- an `NSSwitch`
/// renders grey in both states there, and only knob position carried the
/// state, which read as broken. `NSApp.activate()` on open was rejected: it
/// would leave beckon active after the menu closes, stealing focus from
/// whatever the user was in. So the track and the knob are two plain
/// `NSBox`es coloured by hand (which do not have an "inactive" appearance to
/// fall back to), with a transparent `NSButton` carrying the same
/// target/action/tag an `NSSwitch` would have.
///
/// ## This row is laid out by FRAME. There is no Auto Layout in it, and that
/// is the fix, not a shortcut
///
/// Two photographs on airm3 measured the same failure twice:
///
/// - with the track as an `NSStackView`'s arranged view, the blue track
///   stretched the whole width of the header and past the menu's right edge
///   -- an `NSBox` has no intrinsic content size, so the stack had nothing to
///   measure and gave it every point of slack;
/// - with the hit-target `NSButton` holding `translatesAutoresizingMask...
///   (false)` plus required-priority `widthAnchor`/`heightAnchor`
///   `constraintEqualToConstant(34/20)` constraints, the track still measured
///   about **72 x 25** on screen and the knob was clipped. The constraints
///   did not win.
///
/// A menu item's custom view is a frame world: `item.setView(...)` hands
/// AppKit a view it positions by frame, and the row already carried an
/// explicit frame and a width-sizable autoresizing mask. Mixing a constraint
/// island into that is what produced two different wrong sizes, so the whole
/// header is now plain frame math: an `NSView` container, four subviews with
/// hand-set frames, and autoresizing masks for the one thing that does change
/// (the menu is wider than `HEADER_WIDTH` whenever a binding row is, so the
/// text pins left and the toggle pins right).
///
/// Nothing here calls `setTranslatesAutoresizingMaskIntoConstraints` or
/// activates a constraint. Re-adding either re-opens the two failures above.
///
/// ## The transparent hit target is a SIBLING of the track, never its parent
///
/// Measured on airm3 with the frames above printed and confirmed correct
/// (`toggle frame (212, 12, 34, 20)`, `track (0, 0, 34, 20)`): the header
/// drew the dot, the title and the subtitle, and **nothing at all** where the
/// toggle was. A `setTransparent(true)` `NSButton` draws nothing, and that
/// suppressed the drawing of its whole subtree -- the track was its child at
/// the time. In the two rounds before that the track was a SIBLING of the
/// button and did draw, blue and visible; becoming the button's child was the
/// only structural change between them.
///
/// So the container holds dot, title, subtitle, **track, then button**, in
/// that order. The track and the button carry the same frame and the same
/// `ViewMinXMargin` mask, so they stay on top of each other; the button is
/// added last, which puts it above the track in z-order and is what makes the
/// click land on it. The button has no subviews. "Correct frames, nothing
/// drawn" is the symptom to remember if anything ever re-parents them.
fn header_item(
    id: u32,
    h: &Header,
    target: &MenuTarget,
    mtm: MainThreadMarker,
) -> Retained<NSMenuItem> {
    let item = unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            NSMenuItem::alloc(mtm),
            &NSString::from_str(&h.title),
            None,
            &NSString::from_str(""),
        )
    };

    // The container. `HEADER_WIDTH` is only the starting frame: AppKit widens
    // a menu item's custom view to the menu's width, and the mask is what
    // lets it (an ordinary binding row -- "Hermes  missing  <caps>H" --
    // already clears 260pt).
    let container = NSView::new(mtm);
    container.setFrame(NSRect::new(
        NSPoint::new(0.0, 0.0),
        NSSize::new(HEADER_WIDTH, HEADER_HEIGHT),
    ));
    container.setAutoresizingMask(NSAutoresizingMaskOptions::ViewWidthSizable);

    let dot = NSTextField::labelWithString(&NSString::from_str("\u{25CF}"), mtm);
    dot.setTextColor(Some(&dot_color(h.dot)));
    dot.sizeToFit();
    dot.setFrameOrigin(NSPoint::new(DOT_X, DOT_Y));
    dot.setAutoresizingMask(NSAutoresizingMaskOptions::ViewMaxXMargin);
    container.addSubview(&dot);

    let title = NSTextField::labelWithString(&NSString::from_str(&h.title), mtm);
    title.setFont(Some(&NSFont::boldSystemFontOfSize(13.0)));
    title.sizeToFit();
    title.setFrameOrigin(NSPoint::new(TEXT_X, TITLE_Y));
    title.setAutoresizingMask(NSAutoresizingMaskOptions::ViewMaxXMargin);
    container.addSubview(&title);

    let subtitle = NSTextField::labelWithString(&NSString::from_str(&h.subtitle), mtm);
    subtitle.setFont(Some(&NSFont::systemFontOfSize(11.0)));
    subtitle.setTextColor(Some(&NSColor::secondaryLabelColor()));
    subtitle.sizeToFit();
    subtitle.setFrameOrigin(NSPoint::new(TEXT_X, SUBTITLE_Y));
    subtitle.setAutoresizingMask(NSAutoresizingMaskOptions::ViewMaxXMargin);
    container.addSubview(&subtitle);

    // The drawn switch. `ViewMinXMargin` -- the LEFT margin is the flexible
    // one -- is what keeps it hard against the right edge as the menu widens,
    // and the track and the button below carry the SAME frame and the SAME
    // mask so they stay on top of each other.
    let toggle_frame = NSRect::new(
        NSPoint::new(HEADER_WIDTH - HEADER_PAD_X - TRACK_W, TOGGLE_Y),
        NSSize::new(TRACK_W, TRACK_H),
    );

    let track = plain_box(mtm);
    track.setCornerRadius(TRACK_H / 2.0);
    track.setFillColor(&track_color(h.on));
    track.setFrame(toggle_frame);
    track.setAutoresizingMask(NSAutoresizingMaskOptions::ViewMinXMargin);
    container.addSubview(&track);

    // In the TRACK's own coordinates, which `plain_box`'s zero content-view
    // margins are what make true.
    let knob = plain_box(mtm);
    knob.setCornerRadius(KNOB_SIZE / 2.0);
    knob.setFillColor(&NSColor::whiteColor());
    knob.setFrame(NSRect::new(
        NSPoint::new(knob_x(h.on), KNOB_MARGIN),
        NSSize::new(KNOB_SIZE, KNOB_SIZE),
    ));
    track.addSubview(&knob);

    // The hit target: a SIBLING of the track, added AFTER it so it is above
    // it in z-order and takes the click, and deliberately childless. Same
    // target/action/tag the `NSSwitch` used to carry, so `dispatch` and
    // `beckon_control_action` are unchanged.
    let button = NSButton::new(mtm);
    button.setTitle(&NSString::from_str(""));
    button.setBordered(false);
    button.setTransparent(true);
    unsafe {
        button.setTarget(Some(target as &AnyObject));
        button.setAction(Some(sel!(beckonControlAction:)));
    }
    button.setTag(id as isize);
    button.setAccessibilityLabel(Some(&switch_accessibility_label(h.on)));
    button.setFrame(toggle_frame);
    button.setAutoresizingMask(NSAutoresizingMaskOptions::ViewMinXMargin);
    container.addSubview(&button);

    item.setView(Some(&container));

    // TEMPORARY, kept for one more round: the controller reads these off a
    // real `serve -v` to settle the geometry without a screenshot. Remove
    // when told to. `header_item` runs on every menu open, so one line per
    // open. `track` is now in CONTAINER coordinates (it is the container's
    // own subview since the re-parenting above), so it reports the
    // right-pinning directly rather than the button's bounds.
    if beckon_core::verbose() {
        eprintln!(
            "beckon serve: header container {:?} dot {:?} title {:?} subtitle {:?} \
             toggle frame {:?} track in container {:?} knob in track {:?}",
            container.frame(),
            dot.frame(),
            title.frame(),
            subtitle.frame(),
            button.frame(),
            track.frame(),
            knob.frame(),
        );
    }

    TRAY.with(|t| {
        if let Some(x) = t.borrow_mut().as_mut() {
            x.header = Some(HeaderParts {
                dot,
                subtitle,
                track,
                knob,
                button,
            });
        }
    });
    item
}

/// After the switch was flipped: rebuild, and move the header's parts to
/// what the rebuilt header says. The rest of the open menu stays as drawn
/// until the next open, which is when `menuNeedsUpdate:` rebuilds it anyway.
fn refresh_header() {
    let Some(entries) = built() else { return };
    let Some(h) = entries.iter().find_map(|e| match &e.kind {
        EntryKind::Header(h) => Some(h.clone()),
        _ => None,
    }) else {
        return;
    };
    let parts = TRAY.with(|t| {
        t.borrow().as_ref().and_then(|x| {
            x.header.as_ref().map(|p| {
                (
                    p.dot.clone(),
                    p.subtitle.clone(),
                    p.track.clone(),
                    p.knob.clone(),
                    p.button.clone(),
                )
            })
        })
    });
    if let Some((dot, subtitle, track, knob, button)) = parts {
        dot.setTextColor(Some(&dot_color(h.dot)));
        subtitle.setStringValue(&NSString::from_str(&h.subtitle));
        // The label is frame-sized now, not an arranged view of a stack that
        // re-lays itself out, so a longer string than the one it was built
        // with ("Paused - shortcuts are off" against "3 shortcuts") would be
        // truncated to the old width. `sizeToFit` anchors at `frame.origin`
        // on an unflipped view, so the origin is re-set only for clarity.
        subtitle.sizeToFit();
        subtitle.setFrameOrigin(NSPoint::new(TEXT_X, SUBTITLE_Y));
        track.setFillColor(&track_color(h.on));
        knob.setFrame(NSRect::new(
            NSPoint::new(knob_x(h.on), KNOB_MARGIN),
            NSSize::new(KNOB_SIZE, KNOB_SIZE),
        ));
        button.setAccessibilityLabel(Some(&switch_accessibility_label(h.on)));
        // TEMPORARY, kept for one more round, and the more informative of the
        // two prints: this one runs AFTER the menu has been displayed and
        // laid out, so these are the frames as drawn, not the ones just set.
        // It also proves the click reached the button at all. Both
        // `origin.x`es report the container's live width, since the
        // autoresizing mask keeps them at `width - 14 - 34`, and they must
        // AGREE with each other or the track and the hit target have drifted
        // apart. Remove with the print at the end of `header_item`.
        if beckon_core::verbose() {
            eprintln!(
                "beckon serve: header AFTER LAYOUT on={} toggle frame {:?} \
                 track in container {:?} knob in track {:?}",
                h.on,
                button.frame(),
                track.frame(),
                knob.frame(),
            );
        }
    }
}

/// Install the menu bar item. Idempotent: a second call replaces the
/// callbacks and leaves the existing item in place.
///
/// `Err` means the item could not be created. Callers must keep serving —
/// hotkeys are the feature and this is only the control surface.
pub fn set_menu(build: MenuBuilder, on_click: MenuHandler) -> Result<(), String> {
    let Some(mtm) = MainThreadMarker::new() else {
        return Err("the menu bar item must be installed on the main thread".into());
    };

    let already = TRAY.with(|t| t.borrow().is_some());
    if already {
        TRAY.with(|t| {
            if let Some(tray) = t.borrow_mut().as_mut() {
                tray.build = build;
                tray.on_click = on_click;
            }
        });
        return Ok(());
    }

    // NSStatusBar needs NSApp to exist. Whether it needs NSApp to be
    // *running* is what examples/tray_probe.rs measures; this call is
    // required either way.
    let _app = NSApplication::sharedApplication(mtm);

    let item = NSStatusBar::systemStatusBar().statusItemWithLength(NSVariableStatusItemLength);
    let Some(button) = item.button(mtm) else {
        return Err("status item has no button (no window server?)".into());
    };
    // **An icon, not the word.** The item read `beckon` in menu-bar text
    // while every neighbour was a glyph, which is what a person notices
    // first and reported before anything else about the port.
    //
    // **The mark is beckon's own, embedded, and deliberately NOT an SF
    // Symbol.** It was `b.square.fill` until 2026-08-17, on a belief written
    // down right here: that the symbol "is the same shape as the About door's
    // mark: a rounded square carrying the letter". Rendered and looked at, it
    // is a capital **`B`** — the SF Symbols letter family is upper case by
    // design and has no lower-case member (`a.square.fill` draws `A`, and
    // both measure 1.11:1 rather than square). Every other `b` in this
    // program is lower case: `assets/beckon.ico`, `site/favicon.png`,
    // `cap::MARK` on Windows, and `heading("b")` on the About door two files
    // away. So the menu bar was the one surface showing a different letter,
    // in a glyph beckon does not own — and the comment asserting otherwise
    // had never been run.
    //
    // `assets/beckon-menubar.png` is derived FROM `beckon.ico` by
    // `tools/make-menubar-mark.py`, so the letterform cannot drift from the
    // Windows tray icon. Two things that script does which this code depends
    // on. A menu bar image must be a TEMPLATE — one colour plus alpha, tinted
    // by the system — or it survives neither a light bar, nor a dark one, nor
    // increased contrast; `setTemplate(true)` asks for that, and it is
    // load-bearing now in a way it was not before, because an SF Symbol
    // already answers `true` and a PNG does not. And it ROUNDS the tile,
    // which `beckon.ico` does not: that file is full-bleed (measured — its
    // corner pixels are opaque `#3B82F6`), which is right where the shell
    // applies its own shape and a solid black block here.
    //
    // **17x17 pt, and the number is measured rather than chosen.** It was
    // 14x14 first — what `b.square.fill` had occupied (15x14) — and a user
    // reported that as small beside their other menu bar tools. Measuring the
    // SF Symbols Apple's own extras use, at default size on a 22 pt bar:
    // `wifi` 17x13, `battery.100` 22x11, `speaker.wave.2.fill` 19x14,
    // `display` 19x15, `bolt.fill` 13x17. **The complaint is about WIDTH, not
    // height**: at 14 the mark was the narrowest thing in the bar, and a menu
    // bar is a horizontal strip, so extent is what reads as size. 17 is the
    // tallest neighbour and matches `wifi`'s width.
    //
    // 18 was rendered against those neighbours and rejected — visibly the
    // largest object in the bar. The tile can carry 17 where an outline glyph
    // could not, because it is 78% ink against `wifi`'s 26% (measured), which
    // is the same reason it must not go further.
    //
    // The source is 34x34, exactly @2x of this; `tools/make-menubar-mark.py`
    // holds the pair and its `PT` must move with this line.
    //
    // The title is cleared explicitly. An `NSStatusBarButton` draws both if
    // both are set, and the result is the icon followed by the word.
    const MARK: &[u8] = include_bytes!("../../../assets/beckon-menubar.png");
    let mark = NSImage::initWithData(NSImage::alloc(), &NSData::with_bytes(MARK));
    match mark {
        Some(img) => {
            img.setSize(NSSize::new(17.0, 17.0));
            img.setTemplate(true);
            button.setImage(Some(&img));
            button.setTitle(&NSString::from_str(""));
        }
        // **Not the old SF Symbols version guard.** That arm was reachable —
        // any macOS below 11 — and it is gone with the symbol. This one is
        // unreachable in practice, since the bytes are compiled in and
        // ImageIO decodes PNG; it stays only because an item with neither
        // image nor title is a blank gap nobody can click on purpose, so the
        // word remains the floor rather than nothing.
        None => button.setTitle(&NSString::from_str("beckon")),
    }

    let target: Retained<MenuTarget> = unsafe { msg_send![MenuTarget::alloc(mtm), init] };

    // Route every click through `beckonStatusClick:` instead of attaching
    // the menu (no `item.setMenu` here) -- attached, AppKit opens the menu
    // on every click and no modifier can be seen (spec §3.3). `pop_menu`
    // attaches it for the length of one `performClick` instead.
    unsafe {
        button.setTarget(Some(&*target as &AnyObject));
        button.setAction(Some(sel!(beckonStatusClick:)));
    }
    let _ = button.sendActionOn(
        objc2_app_kit::NSEventMask::LeftMouseUp | objc2_app_kit::NSEventMask::RightMouseUp,
    );

    let menu = NSMenu::new(mtm);
    // Without this AppKit greys every row it thinks nobody handles.
    menu.setAutoenablesItems(false);
    menu.setDelegate(Some(ProtocolObject::from_ref(&*target)));

    TRAY.with(|t| {
        *t.borrow_mut() = Some(Tray {
            item,
            _target: target,
            menu,
            build,
            on_click,
            icons: HashMap::new(),
            header: None,
        });
    });
    Ok(())
}

/// One line of status, shown on hover.
///
/// Windows puts this in the tray tooltip; macOS has no equivalent surface
/// except the button's own tooltip, so that is where it goes. The status
/// line is ALSO the menu's first row, which is where it is actually
/// readable — this is the redundant half, not the primary one.
pub fn set_status(text: &str) {
    let Some(mtm) = MainThreadMarker::new() else {
        // Deliberately not a panic: a status string is never worth taking
        // the daemon down for. When the settings window brings a worker
        // thread, this arm becomes a main-queue hop.
        if beckon_core::verbose() {
            eprintln!("beckon serve: set_status called off the main thread; ignored");
        }
        return;
    };
    // Handle out first, borrow released, THEN the AppKit call -- the rule
    // `settings_window::controls` exists to enforce.
    let item = TRAY.with(|t| t.borrow().as_ref().map(|x| x.item.clone()));
    if let Some(item) = item {
        if let Some(button) = item.button(mtm) {
            button.setToolTip(Some(&NSString::from_str(text)));
        }
    }
}

/// Dim the icon while paused, the way Maccy does, and say so to VoiceOver.
/// ASCII, like every other display string here.
pub fn set_dimmed(dimmed: bool) {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let item = TRAY.with(|t| t.borrow().as_ref().map(|x| x.item.clone()));
    if let Some(button) = item.and_then(|i| i.button(mtm)) {
        button.setAppearsDisabled(dimmed);
        button.setAccessibilityLabel(Some(&NSString::from_str(if dimmed {
            "beckon - paused"
        } else {
            "beckon"
        })));
    }
}

/// Leave the run loop. `hotkey::run_forever` never returns, so quitting is
/// an exit, matching what the Windows tray's Quit ends up doing.
pub fn request_quit() -> ! {
    // **`Quit` never reaches a window delegate**, so this is the only place
    // that can end a recording on the way out -- the same gap
    // `hotkey::run_forever`'s two `process::exit` arms cover on Windows. A
    // tap left armed past `exit` is not a leak the OS cleans up quietly: it
    // is a process that swallowed the user's keyboard and then vanished.
    crate::caps_tap::end_capture();
    std::process::exit(0)
}

/// Re-exported so `serve.rs` can reach it through whichever module is the
/// tray on this platform, exactly as it does on Windows.
pub use beckon_core::menu::MENU_ID_DOUBLE_CLICK as DOUBLE_CLICK;
const _: () = assert!(DOUBLE_CLICK == MENU_ID_DOUBLE_CLICK);
