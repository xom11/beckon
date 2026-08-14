//! Dialog control ids, and the two command ids that belong to no control.
//!
//! Split out of `mod.rs` so the cross-check against
//! `beckon_core::settings::CONTROL_IDS` sits beside the numbers it checks,
//! rather than 6 000 lines away from them.
//!
//! **Core does not define these.** It carries a table with a test attached;
//! this file is the definition, and `ids_match_the_core_table` is what keeps
//! the two from drifting. Core is the crate that must stay free of Win32
//! concepts, and a dialog control id is one.
//!
//! **`IDM_` is not `IDC_`, and the prefix is load-bearing.** Every test below
//! iterates `MINE`, and `every_declared_id_has_a_row_in_mine` finds its
//! subjects by reading this file's own source for `const IDC_` -- so a name
//! that starts `IDM_` is deliberately outside that net, because it names no
//! control and so has no window to place, no page to belong to and no row in
//! core's `CONTROL_IDS`. What it still shares with a control id is the
//! `WM_COMMAND` id space: `handle_command` matches on the number alone, so a
//! collision would route an accelerator into a button.
//! `the_command_ids_are_not_control_ids` is that guard, and it is why the two
//! live here rather than beside the accelerator table that is their only
//! sender.

pub(super) const IDC_LIST: i32 = 1001;
pub(super) const IDC_COMBO: i32 = 1002;
pub(super) const IDC_APP: i32 = 1003;
pub(super) const IDC_NOTES: i32 = 1004;
pub(super) const IDC_ADD: i32 = 1005;
pub(super) const IDC_REMOVE: i32 = 1006;
pub(super) const IDC_APPLY: i32 = 1007;
pub(super) const IDC_CAPS: i32 = 1008;
// 1009-1011 were the three `Tapping Caps alone` radios. They are free
// again -- unlike 1001-1008, 1012 and 1013, which `examples/settings_probe.rs`
// hard-codes -- but nothing should reclaim them: a probe built against an
// older binary would find a control it thinks it recognises.
pub(super) const IDC_OPENFILE: i32 = 1012;
pub(super) const IDC_CLOSE: i32 = 1013;
pub(super) const IDC_BANNER: i32 = 1014;
pub(super) const IDC_RELOAD: i32 = 1015;
pub(super) const IDC_KEEPMINE: i32 = 1016;
// Labels and the group box need real ids, not -1: `layout` positions
// controls through `GetDlgItem`, and every -1 resolves to the same first
// match, so sharing one id left all but the first stacked at the origin.
pub(super) const IDC_LBL_SHORTCUT: i32 = 1017;
pub(super) const IDC_LBL_APP: i32 = 1018;
pub(super) const IDC_GRP_KEYBOARD: i32 = 1019;
/// The `Shortcuts` heading in band 2. New ids go ABOVE the existing range:
/// 1001-1007 and the class name are hard-coded in
/// `examples/settings_probe.rs` and are fixed points.
pub(super) const IDC_LBL_SECTION: i32 = 1020;
pub(super) const IDC_FILTER: i32 = 1021;
/// The keyboard row: three `Hold` chips, the `Tap` combo, and the two
/// static words that name each half. `Hold` and `Tap` are the only two
/// things Caps can do, so the row names both rather than making the second
/// an afterthought of the first -- which is what the radios did, by gluing
/// the question onto the first answer.
pub(super) const IDC_HOLD_CTRL: i32 = 1022;
pub(super) const IDC_HOLD_WIN: i32 = 1023;
pub(super) const IDC_HOLD_ALT: i32 = 1024;
pub(super) const IDC_TAP: i32 = 1025;
pub(super) const IDC_LBL_HOLD: i32 = 1026;
pub(super) const IDC_LBL_TAP: i32 = 1027;
/// The editor strip's four modifier chips. `IDC_COMBO` (1002) keeps its
/// number beside them and changes CLASS instead: it is the id
/// `examples/settings_probe.rs` hard-codes for "the shortcut control", so
/// reusing it is what keeps that probe pointed at the right thing, and
/// retiring it would leave the probe reading a control that no longer
/// exists.
pub(super) const IDC_MOD_CTRL: i32 = 1028;
pub(super) const IDC_MOD_WIN: i32 = 1029;
pub(super) const IDC_MOD_ALT: i32 = 1030;
pub(super) const IDC_MOD_SHIFT: i32 = 1031;
/// The editor strip's two commands. `Record` arms the `WH_KEYBOARD_LL`
/// capture and reads `Stop` while it is armed; `Reset` clears the row's
/// combo and leaves it without a shortcut.
pub(super) const IDC_RECORD: i32 = 1032;
pub(super) const IDC_RESET: i32 = 1033;

