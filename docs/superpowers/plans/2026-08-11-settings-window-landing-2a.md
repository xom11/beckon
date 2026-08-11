# Settings window redesign — Landing 2a: the window, the list, the status vocabulary

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Rebuild the settings window on the foundation Landing 1 laid — five quiet bands instead of a 45/55 split, the app name leading, healthy rows saying nothing, a fixed-height list with per-row checkboxes, and a status vocabulary a non-developer can read.

**Architecture:** The decision half is pure and lives in `beckon-core/src/settings.rs` — `Severity`, the split of `Mark::Unknown`, `Row.marked`, multi-row delete, and one severity function feeding both the list and the editor. The drawing half is `beckon-windows/src/settings_window.rs`, which holds no policy and consumes `ControlState`. Capture, the availability probe and the Caps row are **not** here; they are Landing 2b.

**Tech Stack:** Rust (MSRV 1.75), `windows` 0.61, raw Win32, comctl32 v6 (the manifest landed in Landing 1).

## Global Constraints

- **MSRV is 1.75.** No API newer than that.
- **Pure logic in `beckon-core`/`beckon-cli`; Win32 in `beckon-windows`.** CI passes `--exclude beckon-windows` on the Linux and macOS jobs, so anything placed there is tested by one job in three.
- **MACCHECK**, after every task:
  ```bash
  cargo test   --workspace --exclude beckon-linux --exclude beckon-windows
  cargo clippy --workspace --exclude beckon-linux --exclude beckon-windows --all-targets -- -D warnings
  ```
- **WINCHECK**, after touching any Windows-conditional code — both halves, the clippy one is not optional:
  ```bash
  cargo check  --target x86_64-pc-windows-gnu -p beckon-windows -p beckon-cli --all-targets
  cargo clippy --target x86_64-pc-windows-gnu -p beckon-windows -p beckon-cli --all-targets -- -D warnings
  ```
- `cargo fmt --all -- --check` clean.
- **No new crate dependency.**
- **`serve` log messages stay ASCII.** Window captions and dialog text are not log output.
- **Displayed strings are ASCII too**, for the reason `mark_glyph` already gives: the window inherits the shell font and a missing glyph reads as a rendering bug rather than a status.
- **`settings_probe` hard-codes the class name `BeckonSettingsWindow` and control ids 1002–1007.** They are fixed points, not implementation details.

## What Landing 1 established, so nobody re-derives it

Measured on a14 (Windows 11 26200, ARM64), each with a control run — see
`docs/superpowers/measurements/2026-08-11-landing-1-a14.md`:

- `beckon-serve.exe` is **`PER_MONITOR_AWARE`**. The DPI work is real now;
  `scale(v, dpi)` at `settings_window.rs:72` is the single scaling helper and
  is no longer the identity function.
- `lfMessageFont` is **`Segoe UI`, `lfHeight = -12`, weight 400** — *not*
  Segoe UI Variable. `iCaptionHeight=22`, `iScrollWidth=17`, `iMenuHeight=19`,
  `SM_CXSMICON=16`, `SM_CXICON=32`.
- **Segoe UI Variable exposes twelve GDI families**, and the 31-character
  truncation is not uniform: `Segoe UI Variable Text Semibold` survives in
  full, `Segoe UI Variable Display Semib` and `…Small Semibol` do not. Asking
  for `Segoe UI Variable Text Semib` returns **Arial**, measured, against a
  control that also returned Arial.
- `Segoe UI Variable Text` / `Small` / `Display`, `Segoe Fluent Icons` and
  `Segoe MDL2 Assets` all resolve exactly.
- The settings window opens and is drivable under the manifest.

**Not measured, and Task 2 exists to fix that:** themed control heights under
comctl32 v6, and anything at 150 % scale.

## File Structure

**Modify:**

| Path | Change |
|---|---|
| `crates/beckon-core/src/settings.rs` | `Severity`; `Mark` split; `Row.marked`; `remove_rows`; one severity function feeding list and editor; `ControlState` grows the fields the new layout draws |
| `crates/beckon-windows/src/settings_window.rs` | The `suppressed()` guard; checkboxes; diff-instead-of-rebuild; five-band layout; type ramp; `GetSysColor`; command bar; mnemonics |
| `crates/beckon-windows/examples/settings_probe.rs` | Print each control's rect, so v6 metrics are measurable |
| `crates/beckon-cli/src/serve.rs` | Wire `on_mark`, `on_remove_marked`, and the read-only path |
| `README.md`, `CLAUDE.md` | The window's new shape; the status vocabulary |

