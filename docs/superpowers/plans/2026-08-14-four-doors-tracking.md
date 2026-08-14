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
| Window narrows 760 → **680** | **open** | Task 8. Deliberately last, so every earlier task's arithmetic is checked at the width it was written for |
| `MIN_WIDTH` 660 unchanged | **done** | and must not move until G1 runs — `layout.rs` states that as a rule |
| `MIN_HEIGHT` 560 unchanged | **changed** | kept at 560, but its **four-row guarantee is withdrawn**: the strip costs 34 px and the floor now buys two rows. Design §4 makes the list scroll, so the floor's job changed. Recorded in `mod.rs` |
| Defaults to **dark** | **open** | System page (Task 7+) |
| Transparency slider 85-100 %, default 96 % | **open** | System page |
| Strip sits below the title bar, never inside the caption | **done** | and **verified free**: `chrome::nchittest` returns `HTCLIENT` below `TITLEBAR_H`, so no drag-zone arithmetic was needed |
| Tokens `TABSTRIP_H 36`, `TAB_PAD_X 14`, `TAB_PAD_Y 2`, `TAB_VISUAL 26`, `FOCUS_SLACK 3` | **done** | `layout.rs` `mod tok` |
| New palette tokens `strip` / `strip_hover` with `pairs()` rows | **changed** | done, but `LIGHT.strip_hover` moved `#CBD1DE` → `#C2C9D8`: the design's value measures **1.126** against the trough, under its own 1.2 floor |
| Shortcuts pill carries a count badge | **open** | Task 6 |
| Warn dot when the banner is up on another page | **open** | Task 6. **Until it lands the banner shows on every page** — see §6 below |
| `Ctrl+Tab` / `Ctrl+Shift+Tab` / `Ctrl+1..4` | **done** | Task 5, `build_accelerators`. The review that followed moved the focus repair's target: a door change now leaves focus on the pill it just opened, not on `Close`, so Enter after `Ctrl+2` no longer closes the window (`repair_hidden_button`'s `successor`) |
| The strip is ONE tab stop, Left/Right between pills | **done, free** | **measured with a control (G-S2), a14 2026-08-14**: `WS_TABSTOP` moves off A and onto B when B is checked, so user32 migrates it by itself. The first run of this gate could not say so — see the gates table |
| **No `&` mnemonics on tab names** | **done** | and settled by counting: `About` has no free letter left, so four unique mnemonics do not exist |

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

Both are **a single waiting line** today, by decision — see the shell spec §8.
Every id is allocated (1070-1099, 1100-1119) and tested for uniqueness, so the
pages can be filled without touching anything else.

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
  on three pages while Save stayed reachable. It now shows on **every** page
  while `external_change` is true. That is wider than the design asks;
  Task 6's warn dot is what narrows it back.

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
