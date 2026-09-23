# Auto-save and Undo (phase 3) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Every valid change to the shortcut table lands in `apps.toml` on its
own, with a bounded Undo beside a saved readout, and with the safety net that
replaces the Save button strung *before* the button comes down.

**Architecture:** The whole decision layer is pure code in `beckon-core` —
when a write may happen, when it must be held, what the footer says, and what
Undo restores. `beckon-cli`'s `serve.rs` owns the one write path
(`write_config_text`) and the driver that runs the plan after every edit.
`beckon-macos` consumes it and loses its command bar in the LAST task, so the
tree is shippable at every commit before that.

**Tech Stack:** Rust; `toml_edit` for the in-place render; objc2 / AppKit on
the macOS side; no new dependencies.

**Spec:** `docs/superpowers/specs/2026-09-22-macos-ui-redesign-design.md` §5.5,
which delegates the design to
`docs/superpowers/specs/2026-08-14-four-doors-settings-window-design.md` §6.
Read §6 in full before Task 1: it is the authority, it is written for Windows,
and this plan is its macOS-first, core-shaped reading.

## Global Constraints

- **The net goes up before the button comes down.** `Save`, `Close` and
  `Open config file` stay on screen until Task 11. Every task before it leaves
  a tree that still works the way `main` does.
- **`beckon_core` owns every decision. A window owns none.** If a task finds
  itself writing a policy branch in `beckon-macos`, the branch belongs in
  `settings.rs` with a test.
- **Every write goes through `write_config_text`** (`crates/beckon-cli/src/serve.rs:829`),
  which canonicalises the path before renaming onto it.
  `saving_through_a_symlink_writes_the_target_and_keeps_the_link`
  (`serve.rs:3075`) and `saving_a_plain_file_still_replaces_it_in_place`
  (`serve.rs:3115`) must stay green in every task. That is the spec's named
  probe for this phase.
- **Never point a running build at a real config.** Live checks use a
  throwaway copy. The author's own config is reached through a two-hop
  symlink chain that ends inside a git repo
  (`~/.config/beckon/apps.toml` -> a read-only nix store path ->
  `~/.nix/configs/shortcuts/launch-app.toml`), so a stray write dirties their
  dotfiles. Implementers do not run the GUI at all; the controller does, and
  against `/tmp/beckon-p3/apps.toml`.
- **`beckon-windows` is untouched** except for the one call-site change
  Task 5 names. Windows keeps its command bar and its own Save.
- **ASCII in every user-visible string** this branch adds or edits.
- **No `#[allow(dead_code)]`** added to silence a gate. The four blanket
  allows on `settings_window`'s modules were removed in phase 2; they do not
  come back.
- **Do not add objc2 / objc2-app-kit features to `Cargo.toml`.** Every class
  feature is on by default; the explicit list is decorative.
- **Borrow discipline:** no `RefCell` borrow (`UI`, `CB`, `ServeState`) held
  across an AppKit call that can re-enter. A panic inside an objc method
  aborts the daemon.
- **Gate after every commit**, unfiltered, with
  `CARGO_TARGET_DIR=/Users/kln/Documents/dev/beckon/target`:
  `cargo +stable fmt --all -- --check`,
  `cargo +stable clippy --workspace --all-targets -- -D warnings`,
  `cargo +stable clippy --target aarch64-pc-windows-msvc --all-targets -- -D warnings`,
  `cargo +stable test --workspace`.
- **Use `git commit --only <file>`**, never `git add` + `git commit`.

## What §6's guards cost, and which ones this plan builds

§6 lists eleven guards, G-a to G-k. They are preconditions, not polish — the
spec's own verdict is *"what is lost with Save is not a button, it is a safety
net"*. Their disposition here, decided before execution and not open for an
implementer to re-rule:

| guard | disposition |
|---|---|
| G-a compare-and-swap before every write | Task 3 (core decision), Task 7 (driver) |
| G-b reseed selection by identity, not index | Task 4 |
| G-c retyped chord is an in-place key rename | Task 1 |
| G-d do not inject `keyboard.caps` / `caps_tap` | **ALREADY DONE** — `config_write.rs:141-142` computes `write_caps`/`write_tap` from `spelled(..) \|\| value != default`, and its comment already names the nix-config-in-git case this guard was written for. Task 1 adds a regression test and nothing else. Do not rebuild it. |
| G-e hold the write while the selected row's app went valid -> missing | Task 3 |
| G-f confirm Remove for more than one row, or under a filter | Task 4 (core predicate), Task 9 (macOS dialog) |
| G-g bounded Undo stack pushed before every write | Task 2 |
| G-h `<config>.bak` written at window open | Task 7 |
| G-i `WM_QUERYENDSESSION` on the flush list | **OUT OF SCOPE, deliberately.** It is a Win32 message, and Windows does not auto-save in this phase — it keeps its Save button. Revisit when Windows consumes this layer. Task 12 records the reason. |
| G-j one prompt: refuse close when dirty AND the last write failed | Task 10 |
| G-k carry filter, ticks and selection across an external reload | Task 4 |

§6.3's shipping bug — `Remove` under a filter deleting the whole config
because `Model::visible` matched the combo as well as the app — **is already
fixed**: `visible` (`settings.rs`) now matches `r.app` only. Task 4 adds the
regression test that pins it, because auto-save is what made it fatal.

## Two decisions this plan makes that §6 leaves open

- **Undo does not push.** The stack is popped by Undo and pushed by every
  other write. Undo writing its own restore back onto the stack would make
  the control a two-state toggle, which is not what "bounded stack" means.
  Redo is out of scope and is not built.
- **An external change clears the Undo stack.** Every entry is a whole file
  text whose base is the model's `original`. Once someone else has edited the
  file, restoring an entry would clobber their edit — the exact loss G-a
  exists to prevent, arriving through the Undo button instead of through a
  stale write. Clearing is the only answer that does not need a merge.

## File structure

