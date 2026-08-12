# Landing 2b-i: the capture state machine — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The pure decision logic behind chord capture — `beckon_core::capture` —
with no Win32, no window, and no hook.

**Architecture:** Landing 2b decomposes into four independent pieces; this is
the first and the other three depend on it. It mirrors `beckon_core::caps`
exactly: a pure state machine beside the impure hook, for the reason
`caps.rs` already gives — *"a keyboard state machine is the last thing that
should be tested by one job in three."* `beckon-core` compiles on all three
CI jobs; `beckon-windows` on one.

**Tech Stack:** Rust, `beckon-core` only. No new dependencies. Nothing in
this plan touches `beckon-windows` or `beckon-cli`.

**Spec:** `docs/superpowers/specs/2026-08-11-settings-window-redesign.md`
§F.3 (the state machine), §F.4 (edge cases), §F.5 (what cannot be captured —
**measured 2026-08-12**, see `docs/superpowers/measurements/2026-08-11-landing-1-a14.md` §47–§50).

## Global Constraints

- **`step` must not allocate and must not format strings.** §F.2: the
  callback *"does three things only: read `vkCode`, update a fixed-size held
  array, `PostMessage`… Everything visible is built on the UI thread when
  `WM_CAPTURE` arrives."* So `step` returns a `Copy` enum and the display
  strings are built later, on demand, by separate methods. A `String` in
  `step`'s return type is a design error, not a style one — the callback runs
  under `LowLevelHooksTimeout` (300 ms default) and Windows silently unhooks
  a callback that overruns, with no error anywhere.
- **Display strings are ASCII**, like `mark_glyph`: the window inherits the
  shell font and a missing glyph reads as a rendering bug. Comments and test
  assertion messages are exempt.
- **No new dependencies.**
- Gates, from the repo root: `cargo fmt --all -- --check`,
  `cargo test -p beckon-core`, and
  `cargo check --target x86_64-pc-windows-gnu -p beckon-windows` (a
  cross-check that does not link and cannot see MSVC).
- `cargo test --workspace` is **already broken on macOS** at the branch
  point — `beckon-windows` cannot resolve the `windows` crate for the host
  target. Verified identical before and after this branch's parent. Do not
  try to fix it and do not treat it as a regression.

---

## File Structure

| File | Responsibility | Task |
|---|---|---|
| `crates/beckon-core/src/shortcuts.rs` | `lookup_win_vk`, the reverse of `lookup_key` | 1 |
| `crates/beckon-core/src/capture.rs` | **new** — `CaptureState`, `Outcome`, `Refusal`, `step` | 2 |
| `crates/beckon-core/src/lib.rs` | `pub mod capture;` | 2 |

---

## Task 1: reverse the key table

**Files:**
- Modify: `crates/beckon-core/src/shortcuts.rs` — beside `lookup_key` (~line 120)
- Test: same file, `mod tests`

**Interfaces:**
- Consumes: the existing `all_keys() -> &'static [KeyDef]` and
  `KeyDef { name: String, mac: u16, win: u32 }`.
- Produces: `pub fn lookup_win_vk(vk: u32) -> Option<&'static KeyDef>`, used
  by Task 2 to turn a `KBDLLHOOKSTRUCT.vkCode` into a key name.

- [ ] **Step 1: Write the failing tests**

```rust
    #[test]
    fn a_vk_maps_back_to_the_key_that_owns_it() {
        // 0x41 is VK_A; 0x1B is VK_ESCAPE.
        assert_eq!(lookup_win_vk(0x41).map(|k| k.name.as_str()), Some("a"));
        assert_eq!(lookup_win_vk(0x1B).map(|k| k.name.as_str()), Some("escape"));
    }

    #[test]
    fn a_vk_no_key_claims_maps_to_nothing() {
        // 0xFC is VK_NONAME, which `caps` uses precisely because nothing
        // reaches it. 0x00 is not a virtual key at all.
        assert_eq!(lookup_win_vk(0xFC).map(|k| k.name.as_str()), None);
        assert_eq!(lookup_win_vk(0x00).map(|k| k.name.as_str()), None);
    }

    /// The reverse lookup is only well defined if the forward table is
    /// injective on `win`. If two keys ever share a VK, `lookup_win_vk`
    /// silently starts returning whichever the iteration order reaches
    /// first -- so pin it here rather than discovering it through a
    /// mis-captured chord.
    #[test]
    fn no_two_keys_share_a_windows_vk() {
        let mut seen: std::collections::HashMap<u32, &str> = std::collections::HashMap::new();
        for k in all_keys() {
            if let Some(prev) = seen.insert(k.win, k.name.as_str()) {
                panic!("`{prev}` and `{}` both claim VK {:#04x}", k.name, k.win);
            }
        }
    }

    /// Every key the user can type must survive the round trip, or capture
    /// can record a chord that `Combo::parse` then rejects.
    #[test]
    fn every_key_round_trips_through_its_vk() {
        for k in all_keys() {
            let back = lookup_win_vk(k.win).unwrap_or_else(|| panic!("{} lost", k.name));
            assert_eq!(back.name, k.name);
        }
    }
```

