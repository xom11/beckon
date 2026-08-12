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
