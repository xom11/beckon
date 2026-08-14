# Four Doors — design vs. reality

**What this is.** One row per commitment in
`specs/2026-08-14-four-doors-settings-window-design.md`, with what is actually
in the tree. Read it to answer "is the window the design asked for yet?"
without reading five other documents.

Status words: **done** (landed and gated) · **partial** (some of it) ·
**open** (not started) · **changed** (the design was wrong or was overruled —
each one says why).

Last updated: 2026-08-14, branch `four-doors-phase-0`.

---

## §2 The window

| Design says | Status | Where / why |
|---|---|---|
| Four tab pills in a horizontal owner-drawn pill strip below the title bar | **done** | `mod.rs` `build_children`; ids 1040-1043 |
| `BS_AUTORADIOBUTTON \| BS_PUSHLIKE`, not `BS_OWNERDRAW` | **done** | and **measured**: gate G2 on a14, `CDIS_HOT` reaches it under comctl32 6.16 |
| Window narrows 760 → **680** | **done** | Task 8, deliberately last so every earlier task's arithmetic was written and checked at 760 and this is the one change that re-tests all of it. One constant plus the probe's transcribed copy — and `ids::geometry_matches_the_probe` failed on the unedited copy, on the dev machine, which is exactly the drift that test was built for. A card interior is 638 at 96 DPI (`cw1` / `grp_w` / `kb_w` alike), and **`col_app` is 421, not the design's ~438** — `layout` subtracts `SM_CXVSCROLL` from the list's client width whether or not a bar is up, so it is `638 − 17 − 200`, and 404 with one actually up. `MIN_WIDTH` did not move; `layout.rs` states that as a rule until G1 runs |
| `MIN_WIDTH` 660 unchanged | **done** | and must not move until G1 runs — `layout.rs` states that as a rule |
| `MIN_HEIGHT` 560 unchanged | **changed** | kept at 560, but its **four-row guarantee is withdrawn**: the strip costs 34 px and the floor now buys two rows. Design §4 makes the list scroll, so the floor's job changed. Recorded in `mod.rs` |
| Defaults to **dark** | **open** | System page (Task 7+) |
| Transparency slider 85-100 %, default 96 % | **open** | System page |
| Strip sits below the title bar, never inside the caption | **done** | and **verified free**: `chrome::nchittest` returns `HTCLIENT` below `TITLEBAR_H`, so no drag-zone arithmetic was needed |
| Tokens `TABSTRIP_H 36`, `TAB_PAD_X 14`, `TAB_PAD_Y 2`, `TAB_VISUAL 26`, `FOCUS_SLACK 3` | **done** | `layout.rs` `mod tok` |
| New palette tokens `strip` / `strip_hover` with `pairs()` rows | **changed** | done, but `LIGHT.strip_hover` moved `#CBD1DE` → `#C2C9D8`: the design's value measures **1.126** against the trough, under its own 1.2 floor |
| The four pills are drawn by beckon, three states each | **done** | Task 6, `paint::tab_pill` — a SIBLING of `paint::button` with its own `NM_CUSTOMDRAW` arm, because the pills are absent from `PUSH_BUTTONS` and that dispatch is gated on `is_push_button`. Selected-ness from `is_checked` (an auto-radio's notification has no ticked bit); hover swaps the ink as well as the ground, because `text_muted` on `strip_hover` is 3.700 and fails 4.5 |
| The pills sit in a **painted** trough | **changed** | Task 6 paints it (`paint::trough`, from `WM_PAINT`) — it had not been painted at all before, so the pills were sitting on `bg` and every `strip` contrast row described a surface nobody could see. **It spans the whole band; the mockup's hugs the four pills.** Closing that needs the run's width, which only `layout`'s placement loop computes, so it is a second shared geometry function beside `strip_rect` rather than a number invented in the painter. Deferred, argued at `paint::trough` |
| Shortcuts pill carries a count badge | **done** | Task 6. A new `ControlState::binding_count`, **not** `items.len()` — that is filtered and exempts the selected row, and the badge is read from three pages with no filter box. It reaches the painter through a thread-local `Cell`, never through `UI` (a paint arrives while `UI` is borrowed) and never through the caption (`layout` sizes buttons from their caption, and `layout` on a data push is the data-loss call). `layout` reserves a **fixed** four-digit slot for the same reason |
| Warn dot when the banner is up on another page | **done** | Task 6. A drawn GDI `Ellipse` in the pill's top-right corner, never `U+25CF`: a text face draws a missing glyph as a box. It costs no width, which is what keeps it off the `layout` path. `warn_dot_shown` is written as the exact complement of `banner_shown`, so the two partition `external_change` — that is what let the banner narrow back to Shortcuts, and `the_warning_is_on_screen_from_every_door` is the assertion that pays for it. **That assertion could not fail until Task 6's review**: written as `banner ^ dot` over a dot defined as `!banner`, it reduced to `B ^ !B` and passed for any `banner_shown` at all, a body returning `false` on every door included (falsified by doing exactly that: the old body passed, the new one fails). It now asserts per-page constants |
| The pills carry a focus ring | **done** | Task 6, and it was not optional: `CDRF_SKIPDEFAULT` means comctl32 draws nothing, so without one a keyboard-focused pill had no indication at all. Drawn in the `FOCUS_SLACK` margin the token is named for, so its ground is the TROUGH in all three states — `accent` everywhere (3.802 Light / 4.208 Dark on `strip`). It shipped with `BtnTier::Accent`'s `accent_on` swap, which is right for a ring inset INTO an `accent_fill` control and wrong here: `accent_on` on `LIGHT.strip` is **1.360**, an invisible ring, and no `pairs()` row covered the pair. Fixed by Task 6's review |
| `Ctrl+Tab` / `Ctrl+Shift+Tab` / `Ctrl+1..4` | **done** | Task 5, `build_accelerators`. The review that followed moved the focus repair's target: a door change now leaves focus on the pill it just opened, not on `Close`, so Enter after `Ctrl+2` no longer closes the window (`repair_hidden_button`'s `successor`). **Task 6's review added the other half, `focus_the_open_door`**: hiding the focused control hands focus to the WINDOW, which `repair_hidden_button` cannot see (`IsChild` is false for a window against itself, and the parent's control id is 0), and focus left there makes Tab dead until the user clicks — `GetNextDlgTabItem` refuses a starting point that is not a child. Every control except the App combo, whose inner EDIT stays a child, reached that state |
| The strip is ONE tab stop, Left/Right between pills | **done, free** | **measured with a control (G-S2), a14 2026-08-14**: `WS_TABSTOP` moves off A and onto B when B is checked, so user32 migrates it by itself. The first run of this gate could not say so — see the gates table |
| **No `&` mnemonics on tab names** | **done** | and settled by counting: `About` has no free letter left, so four unique mnemonics do not exist |

