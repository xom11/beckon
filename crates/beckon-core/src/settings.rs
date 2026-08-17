//! Settings-window model. Everything the window draws is computed here, so
//! the drawing is a pure function of a snapshot — the same shape
//! `MenuModel`/`build_entries` already use for the tray menu, and for the
//! same reason: it can be tested without a window, a message loop or a
//! registry.

use crate::config_write::{render, RowWrite};
use crate::shortcuts::{parse_config, CapsTap, Chord, Combo, KeyboardConfig};
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    /// The raw key this row was loaded from; `None` for a row the user
    /// added. Passed straight through to `RowWrite` so an untouched row
    /// keeps its original spelling and position in the file.
    pub orig_key: Option<String>,
    pub combo: String,
    pub app: String,
    /// Checked for multi-row delete. UI state only -- never written to
    /// disk (`RowWrite` has no such field) and never makes the model
    /// dirty (see `Model::set_marked`).
    pub marked: bool,
}

#[derive(Debug, Clone)]
pub struct Model {
    pub rows: Vec<Row>,
    pub keyboard: KeyboardConfig,
    pub selected: Option<usize>,
    original: String,
    dirty: bool,
    /// The list filter. **View state**: never written to disk, never makes
    /// the model dirty. It lives here rather than in the window because
    /// `Model::remove_pressed` and `Model::marked_count` act only on the
    /// rows the filter is showing, and `control_state` needs it to compute
    /// `ControlState::items`, `ControlState::selected` (a position within
    /// those items, not a model row) and `ControlState::remove_enabled`.
    /// Those are decisions that belong in the crate all three CI jobs
    /// compile.
    ///
    /// `Model::selected` is deliberately NOT one of them: it stays a model
    /// row whatever the filter says, and `Model::visible` exempts it so the
    /// filter can never hide the row the user is working on.
    filter: String,
}

/// How much a `Problem` costs. `Error` refuses the write; `Warning` is
/// something the user should see but which must not hold the rest of the
/// file hostage -- see `Model::render`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
}

/// One reason this model is not clean. `row` is `None` for a problem with
/// the file as a whole rather than with any single row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Problem {
    pub row: Option<usize>,
    pub severity: Severity,
    pub message: String,
}

/// What `serve` knows that the file does not.
#[derive(Debug, Clone, Default)]
pub struct RuntimeStatus {
    /// Canonical combo -> registration outcome, from the last pass. Keyed
    /// by canonical string rather than by index because `ServeState` holds
    /// shortcuts in `toml::Table` order while the window shows file order,
    /// and aligning two different orderings by position is a bug waiting
    /// to happen.
    pub registered: HashMap<String, Result<(), String>>,
    /// Installed app names. `None` until the catalog scan finishes — which
    /// is NOT the same as "no apps installed", and the UI must not conflate
    /// the two.
    pub catalog: Option<Vec<String>>,
    /// Hotkeys are deliberately unregistered from the tray menu. `serve`
    /// CLEARS `registered` when it pauses, so without this flag every row
    /// would read as "not registered yet" and the window would have nowhere
    /// at all that says beckon is paused.
    pub paused: bool,
    /// The last probe verdict, and the combo it was about. `None` until one
    /// has run -- **not-yet-probed is not the same as free**, the same
    /// distinction `catalog` makes.
    ///
    /// The combo is carried so a verdict for a chord the user has since
    /// changed can be ignored rather than shown against the new one.
    pub probe: Option<ProbeResult>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mark {
    Ok,
    /// Worth saying, not worth stopping for: paused, or a chord Caps cannot
    /// reach.
    Warn,
    Bad,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListItem {
    pub combo: String,
    pub app: String,
    pub mark: Mark,
    /// The short word beside the app name; `None` on a healthy row, which
    /// is the point -- a row that is fine says nothing at all.
    pub flag: Option<String>,
    /// Mirrors `Row::marked` -- the ListView sets this row's check state
    /// from it.
    pub marked: bool,
    /// This item's index in `Model.rows`. **Not** its position in `items`
    /// once a filter is active. Every callback that reaches the model must
    /// go through this: `on_select` and `on_mark` take model indices, and
    /// the ListView only ever knows the view index.
    pub row: usize,
}

/// Every word `row_condition` can put in `ListItem::flag`. The vocabulary is
/// four words and this is the list -- see `split_app_cell`, which is what
/// needs it to be closed.
///
/// **REWORDED 2026-08-15 to design 3.1's four:** `key in use` -> `in use`,
/// `not installed` -> `missing`, `custom` -> `other chord`. `paused` was
/// already right. The precedence did not move -- it is the order the words are
/// claimed in `row_condition`, and that order was already the design's.
///
/// The three replacements are each shorter than what they replace, which is
/// not incidental: the flag rides INSIDE the App cell (`app_cell`), so every
/// character it spends is a character the app name does not get, and
/// `col_app` is 421 px at 96 DPI with long PWA names already in it.
///
/// **REFUTED 2026-08-15: "no word may be a suffix of another".** This entry
/// said that, and cited `in use` beside the word it replaced, `key in use`, as
/// a pair that would make `split_app_cell` hand the painter `key` as part of
/// the app name. **Traced against the function, and it does not**:
/// `split_app_cell` strips the flag AND then requires `FLAG_SEP` in front of
/// what is left, so on `Notepad   key in use` the `in use` arm strips to
/// `Notepad   key `, fails the separator test on a single trailing space, and
/// falls through to the `key in use` arm, which splits correctly. A bare suffix
/// is harmless at every position.
///
/// What is NOT harmless is a word that ends with `FLAG_SEP` followed by another
/// word -- there the separator test passes on the wrong boundary and the
/// earlier entry in this table wins. That requirement is real, and it is
/// checked where it can be checked honestly: by
/// `split_app_cell_inverts_app_cell_for_every_flag`, which runs the actual
/// round trip over the whole vocabulary rather than restating a syntactic rule
/// beside it. The syntactic guard that used to sit here
/// (`no_flag_word_is_a_suffix_of_another`) is deleted -- it rejected a
/// superset it did not need to reject and covered nothing the round trip does
/// not.
pub const FLAGS: [&str; 4] = ["paused", "in use", "missing", "other chord"];

/// What separates an app name from its flag inside one App cell.
///
/// Three spaces rather than a glyph, and it is load-bearing in both
/// directions: `app_cell` joins with it and `split_app_cell` takes it apart
/// again, so the painter can colour the flag without a second source for
/// what the flag IS.
pub const FLAG_SEP: &str = "   ";

/// The App column's text: the app name, and the row's flag beside it when it
/// has one.
///
/// **One cell, not two columns**, because B.2 names exactly two columns and
/// B.1 draws the flag inline. The cell text is also the accessible name, so
/// the flag has to be IN it rather than painted over it -- a screen reader
/// that cannot hear "missing" is worse than a flag that is not coloured.
/// Since 2026-08-15 that is not merely better, it is the only route: three of
/// the four words no longer push a note either, so the cell is the ONLY place
/// they are said at all.
///
/// ASCII, because the face is a text font, not a symbol one. A healthy row
/// says nothing at all -- `flag` is `None` and the name stands alone, which
/// is the whole point of deleting the status column that used to say `OK` on
/// every row.
pub fn app_cell(app: &str, flag: Option<&str>) -> String {
    match flag {
        Some(f) => format!("{app}{FLAG_SEP}{f}"),
        None => app.to_string(),
    }
}

/// How a flag is COLOURED. Not the same question as `Mark`, and that is the
/// whole reason it exists.
///
/// `in use` and `missing` are both `Mark::Bad` -- see `flag_mark` -- so
/// severity cannot tell them apart, while the design deliberately does: a
/// chord another program has taken is red, an app beckon cannot find is
/// amber. Severity answers "how bad"; this answers "which of the four words
/// is it", which is a property of the closed vocabulary rather than a second
/// opinion about how serious anything is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlagTone {
    /// Something else owns the key. Nothing beckon can do from here.
    Bad,
    /// Worth saying, not worth stopping for.
    Warn,
    /// True, and not a problem. No pill at all.
    Neutral,
}

/// The tone for one flag word. Unknown words are `Neutral`, which is the safe
/// direction: a word nobody assigned a colour to must not shout.
pub fn flag_tone(flag: &str) -> FlagTone {
    match flag {
        "in use" => FlagTone::Bad,
        "missing" | "paused" => FlagTone::Warn,
        _ => FlagTone::Neutral,
    }
}

/// How SERIOUS one flag word is, which is what `ListItem::mark` reports.
///
/// **This function is what design 3.1's silent notes cost.** `mark` used to
/// be derived from the notes alone, and the doc for that derivation called it
/// what makes "the list and the editor cannot disagree" true by construction.
/// Three of the four words then lost their note -- a note that only repeated
/// the word is exactly what rule 2 deletes -- and with nothing left to derive
/// from, a `missing` row would have come back `Mark::Ok`. Not a visible
/// regression today, because nothing in either window reads `ListItem::mark`,
/// which is precisely why it would have gone unnoticed until something did.
///
/// The marks below are the ones the deleted notes carried, so the split is
/// behaviour-preserving on `mark` rather than a new opinion:
/// `the_deleted_notes_did_not_take_the_marks_with_them` is what says so.
///
/// **CORRECTED 2026-08-15: "behaviour-preserving on `mark`" was false for one
/// combination, and this function was only half the repair.** A row that is
/// PAUSED and whose app is MISSING used to carry both deleted notes -- `Warn`
/// for the pause, `Bad` for the app -- and came out `Mark::Bad`. Feeding the
/// winning FLAG in here instead reports `flag_mark("paused")`, i.e. `Warn`,
/// because `paused` outranks `missing` in the cell and the loser stopped being
/// visible to the severity fold at all. The four words are a precedence for the
/// CELL; they are not a claim that a lower-precedence problem went away. The
/// fix is in `row_condition`, which now collects EVERY condition it finds and
/// folds this function over all of them -- see the `conditions` vector there --
/// and `a_paused_row_whose_app_is_missing_is_still_bad` is what fails on the
/// half-repair.
///
/// **Takes one word, not an `Option`, since that fix.** There is no longer a
/// "the row's flag" to hand it: there is a list of conditions, each of which
/// has a severity, and the empty list means nothing to fold rather than a word
/// meaning `Ok`.
///
/// **Not derivable from `FlagTone`, and the two must not be merged.** They
/// disagree on `missing` (`Bad` here, `Warn` there) and on `other chord`
/// (`Warn` here, `Neutral` there), because one answers "how bad is this row"
/// and the other answers "which of the four words is this" -- see `FlagTone`.
/// A row on a chord Caps cannot reach is worth a `Warn` and worth no colour.
fn flag_mark(flag: &str) -> Mark {
    match flag {
        "in use" | "missing" => Mark::Bad,
        "paused" | "other chord" => Mark::Warn,
        // A word nobody assigned a severity to must not raise one, the same
        // safe direction `flag_tone` takes for colour.
        _ => Mark::Ok,
    }
}

/// Take an App cell back apart into `(name, flag)`.
///
/// **The inverse of `app_cell`, and it exists for the painter.** Colouring
/// the flag means knowing where it starts, and the only thing a
/// `NM_CUSTOMDRAW` handler can read without touching the window's own state
/// is the cell's text -- so the split has to be recoverable from the string
/// alone.
///
/// **Matched against `FLAGS`, never on the separator alone.** Splitting at
/// the last run of spaces would make any app whose name ends in three spaces
/// grow a flag, and would silently colour whatever followed. Testing the
/// suffix against the closed vocabulary means the worst case is an app
/// genuinely named `...   missing`, which is not a name.
///
/// **The separator test is the second half and is what makes the vocabulary
/// safe against itself**, not a rule about suffixes: a word that is a bare
/// suffix of another (`in use` inside `key in use`) fails it on the space that
/// precedes it and falls through to the longer word, so the two can coexist.
/// See `FLAGS`, which used to claim the opposite.
///
/// **CORRECTED 2026-08-15: this comment was attached to the wrong item.** It
/// sat above `FlagTone` with no blank line between the two blocks, so
/// rustdoc read the whole run as `FlagTone`'s -- an enum about colour
/// documented as "Take an App cell back apart" -- and this function had no
/// doc at all. Only the placement moved; the words are unchanged apart from
/// the flag rename and the sentence above.
pub fn split_app_cell(cell: &str) -> (&str, Option<&str>) {
    for f in FLAGS {
        if let Some(rest) = cell.strip_suffix(f) {
            if let Some(name) = rest.strip_suffix(FLAG_SEP) {
                return (name, Some(f));
            }
        }
    }
    (cell, None)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Note {
    pub mark: Mark,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Detail {
    pub combo: String,
    pub app: String,
    pub notes: Vec<Note>,
}

/// Exactly what the window draws. The window never reads `Model`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ControlState {
    pub items: Vec<ListItem>,
    /// How many bindings the FILE has, which is not how many `items` has.
    ///
    /// **The two are different on purpose and the difference is the reason
    /// this field exists.** `items` is built from `Model::visible`, which is
    /// filter-dependent and additionally exempts the selected row from the
    /// filter (see `Model::visible`) -- so under a filter it is a count of
    /// what is on screen. The Shortcuts pill's count badge is read from three
    /// pages that have no filter box at all, where "the rows matching a filter
    /// the user cannot see" is not a number that means anything.
    ///
    /// **There is now exactly one count on screen, and this is it.** A second
    /// STATIC (`LBL_COUNT`, id 1035, retired) used to say `· 18 bindings`
    /// beside the Shortcuts heading from `items.len()`, so the window showed
    /// the same number twice and the two could differ under a filter while
    /// both were right. Design 2 moved the count to the pill precisely to end
    /// that; the heading's copy went on 2026-08-15.
    ///
    /// A row added and not yet saved counts. The badge follows the model
    /// rather than the disk for the same reason the title bar's `*` does:
    /// what the window is showing IS the pending file.
    pub binding_count: usize,
    /// Which row is current, as an index into `items`. `detail` already
    /// says *whether* there is one, but the ListView needs to know *which*
    /// to put `LVIS_SELECTED` back on after it rebuilds -- and it must not
    /// reuse the highlight it had before, because `add_row` and
    /// `remove_row` both move the selection without the window hearing
    /// about it.
    ///
    /// `None` only when `Model::selected` is `None`. A filter cannot make it
    /// `None`, because `Model::visible` exempts the selected row from the
    /// filter -- see the reasoning there; it used to be able to, and that is
    /// exactly what disabled a focused field mid-edit.
    pub selected: Option<usize>,
    pub detail: Option<Detail>,
    /// What the filter box should show. Pushed so `Add` can clear it; the
    /// window writes it back ONLY when it differs from what the control
    /// already holds, because an unconditional `WM_SETTEXT` raises
    /// `EN_CHANGE` on every push and would fight the user's typing.
    pub filter: String,
    pub caps_checked: bool,
    pub caps_tap: CapsTap,
    /// What holding Caps Lock stands for. The window shows it as three
    /// chips; `Chord` has no `shift` field and must not grow one -- see its
    /// own doc for why.
    pub caps_hold: Chord,
    /// Are there unsaved edits? **Not the same question as
    /// `apply_enabled`**, which is `dirty && no errors` -- an invalid model
    /// is still dirty, and the title bar's `*` has to say so even while
    /// Save is greyed out. It rides on every push because the title has to
    /// follow every keystroke; the config PATH does not, because it cannot
    /// change while the window is open.
    pub dirty: bool,
    pub apply_enabled: bool,
    pub remove_enabled: bool,
    /// How many rows are ticked.
    ///
    /// **The button's caption stays the constant `Remove`**, and this field
    /// does not feed it. The plan asked for `Remove N`, but `layout` sizes
    /// every button from `text_size` of its own caption, so a caption that
    /// grows with the tick count is one more input to `layout` -- and calling
    /// `layout` on a data push is what `SetWindowPos`es the populated App
    /// combo and throws away what the user typed. That is the measured
    /// data-loss bug `Ui::shown_external` exists to prevent. This is not the
    /// only route to a live count: reserving width for the widest caption at
    /// `layout` time and driving the count with `SetWindowTextW` alone on
    /// pushes would honour it without ever calling `layout` or moving the
    /// combo. That route is open, just not taken here -- a live count is
    /// cosmetic and not worth the hardware time this pass has left.
    ///
    /// What the count IS for: `remove_enabled`, so a window whose selection
    /// sits elsewhere still offers Remove for the ticked rows.
    pub marked_count: usize,
    /// May the user change the file through this window at all?
    ///
    /// `true` for every state projected from a `Model`, and `false` for the
    /// one state that has no `Model` to project from: a config file that did
    /// not parse (`unreadable_state`). beckon opens that file rather than
    /// refusing -- the moment someone who has never seen TOML most needs a
    /// GUI is the moment their file is broken -- but it will not write over
    /// something it cannot read, so every control that mutates is off.
    ///
    /// **One field, ANDed at the window's `enable` call sites, rather than a
    /// branch there.** The window is not allowed to know *why* it is read
    /// only, or that "the file did not parse" is even a state; it knows only
    /// that this flag is off, exactly as it knows `apply_enabled` without
    /// knowing what makes Save legal.
    pub editable: bool,
    /// The command bar's live service line (design §6.4), on all four doors.
    ///
    /// **Here rather than in a push of its own**, which settles design §12's
    /// open question 4 in the direction that question points: every input
    /// `service_line` needs is already in this projection's two arguments, so
    /// a fourth push would be a second route to state this one already
    /// carries — and it would have to be `cfg`-gated per platform, where this
    /// is free on all three.
    pub service: ServiceLine,
}

/// Which door the window is showing.
///
/// In core, not in the Windows crate, so `DefaultButton::visible(external,
/// page)` stays testable on all three CI jobs -- which is the stated reason
/// `DefaultButton` is in core at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Page {
    #[default]
    Shortcuts,
    Keyboard,
    System,
    About,
}

impl Page {
    /// The door to the right, wrapping past the last one.
    ///
    /// `Ctrl+Tab`'s answer, and it is here rather than in the window for the
    /// same reason the rest of `Page` is: the two CI jobs that never compile a
    /// wndproc can check it. The window's own spelling of strip order is
    /// `TABS`, and `the_strip_order_is_the_cycle` pins the two together --
    /// which is the only thing that can, since a table and a cycle are the
    /// same fact written twice.
    ///
    /// **An exhaustive `match` rather than an index into a list**, matching
    /// `tab_id_of`'s decision for the same reason: a fifth `Page` is then a
    /// compile error in this file instead of a silent wrap at the one call
    /// that decides which door `Ctrl+Tab` opens. The cost is that the cycle is
    /// spelled twice, forwards and backwards, and `next_and_prev_are_inverses`
    /// is what stops those two drifting.
    ///
    /// **It wraps.** Four doors and a key that means "the next one" have
    /// nowhere else to go, and a `Ctrl+Tab` that stops dead on `About` reads
    /// as a broken key rather than as an edge.
    pub fn next(self) -> Page {
        match self {
            Page::Shortcuts => Page::Keyboard,
            Page::Keyboard => Page::System,
            Page::System => Page::About,
            Page::About => Page::Shortcuts,
        }
    }

    /// The door to the left, wrapping past the first one -- `Ctrl+Shift+Tab`.
    pub fn prev(self) -> Page {
        match self {
            Page::Shortcuts => Page::About,
            Page::Keyboard => Page::Shortcuts,
            Page::System => Page::Keyboard,
            Page::About => Page::System,
        }
    }

    /// Does this door write `apps.toml`?
    ///
    /// **Design §1's split by STORE, as a function rather than as prose.** It
    /// read "Shortcuts and Keyboard write `apps.toml`; System and About write
    /// `HKCU\Software\beckon`, the Run key, or nothing" and lived only in the
    /// design document and in one comment inside `pressable` -- so the window
    /// drew a `Save` button under all four doors, including the two that have
    /// nothing to save. `Ctrl+S` wrote the file from them too.
    ///
    /// **An exhaustive `match`, on `next`/`prev`'s reasoning**: a fifth door is
    /// then a compile error in this file rather than a silent `false` at the
    /// one call that decides whether a page can save.
    ///
    /// Note which side `Keyboard` is on and why it is not obvious: its three
    /// controls set `keyboard.caps`, `keyboard.caps_hold` and
    /// `keyboard.caps_tap`, which are keys in `apps.toml` -- so it saves,
    /// even though nothing on it is a shortcut.
    pub fn writes_config(self) -> bool {
        match self {
            Page::Shortcuts | Page::Keyboard => true,
            Page::System | Page::About => false,
        }
    }
}

/// The live service line at the left end of the command bar (design §6.4).
///
/// **On all four doors**, which is the point: since the store split the bar
/// draws no buttons on System or About, so that band is empty ground on half
/// the window. This is what §6.4 fills it with, and it is the only thing on
/// screen that says whether the hotkeys are actually working.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceLine {
    /// Drawn as a GDI `Ellipse`, never as a character. Design §6.4 is
    /// explicit: an em-dash in `serve --log` already came back as `?"` once,
    /// and a text face draws a missing glyph as a box.
    pub mark: Mark,
    pub text: String,
}

/// What the service line says.
///
/// Four inputs, all of which `control_state` already holds, which is why this
/// needs no push of its own — see design §12 q4, which phrases the data as
/// "the new `ControlState` fields" and is settled that way here.
///
/// **The denominator is the MODEL's row count, not the registration map's.**
/// The Shortcuts pill's badge is `binding_count` (`m.rows.len()`), and the
/// drawing puts `Shortcuts 19` and `Serving · 18 of 19` on screen together —
/// so the two 19s have to be the same 19. `registered` is the last
/// registration PASS, so a row added in the window but not yet saved makes it
/// the smaller number: `Serving · 19 of 20` is then true and legible, where a
/// denominator taken from the map would read `19 of 19` beside a badge saying
/// 20 and neither number would explain the other.
///
/// **The numerator has to be counted over those same rows, and the paragraph
/// above only reasons about one direction.** The other one is worse: a row
/// REMOVED in the window is still in the last registration PASS, so a
/// numerator taken from the map's own values outran the total and printed
/// `Serving · 3 of 2` -- in amber, on the one line that says whether the
/// hotkeys are working. `control_state` therefore counts the rows the model
/// still has whose chord the last pass registered.
///
/// **The mark is a function of the text**, and that is fine but is not a
/// reason for the window to cache it: `WM_DRAWITEM` cannot re-derive a `Mark`
/// from a window's text, so the painter needs it pushed alongside.
pub fn service_line(
    editable: bool,
    paused: bool,
    registered_ok: usize,
    total: usize,
) -> ServiceLine {
    // Order matters and is the status vocabulary's own precedence, one
    // surface up: a file that did not parse outranks a pause, because pausing
    // a service that was never serving is not the fact worth printing.
    if !editable {
        return ServiceLine {
            mark: Mark::Bad,
            text: "Not serving — the config did not parse".to_string(),
        };
    }
    if paused {
        return ServiceLine {
            mark: Mark::Warn,
            text: "Paused".to_string(),
        };
    }
    ServiceLine {
        // `Warn`, not `Bad`, when some chord did not take: the ones that did
        // are still working, which is the same reading `row_condition` gives
        // `in use` on a single row.
        mark: if registered_ok == total {
            Mark::Ok
        } else {
            Mark::Warn
        },
        text: format!("Serving · {registered_ok} of {total}"),
    }
}

/// Which chord the Shortcuts list folds into a single `Caps` cap, if any.
///
/// Design §3.2's `Write shortcuts as [Caps] instead of [Ctrl][Win][Alt]`, as
/// one decision with two inputs, in core so both are tested on all three CI
/// jobs rather than on the one that can run a ListView.
///
/// `want` is the view preference (`HKCU\Software\beckon\CapsView`, default
/// OFF). `caps_on` is `keyboard.caps` from `apps.toml` — whether the hook is
/// armed at all.
///
/// **The AND is the decision the design did not make, and it is taken here
/// rather than left to the painter.** §3.2 says the preference is a view
/// preference and says nothing about the Caps feature being off, which is a
/// state a user can reach in one click: the switch that arms Caps is the row
/// directly above this one. With Caps off, `Caps+B` does nothing at all, so a
/// list drawing `[Caps][B]` would be advertising a keystroke that is not
/// bound — the window lying about the machine, which is the one thing the
/// status vocabulary exists to prevent. The chips stay long instead.
///
/// **The toggle is disabled rather than silently ineffective** when
/// `caps_on` is false — see `caps_view_enabled`. §7 rule 7 wants a disabled
/// control to explain itself in its own slot, and here the explanation is
/// adjacency: the control immediately above it is the one that is off.
pub fn caps_view_fold(want: bool, caps_on: bool, hold: Chord) -> Option<Chord> {
    (want && caps_on).then_some(hold)
}

/// Is the `Write shortcuts as [Caps]` switch usable?
///
/// Split from `caps_view_fold` because the two answer different questions and
/// a caller that conflated them would grey the switch whenever the fold was
/// off — including when the fold is off precisely because the user turned
/// this switch off, which would make it impossible to turn back on.
pub fn caps_view_enabled(caps_on: bool) -> bool {
    caps_on
}

/// Is the command bar's row of buttons on screen?
///
/// **One condition, three readers**, in `banner_shown`'s shape and for the same
/// reason: `DefaultButton::visible` decides whether the ring may rest there,
/// `show_page_controls` decides whether the three windows are shown, and
/// `handle_command`'s `IDC_APPLY` arm decides whether `Ctrl+S` writes. Three
/// spellings would drift, and the drift that matters is the last: an
/// accelerator that saves from a door showing no Save is a file written with
/// nothing on screen having offered to write it.
///
/// **The BAND is not what this hides.** `compute_card_rects` reserves
/// `pad + ctl` at the bottom of every page whatever this answers, so the stop
/// every card shares stays one expression on all four doors. This is the
/// BUTTON ROW, not the band.
///
/// **And the band is no longer empty where the buttons are gone.** This
/// comment used to end "an empty bar is indistinguishable from the window
/// ground beside it", which was the honest reading for one day and stopped
/// being true on 2026-08-16: design §6.4's service line (`IDC_SERVICE_LINE`)
/// is chrome, drawn on all four doors, and on System and About it has the
/// whole bar to itself. What §6.4 still owes is the band's right half -- the
/// `Saved` readout and `Undo` -- both of which belong to §6's auto-save.
pub fn command_bar_shown(page: Page) -> bool {
    page.writes_config()
}

/// Is the external-change banner on screen?
///
/// **One condition written once, for five readers.** The banner is a STATIC
/// and two push buttons, and the three are placed by `layout`, shown by
/// `apply_state` and by `show_page_controls`, given a card rect by
/// `compute_card_rects` and reasoned about by `DefaultButton::visible` -- so
/// "is the announcement up" had five places to be spelled and five chances to
/// be spelled differently. The one that mattered is the last: `visible`
/// deciding the banner's Reload is on screen while the window has hidden it is
/// exactly the measured defect `default_button` exists for, reached through a
/// door instead of through a dismissal.
///
/// **The history matters because the condition has been here before and did
/// not survive.** Task 4 shipped this exact body and it had to be widened the
/// same day. Save is chrome: on all four
/// pages, enabled from `apply_enabled` alone, the resting place of the default
/// ring and the target of `Ctrl+S`. `apply_settings` writes unconditionally and
/// there is no prompt anywhere, so **the banner being on screen WAS the whole
/// protection**, and three quarters of the window had lost it. Edit a binding,
/// open the Keyboard door, let an editor save the config underneath, press
/// `Ctrl+S`, and the file was overwritten with nothing having said it moved.
///
/// **REPLACED 2026-08-14.** The first repair (`aa9fbd6`, `save_press`) had Save
/// refuse the press once and switch the window to `BANNER_PAGE`. It worked, and
/// it cost a THIRD route into `show_page` -- one that moves no focus and changes
/// card geometry, which is precisely the shape `repair_hidden_button` and
/// `combo_needs_placing` exist to survive. Showing the banner on every page
/// removes a page-switch route instead of adding one, and it answers the failure
/// directly: what went wrong was that the warning was invisible while Save
/// stayed reachable, so the fix is a warning the user can see from where they
/// are standing.
///
/// **NARROWED BACK 2026-08-14, Task 6, and the dot is what paid for it.** The
/// paragraph above stood while this function ignored `page` and answered
/// `external_change` on every door. That was the wide version, and it was
/// explicitly a holding position: the announcement costs Keyboard, System and
/// About a band of height each, in a state none of them is about. It could only
/// narrow once something else carried the fact to those three doors, because
/// what it was protecting against is Save being pressable where the warning is
/// not. `warn_dot_shown` is that something, `paint::tab_pill` draws it, and the
/// two now partition `external_change` between them -- pinned by
/// `the_warning_is_on_screen_from_every_door`, which is the assertion that
/// makes this narrowing safe rather than the comment claiming it does.
///
/// The protection is weaker in one honest respect and stronger in another. A
/// dot says less than a sentence and two buttons; but it says it from all four
/// doors at once, whereas the wide banner said it four times over and the
/// design (§2) rejected that. Save stays pressable everywhere either way --
/// `apply_settings` has no external-change guard and deliberately does not want
/// one; see the `REVERTED 2026-08-14` note there.
pub fn banner_shown(external_change: bool, page: Page) -> bool {
    external_change && page == BANNER_PAGE
}

/// Does the Shortcuts pill carry its warn dot?
///
/// **The exact complement of `banner_shown` within `external_change`**, and
/// written as that complement rather than as `page != BANNER_PAGE` so the two
/// cannot drift into a state where the file has moved and nothing anywhere says
/// so. That partition is what let `banner_shown` narrow at all, so it is the
/// invariant `the_warning_is_on_screen_from_every_door` asserts directly.
///
/// It also means the dot is never drawn on a pill that is lit: the door the
/// announcement is about is the door whose pill would carry it, and on that
/// door the announcement itself is on screen. The painter therefore never has
/// to put `warn` ink on `accent_fill` -- which is just as well, because it
/// measures 1.212 in Light and no row in `theme::pairs` covers it.
pub fn warn_dot_shown(external_change: bool, page: Page) -> bool {
    external_change && !banner_shown(external_change, page)
}

/// The door the external-change announcement is ABOUT, and now also where it
/// is drawn.
///
/// The file that moved is the shortcut table, so it is Shortcuts' pill that
/// grows the warn dot on the other three doors.
pub const BANNER_PAGE: Page = Page::Shortcuts;

/// Where the App combo sits, in the window's client coordinates: left, top
/// and WIDTH.
///
/// **There is no height, and its absence is the design.** `GetWindowRect` on
/// a closed `CBS_DROPDOWN` reports the CLOSED height, while `layout` asks for
/// the DROPPED one (`field_h * 9`) -- about nine times larger. The two can
/// never agree, so a height carried here could only ever force a placement
/// that is not needed. It is also the component comctl32 v6 ignores outright:
/// `CB_SETMINVISIBLE(8)`, sent once at creation, is what decides how tall the
/// list opens, and the `cy` argument stopped deciding it (`build_children`,
/// in the Windows crate, records that).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ComboSpot {
    pub x: i32,
    pub y: i32,
    pub cx: i32,
}

/// Must `layout` call `SetWindowPos` on the App combo this pass?
///
/// **The one call in this window that can destroy data.** A populated
/// `CBS_DROPDOWN` answers a resize by re-synchronising its edit field to the
/// closest matching catalogue entry and selecting the whole string, so the
/// next keystroke replaces what the user typed -- measured on a14, comctl32
/// 6.16, 121 items. The four-doors landing stopped `layout` placing the combo
/// while another door is open. That guard is ONE-DIRECTIONAL: every switch
/// back INTO Shortcuts still ran the placement, on a combo that had not
/// moved a pixel.
///
/// **And that return trip really is a resize, every time.** `layout` passes
/// `field_h * 9` as `cy` while the combo's window rect holds its closed
/// height, so the size the OS is asked for never equals the size the OS
/// currently reports and nothing upstream can elide the call. The `cy` is
/// deliberate and is not what to change -- a combo's height argument sizes
/// its dropped list, not its closed box. What changes is that the call does
/// not happen.
///
/// **This does not depend on spec §10 open question 1** -- whether
/// `SetWindowPos` with an unchanged rect re-syncs a populated combo anyway.
/// That answer would decide whether an `SWP_NOSIZE | SWP_NOMOVE`
/// short-circuit inside the call is safe. Not making the call is safe under
/// either answer, which is why the fix is spelled this way round.
///
/// **What it does NOT do, and what does:** it answers only "is this placement
/// unnecessary". Some are necessary -- `layout` skips the combo from three of
/// the four doors, so any of its inputs that moves while one of those is open
/// leaves the combo genuinely stale, and the trip back genuinely has to place
/// it. Returning `true` there is correct, and the placement is still the
/// measured data-loss call. `settings_window::place_app_combo` (Windows crate)
/// is the guard on that side: it saves the edit's text and selection across the
/// `SetWindowPos`. Neither guard subsumes the other, and a reading of this
/// function as "the combo is now safe" is the reading to avoid.
///
/// `seen` is where the OS says the control is now, and `None` means the
/// question could not be asked -- which places, never skips.
///
/// **`seen` being the OS's own answer is what makes this safe across window
/// lifetimes.** A remembered rect would not be: the settings window opens and
/// closes many times on one thread and Windows recycles handles, so a memory
/// could authorise a skip for a control that no longer exists. Asking the
/// control cannot: a freshly created child sits at `CreateWindowExW`'s
/// `0, 0, 10, 10` (`child`, in the Windows crate), which no computed
/// placement matches, so the first pass after any reopen places the combo.
pub fn combo_needs_placing(want: ComboSpot, seen: Option<ComboSpot>) -> bool {
    seen != Some(want)
}

/// Where the two files this window talks about live.
///
/// `log` is `None` when `serve` was started without `--log`. The System page
/// omits the row rather than showing a path that does not exist -- the same
/// reasoning the tray menu uses for `Start with Windows` under
/// `beckon.exe serve`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Paths {
    pub config: std::path::PathBuf,
    pub log: Option<std::path::PathBuf>,
}

/// A file or a URL the window can ask the caller to open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    Config,
    Log,
    Github,
    Releases,
    BugReport,
}

impl Target {
    /// The address `Open` sends a browser to, or `None` for the two targets
    /// that are files on this machine.
    ///
    /// **The three URLs live here, not in the window and not in `serve`**, for
    /// the reason every other decision in this module does: they are checkable
    /// on all three CI jobs, and the alternative is three string literals in a
    /// wndproc that nothing has ever read back. A link that opens the wrong
    /// page is worse than one that is not built -- so the strings are somewhere
    /// a test can look at them.
    ///
    /// **`Option`, not a fallback URL**, because `Config` and `Log` are paths
    /// and a "default" address for them would be a lie with a `https://` in
    /// front of it. `open_target` in `serve.rs` matches on the `None` to pick
    /// the file route, so the two answers stay one decision.
    ///
    /// `issues/new` rather than `issues`: the button says `Report a bug`, and
    /// landing on the list of everyone else's is a different verb.
    pub fn url(self) -> Option<&'static str> {
        match self {
            Target::Config | Target::Log => None,
            Target::Github => Some("https://github.com/xom11/beckon"),
            Target::Releases => Some("https://github.com/xom11/beckon/releases"),
            Target::BugReport => Some("https://github.com/xom11/beckon/issues/new"),
        }
    }
}

