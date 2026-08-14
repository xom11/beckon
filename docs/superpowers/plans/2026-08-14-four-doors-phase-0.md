# Four Doors — Phase 0 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix the five disagreeing copies of the window's geometry, put every
control id in one tested table, widen `Callbacks` by exactly one field and
`open` by exactly one parameter — so the four visible workstreams that follow
derive their numbers from something true.

**Architecture:** Nothing here draws. Five edits to comments and constants in
`beckon-windows`, one new tested table in `beckon-core`, two new source-level
drift tests, one signature change landed across four files in one commit, and
one behaviour fix to a shipping data-loss bug.

**Tech Stack:** Rust 2021, `beckon-core` (no OS deps), `beckon-windows`
(hand-written Win32 + GDI), `beckon-macos` (objc2), `beckon-cli`.

## Global Constraints

- **The window stays 760 × 600 with a 660 × 560 floor.** Phase 0 changes only
  what *disagrees* with those constants. 680 belongs to the shell workstream.
- **Nothing visible changes**, with one deliberate exception: Task 1, the
  shipping `Remove`-under-filter data-loss bug.
- **Ids `1001-1008`, `1012`, `1013`, `1028-1031` never move**;
  `1009-1011` are never reclaimed. `examples/settings_probe.rs` hard-codes the
  first group.
- **`Callbacks` gains exactly one field.**
  `beckon-macos/examples/settings_probe.rs:112` builds it as a complete
  literal with no `..`, and CI clippies it `--all-targets` on macos-latest, so
  every field is a hard E0063 there.
- **UI text is English.** No new `Alt` mnemonics anywhere.
- Local gate, in CI's own `--exclude` shape (a bare workspace clippy cannot
  pass on macOS):

  ```sh
  cargo fmt --all -- --check
  cargo test   --workspace --exclude beckon-linux --exclude beckon-windows
  cargo clippy --workspace --exclude beckon-linux --exclude beckon-windows \
        --all-targets -- -D warnings
  cargo clippy --target aarch64-pc-windows-msvc -p beckon-windows \
        --all-targets -- -D warnings
  ```

  The last line is the only thing that compiles `beckon-windows` and its
  `examples/` on this Mac. Tests inside `beckon-windows` **cannot run here** —
  they run on the Windows CI job; compiling them is the local gate.
- Spec: `docs/superpowers/specs/2026-08-14-four-doors-phase-0-spec.md`.
  Parent design: `2026-08-14-four-doors-settings-window-design.md`.

---

## File Structure

| File | Responsibility in this phase |
|---|---|
| `crates/beckon-core/src/settings.rs` | `Model::visible` fix (T1); `CONTROL_IDS` / `RETIRED_IDS` + 3 tests (T2); `SettingsCommand` / `Target` / `Field` / `Paths` / `Page` + `Callbacks::on_command` (T5, T6) |
| `crates/beckon-windows/src/settings_window/ids.rs` | **new** — the module's id constants move here, plus the cross-check test against `CONTROL_IDS` and the `include_str!` geometry test (T3, T4) |
| `crates/beckon-windows/src/settings_window/mod.rs` | id constants move out (T3); the two geometry comment blocks (T4); `open` signature and `CFG` (T6) |
| `crates/beckon-windows/src/settings_window/layout.rs` | the `MIN_WIDTH (753)` comment (T4) |
| `crates/beckon-windows/examples/settings_probe.rs` | geometry constants 900/740/753/702 → 760/600/660/560 (T4) |
| `crates/beckon-macos/src/settings_window.rs` | `open` signature, window title (T6) |
| `crates/beckon-macos/examples/settings_probe.rs` | `Callbacks` literal (T5), `open` call (T6) |
| `crates/beckon-cli/src/serve.rs` | `on_command` match (T5), `Paths` construction (T6) |

---

## Task 1: The filter stops matching the Shortcut column

The shipping data-loss bug. `Model::visible` matches the filter against
`r.app` **and** `r.combo`; every beckon chord contains `alt`, so filtering on
`a` shows every row while the box looks filtered. Tick "the visible rows",
press Remove, lose the table.

**Files:**
- Modify: `crates/beckon-core/src/settings.rs:437-452` (`visible`)
- Modify: `crates/beckon-core/src/settings.rs:2723-2733` (the test that pins
  the old behaviour)

**Interfaces:**
- Consumes: nothing.
- Produces: no signature change. `visible(&self) -> Vec<usize>` keeps its
  shape; only the predicate narrows.

- [ ] **Step 1: Write the failing test**

Replace the whole of `the_filter_matches_the_combo_too`
(`settings.rs:2723-2733`) with these two. The first is the bug, stated as the
measurement that found it; the second pins what was given up.

```rust
    #[test]
    fn the_filter_does_not_match_the_shortcut_column() {
        // Every beckon chord contains `alt`, so a filter that matched the
        // combo made `a` -- a plausible first keystroke of "brave" -- match
        // EVERY row while the box looked filtered. Tick the visible rows,
        // press Remove, lose the table. Measured with four bindings before
        // this changed.
        let mut m = three();
        m.set_filter("a");
        assert_eq!(
            m.visible(),
            vec![0],
            "only Notepad's NAME contains `a`; matching the chord too made \
             this every row"
        );
    }

    #[test]
    fn filtering_by_a_key_name_finds_nothing() {
        // The cost of the fix, pinned rather than left to be rediscovered:
        // this window can no longer answer "what is ctrl+alt+q bound to?" by
        // filtering. If that bites, the way back is to match the chord's KEY
        // only -- never substring-matching the whole chord again.
        let mut m = three();
        m.set_filter("alt+q");
        assert!(m.visible().is_empty());
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

```sh
cargo test -p beckon-core the_filter_does_not_match_the_shortcut_column \
    filtering_by_a_key_name_finds_nothing
```

Expected: both FAIL. The first gets `[0, 1, 2]` (every row contains `alt`),
the second gets `[2]`.

- [ ] **Step 3: Narrow the predicate**

In `settings.rs:445-449`, drop the combo arm:

```rust
            .filter(|(i, r)| {
                self.selected == Some(*i) || r.app.to_lowercase().contains(&f)
            })