---

### Task 1: The `suppressed()` guard on `WM_NOTIFY` — abort-class, lands first

`settings_window.rs`'s `WM_NOTIFY` arm does not consult `suppressed()`, unlike every `WM_COMMAND` arm. The moment `apply_state` writes selection or check state with `LVM_SETITEMSTATE`, comctl32 fires `LVN_ITEMCHANGED` **synchronously inside `apply_state`** → `on_select` → `refresh_settings` → `apply_state`: unbounded recursion across an `extern "system"` boundary, where a `RefCell` double-borrow **aborts the process** rather than unwinding.

Nothing else in this plan may land before it.

**Files:**
- Modify: `crates/beckon-windows/src/settings_window.rs` (the `WM_NOTIFY` arm, ~`:880`)

**Interfaces:**
- Consumes: `suppressed()`, already present.
- Produces: nothing new.

- [ ] **Step 1: Add the guard**

At the very top of the `WM_NOTIFY` arm, before it reads `NMHDR`:

```rust
            WM_NOTIFY => {
                // Every WM_COMMAND arm asks this; this one did not, and that
                // becomes fatal the moment `apply_state` writes item state.
                // `LVM_SETITEMSTATE` makes comctl32 fire LVN_ITEMCHANGED
                // SYNCHRONOUSLY, inside `apply_state` -- so the chain
                // apply_state -> on_select -> refresh_settings -> apply_state
                // recurses without bound across an `extern "system"`
                // boundary, where a second RefCell borrow ABORTS the process
                // instead of unwinding. Landing 2a writes item state for the
                // first time, so this guard has to exist before any of it.
                if suppressed() {
                    return LRESULT(0);
                }
```

- [ ] **Step 2: WINCHECK and commit**

```bash
cargo fmt --all
cargo check  --target x86_64-pc-windows-gnu -p beckon-windows -p beckon-cli --all-targets
cargo clippy --target x86_64-pc-windows-gnu -p beckon-windows -p beckon-cli --all-targets -- -D warnings
git add crates/beckon-windows/src/settings_window.rs
git commit -m "fix(windows): WM_NOTIFY never asked whether we were suppressing

Every WM_COMMAND arm does. The moment apply_state writes item state,
comctl32 fires LVN_ITEMCHANGED synchronously from inside it, so
apply_state -> on_select -> refresh_settings -> apply_state recurses
without bound across an extern \"system\" boundary -- where a second
RefCell borrow aborts the process rather than unwinding. Landing 2a
writes item state for the first time, so this lands before any of it."
```

---

### Task 2: Teach `settings_probe` to print rects, and measure v6 on a14

Every spacing token below is a guess until this runs. Landing 1 measured the font and the DPI awareness but not control heights, because `settings_probe` prints class names without rects and closes the window faster than an external poller can catch it.

**Files:**
- Modify: `crates/beckon-windows/examples/settings_probe.rs`
- Create: append to `docs/superpowers/measurements/2026-08-11-landing-1-a14.md`

**Interfaces:**
- Consumes: nothing.
- Produces: the real v6 numbers that Task 6's token table uses.

- [ ] **Step 1: Print each control's rect**

In the child-enumeration loop that currently prints class name and text, add the window rect converted to client coordinates of the settings window, and print `w x h`. Also print the settings window's own `GetDpiForWindow`, and for the `SysListView32` send `LVM_GETITEMRECT` for item 0 (`LVIR_BOUNDS`) and `LVM_GETCOUNTPERPAGE`.

Keep the existing output lines exactly as they are — they are what the probe's PASS/FAIL is read from.

- [ ] **Step 2: Run it on a14**

Route recorded in the measurements file and in `examples/windows/serve/README.md`:

```bash
# from the dev Mac -- ~/.ssh/config's colima Include is TCC-blocked
grep -v -i '^Include' ~/.ssh/config > /tmp/sshcfg
# quoting dies through plain -Command; encode instead
ENC=$(iconv -f UTF-8 -t UTF-16LE probe.ps1 | base64 | tr -d '\n')
ssh -F /tmp/sshcfg a14 "powershell -NoProfile -NonInteractive -EncodedCommand $ENC"
```