- [ ] **Step 2: Run them and confirm they fail**

Run: `cargo test -p beckon-core --lib shortcuts::tests 2>&1 | tail -20`
Expected: FAIL — `cannot find function 'lookup_win_vk'`.

- [ ] **Step 3: Implement**

Beside `lookup_key`:

```rust
/// The reverse of `lookup_key`: the key a Windows virtual-key code belongs
/// to, or `None` for one no binding can name (numpad, media, IME,
/// `VK_PROCESSKEY`).
///
/// Linear, like `lookup_key`, over 81 entries. It runs once per captured
/// key-down, not per keystroke, so a map would buy nothing and cost a
/// second source of truth.
pub fn lookup_win_vk(vk: u32) -> Option<&'static KeyDef> {
    all_keys().iter().find(|k| k.win == vk)
}
```

- [ ] **Step 4: Run them and confirm they pass**

Run: `cargo test -p beckon-core --lib 2>&1 | grep -E '^test result'`
Expected: PASS, nothing previously green turned red.

- [ ] **Step 5: Break it on purpose**

Change `k.win == vk` to `k.mac as u32 == vk`, run
`cargo test -p beckon-core --lib shortcuts::tests`. Expected: the round-trip
and the two lookup tests FAIL. Restore and re-run: PASS. A test that cannot
tell the defect from the fix is not a test.

- [ ] **Step 6: Commit**

```bash
git add crates/beckon-core/src/shortcuts.rs
git commit -m "feat(core): look a key up by its Windows VK

The reverse of lookup_key, which chord capture needs to turn a
KBDLLHOOKSTRUCT.vkCode into a key name. Pins two properties the reverse
direction depends on and the forward one never needed: the table is
injective on \`win\`, and every key round-trips."
```

---

## Task 2: the capture state machine

**Files:**
- Create: `crates/beckon-core/src/capture.rs`
- Modify: `crates/beckon-core/src/lib.rs` — add `pub mod capture;`
- Test: `crates/beckon-core/src/capture.rs`, `mod tests`

**Interfaces:**
- Consumes: `lookup_win_vk` (Task 1); `caps::{KeyEvent, Edge}`;
  `shortcuts::{Combo, KeyDef}`.
- Produces: `CaptureState`, `Outcome`, `Refusal`, `capture::step`. Later
  pieces of Landing 2b call `step` from the hook and read the display
  strings from `CaptureState` on the UI thread.

- [ ] **Step 1: Write the failing tests**

Create `crates/beckon-core/src/capture.rs` containing ONLY the test module
below plus `use` lines, so the tests fail to compile against the missing
API rather than passing vacuously.

```rust
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
        KeyEvent { vk, edge, injected_by_us: false, time_ms: 0 }
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
        assert_eq!(down(&mut st, VK_NUMPAD0), Outcome::Refused(Refusal::UnknownKey));
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
        assert_eq!(down(&mut st, VK_CAPITAL), Outcome::Refused(Refusal::Reserved));
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
        assert_eq!(st.partial(), None, "nothing is held, so there is no partial combo");
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

    #[test]
    fn a_cancel_drains_too() {
        let mut st = CaptureState::armed();
        down(&mut st, VK_CONTROL);
        assert_eq!(down(&mut st, VK_ESCAPE), Outcome::Cancelled);
        assert!(st.draining());
        assert_eq!(up(&mut st, VK_ESCAPE), Outcome::Ignored);
        assert_eq!(up(&mut st, VK_CONTROL), Outcome::Disarmed);
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
        let injected = KeyEvent { vk: VK_CONTROL, edge: Edge::Down, injected_by_us: true, time_ms: 0 };
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
```

