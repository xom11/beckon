//! The settings window: horizontal bands stacked top to bottom — an
//! external-change banner, a section head, the shortcut list, an editor
//! strip, the keyboard group, and a command bar. Win32 only — every
//! decision it draws comes from `beckon_core::settings::ControlState`, and
//! every edit it collects goes back out through `Callbacks`. This file
//! holds no policy.
//!
//! **Bands, not a split pane.** The 45/55 column split this replaced put
//! three fixed-width columns (34 + 190 + 150 = 561 px at 150 %) inside a
//! list pane 482 px wide, so beckon shipped a horizontal scroll bar and a
//! clipped App column. Widths are now a proportion of the live list width,
//! computed in `layout`, so that cannot recur — see the comment on
//! `LIST_COLUMNS`.
//!
//! **Modeless, and created on the `serve` thread.** Hotkeys must keep
//! firing while it is open, so it cannot be a dialog box with its own modal
//! loop; `hotkey::run_forever` dispatches its messages like any others and
//! calls `filter_dialog_message` first so Ctrl+S, Tab, Enter, Esc, the
//! arrows and the Alt-mnemonics work inside it.
//!
//! **The App field's typing defect was `layout`, not the combo box.** This
//! header used to claim that a populated `CBS_DROPDOWN` rewrites its own edit
//! text *as you type*, and that `WM_APP_EDITED` is what stops it writing
//! single characters into the config. Both halves are false, and the first is
//! what sent a fix attempt down the wrong path for a day. Measured on a14
//! (`examples/combo_probe.rs`, comctl32 6.16, 121 items, session 1, real
//! `SendInput` keystrokes): typing rewrites nothing — `CB_GETCURSEL` stays
//! -1 and the child EDIT is sent nothing but `WM_KEYDOWN`/`WM_CHAR`. What the
//! control *does* do is re-synchronise its edit field to the closest matching
//! item, and select the whole string, when it is **resized** — and
//! `apply_state` used to end with an unconditional `layout(hwnd)`, which
//! `SetWindowPos`es every control on every keystroke. Typing `Notepad` left
//! `d` in the model and "Debuggable Package Manager" on screen. The fix is
//! `Ui::shown_external` plus `Ui::shown_empty` plus `Ui::shown_page`, which
//! make that layout conditional; see
//! `docs/superpowers/measurements/2026-08-11-landing-1-a14.md` sections
//! 24-26.
//!
//! **The tab strip is the same hazard from the other side, and the answer is
//! the same call site.** `layout` places only the CURRENT page's controls, so
//! a switch away from Shortcuts never reaches the App combo at all -- which
//! is a correctness requirement rather than an optimisation, because
//! `Ctrl+1`..`Ctrl+4` BECOME accelerators in Task 5 and
//! `TranslateAcceleratorW` runs before `IsDialogMessageW` and moves no focus,
//! so the combo would otherwise be resized while focused, populated and
//! holding half-typed text. The future tense is the honest one and this
//! sentence used to be written in the present: `build_accelerators` holds
//! `Ctrl+S` and nothing else today, so the only switch that exists yet is a
//! pill click or an arrow key, both of which move focus onto the pill first.
//! `handle_command`'s pill arm already takes `CMD_FROM_ACCELERATOR` so that
//! task is one line there.
//!
//! **The App field's text is still read from the message loop, one keystroke
//! behind the notification that reported it** — see `WM_APP_EDITED`. It is
//! deferred debt rather than settled design: with the layout defect fixed the
//! deferral is belt-and-braces, and it stays only because collapsing it would
//! have to re-establish the `CBN_CLOSEUP` ordering `05db60b` settled. It has
//! one accepted cost, and it is not a
//! bug: an unrelated state push dispatched between the post and the read —
//! a `WM_CATALOG` arriving, or a file-change tick — runs `apply_state`
//! against a model that has not seen the keystroke yet, rewrites the App
//! field over the typed character, and bumps `Ui::app_epoch`, which drops
//! the pending read. That character is lost from the model until the next
//! one is typed (or until `commit_fields` runs on focus loss or Save). The
//! alternative — honouring the read anyway — reports text `apply_state`
//! itself wrote as a user edit, which is worse and silent. Dropping the read
//! is the deliberate side of that trade.
//!
//! **Read-only is a flag, not a mode.** A config file that does not parse
//! opens here rather than being refused, with every mutating control off --
//! but this file has no idea that state exists. `ControlState::editable`
//! arrives false, the `enable` calls in `apply_state` AND it, and the
//! explanation arrives as ordinary notes. There is no "the file is broken"
//! branch to find, because there is no such branch.
//!
//! **The shortcut is five controls, not a text field.** Four modifier check
//! boxes and a closed list of the 81 key names, so an invalid combo is
//! unrepresentable rather than merely reported, and someone who cannot
//! physically produce a chord can still author one. The window never spells
//! a combo itself: `combo_view` turns the stored string into the five
//! control values and `Combo::canonical` turns them back.
//!
//! That list is a `CBS_DROPDOWNLIST`, which has no edit control at all — so
//! the resize defect described above, where a populated `CBS_DROPDOWN`
//! re-synchronises its edit field the moment `SetWindowPos` reaches it, is
//! structurally impossible on this control rather than guarded against. The
//! App field next to it stays a `CBS_DROPDOWN` because beckon deliberately
//! supports apps that are in no catalog; the key set has no such open end.
//!
//! **There is now a `Record` button, and the sentence that used to stand
//! here is retracted rather than deleted.** It read: *"A deliberate
//! non-feature: there is still no 'press a key to capture the shortcut'
//! field. `msctls_hotkey32` cannot capture the Windows key and Explorer eats
//! `Win+T` and its siblings before a normal window sees them, so the typed
//! path above is the whole of it."* Both facts are true and both are about
//! **a window receiving `WM_KEYDOWN`** — which is not the layer capture uses.
//! A `WH_KEYBOARD_LL` callback runs before the keystroke reaches any queue
//! and before shell hotkey processing, sees `VK_LWIN` as an ordinary
//! `vkCode`, and suppresses the key by returning 1. Measured on a14
//! 2026-08-12: `Win+T`, `Win+X`, `Win+D`, `Win+E`, `Win+R`, `Win+Tab`,
//! `Alt+Tab` and `Ctrl+Shift+Esc` all came back `SEEN=True SWALLOWED=True
//! ACTED=False`. Spec §F.1 keeps the retraction so a later session does not
//! re-derive the dropdown-only design as the safe option.
//!
//! **The typed path stays primary and capture is the accelerator.** The four
//! check boxes and the key list are always present and always Tab-navigable;
//! they are the only path that works for someone who cannot physically
//! produce the chord. While a capture is armed they are `EnableWindow(false)`
//! and `Ui::capture` is what says so — two writers on one value is the App
//! field's measured defect in another costume.

use crate::caps_hook;
use crate::shell;
use beckon_core::capture::{hint, Outcome, HINT_ARMED, HINT_UNAVAILABLE};
use beckon_core::settings::{
    banner_shown, default_button, ControlState, DefaultButton, FlagTone, ListItem, Mark, Note,
    Page, Paths, SettingsCommand,
};
use beckon_core::shortcuts::{combo_display, combo_view, key_table, CapsTap, Chord, ComboView};
use std::cell::RefCell;
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM};
/// `DWMWA_WINDOW_CORNER_PREFERENCE` is not in `windows` 0.61's own constant
/// table -- `theme.rs` already names the same reason for
/// `DWMWA_USE_IMMERSIVE_DARK_MODE` -- so `create` defines it locally.
use windows::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWINDOWATTRIBUTE};
use windows::Win32::Graphics::Gdi::*;
/// `MessageBeep` lives under Diagnostics::Debug, not WindowsAndMessaging --
/// where the `MESSAGEBOX_STYLE` it takes is defined. Named rather than
/// glob-imported so the surprise is written down once.
use windows::Win32::System::Diagnostics::Debug::MessageBeep;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
/// `HIGHCONTRASTW` is filed under Accessibility, not WindowsAndMessaging
/// where `SPI_GETHIGHCONTRAST` itself lives. Named rather than glob-imported
/// so the surprise is written down once -- the same reason `MessageBeep` is.
use windows::Win32::UI::Accessibility::{HCF_HIGHCONTRASTON, HIGHCONTRASTW};
use windows::Win32::UI::Controls::*;
use windows::Win32::UI::HiDpi::{
    GetDpiForMonitor, GetDpiForWindow, GetSystemMetricsForDpi, SystemParametersInfoForDpi,
    MDT_EFFECTIVE_DPI,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    EnableWindow, GetFocus, IsWindowEnabled, SetFocus, TrackMouseEvent, TME_LEAVE, TME_NONCLIENT,
    TRACKMOUSEEVENT,
};
use windows::Win32::UI::WindowsAndMessaging::*;

mod chrome;
mod layout;
use layout::*;
mod paint;
use paint::*;
mod theme;

/// `SS_NOPREFIX` (0x0080), which `windows` 0.61 does not export either.
///
/// A STATIC treats `&` in its text as a mnemonic marker and draws the next
/// character underlined instead of drawing the ampersand. `IDC_NOTES` is the
/// one control in this window whose text comes from the CATALOG rather than
/// from us -- Start Menu display names really do contain `&` (`Notes & To
/// Do`, `Arts & Crafts`) -- so without this an app name renders as a
/// mangled, underlined string that looks like a beckon bug.
///
/// **`SS_ENDELLIPSIS` is deliberately NOT here**, which used to matter
/// because the three ellipsis styles force a static onto ONE line with no
/// word wrap (documented on Static Control Styles) and `IDC_NOTES` was a
/// multi-line strip that relied on native word-wrap to show a second note at
/// all. **Since Task 12, `IDC_NOTES` is `SS_OWNERDRAW`** and neither wraps
/// nor ellipsises through the control at all -- `paint::draw_notes` draws
/// each note as its own `DT_SINGLELINE | DT_END_ELLIPSIS` line at a fixed
/// height, so the risk this paragraph used to warn about (the whole band
/// collapsing to its first line) cannot recur regardless of what this
/// constant is combined with.
const SS_NOPREFIX_STYLE: WINDOW_STYLE = WINDOW_STYLE(0x0080);

/// `SS_OWNERDRAW` (0x0000000D). `windows` 0.61 DOES define this constant,
/// but under `System::SystemServices`, not `WindowsAndMessaging` where the
/// rest of this file's STATIC style bits live -- named locally rather than
/// pulling in that whole module for one constant, the same call
/// `SS_NOPREFIX_STYLE` above already makes for a different gap.
///
/// **A different VALUE of a STATIC's type field, not a flag beside
/// `SS_LEFT`** -- `SS_LEFT` is 0, `SS_OWNERDRAW` is 13, and both occupy the
/// same low bits (`SS_TYPEMASK`), exactly the relationship `button`'s own
/// doc comment describes for `BS_OWNERDRAW`/`BS_DEFPUSHBUTTON`. `IDC_NOTES`
/// is the only control that carries this style (Task 12), and was the only
/// caller of the file's own `SS_LEFT_STYLE` constant -- deleted along with
/// this replacing it, rather than left unused.
const SS_OWNERDRAW_STYLE: WINDOW_STYLE = WINDOW_STYLE(0x0000000D);

/// `DM_GETDEFID` is `WM_USER + 0`, `DM_SETDEFID` is `WM_USER + 1`, and
/// `DC_HASDEFID` is the magic `0x534B` winuser.h gives it -- not a bit flag,
/// and not 1. Defined here rather than imported so this file compiles the
/// same whatever the `windows` crate's metadata does or does not carry for
/// the dialog-manager messages.
const DM_GETDEFID_MSG: u32 = WM_USER;
const DM_SETDEFID_MSG: u32 = WM_USER + 1;
const DC_HASDEFID_FLAG: u32 = 0x534B;

/// `BS_TYPEMASK` -- the four bits of a BUTTON's style that say which KIND of
/// button it is. `BM_SETSTYLE` is a read-modify-write through this mask
/// rather than a bare assignment, so migrating the default ring cannot
/// switch off `BS_NOTIFY` and take the focus notifications that drive the
/// migration with it.
const BS_TYPEMASK_BITS: u32 = 0x0F;

/// The code in a `WM_COMMAND`'s high word when it came from an accelerator
/// rather than from a control. `Ctrl+S` arrives this way; every other route
/// to a button (mouse, mnemonic, Enter, Esc's synthesised `IDCANCEL`) sends
/// `BN_CLICKED`.
const CMD_FROM_ACCELERATOR: u32 = 1;

/// `SS_CENTERIMAGE` (0x0200), which `windows` 0.61 does not export either.
/// On a STATIC holding text it centres that text vertically in the control
/// rect and clips it to one line — which is what lets a label share a band
/// line with controls taller than its own text instead of floating against
/// the top edge of it. Never on `IDC_NOTES`, which is deliberately several
/// lines tall.
const SS_CENTERIMAGE_STYLE: WINDOW_STYLE = WINDOW_STYLE(0x0200);

/// `EM_SETCUEBANNER` (`ECM_FIRST + 1`), which `windows` 0.61 does not
/// export -- the same gap `SS_CENTERIMAGE_STYLE` above fills.
const EM_SETCUEBANNER_MSG: u32 = 0x1501;

/// Posted by the catalog worker thread with the scanned app names.
pub const WM_CATALOG: u32 = WM_APP + 2;

/// Posted to this window by `handle_command` when the App combo box reports
/// that the user TYPED into it (`CBN_EDITCHANGE`). Carries the
/// `Ui::app_epoch` stamp current at the moment of posting.
///
/// A pick from the list (`CBN_SELCHANGE`) does NOT come this way -- it is
/// read synchronously out of the list, which is not subject to the defect
/// below and cannot be undone by the `CBN_CLOSEUP` backstop that follows it.
/// See the two arms in `handle_command`.
///
/// It is POSTED rather than sent, which costs nothing and keeps the read off
/// the notification's own stack. It is **not** what fixed the App field, and
/// the claim that used to stand here was wrong.
///
/// **A populated `CBS_DROPDOWN` does not rewrite its own edit text as you
/// type.** That was asserted here from the outside-in symptom and is false:
/// measured on a14 under comctl32 **6.16** with 121 items, in session 1, with
/// real focus and real `SendInput` keystrokes, the field reads exactly what
/// was typed, `CB_GETCURSEL` stays -1, and a subclass on the child EDIT sees
/// nothing but `WM_KEYDOWN`/`WM_CHAR` -- no `WM_SETTEXT`, no `EM_REPLACESEL`,
/// no `EM_SETSEL`. `crates/beckon-windows/examples/combo_probe.rs` is that
/// measurement, with an empty combo box and a plain EDIT as controls.
///
/// What actually replaced the user's typing with "Narrator" was `apply_state`
/// calling `layout`, whose `SetWindowPos` on the COMBOBOX makes the control
/// re-synchronise its edit to the closest matching item. See
/// `Ui::shown_external`, which is the fix, and `combo_probe`'s
/// `ModelLoopWithLayout` scenario, which reproduces the whole defect by
/// adding that one call to a loop that otherwise agrees.
///
/// **So this message, `Ui::app_epoch` and the deferred read are DEFERRED DEBT,
/// not settled design.** They were built for a mechanism that does not exist,
/// their original justification is gone, and what remains is belt-and-braces:
/// they cost one posted message per keystroke and buy a re-read that the
/// synchronous path would have got right. They are kept, rather than removed
/// in the same change that fixed the real defect, because collapsing them
/// means re-establishing the `CBN_CLOSEUP` ordering that `05db60b` fixed --
/// see the asymmetry comment in `handle_command` -- and that is a separate
/// change with its own hardware run. Do not read their survival as evidence
/// they are needed.
///
/// Private, unlike `WM_CATALOG`: nothing outside this file may post it, and
/// a stamp forged from outside would defeat the staleness check.
const WM_APP_EDITED: u32 = WM_APP + 3;

/// Posted by the `WH_KEYBOARD_LL` callback in `caps_hook.rs` for every
/// capture outcome this window must react to — which is every outcome
/// `beckon_core::capture::Outcome::post` returns true for, and no others:
/// auto-repeat of a held modifier would otherwise wake this thread once per
/// repeat.
///
/// The `WPARAM` is `Outcome::code()` and the `LPARAM` is unused. An integer
/// rather than a pointer to anything, because the callback may not allocate
/// (`LowLevelHooksTimeout`, and Windows unhooks a slow callback silently);
/// every string is built here, on this thread, from `CaptureState`.
///
/// `WM_APP + 4`: `+ 1` is `hotkey.rs`'s tray callback, `+ 2` is `WM_CATALOG`
/// and `+ 3` is `WM_APP_EDITED`. The tray message is on a different window,
/// so sharing a number with it would be harmless — the numbering is kept
/// global to this process anyway, because "harmless on the window it is
/// posted to" is a fact that has to be re-checked every time a message moves.
///
/// `pub`, unlike `WM_APP_EDITED`, for the same reason `WM_CATALOG` is: it is
/// posted from outside this file.
pub const WM_CAPTURE: u32 = WM_APP + 4;

/// `WM_APP + 5`: what a chip's state reads as from ANOTHER process. `WPARAM`
/// is the control id.
///
/// **It exists because `BM_GETCHECK` stopped being an answer.**
/// `examples/settings_probe.rs` drives this window across a process boundary
/// and rebuilds the whole shortcut from the four modifier chips; the moment
/// those became `BS_OWNERDRAW` the message it used began answering 0 forever,
/// so all four would have read as clear and the probe would have reported a
/// confident wrong chord -- the exact failure shape the handoff calls
/// "measuring a proxy". The window's own bit is the only real answer, and
/// this is the only channel a foreign process has to it: a bare integer
/// message needs no marshalling, unlike every comctl32 message `Remote`
/// exists for.
///
/// **The reply is deliberately not a bool.** `0` means "this build does not
/// answer this message", `1` clear, `2` armed. An unhandled `WM_APP + n`
/// comes back as 0 through `DefWindowProcW`, so a probe run against an older
/// `beckon-serve` can say it cannot tell instead of reporting four unticked
/// chips.
pub const WM_CHIP_STATE: u32 = WM_APP + 5;

mod ids;
use ids::*;

/// The watchdog that bounds an armed capture (spec F.2/F.4).
///
/// It is not belt-and-braces. `caps_hook::is_installed()` CAN LIE: past
/// `LowLevelHooksTimeout` Windows removes a `WH_KEYBOARD_LL` hook silently
/// and there is no API to ask, so a capture can stop receiving events with
/// nothing to notice it. Without this the window would sit showing `Stop`
/// with the typed path greyed out and no way back except closing it.
///
/// The only timer this window owns, so the `WM_TIMER` arm can identify it by
/// id alone.
const IDT_CAPTURE: usize = 1;

/// How long a capture may go without hearing anything before it gives up.
///
/// **The bound is on silence, not on the session.** Every `WM_CAPTURE`
/// re-arms it, because silence is precisely the symptom of the failure above
/// -- a hook Windows has quietly unhooked delivers nothing at all -- while a
/// user who takes twenty seconds to decide on a chord is not a failure and
/// should not be treated as one.
const CAPTURE_TIMEOUT_MS: u32 = 10_000;

/// The five controls that spell ONE shortcut: four modifier check boxes and
/// the key list.
///
/// One list because `apply_state` enables and disables all five together --
/// a combo the user can half-operate is not a combo, and a greyed key list
/// beside a live `Shift` box would say the row is editable when it is not.
const SHORTCUT_CONTROLS: [i32; 5] = [
    IDC_MOD_CTRL,
    IDC_MOD_WIN,
    IDC_MOD_ALT,
    IDC_MOD_SHIFT,
    IDC_COMBO,
];

/// Every `BS_PUSHBUTTON`/`BS_DEFPUSHBUTTON` in the window. Three things key
/// off exactly this set, which is why it is one list and not three:
///
/// 1. `BS_NOTIFY` is set on each of them -- without that style a button
///    never reports `BN_SETFOCUS` / `BN_KILLFOCUS` at all.
/// 2. `handle_command` migrates the default ring on those notifications.
/// 3. `handle_command` also drops every other non-click notification these
///    controls now emit. `BS_NOTIFY` widens what a button says to its
///    parent, and the per-id arms below match `(id, _)` -- ANY code -- so
///    without that filter merely tabbing onto Save would press it.
///
/// Every check box in the window is deliberately absent, and so are the
/// seven owner-draw chips: none carries `BS_NOTIFY`, none can be the default
/// button, and a default ring on either is not a thing Windows draws. On the
/// chips that absence does a second job -- without `BS_NOTIFY` an owner-draw
/// button emits only `BN_CLICKED` and `BN_DOUBLECLICKED`, which is exactly
/// the pair `is_chip_click` takes.
const PUSH_BUTTONS: [i32; 9] = [
    IDC_ADD,
    IDC_REMOVE,
    IDC_APPLY,
    IDC_OPENFILE,
    IDC_CLOSE,
    IDC_RELOAD,
    IDC_KEEPMINE,
    IDC_RECORD,
    IDC_RESET,
];

fn is_push_button(id: i32) -> bool {
    PUSH_BUTTONS.contains(&id)
}

/// The tab strip: each pill's control id, the door it opens, and its caption,
/// in the order they are created, drawn and Tab-navigated.
///
/// **One table, four readers**, for the reason `mod cap` gives for being one
/// table: `build_children` creates the four from it, `layout` places them from
/// it, `page_of_tab` turns a `WM_COMMAND` back into a door with it, and the
/// test below pins `tab_id_of` against it. The alternative is four lists that
/// agree on the day they are written, which is the shape `ids.rs`'s `MINE`
/// has -- and `MINE` needed a test reading the source to stay honest.
///
/// **The pills are deliberately absent from `PUSH_BUTTONS`.** Membership would
/// put them through `set_button_type`, a read-modify-write of
/// `BS_TYPEMASK_BITS` that would rewrite `BS_AUTORADIOBUTTON` (9) into
/// `BS_PUSHBUTTON` (0) the first time the default ring moved; and
/// `every_push_button_round_trips_through_the_default_button_enum` requires
/// every member to name a `DefaultButton`, which a tab must never be -- Enter
/// on a focused pill must not press it as if it were a command.
const TABS: [(i32, Page, &str); 4] = [
    (IDC_TAB_SHORTCUTS, Page::Shortcuts, cap::TAB_SHORTCUTS),
    (IDC_TAB_KEYBOARD, Page::Keyboard, cap::TAB_KEYBOARD),
    (IDC_TAB_SYSTEM, Page::System, cap::TAB_SYSTEM),
    (IDC_TAB_ABOUT, Page::About, cap::TAB_ABOUT),
];

/// Which door this pill opens, or `None` for every other control in the
/// window.
///
/// `handle_command` tests membership with this rather than listing the four
/// ids in its own pattern -- the shape `is_push_button` already establishes,
/// and the reason `TABS` is one table.
fn page_of_tab(id: i32) -> Option<Page> {
    TABS.iter().find(|(t, _, _)| *t == id).map(|(_, p, _)| *p)
}

/// The pill that stands for a door.
///
/// An exhaustive `match` rather than a lookup in `TABS`, so a fifth `Page`
/// is a compile error here instead of a silent fallback at the one call that
/// decides which pill is lit. That it agrees with `TABS` is a test, since
/// nothing else can check two spellings of the same fact.
fn tab_id_of(page: Page) -> i32 {
    match page {
        Page::Shortcuts => IDC_TAB_SHORTCUTS,
        Page::Keyboard => IDC_TAB_KEYBOARD,
        Page::System => IDC_TAB_SYSTEM,
        Page::About => IDC_TAB_ABOUT,
    }
}

/// Which door each control lives behind.
///
/// **Three kinds of control are absent, and each absence is a decision:**
///
/// - **The four pills and the command bar's three buttons are chrome.** They
///   are drawn on every page, so they belong to none, and listing them here
///   with a page would be a lie the first time someone read it.
/// - **The banner's three (`IDC_BANNER`, `IDC_RELOAD`, `IDC_KEEPMINE`) are
///   conditional twice over** -- `banner_shown` -- so a table that only knows
///   about pages would show them on Shortcuts whether or not the file moved.
///   `show_page_controls` handles them beside this loop, from the same
///   function core's `DefaultButton::visible` reads.
/// - **System and About own nothing yet.** Both draw as an empty surface
///   below the strip until Task 7 gives them a line each.
///
/// `every_control_belongs_to_exactly_one_group` in `ids.rs` is what keeps the
/// three absences honest: it partitions `MINE` across this table, the pills,
/// the banner and the command bar, and fails on any control that lands in
/// neither or in two. Without it, a control added later and forgotten here is
/// simply visible on all four pages -- which looks like a layout bug and is a
/// table bug.
const PAGE_CONTROLS: [(i32, Page); 26] = [
    // -- Shortcuts: the head row, the list, and the editor strip below it.
    (IDC_LBL_SECTION, Page::Shortcuts),
    (IDC_LBL_COUNT, Page::Shortcuts),
    (IDC_FILTER, Page::Shortcuts),
    (IDC_REMOVE, Page::Shortcuts),
    (IDC_ADD, Page::Shortcuts),
    (IDC_LIST, Page::Shortcuts),
    (IDC_GRP_EDITOR, Page::Shortcuts),
    (IDC_LBL_APP, Page::Shortcuts),
    (IDC_APP, Page::Shortcuts),
    (IDC_LBL_SHORTCUT, Page::Shortcuts),
    (IDC_MOD_CTRL, Page::Shortcuts),
    (IDC_MOD_WIN, Page::Shortcuts),
    (IDC_MOD_ALT, Page::Shortcuts),
    (IDC_MOD_SHIFT, Page::Shortcuts),
    (IDC_COMBO, Page::Shortcuts),
    (IDC_RECORD, Page::Shortcuts),
    (IDC_RESET, Page::Shortcuts),
    (IDC_NOTES, Page::Shortcuts),
    // -- Keyboard: the Caps line, and nothing else yet.
    (IDC_GRP_KEYBOARD, Page::Keyboard),
    (IDC_CAPS, Page::Keyboard),
    (IDC_LBL_HOLD, Page::Keyboard),
    (IDC_HOLD_CTRL, Page::Keyboard),
    (IDC_HOLD_WIN, Page::Keyboard),
    (IDC_HOLD_ALT, Page::Keyboard),
    (IDC_LBL_TAP, Page::Keyboard),
    (IDC_TAP, Page::Keyboard),
];

/// Show the controls `page` owns and hide every other page's.
///
/// **Pages HIDE, they are never destroyed**, and three things that keep
/// working on a hidden control would break on a destroyed one: `enable`,
/// `check` and `set_text_if_changed` all resolve through `GetDlgItem`;
/// `list_row_height` behind `Ui::shown_empty` needs the ListView to exist in
/// order to measure a row, and the ListView is off screen from three of the
/// four pages; and `IsDialogMessageW`'s `GetNextDlgTabItem` already skips
/// non-`WS_VISIBLE` controls, so hiding takes a page out of the tab order for
/// free.
///
/// Called by `show_page` on every switch and by `build_children` once, since
/// `show_page` cannot establish the INITIAL page -- it returns early on an
/// unchanged door, and `PAGE` already holds the one `open` asked for by the
/// time any control exists.
unsafe fn show_page_controls(hwnd: HWND, page: Page, external_change: bool) {
    for (id, owner) in PAGE_CONTROLS {
        if let Ok(h) = GetDlgItem(Some(hwnd), id) {
            show(h, owner == page);
        }
    }
    // The banner's three, from the same function `layout`'s card 0 and
    // core's `DefaultButton::visible` both read, so the announcement, its
    // card and its two buttons cannot disagree about whether it is up.
    let on = banner_shown(external_change, page);
    for id in [IDC_BANNER, IDC_RELOAD, IDC_KEEPMINE] {
        if let Ok(h) = GetDlgItem(Some(hwnd), id) {
            show(h, on);
        }
    }
}

/// Every operable control's caption, with its mnemonic.
///
/// **One table, because two call sites read it**: `build_children` creates
/// the control with it and `layout` MEASURES it to size the control's box.
/// A literal repeated in both is a button that silently stops fitting its
/// own caption the first time one of the two is edited.
///
/// **No two mnemonics collide, and that is a property of this table.**
/// Windows does not check, and a duplicate does not fail -- `Alt+R` simply
/// cycles focus between the claimants instead of pressing either, which
/// reads as "the keyboard is broken" rather than as a conflict. The
/// letters:
///
/// | Key | Control | Key | Control |
/// |---|---|---|---|
/// | `A` | Add | `R` | Reload |
/// | `M` | Re**m**ove | `K` | Keep mine |
/// | `U` | **U**se Caps Lock (check box) | `T` | C**t**rl (hold chip) |
/// | `C` | Close | `W` | **W**in (hold chip) |
/// | `O` | Open config file | `L` | A**l**t (hold chip) |
/// | `S` | **S**ave | `D` | Recor**d** |
/// | `E` | R**e**set | | |
///
/// **Mnemonic uniqueness is maintained by hand.** There is no test for it,
/// so verify by inspection before adding new captions.
///
/// `Record` and `Reset` are why this table has an awkward corner. `R` is
/// `Reload`'s, `S` is Save's, `T` is the Ctrl chip's, `O` is Open's and `C`
/// is Close's -- so between them the two captions have exactly two letters
/// left, `d` and `e`, and taking the obvious `e` for `Record` would leave
/// `Reset` with nothing (`r`, `e`, `s`, `t` are then all spoken for). Hence
/// `Recor&d` and `R&eset` rather than the other way round.
///
/// `Stop`, which `Record` reads while a capture is armed, deliberately
/// carries NO mnemonic and needs none: while armed the `WH_KEYBOARD_LL` hook
/// swallows every keystroke before it reaches a queue, so no `Alt`-anything
/// can reach this window at all. Esc, the mouse, losing focus and the
/// watchdog are the ways out.
///
/// `Remove` cannot take `R` because `Reload` has it, and `Reload` is the
/// one that appears without warning -- a banner the user did not ask for is
/// the worse place to make someone hunt for a letter. The two field labels
/// (`App`, `Shortcut`) deliberately carry NO mnemonic: a STATIC's mnemonic
/// moves focus to the next control in tab order, so each one would have to
/// hold a letter for a control that is already one Tab away.
///
/// **The editor strip's four modifier chips carry no mnemonic either**, and
/// that is this table's doing rather than an oversight. `Ctrl`, `Win` and
/// `Alt` name the same three modifiers the `Hold` chips do, and those
/// already hold `t`, `w` and `l` -- so the obvious letter is taken in every
/// case, and `Shift`'s `s` is Save's. The four sit between two `WS_TABSTOP`
/// controls on one line, which is one Tab each; a duplicate letter would
/// have cost the keyboard route on Save or on the Caps row to save that.
///
/// **The four tab pills carry none either, and there is no arrangement of
/// this table under which they could.** Spelled out at `cap::TAB_SHORTCUTS`:
/// the letters left over do not stretch to four unique ones, so the strip's
/// keyboard route is `Ctrl+1`..`Ctrl+4` rather than `Alt`-anything.
mod cap {
    pub const ADD: &str = "&Add";
    pub const REMOVE: &str = "Re&move";
    /// Was `Apply`. The id is still `IDC_APPLY` on purpose: 1002-1007 are
    /// hard-coded in `examples/settings_probe.rs`, which reads this button
    /// by id and `IsWindowEnabled` and never by caption, so renaming is
    /// free and renumbering would not be.
    pub const SAVE: &str = "&Save";
    pub const CLOSE: &str = "&Close";
    pub const OPEN_FILE: &str = "&Open config file";
    pub const RELOAD: &str = "&Reload";
    pub const KEEP_MINE: &str = "&Keep mine";
    pub const CAPS: &str = "&Use Caps Lock as a shortcut key";
    pub const HOLD: &str = "Hold";
    pub const TAP: &str = "Tap";
    pub const HOLD_CTRL: &str = "C&trl";
    pub const HOLD_WIN: &str = "&Win";
    pub const HOLD_ALT: &str = "A&lt";
    /// The editor strip's four modifier chips. NO `&` on any of them -- see
    /// the mnemonic table above, which is the only guard there is against a
    /// collision. `Win` rather than `Super`: the config file spells the key
    /// `super`, but nothing on a Windows keyboard is labelled that.
    ///
    /// These four and the three `HOLD_*` above are drawn as KEYCAPS by
    /// `draw_chip`, not written as text beside a tick. That is why the `&`
    /// rule matters more here than it looks: `draw_keycaps` measures a chip
    /// through `shown` and draws the raw caption, so an `&` added to one of
    /// these four would start rendering as an underline the mnemonic table
    /// says nothing owns.
    pub const MOD_CTRL: &str = "Ctrl";
    pub const MOD_WIN: &str = "Win";
    pub const MOD_ALT: &str = "Alt";
    pub const MOD_SHIFT: &str = "Shift";
    /// The editor strip's two commands. See the mnemonic table above for why
    /// the letters are `d` and `e` rather than the obvious ones.
    pub const RECORD: &str = "Recor&d";
    pub const RESET: &str = "R&eset";
    /// What `Record` reads while a capture is armed. No mnemonic -- see the
    /// table -- and deliberately NARROWER than `Record`, which is what makes
    /// it safe for `layout` to size the button once, from `RECORD`, and never
    /// run again when the caption flips. A wider armed caption would need
    /// `layout` on the capture path, and `layout` means `SetWindowPos` on the
    /// populated App combo: the measured data-loss call (`Ui::shown_external`).
    pub const STOP: &str = "Stop";
    /// The three `Tap` items, in `CB_ADDSTRING` order. Read back by INDEX
    /// with `CB_GETCURSEL`, never by text: even a `DROPDOWNLIST` has
    /// typeahead, which moves the selection.
    pub const TAP_ITEMS: [&str; 3] = ["Caps Lock", "Esc", "Nothing"];
    /// The filter box's placeholder. ASCII, like every display string.
    pub const FILTER_CUE: &str = "Filter";
    /// The editor group's caption in **two of its three states**. No row
    /// selected is `EDITOR_NONE`; a row with no app name yet is
    /// `EDITOR_UNNAMED`; a named row gets `Editing "<app>"`, which is built
    /// in `apply_state` rather than living here because it interpolates.
    ///
    /// **No `&` on either constant.** A group box caption's mnemonic moves
    /// focus to the next control in tab order, which is the same reason the
    /// `App` and `Shortcut` labels carry none -- and the collision table
    /// above has no room to spare. The third caption cannot rely on that,
    /// since the catalog supplies its text: `apply_state` doubles any `&`
    /// before writing it, and says there why.
    pub const EDITOR_NONE: &str = "No shortcut selected";
    pub const EDITOR_UNNAMED: &str = "Editing this shortcut";
    /// The four tab pills, in strip order. Read through `TABS`, which pairs
    /// each with its control id and its `Page`; they are here rather than
    /// inline in that table so this module stays what its own header says it
    /// is -- every operable caption, in one place, because `build_children`
    /// creates the control with it and `layout` measures it to size the
    /// control's box.
    ///
    /// **No `&` on any of the four, and that is arithmetic rather than
    /// taste.** The collision table above has `A M U C O S E R K T W L D`
    /// spoken for, leaving `{B,F,G,H,I,J,N,P,Q,V,X,Y,Z}`. Against that set
    /// `Shortcuts` can take only `h`, `System` only `y`, `About` only `b`,
    /// and `Keyboard` only `y` or `b` -- so `System` takes `y`, `Keyboard` is
    /// forced onto `b`, and `About` is left with nothing at all. Four unique
    /// mnemonics do not exist, which settles the question by counting rather
    /// than by taste (spec 3.3). `Ctrl+1`..`Ctrl+4` is the keyboard route
    /// instead.
    ///
    /// One of these repeats a string: `IDC_LBL_SECTION`'s caption is also
    /// `Shortcuts`, written as a literal at its creation and measured as one
    /// in `layout`. Two controls that happen to be named the same thing, not
    /// one caption used twice -- the heading names the card, the pill names
    /// the door, and either could be renamed without the other.
    pub const TAB_SHORTCUTS: &str = "Shortcuts";
    pub const TAB_KEYBOARD: &str = "Keyboard";
    pub const TAB_SYSTEM: &str = "System";
    pub const TAB_ABOUT: &str = "About";
}

/// A caption as the user SEES it: a lone `&` marks the mnemonic and is not
/// drawn at all, `&&` draws one literal ampersand.
///
/// `layout` measures through this rather than measuring the raw caption,
/// because the marker is not ink -- measuring it makes every button one
/// character wider than it needs to be, and the error grows with DPI.
fn shown(caption: &str) -> String {
    let mut out = String::with_capacity(caption.len());
    let mut chars = caption.chars();
    while let Some(c) = chars.next() {
        if c != '&' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('&') => out.push('&'),
            Some(next) => out.push(next),
            None => {}
        }
    }
    out
}

/// The window title, without the dirty mark: `beckon - <file name>`.
///
/// **ASCII hyphen, not an em-dash**: this window inherits the shell's text
/// face, and a glyph it does not carry draws as a box that reads like a
/// rendering bug rather than as information. beckon has been bitten by
/// exactly this once already: a UTF-8 em-dash written to a `serve --log`
/// came back as `?"` through Windows PowerShell 5.1's `Get-Content`.
///
/// The FILE NAME, not the path: `serve` can be pointed anywhere and nothing
/// on screen used to say where, but a full path in a title bar is truncated
/// from the right by every taskbar and Alt-Tab label there is -- i.e. it
/// loses precisely the file name it was there to show. The path goes in the
/// `Open config file` tooltip instead, where there is room for it.
fn title_base(config: &std::path::Path) -> String {
    match config.file_name() {
        Some(f) => format!("beckon - {}", f.to_string_lossy()),
        None => "beckon".to_string(),
    }
}

/// ListView columns, in order: title and text alignment.
///
/// **Widths are deliberately absent.** They are a proportion of the live
/// list width, computed once per `layout` from the control's own client
/// rect minus a scroll bar — which is what makes the §A.3 overflow
/// (561 px of columns inside a 482 px list) structurally impossible rather
/// than merely unlikely. Putting a width back here would reintroduce it.
///
/// `App` is column 0 and must stay left-aligned: comctl32 forces
/// `LVCFMT_LEFT` on subitem 0 of a report view whatever is asked for, so
/// only a later column can carry `LVCFMT_RIGHT`. Column 0 is also where
/// `LVS_EX_CHECKBOXES` puts the tick, which is a state image and not a
/// column — it survived the status column's deletion untouched.
const LIST_COLUMNS: [(&str, LVCOLUMNW_FORMAT); 2] =
    [("App", LVCFMT_LEFT), ("Shortcut", LVCFMT_RIGHT)];

/// A row's tick, as `LVIS_STATEIMAGEMASK` bits: the one-based index of the
/// state image, shifted up by 12. Image 1 is the empty box and image 2 the
/// ticked one; **0 means "no state image at all"**, which is what an item
/// inserted without `LVIF_STATE` gets -- and the `LVN_ITEMCHANGED` that
/// comctl32 then fires as it paints the first box (0 -> 1) is
/// indistinguishable from a user clicking a tick off.
///
/// This pair is also why `ListView_GetCheckState` is not ported: it is
/// `(state >> 12) - 1` on an *unsigned* value, so an item that never got a
/// state image reads back `0xFFFFFFFF` rather than `0`. Read
/// `LVM_GETITEMSTATE` masked by `LVIS_STATEIMAGEMASK` and compare against
/// these instead.
const LVIS_UNCHECKED: u32 = 1 << 12; // 0x1000
const LVIS_CHECKED: u32 = 2 << 12; // 0x2000

/// Window creation size, at 96 DPI. Shared between the initial
/// `CreateWindowExW` and the post-creation `SetWindowPos` correction (the
/// window is born on whichever monitor `CW_USEDEFAULT` picked, which
/// `GetDpiForWindow` can then reveal was guessed wrong) -- both must agree
/// on the un-scaled size or the correction would resize to the wrong target.
// **760x600, since the 2026-08-13 compaction pass.** The 900x740 derivation
// that stood here was for a window with 26 px rows and 16 px padding and was
// already marked superseded; a full derivation of a window that does not
// exist is worse than none, so it is gone rather than annotated again.
//
// What replaced it as evidence is better than a table: the window was built
// and run on a14 at 144 DPI and measured **1140 x 900** -- exactly 760 x 600
// scaled by 1.5 -- with all eight list rows present and no scroll bar.
//
// **The eight-rows half of that run no longer describes this window.** The
// tab strip's band (`tok::TABSTRIP_H`, added below) costs the list 34 px, and
// at 600 the cap lands at 178 against a `want` of 197 -- seven rows. The
// 1140 x 900 half is untouched: it measures the window against the constants,
// which is what it was run for, and the constants have not moved.
//
// **Which terms compose the height, in order** -- the part of the old block
// that was worth keeping, restated against the shipped tokens. This is a map
// of what a token change spends, not a claim about the total:
//
//   title bar (chrome::TITLEBAR_H)                     34
//   tab strip (tok::TABSTRIP_H)                        36
//   gap_card                                            8
//   card 0  banner -- NO height unless it is up      0/48
//           (plus one gap_card, 8, when it is)
//   card 1  Shortcuts: 2*CARD_PAD, head CTL, GAP,
//           header (~21) + ROWS * row (~22)           251
//   gap_card                                            8
//   card 2  editor: 2*CARD_PAD, caption s(24),
//           2*CTL, 2*GAP, notes_height, GAP    116 + notes
//   gap_card                                            8
//   card 3  keyboard: 2*CARD_PAD, caption s(24),
//           CTL, GAP                                   78
//   gap_card                                            8
//   command bar (CTL, not a card)                      26
//   pad                                                10
//
// **There is no frame term, and the list above is the whole window.**
// `chrome::nccalcsize` returns `LRESULT(0)` without calling `DefWindowProcW`
// and without reading either parameter (`chrome.rs`), so the proposed WINDOW
// rect is handed back untouched as the CLIENT rect: client == window on all
// four edges. See `MIN_HEIGHT` for the same point at length.
//
// **CORRECTED 2026-08-14.** This list carried a twelfth row, `frame, bottom
// only ("chrome::nccalcsize" gives the rest back to the client) 8`, and it
// was believed because it described the handler accurately until `c523e8e`
// (2026-08-13, "reclaim the whole frame, and hit-test the eight resize
// edges") -- before that commit `nccalcsize` restored only `.top` and the
// bottom edge really did stay non-client. Reading the shipped handler
// falsifies it. Every figure in the paragraph below moved with the row.
//
// `notes_height` is a live font measurement (`2 * Caption line + 4`), so it
// does not scale by 1.5 between DPIs and no total here is unconditional. The
// one conditional total worth writing down, since it is what the constant is
// answerable for: at 96 DPI with a 16 px Caption line -- `notes_height` 36 --
// and the banner down, the terms above sum to a **619** px window, so the
// shipped 600 is **19 px short** of a full eight rows. Re-derived from
// `compute_card_rects` for this comment, not carried over: its room-based cap
// for the list evaluates to `h - 386 - notes_h` = **178** px against a `want`
// of 197, which is seven whole rows and 3 px of an eighth.
//
// **That total was 585, and the slack was 15 px the other way, until the tab
// strip landed.** The band spends 34 -- `TABSTRIP_H` 36 plus a `gap_card` 8,
// less the `pad` 10 it displaced -- and the whole of it comes out of the
// list, because the list is the one figure in the window that flexes
// (`compute_card_rects`, and `MIN_HEIGHT` below at length). 600 is left
// deliberately: the design makes the list scroll, so the constant stops being
// answerable for showing every row. (Both figures were 8 lower again -- 593
// and 204 -- while the frame row stood.)
//
// `compute_card_rects` (`layout.rs`) is the arithmetic; this is a reading of
// it, and the direction of that dependency is not negotiable.
//
// **Which tokens the compaction pass moved.** All eight moved in one commit,
// `1f46335` (2026-08-13, "a tighter window, and transparency backed off to a
// hint"); read back from that commit and its parent rather than carried over:
//
//   tok::PAD            16 -> 10
//   tok::CARD_PAD       16 -> 11
//   tok::GAP_CARD       12 ->  8
//   tok::GAP             8 ->  6
//   tok::LABEL          12 -> 10
//   tok::CTL            32 -> 26
//   tok::ROW_H          26 -> 22
//   chrome::TITLEBAR_H  40 -> 34
//
// This list travelled inside the 900x740 derivation table and was deleted
// with it. Deleting the table was right -- a full derivation of a window that
// does not exist is worse than none -- but the list is not a derivation, it is
// a record of an event, and it had no other home: after the deletion
// `grep -rn "ROW_H 26" crates/ docs/` matched nothing. `Ui::shown_empty`'s
// comment is the worked example of what that costs. A pixel figure derived
// from `tok::ROW_H` has gone stale there twice now, once per move, and the
// second time there was nothing left in the tree to check it against.
const WINDOW_WIDTH: i32 = 760;
const WINDOW_HEIGHT: i32 = 600;

/// Minimum resize size, at 96 DPI, enforced in `WM_GETMINMAXINFO` through
/// `ptMinTrackSize` — so both are WINDOW dimensions, caption and frame
/// included, never client ones.
///
/// **This is no longer "the point where `layout` starts overlapping
/// controls".** Every subtraction in `layout` (and in `compute_card_rects`,
/// which now runs this arithmetic) is clamped, and card 1 gives up **its
/// own** height — `list_h`, the one flexing figure in the window — before
/// anything below it moves, so a window dragged past this floor produces a
/// list with fewer rows — eventually none — rather than two controls in the
/// same place. What the floor buys is that **the list is still worth
/// looking at**.
///
/// (`editor_min` is not that height and must not be read as it: it is what
/// card 1 RESERVES for card 2 before choosing `list_h`, and it equals
/// `card2_h`, card 2's own height — `CARD_PAD` included, unlike the
/// pre-Task-8 `grp_h` it replaced, because a card's footprint is its content
/// plus its own padding. The distinction earns its ink here because this
/// block is the derivation everything vertical is checked against.)
///
/// `MIN_WIDTH` is **660**, and it clears both zero points `layout` has --
/// the width at which the Shortcuts card's heading runs out, and the width
/// at which the editor card's key list does. Both are **raw client widths**,
/// ~364 and ~551, hand-traced through `layout` before the compaction pass.
/// That pass shrank the fixed overhead those traces were carrying, so the
/// real zero points are now lower than the two figures and each is a ceiling
/// on its own zero point rather than the zero point -- the safe direction.
/// Against 660 the margins are therefore **at least ~109 px** on the key list
/// and **~296 px** on the heading.
///
/// **The keyboard line is the one that does not clear it comfortably, and
/// this is the honest number.** A card's interior is
/// `w - 2*tok::PAD - 2*tok::CARD_PAD = w - 42`, so at 660 an interior is 618
/// px -- but the keyboard line is inset one `tok::GAP` at each end
/// (`inner_x` on the left, the `- gap` inside `tap_w` on the right), so the
/// run it actually gets is **606**. `"Use Caps Lock as a shortcut key"` plus
/// its chips was hand-measured at ~547 px, which leaves `IDC_TAP` about
/// **59 px** of it, against a 200 px ceiling. An earlier version of this
/// paragraph reported ~150 px, because it was computing against a `MIN_WIDTH`
/// of 753 that has not existed since the compaction pass -- and it compared
/// the line against `cw` (705 there) rather than against the run, so it was
/// over-generous by the two `CARD_PAD`s and one `gap` on top of that. 59 px
/// is enough to draw a combo and almost certainly not enough to draw
/// `Caps Lock` inside one. The ~547 is itself pre-compaction, and the
/// compaction narrowed the gaps inside that line, so it over-states the
/// current line by some unknown amount in the safe direction. **Gate G1
/// measures it with `GetTextExtentPoint32W`**; nothing here should be trusted
/// until it has.
///
/// **The frame eats none of `w`, and that is settled by reading rather than
/// by gate G3.** `chrome::nccalcsize` returns `LRESULT(0)` without calling
/// `DefWindowProcW` and without reading either parameter (`chrome.rs`), so
/// the proposed WINDOW rect survives untouched as the CLIENT rect on all four
/// edges. Every figure above therefore takes client == window on the
/// horizontal as fact, not as an assumption.
///
/// **CORRECTED 2026-08-14**: this paragraph used to end "yet `MIN_HEIGHT`
/// below still subtracts an 8 px bottom frame", and that contradiction is
/// what made it a gate. The 8 px was the stale half — it described
/// `nccalcsize` as it was before `c523e8e` (2026-08-13) reclaimed the whole
/// frame — and `MIN_HEIGHT` no longer subtracts it. With both halves agreeing
/// there is no question left for hardware to answer.
///
/// Two things G3 was carrying do survive it, and neither is this one:
/// **confirming the reading on the machine**, which costs one a14 run of
/// `examples/settings_probe.rs` — `measure_geometry` already prints
/// `GetClientRect` beside `GetWindowRect` and asserts a 0 px top inset, so
/// there is nothing to build; and **the look question**, which is new rather
/// than settled. The ~8 px strip at 96 DPI that `chrome::nchittest` still
/// answers as a resize direction is now painted, because it is client area:
/// the window ground runs under it, and so does `chrome::paint`'s title-bar
/// band, whose caption buttons are right-aligned flush against the client
/// edge — so the outermost ~8 px column of Close is inside `HTRIGHT`, and
/// `edge` is matched before `hit_button`. The cards themselves stay clear
/// (`tok::PAD` is 10 at 96 DPI and both scale with the same DPI), so nothing
/// is obscured; what is unknown is whether an invisible border painted like
/// ordinary window ground reads as one. Only a person looking at it can say.
///
/// **WITHDRAWN 2026-08-14: the four-row guarantee.** This paragraph read
/// "`MIN_HEIGHT` is derived, at 96 DPI, from the smallest client height at
/// which card 1's list still shows **four** rows — half of `tok::ROWS` —
/// **with the external-change banner up**. Four is enough to see a selection
/// with a row of context above and below it; a window whose list shows one
/// row is not a smaller version of this window, it is a broken one."
///
/// The standard in that last sentence stands and is not what changed. What
/// changed is underneath it: the tab strip takes 34 px out of the list
/// (`compute_card_rects`' `y`), which leaves the floor two rows rather than
/// four — and design §4 makes the list **short and scrolling** instead of a
/// list the window grows to fit. Once it scrolls, a floor's job stops being
/// "enough rows to see context" and becomes "enough rows to see that it is a
/// list". Two rows plus a scrollbar meets that; one row does not. So the
/// constant keeps its meaning while its derivation changes, and it is
/// withdrawn here in writing rather than left to be discovered as a window
/// that no longer does what its own comment claims.
///
/// The alternatives were costed and rejected in the spec (§2.3), and both of
/// its figures move when re-derived from the table below — check them before
/// reopening either. Raising the floor to keep four rows spends draggability
/// on a promise design §4 has already retired; the spec puts that floor at
/// 596, and solving the table for four rows gives **587**. Waiting instead
/// for the Shortcuts workstream to return the editor's `Editing "…"` caption
/// line (the `s(24)` inside `grp_content_h`) couples two landings that are
/// otherwise independent; the spec says 572 then suffices, and the same solve
/// with that `s(24)` struck out gives **563**. Neither gap changes which
/// option was taken — both lose on their own terms rather than on a pixel —
/// but neither of the spec's two numbers should be quoted onward as measured.
///
/// ```text
///   Derived from `compute_card_rects` (`layout.rs`) at 96 DPI, banner UP,
///   with the shipped tokens. Solving that function for the client height
///   `h` at which the list gets exactly two rows:
///
///     bar_y     = h - PAD - CTL                       = h - 36
///     kb_card_h = 2*CARD_PAD + (24 + CTL + GAP)       = 78
///     kb_y      = bar_y - GAP_CARD - kb_card_h        = h - 122
///     card2_h   = 2*CARD_PAD + (24 + 2*CTL + 2*GAP
///                 + notes_h + GAP)                    = 116 + notes_h
///     y0        = TITLEBAR_H + TABSTRIP_H + GAP_CARD  = 78
///     card0     = 2*CARD_PAD + CTL = 48, so y         = 134
///     list_top  = y + CARD_PAD + CTL + GAP            = 177
///     room      = kb_y - GAP_CARD - list_top          = h - 307
///     list_h    = room - GAP_CARD - CARD_PAD - card2_h
///               = h - 442 - notes_h
///
///   Two rows is `list_header_height` (21) + 2 * `list_row_height` (22)
///   = 65, and `notes_h` is 36 when the Caption line is 16 px, so
///
///     h = 442 + 36 + 65 = 543  client == window (see below)
/// ```
///
/// **543, and the constant stays 560.** The spec (§2.3) puts it as "560 is
/// where two rows stop fitting"; solving the function says 543 is, and 560
/// clears two rows by 17 px. The decision the spec was recording is
/// unaffected — nothing here argues for lowering the floor to its exact
/// two-row point, any more than the previous derivation argued for 553 — but
/// the sentence is off by 17 px and this is the file that has to be right
/// about it. Three rows need 87, so 560 misses a third by 5.
///
/// **The client rect IS the window rect, so there is no frame term.**
/// `chrome::nccalcsize` returns `LRESULT(0)` without calling `DefWindowProcW`
/// (`chrome.rs:142`), which leaves the proposed WINDOW rect untouched as the
/// client rect on all four edges — its own comment says so, and the window
/// carries no `WS_CAPTION` (`WS_POPUP | WS_SYSMENU | WS_THICKFRAME |
/// WS_MINIMIZEBOX`, the `CreateWindowExW` below).
///
/// **CORRECTED 2026-08-14.** Until `0098457` the table added `+ 8  bottom
/// frame` here and concluded the shipped 560 was one pixel short of four
/// rows. Both were wrong, and the error was inherited rather than invented:
/// the `+ 8` and its justification ("`nccalcsize` hands the whole caption
/// back to the client, so only the bottom edge remains non-client") describe
/// the handler as it was BEFORE `c523e8e` reclaimed the whole frame and moved
/// the eight resize directions into `chrome::nchittest`. With no frame term
/// the floor got `list_h = 560 - 408 - 36 = 116` against the 109 four rows
/// need — it cleared them by 7 px rather than missing by 1. That is the
/// window BEFORE the tab strip; the strip then spent 34 of it, which is the
/// other half of why four rows are gone and is not a reason to re-open this
/// half.
///
/// The floor's margin is now **17 px** — `list_h = 560 - 442 - 36 = 82`
/// against the 65 two rows need — and `notes_h`'s honest error is what eats
/// into it, since that is a live font measurement: `notes_h = 2L + 4`, so
/// every extra pixel of Caption line `L` costs the list two. `L = 24` leaves
/// one pixel of the seventeen; `L = 25` takes the second row. **Nothing
/// on the machine this was derived on can display the window**, and
/// `examples/settings_probe.rs`'s `measure_geometry` already prints
/// `GetClientRect` beside `GetWindowRect` with a verdict, so the reading
/// above costs one a14 run to confirm rather than a new probe.
///
/// The two row figures are `list_row_height` / `list_header_height`'s own
/// 96-DPI fallbacks (`tok::ROW_H` and a literal 21). They are the honest
/// numbers to derive from: comctl32 picks the real ones from the live font
/// at the live DPI, which is exactly why neither is a token.
///
/// **Card 0 is in the table, and that is what the number is for.** The
/// banner contributes no height until the config file moves under us, so
/// reserving its `CARD_PAD*2 + ctl` costs 48 px of floor — 56 with the
/// `gap_card` that follows it — for a state that is normally absent. But the
/// state it pays for is exactly the one in which the window is least
/// disposable, and the pre-Task-8 alternative was measured: at a floor
/// derived without the banner, raising it took the list from four rows to
/// one. Nothing overlapped there; the failure was a useless window, not a
/// corrupt one, and that is the standard this constant is held to.
///
/// **What the floor and the shipped size actually buy, re-traced through
/// `compute_card_rects` for this pass rather than carried over.** With the
/// banner down the stack starts 56 px higher, so the list's cap is
/// `h - 386 - notes_h` instead of `h - 442 - notes_h`, and `want` is
/// `21 + 8*22 = 197`:
///
/// - at the floor (client 560, which IS `MIN_HEIGHT` — see above), banner up:
///   82 px, two whole rows and 17 px of a third;
/// - at the floor, banner down: 138 px, five whole rows and 7 px of a sixth;
/// - at `WINDOW_HEIGHT` (client 600, likewise), banner down: the cap is 178,
///   19 px **below** `want`, so it binds — seven whole rows and 3 px of an
///   eighth. The list no longer reaches `tok::ROWS` at the shipped size, and
///   that is the strip being paid for rather than a regression: design §4
///   makes the list scroll.
///
/// **CORRECTED 2026-08-14** — all three bullets moved twice in one day, and
/// the record of the first move is worth as much as the numbers. They read
/// `client 552`, `client 592`, `108`, `164` and `204` while an 8 px bottom
/// frame was subtracted from the constant beside each; that subtraction
/// described `chrome::nccalcsize` as it was before `c523e8e` (2026-08-13),
/// and removing it (`0098457`) made the client heights the constants
/// themselves and every cap 8 px larger — 116 / 172 / 212. The tab strip then
/// took 34 off all three. The first bullet's "the one-pixel shortfall above"
/// went with the first move and has not come back.
///
/// The last of those is the same 19 px `WINDOW_HEIGHT`'s own comment reports
/// as a shortfall, measured the other way, and it is a property of these
/// particular numbers rather than anything designed in — a future change to
/// `notes_height`, `card2_h` or the row/header fallbacks moves it in either
/// direction — so re-check it by the same hand trace rather than assuming it
/// survives. Simulated, not seen: nothing on the machine this was written
/// on can display the window.
const MIN_WIDTH: i32 = 660;
const MIN_HEIGHT: i32 = 560;

/// §B.3's type roles. The seven roles — Title, Subtitle, BodyStrong, Body,
/// Caption, Keycap, Chrome — map to five visual levels (Title, Subtitle, Body,
/// Caption/Keycap, Chrome). Keycap serves keycap rendering in the editor
/// strip and shortcut list; Title and Chrome serve the client-drawn title
/// bar `chrome::paint` draws (Task 7).
#[derive(Clone, Copy)]
enum Role {
    /// The title-bar app name. Read by `chrome::paint`.
    Title,
    Subtitle,
    /// Card captions, the ListView column headers, and the `Save` caption.
    BodyStrong,
    Body,
    Caption,
    /// Keycap rendering in the editor strip (modifier chips, Tap combo) and
    /// the shortcut list column. 11 px semibold, matching keycap design
    /// guidelines.
    Keycap,
    /// The two caption-button glyphs. Read by `chrome::paint`.
    Chrome,
}

/// Which role a control takes, keyed on its id.
///
/// **This is the single mapping**, consulted by the creation path (`child`,
/// which every control in the window goes through) and by the
/// `WM_DPICHANGED` rebroadcast (which walks `GW_HWNDNEXT` and asks
/// `GetDlgCtrlID`). Those two must not each hold an opinion, for the same
/// reason `cells()` is the one funnel for column text: the second copy is
/// the one that drifts.
fn role_of(id: i32) -> Role {
    match id {
        // The one band heading. Subtitle exists so the list reads as a
        // section of the window rather than as the whole of it.
        IDC_LBL_SECTION => Role::Subtitle,
        // Card captions and the Save caption. `IDC_GRP_EDITOR` /
        // `IDC_GRP_KEYBOARD` are the two card heads -- reclassed from
        // `BS_GROUPBOX` to a plain caption `STATIC` in Task 8's review pass
        // (see `child`'s creation calls for both ids): a themed group-box
        // frame nested inside the new rounded `card()` background drew as
        // two frames around one set of controls, and the fix is a coordinate
        // shift plus a control-class change, not a renumbering -- both ids
        // are unchanged, and `settings_probe` still reads their caption with
        // `WM_GETTEXT`, which a `STATIC` answers identically to a `BUTTON`.
        // `IDC_APPLY` reads its font through this same mapping even though
        // it is custom-drawn -- `paint::button` asks the button for its own
        // `WM_GETFONT` rather than picking a role directly, so this arm is
        // the only place its weight is decided. The ListView's OWN column
        // headers are a comctl32-owned Header control, never a child of
        // `hwnd` and therefore never routed through `role_of` at all --
        // `build_children` and `WM_DPICHANGED` each set that font directly.
        IDC_GRP_EDITOR | IDC_GRP_KEYBOARD | IDC_APPLY => Role::BodyStrong,
        // Secondary prose, at Caption size. The banner is deliberately NOT
        // here: it announces that the file moved under us, which is the
        // least appropriate text in the window to shrink. `IDC_LBL_COUNT`
        // joins because B draws the count small and grey beside a Subtitle
        // heading -- one STATIC has one font, which is the whole reason it
        // is a second control.
        IDC_NOTES | IDC_LBL_COUNT => Role::Caption,
        // The three `Hold` chips (`Caps+<key>`'s modifier row), moved off
        // `Role::Body` in Task 8. `layout`'s `chip_kc` measures them in this
        // same font -- the draw font and the measuring font move together,
        // or a chip is sized for 14 px Body text and drawn with 11 px
        // Keycap text. The FOUR chips in the editor strip (`IDC_MOD_CTRL`
        // etc.) stay on `Role::Body` deliberately -- Task 8's brief names
        // only the `Hold` chips, and `layout`'s plain `chip`/`tw` still
        // measure those four.
        IDC_HOLD_CTRL | IDC_HOLD_WIN | IDC_HOLD_ALT => Role::Keycap,
        // Everything the user reads or operates: the ListView, the filter
        // EDIT, the App / key / Tap COMBOBOXes, their labels, every BUTTON
        // (push and check), the banner -- and anything added later that does
        // not say otherwise. No group box is left in the window as of the
        // reclass above.
        _ => Role::Body,
    }
}

/// The seven live `HFONT`s. `Copy`, so `LayoutHandles` stays `Copy` and the
/// abort-class rule below keeps holding.
#[derive(Clone, Copy)]
struct Fonts {
    title: HFONT,
    subtitle: HFONT,
    body_strong: HFONT,
    body: HFONT,
    caption: HFONT,
    keycap: HFONT,
    chrome: HFONT,
}

impl Fonts {
    fn get(self, role: Role) -> HFONT {
        match role {
            Role::Title => self.title,
            Role::Subtitle => self.subtitle,
            Role::BodyStrong => self.body_strong,
            Role::Body => self.body,
            Role::Caption => self.caption,
            Role::Keycap => self.keycap,
            Role::Chrome => self.chrome,
        }
    }

    fn for_id(self, id: i32) -> HFONT {
        self.get(role_of(id))
    }

    /// Release all seven.
    ///
    /// Only ever called AFTER the controls have been told about their
    /// replacements -- deleting a font that is still selected into a DC is
    /// undefined. Landing 1 established this discipline for one font
    /// because one `HFONT` was leaking per window open; seven roles means
    /// seven leaks if only one of them is freed.
    ///
    /// Deduplicated because the total-failure path hands every role the
    /// same stock handle. `DeleteObject` on a stock object is documented
    /// harmless, but "harmless twice" is not a property worth relying on.
    unsafe fn delete(self) {
        let all = [
            self.title,
            self.subtitle,
            self.body_strong,
            self.body,
            self.caption,
            self.keycap,
            self.chrome,
        ];
        for (i, f) in all.iter().enumerate() {
            if f.is_invalid() || all[..i].iter().any(|p| p.0 == f.0) {
                continue;
            }
            let _ = DeleteObject(HGDIOBJ(f.0));
        }
    }
}

/// Scales a 96-DPI value to `dpi`. The only scaling rule in this file --
/// `MulDiv` (round-half-up) was tried for the creation size and the list
/// columns and dropped, because it quietly disagrees with this truncating
/// formula at in-between DPIs (at 125%: `10 * 120 / 96 == 12` here, but
/// `MulDiv(10, 120, 96) == 13`). `layout`'s own `s` closure computes the
/// same thing inline, for a value it already has in scope.
fn scale(v: i32, dpi: u32) -> i32 {
    v * dpi as i32 / 96
}

/// The same colour at `num/den` of its own brightness, per channel.
///
/// **Used for exactly one thing: the bottom edge of an ARMED keycap**, which
/// direction B draws as a darker shade of the fill (`#2563eb` face over a
/// `#1d4fc4` edge -- about four fifths, which is where the ratio comes from).
///
/// **Derived, never a literal.** The face is `COLOR_HIGHLIGHT`, i.e. whatever
/// accent the user picked, so the edge has to be computed from it: a fixed
/// dark blue under a green or magenta accent is not a shadow, it is a
/// mistake. `COLORREF` is `0x00BBGGRR`, so the channels come out in that
/// order.
fn shade(c: COLORREF, num: u32, den: u32) -> COLORREF {
    let ch = |sh: u32| ((c.0 >> sh) & 0xFF) * num / den;
    COLORREF(ch(0) | (ch(8) << 8) | (ch(16) << 16))
}

/// `num/den` of `a` blended with the rest of `b`, per channel. `shade` above
/// answers "the same colour, darker"; this answers "partway between two
/// DIFFERENT colours" -- the Shortcut column's hover tint (Task 10),
/// `accent_soft` blended toward `card` so a hovered row reads as a hint
/// rather than the stronger, unblended fill a selected row gets.
///
/// **Never called under high contrast.** Blending two arbitrary
/// `GetSysColor` answers has no guaranteed relationship to anything -- the
/// same high-contrast collision this window has already shipped three
/// times, just reached through a blend instead of a mismatched `sys` index.
/// Callers gate on `high_contrast()` themselves rather than this function
/// gating on it, because there is no single safe fallback colour to hand
/// back instead: falling through to whichever of `a`/`b` the caller means as
/// "resting" is a decision only the caller can make.
fn blend(a: COLORREF, b: COLORREF, num: u32, den: u32) -> COLORREF {
    let ch = |sh: u32| {
        let ca = (a.0 >> sh) & 0xFF;
        let cb = (b.0 >> sh) & 0xFF;
        (ca * num + cb * (den - num)) / den
    };
    COLORREF(ch(0) | (ch(8) << 8) | (ch(16) << 16))
}

/// Everything the window reports back. The caller owns all policy: what an
/// edit means, whether a close is allowed, what Apply writes.
///
/// Defined in `beckon_core::settings` and re-exported here so the macOS
/// window implements the same contract and `serve.rs` builds one set. Two
/// notes that are Win32-specific and so did not travel with it:
///
/// - `on_probe_shortcut` is also NOT sent by `commit_fields` (an App-field
///   focus loss, a Save), where the chord has not moved and there is
///   nothing to find out; `apply_state` pushes data on every keystroke,
///   which `push_shortcut`'s `suppressed()` guard keeps out of there.
/// - Nothing is sent while a key is not selected, exactly as
///   `on_edit_combo` is not -- see `shortcut_shown`.
pub use beckon_core::settings::Callbacks;

struct Ui {
    hwnd: HWND,
    list: HWND,
    combo: HWND,
    app: HWND,
    notes: HWND,
    filter: HWND,
    banner: HWND,
    reload: HWND,
    keep: HWND,
    /// The keyboard row's `Tap` combo. Kept here rather than fetched with
    /// `GetDlgItem` per use so `apply_state` and `handle_command` each read
    /// it out of the ONE borrow they already take -- see `LayoutHandles` for
    /// why a second borrow is not merely untidy.
    tap: HWND,
    /// The three type roles, rebuilt on every `WM_DPICHANGED` and freed on
    /// `WM_DESTROY`. Which control uses which is `role_of`'s answer, never
    /// a decision taken at a call site.
    fonts: Fonts,
    /// `Ctrl+S`, and nothing else -- Enter and Esc are the dialog manager's
    /// (`DM_GETDEFID` and `IDCANCEL`), not this table's. Created in
    /// `build_children` and destroyed in `WM_DESTROY`: an accelerator table
    /// is a system resource with the same lifetime discipline as the
    /// `HFONT`s beside it, and Landing 1 had to close a one-per-open leak of
    /// those already.
    accel: HACCEL,
    /// Which button Enter presses, and therefore which one wears the ring.
    /// `IDC_APPLY` at rest; `set_default_id` moves it to whichever push
    /// button has focus and back again when that focus goes.
    ///
    /// A real dialog keeps this inside `DefDlgProc`, which migrates it by
    /// sending itself `DM_SETDEFID` as focus moves. This window is not a
    /// dialog box, so it keeps the field and answers both dialog-manager
    /// messages itself -- and it MUST, because `IsDialogMessageW` only
    /// activates the focused control on Enter when that control answers
    /// `WM_GETDLGCODE` with `DLGC_DEFPUSHBUTTON`. A plain `BS_PUSHBUTTON`
    /// answers `DLGC_UNDEFPUSHBUTTON` instead, so Enter falls through to
    /// `DM_GETDEFID` -- and a fixed answer there meant Enter on Close SAVED,
    /// and Enter on the external-change banner's Reload overwrote the very
    /// edit the banner had appeared to warn about.
    defid: i32,
    /// `beckon - <file name>`, computed once at creation. The `*` prefix is
    /// added per push; this half never changes, because `serve` cannot be
    /// repointed at another file while its window is open.
    title_base: String,
    /// The full config path, kept alive because the tooltip holds a POINTER
    /// to it rather than a copy (`TTM_ADDTOOLW` stores `lpszText`). Moving
    /// the `Vec` into this struct does not move its heap buffer, so the
    /// pointer handed to comctl32 in `build_children` stays valid.
    tip_text: Vec<u16>,
    /// The dirty state the title bar currently shows, so `apply_state` --
    /// which runs on every keystroke -- only rewrites the caption when the
    /// mark actually flips. `None` until the first push.
    shown_dirty: Option<bool>,
    /// The banner visibility the CURRENT layout was computed for, so
    /// `apply_state` re-runs `layout` only when the geometry can actually
    /// have changed. `None` until the first push, which therefore always
    /// lays out.
    ///
    /// **This is a correctness guard, not an optimisation, and removing it
    /// reintroduces a measured data-loss bug.** `layout` re-places the App
    /// COMBOBOX with `SetWindowPos`, and a populated combo box responds to
    /// being resized by re-synchronising its edit field to the closest
    /// matching item in its list -- so a `SetWindowPos` on the keystroke path
    /// silently replaced what the user had typed with a catalogue entry.
    /// Measured on a14 (comctl32 6.16, 121 items): typing `N` left `N` in the
    /// model and `Narrator` on screen, 2.8 ms later, inside `apply_state`
    /// itself. See `docs/superpowers/measurements/2026-08-11-landing-1-a14.md`
    /// section 24 for the bisect that pinned it to this one call.
    ///
    /// **CORRECTED 2026-08-14: `layout`'s output depends on SIX things, not
    /// five.** This paragraph listed the client rect, the DPI, the banner's
    /// visibility, whether the list has any rows in it, and the list's own
    /// client width, which shrinks by `SM_CXVSCROLL` the moment the item
    /// count crosses the page size and comctl32 grows a vertical scroll bar.
    /// The tab strip added a sixth, the PAGE: `layout` places only the
    /// current page's controls and `compute_card_rects` gives every other
    /// page's card a zero-height rect. See `Ui::shown_page`.
    ///
    /// The first two arrive as `WM_SIZE` / `WM_DPICHANGED`, which call
    /// `layout` directly and still do. Three can change on a data push, so a
    /// push watches all three: this field, `shown_empty` and `shown_page`.
    ///
    /// **The list's client width is deliberately NOT guarded**, and the
    /// reason it is safe to leave unguarded is written out at its own site --
    /// see the column sizing in `layout`. In one sentence: the error it
    /// produces is always a gutter and never a clipped column, and buying it
    /// back would mean running `layout`, and therefore `SetWindowPos` on the
    /// populated App combo, on more data pushes than these three fields
    /// already allow -- trading a cosmetic stale margin for a re-entry into
    /// the measured data-loss path above.
    shown_external: Option<bool>,
    /// Whether the list was EMPTY when the current layout was computed, for
    /// the same reason `shown_external` exists: it is the fourth of `layout`'s
    /// six inputs, and skipping a layout that one of them has invalidated
    /// leaves stale geometry on screen. (The list's own client width is
    /// tolerated rather than guarded -- see `shown_external`; the page is
    /// guarded, by `shown_page`.)
    ///
    /// The path runs through `list_row_height`, which cannot measure a row
    /// that is not there and returns `scale(tok::ROW_H, dpi)` when the list
    /// is empty -- `scale(22, dpi)`, 33 px at a14's 144 DPI.
    ///
    /// **CORRECTED TWICE, for the same reason both times.** It first read
    /// `scale(20, dpi)` / "30 px ... against 29 measured" / "~8 px taller",
    /// three numbers that were right while `tok::ROW_H` was 20 and went
    /// stale when Task 10 raised it to 26. The re-derivation that fixed
    /// that wrote `scale(26, dpi)` / "39 px", which went stale in turn on
    /// 2026-08-13 when the compaction pass (`1f46335`) took `tok::ROW_H`
    /// from 26 to 22 -- see the token record beside `WINDOW_HEIGHT`, which
    /// exists because of this. The recurring cause is not carelessness in
    /// either pass: it is a figure DERIVED from a token, written out as a
    /// literal, in a comment nothing compiles and no test reads. `scale` is
    /// `v * dpi / 96` (`fn scale`, mod.rs:916), so `tok::ROW_H` and this are
    /// one edit apart and nothing links them -- moving the token leaves the
    /// prose silently wrong. The second time round there was also nothing
    /// left to check it against: `grep -rn "ROW_H 26" crates/ docs/` matched
    /// NOTHING, because the one place that recorded the 26 was the 900x740
    /// derivation table, deleted in the same pass. Re-derive rather than
    /// nudge, and expect to be back.
    ///
    /// The 33 px figure is also not directly comparable to a "measured"
    /// figure the way the first version of this text implied: since Task 10,
    /// `rebuild_state_image_list`'s state image list FORCES the live row to
    /// be at least `scale(tok::ROW_H, dpi)`, but comctl32 is still free to
    /// pad further on top of that image height (`list_row_height`'s own
    /// doc), and no hardware measurement of the live, non-empty row exists
    /// yet (Gate 05, `NOT YET RUN`). So a window opened on a config with no
    /// shortcuts lays out against a LOWER BOUND that may or may not equal
    /// the true row height, and without this field the first Add would keep
    /// whatever the fallback got wrong: `external_change` does not move, the
    /// layout is skipped, and the list is left at whatever height the
    /// fallback produced rather than the one the real rows need.
    ///
    /// **Where a too-short fallback's slack goes changed with Task 9, and
    /// the guard is what stops it mattering more, not less.** A shortfall
    /// used to be absorbed by the notes strip, which flexed into whatever
    /// the bands above left; the notes are a fixed line inside the editor
    /// group now (`notes_height`), so nothing absorbs anything -- any gap
    /// between the fallback and the true row height pushes `y`, therefore
    /// `grp_y`, therefore the whole editor group, eating slack above the
    /// keyboard group and, near `MIN_HEIGHT`, running into `y.min(kb_y)`.
    /// The other reason it is guarded rather than tolerated is unchanged:
    /// `list_row_height`'s own comment used to justify the fallback by
    /// saying `apply_state` re-lays-out the instant a row appears, which
    /// `shown_external` made false.
    ///
    /// Empty-vs-not is the whole condition: every non-empty list measures the
    /// same row, so no other transition changes the answer.
    shown_empty: Option<bool>,
    /// Which door the CURRENT layout was computed for -- the third guard,
    /// beside `shown_external` and `shown_empty`, and the third of `layout`'s
    /// inputs a data push can invalidate.
    ///
    /// **`layout` has six inputs now, not five.** `Ui::shown_external`
    /// enumerates them and its list is the one being extended: the client
    /// rect, the DPI, the banner's visibility, whether the list is empty,
    /// the list's own client width (deliberately unguarded, argued at the
    /// column sizing in `layout`) -- and now the page, because `layout`
    /// places only the current page's controls and gives every other page's
    /// card a zero-height rect.
    ///
    /// It is guarded rather than tolerated for the same reason
    /// `shown_external` is: without it a page switch that arrives through
    /// `apply_state` -- a file-watch tick or a catalog landing in the same
    /// instant as a `Ctrl+Tab` -- would skip the layout and leave the
    /// previous page's geometry on screen. `show_page` writes this field
    /// itself after its own `layout`, so the ordinary switch is not laid out
    /// twice.
    ///
    /// `None` until the first push, like its two neighbours, which therefore
    /// always lays out.
    shown_page: Option<Page>,
    /// Set while `apply_state` is writing control contents, so the
    /// `EN_CHANGE`/`CBN_EDITCHANGE` those writes generate are not mistaken
    /// for the user typing. Without it, every repaint would feed the old
    /// text straight back into the model and mark it dirty.
    suppress: bool,
    /// Last state pushed, so the banner's visibility can be recomputed
    /// without asking the caller again.
    external_change: bool,
    /// The rows currently in the ListView, exactly as `apply_state` last
    /// pushed them. `apply_state` diffs the next snapshot against this
    /// instead of deleting and reinserting, which is what stops a
    /// keystroke from wiping the ticks and scrolling back to the top.
    ///
    /// Never read while a message is in flight: every use takes it out of
    /// the `RefCell` first (`mem::take`), so an empty vector means "the
    /// control's contents are unknown" and the next push rebuilds -- which
    /// is always correct, just slower.
    items: Vec<ListItem>,
    /// Which deferred read of the App combo box is still wanted. Bumped in
    /// two places, and both are load-bearing:
    ///
    /// 1. **`post_app_read`**, so that of several keystrokes queued before
    ///    any of them is dispatched only the LAST is honoured. The earlier
    ///    reads would return the same final text anyway; dropping them saves
    ///    a full `apply_state` per character.
    /// 2. **`apply_state`, whenever it writes the App field itself.** This
    ///    is the suppression hole `suppress` alone cannot close. `suppress`
    ///    covers the window during which `apply_state` is writing, and a
    ///    posted message cannot be dispatched inside that window -- nothing
    ///    `apply_state` calls pumps the queue. What it cannot cover is a
    ///    read posted BEFORE a push and dispatched AFTER it: by then
    ///    `suppress` is false again, and the field holds what `apply_state`
    ///    put there rather than what the user typed. Feeding that back would
    ///    report a programmatic write as a user edit.
    ///
    /// A push that leaves the field alone does NOT bump, so an ordinary
    /// keystroke -- where the model has already caught up and `apply_state`
    /// finds the text it wanted is already there -- keeps its pending read.
    app_epoch: u32,
    /// The selected row's combo string exactly as `apply_state` last wrote
    /// it to the five shortcut controls -- `st.detail.map(|d| d.combo)`,
    /// `None` when there is no row. `commit_fields` compares this, as a
    /// `ComboView`, against what the controls show now, so it can tell "the
    /// user actually changed the shortcut" apart from "an unrelated commit
    /// re-read five controls that still say what they were told to say".
    /// See `commit_fields` for why a string compare there is wrong.
    shown_combo: Option<String>,
    /// The recording session, as far as DRAWING is concerned. `None` is
    /// Idle.
    ///
    /// **This is not the same lifetime as the hook's.** `caps_hook` keeps
    /// the hook past a commit or a cancel, draining until every physically
    /// held key is up (spec F.3) -- that is what makes `alt+tab` recordable
    /// without the system seeing a bare Alt-up. This field ends the moment
    /// the chord is decided, because that is when the typed path comes back
    /// and the strip stops saying `Stop`. `caps_hook::capture_armed()` is
    /// the hook's answer; this one is the window's.
    ///
    /// `apply_state` reads it, which is what stops an unrelated push --
    /// a file-watch tick, a catalog arriving -- from re-enabling the five
    /// typed controls underneath a live capture. Two writers on one value is
    /// exactly what spec C.4 forbids.
    capture: Option<Capture>,
    /// The OS's current light/dark/high-contrast answer, and the brushes it
    /// implies. Resolved once at creation and re-resolved on
    /// `WM_SETTINGCHANGE`/`WM_THEMECHANGED`; see `on_theme_changed` below.
    /// Painting code never reads this field -- see `PAINT_THEME`, which is
    /// kept in step with it at both points it rebuilds.
    theme: theme::ThemeCache,
    /// Which title-bar caption button the cursor is over -- `HTCLOSE` or
    /// `HTMINBUTTON`, or `None` -- kept here rather than recomputed in
    /// `chrome::paint` because Windows already hands it to `WM_NCMOUSEMOVE`
    /// as `wParam`, and `chrome`'s own rule is that it never reads `UI`. The
    /// `WM_NCMOUSEMOVE` / `WM_NCMOUSELEAVE` arms write it and repaint only
    /// the bar; `chrome::paint` reads it by value, once per call.
    hot: Option<i32>,
}

/// What a live capture has to say, rebuilt on every `WM_CAPTURE`.
///
/// Both strings are built HERE, on the UI thread, never in the hook
/// callback: `CaptureState::partial` and `capture::hint` allocate, and a
/// `WH_KEYBOARD_LL` callback that overruns `LowLevelHooksTimeout` is
/// unhooked by Windows with no error anywhere.
struct Capture {
    /// The modifiers held so far, canonically ordered -- `ctrl+super+...` --
    /// exactly as they would be written to the TOML. `None` when nothing is
    /// held, which is the ordinary state right after arming.
    partial: Option<String>,
    /// The line under it: `HINT_ARMED` at rest, a refusal's sentence
    /// otherwise. Never empty.
    hint: String,
    /// The vk the last beep was for.
    ///
    /// A refused key never enters `CaptureState`'s held set -- admitting it
    /// would let rolled-over bare keys eat the twelve fixed slots and drop a
    /// real modifier -- so the auto-repeat filter cannot see it, and holding
    /// `a` down yields one `Refused` per repeat. This de-duplicates the
    /// BEEP, which is the only part of that the user can hear.
    ///
    /// **Known under-beep, deliberate.** Nothing clears this on the refused
    /// key's own key-up: that up answers `PassThrough`, which does not post,
    /// so the window never hears about it. Press `a`, release, press `a`
    /// again with nothing in between and the second press is silent. A
    /// duplicated beep is worse than a missing one, and every other outcome
    /// clears it.
    beeped_vk: Option<u32>,
}

thread_local! {
    static UI: RefCell<Option<Ui>> = const { RefCell::new(None) };
    static CB: RefCell<Option<Callbacks>> = const { RefCell::new(None) };
    /// The config and log paths, handed over by `open` and read by
    /// `build_children` inside `WM_CREATE`. Same shape as `CB` for the same
    /// reason: `CreateWindowExW` calls the wndproc before it returns, so
    /// there is no window handle to hang an argument on yet.
    ///
    /// **Constant for the window's lifetime**, which is why it lives here
    /// and not in `ControlState`: `serve` opens the window against
    /// `ServeState::config` and nothing can repoint that while it is open,
    /// so making it ride on every keystroke's push would be paying per
    /// keystroke for a fact that is fixed at creation. `build_children`
    /// therefore reads it rather than taking it, and `WM_DESTROY` clears
    /// it: the log path has to outlive creation for the System page's file
    /// rows, which is the whole reason this holds a `Paths` and not a
    /// config path on its own.
    static CFG: RefCell<Option<Paths>> = const { RefCell::new(None) };
    /// The door the window is showing. Seeded by `open`, moved only by
    /// `show_page`.
    ///
    /// **A `Cell`, not a field of `Ui`, and that is the point of it.**
    /// Reading it takes no `RefCell` borrow, so `layout` -- and through it
    /// `compute_card_rects`, which is documented never to touch `UI` -- can
    /// consult the current page without becoming the second borrow that
    /// aborts the process across an `extern "system"` boundary.
    static PAGE: std::cell::Cell<Page> = const { std::cell::Cell::new(Page::Shortcuts) };
}

/// The window's handle, or `None` when it is closed.
/// Is the window open?
///
/// The same question `hwnd().is_some()` answers, under the name the macOS
/// window uses, so `serve::open_settings` is one function rather than two.
/// `hwnd` itself stays because the probe and the catalog worker need the
/// handle, not merely the fact.
pub fn is_open() -> bool {
    hwnd().is_some()
}

pub fn hwnd() -> Option<HWND> {
    UI.with(|u| u.borrow().as_ref().map(|ui| ui.hwnd))
}

/// An `HWND` a worker thread may carry.
///
/// `HWND` is a raw pointer and therefore not `Send`, but a window handle is
/// a kernel-side id, not a pointer into this thread's memory, and
/// `PostMessageW` is explicitly documented as callable from any thread —
/// posting to another thread's queue is the whole point of it. The only
/// thing this wrapper must never be used for is calling a window API that
/// requires the owning thread; the catalog worker calls exactly one
/// function, and it is `PostMessageW`.
#[derive(Clone, Copy)]
pub struct WindowHandle(pub HWND);
unsafe impl Send for WindowHandle {}

/// Raise the window that is already open. Cheaper than `open` when the
/// caller has already established there is one.
pub fn open_existing() -> bool {
    match hwnd() {
        Some(h) => unsafe { SetForegroundWindow(h) }.as_bool(),
        None => false,
    }
}

/// Give the settings window first refusal on a message so `Ctrl+S`, Tab,
/// Enter, Esc and arrow navigation work inside it. Returns `true` when it
/// consumed the message and the caller must not dispatch it.
///
/// `WM_HOTKEY` is not a dialog message and is never consumed here, so
/// hotkeys keep firing while the window is open — which is the entire
/// reason this window is modeless.
///
/// **`TranslateAcceleratorW` runs BEFORE `IsDialogMessageW`, and the order
/// is the whole point.** The dialog manager claims keys on its own account
/// — Tab, the arrows, Enter, Esc, and every `Alt`-mnemonic — and it does
/// not consult an accelerator table before doing so. Behind it, a table
/// entry for any key it wants is simply never reached.
pub fn filter_dialog_message(msg: &MSG) -> bool {
    // ONE borrow, taken and dropped on this line. Both calls below dispatch
    // straight into this window's wndproc, and a second `RefCell` borrow
    // across an `extern "system"` boundary ABORTS the process rather than
    // unwinding.
    let Some((h, accel)) = UI.with(|u| u.borrow().as_ref().map(|ui| (ui.hwnd, ui.accel))) else {
        return false;
    };
    unsafe {
        // `TranslateAcceleratorW` does NOT check that the message belongs to
        // the window it is given -- it translates any WM_KEYDOWN in `msg`
        // and sends the WM_COMMAND to `h` regardless. This thread also pumps
        // the tray window and whatever hidden windows the shell/COM
        // machinery creates on it, so the ownership test `IsDialogMessageW`
        // makes internally has to be made by hand out here.
        if !accel.is_invalid()
            && (msg.hwnd == h || IsChild(h, msg.hwnd).as_bool())
            && TranslateAcceleratorW(h, accel, msg) != 0
        {
            return true;
        }
        IsDialogMessageW(h, msg).as_bool()
    }
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn text_of(h: HWND) -> String {
    let len = unsafe { GetWindowTextLengthW(h) };
    if len <= 0 {
        return String::new();
    }
    let mut buf = vec![0u16; len as usize + 1];
    let n = unsafe { GetWindowTextW(h, &mut buf) };
    String::from_utf16_lossy(&buf[..n.max(0) as usize])
}

fn set_text(h: HWND, s: &str) {
    unsafe {
        let _ = SetWindowTextW(h, PCWSTR(wide(s).as_ptr()));
    }
}

fn enable(parent: HWND, id: i32, on: bool) {
    if let Ok(h) = unsafe { GetDlgItem(Some(parent), id) } {
        unsafe {
            let _ = EnableWindow(h, on);
        }
    }
}

/// Is this control currently operable? The mirror of `enable`, and the
/// gate every keyboard route through `handle_command` has to pass — a
/// disabled button is a promise that the command is unavailable, and an
/// accelerator that ignores it makes a liar of the greying.
fn enabled(parent: HWND, id: i32) -> bool {
    match unsafe { GetDlgItem(Some(parent), id) } {
        Ok(h) => unsafe { IsWindowEnabled(h) }.as_bool(),
        Err(_) => false,
    }
}

thread_local! {
    /// Which of the seven toggle chips are armed, one bit each.
    ///
    /// **This exists because Windows stopped tracking it.** `BS_OWNERDRAW`
    /// is an alternative VALUE of a BUTTON's type field, not a flag beside
    /// `BS_AUTOCHECKBOX`, so the four modifier chips and the three `Hold`
    /// chips have no check state at all: `BM_SETCHECK` is clamped away and
    /// `BM_GETCHECK` answers 0 forever. `check` and `is_checked` route here
    /// instead, which is why every one of their ~14 call sites is unchanged.
    ///
    /// **A `Cell`, for `CAP_FONT`'s reason.** `WM_DRAWITEM` arrives inside a
    /// paint, and a paint reaches this window while `UI` is already borrowed
    /// -- measured on a14 for the Shortcut column, where every subitem
    /// notification exited at `try_borrow` and the column silently drew as
    /// text. A `Cell` cannot be contended. One settings window exists per
    /// thread, so one word per thread is the whole store.
    static CHIPS: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

/// This chip's bit in `CHIPS`, or `None` for a control that is a real check
/// box and keeps its own state.
///
/// **`IDC_CAPS` is deliberately absent.** It is a sentence -- *Use Caps Lock
/// as a shortcut key* -- not a key, so it stays a `BS_AUTOCHECKBOX` and a
/// keycap would be a lie about what it is.
fn chip_bit(id: i32) -> Option<u32> {
    let i = match id {
        IDC_MOD_CTRL => 0,
        IDC_MOD_WIN => 1,
        IDC_MOD_ALT => 2,
        IDC_MOD_SHIFT => 3,
        IDC_HOLD_CTRL => 4,
        IDC_HOLD_WIN => 5,
        IDC_HOLD_ALT => 6,
        _ => return None,
    };
    Some(1u32 << i)
}

fn chip_armed(bit: u32) -> bool {
    CHIPS.with(|c| c.get()) & bit != 0
}

/// Arm or disarm a chip and repaint it -- but only when the state actually
/// moves.
///
/// **The guard is not an optimisation.** `apply_state` runs on every
/// keystroke and pushes all seven chips, so an unconditional `InvalidateRect`
/// is seven repaints per character typed into the App field: flicker on the
/// row the user is not even looking at. It is the same "guarded by a read"
/// rule `set_key_sel` and the `Tap` combo already follow.
///
/// **`erase: false`, because `draw_chip` fills the whole `rcItem` itself.**
/// An owner-draw button paints nothing of its own, background included, so
/// letting Windows erase first would be one extra pass over pixels that are
/// about to be overwritten.
///
/// Safe to call from inside `apply_state`: `InvalidateRect` only marks the
/// control dirty. The `WM_PAINT` -- and the `WM_DRAWITEM` it sends back here
/// -- arrives later, from the message loop, not from this call.
fn set_chip(parent: HWND, id: i32, bit: u32, on: bool) {
    let cur = CHIPS.with(|c| c.get());
    let want = if on { cur | bit } else { cur & !bit };
    if want == cur {
        return;
    }
    CHIPS.with(|c| c.set(want));
    if let Ok(h) = unsafe { GetDlgItem(Some(parent), id) } {
        unsafe {
            let _ = InvalidateRect(Some(h), None, false);
        }
    }
}

/// Is this chip armed, or this check box ticked? The mirror of `check`, and
/// the only way `handle_command` learns what a click did.
///
/// **The state is read back from the WINDOW, never from the notification**,
/// and both kinds of control honour that: a `BS_AUTOCHECKBOX` toggles itself
/// before `BN_CLICKED` arrives, and `handle_command` calls `toggle_chip`
/// before it reads an owner-draw chip. So a caller always sees what the user
/// now sees. Which control is which is `chip_bit`'s business, not a caller's.
///
/// A control that is missing reads as clear. That is the same answer
/// `enabled` gives for the same reason: the alternative is an `Option` every
/// call site would have to collapse to a bool anyway.
fn is_checked(parent: HWND, id: i32) -> bool {
    if let Some(bit) = chip_bit(id) {
        return chip_armed(bit);
    }
    match unsafe { GetDlgItem(Some(parent), id) } {
        Ok(h) => {
            unsafe { SendMessageW(h, BM_GETCHECK, Some(WPARAM(0)), Some(LPARAM(0))) }.0
                == BST_CHECKED.0 as isize
        }
        Err(_) => false,
    }
}

/// A combo box's selected index exactly as the control reports it: `CB_ERR`
/// (-1) means nothing is selected.
///
/// The raw form exists because the key list has to WRITE that -1 as well as
/// read it -- `CB_SETCURSEL` with -1 is how a selection is cleared, and the
/// guard in front of it compares against the same integer. Squeezing that
/// through an `Option` and back is two conversions to get the same number.
fn cur_sel_raw(h: HWND) -> i32 {
    unsafe { SendMessageW(h, CB_GETCURSEL, Some(WPARAM(0)), Some(LPARAM(0))) }.0 as i32
}

/// A combo box's selected index, or `None` when nothing is selected.
///
/// The `Tap` combo is read and written through this and never by text.
/// `CB_ERR` is -1, which as an index would be a very large `usize`, so the
/// sign test happens before the cast rather than after it.
fn cur_sel(h: HWND) -> Option<usize> {
    let i = cur_sel_raw(h);
    if i < 0 {
        None
    } else {
        Some(i as usize)
    }
}

/// Move the default ring -- the button Enter presses -- onto `id`, and
/// record it so `DM_GETDEFID` names the same button the ring is drawn on.
///
/// `DefDlgProc` does this for a real dialog, by sending itself `DM_SETDEFID`
/// as focus moves; this window is not a dialog box and has to do it by hand.
/// It cannot be skipped, because `IsDialogMessageW`'s `VK_RETURN` path
/// activates the FOCUSED control only when that control answers
/// `WM_GETDLGCODE` with `DLGC_DEFPUSHBUTTON`. A plain `BS_PUSHBUTTON`
/// answers `DLGC_UNDEFPUSHBUTTON`, so Enter falls through to `DM_GETDEFID`
/// -- which, until this existed, always said Save.
///
/// **No `UI` borrow survives the first statement.** `BM_SETSTYLE` repaints a
/// child and can re-enter this window's wndproc, and a second `RefCell`
/// borrow across an `extern "system"` boundary ABORTS the process rather
/// than unwinding. The one borrow here reads and writes an `i32` and drops
/// with its closure.
fn set_default_id(parent: HWND, id: i32) {
    let Some(prev) = UI.with(|u| {
        u.borrow_mut().as_mut().map(|ui| {
            let prev = ui.defid;
            ui.defid = id;
            prev
        })
    }) else {
        return;
    };
    if prev == id {
        // Not merely an optimisation. `BM_SETSTYLE` carries a redraw flag,
        // and Tab from Close back to Save raises `BN_KILLFOCUS`(Close) --
        // which restores Save -- immediately followed by `BN_SETFOCUS`(Save),
        // i.e. asks for Save twice in a row.
        return;
    }
    unsafe {
        set_button_type(parent, prev, BS_PUSHBUTTON as u32);
        set_button_type(parent, id, BS_DEFPUSHBUTTON as u32);
    }
}

/// `Ui::defid` in the vocabulary the pure decision speaks.
///
/// Total: anything that is not one of the seven push buttons reads as
/// `HOME`, which is where the ring lives at rest and what `DM_GETDEFID`
/// answers before focus has ever touched a button.
fn default_button_of(id: i32) -> DefaultButton {
    match id {
        IDC_ADD => DefaultButton::Add,
        IDC_REMOVE => DefaultButton::Remove,
        IDC_OPENFILE => DefaultButton::OpenFile,
        IDC_CLOSE => DefaultButton::Close,
        IDC_RELOAD => DefaultButton::Reload,
        IDC_KEEPMINE => DefaultButton::KeepMine,
        IDC_RECORD => DefaultButton::Record,
        IDC_RESET => DefaultButton::Reset,
        _ => DefaultButton::HOME,
    }
}

/// The other direction. Total by construction -- the enum has no variant
/// without an id.
fn id_of_default_button(b: DefaultButton) -> i32 {
    match b {
        DefaultButton::Save => IDC_APPLY,
        DefaultButton::Add => IDC_ADD,
        DefaultButton::Remove => IDC_REMOVE,
        DefaultButton::OpenFile => IDC_OPENFILE,
        DefaultButton::Close => IDC_CLOSE,
        DefaultButton::Reload => IDC_RELOAD,
        DefaultButton::KeepMine => IDC_KEEPMINE,
        DefaultButton::Record => IDC_RECORD,
        DefaultButton::Reset => IDC_RESET,
    }
}

/// Take the ring -- and the focus -- off anything this push has just put out
/// of reach.
///
/// **CORRECTED 2026-08-14: `apply_state` is no longer the only authoritative
/// moment.** This paragraph read "**`apply_state` is the authoritative
/// moment, and it is the only one.** ... Every `show` and every `enable` in
/// this window happens in `apply_state`, so running this after the last of
/// them closes the gap by construction rather than by listing the cases."
/// The premise was true until the tab strip landed and is not now:
/// `show_page` hides a whole page's controls without going near
/// `apply_state`, so there are two moments, and `repair_hidden_button` below
/// is the half they share.
///
/// What has not changed is why either moment exists. The window's normal
/// migration is focus-driven (`BN_SETFOCUS` / `BN_KILLFOCUS` in
/// `handle_command`), and that covers every way a user can move the ring by
/// hand. What it cannot cover is a control going away underneath it: hiding a
/// window raises no focus notification at all (measured on a14 2026-08-11 --
/// `DM_GETDEFID` still answered `IDC_RELOAD` after the banner was dismissed,
/// and Enter pressed a button that was not on screen). A page switch reaches
/// the identical defect by another route, and with four buttons rather than
/// two: `Add`, `Remove`, `Record` and `Reset` are all Shortcuts-page controls
/// and all four are in `PUSH_BUTTONS`.
///
/// Two repairs, in this order, because the first can make the second
/// unnecessary. **Both now live in `repair_hidden_button`**, which this
/// function calls first and `show_page` calls on its own; the third repair,
/// the enablement pass at the bottom of this function, is the half that
/// needs a `ControlState` and is the reason the two are separate functions
/// rather than one:
///
/// 1. **Focus.** Measured on Windows ARM64: by the time that function runs,
///    focus is usually already off the vanished button. The `show` calls
///    that hide whatever just lost `visible()` -- the banner's three in
///    `apply_state`, a whole page's in `show_page` -- have already run, and
///    hiding a control that currently holds focus is enough for user32 to
///    hand focus to the PARENT on its own, as part of that same
///    `ShowWindow(SW_HIDE)` call.
///    So `GetFocus()` below typically already resolves to the window itself
///    -- `GetDlgCtrlID` reads back `0`, not `IDC_CLOSE` -- `is_push_button`
///    on that id is false, and the match has nothing left to do. It stays
///    written for whatever still resolves to a live push button whose
///    `visible()` disagrees: `Ok(close)` is the repair for that case, moving
///    focus onto `IDC_CLOSE` -- looked up with `GetDlgItem` -- **not onto
///    the window itself.** The `Err(_)` arm below is not dead code either:
///    `GetDlgItem` failing to resolve `IDC_CLOSE` is reached in practice,
///    not just a defensive branch that never fires, so it falls back to
///    `hwnd` rather than leave focus wherever the failed lookup found it.
///    An earlier version of this fix parked focus on `hwnd` on the theory that
///    `IsDialogMessageW` would then tab out of it into the control table;
///    that is true for a real dialog, where `DefDlgProc`'s `WM_SETFOCUS`
///    forwards focus to the first tabstop, but this window is a custom
///    class driven by plain `DefWindowProc`, which has no such arm. Without
///    it, `IsDialogMessageW`'s Tab branch resolves through
///    `GetNextDlgTabItem(h, msg.hwnd, ...)`, which returns NULL unless
///    `msg.hwnd` is `IsChild` of `h` -- and a window is never its own child
///    -- so Tab went dead until the user clicked a control. Caught in
///    review, not measured on a14; see the hardware-fixes report for the
///    corrected probe. `IDC_CLOSE` is always present, always enabled (even
///    in the read-only state -- see `ControlState::editable`), and it is
///    never the button being hidden out from under focus here, so this
///    cannot recurse into needing its own repair. Only for a HIDDEN button;
///    a *disabled* one already raises `BN_KILLFOCUS`, which the existing arm
///    handles.
/// 2. **The ring**, read AFTER the focus move so it sees the
///    `BN_SETFOCUS`/`BN_KILLFOCUS` pair that move just raised. Because
///    `IDC_CLOSE` is itself a push button, the ring follows focus onto it
///    (the same "ring follows focus" rule `handle_command` applies to every
///    other Tab step, e.g. onto `Remove`) rather than resting on `HOME`.
///    That is intentional, not a leftover of the old `hwnd` target: Close is
///    always a safe place for Enter or Space to land, because both route
///    through `on_close_request` -- the same gate Esc and the title-bar [X]
///    already use -- so a stray press asks before it discards anything; it
///    does not silently reload like the button this repair is fleeing.
///
/// **Borrows.** `SetFocus` re-enters this wndproc and `set_default_id` sends
/// `BM_SETSTYLE`; a second `UI` borrow across either would abort the process.
/// No borrow is held here across anything: `GetFocus`/`GetDlgCtrlID`/
/// `GetDlgItem` take none, the `defid` read is taken and dropped inside its
/// own `UI.with`, and `set_default_id` takes and drops its own before it
/// sends.
unsafe fn repair_default_button(hwnd: HWND, st: &ControlState, external_change: bool) {
    // Both repairs that need only VISIBILITY, run first and shared with
    // `show_page`. `default_button` below cannot undo them: if this moved the
    // ring HOME, `default_button(HOME, ..)` returns HOME by its own early
    // return, and if it left the ring alone the button is on screen and the
    // enablement pass runs exactly as it did before pages existed.
    let page = PAGE.with(|p| p.get());
    repair_hidden_button(hwnd, external_change, page);
    let cur = UI
        .with(|u| u.borrow().as_ref().map(|ui| ui.defid))
        .unwrap_or(IDC_APPLY);
    let want = default_button(default_button_of(cur), st, external_change, page);
    // `set_default_id` no-ops when the id it is handed is already the
    // default, so the overwhelmingly common push repaints nothing.
    set_default_id(hwnd, id_of_default_button(want));
}

/// The half of `repair_default_button` that needs no `ControlState`: move
/// focus off any control that is not on screen, and the ring off a BUTTON
/// that is not.
///
/// **`show_page` has no `ControlState` and does not need one.** Nothing calls
/// `apply_state` on a tab click -- there is no model change to push -- and a
/// page switch cannot change whether a button is ENABLED, only whether it is
/// drawn. `DefaultButton::visible` is exactly that question, and the
/// enablement half runs on the next push as usual.
///
/// **The focus test is TWO tests, and the wider one is the reason this
/// function is not about buttons.** The ring can only rest on one of the nine
/// `PUSH_BUTTONS`, so the ring half below asks `DefaultButton::visible`; focus
/// can rest on any control the window owns, and `IDC_APP`, `IDC_FILTER`,
/// `IDC_COMBO` and `IDC_LIST` are none of the nine. So `hidden_child` asks the
/// screen instead of the model: focus on a control this window has just
/// hidden is repaired whatever kind of control it is. Left to the button test
/// alone, a switch taken while the App combo held focus would leave `GetFocus`
/// on an off-screen COMBOBOX -- keystrokes into a field nobody can see, no
/// `CBN_KILLFOCUS`, so no `commit_fields`, so text in no model and on no
/// screen.
///
/// **Fixed before it can fire, deliberately.** Every switch that exists today
/// arrives as a pill click or an arrow key, and both move focus onto the pill
/// BEFORE `show_page` hides anything -- so the control being hidden is never
/// the focused one and this arm cannot be reached. `Ctrl+Tab` / `Ctrl+1`..`4`
/// (Task 5) are what make it reachable: `TranslateAcceleratorW` runs before
/// `IsDialogMessageW` and moves no focus at all, which is the same property
/// that makes them the sharp case for `layout` (module header). A repair
/// added with the keystroke would be a repair written under the pressure of
/// the defect it prevents.
///
/// The two tests agree at both call sites and neither is redundant for that
/// reason: `apply_state` pushes the banner's visibility and `show_page` calls
/// `show_page_controls` before either reaches here, so the screen and
/// `DefaultButton::visible` already say the same thing about a button. The
/// button test stays because it is the ring's own predicate, tested in core on
/// all three CI jobs, and because a future caller that repairs before pushing
/// visibility still gets the ring right.
///
/// **The focus move can now raise a notification that carries data**, which
/// the button-only version could not: `IDC_APP` losing focus fires
/// `CBN_KILLFOCUS`, whose arm calls `commit_fields`. That is the correct
/// answer rather than a side effect -- it is exactly what tabbing away from
/// the field does, and it is why the text survives the switch instead of
/// being stranded in a hidden control.
///
/// The ring falls back to `HOME` rather than following focus onto
/// `IDC_CLOSE`. That looks like a disagreement with repair 2 above and is
/// not: `SetFocus` on a push button raises `BN_SETFOCUS`, `handle_command`
/// answers it with `set_default_id(hwnd, id)`, and that has already happened
/// by the time the `visible` test below runs -- so the fallback fires only
/// when focus did NOT land on a push button, which is the case `HOME` is for.
unsafe fn repair_hidden_button(hwnd: HWND, external_change: bool, page: Page) {
    let focus = GetFocus();
    if !focus.is_invalid() {
        let fid = GetDlgCtrlID(focus);
        if hidden_child(hwnd, focus)
            || (is_push_button(fid) && !default_button_of(fid).visible(external_change, page))
        {
            match GetDlgItem(Some(hwnd), IDC_CLOSE) {
                Ok(close) => {
                    let _ = SetFocus(Some(close));
                }
                Err(_) => {
                    // Not dead code: `IDC_CLOSE` is created unconditionally
                    // in `build_children`, but this arm is reached in
                    // practice, not merely a defensive branch that never
                    // fires -- see `repair_default_button`'s doc comment.
                    // Fall back to the window itself rather than leave focus
                    // stranded on a vanished control -- a dead Tab key is a
                    // smaller defect than Space reaching a hidden button.
                    if beckon_core::verbose() {
                        eprintln!(
                            "verbose: settings window: GetDlgItem(IDC_CLOSE) \
                             failed while moving focus off a hidden button"
                        );
                    }
                    let _ = SetFocus(Some(hwnd));
                }
            }
        }
    }
    let cur = UI
        .with(|u| u.borrow().as_ref().map(|ui| ui.defid))
        .unwrap_or(IDC_APPLY);
    if !default_button_of(cur).visible(external_change, page) {
        set_default_id(hwnd, id_of_default_button(DefaultButton::HOME));
    }
}

/// Swap which KIND of button `id` is, keeping every other `BS_` bit it has.
///
/// Read-modify-write through `BS_TYPEMASK` rather than assigning the type on
/// its own: `BS_NOTIFY` sits in the same low word, and it is what makes the
/// focus notifications that drive this migration arrive at all -- so a bare
/// assignment would move the ring once and then never again. The style's
/// HIGH word (`WS_TABSTOP`, `WS_GROUP`, `WS_VISIBLE`) is not
/// `BM_SETSTYLE`'s to touch, and is masked off here rather than trusted to
/// comctl32's own masking.
unsafe fn set_button_type(parent: HWND, id: i32, ty: u32) {
    let Ok(h) = GetDlgItem(Some(parent), id) else {
        return;
    };
    let cur = GetWindowLongW(h, GWL_STYLE) as u32;
    let new = (cur & 0xFFFF & !BS_TYPEMASK_BITS) | (ty & BS_TYPEMASK_BITS);
    SendMessageW(
        h,
        BM_SETSTYLE,
        Some(WPARAM(new as usize)),
        // Redraw. The ring has to MOVE on screen, not just in the style
        // bits: a ring that stays on Save while Enter presses Close is the
        // same lie in a different place.
        Some(LPARAM(1)),
    );
}

fn show(h: HWND, on: bool) {
    unsafe {
        let _ = ShowWindow(h, if on { SW_SHOW } else { SW_HIDE });
    }
}

/// Is `h` a control of `parent`'s that `show(h, false)` has taken off screen?
///
/// **The control's OWN `WS_VISIBLE` bit, not `IsWindowVisible`**, which
/// answers for the whole ancestor chain and so folds in a second question --
/// whether the window itself is on screen -- that the one caller has no
/// opinion about. `paint::field_border` wants the chain and uses the other
/// call; this wants "did this window hide it", which is exactly the bit
/// `show` writes.
///
/// **`IsChild` is the other half and is not defensive.** `GetFocus` answers
/// for the whole THREAD, and `serve` owns another window on it (the tray's,
/// plus whatever COM creates), so without it a hidden window that is none of
/// this window's business would be read as a hidden control of ours and pull
/// focus away from it.
unsafe fn hidden_child(parent: HWND, h: HWND) -> bool {
    IsChild(parent, h).as_bool() && (GetWindowLongW(h, GWL_STYLE) as u32) & WS_VISIBLE.0 == 0
}

thread_local! {
    /// What `IDC_NOTES` is currently showing, one severity-tagged line per
    /// entry -- `paint::draw_notes`'s own input, and the paint-side mirror
    /// of `CHIPS` for exactly `CHIPS`'s own reason: `WM_DRAWITEM` can arrive
    /// while `UI` is already borrowed, so the notes cannot be re-derived
    /// from `Ui::detail` at draw time. `show_notes` keeps this in step with
    /// whatever it writes to the control's own window text, in the same
    /// call, so the two can never disagree about which lines are showing.
    static SHOWN_NOTES: RefCell<Vec<Note>> = const { RefCell::new(Vec::new()) };
}

/// Push `body` to `IDC_NOTES`: mirrored into `SHOWN_NOTES` for `WM_DRAWITEM`
/// to paint (`paint::draw_notes` needs the `Mark` beside each line, which
/// plain window text cannot carry) and, separately, written as plain joined
/// text through `set_text` -- so `GetWindowText`, which is what a screen
/// reader and `examples/settings_probe.rs`'s `dump` both read, still answers
/// with something. The plain text drops the severity a screen reader cannot
/// see drawn as colour anyway; the note's own words already say what is
/// wrong, exactly as they did before this task.
///
/// Replaces the old per-mark glyph prefix (`mark_glyph`, deleted): alignment
/// is structural now -- `paint::draw_notes` draws every dot at the same
/// fixed x regardless of which mark it is -- so there is no glyph column
/// left to keep aligned inside the string.
fn show_notes(notes_hwnd: HWND, body: Vec<Note>) {
    let plain = body
        .iter()
        .map(|n| n.text.as_str())
        .collect::<Vec<_>>()
        .join("\r\n");
    SHOWN_NOTES.with(|c| *c.borrow_mut() = body);
    set_text(notes_hwnd, &plain);
}

/// Move to another door. Returns whether the door actually changed.
///
/// **A door is an exit path, so `end_capture` runs before any of the five
/// steps below.** It was not on the list of them, and the omission was a
/// swallowed keyboard: `Record` reads `Stop` while a capture is armed, `Stop`
/// IS `IDC_RECORD`, and `IDC_RECORD` is a Shortcuts-page control that step 3
/// hides. The hook swallows the KEYBOARD only, so the mouse reaches the pills
/// freely; none of spec F.4's three focus layers fires for a child-to-child
/// focus move inside one window, which is exactly what a pill click is. The
/// result was a hook still armed with its only visible way out off screen,
/// and the 10 s watchdog is a weak bound on that -- `CAPTURE_TIMEOUT_MS`
/// bounds SILENCE, and `on_capture` re-arms the timer for every outcome the
/// hook posts, so a held modifier or a refused chord keeps the clock running
/// and the keyboard swallowed. Worse than swallowed: a chord completed on
/// another page still runs `Outcome::Captured`, which writes the five
/// controls and ends in `push_shortcut` -- so the recording finishes into the
/// model from a page that shows nothing about it.
///
/// Here rather than in `handle_command`'s pill arm because this function is
/// the one funnel every door change goes through -- the arm, and `Ctrl+Tab` /
/// `Ctrl+1`..`4` when Task 5 adds them. `end_capture` is idempotent, so the
/// overwhelmingly common switch, with nothing armed, costs a cleared flag and
/// a `KillTimer` that fails.
///
/// **After the unchanged-door guard, not before it.** Clicking the pill you
/// are already behind changes nothing and hides nothing, and `Stop` is still
/// on screen; cancelling a recording there would be this function inventing a
/// second way to stop one.
///
/// Five steps, and the order of the middle three is the whole of the
/// function:
///
/// 1. **`PAGE` first**, because `layout` reads the page out of it rather
///    than being handed one (`LayoutHandles::page`), so nothing below would
///    place the incoming page's controls if the `Cell` still named the
///    outgoing one.
/// 2. `CheckRadioButton` even though a mouse click has already moved the
///    tick -- the `AUTO` in `BS_AUTORADIOBUTTON` only fires for a click, and
///    the accelerator route moves nothing at all.
/// 3. **Hide and show before `layout`, never after.** A control placed and
///    then hidden flickers at its new position for one frame; a control
///    shown and then placed appears where it belongs. `show_page_controls`
///    is also what makes the tab order correct before anything can Tab.
/// 4. **`layout` DIRECTLY, the way `WM_SIZE` does** -- never through
///    `apply_state`, which nothing calls on a tab click and which has no
///    model change to push. The invalidate afterwards is `WM_SIZE`'s too and
///    for its reason: `SetWindowPos` on a child only invalidates what THAT
///    child vacated, so the cards, their `CARD_PAD` rings and the gaps
///    between them are left painted with the outgoing page's geometry
///    otherwise.
/// 5. **`repair_hidden_button` last, and it is not optional.** Hiding a
///    control raises no focus notification at all, so without it the ring
///    and the focus are left on an off-screen button and Enter presses it --
///    the measured a14 defect, reached through a door instead of through the
///    banner. `Add`, `Remove`, `Record` and `Reset` are all Shortcuts-page
///    controls and all four are in `PUSH_BUTTONS`. It runs after step 3
///    rather than before it, because `ShowWindow(SW_HIDE)` on the focused
///    control is what usually moves focus in the first place.
///
/// **The unchanged-page guard is not an optimisation.** `layout` is
/// `SetWindowPos` on the populated App combo, the measured path that
/// re-synchronises its edit field and discards what the user typed (see
/// `Ui::shown_external`), so clicking the pill you are already on, or
/// pressing its accelerator, must not reach it. `set_default_id` guards
/// itself the same way and for a smaller reason.
///
/// It also means a caller cannot use this to establish an INITIAL page: at
/// creation `PAGE` already holds the door `open` asked for, so a call naming
/// that door does nothing. `build_children` therefore calls
/// `show_page_controls` itself.
fn show_page(hwnd: HWND, page: Page) -> bool {
    if PAGE.with(|p| p.get()) == page {
        return false;
    }
    // Before anything is hidden, and before `PAGE` moves: the teardown writes
    // `Record`'s caption and re-enables the five typed-path controls, and
    // those are the outgoing page's, so it runs while they are still the ones
    // on screen. It takes and drops its own `UI` borrows, so it is safe ahead
    // of the read below -- see the doc.
    unsafe { end_capture(hwnd) };
    PAGE.with(|p| p.set(page));
    // ONE borrow, taken and dropped on this line: everything below sends or
    // re-enters this wndproc, and a second `RefCell` borrow across an
    // `extern "system"` boundary aborts the process rather than unwinding.
    let external_change = UI
        .with(|u| u.borrow().as_ref().map(|ui| ui.external_change))
        .unwrap_or(false);
    unsafe {
        let _ = CheckRadioButton(hwnd, IDC_TAB_SHORTCUTS, IDC_TAB_ABOUT, tab_id_of(page));
        show_page_controls(hwnd, page, external_change);
        layout(hwnd);
        let _ = InvalidateRect(Some(hwnd), None, true);
        repair_hidden_button(hwnd, external_change, page);
    }
    // `Ui::shown_page` records the page the CURRENT layout was computed for,
    // and this is that layout -- so `apply_state`'s guard does not run a
    // second one for a switch that has already been laid out.
    UI.with(|u| {
        if let Some(ui) = u.borrow_mut().as_mut() {
            ui.shown_page = Some(page);
        }
    });
    true
}

// ---------------------------------------------------------------------------
// Creation
// ---------------------------------------------------------------------------

/// Open the window, or raise it if it is already open.
///
/// `paths.config` is the file the caller's callbacks read and write. It names
/// the window (`beckon - <file name>`) and fills the `Open config file`
/// tooltip; it is taken ONCE, here, because it cannot change while the
/// window is open. `paths.log` rides along for the same reason and is
/// `None` when `serve` was started without `--log`.
///
/// `page` is the door to land on: it is stored in `PAGE`, and
/// `build_children` lights the matching pill before any other control
/// exists, then hides every other page's controls with `show_page_controls`.
///
/// **CORRECTED 2026-08-14, twice.** This read "stored and read nowhere yet:
/// there is nothing to switch until the tab strip exists" until the strip
/// landed, and then "what it does NOT do yet is decide which controls are
/// shown: every page's controls are still created and placed together, so
/// landing on `Keyboard` currently lights the second pill and shows the same
/// window" until page switching did. Both were true when written and neither
/// lasted a day.
///
/// `serve` passes the door the user last left the window on
/// (`ServeState::settings_page`), which is `Shortcuts` until they move.
pub fn open(cb: Callbacks, paths: &Paths, page: Page) -> Result<(), String> {
    if let Some(h) = hwnd() {
        unsafe {
            let _ = SetForegroundWindow(h);
        }
        // Keep the existing callbacks: they close over the caller's live
        // state, and the second set would be a duplicate of the first.
        return Ok(());
    }
    CB.with(|c| *c.borrow_mut() = Some(cb));
    CFG.with(|c| *c.borrow_mut() = Some(paths.clone()));
    PAGE.with(|p| p.set(page));
    unsafe { create() }
}

unsafe fn create() -> Result<(), String> {
    let hinst = GetModuleHandleW(None).map_err(|e| format!("GetModuleHandleW: {e}"))?;

    // The common-controls DLL must be loaded before a SysListView32 is
    // created, or CreateWindowExW fails with "class not found".
    // ICC_BAR_CLASSES is what registers `tooltips_class32`; neither of the
    // other two does, and the tooltip on `Open config file` is the only
    // place the full config path is shown.
    let icc = INITCOMMONCONTROLSEX {
        dwSize: std::mem::size_of::<INITCOMMONCONTROLSEX>() as u32,
        dwICC: ICC_LISTVIEW_CLASSES | ICC_STANDARD_CLASSES | ICC_BAR_CLASSES,
    };
    let _ = InitCommonControlsEx(&icc);

    let class = w!("BeckonSettingsWindow");
    // Resource id 1, the same icon beckon.rc embeds and the tray already
    // uses. hIcon wants the large (SM_CXICON, 32x32) variant LoadIconW
    // returns; hIconSm wants the small (SM_CXSMICON, typically 16x16) one,
    // loaded explicitly via LoadImageW exactly like the tray's own
    // tray_add -- letting the shell downsample the large icon to 16x16 on
    // the fly is what tray_add's comment says blurs an icon that is crisp
    // at 16x16 in the .ico itself. Both fall back to the stock
    // IDI_APPLICATION icon, matching tray_add, so a build without the .rc
    // resource still shows an icon instead of none.
    let icon = LoadIconW(Some(hinst.into()), PCWSTR(1 as *const u16))
        .or_else(|_| LoadIconW(None, IDI_APPLICATION))
        .unwrap_or_default();
    let icon_sm = LoadImageW(
        Some(hinst.into()),
        PCWSTR(1 as *const u16),
        IMAGE_ICON,
        GetSystemMetrics(SM_CXSMICON),
        GetSystemMetrics(SM_CYSMICON),
        LR_DEFAULTCOLOR,
    )
    .map(|h| HICON(h.0))
    .or_else(|_| LoadIconW(None, IDI_APPLICATION))
    .unwrap_or_default();
    // WNDCLASSEXW, not WNDCLASSW: the brief called for hIconSm, but that
    // field only exists on the Ex struct (paired with RegisterClassExW) --
    // WNDCLASSW has no small-icon slot at all. Same feature flag either way
    // (Win32_UI_WindowsAndMessaging), so this is not a new dependency.
    let wc = WNDCLASSEXW {
        cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
        // Deliberately NOT `CS_HREDRAW | CS_VREDRAW`. The client became a
        // painted layer of cards in Task 8, so a resize genuinely does need
        // a full repaint -- but `WM_SIZE`/`WM_DPICHANGED` are not the only
        // way the card stack moves: the banner appearing or disappearing
        // (`apply_state`'s `relayout` block) reflows every card below it
        // with no `WM_SIZE` in sight, so a class style that only fires on a
        // size change would leave that path uncovered. `WM_SIZE`,
        // `WM_DPICHANGED` and `apply_state`'s `relayout` block each call
        // `InvalidateRect(hwnd, None, true)` explicitly instead -- one
        // mechanism that covers all three, rather than a class style plus a
        // hand-rolled special case for the one it cannot reach.
        style: WNDCLASS_STYLES(0),
        lpfnWndProc: Some(wndproc),
        hInstance: hinst.into(),
        lpszClassName: class,
        hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
        // **No class background brush at all.** WNDCLASS takes a system
        // colour index PLUS ONE here, and this was `COLOR_BTNFACE + 1` -- a
        // LIGHT system colour, which is the only light thing anywhere in this
        // window's paint path once the theme took over.
        //
        // It showed. Measured on a14 2026-08-13, right after `nccalcsize`
        // began reclaiming the whole frame: a #B1B1B1 band 10 px wide down
        // the inside of the left and top edges -- exactly the strip that had
        // just stopped being non-client -- which vanished on the next
        // repaint. `DefWindowProc` erases a newly-exposed region with this
        // brush before `WM_PAINT` gets to it, so any region the theme has not
        // painted yet flashes a system colour, and the wider the client grows
        // the more of it there is to flash.
        //
        // A null brush means `DefWindowProc` erases nothing and
        // `WM_ERASEBKGND` owns the ground unconditionally -- which it already
        // did for every tier that paints, and deliberately does not under
        // Mica, where painting is the thing that would hide the backdrop.
        hbrBackground: HBRUSH(std::ptr::null_mut()),
        hIcon: icon,
        hIconSm: icon_sm,
        ..Default::default()
    };
    // Non-zero on success; a second call for an already-registered class
    // fails harmlessly, which is what happens when the window is reopened.
    RegisterClassExW(&wc);

    // CW_USEDEFAULT for position, but the SIZE must be scaled by hand:
    // under per-monitor-v2 these are physical pixels, and no WM_DPICHANGED
    // arrives to correct a window that was born the wrong size.
    let dpi = {
        let mut pt = POINT::default();
        let _ = GetCursorPos(&mut pt);
        let mon = MonitorFromPoint(pt, MONITOR_DEFAULTTOPRIMARY);
        let (mut x, mut y) = (96u32, 96u32);
        let _ = GetDpiForMonitor(mon, MDT_EFFECTIVE_DPI, &mut x, &mut y);
        x.max(96)
    };
    let w = scale(WINDOW_WIDTH, dpi);
    let h = scale(WINDOW_HEIGHT, dpi);

    // Named at birth rather than on the first `apply_state`, so the window
    // never flashes a bare `beckon` in the taskbar before the first push.
    // The borrow is dropped on this line: `CreateWindowExW` runs the wndproc
    // -- and therefore `build_children`, which reads `CFG` too -- before it
    // returns.
    let title = wide(&title_base(
        &CFG.with(|c| c.borrow().as_ref().map(|p| p.config.clone()))
            .unwrap_or_default(),
    ));

    let hwnd = CreateWindowExW(
        WINDOW_EX_STYLE(0),
        class,
        PCWSTR(title.as_ptr()),
        // WS_OVERLAPPEDWINDOW minus WS_MAXIMIZEBOX. Dropping maximize is
        // LOAD-BEARING, not cosmetic: it removes the HTMAXBUTTON / Snap
        // Layouts obligation AND makes the maximized state -- where
        // WM_NCCALCSIZE overflows the monitor by the frame thickness unless
        // corrected by hand -- unreachable. The window is still resizable by
        // its edges; `layout` already handles that.
        // **No `WS_CAPTION`.** Measured on a14 2026-08-13: with it, Windows 11
        // composites its OWN caption buttons over the reclaimed client area,
        // so the window wore two minimise glyphs and two close glyphs drawn on
        // top of each other -- plus a maximise button this design does not
        // have -- visible as colour fringing where the two renderings of the
        // same X disagree. `WM_NCCALCSIZE` reclaims the SPACE; it does not
        // stop DWM drawing the buttons it believes a captioned window needs.
        //
        // `WS_POPUP` keeps `WS_THICKFRAME`'s resizability and the DWM shadow
        // while declaring no caption for DWM to furnish. Resizability, not a
        // visible border: `chrome::nccalcsize` gives the border's space back
        // to the client and `chrome::nchittest` re-creates the eight
        // directions as a hit-test strip over painted pixels. `WS_THICKFRAME`
        // is still load-bearing -- it is what makes those hit-test codes mean
        // anything to `DefWindowProc`'s sizing loop. MSDN says
        // `WS_SYSMENU` wants `WS_CAPTION`; it is kept anyway because the Alt
        // +Space menu and the taskbar's own close entry route through it, and
        // dropping it changes those without fixing anything.
        WS_POPUP | WS_SYSMENU | WS_THICKFRAME | WS_MINIMIZEBOX,
        CW_USEDEFAULT,
        CW_USEDEFAULT,
        w,
        h,
        None,
        None,
        Some(hinst.into()),
        None,
    )
    .map_err(|e| format!("CreateWindowExW: {e}"))?;

    // Round the top corners to match Windows 11's own window chrome, which
    // a client-drawn caption otherwise loses -- DWM owns the window BORDER
    // even though this window now draws its own caption content. `create`
    // is already an `unsafe fn`, so -- like every other call in it -- this
    // is not re-wrapped in its own `unsafe {}`; doing so is what the rest of
    // this function avoids, and rustc flags it as a redundant block.
    const DWMWA_WINDOW_CORNER_PREFERENCE: DWMWINDOWATTRIBUTE = DWMWINDOWATTRIBUTE(33);
    const DWMWCP_ROUND: u32 = 2;
    let pref = DWMWCP_ROUND;
    // No-op on Windows 10; the call returns an error we deliberately drop.
    let _ = DwmSetWindowAttribute(
        hwnd,
        DWMWA_WINDOW_CORNER_PREFERENCE,
        &pref as *const _ as *const _,
        std::mem::size_of::<u32>() as u32,
    );

    // The OS's theme answer, resolved once up front. `build_children` (run
    // synchronously inside `WM_CREATE`, above) has already stored a fresh
    // `Ui` with a default (unresolved) `ThemeCache`, so this is the first
    // real answer it gets. The borrow is taken and dropped on this one
    // statement; `apply_dwm_dark` below holds none.
    //
    // `PAINT_THEME` is rebuilt right alongside it -- see `PAINT_THEME` for
    // why painting code needs its own copy rather than reading `ui.theme`.
    let t = beckon_core::theme::resolve(theme::read_inputs());
    UI.with(|u| {
        if let Some(ui) = u.borrow_mut().as_mut() {
            ui.theme.rebuild(t);
        }
    });
    PAINT_THEME.with(|c| {
        c.borrow_mut().rebuild(t);
    });
    theme::apply_dwm_dark(hwnd, t == beckon_core::theme::Theme::Dark);
    // DWM still draws the 1 px border around the window -- reclaiming the
    // frame in `chrome::nccalcsize` took the sizing border, not this. Tint it
    // to the window's own ground.
    //
    // **CORRECTED 2026-08-14**: this comment (and its twin in
    // `on_theme_changed`) read "The resize frame is still non-client on three
    // sides, and DWM paints it black without a caption." True until
    // `c523e8e`, and it is the black band that commit was written to fix --
    // but the fix was reclaiming the frame, not this attribute, which
    // `c523e8e`'s message records as NOT reaching the sizing border at all.
    // See `theme::apply_dwm_border` for the full reversal.
    theme::apply_dwm_border(hwnd, t);
    // First backdrop decision. See `apply_current_backdrop`, below, for why
    // this window never calls `theme::read_backdrop_inputs` directly.
    apply_current_backdrop(hwnd);

    // Position was CW_USEDEFAULT, so Windows -- not the cursor position
    // used above -- decided which monitor the window actually landed on.
    // GetDpiForWindow(hwnd) is authoritative now that hwnd exists; correct
    // the size once, before anything is shown, if the guess was wrong. No
    // WM_DPICHANGED arrives to do this for us: the window was already born
    // on its final monitor, so nothing "changed" from Windows' point of view.
    let real_dpi = GetDpiForWindow(hwnd).max(96);
    if real_dpi != dpi {
        let _ = SetWindowPos(
            hwnd,
            None,
            0,
            0,
            scale(WINDOW_WIDTH, real_dpi),
            scale(WINDOW_HEIGHT, real_dpi),
            SWP_NOZORDER | SWP_NOACTIVATE | SWP_NOMOVE,
        );
    }

    // The list's own theme sync (Task 10) -- `LVM_SETBKCOLOR` and friends,
    // `SetWindowTheme`, and the state image list that forces `tok::ROW_H`
    // rows. All three read state that is only final HERE: `theme_list`
    // through `PAINT_THEME`, already rebuilt above; `rebuild_state_image_list`
    // through `real_dpi`, not the earlier guess `dpi` -- a wrong initial
    // monitor guess must not leave the tick sized for the wrong DPI with no
    // `WM_DPICHANGED` ever arriving to correct it, since the window was
    // already born on its final monitor (the comment above this block gives
    // the same reason for the `SetWindowPos` correction it guards). Before
    // `ShowWindow`, so nothing paints with the wrong sizing first.
    theme_list(hwnd, t == beckon_core::theme::Theme::Dark);
    if let Ok(list) = GetDlgItem(Some(hwnd), IDC_LIST) {
        rebuild_state_image_list(list, real_dpi);
    }

    let _ = ShowWindow(hwnd, SW_SHOW);
    let _ = SetForegroundWindow(hwnd);
    Ok(())
}

/// The shell's own `lfMessageFont` at a specific DPI: the base every role's
/// `LOGFONT` is derived from, and the face all three fall back to.
///
/// `SystemParametersInfoForDpi` first: `SystemParametersInfoW` answers for
/// the system DPI, which is the wrong number for a per-monitor-v2 process on
/// a secondary display. But `SystemParametersInfoForDpi` is documented as
/// valid only for a DPI-aware process, and can fail where the old call never
/// did -- `build.rs` embeds the manifest from Task 6 only for `-msvc`, so a
/// `-gnu` build, or `cargo install --git` on a host with no resource
/// compiler, is still DPI-unaware. Falling back to `SystemParametersInfoW`
/// there keeps the real shell font instead of dropping straight to the
/// stock 1995 bitmap font this whole function exists to avoid. Whether
/// `SystemParametersInfoForDpi` actually returns FALSE on a non-PM process,
/// rather than silently answering for the system DPI, is not something a
/// cross-compile can confirm -- unverified, flagged for the hardware pass.
///
/// Measured on a14 2026-08-11: this returns plain `Segoe UI`, weight 400,
/// at `lfHeight = -12`. NOT Segoe UI Variable -- that reaches the shell
/// through DirectWrite and XAML, never through `NONCLIENTMETRICS`, so a
/// Win32 app has to ask for it by name (`build_fonts`).
unsafe fn message_logfont(dpi: u32) -> LOGFONTW {
    let mut ncm = NONCLIENTMETRICSW {
        cbSize: std::mem::size_of::<NONCLIENTMETRICSW>() as u32,
        ..Default::default()
    };
    let ok = SystemParametersInfoForDpi(
        SPI_GETNONCLIENTMETRICS.0,
        ncm.cbSize,
        Some(&mut ncm as *mut _ as *mut _),
        0,
        dpi,
    )
    .is_ok()
        || SystemParametersInfoW(
            SPI_GETNONCLIENTMETRICS,
            ncm.cbSize,
            Some(&mut ncm as *mut _ as *mut _),
            Default::default(),
        )
        .is_ok();
    if ok {
        return ncm.lfMessageFont;
    }
    // Describe the stock GUI font rather than hand back a zeroed LOGFONT: a
    // zeroed one asks the mapper for "any face at any size", which is how
    // you land on the 1995 bitmap font this path exists to avoid. GetObjectW
    // can itself fail -- unreached today, since both SystemParametersInfo
    // calls above already have to fail first -- and an unchecked failure
    // here hands back exactly the zeroed LOGFONTW this comment says never
    // to return, silently. Check it and say so, rather than let the two
    // disagree.
    let mut lf = LOGFONTW::default();
    let got = GetObjectW(
        GetStockObject(DEFAULT_GUI_FONT),
        std::mem::size_of::<LOGFONTW>() as i32,
        Some(&mut lf as *mut _ as *mut _),
    );
    if got == 0 && beckon_core::verbose() {
        eprintln!(
            "verbose: settings window: GetObjectW(DEFAULT_GUI_FONT) failed \
             -- both SystemParametersInfo calls already failed too, so this \
             LOGFONTW is zeroed and every role will ask the mapper for \
             \"any face at any size\""
        );
    }
    lf
}

/// Write `face` into `lf.lfFaceName`, or report that it does not fit.
///
/// `lfFaceName` is 32 wchars INCLUDING the NUL, so 31 characters is the
/// ceiling -- and a silent truncation there is the exact trap §B.3 records.
/// `Segoe UI Variable Display Semibold` truncated to `Segoe UI Variable
/// Display Semib` names nothing, and GDI answers a name that names nothing
/// with **Arial**, not with an error. Refusing to truncate means no future
/// edit can reintroduce that by accident; `face_matches` is the second net
/// under it.
fn set_face(lf: &mut LOGFONTW, face: &str) -> bool {
    let name: Vec<u16> = face.encode_utf16().collect();
    if name.len() >= lf.lfFaceName.len() {
        return false;
    }
    lf.lfFaceName = [0; 32];
    lf.lfFaceName[..name.len()].copy_from_slice(&name);
    true
}

/// Did GDI actually hand back the face we asked for?
///
/// **`CreateFontIndirectW` never fails on an unknown name** -- the font
/// mapper substitutes silently, so a successful create proves nothing.
/// Measured on a14 2026-08-11: asking for `Segoe UI Variable Text Semib`
/// returned `Arial`, exactly as a `This Font Does Not Exist` control did.
/// The only way to know is to select the font into a DC and read back what
/// the DC now holds.
unsafe fn face_matches(hwnd: HWND, font: HFONT, want: &str) -> bool {
    let dc = GetDC(Some(hwnd));
    if dc.is_invalid() {
        // `make_font` treats this exactly like the face genuinely being
        // absent -- same fallback, same silence otherwise. Log it so a
        // transient GetDC failure and a missing face read differently in a
        // log instead of both showing up as "role fell back" with no trace
        // of which cause it was.
        if beckon_core::verbose() {
            eprintln!(
                "verbose: settings window: GetDC failed while checking for \
                 {want} -- falling back to the shell face this time, not \
                 because it is absent"
            );
        }
        return false;
    }
    let prev = SelectObject(dc, HGDIOBJ(font.0));
    // LF_FACESIZE is 32; the slack costs nothing and removes the question of
    // whether the returned count includes the terminator.
    let mut buf = [0u16; 64];
    let n = GetTextFaceW(dc, Some(&mut buf));
    if !prev.is_invalid() {
        SelectObject(dc, prev);
    }
    ReleaseDC(Some(hwnd), dc);
    if n <= 0 {
        return false;
    }
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..end]).eq_ignore_ascii_case(want)
}

/// One role's font: the shell's `lfMessageFont` with this role's face,
/// pixel height and weight -- and the requested face only when GDI really
/// produced it.
///
/// **`px` is a PIXEL height**, applied as a negative `lfHeight` (character
/// height, the usual GDI convention). Our own measurement is the
/// corroboration: a14 reported `lfMessageFont.lfHeight = -12`, so the shell
/// font is exactly the Caption size. Read as points, Body would be 14 pt --
/// larger than any shell UI and inconsistent with that baseline.
///
/// The fallback keeps this role's SIZE and WEIGHT and gives up only the
/// face, because size and weight are the hierarchy. Segoe UI ships a
/// Semibold, so even a fallen-back Subtitle stays heavier than the Body
/// around it.
unsafe fn make_font(
    hwnd: HWND,
    base: &LOGFONTW,
    face: &str,
    px: i32,
    weight: i32,
    dpi: u32,
) -> HFONT {
    let mut spec = *base;
    spec.lfHeight = -scale(px, dpi);
    // The height is ours now, so the base's paired width would stretch the
    // glyphs; 0 asks the mapper for the face's own aspect ratio.
    spec.lfWidth = 0;
    spec.lfWeight = weight;

    let mut want = spec;
    if set_face(&mut want, face) {
        let f = CreateFontIndirectW(&want);
        if !f.is_invalid() {
            if face_matches(hwnd, f, face) {
                return f;
            }
            // A real handle, ours to free -- just the wrong font in it.
            let _ = DeleteObject(HGDIOBJ(f.0));
        }
    }

    let f = CreateFontIndirectW(&spec);
    if !f.is_invalid() {
        return f;
    }
    HFONT(GetStockObject(DEFAULT_GUI_FONT).0)
}

/// The six type roles of §B.3, built for `dpi`.
///
/// | Role | Size | Weight | Used for |
/// |---|---|---|---|
/// | Title | 15 px | semibold | the title-bar app name (Task 7) |
/// | Subtitle | 18 px | semibold | the `Shortcuts` card head |
/// | BodyStrong | 14 px | semibold | card captions, list column headers, `Save` |
/// | Body | 14 px | regular | list, fields, buttons |
/// | Caption | 12 px | regular | notes |
/// | Chrome | 10 px | regular | the two caption-button glyphs (Task 7) |
///
/// **Subtitle is 18 px, not 20.** An 18 px Semibold heading is Win11's own
/// proportion for a card head, and 20 fought the 14 px body around it.
///
/// **The face names are spelled in full, from the a14 measurement, and are
/// NOT uniform.** `lfFaceName` holds 32 wchar. `Segoe UI Variable Text
/// Semibold` is exactly 31 characters and survives intact; `Segoe UI
/// Variable Display Semib` and `Segoe UI Variable Small Semibol` are cut at
/// that same 32-wchar limit and must be spelled exactly as truncated --
/// "regularising" any of the three to match the others hands `make_font` a
/// name GDI cannot resolve. A wrong spelling does not fail: `CreateFontW`
/// succeeds and hands back Arial. `make_font`'s `GetTextFace` round-trip is
/// what actually catches that, which is why this table is written out
/// rather than generated from one pattern.
///
/// Optical size is why Body/Caption/BodyStrong differ in family at all:
/// Segoe UI Variable ships Small for caption sizes, Text for body and
/// headings up to ~30 px, Display above that -- Title's 15 px still reads
/// as a heading (it is the app name in the title bar) and so takes Display
/// rather than Text.
unsafe fn build_fonts(hwnd: HWND, dpi: u32) -> Fonts {
    let base = message_logfont(dpi);
    Fonts {
        title: make_font(
            hwnd,
            &base,
            "Segoe UI Variable Display Semib",
            15,
            FW_SEMIBOLD.0 as i32,
            dpi,
        ),
        subtitle: make_font(
            hwnd,
            &base,
            "Segoe UI Variable Text Semibold",
            18,
            FW_SEMIBOLD.0 as i32,
            dpi,
        ),
        body_strong: make_font(
            hwnd,
            &base,
            "Segoe UI Variable Text Semibold",
            14,
            FW_SEMIBOLD.0 as i32,
            dpi,
        ),
        body: make_font(
            hwnd,
            &base,
            "Segoe UI Variable Text",
            14,
            FW_NORMAL.0 as i32,
            dpi,
        ),
        caption: make_font(
            hwnd,
            &base,
            "Segoe UI Variable Small",
            12,
            FW_NORMAL.0 as i32,
            dpi,
        ),
        keycap: make_font(
            hwnd,
            &base,
            "Segoe UI Variable Small Semibol",
            11,
            FW_SEMIBOLD.0 as i32,
            dpi,
        ),
        chrome: make_font(
            hwnd,
            &base,
            "Segoe Fluent Icons",
            10,
            FW_NORMAL.0 as i32,
            dpi,
        ),
    }
}

unsafe fn child(
    parent: HWND,
    class: PCWSTR,
    text: &str,
    style: WINDOW_STYLE,
    id: i32,
    fonts: &Fonts,
) -> HWND {
    let h = CreateWindowExW(
        WINDOW_EX_STYLE(0),
        class,
        PCWSTR(wide(text).as_ptr()),
        style | WS_CHILD | WS_VISIBLE,
        0,
        0,
        10,
        10,
        Some(parent),
        Some(HMENU(id as isize as *mut _)),
        None,
        None,
    )
    .unwrap_or_default();
    // The role comes from the id, through the SAME `role_of` the
    // `WM_DPICHANGED` rebroadcast consults. One mapping, two call sites --
    // if creation and that broadcast each carried their own idea of which
    // control gets which font, a walk across monitors would silently
    // re-role half the window.
    let font = fonts.for_id(id);
    SendMessageW(
        h,
        WM_SETFONT,
        Some(WPARAM(font.0 as usize)),
        Some(LPARAM(1)),
    );
    h
}

/// Create every child, **in the order it is drawn**.
///
/// Creation order is Tab order, and that is the whole reason this function
/// reads top-to-bottom. The banner's `Reload` / `Keep mine` used to be
/// created last: the one pair that answers an urgent event — the file moved
/// under us — sat at the end of the Tab order, behind everything it
/// interrupts.
///
/// Every control leaves here already carrying its role's font, which is why
/// `WM_CREATE` can call `layout` immediately afterwards: comctl32 derives
/// the ListView row height from the control's font, and `layout` QUERIES
/// that height rather than assuming it.
unsafe fn build_children(hwnd: HWND) {
    let dpi = GetDpiForWindow(hwnd).max(96);
    let fonts = build_fonts(hwnd, dpi);
    set_cap_font(fonts.get(Role::Keycap));

    // -- Band 0: the tab strip. FIRST, and that is one decision rather than
    // two. Creation order is Tab order (this function's own doc), the strip
    // is the top band, so it leads -- and because nothing is created ahead of
    // them the four pills are the head of the sibling chain, which is what
    // makes them a group without anyone writing `WS_GROUP` on the first one.
    // The group's closing boundary is on the banner below.
    //
    // **`BS_AUTORADIOBUTTON | BS_PUSHLIKE`, not `BS_OWNERDRAW`**, and both
    // halves of that refusal are already written down in this file for other
    // controls. Owner-draw never receives `ODS_HOTLIGHT` -- see
    // `push_button_custom_draw`, "the one bit a REAL `WM_DRAWITEM` never
    // carries for a classic push button", which is why all nine push buttons
    // stayed on `NM_CUSTOMDRAW` rather than becoming genuinely owner-draw --
    // so an owner-draw pill could not have a hover state, and hover is one of
    // the three states the design gives a pill. And it kills `BM_GETCHECK`,
    // which is the whole reason `WM_CHIP_STATE` had to be invented for the
    // seven chips, while a pill's whole job is to say which door is open.
    //
    // Nothing draws a pill yet: `push_button_custom_draw`'s dispatch is gated
    // on `is_push_button`, which these deliberately are not, so until
    // `paint::tab_pill` and its own `NM_CUSTOMDRAW` arm land the four render
    // as ordinary themed push buttons, one of them stuck down.
    //
    // The `AUTO` is doing real work: it clears the sibling pills on a click
    // and it makes Left/Right inside the group select as they move. What it
    // does NOT do is move on an accelerator, which is why `show_page` ticks
    // the pill itself rather than trusting the click path.
    //
    // `WS_TABSTOP` on all four, which may or may not be what Tab ends up
    // seeing. In a real dialog user32 migrates the style onto whichever radio
    // is checked, so a group is ONE tab stop; nothing in this tree has ever
    // exercised a radio group -- the three that existed were retired -- so
    // whether it does that for hand-created controls is unsettled and is gate
    // G-S2. Setting it on all four is the safe end of that: worst case Tab
    // visits four stops instead of one, where the other way round would leave
    // a pill unreachable from the keyboard.
    //
    // No `&` on any of the four captions -- `mod cap` writes out why four
    // unique mnemonics do not exist.
    for (id, _, caption) in TABS {
        child(
            hwnd,
            w!("BUTTON"),
            caption,
            WINDOW_STYLE((BS_AUTORADIOBUTTON | BS_PUSHLIKE) as u32) | WS_TABSTOP,
            id,
            &fonts,
        );
    }
    // The door `open` was asked for, lit before any other control exists.
    //
    // **`CheckRadioButton`, never this file's `check()`.** `check` falls
    // through to `BM_SETCHECK`, which sets one button's state and clears no
    // sibling -- so seeding through it would leave two pills lit the first
    // time the window opened on a page other than Shortcuts, and the
    // auto-radio would not correct it until the user clicked. The first/last
    // pair below is why `ids.rs` keeps these four ids contiguous.
    let _ = CheckRadioButton(
        hwnd,
        IDC_TAB_SHORTCUTS,
        IDC_TAB_ABOUT,
        tab_id_of(PAGE.with(|p| p.get())),
    );

    // -- Band 1: the external-change banner. Hidden until `apply_state`
    // says the file moved; `layout` gives it no height at all while it is
    // hidden, so the bands below close up rather than leaving a gap.
    //
    // **`WS_GROUP` closes the tab strip's group**, and it is here rather than
    // anywhere else because this is the control created immediately after the
    // last pill. Both an auto-radio group and `IsDialogMessageW`'s arrow-key
    // group run until the NEXT control carrying `WS_GROUP`, so without a
    // boundary Left/Right would walk out of the strip into the banner, the
    // filter EDIT and the ListView, and the auto-radio's clear-siblings pass
    // would reach past the strip as well. Until this landed, `IDC_OPENFILE`
    // was the file's only `WS_GROUP`.
    //
    // **The boundary is the style bit, not the control**, and that
    // distinction is load-bearing here because this control is HIDDEN at rest
    // -- `show(banner, false)` runs below, once its two buttons exist. The
    // group walk tests `WS_GROUP` before it tests visibility, so a hidden
    // terminator still terminates. That is how `GetNextDlgGroupItem` reads,
    // and it is NOT something this branch has run on hardware. If the arrow
    // keys are ever seen to escape the strip, suspect this first;
    // `IDC_LBL_SECTION` -- always visible -- is the fallback boundary, at the
    // cost of taking the banner's two buttons into the strip's group.
    let banner = child(
        hwnd,
        w!("STATIC"),
        "This file changed on disk.",
        SS_CENTERIMAGE_STYLE | WS_GROUP,
        IDC_BANNER,
        &fonts,
    );
    // `BS_NOTIFY` on this and every other push button (`PUSH_BUTTONS`), and
    // it is not decoration: without it a BUTTON never reports `BN_SETFOCUS` /
    // `BN_KILLFOCUS`, and those are what carry the default ring to the
    // focused button. Enter on a focused non-default button does NOT press
    // it -- `IsDialogMessageW` asks `DM_GETDEFID` instead -- so a ring that
    // cannot move is a ring that lies. On THIS button that was the sharp
    // end: Enter on Reload used to Save, i.e. overwrite the external edit
    // the banner had just appeared to warn about.
    let reload = child(
        hwnd,
        w!("BUTTON"),
        cap::RELOAD,
        WINDOW_STYLE((BS_PUSHBUTTON | BS_NOTIFY) as u32) | WS_TABSTOP,
        IDC_RELOAD,
        &fonts,
    );
    let keep = child(
        hwnd,
        w!("BUTTON"),
        cap::KEEP_MINE,
        WINDOW_STYLE((BS_PUSHBUTTON | BS_NOTIFY) as u32) | WS_TABSTOP,
        IDC_KEEPMINE,
        &fonts,
    );
    // The three are hidden at the end of this function rather than here.
    // `show_page_controls` puts every control behind its own door in one
    // pass, and the banner's condition is `banner_shown` -- the same function
    // `layout`'s card 0 and core's `DefaultButton::visible` read -- so
    // hiding them beside their creation would be a second, page-blind
    // spelling of a rule that has to be total to be worth anything.

    // -- Band 2: the section head, the filter, then Remove and Add.
    child(
        hwnd,
        w!("STATIC"),
        "Shortcuts",
        SS_CENTERIMAGE_STYLE,
        IDC_LBL_SECTION,
        &fonts,
    );
    // `SS_NOPREFIX` because the text is a COUNT, not a caption: `&` cannot
    // appear in it today, but a static that would silently eat one is a
    // trap, and this one carries no mnemonic by design.
    child(
        hwnd,
        w!("STATIC"),
        "",
        SS_CENTERIMAGE_STYLE | SS_NOPREFIX_STYLE,
        IDC_LBL_COUNT,
        &fonts,
    );
    // `WS_BORDER` off: this is a field-styled control now, and its border is
    // `paint::field_border`, drawn from the PARENT's `WM_PAINT` outside the
    // control's own rect (Task 9) via `WM_CTLCOLOREDIT` for the fill/ink and
    // this rounded stroke for the edge. Nothing here owner-draws the EDIT
    // itself -- unlike `IDC_APP`, a plain EDIT has no child of its own for
    // that to endanger, but the two stay on the same rule rather than one
    // carving out an exception the other does not need.
    let filter = child(
        hwnd,
        w!("EDIT"),
        "",
        WINDOW_STYLE(ES_AUTOHSCROLL as u32) | WS_TABSTOP,
        IDC_FILTER,
        &fonts,
    );
    // Placeholder text rather than a STATIC label: it costs no band-2 width
    // and gets out of the way on focus. comctl32 v6 only, which the manifest
    // guarantees. The buffer must outlive the call, so it is bound.
    let cue = wide(cap::FILTER_CUE);
    SendMessageW(
        filter,
        EM_SETCUEBANNER_MSG,
        Some(WPARAM(0)),
        Some(LPARAM(cue.as_ptr() as isize)),
    );
    child(
        hwnd,
        w!("BUTTON"),
        cap::REMOVE,
        WINDOW_STYLE((BS_PUSHBUTTON | BS_NOTIFY) as u32) | WS_TABSTOP,
        IDC_REMOVE,
        &fonts,
    );
    child(
        hwnd,
        w!("BUTTON"),
        cap::ADD,
        WINDOW_STYLE((BS_PUSHBUTTON | BS_NOTIFY) as u32) | WS_TABSTOP,
        IDC_ADD,
        &fonts,
    );

    // -- Band 3: the list.
    // `WS_BORDER` off (Task 10): the card is the border now
    // (`paint::card`, drawn by the PARENT's `WM_PAINT` around the whole
    // Shortcuts card), the same move Task 9 already made for `IDC_FILTER`
    // and `IDC_APP`. `LVM_GETITEMRECT`/`layout`'s own `border` term drops
    // with it -- see `compute_card_rects`'s comment in `layout.rs`.
    let list = child(
        hwnd,
        w!("SysListView32"),
        "",
        WINDOW_STYLE(LVS_REPORT | LVS_SINGLESEL | LVS_SHOWSELALWAYS | LVS_NOSORTHEADER)
            | WS_TABSTOP,
        IDC_LIST,
        &fonts,
    );
    // LVS_EX_CHECKBOXES rides in column 0's state image, beside its text --
    // it is not a column, so deleting the status column later is compatible.
    // The window style above deliberately keeps LVS_SINGLESEL: ticks are
    // independent of the highlight, so several rows can be marked for
    // deletion while the editor strip still has exactly one current row.
    // LVS_EX_AUTOCHECKSELECT is the opposite of that and must never appear.
    SendMessageW(
        list,
        LVM_SETEXTENDEDLISTVIEWSTYLE,
        Some(WPARAM(0)),
        Some(LPARAM(
            (LVS_EX_FULLROWSELECT | LVS_EX_DOUBLEBUFFER | LVS_EX_CHECKBOXES) as isize,
        )),
    );
    // Seed the cache the Shortcut column's custom draw reads. It is refreshed
    // on `SPI_SETHIGHCONTRAST`; this is the only other moment the answer can
    // be wrong, because the window can be opened into a theme that was already
    // active.
    refresh_high_contrast();
    // No LVCF_WIDTH: `layout` owns every column width, so there is exactly
    // one place a column can be made too wide for its list.
    for (i, (title, fmt)) in LIST_COLUMNS.iter().enumerate() {
        let mut t = wide(title);
        let col = LVCOLUMNW {
            mask: LVCF_TEXT | LVCF_FMT | LVCF_SUBITEM,
            fmt: *fmt,
            pszText: windows::core::PWSTR(t.as_mut_ptr()),
            iSubItem: i as i32,
            ..Default::default()
        };
        SendMessageW(
            list,
            LVM_INSERTCOLUMNW,
            Some(WPARAM(i)),
            Some(LPARAM(&col as *const _ as isize)),
        );
    }
    // The Header is comctl32's own child of the ListView, never a child of
    // `hwnd` -- so it never goes through `child()` and never gets a font
    // via `role_of`. `set_header_font` is the one place that sets it, so
    // creation here and the `WM_DPICHANGED` rebroadcast cannot disagree.
    set_header_font(list, fonts.get(Role::BodyStrong));

    // -- Band 4: the editor group. The strip's two lines live inside it and
    // its caption names the row, so seven controls read as one thing (spec
    // A.1).
    //
    // Created BEFORE its children, same order kept across the reclass below:
    // the caption line at the top of the card should read first regardless
    // of z-order.
    //
    // Not a tab stop: it is not operable, so it must never take the default
    // ring.
    //
    // **Reclassed from `BS_GROUPBOX` to a plain caption `STATIC`** (review
    // finding on Task 8, not a Task 8 original): a themed group-box frame,
    // drawn inside the new rounded `card()` background, read as two frames
    // around one set of controls. The id is unchanged (1034) --
    // `settings_probe` still finds it there and reads its caption with
    // `WM_GETTEXT`, which a `STATIC` answers the same way a `BUTTON` does.
    // `SS_CENTERIMAGE_STYLE`, the same single-line style every other label
    // in this window uses, and deliberately no `BS_NOTIFY` any more --
    // that style only ever meant something on a `BUTTON`. `&` in the
    // caption still needs doubling: a plain `STATIC` reads a lone `&` as a
    // mnemonic prefix exactly like a `BUTTON` caption did, unless
    // `SS_NOPREFIX` is given -- deliberately not given here, so the
    // doubling logic at `apply_state` needed no change. `layout.rs` places
    // this at `grp_x, grp_y, grp_w, s(24)` now, not the group's old full
    // interior height -- see `compute_card_rects`' and `layout`'s own
    // comments on why `card2_h`'s budget does not move.
    child(
        hwnd,
        w!("STATIC"),
        cap::EDITOR_NONE,
        SS_CENTERIMAGE_STYLE,
        IDC_GRP_EDITOR,
        &fonts,
    );

    // -- Band 4: the editor strip. App first, then the shortcut, mirroring
    // the row above it (B.1: "laid out to mirror a row").
    child(
        hwnd,
        w!("STATIC"),
        "App",
        SS_CENTERIMAGE_STYLE,
        IDC_LBL_APP,
        &fonts,
    );
    // CBS_DROPDOWN, not CBS_DROPDOWNLIST: beckon deliberately supports apps
    // with no Start Menu entry, so free typing must stay possible even once
    // the catalog has loaded.
    let app = child(
        hwnd,
        w!("COMBOBOX"),
        "",
        WINDOW_STYLE((CBS_DROPDOWN | CBS_AUTOHSCROLL | CBS_SORT) as u32) | WS_VSCROLL | WS_TABSTOP,
        IDC_APP,
        &fonts,
    );
    // Under comctl32 v6 the `cy` passed to SetWindowPos no longer decides
    // how tall the drop-down is; this does. Without it the list opens at
    // the default 30 items regardless of the height layout computes.
    SendMessageW(app, CB_SETMINVISIBLE, Some(WPARAM(8)), Some(LPARAM(0)));
    child(
        hwnd,
        w!("STATIC"),
        "Shortcut",
        SS_CENTERIMAGE_STYLE,
        IDC_LBL_SHORTCUT,
        &fonts,
    );
    // The four modifier chips, created BEFORE the key list, because
    // creation order IS tab order: Ctrl -> Win -> Alt -> Shift -> key, left
    // to right, which is also the order the canonical string prints them.
    //
    // FOUR here against the `Hold` row's three, and the extra one is
    // `Shift`. `Combo` has a `shift` field and `Chord` deliberately does
    // not: the Caps hook has to release whatever it presses, and releasing
    // Shift under the user's fingers makes everything they type next
    // lowercase. An individual binding presses nothing, so Shift belongs on
    // this row and has nowhere to land on that one.
    //
    // No `&` on any of the four captions. See `mod cap`'s table -- it is
    // the only guard against a mnemonic collision, and `Hold` already
    // claimed `t`, `w` and `l`.
    //
    // **`BS_OWNERDRAW`, which REPLACES `BS_AUTOCHECKBOX` rather than joining
    // it**: the two are alternative values of a BUTTON's four-bit type
    // field, not flags that combine. So these four have no check state of
    // their own -- see `CHIPS`, `check` and `toggle_chip`, which are between
    // them the whole of what Windows used to do here -- and they are drawn
    // by `draw_chip`, through the same painter the Shortcut column uses.
    //
    // **No `BS_NOTIFY`.** It is what `PUSH_BUTTONS` carry so the default
    // ring can follow focus onto them, and a ring on a chip is not a thing
    // Windows draws. Left off, an owner-draw button says exactly two things
    // -- `BN_CLICKED` and `BN_DOUBLECLICKED` -- and `is_chip_click` takes
    // both.
    for (caption, id) in [
        (cap::MOD_CTRL, IDC_MOD_CTRL),
        (cap::MOD_WIN, IDC_MOD_WIN),
        (cap::MOD_ALT, IDC_MOD_ALT),
        (cap::MOD_SHIFT, IDC_MOD_SHIFT),
    ] {
        child(
            hwnd,
            w!("BUTTON"),
            caption,
            WINDOW_STYLE(BS_OWNERDRAW as u32) | WS_TABSTOP,
            id,
            &fonts,
        );
    }
    // CBS_DROPDOWNLIST, not CBS_DROPDOWN -- and, just as deliberately, no
    // CBS_SORT.
    //
    // The key set is closed: 81 names, and nothing else is a key. So unlike
    // the App field there is nothing to free-type, a list with no edit
    // control cannot be left holding text that matches no item, and there
    // is no edit field for a `SetWindowPos` to re-synchronise -- the
    // measured data-loss path in the module header cannot exist on this
    // control.
    //
    // **CBS_SORT would break the index contract.** `ComboView::key` is an
    // index into `key_table()`, `CB_SETCURSEL` takes the same integer, and
    // that holds only while `CB_ADDSTRING` APPENDS in the order the loop
    // below feeds it. A sorted combo box inserts by collation instead --
    // which for this table puts `f10` ahead of `f2` and every digit ahead
    // of every letter -- and every index in the window would then name the
    // wrong key. Nothing reachable from a unit test can catch that, so the
    // style bit is simply absent and this comment is the guard.
    //
    // **`CBS_OWNERDRAWFIXED`, added in Task 9.** Safe here in a way it is
    // NOT for `IDC_APP`: this control has no edit child, so there is no
    // typing path an owner-draw redraw could clobber, and `CB_SETCURSEL` /
    // `CB_GETCURSEL` still move the same integer index regardless of who
    // paints the item. `paint::draw_combo_item` reads the row's text back
    // out of `key_table()` by that index rather than out of the control, so
    // `CBS_HASSTRINGS` is not needed either. `Ui::defid`/`set_default_id`
    // never touch this control -- it is not in `PUSH_BUTTONS` and cannot
    // carry the default ring -- so none of `button`'s own reason for
    // avoiding `BS_OWNERDRAW` on push buttons applies to a COMBOBOX at all.
    let combo = child(
        hwnd,
        w!("COMBOBOX"),
        "",
        WINDOW_STYLE((CBS_DROPDOWNLIST | CBS_OWNERDRAWFIXED) as u32) | WS_VSCROLL | WS_TABSTOP,
        IDC_COMBO,
        &fonts,
    );
    // Same reason the App combo has one: under comctl32 v6 the `cy` passed
    // to SetWindowPos does not decide the dropped-down height, this does.
    // Without it the 81 items open at comctl32's default 30-item guess.
    SendMessageW(combo, CB_SETMINVISIBLE, Some(WPARAM(8)), Some(LPARAM(0)));
    // Filled once, here, from `key_table()` IN ORDER, and never
    // repopulated: the key list is a constant, not data. Each buffer is
    // bound to a local so it outlives its send.
    for k in key_table() {
        let t = wide(&k.name);
        SendMessageW(
            combo,
            CB_ADDSTRING,
            Some(WPARAM(0)),
            Some(LPARAM(t.as_ptr() as isize)),
        );
    }
    // The two commands, created AFTER the key list because creation order is
    // tab order and the strip reads left to right: App -> chips -> key ->
    // Record -> Reset.
    //
    // `BS_NOTIFY` and membership in `PUSH_BUTTONS`, like every other push
    // button here, and on this pair it is load-bearing rather than uniform:
    // without the focus notifications the default ring cannot follow focus
    // onto them, `IsDialogMessageW` falls through to `DM_GETDEFID`, and
    // Enter on a focused `Record` would SAVE. That is the `Reload` defect
    // one band higher.
    for (caption, id) in [(cap::RECORD, IDC_RECORD), (cap::RESET, IDC_RESET)] {
        child(
            hwnd,
            w!("BUTTON"),
            caption,
            WINDOW_STYLE((BS_PUSHBUTTON | BS_NOTIFY) as u32) | WS_TABSTOP,
            id,
            &fonts,
        );
    }
    // On its own line directly beneath the strip, which is where B.1's
    // mock-up draws it. Several lines tall, so no SS_CENTERIMAGE.
    //
    // **`SS_OWNERDRAW` since Task 12** -- a different VALUE of a STATIC's
    // type field, replacing `SS_LEFT` rather than joining it (`draw_chip`'s
    // own reason for `BS_OWNERDRAW`), so this control paints nothing of
    // itself any more, background included: `paint::draw_notes`, reached
    // through `WM_DRAWITEM`, owns the whole surface now, and
    // `WM_CTLCOLORSTATIC` no longer reaches this id (see that arm in
    // `mod.rs`). `SS_NOPREFIX` is kept anyway -- harmless on an owner-draw
    // static, since it never runs the native prefix-parsing `DrawText` path
    // this style would otherwise change (`draw_notes` passes `DT_NOPREFIX`
    // itself) -- as cheap insurance against a future revert away from
    // owner-draw silently losing ampersand handling along with it.
    let notes = child(
        hwnd,
        w!("STATIC"),
        "",
        SS_OWNERDRAW_STYLE | SS_NOPREFIX_STYLE,
        IDC_NOTES,
        &fonts,
    );

    // -- Band 5: the suggestion row. Nothing is created for it and it
    // contributes zero height. A placeholder would be a control to keep in
    // sync with a feature that does not exist yet.

    // -- Band 6: the keyboard group, directly above the command bar. ONE
    // content line, naming the two things the key can do:
    //
    //   [x] Use Caps Lock ...   Hold [x]Ctrl [x]Win [x]Alt   Tap [ v ]
    //
    // It replaces a check box over three radios captioned `Tapping Caps
    // alone: Caps Lock` / `Esc` / `nothing`, where the question governing
    // the group was glued to the first option -- so the other two did not
    // read as answers to it, and `Hold` had no representation at all.
    //
    // **Reclassed from `BS_GROUPBOX` to a plain caption `STATIC`**, same
    // review finding and same reasoning as `IDC_GRP_EDITOR` just above: a
    // themed group-box frame inside the new rounded `card()` background read
    // as two frames around one set of controls. Id unchanged (1019), no
    // `SS_NOPREFIX` (this caption carries no `&` today, but if one is ever
    // added it needs the same doubling `IDC_GRP_EDITOR`'s caption does).
    // `layout.rs` places this at `kb_x, kb_y, kb_w, s(24)` now, not the
    // card's full interior height -- see `compute_card_rects`'s and
    // `layout`'s own comments on why `kb_card_h`'s budget does not move.
    child(
        hwnd,
        w!("STATIC"),
        "Keyboard",
        SS_CENTERIMAGE_STYLE,
        IDC_GRP_KEYBOARD,
        &fonts,
    );
    child(
        hwnd,
        w!("BUTTON"),
        cap::CAPS,
        WINDOW_STYLE(BS_AUTOCHECKBOX as u32) | WS_TABSTOP,
        IDC_CAPS,
        &fonts,
    );
    child(
        hwnd,
        w!("STATIC"),
        cap::HOLD,
        SS_CENTERIMAGE_STYLE,
        IDC_LBL_HOLD,
        &fonts,
    );
    // Three chips, not four: `Chord` has fields `ctrl` / `super_` / `alt`
    // and deliberately no `shift`. The hook has to release whatever it
    // presses, and releasing Shift under the user's fingers makes
    // everything they type next lowercase -- see `Chord`'s own doc. A
    // fourth chip here would have nowhere in the model to land.
    //
    // `BS_OWNERDRAW` for the editor chips' reasons, and these three are in
    // scope WITH those four rather than after them: they name the same three
    // modifiers eight lines apart, and one window wearing two chip styles is
    // worse than either style alone.
    //
    // **These three keep their mnemonics** (`t`, `w`, `l`), which the four
    // above deliberately have not got. That costs `draw_keycaps` a rule the
    // Shortcut column never needed: measure through `shown`, draw the raw
    // caption, and let the window's UI state say whether the underline is
    // visible.
    child(
        hwnd,
        w!("BUTTON"),
        cap::HOLD_CTRL,
        WINDOW_STYLE(BS_OWNERDRAW as u32) | WS_TABSTOP,
        IDC_HOLD_CTRL,
        &fonts,
    );
    child(
        hwnd,
        w!("BUTTON"),
        cap::HOLD_WIN,
        WINDOW_STYLE(BS_OWNERDRAW as u32) | WS_TABSTOP,
        IDC_HOLD_WIN,
        &fonts,
    );
    child(
        hwnd,
        w!("BUTTON"),
        cap::HOLD_ALT,
        WINDOW_STYLE(BS_OWNERDRAW as u32) | WS_TABSTOP,
        IDC_HOLD_ALT,
        &fonts,
    );
    child(
        hwnd,
        w!("STATIC"),
        cap::TAP,
        SS_CENTERIMAGE_STYLE,
        IDC_LBL_TAP,
        &fonts,
    );
    // CBS_DROPDOWNLIST, not CBS_DROPDOWN: the three answers are the whole
    // domain, so unlike the App field there is nothing to free-type here --
    // and a list with no edit field cannot be left holding text that matches
    // no item.
    //
    // `CBS_OWNERDRAWFIXED`, added in Task 9, for `IDC_COMBO`'s own reason:
    // no edit child, so no typing path to endanger, and `paint::draw_combo_item`
    // reads `cap::TAP_ITEMS` by index rather than the control's own text.
    // Still no `CBS_SORT` -- three items, filled in the fixed order
    // `cur_sel`'s callers already assume.
    let tap = child(
        hwnd,
        w!("COMBOBOX"),
        "",
        WINDOW_STYLE((CBS_DROPDOWNLIST | CBS_OWNERDRAWFIXED) as u32) | WS_VSCROLL | WS_TABSTOP,
        IDC_TAP,
        &fonts,
    );
    // Filled once, here, and never repopulated: the items are a constant,
    // not data. Each buffer is bound to a local so it outlives its send.
    for item in cap::TAP_ITEMS {
        let t = wide(item);
        SendMessageW(
            tap,
            CB_ADDSTRING,
            Some(WPARAM(0)),
            Some(LPARAM(t.as_ptr() as isize)),
        );
    }

    // -- Band 7: the command bar. `Open config file` far left, then Close
    // and Save on the right, Save outermost and default.
    //
    // **Save is here, and Remove is not.** They used to share a row
    // mid-window -- a destructive button with no confirm and no undo as the
    // visual peer of the one that writes to disk -- while the bottom bar
    // held only Close. So the bar people aim at held the one command that
    // does not save, and the save prompt on the way out became the real
    // save path.
    //
    // WS_GROUP starts a fresh arrow-key group at the command bar, so the
    // bottom row is its own navigation unit rather than the tail of the
    // keyboard row above it. It used to be described as terminating the
    // radio group `IDC_TAP_CAPSLOCK` opened; there are no radios any more
    // and no group left to close, but the boundary is still the right one
    // to draw here.
    let openfile = child(
        hwnd,
        w!("BUTTON"),
        cap::OPEN_FILE,
        WINDOW_STYLE((BS_PUSHBUTTON | BS_NOTIFY) as u32) | WS_GROUP | WS_TABSTOP,
        IDC_OPENFILE,
        &fonts,
    );
    child(
        hwnd,
        w!("BUTTON"),
        cap::CLOSE,
        WINDOW_STYLE((BS_PUSHBUTTON | BS_NOTIFY) as u32) | WS_TABSTOP,
        IDC_CLOSE,
        &fonts,
    );
    // BS_DEFPUSHBUTTON draws the default ring; the `DM_GETDEFID` arm of
    // `wndproc` is what makes Enter honour it. This window is not a dialog
    // box, so without that arm `IsDialogMessageW` falls back to IDOK -- an
    // id nothing here answers to -- and the ring promises a key that does
    // nothing, which is exactly what shipped.
    //
    // This is only the STARTING default. `set_default_id` moves both the
    // style and `Ui::defid` as focus walks the command bar, so the ring and
    // the key agree wherever focus is.
    child(
        hwnd,
        w!("BUTTON"),
        cap::SAVE,
        WINDOW_STYLE((BS_DEFPUSHBUTTON | BS_NOTIFY) as u32) | WS_TABSTOP,
        IDC_APPLY,
        &fonts,
    );

    // The config path: the title bar gets its file name, the tooltip on
    // `Open config file` gets the whole thing. A title bar cannot hold a
    // path -- taskbar and Alt-Tab labels truncate from the right, dropping
    // the one part that identifies the file -- and a title bar has no
    // tooltip of its own, so the button that opens the file is where the
    // answer to "which file?" belongs.
    let path = CFG
        .with(|c| c.borrow().as_ref().map(|p| p.config.clone()))
        .unwrap_or_default();
    let mut tip_text = wide(&path.to_string_lossy());
    add_tooltip(hwnd, openfile, &mut tip_text);

    // Every control now exists, so this is the first moment the page rule can
    // be applied at all -- and it has to be applied HERE rather than through
    // `show_page`, which returns early on an unchanged door and so cannot
    // establish the door `open` asked for. `external_change` is false at
    // creation: `serve` pushes the first state after `open` returns, and a
    // file cannot have moved under a window that does not exist yet.
    show_page_controls(hwnd, PAGE.with(|p| p.get()), false);

    UI.with(|u| {
        *u.borrow_mut() = Some(Ui {
            hwnd,
            list,
            combo,
            app,
            notes,
            filter,
            banner,
            reload,
            keep,
            tap,
            fonts,
            accel: build_accelerators(),
            // Matches the `BS_DEFPUSHBUTTON` handed to `IDC_APPLY` above.
            // The field and the style start out agreeing, and
            // `set_default_id` is the only thing that moves either, so they
            // cannot drift apart.
            defid: IDC_APPLY,
            title_base: title_base(&path),
            // Moved, not copied: the heap buffer `add_tooltip` handed
            // comctl32 a pointer to travels with it.
            tip_text,
            shown_dirty: None,
            shown_external: None,
            shown_empty: None,
            // `None`, not the page `show_page_controls` was just handed:
            // `WM_CREATE`'s own `layout` runs after this, so the first push
            // must lay out unconditionally exactly as its two neighbours
            // make it.
            shown_page: None,
            suppress: false,
            external_change: false,
            items: Vec::new(),
            app_epoch: 0,
            shown_combo: None,
            capture: None,
            theme: theme::ThemeCache::default(),
            hot: None,
        })
    });
}

/// The window's accelerator table: `Ctrl+S` -> Save, and nothing else.
///
/// Enter and Esc are deliberately absent. Both are the dialog manager's
/// already — Enter through `DM_GETDEFID`, Esc through the `IDCANCEL`
/// `WM_COMMAND` it synthesises — and an entry here would only race
/// `IsDialogMessageW` for keys it already routes correctly.
///
/// An empty or failed table is not fatal: `filter_dialog_message` skips an
/// invalid handle and every command it would have carried is still reachable
/// by mouse, by mnemonic and by Tab-then-Enter.
unsafe fn build_accelerators() -> HACCEL {
    let table = [ACCEL {
        // FVIRTKEY is what makes `key` a virtual-key code rather than a
        // character, and it is REQUIRED for FCONTROL to mean anything.
        fVirt: FVIRTKEY | FCONTROL,
        key: b'S' as u16,
        cmd: IDC_APPLY as u16,
    }];
    CreateAcceleratorTableW(&table).unwrap_or_default()
}

/// Attach `text` to `tool` as a tooltip, through a tooltip window owned by
/// `parent`.
///
/// Three details that are each load-bearing:
///
/// - **`TTS_NOPREFIX`**, for the same reason `IDC_NOTES` carries
///   `SS_NOPREFIX`: this text is a file path, and a path may contain `&`
///   (`C:\Users\A&B\...`), which a tooltip would otherwise eat as a
///   mnemonic marker.
/// - **`TTF_SUBCLASS`** makes the tooltip subclass `tool` to collect its own
///   mouse messages, so no relaying is needed from `serve`'s message loop.
/// - **`text` is borrowed, not copied.** `TTM_ADDTOOLW` stores the
///   `lpszText` pointer; the buffer must outlive the tooltip, which is why
///   the caller keeps it in `Ui`.
///
/// The tooltip is a `WS_POPUP` OWNED by `parent`, not a child of it, so it
/// is destroyed with the window and it is skipped by the `GW_CHILD` walks
/// that rebroadcast `WM_SETFONT` and `WM_THEMECHANGED` — which is right:
/// a tooltip draws itself from the theme, and it has no `role_of` entry.
unsafe fn add_tooltip(parent: HWND, tool: HWND, text: &mut [u16]) {
    // `wide("")` is one NUL, not an empty slice -- so test the first wchar,
    // not the length. A tooltip with nothing in it is worse than none: it
    // still opens, as an empty box.
    if tool.is_invalid() || text.first().copied().unwrap_or(0) == 0 {
        return;
    }
    let Ok(tip) = CreateWindowExW(
        WINDOW_EX_STYLE(0),
        w!("tooltips_class32"),
        PCWSTR::null(),
        WS_POPUP | WINDOW_STYLE(TTS_ALWAYSTIP | TTS_NOPREFIX),
        CW_USEDEFAULT,
        CW_USEDEFAULT,
        CW_USEDEFAULT,
        CW_USEDEFAULT,
        Some(parent),
        None,
        None,
        None,
    ) else {
        return;
    };
    // Without a maximum width a tooltip is ONE line, however long the text
    // is -- and this text is a file path, which is the longest string the
    // window can be asked to show. `C:\Users\<user>\AppData\Roaming\beckon\
    // shortcuts.toml` is already wider than the window at 96 DPI, and a
    // tooltip is not clipped to the monitor politely: it is drawn where it
    // was asked for and runs off the edge, losing the tail, which is the
    // half that says which file.
    //
    // Setting any positive maximum is also what switches the control into
    // its multi-line layout at all -- the width is the trigger, not just a
    // bound. Scaled, like every other pixel in this file: an unscaled 400
    // would wrap after a third of a line at 300 % DPI.
    SendMessageW(
        tip,
        TTM_SETMAXTIPWIDTH,
        Some(WPARAM(0)),
        Some(LPARAM(
            scale(tok::TOOLTIP_MAX, GetDpiForWindow(parent).max(96)) as isize,
        )),
    );
    let info = TTTOOLINFOW {
        cbSize: std::mem::size_of::<TTTOOLINFOW>() as u32,
        uFlags: TTF_IDISHWND | TTF_SUBCLASS,
        hwnd: parent,
        // With TTF_IDISHWND the id IS the control's handle.
        uId: tool.0 as usize,
        lpszText: windows::core::PWSTR(text.as_mut_ptr()),
        ..Default::default()
    };
    SendMessageW(
        tip,
        TTM_ADDTOOLW,
        Some(WPARAM(0)),
        Some(LPARAM(&info as *const _ as isize)),
    );
}

// ---------------------------------------------------------------------------
// Layout
// ---------------------------------------------------------------------------

/// The size of `s` rendered in `font`, in physical pixels.
///
/// Widths are measured, never tabulated, so a button is never narrower
/// than its own caption and a label never overlaps the field beside it —
/// the defects B.2 records, all of which come from constants that were
/// right for one font at one DPI. B.3 has since given the window three
/// fonts, and this needed no change to survive it — but the CALLER now has
/// to pass the font of the role it is measuring FOR, not whichever handle
/// is nearest to hand, or a caption gets a box sized for a different face.
///
/// The estimate on the failure path is deliberately generous: too wide
/// costs a gap, too narrow clips.
unsafe fn text_size(hwnd: HWND, font: HFONT, dpi: u32, s: &str) -> (i32, i32) {
    let est = (
        scale(8, dpi) * s.chars().count() as i32,
        scale(16, dpi).max(1),
    );
    let dc = GetDC(Some(hwnd));
    if dc.is_invalid() {
        return est;
    }
    let prev = SelectObject(dc, HGDIOBJ(font.0));
    let text = wide(s);
    let mut sz = SIZE::default();
    // `wide` appends a NUL and this API takes a length, so the NUL would
    // be measured as a character.
    let ok = GetTextExtentPoint32W(dc, &text[..text.len() - 1], &mut sz).as_bool();
    if !prev.is_invalid() {
        SelectObject(dc, prev);
    }
    ReleaseDC(Some(hwnd), dc);
    if ok && sz.cy > 0 {
        (sz.cx, sz.cy)
    } else {
        est
    }
}

/// How one run of keycaps is drawn.
///
/// **Two styles, ONE painter.** The Shortcut column and the seven toggle
/// chips draw the same object -- a key on a keyboard -- and a second painter
/// for the second surface is how the two quietly stop agreeing about what a
/// key looks like. Everything that differs between them is a field here
/// rather than a fork inside `draw_keycaps`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum CapStyle {
    /// A whole chord in a ListView cell: left-aligned so the column lines up
    /// down its own length, and the last cap -- the key actually pressed --
    /// brighter than the modifiers holding it down. A cell holds DATA, never
    /// a caption, so an `&` in it is a character and is drawn.
    Chord,
    /// One toggle chip filling its own control rect: centred in it, and
    /// filled with the user's accent while armed.
    ///
    /// Every flag here is something `BS_OWNERDRAW` stops Windows doing on
    /// its own. An owner-draw button has no disabled rendering, no pressed
    /// rendering and no mnemonic handling beyond what its parent draws --
    /// see `draw_chip`, which is the only place these are filled in.
    Toggle {
        armed: bool,
        pressed: bool,
        disabled: bool,
        /// The window's UI state says keyboard cues are hidden, i.e. Alt has
        /// not been pressed yet, so the mnemonic underline stays off.
        hide_accel: bool,
    },
}

/// One ListView row, in physical pixels at the live DPI.
///
/// **Queried, never scaled from a token, when a row exists to measure.**
/// Pre-Task-10, 29 px measured on a14 at 144 DPI was 19.33 at 96, and a
/// non-integer was the tell that comctl32 derived the row height from the
/// font rather than from a design constant. Since Task 10 the height is
/// FORCED by `rebuild_state_image_list`'s state image list to be at least
/// `scale(tok::ROW_H, dpi)`, but comctl32 is still free to add its own
/// padding on top of that image height, so the real answer is asked of the
/// control rather than assumed either way -- a 96-DPI token pushed through
/// `scale` would still be wrong at every non-integer scale.
///
/// `LVM_GETITEMRECT` needs a row to measure. When the list is empty there is
/// none, so this falls back to `scale(tok::ROW_H, dpi)` -- the forced LOWER
/// BOUND, not a hardware measurement: since Task 10 the true figure can only
/// be equal to or greater than it, never less.
///
/// **The list's item count is therefore an input to `layout`, and
/// `apply_state` has to treat it as one.** This comment used to say the
/// fallback barely mattered because `apply_state` re-lays-out "the instant it
/// puts a row in"; `Ui::shown_external` made that false by laying out only
/// when the banner's visibility moves. `Ui::shown_empty` is the other half of
/// that guard -- do not remove it without putting an unconditional `layout`
/// back, and do not restore the old claim.
unsafe fn list_row_height(list: HWND, dpi: u32) -> i32 {
    let count = SendMessageW(list, LVM_GETITEMCOUNT, Some(WPARAM(0)), Some(LPARAM(0))).0;
    if count > 0 {
        // `left` is the input: which of the item's rectangles is wanted.
        let mut rc = RECT {
            left: LVIR_BOUNDS as i32,
            ..Default::default()
        };
        let got = SendMessageW(
            list,
            LVM_GETITEMRECT,
            Some(WPARAM(0)),
            Some(LPARAM(&mut rc as *mut RECT as isize)),
        )
        .0 != 0;
        let h = rc.bottom - rc.top;
        if got && h > 0 {
            return h;
        }
    }
    scale(tok::ROW_H, dpi)
}

/// Give the ListView's own Header control a font.
///
/// comctl32 does not propagate a `WM_SETFONT` sent to the ListView down to
/// its Header child -- the two are separate windows -- so without this the
/// column headers stay on whatever font the Header was born with. Called
/// once at creation (`build_children`) and again on every `WM_DPICHANGED`,
/// because the Header is a child of `list`, not of `hwnd`, and so is never
/// reached by that handler's `GW_CHILD` / `GW_HWNDNEXT` walk.
unsafe fn set_header_font(list: HWND, font: HFONT) {
    let hdr = HWND(SendMessageW(list, LVM_GETHEADER, Some(WPARAM(0)), Some(LPARAM(0))).0 as *mut _);
    if !hdr.is_invalid() {
        SendMessageW(
            hdr,
            WM_SETFONT,
            Some(WPARAM(font.0 as usize)),
            Some(LPARAM(1)),
        );
    }
}

/// The ListView's Header child, by `HWND` rather than by dialog control id --
/// the Header carries none of its own, so this is the only way to name it
/// from a `WM_NOTIFY`'s bare `hwndFrom` (Task 10's header custom draw).  Same
/// `LVM_GETHEADER` round trip `set_header_font` / `list_header_height` already
/// pay; a fourth call site rather than a cache, because it is only reached on
/// a custom-draw notification that neither `IDC_LIST` nor a push button
/// already claimed -- at most once per repaint.
unsafe fn header_of(hwnd: HWND) -> HWND {
    let Ok(list) = GetDlgItem(Some(hwnd), IDC_LIST) else {
        return HWND::default();
    };
    HWND(SendMessageW(list, LVM_GETHEADER, Some(WPARAM(0)), Some(LPARAM(0))).0 as *mut _)
}

/// The ListView's header, in physical pixels at the live DPI. Measured 31
/// at 144 DPI, which is 20.67 at 96 — a non-integer for the same reason a
/// row is, so it is asked for rather than tabulated.
unsafe fn list_header_height(list: HWND, dpi: u32) -> i32 {
    let hdr = HWND(SendMessageW(list, LVM_GETHEADER, Some(WPARAM(0)), Some(LPARAM(0))).0 as *mut _);
    if !hdr.is_invalid() {
        let mut rc = RECT::default();
        if GetWindowRect(hdr, &mut rc).is_ok() {
            let h = rc.bottom - rc.top;
            if h > 0 {
                return h;
            }
        }
    }
    scale(21, dpi)
}

/// Replace `list`'s checkbox state image list with one tall enough to force
/// `tok::ROW_H` rows -- a ListView's row height comes from its image list,
/// not from a token of its own (see `tok::ROW_H`'s own comment), so the
/// state list the per-row ticks already ride in is the lever.
///
/// **The glyphs are comctl32's own, not hand-drawn.** `LVS_EX_CHECKBOXES`
/// already built two correctly-themed, correctly-DPI-scaled checkbox images
/// the moment it was turned on; `DrawFrameControl` would draw the pre-Vista
/// flat checkbox instead of the rounded one Explorer itself uses, sitting
/// oddly next to `theme_list`'s `SetWindowTheme("DarkMode_Explorer")`. Each
/// glyph is copied, unmodified, onto a taller canvas sized
/// `s(16) x s(tok::ROW_H)`, centred -- the tick itself stays whatever size
/// comctl32 drew it at (~15 px), only the cell around it grows.
///
/// **A real 32-bit alpha canvas, not a legacy colour-key mask.** GDI drawing
/// onto an ordinary `CreateCompatibleBitmap` does not reliably touch the
/// alpha byte, so the canvas is a `CreateDIBSection` this function zeroes by
/// hand (alpha 0, fully transparent) before every frame. `ImageList_Draw` --
/// unlike a raw `BitBlt`/`DrawFrameControl` -- is comctl32's own API and is
/// alpha-aware when the destination is a real 32bpp DIB, so the composited
/// glyph keeps a working alpha channel that `ImageList_Add`'s `ILC_COLOR32`
/// list (no mask argument) can use directly.
///
/// Called once at creation (`create`, after the theme and DPI are both
/// final) and again on every `WM_DPICHANGED`. **Known limitation, disclosed
/// rather than fixed**: after the first call, `LVM_GETIMAGELIST` reads back
/// OUR OWN previous composite as the source to re-centre, not comctl32's
/// native default -- once we own `LVSIL_STATE` there is no API to ask
/// comctl32 to regenerate its own default at a new DPI. The CELL still
/// rescales correctly on a live DPI change (`cx`/`cy` are computed fresh
/// every call); the tick's own pixel resolution does not, and stays at
/// whatever DPI first installed it. A monitor move is the only way to reach
/// this, and the result is a soft-scaled tick, never a missing or
/// mis-centred one.
///
/// **Unverified on hardware.** This host cannot run Windows; Gate 05
/// (Task 15) is what actually confirms the tick still centres.
unsafe fn rebuild_state_image_list(list: HWND, dpi: u32) {
    let s = |v: i32| v * dpi as i32 / 96;
    let cx = s(16);
    let cy = s(tok::ROW_H);
    if cx <= 0 || cy <= 0 {
        return;
    }

    let src = HIMAGELIST(
        SendMessageW(
            list,
            LVM_GETIMAGELIST,
            Some(WPARAM(LVSIL_STATE as usize)),
            Some(LPARAM(0)),
        )
        .0,
    );
    if src.is_invalid() {
        return;
    }
    let (mut src_cx, mut src_cy) = (0i32, 0i32);
    let _ = ImageList_GetIconSize(
        src,
        Some(&mut src_cx as *mut i32),
        Some(&mut src_cy as *mut i32),
    );
    if src_cx <= 0 || src_cy <= 0 {
        return;
    }

    let il = ImageList_Create(cx, cy, ILC_COLOR32, 2, 0);
    if il.is_invalid() {
        return;
    }

    let screen = GetDC(None);
    let mem = CreateCompatibleDC(Some(screen));
    if mem.is_invalid() {
        let _ = ReleaseDC(None, screen);
        let _ = ImageList_Destroy(Some(il));
        return;
    }
    let header = BITMAPINFOHEADER {
        biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
        biWidth: cx,
        // Negative: top-down, so (0,0) is the top-left -- the same sense
        // `ImageList_Draw`'s own (x, y) offset expects.
        biHeight: -cy,
        biPlanes: 1,
        biBitCount: 32,
        biCompression: 0, // BI_RGB
        ..Default::default()
    };
    let bmi = BITMAPINFO {
        bmiHeader: header,
        ..Default::default()
    };
    let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
    let dib = CreateDIBSection(Some(mem), &bmi, DIB_RGB_COLORS, &mut bits, None, 0);
    let _ = ReleaseDC(None, screen);
    let Ok(dib) = dib else {
        let _ = DeleteDC(mem);
        let _ = ImageList_Destroy(Some(il));
        return;
    };

    let xoff = ((cx - src_cx) / 2).max(0);
    let yoff = ((cy - src_cy) / 2).max(0);
    let px_bytes = (cx as usize) * (cy as usize) * 4;

    // Unchecked (state 1, image list index 0), then checked (state 2, index
    // 1) -- `LVIS_UNCHECKED`/`LVIS_CHECKED`'s own one-based-minus-one
    // mapping, unchanged by this swap.
    for i in 0..2i32 {
        let old_bmp = SelectObject(mem, HGDIOBJ(dib.0));
        if !bits.is_null() {
            std::ptr::write_bytes(bits as *mut u8, 0, px_bytes);
        }
        let _ = ImageList_Draw(src, i, mem, xoff, yoff, ILD_TRANSPARENT);
        // Deselected before `ImageList_Add` touches the bitmap directly --
        // GDI does not allow a bitmap to be read by one caller while it is
        // still selected into a DC of ours.
        SelectObject(mem, old_bmp);
        ImageList_Add(il, dib, None);
    }
    let _ = DeleteObject(HGDIOBJ(dib.0));
    let _ = DeleteDC(mem);

    let prev = SendMessageW(
        list,
        LVM_SETIMAGELIST,
        Some(WPARAM(LVSIL_STATE as usize)),
        Some(LPARAM(il.0)),
    );
    // `LVM_SETIMAGELIST` does not free the list it displaces -- ours to
    // free, the same rule as any other `HIMAGELIST` we swap in.
    let old = HIMAGELIST(prev.0);
    if !old.is_invalid() && old.0 != il.0 {
        let _ = ImageList_Destroy(Some(old));
    }
}

/// Set a column's width, but only when it is not already right.
///
/// `apply_state` calls `layout`, and `apply_state` runs on every keystroke,
/// so this write happens per keystroke — and a width write invalidates the
/// header whether or not the number changed. Reading first is the same
/// guard, for the same reason, as the one on `set_item_state`.
unsafe fn set_column_width(list: HWND, col: usize, cx: i32) {
    let cur = SendMessageW(list, LVM_GETCOLUMNWIDTH, Some(WPARAM(col)), Some(LPARAM(0))).0;
    if cur == cx as isize {
        return;
    }
    SendMessageW(
        list,
        LVM_SETCOLUMNWIDTH,
        Some(WPARAM(col)),
        Some(LPARAM(cx as isize)),
    );
}

/// Height of the notes line inside the editor group: exactly two lines of
/// the notes' own face, whatever the DPI and whatever the face.
///
/// **Fixed, not flexing.** It used to take every pixel between the strip and
/// the keyboard group, which measured on a14 as a 1220x177 control holding a
/// single 258 px line -- a large blank band whose only job was to exist.
///
/// Two is a guess and is worth revisiting: nobody has looked at what three
/// notes at once reads like, which is exactly the gap the followups record.
/// It is a cheap guess to change and an expensive band to leave empty.
///
/// **It is an input to `MIN_HEIGHT`.** The floor is derived from `card2_h`
/// (Task 8's card wrapping `grp_h`), and `card2_h` is derived from this --
/// change what a notes line costs and the floor moves with it. 16 px (96
/// DPI, derived) / 24 px (144 DPI, measured): the 144 figure IS a fresh a14
/// reading -- item 10 of the 2026-08-11 a14 pass sized the read-only notes
/// STATIC against "5 lines x 24" at 144 DPI, the same Caption face this line
/// measures. The 96 DPI figure comes from applying the same internal-leading
/// ratio the Body font showed at that pass (`text_h` 28 against a requested
/// 21, i.e. 4/3) to Caption's 12 px request -- and that same ratio, applied
/// to Caption's 144-DPI request of 18, reproduces the hardware 24 exactly,
/// which is why it is trusted for the DPI nobody has measured. If a real
/// 96-DPI reading disagrees, `MIN_HEIGHT` must be re-derived from it, not
/// nudged -- though the disagreement is bounded, not open-ended: the derived
/// window height is `543 + 2(L - 16)` for a real Caption line height `L`.
/// The FORM has never changed -- `notes_h` is a single linear term inside
/// `card2_h`, which is a single linear term inside the total, so the
/// coefficient survives every re-derivation and only the anchor moves. Six
/// anchors so far: 546 before Task 7's title bar, 555 after it, 675 after
/// Task 8's cards, 697 after Task 10's 26 px rows, 553 after the 2026-08-13
/// compaction pass, and 543 after the tab strip -- which is the one this line
/// now carries.
///
/// **The sixth anchor is not the fifth plus the strip's 34.** It is 10
/// LOWER, because the row count it solves for changed in the same landing:
/// `MIN_HEIGHT` promises two rows now, not four (see its own comment for the
/// withdrawal). Two rows banner-up is `client = 507 + notes_h` = `511 + 2L`
/// at 96 DPI, i.e. 543 at `L = 16`. The four-row floor did move by the full
/// 34, from 553 to 587; it is simply not what anything is derived from any
/// more.
///
/// (**CORRECTED 2026-08-14: the fifth anchor read 561 and is 553.** That
/// anchor's own derivation, from the window before the tab strip: the
/// banner-up four-row floor is `client = 517 + notes_h`, and
/// `notes_h = 2L + scale(4, dpi)`, so `client = 521 + 2L` at 96 DPI -- 553 at
/// `L = 16`. 561 was that same 553 plus an 8 px bottom frame the shipped
/// `chrome::nccalcsize` does not reserve: it returns `LRESULT(0)` without
/// calling `DefWindowProcW` and reads neither parameter, so client == window
/// on all four edges. What makes the error believable is its timing.
/// `c523e8e` reclaimed the frame at 23:18 on 2026-08-13; 561 was written the
/// next afternoon in `9e4e026`, a pass whose whole subject was making eight
/// copies of this geometry agree. They did agree -- on a term that had
/// stopped existing fourteen hours earlier. Agreement is not correctness,
/// and this is what that failure looks like.)
///
/// (**CORRECTED earlier**: this sentence said "Four anchors" and skipped
/// Task 8's 675. The three separate re-derivation paragraphs that stood here
/// were compressed into one sentence in `9e4e026`, and the middle anchor was
/// lost in the compression rather than refuted. It is in that commit's
/// parent, under "Re-derived for Task 8's cards": at the banner-up four-row
/// floor `client = 631 + notes_h` exactly, and `notes_h = 2L + scale(4,
/// dpi)`, so `client = 635 + 2L` at 96 DPI -- `675 + 2(L-16)` once the `+8`
/// non-client frame and the `-32` from centring the formula on `L = 16` are
/// folded in. **That `+8` is correct where it stands and must not be
/// "fixed":** Task 8 predates `c523e8e` by weeks, so the bottom edge really
/// was non-client when 675 was derived. It records an old geometry; only the
/// current anchor answers for this one.)
///
/// `MIN_HEIGHT`'s own table is where 543 comes from; re-read it there rather
/// than trusting this sentence.
///
/// **The shipped 560 absorbs `L = 16` with the two-row banner-up guarantee
/// intact, and 17 px to spare.** The list is handed `114 - 2L` px at the
/// floor, against the 65 two rows need, so the guarantee holds to `L <= 24`;
/// at `L = 25` the list draws one whole row and 21 px of a second, and it
/// does not lose that one until `L = 36`. Nothing
/// there can overlap: `editor_min = card2_h` in `compute_card_rects` (see its
/// own comment) is computed from the RUNTIME value, not from this estimate,
/// so a wrong `L` can only shrink the list at the absolute floor. That is the
/// safe direction, and it is why a large `L` would be a note rather than a
/// bug.
///
/// (Pre-strip this read `148 - 2L` against the 109 four rows need, holding to
/// `L <= 19`. The 34 px the strip takes moves every figure in the sentence;
/// the shape of it -- one linear term, a floor, and a bounded, safe-direction
/// error -- is what survives the move.)
///
/// (**CORRECTED 2026-08-14**: this paragraph read "The shipped 560 does NOT
/// absorb `L = 16` with the four-row banner-up guarantee intact -- it is one
/// pixel short of it ... The guarantee holds at `L <= 15`; from `L = 16` the
/// list draws three whole rows and part of a fourth, and it does not lose a
/// second whole row until `L = 27`." Every figure was 8 px of list short, off
/// the same phantom bottom frame -- the trace ran `140 - 2L` where the
/// shipped window gives `148 - 2L`. The shortfall it reported does not
/// exist, and `MIN_HEIGHT`'s "recorded rather than fixed" note went with it.)
unsafe fn notes_height(hwnd: HWND, ui: &LayoutHandles, dpi: u32) -> i32 {
    let line = text_size(hwnd, ui.fonts.get(Role::Caption), dpi, "Ag").1;
    line * 2 + scale(4, dpi)
}

// ---------------------------------------------------------------------------
// Drawing
// ---------------------------------------------------------------------------

/// Push a snapshot into the controls. The only path that changes what is on
/// screen; the window never reads the model.
pub fn apply_state(st: &ControlState, external_change: bool, catalog: Option<&[String]>) {
    let Some((hwnd, list, combo, app, notes, filter, banner, reload, keep, tap)) = UI.with(|u| {
        u.borrow().as_ref().map(|x| {
            (
                x.hwnd, x.list, x.combo, x.app, x.notes, x.filter, x.banner, x.reload, x.keep,
                x.tap,
            )
        })
    }) else {
        return;
    };
    // Taken, not cloned, and taken HERE: every `SendMessageW` below can
    // re-enter this window's wndproc, `suppressed()` takes a fresh
    // `UI.borrow()` when it does, and a second borrow across an
    // `extern "system"` boundary aborts the process instead of unwinding.
    // So no borrow may be alive once the sending starts. Taking also makes
    // the failure mode safe: a lost cache means the next push rebuilds.
    let prev: Vec<ListItem> = UI.with(|u| {
        u.borrow_mut()
            .as_mut()
            .map(|x| std::mem::take(&mut x.items))
            .unwrap_or_default()
    });
    // The title, when and only when the dirty mark flips. `apply_state` runs
    // on every keystroke and `SetWindowTextW` on a top-level window repaints
    // the caption and pokes the taskbar, so an unconditional write would put
    // that on the typing path for no change on screen.
    //
    // `*`, ASCII, for the reason `title_base` gives. The borrow is taken and
    // dropped on these lines -- `set_text` below is a `WM_SETTEXT` send.
    let new_title: Option<String> = UI.with(|u| {
        u.borrow().as_ref().and_then(|x| {
            if x.shown_dirty == Some(st.dirty) {
                return None;
            }
            Some(if st.dirty {
                format!("*{}", x.title_base)
            } else {
                x.title_base.clone()
            })
        })
    });
    // Writing control text raises EN_CHANGE / CBN_EDITCHANGE. Without this
    // guard every repaint would feed the control's own text back into the
    // model and mark it dirty. It is also what swallows the LVN_ITEMCHANGED
    // that `sync_list`'s own `LVM_SETITEMSTATE` fires synchronously.
    UI.with(|u| {
        if let Some(ui) = u.borrow_mut().as_mut() {
            ui.suppress = true;
            ui.external_change = external_change;
        }
    });

    // What a live capture wants on the notes strip, or `None` when the model
    // owns it as usual. ONE borrow, dropped on this line; `capture_notes`
    // allocates but makes no OS call.
    //
    // **This is what stops an unrelated push undoing a recording.** A
    // file-watch tick or a catalog arriving lands here mid-capture, and
    // without this it would re-enable the five typed controls and write the
    // model's notes over the prompt -- two writers on one value, which is
    // the defect spec C.4 forbids by name.
    let cap_notes: Option<Vec<Note>> = UI.with(|u| {
        u.borrow()
            .as_ref()
            .and_then(|x| x.capture.as_ref().map(capture_notes))
    });
    let capturing = cap_notes.is_some();

    // Did this push write the App field? Collected here and acted on in the
    // trailing block, which is equivalent to bumping at the write itself:
    // the stamp only has to be current before a POSTED message can be
    // dispatched, and nothing between these two points pumps the queue.
    let mut wrote_app = false;

    unsafe {
        if let Some(t) = &new_title {
            set_text(hwnd, t);
        }
        sync_list(list, &prev, st);

        if let Some(names) = catalog {
            // Repopulating on every repaint would fight the user's typing;
            // only fill an empty list.
            let count = SendMessageW(app, CB_GETCOUNT, Some(WPARAM(0)), Some(LPARAM(0))).0;
            if count <= 0 {
                for n in names {
                    let t = wide(n);
                    SendMessageW(
                        app,
                        CB_ADDSTRING,
                        Some(WPARAM(0)),
                        Some(LPARAM(t.as_ptr() as isize)),
                    );
                }
            }
        }

        match &st.detail {
            Some(d) => {
                // `st.editable`, not `true`: there is a row to show but the
                // file it came from may be one beckon could not read. The
                // window is not told which -- see `ControlState::editable`.
                //
                // `&& !capturing` for a different reason, and it is the one
                // spec C.4 states: the five controls and the recording hook
                // are two views of one value, and two writers on one value
                // is the App field's measured defect in another costume.
                for id in SHORTCUT_CONTROLS {
                    enable(hwnd, id, st.editable && !capturing);
                }
                enable(hwnd, IDC_APP, st.editable);
                // The shortcut, as the five controls that show it.
                //
                // The four `check` calls need no read guard, unlike every
                // text write in this function: on an owner-draw chip `check`
                // writes a `Cell` and marks a rectangle dirty, which raises
                // nothing at all, so a push cannot feed itself back as a
                // user click. It carries its OWN read guard for a different
                // reason -- `apply_state` runs per keystroke and an
                // unconditional repaint would flicker; see `set_chip`.
                // `set_key_sel` is guarded too, for the reason written on it.
                //
                // A string that does not parse arrives here as
                // `ComboView::default()` -- nothing ticked, nothing selected
                // -- rather than as an error. That is the right thing to
                // SHOW: `Model::problems` is what says why, and it is
                // already in the notes below.
                let v = combo_view(&d.combo);
                check(hwnd, IDC_MOD_CTRL, v.ctrl);
                check(hwnd, IDC_MOD_WIN, v.super_);
                check(hwnd, IDC_MOD_ALT, v.alt);
                check(hwnd, IDC_MOD_SHIFT, v.shift);
                set_key_sel(combo, v.key);
                if text_of(app) != d.app {
                    set_text(app, &d.app);
                    wrote_app = true;
                }
                // The capture prompt outranks the row's notes while one is
                // live: it is the only thing on screen telling the user what
                // beckon is doing with their keyboard.
                let body: Vec<Note> = match &cap_notes {
                    Some(lines) => lines.clone(),
                    None => {
                        // Caps at two NOTES, not two RENDERED lines -- but
                        // now that IS the same cap: `paint::draw_notes`
                        // draws exactly one `DT_SINGLELINE | DT_END_ELLIPSIS`
                        // line per entry, at the fixed height `notes_height`
                        // budgets, so a long note truncates with an ellipsis
                        // instead of wrapping onto a line nothing reserved
                        // room for -- the old failure mode, where a wrapped
                        // note could push "(+N more)" onto a clipped third
                        // line, is structurally gone. "(+N more)" is folded
                        // into the SECOND note's own text rather than added
                        // as a third entry, so it can never exceed the
                        // two-line budget either.
                        const NOTE_LINES: usize = 2;
                        let mut body: Vec<Note> =
                            d.notes.iter().take(NOTE_LINES).cloned().collect();
                        if d.notes.len() > NOTE_LINES {
                            if let Some(last) = body.last_mut() {
                                last.text
                                    .push_str(&format!("  (+{} more)", d.notes.len() - NOTE_LINES));
                            }
                        }
                        body
                    }
                };
                show_notes(notes, body);
            }
            None => {
                for id in SHORTCUT_CONTROLS {
                    enable(hwnd, id, false);
                }
                enable(hwnd, IDC_APP, false);
                // All five cleared, through the same two calls the `Some`
                // arm sets them with -- so there is one description of what
                // each control does with a value and no second one for the
                // empty case to drift from.
                check(hwnd, IDC_MOD_CTRL, false);
                check(hwnd, IDC_MOD_WIN, false);
                check(hwnd, IDC_MOD_ALT, false);
                check(hwnd, IDC_MOD_SHIFT, false);
                set_key_sel(combo, None);
                // Conditional, like the `Some` arm above, and for the same
                // two reasons: an unconditional `WM_SETTEXT` raises an
                // `EN_CHANGE` / `CBN_EDITCHANGE` on every push, and clearing
                // a field that is already clear must not invalidate a
                // pending read of it.
                if !text_of(app).is_empty() {
                    set_text(app, "");
                    wrote_app = true;
                }
                // Reachable while a capture is live: the file can change on
                // disk mid-recording and take the selection with it. The
                // prompt still outranks the placeholder, and the commit that
                // follows simply lands nowhere -- `on_edit_combo` reaches a
                // model with no `selected` and changes nothing.
                //
                // `Mark::Unknown` for the placeholder: it is an
                // informational, not-yet-decided state -- the same meaning
                // `row_condition` already gives `Mark::Unknown` elsewhere
                // ("Checking installed apps...", "Not registered yet."), not
                // a new case invented for this one line.
                let body = cap_notes.clone().unwrap_or_else(|| {
                    vec![Note {
                        mark: Mark::Unknown,
                        text: "Select a shortcut, or press Add.".into(),
                    }]
                });
                show_notes(notes, body);
            }
        }
        // The card head's caption, and it is a TEXT write, not a geometry
        // one: it must never reach `layout`, because `layout` means
        // `SetWindowPos` on the populated App combo -- the measured
        // data-loss call (`Ui::shown_external`). A caption is never measured
        // by `layout`, so there is no second path back in.
        //
        // **`&` is DOUBLED here, and only here.** `IDC_GRP_EDITOR` is a
        // plain caption `STATIC` since the review fix on Task 8 (was
        // `BS_GROUPBOX`, a BUTTON -- see the creation comment in
        // `build_children`), and a `STATIC` reads a lone `&` as a mnemonic
        // prefix the same way a `BUTTON` caption does, unless `SS_NOPREFIX`
        // is given: it is not drawn, and the letter after it gets an
        // underline that steals a key. The two static captions
        // (`cap::EDITOR_NONE` / `EDITOR_UNNAMED`) need no escape because
        // they simply contain no `&` -- see the note on them. This third
        // caption is the only one in the window fed from the CATALOG, and
        // Start Menu names really do carry ampersands:
        // `SS_NOPREFIX_STYLE`'s comment names `Notes & To Do` and
        // `Arts & Crafts` for exactly this reason. Unescaped, the first
        // draws as `Editing "Notes  To Do"` with **T** underlined --
        // colliding with the `Ctrl` hold chip -- and the second underlines
        // **C**, colliding with `Close`. `SS_NOPREFIX` would also fix this,
        // and is now available where it was not before the reclass -- but
        // switching to it is a different change from the reclass this
        // comment documents, and doubling already works, so it stays.
        //
        // **Not `shown()`**: that helper does the INVERSE (it strips markers
        // so `layout` measures ink, not `&`), and running it here would drop
        // the ampersand instead of drawing it. `set_text_if_changed` stays
        // correct because it compares the same escaped string it writes.
        let editor_caption = match &st.detail {
            None => cap::EDITOR_NONE.to_string(),
            Some(d) if d.app.trim().is_empty() => cap::EDITOR_UNNAMED.to_string(),
            Some(d) => format!("Editing \"{}\"", d.app.trim().replace('&', "&&")),
        };
        set_text_if_changed(hwnd, IDC_GRP_EDITOR, &editor_caption);

        // The count beside the heading. A TEXT write like the caption above,
        // and safe for the same reason: `layout` measures `IDC_LBL_SECTION`
        // from the constant `"Shortcuts"` and gives this control the leftover,
        // so its own text is never a layout input and can never reach
        // `SetWindowPos` on the App combo.
        //
        // Counts what the LIST shows -- `st.items` is already filtered -- so
        // under a filter it describes the rows on screen rather than the
        // file. Empty rather than `· 0 bindings` when there is nothing:
        // the list says that better than a number does, and B's mock-up puts
        // a count next to a populated list.
        let count = st.items.len();
        set_text_if_changed(
            hwnd,
            IDC_LBL_COUNT,
            &match count {
                0 => String::new(),
                1 => "\u{b7} 1 binding".to_string(),
                n => format!("\u{b7} {n} bindings"),
            },
        );

        // The editor strip's two commands. `Record` stays live while a
        // capture is armed even if the row went away underneath it: it reads
        // `Stop` then, and it is the only way to end a recording with the
        // mouse -- the hook is swallowing every keystroke, so there is no
        // keyboard route to fall back on.
        //
        // `Reset` is greyed while armed for the same reason the five typed
        // controls are: it writes the value the hook is in the middle of
        // recording.
        let row = st.detail.is_some();
        enable(hwnd, IDC_RECORD, capturing || (st.editable && row));
        enable(hwnd, IDC_RESET, st.editable && row && !capturing);
        // Guarded by a read, like every other write in this function.
        set_text_if_changed(
            hwnd,
            IDC_RECORD,
            if capturing { cap::STOP } else { cap::RECORD },
        );

        // Conditional, like every other field write here: an unconditional
        // WM_SETTEXT raises EN_CHANGE on every push, which for this control
        // would mean fighting the user's own typing on every keystroke. It
        // is written at all only so `Add` can clear it.
        if text_of(filter) != st.filter {
            set_text(filter, &st.filter);
        }

        enable(hwnd, IDC_APPLY, st.apply_enabled);
        enable(hwnd, IDC_REMOVE, st.remove_enabled);
        // The rest of what can change the file. `apply_enabled` and
        // `remove_enabled` already carry `editable` inside them (both are
        // false in a state with no model); these four have no other input,
        // so they read the flag directly.
        //
        // The list is disabled rather than merely empty: its tick boxes
        // mutate, and a control that cannot be operated says "read only" in
        // a way an empty control does not.
        //
        // The filter belongs here too, not beside `IDC_COMBO`/`IDC_APP`
        // above: it is not gated on `st.detail` (there is no row to filter
        // without a model either way), it is gated on the same `editable`
        // flag as Add/List/Caps. Left un-greyed, it stayed clickable in the
        // read-only state that `unreadable_state` produces, and every
        // keystroke there was silently erased on the next `apply_state`
        // pass -- `on_filter` reaches a `None` model and changes nothing,
        // so `st.filter` stays `""` and the conditional write above puts it
        // back.
        enable(hwnd, IDC_ADD, st.editable);
        enable(hwnd, IDC_FILTER, st.editable);
        enable(hwnd, IDC_LIST, st.editable);
        enable(hwnd, IDC_CAPS, st.editable);
        // These four `check` calls need no `suppressed()` guard, unlike every
        // text write above. `IDC_CAPS` is a real check box and `BM_SETCHECK`
        // sets its state without raising `BN_CLICKED`; the three `Hold` chips
        // are owner-draw and `check` writes a `Cell` for them, which raises
        // nothing whatsoever. Either way a push cannot feed itself back as a
        // user click.
        //
        // The three chips are written unconditionally FROM THE MODEL, and
        // that is what makes `Model::set_caps_hold` refusing an empty chord
        // safe to ignore up here: unticking the last chip leaves the model
        // holding the previous value, so this push re-ticks the box the user
        // just cleared. The chord always has a modifier in it, on screen and
        // in the file alike.
        check(hwnd, IDC_CAPS, st.caps_checked);
        check(hwnd, IDC_HOLD_CTRL, st.caps_hold.ctrl);
        check(hwnd, IDC_HOLD_WIN, st.caps_hold.super_);
        check(hwnd, IDC_HOLD_ALT, st.caps_hold.alt);
        // By INDEX. Even a DROPDOWNLIST has typeahead, which moves the
        // selection, so reading or writing this control by TEXT would make
        // the model follow whatever the user's last keystroke selected.
        //
        // Guarded by a read, like every other field write in this function:
        // an unconditional `CB_SETCURSEL` is a write on every keystroke, and
        // a control that is asked to change is a control that may answer.
        let want = match st.caps_tap {
            CapsTap::CapsLock => 0usize,
            CapsTap::Escape => 1,
            CapsTap::None => 2,
        };
        if cur_sel(tap) != Some(want) {
            SendMessageW(tap, CB_SETCURSEL, Some(WPARAM(want)), Some(LPARAM(0)));
        }
        // What Caps stands for only means anything when Caps is on -- the
        // two static words included, or the row would read as half greyed.
        for id in [
            IDC_LBL_HOLD,
            IDC_HOLD_CTRL,
            IDC_HOLD_WIN,
            IDC_HOLD_ALT,
            IDC_LBL_TAP,
            IDC_TAP,
        ] {
            enable(hwnd, id, st.editable && st.caps_checked);
        }

        // `banner_shown`, not `external_change` on its own: the file moving
        // is a window-wide fact, but the announcement is drawn on the page it
        // is about. The warn dot on the Shortcuts pill is how it stays
        // visible from the other three, and `external_change` is untouched --
        // it still says the file moved whichever door is open.
        //
        // One function, four readers: this, `layout`'s card 0,
        // `compute_card_rects`, and core's `DefaultButton::visible` for the
        // two buttons. A ring left on a `Reload` this line has hidden is the
        // measured defect `default_button` exists for.
        let page = PAGE.with(|p| p.get());
        let banner_on = banner_shown(external_change, page);
        show(banner, banner_on);
        show(reload, banner_on);
        show(keep, banner_on);
        // Geometry only, and ONLY when the geometry can have changed.
        //
        // `layout` re-places every control, the App COMBOBOX included, and a
        // populated combo box rewrites its own edit field when it is resized
        // -- so running this on every keystroke threw the user's typing away
        // and put a catalogue entry on screen instead. See
        // `Ui::shown_external`. `WM_SIZE` and `WM_DPICHANGED` still call
        // `layout` directly; this is the data path, where nothing moves
        // unless the banner appears or disappears -- or the list gains its
        // first row or loses its last, which changes the row height `layout`
        // measures. See `Ui::shown_empty`. `sync_list` has already run, so
        // `st.items` is what the control holds.
        //
        // These three do not cover `layout`'s list-client-width input; that
        // omission is deliberate and is argued at the column sizing inside
        // `layout`.
        //
        // **THREE terms since the tab strip, not two.** `layout` places only
        // the current page's controls and gives every other page's card a
        // zero-height rect, so the page is an input like the banner and the
        // empty list are -- see `Ui::shown_page`. `show_page` lays out on its
        // own and records the page it laid out for, so the ordinary switch
        // arrives here already satisfied and is not laid out twice; what this
        // term catches is a switch racing a push.
        let list_empty = st.items.is_empty();
        let relayout = UI.with(|u| {
            u.borrow()
                .as_ref()
                .map(|x| {
                    x.shown_external != Some(external_change)
                        || x.shown_empty != Some(list_empty)
                        || x.shown_page != Some(page)
                })
                .unwrap_or(true)
        });
        if relayout {
            layout(hwnd);
            // The banner appearing/disappearing, or the list gaining its
            // first row / losing its last, shifts every card below it --
            // 56 px for the banner alone (its 48 px card plus the
            // `gap_card` below it). `sync_list`'s own
            // `InvalidateRect` (below) targets the LIST control, not
            // `hwnd`, so without this the stack's old position stays
            // painted behind its new one: cards slide but their old fills
            // and 1 px borders do not go away. No `UI` borrow is held
            // here -- the borrow that produced `relayout` above already
            // ended on the line that computed it, the same discipline
            // `layout` itself follows.
            let _ = InvalidateRect(Some(hwnd), None, true);
        }
        // LAST, after every `enable` and every `show` above: this is what
        // makes it the authoritative moment rather than one more place that
        // has to be kept in step. See `repair_default_button`.
        repair_default_button(hwnd, st, external_change);
    }

    // Nothing is sent from here on, so this borrow is safe to hold while it
    // records what the control now shows.
    UI.with(|u| {
        if let Some(ui) = u.borrow_mut().as_mut() {
            ui.suppress = false;
            ui.items = st.items.clone();
            // Recorded AFTER the write, not instead of it, so a caption that
            // never made it to the screen is retried on the next push.
            ui.shown_dirty = Some(st.dirty);
            // Recorded after the layout above, for the same reason
            // `shown_dirty` is recorded after the caption write.
            ui.shown_external = Some(external_change);
            ui.shown_empty = Some(st.items.is_empty());
            // Read again rather than carried down from the block above: it
            // is a `Cell`, so a fresh read costs nothing, and the value
            // cannot have moved in between -- `show_page` is the only writer
            // and it runs on this thread, from a `WM_COMMAND` this function
            // is not inside.
            ui.shown_page = Some(PAGE.with(|p| p.get()));
            // What the five shortcut controls now show, in model terms --
            // see `Ui::shown_combo` and `commit_fields`.
            ui.shown_combo = st.detail.as_ref().map(|d| d.combo.clone());
            // Any read of the App field posted before this push is now about
            // text WE wrote, not text the user typed. See `Ui::app_epoch`.
            if wrote_app {
                ui.app_epoch = ui.app_epoch.wrapping_add(1);
            }
        }
    });
}

/// The column texts for one row, in `LIST_COLUMNS` order.
///
/// Both the rebuild and the diff go through here, so they cannot disagree
/// about what a cell says -- and the column set is one edit, in one place,
/// when it changes.
fn cells(it: &ListItem) -> Vec<String> {
    vec![app_cell(it), combo_cell(it)]
}

/// The Shortcut column's text: the chord as a keyboard spells it.
///
/// **`ListItem::combo` is unchanged** -- that is the config string, and
/// `Model` writes it back to the file. This is the display of it, and the
/// two must not be conflated: `beckon_core::shortcuts` keeps `combo_display`
/// separate from `Combo::canonical` for exactly this reason (spec §B.4).
///
/// Falls back to the raw string when the chord does not parse, so a row
/// whose stored text is not a valid combo still shows what is actually in
/// the file rather than an empty cell -- `Model::problems` is what says why.
///
/// **Real text, not a placeholder for a later custom draw.** Spec §B.5: the
/// keycaps land *over* text that is really there, which is what keeps
/// `LVM_GETITEMTEXT` working for `examples/settings_probe.rs` and keeps a
/// screen reader announcing what the screen shows.
fn combo_cell(it: &ListItem) -> String {
    let d = combo_display(&it.combo);
    if d.is_empty() {
        it.combo.clone()
    } else {
        d
    }
}

/// The App column's text, from `beckon_core::settings::app_cell`.
///
/// **The joining rule lives in core, with its inverse.** `list_custom_draw`
/// has to take this string back apart to colour the flag, and a painter that
/// split on its own idea of the separator would be a second description of
/// the same fact -- see `split_app_cell`, which is tested against this on all
/// three CI jobs rather than on the one that can run a ListView.
///
/// **The flag no longer takes the list's Body font by necessity.** That was
/// true while this text was drawn by comctl32, which has no per-run font in a
/// report view; subitem 0 is custom-drawn now (G3 measured that the tick
/// survives `CDRF_SKIPDEFAULT`), so the flag gets Caption and a colour.
fn app_cell(it: &ListItem) -> String {
    beckon_core::settings::app_cell(&it.app, it.flag.as_deref())
}

/// Push `st.items` into the ListView, rebuilding only when it has to.
///
/// **The row count is the whole discriminator.** Every text edit leaves it
/// unchanged, so every text edit takes the diff, where nothing is deleted
/// and therefore neither the scroll position nor a tick nor the highlight
/// can be disturbed. Only Add, Remove and a reload change it, and only they
/// pay for a rebuild. That is what keeps this trivial: no keyed
/// reconciliation and no ids in `LVITEM.lParam`.
///
/// The control's own count is consulted as well as the cache, so a list
/// emptied by anything other than `apply_state` rebuilds rather than being
/// written past the end.
unsafe fn sync_list(list: HWND, prev: &[ListItem], st: &ControlState) {
    let live = SendMessageW(list, LVM_GETITEMCOUNT, Some(WPARAM(0)), Some(LPARAM(0))).0;
    if prev.len() != st.items.len() || live != st.items.len() as isize {
        rebuild_list(list, st);
        return;
    }
    for (i, it) in st.items.iter().enumerate() {
        let now = cells(it);
        let was = cells(&prev[i]);
        for (sub, text) in now.iter().enumerate() {
            if was.get(sub) != Some(text) {
                set_item_text(list, i, sub as i32, text);
            }
        }
        set_item_state(list, i, it.marked, st.selected == Some(i));
    }
}

/// Delete and reinsert every row. Only for a changed row count.
unsafe fn rebuild_list(list: HWND, st: &ControlState) {
    // Read the scroll position while it still means something.
    let top = SendMessageW(list, LVM_GETTOPINDEX, Some(WPARAM(0)), Some(LPARAM(0))).0;
    let per = SendMessageW(list, LVM_GETCOUNTPERPAGE, Some(WPARAM(0)), Some(LPARAM(0))).0;

    SendMessageW(list, WM_SETREDRAW, Some(WPARAM(0)), Some(LPARAM(0)));
    SendMessageW(list, LVM_DELETEALLITEMS, Some(WPARAM(0)), Some(LPARAM(0)));
    for (i, it) in st.items.iter().enumerate() {
        let texts = cells(it);
        let mut first = wide(&texts[0]);
        // The state goes in with the insert, not after it: an item that is
        // inserted without LVIF_STATE has no state image, and the
        // LVN_ITEMCHANGED comctl32 fires when it paints the first empty box
        // looks exactly like the user clicking a tick off.
        //
        // LVIS_FOCUSED is deliberately absent. Setting it scrolls the item
        // into view, which would fight the scroll restore below. Consequence:
        // after any Add / Remove / reload, the first arrow key press jumps
        // to row 0 instead of continuing from the current selection.
        let item = LVITEMW {
            mask: LVIF_TEXT | LVIF_STATE,
            iItem: i as i32,
            iSubItem: 0,
            pszText: windows::core::PWSTR(first.as_mut_ptr()),
            stateMask: LIST_VIEW_ITEM_STATE_FLAGS(LVIS_STATEIMAGEMASK.0 | LVIS_SELECTED.0),
            state: LIST_VIEW_ITEM_STATE_FLAGS(
                check_bits(it.marked) | selected_bits(st.selected == Some(i)),
            ),
            ..Default::default()
        };
        SendMessageW(
            list,
            LVM_INSERTITEMW,
            Some(WPARAM(0)),
            Some(LPARAM(&item as *const _ as isize)),
        );
        for (sub, text) in texts.iter().enumerate().skip(1) {
            set_item_text(list, i, sub as i32, text);
        }
    }

    SendMessageW(list, WM_SETREDRAW, Some(WPARAM(1)), Some(LPARAM(0)));

    // A rebuild leaves the view at the top, so a lone ENSUREVISIBLE(top)
    // does nothing at all -- `top` is already on screen. Ensuring the
    // BOTTOM of the page that used to be showing is what scrolls; ensuring
    // `top` afterwards stops it overshooting by a row.
    //
    // After WM_SETREDRAW TRUE on purpose, so the scroll is not asked of a
    // control that has been told not to draw. It costs no flicker: lifting
    // the block does not paint, it only marks the control dirty, and
    // nothing reaches the screen until the WM_PAINT that follows this
    // whole refresh.
    let count = st.items.len() as isize;
    if count > 0 && top > 0 {
        let top = top.min(count - 1);
        let bottom = (top + per.max(1) - 1).min(count - 1);
        ensure_visible(list, bottom);
        ensure_visible(list, top);
    }

    // The pair above restores the pre-rebuild scroll position, which is
    // right for a reload (`Model::from_text` leaves `st.selected` as
    // `None`, so this block does nothing there) but wrong for
    // `Model::add_row`, which always selects the newly appended last row:
    // on a list longer than one page, "restore the old top" leaves that
    // new, selected, empty row off-screen while the editor strip below is
    // already showing it -- and when the old top was 0 the pair above
    // skips entirely, so the row stays off-screen with nothing to fix it.
    // `LVM_ENSUREVISIBLE` is a no-op when the row is already fully on
    // screen, so this only moves the view when the restore above left the
    // selection outside it -- it never fights the restore for the reload
    // case.
    if let Some(sel) = st.selected {
        if (sel as isize) < count {
            ensure_visible(list, sel as isize);
        }
    }

    let _ = InvalidateRect(Some(list), None, false);
}

unsafe fn ensure_visible(list: HWND, i: isize) {
    SendMessageW(
        list,
        LVM_ENSUREVISIBLE,
        Some(WPARAM(i as usize)),
        // fPartialOK = FALSE: the row must be fully on screen, or the pair
        // above can land half a row short.
        Some(LPARAM(0)),
    );
}

unsafe fn set_item_text(list: HWND, i: usize, sub: i32, text: &str) {
    let mut t = wide(text);
    let it = LVITEMW {
        iSubItem: sub,
        pszText: windows::core::PWSTR(t.as_mut_ptr()),
        ..Default::default()
    };
    SendMessageW(
        list,
        LVM_SETITEMTEXTW,
        Some(WPARAM(i)),
        Some(LPARAM(&it as *const _ as isize)),
    );
}

fn check_bits(on: bool) -> u32 {
    if on {
        LVIS_CHECKED
    } else {
        LVIS_UNCHECKED
    }
}

fn selected_bits(on: bool) -> u32 {
    if on {
        LVIS_SELECTED.0
    } else {
        0
    }
}

/// Set a row's tick and highlight, but only when they are not already
/// right. Reading first keeps the diff from firing an `LVN_ITEMCHANGED`
/// per row per keystroke, which the suppression guard would swallow but
/// which comctl32 still has to raise.
unsafe fn set_item_state(list: HWND, i: usize, marked: bool, selected: bool) {
    let mask = LVIS_STATEIMAGEMASK.0 | LVIS_SELECTED.0;
    let want = check_bits(marked) | selected_bits(selected);
    let cur = SendMessageW(
        list,
        LVM_GETITEMSTATE,
        Some(WPARAM(i)),
        Some(LPARAM(mask as isize)),
    )
    .0 as u32
        & mask;
    if cur == want {
        return;
    }
    let it = LVITEMW {
        state: LIST_VIEW_ITEM_STATE_FLAGS(want),
        stateMask: LIST_VIEW_ITEM_STATE_FLAGS(mask),
        ..Default::default()
    };
    SendMessageW(
        list,
        LVM_SETITEMSTATE,
        Some(WPARAM(i)),
        Some(LPARAM(&it as *const _ as isize)),
    );
}

/// Show `key` in the key list -- BY INDEX, and only when it is not already
/// what the control holds.
///
/// **By index, because the index IS the contract.** `ComboView::key` is a
/// position in `key_table()`, `build_children` fills the list from that same
/// slice in order and without `CBS_SORT`, so the two integers are the same
/// integer. Writing this control by TEXT would throw that away, and reading
/// it by text would follow the user's typeahead: even a `CBS_DROPDOWNLIST`
/// searches its list as you type, which moves the selection.
///
/// **Guarded by a read**, like every other control write in `apply_state`.
/// An unconditional `CB_SETCURSEL` is a write on every push, and a control
/// asked to change is a control that may answer -- `CB_SETCURSEL` does not
/// itself raise `CBN_SELCHANGE`, but that is comctl32's promise rather than
/// this file's, and the guard costs one message.
///
/// `None` is written as -1, which is what clears a selection. A row that has
/// never been given a shortcut, and one whose stored text does not parse,
/// must both show nothing rather than the first key in the table.
unsafe fn set_key_sel(combo: HWND, key: Option<usize>) {
    let want = key.map(|i| i as i32).unwrap_or(-1);
    if cur_sel_raw(combo) != want {
        SendMessageW(
            combo,
            CB_SETCURSEL,
            // `as isize as usize`, not `as usize`: sign-extended, so a
            // 64-bit combo box is handed the C idiom's `(WPARAM)-1` rather
            // than 0x00000000FFFFFFFF.
            Some(WPARAM(want as isize as usize)),
            Some(LPARAM(0)),
        );
    }
}

/// Push a value into a chip or a check box.
///
/// Which of the two is `chip_bit`'s business. Both halves raise **nothing**:
/// `BM_SETCHECK` sets a check box's state without a `BN_CLICKED`, and
/// `set_chip` writes a `Cell` and marks a rectangle dirty, which is not a
/// notification at all. That is what lets every `check` call in `apply_state`
/// and on the capture path skip the `suppressed()` guard every text write
/// there carries.
unsafe fn check(parent: HWND, id: i32, on: bool) {
    if let Some(bit) = chip_bit(id) {
        set_chip(parent, id, bit, on);
        return;
    }
    if let Ok(h) = GetDlgItem(Some(parent), id) {
        SendMessageW(
            h,
            BM_SETCHECK,
            Some(WPARAM(
                if on { BST_CHECKED.0 } else { BST_UNCHECKED.0 } as usize
            )),
            Some(LPARAM(0)),
        );
    }
}

/// Flip a chip and repaint it, the way `BS_AUTOCHECKBOX` used to flip itself.
///
/// **Before the handler runs, never after.** Both chip handlers read all of
/// their chips back out of `is_checked` rather than out of the notification
/// -- the `Hold` chord because a setter taking one flag at a time could not
/// refuse "none ticked" without knowing the other two, the four modifiers
/// because `push_shortcut` spells the whole combo. That read has to see the
/// state the user now sees, which is exactly the property `BS_AUTOCHECKBOX`
/// gave away for free and `BS_OWNERDRAW` does not.
fn toggle_chip(hwnd: HWND, id: i32) {
    if let Some(bit) = chip_bit(id) {
        set_chip(hwnd, id, bit, !chip_armed(bit));
    }
}

/// Does this notification code mean the user pressed a chip?
///
/// **`BN_DOUBLECLICKED` is not noise here, it is the second click.**
/// `BS_OWNERDRAW` sends that code automatically -- `BS_NOTIFY` is neither
/// needed nor set on these seven -- and the button sends it INSTEAD of a
/// second `BN_CLICKED`, not alongside one. A handler that ignored it would
/// toggle once for two clicks, where a real check box toggles twice.
///
/// Narrow rather than `(id, _)` for the reason the key list's arm is narrow:
/// a control that says more than one thing must not have all of it read as
/// an edit.
fn is_chip_click(code: u32) -> bool {
    code == BN_CLICKED || code == BN_DOUBLECLICKED
}

/// Hand the scanned catalog to the window, from the worker thread.
///
/// The `Vec` is leaked into the message and reclaimed by the `WM_CATALOG`
/// arm of `wndproc`. If the post fails — the window closed while the scan
/// was running — this reclaims it here instead, so the failure costs
/// nothing but the scan.
pub fn post_catalog(target: WindowHandle, names: Vec<String>) {
    let boxed = Box::into_raw(Box::new(names));
    let posted = unsafe {
        PostMessageW(
            Some(target.0),
            WM_CATALOG,
            WPARAM(0),
            LPARAM(boxed as isize),
        )
    };
    if posted.is_err() {
        drop(unsafe { Box::from_raw(boxed) });
    }
}

// ---------------------------------------------------------------------------
// Message handling
// ---------------------------------------------------------------------------

fn with_cb(f: impl FnOnce(&mut Callbacks)) {
    // Take-then-run, matching `hotkey.rs`: a handler that pumps (open_path's
    // ShellExecuteW, a MessageBox) can re-enter this window's wndproc, and a
    // second borrow of the same RefCell would panic across an
    // `extern "system"` boundary, which aborts the process rather than
    // unwinding.
    let taken = CB.with(|c| c.borrow_mut().take());
    if let Some(mut cb) = taken {
        f(&mut cb);
        CB.with(|c| {
            if c.borrow().is_none() {
                *c.borrow_mut() = Some(cb);
            }
        });
    }
}

fn suppressed() -> bool {
    UI.with(|u| u.borrow().as_ref().map(|x| x.suppress).unwrap_or(true))
}

thread_local! {
    /// Is a high-contrast theme active? See `high_contrast`.
    static HIGH_CONTRAST: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Is a high-contrast theme active?
///
/// **Cached, never asked per paint.** `SystemParametersInfoW` is a round trip
/// into the system's own settings, and the Shortcut column asks this once per
/// visible row. Refreshed exactly where the answer can change: at creation,
/// and on the `SPI_SETHIGHCONTRAST` arm that already forwards the message to
/// every child.
///
/// Only the *shape* of a keycap consults it. Every colour comes from `col`,
/// which already answers correctly for high contrast on its own -- what is
/// not correct there is a rounded box with a soft bottom edge, which reads as
/// a rendering artefact against a theme built on flat fills and hard borders.
fn high_contrast() -> bool {
    HIGH_CONTRAST.with(|c| c.get())
}

thread_local! {
    /// The Caption `HFONT` the Shortcut column draws its keycaps in, as a raw
    /// handle.
    ///
    /// **A `Cell`, and deliberately not read from `UI`.** Custom draw runs
    /// inside a paint, and a paint reaches this window while `UI` is already
    /// borrowed -- measured on a14: every subitem-1 notification exited at
    /// `try_borrow` and the column silently drew as text. A `Cell` cannot be
    /// contended, so the paint path never depends on who else is mid-borrow.
    /// Refreshed wherever `build_fonts` runs, which is creation and
    /// `WM_DPICHANGED`.
    static CAP_FONT: std::cell::Cell<isize> = const { std::cell::Cell::new(0) };
}

fn set_cap_font(f: HFONT) {
    CAP_FONT.with(|c| c.set(f.0 as isize));
}

fn cap_font() -> Option<HFONT> {
    let v = CAP_FONT.with(|c| c.get());
    if v == 0 {
        None
    } else {
        Some(HFONT(v as *mut core::ffi::c_void))
    }
}

thread_local! {
    /// Mirrors `Ui::theme`, apart from it for `CAP_FONT`'s own reason: a
    /// paint reaches this window while `UI` is already borrowed, and `col` /
    /// `brush` are exactly the calls a paint makes. `RefCell`, not a `Cell`
    /// like `CAP_FONT` -- `ThemeCache::brush` needs `&mut self` to grow its
    /// cache, and a `Cell` can hand out a copy but never a borrow. Refreshed
    /// at both points `Ui::theme.rebuild` runs: creation and
    /// `on_theme_changed`.
    static PAINT_THEME: RefCell<theme::ThemeCache> = RefCell::new(theme::ThemeCache::default());
}

/// A colour, resolved through the paint-safe mirror. See `PAINT_THEME` for
/// why painting code reaches this instead of `UI`.
fn theme_col(pick: impl Fn(&beckon_core::theme::Palette) -> u32, sys: SYS_COLOR_INDEX) -> COLORREF {
    PAINT_THEME.with(|cache| cache.borrow().col(pick, sys))
}

/// A cached brush from the same mirror. Never a system brush -- see the
/// `GetSysColorBrush` ban documented at the top of `paint.rs`.
fn theme_brush(c: COLORREF) -> HBRUSH {
    PAINT_THEME.with(|cache| cache.borrow_mut().brush(c))
}

/// One subitem's text, read from the control rather than from the model.
///
/// The paint path must not touch `UI` (see `CAP_FONT`), and it does not need
/// to: the cell already holds the display spelling, which is exactly what
/// `combo_display` produced when the row was pushed. Reading it back keeps
/// the caps and the accessible name the same string by construction rather
/// than by two code paths agreeing.
unsafe fn subitem_text(list: HWND, item: usize, subitem: i32) -> String {
    let mut buf = [0u16; 256];
    let it = LVITEMW {
        iSubItem: subitem,
        pszText: windows::core::PWSTR(buf.as_mut_ptr()),
        cchTextMax: buf.len() as i32,
        ..Default::default()
    };
    let n = SendMessageW(
        list,
        LVM_GETITEMTEXTW,
        Some(WPARAM(item)),
        Some(LPARAM(&it as *const LVITEMW as isize)),
    );
    let n = n.0.max(0) as usize;
    String::from_utf16_lossy(&buf[..n.min(buf.len())])
}

/// Which tier `button` (`paint.rs`) should paint a push button as.
///
/// Every id but `IDC_RECORD` is static. `IDC_RECORD` alone is read back from
/// its OWN caption -- `Danger` while it reads `cap::STOP` (armed), `Outline`
/// otherwise -- rather than from a second flag next to it: this file must
/// not touch `UI` from a paint path (`CAP_FONT`'s reason, a paint can arrive
/// while it is borrowed), and the caption `set_text_if_changed` last wrote
/// already says which the button currently is. Reading it back is also what
/// keeps the two in agreement by construction: a caption and a tier stored
/// in two places can drift, one read from one place cannot.
fn tier_of(id: i32, hwnd_item: HWND) -> BtnTier {
    match id {
        IDC_APPLY => BtnTier::Accent,
        IDC_RESET => BtnTier::Outline,
        IDC_RECORD if text_of(hwnd_item) == cap::STOP => BtnTier::Danger,
        IDC_RECORD => BtnTier::Outline,
        _ => BtnTier::Secondary,
    }
}

/// Paint any of the nine `PUSH_BUTTONS`, `Save` included, by translating the
/// `NMCUSTOMDRAW` comctl32 hands this window into the `DRAWITEMSTRUCT`
/// `paint::button` actually draws from.
///
/// **`NM_CUSTOMDRAW`, NOT `BS_OWNERDRAW`, for all nine.** See the call
/// site's own comment and `button`'s doc for why: every one of these nine
/// can carry the default ring, and `BS_OWNERDRAW` would take the machinery
/// that moves it along for the ride.
unsafe fn push_button_custom_draw(hwnd: HWND, p: *const NMCUSTOMDRAW) -> isize {
    let cd = &*p;
    if cd.dwDrawStage != CDDS_PREPAINT {
        return CDRF_DODEFAULT as isize;
    }
    let btn = cd.hdr.hwndFrom;
    let id = cd.hdr.idFrom as i32;
    let tier = tier_of(id, btn);
    let dpi = GetDpiForWindow(hwnd).max(96);
    let mut state = ODS_FLAGS(0);
    if cd.uItemState.0 & CDIS_DISABLED.0 != 0 {
        state.0 |= ODS_DISABLED.0;
    }
    if cd.uItemState.0 & CDIS_SELECTED.0 != 0 {
        state.0 |= ODS_SELECTED.0;
    }
    if cd.uItemState.0 & CDIS_FOCUS.0 != 0 {
        state.0 |= ODS_FOCUS.0;
    }
    // `ODS_HOTLIGHT` is the one bit a REAL `WM_DRAWITEM` never carries for a
    // classic push button -- Windows does not hover-track an owner-draw
    // button on its own, which is exactly why `button` staying reachable
    // from `NM_CUSTOMDRAW` (via this translation) rather than becoming
    // genuinely owner-draw is worth more than tidiness: true hover feedback,
    // for free, on all nine. Using it as the carrier here is safe for that
    // same reason -- nothing else can ever set it on a `DRAWITEMSTRUCT`
    // `button` receives.
    if cd.uItemState.0 & CDIS_HOT.0 != 0 {
        state.0 |= ODS_HOTLIGHT.0;
    }
    let di = DRAWITEMSTRUCT {
        CtlType: ODT_BUTTON,
        CtlID: id as u32,
        itemState: state,
        hwndItem: btn,
        hDC: cd.hdc,
        rcItem: cd.rc,
        ..Default::default()
    };
    PAINT_THEME.with(|c| button(&di, tier, &mut c.borrow_mut(), dpi));
    CDRF_SKIPDEFAULT as isize
}

/// Paint `IDC_CAPS` -- the one toggle switch in this window -- by reading
/// the three bits `paint::toggle` needs off the `NMCUSTOMDRAW` comctl32
/// hands this window, the same shape `push_button_custom_draw` uses one
/// function up for the nine `PUSH_BUTTONS`.
///
/// **`NM_CUSTOMDRAW`, NOT `BS_OWNERDRAW`.** `IDC_CAPS` stays
/// `BS_AUTOCHECKBOX` -- see its creation call and `paint::toggle`'s own doc
/// for why: owner-draw is a different VALUE of the same 4-bit type field,
/// not a flag beside it, and adopting it would throw away the check box
/// state machine and the UIA role a screen reader announces.
///
/// `on` is read with `is_checked`, not off a bit this notification carries
/// -- a check box's `NMCUSTOMDRAW` has no state bit for "ticked", only
/// `CDIS_DISABLED` / `CDIS_FOCUS` / `CDIS_SELECTED` / `CDIS_HOT`, none of
/// which mean checked. `is_checked` already routes `IDC_CAPS` to
/// `BM_GETCHECK` (see `chip_bit`'s own doc: `IDC_CAPS` is deliberately
/// absent from the chip table), so this asks the control the same way
/// `handle_command`'s `(IDC_CAPS, _)` arm already does.
unsafe fn caps_custom_draw(hwnd: HWND, p: *const NMCUSTOMDRAW) -> isize {
    let cd = &*p;
    if cd.dwDrawStage != CDDS_PREPAINT {
        return CDRF_DODEFAULT as isize;
    }
    let dpi = GetDpiForWindow(hwnd).max(96);
    let on = is_checked(hwnd, IDC_CAPS);
    let enabled = cd.uItemState.0 & CDIS_DISABLED.0 == 0;
    let focused = cd.uItemState.0 & CDIS_FOCUS.0 != 0;
    PAINT_THEME.with(|c| toggle(cd, on, enabled, focused, &mut c.borrow_mut(), dpi));
    CDRF_SKIPDEFAULT as isize
}

unsafe fn refresh_high_contrast() {
    let mut hc = HIGHCONTRASTW {
        cbSize: std::mem::size_of::<HIGHCONTRASTW>() as u32,
        ..Default::default()
    };
    let ok = SystemParametersInfoW(
        SPI_GETHIGHCONTRAST,
        std::mem::size_of::<HIGHCONTRASTW>() as u32,
        Some(&mut hc as *mut HIGHCONTRASTW as *mut std::ffi::c_void),
        SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
    )
    .is_ok();
    HIGH_CONTRAST.with(|c| c.set(ok && hc.dwFlags.0 & HCF_HIGHCONTRASTON.0 != 0));
}

/// The App combo box's handle, fetched under a borrow that returns a `Copy`
/// value and drops with its closure.
///
/// One function rather than the same `UI.with` written out at each of the
/// three sites that needs it: the handle is only ever wanted immediately
/// before a send, and a borrow left alive across one of those aborts the
/// process. Keeping the fetch in one place keeps that property checkable.
fn app_handle() -> Option<HWND> {
    UI.with(|u| u.borrow().as_ref().map(|x| x.app))
}

/// Ask for the App combo box's text to be read from the message loop rather
/// than from inside the notification that reported it changed. See
/// `WM_APP_EDITED` for why, and `Ui::app_epoch` for what the stamp is for.
///
/// **No `UI` borrow survives the first statement.** `PostMessageW` does not
/// re-enter this wndproc, but the discipline is the file's, not the call's:
/// the borrow reads and writes a `u32` and drops with its closure.
fn post_app_read(hwnd: HWND) {
    let Some(stamp) = UI.with(|u| {
        u.borrow_mut().as_mut().map(|ui| {
            ui.app_epoch = ui.app_epoch.wrapping_add(1);
            ui.app_epoch
        })
    }) else {
        return;
    };
    unsafe {
        let _ = PostMessageW(Some(hwnd), WM_APP_EDITED, WPARAM(stamp as usize), LPARAM(0));
    }
}

/// Shared tail for `WM_SYSCOLORCHANGE`, `WM_THEMECHANGED`, and
/// `WM_SETTINGCHANGE`(`SPI_SETHIGHCONTRAST`): forward the message verbatim
/// to every child, then invalidate and relayout.
///
/// The system delivers all three to TOP-LEVEL windows only. A themed common
/// control (the ListView, the group box) needs `WM_THEMECHANGED` itself to
/// reopen its theme handle, and none of them see it unless we pass it on --
/// so without this forwarding every control keeps rendering from stale
/// theme data after a theme switch or a high-contrast toggle, which is
/// exactly the path this window uses as its dark mode.
///
/// Same enumeration `WM_DPICHANGED` uses to rebroadcast `WM_SETFONT` -- one
/// funnel for "walk every child", not a second one invented here. Only
/// direct children: every control in this window is a sibling of `hwnd`,
/// same as the font rebroadcast relies on. Never sent to `hwnd` itself --
/// that would recurse into this wndproc.
///
/// No `UI` borrow is held across any of these sends: `GetWindow` /
/// `SendMessageW` / `InvalidateRect` don't touch the struct, and `layout`
/// takes and drops its own borrow before any of ITS sends (see the comment
/// at its top) -- the same discipline `WM_DPICHANGED` follows.
unsafe fn broadcast_theme_change(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) {
    let mut child = GetWindow(hwnd, GW_CHILD).unwrap_or_default();
    while !child.is_invalid() {
        SendMessageW(child, msg, Some(wp), Some(lp));
        child = GetWindow(child, GW_HWNDNEXT).unwrap_or_default();
    }
    let _ = InvalidateRect(Some(hwnd), None, true);
    // Relayout, not just repaint: high contrast and theme switches can
    // change the system metrics `layout` reads live -- ListView row height
    // (`list_row_height`, the same value WM_DPICHANGED's font swap already
    // invalidates), SM_CXVSCROLL, and control heights read back through
    // GetWindowRect. `SM_CYBORDER` used to be one of these too, but
    // `layout.rs`'s `compute_card_rects` stopped reading it once `WS_BORDER`
    // came off the list (Task 10). Those are exactly the metrics that move
    // when a user enters or leaves high contrast, and layout already
    // queries them at call time instead of assuming a constant -- staying
    // stale here would reintroduce the clipping bug that query was added to
    // fix. Rare, user-initiated events; a handful of extra SetWindowPos
    // calls is not a cost worth avoiding for it.
    layout(hwnd);
}

/// Does `WM_SETTINGCHANGE`'s `lParam` name the immersive colour set?
///
/// `WM_SETTINGCHANGE` fires for dozens of settings that are not the palette
/// (mouse speed, wallpaper, ...); the light/dark toggle in
/// Settings > Personalization > Colors is reported by a `lParam` string
/// naming `"ImmersiveColorSet"`, and is the only reliable signal for it --
/// unlike the `SPI_SETHIGHCONTRAST` case a few lines below, this change sets
/// no `wParam` action code at all.
fn is_immersive_colour_set(lp: LPARAM) -> bool {
    if lp.0 == 0 {
        return false;
    }
    let p = PCWSTR(lp.0 as *const u16);
    unsafe {
        p.to_string()
            .map(|s| s == "ImmersiveColorSet")
            .unwrap_or(false)
    }
}

/// Re-resolve the theme, rebuild `ThemeCache` if it changed, and repaint.
///
/// **Never calls `layout`.** No colour change moves a control, and `layout`
/// means `SetWindowPos` on the populated App combo -- the measured data-loss
/// path documented at `Ui::shown_external`.
///
/// **The `UI` borrow is taken and dropped on one expression.**
/// `InvalidateRect` re-enters this wndproc, and a second `RefCell` borrow
/// across an `extern "system"` boundary aborts the process instead of
/// unwinding -- the same rule `WM_DPICHANGED` and `WM_DESTROY` already
/// follow.
///
/// `PAINT_THEME` is rebuilt unconditionally, even when `ui.theme` did not
/// move (or there is no `Ui` at all, mid-teardown) -- `ThemeCache::rebuild`
/// is a no-op on a repeat theme, so this is never wasted work, and it is
/// what stops the paint-safe mirror from answering for a theme `Ui` already
/// left behind.
unsafe fn on_theme_changed(hwnd: HWND) {
    let t = beckon_core::theme::resolve(theme::read_inputs());
    let changed = UI.with(|u| {
        u.borrow_mut()
            .as_mut()
            .map(|ui| ui.theme.rebuild(t))
            .unwrap_or(false)
    });
    PAINT_THEME.with(|c| {
        c.borrow_mut().rebuild(t);
    });
    // The backdrop tier depends on `EnableTransparency` and the high-contrast
    // flag, NOT on `Theme` — toggling Transparency in Settings > Personalization >
    // Colors broadcasts "ImmersiveColorSet" without changing Theme, so gating this
    // on `changed` would leave the window stuck at its old tier. The DWM and
    // SetWindowLong calls underneath are idempotent, so running it every time
    // costs nothing.
    apply_current_backdrop(hwnd);
    if !changed {
        return;
    }
    theme::apply_dwm_dark(hwnd, t == beckon_core::theme::Theme::Dark);
    // DWM's 1 px border around the window, tinted to the window's own ground
    // -- same call and same reason as in `create`, which carries the note on
    // what this comment used to claim and why it was wrong.
    theme::apply_dwm_border(hwnd, t);
    theme_list(hwnd, t == beckon_core::theme::Theme::Dark);
    let _ = InvalidateRect(Some(hwnd), None, true);
}

/// Resolve the current backdrop tier and apply it.
///
/// The single call site `theme::read_backdrop_inputs` has -- both `create`
/// (the window's first paint) and `on_theme_changed` (every later
/// re-evaluation) go through this function rather than calling
/// `read_backdrop_inputs` themselves, so `theme::MICA_SUPPORTED` is the one
/// flag Gate 01 has to flip on a hardware failure. See its doc comment.
fn apply_current_backdrop(hwnd: HWND) {
    let inputs = theme::read_backdrop_inputs(theme::MICA_SUPPORTED);
    theme::apply_backdrop(hwnd, beckon_core::theme::backdrop(inputs));
}

/// Bring the ListView's own background in line with the theme (Task 10).
///
/// **`LVM_SETBKCOLOR` / `LVM_SETTEXTBKCOLOR` / `LVM_SETTEXTCOLOR`, all
/// three.** Rows are custom-drawn, but comctl32 paints the ground BELOW the
/// last row -- and, for subitem 0 (the App column), the row's own background
/// and text too, since `list_custom_draw` deliberately leaves that subitem
/// to comctl32's default draw so the check box survives
/// (`CDRF_NOTIFYPOSTPAINT`, see its own comment). Both messages default to
/// `COLOR_WINDOW`/black until told otherwise, which is why a dark-mode list
/// used to show themed rows sitting on a light ground with black text on it.
///
/// **`SetWindowTheme` rides along.** A public exported function, NOT one of
/// the uxtheme ordinals the 2026-08-11 spec rejected -- but the theme class
/// name is undocumented and the call degrades silently on builds that do not
/// know it, which is why nothing downstream depends on it having worked. It
/// is also what enables the dark scrollbar AND native hot-item tracking
/// (`LVM_GETHOTITEM`), which the Shortcut column's own hover tint
/// (`list_custom_draw`) reads.
///
/// Called at creation (`create`, once the theme is resolved) and again on
/// every theme change (`on_theme_changed`) -- the same two moments
/// `theme::apply_dwm_dark` already runs at, and for the same reason: nothing
/// else tells this control its colours moved.
unsafe fn theme_list(hwnd: HWND, dark: bool) {
    let Ok(list) = GetDlgItem(Some(hwnd), IDC_LIST) else {
        return;
    };
    let bg = theme_col(|p| p.card, COLOR_WINDOW);
    let text = theme_col(|p| p.text, COLOR_WINDOWTEXT);
    SendMessageW(
        list,
        LVM_SETBKCOLOR,
        Some(WPARAM(0)),
        Some(LPARAM(bg.0 as isize)),
    );
    SendMessageW(
        list,
        LVM_SETTEXTBKCOLOR,
        Some(WPARAM(0)),
        Some(LPARAM(bg.0 as isize)),
    );
    SendMessageW(
        list,
        LVM_SETTEXTCOLOR,
        Some(WPARAM(0)),
        Some(LPARAM(text.0 as isize)),
    );
    let name = if dark {
        w!("DarkMode_Explorer")
    } else {
        w!("Explorer")
    };
    let _ = SetWindowTheme(list, name, None);

    // **The header and the four fields need their own theme class, and
    // nothing else reaches them.** Measured on a14 2026-08-13: with only the
    // work above, the shipped dark window had a BRIGHT WHITE header band
    // across it and three white combo faces, because those parts are painted
    // by the visual style rather than by anything this window controls.
    //
    // `WM_CTLCOLOR*` cannot fix either: a `CBS_DROPDOWNLIST` has no edit
    // child to answer for, and its closed face comes from the theme, not from
    // `WM_DRAWITEM` -- which is why the `CBS_OWNERDRAWFIXED` added for the
    // drop-down ITEMS left the closed control untouched.
    //
    // **MEASURED INEFFECTIVE on a14 2026-08-13. These calls change nothing
    // today, and the reason is a constraint this window cannot satisfy.**
    // Screenshot after adding them is pixel-identical in the header and all
    // four fields: still white.
    //
    // `DarkMode_*` theme classes are inert until the PROCESS opts into dark
    // mode through uxtheme's undocumented ordinals (`SetPreferredAppMode` #135
    // / `AllowDarkModeForWindow` #133), and the 2026-08-11 spec rejected
    // uxtheme ordinals outright. So the class name is accepted, silently does
    // nothing, and the visual style keeps painting these parts light.
    //
    // Kept rather than deleted because the calls are harmless, they are
    // already correct for the day the ordinal question is reopened, and
    // deleting them would delete the measurement with them. Nothing depends
    // on them having worked -- see the caller's own note. The scrollbar call
    // above is in the same position: it may or may not be doing anything.
    //
    // What this leaves: in dark mode the ListView header and the three combo
    // faces render light. Fixing it needs one of — owner-drawing the header
    // (possible, and the NM_CUSTOMDRAW path meant to do it is not firing),
    // replacing the combos with owner-drawn controls, or reopening the
    // ordinals decision. That is a design call, not a code one.
    let header = header_of(list);
    if !header.is_invalid() {
        let hname = if dark {
            w!("DarkMode_ItemsView")
        } else {
            w!("ItemsView")
        };
        let _ = SetWindowTheme(header, hname, None);
    }
    // `CFD` is the combo/edit family. The filter box is a plain EDIT and takes
    // the same class.
    let fname = if dark { w!("DarkMode_CFD") } else { w!("CFD") };
    for id in [IDC_APP, IDC_COMBO, IDC_TAP, IDC_FILTER] {
        if let Ok(c) = GetDlgItem(Some(hwnd), id) {
            let _ = SetWindowTheme(c, fname, None);
        }
    }
}

/// Repaint just the title bar band, not the whole client -- a hover move is
/// the only thing that changed, and `chrome::paint` fills the whole band
/// itself, so `erase: false` skips a redundant `WM_ERASEBKGND` pass (the
/// same reasoning `set_chip` gives for an owner-draw button).
unsafe fn invalidate_titlebar(hwnd: HWND) {
    let dpi = GetDpiForWindow(hwnd).max(96);
    let mut rc = RECT::default();
    if GetClientRect(hwnd, &mut rc).is_ok() {
        rc.bottom = rc.bottom.min(scale(chrome::TITLEBAR_H, dpi));
        let _ = InvalidateRect(Some(hwnd), Some(&rc), false);
    }
}

extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_CREATE => {
                // Font BEFORE geometry, and the order is load-bearing:
                // `build_children` leaves every control carrying its role's
                // font, and `layout` asks comctl32 for the ListView's row
                // height -- which comctl32 derives from that font. Placing
                // first would size the list against whatever the control
                // was born with.
                build_children(hwnd);
                layout(hwnd);
                LRESULT(0)
            }
            DM_GETDEFID_MSG => {
                // What makes Enter press a button.
                //
                // `IsDialogMessageW` asks the window which control is the
                // default, and this window is not a dialog box -- so without
                // this arm the question reaches `DefWindowProcW`, which does
                // not answer it, and the dialog manager falls back to IDOK
                // (1). Nothing here has that id, so Enter did nothing at all
                // while `BS_DEFPUSHBUTTON` drew the ring that promises it.
                //
                // The answer is MAKELONG(id, DC_HASDEFID): the id in the low
                // word, the "yes, I have one" magic in the high word.
                // Returning the id alone reads as `DC_HASDEFID == 0`, i.e.
                // no default at all.
                //
                // `Ui::defid`, NOT a constant. The dialog manager reaches
                // this message even when a push button has focus, because
                // Enter only activates the focused control directly when it
                // answers `WM_GETDLGCODE` with `DLGC_DEFPUSHBUTTON` -- so a
                // constant here made Enter on Close save, and Enter on the
                // banner's Reload save OVER the external change it was
                // warning about. The borrow is taken and dropped inside the
                // closure; nothing is sent from this arm.
                //
                // A disabled default is handled by the dialog manager, which
                // does not send a command to a disabled control -- and the
                // `IDC_APPLY` arm of `handle_command` checks anyway, because
                // `TranslateAcceleratorW` has no such scruple about Ctrl+S.
                let id = UI
                    .with(|u| u.borrow().as_ref().map(|ui| ui.defid))
                    .unwrap_or(IDC_APPLY);
                LRESULT(((DC_HASDEFID_FLAG << 16) | id as u32) as isize)
            }
            DM_SETDEFID_MSG => {
                // The other half. Nothing in beckon sends this today --
                // `handle_command` calls `set_default_id` directly -- but the
                // dialog manager may, and a window that answers
                // `DM_GETDEFID` while ignoring `DM_SETDEFID` is a window
                // whose ring and whose Enter key can be driven apart from
                // outside. Both routes converge on one function, so they
                // cannot disagree.
                set_default_id(hwnd, (wp.0 & 0xFFFF) as i32);
                LRESULT(1)
            }
            WM_GETMINMAXINFO => {
                // A frame promise, not an arithmetic one -- Step 2 clamps
                // independently, because a floor does not make subtraction
                // safe, it only makes it unlikely.
                let dpi = GetDpiForWindow(hwnd).max(96);
                let mm = &mut *(lp.0 as *mut MINMAXINFO);
                mm.ptMinTrackSize.x = scale(MIN_WIDTH, dpi);
                mm.ptMinTrackSize.y = scale(MIN_HEIGHT, dpi);
                LRESULT(0)
            }
            WM_SIZE => {
                layout(hwnd);
                // Card 3 is bottom-anchored and cards 1/2 flex with the list
                // (Task 8), so a resize moves and resizes every card's rect,
                // not just the children `layout` repositions.
                // `SetWindowPos` on a child only invalidates what THAT child
                // vacated -- strictly inside its own card -- so without this
                // the `CARD_PAD` ring, the inter-card gaps and whatever a
                // shrunk card used to cover are left painted with the OLD
                // geometry: `DefWindowProc`'s own erase only knows the new
                // client size, not where the cards used to be. The
                // `WNDCLASSEXW` above deliberately carries no
                // `CS_HREDRAW`/`CS_VREDRAW` -- see its own comment -- so
                // there is nothing upstream of this call that would do it
                // instead.
                let _ = InvalidateRect(Some(hwnd), None, true);
                LRESULT(0)
            }
            WM_DPICHANGED => {
                // HIWORD(wParam) is the new DPI; lParam is a RECT with the
                // position and size Windows wants. Ignoring lParam leaves
                // the window the wrong size on the new monitor, and no
                // second message arrives to correct it.
                let dpi = ((wp.0 >> 16) & 0xFFFF) as u32;
                let fonts = build_fonts(hwnd, dpi);
                set_cap_font(fonts.get(Role::Keycap));
                // The borrow is taken and dropped on these lines. Nothing
                // below may hold one: `WM_SETFONT` re-enters this wndproc,
                // and a second `RefCell` borrow across an `extern "system"`
                // boundary ABORTS the process rather than unwinding.
                let old = UI.with(|u| {
                    u.borrow_mut().as_mut().map(|ui| {
                        let prev = ui.fonts;
                        ui.fonts = fonts;
                        prev
                    })
                });
                // Every child must be told, including ones `layout` places
                // through GetDlgItem rather than a stored handle -- and
                // each must be told about ITS OWN role, read back from the
                // same `role_of` the creation path used. A single font
                // broadcast here would flatten the ramp on the first walk
                // between monitors.
                let mut child = GetWindow(hwnd, GW_CHILD).unwrap_or_default();
                while !child.is_invalid() {
                    let f = fonts.for_id(GetDlgCtrlID(child));
                    SendMessageW(
                        child,
                        WM_SETFONT,
                        Some(WPARAM(f.0 as usize)),
                        Some(LPARAM(1)),
                    );
                    child = GetWindow(child, GW_HWNDNEXT).unwrap_or_default();
                }
                // The Header is `list`'s child, not `hwnd`'s, so the walk
                // above never reaches it -- same reason `build_children`
                // sets it separately rather than through `role_of`.
                if let Ok(list) = GetDlgItem(Some(hwnd), IDC_LIST) {
                    set_header_font(list, fonts.get(Role::BodyStrong));
                    // The state image list is sized in DEVICE pixels
                    // (Task 10), so a monitor move needs a fresh one at the
                    // new DPI for the same reason the font does. Known
                    // limitation: the composited GLYPH itself is copied from
                    // whichever image list is CURRENTLY installed, which
                    // after the first call is our own previous composite,
                    // not comctl32's native default -- so only the CELL
                    // rescales cleanly across a live DPI change; the tick's
                    // own pixels stay at their first-installed resolution.
                    // Unverified on hardware; see `rebuild_state_image_list`.
                    rebuild_state_image_list(list, dpi);
                }
                // AFTER the broadcast, never before: the old handles were
                // selected into those controls until the loop above replaced
                // them, and deleting a font that is still selected is
                // undefined.
                //
                // If `UI` is somehow absent, `fonts` was never stored above,
                // so free THAT instead of leaking three -- practically
                // unreachable (`UI` is populated in WM_CREATE before any
                // other message can arrive), but cheap to close.
                old.unwrap_or(fonts).delete();
                // Font before geometry: the controls already carry the new
                // fonts by the time `SetWindowPos` (which raises WM_SIZE)
                // and the explicit `layout` below run, so the ListView's
                // row height is queried at the size it will actually draw.
                //
                // No column-width loop here any more. Widths used to be
                // fixed per-DPI constants that only this arm refreshed;
                // they are now a proportion of the live list width and
                // `layout`, called at the bottom of this arm, is the one
                // place that sets them.
                let rc = &*(lp.0 as *const RECT);
                let _ = SetWindowPos(
                    hwnd,
                    None,
                    rc.left,
                    rc.top,
                    rc.right - rc.left,
                    rc.bottom - rc.top,
                    SWP_NOZORDER | SWP_NOACTIVATE,
                );
                layout(hwnd);
                // Same reason as `WM_SIZE`: the card stack's geometry moved
                // (here the fonts changed size too, which can itself move
                // `card2_h`/`card1_h` through `notes_height`/`list_h`), so
                // the previous paint is stale everywhere, not only inside
                // the children `layout` repositioned. See the `WNDCLASSEXW`
                // comment above for why the class style cannot do this
                // instead.
                let _ = InvalidateRect(Some(hwnd), None, true);
                LRESULT(0)
            }
            // -- The client-drawn title bar (Task 7) ------------------------
            WM_NCCALCSIZE => chrome::nccalcsize(hwnd, wp, lp),
            WM_NCHITTEST => {
                // LOWORD/HIWORD of lParam: screen coordinates, signed --
                // negative on a monitor left of or above the primary.
                let pt = POINT {
                    x: (lp.0 & 0xFFFF) as u16 as i16 as i32,
                    y: ((lp.0 >> 16) & 0xFFFF) as u16 as i16 as i32,
                };
                match chrome::nchittest(hwnd, pt) {
                    Some(lr) => lr,
                    None => DefWindowProcW(hwnd, msg, wp, lp),
                }
            }
            // With no maximize box there is nothing for a caption
            // double-click to do, and letting DefWindowProc try it is how
            // the unreachable maximized state gets reached.
            WM_NCLBUTTONDBLCLK if wp.0 as u32 == HTCAPTION => LRESULT(0),
            WM_NCMOUSEMOVE => {
                // wParam is already the hit-test code from OUR OWN
                // WM_NCHITTEST at this position -- Windows computes it
                // before sending this message -- so there is no second
                // geometry calculation here to keep in step with
                // `chrome::nchittest` / `chrome::hit_button`.
                let code = wp.0 as u32;
                let want = (code == HTCLOSE || code == HTMINBUTTON).then_some(code as i32);
                let moved = UI.with(|u| {
                    u.borrow_mut()
                        .as_mut()
                        .map(|ui| {
                            let changed = ui.hot != want;
                            ui.hot = want;
                            changed
                        })
                        .unwrap_or(false)
                });
                if moved {
                    invalidate_titlebar(hwnd);
                }
                // WM_NCMOUSELEAVE does not fire on its own: TrackMouseEvent's
                // tracking is one-shot per arrival, so every move re-arms
                // it. Cheap -- one syscall -- and idempotent while the
                // cursor stays inside the bar.
                let mut tme = TRACKMOUSEEVENT {
                    cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                    dwFlags: TME_LEAVE | TME_NONCLIENT,
                    hwndTrack: hwnd,
                    dwHoverTime: 0,
                };
                let _ = TrackMouseEvent(&mut tme);
                DefWindowProcW(hwnd, msg, wp, lp)
            }
            WM_NCMOUSELEAVE => {
                let had = UI.with(|u| {
                    u.borrow_mut()
                        .as_mut()
                        .map(|ui| ui.hot.take().is_some())
                        .unwrap_or(false)
                });
                if had {
                    invalidate_titlebar(hwnd);
                }
                DefWindowProcW(hwnd, msg, wp, lp)
            }
            WM_ERASEBKGND => {
                // Owned by this window since Task 13, not `DefWindowProcW`
                // and not the class's own `hbrBackground` (`create`, a plain
                // system colour with no notion of the light/dark palette or
                // of a backdrop tier at all).
                //
                // Under Mica, painting ANYTHING here -- even the theme's own
                // background colour -- is the one thing that hides it: DWM
                // has already composited the backdrop into this exact rect
                // before `WM_PAINT` runs, and an opaque fill on top of it
                // covers the material just as completely as an unrelated
                // colour would. Returning 1 with nothing drawn is what keeps
                // it visible in the gaps between cards; `WM_PAINT`'s own
                // card loop still draws the four cards on top either way.
                if theme::current_tier() == beckon_core::theme::Backdrop::Mica {
                    LRESULT(1)
                } else {
                    let hdc = HDC(wp.0 as *mut core::ffi::c_void);
                    let mut rc = RECT::default();
                    let _ = GetClientRect(hwnd, &mut rc);
                    FillRect(hdc, &rc, theme_brush(theme_col(|p| p.bg, COLOR_BTNFACE)));
                    LRESULT(1)
                }
            }
            WM_PAINT => {
                let mut ps = PAINTSTRUCT::default();
                let hdc = BeginPaint(hwnd, &mut ps);
                // The four cards, painted first: they are this window's own
                // background layer, and every child control paints itself
                // separately afterward and lands on top regardless of the
                // order drawing happens in here. `card_rects` runs the SAME
                // arithmetic `layout` places controls against -- see its own
                // comment for why that must stay one function -- and takes
                // its own `UI` borrow, dropped before it returns, so calling
                // it here (before the `PAINT_THEME` borrow below) cannot
                // collide with anything.
                //
                // `card` reads the theme through `theme_col` / `theme_brush`,
                // each of which takes and drops its own `PAINT_THEME` borrow
                // -- never `Ui::theme` -- so this loop must run OUTSIDE the
                // `PAINT_THEME.with` block below: nesting it inside would be
                // a second borrow of the same `RefCell` while the first
                // (`chrome::paint`'s) is still alive, which panics.
                for rc in card_rects(hwnd) {
                    // The banner's rect is zero height when the banner is
                    // hidden -- `RoundRect` on a degenerate rect is nothing
                    // worth asking GDI to draw.
                    if rc.bottom > rc.top {
                        let dpi = GetDpiForWindow(hwnd).max(96);
                        card(hdc, rc, dpi);
                    }
                }
                // ONE borrow, taken and dropped on this line -- `chrome::paint`
                // below must not run with `UI` still borrowed, on the same
                // rule every other arm in this function follows.
                let ui_bits = UI.with(|u| {
                    u.borrow()
                        .as_ref()
                        .map(|ui| (ui.fonts, ui.hot, ui.app, ui.filter))
                });
                if let Some((fonts, hot, app, filter)) = ui_bits {
                    let dpi = GetDpiForWindow(hwnd).max(96);
                    // `PAINT_THEME`'s borrow is passed straight into
                    // `chrome::paint` (and, added in Task 9, `field_border`)
                    // rather than read through `theme_col` / `theme_brush`,
                    // which would try to borrow this same `RefCell` a second
                    // time and panic.
                    PAINT_THEME.with(|c| {
                        let mut cache = c.borrow_mut();
                        chrome::paint(hwnd, hdc, &mut cache, &fonts, dpi, hot);
                        // `GetFocus()` read fresh here rather than cached
                        // anywhere: this is the only place that needs the
                        // answer, and it is what lets `handle_command`'s
                        // `CBN_SETFOCUS`/`EN_SETFOCUS` arms be pure
                        // "ask for a repaint" triggers with no state of
                        // their own to keep in step with this.
                        let focus = GetFocus();
                        field_border(hdc, app, hwnd, &mut cache, focus == app, dpi);
                        field_border(hdc, filter, hwnd, &mut cache, focus == filter, dpi);
                    });
                }
                let _ = EndPaint(hwnd, &ps);
                LRESULT(0)
            }
            WM_SYSCOLORCHANGE => {
                // System palette changed (e.g. entering/leaving high
                // contrast). This window's own cached colours -- `ThemeCache`
                // and its paint-safe mirror `PAINT_THEME` -- are refreshed by
                // `on_theme_changed`, reached separately: `WM_THEMECHANGED`
                // fires for the same high-contrast transition, and
                // `WM_SETTINGCHANGE`'s `ImmersiveColorSet` check catches the
                // light/dark toggle. Nothing here rebuilds them a second
                // time; the forward+invalidate below is what makes the
                // CHILDREN's own cached colours (edit control backgrounds,
                // ListView text/back colour) catch up.
                broadcast_theme_change(hwnd, msg, wp, lp);
                LRESULT(0)
            }
            WM_THEMECHANGED => {
                // Visual style changed. Themed common controls (the
                // ListView) open their theme handle once and keep it until
                // told otherwise; WM_THEMECHANGED is that notice, and it
                // only reaches top-level windows, hence the forward.
                broadcast_theme_change(hwnd, msg, wp, lp);
                // High contrast on/off arrives here too (Windows treats it
                // as a visual-style switch), so this is also a live signal
                // for the light/dark/high-contrast `ThemeCache`, not just
                // the keycap-shape flag `HIGH_CONTRAST` above.
                on_theme_changed(hwnd);
                LRESULT(0)
            }
            WM_SETTINGCHANGE => {
                // WM_SETTINGCHANGE fires for dozens of unrelated SPI_
                // actions (wallpaper, mouse trails, ...) -- wParam carries
                // the SPI_ action code when SystemParametersInfo was called
                // with SPIF_SENDCHANGE, which is how Windows reports a
                // high-contrast toggle. Only that one is the keycap shape's
                // concern.
                //
                // The light/dark palette additionally watches lParam for
                // `"ImmersiveColorSet"` (`is_immersive_colour_set`), which is
                // how Windows reports the Settings > Personalization > Colors
                // toggle -- a change that sets neither `SPI_SETHIGHCONTRAST`
                // nor `WM_THEMECHANGED`. Everything else must fall through to
                // DefWindowProcW untouched rather than relayout on every
                // unrelated settings change.
                if wp.0 == SPI_SETHIGHCONTRAST.0 as usize {
                    // Before the broadcast, so the relayout and every repaint
                    // it triggers already see the new answer.
                    refresh_high_contrast();
                    broadcast_theme_change(hwnd, msg, wp, lp);
                    on_theme_changed(hwnd);
                    LRESULT(0)
                } else if is_immersive_colour_set(lp) {
                    on_theme_changed(hwnd);
                    DefWindowProcW(hwnd, msg, wp, lp)
                } else {
                    DefWindowProcW(hwnd, msg, wp, lp)
                }
            }
            WM_APP_EDITED => {
                // The deferred half of the App combo box's edit handling.
                // Whatever the control was doing to its own text when it
                // told us it had changed, it has finished doing by now, so
                // the edit holds what the user is looking at.
                //
                // Two ways to be stale, and neither is an error:
                //
                // - a newer keystroke has already asked for a read, so this
                //   one would only produce the same answer one push earlier;
                // - `apply_state` wrote the field between the post and here,
                //   so what is in it is ours and not the user's.
                //
                // Both are the same test, because both bump the stamp. The
                // `suppress` test is separate and belt-and-braces: no posted
                // message can be dispatched inside `apply_state` (nothing it
                // calls pumps), but a future edit there must not be able to
                // make that untrue silently.
                //
                // The borrow is taken and dropped inside each closure;
                // `text_of` sends `WM_GETTEXTLENGTH`/`WM_GETTEXT` and
                // `with_cb` runs caller code that can pump, so neither may
                // run with one held.
                let fresh = UI.with(|u| {
                    u.borrow()
                        .as_ref()
                        .map(|ui| !ui.suppress && ui.app_epoch == wp.0 as u32)
                        .unwrap_or(false)
                });
                if fresh {
                    if let Some(app) = app_handle() {
                        let t = text_of(app);
                        with_cb(|cb| (cb.on_edit_app)(t));
                    }
                }
                LRESULT(0)
            }
            WM_CAPTURE => {
                on_capture(hwnd, wp.0);
                LRESULT(0)
            }
            WM_TIMER if wp.0 == IDT_CAPTURE => {
                // The watchdog. Spec F.2: `is_installed()` can lie, because
                // past `LowLevelHooksTimeout` Windows removes the hook
                // silently and there is no API to ask -- so the only
                // evidence a capture is still alive is that it keeps
                // arriving, and this is what happens when it stops.
                //
                // It is also the backstop for every case in spec F.5's table
                // that ends "the watchdog fires": an elevated window taking
                // focus under UIPI, a secure desktop, another remapper
                // sitting ahead of us in the chain.
                end_capture(hwnd);
                LRESULT(0)
            }
            WM_KILLFOCUS => {
                // The first of spec F.4's three focus layers. It fires
                // rarely -- focus normally lives on a CHILD, and a child's
                // `WM_KILLFOCUS` goes to the child -- but the window itself
                // does hold focus after `repair_default_button`'s
                // `SetFocus(hwnd)` fallback, and this is what covers that.
                // The layer that carries the ordinary case is `WM_ACTIVATE`
                // below.
                end_capture(hwnd);
                DefWindowProcW(hwnd, msg, wp, lp)
            }
            WM_ACTIVATE => {
                // The second layer, and the one that actually fires when the
                // user clicks another window. LOWORD(wParam) is the
                // activation state.
                if (wp.0 & 0xFFFF) as u32 == WA_INACTIVE {
                    end_capture(hwnd);
                }
                DefWindowProcW(hwnd, msg, wp, lp)
            }
            WM_ACTIVATEAPP => {
                // The third: this process as a whole lost the foreground.
                // Not redundant with `WM_ACTIVATE` -- that one speaks for
                // this window against its siblings, and beckon has more than
                // one (the tray window, and whatever COM creates on this
                // thread).
                //
                // The fourth layer is not here and cannot be: the per-event
                // `GetForegroundWindow()` gate inside the hook itself, which
                // is the only one that fires when a UAC prompt or an
                // elevated window takes the foreground without sending
                // either of these.
                if wp.0 == 0 {
                    end_capture(hwnd);
                }
                DefWindowProcW(hwnd, msg, wp, lp)
            }
            WM_CATALOG => {
                // Reclaims what `post_catalog` leaked into the message.
                let names = *Box::from_raw(lp.0 as *mut Vec<String>);
                // The caller stores it and calls back into `apply_state`,
                // which is what actually fills the combo box -- one path
                // for putting things on screen, not two.
                with_cb(|cb| (cb.on_catalog)(names));
                LRESULT(0)
            }
            WM_NOTIFY => {
                // Every WM_COMMAND arm asks this; this one did not, and that
                // becomes fatal the moment `apply_state` writes item state.
                // `LVM_SETITEMSTATE` makes comctl32 fire LVN_ITEMCHANGED
                // SYNCHRONOUSLY, inside `apply_state` -- so the chain
                // apply_state -> on_select -> refresh_settings -> apply_state
                // recurses without bound across an `extern "system"`
                // boundary, where a second RefCell borrow ABORTS the process
                // instead of unwinding. Landing 2a writes item state for the
                // first time, so this guard has to exist before any of it.
                let nm = &*(lp.0 as *const NMHDR);
                // Custom draw is answered BEFORE the `suppressed()` guard, and
                // deliberately: it is pure painting, it reaches no callback and
                // it cannot recurse into `apply_state`, so the guard's reason
                // does not apply to it. Falling through while suppressed would
                // paint the raw display string for that frame -- legible,
                // because the cell really does hold it, but a visible flicker
                // between two spellings of the same chord.
                if nm.idFrom == IDC_LIST as usize && nm.code == NM_CUSTOMDRAW {
                    return LRESULT(list_custom_draw(hwnd, lp.0 as *const NMLVCUSTOMDRAW));
                }
                // The ListView's own Header (Task 10): a child of `IDC_LIST`,
                // never of `hwnd` -- `set_header_font`'s own reason -- so its
                // `WM_NOTIFY`s carry `hwndFrom` equal to the HEADER's own
                // HWND rather than `IDC_LIST`'s, and `idFrom` cannot tell the
                // two custom-draw sources apart the way it does above.
                // Answered here, before `suppressed()`, for the same reason
                // the list's own custom draw is: pure painting, no callback,
                // cannot recurse into `apply_state`.
                if nm.code == NM_CUSTOMDRAW && nm.hwndFrom == header_of(hwnd) {
                    return LRESULT(header_custom_draw(hwnd, lp.0 as *const NMCUSTOMDRAW));
                }
                // Every push button paints through here now, `Save` included
                // -- Task 9 widened this from `Save` alone to all nine of
                // `PUSH_BUTTONS`.
                //
                // **`NM_CUSTOMDRAW`, NOT `BS_OWNERDRAW`, for ALL of them.**
                // `BS_OWNERDRAW` replaces a button's TYPE, and every one of
                // these nine can carry `BS_DEFPUSHBUTTON` -- the ring
                // `set_default_id` moves around with a `BM_SETSTYLE`
                // read-modify-write through `BS_TYPEMASK_BITS` -- not `Save`
                // alone. Owner-draw would take that machinery with it, and
                // Enter-on-`Reload`-saves is a defect this window has already
                // had once. Custom draw leaves the type, the notifications
                // and the ring exactly as they are and only replaces the
                // pixels. `push_button_custom_draw` (`mod.rs`) translates the
                // `NMCUSTOMDRAW` this message carries into the
                // `DRAWITEMSTRUCT` `paint::button` actually draws from, so
                // there is one painter behind both `WM_NOTIFY` here and
                // `WM_DRAWITEM` below.
                if is_push_button(nm.idFrom as i32) && nm.code == NM_CUSTOMDRAW {
                    return LRESULT(push_button_custom_draw(hwnd, lp.0 as *const NMCUSTOMDRAW));
                }
                // `IDC_CAPS` (Task 11): the one toggle switch in this
                // window, reached the same way and for the same reason as
                // the nine push buttons just above -- pure painting, no
                // callback, cannot recurse into `apply_state`, so it is
                // answered before `suppressed()` too.
                if nm.idFrom == IDC_CAPS as usize && nm.code == NM_CUSTOMDRAW {
                    return LRESULT(caps_custom_draw(hwnd, lp.0 as *const NMCUSTOMDRAW));
                }
                if suppressed() {
                    return LRESULT(0);
                }
                if nm.idFrom == IDC_LIST as usize && nm.code == LVN_ITEMCHANGED {
                    let lv = &*(lp.0 as *const NMLISTVIEW);
                    // iItem is -1 on the notifications that speak for the
                    // whole list rather than one row; `as usize` would turn
                    // that into an index no model has, and `set_marked`
                    // indexes straight into `rows`.
                    if lv.iItem >= 0 {
                        let i = lv.iItem as usize;
                        // The MODEL row this view row stands for, copied out
                        // and the borrow DROPPED before either callback runs.
                        // Both re-enter `refresh_settings` -> `apply_state`,
                        // which sends messages, and a `UI` borrow still open
                        // across an `extern "system"` boundary ABORTS the
                        // process instead of unwinding. Same discipline as
                        // `layout`'s `LayoutHandles`.
                        //
                        // `get(i)`, not `[i]`: comctl32 can deliver an
                        // LVN_ITEMCHANGED for a row that a just-pushed,
                        // shorter `items` no longer has -- which a filter
                        // makes routine rather than exotic.
                        let row = UI.with(|u| {
                            u.borrow()
                                .as_ref()
                                .and_then(|x| x.items.get(i).map(|it| it.row))
                        });
                        let Some(row) = row else {
                            return LRESULT(0);
                        };
                        // A tick and a selection both arrive as LVIF_STATE
                        // and `uChanged` cannot tell them apart, so the two
                        // bits are tested independently. Never `else if`:
                        // clicking an unselected row's box changes both in
                        // ONE message, and an `else if` drops whichever the
                        // arm did not reach.
                        let changed = lv.uOldState ^ lv.uNewState;
                        if changed & LVIS_STATEIMAGEMASK.0 != 0 {
                            let on = (lv.uNewState & LVIS_STATEIMAGEMASK.0) == LVIS_CHECKED;
                            with_cb(|cb| (cb.on_mark)(row, on));
                        }
                        if changed & LVIS_SELECTED.0 != 0 && lv.uNewState & LVIS_SELECTED.0 != 0 {
                            with_cb(|cb| (cb.on_select)(row));
                        }
                    }
                }
                LRESULT(0)
            }
            WM_CTLCOLORSTATIC => {
                // **Every on-card STATIC, group box and check box, by id.**
                // Before Task 8 this arm answered for `IDC_LBL_COUNT` alone
                // and let `DefWindowProcW` cover the rest, on the strength of
                // a comment that read: "the group boxes and the window share
                // the same `bg` token, and letting the parent's paint show
                // through is what keeps this correct if that ever stops
                // being true." Task 8 is exactly the change that comment
                // named. Every control below now sits on one of the four
                // `card()` fills (`paint.rs`), which is its OWN token,
                // distinct from `bg` in both palettes (light: bg 0xF2F4F8 /
                // card 0xFFFFFF; dark: bg 0x15171C / card 0x1D2027) --
                // `DefWindowProcW`'s opaque `COLOR_3DFACE` brush is neither,
                // so the fall-through punched a visible system-grey
                // rectangle into every one of these: both group-box
                // captions, the four field labels, the notes, the banner and
                // the Caps Lock check box.
                //
                // Both the returned fill brush AND `SetBkColor` are set to
                // the same `card` token, and the mode is `OPAQUE` (the DC
                // default -- named here rather than left implicit), not the
                // `TRANSPARENT` the old single-id arm used: the control's own
                // paint should be correct by itself, not by depending on
                // whatever the card underneath happens to have been left
                // showing -- see the `WM_SIZE`/`WM_DPICHANGED`/`apply_state`
                // repaint fix above for why that dependency used to be
                // riskier than it looked.
                //
                // `theme_brush` returns a brush `PAINT_THEME` owns and frees
                // on its next theme change -- never here. It survives this
                // window's own `WM_DESTROY` on purpose: `PAINT_THEME` is a
                // thread-local, not a field of `Ui`, so the same handle is
                // still good the next time this window opens on an unchanged
                // theme. Never a system brush either: `GetSysColorBrush` is
                // banned from this window's drawing code (see the comment at
                // the top of `paint.rs`).
                //
                // `IDC_LBL_COUNT` alone keeps the dimmer `text_faint` ink
                // Task 6 gave it -- it sits beside a Subtitle heading, not
                // inside a run of body text.
                let ctl = HWND(lp.0 as *mut core::ffi::c_void);
                let id = GetDlgCtrlID(ctl);
                // **`IDC_APP` / `IDC_FILTER`, disabled.** Windows routes a
                // disabled EDIT or COMBOBOX through THIS message, not
                // `WM_CTLCOLOREDIT` -- so without this arm a greyed App
                // combo or filter box fell through to `DefWindowProcW`'s
                // `COLOR_3DFACE`, the exact system-grey rectangle Task 8
                // fixed for the STATICs above, just reached by a different
                // message. `field`/`text_faint` rather than `card`/`text`:
                // these two sit on their OWN `field` surface even when
                // enabled (see `WM_CTLCOLOREDIT` below), and disabled only
                // dims the ink, matching `button`'s own "Disabled" row for
                // every push-button tier.
                if id == IDC_APP || id == IDC_FILTER {
                    let hdc = HDC(wp.0 as *mut core::ffi::c_void);
                    let field = theme_col(|p| p.field, COLOR_BTNFACE);
                    let text = theme_col(|p| p.text_faint, COLOR_GRAYTEXT);
                    SetTextColor(hdc, text);
                    SetBkColor(hdc, field);
                    SetBkMode(hdc, OPAQUE);
                    return LRESULT(theme_brush(field).0 as isize);
                }
                // `IDC_NOTES` is deliberately ABSENT since Task 12: it is
                // `SS_OWNERDRAW` now, and an owner-draw static never asks
                // its parent for a background brush at all -- `draw_chip`'s
                // own controls (the seven toggle chips) are absent from this
                // list for exactly the same reason, and `push_button_custom_draw`'s
                // nine buttons never were in it either. `paint::draw_notes`
                // paints this control's background itself, through
                // `WM_DRAWITEM`.
                let on_card = matches!(
                    id,
                    IDC_LBL_SECTION
                        | IDC_LBL_COUNT
                        | IDC_BANNER
                        | IDC_GRP_EDITOR
                        | IDC_LBL_APP
                        | IDC_LBL_SHORTCUT
                        | IDC_GRP_KEYBOARD
                        | IDC_CAPS
                        | IDC_LBL_HOLD
                        | IDC_LBL_TAP
                );
                if on_card {
                    let hdc = HDC(wp.0 as *mut core::ffi::c_void);
                    // `COLOR_WINDOW`, matching every other `card` fallback
                    // in this window (`paint::card`, and the six other
                    // `theme_col(|p| p.card, ...)` sites) -- `COLOR_BTNFACE`
                    // here was the one place `card` resolved to a DIFFERENT
                    // sys index, and its ink two lines down is
                    // `COLOR_WINDOWTEXT`. Under high contrast that pairs a
                    // BTNFACE fill with WINDOWTEXT ink -- a cross-family
                    // pair latent only because the four shipped HC schemes
                    // happen to make those two indices equal.
                    let card = theme_col(|p| p.card, COLOR_WINDOW);
                    let text = if id == IDC_LBL_COUNT {
                        theme_col(|p| p.text_faint, COLOR_GRAYTEXT)
                    } else {
                        theme_col(|p| p.text, COLOR_WINDOWTEXT)
                    };
                    SetTextColor(hdc, text);
                    SetBkColor(hdc, card);
                    SetBkMode(hdc, OPAQUE);
                    return LRESULT(theme_brush(card).0 as isize);
                }
                DefWindowProcW(hwnd, msg, wp, lp)
            }
            WM_CTLCOLOREDIT | WM_CTLCOLORLISTBOX => {
                // `IDC_APP` (its edit child, via `WM_CTLCOLOREDIT`, AND its
                // drop-down LISTBOX, via `WM_CTLCOLORLISTBOX` -- a combo
                // box's internal list shares its own control id, the same
                // fact `WM_CTLCOLORLISTBOX` handlers everywhere rely on) and
                // `IDC_FILTER` (`WM_CTLCOLOREDIT` only -- a plain EDIT has no
                // drop-down). Neither control is owner-drawn (see each
                // creation comment); this is the ENTIRE extent to which this
                // window colours either one -- comctl32 still draws every
                // character, the caret and the selection itself.
                let ctl = HWND(lp.0 as *mut core::ffi::c_void);
                let id = GetDlgCtrlID(ctl);
                if id == IDC_APP || id == IDC_FILTER {
                    let hdc = HDC(wp.0 as *mut core::ffi::c_void);
                    let field = theme_col(|p| p.field, COLOR_WINDOW);
                    let text = theme_col(|p| p.text, COLOR_WINDOWTEXT);
                    SetTextColor(hdc, text);
                    SetBkColor(hdc, field);
                    SetBkMode(hdc, OPAQUE);
                    return LRESULT(theme_brush(field).0 as isize);
                }
                DefWindowProcW(hwnd, msg, wp, lp)
            }
            WM_DRAWITEM => {
                // Answered BEFORE any `suppressed()` consideration, exactly
                // like the ListView's custom draw one arm up: it is pure
                // painting, it reaches no callback and it cannot recurse
                // into `apply_state`.
                //
                // `DefWindowProcW` on anything else -- and on a menu, whose
                // `CtlID` is 0 and which this window has none of today.
                // Returning 1 for a message we did not draw would leave the
                // control blank.
                let di = &*(lp.0 as *const DRAWITEMSTRUCT);
                if draw_chip(hwnd, di) {
                    return LRESULT(1);
                }
                // `IDC_NOTES`, added in Task 12. `SHOWN_NOTES` rather than a
                // read of `Ui::detail`: this arm is answered BEFORE
                // `suppressed()` below, on `list_custom_draw`'s own rule --
                // pure painting, no callback, cannot recurse into
                // `apply_state` -- and a paint can arrive while `UI` is
                // already borrowed, which is `CHIPS`'s reason too.
                if di.CtlType == ODT_STATIC && di.CtlID as i32 == IDC_NOTES {
                    let dpi = GetDpiForWindow(hwnd).max(96);
                    let body = SHOWN_NOTES.with(|c| c.borrow().clone());
                    PAINT_THEME.with(|c| paint::draw_notes(di, &body, &mut c.borrow_mut(), dpi));
                    return LRESULT(1);
                }
                // `IDC_COMBO` and `IDC_TAP`, added in Task 9: both are
                // `CBS_OWNERDRAWFIXED` `CBS_DROPDOWNLIST`s with no edit
                // child, so unlike `IDC_APP` there is no typing path this can
                // endanger -- see each control's own creation comment.
                if di.CtlType == ODT_COMBOBOX
                    && (di.CtlID as i32 == IDC_COMBO || di.CtlID as i32 == IDC_TAP)
                {
                    let dpi = GetDpiForWindow(hwnd).max(96);
                    PAINT_THEME.with(|c| draw_combo_item(di, &mut c.borrow_mut(), dpi));
                    return LRESULT(1);
                }
                DefWindowProcW(hwnd, msg, wp, lp)
            }
            WM_CHIP_STATE => LRESULT(match chip_bit(wp.0 as i32) {
                Some(bit) if chip_armed(bit) => 2,
                Some(_) => 1,
                None => 0,
            }),
            WM_COMMAND => {
                let id = (wp.0 & 0xFFFF) as i32;
                let code = ((wp.0 >> 16) & 0xFFFF) as u32;
                handle_command(hwnd, id, code);
                LRESULT(0)
            }
            WM_CLOSE => {
                // BEFORE the save prompt, not after. `on_close_request` can
                // put up a `MessageBoxW`, which runs a MODAL LOOP on this
                // thread -- the same thread that dispatches the hook
                // callback. That does not put the dialog's own keyboard at
                // risk: once the box is foreground, `hook_proc`'s per-event
                // `GetForegroundWindow()` gate stops matching `hwnd`, and
                // losing activation fires the `WM_ACTIVATE(WA_INACTIVE)` arm
                // above, which already calls `end_capture`. The reason to
                // call it here too is F.2/F.4's own rule -- every route out
                // of a live capture tears the hook down -- and WM_CLOSE is
                // one more such route: it should not depend on whichever
                // teardown path happens to run first, nor on
                // `on_close_request` continuing to show a modal dialog at
                // all.
                //
                // Cheap when nothing is armed, which is the overwhelmingly
                // common case: `end_capture` is idempotent by construction.
                end_capture(hwnd);
                let mut may = true;
                with_cb(|cb| may = (cb.on_close_request)());
                if may {
                    let _ = DestroyWindow(hwnd);
                }
                LRESULT(0)
            }
            WM_DESTROY => {
                // FIRST, before `UI` is taken: `end_capture` reads it to
                // take the overlay down, and after the take there is no
                // window left for `caps_hook`'s foreground gate to match
                // either. Skipping the drain is deliberate (spec F.3) --
                // there is no window left to protect, and holding the hook
                // one beat longer than the window leaves a swallowed
                // keyboard.
                //
                // Reached on every death this window has, including the ones
                // that do not go through `WM_CLOSE`: a `DestroyWindow` from
                // anywhere, and the system's own teardown of a child of a
                // dying thread.
                end_capture(hwnd);
                // Taken out of the `RefCell` first, so all three
                // `DeleteObject` calls run with no borrow alive -- and so
                // all three run at all. One `HFONT` per window open was
                // already a leak Landing 1 had to close; three roles make
                // it three.
                let ui = UI.with(|u| u.borrow_mut().take());
                if let Some(ui) = ui {
                    ui.fonts.delete();
                    // Same discipline as the fonts, and for the same
                    // reason: an accelerator table is a system resource, and
                    // this window can be opened and closed all day from the
                    // tray. `DestroyAcceleratorTable` on an invalid handle
                    // is not something to rely on, so ask first.
                    if !ui.accel.is_invalid() {
                        let _ = DestroyAcceleratorTable(ui.accel);
                    }
                    // The tooltip window is OWNED by this one, so Windows
                    // destroyed it before this message arrived -- which is
                    // the only reason freeing the buffer it held a POINTER
                    // into is safe. Written out rather than left to `ui`
                    // going out of scope, because "nothing reads this field"
                    // is exactly the impression that gets it deleted.
                    drop(ui.tip_text);
                }
                CB.with(|c| *c.borrow_mut() = None);
                CFG.with(|c| *c.borrow_mut() = None);
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wp, lp),
        }
    }
}

/// Push whatever the editor strip currently shows into the model.
///
/// Separate from the per-keystroke notifications on purpose: those tell us
/// *that* something changed, but a control is free to rewrite its own text
/// afterwards without saying so. This reads the final state.
///
/// It used to say the App combo box "does exactly that" as you type. It does
/// not -- that claim was falsified by `examples/combo_probe.rs` (see
/// `WM_APP_EDITED`), and the rewrite it named was `layout` resizing the
/// control, not the control autocompleting. This is a general backstop for a
/// notification that never arrives, not a workaround for a known offender.
fn commit_fields() {
    // `apply_state`'s `None` arm calls `enable(hwnd, IDC_COMBO, false)` /
    // `enable(hwnd, IDC_APP, false)` whenever `st.detail` is `None`, and
    // `EnableWindow(FALSE)` on a focused control moves focus off it
    // synchronously -- which fires EN_KILLFOCUS / CBN_KILLFOCUS and
    // re-enters this wndproc from inside `apply_state`, before that
    // `apply_state` call has finished writing the new control state.
    // Without this guard, that re-entrant notification would read back
    // whatever the field still shows and feed it into the model as if the
    // user had typed it. `apply_state` sets `ui.suppress = true` before it
    // starts disabling anything, so `suppressed()` catches exactly that
    // window.
    //
    // Before the filter, `st.detail` went `None` only on an explicit
    // deselect (e.g. after the last row is removed), so this path was
    // near-unreachable. Now it also goes `None` whenever the filter hides
    // the selected row (`ControlState::selected` -- see its doc), and the
    // filter matches the Combo/App text itself -- so ordinary edits to the
    // selected row's own Combo or App field can filter that row out of
    // view mid-keystroke, disabling the very field the user is typing in
    // and re-entering right here.
    if suppressed() {
        return;
    }
    let Some((hwnd, combo, app, stored)) = UI.with(|u| {
        u.borrow()
            .as_ref()
            .map(|x| (x.hwnd, x.combo, x.app, x.shown_combo.clone()))
    }) else {
        return;
    };
    // The shortcut half sends NOTHING when no key is selected, exactly as
    // the notification path does -- see `shortcut_shown`. Save must not be
    // the moment a half-finished chord becomes an invalid row.
    //
    // It also sends nothing when the five controls already agree with what
    // is stored -- compared as `ComboView`s, not as the strings
    // `shortcut_shown` and `Ui::shown_combo` hold. `Combo::parse` accepts
    // modifiers in any order, but `Combo::canonical` always rebuilds
    // ctrl -> super -> alt -> shift -> key, so a file written
    // `"super+ctrl+alt+t"` reads back through these controls as
    // `"ctrl+super+alt+t"` on EVERY call here -- an unrelated App-field
    // focus loss, or Save with nothing touched -- even though no control
    // changed. A string compare (`live == stored`) would see those two
    // spellings as different and push the rewrite, and `Model::set_combo`
    // would mark the row dirty for a change that was never made. Comparing
    // `ComboView`s instead makes the reordering invisible, because
    // `combo_view` -- like `Combo::parse` underneath it -- does not care
    // what order the modifiers were written in.
    //
    // Capture the App field text BEFORE the guard: if the combo push
    // re-enters `apply_state`, it may rewrite the App field, so we must read
    // it before any control changes that could trigger that re-entrancy.
    let a = text_of(app);
    if Some(combo_view_of(hwnd, combo)) == stored.as_deref().map(combo_view) {
        with_cb(|cb| (cb.on_edit_app)(a));
        return;
    }
    if let Some(c) = shortcut_shown(hwnd, combo) {
        with_cb(|cb| (cb.on_edit_combo)(c));
    }
    with_cb(|cb| (cb.on_edit_app)(a));
}

/// The five shortcut controls' current values, as a `ComboView` -- the same
/// shape `combo_view` derives from a stored string, so `commit_fields` can
/// compare live controls against stored text without going through a
/// string. `key` is `None` exactly when `CB_GETCURSEL` reports no
/// selection, matching what `ComboView::key` already means.
fn combo_view_of(hwnd: HWND, combo: HWND) -> ComboView {
    let i = cur_sel_raw(combo);
    ComboView {
        ctrl: is_checked(hwnd, IDC_MOD_CTRL),
        super_: is_checked(hwnd, IDC_MOD_WIN),
        alt: is_checked(hwnd, IDC_MOD_ALT),
        shift: is_checked(hwnd, IDC_MOD_SHIFT),
        key: if i < 0 { None } else { Some(i as usize) },
    }
}

/// The combo the five shortcut controls currently spell, or `None` when the
/// key list has no selection.
///
/// **`None` is not an error and must not be turned into one.** A modifier
/// set with no main key is not a combo: writing `ctrl+` into the model would
/// make the row invalid on a keystroke the user has not finished, and flag
/// it for a mistake it is halfway through not making. Every caller sends
/// nothing instead, so the row keeps whatever it had until a key is chosen.
///
/// Spelled through `ComboView::spell` -- i.e. through `Combo::canonical`,
/// not by joining strings -- so the order this window writes and the order
/// the parser prints cannot drift apart.
///
/// The reading of the controls lives in `combo_view_of` and the spelling in
/// core, which is what makes this the exact inverse of the `combo_view` that
/// `commit_fields` compares against. When the two were separate
/// implementations, keeping them inverse was a convention; now it is
/// `spell_round_trips_through_combo_view`, and the macOS window gets it for
/// free rather than by copying this function.
fn shortcut_shown(hwnd: HWND, combo: HWND) -> Option<String> {
    combo_view_of(hwnd, combo).spell()
}

/// Send what the five shortcut controls now spell, if they spell anything.
///
/// `suppressed()` for the reason every other notification here carries it:
/// `apply_state`'s own `BM_SETCHECK` and `CB_SETCURSEL` writes must never
/// come back as user edits. **That guard is also what keeps the availability
/// probe off the data path**: a probe asks the OS for a global registration,
/// and `apply_state` runs on every keystroke.
///
/// The two sends are ordered, not merely adjacent: the probe goes first,
/// while the model still holds the row's previous chord, and the edit second,
/// so the push it triggers is the one that draws the verdict. See
/// `Callbacks::on_probe_shortcut`.
///
/// Two `with_cb` calls rather than one, matching `commit_fields`'s own pair
/// of sends. Either shape is sound -- `with_cb` takes the slot out and holds
/// no borrow while a handler runs -- but taking it once per send keeps the
/// take-then-run discipline local to each, and it is what the file already
/// does everywhere two callbacks fire in a row.
fn push_shortcut(hwnd: HWND, combo: HWND) {
    if suppressed() {
        return;
    }
    if let Some(s) = shortcut_shown(hwnd, combo) {
        with_cb(|cb| (cb.on_probe_shortcut)(s.clone()));
        with_cb(|cb| (cb.on_edit_combo)(s));
    }
}

// ---------------------------------------------------------------------------
// Capture (spec F.3)
// ---------------------------------------------------------------------------
//
// The state machine itself is `beckon_core::capture::step`, run inside the
// `WH_KEYBOARD_LL` callback in `caps_hook.rs`; everything here is drawing and
// lifetime. Three rules govern the whole section and none of them is local:
//
// 1. **No `UI` borrow survives a send.** Every `EnableWindow`,
//    `SetWindowTextW`, `SetTimer`, `KillTimer` and `MessageBeep` below can
//    re-enter this wndproc, and a second `RefCell` borrow taken across an
//    `extern "system"` boundary ABORTS the process rather than unwinding.
//    Every function here copies out what it needs in one borrow and sends
//    afterwards.
// 2. **Nothing here blocks.** The hook callback is dispatched by THIS
//    thread's message loop, so a modal dialog or a synchronous scan starves
//    it exactly as a slow callback would -- and Windows unhooks a callback
//    that overruns `LowLevelHooksTimeout` silently.
// 3. **There is no path where the window dies with the hook armed, and none
//    where `Stop` goes off screen with it armed either.** `end_capture` is
//    idempotent and every route out of this window calls it first: the `Stop`
//    button, all three of spec F.4's focus layers, the watchdog, `WM_CLOSE`
//    (before the save prompt), `WM_DESTROY` -- and, since the tab strip, a
//    page switch. The second clause is the one the strip added: `Stop` is
//    `IDC_RECORD`, a Shortcuts-page control, so a door is an exit path even
//    though the window survives it. `show_page` is where that call lives, and
//    its doc says why it is not in `handle_command`'s pill arm.

/// The notes strip's content while a capture is live: the partial combo,
/// then the hint -- each a `Note` so `paint::draw_notes` draws it exactly
/// like a row's own notes, through the same function.
///
/// **`Mark::Unknown` for both, not `Mark::Ok`.** The blank `Ok` glyph this
/// used to borrow (`mark_glyph(Mark::Ok)`, deleted) was chosen only because
/// it was blank -- pure alignment, nothing about the mark's real meaning.
/// Now that alignment is structural (`draw_notes` draws every dot at the
/// same fixed x regardless of colour), there is no reason left to borrow a
/// mark whose real meaning is "registered and working". A capture in
/// progress is informational, not a verdict either way -- exactly what
/// `Mark::Unknown` already means elsewhere in this window ("Checking
/// installed apps...", "Not registered yet."), so reusing it here says the
/// same thing `row_condition` already says about not-yet-known state,
/// rather than inventing a new case.
///
/// **Two lines is the ceiling `notes_height` reserves, and this fits it
/// exactly, not by accident.** The `Some(p)` arm below IS two lines -- the
/// partial combo, then the hint -- so calling the capture prompt "one line
/// by construction" undercounts it; it fits because two is what it is. A
/// third capture line would clip exactly as a third NOTE line does, and
/// nothing here would stop one from being added -- if one ever is, check it
/// against this ceiling first.
fn capture_notes(c: &Capture) -> Vec<Note> {
    let mut out = Vec::with_capacity(2);
    if let Some(p) = &c.partial {
        out.push(Note {
            mark: Mark::Unknown,
            text: p.clone(),
        });
    }
    out.push(Note {
        mark: Mark::Unknown,
        text: c.hint.clone(),
    });
    out
}

/// Write a caption only when it would change.
///
/// `SetWindowTextW` on a control repaints it, and this runs from
/// `apply_state`, which runs on every keystroke. The comparison is against
/// the RAW caption, `&` and all: that is what `GetWindowTextW` gives back,
/// and `shown` would report a difference on every call.
unsafe fn set_text_if_changed(parent: HWND, id: i32, s: &str) {
    if let Ok(h) = GetDlgItem(Some(parent), id) {
        if text_of(h) != s {
            set_text(h, s);
        }
    }
}

/// Is a capture live, as far as this window's drawing is concerned?
///
/// **Not `caps_hook::capture_armed()`**, which stays true through the drain
/// after a commit or a cancel -- see `Ui::capture`.
fn capture_showing() -> bool {
    UI.with(|u| {
        u.borrow()
            .as_ref()
            .map(|x| x.capture.is_some())
            .unwrap_or(false)
    })
}

/// One fixed sentence on the notes strip, with no state behind it.
///
/// It answers one button press, and the next `apply_state` -- the only other
/// writer of that control -- is what ends it. A field would have to be
/// cleared by something, and there is nothing sensible to clear it on.
unsafe fn say_unavailable() {
    if let Some(notes) = UI.with(|u| u.borrow().as_ref().map(|x| x.notes)) {
        show_notes(
            notes,
            vec![Note {
                mark: Mark::Bad,
                text: HINT_UNAVAILABLE.to_string(),
            }],
        );
    }
}

/// Draw the armed state: the two capture lines, the typed path off, the
/// button reading `Stop`.
///
/// Reads `Ui::capture` and does nothing when it is `None`, so it is safe to
/// call from anywhere.
unsafe fn show_capture(hwnd: HWND) {
    // ONE borrow, dropped on this line. `capture_notes` allocates but makes
    // no OS call, so building the `Vec<Note>` inside it is sound; every send
    // is below.
    let Some((notes, body)) = UI.with(|u| {
        u.borrow()
            .as_ref()
            .and_then(|x| x.capture.as_ref().map(|c| (x.notes, capture_notes(c))))
    }) else {
        return;
    };
    // Compared against `SHOWN_NOTES`, not `text_of(notes)`: the window text
    // is now a DERIVED plain-text mirror (`show_notes`'s own doc), so it is
    // `SHOWN_NOTES` that says whether this call would actually change
    // anything.
    if SHOWN_NOTES.with(|c| *c.borrow() != body) {
        show_notes(notes, body);
    }
    // Two writers on one value is what spec C.4 forbids, and this is the
    // half that enforces it: while the hook is recording, the five controls
    // that spell the same chord cannot be operated.
    for id in SHORTCUT_CONTROLS {
        enable(hwnd, id, false);
    }
    enable(hwnd, IDC_RESET, false);
    // `Stop` must never be out of reach: it is the only way to end a
    // recording with the mouse, and while armed the hook swallows every
    // keystroke, so there is no keyboard route to fall back on.
    enable(hwnd, IDC_RECORD, true);
    set_text_if_changed(hwnd, IDC_RECORD, cap::STOP);
}

/// Take the armed state down, leaving the hook alone.
///
/// **The overlay and the hook have different lifetimes**, and this is the
/// difference: a commit or a cancel ends the overlay immediately, while
/// `caps_hook` keeps the hook until every physically held key is up (spec
/// F.3's draining state). That drain is what makes `alt+tab` recordable
/// without the system ever seeing a bare Alt-up.
///
/// Idempotent -- the field is TAKEN -- which is what lets every caller call
/// it without asking.
///
/// The five controls are restored to enabled rather than to a remembered
/// value, because there is only one state they can have been in: `Record` is
/// pressable exactly when they are (`DefaultButton::Record::pressable`), so
/// arming from a state where they were greyed is unreachable. If the model
/// moved underneath the capture -- the file changed on disk and the
/// selection went away -- `apply_state` is authoritative and corrects them
/// on its next push.
unsafe fn end_overlay(hwnd: HWND) {
    if UI
        .with(|u| u.borrow_mut().as_mut().and_then(|x| x.capture.take()))
        .is_none()
    {
        return;
    }
    set_text_if_changed(hwnd, IDC_RECORD, cap::RECORD);
    for id in SHORTCUT_CONTROLS {
        enable(hwnd, id, true);
    }
    enable(hwnd, IDC_RESET, true);
}

/// Give the hook back and take the armed state down. **Safe to call when
/// nothing is armed**, which is the whole point: every route out of this
/// window calls it without checking first.
unsafe fn end_capture(hwnd: HWND) {
    // The hook FIRST and unconditionally. Holding it one beat longer than
    // the window leaves a swallowed keyboard with nothing left to give it
    // back. `disarm_capture` clears the armed flag before it unhooks, so no
    // further keystroke can enter the capture arm while it is being torn
    // down, and it is a no-op when nothing is armed.
    //
    // Cutting a drain short this way is safe in the one direction that
    // matters. The key-DOWNS this session swallowed never reached the
    // system, so the key-ups that now get through are ups for keys nothing
    // believes are held -- which latch nothing. The dangerous direction is
    // the reverse, a swallowed up after a real down, and that is what
    // `Outcome::PassThrough` covers inside the hook itself.
    caps_hook::disarm_capture();
    // `Err` when there is no such timer, which is the common case: every
    // route out of this window calls this whether or not one was armed.
    let _ = KillTimer(Some(hwnd), IDT_CAPTURE);
    end_overlay(hwnd);
}

/// `Record` was pressed. Arm the hook, or say why not.
unsafe fn start_capture(hwnd: HWND) {
    if capture_showing() {
        return;
    }
    if !caps_hook::arm_capture() {
        // Spec F.3: do NOT enter Armed, and do NOT fall back to
        // message-queue capture. That path cannot see the Windows key, so it
        // fails on precisely the chords beckon recommends -- and it fails by
        // recording the WRONG chord rather than by refusing, which is the
        // worse of the two. `arm_capture` has already logged the underlying
        // `SetWindowsHookExW` error; this is the only thing the user sees.
        say_unavailable();
        return;
    }
    if SetTimer(Some(hwnd), IDT_CAPTURE, CAPTURE_TIMEOUT_MS, None) == 0 {
        // No watchdog, no capture. The watchdog is the ONLY thing that gets
        // the keyboard back when the hook stops delivering without saying so
        // -- `is_installed()` can lie, because past `LowLevelHooksTimeout`
        // Windows removes the hook silently and there is no API to ask (spec
        // F.2). A capture with no bound on it is a keyboard that may never
        // come back, which is the worst outcome this feature has, so it is
        // not entered at all.
        caps_hook::disarm_capture();
        say_unavailable();
        return;
    }
    UI.with(|u| {
        if let Some(x) = u.borrow_mut().as_mut() {
            x.capture = Some(Capture {
                partial: None,
                hint: HINT_ARMED.to_string(),
                beeped_vk: None,
            });
        }
    });
    show_capture(hwnd);
}

/// One outcome from the hook, decoded and drawn.
///
/// The `WPARAM` is `Outcome::code()`; everything else is rebuilt here from
/// `CaptureState`, because the callback that posted this may not allocate.
unsafe fn on_capture(hwnd: HWND, code: usize) {
    // `WM_APP + n` is private by convention only, so a code this version
    // never wrote decodes to nothing rather than to the first variant.
    let Some(outcome) = Outcome::from_code(code) else {
        return;
    };
    // A message posted an instant before a disarm can still be sitting in
    // the queue; acting on it would re-arm a watchdog for a hook that is
    // gone.
    if !caps_hook::capture_armed() {
        return;
    }
    // The watchdog bounds SILENCE rather than the session -- see
    // `CAPTURE_TIMEOUT_MS`. Anything arriving here is proof the hook is
    // still delivering, so the clock starts again. `SetTimer` with a live id
    // replaces the timer rather than adding one.
    let _ = SetTimer(Some(hwnd), IDT_CAPTURE, CAPTURE_TIMEOUT_MS, None);
    match outcome {
        // Every held key is up: the drain is over and the hook goes back.
        // Reached with the overlay already down whenever a commit or a
        // cancel came first, which `end_overlay`'s idempotence absorbs.
        Outcome::Disarmed => end_capture(hwnd),
        Outcome::Captured => {
            let snap = caps_hook::capture_snapshot();
            // The overlay comes down BEFORE the five controls are written.
            // `push_shortcut` below re-enters the caller, which re-enters
            // `apply_state`, which reads `Ui::capture` to decide whether the
            // typed path is disabled -- so leaving it up would have that
            // push grey out the controls this arm has just filled in.
            end_overlay(hwnd);
            let Some(c) = snap.captured else {
                return;
            };
            let Some(combo) = UI.with(|u| u.borrow().as_ref().map(|x| x.combo)) else {
                return;
            };
            // Through `Combo::canonical` and back through `combo_view`,
            // rather than looking the key up in `key_table` here. That pair
            // is the seam `apply_state` already uses to turn a stored combo
            // into control values, so the captured chord cannot disagree
            // with a typed one about which index a key is -- which is the
            // whole of `set_key_sel`'s contract.
            let v = combo_view(&c.canonical());
            // No `suppressed()` guard and none wanted. The reasoning that
            // used to sit here was about `BM_SETCHECK` raising no
            // `BN_CLICKED`, and it does NOT carry over -- these four are
            // `BS_OWNERDRAW` now and have no check state for that message to
            // set. The conclusion survives on stronger ground: `check`
            // reaches `set_chip`, which writes a `Cell` and marks a rectangle
            // dirty, and neither is a notification. `CB_SETCURSEL` still
            // raises no `CBN_SELCHANGE`. And `push_shortcut` below is itself
            // suppression-guarded, so setting the flag here would silently
            // drop the probe.
            check(hwnd, IDC_MOD_CTRL, v.ctrl);
            check(hwnd, IDC_MOD_WIN, v.super_);
            check(hwnd, IDC_MOD_ALT, v.alt);
            check(hwnd, IDC_MOD_SHIFT, v.shift);
            set_key_sel(combo, v.key);
            // Probe first, then the edit -- `push_shortcut` owns that order
            // and the reason is `Callbacks::on_probe_shortcut`'s.
            push_shortcut(hwnd, combo);
        }
        // Bare Esc. The hook swallowed it, so it never became a `MSG`,
        // `IsDialogMessageW` never turned it into `IDCANCEL`, and the window
        // does not close. The hook stays for the drain.
        Outcome::Cancelled => end_overlay(hwnd),
        Outcome::Partial => {
            let snap = caps_hook::capture_snapshot();
            UI.with(|u| {
                if let Some(c) = u.borrow_mut().as_mut().and_then(|x| x.capture.as_mut()) {
                    c.partial = snap.partial;
                    // Holding modifiers is still the prompt, not an error:
                    // releasing them all returns to Armed and spec F.3 calls
                    // that "not an error".
                    c.hint = HINT_ARMED.to_string();
                    // A modifier moved, so a refusal that came before it is
                    // history and the same key pressed again is a new one.
                    c.beeped_vk = None;
                }
            });
            show_capture(hwnd);
        }
        Outcome::Refused(_) => {
            let snap = caps_hook::capture_snapshot();
            // ONE borrow: update the overlay and decide about the beep
            // inside it, then drop it. `MessageBeep` plays a sound
            // asynchronously and is not a call to hold a borrow across.
            let beep = UI
                .with(|u| {
                    u.borrow_mut()
                        .as_mut()
                        .and_then(|x| x.capture.as_mut())
                        .map(|c| {
                            let beep = snap.refused_vk != c.beeped_vk;
                            c.beeped_vk = snap.refused_vk;
                            c.partial = snap.partial;
                            // Always `Some` for a refusal; the fallback is
                            // for totality, not for a reachable case.
                            c.hint = hint(outcome, snap.refused_keycap)
                                .unwrap_or_else(|| HINT_ARMED.to_string());
                            beep
                        })
                })
                .unwrap_or(false);
            if beep {
                // Discarded on purpose: a beep that did not sound -- muted
                // machine, no audio device -- is not a reason to do anything
                // differently, and the hint carries the whole message anyway.
                let _ = MessageBeep(MB_ICONWARNING);
            }
            show_capture(hwnd);
        }
        // Neither is ever posted -- `Outcome::post` is false for both -- and
        // neither says anything about what the window shows.
        Outcome::Ignored | Outcome::PassThrough => {}
    }
}

fn handle_command(hwnd: HWND, id: i32, code: u32) {
    // The shortcut key list and the filter EDIT, and nothing else. The App
    // COMBOBOX is deliberately absent: the one arm that still reads it
    // synchronously reads its LIST, not its edit field, and fetches the
    // handle itself through `app_handle()`. A handle in scope for every arm
    // is an invitation to read the edit field from inside a notification
    // again -- which is the defect `WM_APP_EDITED` exists to prevent.
    // One borrow, one tuple of `Copy` handles, dropped before the match --
    // `apply_state`'s house pattern. Two sequential reads worked but took the
    // borrow twice for no reason.
    let (combo, filter, tap) =
        match UI.with(|u| u.borrow().as_ref().map(|x| (x.combo, x.filter, x.tap))) {
            Some(t) => t,
            None => return,
        };
    match (id, code) {
        // ---- The default ring follows focus. THESE ARMS MUST COME FIRST.
        //
        // Every per-id arm below matches `(id, _)` -- any notification code
        // whatsoever -- and `BS_NOTIFY` has just widened what a push button
        // reports. Behind those arms, tabbing onto Save would PRESS Save.
        //
        // On `BN_SETFOCUS` the focused button becomes the default; on
        // `BN_KILLFOCUS` Save takes it back. The order sorts itself out:
        // Windows delivers the outgoing control's `WM_KILLFOCUS` before the
        // incoming one's `WM_SETFOCUS`, so moving between two buttons
        // restores Save and then immediately overrides it -- and
        // `set_default_id` no-ops when the id it is handed is already the
        // default, so the common Tab step repaints nothing.
        //
        // Deliberately NOT `suppressed()`-guarded, unlike `commit_fields`.
        // Suppression exists to stop `apply_state`'s own control writes being
        // read back as user edits; this changes no model state at all. It
        // does run from inside `apply_state` -- `enable(hwnd, IDC_REMOVE,
        // false)` on a focused Remove raises `BN_KILLFOCUS` -- and that is
        // exactly when the ring MUST leave: focus has genuinely gone. (Safe
        // to re-enter from there: `apply_state` holds no borrow across the
        // block that calls `enable`.)
        (_, c) if is_push_button(id) && (c == BN_SETFOCUS || c == BN_KILLFOCUS) => {
            set_default_id(hwnd, if c == BN_SETFOCUS { id } else { IDC_APPLY });
        }
        // Anything else a push button says is not a command. `BS_NOTIFY`
        // documents `BN_PAINT` / `BN_PUSHED` / `BN_UNPUSHED` / `BN_DISABLE`
        // alongside the two above, and `BN_DOUBLECLICKED` arrives with it in
        // every implementation -- so leaving `(id, _)` to catch them would
        // let a double-click Add twice, or a repaint Save. Only a real click
        // and the accelerator's own code get through.
        (_, c) if is_push_button(id) && c != BN_CLICKED && c != CMD_FROM_ACCELERATOR => {}
        // ---- The tab strip. One arm for all four pills; which door a pill
        // opens is `TABS`'s answer, never a second list written here.
        //
        // Three codes, and none of them is padding. A BUTTON with no
        // `BS_NOTIFY` says exactly `BN_CLICKED` and `BN_DOUBLECLICKED` --
        // `is_chip_click`'s pair, and the seven chips are why that is written
        // down: the second click of a double-click arrives as
        // `BN_DOUBLECLICKED` INSTEAD of a second `BN_CLICKED`, so an arm that
        // took only the one code would drop it. Here that would mean a
        // double-click on a pill switching once and then appearing not to.
        // (Switching twice is not a risk: `show_page` returns on an unchanged
        // door.) `CMD_FROM_ACCELERATOR` is what `Ctrl+1`..`Ctrl+4` arrive as
        // once `build_accelerators` names these ids -- taken now so that is a
        // one-line change there rather than a key that silently does nothing,
        // which is the same failure mode spec 4.4 warns about for `Ctrl+Tab`.
        //
        // A mouse click has already moved the tick before this runs, because
        // the pills are auto-radios. `show_page` ticks anyway; see its doc.
        //
        // `SettingsCommand::ShowPage` is raised only when the door really
        // moved -- the caller stores it so the next `open` lands where the
        // user left off, and "the user moved to another door" should not be
        // reported for a click on the door they are already behind.
        (_, c)
            if page_of_tab(id).is_some()
                && (c == BN_CLICKED || c == BN_DOUBLECLICKED || c == CMD_FROM_ACCELERATOR) =>
        {
            if let Some(page) = page_of_tab(id) {
                if show_page(hwnd, page) {
                    with_cb(|cb| (cb.on_command)(SettingsCommand::ShowPage(page)));
                }
            }
        }
        // The five controls that spell one shortcut, in the two
        // notifications that mean the user changed one: a check box reports
        // `BN_CLICKED`, the key list reports `CBN_SELCHANGE`.
        //
        // Two arms rather than the one `(id, _)` the Caps row uses, and the
        // narrowing is deliberate. A COMBOBOX also says `CBN_SETFOCUS`,
        // `CBN_DROPDOWN`, `CBN_CLOSEUP`, `CBN_SELENDOK` and
        // `CBN_KILLFOCUS`; none of those is an edit, and answering them
        // would push the same value back through the model every time the
        // list is merely opened or tabbed away from.
        //
        // **`toggle_chip` FIRST, and that ordering is the feature**, not a
        // detail of this arm. `push_shortcut` reads all five controls back
        // and spells the whole combo from them, so it has to see the state
        // the user now sees. A `BS_AUTOCHECKBOX` gave that away for free by
        // toggling itself before the notification arrived; `BS_OWNERDRAW`
        // has no state to toggle, so the window does it here. The `Hold`
        // chips below rely on exactly the same order.
        //
        // `is_chip_click` rather than `c == BN_CLICKED`: an owner-draw
        // button sends `BN_DOUBLECLICKED` for the second click of a
        // double-click INSTEAD of a second `BN_CLICKED`, so testing only the
        // one code would toggle once for two clicks.
        (IDC_MOD_CTRL, c) | (IDC_MOD_WIN, c) | (IDC_MOD_ALT, c) | (IDC_MOD_SHIFT, c)
            if is_chip_click(c) =>
        {
            toggle_chip(hwnd, id);
            push_shortcut(hwnd, combo);
        }
        (IDC_COMBO, c) if c == CBN_SELCHANGE => push_shortcut(hwnd, combo),
        (IDC_FILTER, c) if c == EN_CHANGE => {
            if !suppressed() {
                let t = text_of(filter);
                with_cb(|cb| (cb.on_filter)(t));
            }
        }
        // Ask the parent to repaint `paint::field_border`'s ring around
        // `IDC_FILTER` -- see `invalidate_field_border`'s own doc for why
        // this is the whole of what this arm does: `WM_PAINT` reads
        // `GetFocus()` itself, so there is no state to write here, only a
        // repaint to request.
        (IDC_FILTER, c) if c == EN_SETFOCUS || c == EN_KILLFOCUS => unsafe {
            invalidate_field_border(hwnd, filter);
        },
        // ONE of the two codes is deferred, and the asymmetry is the point.
        //
        // `CBN_EDITCHANGE` is deferred through `WM_APP_EDITED`. NOT because
        // the control rewrites its own text -- it does not, and the comment
        // that used to say so here was falsified by measurement; see
        // `WM_APP_EDITED` and `examples/combo_probe.rs`. The deferral is
        // merely harmless, and the App field's actual defect was `layout`
        // resizing the combo box on the keystroke path (`Ui::shown_external`).
        //
        // `CBN_SELCHANGE` is read synchronously out of the LIST, and
        // deferring it was a regression. A mouse pick raises `CBN_SELCHANGE`
        // and then, IN THE SAME BREATH, `CBN_CLOSEUP` -- whose arm calls
        // `commit_fields` SYNCHRONOUSLY. A posted read is dispatched after
        // both, so if the edit field is still stale when `CBN_CLOSEUP`
        // arrives -- the exact uncertainty that made a list read necessary in
        // the first place -- `commit_fields` commits the stale text,
        // `apply_state` writes it back into the field (visibly undoing the
        // pick) and bumps `app_epoch`, discarding the only read that still
        // knew the right value. Reading the list here instead restores the
        // self-correcting order: the model gets the picked value, so
        // `apply_state` puts it in the field, so the `CBN_CLOSEUP` backstop
        // commits the same value again.
        //
        // Reading the LIST here is safe: whatever rewrites the edit field
        // does not move the list selection. This used to be argued from a
        // "type-ahead" that turned out not to exist -- the rewrite is the
        // resize (`Ui::shown_external`), not typing -- but the conclusion
        // survives its premise, and on measurement rather than inference:
        // `CB_GETCURSEL` read -1 at every keystroke (`combo_probe`) AND in the
        // sample taken while the field said "Narrator" and the model said "N"
        // -- i.e. during the rewrite itself (`settings_probe`, a14). The
        // selection never moved. `text_of` is
        // the fallback for a combo with nothing selected (free-typed names
        // that are in no catalog, which beckon deliberately supports).
        (IDC_APP, c) if c == CBN_EDITCHANGE => {
            if !suppressed() {
                post_app_read(hwnd);
            }
        }
        (IDC_APP, c) if c == CBN_SELCHANGE => {
            if !suppressed() {
                // The borrow returns a `Copy` handle and drops with its
                // closure; every send below runs with none held.
                if let Some(app) = app_handle() {
                    let t = selected_combo_text(app).unwrap_or_else(|| text_of(app));
                    with_cb(|cb| (cb.on_edit_app)(t));
                }
            }
        }
        // Tabbing or clicking away commits what is in the field, so a value
        // that reached the control without a notification we acted on is not
        // silently lost. Generic, not aimed at a known offender: the combo
        // box does not rewrite its own text as you type (see
        // `WM_APP_EDITED`).
        //
        // `CBN_CLOSEUP` is safe to handle synchronously only BECAUSE
        // `CBN_SELCHANGE` above is: the pick is already in the model, so
        // `apply_state` has already put it in the field, so this reads the
        // picked value rather than clobbering it with a stale one. Deferring
        // `CBN_SELCHANGE` would make this line undo the pick.
        //
        // There is no `IDC_COMBO` counterpart any more, and its absence is
        // the point rather than an omission: the old one existed because an
        // EDIT can be left holding text nobody reported, and a
        // `CBS_DROPDOWNLIST` holds an index that only `CBN_SELCHANGE` and
        // `CB_SETCURSEL` can move. `commit_fields` still reads all five
        // controls on Save, which is the backstop that was actually load
        // bearing.
        // Field-border repaint, folded into the SAME arm `CBN_KILLFOCUS`
        // already reaches rather than a second one ahead of it: two arms
        // matching the same `(id, code)` would let only the first ever run,
        // and `commit_fields` here is the one that must not be dropped.
        // `CBN_CLOSEUP` never carries a focus change on its own (a pick
        // closes the list but the edit keeps focus), so it is excluded from
        // the repaint request.
        (IDC_APP, c) if c == CBN_KILLFOCUS || c == CBN_CLOSEUP => {
            if c == CBN_KILLFOCUS {
                if let Some(app) = app_handle() {
                    unsafe { invalidate_field_border(hwnd, app) };
                }
            }
            commit_fields();
        }
        (IDC_APP, c) if c == CBN_SETFOCUS => {
            if let Some(app) = app_handle() {
                unsafe { invalidate_field_border(hwnd, app) };
            }
        }
        (IDC_ADD, _) => with_cb(|cb| (cb.on_add)()),
        (IDC_REMOVE, _) => with_cb(|cb| (cb.on_remove)()),
        // One button, two commands: `Record` while idle, `Stop` while armed.
        // The caption is what the user is looking at, so it is the caption's
        // meaning that is dispatched on -- read from `Ui::capture`, which is
        // what wrote it.
        //
        // No `enabled()` guard, matching Add and Remove above and unlike
        // `IDC_APPLY`: the dialog manager does not dispatch a command to a
        // disabled control, a mnemonic on one beeps instead, and there is no
        // accelerator pointed here for `TranslateAcceleratorW` to bypass
        // that with.
        (IDC_RECORD, _) => unsafe {
            if capture_showing() {
                end_capture(hwnd);
            } else {
                start_capture(hwnd);
            }
        },
        // Spec F.3: `Reset` clears the combo and leaves the row without a
        // shortcut. An empty string is exactly what `Model::add_row` gives a
        // new row, so this is a state the model, the renderer and
        // `combo_view` all already handle.
        //
        // No probe: `on_probe_shortcut` asks the OS whether a chord is free,
        // and there is no chord here to ask about. That is also why this
        // does not go through `push_shortcut`, which reads the five controls
        // and sends NOTHING while no key is selected -- see `shortcut_shown`.
        (IDC_RESET, _) => with_cb(|cb| (cb.on_edit_combo)(String::new())),
        (IDC_APPLY, _) => {
            // Ctrl+S reaches this arm too, and `TranslateAcceleratorW` does
            // not care whether the button it names is enabled -- it sends
            // the WM_COMMAND either way. Without this check the keyboard
            // route could save a model the button refuses to save: one that
            // is clean (a pointless rewrite that trips the file watcher) or,
            // worse, one `apply_state` disabled Save for because it has
            // errors in it. A key must never do what its button cannot.
            if !enabled(hwnd, IDC_APPLY) {
                return;
            }
            // The fields are the source of truth at the moment Save is
            // pressed.
            //
            // This used to be described as the ONLY thing standing between a
            // COMBOBOX "that rewrites its own text" and a config file full of
            // single characters (measured on a14: typing "Notepad" wrote
            // "d"). The symptom was real; the cause named was not. The combo
            // box does not rewrite anything while you type -- `apply_state`
            // was resizing it on every keystroke, and a resize is what makes
            // it re-synchronise its edit to the catalogue. See
            // `WM_APP_EDITED` and `Ui::shown_external`.
            //
            // So this is a backstop, and always was one: it stays because a
            // notification that never arrives at all still has to be caught
            // somewhere, and it costs one `WM_GETTEXT` per Save.
            commit_fields();
            with_cb(|cb| (cb.on_apply)())
        }
        (IDC_CAPS, _) => {
            let on = is_checked(hwnd, IDC_CAPS);
            with_cb(|cb| (cb.on_caps)(on));
        }
        // Narrowed from `(id, _)` when these became `BS_OWNERDRAW`, and the
        // narrowing is load-bearing rather than tidy: an owner-draw button
        // sends `BN_DOUBLECLICKED` on its own, so a wildcard arm that also
        // toggles would fire twice for one physical event.
        (IDC_HOLD_CTRL, c) | (IDC_HOLD_WIN, c) | (IDC_HOLD_ALT, c) if is_chip_click(c) => {
            // All three read together: the chord is one value, and a setter
            // that took one flag at a time could not refuse "none ticked"
            // without knowing the other two. `toggle_chip` runs FIRST so
            // that read sees the state the user now sees -- the property
            // `BS_AUTOCHECKBOX` used to provide by toggling itself before
            // the notification arrived.
            toggle_chip(hwnd, id);
            let hold = Chord {
                ctrl: is_checked(hwnd, IDC_HOLD_CTRL),
                super_: is_checked(hwnd, IDC_HOLD_WIN),
                alt: is_checked(hwnd, IDC_HOLD_ALT),
            };
            with_cb(|cb| (cb.on_caps_hold)(hold));
        }
        // Read out of the LIST by index, never from text. The `suppressed()`
        // guard is the same one every other combo notification here carries:
        // it drops anything raised while `apply_state` is writing, so a push
        // can never be read back as a pick.
        (IDC_TAP, c) if c == CBN_SELCHANGE => {
            if !suppressed() {
                if let Some(i) = cur_sel(tap) {
                    let t = match i {
                        0 => CapsTap::CapsLock,
                        1 => CapsTap::Escape,
                        _ => CapsTap::None,
                    };
                    with_cb(|cb| (cb.on_caps_tap)(t));
                }
            }
        }
        (IDC_OPENFILE, _) => with_cb(|cb| (cb.on_open_file)()),
        (IDC_RELOAD, _) => with_cb(|cb| (cb.on_reload_from_disk)()),
        (IDC_KEEPMINE, _) => with_cb(|cb| (cb.on_keep_mine)()),
        // Both the Close button and Esc go through WM_CLOSE, so the save
        // prompt is asked once however the window is dismissed. Esc arrives
        // as a WM_COMMAND with IDCANCEL, synthesised by `IsDialogMessageW`
        // -- the id is spelled 2 here because `windows` types the constant
        // as a MESSAGEBOX_RESULT, which is what `shell::ask_save` returns
        // and not what a control id is. There is exactly one caller of
        // `on_close_request`, which is what makes "asked once" true: the
        // system's own [X] posts WM_CLOSE straight to the wndproc and lands
        // in the same arm.
        (IDC_CLOSE, _) | (2 /* IDCANCEL */, _) => unsafe {
            let _ = PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
        },
        _ => {}
    }
}

/// What the App combo box's LIST currently has selected, which on
/// `CBN_SELCHANGE` is the value the user picked -- the edit field is
/// documented not to have caught up when that notification is sent.
///
/// `None` when nothing is selected, which is the ordinary state for a name
/// the user typed that is in no catalog. The caller falls back to the field.
fn selected_combo_text(app: HWND) -> Option<String> {
    unsafe {
        let i = SendMessageW(app, CB_GETCURSEL, Some(WPARAM(0)), Some(LPARAM(0))).0;
        if i < 0 {
            return None;
        }
        let len = SendMessageW(
            app,
            CB_GETLBTEXTLEN,
            Some(WPARAM(i as usize)),
            Some(LPARAM(0)),
        )
        .0;
        if len <= 0 {
            return None;
        }
        let mut buf = vec![0u16; len as usize + 1];
        SendMessageW(
            app,
            CB_GETLBTEXT,
            Some(WPARAM(i as usize)),
            Some(LPARAM(buf.as_mut_ptr() as isize)),
        );
        let n = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        Some(String::from_utf16_lossy(&buf[..n]))
    }
}

/// Report a save failure. The window has somewhere to put this, unlike
/// bare `serve`.
pub fn error(body: &str) {
    shell::error_dialog("beckon", body);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    // Nearly everything in this file needs a live window, and a window on a
    // CI agent is a different question from a window on a desktop -- which
    // is what `examples/settings_probe.rs` exists for. These two functions
    // are the exception: pure, total, and each with an edge case that is
    // silent when it is wrong. They run on the Windows CI job, which is the
    // only one that compiles this module.

    #[test]
    fn shown_drops_the_mnemonic_marker() {
        assert_eq!(shown("&Save"), "Save");
        assert_eq!(shown("Re&move"), "Remove");
        assert_eq!(shown("Close"), "Close");
    }

    #[test]
    fn shown_collapses_a_doubled_ampersand_to_one() {
        // The case that makes this a function rather than a `replace`.
        // `layout` measures through `shown`, and a doubled ampersand is one
        // character of INK -- dropping both would size the button too
        // narrow for its own caption, which is the defect measurement was
        // introduced to prevent.
        assert_eq!(shown("Notes && To Do"), "Notes & To Do");
        assert_eq!(shown("&&"), "&");
        assert_eq!(shown("&&&Save"), "&Save");
    }

    #[test]
    fn shown_swallows_a_trailing_ampersand() {
        // A marker with nothing to mark. Windows draws nothing for it, so
        // measuring it would make the button one character too wide -- and
        // the naive loop that reads the NEXT character would run off the
        // end.
        assert_eq!(shown("Save&"), "Save");
        assert_eq!(shown("&"), "");
    }

    #[test]
    fn title_base_is_the_file_name_only() {
        // `Path::new` at every call: `title_base` takes a `&Path` now that
        // `open` takes a `Paths`. The cases are the same ones, and the
        // function still asks `Path::file_name` the same question.
        assert_eq!(
            title_base(Path::new(
                r"C:\Users\a\AppData\Roaming\beckon\shortcuts.toml"
            )),
            "beckon - shortcuts.toml"
        );
        // Forward slashes are separators on Windows too, and `serve` is
        // perfectly reachable with a path typed that way.
        assert_eq!(
            title_base(Path::new("C:/cfg/shortcuts.toml")),
            "beckon - shortcuts.toml"
        );
        assert_eq!(
            title_base(Path::new("shortcuts.toml")),
            "beckon - shortcuts.toml"
        );
    }

    #[test]
    fn title_base_falls_back_when_there_is_no_file_name() {
        // Every one of these makes `Path::file_name` return None, and the
        // format string would otherwise put an empty name after the
        // separator -- a title bar reading `beckon - ` looks like the window
        // failed to load something.
        assert_eq!(title_base(Path::new("")), "beckon");
        assert_eq!(title_base(Path::new(r"C:\")), "beckon");
        assert_eq!(title_base(Path::new("..")), "beckon");
        assert_eq!(title_base(Path::new(r"C:\cfg\..")), "beckon");
    }

    #[test]
    fn the_starting_default_button_is_a_push_button() {
        // `set_default_id` only ever moves the ring between members of this
        // set, and `handle_command` only filters notifications for members
        // of it. A Save left out of it would take the ring off the window
        // the first time focus touched any other button.
        assert!(is_push_button(IDC_APPLY));
        assert!(is_push_button(IDC_RELOAD));
        assert!(is_push_button(IDC_CLOSE));
        // The four check boxes must stay out: they are BUTTONs, they have no
        // `BS_NOTIFY`, and their `(id, _)` arms in `handle_command` are what
        // carries a click to `on_caps` / `on_caps_hold`.
        assert!(!is_push_button(IDC_CAPS));
        assert!(!is_push_button(IDC_HOLD_CTRL));
        assert!(!is_push_button(IDC_HOLD_WIN));
        assert!(!is_push_button(IDC_HOLD_ALT));
        assert!(!is_push_button(IDC_LIST));
    }

    /// Which door a control lives behind, or `None` for the chrome that is
    /// drawn on every page. Test-local: `show_page_controls` walks
    /// `PAGE_CONTROLS` once for the whole table rather than asking per
    /// control, and a second reader in the shipping window would be a second
    /// spelling of what that loop already does.
    fn page_of_control(id: i32) -> Option<Page> {
        PAGE_CONTROLS
            .iter()
            .find(|(c, _)| *c == id)
            .map(|(_, p)| *p)
    }

    /// Why `repair_hidden_button` asks the SCREEN as well as
    /// `DefaultButton::visible`.
    ///
    /// Each of these can hold keyboard focus, each is behind the Shortcuts
    /// door, and none is one of the nine `PUSH_BUTTONS` -- so a test on
    /// buttons alone cannot reach any of them, and a switch taken with focus
    /// on one would leave `GetFocus` on an off-screen control. On `IDC_APP`
    /// that is not merely invisible typing: no `CBN_KILLFOCUS` means no
    /// `commit_fields`, so the text reaches no model either.
    #[test]
    fn the_focusable_controls_a_door_hides_are_not_push_buttons() {
        for id in [IDC_APP, IDC_FILTER, IDC_COMBO, IDC_LIST] {
            assert!(
                !is_push_button(id),
                "control {id} is a push button, so this test no longer says \
                 anything about the gap `hidden_child` closes"
            );
            assert_eq!(
                page_of_control(id),
                Some(Page::Shortcuts),
                "control {id} is not behind a door, so a switch cannot hide it"
            );
        }
    }

    /// Why `show_page` calls `end_capture`.
    ///
    /// `Stop` is not a control of its own: it is `IDC_RECORD` wearing another
    /// caption, and `IDC_RECORD` is behind the Shortcuts door. So a switch
    /// takes the only visible way to end a recording off the screen while the
    /// `WH_KEYBOARD_LL` hook is still swallowing every keystroke.
    #[test]
    fn stop_is_behind_the_shortcuts_door() {
        assert_eq!(page_of_control(IDC_RECORD), Some(Page::Shortcuts));
    }

    /// The seam between `Ui::defid` (a control id) and the pure decision (an
    /// enum). It carries the whole default-button fix, and a mapping that
    /// disagreed with itself would be silent: the ring would simply stop
    /// moving, exactly as it did before the fix existed.
    #[test]
    fn every_push_button_round_trips_through_the_default_button_enum() {
        for id in PUSH_BUTTONS {
            assert_eq!(
                id_of_default_button(default_button_of(id)),
                id,
                "control {id} does not survive the round trip"
            );
        }
        for b in DefaultButton::ALL {
            assert_eq!(default_button_of(id_of_default_button(b)), b);
            assert!(
                is_push_button(id_of_default_button(b)),
                "{b:?} maps to an id `handle_command` does not treat as a \
                 push button, so its ring would never move"
            );
        }
    }

    /// The strip's ids have to be a contiguous ascending run, because
    /// `build_children` and `show_page` both tick a pill with
    /// `CheckRadioButton(hwnd, IDC_TAB_SHORTCUTS, IDC_TAB_ABOUT, id)` -- a
    /// call that takes a FIRST and a LAST id and clears everything between
    /// them. A gap would put ids the strip does not own inside that range; a
    /// re-order would make the pair name the wrong ends. Neither is visible
    /// from the constants themselves.
    ///
    /// It also pins `tab_id_of` -- an exhaustive `match`, so that a fifth
    /// `Page` is a compile error -- against `TABS`, which is the same fact
    /// spelled a second way and therefore the one that can drift.
    #[test]
    fn the_tab_ids_are_contiguous_and_agree_with_tab_id_of() {
        for (i, (id, page, _)) in TABS.iter().enumerate() {
            assert_eq!(
                *id,
                IDC_TAB_SHORTCUTS + i as i32,
                "pill {i} breaks the contiguous run CheckRadioButton needs"
            );
            assert_eq!(tab_id_of(*page), *id, "tab_id_of disagrees with TABS");
            assert_eq!(page_of_tab(*id), Some(*page));
        }
        assert_eq!(
            TABS[TABS.len() - 1].0,
            IDC_TAB_ABOUT,
            "the last pill is what `CheckRadioButton` is handed as its last id"
        );
        // Nothing else answers. `handle_command`'s tab arm is a membership
        // test on this function, so an id that matched by accident would
        // switch pages on a click meant for another control.
        assert_eq!(page_of_tab(IDC_APPLY), None);
        assert_eq!(page_of_tab(IDC_CAPS), None);
        assert_eq!(page_of_tab(0), None);
    }

    /// A pill must never be a push button, and there are two independent
    /// reasons -- `set_button_type` would rewrite `BS_AUTORADIOBUTTON` into
    /// `BS_PUSHBUTTON` through `BS_TYPEMASK_BITS` the first time the default
    /// ring moved, and
    /// `every_push_button_round_trips_through_the_default_button_enum`
    /// requires every member of `PUSH_BUTTONS` to name a `DefaultButton`,
    /// which would make Enter on a focused pill press it as a command.
    #[test]
    fn a_tab_pill_is_never_a_push_button() {
        for (id, _, _) in TABS {
            assert!(!is_push_button(id), "pill {id} is in PUSH_BUTTONS");
            assert_eq!(default_button_of(id), DefaultButton::HOME);
        }
    }

    /// The mapping is total, and anything unknown reads as the button the
    /// ring rests on. `GetDlgCtrlID` returns 0 for the parent window and
    /// comctl32 gives a combo box's inner EDIT an id of its own choosing --
    /// both reach `default_button_of`.
    #[test]
    fn an_id_that_is_not_a_push_button_reads_as_home() {
        assert_eq!(default_button_of(0), DefaultButton::HOME);
        assert_eq!(default_button_of(IDC_CAPS), DefaultButton::HOME);
        assert_eq!(default_button_of(-1), DefaultButton::HOME);
        assert_eq!(id_of_default_button(DefaultButton::HOME), IDC_APPLY);
    }
}