The probe itself must run in **session 1** via a scheduled task registered
with a **SID** principal (not `DOMAIN\user`: a non-domain-joined machine fails
with "No mapping between account names and security IDs was done") and
`New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries` (the default refuses
to start on battery and leaves the task `Queued` forever on a laptop).

Start `beckon-serve.exe` first — `settings_probe` needs the tray window and
reports `FAIL: no beckon-serve-tray window` without it.

- [ ] **Step 3: Record what came back**

Append to the measurements file: ListView row height and header height, a
themed `BUTTON`'s height, an `EDIT`'s, the `COMBOBOX`'s closed height, and
`GetDpiForWindow` for the settings window. If the display is still at 100 %,
say so — the numbers are then the 96-DPI base, which is exactly what the
tokens are defined at, and the 150 % check stays owed.

- [ ] **Step 4: Commit**

```bash
git add crates/beckon-windows/examples/settings_probe.rs docs/superpowers/measurements/
git commit -m "test(windows): settings_probe prints control rects

Landing 2a's spacing tokens are guesses without them, and the probe was
the only thing that can see a themed control inside a manifested
process -- a PowerShell process is not manifested, so controls created
there are v5 and answer the wrong question."
```

---

### Task 3: `Severity`, and splitting `Mark::Unknown`

`Mark::Unknown` currently means three different things at once — "not registered yet", "the catalog scan has not finished", and "beckon is paused" — and the window has nowhere that says beckon is paused. And every problem blocks Save, so an unfinished new row disables saving the entire file.

**Files:**
- Modify: `crates/beckon-core/src/settings.rs`
- Test: in-file `mod tests`

**Interfaces:**
- Consumes: `RuntimeStatus` as it stands.
- Produces:
  - `pub enum Severity { Error, Warning }`
  - `pub struct Problem { pub row: Option<usize>, pub severity: Severity, pub message: String }`
  - `pub enum Mark { Ok, Bad, Warn, Unknown }`
  - `RuntimeStatus.paused: bool`
  - `ControlState.items[i].flag: Option<String>` — the short word beside the app name, `None` on a healthy row

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn a_healthy_row_carries_no_flag_at_all() {
    let cs = control_state(&model(), &status_all_ok());
    assert_eq!(cs.items[0].flag, None, "healthy rows must be silent");
}

#[test]
fn a_warning_does_not_block_saving_the_rest_of_the_file() {
    let mut m = model();
    m.set_app(0, "Windows Terminal");
    m.add_row();                       // neutral, not an error
    let cs = control_state(&m, &status_all_ok());
    assert!(
        cs.apply_enabled,
        "an unfinished new row must not disable Save for edits made elsewhere"
    );
}

#[test]
fn an_error_still_blocks_saving() {
    let mut m = model();
    m.set_combo(0, "bad+++");
    assert!(!control_state(&m, &status_all_ok()).apply_enabled);
}

#[test]
fn paused_is_its_own_word_and_not_unknown() {
    let st = RuntimeStatus { paused: true, ..status_all_ok() };
    let cs = control_state(&model(), &st);
    assert_eq!(cs.items[0].flag.as_deref(), Some("paused"));
}

#[test]
fn a_scan_still_running_is_not_the_same_as_an_app_that_is_missing() {
    let mut m = model();
    m.selected = Some(0);
    let scanning = RuntimeStatus { catalog: None, ..status_all_ok() };
    let cs = control_state(&m, &scanning);
    assert_eq!(cs.items[0].flag, None, "a scan in progress is not a row problem");
    let note = cs.detail.unwrap().notes.into_iter()
        .find(|n| n.text.contains("Checking")).expect("the editor says so instead");
    assert_eq!(note.mark, Mark::Unknown);
}