/// A row on About whose value can be copied.
///
/// The three rows are the three copy buttons; `copy_text` says what each one
/// puts on the clipboard, and it is deliberately **not** the string the row
/// shows -- see `AboutValue`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    Build,
    Location,
    Licence,
}

/// Everything the window can ask the caller to DO that is not an edit to a
/// binding.
///
/// **One `Callbacks` field rather than eleven.**
/// `beckon-macos/examples/settings_probe.rs` builds `Callbacks` as a complete
/// literal with no `..`, and CI clippies it `--all-targets` on macos-latest --
/// so every added field is a hard E0063 on a job that has nothing to do with
/// the feature. That is a real cost paid by a real job, not a hypothetical.
///
/// `Copy + Eq` and no variant carries a `String`, so a caller can match, log
/// and test one without cloning.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsCommand {
    /// The user moved to another door. The caller stores it, so the next
    /// open lands where they left off.
    ShowPage(Page),
    SetPaused(bool),
    SetAutostart(bool),
    /// The System page's Reload -- the tray's own, NOT the banner's
    /// "reload from disk", which is `on_reload_from_disk` and answers a
    /// different question.
    ReloadNow,
    SetDarkMode(bool),
    /// 85..=100. The window clamps before sending; the caller may assume it.
    SetOpacity(u8),
    SetCapsShorthand(bool),
    Open(Target),
    Reveal(Target),
    Copy(Field),
    Undo,
}

// ---------------------------------------------------------------------------
// The System page
// ---------------------------------------------------------------------------

/// The narrowest the transparency slider goes, as a percentage.
///
/// **85, not 0, and the floor is a measurement rather than a preference.**
/// `SetLayeredWindowAttributes` dims without blurring -- Mica and Acrylic
/// blur, and Mica is measured dead on this window (gate 01, a14) -- so every
/// step of visible transparency is a step of legible clutter with nothing
/// gained. 91 % was tried on a real desktop and rejected in those words
/// ("trong suot qua da, va khong co lam mo nen rat kho nhin"), which is where
/// the floor comes from: the slider offers the band either side of "barely
/// there", not the band that includes "unreadable". See
/// `theme::TIER2_ALPHA`'s own note, which is the same finding from the other
/// end.
pub const OPACITY_MIN: u8 = 85;

/// Fully opaque, and the top of the range: a slider whose top end is not
/// "off" would leave no way to turn the effect off from the control that
/// turns it on.
pub const OPACITY_MAX: u8 = 100;

/// Where the slider sits until the user moves it.
///
/// A hint of depth at the window's edges and nothing more, which is design
/// §5.1's whole claim for tier 2. It is deliberately NOT `theme::TIER2_ALPHA`
/// (250, i.e. 98 %) converted: that constant is the fixed alpha for a window
/// with no slider, and the two answer different questions -- one is what
/// beckon picks when nobody is asked, the other is where the asking starts.
pub const OPACITY_DEFAULT: u8 = 96;

/// Bring a stored or typed percentage into range.
///
/// The window clamps before sending `SettingsCommand::SetOpacity`, so the
/// caller may assume 85..=100 -- but the value also arrives from
/// `HKCU\Software\beckon`, which anything can write, so the read path clamps
/// too. Two callers, one function: a value that reached the alpha
/// calculation unclamped would produce a window the user cannot see through
/// a slider that cannot reach it.
pub fn clamp_opacity(v: u8) -> u8 {
    v.clamp(OPACITY_MIN, OPACITY_MAX)
}

/// A percentage as `SetLayeredWindowAttributes`' 0..=255 alpha.
///
/// Rounded rather than truncated: 96 % truncates to 244 and rounds to 245,
/// and the whole visible range is 218..=255, so a systematic one-step bias
/// across sixteen positions is worth the `+ 50`.
///
/// 100 % comes back as exactly 255, which is what makes the top of the
/// slider genuinely opaque rather than nearly so.
pub fn opacity_alpha(percent: u8) -> u8 {
    let p = clamp_opacity(percent) as u32;
    ((p * 255 + 50) / 100) as u8
}

/// What the slider's slot reads while it is live: `96%`.
///
/// No space before the sign, matching the mock-up.
pub fn opacity_label(percent: u8) -> String {
    format!("{}%", clamp_opacity(percent))
}

/// The transparency row's state: a live slider, or the reason there is none.
///
/// **Two states rather than an `enabled: bool` beside a percentage**, because
/// the row draws one thing or the other in the same slot and a pair would let
/// the two disagree -- a greyed slider still showing `96%` says the window is
/// 96 % opaque, which under high contrast it is not.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transparency {
    /// The slider is live at this percentage, within `OPACITY_MIN..=OPACITY_MAX`.
    On(u8),
    /// Forced off by the OS. The window disables the slider and puts
    /// `TransparencyBlock::reason()` in the value slot beside it.
    Off(crate::theme::TransparencyBlock),
}

impl Transparency {
    /// Resolve the row from the machine's own answer and the stored
    /// preference.
    ///
    /// **The block is asked for, not re-derived.** The caller passes what
    /// `theme::transparency_block` said, which is the same predicate
    /// `theme::backdrop` uses to pick the window's tier -- so a slider that
    /// is live and a window that is transparent are the same fact rather
    /// than two facts that agree today.
    pub fn resolve(block: Option<crate::theme::TransparencyBlock>, percent: u8) -> Transparency {
        match block {
            Some(b) => Transparency::Off(b),
            None => Transparency::On(clamp_opacity(percent)),
        }
    }

    /// What goes in the row's value slot -- a percentage, or the reason.
    ///
    /// **One slot, never a sub-line and never a tooltip.** Design §7 rule 7:
    /// a disabled Win32 control receives no mouse messages, so a tooltip on
    /// one silently never appears. The slot is on the same line as the label
    /// either way, so the row's height does not move when the reason
    /// replaces the number.
    pub fn slot(self) -> String {
        match self {
            Transparency::On(p) => opacity_label(p),
            Transparency::Off(b) => b.reason().to_string(),
        }
    }

    /// Is the slider operable?
    pub fn enabled(self) -> bool {
        matches!(self, Transparency::On(_))
    }
}

/// One of the System page's two file rows: the file's own name, and the one
/// fact about it worth a value slot.
///
/// **The name IS the label**, which is why there is no `Config` / `Log`
/// caption anywhere -- design §3.3, and it is rule 1 applied to a row: a
/// label that says "Config" beside a control that says `apps.windows.toml`
/// has told the reader nothing the filename did not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileRow {
    pub name: String,
    pub value: String,
}

/// A file size for the log row's value slot.
///
/// **The log's size is worth showing and the config's is not**, which is why
/// only one of the two rows carries this: `roll_if_oversized` caps the log
/// pair at 10 MiB and rolls at 5, so the number is on its way somewhere. A
/// shortcuts TOML is a few hundred bytes for ever.
///
/// `KB`/`MB` in the Windows sense (multiples of 1024), matching what Explorer
/// shows for the same file -- a number that disagreed with the one beside it
/// in Explorer would read as a beckon bug rather than as a units convention.
pub fn size_label(bytes: u64) -> String {
    const KB: u64 = 1024;
    if bytes < KB {
        // Never "0 KB" for a file that has something in it: a log with one
        // line is the ordinary state seconds after `serve` starts.
        return format!("{bytes} bytes");
    }
    // **Round first, then pick the unit.** The test used to be `bytes < MB`,
    // made on the RAW count, while the rounding that follows it can carry into
    // 1024 -- so every size from 1_048_064 up printed `1024 KB`, a unit
    // Explorer never shows and the doc above promises to match. The MB arm
    // reads the same rounded figure for the same reason: taken from `bytes` it
    // would answer `0.9 MB` for a file the KB arm had just called 1024.
    let kb = (bytes + KB / 2) / KB;
    if kb < KB {
        return format!("{kb} KB");
    }
    format!("{}.{} MB", kb / KB, (kb % KB) * 10 / KB)
}

/// Everything the System page draws that is not in `ControlState`.
///
/// **A second push, not more fields on `ControlState`, and that is design §1's
/// split by STORE rather than a convenience.** Shortcuts and Keyboard write
/// `apps.toml`; System writes `HKCU\Software\beckon`, the Run key, or
/// nothing. `ControlState` is the projection of a `Model`, and a `Model` is
/// what a config file that does not parse fails to produce -- which is
/// exactly the state (`unreadable_state`) in which the theme switch and
/// `Start with Windows` must still work, because neither has anything to do
/// with that file. Riding on `ControlState` would have made the whole System
/// page hostage to a TOML error, which is the defect design §1 names as
/// fixed "as a side effect".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SystemState {
    /// Hotkeys are unregistered. The same flag `RuntimeStatus::paused`
    /// carries, from the same `ServeState` field -- the switch and the
    /// `paused` status word on every Shortcuts row are one fact.
    pub paused: bool,
    /// `None` OMITS the row -- this process cannot offer autostart at all.
    /// `Some(on)` shows it, ticked per `on`.
    ///
    /// **Omitted, never greyed**, and the reasoning is copied from the tray
    /// menu rather than re-derived: a greyed row asks "why is this greyed?"
    /// with no answer available in the row itself, so a capability this
    /// process does not have is left out. `beckon.exe serve` is the case --
    /// a Run value pointing at a console-subsystem binary would open a
    /// console window at every logon, so the capability is absent by
    /// construction and `MenuModel::autostart` is already `None` there.
    pub autostart: Option<bool>,
    /// The window's own theme switch. Two states, and design §3.3 is explicit
    /// that this is a behaviour change: beckon followed Windows and now does
    /// not. "Follow system" would have to be a third state, which the design
    /// rejects.
    pub dark: bool,
    pub transparency: Transparency,
    pub config: FileRow,
    /// `None` OMITS the row: `serve` ran without `--log`, so there is no log
    /// file and a path would be a lie. Same reasoning as `autostart`, and the
    /// same one `Paths::log` already carries.
    pub log: Option<FileRow>,
}

/// What the caller knows that `system_state` turns into a page.
#[derive(Debug, Clone, Copy)]
pub struct SystemInputs<'a> {
    pub paused: bool,
    pub autostart: Option<bool>,
    pub dark: bool,
    /// The stored preference, clamped on the way in.
    pub opacity: u8,
    /// `theme::transparency_block`'s answer for this machine, right now.
    pub block: Option<crate::theme::TransparencyBlock>,
    pub paths: &'a Paths,
    /// The log file's size in bytes, or `None` when it could not be read --
    /// which is not the same as zero and must not render as `0 bytes`.
    pub log_bytes: Option<u64>,
}

/// The System page, decided in one place.
///
/// Every branch here is a decision the design argues for and the two CI jobs
/// that never compile a wndproc can check: which rows appear, what the
/// transparency slot says, and which half of each file row is the label.
pub fn system_state(i: SystemInputs) -> SystemState {
    SystemState {
        paused: i.paused,
        autostart: i.autostart,
        dark: i.dark,
        transparency: Transparency::resolve(i.block, i.opacity),
        config: FileRow {
            name: file_name_of(&i.paths.config),
            // The DIRECTORY, because the file name is already the row's own
            // label and a row that said `apps.windows.toml` twice would be
            // spending its value slot on nothing. The window draws it through
            // `SS_PATHELLIPSIS`, so the shortening is width-aware and belongs
            // to the OS rather than to a character count here.
            value: dir_of(&i.paths.config),
        },
        log: i.paths.log.as_ref().map(|p| FileRow {
            name: file_name_of(p),
            value: match i.log_bytes {
                Some(n) => size_label(n),
                // A path `serve` was given but has not written to, or one
                // deleted underneath. Short, value-shaped and true; `0 bytes`
                // would claim the file exists and is empty.
                None => "not found".to_string(),
            },
        }),
    }
}

/// A path's last component, or the whole path when it has none.
fn file_name_of(p: &std::path::Path) -> String {
    match p.file_name() {
        Some(f) => f.to_string_lossy().into_owned(),
        None => p.to_string_lossy().into_owned(),
    }
}

/// The directory a file sits in, with a trailing separator so it reads as a
/// folder rather than as a file that lost its extension.
///
/// A path with no parent (a bare file name, which is what `serve some.toml`
/// gives) has no directory to show, so the slot is empty rather than `.\`:
/// the row's job is to say where the file is, and "here" is what the reader
/// already assumed.
fn dir_of(p: &std::path::Path) -> String {
    match p.parent() {
        Some(d) if !d.as_os_str().is_empty() => {
            let mut s = d.to_string_lossy().into_owned();
            if !s.ends_with(std::path::MAIN_SEPARATOR) {
                s.push(std::path::MAIN_SEPARATOR);
            }
            s
        }
        _ => String::new(),
    }
}

// ---------------------------------------------------------------------------
// The About page
// ---------------------------------------------------------------------------

/// The hook disclosure, moved off Keyboard by design §3.4 and shown on About.
///
/// **Two halves, and the second one is the whole reason this is a sentence
/// rather than an icon.** An unsigned process that holds a `WH_KEYBOARD_LL`
/// hook, calls `SendInput` and writes an autorun key owes the reader both
/// *when it holds the hook* and *what it does not keep*. The second is a
/// NEGATIVE claim -- there is no colour, dot or control state that can draw
/// "beckon keeps no record of what you type", because the absence of a
/// keystroke log has no rendering. Words are the only surface it has.
///
/// **Verbatim from the mock-up**, and `"while Caps Lock is on"` means the
/// SETTING on the Keyboard page (`Use Caps Lock as a shortcut key`), not the
/// lock's LED. Do not "correct" it to the LED: `sync_caps_hook` installs the
/// hook from `keyboard.caps`, and the sentence would then describe something
/// beckon does not do.
///
/// **The claim is conservative in the safe direction**, which is worth
/// knowing before anyone tightens it. Pausing also removes the hook
/// (`clear_bindings` plus `uninstall_for(HookReason::Caps)`), so the set of
/// moments the hook is really installed is a SUBSET of the two named here --
/// and the load-bearing half of the sentence is the negative one, "not at any
/// other time", which stays true under a subset.
///
/// ASCII, like every other display string in this window.
pub const HOOK_DISCLOSURE: &str = "The keyboard hook is installed only while \
     Caps Lock is on, or while you are recording a shortcut. beckon keeps no \
     record of what you type.";

/// One About row's two strings: what the page shows, and what the copy
/// button puts on the clipboard.
///
/// **They are not the same string, and the type is what says so.** `shown`
/// may carry a verdict clause the reader needs (`Location`) and is shortened
/// by the OS at draw time (`SS_PATHELLIPSIS`); `copy` is the bare payload,
/// because the thing a user does with a copied path is paste it into Explorer
/// or a bug report, and both fail on `…\current\beckon-serve.exe  (updated on
/// disk, restart to run it)`. A single `String` here would have made the two
/// jobs one field and the clipboard would have got whichever won.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AboutValue {
    pub shown: String,
    pub copy: String,
}

/// What is at the running image's own path right now.
///
/// The caller does the `stat`; this is the three answers it can come back
/// with, kept apart because `Gone` and `Unknown` mean opposite things to the
/// reader -- one is a fact worth printing, the other is beckon declining to
/// claim anything.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageOnDisk {
    /// `stat` succeeded, and this is the file's modification time.
    Written(std::time::SystemTime),
    /// The path resolves to nothing: the version directory was cleaned up, or
    /// beckon was uninstalled while this process kept running.
    Gone,
    /// It could not be asked -- no `current_exe()`, a `stat` that failed for
    /// some reason other than absence, or a file system with no mtime.
    Unknown,
}

/// Whether the file this process is RUNNING is the file its launch path
/// names today.
///
/// **This is the identity half of the stale-image verdict, and it is the half
/// that can see the recorded failure.** The clock half (`image_age`'s
/// `started` vs `disk` comparison) provably cannot -- see the measurement in
/// `image_age`'s own doc. On a scoop install the launch path is
/// `…\apps\beckon\current\beckon-serve.exe`, a junction; a `scoop update`
/// repoints it at a new version directory while a running process goes on
/// executing the old one. Comparing the two resolutions is the question
/// "am I the file my own path names?", which is what the a14 incident was.
///
/// **The comparison FAILS SAFE, and that is load-bearing**, because what
/// `QueryFullProcessImageNameW` returns for a junction launch has not been
/// measured on hardware. Both sides are canonicalised before comparison, so:
///
/// - if it returns the RESOLVED image path (the documented reading), the old
///   version directory canonicalises to itself, today's launch path
///   canonicalises to the new one, and the two differ -- `Diverged`;
/// - if it returns the UNRESOLVED launch path (`GetModuleFileNameW`'s
///   behaviour, which is what PowerShell's `MainModule.FileName` showed on
///   a14), canonicalising it yields today's target, both sides are the same
///   string, and the answer is `Same`.
///
/// The pessimistic reading therefore degrades to **silence**, never to a
/// false alarm. That matters more than the optimistic gain: a check that
/// cried `updated on disk, restart to run it` at every scoop user on every
/// open would be worse than the row not existing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageIdentity {
    /// Both resolve to the same file.
    Same,
    /// They resolve to different files. The launch path has been repointed
    /// and this process is still executing what it used to name.
    Diverged,
    /// One of the two could not be resolved -- no `current_exe()`, no
    /// process image path, or a `canonicalize` that failed. Includes the
    /// ordinary case where `scoop cleanup` has deleted the version directory
    /// this process is running from, so the running image's own path no
    /// longer resolves.
    Unknown,
}

/// Compare the running image's path against what the launch path resolves to
/// now.
///
/// **Both arguments must already be canonicalised, by the same call, on the
/// same machine** -- that is the caller's job because canonicalisation is an
/// OS operation and this crate compiles on three of them. See `ImageIdentity`
/// for why canonicalising both is what makes the answer fail safe.
///
/// The comparison is ASCII-case-insensitive: Windows paths are, and both
/// strings come out of the same API on the same volume so the fold is a belt
/// rather than a decision.
pub fn image_identity(
    running: Option<&std::path::Path>,
    launch_target: Option<&std::path::Path>,
) -> ImageIdentity {
    match (running, launch_target) {
        (Some(a), Some(b)) => {
            let (a, b) = (a.to_string_lossy(), b.to_string_lossy());
            if a.eq_ignore_ascii_case(&b) {
                ImageIdentity::Same
            } else {
                ImageIdentity::Diverged
            }
        }
        _ => ImageIdentity::Unknown,
    }
}

/// Whether the process is still running the image that is on disk.
///
/// **This row exists because of a recorded failure, not a hypothetical
/// one.** A watchdog-started beckon on a14 ran the 0.8.0 image for three
/// hours while `beckon --version` said 0.9.0 and scoop's `current` junction
/// pointed at 0.9.0 -- **both obvious surfaces lied**, because both were
/// answering about the file on disk and the question was about the process.
///
/// **CORRECTED 2026-08-15: for one day this enum's only producer could not
/// see that failure.** The paragraph above says why the row exists and was
/// read as saying the row covers it. It did not: `image_age` was a clock
/// comparison alone, and the measurement in its doc shows the clock answers
/// `Current` -- silence -- on the exact a14 timeline. `ImageIdentity` is the
/// second producer, added to close it; the clock is kept because it catches
/// a different case (an in-place overwrite, where the path never changes).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageAge {
    /// Nothing says the file has been replaced. **Not a clean bill of
    /// health** -- see `note`.
    Current,
    /// What is on disk is not what is running. **Two producers, and they
    /// catch different failures**: the launch path now resolves to a
    /// different file from the one this process is executing
    /// (`ImageIdentity::Diverged` -- a moved scoop junction), or the file at
    /// the launch path was written after this process started (the clock --
    /// an in-place overwrite). Neither subsumes the other.
    Replaced,
    /// There is no file at the launch path any more.
    Missing,
    /// One of the two timestamps could not be read.
    Unknown,
}

impl ImageAge {
    /// What the `Location` row adds after the path, or `None` for silence.
    ///
    /// **`Current` and `Unknown` both say nothing, and that is rule 2 rather
    /// than laziness**: silence is the healthy state, and the alternative
    /// here is worse than merely noisy. Both tests behind `image_age` are
    /// ONE-SIDED -- `Replaced` is reliable, `Current` is only "no evidence of
    /// replacement", and `image_age`'s doc measures a real case where that
    /// evidence is absent by construction -- so a row that printed *up to
    /// date* would be making the exact kind of confident claim the a14
    /// incident is about. Saying nothing costs a missed warning; saying "up
    /// to date" would cost the reader the reason they came to this row.
    pub fn note(self) -> Option<&'static str> {
        match self {
            ImageAge::Current | ImageAge::Unknown => None,
            // Restart, not "update": beckon is already updated on disk. The
            // action left is the one only the user can take.
            ImageAge::Replaced => Some("updated on disk, restart to run it"),
            ImageAge::Missing => Some("no longer on disk"),
        }
    }
}

/// Decide the verdict from an identity test and a clock comparison.
///
/// **STRUCK 2026-08-15, and the measurement is below.** This function used to
/// take two arguments and carry this claim: *"Why time rather than identity.
/// The strong test would be 'is the file at this path the same file this
/// process mapped', and Win32 has no cheap way to ask it … So the comparison
/// available is coarse."* The identity test is one call
/// (`QueryFullProcessImageNameW`) plus two `canonicalize`s and is now the
/// first thing this function asks; see `image_identity`, and note that its
/// untested half degrades to silence rather than to a false alarm, which is
/// what made building it cheaper than continuing to name it.
///
/// # The clock half cannot fire on the incident this row exists for
///
/// Not reasoned -- **measured on the artifact itself, 2026-08-15**, by
/// downloading `beckon-0.9.0-aarch64-pc-windows-msvc.zip` from the release
/// that a14 updated to and reading the entry timestamps out of the zip
/// directory:
///
/// ```text
/// beckon-serve.exe  2026-08-12T22:37:14
/// beckon.exe        2026-08-12T22:37:18
/// ```
///
/// Those are the stored `LastWriteTime`s that `Compress-Archive` put in
/// (`.github/workflows/release.yml`, the Windows packaging step), and every
/// extractor scoop uses restores them.
///
/// The a14 timeline (`a14-upgrade-verify-running-image`): the watchdog
/// started beckon at 05:40:01 and scoop created `…\apps\beckon\0.9.0` at
/// 05:40:05 -- **four seconds later**. scoop cannot have unpacked an artifact
/// before that artifact existed, so the process started at most four seconds
/// before something that necessarily follows 22:37:18Z. Therefore
/// `written < started`, in every timezone, and the clock comparison answers
/// `Current`, whose `note()` is `None`. **The row said nothing at all for the
/// three hours it existed to describe.** No arithmetic about a14's clock is
/// needed to reach that; the ordering is forced by the causality.
///
/// Two mechanisms put it there and either is sufficient:
///
/// 1. **The mtime is the release build's**, as measured above -- so a
///    freshly unpacked image and a months-old one are indistinguishable by
///    it. The doc under `ImageAge` already said scoop's unpack preserves
///    stored timestamps; what nobody did was join that sentence to the row's
///    coverage claim.
/// 2. **The `stat` follows the junction anyway.** `disk` comes from
///    `metadata(current_exe())`, and path traversal resolves `\current\` at
///    open time -- so the file being timed is the NEW image, never the one
///    actually running. The clock half is structurally answering a question
///    about a file this process is not executing.
///
/// **What the clock half IS still for**, and why it is kept: an in-place
/// overwrite, where the launch path never changes and identity therefore says
/// `Same`. `cargo build` over a running binary's own path is the everyday
/// case, and a non-scoop install updated by copying a new exe over the old is
/// the shipped one.
///
/// **And it is one-sided; do not read `Current` as a guarantee.** For the
/// reason measured above, `Current` means only "neither test found evidence
/// of a replacement". That is why `ImageAge::note` is silent for it: a false
/// negative costs a missing warning, a false positive would cost the row its
/// credibility.
///
/// Equality is `Current`: the image has to exist before it can be executed,
/// so `written == started` is the ordinary case on a fast machine, not a
/// replacement in the same tick.
///
/// **Order: `Gone`, then identity, then the clock.** `Gone` first because a
/// path that resolves to nothing makes both other tests meaningless.
/// Identity before the clock because it is the reliable one -- and note that
/// `ImageIdentity::Same` does NOT short-circuit to `Current`: it is not
/// evidence against an in-place overwrite, only against a repointed path.
pub fn image_age(
    started: Option<std::time::SystemTime>,
    disk: ImageOnDisk,
    identity: ImageIdentity,
) -> ImageAge {
    if let ImageOnDisk::Gone = disk {
        return ImageAge::Missing;
    }
    if let ImageIdentity::Diverged = identity {
        return ImageAge::Replaced;
    }
    match (started, disk) {
        (Some(start), ImageOnDisk::Written(w)) => {
            if w > start {
                ImageAge::Replaced
            } else {
                ImageAge::Current
            }
        }
        _ => ImageAge::Unknown,
    }
}

/// What the caller knows that `about_state` turns into a page.
#[derive(Debug, Clone, Copy)]
pub struct AboutInputs<'a> {
    /// `env!("CARGO_PKG_VERSION")` of the crate that DREW the window.
    ///
    /// **This one cannot lie**, and it is half the answer to the a14
    /// incident on its own: the page is painted by the running process, so
    /// its version is the running version -- unlike a fresh `beckon
    /// --version`, which starts whatever is on disk today.
    pub version: &'a str,
    /// The target triple, assembled by the caller from what the compiler
    /// told it. Composed there rather than here because this crate compiles
    /// on three platforms and `-pc-windows-` is not a fact about any of them.
    pub target: &'a str,
    /// `std::env::current_exe()`, deliberately UNRESOLVED. `None` when it
    /// could not be read at all.
    pub exe: Option<&'a std::path::Path>,
    /// When this process started, for `image_age`'s clock half.
    pub started: Option<std::time::SystemTime>,
    /// What is at `exe` now, for `image_age`'s clock half.
    pub disk: ImageOnDisk,
    /// Whether the image this process is executing is the file `exe` names
    /// today -- `image_age`'s identity half, and the only half that can see
    /// a moved scoop junction. The caller resolves both paths; see
    /// `image_identity`.
    pub identity: ImageIdentity,
    /// `env!("CARGO_PKG_LICENSE")`.
    pub licence: &'a str,
}

/// What to do the moment Accessibility appears, having been missing.
///
/// **macOS hands a grant to a NEW process**, so beckon cannot simply start
/// working when the switch is flipped -- it has to be restarted. Left to the
/// user that is a second instruction after the dialog, and a new user has no
/// reason to know it: they allowed beckon, nothing happened, and the obvious
/// reading is that beckon is broken.
///
/// The answer differs by who started beckon, and the signal is the one
/// `macos_broken_config` already uses:
///
/// * **launchd** (stderr is not a terminal) -- exit non-zero so
///   `KeepAlive { SuccessfulExit: false }` brings beckon straight back. Zero
///   would NOT restart, deliberately: that is what makes the tray's Quit work.
/// * **a person in a terminal** -- say so and keep running. Killing a process
///   somebody started by hand, to restart something they are not running under
///   a supervisor, would lose them their beckon entirely.
///
/// `Nothing` covers both the ordinary case (still missing, or granted all
/// along) and the one that matters for correctness: this must fire on the
/// TRANSITION only, or a beckon whose grant is simply present would exit on
/// every tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GrantRecovery {
    Nothing,
    TellAndStay,
    RestartUnderLaunchd,
}

pub fn grant_recovery(
    was_missing: bool,
    granted_now: bool,
    stderr_is_terminal: bool,
) -> GrantRecovery {
    if !(was_missing && granted_now) {
        return GrantRecovery::Nothing;
    }
    if stderr_is_terminal {
        GrantRecovery::TellAndStay
    } else {
        GrantRecovery::RestartUnderLaunchd
    }
}

/// Should the About page offer a button that asks for Accessibility?
///
/// **Only when the grant is missing, and this is the whole rule.** A button
/// that is always there invites a person with working permissions to press it
/// and get nothing: macOS raises the dialog only for a process with no answer
/// recorded, so for everyone else it is a control that visibly does nothing.
///
/// It exists because of what a user actually hit -- beckon said the grant was
/// missing and gave no way to give it. `IOHIDCheckAccess` and
/// `AXIsProcessTrusted` only ASK; neither raises a dialog, so a binary with no
/// TCC row can never acquire one through them. Finding the pane by hand means
/// typing a path that, on a nix or Homebrew install, carries a hash or a
/// version and changes on every update.
pub fn grant_button_shown(granted: bool) -> bool {
    !granted
}

/// The About page, decided in one place.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AboutState {
    /// `beckon 0.9.3` -- the name row under the mark.
    pub name: String,
    pub build: AboutValue,
    /// **The highest-value row on the page** (design §3.4): the running
    /// image's own path, plus a verdict when there is one worth printing.
    pub location: AboutValue,
    pub licence: AboutValue,
    /// Kept beside `location` so a caller can act on the verdict without
    /// parsing the string it produced.
    pub image: ImageAge,
}

/// `SystemTime` as `YYYY-MM-DD`, UTC, or `None` when it is before the epoch.
///
/// **Hand-rolled because the workspace has no date crate and this is the only
/// date it shows.** Howard Hinnant's `civil_from_days`, which is exact for
/// every day in the proleptic Gregorian calendar and is about twenty lines --
/// against a dependency that would be compiled into six release artefacts to
/// format one string.
///
/// UTC, not local. The row it feeds identifies a BUILD, and a build has one
/// date wherever it is read; rendering it in the reader's zone would make the
/// same binary claim two different dates on two machines, which is exactly the
/// confusion the About page exists to end.
fn ymd(t: std::time::SystemTime) -> Option<String> {
    let secs = t.duration_since(std::time::UNIX_EPOCH).ok()?.as_secs();
    // Days since 1970-01-01, shifted to an era starting 0000-03-01 so that the
    // leap day lands at the END of a year and every month has a fixed length.
    let z = (secs / 86_400) as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097); // day of era, 0..=146096
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365; // 0..=399
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // 0..=365
    let mp = (5 * doy + 2) / 153; // 0..=11, March-based
    let d = doy - (153 * mp + 2) / 5 + 1; // 1..=31
    let m = if mp < 10 { mp + 3 } else { mp - 9 }; // 1..=12
    let y = if m <= 2 { y + 1 } else { y };
    Some(format!("{y:04}-{m:02}-{d:02}"))
}

/// Build the page. Every branch is a decision the design argues for and the
/// two CI jobs that never compile a wndproc can check.
pub fn about_state(i: AboutInputs) -> AboutState {
    let age = image_age(i.started, i.disk, i.identity);
    // `None` from `current_exe()` is a real state -- the call can fail -- and
    // an empty row would read as a rendering fault. `unknown` is short,
    // value-shaped and true, the same shape `system_state` uses for a log
    // file it cannot stat.
    let path = i
        .exe
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|| "unknown".to_string());
    // Design §3.4 draws `Build   aarch64-pc-windows-msvc · 2026-08-13`, and the
    // date came from nowhere for a day: `AboutInputs` had no member for one.
    //
    // **It costs no new plumbing, because the page already holds it.**
    // `i.disk` is `image_age`'s clock half, and the date is that mtime.
    //
    // **CORRECTED within a day of landing: this said "the date of the FILE
    // THIS PROCESS IS RUNNING", and that is false.** `disk` is a `stat` of
    // `current_exe()` — the LAUNCH PATH — which on a scoop install runs
    // through the `current` junction, so after an update it dates the image
    // the junction names NOW, not the one executing. That is the very surface
    // the a14 incident showed lying, and asserting the opposite here would
    // have put a fresh false claim into the file whose whole discipline is
    // removing them. `about_now`'s own doc already drew the distinction —
    // `stat` is the clock half, `QueryFullProcessImageNameW` is the identity
    // half — and this comment contradicted the function it reads from.
    //
    // **Stat'ing the running image instead does not rescue it**, which is why
    // the wording moved rather than the code. `running_image_path()` is
    // available, but on a14 `QueryFullProcessImageNameW` gave back the launch
    // path (`about_now` records the measurement), so that `stat` traverses the
    // same junction and reaches the same file. There is no cheap route to a
    // date that is guaranteed to be the running image's.
    //
    // **What makes the row honest is the page, not this cell.** `Location`
    // carries the stale-image verdict from `image_age`, so when the launch
    // path has been repointed the reader is told so one row down — and a date
    // that agrees with a `Replaced` verdict is read as "this is what is on
    // disk now", which is what it is.
    //
    // Measured 2026-08-16: after `scoop update beckon` to 0.9.5 on a14 the
    // image's mtime read 03:50:34 against a release built at 03:50 -- so the
    // timestamp survives the release zip and scoop's extraction, and it is a
    // build date rather than an install date.
    //
    // Absent when the file is `Gone` or `Unknown`, and then the row is the
    // triple alone: a row that says less is better than one that guesses.
    let built = match i.disk {
        ImageOnDisk::Written(t) => ymd(t),
        ImageOnDisk::Gone | ImageOnDisk::Unknown => None,
    };
    let build_text = match &built {
        Some(d) => format!("{} · {}", i.target, d),
        None => i.target.to_string(),
    };
    AboutState {
        name: format!("beckon {}", i.version),
        // **`copy` is the same string as `shown` here, unlike `Location`
        // below**, and the difference is what `copy_text`'s rule is actually
        // about: it says copy the row's bare payload rather than what is on
        // screen BECAUSE `Location` adds a verdict clause and is shortened by
        // `SS_PATHELLIPSIS`, so its screen text is neither complete nor
        // pasteable. This row has no verdict and no ellipsis -- the triple and
        // the date are both payload, and a bug report wants both.
        build: AboutValue {
            shown: build_text.clone(),
            copy: build_text,
        },
        location: AboutValue {
            shown: match age.note() {
                // Two spaces, the same separator `opacity_slot` uses one page
                // across, for the same reason: one STATIC, one alignment, and
                // a clause that has to read as an aside rather than as part
                // of the path.
                Some(note) => format!("{path}  ({note})"),
                None => path.clone(),
            },
            // The BARE path, never the annotated one: a copied path is
            // pasted into Explorer or a bug report, and both fail on a string
            // with a parenthesised verdict glued to the end of it.
            copy: path,
        },
        licence: AboutValue {
            shown: i.licence.to_string(),
            copy: i.licence.to_string(),
        },
        image: age,
    }
}

/// What each copy button puts on the clipboard.
///
/// **One rule for three rows: the row's own payload, unshortened and
/// unannotated.** The alternatives were weighed and both lose to it -- a
/// `label: value` pair would break the only thing a copied path is for, and a
/// one-button "copy everything" is the `Copy diagnostics` button design §3.3
/// deleted, whose whole argument was that every fact it gathered is already
/// on screen with a button beside it.
pub fn copy_text(st: &AboutState, f: Field) -> &str {
    match f {
        Field::Build => &st.build.copy,
        Field::Location => &st.location.copy,
        Field::Licence => &st.licence.copy,
    }
}

