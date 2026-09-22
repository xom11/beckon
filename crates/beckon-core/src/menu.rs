//! The tray / menu bar context menu, as data.
//!
//! This type used to live in `beckon-windows::hotkey`, where it already
//! carried the comment that made it OS-neutral in intent: the drawing layer
//! reports a click and what any row *means* is the caller's business, so
//! there is no enum of actions here. Moving it to core makes that true in
//! fact -- `serve::build_entries` composes the menu once for every platform,
//! and the composition is compiled and tested by all three CI jobs rather
//! than only the Windows one.
//!
//! Nothing here draws anything. `beckon_windows::hotkey` renders it with
//! `AppendMenuW`; `beckon_macos::tray` renders it with `NSMenu`.

/// One row of the context menu.
///
/// **The first four fields are the whole of what Windows draws**, and
/// `serve::build_entries` fills only those, so the Windows menu is unchanged
/// by everything below them. The rest are macOS rows (spec
/// `2026-09-22-macos-ui-redesign-design.md` §3.4). Each defaults to "absent",
/// which is what lets a row that does not use a field say nothing.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MenuEntry {
    pub id: u32,
    pub label: String,
    /// `None` for a plain item, `Some(bool)` for a check box.
    pub checked: Option<bool>,
    pub enabled: bool,
    /// What kind of row. `Item` for every row Windows draws.
    pub kind: EntryKind,
    /// Right-aligned secondary text: the binding's chord, in glyphs.
    pub detail: Option<String>,
    /// One word from `settings::FLAGS`, drawn beside the label.
    pub flag: Option<&'static str>,
    /// A bundle id whose application icon leads the row.
    pub icon: Option<String>,
    /// A key equivalent AppKit draws and honours while the menu is open,
    /// always with Command (`,` `r` `q`).
    pub key: Option<char>,
    /// Words for the row, for hover and for VoiceOver -- the glyphs in
    /// `detail` are not something a screen reader should be handed.
    pub tooltip: Option<String>,
    /// The rows of a submenu. Non-empty only for `EntryKind::Submenu`.
    pub children: Vec<MenuEntry>,
}

/// What a row is. Everything but `Item` is macOS-only in practice.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum EntryKind {
    #[default]
    Item,
    /// A small grey title over the rows that follow (`Needs attention`).
    SectionHeader,
    /// The menu's first row: a status dot, a title, a subtitle and a switch.
    /// The switch reports this entry's `id` when flipped.
    Header(Header),
    /// A row that opens `children`.
    Submenu,
}

/// The header row's content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Header {
    pub title: String,
    pub subtitle: String,
    pub dot: Dot,
    /// Whether the switch is on -- i.e. shortcuts are NOT paused.
    pub on: bool,
}

/// The header's status dot. Grey while paused, orange when something needs
/// the user, green otherwise (spec §3.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dot {
    Ok,
    Warn,
    Off,
}

impl MenuEntry {
    /// A horizontal rule: a plain row with an empty label.
    pub fn separator() -> Self {
        Self::default()
    }

    /// A plain, enabled row.
    pub fn item(id: u32, label: impl Into<String>) -> Self {
        Self {
            id,
            label: label.into(),
            enabled: true,
            ..Self::default()
        }
    }

    /// A section title. It carries no id and nothing to click.
    pub fn section_header(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            kind: EntryKind::SectionHeader,
            ..Self::default()
        }
    }

    /// Recognised by an empty label on a PLAIN row. A header or a section
    /// title with an empty label is still that thing, not a rule.
    pub fn is_separator(&self) -> bool {
        matches!(self.kind, EntryKind::Item) && self.label.is_empty()
    }
}

/// Delivered to `on_click` when the icon is double-clicked. Callers must
/// number their real entries below both reserved ids.
pub const MENU_ID_DOUBLE_CLICK: u32 = u32::MAX;

/// Delivered to `on_click` when the icon is ⌥-clicked (macOS). It never opens
/// the menu; `serve` toggles pause on it.
pub const MENU_ID_ALT_CLICK: u32 = u32::MAX - 1;

/// The `Check for updates` row's label, which differs by platform in case
/// only.
///
/// macOS title-cases menu items and Windows does not. **ASCII dots, not an
/// ellipsis**, like every other display string this program draws.
///
/// The platform arrives as a parameter rather than as a `cfg!` inside, for
/// the reason `menu_log_row` takes one: both readings are then compiled and
/// tested by all three CI jobs, not only by the one that ships them.
pub fn update_label(macos: bool) -> &'static str {
    if macos {
        "Check for Updates..."
    } else {
        "Check for updates..."
    }
}

