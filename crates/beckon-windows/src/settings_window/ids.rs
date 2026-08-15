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
// 1017 and 1018 were `IDC_LBL_SHORTCUT` and `IDC_LBL_APP`, the editor's two
// field labels. Deleted 2026-08-15 (design §3.1, "no field labels") and
// RETIRED in `beckon_core::settings::RETIRED_IDS`, not freed: the App combo's
// cue banner reads `App` while it is empty, and the key list sits at the end
// of the modifier run where a key goes, so neither word was telling the user
// anything the control did not.
//
// Labels that DO survive still need real ids, not -1, and that is what this
// comment was here for: `layout` positions controls through `GetDlgItem`, and
// every -1 resolves to the same first match, so sharing one id left all but
// the first stacked at the origin. `IDC_LBL_HOLD` and `IDC_LBL_TAP` are the
// remaining two.
pub(super) const IDC_GRP_KEYBOARD: i32 = 1019;
// 1020 was `IDC_LBL_SECTION`, the `Shortcuts` heading at the top of card 1.
// Deleted 2026-08-15 and RETIRED in `beckon_core::settings::RETIRED_IDS`, not
// freed: design §3.1's drawing and the mock-up both open that card with the
// filter and the two buttons and nothing else, while the window drew an
// 18 px Subtitle reading `Shortcuts` directly beneath a tab pill captioned
// `Shortcuts`. The pill says which door is open from all four doors; the
// heading repeated it on one.
//
// It cost the row no height and returns none: the head row is `ctl` tall
// because of the buttons in it.
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
/// capture and reads `Stop` while it is armed; `Revert` clears the row's
/// combo and leaves it without a shortcut.
pub(super) const IDC_RECORD: i32 = 1032;
pub(super) const IDC_REVERT: i32 = 1033;

// 1034 was `IDC_GRP_EDITOR`, the editor card's `Editing "…"` caption, and
// 1035 was `IDC_LBL_COUNT`, the `· 18 bindings` STATIC beside the `Shortcuts`
// heading. Both deleted 2026-08-15 and RETIRED in
// `beckon_core::settings::RETIRED_IDS`, not freed.
//
// 1035: the count moved to the Shortcuts pill's badge, where all four doors
// can read it, and the photograph of the shipped window shows both at once --
// which is the state that move was meant to end.
//
// 1034: design §3.1 deletes the caption. The list above the editor already
// highlights the row being edited, so the caption was a second answer to a
// question the selection had answered -- and it was the only control on that
// card whose text came from the catalog, which is why `apply_state` carried an
// `&`-doubling rule that went with it.
//
// Nothing may reclaim either; `no_defined_id_is_retired` below is what
// enforces that here, and `retired_ids_stay_retired` in core enforces it
// there.

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

/// The System page (design §3.3), in the order the rows are drawn.
///
/// **Not chosen here either.** Phase 0 fixed all fourteen, and the page is
/// built from exactly that list -- no number was picked, skipped or reused
/// while the rows were written.
///
/// Three of them are switches drawn by `paint::toggle` and one is a
/// `msctls_trackbar32`; the rest are `STATIC`s and push buttons. What binds
/// them together is `SYS_ROWS` (`mod.rs`), which says which of the two
/// CONDITIONAL rows are on screen -- `IDC_AUTOSTART`, and the log row's four.
///
/// **`IDC_SYS_RELOAD` is not `IDC_RELOAD` (1015)**, and the two must never be
/// merged: 1015 is the external-change banner's *reload from disk*, which
/// discards the window's unsaved edits, while this one is the tray's own
/// *reload now*, which re-reads the file and re-registers the hotkeys. Same
/// word, opposite blast radius.
pub(super) const IDC_PAUSE: i32 = 1070;
pub(super) const IDC_AUTOSTART: i32 = 1071;
pub(super) const IDC_SYS_RELOAD: i32 = 1072;
pub(super) const IDC_DARK: i32 = 1073;
pub(super) const IDC_OPACITY: i32 = 1074;
pub(super) const IDC_OPACITY_VALUE: i32 = 1075;
pub(super) const IDC_CONFIG_NAME: i32 = 1076;
pub(super) const IDC_CONFIG_DIR: i32 = 1077;
pub(super) const IDC_CONFIG_OPEN: i32 = 1078;
pub(super) const IDC_CONFIG_SHOW: i32 = 1079;
pub(super) const IDC_LOG_NAME: i32 = 1080;
pub(super) const IDC_LOG_SIZE: i32 = 1081;
pub(super) const IDC_LOG_OPEN: i32 = 1082;
pub(super) const IDC_LOG_SHOW: i32 = 1083;

/// The one line About shows while it is waiting.
///
/// **From the reserved TAIL of the page's range, not the next free number.**
/// 1100-1114 are already named in `CONTROL_IDS` for controls Phase 0
/// specified and nothing has built, and a placeholder is the one control on a
/// page that is meant to be *deleted* -- taking a number out of the middle of
/// a block would leave a hole in that page's numbering the day it goes.
///
/// **That reasoning has now been paid off once.** System's own placeholder
/// was 1084, out of 1084-1099, and design §3.3's fourteen rows replaced it on
/// 2026-08-15 with 1070-1083 intact and no gap in them. 1084 is RETIRED in
/// `beckon_core::settings::RETIRED_IDS`, not freed.
///
/// **It does not sit on a card, so it is not in the `on_card` match** in
/// `mod.rs`'s `WM_CTLCOLORSTATIC` arm. The About page leaves all four card
/// rects at zero height (`compute_card_rects`), so the ground under it is the
/// window's own `bg`; it is answered by its own branch of that arm, above the
/// `on_card` one. Falling through to `DefWindowProcW` would draw it as a
/// `COLOR_3DFACE` rectangle -- the defect that once hit eight controls at once
/// -- and joining `on_card` would paint a card-coloured strip on a page with
/// no card behind it.
pub(super) const IDC_ABOUT_PLACEHOLDER: i32 = 1115;

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
        ("GRP_KEYBOARD", super::IDC_GRP_KEYBOARD),
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
        ("REVERT", super::IDC_REVERT),
        ("TAB_SHORTCUTS", super::IDC_TAB_SHORTCUTS),
        ("TAB_KEYBOARD", super::IDC_TAB_KEYBOARD),
        ("TAB_SYSTEM", super::IDC_TAB_SYSTEM),
        ("TAB_ABOUT", super::IDC_TAB_ABOUT),
        ("PAUSE", super::IDC_PAUSE),
        ("AUTOSTART", super::IDC_AUTOSTART),
        ("SYS_RELOAD", super::IDC_SYS_RELOAD),
        ("DARK", super::IDC_DARK),
        ("OPACITY", super::IDC_OPACITY),
        ("OPACITY_VALUE", super::IDC_OPACITY_VALUE),
        ("CONFIG_NAME", super::IDC_CONFIG_NAME),
        ("CONFIG_DIR", super::IDC_CONFIG_DIR),
        ("CONFIG_OPEN", super::IDC_CONFIG_OPEN),
        ("CONFIG_SHOW", super::IDC_CONFIG_SHOW),
        ("LOG_NAME", super::IDC_LOG_NAME),
        ("LOG_SIZE", super::IDC_LOG_SIZE),
        ("LOG_OPEN", super::IDC_LOG_OPEN),
        ("LOG_SHOW", super::IDC_LOG_SHOW),
        ("ABOUT_PLACEHOLDER", super::IDC_ABOUT_PLACEHOLDER),
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