## The window as photographed, 2026-08-14

`../measurements/2026-08-14-four-doors-shell-a14-dark.png` — a14, dark, the
real binary at `1020x900`, which is `680x600` scaled by 1.5 at 144 DPI. That
one number confirms Task 8 landed.

**What the photograph confirms:** the four pills, the painted trough, the
active pill on `accent_fill`, the `18` badge on Shortcuts, and the window at
its new width.

**What the photograph shows still wrong**, all of it Shortcuts-page work that
is honestly marked *open* below, listed here because a reader comparing the
window to the mockup will see these first:

1. **The `App` / `Shortcut` column headers are still there.** Design §3.1
   deletes them — "keycap chips look like keys and app names look like app
   names".
2. **The editor still says `No shortcut selected`** and still labels its
   fields `App` and `Shortcut`. Design §3.1 deletes both.
3. **`Reset` has not become `Revert`.**
4. **The count appears twice** — `18` on the pill and `· 18 bindings` beside
   the card heading. §2 moved it to the pill so it reads from all four pages;
   the heading copy is what should go.
5. **`Save` and `Close` are still there**, correctly: auto-save is §6 and has
   not started.
6. **A card-sized void sits between the editor and the command bar.** That is
   the keyboard card's reserved height on a page that does not draw it —
   Task 7 raised it, decided not to re-stack, and this is what that decision
   looks like. It is the largest visual difference from the mockup.

## §3.1 Shortcuts page

