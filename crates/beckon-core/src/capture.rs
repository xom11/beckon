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
const VK_DELETE: u32 = 0x2E;
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
    /// A chord beckon cannot bind. Two unrelated reasons wear this one
    /// variant -- see `is_reserved`: `Win+L` and `Ctrl+Alt+Del` are Windows'
    /// own (measured on a14, the hook sees `Win+L` but cannot suppress it,
    /// so recording it would hand the user a binding that can never fire),
    /// while the three lock keys are beckon's own limit, their light having
    /// already toggled by the time the hook runs.
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
    /// Let it reach the system: we never swallowed its down, so swallowing
    /// its up would strand it.
    ///
    /// **This is the ONE outcome for which the hook must not return 1.**
    /// Recording starts on a click as readily as on a keystroke, so a key
    /// can already be physically down when the field arms -- hold Ctrl,
    /// click `Record` with the mouse, and the Ctrl-down was never seen. If
    /// the matching up is swallowed anyway, the system believes Ctrl is
    /// held with no up ever coming, and nothing short of killing beckon
    /// gets it back. That is the stuck-modifier failure spec D.1 exists to
    /// prevent, so this outcome is worth its own variant rather than being
    /// folded into `Ignored`.
    ///
    /// It also covers the up of a key whose down was refused, since refused
    /// keys never enter the held set -- see `step`'s doc. A stray key-up
    /// for a character key latches nothing; a swallowed one for a modifier
    /// is the failure above, and only the held set can tell them apart.
    PassThrough,
}

impl Outcome {
    /// Whether the hook should `PostMessage` for this outcome. `Ignored` is
    /// the whole reason this exists: auto-repeat would otherwise wake the
    /// UI thread once per repeat. `PassThrough` is here for the same reason
    /// -- it says something about the hook's return value, nothing about
    /// what the window shows.
    pub fn post(self) -> bool {
        !matches!(self, Outcome::Ignored | Outcome::PassThrough)
    }

    /// The integer this outcome travels in, as a `WM_CAPTURE` `WPARAM`.
    ///
    /// A number rather than a boxed `Outcome`, because the hook callback
    /// posts it and the hook callback may not allocate -- see the module
    /// doc. The window rebuilds everything else from `CaptureState` when the
    /// message arrives.
    ///
    /// Pure, and here rather than in `caps_hook.rs`, so the round trip is
    /// tested on all three CI jobs: `caps_hook.rs` compiles on one job in
    /// three, and a `code`/`from_code` pair that disagreed would be a window
    /// silently reacting to the wrong outcome. The match is exhaustive on
    /// purpose -- a new variant fails the build here rather than falling
    /// through to a default and arriving as something else.
    pub fn code(self) -> usize {
        match self {
            Outcome::Ignored => 0,
            Outcome::Partial => 1,
            Outcome::Captured => 2,
            Outcome::Cancelled => 3,
            Outcome::Disarmed => 4,
            Outcome::PassThrough => 5,
            Outcome::Refused(Refusal::NoModifier) => 6,
            Outcome::Refused(Refusal::UnknownKey) => 7,
            Outcome::Refused(Refusal::Reserved) => 8,
        }
    }

    /// The inverse of `code`. `None` for anything this version did not
    /// write, which a stray `WM_CAPTURE` from outside beckon could be: the
    /// message id is `WM_APP`-relative and therefore only private by
    /// convention.
    pub fn from_code(code: usize) -> Option<Outcome> {
        Some(match code {
            0 => Outcome::Ignored,
            1 => Outcome::Partial,
            2 => Outcome::Captured,
            3 => Outcome::Cancelled,
            4 => Outcome::Disarmed,
            5 => Outcome::PassThrough,
            6 => Outcome::Refused(Refusal::NoModifier),
            7 => Outcome::Refused(Refusal::UnknownKey),
            8 => Outcome::Refused(Refusal::Reserved),
            _ => return None,
        })
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
///
/// `Ctrl+Alt+Del` is here on scope, not on mechanism. Spec F.5 records it as
/// **unverified** and says to treat it as refused until it is measured; no
/// explanation of what the hook would see is offered here, and none should
/// be added without a measurement, because the story this family used to
/// share was disproved for `Win+L` (measurements §48). `delete` is in the
/// 81-key table, so without this arm the chord is recordable.
fn is_reserved(vk: u32, mods: Mods) -> bool {
    matches!(vk, VK_CAPITAL | VK_NUMLOCK | VK_SCROLL)
        || (mods.super_ && vk == VK_L)
        || (mods.ctrl && mods.alt && vk == VK_DELETE)
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
    refused_vk: Option<u32>,
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
            refused_vk: None,
            draining: false,
        }
    }

