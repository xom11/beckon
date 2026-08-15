# Four Doors — design vs. reality

**What this is.** One row per commitment in
`specs/2026-08-14-four-doors-settings-window-design.md`, with what is actually
in the tree. Read it to answer "is the window the design asked for yet?"
without reading five other documents.

Status words: **done** (landed and gated) · **partial** (some of it) ·
**open** (not started) · **changed** (the design was wrong or was overruled —
each one says why).

Last updated: 2026-08-15, branch `four-doors-phase-0`. The most recent entry
is *What the 2026-08-15 review pass changed*, after the About section.

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
| Defaults to **dark** | **done** | 2026-08-15, and it is the behaviour change design §5.2 flags rather than a tidy-up. `theme::read_inputs`' `apps_use_light_theme` is no longer `Themes\Personalize\AppsUseLightTheme` — it is `Some(u32::from(!prefs::dark()))`, i.e. beckon's own `HKCU\Software\beckon\DarkMode`, absent meaning DARK. A user on light Windows now gets a dark window. High contrast still outranks it, unchanged, in `theme::resolve` — that is the OS enforcing a choice rather than expressing one. The field keeps its registry-shaped name on purpose: core knows the SHAPE of the answer and the Windows crate knows where it comes from, and a second `ThemeInputs` field would have been two ways to say one thing plus a rule about which wins |
| Transparency slider 85-100 %, default 96 % | **done** | 2026-08-15. `IDC_OPACITY` is a `msctls_trackbar32` with `TBS_NOTICKS`, range set from `OPACITY_MIN`/`OPACITY_MAX`, page size 5. **The tier stays core's and only the LEVEL is the user's**: `apply_current_backdrop` matches `backdrop(...)`, and substitutes `opacity_alpha(prefs::opacity())` for `TIER2_ALPHA` on the `Alpha` arm alone — so a blocked machine (`transparency_block`) never reaches the substitution and the slider can never make an opaque window transparent. Applied on every step of a drag, not on `TB_ENDTRACK`: the window's own alpha is what the user is judging the value by, so a slider you have to let go of to see is not one. **The row went STALE on a live change for one day** — its answer was pushed only by `apply_system_state`, which only `serve` calls, while `on_theme_changed` re-resolved the backdrop and left the row alone. So turning high contrast on (or an `EnableTransparency` flip, which broadcasts `ImmersiveColorSet` without moving `Theme` at all) made the window opaque while the row went on offering a live slider and a percentage. Closed 2026-08-15 by `refresh_transparency_row`, called from `on_theme_changed` beside `apply_current_backdrop` and above its `!changed` return, for that function's own stated reason. It needs nothing from `serve` — the block is a `GetSystemMetrics` plus a registry read, the level is `HKCU\Software\beckon` — and no `UI` borrow, which is what makes it safe at that point in a wndproc. **One predicate with two readers is worth nothing if only one of them is ever asked again**, which is the general form and the reason this is written here rather than only in the code. **Scope, stated rather than implied**: the row now agrees with the backdrop at every moment the backdrop is re-resolved, and no more — entering a remote session raises `WM_WTSSESSION_CHANGE`, not `WM_THEMECHANGED`, and leaves BOTH stale until the next `apply_system_state`. That is one open defect about `SM_REMOTESESSION`, not two about this row |
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
| The Caps switch itself | **changed 2026-08-15, by the System page and not by this workstream.** `paint::toggle` now draws its track at the RIGHT of the control and its caption at the left; it was the other way round. The mock-up draws the Caps row's switch right-aligned like System's three, so this is a step toward §3.2's own drawing rather than away from it — but only a step: the switch lands at the right end of `IDC_CAPS`' own rect, not at the card's right edge, because that needs the three-row Keyboard card this workstream owns. `layout` did not move: a switch's control is its caption plus `toggle_glyph` either way |
| `Write shortcuts as [Caps] instead of [Ctrl][Win][Alt]` toggle, default OFF | **open** — id 1060 reserved. **It carries a fifth job nobody has scheduled: retiring the `other chord` status word.** §3.2 argues that once bindings on the caps chord collapse to `[Caps][B]` while every other chord keeps its full run of chips, "other chord" is visible at a glance and the word can go — and the mock-up is drawn in that future, which is why its `Telegram Web` row (a genuine `ctrl+super+alt+shift+t`) shows an EMPTY status cell while §3.1's own list of four words still names `other chord`. The two are not in conflict, they are dated: `row_condition` produces the word today because the toggle it depends on does not exist. Whoever builds 1060 owns deleting it — and `FLAGS` is a closed four-word table with three tests reading it, so that is a real task, not a line |
| `If Caps Lock does nothing` expander | **open** — ids 1061/1062 reserved |
| The hook-disclosure line moves to About | **done** — 2026-08-15. `beckon_core::settings::HOOK_DISCLOSURE`, drawn by `paint::disclosure` on `IDC_ABOUT_DISCLOSURE` (1111). It is the mock-up's wording verbatim, and `the_hook_disclosure_keeps_both_halves` pins BOTH halves against a later trim: when the hook is held, and what beckon does not keep. The second is a **negative claim** — no icon, colour or control state can draw "beckon keeps no record of what you type", which is why it is a sentence and why nothing on that row but a severity dot is not words. Note the reading of "while Caps Lock is on": it means the SETTING (`keyboard.caps`), not the LED, and the claim is conservative in the safe direction — pausing removes the hook too, so the moments it is really installed are a SUBSET of the two named |

