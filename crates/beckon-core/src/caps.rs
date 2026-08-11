//! Caps Lock as the beckon key: the decision half.
//!
//! Caps is an ALIAS for the configured chord -- `ctrl+super+alt` by default
//! -- not a fifth modifier. The hook injects the chord `RegisterHotKey` is
//! already listening for, so `Combo`, `parse_shortcuts` and `register_all`
//! are untouched — and, critically, the hook never calls `backend.beckon()`.
//! A `WH_KEYBOARD_LL` callback that outruns `LowLevelHooksTimeout` (300 ms
//! by default) is silently unhooked by Windows with no error anywhere, and
//! `backend.beckon()` was measured at ~57 ms typical and ~945 ms on the
//! miss path. Here the callback does a hash lookup and at most one
//! `SendInput`; the real work happens later on the ordinary `WM_HOTKEY`
//! path.
//!
//! Windows-only in effect, but it lives in `beckon-core` because CI passes
//! `--exclude beckon-windows` on the Linux and macOS jobs. A keyboard state
//! machine is the last thing that should be tested by one job in three.
//!
//! Tap-vs-hold follows the same rule kanata does, since that is what this
//! project's users already have in their fingers: a release inside
//! `HOLD_TIMEOUT_MS` with no key pressed is a tap, anything else is a hold.
//! Where beckon differs deliberately: kanata's plain `tap-hold` engages the
//! hold purely on the clock, so `Caps+N` pressed quickly emits `Esc` then
//! `n`. beckon engages the hold as soon as any key is pressed, which is
//! kanata's `tap-hold-press` behaviour and needs no 200 ms wait before the
//! first chord works.

use crate::shortcuts::{CapsTap, Chord, Shortcut};
use std::collections::HashSet;

/// How long Caps may be held before a release stops counting as a tap.
///
/// 200 ms, matching the `tap-hold 200 200` this user's kanata config binds
/// Caps to. Resting a finger on Caps and letting go must not emit anything;
/// that is what "hold" means, and every tap-hold implementation decides it
/// on a clock.
pub const HOLD_TIMEOUT_MS: u32 = 200;

pub const VK_CAPITAL: u32 = 0x14;
pub const VK_ESCAPE: u32 = 0x1B;
pub const VK_LCONTROL: u32 = 0xA2;
pub const VK_LWIN: u32 = 0x5B;
pub const VK_LMENU: u32 = 0xA4;
/// `VK_NONAME` (0xFC) is documented as reserved and produces no character,
/// no navigation and no shell action. It exists here only to be a
/// non-modifier key between a modifier's down and its up.
pub const VK_NONAME: u32 = 0xFC;

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
    /// `KBDLLHOOKSTRUCT.time` — milliseconds since boot. Compared only
    /// against another value from the same source, with `wrapping_sub`, so
    /// the 49-day rollover costs at most one mistimed keypress.
    pub time_ms: u32,
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
    /// Set by ANY key pressed while Caps is down, not only a bound one.
    /// `Caps+g` where `g` is unbound is still someone using Caps as a
    /// modifier and getting nothing; it must not also toggle Caps Lock on
    /// release.
    used: bool,
    /// The chord that was actually injected, if any -- not merely whether
    /// one was. `reload()` can swap the configured chord between Caps-down
    /// and Caps-up, and releasing a different set than was pressed leaves a
    /// modifier stuck down, which the user cannot recover from without
    /// killing beckon.
    injected: Option<Chord>,
    consumed: HashSet<u32>,
    down_at: u32,
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

/// The modifier VKs a chord presses, in a fixed order.
fn modifier_vks(hold: Chord) -> Vec<u32> {
    let mut v = Vec::with_capacity(3);
    if hold.ctrl {
        v.push(VK_LCONTROL);
    }
    if hold.super_ {
        v.push(VK_LWIN);
    }
    if hold.alt {
        v.push(VK_LMENU);
    }
    v
}