    /// The chord this session recorded, once `step` has returned
    /// `Outcome::Captured`.
    pub fn captured(&self) -> Option<Combo> {
        self.captured
    }

    /// The key behind the most recent `Outcome::Refused`, when the 81-key
    /// table can name it.
    ///
    /// `None` means only that the table could not name the key. It does NOT
    /// identify which refusal happened: it is always `None` for
    /// `Refusal::UnknownKey`, but a bare unnameable key -- numpad0, or Caps
    /// Lock without a modifier -- is refused as `NoModifier` and leaves it
    /// `None` too. Read the `Refusal` to know which.
    ///
    /// Read when a `Refused` arrives, not otherwise: it deliberately keeps
    /// the last refusal rather than clearing itself on the next keystroke,
    /// so a later `Partial` leaves it stale. A commit does clear it, so
    /// `captured()` and this can never both look meaningful at once.
    pub fn refused_keycap(&self) -> Option<&'static KeyDef> {
        self.refused_keycap
    }

    /// The virtual-key code behind the most recent `Outcome::Refused`.
    ///
    /// **This is what the beep is de-duplicated by, and `refused_keycap` is
    /// not a substitute.** A refused key never enters the held set (see
    /// `step`'s doc for why admitting it would be worse), so the auto-repeat
    /// filter cannot see it and a held-down bare `a` yields one
    /// `Refused(NoModifier)` per repeat, each of which posts. The window
    /// beeps only when this changes. `refused_keycap` cannot carry that: it
    /// is `None` for every key the 81-key table cannot name, so two different
    /// unnameable keys -- and every repeat of one -- would look identical.
    ///
    /// Kept and cleared exactly as `refused_keycap` is, so the pair can never
    /// describe different keystrokes.
    pub fn refused_vk(&self) -> Option<u32> {
        self.refused_vk
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

    /// Drop `vk` from the held set, reporting whether it was actually in
    /// there. The bool is what tells a key-up whose key-down we swallowed
    /// apart from one we never saw -- see `Outcome::PassThrough`.
    fn release(&mut self, vk: u32) -> bool {
        match self.held[..self.held_len].iter().position(|&h| h == vk) {
            Some(i) => {
                self.held_len -= 1;
                self.held[i] = self.held[self.held_len];
                true
            }
            None => false,
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
///
/// Two hazards the hook wiring inherits from this, neither fixable here:
///
/// - **A refused key is refused again on every auto-repeat.** Refused keys
///   never enter the held set, so the already-held filter cannot see them:
///   holding `a` down while armed yields one `Refused(NoModifier)` per
///   repeat, each of which posts. Per F.3 a refusal beeps, so the hook must
///   de-duplicate the beep **by vk** -- one beep while that key stays down
///   -- and not per outcome.
/// - **Admitting refused keys to the held set is the obvious fix and it is
///   wrong.** It is a `[u32; HELD_MAX]`, and rolled-over bare keys would
///   eat the slots: mash six unmodified keys and `hold` starts dropping,
///   so the Ctrl the user presses next is silently absent from `mods()`
///   and the chord commits without it. Losing a modifier is far worse than
///   a repeated refusal, so the repeat is handled at the beep instead.
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
            // A key we never swallowed the down for is not ours to swallow
            // the up for either; see `Outcome::PassThrough`.
            if !st.release(ev.vk) {
                return Outcome::PassThrough;
            }
            if st.held_len == 0 {
                st.draining = false;
                return Outcome::Disarmed;
            }
        }
        return Outcome::Ignored;
    }

    if ev.edge == Edge::Up {
        if !st.release(ev.vk) {
            return Outcome::PassThrough;
        }
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
        st.refused_vk = Some(ev.vk);
        return Outcome::Refused(Refusal::NoModifier);
    }
    if is_reserved(ev.vk, mods) {
        st.refused_keycap = keycap;
        st.refused_vk = Some(ev.vk);
        return Outcome::Refused(Refusal::Reserved);
    }
    let Some(key) = keycap else {
        st.refused_keycap = None;
        // Set even though the keycap is not, because this is exactly the
        // refusal the keycap cannot identify -- and the one whose auto-repeat
        // the window has to de-duplicate.
        st.refused_vk = Some(ev.vk);
        return Outcome::Refused(Refusal::UnknownKey);
    };

    st.refused_keycap = None;
    st.refused_vk = None;
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

// ---------------------------------------------------------------------------
// What the window says (spec F.3)
// ---------------------------------------------------------------------------

/// Armed: `Record` pressed, nothing typed yet.
///
/// A const rather than an arm of `hint`, because arming is not an outcome --
/// no key has been pressed when the window first shows it.
pub const HINT_ARMED: &str = "Press the shortcut. Esc stops recording.";

/// Shown *instead of* arming when `SetWindowsHookExW` fails.
///
/// Spec F.3: do not silently fall back to message-queue capture. That path
/// cannot see the Windows key, so it fails on precisely the chords beckon
/// recommends -- and it would fail by recording the wrong thing rather than
/// by refusing, which is the worse of the two.
pub const HINT_UNAVAILABLE: &str =
    "Cannot record here. Use the modifier boxes and the Key list instead.";

const HINT_UNKNOWN_KEY: &str = "beckon has no name for that key. Pick one from the Key list.";

/// Not from spec F.3, which gives no wording for `Refusal::Reserved` -- only
/// F.5's instruction that a refused chord arrives "with the help line saying
/// so". The one invented string in this module, so what it may claim is
/// bounded by what is true of all five chords `is_reserved` covers.
///
/// **It names no mechanism, because the five do not share one.** An earlier
/// draft read `Windows reserves that shortcut.`, which is true of `Win+L`
/// and `Ctrl+Alt+Del` and false of the three lock keys: those are refused
/// because the lock state toggles before the hook runs, so swallowing the
/// key cannot undo the light. That is beckon's limit, not a Windows
/// reservation, and stating it the other way is a confidently-worded wrong
/// sentence shown to the user -- worse, in this project, than saying less.
///
/// It also drops that draft's `Press Record and try again.` tail. A refusal
/// leaves the field Armed (see `step`), so there is nothing to press; the
/// mandated bare-key line carries the same tail and is copied verbatim
/// anyway, because F.3 fixes its wording and does not fix this one.
const HINT_RESERVED: &str = "beckon cannot bind that shortcut. Try a different one.";

/// The hint line for one `step` outcome. `None` means the line is idle --
/// recording has ended (or never started) and there is nothing to say.
///
/// **UI-thread only**, like every other formatting method here: it
/// allocates. The hook callback gets an `Outcome` and posts; the string is
/// built when `WM_CAPTURE` arrives.
///
/// `refused_keycap` is `CaptureState::refused_keycap()` -- read it straight
/// off the state that produced `outcome`, and read it only then. It keeps
/// the *last* refusal rather than clearing itself, so pairing a stale one
/// with a fresh outcome names the wrong key.
pub fn hint(outcome: Outcome, refused_keycap: Option<&KeyDef>) -> Option<String> {
    match outcome {
        // Holding modifiers is still the prompt, not an error: releasing
        // them all returns to Armed and F.3 calls that "not an error".
        Outcome::Partial => Some(HINT_ARMED.to_string()),
        Outcome::Refused(Refusal::NoModifier) => Some(match refused_keycap {
            Some(k) => format!(
                "{} alone is not a shortcut - hold Ctrl, Win or Alt as well. \
                 Press Record and try again.",
                keycap(k)
            ),
            // A bare key the 81-key table cannot name -- numpad0, or Caps
            // Lock. There is no name to put in the sentence above, and the
            // honest thing to say is the one that is already true of it:
            // beckon could not bind that key with any modifier either.
            None => HINT_UNKNOWN_KEY.to_string(),
        }),
        Outcome::Refused(Refusal::UnknownKey) => Some(HINT_UNKNOWN_KEY.to_string()),
        Outcome::Refused(Refusal::Reserved) => Some(HINT_RESERVED.to_string()),
        Outcome::Ignored
        | Outcome::PassThrough
        | Outcome::Captured
        | Outcome::Cancelled
        | Outcome::Disarmed => None,
    }
}

/// A key name as the sentence above wants to read it: `a` -> `A`, `f1` ->
/// `F1`, matching the `Ctrl, Win or Alt` beside it.
///
/// Only the first character, and only ASCII -- every name in the 81-key
/// table is ASCII, and upper-casing the whole of `bracketleft` shouts. This
/// is not keycap rendering (`settings_window.rs` says the window does none);
/// it is the minimum that makes `A alone is not a shortcut` read as English
/// rather than as an article.
fn keycap(k: &KeyDef) -> String {
    let mut c = k.name.chars();
    match c.next() {
        Some(first) => first.to_ascii_uppercase().to_string() + c.as_str(),
        None => String::new(),
    }
}

// ---------------------------------------------------------------------------
// Who holds the hook (spec F.2)
// ---------------------------------------------------------------------------

/// Why the one `WH_KEYBOARD_LL` hook is installed.
///
/// **There is exactly one hook, and these are its two reasons.** A second
/// hook is not an option: `WH_KEYBOARD_LL` hooks chain, so a capture hook
/// running beside the Caps one records the alias `Caps+T` injects instead of
/// the key pressed, and swallows the Caps-up that `CapsState.held` is
/// waiting for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HookReason {
    /// `keyboard.caps` is on and not paused. Resident for the session.
    Caps,
    /// A shortcut field is recording. Transient, seconds at a time.
    Capture,
}