/// Everything a settings window reports back. The caller owns all policy:
/// what an edit means, whether a close is allowed, what Save writes.
///
/// Defined here rather than in a per-OS crate for the same reason
/// `ControlState` is: the window is a renderer of `ControlState` and a
/// raiser of these, and neither half is Win32- or AppKit-shaped. Both
/// `beckon_windows::settings_window` and `beckon_macos::settings_window`
/// implement exactly this, so `serve.rs` builds one set of callbacks.
pub struct Callbacks {
    /// A row became current. The index is a **model** row -- the window has
    /// already mapped it through `ListItem::row`, because a list widget
    /// only ever knows the position within the filtered list it was given.
    pub on_select: Box<dyn FnMut(usize)>,
    /// A row's tick changed: `(model row, ticked)`. Independent of
    /// `on_select` -- one click can raise both, and neither implies the
    /// other.
    pub on_mark: Box<dyn FnMut(usize, bool)>,
    pub on_edit_combo: Box<dyn FnMut(String)>,
    /// The shortcut controls now spell a whole chord: find out whether
    /// anything else already has it.
    ///
    /// Separate from `on_edit_combo`, and raised FIRST, for two reasons
    /// that are both about not lying:
    ///
    /// 1. **It is a global OS mutation**, however brief -- one hotkey
    ///    registration round trip -- so it must be raised by a change to the
    ///    shortcut and by nothing else.
    /// 2. **The model must still hold the row's PREVIOUS chord** when the
    ///    caller decides. `probe_plan`'s "Unchanged - this row already uses
    ///    it" compares the typed chord against the row's own, so a probe
    ///    asked after `on_edit_combo` has written it would find every chord
    ///    unchanged and never ask the OS anything.
    pub on_probe_shortcut: Box<dyn FnMut(String)>,
    pub on_edit_app: Box<dyn FnMut(String)>,
    /// The filter box's text changed. Indices in `on_select` / `on_mark`
    /// are model rows either way -- the window maps them.
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
    /// Everything that is not an edit to a binding. See `SettingsCommand`
    /// for why this is one field and not eleven.
    pub on_command: Box<dyn FnMut(SettingsCommand)>,
}

impl Model {
    pub fn from_text(text: &str) -> Result<Model, String> {
        let cfg = parse_config(text)?;
        // `parse_config` returns BTreeMap order; the window shows file
        // order, which is what the user sees in their editor.
        let order = top_level_keys(text);
        let mut rows: Vec<Row> = cfg
            .shortcuts
            .iter()
            .map(|s| {
                let canon = s.combo.canonical();
                let raw = order
                    .iter()
                    .find(|k| {
                        Combo::parse(k)
                            .map(|c| c.canonical() == canon)
                            .unwrap_or(false)
                    })
                    .cloned();
                Row {
                    orig_key: raw.clone(),
                    combo: raw.unwrap_or(canon),
                    app: s.app.clone(),
                    marked: false,
                }
            })
            .collect();
        rows.sort_by_key(|r| {
            r.orig_key
                .as_deref()
                .and_then(|k| order.iter().position(|o| o == k))
                .unwrap_or(usize::MAX)
        });
        Ok(Model {
            rows,
            keyboard: cfg.keyboard,
            selected: None,
            original: text.to_string(),
            dirty: false,
            filter: String::new(),
        })
    }

    pub fn dirty(&self) -> bool {
        self.dirty
    }

    /// Set the list filter. Deliberately does NOT set `dirty`, for the same
    /// reason `set_marked` does not: `apply_enabled` is `dirty && valid`, so
    /// a filter that dirtied the model would light up Save and rewrite the
    /// file byte-identical.
    pub fn set_filter(&mut self, v: &str) {
        self.filter = v.to_string();
    }

    pub fn filter(&self) -> &str {
        &self.filter
    }

    /// The indices in `rows` the filter is showing, in model order.
    ///
    /// Matched case-insensitively against the app name.
    ///
    /// Trimmed first: a trailing space left by typing would otherwise match
    /// nothing and hide every row, which reads as a hang.
    ///
    /// **The selected row is always visible, matching or not.** Without that
    /// exception, editing a row until it stops matching pulls the row out from
    /// under the editor mid-word: `control_state` returns `selected: None` and
    /// `detail: None`, and `apply_state`'s `None` arm then disables the very
    /// field that has keyboard focus and blanks it, leaving the partial value
    /// in the model and nothing on screen to explain it. Type `Brave` into a
    /// row while the filter says `brave`, press Backspace, and the field dies.
    ///
    /// It also closes the relayout path that measurements §40 measured and
    /// judged benign: with a row selected the list can no longer reach zero
    /// rows, so `Ui::shown_empty` cannot flip, so `layout` -- and its
    /// `SetWindowPos` on a populated App combo -- never runs on a filter
    /// keystroke at all. §40's argument was that the whole-string selection
    /// could not be consumed; this removes the selection instead of arguing
    /// about it.
    ///
    /// This does NOT undo `add_row` clearing the filter. That decision is
    /// about a *new* row, which is empty and would match no filter; this is
    /// about an *existing* row the user is working on.
    ///
    /// Model order is a precondition of `remove_indices`, not a convenience.
    ///
    /// **The filter matches the app name ONLY, never the chord.** It used to
    /// match both, and every beckon chord contains `alt` -- so `a` matched
    /// every row while the filter box looked as though it had narrowed the
    /// list. With `Remove` taking the ticked rows, that is a path to
    /// deleting the whole table by typing one letter. Measured with four
    /// bindings and filter `a`: `visible` returned all four.
    ///
    /// This block used to justify the combo arm by saying both columns is
    /// the rule `beckon search` already uses, so the program has no third
    /// matching dialect. That sentence was true and is no longer: `search`
    /// still matches widely because its worst outcome is a long list, while
    /// this window's worst outcome is a deleted binding. The dialects differ
    /// on purpose, and the reason is the consequence of a false positive,
    /// not the shape of the data.
    ///
    /// What this gives up is real and is pinned by
    /// `filtering_by_a_key_name_finds_nothing`: the window can no longer
    /// answer "what already owns this chord?" by filtering. If that has to
    /// come back, match the chord's KEY (`f2`, `b`) -- the half a person
    /// searches for and the half that is not `alt` on every row -- and never
    /// the whole chord as a substring again.
    fn visible(&self) -> Vec<usize> {
        let f = self.filter.trim().to_lowercase();
        if f.is_empty() {
            return (0..self.rows.len()).collect();
        }
        self.rows
            .iter()
            .enumerate()
            .filter(|(i, r)| self.selected == Some(*i) || r.app.to_lowercase().contains(&f))
            .map(|(i, _)| i)
            .collect()
    }

    pub fn set_combo(&mut self, i: usize, v: &str) {
        if self.rows[i].combo != v {
            self.rows[i].combo = v.to_string();
            self.dirty = true;
        }
    }

    pub fn set_app(&mut self, i: usize, v: &str) {
        if self.rows[i].app != v {
            self.rows[i].app = v.to_string();
            self.dirty = true;
        }
    }

    pub fn add_row(&mut self) {
        // A new row matches no non-empty filter, so it would be created off
        // screen while the editor strip pointed at it. Checklist item 6
        // ("after Add, the new row is visible AND selected") keeps its
        // meaning this way instead of needing a new one.
        self.filter.clear();
        self.rows.push(Row {
            orig_key: None,
            combo: String::new(),
            app: String::new(),
            marked: false,
        });
        self.selected = Some(self.rows.len() - 1);
        self.dirty = true;
    }

    pub fn remove_row(&mut self, i: usize) {
        self.rows.remove(i);
        self.selected = if self.rows.is_empty() {
            None
        } else {
            Some(i.min(self.rows.len() - 1))
        };
        self.dirty = true;
    }

    /// Toggle the checkbox for row `i`. Deliberately does NOT set `dirty`:
    /// a tick changes nothing on disk, and `apply_enabled` is
    /// `dirty && valid` -- marking would light up Save for an otherwise
    /// untouched model and rewrite the file byte-identical.
    pub fn set_marked(&mut self, i: usize, on: bool) {
        self.rows[i].marked = on;
    }

    /// How many **visible** rows are ticked.
    ///
    /// Visible rather than all, because `remove_pressed` acts only on rows
    /// the filter is showing -- a count that included hidden ticks would put
    /// a number on screen that Remove does not honour. Feeds
    /// `ControlState::marked_count`, which does NOT caption the remove button
    /// `Remove N` -- see that field's doc for why the caption stays constant.
    pub fn marked_count(&self) -> usize {
        self.visible()
            .iter()
            .filter(|&&i| self.rows[i].marked)
            .count()
    }

    /// Remove the rows at `idx`, in one go.
    ///
    /// Walks the marked indices in reverse: removing row 0 first would
    /// shift every later row down by one, so the next removal by index
    /// would take the wrong row. Reverse order removes the highest index
    /// first, which never disturbs the position of any index still queued.
    ///
    /// `selected` is recomputed from how many marked rows sat ahead of it
    /// -- that count is exactly how far its slot shifts down, whether the
    /// selected row itself survives (it keeps pointing at the same row) or
    /// was removed alongside the others (it lands where that row's slot
    /// now falls, then gets clamped like `remove_row` already does).
    ///
    /// **`idx` must be ascending.** The reverse walk below removes the
    /// highest index first precisely so that nothing still queued shifts
    /// underneath it, and `Model::visible` returns model order, which
    /// satisfies that.
    fn remove_indices(&mut self, idx: &[usize]) {
        if idx.is_empty() {
            return;
        }
        self.selected = self.selected.map(|sel| {
            let before = idx.iter().filter(|&&m| m < sel).count();
            sel - before
        });
        for &i in idx.iter().rev() {
            self.rows.remove(i);
        }
        self.selected = if self.rows.is_empty() {
            None
        } else {
            self.selected.map(|sel| sel.min(self.rows.len() - 1))
        };
        self.dirty = true;
    }

    /// Remove every ticked row the filter is currently showing.
    pub fn remove_marked(&mut self) {
        let idx: Vec<usize> = self
            .visible()
            .into_iter()
            .filter(|&i| self.rows[i].marked)
            .collect();
        self.remove_indices(&idx);
    }

    /// What the Remove button does -- the WHOLE of what it does.
    ///
    /// **Ticks win over the selection.** Clicking a tick box also moves the
    /// highlight onto that row, so after ticking rows 1 and 3 the selection
    /// is 3, and a selection-only Remove would delete row 3, leave row 1
    /// ticked, and clamp the highlight onto a row the user never touched --
    /// so the NEXT press deletes that one. A tick box that a destructive
    /// button with no confirm and no undo ignores is worse than no tick box.
    ///
    /// It lives here, not in the wndproc's `on_remove` closure, for the same
    /// reason `default_button_of` does: `beckon-windows` compiles on one of
    /// the three CI jobs, and this decision is worth a test on all three.
    ///
    /// **And it never touches a row the filter is hiding.** Ticks survive
    /// being filtered out and come back when the filter is cleared, but they
    /// are inert while off screen: the property that makes a no-confirm,
    /// no-undo button acceptable is that its effect is visible.
    pub fn remove_pressed(&mut self) {
        let vis = self.visible();
        let marked: Vec<usize> = vis
            .iter()
            .copied()
            .filter(|&i| self.rows[i].marked)
            .collect();
        if !marked.is_empty() {
            self.remove_indices(&marked);
        } else if let Some(i) = self.selected.filter(|i| vis.contains(i)) {
            // Defence in depth, and currently unreachable: `visible()` exempts
            // the selected row from the filter, so a `Some` selection is always
            // in `vis`. Kept anyway, because the invariant belongs at the point
            // of deletion rather than being inherited from a view policy three
            // functions away -- if that exemption is ever reconsidered, this
            // holds the line instead of the line quietly moving.
            self.remove_row(i);
        }
    }

    pub fn set_caps(&mut self, on: bool) {
        if self.keyboard.caps != on {
            self.keyboard.caps = on;
            self.dirty = true;
        }
    }

    pub fn set_caps_tap(&mut self, t: CapsTap) {
        if self.keyboard.caps_tap != t {
            self.keyboard.caps_tap = t;
            self.dirty = true;
        }
    }

    /// Set what holding Caps stands for. Returns whether the model now holds
    /// `c`.
    ///
    /// **Refuses a chord with no modifiers.** The window can reach that by
    /// unticking the last chip, and `Chord::parse` rejects the same value on
    /// the way back in -- so accepting it here would let the window write a
    /// file beckon cannot read. Refusing at the setter keeps the unwritable
    /// state out of the model rather than catching it at render time.
    pub fn set_caps_hold(&mut self, c: Chord) -> bool {
        if !(c.ctrl || c.super_ || c.alt) {
            return false;
        }
        if self.keyboard.caps_hold != c {
            self.keyboard.caps_hold = c;
            self.dirty = true;
        }
        true
    }

    /// Every reason this model is not clean, one entry per offending row. A
    /// row may appear more than once. Only `Severity::Error` entries stop a
    /// write.
    pub fn problems(&self) -> Vec<Problem> {
        let mut out = Vec::new();
        let mut canon: Vec<Option<String>> = Vec::with_capacity(self.rows.len());
        for (i, r) in self.rows.iter().enumerate() {
            // A row the user just added and has not finished is a half-typed
            // edit, not a fault: `render` drops it instead of refusing. Its
            // complaints are warnings so they cannot disable Save for edits
            // made elsewhere in the file.
            let severity = if is_unfinished_new_row(r) {
                Severity::Warning
            } else {
                Severity::Error
            };
            match Combo::parse(&r.combo) {
                Ok(c) => canon.push(Some(c.canonical())),
                Err(e) => {
                    canon.push(None);
                    out.push(Problem {
                        row: Some(i),
                        severity,
                        message: e,
                    });
                }
            }
            if r.app.trim().is_empty() {
                out.push(Problem {
                    row: Some(i),
                    severity,
                    message: "app name is empty".to_string(),
                });
            }
        }
        // Duplicates: flag EVERY row in a colliding group, not just the
        // later ones -- the user needs to see both ends of the collision.
        // Unfinished new rows are skipped: `render` drops them, so they
        // cannot collide with anything in the file that gets written.
        let mut groups: HashMap<&str, Vec<usize>> = HashMap::new();
        for (i, c) in canon.iter().enumerate() {
            if is_unfinished_new_row(&self.rows[i]) {
                continue;
            }
            if let Some(c) = c {
                groups.entry(c.as_str()).or_default().push(i);
            }
        }
        let mut dups: Vec<(&str, Vec<usize>)> =
            groups.into_iter().filter(|(_, v)| v.len() > 1).collect();
        dups.sort_by_key(|(c, _)| *c);
        for (c, rows) in dups {
            for i in rows {
                out.push(Problem {
                    row: Some(i),
                    severity: Severity::Error,
                    message: format!("duplicate shortcut: another row also means `{c}`"),
                });
            }
        }
        out.sort_by_key(|p| p.row);
        out
    }

    /// The file text this model would write. `Err` if the model is invalid
    /// or the writer refuses. Never touches the filesystem.
    ///
    /// Refuses on `Severity::Error` only. A row the user just added and has
    /// not finished is dropped from the write instead of blocking it --
    /// otherwise clicking "Add" would disable Save until the new row was
    /// filled in, which is exactly the state a user is in while typing.
    ///
    /// The asymmetry with a row that came FROM the file and has been emptied
    /// out is deliberate: dropping that one would mean clearing an App field
    /// and pressing Save silently DELETES the binding. `orig_key` already
    /// tells the two apart, so distinguishing them costs nothing. Do not
    /// "simplify" the two rules into one.
    pub fn render(&self) -> Result<String, String> {
        if let Some(p) = self
            .problems()
            .iter()
            .find(|p| p.severity == Severity::Error)
        {
            return Err(match p.row {
                Some(i) => format!("row {}: {}", i + 1, p.message),
                None => p.message.clone(),
            });
        }
        let writes: Vec<RowWrite> = self
            .rows
            .iter()
            .filter(|r| !is_unfinished_new_row(r))
            .map(|r| RowWrite {
                orig_key: r.orig_key.clone(),
                combo: r.combo.clone(),
                app: r.app.trim().to_string(),
            })
            .collect();
        let text = render(&self.original, &writes, &self.keyboard)?;
        // Validate through the real parser rather than a second rule set:
        // this is what makes "what the UI writes is what beckon reads" true
        // by construction.
        parse_config(&text)?;
        Ok(text)
    }
}

/// Bare `key = value` lines at the root, in file order.
///
/// Deliberately a line scanner and not a parser: its only job is to make
/// the window show rows in the order the user wrote them. Anything it
/// misses simply falls back to canonical order, and `render` still
/// validates through `parse_config`, so this cannot become a second source
/// of truth about the file.
fn top_level_keys(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in text.lines() {
        let t = line.trim_start();
        if t.starts_with('[') {
            break; // a table header: everything after belongs to it
        }
        if t.starts_with('#') {
            continue;
        }
        let Some(eq) = t.find('=') else { continue };
        let raw = t[..eq].trim();
        if raw.is_empty() {
            continue;
        }
        let key = raw.trim_matches(|c| c == '"' || c == '\'');
        // Skip dotted keys (`keyboard.caps`): they are settings, not combos.
        if key.contains('.') && !key.starts_with('"') {
            continue;
        }
        out.push(key.to_string());
    }
    out
}

/// A row the user added and has not finished filling in. `orig_key` is what
/// tells it apart from a row that came from the file and has been emptied
/// out -- see `Model::render` for why the two must not be treated alike.
fn is_unfinished_new_row(r: &Row) -> bool {
    r.orig_key.is_none() && (r.combo.trim().is_empty() || r.app.trim().is_empty())
}

/// Whether holding Caps Lock can reach this combo: same modifiers as
/// `keyboard.caps_hold`, and no `shift` on top (the hook injects the chord
/// and nothing else).
fn combo_is_caps_chord(c: &Combo, hold: &Chord) -> bool {
    c.ctrl == hold.ctrl && c.super_ == hold.super_ && c.alt == hold.alt && !c.shift
}

// ---------------------------------------------------------------------------
// The availability probe
// ---------------------------------------------------------------------------

/// Everything beckon can say about whether a chord is free to bind.
///
/// The variants are the rows of the spec's string table (§F.6) and there are
/// no others, which is why they are matched exhaustively in `probe_notes` --
/// a new outcome cannot be added without writing the sentence that goes with
/// it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Availability {
    /// The OS accepted a registration for this chord and nothing else is
    /// holding it. **Not the same as "it works"** -- see `probe_notes`.
    Free,
    /// `Free`, and the chord carries the Windows key, which Windows reserves
    /// and can reclaim on its own.
    FreeWithWin,
    /// The row being edited already means this chord, so there is nothing to
    /// find out and nothing to collide with.
    Unchanged,
    /// Another row of this same file already means it. `app` is that row's
    /// app, so the note can name where it went.
    DuplicateInFile { app: String },
    /// The OS refused the registration. It does not say who holds the chord,
    /// and the note does not pretend otherwise.
    Taken,
    /// `f12` is somewhere in the chord.
    F12,
    /// Capture ran and no key-down arrived, so something above hotkey
    /// dispatch consumed it.
    ///
    /// **Nothing produces this yet** -- capture is a later task. It is
    /// defined here because it is one of §F.6's strings and the strings live
    /// together.
    CaptureSawNothing,
}

/// A verdict, and the chord it was about.
///
/// The chord rides along so a verdict can be *discarded* once the user types
/// something else, rather than shown against a chord it was never about.
/// `row_condition` is where that check happens.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeResult {
    pub combo: String,
    pub verdict: Availability,
}

/// What `probe_plan` concluded: either an answer, or permission to ask the
/// OS for one.
///
/// `AskTheOs` is deliberately not an `Availability` variant. It is the
/// *absence* of a verdict, and making it one would let a caller store it in
/// `RuntimeStatus::probe` and render it as though beckon had decided
/// something.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProbePlan {
    Verdict(Availability),
    AskTheOs,
}

/// Do two combo spellings mean the same chord?
///
/// `false` whenever either side fails to parse -- a string that names no
/// chord cannot be equal to one, not even to a byte-identical string that
/// also names none. That is what keeps a verdict about an unparseable combo
/// off the screen entirely (see `probe_plan` step 1).
fn same_chord(a: &str, b: &str) -> bool {
    match (Combo::parse(a), Combo::parse(b)) {
        (Ok(x), Ok(y)) => x == y,
        _ => false,
    }
}

/// Decide whether `combo` is free for row `row`, or whether only the OS can
/// say.
///
/// **The order below is the whole design, and it is not a preference.** A
/// live registration answers exactly one question -- *is anything else
/// holding this chord at this instant* -- and it answers `yes, free` for a
/// chord reserved for debuggers, for one this very file already binds two
/// rows down, and for the row's own current chord. Every step ahead of
/// `AskTheOs` is therefore a fact the OS cannot tell us, and asking first
/// would mean the green answer had already been computed before anything
/// noticed it was wrong.
///
/// **Every step before `AskTheOs` is load-bearing where it stands.** An
/// earlier version of this comment claimed the F12 guard "commutes with the
/// self-conflict checks", which is half true and therefore false: swapped
/// with step 4, both orders still refuse and land on the same `Mark::Bad` --
/// but not on the same verdict, since an f12 chord also duplicated in-file
/// answers `DuplicateInFile` in one order and `F12` in the other, so
/// "commutes" claims more than that. Swapped with step 3 the two are not
/// even that close: nothing rejects a row bound to
/// `ctrl+alt+f12` -- `problems()` has no f12 rule -- so with the guard below
/// step 3 that row, probed against its own chord, answers `Unchanged`, which
/// `probe_notes` renders `Mark::Ok`. That is a green OK on the one key this
/// guard exists to keep from ever coming back green.
/// `f12_outranks_the_rows_own_chord` pins it; the experiment that missed it
/// used a fixture binding no f12 and so measured the fixture.
///
/// **`RuntimeStatus::registered` is not consulted, ever.** `set_paused` and
/// `reload` CLEAR that map, so a paused beckon would report its own bound
/// chord as free. That map explains why a row is red; it never decides
/// whether a chord is free.
pub fn probe_plan(m: &Model, row: usize, combo: &str) -> ProbePlan {
    // 1. Not a chord at all. The row's own `problems()` already quotes the
    //    parser's reason verbatim, so the probe must add nothing: `Unchanged`
    //    is the only verdict that asserts nothing about the world outside
    //    this row. It cannot reach the screen either way -- `row_condition`
    //    folds a verdict in only on a canonical match, and an unparseable
    //    string has no canonical form -- so this is a fallback that says
    //    nothing new in both senses.
    let Ok(c) = Combo::parse(combo) else {
        return ProbePlan::Verdict(Availability::Unchanged);
    };

    // 2. MSDN reserves VK_F12 for debuggers "at all times ... even when you
    //    are not debugging". A registration on it therefore succeeds and
    //    proves nothing, which is the one outcome worth refusing outright:
    //    a green Available on a key documented never to arrive.
    //
    //    **Above step 3, and measured to be so.** See the doc comment: a row
    //    bound to f12 probed against its own chord answers `Unchanged` --
    //    `Mark::Ok` -- the moment this block moves below it.
    if c.key.name == "f12" {
        return ProbePlan::Verdict(Availability::F12);
    }

    // Compared in canonical form so `alt+ctrl+t` and `ctrl+alt+t` are one
    // chord, which is the same rule `problems()` uses for duplicates.
    let want = c.canonical();

    // 3. The row's own chord. A row is not in conflict with itself, and
    //    there is nothing to find out about a chord that is already bound
    //    to the thing being edited.
    if m.rows.get(row).is_some_and(|r| same_chord(&r.combo, &want)) {
        return ProbePlan::Verdict(Availability::Unchanged);
    }

    // 4. Any OTHER row of this same file. Read from the model in memory,
    //    which is the only place that knows about edits the user has not
    //    saved yet.
    //
    //    A half-typed new row is skipped for the same reason `problems()`
    //    skips it: `render` DROPS it, so it can collide with nothing that
    //    ever reaches the file. Blaming it produced
    //    `Already used by "" in this file.` -- empty quotes, naming a row
    //    that will never be written. The predicate is shared with
    //    `problems()` rather than restated, so the two cannot drift.
    for (i, r) in m.rows.iter().enumerate() {
        if i != row && !is_unfinished_new_row(r) && same_chord(&r.combo, &want) {
            return ProbePlan::Verdict(Availability::DuplicateInFile {
                app: r.app.trim().to_string(),
            });
        }
    }

    // 4b. The row's SAVED chord, which beckon itself is still holding.
    //
    //     `RegisterHotKey` refuses a chord registered anywhere on this
    //     desktop, and that includes `serve`'s own live table: the separate
    //     HWND rules out an `(hWnd, id)` identity collision, never a chord
    //     collision. The live table is the SAVED file while everything above
    //     reads the EDITED model, and the window raises the probe *before*
    //     `on_edit_combo` (so step 3 has the row's previous chord to compare
    //     against) -- so editing a row away from its saved chord and back
    //     misses step 3, reaches the OS, and comes back `Taken`: "Another
    //     program already has this shortcut", about beckon.
    //
    //     **Below step 4, not folded into step 3.** `Unchanged` is a
    //     `Mark::Ok` sentence, and a saved chord that another row spells NOW
    //     is a genuine conflict the user has to fix. Ranking this above the
    //     duplicate check would answer green there -- the same failure as
    //     moving the f12 guard below step 3. It only ever needs to outrank
    //     `AskTheOs`.
    //
    //     This narrows the false alarm to the row that owns the chord; a
    //     chord some OTHER row was saved with -- and has since been edited
    //     away from, or whose row was deleted outright, leaving no
    //     `orig_key` anywhere in `m.rows` to find -- is still asked about,
    //     and still answers `Taken`. Closing that needs the probe to read
    //     `ServeState::shortcuts`, which is a policy §F.6 has no verdict or
    //     string for -- so it stays disclosed rather than guessed at.
    if m.rows
        .get(row)
        .and_then(|r| r.orig_key.as_deref())
        .is_some_and(|k| same_chord(k, &want))
    {
        return ProbePlan::Verdict(Availability::Unchanged);
    }

    // 5. Nothing beckon holds can decide this. Now, and only now, ask.
    ProbePlan::AskTheOs
}

/// The sentences a verdict puts in the editor's notes strip.
///
/// The wording is §F.6's, verbatim and ASCII -- ASCII for the same reason
/// the mark glyphs are: the window inherits the shell font, and a missing
/// glyph reads as a defect. Nothing here names an API or an error code,
/// because none of those are things the person editing a shortcut can act
/// on.
///
/// **A registration is never reported as "this shortcut works."** The
/// strongest claim it licenses is that nothing else is holding the chord,
/// and that is what the `Free` strings say. A chord can still be swallowed
/// above hotkey dispatch, where nobody registered it and the probe therefore
/// succeeded, and another process may claim it between here and Save.
///
/// The paused sentence is *appended* rather than replacing the verdict, and
/// it is a different claim from `row_condition`'s "beckon is paused, so no
/// shortcut is active": that one is about the row, this one is about how far
/// to trust the verdict above it. Both appearing at once is correct.
pub fn probe_notes(r: &ProbeResult, paused: bool) -> Vec<Note> {
    let (mark, text) = match &r.verdict {
        Availability::Free => (
            Mark::Ok,
            "Available. Nothing else on this PC is using it.".to_string(),
        ),
        Availability::FreeWithWin => (
            Mark::Ok,
            "Available right now. Windows reserves Windows-key shortcuts and can take this one \
             back after an update, so press it once after saving to be sure."
                .to_string(),
        ),
        Availability::Unchanged => (
            Mark::Ok,
            "Unchanged - this row already uses it.".to_string(),
        ),
        Availability::DuplicateInFile { app } => (
            Mark::Bad,
            format!("Already used by \"{app}\" in this file. A shortcut can only mean one thing."),
        ),
        Availability::Taken => (
            Mark::Bad,
            "Another program already has this shortcut. Windows does not tell beckon which one, \
             so beckon cannot name it. Saved as-is, it will not fire."
                .to_string(),
        ),
        Availability::F12 => (
            Mark::Bad,
            "F12 is reserved for debugging tools and never reaches beckon. Pick a different key."
                .to_string(),
        ),
        // §F.6 puts this in the `..` column, not `!!`. Within that column the
        // shipped vocabulary splits `Unknown` ("beckon does not know yet":
        // the catalog scan is running, the row has not been registered yet)
        // from `Warn` ("beckon knows, and it is worth saying"). Capture
        // having run and seen nothing is knowledge, so it is `Warn`.
        // **`Win+L` used to be this sentence's example and is not any more.**
        // Design 3.1 puts that fact at the moment `Record` hears the chord,
        // and `capture::HINT_SYSTEM_CHORD` is where it went. Keeping it here
        // as well would be worse than a duplicate: `Win+L` cannot reach this
        // verdict at all -- capture DOES see it (measurements §48) and refuses
        // it as `Refusal::SystemChord` -- so the one chord this sentence named
        // was the one it could never be about. The general fact survives; the
        // example does not.
        Availability::CaptureSawNothing => (
            Mark::Warn,
            "Windows handled that shortcut itself, so beckon never saw it. Some shortcuts cannot \
             be reassigned by any program."
                .to_string(),
        ),
    };
    let mut out = vec![Note { mark, text }];

    if paused {
        out.push(Note {
            mark: Mark::Warn,
            text: "beckon is paused, so this shows what will happen when you resume.".into(),
        });
    }

    // Ctrl+Alt with no Windows key IS Alt Gr on an international layout, so
    // the chord fires while the user is typing an accented character. Worth
    // saying, not worth refusing -- the chord is genuinely free.
    if Combo::parse(&r.combo).is_ok_and(|c| c.ctrl && c.alt && !c.super_) {
        out.push(Note {
            mark: Mark::Warn,
            text: "On international layouts this is Alt Gr, so typing an accented character will \
                   fire it."
                .into(),
        });
    }
    out
}

