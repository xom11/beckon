# Four Doors — the shell: Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Four tab pills in a painted trough below the title bar, real page
switching for Shortcuts and Keyboard, and the window at 680 px.

**Architecture:** One vertical offset in `compute_card_rects` buys the band.
Four `BS_AUTORADIOBUTTON | BS_PUSHLIKE` controls, custom-drawn by a sibling of
`paint::button`. A `Page` in the existing `PAGE` thread-local reaches `layout`
through `LayoutHandles`, and `layout` places only the current page's controls
— which is a correctness requirement, not an optimisation, because `layout`
is the measured data-loss path.

**Tech Stack:** Hand-written Win32 + GDI, `beckon-core` for anything testable
on all three CI jobs. No new crates.

## Global Constraints

- **Spec is `docs/superpowers/specs/2026-08-14-four-doors-shell-spec.md`.**
  Read §1 before writing anything; it is the verified ground truth and it
  records three places the design was wrong.
- **Ids are fixed by Phase 0** and may not be chosen: `IDC_TAB_SHORTCUTS 1040`,
  `IDC_TAB_KEYBOARD 1041`, `IDC_TAB_SYSTEM 1042`, `IDC_TAB_ABOUT 1043`. Any
  new id comes from its page's reserved range in
  `2026-08-14-four-doors-phase-0-spec.md` §3.2, and **every** new id needs a
  row in `beckon_core::settings::CONTROL_IDS` and in `ids.rs`'s `MINE`, or
  `every_declared_id_has_a_row_in_mine` fails.
- **`MIN_WIDTH` does not move**, in either direction, until G1 has run
  (`layout.rs:366-368` states this as a rule).
- **No new `Alt` mnemonics.** Four unique ones do not exist — see spec §3.3.
- **UI text is English.**
- Local gate, run all four:

  ```sh
  export CARGO_TARGET_DIR=/Users/lenamkhanh/Documents/dev/beckon/target
  cargo fmt --all -- --check
  cargo test   --workspace --exclude beckon-linux --exclude beckon-windows
  cargo clippy --workspace --exclude beckon-linux --exclude beckon-windows \
        --all-targets -- -D warnings
  cargo clippy --target aarch64-pc-windows-msvc -p beckon-windows --all-targets -- -D warnings
  cargo clippy --target x86_64-pc-windows-msvc  -p beckon-windows --all-targets -- -D warnings
  ```

  **Both Windows targets.** `windows-latest` on CI is x86_64; a14 is ARM64.
  `CARGO_TARGET_DIR` is not optional in a worktree — without it the build dies
  in dependency build scripts. A cargo death with `signal: 9` and no other
  error is environmental: re-run, up to 5 times.

- **`beckon-windows` tests cannot RUN on macOS.** Compiling them is the local
  gate; the Windows CI job runs them.
- Hardware gates are spec §9. `rog-win` (x86_64, Tailscale `100.79.249.5`,
  user `kln`) may now be reachable; a14 is ARM64 and may be off. **SSH lands
  in session 0 on either**, so every visual observation goes through a
  scheduled task in session 1 with **both** `-AllowStartIfOnBatteries
  -Priority 4`, and every gate needs a control.

---

## File Structure

| File | Responsibility |
|---|---|
| `crates/beckon-core/src/theme.rs` | `strip` / `strip_hover` on `Palette`, their `pairs()` rows (T1) |
| `crates/beckon-core/src/settings.rs` | `DefaultButton::visible(external, page)`; `ControlState::binding_count` (T5, T6) |
| `crates/beckon-windows/src/settings_window/layout.rs` | the five tokens, `strip_rect`, the one vertical offset, per-page placement (T2, T4) |
| `crates/beckon-windows/src/settings_window/ids.rs` | two placeholder ids + their `MINE` rows (T7) |
| `crates/beckon-windows/src/settings_window/mod.rs` | pill creation, `WS_GROUP`, `WM_COMMAND`, `NM_CUSTOMDRAW` dispatch, accelerators, `show_page`, `on_card`, `WINDOW_WIDTH` (T3-T8) |
| `crates/beckon-windows/src/settings_window/paint.rs` | `paint::tab_pill` (T3) |
| `crates/beckon-windows/examples/settings_probe.rs` | the four geometry constants, and a strip section (T8) |

**On code in this plan.** Where a signature, constant or decision is fixed,
it is written out below and is not negotiable. Where the existing code must be
read to write the change correctly — `build_children`'s creation idiom,
`paint::button`'s body, the `WM_NOTIFY` arm's shape — the step says what to
read and what the result must satisfy, rather than guessing at source the
plan's author did not have in front of them. Phase 0 taught that a plan
dictating code it has not read produces comments asserting things that are not
true; the steps below give criteria, and the implementer verifies them.

