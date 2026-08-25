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
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuEntry {
    pub id: u32,
    pub label: String,
    /// `None` for a plain item, `Some(bool)` for a check box.
    pub checked: Option<bool>,
    pub enabled: bool,
}

impl MenuEntry {
    /// A horizontal rule. Recognised by its empty label.
    pub fn separator() -> Self {
        Self {
            id: 0,
            label: String::new(),
            checked: None,
            enabled: false,
        }
    }

    /// A plain, enabled row.
    pub fn item(id: u32, label: impl Into<String>) -> Self {
        Self {
            id,
            label: label.into(),
            checked: None,
            enabled: true,
        }
    }

    pub fn is_separator(&self) -> bool {
        self.label.is_empty()
    }
}

/// Delivered to `on_click` when the icon is double-clicked. Callers must
/// number their real entries below this.
pub const MENU_ID_DOUBLE_CLICK: u32 = u32::MAX;

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
}