/// The one place a row's condition is decided. Both the list flag and the
/// editor's notes are derived from it, so they cannot contradict each other
/// -- which they could when `items` read only the registration map and
/// `detail` read the catalog as well.
///
/// A row can be several things at once while `flag` is a single word, so the
/// order in which conditions are PUSHED below IS the precedence:
/// `paused` > `in use` > `missing` > `other chord` > none, and the flag is
/// whichever came first. `paused`
/// sits above the registration map deliberately: `serve` CLEARS that map
/// when it pauses, so consulting the map first would render every row as
/// "not registered yet" and never say why.
///
/// **Every condition is kept, not just the winning word**, which is the
/// difference between a precedence for the CELL and a claim that the losers
/// stopped existing. `conditions` is the whole list; `flag` is its head.
///
/// `mark` is derived from the notes AND every condition at the end rather than
/// assigned along the way, which is what makes "the list and the editor
/// cannot disagree" true by construction instead of by discipline. The
/// condition half is new -- see `flag_mark` -- and is what kept that true when
/// the notes went quiet.
///
/// **CORRECTED 2026-08-15: it folded the FLAG rather than the conditions, and
/// that lost a severity.** A paused row whose app is missing pushes both
/// `paused` and `missing`; `paused` wins the cell, and folding only the winner
/// reported `Mark::Warn` for a row that carried a `Mark::Bad` before the notes
/// went quiet. Nothing on screen reads `ListItem::mark` today, so it would have
/// stayed hidden until something did.
/// `a_paused_row_whose_app_is_missing_is_still_bad` is the pin, and it fails on
/// the version this replaces.
///
/// **Design 3.1: notes appear only for a condition the status word does not
/// already state, and a healthy row says nothing at all.** Four sentences
/// went, and which four is the whole of the rule:
///
/// | Was | Why it went |
/// |---|---|
/// | `Registered and working.` | the healthy state. Deleted by name in 3.1 -- rule 2, silence IS the report |
/// | `Another program already has this shortcut.` | kept, REWORDED. The word `in use` says a program has it; the mock-up's sentence adds the half the word cannot -- that beckon will never be able to name which |
/// | `No installed app has this name.` | `missing` says exactly that, in one word, on the row the user is looking at |
/// | `Uses a different chord.` | `other chord` says exactly that |
/// | `beckon is paused, so no shortcut is active.` | `paused` says it on every row at once, which is what makes a per-row sentence noise rather than news |
///
/// What SURVIVED is the test of the rule, because each says something no
/// status word does: `Pick a key and an app.` (an unfinished row claims no
/// flag), `Not registered yet.` and `Checking installed apps...` (both are
/// "beckon does not know", which is not a condition of the row), the
/// availability verdicts (about the chord in the editor right now, not about
/// the row), and every `Problem` the validator produces verbatim.
///
/// **The risk this takes, named rather than argued away:** three conditions
/// now speak in one word each, and 3.1 §7 records the same worry -- the old
/// draft shouted at everyone, this one risks whispering to nobody. The
/// mitigation is not in this function: it is that the four words survive at
/// all, which §7 lists under *what must never be cut*, because `in use` and
/// `missing` are the same severity and need completely different fixes.
fn row_condition(
    m: &Model,
    i: usize,
    rt: &RuntimeStatus,
    problems: &[Problem],
) -> (Mark, Option<String>, Vec<Note>) {
    let r = &m.rows[i];

    // Nothing below has anything to say about a row the user has not
    // finished: there is no combo to register and no app to look up. That
    // includes `paused`, which describes a shortcut that exists.
    if is_unfinished_new_row(r) {
        return (
            Mark::Unknown,
            None,
            vec![Note {
                mark: Mark::Unknown,
                text: "Pick a key and an app.".into(),
            }],
        );
    }

    let mut notes: Vec<Note> = Vec::new();
    // Every status word this row earns, in precedence order because that is
    // the order they are pushed in. The CELL shows the first; the severity
    // fold at the bottom sees all of them.
    let mut conditions: Vec<&'static str> = Vec::new();
    let combo = Combo::parse(&r.combo);

    // 1. The key.
    if rt.paused {
        // No note. Pausing is true of the WINDOW, and `paused` already lands
        // on every row at once -- a sentence per row is the noisiest possible
        // place to say a thing that is not about any one of them.
        //
        // **What is meant to carry it instead does not exist yet.** Design
        // 6.4's service line (`Serving · 18 of 19` / `Paused`) is on all four
        // pages and is unbuilt, so until it lands the whole of what a paused
        // beckon says is the word on each row. `probe_notes` still appends
        // its own paused sentence, but that one is about how far to trust a
        // verdict and only appears while a probe is showing.
        conditions.push("paused");
    } else if let Ok(c) = &combo {
        match rt.registered.get(&c.canonical()) {
            // Healthy. Nothing to say -- design 3.1 deletes
            // `Registered and working.` by name, and rule 2 is that silence
            // is the report.
            Some(Ok(())) => {}
            Some(Err(_)) => {
                conditions.push("in use");
                // The mock-up's sentence, verbatim. The second half is why
                // this note survived the cut while three others did not: the
                // word `in use` says a program has the chord, and only the
                // sentence says beckon can never tell you which -- which is
                // the difference between a user who picks another key and a
                // user who goes hunting for a culprit no API will name.
                notes.push(Note {
                    mark: Mark::Bad,
                    text: "Another program owns this key. Windows will not say which.".into(),
                });
            }
            // In the file but not in the last registration pass -- an edit
            // that has not been saved and reloaded yet. Honest, not a fault,
            // and no status word claims it: this is "beckon does not know",
            // which is not a condition of the row.
            None => notes.push(Note {
                mark: Mark::Unknown,
                text: "Not registered yet.".into(),
            }),
        }
    }
    // A combo that does not parse says so through its `Problem` below;
    // repeating it here would put the same sentence on screen twice.

    // 1b. The availability probe. Unlike everything above, this is about the
    //     chord as it stands in the editor RIGHT NOW rather than about the
    //     last registration pass -- so it appears on the row being edited
    //     and nowhere else, and only while it is still about that row's
    //     chord. `ProbeResult` carries the chord it was about for exactly
    //     this reason: the user types on, the verdict goes stale, and a
    //     stale verdict has to vanish rather than be shown against the chord
    //     that replaced it. `same_chord` refuses on either side failing to
    //     parse, so a half-typed chord shows no verdict at all.
    //
    //     No `flag` is claimed. The flag is the list column's one word about
    //     every row; a probe is about the one row being edited, and a word
    //     that appeared and vanished as the selection moved would be worse
    //     than none. `mark` still follows, because it is derived from the
    //     notes at the end.
    let probe = rt
        .probe
        .as_ref()
        .filter(|p| m.selected == Some(i) && same_chord(&p.combo, &r.combo));
    if let Some(p) = probe {
        notes.extend(probe_notes(p, rt.paused));
    }

    // 2. The app. Silent on success -- a healthy row says nothing.
    match &rt.catalog {
        // A scan that has not finished cannot prove absence.
        None => notes.push(Note {
            mark: Mark::Unknown,
            text: "Checking installed apps...".into(),
        }),
        Some(names) => {
            let want = r.app.trim().to_lowercase();
            // An empty app name is reported as a `Problem`, not as a
            // catalog miss -- "no installed app has this name" is a strange
            // thing to say about no name at all.
            if !want.is_empty() && !names.iter().any(|n| n.to_lowercase() == want) {
                // **Not a miss yet.** Every beckon resolver ends in a
                // case-insensitive substring tier, and `check --resolve`
                // passes it deliberately -- a `Certainty::Guess` prints and
                // exits 0. Comparing for equality here made the window say
                // `missing` about bindings the resolver resolves, which is
                // one program answering a question two ways. Measured on
                // airm3 2026-08-16: `Settings` and `DeepSeek` came back
                // `MISSING` from this catalog while `check --resolve` exited
                // 0 for both.
                let loose: Vec<&String> = names
                    .iter()
                    .filter(|n| n.to_lowercase().contains(&want))
                    .collect();
                match loose.as_slice() {
                    // No note: `missing` is the sentence, in one word, on the
                    // row the user is already looking at. `flag_mark` keeps
                    // the `Mark::Bad` the deleted note used to carry -- and it
                    // is PUSHED rather than inserted-if-absent, so a paused
                    // row is still `Bad` for an app that is not installed even
                    // though the cell can only say `paused`.
                    [] => conditions.push("missing"),
                    // The two hazards `check --resolve` distinguishes, in its
                    // own words. One candidate is a name a later install can
                    // take; several means the winner is already decided by
                    // sort order rather than by anything the user wrote.
                    //
                    // A note rather than a fifth status word: design 3.1 fixes
                    // the vocabulary at four and lists that under *what must
                    // never be cut*. The severity still reaches the row,
                    // because `mark` folds the notes.
                    [one] => notes.push(Note {
                        mark: Mark::Warn,
                        text: format!(
                            "Matches \"{one}\" by substring, so an app installed \
                             later can quietly take this name."
                        ),
                    }),
                    many => notes.push(Note {
                        mark: Mark::Warn,
                        text: format!(
                            "Matches {} installed apps by substring; \"{}\" wins \
                             only because it sorts first.",
                            many.len(),
                            many[0]
                        ),
                    }),
                }
            }
        }
    }

    // 3. Reachable by holding Caps? Compared against `keyboard.caps_hold`
    //    unconditionally -- NOT gated on `keyboard.caps`. A gate was tried
    //    and reverted: the spec names this flag with no such qualifier, and
    //    the README's own justifying example,
    //    `"ctrl+super+alt+shift+t" = "Telegram Web"`, ships with no
    //    `keyboard.caps` block at all -- so a gate would silently unflag
    //    the one case the spec cites as the reason `custom` exists. It also
    //    reinstates the coupling between the list's appearance and a
    //    keyboard setting three sections away that was deliberately removed
    //    elsewhere (the keycap-dimming rule).
    if let Ok(c) = &combo {
        if !combo_is_caps_chord(c, &m.keyboard.caps_hold) {
            // No note, for the same reason as `missing`: `other chord` is the
            // sentence. The word was `custom`, which named a property of the
            // binding; `other chord` names what the reader can see about it.
            conditions.push("other chord");
        }
    }

    // 4. Whatever stops the file being written, verbatim from the validator.
    for p in problems.iter().filter(|p| p.row == Some(i)) {
        notes.push(Note {
            mark: match p.severity {
                Severity::Error => Mark::Bad,
                Severity::Warning => Mark::Warn,
            },
            text: p.message.clone(),
        });
    }

    // The worst thing said anywhere about this row -- by a note, or by any
    // condition on its own now that three of the four words carry no note.
    // Conditions are folded in as extra voices rather than consulted first,
    // because a `Problem` from the validator can be worse than any of them and
    // an unfinished row is `Unknown` with no condition at all.
    //
    // **All of them, not `flag`.** The cell shows one word; the row can have
    // earned several, and a word that lost the cell did not stop being true --
    // see this function's own correction note.
    let worst =
        |m: Mark| notes.iter().any(|n| n.mark == m) || conditions.iter().any(|c| flag_mark(c) == m);
    let mark = if worst(Mark::Bad) {
        Mark::Bad
    } else if worst(Mark::Warn) {
        Mark::Warn
    } else if worst(Mark::Unknown) {
        Mark::Unknown
    } else {
        Mark::Ok
    };
    (mark, conditions.first().map(|c| (*c).to_string()), notes)
}

pub fn control_state(m: &Model, rt: &RuntimeStatus) -> ControlState {
    let problems = m.problems();

    let vis = m.visible();
    // Counted here rather than through `Model::marked_count`, which would
    // rebuild `vis` a second and third time -- and this runs on every
    // keystroke now that a filter box feeds it.
    let marked_count = vis.iter().filter(|&&i| m.rows[i].marked).count();
    let mut items = Vec::with_capacity(vis.len());
    let mut detail = None;
    // The VIEW index of the selected row, which is what the ListView needs
    // in order to put `LVIS_SELECTED` back after a rebuild -- see
    // `ControlState::selected`.
    let mut selected = None;
    for (pos, &i) in vis.iter().enumerate() {
        let r = &m.rows[i];
        let (mark, flag, notes) = row_condition(m, i, rt, &problems);
        items.push(ListItem {
            combo: r.combo.clone(),
            app: r.app.clone(),
            mark,
            flag,
            marked: r.marked,
            row: i,
        });
        // Same call, same answer: the editor cannot say something the list
        // does not.
        if m.selected == Some(i) {
            selected = Some(pos);
            detail = Some(Detail {
                combo: r.combo.clone(),
                app: r.app.clone(),
                notes,
            });
        }
    }

    ControlState {
        items,
        // `m.rows`, never `items.len()` -- see the field's own doc. `vis` is
        // already computed above and is the filtered set; this is the file.
        binding_count: m.rows.len(),
        service: service_line(
            true,
            rt.paused,
            // Over `m.rows` and not over the map's values, so the numerator
            // is bounded by the denominator beside it -- see `service_line`'s
            // own doc for the removal direction that closes.
            m.rows
                .iter()
                .filter(|r| {
                    Combo::parse(&r.combo)
                        .is_ok_and(|c| matches!(rt.registered.get(&c.canonical()), Some(Ok(()))))
                })
                .count(),
            m.rows.len(),
        ),
        selected,
        detail,
        filter: m.filter().to_string(),
        caps_checked: m.keyboard.caps,
        caps_tap: m.keyboard.caps_tap,
        caps_hold: m.keyboard.caps_hold,
        dirty: m.dirty(),
        apply_enabled: m.dirty() && !problems.iter().any(|p| p.severity == Severity::Error),
        // Either gesture arms the button, because either gesture is one
        // `remove_pressed` acts on -- and both are scoped to what is on
        // screen, so an armed Remove always has something visible to take.
        remove_enabled: selected.is_some() || marked_count > 0,
        marked_count,
        // There is a `Model`, therefore the file parsed, therefore it can be
        // edited. The only `false` in the program is `unreadable_state`.
        editable: true,
    }
}

// ---------------------------------------------------------------------------
// The default button
// ---------------------------------------------------------------------------

/// A push button the default ring -- the one Enter presses -- can sit on.
///
/// The window keeps the ring's position in `Ui::defid` as a control id and
/// migrates it as focus moves. That much is Win32; **which button is a legal
/// place for it to stop is not**, and it lives here so it can be tested on
/// all three CI jobs rather than on the one that compiles a wndproc.
///
/// The set is `settings_window::PUSH_BUTTONS`, in the same order. It is not
/// derived from anything -- a check box cannot wear a default ring and the
/// two field labels are not buttons at all -- so the two lists are kept in
/// step by `default_button_of` / `id_of_default_button`, which are total in
/// both directions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DefaultButton {
    Save,
    Add,
    Remove,
    OpenFile,
    Close,
    /// Reload and Keep mine live in the external-change banner, which is
    /// HIDDEN unless the file changed underneath an unsaved window. They are
    /// the only two members of this set that can be off screen, and they are
    /// the whole reason this module exists -- see `default_button`.
    Reload,
    KeepMine,
    /// The editor strip's two. They are here for one reason: a push button
    /// that is not in this set never gets `BS_NOTIFY`, so the ring cannot
    /// follow focus onto it -- and `IsDialogMessageW` then asks
    /// `DM_GETDEFID`, which still says Save. Enter on a focused `Record`
    /// would SAVE. That is the defect this whole module was written for,
    /// respelled one band higher.
    Record,
    /// Was `Reset` until 2026-08-15, in the enum as well as on the button:
    /// design 3.1 renames the caption to name the effect rather than the
    /// mechanism, and a variant still called `Reset` would leave the window's
    /// own vocabulary two words for one control. The id NUMBER did not move --
    /// `IDC_REVERT` is still 1033 -- because that is what a probe reads.
    Revert,
    /// The System page's five, all added the day design §3.3's rows were
    /// built. They are here for the reason `Record` is, restated one page
    /// across: a push button outside this set gets no `BS_NOTIFY`, so the
    /// ring cannot follow focus onto it, so `IsDialogMessageW` asks
    /// `DM_GETDEFID` and is told `Save` -- and Enter on a focused
    /// `Open config file` glyph would write the config file.
    SysReload,
    ConfigOpen,
    ConfigShow,
    LogOpen,
    LogShow,
    /// The About page's six, all added the day design §3.4's rows were built,
    /// and here for the reason `SysReload` and its four are: a push button
    /// outside this set gets no `BS_NOTIFY`, so the ring cannot follow focus
    /// onto it, so `IsDialogMessageW` asks `DM_GETDEFID` and is told `Save`.
    /// Enter on a focused `Report a bug` would have written the config file.
    ///
    /// **A copy button is not a harmless place for the ring to be wrong
    /// either**, which is worth saying because three of these six look like
    /// ornaments: pressing one silently replaces whatever the user had on the
    /// clipboard, which is a loss they will not see until they paste.
    AboutBuildCopy,
    AboutLocationCopy,
    AboutLicenceCopy,
    AboutGithub,
    AboutReleases,
    AboutBug,
}

impl DefaultButton {
    /// Every variant, for exhaustive tests. A new button added to the enum
    /// and forgotten here weakens those tests silently, so the array is
    /// length-annotated: adding a variant without extending it fails to
    /// compile.
    pub const ALL: [DefaultButton; 20] = [
        DefaultButton::Save,
        DefaultButton::Add,
        DefaultButton::Remove,
        DefaultButton::OpenFile,
        DefaultButton::Close,
        DefaultButton::Reload,
        DefaultButton::KeepMine,
        DefaultButton::Record,
        DefaultButton::Revert,
        DefaultButton::SysReload,
        DefaultButton::ConfigOpen,
        DefaultButton::ConfigShow,
        DefaultButton::LogOpen,
        DefaultButton::LogShow,
        DefaultButton::AboutBuildCopy,
        DefaultButton::AboutLocationCopy,
        DefaultButton::AboutLicenceCopy,
        DefaultButton::AboutGithub,
        DefaultButton::AboutReleases,
        DefaultButton::AboutBug,
    ];

    /// Where the ring rests on `page`, and the button `default_button` falls
    /// back to there. It is what `DM_GETDEFID` answers before focus has ever
    /// touched a button.
    ///
    /// **It was `const HOME: DefaultButton = Save` until 2026-08-15**, and
    /// `default_button`'s doc named the exact condition that would end it:
    /// *"`HOME`'s exemption survives that unchanged, because `Save` is on every
    /// page -- if it ever stops being, this early return is the line that
    /// breaks."* `Page::writes_config` is that change, so the constant becomes
    /// a function of the door and the early return became a comparison against
    /// its answer.
    ///
    /// **`None` is a real answer, not a missing one.** System and About have no
    /// primary action: their eleven buttons are five file/service verbs and six
    /// copies and links, and promoting any one of them to the Enter key would
    /// be inventing a default the design never asked for. Enter on those doors
    /// does nothing until the user tabs onto a button, at which point
    /// `BN_SETFOCUS` moves the ring there -- which is the same rule the other
    /// two doors follow, minus a resting place.
    pub fn home(page: Page) -> Option<DefaultButton> {
        command_bar_shown(page).then_some(DefaultButton::Save)
    }

    /// Is this button on screen in the state described?
    ///
    /// **Two conditions now, not one.** The sentence that stood here read
    /// "`external_change` is the banner's visibility, which is the window's
    /// only conditional geometry: everything else in the command bar is
    /// created once and never hidden". The first half is still true and the
    /// second stopped being true the moment the window grew four doors: SIX
    /// of these nine live on the Shortcuts page and are HIDDEN behind any of
    /// the other three, and only the command bar's own three are on every
    /// page. Four of the six are unconditional there; the banner's two are
    /// conditional twice over -- `banner_shown` is the AND of both, and is
    /// where that pair is spelled once.
    ///
    /// This is the payoff for `Page` living in core: which button is on
    /// screen is now a decision the two CI jobs that never compile a wndproc
    /// can check, and the window applies it rather than restating it.
    pub fn visible(self, external_change: bool, page: Page) -> bool {
        match self {
            // **The command bar stopped being chrome on 2026-08-15.** This arm
            // read `=> true`, with a comment saying the three "are created
            // once, placed by `layout` on every page, and never hidden" and
            // that "`Save` in particular MUST stay on all four -- it is
            // `HOME`". Both halves went together: `HOME` is now
            // `home(page)`, and the three follow design §1's store split like
            // every other button on the page that owns them.
            //
            // `Close` and `Open config file` go with `Save` rather than
            // staying: neither writes the file, but both are the SAVE
            // gesture's other half -- `Close` is where "discard" lives (the
            // dirty prompt is in `WM_CLOSE`) and `Open config file` is a
            // second route to the file the System page already lists with its
            // own two glyphs. A bar holding one orphaned button under a page
            // that cannot save is not a smaller command bar, it is a stranded
            // one.
            //
            // **No door loses its way out**, by two routes rather than one:
            // the client-drawn caption's `X` is chrome and is on all four, and
            // Escape reaches `handle_command`'s `IDCANCEL` arm, which shares
            // its body with `IDC_CLOSE` -- the dialog manager sends that id,
            // not the button, so hiding the button does not disarm the key.
            DefaultButton::Save | DefaultButton::OpenFile | DefaultButton::Close => {
                command_bar_shown(page)
            }
            DefaultButton::Reload | DefaultButton::KeepMine => banner_shown(external_change, page),
            // Shortcuts-page controls. `Add` and `Remove` sit on the list's
            // head row; `Record` and `Revert` close the editor strip's second
            // line.
            DefaultButton::Add
            | DefaultButton::Remove
            | DefaultButton::Record
            | DefaultButton::Revert => page == Page::Shortcuts,
            // System-page controls, and the page is the WHOLE condition even
            // though two of the five can be hidden while it is open.
            //
            // **`LogOpen` / `LogShow` are hidden exactly when `Paths::log` is
            // `None`, which is fixed for the window's lifetime** -- `serve`
            // is started with `--log` or it is not, and nothing repoints it
            // while the window is up. So a hidden log button was hidden from
            // the moment it was created and has never held focus; the ring
            // reaches a button only through its own `BN_SETFOCUS`, so
            // `current` can never BE one of those two while they are hidden,
            // and this arm's answer for them is unreachable rather than
            // wrong. Modelling it properly would mean threading the log's
            // presence through `visible`, `pressable` and `default_button`
            // for a state no keystroke can produce.
            DefaultButton::SysReload
            | DefaultButton::ConfigOpen
            | DefaultButton::ConfigShow
            | DefaultButton::LogOpen
            | DefaultButton::LogShow => page == Page::System,
            // About-page controls, and the page is the WHOLE condition with
            // nothing like the log row's exception: every one of the six is
            // created once and shown whenever that door is open. The page has
            // no conditional row at all -- the mark, the three values and the
            // three links are true of every machine beckon runs on.
            DefaultButton::AboutBuildCopy
            | DefaultButton::AboutLocationCopy
            | DefaultButton::AboutLicenceCopy
            | DefaultButton::AboutGithub
            | DefaultButton::AboutReleases
            | DefaultButton::AboutBug => page == Page::About,
        }
    }

    /// Would pressing it do anything -- on screen AND enabled?
    ///
    /// Each arm reads the same `ControlState` field the window's own `enable`
    /// call uses, so the two cannot drift: if this says a button is pressable
    /// and the window greys it out, one of the two is reading a different
    /// field and the disagreement is a one-line diff.
    ///
    /// **`page` reaches the enablement arms through `visible` and nowhere
    /// else, and that is deliberate.** Being behind another door is not the
    /// same as being greyed: the window does not `enable(false)` a control it
    /// hides, so `st` says nothing about the page and the arms below must not
    /// pretend it does.
    pub fn pressable(self, st: &ControlState, external_change: bool, page: Page) -> bool {
        self.visible(external_change, page)
            && match self {
                DefaultButton::Save => st.apply_enabled,
                DefaultButton::Add => st.editable,
                DefaultButton::Remove => st.remove_enabled,
                // Both act on the row the editor strip is showing, so both
                // read the same pair the window's own `enable` call reads --
                // exactly as the comment above promises.
                //
                // **Neither knows a capture is armed, and that is not a
                // hole.** While armed the window greys `Revert` (two writers
                // on one value is what §C.4 forbids) while this still calls
                // it pressable, so for those seconds the two disagree. It
                // cannot be observed: the `WH_KEYBOARD_LL` hook swallows
                // every keystroke while a capture is armed and the window is
                // foreground, so no Enter reaches the dialog manager to ask;
                // and if the window is NOT foreground, all three of spec
                // F.4's focus layers have already disarmed. Modelling it here
                // would mean `ControlState` carrying a runtime fact that
                // exists for seconds at a time.
                DefaultButton::Record | DefaultButton::Revert => st.editable && st.detail.is_some(),
                // The two escape routes are enabled in every state,
                // including read only -- that is what makes them escapes.
                // The banner's two answers are enabled whenever the banner is
                // up, which `visible` above has already established.
                DefaultButton::OpenFile
                | DefaultButton::Close
                | DefaultButton::Reload
                | DefaultButton::KeepMine => true,
                // The System page is deliberately NOT gated on
                // `st.editable`, and that is design §1's split by store
                // rather than an oversight: `editable` is false exactly when
                // `apps.toml` did not parse, and none of these five touches
                // that file. Reloading, opening the config in an editor and
                // showing it in Explorer are the three things a user whose
                // config is broken most needs, and greying them out is what
                // the split exists to stop.
                DefaultButton::SysReload
                | DefaultButton::ConfigOpen
                | DefaultButton::ConfigShow
                | DefaultButton::LogOpen
                | DefaultButton::LogShow => true,
                // The About page is not gated on `st.editable` either, and
                // for a stronger version of the System page's reason: not one
                // of these six reads `apps.toml` at all. `Report a bug` in
                // particular is the button a user whose config will not parse
                // is most likely to want, and greying it out because the
                // config will not parse is a joke the window should not make.
                DefaultButton::AboutBuildCopy
                | DefaultButton::AboutLocationCopy
                | DefaultButton::AboutLicenceCopy
                | DefaultButton::AboutGithub
                | DefaultButton::AboutReleases
                | DefaultButton::AboutBug => true,
            }
    }
}

/// Where the default ring belongs, given where it is now.
///
/// **The defect this exists for, measured on a14 2026-08-11:**
/// `ShowWindow(SW_HIDE)` raises no `BN_KILLFOCUS`, so the window's own
/// focus-driven migration never fires when the external-change banner is
/// dismissed -- and `DM_GETDEFID` was left naming `Reload`, a button no
/// longer on screen. Enter then pressed it, discarding the user's edits from
/// a control they could not see. Reachable with the mouse alone: the banner
/// appears on its own when the file changes, the user clicks `Reload`, and
/// the next Enter reloads again.
///
/// The invariant: **the ring never stops on a button that is not visible,
/// and never on a disabled button other than `HOME`.**
///
/// `HOME`'s exemption is deliberate and is not a hole. Save is on screen in
/// every state, and the dialog manager does not dispatch a command to a
/// disabled control -- so a disabled Save means Enter does nothing, which is
/// exactly right when there is nothing to save. Chasing Save's enabled state
/// as well would hand the ring to Close in a clean model, so Enter would mean
/// "close the window" until the first keystroke and "save" after it: the
/// meaning of a key changing under the user, which is worse than an inert
/// one.
///
/// **A tab switch reaches the same defect by another route**, which is why
/// `page` is here: hiding a page's controls raises no focus notification
/// either, so a ring left on `Add` while the user is behind the Keyboard
/// door makes Enter add a row the user cannot see.
///
/// **`HOME` became `home(page)` on 2026-08-15, and the paragraph that stood
/// here predicted it.** It read: *"`HOME`'s exemption survives that unchanged,
/// because `Save` is on every page -- if it ever stops being, this early return
/// is the line that breaks."* Design §1's store split is that change, so the
/// early return now compares against the door's own answer instead of a
/// constant. The exemption itself is unchanged in meaning: where a page HAS a
/// resting place, the ring may sit there disabled, for the reason above.
///
/// **`None` in and `None` out are different facts and both are real.** `None`
/// in is "the ring is nowhere" -- the state System and About rest in. `None`
/// out is "it belongs nowhere on this door". They coincide on those two doors
/// and must not be collapsed: `Some(Save)` arriving from a door change with
/// `page == System` has to come back `None`, which is the whole repair.
pub fn default_button(
    current: Option<DefaultButton>,
    st: &ControlState,
    external_change: bool,
    page: Page,
) -> Option<DefaultButton> {
    let home = DefaultButton::home(page);
    match current {
        // The exemption: at home, stay home, without consulting enablement.
        c if c == home => home,
        Some(c) if c.pressable(st, external_change, page) => Some(c),
        _ => home,
    }
}

// ---------------------------------------------------------------------------
// The file did not parse
// ---------------------------------------------------------------------------

/// What the window draws when the config file could not be read.
///
/// `Model::from_text` returned `Err`, so there is no model to project a
/// `ControlState` out of -- this IS the projection, and it is a function
/// rather than a `Default` so that the one thing it varies, the explanation,
/// has to be supplied.
///
/// The explanation rides in `detail.notes`, which is the notes strip's only
/// input: `Detail` is "what the editor strip is showing", and here it is
/// showing nothing at all, with the notes saying why. Every enable flag is
/// off, `items` is empty, and `dirty` is false -- so Save is greyed and,
/// just as importantly, dismissing the window cannot raise a save prompt for
/// changes that do not exist.
pub fn unreadable_state(notes: Vec<Note>) -> ControlState {
    ControlState {
        items: Vec::new(),
        // There is no model, so there is no count. The badge draws `0`, which
        // is true of a file beckon cannot read: it has no bindings beckon can
        // act on.
        binding_count: 0,
        selected: None,
        detail: Some(Detail {
            combo: String::new(),
            app: String::new(),
            notes,
        }),
        filter: String::new(),
        caps_checked: false,
        caps_tap: CapsTap::default(),
        caps_hold: Chord::default(),
        dirty: false,
        apply_enabled: false,
        remove_enabled: false,
        marked_count: 0,
        editable: false,
        // The one state where the phrase is not about counts at all: there is
        // no model to count. `service_line`'s first branch is exactly this,
        // and it outranks a pause for the reason stated there.
        service: service_line(false, false, 0, 0),
    }
}

/// Longest slice of the user's own file quoted back at them. The notes
/// STATIC wraps, so a long line costs vertical room rather than being
/// clipped, but a 4 KB minified line would still push everything under it
/// off the band.
const QUOTED_LINE_MAX: usize = 120;

/// Say, to someone who has never seen TOML, what is wrong with their file.
///
/// `text` is the file as read and `err` is what `Model::from_text` said
/// about it. Both are needed: the error names a line NUMBER at best, and the
/// line itself is quoted out of `text` rather than scraped out of the
/// error's own snippet, which is ASCII art that only lines up in a monospace
/// font this window does not have.
///
/// **Every string beckon contributes here is ASCII**, because this window
/// carries a text face (not a symbol one) and a glyph it lacks draws as a box
/// that reads like a rendering bug. The two pass-through fragments -- the
/// offending line and the parser's reason -- are not folded: the line is the
/// user's own data and mangling it would defeat the purpose of quoting it, and
/// every message `parse_config` produces is ASCII by the rule stated at its
/// array arm.
pub fn explain_unreadable(text: &str, err: &str) -> Vec<Note> {
    let mut out = vec![Note {
        mark: Mark::Bad,
        text: "beckon cannot read this file, so it is open read only and \
               nothing here can be changed."
            .into(),
    }];
    out.push(Note {
        mark: Mark::Bad,
        text: location_note(text, err),
    });
    out.push(Note {
        mark: Mark::Bad,
        text: format!("What went wrong: {}", error_reason(err)),
    });
    out.push(Note {
        mark: Mark::Warn,
        // Precisely what happens, and no more: `serve` re-reads on the
        // watcher and turns the window editable the moment the file parses
        // (`settings_retry_unreadable`). It does NOT promise the explanation
        // above updates on every keystroke of an external edit.
        text: "Press Open config file to fix it in a text editor. This window \
               turns editable on its own as soon as the file reads."
            .into(),
    });
    out.push(Note {
        mark: Mark::Warn,
        text: "beckon never writes over a file it cannot read.".into(),
    });
    out
}

/// Where in the file to look, said only as precisely as the error allows.
///
/// **The `None` arm is the requirement, not a fallback.** Only the TOML
/// *syntax* errors carry a location; beckon's own checks -- a duplicate
/// combo, an unknown `keyboard.` setting, a value that is not a string --
/// are about a key, not a place. Pointing at a line beckon had to guess
/// would send someone to the wrong part of their file, so it says it does
/// not know.
fn location_note(text: &str, err: &str) -> String {
    let Some(n) = error_line(err) else {
        return "The error does not say which line.".into();
    };
    // Empty covers two cases, and neither is worth its own sentence: the
    // line is blank, or it is one past the end of the file, which is where a
    // TOML parser points an unexpected EOF.
    let line = text.lines().nth(n - 1).map(str::trim).unwrap_or("");
    if line.is_empty() {
        format!("The problem is on line {n}.")
    } else {
        format!("The problem is on line {n}: {}", clip(line))
    }
}

fn clip(line: &str) -> String {
    if line.chars().count() <= QUOTED_LINE_MAX {
        return line.to_string();
    }
    let head: String = line.chars().take(QUOTED_LINE_MAX).collect();
    format!("{head}...")
}

/// The 1-based line a `parse_config` error points at, when it points at one.
///
/// Measured against toml 0.8.23, not assumed: a TOML *syntax* error is
/// `toml::de::Error`'s own rendering and its first line reads
/// `TOML parse error at line 2, column 5`. Every other error `parse_config`
/// produces is one of beckon's own, is a single line, and names no location
/// at all -- so this returns `Option` and `explain_unreadable` says which
/// case it is in rather than inventing a number.
///
/// Only the FIRST line is searched, so a parser reason that happens to
/// contain the words "at line" cannot be mistaken for a location.
fn error_line(err: &str) -> Option<usize> {
    let head = err.lines().next()?;
    let rest = head.split_once("at line ")?.1;
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    digits.parse().ok().filter(|n| *n > 0)
}

/// The parser's own reason for refusing, on one line.
///
/// `toml::de::Error` renders as a header, a three-line caret snippet, and
/// then the reason:
///
/// ```text
/// TOML parse error at line 2, column 5
///   |
/// 2 | oops
///   |     ^
/// expected `.`, `=`
/// ```
///
/// The snippet is dropped -- it lines up only in a monospace font, and the
/// window has none -- so a multi-line error is reduced to its last non-empty
/// line and a single-line error is its own reason. If that last line is part
/// of the snippet rather than a reason (a rendering with no trailing
/// explanation), the header is used instead: a caret drawn in a proportional
/// font is worse than no detail at all.
fn error_reason(err: &str) -> String {
    let mut lines = err.lines().map(str::trim).filter(|l| !l.is_empty());
    let head = lines.next().unwrap_or("");
    match lines.next_back() {
        Some(last) if !is_snippet(last) => last.to_string(),
        _ => head.to_string(),
    }
}

/// Is this line part of a rendered source snippet rather than prose? The
/// gutter, the caret row and the quoted source all begin with a digit or a
/// pipe; no reason `parse_config` can produce does.
fn is_snippet(line: &str) -> bool {
    line.starts_with('|') || line.chars().next().is_some_and(|c| c.is_ascii_digit())
}

// ---------------------------------------------------------------------------
// Control ids
// ---------------------------------------------------------------------------

/// Every dialog control id the settings window uses, on every platform that
/// has one, in one place.
///
/// **This is documentation with a test attached, not the definition.** The
/// Windows module keeps its own `const IDC_… : i32`, and a `#[test]` there
/// compares the two. Making this the definition would put a Win32 concept in
/// the crate whose whole purpose is to be free of one.
///
/// It lives here so `ids_are_unique` and `retired_ids_stay_retired` run on
/// **all three** CI jobs rather than only the Windows one -- the same reason
/// `ControlState` and `DefaultButton` are here. The failure it guards is not
/// hypothetical: `layout` resolves controls through `GetDlgItem`, which maps
/// a duplicated id to the first match, so the second control is created,
/// never placed, and left at the origin. That shipped once already (three
/// labels sharing `-1`), and two drafts of the Four Doors design each claimed
/// 1060-1069 for a different page.
///
/// **Ranges are disjoint by page**, and a page's controls never appear on
/// another page:
///
/// | Range | Owner |
/// |---|---|
/// | 1001-1039 | the pre-Four-Doors window |
/// | 1040-1049 | shell: the tab strip and the command bar |
/// | 1050-1059 | Shortcuts (reserved; the page reuses its existing ids) |
/// | 1060-1069 | Keyboard |
/// | 1070-1099 | System |
/// | 1100-1119 | About |
pub const CONTROL_IDS: &[(&str, i32)] = &[
    // -- the pre-Four-Doors window -----------------------------------------
    ("LIST", 1001),
    ("COMBO", 1002),
    ("APP", 1003),
    ("NOTES", 1004),
    ("ADD", 1005),
    ("REMOVE", 1006),
    // `APPLY` and `CLOSE` keep their ids after auto-save deletes both
    // buttons: the probe hard-codes them, and a different control answering
    // 1007 would make it report a confident wrong result.
    ("APPLY", 1007),
    ("CAPS", 1008),
    ("OPENFILE", 1012),
    ("CLOSE", 1013),
    ("BANNER", 1014),
    ("RELOAD", 1015),
    ("KEEPMINE", 1016),
    // 1017 and 1018 were `LBL_SHORTCUT` and `LBL_APP`, the editor's two field
    // labels. Retired, not free -- see `RETIRED_IDS`.
    // 1020 was `LBL_SECTION`, the `Shortcuts` heading at the top of the
    // Shortcuts card. Retired, not free -- see `RETIRED_IDS`.
    ("FILTER", 1021),
    ("HOLD_CTRL", 1022),
    ("HOLD_WIN", 1023),
    ("HOLD_ALT", 1024),
    ("TAP", 1025),
    ("LBL_HOLD", 1026),
    ("LBL_TAP", 1027),
    ("MOD_CTRL", 1028),
    ("MOD_WIN", 1029),
    ("MOD_ALT", 1030),
    ("MOD_SHIFT", 1031),
    ("RECORD", 1032),
    ("REVERT", 1033),
    // 1034 was `GRP_EDITOR`, the editor card's `Editing "…"` caption, and 1035
    // was `LBL_COUNT`, the `· 18 bindings` beside the Shortcuts heading.
    // Retired, not free -- see `RETIRED_IDS`.
    // -- shell: the tab strip and the command bar --------------------------
    ("TAB_SHORTCUTS", 1040),
    ("TAB_KEYBOARD", 1041),
    ("TAB_SYSTEM", 1042),
    ("TAB_ABOUT", 1043),
    ("SERVICE_LINE", 1044),
    ("SAVED", 1045),
    ("UNDO", 1046),
    // -- Keyboard ----------------------------------------------------------
    ("CAPS_SHORTHAND", 1060),
    ("TROUBLE_HEAD", 1061),
    ("TROUBLE_BODY", 1062),
    // -- System ------------------------------------------------------------
    ("PAUSE", 1070),
    ("AUTOSTART", 1071),
    // NOT `RELOAD` (1015): that is the banner's "reload from disk", which
    // answers a different question from the tray's own reload.
    ("SYS_RELOAD", 1072),
    ("DARK", 1073),
    ("OPACITY", 1074),
    ("OPACITY_VALUE", 1075),
    ("CONFIG_NAME", 1076),
    ("CONFIG_DIR", 1077),
    ("CONFIG_OPEN", 1078),
    ("CONFIG_SHOW", 1079),
    ("LOG_NAME", 1080),
    ("LOG_SIZE", 1081),
    ("LOG_OPEN", 1082),
    ("LOG_SHOW", 1083),
    // 1084 was `SYS_PLACEHOLDER`, the `Nothing here yet.` line this page
    // showed between Task 7 and the day the fourteen rows above were built.
    // Retired, not free -- see `RETIRED_IDS`, and note that the tail it came
    // out of (1084-1099) did its job: the fourteen numbers above have no hole
    // in them.
    // -- About -------------------------------------------------------------
    ("ABOUT_MARK", 1100),
    ("ABOUT_NAME", 1101),
    ("ABOUT_BUILD_LABEL", 1102),
    ("ABOUT_BUILD_VALUE", 1103),
    ("ABOUT_BUILD_COPY", 1104),
    ("ABOUT_LOCATION_LABEL", 1105),
    ("ABOUT_LOCATION_VALUE", 1106),
    ("ABOUT_LOCATION_COPY", 1107),
    ("ABOUT_LICENCE_LABEL", 1108),
    ("ABOUT_LICENCE_VALUE", 1109),
    ("ABOUT_LICENCE_COPY", 1110),
    ("ABOUT_DISCLOSURE", 1111),
    ("ABOUT_GITHUB", 1112),
    ("ABOUT_RELEASES", 1113),
    ("ABOUT_BUG", 1114),
    // 1115 was `ABOUT_PLACEHOLDER`, this page's `Nothing here yet.` line,
    // deleted the day design §3.4's rows were built. Retired, not free -- see
    // `RETIRED_IDS`, and note that the tail it came out of (1115-1119) did the
    // same job `SYS_PLACEHOLDER`'s did: the fifteen numbers above have no hole
    // in them.
];

