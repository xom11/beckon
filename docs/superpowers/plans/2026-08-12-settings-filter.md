# Settings-window filter box — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a filter box to the `beckon-serve` settings window, together with
the view-index-to-model-index mapping that stops it from ticking one binding
and deleting another.

**Architecture:** The filter is `Model` state in `beckon-core`, not window
state, because four decisions depend on which rows are visible
(`remove_pressed`, `remove_enabled`, `marked_count`, `selected`). `ListItem`
carries its model row; the window translates before calling any callback, so
`on_select` and `on_mark` keep receiving model indices. **Tasks 1–4 land the
mapping before Task 5 adds the control**, so no commit ever has a filter
without a mapping.

**Tech Stack:** Rust, `beckon-core` (pure, no platform deps), `beckon-windows`
(Win32 via `windows` 0.61), `beckon-cli` (`serve.rs` owns the callbacks).

**Spec:** `docs/superpowers/specs/2026-08-12-settings-filter-design.md`

## Global Constraints

- **ABORT-CLASS.** Never hold a `RefCell` borrow of `UI` (`settings_window.rs`)
  or `ServeState` (`serve.rs`) across any `SendMessageW` / `PostMessageW` /
  `SetWindowPos` / `SetFocus` / `SetWindowTextW`. A second borrow across an
  `extern "system"` boundary **aborts the process** rather than unwinding —
  the compiler, the tests and the cross-compile all miss it. Copy the value
  out, drop the borrow, then send.
- **`layout` is conditional** (`Ui::shown_external`, `Ui::shown_empty`) and
  that is what fixed the combo-box data loss. **Do not call `layout` on a data
  push.** A populated `CBS_DROPDOWN` re-synchronises its edit field and selects
  the whole string when it is **resized**.
- **`layout` has five inputs and the guard tracks four.** The fifth is the
  list's own client width; leave it unguarded — its error is always a gutter,
  never a clipped column.
- **Control ids 1002–1007 and the class name `BeckonSettingsWindow` are fixed
  points**, hard-coded in `crates/beckon-windows/examples/settings_probe.rs`.
  Do not renumber. New ids continue upward from 1020.
- **Push-button token filter:** Tab onto `Remove` must not delete a row.
- **Display strings and log lines are ASCII.** Windows PowerShell 5.1's
  `Get-Content` defaults to ANSI. Comments and test assertion messages may use
  anything.
- **Gates, run from the repo root on the mac:**
  - `cargo fmt --all -- --check` — **does** cover `cfg`-gated modules.
  - `cargo test -p beckon-core`
  - `cargo check --target x86_64-pc-windows-gnu -p beckon-windows` (WINCHECK:
    a cross-check that does not link and cannot see MSVC)
  - The only native Windows build is on a14 (`cargo build --all-targets`, **not**
    `--examples`, which skips `[[bin]]` targets).

---

## File Structure

| File | Responsibility | Tasks |
|---|---|---|
| `crates/beckon-core/src/settings.rs` | `Model.filter`, `visible()`, `ListItem.row`, visible-scoped remove semantics, and every test that pins them | 1, 2, 3 |
| `crates/beckon-windows/src/settings_window.rs` | View→model translation, the `IDC_FILTER` control, band-2 layout | 4, 5 |
| `crates/beckon-cli/src/serve.rs` | Wiring `on_filter`; `on_add` clears the filter | 5 |
| `docs/superpowers/measurements/2026-08-11-landing-1-a14.md` | The hardware gate results | 6 |

No new files. `settings.rs` is 1989 lines and `settings_window.rs` 3540; both
are the established home for this work and neither is split by this change.

---

## Task 1: The filter in the model

**Files:**
- Modify: `crates/beckon-core/src/settings.rs` — `struct Model` (~line 26),
  `Model::from_text` (~line 199), new methods after `dirty()` (~line 210)
- Test: same file, `mod tests`

**Interfaces:**
- Consumes: nothing.
- Produces: `Model::set_filter(&mut self, v: &str)`,
  `Model::filter(&self) -> &str`, and the private
  `Model::visible(&self) -> Vec<usize>` used by Tasks 2 and 3.

- [ ] **Step 1: Write the failing tests**

Add to `mod tests` in `crates/beckon-core/src/settings.rs`:

```rust
    // ---------- the filter ----------

    fn three() -> Model {
        Model::from_text(
            "\"ctrl+alt+a\"=\"Notepad\"\n\"ctrl+alt+b\"=\"Brave\"\n\"ctrl+alt+q\"=\"Weather\"\n",
        )
        .unwrap()
    }

    #[test]
    fn an_empty_filter_shows_every_row() {
        let m = three();
        assert_eq!(m.visible(), vec![0, 1, 2]);
    }

    #[test]
    fn the_filter_matches_the_app_name_case_insensitively() {
        let mut m = three();
        m.set_filter("BRA");
        assert_eq!(m.visible(), vec![1]);
    }

    #[test]
    fn the_filter_matches_the_combo_too() {
        let mut m = three();
        m.set_filter("alt+q");
        assert_eq!(
            m.visible(),
            vec![2],
            "the question this file is usually opened to answer is what a key \
             is already bound to"
        );
    }

    #[test]
    fn a_filter_matching_nothing_shows_no_rows() {
        let mut m = three();
        m.set_filter("zzz");
        assert!(m.visible().is_empty());
    }

    #[test]
    fn the_filter_is_trimmed_before_matching() {
        let mut m = three();
        m.set_filter("brave ");
        assert_eq!(
            m.visible(),
            vec![1],
            "a trailing space left by typing would otherwise hide every row, \
             which reads as a hang"
        );
    }

    #[test]
    fn setting_a_filter_is_not_a_file_change() {
        let mut m = three();
        let before = m.render().unwrap();
        m.set_filter("brave");
        assert!(!m.dirty(), "a filter changes nothing on disk");
        assert_eq!(m.render().unwrap(), before, "the filter is never written");
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p beckon-core --lib settings::tests 2>&1 | tail -20`
Expected: FAIL — `no method named 'set_filter'`, `no method named 'visible'`.