---

## Task 1: The two palette tokens, with their contrast rows

Lands **before** anything draws them, per design §2.

**Files:**
- Modify: `crates/beckon-core/src/theme.rs` — `Palette` struct, `LIGHT` (~:40), `DARK` (~:64), `pairs()` (~:200-240)

**Interfaces:**
- Produces: `Palette::strip`, `Palette::strip_hover`. T3 consumes both.

- [ ] **Step 1: Write the failing contrast test**

Add to `theme.rs`'s test module. These five rows are measured, not guessed —
recompute each before trusting it:

```rust
    #[test]
    fn the_tab_strip_pairs_clear_their_floors() {
        for (name, fg, bg, floor) in [
            ("text_muted on strip", |p: &Palette| p.text_muted, |p: &Palette| p.strip, 4.5),
            ("text on strip_hover", |p: &Palette| p.text, |p: &Palette| p.strip_hover, 4.5),
            ("strip on bg", |p: &Palette| p.strip, |p: &Palette| p.bg, 1.2),
            ("strip_hover on strip", |p: &Palette| p.strip_hover, |p: &Palette| p.strip, 1.2),
            ("accent_fill on strip", |p: &Palette| p.accent_fill, |p: &Palette| p.strip, 1.2),
        ] {
            for (theme, p) in [("LIGHT", &LIGHT), ("DARK", &DARK)] {
                let got = contrast(fg(p), bg(p));
                assert!(
                    got >= floor,
                    "{theme} {name}: {got:.3} is under the {floor} floor"
                );
            }
        }
    }
```

Match the closure/loop idiom the existing contrast tests use rather than this
shape if they differ — read `pairs()` and its tests first. The point is the
five pairs and the two floors, not the syntax.

- [ ] **Step 2: Run it to verify it fails**

```sh
cargo test -p beckon-core -- the_tab_strip_pairs_clear_their_floors
```

Expected: fails to compile — `strip` and `strip_hover` do not exist.

- [ ] **Step 3: Add the tokens**

```rust
    /// The trough the tab pills sit in.
    ///
    /// **In dark mode this is LIGHTER than the card, and that is forced by
    /// arithmetic rather than taste.** No colour darker than `DARK.bg` can
    /// clear the 1.2 border floor against it — pure black reaches only
    /// 1.171 — so a dark trough has to move away from the ground upward.
    /// The light half is symmetric: pure white against `LIGHT.bg` is 1.101,
    /// which forecloses a near-white trough the same way.
    pub strip: u32,
    /// Hover on an inactive pill.
    ///
    /// `LIGHT` is `#C2C9D8` and not the design's `#CBD1DE`, which measures
    /// 1.126 against `strip` — under the 1.2 floor, i.e. a hover state that
    /// cannot be seen. The design states the floor and then gives a value
    /// that fails it; the floor wins.
    ///
    /// The ink changes with the state: an inactive pill draws `text_muted`
    /// on `strip`, and `text` on `strip_hover`. `text_muted` on
    /// `strip_hover` measures 3.700 / 4.304 and would fail — which is why
    /// the hover swaps both halves, not just the ground.
    pub strip_hover: u32,
```

Values: `LIGHT { strip: 0xD9DDE7, strip_hover: 0xC2C9D8 }`,
`DARK { strip: 0x2E323D, strip_hover: 0x3A3F4C }`.

**Check the byte order against the neighbouring fields before writing them** —
`beckon-core/src/theme.rs` has a documented BGR swap case, and a token written
in the wrong order is a colour bug no test catches.

- [ ] **Step 4: Add the five rows to `pairs()`**

Read `pairs()`'s existing shape and add rows in its idiom. Expected measured
values, for the assertion messages and for checking your work:

| Pair | Floor | Light | Dark |
|---|---|---|---|
| `text_muted` / `strip` | 4.5 | 4.522 | 5.237 |
| `text` / `strip_hover` | 4.5 | 10.700 | 8.664 |
| `strip` / `bg` | 1.2 | 1.235 | 1.400 |
| `strip_hover` / `strip` | 1.2 | 1.222 | 1.217 |
| `accent_fill` / `strip` | 1.2 | 3.802 | 2.826 |

**Four of these ten measurements clear by under 0.04**, and the narrowest is
`strip_hover`/`strip` in DARK at +0.017 — not one of the three light cells.
Say so in a comment beside them: they
are correct and fragile, and the rows exist so that a future move of
`text_muted`, `bg` or either strip token is a test failure rather than a
screenshot.

- [ ] **Step 5: Run the full core suite**

```sh
cargo test -p beckon-core
```

Expected: PASS, including `every_pair_clears_its_floor_in_both_themes`.

- [ ] **Step 6: Commit**

```bash
git add crates/beckon-core/src/theme.rs
git commit -m "feat(theme): strip and strip_hover, with their contrast rows