/// Ids that were used, are not any more, and must never be reused.
///
/// 1009-1011 were the three `Tapping Caps alone` radios. A probe built
/// against an older binary would find a control it thinks it recognises.
///
/// **1035 was `LBL_COUNT`**, the `· 18 bindings` STATIC beside the Shortcuts
/// heading, deleted 2026-08-15. Design 2 moved the count onto the Shortcuts
/// pill so it reads from all four doors, and the photograph of the shipped
/// window shows the number in both places at once -- which is the state that
/// move existed to END, not a stage on the way to it. The pill's badge is
/// `ControlState::binding_count` and counts the FILE; this one counted the
/// filtered list, so they could also disagree while both were right.
///
/// It is retired rather than deleted outright, and the distinction is the
/// point: `retired_ids_stay_retired` fails if anything reclaims 1035, so a
/// later page cannot quietly take a number that a `settings_probe` built
/// against today's binary is still looking for. Deleting the row from
/// `CONTROL_IDS` alone would have left 1035 looking free to every reader and
/// every test.
///
/// **1017, 1018 and 1034 went on 2026-08-15**, all three for design §3.1's
/// "no `Editing "…"` caption, no field labels": 1017 `LBL_SHORTCUT`, 1018
/// `LBL_APP`, 1034 `GRP_EDITOR`. The App combo's cue banner says `App` while
/// it is empty and the key list sits at the end of the modifier run, so the
/// two words were each naming a control that already says what it is; the
/// caption named a row the list above it already highlights.
///
/// **They are the first RETIRED ids that give geometry back rather than only
/// a number**, which is why the entry says so here as well as in `layout.rs`:
/// the caption was the `s(24)` line inside `grp_content_h` and the labels were
/// the editor's whole label column. A later page reclaiming one of these three
/// would not merely confuse a probe, it would collide with a control whose
/// absence the vertical arithmetic now depends on.
///
/// **1020 was `LBL_SECTION`**, the `Shortcuts` heading at the top of the
/// Shortcuts card, deleted 2026-08-15 for the same reason as 1035 and by the
/// same reading of the same picture: neither design 3.1's drawing nor the
/// mock-up has a heading there, and the window drew one -- in Subtitle, at the
/// top of the card -- directly under a tab pill whose caption is the same word.
/// The pill names the door and is on screen from all four; a second
/// `Shortcuts` two lines below it named nothing the reader could not already
/// see. Unlike 1017/1018/1034 it gives NO geometry back: the head row it
/// opened is still there, still `ctl` tall, still holding the filter and the
/// two buttons.
///
/// **1084 was `SYS_PLACEHOLDER`**, the System page's `Nothing here yet.`
/// line, deleted the day design §3.3's rows were built. It is the first id
/// retired for the reason it was ALLOCATED: Task 7 took it from the reserved
/// tail 1084-1099 rather than from the next free number precisely because a
/// placeholder is the one control on a page that is meant to be deleted, and
/// taking 1084 out of the middle of 1070-1083 would have left a hole in the
/// numbering of the page that replaced it. It did not.
///
/// **1115 was `ABOUT_PLACEHOLDER`**, and it went the same way one day later,
/// when design §3.4's fifteen rows replaced it. The entry here used to read
/// "About's own placeholder is NOT retired and is still a live control: that
/// page is still waiting." It is not waiting any more, and 1100-1114 came out
/// of that replacement intact -- the second time Task 7's tail-allocation
/// reasoning has been paid off, and the last, because there are no
/// placeholders left.
pub const RETIRED_IDS: &[i32] = &[
    1009, 1010, 1011, 1017, 1018, 1019, 1020, 1034, 1035, 1084, 1115,
];

/// The ids `crates/beckon-windows/examples/settings_probe.rs` hard-codes.
///
/// It drives ANOTHER process across a process boundary, so it cannot link
/// this crate and cannot be recompiled into agreement: these forty-eight are
/// fixed points, and `probe_pinned_ids_have_not_moved` is what says so out
/// loud.
///
/// **WIDENED 2026-08-15, from fifteen to forty-eight**, on review. This list
/// used to hold only the `const IDC_*` declarations at the top of the probe,
/// and the sentence below still counts them that way -- `grep -c "const
/// IDC_"` is 15 and stays 15. That was never the rule the list claimed to
/// enforce. `measure_system` transcribes 1070-1083 and `measure_about`
/// transcribes 1100-1114 as **bare literals in `ROWS` tables**, plus
/// literals in the arms that read them (`SWITCHES`, the `id == 1074`
/// trackbar arm, `shown(1071)`, the four log-row `shown(…)` calls, the three
/// copy glyphs, `text(1106)`, `text(1111)`). A literal in a table is a fixed
/// point across a process boundary in exactly the way a `const` is; the
/// spelling is not the property. So the twenty-nine System and About numbers
/// are here too.
///
/// **The NAME in each pair is `CONTROL_IDS`' name, not the probe's printed
/// label**, and for the About block the two differ: `CONTROL_IDS` says
/// `ABOUT_MARK` where the probe's `ROWS` prints `MARK`, because that column
/// is width-limited and the prefix is redundant once the section heading
/// says `About page`. What is pinned is the NUMBER; the label is how a human
/// reads the run. A test that matched labels instead would fail on a
/// cosmetic column change and say nothing about a renumber.
///
/// Of the original fifteen: the spec's three **pinned** rows
/// (`docs/superpowers/specs/2026-08-14-four-doors-phase-0-spec.md:141-145`
/// -- 1001-1008, 1012/1013, 1028-1031) account for only fourteen of them.
/// The fifteenth is `IDC_TAP 1025`, which that same table files under
/// `1014-1027 | in use, unpinned` at spec line 144 while
/// `settings_probe.rs:249` hard-codes it regardless. Counted from the probe
/// rather than from the spec:
/// `grep -c "const IDC_" crates/beckon-windows/examples/settings_probe.rs`
/// is 15 -- which is the count of DECLARATIONS, and is why that grep on its
/// own missed the twenty-nine literals the widening above added.
///
/// The spec's 1001-1008 row cites `settings_probe.rs:229-242` as its
/// evidence -- evidence for that row alone, not for the pinned set, since
/// 1012/1013 and 1028-1031 are pinned by separate rows. Seven of the fifteen
/// declarations fall inside that span; the other eight do not, and two of
/// those eight are the ones worth spelling out, because **they fail
/// differently and neither failure is loud.**
///
/// `IDC_NOTES 1004` is declared at `settings_probe.rs:1303` and read at 1308
/// as `dlg_item(h, IDC_NOTES).map(ctl_text).unwrap_or_default()`. A renumber
/// does not fail there at all: `dump` prints `notes:` with nothing after it,
/// which is indistinguishable from a model that genuinely has no notes, so
/// the run reads clean while the one control that says whether an event
/// landed is no longer being read.
///
/// `IDC_TAP 1025` does **not** go through `unwrap_or_default`.
/// `settings_probe.rs:940` reads it under
/// `if let Some(ctl) = dlg_item(parent, IDC_TAP)`, and the `else` arm at 986
/// prints `COMBOBOX IDC_TAP:     MISSING`. So a renumber does say something
/// -- and what it says is that a control which is on screen is absent. That
/// is worth pinning for the opposite reason to `IDC_NOTES`: a confident false
/// negative sends the next session hunting a control that was never lost, and
/// the hardware to check it against is scarce -- an SSH shell on a14 lands in
/// session 0 with no desktop, so every probe run costs a scheduled task in
/// session 1 (see the live-Windows-tests note in `CLAUDE.md`).
pub const PROBE_PINNED_IDS: &[(&str, i32)] = &[
    ("LIST", 1001),
    ("COMBO", 1002),
    ("APP", 1003),
    ("NOTES", 1004),
    ("ADD", 1005),
    ("REMOVE", 1006),
    ("APPLY", 1007),
    ("CAPS", 1008),
    ("OPENFILE", 1012),
    ("CLOSE", 1013),
    ("TAP", 1025),
    // The four pills. `examples/settings_probe.rs`'s `TAB_PILLS` transcribes
    // 1040-1043 as bare literals, exactly like every other entry here — they
    // were missed when this table grew from 15 to 44 because that pass walked
    // the System and About sections and the pills are neither.
    ("TAB_SHORTCUTS", 1040),
    ("TAB_KEYBOARD", 1041),
    ("TAB_SYSTEM", 1042),
    ("TAB_ABOUT", 1043),
    ("MOD_CTRL", 1028),
    ("MOD_WIN", 1029),
    ("MOD_ALT", 1030),
    ("MOD_SHIFT", 1031),
    // -- `measure_system`'s `ROWS`, transcribed as bare literals -----------
    // 1084 (`SYS_PLACEHOLDER`) is deliberately absent from the probe and so
    // from here: it is RETIRED, and `retired_ids_stay_retired` is what covers
    // a number nothing looks for any more.
    ("PAUSE", 1070),
    ("AUTOSTART", 1071),
    ("SYS_RELOAD", 1072),
    ("DARK", 1073),
    ("OPACITY", 1074),
    ("OPACITY_VALUE", 1075),
    ("CONFIG_NAME", 1076),
    ("CONFIG_DIR", 1077),
    ("CONFIG_OPEN", 1078),
    ("CONFIG_SHOW", 1079),
    ("LOG_NAME", 1080),
    ("LOG_SIZE", 1081),
    ("LOG_OPEN", 1082),
    ("LOG_SHOW", 1083),
    // -- `measure_about`'s `ROWS`, same shape ------------------------------
    // The probe prints these without the `ABOUT_` prefix; see the doc above
    // for why the pin is on the number and not on the label.
    ("ABOUT_MARK", 1100),
    ("ABOUT_NAME", 1101),
    ("ABOUT_BUILD_LABEL", 1102),
    ("ABOUT_BUILD_VALUE", 1103),
    ("ABOUT_BUILD_COPY", 1104),
    ("ABOUT_LOCATION_LABEL", 1105),
    ("ABOUT_LOCATION_VALUE", 1106),
    ("ABOUT_LOCATION_COPY", 1107),
    ("ABOUT_LICENCE_LABEL", 1108),
    ("ABOUT_LICENCE_VALUE", 1109),
    ("ABOUT_LICENCE_COPY", 1110),
    ("ABOUT_DISCLOSURE", 1111),
    ("ABOUT_GITHUB", 1112),
    ("ABOUT_RELEASES", 1113),
    ("ABOUT_BUG", 1114),
];

#[cfg(test)]
mod tests {
    use super::*;

    /// **Only the transition acts.** A beckon whose grant was present all
    /// along must not exit on every tick, and one still waiting must not
    /// either -- the user may not have reached the switch yet.
    #[test]
    fn only_the_moment_the_grant_appears_does_anything() {
        use GrantRecovery::*;
        assert_eq!(
            grant_recovery(false, true, false),
            Nothing,
            "granted all along"
        );
        assert_eq!(grant_recovery(true, false, false), Nothing, "still waiting");
        assert_eq!(grant_recovery(false, false, false), Nothing);
        assert_eq!(grant_recovery(true, true, false), RestartUnderLaunchd);
    }

    /// **Who started beckon decides what happens**, on the same signal
    /// `macos_broken_config` reads. Exiting under a person's own terminal
    /// would take their beckon away with nothing to bring it back.
    #[test]
    fn a_hand_started_beckon_is_told_rather_than_killed() {
        assert_eq!(grant_recovery(true, true, true), GrantRecovery::TellAndStay);
        assert_eq!(
            grant_recovery(true, true, false),
            GrantRecovery::RestartUnderLaunchd
        );
    }

    /// The offer appears exactly when it can do something.
    #[test]
    fn the_grant_button_is_offered_only_when_the_grant_is_missing() {
        assert!(grant_button_shown(false));
        assert!(!grant_button_shown(true));
    }
    use crate::shortcuts::Chord;

    const FILE: &str =
        "# mine\n\"ctrl+super+alt+t\" = \"Terminal\"\n\"ctrl+super+alt+e\" = \"File Explorer\"\n";

    fn model() -> Model {
        Model::from_text(FILE).unwrap()
    }

    #[test]
    fn loading_keeps_file_order_and_original_spelling() {
        let m = Model::from_text("\"alt+ctrl+t\" = \"Terminal\"\n").unwrap();
        assert_eq!(m.rows[0].orig_key.as_deref(), Some("alt+ctrl+t"));
        assert_eq!(m.rows[0].combo, "alt+ctrl+t");
        assert!(!m.dirty(), "just-loaded is not dirty");
    }

    /// `parse_config` returns BTreeMap order; the window must not.
    #[test]
    fn file_order_is_kept_even_when_it_is_not_alphabetical() {
        let m = Model::from_text(
            "\"ctrl+alt+z\" = \"Zed\"\n\"ctrl+alt+a\" = \"Apple\"\n\"ctrl+alt+m\" = \"Mail\"\n",
        )
        .unwrap();
        let apps: Vec<&str> = m.rows.iter().map(|r| r.app.as_str()).collect();
        assert_eq!(apps, vec!["Zed", "Apple", "Mail"]);
    }

    #[test]
    fn editing_marks_dirty_and_keeps_comments_on_render() {
        let mut m = model();
        assert!(!m.dirty());
        m.set_app(0, "Windows Terminal");
        assert!(m.dirty());
        let out = m.render().unwrap();
        assert!(out.contains("Windows Terminal"));
        assert!(out.contains("# mine"), "comment lost:\n{out}");
    }

    #[test]
    fn add_and_remove_rows() {
        let mut m = model();
        m.add_row();
        assert_eq!(m.rows.len(), 3);
        assert_eq!(m.selected, Some(2), "a new row selects itself");
        assert!(m.rows[2].orig_key.is_none());
        m.remove_row(2);
        assert_eq!(m.rows.len(), 2);
        assert!(m.dirty());
    }

    #[test]
    fn removing_the_last_row_clears_the_selection() {
        let mut m = Model::from_text("\"ctrl+alt+t\" = \"Terminal\"\n").unwrap();
        m.selected = Some(0);
        m.remove_row(0);
        assert_eq!(m.selected, None);
    }

    // ---------- validation ----------

    #[test]
    fn a_bad_combo_is_reported_verbatim_from_the_parser() {
        let mut m = model();
        m.set_combo(0, "ctrl+super+alt+T");
        let p = m.problems();
        assert_eq!(p.len(), 1);
        assert_eq!(p[0].row, Some(0));
        assert_eq!(p[0].severity, Severity::Error);
        assert!(p[0].message.contains("uppercase"), "{}", p[0].message);
    }

    #[test]
    fn an_empty_app_is_a_problem() {
        let mut m = model();
        m.set_app(1, "   ");
        let p = m.problems();
        assert_eq!(p.len(), 1);
        assert_eq!(p[0].row, Some(1));
    }

    #[test]
    fn duplicates_flag_both_rows_and_name_the_canonical_form() {
        let mut m = model();
        m.set_combo(1, "alt+ctrl+super+t");
        let p = m.problems();
        assert_eq!(p.len(), 2, "both rows must be flagged, not just the second");
        assert!(p.iter().any(|x| x.row == Some(0)));
        assert!(p.iter().any(|x| x.row == Some(1)));
        assert!(
            p[0].message.contains("ctrl+super+alt+t"),
            "{}",
            p[0].message
        );
    }

    #[test]
    fn render_refuses_an_invalid_model() {
        let mut m = model();
        m.set_combo(0, "nope+t");
        assert!(m.render().is_err());
    }

    /// The load-bearing guarantee: what the UI writes, the parser accepts.
    #[test]
    fn every_valid_model_round_trips_through_the_real_parser() {
        let mut m = model();
        m.set_app(0, "Windows Terminal");
        m.add_row();
        m.set_combo(2, "ctrl+super+alt+c");
        m.set_app(2, "Claude");
        m.set_caps(true);
        m.set_caps_tap(CapsTap::Escape);
        assert!(m.problems().is_empty(), "{:?}", m.problems());

        let text = m.render().unwrap();
        let parsed = parse_config(&text).expect("the writer must emit what the reader accepts");
        assert_eq!(parsed.shortcuts.len(), 3);
        assert!(parsed.keyboard.caps);
        assert_eq!(parsed.keyboard.caps_tap, CapsTap::Escape);

        let reloaded = Model::from_text(&text).unwrap();
        assert_eq!(reloaded.rows.len(), 3);
        assert_eq!(reloaded.keyboard, m.keyboard);
        assert!(!reloaded.dirty());
    }

    // ---------- the drawing projection ----------

    fn status_all_ok() -> RuntimeStatus {
        let mut r = HashMap::new();
        r.insert("ctrl+super+alt+t".to_string(), Ok(()));
        r.insert("ctrl+super+alt+e".to_string(), Ok(()));
        RuntimeStatus {
            registered: r,
            catalog: Some(vec!["Terminal".into(), "File Explorer".into()]),
            paused: false,
            probe: None,
        }
    }

    #[test]
    fn a_healthy_row_is_marked_ok() {
        let cs = control_state(&model(), &status_all_ok());
        assert_eq!(cs.items.len(), 2);
        assert_eq!(cs.items[0].mark, Mark::Ok);
        assert!(!cs.apply_enabled, "nothing to apply on a clean model");
    }

    #[test]
    fn a_failed_registration_marks_the_right_row() {
        let mut st = status_all_ok();
        st.registered.insert(
            "ctrl+super+alt+e".into(),
            Err("hotkey already taken".into()),
        );
        let cs = control_state(&model(), &st);
        assert_eq!(cs.items[0].mark, Mark::Ok);
        assert_eq!(cs.items[1].mark, Mark::Bad);
    }

    /// A scan that did not run cannot prove an app is absent.
    #[test]
    fn an_unscanned_catalog_shows_unknown_not_missing() {
        let st = RuntimeStatus {
            registered: status_all_ok().registered,
            catalog: None,
            paused: false,
            probe: None,
        };
        let mut m = model();
        m.selected = Some(0);
        let cs = control_state(&m, &st);
        let note = cs
            .detail
            .unwrap()
            .notes
            .into_iter()
            .find(|n| n.text.contains("installed"))
            .expect("there must be an app-resolution note");
        assert_eq!(note.mark, Mark::Unknown);
    }

    /// The pair to the test above, and the contrast is the point: a scan that
    /// has not finished says `Checking installed apps...` because "beckon does
    /// not know" is not a condition any status word claims, while a FINISHED
    /// scan that found nothing says `missing` and nothing else.
    ///
    /// It used to read the `Bad` off the note it no longer has. `Bad` is now
    /// on the row, from `flag_mark`, which is the same severity reached
    /// without repeating the word -- see `row_condition`.
    #[test]
    fn an_app_missing_from_a_scanned_catalog_is_marked_bad() {
        let mut m = model();
        m.set_app(0, "Nonexistent App");
        m.selected = Some(0);
        let cs = control_state(&m, &status_all_ok());
        assert_eq!(cs.items[0].flag.as_deref(), Some("missing"));
        assert_eq!(cs.items[0].mark, Mark::Bad);
        assert!(
            cs.detail.unwrap().notes.is_empty(),
            "one word, no sentence repeating it"
        );
    }

    /// The window must not call a binding broken that the resolver resolves.
    ///
    /// Every beckon resolver ends in a case-insensitive **substring** tier,
    /// and `check --resolve` deliberately passes it: a `Certainty::Guess`
    /// prints and exits 0, because two of the author's own bindings depend on
    /// that tier (`Settings` matching *System Settings*, `DeepSeek` matching
    /// *DeepSeek - Into the Unknown*). Failing them "would turn a correct file
    /// red, which is how a check stops being run".
    ///
    /// `row_condition` compared for EQUALITY, so the window said `missing`
    /// about exactly those bindings -- two halves of one program answering
    /// differently about one row. Measured on airm3 2026-08-16 with
    /// `examples/catalog_probe.rs` against the machine's real config:
    /// `Finder present`, `Settings MISSING`, `DeepSeek MISSING`, while
    /// `beckon check --resolve` exited 0 for both.
    ///
    /// This is the same defect `b4153ba` fixed for `Finder`, which widened the
    /// catalog and left the comparison alone -- so it fixed the one name whose
    /// problem was absence and none of the ones whose problem was the tier.
    #[test]
    fn an_app_that_resolves_by_substring_is_not_missing() {
        let mut m = model();
        m.set_app(0, "Settings");
        m.selected = Some(0);
        let st = RuntimeStatus {
            catalog: Some(vec!["System Settings".into(), "Terminal".into()]),
            ..status_all_ok()
        };
        let cs = control_state(&m, &st);
        assert_ne!(
            cs.items[0].flag.as_deref(),
            Some("missing"),
            "`Settings` resolves against *System Settings* by substring, which \
             is the tier `check --resolve` passes"
        );

        // Control: the tier is what saves it, not a blanket amnesty. A name
        // that is a substring of nothing is still missing.
        let mut m = model();
        m.set_app(0, "Nonexistent App");
        let cs = control_state(&m, &st);
        assert_eq!(cs.items[0].flag.as_deref(), Some("missing"));
    }

    /// Resolving loosely is not the same as resolving, and the row says which.
    ///
    /// `check --resolve` prints these under *"These shortcuts resolve, but
    /// only loosely"* and names the hazard: a substring match means an app
    /// installed later can quietly take the name. The window has no fifth
    /// status word to spend on that -- design 3.1 fixes the vocabulary at four
    /// -- so it says it the way every other non-word condition is said, in the
    /// editor's notes, and carries the severity on the row.
    ///
    /// Silent would be wrong in the direction that costs the user something:
    /// `Settings` resolving today against *System Settings* and tomorrow
    /// against a newly installed *Settings Sync* is exactly the failure the
    /// hazard describes, and the row is the only place it can be seen before
    /// it happens.
    #[test]
    fn a_substring_match_says_so_rather_than_passing_as_exact() {
        let st = RuntimeStatus {
            catalog: Some(vec!["System Settings".into(), "Terminal".into()]),
            ..status_all_ok()
        };

        let mut m = model();
        m.set_app(0, "Settings");
        m.selected = Some(0);
        let cs = control_state(&m, &st);
        assert_eq!(
            cs.items[0].mark,
            Mark::Warn,
            "loose is not clean; the row carries the severity the cell has no \
             word for"
        );
        let notes = cs.detail.unwrap().notes;
        assert!(
            notes.iter().any(|n| n.text.contains("System Settings")),
            "the note names what it actually matched, so the reader can see \
             whether it is the app they meant: {notes:?}"
        );

        // Control: an exact match stays silent. A healthy row says nothing,
        // and this note must not appear on every row in the file.
        let mut m = model();
        m.set_app(0, "Terminal");
        m.selected = Some(0);
        let cs = control_state(&m, &st);
        assert_eq!(cs.items[0].mark, Mark::Ok);
        assert!(
            cs.detail.unwrap().notes.is_empty(),
            "an exact match is not a hazard"
        );
    }

    #[test]
    fn catalog_matching_is_case_insensitive_like_every_beckon_resolver() {
        let mut m = model();
        m.set_app(0, "terminal");
        m.selected = Some(0);
        let cs = control_state(&m, &status_all_ok());
        assert_eq!(cs.items[0].flag, None, "a case difference is not a miss");
        let notes = cs.detail.unwrap().notes;
        assert!(
            !notes.iter().any(|n| n.text.contains("No installed app")),
            "{notes:?}"
        );
    }

    #[test]
    fn apply_needs_both_dirty_and_valid() {
        let mut m = model();
        assert!(!control_state(&m, &status_all_ok()).apply_enabled);
        m.set_app(0, "Windows Terminal");
        assert!(control_state(&m, &status_all_ok()).apply_enabled);
        m.set_combo(0, "bad+++");
        assert!(
            !control_state(&m, &status_all_ok()).apply_enabled,
            "a broken model must not be writable"
        );
    }

    /// The title bar's `*` is driven by `dirty`, and it must keep showing
    /// while Save is greyed out -- an unsaved edit that broke the file is
    /// the state where "you have unsaved changes" matters MOST. Reusing
    /// `apply_enabled` for it would drop the mark exactly there.
    #[test]
    fn dirty_outlives_apply_enabled_on_a_broken_model() {
        let mut m = model();
        let clean = control_state(&m, &status_all_ok());
        assert!(!clean.dirty, "just-loaded is not dirty");
        assert!(!clean.apply_enabled);

        m.set_app(0, "Windows Terminal");
        let edited = control_state(&m, &status_all_ok());
        assert!(edited.dirty);
        assert!(edited.apply_enabled);

        m.set_combo(0, "bad+++");
        let broken = control_state(&m, &status_all_ok());
        assert!(
            broken.dirty,
            "a broken model still has unsaved edits in it -- the title must \
             keep its mark"
        );
        assert!(
            !broken.apply_enabled,
            "...while Save stays disabled, which is why these are two fields"
        );
    }

    #[test]
    fn remove_is_disabled_with_no_selection() {
        let mut m = model();
        assert!(!control_state(&m, &status_all_ok()).remove_enabled);
        m.selected = Some(1);
        assert!(control_state(&m, &status_all_ok()).remove_enabled);
    }

    /// The window rebuilds the ListView whenever the row count changes,
    /// which wipes `LVIS_SELECTED` off every item. It can only put the
    /// highlight back if the snapshot says where it goes.
    #[test]
    fn the_snapshot_says_which_row_is_selected() {
        let mut m = model();
        assert_eq!(control_state(&m, &status_all_ok()).selected, None);
        m.selected = Some(1);
        assert_eq!(control_state(&m, &status_all_ok()).selected, Some(1));
        // add_row moves the selection on its own, so a window that reused
        // the highlight it had before would land on the wrong row.
        m.add_row();
        assert_eq!(control_state(&m, &status_all_ok()).selected, Some(2));
    }

    #[test]
    fn the_keyboard_group_reflects_the_model() {
        let mut m = model();
        m.set_caps(true);
        m.set_caps_tap(CapsTap::None);
        let cs = control_state(&m, &status_all_ok());
        assert!(cs.caps_checked);
        assert_eq!(cs.caps_tap, CapsTap::None);
    }

    // ---------- the status vocabulary ----------

    #[test]
    fn a_healthy_row_carries_no_flag_at_all() {
        let cs = control_state(&model(), &status_all_ok());
        assert_eq!(cs.items[0].flag, None, "healthy rows must be silent");
    }

    /// Design 3.1, rule 2: a healthy row says nothing at all -- no flag AND
    /// no note. This test used to assert the opposite, that a healthy row
    /// said `Registered and working.`; 3.1 deletes that sentence by name.
    ///
    /// It asserts on the empty `Vec`, not on "contains no such string": a
    /// note that had merely been REWORDED would pass the second and is
    /// exactly what the rule forbids.
    #[test]
    fn a_healthy_row_says_nothing_at_all() {
        let mut m = model();
        m.selected = Some(0);
        let cs = control_state(&m, &status_all_ok());
        let notes = cs.detail.unwrap().notes;
        assert!(notes.is_empty(), "silence is the healthy state: {notes:?}");
        assert_eq!(cs.items[0].flag, None);
        assert_eq!(cs.items[0].mark, Mark::Ok);
    }

    #[test]
    fn a_warning_does_not_block_saving_the_rest_of_the_file() {
        let mut m = model();
        m.set_app(0, "Windows Terminal");
        m.add_row(); // neutral, not an error
        let cs = control_state(&m, &status_all_ok());
        assert!(
            cs.apply_enabled,
            "an unfinished new row must not disable Save for edits made elsewhere"
        );
    }

    #[test]
    fn an_error_still_blocks_saving() {
        let mut m = model();
        m.set_combo(0, "bad+++");
        assert!(!control_state(&m, &status_all_ok()).apply_enabled);
    }

    #[test]
    fn paused_is_its_own_word_and_not_unknown() {
        let st = RuntimeStatus {
            paused: true,
            ..status_all_ok()
        };
        let cs = control_state(&model(), &st);
        assert_eq!(cs.items[0].flag.as_deref(), Some("paused"));
    }

    #[test]
    fn a_scan_still_running_is_not_the_same_as_an_app_that_is_missing() {
        let mut m = model();
        m.selected = Some(0);
        let scanning = RuntimeStatus {
            catalog: None,
            ..status_all_ok()
        };
        let cs = control_state(&m, &scanning);
        assert_eq!(
            cs.items[0].flag, None,
            "a scan in progress is not a row problem"
        );
        let note = cs
            .detail
            .unwrap()
            .notes
            .into_iter()
            .find(|n| n.text.contains("Checking"))
            .expect("the editor says so instead");
        assert_eq!(note.mark, Mark::Unknown);
    }

    /// The list flag and the row's mark are computed by ONE function, so they
    /// cannot contradict each other.
    ///
    /// **REWRITTEN 2026-08-15, and the old body would now pass for the wrong
    /// reason.** It asserted that a `missing` row had a `Mark::Bad` NOTE --
    /// true while `No installed app has this name.` existed, and design 3.1
    /// deletes it (`missing` already says that). What survives the deletion is
    /// the actual invariant: the row still reports `Mark::Bad`, now from
    /// `flag_mark` rather than from a sentence. Asserting the note again here
    /// would pin the very duplication rule 2 removes.
    #[test]
    fn the_list_and_the_editor_cannot_disagree_about_a_row() {
        let mut m = model();
        m.set_app(0, "Nonexistent App");
        m.selected = Some(0);
        let cs = control_state(&m, &status_all_ok());
        assert_eq!(
            cs.items[0].flag.as_deref(),
            Some("missing"),
            "the list must show the problem"
        );
        assert_eq!(
            cs.items[0].mark,
            Mark::Bad,
            "and the row's severity must agree it is one, with or without a note"
        );
        assert!(
            !cs.detail
                .unwrap()
                .notes
                .iter()
                .any(|n| n.text.contains("installed app")),
            "the note repeated the word and is deleted"
        );
    }

    #[test]
    fn a_taken_key_is_reported_in_words_a_non_developer_can_act_on() {
        let mut st = status_all_ok();
        st.registered
            .insert("ctrl+super+alt+t".into(), Err("0x581".into()));
        let mut m = model();
        m.selected = Some(0);
        let cs = control_state(&m, &st);
        assert_eq!(cs.items[0].flag.as_deref(), Some("in use"));
        assert_eq!(cs.items[0].mark, Mark::Bad);
        let notes = cs.detail.unwrap().notes;
        // The mock-up's sentence, verbatim. This is the ONE flagged
        // condition that keeps a note, and the second half is why: the word
        // `in use` says a program has the chord; only the sentence says
        // beckon can never tell you which, which is what stops the user
        // hunting for a name no Windows API returns.
        assert!(
            notes
                .iter()
                .any(|n| n.text == "Another program owns this key. Windows will not say which."),
            "{notes:?}"
        );
    }

    #[test]
    fn a_brand_new_row_asks_for_a_key_and_an_app_instead_of_shouting() {
        let mut m = model();
        m.add_row();
        let cs = control_state(&m, &status_all_ok());
        assert_eq!(cs.items[2].flag, None);
        assert_eq!(cs.items[2].mark, Mark::Unknown);
        let notes = cs.detail.unwrap().notes;
        assert_eq!(notes.len(), 1, "{notes:?}");
        assert_eq!(notes[0].text, "Pick a key and an app.");
        assert_eq!(notes[0].mark, Mark::Unknown);
    }

    /// `flag` is a single word but a row can be several things at once.
    /// Highest wins: paused, then in use, then missing, then other chord.
    /// `paused` sits above the registration map on purpose -- `serve` CLEARS
    /// that map when it pauses, so a paused row would otherwise read as "not
    /// registered yet" and say nothing about why.
    #[test]
    fn the_flag_precedence_is_paused_then_in_use_then_missing_then_other_chord() {
        let mut m = model();
        m.set_caps(true); // realistic config; `other chord` does not need this on
        m.set_combo(0, "ctrl+alt+t"); // not the caps_hold chord
        m.set_app(0, "Nonexistent App"); // not in the catalog
        let mut rt = status_all_ok();
        rt.registered
            .insert("ctrl+alt+t".into(), Err("already taken".into()));
        rt.paused = true;

        let flag = |m: &Model, rt: &RuntimeStatus| control_state(m, rt).items[0].flag.clone();
        assert_eq!(flag(&m, &rt).as_deref(), Some("paused"));
        rt.paused = false;
        assert_eq!(flag(&m, &rt).as_deref(), Some("in use"));
        rt.registered.insert("ctrl+alt+t".into(), Ok(()));
        assert_eq!(flag(&m, &rt).as_deref(), Some("missing"));
        m.set_app(0, "Terminal");
        assert_eq!(flag(&m, &rt).as_deref(), Some("other chord"));
        m.set_combo(0, "ctrl+super+alt+t");
        assert_eq!(flag(&m, &rt), None);
    }

    /// `other chord` answers one question: "does this combo match
    /// `keyboard.caps_hold`?" -- decided purely by comparing modifiers, with
    /// NO dependency on whether `keyboard.caps` itself is on. A gate on
    /// `keyboard.caps` was tried and reverted: the README's own
    /// `"ctrl+super+alt+shift+t" = "Telegram Web"` example, cited by the
    /// spec as the reason this flag exists, ships with no `keyboard.caps`
    /// block at all, so the gate would have left the spec's own example
    /// silently unflagged.
    #[test]
    fn other_chord_follows_caps_hold_regardless_of_whether_caps_is_on() {
        let mut m = model();
        m.set_combo(0, "ctrl+alt+t"); // not the default caps_hold (ctrl+super+alt)
        let mut rt = status_all_ok();
        rt.registered.insert("ctrl+alt+t".into(), Ok(()));
        assert_eq!(
            control_state(&m, &rt).items[0].flag.as_deref(),
            Some("other chord"),
            "Caps off must not silence `other chord`"
        );

        m.set_caps(true);
        assert_eq!(
            control_state(&m, &rt).items[0].flag.as_deref(),
            Some("other chord"),
            "and turning Caps on changes nothing about it"
        );

        m.keyboard.caps_hold = Chord::parse("ctrl+alt").unwrap();
        assert_eq!(
            control_state(&m, &rt).items[0].flag,
            None,
            "the chord is configurable, so `other chord` must follow it"
        );
    }

