//! Caps Lock as the beckon key: the decision half.
//!
//! Caps is an ALIAS for `ctrl+super+alt`, not a fifth modifier. The hook
//! injects the chord `RegisterHotKey` is already listening for, so `Combo`,
//! `parse_shortcuts` and `register_all` are untouched — and, critically,
//! the hook never calls `backend.beckon()`. A `WH_KEYBOARD_LL` callback
//! that outruns `LowLevelHooksTimeout` (300 ms by default) is silently
//! unhooked by Windows with no error anywhere, and `backend.beckon()` was
//! measured at ~57 ms typical and ~945 ms on the miss path. Here the
//! callback does a hash lookup and at most one `SendInput`; the real work
//! happens later on the ordinary `WM_HOTKEY` path.
//!
//! Windows-only in effect, but it lives in `beckon-cli` because CI passes
//! `--exclude beckon-windows` on the Linux and macOS jobs. A keyboard state
//! machine is the last thing that should be tested by one job in three.

use crate::shortcuts::{CapsTap, Shortcut};
use std::collections::HashSet;

pub const VK_CAPITAL: u32 = 0x14;
pub const VK_ESCAPE: u32 = 0x1B;
pub const VK_LCONTROL: u32 = 0xA2;
pub const VK_LWIN: u32 = 0x5B;
pub const VK_LMENU: u32 = 0xA4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edge {
    Down,
    Up,
}

/// One key transition as the hook sees it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyEvent {
    pub vk: u32,
    pub edge: Edge,
    /// True when this event carries our own `dwExtraInfo` marker, i.e. we
    /// injected it. Without this the first injected stroke would re-enter
    /// `decide` and the whole thing would spiral.
    pub injected_by_us: bool,
}

/// One key transition we are asking the OS to perform.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stroke {
    pub vk: u32,
    pub edge: Edge,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// Let it reach the application untouched.
    PassThrough,
    /// Eat it; the application never sees it.
    Swallow,
    /// Eat it and send these instead, in order, as one `SendInput` call.
    SwallowAndInject(Vec<Stroke>),
}

#[derive(Debug, Default)]
pub struct CapsState {
    held: bool,
    used: bool,
    consumed: HashSet<u32>,
}

/// Keys reachable through Caps: the main key of every binding whose combo
/// carries ctrl + super + alt.
///
/// Shift is deliberately not part of the filter. The user's physical Shift
/// is still down while the chord is injected, so `Caps+Shift+T` arrives at
/// the system as `ctrl+super+alt+shift+t` and lands on a shift binding by
/// itself. Filtering shift out here would make that binding unreachable.
pub fn bound_keys(shortcuts: &[Shortcut]) -> HashSet<u32> {
    shortcuts
        .iter()
        .filter(|s| s.combo.ctrl && s.combo.super_ && s.combo.alt)
        .map(|s| s.combo.key.win)
        .collect()
}

/// The whole chord as one burst.
///
/// Deliberately not "hold the modifiers down while Caps is held": that
/// shape has two defects this one does not. A bare Caps tap would press and
/// release the Windows key with nothing in between, which is exactly the
/// gesture that opens the Start menu; and with the modifiers physically
/// held, `Caps+<any key>` becomes a genuine ctrl+win+alt chord the shell
/// may act on. Here Win always has a real key between its down and its up,
/// and only bound keys are ever injected for.
fn chord(vk: u32) -> Vec<Stroke> {
    vec![
        Stroke {
            vk: VK_LCONTROL,
            edge: Edge::Down,
        },
        Stroke {
            vk: VK_LWIN,
            edge: Edge::Down,
        },
        Stroke {
            vk: VK_LMENU,
            edge: Edge::Down,
        },
        Stroke {
            vk,
            edge: Edge::Down,
        },
        Stroke { vk, edge: Edge::Up },
        Stroke {
            vk: VK_LMENU,
            edge: Edge::Up,
        },
        Stroke {
            vk: VK_LWIN,
            edge: Edge::Up,
        },
        Stroke {
            vk: VK_LCONTROL,
            edge: Edge::Up,
        },
    ]
}