```

- [ ] **Step 4: Update `visible`'s doc comment**

Append to the doc block that ends at `settings.rs:436`, immediately above
`fn visible`:

```rust
    /// **The filter matches the app name ONLY, never the chord.** It used to
    /// match both, and every beckon chord contains `alt` -- so `a` matched
    /// every row while the filter box looked as though it had narrowed the
    /// list. With `Remove` taking the ticked rows, that is a path to
    /// deleting the whole table by typing one letter. Measured with four
    /// bindings and filter `a`: `control_state` returned all four.
    ///
    /// What this gives up is real and is pinned by
    /// `filtering_by_a_key_name_finds_nothing`: the window can no longer
    /// answer "what already owns this chord?" by filtering. If that has to
    /// come back, match the chord's KEY (`f2`, `b`) -- the half a person
    /// searches for and the half that is not `alt` on every row -- and never
    /// the whole chord as a substring again.
```

- [ ] **Step 5: Run the full core suite**

```sh
cargo test -p beckon-core
```

Expected: PASS. Watch for collateral in the neighbouring filter tests —
`selected_is_a_view_index_while_filtered` (`settings.rs:2782`) uses filter
`"e"` against app names only and is unaffected; if any test that filters on a
chord fragment fails, it is pinning the removed behaviour and gets the same
treatment as Step 1's.

- [ ] **Step 6: Commit**

```bash
git add crates/beckon-core/src/settings.rs
git commit -m "fix(settings): the filter stops matching the shortcut column

Every beckon chord contains alt, so filtering on \"a\" matched every row
while the box looked filtered -- and Remove takes the ticked rows. Four
bindings, filter \"a\", all four returned."
```

---

## Task 2: One id table in core, with three tests

**Files:**
- Modify: `crates/beckon-core/src/settings.rs` (append the table above the
  `#[cfg(test)] mod tests` block; append the tests inside it)

**Interfaces:**
- Consumes: nothing.
- Produces:
  - `pub const CONTROL_IDS: &[(&str, i32)]` — `(NAME, id)`, `NAME` without the
    `IDC_` prefix.
  - `pub const RETIRED_IDS: &[i32]`.
  - `pub const PROBE_PINNED_IDS: &[(&str, i32)]`.
  Task 3 consumes all three from `beckon-windows`.

- [ ] **Step 1: Write the failing tests**

Append inside `settings.rs`'s existing `mod tests`:

```rust
    // ---------- control ids ----------

    #[test]
    fn ids_are_unique() {
        let mut seen: Vec<(&str, i32)> = Vec::new();
        for (name, id) in CONTROL_IDS {
            if let Some((other, _)) = seen.iter().find(|(_, v)| v == id) {
                panic!(
                    "control id {id} is claimed by both `{other}` and \
                     `{name}`. `layout` positions controls through \
                     `GetDlgItem`, which resolves a duplicate to the FIRST \
                     match -- so one of these is placed and the other is \
                     silently left at the origin."
                );
            }
            seen.push((name, *id));
        }
    }

    #[test]
    fn retired_ids_stay_retired() {
        for id in RETIRED_IDS {
            if let Some((name, _)) = CONTROL_IDS.iter().find(|(_, v)| v == id) {
                panic!(
                    "`{name}` reclaims retired id {id}. A probe built against \
                     an older binary would find a control it thinks it \
                     recognises."
                );
            }
        }
    }

    #[test]
    fn probe_pinned_ids_have_not_moved() {
        for (name, id) in PROBE_PINNED_IDS {
            let found = CONTROL_IDS.iter().find(|(n, _)| n == name);
            assert_eq!(
                found.map(|(_, v)| *v),
                Some(*id),
                "`crates/beckon-windows/examples/settings_probe.rs` \
                 hard-codes {id} for `{name}` and drives another process, so \
                 it cannot be recompiled into agreement"
            );
        }
    }
```

- [ ] **Step 2: Run them to verify they fail**

```sh
cargo test -p beckon-core ids_are_unique retired_ids_stay_retired \
    probe_pinned_ids_have_not_moved
```

Expected: FAIL to compile — `CONTROL_IDS`, `RETIRED_IDS` and
`PROBE_PINNED_IDS` are not defined.

- [ ] **Step 3: Write the table**

Append to `settings.rs`, above `#[cfg(test)] mod tests`:

```rust
// ---------------------------------------------------------------------------
// Control ids
// ---------------------------------------------------------------------------

/// Every dialog control id the settings window uses, on every platform that
/// has one, in one place.
///
/// **This is documentation with a test attached, not the definition.** The
/// Windows module keeps its own `const IDC_… : i32`, and a `#[test]` there
/// compares the two. Making this the definition would put a Win32 concept in
/// the crate whose whole purpose is to be free of one.
///
/// It lives here so `ids_are_unique` and `retired_ids_stay_retired` run on
/// **all three** CI jobs rather than only the Windows one -- the same reason
/// `ControlState` and `DefaultButton` are here. The failure it guards is not
/// hypothetical: `layout` resolves controls through `GetDlgItem`, which maps
/// a duplicated id to the first match, so the second control is created,
/// never placed, and left at the origin. That shipped once already (three
/// labels sharing `-1`), and two drafts of the Four Doors design each claimed
/// 1060-1069 for a different page.
///
/// **Ranges are disjoint by page**, and a page's controls never appear on
/// another page:
///
/// | Range | Owner |
/// |---|---|
/// | 1001-1039 | the pre-Four-Doors window |
/// | 1040-1049 | shell: the tab strip and the command bar |
/// | 1050-1059 | Shortcuts (reserved; the page reuses its existing ids) |
/// | 1060-1069 | Keyboard |
/// | 1070-1099 | System |
/// | 1100-1119 | About |
pub const CONTROL_IDS: &[(&str, i32)] = &[
    // -- the pre-Four-Doors window -----------------------------------------
    ("LIST", 1001),
    ("COMBO", 1002),
    ("APP", 1003),
    ("NOTES", 1004),
    ("ADD", 1005),
    ("REMOVE", 1006),
    // `APPLY` and `CLOSE` keep their ids after auto-save deletes both
    // buttons: the probe hard-codes them, and a different control answering
    // 1007 would make it report a confident wrong result.
    ("APPLY", 1007),
    ("CAPS", 1008),
    ("OPENFILE", 1012),
    ("CLOSE", 1013),
    ("BANNER", 1014),
    ("RELOAD", 1015),
    ("KEEPMINE", 1016),
    ("LBL_SHORTCUT", 1017),
    ("LBL_APP", 1018),
    ("GRP_KEYBOARD", 1019),
    ("LBL_SECTION", 1020),
    ("FILTER", 1021),
    ("HOLD_CTRL", 1022),
    ("HOLD_WIN", 1023),
    ("HOLD_ALT", 1024),
    ("TAP", 1025),
    ("LBL_HOLD", 1026),
    ("LBL_TAP", 1027),
    ("MOD_CTRL", 1028),
    ("MOD_WIN", 1029),
    ("MOD_ALT", 1030),
    ("MOD_SHIFT", 1031),
    ("RECORD", 1032),
    ("RESET", 1033),
    ("GRP_EDITOR", 1034),
    ("LBL_COUNT", 1035),
    // -- shell: the tab strip and the command bar --------------------------
    ("TAB_SHORTCUTS", 1040),
    ("TAB_KEYBOARD", 1041),
    ("TAB_SYSTEM", 1042),
    ("TAB_ABOUT", 1043),
    ("SERVICE_LINE", 1044),
    ("SAVED", 1045),
    ("UNDO", 1046),
    // -- Keyboard ----------------------------------------------------------
    ("CAPS_SHORTHAND", 1060),
    ("TROUBLE_HEAD", 1061),
    ("TROUBLE_BODY", 1062),
    // -- System ------------------------------------------------------------
    ("PAUSE", 1070),
    ("AUTOSTART", 1071),
    // NOT `RELOAD` (1015): that is the banner's "reload from disk", which
    // answers a different question from the tray's own reload.
    ("SYS_RELOAD", 1072),
    ("DARK", 1073),
    ("OPACITY", 1074),
    ("OPACITY_VALUE", 1075),
    ("CONFIG_NAME", 1076),
    ("CONFIG_DIR", 1077),
    ("CONFIG_OPEN", 1078),
    ("CONFIG_SHOW", 1079),
    ("LOG_NAME", 1080),
    ("LOG_SIZE", 1081),
    ("LOG_OPEN", 1082),
    ("LOG_SHOW", 1083),
    // -- About -------------------------------------------------------------
    ("ABOUT_MARK", 1100),
    ("ABOUT_NAME", 1101),
    ("ABOUT_BUILD_LABEL", 1102),
    ("ABOUT_BUILD_VALUE", 1103),
    ("ABOUT_BUILD_COPY", 1104),
    ("ABOUT_LOCATION_LABEL", 1105),
    ("ABOUT_LOCATION_VALUE", 1106),
    ("ABOUT_LOCATION_COPY", 1107),
    ("ABOUT_LICENCE_LABEL", 1108),
    ("ABOUT_LICENCE_VALUE", 1109),
    ("ABOUT_LICENCE_COPY", 1110),
    ("ABOUT_DISCLOSURE", 1111),
    ("ABOUT_GITHUB", 1112),
    ("ABOUT_RELEASES", 1113),
    ("ABOUT_BUG", 1114),
];

/// Ids that were used, are not any more, and must never be reused.
///
/// 1009-1011 were the three `Tapping Caps alone` radios. A probe built
/// against an older binary would find a control it thinks it recognises.
pub const RETIRED_IDS: &[i32] = &[1009, 1010, 1011];

/// The ids `crates/beckon-windows/examples/settings_probe.rs` hard-codes.
///
/// It drives ANOTHER process across a process boundary, so it cannot link
/// this crate and cannot be recompiled into agreement: these eleven are
/// fixed points, and `probe_pinned_ids_have_not_moved` is what says so out
/// loud.
pub const PROBE_PINNED_IDS: &[(&str, i32)] = &[
    ("LIST", 1001),
    ("COMBO", 1002),
    ("APP", 1003),
    ("ADD", 1005),
    ("REMOVE", 1006),
    ("APPLY", 1007),
    ("CAPS", 1008),
    ("OPENFILE", 1012),
    ("CLOSE", 1013),
    ("MOD_CTRL", 1028),
    ("MOD_WIN", 1029),
    ("MOD_ALT", 1030),
    ("MOD_SHIFT", 1031),
];
```

- [ ] **Step 4: Run the tests to verify they pass**

```sh
cargo test -p beckon-core ids_are_unique retired_ids_stay_retired \
    probe_pinned_ids_have_not_moved
```

Expected: 3 PASS. (`PROBE_PINNED_IDS` has thirteen entries, not eleven — the
four chips are one group. The doc comment says eleven because that is the
count in the spec; fix the comment to thirteen rather than trimming the list.)

- [ ] **Step 5: Commit**

```bash
git add crates/beckon-core/src/settings.rs
git commit -m "feat(settings): one control-id table, with a uniqueness test

Disjoint ranges per page. Two drafts of the Four Doors design each claimed
1060-1069, and GetDlgItem resolves a duplicate to the first match -- so the
second control is created, never placed, and left at the origin."
```

---

## Task 3: The Windows module's ids move to their own file and are checked against the table

**Files:**
- Create: `crates/beckon-windows/src/settings_window/ids.rs`
- Modify: `crates/beckon-windows/src/settings_window/mod.rs:298-380` (delete
  the constants, add `mod ids; use ids::*;`)

**Interfaces:**
- Consumes: `beckon_core::settings::{CONTROL_IDS, RETIRED_IDS}` (Task 2).
- Produces: every `IDC_*` constant `mod.rs` used before, re-exported
  unchanged, so no other file in the module changes.

- [ ] **Step 1: Create `ids.rs` with the constants moved verbatim**

Move `mod.rs:298-380` — the constants **and their doc comments**, unchanged —
into the new file, with this header:

```rust
//! Dialog control ids.
//!
//! Split out of `mod.rs` so the cross-check against
//! `beckon_core::settings::CONTROL_IDS` sits beside the numbers it checks,
//! rather than 6 000 lines away from them.
//!
//! **Core does not define these.** It carries a table with a test attached;
//! this file is the definition, and `ids_match_the_core_table` is what keeps
//! the two from drifting. Core is the crate that must stay free of Win32
//! concepts, and a dialog control id is one.