/// What the macOS header says, and the colour of its dot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Headline {
    pub text: String,
    pub dot: Dot,
}

/// Everything `menu_headline` weighs, already counted by `serve`.
#[derive(Debug, Clone, Copy)]
pub struct HeadlineInputs<'a> {
    pub paused: bool,
    pub accessibility: bool,
    /// `serve`'s `last_phrase`, said verbatim when a registration failed so
    /// the header and the tooltip use one sentence.
    pub phrase: &'a str,
    /// Bindings whose registration returned an error.
    pub failed: usize,
    /// Bindings whose every candidate is a `NoMatch`.
    pub missing: usize,
    pub total: usize,
}

/// The header's subtitle and dot. The precedence is spec §3.2, and each rung
/// hides the ones below it because the one above explains more.
pub fn menu_headline(i: HeadlineInputs) -> Headline {
    if i.paused {
        return Headline {
            text: "Paused - shortcuts are off".into(),
            dot: Dot::Off,
        };
    }
    if !i.accessibility {
        return Headline {
            text: "Needs Accessibility to switch windows".into(),
            dot: Dot::Warn,
        };
    }
    if i.failed > 0 {
        return Headline {
            text: i.phrase.to_string(),
            dot: Dot::Warn,
        };
    }
    let noun = if i.total == 1 {
        "shortcut"
    } else {
        "shortcuts"
    };
    let text = if i.missing > 0 {
        format!("{} {noun}, {} missing", i.total, i.missing)
    } else {
        format!("{} {noun}", i.total)
    };
    Headline { text, dot: Dot::Ok }
}

/// The one word a binding row carries in the menu, from `settings::FLAGS`,
/// in `row_condition`'s precedence.
///
/// `paused` is absent on purpose -- the header says it once, instead of every
/// row saying it. `other chord` is absent because it is a view fact about the
/// list, not a fault (`docs/notes/settings-window.md`).
pub fn row_flag(in_use: bool, missing: bool) -> Option<&'static str> {
    if in_use {
        Some(crate::settings::FLAGS[1])
    } else if missing {
        Some(crate::settings::FLAGS[2])
    } else {
        None
    }
}

/// How many broken rows the *Needs attention* section lists before folding
/// the rest into one `and N more...` row.
pub const ATTENTION_ROWS: usize = 3;

/// Which rows *Needs attention* shows, in file order, and which it folds.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Attention {
    pub shown: Vec<usize>,
    pub rest: Vec<usize>,
}

