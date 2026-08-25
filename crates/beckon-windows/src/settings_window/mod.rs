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
//! `Ctrl+Tab` and `Ctrl+1`..`Ctrl+4` ARE accelerators, and
//! `TranslateAcceleratorW` runs before `IsDialogMessageW` and moves no focus,
//! so the combo would otherwise be resized while focused, populated and
//! holding half-typed text.
//!
//! **This sentence was written in the future tense until the table grew, and
//! before that in the present when it should not have been.** What it said
//! while `build_accelerators` held `Ctrl+S` alone was that every switch
//! arrives as a pill click or an arrow key, both of which move focus onto the
//! pill first -- true then, and no longer: an accelerator moves no focus at
//! all, so the six keys reach `show_page` with focus wherever the user left
//! it. That is why the guard here and `repair_hidden_button` in `show_page`'s
//! step 5 both had to exist BEFORE this table did.
//!
//! **CORRECTED 2026-08-14: that guard is ONE-DIRECTIONAL, and on its own it
//! left half the round trip unfixed.** Skipping the band keeps the combo out
//! of reach on the way OUT; every switch back IN still placed it -- and that
//! placement was a genuine resize every single time, not only when the
//! geometry had drifted, because `layout` passes `field_h * 9` (the DROPPED
//! height) while the combo's window rect holds its closed height, so the two
//! can never match and nothing upstream can elide the call. That is the same
//! mechanism the a14 measurement pinned: nothing in the layout had moved and
//! the typing was still lost. The second half is
//! `beckon_core::settings::combo_needs_placing`, which asks the control where
//! it is and does not call `SetWindowPos` when the answer is "already
//! there" -- deliberately spelled as "do not make the call" rather than as a
//! `SWP_NOSIZE` short-circuit, so it does not depend on spec 10 open
//! question 1.
//!
//! **EXTENDED 2026-08-14: skipping the unnecessary placements is not the whole
//! of it either, because some of them are necessary.** `layout` reads six
//! inputs (`Ui::shown_external` lists them) and skips the App combo whenever
//! another door is open, so any input that moves while the user is behind
//! Keyboard, System or About leaves the combo genuinely stale and the trip back
//! genuinely has to place it. A resize taken on another door, a
//! `WM_DPICHANGED` there and the list gaining its first row are all such
//! inputs, and `WM_SIZE` on Shortcuts itself never went through the doors at
//! all. (**CORRECTED 2026-08-14, Task 6:** this named the banner as a fourth.
//! It was one while `banner_shown` ignored the page; narrowed back to
//! `BANNER_PAGE`, card 0 cannot gain height while the user is behind another
//! door, so it no longer is. The other three are untouched and are why the
//! rule stands.) `place_app_combo` is what makes
//! the necessary placement survivable: it saves the edit's text and selection
//! across the one `SetWindowPos` and puts them back if the control rewrote
//! them. So the rule for this call site now has three parts -- do not place
//! from another door, do not place a combo that has not moved, and restore the
//! edit when you do place.
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
    about_state, banner_shown, caps_view_enabled, caps_view_fold, combo_needs_placing,
    command_bar_shown, copy_text, default_button, image_identity, opacity_alpha, system_state,
    warn_dot_shown, AboutInputs, AboutState, ComboSpot, ControlState, DefaultButton, Field,
    FlagTone, ImageOnDisk, ListItem, Mark, Note, Page, Paths, ServiceLine, SettingsCommand,
    SystemInputs, SystemState, Target, Transparency, BANNER_PAGE,
};
use beckon_core::shortcuts::{
    combo_display_folded, combo_view, key_label, key_table, CapsTap, Chord, ComboView,
};
use std::cell::RefCell;
use windows::core::{w, PCWSTR, PWSTR};
use windows::Win32::Foundation::{
    COLORREF, FILETIME, HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM,
};
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
/// `GetProcessTimes` against `GetCurrentProcess`' pseudo-handle: half of the
/// About page's stale-image verdict, and the half that cannot be got any
/// other way -- a process's own start time is not in any environment variable
/// or any file. `QueryFullProcessImageNameW` is the OTHER half, the identity
/// test, and it is the one that can see a moved scoop junction; `current_exe`
/// deliberately cannot, because it answers about the launch path. Neither
/// costs a new `windows` feature -- both live in `Win32_System_Threading`,
/// which this crate already enables for `OpenProcess`, and `window_ops.rs`
/// already calls `QueryFullProcessImageNameW` (about OTHER processes, for
/// AUMID resolution) with the same `PROCESS_NAME_WIN32` / `PWSTR` shape used
/// below.
use windows::Win32::System::Threading::{
    GetCurrentProcess, GetProcessTimes, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
};
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
    TRACKMOUSEEVENT, VK_1, VK_TAB,
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

/// `SS_PATHELLIPSIS` (0x8000), which `windows` 0.61 does not export either.
///
/// It shortens a string that does not fit by replacing characters in the
/// MIDDLE with `...`, keeping the beginning and the end -- for a path, the
/// drive and the last folder, which are the two parts that identify it. That
/// is why the System page's config row carries it and its log row does not:
/// only one of the two values is a path, and on `112 KB` the style finds no
/// separator to cut at.
///
/// **The shortening belongs to the OS, deliberately.**
/// `beckon_core::settings::system_state` supplies the whole directory and
/// counts no characters, because a character count is not a width -- the face
/// is proportional and the control's width moves with the window.
const SS_PATHELLIPSIS_STYLE: WINDOW_STYLE = WINDOW_STYLE(0x8000);

/// `SS_RIGHT` (0x0002), which `windows` 0.61 does not export either.
///
/// **A different VALUE of a STATIC's type field, not a flag** -- the same
/// low-bit field `SS_LEFT` (0) and `SS_OWNERDRAW` (13) occupy, exactly the
/// relationship `SS_OWNERDRAW_STYLE` above describes. It is what puts the
/// System page's two file-row values against the glyph buttons rather than
/// stranded in the middle of the row, which is where the mock-up draws them.
///
/// **Combining it with `SS_PATHELLIPSIS` is the one thing here that is
/// documented ambiguously**, and the config row does exactly that. The
/// ellipsis styles are described against `SS_LEFT`; whether the shortening
/// still runs under `SS_RIGHT` is not stated, and nothing on the machine this
/// was written on can display the window. The failure mode if it does not is
/// a long path CLIPPED rather than shortened -- the row still shows the start
/// of the directory, which is the half a reader can act on -- so this is a
/// look question, not a correctness one. `settings_probe`'s System section
/// prints the control's text and a screenshot shows which happened.
const SS_RIGHT_STYLE: WINDOW_STYLE = WINDOW_STYLE(0x0002);

/// `SS_CENTER` (0x0001), which `windows` 0.61 does not export either.
///
/// A third VALUE of the same low-bit type field `SS_LEFT` (0), `SS_RIGHT` (2)
/// and `SS_OWNERDRAW` (13) share -- so it is `|`-ed in like a flag and is not
/// one, and combining it with `SS_RIGHT` would produce `SS_SIMPLE` (3) rather
/// than a contradiction the compiler could catch.
///
/// One control carries it: `IDC_ABOUT_NAME`, the `beckon 0.9.3` line under
/// the mark. It is the only centred text in the window, which is why the
/// constant arrives with the About page rather than earlier.
const SS_CENTER_STYLE: WINDOW_STYLE = WINDOW_STYLE(0x0001);

/// `EM_SETCUEBANNER` (`ECM_FIRST + 1`), which `windows` 0.61 does not
/// export -- the same gap `SS_CENTERIMAGE_STYLE` above fills.
const EM_SETCUEBANNER_MSG: u32 = 0x1501;

