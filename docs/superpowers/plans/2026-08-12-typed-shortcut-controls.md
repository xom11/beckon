# Landing 2b-iii: the typed shortcut controls — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the settings window's free-text Shortcut `EDIT` with the
typed path spec §C.4 calls primary: four modifier check boxes plus a closed
list of the 81 key names.

**Architecture:** The window keeps writing a canonical combo string through
the existing `on_edit_combo` / `Model::set_combo`, so nothing downstream
changes. What is new is a pure core function that turns a combo string into
the five control values, and a public view of the key table so the window can
fill the list.

**Tech Stack:** Rust. `beckon-core` (all three CI jobs), `beckon-windows`
(one). No new dependencies.

**Spec:** `docs/superpowers/specs/2026-08-11-settings-window-redesign.md` §C.4.

## Global Constraints

- **ABORT-CLASS.** Never hold a `RefCell` borrow of `UI` across any
  `SendMessageW` / `PostMessageW` / `SetWindowPos` / `SetFocus` /
  `SetWindowTextW`. A second borrow across the `extern "system"` wndproc
  boundary **aborts the process** rather than unwinding; the compiler, the
  tests and the cross-compile all miss it. Copy handles out, drop the borrow,
  then send.
- **Do not change *when* `layout` runs** — only what it places.
- **Every control write in `apply_state` is guarded by a read.** An
  unconditional write raises a change notification on every push and the
  model follows its own echo.
- **Display strings are ASCII.** Comments and test assertion messages are exempt.
- **`IDC_COMBO` stays 1002.** It changes class, not number: it was the
  Shortcut `EDIT` and becomes the key list. `examples/settings_probe.rs`
  hard-codes 1002 and drives it as an `EDIT`, so the probe must be updated in
  the same landing — that is expected, and is why the id is reused rather
  than retired.
- **The four new check boxes carry NO `&` mnemonic.** `Hold` already took
  `t`, `w` and `l` (`C&trl`, `&Win`, `A&lt`); four more modifier boxes would
  collide immediately. They are Tab-reachable, in a strip the user arrives at
  by selecting a row. Do not add mnemonics "for consistency" — check the
  table in `mod cap` first, which is the only guard that exists.
- Gates: `cargo fmt --all -- --check`, `cargo test -p beckon-core`,
  `cargo clippy -p beckon-core --all-targets -- -D warnings`,
  `cargo check --target x86_64-pc-windows-gnu -p beckon-windows`,
  `cargo check --target x86_64-pc-windows-gnu -p beckon-cli`.
- `cargo test --workspace` is **already broken on macOS** (`beckon-windows`
  cannot resolve the `windows` crate for the host target). Pre-existing and
  verified; ignore it.

---

## File Structure

| File | Responsibility | Task |
|---|---|---|
| `crates/beckon-core/src/shortcuts.rs` | `key_table()`, `ComboView`, `combo_view()` | 1 |
| `crates/beckon-windows/src/settings_window.rs` | the five controls, layout, push, commands | 2 |
| `crates/beckon-windows/examples/settings_probe.rs` | drive a list instead of an edit | 2 |

---

## Task 1: the combo, as five control values

**Files:**
- Modify: `crates/beckon-core/src/shortcuts.rs` — beside `lookup_win_vk` (~line 131)
- Test: same file, `mod tests`

**Interfaces:**
- Consumes: the existing `all_keys()`, `Combo::parse`, `KeyDef`.
- Produces: `pub fn key_table() -> &'static [KeyDef]`,
  `pub struct ComboView { ctrl, super_, alt, shift: bool, key: Option<usize> }`,
  `pub fn combo_view(s: &str) -> ComboView`. Task 2 uses all three.

- [ ] **Step 1: Write the failing tests**

```rust
    #[test]
    fn the_key_table_is_the_whole_key_list() {
        assert_eq!(key_table().len(), all_keys().len());
        assert!(key_table().iter().any(|k| k.name == "t"));
        assert!(key_table().iter().any(|k| k.name == "escape"));
    }

    #[test]
    fn a_combo_becomes_five_control_values() {
        let v = combo_view("ctrl+super+alt+t");
        assert!(v.ctrl && v.super_ && v.alt);
        assert!(!v.shift);
        assert_eq!(key_table()[v.key.expect("a key")].name, "t");
    }

    #[test]
    fn shift_is_a_modifier_here_unlike_in_a_hold_chord() {
        let v = combo_view("ctrl+shift+a");
        assert!(v.ctrl && v.shift);
        assert!(!v.super_ && !v.alt);
        assert_eq!(key_table()[v.key.unwrap()].name, "a");
    }

    /// A row that has never been given a shortcut, and a row whose stored
    /// text does not parse, must both render as "nothing chosen" rather
    /// than panicking or inventing a key.
    #[test]
    fn an_unparseable_combo_selects_nothing() {
        for s in ["", "ctrl+", "ctrl+nosuchkey", "banana"] {
            let v = combo_view(s);
            assert_eq!(v.key, None, "{s:?} must select no key");
            assert!(!(v.ctrl || v.super_ || v.alt || v.shift), "{s:?}");
        }
    }

    /// The round trip the window depends on: whatever the controls show,
    /// rebuilding the canonical string from them must mean the same thing.
    #[test]
    fn a_view_rebuilds_the_same_canonical_combo() {
        for s in ["ctrl+t", "ctrl+super+alt+shift+f1", "alt+escape", "super+space"] {
            let v = combo_view(s);
            let mut parts: Vec<&str> = Vec::new();
            if v.ctrl { parts.push("ctrl"); }
            if v.super_ { parts.push("super"); }
            if v.alt { parts.push("alt"); }
            if v.shift { parts.push("shift"); }
            let key = &key_table()[v.key.unwrap()].name;
            parts.push(key);
            assert_eq!(parts.join("+"), Combo::parse(s).unwrap().canonical(), "{s:?}");
        }
    }
```