    // ---------- unfinished rows: the asymmetry, and why it exists ----------

    /// Dropping EVERY incomplete row would mean that clearing an existing
    /// binding's App field and pressing Save silently DELETES that binding.
    /// `orig_key` already tells the two cases apart -- `None` is a row the
    /// user just added and has not finished, `Some` is a row that came from
    /// the file -- so the asymmetry costs nothing. Do not "simplify" the two
    /// rules into one; the pair of tests below is what that would break.
    #[test]
    fn an_unfinished_new_row_is_dropped_from_the_write() {
        let mut m = model();
        m.set_app(0, "Windows Terminal");
        m.add_row();
        let text = m
            .render()
            .expect("an unfinished new row must not block the write");
        let parsed = parse_config(&text).unwrap();
        assert_eq!(parsed.shortcuts.len(), 2, "the two real rows still write");
        assert!(text.contains("Windows Terminal"), "{text}");
    }

    /// The other half of the asymmetry -- see the comment above.
    #[test]
    fn an_emptied_out_row_that_came_from_the_file_still_blocks_saving() {
        let mut m = model();
        assert!(m.rows[0].orig_key.is_some(), "this row came from the file");
        m.set_app(0, "");
        assert!(
            m.render().is_err(),
            "clearing an App field must not silently delete the binding"
        );
        assert!(!control_state(&m, &status_all_ok()).apply_enabled);
    }

    /// The keyboard block is not a shortcut and must never show up as a row.
    #[test]
    fn dotted_keyboard_settings_do_not_become_rows() {
        let m = Model::from_text(
            "keyboard.caps = true\nkeyboard.caps_tap = \"escape\"\n\"ctrl+alt+t\" = \"Terminal\"\n",
        )
        .unwrap();
        assert_eq!(m.rows.len(), 1);
        assert_eq!(m.rows[0].app, "Terminal");
        assert!(m.keyboard.caps);
    }

    // ---------- marking rows for multi-delete ----------

    #[test]
    fn marking_a_row_is_not_a_file_change() {
        let mut m = model();
        m.set_marked(0, true);
        assert!(
            !m.dirty(),
            "a tick changes nothing on disk; making it dirty would enable Save \
             for an empty edit and rewrite the file unchanged"
        );
        assert!(!control_state(&m, &status_all_ok()).apply_enabled);
    }

    #[test]
    fn removing_marked_rows_removes_all_of_them() {
        let mut m =
            Model::from_text("\"ctrl+alt+a\"=\"A\"\n\"ctrl+alt+b\"=\"B\"\n\"ctrl+alt+c\"=\"C\"\n")
                .unwrap();
        m.set_marked(0, true);
        m.set_marked(2, true);
        assert_eq!(m.marked_count(), 2);
        m.remove_marked();
        let apps: Vec<&str> = m.rows.iter().map(|r| r.app.as_str()).collect();
        assert_eq!(
            apps,
            vec!["B"],
            "index shifting must not drop the wrong row"
        );
        assert!(m.dirty());
    }

    #[test]
    fn an_external_reload_drops_the_marks() {
        let mut m = model();
        m.set_marked(0, true);
        let reloaded = Model::from_text(&m.render().unwrap_or_else(|_| FILE.into())).unwrap();
        assert!(
            !reloaded.rows[0].marked,
            "marks are UI state, never file state"
        );
    }

    /// The row that survives a multi-delete must still point at the SAME
    /// row after the marked ones are gone -- its index shifts down by
    /// however many marked rows sat ahead of it, it does not silently jump
    /// to whatever now occupies its old slot.
    #[test]
    fn a_surviving_selection_follows_its_own_row_through_a_multi_delete() {
        let mut m =
            Model::from_text("\"ctrl+alt+a\"=\"A\"\n\"ctrl+alt+b\"=\"B\"\n\"ctrl+alt+c\"=\"C\"\n")
                .unwrap();
        m.selected = Some(1); // B: not marked, must survive selected
        m.set_marked(0, true); // A: ahead of B, will be removed
        m.remove_marked();
        assert_eq!(
            m.selected,
            Some(0),
            "B shifted down by the one removed row ahead of it"
        );
        assert_eq!(m.rows[m.selected.unwrap()].app, "B");
    }

    /// The whole point of the tick boxes, and the test that fails loudly if
    /// the button is ever rewired back to the selection alone.
    ///
    /// Discriminating by construction: the marks and the selection name
    /// DIFFERENT rows, and the two readings disagree about every row. Marks
    /// win -> `A` and `C` go, `B` stays. Selection wins -> `B` goes, `A` and
    /// `C` stay, still ticked. Nothing about this passes both ways.
    #[test]
    fn remove_takes_the_ticked_rows_not_the_selected_one() {
        let mut m =
            Model::from_text("\"ctrl+alt+a\"=\"A\"\n\"ctrl+alt+b\"=\"B\"\n\"ctrl+alt+c\"=\"C\"\n")
                .unwrap();
        m.set_marked(0, true); // A
        m.set_marked(2, true); // C
        m.selected = Some(1); // B -- ticked by nobody
        m.remove_pressed();
        let apps: Vec<&str> = m.rows.iter().map(|r| r.app.as_str()).collect();
        assert_eq!(
            apps,
            vec!["B"],
            "Remove must take every ticked row; taking the selected row instead \
             leaves a tick box that lies on a button with no undo"
        );
    }

    /// The other half: no ticks at all, and Remove still has to work.
    #[test]
    fn remove_falls_back_to_the_selection_when_nothing_is_ticked() {
        let mut m =
            Model::from_text("\"ctrl+alt+a\"=\"A\"\n\"ctrl+alt+b\"=\"B\"\n\"ctrl+alt+c\"=\"C\"\n")
                .unwrap();
        m.selected = Some(1);
        m.remove_pressed();
        let apps: Vec<&str> = m.rows.iter().map(|r| r.app.as_str()).collect();
        assert_eq!(apps, vec!["A", "C"]);
    }

    /// And with neither, it is a no-op rather than a panic -- `on_remove`
    /// reaches this from a wndproc, where an unwind is undefined behaviour.
    #[test]
    fn remove_with_no_ticks_and_no_selection_does_nothing() {
        let mut m = model();
        let before = m.rows.len();
        m.selected = None;
        m.remove_pressed();
        assert_eq!(m.rows.len(), before);
        assert!(!m.dirty(), "a no-op must not arm Save");
    }

    /// Ticks alone arm the button. Before this, a user who ticked rows and
    /// then clicked into the empty space below the list -- or opened a
    /// window whose selection was never set -- found Remove greyed out with
    /// three rows visibly ticked.
    #[test]
    fn ticks_alone_enable_remove() {
        let mut m = model();
        m.selected = None;
        assert!(!control_state(&m, &status_all_ok()).remove_enabled);
        m.set_marked(0, true);
        let cs = control_state(&m, &status_all_ok());
        assert!(cs.remove_enabled);
        assert_eq!(cs.marked_count, 1);
    }

    // ---------- the file did not parse ----------

    /// The failure this whole path exists for, end to end: a real
    /// `Model::from_text` error on a real file, not a hand-written string.
    fn explain(text: &str) -> Vec<Note> {
        let err = Model::from_text(text).expect_err("this text must not parse");
        explain_unreadable(text, &err)
    }