| file | responsibility in this phase |
|---|---|
| `crates/beckon-core/src/config_write.rs` | render; gains the in-place key rename (G-c) |
| `crates/beckon-core/src/settings.rs` | `Model`'s undo history; `autosave_plan`; `SavedReadout`; `remove_needs_confirm`; `command_bar_shown` platform split; reload-preserving reseed |
| `crates/beckon-cli/src/serve.rs` | the driver: runs the plan after every edit, owns `write_config_text`, the `.bak`, and the debounce deadline |
| `crates/beckon-macos/src/settings_window/mod.rs` | the App field's per-keystroke flush, the footer readout and Undo control, the Remove confirm, the close guard, and (last) the command bar's removal |
| `docs/notes/settings-window.md` | what replaced Save, and the guards a reader must not "simplify" |

---

### Task 1: A retyped chord is a key rename, not a delete and an append (G-c)

**Files:**
- Modify: `crates/beckon-core/src/config_write.rs:24-90` (the `render` function's steps 1 and 2)
- Test: `crates/beckon-core/src/config_write.rs` (the existing `mod tests` at the foot of the file)

**Interfaces:**
- Consumes: nothing from earlier tasks.
- Produces: no signature change. `render(original, rows, keyboard)` keeps its
  shape; only what it does to a row whose `combo` differs from its `orig_key`
  changes.

**Why this is first.** Every later task writes the file more often. Today a
retyped chord is a remove-plus-insert — `render`'s own step-1 comment says so
— which destroys the line's trailing comment and moves the binding to the
bottom of the file. At Save-per-session that is a once-a-day annoyance. Under
auto-save it happens while the user is still typing the chord, and the row
they are editing jumps out from under them.

- [ ] **Step 1: Write the failing test**

Add to `config_write.rs`'s `mod tests`:

```rust
#[test]
fn retyping_a_chord_renames_the_key_in_place_and_keeps_its_comment() {
    let original = "\
\"ctrl+alt+a\" = \"Anki\"  # the flashcards one
\"ctrl+alt+b\" = \"Brave\"
";
    let rows = vec![
        RowWrite {
            orig_key: Some("ctrl+alt+a".into()),
            combo: "ctrl+alt+z".into(),
            app: "Anki".into(),
        },
        RowWrite {
            orig_key: Some("ctrl+alt+b".into()),
            combo: "ctrl+alt+b".into(),
            app: "Brave".into(),
        },
    ];
    let out = render(original, &rows, &KeyboardConfig::default()).unwrap();

    // The renamed binding keeps its place: first line, not appended last.
    let first = out.lines().next().unwrap();
    assert!(
        first.starts_with("\"ctrl+alt+z\""),
        "the renamed key should still be the first line, got: {first}"
    );
    // And it keeps the trailing comment that sat on that line.
    assert!(
        first.contains("# the flashcards one"),
        "the trailing comment should survive a rename, got: {first}"
    );
    // The old spelling is gone.
    assert!(!out.contains("ctrl+alt+a"), "old key still present:\n{out}");
}
```

- [ ] **Step 2: Run it and watch it fail**

Run: `cargo +stable test -p beckon-core retyping_a_chord_renames_the_key_in_place -- --nocapture`

Expected: FAIL. Today the first line is `"ctrl+alt+b" = "Brave"` and
`"ctrl+alt+z"` is appended at the end with no comment, because step 1 drops
every key whose row no longer spells it and step 2 inserts the new spelling
fresh.

- [ ] **Step 3: Rename in place before anything is dropped**

In `render`, **before** the existing step 1, add a rename pass. `toml_edit`'s
`Table` preserves insertion order and exposes `get_key_value_mut`; the
position-preserving move is to rename the key while keeping the item and its
decor. Insert this as a new step 1, and renumber the existing comments:

```rust
    // 1. Rename, before anything is dropped. A row whose `combo` differs
    //    from its `orig_key` is the SAME binding retyped, not a removal
    //    plus an addition -- four-doors design G-c. Dropping and
    //    re-inserting it destroyed the line's trailing comment and moved
    //    the binding to the end of the file, which under auto-save happens
    //    while the user is still typing the chord.
    //
    //    Only a row whose old key is really there and whose new key is not
    //    is renamed; anything else falls through to the drop-and-insert
    //    path below, which is still correct for a genuine add or remove.
    for r in rows {
        let Some(old) = r.orig_key.as_deref() else {
            continue;
        };
        if old == r.combo {
            continue;
        }
        if doc.get(old).is_none() || doc.get(r.combo.as_str()).is_some() {
            continue;
        }
        let Some(table) = doc.as_table_mut().get_mut(old) else {
            continue;
        };
        let item = std::mem::replace(table, toml_edit::Item::None);
        // `insert` on the document's table appends; to keep the binding's
        // position the key itself is renamed through the table's key API.
        doc.as_table_mut().insert(r.combo.as_str(), item);
        if let Some((mut k, _)) = doc.as_table_mut().get_key_value_mut(r.combo.as_str()) {
            let _ = &mut k;
        }
        doc.as_table_mut().remove(old);
    }
```

**Read this before writing it.** The block above is the SHAPE, not a
guarantee: `toml_edit` 0.22's exact accessor for renaming a key while keeping
its position and decor is what you must verify against the crate's own docs
and source in the vendored registry. If a direct key rename exists, use it —
that is the whole point of the guard. If it does not, achieve the same
observable result (position kept, decor kept) and say in the report exactly
which API you used and why. **The test is the contract; the code above is a
starting point.**

- [ ] **Step 4: Run the test and the two symlink tests**

Run:
```
cargo +stable test -p beckon-core config_write
cargo +stable test -p beckon-cli symlink
```
Expected: the new test passes, every existing `config_write` test still
passes — in particular `comments_and_spelling_survive_an_unrelated_edit`
(`config_write.rs:231`) and `round_trip_preserves_meaning_and_is_idempotent`
(`config_write.rs:556`) — and both symlink tests stay green.

- [ ] **Step 5: Add the G-d regression test**

G-d is already implemented. Pin it so a later task cannot undo it:

```rust
#[test]
fn a_file_that_never_spelled_caps_does_not_gain_it() {
    let original = "\"ctrl+alt+a\" = \"Anki\"\n";
    let rows = vec![RowWrite {
        orig_key: Some("ctrl+alt+a".into()),
        combo: "ctrl+alt+a".into(),
        app: "Anki".into(),
    }];
    // The model holds defaults, and the file never mentioned `keyboard`.
    let out = render(original, &rows, &KeyboardConfig::default()).unwrap();
    assert!(
        !out.contains("keyboard"),
        "a default-valued keyboard block must not be injected:\n{out}"
    );
}
```

Run: `cargo +stable test -p beckon-core a_file_that_never_spelled_caps`
Expected: PASS on the first run — this is a pin, not a fix. **If it fails,
stop and report**: G-d is not where this plan says it is, and Task 3's
assumptions change.

- [ ] **Step 6: Gate and commit**

```bash
cargo +stable fmt --all -- --check
cargo +stable clippy --workspace --all-targets -- -D warnings
cargo +stable clippy --target aarch64-pc-windows-msvc --all-targets -- -D warnings
cargo +stable test --workspace
git commit --only crates/beckon-core/src/config_write.rs -m "fix(core): a retyped chord is a key rename, not a delete and an append (G-c)"
```

---

### Task 2: A bounded Undo history on the model (G-g)

**Files:**
- Modify: `crates/beckon-core/src/settings.rs` (the `Model` struct at :27 and its `impl`)
- Test: `crates/beckon-core/src/settings.rs`'s `mod tests`

**Interfaces:**
- Consumes: nothing.
- Produces, all on `Model`:
  - `pub const UNDO_DEPTH: usize = 20;`
  - `pub fn push_undo(&mut self, text: String)` — records the file text as it
    was BEFORE a write. Drops the oldest entry past `UNDO_DEPTH`.
  - `pub fn can_undo(&self) -> bool`
  - `pub fn take_undo(&mut self) -> Option<String>` — pops the most recent
    entry and returns it. Does NOT push anything.
  - `pub fn clear_undo(&mut self)`

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn undo_returns_the_text_from_before_the_last_write() {
    let mut m = Model::from_text("\"ctrl+alt+a\" = \"Anki\"\n").unwrap();
    m.push_undo("first".into());
    m.push_undo("second".into());
    assert!(m.can_undo());
    assert_eq!(m.take_undo().as_deref(), Some("second"));
    assert_eq!(m.take_undo().as_deref(), Some("first"));
    assert_eq!(m.take_undo(), None);
    assert!(!m.can_undo());
}

#[test]
fn the_undo_stack_is_bounded_and_drops_the_oldest() {
    let mut m = Model::from_text("").unwrap();
    for i in 0..(Model::UNDO_DEPTH + 5) {
        m.push_undo(format!("{i}"));
    }
    // The newest survives.
    assert_eq!(
        m.take_undo().as_deref(),
        Some(format!("{}", Model::UNDO_DEPTH + 4).as_str())
    );
    // Exactly UNDO_DEPTH entries were kept, so the oldest five are gone.
    let mut left = 1; // one already taken
    while m.take_undo().is_some() {
        left += 1;
    }
    assert_eq!(left, Model::UNDO_DEPTH);
}

#[test]
fn clearing_the_undo_stack_leaves_nothing_to_restore() {
    let mut m = Model::from_text("").unwrap();
    m.push_undo("a".into());
    m.clear_undo();
    assert!(!m.can_undo());
    assert_eq!(m.take_undo(), None);
}
```

- [ ] **Step 2: Run them and watch them fail**

Run: `cargo +stable test -p beckon-core undo`
Expected: FAIL to compile — `UNDO_DEPTH`, `push_undo`, `can_undo`,
`take_undo` and `clear_undo` do not exist.

- [ ] **Step 3: Implement**

Add the field to `Model` beside `original`:

```rust
    /// File texts from before each of the last `UNDO_DEPTH` writes, oldest
    /// first. Pushed by the driver before every write and popped by Undo.
    ///
    /// **Undo does not push.** Popping and then recording the restore would
    /// make the control a two-state toggle rather than a stack. Redo is
    /// deliberately not built.
    ///
    /// **An external change clears this.** Every entry's base is this
    /// model's `original`; once the file has moved under us, restoring one
    /// would clobber whatever the other writer did -- the loss the
    /// compare-and-swap guard exists to prevent, arriving through the Undo
    /// button instead of through a stale write.
    undo: Vec<String>,
```

and the methods:

```rust
impl Model {
    /// How many writes back Undo can reach.
    ///
    /// Chosen, not measured: deep enough that a session of editing stays
    /// recoverable, shallow enough that the whole history is a handful of
    /// file-sized strings.
    pub const UNDO_DEPTH: usize = 20;

    pub fn push_undo(&mut self, text: String) {
        self.undo.push(text);
        if self.undo.len() > Self::UNDO_DEPTH {
            self.undo.remove(0);
        }
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn take_undo(&mut self) -> Option<String> {
        self.undo.pop()
    }

    pub fn clear_undo(&mut self) {
        self.undo.clear();
    }
}
```

Initialise `undo: Vec::new()` in `from_text`.

- [ ] **Step 4: Run and watch them pass**

Run: `cargo +stable test -p beckon-core undo`
Expected: 3 passed.

- [ ] **Step 5: Gate and commit**

```bash
git commit --only crates/beckon-core/src/settings.rs -m "feat(core): a bounded undo history on the model (G-g)"
```

---

### Task 3: `autosave_plan` — the decision that replaces the Save button

**Files:**
- Modify: `crates/beckon-core/src/settings.rs`
- Test: `crates/beckon-core/src/settings.rs`'s `mod tests`

**Interfaces:**
- Consumes: `Model::dirty()`, `Model::render()`, `Model::problems()`,
  `Model` `original` (via a new accessor if one is needed — add
  `pub fn original(&self) -> &str` if it is not already public).
- Produces:

```rust
/// Why a valid-looking edit was not written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotSaved {
    /// The model does not render -- a half-typed row, a duplicate chord.
    FinishTheRow,
    /// The file on disk is no longer the text this model was loaded from.
    FileMoved,
    /// The selected row's app was resolvable and now matches nothing.
    /// Writing would put a broken hotkey live mid-word.
    AppWentMissing,
    /// `write_config_text` returned an error.
    CannotWrite,
}

/// What the driver should do after an edit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AutosavePlan {
    /// Nothing changed since the last write.
    Nothing,
    /// Write this text, after pushing the current on-disk text onto the
    /// undo stack.
    Write(String),
    /// Do not write, and say why.
    Hold(NotSaved),
}

