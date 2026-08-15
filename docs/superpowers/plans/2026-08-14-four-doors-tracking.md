# Four Doors — design vs. reality

**What this is.** One row per commitment in
`specs/2026-08-14-four-doors-settings-window-design.md`, with what is actually
in the tree. Read it to answer "is the window the design asked for yet?"
without reading five other documents.

Status words: **done** (landed and gated) · **partial** (some of it) ·
**open** (not started) · **changed** (the design was wrong or was overruled —
each one says why).

Last updated: 2026-08-15, branch `four-doors-phase-0`.

---

## §2 The window

| Design says | Status | Where / why |
|---|---|---|
| Four tab pills in a horizontal owner-drawn pill strip below the title bar | **done** | `mod.rs` `build_children`; ids 1040-1043 |
| `BS_AUTORADIOBUTTON \| BS_PUSHLIKE`, not `BS_OWNERDRAW` | **done** | and **measured**: gate G2 on a14, `CDIS_HOT` reaches it under comctl32 6.16 |
| Window narrows 760 → **680** | **done** | Task 8, deliberately last so every earlier task's arithmetic was written and checked at 760 and this is the one change that re-tests all of it. One constant plus the probe's transcribed copy — and `ids::geometry_matches_the_probe` failed on the unedited copy, on the dev machine, which is exactly the drift that test was built for. A card interior is 638 at 96 DPI (`cw1` / `ed_w` / `kb_w` alike — `ed_w` was `grp_w`, and its contents sat a further `tok::GAP` inside it until 2026-08-15 deleted the group box's caption and with it the inset), and **`col_app` is 421, not the design's ~438** — `layout` subtracts `SM_CXVSCROLL` from the list's client width whether or not a bar is up, so it is `638 − 17 − 200`, and 404 with one actually up. `MIN_WIDTH` did not move; `layout.rs` states that as a rule until G1 runs |
| `MIN_WIDTH` 660 unchanged | **done** | and must not move until G1 runs — `layout.rs` states that as a rule |
| `MIN_HEIGHT` 560 unchanged | **changed** | kept at 560 through two re-derivations, and the constant has not moved in either. Its **four-row guarantee is withdrawn** — design §4 makes the list scroll, so a row count is not what a floor should promise — and the arithmetic under it has now swung from *two* rows to **eight**: 2026-08-15 returned 110 px to the list (the keyboard card's cross-page reservation, the editor caption, the column header) and deleted the cap that would have absorbed it. The two-row point is **412**, so the floor clears its own standard by 148 px. **Not moved, deliberately**: the standard is met at both ends so it cannot choose, the slack points the safe way (too high costs draggability, too low ships a one-row list), and `MIN_WIDTH` is frozen until G1 for the same class of reason. Numbers for whoever lowers it are in `MIN_HEIGHT`'s own comment — 412 / 456 / 500 for two / four / six rows |
| `WINDOW_HEIGHT` 600 unchanged | **done** | and re-derived 2026-08-15: it buys **13 rows** banner-down where it bought 7. Left alone on purpose — the only argument for a shorter window is that the mock-up's page is ~436 px, and the mock-up is drawn **without a command bar**, which is design §6's job, not this pass's |
| Defaults to **dark** | **open** | System page (Task 7+) |
| Transparency slider 85-100 %, default 96 % | **open** | System page |
| Strip sits below the title bar, never inside the caption | **done** | and **verified free**: `chrome::nchittest` returns `HTCLIENT` below `TITLEBAR_H`, so no drag-zone arithmetic was needed |
| Tokens `TABSTRIP_H 36`, `TAB_PAD_X 14`, `TAB_PAD_Y 2`, `TAB_VISUAL 26`, `FOCUS_SLACK 3` | **done** | `layout.rs` `mod tok` |
| New palette tokens `strip` / `strip_hover` with `pairs()` rows | **changed** | done, but `LIGHT.strip_hover` moved `#CBD1DE` → `#C2C9D8`: the design's value measures **1.126** against the trough, under its own 1.2 floor |
| The four pills are drawn by beckon, three states each | **done** | Task 6, `paint::tab_pill` — a SIBLING of `paint::button` with its own `NM_CUSTOMDRAW` arm, because the pills are absent from `PUSH_BUTTONS` and that dispatch is gated on `is_push_button`. Selected-ness from `is_checked` (an auto-radio's notification has no ticked bit); hover swaps the ink as well as the ground, because `text_muted` on `strip_hover` is 3.700 and fails 4.5 |
| The pills sit in a **painted** trough | **changed** | Task 6 paints it (`paint::trough`, from `WM_PAINT`) — it had not been painted at all before, so the pills were sitting on `bg` and every `strip` contrast row described a surface nobody could see. **It spans the whole band; the mockup's hugs the four pills.** Closing that needs the run's width, which only `layout`'s placement loop computes, so it is a second shared geometry function beside `strip_rect` rather than a number invented in the painter. Deferred, argued at `paint::trough` |
| Shortcuts pill carries a count badge | **done** | Task 6. A new `ControlState::binding_count`, **not** `items.len()` — that is filtered and exempts the selected row, and the badge is read from three pages with no filter box. It reaches the painter through a thread-local `Cell`, never through `UI` (a paint arrives while `UI` is borrowed) and never through the caption (`layout` sizes buttons from their caption, and `layout` on a data push is the data-loss call). `layout` reserves a **fixed** four-digit slot for the same reason |
| …and it is now the **only** count on screen | **done** | 2026-08-15. `IDC_LBL_COUNT` (1035), the `· 18 bindings` STATIC beside the card heading, is deleted. Task 6 moved the count to the pill precisely so it would read from all four doors, and the photograph shows the window carrying both — which is the state that move existed to end, and worse than a duplicate, since the badge counts the FILE and the heading counted the FILTERED list, so under a filter they disagreed while both were right. **Retired, not freed**: 1035 is in `RETIRED_IDS`, out of `CONTROL_IDS`, and `retired_ids_stay_retired` fails on anything reclaiming it — a `settings_probe` built against today's binary is still looking for that number. **`layout` does nothing with the space, deliberately**: `head_w` is `text_size` of the constant `"Shortcuts"` and was only ever CLAMPED by where the filter starts, never sized from the count, so deleting the second control frees a gap and moves no edge. Stretching the heading into it would change a left-aligned STATIC's box to no visible effect, and re-flowing the row is geometry this pass does not own. **The heading itself went the same day** (see §3.1), and there `layout` DOES move an edge — the two calls differ because the count freed a gap between two controls while the heading was the row's leading one |
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

**What the photograph shows still wrong**, listed here because a reader
comparing the window to the mockup will see these first. **The photograph is
dated: six of the seven were fixed on 2026-08-15 and are struck through rather
than deleted, so the picture and this list still line up. Only item 5 stands,
and it stands correctly.**

**Item 7 was missing from this list until it was reviewed**, which is the one
thing worth taking from it: the list was written by comparing the photograph to
§3.1's *bullets*, and the heading is absent from the *drawing*. Where the two
halves of a spec disagree about what is on screen, the drawing is the one that
can be compared to a photograph.

1. ~~**The `App` / `Shortcut` column headers are still there.**~~ **Fixed
   2026-08-15**, with `LVS_NOCOLUMNHEADER` on `IDC_LIST`. Note what else the
   photograph shows and this closes: the header band is painted **bright
   white** in a dark window. That is a measured, unfixed defect from a14
   2026-08-13 (`theme_list`) — `DarkMode_ItemsView` is inert without the
   uxtheme ordinals the 2026-08-11 spec rejected, and the `NM_CUSTOMDRAW`
   route meant to owner-draw the band was not firing for reasons nobody
   established. A control that is not created cannot be painted wrong.
2. ~~**The editor still says `No shortcut selected`** and still labels its
   fields `App` and `Shortcut`.~~ **Fixed 2026-08-15.** All three controls are
   gone and all three ids (1034, 1018, 1017) are **retired**, not freed.
3. ~~**`Reset` has not become `Revert`.**~~ **Fixed 2026-08-15.** The caption
   is `R&evert`; the mnemonic stayed `e` deliberately (see below).
4. ~~**The count appears twice** — `18` on the pill and `· 18 bindings` beside
   the card heading.~~ **Fixed 2026-08-15**, by deleting the heading's copy.
   Id 1035 is **retired**, not freed.
5. **`Save` and `Close` are still there**, correctly: auto-save is §6 and has
   not started.
6. ~~**A card-sized void sits between the editor and the command bar.**~~
   **Fixed 2026-08-15** — the stack is page-dependent now, so the Keyboard
   card is not reserved on a page that does not draw it. This was the largest
   visual difference from the mockup, and **fixing it alone would have moved
   the void rather than removed it**: `tok::ROWS` capped the list at eight
   rows, so every pixel the re-stack and the other deletions returned would
   have re-appeared as empty space below the editor card — 112 px of it at the
   shipped size. That is why `tok::ROWS` had to go in the same commit. See the
   re-stack section below.
7. ~~**The `Shortcuts` heading is on screen twice**, once as the lit tab pill
   and once as an 18 px Subtitle at the top of card 1, two lines apart.~~
   **Fixed 2026-08-15** — added to this list by the review that found it, not
   by the pass. Id 1020 is **retired**, not freed, and the filter moves into
   the row's leading edge. See the §3.1 row.

**What no photograph will show until someone takes a new one.** Every claim in
the five struck-through items is arithmetic and code, checked by five green
gates on two Windows targets and by hand-verifying the four id invariants that
only run on the Windows CI job. **Nothing here has been seen.** A fresh a14
capture at `1020x900` is the cheapest way to check all five at once, and
`examples/settings_probe.rs` now prints `LVS_NOCOLUMNHEADER` and a
header-visibility verdict so the run says something even without a screenshot.

**A seventh difference the photograph cannot show, and the reader should not
go looking for it.** The status words changed (`key in use` → `in use`,
`not installed` → `missing`, `custom` → `other chord`) and three of the four
lost the note that repeated them — but the photograph's config is healthy, so
every row was already silent in it. Nothing in that picture moved.

## §3.1 Shortcuts page

| Design says | Status |
|---|---|
| The page the window opens on, including when the config does not parse | **done** |
| No column headers | **done** — 2026-08-15. `LVS_NOCOLUMNHEADER` on `IDC_LIST`, **not** a hidden Header window, and the difference is the arithmetic: a hidden Header still answers `LVM_GETHEADER` and still has a rect, so `list_header_height` would have gone on returning its 21 px fallback for a band nobody can see — a live term in `compute_card_rects` paying for nothing. With the style there is no term: `list_header_height`, `set_header_font`, `header_of` and `paint::header_custom_draw` are all deleted. The two COLUMNS stay (`LVS_REPORT`, `LVCF_TEXT` and all) — two columns are what put the chord flush right against the app name, and a column's text is still what the control reports about itself to anything that asks |
| No `Shortcuts` heading on the list card (the drawing's head row is the filter and the two buttons) | **done** — 2026-08-15, **found by review rather than by the pass**: neither design §3.1's drawing nor the mock-up has a heading there, and the window drew one — `IDC_LBL_SECTION` (1020), an 18 px Subtitle STATIC reading `Shortcuts`, at the top of card 1, **directly beneath a tab pill captioned `Shortcuts`**. It is the same duplication the count badge had, one row lower and in words rather than digits, and it survived the four-deletion pass because that pass worked from §3.1's bullet list, which names the caption and the field labels and does not mention the heading. The drawing does. **Retired, not freed**: 1020 is in `RETIRED_IDS`, out of `CONTROL_IDS`, out of `MINE` and out of `PAGE_CONTROLS` (24 rows → 23), and `retired_ids_stay_retired` / `no_defined_id_is_retired` fail on anything reclaiming it. **What `layout` does with the row**: it keeps its height — `ctl`, decided by the buttons in it, so `list_y` and everything below are untouched — and the filter takes the card's own left edge, which is where the drawing and the mock-up both put it. `Add` and `Remove` do not move. This is the opposite call from `IDC_LBL_COUNT`'s (where `layout` deliberately did nothing) and for a stated reason: the count freed a gap BETWEEN two controls, while the heading was the row's LEADING control, so leaving its width blank would open the card with a hole. Two consequences worth knowing: `Role::Subtitle` now has no reader and carries an `#[allow(dead_code)]` naming About's `ABOUT_NAME` as the next one (`#[expect]` would be better and needs 1.81 against a 1.75 floor), and `text_size` in `layout` is down to `tw` / `tw_kc` / `"Ag"` — the heading was the one string measured in a third font |
| No `Editing "…"` caption on the editor card | **done** — 2026-08-15. `IDC_GRP_EDITOR` (1034) retired; `card2_h` loses the `s(24)` the spec costed it at. **A rule went with it**: this was the only caption in the window fed from the CATALOG, so it was the only one that had to double an `&` before writing (Start Menu names really do carry them — `Notes & To Do` drew as `Editing "Notes  To Do"` with **T** underlined, colliding with the `Ctrl` chip). Nothing writes catalog text into a `STATIC` any more, so the rule has no subject; `apply_state` says where to reinstate it |
| No field labels; App combo cue banner only while empty | **done** — 2026-08-15. `IDC_LBL_APP` (1018) and `IDC_LBL_SHORTCUT` (1017) retired; `CB_SETCUEBANNER` puts `App` in the combo, and the key list carries its meaning from where it sits. **The `gap` inset went with them**, which the spec did not cost: `ins_x`/`ins_w` were clearance for a `BS_GROUPBOX` frame that stopped existing at Task 8's reclass, and they were misaligning the editor card's contents 6 px right of the list's. **The spec's "this puts the key list back over `SHORTCUT_COL`" is verified at 680**, and the *before* half needs no font: the run was ~~`212 − lw_lbl`, under the 200 ceiling for any label column wider than 12 px~~ **`220 − lw_lbl`, under the 200 ceiling for any label column wider than 20 px** — struck and re-derived on review, because 212 and 12 do not follow from the run the code writes. Substituting into `(ins_w − bw_reset − gap − bw_record) − gap − (lw_lbl + lblgap + chips)` with `ins_w` 626, both buttons at 88, `lblgap` = `tok::LABEL` = 10 and the chips at their floor (208, four caps plus four gaps) gives `626 − 176 − 12 − 10 − 208` = `220 − lw_lbl`. The conclusion is unchanged: `lw_lbl` is `tw("Shortcut") + s(4)`, so the threshold on the caption is 16 px, which is also what the pre-rewrite `tok::SHORTCUT_COL` entry had derived from the other end — two independent traces agreeing is what settles it. After, it is `450 − chips` = 242 at the chips' floor, so the ceiling binds with 42 px of headroom — that half was right — a THRESHOLD, since no string in this window has been through `GetTextExtentPoint32W` on hardware (gate G1). All three copies (`layout`'s `key_w`, `tok::SHORTCUT_COL`, this row) now carry the same figures |
| `Reset` → **`Revert`** | **done** — 2026-08-15. `cap::REVERT` is `R&evert`, and it **keeps `e`**: design §10 forbids new mnemonics until a uniqueness `#[test]` lands, and `Revert`'s free `v` is not the test — whether anything checks is. Reusing the letter the button already had leaves `mod cap`'s hand-maintained table with one row's caption changed and no key changed. `DefaultButton::Reset`, `IDC_RESET` and the `CONTROL_IDS` name all became `Revert`/`REVERT` too, because a window whose code says one word and whose button says another costs every later reader a lookup. **Id 1033 did not move** — that is what a probe reads |
| Status words `paused` > `in use` > `missing` > `other chord`, healthy row empty | **done** — 2026-08-15. Only the wording moved; the precedence was already the design's, and `row_condition` claims the four in that order. The three renames are each SHORTER than what they replace, which matters because the flag rides inside the App cell and `col_app` is 421 px. ~~New guard: `no_flag_word_is_a_suffix_of_another` — `split_app_cell` recovers the flag by stripping a suffix, so `in use` beside the word it replaced (`key in use`) would have split `Notepad   key in use` into `("Notepad   key", Some("in use"))`. That requirement is as old as `split_app_cell` and had no test until this rename nearly tripped it.~~ **STRUCK and the test deleted, same day, on review.** Traced against the function: `split_app_cell` strips the flag and *then* requires `FLAG_SEP` — three spaces — in front of what is left, so the `in use` arm on `Notepad   key in use` fails on the single space after `key` and falls through to the `key in use` arm, which splits correctly. The pair it named is safe and the hazard cannot occur. The real requirement is narrower (no word may end with `FLAG_SEP` plus another word) and is already carried behaviourally by `split_app_cell_inverts_app_cell_for_every_flag`, which runs the round trip instead of restating a syntactic rule beside it. A test that fires only on non-hazards, cited as the guard for a real one, is worse than none |
| Notes silent when healthy (`Registered and working.` deleted) | **done** — 2026-08-15, and it took three more sentences with it. `No installed app has this name.` and `Uses a different chord.` each only respelled their own status word; the per-row `beckon is paused, so no shortcut is active.` said on every row at once what one word says on each. **`in use` alone kept a note**, reworded to the mock-up's `Another program owns this key. Windows will not say which.` — the second clause is the test of the rule: the word says a program has the chord, and only the sentence says beckon can never name which, which is the difference between picking another key and hunting a culprit no API returns. What survived: `Pick a key and an app.`, `Not registered yet.`, `Checking installed apps...` (all three are "beckon does not know", which no status word claims), the availability verdicts, and every validator `Problem` verbatim |
| `Win+L is reserved` said by Record at the moment it happens, not as a bullet | **done** — 2026-08-15, and the fact had to be **moved off a verdict that could never carry it**. It lived in `probe_notes`' `CaptureSawNothing` sentence — a verdict nothing produces, describing a case `Win+L` cannot reach, since capture *does* see `Win+L` (measurements §48). `Refusal::Reserved` split: `SystemChord` for `Win+L` alone, `Reserved` for the three lock keys and `Ctrl+Alt+Del`. The split is what lets a hint name a mechanism at all — one variant covering two families could only say something true of both, which is why it said nothing about why |
| The list is short and **scrolls** | **done** — 2026-08-15, see §4. Short means "shorter than the config", not "capped": the list now takes the room the page leaves and the scrollbar does the rest |

## §3.2 Keyboard page

| Design says | Status |
|---|---|
| The Caps row exists as its own page | **done** — Task 4 |
| Three `Hold` chips and never four | **done** (pre-existing; `Chord` has exactly ctrl/super/alt) |
| `Tap` is a `CBS_DROPDOWNLIST` read and written by index | **done** (pre-existing) |
| `Write shortcuts as [Caps] instead of [Ctrl][Win][Alt]` toggle, default OFF | **open** — id 1060 reserved. **It carries a fifth job nobody has scheduled: retiring the `other chord` status word.** §3.2 argues that once bindings on the caps chord collapse to `[Caps][B]` while every other chord keeps its full run of chips, "other chord" is visible at a glance and the word can go — and the mock-up is drawn in that future, which is why its `Telegram Web` row (a genuine `ctrl+super+alt+shift+t`) shows an EMPTY status cell while §3.1's own list of four words still names `other chord`. The two are not in conflict, they are dated: `row_condition` produces the word today because the toggle it depends on does not exist. Whoever builds 1060 owns deleting it — and `FLAGS` is a closed four-word table with three tests reading it, so that is a real task, not a line |
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

## The vertical stack is page-dependent — taken 2026-08-15, after two defers

`compute_card_rects` used to reserve the keyboard card's height on **every**
page, so Shortcuts carried a card-shaped hole above the command bar and
Keyboard carried a larger one below the strip. Task 4's agent recommended Task
7 own the re-stack; Task 7 weighed it and deferred, on two reasons that were
good at the time:

- *The re-stack changes the Shortcuts page's vertical geometry, which is
  another workstream's open subject — §4 uncaps the list and deletes
  `tok::ROWS`, §3.1 deletes the editor caption. Doing it now means deriving the
  table twice.* **Spent**: both of those landed in this same pass, which is
  exactly the condition Task 7 was waiting for. The table is derived once.
- *Nothing on the host can display this window; every vertical figure is a hand
  trace, corrected twice already.* **Still true, and it argues for deriving
  carefully rather than for deferring again** — a pass costs the same whenever
  it is taken.

**The shape now.** One `content_bottom` (a `gap_card` above the command bar,
the only anchored thing) and one `content_top` (the strip's bottom, past the
banner). Shortcuts stacks card 0 / card 1 / card 2 down to `content_bottom`;
Keyboard puts card 3 at `content_top` and nothing else. That last half is the
second reason the re-stack was worth taking: **one card at the origin with
space below it is a page with one thing on it; one card at the bottom with
space above it is a page that failed to lay out.** It also makes Keyboard read
the same way System and About already did.

**What it gave back, and where each piece came from.** At 96 DPI the list's
room went from `h − 442 − notes_h` to `h − 332 − notes_h` with the banner up:
**110 px**, being 86 (the keyboard card plus its `gap_card`) + 24 (the editor
caption). The column header returns a further ~21 px inside the list. At the
shipped client height of 600, banner down, that is **288 px of room where there
were 178** — and with `tok::ROWS` gone all of it reaches the list, which is 13
whole rows against 7.

**`tok::ROWS` had to go in the same commit, and that is arithmetic rather than
appetite.** With the cap left in place the 110 px would have arrived as empty
space *below* the editor card — the same void, moved down the window. Anyone
splitting these four changes across two landings should split them the other
way round.

**What is not free.** `list_h` is snapped down to whole rows, so up to
`row_h − 1` (21 px at 96 DPI) is left between the editor card and the command
bar. That is a margin, and it is the price of keeping `Ui::shown_empty` a live
guard — design §12 q2's "keep the whole-row snap or delete the guard", answered
by keeping the snap. The alternative was a list whose last row is sliced
horizontally, which reads as a rendering fault.

## §4 The list

| Design says | Status |
|---|---|
| Uncap the list so it follows window height | **done** — 2026-08-15, and this row was **wrong before**: it read "already true before this work", citing a fix that had landed. The design's own §4 gives the falsifying evidence in the same breath — `let want = list_header_height(..) + row_h * tok::ROWS;` was still in `layout.rs`, and `list_h` was `want.min(room)`, so the list was capped at eight rows at every window height. What had landed was something else. `want` is gone; `list_h` is the room the page leaves |
| `tok::ROWS` should be deleted | **done** — 2026-08-15, and forced rather than chosen: the other three §3.1 deletions return 110 px, and with the cap in place all of it would have re-appeared as empty space below the editor card. `tok::ROW_H` is NOT its replacement and stays on its own reader (`rebuild_state_image_list`) |
| Keep the whole-row snap or delete `Ui::shown_empty` | **done** — 2026-08-15, **snap kept**. `list_h = avail − avail % row_h`, so `list_row_height` is still an input to `compute_card_rects` and `shown_empty` still guards a real transition: the fallback `tok::ROW_H` is a LOWER BOUND on the live row, so the first row to arrive can change the answer. Deleting the guard instead would have been the larger change — it is one of `layout`'s six guarded inputs |

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

**Partial.** Rules 1 and 2 have now been applied to the Shortcuts page in
full; the rest govern wording that has not been written yet (System, About,
the status line).

- **Rule 2, silence is the healthy state**, was the one `row_condition`
  predated — for the FLAG column only. On 2026-08-15 it was promoted to the
  notes as well, which is what the design asks for: four sentences went, one
  was reworded, and the surviving three all say something no status word
  claims. See the §3.1 row.
- **Rule 1, a label is a name and not a sentence**, is what turned `Reset`
  into `Revert`: the old caption named the mechanism (it clears a field) and
  the new one names the effect.

**What rule 2 cost, and it is not free.** `mark` was derived from the notes
alone, so three of the four status words losing their note would have made
every flagged row report `Mark::Ok`. Nothing in either window reads
`ListItem::mark` today — which is exactly why that would have gone unnoticed
until something did. `settings::flag_mark` is the repair, and it assigns each
word the mark its DELETED note carried.
~~so the change is provably behaviour-preserving on `mark` rather than a fresh
opinion: `the_deleted_notes_did_not_take_the_marks_with_them` asserts all four,
with `in use` — the one word that kept a note — as the control.~~

**STRUCK 2026-08-15 on review: it was NOT behaviour-preserving, on exactly one
combination, and the four-row test could not see it.** A row that is **paused**
and whose app is **missing** used to carry both deleted notes — `Warn` for the
pause, `Bad` for the app — and came out `Mark::Bad`. After the deletion the
cell holds one word, `paused` outranks `missing`, and folding the WINNING word
alone reported `Warn`. The four rows of
`the_deleted_notes_did_not_take_the_marks_with_them` each have exactly one
condition, so all four pass either way — which is how a test asserting the very
property that had broken stayed green.

The precedence is for the **cell**. It is not a claim that the outranked
problem stopped existing, and design §3.1 does not say it is.
`row_condition` now keeps a `conditions` vector — every word the row earned, in
precedence order — with the flag as its head and the severity fold running over
all of it; `flag_mark` takes a word rather than an `Option`, since there is no
longer a single "the flag" to hand it. Pinned by
`a_paused_row_whose_app_is_missing_is_still_bad`, **falsified by restoring the
flag-only fold**: that test fails, the four-row test still passes, 274/275.

**The known risk §7 names is now live on this page.** Three conditions speak
in one word each and nowhere else. The design's own mitigation is that the
four words survive at all (§7, *what must never be cut*), because `in use`
and `missing` are the same severity and need completely different fixes. The
second mitigation it names — a registration failure arriving uninvited on the
status line — is §6.4 and has not been built, so **until it is, a failure is
one word on one row and nothing else.**

## What this pass falsified OUTSIDE its own tree

Four of the deletions above contradict standing prose in files that are not
part of the design, and leaving those wrong is worse than leaving them
undone — `CLAUDE.md` is read as instruction by every later session, and
`README.md` is read by users. All corrected 2026-08-15, marked in the repo
form rather than quietly rewritten.

| File | Claim | Why it is false now |
|---|---|---|
| `CLAUDE.md` | the list is *"a **fixed eight rows** (`tok::ROWS`) at every DPI … so it does not grow with the config"* | `tok::ROWS` is deleted (§4). The list takes the room the page leaves and scrolls; what survives is the whole-row snap |
| `CLAUDE.md` | the band list — *"Banner / `Shortcuts` head … / keyboard group / command bar"* | two things: the stack is page-dependent (the keyboard card is no longer reserved on Shortcuts), and the head has no heading in it |
| `CLAUDE.md` | *"`paused` > `key in use` > `not installed` > `custom`"*, and *"derives `mark` from the notes at the end"* | three words were reworded by design §3.1, and `mark` folds the notes **and every condition** — the paused-and-missing defect above is what that sentence had stopped covering |
| `README.md` | the list is *"eight rows tall and staying eight rows tall whatever is in it"*, under a bullet naming **Shortcuts** as the head row | same two deletions, seen from the user's side. `tools/check-site.sh` passes on the edit (it asserts the install commands and the letter→app table byte-for-byte; neither is in this region, and the run was made rather than assumed) |

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

**A new one, unrun, and cheap: G-S8 — did the five 2026-08-15 deletions land
the way the arithmetic says?** (Four when this was written; the `Shortcuts`
heading is the fifth, and it is the one a screenshot answers on its own —
either the word appears twice down the left of the window or it does not.)
One `settings_probe` run answers all of it, and
the instrument is already there. `measure_listview` prints `LVS_NOCOLUMNHEADER`
with a `<<< FAIL` verdict and, if a `SysHeader32` still exists, whether it is
VISIBLE — the style is the primary reading because comctl32 is not documented
to destroy the window, only to stop showing it. `LVM_GETCOUNTPERPAGE` beside
`LVM_GETITEMCOUNT` says how many rows the uncapped list actually got (13 at
600 px and 96 DPI by the table; a14 runs at 144, where `notes_h` and the live
row height both move, so derive before comparing). And a screenshot answers the
void, the missing captions, the missing card heading, the filter's move to the
card's left edge, and the App cue banner at once. **The one thing the
probe cannot answer is whether `CB_SETCUEBANNER` keeps `App` on screen while
the field has focus** — `CB_SETCUEBANNER` takes no flag for it and does not
document which it picks (`cap::APP_CUE`); either behaviour satisfies §3.1, so
this is worth noting rather than gating on.

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