fn tap(vk: u32) -> Vec<Stroke> {
    vec![
        Stroke {
            vk,
            edge: Edge::Down,
        },
        Stroke { vk, edge: Edge::Up },
    ]
}

/// Release the three modifiers the chord presses, unconditionally.
///
/// Emitted when Caps is released after at least one chord. Releasing a key
/// that is already up is a no-op, so the cost is one extra `SendInput`; the
/// cost of NOT doing it is a keyboard where every subsequent key is silently
/// a `ctrl+win+alt` chord, which is unrecoverable without killing beckon.
///
/// This exists because the chord's own key-ups are not guaranteed to land.
/// `SendInput` can insert fewer events than asked for — UIPI blocks it
/// without setting an error, and another thread holding the input queue
/// makes it return zero — and the `n↓` in the middle of the burst fires
/// `WM_HOTKEY`, whose handler runs `backend.beckon()` (57 ms typical, 945 ms
/// on the miss path) and pumps the message queue while it does. Anything in
/// that window can reorder or drop what follows.
fn release_modifiers() -> Vec<Stroke> {
    vec![
        Stroke {
            vk: VK_LMENU,
            edge: Edge::Up,
        },
        Stroke {
            vk: VK_LWIN,
            edge: Edge::Up,
        },
        Stroke {
            vk: VK_LCONTROL,
            edge: Edge::Up,
        },
    ]
}