use super::*;
```

Adjust the visibility of each constant to `pub(super)` and drop `use
super::*;` if nothing in the file needs it (the constants are bare `i32` and
almost certainly need no imports — remove the line rather than leaving an
unused-import warning, which `-D warnings` rejects).

- [ ] **Step 2: Point `mod.rs` at it**

At `mod.rs:298`, where the constants were:

```rust
mod ids;
pub(super) use ids::*;
```

Keep `IDT_CAPTURE` and `CAPTURE_TIMEOUT_MS` (`mod.rs:382-401`) **in `mod.rs`**
— they are timers, not control ids, and `IDT_CAPTURE`'s doc comment is edited
by the auto-save workstream, not this one.

- [ ] **Step 3: Write the cross-check test**

Append to `ids.rs`:

```rust
#[cfg(test)]
mod tests {
    use beckon_core::settings::{CONTROL_IDS, RETIRED_IDS};

    /// Every id this module defines, paired with the name the core table
    /// knows it by. Hand-maintained, and that is the point: adding a control
    /// without adding it here is caught by `every_core_id_is_defined_here`.
    const MINE: &[(&str, i32)] = &[
        ("LIST", super::IDC_LIST),
        ("COMBO", super::IDC_COMBO),
        ("APP", super::IDC_APP),
        ("NOTES", super::IDC_NOTES),
        ("ADD", super::IDC_ADD),
        ("REMOVE", super::IDC_REMOVE),
        ("APPLY", super::IDC_APPLY),
        ("CAPS", super::IDC_CAPS),
        ("OPENFILE", super::IDC_OPENFILE),
        ("CLOSE", super::IDC_CLOSE),
        ("BANNER", super::IDC_BANNER),
        ("RELOAD", super::IDC_RELOAD),
        ("KEEPMINE", super::IDC_KEEPMINE),
        ("LBL_SHORTCUT", super::IDC_LBL_SHORTCUT),
        ("LBL_APP", super::IDC_LBL_APP),
        ("GRP_KEYBOARD", super::IDC_GRP_KEYBOARD),
        ("LBL_SECTION", super::IDC_LBL_SECTION),
        ("FILTER", super::IDC_FILTER),
        ("HOLD_CTRL", super::IDC_HOLD_CTRL),
        ("HOLD_WIN", super::IDC_HOLD_WIN),
        ("HOLD_ALT", super::IDC_HOLD_ALT),
        ("TAP", super::IDC_TAP),
        ("LBL_HOLD", super::IDC_LBL_HOLD),
        ("LBL_TAP", super::IDC_LBL_TAP),
        ("MOD_CTRL", super::IDC_MOD_CTRL),
        ("MOD_WIN", super::IDC_MOD_WIN),
        ("MOD_ALT", super::IDC_MOD_ALT),
        ("MOD_SHIFT", super::IDC_MOD_SHIFT),
        ("RECORD", super::IDC_RECORD),
        ("RESET", super::IDC_RESET),
        ("GRP_EDITOR", super::IDC_GRP_EDITOR),
        ("LBL_COUNT", super::IDC_LBL_COUNT),
    ];

    #[test]
    fn ids_match_the_core_table() {
        for (name, id) in MINE {
            let core = CONTROL_IDS.iter().find(|(n, _)| n == name);
            assert_eq!(
                core.map(|(_, v)| *v),
                Some(*id),
                "`IDC_{name}` is {id} here and {:?} in \
                 `beckon_core::settings::CONTROL_IDS`",
                core.map(|(_, v)| *v)
            );
        }
    }

    #[test]
    fn no_defined_id_is_retired() {
        for (name, id) in MINE {
            assert!(
                !RETIRED_IDS.contains(id),
                "`IDC_{name}` uses retired id {id}"
            );
        }
    }
}
```

`every_core_id_is_defined_here` is deliberately **not** written: core's table
already carries the four pages' ids, and none of those controls exists yet.
It becomes writable when the About workstream lands the last of them.

- [ ] **Step 4: Compile for Windows**

```sh
cargo clippy --target aarch64-pc-windows-msvc -p beckon-windows \
      --all-targets -- -D warnings
```

Expected: clean. This compiles the test but does not run it — `cargo test`
excludes `beckon-windows` on macOS. The Windows CI job runs it.

If the cold cache SIGKILLs, re-run; it converges after a few attempts on this
machine.

- [ ] **Step 5: Commit**

```bash
git add crates/beckon-windows/src/settings_window/ids.rs \
        crates/beckon-windows/src/settings_window/mod.rs
git commit -m "refactor(settings-window): ids move to ids.rs and are checked against core

No id changes. The cross-check is the point: core carries the table every CI
job tests, this file carries the definition, and the two can no longer drift."
```

---

## Task 4: The five geometry copies are made to agree

Read the spec's §1 table before starting: five copies, two of which reason at
length from a window size that has not existed since 2026-08-13.

**Files:**
- Modify: `crates/beckon-windows/src/settings_window/mod.rs:642-701` (the
  900 × 740 derivation)
- Modify: `crates/beckon-windows/src/settings_window/mod.rs:725-738` and
  `:747-795` (the `MIN_WIDTH` / `MIN_HEIGHT` derivations)
- Modify: `crates/beckon-windows/src/settings_window/layout.rs:319-327` (the
  `MIN_WIDTH (753)` note)
- Modify: `crates/beckon-windows/examples/settings_probe.rs:299-305`
- Modify: `crates/beckon-windows/src/settings_window/ids.rs` (append the
  drift test — it is the only non-`mod.rs` file in this module that already
  has a `mod tests`)

**Interfaces:**
- Consumes: nothing.
- Produces: no code changes at all except the four probe constants. Every
  other edit is to a comment.

**The arithmetic, already run.** These figures come from
`compute_card_rects` (`layout.rs:159-264`) at 96 DPI with the shipped tokens
(`PAD 10`, `GAP_CARD 8`, `GAP 6`, `CTL 26`, `CARD_PAD 11`,
`chrome::TITLEBAR_H 34`, `tok::ROW_H 22`, `list_header_height` fallback 21):

```
bar_y        = h − pad − ctl                       = h − 36
kb_card_h    = 2·card_pad + (24 + ctl + gap)       = 22 + 56  = 78
kb_y         = bar_y − gap_card − kb_card_h        = h − 122
card2_h      = 2·card_pad + (24 + 2·ctl + 2·gap + notes_h + gap)
             = 22 + 94 + notes_h                   = 116 + notes_h