| Design says | Status |
|---|---|
| The page the window opens on, including when the config does not parse | **done** |
| No column headers | **open** |
| No `Editing "…"` caption on the editor card | **open** |
| No field labels; App combo cue banner only while empty | **open** |
| `Reset` → **`Revert`** | **open** |
| Status words `paused` > `in use` > `missing` > `other chord`, healthy row empty | **partial** — precedence already exists in `row_condition`; the wording change is open |
| Notes silent when healthy (`Registered and working.` deleted) | **open** |
| `Win+L is reserved` said by Record at the moment it happens, not as a bullet | **open** |
| The list is short and **scrolls** | **partial** — the strip already costs it rows; the scroll behaviour itself is unchanged |

## §3.2 Keyboard page

| Design says | Status |
|---|---|
| The Caps row exists as its own page | **done** — Task 4 |
| Three `Hold` chips and never four | **done** (pre-existing; `Chord` has exactly ctrl/super/alt) |
| `Tap` is a `CBS_DROPDOWNLIST` read and written by index | **done** (pre-existing) |
| `Write shortcuts as [Caps] instead of [Ctrl][Win][Alt]` toggle, default OFF | **open** — id 1060 reserved |
| `If Caps Lock does nothing` expander | **open** — ids 1061/1062 reserved |
| The hook-disclosure line moves to About | **open** |

## §3.3 System page · §3.4 About page

| Design says | Status | Where / why |
|---|---|---|
| Both pages exist and open | **done** | Task 7. Each owns one `STATIC` reading `Nothing here yet.` — `IDC_SYS_PLACEHOLDER 1084`, `IDC_ABOUT_PLACEHOLDER 1115`, from the reserved TAILS of their ranges rather than the next free number, so deleting a placeholder cannot leave a hole in the middle of a page's numbering. `every_door_owns_at_least_one_control` is the assertion: before Task 7 both doors opened onto the strip, the command bar and nothing between, which reads as a window that failed to draw |
| Every id allocated (1070-1099, 1100-1119) | **done** (Phase 0) | and tested for uniqueness, so the pages can be filled without touching anything else |
| Every real System control (pause, autostart, dark, opacity, the two file rows) | **open** | |
| Every real About control (mark, version, build, location, licence, the hook disclosure, three links) | **open** | |

**The placeholders sit on `bg`, not on `card`, and that is a correction to the
plan.** The plan and shell spec §8 both say "**both must be added to the
`on_card` match** or they fall through to `COLOR_3DFACE` and draw as grey
rectangles". The hazard is real and is closed; the prescription was half right.
Neither page has a card at all — `compute_card_rects` leaves all four rects at
zero height behind those two doors — so `on_card` would have painted a
card-coloured strip the width of one line onto a page with no card behind it.
They get their own branch of `WM_CTLCOLORSTATIC` instead, returning the `bg`
brush with `text_muted` ink (already covered by `theme::pairs`' *muted text on
window bg* row) and `COLOR_BTNTEXT`/`COLOR_BTNFACE` under high contrast — a
same-family pair, unlike the cross-family one the `on_card` branch below it
carries its own correction about.

**They are also the first strings this window draws outside a card**, which
reopens a hazard `theme::apply_backdrop` had closed by naming that exact
change: GDI text drawn straight onto Mica glass loses its alpha and fringes
black. `OPAQUE` plus the `bg` fill is what keeps it closed — the ink lands on
an opaque surface either way. That comment has been corrected rather than left
describing a window that no longer exists.

## The vertical stack is still page-blind, and Task 7 decided not to fix it

`compute_card_rects` reserves the keyboard card's height on **every** page, so
Shortcuts keeps a card-shaped gap above the command bar and Keyboard keeps a
larger one below the strip. Task 4's agent recommended Task 7 own the re-stack;
Task 7 weighed it and deferred, and the argument is at `compute_card_rects`
rather than only here:

- The re-stack changes the **Shortcuts** page's vertical geometry, which is
  another workstream's open subject — design §4 uncaps the list and deletes
  `tok::ROWS`, design §3.1 deletes the editor's `Editing "…"` caption line
  (an input to `card2_h` that `MIN_HEIGHT`'s comment already solves the table
  without). Doing it now means re-deriving the same table twice.