/// What the footer's right-hand side reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SavedReadout {
    /// The config does not parse: there is nothing to save and nothing to
    /// claim.
    Blank,
    /// `Saved just now`, with Undo offered when the stack is not empty.
    Saved { undo: bool },
    NotSaved(NotSaved),
}

pub fn autosave_plan(
    model: &Model,
    on_disk: &str,
    selected_app_missing: bool,
) -> AutosavePlan;

pub fn saved_readout(
    parsed: bool,
    last: Option<NotSaved>,
    can_undo: bool,
) -> SavedReadout;

/// The sentence the footer shows for each refusal. ASCII only.
pub fn not_saved_phrase(r: NotSaved) -> &'static str;
```

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn a_clean_model_plans_no_write() {
    let m = Model::from_text("\"ctrl+alt+a\" = \"Anki\"\n").unwrap();
    assert_eq!(
        autosave_plan(&m, m.original(), false),
        AutosavePlan::Nothing
    );
}

#[test]
fn an_edit_plans_the_rendered_text() {
    let mut m = Model::from_text("\"ctrl+alt+a\" = \"Anki\"\n").unwrap();
    m.selected = Some(0);
    m.set_app(0, "Brave");
    match autosave_plan(&m, m.original(), false) {
        AutosavePlan::Write(text) => assert!(text.contains("Brave"), "{text}"),
        other => panic!("expected a write, got {other:?}"),
    }
}

#[test]
fn a_file_that_moved_under_us_holds_the_write() {
    let mut m = Model::from_text("\"ctrl+alt+a\" = \"Anki\"\n").unwrap();
    m.set_app(0, "Brave");
    assert_eq!(
        autosave_plan(&m, "somebody else edited this\n", false),
        AutosavePlan::Hold(NotSaved::FileMoved)
    );
}

#[test]
fn an_unrenderable_model_holds_the_write_and_says_to_finish_the_row() {
    let mut m = Model::from_text("\"ctrl+alt+a\" = \"Anki\"\n").unwrap();
    m.set_combo(0, "not a chord");
    assert_eq!(
        autosave_plan(&m, m.original(), false),
        AutosavePlan::Hold(NotSaved::FinishTheRow)
    );
}

#[test]
fn a_selected_row_whose_app_went_missing_holds_the_write() {
    let mut m = Model::from_text("\"ctrl+alt+a\" = \"Anki\"\n").unwrap();
    m.selected = Some(0);
    m.set_app(0, "B");
    assert_eq!(
        autosave_plan(&m, m.original(), true),
        AutosavePlan::Hold(NotSaved::AppWentMissing)
    );
}

#[test]
fn the_readout_is_blank_when_the_config_does_not_parse() {
    assert_eq!(saved_readout(false, None, true), SavedReadout::Blank);
}

#[test]
fn the_readout_offers_undo_only_when_there_is_something_to_undo() {
    assert_eq!(
        saved_readout(true, None, true),
        SavedReadout::Saved { undo: true }
    );
    assert_eq!(
        saved_readout(true, None, false),
        SavedReadout::Saved { undo: false }
    );
}

#[test]
fn a_refusal_outranks_the_saved_readout() {
    assert_eq!(
        saved_readout(true, Some(NotSaved::CannotWrite), true),
        SavedReadout::NotSaved(NotSaved::CannotWrite)
    );
}

#[test]
fn every_refusal_phrase_is_ascii_and_says_what_to_do() {
    for r in [
        NotSaved::FinishTheRow,
        NotSaved::FileMoved,
        NotSaved::AppWentMissing,
        NotSaved::CannotWrite,
    ] {
        let s = not_saved_phrase(r);
        assert!(s.is_ascii(), "{r:?} phrase is not ASCII: {s}");
        assert!(s.starts_with("Not saved"), "{r:?}: {s}");
    }
}
```

