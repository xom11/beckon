# The settings window's filter box

Date: 2026-08-12
Branch: `landing-2a-followups`
Supersedes: `docs/superpowers/2026-08-12-landing-2a-followups.md` §1, which
recorded the trap but not the design.

Landing 2a's plan listed a filter `EDIT` in band 2 and deliberately did not
ship it. The reason was a defect the control would have created, not the cost
of the control: `on_select(i)` and `on_mark(i)` index `Model.rows` directly,
so a filtered list turns `i` into a *view* index and both callbacks address
the wrong row — ticking one binding and deleting another, on a destructive
button with no confirm and no undo.

This spec is the control **plus** everything that makes it safe. The mapping
is the feature; the `EDIT` is the small part.

---

## 1. What the filter matches

Case-insensitive substring against **both** columns — the app name and the
combo. An empty filter (after trimming) matches every row.

Matching the combo as well as the app is what answers the question this file
is usually opened to answer: *what is this key already bound to?* Typing
`alt+q` finds it. The rule is the one `beckon search` and the `.desktop`
tier-4 resolver already use, so there is no third matching dialect in the
program.

The filter string is **trimmed before matching**. A trailing space left by
typing would otherwise hide every row, which reads as a hang.

## 2. Where the filter lives, and why it is not window state

`Model.filter: String`.

The tempting shape is to keep it in `Ui` and let the window hide rows as it
pushes them. That is wrong here, because four decisions depend on which rows
are visible:

- `remove_pressed` — which rows Remove deletes.
- `remove_enabled` — whether Remove is even armed.
- `marked_count` — how many rows are ticked.
- `selected` — which line the ListView re-highlights after a rebuild.

Keeping the filter in the window means passing it into all four, which puts
those decisions back in the wndproc. `beckon-core` exists to stop exactly
that: it compiles on **three** CI jobs and `beckon-windows` on one, and the
same argument is already written out at `remove_pressed` and
`default_button_of`.

Two properties the filter shares with `Row::marked`, for the same reasons:

- **It never sets `dirty`.** It changes nothing on disk. `apply_enabled` is
  `dirty && valid`, so a filter that marked the model dirty would light up
  Save and rewrite the file byte-identical.
- **It is never written.** `RowWrite` has no such field and `render()` never
  sees it. A test pins this rather than leaving it to inspection.

## 3. The API

```rust
impl Model {
    /// View-only: never written to disk, never makes the model dirty.
    pub fn set_filter(&mut self, v: &str);
    pub fn filter(&self) -> &str;

    /// Model-row indices that pass the filter, in model order.
    fn visible(&self) -> Vec<usize>;
}

pub struct ListItem {
    // ...
    /// This item's index in `Model.rows`. **Not** its position in `items`
    /// once a filter is active. Every callback that reaches the model must
    /// go through this.
    pub row: usize,
}
```

`ControlState::selected` keeps its existing meaning verbatim — its doc
already says *"as an index into `items`"* — which now makes it a **view**
index. It is `None` when the selected model row is filtered out, and that is
load-bearing rather than incidental; see §5.

## 4. Remove, and the invariant that is a function rather than a discipline

**Remove never deletes a row you cannot see.**

```rust
pub fn remove_pressed(&mut self) {
    let vis = self.visible();
    let marked: Vec<usize> = vis.iter().copied().filter(|&i| self.rows[i].marked).collect();
    if !marked.is_empty() {
        self.remove_indices(&marked);
    } else if let Some(i) = self.selected.filter(|i| vis.contains(i)) {
        self.remove_row(i);
    }
}
```

Ticks **survive** being filtered out and come back when the filter is
cleared. The alternative — clearing a row's tick as it leaves the view — was
considered and rejected: a user who ticks three rows and then types in the
filter to find a fourth would silently lose the three, and clearing the
filter would not bring them back. Losing user intent to a keystroke is worse
than keeping inert state that reappears where it was left.

That choice has a mandatory consequence. **`marked_count` and
`remove_enabled` must be computed over the visible set too.** Otherwise the
window says four rows are ticked and Remove deletes one, which is a UI that
lies. Both move onto `visible()`:

```rust
remove_enabled: visible_selected.is_some() || visible_marked_count > 0,
marked_count:   visible_marked_count,
```