/// The list mark and the editor note are computed by ONE function, so they
/// cannot contradict each other -- which they can today.
#[test]
fn the_list_and_the_editor_cannot_disagree_about_a_row() {
    let mut m = model();
    m.set_app(0, "Nonexistent App");
    m.selected = Some(0);
    let cs = control_state(&m, &status_all_ok());
    assert!(cs.items[0].flag.is_some(), "the list must show the problem");
    let notes = cs.detail.unwrap().notes;
    assert!(notes.iter().any(|n| n.mark == Mark::Bad),
        "and the editor must agree it is one");
}
```

- [ ] **Step 2: Run them and watch them fail**

`cargo test -p beckon-core settings::` — expect compile errors on `flag`, `paused` and `Severity`.

- [ ] **Step 3: Implement**

Add `Severity`, give `Problem` its `severity` and an `Option<usize>` row (file-scope problems have no row). Add `paused` to `RuntimeStatus`. Extract a single function

```rust
/// The one place a row's condition is decided. Both the list flag and the
/// editor's notes are derived from it, so they cannot contradict each other
/// -- which they could when `items` read only the registration map and
/// `detail` read the catalog as well.
fn row_condition(
    m: &Model, i: usize, rt: &RuntimeStatus, problems: &[Problem],
) -> (Mark, Option<String>, Vec<Note>) { … }
```

and have `control_state` call it for every row. The vocabulary, verbatim:

| Condition | `flag` | Note in the editor |
|---|---|---|
| registered and the app resolves | `None` | `Registered and working.` |
| `RegisterHotKey` failed | `Some("key in use")` | `Another program already has this shortcut.` |
| the app is not in a finished catalog | `Some("not installed")` | `No installed app has this name.` |
| catalog still scanning | `None` | `Checking installed apps...` |
| beckon is paused | `Some("paused")` | `beckon is paused, so no shortcut is active.` |
| a new row with nothing set | `None` | `Pick a key and an app.` |
| combo is not the Caps chord | `Some("custom")` | `Uses a different chord.` |

`apply_enabled = m.dirty() && !problems.iter().any(|p| p.severity == Severity::Error)`.

- [ ] **Step 4: Tests pass, then MACCHECK and commit**

```bash
cargo fmt --all
cargo test   --workspace --exclude beckon-linux --exclude beckon-windows
cargo clippy --workspace --exclude beckon-linux --exclude beckon-windows --all-targets -- -D warnings
git add crates/beckon-core/src/settings.rs
git commit -m "feat(core): healthy rows say nothing, and paused is its own word

Mark::Unknown meant three things at once -- not registered, catalog
still scanning, and beckon paused -- and the window had nowhere that
said paused at all. Every problem also blocked Save, so an unfinished
new row disabled saving edits made elsewhere in the file.

One function now decides a row's condition and feeds both the list flag
and the editor's notes, so they cannot contradict each other. They can
today: `items` reads only the registration map while `detail` also reads
the catalog, so a row can say OK in the list and \"no installed app has
this name\" directly below it."
```

---

### Task 4: `Row.marked` and multi-row delete

**Files:**
- Modify: `crates/beckon-core/src/settings.rs`
- Test: in-file

**Interfaces:**
- Produces: `Row.marked: bool`, `Model::set_marked(&mut self, i: usize, on: bool)`, `Model::marked_count(&self) -> usize`, `Model::remove_marked(&mut self)`.

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn marking_a_row_is_not_a_file_change() {
    let mut m = model();
    m.set_marked(0, true);
    assert!(!m.dirty(),
        "a tick changes nothing on disk; making it dirty would enable Save \
         for an empty edit and rewrite the file unchanged");
    assert!(!control_state(&m, &status_all_ok()).apply_enabled);
}

#[test]
fn removing_marked_rows_removes_all_of_them() {
    let mut m = Model::from_text(
        "\"ctrl+alt+a\"=\"A\"\n\"ctrl+alt+b\"=\"B\"\n\"ctrl+alt+c\"=\"C\"\n").unwrap();
    m.set_marked(0, true);
    m.set_marked(2, true);
    assert_eq!(m.marked_count(), 2);
    m.remove_marked();
    let apps: Vec<&str> = m.rows.iter().map(|r| r.app.as_str()).collect();
    assert_eq!(apps, vec!["B"], "index shifting must not drop the wrong row");
    assert!(m.dirty());
}

#[test]
fn an_external_reload_drops_the_marks() {
    let mut m = model();
    m.set_marked(0, true);
    let reloaded = Model::from_text(&m.render().unwrap_or_else(|_| FILE.into())).unwrap();
    assert!(!reloaded.rows[0].marked, "marks are UI state, never file state");
}
```

- [ ] **Step 2: Watch them fail, then implement**

`remove_marked` must iterate **in reverse index order**, or removing row 0 shifts row 2 and the second removal takes the wrong one — which is exactly what the second test pins. `RowWrite` does **not** gain a field: marks are UI state, and `Model::from_text` defaults them to `false`.

- [ ] **Step 3: MACCHECK and commit**