The design's LIGHT.strip_hover measures 1.126 against the trough, under the
1.2 border floor -- a hover state that cannot be seen. #C2C9D8 clears it at
1.222. Three of the five new rows clear by under 0.04, which is why they are
rows rather than a screenshot."
```

---

## Task 2: The band exists, empty

Geometry only. Nothing is drawn and no control is created, so the visible
result is that every card moves down by 34 px and the list loses a row. That
is deliberate: it isolates the arithmetic from the controls.

**Files:**
- Modify: `crates/beckon-windows/src/settings_window/layout.rs` — `mod tok`, `compute_card_rects` (~:241), new `strip_rect`

**Interfaces:**
- Produces: `tok::{TABSTRIP_H, TAB_PAD_X, TAB_PAD_Y, TAB_VISUAL, FOCUS_SLACK}`;
  `pub(super) fn strip_rect(rc: RECT, dpi: u32) -> RECT`. T3 and T4 consume both.

- [ ] **Step 1: Add the tokens**

```rust
    /// The tab strip's trough.
    ///
    /// Not an independent number: `TAB_VISUAL 26 + 2*TAB_PAD_Y 2 +
    /// 2*FOCUS_SLACK 3 = 36`. Change any of the three and this follows.
    pub const TABSTRIP_H: i32 = 36;
    /// A pill's inner padding, left and right.
    pub const TAB_PAD_X: i32 = 14;
    /// The trough's inner padding around the pill row.
    pub const TAB_PAD_Y: i32 = 2;
    /// A pill's drawn height.
    pub const TAB_VISUAL: i32 = 26;
    /// A pill's margin inside the trough. The perceived gap between two
    /// pills is `2 * FOCUS_SLACK = 6`, which is `tok::GAP` -- the pills
    /// touch, and the space between them is their own margin.
    pub const FOCUS_SLACK: i32 = 3;
```

- [ ] **Step 2: Write `strip_rect`**

```rust
/// The tab strip's trough, in client coordinates.
///
/// Separate from `compute_card_rects` because it is not a card and the
/// `WM_PAINT` card loop must not draw it -- but it is the SOURCE of the
/// strip's height, and `compute_card_rects` calls it rather than repeating
/// `s(tok::TABSTRIP_H)`. Two copies of that arithmetic would drift, and the
/// drift would look like a rendering bug.
///
/// Inset by `tok::PAD` left and right, matching the cards. **That inset is
/// load-bearing beyond looks**: `chrome::nchittest` resolves the eight
/// resize directions itself and is only consulted for points no child
/// covers, so a pill reaching the client edge would kill the left and right
/// resize edge across this whole band. `PAD` is 10 at 96 DPI against a
/// border of roughly `SM_CXSIZEFRAME + SM_CXPADDEDBORDER`, and 15 against
/// that border at 144 -- a margin of 2-3 px, which is why gate G-S5 prints
/// those metrics by name rather than assuming them.
pub(super) fn strip_rect(rc: RECT, dpi: u32) -> RECT
```

Top is `s(chrome::TITLEBAR_H)`, height `s(tok::TABSTRIP_H)`, left/right inset
by `s(tok::PAD)`. Clamp every subtraction — `WM_SIZE` fires with a 0×0 client
rect on minimize, which is why every other subtraction in this file is
clamped.

- [ ] **Step 3: Spend the offset**

At `layout.rs:241`, `let mut y = pad + s(chrome::TITLEBAR_H);` becomes the
strip-aware form. **The surface `PAD` above the first card is spent by the
strip, not added to it** — net cost 34, not 44. Derive `y` from
`strip_rect(...).bottom + gap_card` rather than re-adding `TABSTRIP_H`, so
there is genuinely one source.

Update the term list above `WINDOW_HEIGHT` (which this branch just re-derived)
to include the band, and re-run its totals.

- [ ] **Step 4: Rewrite `MIN_HEIGHT`'s four-row paragraph**

The guarantee is **withdrawn**, not silently broken. `mod.rs:724-729` says "a
window whose list shows one row is not a smaller version of this window, it is
a broken one". Keep that standard and record why it now resolves differently:
design §4 makes the list scroll, so a floor's job becomes "enough rows to see
that it is a list" — two rows plus a scrollbar meets it, one does not, and 560
is where two rows stop fitting.

Re-derive the numbers rather than copying: at 560 with the banner up,
`list_h = 560 − 408 − notes_h − 34`. State the row count that falls out.

- [ ] **Step 5: Gate**

Run all five gate commands. Expected: clean. Nothing to see yet — the band is
empty space.

- [ ] **Step 6: Commit**

```bash
git add crates/beckon-windows/src/settings_window/layout.rs \
        crates/beckon-windows/src/settings_window/mod.rs
