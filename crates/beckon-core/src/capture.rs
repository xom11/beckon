//! Chord capture: what to do with each key event while the field is
//! recording. Pure, and beside `caps::decide` for the same reason that one
//! is pure -- a keyboard state machine is the last thing that should be
//! tested by one job in three.
//!
//! **`step` does not allocate and does not format.** It runs inside the
//! `WH_KEYBOARD_LL` callback, which Windows silently unhooks if it overruns
//! `LowLevelHooksTimeout` (300 ms by default) with no error anywhere. Every
//! display string is built later, on the UI thread, by the methods below.

use crate::caps::{Edge, KeyEvent, VK_CAPITAL, VK_ESCAPE, VK_LCONTROL, VK_LMENU, VK_LWIN};
use crate::shortcuts::{lookup_win_vk, Combo, KeyDef};

// The modifier VKs `caps.rs` had no use for. `KBDLLHOOKSTRUCT` reports the
// sided code for a real keypress, but the unsided ones are what an injected
// stroke and every test spell, so both have to resolve.
const VK_SHIFT: u32 = 0x10;
const VK_CONTROL: u32 = 0x11;
const VK_MENU: u32 = 0x12;
const VK_RWIN: u32 = 0x5C;
const VK_LSHIFT: u32 = 0xA0;
const VK_RSHIFT: u32 = 0xA1;
const VK_RCONTROL: u32 = 0xA3;
const VK_RMENU: u32 = 0xA5;

const VK_L: u32 = 0x4C;
const VK_NUMLOCK: u32 = 0x90;
const VK_SCROLL: u32 = 0x91;

/// How many key-downs may be held at once.
///
/// Twelve, because that is the ceiling and not a guess: eight sided modifier
/// keys, the three unsided codes an injected stroke can carry instead, and
/// one main key. A thirteenth is dropped rather than growing the array,
/// which keeps `step` allocation-free; no keyboard can reach it.
const HELD_MAX: usize = 12;

/// Why a keystroke was heard and not recorded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// A key with no modifier held.
    NoModifier,
    /// A key the 81-key table cannot name: numpad, media, IME.
    UnknownKey,
    /// A chord Windows keeps for itself. Measured on a14: the hook sees
    /// `Win+L` but cannot suppress it, so recording it would hand the user
    /// a binding that can never fire.
    Reserved,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// Swallow it; the window has nothing to redraw.
    Ignored,
    /// The held modifier set changed.
    Partial,
    /// A complete chord is in `CaptureState::captured`.
    Captured,
    Refused(Refusal),
    Cancelled,
    /// Every held key is up; the hook may be released.
    Disarmed,
}

impl Outcome {
    /// Whether the hook should `PostMessage` for this outcome. `Ignored` is
    /// the whole reason this exists: auto-repeat would otherwise wake the
    /// UI thread once per repeat.
    pub fn post(self) -> bool {
        !matches!(self, Outcome::Ignored)
    }
}

/// The four modifiers a `Combo` can carry, as capture currently sees them.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Mods {
    ctrl: bool,
    super_: bool,
    alt: bool,
    shift: bool,
}

