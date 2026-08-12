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
//! A deliberate non-feature: there is no "press a key to capture the
//! shortcut" field. `msctls_hotkey32` cannot capture the Windows key and
//! Explorer eats `Win+T` and its siblings before a normal window sees them,
//! so combos are typed as text and validated by the same parser `serve`
//! uses.

use crate::shell;
use beckon_core::settings::{default_button, ControlState, DefaultButton, ListItem, Mark};
use beckon_core::shortcuts::{CapsTap, Chord};
use std::cell::RefCell;
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM};
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
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
/// `\r\n`-joined note lines, and `layout` hands it every pixel the flex band
/// has -- so adding the style would collapse the whole notes band to its
/// first line. Ellipsised multi-line text needs an owner-draw `DrawText`
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
/// The four check boxes are deliberately absent: they carry no `BS_NOTIFY`,
/// they cannot be the default button, and a default ring on a check box is
/// not a thing Windows draws.
const PUSH_BUTTONS: [i32; 7] = [
    IDC_ADD,
    IDC_REMOVE,
    IDC_APPLY,
    IDC_OPENFILE,
    IDC_CLOSE,
    IDC_RELOAD,
    IDC_KEEPMINE,
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
/// **Mnemonics must not collide.** Windows does not check, and a duplicate
/// does not fail -- `Alt+R` simply cycles focus between the claimants
/// instead of pressing either, which reads as "the keyboard is broken"
/// rather than as a conflict. The letters:
///
/// | Key | Control | Key | Control |
/// |---|---|---|---|
/// | `A` | Add | `R` | Reload |
/// | `M` | Re**m**ove | `K` | Keep mine |
/// | `S` | Save | `C` | Use **C**aps Lock (check box) |
/// | `C` | Close | `C` | **C**trl (hold chip) |
/// | `O` | Open config file | `W` | **W**in (hold chip) |
/// |  |  | `L` | A**l**t (hold chip) |
///
/// **`C` is claimed three times, and that is a known defect rather than a
/// decision.** `Close`, the Caps check box and the `Ctrl` chip all take it,
/// so `Alt+C` cycles focus between the three instead of pressing any of
/// them. Both new claimants arrived with the Caps row -- the check box used
/// to be `&Use`, i.e. `U` -- and the captions above are the ones the landing
/// plan specified verbatim, so respelling them was not this pass's call to
/// make. The fix costs nothing but the position of two ampersands, and both
/// letters are free: `&Use Caps Lock as a shortcut key` (`U`) and `C&trl`
/// (`T`).
///
/// `Remove` cannot take `R` because `Reload` has it, and `Reload` is the
/// one that appears without warning -- a banner the user did not ask for is
/// the worse place to make someone hunt for a letter. The two field labels
/// (`App`, `Shortcut`) deliberately carry NO mnemonic: a STATIC's mnemonic
/// moves focus to the next control in tab order, so each one would have to
/// hold a letter for a control that is already one Tab away.
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
    pub const CAPS: &str = "Use &Caps Lock as a shortcut key";
    pub const HOLD: &str = "Hold";
    pub const TAP: &str = "Tap";
    pub const HOLD_CTRL: &str = "&Ctrl";
    pub const HOLD_WIN: &str = "&Win";
    pub const HOLD_ALT: &str = "A&lt";
    /// The three `Tap` items, in `CB_ADDSTRING` order. Read back by INDEX
    /// with `CB_GETCURSEL`, never by text: even a `DROPDOWNLIST` has
    /// typeahead, which moves the selection.
    pub const TAP_ITEMS: [&str; 3] = ["Caps Lock", "Esc", "Nothing"];
    /// The filter box's placeholder. ASCII, like every display string.
    pub const FILTER_CUE: &str = "Filter";
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
    /// The right-aligned `Shortcut` column, and the editor field under it.
    pub const SHORTCUT_COL: i32 = 200;
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
// 860 is spec B.2's stated width. 640 is the height raised from 560 so the
// notes band (the flex band -- see `layout`) fits four lines at 96 DPI: the
// band gets `kb_y`'s leftover directly, so every pixel added here becomes a
// pixel of notes room. Worked through in the task-6 report, section on
// fix 2: at the default size the notes band comes out to ~173 px against a
// ~19-21 px line height, i.e. 8+ lines against a 4-line requirement.
const WINDOW_WIDTH: i32 = 860;
const WINDOW_HEIGHT: i32 = 640;

/// Minimum resize size, at 96 DPI, enforced in `WM_GETMINMAXINFO`. Smaller
/// than `WINDOW_WIDTH`/`WINDOW_HEIGHT` so the window can be shrunk, but not
/// below the point where `layout` starts overlapping controls.
const MIN_WIDTH: i32 = 720;
const MIN_HEIGHT: i32 = 460;

/// One of §B.3's three type roles. There is no fourth: the `Keys` role the
/// spec table also lists belongs to keycap rendering, which this window
/// does not do -- combos are typed as text into an ordinary EDIT.
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
        // Secondary prose, and the only thing at Caption size. The banner
        // is deliberately NOT here: it announces that the file moved under
        // us, which is the least appropriate text in the window to shrink.
        IDC_NOTES => Role::Caption,
        // Everything the user reads or operates: the ListView, the shortcut
        // EDIT, the App and Tap COMBOBOXes, their labels, every BUTTON
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
    /// it is sized for, with the notes strip absorbing the difference until
    /// the next resize. Small on screen; the reason it is guarded rather than
    /// tolerated is that `list_row_height`'s own comment used to justify the
    /// fallback by saying `apply_state` re-lays-out the instant a row appears,
    /// which `shown_external` made false.
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

/// Is this check box ticked? The mirror of `check`, and the only way
/// `handle_command` learns what a click did: `BS_AUTOCHECKBOX` toggles
/// itself before the `BN_CLICKED` arrives, so the control -- not the
/// notification -- is what carries the new state.
///
/// A control that is missing reads as clear. That is the same answer
/// `enabled` gives for the same reason: the alternative is an `Option` every
/// call site would have to collapse to a bool anyway.
fn is_checked(parent: HWND, id: i32) -> bool {
    match unsafe { GetDlgItem(Some(parent), id) } {
        Ok(h) => {
            unsafe { SendMessageW(h, BM_GETCHECK, Some(WPARAM(0)), Some(LPARAM(0))) }.0
                == BST_CHECKED.0 as isize
        }
        Err(_) => false,
    }
}

/// A combo box's selected index, or `None` when nothing is selected.
///
/// The `Tap` combo is read and written through this and never by text.
/// `CB_ERR` is -1, which as an index would be a very large `usize`, so the
/// sign test happens before the cast rather than after it.
fn cur_sel(h: HWND) -> Option<usize> {
    let i = unsafe { SendMessageW(h, CB_GETCURSEL, Some(WPARAM(0)), Some(LPARAM(0))) }.0;
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
    let combo = child(
        hwnd,
        w!("EDIT"),
        "",
        WINDOW_STYLE(ES_AUTOHSCROLL as u32) | WS_BORDER | WS_TABSTOP,
        IDC_COMBO,
        &fonts,
    );
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
    child(
        hwnd,
        w!("BUTTON"),
        cap::HOLD_CTRL,
        WINDOW_STYLE(BS_AUTOCHECKBOX as u32) | WS_TABSTOP,
        IDC_HOLD_CTRL,
        &fonts,
    );
    child(
        hwnd,
        w!("BUTTON"),
        cap::HOLD_WIN,
        WINDOW_STYLE(BS_AUTOCHECKBOX as u32) | WS_TABSTOP,
        IDC_HOLD_WIN,
        &fonts,
    );
    child(
        hwnd,
        w!("BUTTON"),
        cap::HOLD_ALT,
        WINDOW_STYLE(BS_AUTOCHECKBOX as u32) | WS_TABSTOP,
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

/// Seven horizontal bands, top to bottom: the external-change banner (no
/// height when hidden), the section head, the list, the editor strip, the
/// suggestion row (no control, no height, in this landing), the keyboard
/// group and the command bar.
///
/// Everything is placed from the client rect at the current DPI, so a
/// 150 % display is not an afterthought — `GetDpiForWindow` scales the
/// tokens rather than the tokens assuming 96.
///
/// **Vertical shape.** The command bar is anchored to the bottom and the
/// keyboard group sits directly above it; the top bands stack downward.
/// The one thing that flexes is the notes STATIC between them, so a resize
/// lands there. The list wants `header + 8 rows` and gives that up rather
/// than let anything overlap when the window is short — a shrunk list
/// scrolls, an overlapped control is unreachable.
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
    // the gap. It was two lines while the group held a check box over three
    // radios; the Caps row is one line, so the group is one `ctl + gap`
    // shorter and the flexing notes band above it gets those pixels.
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
    // Shortcut field, so the two text boxes narrow together. The HEADING
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
    place(IDC_LBL_SECTION, cx, y, clamp(filter_x - gap - cx), ctl);
    // A control gap, not a band gap: the head labels the list directly
    // below it, so the two read as one group.
    y += ctl + gap;

    // -- Band 3: the list.
    let row_h = list_row_height(ui.list, dpi);
    // `want` is a WINDOW height (it feeds SetWindowPos below), but the list
    // carries WS_BORDER, so its client height -- where header_height + 8
    // rows actually get drawn -- is 2*SM_CYBORDER less than that. Without
    // this the 8th row was clipped by the border and comctl32 drew a sliver
    // of a 9th.
    let border = 2 * GetSystemMetricsForDpi(SM_CYBORDER, dpi);
    let want = list_header_height(ui.list, dpi) + row_h * tok::ROWS + border;
    // The editor strip below needs its own line plus at least one line of
    // notes; the list yields its fixed height before anything overlaps.
    let editor_min = ctl + gap + ctl;
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
    // In that state `y` here can still land past `kb_y`, and unlike the
    // list and the notes STATIC below, the strip's height is the fixed
    // `ctl`, not something `clamp` already shrinks -- so without this it
    // draws over the keyboard group box.
    y = y.min(kb_y);

    // -- Band 4: the editor strip, one line, then the notes beneath it.
    //
    // A single-line EDIT draws its text at the TOP of its client rect --
    // Win32 gives it no vertical centring at all -- so stretching one to
    // the 32 px band line would park the text against the top edge. Neither
    // text field takes the token, then: both are centred within the line,
    // and both take the height the COMBOBOX's theme picked (see `combo_h`,
    // computed above band 2 because the filter box needs it too). `field_h`
    // is what the font alone justifies, and remains the fallback for when
    // the combo cannot be measured -- plus the unit of the dropped-down
    // list's height. The buttons do honour `cy` and look right at 32, so
    // they take the token directly.
    //
    // A hair of slack past the measured width: a STATIC clips to its rect,
    // and SS_CENTERIMAGE clips harder because it also refuses to wrap.
    let lw_app = tw("App") + s(4);
    let lw_short = tw("Shortcut") + s(4);
    // The shortcut field sits under the Shortcut column so the strip
    // mirrors a row. A third of the width is its ceiling on a narrow one.
    let field_w = s(tok::SHORTCUT_COL).min(clamp(cw / 3));
    let edit_x = cx + clamp(cw - field_w);
    let lbl_short_x = (edit_x - lblgap - lw_short).max(cx);
    let app_x = cx + lw_app + lblgap;
    let app_w = clamp(lbl_short_x - gap - app_x);

    place(IDC_LBL_APP, cx, y, lw_app, ctl);
    // A COMBOBOX's `cy` is the height of its DROPPED-DOWN list, not of the
    // closed control -- and under comctl32 v6 even that is capped by
    // `build_children`'s CB_SETMINVISIBLE(8). The closed height is the
    // system's to choose from the font, which is why `combo_h` above asks
    // what it took rather than guessing a chrome delta the next font change
    // would invalidate.
    place_h(ui.app, app_x, y + edit_dy, app_w, field_h * 9);
    // The Shortcut EDIT takes the combo's height, so the fields on this line
    // are ONE box repeated rather than two boxes sharing a midline. Measured
    // at 144 DPI before this: EDIT 43 px against the combo's 36, centres
    // agreeing to within half a pixel -- concentric and unequal, which reads
    // as a mistake rather than a pair.
    place(IDC_LBL_SHORTCUT, lbl_short_x, y, lw_short, ctl);
    place_h(ui.combo, edit_x, y + edit_dy, field_w, edit_h);
    y += ctl + gap;
    place_h(ui.notes, cx, y, cw, clamp(kb_y - band - y));

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
    // `glyph` is the check box's own square plus the gap it leaves before
    // its caption; the two STATICs get a hair of slack instead, for the
    // reason the editor strip's labels do -- SS_CENTERIMAGE clips rather
    // than wraps.
    let glyph = s(24);
    let w_caps = tw(cap::CAPS) + glyph;
    let w_hold = tw(cap::HOLD) + s(4);
    let w_ctrl = tw(cap::HOLD_CTRL) + glyph;
    let w_win = tw(cap::HOLD_WIN) + glyph;
    let w_alt = tw(cap::HOLD_ALT) + glyph;
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
    // and the shortcut field take, so every box in the window narrows
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
                enable(hwnd, IDC_COMBO, st.editable);
                enable(hwnd, IDC_APP, st.editable);
                if text_of(combo) != d.combo {
                    set_text(combo, &d.combo);
                }
                if text_of(app) != d.app {
                    set_text(app, &d.app);
                    wrote_app = true;
                }
                let body: Vec<String> = d
                    .notes
                    .iter()
                    .map(|n| format!("{}  {}", mark_glyph(n.mark), n.text))
                    .collect();
                set_text(notes, &body.join("\r\n"));
            }
            None => {
                enable(hwnd, IDC_COMBO, false);
                enable(hwnd, IDC_APP, false);
                // Conditional, like the `Some` arm above, and for the same
                // two reasons: an unconditional `WM_SETTEXT` raises an
                // `EN_CHANGE` / `CBN_EDITCHANGE` on every push, and clearing
                // a field that is already clear must not invalidate a
                // pending read of it.
                if !text_of(combo).is_empty() {
                    set_text(combo, "");
                }
                if !text_of(app).is_empty() {
                    set_text(app, "");
                    wrote_app = true;
                }
                set_text(notes, "Select a shortcut, or press Add.");
            }
        }

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
        // text write above: `BM_SETCHECK` sets the state without raising
        // `BN_CLICKED`, so a push cannot feed itself back as a user click.
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
    vec![app_cell(it), it.combo.clone()]
}