- [ ] **Step 2: Run and watch them fail**

Run: `cargo +stable test -p beckon-core autosave`
Expected: FAIL to compile — none of the types exist.

- [ ] **Step 3: Implement**

The order of the checks is the design and must not be rearranged:
**moved-under-us first** (writing at all would be wrong), then
**renderable** (nothing to write), then **the app went missing** (writing
would put a broken hotkey live). Write it so the order is visible:

```rust
/// Decide what an edit should do to the file.
///
/// **The order of these checks is the design.** `FileMoved` is first
/// because when the base is stale no amount of validity makes the write
/// safe. `FinishTheRow` is next because an unrenderable model has nothing
/// to offer the file. `AppWentMissing` is last because it is the only one
/// that refuses a write that WOULD have succeeded -- it is a judgement
/// about the live hotkey, not about the text.
pub fn autosave_plan(model: &Model, on_disk: &str, selected_app_missing: bool) -> AutosavePlan {
    if !model.dirty() {
        return AutosavePlan::Nothing;
    }
    if on_disk != model.original() {
        return AutosavePlan::Hold(NotSaved::FileMoved);
    }
    let Ok(text) = model.render() else {
        return AutosavePlan::Hold(NotSaved::FinishTheRow);
    };
    if selected_app_missing {
        return AutosavePlan::Hold(NotSaved::AppWentMissing);
    }
    AutosavePlan::Write(text)
}

pub fn saved_readout(parsed: bool, last: Option<NotSaved>, can_undo: bool) -> SavedReadout {
    if !parsed {
        return SavedReadout::Blank;
    }
    match last {
        Some(r) => SavedReadout::NotSaved(r),
        None => SavedReadout::Saved { undo: can_undo },
    }
}

pub fn not_saved_phrase(r: NotSaved) -> &'static str {
    match r {
        NotSaved::FinishTheRow => "Not saved - finish the row below",
        NotSaved::FileMoved => "Not saved - the file changed on disk",
        NotSaved::AppWentMissing => "Not saved - that app name matches nothing",
        NotSaved::CannotWrite => "Not saved - cannot write the file",
    }
}
```