git commit -m "feat(settings-window): the tab strip's band, still empty

One offset in compute_card_rects, which owns all vertical geometry, and a
strip_rect that is its source rather than a second copy. The band costs 34 px,
not 36: the surface PAD above the first card is spent by the strip rather than
added to it.

MIN_HEIGHT stays 560 and its four-row guarantee is withdrawn rather than
quietly broken -- design 4 makes the list scroll, so the floor's job becomes
enough rows to see that it is a list."
```

---

## Task 3: Four pills that exist and switch

**Files:**
- Modify: `crates/beckon-windows/src/settings_window/mod.rs` — `build_children`, `Ui`, `WM_COMMAND`

**Interfaces:**
- Consumes: T2's tokens.
- Produces: four pill HWNDs in `Ui`; `PAGE` is written on click. T4 reads `PAGE`.

- [ ] **Step 1: Read before writing**

Read `build_children`'s creation idiom (class, styles, font, parent, the
`role_of` font path at `mod.rs:797-846`) and `mod.rs:2300-2306`, which states
that **creation order is tab order**.

- [ ] **Step 2: Create the four pills, first**

Styles: `BS_AUTORADIOBUTTON | BS_PUSHLIKE | WS_CHILD | WS_VISIBLE | WS_TABSTOP`.
Captions `Shortcuts`, `Keyboard`, `System`, `About`, **no `&`**.

Created **before `IDC_BANNER`**, because creation order is tab order and the
strip draws above everything.

**The control created immediately after the last pill gains `WS_GROUP`.**
Today `mod.rs:2828` is the file's only `WS_GROUP`. An auto-radio group and
`IsDialogMessageW`'s arrow-key group both run until the next `WS_GROUP`, so
without a closing boundary Left/Right walks out of the strip into the banner,
the filter EDIT and the ListView — and the auto-radio's clear-siblings pass
sprays `BM_SETCHECK(0)` across them.

**Do not add them to `PUSH_BUTTONS`** (`mod.rs:353-363`): `set_button_type`
(`mod.rs:1679-1694`) read-modify-writes `BS_TYPEMASK_BITS` and would rewrite
`BS_AUTORADIOBUTTON` (9) into `BS_PUSHBUTTON` (0), and the test at
`mod.rs:6481-6498` requires every member to map to a `DefaultButton`.

- [ ] **Step 3: Set the active pill**

Use `CheckRadioButton(hwnd, IDC_TAB_SHORTCUTS, IDC_TAB_ABOUT, id)` — in
`Win32::UI::Controls`, already glob-imported. **Not** this file's `check()`,
which falls through to `BM_SETCHECK` (`mod.rs:4120-4135`) and does not clear
sibling auto-radios. The four ids are contiguous, as `CheckRadioButton`
requires.

Seed it from the `page` argument `open` already stores in `PAGE`.

- [ ] **Step 4: Handle the click**

A `WM_COMMAND` arm for the four ids: write `PAGE`, raise
`SettingsCommand::ShowPage(page)` through `on_command`, then call the
`show_page` of T4. In this task `show_page` may be a stub that only calls
`layout` — the visible result is a strip you can click whose pills highlight.

- [ ] **Step 5: Gate, then verify tab order by reading**

Run all five. Then confirm by reading that the pills precede `IDC_BANNER` in
`build_children` and that exactly one control after them carries `WS_GROUP`.

- [ ] **Step 6: Commit**

```bash
git add crates/beckon-windows/src/settings_window/mod.rs
git commit -m "feat(settings-window): four tab pills

BS_AUTORADIOBUTTON | BS_PUSHLIKE, not BS_OWNERDRAW: owner-draw never receives
ODS_HOTLIGHT, so there would be no hover state, and it kills BM_GETCHECK,
which is why WM_CHIP_STATE had to be invented.

