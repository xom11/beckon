# Landing 2b-ii: the Caps Lock row — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the settings window's three-radio "beckon key" block with the
one-line definition spec §F.8 specifies: what holding Caps stands for, and what
tapping it alone does.

**Architecture:** The model already carries both values — `KeyboardConfig`
has `caps_hold: Chord` and `caps_tap: CapsTap`, and `config_write` already
writes them. This landing exposes `caps_hold` through `ControlState`, adds the
one setter it lacks, and rebuilds the window's keyboard group around them.

**Tech Stack:** Rust. `beckon-core` (all three CI jobs) and
`beckon-windows` (one). No new dependencies.

**Spec:** `docs/superpowers/specs/2026-08-11-settings-window-redesign.md` §F.8,
and §D.2 for why `Chord` is shaped the way it is.

## Global Constraints

- **ABORT-CLASS.** Never hold a `RefCell` borrow of `UI` across any
  `SendMessageW` / `PostMessageW` / `SetWindowPos` / `SetFocus` /
  `SetWindowTextW`. A second borrow across the `extern "system"` wndproc
  boundary **aborts the process** rather than unwinding, and neither the
  compiler, the tests nor the cross-compile can see it. Copy handles out
  (they are `Copy`), drop the borrow, then send.
- **Do not change *when* `layout` runs.** It is called only when
  `Ui::shown_external` or `Ui::shown_empty` flips. A populated `CBS_DROPDOWN`
  re-synchronises its edit field and selects the whole string when resized;
  an unconditional `layout` on the keystroke path once silently replaced what
  a user typed with a catalogue entry.
- **Display strings are ASCII.** Comments and test assertion messages are exempt.
- **Control ids 1001–1007 are fixed points**, hard-coded in
  `crates/beckon-windows/examples/settings_probe.rs`; it also reads 1008, 1012
  and 1013. It does **not** reference 1009–1011, so those are free to delete.
  New ids continue upward from 1021 (the filter box).
- Gates: `cargo fmt --all -- --check`, `cargo test -p beckon-core`,
  `cargo clippy -p beckon-core --all-targets -- -D warnings`,
  `cargo check --target x86_64-pc-windows-gnu -p beckon-windows`,
  `cargo check --target x86_64-pc-windows-gnu -p beckon-cli`.
- `cargo test --workspace` is **already broken on macOS** — `beckon-windows`
  cannot resolve the `windows` crate for the host target. Pre-existing and
  verified. Do not try to fix it; do not treat it as a regression.

## The spec is wrong about one thing, and the type is right

§F.8 sketches the row as:

```
☐ Caps Lock    Hold  [Ctrl] [Win] [Alt] [Shift]    Tap  [ Caps Lock ▾ ]
```

**There is no Shift chip.** `Chord` (`shortcuts.rs`) has exactly three fields —
`ctrl`, `super_`, `alt` — and its doc says why in terms this landing must not
undo: *"Shift is absent from the type rather than rejected by a rule. The hook
has to press and release whatever is here, and releasing Shift while the user
is physically holding it tells Windows their Shift is up — so everything they
type next arrives lowercase, silently."* `Chord::parse` refuses `shift`
explicitly with that message.

So: **three chips.** A fourth would either not compile or would need `Chord`
to grow the field its own documentation exists to prevent.

---

## File Structure

| File | Responsibility | Task |
|---|---|---|
| `crates/beckon-core/src/settings.rs` | `ControlState::caps_hold`, `Model::set_caps_hold` | 1 |
| `crates/beckon-windows/src/settings_window.rs` | delete the radios, build and lay out the new row | 2 |
| `crates/beckon-cli/src/serve.rs` | wire `on_caps_hold` | 2 |

---

## Task 1: the model side

**Files:**
- Modify: `crates/beckon-core/src/settings.rs` — `ControlState` (~line 148),
  `control_state` (~line 745), beside `set_caps_tap` (~line 452)
- Test: same file, `mod tests`

**Interfaces:**
- Consumes: `shortcuts::Chord`, already a field of `KeyboardConfig`.
- Produces: `ControlState::caps_hold: Chord` and
  `Model::set_caps_hold(&mut self, c: Chord) -> bool`. Task 2 calls both.

- [ ] **Step 1: Write the failing tests**

