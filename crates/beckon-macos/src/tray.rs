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
    NSAccessibility, NSApplication, NSAttributedStringNSStringDrawing, NSColor, NSControl, NSFont,
    NSFontAttributeName, NSForegroundColorAttributeName, NSImage, NSLayoutAttribute, NSMenu,
    NSMenuDelegate, NSMenuItem, NSMutableParagraphStyle, NSParagraphStyleAttributeName,
    NSStackView, NSStatusBar, NSStatusItem, NSSwitch, NSTextAlignment, NSTextField, NSTextTab,
    NSUserInterfaceLayoutOrientation, NSVariableStatusItemLength, NSView, NSWorkspace,
};
use objc2_foundation::{
    MainThreadMarker, NSArray, NSAttributedString, NSAttributedStringKey, NSData, NSDictionary,
    NSEdgeInsets, NSMutableAttributedString, NSObject, NSObjectProtocol, NSOperatingSystemVersion,
    NSPoint, NSProcessInfo, NSRect, NSSize, NSString,
};
use std::cell::RefCell;
use std::collections::HashMap;

type MenuBuilder = Box<dyn Fn() -> Vec<MenuEntry>>;
type MenuHandler = Box<dyn FnMut(u32)>;

struct Tray {
    /// Held for the life of the process. Releasing an `NSStatusItem`
    /// removes the icon, so this is not an idle field.
    _item: Retained<NSStatusItem>,
    /// Kept alive because `NSMenu`'s delegate reference is weak/unowned;
    /// dropping this makes `menuNeedsUpdate:` stop arriving and the menu
    /// silently freeze at whatever it last showed.
    _target: Retained<MenuTarget>,
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
    switch: Retained<NSSwitch>,
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
    }
);

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

/// The header: dot, title over subtitle, and the switch hard right.
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
    let dot = NSTextField::labelWithString(&NSString::from_str("\u{25CF}"), mtm);
    dot.setTextColor(Some(&dot_color(h.dot)));
    let title = NSTextField::labelWithString(&NSString::from_str(&h.title), mtm);
    title.setFont(Some(&NSFont::boldSystemFontOfSize(13.0)));
    let subtitle = NSTextField::labelWithString(&NSString::from_str(&h.subtitle), mtm);
    subtitle.setFont(Some(&NSFont::systemFontOfSize(11.0)));
    subtitle.setTextColor(Some(&NSColor::secondaryLabelColor()));
    let text = NSStackView::stackViewWithViews(
        &NSArray::from_slice(&[title.as_ref() as &NSView, subtitle.as_ref()]),
        mtm,
    );
    text.setOrientation(NSUserInterfaceLayoutOrientation::Vertical);
    text.setAlignment(NSLayoutAttribute::Leading);
    text.setSpacing(1.0);
    // The text column is what stretches, so the switch sits hard right.
    text.setContentHuggingPriority_forOrientation(
        1.0,
        objc2_app_kit::NSLayoutConstraintOrientation::Horizontal,
    );
    let switch = NSSwitch::new(mtm);
    switch.setState(if h.on { 1 } else { 0 });
    unsafe {
        switch.setTarget(Some(target as &AnyObject));
        switch.setAction(Some(sel!(beckonControlAction:)));
    }
    switch.setTag(id as isize);
    switch.setAccessibilityLabel(Some(&NSString::from_str("Shortcuts on")));
    let row = NSStackView::stackViewWithViews(
        &NSArray::from_slice(&[dot.as_ref() as &NSView, text.as_ref(), switch.as_ref()]),
        mtm,
    );
    row.setOrientation(NSUserInterfaceLayoutOrientation::Horizontal);
    row.setSpacing(8.0);
    row.setEdgeInsets(NSEdgeInsets {
        top: 6.0,
        left: 14.0,
        bottom: 6.0,
        right: 14.0,
    });
    row.setFrame(NSRect::new(
        NSPoint::new(0.0, 0.0),
        NSSize::new(HEADER_WIDTH, 44.0),
    ));
    item.setView(Some(&row));
    TRAY.with(|t| {
        if let Some(x) = t.borrow_mut().as_mut() {
            x.header = Some(HeaderParts {
                dot,
                subtitle,
                switch,
            });
        }
    });
    item
}

/// After the switch was flipped: rebuild, and move the header's three parts
/// to what the rebuilt header says. The rest of the open menu stays as drawn
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
            x.header
                .as_ref()
                .map(|p| (p.dot.clone(), p.subtitle.clone(), p.switch.clone()))
        })
    });
    if let Some((dot, subtitle, switch)) = parts {
        dot.setTextColor(Some(&dot_color(h.dot)));
        subtitle.setStringValue(&NSString::from_str(&h.subtitle));
        switch.setState(if h.on { 1 } else { 0 });
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

    let menu = NSMenu::new(mtm);
    // Without this AppKit greys every row it thinks nobody handles.
    menu.setAutoenablesItems(false);
    menu.setDelegate(Some(ProtocolObject::from_ref(&*target)));
    item.setMenu(Some(&menu));

    TRAY.with(|t| {
        *t.borrow_mut() = Some(Tray {
            _item: item,
            _target: target,
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
    let item = TRAY.with(|t| t.borrow().as_ref().map(|x| x._item.clone()));
    if let Some(item) = item {
        if let Some(button) = item.button(mtm) {
            button.setToolTip(Some(&NSString::from_str(text)));
        }
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
