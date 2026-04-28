//! Compositor-neutral focus algorithm shared by every Linux backend.
//!
//! Each backend converts its own native window enumeration (sway tree,
//! Hyprland `j/clients`, X11 `_NET_CLIENT_LIST_STACKING`) into
//! `Vec<WindowSnapshot>`, calls [`decide`], and then translates the
//! returned [`Decision`] back into native commands. This keeps the focus
//! / cycle / toggle / hide policy in one place — backends only own the
//! IPC plumbing.
//!
//! Algorithm steps mirror what the i3ipc / Hyprland modules used to
//! implement inline; see CLAUDE.md "Focus algorithm" for the full spec:
//!   3.  no window of `target` exists                    → `Launch`
//!   4.  exists, focus is on a different app             → `Focus(addr)`
//!   5a. exists & focused, more windows of same app      → `Cycle(addr)`
//!   5b. exists & focused, only one of `target`          → `ToggleBack(addr)`
//!                       prefer MRU previous_app, fall back to other-app by recency
//!   5c. exists & focused, nothing else exists           → `Hide(addr)`
//!
//! `recency` semantics: lower = more recent. Backends fill it from whatever
//! native order is closest to MRU:
//!   - Hyprland: `focusHistoryID` directly (0 = currently focused).
//!   - X11 (EWMH): inverted index into `_NET_CLIENT_LIST_STACKING` (top of
//!     stack = recency 0).
//!   - sway / i3: tree traversal index. The tree carries no real focus
//!     history, so this degenerates to "first match in tree order" — the
//!     same behaviour `i3ipc.rs` had before this module existed.

use std::cmp::Ordering;

/// Compositor-neutral view of one window. The `address` is opaque to the
/// algorithm — backends mint it from their native id (con_id, hex pointer,
/// X11 window id) and parse it back when applying a [`Decision`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowSnapshot {
    pub address: String,
    pub class: String,
    pub recency: i32,
}

impl WindowSnapshot {
    pub fn new(address: impl Into<String>, class: impl Into<String>, recency: i32) -> Self {
        Self {
            address: address.into(),
            class: class.into(),
            recency,
        }
    }
}

/// What the caller should do next. Carries an address for everything except
/// `Launch`, where the caller falls through to its own `.desktop`-driven
/// launch path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    Launch,
    Focus(String),
    Cycle(String),
    ToggleBack(String),
    Hide(String),
}

/// Pure decision function. See module docs for the algorithm. `active` is
/// the currently focused address (if any), `target` the resolved app
/// id/class, `previous_app` the class persisted in the MRU state file.
pub fn decide(
    windows: &[WindowSnapshot],
    active: Option<&str>,
    target: &str,
    previous_app: Option<&str>,
) -> Decision {
    let app_windows: Vec<&WindowSnapshot> = windows.iter().filter(|w| w.class == target).collect();

    if app_windows.is_empty() {
        return Decision::Launch;
    }

    let focused_in_app = active
        .and_then(|addr| windows.iter().find(|w| w.address == addr))
        .map(|w| w.class == target)
        .unwrap_or(false);

    if !focused_in_app {
        // Step 4: pick the most-recent same-app window. Stable tie-break by
        // address string so callers see deterministic output across runs
        // when multiple windows share the same recency (sway tree case).
        let win = app_windows
            .iter()
            .min_by(|a, b| cmp_recency_then_address(a, b))
            .copied()
            .expect("non-empty by check above");
        return Decision::Focus(win.address.clone());
    }

    let focused_addr = active.expect("focused_in_app implies active.is_some()");

    // Step 5a: another window of the same app — cycle to it.
    if let Some(next) = app_windows
        .iter()
        .filter(|w| w.address != focused_addr)
        .min_by(|a, b| cmp_recency_then_address(a, b))
    {
        return Decision::Cycle(next.address.clone());
    }

    // Step 5b: only one window of target. Honour the MRU "previous" first
    // (and only when it isn't `target`), otherwise pick the most-recent
    // window of any other app.
    let mru_choice = previous_app.filter(|app| *app != target).and_then(|app| {
        windows
            .iter()
            .filter(|w| w.class == app)
            .min_by(cmp_recency_then_address)
    });
    let other = mru_choice.or_else(|| {
        windows
            .iter()
            .filter(|w| w.class != target)
            .min_by(cmp_recency_then_address)
    });
    if let Some(win) = other {
        return Decision::ToggleBack(win.address.clone());
    }

    // Step 5c: lone window of the target app, nothing else to toggle to.
    Decision::Hide(focused_addr.to_string())
}