/// The whole chord as one burst.
///
/// Deliberately not "hold the modifiers down while Caps is held": that
/// shape has two defects this one does not. A bare Caps tap would press and
/// release the Windows key with nothing in between, which is exactly the
/// gesture that opens the Start menu; and with the modifiers physically
/// held, `Caps+<any key>` becomes a genuine ctrl+win+alt chord the shell
/// may act on. Here every modifier in the burst has a real key between its
/// own down and its own up, and only bound keys are ever injected for.
fn chord(hold: Chord, vk: u32) -> Vec<Stroke> {
    let mods = modifier_vks(hold);
    let mut out = Vec::with_capacity(mods.len() * 2 + 2);
    for &m in &mods {
        out.push(Stroke {
            vk: m,
            edge: Edge::Down,
        });
    }
    out.push(Stroke {
        vk,
        edge: Edge::Down,
    });
    out.push(Stroke { vk, edge: Edge::Up });
    for &m in mods.iter().rev() {
        out.push(Stroke {
            vk: m,
            edge: Edge::Up,
        });
    }
    out
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

/// Release the modifiers that chord presses, unconditionally.
///
/// Emitted when Caps is released after at least one chord. Releasing a key
/// that is already up is a no-op, so the cost is one extra `SendInput`; the
/// cost of NOT doing it is a keyboard where every subsequent key is silently
/// held down by whichever modifiers that chord presses, which is
/// unrecoverable without killing beckon.
///
/// This exists because the chord's own key-ups are not guaranteed to land.
/// `SendInput` can insert fewer events than asked for -- UIPI blocks it
/// without setting an error, and another thread holding the input queue
/// makes it return zero -- and the `n` down in the middle of the burst fires
/// `WM_HOTKEY`, whose handler runs `backend.beckon()` (57 ms typical, 945 ms
/// on the miss path) and pumps the message queue while it does. Anything in
/// that window can reorder or drop what follows.
///
/// The burst OPENS WITH A FILLER KEY, and that is load-bearing regardless of
/// how many modifiers this chord has. The invariant `chord()` satisfies --
/// every modifier has a non-modifier key between its own down and its own
/// up -- spans both bursts, because the down happened in the chord and the
/// up happens here. The hazard is specific to Win: when the chord includes
/// `super_` and its down landed but its own up did not, a bare `Win` up
/// here -- with nothing between it and the down that came before, across
/// two separate `SendInput` calls -- is the Start-menu gesture. `VK_NONAME`
/// breaks that pair. A chord without `super_` never hits this case, but the
/// filler stays unconditional so this function does not need to know which
/// modifier is dangerous -- one extra harmless `SendInput` is cheaper than
/// that knowledge going stale the next time the hazard changes.
fn release_modifiers(hold: Chord) -> Vec<Stroke> {
    let mut out = vec![
        Stroke {
            vk: VK_NONAME,
            edge: Edge::Down,
        },
        Stroke {
            vk: VK_NONAME,
            edge: Edge::Up,
        },
    ];
    for m in modifier_vks(hold).into_iter().rev() {
        out.push(Stroke {
            vk: m,
            edge: Edge::Up,
        });
    }
    out
}

/// Decide what to do with one key transition. Pure apart from `st`.
pub fn decide(
    ev: KeyEvent,
    st: &mut CapsState,
    bound: &HashSet<u32>,
    hold: Chord,
    caps_tap: CapsTap,
) -> Action {
    if ev.injected_by_us {
        return Action::PassThrough;
    }
    match (ev.vk, ev.edge) {
        (VK_CAPITAL, Edge::Down) => {
            if !st.held {
                st.held = true;
                st.used = false;
                st.injected = None;
                st.consumed.clear();
                st.down_at = ev.time_ms;
            }
            Action::Swallow
        }
        (VK_CAPITAL, Edge::Up) => {
            // Tap only when Caps did nothing AND was let go quickly. A long
            // hold is a hold even if no key followed it -- see
            // `HOLD_TIMEOUT_MS`.
            let held_ms = ev.time_ms.wrapping_sub(st.down_at);
            let used = st.used || held_ms >= HOLD_TIMEOUT_MS;
            let injected = st.injected.take();
            st.held = false;
            st.used = false;
            // `consumed` is deliberately NOT cleared here: a key released
            // after Caps must still have its physical key-up swallowed, or
            // the application receives an up with no matching down. The
            // next Caps-down clears it.
            if used {
                // See `release_modifiers`: the chord's own key-ups are not
                // guaranteed to have landed, and the failure mode is a
                // keyboard where every key is silently held down by
                // whichever modifiers that chord presses.
                // Only worth doing when a chord was actually injected, and
                // then only for the chord that was ACTUALLY injected -- the
                // file watcher can reload `hold` between Caps-down and
                // Caps-up, and releasing a different set than was pressed
                // leaves a modifier stuck down. A timed-out hold, or Caps
                // plus an unbound key, pressed no modifier -- releasing one
                // there would desync a modifier the user is genuinely
                // holding.
                if let Some(pressed) = injected {
                    Action::SwallowAndInject(release_modifiers(pressed))
                } else {
                    Action::Swallow
                }
            } else {
                match caps_tap {
                    CapsTap::CapsLock => Action::SwallowAndInject(tap(VK_CAPITAL)),
                    CapsTap::Escape => Action::SwallowAndInject(tap(VK_ESCAPE)),
                    CapsTap::None => Action::Swallow,
                }
            }
        }
        (vk, Edge::Down) if st.held => {
            // ANY key marks Caps as used, including one with no binding and
            // including a modifier the user is stacking on top. Otherwise
            // `Caps+g` types `g` and then toggles Caps Lock on release,
            // which is the surprise this flag exists to prevent.
            st.used = true;
            if !bound.contains(&vk) {
                return Action::PassThrough;
            }
            if st.consumed.contains(&vk) {
                return Action::Swallow; // auto-repeat
            }
            st.consumed.insert(vk);
            st.injected = Some(hold);
            Action::SwallowAndInject(chord(hold, vk))
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
        at(vk, Edge::Down, 0)
    }
    fn up(vk: u32) -> KeyEvent {
        // Default releases are "immediate": inside the hold timeout, so a
        // test that says nothing about timing gets the tap branch.
        at(vk, Edge::Up, 10)
    }
    fn at(vk: u32, edge: Edge, time_ms: u32) -> KeyEvent {
        KeyEvent {
            vk,
            edge,
            injected_by_us: false,
            time_ms,
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
            decide(down(VK_CAPITAL), &mut st, &b, HOLD, CapsTap::CapsLock),
            Action::Swallow
        );
        let a = decide(down(VK_T), &mut st, &b, HOLD, CapsTap::CapsLock);
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
        decide(down(VK_CAPITAL), &mut st, &b, HOLD, CapsTap::CapsLock);
        let Action::SwallowAndInject(s) = decide(down(VK_T), &mut st, &b, HOLD, CapsTap::CapsLock)
        else {
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
        decide(down(VK_CAPITAL), &mut st, &b, HOLD, CapsTap::CapsLock);
        assert_eq!(
            decide(down(VK_F5), &mut st, &b, HOLD, CapsTap::CapsLock),
            Action::PassThrough,
            "Caps+F5 must still be F5, not a stray ctrl+win+alt chord"
        );
        assert_eq!(
            decide(up(VK_F5), &mut st, &b, HOLD, CapsTap::CapsLock),
            Action::PassThrough
        );
    }

    #[test]
    fn auto_repeat_injects_once_not_thirty_times_a_second() {
        let mut st = CapsState::default();
        let b = bound_t();
        decide(down(VK_CAPITAL), &mut st, &b, HOLD, CapsTap::CapsLock);
        assert!(matches!(
            decide(down(VK_T), &mut st, &b, HOLD, CapsTap::CapsLock),
            Action::SwallowAndInject(_)
        ));
        for _ in 0..5 {
            assert_eq!(
                decide(down(VK_T), &mut st, &b, HOLD, CapsTap::CapsLock),
                Action::Swallow,
                "auto-repeat must not re-fire the hotkey"
            );
        }
    }

    #[test]
    fn the_physical_key_up_is_swallowed_too() {
        let mut st = CapsState::default();
        let b = bound_t();
        decide(down(VK_CAPITAL), &mut st, &b, HOLD, CapsTap::CapsLock);
        decide(down(VK_T), &mut st, &b, HOLD, CapsTap::CapsLock);
        assert_eq!(
            decide(up(VK_T), &mut st, &b, HOLD, CapsTap::CapsLock),
            Action::Swallow,
            "we already injected T-up; a second one would reach the app unmatched"
        );
    }

    #[test]
    fn a_key_up_after_caps_was_released_is_still_swallowed() {
        let mut st = CapsState::default();
        let b = bound_t();
        decide(down(VK_CAPITAL), &mut st, &b, HOLD, CapsTap::CapsLock);
        decide(down(VK_T), &mut st, &b, HOLD, CapsTap::CapsLock);
        decide(up(VK_CAPITAL), &mut st, &b, HOLD, CapsTap::CapsLock);
        assert_eq!(
            decide(up(VK_T), &mut st, &b, HOLD, CapsTap::CapsLock),
            Action::Swallow,
            "releasing Caps first must not leak a stray T-up into the app"
        );
    }

    // ---------- the bare tap ----------

    #[test]
    fn a_bare_tap_restores_caps_lock_by_default() {
        let mut st = CapsState::default();
        let b = bound_t();
        decide(down(VK_CAPITAL), &mut st, &b, HOLD, CapsTap::CapsLock);
        assert_eq!(
            decide(up(VK_CAPITAL), &mut st, &b, HOLD, CapsTap::CapsLock),
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
            decide(down(VK_CAPITAL), &mut st, &b, HOLD, tap_mode);
            assert_eq!(
                decide(up(VK_CAPITAL), &mut st, &b, HOLD, tap_mode),
                expect,
                "{tap_mode:?}"
            );
        }
    }

    #[test]
    fn caps_used_as_a_modifier_does_not_also_fire_the_tap_action() {
        let mut st = CapsState::default();
        let b = bound_t();
        decide(down(VK_CAPITAL), &mut st, &b, HOLD, CapsTap::CapsLock);
        decide(down(VK_T), &mut st, &b, HOLD, CapsTap::CapsLock);
        decide(up(VK_T), &mut st, &b, HOLD, CapsTap::CapsLock);
        let a = decide(up(VK_CAPITAL), &mut st, &b, HOLD, CapsTap::CapsLock);
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
        decide(down(VK_CAPITAL), &mut st, &b, HOLD, CapsTap::CapsLock);
        decide(down(VK_T), &mut st, &b, HOLD, CapsTap::CapsLock);
        let a = decide(up(VK_CAPITAL), &mut st, &b, HOLD, CapsTap::CapsLock);
        let Action::SwallowAndInject(s) = a else {
            panic!("expected an injection, got {a:?}");
        };
        for vk in [VK_LCONTROL, VK_LWIN, VK_LMENU] {
            assert!(
                s.iter().any(|k| k.vk == vk && k.edge == Edge::Up),
                "modifier 0x{vk:02X} was not released: {s:?}"
            );
        }
        // Not "must not press anything" -- the filler key legitimately goes
        // down and up (see `release_modifiers`). What must hold is narrower
        // and still load-bearing: none of the three modifiers themselves is
        // ever pressed here, only released.
        assert!(
            s.iter()
                .filter(|k| [VK_LCONTROL, VK_LWIN, VK_LMENU].contains(&k.vk))
                .all(|k| k.edge == Edge::Up),
            "the release must not press a modifier: {s:?}"
        );
    }

    /// A bare tap pressed no modifiers, so it must not release any either --
    /// that would desync a modifier the user is genuinely holding.
    #[test]
    fn a_bare_tap_does_not_release_modifiers() {
        let mut st = CapsState::default();
        let b = bound_t();
        decide(down(VK_CAPITAL), &mut st, &b, HOLD, CapsTap::CapsLock);
        let a = decide(up(VK_CAPITAL), &mut st, &b, HOLD, CapsTap::CapsLock);
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
        decide(down(VK_CAPITAL), &mut st, &b, HOLD, CapsTap::CapsLock);
        decide(down(VK_T), &mut st, &b, HOLD, CapsTap::CapsLock);
        decide(up(VK_T), &mut st, &b, HOLD, CapsTap::CapsLock);
        decide(up(VK_CAPITAL), &mut st, &b, HOLD, CapsTap::CapsLock);
        decide(down(VK_CAPITAL), &mut st, &b, HOLD, CapsTap::CapsLock);
        let a = decide(up(VK_CAPITAL), &mut st, &b, HOLD, CapsTap::CapsLock);
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
        decide(down(VK_CAPITAL), &mut st, &b, HOLD, CapsTap::CapsLock);
        let mine = KeyEvent {
            vk: VK_LWIN,
            edge: Edge::Down,
            injected_by_us: true,
            time_ms: 0,
        };
        assert_eq!(
            decide(mine, &mut st, &b, HOLD, CapsTap::CapsLock),
            Action::PassThrough
        );
        let mine_caps = KeyEvent {
            vk: VK_CAPITAL,
            edge: Edge::Down,
            injected_by_us: true,
            time_ms: 0,
        };
        assert_eq!(
            decide(mine_caps, &mut st, &b, HOLD, CapsTap::CapsLock),
            Action::PassThrough,
            "the caps_tap injection must not re-enter the state machine"
        );
    }

    // ---------- inert when nothing is bound ----------

    #[test]
    fn nothing_is_touched_when_no_key_is_bound_to_the_chord() {
        let mut st = CapsState::default();
        let b: HashSet<u32> = HashSet::new();
        decide(down(VK_CAPITAL), &mut st, &b, HOLD, CapsTap::CapsLock);
        assert_eq!(
            decide(down(VK_T), &mut st, &b, HOLD, CapsTap::CapsLock),
            Action::PassThrough
        );
    }

    #[test]
    fn modifiers_the_user_holds_themselves_pass_through() {
        let mut st = CapsState::default();
        let b = bound_t();
        decide(down(VK_CAPITAL), &mut st, &b, HOLD, CapsTap::CapsLock);
        assert_eq!(
            decide(down(VK_SHIFT), &mut st, &b, HOLD, CapsTap::CapsLock),
            Action::PassThrough,
            "Shift must stay physically down so Caps+Shift+T reaches a shift binding"
        );
    }

    // ---------- tap vs hold ----------

    /// `Caps+<unbound key>` is someone using Caps as a modifier and getting
    /// nothing back. It must not ALSO toggle Caps Lock when they let go.
    #[test]
    fn an_unbound_key_still_counts_as_using_caps() {
        let mut st = CapsState::default();
        let b = bound_t();
        decide(down(VK_CAPITAL), &mut st, &b, HOLD, CapsTap::CapsLock);
        decide(down(VK_F5), &mut st, &b, HOLD, CapsTap::CapsLock);
        decide(up(VK_F5), &mut st, &b, HOLD, CapsTap::CapsLock);
        assert_eq!(
            decide(up(VK_CAPITAL), &mut st, &b, HOLD, CapsTap::CapsLock),
            Action::Swallow,
            "Caps+F5 must not toggle Caps Lock on release"
        );
    }

    /// A hold is a hold even when no key follows it. kanata decides this on
    /// a 200 ms clock and so does beckon.
    #[test]
    fn holding_caps_past_the_timeout_is_never_a_tap() {
        let mut st = CapsState::default();
        let b = bound_t();
        decide(
            at(VK_CAPITAL, Edge::Down, 1_000),
            &mut st,
            &b,
            HOLD,
            CapsTap::CapsLock,
        );
        let a = decide(
            at(VK_CAPITAL, Edge::Up, 1_000 + HOLD_TIMEOUT_MS),
            &mut st,
            &b,
            HOLD,
            CapsTap::CapsLock,
        );
        assert_eq!(a, Action::Swallow, "a long hold emitted something: {a:?}");
    }

    #[test]
    fn a_quick_tap_is_still_a_tap() {
        let mut st = CapsState::default();
        let b = bound_t();
        decide(
            at(VK_CAPITAL, Edge::Down, 1_000),
            &mut st,
            &b,
            HOLD,
            CapsTap::CapsLock,
        );
        let a = decide(
            at(VK_CAPITAL, Edge::Up, 1_000 + HOLD_TIMEOUT_MS - 1),
            &mut st,
            &b,
            HOLD,
            CapsTap::CapsLock,
        );
        assert!(
            matches!(a, Action::SwallowAndInject(ref s) if s.iter().any(|k| k.vk == VK_CAPITAL)),
            "a quick tap must still toggle Caps Lock: {a:?}"
        );
    }

    /// The tick count wraps every ~49 days. `wrapping_sub` must keep a tap
    /// across the boundary a tap rather than reading as a 49-day hold.
    #[test]
    fn the_millisecond_counter_may_wrap() {
        let mut st = CapsState::default();
        let b = bound_t();
        decide(
            at(VK_CAPITAL, Edge::Down, u32::MAX - 5),
            &mut st,
            &b,
            HOLD,
            CapsTap::CapsLock,
        );
        let a = decide(
            at(VK_CAPITAL, Edge::Up, 10),
            &mut st,
            &b,
            HOLD,
            CapsTap::CapsLock,
        );
        assert!(
            matches!(a, Action::SwallowAndInject(_)),
            "a 15 ms tap across the rollover read as a hold: {a:?}"
        );
    }

    /// A timed-out hold pressed nothing, so there is nothing to release --
    /// emitting modifier-ups there would desync a modifier the user holds.
    #[test]
    fn a_timed_out_hold_does_not_release_modifiers() {
        let mut st = CapsState::default();
        let b = bound_t();
        decide(
            at(VK_CAPITAL, Edge::Down, 0),
            &mut st,
            &b,
            HOLD,
            CapsTap::CapsLock,
        );
        assert_eq!(
            decide(
                at(VK_CAPITAL, Edge::Up, 5_000),
                &mut st,
                &b,
                HOLD,
                CapsTap::CapsLock
            ),
            Action::Swallow
        );
    }

    /// The property that actually matters, stated correctly: every modifier in
    /// a burst must have a non-modifier key between its own down and its own
    /// up. `chord()` satisfies it by construction. `release_modifiers()` did
    /// not -- it emitted only modifier-ups, and a bare Win-up is the gesture
    /// that opens the Start menu.
    #[test]
    fn release_modifiers_never_starts_with_a_bare_modifier_up() {
        let out = release_modifiers(HOLD);
        let first = out.first().expect("release burst must not be empty");
        assert_eq!(
            (first.vk, first.edge),
            (VK_NONAME, Edge::Down),
            "the burst must open with a filler key-down, or the Win-up that \
             follows is a bare Win tap and opens Start: {out:?}"
        );
        assert_eq!(
            (out[1].vk, out[1].edge),
            (VK_NONAME, Edge::Up),
            "the filler must be released immediately: {out:?}"
        );
        for vk in [VK_LCONTROL, VK_LWIN, VK_LMENU] {
            assert!(
                out.iter().any(|s| s.vk == vk && s.edge == Edge::Up),
                "still must release {vk:#x}: {out:?}"
            );
        }
    }

    // ---------- the chord as a parameter ----------

    const HOLD: Chord = Chord {
        ctrl: true,
        super_: true,
        alt: true,
    };

    /// The burst contains exactly the chord's modifiers and no others.
    #[test]
    fn a_two_modifier_chord_injects_two_modifiers() {
        let hold = Chord::parse("ctrl+alt").unwrap();
        let out = chord(hold, 0x4E);
        assert!(
            !out.iter().any(|s| s.vk == VK_LWIN),
            "no Win asked for: {out:?}"
        );
        assert!(out.iter().any(|s| s.vk == VK_LCONTROL));
        assert!(out.iter().any(|s| s.vk == VK_LMENU));
    }

    /// The property, tested structurally rather than against a fixed vector, so
    /// it holds for every chord shape and not just the default.
    #[test]
    fn every_modifier_in_a_burst_has_a_real_key_between_its_down_and_its_up() {
        for spec in ["ctrl", "ctrl+alt", "super+alt", "ctrl+super+alt"] {
            let hold = Chord::parse(spec).unwrap();
            let out = chord(hold, 0x4E);
            for m in [VK_LCONTROL, VK_LWIN, VK_LMENU] {
                let down = out.iter().position(|s| s.vk == m && s.edge == Edge::Down);
                let up = out.iter().position(|s| s.vk == m && s.edge == Edge::Up);
                let (Some(down), Some(up)) = (down, up) else {
                    continue;
                };
                assert!(
                    out[down..up]
                        .iter()
                        .any(|s| !matches!(s.vk, VK_LCONTROL | VK_LWIN | VK_LMENU)),
                    "{spec}: modifier {m:#x} has no real key between its down and up: {out:?}"
                );
            }
        }
    }

    #[test]
    fn release_names_exactly_the_modifiers_the_chord_pressed() {
        let hold = Chord::parse("ctrl+alt").unwrap();
        let rel = release_modifiers(hold);
        assert!(rel
            .iter()
            .any(|s| s.vk == VK_LCONTROL && s.edge == Edge::Up));
        assert!(rel.iter().any(|s| s.vk == VK_LMENU && s.edge == Edge::Up));
        assert!(
            !rel.iter().any(|s| s.vk == VK_LWIN),
            "releasing a modifier the chord never pressed desyncs one the user \
             may be genuinely holding: {rel:?}"
        );
    }

    /// The reason `injected` is an Option<Chord> and not a bool: the file
    /// watcher can reload between Caps-down and Caps-up.
    #[test]
    fn changing_the_chord_mid_hold_releases_what_was_actually_pressed() {
        let old = Chord::parse("ctrl+super+alt").unwrap();
        let new = Chord::parse("ctrl+alt").unwrap();
        let mut st = CapsState::default();
        let b: HashSet<u32> = [0x4E].into_iter().collect();

        decide(down(VK_CAPITAL), &mut st, &b, old, CapsTap::CapsLock);
        decide(down(0x4E), &mut st, &b, old, CapsTap::CapsLock);
        // reload() lands here and swaps the chord.
        let act = decide(
            at(VK_CAPITAL, Edge::Up, 10),
            &mut st,
            &b,
            new,
            CapsTap::CapsLock,
        );
        let Action::SwallowAndInject(rel) = act else {
            panic!("a chord was injected, so the modifiers must be released: {act:?}");
        };
        assert!(
            rel.iter().any(|s| s.vk == VK_LWIN && s.edge == Edge::Up),
            "Win was pressed by the OLD chord and must still be released: {rel:?}"
        );
    }
}