- [ ] **Step 2: Run and confirm they fail**

Add `pub mod capture;` to `crates/beckon-core/src/lib.rs` first, or the file
is not compiled and the tests cannot fail.

Run: `cargo test -p beckon-core --lib capture 2>&1 | tail -20`
Expected: compile errors naming `CaptureState`, `Outcome`, `Refusal`, `step`.

- [ ] **Step 3: Implement**

Write `crates/beckon-core/src/capture.rs` above its test module. The shape:

```rust
//! Chord capture: what to do with each key event while the field is
//! recording. Pure, and beside `caps::decide` for the same reason that one
//! is pure -- a keyboard state machine is the last thing that should be
//! tested by one job in three.
//!
//! **`step` does not allocate and does not format.** It runs inside the
//! `WH_KEYBOARD_LL` callback, which Windows silently unhooks if it overruns
//! `LowLevelHooksTimeout` (300 ms by default) with no error anywhere. Every
//! display string is built later, on the UI thread, by the methods below.

use crate::caps::{Edge, KeyEvent};
use crate::shortcuts::{lookup_win_vk, Combo, KeyDef};

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
```

`CaptureState` holds: a fixed-size held array (no allocation), the four
normalised modifier flags derived from it, `captured: Option<Combo>`,
`refused_keycap: Option<&'static KeyDef>`, and a `draining` flag. Methods
`armed()`, `captured()`, `partial()`, `refused_keycap()`, `draining()`.
`partial()` returns `Option<String>` and is a UI-thread method, never called
from the callback.

`step` in order:
1. `ev.injected_by_us` → `Ignored`.
2. While `draining`: an `Up` clears the vk from the held set; when the set is
   empty return `Disarmed`, else `Ignored`. A `Down` while draining is
   `Ignored`.
3. `Edge::Up` → clear from the held set; `Partial` if it was a modifier,
   else `Ignored`.
4. `Edge::Down` for a vk already held → `Ignored` (auto-repeat).
5. `Edge::Down` of a modifier → record, `Partial`.
6. Bare `VK_ESCAPE` with no modifier held → `Cancelled`, start draining.
7. `Edge::Down` of a non-modifier:
   - no modifier held → `Refused(NoModifier)`, remember the keycap if the
     table names it.
   - in the reserved set → `Refused(Reserved)`.
   - not in the table → `Refused(UnknownKey)`.
   - otherwise build the `Combo`, store it, start draining, `Captured`.

Normalise `VK_LCONTROL`/`VK_RCONTROL` to `VK_CONTROL` and the same for the
other three, plus `VK_LWIN`/`VK_RWIN`.

The reserved set is `VK_L` while Win is held (measured), plus `VK_CAPITAL`,
`VK_NUMLOCK` and `VK_SCROLL` as main keys. Write the a14 measurement
reference into the comment beside it.

- [ ] **Step 4: Run and confirm they pass**

Run: `cargo test -p beckon-core --lib 2>&1 | grep -E '^test result'`
Expected: PASS, nothing previously green red.

- [ ] **Step 5: Break each guard on purpose, one at a time**

Each revert must turn its own named test red, and be restored before the
next:

1. Drop the `injected_by_us` early return → `our_own_injected_keys_are_ignored`.
2. Drop the already-held check → `auto_repeat_of_a_held_modifier_changes_nothing`.
3. Drop the reserved set → `a_reserved_chord_is_refused_rather_than_recorded`
   and `a_lock_key_is_refused_as_a_main_key`.
4. Let `Cancelled` skip draining → `a_cancel_drains_too`.

Paste each result.

- [ ] **Step 6: Gates and commit**

