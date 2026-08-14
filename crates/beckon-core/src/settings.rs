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
pub const FLAGS: [&str; 4] = ["paused", "key in use", "not installed", "custom"];

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
/// that cannot hear "not installed" is worse than a flag that is not
/// coloured.
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
/// genuinely named `... key in use`, which is not a name.
/// How a flag is COLOURED. Not the same question as `Mark`, and that is the
/// whole reason it exists.
///
/// `key in use` and `not installed` are both `Mark::Bad` -- each pushes a
/// `Bad` note and the row's mark is the worst of them -- so severity cannot
/// tell them apart, while the design deliberately does: a chord another
/// program has taken is red, an app beckon cannot find is amber. Severity
/// answers "how bad"; this answers "which of the four words is it", which is
/// a property of the closed vocabulary rather than a second opinion about
/// how serious anything is.
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
        "key in use" => FlagTone::Bad,
        "not installed" | "paused" => FlagTone::Warn,
        _ => FlagTone::Neutral,
    }
}

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
/// **`page` is taken and ignored, and that is this pass's decision rather than
/// a leftover.** Task 4 shipped `external_change && page == BANNER_PAGE`, which
/// drew the announcement on Shortcuts alone -- and Save is chrome: on all four
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
/// **Task 6 narrows this again, and this is deliberately the wide version until
/// it does.** The design's own answer (spec §5) is a warn dot on the Shortcuts
/// pill: the announcement goes back to `BANNER_PAGE`, and the dot is what
/// carries the fact to the other three doors. Until that dot exists there is
/// nothing else to carry it. The cost of being wide is that Keyboard, System and
/// About each spend a band of height on the banner in a state that is rare --
/// a layout cost, not a correctness one, and the parameter is kept in the
/// signature so Task 6 is one edit to this body and to nothing else.
pub fn banner_shown(external_change: bool, _page: Page) -> bool {
    external_change
}

/// The door the external-change announcement is ABOUT.
///
/// Not currently where it is drawn -- `banner_shown` draws it on every page
/// until Task 6 -- but the two are the same question again the moment that
/// task lands, and this is the door its warn dot goes on: the file that moved
/// is the shortcut table, so the pill that has to grow a dot is Shortcuts'.
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

