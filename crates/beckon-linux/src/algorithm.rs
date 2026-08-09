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

/// Every runtime window class that counts as "the app the user asked for".
///
/// One id legitimately shows up under more than one string, and which one
/// you get depends on the client, not on the user:
///   - a Wayland-native client reports the `.desktop` filename stem as its
///     `app_id` (`foot`, `org.gnome.Calculator`);
///   - the same app under X11/XWayland reports its `WM_CLASS`, which is what
///     `StartupWMClass=` records and is frequently capitalised differently
///     (`debian-xterm.desktop` ⇒ `XTerm`);
///   - an app with no `.desktop` file at all only ever has the raw string
///     the user typed.
///
/// Matching against a single string therefore misses real windows and makes
/// beckon launch a duplicate on every keypress. Comparison is
/// case-insensitive for the same reason (`xterm` vs `XTerm`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Target {
    /// Lowercased, de-duplicated, non-empty.
    candidates: Vec<String>,
    /// First candidate in original case — what error messages should show.
    primary: String,
}

impl Target {
    pub fn new<I, S>(candidates: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let mut out = Vec::new();
        let mut primary = String::new();
        for c in candidates {
            let c = c.into();
            let trimmed = c.trim();
            if trimmed.is_empty() {
                continue;
            }
            if primary.is_empty() {
                primary = trimmed.to_string();
            }
            let lowered = trimmed.to_lowercase();
            if !out.contains(&lowered) {
                out.push(lowered);
            }
        }
        Self {
            candidates: out,
            primary,
        }
    }

    /// Does this window's class name the target app?
    pub fn matches(&self, class: &str) -> bool {
        let lowered = class.trim().to_lowercase();
        !lowered.is_empty() && self.candidates.contains(&lowered)
    }

    /// The candidate to name in user-facing messages.
    pub fn primary(&self) -> &str {
        &self.primary
    }

    pub fn is_empty(&self) -> bool {
        self.candidates.is_empty()
    }
}

impl From<&str> for Target {
    fn from(s: &str) -> Self {
        Target::new([s])
    }
}

impl From<String> for Target {
    fn from(s: String) -> Self {
        Target::new([s])
    }
}