Created ahead of the banner because creation order is tab order, with WS_GROUP
on the control after the last pill -- an auto-radio group runs until the next
WS_GROUP, and without one the clear-siblings pass sprays BM_SETCHECK(0) across
the banner, the filter and the list."
```

---

## Task 4: Page switching, and the data-loss hazard

The dangerous task. Read spec §4 in full first.

**Files:**
- Modify: `crates/beckon-windows/src/settings_window/layout.rs` — `LayoutHandles`, `layout`
- Modify: `crates/beckon-windows/src/settings_window/mod.rs` — `show_page`, `Ui::shown_page`, the `apply_state` guard, the banner's `show` calls

**Interfaces:**
- Consumes: `PAGE` (T3), `beckon_core::settings::Page`.
- Produces: `fn show_page(hwnd: HWND, page: Page)`. T5 calls it from accelerators.

- [ ] **Step 1: Carry `Page` into `layout`**

Add `page: Page` to `LayoutHandles` (`layout.rs:124-151`), sourced from the
`PAGE` thread-local. It is a `Cell<Page>`, so reading it takes no `RefCell`
borrow and `compute_card_rects` keeps its documented "never touches `UI`"
property (`layout.rs:165-168`). **Do not put `page` in `Ui`** — it would work
and it loses that property.

- [ ] **Step 2: Place only the current page's controls**

`layout` currently places every control unconditionally through `GetDlgItem`.
Make each placement conditional on its page.

**This is a correctness requirement, not an optimisation.** `layout` is
`SetWindowPos` on the populated App combo (`layout.rs:650`), which is the
measured path that destroys typed text (`mod.rs:1033-1042`). The sharp case is
`Ctrl+1..4` from T5: `TranslateAcceleratorW` runs before `IsDialogMessageW`
and moves no focus, so without the skip the combo is resized while focused and
populated.

- [ ] **Step 3: Write `show_page`**

It must, in this order: hide the outgoing page's controls, show the incoming
page's, call `layout` directly (the way `WM_SIZE` does at `mod.rs:4767` — not
through `apply_state`, which nothing calls on a tab click), then call
`repair_default_button`.

**`repair_default_button` is not optional.** It runs only from `apply_state`
today (`mod.rs:3815`), and hiding a control raises no focus notification
(`mod.rs:1569-1578`) — so without it the default ring is left on an
off-screen button and Enter presses it. `Add`, `Remove`, `Record` and `Reset`
are all Shortcuts-page controls and all four are in `PUSH_BUTTONS`.

- [ ] **Step 4: The banner becomes a Shortcuts control**

`show(banner, external_change)` and its two buttons (`mod.rs:3769-3771`)
become `external_change && page == Page::Shortcuts`. `external_change` stays a
window-wide fact — T6's warn dot is how it stays visible from the other pages.

- [ ] **Step 5: Add `Ui::shown_page` to the `layout` guard**

The guard at `mod.rs:3789-3796` skips `layout` whenever no listed input moved.
Add a `shown_page` term beside `shown_external` and `shown_empty`, or a page
switch through `apply_state` leaves the previous page's geometry on screen.
Read `Ui::shown_external`'s doc (`mod.rs:1044-1051`), which enumerates
`layout`'s inputs and is the list being extended.

- [ ] **Step 6: Gate**

All five. Expected: clean.

- [ ] **Step 7: Commit**

```bash
git add crates/beckon-windows/src/settings_window/layout.rs \
        crates/beckon-windows/src/settings_window/mod.rs
git commit -m "feat(settings-window): page switching

layout places only the current page's controls, and that is a correctness
requirement rather than an optimisation: layout is SetWindowPos on the
populated App combo, the measured path that destroys typed text. Ctrl+1..4 is
the sharp case -- an accelerator runs before IsDialogMessageW and moves no
focus, so the combo would be resized while focused and populated.