/// The App column's text: the app name, and the row's flag beside it when
/// it has one.
///
/// **Appended to the cell, not a third column and not `NM_CUSTOMDRAW`.**
/// B.1 draws the flag inline beside the app name, B.2 names exactly two
/// columns, and B.5 is explicit that the Fluent glyphs come later "via
/// `NM_CUSTOMDRAW` as decoration over text that already works". This is
/// that text. It is produced inside the `cells` funnel so the rebuild path
/// and the diff path cannot disagree about it.
///
/// **The flag takes the list's Body font, and cannot take Caption.** B.3
/// puts flags at Caption size, but this text is part of the App CELL, and a
/// ListView draws a cell in the control's one font -- there is no
/// per-run font in a report view. Giving the flag its own would mean
/// `NM_CUSTOMDRAW`, which B.5 explicitly defers to a later pass. So this is
/// a deferral, not an oversight: it lands with the Fluent glyphs or not at
/// all.
///
/// ASCII, like `mark_glyph`, and for the same reason: the face here is a
/// text font, not a symbol one. A healthy row still says nothing at all --
/// `flag` is `None` and the name stands alone, which is the whole point of
/// deleting the status column that used to say `OK` on every row.
fn app_cell(it: &ListItem) -> String {
    match &it.flag {
        Some(f) => format!("{}   {}", it.app, f),
        None => it.app.clone(),
    }
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

unsafe fn check(parent: HWND, id: i32, on: bool) {
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
                if suppressed() {
                    return LRESULT(0);
                }
                let nm = &*(lp.0 as *const NMHDR);
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
            WM_COMMAND => {
                let id = (wp.0 & 0xFFFF) as i32;
                let code = ((wp.0 >> 16) & 0xFFFF) as u32;
                handle_command(hwnd, id, code);
                LRESULT(0)
            }
            WM_CLOSE => {
                let mut may = true;
                with_cb(|cb| may = (cb.on_close_request)());
                if may {
                    let _ = DestroyWindow(hwnd);
                }
                LRESULT(0)
            }
            WM_DESTROY => {
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

/// Push whatever the two edit fields currently show into the model.
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
    let Some((combo, app)) = UI.with(|u| u.borrow().as_ref().map(|x| (x.combo, x.app))) else {
        return;
    };
    let c = text_of(combo);
    let a = text_of(app);
    with_cb(|cb| (cb.on_edit_combo)(c));
    with_cb(|cb| (cb.on_edit_app)(a));
}

fn handle_command(hwnd: HWND, id: i32, code: u32) {
    // The shortcut EDIT and the filter EDIT, and nothing else. The App
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
        (IDC_COMBO, c) if c == EN_CHANGE => {
            if !suppressed() {
                let t = text_of(combo);
                with_cb(|cb| (cb.on_edit_combo)(t));
            }
        }
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
        (IDC_COMBO, c) if c == EN_KILLFOCUS => commit_fields(),
        (IDC_APP, c) if c == CBN_KILLFOCUS || c == CBN_CLOSEUP => commit_fields(),
        (IDC_ADD, _) => with_cb(|cb| (cb.on_add)()),
        (IDC_REMOVE, _) => with_cb(|cb| (cb.on_remove)()),
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
        (IDC_HOLD_CTRL, _) | (IDC_HOLD_WIN, _) | (IDC_HOLD_ALT, _) => {
            // All three read together: the chord is one value, and a setter
            // that took one flag at a time could not refuse "none ticked"
            // without knowing the other two. `BS_AUTOCHECKBOX` toggles
            // itself before the notification arrives, so reading all three
            // back is reading the state the user now sees.
            let c = Chord {
                ctrl: is_checked(hwnd, IDC_HOLD_CTRL),
                super_: is_checked(hwnd, IDC_HOLD_WIN),
                alt: is_checked(hwnd, IDC_HOLD_ALT),
            };
            with_cb(|cb| (cb.on_caps_hold)(c));
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