/// A row on About whose value can be copied.
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
        Availability::CaptureSawNothing => (
            Mark::Warn,
            "Windows handled that shortcut itself, so beckon never saw it. A few shortcuts, like \
             Win+L, cannot be reassigned by any program."
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
/// order in which `flag` is claimed below IS the precedence:
/// `paused` > `key in use` > `not installed` > `custom` > none. `paused`
/// sits above the registration map deliberately: `serve` CLEARS that map
/// when it pauses, so consulting the map first would render every row as
/// "not registered yet" and never say why.
///
/// `mark` is derived from the notes at the end rather than assigned along
/// the way, which is what makes "the list and the editor cannot disagree"
/// true by construction instead of by discipline.
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
    let mut flag: Option<String> = None;
    let combo = Combo::parse(&r.combo);

    // 1. The key.
    if rt.paused {
        flag = Some("paused".into());
        notes.push(Note {
            mark: Mark::Warn,
            text: "beckon is paused, so no shortcut is active.".into(),
        });
    } else if let Ok(c) = &combo {
        match rt.registered.get(&c.canonical()) {
            Some(Ok(())) => notes.push(Note {
                mark: Mark::Ok,
                text: "Registered and working.".into(),
            }),
            Some(Err(_)) => {
                flag = Some("key in use".into());
                notes.push(Note {
                    mark: Mark::Bad,
                    text: "Another program already has this shortcut.".into(),
                });
            }
            // In the file but not in the last registration pass -- an edit
            // that has not been saved and reloaded yet. Honest, not a fault.
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
                flag.get_or_insert_with(|| "not installed".into());
                notes.push(Note {
                    mark: Mark::Bad,
                    text: "No installed app has this name.".into(),
                });
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
            flag.get_or_insert_with(|| "custom".into());
            notes.push(Note {
                mark: Mark::Warn,
                text: "Uses a different chord.".into(),
            });
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

    let mark = if notes.iter().any(|n| n.mark == Mark::Bad) {
        Mark::Bad
    } else if notes.iter().any(|n| n.mark == Mark::Warn) {
        Mark::Warn
    } else if notes.iter().any(|n| n.mark == Mark::Unknown) {
        Mark::Unknown
    } else {
        Mark::Ok
    };
    (mark, flag, notes)
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
    Reset,
}

impl DefaultButton {
    /// Every variant, for exhaustive tests. A new button added to the enum
    /// and forgotten here weakens those tests silently, so the array is
    /// length-annotated: adding a variant without extending it fails to
    /// compile.
    pub const ALL: [DefaultButton; 9] = [
        DefaultButton::Save,
        DefaultButton::Add,
        DefaultButton::Remove,
        DefaultButton::OpenFile,
        DefaultButton::Close,
        DefaultButton::Reload,
        DefaultButton::KeepMine,
        DefaultButton::Record,
        DefaultButton::Reset,
    ];

    /// Where the ring rests, and the one button `default_button` will fall
    /// back to. Always on screen, and it is what `DM_GETDEFID` answers before
    /// focus has ever touched a button.
    pub const HOME: DefaultButton = DefaultButton::Save;

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
            // The command bar is chrome: `Open config file`, `Close` and
            // `Save` are created once, placed by `layout` on every page, and
            // never hidden. `Save` in particular MUST stay on all four --
            // it is `HOME`, the button `default_button` falls back to.
            DefaultButton::Save | DefaultButton::OpenFile | DefaultButton::Close => true,
            DefaultButton::Reload | DefaultButton::KeepMine => banner_shown(external_change, page),
            // Shortcuts-page controls. `Add` and `Remove` sit on the list's
            // head row; `Record` and `Reset` close the editor strip's second
            // line.
            DefaultButton::Add
            | DefaultButton::Remove
            | DefaultButton::Record
            | DefaultButton::Reset => page == Page::Shortcuts,
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
                // hole.** While armed the window greys `Reset` (two writers
                // on one value is what §C.4 forbids) while this still calls
                // it pressable, so for those seconds the two disagree. It
                // cannot be observed: the `WH_KEYBOARD_LL` hook swallows
                // every keystroke while a capture is armed and the window is
                // foreground, so no Enter reaches the dialog manager to ask;
                // and if the window is NOT foreground, all three of spec
                // F.4's focus layers have already disarmed. Modelling it here
                // would mean `ControlState` carrying a runtime fact that
                // exists for seconds at a time.
                DefaultButton::Record | DefaultButton::Reset => st.editable && st.detail.is_some(),
                // The two escape routes are enabled in every state,
                // including read only -- that is what makes them escapes.
                // The banner's two answers are enabled whenever the banner is
                // up, which `visible` above has already established.
                DefaultButton::OpenFile
                | DefaultButton::Close
                | DefaultButton::Reload
                | DefaultButton::KeepMine => true,
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
/// door makes Enter add a row the user cannot see. `HOME`'s exemption
/// survives that unchanged, because `Save` is on every page -- if it ever
/// stops being, this early return is the line that breaks.
pub fn default_button(
    current: DefaultButton,
    st: &ControlState,
    external_change: bool,
    page: Page,
) -> DefaultButton {
    if current == DefaultButton::HOME {
        return DefaultButton::HOME;
    }
    if current.pressable(st, external_change, page) {
        current
    } else {
        DefaultButton::HOME
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
    ("LBL_SHORTCUT", 1017),
    ("LBL_APP", 1018),
    ("GRP_KEYBOARD", 1019),
    ("LBL_SECTION", 1020),
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
    ("RESET", 1033),
    ("GRP_EDITOR", 1034),
    ("LBL_COUNT", 1035),
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
];

/// Ids that were used, are not any more, and must never be reused.
///
/// 1009-1011 were the three `Tapping Caps alone` radios. A probe built
/// against an older binary would find a control it thinks it recognises.
pub const RETIRED_IDS: &[i32] = &[1009, 1010, 1011];

/// The ids `crates/beckon-windows/examples/settings_probe.rs` hard-codes.
///
/// It drives ANOTHER process across a process boundary, so it cannot link
/// this crate and cannot be recompiled into agreement: these fifteen are
/// fixed points, and `probe_pinned_ids_have_not_moved` is what says so out
/// loud.
///
/// Fifteen, and the spec's three **pinned** rows
/// (`docs/superpowers/specs/2026-08-14-four-doors-phase-0-spec.md:141-145`
/// -- 1001-1008, 1012/1013, 1028-1031) account for only fourteen of them.
/// The fifteenth is `IDC_TAP 1025`, which that same table files under
/// `1014-1027 | in use, unpinned` at spec line 144 while
/// `settings_probe.rs:249` hard-codes it regardless. Counted from the probe
/// rather than from the spec:
/// `grep -c "const IDC_" crates/beckon-windows/examples/settings_probe.rs`
/// is 15.
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
    ("MOD_CTRL", 1028),
    ("MOD_WIN", 1029),
    ("MOD_ALT", 1030),
    ("MOD_SHIFT", 1031),
];

#[cfg(test)]
mod tests {
    use super::*;
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

    #[test]
    fn an_app_missing_from_a_scanned_catalog_is_marked_bad() {
        let mut m = model();
        m.set_app(0, "Nonexistent App");
        m.selected = Some(0);
        let cs = control_state(&m, &status_all_ok());
        let note = cs
            .detail
            .unwrap()
            .notes
            .into_iter()
            .find(|n| n.text.contains("installed"))
            .unwrap();
        assert_eq!(note.mark, Mark::Bad);
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

    #[test]
    fn a_healthy_row_says_registered_and_working_and_nothing_about_the_catalog() {
        let mut m = model();
        m.selected = Some(0);
        let notes = control_state(&m, &status_all_ok()).detail.unwrap().notes;
        assert_eq!(notes.len(), 1, "one sentence, not a report: {notes:?}");
        assert_eq!(notes[0].text, "Registered and working.");
        assert_eq!(notes[0].mark, Mark::Ok);
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

    /// The list mark and the editor note are computed by ONE function, so they
    /// cannot contradict each other -- which they can today.
    #[test]
    fn the_list_and_the_editor_cannot_disagree_about_a_row() {
        let mut m = model();
        m.set_app(0, "Nonexistent App");
        m.selected = Some(0);
        let cs = control_state(&m, &status_all_ok());
        assert!(cs.items[0].flag.is_some(), "the list must show the problem");
        let notes = cs.detail.unwrap().notes;
        assert!(
            notes.iter().any(|n| n.mark == Mark::Bad),
            "and the editor must agree it is one"
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
        assert_eq!(cs.items[0].flag.as_deref(), Some("key in use"));
        assert_eq!(cs.items[0].mark, Mark::Bad);
        let notes = cs.detail.unwrap().notes;
        assert!(
            notes
                .iter()
                .any(|n| n.text == "Another program already has this shortcut."),
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
    /// Highest wins: paused, then key in use, then not installed, then
    /// custom. `paused` sits above the registration map on purpose --
    /// `serve` CLEARS that map when it pauses, so a paused row would
    /// otherwise read as "not registered yet" and say nothing about why.
    #[test]
    fn the_flag_precedence_is_paused_then_key_in_use_then_not_installed_then_custom() {
        let mut m = model();
        m.set_caps(true); // realistic config; `custom` does not need this on
        m.set_combo(0, "ctrl+alt+t"); // not the caps_hold chord
        m.set_app(0, "Nonexistent App"); // not in the catalog
        let mut rt = status_all_ok();
        rt.registered
            .insert("ctrl+alt+t".into(), Err("already taken".into()));
        rt.paused = true;

        let flag = |m: &Model, rt: &RuntimeStatus| control_state(m, rt).items[0].flag.clone();
        assert_eq!(flag(&m, &rt).as_deref(), Some("paused"));
        rt.paused = false;
        assert_eq!(flag(&m, &rt).as_deref(), Some("key in use"));
        rt.registered.insert("ctrl+alt+t".into(), Ok(()));
        assert_eq!(flag(&m, &rt).as_deref(), Some("not installed"));
        m.set_app(0, "Terminal");
        assert_eq!(flag(&m, &rt).as_deref(), Some("custom"));
        m.set_combo(0, "ctrl+super+alt+t");
        assert_eq!(flag(&m, &rt), None);
    }

    /// `custom` answers one question: "does this combo match
    /// `keyboard.caps_hold`?" -- decided purely by comparing modifiers, with
    /// NO dependency on whether `keyboard.caps` itself is on. A gate on
    /// `keyboard.caps` was tried and reverted: the README's own
    /// `"ctrl+super+alt+shift+t" = "Telegram Web"` example, cited by the
    /// spec as the reason this flag exists, ships with no `keyboard.caps`
    /// block at all, so the gate would have left the spec's own example
    /// silently unflagged.
    #[test]
    fn custom_follows_caps_hold_regardless_of_whether_caps_is_on() {
        let mut m = model();
        m.set_combo(0, "ctrl+alt+t"); // not the default caps_hold (ctrl+super+alt)
        let mut rt = status_all_ok();
        rt.registered.insert("ctrl+alt+t".into(), Ok(()));
        assert_eq!(
            control_state(&m, &rt).items[0].flag.as_deref(),
            Some("custom"),
            "Caps off must not silence `custom`"
        );

        m.set_caps(true);
        assert_eq!(
            control_state(&m, &rt).items[0].flag.as_deref(),
            Some("custom"),
            "and turning Caps on changes nothing about it"
        );

        m.keyboard.caps_hold = Chord::parse("ctrl+alt").unwrap();
        assert_eq!(
            control_state(&m, &rt).items[0].flag,
            None,
            "the chord is configurable, so `custom` must follow it"
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
            default_button(DefaultButton::Reload, &st, false, Page::Shortcuts),
            DefaultButton::Save,
            "the banner is down, so Enter must not reach Reload"
        );
        assert_eq!(
            default_button(DefaultButton::KeepMine, &st, false, Page::Shortcuts),
            DefaultButton::Save
        );
    }

    #[test]
    fn reload_keeps_the_default_while_the_banner_is_up() {
        // The other half: the fix must not take the ring off a button the
        // user has genuinely tabbed to.
        let st = busy_state();
        assert_eq!(
            default_button(DefaultButton::Reload, &st, true, Page::Shortcuts),
            DefaultButton::Reload
        );
        assert_eq!(
            default_button(DefaultButton::KeepMine, &st, true, Page::Shortcuts),
            DefaultButton::KeepMine
        );
    }

    #[test]
    fn a_disabled_button_loses_the_default() {
        let rest = rest_state();
        assert!(!rest.remove_enabled, "precondition: nothing is selected");
        assert_eq!(
            default_button(DefaultButton::Remove, &rest, false, Page::Shortcuts),
            DefaultButton::Save
        );
        // And keeps it while it is live, so this is a real test and not one
        // that passes because everything falls back.
        let busy = busy_state();
        assert!(busy.remove_enabled, "precondition: a row is selected");
        assert_eq!(
            default_button(DefaultButton::Remove, &busy, false, Page::Shortcuts),
            DefaultButton::Remove
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
            default_button(DefaultButton::Save, &rest, false, Page::Shortcuts),
            DefaultButton::Save
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
        assert!(!DefaultButton::Reset.pressable(&rest, false, Page::Shortcuts));
        assert_eq!(
            default_button(DefaultButton::Record, &rest, false, Page::Shortcuts),
            DefaultButton::Save,
            "Enter must not reach a greyed Record"
        );

        let busy = busy_state();
        assert!(busy.detail.is_some(), "precondition: a row is selected");
        assert!(DefaultButton::Record.pressable(&busy, false, Page::Shortcuts));
        assert!(DefaultButton::Reset.pressable(&busy, false, Page::Shortcuts));
        assert_eq!(
            default_button(DefaultButton::Record, &busy, false, Page::Shortcuts),
            DefaultButton::Record
        );

        // A file that did not parse has a Model behind neither, so both are
        // off for the same reason every other mutating control is.
        let ro = unreadable_state(explain("\"ctrl+alt+t\" = \"A\"\noops\n"));
        assert!(!DefaultButton::Record.pressable(&ro, false, Page::Shortcuts));
        assert!(!DefaultButton::Reset.pressable(&ro, false, Page::Shortcuts));
    }

    #[test]
    fn the_read_only_state_leaves_the_default_on_save_or_an_escape() {
        // A file that did not parse: everything that mutates is off, and the
        // two escape routes are the only live buttons. The ring must land on
        // one of those or on Save, never on Add.
        let ro = unreadable_state(explain("\"ctrl+alt+t\" = \"A\"\noops\n"));
        assert_eq!(
            default_button(DefaultButton::Add, &ro, false, Page::Shortcuts),
            DefaultButton::Save
        );
        assert_eq!(
            default_button(DefaultButton::Close, &ro, false, Page::Shortcuts),
            DefaultButton::Close
        );
        assert_eq!(
            default_button(DefaultButton::OpenFile, &ro, false, Page::Shortcuts),
            DefaultButton::OpenFile
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
                        let got = default_button(b, st, external, page);
                        assert!(
                            got.visible(external, page),
                            "{b:?} -> {got:?} is off screen \
                             (external_change={external}, page={page:?})"
                        );
                        assert!(
                            got.pressable(st, external, page) || got == DefaultButton::HOME,
                            "{b:?} -> {got:?} is disabled and is not HOME"
                        );
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
            DefaultButton::Reset,
        ] {
            assert!(
                b.visible(false, Page::Shortcuts),
                "precondition: {b:?} is on screen on its own page"
            );
            for page in [Page::Keyboard, Page::System, Page::About] {
                assert!(!b.visible(false, page), "{b:?} is drawn on {page:?}");
                assert_eq!(
                    default_button(b, &busy, false, page),
                    DefaultButton::Save,
                    "Enter must not reach {b:?} from {page:?}"
                );
            }
        }
    }

    /// The banner's two follow `external_change` and nothing else, on every
    /// door.
    ///
    /// **This test used to assert the opposite half** -- that
    /// `visible(true, Page::Keyboard)` is `false` -- and that was the defect,
    /// not the design. Save is chrome and `apply_settings` writes without a
    /// prompt, so an announcement drawn on Shortcuts alone left three pages
    /// where the file could be overwritten with nothing on screen saying it
    /// had moved. Task 6's warn dot is what lets the drawing narrow again; the
    /// assertion narrows with it.
    #[test]
    fn the_banners_two_follow_the_change_on_every_door() {
        let busy = busy_state();
        for b in [DefaultButton::Reload, DefaultButton::KeepMine] {
            for page in [Page::Shortcuts, Page::Keyboard, Page::System, Page::About] {
                assert!(b.visible(true, page), "{b:?} is hidden on {page:?}");
                assert!(!b.visible(false, page), "no change to announce on {page:?}");
                // The ring may rest on a banner button from any door now,
                // which is the other half of "it is on screen there".
                assert_eq!(default_button(b, &busy, true, page), b);
                assert_eq!(default_button(b, &busy, false, page), DefaultButton::Save);
            }
        }
        for page in [Page::Shortcuts, Page::Keyboard, Page::System, Page::About] {
            assert!(banner_shown(true, page));
            assert!(!banner_shown(false, page));
        }
    }

    /// The regression the four doors opened, stated as the thing that closes
    /// it: on every page Save is pressable, the announcement is drawn too.
    ///
    /// Written as a pairing with `visible` rather than as a restatement of
    /// `banner_shown`, because the defect was exactly a disagreement between
    /// those two -- Save reachable where the warning was not.
    #[test]
    fn save_is_never_pressable_where_the_announcement_is_not_drawn() {
        for page in [Page::Shortcuts, Page::Keyboard, Page::System, Page::About] {
            assert!(
                DefaultButton::Save.visible(true, page),
                "Save is chrome; if it is not on {page:?} this test measures nothing"
            );
            assert!(
                banner_shown(true, page),
                "Save can overwrite an externally changed file from {page:?} \
                 with nothing on screen saying it moved"
            );
            assert!(
                DefaultButton::Reload.visible(true, page)
                    && DefaultButton::KeepMine.visible(true, page),
                "the announcement is drawn on {page:?} without its two answers"
            );
        }
    }

    /// `BANNER_PAGE` is Task 6's target, and it has to name a door the strip
    /// really has -- the warn dot goes on that pill, and the announcement goes
    /// back to that page with it.
    #[test]
    fn the_banner_page_is_the_shortcut_table_it_is_about() {
        assert_eq!(BANNER_PAGE, Page::Shortcuts);
        assert!(banner_shown(true, BANNER_PAGE));
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

    /// The command bar is chrome, and `HOME` above all: `default_button`'s
    /// fallback is `Save`, so a `Save` that could be hidden would make the
    /// fallback itself unreachable and every assertion in this section
    /// vacuous.
    #[test]
    fn the_command_bar_is_on_every_page() {
        for page in [Page::Shortcuts, Page::Keyboard, Page::System, Page::About] {
            for b in [
                DefaultButton::Save,
                DefaultButton::OpenFile,
                DefaultButton::Close,
            ] {
                assert!(b.visible(false, page), "{b:?} is missing from {page:?}");
            }
        }
        assert!(DefaultButton::HOME.visible(false, Page::About));
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
    #[test]
    fn every_flag_row_condition_produces_is_in_the_table() {
        for f in ["paused", "key in use", "not installed", "custom"] {
            assert!(FLAGS.contains(&f), "{f:?} missing from FLAGS");
        }
    }

    /// **The two `Mark::Bad` flags must not share a tone.** This is the whole
    /// reason `flag_tone` is not derived from `Mark`: `key in use` and
    /// `not installed` are both `Bad`, and the design draws one red and the
    /// other amber. A refactor that "simplifies" the tone away to severity
    /// makes them identical and this test goes red.
    #[test]
    fn the_two_bad_flags_are_told_apart_by_tone() {
        assert_eq!(flag_tone("key in use"), FlagTone::Bad);
        assert_eq!(flag_tone("not installed"), FlagTone::Warn);
        assert_ne!(flag_tone("key in use"), flag_tone("not installed"));
    }

    /// An unknown word must be silent rather than shout in a colour nobody
    /// chose for it.
    #[test]
    fn an_unknown_flag_word_is_neutral() {
        assert_eq!(flag_tone("something new"), FlagTone::Neutral);
        assert_eq!(flag_tone("custom"), FlagTone::Neutral);
    }

    /// An app name that merely CONTAINS a flag word keeps it. Only a whole
    /// suffix behind the separator counts.
    #[test]
    fn an_app_named_after_a_flag_word_is_not_split() {
        assert_eq!(split_app_cell("Custom"), ("Custom", None));
        assert_eq!(split_app_cell("paused"), ("paused", None));
        assert_eq!(
            split_app_cell("Key In Use Manager"),
            ("Key In Use Manager", None)
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
}