- [ ] **Step 2: Run and confirm they fail**

Run: `cargo test -p beckon-core --lib shortcuts::tests 2>&1 | tail -20`
Expected: FAIL — `key_table`, `combo_view`, `ComboView` not found.

- [ ] **Step 3: Implement**

```rust
/// The whole key list, in the order the settings window shows it.
///
/// Public so the window can fill its key list without a second copy of the
/// names. Index into it is what `ComboView::key` means, and the two must
/// stay the same slice — which is why this returns `all_keys()` rather
/// than building anything.
pub fn key_table() -> &'static [KeyDef] {
    all_keys()
}

/// A combo as the five controls that show it: four modifier check boxes and
/// one index into `key_table`.
///
/// `key` is `None` when the string does not parse — a row that has never
/// been given a shortcut, or one whose stored text is not a valid combo.
/// The window shows that as "nothing selected" rather than guessing, and
/// `Model::problems` is what tells the user why.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ComboView {
    pub ctrl: bool,
    pub super_: bool,
    pub alt: bool,
    pub shift: bool,
    pub key: Option<usize>,
}

/// Render a combo string as control values. Never fails: an unparseable
/// string is `ComboView::default()`, i.e. nothing ticked and no key chosen.
pub fn combo_view(s: &str) -> ComboView {
    let Ok(c) = Combo::parse(s) else {
        return ComboView::default();
    };
    ComboView {
        ctrl: c.ctrl,
        super_: c.super_,
        alt: c.alt,
        shift: c.shift,
        key: key_table().iter().position(|k| k.name == c.key.name),
    }
}
```

- [ ] **Step 4: Run and confirm they pass**

Run: `cargo test -p beckon-core --lib 2>&1 | grep -E '^test result'`

- [ ] **Step 5: Break it on purpose**

Make `combo_view` return `ComboView::default()` unconditionally. Expected:
three of the five tests FAIL. Restore, re-run: PASS.

- [ ] **Step 6: Gates and commit**

```bash
cargo fmt --all -- --check
cargo test -p beckon-core
cargo clippy -p beckon-core --all-targets -- -D warnings
git add crates/beckon-core/src/shortcuts.rs
git commit -m "feat(core): a combo as the five controls that show it

key_table exposes the key list so the window needs no second copy of the
names, and combo_view turns a combo string into four modifier flags plus an
index into it. Unparseable input is nothing-ticked-and-no-key rather than an
error: a row that has never been given a shortcut has to render, and
Model::problems is already what tells the user why it is not valid."
```

---

## Task 2: the controls

**Files:**
- Modify: `crates/beckon-windows/src/settings_window.rs`
- Modify: `crates/beckon-windows/examples/settings_probe.rs`

**Interfaces:**
- Consumes: `key_table`, `combo_view`, `ComboView` (Task 1); the existing
  `Callbacks::on_edit_combo(String)`, unchanged.
- Produces: nothing new outside the window.

**Ids.** `IDC_COMBO` (1002) keeps its number and becomes a
`CBS_DROPDOWNLIST`. Four check boxes arrive:

| id | caption |
|---|---|
| 1028 | `Ctrl` |
| 1029 | `Win` |
| 1030 | `Alt` |
| 1031 | `Shift` |

No `&` on any of them — see the constraint above.

- [ ] **Step 1: Swap the control**

In `build_children`, replace the `IDC_COMBO` `EDIT` with a
`CBS_DROPDOWNLIST | WS_VSCROLL | WS_TABSTOP` combo, and fill it from
`key_table()` in order, so a `CB_SETCURSEL` index equals a `ComboView::key`
index. Bind each `wide()` buffer to a local that outlives its
`SendMessageW`.

Create the four check boxes **before** it, so tab order reads
`Ctrl → Win → Alt → Shift → key`.

`CB_SETMINVISIBLE` or a generous `cy` at creation: the list has 81 items, so
the dropped-down height must not be the default 30-item guess. Follow what
`IDC_APP` already does.

- [ ] **Step 2: Lay the strip out**