`remove_marked()` generalises to `remove_indices(&[usize])`, keeping its
reverse-removal walk and its `selected` shift arithmetic unchanged — both are
already correct for an arbitrary ascending index list, which is what the
existing doc comment describes. **Ascending order is a precondition, not a
convention**: the walk removes highest-first precisely so no queued index
shifts, and `visible()` returns model order, which satisfies it. The two
existing `remove_marked` tests keep working through a thin wrapper.

The `else if` arm's `.filter(|i| vis.contains(i))` is the second half of the
invariant and is easy to drop as "redundant". It is not: `Model.selected`
still points at a model row while that row is hidden, so without the guard a
Remove pressed on a filtered list with nothing ticked would delete an
invisible row.

## 5. The window: reading the mapping without aborting the process

`on_select` and `on_mark` keep taking **model** indices. The window
translates, using the `items` it already keeps in `Ui` (`Ui::items`, set at
the end of `apply_state`).

The translation has to happen **before** the callback runs:

```rust
// The model row, copied out and the borrow dropped, BEFORE `with_cb`.
// Both callbacks re-enter `refresh_settings` -> `apply_state`, which sends
// messages; a `UI` borrow still open across that boundary ABORTS the
// process rather than unwinding.
let row = UI.with(|u| u.borrow().as_ref().and_then(|x| x.items.get(i).map(|it| it.row)));
let Some(row) = row else { return LRESULT(0) };
```

This is the same discipline `layout` follows with `LayoutHandles`. The
`get(i)` rather than `[i]` matters too: comctl32 can deliver an
`LVN_ITEMCHANGED` for a row index that a just-pushed shorter `items` no
longer has.

## 6. The control

- `IDC_FILTER = 1021`. `IDC_LBL_SECTION` is 1020, so this is the next free
  id. **1002–1007 are not renumbered** — `examples/settings_probe.rs`
  hard-codes them, as does the class name `BeckonSettingsWindow`.
- Band 2 becomes `Shortcuts` │ filter │ `Remove` │ `Add`. The heading keeps
  its current rule of taking whatever the controls to its right leave it, so
  no Subtitle-width measurement is introduced.
- **No label.** `EM_SETCUEBANNER` with the ASCII string `Filter`, which costs
  no layout width and disappears on focus. comctl32 v6 only, which the
  manifest guarantees. ASCII because it is a display string.
- Created after `IDC_LBL_SECTION` and before `Remove`/`Add`, so tab order
  follows reading order: filter → Remove → Add → list.
- It is an `EDIT`, so `is_push_button` is false and the default ring never
  rests on it. Enter from the filter therefore saves, which is the same
  answer the other fields, the list and the check boxes already give. Esc
  still closes — it deliberately does **not** clear the filter, because
  changing what Esc means in this window is a bigger decision than this
  control earns.
- Height: the same height the App `COMBOBOX`'s theme picked, shared with the
  Shortcut `EDIT` (see commit `f0644d5`). A single-line `EDIT` stretched to
  the full `ctl` band line would park its text against the top edge.

  This moves the `combo_h` read from band 4 up to the top of `layout`, since
  band 2 is placed first — so it returns the height the combo had **on the
  previous pass**, not this one. That is sound and worth stating rather than
  discovering: the value is the theme's choice for a font and a DPI, so it
  moves only on `WM_DPICHANGED` or a font change, both of which run `layout`
  again immediately. The one pass that can read a not-yet-snapped height is
  the first, where the combo has just been created; the existing floor
  (`ah >= text_h + s(2)`, else the font-derived height) already covers it,
  and the next `layout` corrects it.

`Callbacks` gains `on_filter: Box<dyn FnMut(&str)>`.

## 7. Add clears the filter

A new row is empty, so it matches no non-empty filter and would be created
invisible — while `add_row` selects it and the editor strip points at
something not on screen.