/// The editor card's caption. Names which row is being edited, so the two
/// lines below it read as one thing rather than as seven controls that
/// happen to share a card. A `BS_GROUPBOX` until a review fix on Task 8
/// reclassed it to a plain caption `STATIC` -- a themed group-box frame,
/// nested inside the new rounded `card()` background, drew as two frames
/// around one set of controls. See the creation comment in
/// `build_children` and the `role_of` doc.
///
/// 1034 because 1033 is the current maximum and 1001-1007 are pinned by
/// `examples/settings_probe.rs`; the reclass did not renumber it. Not
/// operable, so it carries no mnemonic and no entry in `mod cap`'s
/// collision table.
pub(super) const IDC_GRP_EDITOR: i32 = 1034;

/// The count beside the `Shortcuts` heading -- `· 18 bindings`.
///
/// **A second STATIC rather than a longer caption**, because the two are
/// different type: B draws the heading at Subtitle and the count small and
/// grey, and one STATIC has one font. It is also the only on-card control
/// with a dimmer ink of its own -- `WM_CTLCOLORSTATIC` answers every STATIC,
/// group box and check box left in this window now (see that arm), but this
/// id alone keeps `text_faint` rather than the ordinary `text` token the
/// rest draw with.
///
/// It counts what the LIST is showing, so under a filter it says how many
/// rows are on screen rather than how many the file holds. That is the
/// honest reading of a number sitting on top of the list it describes.
pub(super) const IDC_LBL_COUNT: i32 = 1035;

/// The four tab pills, in strip order.
///
/// **Not chosen here.** Phase 0 fixed them, and the reason they are 1040
/// rather than 1036 is `CONTROL_IDS`' range table: 1001-1039 belongs to the
/// pre-Four-Doors window and 1040-1049 to the shell. Those ranges exist
/// because two drafts of the Four Doors design each claimed 1060-1069 for a
/// different page.
///
/// **Contiguous and ascending, and that is load-bearing rather than tidy.**
/// `build_children` and `show_page` tick a pill with
/// `CheckRadioButton(hwnd, IDC_TAB_SHORTCUTS, IDC_TAB_ABOUT, id)`, which takes
/// a FIRST and a LAST id and clears everything between them -- so a gap here
/// would hand that call a range covering an id the strip does not own, and a
/// re-order would make the range name the wrong pair.
/// `the_tab_ids_are_contiguous_and_agree_with_tab_id_of` in `mod.rs` is the
/// guard, since nothing about the ids themselves says so.
pub(super) const IDC_TAB_SHORTCUTS: i32 = 1040;
pub(super) const IDC_TAB_KEYBOARD: i32 = 1041;
pub(super) const IDC_TAB_SYSTEM: i32 = 1042;
pub(super) const IDC_TAB_ABOUT: i32 = 1043;

