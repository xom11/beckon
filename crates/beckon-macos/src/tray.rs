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

use beckon_core::menu::{MenuEntry, MENU_ID_DOUBLE_CLICK};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2::{define_class, msg_send, sel, AnyThread, MainThreadOnly};
use objc2_app_kit::{
    NSApplication, NSImage, NSMenu, NSMenuDelegate, NSMenuItem, NSStatusBar, NSStatusItem,
    NSVariableStatusItemLength,
};
use objc2_foundation::{MainThreadMarker, NSData, NSObject, NSObjectProtocol, NSSize, NSString};
use std::cell::RefCell;

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
            // The builder is taken OUT of the slot, not borrowed across, for
            // the reason `dispatch` states and the settings window learned
            // the hard way (`settings_window::controls`): a callback that
            // re-enters this module while a borrow is live panics. Today's
            // builder only reads `ServeState`, so this is insurance rather
            // than a fix -- but it is the same shape as the bug that
            // actually happened, and the cost is one take-and-restore.
            let Some(build) = TRAY.with(|t| {
                t.borrow_mut()
                    .as_mut()
                    .map(|x| std::mem::replace(&mut x.build, Box::new(Vec::new)))
            }) else {
                return;
            };
            let entries = build();
            TRAY.with(|t| {
                if let Some(x) = t.borrow_mut().as_mut() {
                    x.build = build;
                }
            });
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
    }
);

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

/// Replace `menu`'s rows with `entries`.
fn populate(menu: &NSMenu, entries: &[MenuEntry], target: &MenuTarget, mtm: MainThreadMarker) {
    menu.removeAllItems();
    for e in entries {
        if e.is_separator() {
            menu.addItem(&NSMenuItem::separatorItem(mtm));
            continue;
        }
        let title = NSString::from_str(&e.label);
        let item = unsafe {
            NSMenuItem::initWithTitle_action_keyEquivalent(
                NSMenuItem::alloc(mtm),
                &title,
                // A disabled row still gets the action: AppKit decides
                // enablement from `setEnabled:` below, and leaving the
                // selector off would ALSO grey the row, making `enabled`
                // impossible to observe separately.
                Some(sel!(beckonMenuAction:)),
                &NSString::from_str(""),
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
        menu.addItem(&item);
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
    // 14x14 pt is what `b.square.fill` occupied (15x14, measured), so the
    // item does not change width across the upgrade; the source is 28x28,
    // i.e. exactly @2x.
    //
    // The title is cleared explicitly. An `NSStatusBarButton` draws both if
    // both are set, and the result is the icon followed by the word.
    const MARK: &[u8] = include_bytes!("../../../assets/beckon-menubar.png");
    let mark = NSImage::initWithData(NSImage::alloc(), &NSData::with_bytes(MARK));
    match mark {
        Some(img) => {
            img.setSize(NSSize::new(14.0, 14.0));
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