So `Add` clears the filter. Checklist item 6 (*"after Add, the new row is
visible **and** selected"*) then keeps its current meaning instead of needing
a new one.

## 8. The risk this feature reaches, which is not in the follow-ups note

> **SUPERSEDED, and by something this section did not anticipate.** The
> argument below was measured and held (measurements §40), but the final
> whole-branch review found a defect it never considered: this section only
> asked what happens when the **filter box** is the input. When the **App
> field** is the input — the user edits a row until it stops matching — the
> same "drop it from the view" rule pulls the row out from under the editor
> mid-word, and `apply_state`'s `None` arm disables the control that has
> keyboard focus and blanks it.
>
> `Model::visible` now exempts the selected row from the filter, which fixes
> that *and* closes this section's risk outright: with a row selected the list
> cannot reach zero rows, so `Ui::shown_empty` cannot flip, so `layout` never
> runs on a filter keystroke at all. Measurements §42–§44. The reasoning below
> is kept because it is what the design argued at the time, and because the
> gate it produced is what proved the replacement.

Filtering down to zero rows empties `st.items`, which flips `Ui::shown_empty`,
which makes `apply_state` call `layout`, which `SetWindowPos`es the App
`COMBOBOX`. **That is the exact path that silently replaced what the user
typed with a catalogue entry** — spec `2026-08-11-settings-window-redesign.md`
§7.15 and measurements §24–§26. The filter turns it from a rare transition
(add the first row, remove the last) into one reachable on a keystroke.

The design defuses it: when the selected row is filtered out, `selected` is
`None` and `detail` is `None`, so the App field is **cleared before `layout`
runs** — `apply_state` writes the fields first and calls `layout` last — and
an empty combo has nothing to re-synchronise to. `combo_probe` measured that
the empty combo behaves differently from the populated one.

**That is an argument, not a measurement, and it must not ship as one.** It
becomes a hardware gate: §10.

## 9. Rejected

- **Filter state in `Ui`.** §2.
- **Clearing a row's tick when the filter hides it.** §4.
- **Letting Remove delete ticked-but-hidden rows.** The property that makes
  a no-confirm, no-undo button acceptable is that its effect is on screen.
- **Storing the model index in the ListView's `LVITEM.lParam`** instead of in
  `ListItem`. It puts the map where the rows are and cannot drift, which is
  genuinely attractive — but it is readable only from `beckon-windows`, and
  the whole point of §1 is that this mapping gets a test on all three CI
  jobs. Reconsider only if `Ui::items` ever stops being kept.
- **A `Filter` STATIC label.** Costs band-2 width for what a cue banner says
  for free.
- **Esc clears the filter.** §6.

## 10. Testing

**`beckon-core`, so it runs on all three CI jobs** — this is the reason §1
insisted the mapping live here:

1. `visible()` with an empty filter is every row, in model order.
2. The filter matches the app name, case-insensitively.
3. The filter matches the combo (`alt+q` finds the row bound to it).
4. A filter matching nothing yields no items.
5. **`ListItem.row` is the model index, not the position.** Asserts both
   `items[1].row == <expected model index>` and `items[1].row != 1`, so the
   §1 trap itself turns the test red if anyone drops the mapping.
6. `ControlState::selected` is the **view** index while filtered.
7. `selected` is `None` when the selected row is filtered out.
8. `remove_pressed` deletes ticked rows that are **visible** and leaves
   ticked rows that are **hidden**.
9. `remove_pressed` with nothing ticked and the selected row hidden deletes
   **nothing**.
10. `marked_count` and `remove_enabled` count only visible rows.
11. `set_filter` leaves `dirty()` false, and `render()` output is unchanged
    by it.

Each is written to fail before the code exists, and the ones that guard a
specific defect (5, 8, 9) are validated by reverting the guard and watching
them go red — the only check that has never been wrong in this project.

**On a14, in session 1** — none of this is reachable from a unit test:

- **The §8 gate.** Select a row so the App field holds a name, type into the
  filter until the list is empty, clear it again, and read the App field
  back with `WM_GETTEXT`. It must be unchanged. Run the whole thing a second
  time with the App combo left empty as the control, so a passing result is
  not merely a combo that had nothing to lose.
- **The mapping, end to end.** With a filter active, tick the second visible
  row and confirm the model ticked the row the user clicked — the defect this
  feature exists to avoid, observed rather than argued.
- Band 2 lays out without overlap at 150 %, and the filter `EDIT` measures
  the same height as the Shortcut `EDIT` and the App `COMBOBOX`.

**Note on driving the window from a probe**, learned in measurements §37 and
directly relevant here: `WM_SETTEXT` into a field reads back empty while no
row is selected, because the window pushes the model's state back over it;
and synthetic `SendInput` clicks did not select a list row on this machine.
`WM_COMMAND` to a button id crosses the process boundary and works. The
filter `EDIT` is a field, so a probe must expect the same echo behaviour —
though unlike the others it has no model row to depend on, which is itself
worth confirming rather than assuming.