/// `Ctrl+Tab` and `Ctrl+Shift+Tab`: "the next door" and "the one before it".
///
/// **Commands, not controls.** `Ctrl+1`..`Ctrl+4` name a door outright and so
/// ride on the four pill ids above; these two name a *direction*, whose answer
/// depends on the door that is open, so they need ids of their own. An
/// accelerator table entry carries a `WM_COMMAND` id and nothing else, which
/// is why the direction cannot be resolved before `handle_command` sees it.
///
/// **2001 rather than 1044**, deliberately far from the control range. 1044-6
/// are already spoken for in core's `CONTROL_IDS` (`SERVICE_LINE`, `SAVED`,
/// `UNDO`) and the whole 1001-1115 span belongs to controls this window either
/// has or is going to grow; a command id sitting inside it would be a number
/// that has to be skipped by everyone allocating from a range, for ever. The
/// two are contiguous and ascending for no reason beyond reading order --
/// nothing does arithmetic on them, unlike the pills, whose contiguity
/// `CheckRadioButton` depends on.
pub(super) const IDM_PAGE_NEXT: i32 = 2001;
pub(super) const IDM_PAGE_PREV: i32 = 2002;

#[cfg(test)]
mod tests {
    use beckon_core::settings::{CONTROL_IDS, RETIRED_IDS};

    /// Every id this module defines, paired with the name the core table
    /// knows it by. Hand-maintained, and `every_declared_id_has_a_row_in_mine`
    /// is what keeps the hand honest: it reads this file's own source and
    /// fails when a constant above has no row below.
    ///
    /// CORRECTED 2026-08-14: this comment used to say the omission was caught
    /// by `every_core_id_is_defined_here`. That test was specified by the plan
    /// and cancelled two paragraphs later in the same plan, so it was never
    /// written -- grep found the name in this comment and nowhere else. The
    /// claim was the whole safety argument for the design: every test in this
    /// module iterates `MINE` and none iterates `CONTROL_IDS`, so `MINE` is
    /// the hinge, and until the test below existed nothing guarded it.
    const MINE: &[(&str, i32)] = &[
        ("LIST", super::IDC_LIST),
        ("COMBO", super::IDC_COMBO),
        ("APP", super::IDC_APP),
        ("NOTES", super::IDC_NOTES),
        ("ADD", super::IDC_ADD),
        ("REMOVE", super::IDC_REMOVE),
        ("APPLY", super::IDC_APPLY),
        ("CAPS", super::IDC_CAPS),
        ("OPENFILE", super::IDC_OPENFILE),
        ("CLOSE", super::IDC_CLOSE),
        ("BANNER", super::IDC_BANNER),
        ("RELOAD", super::IDC_RELOAD),
        ("KEEPMINE", super::IDC_KEEPMINE),
        ("LBL_SHORTCUT", super::IDC_LBL_SHORTCUT),
        ("LBL_APP", super::IDC_LBL_APP),
        ("GRP_KEYBOARD", super::IDC_GRP_KEYBOARD),
        ("LBL_SECTION", super::IDC_LBL_SECTION),
        ("FILTER", super::IDC_FILTER),
        ("HOLD_CTRL", super::IDC_HOLD_CTRL),
        ("HOLD_WIN", super::IDC_HOLD_WIN),
        ("HOLD_ALT", super::IDC_HOLD_ALT),
        ("TAP", super::IDC_TAP),
        ("LBL_HOLD", super::IDC_LBL_HOLD),
        ("LBL_TAP", super::IDC_LBL_TAP),
        ("MOD_CTRL", super::IDC_MOD_CTRL),
        ("MOD_WIN", super::IDC_MOD_WIN),
        ("MOD_ALT", super::IDC_MOD_ALT),
        ("MOD_SHIFT", super::IDC_MOD_SHIFT),
        ("RECORD", super::IDC_RECORD),
        ("RESET", super::IDC_RESET),
        ("GRP_EDITOR", super::IDC_GRP_EDITOR),
        ("LBL_COUNT", super::IDC_LBL_COUNT),
        ("TAB_SHORTCUTS", super::IDC_TAB_SHORTCUTS),
        ("TAB_KEYBOARD", super::IDC_TAB_KEYBOARD),
        ("TAB_SYSTEM", super::IDC_TAB_SYSTEM),
        ("TAB_ABOUT", super::IDC_TAB_ABOUT),
    ];