- Nothing on the host can display this window. Every vertical figure under
  `MIN_HEIGHT` and beside `WINDOW_HEIGHT` is a hand trace, corrected twice on
  2026-08-13/14 and re-derived again when the strip landed. Two `STATIC`s are
  not worth a fourth pass.

**What deferring costs, re-derived rather than asserted:** the reservation is
`gap_card + kb_card_h` = 86 px at 96 DPI. With it gone the list's cap at the
shipped client height of 600 (banner down, `notes_h` 36) would be 264 instead
of 178, against a `want` of 197 — so `want` would bind and the list would show
**eight** rows instead of seven. One row at the shipped size, plus the 86 px
gap above the command bar, which is the visible half and is on the page the
user lives on. `kb_y` in `compute_card_rects` is the single line to change when
it is taken.

System and About needed none of it: a page whose whole content is one line has
no stack to re-derive, and the emptiness below that line is the page being
empty rather than the line being misplaced.

## §4 The list

| Design says | Status |
|---|---|
| Uncap the list so it follows window height | **already true** before this work — `layout.rs` was verified frozen at 8 rows and the fix had landed |
| `tok::ROWS` should be deleted | **open** |
| Keep the whole-row snap or delete `Ui::shown_empty` | **open** — design §12 q2 |

## §5 Two reversals

| Design says | Status |
|---|---|
| Transparency slider is buildable via `SetLayeredWindowAttributes` | **open** |
| Dark by default | **open** |

## §6 Auto-save

**Not started.** No Save/Close removal, no debounce, none of the eleven guards
(G-a … G-k). Two things from its neighbourhood did land early because Task 4
made them urgent:

- **§6.3's shipping bug is fixed**: `Remove` under a filter could delete the
  whole config, because the filter matched the Shortcut column and every
  beckon chord contains `alt`. The filter now matches the app name only.
- **The external-change protection was rebuilt** after Task 4 hid the banner
  on three pages while Save stayed reachable, and has now been narrowed back.
  The sequence is worth keeping because the condition has been written three
  ways in a day: Shortcuts-only (Task 4, the defect — Save is chrome and
  `apply_settings` writes without a prompt, so three doors could overwrite an
  externally changed file with nothing on screen saying it moved), then every
  page (the repair, deliberately wide), then Shortcuts-only again with the
  warn dot carrying the fact to the other three (Task 6).
  **What must never come back is a door with `external_change` set and nothing
  on screen about it**, and that is now an assertion rather than a claim:
  `banner_shown` and `warn_dot_shown` partition `external_change`, pinned by
  `the_warning_is_on_screen_from_every_door`. **It was a claim wearing an
  assertion's clothes for one commit** — `banner ^ dot` over a dot *defined* as
  `!banner` is `B ^ !B`, so the test passed for a `banner_shown` that showed
  nothing anywhere. Rewritten against per-page constants, and falsified by
  breaking `banner_shown` that exact way: the old body passes, the new one
  fails on Shortcuts. The warning is weaker on three
  doors than a sentence and two buttons — that is the design's own trade
  (§2), not an accident.

## §7 The seven editing rules

**Open.** They govern wording that has not been written yet (System, About,
the notes, the status line). The one rule already exercised is rule 2 —
silence is the healthy state — which `row_condition` predates this design.

---

## What has been measured on hardware

Everything else in this document is code, not evidence. These ran on a14
(Windows 11 ARM64, build 26200) through a session-1 scheduled task:

| Gate | Result |
|---|---|
| **G2** — does `CDIS_HOT` reach a `BS_PUSHLIKE` auto-radio? | **PASS** — radio and the plain-push-button control both report HOT, comctl32 6.16 |
| **G-S2** — does user32 migrate `WS_TABSTOP` onto the checked radio? | **PASS, on the re-run** — with radio A checked the styles read `A: WS_TABSTOP=true B: false`; with radio B checked they read `A: false B: true`. The bits followed the check, so the strip is ONE tab stop for free and no code may hand-maintain it. **The first run was blind and this row said YES on its strength** — see the note below |
| **G-S3** — does `BM_GETCHECK` answer an auto-radio? | **YES** — 1 / 0 |
| **G3** — is the client rect the window rect? | **settled by reading**, not yet confirmed by a probe run |