If `Model::original()` is not already public, add it:

```rust
    /// The file text this model was loaded from. The compare-and-swap
    /// guard's base.
    pub fn original(&self) -> &str {
        &self.original
    }
```

- [ ] **Step 4: Run and watch them pass**

Run: `cargo +stable test -p beckon-core autosave`
Expected: 9 passed.

- [ ] **Step 5: Gate and commit**

```bash
git commit --only crates/beckon-core/src/settings.rs -m "feat(core): autosave_plan decides when a write may happen (G-a, G-e)"
```

---

### Task 4: The three guards that protect the view, not the file (G-b, G-f, G-k)

**Files:**
- Modify: `crates/beckon-core/src/settings.rs`
- Test: `crates/beckon-core/src/settings.rs`'s `mod tests`

**Interfaces:**
- Produces:

```rust
/// Does pressing Remove need a confirmation first?
///
/// Auto-save is what makes this load-bearing: without a Save to gate it,
/// a mis-aimed multi-row delete reaches the file immediately.
pub fn remove_needs_confirm(marked: usize, filter_active: bool) -> bool;

/// The state a reseed must carry across a reload, so "the file wins" does
/// not also cost the user their view.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ViewState {
    pub filter: String,
    /// The `orig_key` of the selected row, not its index.
    pub selected_key: Option<String>,
    /// The `orig_key` of every ticked row.
    pub marked_keys: Vec<String>,
}

impl Model {
    /// Capture what the user is looking at, by identity.
    pub fn view_state(&self) -> ViewState;
    /// Put it back after a reseed. Rows that no longer exist are dropped
    /// silently; that is the file winning, which is the point.
    pub fn restore_view_state(&mut self, v: &ViewState);
}
```

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn removing_one_unfiltered_row_needs_no_confirmation() {
    assert!(!remove_needs_confirm(1, false));
}

#[test]
fn removing_several_rows_needs_confirmation() {
    assert!(remove_needs_confirm(2, false));
}

#[test]
fn removing_under_a_filter_needs_confirmation_even_for_one_row() {
    assert!(remove_needs_confirm(1, true));
}

#[test]
fn the_filter_matches_the_app_column_only() {
    // Pins the fix for four-doors design 6.3: every beckon chord contains
    // `alt`, so a filter matching the combo showed everything while
    // looking filtered, and Remove then took the table.
    let mut m = Model::from_text(
        "\"ctrl+alt+a\" = \"Anki\"\n\"ctrl+alt+b\" = \"Brave\"\n",
    )
    .unwrap();
    m.set_filter("alt");
    let shown = m.control_state_rows_len_for_test();
    assert_eq!(shown, 0, "a filter matching only combos must show nothing");
}

#[test]
fn a_reseed_carries_the_filter_the_selection_and_the_ticks() {
    let text = "\"ctrl+alt+a\" = \"Anki\"\n\"ctrl+alt+b\" = \"Brave\"\n";
    let mut m = Model::from_text(text).unwrap();
    m.set_filter("br");
    m.selected = Some(1);
    m.set_marked(1, true);
    let v = m.view_state();

    // The file changed underneath and the model is reseeded from it.
    let mut fresh = Model::from_text(text).unwrap();
    fresh.restore_view_state(&v);

    assert_eq!(fresh.filter(), "br");
    assert_eq!(fresh.selected, Some(1));
    assert!(fresh.rows[1].marked);
}

#[test]
fn a_reseed_drops_view_state_for_rows_the_file_no_longer_has() {
    let mut m = Model::from_text(
        "\"ctrl+alt+a\" = \"Anki\"\n\"ctrl+alt+b\" = \"Brave\"\n",
    )
    .unwrap();
    m.selected = Some(1);
    m.set_marked(1, true);
    let v = m.view_state();

    // Somebody deleted the Brave binding by hand.
    let mut fresh = Model::from_text("\"ctrl+alt+a\" = \"Anki\"\n").unwrap();
    fresh.restore_view_state(&v);

    assert_eq!(fresh.selected, None, "the selection must not point at a stranger");
    assert!(!fresh.rows[0].marked);
}
```

**`control_state_rows_len_for_test` does not exist.** If `Model::visible` is
private and there is no public way to count shown rows, assert through
`control_state(..)`'s returned row list instead — read that function's real
signature and write the assertion against it. Do not add a test-only
accessor to production code; use what the contract already exposes.

- [ ] **Step 2: Run and watch them fail**

Run: `cargo +stable test -p beckon-core -- remove_needs_confirm view_state filter_matches`

- [ ] **Step 3: Implement**

```rust
/// Does pressing Remove need a confirmation first?
///
/// **Auto-save is what makes this load-bearing.** `remove_pressed`'s own
/// doc used to argue a silent multi-row delete was acceptable because the
/// effect is visible and Save is still a gate. Auto-save falsifies the
/// second half: the delete reaches the file before the user has looked.
///
/// The filter arm is the sharper one. Four-doors design 6.3 measured a
/// filter that matched the combo column showing every row while the window
/// looked filtered; the column bug is fixed, but "I am looking at a subset"
/// remains the state in which a multi-delete surprises somebody.
pub fn remove_needs_confirm(marked: usize, filter_active: bool) -> bool {
    marked > 1 || filter_active
}
```

```rust
impl Model {
    pub fn view_state(&self) -> ViewState {
        ViewState {
            filter: self.filter.clone(),
            selected_key: self
                .selected
                .and_then(|i| self.rows.get(i))
                .and_then(|r| r.orig_key.clone()),
            marked_keys: self
                .rows
                .iter()
                .filter(|r| r.marked)
                .filter_map(|r| r.orig_key.clone())
                .collect(),
        }
    }