- [ ] **Step 3: Implement**

In `struct Model`, add the field after `dirty`:

```rust
    dirty: bool,
    /// The list filter. **View state**: never written to disk, never makes
    /// the model dirty. It lives here rather than in the window because
    /// `remove_pressed`, `remove_enabled`, `marked_count` and `selected` all
    /// depend on which rows are visible, and those decisions belong in the
    /// crate all three CI jobs compile.
    filter: String,
```

In `Model::from_text`, add to the returned literal:

```rust
            dirty: false,
            filter: String::new(),
```

Add these methods after `dirty()`:

```rust
    /// Set the list filter. Deliberately does NOT set `dirty`, for the same
    /// reason `set_marked` does not: `apply_enabled` is `dirty && valid`, so
    /// a filter that dirtied the model would light up Save and rewrite the
    /// file byte-identical.
    pub fn set_filter(&mut self, v: &str) {
        self.filter = v.to_string();
    }

    pub fn filter(&self) -> &str {
        &self.filter
    }

    /// The indices in `rows` the filter is showing, in model order.
    ///
    /// Matched case-insensitively against BOTH columns -- the app name and
    /// the combo -- which is the rule `beckon search` already uses, so the
    /// program has no third matching dialect.
    ///
    /// Trimmed first: a trailing space left by typing would otherwise match
    /// nothing and hide every row, which reads as a hang.
    ///
    /// Model order is a precondition of `remove_indices`, not a convenience.
    fn visible(&self) -> Vec<usize> {
        let f = self.filter.trim().to_lowercase();
        if f.is_empty() {
            return (0..self.rows.len()).collect();
        }
        self.rows
            .iter()
            .enumerate()
            .filter(|(_, r)| {
                r.app.to_lowercase().contains(&f) || r.combo.to_lowercase().contains(&f)
            })
            .map(|(i, _)| i)
            .collect()
    }
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p beckon-core --lib settings::tests 2>&1 | tail -5`
Expected: PASS, and no previously-green test turns red.

- [ ] **Step 5: Run the format gate**

Run: `cargo fmt --all -- --check`
Expected: exit 0.

- [ ] **Step 6: Commit**

```bash
git add crates/beckon-core/src/settings.rs
git commit -m "feat(core): the settings model gains a list filter

View state: never written, never dirties the model -- the same argument as
Row::marked. It lives in the model rather than the window because
remove_pressed, remove_enabled, marked_count and selected all depend on
which rows are visible, and those decisions belong where all three CI jobs
compile them. Nothing consumes it yet."
```

---

## Task 2: `ListItem` carries its model row

**Files:**
- Modify: `crates/beckon-core/src/settings.rs` — `struct ListItem` (~line 83),
  `control_state` (~line 623), `unreadable_state` (~line 809)
- Test: same file, `mod tests`

**Interfaces:**
- Consumes: `Model::visible()` from Task 1.
- Produces: `ListItem::row: usize` (model index) and a `ControlState::selected`
  that is a **view** index. Task 4 reads `items[i].row`.

- [ ] **Step 1: Write the failing tests**

```rust
    /// The defect this whole feature had to be designed around: with a
    /// filter active, the list's own index is NOT the model's, so a callback
    /// that passes it straight through ticks one binding and deletes
    /// another.
    #[test]
    fn list_items_carry_their_model_row_not_their_position() {
        let mut m = three();
        m.set_filter("weather");
        let cs = control_state(&m, &status_all_ok());
        assert_eq!(cs.items.len(), 1);
        assert_eq!(cs.items[0].row, 2, "Weather is row 2 of the model");
        assert_ne!(
            cs.items[0].row, 0,
            "if this ever passes by accident, the mapping has been dropped \
             and position is being used as the model index again"
        );
    }

    #[test]
    fn selected_is_a_view_index_while_filtered() {
        let mut m = three();
        m.selected = Some(2); // Weather, model row 2
        m.set_filter("e"); // Notepad, Brave and Weather all contain "e"
        let cs = control_state(&m, &status_all_ok());
        assert_eq!(cs.items.len(), 3);
        assert_eq!(cs.selected, Some(2));

        m.set_filter("weather"); // now Weather is the ONLY visible row
        let cs = control_state(&m, &status_all_ok());
        assert_eq!(
            cs.selected,
            Some(0),
            "ControlState::selected indexes `items`, which the filter has \
             shortened -- the ListView needs the line number, not the row"
        );
        assert_eq!(cs.detail.unwrap().app, "Weather");
    }

    #[test]
    fn selected_is_none_when_its_row_is_filtered_out() {
        let mut m = three();
        m.selected = Some(0); // Notepad
        m.set_filter("brave");
        let cs = control_state(&m, &status_all_ok());
        assert_eq!(cs.selected, None);
        assert!(
            cs.detail.is_none(),
            "the editor strip must not describe a row that is not on screen -- \
             and clearing the App field before `layout` runs is what keeps a \
             filter keystroke off the combo-box data-loss path"
        );
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p beckon-core --lib settings::tests 2>&1 | tail -20`
Expected: FAIL — `struct ListItem has no field named 'row'`.