Band 4 becomes: `App` label · App combo · `Shortcut` label · four check boxes
· key list. Widths from `tw(...)` plus the check-box glyph, exactly as the
`Hold` chips do. The key list takes what is left, capped like the old field
was.

Both the check boxes and the list take the shared field height (`edit_h` /
`edit_dy`), so all three of App, the key list and the filter are one box
repeated.

- [ ] **Step 3: Push state**

Replace the `set_text(combo, &d.combo)` write with, from
`combo_view(&d.combo)`:

```rust
        let v = combo_view(&d.combo);
        check(hwnd, IDC_MOD_CTRL, v.ctrl);
        check(hwnd, IDC_MOD_WIN, v.super_);
        check(hwnd, IDC_MOD_ALT, v.alt);
        check(hwnd, IDC_MOD_SHIFT, v.shift);
        // By index, and guarded by a read. Even a DROPDOWNLIST has
        // typeahead, which moves the selection, so a text read would push a
        // key the user never chose; and an unconditional CB_SETCURSEL
        // raises CBN_SELCHANGE on every push.
        let want = v.key.map(|i| i as i32).unwrap_or(-1);
        if cur_sel_raw(combo) != want {
            SendMessageW(combo, CB_SETCURSEL, Some(WPARAM(want as usize)), Some(LPARAM(0)));
        }
```

`CB_SETCURSEL` with `-1` clears the selection, which is what an unparseable
or absent combo must show. The `None` arm of `apply_state` clears all five
the same way.

- [ ] **Step 4: Commands**

Replace the `(IDC_COMBO, EN_CHANGE)` and `(IDC_COMBO, EN_KILLFOCUS)` arms
with one arm covering all five controls: on `BN_CLICKED` from any check box,
or `CBN_SELCHANGE` from the list, rebuild the canonical string and send it
through the existing `on_edit_combo`.

```rust
        (IDC_MOD_CTRL, _) | (IDC_MOD_WIN, _) | (IDC_MOD_ALT, _) | (IDC_MOD_SHIFT, _)
        | (IDC_COMBO, _) if !suppressed() => { ... }
```

Build the string in canonical order — `ctrl`, `super`, `alt`, `shift`, key —
and send **nothing at all** when no key is selected: a modifier set with no
main key is not a combo, and writing `"ctrl+"` into the model would make the
row invalid on a keystroke the user has not finished. The row keeps whatever
it had until a key is chosen.

- [ ] **Step 5: Update the probe**

`examples/settings_probe.rs` hard-codes 1002 and drives it as an `EDIT`:
`ctl_text` on it, `fmt_wh` labelled `EDIT IDC_COMBO`, and `drive_an_edit`
typing into it. Update all of it:

- the geometry line reads `COMBOBOX IDC_COMBO` and reports `CB_GETCOUNT`,
  which must be 81;
- reading the shortcut back means `CB_GETCURSEL` plus `CB_GETLBTEXT`, not
  `WM_GETTEXT`;
- `drive_an_edit`'s shortcut half selects an item rather than typing.

Do not weaken any existing assertion to make it pass. If a check cannot be
expressed against a list, say so in the report rather than deleting it.

- [ ] **Step 6: Gates, self-review, commit**

Run all five gates. Then read your own diff against the ABORT-class rule and
against "did I change when `layout` runs", and say in your report what you
checked.

```bash
git add crates/beckon-windows/src/settings_window.rs crates/beckon-windows/examples/settings_probe.rs
git commit -m "feat(windows): the shortcut is four check boxes and a key list

Spec C.4's typed path, which it calls primary: the free-text Shortcut EDIT
becomes four modifier check boxes plus a closed list of the 81 key names.
They make an invalid combo unrepresentable, they are the only path that
works for someone who cannot physically produce a chord, and a
DROPDOWNLIST has no edit field for a resize to corrupt.

IDC_COMBO keeps its number and changes class, so the id settings_probe pins
still names the shortcut control. The probe drives a list now, not an edit.

No mnemonics on the four boxes: Hold already claimed t, w and l, and the
table in mod cap is the only guard against a collision."
```

---

## Self-Review

**Spec coverage:** §C.4's four check boxes and `CBS_DROPDOWNLIST` are Task 2
steps 1-4; "the two views must never both write at once" is the read-guard in
step 3; `DROPDOWNLIST` rather than `DROPDOWN` is step 1, and its reason —
no edit field, so §B.7's resize defect is structurally impossible — is in the
commit message.

**Deliberately NOT here:** the `Record` and `Reset` buttons and everything
that arms the hook (§F.1-F.3) are 2b-v; the availability probe is 2b-iv. This
landing leaves the typed path complete and working on its own, which is what
makes capture an accelerator rather than the only way in.

**Type consistency:** `ComboView::key` is `Option<usize>`, an index into
`key_table()`, and `CB_SETCURSEL` takes the same index — which only holds
because the list is filled from `key_table()` in order. Task 2 step 1 says so.
