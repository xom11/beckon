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
    fn the_two_reserved_ids_are_distinct_and_at_the_top() {
        assert_ne!(MENU_ID_ALT_CLICK, MENU_ID_DOUBLE_CLICK);
        assert!(MENU_ID_ALT_CLICK > 1_000_000);
    }
}