/// Which reasons currently want the hook installed.
///
/// Two bools, and pure, so all three CI jobs test it -- `caps_hook.rs`
/// compiles on one job in three, and a lifetime bug there would be invisible
/// to the other two. Same argument `caps.rs` makes for `decide`.
///
/// `add` and `remove` answer one question: **is an OS call needed now?**
/// Install on the first reason, unhook on the last, nothing in between. That
/// is what keeps a capture ending from resetting `CapsState`, and a config
/// reload mid-capture from reinstalling the hook underneath the capture.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct HookOwners {
    caps: bool,
    capture: bool,
}

impl HookOwners {
    /// Nobody holds it. `const` so a `thread_local!` can use a `const` block.
    pub const fn new() -> Self {
        HookOwners {
            caps: false,
            capture: false,
        }
    }

    /// Whether the hook should be installed right now.
    pub fn wanted(&self) -> bool {
        self.caps || self.capture
    }

    /// Take a reason. Returns whether `SetWindowsHookExW` must now be called
    /// -- true only for the first reason.
    pub fn add(&mut self, reason: HookReason) -> bool {
        let before = self.wanted();
        *self.slot(reason) = true;
        !before
    }

    /// Drop a reason. Returns whether `UnhookWindowsHookEx` must now be
    /// called -- true only when the last one goes.
    ///
    /// Dropping a reason that never held it is a no-op, not an unhook: that
    /// is the whole safety property, and `caps_hook` resets `CapsState`
    /// only on a `true` from here.
    pub fn remove(&mut self, reason: HookReason) -> bool {
        let before = self.wanted();
        *self.slot(reason) = false;
        before && !self.wanted()
    }

