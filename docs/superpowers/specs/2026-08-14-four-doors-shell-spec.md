# Four Doors — the shell: tab strip, page switching, and 680 px

**Status: spec, agreed 2026-08-14.** Input to the shell implementation plan.

Parent design: `2026-08-14-four-doors-settings-window-design.md` §2 (settled,
not up for redesign). Drawing: `2026-08-14-four-doors-mockup.html`.
Predecessor: `2026-08-14-four-doors-phase-0-spec.md` (landed — id ranges,
`Page`, `Paths`, `SettingsCommand`, `on_command`).

Every file:line below was opened during a five-region survey of the source.
Where the survey contradicted the design, the source wins and the disagreement
is recorded.

---

## 0. The decision in one paragraph

Four `BS_AUTORADIOBUTTON | BS_PUSHLIKE` pills sit in a painted trough between
the client-drawn title bar and the first card. Switching pills hides one
page's controls and shows another's, then calls `layout` — which is the
window's measured data-loss path, so **skipping off-page controls is a
correctness requirement, not an optimisation**. The window narrows to 680 px.
`MIN_HEIGHT` stays 560 and its four-row promise is formally withdrawn. This
landing ships Shortcuts and Keyboard as real pages built from controls that
already exist; System and About are a single waiting line each.

---

## 1. Verified in the source

| Claim | Evidence |
|---|---|
| The client rect **is** the window rect, all four edges | `chrome.rs:142` — `nccalcsize` is `LRESULT(0)` with no `DefWindowProcW` and no use of either parameter; no `WS_CAPTION` on the window |
| A strip below the title bar needs **no** drag-zone arithmetic | `chrome.rs:252-256` — `nchittest` returns `HTCLIENT` for every `pt.y >= rc.top + TITLEBAR_H*dpi/96`, before it considers `HTCAPTION` at `:265` |
| There is exactly **one** "content starts below the bar" statement | `layout.rs:241` — `let mut y = pad + s(chrome::TITLEBAR_H);`. The other four `TITLEBAR_H` consumers (`chrome.rs:91`, `:292`, `:252`, `mod.rs:4791`) all stay inside the bar |
| `compute_card_rects` owns all vertical geometry | `layout.rs:177-312`, destructured at `layout.rs:500` (`layout`) and read back through `card_rects` (`layout.rs:322-328`) at `mod.rs:4985` (`WM_PAINT`) |
| The strip is paid for by `list_h` and nothing else | `layout.rs:294`. `bar_y` (220), `kb_card_h` (222), `kb_y` (223), `card3` (224), `card2_h` (236), `editor_min` (292) are bottom-anchored or content-derived |
| Creation order **is** tab order | `mod.rs:2300-2306` |
| `WS_GROUP` appears exactly once in the file | `mod.rs:2828` |
| `set_button_type` would rewrite a radio into a push button | `mod.rs:1679-1694`, masking with `BS_TYPEMASK_BITS` (`mod.rs:189`) |
| Every `PUSH_BUTTONS` member must map to a `DefaultButton` | `mod.rs:353-363`; the test at `mod.rs:6481-6498` |
| `TranslateAcceleratorW` runs **before** `IsDialogMessageW` | `mod.rs:1299-1325` |
| Hiding a control raises no focus notification, so the default ring is left on an off-screen button | `mod.rs:1569-1578`, which is why `repair_default_button` (`mod.rs:1633-1668`) exists |
| `SetWindowPos` on a populated `CBS_DROPDOWN` destroys typed text | `mod.rs:1033-1042`, measured; `Ui::shown_external` exists for it |
| A paint can arrive while `UI` is already borrowed | `mod.rs:1378-1383` (`CHIPS`), `:4238-4243` (`CAP_FONT`), `:1704-1710` (`SHOWN_NOTES`) — measured on a14 |
| `layout` sizes buttons from their caption | `layout.rs:432-434` |
| Custom draw must read HC as `cache.theme()`, never `high_contrast()` | `paint.rs:1156-1162`; the `Cell` at `mod.rs:4218-4232` refreshes only on `WM_SETTINGCHANGE` |
| A `STATIC` not in the `on_card` match falls through to `COLOR_3DFACE` | `mod.rs:5363-5375`, and the eight-control defect recorded at `:5294-5309` |
| `paint::button` takes its caption from the control, one string, no second field | `paint.rs:1228` |