```bash
git commit -m "feat(core): tick rows to delete several at once

set_marked deliberately does not set dirty: a tick changes nothing on
disk, and apply_enabled is dirty && valid, so marking would light up
Save for an empty edit and rewrite the file byte-identical.

remove_marked walks in reverse index order. Forward, removing row 0
shifts row 2 down and the second removal takes the wrong row."
```

---

### Task 5: The ListView — checkboxes, and stop rebuilding on every keystroke

**Files:**
- Modify: `crates/beckon-windows/src/settings_window.rs`

**Interfaces:**
- Consumes: `Row.marked` (Task 4), `ControlState.items[i].flag` (Task 3), the `suppressed()` guard (Task 1).

- [ ] **Step 1: Enable checkboxes and keep single-select**

Add `LVS_EX_CHECKBOXES` to the existing `LVM_SETEXTENDEDLISTVIEWSTYLE` call. **Keep `LVS_SINGLESEL`** — check state is independent of selection, so multi-delete works without multi-select and the editor keeps having exactly one current row. **Never set `LVS_EX_AUTOCHECKSELECT`**, which ties checking to selection and reintroduces the ambiguity being avoided.

**Do not port `ListView_GetCheckState`.** It is `(state >> 12) - 1` on an unsigned value, so an item never given a state image returns `0xFFFFFFFF`, not `0`. Read with `LVM_GETITEMSTATE` masked by `LVIS_STATEIMAGEMASK` and compare against `0x2000`.

- [ ] **Step 2: Set check state as part of the insert, not afterwards**

Insert items with `mask: LVIF_TEXT | LVIF_STATE`, `stateMask: LVIS_STATEIMAGEMASK`, `state: (k << 12)` where `k` is 1 (unchecked) or 2 (checked). The first `LVN_ITEMCHANGED` after each insert otherwise looks exactly like a user toggle (state image 0 → 1).

- [ ] **Step 3: Distinguish a check from a selection**

Both arrive as `LVN_ITEMCHANGED` with `LVIF_STATE`, and `uChanged` cannot separate them. Test the bits independently — never `else if`, because both can change in one message:

```rust
                    let changed = lv.uOldState ^ lv.uNewState;
                    if changed & LVIS_STATEIMAGEMASK.0 != 0 {
                        let on = (lv.uNewState & LVIS_STATEIMAGEMASK.0) == 0x2000;
                        with_cb(|cb| (cb.on_mark)(lv.iItem as usize, on));
                    }
                    if changed & LVIS_SELECTED.0 != 0
                        && lv.uNewState & LVIS_SELECTED.0 != 0
                    {
                        with_cb(|cb| (cb.on_select)(lv.iItem as usize));
                    }
```

- [ ] **Step 4: Diff instead of rebuild**

Cache the last-pushed `Vec<ListItem>` in `Ui`. When the item count is unchanged — which is every text edit — send `LVM_SETITEMTEXTW` only for cells whose text actually differs, and `LVM_SETITEMSTATE` only where check or selection differs. **Never `LVM_DELETEALLITEMS`.** Count is the discriminator that keeps this trivial: no keyed reconciliation, no ids in `LVITEM.lParam`.

Only Add, Remove and reload change the count, and only they rebuild. On that path, restore scroll with a **pair** of `LVM_ENSUREVISIBLE`: read `LVM_GETTOPINDEX` and `LVM_GETCOUNTPERPAGE` first, then ensure `min(top + per - 1, count - 1)` and then `top`. A single `ENSUREVISIBLE(top)` is a no-op because after a rebuild `top` is already on screen. Wrap the rebuild in `WM_SETREDRAW`.

- [ ] **Step 5: Restore the selection highlight**

A pre-existing defect this pass must fix: `LVM_DELETEALLITEMS` clears `LVIS_SELECTED` from every item and the reinsert loop sets `mask: LVIF_TEXT` only, so nothing puts it back — typing one character into the App field loses the highlight while `Model.selected` still says otherwise.

- [ ] **Step 6: WINCHECK and commit**

```bash
git commit -m "feat(windows): tick rows, and stop rebuilding the list per keystroke

apply_state deleted and reinserted every item on every refresh. With
checkboxes that wipes the ticks and with a scrollbar it jumps to the
top -- on every character typed. The item count is unchanged for every
text edit, which makes the diff trivial: set only the cells whose text
actually differs and nothing is ever destroyed, so neither scroll
position nor check state can be disturbed.

Also restores the selection highlight, which DELETEALLITEMS has been
clearing and the reinsert loop never put back."
```