/// Pure decision function. See module docs for the algorithm. `active` is
/// the currently focused address (if any), `target` every class that counts
/// as the requested app, `previous_app` the class persisted in the MRU
/// state file.
pub fn decide(
    windows: &[WindowSnapshot],
    active: Option<&str>,
    target: impl Into<Target>,
    previous_app: Option<&str>,
) -> Decision {
    let target = target.into();
    let app_windows: Vec<&WindowSnapshot> = windows
        .iter()
        .filter(|w| target.matches(&w.class))
        .collect();

    if app_windows.is_empty() {
        return Decision::Launch;
    }

    let focused_in_app = active
        .and_then(|addr| windows.iter().find(|w| w.address == addr))
        .map(|w| target.matches(&w.class))
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

    // Step 5a: another window of the same app — rotate to the next one.
    //
    // The ring is ordered by *address*, deliberately not by `recency`.
    // On GNOME / Hyprland / X11 `recency` is real focus history, so it
    // reshuffles the moment we focus something: the window we just left
    // becomes the least-recent again and the next keypress goes straight
    // back to it. That is a 2-cycle — with three or more windows open,
    // windows 3..N are unreachable no matter how often the user presses
    // the key. Addresses are minted from the compositor's own window id
    // (con_id / stable_sequence / X11 id / Hyprland pointer), which is
    // stable for the window's lifetime and ordered by creation, so
    // rotating over it visits every window exactly once per lap.
    if app_windows.len() > 1 {
        let mut ring = app_windows.clone();
        ring.sort_by(|a, b| cmp_address(&a.address, &b.address));
        if let Some(pos) = ring.iter().position(|w| w.address == focused_addr) {
            return Decision::Cycle(ring[(pos + 1) % ring.len()].address.clone());
        }
    }

    // Step 5b: only one window of target. Honour the MRU "previous" first
    // (and only when it isn't `target`), otherwise pick the most-recent
    // window of any other app.
    let mru_choice = previous_app
        .filter(|app| !target.matches(app))
        .and_then(|app| {
            windows
                .iter()
                .filter(|w| w.class.eq_ignore_ascii_case(app))
                .min_by(cmp_recency_then_address)
        });
    let other = mru_choice.or_else(|| {
        windows
            .iter()
            .filter(|w| !target.matches(&w.class))
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
        .then_with(|| cmp_address(&a.address, &b.address))
}

/// Order two window addresses. Addresses are numeric ids rendered as
/// strings — decimal for sway/i3/X11/GNOME, `0x`-hex for Hyprland — so
/// compare them as numbers when both parse, otherwise byte-wise. Without
/// this, `"10"` would sort before `"9"` and the cycle ring would jump
/// around instead of following creation order.
fn cmp_address(a: &str, b: &str) -> Ordering {
    match (parse_address(a), parse_address(b)) {
        (Some(x), Some(y)) => x.cmp(&y),
        _ => a.cmp(b),
    }
}

fn parse_address(s: &str) -> Option<u128> {
    match s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        Some(hex) => u128::from_str_radix(hex, 16).ok(),
        None => s.parse::<u128>().ok(),
    }
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

    /// The bug this guards: cycling used to pick the *globally* least-recent
    /// other window of the app. On every backend whose `recency` is real
    /// focus history (GNOME, Hyprland, X11), focusing a window promotes it
    /// to 0 and demotes the one we just left — so the next press went
    /// straight back and windows 3..N were unreachable forever. Reproduced
    /// live on sway with three `foot` windows before the fix.
    #[test]
    fn cycle_reaches_every_window_under_true_mru_recency() {
        let addrs = ["101", "102", "103"];
        // Model a real MRU backend: the focused window is recency 0 and the
        // rest keep their relative order behind it.
        let snapshot = |focused: &str| -> Vec<WindowSnapshot> {
            let mut rest: Vec<&str> = addrs.iter().copied().filter(|a| *a != focused).collect();
            rest.sort();
            let mut out = vec![w(focused, "claude", 0)];
            for (i, a) in rest.iter().enumerate() {
                out.push(w(a, "claude", i as i32 + 1));
            }
            out
        };

        let mut focused = "101".to_string();
        let mut visited = vec![focused.clone()];
        for _ in 0..5 {
            let ws = snapshot(&focused);
            match decide(&ws, Some(&focused), "claude", None) {
                Decision::Cycle(next) => {
                    focused = next;
                    visited.push(focused.clone());
                }
                other => panic!("expected Cycle, got {other:?}"),
            }
        }
        assert_eq!(
            visited,
            vec!["101", "102", "103", "101", "102", "103"],
            "cycling must round-robin every window, not ping-pong between two"
        );
    }

    #[test]
    fn cycle_wraps_from_last_window_to_first() {
        let ws = vec![
            w("103", "claude", 0),
            w("101", "claude", 1),
            w("102", "claude", 2),
        ];
        assert_eq!(
            decide(&ws, Some("103"), "claude", None),
            Decision::Cycle("101".to_string())
        );
    }

    /// Addresses are numeric ids rendered as strings, so `"10"` must sort
    /// after `"9"` — a byte-wise ring would jump around instead of
    /// following window creation order.
    #[test]
    fn cycle_ring_orders_addresses_numerically() {
        let ws = vec![
            w("9", "claude", 0),
            w("10", "claude", 1),
            w("11", "claude", 2),
        ];
        assert_eq!(
            decide(&ws, Some("9"), "claude", None),
            Decision::Cycle("10".to_string())
        );
    }

    // ---- target matching ----

    /// `xterm.desktop` has no `StartupWMClass` and the running window
    /// advertises `WM_CLASS` = `XTerm`. A byte-wise compare made beckon
    /// launch a new xterm on every keypress — reproduced live on sway.
    #[test]
    fn target_matches_class_case_insensitively() {
        let ws = vec![w("1", "XTerm", 0), w("2", "kitty", 1)];
        assert_eq!(
            decide(&ws, Some("2"), "xterm", None),
            Decision::Focus("1".to_string())
        );
    }

    /// The `.desktop` stem and `StartupWMClass` are both legitimate runtime
    /// classes — Wayland clients report the former, X11/XWayland the latter.
    #[test]
    fn target_matches_any_candidate() {
        let target = Target::new(["debian-xterm", "XTerm"]);
        let ws = vec![w("1", "kitty", 0), w("2", "XTerm", 1)];
        assert_eq!(
            decide(&ws, Some("1"), target.clone(), None),
            Decision::Focus("2".to_string())
        );

        // ...and the Wayland-side candidate still works on its own.
        let ws = vec![w("1", "kitty", 0), w("2", "debian-xterm", 1)];
        assert_eq!(
            decide(&ws, Some("1"), target, None),
            Decision::Focus("2".to_string())
        );
    }

    #[test]
    fn target_drops_empty_and_duplicate_candidates() {
        let t = Target::new(["Foo", "", "  ", "foo", "FOO"]);
        assert_eq!(t.primary(), "Foo");
        assert!(t.matches("fOo"));
        assert!(!t.matches(""));
        assert!(!t.matches("bar"));
        assert!(Target::new(Vec::<String>::new()).is_empty());
    }

    /// Step 5b must not toggle "back" to the target itself just because the
    /// MRU file recorded it under a different case.
    #[test]
    fn toggle_back_compares_previous_app_case_insensitively() {
        let ws = vec![w("1", "XTerm", 0), w("2", "kitty", 1)];
        assert_eq!(
            decide(&ws, Some("1"), "xterm", Some("xterm")),
            Decision::ToggleBack("2".to_string()),
            "previous == target (modulo case) must fall through to the other app"
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