## §3.3 System page · §3.4 About page

| Design says | Status | Where / why |
|---|---|---|
| Both pages exist and open | **done, and no placeholder is left in the window** | Task 7 gave each a `STATIC` reading `Nothing here yet.`, from the reserved TAILS of their ranges rather than the next free number, so deleting a placeholder could not leave a hole in the middle of a page's numbering. **That reasoning has now been paid off twice**: System's went on 2026-08-15 with 1070-1083 intact (`IDC_SYS_PLACEHOLDER`, 1084, RETIRED), About's went the same day with 1100-1114 intact (`IDC_ABOUT_PLACEHOLDER`, 1115, RETIRED). Neither range has a hole, and there is no third placeholder for the argument to apply to. `every_door_owns_at_least_one_control` is the assertion: before Task 7 both doors opened onto the strip, the command bar and nothing between, which reads as a window that failed to draw |
| Every id allocated (1070-1099, 1100-1119) | **done** (Phase 0) | and tested for uniqueness. **System spent all fourteen of its named ids and About all fifteen of its, on 2026-08-15, and between them they invented none** — each page was built from Phase 0's own table, in that table's order |
| Every real System control (pause, autostart, dark, opacity, the two file rows) | **done** | 2026-08-15; the rest of this section is what it cost |
| Every real About control (mark, version, build, location, licence, the hook disclosure, three links) | **done** | 2026-08-15; *What the About page is made of* below is what it cost |

**SPENT 2026-08-15: the waiting lines' own `WM_CTLCOLORSTATIC` branch is
deleted with them.** The entry here read, correctly for its subject: *"About's
placeholder sits on `bg`, not on `card`, and that is a correction to the plan.
The plan and shell spec §8 both say '**both must be added to the `on_card`
match** or they fall through to `COLOR_3DFACE` and draw as grey rectangles'.
The hazard is real and is closed; the prescription was half right. That page
has no card at all … so `on_card` would have painted a card-coloured strip the
width of one line onto a page with no card behind it."* System left that branch
when it grew a card and About left it the next day; neither half applies to any
control that now exists.

**Two findings from it are kept, because the next control drawn outside a card
will meet them again.** Those were the first strings this window drew on bare
ground, and `theme::apply_backdrop` names exactly that change as the one that
reopens Mica's documented hazard: GDI text drawn straight onto glass loses its
alpha and fringes black. `OPAQUE` plus a `bg` fill is what closed it;
`TRANSPARENT` is the spelling that fringes. And the high-contrast pair was
`COLOR_BTNTEXT` on `COLOR_BTNFACE`, same-family, unlike the cross-family pair
the `on_card` branch carries its own correction about. Both survive as a
comment at the site the branch occupied.

**What each page has instead.** System: its five STATICs and switches are in
`on_card`, and its three VALUE slots have a branch of their own in
`text_muted` on `card`. About: the name row and its three VALUE slots are in
`on_card`, its three LABELS joined the `text_muted` branch, and the two
`SS_OWNERDRAW` controls (`IDC_ABOUT_MARK`, `IDC_ABOUT_DISCLOSURE`) are
deliberately in NEITHER — `IDC_NOTES`' rule, not an exception to §8's: an
owner-draw static never asks its parent for a brush at all, and each painter
fills its own rect with `card` first.

### What the System page is made of, 2026-08-15

**Card 4, at the content origin, its height its CONTENTS'.** It follows card
3's shape rather than card 1's: nothing on it flexes, so the page is a card
with space below it — which is what a page with one thing on it looks like —
rather than a card stretched to the command bar. `system_plan` walks nine
slots in the drawing's order (three service rows, a divider, two look rows, a
divider, two file rows) and is the **one** arithmetic three readers share:
`compute_card_rects` takes its `content_h`, `layout` takes the row offsets,
and `WM_PAINT` takes the two divider offsets through `system_dividers`. Three
spellings of "how tall is the System card" would drift, and the drift reads as
a divider through a row.