    fn joined(notes: &[Note]) -> String {
        notes
            .iter()
            .map(|n| n.text.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// `toml::de::Error` DOES carry a location, so the note names the line
    /// and quotes it out of the user's own file.
    #[test]
    fn a_syntax_error_names_the_line_and_quotes_it() {
        let notes = explain("\"ctrl+alt+t\" = \"A\"\noops\n\"ctrl+alt+e\" = \"B\"\n");
        let all = joined(&notes);
        assert!(
            all.contains("The problem is on line 2: oops"),
            "the offending line must be named and quoted:\n{all}"
        );
        assert!(
            all.contains("What went wrong: expected"),
            "the parser's own reason must survive:\n{all}"
        );
    }

    /// beckon's OWN errors -- a duplicate combo, an unknown `keyboard.`
    /// setting, a non-string value -- name a key, not a place. Saying so is
    /// the requirement; inventing a line number would send someone to the
    /// wrong part of their file.
    #[test]
    fn a_beckon_error_admits_it_does_not_know_the_line() {
        for text in [
            "\"ctrl+alt+t\" = \"A\"\n\"alt+ctrl+t\" = \"B\"\n",
            "\"ctrl+alt+t\" = 5\n",
            "[keyboard]\ncaps = \"yes\"\n",
        ] {
            let all = joined(&explain(text));
            assert!(
                all.contains("The error does not say which line."),
                "no line number is claimed for `{text}`:\n{all}"
            );
            assert!(
                !all.contains("The problem is on line"),
                "and none is invented either:\n{all}"
            );
        }
    }

    /// The caret snippet lines up only in a monospace font, and this window
    /// has none, so none of it may reach the notes.
    #[test]
    fn the_parsers_ascii_art_never_reaches_the_notes() {
        let all = joined(&explain("\"ctrl+alt+t\" = \"A\"\noops\n"));
        assert!(!all.contains('|'), "gutter/caret rows leaked:\n{all}");
        assert!(!all.contains('^'), "caret leaked:\n{all}");
        assert!(
            !all.contains("TOML parse error"),
            "the header is a location, not a reason, and the line note \
             already carries the location:\n{all}"
        );
    }

    /// A location with no text behind it: past the end of the file, which is
    /// where a TOML parser points an unexpected EOF, or on a blank line. The
    /// location is still worth saying; the note must not trail off as
    /// `on line 9: `.
    ///
    /// The error string here is synthetic, and deliberately so -- it is
    /// `toml::de::Error`'s shape with a location the fixture text does not
    /// reach, which is easy to state and awkward to provoke. Every other
    /// test in this group goes through a real `Model::from_text` failure.
    #[test]
    fn a_location_with_no_line_behind_it_still_names_the_line() {
        for (text, line) in [("\"ctrl+alt+t\" = \"A\"\n", 9), ("a\n\nb\n", 2)] {
            let err = format!("TOML parse error at line {line}, column 1\nexpected `=`");
            let all = joined(&explain_unreadable(text, &err));
            assert!(
                all.contains(&format!("The problem is on line {line}.")),
                "the location is still worth saying:\n{all}"
            );
            assert!(
                !all.lines().any(|l| l.trim_end().ends_with(':')),
                "no note ends in a colon with nothing after it:\n{all}"
            );
        }
    }

    /// The quoted line is the user's own data, so it is passed through --
    /// but a minified 4 KB line would push every note under it off the band.
    #[test]
    fn a_very_long_offending_line_is_clipped() {
        let long = "x".repeat(4000);
        let all = joined(&explain(&format!("\"ctrl+alt+t\" = \"A\"\n{long}\n")));
        assert!(all.contains("xxx"), "the line is still quoted:\n{all}");
        assert!(
            all.lines().all(|l| l.chars().count() < 200),
            "no note is longer than a couple of wrapped lines"
        );
    }

    /// Everything beckon itself writes here is ASCII: this window carries a
    /// text face (not a symbol one), and a glyph it lacks draws as a box that
    /// reads like a rendering bug. The user's own file line is the one thing
    /// exempt, so it is ASCII in these fixtures.
    ///
    /// This is a live check on `parse_config`'s messages too, not only on
    /// the wrapper's: two of them carried an em-dash until the read-only
    /// window made them something a STATIC has to draw.
    ///
    /// **One fixture per error site `parse_config` can reach**, not a
    /// sample. Five of roughly twenty-one were covered when this was
    /// written, so a new em-dash in `unknown key`, `empty app name`,
    /// `duplicate modifier` or any of the `keyboard.*` messages would have
    /// passed. `explain` calls `expect_err`, so a fixture that stops
    /// producing an error fails here rather than quietly covering nothing.
    #[test]
    fn every_displayed_string_is_ascii() {
        for text in [
            // --- toml itself
            "\"ctrl+alt+t\" = \"A\"\noops\n",
            // --- Combo::parse, on a top-level key
            "\"ctrl++t\" = \"A\"\n",          // empty token
            "\"nope+t\" = \"A\"\n",           // expected a modifier
            "\"ctrl+ctrl+t\" = \"A\"\n",      // duplicate modifier
            "\"ctrl+super+alt+T\" = \"A\"\n", // uppercase key
            "\"ctrl+alt+zzz\" = \"A\"\n",     // unknown key
            "\"ctrl+alt\" = \"A\"\n",         // unknown key, with the hint
            // --- parse_config's own
            "\"ctrl+alt+t\" = \"A\"\n\"alt+ctrl+t\" = \"B\"\n", // duplicate combo
            "\"ctrl+alt+t\" = \"   \"\n",                       // empty app name
            "\"ctrl+alt+t\" = [\"A\", \"B\"]\n",                // an array
            "\"ctrl+alt+t\" = 3\n",                             // any other type
            // --- parse_keyboard
            "keyboard = 3\n",                       // not a table
            "[keyboard]\ncaps = \"yes\"\n",         // caps not a bool
            "[keyboard]\ncaps_tap = 3\n",           // caps_tap not a string
            "[keyboard]\ncaps_tap = \"nope\"\n",    // CapsTap::parse
            "[keyboard]\ncaps_hold = 3\n",          // caps_hold not a string
            "[keyboard]\nnope = 1\n",               // unknown setting
            "[keyboard]\n\"ctrl+alt+t\" = \"A\"\n", // a shortcut nested under it
            // --- Chord::parse, reached through caps_hold
            "[keyboard]\ncaps_hold = \"ctrl+shift\"\n", // shift is refused
            "[keyboard]\ncaps_hold = \"ctrl+\"\n",      // needs a modifier
            "[keyboard]\ncaps_hold = \"nope\"\n",       // expected a modifier
            "[keyboard]\ncaps_hold = \"ctrl+ctrl\"\n",  // duplicate modifier
        ] {
            let all = joined(&explain(text));
            assert!(all.is_ascii(), "non-ASCII reached the window:\n{all}");
        }
    }

    /// The backstop for the fixture list above: a message no fixture reaches
    /// is still a message this window may have to draw.
    ///
    /// Reads `shortcuts.rs` itself and requires every line of code outside
    /// its test module to be ASCII, with `//` comments stripped first -- the
    /// file's prose deliberately uses arrows and em-dashes, and prose is
    /// never drawn. What is left after stripping is string and char
    /// literals, which is exactly the set that can reach a STATIC.
    ///
    /// Cheap, total, and it does not care whether anyone remembered to add a
    /// fixture for a new error.
    #[test]
    fn no_message_in_the_parser_can_carry_a_non_ascii_character() {
        let src = include_str!("shortcuts.rs");
        let code = src
            .split("#[cfg(test)]")
            .next()
            .expect("split always yields one part");
        assert!(
            code.len() < src.len(),
            "the test module marker moved; this scan would cover the whole file"
        );
        for (i, line) in code.lines().enumerate() {
            let stripped = line.split("//").next().unwrap_or("");
            assert!(
                stripped.is_ascii(),
                "shortcuts.rs:{} carries a non-ASCII character in code, which the \
                 settings window may have to draw: {line}",
                i + 1
            );
        }
    }

    /// The read-only state is a real `ControlState`, so the window needs no
    /// idea that it exists -- and above all it is NOT dirty, because a
    /// window with nothing to save must not ask whether to save it.
    #[test]
    fn the_unreadable_state_edits_nothing_and_saves_nothing() {
        let cs = unreadable_state(explain("\"ctrl+alt+t\" = \"A\"\noops\n"));
        assert!(!cs.editable, "every mutating control is off");
        assert!(!cs.apply_enabled);
        assert!(!cs.remove_enabled);
        assert!(!cs.dirty, "no save prompt on the way out");
        assert!(cs.items.is_empty());
        assert_eq!(cs.selected, None);
        assert_eq!(cs.marked_count, 0);
        let notes = cs.detail.expect("the notes strip is the explanation").notes;
        assert!(!notes.is_empty());
        assert!(notes.iter().any(|n| n.mark == Mark::Bad));
    }

    // -----------------------------------------------------------------
    // The default button
    //
    // Measured on a14 2026-08-11: after the external-change banner was
    // dismissed, `DM_GETDEFID` still answered `IDC_RELOAD` (1015) -- a
    // button that is no longer on screen -- because `ShowWindow(SW_HIDE)`
    // raises no `BN_KILLFOCUS` for the window's focus-driven migration to
    // react to. Enter then pressed a hidden button. These tests are the
    // decision that closes it, and they run on all three CI jobs; the
    // wndproc that applies it compiles on one.
    // -----------------------------------------------------------------

    /// A clean model: Save disabled, nothing selected, no banner.
    fn rest_state() -> ControlState {
        control_state(&model(), &RuntimeStatus::default())
    }

    /// A dirty model with a row selected: Save and Remove both live.
    fn busy_state() -> ControlState {
        let mut m = model();
        m.selected = Some(0);
        m.set_app(0, "Something Else");
        control_state(&m, &RuntimeStatus::default())
    }

    #[test]
    fn a_hidden_reload_loses_the_default() {
        // The measured defect, stated as the decision that prevents it.
        let st = busy_state();
        assert_eq!(
            default_button(Some(DefaultButton::Reload), &st, false, Page::Shortcuts),
            Some(DefaultButton::Save),
            "the banner is down, so Enter must not reach Reload"
        );
        assert_eq!(
            default_button(Some(DefaultButton::KeepMine), &st, false, Page::Shortcuts),
            Some(DefaultButton::Save)
        );
    }

    #[test]
    fn reload_keeps_the_default_while_the_banner_is_up() {
        // The other half: the fix must not take the ring off a button the
        // user has genuinely tabbed to.
        let st = busy_state();
        assert_eq!(
            default_button(Some(DefaultButton::Reload), &st, true, Page::Shortcuts),
            Some(DefaultButton::Reload)
        );
        assert_eq!(
            default_button(Some(DefaultButton::KeepMine), &st, true, Page::Shortcuts),
            Some(DefaultButton::KeepMine)
        );
    }

    #[test]
    fn a_disabled_button_loses_the_default() {
        let rest = rest_state();
        assert!(!rest.remove_enabled, "precondition: nothing is selected");
        assert_eq!(
            default_button(Some(DefaultButton::Remove), &rest, false, Page::Shortcuts),
            Some(DefaultButton::Save)
        );
        // And keeps it while it is live, so this is a real test and not one
        // that passes because everything falls back.
        let busy = busy_state();
        assert!(busy.remove_enabled, "precondition: a row is selected");
        assert_eq!(
            default_button(Some(DefaultButton::Remove), &busy, false, Page::Shortcuts),
            Some(DefaultButton::Remove)
        );
    }

    #[test]
    fn save_keeps_the_default_even_when_it_is_disabled() {
        // The one exemption, and it is deliberate: Save is always on screen,
        // and the dialog manager will not dispatch to a disabled control, so
        // Enter is inert -- which is what "there is nothing to save" should
        // feel like. Moving the ring to Close here would make Enter close a
        // clean window and save a dirty one.
        let rest = rest_state();
        assert!(!rest.apply_enabled, "precondition: nothing to save");
        assert_eq!(
            default_button(Some(DefaultButton::Save), &rest, false, Page::Shortcuts),
            Some(DefaultButton::Save)
        );
    }

    /// `pressable`'s `Save` arm is never reached THROUGH `default_button`:
    /// the `current == HOME` early return intercepts it first (that is what
    /// the test above pins), so `default_button` never calls
    /// `Save.pressable(...)`. And the one place that arm does run today --
    /// `the_default_is_never_left_on_a_hidden_button`'s
    /// `got.pressable(st, external, page) || got == DefaultButton::HOME` -- does
    /// not depend on its answer, because the `|| got == HOME` half already
    /// makes the assertion pass whenever `got` is `Save`. `pressable` is
    /// `pub` and its own doc comment promises every arm reads the same
    /// `ControlState` field the window's `enable` call does; nothing today
    /// calls it directly with `Save` to hold that promise to account, so
    /// this test does.
    #[test]
    fn pressable_save_mirrors_apply_enabled() {
        let rest = rest_state();
        assert!(!rest.apply_enabled, "precondition: nothing to save");
        assert!(!DefaultButton::Save.pressable(&rest, false, Page::Shortcuts));

        let busy = busy_state();
        assert!(busy.apply_enabled, "precondition: a live edit exists");
        assert!(DefaultButton::Save.pressable(&busy, false, Page::Shortcuts));
    }

    /// The editor strip's two buttons act on the row it is showing, so they
    /// are live exactly when there is one. `rest_state` has no selection, so
    /// this separates them from the four that are pressable in every state.
    #[test]
    fn record_and_reset_need_a_row_to_act_on() {
        let rest = rest_state();
        assert!(rest.detail.is_none(), "precondition: nothing is selected");
        assert!(!DefaultButton::Record.pressable(&rest, false, Page::Shortcuts));
        assert!(!DefaultButton::Revert.pressable(&rest, false, Page::Shortcuts));
        assert_eq!(
            default_button(Some(DefaultButton::Record), &rest, false, Page::Shortcuts),
            Some(DefaultButton::Save),
            "Enter must not reach a greyed Record"
        );

        let busy = busy_state();
        assert!(busy.detail.is_some(), "precondition: a row is selected");
        assert!(DefaultButton::Record.pressable(&busy, false, Page::Shortcuts));
        assert!(DefaultButton::Revert.pressable(&busy, false, Page::Shortcuts));
        assert_eq!(
            default_button(Some(DefaultButton::Record), &busy, false, Page::Shortcuts),
            Some(DefaultButton::Record)
        );

        // A file that did not parse has a Model behind neither, so both are
        // off for the same reason every other mutating control is.
        let ro = unreadable_state(explain("\"ctrl+alt+t\" = \"A\"\noops\n"));
        assert!(!DefaultButton::Record.pressable(&ro, false, Page::Shortcuts));
        assert!(!DefaultButton::Revert.pressable(&ro, false, Page::Shortcuts));
    }

    #[test]
    fn the_read_only_state_leaves_the_default_on_save_or_an_escape() {
        // A file that did not parse: everything that mutates is off, and the
        // two escape routes are the only live buttons. The ring must land on
        // one of those or on Save, never on Add.
        let ro = unreadable_state(explain("\"ctrl+alt+t\" = \"A\"\noops\n"));
        assert_eq!(
            default_button(Some(DefaultButton::Add), &ro, false, Page::Shortcuts),
            Some(DefaultButton::Save)
        );
        assert_eq!(
            default_button(Some(DefaultButton::Close), &ro, false, Page::Shortcuts),
            Some(DefaultButton::Close)
        );
        assert_eq!(
            default_button(Some(DefaultButton::OpenFile), &ro, false, Page::Shortcuts),
            Some(DefaultButton::OpenFile)
        );
    }

    /// The invariant itself, over every button, every state this crate can
    /// produce, and now every door. A new `ControlState` field that gates a
    /// button, wired into the window but not into `pressable`, is what this
    /// catches -- and so is a fifth `Page` whose controls nobody assigned.
    #[test]
    fn the_default_is_never_left_on_a_hidden_button() {
        let states = [
            rest_state(),
            busy_state(),
            unreadable_state(explain("\"ctrl+alt+t\" = \"A\"\noops\n")),
        ];
        for st in &states {
            for external in [false, true] {
                for page in [Page::Shortcuts, Page::Keyboard, Page::System, Page::About] {
                    for b in DefaultButton::ALL {
                        // `None` in as well as `Some(b)`: the ring resting
                        // nowhere is the state System and About open in, so
                        // it is a real input and not just a possible output.
                        for current in [Some(b), None] {
                            let Some(got) = default_button(current, st, external, page) else {
                                // Nowhere is always a legal answer -- and on a
                                // door with no `home` it is the only one. What
                                // must never happen is a ring on a button that
                                // is not there, which is what the two asserts
                                // below are for.
                                continue;
                            };
                            assert!(
                                got.visible(external, page),
                                "{current:?} -> {got:?} is off screen \
                                 (external_change={external}, page={page:?})"
                            );
                            assert!(
                                got.pressable(st, external, page)
                                    || Some(got) == DefaultButton::home(page),
                                "{current:?} -> {got:?} is disabled and is not this door's home"
                            );
                        }
                    }
                }
            }
        }
    }

    /// Every button the Shortcuts page owns leaves the ring the moment
    /// another door opens.
    ///
    /// This is the tab-switch spelling of the measured a14 defect above:
    /// `ShowWindow(SW_HIDE)` raises no `BN_KILLFOCUS` whether the control
    /// went away because the banner was dismissed or because the user moved
    /// to another page, so a ring left on `Add` behind the Keyboard door
    /// makes Enter add a row nobody can see. The window cannot test this --
    /// it needs a wndproc and a person -- which is the whole reason `Page`
    /// lives in this crate.
    #[test]
    fn the_shortcuts_pages_buttons_lose_the_default_behind_another_door() {
        let busy = busy_state();
        for b in [
            DefaultButton::Add,
            DefaultButton::Remove,
            DefaultButton::Record,
            DefaultButton::Revert,
        ] {
            assert!(
                b.visible(false, Page::Shortcuts),
                "precondition: {b:?} is on screen on its own page"
            );
            for page in [Page::Keyboard, Page::System, Page::About] {
                assert!(!b.visible(false, page), "{b:?} is drawn on {page:?}");
                // `home(page)`, not `Save`: since the store split, two of
                // these three doors have no resting place at all, so the
                // right answer here is `None` on System and About.
                assert_eq!(
                    default_button(Some(b), &busy, false, page),
                    DefaultButton::home(page),
                    "Enter must not reach {b:?} from {page:?}"
                );
            }
        }
    }

    /// The banner's two are on screen exactly where the banner is, and the
    /// default ring follows them there and only there.
    ///
    /// **This assertion has now been written three ways in one day and this is
    /// the third.** Task 4 shipped Shortcuts-only, which was the defect below;
    /// the repair widened it to every door and this test asserted that; Task 6
    /// narrows it back, because the warn dot now carries the fact to the three
    /// doors the banner has left. What must never come back is a state with
    /// `external_change` set and NOTHING on screen about it, and that is a
    /// different assertion -- `the_warning_is_on_screen_from_every_door` --
    /// deliberately not folded into this one.
    #[test]
    fn the_banners_two_are_on_screen_only_on_the_door_they_are_about() {
        let busy = busy_state();
        for b in [DefaultButton::Reload, DefaultButton::KeepMine] {
            assert!(
                b.visible(true, BANNER_PAGE),
                "{b:?} is hidden on its own door"
            );
            // The ring may rest on a banner button there, which is the other
            // half of "it is on screen".
            assert_eq!(default_button(Some(b), &busy, true, BANNER_PAGE), Some(b));
            for page in [Page::Shortcuts, Page::Keyboard, Page::System, Page::About] {
                assert!(!b.visible(false, page), "no change to announce on {page:?}");
                assert_eq!(
                    default_button(Some(b), &busy, false, page),
                    DefaultButton::home(page)
                );
                if page != BANNER_PAGE {
                    assert!(!b.visible(true, page), "{b:?} is drawn on {page:?}");
                    assert_eq!(
                        default_button(Some(b), &busy, true, page),
                        DefaultButton::home(page),
                        "Enter must not reach {b:?} from {page:?}"
                    );
                }
            }
        }
        assert!(banner_shown(true, BANNER_PAGE));
        for page in [Page::Shortcuts, Page::Keyboard, Page::System, Page::About] {
            assert!(!banner_shown(false, page));
        }
    }

    /// The regression the four doors opened, stated as the thing that closes
    /// it: Save is pressable from every door, so from every door SOMETHING has
    /// to say the file moved.
    ///
    /// **This is the assertion that pays for `banner_shown` being narrow.**
    /// The banner and the warn dot partition `external_change` -- exactly one
    /// of them is up on any door, never both, never neither -- so there is no
    /// page from which `Ctrl+S` can overwrite an externally changed file with
    /// nothing on screen about it. Written against `visible` as well as against
    /// the two conditions, because the defect was a disagreement between them:
    /// Save reachable where the warning was not.
    ///
    /// **REWRITTEN 2026-08-14: it could not fail.** The assertion was
    /// `banner_shown(true, page) ^ warn_dot_shown(true, page)`, and
    /// `warn_dot_shown` IS `external_change && !banner_shown(..)` -- so with
    /// `external = true` the whole thing reduces to `B ^ !B`, true for any body
    /// of `banner_shown` whatsoever, a body returning `false` on every door
    /// included. That is precisely the state four documents cite this test as
    /// ruling out. Writing the second surface as the exact complement of the
    /// first is what makes them partition `external_change`; it is also what
    /// makes any assertion phrased as a relation BETWEEN them vacuous, so this
    /// asserts each against a constant instead.
    #[test]
    fn the_warning_is_on_screen_from_every_door() {
        // One row per door: what the banner does there, and what the dot does.
        // Constants, not expressions over the functions under test.
        let doors = [
            (Page::Shortcuts, true, false),
            (Page::Keyboard, false, true),
            (Page::System, false, true),
            (Page::About, false, true),
        ];
        // "Every door" is the claim, so the table has to BE every door, and
        // the walk is what makes that true rather than asserted: `Page::next`
        // visits each door once and returns home
        // (`four_steps_forward_visit_every_door_and_come_home`), so walking
        // the whole cycle and counting is a check a FIFTH door cannot slip
        // past -- a fixed `for _ in 0..doors.len()` would have tested four of
        // five and passed. The duplicate check is the other half: without it a
        // table listing one door twice would have the right length and leave
        // another with no row at all.
        let mut walk = Page::default();
        let mut doors_in_the_cycle = 0;
        loop {
            assert!(
                doors.iter().any(|(p, _, _)| *p == walk),
                "no row for {walk:?}, so this test does not cover every door"
            );
            walk = walk.next();
            doors_in_the_cycle += 1;
            assert!(
                doors_in_the_cycle <= doors.len(),
                "Page::next does not come home"
            );
            if walk == Page::default() {
                break;
            }
        }
        assert_eq!(
            doors_in_the_cycle,
            doors.len(),
            "the table has {} rows for {doors_in_the_cycle} doors",
            doors.len()
        );
        for (i, (p, _, _)) in doors.iter().enumerate() {
            assert!(
                !doors[..i].iter().any(|(q, _, _)| q == p),
                "{p:?} is in the table twice, so some door has no row"
            );
        }
        for (page, banner, dot) in doors {
            // **REVISED 2026-08-15.** This was
            // `assert!(DefaultButton::Save.visible(true, page), "Save is
            // chrome; if it is not on {page:?} this test measures nothing")`,
            // and design §1's store split falsified its premise: Save is drawn
            // on two doors now, not four.
            //
            // The claim the test makes did not narrow with it. The warning is
            // still owed from every door, including the two that can no longer
            // save -- what a person does next from System is open the
            // Shortcuts door, and arriving there to find the file already
            // reloaded under them is the failure either way. So the row below
            // still covers four doors; only this precondition changed, into
            // the pairing that now makes the test meaningful.
            assert_eq!(
                DefaultButton::Save.visible(true, page),
                page.writes_config(),
                "the command bar and the store split disagree on {page:?}"
            );
            // The table's own shape, checked before it is used as an oracle:
            // somebody is warning on every door, and never both at once.
            assert!(banner || dot, "the table says {page:?} announces nothing");
            assert!(!(banner && dot), "the table says {page:?} announces twice");

            assert_eq!(
                banner_shown(true, page),
                banner,
                "the file has moved: the banner is {} on {page:?}",
                if banner { "missing" } else { "drawn" }
            );
            assert_eq!(
                warn_dot_shown(true, page),
                dot,
                "the file has moved: the warn dot is {} on {page:?}",
                if dot { "missing" } else { "drawn" }
            );
            assert!(
                !banner_shown(false, page) && !warn_dot_shown(false, page),
                "nothing has moved, so {page:?} must announce nothing"
            );
            // The banner is a sentence and two buttons; a banner without its
            // answers is an announcement the user cannot act on.
            if banner {
                assert!(
                    DefaultButton::Reload.visible(true, page)
                        && DefaultButton::KeepMine.visible(true, page),
                    "the announcement is drawn on {page:?} without its two answers"
                );
            }
        }
    }

    /// The dot goes on a pill that is not lit, which is what keeps its ink off
    /// `accent_fill`.
    ///
    /// `warn` on `accent_fill` measures 1.212 in Light and has no row in
    /// `theme::pairs`, so a dot drawn on the ACTIVE Shortcuts pill would be a
    /// contrast failure no test could see. It cannot happen, and this is why
    /// rather than a comment saying so: the dot is up only where the banner is
    /// not, and the banner is up exactly on the door whose pill is lit.
    #[test]
    fn the_dot_is_never_on_the_door_that_is_open() {
        assert!(!warn_dot_shown(true, BANNER_PAGE));
        for page in [Page::Keyboard, Page::System, Page::About] {
            assert!(warn_dot_shown(true, page), "no dot on {page:?}");
        }
    }

    /// The badge counts the file, never the filtered list. A filter separates
    /// the two, which is the whole reason the field exists -- and since
    /// 2026-08-15 the badge is the only count on screen, so being right about
    /// the file is the only thing it can be right about.
    #[test]
    fn the_badge_counts_the_file_not_the_filtered_list() {
        let mut m = model();
        let all = m.rows.len();
        assert!(
            all >= 2,
            "precondition: more than one row to filter down to"
        );
        // Filter to the first row's app, with nothing selected so `visible`
        // has no exemption to grant either.
        m.selected = None;
        let app = m.rows[0].app.clone();
        m.set_filter(&app);
        let st = control_state(&m, &RuntimeStatus::default());
        assert!(
            st.items.len() < all,
            "precondition: the filter has to actually hide something"
        );
        assert_eq!(st.binding_count, all, "the badge followed the filter");
        // And the read-only projection has no file to count.
        assert_eq!(unreadable_state(Vec::new()).binding_count, 0);
    }

    /// `BANNER_PAGE` is Task 6's target, and it has to name a door the strip
    /// really has -- the warn dot goes on that pill, and the announcement goes
    /// back to that page with it.
    #[test]
    fn the_banner_page_is_the_shortcut_table_it_is_about() {
        assert_eq!(BANNER_PAGE, Page::Shortcuts);
        assert!(banner_shown(true, BANNER_PAGE));
    }

    /// `Ctrl+Tab` four times comes home, having stopped at every door once.
    ///
    /// This is the whole of what a cycle has to be, and both halves are worth
    /// asserting separately from the inverse test below: a `next` that skipped
    /// a door would still return home in four steps if it visited another
    /// twice, and a `next` that returned its argument would visit one door
    /// four times and come "home" every step.
    #[test]
    fn four_steps_forward_visit_every_door_and_come_home() {
        let start = Page::Shortcuts;
        let mut seen = Vec::new();
        let mut p = start;
        for _ in 0..4 {
            assert_ne!(p.next(), p, "{p:?} is its own next door");
            p = p.next();
            seen.push(p);
        }
        assert_eq!(p, start, "four doors, four steps, and this is not home");
        for door in [Page::Shortcuts, Page::Keyboard, Page::System, Page::About] {
            assert_eq!(
                seen.iter().filter(|s| **s == door).count(),
                1,
                "{door:?} is visited {} times in one lap of {seen:?}",
                seen.iter().filter(|s| **s == door).count()
            );
        }
    }

    /// `Ctrl+Shift+Tab` undoes `Ctrl+Tab`, from every door.
    ///
    /// The cycle is spelled twice -- once forwards and once backwards, both as
    /// exhaustive `match`es so that a fifth `Page` cannot be added without
    /// answering for it in both -- so this is the test that stops the two
    /// spellings drifting. Nothing else can: each is internally consistent on
    /// its own.
    #[test]
    fn next_and_prev_are_inverses() {
        for page in [Page::Shortcuts, Page::Keyboard, Page::System, Page::About] {
            assert_eq!(page.next().prev(), page, "next then prev left {page:?}");
            assert_eq!(page.prev().next(), page, "prev then next left {page:?}");
        }
    }

    /// The strip wraps in both directions, named at the two ends where it is
    /// the only thing to do.
    ///
    /// Spelled out rather than left to the lap above, because "wraps" is a
    /// decision (`Page::next`'s doc) rather than an implementation detail: a
    /// later reader who thinks `Ctrl+Tab` should stop at `About` has to delete
    /// a test that says otherwise.
    #[test]
    fn the_strip_wraps_at_both_ends() {
        assert_eq!(Page::About.next(), Page::Shortcuts);
        assert_eq!(Page::Shortcuts.prev(), Page::About);
    }

    /// The return trip into Shortcuts is a placement on a combo that has not
    /// moved, and a placement is the measured data-loss call.
    #[test]
    fn a_combo_already_where_it_belongs_is_not_placed_again() {
        let want = ComboSpot {
            x: 140,
            y: 300,
            cx: 420,
        };
        assert!(!combo_needs_placing(want, Some(want)));
    }

    /// Each of the three components on its own has to force the placement, or
    /// a real move would be skipped and the control would be left behind.
    #[test]
    fn a_combo_that_moved_or_resized_is_placed() {
        let want = ComboSpot {
            x: 140,
            y: 300,
            cx: 420,
        };
        for seen in [
            ComboSpot { x: 141, ..want },
            ComboSpot { y: 301, ..want },
            ComboSpot { cx: 421, ..want },
        ] {
            assert!(
                combo_needs_placing(want, Some(seen)),
                "{seen:?} is not {want:?} and must be placed"
            );
        }
    }

    /// Two states that are not "already correct" and must never be read as
    /// one: a position that could not be read at all, and the `0, 0, 10, 10`
    /// every child is created at -- which is what the first pass after the
    /// window is reopened sees.
    #[test]
    fn an_unreadable_or_freshly_created_combo_is_placed() {
        let want = ComboSpot {
            x: 140,
            y: 300,
            cx: 420,
        };
        assert!(combo_needs_placing(want, None));
        assert!(combo_needs_placing(
            want,
            Some(ComboSpot { x: 0, y: 0, cx: 10 })
        ));
    }

    /// The fold needs BOTH the preference and an armed Caps, and the switch
    /// needs only the latter. Getting the second half wrong is the trap: gate
    /// the switch on the fold and turning the preference off would grey the
    /// only control that can turn it back on.
    #[test]
    fn the_caps_fold_needs_the_preference_and_an_armed_caps() {
        let hold = Chord::default();
        assert_eq!(caps_view_fold(true, true, hold), Some(hold));
        assert_eq!(caps_view_fold(false, true, hold), None, "preference off");
        assert_eq!(caps_view_fold(true, false, hold), None, "Caps not armed");
        assert_eq!(caps_view_fold(false, false, hold), None);

        assert!(caps_view_enabled(true));
        assert!(!caps_view_enabled(false));
        // The trap, stated: with the preference off and Caps armed, the fold
        // is off and the switch is STILL usable.
        assert_eq!(caps_view_fold(false, true, hold), None);
        assert!(caps_view_enabled(true));
    }

    /// The fold carries the CONFIGURED chord through, not a default, so a
    /// user who changed `caps_hold` sees their own chord fold.
    #[test]
    fn the_fold_passes_the_configured_chord_through() {
        let odd = Chord {
            ctrl: true,
            super_: false,
            alt: false,
        };
        assert_eq!(caps_view_fold(true, true, odd), Some(odd));
    }

    /// The epoch, the day before it, and the four cases the era-shifted
    /// algorithm exists to get right: a common-year 28 Feb / 1 Mar boundary, a
    /// leap year's 29 Feb, the century that is NOT a leap year (1900), and the
    /// 400-year exception that IS (2000).
    ///
    /// Spot values rather than a formula, because a formula here would be this
    /// function's own body written twice and would agree with any bug in it.
    #[test]
    fn the_build_date_is_the_gregorian_calendar() {
        assert_eq!(ymd(t(0)).as_deref(), Some("1970-01-01"));
        assert_eq!(ymd(t(86_399)).as_deref(), Some("1970-01-01"), "same day");
        assert_eq!(ymd(t(86_400)).as_deref(), Some("1970-01-02"));
        // 2000-02-29: a leap day in the century that IS a leap year.
        assert_eq!(ymd(t(951_782_400)).as_deref(), Some("2000-02-29"));
        assert_eq!(ymd(t(951_868_800)).as_deref(), Some("2000-03-01"));
        // 1900 was NOT a leap year, but it is before the epoch, so the nearest
        // reachable check on the same rule is 2100 -- also not a leap year.
        assert_eq!(ymd(t(4_107_456_000)).as_deref(), Some("2100-02-28"));
        assert_eq!(ymd(t(4_107_542_400)).as_deref(), Some("2100-03-01"));
        // A common year's month boundary, and the release this landed in.
        assert_eq!(ymd(t(1_709_164_800)).as_deref(), Some("2024-02-29"));
        assert_eq!(ymd(t(1_735_689_599)).as_deref(), Some("2024-12-31"));
        assert_eq!(ymd(t(1_735_689_600)).as_deref(), Some("2025-01-01"));
    }

    /// Before the epoch there is no answer, and `None` is how the row says so
    /// -- it falls back to the triple alone rather than printing a wrong date.
    #[test]
    fn a_time_before_the_epoch_has_no_build_date() {
        assert_eq!(
            ymd(std::time::UNIX_EPOCH - std::time::Duration::from_secs(1)),
            None
        );
    }

    /// The row is the triple ALONE when the image cannot be stat'd, and the
    /// triple plus a date when it can. `copy` follows `shown` on this row --
    /// see the comment at the construction site for why it differs from
    /// `Location`.
    #[test]
    fn the_build_row_carries_the_running_images_date_or_nothing() {
        let exe = exe_path();
        let dated = about(&exe, Some(t(1_000)), ImageOnDisk::Written(t(1_723_000_000)));
        assert_eq!(dated.build.shown, "aarch64-pc-windows-msvc · 2024-08-07");
        assert_eq!(dated.build.copy, dated.build.shown);

        for gone in [ImageOnDisk::Gone, ImageOnDisk::Unknown] {
            let s = about(&exe, Some(t(1_000)), gone);
            assert_eq!(
                s.build.shown, "aarch64-pc-windows-msvc",
                "a row that cannot date itself says less rather than guessing"
            );
            assert_eq!(s.build.copy, s.build.shown);
        }
    }

    /// The four phrases, and the precedence between the first two.
    ///
    /// A file that did not parse outranks a pause, because pausing a service
    /// that was never serving is not the fact worth printing -- the same
    /// reading `row_condition` gives its own four words one surface down.
    #[test]
    fn the_service_line_says_one_thing_at_a_time() {
        let broken = service_line(false, false, 0, 0);
        assert_eq!(broken.mark, Mark::Bad);
        assert!(broken.text.starts_with("Not serving"));
        // ...and it still outranks a pause.
        assert_eq!(service_line(false, true, 0, 0), broken);

        let paused = service_line(true, true, 3, 19);
        assert_eq!(paused.mark, Mark::Warn);
        assert_eq!(paused.text, "Paused");

        let all = service_line(true, false, 19, 19);
        assert_eq!(all.mark, Mark::Ok);
        assert_eq!(all.text, "Serving · 19 of 19");

        // Some chord did not take. `Warn`, not `Bad`: the rest are working,
        // which is `row_condition`'s own reading of `in use` on one row.
        let some = service_line(true, false, 18, 19);
        assert_eq!(some.mark, Mark::Warn);
        assert_eq!(some.text, "Serving · 18 of 19");
    }

    /// **The denominator is the same number the Shortcuts pill shows.** The
    /// drawing puts `Shortcuts 19` and `Serving · 18 of 19` on screen at once,
    /// so the two 19s must be one number -- and the trap is that
    /// `RuntimeStatus::registered` is the last registration PASS while the
    /// badge is the pending model. A row added and not yet saved has to make
    /// the numerator smaller, never the denominator.
    #[test]
    fn the_service_lines_total_is_the_badges_total() {
        let mut m = model();
        let rt = RuntimeStatus::default();
        let before = control_state(&m, &rt);
        assert_eq!(
            before.service.text,
            format!("Serving · 0 of {}", before.binding_count)
        );

        m.add_row();
        let after = control_state(&m, &rt);
        assert_eq!(
            after.binding_count,
            before.binding_count + 1,
            "precondition: the badge counts the new row"
        );
        assert_eq!(
            after.service.text,
            format!("Serving · 0 of {}", after.binding_count),
            "the bar and the badge must not print two different totals"
        );
    }

    /// **And the numerator is counted over those same rows.** The test above
    /// only ever ADDS a row, which is the direction that keeps the numerator
    /// the smaller number; removing one runs it the other way. `RuntimeStatus`
    /// is unchanged across the removal here on purpose -- that is what a
    /// `Remove` before a save does, since nothing re-registers until the file
    /// is written -- and a numerator taken from `registered`'s own values then
    /// printed `Serving · 2 of 1`, an impossibility, in amber.
    #[test]
    fn removing_a_row_before_saving_cannot_serve_more_than_the_total() {
        let mut m = model();
        let rt = status_all_ok();
        let before = control_state(&m, &rt);
        assert_eq!(
            before.service.text, "Serving · 2 of 2",
            "precondition: both rows are in the last registration pass"
        );
        assert_eq!(before.service.mark, Mark::Ok);

        m.selected = Some(1);
        m.remove_pressed();
        let after = control_state(&m, &rt);
        assert_eq!(after.binding_count, 1, "precondition: the row is gone");
        assert_eq!(after.service.text, "Serving · 1 of 1");
        assert_eq!(
            after.service.mark,
            Mark::Ok,
            "a binding the model no longer has is not a chord that failed"
        );
    }

    /// A file that did not parse has no model to count, and the line says so
    /// rather than printing `0 of 0` as though it were serving nothing.
    #[test]
    fn the_unreadable_state_carries_the_broken_phrase() {
        let ro = unreadable_state(explain("\"ctrl+alt+t\" = \"A\"\noops\n"));
        assert_eq!(ro.service.mark, Mark::Bad);
        assert!(!ro.service.text.contains("Serving"));
    }

    /// Design §1's split by store, pinned as a table rather than as prose.
    ///
    /// `Keyboard` is the row worth having a test for: nothing on that door is
    /// a shortcut, so "the Shortcuts page saves" is a plausible misreading --
    /// but its three controls write `keyboard.caps`, `keyboard.caps_hold` and
    /// `keyboard.caps_tap`, which are keys in the same file.
    #[test]
    fn the_store_split_is_two_doors_each_way() {
        assert!(Page::Shortcuts.writes_config());
        assert!(Page::Keyboard.writes_config());
        assert!(!Page::System.writes_config());
        assert!(!Page::About.writes_config());
        for page in [Page::Shortcuts, Page::Keyboard, Page::System, Page::About] {
            assert_eq!(
                command_bar_shown(page),
                page.writes_config(),
                "the bar and the store split disagree on {page:?}"
            );
        }
    }

    /// **REPLACED 2026-08-15.** This test was `the_command_bar_is_on_every_page`
    /// and asserted the opposite of what it now asserts: that all three
    /// buttons are visible on all four doors, ending
    /// `assert!(DefaultButton::HOME.visible(false, Page::About))`. It was
    /// right about the code and wrong about the window -- design §1 splits by
    /// STORE, and a `Save` under a door that writes no file is a button with
    /// nothing to do.
    #[test]
    fn the_command_bar_follows_the_store_split() {
        const BAR: [DefaultButton; 3] = [
            DefaultButton::Save,
            DefaultButton::OpenFile,
            DefaultButton::Close,
        ];
        for page in [Page::Shortcuts, Page::Keyboard] {
            for b in BAR {
                assert!(b.visible(false, page), "{b:?} is missing from {page:?}");
            }
            assert_eq!(DefaultButton::home(page), Some(DefaultButton::Save));
        }
        for page in [Page::System, Page::About] {
            for b in BAR {
                assert!(!b.visible(false, page), "{b:?} still draws on {page:?}");
            }
            assert_eq!(
                DefaultButton::home(page),
                None,
                "{page:?} has no Save, so the ring has nowhere to rest"
            );
        }
    }

    /// The repair itself, stated as the transition it has to survive: the
    /// window opens on Shortcuts with the ring at `Save`, the user presses
    /// `Ctrl+3`, and `Save` is no longer on screen.
    ///
    /// **This is the case `default_button`'s old early return got wrong by
    /// construction** -- `current == HOME` returned `HOME` without looking at
    /// the page at all, so the ring stayed on a button the door does not draw
    /// and Enter would have pressed it. Hiding a control raises no focus
    /// notification (measured, a14 2026-08-11), so nothing else would have
    /// moved it.
    #[test]
    fn changing_to_a_door_that_cannot_save_takes_the_ring_off_save() {
        let busy = busy_state();
        assert!(busy.apply_enabled, "precondition: Save would be live");
        assert_eq!(
            default_button(Some(DefaultButton::Save), &busy, false, Page::Shortcuts),
            Some(DefaultButton::Save)
        );
        for page in [Page::System, Page::About] {
            assert_eq!(
                default_button(Some(DefaultButton::Save), &busy, false, page),
                None,
                "the ring stayed on Save behind {page:?}"
            );
        }
    }

    /// The System and About doors still take the ring onto a button the user
    /// tabs to -- "no home" is not "no default ever", and reading it that way
    /// would make Enter dead on eleven working buttons.
    #[test]
    fn a_door_without_a_home_still_follows_focus_onto_its_own_buttons() {
        let rest = rest_state();
        assert_eq!(
            default_button(Some(DefaultButton::SysReload), &rest, false, Page::System),
            Some(DefaultButton::SysReload)
        );
        assert_eq!(
            default_button(Some(DefaultButton::AboutGithub), &rest, false, Page::About),
            Some(DefaultButton::AboutGithub)
        );
        // ...and drops it when the button belongs to another door.
        assert_eq!(
            default_button(Some(DefaultButton::SysReload), &rest, false, Page::About),
            None
        );
    }

    /// The mirror of the test above: a state projected from a real model is
    /// always editable, so `editable` cannot quietly become "sometimes off"
    /// for a file that parsed.
    #[test]
    fn a_state_projected_from_a_model_is_always_editable() {
        let mut m = model();
        assert!(control_state(&m, &RuntimeStatus::default()).editable);
        m.set_combo(0, "not a combo at all");
        assert!(
            control_state(&m, &RuntimeStatus::default()).editable,
            "an invalid EDIT is not a read-only FILE: Save is greyed, the \
             fields stay live so it can be fixed"
        );
    }

    #[test]
    fn a_leading_comment_survives_a_save() {
        let src = "# my notes about this file\n\"ctrl+alt+a\" = \"Notepad\"\n\"ctrl+alt+b\" = \"Brave\"\n";
        let mut m = Model::from_text(src).unwrap();
        m.set_app(0, "Notepad++");
        let out = m.render().unwrap();
        assert_eq!(
            out.matches("# my notes about this file").count(),
            1,
            "a hand-written comment must survive a window edit exactly ONCE -- the \
         point of writing back through toml_edit is that hand edits and window \
         edits stay interchangeable, and a header-restoring fix that cannot \
         tell 'survived' from 'was eaten' would duplicate it here.\n\
         --- got ---\n{out}"
        );
    }

    #[test]
    fn deleting_the_first_row_eats_the_file_header_comment() {
        let src = "# my notes about this file\n\"ctrl+alt+a\" = \"Notepad\"\n\"ctrl+alt+b\" = \"Brave\"\n";
        let mut m = Model::from_text(src).unwrap();
        m.selected = Some(0);
        m.remove_pressed();
        let out = m.render().unwrap();
        assert!(
            out.contains("# my notes about this file"),
            "a comment above the FIRST binding must not be deleted along with \
             it -- toml_edit carries a leading comment as the first key's \
             prefix decor, so removing that key takes the comment with it.\n\
             --- got ---\n{out}"
        );
    }

    // ---------- the filter ----------

    fn three() -> Model {
        Model::from_text(
            "\"ctrl+alt+a\"=\"Notepad\"\n\"ctrl+alt+b\"=\"Brave\"\n\"ctrl+alt+q\"=\"Weather\"\n",
        )
        .unwrap()
    }

    #[test]
    fn an_empty_filter_shows_every_row() {
        let m = three();
        assert_eq!(m.visible(), vec![0, 1, 2]);
    }

    #[test]
    fn the_filter_matches_the_app_name_case_insensitively() {
        let mut m = three();
        m.set_filter("BRA");
        assert_eq!(m.visible(), vec![1]);
    }

    #[test]
    fn the_filter_does_not_match_the_shortcut_column() {
        // Every beckon chord contains `alt`, so a filter that matched the
        // combo made `a` -- a plausible first keystroke of "brave" -- match
        // EVERY row while the box looked filtered. Tick the visible rows,
        // press Remove, lose the table. These are the four bindings the bug
        // was measured with; before this changed, `visible` returned all
        // four.
        //
        // `three()` cannot carry this test: `notepad`, `brave` and `weather`
        // all contain an `a`, so filter `a` returns every row on the app
        // column alone and the assertion could not tell the two rules apart.
        let mut m = Model::from_text(
            "\"ctrl+alt+b\"=\"Brave\"\n\
             \"ctrl+alt+k\"=\"Kitty\"\n\
             \"ctrl+alt+f\"=\"Firefox\"\n\
             \"ctrl+alt+d\"=\"Discord\"\n",
        )
        .unwrap();
        m.set_filter("a");
        assert_eq!(
            m.visible(),
            vec![0],
            "only Brave's NAME contains `a`; matching the chord too made \
             this every row"
        );
    }

    #[test]
    fn filtering_by_a_key_name_finds_nothing() {
        // The cost of the fix, pinned rather than left to be rediscovered:
        // this window can no longer answer "what is ctrl+alt+q bound to?" by
        // filtering. If that bites, the way back is to match the chord's KEY
        // only -- never substring-matching the whole chord again.
        let mut m = three();
        m.set_filter("alt+q");
        assert!(m.visible().is_empty());
    }

    #[test]
    fn a_filter_matching_nothing_shows_no_rows() {
        let mut m = three();
        m.set_filter("zzz");
        assert!(m.visible().is_empty());
    }

    #[test]
    fn the_filter_is_trimmed_before_matching() {
        let mut m = three();
        m.set_filter("brave ");
        assert_eq!(
            m.visible(),
            vec![1],
            "a trailing space left by typing would otherwise hide every row, \
             which reads as a hang"
        );
    }

    #[test]
    fn setting_a_filter_is_not_a_file_change() {
        let mut m = three();
        let before = m.render().unwrap();
        m.set_filter("brave");
        assert!(!m.dirty(), "a filter changes nothing on disk");
        assert_eq!(m.render().unwrap(), before, "the filter is never written");
    }

    /// The defect this whole feature had to be designed around: with a
    /// filter active, the list's own index is NOT the model's, so a callback
    /// that passes it straight through ticks one binding and deletes
    /// another.
    #[test]
    fn list_items_carry_their_model_row_not_their_position() {
        let mut m = three();
        m.set_filter("weather");
        let cs = control_state(&m, &status_all_ok());
        assert_eq!(cs.items.len(), 1);
        assert_eq!(cs.items[0].row, 2, "Weather is row 2 of the model");
        assert_ne!(
            cs.items[0].row, 0,
            "if this ever passes by accident, the mapping has been dropped \
             and position is being used as the model index again"
        );
    }

    #[test]
    fn selected_is_a_view_index_while_filtered() {
        let mut m = three();
        m.selected = Some(2); // Weather, model row 2
        m.set_filter("e"); // Notepad, Brave and Weather all contain "e"
        let cs = control_state(&m, &status_all_ok());
        assert_eq!(cs.items.len(), 3);
        assert_eq!(cs.selected, Some(2));

        m.set_filter("weather"); // now Weather is the ONLY visible row
        let cs = control_state(&m, &status_all_ok());
        assert_eq!(
            cs.selected,
            Some(0),
            "ControlState::selected indexes `items`, which the filter has \
             shortened -- the ListView needs the line number, not the row"
        );
        assert_eq!(cs.detail.unwrap().app, "Weather");
    }

    #[test]
    fn the_selected_row_stays_visible_even_when_it_stops_matching() {
        let mut m = three();
        m.selected = Some(0); // Notepad, which "brave" does not match
        m.set_filter("brave");
        let cs = control_state(&m, &status_all_ok());
        assert_eq!(cs.items.len(), 2, "Brave, plus the selected Notepad");
        assert_eq!(cs.selected, Some(0), "Notepad leads in model order");
        assert_eq!(
            cs.detail.as_ref().unwrap().app,
            "Notepad",
            "the editor must keep describing the row the user has selected"
        );
    }

    /// The defect the exception exists for. Dropping a row from the view the
    /// moment it stops matching pulls it out from under the editor mid-word:
    /// `apply_state`'s `None` arm disables the field that has keyboard focus
    /// and blanks it, leaving the half-typed value in the model with nothing
    /// on screen to explain it.
    #[test]
    fn editing_a_row_until_it_stops_matching_does_not_kill_the_editor() {
        let mut m = three();
        m.set_filter("brave");
        let cs = control_state(&m, &status_all_ok());
        assert_eq!(cs.items.len(), 1);
        let row = cs.items[0].row;
        m.selected = Some(row);

        // The user backspaces "Brave" down to "Brav", which no longer matches.
        m.set_app(row, "Brav");
        let cs = control_state(&m, &status_all_ok());
        assert_eq!(cs.items.len(), 1, "the row being edited stays on screen");
        assert_eq!(cs.selected, Some(0));
        assert_eq!(
            cs.detail.unwrap().app,
            "Brav",
            "the editor keeps the partial value it is holding"
        );
    }

    #[test]
    fn remove_takes_the_ticked_row_you_can_see() {
        let mut m = three();
        m.set_marked(1, true); // Brave
        m.set_filter("brave");
        m.remove_pressed();
        let apps: Vec<&str> = m.rows.iter().map(|r| r.app.as_str()).collect();
        assert_eq!(apps, vec!["Notepad", "Weather"]);
    }

    /// The invariant the whole design turns on: a destructive button with no
    /// confirm and no undo must not act on rows that are off screen.
    #[test]
    fn remove_leaves_a_ticked_row_the_filter_is_hiding() {
        let mut m = three();
        m.set_marked(0, true); // Notepad, about to be hidden
        m.set_marked(1, true); // Brave, will stay visible
        m.set_filter("brave");
        m.remove_pressed();
        let apps: Vec<&str> = m.rows.iter().map(|r| r.app.as_str()).collect();
        assert_eq!(
            apps,
            vec!["Notepad", "Weather"],
            "Brave was visible and ticked so it goes; Notepad was ticked but \
             hidden, and Remove must never delete what is not on screen"
        );
        assert!(m.rows[0].marked, "the hidden tick survives to come back");
    }

    #[test]
    fn remove_takes_the_selected_row_because_the_filter_cannot_hide_it() {
        let mut m = three();
        m.selected = Some(0); // Notepad, which "brave" does not match
        m.set_filter("brave"); // nothing is ticked, so the fallback runs
        m.remove_pressed();
        let apps: Vec<&str> = m.rows.iter().map(|r| r.app.as_str()).collect();
        assert_eq!(
            apps,
            vec!["Brave", "Weather"],
            "the selected row is exempt from the filter, so it IS on screen \
             and Remove is entitled to take it -- the invariant is 'never \
             delete a row you cannot see', not 'never delete a row that does \
             not match'"
        );
    }

    #[test]
    fn marked_count_and_remove_enabled_count_only_visible_rows() {
        let mut m = three();
        m.set_marked(0, true); // Notepad
        m.set_filter("brave"); // hides it
        let cs = control_state(&m, &status_all_ok());
        assert_eq!(
            cs.marked_count, 0,
            "a count that included hidden ticks would put a number on screen \
             that Remove does not honour"
        );
        assert!(!cs.remove_enabled, "nothing visible is ticked or selected");
    }

    #[test]
    fn add_clears_the_filter_so_the_new_row_is_visible() {
        let mut m = three();
        m.set_filter("brave");
        m.add_row();
        assert_eq!(m.filter(), "");
        let cs = control_state(&m, &status_all_ok());
        assert_eq!(cs.items.len(), 4);
        assert_eq!(
            cs.selected,
            Some(3),
            "the new row must be both visible and selected"
        );
    }

    // ---------- the Caps hold chord ----------

    #[test]
    fn the_hold_chord_reaches_the_window() {
        let m = model();
        let cs = control_state(&m, &status_all_ok());
        assert_eq!(
            cs.caps_hold,
            Chord::default(),
            "an untouched file uses the default chord, and the window has to \
             show it rather than guess"
        );
    }

    #[test]
    fn setting_the_hold_chord_is_a_file_change() {
        let mut m = model();
        assert!(m.set_caps_hold(Chord {
            ctrl: true,
            super_: false,
            alt: false
        }));
        assert!(m.dirty());
        assert_eq!(
            control_state(&m, &status_all_ok()).caps_hold.canonical(),
            "ctrl"
        );
    }

    #[test]
    fn setting_the_same_hold_chord_twice_is_not_a_change() {
        let mut m = model();
        assert!(m.set_caps_hold(Chord::default()));
        assert!(
            !m.dirty(),
            "writing back what was already there is not an edit"
        );
    }

    /// `Chord::parse` refuses an empty chord because the hook has to have
    /// something to press. The window can reach that state by unticking the
    /// last chip, so the model refuses it there too rather than letting an
    /// unwritable value into itself.
    #[test]
    fn unticking_the_last_modifier_is_refused() {
        let mut m = model();
        let before = m.keyboard.caps_hold;
        assert!(!m.set_caps_hold(Chord {
            ctrl: false,
            super_: false,
            alt: false
        }));
        assert_eq!(m.keyboard.caps_hold, before, "the chord is unchanged");
        assert!(!m.dirty());
    }

    /// The round trip that matters: whatever the window sets must come back
    /// out of the file meaning the same thing.
    #[test]
    fn the_hold_chord_survives_a_save_and_reload() {
        let mut m = model();
        m.set_caps_hold(Chord {
            ctrl: true,
            super_: false,
            alt: true,
        });
        let text = m.render().unwrap();
        let back = Model::from_text(&text).unwrap();
        assert_eq!(back.keyboard.caps_hold.canonical(), "ctrl+alt");
    }

    // ---------- the availability probe ----------

    fn rows3() -> Model {
        Model::from_text(
            "\"ctrl+alt+a\"=\"Notepad\"\n\"ctrl+alt+b\"=\"Brave\"\n\"ctrl+alt+q\"=\"Weather\"\n",
        )
        .unwrap()
    }

    /// F12 is reserved for debuggers "at all times", so a successful
    /// registration proves nothing. It has to be refused BEFORE the OS is
    /// asked, or the probe reports a green Available on a key documented
    /// never to arrive.
    #[test]
    fn f12_is_refused_before_the_os_is_asked() {
        let m = rows3();
        assert_eq!(
            probe_plan(&m, 0, "ctrl+alt+f12"),
            ProbePlan::Verdict(Availability::F12)
        );
    }

    #[test]
    fn a_combo_already_in_this_file_is_a_self_conflict() {
        let m = rows3();
        // Row 0 is being edited to what row 1 already holds.
        assert_eq!(
            probe_plan(&m, 0, "ctrl+alt+b"),
            ProbePlan::Verdict(Availability::DuplicateInFile {
                app: "Brave".into()
            })
        );
    }

    /// A row keeping its own combo is not a conflict with itself.
    #[test]
    fn a_row_keeping_its_own_combo_is_unchanged_not_a_duplicate() {
        let m = rows3();
        assert_eq!(
            probe_plan(&m, 0, "ctrl+alt+a"),
            ProbePlan::Verdict(Availability::Unchanged)
        );
    }

    /// Only when nothing above matched may the OS be asked. Getting this
    /// order wrong is what makes a probe claim a reserved or duplicated
    /// chord is free.
    #[test]
    fn a_clean_combo_reaches_the_os() {
        let m = rows3();
        assert_eq!(probe_plan(&m, 0, "ctrl+alt+z"), ProbePlan::AskTheOs);
    }

    /// `Verdict(_)` is not the assertion this test exists to make:
    /// `Verdict(Free)` satisfies it, and a green Available on a string that
    /// names no chord is the single worst thing the ordering prevents.
    #[test]
    fn an_unparseable_combo_never_reaches_the_os() {
        let m = rows3();
        assert_eq!(
            probe_plan(&m, 0, "banana"),
            ProbePlan::Verdict(Availability::Unchanged)
        );
    }

    /// **The F12 guard does NOT commute with the own-row check**, which is
    /// the claim `probe_plan`'s doc comment used to make. Nothing rejects a
    /// row bound to `ctrl+alt+f12` -- `problems()` has no f12 rule -- so
    /// such a row probed against its own chord reaches step 3 the moment
    /// the guard is moved below it, and answers `Unchanged`, which
    /// `probe_notes` renders as `Mark::Ok`: a green OK on the one key §F.6
    /// exists to keep from ever coming back green.
    ///
    /// The fixture is the point. `rows3()` binds no f12, so the Step 5
    /// half-A reorder experiment measured the fixture and not the property.
    #[test]
    fn f12_outranks_the_rows_own_chord() {
        let m = Model::from_text("\"ctrl+alt+f12\"=\"Notepad\"\n").unwrap();
        assert_eq!(m.rows[0].combo, "ctrl+alt+f12");
        assert_eq!(
            probe_plan(&m, 0, "ctrl+alt+f12"),
            ProbePlan::Verdict(Availability::F12),
            "moving the f12 guard below step 3 makes this Unchanged, i.e. Mark::Ok"
        );
    }

    /// `render` DROPS a half-typed new row, so it can collide with nothing
    /// that ever reaches the file -- which is why `problems()` skips it for
    /// duplicates. Step 4 used to blame it anyway, producing
    /// `Already used by "" in this file.`: empty quotes, naming a row that
    /// will never be written.
    #[test]
    fn a_half_typed_new_row_is_not_blamed_for_a_duplicate() {
        let mut m = rows3();
        m.add_row();
        let new = m.rows.len() - 1;
        m.set_combo(new, "ctrl+alt+z"); // app still blank
        assert!(is_unfinished_new_row(&m.rows[new]));
        assert_eq!(probe_plan(&m, 0, "ctrl+alt+z"), ProbePlan::AskTheOs);

        // Finished, it counts -- the rule is "unfinished", not "new".
        m.set_app(new, "Weather");
        assert_eq!(
            probe_plan(&m, 0, "ctrl+alt+z"),
            ProbePlan::Verdict(Availability::DuplicateInFile {
                app: "Weather".into()
            })
        );
    }

    /// `RegisterHotKey` refuses a chord held anywhere on the desktop, and
    /// that includes beckon's own live table -- which is the SAVED file,
    /// while `probe_plan` reads the edited model. The window raises the
    /// probe BEFORE `on_edit_combo` (deliberately, so step 3 has the row's
    /// previous chord to compare against), so a row edited away from its
    /// saved chord and back is not caught by step 3, reaches the OS, and
    /// gets `Taken` -- "Another program already has this shortcut" -- about
    /// beckon itself.
    #[test]
    fn a_rows_own_saved_chord_is_unchanged_not_taken() {
        let mut m = rows3();
        m.set_combo(0, "ctrl+alt+z");
        assert_eq!(m.rows[0].orig_key.as_deref(), Some("ctrl+alt+a"));
        assert_eq!(
            probe_plan(&m, 0, "ctrl+alt+a"),
            ProbePlan::Verdict(Availability::Unchanged)
        );
    }

    /// The saved-chord relaxation must not outrank step 4. Another row
    /// spelling the chord NOW is a real conflict the user has to fix, and
    /// answering `Unchanged` there would be the same green-on-a-bad-chord
    /// failure as moving the f12 guard.
    #[test]
    fn a_saved_chord_another_row_now_spells_is_still_a_duplicate() {
        let mut m = rows3();
        m.set_combo(0, "ctrl+alt+z");
        m.set_combo(1, "ctrl+alt+a"); // row 1 took row 0's saved chord
        assert_eq!(
            probe_plan(&m, 0, "ctrl+alt+a"),
            ProbePlan::Verdict(Availability::DuplicateInFile {
                app: "Brave".into()
            })
        );
    }

    // ---------- folding a verdict into a row's condition ----------

    /// §F.6's `Free` sentence, verbatim. Asserted on by identity because a
    /// substring would also match a sentence that had been reworded around
    /// it.
    const AVAILABLE: &str = "Available. Nothing else on this PC is using it.";

    fn status_probing(combo: &str, verdict: Availability) -> RuntimeStatus {
        RuntimeStatus {
            registered: HashMap::new(),
            catalog: Some(vec!["Notepad".into(), "Brave".into(), "Weather".into()]),
            paused: false,
            probe: Some(ProbeResult {
                combo: combo.into(),
                verdict,
            }),
        }
    }

    /// A verdict is about the row being EDITED. Folded into every row, one
    /// row's answer would appear on another row's line -- and since `mark`
    /// is derived from the notes, a `Bad` verdict would redden the wrong
    /// row in the LIST too.
    #[test]
    fn a_verdict_shows_only_on_the_selected_row() {
        let mut m = rows3();
        let st = status_probing("ctrl+alt+b", Availability::Free);

        m.selected = Some(0);
        let (_, _, notes) = row_condition(&m, 1, &st, &m.problems());
        assert!(
            !notes.iter().any(|n| n.text == AVAILABLE),
            "row 1 is not selected: {notes:?}"
        );

        m.selected = Some(1);
        let (_, _, notes) = row_condition(&m, 1, &st, &m.problems());
        assert!(
            notes.iter().any(|n| n.text == AVAILABLE),
            "row 1 is selected and spells the probed chord: {notes:?}"
        );
    }

    /// The user types on and the verdict goes stale. A stale verdict has to
    /// vanish rather than be shown against the chord that replaced it --
    /// which is the entire reason `ProbeResult` carries a combo.
    #[test]
    fn a_verdict_about_a_chord_the_row_no_longer_spells_is_ignored() {
        let mut m = rows3();
        m.selected = Some(0);
        let st = status_probing("ctrl+alt+z", Availability::Free);

        let (_, _, notes) = row_condition(&m, 0, &st, &m.problems());
        assert!(
            !notes.iter().any(|n| n.text == AVAILABLE),
            "the row means ctrl+alt+a, the verdict is about ctrl+alt+z: {notes:?}"
        );

        // Spelled differently is not stale: `same_chord` compares canonical
        // forms, the same rule `problems()` uses for duplicates.
        m.set_combo(0, "alt+ctrl+z");
        let (_, _, notes) = row_condition(&m, 0, &st, &m.problems());
        assert!(
            notes.iter().any(|n| n.text == AVAILABLE),
            "alt+ctrl+z and ctrl+alt+z are one chord: {notes:?}"
        );
    }

    /// `probe_plan` answers `Unchanged` for a string that names no chord,
    /// and `Unchanged` is a `Mark::Ok` sentence. It stays off the screen
    /// only because `same_chord` refuses when EITHER side fails to parse --
    /// so even a byte-identical pair matches nothing. That is load-bearing,
    /// and until now it was argued rather than tested.
    #[test]
    fn a_verdict_about_an_unparseable_combo_never_renders() {
        let mut m = rows3();
        m.set_combo(0, "banana");
        m.selected = Some(0);
        let st = status_probing("banana", Availability::Unchanged);
        let (_, _, notes) = row_condition(&m, 0, &st, &m.problems());
        assert!(
            !notes
                .iter()
                .any(|n| n.text == "Unchanged - this row already uses it."),
            "{notes:?}"
        );
    }

    /// The strings are the spec's, verbatim, and a free verdict must never
    /// say the shortcut WORKS -- only that nothing else is holding it.
    #[test]
    fn a_free_verdict_does_not_claim_the_shortcut_works() {
        let n = probe_notes(
            &ProbeResult {
                combo: "ctrl+alt+z".into(),
                verdict: Availability::Free,
            },
            false,
        );
        assert_eq!(n[0].text, "Available. Nothing else on this PC is using it.");
        assert!(
            !n.iter().any(|x| x.text.to_lowercase().contains("works")),
            "a registration proves nothing else holds the chord, not that it fires"
        );
    }

    #[test]
    fn a_windows_key_chord_says_windows_may_take_it_back() {
        let n = probe_notes(
            &ProbeResult {
                combo: "super+z".into(),
                verdict: Availability::FreeWithWin,
            },
            false,
        );
        assert_eq!(
            n[0].text,
            "Available right now. Windows reserves Windows-key shortcuts and can take this one back after an update, so press it once after saving to be sure."
        );
    }

    #[test]
    fn a_taken_chord_does_not_name_a_program_it_cannot_know() {
        let n = probe_notes(
            &ProbeResult {
                combo: "ctrl+alt+z".into(),
                verdict: Availability::Taken,
            },
            false,
        );
        assert_eq!(
            n[0].text,
            "Another program already has this shortcut. Windows does not tell beckon which one, so beckon cannot name it. Saved as-is, it will not fire."
        );
        assert_eq!(n[0].mark, Mark::Bad);
    }

    #[test]
    fn probing_while_paused_says_so() {
        let n = probe_notes(
            &ProbeResult {
                combo: "ctrl+alt+z".into(),
                verdict: Availability::Free,
            },
            true,
        );
        assert!(
            n.iter()
                .any(|x| x.text
                    == "beckon is paused, so this shows what will happen when you resume."),
            "the verdict is about the future while paused, and must say so"
        );
    }

    /// No string may leak an API name or an error code.
    #[test]
    fn no_string_names_an_api() {
        for v in [
            Availability::Free,
            Availability::FreeWithWin,
            Availability::Unchanged,
            Availability::Taken,
            Availability::F12,
            Availability::CaptureSawNothing,
            Availability::DuplicateInFile { app: "X".into() },
        ] {
            for n in probe_notes(
                &ProbeResult {
                    combo: "ctrl+alt+z".into(),
                    verdict: v,
                },
                false,
            ) {
                for bad in ["RegisterHotKey", "UIPI", "0x", "HRESULT"] {
                    assert!(!n.text.contains(bad), "{bad} leaked into {:?}", n.text);
                }
            }
        }
    }

    /// The pair has to round-trip for EVERY flag, because the painter takes
    /// the cell apart with no other information about it.
    ///
    /// **This is also the guard on the vocabulary being safe against itself**,
    /// a job `no_flag_word_is_a_suffix_of_another` claimed until 2026-08-15.
    /// That test asserted no word is a suffix of another and justified itself
    /// with `in use` beside `key in use` -- a pair `split_app_cell` handles
    /// correctly, because it demands `FLAG_SEP` in front of the word it
    /// strips and `key in use` offers one space there, not three. The rule it
    /// enforced was a superset of the real one and the real one is behavioural,
    /// so it lives here instead: add a word ending in `FLAG_SEP` plus another
    /// word and this loop returns the wrong pair for it. Deleted rather than
    /// corrected, because two tests over the same table invite the reader to
    /// trust the cheaper one.
    #[test]
    fn split_app_cell_inverts_app_cell_for_every_flag() {
        for f in FLAGS {
            let cell = app_cell("Terminal", Some(f));
            assert_eq!(split_app_cell(&cell), ("Terminal", Some(f)), "flag {f:?}");
        }
        let cell = app_cell("Terminal", None);
        assert_eq!(split_app_cell(&cell), ("Terminal", None));
    }

    /// **Every word `row_condition` can produce must be in `FLAGS`**, or the
    /// painter silently stops colouring one. This is the guard that makes the
    /// vocabulary closed rather than merely documented as closed.
    ///
    /// **CORRECTED 2026-08-16: it used to restate the table instead of running
    /// it.** The loop was over a hand-copy of `FLAGS`' own four words and
    /// asserted `FLAGS.contains`, i.e. `FLAGS ⊇ FLAGS` -- green whatever
    /// `row_condition` does, and blind in the one direction it exists to
    /// guard. Measured: a `conditions.push("no key")` added to `row_condition`
    /// left it, `split_app_cell_inverts_app_cell_for_every_flag` and
    /// `an_unknown_flag_word_is_neutral` all green while the list drew an
    /// uncoloured `Notepad   no key`.
    ///
    /// It drives the real producer now, the way the round trip beside it does.
    /// One fixture per word, plus a healthy row that must stay silent -- which
    /// is where an unconditional fifth push lands. The fixtures ARE the
    /// coverage: a word pushed on a branch none of them reaches is still
    /// invisible here, and only a type could close that.
    #[test]
    fn every_flag_row_condition_produces_is_in_the_table() {
        let mut missing = model();
        missing.set_app(0, "Nonexistent App");
        let mut other = model();
        other.set_combo(0, "ctrl+alt+t");
        let mut other_rt = status_all_ok();
        other_rt.registered.insert("ctrl+alt+t".into(), Ok(()));

        let mut taken = status_all_ok();
        taken
            .registered
            .insert("ctrl+super+alt+t".into(), Err("0x581".into()));
        let paused = RuntimeStatus {
            paused: true,
            ..status_all_ok()
        };

        let mut seen: Vec<&str> = Vec::new();
        for (m, rt) in [
            (model(), status_all_ok()),
            (model(), paused),
            (model(), taken),
            (missing, status_all_ok()),
            (other, other_rt),
        ] {
            for it in control_state(&m, &rt).items {
                let Some(f) = it.flag else { continue };
                assert!(FLAGS.contains(&f.as_str()), "{f:?} missing from FLAGS");
                let f = FLAGS.iter().find(|w| **w == f).unwrap();
                if !seen.contains(f) {
                    seen.push(f);
                }
            }
        }
        // The fixtures have to keep reaching every word, or the loop above
        // stops asserting anything about the ones they dropped.
        for f in FLAGS {
            assert!(seen.contains(&f), "no fixture produces {f:?} any more");
        }
    }

    /// **The notes went quiet and took no severity with them.**
    ///
    /// Three of the four words lost the note that only repeated them (design
    /// 3.1), and `mark` was derived from the notes alone -- so without
    /// `flag_mark` every one of these rows would now report `Mark::Ok`.
    /// Nothing in either window reads `ListItem::mark` today, so that
    /// regression would have been invisible until something did read it.
    ///
    /// The expected marks are the ones the DELETED notes carried, which is
    /// what makes this a before/after assertion rather than a fresh opinion.
    #[test]
    fn the_deleted_notes_did_not_take_the_marks_with_them() {
        // `in use`: was a `Bad` note, still `Bad`. It is also the one word
        // that kept a note, so it is the control -- if this row alone were
        // right, the mark would be coming from the note as before.
        let mut taken = status_all_ok();
        taken
            .registered
            .insert("ctrl+super+alt+t".into(), Err("0x581".into()));
        assert_eq!(control_state(&model(), &taken).items[0].mark, Mark::Bad);

        // `missing`: was a `Bad` note, now no note at all.
        let mut gone = model();
        gone.set_app(0, "Nonexistent App");
        let cs = control_state(&gone, &status_all_ok());
        assert_eq!(cs.items[0].flag.as_deref(), Some("missing"));
        assert_eq!(cs.items[0].mark, Mark::Bad);

        // `other chord`: was a `Warn` note, now no note at all.
        let mut other = model();
        other.set_combo(0, "ctrl+alt+t");
        let mut rt = status_all_ok();
        rt.registered.insert("ctrl+alt+t".into(), Ok(()));
        let cs = control_state(&other, &rt);
        assert_eq!(cs.items[0].flag.as_deref(), Some("other chord"));
        assert_eq!(cs.items[0].mark, Mark::Warn);

        // `paused`: was a `Warn` note on every row, now no note at all.
        let paused = RuntimeStatus {
            paused: true,
            ..status_all_ok()
        };
        let cs = control_state(&model(), &paused);
        assert_eq!(cs.items[0].flag.as_deref(), Some("paused"));
        assert_eq!(cs.items[0].mark, Mark::Warn);
    }

    /// **The precedence is for the CELL. It does not delete the conditions it
    /// outranks.**
    ///
    /// The one combination the note deletion got wrong, and the reason
    /// `row_condition` keeps a `conditions` vector rather than a single
    /// `Option<String>`. Before the notes went quiet, a paused row whose app is
    /// missing carried two of them -- `Warn` for the pause and `Bad` for the
    /// app -- and `mark` was `Bad`. Afterwards `paused` won the cell, `missing`
    /// was never recorded anywhere, and folding the winning WORD alone reported
    /// `Warn` for a row that had been `Bad`: a silent severity drop on the one
    /// pair where the two problems are independent.
    ///
    /// It fails on that version, which is what makes it a regression pin rather
    /// than a restatement. The other three flag rows in
    /// `the_deleted_notes_did_not_take_the_marks_with_them` pass either way,
    /// because each of them is a row with exactly one condition.
    ///
    /// The controls are below it: pausing on its own is still `Warn` (so this
    /// is not "paused became Bad"), and the missing app on its own is still
    /// `Bad` with the cell reading `missing` (so the precedence itself did not
    /// move).
    #[test]
    fn a_paused_row_whose_app_is_missing_is_still_bad() {
        let mut m = model();
        m.set_app(0, "Nonexistent App");
        let paused = RuntimeStatus {
            paused: true,
            ..status_all_ok()
        };
        let cs = control_state(&m, &paused);
        assert_eq!(
            cs.items[0].flag.as_deref(),
            Some("paused"),
            "the cell still shows the highest-precedence word"
        );
        assert_eq!(
            cs.items[0].mark,
            Mark::Bad,
            "the app is missing whether or not beckon is paused, and that was \
             a `Mark::Bad` note before design 3.1 deleted the sentence"
        );

        // Control 1: the pause alone is a `Warn`, so the `Bad` above comes
        // from the app rather than from the pause changing severity.
        let cs = control_state(&model(), &paused);
        assert_eq!(cs.items[0].mark, Mark::Warn);

        // Control 2: the missing app alone still reads `missing` in the cell,
        // so the assertion above is about the FOLD and not about precedence.
        let cs = control_state(&m, &status_all_ok());
        assert_eq!(cs.items[0].flag.as_deref(), Some("missing"));
        assert_eq!(cs.items[0].mark, Mark::Bad);
    }

    /// **The two `Mark::Bad` flags must not share a tone.** This is the whole
    /// reason `flag_tone` is not derived from `Mark`: `in use` and `missing`
    /// are both `Bad` (see `flag_mark`), and the design draws one red and the
    /// other amber. A refactor that "simplifies" the tone away to severity
    /// makes them identical and this test goes red.
    ///
    /// It reads `flag_mark` rather than restating "both are Bad", so the two
    /// tables are compared instead of both being transcribed here.
    #[test]
    fn the_two_bad_flags_are_told_apart_by_tone() {
        assert_eq!(flag_mark("in use"), flag_mark("missing"));
        assert_eq!(flag_tone("in use"), FlagTone::Bad);
        assert_eq!(flag_tone("missing"), FlagTone::Warn);
        assert_ne!(flag_tone("in use"), flag_tone("missing"));
    }

    /// An unknown word must be silent rather than shout in a colour nobody
    /// chose for it.
    #[test]
    fn an_unknown_flag_word_is_neutral() {
        assert_eq!(flag_tone("something new"), FlagTone::Neutral);
        assert_eq!(flag_tone("other chord"), FlagTone::Neutral);
    }

    /// An app name that merely CONTAINS a flag word keeps it. Only a whole
    /// suffix behind the separator counts.
    #[test]
    fn an_app_named_after_a_flag_word_is_not_split() {
        assert_eq!(split_app_cell("Missing"), ("Missing", None));
        assert_eq!(split_app_cell("paused"), ("paused", None));
        assert_eq!(
            split_app_cell("Files In Use Manager"),
            ("Files In Use Manager", None)
        );
    }

    // ---------- control ids ----------

    #[test]
    fn ids_are_unique() {
        let mut seen: Vec<(&str, i32)> = Vec::new();
        for (name, id) in CONTROL_IDS {
            if let Some((other, _)) = seen.iter().find(|(_, v)| v == id) {
                panic!(
                    "control id {id} is claimed by both `{other}` and \
                     `{name}`. `layout` positions controls through \
                     `GetDlgItem`, which resolves a duplicate to the FIRST \
                     match -- so one of these is placed and the other is \
                     silently left at the origin."
                );
            }
            seen.push((name, *id));
        }
    }

    #[test]
    fn retired_ids_stay_retired() {
        for id in RETIRED_IDS {
            if let Some((name, _)) = CONTROL_IDS.iter().find(|(_, v)| v == id) {
                panic!(
                    "`{name}` reclaims retired id {id}. A probe built against \
                     an older binary would find a control it thinks it \
                     recognises."
                );
            }
        }
    }

    #[test]
    fn probe_pinned_ids_have_not_moved() {
        for (name, id) in PROBE_PINNED_IDS {
            let found = CONTROL_IDS.iter().find(|(n, _)| n == name);
            assert_eq!(
                found.map(|(_, v)| *v),
                Some(*id),
                "`crates/beckon-windows/examples/settings_probe.rs` \
                 hard-codes {id} for `{name}` and drives another process, so \
                 it cannot be recompiled into agreement"
            );
        }
    }

    /// The doc comment on `PROBE_PINNED_IDS` counts the list out loud, and
    /// **that count has been wrong twice** -- once when the list was fifteen
    /// and the prose said fourteen, and again when `measure_system` and
    /// `measure_about` transcribed twenty-nine more numbers and the prose
    /// still said fifteen. A number in prose beside a list is a claim nothing
    /// checks, so this checks it: the failure message names the line to edit.
    ///
    /// The two uniqueness assertions ride along because this list is
    /// maintained by hand from another file, which is the shape a copy-paste
    /// duplicate arrives in. A duplicated pair is not merely untidy -- it
    /// makes the count claim above true for the wrong reason.
    #[test]
    fn probe_pinned_ids_count_matches_its_doc() {
        assert_eq!(
            PROBE_PINNED_IDS.len(),
            48,
            "`PROBE_PINNED_IDS` changed length. Its own doc comment says how \
             many there are (\"these forty-eight are fixed points\") -- update \
             the word as well as the list"
        );
        let mut ids: Vec<i32> = PROBE_PINNED_IDS.iter().map(|(_, v)| *v).collect();
        ids.sort_unstable();
        let before = ids.len();
        ids.dedup();
        assert_eq!(before, ids.len(), "`PROBE_PINNED_IDS` repeats an id");
        let mut names: Vec<&str> = PROBE_PINNED_IDS.iter().map(|(n, _)| *n).collect();
        names.sort_unstable();
        let before = names.len();
        names.dedup();
        assert_eq!(before, names.len(), "`PROBE_PINNED_IDS` repeats a name");
    }

    // -- The System page ---------------------------------------------------

    /// **Built from components, never from a literal with backslashes in
    /// it.** These tests run on all three CI jobs, and `std::path` on Unix
    /// does not treat `\` as a separator -- so a hard-coded
    /// `C:\Users\me\apps.toml` is ONE file-name component there, and every
    /// assertion below would be checking `dir_of` against a string it never
    /// had to split. `collect` gives whatever the host's separator is, which
    /// is what `dir_of` appends too.
    fn sys_paths(log: bool) -> Paths {
        Paths {
            config: ["home", "me", "shortcuts", "apps.windows.toml"]
                .iter()
                .collect(),
            log: log.then(|| ["home", "me", "logs", "beckon-serve.log"].iter().collect()),
        }
    }

    fn sys(paths: &Paths) -> SystemInputs<'_> {
        SystemInputs {
            paused: false,
            autostart: Some(false),
            dark: true,
            opacity: OPACITY_DEFAULT,
            block: None,
            paths,
            log_bytes: Some(114_688),
        }
    }

    /// The two conditional rows are ABSENT, not present-and-off.
    ///
    /// The window reads `is_some()` on each to decide whether the control is
    /// on screen at all, so an implementation that returned
    /// `Some(FileRow{..})` with empty strings -- or `Some(false)` for a
    /// process that cannot autostart -- would draw a row with nothing in it
    /// rather than no row. That is the difference design §3.3 spells out for
    /// `Start with Windows` ("OMITTED, not greyed") and `Paths::log` ("omit
    /// the row rather than show a path that does not exist").
    #[test]
    fn a_capability_this_process_lacks_leaves_no_row_behind() {
        let p = sys_paths(false);
        let st = system_state(SystemInputs {
            autostart: None,
            ..sys(&p)
        });
        assert_eq!(st.autostart, None);
        assert_eq!(st.log, None);

        let p = sys_paths(true);
        let st = system_state(SystemInputs {
            autostart: Some(true),
            ..sys(&p)
        });
        assert_eq!(st.autostart, Some(true));
        assert_eq!(
            st.log,
            Some(FileRow {
                name: "beckon-serve.log".into(),
                value: "112 KB".into(),
            })
        );
    }

    /// A log path that names a file nothing has written says so, and does not
    /// say `0 bytes`.
    #[test]
    fn a_missing_log_is_not_an_empty_one() {
        let p = sys_paths(true);
        let st = system_state(SystemInputs {
            log_bytes: None,
            ..sys(&p)
        });
        assert_eq!(st.log.unwrap().value, "not found");
        let st = system_state(SystemInputs {
            log_bytes: Some(0),
            ..sys(&p)
        });
        assert_eq!(st.log.unwrap().value, "0 bytes");
    }

    /// The file rows say the name once and the location once.
    ///
    /// The row's LABEL is the file name, so a value slot repeating it would
    /// be the duplication design §3.3 deletes the `Config` / `Log` captions
    /// to avoid -- from the other end.
    #[test]
    fn the_config_row_shows_the_directory_not_the_file() {
        let p = sys_paths(false);
        let st = system_state(sys(&p));
        assert_eq!(st.config.name, "apps.windows.toml");
        assert!(
            !st.config.value.contains("apps.windows.toml"),
            "the value slot repeats the row's own label: {}",
            st.config.value
        );
        assert!(st.config.value.ends_with(std::path::MAIN_SEPARATOR));

        // A bare file name has no directory to show, and `.\` is not one.
        let bare = Paths {
            config: std::path::PathBuf::from("apps.windows.toml"),
            log: None,
        };
        let st = system_state(sys(&bare));
        assert_eq!(st.config.name, "apps.windows.toml");
        assert_eq!(st.config.value, "");
    }

    /// A blocked machine gets the reason in the slot the percentage was in,
    /// and the slider off.
    #[test]
    fn a_forced_off_slider_says_why_in_its_own_slot() {
        let p = sys_paths(false);
        let live = system_state(sys(&p)).transparency;
        assert_eq!(live, Transparency::On(OPACITY_DEFAULT));
        assert!(live.enabled());
        assert_eq!(live.slot(), "96%");

        for b in [
            crate::theme::TransparencyBlock::HighContrast,
            crate::theme::TransparencyBlock::RemoteSession,
            crate::theme::TransparencyBlock::SystemSetting,
        ] {
            let off = system_state(SystemInputs {
                block: Some(b),
                ..sys(&p)
            })
            .transparency;
            assert_eq!(off, Transparency::Off(b));
            assert!(!off.enabled());
            assert_eq!(off.slot(), b.reason());
            // The slot never holds a percentage AND a reason: one control,
            // one string.
            assert!(!off.slot().contains('%'));
        }
    }

    /// The stored value is clamped on the way in, not trusted.
    ///
    /// It comes out of `HKCU\Software\beckon`, which anything can write, and
    /// the slider's own range is 85..=100 -- so an unclamped 0 would set the
    /// window to an alpha the user could not reverse from the control that
    /// set it.
    #[test]
    fn opacity_out_of_range_is_pulled_back_into_it() {
        assert_eq!(clamp_opacity(0), OPACITY_MIN);
        assert_eq!(clamp_opacity(84), OPACITY_MIN);
        assert_eq!(clamp_opacity(85), 85);
        assert_eq!(clamp_opacity(100), 100);
        assert_eq!(clamp_opacity(255), OPACITY_MAX);
        let p = sys_paths(false);
        assert_eq!(
            system_state(SystemInputs {
                opacity: 3,
                ..sys(&p)
            })
            .transparency,
            Transparency::On(OPACITY_MIN)
        );
    }

    /// 100 % is fully opaque and the range is monotone.
    ///
    /// The rounding matters at this scale: the whole visible band is
    /// 218..=255, so a truncating conversion biases every one of the sixteen
    /// positions one step toward transparent.
    #[test]
    fn the_top_of_the_slider_is_actually_opaque() {
        assert_eq!(opacity_alpha(100), 255);
        assert_eq!(opacity_alpha(96), 245);
        assert_eq!(opacity_alpha(85), 217);
        let mut prev = 0u8;
        for p in OPACITY_MIN..=OPACITY_MAX {
            let a = opacity_alpha(p);
            assert!(a > prev, "alpha did not rise at {p}%");
            prev = a;
        }
    }

    #[test]
    fn a_size_reads_the_way_explorer_reads_it() {
        assert_eq!(size_label(0), "0 bytes");
        assert_eq!(size_label(1023), "1023 bytes");
        assert_eq!(size_label(1024), "1 KB");
        assert_eq!(size_label(114_688), "112 KB");
        // `roll_if_oversized` rolls at 5 MiB and caps the pair at 10.
        assert_eq!(size_label(5 * 1024 * 1024), "5.0 MB");
        assert_eq!(size_label(5_400_000), "5.1 MB");
    }

    /// **`1024 KB` is not a unit Explorer has**, and the old code printed it
    /// for the 512 bytes below 1 MiB: the `bytes < MB` branch was decided on
    /// the raw count, before the rounding that carries into the next unit.
    /// Reachable on any `--log` file on its way to `roll_if_oversized`'s
    /// 5 MiB.
    ///
    /// The 1023 either side are the controls: the band is exactly the one the
    /// carry creates, so this is not "KB stops at some other number".
    #[test]
    fn a_size_that_rounds_up_to_the_next_unit_is_named_in_that_unit() {
        assert_eq!(size_label(1_048_063), "1023 KB");
        assert_eq!(size_label(1_048_064), "1.0 MB");
        assert_eq!(size_label(1_048_575), "1.0 MB");
        assert_eq!(size_label(1_048_576), "1.0 MB");
    }

    // -- The About page ----------------------------------------------------

    /// The scoop shape, built from components for `sys_paths`' reason: a
    /// literal with backslashes is one file-name component on the two CI jobs
    /// that are not Windows.
    fn exe_path() -> std::path::PathBuf {
        [
            "home",
            "me",
            "scoop",
            "apps",
            "beckon",
            "current",
            "beckon-serve.exe",
        ]
        .iter()
        .collect()
    }

    /// The clock half on its own -- identity says `Same`, which is what a
    /// machine whose launch path is not a junction reports.
    fn about(exe: &std::path::Path, started: Option<SystemTime>, disk: ImageOnDisk) -> AboutState {
        about_with(exe, started, disk, ImageIdentity::Same)
    }

    fn about_with(
        exe: &std::path::Path,
        started: Option<SystemTime>,
        disk: ImageOnDisk,
        identity: ImageIdentity,
    ) -> AboutState {
        about_state(AboutInputs {
            version: "0.9.3",
            target: "aarch64-pc-windows-msvc",
            exe: Some(exe),
            started,
            disk,
            identity,
            licence: "MIT OR Apache-2.0",
        })
    }

    use std::time::{Duration, SystemTime};

    fn t(secs: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(secs)
    }

    /// The whole point of the row, in four states.
    ///
    /// The recorded failure is a process running an image that is no longer
    /// the one on disk, with `--version` and the `current` junction both
    /// reporting the new one. `Replaced` is the state that has to be loud.
    #[test]
    fn the_location_row_speaks_only_when_the_image_really_moved() {
        let exe = exe_path();
        let start = t(1_000);

        // Written before the process started: the ordinary state, and the row
        // says nothing beyond the path.
        let st = about(&exe, Some(start), ImageOnDisk::Written(t(900)));
        assert_eq!(st.image, ImageAge::Current);
        assert_eq!(st.location.shown, st.location.copy);

        // Written after: a scoop update landed under a running daemon.
        let st = about(&exe, Some(start), ImageOnDisk::Written(t(1_100)));
        assert_eq!(st.image, ImageAge::Replaced);
        assert!(
            st.location.shown.contains("restart"),
            "the one state that must be loud said nothing: {}",
            st.location.shown
        );

        // The version directory was cleaned up under us.
        let st = about(&exe, Some(start), ImageOnDisk::Gone);
        assert_eq!(st.image, ImageAge::Missing);
        assert!(st.location.shown.contains("no longer on disk"));

        // Neither timestamp could be read: no claim either way.
        let st = about(&exe, None, ImageOnDisk::Written(t(1_100)));
        assert_eq!(st.image, ImageAge::Unknown);
        assert_eq!(st.location.shown, st.location.copy);
    }

    /// Equality is not a replacement. The image must exist before it can be
    /// executed, so `written == started` is a fast machine, not an update.
    #[test]
    fn an_image_written_in_the_same_tick_is_not_a_replacement() {
        assert_eq!(
            image_age(
                Some(t(1_000)),
                ImageOnDisk::Written(t(1_000)),
                ImageIdentity::Same
            ),
            ImageAge::Current
        );
    }

    /// **The a14 incident, as a test, with the artifact's own numbers.**
    ///
    /// Measured 2026-08-15 out of the v0.9.0 arm64 release zip: the stored
    /// `LastWriteTime` of `beckon.exe` is `2026-08-12T22:37:18`. The watchdog
    /// started the stale process at 05:40:01 and scoop unpacked four seconds
    /// later, which cannot precede the artifact -- so `written < started` no
    /// matter what a14's clock reads. Here that is `w = 900`, `start = 1_000`.
    ///
    /// **The clock half answers `Current`, and the row is silent.** That is
    /// the defect: this is the one timeline the row was built for. The
    /// identity half is what makes the same inputs speak, and the second
    /// assertion is the only difference between the two calls.
    ///
    /// Falsified by deleting the `Diverged` arm from `image_age`: the second
    /// assertion fails and the first still passes, which is exactly how this
    /// went unnoticed for a day.
    #[test]
    fn the_a14_timeline_is_silent_on_the_clock_and_loud_on_identity() {
        let exe = exe_path();
        let (start, written) = (t(1_000), t(900));

        let clock_only = about_with(
            &exe,
            Some(start),
            ImageOnDisk::Written(written),
            ImageIdentity::Same,
        );
        assert_eq!(
            clock_only.image,
            ImageAge::Current,
            "the clock comparison is expected to MISS this; if it starts \
             catching it, the measurement in `image_age`'s doc has changed \
             and both should be revisited"
        );
        assert_eq!(clock_only.location.shown, clock_only.location.copy);

        let with_identity = about_with(
            &exe,
            Some(start),
            ImageOnDisk::Written(written),
            ImageIdentity::Diverged,
        );
        assert_eq!(with_identity.image, ImageAge::Replaced);
        assert!(
            with_identity.location.shown.contains("restart"),
            "the incident the row exists for said nothing: {}",
            with_identity.location.shown
        );
    }

    /// `Same` is not evidence against an in-place overwrite, and `Gone`
    /// outranks everything.
    ///
    /// Three orderings in one place because each is a decision rather than a
    /// consequence: identity beats the clock (it is the reliable half), the
    /// clock still runs under `Same` (a `cargo build` over a running binary
    /// never moves the path), and `Gone` beats both (a path that resolves to
    /// nothing makes either comparison meaningless).
    #[test]
    fn the_two_halves_of_the_verdict_do_not_shadow_each_other() {
        assert_eq!(
            image_age(
                Some(t(1_000)),
                ImageOnDisk::Written(t(1_100)),
                ImageIdentity::Same
            ),
            ImageAge::Replaced
        );
        assert_eq!(
            image_age(None, ImageOnDisk::Unknown, ImageIdentity::Diverged),
            ImageAge::Replaced
        );
        assert_eq!(
            image_age(Some(t(1_000)), ImageOnDisk::Gone, ImageIdentity::Diverged),
            ImageAge::Missing
        );
        // Unknown identity leaves the clock exactly as it was.
        assert_eq!(
            image_age(
                Some(t(1_000)),
                ImageOnDisk::Written(t(900)),
                ImageIdentity::Unknown
            ),
            ImageAge::Current
        );
    }

    /// The fail-safe property, which is what lets an unmeasured Win32 reading
    /// ship at all.
    ///
    /// If `QueryFullProcessImageNameW` turns out to return the UNRESOLVED
    /// launch path, `about_now` canonicalises both sides and hands this
    /// function two equal strings -- so the answer is `Same`, the verdict
    /// falls back to the clock, and nothing on screen changes. A missing
    /// side is `Unknown` for the same reason: `scoop cleanup` deletes the
    /// version directory a stale process is running from, and a path that
    /// will not canonicalise must not be read as a divergence.
    #[test]
    fn an_unresolvable_or_identical_image_path_never_cries_wolf() {
        let a: std::path::PathBuf = ["v", "0.9.0", "beckon.exe"].iter().collect();
        let b: std::path::PathBuf = ["v", "0.8.0", "beckon.exe"].iter().collect();
        assert_eq!(image_identity(Some(&a), Some(&a)), ImageIdentity::Same);
        assert_eq!(image_identity(Some(&a), Some(&b)), ImageIdentity::Diverged);
        assert_eq!(image_identity(None, Some(&a)), ImageIdentity::Unknown);
        assert_eq!(image_identity(Some(&a), None), ImageIdentity::Unknown);
        assert_eq!(image_identity(None, None), ImageIdentity::Unknown);
        // Windows paths are case-insensitive, and both sides come back from
        // the same API -- but a fold that only worked by luck would be a
        // divergence reported on every open.
        let upper: std::path::PathBuf = ["V", "0.9.0", "BECKON.EXE"].iter().collect();
        assert_eq!(image_identity(Some(&a), Some(&upper)), ImageIdentity::Same);
    }

    /// What is copied is the payload, never the annotated string.
    ///
    /// A path with `(updated on disk, restart to run it)` glued to it fails
    /// in the only two places a copied path goes: an Explorer address bar and
    /// a bug report.
    #[test]
    fn copying_a_row_gives_the_payload_and_not_the_verdict() {
        let exe = exe_path();
        let st = about(&exe, Some(t(1_000)), ImageOnDisk::Written(t(1_100)));
        assert_eq!(copy_text(&st, Field::Location), exe.to_string_lossy());
        assert!(!copy_text(&st, Field::Location).contains('('));
        assert_eq!(
            copy_text(&st, Field::Build),
            "aarch64-pc-windows-msvc \u{b7} 1970-01-01",
            "the Build row carries the running image's date since 2026-08-16"
        );
        assert_eq!(copy_text(&st, Field::Licence), "MIT OR Apache-2.0");
        // Every row copies exactly what it shows, EXCEPT the one that has a
        // verdict to carry -- so the exception is one row and is testable as
        // one.
        assert_eq!(copy_text(&st, Field::Build), st.build.shown);
        assert_eq!(copy_text(&st, Field::Licence), st.licence.shown);
        assert_ne!(copy_text(&st, Field::Location), st.location.shown);
    }

    /// A path that could not be read is a word, not an empty row.
    #[test]
    fn an_unreadable_exe_path_still_fills_the_row() {
        let st = about_state(AboutInputs {
            version: "0.9.3",
            target: "aarch64-pc-windows-msvc",
            exe: None,
            started: None,
            disk: ImageOnDisk::Unknown,
            identity: ImageIdentity::Unknown,
            licence: "MIT OR Apache-2.0",
        });
        assert_eq!(st.location.shown, "unknown");
        assert_eq!(st.name, "beckon 0.9.3");
    }

    /// The three links go somewhere, the two files do not, and nothing goes
    /// anywhere over plain http.
    #[test]
    fn every_link_target_has_a_url_and_every_file_target_has_none() {
        assert_eq!(Target::Config.url(), None);
        assert_eq!(Target::Log.url(), None);
        for t in [Target::Github, Target::Releases, Target::BugReport] {
            let u = t.url().unwrap_or_else(|| panic!("{t:?} has no url"));
            assert!(
                u.starts_with("https://github.com/xom11/beckon"),
                "{t:?} points outside the project: {u}"
            );
        }
        // Three buttons, three destinations. A copy-paste that left two
        // captions pointing at one page is the defect this catches.
        assert_ne!(Target::Github.url(), Target::Releases.url());
        assert_ne!(Target::Github.url(), Target::BugReport.url());
        assert_ne!(Target::Releases.url(), Target::BugReport.url());
    }

    /// The disclosure's second half is the half that cannot be drawn.
    ///
    /// Design §3.4: an unsigned process holding a `WH_KEYBOARD_LL` hook owes
    /// the reader both when it holds it and what it does not keep, and the
    /// second is a negative claim with no icon, colour or control state
    /// available to make it. This pins the sentence against being trimmed to
    /// the half a status dot could have carried.
    #[test]
    fn the_hook_disclosure_keeps_both_halves() {
        assert!(HOOK_DISCLOSURE.contains("Caps Lock"));
        assert!(HOOK_DISCLOSURE.contains("recording a shortcut"));
        assert!(HOOK_DISCLOSURE.contains("keeps no record of what you type"));
        assert!(
            HOOK_DISCLOSURE.is_ascii(),
            "every display string in this window is ASCII: a face that lacks \
             a glyph draws a box"
        );
    }
}