/// `TBM_GETPOS`, which is `WM_USER + 0` and therefore the one trackbar
/// message the `windows` crate does not generate a constant for -- every
/// other `TBM_*` this window sends (`TBM_SETRANGE`, `TBM_SETPOS`,
/// `TBM_SETPAGESIZE`, `TBM_GETRANGEMIN`, `TBM_GETRANGEMAX`) is imported by
/// name. Spelled out here rather than as a bare `WM_USER` at the call site,
/// for the same reason `DM_GETDEFID_MSG` above is.
const TBM_GETPOS_MSG: u32 = WM_USER;

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
/// **The System page's five joined on 2026-08-15 and had to.** The four glyph
/// buttons look like ornaments and are ordinary push buttons; leaving them out
/// would mean no `BS_NOTIFY`, so the ring could not follow focus onto them,
/// so `IsDialogMessageW` would fall through to `DM_GETDEFID` -- which still
/// says `Save`. Enter on a focused `Open config file` glyph would have
/// written the config file. That is the `Reload` defect this list was built
/// for, two pages across.
///
/// **The About page's six joined the next day, for the same reason and with
/// one extra consequence worth naming.** Three of them are copy glyphs, which
/// look even more like ornaments than System's four -- and a stray Enter on a
/// copy button silently replaces whatever the user had on the clipboard,
/// which is a loss they do not see until they paste. The other three open a
/// browser.
///
/// **The update check's three joined on 2026-08-25 (Task 9), for the same
/// reason again.** `IDC_ABOUT_CHECK_NOW` is an ordinary push button; leaving
/// it out would mean Enter on a focused `Check now` fell through to whatever
/// `DM_GETDEFID` last said, which on this door is `home(Page::About)` --
/// `None` -- so nothing would happen at all, silently, on the one button this
/// page most wants a keyboard route to. `IDC_ABOUT_UPDATE_COPY` carries
/// `AboutBuildCopy`'s own warning doubled: a stray Enter on it while disabled
/// does nothing (the dialog manager will not dispatch to a disabled
/// control), but while enabled it is a copy button like the other three, and
/// silently replacing the clipboard is a loss the user will not see until
/// they paste. `IDC_ABOUT_OPEN_RELEASES` opens a browser, `IDC_ABOUT_RELEASES`'s
/// own reason for being here.
const PUSH_BUTTONS: [i32; 21] = [
    IDC_ADD,
    IDC_REMOVE,
    IDC_APPLY,
    IDC_OPENFILE,
    IDC_CLOSE,
    IDC_RELOAD,
    IDC_KEEPMINE,
    IDC_RECORD,
    IDC_REVERT,
    IDC_SYS_RELOAD,
    IDC_CONFIG_OPEN,
    IDC_CONFIG_SHOW,
    IDC_LOG_OPEN,
    IDC_LOG_SHOW,
    IDC_ABOUT_BUILD_COPY,
    IDC_ABOUT_LOCATION_COPY,
    IDC_ABOUT_GITHUB,
    IDC_ABOUT_RELEASES,
    IDC_ABOUT_BUG,
    IDC_ABOUT_CHECK_NOW,
    IDC_ABOUT_UPDATE_COPY,
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

/// The widest count the Shortcuts pill's badge reserves room for, as the
/// string that gets MEASURED rather than as a number of digits.
///
/// **Zeros, not nines, and the difference is not zero.** The face is
/// proportional; `0` is the widest digit in most UI faces and never narrower
/// than `9`, so measuring `0000` cannot under-reserve where `9999` might.
///
/// Four digits is the reservation, not the limit. A fifth digit draws into
/// the pill's own `TAB_PAD_X` padding, which is 14 px against a digit of
/// roughly 6 -- so a config with 10 000 bindings looks slightly tight and one
/// with 100 000 clips. That is the right way round: the alternative is a pill
/// whose width follows the data, and the only way to apply a new width is
/// `layout`, which is `SetWindowPos` on the populated App combo -- the
/// measured data-loss call. See `badge_slot_w`.
const BADGE_SLOT: &str = "0000";

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
/// **Two kinds of control are absent, and each absence is a decision:**
///
/// - **The four pills and the command bar's three buttons are chrome.** They
///   are drawn on every page, so they belong to none, and listing them here
///   with a page would be a lie the first time someone read it.
/// - **The banner's three (`IDC_BANNER`, `IDC_RELOAD`, `IDC_KEEPMINE`) answer
///   to `banner_shown`, not to a page.** They are on every door while the file
///   has moved and on none of them otherwise, so a table that only knows about
///   pages would either strand them on one door or show them with nothing to
///   announce. `show_page_controls` handles them beside this loop, from the
///   same function core's `DefaultButton::visible` reads -- which is what
///   keeps them a single decision when Task 6 narrows it back to Shortcuts.
///
/// **CORRECTED 2026-08-14, Task 7.** There was a third bullet, and it read
/// "**System and About own nothing yet.** Both draw as an empty surface below
/// the strip until Task 7 gives them a line each." Each now owns its waiting
/// line, so the absence it described is gone and its two rows are at the
/// bottom of the table. What has NOT changed is the vertical stack behind
/// those two doors -- see `compute_card_rects` in `layout.rs` for why the
/// re-stack was weighed and deferred again rather than taken here.
///
/// **CORRECTED A THIRD TIME 2026-08-15**: both waiting lines are gone and
/// both pages own real controls, so no door in this window is a placeholder
/// any more. About's fifteen are unconditional -- unlike System's fourteen,
/// two of whose rows depend on `SYS_ROWS`, there is nothing about this
/// machine that can remove a row from the About page.
///
/// **CORRECTED A FOURTH TIME 2026-08-25 (Task 9)**: About grew five more,
/// for the update check, and every one of the twenty is STILL unconditional
/// in the sense this paragraph means -- on screen or not is `page` alone,
/// with no `SYS_ROWS`-style second table. Two of the five vary with LIVE
/// state (an update check can finish or fail while the window is open) but
/// answer that with `EnableWindow`, never `ShowWindow`, precisely so they
/// need no second table here -- see `IDC_ABOUT_UPDATE_STATUS`'s own doc in
/// `ids.rs`.
///
/// **CORRECTED AGAIN 2026-08-15**: System's waiting line is gone and its
/// fourteen real controls are here. Two of the page's rows are CONDITIONAL --
/// `Start with Windows` when this process cannot offer it, and the log row
/// when `serve` ran without `--log` -- and they are in this table like every
/// other control. Being behind the System door and being on screen are two
/// questions; this table answers the first, and `SYS_ROWS` answers the
/// second, in `show_page_controls`, which applies both. Putting the
/// conditional five in the banner's exempt group instead would have hidden
/// them from `every_control_belongs_to_exactly_one_group`'s partition for a
/// condition that has nothing to do with `external_change`.
///
/// `every_control_belongs_to_exactly_one_group` in `ids.rs` is what keeps the
/// two absences honest: it partitions `MINE` across this table, the pills,
/// the banner and the command bar, and fails on any control that lands in
/// neither or in two. Without it, a control added later and forgotten here is
/// simply visible on all four pages -- which looks like a layout bug and is a
/// table bug.
const PAGE_CONTROLS: [(i32, Page); 51] = [
    // -- Shortcuts: the head row, the list, and the editor strip below it.
    // The head row's `Shortcuts` heading (`IDC_LBL_SECTION`, 1020) left this
    // table with the control on 2026-08-15; the row itself is unchanged.
    (IDC_FILTER, Page::Shortcuts),
    (IDC_REMOVE, Page::Shortcuts),
    (IDC_ADD, Page::Shortcuts),
    (IDC_LIST, Page::Shortcuts),
    (IDC_APP, Page::Shortcuts),
    (IDC_MOD_CTRL, Page::Shortcuts),
    (IDC_MOD_WIN, Page::Shortcuts),
    (IDC_MOD_ALT, Page::Shortcuts),
    (IDC_MOD_SHIFT, Page::Shortcuts),
    (IDC_COMBO, Page::Shortcuts),
    (IDC_RECORD, Page::Shortcuts),
    (IDC_REVERT, Page::Shortcuts),
    (IDC_NOTES, Page::Shortcuts),
    // -- Keyboard: the Caps line, and nothing else yet.
    (IDC_CAPS_SHORTHAND, Page::Keyboard),
    (IDC_CAPS, Page::Keyboard),
    (IDC_LBL_HOLD, Page::Keyboard),
    (IDC_HOLD_CTRL, Page::Keyboard),
    (IDC_HOLD_WIN, Page::Keyboard),
    (IDC_HOLD_ALT, Page::Keyboard),
    (IDC_LBL_TAP, Page::Keyboard),
    (IDC_TAP, Page::Keyboard),
    // -- System: the service group, the look group, the two file rows.
    (IDC_PAUSE, Page::System),
    (IDC_AUTOSTART, Page::System),
    (IDC_SYS_RELOAD, Page::System),
    (IDC_DARK, Page::System),
    (IDC_OPACITY_VALUE, Page::System),
    (IDC_OPACITY, Page::System),
    (IDC_CONFIG_NAME, Page::System),
    (IDC_CONFIG_DIR, Page::System),
    (IDC_CONFIG_OPEN, Page::System),
    (IDC_CONFIG_SHOW, Page::System),
    (IDC_LOG_NAME, Page::System),
    (IDC_LOG_SIZE, Page::System),
    (IDC_LOG_OPEN, Page::System),
    (IDC_LOG_SHOW, Page::System),
    // -- About: the mark and the name, the three value rows, the disclosure,
    // the three links.
    (IDC_ABOUT_MARK, Page::About),
    (IDC_ABOUT_NAME, Page::About),
    (IDC_ABOUT_BUILD_LABEL, Page::About),
    (IDC_ABOUT_BUILD_VALUE, Page::About),
    (IDC_ABOUT_BUILD_COPY, Page::About),
    (IDC_ABOUT_UPDATE_STATUS, Page::About),
    (IDC_ABOUT_CHECK_NOW, Page::About),
    (IDC_ABOUT_UPDATE_VALUE, Page::About),
    (IDC_ABOUT_UPDATE_COPY, Page::About),
    (IDC_ABOUT_LOCATION_LABEL, Page::About),
    (IDC_ABOUT_LOCATION_VALUE, Page::About),
    (IDC_ABOUT_LOCATION_COPY, Page::About),
    (IDC_ABOUT_DISCLOSURE, Page::About),
    (IDC_ABOUT_GITHUB, Page::About),
    (IDC_ABOUT_RELEASES, Page::About),
    (IDC_ABOUT_BUG, Page::About),
];

/// Which of the System page's two CONDITIONAL rows are on screen.
///
/// **A `Cell`, for `PILL_BADGE`'s reason, and this is the fifth time that
/// reason has decided a design here.** `compute_card_rects` is documented
/// never to touch `UI` -- `card_rects` calls it from inside `WM_PAINT`, where
/// `UI` can already be borrowed, and a second `RefCell` borrow across an
/// `extern "system"` boundary aborts the process rather than unwinding. The
/// System card's HEIGHT depends on both flags, so the answer has to be
/// reachable from there.
///
/// **Both facts are fixed for the window's lifetime**, which is what makes a
/// stale read impossible rather than merely unlikely: `serve` is started with
/// `--log` or it is not, and `AutostartCapability` is decided before the tray
/// exists. They still arrive through `apply_system_state` rather than through
/// `open`, because the window is a renderer of pushed state and adding a
/// second way in would be a second thing to keep in step.
///
/// The resting value hides both. The first push arrives immediately after
/// `open` (`serve.rs`'s `refresh_settings`), and hidden-then-shown is the
/// safe direction: a row that flickers into existence is a cosmetic fault,
/// while a row shown for a capability this process does not have is a
/// control that does nothing.
/// **The type moved to `beckon_core::page_plan` on 2026-08-15** with the plan
/// function that reads it; this alias is what keeps the ~8 sites here reading
/// as they did. The thread-local, the push and `sys_row_shown` are still the
/// window's -- only "which rows exist, and therefore how tall the card is" is
/// core's.
use beckon_core::page_plan::SystemRows;

thread_local! {
    static SYS_ROWS: std::cell::Cell<SystemRows> = const {
        std::cell::Cell::new(SystemRows {
            autostart: false,
            log: false,
        })
    };
}

fn sys_rows() -> SystemRows {
    SYS_ROWS.with(|c| c.get())
}

/// Is this control on screen at all, given which System rows exist?
///
/// **One function, three readers**: `show_page_controls` (which owns the
/// `ShowWindow`), `layout` (which skips placing a row that is not there) and
/// `compute_card_rects` (which does not reserve its height). Three spellings
/// of "is the log row up" would disagree the first time one of them was
/// edited, and the disagreement reads as a rendering fault -- a card with a
/// gap at the bottom, or a row drawn half outside it.
///
/// Everything not named here is unconditional and answers `true`, including
/// every control on the other three pages: this is a mask applied ON TOP of
/// `PAGE_CONTROLS`, never a second page table.
fn sys_row_shown(id: i32, rows: SystemRows) -> bool {
    match id {
        IDC_AUTOSTART => rows.autostart,
        IDC_LOG_NAME | IDC_LOG_SIZE | IDC_LOG_OPEN | IDC_LOG_SHOW => rows.log,
        _ => true,
    }
}

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
    // Two conditions, ANDed, and neither may be dropped: the door decides
    // which page's controls are candidates, and `sys_row_shown` decides which
    // of the System page's candidates exist on this machine at all. Only the
    // second is a `SYS_ROWS` question, and only the first is a `page` one.
    let rows = sys_rows();
    for (id, owner) in PAGE_CONTROLS {
        if let Ok(h) = GetDlgItem(Some(hwnd), id) {
            show(
                h,
                owner == page && sys_row_shown(id, rows) && about_row_shown(id),
            );
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
    // The command bar's three, handled here rather than through
    // `PAGE_CONTROLS` for the banner's exact reason: that table maps an id to
    // ONE owning door, and these three belong to two. `command_bar_shown` is
    // the same function `DefaultButton::visible` reads, so the buttons on
    // screen and the ring's opinion of them cannot disagree.
    //
    // **The BAND is not hidden with them.** `compute_card_rects` reserves
    // `pad + ctl` at the bottom of all four doors whatever this says, so
    // `content_bottom` stays one expression with one meaning.
    //
    // **The band is no longer empty where these three are gone.** This said
    // "an empty bar is indistinguishable from the window ground it is painted
    // on", which held for one day: design §6.4's service line
    // (`IDC_SERVICE_LINE`) is chrome, is drawn on all four doors, and on
    // System and About has the whole bar to itself.
    let bar = command_bar_shown(page);
    for id in [IDC_OPENFILE, IDC_CLOSE, IDC_APPLY] {
        if let Ok(h) = GetDlgItem(Some(hwnd), id) {
            show(h, bar);
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
/// | `E` | R**e**vert | | |
///
/// **Mnemonic uniqueness is maintained by hand.** There is no test for it,
/// so verify by inspection before adding new captions.
///
/// `Record` and `Revert` are why this table has an awkward corner. `R` is
/// `Reload`'s, `S` is Save's, `T` is the Ctrl chip's, `O` is Open's and `C`
/// is Close's -- so between them the two captions have exactly two letters
/// left, `d` and `e`, and taking the obvious `e` for `Record` would leave
/// the other with nothing. Hence `Recor&d` and `R&evert` rather than the
/// other way round.
///
/// **The 2026-08-15 rename of `Reset` to `Revert` deliberately kept `e`**,
/// even though `Revert` has a free `v` that `Reset` did not: this table has
/// no test, so the safe rename is the one that changes no key at all. See
/// `cap::REVERT`.
///
/// `Stop`, which `Record` reads while a capture is armed, deliberately
/// carries NO mnemonic and needs none: while armed the `WH_KEYBOARD_LL` hook
/// swallows every keystroke before it reaches a queue, so no `Alt`-anything
/// can reach this window at all. Esc, the mouse, losing focus and the
/// watchdog are the ways out.
///
/// `Remove` cannot take `R` because `Reload` has it, and `Reload` is the
/// one that appears without warning -- a banner the user did not ask for is
/// the worse place to make someone hunt for a letter.
///
/// **CORRECTED 2026-08-15: the paragraph here used to close on the editor's
/// two field labels** (`App`, `Shortcut`) carrying no mnemonic, because a
/// STATIC's mnemonic only moves focus to the next control in tab order.
/// Design §3.1 deleted both labels, so the reasoning has nothing left to be
/// about -- but it is still the rule for `IDC_LBL_HOLD` and `IDC_LBL_TAP`,
/// which is why it is restated rather than dropped: a label that names the
/// control after it buys nothing by holding a letter for it.
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
    /// Design §3.2's third group. The mnemonic is `p`, which the table above
    /// leaves free -- `W`, `S`, `C`, `A`, `R` and `t` are all spoken for, and
    /// `p` is the first letter in the sentence that is not.
    ///
    /// **The drawing sets `[Caps]` and `[Ctrl] [Win] [Alt]` as real keycaps
    /// inside the sentence, and this is plain text.** No painter in this
    /// window interleaves text runs with caps; building one blind would put a
    /// third unverifiable thing on this door. The words are the drawing's.
    pub const CAPS_SHORTHAND: &str = "Write shortcuts as Ca&ps instead of Ctrl + Win + Alt";
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
    ///
    /// **`Revert`, not `Reset`, since 2026-08-15** (design 3.1). `Reset`
    /// named the mechanism -- it clears a field -- and `Revert` names the
    /// effect the user wanted, which is rule 1 read for a button: a caption is
    /// a name for what happens, not for how.
    ///
    /// **It keeps `e`, and that is a decision rather than an oversight.**
    /// Design 10 forbids new `Alt` mnemonics until a uniqueness `#[test]`
    /// lands, and the collision table above is hand-maintained with no test at
    /// all. `Re&vert` would have claimed `v`, which is free -- and free is not
    /// the test; the test is whether anything checks. `R&evert` reuses the
    /// letter this button already had, so the table's row changes its caption
    /// and not its key, and the awkward-corner paragraph above still holds
    /// exactly as written: `d` for Record, `e` for this.
    pub const RECORD: &str = "Recor&d";
    pub const REVERT: &str = "R&evert";
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
    /// The App combo's placeholder, and design §3.1's replacement for the
    /// `App` label that used to sit beside it (`IDC_LBL_APP`, retired).
    ///
    /// **A cue banner, not a caption**, which is what makes the swap a
    /// deletion rather than a move: it costs no width at all, and it is gone
    /// the moment the field has content -- so the word is on screen exactly
    /// while the field cannot say what it is for itself. The filter box has
    /// worked this way since Task 9 and this is the same trade, one card down.
    ///
    /// It rides on `CB_SETCUEBANNER`, which needs an edit child, which is why
    /// the key list beside it gets no equivalent: a `CBS_DROPDOWNLIST` has no
    /// edit control, so §3.1 places it "at the end of the modifier run, where a
    /// key goes" and lets the position carry the meaning.
    ///
    /// **Unverified, and it is the focus case rather than the empty case.**
    /// `EM_SETCUEBANNER` takes an lParam saying whether to keep the cue while
    /// the control has focus; `CB_SETCUEBANNER` takes no such flag and does not
    /// document which it picks. So "empty" is certain and "empty AND unfocused"
    /// is possible. Either reading satisfies §3.1 -- a focused App field is one
    /// the user is already typing into -- and nothing on this host can display
    /// the window to check. Gate G1's run would see it in passing.
    ///
    /// No `&`: a cue banner is not a caption and owns no mnemonic. It is drawn
    /// verbatim, so an `&` here would appear as one.
    pub const APP_CUE: &str = "App";
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
    /// **These four are now the ONLY `Shortcuts` on screen.** The word used to
    /// appear twice on the Shortcuts page: this pill, and `IDC_LBL_SECTION`'s
    /// Subtitle heading two lines below it. The argument for keeping both was
    /// that they are two controls that happen to be named the same thing --
    /// the heading names the card, the pill names the door -- which is true
    /// and was not the question. The design's drawing and the mock-up both
    /// open that card with the filter and two buttons, and a reader looking at
    /// a heading directly under an identically captioned pill learns nothing
    /// from the second one. 1020 is retired; see `ids.rs`.
    pub const TAB_SHORTCUTS: &str = "Shortcuts";
    pub const TAB_KEYBOARD: &str = "Keyboard";
    pub const TAB_SYSTEM: &str = "System";
    pub const TAB_ABOUT: &str = "About";
    // `PLACEHOLDER` ("Nothing here yet.") stood here until 2026-08-15 and is
    // gone with the last control that read it. It was two controls' caption,
    // then one, then none: design §3.3's rows took System's waiting line and
    // §3.4's took About's the next day. Nothing is waiting any more, so the
    // constant has nothing left to be about.

    /// The System page's five rows (design §3.3), in drawing order.
    ///
    /// **Not one of these carries an `&`, and it is the same counting
    /// argument the tab pills lost.** The collision table above has
    /// `A M U C O S E R K T W L D` spoken for; `Pause shortcuts` could take
    /// only `p`, `h` or one of the two `s`s, `Start with Windows` overlaps it
    /// on every letter it has, `Dark mode` has only `k` free, and the four
    /// glyph buttons have no letters at all. Rather than spend the four free
    /// letters left in the alphabet on a page whose keyboard route is already
    /// `Ctrl+3` plus Tab, the page carries none -- which is also design §10's
    /// standing rule: no new `Alt` mnemonics until a uniqueness `#[test]`
    /// lands.
    ///
    /// `Pause shortcuts`, not the tray's `Pause hotkeys`: the window's own
    /// word for a row of the table is `shortcut` everywhere else on screen
    /// (the file is a shortcuts TOML, the pill says `Shortcuts`), and two
    /// words for one thing costs the reader a lookup. The tray keeps its
    /// wording -- a menu the user reads once has no such context to be
    /// consistent with.
    pub const PAUSE: &str = "Pause shortcuts";
    pub const AUTOSTART: &str = "Start with Windows";
    /// The tray's own reload, which re-reads the file and re-registers every
    /// hotkey. NOT the banner's `&Reload` (`cap::RELOAD`), which discards the
    /// window's unsaved edits -- see `IDC_SYS_RELOAD` in `ids.rs`. The two
    /// captions are the same word because they are the same word to the
    /// reader; they are never on screen together, since the banner is
    /// `BANNER_PAGE`-only.
    pub const SYS_RELOAD: &str = "Reload";
    pub const DARK: &str = "Dark mode";
    pub const TRANSPARENCY: &str = "Window transparency";

    /// The two glyph buttons every file row carries.
    ///
    /// **Glyphs, not `Open` and `Show in folder`**, and the filename beside
    /// them is why: design §3.3 makes the file's own name the row's label, so
    /// two verbs would be the only words on a line that already says what it
    /// is about. Each carries a tooltip with the full sentence.
    ///
    /// **ASCII would be better and there is no ASCII for this.** Every other
    /// display string in this window is ASCII because a face that lacks a
    /// glyph draws a box -- `serve --log`'s em-dash came back as `?"` once.
    /// These two are `U+2197 NORTH EAST ARROW` and `U+25A4 SQUARE WITH
    /// HORIZONTAL FILL`, both in Segoe UI's coverage on every Windows 10/11
    /// build, and both are what the mock-up draws. They are the FIRST
    /// non-ASCII the window puts on screen, so if a box appears anywhere it
    /// will be here; `settings_probe` reads the captions back, which is the
    /// cheapest check available without a screenshot.
    pub const OPEN_GLYPH: &str = "\u{2197}";
    pub const SHOW_GLYPH: &str = "\u{25A4}";
    /// The four glyph buttons' tooltips -- the words the buttons do not
    /// spend width on. `%s` is not a thing here: each is written out per row,
    /// because "Open the config file" and "Open the log file" differ by more
    /// than a noun once a screen reader is reading them out.
    pub const TIP_CONFIG_OPEN: &str = "Open the config file";
    pub const TIP_CONFIG_SHOW: &str = "Show the config file in Explorer";
    pub const TIP_LOG_OPEN: &str = "Open the log file";
    pub const TIP_LOG_SHOW: &str = "Show the log file in Explorer";

    /// The About page (design §3.4).
    ///
    /// **The letter, not a picture.** `beckon.ico` is embedded in both
    /// binaries and `LoadImageW` could draw it here -- and the mock-up draws a
    /// `b` in a rounded accent square, which is what the drawing decides.
    /// Loading the icon would also need a fallback for the build that has no
    /// resource, i.e. two paths where the drawing asks for one.
    pub const MARK: &str = "b";
    /// **No `&` on any of the three labels**, and unlike the tab pills this
    /// is not a counting argument -- free letters exist (`b`, `i`, `n` among
    /// them). Two reasons that do not depend on the alphabet:
    ///
    /// - **A label's mnemonic presses nothing.** `Alt+B` on a STATIC moves
    ///   focus to the next control in tab order, which here is a value the
    ///   user cannot edit and then a copy button they did not aim at. That is
    ///   the rule the editor's own two labels carried and `IDC_LBL_HOLD` /
    ///   `IDC_LBL_TAP` still carry: a label that names the control after it
    ///   buys nothing by holding a letter for it.
    /// - **Design §10 forbids new `Alt` mnemonics until a uniqueness
    ///   `#[test]` lands**, and the collision table above is still maintained
    ///   by hand with nothing checking it.
    ///
    /// `Licence`, not `License`: the mock-up spells it this way, and it is
    /// the row's LABEL rather than the identifier under it -- the value stays
    /// `MIT OR Apache-2.0`, which is Cargo's string and not a word this
    /// window gets to spell.
    pub const ABOUT_BUILD: &str = "Build";
    pub const ABOUT_LOCATION: &str = "Location";
    /// The three copy buttons' one caption.
    ///
    /// **`U+29C9 TWO JOINED SQUARES`, the third non-ASCII string this window
    /// draws and the least certain of the three.** `OPEN_GLYPH` (U+2197) and
    /// `SHOW_GLYPH` (U+25A4) were argued to be in Segoe UI's coverage on every
    /// Windows 10/11 build; this one is a mathematical symbol, so the face
    /// that answers for it is more likely to be Segoe UI Symbol through font
    /// linking than Segoe UI itself. It is what design §3.4's drawing uses,
    /// and the failure mode is the one every glyph here has -- a box --
    /// which `settings_probe` reads back by caption, the same check the other
    /// two get and the cheapest one available without a screenshot.
    ///
    /// The alternative was the word `Copy` three times, and it loses to rule
    /// 1: the row already carries a label AND a value, so a third word on it
    /// would be the only one naming a mechanism rather than a fact.
    pub const COPY_GLYPH: &str = "\u{29C9}";
    /// The three copy tooltips -- the words the buttons do not spend width
    /// on. Written out per row rather than templated, for the reason the
    /// System page's four are: read aloud, "Copy the build" and "Copy the
    /// path" differ by more than a noun.
    ///
    /// `TIP_LOCATION_COPY` says **path**, and the button really does copy the
    /// bare path -- not the string on screen, which may carry a verdict and is
    /// shortened by `SS_PATHELLIPSIS`. See `beckon_core::settings::copy_text`.
    pub const TIP_BUILD_COPY: &str = "Copy the build identifier";
    pub const TIP_LOCATION_COPY: &str = "Copy the full path";
    pub const TIP_LICENCE_COPY: &str = "Copy the licence";
    /// The update check's own row (Task 9). No `&` on either: `Check now`'s
    /// `c` is `IDC_CAPS`' on the Keyboard page, not this one, but design
    /// §10's standing rule applies here exactly as it does to the three links
    /// below -- no new `Alt` mnemonic without a uniqueness `#[test]`, and
    /// there is no test.
    pub const CHECK_NOW: &str = "Check now";
    /// **A fourth "go to the releases page" caption, deliberately beside the
    /// third (`ABOUT_RELEASES`) rather than replacing it.** The two answer
    /// different questions: `ABOUT_RELEASES` in the links row is always
    /// there, for a reader who came to this page to look something up; this
    /// one is beside a FAILED check's own status line, for a reader who did
    /// not and has no reason to scroll down -- the macOS twin ships the same
    /// duplication for the same reason (`about.rs`'s own doc there). Longer
    /// than `Releases` on purpose: it names the destination rather than
    /// assuming the status line above it was read.
    /// The upgrade command's own copy button, beside `Copy the build
    /// identifier` and its two siblings above -- same glyph (`COPY_GLYPH`),
    /// different tooltip, because what it copies is a different thing:
    /// `cmd.copy`, the bare upgrade command, never the annotated `cmd.shown`
    /// a caveat may be glued to. See `beckon_core::settings::Field::UpdateCommand`.
    pub const TIP_UPDATE_COPY: &str = "Copy the upgrade command";
    /// The three links, in drawing order. No `&` on any of them: `g` and `b`
    /// are genuinely free, and design §10's standing rule is the reason
    /// anyway -- no new `Alt` mnemonics until the collision table above has a
    /// uniqueness `#[test]` behind it. `Releases` and `Report a bug` would
    /// also have to split `r`, which `Reload` already owns.
    ///
    /// They are the only captions in this window that name something outside
    /// it, which is why none of them is a verb: `GitHub` and `Releases` are
    /// places, and `Report a bug` is the one that is an errand.
    pub const ABOUT_GITHUB: &str = "GitHub";
    pub const ABOUT_RELEASES: &str = "Releases";
    pub const ABOUT_BUG: &str = "Report a bug";
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
/// **The titles are not drawn anywhere.** `IDC_LIST` carries
/// `LVS_NOCOLUMNHEADER` since 2026-08-15 (design §3.1), so `"App"` and
/// `"Shortcut"` reach `LVM_INSERTCOLUMNW` and stop there. They stay because a
/// column's text is its name to anything that asks the control about a
/// subitem, and because the array is still the one place the two columns are
/// declared -- the ALIGNMENT below is live and is what puts the chord flush
/// right against the app name.
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
// **680x600 since Task 8; 760x600 from the 2026-08-13 compaction pass until
// then.** The 900x740 derivation that stood here was for a window with 26 px
// rows and 16 px padding and was already marked superseded; a full derivation
// of a window that does not exist is worse than none, so it is gone rather
// than annotated again.
//
// What replaced it as evidence is better than a table: the window was built
// and run on a14 at 144 DPI and measured **1140 x 900** -- exactly 760 x 600
// scaled by 1.5 -- with all eight list rows present and no scroll bar.
//
// **NEITHER half of that run describes this window any more**, and it is kept
// because it is a record of an event rather than a table to check against. The
// tab strip's band (`tok::TABSTRIP_H`, added below) costs the list 34 px, so
// at 600 the cap lands at 178 against a `want` of 197 -- seven rows, not
// eight. And the width is 680 now, so a fresh run at 144 DPI has to come back
// **1020 x 900**. What survives untouched is the PROPERTY that run
// established, which is the only part a later run can re-check: the window
// comes up at exactly `scale(WINDOW_WIDTH, dpi)` by `scale(WINDOW_HEIGHT,
// dpi)`. `examples/settings_probe.rs` re-asserts it against its own
// transcribed copy of both constants, and `ids::geometry_matches_the_probe`
// is what keeps that copy from going stale -- it failed on this commit, which
// is the net working.
//
// **What 680 does to the widths, derived here rather than carried over from
// the spec.** A card's interior is `w - 2*tok::PAD - 2*tok::CARD_PAD` = **638**
// px at 96 DPI, and that one number is `cw1`, `ed_w` and `kb_w` alike
// (`layout.rs`) -- the three cards share a width because they share both
// insets. `ed_w` was `grp_w` and its contents sat one `tok::GAP` inside it,
// clearance for a group box's frame; design §3.1 took the caption and the
// frame with it on 2026-08-15, so the editor card's contents now start at the
// same x as the list's.
//
// Inside the list that leaves `col_app` at **421**, not the design's ~438.
// `layout` subtracts `GetSystemMetricsForDpi(SM_CXVSCROLL, dpi)` from the
// list's own client width whether or not a scroll bar is showing (that
// subtraction is what makes a clipped column structurally impossible), so
// `col_app` is `638 - 17 - 200` with the list unscrolled and `638 - 34 - 200`
// = **404** with a scroll bar actually up. The design's figure is `638 - 200`
// and forgets both. 17 is `SM_CXVSCROLL` at 96 DPI on the default theme --
// the same figure `layout`'s own "34 px gutter at 96 DPI, 52 at 150 %" note
// is computed from -- and it is a system metric rather than a constant, which
// is why `settings_probe` now prints it by name beside the frame metrics.
//
// **Which terms compose the height, in order** -- the part of the old block
// that was worth keeping, restated against the shipped tokens. This is a map
// of what a token change spends, not a claim about the total.
//
// **It is PER PAGE since 2026-08-15**, because the stack is
// (`compute_card_rects`): the keyboard card used to be bottom-anchored and to
// reserve its height on every page, so one list served all four doors. Now
// every page pays the chrome and then only for what it draws.
//
//   -- chrome, every page --------------------------------------------
//   title bar (chrome::TITLEBAR_H)                     34
//   tab strip (tok::TABSTRIP_H)                        36
//   gap_card, above the first card                      8
//   gap_card, below the last card                       8
//   command bar (CTL, not a card)                      26
//   pad                                                10
//                                                     ---
//                                                     122
//
//   -- Shortcuts -----------------------------------------------------
//   card 0  banner -- NO height unless it is up      0/48
//           (plus one gap_card, 8, when it is)
//   card 1  2*CARD_PAD, head CTL, GAP, list_h    54 + list
//   gap_card                                            8
//   card 2  editor: 2*CARD_PAD, 2*CTL, 2*GAP,
//           notes_height, GAP                    92 + notes
//
//   -- Keyboard ------------------------------------------------------
//   card 3  2*CARD_PAD, caption s(24), CTL, GAP         78
//
//   -- System, About -------------------------------------------------
//   one STATIC at the content origin, no card           26
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
// does not scale by 1.5 between DPIs and no total here is unconditional.
//
// **The constant is no longer answerable for a row count, and that is the
// 2026-08-15 change** (design 4, `tok::ROWS` deleted). The list used to be
// capped at eight rows, so the interesting question about 600 was whether it
// was tall enough to show all eight; it now takes whatever the page leaves,
// so the question is simply how many rows 600 buys. Re-derived from
// `compute_card_rects` for this comment, at 96 DPI with a 16 px Caption line
// (`notes_height` 36):
//
//   banner down:  list room = h - 276 - notes_h = **288**  -> 13 rows (286)
//   banner up:    list room = h - 332 - notes_h = **232**  -> 10 rows (220)
//
// The remainder in each case is the whole-row snap, and it lands between the
// editor card and the command bar: 2 px and 12 px respectively.
//
// **It was seven rows and a 178 px cap the day before**, with an 86 px
// keyboard-card reservation and a 24 px editor caption above it. Those two,
// plus the header band's ~21, are the 110 px the list gained. The window is
// the same size; the page stopped spending its height on things it does not
// draw.
//
// **600 is left where it is, deliberately.** Nothing in the four changes
// forces it, and the one number that would argue for a shorter window -- the
// mock-up's own page, which is roughly 436 px tall -- is drawn WITHOUT a
// command bar, because design 6 replaces it with auto-save. That is 44 px of
// chrome this window still pays (`CTL` + `pad` + a `gap_card`) and a decision
// that belongs to whoever builds 6.
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
const WINDOW_WIDTH: i32 = 680;
/// **600 -> 500 on 2026-08-15, and 600 was never derived for this window.**
///
/// Design §2's table reads "680 × 600 at 100 % DPI (was 760 × 600)" and then
/// spends a paragraph deriving the WIDTH -- the 638 px of card interior, the
/// 200 px shortcut column, the ~438 left for an app name. The height is
/// carried over from the pre-Four-Doors window unexamined, and it is the
/// larger half of the void the System and About doors shipped with.
///
/// **The mock-up is 497 px tall.** Measured rather than read off the drawing's
/// own caption, which says 600: rendered at its stated 680 px in Chrome, the
/// `.win` element is 496.9 -- 34 title bar, 47 strip, 336-374 page, 47 command
/// bar -- and its System card fills its page to within 10 px. The shipped
/// window gave that same page 478 px and the card needed 254, so 224 px
/// became ground. 500 is that drawing, rounded to the ten.
///
/// **What it costs, and why that is the right side to lose on.** At 96 DPI
/// with the banner down the list gets `500 - 276 - notes_h(36)` = 188 px, which
/// snaps to **eight** whole rows where 600 gave thirteen. Eight is more than
/// the mock-up draws (six). The window is resizable and the list is the ONE
/// thing that flexes (`compute_card_rects`' `list_h`), so a taller window is
/// one drag away and every pixel of it reaches the list -- whereas no drag
/// helps the three doors whose cards are fixed, because their content cannot
/// grow into the space. The default therefore serves the doors that cannot
/// help themselves.
///
/// Re-derive BOTH ends before moving this again: the table under `MIN_HEIGHT`
/// is the arithmetic, and `layout.rs`'s `the_fixed_doors_fit_above_the_command_bar`
/// is what fails if this and the floor stop agreeing.
///
/// **592 -> 546 the same day, when the About page was compacted.** The
/// `Licence` row went -- one `pitch()` = 46 px of card -- so the floor this
/// constant sits +20 above came down with it. `MIN_HEIGHT`'s own table
/// carries the re-derivation. It gives back HALF of what the update check
/// cost and no more, which is the honest arithmetic: one row removed against
/// the two that were added.
///
/// **500 -> 592 on 2026-08-25 (Task 9)**, following its own instruction: the
/// About card grew two rows, `MIN_HEIGHT`'s own "CORRECTED 2026-08-25" note
/// carries the re-derivation, and this constant keeps the same +20 margin
/// over the floor it always had (480 + 20 then, 572 + 20 now). The mock-up
/// paragraph above is left as it was written -- about a page this door did
/// not carry yet -- rather than restated for content the mock-up never drew.
const WINDOW_HEIGHT: i32 = 546;

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
/// (`compute_card_rects`' `y`), which left the floor two rows rather than
/// four — and design §4 makes the list **short and scrolling** instead of a
/// list the window grows to fit. Once it scrolls, a floor's job stops being
/// "enough rows to see context" and becomes "enough rows to see that it is a
/// list". Two rows plus a scrollbar meets that; one row does not. So the
/// constant keeps its meaning while its derivation changes, and it is
/// withdrawn here in writing rather than left to be discovered as a window
/// that no longer does what its own comment claims.
///
/// **The withdrawal stands even though the arithmetic has swung back past
/// it.** Since 2026-08-15 the floor buys **eight** rows, not four — the
/// page-dependent stack, the deleted editor caption and the deleted column
/// header returned 110 px, and `tok::ROWS` going means all of it reaches the
/// list. That is the constant clearing an old promise by a wide margin, which
/// is not the same as the promise being back: `MIN_HEIGHT` is answerable for
/// what it GUARANTEES, and design §4's scrolling list is still the reason a
/// row count is not the thing to guarantee. Restoring the four-row wording
/// would re-couple this constant to a number the design deliberately let go.
///
/// **RE-DERIVED AGAIN 2026-08-15, and this time the SUBJECT changed.** Every
/// version of this block before it solved the SHORTCUTS page for a row count.
/// That is no longer the binding constraint, and pretending otherwise is what
/// let the System and About doors ship with a third of the window empty: their
/// cards are sized by their own contents, so a floor derived from the list says
/// nothing at all about whether they fit.
///
/// **The floor is now the tallest FIXED page.** Of the four doors, three have
/// no flexing element -- Keyboard, System and About are one card each, as tall
/// as their rows. The list gives room up before anything else moves
/// (`compute_card_rects`), so the Shortcuts door cannot be the one that runs
/// out of room first; one of the other three is, and it is About.
///
/// ```text
///   At 96 DPI with the shipped tokens. The banner is a Shortcuts-only card
///   (`banner_shown`), so on the fixed doors `content_top` is the strip's
///   bottom plus one gap and nothing else:
///
///     content_top    = TITLEBAR_H + TABSTRIP_H + GAP_CARD   = 78
///     bar_y          = h - PAD - CTL                        = h - 36
///     content_bottom = bar_y - GAP_CARD                     = h - 44
///
///   Card heights come from `beckon_core::page_plan`, which is where the
///   arithmetic now lives and where it is TESTED (all three CI jobs):
///
///     sys_card_h     = 2*CARD_PAD + system_plan.content_h
///                    = 22 + 304                             = 326
///     about_card_h   = 2*CARD_PAD + about_plan.content_h
///                    = 22 + (286 + disclosure_h)
///
///   `disclosure_h` is the ONE measured term on either page -- a DT_CALCRECT
///   of `HOOK_DISCLOSURE` -- so About is the door whose height moves with the
///   font. At 96 DPI in a 680 px window it wraps to two lines (32 px), which
///   is what the a14 photograph shows:
///
///     about_card_h   2 lines (32) = 340    3 lines (48) = 356
///                    4 lines (64) = 372
///
///   Fitting is `content_top + card_h <= content_bottom`, i.e.
///   `h >= 122 + card_h`:
///
///     System                      h >= 448
///     About, two-line disclosure  h >= 462
///     About, three lines          h >= 478   <-- the floor
///     About, four lines           h >= 494
/// ```
///
/// **480, from the three-line row, and the choice of row is the judgement.**
/// Two lines is what the shipped string measures at every DPI the window is
/// drawn at, so a floor at 462 would fit the window as it is; three is one
/// line of headroom for a larger system font, and 480 clears it. Four lines
/// does not fit, and the honest statement of what happens then is that the
/// About card runs 14 px past `content_bottom` -- into the command bar's band,
/// which **on that door is empty ground since the store split**
/// (`command_bar_shown`). So the overflow state has nothing to collide with;
/// it is the card sitting closer to the bottom edge than intended, not a
/// control drawn over another. That is why the floor is set from the
/// three-line row rather than the four.
///
/// **CORRECTED 2026-08-25 (Task 9): both constants moved again, and this time
/// About is what moved them.** The update check gave the About card two more
/// fixed rows -- `beckon_core::page_plan::AboutPlan`'s `update` and `command`
/// -- and `about_plan`'s own tests now pin its `content_h` at 410 (two-line
/// disclosure) / 426 (three-line) where this table's `286 + disclosure_h` had
/// it at 318 / 334. Redoing exactly the arithmetic above with the new number:
///
/// ```text
///     about_card_h   2 lines (32) = 432    3 lines (48) = 448
///
///   Fitting is still `h >= 122 + card_h`:
///
///     About, two-line disclosure  h >= 554
///     About, three lines          h >= 570   <-- the floor Task 9 set
/// ```
///
/// **572, from the three-line row, the same +2 margin the old 480 carried
/// over its own 478.** `MIN_HEIGHT` moved from 480 to 572 and `WINDOW_HEIGHT`
/// kept its old +20 margin over the floor, moving from 500 to 592. Every
/// number in the two paragraphs above this one (`462`, `478`, `494`, `168`,
/// `112`, seven rows, five rows) describes the window as it stood before this
/// task and is superseded here, not corrected in place -- this file's own
/// convention, followed rather than broken.
///
/// **RE-DERIVED 2026-08-25, second pass the same day: the About page was
/// compacted and this came back down.** The `Licence` row went -- see
/// `Field`'s own doc for why it was earning nothing -- so `about_plan`'s
/// `content_h` fell from 410 / 426 to **364 / 380**. The same arithmetic
/// again, with no other term touched:
///
/// ```text
///     about_card_h   2 lines (32) = 386    3 lines (48) = 402
///
///   Fitting is still `h >= 122 + card_h`:
///
///     About, two-line disclosure  h >= 508
///     About, three lines          h >= 524   <-- the floor now
/// ```
///
/// **526, three-line row plus the same +2 margin**, and `WINDOW_HEIGHT`
/// keeps its +20: **546**. Half of Task 9's +92 comes back and no more,
/// because one row was removed against the two it added.
///
/// **What this improves but does not fix.** System's card is still 326 and
/// still did not grow, so ground below it goes from Task 9's **144 px** to
/// **98** -- better than the 144 that shipped, still above the 60 px both
/// doors were once held under, and still above the **52** it sat at before
/// the update check existed. Shrinking the rest would mean compressing the
/// update check's two rows below the page's ordinary row rhythm or inventing
/// content for System to fill the gap with; both remain design calls outside
/// a floor constant's authority.
///
/// **The high-DPI floor improves with it, and is still worth watching.**
/// `ptMinTrackSize.y` is `scale(MIN_HEIGHT, dpi)`, so at 200 % the floor is
/// now `526 * 2 = 1052` physical px against a 1080p work area of roughly
/// 1040 -- marginal rather than the clear 1144 it was, and comfortable again
/// at 175 % (920). Simulated, not seen: nothing here can display the window.
///
/// **The Shortcuts list is now a consequence rather than the derivation**, and
/// it lands well: at 572 with the banner down `avail` is `572 - 276 - 36` =
/// 260, which snaps to **eleven** whole rows (see the CORRECTED note above for
/// why these numbers are not the ones two paragraphs up), and with the banner
/// up `572 - 332 - 36` = 204, or **nine**. The withdrawn four-row guarantee
/// above is cleared at the floor by five rows now, not one, without the
/// constant being answerable for it.
///
/// **"Nothing forces a move, so it does not move" is FALSIFIED, and it was the
/// previous paragraph here.** It reasoned that the standard ("enough rows to
/// see that it is a list") was met at 412 and at 560 alike, so nothing could
/// choose between them -- true, and beside the point, because the standard it
/// consulted was about the list. What forces the move is `WINDOW_HEIGHT`:
/// 500 cannot be the default size of a window whose minimum is 560. The old
/// note ends with the numbers it offered whoever lowered it -- "412 for two
/// rows, 456 for four, 500 for six" -- and those were banner-UP figures for
/// the old rhythm; the table above supersedes them.
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
/// The floor's margin is now **148 px** — `avail = 560 - 332 - 36 = 192`
/// against the 44 two rows need — where it was 17. `notes_h`'s honest error
/// still eats into it and is still the only term here that is not a token:
/// `notes_h = 2L + 4`, so every extra pixel of Caption line `L` costs the list
/// two. It used to take the second row at `L = 25`; it would now have to reach
/// `L = 90` to do that, which is not a font size. **Nothing on the machine
/// this was derived on can display the window**, and
/// `examples/settings_probe.rs`'s `measure_geometry` already prints
/// `GetClientRect` beside `GetWindowRect` with a verdict, so the reading
/// above costs one a14 run to confirm rather than a new probe.
///
/// The row figure is `list_row_height`'s own 96-DPI fallback (`tok::ROW_H`).
/// It is the honest number to derive from: comctl32 picks the real one from
/// the live font at the live DPI, which is exactly why it is not a token here.
/// **The header's 21 px partner is gone** — `list_header_height` was deleted
/// with the column headers on 2026-08-15, and a table that still subtracted
/// its fallback would be paying for a control the window does not create.
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
/// banner down the stack starts 56 px higher, so the list's room is
/// `h - 276 - notes_h` instead of `h - 332 - notes_h`, and there is no cap:
/// whatever is left goes to the list, snapped down to whole rows.
///
/// - at the floor (client 480, which IS `MIN_HEIGHT` — see above), banner up:
///   112 px, **five** whole rows and 2 px over;
/// - at the floor, banner down: 168 px, **seven** rows and 14 px over;
/// - at `WINDOW_HEIGHT` (client 500, likewise), banner down: 188 px,
///   **eight** rows and 12 px over.
///
/// **RE-DERIVED 2026-08-15 (second pass, the four-doors visual gaps).** The
/// bullets read 192 / 248 / 288 — eight, eleven and thirteen rows — against a
/// 560 floor and a 600 default. Only the two constants moved; the arithmetic
/// above them is untouched, because the editor card and the banner are spaced
/// with `tok::GAP` and the row rhythm that changed (`tok::ROW_GAP`) belongs to
/// the System and About cards alone.
///
/// **The pass before that, the same day**, moved them from 82 / 138 / 178 —
/// two rows, five, seven — because the page reserved 86 px for a keyboard card
/// it did not draw, the editor card reserved 24 px for a caption, the list
/// reserved ~21 px for a column header, and `tok::ROWS` capped the result at
/// eight anyway. All four went in one pass (design §3.1 and §4).
///
/// **CORRECTED 2026-08-14** — the same three bullets moved twice in one day
/// before that, and the record is worth keeping because it is the reason every
/// figure here is spelled as arithmetic. They read `client 552`, `client 592`,
/// `108`, `164` and `204` while an 8 px bottom frame was subtracted from the
/// constant beside each; that subtraction described `chrome::nccalcsize` as it
/// was before `c523e8e` (2026-08-13), and removing it (`0098457`) made the
/// client heights the constants themselves and every cap 8 px larger —
/// 116 / 172 / 212. The tab strip then took 34 off all three.
///
/// The remainder in each bullet is the whole-row snap and it lands between the
/// editor card and the command bar — a margin of at most `row_h - 1`, which is
/// a property of these particular numbers rather than anything designed in. A
/// future change to `notes_height`, `card2_h` or the row fallback moves it, so
/// re-check it by the same hand trace rather than assuming it survives.
/// Simulated, not seen: nothing on the machine this was written on can display
/// the window.
const MIN_WIDTH: i32 = 660;
const MIN_HEIGHT: i32 = 526;

/// The default size has to be one the floor allows, or `WM_GETMINMAXINFO`
/// resizes the window in the same breath it is created.
///
/// A `const` block rather than a `#[test]`, and clippy is what settled it:
/// both sides are constants, so `assert!` in a test is
/// `clippy::assertions_on_constants` and would not have compiled under
/// `-D warnings`. Here the same comparison fails the BUILD instead of a test
/// run, on every job that compiles this crate.
const _: () = {
    assert!(WINDOW_HEIGHT >= MIN_HEIGHT);
    assert!(WINDOW_WIDTH >= MIN_WIDTH);
};

/// §B.3's type roles. The seven roles — Title, Subtitle, BodyStrong, Body,
/// Caption, Keycap, Chrome — map to five visual levels (Title, Subtitle, Body,
/// Caption/Keycap, Chrome). Keycap serves keycap rendering in the editor
/// strip and shortcut list; Title and Chrome serve the client-drawn title
/// bar `chrome::paint` draws (Task 7).
#[derive(Clone, Copy)]
enum Role {
    /// The title-bar app name. Read by `chrome::paint`.
    Title,
    /// 18 px semibold. **It was unconstructed for one day** -- design §3.1
    /// deleted its only reader, the `Shortcuts` card heading
    /// (`IDC_LBL_SECTION`), and the role was kept behind an `#[allow(dead_code)]`
    /// naming About's name row as the next control that would want it. Design
    /// §3.4 built that row on 2026-08-15, so the `allow` is gone and the
    /// prediction is spent: `IDC_ABOUT_NAME` draws `beckon 0.9.3` in this, and
    /// `IDC_ABOUT_MARK` draws the letter in the tile beside it.
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
        // **`Role::Subtitle` has a reader again since 2026-08-15**, and it is
        // the one the previous version of this comment predicted: About's name
        // row. That comment read "No arm returns `Role::Subtitle` ... About's
        // own name row (`ABOUT_NAME`, 1101) is the next control that will want
        // an 18 px semibold", written the day `IDC_LBL_SECTION` was deleted;
        // the role and its font were kept for it and are now spent on it. The
        // `#[allow(dead_code)]` on the variant went with this arm.
        //
        // **`IDC_ABOUT_MARK` shares it, and that is a size decision rather
        // than a semantic one.** The mock-up draws a 48 px tile with a 28 px
        // letter -- a ratio of 0.58 -- and there is no 28 px face in this
        // window: §B.3 names seven roles and the largest is this 18 px
        // semibold. Inventing an eighth for one letter is a change to the type
        // scale, so the TILE shrinks instead (`paint::mark`'s `MARK_D`, 36 px,
        // ratio 0.5) and the scale stays closed.
        IDC_ABOUT_NAME | IDC_ABOUT_MARK => Role::Subtitle,
        //
        // The one card caption left, and the Save caption. `IDC_GRP_KEYBOARD`
        // was reclassed from `BS_GROUPBOX` to a plain caption `STATIC` in Task
        // 8's review pass (see `child`'s creation call): a themed group-box
        // frame nested inside the new rounded `card()` background drew as two
        // frames around one set of controls, and the fix is a coordinate shift
        // plus a control-class change, not a renumbering.
        // `IDC_APPLY` reads its font through this same mapping even though
        // it is custom-drawn -- `paint::button` asks the button for its own
        // `WM_GETFONT` rather than picking a role directly, so this arm is
        // the only place its weight is decided.
        //
        // **`IDC_GRP_EDITOR` shared this arm until 2026-08-15**, when design
        // §3.1 deleted the editor card's caption outright. Nothing replaced it:
        // the row being edited is the row the list above highlights, so the
        // card's first line is now the App field itself.
        //
        // The ListView's column headers used to be named here as the one text
        // this mapping cannot reach -- a comctl32-owned Header control, never a
        // child of `hwnd`. **There is no Header any more** (`LVS_NOCOLUMNHEADER`,
        // same design bullet), so `role_of` now covers every string in the
        // window that is not painted by `paint.rs` directly.
        IDC_APPLY => Role::BodyStrong,
        // Secondary prose, at Caption size. The banner is deliberately NOT
        // here: it announces that the file moved under us, which is the
        // least appropriate text in the window to shrink. `IDC_LBL_COUNT`
        // used to share this arm -- B drew the count small and grey beside a
        // Subtitle heading, and one STATIC has one font, which was the whole
        // reason it was a second control. It is retired (design 2 moved the
        // count to the pill), so `IDC_NOTES` is alone here now.
        //
        // **The System page's three VALUE slots joined on 2026-08-15**
        // (design §3.3, rule 3: "a fact about this machine is a value, not a
        // sentence"). `…\shortcuts\`, `112 KB` and `96%` are all facts about
        // the machine rather than prose, they all sit right of a Body label
        // on the same line, and drawing them at Body weight would make each
        // row read as two labels. The mock-up draws them at 12.5 px against
        // its 14 px body, which is this role.
        //
        // **About's three LABELS are here and its three VALUES are not, which
        // is the exact opposite of the System page one line up.** That is the
        // mock-up rather than an inconsistency: `.kv .k` is muted at label
        // size and `.kv .v` is not. The rows answer different questions. On
        // System the label names a setting the reader operates and the value
        // is the machine's answer to it; on About the label is a signpost --
        // `Build`, `Location`, `Licence` -- and the VALUE is the thing the
        // reader opened the page for. Whichever half carries the question gets
        // the quieter face.
        IDC_NOTES
        | IDC_CONFIG_DIR
        | IDC_LOG_SIZE
        | IDC_OPACITY_VALUE
        | IDC_ABOUT_BUILD_LABEL
        | IDC_ABOUT_LOCATION_LABEL
        | IDC_ABOUT_DISCLOSURE => Role::Caption,
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
    /// Seven entries: `Ctrl+S` -> Save, `Ctrl+Tab` / `Ctrl+Shift+Tab`, and
    /// `Ctrl+1`..`Ctrl+4`. `build_accelerators` is the table and the reason
    /// for each one.
    ///
    /// **CORRECTED 2026-08-14: this read "`Ctrl+S`, and nothing else".** True
    /// until the tab strip's six keys landed, and the correction is worth
    /// making rather than deleting: this sentence sits on the FIELD, so it is
    /// the one a reader meets first, and it was the last place in the file
    /// still saying the window answers a single accelerator.
    ///
    /// Enter and Esc really are absent, and that half stands: both are the
    /// dialog manager's already (`DM_GETDEFID` and `IDCANCEL`), and an entry
    /// here would only race `IsDialogMessageW` for keys it routes correctly.
    /// `Ctrl+Tab` is the exception that had to be taken off it -- the
    /// `VK_TAB` branch is not documented to consult Ctrl, so leaving it there
    /// moves focus one control and reads as nothing happening.
    ///
    /// Created in `build_children` and destroyed in `WM_DESTROY`: an
    /// accelerator table is a system resource with the same lifetime
    /// discipline as the `HFONT`s beside it, and Landing 1 had to close a
    /// one-per-open leak of those already.
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
    /// The System page's four glyph-button tooltips, kept alive for
    /// `tip_text`'s reason -- `TTM_ADDTOOLW` stores the pointer, not the
    /// bytes.
    ///
    /// **A `Vec` of `Vec`s rather than one buffer with four offsets**,
    /// because comctl32 wants four independent NUL-terminated pointers and
    /// each inner `Vec` owns a heap allocation that does not move when the
    /// outer one is moved into this struct. Four tooltips, four buffers, and
    /// the whole set drops with the window.
    ///
    /// They carry the words the buttons do not: design §3.3 makes each file
    /// row's own filename its label, so `Open` and `Show` have nowhere on
    /// screen to be said and a glyph on its own says nothing to a reader who
    /// has not met it.
    sys_tips: Vec<Vec<u16>>,
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
    ///
    /// **`layout` running is no longer the same event as the combo being
    /// resized**, and since 2026-08-14 it is not the same event as the App
    /// field being LOST either. `combo_needs_placing` means a `layout` whose
    /// geometry comes out identical makes no `SetWindowPos` on the combo at
    /// all, and `place_app_combo` means the placements that do run put the
    /// edit's text and selection back afterwards. That weakens nothing here --
    /// every push this guard suppresses is one whose geometry the guard cannot
    /// know is unchanged, and the three fields are still what keep `layout` off
    /// the keystroke path, where the cheapest correct answer is not to run it
    /// -- but it is why the "run `layout` more often" trade above is now
    /// cheaper than the paragraph makes it sound. Do not take that as
    /// permission to reopen it without measuring: the argument for the gutter
    /// is that it is a margin and never a clipped column, and that has not
    /// changed either.
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
    /// card now (`notes_height`), so nothing absorbs anything -- any gap
    /// between the fallback and the true row height pushes the editor card
    /// down, eating slack above the command bar and, near `MIN_HEIGHT`,
    /// running into `compute_card_rects`' `.min(content_bottom)`.
    /// The other reason it is guarded rather than tolerated is unchanged:
    /// `list_row_height`'s own comment used to justify the fallback by
    /// saying `apply_state` re-lays-out the instant a row appears, which
    /// `shown_external` made false.
    ///
    /// **2026-08-15 made this guard MORE load-bearing, not less, and design
    /// §12 q2 predicted the alternative.** `list_h` used to be
    /// `want.min(room)` with `want` carrying `row_h * tok::ROWS`; deleting
    /// `ROWS` would have taken `row_h` out of the arithmetic entirely and left
    /// this field guarding nothing. It is still an input because
    /// `compute_card_rects` snaps the room down to whole rows -- which is
    /// exactly the "keep the whole-row snap or delete the guard" choice §12 q2
    /// puts, answered by keeping the snap.
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
    /// The two model facts the Shortcuts list's caps fold needs, recorded at
    /// the last push (design §3.2).
    ///
    /// The third input — the view preference itself — is read from `HKCU` at
    /// the moment it is needed and deliberately not cached: it changes only
    /// through `IDC_CAPS_SHORTHAND`, whose handler is the one place that
    /// wants it, and a cached copy would be a second answer to a question the
    /// registry already answers.
    ///
    /// Cached at all because that handler has no `ControlState` to hand: it
    /// runs from `WM_COMMAND`, not from a push, and re-deriving these two by
    /// reading the check boxes back off the screen would make the list's text
    /// depend on the window's own controls rather than on the model.
    caps_on: bool,
    caps_hold: Chord,
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
    /// What About should say about the update check.
    ///
    /// Pushed in by `serve` through `set_update_state` and read by
    /// `apply_about_state`, which builds every OTHER `AboutInputs` field
    /// from local sources -- `current_exe()`, `fs::metadata`, `env!`. This
    /// one cannot be local: the check runs in `serve`, not in the window.
    update: beckon_core::update::UpdateState,
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

/// Paint the pending frame NOW, rather than on the next pump turn.
///
/// Called immediately before `serve` blocks this thread on a network check.
/// `apply_state` has already invalidated the controls that changed --
/// `Checking...` on a STATIC and `Check now` disabled on a `PUSH_BUTTONS`
/// custom-draw button -- and this forces that pending frame to the screen
/// before the block instead of after it.
///
/// **Plain `UpdateWindow(h)` is not enough here**, and was the first
/// attempt: `UpdateWindow` sends `WM_PAINT` only to the window whose OWN
/// update region is non-empty; it does not descend into children.
/// `set_text_if_changed` and `enable()` invalidate the CHILD controls
/// (`SetWindowTextW` / `EnableWindow`), not `h` itself, so `h`'s own update
/// region is typically empty and `UpdateWindow(h)` returns having painted
/// nothing. `RedrawWindow` with `RDW_ALLCHILDREN` is what actually walks the
/// child windows.
///
/// A no-op when the window is closed, which is the right answer rather than
/// an error: the caller does not check first.
///
/// 2026-08-25: the flush call itself and the `RDW_ALLCHILDREN` mechanism are
/// verified against the Win32 docs, but the on-screen result -- whether
/// `Check now` visibly greys out before the block -- is **not yet verified
/// on a real Windows desktop**; this session has none. Confirm with
/// `crates/beckon-windows/examples/settings_probe.rs`.
pub fn flush_paint() {
    // `hwnd()` takes and releases the `UI` borrow before we return, so
    // nothing is held across the paint -- the rule `open_existing` follows
    // one function above.
    if let Some(h) = hwnd() {
        unsafe {
            let _ = RedrawWindow(Some(h), None, None, RDW_UPDATENOW | RDW_ALLCHILDREN);
        }
    }
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

/// `SetWindowPos` on the App combo, with the edit field it rewrites put back.
///
/// **The other half of `combo_needs_placing`, and the half that was missing.**
/// That function stopped the placements that were NOT needed -- the return trip
/// into Shortcuts on a combo that had not moved a pixel. It cannot stop the
/// ones that are: `layout` skips the App combo entirely while another door is
/// open (`if shortcuts`), so every input `layout` reads that moves while the
/// user is behind Keyboard, System or About leaves the combo genuinely stale,
/// and the trip back has to place it. A resize on another door, a
/// `WM_DPICHANGED` there, and the list gaining its first row all reach it that
/// way, so this is not a doors defect wearing a doors fix.
///
/// **CORRECTED 2026-08-14, Task 6.** This named the banner as a fourth such
/// input, and while `banner_shown` ignored the page it was one: the
/// announcement could appear while the user was elsewhere, card 0 gained
/// height, card 2 moved down and `want_app.y` differed. Narrowed back to
/// `BANNER_PAGE`, card 0 stays zero-height on the other three doors and the
/// banner can only move while Shortcuts is the door open -- which is a
/// `WM_SIZE`-shaped path, not a doors-shaped one. The other three inputs are
/// unchanged and this function is still needed for them.
///
/// And a placement is the measured data-loss call: a populated `CBS_DROPDOWN`
/// answers a resize by re-synchronising its edit to the closest catalogue entry
/// and selecting the whole string (a14, comctl32 6.16, 121 items; see
/// `Ui::shown_external`). So this saves the edit's text and selection, places,
/// and puts both back if the control rewrote them.
///
/// **It closes the same defect on the paths that are not doors at all**, which
/// is the reason it is spelled here rather than at the one call site's `if`:
/// `Ui::shown_external` keeps `layout` OFF the keystroke path, but a `WM_SIZE`
/// or a `WM_DPICHANGED` arriving while the App field holds half-typed text was
/// always going to run it, and was always going to lose the text.
///
/// **The text restore is conditional; the SELECTION restore is not**, and the
/// asymmetry is the whole fix rather than a detail of it. The measured
/// mechanism is two things at once: the control re-synchronises its edit to the
/// closest matching item AND selects the whole string. When the field already
/// holds an exact catalogue entry -- which is ordinary, not exotic, since
/// `apply_state` writes `d.app` straight from the model and bindings normally
/// name apps by their exact Start-menu name -- the re-sync leaves the text
/// byte-identical, a text-only guard sees no change, and the select-all
/// survives. The next keystroke then replaces the field, which is the defect
/// verbatim with the right characters on screen.
///
/// So `CB_SETEDITSEL` runs unconditionally. It is a no-op when the selection
/// was already what it puts back, and it costs one message on a call that only
/// runs when the geometry moved.
///
/// **The restore's own notification is harmless, and that is a property of the
/// value rather than of a suppression.** `WM_SETTEXT` raises `CBN_EDITCHANGE`,
/// whose arm posts a deferred read (`WM_APP_EDITED`); the read is dispatched
/// from the message loop after this returns, so it reads the text this put
/// back -- the user's own -- and `Model::set_app` compares before assigning, so
/// a value the model already holds does not mark the file dirty. The re-snap's
/// own `CBN_EDITCHANGE` is the same read and reads the same restored value.
///
/// `CB_GETEDITSEL` packs the selection start in the LOW word and the end in the
/// HIGH word, which is exactly what `CB_SETEDITSEL` takes back as its `LPARAM`,
/// so the value passes through unexamined. Reading it BEFORE the placement is
/// the whole point: afterwards the control has already selected everything, and
/// "everything is selected" is what makes the next keystroke destructive rather
/// than merely surprising.
unsafe fn place_app_combo(h: HWND, x: i32, y: i32, cx: i32, cy: i32) {
    let before = text_of(h);
    let sel = SendMessageW(h, CB_GETEDITSEL, None, None).0 as u32;
    let _ = SetWindowPos(h, None, x, y, cx, cy, SWP_NOZORDER | SWP_NOACTIVATE);
    if text_of(h) != before {
        set_text(h, &before);
    }
    // Unconditional: an exact catalogue entry re-snaps to identical TEXT and a
    // selected-all EDIT, so a guard on the text alone leaves the destructive
    // half in place. See the doc above.
    SendMessageW(h, CB_SETEDITSEL, None, Some(LPARAM(sel as isize)));
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
/// **`Option`, and `IDC_APPLY` has its own arm, since 2026-08-15.** Both
/// changes are one change. This function used to end `_ => DefaultButton::HOME`
/// and `HOME` was the constant `Save`, so `IDC_APPLY` needed no arm — it fell
/// through the catch-all and came back as itself, which was true by coincidence
/// rather than by construction. `HOME` is now `home(page)` and answers `None`
/// on the two doors that do not draw Save, so the catch-all cannot name a
/// button any more: it means "the ring is on nothing", which is exactly what
/// `NO_DEFAULT` records and what System and About rest in.
fn default_button_of(id: i32) -> Option<DefaultButton> {
    Some(match id {
        IDC_APPLY => DefaultButton::Save,
        IDC_ADD => DefaultButton::Add,
        IDC_REMOVE => DefaultButton::Remove,
        IDC_OPENFILE => DefaultButton::OpenFile,
        IDC_CLOSE => DefaultButton::Close,
        IDC_RELOAD => DefaultButton::Reload,
        IDC_KEEPMINE => DefaultButton::KeepMine,
        IDC_RECORD => DefaultButton::Record,
        IDC_REVERT => DefaultButton::Revert,
        IDC_SYS_RELOAD => DefaultButton::SysReload,
        IDC_CONFIG_OPEN => DefaultButton::ConfigOpen,
        IDC_CONFIG_SHOW => DefaultButton::ConfigShow,
        IDC_LOG_OPEN => DefaultButton::LogOpen,
        IDC_LOG_SHOW => DefaultButton::LogShow,
        IDC_ABOUT_BUILD_COPY => DefaultButton::AboutBuildCopy,
        IDC_ABOUT_LOCATION_COPY => DefaultButton::AboutLocationCopy,
        IDC_ABOUT_GITHUB => DefaultButton::AboutGithub,
        IDC_ABOUT_RELEASES => DefaultButton::AboutReleases,
        IDC_ABOUT_BUG => DefaultButton::AboutBug,
        IDC_ABOUT_CHECK_NOW => DefaultButton::AboutCheckNow,
        IDC_ABOUT_UPDATE_COPY => DefaultButton::AboutUpdateCopy,
        _ => return None,
    })
}

/// The id `Ui::defid` and `DM_GETDEFID` carry for "the ring is on nothing".
///
/// Zero is the value `DM_GETDEFID`'s own contract already reserves — it
/// answers `0` in the low word when a dialog has no default — and no beckon
/// control can collide with it: `ids.rs` starts at 1001 and `RETIRED_IDS`
/// never freed anything below it. `set_default_id` still demotes the OUTGOING
/// button when it is handed this, which is the half that matters on screen.
const NO_DEFAULT: i32 = 0;

/// `id_of_default_button` over the `Option` the decision now speaks in.
fn id_of_default_button_opt(b: Option<DefaultButton>) -> i32 {
    b.map_or(NO_DEFAULT, id_of_default_button)
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
        DefaultButton::Revert => IDC_REVERT,
        DefaultButton::SysReload => IDC_SYS_RELOAD,
        DefaultButton::ConfigOpen => IDC_CONFIG_OPEN,
        DefaultButton::ConfigShow => IDC_CONFIG_SHOW,
        DefaultButton::LogOpen => IDC_LOG_OPEN,
        DefaultButton::LogShow => IDC_LOG_SHOW,
        DefaultButton::AboutBuildCopy => IDC_ABOUT_BUILD_COPY,
        DefaultButton::AboutLocationCopy => IDC_ABOUT_LOCATION_COPY,
        DefaultButton::AboutGithub => IDC_ABOUT_GITHUB,
        DefaultButton::AboutReleases => IDC_ABOUT_RELEASES,
        DefaultButton::AboutBug => IDC_ABOUT_BUG,
        DefaultButton::AboutCheckNow => IDC_ABOUT_CHECK_NOW,
        DefaultButton::AboutUpdateCopy => IDC_ABOUT_UPDATE_COPY,
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
/// two: `Add`, `Remove`, `Record` and `Revert` are all Shortcuts-page controls
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
///    on that id is false, and the match has nothing left to do. **That is a
///    hole as well as a convenience, and only the door-change path fills it**:
///    focus parked on the window makes Tab dead, because
///    `IsDialogMessageW` starts its walk at `msg.hwnd` and
///    `GetNextDlgTabItem` returns NULL for a starting point that is not a
///    child. `show_page` follows this call with `focus_the_open_door`, which
///    has an obvious place to send it; THIS caller does not, and widening the
///    repair to grab focus off a legitimately parent-focused window would
///    change behaviour on a path nobody has measured. It stays
///    written for whatever still resolves to a live push button whose
///    `visible()` disagrees: `Ok(close)` is the repair for that case, moving
///    focus onto the successor **this** caller names -- `IDC_CLOSE`, looked
///    up with `GetDlgItem` -- **not onto the window itself.** The successor
///    is an argument because `show_page` has a better one; see
///    `repair_hidden_button`. The `Err(_)` arm below is not dead code either:
///    `GetDlgItem` failing to resolve it is reached in practice,
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
    // `IDC_CLOSE` is this caller's successor: a control went out of reach
    // under the focus and there is no obvious place for it to go, which is
    // the situation the fallback was written for. A door change is not that
    // situation and names its own -- see `repair_hidden_button`.
    //
    // **...but only where `Close` is on screen, 2026-08-15.** The store split
    // hides the command bar's three buttons on System and About, so the
    // successor this caller had always been able to name unconditionally --
    // "always present, always enabled even in the read-only state", as
    // `repair_hidden_button`'s own doc puts it -- is behind another door on
    // half of them. Moving focus onto a hidden control is the exact defect
    // that function exists to repair, so naming one here would have made the
    // repair the fault.
    //
    // The fallback is the open door's own pill, which is `show_page`'s
    // successor and is on screen by construction: the strip is chrome and is
    // never hidden.
    let successor = if command_bar_shown(page) {
        IDC_CLOSE
    } else {
        tab_id_of(page)
    };
    repair_hidden_button(hwnd, external_change, page, successor);
    let cur = UI
        .with(|u| u.borrow().as_ref().map(|ui| ui.defid))
        .unwrap_or(IDC_APPLY);
    let want = default_button(default_button_of(cur), st, external_change, page);
    // `set_default_id` no-ops when the id it is handed is already the
    // default, so the overwhelmingly common push repaints nothing.
    set_default_id(hwnd, id_of_default_button_opt(want));
}

/// The half of `repair_default_button` that needs no `ControlState`: move
/// focus off any control that is not on screen, and the ring off a BUTTON
/// that is not.
///
/// **`successor` is the caller's answer, not this function's -- new
/// 2026-08-14, and it is a fix.** Where focus should GO when it has to move is
/// a question the two callers answer differently, and answering it here for
/// both of them was a defect. `repair_default_button` fires when a control
/// went out of reach under the focus with no obvious replacement, so it names
/// `IDC_CLOSE`: always enabled even in the read-only state, and safe under a
/// stray Space because it routes through `on_close_request`.
///
/// **"Always present" came off that list on 2026-08-15.** Design §1's store
/// split hides the command bar's three buttons on System and About, so
/// `repair_default_button` names `IDC_CLOSE` only where `command_bar_shown`
/// is true and the open door's pill otherwise -- which makes the two callers
/// agree on the fixed doors and keeps the successor a control that is
/// guaranteed on screen. A successor that is itself hidden would make this
/// function the source of the state it exists to repair.
///
/// **A door change HAS an obvious successor, and it is the pill for the door
/// just opened.** `show_page` names that instead, for three reasons:
///
/// - it is where the user's attention already is -- they pressed `Ctrl+2`,
///   and that pill is the thing that lit up;
/// - it keeps Left/Right browsing the strip, which is what someone who just
///   changed doors is most likely to do next. (Tab keeps working too: the
///   pill is checked by step 2 before this runs, and G-S2 measured that
///   user32 puts `WS_TABSTOP` on the checked radio.)
/// - and **it cannot press anything.** A pill is not in `PUSH_BUTTONS` and is
///   not a command at all, where `IDC_CLOSE` under the same finger asks to
///   close the window. That was the defect: `IDC_CLOSE` IS a push button, so
///   the ring followed focus onto it (`BN_SETFOCUS` -> `set_default_id`, the
///   same rule every Tab step obeys), and Enter straight after `Ctrl+2`
///   closed the window the user had just started browsing.
///
/// The focus move onto a pill cannot open a door: `SetFocus` does not check
/// an auto-radio -- a click does, and so does `IsDialogMessageW`'s own
/// arrow-key walk, neither of which is happening here -- and `CheckRadioButton`
/// has already ticked this one at `show_page`'s step 2 in any case. So there
/// is no `BN_CLICKED`, no re-entry into `show_page`, and nothing for the
/// unchanged-door guard to catch.
///
/// **What Enter does with a pill focused, settled here rather than left to be
/// found out: it Saves.** A pill is a radio button, so it answers
/// `WM_GETDLGCODE` without `DLGC_DEFPUSHBUTTON` and `IsDialogMessageW` falls
/// through to `DM_GETDEFID` -- and the pills carry no `BS_NOTIFY`, so no
/// `BN_SETFOCUS` arrives to move the ring onto them. The ring is therefore at
/// `HOME` by the time this returns, on either route into the `SetFocus`: it
/// was already there (focus had left the last push button, and `BN_KILLFOCUS`
/// restores Save), or the `visible` test at the bottom sends it there because
/// the button it named is behind the door that just closed. Enter saves, a
/// Save with nothing to save is disabled, and the dialog manager does not
/// dispatch to a disabled control -- `DefaultButton::HOME`'s own argument,
/// unchanged.
///
/// That settles **half** of spec 10 open question 4. The half it settles is
/// what Enter on a pill means today; the half it does not is what
/// `Ui::defid` rests on once auto-save deletes Save, when `DM_GETDEFID` would
/// name a button that no longer exists. This repair makes a focused pill
/// commoner, so it sharpens that question rather than answering it, and the
/// auto-save workstream still owns it.
///
/// Reasoned from `IsDialogMessageW`'s documented order and this window's own
/// `DM_GETDEFID` arm, **not measured**: the run that would confirm it is
/// `Ctrl+2` then Enter with the config dirty, which must write the file and
/// leave the window open.
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
/// **"Fixed before it can fire" is no longer true, and this arm now fires.**
/// The claim was that every switch arrives as a pill click or an arrow key,
/// both of which move focus onto the pill BEFORE `show_page` hides anything, so
/// the control being hidden is never the focused one. It was true when it was
/// written; `aa9fbd6` broke it in the same commit that wrote it down
/// (`apply_settings` -> `switch_to_page` -> `show_page` was a third route, it
/// moved no focus at all, and `Ctrl+S` reaches it with focus wherever the user
/// left it -- the App combo included, live and shipped rather than
/// hypothetical); that route was then deleted and the claim restored.
///
/// **`Ctrl+Tab` and `Ctrl+1`..`4` retire it for good.**
/// `TranslateAcceleratorW` runs before `IsDialogMessageW` and moves no focus at
/// all -- the same property that made the deleted route sharp, and what makes
/// these six keys the sharp case for `layout` (module header). So the ordinary
/// way to reach this arm is now: focus the App field, press `Ctrl+2`, and the
/// combo it was typing into is hidden under the focus. `hidden_child` is what
/// sees that (its own doc explains why the parent chain is the question, since
/// the focused window is the combo's inner EDIT and keeps its own `WS_VISIBLE`
/// bit for ever), and `CBN_KILLFOCUS` -> `commit_fields` is what keeps the
/// half-typed text.
///
/// The order this landed in is deliberate and is the point: the repair shipped
/// one commit ahead of the keystroke that needs it, rather than being written
/// under the pressure of a defect it was supposed to prevent. The one commit
/// this arm spent live in `aa9fbd6` is the evidence that "unreachable today" is
/// a statement with a shelf life, not a property.
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
/// The ring falls back to `HOME` rather than following focus onto the
/// successor. That looks like a disagreement with repair 2 above and is
/// not: `SetFocus` on a push button raises `BN_SETFOCUS`, `handle_command`
/// answers it with `set_default_id(hwnd, id)`, and that has already happened
/// by the time the `visible` test below runs -- so the fallback fires only
/// when focus did NOT land on a push button, which is the case `HOME` is for.
/// Both successors are now live examples of the two halves: `IDC_CLOSE` takes
/// the ring with it, a pill leaves it at `HOME`.
unsafe fn repair_hidden_button(hwnd: HWND, external_change: bool, page: Page, successor: i32) {
    let focus = GetFocus();
    if !focus.is_invalid() {
        let fid = GetDlgCtrlID(focus);
        // A push button this function cannot name is one whose id left
        // `default_button_of`'s table, and moving focus off it is the safe
        // reading of an id nobody claims -- the alternative is Space reaching
        // a control the decision layer has no opinion about.
        //
        // `map_or(true, ..)` rather than `is_none_or`: the workspace pins
        // `rust-version = "1.75"` and `Option::is_none_or` is stable since
        // 1.82. Clippy's `incompatible_msrv` catches it on the Windows job.
        #[allow(clippy::unnecessary_map_or)]
        let ring_is_off_this_page =
            default_button_of(fid).map_or(true, |b| !b.visible(external_change, page));
        if hidden_child(hwnd, focus) || (is_push_button(fid) && ring_is_off_this_page) {
            match GetDlgItem(Some(hwnd), successor) {
                Ok(next) => {
                    let _ = SetFocus(Some(next));
                }
                Err(_) => {
                    // Not dead code: both successors are created
                    // unconditionally in `build_children`, but this arm is
                    // reached in practice, not merely a defensive branch that
                    // never fires -- see `repair_default_button`'s doc
                    // comment. Fall back to the window itself rather than
                    // leave focus stranded on a vanished control -- a dead Tab
                    // key is a smaller defect than Space reaching a hidden
                    // button.
                    if beckon_core::verbose() {
                        eprintln!(
                            "verbose: settings window: GetDlgItem({successor}) \
                             failed while moving focus off a hidden control"
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
    // `DefaultButton::home(page)`, not the old `HOME` constant: on System and
    // About that is `None`, so this both takes the ring OFF the vanished
    // button and declines to hand it to a Save those doors do not draw. The
    // `is_none_or` arm is the resting state itself -- `cur` is already
    // `NO_DEFAULT` there, `default_button_of` answers `None`, and
    // `set_default_id` no-ops on the repeat.
    #[allow(clippy::unnecessary_map_or)] // MSRV 1.75; `is_none_or` is 1.82.
    let gone = default_button_of(cur).map_or(true, |b| !b.visible(external_change, page));
    if gone {
        set_default_id(hwnd, id_of_default_button_opt(DefaultButton::home(page)));
    }
}

/// Put focus on `page`'s pill when the door change left it on the WINDOW
/// itself.
///
/// **The gap `repair_hidden_button` structurally cannot close, new
/// 2026-08-14.** Hiding the focused control hands focus to the parent -- the
/// measured behaviour `repair_default_button`'s repair 1 is written around --
/// and neither of that function's two arms can see the parent afterwards:
/// `hidden_child(hwnd, hwnd)` is false because `IsChild` is false for a window
/// against itself, and `GetDlgCtrlID(hwnd)` answers 0, which `is_push_button`
/// rejects (`an_id_that_is_not_a_push_button_reads_as_home` and
/// `the_window_itself_is_not_a_push_button` pin both ids). So it returns
/// having moved nothing, focus stays on `hwnd`, and `IsDialogMessageW`'s Tab
/// branch resolves through `GetNextDlgTabItem(h, msg.hwnd, ..)`, which is NULL
/// unless `msg.hwnd` is `IsChild` of `h`: **Tab does nothing at all until the
/// user clicks a control.** The one focused control that escaped this is the
/// App combo, whose inner EDIT stays a child of the window, so `hidden_child`
/// catches it and the repair reaches the successor by the ordinary route.
///
/// The same argument is already written down twice -- in
/// `repair_default_button`'s "an earlier version of this fix parked focus on
/// `hwnd`", which is where the `GetNextDlgTabItem` fact came from (caught in
/// review there, not measured), and in `repair_hidden_button`'s
/// `Err(_)` arm, which parks focus there deliberately and calls the dead Tab
/// key the smaller of two defects. What was missing is that a door change
/// reaches the same place without anyone choosing it.
///
/// **Why this is not a third arm inside `repair_hidden_button`.** Its other
/// caller is `repair_default_button`, which runs after every `apply_state`
/// push; a window legitimately holding its own focus there is a state nobody
/// has measured, and grabbing focus back from it would change behaviour on a
/// path this defect is not about. A door change has an answer that path does
/// not: the pill the user just lit up, which is already `show_page`'s chosen
/// successor and already argued for at length in `repair_hidden_button`.
///
/// **After `repair_hidden_button`, never before.** That function's own
/// `SetFocus` is what normally lands focus somewhere legitimate, and its
/// `Err(_)` fallback parks it on `hwnd` on purpose -- both leave this test
/// answering correctly, since the fallback only fires when the pill could not
/// be resolved and this looks up the same pill and does nothing when it cannot
/// be found either.
///
/// **`== hwnd`, not "is not a child".** `GetFocus` answers for the whole
/// thread and `serve` owns other windows on it (the tray's, plus whatever COM
/// creates), so a null answer or another window's is none of this door's
/// business -- the same reasoning `hidden_child`'s `IsChild` half already
/// makes. Only focus parked on our own window is a state this switch created.
///
/// The ring needs nothing here: `repair_hidden_button` has already sent it to
/// `HOME` if the button it named went behind the door, and a pill raises no
/// `BN_SETFOCUS` to move it -- the pills carry no `BS_NOTIFY`, and `SetFocus`
/// does not check an auto-radio. Same argument, same conclusion, as the pill
/// successor's.
///
/// Reasoned from `IsDialogMessageW`'s and `GetNextDlgTabItem`'s documented
/// behaviour, **not measured**: the run that confirms it is `Ctrl+2` from a
/// keyboard-focused control on Shortcuts, then Tab, which must move focus to a
/// control rather than doing nothing.
unsafe fn focus_the_open_door(hwnd: HWND, page: Page) {
    if GetFocus() != hwnd {
        return;
    }
    if let Ok(pill) = GetDlgItem(Some(hwnd), tab_id_of(page)) {
        let _ = SetFocus(Some(pill));
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

/// Is `h` a control of `parent`'s that is not on screen?
///
/// **`IsWindowVisible`, which folds in the whole ancestor chain -- CORRECTED
/// 2026-08-14.** This read `GetWindowLongW(h, GWL_STYLE) & WS_VISIBLE`, "the
/// control's OWN bit, not `IsWindowVisible`", on the argument that the parent
/// chain is a second question the one caller has no opinion about. That
/// reasoning is backwards, and it made the repair blind to exactly the control
/// it was written for.
///
/// `IDC_APP` is a `CBS_DROPDOWN`, so comctl32 gives it an inner EDIT with an id
/// of its own choosing, and **that EDIT is what `GetFocus` returns** -- the
/// same fact `an_id_that_is_not_a_push_button_reads_as_home` records and that
/// `WM_APP_EDITED`'s measurement rests on. `show(combo, false)` clears
/// `WS_VISIBLE` on the COMBOBOX and on nothing else, so the inner EDIT keeps
/// its own bit set for ever. The own-bit test therefore answered "visible" for
/// the one focused control a door can hide invisibly, which is the case the
/// commit message, the doc and the test all named.
///
/// So the parent chain IS this repair's question, and it is the only one worth
/// asking: if an ancestor is hidden the control cannot be seen, so focus on it
/// is stranded whichever window in the chain did the hiding. "Did THIS window
/// hide it" was never the interesting half -- it is a narrower question with
/// the same answer in every case that reaches here, except the one it gets
/// wrong. `paint::field_border` asks with the same call, for its own reason (a
/// hidden window keeps its window rect, and the border is drawn by the parent),
/// which is why that reason is spelled there and not borrowed here.
///
/// **`IsChild` is the other half and is not defensive.** `GetFocus` answers
/// for the whole THREAD, and `serve` owns another window on it (the tray's,
/// plus whatever COM creates), so without it a hidden window that is none of
/// this window's business would be read as a hidden control of ours and pull
/// focus away from it. It also reads "descendant", not "immediate child",
/// which is what lets the inner EDIT above pass it at all.
unsafe fn hidden_child(parent: HWND, h: HWND) -> bool {
    IsChild(parent, h).as_bool() && !IsWindowVisible(h).as_bool()
}

thread_local! {
    /// What `IDC_SERVICE_LINE` is currently showing -- `SHOWN_NOTES`' twin,
    /// for that mirror's exact reason: `WM_DRAWITEM` can arrive while `UI` is
    /// already borrowed, and a window's plain text cannot carry the `Mark` the
    /// dot is drawn from.
    ///
    /// **`None` is a real state and the painter must survive it.** The control
    /// is created `WS_VISIBLE`, so it can be asked to paint before the first
    /// push ever runs -- and a `WM_DRAWITEM` that draws nothing leaves
    /// whatever was last in that rect.
    ///
    /// It is also the state `WM_DESTROY` restores, and that is not tidiness:
    /// this is a thread_local, the tray opens and closes this window on one
    /// thread all day, and `show_service` skips the write when the mirror
    /// already agrees. Left holding the last window's line, it would skip the
    /// FIRST write of the next one.
    static SHOWN_SERVICE: RefCell<Option<ServiceLine>> = const { RefCell::new(None) };
}

/// Push the service line: mirrored for `WM_DRAWITEM`, and written as plain
/// text so `GetWindowText` still answers -- `show_notes`' arrangement exactly,
/// one band lower.
unsafe fn show_service(hwnd: HWND, line: &ServiceLine) {
    let changed = SHOWN_SERVICE.with(|c| c.borrow().as_ref() != Some(line));
    if !changed {
        return;
    }
    SHOWN_SERVICE.with(|c| *c.borrow_mut() = Some(line.clone()));
    if let Ok(h) = GetDlgItem(Some(hwnd), IDC_SERVICE_LINE) {
        // `set_text` first, then invalidate: the mirror is what the painter
        // reads, and the text is what a screen reader and the probe read.
        set_text(h, &line.text);
        let _ = InvalidateRect(Some(h), None, false);
    }
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
/// the one funnel every door change goes through -- that arm, and the two
/// accelerator arms beside it, which now cover `Ctrl+Tab`, `Ctrl+Shift+Tab`
/// and `Ctrl+1`..`Ctrl+4`. `end_capture` is idempotent, so the overwhelmingly
/// common switch, with nothing armed, costs a cleared flag and a `KillTimer`
/// that fails.
///
/// **The keyboard routes cannot in practice arrive with a capture armed, and
/// that is not why this call is here.** While armed the hook swallows every
/// keystroke with the window foreground (`show_capture`: "there is no keyboard
/// route to fall back on", which is why `Stop` must stay enabled), so
/// `Ctrl+Tab` is recorded as a chord rather than delivered as an accelerator;
/// and with the window NOT foreground, all three of spec F.4's focus layers
/// have already disarmed. Those are two facts about the hook, both of which
/// could change; this call is a property of the funnel, which is what makes it
/// worth having anyway. The MOUSE route is the one that measurably needed it.
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
///    banner. `Add`, `Remove`, `Record` and `Revert` are all Shortcuts-page
///    controls and all four are in `PUSH_BUTTONS`. It runs after step 3
///    rather than before it, because `ShowWindow(SW_HIDE)` on the focused
///    control is what usually moves focus in the first place.
///
///    **The successor it is handed is this door's own pill, never
///    `IDC_CLOSE`.** The pill is the one control this switch is guaranteed
///    not to have hidden -- the strip is chrome, so it is absent from
///    `PAGE_CONTROLS` and `show_page_controls` never touches it -- it is what
///    the user just lit up, and Enter on it cannot press a command. Sending
///    focus to the exit instead put Enter one keystroke from closing the
///    window; `repair_hidden_button` spells out the whole argument, including
///    what Enter does from here.
///
///    **`focus_the_open_door` immediately after it, and it is the other half
///    of the same step.** Hiding the focused control does not merely fail to
///    raise a notification -- it hands focus to the WINDOW, and neither of
///    `repair_hidden_button`'s arms can see that (a window is not its own
///    child, and its control id is 0). Focus left there makes Tab dead until
///    the user clicks, because `IsDialogMessageW` walks from `msg.hwnd` and
///    `GetNextDlgTabItem` refuses a starting point that is not a child. Only
///    the App combo escaped, through its inner EDIT.
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
        // The successor is this door's pill, ticked at step 2 and placed at
        // step 4 -- the one control on screen that the user was just looking
        // at and that Enter cannot turn into a command.
        repair_hidden_button(hwnd, external_change, page, tab_id_of(page));
        // And the case that repair cannot see: user32 handed focus to the
        // WINDOW when it hid the focused control, where Tab is dead. Same
        // pill, same argument -- see `focus_the_open_door`.
        focus_the_open_door(hwnd, page);
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

/// Move to `page` and tell the caller, if the door really moved.
///
/// The three `handle_command` arms that open a door -- a pill, `Ctrl+1`..`4`
/// and `Ctrl+Tab` -- all come through here, so "report `ShowPage` only when
/// the page changed" is spelled once. It matters because the caller STORES
/// what it is told (`ServeState::settings_page`, so the next `open` lands
/// where the user left off) and because pressing the accelerator for the door
/// you are already behind is not the user moving anywhere.
///
/// `show_page`'s own return value is what says so; this function exists only
/// to keep that test off three call sites.
fn go_to_door(hwnd: HWND, page: Page) {
    if show_page(hwnd, page) {
        with_cb(|cb| (cb.on_command)(SettingsCommand::ShowPage(page)));
    }
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
    // **Once per process, icons included.** `RegisterClassExW` fails
    // harmlessly for an already-registered class, which is what happens when
    // the window is reopened from the tray -- but the small icon does not:
    // `LoadImageW` without `LR_SHARED` hands back a handle this process owns,
    // the class keeps the FIRST registration's icon (`chrome::paint` reads it
    // back with `GetClassLongPtrW(GCLP_HICONSM)`), and nothing calls
    // `DestroyIcon`. Loading it on every `create` leaked one HICON per reopen
    // in a process that runs from logon to logoff, for a handle that was never
    // used. `LR_SHARED` was the other candidate and is worse here: it is
    // documented to return a cached handle whose size may not match the
    // request, and the request is the whole point of the call.
    static REGISTER_CLASS: std::sync::Once = std::sync::Once::new();
    REGISTER_CLASS.call_once(|| {
        // Resource id 1, the same icon beckon.rc embeds and the tray already
        // uses. hIcon wants the large (SM_CXICON, 32x32) variant LoadIconW
        // returns; hIconSm wants the small (SM_CXSMICON, typically 16x16) one,
        // loaded explicitly via LoadImageW exactly like the tray's own
        // tray_add -- letting the shell downsample the large icon to 16x16 on
        // the fly is what tray_add's comment says blurs an icon that is crisp
        // at 16x16 in the .ico itself. Both fall back to the stock
        // IDI_APPLICATION icon, matching tray_add, so a build without the .rc
        // resource still shows an icon instead of none.
        // `without_provenance` spells the id, because `MAKEINTRESOURCE` is an
        // integer Win32 packs into the pointer slot and never dereferences.
        // See `hotkey.rs`'s tray_add for why `std::ptr::dangling` is the wrong
        // answer here: it returns the alignment, so `u16` would ask for id 2.
        let icon = LoadIconW(Some(hinst.into()), PCWSTR(std::ptr::without_provenance(1)))
            .or_else(|_| LoadIconW(None, IDI_APPLICATION))
            .unwrap_or_default();
        let icon_sm = LoadImageW(
            Some(hinst.into()),
            PCWSTR(std::ptr::without_provenance(1)),
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
        // Non-zero on success; a failure here leaves `CreateWindowExW` below
        // to report it, which it does by returning an error for an unknown
        // class.
        RegisterClassExW(&wc);
    });

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
    // **Both halves measured on a14 2026-08-14, comctl32 6.16
    // (`examples/pill_probe.rs`, gates G2 and G-S3), and the choice stands.**
    // `CDIS_HOT` does reach a `BS_AUTORADIOBUTTON | BS_PUSHLIKE`, with the
    // plain `BS_PUSHBUTTON` beside it reporting hot in the same run -- the
    // control that makes a clean result mean anything. And `BM_GETCHECK`
    // answers an auto-radio 1 / 0, which is what `is_checked` reads and what
    // Task 6's painter takes selected-ness from instead of `CDIS_CHECKED`.
    // So the named fallback (`BS_PUSHBUTTON + BS_NOTIFY`, a `BN_SETFOCUS` arm
    // and `TrackMouseEvent`) is not needed and should not be built.
    //
    // **`paint::tab_pill` draws them, through its own `WM_NOTIFY` arm.** That
    // arm is a sibling of `push_button_custom_draw`'s rather than a widening
    // of it, because the latter is gated on `is_push_button` and these
    // deliberately are not -- the same absence from `PUSH_BUTTONS` this
    // comment's own paragraph above is about. Landed in Task 6; before it the
    // four rendered as ordinary themed push buttons, one of them stuck down.
    //
    // The `AUTO` is doing real work: it clears the sibling pills on a click
    // and it makes Left/Right inside the group select as they move. What it
    // does NOT do is move on an accelerator -- `Ctrl+1`..`Ctrl+4` and
    // `Ctrl+Tab` are exactly that -- which is why `show_page` ticks the pill
    // itself rather than trusting the click path.
    //
    // `WS_TABSTOP` on all four, which is not what Tab ends up seeing. In a
    // real dialog user32 migrates the style onto whichever radio is checked,
    // so a group is ONE tab stop; nothing in this tree had ever exercised a
    // radio group -- the three that existed were retired -- so whether it does
    // that for hand-created controls was gate G-S2.
    //
    // **G-S2 PASSED on the re-run, a14 2026-08-14
    // (`examples/pill_probe.rs`): it migrates.** `[A checked] A:
    // WS_TABSTOP=true B: false`, then `[B checked] A: false B: true` -- the
    // bits followed the check. **Do not add code to migrate the style by hand
    // now that this is known**: it would be a second writer on a value user32
    // owns.
    //
    // The FIRST run of that gate answered nothing, and it is recorded because
    // it looked like a pass. It read back `WS_TABSTOP=true WS_GROUP=true` on
    // the checked radio and `false false` on its sibling -- which is exactly
    // how the probe CREATED them (`pill_probe.rs:113-131`: radio A gets
    // `WS_GROUP | WS_TABSTOP`, radio B gets `WINDOW_STYLE(0)`), and the radio
    // it checked was A. Migration and a total no-op produce that same
    // readback: the probe's own missing control, in the file that opens by
    // insisting every gate needs one. It now checks radio B as well, and only
    // the CHANGE between the two readings is the evidence above.
    //
    // Nothing here changes either way, which is why a blind gate was a note
    // and not a defect. Setting the bit on all four is the safe end: worst
    // case Tab visits four stops instead of one, where the other way round
    // would leave a pill unreachable from the keyboard.
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
    // `IDC_FILTER` -- created next after this control, and visible whenever
    // the Shortcuts door is open -- is the fallback boundary, at the cost of
    // taking the banner's two buttons into the strip's group. (That fallback
    // named `IDC_LBL_SECTION` until 2026-08-15, when the heading was deleted.
    // `IDC_FILTER` is the weaker stand-in of the two: it is hidden behind the
    // other three doors, where this hidden banner is the only terminator left.)
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

    // -- Band 2: the head row -- the filter, then Remove and Add.
    //
    // **Two STATICs used to open this row and neither is left.** The
    // `· 18 bindings` count went first (design 2: the count belongs on the
    // pill, where all four doors can read it), and the `Shortcuts` heading
    // itself went on 2026-08-15 (design §3.1's drawing and the mock-up both
    // open the card with the filter). Ids 1035 and 1020 are RETIRED, not
    // freed. `layout` leaves the width they occupied blank and moves the
    // filter into it -- see the placement loop.
    //
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
    //
    // **`LVS_NOCOLUMNHEADER` since 2026-08-15** -- design §3.1's "no column
    // headers: keycap chips look like keys and app names look like app names".
    // The STYLE and not `ShowWindow` on the Header, deliberately: the style is
    // what makes `LVM_GETCOUNTPERPAGE` and the list's own client rect agree
    // that the header height is zero, so no arithmetic anywhere has to
    // subtract a band nobody can see. Hiding the window instead leaves
    // comctl32 still reserving the row and every reader of that rect still
    // believing in it.
    //
    // The COLUMNS stay, and `LVS_REPORT` with them: two columns are what put
    // the chord flush right against the app name, which is the whole shape of
    // a row. Only the caption band goes.
    //
    // It also retires a defect nobody could fix. `theme_list`'s own comment
    // records a measurement from a14 2026-08-13: in dark mode the Header
    // painted BRIGHT WHITE across the card, because `DarkMode_ItemsView` is
    // inert until the process opts in through uxtheme's undocumented ordinals
    // -- which the 2026-08-11 spec rejected -- and the `NM_CUSTOMDRAW` path
    // meant to owner-draw it was not firing either. The 2026-08-14 photograph
    // shows that white band. A control that is not created cannot be painted
    // the wrong colour.
    let list = child(
        hwnd,
        w!("SysListView32"),
        "",
        WINDOW_STYLE(
            LVS_REPORT | LVS_SINGLESEL | LVS_SHOWSELALWAYS | LVS_NOSORTHEADER | LVS_NOCOLUMNHEADER,
        ) | WS_TABSTOP,
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
    //
    // `LVCF_TEXT` stays even though `LVS_NOCOLUMNHEADER` means the titles are
    // never drawn: a column's text is what the control reports as that
    // column's name when anything asks it about a subitem, and dropping it
    // would change what the window says about itself to save two string
    // literals. Reasoned from the API, not measured -- nothing on the host
    // this was written on can run a screen reader against the window.
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
    // -- Band 4: the editor strip.
    //
    // **Three controls used to be created here and are not** (2026-08-15,
    // design §3.1): `IDC_GRP_EDITOR`, the card's `Editing "…"` caption, and
    // the `App` / `Shortcut` STATICs that labelled the two field lines. All
    // three are in `RETIRED_IDS`. The card's first line is the App field
    // itself now, and the two words it lost are carried by the field's own cue
    // banner and by where the key list sits.
    //
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
    // The `App` label's replacement -- see `cap::APP_CUE`. `CB_SETCUEBANNER`
    // rather than `EM_SETCUEBANNER_MSG`: the message goes to the COMBOBOX,
    // which forwards it to its own edit child, and the edit child's handle is
    // not one this function holds. Same buffer-lifetime rule as the filter
    // box's -- the string must outlive the call, so it is bound.
    let app_cue = wide(cap::APP_CUE);
    SendMessageW(
        app,
        CB_SETCUEBANNER,
        Some(WPARAM(0)),
        Some(LPARAM(app_cue.as_ptr() as isize)),
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
    // `key_label`, not `k.name` -- see the macOS twin's note at the same
    // control. `k.name` is the config token (`j`, `pagedown`, `bracketleft`) and
    // the list column in this same window already spells these keys the way the
    // keyboard does. Owner-drawn, so `paint::combo_item` had the identical bug
    // and takes the identical fix; the index into `key_table()` is untouched.
    for k in key_table() {
        let t = wide(&key_label(&k.name));
        SendMessageW(
            combo,
            CB_ADDSTRING,
            Some(WPARAM(0)),
            Some(LPARAM(t.as_ptr() as isize)),
        );
    }
    // The two commands, created AFTER the key list because creation order is
    // tab order and the strip reads left to right: App -> chips -> key ->
    // Record -> Revert.
    //
    // `BS_NOTIFY` and membership in `PUSH_BUTTONS`, like every other push
    // button here, and on this pair it is load-bearing rather than uniform:
    // without the focus notifications the default ring cannot follow focus
    // onto them, `IsDialogMessageW` falls through to `DM_GETDEFID`, and
    // Enter on a focused `Record` would SAVE. That is the `Reload` defect
    // one band higher.
    for (caption, id) in [(cap::RECORD, IDC_RECORD), (cap::REVERT, IDC_REVERT)] {
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

    // The command bar's service line (design §6.4). `SS_OWNERDRAW`, on
    // `IDC_NOTES`' reasoning one band lower: the dot is a drawn GDI `Ellipse`
    // and a `Mark` cannot ride in window text, so the paint has to be ours.
    //
    // **Chrome: created once, never hidden, on all four doors** -- which is
    // what makes it §6.4's answer to a bar whose buttons are now drawn on
    // two. It is not in `PAGE_CONTROLS` for the reason the pills are not.
    child(
        hwnd,
        w!("STATIC"),
        "",
        SS_OWNERDRAW_STYLE | SS_NOPREFIX_STYLE,
        IDC_SERVICE_LINE,
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
    // **Reclassed from `BS_GROUPBOX` to a plain caption `STATIC`** on the same
    // Task 8 review finding that reclassed the editor card's caption: a themed
    // group-box frame inside the new rounded `card()` background read as two
    // frames around one set of controls. Id unchanged (1019), no
    // `SS_NOPREFIX` -- this caption carries no `&` today, and one added later
    // would have to be doubled, since a plain `STATIC` reads a lone `&` as a
    // mnemonic prefix exactly as a `BUTTON` caption did.
    //
    // **This is the LAST card caption in the window.** The editor's went on
    // 2026-08-15 with `IDC_GRP_EDITOR` (design §3.1), which is why the
    // reasoning above no longer points at a sibling.
    // `layout.rs` places this at `kb_x, kb_y, kb_w, s(24)` now, not the
    // card's full interior height -- see `compute_card_rects`'s and
    // `layout`'s own comments on why `kb_card_h`'s budget does not move.
    // **The `Keyboard` heading is gone (2026-08-16)**, and with it
    // `IDC_GRP_KEYBOARD`. It drew that word directly beneath a tab pill
    // captioned `Keyboard`; design §3.1 deleted the same duplication on the
    // Shortcuts door and §7 rule 5 forbids it. The id is RETIRED, not freed.
    //
    // Design §3.2's third group takes its place at the bottom of the card.
    child(
        hwnd,
        w!("BUTTON"),
        cap::CAPS_SHORTHAND,
        WINDOW_STYLE(BS_AUTOCHECKBOX as u32) | WS_TABSTOP,
        IDC_CAPS_SHORTHAND,
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

    // -- Band 8: the System page (design §3.3). One card, five rows, two
    // painted dividers between the three groups.
    //
    // Creation order is Tab order, and this page's order is the reading
    // order of the drawing: Pause, Start with Windows, Reload / Dark mode,
    // transparency / config row, log row. The two STATICs in each file row
    // carry no tab stop, so Tab goes label-less from the slider straight to
    // the four glyph buttons.
    //
    // **`BS_AUTOCHECKBOX` for all three switches**, painted by
    // `paint::toggle` through `NM_CUSTOMDRAW` -- `TOGGLES` is the list and
    // `toggle_custom_draw` is the dispatcher. Never `BS_OWNERDRAW`: that is
    // a different VALUE of the same 4-bit type field, and taking it would
    // throw away the check-box state machine and the UIA role a screen
    // reader announces. On this page that matters more than it did for
    // `IDC_CAPS`, because three of the four switches in the window are here.
    for (caption, id) in [
        (cap::PAUSE, IDC_PAUSE),
        (cap::AUTOSTART, IDC_AUTOSTART),
        (cap::DARK, IDC_DARK),
    ] {
        child(
            hwnd,
            w!("BUTTON"),
            caption,
            WINDOW_STYLE(BS_AUTOCHECKBOX as u32) | WS_TABSTOP,
            id,
            &fonts,
        );
    }
    // The tray's own reload. `BS_NOTIFY` and membership in `PUSH_BUTTONS`,
    // like every other push button here -- see that list's own comment for
    // why the System five had to join it.
    child(
        hwnd,
        w!("BUTTON"),
        cap::SYS_RELOAD,
        WINDOW_STYLE((BS_PUSHBUTTON | BS_NOTIFY) as u32) | WS_TABSTOP,
        IDC_SYS_RELOAD,
        &fonts,
    );
    child(
        hwnd,
        w!("STATIC"),
        cap::TRANSPARENCY,
        SS_CENTERIMAGE_STYLE,
        IDC_OPACITY_VALUE,
        &fonts,
    );
    // The transparency slider.
    //
    // **`TBS_NOTICKS` and `TBS_HORZ`.** Sixteen ticks under a sixteen-step
    // slider is noise the mock-up does not have, and suppressing the stage is
    // also what keeps `paint::slider_part` down to two parts.
    //
    // **The range is 85..=100, the control's own**, set here and read back by
    // the painter rather than assumed there -- `beckon_core::settings`'
    // `OPACITY_MIN`/`OPACITY_MAX` are the one source. A page size of 5 makes
    // PageUp/PageDown cross the band in three presses; the default would be a
    // quarter of the range, which on a 15-step range is a coarser step than
    // the eye can even see.
    //
    // The label `IDC_OPACITY_VALUE` is created BEFORE it, which is why the
    // caption `cap::TRANSPARENCY` is on that STATIC and not here: a trackbar
    // has no caption, and the row's words and its value share one slot only
    // in the drawing, not in the control tree. See `layout`'s System band --
    // `IDC_OPACITY_VALUE` draws the ROW LABEL at its left and is rewritten
    // with the percentage or the forced-off reason at its right.
    let opacity = child(
        hwnd,
        TRACKBAR_CLASS,
        "",
        WINDOW_STYLE(TBS_HORZ | TBS_NOTICKS) | WS_TABSTOP,
        IDC_OPACITY,
        &fonts,
    );
    SendMessageW(
        opacity,
        TBM_SETRANGE,
        Some(WPARAM(0)),
        Some(LPARAM(
            ((beckon_core::settings::OPACITY_MAX as i32) << 16
                | beckon_core::settings::OPACITY_MIN as i32) as isize,
        )),
    );
    SendMessageW(opacity, TBM_SETPAGESIZE, Some(WPARAM(0)), Some(LPARAM(5)));
    // The two file rows. Four controls each: the file's own NAME as the
    // label (design §3.3 deletes the `Config` / `Log` captions -- the
    // filename identifies the row), the one fact worth a value slot, and two
    // glyph buttons.
    //
    // **Both VALUE slots are `SS_RIGHT`**, so the directory and the size land
    // against the glyph buttons rather than in the middle of the row -- the
    // mock-up's `.val` sits immediately left of them because its `.lab` is the
    // flexing half. Left-aligned they would strand `112 KB` with 150 px of
    // nothing between it and the buttons at the shipped width.
    //
    // **`SS_PATHELLIPSIS` on the config row's value and not on the log's**,
    // and the asymmetry is the whole reason the two are not one loop: that
    // style shortens a path by eating the MIDDLE, keeping the drive and the
    // last folder, which is exactly what a directory wants and exactly wrong
    // for `112 KB` -- there the style would find no separator, fall back to
    // clipping, and it is a value that fits anyway. See `SS_RIGHT_STYLE` for
    // the one thing about that pairing that is documented ambiguously. Both
    // texts are pushed by `apply_system_state`; the shortening is the OS's, so
    // nothing here or in core counts characters.
    child(
        hwnd,
        w!("STATIC"),
        "",
        SS_CENTERIMAGE_STYLE | SS_NOPREFIX_STYLE,
        IDC_CONFIG_NAME,
        &fonts,
    );
    child(
        hwnd,
        w!("STATIC"),
        "",
        SS_CENTERIMAGE_STYLE | SS_NOPREFIX_STYLE | SS_PATHELLIPSIS_STYLE | SS_RIGHT_STYLE,
        IDC_CONFIG_DIR,
        &fonts,
    );
    child(
        hwnd,
        w!("STATIC"),
        "",
        SS_CENTERIMAGE_STYLE | SS_NOPREFIX_STYLE,
        IDC_LOG_NAME,
        &fonts,
    );
    child(
        hwnd,
        w!("STATIC"),
        "",
        SS_CENTERIMAGE_STYLE | SS_NOPREFIX_STYLE | SS_RIGHT_STYLE,
        IDC_LOG_SIZE,
        &fonts,
    );
    for (caption, id) in [
        (cap::OPEN_GLYPH, IDC_CONFIG_OPEN),
        (cap::SHOW_GLYPH, IDC_CONFIG_SHOW),
        (cap::OPEN_GLYPH, IDC_LOG_OPEN),
        (cap::SHOW_GLYPH, IDC_LOG_SHOW),
    ] {
        child(
            hwnd,
            w!("BUTTON"),
            caption,
            WINDOW_STYLE((BS_PUSHBUTTON | BS_NOTIFY) as u32) | WS_TABSTOP,
            id,
            &fonts,
        );
    }

    // -- Card 5: the About page (design §3.4). The mark and the name, a
    // divider, three value rows with copy buttons, a divider, the hook
    // disclosure, and three links.
    //
    // **The waiting line this replaces was the last placeholder in the
    // window** (`IDC_ABOUT_PLACEHOLDER`, 1115, retired), and with it goes the
    // whole "sits in no band" arrangement its creation comment described: this
    // page has a card now, like every other door, so `compute_card_rects`
    // gives card 5 a height behind it and every STATIC below sits on `card`
    // rather than on the window's own `bg`.
    //
    // **Two of the fifteen are `SS_OWNERDRAW` and the rest are not**, which is
    // what decides whether a control needs a row in the `on_card` match: an
    // owner-draw static never asks its parent for a background brush, so
    // `IDC_ABOUT_MARK` and `IDC_ABOUT_DISCLOSURE` are deliberately absent from
    // it (`IDC_NOTES`' rule) while every other STATIC here is in it.
    //
    // The mark: a rounded accent tile with a `b` in it, drawn by
    // `paint::mark`. Owner-draw because neither half of it is text a STATIC
    // can produce -- the tile is a `RoundRect` and the letter has to be
    // centred in it on both axes.
    child(
        hwnd,
        w!("STATIC"),
        cap::MARK,
        SS_OWNERDRAW_STYLE | SS_NOPREFIX_STYLE,
        IDC_ABOUT_MARK,
        &fonts,
    );
    // `beckon 0.9.3`. **The version is the running IMAGE's**, compiled in --
    // which is what makes this row the honest half of the a14 incident: a
    // fresh `beckon --version` starts whatever is on disk today, while this
    // string was baked into the process that is painting it.
    //
    // `SS_CENTER`, the only centred text in the window; see that constant.
    child(
        hwnd,
        w!("STATIC"),
        "",
        SS_CENTERIMAGE_STYLE | SS_NOPREFIX_STYLE | SS_CENTER_STYLE,
        IDC_ABOUT_NAME,
        &fonts,
    );
    // The three value rows. Label, value, copy button -- and the LABELS are
    // created with their captions while the VALUES arrive through
    // `apply_about_state`, because only the second kind can change.
    //
    // **`SS_PATHELLIPSIS` on the Location value alone**, the same asymmetry
    // the System page's two file rows have and for the same reason: it
    // shortens by eating the middle of a path, which is right for
    // `…\scoop\apps\beckon\current\beckon-serve.exe` and finds no separator to
    // cut in `MIT OR Apache-2.0`. Unlike System's config row this one is NOT
    // `SS_RIGHT`: a path reads from its start, and the copy button beside it
    // is a fixed square rather than something the text has to reach.
    //
    // **`SS_NOPREFIX` on all six.** A path can contain an ampersand (`C:\R&D\`
    // is a legal directory name), and without this the STATIC would eat it and
    // underline the next character -- the same trap the app-name column
    // carries this style for.
    for (label_id, label, value_id, value_style) in [
        (
            IDC_ABOUT_BUILD_LABEL,
            cap::ABOUT_BUILD,
            IDC_ABOUT_BUILD_VALUE,
            WINDOW_STYLE(0),
        ),
        (
            IDC_ABOUT_LOCATION_LABEL,
            cap::ABOUT_LOCATION,
            IDC_ABOUT_LOCATION_VALUE,
            SS_PATHELLIPSIS_STYLE,
        ),
    ] {
        child(
            hwnd,
            w!("STATIC"),
            label,
            SS_CENTERIMAGE_STYLE | SS_NOPREFIX_STYLE,
            label_id,
            &fonts,
        );
        child(
            hwnd,
            w!("STATIC"),
            "",
            SS_CENTERIMAGE_STYLE | SS_NOPREFIX_STYLE | value_style,
            value_id,
            &fonts,
        );
    }
    for id in [IDC_ABOUT_BUILD_COPY, IDC_ABOUT_LOCATION_COPY] {
        child(
            hwnd,
            w!("BUTTON"),
            cap::COPY_GLYPH,
            WINDOW_STYLE((BS_PUSHBUTTON | BS_NOTIFY) as u32) | WS_TABSTOP,
            id,
            &fonts,
        );
    }

    // The update check (Task 9, 2026-08-25): two rows under `Build`. Both are
    // created unconditionally, like every other row on this page -- see
    // `IDC_ABOUT_UPDATE_STATUS`'s own doc in `ids.rs` for why the row is
    // disabled rather than hidden when there is nothing for it to say.
    //
    // Row one: the status line, `Check now`, `Open releases page`.
    // `SS_NOPREFIX` on the status line for the reason the value rows above
    // carry it -- an update-check status string is server-supplied text
    // (a version, a release note's first line) and could contain an `&`.
    child(
        hwnd,
        w!("STATIC"),
        "",
        SS_CENTERIMAGE_STYLE | SS_NOPREFIX_STYLE,
        IDC_ABOUT_UPDATE_STATUS,
        &fonts,
    );
    child(
        hwnd,
        w!("BUTTON"),
        cap::CHECK_NOW,
        WINDOW_STYLE((BS_PUSHBUTTON | BS_NOTIFY) as u32) | WS_TABSTOP,
        IDC_ABOUT_CHECK_NOW,
        &fonts,
    );
    // Row two: the upgrade command's value and its own copy button --
    // `value_row`'s shape one page up, minus the label column: nothing here
    // needs a signpost that `Check now` and the status line above it have
    // not already given.
    child(
        hwnd,
        w!("STATIC"),
        "",
        SS_CENTERIMAGE_STYLE | SS_NOPREFIX_STYLE,
        IDC_ABOUT_UPDATE_VALUE,
        &fonts,
    );
    child(
        hwnd,
        w!("BUTTON"),
        cap::COPY_GLYPH,
        WINDOW_STYLE((BS_PUSHBUTTON | BS_NOTIFY) as u32) | WS_TABSTOP,
        IDC_ABOUT_UPDATE_COPY,
        &fonts,
    );

    // The hook disclosure (design §3.4, moved off Keyboard).
    //
    // **`SS_OWNERDRAW`, for two reasons that a plain STATIC gives up
    // together.** It carries a severity dot, which is a drawn `Ellipse` and
    // never the character `●` -- the same rule `draw_notes` follows, and the
    // reason an em-dash in `serve --log` once came back as `?"`. And it is the
    // only WRAPPED prose in the window: `paint::disclosure` runs `DT_WORDBREAK`
    // over whatever width it is given, against a height `layout` measured with
    // `DT_CALCRECT` from the same string in the same font.
    //
    // The caption is set here and never rewritten: it is a constant
    // (`beckon_core::settings::HOOK_DISCLOSURE`), and the painter reads it back
    // off the control rather than being handed it, so there is one copy of the
    // sentence in the process.
    child(
        hwnd,
        w!("STATIC"),
        beckon_core::settings::HOOK_DISCLOSURE,
        SS_OWNERDRAW_STYLE | SS_NOPREFIX_STYLE,
        IDC_ABOUT_DISCLOSURE,
        &fonts,
    );
    // The three links. Ordinary push buttons -- **not** a syslink or a
    // colour-as-affordance: this window custom-draws every button it has, and
    // a fourth appearance that means "this leaves the window" would need a
    // fifth `BtnTier`, its own high-contrast pair and its own `theme::pairs`
    // row before it drew anything. The captions say where they go.
    for (caption, id) in [
        (cap::ABOUT_GITHUB, IDC_ABOUT_GITHUB),
        (cap::ABOUT_RELEASES, IDC_ABOUT_RELEASES),
        (cap::ABOUT_BUG, IDC_ABOUT_BUG),
    ] {
        child(
            hwnd,
            w!("BUTTON"),
            caption,
            WINDOW_STYLE((BS_PUSHBUTTON | BS_NOTIFY) as u32) | WS_TABSTOP,
            id,
            &fonts,
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

    // The System page's four glyph buttons. Same lifetime rule as the line
    // above -- comctl32 keeps the pointer -- so each buffer is built here and
    // then MOVED into `Ui::sys_tips`, which outlives every tooltip because
    // both die with the window.
    //
    // Attached unconditionally, including to the log row's two buttons on a
    // run with no log: a tooltip on a hidden control never shows, and
    // wiring them behind `SYS_ROWS` would put a second reader on a fact that
    // arrives after this function has finished.
    // **About's three copy glyphs ride in the same vector**, which is why it
    // is no longer named for the System page alone in anything but its field
    // name (`Ui::sys_tips`, kept because renaming a field is churn a reader
    // gains nothing from). The lifetime rule is what they share and it is the
    // whole reason there is a vector at all: comctl32 keeps the POINTER, so
    // every buffer has to outlive the tooltip, and `Ui` is what outlives both.
    let mut sys_tips: Vec<Vec<u16>> = [
        cap::TIP_CONFIG_OPEN,
        cap::TIP_CONFIG_SHOW,
        cap::TIP_LOG_OPEN,
        cap::TIP_LOG_SHOW,
        cap::TIP_BUILD_COPY,
        cap::TIP_LOCATION_COPY,
        cap::TIP_LICENCE_COPY,
        cap::TIP_UPDATE_COPY,
    ]
    .iter()
    .map(|t| wide(t))
    .collect();
    for (i, id) in [
        IDC_CONFIG_OPEN,
        IDC_CONFIG_SHOW,
        IDC_LOG_OPEN,
        IDC_LOG_SHOW,
        IDC_ABOUT_BUILD_COPY,
        IDC_ABOUT_LOCATION_COPY,
        IDC_ABOUT_UPDATE_COPY,
    ]
    .into_iter()
    .enumerate()
    {
        if let Ok(h) = GetDlgItem(Some(hwnd), id) {
            add_tooltip(hwnd, h, &mut sys_tips[i]);
        }
    }

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
            sys_tips,
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
            // The resting values are the safe direction, matching `items`
            // above: no fold until the first push says otherwise. A window
            // that briefly spells the chord in full is right about the file;
            // one that briefly folds is claiming a Caps key nobody has armed.
            caps_on: false,
            caps_hold: Chord::default(),
            app_epoch: 0,
            shown_combo: None,
            capture: None,
            theme: theme::ThemeCache::default(),
            hot: None,
            update: beckon_core::update::UpdateState::Idle,
        })
    });
}

/// The window's accelerator table: `Ctrl+S` -> Save, and the six keys that
/// move between doors.
///
/// Enter and Esc are deliberately absent. Both are the dialog manager's
/// already — Enter through `DM_GETDEFID`, Esc through the `IDCANCEL`
/// `WM_COMMAND` it synthesises — and an entry here would only race
/// `IsDialogMessageW` for keys it already routes correctly.
///
/// **`Ctrl+Tab` is the exception to that, and it MUST be an entry here.**
/// `IsDialogMessageW` claims `VK_TAB` on its own account, and its `VK_TAB`
/// branch is **not documented to consult the Ctrl state** — the documented
/// behaviour is "move to the next control, or the previous one with Shift",
/// with Ctrl named nowhere. So the failure mode of leaving `Ctrl+Tab` to the
/// dialog manager is not a dead key, which would be noticed: it is focus
/// moving one control, which is what plain Tab does and which reads as the
/// keystroke having done nothing at all. `filter_dialog_message` calls
/// `TranslateAcceleratorW` first precisely so an entry here can win that race.
///
/// **An accelerator moves no focus**, and that is the property every reader of
/// this function needs to carry away. `Ctrl+1` taken while the App combo holds
/// focus switches the door with the focus still on a control the switch is
/// about to hide — which is why `layout` places only the current page's
/// controls (module header) and why `show_page` ends in `repair_hidden_button`
/// (its step 5). Both were built for this table before it existed; this is the
/// commit that makes them fire.
///
/// The four digits are `VK_1`..`VK_4`, which are the ASCII codes and are
/// contiguous — so the table is built by walking `TABS`, and strip order IS
/// digit order rather than a second list that agrees with it today.
/// `the_digits_match_the_strip` pins the contiguity that walk assumes. The
/// **numeric keypad** is not covered: `VK_NUMPAD1` is a different code, and
/// `Ctrl+1` on the keypad reaches nothing. Left that way on purpose — the
/// entries are an accelerator for the pills, which stay clickable, rather than
/// the only way in.
///
/// An empty or failed table is not fatal: `filter_dialog_message` skips an
/// invalid handle and every command it would have carried is still reachable
/// by mouse, by mnemonic and by Tab-then-Enter.
unsafe fn build_accelerators() -> HACCEL {
    // FVIRTKEY is what makes `key` a virtual-key code rather than a
    // character, and it is REQUIRED for FCONTROL to mean anything.
    let ctrl = FVIRTKEY | FCONTROL;
    // Sized from `TABS` rather than written as 7, so a fifth door is a
    // one-line change in one place. The three fixed entries lead.
    let mut table = [ACCEL::default(); 3 + TABS.len()];
    table[0] = ACCEL {
        fVirt: ctrl,
        key: b'S' as u16,
        cmd: IDC_APPLY as u16,
    };
    table[1] = ACCEL {
        fVirt: ctrl,
        key: VK_TAB.0,
        cmd: IDM_PAGE_NEXT as u16,
    };
    // `FSHIFT` is an ADDITIONAL requirement, not an alternative: with both
    // entries present, `Ctrl+Tab` matches only the first and
    // `Ctrl+Shift+Tab` only the second. An entry naming no shift state would
    // match both and swallow the reverse direction.
    table[2] = ACCEL {
        fVirt: ctrl | FSHIFT,
        key: VK_TAB.0,
        cmd: IDM_PAGE_PREV as u16,
    };
    for (i, (id, _, _)) in TABS.iter().enumerate() {
        table[3 + i] = ACCEL {
            fVirt: ctrl,
            key: VK_1.0 + i as u16,
            cmd: *id as u16,
        };
    }
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

/// How much width the Shortcuts pill sets aside for its count badge,
/// including the gap that separates it from the caption.
///
/// **ONE arithmetic, two callers, and they are on opposite sides of the same
/// pixel.** `layout` adds this to that pill's width; `paint::tab_pill` takes
/// it off the right of the content box before centring the caption in what is
/// left. Two spellings that agreed on the day they were written would drift
/// into a caption drawn off-centre or a badge drawn over it, and neither is
/// visible from a non-Windows host.
///
/// **Constant, never a function of the count**, which is the property the
/// whole badge design rests on: `layout` sizes controls, `layout` is
/// `SetWindowPos` on the populated App combo, and that is the measured
/// data-loss call (`Ui::shown_external`). A badge slot that grew with the
/// number would put a data push on that path. `ControlState::marked_count`'s
/// doc names this same route -- "reserving width for the widest caption at
/// `layout` time" -- as the one open way to have a live count without calling
/// `layout`; this is that route, taken.
///
/// `cap_font()`, not `Role::Body`: the badge draws in the window's one small
/// face and is measured in the same handle it is drawn in. The fallback is
/// the estimate `text_size` already makes when it cannot get a DC, which is
/// deliberately generous -- too wide costs a gap, too narrow clips.
unsafe fn badge_slot_w(hwnd: HWND, dpi: u32) -> i32 {
    let font = cap_font().unwrap_or_else(|| HFONT(GetStockObject(DEFAULT_GUI_FONT).0));
    scale(tok::GAP, dpi) + text_size(hwnd, font, dpi, BADGE_SLOT).0
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

// **Three Header helpers stood here until 2026-08-15 and all three are gone**
// with the Header itself (design §3.1, `LVS_NOCOLUMNHEADER` at the list's
// creation): `set_header_font`, which pushed `Role::BodyStrong` down to a
// control `WM_SETFONT` does not reach on its own; `header_of`, which named it
// by `HWND` for the one `WM_NOTIFY` arm that could not use an `idFrom`; and
// `list_header_height`, whose 96-DPI fallback of 21 was a live input to
// `compute_card_rects`.
//
// **The last of those is why the style was the right lever and hiding the
// window was not.** A hidden Header still answers `LVM_GETHEADER` and still
// has a rect, so `list_header_height` would have gone on returning 21 for a
// band nobody could see -- a term in the vertical arithmetic paying for
// nothing, which is exactly the kind of figure this window has had to correct
// twice already. With no header there is no term: `want` used to be
// `list_header_height(..) + row_h * ROWS` and the whole expression is gone
// (see `compute_card_rects`, which now measures the room and snaps it to whole
// rows).

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
/// **It stopped being an input to `MIN_HEIGHT` on 2026-08-15, and everything
/// below about the floor is a RECORD rather than an instruction.** The floor
/// was derived from `card2_h` (Task 8's card wrapping `grp_h`), which is
/// derived from this -- until the floor changed SUBJECT to the About card.
/// The Shortcuts list yields its room to `card2_h` before anything else moves
/// (`editor_min = card2_h` in `compute_card_rects`), so what a notes line
/// costs is list rows, and the floor cannot move with it. Change this and
/// re-read `MIN_HEIGHT`'s own table; do not re-derive it from here. 16 px (96
/// DPI, derived) / 24 px (144 DPI, measured): the 144 figure IS a fresh a14
/// reading -- item 10 of the 2026-08-11 a14 pass sized the read-only notes
/// STATIC against "5 lines x 24" at 144 DPI, the same Caption face this line
/// measures. The 96 DPI figure comes from applying the same internal-leading
/// ratio the Body font showed at that pass (`text_h` 28 against a requested
/// 21, i.e. 4/3) to Caption's 12 px request -- and that same ratio, applied
/// to Caption's 144-DPI request of 18, reproduces the hardware 24 exactly,
/// which is why it is trusted for the DPI nobody has measured. If a real
/// 96-DPI reading disagrees, what moves is the list's row count, not the
/// floor -- and while the floor was still derived here the disagreement was
/// bounded rather than open-ended: the window height these anchors solve for
/// was `543 + 2(L - 16)` for a real Caption line height `L`.
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
/// 543 was the last anchor derived here, against a 560 window. `MIN_HEIGHT`'s
/// own table has since been re-derived from the About card and carries no such
/// figure -- looking for 543 there is a dead end, not a cross-check.
///
/// **(That pass's arithmetic, kept as the record: the shipped 560 absorbed
/// `L = 16` with the two-row banner-up guarantee intact, and 17 px to
/// spare.)** The list was handed `114 - 2L` px at that floor, against the 65
/// two rows need, so the guarantee held to `L <= 24`; at `L = 25` the list
/// drew one whole row and 21 px of a second, and did not lose that one until
/// `L = 36`. The window ships at 500 over a 480 floor now, and the current
/// margin is in `MIN_HEIGHT`'s own comment. What survives the move unchanged
/// is the safety argument: nothing
/// there can overlap, because `editor_min = card2_h` in `compute_card_rects` (see its
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

/// The transparency row's whole caption: its label, then its value slot.
///
/// **One STATIC holds both, and that is Phase 0's id table rather than a
/// preference.** 1074 is the slider and 1075 is "`96%`, or the reason it is
/// forced off"; there is **no id for the row's label**, and ids are fixed and
/// may not be invented. So the label rides on 1075, and a STATIC has one
/// alignment -- which is why the value sits after a wide gap rather than
/// against the slider the way the mock-up draws it. It reads better than the
/// drawing in the case rule 7 is actually about:
/// `Window transparency    Off in a remote session`.
///
/// **Two writers, one spelling.** `render_system` writes it on every push and
/// the `WM_HSCROLL` arm rewrites it on every step of a drag -- and the drag
/// path must NOT go through `layout` (that is `SetWindowPos` on the populated
/// App combo, the measured data-loss call), so the two are genuinely separate
/// call sites. A separator spelled twice would make the label jump by a space
/// the first time one of them was edited.
fn opacity_slot(value: &str) -> String {
    format!("{}    {}", cap::TRANSPARENCY, value)
}

/// Push what `serve` knows onto the System page.
///
/// **A second entry point beside `apply_state`, and design §1's split by
/// STORE is the whole reason.** `apply_state` renders a `ControlState`, which
/// is the projection of a `Model`, which is what a config file that does not
/// parse fails to produce. Every System row would then be hostage to a TOML
/// error it has nothing to do with -- the defect the design names as fixed
/// "as a side effect" of splitting the window by store. So the System page
/// has its own push, its own state type, and no `editable` flag anywhere in
/// it.
///
/// **Two arguments, not a `SystemState`**: everything else the page draws is
/// something this crate can ask for itself and `serve` cannot. The config and
/// log PATHS are already in `CFG` (handed over once at `open`); the log's SIZE
/// is a `stat`; the theme and opacity preferences are `HKCU\Software\beckon`,
/// which is this window's own store; and whether the machine may be
/// transparent at all is a `GetSystemMetrics` plus a registry read. Passing
/// any of those in would mean `serve` reading Windows-only state on a
/// cross-platform path and this function trusting a copy of it.
///
/// The DECISIONS are still core's -- which rows exist, what the transparency
/// slot says, how a size reads -- and `system_state` is where they are made
/// and where all three CI jobs test them. This function gathers and renders.
pub fn apply_system_state(paused: bool, autostart: Option<bool>) {
    let Some(hwnd) = UI.with(|u| u.borrow().as_ref().map(|x| x.hwnd)) else {
        return;
    };
    let Some(paths) = CFG.with(|c| c.borrow().clone()) else {
        return;
    };
    // The log's size, read fresh on every push: it is the one value on this
    // page that moves on its own, and a `serve` that has just rolled its log
    // (`roll_if_oversized`, 5 MiB) should say so the next time anything
    // refreshes. `None` when the file is not there, which
    // `system_state` renders as `not found` rather than as `0 bytes`.
    let log_bytes = paths
        .log
        .as_ref()
        .and_then(|p| std::fs::metadata(p).ok())
        .map(|m| m.len());
    let st = system_state(SystemInputs {
        paused,
        autostart,
        dark: crate::prefs::dark(),
        opacity: crate::prefs::opacity(),
        block: crate::prefs::transparency_block_now(),
        paths: &paths,
        log_bytes,
    });
    unsafe { render_system(hwnd, &st) };
}

/// Write `st` onto the controls.
///
/// Split from `apply_system_state` so the gathering above has no `unsafe` in
/// it and this has no policy in it -- the same division `apply_state` and
/// `control_state` already have across a crate boundary.
unsafe fn render_system(hwnd: HWND, st: &SystemState) {
    // The row set FIRST, because everything below is placed against it: the
    // card's height, where each row sits, and which controls `layout` places
    // at all. `SYS_ROWS` is read by `compute_card_rects` without a `UI`
    // borrow -- see its own doc.
    let rows = SystemRows {
        autostart: st.autostart.is_some(),
        log: st.log.is_some(),
    };
    let rows_moved = SYS_ROWS.with(|c| c.replace(rows)) != rows;

    // `BM_SETCHECK` raises no `BN_CLICKED`, so none of these three can be
    // read back as a user gesture and none needs the `suppress` guard the
    // Shortcuts page's fields do. `check` routes to `BM_SETCHECK` for a real
    // check box -- none of the three is in the chip table.
    check(hwnd, IDC_PAUSE, st.paused);
    check(hwnd, IDC_AUTOSTART, st.autostart.unwrap_or(false));
    check(hwnd, IDC_DARK, st.dark);

    render_transparency_row(hwnd, st.transparency);

    set_text_if_changed(hwnd, IDC_CONFIG_NAME, &st.config.name);
    set_text_if_changed(hwnd, IDC_CONFIG_DIR, &st.config.value);
    if let Some(log) = st.log.as_ref() {
        set_text_if_changed(hwnd, IDC_LOG_NAME, &log.name);
        set_text_if_changed(hwnd, IDC_LOG_SIZE, &log.value);
    }

    // Only when the row set actually moved, which after the first push is
    // never: both facts are fixed for the window's lifetime (see `SYS_ROWS`).
    // `layout` is `SetWindowPos` on the populated App combo, the measured
    // data-loss call -- so this is guarded for `Ui::shown_external`'s reason
    // and not for tidiness, even though the guard can only fire once.
    if rows_moved {
        show_page_controls(
            hwnd,
            PAGE.with(|p| p.get()),
            UI.with(|u| {
                u.borrow()
                    .as_ref()
                    .map(|x| x.external_change)
                    .unwrap_or(false)
            }),
        );
        layout(hwnd);
        let _ = InvalidateRect(Some(hwnd), None, true);
    }
}

/// Write the transparency row: the slider's position, whether it is live, and
/// the slot beside it.
///
/// **Split out 2026-08-15 because it has two callers and only one of them has
/// a `SystemState`.** `render_system` pushes it as part of the page;
/// `refresh_transparency_row` pushes it on a theme change, where there is no
/// `serve` in the call stack to supply `paused`/`autostart`. Spelled once so
/// the two cannot drift into disagreeing about the same row.
///
/// `TBM_SETPOS` raises no `WM_HSCROLL`, so a push here cannot be mistaken for
/// a drag -- the same property `BM_SETCHECK` has, and the reason neither
/// needs the `suppress` guard the Shortcuts page's fields do.
///
/// The position is written only on the `On` arm. On `Off` the slider is
/// disabled and its thumb is not a claim about anything; moving it would be
/// writing a percentage into a control that has just been told to say
/// `Off in a remote session`.
unsafe fn render_transparency_row(hwnd: HWND, t: Transparency) {
    if let Transparency::On(p) = t {
        if let Ok(h) = GetDlgItem(Some(hwnd), IDC_OPACITY) {
            SendMessageW(h, TBM_SETPOS, Some(WPARAM(1)), Some(LPARAM(p as isize)));
        }
    }
    enable(hwnd, IDC_OPACITY, t.enabled());
    set_text_if_changed(hwnd, IDC_OPACITY_VALUE, &opacity_slot(&t.slot()));
}

/// Re-read the transparency row from the machine and push it, with no
/// `SystemState` and no `serve` behind it.
///
/// **This closes a row that could say the opposite of what the window was
/// doing.** The resolution lives in one place -- `theme::transparency_block`,
/// which `theme::backdrop` and `Transparency::resolve` both consult -- but
/// until now only `apply_system_state` PUSHED the answer, and only `serve`
/// calls that. `on_theme_changed` re-resolved the backdrop and left the row
/// alone, so turning high contrast on (or an `EnableTransparency` flip, which
/// broadcasts `ImmersiveColorSet` without moving `Theme` at all) made the
/// window opaque while the row went on offering a live slider and a
/// percentage. One predicate with two readers is worth nothing if only one of
/// them is ever asked again.
///
/// **It needs nothing from `serve`**, which is what lets it run from a
/// wndproc: the block is a `GetSystemMetrics` plus a registry read and the
/// level is `HKCU\Software\beckon`. No `UI` borrow either -- `enable` and
/// `set_text_if_changed` take only the `HWND` -- so it is safe at the point
/// in `on_theme_changed` where a `UI` borrow would abort the process.
///
/// **Scope, stated rather than implied**: this makes the row agree with the
/// backdrop at every moment the backdrop is re-resolved, and no more. A
/// transition the window is not told about -- entering a remote session,
/// which raises `WM_WTSSESSION_CHANGE` and not `WM_THEMECHANGED` -- leaves
/// BOTH stale until the next `apply_system_state`, and that is one defect
/// about `SM_REMOTESESSION` rather than two about this row.
fn refresh_transparency_row(hwnd: HWND) {
    let t = Transparency::resolve(
        crate::prefs::transparency_block_now(),
        crate::prefs::opacity(),
    );
    unsafe { render_transparency_row(hwnd, t) };
}

/// The target triple this binary was built for, stamped in by `build.rs`.
///
/// **Why the row is worth a control at all**: a14 is ARM64 and runs x64
/// binaries under emulation, so *am I running the emulated build?* is a real
/// question with a real performance answer, and nothing else on screen
/// answers it.
///
/// **Cargo's own `TARGET`, not a `cfg!`-derived guess.** A triple assembled
/// from `std::env::consts::ARCH` plus `cfg!(target_env)` gets the emulation
/// question right too -- both are compile-time facts -- and it cannot see a
/// vendor other than `pc`, so `aarch64-uwp-windows-msvc` and its siblings
/// would come back mislabelled. beckon does not build for those, so the
/// difference is one word in one row; it is taken because the build script
/// already existed for the examples' manifest and forwarding one variable
/// through it cost two lines.
///
/// **No build DATE**, which design §3.4's drawing shows beside the triple.
/// `build.rs`'s own comment carries the reasoning: a stamped date is really
/// "when the build script last ran", cargo caches that, and the version on
/// the row above answers "how old is this" without being able to drift from
/// the running process.
const TARGET_TRIPLE: &str = env!("BECKON_TARGET");

/// Gather the About page from what only this process can know.
///
/// **`current_exe()` is used UNRESOLVED, deliberately**, and this is the one
/// line of the page that a well-meaning simplification would break:
/// `GetFinalPathNameByHandleW` on it would report where the junction points
/// TODAY, which is exactly the surface that lied on a14 -- a watchdog-started
/// beckon ran the 0.8.0 image for three hours while `--version` and scoop's
/// `current` junction both said 0.9.0. std's `current_exe` is
/// `GetModuleFileNameW`, which returns the launch path with `\current\` still
/// in it, and that is what the row must show.
///
/// The verdict has two halves and they answer different failures.
/// `GetProcessTimes` against one `stat` is the CLOCK half, which catches an
/// in-place overwrite; `QueryFullProcessImageNameW` against a `canonicalize`
/// of the launch path is the IDENTITY half, which catches a repointed
/// junction and is the only one that can see the a14 incident.
/// `beckon_core::settings::image_age` decides what they mean together, and
/// its doc carries the measurement showing why the clock half alone was not
/// enough.
fn about_now() -> AboutState {
    let exe = std::env::current_exe().ok();
    // `Written` / `Gone` / `Unknown` are three different answers to the
    // reader, so the error is inspected rather than flattened: a file that is
    // not there is a fact worth printing, and a `stat` that failed for any
    // other reason is beckon declining to claim anything.
    let disk = match exe.as_ref().map(std::fs::metadata) {
        Some(Ok(m)) => match m.modified() {
            Ok(t) => ImageOnDisk::Written(t),
            Err(_) => ImageOnDisk::Unknown,
        },
        Some(Err(e)) if e.kind() == std::io::ErrorKind::NotFound => ImageOnDisk::Gone,
        _ => ImageOnDisk::Unknown,
    };
    // **Both sides go through `canonicalize`, and that is what makes an
    // untested Win32 reading safe to ship.** `std::fs::canonicalize` on
    // Windows is `GetFinalPathNameByHandleW`, so each side becomes a
    // `\\?\`-prefixed path with every junction resolved. If
    // `QueryFullProcessImageNameW` gives back the resolved image (documented)
    // the two differ whenever the junction has moved; if it gives back the
    // launch path (what `MainModule.FileName` showed on a14) the two
    // canonicalise to the same string and the answer is `Same`. The
    // pessimistic reading costs silence, never a false alarm -- see
    // `ImageIdentity`.
    //
    // Note what is NOT canonicalised: `exe` itself, which goes to the row
    // unresolved. Resolving it there is the exact simplification design §3.4
    // forbids, because `…\current\` is the string the row must show.
    let running = running_image_path().and_then(|p| std::fs::canonicalize(p).ok());
    let target_now = exe.as_ref().and_then(|p| std::fs::canonicalize(p).ok());
    // The one field below that is not local: `serve` runs the check, not
    // this window, so `set_update_state` is the only writer and this is a
    // plain read of what it last stored.
    let update = UI.with(|u| {
        u.borrow()
            .as_ref()
            .map(|x| x.update)
            .unwrap_or(beckon_core::update::UpdateState::Idle)
    });
    about_state(AboutInputs {
        version: env!("CARGO_PKG_VERSION"),
        target: TARGET_TRIPLE,
        exe: exe.as_deref(),
        started: process_start_time(),
        disk,
        identity: image_identity(running.as_deref(), target_now.as_deref()),
        update,
    })
}

/// The path of the executable file this process is running, as the kernel
/// records it.
///
/// **Not `current_exe()`**, which is `GetModuleFileNameW` and returns the
/// LAUNCH path -- `…\scoop\apps\beckon\current\beckon-serve.exe` with the
/// junction still in it. That string is what the `Location` row must show and
/// is exactly why it cannot also answer "which file am I". This is the other
/// question, and `QueryFullProcessImageNameW` is documented to answer it
/// about the process rather than about a module handle.
///
/// **`PROCESS_NAME_WIN32`, not `PROCESS_NAME_NATIVE`**: the native form is a
/// `\Device\HarddiskVolume3\…` NT path, which `canonicalize` cannot compare
/// against a drive-letter path without a volume-name lookup this row does not
/// need. Same flag `window_ops::get_process_info` passes.
///
/// **`None` on any failure**, which `image_identity` reads as "no claim".
/// A pseudo-handle to our own process cannot realistically be refused; the
/// arm exists because this runs from a wndproc, where a panic crosses an
/// `extern "system"` boundary and aborts -- `process_start_time`'s rule.
///
/// **Unverified on hardware.** Nothing on the host this was written on can
/// run a Windows process, so what this returns for a junction launch is read
/// from documentation. `about_now` is built so that the wrong reading costs
/// silence rather than a wrong verdict, and `measure_about` in
/// `examples/settings_probe.rs` is where a run would settle it.
fn running_image_path() -> Option<std::path::PathBuf> {
    // MAX_PATH is not the limit here -- a long path is legal in the buffer
    // this fills -- so it is sized past it and the call reports how much it
    // used.
    let mut buf = [0u16; 1024];
    let mut len = buf.len() as u32;
    unsafe {
        QueryFullProcessImageNameW(
            GetCurrentProcess(),
            PROCESS_NAME_WIN32,
            PWSTR(buf.as_mut_ptr()),
            &mut len,
        )
        .ok()?;
    }
    let len = len as usize;
    if len == 0 || len > buf.len() {
        return None;
    }
    Some(std::path::PathBuf::from(String::from_utf16_lossy(
        &buf[..len],
    )))
}

/// When this process started, as a `SystemTime`.
///
/// `GetProcessTimes` answers in `FILETIME`, i.e. 100-nanosecond ticks since
/// 1601-01-01 UTC, and the gap to the Unix epoch is a fixed 11 644 473 600
/// seconds -- both calendars are UTC with no leap seconds, so the conversion
/// is arithmetic rather than a calendar question.
///
/// `None` on failure, which `image_age` reads as "no claim". Asking a process
/// about itself with a pseudo-handle cannot realistically fail; the arm is
/// here because the alternative is `unwrap` in a wndproc, where a panic
/// crosses an `extern "system"` boundary and aborts.
fn process_start_time() -> Option<std::time::SystemTime> {
    const EPOCH_DELTA_SECS: u64 = 11_644_473_600;
    let mut created = FILETIME::default();
    let mut exited = FILETIME::default();
    let mut kernel = FILETIME::default();
    let mut user = FILETIME::default();
    unsafe {
        GetProcessTimes(
            GetCurrentProcess(),
            &mut created,
            &mut exited,
            &mut kernel,
            &mut user,
        )
        .ok()?;
    }
    let ticks = ((created.dwHighDateTime as u64) << 32) | created.dwLowDateTime as u64;
    let secs = ticks / 10_000_000;
    let nanos = ((ticks % 10_000_000) * 100) as u32;
    secs.checked_sub(EPOCH_DELTA_SECS)
        .map(|s| std::time::SystemTime::UNIX_EPOCH + std::time::Duration::new(s, nanos))
}

/// Put one About row's payload on the clipboard, then tell the caller.
///
/// See the `IDC_ABOUT_*_COPY` arms in `handle_command` for why the act
/// happens here rather than in `serve`. What is copied is core's decision:
/// the row's bare value, never the string on screen, which for `Location` may
/// carry a verdict and is shortened by `SS_PATHELLIPSIS` on its way to the
/// pixels.
fn copy_about_field(field: Field) {
    let st = about_now();
    if let Err(e) = crate::clipboard::set_text(copy_text(&st, field)) {
        // Swallowed, on `set_dark`'s reasoning: what is lost is one
        // clipboard write the user can retry, and a modal dialog for it would
        // be worse than the fault.
        eprintln!("beckon: cannot copy to the clipboard: {e}");
    }
    with_cb(|cb| (cb.on_command)(SettingsCommand::Copy(field)));
}

/// Take the update check's latest answer and redraw About with it.
///
/// **A second push, and for the reason `refresh_settings` already gives for
/// the System page's:** About must keep working in the `unreadable_state`
/// case, where there is no `Model` to project a `ControlState` out of, so
/// riding on `apply_state` would make the update row hostage to a TOML
/// error.
///
/// A no-op when the window is closed -- the caller does not check first, the
/// way `refresh_settings` does not.
pub fn set_update_state(update: beckon_core::update::UpdateState) {
    // Written and released here; `apply_about_state` below takes its own
    // `UI` borrow to read it back, and to reach `hwnd` for the redraw. Two
    // separate short borrows, never one held across the other.
    UI.with(|u| {
        if let Some(ui) = u.borrow_mut().as_mut() {
            ui.update = update;
        }
    });
    apply_about_state();
}

/// Push the About page. Third entry point beside `apply_state` and
/// `apply_system_state`, and it takes no arguments at all.
///
/// **Almost everything on this page is something only this process can
/// know**, which is the System page's argument taken all the way: the
/// version is compiled into this binary, the triple is stamped into it by
/// `build.rs`, the launch path is `current_exe()`, and both halves of the
/// stale-image verdict are Win32 calls about this process and this file.
/// There is nothing for `serve` to pass in for any of those, and anything it
/// did pass would be a copy of a fact this crate can read directly.
///
/// **CORRECTED 2026-08-25 (macmini): one field is the exception.** The
/// update check runs in `serve`, not in this window, so `update` genuinely
/// cannot be read locally the way the rest of this page is. It arrives
/// through `set_update_state`'s write into `UI` and this function reads it
/// back rather than taking it as an argument -- `apply_about_state` staying
/// nullary is what lets `serve` call it (via `set_update_state`) without a
/// `ControlState` to build it from, which is the point in the first place.
///
/// **It is called on every refresh rather than once at open**, because two
/// of the things it writes genuinely move: the file at the launch path can
/// be replaced while the window is up (the `Location` row), and the update
/// check can finish while the window is up (the row `set_update_state`
/// drives). Everything else is fixed for the process's lifetime and costs a
/// `set_text_if_changed` that finds nothing to change.
pub fn apply_about_state() {
    let Some(hwnd) = UI.with(|u| u.borrow().as_ref().map(|x| x.hwnd)) else {
        return;
    };
    let st = about_now();
    unsafe { render_about(hwnd, &st) };
}

// The update status line's ink, cached outside `UI` so `WM_CTLCOLORSTATIC`
// never has to borrow it.
//
// **A `Cell`, for `PILL_BADGE`'s reason, stated in full at `SYS_ROWS`.**
// `WM_CTLCOLORSTATIC` fires on every repaint of `IDC_ABOUT_UPDATE_STATUS`,
// which can be far more often than `render_about` runs -- a resize, a theme
// change, the window merely being uncovered -- and deriving the tone fresh
// each time would mean calling `about_now()` from inside a paint message.
// `about_now()` does a `stat` and two `canonicalize` calls; that cost
// belongs on the refresh path, never the paint path. `render_about` writes
// this once per real push and the paint arm only ever reads it.
//
// (Plain `//`, not `///`: a doc comment directly above a `thread_local!`
// macro invocation is an `unused_doc_comments` lint under `-D warnings` --
// the macro does not forward it to anything rustdoc can attach it to. Every
// other thread-local in this file docs the PRECEDING item instead (`PAGE`
// has none to dock to; `SYS_ROWS` docs its `use`; `PILL_BADGE` docs its
// `struct`); this one has neither, so a plain comment is the correct
// reading, not a workaround.)
thread_local! {
    static ABOUT_UPDATE_TONE: std::cell::Cell<FlagTone> =
        const { std::cell::Cell::new(FlagTone::Neutral) };
}

// Is there an upgrade command to show right now?
//
// Cached here for `show_page_controls`' sake, exactly as `ABOUT_UPDATE_TONE`
// above is cached for `WM_CTLCOLORSTATIC`'s: that function runs on a page
// switch, has no `AboutState` in hand, and must not build one -- `about_now`
// costs a `current_exe`, a `stat` and two `canonicalize` calls.
// `render_about` is the only writer.
//
// (A plain comment rather than a doc comment for the reason the block above
// gives: `thread_local!` does not carry one through, and rustc says so.)
thread_local! {
    static ABOUT_HAS_COMMAND: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Does this About control exist right now, independent of which page is up?
///
/// The About twin of `sys_row_shown`, and the same shape: `show_page_controls`
/// ANDs it with the page test, so a control can be a candidate for its door
/// and still be absent.
///
/// **One row uses it, and it is the row that had no state to be absent for
/// until it did.** The upgrade command is `Some` only once a check finds a
/// newer release, so for the whole of an ordinary session there is nothing
/// for its value to say and nothing for its Copy to copy.
fn about_row_shown(id: i32) -> bool {
    match id {
        IDC_ABOUT_UPDATE_VALUE | IDC_ABOUT_UPDATE_COPY => ABOUT_HAS_COMMAND.with(|c| c.get()),
        _ => true,
    }
}

/// Write `st` onto the controls.
///
/// Split from `apply_about_state` for `render_system`'s reason: the gathering
/// above has no `unsafe` in it and this has no policy in it.
///
/// **CORRECTED: this page now has exactly one row that can vanish** -- the
/// upgrade command's, hidden until a check finds one. It still needs no
/// `layout` call, because the row keeps its slot in `about_plan` either way
/// and nothing below it moves; what changes is only whether the two controls
/// in that slot are drawn. The paragraph below describes the rest.
///
/// **No `layout` call**, unlike `render_system`: apart from that one row this
/// page has nothing that can appear or vanish, so a push can never change the
/// card's height. That is what keeps the About page off the one path that
/// reaches `SetWindowPos` on the populated App combo. **Still true after
/// Task 9's two new rows**: both are always on screen, so what varies with
/// `st.update` is text and `EnableWindow`, never `ShowWindow` -- see
/// `IDC_ABOUT_UPDATE_STATUS`'s own doc in `ids.rs` for why.
unsafe fn render_about(hwnd: HWND, st: &AboutState) {
    set_text_if_changed(hwnd, IDC_ABOUT_NAME, &st.name);
    set_text_if_changed(hwnd, IDC_ABOUT_BUILD_VALUE, &st.build.shown);
    set_text_if_changed(hwnd, IDC_ABOUT_LOCATION_VALUE, &st.location.shown);

    // The update check's own row. `status` is `None` only in `Idle` --
    // `UpdateRow`'s own rule is to draw nothing at all then, not an empty
    // line -- so the field's text is blanked rather than the control hidden;
    // see `render_about`'s own doc for why hiding it is the wrong move here.
    set_text_if_changed(
        hwnd,
        IDC_ABOUT_UPDATE_STATUS,
        st.update.status.as_deref().unwrap_or(""),
    );
    // Set on every push, not only when the tone is `Warn`: a line left
    // coloured from a failed check must not stay that colour once a later
    // check succeeds. Written before the possible repaint below so the two
    // can never observe different values.
    let prev_tone = ABOUT_UPDATE_TONE.with(|c| c.replace(st.update.tone));
    if prev_tone != st.update.tone {
        if let Ok(h) = GetDlgItem(Some(hwnd), IDC_ABOUT_UPDATE_STATUS) {
            let _ = InvalidateRect(Some(h), None, true);
        }
    }
    enable(hwnd, IDC_ABOUT_CHECK_NOW, st.update.can_check);

    // The upgrade command's own row. `cmd.shown` is what is drawn; the Copy
    // button routes `Field::UpdateCommand` through `copy_about_field`, which
    // reads `cmd.copy` instead -- see that field's own doc for why the two
    // must never be swapped.
    //
    // **HIDDEN when there is no command, not merely disabled.** It shipped
    // disabled-and-visible in 0.11.0 and was reported on sight once 0.11.1
    // compacted the page around it: with the `Licence` row gone from below
    // and `Open releases page` gone from the row above, what was left was a
    // greyed copy button beside an empty field, with nothing near it to
    // explain what it belonged to. A control that can do nothing and names
    // nothing is not a disabled control, it is a loose one.
    //
    // **`enable(false)` BEFORE `show(false)`, and the order is the whole
    // reason this is safe.** Task 9 declined to hide anything here because
    // `ShowWindow(SW_HIDE)` does not raise `BN_KILLFOCUS`, so a focused
    // button can be hidden with the dialog manager still pointing at it --
    // the defect this window already paid for once on the banner's `Reload`.
    // `EnableWindow(FALSE)` on a focused control moves focus off it first,
    // which is user32's own behaviour and not something arranged here, so by
    // the time the hide runs there is no focus left on it to strand. The
    // page-switch path is covered separately by `about_row_shown` and by
    // `show_page`'s own closing `repair_hidden_button`.
    let command_shown = st.update.command.as_ref().map(|c| c.shown.as_str());
    set_text_if_changed(hwnd, IDC_ABOUT_UPDATE_VALUE, command_shown.unwrap_or(""));
    enable(hwnd, IDC_ABOUT_UPDATE_COPY, command_shown.is_some());
    ABOUT_HAS_COMMAND.with(|c| c.set(command_shown.is_some()));
    for id in [IDC_ABOUT_UPDATE_VALUE, IDC_ABOUT_UPDATE_COPY] {
        if let Ok(h) = GetDlgItem(Some(hwnd), id) {
            show(h, command_shown.is_some());
        }
    }
}

/// Push a snapshot into the controls. The only path that changes what is on
/// screen; the window never reads the model.
///
/// **REATTACHED 2026-08-15.** These two lines spent a day above
/// `opacity_slot`, because the System pass inserted that function and four
/// others between this doc and its own item -- the same slip that moved
/// `reload`'s borrow-safety block onto `set_autostart` in `serve.rs`, in the
/// same commit. It read as a claim that a `format!` of two strings is "the
/// only path that changes what is on screen", which is the opposite of what
/// that sentence exists to say: `opacity_slot` is one of the two SPELLINGS
/// of one row's caption, and this is the entry point that pushes every
/// Shortcuts control at once.
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
        // **The editor card's caption is gone, and nothing replaced it here**
        // (2026-08-15, design §3.1). It said `Editing "<app>"`, or
        // `No shortcut selected` with no row -- which the list one card up
        // already says by highlighting a row or highlighting none, and which
        // the App field below already says by holding the name or being empty.
        //
        // **What went with it was a rule, not just three lines.** This was the
        // only caption in the window fed from the CATALOG, so it was the only
        // one that had to double an `&` before writing it: a `STATIC` reads a
        // lone `&` as a mnemonic prefix unless `SS_NOPREFIX` is given, and
        // Start Menu names really do carry ampersands (`SS_NOPREFIX_STYLE`'s
        // own comment names `Notes & To Do` and `Arts & Crafts`). Unescaped,
        // the first drew as `Editing "Notes  To Do"` with **T** underlined,
        // colliding with the `Ctrl` hold chip. Nothing in the window writes
        // catalog text into a caption any more -- the App combo's edit field
        // is an EDIT, which draws `&` literally -- so the rule has no
        // remaining subject. Reinstate it with any future control that puts an
        // app name in a `STATIC` or a `BUTTON`.
        //
        // **The count beside the heading is gone, and nothing replaced it
        // here.** It read `st.items.len()`, which is the FILTERED list, while
        // the Shortcuts pill's badge reads `st.binding_count`, which is the
        // file -- so the window carried two numbers that disagreed under a
        // filter and were both correct. Design 2 moved the count to the pill
        // so it reads from all four doors; `set_pill_badge` below is the one
        // that survived. See `RETIRED_IDS` for why 1035 is not reusable.

        // The editor strip's two commands. `Record` stays live while a
        // capture is armed even if the row went away underneath it: it reads
        // `Stop` then, and it is the only way to end a recording with the
        // mouse -- the hook is swallowing every keystroke, so there is no
        // keyboard route to fall back on.
        //
        // `Revert` is greyed while armed for the same reason the five typed
        // controls are: it writes the value the hook is in the middle of
        // recording.
        let row = st.detail.is_some();
        enable(hwnd, IDC_RECORD, capturing || (st.editable && row));
        enable(hwnd, IDC_REVERT, st.editable && row && !capturing);
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
        // The service line, pushed on every state: it is the one thing in the
        // window that answers "are the hotkeys working", and it is chrome, so
        // it is written whichever door is open. `show_service` no-ops when
        // nothing changed, so this costs a comparison on the common push.
        show_service(hwnd, &st.service);
        enable(hwnd, IDC_CAPS, st.editable);
        // **The view switch is NOT gated on `st.editable`**, and that is
        // design §1's split by store rather than an oversight -- the same
        // reasoning `pressable` gives the System page's five. `editable` is
        // false exactly when `apps.toml` did not parse, and this control
        // writes `HKCU`, so greying it would make a file beckon cannot read
        // disable a preference about how beckon draws. What it IS gated on is
        // `caps_view_enabled`: with Caps unarmed the fold would advertise a
        // keystroke that does nothing.
        enable(hwnd, IDC_CAPS_SHORTHAND, caps_view_enabled(st.caps_checked));
        check(hwnd, IDC_CAPS_SHORTHAND, crate::prefs::caps_view());
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

        // `banner_shown`, not `external_change` on its own. The function is
        // where the condition is allowed to change, and it just did: Task 6
        // narrowed it back to `BANNER_PAGE` now that the Shortcuts pill carries
        // a warn dot on the other three doors. Reading the flag here would have
        // been one of five sites to find and edit that day, and the one whose
        // omission is silent.
        //
        // One function, five readers: this, `show_page_controls`, `layout`'s
        // card 0, `compute_card_rects`, and core's `DefaultButton::visible` for
        // the two buttons. A ring left on a `Reload` this line has hidden is
        // the measured defect `default_button` exists for.
        let page = PAGE.with(|p| p.get());
        let banner_on = banner_shown(external_change, page);
        // The pill's two values, pushed through a `Cell` rather than through
        // `UI` or through a caption -- `PILL_BADGE` carries both reasons. The
        // count is the FILE's, never `st.items.len()`, which is filtered and
        // exempts the selected row; the badge is read from three pages that
        // have no filter box.
        //
        // `external_change` is stored rather than the dot's own condition,
        // because that condition also depends on the page and the page moves
        // through `show_page`, which never calls this function.
        set_pill_badge(hwnd, st.binding_count, external_change);
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
            ui.caps_on = st.caps_checked;
            ui.caps_hold = st.caps_hold;
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
fn cells(it: &ListItem, fold: Option<Chord>) -> Vec<String> {
    vec![app_cell(it), combo_cell(it, fold)]
}

/// Which chord the list folds, for a state about to be pushed.
///
/// The three inputs meet here and nowhere else: the view preference from
/// `HKCU`, and the two model facts from the state. `caps_view_fold` is core's
/// and carries the decision; this only fetches.
fn fold_for(st: &ControlState) -> Option<Chord> {
    caps_view_fold(crate::prefs::caps_view(), st.caps_checked, st.caps_hold)
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
fn combo_cell(it: &ListItem, fold: Option<Chord>) -> String {
    let d = combo_display_folded(&it.combo, fold);
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
    let fold = fold_for(st);
    for (i, it) in st.items.iter().enumerate() {
        let now = cells(it, fold);
        let was = cells(&prev[i], fold);
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
    let fold = fold_for(st);
    for (i, it) in st.items.iter().enumerate() {
        let texts = cells(it, fold);
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

/// What the Shortcuts pill draws besides its caption: how many bindings the
/// FILE has, and whether the config moved on disk underneath the window.
///
/// **A `Cell`, for `CAP_FONT`'s reason, and this is the third time that
/// reason has decided a design here.** `paint::tab_pill` runs inside a paint,
/// and a paint reaches this window while `UI` is already borrowed -- measured
/// on a14, where every subitem-1 notification exited at `try_borrow` and the
/// Shortcut column silently drew as plain text. `CHIPS`, `CAP_FONT` and
/// `SHOWN_NOTES` are the three precedents; this is the fourth.
///
/// **Neither value may ride in the pill's CAPTION**, which is the other way
/// they could have reached the painter. `layout` sizes every button from
/// `text_size` of its own caption, so a caption that carried the count would
/// make the count a `layout` input -- and `layout` on a data push is
/// `SetWindowPos` on the populated App combo, the measured data-loss call.
/// `cap::STOP` is the same decision already taken once, for `Record`.
///
/// **`external` is stored, not `warn`.** The dot's condition is
/// `warn_dot_shown(external, page)` and `page` moves through `show_page`,
/// which does not call `apply_state` at all -- so a stored `warn` would go
/// stale on the very keystroke it exists for. The painter reads `PAGE`, which
/// is itself a paint-safe `Cell`, and asks core. That the repaint happens is
/// separate and is `CheckRadioButton`'s doing: `warn_dot_shown` changes
/// exactly when `page == BANNER_PAGE` does, which is exactly when the
/// Shortcuts pill's own tick changes, and changing a radio's tick invalidates
/// it.
#[derive(Clone, Copy, PartialEq, Eq)]
struct PillBadge {
    count: usize,
    external: bool,
}

thread_local! {
    static PILL_BADGE: std::cell::Cell<PillBadge> = const {
        std::cell::Cell::new(PillBadge {
            count: 0,
            external: false,
        })
    };
}

/// Push the badge count and the external-change flag, and repaint the
/// Shortcuts pill -- but only when one of them actually moved.
///
/// **The guard is `set_chip`'s, and it is not an optimisation.**
/// `apply_state` runs on every keystroke; an unconditional `InvalidateRect`
/// would repaint a pill nobody is looking at once per character typed into
/// the App field.
///
/// Safe to call from inside `apply_state`: `InvalidateRect` only marks the
/// control dirty, and the `WM_PAINT` (and the `NM_CUSTOMDRAW` it sends back
/// here) arrives later, from the message loop.
fn set_pill_badge(parent: HWND, count: usize, external: bool) {
    let want = PillBadge { count, external };
    if PILL_BADGE.with(|c| c.get()) == want {
        return;
    }
    PILL_BADGE.with(|c| c.set(want));
    if let Ok(h) = unsafe { GetDlgItem(Some(parent), IDC_TAB_SHORTCUTS) } {
        unsafe {
            let _ = InvalidateRect(Some(h), None, false);
        }
    }
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
/// **The System page's five are all `Secondary`, by falling through**, and the
/// four glyph buttons are a KNOWN deviation from the mock-up rather than a
/// match. There `.btn.glyph` is `border-color:transparent;
/// background:transparent`, i.e. a fifth "ghost" tier; here they wear
/// `Secondary`'s `field` fill and `field_border` edge, so each file row ends
/// in two small boxes rather than two bare glyphs. A ghost tier would need its
/// own `colours` arm, its own high-contrast pair and its own
/// `theme::pairs` rows before anything drew it -- and its resting state would
/// be indistinguishable from the card, which is the one thing a button that
/// deletes nothing but LAUNCHES something should not be. Deferred deliberately;
/// nothing else depends on which way it goes.
fn tier_of(id: i32, hwnd_item: HWND) -> BtnTier {
    match id {
        IDC_APPLY => BtnTier::Accent,
        IDC_REVERT => BtnTier::Outline,
        IDC_RECORD if text_of(hwnd_item) == cap::STOP => BtnTier::Danger,
        IDC_RECORD => BtnTier::Outline,
        _ => BtnTier::Secondary,
    }
}

/// Paint any of the twenty-three `PUSH_BUTTONS`, `Save` included, by translating the
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

/// Every switch in this window, in creation order.
///
/// **Four since 2026-08-15**, where `paint::toggle`'s own doc still says "the
/// one toggle switch in this window": `IDC_CAPS` on Keyboard, and design
/// §3.3's `Pause shortcuts` / `Start with Windows` / `Dark mode` on System.
/// One list because one `WM_NOTIFY` arm dispatches all four and one
/// `WM_CTLCOLORSTATIC` arm answers for all four -- a second switch added
/// without a row here paints as an ordinary themed check box beside three
/// switches, which reads as a rendering fault rather than as a missing row.
///
/// Every one of them is a real `BS_AUTOCHECKBOX`; see `is_a_toggle`.
/// **Five since 2026-08-16**: design §3.2's `Write shortcuts as Caps …`
/// joins them. It is the second switch on the Keyboard door, and the first
/// switch anywhere in the window that writes `HKCU` from a page that also
/// writes `apps.toml` -- see `IDC_CAPS_SHORTHAND`'s own note.
const TOGGLES: [i32; 5] = [
    IDC_CAPS,
    IDC_CAPS_SHORTHAND,
    IDC_PAUSE,
    IDC_AUTOSTART,
    IDC_DARK,
];

fn is_a_toggle(id: i32) -> bool {
    TOGGLES.contains(&id)
}

/// Paint one of the four switches by reading the three bits `paint::toggle`
/// needs off the `NMCUSTOMDRAW` comctl32 hands this window -- the same shape
/// `push_button_custom_draw` uses one function up for the fourteen
/// `PUSH_BUTTONS`.
///
/// **`NM_CUSTOMDRAW`, NOT `BS_OWNERDRAW`.** All four stay `BS_AUTOCHECKBOX`
/// -- see their creation calls and `paint::toggle`'s own doc for why:
/// owner-draw is a different VALUE of the same 4-bit type field, not a flag
/// beside it, and adopting it would throw away the check box state machine
/// and the UIA role a screen reader announces. That matters more on System
/// than it did on Keyboard: three of the four are there, and a switch that
/// announces itself as nothing is a switch a screen-reader user cannot find.
///
/// `on` is read with `is_checked`, not off a bit this notification carries
/// -- a check box's `NMCUSTOMDRAW` has no state bit for "ticked", only
/// `CDIS_DISABLED` / `CDIS_FOCUS` / `CDIS_SELECTED` / `CDIS_HOT`, none of
/// which mean checked. `is_checked` already routes these four to
/// `BM_GETCHECK` (see `chip_bit`'s own doc: none of them is in the chip
/// table), so this asks the control the same way `handle_command`'s own arms
/// do.
unsafe fn toggle_custom_draw(hwnd: HWND, id: i32, p: *const NMCUSTOMDRAW) -> isize {
    let cd = &*p;
    if cd.dwDrawStage != CDDS_PREPAINT {
        return CDRF_DODEFAULT as isize;
    }
    let dpi = GetDpiForWindow(hwnd).max(96);
    let on = is_checked(hwnd, id);
    let enabled = cd.uItemState.0 & CDIS_DISABLED.0 == 0;
    let focused = cd.uItemState.0 & CDIS_FOCUS.0 != 0;
    PAINT_THEME.with(|c| toggle(cd, on, enabled, focused, &mut c.borrow_mut(), dpi));
    CDRF_SKIPDEFAULT as isize
}

/// Paint the transparency slider.
///
/// **A `msctls_trackbar32` with `NM_CUSTOMDRAW`, not a hand-rolled control.**
/// Everything a slider has to get right for a keyboard -- Left/Right by one,
/// Page by a chunk, Home/End to the ends, the focus rectangle, the
/// `WM_HSCROLL` stream, the UIA range-value pattern a screen reader reads --
/// is comctl32's and is free. What is not free is the colours: a trackbar
/// draws its channel and thumb from the *light* visual style whatever
/// `SetWindowTheme` is told, which in a `#15171C` card is a pale slot with a
/// pale lozenge in it. So the control is kept and only its pixels are
/// replaced, exactly the trade `push_button_custom_draw` makes for every
/// member of `PUSH_BUTTONS`.
///
/// **Three stages, and the two that matter are the ITEM ones.** A trackbar's
/// custom draw is a two-level notification: `CDDS_PREPAINT` must answer
/// `CDRF_NOTIFYITEMDRAW` or no item stage ever arrives, and then
/// `CDDS_ITEMPREPAINT` arrives once per part with `dwItemSpec` naming it.
/// Answering `CDRF_SKIPDEFAULT` at the top level instead -- which is what the
/// other three painters in this file do -- would suppress the whole control,
/// tick marks, channel, thumb and all, and leave an empty rectangle.
///
/// `TBCD_TICS` is skipped rather than drawn: the control is created
/// `TBS_NOTICKS`, so the stage does not arrive, and drawing sixteen ticks
/// under a sixteen-step slider is noise the mock-up does not have.
unsafe fn slider_custom_draw(hwnd: HWND, p: *const NMCUSTOMDRAW) -> isize {
    let cd = &*p;
    let dpi = GetDpiForWindow(hwnd).max(96);
    match cd.dwDrawStage {
        CDDS_PREPAINT => CDRF_NOTIFYITEMDRAW as isize,
        CDDS_ITEMPREPAINT => {
            let enabled = enabled(hwnd, IDC_OPACITY);
            // The focus ring is the CONTROL's, not the thumb's, so it is read
            // once here rather than off `uItemState` -- a trackbar reports
            // `CDIS_FOCUS` on neither part.
            let focused = GetFocus() == cd.hdr.hwndFrom;
            let drawn = PAINT_THEME.with(|c| {
                paint::slider_part(
                    cd,
                    cd.dwItemSpec as u32,
                    enabled,
                    focused,
                    &mut c.borrow_mut(),
                    dpi,
                )
            });
            if drawn {
                CDRF_SKIPDEFAULT as isize
            } else {
                CDRF_DODEFAULT as isize
            }
        }
        _ => CDRF_DODEFAULT as isize,
    }
}

/// Paint one of the four tab pills, by reading off the `NMCUSTOMDRAW` the
/// three things `paint::tab_pill` cannot ask for itself.
///
/// The same shape `push_button_custom_draw` and `toggle_custom_draw` use one
/// and two functions up: one notification in, one full repaint out,
/// `CDRF_SKIPDEFAULT` on the way back.
///
/// **`is_checked`, not `CDIS_CHECKED`** -- there is no such bit. See
/// `paint::tab_pill`'s own doc, and `toggle_custom_draw` for the identical
/// decision taken for `IDC_CAPS`.
///
/// **The badge and the dot come from `PILL_BADGE` and `PAGE`, never from
/// `UI`.** Both are `Cell`s, and a paint reaches this window while `UI` is
/// already borrowed. The dot's condition is asked of core rather than spelled
/// here, so it stays the exact complement of `banner_shown` -- which is what
/// makes narrowing that function safe.
unsafe fn tab_pill_custom_draw(hwnd: HWND, opens: Page, p: *const NMCUSTOMDRAW) -> isize {
    let cd = &*p;
    if cd.dwDrawStage != CDDS_PREPAINT {
        return CDRF_DODEFAULT as isize;
    }
    let dpi = GetDpiForWindow(hwnd).max(96);
    let id = cd.hdr.idFrom as i32;
    let active = is_checked(hwnd, id);
    let st = PILL_BADGE.with(|c| c.get());
    // `opens` is the door THIS PILL leads to; `PAGE` is the door currently
    // open. They are different questions and the dot needs both -- it is drawn
    // on the Shortcuts pill (`opens`) while the user is standing somewhere
    // else (`PAGE`).
    //
    // Only that one pill carries either, and only it has the slot `layout`
    // reserved for a badge -- so `None` here is what keeps the other three
    // centring their captions across the full content box.
    let (badge, warn) = if opens == BANNER_PAGE {
        (
            Some(st.count),
            warn_dot_shown(st.external, PAGE.with(|p| p.get())),
        )
    } else {
        (None, false)
    };
    PAINT_THEME.with(|c| tab_pill(cd, active, badge, warn, &mut c.borrow_mut(), dpi));
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
    // The System page's transparency row reads the SAME predicate the tier
    // above does (`theme::transparency_block`), so it has to be re-read at
    // the same moments or it starts describing a window that no longer
    // exists. Above the `!changed` return for `apply_current_backdrop`'s own
    // reason, and it is not a hypothetical here either: an
    // `EnableTransparency` flip is precisely a broadcast that moves the block
    // and not the `Theme`.
    refresh_transparency_row(hwnd);
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
    let inputs = backdrop_inputs();
    // **The tier is core's answer; the ALPHA is the user's.** `backdrop`
    // decides whether this machine may be transparent at all -- the three
    // refusals `theme::transparency_block` owns, and the Mica/alpha
    // capability split -- and returns `TIER2_ALPHA` as the level, which is
    // what beckon picks when nobody is asked. The System page's slider is
    // exactly "someone was asked", so the tier survives and only the level is
    // replaced. Substituting here rather than inside `backdrop` keeps that
    // function a pure decision the two non-Windows CI jobs can test, and
    // keeps the slider from being able to make an opaque window transparent:
    // a blocked machine never reaches this arm.
    let tier = match beckon_core::theme::backdrop(inputs) {
        beckon_core::theme::Backdrop::Alpha(_) => {
            beckon_core::theme::Backdrop::Alpha(opacity_alpha(crate::prefs::opacity()))
        }
        other => other,
    };
    theme::apply_backdrop(hwnd, tier);
}

/// What this machine says about transparency, right now.
///
/// Public so `crate::prefs::transparency_block_now` can ask the same question
/// of the same inputs the backdrop tier is decided from -- one reading, two
/// readers, which is what stops the System page calling the slider live on a
/// window that is already opaque. `theme::MICA_SUPPORTED` is threaded through
/// here for the reason its own doc gives: one flag for a hardware failure to
/// flip.
pub fn backdrop_inputs() -> beckon_core::theme::BackdropInputs {
    theme::read_backdrop_inputs(theme::MICA_SUPPORTED)
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

    // **The four fields need their own theme class, and nothing else reaches
    // them.** Measured on a14 2026-08-13: with only the work above, the
    // shipped dark window had three white combo faces, because that part is
    // painted by the visual style rather than by anything this window
    // controls.
    //
    // **The same measurement covered a fourth surface, and it is gone rather
    // than fixed.** The run recorded a BRIGHT WHITE Header band across the
    // card, and the 2026-08-14 photograph shows it. Nothing here could reach
    // it -- `DarkMode_ItemsView` is inert without the uxtheme ordinals, and
    // the `NM_CUSTOMDRAW` route meant to owner-draw it was not firing -- so
    // design §3.1's `LVS_NOCOLUMNHEADER` closed it by removing the control.
    // The `SetWindowTheme` call that used to sit here went with it. The
    // measurement is kept because it is what says the remaining three faces
    // are still wrong.
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
    // What this leaves: in dark mode the three combo faces render light.
    // Fixing it needs one of — replacing the combos with owner-drawn controls,
    // or reopening the ordinals decision. That is a design call, not a code
    // one. (The list of remedies used to lead with "owner-drawing the header
    // (possible, and the NM_CUSTOMDRAW path meant to do it is not firing)".
    // The header is gone, so that route has nothing left to fix.)
    //
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
                //
                // **The fallback is the DOOR's home since 2026-08-15**, where
                // it was the constant `IDC_APPLY`. It is reached only before
                // `UI` exists, but answering `Save` there while the System
                // door is open is the same lie this arm was written to stop,
                // one message earlier.
                let id = UI
                    .with(|u| u.borrow().as_ref().map(|ui| ui.defid))
                    .unwrap_or_else(|| {
                        id_of_default_button_opt(DefaultButton::home(PAGE.with(|p| p.get())))
                    });
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
                // A `set_header_font` call stood here, for the Header the walk
                // above could never reach -- it is `list`'s child, not
                // `hwnd`'s. There is no Header since 2026-08-15
                // (`LVS_NOCOLUMNHEADER`, design §3.1), so the exception is
                // gone and the walk really does cover every control.
                if let Ok(list) = GetDlgItem(Some(hwnd), IDC_LIST) {
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
                // The System and About cards' two dividers each, in the same
                // layer as the cards and immediately after them -- they are
                // drawn ON a card, so a card painted afterwards would erase
                // them. Their geometry is `system_plan`'s and `about_plan`'s,
                // the same arithmetic `layout` places the rows either side of
                // them from, reached through these two functions for
                // `card_rects`' reason: one function per page, two readers
                // each, no second copy to drift.
                //
                // **Two functions chained rather than one taking a page.**
                // The pages compute their offsets from unrelated plans, so a
                // shared entry point would be an `if` wrapped around two
                // arithmetics. Each answers with zero-width rects behind any
                // door but its own, which `divider` declines to draw -- the
                // same degenerate-rect rule the loop above applies.
                {
                    let dpi = GetDpiForWindow(hwnd).max(96);
                    for rc in system_dividers(hwnd)
                        .into_iter()
                        .chain(about_dividers(hwnd))
                        .chain(keyboard_dividers(hwnd))
                    {
                        divider(hdc, rc, dpi);
                    }
                }
                // The tab strip's trough, in the same layer as the cards and
                // for the same reason: it is the window's own background under
                // four child controls that paint themselves afterwards. It is
                // NOT one of the four rects above -- `compute_card_rects`
                // deliberately does not return it, because it is not a card
                // and `card` would give it a border and the wrong fill.
                //
                // `strip_rect` is the one source of that geometry, the same
                // function `layout` places the pills from and
                // `compute_card_rects` reads the first card's `y` out of.
                //
                // Outside the `PAINT_THEME.with` block below, exactly like the
                // card loop: `trough` reads the theme through `theme_col` /
                // `theme_brush`, each of which takes its own borrow, and a
                // nested second borrow of that `RefCell` panics.
                {
                    let dpi = GetDpiForWindow(hwnd).max(96);
                    let mut rc = RECT::default();
                    if GetClientRect(hwnd, &mut rc).is_ok() {
                        trough(hdc, strip_rect(rc, dpi), dpi);
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
                // **An arm for the ListView's own Header (Task 10) stood here
                // and is gone** with the control (design §3.1,
                // `LVS_NOCOLUMNHEADER`). It matched on `hwndFrom` rather than
                // `idFrom`, because the Header is a child of `IDC_LIST` and
                // carries no dialog id -- so it was the one custom-draw source
                // in the window that could not be told apart by number.
                //
                // Worth knowing before anything else is dispatched by handle:
                // that arm was **measured not to fire** on a14 2026-08-13
                // (`theme_list`'s note), and nobody found out why. Deleting it
                // removes a suspect rather than a working path.
                //
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
                // The four toggle switches (`TOGGLES`) -- `IDC_CAPS` on
                // Keyboard since Task 11, and design §3.3's three on System
                // since 2026-08-15. Reached the same way and for the same
                // reason as the `PUSH_BUTTONS` just above: pure
                // painting, no callback, cannot recurse into `apply_state`,
                // so it is answered before `suppressed()` too.
                if is_a_toggle(nm.idFrom as i32) && nm.code == NM_CUSTOMDRAW {
                    return LRESULT(toggle_custom_draw(
                        hwnd,
                        nm.idFrom as i32,
                        lp.0 as *const NMCUSTOMDRAW,
                    ));
                }
                // The transparency slider. A SIBLING arm rather than a
                // widening of any above it, because a trackbar's custom draw
                // is two-level -- `CDRF_NOTIFYITEMDRAW` then one
                // `CDDS_ITEMPREPAINT` per part -- while every other painter
                // here answers once and returns `CDRF_SKIPDEFAULT`. Sharing a
                // function would mean an `if` on the control class inside it.
                if nm.idFrom == IDC_OPACITY as usize && nm.code == NM_CUSTOMDRAW {
                    return LRESULT(slider_custom_draw(hwnd, lp.0 as *const NMCUSTOMDRAW));
                }
                // The four tab pills (Task 6). A SIBLING arm rather than a
                // widening of the push-button one above: the pills are
                // deliberately absent from `PUSH_BUTTONS` -- see `TABS` for
                // both reasons -- so `is_push_button` does not match them and
                // must not be taught to. `paint::tab_pill` is likewise a
                // sibling of `paint::button` and not a branch inside it.
                //
                // Before `suppressed()`, on the same rule as the three arms
                // above: custom draw is pure painting, it reaches no callback
                // and it cannot recurse into `apply_state`, so the guard's
                // reason does not apply. Falling through while suppressed
                // would draw the pills as ordinary themed push buttons for
                // that frame -- a visible flicker of a different control.
                if nm.code == NM_CUSTOMDRAW {
                    if let Some(opens) = page_of_tab(nm.idFrom as i32) {
                        return LRESULT(tab_pill_custom_draw(
                            hwnd,
                            opens,
                            lp.0 as *const NMCUSTOMDRAW,
                        ));
                    }
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
                // Before Task 8 this arm answered for the retired
                // `IDC_LBL_COUNT` alone
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
                // Every id below now takes the same `text` ink. `IDC_LBL_COUNT`
                // was the one exception -- `text_faint`, because a count
                // sitting beside a Subtitle heading is not body text -- and it
                // went with the control on 2026-08-15. The token itself is
                // still in use elsewhere (the disabled App combo and filter,
                // and `Mark::Unknown` notes), so no `theme::pairs` row lost
                // its subject.
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
                // **DELETED 2026-08-15: the waiting-line branch.** A branch
                // stood here answering `IDC_SYS_PLACEHOLDER` and then
                // `IDC_ABOUT_PLACEHOLDER` with the window's own `bg` rather
                // than `card`, because those two pages had no card at all --
                // behind them `compute_card_rects` left every rect at zero
                // height, so `on_card` would have painted a card-coloured
                // strip onto bare ground while `DefWindowProcW` would have
                // painted the `COLOR_3DFACE` rectangle Task 8 fixed for eight
                // controls at once. Both pages have real cards now and both
                // placeholders are retired, so the branch has no subject.
                //
                // What it discovered is worth keeping even with nothing left
                // to apply it to, because the next control drawn outside a
                // card will meet it again: those were the first strings this
                // window drew on bare ground, and `theme::apply_backdrop`
                // names exactly that as the change that reopens Mica's
                // documented hazard -- GDI text drawn straight onto glass
                // loses its alpha and fringes black. `OPAQUE` plus a `bg` fill
                // is what closed it; `TRANSPARENT` is the spelling that
                // fringes. And the high-contrast pair was `COLOR_BTNTEXT` on
                // `COLOR_BTNFACE`, same-family, unlike the cross-family pair
                // the `on_card` branch below carries its own correction about.
                //
                // **The System page's three VALUE slots**, on the card like
                // everything else on that page but in `text_muted` rather
                // than `text` -- design §7 rule 3: a fact about this machine
                // is a value, and a value beside its own label should not
                // compete with it. The pair is already covered by
                // `theme::pairs`' *muted text on card* row at the 4.5 floor,
                // so no new row was needed and moving either token stays a
                // test failure.
                //
                // `IDC_OPACITY_VALUE` is in this branch even though the label
                // half of its text is not a value: the control holds
                // `Window transparency` and `96%` in one string (see
                // `render_system` for why one control), and a STATIC has one
                // ink. Muted for both is the safe direction -- the label is
                // still 4.5 against the card, while `text` on the value would
                // make a percentage read as loud as the row it belongs to.
                //
                // **`COLOR_GRAYTEXT` under high contrast, not
                // `COLOR_WINDOWTEXT`.** It is the index the four HC schemes
                // define for de-emphasised text on `COLOR_WINDOW`, and it is
                // the pair `colours`' disabled row and `toggle`'s disabled ink
                // already use. What it must not be is the fill's own index,
                // which is the collision class that put five invisible strings
                // on screen in the last redesign.
                //
                // **About's three LABELS joined this branch on 2026-08-15,
                // and its three VALUES deliberately did not** -- the inversion
                // `role_of` carries in the type scale, carried again in the
                // ink. On System the muted half is the machine's answer; on
                // About the muted half is the signpost (`Build`,
                // `Location`) and the value is what the reader opened the
                // page for. Same two tokens, opposite halves of the row,
                // because the rows ask opposite questions.
                if matches!(
                    id,
                    IDC_CONFIG_DIR
                        | IDC_LOG_SIZE
                        | IDC_OPACITY_VALUE
                        | IDC_ABOUT_BUILD_LABEL
                        | IDC_ABOUT_LOCATION_LABEL
                ) {
                    let hdc = HDC(wp.0 as *mut core::ffi::c_void);
                    let card = theme_col(|p| p.card, COLOR_WINDOW);
                    let text = theme_col(|p| p.text_muted, COLOR_GRAYTEXT);
                    SetTextColor(hdc, text);
                    SetBkColor(hdc, card);
                    SetBkMode(hdc, OPAQUE);
                    return LRESULT(theme_brush(card).0 as isize);
                }
                // **The update check's status line (Task 9), on its own
                // branch rather than folded into `on_card` below**: its ink
                // is the one thing on this page that varies with STATE
                // rather than with role, so it cannot share the plain
                // `text`-on-`card` arm every other About VALUE uses.
                //
                // **Fill is `card` (`COLOR_WINDOW`) in every branch, exactly
                // as `on_card` below uses** -- this row draws on the same
                // card every other row on this page does, never on a tinted
                // background of its own. Ink is `card`'s own `theme::pairs`
                // partner: `text` (`COLOR_WINDOWTEXT`) at rest, and for
                // `Warn`, `p.warn` -- the SAME token `flag_colours`' `Warn`
                // arm uses for the flag pill, tested against `p.card` at the
                // 4.5 floor as `"warn note dot"` in `beckon_core::theme`'s
                // own `pairs()`, so this is not a new pair, only a new site
                // for one already proven not to collide. `Bad` is exhaustive
                // rather than folded into `Neutral` for `update_row`'s own
                // reason: this row never actually produces it, and the match
                // stays total so a future tone this page has no colour for is
                // a compile error here, not a silent default -- `p.bad`
                // against `p.card` is `"bad note dot"`, the same table, so it
                // costs nothing to cover.
                //
                // **High contrast never invents a warning colour**, on
                // `flag_colours`' own precedent: both `Warn` and `Bad` fall
                // back to `COLOR_WINDOWTEXT`, the SAME sys index `Neutral`
                // uses, so under a high-contrast theme this row reads in the
                // theme's ordinary text colour rather than in an invented
                // one -- a lost distinction, never a lost pairing, because
                // the fill fallback stays `COLOR_WINDOW` throughout.
                if id == IDC_ABOUT_UPDATE_STATUS {
                    let hdc = HDC(wp.0 as *mut core::ffi::c_void);
                    let card = theme_col(|p| p.card, COLOR_WINDOW);
                    let tone = ABOUT_UPDATE_TONE.with(|c| c.get());
                    let text = match tone {
                        FlagTone::Warn => theme_col(|p| p.warn, COLOR_WINDOWTEXT),
                        FlagTone::Bad => theme_col(|p| p.bad, COLOR_WINDOWTEXT),
                        FlagTone::Neutral => theme_col(|p| p.text, COLOR_WINDOWTEXT),
                    };
                    SetTextColor(hdc, text);
                    SetBkColor(hdc, card);
                    SetBkMode(hdc, OPAQUE);
                    return LRESULT(theme_brush(card).0 as isize);
                }
                // `IDC_NOTES` is deliberately ABSENT since Task 12: it is
                // `SS_OWNERDRAW` now, and an owner-draw static never asks
                // its parent for a background brush at all -- `draw_chip`'s
                // own controls (the seven toggle chips) are absent from this
                // list for exactly the same reason, and `push_button_custom_draw`'s
                // nine buttons never were in it either. `paint::draw_notes`
                // paints this control's background itself, through
                // `WM_DRAWITEM`.
                // Four ids left this list on 2026-08-15 with the controls
                // themselves: `IDC_GRP_EDITOR`, `IDC_LBL_APP`,
                // `IDC_LBL_SHORTCUT` (design §3.1) and `IDC_LBL_SECTION` (the
                // `Shortcuts` heading). Every STATIC that sits on a
                // card still has to be here -- falling through to
                // `DefWindowProcW` draws it as a `COLOR_3DFACE` rectangle,
                // which is the defect that once hit eight controls at once.
                // The System page's three switches and its two file-name
                // labels joined on 2026-08-15. All five sit on that page's
                // one card, so all five would otherwise be `COLOR_3DFACE`
                // rectangles -- the defect that once hit eight controls at
                // once, respelled on a new page. The three VALUE slots are
                // NOT here; they have their own branch above, in `text_muted`.
                //
                // **`IDC_OPACITY` is here too, and it is not a STATIC.** A
                // `msctls_trackbar32` asks its PARENT for a background brush
                // through `WM_CTLCOLORSTATIC`, which is the message trackbars
                // and progress bars have always used, and
                // `paint::slider_part` only ever fills the two rects comctl32
                // hands it -- the channel's and the thumb's. Everything
                // outside those two, which on a 120x20 control is most of it,
                // is erased with whatever this arm returns. Without the id
                // here it fell through to `DefWindowProcW` and the slider sat
                // in a `COLOR_3DFACE` rectangle: the defect that once hit
                // eight controls at once, reached through a control class
                // rather than through a page.
                //
                // **About's name row and its three VALUE slots joined on
                // 2026-08-15; `IDC_ABOUT_UPDATE_VALUE` joined them on
                // 2026-08-25 (Task 9), the same plain `text`-on-`card` ink
                // every other About value gets.** Its mark and its disclosure
                // did NOT and must not: both are `SS_OWNERDRAW`, so like
                // `IDC_NOTES` they never send this message at all --
                // `paint::mark` and `paint::disclosure` fill their own rects
                // with `card` first, which is the same job this arm does for
                // the others.
                let on_card = matches!(
                    id,
                    IDC_BANNER
                        | IDC_CAPS
                        | IDC_CAPS_SHORTHAND
                        | IDC_LBL_HOLD
                        | IDC_LBL_TAP
                        | IDC_PAUSE
                        | IDC_AUTOSTART
                        | IDC_DARK
                        | IDC_OPACITY
                        | IDC_CONFIG_NAME
                        | IDC_LOG_NAME
                        | IDC_ABOUT_NAME
                        | IDC_ABOUT_BUILD_VALUE
                        | IDC_ABOUT_LOCATION_VALUE
                        | IDC_ABOUT_UPDATE_VALUE
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
                    let text = theme_col(|p| p.text, COLOR_WINDOWTEXT);
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
                    PAINT_THEME.with(|c| {
                        paint::draw_notes(
                            di,
                            &body,
                            paint::NoteGround::Card,
                            &mut c.borrow_mut(),
                            dpi,
                        )
                    });
                    return LRESULT(1);
                }
                // The command bar's service line (design §6.4), on
                // `IDC_NOTES`' arrangement: the same painter, a one-element
                // slice, and `NoteGround::Window` because the bar is the
                // window's own ground rather than a card.
                //
                // **The `None` arm still paints.** The control is created
                // `WS_VISIBLE`, so a paint can arrive before the first push;
                // returning without filling would leave whatever was last in
                // that rect, which is the fault this arm exists to avoid.
                if di.CtlType == ODT_STATIC && di.CtlID as i32 == IDC_SERVICE_LINE {
                    let dpi = GetDpiForWindow(hwnd).max(96);
                    let line = SHOWN_SERVICE.with(|c| c.borrow().clone());
                    let body: Vec<Note> = line
                        .into_iter()
                        .map(|l| Note {
                            mark: l.mark,
                            text: l.text,
                        })
                        .collect();
                    PAINT_THEME.with(|c| {
                        paint::draw_notes(
                            di,
                            &body,
                            paint::NoteGround::Window,
                            &mut c.borrow_mut(),
                            dpi,
                        )
                    });
                    return LRESULT(1);
                }
                // About's two owner-draw STATICs (design §3.4). Neither is
                // reachable from `WM_CTLCOLORSTATIC` at all -- that is what
                // `SS_OWNERDRAW` costs and buys -- so each fills its own rect
                // with `card` before drawing.
                //
                // **Both read their text off the control** rather than being
                // handed it, which is why no cache appears beside
                // `SHOWN_NOTES` here: the mark's letter and the disclosure's
                // sentence are constants set at creation and never rewritten,
                // so the control IS the one copy in the process. The values
                // that DO change on this page (`…_VALUE`) are ordinary STATICs
                // and go through `set_text_if_changed`.
                if di.CtlType == ODT_STATIC && di.CtlID as i32 == IDC_ABOUT_MARK {
                    let dpi = GetDpiForWindow(hwnd).max(96);
                    PAINT_THEME.with(|c| paint::mark(di, &mut c.borrow_mut(), dpi));
                    return LRESULT(1);
                }
                if di.CtlType == ODT_STATIC && di.CtlID as i32 == IDC_ABOUT_DISCLOSURE {
                    let dpi = GetDpiForWindow(hwnd).max(96);
                    PAINT_THEME.with(|c| paint::disclosure(di, &mut c.borrow_mut(), dpi));
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
            // The transparency slider. A trackbar reports through
            // `WM_HSCROLL`, not `WM_COMMAND`, which is why this is an arm of
            // its own rather than a line in `handle_command`: the id is not
            // in `wParam` at all -- `lParam` is the control's HWND, and the
            // low word of `wParam` is the scroll code.
            //
            // **The position is read from the control, never from the code.**
            // `TB_THUMBTRACK` carries the position in `wParam`'s high word
            // and `TB_ENDTRACK` carries nothing at all, while
            // `TBM_GETPOS` answers correctly for every one of the ten codes
            // -- including the keyboard's `TB_LINEUP`/`TB_PAGEDOWN`/`TB_TOP`,
            // which is the whole reason a hand-rolled slider was not built.
            //
            // **Applied on every step, including mid-drag.** The window's own
            // alpha is what the user is judging the value by, so a control
            // that only committed on `TB_ENDTRACK` would be a slider you have
            // to let go of to see. `SetLayeredWindowAttributes` is cheap
            // enough for a drag; the registry write beside it is not free,
            // but a drag is a human gesture measured in tens of writes, not
            // thousands.
            // The `lp.0 != 0` half is not defensive dressing: a `WM_HSCROLL`
            // from a scroll BAR rather than a control carries a null `lParam`,
            // and `GetDlgCtrlID(HWND(null))` is an error whose return value is
            // 0 -- which would be indistinguishable from a control whose id
            // really is 0 if anything here had one.
            WM_HSCROLL
                if lp.0 != 0
                    && GetDlgCtrlID(HWND(lp.0 as *mut core::ffi::c_void)) == IDC_OPACITY =>
            {
                let h = HWND(lp.0 as *mut core::ffi::c_void);
                let pos = SendMessageW(h, TBM_GETPOS_MSG, None, None).0;
                let pct = beckon_core::settings::clamp_opacity(pos.clamp(0, 255) as u8);
                if let Err(e) = crate::prefs::set_opacity(pct) {
                    eprintln!("beckon: cannot store the transparency preference: {e}");
                }
                apply_current_backdrop(hwnd);
                // The readout, without `layout`: `set_text_if_changed` is a
                // `WM_SETTEXT`, and the slot's width is fixed by `layout` from
                // the constant label plus a reserved value column -- see the
                // System band there. A caption that fed `layout` would be
                // `SetWindowPos` on the populated App combo once per drag
                // step, which is the measured data-loss call
                // (`Ui::shown_external`).
                set_text_if_changed(
                    hwnd,
                    IDC_OPACITY_VALUE,
                    &opacity_slot(&beckon_core::settings::opacity_label(pct)),
                );
                with_cb(|cb| (cb.on_command)(SettingsCommand::SetOpacity(pct)));
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
                    // The System page's four glyph-button tooltips, on the
                    // same rule and for the same reason. `sys_tips` and
                    // `tip_text` are the whole set: every tooltip in this
                    // window holds a pointer into one of the five buffers
                    // these two fields own.
                    drop(ui.sys_tips);
                }
                CB.with(|c| *c.borrow_mut() = None);
                CFG.with(|c| *c.borrow_mut() = None);
                // The service line's mirror outlives the control it mirrors,
                // and `show_service` is the one push in this window that
                // RETURNS EARLY when nothing changed. So a second open --
                // the tray can open and close this window all day -- began
                // with a freshly created, empty `IDC_SERVICE_LINE` and a
                // mirror still holding the line the LAST window ended on: if
                // the two agreed, `set_text` never ran. Nothing looked wrong,
                // because `WM_DRAWITEM` paints from the mirror; but the
                // control's own window text stayed empty, which is what a
                // screen reader and `examples/settings_probe.rs`'s `dump`
                // both read. `SHOWN_NOTES` needs no such line -- `show_notes`
                // writes unconditionally.
                SHOWN_SERVICE.with(|c| *c.borrow_mut() = None);
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
    enable(hwnd, IDC_REVERT, false);
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
    enable(hwnd, IDC_REVERT, true);
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
        //
        // **`BN_KILLFOCUS` returns the ring to the DOOR's home since
        // 2026-08-15**, where it returned it to the literal `IDC_APPLY`. Tab
        // off the last button on the System door and that constant put the
        // ring on a Save that door does not draw -- the defect
        // `repair_hidden_button` exists for, reached through the Tab key
        // instead of through a door change, and reached in a state where
        // nothing afterwards would have corrected it.
        (_, c) if is_push_button(id) && (c == BN_SETFOCUS || c == BN_KILLFOCUS) => {
            let to = if c == BN_SETFOCUS {
                id
            } else {
                id_of_default_button_opt(DefaultButton::home(PAGE.with(|p| p.get())))
            };
            set_default_id(hwnd, to);
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
        // door.) `CMD_FROM_ACCELERATOR` is what `Ctrl+1`..`Ctrl+4` arrive as,
        // and it is the reason this arm does NOT filter on `BN_CLICKED`
        // alone: an accelerator's `WM_COMMAND` carries 1 in the high word
        // where a click carries 0, so a `c == BN_CLICKED` test would leave
        // four keys that silently do nothing -- the same failure mode
        // `build_accelerators` records for `Ctrl+Tab`.
        //
        // A mouse click has already moved the tick before this runs, because
        // the pills are auto-radios. An accelerator has moved nothing at all.
        // `show_page` ticks in both cases; see its doc.
        (_, c)
            if page_of_tab(id).is_some()
                && (c == BN_CLICKED || c == BN_DOUBLECLICKED || c == CMD_FROM_ACCELERATOR) =>
        {
            if let Some(page) = page_of_tab(id) {
                go_to_door(hwnd, page);
            }
        }
        // ---- `Ctrl+Tab` and `Ctrl+Shift+Tab`. Neither id belongs to a
        // control, so the accelerator table is their only sender and there is
        // no notification code to sort -- which is why this arm takes `_`
        // where the pills above take three codes.
        //
        // The direction is resolved HERE rather than in the table because it
        // depends on the door that is currently open, and an `ACCEL` carries
        // a command id and nothing else. `PAGE` is the authority for that,
        // not the pills' checked state: `show_page` writes both, but `PAGE`
        // is what `layout` reads, so reading anything else here could open a
        // door the layout does not agree with.
        (IDM_PAGE_NEXT, _) => go_to_door(hwnd, PAGE.with(|p| p.get()).next()),
        (IDM_PAGE_PREV, _) => go_to_door(hwnd, PAGE.with(|p| p.get()).prev()),
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
        // Spec F.3: `Revert` clears the combo and leaves the row without a
        // shortcut. An empty string is exactly what `Model::add_row` gives a
        // new row, so this is a state the model, the renderer and
        // `combo_view` all already handle.
        //
        // No probe: `on_probe_shortcut` asks the OS whether a chord is free,
        // and there is no chord here to ask about. That is also why this
        // does not go through `push_shortcut`, which reads the five controls
        // and sends NOTHING while no key is selected -- see `shortcut_shown`.
        (IDC_REVERT, _) => with_cb(|cb| (cb.on_edit_combo)(String::new())),
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
            // **The same rule one step further, 2026-08-15: a key must never
            // do what its button is not THERE to do.** Since design §1's store
            // split the command bar is drawn on two doors, and `Ctrl+S` is an
            // accelerator on the window, not on the page -- so from System or
            // About it arrived here and wrote `apps.toml` with no Save on
            // screen and nothing having offered to write anything. That is the
            // invisible write the four-doors pass set out to remove.
            //
            // **Inert, not "switch to Shortcuts and save".** The model stays
            // dirty and Save stays enabled, so the keystroke is not lost --
            // it is waiting on the door that owns it, one `Ctrl+1` away. A key
            // that changes doors under the user and then writes is a bigger
            // surprise than a key that does nothing, and `show_page` is a
            // route with its own focus and geometry repairs that nothing
            // should acquire a fourth caller of by accident.
            //
            // `enabled` alone would not have covered this: `apply_enabled` is
            // `dirty && no errors` and has no page term, and a hidden button
            // is not a disabled one -- the window never calls
            // `enable(false)` on a control it hides.
            if !command_bar_shown(PAGE.with(|p| p.get())) {
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
        // ---- The System page (design §3.3).
        //
        // **`Pause shortcuts` and `Reload` reach the tray's OWN `set_paused`
        // and `reload`, never a second implementation**, and the command
        // channel is what makes that structural: this window can only ask,
        // and `serve.rs` answers with the same two functions the tray menu
        // calls. `set_paused` does five ordered things -- unregister, set the
        // flag, rewrite the status phrase, CLEAR `registered`, and sync the
        // Caps hook -- and the cleared map is what makes the `paused` status
        // word load-bearing on every Shortcuts row. A window that flipped a
        // flag itself would leave nineteen rows claiming to be registered
        // while nothing was.
        //
        // `is_checked` AFTER the click, not a stored value: all three are
        // `BS_AUTOCHECKBOX`, so Windows has already flipped the state by the
        // time this notification arrives -- the same read
        // `(IDC_CAPS, _)` above makes, and the opposite of the chip arms,
        // which have to flip the state themselves first because
        // `BS_OWNERDRAW` has none.
        (IDC_PAUSE, _) => {
            let on = is_checked(hwnd, IDC_PAUSE);
            with_cb(|cb| (cb.on_command)(SettingsCommand::SetPaused(on)));
        }
        (IDC_AUTOSTART, _) => {
            let on = is_checked(hwnd, IDC_AUTOSTART);
            with_cb(|cb| (cb.on_command)(SettingsCommand::SetAutostart(on)));
        }
        // **The window applies this one itself and tells the caller after.**
        // Every other command here is something only `serve` can do; the
        // theme is this window's own look, stored in this window's own key,
        // and a round trip through `serve` would mean the switch stayed on
        // the old colours until the next push. The caller still hears about
        // it -- it owns the tray, and a future tray that wants to follow the
        // same preference should not have to read the registry to find out.
        (IDC_DARK, _) => {
            let on = is_checked(hwnd, IDC_DARK);
            if let Err(e) = crate::prefs::set_dark(on) {
                // Swallowed rather than surfaced: what is lost is that the
                // choice does not survive a restart, and a modal dialog for
                // that -- on a switch the user just flipped -- is worse than
                // the fault. The window has already changed colour.
                eprintln!("beckon: cannot store the theme preference: {e}");
            }
            unsafe { on_theme_changed(hwnd) };
            with_cb(|cb| (cb.on_command)(SettingsCommand::SetDarkMode(on)));
        }
        // **`IDC_DARK`'s arrangement, one door across**: it acts here and
        // does not go through `serve` at all. What it changes is how this
        // window SPELLS the chord, stored in this window's own key, so a
        // round trip would leave the list showing the old caps until the next
        // push -- and there may not be one, since nothing about the model
        // changed.
        //
        // **`sync_list` structurally cannot do this repaint**, which is why
        // the column is rewritten here by hand. That function diffs
        // `cells(new)` against `cells(prev)`, and both sides would be spelled
        // with the SAME (new) fold -- so every cell would compare equal and
        // nothing would be written. The rebuild path is not reachable either:
        // it keys off a changed row count, and the count is identical.
        (IDC_CAPS_SHORTHAND, _) => {
            let on = is_checked(hwnd, IDC_CAPS_SHORTHAND);
            if let Err(e) = crate::prefs::set_caps_view(on) {
                // Swallowed, on `IDC_DARK`'s reasoning: what is lost is that
                // the choice does not survive a restart, and a modal dialog
                // about that -- on a switch the user just flipped -- is worse
                // than the fault. The list has already changed.
                eprintln!("beckon: cannot store the caps-view preference: {e}");
            }
            // The borrow is taken and dropped before anything is SENT: the
            // `LVM_SETITEMTEXT` below re-enters this wndproc, and a second
            // `UI` borrow across it aborts the process.
            let rows: Option<(Vec<ListItem>, Option<Chord>)> = UI.with(|u| {
                u.borrow().as_ref().map(|ui| {
                    (
                        ui.items.clone(),
                        caps_view_fold(on, ui.caps_on, ui.caps_hold),
                    )
                })
            });
            if let Some((items, fold)) = rows {
                if let Ok(list) = unsafe { GetDlgItem(Some(hwnd), IDC_LIST) } {
                    for (i, it) in items.iter().enumerate() {
                        unsafe { set_item_text(list, i, 1, &combo_cell(it, fold)) };
                    }
                }
            }
        }
        (IDC_SYS_RELOAD, _) => with_cb(|cb| (cb.on_command)(SettingsCommand::ReloadNow)),
        // The four glyph buttons. `Open` hands the file to whatever is
        // registered for it; `Reveal` opens Explorer with it selected. Both
        // are `serve`'s to do -- `ShellExecuteW` pumps this thread's message
        // queue, so it must not run from inside a notification with a borrow
        // alive, which is the rule this whole file states for
        // `backend.beckon()`.
        (IDC_CONFIG_OPEN, _) => {
            with_cb(|cb| (cb.on_command)(SettingsCommand::Open(Target::Config)))
        }
        (IDC_CONFIG_SHOW, _) => {
            with_cb(|cb| (cb.on_command)(SettingsCommand::Reveal(Target::Config)))
        }
        (IDC_LOG_OPEN, _) => with_cb(|cb| (cb.on_command)(SettingsCommand::Open(Target::Log))),
        (IDC_LOG_SHOW, _) => with_cb(|cb| (cb.on_command)(SettingsCommand::Reveal(Target::Log))),
        // ---- The About page (design §3.4).
        //
        // **The three copy buttons are done HERE and reported after**, which
        // is `IDC_DARK`'s arrangement rather than the file rows'. Two reasons,
        // and the second is why it cannot be the other way round:
        //
        // 1. The clipboard is this window's own OS surface, the way the theme
        //    preference is its own store. `serve` has nothing to add.
        // 2. **`serve` does not have the strings and must not build them.**
        //    `SettingsCommand` is `Copy` and carries no `String` by design
        //    (see its own doc), so the caller would have to reconstruct
        //    `AboutState` to answer -- a second author for a page this window
        //    already renders, and the two would disagree the first time one
        //    was edited.
        //
        // The command is still raised, and it is a NOTIFICATION rather than a
        // request: the caller owns the tray and may want to say something.
        // `serve.rs`'s arm for it is deliberately empty.
        //
        // `about_now()` rather than a cached `AboutState`: everything it reads
        // is cheap (`current_exe`, one `stat`) and re-asking cannot go stale
        // between the click and the copy. What it copies is
        // `beckon_core::settings::copy_text`'s decision -- the row's bare
        // payload, never the annotated string on screen.
        (IDC_ABOUT_BUILD_COPY, _) => copy_about_field(Field::Build),
        (IDC_ABOUT_LOCATION_COPY, _) => copy_about_field(Field::Location),
        // The upgrade command's own copy button (Task 9). `copy_about_field`
        // generalises over `Field` already -- this is the SAME path the
        // three rows above use, routing `Field::UpdateCommand` rather than a
        // hand-rolled clipboard write, which is the whole point: Task 8
        // first wrote a window-local pasteboard write on the macOS twin and
        // that was corrected specifically so both doors consume one core
        // decision (`copy_text`'s `Field::UpdateCommand` arm). Reachable only
        // while `IDC_ABOUT_UPDATE_COPY` is enabled -- `render_about` disables
        // it whenever `st.update.command` is `None` -- but the arm does not
        // itself re-check that: a disabled button does not reach
        // `handle_command` at all, on the same fact `Save`'s own doc rests
        // on.
        (IDC_ABOUT_UPDATE_COPY, _) => copy_about_field(Field::UpdateCommand),
        // `Check now` (Task 9). Raised as a notification, not answered here:
        // the check itself runs in `serve`, which owns the tray and the
        // synchronous curl call -- this window has neither. `serve.rs`'s arm
        // for `CheckForUpdates` is `check_for_updates(&st)`.
        (IDC_ABOUT_CHECK_NOW, _) => with_cb(|cb| (cb.on_command)(SettingsCommand::CheckForUpdates)),
        // The three links, through the command channel for the file rows'
        // reason exactly: `ShellExecuteW` performs an out-of-process shell
        // activation and PUMPS this thread's message queue, so it must not run
        // from inside a notification with a `RefCell` borrow alive. Routing
        // through `SettingsCommand` is also what keeps them off `Callbacks` --
        // `beckon-macos/examples/settings_probe.rs` builds that struct as a
        // complete literal with no `..`, so three new fields would be a hard
        // E0063 on a CI job that has nothing to do with this page.
        (IDC_ABOUT_GITHUB, _) => {
            with_cb(|cb| (cb.on_command)(SettingsCommand::Open(Target::Github)))
        }
        (IDC_ABOUT_RELEASES, _) => {
            with_cb(|cb| (cb.on_command)(SettingsCommand::Open(Target::Releases)))
        }
        (IDC_ABOUT_BUG, _) => {
            with_cb(|cb| (cb.on_command)(SettingsCommand::Open(Target::BugReport)))
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
    /// door, and none is one of the `PUSH_BUTTONS` -- so a test on
    /// buttons alone cannot reach any of them, and a switch taken with focus
    /// on one would leave `GetFocus` on an off-screen control. On `IDC_APP`
    /// that is not merely invisible typing: no `CBN_KILLFOCUS` means no
    /// `commit_fields`, so the text reaches no model either.
    ///
    /// **What this test still cannot see, and it is the half that was wrong for
    /// a commit:** `IDC_APP` is a `CBS_DROPDOWN`, so `GetFocus` returns its
    /// inner EDIT rather than the COMBOBOX in this table, and whether
    /// `hidden_child` recognises that EDIT depends on `IsWindowVisible` versus
    /// the control's own style bit -- a distinction no id table can express and
    /// no host but Windows can run. Gate G-S5 is where it gets checked.
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

    /// No door opens onto nothing.
    ///
    /// This is the whole of what Task 7 shipped, stated as the property
    /// rather than as the two ids: before it, System and About had no row in
    /// `PAGE_CONTROLS` at all, so both doors led to a surface with the strip
    /// and the command bar on it and nothing in between -- which reads as a
    /// window that failed to draw, not as a page that is waiting. Nothing
    /// else catches that: `every_control_belongs_to_exactly_one_group` in
    /// `ids.rs` partitions the CONTROLS across the table, so it is silent
    /// about a page that owns none of them, and the placeholders are the one
    /// kind of control whose whole purpose is to be deleted later.
    ///
    /// **The door list is walked with `Page::next` until it comes home**, not
    /// written out as four literals: a fifth door added to the cycle would
    /// otherwise be tested by nobody, which is the same failure one level up.
    /// The loop is bounded independently of the walk so a `next` that never
    /// returns cannot hang the suite -- it fails instead.
    #[test]
    fn every_door_owns_at_least_one_control() {
        let mut page = Page::Shortcuts;
        let mut seen = 0;
        loop {
            assert!(
                PAGE_CONTROLS.iter().any(|(_, p)| *p == page),
                "no control is behind {page:?}, so that door opens onto an \
                 empty surface between the strip and the command bar"
            );
            page = page.next();
            seen += 1;
            if page == Page::Shortcuts {
                break;
            }
            assert!(seen < 64, "`Page::next` never came home");
        }
        assert_eq!(
            seen,
            TABS.len(),
            "the cycle visits {seen} doors and the strip has {} pills",
            TABS.len()
        );
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

    /// Why `show_page` may hand its own pill to `repair_hidden_button` as the
    /// successor, and why `IDC_CLOSE` was the wrong one to inherit.
    ///
    /// A door change moves focus off whatever it has just hidden, so the
    /// place it moves focus TO has to survive that same switch: a successor
    /// behind a door could be the very control being hidden. The strip is
    /// chrome -- absent from `PAGE_CONTROLS`, so `show_page_controls` never
    /// touches it -- and that is what this pins.
    ///
    /// The second half is the contrast at the bottom. `IDC_CLOSE` survives
    /// the switch too, so (a) alone does not separate the two; what does is
    /// that it is a push button, so the ring follows focus onto it and Enter
    /// one keystroke after `Ctrl+2` closed the window. The pills' side of
    /// that is `a_tab_pill_is_never_a_push_button`.
    #[test]
    fn the_successor_a_door_names_is_its_own_pill() {
        for (id, page, _) in TABS {
            assert_eq!(tab_id_of(page), id, "show_page passes tab_id_of(page)");
            assert_eq!(
                page_of_control(id),
                None,
                "pill {id} is behind a door, so the switch that names it as a \
                 successor could be hiding the control it moves focus to"
            );
        }
        assert!(
            is_push_button(IDC_CLOSE),
            "the old successor was safe to press, not free of the ring -- if \
             this ever stops holding, re-read why the door change stopped \
             using it"
        );
    }

    /// The badge slot is reserved on exactly the pill that draws a badge.
    ///
    /// **Two functions decide that independently and they must not disagree.**
    /// `layout` adds `badge_slot_w` to a pill's width when its `TABS` entry
    /// opens `BANNER_PAGE`; `tab_pill_custom_draw` passes `Some(count)` under
    /// the identical test. A slot reserved on a pill that draws no badge is a
    /// caption pushed left of centre for good; a badge drawn into a pill with
    /// no slot lands on top of the caption. Neither is visible from a
    /// non-Windows host, and both are one edited constant away.
    ///
    /// It also pins that there IS one -- `BANNER_PAGE` has to name a door the
    /// strip really has, or the badge is reserved and drawn nowhere -- and
    /// that `BADGE_SLOT` measures to something. An empty slot string would
    /// leave `badge_slot_w` at one `tok::GAP`, which is a badge drawn over the
    /// caption rather than beside it.
    #[test]
    fn the_badge_slot_is_reserved_on_the_pill_that_draws_it() {
        let carriers: Vec<i32> = TABS
            .iter()
            .filter(|(_, page, _)| *page == BANNER_PAGE)
            .map(|(id, _, _)| *id)
            .collect();
        assert_eq!(
            carriers,
            vec![IDC_TAB_SHORTCUTS],
            "the badge and its reserved slot are decided by `opens == \
             BANNER_PAGE` in two places; exactly one pill must answer it"
        );
        assert!(
            !BADGE_SLOT.is_empty() && BADGE_SLOT.chars().all(|c| c.is_ascii_digit()),
            "the slot is reserved by MEASURING this string in the badge's own \
             face, so it has to be digits and it has to be some"
        );
        assert!(
            BADGE_SLOT.len() >= 4,
            "four digits is the reservation; fewer starts clipping counts a \
             real config can reach"
        );
    }

    /// `Ctrl+1`..`Ctrl+4` are built by walking `TABS` and adding `i` to
    /// `VK_1`, which assumes the four digit virtual-key codes are a
    /// contiguous ascending run. They are (`VK_1` is 0x31, the ASCII code),
    /// but nothing in `build_accelerators` says so out loud, and the failure
    /// if that ever stopped holding is a key bound to the wrong door rather
    /// than a key bound to nothing.
    ///
    /// It also pins the count: four doors, four digits, and `Ctrl+5` is not a
    /// key this window answers.
    #[test]
    fn the_digits_match_the_strip() {
        use windows::Win32::UI::Input::KeyboardAndMouse::VK_4;
        assert_eq!(
            VK_4.0 - VK_1.0,
            3,
            "the digit keys are not a contiguous run, so `VK_1.0 + i` names \
             the wrong key"
        );
        assert_eq!(
            TABS.len(),
            (VK_4.0 - VK_1.0 + 1) as usize,
            "the strip has {} doors and Ctrl+1..Ctrl+4 is four keys",
            TABS.len()
        );
    }

    /// `Page::next` and `Page::prev` walk the strip in the order the strip is
    /// drawn in.
    ///
    /// Core owns the cycle (so `Ctrl+Tab`'s answer is testable on all three
    /// CI jobs) and this file owns `TABS` (so the pills' ids and captions sit
    /// beside each other). They are the same fact spelled twice, and this is
    /// the only place both are visible -- core cannot see `TABS`, and a
    /// disagreement would send `Ctrl+Tab` to a door the strip draws somewhere
    /// else, with every pill still lighting correctly.
    #[test]
    fn the_strip_order_is_the_cycle() {
        for (i, (_, page, _)) in TABS.iter().enumerate() {
            let next = TABS[(i + 1) % TABS.len()].1;
            assert_eq!(page.next(), next, "Ctrl+Tab leaves {page:?} for {next:?}");
            assert_eq!(next.prev(), *page, "Ctrl+Shift+Tab disagrees at {page:?}");
        }
    }

    /// The seam between `Ui::defid` (a control id) and the pure decision (an
    /// enum). It carries the whole default-button fix, and a mapping that
    /// disagreed with itself would be silent: the ring would simply stop
    /// moving, exactly as it did before the fix existed.
    #[test]
    fn every_push_button_round_trips_through_the_default_button_enum() {
        for id in PUSH_BUTTONS {
            // **Stronger than it was.** This read
            // `id_of_default_button(default_button_of(id))`, and while
            // `default_button_of` ended `_ => HOME` an id missing from its
            // table came back as `Save` -- so the round trip failed loudly for
            // nineteen ids and SILENTLY for `IDC_APPLY`, the one that reached
            // the catch-all. `Option` makes the gap an assertion.
            assert_eq!(
                default_button_of(id).map(id_of_default_button),
                Some(id),
                "control {id} does not survive the round trip"
            );
        }
        for b in DefaultButton::ALL {
            assert_eq!(default_button_of(id_of_default_button(b)), Some(b));
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
            assert_eq!(default_button_of(id), None);
        }
    }

    /// The mapping names a button or it names none. `GetDlgCtrlID` returns 0
    /// for the parent window and comctl32 gives a combo box's inner EDIT an id
    /// of its own choosing -- both reach `default_button_of`.
    ///
    /// **RENAMED and inverted 2026-08-15**, from
    /// `an_id_that_is_not_a_push_button_reads_as_home`. It asserted that an
    /// unknown id reads as `HOME`, which was `Save` -- true then, and the
    /// reason `IDC_APPLY` needed no arm of its own. With `home` a function of
    /// the door there is no constant left to fall back TO, so the honest
    /// answer for an id nobody claims is that nobody claims it.
    #[test]
    fn an_id_that_is_not_a_push_button_names_no_button() {
        assert_eq!(default_button_of(NO_DEFAULT), None);
        assert_eq!(default_button_of(IDC_CAPS), None);
        assert_eq!(default_button_of(-1), None);
        // The two spellings of "nowhere" have to agree, since `Ui::defid`
        // holds one and the decision layer returns the other.
        assert_eq!(id_of_default_button_opt(None), NO_DEFAULT);
        assert_eq!(
            id_of_default_button_opt(DefaultButton::home(Page::Shortcuts)),
            IDC_APPLY
        );
        assert_eq!(
            id_of_default_button_opt(DefaultButton::home(Page::System)),
            NO_DEFAULT
        );
    }

    /// `NO_DEFAULT` must not collide with a real control, or clearing the ring
    /// would demote-and-promote some button nobody asked about. The other half
    /// -- that no DECLARED id equals it -- is `ids.rs`'s
    /// `the_no_default_id_is_not_a_declared_control`, where the id table is.
    #[test]
    fn the_no_default_id_is_not_a_push_button() {
        assert!(!is_push_button(NO_DEFAULT));
    }

    /// Why `show_page` needs `focus_the_open_door` on top of
    /// `repair_hidden_button`.
    ///
    /// Hiding the focused control hands focus to the WINDOW, and the repair's
    /// button arm asks `is_push_button(GetDlgCtrlID(focus))` -- which is
    /// `is_push_button(0)`, because a window that is nobody's child has no
    /// control id. Its other arm cannot fire either (`IsChild` is false for a
    /// window against itself), so the repair returns having moved nothing and
    /// Tab is dead until the user clicks.
    ///
    /// Both halves are Win32 behaviour this host cannot run. The half that IS
    /// testable is the id table's, and it is the half an edit could silently
    /// change: put `0` in `PUSH_BUTTONS` -- as a sentinel, say -- and the
    /// repair would start moving focus off a legitimately parent-focused
    /// window on every `apply_state` push, which is exactly the widening
    /// `focus_the_open_door` exists to avoid.
    ///
    /// The successor it sends focus to is pinned separately, by
    /// `the_successor_a_door_names_is_its_own_pill`.
    #[test]
    fn the_window_itself_is_not_a_push_button() {
        assert!(
            !is_push_button(0),
            "GetDlgCtrlID answers 0 for the parent window; if that id is a \
             push button, `repair_hidden_button` starts repairing a window \
             that holds its own focus legitimately"
        );
        for (id, page, _) in TABS {
            assert_ne!(
                id, 0,
                "0 is the parent's own id; a pill carrying it is a successor \
                 `GetDlgItem` can never resolve"
            );
            assert_eq!(
                tab_id_of(page),
                id,
                "focus_the_open_door looks up tab_id_of"
            );
        }
    }
}