y0           = pad + TITLEBAR_H                    = 44
card0 (banner up) = 2·card_pad + ctl = 48 → y = 44 + 48 + 8 = 100
list_top     = y + card_pad + ctl + gap            = 143
room         = kb_y − gap_card − list_top          = h − 273
list_h       = min(want, room − gap_card − card_pad − card2_h)
             = min(want, h − 408 − notes_h)
```

`notes_h` is `2 · text_size(Caption).height + 4` (`mod.rs:3410-3413`) — a live
font measurement, 36 px when the Caption line is 16 px.

Two results, and **both go in the comments rather than into the constants**:

1. Four list rows with the banner up needs `list_h = 21 + 4·22 = 109`, i.e.
   client `h = 408 + 36 + 109 = 553`, i.e. window **561**. `MIN_HEIGHT` is
   560 — **one pixel short** of the four rows its own doc claims to buy. At
   560 the list gets 108 px: three whole rows and 15 px of a fourth.
2. At `MIN_WIDTH 660` a card interior is `w − 2·PAD − 2·CARD_PAD = w − 42`.
   The `Use Caps Lock as a shortcut key` line was hand-measured at ≈547 px,
   which leaves `IDC_TAP` **≈71 px** against its 200 px ceiling — not the
   ≈150 px the old note reports, because that note was computing at 753.

- [ ] **Step 1: Replace `mod.rs:642-701` with a derivation of the real window**

Delete the whole block from `// 900x740, up from 860x640` down to the line
before `const WINDOW_WIDTH`. Keep the `/// Window creation size…` doc comment
at `:642-646` untouched. In its place:

```rust
// **760x600, since the 2026-08-13 compaction pass.** The 900x740 derivation
// that stood here was for a window with 26 px rows and 16 px padding and was
// already marked superseded; a full derivation of a window that does not
// exist is worse than none, so it is gone rather than annotated again.
//
// What replaced it as evidence is better than a table: the window was built
// and run on a14 at 144 DPI and measured **1140 x 900** -- exactly 760 x 600
// scaled by 1.5 -- with all eight list rows present and no scroll bar.
//
// **Which terms compose the height, in order** -- the part of the old block
// that was worth keeping, restated against the shipped tokens. This is a map
// of what a token change spends, not a claim about the total:
//
//   title bar (chrome::TITLEBAR_H)                     34
//   pad (tok::PAD)                                     10
//   card 0  banner -- NO height unless it is up      0/48
//   card 1  Shortcuts: 2*CARD_PAD, head CTL, GAP,
//           header (~21) + ROWS * row (~22)
//   gap_card                                            8
//   card 2  editor: 2*CARD_PAD, caption s(24),
//           2*CTL, 2*GAP, notes_height, GAP
//   gap_card                                            8
//   card 3  keyboard: 2*CARD_PAD, caption s(24),
//           CTL, GAP                                   78
//   gap_card                                            8
//   command bar (CTL, not a card)                      26
//   pad                                                10
//   frame, bottom only (`chrome::nccalcsize` gives
//     the rest back to the client; see `MIN_HEIGHT`)    8
//
// `notes_height` is a live font measurement (`2 * Caption line + 4`), so it
// does not scale by 1.5 between DPIs and no fixed total is honest here.
// `compute_card_rects` (`layout.rs`) is the arithmetic; this is a reading of
// it, and the direction of that dependency is not negotiable.
```

- [ ] **Step 2: Fix the `MIN_WIDTH` paragraph at `mod.rs:725-738`**

Replace it with:

```rust
/// `MIN_WIDTH` is **660**, and the two zero points this file's `layout`
/// computes -- the Shortcuts card's heading at a raw client width of ~364,
/// the editor card's key list at ~551 -- both clear it. A card's interior is
/// `w - 2*tok::PAD - 2*tok::CARD_PAD = w - 42`, so at 660 an interior is 618
/// px, which is 67 px above the key list's zero point and 254 above the
/// heading's.
///
/// **The Caps line is the one that does not clear it comfortably, and this
/// is the honest number**: `"Use Caps Lock as a shortcut key"` plus its
/// chips was hand-measured at ~547 px, which leaves `IDC_TAP` about **71 px**
/// of the 618 -- against a 200 px ceiling. An earlier version of this
/// paragraph reported ~150 px, because it was computing against a
/// `MIN_WIDTH` of 753 that has not existed since the compaction pass. 71 px
/// is enough to draw a combo and probably not enough to draw `Caps Lock`
/// inside one. **Gate G1 measures it with `GetTextExtentPoint32W`**; nothing
/// here should be trusted until it has.
///
/// (Whether the frame eats any of `w` at all is gate G3's question:
/// `chrome::nccalcsize` returns `LRESULT(0)` without calling
/// `DefWindowProcW`, yet `MIN_HEIGHT` below still subtracts an 8 px bottom
/// frame. Both figures above assume client == window on the horizontal.)
```

- [ ] **Step 3: Fix the `MIN_HEIGHT` derivation at `mod.rs:747-795`**