show_page calls repair_default_button because hiding a control raises no focus
notification, so the ring would be left on an off-screen button and Enter
would press it. Four of them are Shortcuts-page push buttons."
```

---

## Task 5: Keyboard

**Files:**
- Modify: `crates/beckon-core/src/settings.rs` — `Page::next` / `Page::prev`
- Modify: `crates/beckon-windows/src/settings_window/ids.rs` — the two command ids
- Modify: `crates/beckon-windows/src/settings_window/mod.rs` — `build_accelerators`, `handle_command`, `go_to_door`
- Modify: `crates/beckon-windows/examples/pill_probe.rs` — G-S2's missing control

**CORRECTED 2026-08-14, before this task started: steps 1 and 2 were already
done, and this task is only step 3.** Task 4 moved `page` into
`DefaultButton::visible` / `pressable` / `default_button` along with the core
tests scheduled here, because deferring it made Task 4's own
`repair_default_button` call a no-op for the page hazard — the fix and the
thing it fixes could not be split across two commits. So the "page-aware
default button" half of the commit message below describes work that is
already in the tree.

- [x] ~~**Step 1: Write the failing core test**~~ — landed in Task 4
  (`the_shortcuts_pages_buttons_lose_the_default_behind_another_door`,
  `the_banners_two_follow_the_change_on_every_door`).
- [x] ~~**Step 2: Change the signature and fix every caller**~~ — landed in
  Task 4.

- [x] **Step 3: Extend the accelerator table**

`build_accelerators` goes `[ACCEL; 1]` → `[ACCEL; 3 + TABS.len()]`, i.e. seven:
`Ctrl+Tab`, `Ctrl+Shift+Tab`, `Ctrl+1`..`Ctrl+4`, plus the existing `Ctrl+S`.

**`Ctrl+Tab` must be an accelerator**, not left to the dialog manager:
`IsDialogMessageW`'s `VK_TAB` branch is not documented to consult the Ctrl
state, so forgetting the entry gives "focus moves one control" — which looks
like nothing happened and is far harder to spot than a dead key.

Three things the plan did not name and the work needed:

1. **Two command ids**, `IDM_PAGE_NEXT` / `IDM_PAGE_PREV` (2001-2). The two
   `Tab` entries name a *direction*, not a door, and an `ACCEL` carries a
   command id and nothing else — so the direction is resolved in
   `handle_command` against `PAGE`. `Ctrl+1`..`4` need no new id: they ride on
   the pills'.
2. **`Page::next` / `Page::prev` in core**, as exhaustive `match`es so a fifth
   door is a compile error, with the off-Windows tests this task can have:
   four steps forward visit every door once and come home, `prev` inverts
   `next`, and the strip wraps at both ends. `the_strip_order_is_the_cycle`
   (Windows-side) pins them against `TABS`.
3. **`go_to_door`**, so "raise `SettingsCommand::ShowPage` only when the door
   really moved" is spelled once for all three arms rather than three times.

- [x] **Step 4: Gate**

All five, plus `cargo test -p beckon-core` for the new tests.

- [x] **Step 5: Commit**

```bash
git add crates/beckon-core/src/settings.rs \
        crates/beckon-windows/src/settings_window/ids.rs \
        crates/beckon-windows/src/settings_window/mod.rs \
        crates/beckon-windows/examples/pill_probe.rs \
        docs/superpowers/specs/2026-08-14-four-doors-shell-spec.md \
        docs/superpowers/plans/2026-08-14-four-doors-shell.md
git commit -m "feat(settings-window): Ctrl+Tab, Ctrl+Shift+Tab and Ctrl+1..4

Ctrl+Tab is an accelerator rather than dialog-manager behaviour because
IsDialogMessageW's VK_TAB branch is not documented to consult Ctrl: forgetting
the entry moves focus one control, which reads as nothing happening.

The page-aware default button this task was also scheduled for landed in Task
4 -- splitting it made that task's own repair a no-op -- so what is left is the
table, the two direction ids it needs, and the cycle in core."
```

---

## Task 6: The pill painter, the badge and the warn dot

**Files:**
- Modify: `crates/beckon-windows/src/settings_window/paint.rs` — new `tab_pill`
- Modify: `crates/beckon-windows/src/settings_window/mod.rs` — `NM_CUSTOMDRAW` arm, the badge thread-local
- Modify: `crates/beckon-core/src/settings.rs` — `ControlState::binding_count`

- [ ] **Step 1: Read `paint::button` and the `IDC_CAPS` custom-draw arm**

`paint.rs` around `:1160-1280`, and `mod.rs:5233-5244`. The pill is a sibling
of `button`, not a branch inside it.

- [ ] **Step 2: Write `tab_pill`**

Three states. Active: `accent_fill` ground, `accent_on` ink. Inactive:
`strip` ground, `text_muted` ink. Hover: `strip_hover` ground, **`text`
ink** — the ink swaps with the ground, because `text_muted` on `strip_hover`
measures 3.700 / 4.304 and fails 4.5. (The figure was 4.015 in the first
draft of this plan, which is `text_muted` against the design's REJECTED
`#CBD1DE` — measured before the token moved and not re-measured after. Task 1
caught it. Re-measure any figure quoted here for a token that moved rather
than copying it.)