    /// **By identity, never by index** -- four-doors design G-b. A write
    /// can reorder the file, and a raw index then points the editor at a
    /// different binding while the user is still typing into it.
    pub fn restore_view_state(&mut self, v: &ViewState) {
        self.set_filter(&v.filter);
        self.selected = v.selected_key.as_ref().and_then(|k| {
            self.rows
                .iter()
                .position(|r| r.orig_key.as_deref() == Some(k.as_str()))
        });
        for r in self.rows.iter_mut() {
            r.marked = r
                .orig_key
                .as_deref()
                .is_some_and(|k| v.marked_keys.iter().any(|m| m == k));
        }
    }
}
```

- [ ] **Step 4: Run and watch them pass**

Run: `cargo +stable test -p beckon-core`
Expected: every new test passes and the 458 existing `beckon-core` tests
still pass.

- [ ] **Step 5: Gate and commit**

```bash
git commit --only crates/beckon-core/src/settings.rs -m "feat(core): confirm a risky Remove, and carry the view across a reseed (G-b, G-f, G-k)"
```

---

### Task 5: `command_bar_shown` becomes platform-aware

**Files:**
- Modify: `crates/beckon-core/src/settings.rs` (`command_bar_shown`, around :730)
- Modify: `crates/beckon-windows/src/settings_window/mod.rs` — **call sites only**, at `:833`, `:3034`, `:10149`, and `crates/beckon-windows/src/settings_window/layout.rs:1968`
- Test: `crates/beckon-core/src/settings.rs`'s `mod tests`

**Interfaces:**
- Produces:

```rust
/// Which platform's command bar is being asked about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bar {
    /// Windows still gates its writes behind Save.
    Buttons,
    /// macOS auto-saves; the bar holds a readout and Undo instead.
    Readout,
}

pub fn command_bar_shown(page: Page, bar: Bar) -> bool;
```

**This task changes a shared signature and therefore touches Windows.** That
is the ONE permitted `beckon-windows` edit in this branch: each call site
passes `Bar::Buttons` and nothing else about Windows changes. Do not alter
Windows behaviour, layout or strings.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn the_button_row_is_windows_only_now() {
    for p in [Page::Shortcuts, Page::Keyboard, Page::System, Page::About] {
        assert!(
            !command_bar_shown(p, Bar::Readout),
            "{p:?}: macOS auto-saves and shows no Save button"
        );
    }
    // Windows is unchanged: the buttons follow the page that writes.
    assert!(command_bar_shown(Page::Shortcuts, Bar::Buttons));
    assert!(!command_bar_shown(Page::About, Bar::Buttons));
}
```

- [ ] **Step 2: Run and watch it fail**

Run: `cargo +stable test -p beckon-core the_button_row_is_windows_only_now`
Expected: FAIL to compile — `command_bar_shown` takes one argument.

- [ ] **Step 3: Implement**

```rust
pub fn command_bar_shown(page: Page, bar: Bar) -> bool {
    match bar {
        // **macOS lost the row, not the band.** Auto-save replaced the
        // three buttons with a readout and Undo (design 5.5); the band
        // itself still carries the service line on all four doors.
        Bar::Readout => false,
        Bar::Buttons => page.writes_config(),
    }
}
```

- [ ] **Step 4: Update the four Windows call sites**

Each becomes `command_bar_shown(page, Bar::Buttons)`. Nothing else changes.
Verify with the cross-target clippy leg, which is the only thing that
compiles `beckon-windows` on this machine.

- [ ] **Step 5: Gate and commit**

Run the Windows clippy leg explicitly and confirm it really compiled — a leg
that returns in under a second answered from cache and has told you nothing:

```
touch crates/beckon-windows/src/settings_window/mod.rs
cargo +stable clippy --target aarch64-pc-windows-msvc --all-targets -- -D warnings
```

```bash
git commit --only crates/beckon-core/src/settings.rs crates/beckon-windows/src/settings_window/mod.rs crates/beckon-windows/src/settings_window/layout.rs -m "refactor(core): command_bar_shown answers per platform"
```

---

### Task 6: The App field stops losing what was typed

**Files:**
- Modify: `crates/beckon-macos/src/settings_window/mod.rs` — the `Target`
  class's `NSControlTextEditingDelegate` impl at `:795` (an EMPTY impl
  today), and the App `NSComboBox` built at `:1895`
- Test: none possible — this is AppKit wiring. The controller verifies.

**Interfaces:**
- Consumes: `Model::set_app` through the existing `edit!` path.
- Produces: `controlTextDidChange:` calls the same callback
  `beckonApp:` already calls.

**Why this task exists, and why it is before the button is removed.**
`commit_fields()` (`mod.rs:1373`) flushes the combo box's typed text into the
model, and it is called from exactly one place: `beckonSave:`. The App field's
action fires on Enter or on picking from the list, not per keystroke. So today
the Save button is what rescues text that was typed and never committed.
**Remove Save with this unchanged and every partially-typed app name is lost.**

- [ ] **Step 1: Implement the delegate method**

`unsafe impl NSControlTextEditingDelegate for Target {}` is already declared
and empty, so the combo box's delegate wiring exists. Add the one method:

```rust
        /// Every keystroke in the App field reaches the model.
        ///
        /// **This is what replaces `commit_fields`.** That function is
        /// called from `beckonSave:` alone, so before auto-save the Save
        /// button was the thing that rescued text typed and never
        /// committed -- the combo box's own action fires on Enter or on a
        /// list pick, not per keystroke. With no Save there is no later
        /// rescue, so the keystroke is the commit.
        #[unsafe(method(controlTextDidChange:))]
        fn control_text_did_change(&self, notification: &NSNotification) {
            // ... read the field's current string and raise the same
            // callback `beckonApp:` raises.
        }
```

