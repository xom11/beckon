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
//! `Ui::shown_external` plus `Ui::shown_empty`, which make that layout
//! conditional; see `docs/superpowers/measurements/2026-08-11-landing-1-a14.md`
//! sections 24-26.
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
use beckon_core::settings::{default_button, ControlState, DefaultButton, ListItem, Mark};
use beckon_core::shortcuts::{
    combo_display, combo_view, key_table, CapsTap, Chord, Combo, ComboView,
};
use std::cell::RefCell;
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM};
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
    EnableWindow, GetFocus, IsWindowEnabled, SetFocus,
};
use windows::Win32::UI::WindowsAndMessaging::*;

/// `SS_LEFT` is 0 and `windows` 0.61 does not export it as a constant.
const SS_LEFT_STYLE: WINDOW_STYLE = WINDOW_STYLE(0);

/// `SS_NOPREFIX` (0x0080), which `windows` 0.61 does not export either.
///
/// A STATIC treats `&` in its text as a mnemonic marker and draws the next
/// character underlined instead of drawing the ampersand. `IDC_NOTES` is the
/// one control in this window whose text comes from the CATALOG rather than
/// from us -- Start Menu display names really do contain `&` (`Notes & To
/// Do`, `Arts & Crafts`) -- so without this an app name renders as a
/// mangled, underlined string that looks like a beckon bug.
///
/// **`SS_ENDELLIPSIS` is deliberately NOT here.** The three ellipsis styles
/// force a static onto ONE line with no word wrap (documented on Static
/// Control Styles, and the reason is that the control switches to a
/// single-line DrawText path). `IDC_NOTES` is a multi-line strip -- several
/// `\r\n`-joined note lines, on the line `notes_height` sizes inside the
/// editor group -- so adding the style would collapse the whole notes band to
/// its first line. Ellipsised multi-line text needs an owner-draw `DrawText`
/// with `DT_WORDBREAK | DT_END_ELLIPSIS`, which is not this landing.
const SS_NOPREFIX_STYLE: WINDOW_STYLE = WINDOW_STYLE(0x0080);

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

const IDC_LIST: i32 = 1001;
const IDC_COMBO: i32 = 1002;
const IDC_APP: i32 = 1003;
const IDC_NOTES: i32 = 1004;
const IDC_ADD: i32 = 1005;
const IDC_REMOVE: i32 = 1006;
const IDC_APPLY: i32 = 1007;
const IDC_CAPS: i32 = 1008;
// 1009-1011 were the three `Tapping Caps alone` radios. They are free
// again -- unlike 1001-1008, 1012 and 1013, which `examples/settings_probe.rs`
// hard-codes -- but nothing should reclaim them: a probe built against an
// older binary would find a control it thinks it recognises.
const IDC_OPENFILE: i32 = 1012;
const IDC_CLOSE: i32 = 1013;
const IDC_BANNER: i32 = 1014;
const IDC_RELOAD: i32 = 1015;
const IDC_KEEPMINE: i32 = 1016;
// Labels and the group box need real ids, not -1: `layout` positions
// controls through `GetDlgItem`, and every -1 resolves to the same first
// match, so sharing one id left all but the first stacked at the origin.
const IDC_LBL_SHORTCUT: i32 = 1017;
const IDC_LBL_APP: i32 = 1018;
const IDC_GRP_KEYBOARD: i32 = 1019;
/// The `Shortcuts` heading in band 2. New ids go ABOVE the existing range:
/// 1001-1007 and the class name are hard-coded in
/// `examples/settings_probe.rs` and are fixed points.
const IDC_LBL_SECTION: i32 = 1020;
const IDC_FILTER: i32 = 1021;
/// The keyboard row: three `Hold` chips, the `Tap` combo, and the two
/// static words that name each half. `Hold` and `Tap` are the only two
/// things Caps can do, so the row names both rather than making the second
/// an afterthought of the first -- which is what the radios did, by gluing
/// the question onto the first answer.
const IDC_HOLD_CTRL: i32 = 1022;
const IDC_HOLD_WIN: i32 = 1023;
const IDC_HOLD_ALT: i32 = 1024;
const IDC_TAP: i32 = 1025;
const IDC_LBL_HOLD: i32 = 1026;
const IDC_LBL_TAP: i32 = 1027;
/// The editor strip's four modifier chips. `IDC_COMBO` (1002) keeps its
/// number beside them and changes CLASS instead: it is the id
/// `examples/settings_probe.rs` hard-codes for "the shortcut control", so
/// reusing it is what keeps that probe pointed at the right thing, and
/// retiring it would leave the probe reading a control that no longer
/// exists.
const IDC_MOD_CTRL: i32 = 1028;
const IDC_MOD_WIN: i32 = 1029;
const IDC_MOD_ALT: i32 = 1030;
const IDC_MOD_SHIFT: i32 = 1031;
/// The editor strip's two commands. `Record` arms the `WH_KEYBOARD_LL`
/// capture and reads `Stop` while it is armed; `Reset` clears the row's
/// combo and leaves it without a shortcut.
const IDC_RECORD: i32 = 1032;
const IDC_RESET: i32 = 1033;

/// The editor group box. Its caption says which row is being edited, so the
/// two lines inside it read as one thing rather than as seven controls that
/// happen to share a band.
///
/// 1034 because 1033 is the current maximum and 1001-1007 are pinned by
/// `examples/settings_probe.rs`. A group box is not operable, so it carries
/// no mnemonic and no entry in `mod cap`'s collision table.
const IDC_GRP_EDITOR: i32 = 1034;

/// The count beside the `Shortcuts` heading -- `· 18 bindings`.
///
/// **A second STATIC rather than a longer caption**, because the two are
/// different type: B draws the heading at Subtitle and the count small and
/// grey, and one STATIC has one font. It is also the only control in the
/// window with a colour of its own, which `WM_CTLCOLORSTATIC` supplies by
/// id -- see that arm.
///
/// It counts what the LIST is showing, so under a filter it says how many
/// rows are on screen rather than how many the file holds. That is the
/// honest reading of a number sitting on top of the list it describes.
const IDC_LBL_COUNT: i32 = 1035;

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
/// **ASCII hyphen, not an em-dash**, for the reason `mark_glyph` already
/// gives -- this window inherits the shell's text face, and a glyph it does
/// not carry draws as a box that reads like a rendering bug rather than as
/// information. beckon has been bitten by exactly this once already: a UTF-8
/// em-dash written to a `serve --log` came back as `?"` through Windows
/// PowerShell 5.1's `Get-Content`.
///
/// The FILE NAME, not the path: `serve` can be pointed anywhere and nothing
/// on screen used to say where, but a full path in a title bar is truncated
/// from the right by every taskbar and Alt-Tab label there is -- i.e. it
/// loses precisely the file name it was there to show. The path goes in the
/// `Open config file` tooltip instead, where there is room for it.
fn title_base(config_path: &str) -> String {
    match std::path::Path::new(config_path).file_name() {
        Some(f) => format!("beckon - {}", f.to_string_lossy()),
        None => "beckon".to_string(),
    }
}

/// Layout tokens, at 96 DPI. Every one of them goes through `scale`.
///
/// Two need their reasoning, because they look like they contradict the
/// a14 measurements (`docs/superpowers/measurements/2026-08-11-landing-1-a14.md`)
/// and do not:
///
/// - **`CTL` is 32, not the measured 22.** `BCM_GETIDEALSIZE` returns the
///   smallest box the theme can draw a caption in — a floor, not a layout
///   recommendation. The measurement's job was to prove 32 does not clip,
///   and it does not.
/// - **There is no list-row token.** 29 px measured at 144 DPI is 19.33 at
///   96, and a non-integer is the tell that comctl32 derives the row
///   height from the font at the live DPI. A 96-DPI token pushed through
///   `scale` would be wrong at every non-integer scale and would break
///   again the moment the font changes, so `list_row_height` asks the
///   control instead.
mod tok {
    /// Surface padding — the margin between the client rect and content.
    pub const PAD: i32 = 16;
    /// Between two bands.
    pub const BAND: i32 = 14;
    /// Between two controls inside one band.
    pub const GAP: i32 = 8;
    /// A label and the control it names.
    pub const LABEL: i32 = 12;
    /// Height of one band line, and of every button on it.
    pub const CTL: i32 = 32;
    /// A button is never narrower than this, nor than its own caption.
    pub const BTN: i32 = 88;
    /// The right-aligned `Shortcut` column, the editor field under it, and
    /// the key list's ceiling.
    ///
    /// The key list used to have a token of its own (`KEY_COL`, 140),
    /// derived rather than designed: band 4 was one line, the App combo
    /// absorbed whatever the other six controls left, and 60 px had to come
    /// from somewhere to pay for `Record` and `Reset` sharing that line.
    /// With App on a line of its own there is nothing left to starve, so the
    /// key list is back under this ceiling and the arithmetic is retired.
    pub const SHORTCUT_COL: i32 = 200;
    /// A modifier chip is never narrower than this, nor than its own caption
    /// plus `glyph` -- direction B's `.wtog { min-width:46px }`.
    ///
    /// It exists because `Alt` is three characters and `Shift` is five, and
    /// a row of keys whose widths follow the length of their letters does not
    /// read as a keyboard. Only `Alt` is actually short enough to hit the
    /// floor at 96 DPI; the other six size themselves.
    pub const CHIP_MIN: i32 = 46;
    /// List rows visible without scrolling.
    pub const ROWS: i32 = 8;
    /// Widest a tooltip may draw before it wraps. Comfortably narrower than
    /// `MIN_WIDTH`, so the balloon never overhangs the window that owns it,
    /// and wide enough that an ordinary `%APPDATA%` config path takes two
    /// lines rather than six.
    pub const TOOLTIP_MAX: i32 = 420;
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
// 860 is spec B.2's stated width.
//
// **640's original justification is spent.** It was raised from 560 so the
// notes band -- the flex band at the time -- would fit four lines at 96 DPI,
// on the reasoning that the band took `kb_y`'s leftover directly and every
// pixel added here became a pixel of notes room. The notes are a fixed line
// inside the editor group now (`notes_height`), so the leftover no longer
// lands on them: at 860x640 the list reaches its full `tok::ROWS` and the
// surplus -- **74 px at 96 DPI, 113 at 150 %** -- is slack between the editor
// group and the keyboard group. The number is left alone here because
// re-picking it is a question about what the window should look like at rest,
// not arithmetic, and nothing on this machine can see the answer.
//
// **Those two figures were re-derived against the SHIPPED `notes_height`.**
// They read 78 / 119 when this comment was first written, which was Task 6's
// 32 px notes stub; Task 9's real body costs 36 px at 96 and 54 at 144, so
// `grp_h` grew 4 / 6 and the clearance lost exactly that. Neither the window
// size nor any token moved -- only the number written down here. The
// clearance, with the banner hidden and the list unclamped, is
//
//   clearance = kb_y - (grp_y + grp_h)
//             = h - 2*pad - 5*ctl - 2*band - 2*s(24) - 5*gap - list_h - notes_h
//
// @96, h = 640 - 39 non-client = 601, and pad/ctl/band/gap = 16/32/14/8:
//   bar_y 553, kb_h 64, kb_y 475; y after band 2 = 56; notes_h 36,
//   grp_h 148; want = 21 + 20*8 + 2 = 183 and room 405, so list_h = 183;
//   grp_y 253, group bottom 401 -> 475 - 401 = **74**.
// @144 (150 %), h = 960 - 58 non-client = 902, tokens 24/48/21/12:
//   bar_y 830, kb_h 96, kb_y 713; y after band 2 = 84; notes_h 54,
//   grp_h 222; want = 31 + 30*8 + 2 = 273 and room 608, so list_h = 273;
//   grp_y 378, group bottom 600 -> 713 - 600 = **113**.
// Row and header heights are `list_row_height` / `list_header_height`'s own
// fallbacks, the same honest numbers `MIN_HEIGHT`'s table derives from.
// Simulated, not seen: nothing on the machine this was written on can display
// the window.
const WINDOW_WIDTH: i32 = 860;
const WINDOW_HEIGHT: i32 = 640;

/// Minimum resize size, at 96 DPI, enforced in `WM_GETMINMAXINFO` through
/// `ptMinTrackSize` — so both are WINDOW dimensions, caption and frame
/// included, never client ones.
///
/// **This is no longer "the point where `layout` starts overlapping
/// controls".** That is what this comment used to say, and it stopped being
/// true when band 4 became a fixed-height group: every subtraction in
/// `layout` is clamped, and band 3 gives up **its own** height — `list_h`,
/// the one flexing figure in the window — before anything below it moves, so
/// a window dragged past this floor produces a list with fewer rows —
/// eventually none — rather than two controls in the same place. What the
/// floor buys is that **the list is still worth looking at**.
///
/// (`editor_min` is not that height and must not be read as it: it is what
/// band 3 RESERVES for band 4 before choosing `list_h`, and it equals
/// `grp_h`, band 4's own height. The distinction earns its ink here because
/// this block is the derivation everything vertical is checked against.)
///
/// `MIN_WIDTH` is spec B.2's number and clears both zero points this file
/// computes — band 2's heading at a raw client width of ~332, band 4's key
/// list at ~519 — by a wide margin. Compared like for like, client against
/// client: a 720 px window with a 16 px frame has `w = 704`, so the two
/// margins are ~372 px and ~185 px. Both **shrink** as the OS frame grows,
/// so each is a ceiling on the margin rather than a floor under it.
///
/// `MIN_HEIGHT` is derived, at 96 DPI, from the smallest client height at
/// which band 3 still shows **four** rows — half of `tok::ROWS` — **with the
/// external-change banner up**. Four is enough to see a selection with a row
/// of context above and below it; a window whose list shows one row is not a
/// smaller version of this window, it is a broken one.
///
/// ```text
///   pad                                          16
///   band 1  banner, ctl                          32
///           band                                 14
///   band 2  head, ctl                            32
///           gap                                   8
///   band 3  header  (list_header_height, 21)      21
///           4 * row (list_row_height, 20)         80
///           border  (2 * SM_CYBORDER)              2
///   band                                         14
///   band 4  caption inset s(24)                   24
///           App line, ctl                         32
///           gap                                    8
///           Shortcut line, ctl                    32
///           gap                                    8
///           notes  (`notes_height`)               36
///           bottom inset, gap                      8
///   band                                         14
///   band 6  kb_h = s(24) + ctl + gap              64
///   band                                         14
///   band 7  command bar, ctl                      32
///   pad                                          16
///                                              ----
///   client                                      507
///   caption + frame at 96 DPI (SM_CYCAPTION 23
///     + 2*SM_CYSIZEFRAME + 2*SM_CXPADDEDBORDER)   39
///                                              ----
///   window                                      546
/// ```
///
/// Shipped as 550. The four pixels are slack against a non-client area the
/// OS sizes, not a fudge of the derivation — and the whole constant is scaled
/// linearly by `scale(MIN_HEIGHT, dpi)` rather than re-derived per DPI, so it
/// was never exact at 150 % either. Erring high costs nothing; erring low
/// costs list rows.
///
/// **This constant moved with `notes_height`, as documented on that
/// function, and this is that move.** Task 6 SHIPPED 546 (the old constant,
/// not the raw figure in the table above -- Task 6's own raw derivation was
/// 542; that this task's new raw also lands on 546 is coincidence, not the
/// same number carried forward) against the stub's assumed 32 px notes
/// line; the real body costs 36, four pixels more, and
/// this table -- and the constant -- carry that four pixels through rather
/// than absorbing it as slack. 546 → 550 is that difference and nothing
/// else: every other row in the table is unchanged from Task 6's derivation.
///
/// The two row figures are `list_row_height` / `list_header_height`'s own
/// 96-DPI fallbacks. They are the honest numbers to derive from: comctl32
/// picks the real ones from the live font at the live DPI, which is exactly
/// why neither is a token.
///
/// **Band 1 is in the table, and that is what the number is for.** The banner
/// contributes no height until the config file moves under us, so reserving
/// its `ctl + band` costs 46 px of floor for a state that is normally absent
/// — but the state it pays for is exactly the one in which the window is
/// least disposable, and the alternative was measured: at a floor derived
/// without band 1 (500), raising the banner took the list from four rows to
/// **one**, which the paragraph above calls broken. Nothing overlapped there;
/// the failure was a useless window, not a corrupt one, and that is the
/// standard this constant is held to.
///
/// So the floor buys **four rows with the banner up, six without it**, at
/// both 96 DPI and 150 %; the editor group clears the keyboard group by
/// exactly one `band` in all four cases -- simulated at the new floor the
/// same way Task 6 simulated the old one: 720x550 @96 gives 103+4=107 px of
/// list under the banner (4 rows, 4 px of a fifth row's worth of slack, same
/// shape Task 6's own 103→107 had) and 14 px of clearance; 1080x825 @144
/// gives 161 px of list (4 rows) and 21 px of clearance. Simulated, not
/// seen — nothing on the machine this was written on can display the
/// window.
const MIN_WIDTH: i32 = 720;
const MIN_HEIGHT: i32 = 550;

/// One of §B.3's three type roles. There is no fourth: the `Keys` role the
/// spec table also lists belongs to keycap rendering, which this window
/// does not do -- a combo is four check boxes and a list of plain key
/// names, all of them Body.
#[derive(Clone, Copy)]
enum Role {
    Subtitle,
    Body,
    Caption,
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
        // Secondary prose, at Caption size. The banner is deliberately NOT
        // here: it announces that the file moved under us, which is the
        // least appropriate text in the window to shrink. `IDC_LBL_COUNT`
        // joins because B draws the count small and grey beside a Subtitle
        // heading -- one STATIC has one font, which is the whole reason it
        // is a second control.
        IDC_NOTES | IDC_LBL_COUNT => Role::Caption,
        // Everything the user reads or operates: the ListView, the filter
        // EDIT, the App / key / Tap COMBOBOXes, their labels, every BUTTON
        // (push, check, and the group box), the banner -- and anything added
        // later that does not say otherwise.
        _ => Role::Body,
    }
}