Replace the whole ```text block and the two paragraphs after it with:

```rust
/// ```text
///   Derived from `compute_card_rects` (`layout.rs`) at 96 DPI, banner UP,
///   with the shipped tokens. Solving that function for the client height
///   `h` at which the list gets exactly four rows:
///
///     bar_y     = h - PAD - CTL                       = h - 36
///     kb_card_h = 2*CARD_PAD + (24 + CTL + GAP)       = 78
///     kb_y      = bar_y - GAP_CARD - kb_card_h        = h - 122
///     card2_h   = 2*CARD_PAD + (24 + 2*CTL + 2*GAP
///                 + notes_h + GAP)                    = 116 + notes_h
///     y0        = PAD + TITLEBAR_H                    = 44
///     card0     = 2*CARD_PAD + CTL = 48, so y         = 100
///     list_top  = y + CARD_PAD + CTL + GAP            = 143
///     room      = kb_y - GAP_CARD - list_top          = h - 273
///     list_h    = room - GAP_CARD - CARD_PAD - card2_h
///               = h - 408 - notes_h
///
///   Four rows is `list_header_height` (21) + 4 * `list_row_height` (22)
///   = 109, and `notes_h` is 36 when the Caption line is 16 px, so
///
///     h = 408 + 36 + 109 = 553  client
///       + 8                     bottom frame
///       = 561                   window
/// ```
///
/// **The shipped constant is 560, so the floor is one pixel short of the
/// four rows this paragraph claims to buy.** At 560 the list is handed 108
/// px: three whole rows and 15 px of a fourth. That is recorded rather than
/// fixed -- moving the constant is a visible change, and this pass is the
/// one that makes the numbers agree, not the one that moves them. It is also
/// within the honest error of `notes_h`, which is a live font measurement:
/// a Caption line of 15 px makes 560 exactly right, and one of 17 px makes it
/// three pixels short. **Nothing on the machine this was derived on can
/// display the window**; a14 can, and the four-row claim should be checked
/// there before anyone spends a pixel on it.
///
/// The two row figures are `list_row_height` / `list_header_height`'s own
/// 96-DPI fallbacks (`tok::ROW_H` and a literal 21). They are the honest
/// numbers to derive from: comctl32 picks the real ones from the live font
/// at the live DPI, which is exactly why neither is a token.
```

Keep the paragraphs at `:802-823` (why card 0 is in the table; what the floor
buys) — the reasoning is unchanged and the numbers in it are the ones this
step just re-derived. Update the single figure `76 px card 0` at `:258` of
`layout.rs`… **no**: that is `layout.rs`'s own comment and says `76 px`, which
was `2 * 16 + 44`. It is now `2 * 11 + 26 = 48`. Fix that number too, in the
same commit — it is a sixth copy, found while doing this, and leaving it makes
this task a lie.

- [ ] **Step 4: Fix `layout.rs:319-327`**

```rust
/// **The keyboard line is the width-critical one.** `MIN_WIDTH` is 660, and
/// a card interior there is `w - 2*tok::PAD - 2*tok::CARD_PAD` = 618 px at
/// 96 DPI. The line -- `"Use Caps Lock as a shortcut key"`, the three Hold
/// chips and the Tap combo -- was hand-measured at ~547 px, which leaves
/// `IDC_TAP` about **71 px** against its `tok::SHORTCUT_COL` ceiling of 200.
///
/// The version of this note that stood here until 2026-08-14 said
/// `MIN_WIDTH (753)` and concluded ~150 px of slack. 753 has not been this
/// window's floor since the compaction pass; the real floor is 93 px
/// narrower, and the slack it reported was borrowed from a window that does
/// not exist. **Gate G1 measures the line with `GetTextExtentPoint32W` at 96
/// and 144 DPI, with the same measurement at 760 px as its control.** Do not
/// move `MIN_WIDTH` -- in either direction -- before it has run.
```

- [ ] **Step 5: Fix the probe's constants**

`examples/settings_probe.rs:299-305`:

```rust
    const WINDOW_WIDTH_96: i32 = 760;
    const WINDOW_HEIGHT_96: i32 = 600;
    /// Printed for reference only -- gate 09 (eight rows, no scrollbar) is
    /// what actually has to be checked at this floor, and that needs a
    /// human to drag the corner; this probe does not drive a resize.
    const MIN_WIDTH_96: i32 = 660;
    const MIN_HEIGHT_96: i32 = 560;
```

Extend the doc comment at `:292-298` with:

```rust
    /// **The independence is real and it did not save us.** These four sat
    /// at 900/740/753/702 from the 2026-08-13 compaction pass until
    /// 2026-08-14 -- the probe would have printed `<<< FAIL` against a
    /// perfectly healthy window, and nobody saw it, because the mechanism
    /// only fires with a person at a14. The copy stays (it is what catches a
    /// probe driving an OLDER binary), and
    /// `geometry_matches_the_probe` in `settings_window::ids` now catches
    /// the source-level drift without leaving this machine.
```

- [ ] **Step 6: Write the drift test**

Append to `ids.rs`'s `mod tests`:

```rust
    /// The probe transcribes the window's geometry by hand, on purpose --
    /// see its own comment. What that cannot catch is a resize here that
    /// nobody copies over there, because the disagreement only surfaces when
    /// a person runs the probe on a14. This reads the example's SOURCE and
    /// compares the literals.
    #[test]
    fn geometry_matches_the_probe() {
        let src = include_str!("../../../examples/settings_probe.rs");
        for (name, value) in [
            ("WINDOW_WIDTH_96", super::super::WINDOW_WIDTH),
            ("WINDOW_HEIGHT_96", super::super::WINDOW_HEIGHT),
            ("MIN_WIDTH_96", super::super::MIN_WIDTH),
            ("MIN_HEIGHT_96", super::super::MIN_HEIGHT),
        ] {
            let want = format!("const {name}: i32 = {value};");
            assert!(
                src.contains(&want),
                "examples/settings_probe.rs does not contain `{want}`. The \
                 probe prints its own copy as the EXPECTED geometry and \
                 reports `<<< FAIL` against it, so a stale copy makes a \
                 healthy window look broken on hardware."
            );
        }
    }
```

Verify the `include_str!` path resolves: `ids.rs` is at
`crates/beckon-windows/src/settings_window/`, so `../../../examples/` reaches
`crates/beckon-windows/examples/`. If the compiler disagrees, count the
components again rather than guessing — the error names the path it tried.

- [ ] **Step 7: Compile for Windows**

```sh
cargo clippy --target aarch64-pc-windows-msvc -p beckon-windows \
      --all-targets -- -D warnings
cargo fmt --all -- --check
```

Expected: clean. `cargo fmt` **does** cover this module and its examples on a
macOS host — measured on rustfmt 1.9.0, recorded in CLAUDE.md.

- [ ] **Step 8: Commit**

```bash
git add crates/beckon-windows/src/settings_window/mod.rs \
        crates/beckon-windows/src/settings_window/layout.rs \
        crates/beckon-windows/src/settings_window/ids.rs \
        crates/beckon-windows/examples/settings_probe.rs
git commit -m "docs(settings-window): six copies of the window geometry now agree