```rust
    // ---------- the Caps hold chord ----------

    #[test]
    fn the_hold_chord_reaches_the_window() {
        let m = model();
        let cs = control_state(&m, &status_all_ok());
        assert_eq!(
            cs.caps_hold,
            Chord::default(),
            "an untouched file uses the default chord, and the window has to \
             show it rather than guess"
        );
    }

    #[test]
    fn setting_the_hold_chord_is_a_file_change() {
        let mut m = model();
        assert!(m.set_caps_hold(Chord { ctrl: true, super_: false, alt: false }));
        assert!(m.dirty());
        assert_eq!(control_state(&m, &status_all_ok()).caps_hold.canonical(), "ctrl");
    }

    #[test]
    fn setting_the_same_hold_chord_twice_is_not_a_change() {
        let mut m = model();
        assert!(m.set_caps_hold(Chord::default()));
        assert!(!m.dirty(), "writing back what was already there is not an edit");
    }

    /// `Chord::parse` refuses an empty chord because the hook has to have
    /// something to press. The window can reach that state by unticking the
    /// last chip, so the model refuses it there too rather than letting an
    /// unwritable value into itself.
    #[test]
    fn unticking_the_last_modifier_is_refused() {
        let mut m = model();
        let before = m.keyboard.caps_hold;
        assert!(!m.set_caps_hold(Chord { ctrl: false, super_: false, alt: false }));
        assert_eq!(m.keyboard.caps_hold, before, "the chord is unchanged");
        assert!(!m.dirty());
    }

    /// The round trip that matters: whatever the window sets must come back
    /// out of the file meaning the same thing.
    #[test]
    fn the_hold_chord_survives_a_save_and_reload() {
        let mut m = model();
        m.set_caps_hold(Chord { ctrl: true, super_: false, alt: true });
        let text = m.render().unwrap();
        let back = Model::from_text(&text).unwrap();
        assert_eq!(back.keyboard.caps_hold.canonical(), "ctrl+alt");
    }
```

- [ ] **Step 2: Run them and confirm they fail**

Run: `cargo test -p beckon-core --lib settings::tests 2>&1 | tail -20`
Expected: FAIL — no field `caps_hold` on `ControlState`, no method `set_caps_hold`.

- [ ] **Step 3: Implement**

Add to `ControlState`, beside `caps_tap`:

```rust
    /// What holding Caps Lock stands for. The window shows it as three
    /// chips; `Chord` has no `shift` field and must not grow one -- see its
    /// own doc for why.
    pub caps_hold: Chord,
```

Set it in `control_state`: `caps_hold: m.keyboard.caps_hold,`
and in `unreadable_state`: `caps_hold: Chord::default(),`

Beside `set_caps_tap`:

```rust
    /// Set what holding Caps stands for. Returns whether the model now holds
    /// `c`.
    ///
    /// **Refuses a chord with no modifiers.** The window can reach that by
    /// unticking the last chip, and `Chord::parse` rejects the same value on
    /// the way back in -- so accepting it here would let the window write a
    /// file beckon cannot read. Refusing at the setter keeps the unwritable
    /// state out of the model rather than catching it at render time.
    pub fn set_caps_hold(&mut self, c: Chord) -> bool {
        if !(c.ctrl || c.super_ || c.alt) {
            return false;
        }
        if self.keyboard.caps_hold != c {
            self.keyboard.caps_hold = c;
            self.dirty = true;
        }
        true
    }
```

- [ ] **Step 4: Run and confirm they pass**

Run: `cargo test -p beckon-core --lib 2>&1 | grep -E '^test result'`
Expected: PASS, nothing previously green red.

- [ ] **Step 5: Break it on purpose**

Delete the `if !(c.ctrl || c.super_ || c.alt) { return false; }` guard and run
`cargo test -p beckon-core --lib unticking_the_last`. Expected: FAIL. Restore
and re-run: PASS.

- [ ] **Step 6: Gates and commit**

```bash
cargo fmt --all -- --check
cargo test -p beckon-core
cargo clippy -p beckon-core --all-targets -- -D warnings
git add crates/beckon-core/src/settings.rs
git commit -m "feat(core): the Caps hold chord reaches the window

ControlState gains caps_hold and Model gains the setter it lacked. The
setter refuses a chord with no modifiers: the window can reach that state by
unticking the last chip, and Chord::parse rejects the same value on the way
back in, so accepting it would let the window write a file beckon cannot
read."
```

---

## Task 2: the window row

**Files:**
- Modify: `crates/beckon-windows/src/settings_window.rs` — ids (~line 195),
  `mod cap` (~line 266), `build_children` (~line 1716), `layout` (~line 2334),
  `apply_state` (~line 2533), `handle_command` (~line 3487)
- Modify: `crates/beckon-cli/src/serve.rs` — the `Callbacks` literal (~line 1136)