fn cmp_recency_then_address(a: &&WindowSnapshot, b: &&WindowSnapshot) -> Ordering {
    a.recency
        .cmp(&b.recency)
        .then_with(|| a.address.cmp(&b.address))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w(addr: &str, class: &str, recency: i32) -> WindowSnapshot {
        WindowSnapshot::new(addr, class, recency)
    }

    // ---- step 3: launch ----

    #[test]
    fn launch_when_no_windows_at_all() {
        assert_eq!(decide(&[], None, "claude", None), Decision::Launch);
    }

    #[test]
    fn launch_when_no_target_class_present() {
        let ws = vec![w("0x1", "kitty", 0)];
        assert_eq!(decide(&ws, Some("0x1"), "claude", None), Decision::Launch);
    }

    // ---- step 4: focus ----

    #[test]
    fn focus_picks_most_recent_same_app() {
        let ws = vec![
            w("0xA", "kitty", 0),
            w("0xB", "claude", 1),
            w("0xC", "claude", 2),
        ];
        assert_eq!(
            decide(&ws, Some("0xA"), "claude", None),
            Decision::Focus("0xB".to_string())
        );
    }

    #[test]
    fn focus_works_when_active_is_unset() {
        let ws = vec![w("0xA", "claude", 1), w("0xB", "claude", 0)];
        assert_eq!(
            decide(&ws, None, "claude", None),
            Decision::Focus("0xB".to_string())
        );
    }

    // ---- step 5a: cycle ----

    #[test]
    fn cycle_to_next_recent_same_app() {
        let ws = vec![
            w("0xA", "claude", 0),
            w("0xB", "claude", 1),
            w("0xC", "claude", 2),
        ];
        assert_eq!(
            decide(&ws, Some("0xA"), "claude", None),
            Decision::Cycle("0xB".to_string())
        );
    }

    // ---- step 5b: toggle back ----

    #[test]
    fn toggle_back_uses_mru_previous() {
        let ws = vec![
            w("0xA", "claude", 0),
            w("0xB", "kitty", 5),   // older
            w("0xC", "firefox", 1), // more recent
        ];
        assert_eq!(
            decide(&ws, Some("0xA"), "claude", Some("kitty")),
            Decision::ToggleBack("0xB".to_string()),
            "MRU previous (kitty) must beat the more-recent firefox"
        );
    }

    #[test]
    fn toggle_back_falls_back_when_previous_absent() {
        let ws = vec![
            w("0xA", "claude", 0),
            w("0xC", "firefox", 1),
            w("0xD", "kitty", 3),
        ];
        assert_eq!(
            decide(&ws, Some("0xA"), "claude", Some("vivaldi")),
            Decision::ToggleBack("0xC".to_string())
        );
    }

    #[test]
    fn toggle_back_ignores_previous_equal_to_target() {
        // Defensive: a stale state file pointing at the target app must not
        // pick the target as "the other app".
        let ws = vec![w("0xA", "claude", 0), w("0xB", "kitty", 1)];
        assert_eq!(
            decide(&ws, Some("0xA"), "claude", Some("claude")),
            Decision::ToggleBack("0xB".to_string())
        );
    }

    // ---- step 5c: hide ----

    #[test]
    fn hide_when_only_target_window_exists() {
        let ws = vec![w("0xA", "claude", 0)];
        assert_eq!(
            decide(&ws, Some("0xA"), "claude", None),
            Decision::Hide("0xA".to_string())
        );
    }

    #[test]
    fn hide_when_only_target_window_with_stale_previous() {
        let ws = vec![w("0xA", "claude", 0)];
        assert_eq!(
            decide(&ws, Some("0xA"), "claude", Some("kitty")),
            Decision::Hide("0xA".to_string())
        );
    }

    // ---- sway-style: every recency=0, ties broken by address ----

    #[test]
    fn sway_style_focus_uses_address_for_tie_break() {
        // sway tree traversal hands every window the same recency. The
        // algorithm must still produce a stable, deterministic pick — the
        // smallest address wins. This mirrors the previous `find()` first
        // hit semantics in i3ipc.rs.
        let ws = vec![
            w("0001", "kitty", 0),
            w("0010", "claude", 0),
            w("0020", "claude", 0),
        ];
        assert_eq!(
            decide(&ws, Some("0001"), "claude", None),
            Decision::Focus("0010".to_string())
        );
    }

    #[test]
    fn sway_style_cycle_uses_address_for_tie_break() {
        let ws = vec![
            w("0010", "claude", 0), // focused
            w("0020", "claude", 0),
            w("0030", "claude", 0),
        ];
        assert_eq!(
            decide(&ws, Some("0010"), "claude", None),
            Decision::Cycle("0020".to_string())
        );
    }

    #[test]
    fn sway_style_toggle_back_uses_address_for_tie_break() {
        let ws = vec![
            w("0010", "claude", 0), // focused
            w("0020", "kitty", 0),
            w("0030", "firefox", 0),
        ];
        // No MRU previous, so picks the alphabetically-first non-target
        // address, matching the old i3ipc tree-order behaviour.
        assert_eq!(
            decide(&ws, Some("0010"), "claude", None),
            Decision::ToggleBack("0020".to_string())
        );
    }
}