- [ ] **Step 3: Implement**

Add the field to `ListItem`:

```rust
    /// Mirrors `Row::marked` -- the ListView sets this row's check state
    /// from it.
    pub marked: bool,
    /// This item's index in `Model.rows`. **Not** its position in `items`
    /// once a filter is active. Every callback that reaches the model must
    /// go through this: `on_select` and `on_mark` take model indices, and
    /// the ListView only ever knows the view index.
    pub row: usize,
```

Replace the loop in `control_state` (the `let mut items` block through the
closing brace of the `for`) with:

```rust
    let vis = m.visible();
    let mut items = Vec::with_capacity(vis.len());
    let mut detail = None;
    // The VIEW index of the selected row, which is what the ListView needs
    // in order to put `LVIS_SELECTED` back after a rebuild. `None` when the
    // filter is hiding the selected row -- see `ControlState::selected`.
    let mut selected = None;
    for (pos, &i) in vis.iter().enumerate() {
        let r = &m.rows[i];
        let (mark, flag, notes) = row_condition(m, i, rt, &problems);
        items.push(ListItem {
            combo: r.combo.clone(),
            app: r.app.clone(),
            mark,
            flag,
            marked: r.marked,
            row: i,
        });
        // Same call, same answer: the editor cannot say something the list
        // does not.
        if m.selected == Some(i) {
            selected = Some(pos);
            detail = Some(Detail {
                combo: r.combo.clone(),
                app: r.app.clone(),
                notes,
            });
        }
    }
```

and in the returned `ControlState` literal change `selected: m.selected,` to:

```rust
        selected,
```

`unreadable_state` (~line 809) builds `items: Vec::new()` and needs no change
— checked, not assumed. It also sets `selected: None` already, so the view/model
distinction never reaches it.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p beckon-core --lib 2>&1 | tail -5`
Expected: PASS. If an older test asserted `cs.selected` equals a model index
under no filter, it still passes — with an empty filter the view index and the
model index are equal.

- [ ] **Step 5: Break it on purpose, and confirm the guard notices**

Temporarily change `row: i,` to `row: pos,` and run
`cargo test -p beckon-core --lib settings::tests::list_items_carry 2>&1 | tail -5`.
Expected: **FAIL**. Then restore `row: i,` and re-run: PASS. A test that
cannot tell the defect from the fix is not a test.

- [ ] **Step 6: Commit**

```bash
git add crates/beckon-core/src/settings.rs
git commit -m "feat(core): ListItem carries its model row, selected is a view index

control_state now walks Model::visible() rather than every row. ListItem.row
is the model index; ControlState::selected is the position within items,
which is what its doc already said it was, and None when the filter hides
the selected row.