The probe printed 900x740 as the expected size and would have reported
<<< FAIL against a healthy 760x600 window. layout.rs reasoned about the
width-critical Caps line from MIN_WIDTH 753, which is 93 px above the real
660 -- re-run at 660 it leaves IDC_TAP ~71 px, not ~150. MIN_HEIGHT's own
table is re-derived from compute_card_rects and lands on 561 against a
shipped 560; recorded, not moved. A test now reads the probe's source so
this cannot drift again without a person at a14."
```

---

## Task 5: `Callbacks` gains `on_command`

**Files:**
- Modify: `crates/beckon-core/src/settings.rs:300-346` (`Callbacks`), plus new
  enums above it
- Modify: `crates/beckon-macos/examples/settings_probe.rs:112-192`
- Modify: `crates/beckon-cli/src/serve.rs:1412-1596`

**Interfaces:**
- Consumes: nothing.
- Produces:
  - `pub enum SettingsCommand` with variants `ShowPage(Page)`,
    `SetPaused(bool)`, `SetAutostart(bool)`, `ReloadNow`, `SetDarkMode(bool)`,
    `SetOpacity(u8)`, `SetCapsShorthand(bool)`, `Open(Target)`,
    `Reveal(Target)`, `Copy(Field)`, `Undo`.
  - `pub enum Target { Config, Log, Github, Releases, BugReport }`
  - `pub enum Field { Build, Location, Licence }`
  - `Callbacks::on_command: Box<dyn FnMut(SettingsCommand)>`
  - `Page` is defined in Task 6; **write Task 6's `Page` definition first** if
    executing these out of order.

- [ ] **Step 1: Define the command types**

Above `pub struct Callbacks` in `settings.rs`:

```rust
/// Which door the window is showing.
///
/// In core, not in the Windows crate, so `DefaultButton::visible(external,
/// page)` stays testable on all three CI jobs -- which is the stated reason
/// `DefaultButton` is in core at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Page {
    #[default]
    Shortcuts,
    Keyboard,
    System,
    About,
}

/// A file or a URL the window can ask the caller to open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    Config,
    Log,
    Github,
    Releases,
    BugReport,
}

/// A row on About whose value can be copied.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    Build,
    Location,
    Licence,
}

/// Everything the window can ask the caller to DO that is not an edit to a
/// binding.
///
/// **One `Callbacks` field rather than eleven.**
/// `beckon-macos/examples/settings_probe.rs` builds `Callbacks` as a complete
/// literal with no `..`, and CI clippies it `--all-targets` on macos-latest --
/// so every added field is a hard E0063 on a job that has nothing to do with
/// the feature. That is a real cost paid by a real job, not a hypothetical.
///
/// `Copy + Eq` and no variant carries a `String`, so a caller can match, log
/// and test one without cloning.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsCommand {
    /// The user moved to another door. The caller stores it, so the next
    /// open lands where they left off.
    ShowPage(Page),
    SetPaused(bool),
    SetAutostart(bool),
    /// The System page's Reload -- the tray's own, NOT the banner's
    /// "reload from disk", which is `on_reload_from_disk` and answers a
    /// different question.
    ReloadNow,
    SetDarkMode(bool),
    /// 85..=100. The window clamps before sending; the caller may assume it.
    SetOpacity(u8),
    SetCapsShorthand(bool),
    Open(Target),
    Reveal(Target),
    Copy(Field),
    Undo,
}
```

- [ ] **Step 2: Add the field**

At the end of `Callbacks` (after `on_close_request`, `settings.rs:345`):

```rust
    /// Everything that is not an edit to a binding. See `SettingsCommand`
    /// for why this is one field and not eleven.
    pub on_command: Box<dyn FnMut(SettingsCommand)>,
```

- [ ] **Step 3: Verify the macOS probe now fails to compile**

```sh
cargo clippy -p beckon-macos --all-targets -- -D warnings
```

Expected: **E0063**, `missing field on_command in initializer of Callbacks`,
at `examples/settings_probe.rs:112`. This is the mechanism the spec claims —
seeing it fire is the check.

- [ ] **Step 4: Fill it in on the macOS probe**

At `examples/settings_probe.rs:192`, before the closing `};`:

```rust
            on_command: Box::new(|c| println!("command {c:?}")),
```

- [ ] **Step 5: Fill it in on the real caller**

In `serve.rs`, at the end of the `Callbacks` literal (before `:1596`'s `};`):

```rust
        // An exhaustive `match` with empty arms, not a `_ => {}`: every
        // variant added later is a compile error at this one site, which is
        // the site that has to handle it. The four workstreams that follow
        // fill these in; Phase 0 only makes the channel exist.
        on_command: Box::new({
            let _st = Rc::clone(state);
            move |c| match c {
                SettingsCommand::ShowPage(_)
                | SettingsCommand::SetPaused(_)
                | SettingsCommand::SetAutostart(_)
                | SettingsCommand::ReloadNow
                | SettingsCommand::SetDarkMode(_)
                | SettingsCommand::SetOpacity(_)
                | SettingsCommand::SetCapsShorthand(_)
                | SettingsCommand::Open(_)
                | SettingsCommand::Reveal(_)
                | SettingsCommand::Copy(_)
                | SettingsCommand::Undo => {}
            }
        }),
```

Add `SettingsCommand` to `serve.rs`'s existing `beckon_core::settings` import
list. If `_st` trips `-D warnings` as an unused binding despite the
underscore, drop the capture entirely — it is there only to document that the
real arms will need it.

- [ ] **Step 6: Run the gate**

```sh
cargo test   --workspace --exclude beckon-linux --exclude beckon-windows
cargo clippy --workspace --exclude beckon-linux --exclude beckon-windows \
      --all-targets -- -D warnings
cargo clippy --target aarch64-pc-windows-msvc -p beckon-windows \
      --all-targets -- -D warnings
```

Expected: all clean. The Windows crate does not construct `Callbacks` — it
receives one — so it needs no change here.

- [ ] **Step 7: Commit**

```bash
git add crates/beckon-core/src/settings.rs \
        crates/beckon-macos/examples/settings_probe.rs \
        crates/beckon-cli/src/serve.rs
git commit -m "feat(settings): one on_command callback for everything that is not an edit