**A row that is omitted contributes NO height**, the same rule the banner's
card follows — so "omitted, not greyed" is a layout property rather than a
`ShowWindow` that leaves a hole. Which rows exist is
`beckon_core::settings::system_state`'s answer (`autostart: Option<bool>`,
`log: Option<FileRow>`), and the window reads only `is_some()`. It reaches
`layout` and `compute_card_rects` through `SYS_ROWS`, a **thread-local `Cell`
and never `Ui`** — `PILL_BADGE`'s reason, and the fifth time it has decided a
design here: a paint arrives while `UI` is borrowed, and
`compute_card_rects` is documented never to touch it.

**A second push, `apply_system_state`, beside `apply_state`.** This is design
§1's split by store made structural rather than described: `ControlState` is
the projection of a `Model`, and a config that does not parse produces no
`Model` at all — so every System row would have been hostage to a TOML error
it has nothing to do with, which is the defect the design names as fixed "as a
side effect". It takes two arguments (`paused`, `autostart`) and asks for
everything else itself: the paths are already in `CFG`, the log's size is a
`stat`, and the look is `HKCU\Software\beckon`. Passing those in would have
meant `serve` reading Windows-only state on a cross-platform path.

**`Pause shortcuts` and `Reload` go through `SettingsCommand` to `serve.rs`'s
own `set_paused` and `reload`** — the same two functions the tray menu calls,
never a second implementation. `set_paused` does five ordered things and one
of them CLEARS `registered`; a window that flipped a flag itself would leave
nineteen rows claiming to be registered while nothing was. `Start with
Windows` goes the same way, to a new `set_autostart` extracted from the tray's
own menu arm so the Run-key command line has one author.

**One function, two callers — but for a day only ONE of the two ends pushed
the result back.** Sharing the mutator makes the two surfaces agree about what
happens; it does not make them agree about what is on screen. The window's
`SettingsCommand` arms ended in `refresh_settings` and the tray's arms did
not, so a pause from the tray left the System page showing the old switch. See
*What the 2026-08-15 review pass changed*.

| Piece | Decided where | Note |
|---|---|---|
| Which rows exist | `settings::system_state` | `None` omits; the window reads `is_some()` |
| What the transparency slot says | `Transparency::slot` | a percentage, or `TransparencyBlock::reason()` |
| Whether the slider is live | `theme::transparency_block` | **the same predicate `theme::backdrop` uses**, factored out rather than copied — `the_slider_is_blocked_exactly_when_the_window_is_opaque` walks all eight combinations. Re-read on every theme change since 2026-08-15 (`refresh_transparency_row`); before that only `serve` ever asked it again, and the row could say the opposite of what the window was doing |
| The opacity range and its alpha | `OPACITY_MIN`/`MAX`/`DEFAULT`, `clamp_opacity`, `opacity_alpha` | clamped on the way out of the registry too: anything can write that value |
| How a size reads | `settings::size_label` | `112 KB`, Explorer's units |
| Which half of a file row is the label | `system_state` | the file's own NAME; the value slot is the directory or the size |

**Five things this pass did that the design did not ask for, each because
something else forced it.**

1. **`paint::toggle` now draws its track at the RIGHT and its caption at the
   left.** It was the other way round while `IDC_CAPS` was the only switch,
   and the mock-up draws both pages' switches right-aligned (`.srow` is
   `label (flex:1)` then `.sw`). Four switches where there was one is what
   made it matter: a page of rows whose controls line up with the card's right
   edge reads as a column. **This changes the Keyboard page too**, and only
   part-way — the switch moves to the right end of `IDC_CAPS`' own rect rather
   than the card's, because §3.2's three-row Keyboard card is another
   workstream's. Nothing in `layout` moved with it: a switch's control is its
   caption plus `toggle_glyph` either way.
2. **The five System push buttons joined `PUSH_BUTTONS` and `DefaultButton`**
   (9 → 14 in both). Without `BS_NOTIFY` the default ring cannot follow focus
   onto them and `IsDialogMessageW` falls through to `DM_GETDEFID`, which
   still says `Save` — so Enter on a focused `Open config file` glyph would
   have written the config file. That is the `Reload` defect the whole module
   was built for, two pages across. `DefaultButton::visible` answers
   `page == Page::System` for all five, and the log pair's hidden state is
   **unreachable with focus** — `Paths::log` is fixed for the window's
   lifetime, so a hidden log button was hidden from creation and has never
   held focus.
3. **`IDC_OPACITY` is in the `on_card` match.** A `msctls_trackbar32` asks its
   parent for a background brush through `WM_CTLCOLORSTATIC`, and
   `paint::slider_part` only fills the two rects comctl32 hands it — so
   without the id there the slider sat in a `COLOR_3DFACE` rectangle. The
   eight-control defect, reached through a control class instead of a page.