Fill the body by reading how `beckonApp:` (`mod.rs:1083`) reaches
`m.set_app`, and raise the identical callback. Do not invent a second path
into the model.

- [ ] **Step 2: Add the debounce deadline**

Per four-doors §6.1.2 the App field is the ONE debounced input
(`AUTOSAVE_QUIET_MS = 600`, chosen not measured). On macOS there is no
`SetTimer`; the equivalent is a deadline the driver checks. Add the constant
in `beckon-core` beside the other settings constants so both platforms read
one number:

```rust
/// How long the App field stays quiet before its text is written.
///
/// Chosen, not measured. It is the only debounced input: every other
/// gesture in this window is discrete and flushes immediately.
pub const AUTOSAVE_QUIET_MS: u64 = 600;
```

and have the macOS side record "last typed at" on each
`controlTextDidChange:` rather than writing from inside it.

- [ ] **Step 3: Gate and commit**

```bash
git commit --only crates/beckon-macos/src/settings_window/mod.rs crates/beckon-core/src/settings.rs -m "feat(macos): every keystroke in the App field reaches the model"
```

**Report:** say explicitly whether `controlTextDidChange:` fires for the
combo box, or only for a plain `NSTextField`. If the combo box does not send
it, say so and stop — the driver's design changes, and that is a ruling for
the controller, not a thing to work around.

---

### Task 7: The driver — run the plan after every edit (G-a, G-h)

**Files:**
- Modify: `crates/beckon-cli/src/serve.rs` — the `edit!` macro at `:2596`,
  `apply_settings` at `:2383`, and the settings-window open path
- Test: `crates/beckon-cli/src/serve.rs`'s `mod tests`

**Interfaces:**
- Consumes: `autosave_plan`, `AutosavePlan`, `NotSaved`, `Model::push_undo`,
  `Model::view_state`, `Model::restore_view_state`, `write_config_text`.
- Produces: `fn autosave(st: &ServeState) -> Option<NotSaved>` — runs the
  plan, performs the write, records the failure if any. Called at the end of
  `edit!`.

- [ ] **Step 1: Write the backup**

G-h: `<config>.bak` written unconditionally when the settings window opens.
It is session-level rollback and it is the only net that survives a stale-base
clobber, which is precisely what G-a cannot catch.

```rust
/// Copy the config beside itself as `<name>.bak`, once, when the window
/// opens.
///
/// **This does not protect against the stale-base clobber.** The
/// compare-and-swap guard refuses a write whose base moved; this is the
/// net for everything else, including a session of edits the user wants
/// out of wholesale. Best effort: a failure is not worth refusing to open
/// the window over.
fn backup_config(path: &Path) {
    let target = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let bak = target.with_extension("toml.bak");
    let _ = std::fs::copy(&target, &bak);
}
```

Call it where the settings window is opened, before the model is built.

- [ ] **Step 2: Write the driver**

```rust
/// Run the auto-save plan after an edit.
///
/// **The read is immediately before the write** -- that pairing IS the
/// compare-and-swap guard, and splitting them widens the window in which
/// somebody else's edit can land between the check and the rename.
fn autosave(st: &ServeState) -> Option<NotSaved> {
    // Read the file as it is right now.
    // Run `autosave_plan` against it.
    // On `Write(text)`: push the on-disk text onto the undo stack FIRST,
    //   then `write_config_text`, then reseed the model from the text just
    //   written and restore the view state.
    // On `Hold(NotSaved::FileMoved)`: set `external_change = true` so the
    //   banner comes up, and clear the undo stack.
    // On `Hold(other)`: record it for the readout and write nothing.
    // On `Nothing`: clear the recorded failure and do nothing.
}
```

Write the body against the real `ServeState` shape. Two rules that are not
negotiable and that the report must state you followed:

1. **Push the undo entry before the write, never after.** An entry pushed
   after a write that failed halfway describes a file state that never
   existed.
2. **Reseed from the text that was written**, not by re-reading the file. A
   re-read invites the same race the guard just closed.

- [ ] **Step 3: Call it from `edit!`**

Every mutator already funnels through `edit!` (`serve.rs:2596`), which
mutates the model and calls `refresh_settings` without writing. Add the
`autosave` call at its end, so a new mutator cannot forget to save.

- [ ] **Step 4: Test the compare-and-swap end to end**

```rust
#[test]
fn a_write_is_abandoned_when_the_file_moved_under_us() {
    // Build a temp config, load a Model from it, edit the model, then
    // overwrite the file behind the model's back, then run the plan.
    // Expect Hold(FileMoved) and the file's on-disk text unchanged.
}
```

Write this against the real helpers the existing `serve.rs` tests use — the
two symlink tests at `:3075` and `:3115` show the temp-file pattern this
module already has. Assert on the FILE's bytes, not on the plan alone: the
whole point is that nothing was written.

- [ ] **Step 5: Gate and commit**

Both symlink tests must still pass.

```bash
git commit --only crates/beckon-cli/src/serve.rs -m "feat(serve): every edit runs the autosave plan (G-a, G-h)"
```

---

### Task 8: The footer says what happened, and offers Undo

**Files:**
- Modify: `crates/beckon-macos/src/settings_window/mod.rs` — the footer built
  around `:1987-1996`
- Modify: `crates/beckon-cli/src/serve.rs` — the `SettingsCommand::Undo` arm
  at `:2885`

**Interfaces:**
- Consumes: `saved_readout`, `SavedReadout`, `not_saved_phrase`,
  `Model::can_undo`, `Model::take_undo`.
- Produces: an `Undo` push button in the footer's right half, sending
  `SettingsCommand::Undo`; and a readout label beside it.

- [ ] **Step 1: Build the two views**