/// The three live `HFONT`s. `Copy`, so `LayoutHandles` stays `Copy` and the
/// abort-class rule below keeps holding.
#[derive(Clone, Copy)]
struct Fonts {
    subtitle: HFONT,
    body: HFONT,
    caption: HFONT,
}

impl Fonts {
    fn get(self, role: Role) -> HFONT {
        match role {
            Role::Subtitle => self.subtitle,
            Role::Body => self.body,
            Role::Caption => self.caption,
        }
    }

    fn for_id(self, id: i32) -> HFONT {
        self.get(role_of(id))
    }

    /// Release all three.
    ///
    /// Only ever called AFTER the controls have been told about their
    /// replacements -- deleting a font that is still selected into a DC is
    /// undefined. Landing 1 established this discipline for one font
    /// because one `HFONT` was leaking per window open; three roles means
    /// three leaks if only one of them is freed.
    ///
    /// Deduplicated because the total-failure path hands every role the
    /// same stock handle. `DeleteObject` on a stock object is documented
    /// harmless, but "harmless twice" is not a property worth relying on.
    unsafe fn delete(self) {
        let all = [self.subtitle, self.body, self.caption];
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

/// Everything the window reports back. The caller owns all policy: what an
/// edit means, whether a close is allowed, what Apply writes.
pub struct Callbacks {
    /// A row became current. The index is a **model** row -- the window has
    /// already mapped it through `ListItem::row`, because the ListView only
    /// ever knows the position within the filtered list it was given.
    pub on_select: Box<dyn FnMut(usize)>,
    /// A row's tick changed: `(model row, ticked)`. Independent of
    /// `on_select` -- one click can raise both, and neither implies the
    /// other.
    pub on_mark: Box<dyn FnMut(usize, bool)>,
    pub on_edit_combo: Box<dyn FnMut(String)>,
    /// The five shortcut controls now spell a whole chord: find out whether
    /// anything else already has it.
    ///
    /// Separate from `on_edit_combo`, and raised FIRST, for two reasons that
    /// are both about not lying:
    ///
    /// 1. **It is a global OS mutation**, however brief -- one
    ///    `RegisterHotKey` round trip -- so it must be raised by a change to
    ///    the shortcut and by nothing else. `on_edit_combo` is also sent by
    ///    `commit_fields` (an App-field focus loss, a Save), where the chord
    ///    has not moved and there is nothing to find out; and `apply_state`
    ///    pushes data on every keystroke, which `push_shortcut`'s
    ///    `suppressed()` guard keeps out of here.
    /// 2. **The model must still hold the row's PREVIOUS chord** when the
    ///    caller decides. `probe_plan`'s "Unchanged - this row already uses
    ///    it" compares the typed chord against the row's own, so a probe
    ///    asked after `on_edit_combo` has written it would find every chord
    ///    unchanged and never ask the OS anything.
    ///
    /// Nothing is sent while a key is not selected, exactly as
    /// `on_edit_combo` is not -- see `shortcut_shown`.
    pub on_probe_shortcut: Box<dyn FnMut(String)>,
    pub on_edit_app: Box<dyn FnMut(String)>,
    /// The filter box's text changed. Indices in `on_select` / `on_mark` are
    /// model rows either way -- the window maps them.
    pub on_filter: Box<dyn FnMut(String)>,
    pub on_add: Box<dyn FnMut()>,
    pub on_remove: Box<dyn FnMut()>,
    pub on_apply: Box<dyn FnMut()>,
    pub on_caps: Box<dyn FnMut(bool)>,
    pub on_caps_tap: Box<dyn FnMut(CapsTap)>,
    /// What holding Caps stands for. The window sends all three chips
    /// together because they are one value.
    pub on_caps_hold: Box<dyn FnMut(Chord)>,
    pub on_open_file: Box<dyn FnMut()>,
    /// The installed-app catalog finished scanning.
    pub on_catalog: Box<dyn FnMut(Vec<String>)>,
    /// Reload the model from disk, discarding in-memory edits.
    pub on_reload_from_disk: Box<dyn FnMut()>,
    /// Keep the in-memory edits and dismiss the external-change banner.
    pub on_keep_mine: Box<dyn FnMut()>,
    /// `true` if the window may close. The caller shows any save prompt.
    pub on_close_request: Box<dyn FnMut() -> bool>,
}

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
    /// `layout`'s output depends on FIVE things -- the client rect, the DPI,
    /// whether the banner is showing, whether the list has any rows in it,
    /// and the list's own client width, which shrinks by `SM_CXVSCROLL` the
    /// moment the item count crosses the page size and comctl32 grows a
    /// vertical scroll bar. The first two arrive as `WM_SIZE` /
    /// `WM_DPICHANGED`, which call `layout` directly and still do. The next
    /// two can change on a data push, so a push watches both: this field and
    /// `shown_empty`.
    ///
    /// **The fifth is deliberately NOT guarded**, and the reason it is safe
    /// to leave unguarded is written out at its own site -- see the column
    /// sizing in `layout`. In one sentence: the error it produces is always a
    /// gutter and never a clipped column, and buying it back would mean
    /// running `layout`, and therefore `SetWindowPos` on the populated App
    /// combo, on more data pushes than these two fields already allow --
    /// trading a cosmetic stale margin for a re-entry into the measured
    /// data-loss path above.
    shown_external: Option<bool>,
    /// Whether the list was EMPTY when the current layout was computed, for
    /// the same reason `shown_external` exists: it is the fourth of `layout`'s
    /// five inputs, and skipping a layout that one of them has invalidated
    /// leaves stale geometry on screen. (The fifth, the list's own client
    /// width, is tolerated rather than guarded -- see `shown_external`.)
    ///
    /// The path runs through `list_row_height`, which cannot measure a row
    /// that is not there and returns `scale(20, dpi)` when the list is empty
    /// -- 30 px at a14's 144 DPI, against 29 measured. So a window opened on a
    /// config with no shortcuts lays out with the fallback, and without this
    /// field the first Add would keep it: `external_change` does not move, the
    /// layout is skipped, and the list stays ~8 px taller than the eight rows
    /// it is sized for.
    ///
    /// **Where those 8 px go changed with Task 9, and the guard is what stops
    /// mattering more, not less.** They used to be absorbed by the notes
    /// strip, which flexed into whatever the bands above left; the notes are
    /// a fixed line inside the editor group now (`notes_height`), so nothing
    /// absorbs anything. The extra 8 px push `y`, therefore `grp_y`,
    /// therefore the whole editor group down by 8 -- eating slack above the
    /// keyboard group, and near `MIN_HEIGHT` running into `y.min(kb_y)`. The
    /// other reason it is guarded rather than tolerated is unchanged:
    /// `list_row_height`'s own comment used to justify the fallback by saying
    /// `apply_state` re-lays-out the instant a row appears, which
    /// `shown_external` made false.
    ///
    /// Empty-vs-not is the whole condition: every non-empty list measures the
    /// same row, so no other transition changes the answer.
    shown_empty: Option<bool>,
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

/// Everything `layout` needs out of `Ui`, copied in ONE borrow that is
/// dropped before a single `SendMessageW` or `SetWindowPos` runs.
///
/// This is not tidiness. A second `RefCell` borrow taken across an
/// `extern "system"` boundary — and every one of those calls can re-enter
/// this window's wndproc — ABORTS the process rather than unwinding, so it
/// shows up as neither a panic nor a test failure nor anything a
/// cross-compile can catch. Copying the handles out first makes it
/// unrepresentable.
#[derive(Clone, Copy)]
struct LayoutHandles {
    list: HWND,
    combo: HWND,
    app: HWND,
    notes: HWND,
    filter: HWND,
    banner: HWND,
    reload: HWND,
    keep: HWND,
    fonts: Fonts,
    external_change: bool,
}

impl LayoutHandles {
    fn of(ui: &Ui) -> Self {
        Self {
            list: ui.list,
            combo: ui.combo,
            app: ui.app,
            notes: ui.notes,
            filter: ui.filter,
            banner: ui.banner,
            reload: ui.reload,
            keep: ui.keep,
            fonts: ui.fonts,
            external_change: ui.external_change,
        }
    }
}

thread_local! {
    static UI: RefCell<Option<Ui>> = const { RefCell::new(None) };
    static CB: RefCell<Option<Callbacks>> = const { RefCell::new(None) };
    /// The config path, handed over by `open` and consumed by
    /// `build_children` inside `WM_CREATE`. Same shape as `CB` for the same
    /// reason: `CreateWindowExW` calls the wndproc before it returns, so
    /// there is no window handle to hang an argument on yet.
    ///
    /// **Constant for the window's lifetime**, which is why it lives here
    /// and not in `ControlState`: `serve` opens the window against
    /// `ServeState::config` and nothing can repoint that while it is open,
    /// so making it ride on every keystroke's push would be paying per
    /// keystroke for a fact that is fixed at creation.
    static CFG: RefCell<Option<String>> = const { RefCell::new(None) };
}

/// The window's handle, or `None` when it is closed.
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
/// **`apply_state` is the authoritative moment, and it is the only one.**
/// The window's normal migration is focus-driven (`BN_SETFOCUS` /
/// `BN_KILLFOCUS` in `handle_command`), and that covers every way a user can
/// move the ring by hand. What it cannot cover is a control going away
/// underneath it: hiding a window raises no focus notification at all
/// (measured on a14 2026-08-11 -- `DM_GETDEFID` still answered `IDC_RELOAD`
/// after the banner was dismissed, and Enter pressed a button that was not on
/// screen). Every `show` and every `enable` in this window happens in
/// `apply_state`, so running this after the last of them closes the gap by
/// construction rather than by listing the cases.
///
/// Two repairs, in this order, because the first can make the second
/// unnecessary:
///
/// 1. **Focus.** Measured on Windows ARM64: by the time this function runs,
///    focus is usually already off the vanished button. `show(reload,
///    external_change)` / `show(keep, external_change)` above -- called
///    earlier in the same push, before this function -- hide whichever of
///    Reload/KeepMine just lost `visible()`, and hiding a control that
///    currently holds focus is enough for user32 to hand focus to the
///    PARENT on its own, as part of that same `ShowWindow(SW_HIDE)` call.
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
    let focus = GetFocus();
    if !focus.is_invalid() {
        let fid = GetDlgCtrlID(focus);
        if is_push_button(fid) && !default_button_of(fid).visible(external_change) {
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
    let want = default_button(default_button_of(cur), st, external_change);
    // `set_default_id` no-ops when the id it is handed is already the
    // default, so the overwhelmingly common push repaints nothing.
    set_default_id(hwnd, id_of_default_button(want));
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

/// The severity prefix on a line of the notes STATIC. Not the list: rows
/// carry `ListItem::flag` beside the app name now (see `app_cell`), and a
/// healthy row says nothing at all rather than `OK`.
fn mark_glyph(m: Mark) -> &'static str {
    // ASCII on purpose: the notes carry a Segoe UI Variable text face, or
    // the shell's own on the fallback path, and neither is a symbol font --
    // a missing glyph shows as a box that reads like a rendering bug rather
    // than a status. (Segoe Fluent Icons IS installed, measured on a14, but
    // spec B.5 defers those glyphs to the NM_CUSTOMDRAW pass that can give
    // them their own font.)
    //
    // **`Ok` is blank, and this comment used to explain why it was not.** It
    // said "All four are two columns wide so the note lines line up", which
    // is a MONOSPACE property asserted about a proportional face -- measured
    // on a14 and false. Each note is laid out as `glyph + "  " + text`, so
    // the glyph's advance IS the x where the text starts:
    //
    // |            | 144 DPI | 96 DPI |
    // |------------|---------|--------|
    // | `OK` + 2sp | 35 px   | 22 px  |
    // | `!!` + 2sp | 20 px   | 14 px  |
    // | `! ` + 2sp | 20 px   | 13 px  |
    // | `..` + 2sp | 18 px   | 12 px  |
    // | 4 spaces   | 20 px   | 12 px  |
    //
    // So `OK` stood 15 px proud of `!!`, and the other three were never
    // equal either -- just close enough not to be noticed. Two spaces put
    // `Ok` inside the 2 px spread the shipped marks already had, and say
    // the same thing spec B.5 says with the list flag: a healthy row is
    // silent. Exact alignment needs a glyph column drawn at a fixed x,
    // which is the NM_CUSTOMDRAW work B.5 defers.
    //
    // The trailing space on `Warn` is still load-bearing, not a typo.
    match m {
        Mark::Ok => "  ",
        Mark::Warn => "! ",
        Mark::Bad => "!!",
        Mark::Unknown => "..",
    }
}

// ---------------------------------------------------------------------------
// Creation
// ---------------------------------------------------------------------------

/// Open the window, or raise it if it is already open.
///
/// `config_path` is the file the caller's callbacks read and write. It names
/// the window (`beckon - <file name>`) and fills the `Open config file`
/// tooltip; it is taken ONCE, here, because it cannot change while the
/// window is open.
pub fn open(cb: Callbacks, config_path: &str) -> Result<(), String> {
    if let Some(h) = hwnd() {
        unsafe {
            let _ = SetForegroundWindow(h);
        }
        // Keep the existing callbacks: they close over the caller's live
        // state, and the second set would be a duplicate of the first.
        return Ok(());
    }
    CB.with(|c| *c.borrow_mut() = Some(cb));
    CFG.with(|c| *c.borrow_mut() = Some(config_path.to_string()));
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
        lpfnWndProc: Some(wndproc),
        hInstance: hinst.into(),
        lpszClassName: class,
        hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
        // WNDCLASS takes a system colour index PLUS ONE here, not a brush
        // and not the raw index -- 0 means "no background", so passing
        // COLOR_BTNFACE unshifted paints the window with COLOR_BTNSHADOW.
        hbrBackground: HBRUSH((COLOR_BTNFACE.0 + 1) as isize as *mut _),
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
    // -- and therefore `build_children`, which takes `CFG` -- before it
    // returns.
    let title = wide(&title_base(
        &CFG.with(|c| c.borrow().clone()).unwrap_or_default(),
    ));

    let hwnd = CreateWindowExW(
        WINDOW_EX_STYLE(0),
        class,
        PCWSTR(title.as_ptr()),
        WS_OVERLAPPEDWINDOW,
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

/// The three type roles of §B.3, built for `dpi`.
///
/// | Role | Size | Weight | Used for |
/// |---|---|---|---|
/// | Subtitle | 20 px | semibold | band headings |
/// | Body | 14 px | regular | list, fields, buttons |
/// | Caption | 12 px | regular | notes |
///
/// **The face names are spelled in full, from the a14 measurement.**
/// `Segoe UI Variable Text Semibold` is exactly 31 characters and survives
/// `lfFaceName` intact; the Display and Small semibolds do not, which is
/// why the family here is Text rather than whichever optical size a naive
/// truncation happens to leave valid. `Segoe UI Variable Text` / `Small` /
/// `Display` were all confirmed present and exact.
///
/// Optical size is why Body and Caption differ at all: Segoe UI Variable
/// ships Small for caption sizes, Text for body and headings up to ~30 px,
/// Display above that. 20 px is Text territory, not Display's.
unsafe fn build_fonts(hwnd: HWND, dpi: u32) -> Fonts {
    let base = message_logfont(dpi);
    Fonts {
        subtitle: make_font(
            hwnd,
            &base,
            "Segoe UI Variable Text Semibold",
            20,
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
    set_cap_font(fonts.get(Role::Caption));

    // -- Band 1: the external-change banner. Hidden until `apply_state`
    // says the file moved; `layout` gives it no height at all while it is
    // hidden, so the bands below close up rather than leaving a gap.
    let banner = child(
        hwnd,
        w!("STATIC"),
        "This file changed on disk.",
        SS_CENTERIMAGE_STYLE,
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
    show(banner, false);
    show(reload, false);
    show(keep, false);

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
    let filter = child(
        hwnd,
        w!("EDIT"),
        "",
        WINDOW_STYLE(ES_AUTOHSCROLL as u32) | WS_BORDER | WS_TABSTOP,
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
    let list = child(
        hwnd,
        w!("SysListView32"),
        "",
        WINDOW_STYLE(LVS_REPORT | LVS_SINGLESEL | LVS_SHOWSELALWAYS | LVS_NOSORTHEADER)
            | WS_BORDER
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

    // -- Band 4: the editor group. The strip's two lines live inside it and
    // its caption names the row, so seven controls read as one thing (spec
    // A.1).
    //
    // Created BEFORE its children: a group box is a BUTTON that paints a
    // frame, and creation order is z-order, so a group created afterwards
    // paints over the controls it is supposed to surround.
    //
    // Not a tab stop, and deliberately no BS_NOTIFY: it is not operable, so
    // it must not join PUSH_BUTTONS and must never take the default ring.
    child(
        hwnd,
        w!("BUTTON"),
        cap::EDITOR_NONE,
        WINDOW_STYLE(BS_GROUPBOX as u32),
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
    let combo = child(
        hwnd,
        w!("COMBOBOX"),
        "",
        WINDOW_STYLE(CBS_DROPDOWNLIST as u32) | WS_VSCROLL | WS_TABSTOP,
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
    // mock-up draws it. Several lines tall, so no SS_CENTERIMAGE -- and, for
    // the same reason, no SS_ENDELLIPSIS either; see SS_NOPREFIX_STYLE.
    let notes = child(
        hwnd,
        w!("STATIC"),
        "",
        SS_LEFT_STYLE | SS_NOPREFIX_STYLE,
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
    child(
        hwnd,
        w!("BUTTON"),
        "Keyboard",
        WINDOW_STYLE(BS_GROUPBOX as u32),
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
    let tap = child(
        hwnd,
        w!("COMBOBOX"),
        "",
        WINDOW_STYLE(CBS_DROPDOWNLIST as u32) | WS_VSCROLL | WS_TABSTOP,
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
    let path = CFG.with(|c| c.borrow_mut().take()).unwrap_or_default();
    let mut tip_text = wide(&path);
    add_tooltip(hwnd, openfile, &mut tip_text);

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
            suppress: false,
            external_change: false,
            items: Vec::new(),
            app_epoch: 0,
            shown_combo: None,
            capture: None,
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

/// Lay a run of keycaps out inside `cell`, or report that it does not fit.
///
/// Two callers, and `style` says which: the **Shortcut column**, where this
/// is why the config's own spelling never reaches the screen -- the user
/// pressed three physical keys, so the column draws three keys, and `super`
/// is a valid TOML token and a word on no keyboard -- and the **seven toggle
/// chips**, which pass one cap each.
///
/// **Returns `false` when the caps do not fit, and the caller falls back to
/// the display string with an ellipsis.** That fallback is structural rather
/// than defensive: `tok::SHORTCUT_COL` is 200 px but `layout` caps the column
/// at `inner / 2`, and a five-modifier chord, a narrow window and a high DPI
/// each reach the limit on their own. A clipped keycap reads as a rendering
/// fault; an ellipsis reads as a narrow column.
///
/// **Every colour is `GetSysColor`.** An accent literal would fight the row
/// highlight four pixels away and would be the first crack in this window's
/// light-only rule; `COLOR_HIGHLIGHT` is the user's own accent and is already
/// right in high contrast. `hc` changes the *shape* only -- see
/// `high_contrast`.
unsafe fn draw_keycaps(
    hdc: HDC,
    cell: RECT,
    caps: &[String],
    font: HFONT,
    dpi: u32,
    hc: bool,
    style: CapStyle,
) -> bool {
    if caps.is_empty() {
        return false;
    }
    let toggle = matches!(style, CapStyle::Toggle { .. });
    // **Two sets of metrics, from direction B's own two rules.** The board
    // gives the column cap (`.wcap`) `height:19px; padding:0 5px` and the
    // toggle chip (`.wtog`) `height:28px; padding:0 10px; min-width:46px` --
    // a chip is a key you press, a column cap is a key you read, and sizing
    // both from the column's numbers is what made the shipped chips look
    // like small grey buttons. A chip therefore takes its control's whole
    // height rather than the column's 19 px ceiling.
    let pad = scale(if toggle { 10 } else { 5 }, dpi);
    let gap = scale(3, dpi);
    let inset = scale(if toggle { 2 } else { 4 }, dpi);
    let row_h = cell.bottom - cell.top;
    let cap_h = if toggle {
        (row_h - inset * 2).max(scale(16, dpi))
    } else {
        (row_h - scale(6, dpi))
            .min(scale(19, dpi))
            .max(scale(12, dpi))
    };
    // The bottom edge, and the whole reason a box reads as a key. `.wcap` and
    // `.wtog` both carry `border-bottom:2px` against a 1 px everywhere else.
    let edge_h = scale(2, dpi).max(1);

    let prev_font = SelectObject(hdc, HGDIOBJ(font.0));

    // Measure the whole set before drawing any of it: the fallback is a
    // decision about the set, not something to discover halfway along it with
    // two caps already on screen.
    let mut widths = Vec::with_capacity(caps.len());
    let mut total = gap * (caps.len() as i32 - 1);
    for c in caps {
        // **Measured through `shown` for a chip and verbatim for a cell**,
        // which is the same split `layout`'s `tw` already makes. A chip's
        // caption carries a mnemonic marker -- `C&trl` -- and the `&` is not
        // drawn, so a cap sized for it is a cap one character too wide. A
        // cell's text is data and has no mnemonic to strip.
        let m = if toggle { shown(c) } else { c.clone() };
        let t = wide(&m);
        let mut sz = SIZE::default();
        // `wide` appends a NUL and this API takes a length, so the NUL would
        // be measured as a character -- same rule as `text_size`.
        let w = if GetTextExtentPoint32W(hdc, &t[..t.len() - 1], &mut sz).as_bool() {
            sz.cx + pad * 2
        } else {
            scale(8, dpi) * m.chars().count() as i32 + pad * 2
        };
        total += w;
        widths.push(w);
    }
    let room = cell.right - cell.left - inset * 2;
    if total > room {
        if !prev_font.is_invalid() {
            SelectObject(hdc, prev_font);
        }
        return false;
    }
    // **A chip is one cap and it owns its control, so it takes the whole
    // width** instead of shrinking to its caption. `min-width:46px` on
    // `.wtog` says the same thing in CSS: a row of keys whose sizes follow
    // the length of their letters does not read as a keyboard. `layout`
    // already floors each chip's control at `tok::CHIP_MIN`, so this is
    // where that floor becomes visible.
    //
    // Sized independently of the text, which is also what keeps a chip from
    // resizing when it is toggled -- the measurement above is now only the
    // fit test.
    if toggle {
        total = room;
        if let Some(w) = widths.first_mut() {
            *w = room;
        }
    }

    // **A pressed key goes DOWN**: one pixel, and no bottom edge. That is the
    // whole effect, and it is the ONLY click feedback these chips have --
    // Windows draws none of its own for an owner-draw button, so without it a
    // chip held under the mouse looks identical to one that is not.
    let press = match style {
        CapStyle::Toggle { pressed: true, .. } => scale(1, dpi),
        _ => 0,
    };
    let top = cell.top + (row_h - cap_h) / 2 + press;
    // Where the run starts. A chip owns its whole control rect and centres in
    // it; a cell is one column of many rows, and those line up down the
    // column, so a chord starts at a fixed inset instead.
    let mut x = match style {
        CapStyle::Chord => cell.left + inset,
        CapStyle::Toggle { .. } => cell.left + ((cell.right - cell.left) - total) / 2,
    };
    // **Every colour is `GetSysColor`, or derived from one.** An armed chip's
    // face is `COLOR_HIGHLIGHT` -- the user's own accent, matching the row
    // highlight four pixels away and already correct in high contrast -- and
    // its edge is that same colour through `shade`. Direction B's `#2563eb` /
    // `#1d4fc4` pair is what the ratio was read off, not a colour to hard-code.
    let armed_face = COLORREF(GetSysColor(COLOR_HIGHLIGHT));
    let (edge_col, border_col) = match style {
        _ if hc => {
            let c = COLORREF(GetSysColor(COLOR_WINDOWTEXT));
            (c, c)
        }
        CapStyle::Toggle { armed: true, .. } => {
            let e = shade(armed_face, 4, 5);
            (e, e)
        }
        // A disabled chip keeps its shape and its depth and loses only its
        // ink -- see the face table below for why it does not also keep the
        // light face.
        CapStyle::Toggle { disabled: true, .. } => {
            let c = COLORREF(GetSysColor(COLOR_BTNSHADOW));
            (c, c)
        }
        _ => {
            let c = COLORREF(GetSysColor(COLOR_BTNSHADOW));
            (c, c)
        }
    };
    let pen = CreatePen(PS_SOLID, 1, border_col);
    let prev_pen = SelectObject(hdc, HGDIOBJ(pen.0));
    SetBkMode(hdc, TRANSPARENT);
    // **A chip's resting face is `COLOR_WINDOW`, not `COLOR_BTNFACE`**, and
    // that one substitution is most of why the shipped chips disappeared:
    // `COLOR_BTNFACE` IS the window's own background, so an unarmed chip was
    // a grey box on a grey surface with only a hairline to prove it existed.
    // Direction B puts `--w-cap:#fafafa` on a `--w-bg:#f3f3f3` window -- the
    // key is LIGHTER than what it sits on, which is how a physical keycap
    // catches light.
    //
    // **Greyed outranks armed, and a disabled chip keeps `COLOR_BTNFACE`.**
    // The light face is what makes an OPERABLE key stand off the surface, so
    // giving it to a disabled one inverts the whole point -- measured on a14:
    // with `keyboard.caps` off, three white `Hold` keys read as the most
    // prominent thing in the band. `.wtog.dis` puts `#f7f7f7` on a `#f3f3f3`
    // window, i.e. it deliberately sinks BACK into the surface. Only the ink
    // and the face change; the box and its edge stay, so the shape survives.
    //
    // What a disabled chip stops saying is which way it is set. That is a
    // real loss on the three `Hold` chips, which are greyed whenever Caps is
    // off while still describing what Caps would do. No accent-on-grey
    // pairing exists in the system palette to settle it; it wants eyes
    // rather than another argument here.
    let (face, text_colour) = match style {
        CapStyle::Chord => (None, COLOR_BTNTEXT),
        CapStyle::Toggle { disabled: true, .. } => (Some(COLOR_BTNFACE), COLOR_GRAYTEXT),
        CapStyle::Toggle { armed: true, .. } => (Some(COLOR_HIGHLIGHT), COLOR_HIGHLIGHTTEXT),
        CapStyle::Toggle { .. } => (Some(COLOR_WINDOW), COLOR_BTNTEXT),
    };
    SetTextColor(hdc, COLORREF(GetSysColor(text_colour)));
    // A cell's text is data and its `&` is a character; a chip's caption
    // carries a mnemonic, and whether the underline SHOWS is the window's UI
    // state to say, not this function's -- see `draw_chip`.
    let text_flags = match style {
        CapStyle::Chord => DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX,
        CapStyle::Toggle { hide_accel, .. } => {
            let base = DT_CENTER | DT_VCENTER | DT_SINGLELINE;
            if hide_accel {
                base | DT_HIDEPREFIX
            } else {
                base
            }
        }
    };

    for (i, c) in caps.iter().enumerate() {
        let w = widths[i];
        // For a chord: the main key is last, and it is the one actually
        // pressed, so it takes the window fill and reads brighter than the
        // modifiers holding it down. Every row in this column shares the same
        // three-modifier prefix, so the key is the only part worth finding at
        // a glance. A chip is one cap and has no such last, which is why the
        // armed fill is decided above and simply wins here.
        let fill = match face {
            Some(f) => f,
            None if i + 1 == caps.len() => COLOR_WINDOW,
            None => COLOR_BTNFACE,
        };
        if hc {
            // Flat, hard and no depth: a high-contrast theme is built on
            // solid fills and hard borders, and a soft edge under one reads
            // as a rendering artefact rather than as a key.
            let brush = CreateSolidBrush(COLORREF(GetSysColor(fill)));
            let prev_brush = SelectObject(hdc, HGDIOBJ(brush.0));
            let _ = Rectangle(hdc, x, top, x + w, top + cap_h);
            if !prev_brush.is_invalid() {
                SelectObject(hdc, prev_brush);
            }
            let _ = DeleteObject(HGDIOBJ(brush.0));
        } else {
            // **Two rounded rects, not a rect plus a line.** The edge is a
            // 2 px BORDER in CSS, so it follows the corner radius; the old
            // inset hairline sat inside the box and read as an underline
            // rather than as the side of a key. Painting the taller shape in
            // the edge colour first and the face over it, `edge_h` shorter,
            // leaves exactly that border showing along the bottom.
            //
            // A pressed key skips it and drops a pixel: at the bottom of its
            // travel there is no side left to see.
            let r = scale(5, dpi) * 2;
            if press == 0 {
                let eb = CreateSolidBrush(edge_col);
                let pb = SelectObject(hdc, HGDIOBJ(eb.0));
                let _ = RoundRect(hdc, x, top, x + w, top + cap_h, r, r);
                if !pb.is_invalid() {
                    SelectObject(hdc, pb);
                }
                let _ = DeleteObject(HGDIOBJ(eb.0));
            }
            let brush = CreateSolidBrush(COLORREF(GetSysColor(fill)));
            let prev_brush = SelectObject(hdc, HGDIOBJ(brush.0));
            let _ = RoundRect(hdc, x, top, x + w, top + cap_h - edge_h, r, r);
            if !prev_brush.is_invalid() {
                SelectObject(hdc, prev_brush);
            }
            let _ = DeleteObject(HGDIOBJ(brush.0));
        }

        // Centred in the FACE, not in the whole cap: the bottom edge is the
        // side of the key, and text centred over it sits low.
        let mut tr = RECT {
            left: x,
            top,
            right: x + w,
            bottom: top + cap_h - if hc || press > 0 { 0 } else { edge_h },
        };
        // The RAW caption, `&` intact: `text_flags` decides whether it marks
        // a mnemonic or is drawn. Only the MEASUREMENT above strips it.
        let mut t = wide(c);
        let n = t.len() - 1;
        DrawTextW(hdc, &mut t[..n], &mut tr, text_flags);
        x += w + gap;
    }

    if !prev_pen.is_invalid() {
        SelectObject(hdc, prev_pen);
    }
    let _ = DeleteObject(HGDIOBJ(pen.0));
    if !prev_font.is_invalid() {
        SelectObject(hdc, prev_font);
    }
    true
}

/// One ListView row, in physical pixels at the live DPI.
///
/// **Queried, never scaled from a token.** 29 px measured on a14 at 144 DPI
/// is 19.33 at 96, and a non-integer is the tell that comctl32 derives the
/// row height from the font rather than from a design constant — a 96-DPI
/// token pushed through `scale` would be wrong at every non-integer scale
/// and would go wrong again the moment B.3 changes the font.
///
/// `LVM_GETITEMRECT` needs a row to measure. When the list is empty there is
/// none, so this falls back to a scaled token -- 30 px at 144 DPI, against the
/// 29 a real row measures.
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
    scale(20, dpi)
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
/// **It is an input to `MIN_HEIGHT`.** The floor is derived from `grp_h`,
/// and `grp_h` is derived from this -- change what a notes line costs and
/// the floor moves with it. 16 px (96 DPI, derived) / 24 px (144 DPI,
/// measured): the 144 figure IS a fresh a14 reading -- item 10 of the
/// 2026-08-11 a14 pass sized the read-only notes STATIC against "5 lines x
/// 24" at 144 DPI, the same Caption face this line measures. The 96 DPI
/// figure comes from applying the same internal-leading ratio the Body font
/// showed at that pass (`text_h` 28 against a requested 21, i.e. 4/3) to
/// Caption's 12 px request -- and that same ratio, applied to Caption's
/// 144-DPI request of 18, reproduces the hardware 24 exactly, which is why
/// it is trusted for the DPI nobody has measured. If a real 96-DPI reading
/// disagrees, `MIN_HEIGHT` must be re-derived from it, not nudged -- though
/// the disagreement is bounded, not open-ended: the derived window height
/// is `546 + 2(L - 16)` for a real Caption line height `L`, so the shipped
/// 550 absorbs any `L` up to 18 px with the four-row banner-up guarantee
/// intact. `L = 19` costs one row and nothing else -- `editor_min = grp_h`
/// in `layout` (see its own comment there) is computed from the RUNTIME
/// value, not this estimate, so a wrong `L` can only shrink the list at the
/// absolute floor; it cannot produce an overlap at any `L`. That is the
/// safe direction.
unsafe fn notes_height(hwnd: HWND, ui: &LayoutHandles, dpi: u32) -> i32 {
    let line = text_size(hwnd, ui.fonts.get(Role::Caption), dpi, "Ag").1;
    line * 2 + scale(4, dpi)
}

/// Seven horizontal bands, top to bottom: the external-change banner (no
/// height when hidden), the section head, the list, the editor group, the
/// suggestion row (no control, no height, in this landing), the keyboard
/// group and the command bar.
///
/// Everything is placed from the client rect at the current DPI, so a
/// 150 % display is not an afterthought — `GetDpiForWindow` scales the
/// tokens rather than the tokens assuming 96.
///
/// **Vertical shape.** The command bar is anchored to the bottom and the
/// keyboard group sits directly above it; the top bands stack downward.
///
/// **The LIST is the one thing that flexes.** It wants `header + 8 rows` and
/// gives that up rather than let anything overlap when the window is short —
/// a shrunk list scrolls, an overlapped control is unreachable. Everything
/// below it is fixed: band 4 is a group box of a computed height (`grp_h`),
/// which band 3 reserves through `editor_min` before choosing its own.
///
/// The notes STATIC used to be the flexing band instead, which is what made
/// it a 1220x177 control holding one 258 px line at the default size. It is
/// now a fixed line inside the editor group — see `notes_height` — so a
/// vertical resize lands on the list, and once the list is at its full 8 rows
/// the surplus is simply slack above the keyboard group.
unsafe fn layout(hwnd: HWND) {
    let mut rc = RECT::default();
    if GetClientRect(hwnd, &mut rc).is_err() {
        return;
    }
    let dpi = GetDpiForWindow(hwnd).max(96);
    let s = |v: i32| v * dpi as i32 / 96;
    // Independent of WM_GETMINMAXINFO: the floor is about the frame, and a
    // clamp is about the arithmetic. Either alone leaves a negative cy
    // reachable -- SetWindowPos with one produces a control the user can
    // never see or focus again. Widths need it as much as heights: WM_SIZE
    // fires with a 0x0 client rect on minimize (ptMinTrackSize only
    // constrains dragging, not that), so `w` is 0 here on every minimize,
    // on every machine, and every subtraction below goes negative without
    // it.
    //
    // **Widths take `clamp`; a POSITION computed leftward from a right edge
    // takes `.max(cx)` instead.** They are not interchangeable: clamping a
    // width to 0 hides the control, which is recoverable, while clamping a
    // position to 0 puts it outside the surface padding -- flush against the
    // window edge, overlapping whatever is to its left -- which is not.
    //
    // No band needs `.max(cx)` today, and that is structural rather than
    // lucky: every rightward position in this function is spelled
    // `origin + clamp(...)`, which cannot fall left of its origin. The rule
    // is written down here, beside the tool it is about, because the band
    // that reintroduces the hazard will be a NEW one subtracting from a right
    // edge, and it will have no local precedent to copy.
    let clamp = |v: i32| v.max(0);

    // ONE borrow of UI, taken here and dropped on this line. Nothing below
    // may hold one: every SetWindowPos and SendMessageW that follows can
    // re-enter this window's wndproc, and a second borrow across an
    // `extern "system"` boundary aborts the process instead of unwinding.
    let Some(ui) = UI.with(|u| u.borrow().as_ref().map(LayoutHandles::of)) else {
        return;
    };

    let pad = s(tok::PAD);
    let band = s(tok::BAND);
    let gap = s(tok::GAP);
    let lblgap = s(tok::LABEL);
    let ctl = s(tok::CTL);

    let w = rc.right - rc.left;
    let h = rc.bottom - rc.top;
    let cx = pad;
    let cw = clamp(w - pad * 2);

    // Body, and only Body: every string measured in this function labels or
    // captions a Body control -- the three command-bar buttons, Add /
    // Remove / Reload / Keep mine, the two field labels, the whole keyboard
    // row, and the "Ag" that sizes the EDIT. The `Shortcuts` heading is the one
    // Subtitle in the window and its width is never measured; it takes
    // whatever Add and Remove leave it.
    //
    // Measured through `shown`, so a caption's `&` -- a mnemonic marker,
    // which is not drawn -- does not buy the control a character of width it
    // will never use.
    let tw = |t: &str| text_size(hwnd, ui.fonts.get(Role::Body), dpi, &shown(t)).0;
    let btn = |t: &str| s(tok::BTN).max(tw(t) + s(24));

    let place = |id: i32, x: i32, y: i32, cxx: i32, cy: i32| {
        if let Ok(c) = GetDlgItem(Some(hwnd), id) {
            let _ = SetWindowPos(c, None, x, y, cxx, cy, SWP_NOZORDER | SWP_NOACTIVATE);
        }
    };
    let place_h = |h_: HWND, x: i32, y: i32, cxx: i32, cy: i32| {
        let _ = SetWindowPos(h_, None, x, y, cxx, cy, SWP_NOZORDER | SWP_NOACTIVATE);
    };

    // The two bottom bands are anchored, not stacked, so the window's
    // bottom edge is where they stay however tall the content above is.
    let bar_y = clamp(h - pad - ctl);
    // Caption inset, ONE control line, then a bottom inset the same size as
    // the gap -- the shape band 4's `grp_h` follows too, so the two group
    // boxes in this window are one rule. It was two lines while the group
    // held a check box over three radios; the Caps row is one line, so the
    // group is one `ctl + gap` shorter. Those pixels used to go to the
    // flexing notes band; now they raise `kb_y`, which is what band 3 sizes
    // the list against.
    let kb_h = s(24) + ctl + gap;
    let kb_y = clamp(bar_y - band - kb_h);

    let mut y = pad;

    // Field geometry, computed before band 2 because the filter box needs it
    // there and the editor strip needs it in band 4. `combo_h` is therefore
    // read BEFORE the combo is placed this pass, i.e. it is the height the
    // combo had on the PREVIOUS pass. That is sound: the value is the theme's
    // choice for a font and a DPI, so it moves only on WM_DPICHANGED or a
    // font change, both of which run `layout` again immediately. The one pass
    // that can read a not-yet-snapped height is the first, and the floor
    // below falls back to the font-derived height there.
    let text_h = text_size(hwnd, ui.fonts.get(Role::Body), dpi, "Ag").1;
    let field_h = (text_h + s(10)).min(ctl);
    let mut arc = RECT::default();
    let combo_h = if GetWindowRect(ui.app, &mut arc).is_ok() {
        let ah = arc.bottom - arc.top;
        if ah > 0 && ah < ctl && ah >= text_h + s(2) {
            Some(ah)
        } else {
            None
        }
    } else {
        None
    };
    // Both EDITs take the combo's height, so the three fields in this window
    // are one box repeated. A single-line EDIT top-aligns its text -- Win32
    // gives it no vertical centring at all -- so it is centred in its band
    // line rather than stretched to it.
    let (edit_h, edit_dy) = match combo_h {
        Some(ah) => (ah, clamp(ctl - ah) / 2),
        None => (field_h, clamp(ctl - field_h) / 2),
    };
    // What a caption costs beyond its own width. Two readings, one number,
    // which is why it is computed here rather than in a band: `IDC_CAPS` is
    // a real `BS_AUTOCHECKBOX` and this is its square plus the gap before
    // its text, while for the seven keycap chips it is the padding around
    // the letters -- `.wtog { padding:0 10px }` plus a hair, since
    // `draw_keycaps` fills whatever width the chip control is given.
    let glyph = s(24);
    // One chip's width: its caption plus that slack, never below
    // `tok::CHIP_MIN`. Both chip rows go through this, so the four modifier
    // chips and the three `Hold` chips cannot drift apart -- and `draw_chip`
    // fills whatever width it is given, so this closure alone decides how
    // big a key is.
    let chip = |c: &str| (tw(c) + glyph).max(s(tok::CHIP_MIN));

    // -- Band 1: the banner. Contributes NO height when hidden.
    if ui.external_change {
        let bw_reload = btn(cap::RELOAD);
        let bw_keep = btn(cap::KEEP_MINE);
        let buttons = bw_reload + gap + bw_keep;
        place_h(ui.banner, cx, y, clamp(cw - buttons - gap), ctl);
        place_h(ui.reload, cx + clamp(cw - buttons), y, bw_reload, ctl);
        place_h(ui.keep, cx + clamp(cw - bw_keep), y, bw_keep, ctl);
        y += ctl + band;
    }

    // -- Band 2: the section head. `Shortcuts` leading, then the filter,
    // then Remove and Add right-aligned.
    let bw_add = btn(cap::ADD);
    let bw_remove = btn(cap::REMOVE);
    // Capped at a third of the width, the same ceiling band 4 puts on the
    // key list, so the boxes narrow together. The HEADING
    // takes what is left, which makes it -- not the filter -- the first
    // thing to run out. At 96 DPI, with `Add`/`Remove` both pinned to
    // `tok::BTN` (88 px -- neither caption needs more), `heading_w` reduces
    // to `clamp(cw - 200 - filter_w)`. Below `cw = 600` (where `filter_w` is
    // itself `floor(cw / 3)`, not the 200 px cap) that clamps to zero at
    // `cw = 300` -- the raw client width `w` this comes from, before the
    // PAD margins, is 332. `MIN_WIDTH` is a WINDOW floor, not a `cw` one:
    // `WM_GETMINMAXINFO` sets `ptMinTrackSize.x`, which bounds the whole
    // window including the OS's own frame, so its 720 does not translate
    // into an exact `cw` this file computes -- only into "hundreds of
    // pixels clear of the 332 px zero point under any frame the OS adds,"
    // which is the actual reason a drag can never reach it. Every
    // subtraction is clamped, so the intermediate rects `WM_DPICHANGED` can
    // suggest below that floor produce a hidden heading rather than a
    // negative width.
    let filter_w = s(tok::SHORTCUT_COL).min(clamp(cw / 3));
    let filter_x = cx + clamp(cw - bw_add - gap - bw_remove - gap - filter_w);
    place(IDC_ADD, cx + clamp(cw - bw_add), y, bw_add, ctl);
    place(
        IDC_REMOVE,
        cx + clamp(cw - bw_add - gap - bw_remove),
        y,
        bw_remove,
        ctl,
    );
    place_h(ui.filter, filter_x, y + edit_dy, filter_w, edit_h);
    // **The heading is measured now**, where it never used to be: it shares
    // its line with the count, so it has to end somewhere definite rather
    // than taking everything up to the filter. Measured in SUBTITLE -- the
    // only string in `layout` that is not Body, and `tw` would under-measure
    // it by a third and put the count on top of it.
    //
    // The count keeps the leftover, so the heading is still the last thing to
    // run out and a narrow window clips the count first -- which is the right
    // order: `Shortcuts` names the band, `· 18 bindings` decorates it.
    let head_w = text_size(hwnd, ui.fonts.get(Role::Subtitle), dpi, "Shortcuts").0 + s(4);
    let head_w = head_w.min(clamp(filter_x - gap - cx));
    place(IDC_LBL_SECTION, cx, y, head_w, ctl);
    place(
        IDC_LBL_COUNT,
        cx + head_w + lblgap,
        y,
        clamp(filter_x - gap - (cx + head_w + lblgap)),
        ctl,
    );
    // A control gap, not a band gap: the head labels the list directly
    // below it, so the two read as one group.
    y += ctl + gap;

    // Band 4's height, computed HERE because band 3 has to yield to it and
    // the two must not each hold an opinion about how tall the editor is --
    // the same reason `glyph` is computed above band 1 rather than twice.
    //
    // Caption inset, two content lines, the notes, then a bottom inset the
    // size of the gap: the same shape band 6's `kb_h` uses, so the two group
    // boxes in this window are one rule. Fixed, not flexing -- see
    // `notes_height`.
    let notes_h = notes_height(hwnd, &ui, dpi);
    let grp_h = s(24) + ctl + gap + ctl + gap + notes_h + gap;

    // -- Band 3: the list.
    let row_h = list_row_height(ui.list, dpi);
    // `want` is a WINDOW height (it feeds SetWindowPos below), but the list
    // carries WS_BORDER, so its client height -- where header_height + 8
    // rows actually get drawn -- is 2*SM_CYBORDER less than that. Without
    // this the 8th row was clipped by the border and comctl32 drew a sliver
    // of a 9th.
    let border = 2 * GetSystemMetricsForDpi(SM_CYBORDER, dpi);
    let want = list_header_height(ui.list, dpi) + row_h * tok::ROWS + border;
    // The editor below is a fixed-height GROUP now, not a two-line strip, so
    // the figure the list must yield to is the whole of `grp_h` -- caption
    // inset, both lines, the notes and the bottom inset. It used to be
    // `ctl + gap + ctl`, which was right when the notes flexed into whatever
    // was left; against a fixed group that under-reserves by the caption
    // inset plus a line, and the group draws over the keyboard group instead
    // of the list giving up a row. **This is the guard**: `y.min(kb_y)` at
    // the end of band 4 cannot help, because `grp_y` is bound before the
    // group is placed and clamping `y` afterwards moves nothing.
    let editor_min = grp_h;
    let room = clamp(kb_y - band - y);
    let list_h = clamp(want.min(clamp(room - band - editor_min)));
    place_h(ui.list, cx, y, cw, list_h);

    // Columns, sized from the list's OWN client width now that it has one,
    // minus a vertical scroll bar's width whether or not one is showing.
    // That subtraction is what makes overflow structurally impossible: a
    // scroll bar appearing later steals client width the columns have
    // already been told not to use. Measured before this change: 561 px of
    // columns inside a 482 px list, i.e. a horizontal scroll bar shipped.
    //
    // **This `GetClientRect` is `layout`'s fifth input, and the ONE the
    // `apply_state` guard does not track.** When a scroll bar is up the list
    // reports `C - SB`, so the columns get `C - 2*SB`; drop back under the
    // page size and the client returns to `C` while the columns keep the
    // narrower figure until the next resize, DPI change or banner flip --
    // roughly a 34 px gutter at 96 DPI, 52 at 150 %.
    //
    // Tolerated, on purpose. The subtraction only ever errs in the safe
    // direction: too narrow is a margin, never a clipped column or a
    // horizontal scroll bar, which is the failure this line was written to
    // kill. Guarding it would mean recording this width and re-running
    // `layout` whenever it moved -- i.e. `SetWindowPos` on the populated App
    // combo on a data push, the exact call that silently replaced what the
    // user typed with a catalogue entry (see `Ui::shown_external`). A stale
    // margin is not worth reopening that.
    //
    // If it is ever fixed: the cheap route is a `shown_list_w: Option<i32>`
    // alongside the other two guards, NOT a wider `layout`.
    let mut lrc = RECT::default();
    let inner = if GetClientRect(ui.list, &mut lrc).is_ok() {
        clamp(lrc.right - lrc.left - GetSystemMetricsForDpi(SM_CXVSCROLL, dpi))
    } else {
        0
    };
    // `Shortcut` never takes more than half, so `App` -- which leads, and
    // carries the tick and the flag -- can never be squeezed out.
    let col_shortcut = s(tok::SHORTCUT_COL).min(inner / 2);
    let col_app = clamp(inner - col_shortcut);
    set_column_width(ui.list, 0, col_app);
    set_column_width(ui.list, 1, col_shortcut);
    y += list_h + band;
    // `list_h` clamps to 0 when `room` itself clamped negative -- reachable
    // only by an intermediate resize below MIN_HEIGHT that WM_DPICHANGED's
    // suggested rect can hand us without asking WM_GETMINMAXINFO (dragging
    // can't reach it; a 0x0 client rect clamps everything to 0 and is fine).
    // In that state `y` here can still land past `kb_y`, and this line is
    // what stops it: band 4 reads `y` straight into `grp_y`, so this is the
    // editor group box's TOP edge, and band 4's height is the fixed `grp_h`
    // -- not something `clamp` shrinks the way it shrinks `list_h` above.
    //
    // **It bounds the top, and only the top.** What keeps the group's BOTTOM
    // off `kb_y` at and above `MIN_HEIGHT` is `editor_min` above, which
    // reserves the whole of `grp_h` before the list takes any height at all.
    // This line cannot do that job -- it runs before `grp_h` is added, so it
    // can pin `grp_y` to `kb_y` but never pull the group's bottom back up.
    // Drop `editor_min` back to the `ctl + gap + ctl` the one-line strip used
    // and this line will not save you: simulated at `MIN_HEIGHT` itself, the
    // group draws 16 px over the keyboard group with the banner down and
    // 62 px with it up (22 / 91 at 150 %; re-simulated for Task 9's floor of
    // 550 -- `notes_height`'s real body costs 4 px more than the stub these
    // figures were first taken against). That is the ordinary minimum drag
    // size, not a sub-floor `WM_DPICHANGED` edge case.
    y = y.min(kb_y);

    // -- Band 4: the editor group. TWO lines inside a titled BS_GROUPBOX,
    // then the notes on a third line inside the same group.
    //
    //   +- Editing "Windows Terminal" ----------------------------------+
    //   |  App       [ ..................................... v ]        |
    //   |  Shortcut  [ ]Ctrl [ ]Win [ ]Alt [ ]Shift [ key v ]  [R] [R]  |
    //   |  ok  Registered. Press Ctrl + Win + Alt + T to focus it.      |
    //   +---------------------------------------------------------------+
    //
    // **App gets a line of its own, and that is the whole point.** On one
    // line it was the control that absorbed whatever the other six left --
    // about 209 px at 860, and ~59 px at MIN_WIDTH. Two derived tokens
    // (`tok::KEY_COL`, `tok::BTN_SM`) existed only to keep that figure above
    // zero, and Task 7 retires both.
    //
    // Bound once, and named rather than left as `y`, because the group's top
    // edge is now a coordinate other controls are placed against -- the
    // empty-state STATIC is the next reader. `y` is a running cursor that
    // three bands above have already moved and a fourth may yet move; this is
    // a fixed point, and the two must not be spelled the same.
    let grp_y = y;
    let grp_x = cx;
    let grp_w = cw;
    // Caption inset, then the content, then a bottom inset the size of the
    // gap -- `grp_h` itself is computed above band 3, which has to yield its
    // own height to it.
    let ins_x = grp_x + gap;
    let ins_w = clamp(grp_w - gap * 2);
    place(IDC_GRP_EDITOR, grp_x, grp_y, grp_w, grp_h);

    // Both lines share one label column, so `App` and `Shortcut` left-align
    // with each other instead of each starting wherever its own line does.
    //
    // A hair of slack past the measured width: a STATIC clips to its rect,
    // and SS_CENTERIMAGE clips harder because it also refuses to wrap.
    let lw_lbl = tw("Shortcut").max(tw("App")) + s(4);
    let fld_x = ins_x + lw_lbl + lblgap;
    let fld_w = clamp(ins_x + ins_w - fld_x);

    // Line 1: App, full width.
    let mut ly = grp_y + s(24);
    place(IDC_LBL_APP, ins_x, ly, lw_lbl, ctl);
    // A COMBOBOX's `cy` is the height of its DROPPED-DOWN list, not of the
    // closed control -- and under comctl32 v6 even that is capped by
    // `build_children`'s CB_SETMINVISIBLE(8). The closed height is the
    // system's to choose from the font, which is why `combo_h` above asks
    // what it took rather than guessing a chrome delta the next font change
    // would invalidate.
    //
    // A single-line EDIT draws its text at the TOP of its client rect --
    // Win32 gives it no vertical centring at all -- so the fields are centred
    // within their line (`edit_dy`) rather than stretched to it, and take the
    // height the COMBOBOX's theme picked. `field_h` is what the font alone
    // justifies, and remains the fallback for when the combo cannot be
    // measured -- plus the unit of the dropped-down list's height.
    place_h(ui.app, fld_x, ly + edit_dy, fld_w, field_h * 9);
    ly += ctl + gap;

    // Line 2: the shortcut. Chips left, then the key list, then the two
    // commands right-aligned -- the same "commands close the line" rule band
    // 2's Add/Remove follow.
    place(IDC_LBL_SHORTCUT, ins_x, ly, lw_lbl, ctl);
    // Sized from `RECORD`, never from `STOP`: the armed caption is the
    // narrower of the two, so a caption flip cannot clip and `layout` never
    // has to run on the capture path -- which matters, because `layout` means
    // `SetWindowPos` on the populated App combo, the measured data-loss call
    // (`Ui::shown_external`).
    let bw_record = btn(cap::RECORD);
    let bw_reset = btn(cap::RESET);
    let res_x = ins_x + clamp(ins_w - bw_reset);
    let rec_x = ins_x + clamp(ins_w - bw_reset - gap - bw_record);
    // Each chip is its caption plus `glyph`, floored at `tok::CHIP_MIN` --
    // exactly as band 6's `Hold` chips are sized, same two constants, one
    // rule. `chip` is declared above band 4 so both rows read from it.
    let w_mod_ctrl = chip(cap::MOD_CTRL);
    let w_mod_win = chip(cap::MOD_WIN);
    let w_mod_alt = chip(cap::MOD_ALT);
    let w_mod_shift = chip(cap::MOD_SHIFT);
    // Chips and key list share the fields' midline (`edit_dy`) and their
    // height, so App, the key list and the filter are ONE box repeated
    // rather than three boxes that happen to be concentric. Measured at
    // 144 DPI before the fields were unified: EDIT 43 px against the
    // combo's 36, centres agreeing to within half a pixel -- which reads as
    // a mistake rather than as a pair. `draw_chip` centres its keycap inside
    // whatever rect the chip is given, both ways, so `edit_h` needs no
    // separate rule for the four of them.
    let mut mx = fld_x;
    place(IDC_MOD_CTRL, mx, ly + edit_dy, w_mod_ctrl, edit_h);
    mx += w_mod_ctrl + gap;
    place(IDC_MOD_WIN, mx, ly + edit_dy, w_mod_win, edit_h);
    mx += w_mod_win + gap;
    place(IDC_MOD_ALT, mx, ly + edit_dy, w_mod_alt, edit_h);
    mx += w_mod_alt + gap;
    place(IDC_MOD_SHIFT, mx, ly + edit_dy, w_mod_shift, edit_h);
    mx += w_mod_shift + gap;
    // The key list takes what is between the chips and the commands, under
    // the shortcut column's ceiling. It no longer needs a token of its own:
    // with App on line 1 there is nothing left on this line for it to starve.
    //
    // **Where line 2 runs out.** The key list is now what this line leaves
    // over, so it is the figure worth writing down -- the role `app_w` used
    // to play. At 96 DPI, with `Record` and `Reset` both pinned to `tok::BTN`
    // (88 px; neither caption needs more, which is what makes `tok::BTN_SM`
    // redundant), the fixed part of the line is
    //
    //   lw_lbl(~54) + lblgap(12) + four chips(~190) + 6*gap(48)
    //     + bw_record(88) + bw_reset(88)   =   ~480 px of `ins_w`
    //
    // -- SIX gaps, not five: three between the chips, one after `Shift`, one
    // between the key list and `Record`, one between the two commands. The
    // chip figure is the four `chip()` widths, i.e. `4 * glyph` (96 px) plus
    // the four measured captions, except that `Alt` is short enough to take
    // `tok::CHIP_MIN` instead; `lw_lbl` is `tw("Shortcut") + 4`, the wider of
    // the two labels.
    //
    // `ins_w` is `cw - 2*gap` and `cw` is `w - 2*pad`, so below its ceiling
    // this whole expression collapses to `key_w = min(200, w - 528)` -- the
    // key list clamps to zero at a raw client `w` of ~528, and above that it
    // IS the margin over that zero point. One number, read two ways.
    //
    // `MIN_WIDTH` is a WINDOW floor, not a `cw` one: `WM_GETMINMAXINFO` sets
    // `ptMinTrackSize.x`, which bounds the whole window including the OS's
    // own frame, so its 720 does not translate into an exact `cw` this file
    // computes. Compare like for like -- client against client -- and a
    // 720 px window with a 16 px frame gives `w = 704`, i.e. ~176 px clear of
    // the zero point, which is why a drag cannot reach it. **That margin is a
    // ceiling, not a floor**: a wider OS frame leaves a narrower client, so
    // the figure only ever falls from 185. (Do not compare the 720 against
    // the 519 directly for a "~200 px" margin -- that is the window-against-
    // client mistake this paragraph exists to avoid.)
    //
    // Concretely at `MIN_WIDTH`, then: the key list is 176 px of its 200 px
    // ceiling -- the same 176, necessarily, by the collapse above -- and the
    // App combo on line 1 has ~590 px, where the old one-line strip left it
    // 59. Every subtraction here is clamped regardless, because
    // `WM_DPICHANGED` can suggest a rect below that floor without asking
    // `WM_GETMINMAXINFO`.
    let key_w = s(tok::SHORTCUT_COL).min(clamp(rec_x - gap - mx));
    // `cy` is the DROPPED-DOWN height here too, capped by the same
    // CB_SETMINVISIBLE(8) the App combo carries.
    place_h(ui.combo, mx, ly + edit_dy, key_w, field_h * 9);
    // Buttons honour `cy` and look right at the band height, so they take
    // `ctl` directly and sit on the band line rather than on the fields'
    // midline -- the same rule the command bar's three follow.
    place(IDC_RECORD, rec_x, ly, bw_record, ctl);
    place(IDC_RESET, res_x, ly, bw_reset, ctl);
    ly += ctl + gap;

    // Line 3: the notes, inside the group and beside what they describe.
    // Fixed height -- see `notes_height`. It used to take every pixel down to
    // the keyboard group, which measured as a 1220x177 control holding one
    // 258 px line.
    place_h(ui.notes, ins_x, ly, ins_w, notes_h);

    // **`y` deliberately stops here.** Band 5 has no control and bands 6 and
    // 7 are anchored to `kb_y` / `bar_y`, so the `y += grp_h + band` that
    // would close this band is a store nothing reads -- and the compiler says
    // so (`unused_assignments`). Restore it, with its `y.min(kb_y)` guard, the
    // moment band 5 grows a control.
    //
    // The guard THAT deleted line used to carry -- not band 3's surviving
    // `y.min(kb_y)`, which still bounds `grp_y` and is still needed -- has
    // moved to band 3's `editor_min`. That is the only place it can still do
    // anything: `grp_y` is read before the group is placed, so clamping `y`
    // afterwards moves nothing.

    // -- Band 5: the suggestion row. No control, no height.

    // -- Band 6: the keyboard group. ONE line, left to right: the check box,
    // then `Hold` and its three chips, then `Tap` and its combo.
    place(IDC_GRP_KEYBOARD, cx, kb_y, cw, kb_h);
    let inner_x = cx + gap;
    let ry = kb_y + s(24);
    // Every width on this line comes from the caption it has to hold. The
    // s(190)/s(70)/s(90) constants the radios used were sized for one font
    // at one DPI and clipped the moment either changed.
    //
    // `glyph` -- the check box's own square plus the gap before its caption
    // -- is declared above band 4, which sizes its four modifier chips by
    // the same rule. The two STATICs get a hair of slack instead, for the
    // reason the editor strip's labels do: SS_CENTERIMAGE clips rather than
    // wraps.
    let w_caps = tw(cap::CAPS) + glyph;
    let w_hold = tw(cap::HOLD) + s(4);
    let w_ctrl = chip(cap::HOLD_CTRL);
    let w_win = chip(cap::HOLD_WIN);
    let w_alt = chip(cap::HOLD_ALT);
    let w_tap = tw(cap::TAP) + s(4);
    // `gap * 2` between the three sections of the line, `lblgap` between a
    // word and what it names, `gap` between chips -- so the grouping is
    // legible from the spacing rather than only from the words.
    let mut kx = inner_x;
    place(IDC_CAPS, kx, ry, w_caps, ctl);
    kx += w_caps + gap * 2;
    place(IDC_LBL_HOLD, kx, ry, w_hold, ctl);
    kx += w_hold + lblgap;
    place(IDC_HOLD_CTRL, kx, ry, w_ctrl, ctl);
    kx += w_ctrl + gap;
    place(IDC_HOLD_WIN, kx, ry, w_win, ctl);
    kx += w_win + gap;
    place(IDC_HOLD_ALT, kx, ry, w_alt, ctl);
    kx += w_alt + gap * 2;
    place(IDC_LBL_TAP, kx, ry, w_tap, ctl);
    kx += w_tap + lblgap;
    // Whatever the line has left, capped at the same width the filter box
    // and the key list take, so every box in the window narrows
    // together. Clamped like every other subtraction here: a window dragged
    // narrow must produce a combo with no width, never a negative one.
    //
    // The `cy` is the DROPPED-DOWN height, not the closed one -- see the App
    // combo in band 4. Three items need far less than the eight that combo
    // asks for, so there is no CB_SETMINVISIBLE to go with it.
    let tap_w = s(tok::SHORTCUT_COL).min(clamp(cx + cw - gap - kx));
    place(IDC_TAP, kx, ry + edit_dy, tap_w, field_h * 5);

    // -- Band 7: the command bar. Save is the outermost button on the right,
    // Close inboard of it, `Open config file` hard left -- as far from Save
    // as the bar allows.
    let bw_open = btn(cap::OPEN_FILE);
    let bw_apply = btn(cap::SAVE);
    let bw_close = btn(cap::CLOSE);
    place(IDC_OPENFILE, cx, bar_y, bw_open, ctl);
    place(IDC_APPLY, cx + clamp(cw - bw_apply), bar_y, bw_apply, ctl);
    place(
        IDC_CLOSE,
        cx + clamp(cw - bw_apply - gap - bw_close),
        bar_y,
        bw_close,
        ctl,
    );
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
    let cap_notes: Option<String> = UI.with(|u| {
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
                match &cap_notes {
                    Some(t) => set_text(notes, t),
                    None => {
                        // Caps at two NOTES, not two RENDERED lines.
                        // `IDC_NOTES` is `SS_LEFT` and word-wraps (see its
                        // own comment above), so this cap does not bound
                        // what actually reaches the screen: a single note
                        // wider than the control's inset width wraps to two
                        // lines on its own, a second note then lands on a
                        // clipped third line, and "(+N more)" -- appended to
                        // the end of note 2 -- inherits that clipping, so
                        // the "there is more" text can itself be the part
                        // nobody sees. `notes_height` reserves exactly two
                        // RENDERED lines; nothing here guarantees these two
                        // NOTES fit inside them. The real fix is
                        // measure-and-truncate or an owner-draw `DrawText`
                        // with `DT_WORDBREAK | DT_END_ELLIPSIS` -- out of
                        // scope for this landing; see the hardware
                        // checklist's wrap-case entry.
                        const NOTE_LINES: usize = 2;
                        let body: Vec<String> = d
                            .notes
                            .iter()
                            .take(NOTE_LINES)
                            .map(|n| format!("{}  {}", mark_glyph(n.mark), n.text))
                            .collect();
                        let mut text = body.join("\r\n");
                        if d.notes.len() > NOTE_LINES {
                            text.push_str(&format!("  (+{} more)", d.notes.len() - NOTE_LINES));
                        }
                        set_text(notes, &text);
                    }
                }
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
                set_text(
                    notes,
                    cap_notes
                        .as_deref()
                        .unwrap_or("Select a shortcut, or press Add."),
                );
            }
        }
        // The group's caption, and it is a TEXT write, not a geometry one: it
        // must never reach `layout`, because `layout` means `SetWindowPos` on
        // the populated App combo -- the measured data-loss call (`Ui::shown_external`).
        // A group box caption is never measured by `layout`, so there is no
        // second path back in.
        //
        // **`&` is DOUBLED here, and only here.** A `BS_GROUPBOX` is a BUTTON,
        // and a button caption reads a lone `&` as a mnemonic prefix: it is
        // not drawn, and the letter after it gets an underline that steals a
        // key. The two static captions (`cap::EDITOR_NONE` /
        // `EDITOR_UNNAMED`) need no escape because they simply contain no
        // `&` -- see the note on them. This third caption is the only one in
        // the window fed from the CATALOG, and Start Menu names really do
        // carry ampersands: `SS_NOPREFIX_STYLE`'s comment names `Notes & To
        // Do` and `Arts & Crafts` for exactly this reason. Unescaped, the
        // first draws as `Editing "Notes  To Do"` with **T** underlined --
        // colliding with the `Ctrl` hold chip -- and the second underlines
        // **C**, colliding with `Close`. There is no `SS_NOPREFIX` for a
        // button, so doubling is the only route.
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

        show(banner, external_change);
        show(reload, external_change);
        show(keep, external_change);
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
        // These two do not cover `layout`'s fifth input, the list's client
        // width; that omission is deliberate and is argued at the column
        // sizing inside `layout`.
        let list_empty = st.items.is_empty();
        let relayout = UI.with(|u| {
            u.borrow()
                .as_ref()
                .map(|x| {
                    x.shown_external != Some(external_change) || x.shown_empty != Some(list_empty)
                })
                .unwrap_or(true)
        });
        if relayout {
            layout(hwnd);
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
        // The `Mark` rides in `lParam` (see `rebuild_list`), so the diff path
        // has to move it too -- a row whose condition changes without its
        // count changing takes THIS path, which is most of them: a chord
        // becoming available, an app appearing in the catalog, a pause.
        // Guarded by a compare for `set_item_state`'s reason, one message per
        // row per keystroke otherwise.
        if prev[i].mark != it.mark {
            let it2 = LVITEMW {
                mask: LVIF_PARAM,
                iItem: i as i32,
                lParam: LPARAM(mark_code(it.mark)),
                ..Default::default()
            };
            SendMessageW(
                list,
                LVM_SETITEMW,
                Some(WPARAM(0)),
                Some(LPARAM(&it2 as *const _ as isize)),
            );
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
        // `LVIF_PARAM` carries the row's `Mark` to the painter, and it is the
        // ONLY thing `lParam` is used for -- there are still no ids in it and
        // still no keyed reconciliation. Custom draw cannot borrow `UI`, and
        // inferring severity from the flag WORD instead would be a second
        // description of what `row_condition` already decided: the whole
        // point of that function is that the cell and the note cannot
        // disagree, and a painter that re-derives severity is exactly how
        // they would start to.
        let item = LVITEMW {
            mask: LVIF_TEXT | LVIF_STATE | LVIF_PARAM,
            iItem: i as i32,
            iSubItem: 0,
            pszText: windows::core::PWSTR(first.as_mut_ptr()),
            lParam: LPARAM(mark_code(it.mark)),
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

/// A `Mark` as an `lParam`, and back.
///
/// The pair exists because custom draw gets an `isize` and must not borrow
/// `UI` to interpret it. `Unknown` is the fallback on purpose: a row whose
/// `lParam` was never set -- which is what a `LVM_SETITEMW` that failed
/// leaves -- must read as "no opinion" and take the plain ink, not as a
/// severity it never had.
fn mark_code(m: Mark) -> isize {
    match m {
        Mark::Ok => 1,
        Mark::Warn => 2,
        Mark::Bad => 3,
        Mark::Unknown => 4,
    }
}

fn mark_of_code(v: isize) -> Mark {
    match v {
        1 => Mark::Ok,
        2 => Mark::Warn,
        3 => Mark::Bad,
        _ => Mark::Unknown,
    }
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
/// Only the *shape* of a keycap consults it. Every colour comes from
/// `GetSysColor`, which is already correct in high contrast without asking --
/// what is not correct there is a rounded box with a soft bottom edge, which
/// reads as a rendering artefact against a theme built on flat fills and hard
/// borders.
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

/// Paint `Save` as the accent-filled primary action.
///
/// **The accent marks the primary action; the default ring marks where Enter
/// goes, and they are not the same thing.** `set_default_id` moves the ring
/// onto whichever push button has focus, so tabbing to `Close` takes the ring
/// away from Save -- correctly, because Enter then closes. Save stays filled
/// throughout, because it is still the action the window is for. Nothing here
/// touches the ring.
///
/// **Disabled is the common state**, not an edge case: Save is greyed until
/// there is something to save. It takes `COLOR_BTNFACE` and `COLOR_GRAYTEXT`,
/// so a window with no edits does not show a bright blue button that does
/// nothing.
///
/// High contrast keeps `COLOR_HIGHLIGHT` -- it is a real colour there, and it
/// is the one the theme uses for exactly this -- but drops the rounded
/// corners, on `draw_keycaps`' rule.
unsafe fn save_custom_draw(hwnd: HWND, p: *const NMCUSTOMDRAW) -> isize {
    let cd = &*p;
    if cd.dwDrawStage != CDDS_PREPAINT {
        return CDRF_DODEFAULT as isize;
    }
    let btn = cd.hdr.hwndFrom;
    let hdc = cd.hdc;
    let rc = cd.rcItem;
    let dpi = GetDpiForWindow(hwnd).max(96);
    let hc = high_contrast();
    let disabled = cd.uItemState.0 & CDIS_DISABLED.0 != 0;
    let pressed = cd.uItemState.0 & CDIS_SELECTED.0 != 0;
    let hot = cd.uItemState.0 & CDIS_HOT.0 != 0;

    // The parent's surface first: a rounded button leaves its corners
    // showing, and whatever was there last frame would stay in them.
    FillRect(hdc, &rc, GetSysColorBrush(COLOR_BTNFACE));

    let accent = COLORREF(GetSysColor(COLOR_HIGHLIGHT));
    let (fill, ink) = if disabled {
        (
            COLORREF(GetSysColor(COLOR_BTNFACE)),
            COLORREF(GetSysColor(COLOR_GRAYTEXT)),
        )
    } else if pressed {
        (
            shade(accent, 4, 5),
            COLORREF(GetSysColor(COLOR_HIGHLIGHTTEXT)),
        )
    } else if hot {
        (
            shade(accent, 9, 10),
            COLORREF(GetSysColor(COLOR_HIGHLIGHTTEXT)),
        )
    } else {
        (accent, COLORREF(GetSysColor(COLOR_HIGHLIGHTTEXT)))
    };
    // A disabled button needs an outline or it is a hole in the window;
    // a filled one is its own shape.
    let border = if disabled {
        COLORREF(GetSysColor(COLOR_BTNSHADOW))
    } else {
        fill
    };
    let brush = CreateSolidBrush(fill);
    let pen = CreatePen(PS_SOLID, 1, border);
    let pb = SelectObject(hdc, HGDIOBJ(brush.0));
    let pp = SelectObject(hdc, HGDIOBJ(pen.0));
    if hc {
        let _ = Rectangle(hdc, rc.left, rc.top, rc.right, rc.bottom);
    } else {
        let r = scale(5, dpi) * 2;
        let _ = RoundRect(hdc, rc.left, rc.top, rc.right, rc.bottom, r, r);
    }
    if !pp.is_invalid() {
        SelectObject(hdc, pp);
    }
    let _ = DeleteObject(HGDIOBJ(pen.0));
    if !pb.is_invalid() {
        SelectObject(hdc, pb);
    }
    let _ = DeleteObject(HGDIOBJ(brush.0));

    let font = HFONT(
        SendMessageW(btn, WM_GETFONT, Some(WPARAM(0)), Some(LPARAM(0))).0 as *mut core::ffi::c_void,
    );
    let prev = if font.is_invalid() {
        HGDIOBJ::default()
    } else {
        SelectObject(hdc, HGDIOBJ(font.0))
    };
    SetBkMode(hdc, TRANSPARENT);
    SetTextColor(hdc, ink);
    // `&Save`'s mnemonic, on the window's own UI state -- the same read the
    // chips make, and for the same reason.
    let ui_state = SendMessageW(btn, WM_QUERYUISTATE, Some(WPARAM(0)), Some(LPARAM(0))).0 as u32;
    let mut flags = DT_CENTER | DT_VCENTER | DT_SINGLELINE;
    if ui_state & UISF_HIDEACCEL != 0 {
        flags |= DT_HIDEPREFIX;
    }
    let caption = text_of(btn);
    let mut t = wide(&caption);
    let n = t.len() - 1;
    let mut tr = rc;
    DrawTextW(hdc, &mut t[..n], &mut tr, flags);
    if !prev.is_invalid() {
        SelectObject(hdc, prev);
    }

    if cd.uItemState.0 & CDIS_FOCUS.0 != 0 && ui_state & UISF_HIDEFOCUS == 0 {
        let d = scale(3, dpi);
        let f = RECT {
            left: rc.left + d,
            top: rc.top + d,
            right: rc.right - d,
            bottom: rc.bottom - d,
        };
        let _ = DrawFocusRect(hdc, &f);
    }
    CDRF_SKIPDEFAULT as isize
}

/// The pill colours for a flag: `(fill, ink)`.
///
/// **The one place in this window that uses colour literals, and the reason
/// is that the system palette has no opinion here.** There is no
/// `COLOR_WARNING`; Windows' own shell draws these states with semantic
/// colours of its own. The values are direction B's Windows palette --
/// `--w-warn:#9d5d00` on `--w-warn-bg:#fff4ce`, `--w-crit:#c42b1c` on
/// `--w-crit-bg:#fdf3f4`, `--w-good:#0f7b0f`.
///
/// **`None` means no pill**: draw the flag as plain secondary text. That is
/// what `Mark::Unknown` gets -- a row nobody has an opinion about -- and it
/// is also what every mark gets in high contrast and on a selected row, where
/// a pale fill would be either a lie about the theme or unreadable on the
/// accent. Those two cases are the CALLER's to decide; this function only
/// knows the mark.
fn flag_colours(m: Mark) -> Option<(COLORREF, COLORREF)> {
    match m {
        Mark::Bad => Some((COLORREF(0x00F4F3FD), COLORREF(0x001C2BC4))),
        Mark::Warn => Some((COLORREF(0x00CEF4FF), COLORREF(0x00005D9D))),
        Mark::Ok => Some((COLORREF(0x00E6F4E6), COLORREF(0x000F7B0F))),
        Mark::Unknown => None,
    }
}

/// Paint the App column: the app name, then its flag as a coloured pill.
///
/// **The flag is why this exists.** It used to be three spaces and more of
/// the same Body text, because a ListView draws a cell in the control's one
/// font and there is no per-run font in a report view -- so `not installed`
/// and `key in use` said exactly as much as the app's own name did. The
/// point of the flag is that a row in trouble stands out.
///
/// **Nothing here reads `UI`**, on `list_custom_draw`'s rule: the text comes
/// from the control, the severity from `lParam` (which `rebuild_list` and
/// `sync_list` both set), and the Caption font from `CAP_FONT`.
///
/// **The tick's strip is never painted.** Drawing starts at `LVIR_LABEL`'s
/// left edge, so whatever comctl32 does with the state image happens on
/// pixels this function has not touched -- which is what makes
/// `TICK_SURVIVES` a property of the code rather than of the draw order.
unsafe fn draw_app_cell(hwnd: HWND, cd: &NMLVCUSTOMDRAW) -> isize {
    let list = cd.nmcd.hdr.hwndFrom;
    let row = cd.nmcd.dwItemSpec;
    let cell = subitem_text(list, row, 0);
    if cell.is_empty() {
        return CDRF_DODEFAULT as isize;
    }
    let (name, flag) = beckon_core::settings::split_app_cell(&cell);
    // Nothing to colour, so nothing to take over: a healthy row is the
    // common case and comctl32 draws plain text better than this does
    // (ellipsis, selection, focus rectangle).
    let Some(flag) = flag else {
        return CDRF_DODEFAULT as isize;
    };
    let Some(cap) = cap_font() else {
        return CDRF_DODEFAULT as isize;
    };

    // `LVIR_LABEL` on the ITEM, which in a report view is the text area of
    // column 0 -- i.e. past the state image. `LVM_GETSUBITEMRECT` would give
    // the whole column, tick included.
    let mut rc = RECT {
        left: LVIR_LABEL as i32,
        ..Default::default()
    };
    let ok = SendMessageW(
        list,
        LVM_GETITEMRECT,
        Some(WPARAM(row)),
        Some(LPARAM(&mut rc as *mut RECT as isize)),
    );
    if ok.0 == 0 || rc.right <= rc.left {
        return CDRF_DODEFAULT as isize;
    }
    // Column 0's right edge, so a long name is clipped at the column rather
    // than running under the Shortcut caps.
    let mut col = RECT {
        left: LVIR_BOUNDS as i32,
        top: 0,
        right: 0,
        bottom: 0,
    };
    if SendMessageW(
        list,
        LVM_GETSUBITEMRECT,
        Some(WPARAM(row)),
        Some(LPARAM(&mut col as *mut RECT as isize)),
    )
    .0 != 0
        && col.right > rc.left
    {
        rc.right = col.right;
    }

    let hdc = cd.nmcd.hdc;
    let sel = SendMessageW(
        list,
        LVM_GETITEMSTATE,
        Some(WPARAM(row)),
        Some(LPARAM(LVIS_SELECTED.0 as isize)),
    )
    .0 != 0;
    let bg = if sel { COLOR_HIGHLIGHT } else { COLOR_WINDOW };
    FillRect(hdc, &rc, GetSysColorBrush(bg));

    let dpi = GetDpiForWindow(hwnd).max(96);
    let hc = high_contrast();
    let ink = if sel {
        COLOR_HIGHLIGHTTEXT
    } else {
        COLOR_WINDOWTEXT
    };
    SetBkMode(hdc, TRANSPARENT);

    // The name, in the list's own font -- taken from the control rather than
    // from `Fonts`, for the same reason the chips do it.
    let body = HFONT(
        SendMessageW(list, WM_GETFONT, Some(WPARAM(0)), Some(LPARAM(0))).0
            as *mut core::ffi::c_void,
    );
    let prev = if body.is_invalid() {
        HGDIOBJ::default()
    } else {
        SelectObject(hdc, HGDIOBJ(body.0))
    };
    let mut nw = SIZE::default();
    let nt = wide(name);
    let _ = GetTextExtentPoint32W(hdc, &nt[..nt.len() - 1], &mut nw);
    SetTextColor(hdc, COLORREF(GetSysColor(ink)));
    let mut ntr = rc;
    let mut nbuf = wide(name);
    let n = nbuf.len() - 1;
    DrawTextW(
        hdc,
        &mut nbuf[..n],
        &mut ntr,
        DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS | DT_NOPREFIX,
    );

    // The pill, in Caption, after the name plus the separator's worth of
    // space. Skipped entirely when the name has already used the column --
    // a half-drawn pill is worse than none.
    let gap = scale(8, dpi);
    let px = rc.left + nw.cx + gap;
    if !prev.is_invalid() {
        SelectObject(hdc, prev);
    }
    let prev_cap = SelectObject(hdc, HGDIOBJ(cap.0));
    let mut fw = SIZE::default();
    let ft = wide(flag);
    let _ = GetTextExtentPoint32W(hdc, &ft[..ft.len() - 1], &mut fw);
    let padx = scale(7, dpi);
    let pill_w = fw.cx + padx * 2;
    if px + pill_w <= rc.right {
        let mark = mark_of_code(cd.nmcd.lItemlParam.0);
        // No fill on a selected row or in high contrast -- see
        // `flag_colours`. Both would be a pale rectangle on an accent or a
        // literal under a theme built to have none.
        let paint = if sel || hc { None } else { flag_colours(mark) };
        let pill_h = (fw.cy + scale(4, dpi)).min(rc.bottom - rc.top);
        let py = rc.top + (rc.bottom - rc.top - pill_h) / 2;
        let flag_ink = match paint {
            Some((fill, ink)) => {
                let brush = CreateSolidBrush(fill);
                let pb = SelectObject(hdc, HGDIOBJ(brush.0));
                let pen = CreatePen(PS_SOLID, 1, fill);
                let pp = SelectObject(hdc, HGDIOBJ(pen.0));
                let r = pill_h;
                let _ = RoundRect(hdc, px, py, px + pill_w, py + pill_h, r, r);
                if !pp.is_invalid() {
                    SelectObject(hdc, pp);
                }
                let _ = DeleteObject(HGDIOBJ(pen.0));
                if !pb.is_invalid() {
                    SelectObject(hdc, pb);
                }
                let _ = DeleteObject(HGDIOBJ(brush.0));
                ink
            }
            None => COLORREF(GetSysColor(if sel {
                COLOR_HIGHLIGHTTEXT
            } else {
                COLOR_GRAYTEXT
            })),
        };
        SetTextColor(hdc, flag_ink);
        let mut ftr = RECT {
            left: px,
            top: py,
            right: px + pill_w,
            bottom: py + pill_h,
        };
        let mut fbuf = wide(flag);
        let f = fbuf.len() - 1;
        DrawTextW(
            hdc,
            &mut fbuf[..f],
            &mut ftr,
            DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX,
        );
    }
    if !prev_cap.is_invalid() {
        SelectObject(hdc, prev_cap);
    }
    CDRF_SKIPDEFAULT as isize
}

/// Paint the Shortcut column as keycaps. Subitem 1, and only subitem 1.
///
/// **Both subitems are drawn now.** Subitem 0 carries `LVS_EX_CHECKBOXES`'
/// state image -- the tick that makes `Remove` a multi-delete -- and whether
/// `CDRF_SKIPDEFAULT` there takes the tick with it was an open hardware
/// question until `examples/customdraw_probe.rs` was finally run on a14
/// (2026-08-13, `VERDICT=TICK_SURVIVES`). See `draw_app_cell`.
///
/// **Nothing here reads `UI`.** Everything it needs comes from the
/// notification itself or from a `Cell`: the list handle from `hdr.hwndFrom`,
/// the chord from the cell's own text, the font from `CAP_FONT`. A paint can
/// arrive while `UI` is borrowed, and it does.
unsafe fn list_custom_draw(hwnd: HWND, p: *const NMLVCUSTOMDRAW) -> isize {
    let cd = &*p;
    let stage = cd.nmcd.dwDrawStage;
    if stage == CDDS_PREPAINT {
        return CDRF_NOTIFYITEMDRAW as isize;
    }
    if stage == CDDS_ITEMPREPAINT {
        return CDRF_NOTIFYSUBITEMDRAW as isize;
    }
    // `NMCUSTOMDRAW_DRAW_STAGE` has no `BitOr` in `windows` 0.61 -- unlike the
    // flag types it is a bare newtype, not a generated bitmask type. Compare
    // the raw u32s; `examples/customdraw_probe.rs` found this the hard way.
    if stage.0 != CDDS_ITEMPREPAINT.0 | CDDS_SUBITEM.0 {
        return CDRF_DODEFAULT as isize;
    }
    // **Subitem 0 is no longer left alone.** It carries `LVS_EX_CHECKBOXES`'
    // state image -- the tick that makes `Remove` a multi-delete -- and
    // whether `CDRF_SKIPDEFAULT` there takes the tick with it was the open
    // hardware question `examples/customdraw_probe.rs` exists to answer.
    // **Answered on a14 2026-08-13: `VERDICT=TICK_SURVIVES`**, with a
    // default-drawn control row reading the same 306 ink pixels as the
    // skipped one. `draw_app_cell` still never paints over the tick's strip,
    // which makes the result independent of the draw ordering rather than
    // dependent on it.
    if cd.iSubItem == 0 {
        return draw_app_cell(hwnd, cd);
    }
    if cd.iSubItem != 1 {
        return CDRF_DODEFAULT as isize;
    }
    let Some(font) = cap_font() else {
        return CDRF_DODEFAULT as isize;
    };

    let list = cd.nmcd.hdr.hwndFrom;
    let row = cd.nmcd.dwItemSpec;
    let shown = subitem_text(list, row, 1);
    if shown.is_empty() {
        return CDRF_DODEFAULT as isize;
    }

    // `LVM_GETSUBITEMRECT` rather than `nmcd.rc`: the message is unambiguous
    // about which rect it returns, and it takes the subitem in `rc.top` and
    // the part in `rc.left`, which is the documented calling convention rather
    // than a quirk.
    let mut rc = RECT {
        left: LVIR_BOUNDS as i32,
        top: 1,
        right: 0,
        bottom: 0,
    };
    let ok = SendMessageW(
        list,
        LVM_GETSUBITEMRECT,
        Some(WPARAM(row)),
        Some(LPARAM(&mut rc as *mut RECT as isize)),
    );
    if ok.0 == 0 || rc.right <= rc.left {
        return CDRF_DODEFAULT as isize;
    }

    let hdc = cd.nmcd.hdc;
    // **`LVM_GETITEMSTATE`, not `nmcd.uItemState`.** At the SUBITEM stage
    // comctl32 reports `CDIS_SELECTED` for every row regardless of the real
    // selection -- measured on a14: with nothing selected, the whole Shortcut
    // column painted `COLOR_HIGHLIGHT`. The control's own answer is the only
    // one worth asking at this stage.
    let sel = SendMessageW(
        list,
        LVM_GETITEMSTATE,
        Some(WPARAM(row)),
        Some(LPARAM(LVIS_SELECTED.0 as isize)),
    )
    .0 != 0;
    // `CDRF_SKIPDEFAULT` means we own the background too, not only the text.
    // Getting this wrong shows up as a selected row with one un-highlighted
    // cell, which is worse than no keycaps at all. `GetSysColorBrush` returns
    // a system brush and must not be deleted.
    let bg = if sel { COLOR_HIGHLIGHT } else { COLOR_WINDOW };
    FillRect(hdc, &rc, GetSysColorBrush(bg));

    // The cell holds `combo_display`'s output, so splitting on its separator
    // recovers exactly the caps `combo_caps` would have produced -- without a
    // second source of truth to keep in step.
    let caps: Vec<String> = shown.split(" + ").map(|s| s.to_string()).collect();
    let dpi = GetDpiForWindow(hwnd).max(96);
    if !draw_keycaps(hdc, rc, &caps, font, dpi, high_contrast(), CapStyle::Chord) {
        let mut tr = RECT {
            left: rc.left + scale(6, dpi),
            ..rc
        };
        let mut t = wide(&shown);
        let n = t.len() - 1;
        SetBkMode(hdc, TRANSPARENT);
        SetTextColor(
            hdc,
            COLORREF(GetSysColor(if sel {
                COLOR_HIGHLIGHTTEXT
            } else {
                COLOR_WINDOWTEXT
            })),
        );
        DrawTextW(
            hdc,
            &mut t[..n],
            &mut tr,
            DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS | DT_NOPREFIX,
        );
    }
    CDRF_SKIPDEFAULT as isize
}

/// Paint one toggle chip as a keycap. The four modifier chips and the three
/// `Hold` chips, and nothing else in this window is owner-draw.
///
/// **Nothing here reads `UI`**, for `list_custom_draw`'s reason: a paint can
/// arrive while `UI` is borrowed, and it does. Everything comes out of the
/// `DRAWITEMSTRUCT` or out of the control itself -- the caption through
/// `text_of`, the font through `WM_GETFONT`, the armed bit from `CHIPS`,
/// which is a `Cell` precisely so this path cannot be contended.
///
/// **The font is asked of the control, not of `Fonts`.** `child` put the
/// role's font on it at creation and `WM_DPICHANGED` rebroadcasts a new one,
/// so `WM_GETFONT` is the live answer and there is no third copy of the
/// mapping to keep in step. `layout` measures these captions in Body through
/// `tw`, which is the same font, which is what makes the fit check below a
/// real one.
///
/// **Whether the mnemonic underline shows is the WINDOW's UI state**, read
/// with `WM_QUERYUISTATE` rather than `SPI_GETKEYBOARDCUES`. The SPI is the
/// global default; the per-window flags are the live answer, and they are
/// what Windows itself moves -- through `WM_UPDATEUISTATE` -- the moment the
/// user presses Alt or navigates by keyboard. Reading the SPI would leave
/// these three chips underlined while every real control beside them was
/// not. The same read answers the focus rect, which owner-draw also has to
/// draw for itself or the keyboard route is silently lost.
unsafe fn draw_chip(hwnd: HWND, di: &DRAWITEMSTRUCT) -> bool {
    if di.CtlType != ODT_BUTTON {
        return false;
    }
    let Some(bit) = chip_bit(di.CtlID as i32) else {
        return false;
    };
    let hdc = di.hDC;
    let rc = di.rcItem;
    // The parent's background, first and over the WHOLE rect. An owner-draw
    // button draws nothing of itself, its background included, so any pixel
    // this function leaves alone keeps whatever the last frame put there --
    // and the cap is deliberately narrower than its control, so there are
    // plenty of them. `COLOR_BTNFACE` is what the window class registers;
    // `GetSysColorBrush` returns a system brush and must not be deleted.
    FillRect(hdc, &rc, GetSysColorBrush(COLOR_BTNFACE));

    // Never zero in practice -- `child` sets a font on every control it
    // creates -- but a null `HFONT` would make `SelectObject` fail and leave
    // the cap in the DC's own stock font, which at this size is unreadable
    // rather than merely wrong.
    let font = HFONT(
        SendMessageW(di.hwndItem, WM_GETFONT, Some(WPARAM(0)), Some(LPARAM(0))).0
            as *mut core::ffi::c_void,
    );
    let font = if font.is_invalid() {
        HFONT(GetStockObject(DEFAULT_GUI_FONT).0)
    } else {
        font
    };
    let ui_state = SendMessageW(
        di.hwndItem,
        WM_QUERYUISTATE,
        Some(WPARAM(0)),
        Some(LPARAM(0)),
    )
    .0 as u32;
    let style = CapStyle::Toggle {
        armed: chip_armed(bit),
        pressed: di.itemState.0 & ODS_SELECTED.0 != 0,
        disabled: di.itemState.0 & ODS_DISABLED.0 != 0,
        hide_accel: ui_state & UISF_HIDEACCEL != 0,
    };
    let dpi = GetDpiForWindow(hwnd).max(96);
    // Read back from the CONTROL, not from `mod cap`, for `subitem_text`'s
    // reason: what is drawn and what an accessibility client reads out are
    // then the same string by construction, rather than by two code paths
    // agreeing.
    let caps = [text_of(di.hwndItem)];
    if !draw_keycaps(hdc, rc, &caps, font, dpi, high_contrast(), style) {
        // The same fallback the Shortcut column takes, and for the same
        // reason: a clipped keycap reads as a rendering fault, plain text
        // reads as a narrow control. `layout` sizes each chip from its own
        // caption so this should be unreachable -- which is exactly why it
        // must not be an empty control if it ever is.
        let prev = SelectObject(hdc, HGDIOBJ(font.0));
        let mut tr = rc;
        let mut t = wide(&caps[0]);
        let n = t.len() - 1;
        SetBkMode(hdc, TRANSPARENT);
        SetTextColor(
            hdc,
            COLORREF(GetSysColor(if di.itemState.0 & ODS_DISABLED.0 != 0 {
                COLOR_GRAYTEXT
            } else {
                COLOR_BTNTEXT
            })),
        );
        let mut flags = DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS;
        if ui_state & UISF_HIDEACCEL != 0 {
            flags |= DT_HIDEPREFIX;
        }
        DrawTextW(hdc, &mut t[..n], &mut tr, flags);
        if !prev.is_invalid() {
            SelectObject(hdc, prev);
        }
    }
    // XOR-drawn, so it goes on LAST or the fill eats it. Suppressed while the
    // window says cues are hidden, which is the state a mouse-driven session
    // stays in -- the same flag word that decides the underline.
    if di.itemState.0 & ODS_FOCUS.0 != 0 && ui_state & UISF_HIDEFOCUS == 0 {
        let d = scale(1, dpi);
        let f = RECT {
            left: rc.left + d,
            top: rc.top + d,
            right: rc.right - d,
            bottom: rc.bottom - d,
        };
        let _ = DrawFocusRect(hdc, &f);
    }
    true
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
    // invalidates), SM_CXVSCROLL / SM_CYBORDER, and control heights read
    // back through GetWindowRect. Those are exactly the metrics that move
    // when a user enters or leaves high contrast, and layout already
    // queries them at call time instead of assuming a constant -- staying
    // stale here would reintroduce the clipping bug that query was added to
    // fix. Rare, user-initiated events; a handful of extra SetWindowPos
    // calls is not a cost worth avoiding for it.
    layout(hwnd);
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
                LRESULT(0)
            }
            WM_DPICHANGED => {
                // HIWORD(wParam) is the new DPI; lParam is a RECT with the
                // position and size Windows wants. Ignoring lParam leaves
                // the window the wrong size on the new monitor, and no
                // second message arrives to correct it.
                let dpi = ((wp.0 >> 16) & 0xFFFF) as u32;
                let fonts = build_fonts(hwnd, dpi);
                set_cap_font(fonts.get(Role::Caption));
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
                LRESULT(0)
            }
            WM_SYSCOLORCHANGE => {
                // System palette changed (e.g. entering/leaving high
                // contrast). Every control on this window already reads
                // colours through GetSysColor / DefWindowProcW's own
                // COLOR_BTNFACE brush -- see the module-level colour audit
                // -- so there is no cached colour of ours to re-read here;
                // the forward+invalidate is what makes the CHILDREN's own
                // cached colours (edit control backgrounds, ListView text/
                // back colour) catch up.
                broadcast_theme_change(hwnd, msg, wp, lp);
                LRESULT(0)
            }
            WM_THEMECHANGED => {
                // Visual style changed. Themed common controls (the
                // ListView) open their theme handle once and keep it until
                // told otherwise; WM_THEMECHANGED is that notice, and it
                // only reaches top-level windows, hence the forward.
                broadcast_theme_change(hwnd, msg, wp, lp);
                LRESULT(0)
            }
            WM_SETTINGCHANGE => {
                // WM_SETTINGCHANGE fires for dozens of unrelated SPI_
                // actions (wallpaper, mouse trails, ...) -- wParam carries
                // the SPI_ action code when SystemParametersInfo was called
                // with SPIF_SENDCHANGE, which is how Windows reports a
                // high-contrast toggle. Only that one is this window's
                // concern; everything else must fall through to
                // DefWindowProcW untouched rather than relayout on every
                // unrelated settings change.
                if wp.0 == SPI_SETHIGHCONTRAST.0 as usize {
                    // Before the broadcast, so the relayout and every repaint
                    // it triggers already see the new answer.
                    refresh_high_contrast();
                    broadcast_theme_change(hwnd, msg, wp, lp);
                    LRESULT(0)
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
                // Save is the primary action, so B fills it with the accent.
                //
                // **`NM_CUSTOMDRAW`, NOT `BS_OWNERDRAW`.** `BS_OWNERDRAW`
                // replaces the button's TYPE, and Save's type is
                // `BS_DEFPUSHBUTTON` -- the ring `set_default_id` moves around
                // with a `BM_SETSTYLE` read-modify-write through
                // `BS_TYPEMASK_BITS`. Owner-draw would take that machinery
                // with it, and Enter-on-`Reload`-saves is a defect this window
                // has already had once. Custom draw leaves the type, the
                // notifications and the ring exactly as they are and only
                // replaces the pixels.
                if nm.idFrom == IDC_APPLY as usize && nm.code == NM_CUSTOMDRAW {
                    return LRESULT(save_custom_draw(hwnd, lp.0 as *const NMCUSTOMDRAW));
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
                // **One control, by id.** Every other STATIC in this window --
                // the banner, the notes, the four labels -- keeps the default
                // treatment; answering for all of them would silently grey the
                // banner, which is the one piece of text here that must not be
                // played down.
                //
                // `GetSysColorBrush` returns a SYSTEM brush, so it is not
                // deleted and can be returned safely. `SetBkMode(TRANSPARENT)`
                // rather than a matching background: the group boxes and the
                // window share `COLOR_BTNFACE`, and letting the parent's paint
                // show through is what keeps this correct if that ever stops
                // being true.
                let ctl = HWND(lp.0 as *mut core::ffi::c_void);
                if GetDlgCtrlID(ctl) == IDC_LBL_COUNT {
                    let hdc = HDC(wp.0 as *mut core::ffi::c_void);
                    SetTextColor(hdc, COLORREF(GetSysColor(COLOR_GRAYTEXT)));
                    SetBkMode(hdc, TRANSPARENT);
                    return LRESULT(GetSysColorBrush(COLOR_BTNFACE).0 as isize);
                }
                DefWindowProcW(hwnd, msg, wp, lp)
            }
            WM_DRAWITEM => {
                // The first owner-draw surface in this window, and the seven
                // toggle chips are all of it. Answered BEFORE any
                // `suppressed()` consideration, exactly like the ListView's
                // custom draw one arm up: it is pure painting, it reaches no
                // callback and it cannot recurse into `apply_state`.
                //
                // `DefWindowProcW` on anything else -- and on a menu, whose
                // `CtlID` is 0 and which this window has none of today.
                // Returning 1 for a message we did not draw would leave the
                // control blank.
                let di = &*(lp.0 as *const DRAWITEMSTRUCT);
                if draw_chip(hwnd, di) {
                    LRESULT(1)
                } else {
                    DefWindowProcW(hwnd, msg, wp, lp)
                }
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
/// Spelled through `Combo::canonical` rather than by joining strings here,
/// so the order this window writes and the order the parser prints cannot
/// drift apart.
fn shortcut_shown(hwnd: HWND, combo: HWND) -> Option<String> {
    let i = cur_sel_raw(combo);
    if i < 0 {
        return None;
    }
    // The index is a position in `key_table()` -- see `set_key_sel` for why
    // that is true and what would break it. `get` rather than an index,
    // because a control is not a proof.
    let key = key_table().get(i as usize)?;
    Some(
        Combo {
            ctrl: is_checked(hwnd, IDC_MOD_CTRL),
            super_: is_checked(hwnd, IDC_MOD_WIN),
            alt: is_checked(hwnd, IDC_MOD_ALT),
            shift: is_checked(hwnd, IDC_MOD_SHIFT),
            key,
        }
        .canonical(),
    )
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
// 3. **There is no path where the window dies with the hook armed.**
//    `end_capture` is idempotent and every route out of this window calls it
//    first: the `Stop` button, all three of spec F.4's focus layers, the
//    watchdog, `WM_CLOSE` (before the save prompt) and `WM_DESTROY`.

/// The notes strip's text while a capture is live: the partial combo, then
/// the hint.
///
/// Indented through `mark_glyph(Mark::Ok)` -- the blank one -- so the two
/// lines sit exactly where a healthy note sits. That is the only reason this
/// goes through the glyph table rather than writing spaces: a second
/// indentation rule is a second thing to keep in step with the first.
///
/// **Two lines is the ceiling `notes_height` reserves, and this fits it
/// exactly, not by accident.** The `Some(p)` arm below IS two lines -- the
/// partial combo, then the hint -- so calling the capture prompt "one line
/// by construction" undercounts it; it fits because two is what it is. A
/// third capture line would clip exactly as a third NOTE line does, and
/// nothing here would stop one from being added -- if one ever is, check it
/// against this ceiling first.
fn capture_notes(c: &Capture) -> String {
    let g = mark_glyph(Mark::Ok);
    match &c.partial {
        Some(p) => format!("{g}  {p}\r\n{g}  {}", c.hint),
        None => format!("{g}  {}", c.hint),
    }
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
        set_text(notes, HINT_UNAVAILABLE);
    }
}

/// Draw the armed state: the two capture lines, the typed path off, the
/// button reading `Stop`.
///
/// Reads `Ui::capture` and does nothing when it is `None`, so it is safe to
/// call from anywhere.
unsafe fn show_capture(hwnd: HWND) {
    // ONE borrow, dropped on this line. `capture_notes` allocates but makes
    // no OS call, so building the string inside it is sound; every send is
    // below.
    let Some((notes, body)) = UI.with(|u| {
        u.borrow()
            .as_ref()
            .and_then(|x| x.capture.as_ref().map(|c| (x.notes, capture_notes(c))))
    }) else {
        return;
    };
    if text_of(notes) != body {
        set_text(notes, &body);
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
        (IDC_APP, c) if c == CBN_KILLFOCUS || c == CBN_CLOSEUP => commit_fields(),
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
        assert_eq!(
            title_base(r"C:\Users\a\AppData\Roaming\beckon\shortcuts.toml"),
            "beckon - shortcuts.toml"
        );
        // Forward slashes are separators on Windows too, and `serve` is
        // perfectly reachable with a path typed that way.
        assert_eq!(
            title_base("C:/cfg/shortcuts.toml"),
            "beckon - shortcuts.toml"
        );
        assert_eq!(title_base("shortcuts.toml"), "beckon - shortcuts.toml");
    }

    #[test]
    fn title_base_falls_back_when_there_is_no_file_name() {
        // Every one of these makes `Path::file_name` return None, and the
        // format string would otherwise put an empty name after the
        // separator -- a title bar reading `beckon - ` looks like the window
        // failed to load something.
        assert_eq!(title_base(""), "beckon");
        assert_eq!(title_base(r"C:\"), "beckon");
        assert_eq!(title_base(".."), "beckon");
        assert_eq!(title_base(r"C:\cfg\.."), "beckon");
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