---

### Task 6: The five-band layout

**Files:**
- Modify: `crates/beckon-windows/src/settings_window.rs` (`build_children`, `layout`)

**Interfaces:**
- Consumes: Task 2's measured v6 heights; Task 3's `flag`.

The bands, top to bottom: **(1)** the external-change banner, only when needed; **(2)** a section head — `Shortcuts`, a filter `EDIT`, `Remove N`, `+ Add`; **(3)** the list, fixed height, scrolling internally; **(4)** the editor strip, one line; **(5)** the suggestion row, empty in 2a. Then the command bar: `Open config file` … `Close` `Save`.

Columns: **`App` first**, taking the remaining width; **`Shortcut`** right-aligned at a fixed width. The status column is deleted — Task 3's `flag` renders beside the app name.

Tokens, all through `scale(v, dpi)`, **replacing the values below with Task 2's measurements where they disagree**:

| Token | 96-DPI value |
|---|---|
| Surface padding | 16 |
| Band gap | 14 |
| Control gap | 8 |
| Control height | whatever Task 2 measured for a themed `BUTTON` |
| List row height | whatever Task 2 measured; the list height is a whole multiple of it |
| Minimum button width | 88 |
| Rows visible | 8 |

- [ ] **Step 1: Create children in visual order**

The banner's `Reload` / `Keep mine` are created last today, so the one pair that responds to an urgent event sits at the **end** of the Tab order. Create them in the order they appear.

- [ ] **Step 2: Rebuild `layout`**

Keep `clamp` on every computed height *and width* — Landing 1 established that minimize fires `WM_SIZE` with a 0×0 client rect, so every subtraction goes negative on every machine.

- [ ] **Step 3: WINCHECK, run `settings_probe` on a14, and commit**

The probe must still find the window and its controls. If it reports `FAIL`, that is the layout, not the probe.

---

### Task 7: The type ramp

**Files:**
- Modify: `crates/beckon-windows/src/settings_window.rs`

Three roles: **Subtitle** 20 semibold (band headings), **Body** 14 (list, fields, buttons), **Caption** 12 (flags, notes). Faces are **probed, not requested** — GDI's font mapper never fails, it substitutes silently, so each candidate is confirmed with a `SelectObject` → `GetTextFace` round trip and falls back to `lfMessageFont`.

**Use the measured names.** `Segoe UI Variable Text` / `Small` / `Display` resolve exactly. **Do not ask for `Segoe UI Variable Text Semib`** — measured, it returns Arial; the full name is `Segoe UI Variable Text Semibold`, while the Display and Small variants really are truncated. All three fonts must be `DeleteObject`ed on `WM_DESTROY` and rebuilt on `WM_DPICHANGED`, the same discipline Landing 1 established for the single font.

---

### Task 8: Colour from `GetSysColor`, and the change handlers

Not one literal colour. `COLOR_BTNFACE`, `COLOR_WINDOW`, `COLOR_WINDOWTEXT`, `COLOR_GRAYTEXT`, `COLOR_HIGHLIGHT`, `COLOR_HIGHLIGHTTEXT`. This is the necessary and sufficient condition for Windows' high-contrast themes — which *are* the supported dark path — because contrast guarantees only hold when foreground and background come from the same palette.

New arms: `WM_SYSCOLORCHANGE`, `WM_THEMECHANGED`, `WM_SETTINGCHANGE` with `SPI_SETHIGHCONTRAST` — invalidate and re-read.

**Do not add a `WM_CTLCOLORSTATIC` handler returning `GetSysColorBrush(COLOR_BTNFACE)`.** `DefWindowProcW` already returns that brush and the class background is the same brush; it is a no-op. This was checked and refuted in the spec (§7.7).

---

### Task 9: The command bar, mnemonics, and the smaller repairs