Right half of the command band, per four-doors §6.4: a readout label and an
`Undo` push button. The service line keeps the left half on all four doors.
Use the phase-2 widgets (`w::label`, `w::secondary`, `w::push`, `w::spring`)
and semantic NSColors.

- [ ] **Step 2: Make `SettingsCommand::Undo` do something**

At `serve.rs:2885` the arm is empty and shares a line with two other
no-op commands. Give it its own arm:

```rust
    SettingsCommand::Undo => {
        // Pop the previous file text and write it. Undo does NOT push:
        // popping and then recording the restore would make the control a
        // two-state toggle rather than a stack.
        // Reseed the model from what was written and restore the view.
    }
```

- [ ] **Step 3: Wire the readout**

`ControlState` gains the readout so the window draws a decision it did not
make. Add a field carrying `SavedReadout` and fill it in the same function
that fills `service`.

- [ ] **Step 4: Gate and commit**

```bash
git commit --only crates/beckon-macos/src/settings_window/mod.rs crates/beckon-cli/src/serve.rs crates/beckon-core/src/settings.rs -m "feat(macos): the footer says what happened and offers Undo"
```

---

### Task 9: Remove asks first when it should (G-f)

**Files:**
- Modify: `crates/beckon-macos/src/settings_window/mod.rs` — `beckonRemove:`
  at `:875`

**Interfaces:**
- Consumes: `remove_needs_confirm(marked, filter_active)`.

- [ ] **Step 1: Ask before removing**

When `remove_needs_confirm` answers true, raise an `NSAlert` naming the
count, in the shape `swin::ask_save` (`mod.rs:1660`) already uses. ASCII
strings. Proceed only on confirmation.

- [ ] **Step 2: Gate and commit**

```bash
git commit --only crates/beckon-macos/src/settings_window/mod.rs -m "feat(macos): a risky Remove asks first (G-f)"
```

---

### Task 10: One prompt, and only one (G-j)

**Files:**
- Modify: `crates/beckon-macos/src/settings_window/mod.rs` — `may_close`
  at `:1533`
- Modify: `crates/beckon-cli/src/serve.rs` — `on_close_request` at `:2752`

**Interfaces:**
- Consumes: the recorded last-failure from Task 7.

- [ ] **Step 1: Replace the dirty prompt with the failure prompt**

Today closing a dirty window raises `ask_save_changes`. Under auto-save a
dirty model is the normal state for 600 ms at a time, so that prompt would
fire constantly and be trained away. §6's G-j keeps **exactly one** prompt:
refuse the close when the model is dirty **and** the last write failed.
Everything else closes silently.

- [ ] **Step 2: Gate and commit**

```bash
git commit --only crates/beckon-macos/src/settings_window/mod.rs crates/beckon-cli/src/serve.rs -m "feat(macos): the only close prompt is a failed write (G-j)"
```

---

### Task 11: The button comes down

**Files:**
- Modify: `crates/beckon-macos/src/settings_window/mod.rs` — `show_page_sized`
  at `:662`, and the three views at `:1987-1989`

**This task is last and must not be moved earlier.** Every guard above is the
net; this is the removal of the old one.

- [ ] **Step 1: Stop building Save, Close and Open config file**

`command_bar_shown(page, Bar::Readout)` is now always false, so the three
views have no page that shows them. Delete them and their selectors rather
than leaving them hidden — a hidden button with a live selector is a control
that can still be reached by the responder chain.

**Check before deleting:** `Close` is one of the window's ways out. Phase 2's
window has the title bar's close button and Escape. Confirm both still work
and say so in the report; if Escape reached `beckonClose:` specifically,
re-point it at the window's own close.

- [ ] **Step 2: Gate and commit**

```bash
git commit --only crates/beckon-macos/src/settings_window/mod.rs -m "feat(macos): auto-save retires the command bar's buttons"
```

---

### Task 12: The notes

**Files:**
- Modify: `docs/notes/settings-window.md`

- [ ] **Step 1: Record what replaced Save**

In the note's house style — what was measured, on what, when, and what a
reader must therefore not do. Cover:

- The eleven guards and which ones this branch built, including **G-d, which
  was already done** and is now pinned by a test, and **G-i, which is out of
  scope because Windows still has its Save button**.
- **Undo does not push, and an external change clears the stack** — both with
  the reason, because both look like omissions.
- **`controlTextDidChange:` is what replaced `commit_fields`**, and why
  removing Save without it would have lost typed text.
- The compare-and-swap's read must stay immediately before the write.
- `<config>.bak` is session rollback and does NOT cover the stale-base case.

- [ ] **Step 2: Commit**

```bash
git commit --only docs/notes/settings-window.md -m "docs: what replaced the Save button"
```

---

## Self-review

**Spec coverage.** §5.5's five bullets: the core implementation (Tasks 1-5),
macOS losing Save/Close/Open (Task 11) with `command_bar_shown` made
platform-aware rather than changed for both (Task 5), the footer's service
line and the three readout states (Task 8), `write_config_text` staying the
one write path with the symlink test green (Global Constraints + Tasks 1, 7),
and close-while-incomplete plus the external-change banner following
four-doors §6 (Tasks 7 and 10). Four-doors §6's guards are dispositioned in
the table above, every one either built, already done, or out of scope with a
reason.

**Placeholders.** Tasks 6, 7, 8, 9, 10 and 11 give shape and rules rather
than complete bodies, because each is AppKit or `ServeState` wiring whose
surrounding code the implementer must read. Every one of them names the exact
file and line to start from and states the rules that must hold. Tasks 1-5,
which are pure and testable, carry complete code and complete tests. Task 1's
step 3 says outright that its block is a starting point and the test is the
contract.

**Type consistency.** `NotSaved`, `AutosavePlan`, `SavedReadout`, `Bar`,
`ViewState` and `AUTOSAVE_QUIET_MS` are defined once in Task 2/3/4/5/6 and
consumed under the same names in Tasks 7-11. `Model::original()` is the one
accessor added on demand, in Task 3.