```bash
cargo fmt --all -- --check
cargo test -p beckon-core
cargo check --target x86_64-pc-windows-gnu -p beckon-windows
git add crates/beckon-core/src/capture.rs crates/beckon-core/src/lib.rs
git commit -m "feat(core): the chord-capture state machine

Pure, beside caps::decide, for the reason that one is pure: a keyboard
state machine is the last thing that should be tested by one job in three.

step does not allocate and does not format -- it runs inside the
WH_KEYBOARD_LL callback, which Windows silently unhooks past
LowLevelHooksTimeout with no error anywhere. It returns a Copy enum and the
display strings are built on the UI thread from the state.

Refusal::Reserved exists because of a measurement, not a document: the hook
DOES see Win+L, contrary to what spec F.5 claimed, and cannot suppress it --
so capture would otherwise record a chord that can never fire."
```

---

## Self-Review

**Spec coverage:** §F.3's Armed / Holding / Committed / Cancelled / Draining
transitions are Task 2 steps 1-7 and their tests. §F.4's rows: Esc (two
tests), Tab (falls out of the generic non-modifier path — `alt+tab` is
recordable and `draining` is what makes it safe), modifier-only (the
release-to-Armed test), auto-repeat (its own test), left/right normalisation
(its own test). §F.5's reserved set is `Refusal::Reserved`.

**Deliberately NOT in this plan**, and each belongs to a later piece of 2b:
Sticky Keys' `GetAsyncKeyState` union at commit (§F.4) needs a live OS
read, so it is a parameter the caller supplies rather than something `step`
can do; the 10 s watchdog, the foreground gate and `SetWindowsHookExW`
failure (§F.3) are all Win32; the availability probe is §F.6; the Caps row
is §F.8.

**Type consistency:** `lookup_win_vk` (Task 1) returns
`Option<&'static KeyDef>` and is consumed by Task 2 for both the captured
key and `refused_keycap`. `Outcome` and `Refusal` are `Copy`, which the
`post()` test relies on.

---

## What 2b-i leaves for the later pieces

Written here rather than in the SDD ledger, which is scratch and gets deleted.

**A contract 2b-iv (hook wiring) must honour, and which nothing enforces yet.**
`Outcome::PassThrough` is the **one** outcome where the hook must NOT return
`1`. Spec §F.2's sketch returns `LRESULT(1)` unconditionally while armed;
that sketch predates this outcome. Wiring that returns 1 for everything
except `Disarmed` reinstates the stuck-modifier bug **with every test in this
crate still green**, because no test in `beckon-core` can see what the
callback returns. Assert it there.

The bug it exists for, so it is not re-simplified away: hold `Ctrl`
physically, click `Record` **with the mouse** — so the Ctrl-down was never
seen — then press `Alt+T`. On commit the drain eats the Ctrl-up, and the
system believes Ctrl is held with no up ever coming. Unrecoverable without
killing beckon. The armed path was worse still: the up was swallowed *and*
posted a redraw.

**Two hazards `step` documents but cannot fix**, both for the hook piece:

- Refusal beeps must be de-duplicated **by vk**. Auto-repeat of a refused
  bare key re-refuses on every repeat, and F.3 beeps on refusal.
- Admitting refused keys to the held set is the obvious in-module fix for
  that and is **wrong**: rolled-over bare keys would consume the fixed
  `HELD_MAX` slots and silently drop a real modifier.

**One deferred defect**, for whichever piece wires the hint strings: a bare
`VK_CAPITAL` reports `Refusal::NoModifier`, so it selects *"hold Ctrl, Win or
Alt as well"* — advice that cannot work for a lock key.

**Still a hardware question:** whether a passed-through key-up with no
matching key-down actually clears the system's modifier state. `PassThrough`
is the right thing to return either way; whether it is *sufficient* is for
the a14 pass.

## Two plan defects this landing found, recorded so the plan is not trusted over the code

1. **The brief's tests contradicted each other.**
   `escape_with_a_modifier_is_a_chord` and `a_cancel_drains_too` had
   byte-identical inputs (`armed → Ctrl↓ → Esc↓`) expecting `Captured` and
   `Cancelled`. No deterministic machine satisfies both. Resolved toward the
   prose, spec §F.3 and the sibling test's own doc comment, all three of
   which agree: bare Escape cancels, Escape with a modifier is a chord.
2. **The brief's reserved set omitted `Ctrl+Alt+Del`**, which spec §F.5 says
   to treat as refused until measured — and `delete` is in the 81-key table,
   so it was recordable.

Both were found by the implementer and flagged rather than silently
resolved, which is the only reason they are written down.