**The active fill is `accent_fill`, never `accent`**: `accent_on` on
`DARK.accent` measures 3.044, and nothing in `pairs()` covers that
combination, so the failure would ship unseen.

Read high contrast as `cache.theme() == Theme::HighContrast`, **never**
`high_contrast()` — that `Cell` (`mod.rs:4218-4232`) refreshes only on
`WM_SETTINGCHANGE`, while `WM_THEMECHANGED` alone rebuilds `ThemeCache`, so a
paint between the two sees a stale value. Under HC, flatten to `Rectangle`
(six existing sites do; `paint.rs:300-302` says why).

Selected-ness comes from `is_checked(hwnd, id)`, **not** `CDIS_CHECKED` — the
identical decision is documented and executed for `IDC_CAPS`
(`mod.rs:4391-4397`, `:4404`).

- [ ] **Step 3: Dispatch it**

A new `NM_CUSTOMDRAW` arm, placed **before** the `suppressed()` gate at
`mod.rs:5244` and modelled on the `IDC_CAPS` arm — custom draw is pure
painting and must not be gated on suppression. The existing dispatch at
`:5233` is gated on `is_push_button` and will not match a pill.

- [ ] **Step 4: The badge and the warn dot, through a thread-local**

Add `ControlState::binding_count` in core. **Not `items.len()`**:
`control_state` builds `items` from `Model::visible()`, which is
filter-dependent and exempts the selected row unconditionally
(`settings.rs:544`). The badge is read from three pages that have no filter
box, so it must be the file's binding count. `IDC_LBL_COUNT` keeps counting
visible rows; the two are different on purpose.

The painter reads the count and the warn flag from a thread-local `Cell`
written by `apply_state` — **never from `UI`**. A paint reaches this window
while `UI` is already borrowed; measured on a14, where every subitem
notification exited at `try_borrow` and the Shortcut column silently drew as
plain text. `CHIPS` (`mod.rs:1378-1383`), `CAP_FONT` (`:4238-4243`) and
`SHOWN_NOTES` (`:1704-1710`) are the three precedents.

**Neither may ride in the caption**: `layout` sizes buttons from `text_size`
of their caption (`layout.rs:432-434`), so a data-dependent caption makes the
caption a `layout` input — and `layout` on a data push is the data-loss call.
`cap::STOP` (`mod.rs:459-465`) is the same decision already taken once.

The warn dot is a drawn GDI `Ellipse`, never the character `●`: a text face
draws a missing glyph as a box, and an em-dash in `serve --log` already came
back as `?"` once.

- [ ] **Step 5: Gate**

All five, plus `cargo test -p beckon-core`.

- [ ] **Step 6: Commit**

```bash
git add crates/beckon-core/src/settings.rs \
        crates/beckon-windows/src/settings_window/paint.rs \
        crates/beckon-windows/src/settings_window/mod.rs
git commit -m "feat(settings-window): the pill painter, count badge and warn dot

A sibling of paint::button, not a branch inside it: the pills are not in
PUSH_BUTTONS and the existing custom-draw dispatch is gated on is_push_button.

The hover swaps ink as well as ground -- text_muted on strip_hover measures
3.700 LIGHT and fails the 4.5 floor, so an inactive pill draws text_muted on
strip and text on strip_hover.

The badge counts the file's bindings, not ControlState::items, which is
filter-dependent and exempts the selected row. It reaches the painter through
a thread-local rather than through UI, because a paint arrives while UI is
already borrowed -- measured, and the reason the Shortcut column once drew as
plain text."
```

---

## Task 7: System and About, waiting

**Files:**
- Modify: `crates/beckon-core/src/settings.rs` — two `CONTROL_IDS` rows
- Modify: `crates/beckon-windows/src/settings_window/ids.rs` — two constants + two `MINE` rows
- Modify: `crates/beckon-windows/src/settings_window/mod.rs` — creation, `on_card`

- [ ] **Step 1: Allocate the ids**

`IDC_SYS_PLACEHOLDER = 1084` and `IDC_ABOUT_PLACEHOLDER = 1115`, from the
reserved tails of their pages' ranges. Add a row to `CONTROL_IDS`, a constant
to `ids.rs` and a row to `MINE` — `every_declared_id_has_a_row_in_mine` and
`ids_are_unique` both fail otherwise, which is the net working.

- [ ] **Step 2: Create them**

Two `STATIC`s reading `Nothing here yet.`

**Both must be added to the `on_card` match at `mod.rs:5363-5375`**, or they
fall through to `DefWindowProcW`'s `COLOR_3DFACE` and draw as grey rectangles
— the defect recorded at `mod.rs:5294-5309`, which hit eight controls at once.