### 1.1 Where the design was wrong

- **`MIN_HEIGHT` was never one pixel short.** The derivation at `mod.rs:731`
  added `+ 8  bottom frame`, which describes `nccalcsize` before `c523e8e`
  reclaimed the whole frame. With client == window the floor gets
  `list_h = 560 − 408 − 36 = 116` against the 109 four rows need — it cleared
  them by 7 px. Corrected on this branch, along with the rest of that prose
  class: the term list above `WINDOW_HEIGHT`, the `MIN_WIDTH` G3
  parenthetical, the floor/shipped re-trace, `notes_height`'s anchor
  (`561 → 553`), `theme::apply_dwm_border`'s justification and both of its
  call sites.
- **Gate G3 is settled by reading**, not by hardware. What remains is
  confirming it with the probe that already prints both rects
  (`examples/settings_probe.rs:781-862`) and the *look* question of painting
  into the invisible resize-border strip.
- **`text_muted` on the trough is fine.** The survey reported it failing, but
  measured it against `strip_hover`; the mockup only uses `text_muted` on
  `strip` and switches to `text` on hover. Re-measured: 4.522 LIGHT /
  5.237 DARK on `strip`, 10.700 / 8.664 for `text` on `strip_hover`.
- **`LIGHT.strip_hover` as drawn fails.** `#CBD1DE` measures **1.126**
  against `#D9DDE7`, under the 1.2 border floor. See §6.2.

---

## 2. Geometry

### 2.1 The band

Tokens, added to `layout.rs`'s `mod tok`:

| Token | Value | Role |
|---|---|---|
| `TABSTRIP_H` | 36 | the trough's height |
| `TAB_PAD_X` | 14 | pill inner padding, left and right |
| `TAB_PAD_Y` | 2 | the trough's inner padding around the pill row |
| `TAB_VISUAL` | 26 | a pill's drawn height |
| `FOCUS_SLACK` | 3 | pill margin inside the trough; the perceived gap between two pills is `2 × FOCUS_SLACK = 6 = tok::GAP` |

`36 = TAB_VISUAL 26 + 2·TAB_PAD_Y 2 + 2·FOCUS_SLACK 3`, which is why
`TABSTRIP_H` is not an independent number. `TAB_GAP` is 0 and is therefore
not a token.

### 2.2 The one edit

`layout.rs:241` becomes

```rust
let mut y = s(chrome::TITLEBAR_H) + s(tok::TABSTRIP_H) + gap_card;
```

**The surface `PAD` above the first card is spent by the strip, not added
to it** — the mockup's `.tabstrip{padding:0 10px 8px}` and
`.page{padding:0 10px 10px}` put no padding above the trough and none above
the first card. So the net cost is `34`, not `44`:

```
before:  pad 10 + TITLEBAR_H 34                          = 44
after:   TITLEBAR_H 34 + TABSTRIP_H 36 + GAP_CARD 8      = 78
```

Nothing else in the module may add a second offset. `WM_PAINT` needs no
change: its card loop already skips degenerate rects (`mod.rs:4985-4993`).

### 2.3 What it costs the list, and the promise being withdrawn

`list_h = h − 408 − notes_h − 34` once the strip lands.

| | before | after |
|---|---|---|
| at `WINDOW_HEIGHT` 600, banner down | 8 rows | 7 rows |
| at `MIN_HEIGHT` 560, banner up | 4 rows (116 px) | **2 rows** (82 px) |

**Decision: `MIN_HEIGHT` stays 560 and the four-row guarantee is withdrawn.**
`mod.rs:724-729` currently reads "a window whose list shows one row is not a
smaller version of this window, it is a broken one" and derives the floor from
four rows. That paragraph must be rewritten, not deleted: it records a real
standard, and the reason it no longer applies is design §4 — the list is short
and **scrolls**, so a floor's job stops being "enough rows to see context" and
becomes "enough rows to see that it is a list". Two rows plus a scrollbar
meets that; one row does not, and 560 is where two rows stop fitting, so the
constant keeps its meaning even though its derivation changes.

The alternatives were costed and rejected: raising `MIN_HEIGHT` to 596 keeps
four rows but makes the window less draggable for a promise design §4 already
retired, and waiting for the Shortcuts workstream to delete the editor's
`Editing "…"` caption (which returns 24 px, making 572 sufficient) couples two
landings that are otherwise independent.