    fn slot(&mut self, reason: HookReason) -> &mut bool {
        match reason {
            HookReason::Caps => &mut self.caps,
            HookReason::Capture => &mut self.capture,
        }
    }
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
    const VK_DELETE: u32 = 0x2E;

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

    /// Spec F.5 carries `Ctrl+Alt+Del` forward as **unverified**, and says
    /// to treat it as refused until it is measured. `delete` is in the
    /// 81-key table, so without an explicit arm this chord is recordable.
    #[test]
    fn ctrl_alt_del_is_refused() {
        let mut st = CaptureState::armed();
        down(&mut st, VK_CONTROL);
        down(&mut st, VK_MENU);
        assert_eq!(
            down(&mut st, VK_DELETE),
            Outcome::Refused(Refusal::Reserved)
        );
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

    /// The beep is de-duplicated by vk, and this is what makes that
    /// possible. Three properties in one test because they are one rule:
    /// every refusal names its key, including the two that leave
    /// `refused_keycap` empty, and a commit clears it.
    #[test]
    fn every_refusal_names_the_key_it_refused() {
        // Nameable, no modifier.
        let mut bare = CaptureState::armed();
        assert_eq!(down(&mut bare, VK_A), Outcome::Refused(Refusal::NoModifier));
        assert_eq!(bare.refused_vk(), Some(VK_A));

        // Unnameable: `refused_keycap` is None here, so it could not stand in.
        let mut unknown = CaptureState::armed();
        down(&mut unknown, VK_CONTROL);
        assert_eq!(
            down(&mut unknown, VK_NUMPAD0),
            Outcome::Refused(Refusal::UnknownKey)
        );
        assert_eq!(unknown.refused_keycap().map(|k| k.name.as_str()), None);
        assert_eq!(
            unknown.refused_vk(),
            Some(VK_NUMPAD0),
            "the one refusal the keycap cannot identify must still be \
             identifiable, or its auto-repeat beeps once per repeat"
        );

        // Reserved.
        let mut reserved = CaptureState::armed();
        down(&mut reserved, VK_LWIN);
        assert_eq!(
            down(&mut reserved, VK_L),
            Outcome::Refused(Refusal::Reserved)
        );
        assert_eq!(reserved.refused_vk(), Some(VK_L));

        // A commit clears it, so `captured()` and this can never both look
        // meaningful at once -- the same rule `refused_keycap` follows.
        let mut ok = CaptureState::armed();
        down(&mut ok, VK_A); // refuse first, so there is something to clear
        down(&mut ok, VK_CONTROL);
        assert_eq!(down(&mut ok, VK_T), Outcome::Captured);
        assert_eq!(ok.refused_vk(), None);
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

    /// A key-up whose key-down we never swallowed must reach the system.
    ///
    /// The user is physically holding Ctrl, starts recording by CLICKING
    /// `Record` with the mouse -- so the Ctrl-down was never seen -- and
    /// presses Alt+T. That commits and starts draining. Swallowing the
    /// Ctrl-up that follows leaves the system believing Ctrl is held with
    /// no up ever coming: every click becomes Ctrl+click and the user
    /// cannot recover without killing beckon.
    #[test]
    fn a_key_up_we_never_swallowed_passes_through_while_draining() {
        let mut st = CaptureState::armed();
        down(&mut st, VK_MENU);
        assert_eq!(down(&mut st, VK_T), Outcome::Captured);
        assert!(st.draining());
        assert_eq!(
            up(&mut st, VK_CONTROL),
            Outcome::PassThrough,
            "its down was never swallowed, so its up must not be either"
        );
        assert!(!Outcome::PassThrough.post(), "the window has nothing to do");
        assert!(st.draining(), "alt and t are still down");
        up(&mut st, VK_T);
        assert_eq!(up(&mut st, VK_MENU), Outcome::Disarmed);
    }

    /// The same hazard before any commit: armed, nothing held, and an up
    /// arrives for a key held since before recording started.
    #[test]
    fn a_key_up_we_never_swallowed_passes_through_while_armed() {
        let mut st = CaptureState::armed();
        down(&mut st, VK_MENU);
        assert_eq!(up(&mut st, VK_CONTROL), Outcome::PassThrough);
        assert_eq!(
            st.partial(),
            Some("alt+...".to_string()),
            "the stray up changed nothing we are holding"
        );
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
    ///
    /// All four are pressed on each side, because a single sided VK proves
    /// only itself: with `VK_RSHIFT`, `VK_RMENU` or `VK_RWIN` missing from
    /// `modifier_of` an `VK_RCONTROL`-only test still passes, while the
    /// missing key is treated as a main key and the chord commits early.
    #[test]
    fn left_and_right_modifiers_are_the_same_modifier() {
        let mut right = CaptureState::armed();
        down(&mut right, VK_RCONTROL);
        down(&mut right, VK_RWIN);
        down(&mut right, VK_RMENU);
        down(&mut right, VK_RSHIFT);
        down(&mut right, VK_T);
        assert_eq!(
            right.captured().expect("a chord").canonical(),
            "ctrl+super+alt+shift+t"
        );

        let mut left = CaptureState::armed();
        down(&mut left, VK_LCONTROL);
        down(&mut left, VK_LWIN);
        down(&mut left, VK_LMENU);
        down(&mut left, VK_LSHIFT);
        down(&mut left, VK_T);
        assert_eq!(
            left.captured().expect("a chord").canonical(),
            "ctrl+super+alt+shift+t"
        );
    }

    /// The reason `modifier_of` normalises on READ rather than at insert,
    /// stated as behaviour: two shift keys occupy two slots, so letting one
    /// go while the other is still physically down must not drop `shift`.
    /// Collapsing both onto one slot at insert time loses the second
    /// release and the field would go blank under the user's hands.
    #[test]
    fn releasing_one_of_two_shifts_keeps_shift_held() {
        let mut st = CaptureState::armed();
        down(&mut st, VK_LSHIFT);
        down(&mut st, VK_RSHIFT);
        assert_eq!(up(&mut st, VK_LSHIFT), Outcome::Partial);
        assert_eq!(
            st.partial(),
            Some("shift+...".to_string()),
            "the right shift is still down"
        );
        assert_eq!(up(&mut st, VK_RSHIFT), Outcome::Partial);
        assert_eq!(st.partial(), None, "now both are up");
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

    // -----------------------------------------------------------------
    // The hook refcount (spec F.2)
    // -----------------------------------------------------------------

    #[test]
    fn the_hook_lives_while_either_reason_holds_it() {
        let mut o = HookOwners::default();
        assert!(!o.wanted());
        assert!(o.add(HookReason::Caps)); // true = the OS call is needed
        assert!(o.wanted());
        assert!(
            !o.add(HookReason::Capture),
            "already installed; no second SetWindowsHookEx"
        );
        assert!(!o.remove(HookReason::Capture), "Caps still wants it");
        assert!(o.wanted());
        assert!(o.remove(HookReason::Caps)); // true = now unhook
        assert!(!o.wanted());
    }

    /// The reason the refcount exists at all: capture ending must not reset
    /// the Caps state machine, and a config reload during capture must not
    /// reinstall the hook underneath it.
    #[test]
    fn dropping_capture_while_caps_holds_does_not_ask_for_an_unhook() {
        let mut o = HookOwners::default();
        o.add(HookReason::Caps);
        o.add(HookReason::Capture);
        assert!(!o.remove(HookReason::Capture));
        assert!(o.wanted());
    }

    #[test]
    fn removing_a_reason_that_never_held_it_changes_nothing() {
        let mut o = HookOwners::default();
        o.add(HookReason::Caps);
        assert!(!o.remove(HookReason::Capture));
        assert!(o.wanted());

        // The case that separates `remove`'s `before && !self.wanted()` from
        // a plain `!self.wanted()`: everything above passes under both
        // spellings. On an EMPTY `HookOwners` the plain one returns true and
        // asks for an `UnhookWindowsHookEx` against a hook that was never
        // installed -- which, in `uninstall_for`, would also reset
        // `CapsState` mid-stream.
        let mut empty = HookOwners::new();
        assert!(
            !empty.remove(HookReason::Caps),
            "nobody held it, so there is nothing to unhook"
        );
        assert!(!empty.wanted());
    }

    // -----------------------------------------------------------------
    // The hint strings (spec F.3, verbatim)
    // -----------------------------------------------------------------

    /// Armed. `Record` has been pressed and nothing typed yet.
    #[test]
    fn the_armed_hint_is_verbatim() {
        assert_eq!(HINT_ARMED, "Press the shortcut. Esc stops recording.");
        let mut st = CaptureState::armed();
        down(&mut st, VK_CONTROL);
        assert_eq!(
            hint(Outcome::Partial, st.refused_keycap()).as_deref(),
            Some("Press the shortcut. Esc stops recording."),
            "holding a modifier is still the prompt, not an error"
        );
    }

    /// The one hint carrying a key name. Built here, on the UI thread --
    /// never in the hook callback, which may not allocate or format.
    #[test]
    fn the_bare_key_hint_names_the_key_and_is_verbatim() {
        let mut st = CaptureState::armed();
        let out = down(&mut st, VK_A);
        assert_eq!(out, Outcome::Refused(Refusal::NoModifier));
        assert_eq!(
            hint(out, st.refused_keycap()).as_deref(),
            Some(
                "A alone is not a shortcut - hold Ctrl, Win or Alt as well. \
                 Press Record and try again."
            )
        );
    }

    #[test]
    fn the_unnameable_key_hint_is_verbatim() {
        let mut st = CaptureState::armed();
        down(&mut st, VK_CONTROL);
        let out = down(&mut st, VK_NUMPAD0);
        assert_eq!(out, Outcome::Refused(Refusal::UnknownKey));
        assert_eq!(
            hint(out, st.refused_keycap()).as_deref(),
            Some("beckon has no name for that key. Pick one from the Key list.")
        );
    }

    /// The other route to that same string, and the judgement call `hint`'s
    /// `NoModifier` arm makes rather than inventing a fifth sentence.
    ///
    /// A BARE unnameable key is `Refused(NoModifier)`, not
    /// `Refused(UnknownKey)` -- `step` tests `mods.any()` first -- and
    /// leaves `refused_keycap()` empty, so the named sentence has no name to
    /// carry. Falling back to the unnameable-key line is honest because it
    /// is true of this key under any modifier too. The sibling above cannot
    /// reach this arm, so without this test the fallback is unpinned.
    #[test]
    fn a_bare_unnameable_key_falls_back_to_the_unnameable_hint() {
        let mut st = CaptureState::armed();
        let out = down(&mut st, VK_NUMPAD0);
        assert_eq!(
            out,
            Outcome::Refused(Refusal::NoModifier),
            "nothing is held, so this is the no-modifier refusal"
        );
        assert_eq!(
            st.refused_keycap().map(|k| k.name.as_str()),
            None,
            "the 81-key table cannot name numpad0"
        );
        assert_eq!(
            hint(out, st.refused_keycap()).as_deref(),
            Some("beckon has no name for that key. Pick one from the Key list.")
        );

        // A bare lock key lands in the same arm for the same reason, and
        // NOT in the reserved arm: `mods.any()` is tested before
        // `is_reserved`.
        let mut caps = CaptureState::armed();
        let out = down(&mut caps, VK_CAPITAL);
        assert_eq!(out, Outcome::Refused(Refusal::NoModifier));
        assert_eq!(
            hint(out, caps.refused_keycap()).as_deref(),
            Some("beckon has no name for that key. Pick one from the Key list.")
        );
    }

    /// The one invented string, so the test is what bounds what it may
    /// claim: `is_reserved` covers two unrelated families and the sentence
    /// has to be true of both.
    ///
    /// `Windows reserves that shortcut.` was not. It is true of `Win+L` and
    /// `Ctrl+Alt+Del`, and false of the three lock keys -- those are refused
    /// because the light toggles before the hook runs, which is beckon's
    /// limit. Both halves below assert the same sentence deliberately.
    #[test]
    fn the_reserved_hint_is_true_of_both_reserved_families() {
        let mut win_l = CaptureState::armed();
        down(&mut win_l, VK_LWIN);
        let out = down(&mut win_l, VK_L);
        assert_eq!(out, Outcome::Refused(Refusal::Reserved));
        assert_eq!(
            hint(out, win_l.refused_keycap()).as_deref(),
            Some("beckon cannot bind that shortcut. Try a different one."),
            "Windows' own reservation"
        );

        let mut lock = CaptureState::armed();
        down(&mut lock, VK_CONTROL);
        let out = down(&mut lock, VK_CAPITAL);
        assert_eq!(out, Outcome::Refused(Refusal::Reserved));
        assert_eq!(
            hint(out, lock.refused_keycap()).as_deref(),
            Some("beckon cannot bind that shortcut. Try a different one."),
            "beckon's own limit -- the sentence must not blame Windows here"
        );
    }

    /// **A spec-text pin, not a behaviour test.** `hint` cannot produce this
    /// string and no assertion below calls it: `HINT_UNAVAILABLE` is shown
    /// *instead of* arming, when `SetWindowsHookExW` fails, and there is no
    /// `Outcome` for a hook that never installed. Nothing consumes the
    /// const yet either -- `settings_window.rs` will, on a `false` from
    /// `arm_capture` -- so until it does this asserts a const against its
    /// own literal and cannot fail under any breakage. It is here to make an
    /// edit to F.3's wording deliberate, and that is the whole of its value.
    ///
    /// The rule it carries: never fall back to message-queue capture when
    /// this is shown. That path cannot see the Windows key, so it fails on
    /// precisely the chords beckon recommends -- and it fails by recording
    /// the wrong chord rather than by refusing.
    #[test]
    fn the_unavailable_hint_is_verbatim() {
        assert_eq!(
            HINT_UNAVAILABLE,
            "Cannot record here. Use the modifier boxes and the Key list instead."
        );
    }

    /// Every Outcome the UI must react to has to be posted; the ones it
    /// need not see must not wake it.
    #[test]
    fn only_outcomes_the_window_must_see_are_posted() {
        assert!(!Outcome::Ignored.post());
        assert!(
            !Outcome::PassThrough.post(),
            "it decides the hook's return value, not what the window shows"
        );
        assert!(Outcome::Partial.post());
        assert!(Outcome::Captured.post());
        assert!(Outcome::Cancelled.post());
        assert!(Outcome::Disarmed.post());
        assert!(Outcome::Refused(Refusal::NoModifier).post());
    }

    /// Every outcome survives the trip through a `WPARAM` and comes back as
    /// itself. Listed rather than generated: the point is that no two share
    /// a code, and a `for` loop over `code()` could not prove that.
    #[test]
    fn every_outcome_round_trips_through_its_code() {
        let all = [
            Outcome::Ignored,
            Outcome::Partial,
            Outcome::Captured,
            Outcome::Cancelled,
            Outcome::Disarmed,
            Outcome::PassThrough,
            Outcome::Refused(Refusal::NoModifier),
            Outcome::Refused(Refusal::UnknownKey),
            Outcome::Refused(Refusal::Reserved),
        ];
        for o in all {
            assert_eq!(
                Outcome::from_code(o.code()),
                Some(o),
                "{o:?} did not survive the trip"
            );
        }
        let mut codes: Vec<usize> = all.iter().map(|o| o.code()).collect();
        codes.sort_unstable();
        codes.dedup();
        assert_eq!(
            codes.len(),
            all.len(),
            "two outcomes share a code, so the window would react to the wrong one"
        );
    }

    /// A code beckon never wrote decodes to nothing rather than to the first
    /// variant. `WM_APP + n` is private by convention only.
    #[test]
    fn an_unknown_code_decodes_to_nothing() {
        assert_eq!(Outcome::from_code(9), None);
        assert_eq!(Outcome::from_code(usize::MAX), None);
    }
}