That last part is load-bearing beyond tidiness: detail goes None with it, so
the App field is cleared BEFORE apply_state calls layout -- and an empty
combo has nothing to re-synchronise, which keeps a filter keystroke off the
SetWindowPos path that caused the data loss in spec 7.15."
```

---

## Task 3: Remove never deletes a row you cannot see

**Files:**
- Modify: `crates/beckon-core/src/settings.rs` — `marked_count` (~line 258),
  `remove_marked` (~line 274), `remove_pressed` (~line 311), `control_state`'s
  `remove_enabled` (~line 660)
- Test: same file, `mod tests`

**Interfaces:**
- Consumes: `Model::visible()` (Task 1), the view-index `selected` (Task 2).
- Produces: `Model::remove_indices(&mut self, idx: &[usize])`. `remove_marked`
  and `remove_pressed` keep their existing signatures.

- [ ] **Step 1: Write the failing tests**

```rust
    #[test]
    fn remove_takes_the_ticked_row_you_can_see() {
        let mut m = three();
        m.set_marked(1, true); // Brave
        m.set_filter("brave");
        m.remove_pressed();
        let apps: Vec<&str> = m.rows.iter().map(|r| r.app.as_str()).collect();
        assert_eq!(apps, vec!["Notepad", "Weather"]);
    }

    /// The invariant the whole design turns on: a destructive button with no
    /// confirm and no undo must not act on rows that are off screen.
    #[test]
    fn remove_leaves_a_ticked_row_the_filter_is_hiding() {
        let mut m = three();
        m.set_marked(0, true); // Notepad, about to be hidden
        m.set_marked(1, true); // Brave, will stay visible
        m.set_filter("brave");
        m.remove_pressed();
        let apps: Vec<&str> = m.rows.iter().map(|r| r.app.as_str()).collect();
        assert_eq!(
            apps,
            vec!["Notepad", "Weather"],
            "Brave was visible and ticked so it goes; Notepad was ticked but \
             hidden, and Remove must never delete what is not on screen"
        );
        assert!(m.rows[0].marked, "the hidden tick survives to come back");
    }

    #[test]
    fn remove_does_nothing_when_the_selected_row_is_filtered_out() {
        let mut m = three();
        m.selected = Some(0); // Notepad
        m.set_filter("brave"); // hides it; nothing is ticked
        m.remove_pressed();
        assert_eq!(
            m.rows.len(),
            3,
            "the selection fallback must check visibility -- Model::selected \
             still points at a model row while that row is hidden"
        );
    }

    #[test]
    fn marked_count_and_remove_enabled_count_only_visible_rows() {
        let mut m = three();
        m.set_marked(0, true); // Notepad
        m.set_filter("brave"); // hides it
        let cs = control_state(&m, &status_all_ok());
        assert_eq!(
            cs.marked_count, 0,
            "a count that included hidden ticks would put a number on screen \
             that Remove does not honour"
        );
        assert!(!cs.remove_enabled, "nothing visible is ticked or selected");
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p beckon-core --lib settings::tests 2>&1 | tail -20`
Expected: FAIL — `remove_leaves_a_ticked_row_the_filter_is_hiding` deletes
Notepad too; `remove_does_nothing_when_the_selected_row_is_filtered_out`
deletes a row; `marked_count` reads 1.

- [ ] **Step 3: Implement**

Replace `marked_count`:

```rust
    /// How many **visible** rows are ticked.
    ///
    /// Visible rather than all, because `remove_pressed` acts only on rows
    /// the filter is showing -- a count that included hidden ticks would put
    /// a number on screen that Remove does not honour. Feeds
    /// `ControlState::marked_count`, which does NOT caption the remove button
    /// `Remove N` -- see that field's doc for why the caption stays constant.
    pub fn marked_count(&self) -> usize {
        self.visible()
            .iter()
            .filter(|&&i| self.rows[i].marked)
            .count()
    }
```

Rename `remove_marked`'s body into `remove_indices` and keep `remove_marked`
as a thin wrapper. The existing doc comment on `remove_marked` moves to
`remove_indices` unchanged — it already describes an arbitrary ascending index
list — with this paragraph added at the end of it:

```rust
    /// **`idx` must be ascending.** The reverse walk below removes the
    /// highest index first precisely so that nothing still queued shifts
    /// underneath it, and `Model::visible` returns model order, which
    /// satisfies that.
    pub fn remove_indices(&mut self, marked_indices: &[usize]) {
        if marked_indices.is_empty() {
            return;
        }
        self.selected = self.selected.map(|sel| {
            let before = marked_indices.iter().filter(|&&m| m < sel).count();
            sel - before
        });
        for &i in marked_indices.iter().rev() {
            self.rows.remove(i);
        }
        self.selected = if self.rows.is_empty() {
            None
        } else {
            self.selected.map(|sel| sel.min(self.rows.len() - 1))
        };
        self.dirty = true;
    }

    /// Remove every ticked row the filter is currently showing.
    pub fn remove_marked(&mut self) {
        let idx: Vec<usize> = self
            .visible()
            .into_iter()
            .filter(|&i| self.rows[i].marked)
            .collect();
        self.remove_indices(&idx);
    }
```

Replace `remove_pressed`'s body, keeping its existing doc comment and adding
the visibility paragraph:

```rust
    /// **And it never touches a row the filter is hiding.** Ticks survive
    /// being filtered out and come back when the filter is cleared, but they
    /// are inert while off screen: the property that makes a no-confirm,
    /// no-undo button acceptable is that its effect is visible.
    pub fn remove_pressed(&mut self) {
        let vis = self.visible();
        let marked: Vec<usize> = vis
            .iter()
            .copied()
            .filter(|&i| self.rows[i].marked)
            .collect();
        if !marked.is_empty() {
            self.remove_indices(&marked);
        } else if let Some(i) = self.selected.filter(|i| vis.contains(i)) {
            // The `filter` is NOT redundant: `Model::selected` still points
            // at a model row while the filter hides it, so without this the
            // fallback would delete an invisible row.
            self.remove_row(i);
        }
    }
```

In `control_state`, `remove_enabled` now reads the **view** selection computed
in Task 2:

```rust
        // Either gesture arms the button, because either gesture is one
        // `remove_pressed` acts on -- and both are scoped to what is on
        // screen, so an armed Remove always has something visible to take.
        remove_enabled: selected.is_some() || m.marked_count() > 0,
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p beckon-core --lib 2>&1 | tail -5`
Expected: PASS, including the two pre-existing `remove_marked` tests, which
run with an empty filter and are unaffected.

- [ ] **Step 5: Break it on purpose, and confirm the guards notice**

Two reverts, one at a time, each followed by
`cargo test -p beckon-core --lib settings::tests 2>&1 | tail -8`:

1. Change `remove_pressed`'s marked list back to `self.rows.iter().enumerate()
   .filter_map(|(i, r)| r.marked.then_some(i)).collect()`.
   Expected: `remove_leaves_a_ticked_row_the_filter_is_hiding` FAILS.
2. Drop `.filter(|i| vis.contains(i))` from the `else if`.
   Expected: `remove_does_nothing_when_the_selected_row_is_filtered_out` FAILS.

Restore both and re-run: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/beckon-core/src/settings.rs
git commit -m "feat(core): Remove never deletes a row the filter is hiding

remove_pressed, marked_count and remove_enabled all move onto visible().
Ticks survive being filtered out -- clearing them would lose a user's
intent to a keystroke with no way back -- but they are inert while off
screen, because the property that makes a no-confirm no-undo button
acceptable is that its effect is visible.

marked_count had to move with it. Left counting every row, the window would
say four are ticked while Remove deletes one.

remove_marked's body becomes remove_indices(&[usize]), whose ascending
precondition is now written down: the reverse walk exists so that nothing
still queued shifts underneath it."
```

---

## Task 4: The window translates view rows to model rows

**Files:**
- Modify: `crates/beckon-windows/src/settings_window.rs` — `Callbacks::on_select`
  / `on_mark` docs (~lines 511-514), the `LVN_ITEMCHANGED` arm (~lines 3128-3151)

**Interfaces:**
- Consumes: `ListItem::row` (Task 2), `Ui::items` (already set at the end of
  `apply_state`).
- Produces: nothing new. `on_select(usize)` and `on_mark(usize, bool)` now
  receive **model** indices, which is what `serve.rs` already assumes.

This task is a no-op at runtime — with no filter yet, the view index equals
the model index. It lands first so that Task 5 cannot introduce a filter
without a mapping.

- [ ] **Step 1: Update the callback docs**

```rust
pub struct Callbacks {
    /// A row became current. The index is a **model** row -- the window has
    /// already mapped it through `ListItem::row`, because the ListView only
    /// ever knows the position within the filtered list it was given.
    pub on_select: Box<dyn FnMut(usize)>,
    /// A row's tick changed: `(model row, ticked)`. Independent of
    /// `on_select` -- one click can raise both, and neither implies the
    /// other.
    pub on_mark: Box<dyn FnMut(usize, bool)>,
```

- [ ] **Step 2: Translate in the `LVN_ITEMCHANGED` arm**

Replace the body of `if lv.iItem >= 0 { ... }` with:

```rust
                    if lv.iItem >= 0 {
                        let i = lv.iItem as usize;
                        // The MODEL row this view row stands for, copied out
                        // and the borrow DROPPED before either callback runs.
                        // Both re-enter `refresh_settings` -> `apply_state`,
                        // which sends messages, and a `UI` borrow still open
                        // across an `extern "system"` boundary ABORTS the
                        // process instead of unwinding. Same discipline as
                        // `layout`'s `LayoutHandles`.
                        //
                        // `get(i)`, not `[i]`: comctl32 can deliver an
                        // LVN_ITEMCHANGED for a row that a just-pushed,
                        // shorter `items` no longer has -- which a filter
                        // makes routine rather than exotic.
                        let row = UI.with(|u| {
                            u.borrow()
                                .as_ref()
                                .and_then(|x| x.items.get(i).map(|it| it.row))
                        });
                        let Some(row) = row else {
                            return LRESULT(0);
                        };
                        // A tick and a selection both arrive as LVIF_STATE
                        // and `uChanged` cannot tell them apart, so the two
                        // bits are tested independently. Never `else if`:
                        // clicking an unselected row's box changes both in
                        // ONE message, and an `else if` drops whichever the
                        // arm did not reach.
                        let changed = lv.uOldState ^ lv.uNewState;
                        if changed & LVIS_STATEIMAGEMASK.0 != 0 {
                            let on = (lv.uNewState & LVIS_STATEIMAGEMASK.0) == LVIS_CHECKED;
                            with_cb(|cb| (cb.on_mark)(row, on));
                        }
                        if changed & LVIS_SELECTED.0 != 0 && lv.uNewState & LVIS_SELECTED.0 != 0 {
                            with_cb(|cb| (cb.on_select)(row));
                        }
                    }
```

- [ ] **Step 3: Run the gates**

Run:
```
cargo fmt --all -- --check && \
cargo check --target x86_64-pc-windows-gnu -p beckon-windows 2>&1 | tail -5
```
Expected: exit 0, `Finished`.

- [ ] **Step 4: Read the diff against the ABORT-class rule**

Run: `git diff crates/beckon-windows/src/settings_window.rs`
Confirm by eye: the `UI.with` closure returns an `Option<usize>` and ends
before `with_cb`. No `SendMessageW`, `SetWindowTextW`, `SetFocus` or
`SetWindowPos` appears inside it. This cannot be checked by the compiler,
the tests, or WINCHECK — reading it is the check.

- [ ] **Step 5: Commit**

```bash
git add crates/beckon-windows/src/settings_window.rs
git commit -m "refactor(windows): map the ListView's row to the model's

on_select and on_mark take MODEL indices; the ListView only ever knows the
position in the list it was handed. Today those are equal, so this changes
no behaviour -- it lands before the filter box so that no commit can ever
have a filter without the mapping.

The lookup copies a usize out and drops the UI borrow before calling either
callback, because both re-enter apply_state, and a second borrow across the
wndproc boundary aborts the process rather than unwinding.

get(i) rather than [i]: comctl32 can report a row index a shorter, freshly
pushed items no longer has, which a filter makes routine."
```

---

## Task 5: The control

**Files:**
- Modify: `crates/beckon-windows/src/settings_window.rs` — new `IDC_FILTER`
  and `EM_SETCUEBANNER_MSG` consts (~line 209), `struct Ui` (~line 533),
  `struct LayoutHandles` + `LayoutHandles::of` (~line 687), `Callbacks`
  (~line 510), `build_children` band 2 (~line 1548), `layout` bands 2 and 4
  (~lines 2128 and 2219), `apply_state` (~line 2412), `handle_command`
  (~line 3233)
- Modify: `crates/beckon-cli/src/serve.rs` — the `Callbacks` literal (~line 1136)

**Interfaces:**
- Consumes: `Model::set_filter` / `Model::filter` (Task 1), the mapping (Task 4).
- Produces: `Callbacks::on_filter: Box<dyn FnMut(String)>` and
  `ControlState::filter: String`.

- [ ] **Step 1: Add the core-side `ControlState::filter`**

In `crates/beckon-core/src/settings.rs`, add to `ControlState`:

```rust
    /// What the filter box should show. Pushed so `Add` can clear it; the
    /// window writes it back ONLY when it differs from what the control
    /// already holds, because an unconditional `WM_SETTEXT` raises
    /// `EN_CHANGE` on every push and would fight the user's typing.
    pub filter: String,
```

Set it in `control_state`: `filter: m.filter().to_string(),`
and in `unreadable_state`: `filter: String::new(),`

Then make `add_row` clear it — a new row is empty, matches no non-empty
filter, and would otherwise be created invisible while `add_row` selects it:

```rust
    pub fn add_row(&mut self) {
        // A new row matches no non-empty filter, so it would be created off
        // screen while the editor strip pointed at it. Checklist item 6
        // ("after Add, the new row is visible AND selected") keeps its
        // meaning this way instead of needing a new one.
        self.filter.clear();
        self.rows.push(Row {
```

Add the test:

```rust
    #[test]
    fn add_clears_the_filter_so_the_new_row_is_visible() {
        let mut m = three();
        m.set_filter("brave");
        m.add_row();
        assert_eq!(m.filter(), "");
        let cs = control_state(&m, &status_all_ok());
        assert_eq!(cs.items.len(), 4);
        assert_eq!(
            cs.selected,
            Some(3),
            "the new row must be both visible and selected"
        );
    }
```

Run: `cargo test -p beckon-core --lib 2>&1 | tail -5` — expected PASS.

- [ ] **Step 2: Declare the id, the message and the caption**

In `crates/beckon-windows/src/settings_window.rs`, after `IDC_LBL_SECTION`:

```rust
const IDC_FILTER: i32 = 1021;
```

Beside `SS_CENTERIMAGE_STYLE` (~line 135), for the same reason it exists:

```rust
/// `EM_SETCUEBANNER` (`ECM_FIRST + 1`), which `windows` 0.61 does not
/// export -- the same gap `SS_CENTERIMAGE_STYLE` above fills.
const EM_SETCUEBANNER_MSG: u32 = 0x1501;
```

In `mod cap`:

```rust
    /// The filter box's placeholder. ASCII, like every display string.
    pub const FILTER_CUE: &str = "Filter";
```

- [ ] **Step 3: Create the control**

In `build_children`, **delete** the "No filter control, and no placeholder for
one" comment block (~line 1548, it is now false) and insert between the
`IDC_LBL_SECTION` `child(...)` call and the `IDC_REMOVE` one:

```rust
    let filter = child(
        hwnd,
        w!("EDIT"),
        "",
        WINDOW_STYLE(ES_AUTOHSCROLL as u32) | WS_BORDER | WS_TABSTOP,
        IDC_FILTER,
        &fonts,
    );
    // Placeholder text rather than a STATIC label: it costs no band-2 width
    // and gets out of the way on focus. comctl32 v6 only, which the manifest
    // guarantees. The buffer must outlive the call, so it is bound.
    let cue = wide(cap::FILTER_CUE);
    SendMessageW(
        filter,
        EM_SETCUEBANNER_MSG,
        Some(WPARAM(0)),
        Some(LPARAM(cue.as_ptr() as isize)),
    );
```

Creation order is tab order, so this puts the filter between the heading (a
`STATIC`, no tab stop) and `Remove`, which is reading order.

Store it: add `filter: HWND,` to `struct Ui` and to the `Ui { .. }` literal
in the creation function; add `filter: HWND,` to `struct LayoutHandles` and
`filter: ui.filter,` to `LayoutHandles::of`.

- [ ] **Step 4: Lay it out**

In `layout`, move the field-height block **above** band 2 — it is currently
inside band 4. Cut these three lines from band 4 and paste them immediately
after `let mut y = pad;`:

```rust
    // Field geometry, computed before band 2 because the filter box needs it
    // there and the editor strip needs it in band 4. `combo_h` is therefore
    // read BEFORE the combo is placed this pass, i.e. it is the height the
    // combo had on the PREVIOUS pass. That is sound: the value is the theme's
    // choice for a font and a DPI, so it moves only on WM_DPICHANGED or a
    // font change, both of which run `layout` again immediately. The one pass
    // that can read a not-yet-snapped height is the first, and the floor
    // below falls back to the font-derived height there.
    let text_h = text_size(hwnd, ui.fonts.get(Role::Body), dpi, "Ag").1;
    let field_h = (text_h + s(10)).min(ctl);
    let mut arc = RECT::default();
    let combo_h = if GetWindowRect(ui.app, &mut arc).is_ok() {
        let ah = arc.bottom - arc.top;
        if ah > 0 && ah < ctl && ah >= text_h + s(2) {
            Some(ah)
        } else {
            None
        }
    } else {
        None
    };
    // Both EDITs take the combo's height, so the three fields in this window
    // are one box repeated. A single-line EDIT top-aligns its text -- Win32
    // gives it no vertical centring at all -- so it is centred in its band
    // line rather than stretched to it.
    let (edit_h, edit_dy) = match combo_h {
        Some(ah) => (ah, clamp(ctl - ah) / 2),
        None => (field_h, clamp(ctl - field_h) / 2),
    };
```

Band 2 becomes:

```rust
    // -- Band 2: the section head. `Shortcuts` leading, then the filter,
    // then Remove and Add right-aligned.
    let bw_add = btn(cap::ADD);
    let bw_remove = btn(cap::REMOVE);
    // The filter yields width before the heading does on a narrow window.
    let filter_w = s(tok::SHORTCUT_COL).min(clamp(cw / 3));
    let filter_x = cx + clamp(cw - bw_add - gap - bw_remove - gap - filter_w);
    place(IDC_ADD, cx + clamp(cw - bw_add), y, bw_add, ctl);
    place(
        IDC_REMOVE,
        cx + clamp(cw - bw_add - gap - bw_remove),
        y,
        bw_remove,
        ctl,
    );
    place(IDC_FILTER, filter_x, y + edit_dy, filter_w, edit_h);
    place(IDC_LBL_SECTION, cx, y, clamp(filter_x - gap - cx), ctl);
    // A control gap, not a band gap: the head labels the list directly
    // below it, so the two read as one group.
    y += ctl + gap;
```

In band 4, delete the now-moved `text_h` / `field_h` / `arc` / `combo_h` /
`edit_h` lines and the `fy` binding, and replace the two field placements
with:

```rust
    place_h(ui.app, app_x, y + edit_dy, app_w, field_h * 9);
    place(IDC_LBL_SHORTCUT, lbl_short_x, y, lw_short, ctl);
    place_h(ui.combo, edit_x, y + edit_dy, field_w, edit_h);
```

Every other use of `fy` in band 4 becomes `y + edit_dy`.

- [ ] **Step 5: Push and read the text**

In `apply_state`, beside the other conditional field writes and **inside the
same block that already holds no borrow**:

```rust
        // Conditional, like every other field write here: an unconditional
        // WM_SETTEXT raises EN_CHANGE on every push, which for this control
        // would mean fighting the user's own typing on every keystroke. It
        // is written at all only so `Add` can clear it.
        if text_of(filter) != st.filter {
            set_text(filter, &st.filter);
        }
```

`filter` comes from the same handle read the other controls use in this
function — add it wherever `combo`, `app` and `notes` are read out of `UI`.

In `handle_command`, beside `let combo = ...`:

```rust
    let filter = match UI.with(|u| u.borrow().as_ref().map(|x| x.filter)) {
        Some(t) => t,
        None => return,
    };
```

and add the arm next to the `IDC_COMBO` `EN_CHANGE` one:

```rust
        (IDC_FILTER, c) if c == EN_CHANGE => {
            if !suppressed() {
                let t = text_of(filter);
                with_cb(|cb| (cb.on_filter)(t));
            }
        }
```

- [ ] **Step 6: Declare and wire the callback**

In `Callbacks`:

```rust
    /// The filter box's text changed. Indices in `on_select` / `on_mark` are
    /// model rows either way -- the window maps them.
    pub on_filter: Box<dyn FnMut(String)>,
```

In `crates/beckon-cli/src/serve.rs`, in the `Callbacks` literal:

```rust
        on_filter: Box::new(edit!(
            state,
            |m: &mut beckon_core::settings::Model, t: String| m.set_filter(&t)
        )),
```

While there, the `on_mark` bound check keeps working — `i` is a model row and
`i < m.rows.len()` still guards it — but update its comment, which now names
the wrong hazard:

```rust
                        // `set_marked` indexes `rows` directly. The window
                        // has already mapped the view row to a model row, so
                        // this guards a stale push rather than a filter.
```

- [ ] **Step 7: Run the gates**

Run:
```
cargo fmt --all -- --check && \
cargo test -p beckon-core 2>&1 | tail -3 && \
cargo check --target x86_64-pc-windows-gnu -p beckon-windows 2>&1 | tail -3 && \
cargo check --target x86_64-pc-windows-gnu -p beckon-cli 2>&1 | tail -3
```
Expected: exit 0 throughout.

- [ ] **Step 8: Commit**

```bash
git add crates/beckon-core/src/settings.rs crates/beckon-windows/src/settings_window.rs crates/beckon-cli/src/serve.rs
git commit -m "feat(windows): a filter box for the shortcut list

IDC_FILTER = 1021 in band 2, between the heading and Remove, so tab order
follows reading order. No label: EM_SETCUEBANNER costs no band width and
gets out of the way on focus. 1002-1007 are untouched -- settings_probe
hard-codes them.

layout's field geometry moves above band 2 because two bands now need it,
which makes combo_h the height the combo had on the previous pass. That is
the theme's choice for a font and a DPI, so it moves only on WM_DPICHANGED
or a font change, and both re-run layout immediately.

Add clears the filter: a new row matches no non-empty filter and would
otherwise be created off screen while the editor strip pointed at it.

The filter text is pushed back to the control only when it differs, like
every other field write here -- an unconditional WM_SETTEXT would raise
EN_CHANGE on every push and fight the user's typing."
```

---

## Task 6: The hardware gates on a14

**Files:**
- Create: `~/hwpass/FilterGate.cs` on a14 (a throwaway probe, not committed)
- Modify: `docs/superpowers/measurements/2026-08-11-landing-1-a14.md`

**Interfaces:**
- Consumes: the whole feature.
- Produces: a measurement record. Nothing depends on it in code.

None of this is reachable from a unit test. **Every run needs its control** —
a check that cannot tell "working" from "broken detector" is not a check.

Environment, all of it load-bearing:
- SSH lands in **session 0**, which has no desktop. Drive everything through a
  scheduled task in session 1, principal
  `S-1-5-21-2934948618-3885663962-3981577510-1001`, `-LogonType Interactive`,
  with **both** `-AllowStartIfOnBatteries` and `-Priority 4` (the default
  priority 7 on battery looks exactly like a hang).
- Quoting always breaks through `-Command`. Use `-EncodedCommand` with
  `iconv -f UTF-8 -t UTF-16LE f | base64 | tr -d '\n'` (python returns empty
  silently on this mac). Put anything with a redirect in a `.bat`.
- `$ErrorActionPreference = "Stop"` plus `cargo ... 2>&1` throws on cargo's
  own progress lines, which go to stderr. Check `$LASTEXITCODE` instead.
- Build with `cargo build --all-targets`, never `--examples`.
- `WM_SETTEXT` into a field reads back empty while no row is selected — the
  window pushes the model's state back over it. `WM_COMMAND` to a button id
  works cross-process; synthetic `SendInput` clicks did not select a list row
  on this machine. Both measured, §37.

- [ ] **Step 1: Gate A — the §8 combo-box path**

The one this feature could plausibly break. Select a row so the App field
holds a name, type a filter that empties the list, clear it, and read the App
field back.

Drive it with `WM_COMMAND` to `IDC_ADD` (to get a selected row),
`WM_SETTEXT` to `IDC_APP` (now that a row is selected it sticks), then
`WM_SETTEXT` to `IDC_FILTER` with `"zzz"`, then `""`, reading `IDC_APP` with
`WM_GETTEXT` at each stage.

Expected: the App text after the filter round trip equals the App text before
it.

**CONTROL:** run the same sequence a second time with the App field left
empty. A pass there proves nothing on its own — it is there so that a pass in
the populated run is not merely a field that had nothing to lose. Report both.

- [ ] **Step 2: Gate B — the mapping, observed rather than argued**

With a filter active that hides row 0, tick the **first visible** row and read
back which model row the config now considers marked. Do it by pressing
Remove and reading the resulting file.

Expected: the row the user could see is gone; the hidden row survives.

**CONTROL:** repeat with no filter set. The same gesture must remove the first
row. If both runs delete the same row, the probe is not exercising the filter.

- [ ] **Step 3: Gate C — band 2 lays out**

Run `settings_probe` and read the geometry block.

Expected: `EDIT IDC_FILTER` is present, its height equals `EDIT IDC_COMBO`'s
and `COMBOBOX IDC_APP`'s, and no band-2 control overlaps another
(`IDC_LBL_SECTION` right edge <= `IDC_FILTER` left edge, and so on rightward).

**CONTROL:** the run's existing `CONTROL harness SessionId: 1` and
`explorer.exe in session: yes` lines. A session-0 run finds nothing and looks
identical to a broken layout.

- [ ] **Step 4: Record the results**

Append a section to
`docs/superpowers/measurements/2026-08-11-landing-1-a14.md` in the style of
§36–§37: what was measured, the control beside it, and verbatim probe output.
State plainly anything that was **not** run.

- [ ] **Step 5: Commit**

```bash
git add docs/superpowers/measurements/2026-08-11-landing-1-a14.md
git commit -m "docs: the filter box on hardware"
```

---

## Self-Review

**Spec coverage:**

| Spec section | Task |
|---|---|
| §1 what the filter matches | 1 |
| §2 filter lives in the model; not dirty; never written | 1 |
| §3 API (`set_filter`, `filter`, `visible`, `ListItem::row`, view `selected`) | 1, 2 |
| §4 Remove invariant, `remove_indices`, visible-scoped counts | 3 |
| §5 window translation without aborting | 4 |
| §6 the control, id, cue banner, tab order, height | 5 |
| §7 Add clears the filter | 5 |
| §8 the `shown_empty` → `layout` → combo risk | 2 (defused), 6 Gate A (measured) |
| §9 rejected alternatives | n/a — no task |
| §10 testing | 1, 2, 3 (core), 6 (hardware) |

**Type consistency:** `Model::visible() -> Vec<usize>` (Task 1) is consumed by
Tasks 2 and 3. `ListItem::row: usize` (Task 2) is read in Task 4 as
`x.items.get(i).map(|it| it.row)`. `ControlState::filter: String` (Task 5,
step 1) is read in Task 5, step 5 as `st.filter`.
`Callbacks::on_filter: Box<dyn FnMut(String)>` (Task 5, step 6) is invoked in
step 5 with a `String` from `text_of`. `remove_indices(&[usize])` (Task 3) is
called by `remove_marked` and `remove_pressed` in the same task.

**Known ordering hazard, stated so it is not rediscovered:** Task 5 step 4
moves `text_h` / `field_h` / `combo_h` above band 2 and renames `fy` to
`y + edit_dy`. Band 4 has several uses of `fy`; missing one is a compile
error, not a silent layout bug, because `fy` ceases to exist.