4. **`beckon-windows/src/prefs.rs`**, a new module: `HKCU\Software\beckon`,
   two `REG_DWORD`s. `RegCreateKeyW` rather than `RegCreateKeyExW` because the
   `Ex` form takes a `SECURITY_ATTRIBUTES` and so is gated behind a
   `Win32_Security` feature this crate does not enable — a wider Win32 surface
   to pass `None` to one parameter.
5. **`shell::reveal_path`**, `explorer.exe /select,<path>`. No shell verb does
   this (`"explore"` opens a folder and selects nothing) and the documented
   route is ~60 lines of COM. **No space after the comma and no quotes around
   the path** — Explorer takes the whole remainder of the command line as the
   item, so the usual quoting rule is inverted here.

**Known deviations from the drawing, both deliberate.**

- **The four glyph buttons wear `BtnTier::Secondary`**, so each file row ends
  in two small boxes; the mock-up's `.btn.glyph` is
  `border-color:transparent; background:transparent`, a fifth "ghost" tier. It
  would need its own `colours` arm, its own high-contrast pair and its own
  `theme::pairs` rows before anything drew it, and its resting state would be
  indistinguishable from the card — which is the one thing a button that
  launches something should not be.
- **`IDC_OPACITY_VALUE` holds the row's LABEL and its value in one string**
  (`Window transparency    96%`), left-aligned, with the slider hard right.
  Phase 0's id table gives the row a slider and a value and **no label id**,
  and ids may not be invented — so the two share one control, and a STATIC has
  one alignment. It reads better than the drawing in the forced-off case,
  which is the case rule 7 is about: `Window transparency    Off in a remote
  session`.

**What is NOT built, and is design §3.3's by rights.** `on_open_file` is not
folded into `Open(Target::Config)`. Phase 0's spec assigns that fold to this
workstream ("The System workstream folds it and deletes the field"), and it is
four sites including `beckon-macos`'s window and its probe's complete-literal
`Callbacks` — a behaviour-neutral refactor across a platform this pass does
not otherwise touch. The window now has two ways to open the same file, which
is a duplication and is the reason this is written down rather than dropped.
`Target::{Github, Releases, BugReport}` were likewise unhandled — *"they
belong to About, which has no controls, and a link that opens the wrong page
is worse than one that is not built"*. **That half closed the next day**:
About was built, and `open_target` answers all three through `Target::url`.
The `on_open_file` fold is still open and still the System workstream's.

**Nothing on this page has been seen.** Every figure above is arithmetic and
code, checked by five green gates on two Windows targets and by hand-verifying
the four id invariants that only run on the Windows CI job (46 declared ids ==
46 `MINE` rows; 36 `PAGE_CONTROLS` + 7 chrome + 3 banner == 46; 14
`PUSH_BUTTONS` == 14 `DefaultButton::ALL`; no retired id reclaimed).
`examples/settings_probe.rs` gained a `measure_system` section for the run
that would change that — see the gates table.

### What the About page is made of, 2026-08-15

**Card 5, at the content origin, its height its CONTENTS' — cards 3 and 4's
shape a third time.** `about_plan` walks nine slots in the drawing's order
(mark, name, divider, three value rows, divider, disclosure, links) and is the
**one** arithmetic three readers share: `compute_card_rects` takes
`content_h`, `layout` takes the row offsets, `WM_PAINT` takes the two divider
offsets through `about_dividers`. **Nothing on the page is conditional** —
unlike System's two omittable rows, there is no fact about a machine that
removes a row here, so `about_plan` takes no `rows` argument.

**One thing is unlike either earlier card: a height that is MEASURED.** The
hook disclosure is the only wrapped prose in the window, so
`disclosure_height` runs `DT_CALCRECT | DT_WORDBREAK` over
`HOOK_DISCLOSURE` at the width the painter will get, and `paint::disclosure`
draws with the identical flag set (`paint::DISCLOSURE_FLAGS`) at the identical
width (`layout::disclosure_text_w`). A fixed line budget was the alternative —
`notes_height`'s shape — and it loses: that sentence is ~150 characters, so it
takes two lines in the shipped card and three in a narrower one, and a budget
short by one line clips the half that is the feature.

**`DT_END_ELLIPSIS` is deliberately NOT in that flag set**, and it was for an
hour. Every other `DrawTextW` in `paint.rs` carries it as a net. Here the net
is unnecessary (measure and paint agree by construction) and the flag is a
hazard: the ellipsis flags are documented against `DT_SINGLELINE`, and if they
collapse a `DT_WORDBREAK` `DT_CALCRECT` to one line, the two calls still AGREE
and the window quietly draws a privacy claim truncated before the sentence
that is the point of it. **Clipping is the better failure — it reads as damage
rather than as a shorter promise.**

