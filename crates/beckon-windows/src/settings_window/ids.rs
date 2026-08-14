//! Dialog control ids.
//!
//! Split out of `mod.rs` so the cross-check against
//! `beckon_core::settings::CONTROL_IDS` sits beside the numbers it checks,
//! rather than 6 000 lines away from them.
//!
//! **Core does not define these.** It carries a table with a test attached;
//! this file is the definition, and `ids_match_the_core_table` is what keeps
//! the two from drifting. Core is the crate that must stay free of Win32
//! concepts, and a dialog control id is one.

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

#[cfg(test)]
mod tests {
    use beckon_core::settings::{CONTROL_IDS, RETIRED_IDS};

    /// Every id this module defines, paired with the name the core table
    /// knows it by. Hand-maintained, and that is the point: adding a control
    /// without adding it here is caught by `every_core_id_is_defined_here`.
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
    ];

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

    #[test]
    fn no_defined_id_is_retired() {
        for (name, id) in MINE {
            assert!(
                !RETIRED_IDS.contains(id),
                "`IDC_{name}` uses retired id {id}"
            );
        }
    }
}