    /// The net under `MINE`. It reads this file's own source -- the same
    /// `include_str!` trick `geometry_matches_the_probe` uses on the probe,
    /// pointed one file closer -- and reads every line that starts with `pub`
    /// and carries `const IDC_` as a declaration. Add a constant above and
    /// forget the row below, and this fails on the Windows CI job, the only
    /// one that compiles this crate at all.
    ///
    /// It cannot count its own text, by construction twice over: every
    /// mention inside `mod tests` is indented, so no line of it starts with
    /// `pub`, and a comment cannot start with `pub` at any indentation
    /// either. A declaration written with no visibility modifier would be
    /// missed -- and cannot exist, because `mod.rs` is what names these and a
    /// private one would be dead code under `-D warnings`.
    ///
    /// What it does not cover: `CONTROL_IDS` on core's side is unguarded in
    /// the other direction, deliberately -- that table already carries ids for
    /// pages nothing has built yet, so a name there with no constant here is
    /// the normal state rather than a defect.
    #[test]
    fn every_declared_id_has_a_row_in_mine() {
        let declared: Vec<&str> = include_str!("ids.rs")
            .lines()
            .filter(|l| l.starts_with("pub"))
            .filter_map(|l| l.split_once("const IDC_"))
            .map(|(_, rest)| rest.split(':').next().unwrap_or(rest))
            .collect();
        for name in &declared {
            assert!(
                MINE.iter().any(|(n, _)| n == name),
                "`IDC_{name}` is declared in ids.rs and has no row in `MINE`. \
                 Every test in this module iterates `MINE`, so an id missing \
                 from it is checked by nothing: it can repeat a number already \
                 in use, and `layout` resolves a duplicate through \
                 `GetDlgItem` to the first match -- the second control is \
                 created, never placed, and left at the origin."
            );
        }
        assert_eq!(
            declared.len(),
            MINE.len(),
            "ids.rs declares {} controls and `MINE` has {} rows. Every \
             declaration found a row above, so the surplus is `MINE`'s own: a \
             row written twice, or one naming a constant that is gone.",
            declared.len(),
            MINE.len()
        );
    }

    #[test]
    fn ids_match_the_core_table() {
        for (name, id) in MINE {
            let core = CONTROL_IDS.iter().find(|(n, _)| n == name);
            assert_eq!(
                core.map(|(_, v)| *v),
                Some(*id),
                "`IDC_{name}` is {id} here and {:?} in \
                 `beckon_core::settings::CONTROL_IDS`",
                core.map(|(_, v)| *v)
            );
        }
    }

    /// The two `IDM_` command ids answer for nothing else in the window.
    ///
    /// `handle_command` dispatches on the id alone, and an accelerator's
    /// `WM_COMMAND` is indistinguishable from a control's once it is in that
    /// match -- so `IDM_PAGE_NEXT` colliding with a control id would make
    /// `Ctrl+Tab` press a button, and nothing above would notice: the `IDM_`
    /// names are outside `MINE` by design, so every other test in this module
    /// looks straight past them.
    ///
    /// `RETIRED_IDS` is checked for the same reason `no_defined_id_is_retired`
    /// checks it: a retired number is one `examples/settings_probe.rs` may
    /// still be looking for.
    #[test]
    fn the_command_ids_are_not_control_ids() {
        assert_ne!(super::IDM_PAGE_NEXT, super::IDM_PAGE_PREV);
        for (name, id) in [
            ("PAGE_NEXT", super::IDM_PAGE_NEXT),
            ("PAGE_PREV", super::IDM_PAGE_PREV),
        ] {
            assert!(
                !MINE.iter().any(|(_, v)| v == &id),
                "`IDM_{name}` ({id}) is also a control id in this file"
            );
            assert!(
                !CONTROL_IDS.iter().any(|(_, v)| *v == id),
                "`IDM_{name}` ({id}) is also a control id in \
                 `beckon_core::settings::CONTROL_IDS`, including the pages \
                 nothing has built yet"
            );
            assert!(
                !RETIRED_IDS.contains(&id),
                "`IDM_{name}` uses retired id {id}"
            );
        }
    }