| Piece | Decided where | Note |
|---|---|---|
| What each row shows | `settings::about_state` | `AboutValue { shown, copy }` — the type is what says the two can differ |
| What each copy button copies | `settings::copy_text` | the row's **bare payload**: an annotated path fails in the only two places a copied path goes |
| Whether the running image is stale | `settings::image_age` | two producers since 2026-08-15 — an identity test and a clock — and only the first can see the recorded failure; see below |
| Whether the running image IS the launch path's file | `settings::image_identity` | fails safe: the untested Win32 reading costs silence, never a false alarm |
| What the verdict says | `ImageAge::note` | `None` for `Current` AND `Unknown` — rule 2 |
| Where the three links go | `Target::url` | three https URLs under `xom11/beckon`, with a test that no two are equal |
| The disclosure's wording | `settings::HOOK_DISCLOSURE` | both halves pinned by a test |

**`Location` is the highest-value row and the reason it is trusted is that it
does LESS than it could.** The path comes from `current_exe()` and is
deliberately **not** resolved through `GetFinalPathNameByHandleW`: resolving
reports today's junction target, which is precisely the surface that lied on
a14 when a watchdog-started beckon ran the 0.8.0 image for three hours while
`--version` and scoop's `current` both said 0.9.0.

~~**The verdict is one-sided, and `ImageAge::note` is silent about it in the
right direction.** `Replaced` (the image's mtime is after this process's start
time, from `GetProcessTimes`) is reliable; `Current` is only *no evidence of
replacement*, because an extractor that preserves an archive's stored
timestamp — which scoop's unpack does — gives a newly installed exe an mtime
from the release build. So the row says nothing at all for `Current` and for
`Unknown`.~~

~~*Named but not built, for whoever has hardware:*
`QueryFullProcessImageNameW` is documented to return the executable path **of
the process**, which for a launch through a junction should be the resolved
target as it was at load time — the version directory actually running.
Comparing that against today's resolution would be an identity test rather
than a clock one. Nothing on this host can run a Windows process, so it is
written down rather than guessed at.~~

**STRUCK 2026-08-15 on review, and the second half is now BUILT. Every
sentence above is true and together they say something the row never said out
loud: the clock comparison could not fire on the incident the row exists
for.** Both paragraphs sat two lines apart and nobody joined them.

**Measured, not reasoned.** `beckon-0.9.0-aarch64-pc-windows-msvc.zip` was
downloaded from the release a14 updated to and its zip directory read:

```
beckon-serve.exe  2026-08-12T22:37:14
beckon.exe        2026-08-12T22:37:18
```

Those are the `LastWriteTime`s `Compress-Archive` stored
(`.github/workflows/release.yml`, Windows packaging step), and every extractor
scoop uses restores them. The a14 timeline: the watchdog started beckon at
05:40:01 and scoop created `…\apps\beckon\0.9.0` **four seconds later**. scoop
cannot unpack an artifact before it exists, so the process started at most
four seconds before something that necessarily follows 22:37:18Z — therefore
`written < started` **in every timezone**, the comparison answers `Current`,
and `note()` is `None`. **The row said nothing for the three hours it was
built to describe.** No arithmetic about a14's clock is needed; the ordering
is forced by the causality.

Two mechanisms put it there, either sufficient. (1) The mtime is the release
BUILD's, so a freshly unpacked image and a months-old one look the same to it.
(2) `metadata(current_exe())` follows the `current` junction, so the file
being timed is the NEW image — the clock half is structurally timing a file
this process is not executing.

**What was built instead: `settings::image_identity`, and the reason it could
be shipped unmeasured is that it fails SAFE.** `QueryFullProcessImageNameW`
against `canonicalize(current_exe())`, both sides canonicalised. If it returns
the resolved image (documented), the two differ whenever the junction has
moved — `Diverged`, and the row speaks. If it returns the launch path (which
is what `MainModule.FileName` showed on a14), canonicalising it yields today's
target, the two are equal, and the answer is `Same` — silence, exactly today's
behaviour. **The pessimistic reading costs a missed warning, never a false
alarm**, which is what makes an untested Win32 reading cheaper to build than to
go on naming. A wrong identity check would cry *restart to run it* at every
scoop user on every open.

**The clock half is KEPT, and not out of caution**: it catches an in-place
overwrite, where the path never moves and identity therefore says `Same`.
`cargo build` over a running binary is the everyday case; a non-scoop install
updated by copying a new exe over the old is the shipped one. `Same` does not
short-circuit to `Current`, and
`the_two_halves_of_the_verdict_do_not_shadow_each_other` is what says so.