- **Save moves to the command bar** as the default button beside Close, with `Open config file` on the far left. Today `Apply` sits mid-window sharing a row with `Remove` — a destructive button with no confirm and no undo is the visual peer of the one that writes to disk — while the bottom bar holds only `Close`, so people aim there, press Close, and the save prompt becomes the real save path.
- **`&` mnemonics on every button, `Ctrl+S` and Enter to save.** No label has one today, there is no accelerator table, and `Apply` carries `BS_DEFPUSHBUTTON` — which *promises* Enter — while the window is not a dialog and does not handle `DM_GETDEFID`, so Enter does nothing. Esc and Close must keep going through the same `WM_CLOSE` path so the save prompt is asked exactly once.
- **Title bar** reads `beckon — shortcuts.toml`, prefixed with `*` when dirty (ASCII, per the constraint), full path in the tooltip.
- **Notes get `SS_NOPREFIX` and `SS_ENDELLIPSIS`.** Without `SS_NOPREFIX` an app Name containing `&` renders wrong, and Start Menu names really do contain them.
- **Close the radio/checkbox group** with `WS_GROUP` on the following control; only one control has it today, so arrow navigation runs off the end of the group into the command bar.

---

### Task 10: An unparseable file opens read-only, and the App combo box stops lying

**The read-only path.** `open_settings` currently refuses with *"Fix it in a text editor first"* — precisely the moment a non-developer most needs the GUI. Open, state the parse error in plain language with the offending line, offer `Open config file`, and keep editing disabled until it parses. beckon still never writes over something it does not understand.

**The App combo box.** Typing "Notepad" wrote `"d"` to the config while the screen showed "Debuggable Package Manager" — measured on a14. `commit_fields` papers over it at Apply time; the field itself still lies while you type. This is the last remaining instance of a defect the rest of the design engineered out, and the spec makes it a gate for this landing.

> **CORRECTED 2026-08-11.** This paragraph used to name the cause: *"A populated `CBS_DROPDOWN` rewrites its own text as you type, and the `CBN_EDITCHANGE` that arrives carries the text from before the rewrite."* That is false. It was inferred from the symptom, never measured, and the fix it produced (deferring the read) failed on hardware in exactly the same shape — the read was always returning the right text. `combo_probe` on a14 (comctl32 6.16, 121 items, session 1, real `SendInput`) shows a `CBS_DROPDOWN` rewriting nothing while you type: `CB_GETCURSEL` stays -1 and the child EDIT sees only `WM_KEYDOWN`/`WM_CHAR`. The rewrite happens on **resize** — the control re-synchronises its edit to the closest catalogue item and selects the whole string — and `apply_state` was calling `layout` on every keystroke. Fixed by `Ui::shown_external` + `Ui::shown_empty`; see spec §7.15 and `docs/superpowers/measurements/2026-08-11-landing-1-a14.md` §24–26.

Keep `CBS_DROPDOWN`, not `DROPDOWNLIST` — beckon deliberately supports apps with no Start Menu entry, so free typing must survive the catalog loading.

---

## Self-review

**Spec coverage (Landing 2a = Part B + the rendering half of Part C + §F.7):**

| Spec section | Task |
|---|---|
| §F.7 `suppressed()` guard | 1 |
| §F.7 checkboxes, diff, scroll/selection | 5 |
| §B.5 status vocabulary, §B.6 `Severity` | 3 |
| `Row.marked`, multi-delete | 4 |
| §B.1 five bands, §B.2 tokens | 6 |
| §B.3 type ramp | 7 |
| §B.4 colour | 8 |
| §B.7 command bar, mnemonics, title, notes | 9 |
| §B.7 read-only mode, App combo fix | 10 |
| Measurement that 2a's tokens depend on | 2 |

**Shipped narrower than the table reads, on purpose (recorded 2026-08-12, after the branch review):** band 2 above says `Shortcuts`, a filter `EDIT`, `Remove N`, `+ Add`. **Neither the filter `EDIT` nor the `Remove N` caption shipped.** The filter was split out and the branch is coherent without it — an eight-row list over a config of that size does not need one. The count in the caption was dropped because `layout` sizes every button from `text_size` of its own caption, so a caption that grows with the tick count becomes another `layout` input, and honouring it on a data push means `SetWindowPos` on the populated App combo — the measured data-loss path `Ui::shown_external` exists to close. The multi-delete itself did ship: `Model::remove_pressed`, ticks over selection.

**Not here, by design:** capture, the availability probe, and the Caps `Hold`/`Tap` row — Landing 2b. Suggestions — Landing 3.

**The one thing this plan cannot fix:** Task 2 may come back saying the display is still at 100 %, in which case the tokens are correct at their base DPI and the 150 % behaviour stays owed. Do not let that block the landing; record it and move on.