**Interfaces:**
- Consumes: `ControlState::caps_hold`, `Model::set_caps_hold` (Task 1).
- Produces: `Callbacks::on_caps_hold: Box<dyn FnMut(Chord)>`.

**What goes away:** `IDC_TAP_CAPSLOCK` (1009), `IDC_TAP_ESCAPE` (1010),
`IDC_TAP_NONE` (1011) and their three `cap::TAP_*` strings. The probe does not
reference them (it pins 1001–1007 and reads 1008, 1012, 1013), so nothing
outside this file breaks.

**What arrives**, ids continuing past the filter box's 1021:

| id | control | style |
|---|---|---|
| 1022 | Hold `Ctrl` | `BS_AUTOCHECKBOX` |
| 1023 | Hold `Win` | `BS_AUTOCHECKBOX` |
| 1024 | Hold `Alt` | `BS_AUTOCHECKBOX` |
| 1025 | Tap | `CBS_DROPDOWNLIST` |
| 1026 | the static word `Hold` | `STATIC`, `SS_CENTERIMAGE` |
| 1027 | the static word `Tap` | `STATIC`, `SS_CENTERIMAGE` |

- [ ] **Step 1: Strings**

In `mod cap`, delete `TAP_CAPSLOCK`, `TAP_ESCAPE`, `TAP_NONE` and change the
check box caption. The phrase "beckon key" leaves the window entirely — it
was internal vocabulary that made the row explain itself in prose.

```rust
    pub const CAPS: &str = "Use &Caps Lock as a shortcut key";
    pub const HOLD: &str = "Hold";
    pub const TAP: &str = "Tap";
    pub const HOLD_CTRL: &str = "&Ctrl";
    pub const HOLD_WIN: &str = "&Win";
    pub const HOLD_ALT: &str = "A&lt";
    /// The three `Tap` items, in `CB_ADDSTRING` order. Read back by INDEX
    /// with `CB_GETCURSEL`, never by text: even a `DROPDOWNLIST` has
    /// typeahead, which moves the selection.
    pub const TAP_ITEMS: [&str; 3] = ["Caps Lock", "Esc", "Nothing"];
```

- [ ] **Step 2: Build the controls**

In `build_children`, replace the three radio `child(...)` calls with the three
Hold check boxes, the two `STATIC`s and the Tap combo. The combo is created
with `CBS_DROPDOWNLIST | WS_VSCROLL | WS_TABSTOP`, then filled:

```rust
    for item in cap::TAP_ITEMS {
        let t = wide(item);
        SendMessageW(tap, CB_ADDSTRING, Some(WPARAM(0)), Some(LPARAM(t.as_ptr() as isize)));
    }
```

`wide` returns a `Vec<u16>`; bind it to a local so it outlives the call.

**Fix the `WS_GROUP` comment on `IDC_OPENFILE`.** It currently explains that
`WS_GROUP` there closes the radio group `IDC_TAP_CAPSLOCK` opened. With the
radios gone there is no radio group; keep the style, rewrite the comment to
say what it now does — a plain group boundary before the command bar.

- [ ] **Step 3: Lay the row out**

The keyboard group becomes ONE content line instead of two. In `layout`,
replace the radio placement with, left to right on `ry`:

`IDC_CAPS` (its own text width) · `Hold` · `Ctrl` `Win` `Alt` · `Tap` ·
the combo.

Widths come from `tw(...)` plus the check box glyph, exactly as the radios
did — the `s(190)/s(70)/s(90)` constants those replaced were sized for one
font at one DPI and clipped the moment either changed. The combo takes
`s(tok::SHORTCUT_COL).min(...)` of what is left.

`kb_h` shrinks by one `ctl + gap` now that the second line is gone. Keep the
caption inset and the bottom inset as they are.

- [ ] **Step 4: Push state and read it back**

In `apply_state`, replace the three `check(...)` calls for the radios with:

```rust
        check(hwnd, IDC_HOLD_CTRL, st.caps_hold.ctrl);
        check(hwnd, IDC_HOLD_WIN, st.caps_hold.super_);
        check(hwnd, IDC_HOLD_ALT, st.caps_hold.alt);
```

and select the Tap item by **index**:

```rust
        // By index. Even a DROPDOWNLIST has typeahead, which moves the
        // selection, so reading or writing this control by TEXT would make
        // the model follow whatever the user's last keystroke selected.
        let want = match st.caps_tap {
            CapsTap::CapsLock => 0usize,
            CapsTap::Escape => 1,
            CapsTap::None => 2,
        };
        if cur_sel(tap) != Some(want) {
            SendMessageW(tap, CB_SETCURSEL, Some(WPARAM(want)), Some(LPARAM(0)));
        }
```