**`Missing` is the third verdict and is fully reliable** — a launch path that
no longer resolves is a process running an orphaned image, which `scoop
cleanup` produces.

**Still unrun, and now cheap to run:**
`the_a14_timeline_is_silent_on_the_clock_and_loud_on_identity` pins both
halves against those numbers on all three CI jobs, but whether
`QueryFullProcessImageNameW` resolves a junction is a fact about Windows that
no test here can reach. The run is `scoop update beckon` with an old
`beckon-serve.exe` still alive, then open About: a verdict means the
documented reading holds, silence means it does not. `measure_about` says so
in its own output now, so a person running the probe does not have to know
this document.

**Five things this pass did that the design did not ask for.**

1. **The About page's six push buttons joined `PUSH_BUTTONS` and
   `DefaultButton`** (14 → 20 in both), for the reason System's five did. The
   extra consequence worth naming: three of them are copy glyphs, and a stray
   Enter on one silently replaces whatever was on the clipboard — a loss the
   user does not see until they paste.
2. **`build.rs` gained `stamp_target`**, four lines forwarding cargo's own
   `TARGET` as `BECKON_TARGET`. A `cfg!`-derived triple answers the question
   that motivates the row (a14 is ARM64 and runs x64 under emulation) equally
   well and cannot see a vendor other than `pc`; the stamp is taken because
   the build script already existed for the examples' manifest. **It carries
   no build DATE**, which the drawing shows: a stamped date is really "when
   the build script last ran", cargo caches that, and the version row above
   answers *how old is this* without being able to drift from the running
   process.
3. **`beckon-windows/src/clipboard.rs`**, a new module, and two `windows`
   features with it (`Win32_System_DataExchange`, `Win32_System_Memory` —
   the latter was already a dev-dependency feature for the probe). Ownership
   of the `HGLOBAL` passes to the system on success and must not be freed
   after it; every failure path frees it and the success path deliberately
   does not.
4. **`shell::open_url`**, which **refuses anything that is not `https://`**.
   Every caller today passes a compile-time constant from `Target::url`, so
   nothing hostile can reach it — and that is exactly the property that stops
   holding silently the day a URL comes from a config file. `ShellExecuteW`'s
   `open` verb runs whatever the string names.
5. **`Role::Subtitle` has a reader again** and its `#[allow(dead_code)]` is
   gone. It was kept for one day with a comment naming `ABOUT_NAME` as the
   next control that would want an 18 px semibold; that prediction is spent.

**Known deviations from the drawing.**

- **No build date** in the `Build` row — see (2) above.
- **The mark's tile is 36 px, not the mock-up's 48.** That drawing pairs 48
  with a 28 px letter (ratio 0.58) and this window's type scale tops out at
  the 18 px `Role::Subtitle`. Inventing an eighth role for one letter is a
  change to the scale; shrinking the tile is not, and 18/36 is 0.5.