Eleven commands behind one field. The macOS probe builds Callbacks as a
complete literal with no .., and CI clippies it --all-targets, so each extra
field would be a hard E0063 on a job unrelated to the feature."
```

---

## Task 6: `open(cb, &Paths, Page)`

The signature cannot land in pieces — four files, one commit.

**Files:**
- Modify: `crates/beckon-core/src/settings.rs` (add `Paths`; `Page` came with
  Task 5)
- Modify: `crates/beckon-windows/src/settings_window/mod.rs:1771-1790`
  (signature, `CFG`), `:604-610` (`title_base`)
- Modify: `crates/beckon-macos/src/settings_window.rs:615`, `:824`
- Modify: `crates/beckon-macos/examples/settings_probe.rs:194`
- Modify: `crates/beckon-cli/src/serve.rs:1598-1607`

**Interfaces:**
- Consumes: `Page` (Task 5).
- Produces: `pub struct Paths { pub config: PathBuf, pub log: Option<PathBuf> }`
  and, on both platforms,
  `pub fn open(cb: Callbacks, paths: &Paths, page: Page) -> Result<(), String>`.

- [ ] **Step 1: Define `Paths`**

In `settings.rs`, beside `Page`:

```rust
/// Where the two files this window talks about live.
///
/// `log` is `None` when `serve` was started without `--log`. The System page
/// omits the row rather than showing a path that does not exist -- the same
/// reasoning the tray menu uses for `Start with Windows` under
/// `beckon.exe serve`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Paths {
    pub config: std::path::PathBuf,
    pub log: Option<std::path::PathBuf>,
}
```

- [ ] **Step 2: Change the Windows definition**

`mod.rs:1775`:

```rust
pub fn open(cb: Callbacks, paths: &Paths, page: Page) -> Result<(), String> {
```

At `:1785`, `CFG` currently stores a `String`. Change its thread-local type to
`Option<Paths>` and store a clone:

```rust
    CFG.with(|c| *c.borrow_mut() = Some(paths.clone()));
```

Then fix every read of `CFG` — `rg 'CFG\.with' crates/beckon-windows/src` —
to go through `.config`. `title_base` (`mod.rs:604`) takes `&std::path::Path`
instead of `&str`:

```rust
fn title_base(config: &std::path::Path) -> String {
    match config.file_name() {
```

Store `page` in the same thread-local, or beside it, and **read it nowhere**:

```rust
    // Accepted and stored; nothing reads it yet, because there is nothing to
    // switch. Storing it now is what lets the tab-strip workstream be a
    // change to one module instead of a change to four crates.
    PAGE.with(|p| p.set(page));
```

Add `PAGE` as a `thread_local! { static PAGE: Cell<Page> = const { Cell::new(Page::Shortcuts) }; }`
beside `CFG`. If `-D warnings` objects that `PAGE` is never read, mark it
`#[allow(dead_code)]` **with the reason inline** rather than deleting it — the
whole point of this task is that the next workstream does not touch four
crates.

- [ ] **Step 3: Change the macOS definition**

`beckon-macos/src/settings_window.rs:615`:

```rust
pub fn open(cb: Callbacks, paths: &Paths, page: Page) -> Result<(), String> {
```

At `:824`:

```rust
        window.setTitle(&NSString::from_str(&format!(
            "beckon - {}",
            paths.config.display()
        )));
```

Add the same "accepted, not read" treatment for `page`:

```rust
    // Same as the Windows side: accepted and ignored. macOS has no tab strip
    // and this signature is shared, not per-platform.
    let _ = page;
```

- [ ] **Step 4: Fix both call sites**

`beckon-macos/examples/settings_probe.rs:194`:

```rust
        let paths = beckon_core::settings::Paths {
            config: "settings_probe (nothing is written)".into(),
            log: None,
        };
        if let Err(e) = win::open(cb, &paths, beckon_core::settings::Page::Shortcuts) {
```

`beckon-cli/src/serve.rs:1598-1602` — `ServeState` already holds both
(`serve.rs:198-216`):

```rust
    // The paths are what name the window and what the System page's two file
    // rows show. Handed over once, at open: `ServeState::config` is what
    // nothing can repoint while the window is up, and `log` is `None`
    // exactly when `serve` was started without `--log`.
    let paths = {
        let s = state.borrow();
        beckon_core::settings::Paths {
            config: s.config.clone(),
            log: s.log.clone(),
        }
    };
    if let Err(e) = swin::open(cb, &paths, beckon_core::settings::Page::Shortcuts) {
```

Note the borrow is scoped and dropped before `swin::open` — `open` re-enters
`ServeState` through the callbacks, and a live `borrow()` across that is the
abort-not-unwind failure `layout.rs:87-92` documents.

- [ ] **Step 5: Run the gate**

```sh
cargo test   --workspace --exclude beckon-linux --exclude beckon-windows
cargo clippy --workspace --exclude beckon-linux --exclude beckon-windows \
      --all-targets -- -D warnings
cargo clippy --target aarch64-pc-windows-msvc -p beckon-windows \
      --all-targets -- -D warnings
cargo fmt --all -- --check
```

Expected: all clean. Every `open` caller and definition is in the four files
above; if a fifth appears, it is a `beckon-linux` reference and the signature
is wrong.

- [ ] **Step 6: Commit**

```bash
git add crates/beckon-core/src/settings.rs \
        crates/beckon-windows/src/settings_window/mod.rs \
        crates/beckon-macos/src/settings_window.rs \
        crates/beckon-macos/examples/settings_probe.rs \
        crates/beckon-cli/src/serve.rs
git commit -m "refactor(settings): open takes Paths and a Page

Both platforms, both macOS call sites, one commit -- a shared signature
cannot land in pieces. The page is stored and read nowhere: there is nothing
to switch yet, and storing it now is what keeps the tab strip a change to one
module."
```

---

## Self-review

**Spec coverage.** §2.1/2.2 → T4 steps 1-4. §2.3 → T4 steps 5-6. §3.2/3.3 →
T2 step 3. §3.5 → T2 step 1 and T3 step 3. §4 → T5. §5 → T6. §6 → T1. §7 →
every task's gate step. §3.4 (`IDT_AUTOSAVE`) is reserved in the spec's table
and deliberately not coded — the constant would be dead code under
`-D warnings`, and the spec's §8 lists editing `IDT_CAPTURE`'s comment as out
of scope.

**Known gaps, stated rather than hidden.**
- The `MIN_HEIGHT` 561-vs-560 finding and the `IDC_TAP` ≈71 px finding are
  **recorded in comments, not fixed**. Both are visible changes; both belong
  to a workstream with a hardware gate behind it. G1 is the gate for the
  second.
- `layout.rs:258`'s `76 px card 0` is a sixth copy, found during planning and
  folded into T4 step 3. There may be more; `rg -n '\b(16|32|44|76|900|860)\b'`
  over the module is the sweep, and any further find goes in the same commit.
- T3's `every_core_id_is_defined_here` is not written, and why is stated at
  the step.