impl Mods {
    fn any(self) -> bool {
        self.ctrl || self.super_ || self.alt || self.shift
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Modifier {
    Ctrl,
    Super,
    Alt,
    Shift,
}

/// The modifier a vk stands for, left and right collapsed onto one.
///
/// Normalising here rather than at the point of use is what keeps the held
/// set raw: `VK_LSHIFT` and `VK_RSHIFT` occupy separate slots, so releasing
/// one while the other is still down does not drop `shift`. Collapsing on
/// insert instead would have made that pair a single slot and lost the
/// second release.
fn modifier_of(vk: u32) -> Option<Modifier> {
    match vk {
        VK_CONTROL | VK_LCONTROL | VK_RCONTROL => Some(Modifier::Ctrl),
        VK_LWIN | VK_RWIN => Some(Modifier::Super),
        VK_MENU | VK_LMENU | VK_RMENU => Some(Modifier::Alt),
        VK_SHIFT | VK_LSHIFT | VK_RSHIFT => Some(Modifier::Shift),
        _ => None,
    }
}

/// Chords capture must refuse rather than record.
///
/// `Win+L` is here because of a measurement, not a document:
/// `docs/superpowers/measurements/2026-08-11-landing-1-a14.md` §48 recorded
/// `SEEN=True SWALLOWED=True ACTED=True` -- the hook is delivered the
/// key-down on the normal desktop, and returning 1 does not stop the lock.
/// So nothing prevents capture from writing `super+l` into the TOML except
/// this list, and the user would be left with a binding that can never fire.
///
/// The three lock keys are refused as main keys for a different reason,
/// unchanged from spec F.5: the lock state toggles before the hook runs, so
/// swallowing the key cannot undo the light.
fn is_reserved(vk: u32, mods: Mods) -> bool {
    matches!(vk, VK_CAPITAL | VK_NUMLOCK | VK_SCROLL) || (mods.super_ && vk == VK_L)
}

/// Everything one recording session needs to remember.
///
/// The held set is a fixed-size array rather than a `Vec` or a `HashSet`
/// because `step` runs in the hook callback; see the module doc.
#[derive(Debug)]
pub struct CaptureState {
    held: [u32; HELD_MAX],
    held_len: usize,
    captured: Option<Combo>,
    refused_keycap: Option<&'static KeyDef>,
    draining: bool,
}

impl CaptureState {
    /// A fresh session: hook armed, nothing held, nothing recorded.
    pub fn armed() -> Self {
        CaptureState {
            held: [0; HELD_MAX],
            held_len: 0,
            captured: None,
            refused_keycap: None,
            draining: false,
        }
    }

    /// The chord this session recorded, once `step` has returned
    /// `Outcome::Captured`.
    pub fn captured(&self) -> Option<Combo> {
        self.captured
    }

    /// The key behind the most recent `Outcome::Refused`, when the 81-key
    /// table can name it -- `None` for `Refusal::UnknownKey`, which is the
    /// refusal that exists precisely because it cannot.
    ///
    /// Read when a `Refused` arrives, not otherwise: it deliberately keeps
    /// the last refusal rather than clearing itself on the next keystroke,
    /// so a later `Partial` leaves it stale. A commit does clear it, so
    /// `captured()` and this can never both look meaningful at once.
    pub fn refused_keycap(&self) -> Option<&'static KeyDef> {
        self.refused_keycap
    }

    /// Whether the hook must keep swallowing. True from a commit or a cancel
    /// until the last physically-held key comes up.
    pub fn draining(&self) -> bool {
        self.draining
    }

    /// The modifiers held so far, canonically ordered, for the live field.
    ///
    /// **UI-thread only.** It allocates, so it must never be called from the
    /// hook callback -- that is the one rule the rest of this module is
    /// built around.
    pub fn partial(&self) -> Option<String> {
        let m = self.mods();
        if !m.any() {
            return None;
        }
        let mut s = String::with_capacity(24);
        if m.ctrl {
            s.push_str("ctrl+");
        }
        if m.super_ {
            s.push_str("super+");
        }
        if m.alt {
            s.push_str("alt+");
        }
        if m.shift {
            s.push_str("shift+");
        }
        s.push_str("...");
        Some(s)
    }

    /// The four flags, derived from the held set on every read rather than
    /// stored alongside it. One source of truth: a cached copy would be a
    /// second one to keep in sync, and the scan is twelve comparisons.
    fn mods(&self) -> Mods {
        let mut m = Mods::default();
        for &vk in &self.held[..self.held_len] {
            match modifier_of(vk) {
                Some(Modifier::Ctrl) => m.ctrl = true,
                Some(Modifier::Super) => m.super_ = true,
                Some(Modifier::Alt) => m.alt = true,
                Some(Modifier::Shift) => m.shift = true,
                None => {}
            }
        }
        m
    }

    fn is_held(&self, vk: u32) -> bool {
        self.held[..self.held_len].contains(&vk)
    }

    fn hold(&mut self, vk: u32) {
        if self.held_len < HELD_MAX {
            self.held[self.held_len] = vk;
            self.held_len += 1;
        }
    }

    fn release(&mut self, vk: u32) {
        if let Some(i) = self.held[..self.held_len].iter().position(|&h| h == vk) {
            self.held_len -= 1;
            self.held[i] = self.held[self.held_len];
        }
    }
}

/// Decide what to do with one key event while recording. Pure apart from
/// `st`, and -- see the module doc -- free of allocation and formatting.
///
/// The order of the checks is load-bearing, so it is spelled out rather than
/// left to the reader: injected, draining, up, auto-repeat, modifier, bare
/// Esc, main key. Two orderings in particular were measured rather than
/// argued, by deleting each and watching one named test go red:
///
/// - The already-held test must come before the modifier arm. Without it
///   auto-repeat of a held Ctrl both fills a second slot and answers
///   `Partial` instead of `Ignored`, which wakes the UI thread once per
///   repeat -- the exact cost `Outcome::post` exists to avoid.
/// - `Refusal::Reserved` must be decided before `Refusal::UnknownKey`.
///   `VK_CAPITAL` has no name in the 81-key table, so with the reserved arm
///   gone a lock key comes back `Refused(UnknownKey)` -- true, but it sends
///   the user to the Key list for a key that is refused no matter how it is
///   spelled.
pub fn step(ev: KeyEvent, st: &mut CaptureState) -> Outcome {
    // Our own injected strokes carry beckon's `dwExtraInfo` marker. The Caps
    // feature injects the CONFIGURED chord, so capturing one would record
    // the alias instead of the key the user actually pressed.
    if ev.injected_by_us {
        return Outcome::Ignored;
    }

    // Committed or cancelled: keep swallowing until the keyboard is quiet.
    // This is what makes `alt+tab` safe to record -- the alt-down was
    // swallowed, so the alt-up must be too, or the system sees a bare
    // Alt-up and switches windows out from under the settings window.
    if st.draining {
        if ev.edge == Edge::Up {
            st.release(ev.vk);
            if st.held_len == 0 {
                st.draining = false;
                return Outcome::Disarmed;
            }
        }
        return Outcome::Ignored;
    }

    if ev.edge == Edge::Up {
        st.release(ev.vk);
        return if modifier_of(ev.vk).is_some() {
            // Releasing every modifier returns to Armed and is not an error:
            // a double-tap of Ctrl shows `ctrl+...` and then the prompt.
            Outcome::Partial
        } else {
            Outcome::Ignored
        };
    }

    // `KBDLLHOOKSTRUCT` carries no repeat count, so the held set is the only
    // filter there is for auto-repeat.
    if st.is_held(ev.vk) {
        return Outcome::Ignored;
    }

    if modifier_of(ev.vk).is_some() {
        st.hold(ev.vk);
        return Outcome::Partial;
    }

    let mods = st.mods();

    // Bare Esc stops recording. With a modifier it is an ordinary main key:
    // `ctrl+escape` is bindable, and only modifiers are ever in the held set
    // before the main key, so "bare" and "nothing held" are the same test.
    if ev.vk == VK_ESCAPE && !mods.any() {
        st.hold(ev.vk);
        st.draining = true;
        return Outcome::Cancelled;
    }

    let keycap = lookup_win_vk(ev.vk);

    if !mods.any() {
        // Remembered so the window can show what beckon heard and then
        // explain why it is not acceptable. Refusing silently reads as a
        // broken keyboard.
        st.refused_keycap = keycap;
        return Outcome::Refused(Refusal::NoModifier);
    }
    if is_reserved(ev.vk, mods) {
        st.refused_keycap = keycap;
        return Outcome::Refused(Refusal::Reserved);
    }
    let Some(key) = keycap else {
        st.refused_keycap = None;
        return Outcome::Refused(Refusal::UnknownKey);
    };

    st.refused_keycap = None;
    st.captured = Some(Combo {
        ctrl: mods.ctrl,
        super_: mods.super_,
        alt: mods.alt,
        shift: mods.shift,
        key,
    });
    // The main key joins the held set so the drain outlives it. Release
    // order is not press order: let go of Ctrl before T and, without this,
    // the set would empty early, the hook would disarm, and the T-up would
    // reach the application with its down already swallowed.
    st.hold(ev.vk);
    st.draining = true;
    Outcome::Captured
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::caps::{Edge, KeyEvent};

    const VK_CONTROL: u32 = 0x11;
    const VK_LWIN: u32 = 0x5B;
    const VK_MENU: u32 = 0x12;
    const VK_SHIFT: u32 = 0x10;
    const VK_A: u32 = 0x41;
    const VK_T: u32 = 0x54;
    const VK_L: u32 = 0x4C;
    const VK_ESCAPE: u32 = 0x1B;
    const VK_CAPITAL: u32 = 0x14;
    const VK_NUMPAD0: u32 = 0x60;

    fn ev(vk: u32, edge: Edge) -> KeyEvent {
        KeyEvent {
            vk,
            edge,
            injected_by_us: false,
            time_ms: 0,
        }
    }

    fn down(st: &mut CaptureState, vk: u32) -> Outcome {
        step(ev(vk, Edge::Down), st)
    }
    fn up(st: &mut CaptureState, vk: u32) -> Outcome {
        step(ev(vk, Edge::Up), st)
    }

    #[test]
    fn a_modifier_then_a_key_captures_the_chord() {
        let mut st = CaptureState::armed();
        assert_eq!(down(&mut st, VK_CONTROL), Outcome::Partial);
        assert_eq!(down(&mut st, VK_LWIN), Outcome::Partial);
        assert_eq!(down(&mut st, VK_T), Outcome::Captured);
        let c = st.captured().expect("a chord");
        assert_eq!(c.canonical(), "ctrl+super+t");
    }

    /// Canonical order is the TOML's order, not press order.
    #[test]
    fn the_captured_combo_is_canonical_not_press_order() {
        let mut st = CaptureState::armed();
        down(&mut st, VK_SHIFT);
        down(&mut st, VK_MENU);
        down(&mut st, VK_CONTROL);
        down(&mut st, VK_T);
        assert_eq!(st.captured().unwrap().canonical(), "ctrl+alt+shift+t");
    }

    #[test]
    fn a_bare_key_is_refused_but_still_shown() {
        let mut st = CaptureState::armed();
        assert_eq!(down(&mut st, VK_A), Outcome::Refused(Refusal::NoModifier));
        assert_eq!(
            st.refused_keycap().map(|k| k.name.as_str()),
            Some("a"),
            "showing what beckon heard and then explaining why it is not \
             acceptable is the point -- silently refusing reads as a broken \
             keyboard"
        );
        assert!(st.captured().is_none());
    }

    #[test]
    fn a_key_with_no_name_is_refused() {
        let mut st = CaptureState::armed();
        down(&mut st, VK_CONTROL);
        assert_eq!(
            down(&mut st, VK_NUMPAD0),
            Outcome::Refused(Refusal::UnknownKey)
        );
        assert!(st.captured().is_none());
    }

    /// Measured on a14 2026-08-12: the hook DOES see Win+L -- spec F.5 said
    /// it saw nothing -- but returning 1 does not stop the lock. So capture
    /// would happily record a chord that can never fire, and has to refuse
    /// it explicitly.
    #[test]
    fn a_reserved_chord_is_refused_rather_than_recorded() {
        let mut st = CaptureState::armed();
        down(&mut st, VK_LWIN);
        assert_eq!(down(&mut st, VK_L), Outcome::Refused(Refusal::Reserved));
        assert!(st.captured().is_none());
    }

    /// The lock keys toggle before the hook runs, so swallowing cannot undo
    /// the light. F.5 excludes them from the capturable set.
    #[test]
    fn a_lock_key_is_refused_as_a_main_key() {
        let mut st = CaptureState::armed();
        down(&mut st, VK_CONTROL);
        assert_eq!(
            down(&mut st, VK_CAPITAL),
            Outcome::Refused(Refusal::Reserved)
        );
    }

    #[test]
    fn bare_escape_cancels() {
        let mut st = CaptureState::armed();
        assert_eq!(down(&mut st, VK_ESCAPE), Outcome::Cancelled);
        assert!(st.captured().is_none());
    }

    /// Esc WITH a modifier is a bindable chord, not a cancel.
    #[test]
    fn escape_with_a_modifier_is_a_chord() {
        let mut st = CaptureState::armed();
        down(&mut st, VK_CONTROL);
        assert_eq!(down(&mut st, VK_ESCAPE), Outcome::Captured);
        assert_eq!(st.captured().unwrap().canonical(), "ctrl+escape");
    }

    #[test]
    fn releasing_every_modifier_returns_to_armed_and_is_not_an_error() {
        let mut st = CaptureState::armed();
        down(&mut st, VK_CONTROL);
        assert_eq!(up(&mut st, VK_CONTROL), Outcome::Partial);
        assert_eq!(
            st.partial(),
            None,
            "nothing is held, so there is no partial combo"
        );
        assert!(st.captured().is_none());
    }

    #[test]
    fn the_partial_combo_reads_in_canonical_order() {
        let mut st = CaptureState::armed();
        down(&mut st, VK_MENU);
        down(&mut st, VK_CONTROL);
        assert_eq!(st.partial(), Some("ctrl+alt+...".to_string()));
    }

    /// KBDLLHOOKSTRUCT carries no repeat count, so the held set is the
    /// filter: a key-down for a vk already held changes nothing.
    #[test]
    fn auto_repeat_of_a_held_modifier_changes_nothing() {
        let mut st = CaptureState::armed();
        down(&mut st, VK_CONTROL);
        let before = st.partial();
        assert_eq!(down(&mut st, VK_CONTROL), Outcome::Ignored);
        assert_eq!(st.partial(), before);
    }

    /// After a commit the hook keeps swallowing until every held key is up.
    /// That is what makes Alt+Tab safe: the alt-down was swallowed, so the
    /// alt-up is too, and the system never sees a bare Alt-up.
    #[test]
    fn draining_holds_the_hook_until_the_last_key_is_released() {
        let mut st = CaptureState::armed();
        down(&mut st, VK_CONTROL);
        down(&mut st, VK_MENU);
        assert_eq!(down(&mut st, VK_T), Outcome::Captured);
        assert!(st.draining());
        assert_eq!(up(&mut st, VK_T), Outcome::Ignored);
        assert!(st.draining(), "ctrl and alt are still down");
        up(&mut st, VK_CONTROL);
        assert_eq!(up(&mut st, VK_MENU), Outcome::Disarmed);
        assert!(!st.draining());
    }

    /// **Rewritten from the brief, which contradicted itself here.** As
    /// written this test pressed Ctrl before Esc -- byte for byte the same
    /// sequence as `escape_with_a_modifier_is_a_chord`, one expecting
    /// `Cancelled` and the other `Captured`. No state machine satisfies
    /// both; measured, the second assertion failed `left: Captured, right:
    /// Cancelled`.
    ///
    /// The prose decides it, and three sources agree: step 6 of the task
    /// brief says "bare `VK_ESCAPE` **with no modifier held**", spec F.3
    /// says "Bare `VK_ESCAPE` -> Cancelled" and commits "on the first
    /// non-modifier key-down while at least one modifier is held", and the
    /// sibling test's own doc comment says Esc with a modifier is a chord.
    ///
    /// So a cancel's drain is always exactly one key long -- only modifiers
    /// reach the held set before the main key, so "bare Esc" and "nothing
    /// held" are the same condition -- and the multi-key drain this test
    /// reached for is unreachable after a cancel by construction. It is
    /// covered anyway, on the commit path, by
    /// `draining_holds_the_hook_until_the_last_key_is_released`. What is
    /// left for this test, and what it still pins, is that a cancel arms the
    /// drain at all rather than disarming on the spot.
    #[test]
    fn a_cancel_drains_too() {
        let mut st = CaptureState::armed();
        assert_eq!(down(&mut st, VK_ESCAPE), Outcome::Cancelled);
        assert!(st.draining());
        assert_eq!(up(&mut st, VK_ESCAPE), Outcome::Disarmed);
    }

    /// Left and right modifiers are normalised -- the TOML cannot express
    /// the distinction.
    #[test]
    fn left_and_right_modifiers_are_the_same_modifier() {
        const VK_RCONTROL: u32 = 0xA3;
        let mut st = CaptureState::armed();
        down(&mut st, VK_RCONTROL);
        down(&mut st, VK_T);
        assert_eq!(st.captured().unwrap().canonical(), "ctrl+t");
    }

    /// Our own injected strokes must never be captured -- the Caps feature
    /// injects the configured chord, and capturing it would record the
    /// alias instead of the key the user pressed.
    #[test]
    fn our_own_injected_keys_are_ignored() {
        let mut st = CaptureState::armed();
        let injected = KeyEvent {
            vk: VK_CONTROL,
            edge: Edge::Down,
            injected_by_us: true,
            time_ms: 0,
        };
        assert_eq!(step(injected, &mut st), Outcome::Ignored);
        assert_eq!(st.partial(), None);
    }

    /// Every Outcome the UI must react to has to be posted; the ones it
    /// need not see must not wake it.
    #[test]
    fn only_outcomes_the_window_must_see_are_posted() {
        assert!(!Outcome::Ignored.post());
        assert!(Outcome::Partial.post());
        assert!(Outcome::Captured.post());
        assert!(Outcome::Cancelled.post());
        assert!(Outcome::Disarmed.post());
        assert!(Outcome::Refused(Refusal::NoModifier).post());
    }
}