- [ ] **Step 3: Gate and commit**

```bash
git add crates/beckon-core/src/settings.rs \
        crates/beckon-windows/src/settings_window/ids.rs \
        crates/beckon-windows/src/settings_window/mod.rs
git commit -m "feat(settings-window): System and About, waiting

Two STATICs, both in the on_card match -- a STATIC outside it falls through to
COLOR_3DFACE and draws as a grey rectangle, which hit eight controls at once
the last time."
```

---

## Task 8: 680 px

Last, so every earlier task's arithmetic has already been checked at the width
it was written for.

**Files:**
- Modify: `crates/beckon-windows/src/settings_window/mod.rs` — `WINDOW_WIDTH`
- Modify: `crates/beckon-windows/examples/settings_probe.rs` — `WINDOW_WIDTH_96`, and a strip section

- [ ] **Step 1: Change the constant and watch the test fail**

`WINDOW_WIDTH: i32 = 680`. Then:

```sh
cargo clippy --target x86_64-pc-windows-msvc -p beckon-windows --all-targets -- -D warnings
```

compiles, but `geometry_matches_the_probe` (`ids.rs`) would fail on CI. **Run
it in your head against the probe's constant and fix the probe** — that test
is Phase 0's and this is exactly the drift it exists to catch.

- [ ] **Step 2: Re-check the widths the spec flags**

Card interior becomes `680 − 2·PAD − 2·CARD_PAD = 638` — confirm against
`cw1` / `grp_w` / `kb_w` (`layout.rs:525`, `:636`, `:713`). Note in the commit
that `col_app` is 421, not the design's ~438: `layout.rs:597-602` subtracts
`SM_CXVSCROLL` unconditionally.

**`MIN_WIDTH` does not move.**

- [ ] **Step 3: Give the probe a strip section**

It should print, for each of the four pills: the id, the style bits (so G-S2
can read `WS_TABSTOP` migration), the rect, and the checked state. Plus
`GetSystemMetricsForDpi(SM_CXSIZEFRAME / SM_CYSIZEFRAME / SM_CXPADDEDBORDER,
dpi)` by name, for G-S5.

**There is no `SM_CYPADDEDBORDER`** — `windows` 0.61.3 defines index 92 as
`SM_CXPADDEDBORDER` alone, and one constant serves both axes.

- [ ] **Step 4: Gate and commit**

```bash
git add crates/beckon-windows/src/settings_window/mod.rs \
        crates/beckon-windows/examples/settings_probe.rs
git commit -m "feat(settings-window): 680 px, and a strip section in the probe

The design's ~438 px for the app name is 17 px optimistic: col_app is 421,
because layout subtracts SM_CXVSCROLL unconditionally, and 404 with a
scrollbar up. MIN_WIDTH does not move until G1 has run.

The probe prints each pill's style bits so G-S2 can read whether user32
migrates WS_TABSTOP onto the checked radio, and the frame metrics by name for
G-S5. There is no SM_CYPADDEDBORDER -- index 92 is SM_CXPADDEDBORDER and
serves both axes."
```

---

## Self-review

**Spec coverage.** §2.1 → T2 S1. §2.2 → T2 S3. §2.3 → T2 S4. §2.4 → T2 S2.
§3.1-3.2 → T3. §3.3 → T3 S2 (captions). §4.1 → T4 S3. §4.2 → T4 S2. §4.3 →
T4 S3-S5, T5 S2. §4.4 → T5 S3. §5 → T6 S4. §6.1 → T6 S2-S3. §6.2 → T1.
§6.3 → T6 S2. §7 → T8. §8 → T7. §9 → T8 S3 builds the instrument; the gates
themselves need hardware.

**Known gaps.**
- **No task runs a hardware gate.** Eight are listed in spec §9 and all eight
  need a Windows box. G2 (`CDIS_HOT` on a pushlike radio) is the one that
  could invalidate T3's control choice; the design names the fallback
  (`BS_PUSHBUTTON + BS_NOTIFY` plus a `BN_SETFOCUS` arm and `TrackMouseEvent`)
  and it is not planned here.
- **T4 is the task most likely to be wrong**, because the skip it introduces
  is invisible to every check available on macOS. G-S1 is its gate and it
  needs a person typing.
- Spec §10's five open questions are untouched by this plan. The first —
  whether `SetWindowPos` with an unchanged rect still re-syncs a populated
  combo — would defuse most of T4 if answered no, and `examples/combo_probe.rs`
  is where to answer it.