/// Split the flagged rows of `flags` (one per binding, in file order) into
/// the first `max` and the rest. Both empty means the section is not drawn.
pub fn attention(flags: &[Option<&str>], max: usize) -> Attention {
    let flagged: Vec<usize> = flags
        .iter()
        .enumerate()
        .filter(|(_, f)| f.is_some())
        .map(|(i, _)| i)
        .collect();
    let cut = flagged.len().min(max);
    Attention {
        shown: flagged[..cut].to_vec(),
        rest: flagged[cut..].to_vec(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Two spellings, one table. Platform strings are tables here, not
    /// literals -- and this is the shape `menu_log_row` already uses: the
    /// platform arrives as a parameter so both readings are testable on every
    /// CI job, not just on the machine that ships them.
    #[test]
    fn the_update_row_is_title_case_on_macos_only() {
        assert_eq!(update_label(true), "Check for Updates...");
        assert_eq!(update_label(false), "Check for updates...");
    }

    /// ASCII dots, not an ellipsis -- like every other display string here.
    #[test]
    fn both_update_labels_are_ascii() {
        assert!(update_label(true).is_ascii());
        assert!(update_label(false).is_ascii());
    }

    /// The whole Windows-safety claim of this change: the rows Windows draws
    /// are built exactly as before.
    #[test]
    fn a_separator_and_an_item_are_what_they_always_were() {
        let s = MenuEntry::separator();
        assert_eq!(
            (s.id, s.label.as_str(), s.checked, s.enabled),
            (0, "", None, false)
        );
        assert!(s.is_separator());
        let i = MenuEntry::item(7, "Quit");
        assert_eq!(
            (i.id, i.label.as_str(), i.checked, i.enabled),
            (7, "Quit", None, true)
        );
        assert_eq!(i.kind, EntryKind::Item);
        assert!(i.children.is_empty() && i.detail.is_none() && i.key.is_none());
    }

    /// Only a plain row with an empty label is a rule. A header or a section
    /// title must never be mistaken for one, whatever its label.
    #[test]
    fn only_a_plain_empty_row_is_a_separator() {
        let h = MenuEntry {
            kind: EntryKind::Header(Header {
                title: String::new(),
                subtitle: String::new(),
                dot: Dot::Ok,
                on: true,
            }),
            ..MenuEntry::default()
        };
        assert!(!h.is_separator());
        assert!(!MenuEntry::section_header("").is_separator());
    }

    #[test]
    #[allow(clippy::assertions_on_constants)]
    fn the_two_reserved_ids_are_distinct_and_at_the_top() {
        assert_ne!(MENU_ID_ALT_CLICK, MENU_ID_DOUBLE_CLICK);
        assert!(MENU_ID_ALT_CLICK > 1_000_000);
    }

    fn inputs() -> HeadlineInputs<'static> {
        HeadlineInputs {
            paused: false,
            accessibility: true,
            phrase: "19 shortcuts registered",
            failed: 0,
            missing: 0,
            total: 19,
        }
    }

    /// One test per rung, each proving it outranks the next (spec §3.2).
    #[test]
    fn paused_outranks_everything() {
        let h = menu_headline(HeadlineInputs {
            paused: true,
            accessibility: false,
            failed: 2,
            missing: 2,
            ..inputs()
        });
        assert_eq!(
            h,
            Headline {
                text: "Paused - shortcuts are off".into(),
                dot: Dot::Off
            }
        );
    }

    #[test]
    fn missing_accessibility_outranks_a_failed_registration() {
        let h = menu_headline(HeadlineInputs {
            accessibility: false,
            failed: 2,
            ..inputs()
        });
        assert_eq!(
            h,
            Headline {
                text: "Needs Accessibility to switch windows".into(),
                dot: Dot::Warn
            }
        );
    }

    /// A failed registration is said in `serve`'s own words, verbatim.
    #[test]
    fn a_failed_registration_uses_the_registration_phrase() {
        let h = menu_headline(HeadlineInputs {
            phrase: "17 of 19 shortcuts registered (2 failed)",
            failed: 2,
            missing: 1,
            ..inputs()
        });
        assert_eq!(h.text, "17 of 19 shortcuts registered (2 failed)");
        assert_eq!(h.dot, Dot::Warn);
    }

    /// Missing apps are counted but do not turn the dot orange: the keys are
    /// registered and serving.
    #[test]
    fn missing_apps_are_counted_on_a_green_dot() {
        let h = menu_headline(HeadlineInputs {
            missing: 2,
            ..inputs()
        });
        assert_eq!(
            h,
            Headline {
                text: "19 shortcuts, 2 missing".into(),
                dot: Dot::Ok
            }
        );
    }

    #[test]
    fn a_clean_file_just_counts_and_agrees_in_number() {
        assert_eq!(menu_headline(inputs()).text, "19 shortcuts");
        assert_eq!(
            menu_headline(HeadlineInputs {
                total: 1,
                ..inputs()
            })
            .text,
            "1 shortcut"
        );
    }

    #[test]
    fn every_headline_is_ascii() {
        for i in [
            inputs(),
            HeadlineInputs {
                paused: true,
                ..inputs()
            },
            HeadlineInputs {
                accessibility: false,
                ..inputs()
            },
            HeadlineInputs {
                missing: 3,
                ..inputs()
            },
        ] {
            assert!(menu_headline(i).text.is_ascii());
        }
    }

    /// The indices into FLAGS are what `row_flag` depends on; pin the words.
    #[test]
    fn row_flag_speaks_the_settings_vocabulary_in_its_precedence() {
        assert_eq!(crate::settings::FLAGS[1], "in use");
        assert_eq!(crate::settings::FLAGS[2], "missing");
        assert_eq!(row_flag(true, true), Some("in use"));
        assert_eq!(row_flag(false, true), Some("missing"));
        assert_eq!(row_flag(false, false), None);
    }

    #[test]
    fn attention_keeps_file_order_and_cuts_at_max() {
        let flags = [
            None,
            Some("missing"),
            None,
            Some("in use"),
            Some("missing"),
            Some("missing"),
            Some("missing"),
        ];
        let a = attention(&flags, ATTENTION_ROWS);
        assert_eq!(a.shown, vec![1, 3, 4]);
        assert_eq!(a.rest, vec![5, 6]);
        assert_eq!(attention(&[None, None], 3), Attention::default());
    }
}