    #[test]
    fn no_defined_id_is_retired() {
        for (name, id) in MINE {
            assert!(
                !RETIRED_IDS.contains(id),
                "`IDC_{name}` uses retired id {id}"
            );
        }
    }

    /// Every control is behind exactly one door, in the banner, or chrome.
    ///
    /// `show_page_controls` shows what `PAGE_CONTROLS` assigns to the
    /// incoming page and hides everything else it names -- so a control
    /// MISSING from that table is not hidden by anything and is drawn on all
    /// four pages, which reads as a layout bug rather than as a table bug.
    /// Nothing else can catch it: `layout` skips the whole band such a
    /// control belongs to, so it would sit wherever it was last placed, and
    /// no compiler has an opinion about a 26-row table.
    ///
    /// The two exempt groups are listed here rather than in `mod.rs` because
    /// this is the only place that needs them enumerated. **Chrome** is drawn
    /// on every page: the four pills and the command bar's three buttons.
    /// **The banner's three** are conditional on `banner_shown`, which is a
    /// page AND `external_change`, so they cannot ride in a page table.
    #[test]
    fn every_control_belongs_to_exactly_one_group() {
        let chrome = [
            super::IDC_TAB_SHORTCUTS,
            super::IDC_TAB_KEYBOARD,
            super::IDC_TAB_SYSTEM,
            super::IDC_TAB_ABOUT,
            super::IDC_OPENFILE,
            super::IDC_CLOSE,
            super::IDC_APPLY,
        ];
        let banner = [super::IDC_BANNER, super::IDC_RELOAD, super::IDC_KEEPMINE];
        for (name, id) in MINE {
            let paged = super::super::PAGE_CONTROLS
                .iter()
                .filter(|(c, _)| c == id)
                .count();
            let other = usize::from(chrome.contains(id)) + usize::from(banner.contains(id));
            assert_eq!(
                paged + other,
                1,
                "`IDC_{name}` ({id}) is in {paged} `PAGE_CONTROLS` rows and \
                 {other} of the two exempt groups. Exactly one is right: a \
                 control in none is shown on every page, and a control in two \
                 is shown and hidden by two rules that will disagree."
            );
        }
        assert_eq!(
            super::super::PAGE_CONTROLS.len() + chrome.len() + banner.len(),
            MINE.len(),
            "the three groups cover {} controls and `MINE` has {} rows. Every \
             `MINE` row found exactly one group above, so the surplus names a \
             constant that is gone.",
            super::super::PAGE_CONTROLS.len() + chrome.len() + banner.len(),
            MINE.len()
        );
    }

    /// The probe transcribes the window's geometry by hand, on purpose --
    /// see its own comment. What that cannot catch is a resize here that
    /// nobody copies over there, because the disagreement only surfaces when
    /// a person runs the probe on a14. This reads the example's SOURCE and
    /// compares the literals.
    #[test]
    fn geometry_matches_the_probe() {
        let src = include_str!("../../examples/settings_probe.rs");
        for (name, value) in [
            ("WINDOW_WIDTH_96", super::super::WINDOW_WIDTH),
            ("WINDOW_HEIGHT_96", super::super::WINDOW_HEIGHT),
            ("MIN_WIDTH_96", super::super::MIN_WIDTH),
            ("MIN_HEIGHT_96", super::super::MIN_HEIGHT),
        ] {
            let want = format!("const {name}: i32 = {value};");
            assert!(
                src.contains(&want),
                "examples/settings_probe.rs does not contain `{want}`. The \
                 probe prints its own copy as the EXPECTED geometry and \
                 reports `<<< FAIL` against it, so a stale copy makes a \
                 healthy window look broken on hardware."
            );
        }
    }
}