Still unrun: **G1** (does the Caps line fit at 680 px — and its scope shrank,
since design §3.2 replaces that line), **G-S1** (does a tab switch preserve
typed text), **G-S4** (the strip under four high-contrast schemes), **G-S5**
(frame metrics and the resize edge across the strip band), **G-S6** (does
`place_app_combo`'s restore restore), **G-S7** (what `GetFocus` returns for the
App combo).

**G-S5's instrument exists now, and G1's control got more expensive.** Task 8
gave `examples/settings_probe.rs` a strip section: per pill the id, the rect,
the checked state and four style bits (`BS_AUTORADIOBUTTON`, `BS_PUSHLIKE`,
`WS_TABSTOP`, `WS_GROUP`), then `GetSystemMetricsForDpi` for
`SM_CXSIZEFRAME` / `SM_CYSIZEFRAME` / `SM_CXPADDEDBORDER` **by name** — there
is no `SM_CYPADDEDBORDER`, index 92 is the X one and `chrome::nchittest`
spends it on both axes — plus `SM_CXVSCROLL` and the two `LVM_GETCOLUMNWIDTH`
readings, which are what make the 421/404 arithmetic above checkable rather
than asserted. It also prints the leftmost pill's `x` against
`SM_CYSIZEFRAME + SM_CXPADDEDBORDER`, which is G-S5's whole question in one
line. One reading of the style bits is still not evidence of `WS_TABSTOP`
migration; the section is built to be run twice with a door change between,
because only the change is evidence — that is the lesson of the first G-S2
run, recorded below. And G1's control was free while the window opened at 760
(measure the line, then measure it at the shipped size); at 680 the wider
control costs a hand-drag, so a G1 result now has to name the width it was
taken at.

**What Task 6 shipped that no test on this host can see.** Everything the
painter draws is pixels, and the two CI jobs that compile it cannot run it.
Reasoned from documentation and from the three measured facts above, NOT
measured: that the badge's reserved slot is wide enough for the face at
144 DPI (the slot is `text_size` of `"0000"` in the Keycap font, and no string
in this window has been through `GetTextExtentPoint32W` on hardware — gate
G1's own scope); that the warn dot lands inside the pill's rounded corner
rather than clipped by it (arithmetic at `paint::tab_pill`: dot centre 3.54
from the arc centre, far edge 7.04 against a radius of 8); that the focus ring
fits in the `FOCUS_SLACK` margin without being clipped by the control's own
`hdc` — **and that it stays OUT of the pill, which is now load-bearing rather
than cosmetic**, since it is why the ring's ground is `strip` and why its ink
is `accent` (the stroke reaches `scale(1) + scale(2)/2` in from the control
against a pill at `scale(3)`: 2 of 3 at 96 and 120 DPI, 3 of 4 at 144, 4 of 6
at 192, 5 of 7 at 240 — arithmetic, not a photograph); and that a full-width
trough behind four left-aligned pills reads as a
strip rather than as a toolbar. G-S4 is the run that would answer the first
three at once, and it needs a person at a14.

**A note on why the first G-S2 run was blind, since this table is what a
reader trusts.** The probe created radio A with `WS_GROUP | WS_TABSTOP` and
radio B with neither, then checked A and read both styles back once. `A: true,
B: false` is what migration produces AND what doing nothing at all produces, so
the reading was the creation styles and the run distinguished nothing — while
looking exactly like a pass. The correction is the second reading: check B and
read again, so only the CHANGE between the two is evidence. That re-run is the
2026-08-14 result recorded above. Nothing in the tree depended on the answer
either way — all four pills carry `WS_TABSTOP`, and the failing direction would
only have cost Tab three extra stops — which is why this was a blind gate
rather than a shipped defect.

**A note on why the first G2 run lied.** It reported every count zero,
including the control. The cause was not the radio: `beckon-windows`'s examples
are a different Cargo package from `beckon-cli` and had **no manifest**, so
they ran under comctl32 **v5**, where a BUTTON sends no `NM_CUSTOMDRAW` at all.
Fixed with `compile_for_examples`. Without the positive control in the same
run, that would have been read as "the design's control choice is wrong".