### 2.4 The trough's rect

`compute_card_rects` keeps returning `[RECT; 4]`. The trough gets its own
function:

```rust
/// The tab strip's trough, in client coordinates.
///
/// Separate from `compute_card_rects` because it is not a card and the
/// `WM_PAINT` card loop must not draw it — but it is the SOURCE of the
/// strip's height, and `compute_card_rects` calls it rather than repeating
/// `s(tok::TABSTRIP_H)`. Two copies of that arithmetic would drift, and the
/// drift would look like a rendering bug (`layout.rs:158-163`).
pub(super) fn strip_rect(rc: RECT, dpi: u32) -> RECT
```

Inset by `tok::PAD` left and right, matching the cards. **That inset is
load-bearing beyond looks**: `chrome::nchittest` resolves the eight resize
directions itself and is only consulted for points no child covers, so a pill
reaching the client edge would kill the left/right resize edge across the
strip's whole band. `PAD` is 10 at 96 DPI against a border of roughly
`SM_CXSIZEFRAME + SM_CXPADDEDBORDER` ≈ 8, and 15 against ≈12 at 144 DPI — a
margin of 2-3 px, thin enough that the metrics should be printed by name in
the probe (§9 G-S5).

---

## 3. The controls

### 3.1 Class and styles

```
BS_AUTORADIOBUTTON | BS_PUSHLIKE | WS_CHILD | WS_VISIBLE | WS_TABSTOP
```