Guard the write with a read, like every other field write in this function:
an unconditional `CB_SETCURSEL` raises `CBN_SELCHANGE` on every push.

Everything in the group follows `st.editable` and `st.caps_checked` for
enablement, the way the radios did: the three chips and the Tap combo are
greyed while the check box is clear.

- [ ] **Step 5: Commands**

In `handle_command`, replace the three `IDC_TAP_*` arms with:

```rust
        (IDC_HOLD_CTRL, _) | (IDC_HOLD_WIN, _) | (IDC_HOLD_ALT, _) => {
            // All three read together: the chord is one value, and a setter
            // that took one flag at a time could not refuse "none ticked"
            // without knowing the other two.
            let c = Chord {
                ctrl: is_checked(hwnd, IDC_HOLD_CTRL),
                super_: is_checked(hwnd, IDC_HOLD_WIN),
                alt: is_checked(hwnd, IDC_HOLD_ALT),
            };
            with_cb(|cb| (cb.on_caps_hold)(c));
        }
        (IDC_TAP, c) if c == CBN_SELCHANGE => {
            if let Some(i) = cur_sel(tap) {
                let t = match i {
                    0 => CapsTap::CapsLock,
                    1 => CapsTap::Escape,
                    _ => CapsTap::None,
                };
                with_cb(|cb| (cb.on_caps_tap)(t));
            }
        }
```

`BS_AUTOCHECKBOX` toggles itself before the notification arrives, so reading
all three back is reading the state the user now sees.

**`set_caps_hold` returns `false` when the user unticks the last chip.** The
model then still holds the previous chord, and the next `apply_state` push
re-ticks the box the user just cleared — which is the correct behaviour and
needs no special case here, because `apply_state` writes checkbox state
unconditionally from the model.

- [ ] **Step 6: Wire the callback**

`Callbacks` gains:

```rust
    /// What holding Caps stands for. The window sends all three chips
    /// together because they are one value.
    pub on_caps_hold: Box<dyn FnMut(Chord)>,
```

In `serve.rs`, beside `on_caps_tap`:

```rust
        on_caps_hold: Box::new(edit!(
            state,
            |m: &mut beckon_core::settings::Model, c| {
                m.set_caps_hold(c);
            }
        )),
```

- [ ] **Step 7: Gates**

```
cargo fmt --all -- --check
cargo test -p beckon-core
cargo check --target x86_64-pc-windows-gnu -p beckon-windows
cargo check --target x86_64-pc-windows-gnu -p beckon-cli
```

Then read your own diff against the ABORT-class rule and say in your report
what you checked: every `UI` borrow you added or moved, and every message
sent after it.

- [ ] **Step 8: Commit**

```bash
git add crates/beckon-windows/src/settings_window.rs crates/beckon-cli/src/serve.rs
git commit -m "feat(windows): the Caps Lock row says what the key does

Three radios and the phrase 'beckon key' go; one line arrives that names
the two things the key can do. Hold is three chips -- Ctrl, Win, Alt -- not
the four spec F.8 sketches: Chord has no shift field, on purpose, because
the hook has to release whatever it presses and releasing Shift under the
user's fingers makes everything they type next lowercase.

Tap is a DROPDOWNLIST read and written by INDEX, never by text, because even
a DROPDOWNLIST has typeahead that moves the selection.

The WS_GROUP on Open config file stays, but its comment no longer describes
a radio group that no longer exists."
```

---

## Self-Review

**Spec coverage:** §F.8's row shape is Task 2 steps 2–3; the radios' deletion
and the `CBS_DROPDOWNLIST` read-by-`CB_GETCURSEL` are steps 2, 4 and 5; the
`WS_GROUP` fix §F.8 asks for "in the same pass" is step 2; the config keys
already exist and are unchanged, which is why no `config_write` work appears
here.

**Deliberately NOT in this plan:** §F.8's line stating the hook cost when the
box is ticked (*"Turning this on installs a keyboard hook while beckon
runs…"*) belongs with the notes strip and the hint wiring, not the layout;
the default-off decision is already the model's and needs no code; the
`Add`-prefills-`caps_hold` behaviour and the dimmed-shared-prefix rendering
are both list-rendering work.

**Type consistency:** `Chord` is `Copy` with fields `ctrl`, `super_`, `alt`.
`set_caps_hold` returns `bool`. `ControlState::caps_hold` is a `Chord`, not
three bools, so the window cannot construct a half-updated value.