- **`Location` shows the full image path, where the drawing shows a
  directory** (`…\scoop\apps\beckon\current\`). §3.4's own bullet says the row
  must carry the *running image path*, which a directory is not; the drawing's
  value is sample data and its bullet is the specific instruction.
- **The three copy glyphs wear `BtnTier::Secondary`**, like System's four and
  unlike the mock-up's transparent `.btn.glyph` — the same fifth "ghost" tier
  that pass declined to build, for the same reasons.

**What is NOT built and is still open from the System pass**: `on_open_file`
is not folded into `Open(Target::Config)`. Phase 0 assigns that to the System
workstream and it stays open; the About links needed no such fold, because
`Target::{Github, Releases, BugReport}` were already unhandled `_` arms rather
than a duplicate callback. Those three ARE handled now, in `open_target`.

**Nothing on this page has been seen either.** Every figure is arithmetic and
code, checked by the five gates on two Windows targets and by hand-verifying
the same id invariants (60 declared ids == 60 `MINE` rows; 50 `PAGE_CONTROLS`
+ 7 chrome + 3 banner == 60; 20 `PUSH_BUTTONS` == 20 `DefaultButton::ALL`; no
retired id reclaimed, 1115 now among them). `examples/settings_probe.rs`
gained a `measure_about` section — and it is the only check there will ever be
on `COPY_GLYPH`, the third and least certain non-ASCII string this window
draws.

## What the 2026-08-15 review pass changed

Five defects, each verified against the tree before being touched; none was
rejected. Four are above in their own sections; the two that are only here:

**The tray and the window could disagree on screen, and only one direction of
the refresh existed.** `SettingsCommand::SetPaused` / `SetAutostart` end in
`refresh_settings`, so a switch flipped in the window reaches `serve` and comes
back. The tray's own `MENU_PAUSE` and `MENU_AUTOSTART` did not — and design
§3.3 put those same two controls on the System page, so from that day the two
surfaces could show opposite states with nothing to say which was real.

**Closed by pushing, not by pulling, and the direction is the whole decision.**
The window has nothing to pull on: it runs no timer, and Windows broadcasts
nothing when beckon's own `paused` flag moves — so a pull would mean a timer
ticking for the ~always that the window is closed. The push is one line at each
of the two arms that already own the mutation, and `refresh_settings` returns
immediately when there is no window, so it costs a `RefCell` read. It goes
AFTER the mutator and never inside it: `set_paused` returns holding no borrow,
while a call from inside would put a `SendMessageW` fan-out inside
`mgr.borrow_mut()` — the shape `serve.rs`'s module doc rules out. Same order
`on_command` already uses.

**`MENU_RELOAD` needed nothing and that is checked rather than assumed**: the
`Ok` arm of `reload` already ends in `settings_saw_external_change` and the
`Err` arm in `settings_retry_unreadable`. Both directions were covered by the
watcher's own path.

**macOS gets the push too, for a weaker but real reason.** That window has no
System page and so no pause switch — but every Shortcuts row's status word
comes from `RuntimeStatus::paused`, and `set_paused` CLEARS `registered`. A
pause from the menu bar left an open window claiming nineteen rows were
registered. `settings_saw_external_change` is Windows-only and is about the
FILE, so it never covered this.

**Two doc comments had come loose**, both in commit `1e7c33a`, both by the same
slip: a function inserted between a doc block and the item it documented.
`reload`'s **borrow-safety argument** ended up on `set_autostart` — a
paragraph about holding `mgr.borrow_mut()` across `set_tray_status`,
documenting a registry write that takes no `mgr` at all — while `set_paused`'s
own doc says *"see `reload`'s doc comment"* and pointed at an empty place.
That cross-reference is what makes it a defect rather than a tidiness point.
`apply_state`'s two-line summary ended up on `opacity_slot`, where "the only
path that changes what is on screen" described a `format!` of two strings.

**Both found by a mechanical scan, and the scan is the part worth keeping**:
for every commit on this branch, an ADDED item line (`fn` / `pub` / `#[…]`)
whose immediately preceding line is an unchanged `///` context line. That
found exactly these two and nothing else. A static pass over the same files
(a one-line `///` paragraph wedged before a blank `///`) turned up nine
candidates, of which seven are ordinary continuation sentences — so the
git-shaped scan is the one with a usable signal-to-noise ratio, and it is
recorded here because this is the second time this failure mode has been found
in these files (`split_app_cell`'s doc, `settings.rs:275`, carries the first).

**`PROBE_PINNED_IDS` widened from fifteen to forty-four.** The list held only
the probe's `const IDC_*` declarations, and its own doc counted them by
`grep -c "const IDC_"`. But `measure_system` transcribes 1070-1083 and
`measure_about` transcribes 1100-1114 as **bare literals** in `ROWS` tables
and in the arms that read them — the same fixed points across the same process
boundary, differing only in spelling. The twenty-nine are now pinned. Note the
names are `CONTROL_IDS`' (`ABOUT_MARK`), not the probe's printed labels
(`MARK`): what is pinned is the number, and a test that matched labels would
fail on a cosmetic column change while saying nothing about a renumber.

**The doc's count claim now has a test**, because it has been wrong twice —
`probe_pinned_ids_count_matches_its_doc`, whose failure message quotes the
words to edit, plus two uniqueness assertions since the list is maintained by
hand from another file (a duplicated pair would make the count true for the
wrong reason).

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
| Transparency slider is buildable via `SetLayeredWindowAttributes` | **done** — 2026-08-15, and it is buildable exactly as §5.1 argues: `apply_backdrop`'s `Alpha` arm already called it, so the change is which number goes in. See the §2 row for the tier/level split |
| Dark by default | **done** — 2026-08-15, see the §2 row. Flagged there as the behaviour change it is |

## §6 Auto-save

**Not started.** No Save/Close removal, no debounce, none of the eleven guards
(G-a … G-k).

**The System page is the first proof that §6's "every valid change is written
immediately" is workable**, and it is a weak one that should not be read as a
strong one: that page has no Save and every row applies on change, which is
exactly §6's shape — but it writes the REGISTRY and the Run key, never
`apps.toml`, so none of the eleven guards is about anything it does. A row
whose write cannot lose a hand edit is not evidence about rows that can.

Two things from §6's neighbourhood did land early because Task 4 made them
urgent:

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
full, and rules 3, 6 and 7 govern the System page as built; the rest govern
wording that has not been written yet (About, the status line).

- **Rule 3, a fact about this machine is a value**, is why the System page has
  three right-hand slots — `…\shortcuts\`, `112 KB`, `96%` — drawn at
  `Role::Caption` in `text_muted` rather than at Body weight, so a row does
  not read as two labels.
- **Rule 6, a choice nobody turns off is not a choice**, was applied by
  DELETING before anything was built: *Remember size and position*, *Show
  error notifications* and *Copy diagnostics* are not on the page and no id
  was spent on them. Phase 0 had already allocated none, which is the design
  working a step ahead of the build.
- **Rule 7, a disabled control explains itself in its own slot**, is the
  transparency row and is the one rule with a mechanism behind it rather than
  a habit: a disabled Win32 control receives no mouse messages, so a tooltip
  there silently never appears. `Transparency::slot` returns the percentage or
  the reason, never both, into the same control on the same line —
  `Window transparency    Off in a remote session`.

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
| `CLAUDE.md` | *"The **only** file beckon reads is the `serve` shortcuts TOML — and since the settings window, the only file it writes"* | still true of FILES, and there is a second store now: `HKCU\Software\beckon`, two `REG_DWORD`s (`prefs.rs`). Corrected in place rather than rewritten, because the sentence as written is not wrong — the split by store is what the addition IS |
| `CLAUDE.md` | the settings window *"lists installed apps only to fill in a Name … and never focuses or launches anything"* | still true of the shortcut table; the window is now `serve`'s control surface as well as its editor — pause, reload, autostart, theme, transparency, open and reveal. Widened in place, with the "never re-implement `set_paused`" rule stated where the window is described |
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

**G-S9, unrun, and the page it is about has never been displayed: does the
System page draw?** `examples/settings_probe.rs`'s `measure_system` is the
instrument, and it must be run **with the System door open** — everything on
that page is behind it, so on any other door every control reads `hidden` and
the section says nothing (which is itself the check that `show_page_controls`
covers the new rows). It prints all fourteen ids with their rect, visibility
and enabled state; the three switches' `BM_GETCHECK`; the trackbar's position
AND its range, since `paint::slider_part` reads that range back to decide how
much of the channel to fill and a 0..=100 range with a position of 96 would
draw a plausible bar in the wrong place for ever; and a verdict line on each
conditional row plus one on the transparency slot, which is the only place the
forced-off reason appears at all. A screenshot answers the rest: whether the
two dividers land between the groups rather than through a row, whether the
switches' tracks line up on the card's right edge, whether the glyphs
(`U+2197`, `U+25A4` — the first non-ASCII this window puts on screen) draw as
arrows or as boxes, and whether `SS_PATHELLIPSIS` shortens the config
directory or merely clips it under `SS_RIGHT`, which is the one pairing the
Win32 documentation leaves ambiguous.

**G-S10, unrun, same shape one door across: does the About page draw?**
`measure_about` is the instrument and it must be run **with the About door
open**, for `measure_system`'s reason. It prints all fifteen ids with their
rect, visibility and text, then four verdicts a reader should not have to
infer:

- **The copy glyph.** `U+29C9 TWO JOINED SQUARES` is the third non-ASCII
  string this window draws and the least certain of the three — the other two
  were argued to be in Segoe UI's own coverage, while this is a mathematical
  symbol likely to arrive through font linking. Reading the caption back does
  **not** prove it rendered (a face without it still reports the character and
  draws a box), so this is the cheap half and a screenshot is the other.
- **The `Location` text**, printed raw. It must be the launch path with
  `\current\` still in it; a version number in that string is the tell that
  something started resolving it, which is the surface that lied on a14.
- **The verdict**, present or silent, so a run on a machine that has just been
  updated says so instead of leaving the reader to compare timestamps. **Since
  2026-08-15 this line is also the answer to an open question about Windows**:
  run straight after `scoop update beckon` with the OLD `beckon-serve.exe`
  still alive, a verdict means `QueryFullProcessImageNameW` resolves a
  junction and silence means it returns the launch path instead. Either is a
  result; the identity check is built so that the second costs silence rather
  than a wrong answer. The probe prints that instruction itself.
- **The disclosure**, both halves and a character count — a control whose text
  comes back truncated is a promise half-made.

A screenshot answers the rest: whether the mark's tile reads as a mark at
36 px with an 18 px letter in it, whether the two dividers land between the
groups, whether the three links centre as a run, and whether the wrapped
disclosure fits the box `DT_CALCRECT` measured for it — the one place where a
disagreement between measure and paint would show, and the reason
`DT_END_ELLIPSIS` is deliberately absent from both.

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