`BS_OWNERDRAW` is refused for two reasons already measured in this source:
owner-draw never receives `ODS_HOTLIGHT` (`mod.rs:4375-4385`, "the one bit a
REAL `WM_DRAWITEM` never carries"), so there would be no hover state; and it
kills `BM_GETCHECK`, which is why `WM_CHIP_STATE` had to be invented
(`mod.rs:277-296`).

Ids are fixed by Phase 0: `IDC_TAB_SHORTCUTS 1040`, `IDC_TAB_KEYBOARD 1041`,
`IDC_TAB_SYSTEM 1042`, `IDC_TAB_ABOUT 1043`. Contiguous, which
`CheckRadioButton` requires.

### 3.2 Four rules that are forced, not chosen

1. **Created first in `build_children`, ahead of `IDC_BANNER`**
   (`mod.rs:2320`). Creation order is tab order (`mod.rs:2300-2306`) and the
   strip draws above everything.
2. **The control created immediately after the last pill carries `WS_GROUP`.**
   Today `mod.rs:2828` is the file's only `WS_GROUP`. An auto-radio group and
   `IsDialogMessageW`'s arrow-key group both run *until the next `WS_GROUP`*,
   so without a closing boundary Left/Right walks out of the strip into the
   banner, the filter EDIT and the ListView — and the auto-radio's
   clear-siblings pass sprays `BM_SETCHECK(0)` across them.
3. **The pills are not in `PUSH_BUTTONS`** (`mod.rs:353-363`). Membership
   would put them through `set_button_type` (`mod.rs:1679-1694`), which
   read-modify-writes `BS_TYPEMASK_BITS` and would rewrite
   `BS_AUTORADIOBUTTON` (9) to `BS_PUSHBUTTON` (0); and the test at
   `mod.rs:6481-6498` requires every member to map to a `DefaultButton`,
   which a tab must never be.
4. **Setting the active pill uses `CheckRadioButton(hwnd, 1040, 1043, id)`**,
   not this file's `check()` — which falls through to `BM_SETCHECK`
   (`mod.rs:4120-4135`) and does not clear sibling auto-radios.

### 3.3 Captions carry no `&` mnemonic, and this is arithmetic

`mod cap`'s collision table (`mod.rs:382-390`) claims
`A M U C O S E R K T W L D`. Free letters are `{B,F,G,H,I,J,N,P,Q,V,X,Y,Z}`.
`Shortcuts` can only take `h`; `System` only `y`; `About` only `b`;
`Keyboard` only `y` or `b`. System takes `y`, so Keyboard must take `b`, and
About is left with nothing. **Four unique mnemonics do not exist**, which
settles the design's "no mnemonics on tab names" by counting rather than by
taste.

---

## 4. Page switching

### 4.1 Pages hide, they never get destroyed

Three things keep working on a hidden control and would break on a destroyed
one: `enable` / `check` / `set_text_if_changed` all resolve through
`GetDlgItem` (`mod.rs:1349`, `:4120`, `:5755`); `list_row_height` behind
`Ui::shown_empty` (`mod.rs:1067-1070`) needs the ListView to exist in order to
measure a row; and `IsDialogMessageW`'s `GetNextDlgTabItem` already skips
non-`WS_VISIBLE` controls, so hiding removes a page from the tab order for
free.

### 4.2 The dangerous part

A tab switch must call `layout` directly, the way `WM_SIZE` does
(`mod.rs:4767`) — not through `apply_state`, which nothing calls on a tab
click. And `layout` is `SetWindowPos` on the populated App combo
(`layout.rs:650`), the measured data-loss path.

**Therefore `layout` must place only the current page's controls.** This is a
correctness requirement. The sharp case is `Ctrl+1..4`:
`TranslateAcceleratorW` runs before `IsDialogMessageW` (`mod.rs:1299-1303`)
and moves no focus, so without the skip the combo is resized *while focused
and populated* — exactly the shape that lost typed text before.

`Page` reaches `layout` as a `Copy` field in `LayoutHandles`
(`layout.rs:124-151`), sourced from the `PAGE` thread-local Phase 0 already
created (`mod.rs:1251`). It is a `Cell<Page>`, so reading it takes no `RefCell`
borrow and `compute_card_rects` keeps its documented "never touches `UI`"
property (`layout.rs:165-168`).

### 4.3 Five consequences that are easy to miss

1. **`repair_default_button` must run on every tab switch.** It runs only
   from `apply_state` today (`mod.rs:3815`), and hiding a control raises no
   focus notification (`mod.rs:1569-1578`), so Enter would press an
   off-screen button. `Add`, `Remove`, `Record` and `Reset` are all
   Shortcuts-page controls and all four are in `PUSH_BUTTONS`.
2. **`DefaultButton::visible` gains a `page` argument**
   (`beckon-core/src/settings.rs:1445`). Its doc sentence "the banner's
   visibility … is the window's only conditional geometry" becomes false and
   must be rewritten.
3. **The banner is a Shortcuts control.** `show(banner, external_change)` and
   its two buttons (`mod.rs:3769-3771`) become
   `external_change && page == Page::Shortcuts`. `external_change` stays a
   window-wide fact — the design's warn dot on the Shortcuts pill is how it
   stays visible from the other three pages.

   **INCOMPLETE, corrected 2026-08-14 after Task 4 landed.** That is the whole
   of what this item said, and it is only half the consequence. Making the
   *announcement* Shortcuts-only while `external_change` stays window-wide
   silently made Save — which is chrome, enabled from `apply_enabled` alone,
   the default ring's resting place and `Ctrl+S`'s target — able to overwrite
   an externally changed file from three pages with nothing on screen saying
   so. `apply_settings` (`serve.rs`) writes unconditionally and there is no
   prompt anywhere: **the banner being on screen WAS the whole protection.**
   The warn dot is a notice and does not close this, so the guard cannot wait
   for Task 6: `beckon_core::settings::save_press` refuses the press once and
   opens `BANNER_PAGE` instead, which puts `Reload from disk` and `Keep mine`
   under the user's hand; the next press writes. It is enforced in
   `apply_settings` rather than on the button because the close prompt's
   `SaveChoice::Save` reaches the same write without going near `IDC_APPLY`.
4. **`Ui` gains `shown_page`**, beside `shown_external` and `shown_empty`, and
   the `layout` guard at `mod.rs:3789-3796` gains that term. Without it a
   page switch through `apply_state` would leave the previous page's geometry
   on screen.
5. **`SettingsCommand::ShowPage(page)` is raised on every switch**, so the
   caller can store it and the next open lands where the user left off.
   Phase 0 already built the channel; `serve.rs`'s exhaustive `match` gains
   its first real arm.

### 4.4 Keyboard

`build_accelerators` (`mod.rs:2918-2927`) goes from `[ACCEL; 1]` to
`[ACCEL; 7]`: `Ctrl+Tab`, `Ctrl+Shift+Tab`, `Ctrl+1`, `Ctrl+2`, `Ctrl+3`,
`Ctrl+4`, plus the existing `Ctrl+S`.

`Ctrl+Tab` **must** be an accelerator rather than relying on the dialog
manager: `IsDialogMessageW`'s `VK_TAB` branch is not documented to consult the
Ctrl state, so the failure mode of forgetting the entry is "focus moves one
control" — which looks like nothing happened and is much harder to spot than a
dead key.

Within the strip, Left/Right move between pills and the auto-radio group
selects as it moves. Whether user32 migrates `WS_TABSTOP` onto the checked
radio (making "the strip is ONE tab stop" free rather than hand-maintained) is
**not settled by reading** — nothing in this tree exercises a radio group, the
three that existed were retired. See §9 G-S2.

---

## 5. `apply_state` and the badge

The Shortcuts pill carries a count badge, and a warn dot when the
external-change banner is up on a page the user is not looking at.

**Neither may ride in the pill's caption.** `layout` sizes buttons from
`text_size` of their caption (`layout.rs:432-434`), so a data-dependent
caption makes the caption a `layout` input — and `layout` on a data push is
the measured data-loss call. `cap::STOP` (`mod.rs:459-465`) is the same
decision already taken once.

**Nor may the painter read them from `UI`.** A paint reaches this window while
`UI` is already borrowed — measured on a14, where every subitem notification
exited at `try_borrow` and the Shortcut column silently drew as plain text.
The badge value and the warn flag live in a thread-local `Cell`, written by
`apply_state`, read by the painter. `CHIPS` (`mod.rs:1378-1383`), `CAP_FONT`
(`:4238-4243`) and `SHOWN_NOTES` (`:1704-1710`) are the three precedents.

**Which count.** Not `ControlState::items.len()`: `control_state` builds
`items` from `Model::visible()` (`settings.rs:1329-1356`), which is
filter-dependent *and* exempts the selected row unconditionally
(`settings.rs:544`). The badge is read from three pages that have no filter
box, so it must be the file's binding count — a new `ControlState` field.
`IDC_LBL_COUNT` keeps counting the visible rows; the two numbers are
different on purpose and the pill's is the stable one.

---

## 6. Painting

### 6.1 A sibling painter

`paint::tab_pill`, modelled on `paint::button` but not part of it. It needs
its own `NM_CUSTOMDRAW` arm in `WM_NOTIFY`, placed **before** the
`suppressed()` gate at `mod.rs:5244` and modelled on the `IDC_CAPS` arm at
`mod.rs:5241-5243` — custom draw is pure painting and must not be gated on
suppression. The existing dispatch at `mod.rs:5233` is gated on
`is_push_button(nm.idFrom)` and will not match a pill.

Selected-ness comes from `is_checked(hwnd, id)`, never from `CDIS_CHECKED` —
the identical decision is already documented and executed for `IDC_CAPS`
(`mod.rs:4391-4397`, `:4404`): a check box's `NMCUSTOMDRAW` has no state bit
for ticked.

Under high contrast a pill flattens to `Rectangle`, per six existing sites
(`paint.rs:299-309`, `:1188-1193`, `:1264-1271`, `:1426-1430`, `:1540-1547`,
`:1642-1647`) and the reason at `:300-302`: a soft edge under an HC theme
reads as a rendering artefact rather than as a control.

### 6.2 Two new tokens, and one correction to the design

| Token | Light | Dark | `sys` index |
|---|---|---|---|
| `strip` | `#D9DDE7` | `#2E323D` | `COLOR_BTNFACE` |
| `strip_hover` | **`#C2C9D8`** | `#3A3F4C` | `COLOR_BTNFACE` |

**`LIGHT.strip_hover` moves from the design's `#CBD1DE` to `#C2C9D8`.**
`#CBD1DE` measures **1.126** against `strip`, under the 1.2 border floor —
the hover state would be invisible. `#C2C9D8` measures 1.222. This reopens a
settled value only because the design's own rule is that the contrast floor
decides, and the design states that rule while giving a value that fails it.

The dark half already passes at 1.217, and the design's arithmetic for *why*
the dark trough must be lighter than the card is confirmed: no colour darker
than `DARK.bg` can clear 1.2, because pure black itself reaches only 1.171.
The same forecloses a near-white light trough — pure white against
`LIGHT.bg` is 1.101.

Rows to add to `pairs()`, all measured:

| Pair | Floor | Light | Dark |
|---|---|---|---|
| `text_muted` on `strip` | 4.5 | **4.522** | 5.237 |
| `text` on `strip_hover` | 4.5 | 10.700 | 8.664 |
| `strip` on `bg` | 1.2 | **1.235** | 1.400 |
| `strip_hover` on `strip` | 1.2 | **1.222** | **1.217** |
| `accent_fill` on `strip` | 1.2 | 3.802 | 2.826 |

**Four of those ten clear by under 0.04**, and the narrowest is the one this
paragraph originally left unbolded: `strip_hover` on `strip` in **DARK**, at
+0.017 against +0.022, +0.035 and +0.022 for the three light cells. Corrected
2026-08-14 during Task 1 — the first version bolded per row on the light
column, which pointed the reader at the wrong cell in a sentence whose entire
job is to flag fragility.

They are correct and they are fragile: any future move of `text_muted`, `bg`
or either strip token can break one, and the rows exist so that break is a
test failure rather than a screenshot.

`accent_on` on `accent_fill` (the active pill's ink on its fill) already has
its row (`beckon-core/src/theme.rs:225`, 5.169 / 4.531). **The active pill's
fill is `accent_fill`, never `accent`** — `accent_on` on `DARK.accent`
measures 3.044, and nothing in `pairs()` covers that combination, so the
failure would ship unseen.

### 6.3 High contrast

`Theme::HighContrast` returns `None` from `palette()` by construction, so no
literal can leak in; every surface resolves through
`ThemeCache::col(token, GetSysColor index)`, and **fill and ink must use
different indices** — five invisible-text collisions were found by hand on the
last redesign and by no compiler.

`COLOR_BTNFACE` for `strip` is chosen deliberately: `accent_fill` is
`COLOR_HIGHLIGHT` for the active pill, so the inactive family must differ;
`COLOR_WINDOW` collides with `card` at eight sites and would make the trough
read as a card; and a ninth, currently-unused index has no sibling site to
check against, which is how the five collisions happened. The consequence —
the trough becomes invisible under HC, with only the active pill's
`COLOR_HIGHLIGHT` distinguishing it — is accepted and must be checked by
screenshot (§9 G-S4).

---

## 7. 680 px

`WINDOW_WIDTH` 760 → 680. One constant, plus the probe: changing it without
editing `examples/settings_probe.rs:308` fails `geometry_matches_the_probe`
(`ids.rs:224-242`) on the Windows CI job. That test is Phase 0's, working as
intended.

**`MIN_WIDTH` does not move.** `layout.rs:366-368` states that as a rule, in
either direction, until G1 has run.

What 680 does to the widths that matter:

- Card interior is `680 − 2·PAD − 2·CARD_PAD = 638`, which is exactly `cw1` /
  `grp_w` / `kb_w` (`layout.rs:525`, `:636`, `:713`). The design's arithmetic
  is confirmed.
- The design's "~438 px for the app name" is **17 px optimistic**: `col_app`
  is 421, because `layout.rs:597-602` subtracts `SM_CXVSCROLL`
  unconditionally, and 404 when a scrollbar is actually up.
- On the Caps line every width except `tap_w` is content-derived, so
  760 → 680 changes `tap_w` and nothing else (`layout.rs:763-792`).
- `tok::SHORTCUT_COL` 200 is a ceiling in four places (`layout.rs:535`,
  `:605`, `:688`, `:792`); at 680 two stop binding.

---

## 8. What this landing ships

| Page | Contents |
|---|---|
| Shortcuts | the banner, the list, the editor strip — every control that exists today |
| Keyboard | the Caps row: `IDC_CAPS`, `IDC_LBL_HOLD`, the three Hold chips, `IDC_LBL_TAP`, `IDC_TAP` |
| System | one line: `Nothing here yet.` |
| About | one line: `Nothing here yet.` |

The two waiting lines are `STATIC`s with ids from their pages' reserved ranges
(`IDC_SYS_PLACEHOLDER`, `IDC_ABOUT_PLACEHOLDER` — allocated from 1084 and 1115
respectively, both inside Phase 0's reserved tails), and **both must be added
to the `on_card` match at `mod.rs:5363-5375`** or they fall through to
`COLOR_3DFACE` and draw as grey rectangles.

Not in this landing: auto-save, the service line, the `Saved` readout, Undo,
the transparency slider, dark-by-default, and every real System/About control.

---

## 9. Gates

All on a14, session 1, scheduled task with **both**
`-AllowStartIfOnBatteries -Priority 4`. Every gate needs a control — a blind
probe and a clean result look identical without one. Kill `beckon*` before
`cargo build`, or the link fails on the locked exe **and leaves the old binary
in place**.

| # | Gate | Control |
|---|---|---|
| G2 | Does `CDIS_HOT` reach a `BS_PUSHLIKE` auto-radio's `NM_CUSTOMDRAW`? The whole control choice rests on it | an ordinary `BS_PUSHBUTTON` in the same run, which is known to get it (`mod.rs:4356-4366`) |
| G3 | `GetClientRect` and `GetWindowRect` logged side by side | already built: `settings_probe.rs:781-862` prints both with a MATCH verdict. Reading says they are equal; this confirms it |
| G-S1 | Does a tab switch preserve text typed into the App combo? Type, switch, switch back | the same sequence with the skip disabled, which must lose the text. **There are TWO skips and the round trip needs both** — `if shortcuts` in `layout` on the way out, `combo_needs_placing` on the way back (§10 item 1) — so disable them one at a time: with only the second removed the text is lost on the way back, which is the half Task 4 shipped |
| G-S2 | Does user32 migrate `WS_TABSTOP` onto the checked radio? Read all four pills' styles back with `GetWindowLongW` | the same read before any pill is checked |
| G-S3 | Does `is_checked` report a `BS_AUTORADIOBUTTON` correctly? | `CheckRadioButton` a known pill, then read all four |
| G-S4 | The strip under each of the four shipped HC schemes | the same screenshot in ordinary dark, where the trough is known to be visible |
| G-S5 | `GetSystemMetricsForDpi(SM_CXSIZEFRAME / SM_CYSIZEFRAME / SM_CXPADDEDBORDER, dpi)` printed by name at 96 and 144, and the left/right resize edge dragged across the strip's band | dragging the same edge below the strip, where it is known to work |
| G1 | `GetTextExtentPoint32W` on the Caps line at 680, 96 and 144 DPI | the same measurement at 760, where it is known to fit |

**G1's scope has shrunk and the design has not caught up.** It measures
`"Use Caps Lock as a shortcut key"` against the *current* single-line layout,
but design §3.2 replaces that line with a toggle and moves the Hold chips to
their own row. Running G1 as written answers a question the redesign retired.
It still decides `MIN_WIDTH`, so it stays — but the Keyboard workstream must
re-scope it rather than inherit it.

---

## 10. Still open

1. Whether `SetWindowPos` with an **unchanged** rect still makes a populated
   combo re-sync its edit. If not, a `SWP_NOSIZE | SWP_NOMOVE` short-circuit
   in `place_h` would defuse most of §4.2 cheaply. The a14 measurement was a
   real resize; the no-op case is unmeasured. `examples/combo_probe.rs` would
   settle it.

   **No longer blocking, 2026-08-14.** §4.2's guard turned out to be
   one-directional — it keeps the combo out of reach on the way OUT of
   Shortcuts, and every switch back IN placed it again. Worse, that placement
   is a *genuine* resize every time rather than only when the geometry drifted:
   `layout` passes `field_h * 9` as `cy` (a combo's height argument sizes its
   dropped list, not its closed box), while `GetWindowRect` reports the closed
   height, so the request can never equal the current state and nothing
   upstream can elide it. The return trip is closed by
   `beckon_core::settings::combo_needs_placing`, which asks the control where
   it is and **does not make the call** when the answer is "already there" —
   deliberately phrased that way round so it is correct under either answer to
   this question. What the answer would still buy is the general case (a
   `place_h` on any combo, any pass); what it can no longer buy is this defect.
2. What `SetWindowPos` does to a **hidden** populated combo — relevant if the
   skip in §4.2 is ever weakened.
3. Whether the trough runs edge to edge or is inset by `PAD`. This spec says
   inset, matching the mockup and the cards; the resize-edge argument in §2.4
   is the reason it is not merely cosmetic.
4. `Ui::defid`'s resting value once auto-save deletes Save. A pill cannot
   answer `DLGC_DEFPUSHBUTTON`, so Enter on a focused pill goes to
   `DM_GETDEFID` — and "Enter on a tab presses a button that no longer
   exists" is worse than the Reload-saves defect it descends from. The
   auto-save workstream owns this; the strip is what makes it urgent.
5. Whether macOS draws any of this. It accepts and discards `page` today
   (`beckon-macos/src/settings_window.rs:626`); design §12 Q4 is still
   unanswered.