/// Decide what to do with one key transition. Pure apart from `st`.
pub fn decide(ev: KeyEvent, st: &mut CapsState, bound: &HashSet<u32>, caps_tap: CapsTap) -> Action {
    if ev.injected_by_us {
        return Action::PassThrough;
    }
    match (ev.vk, ev.edge) {
        (VK_CAPITAL, Edge::Down) => {
            if !st.held {
                st.held = true;
                st.used = false;
                st.consumed.clear();
            }
            Action::Swallow
        }
        (VK_CAPITAL, Edge::Up) => {
            let used = st.used;
            st.held = false;
            st.used = false;
            // `consumed` is deliberately NOT cleared here: a key released
            // after Caps must still have its physical key-up swallowed, or
            // the application receives an up with no matching down. The
            // next Caps-down clears it.
            if used {
                // See `release_modifiers`: the chord's own key-ups are not
                // guaranteed to have landed, and the failure mode is a
                // keyboard where every key is silently a ctrl+win+alt chord.
                Action::SwallowAndInject(release_modifiers())
            } else {
                match caps_tap {
                    CapsTap::CapsLock => Action::SwallowAndInject(tap(VK_CAPITAL)),
                    CapsTap::Escape => Action::SwallowAndInject(tap(VK_ESCAPE)),
                    CapsTap::None => Action::Swallow,
                }
            }
        }
        (vk, Edge::Down) if st.held => {
            if !bound.contains(&vk) {
                return Action::PassThrough;
            }
            if st.consumed.contains(&vk) {
                return Action::Swallow; // auto-repeat
            }
            st.consumed.insert(vk);
            st.used = true;
            Action::SwallowAndInject(chord(vk))
        }
        (vk, Edge::Up) if st.consumed.contains(&vk) => {
            st.consumed.remove(&vk);
            Action::Swallow
        }
        _ => Action::PassThrough,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shortcuts::parse_shortcuts;

    fn shortcuts(text: &str) -> Vec<Shortcut> {
        parse_shortcuts(text).unwrap()
    }

    fn down(vk: u32) -> KeyEvent {
        KeyEvent {
            vk,
            edge: Edge::Down,
            injected_by_us: false,
        }
    }
    fn up(vk: u32) -> KeyEvent {
        KeyEvent {
            vk,
            edge: Edge::Up,
            injected_by_us: false,
        }
    }

    const VK_T: u32 = 0x54;
    const VK_F5: u32 = 0x74;
    const VK_SHIFT: u32 = 0x10;

    fn bound_t() -> HashSet<u32> {
        bound_keys(&shortcuts(r#""ctrl+super+alt+t" = "Terminal""#))
    }

    // ---------- bound_keys ----------

    #[test]
    fn bound_keys_takes_the_beckon_chord_only() {
        let b = bound_keys(&shortcuts(
            "\"ctrl+super+alt+t\" = \"Terminal\"\n\"ctrl+alt+e\" = \"Explorer\"\n",
        ));
        assert!(b.contains(&VK_T));
        assert_eq!(b.len(), 1, "ctrl+alt+e is not reachable through Caps");
    }

    /// Shift is deliberately ignored when collecting bound keys: the user's
    /// physical Shift is still down while the chord is injected, so
    /// `Caps+Shift+T` naturally lands on a `ctrl+super+alt+shift+t` binding.
    #[test]
    fn bound_keys_ignores_shift() {
        let b = bound_keys(&shortcuts(r#""ctrl+super+alt+shift+t" = "Terminal""#));
        assert!(b.contains(&VK_T));
    }

    // ---------- the chord ----------

    #[test]
    fn caps_then_a_bound_key_injects_the_whole_chord_in_one_burst() {
        let mut st = CapsState::default();
        let b = bound_t();
        assert_eq!(
            decide(down(VK_CAPITAL), &mut st, &b, CapsTap::CapsLock),
            Action::Swallow
        );
        let a = decide(down(VK_T), &mut st, &b, CapsTap::CapsLock);
        let Action::SwallowAndInject(strokes) = a else {
            panic!("expected an injection, got {a:?}");
        };
        let expect = vec![
            Stroke {
                vk: VK_LCONTROL,
                edge: Edge::Down,
            },
            Stroke {
                vk: VK_LWIN,
                edge: Edge::Down,
            },
            Stroke {
                vk: VK_LMENU,
                edge: Edge::Down,
            },
            Stroke {
                vk: VK_T,
                edge: Edge::Down,
            },
            Stroke {
                vk: VK_T,
                edge: Edge::Up,
            },
            Stroke {
                vk: VK_LMENU,
                edge: Edge::Up,
            },
            Stroke {
                vk: VK_LWIN,
                edge: Edge::Up,
            },
            Stroke {
                vk: VK_LCONTROL,
                edge: Edge::Up,
            },
        ];
        assert_eq!(strokes, expect);
    }

    /// The Start-menu hazard, pinned. Win goes down and up inside one burst
    /// with a real key between them; it is never pressed on its own.
    #[test]
    fn the_windows_key_is_never_pressed_without_a_key_between_down_and_up() {
        let mut st = CapsState::default();
        let b = bound_t();
        decide(down(VK_CAPITAL), &mut st, &b, CapsTap::CapsLock);
        let Action::SwallowAndInject(s) = decide(down(VK_T), &mut st, &b, CapsTap::CapsLock) else {
            panic!("expected an injection");
        };
        let win_down = s
            .iter()
            .position(|k| k.vk == VK_LWIN && k.edge == Edge::Down)
            .unwrap();
        let win_up = s
            .iter()
            .position(|k| k.vk == VK_LWIN && k.edge == Edge::Up)
            .unwrap();
        assert!(
            s[win_down + 1..win_up].iter().any(|k| k.vk == VK_T),
            "a bare Win press opens the Start menu"
        );
    }

    #[test]
    fn an_unbound_key_passes_through_untouched_while_caps_is_held() {
        let mut st = CapsState::default();
        let b = bound_t();
        decide(down(VK_CAPITAL), &mut st, &b, CapsTap::CapsLock);
        assert_eq!(
            decide(down(VK_F5), &mut st, &b, CapsTap::CapsLock),
            Action::PassThrough,
            "Caps+F5 must still be F5, not a stray ctrl+win+alt chord"
        );
        assert_eq!(
            decide(up(VK_F5), &mut st, &b, CapsTap::CapsLock),
            Action::PassThrough
        );
    }

    #[test]
    fn auto_repeat_injects_once_not_thirty_times_a_second() {
        let mut st = CapsState::default();
        let b = bound_t();
        decide(down(VK_CAPITAL), &mut st, &b, CapsTap::CapsLock);
        assert!(matches!(
            decide(down(VK_T), &mut st, &b, CapsTap::CapsLock),
            Action::SwallowAndInject(_)
        ));
        for _ in 0..5 {
            assert_eq!(
                decide(down(VK_T), &mut st, &b, CapsTap::CapsLock),
                Action::Swallow,
                "auto-repeat must not re-fire the hotkey"
            );
        }
    }

    #[test]
    fn the_physical_key_up_is_swallowed_too() {
        let mut st = CapsState::default();
        let b = bound_t();
        decide(down(VK_CAPITAL), &mut st, &b, CapsTap::CapsLock);
        decide(down(VK_T), &mut st, &b, CapsTap::CapsLock);
        assert_eq!(
            decide(up(VK_T), &mut st, &b, CapsTap::CapsLock),
            Action::Swallow,
            "we already injected T-up; a second one would reach the app unmatched"
        );
    }

    #[test]
    fn a_key_up_after_caps_was_released_is_still_swallowed() {
        let mut st = CapsState::default();
        let b = bound_t();
        decide(down(VK_CAPITAL), &mut st, &b, CapsTap::CapsLock);
        decide(down(VK_T), &mut st, &b, CapsTap::CapsLock);
        decide(up(VK_CAPITAL), &mut st, &b, CapsTap::CapsLock);
        assert_eq!(
            decide(up(VK_T), &mut st, &b, CapsTap::CapsLock),
            Action::Swallow,
            "releasing Caps first must not leak a stray T-up into the app"
        );
    }

    // ---------- the bare tap ----------

    #[test]
    fn a_bare_tap_restores_caps_lock_by_default() {
        let mut st = CapsState::default();
        let b = bound_t();
        decide(down(VK_CAPITAL), &mut st, &b, CapsTap::CapsLock);
        assert_eq!(
            decide(up(VK_CAPITAL), &mut st, &b, CapsTap::CapsLock),
            Action::SwallowAndInject(vec![
                Stroke {
                    vk: VK_CAPITAL,
                    edge: Edge::Down
                },
                Stroke {
                    vk: VK_CAPITAL,
                    edge: Edge::Up
                },
            ])
        );
    }

    #[test]
    fn a_bare_tap_can_be_escape_or_nothing() {
        for (tap_mode, expect) in [
            (
                CapsTap::Escape,
                Action::SwallowAndInject(vec![
                    Stroke {
                        vk: VK_ESCAPE,
                        edge: Edge::Down,
                    },
                    Stroke {
                        vk: VK_ESCAPE,
                        edge: Edge::Up,
                    },
                ]),
            ),
            (CapsTap::None, Action::Swallow),
        ] {
            let mut st = CapsState::default();
            let b = bound_t();
            decide(down(VK_CAPITAL), &mut st, &b, tap_mode);
            assert_eq!(
                decide(up(VK_CAPITAL), &mut st, &b, tap_mode),
                expect,
                "{tap_mode:?}"
            );
        }
    }

    #[test]
    fn caps_used_as_a_modifier_does_not_also_fire_the_tap_action() {
        let mut st = CapsState::default();
        let b = bound_t();
        decide(down(VK_CAPITAL), &mut st, &b, CapsTap::CapsLock);
        decide(down(VK_T), &mut st, &b, CapsTap::CapsLock);
        decide(up(VK_T), &mut st, &b, CapsTap::CapsLock);
        let a = decide(up(VK_CAPITAL), &mut st, &b, CapsTap::CapsLock);
        let Action::SwallowAndInject(s) = a else {
            panic!("expected the defensive modifier release, got {a:?}");
        };
        assert!(
            !s.iter().any(|k| k.vk == VK_CAPITAL),
            "Caps+T must not also toggle Caps Lock: {s:?}"
        );
    }

    /// The 2026-08-11 stuck-keyboard report: after Caps+N, every subsequent
    /// key behaved as ctrl+win+alt+key. Releasing Caps must put the
    /// modifiers down unconditionally, because the chord's own key-ups are
    /// not guaranteed to have landed.
    #[test]
    fn releasing_caps_after_a_chord_releases_every_modifier_it_pressed() {
        let mut st = CapsState::default();
        let b = bound_t();
        decide(down(VK_CAPITAL), &mut st, &b, CapsTap::CapsLock);
        decide(down(VK_T), &mut st, &b, CapsTap::CapsLock);
        let a = decide(up(VK_CAPITAL), &mut st, &b, CapsTap::CapsLock);
        let Action::SwallowAndInject(s) = a else {
            panic!("expected an injection, got {a:?}");
        };
        for vk in [VK_LCONTROL, VK_LWIN, VK_LMENU] {
            assert!(
                s.iter()
                    .any(|k| k.vk == vk && k.edge == Edge::Up),
                "modifier 0x{vk:02X} was not released: {s:?}"
            );
        }
        assert!(
            s.iter().all(|k| k.edge == Edge::Up),
            "the release must not press anything: {s:?}"
        );
    }

    /// A bare tap pressed no modifiers, so it must not release any either --
    /// that would desync a modifier the user is genuinely holding.
    #[test]
    fn a_bare_tap_does_not_release_modifiers() {
        let mut st = CapsState::default();
        let b = bound_t();
        decide(down(VK_CAPITAL), &mut st, &b, CapsTap::CapsLock);
        let a = decide(up(VK_CAPITAL), &mut st, &b, CapsTap::CapsLock);
        let Action::SwallowAndInject(s) = a else {
            panic!("expected the tap action, got {a:?}");
        };
        assert!(
            s.iter().all(|k| k.vk == VK_CAPITAL),
            "a tap must only send Caps: {s:?}"
        );
    }

    #[test]
    fn a_second_tap_after_a_chord_still_taps() {
        let mut st = CapsState::default();
        let b = bound_t();
        decide(down(VK_CAPITAL), &mut st, &b, CapsTap::CapsLock);
        decide(down(VK_T), &mut st, &b, CapsTap::CapsLock);
        decide(up(VK_T), &mut st, &b, CapsTap::CapsLock);
        decide(up(VK_CAPITAL), &mut st, &b, CapsTap::CapsLock);
        decide(down(VK_CAPITAL), &mut st, &b, CapsTap::CapsLock);
        let a = decide(up(VK_CAPITAL), &mut st, &b, CapsTap::CapsLock);
        let Action::SwallowAndInject(s) = a else {
            panic!("state leaked from the previous press: {a:?}");
        };
        assert!(
            s.iter().any(|k| k.vk == VK_CAPITAL),
            "the second press was a bare tap and must toggle Caps Lock: {s:?}"
        );
    }

    // ---------- recursion guard ----------

    #[test]
    fn our_own_injected_events_are_never_reprocessed() {
        let mut st = CapsState::default();
        let b = bound_t();
        decide(down(VK_CAPITAL), &mut st, &b, CapsTap::CapsLock);
        let mine = KeyEvent {
            vk: VK_LWIN,
            edge: Edge::Down,
            injected_by_us: true,
        };
        assert_eq!(
            decide(mine, &mut st, &b, CapsTap::CapsLock),
            Action::PassThrough
        );
        let mine_caps = KeyEvent {
            vk: VK_CAPITAL,
            edge: Edge::Down,
            injected_by_us: true,
        };
        assert_eq!(
            decide(mine_caps, &mut st, &b, CapsTap::CapsLock),
            Action::PassThrough,
            "the caps_tap injection must not re-enter the state machine"
        );
    }

    // ---------- inert when nothing is bound ----------

    #[test]
    fn nothing_is_touched_when_no_key_is_bound_to_the_chord() {
        let mut st = CapsState::default();
        let b: HashSet<u32> = HashSet::new();
        decide(down(VK_CAPITAL), &mut st, &b, CapsTap::CapsLock);
        assert_eq!(
            decide(down(VK_T), &mut st, &b, CapsTap::CapsLock),
            Action::PassThrough
        );
    }

    #[test]
    fn modifiers_the_user_holds_themselves_pass_through() {
        let mut st = CapsState::default();
        let b = bound_t();
        decide(down(VK_CAPITAL), &mut st, &b, CapsTap::CapsLock);
        assert_eq!(
            decide(down(VK_SHIFT), &mut st, &b, CapsTap::CapsLock),
            Action::PassThrough,
            "Shift must stay physically down so Caps+Shift+T reaches a shift binding"
        );
    }
}
