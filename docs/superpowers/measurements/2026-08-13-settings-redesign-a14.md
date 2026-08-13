# Settings-window redesign: the a14 hardware gates — RUNBOOK, NOT RESULTS

**STATUS: NOT YET RUN.** Every "Result" field below is blank on purpose. This
document was written from macOS, which has no access to a14 and cannot drive
a Windows message loop, a tray icon, or a keyboard hook — so nothing in it is
a measurement. It is a script someone takes to the machine: what to run, what
the control is, and what a PASS or a FAIL would look like, so the person at
the keyboard is filling in blanks rather than improvising a check on the
spot.

Do not edit the "Result" lines to say anything other than what was actually
observed on a14. Do not delete the "NOT YET RUN" markers until there is a
real result to replace them with — a half-filled version of this file that
still has some markers is more useful than one that silently drops them.

Tasks 1–14 of this plan shipped code that compiles (`cargo check --target
aarch64-pc-windows-msvc --all-targets` and the matching clippy, both clean —
see Task 15's own report for the pasted output) but that has never run on a
Windows message loop. This file is the list of the ten-plus-four things no
compiler can see, each with a control, because a clean result from a blind
detector and a clean result from a working one look identical — that is the
trap `caps_probe` was built around, and every gate below repeats the pattern
on purpose.

## Before any gate: build and place the binary

```powershell
cargo build --release --target aarch64-pc-windows-msvc --all-targets
```

**`--all-targets`, not `--examples`.** `--examples` does not build `[[bin]]`
targets, and `beckon-serve.exe` is a `[[bin]]` — a build with `--examples`
alone silently leaves the running tray on a stale binary while `cargo`
reports success. This bit the plan itself once already (see
`docs/superpowers/plans/…` and the CLAUDE.md note under *Live Windows
tests*); it is called out again here because Task 15 is exactly the task
where forgetting it would go unnoticed.

## Operational constraints (apply to every gate below)

- **SSH lands in session 0.** Session 0 has no desktop and no keyboard, so
  every visual or interactive result taken over an SSH shell is a **confident
  false negative** — the window may be perfectly correct and invisible to
  that session regardless. Do not run any gate below over a bare SSH shell.
- **Go through a scheduled task in session 1** instead, registered with
  **both** of:
  ```powershell
  New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries -Priority 4
  ```
  Both flags, not one. `schtasks`'s defaults refuse to start on battery and
  leave the task `Queued` forever on a laptop; separately,
  `New-ScheduledTask*` defaults to **priority 7**, and a task left there on
  battery produces no diagnostic of any kind — it looks exactly like the
  thing under test hanging, which is unfalsifiable when the thing under test
  is a GUI nobody can see.
- **Quoting**: use `-EncodedCommand` for PowerShell passed over SSH, and a
  `.bat` file for anything that needs a redirect (`>`, `2>`) — quoting
  through `ssh` → PowerShell → `powershell.exe` is eaten otherwise.
- **A `signal: 9` / `SIGKILL` from a local cross-compile check is
  environmental, not a finding** — re-run it; the workspace's build-script
  cache converges within a few attempts (see `progress.md`'s pre-flight scan
  for the same behaviour measured on this host).

## Summary

| # | Gate | Result |
|---|---|---|
| 01 | Mica composites under GDI | NOT YET RUN |
| 02 | Custom title bar: no 1 px artefact, resize borders grabbable, cross-DPI drag | NOT YET RUN |
| 03 | ListView scrollbar goes dark | NOT YET RUN |
| 04 | `CBS_OWNERDRAWFIXED` keeps index reads and typeahead | NOT YET RUN |
| 05 | The tick centres in a 26 px row | NOT YET RUN |
| 06 | Chrome glyphs render at 10 px | NOT YET RUN |
| 07 | Live light/dark flip with no restart | NOT YET RUN |
| 08 | Every type role resolved (fail any role reporting plain `Segoe UI`) | NOT YET RUN |
| 09 | Eight rows, no partial ninth, no horizontal scrollbar | NOT YET RUN |
| 10 | Toggle still reports as a checkbox and responds to `Space` | NOT YET RUN |
| 11 | 15 px vertical slack at non-100 % DPI (Task 10 finding) | NOT YET RUN |
| 12 | Hover tint renders at all (`LVM_GETHOTITEM` may not populate) | NOT YET RUN |
| 13 | Flicker from the missing `WS_CLIPCHILDREN` | NOT YET RUN |
| 14 | Disabled chip edge vs. enabled keycap edge, light mode | NOT YET RUN |

---

## Gate 01 — Mica composites under GDI

**What to run.** Open the settings window (double-click the tray icon, or
post its `WM_LBUTTONDBLCLK` the way `settings_probe` does) on a build ≥ 22621
(`MICA_MIN_BUILD` in `beckon_core::theme`) with a system capable of Mica.
Screenshot it over a busy desktop background so translucency is visible
against detail, not a flat colour.

**The control.** Force `Backdrop::Alpha` instead by making
`read_backdrop_inputs` report `mica_supported: false` for one run (temporary
local edit, reverted after), or by running once on a build below
`MICA_MIN_BUILD` if one is reachable, and screenshot the same window the same
way. Tier 2 (`Alpha`, `TIER2_ALPHA = 245`) is a near-opaque single-alpha
layered window — visibly flatter than Mica's noise-textured blur. Without
this control, "the window has some translucency" and "the window has Mica"
are not distinguishable by eye alone; a slightly-transparent flat fill can
look like a blur at a glance.

**PASS** looks like: the tier-1 screenshot shows a blurred, noisy backdrop
that shifts with desktop content behind it (the DWM system backdrop type is
`DWMSBT_MAINWINDOW`, applied in `theme::apply_backdrop`); the control
screenshot (tier 2 / `DWMSBT_NONE` path) does not.

**FAIL** looks like: no visible difference between the two screenshots, or
the "Mica" screenshot showing the same flat single-alpha look as the control.
**If this gate fails**, per the plan: set `mica_supported: false` in
`read_backdrop_inputs` (this is the one-line demote the plan names), record
the failure here, and re-run this gate to confirm tier 2 renders correctly as
the new default — do not leave the window claiming Mica support it cannot
deliver.

**Result: NOT YET RUN.**

---

## Gate 02 — Custom title bar: 1 px artefact, resize borders, cross-DPI drag

**What to run.** With the settings window open: (a) zoom a screenshot of the
top edge to check for a 1 px seam between the client-painted bar and the
system frame; (b) drag-resize from each of the window's four edges and the
four corners in turn; (c) drag the window from a monitor at one DPI to a
monitor at a different DPI (if a14 has more than one display; if not, record
that this half of the gate has no hardware to run on rather than skipping it
silently).

**The control.** `chrome::hit_button` and `chrome::nchittest` both work in
CLIENT coordinates now (Task 7's fix for the SC_CLOSE-from-a-resize-drag
defect) — so the control is deliberately trying to break that fix: click
right at the seam between the close button and the resize corner, at more
than one DPI. A window that regressed to comparing screen coordinates against
client-derived rects would misfire exactly there, and only there; clicking
the middle of either button proves nothing about the regression this gate
exists to catch.

**Known, already-recorded minor to confirm rather than re-discover**: the top
resize strip uses `SM_CYSIZEFRAME` alone (`chrome::nchittest`), not
`SM_CYSIZEFRAME + SM_CXPADDEDBORDER` the way the left/right/bottom edges
effectively get from `DefWindowProcW` — so the top grab target is expected to
be about half as tall as the other three. Confirm this is merely "harder to
grab", not "cannot be grabbed at all".

**PASS** looks like: no visible seam; all eight resize handles (4 edges + 4
corners) initiate a resize, not a move or a click-through to a caption
button; a drag across a DPI boundary re-lays the window without clipped or
misplaced controls at the new DPI.

**FAIL** looks like: a visible 1 px line at the top; any edge/corner that
does not resize (in particular the top-right corner, which is where Task 7's
CRITICAL finding lived before the fix); a post-drag layout with fonts,
buttons, or the card grid still sized for the old DPI.

**Result: NOT YET RUN.**

---

## Gate 03 — ListView scrollbar goes dark

**What to run.** Populate the shortcuts list past eight rows (see gate 09 for
how) so the vertical scrollbar appears, in dark mode.

**The control.** Screenshot the same list in **light mode first** — a light
scrollbar next to a dark list would be conspicuous, but only if there is a
same-window light-mode screenshot to compare against; a single dark-mode
screenshot cannot rule out "this was always this colour regardless of
theme".

**Also watch for** the flicker noted under gate 13 while scrolling — Task 8's
review flagged that the parent now overdraws the whole client over unclipped
children with no `WS_CLIPCHILDREN`, and a scrollbar drag is one of the more
likely places to notice it first.

**PASS** looks like: the scrollbar track and thumb are dark in dark mode,
light in light mode, and switching between the two screenshots is the only
way to tell them apart (i.e. nothing else in the list changed).

**FAIL** looks like: a light-coloured scrollbar surviving into the dark-mode
screenshot (comctl32's default, unthemed).

**Result: NOT YET RUN.**

---

## Gate 04 — `CBS_OWNERDRAWFIXED` keeps index reads and typeahead

**What to run.** Two halves:

1. **Automated, already covered by the updated probe**: run
   `settings_probe.exe` against a running `beckon-serve.exe` (see the probe's
   own header comment for the invocation). It now reads `IDC_COMBO`'s and
   `IDC_TAP`'s style bits and asserts `CBS_DROPDOWNLIST | CBS_OWNERDRAWFIXED`
   with `CBS_SORT` absent on both, plus `CB_GETCOUNT` and the fixed-index
   items (`KEY_ORDER` for `IDC_COMBO`, the three `TAP_ITEMS` for `IDC_TAP`).
   This confirms the STYLE is right and the LIST CONTENT is right, but not
   that comctl32's live typeahead behaves.
2. **Manual, needs a keyboard**: with the key list (`IDC_COMBO`) focused and
   closed (not dropped down), type `f1` — two keystrokes, `f` then `1` — and
   read `CB_GETCURSEL` (`settings_probe`'s `key_sel` helper does this, or
   read it by hand with a UI inspector). Compare against what a
   pre-`CBS_OWNERDRAWFIXED` build (i.e. before Task 9) would have selected
   for the same keystrokes, if that build is still available; if not, the
   control is comparing "index of the item whose text starts with `f1`" —
   `key_table()`'s `f1` entry — against whatever index the keystrokes
   actually land on.

**The control.** `CBS_OWNERDRAWFIXED` changes how the control PAINTS a row;
it does not document itself as changing whether comctl32's built-in
first-letter search still runs, but this codebase has already been burned
once by an unverified assumption about a populated combo box's live behaviour
(the `combo_probe` incident in `CLAUDE.md`'s *Live Windows tests* section:
typing "Notepad" silently wrote "d" to the model because a populated
`CBS_DROPDOWN` resynchronises on `SetWindowPos`). The rule that measurement
established — verify comctl32's live behaviour on real hardware with real
keystrokes rather than trust documentation — applies here too, on a
different control and a different style flag.

**PASS** looks like: typing `f1` selects the `f1` item (`CB_GETCURSEL`
reports the index `KEY_ORDER` in the probe says is `36`), and the automated
probe's style-bit and item-order assertions both print no `<<< FAIL` line.

**FAIL** looks like: typeahead landing on the wrong item, not moving at all,
or the probe's own style/order assertions failing (which would mean the
window shipped with `CBS_SORT` present or `CBS_OWNERDRAWFIXED` absent,
silently breaking the index contract `ComboView::key` depends on).

**Result: NOT YET RUN.**

---

## Gate 05 — The tick centres in a 26 px row

**What to run.** Screenshot the shortcuts list with at least one row ticked
(`LVS_EX_CHECKBOXES`, column 0's state image), at 100 % DPI and again at
150 %.

**The control.** Two already-recorded reasons this specific gate cannot be
taken on faith from the arithmetic in the code:

- **Task 10's finding**: `MIN_HEIGHT` was raised from 680 to 702 because the
  26 px row height ate into the vertical slack the window's own comment
  promised (four rows visible with the banner up). `WINDOW_HEIGHT` (740)
  still leaves 15 px of slack, but that 15 px rests on **96-DPI fallback**
  values for `list_row_height` / `list_header_height` — this host cannot
  measure the *live* control, only reason about the fallback formula. **Run
  this gate at a non-100 % DPI specifically**, where `notes_height`'s live
  font measurement is the term nothing on this host could trace.
- **Task 5's finding**: the disabled chip's edge colour
  (`text_faint #6F7785`) is *darker* than an enabled keycap's edge
  (`keycap_edge #B6BFCF`) in light mode — the opposite of what "disabled
  controls read as fainter" would suggest. This needs eyes on hardware; see
  gate 14, which is the same visual region and is worth checking in the same
  screenshot pass.

**PASS** looks like: the tick glyph sits visually centred in its 26 px row at
both DPIs, with no visible clipping top or bottom, and the list still shows
whatever row count the current window height computes without a partial row
or a horizontal scrollbar (cross-check against gate 09).

**FAIL** looks like: the tick riding high or low in the row (more likely at
the non-100 % DPI, per the slack note above), or clipped by the row bounds.

**Result: NOT YET RUN.**

---

## Gate 06 — Chrome glyphs render at 10 px

**What to run.** Screenshot the title bar's minimize and close glyphs,
zoomed. `Role::Chrome` resolves to `"Segoe Fluent Icons"` at 10 px, regular
weight (`build_fonts` in `settings_window/mod.rs`).

**The control.** Request a face that does not exist — e.g. temporarily change
the requested name to something like `"Segoe Fluent Icons ZZZ"` and rebuild —
and confirm the fallback glyph is **visibly different** (almost certainly
Arial's box/question-mark glyph, or a blank box, rather than the actual
minimize/close icon). Without this control, "the glyphs render" and "GDI
silently substituted a fallback font that happens to have *something* at
those code points" are not distinguishable from a screenshot of the real
build alone — `CreateFontIndirectW` never fails on an unresolvable name, it
silently hands back a substitute (documented in this same file's comment
above `make_font`, and independently confirmed by the a14 measurement at
`mod.rs:2016-2017` for the *unrelated* default-font case).

**PASS** looks like: the real build shows crisp minimize/close glyphs at a
size that reads correctly at 10 px; the deliberately-broken control build
shows a visibly different (wrong) glyph in the same position.

**FAIL** looks like: no visible difference between the real and the
deliberately-broken build — meaning the "real" font request was already
silently falling back and this gate would never have caught it without the
control.

**Result: NOT YET RUN.**

---

## Gate 07 — Live light/dark flip with no restart

**What to run.** With the settings window open, flip the OS setting
(*Settings → Personalization → Colors → Choose your mode*) between Light and
Dark **without closing or restarting anything**, and watch the window
repaint.

**The control.** Toggle *twice* (light→dark→light), not once — Task 13's
review found and fixed a real bug in this exact area: `on_theme_changed`
used to return early on `!changed`, computed purely from `Theme`, so
toggling *Transparency effects* (a related but distinct setting that
broadcasts `WM_SETTINGCHANGE("ImmersiveColorSet")` without changing
`Theme`) left the window stuck at its old backdrop tier. A single flip could
pass by luck even with that class of bug present; a there-and-back sequence,
and a separate toggle of *Transparency effects* alone, are what actually
exercises the guard.

**PASS** looks like: every themed surface (title bar, cards, list, chips,
toggle, notes) repaints to the new theme within a perceptible instant, with
no leftover light-mode (or dark-mode) fragment anywhere, and the backdrop
tier (Mica / Alpha) survives the flip rather than reverting to opaque.

**FAIL** looks like: any control stuck on the old theme, a full repaint only
after minimizing/restoring or resizing (meaning the live `WM_THEMECHANGED` /
`WM_SETTINGCHANGE` path is not actually doing the work), or the *Transparency
effects* toggle alone failing to update the backdrop tier.

**Result: NOT YET RUN.**

---

## Gate 08 — Every type role resolved

**What to run.** For each of the seven `Role` variants (`Title`, `Subtitle`,
`BodyStrong`, `Body`, `Caption`, `Keycap`, `Chrome`), pick one control that
uses it (see `role_of` in `settings_window/mod.rs` for the id→role mapping)
and read its font: `WM_GETFONT` to get the `HFONT`, then `GetObjectW` into a
`LOGFONTW` to read `lfFaceName`, `lfHeight`, and `lfWeight`.

**The control.** `build_fonts`' own doc comment names the exact failure mode:
`CreateFontIndirectW` never fails on an unresolvable name — it silently hands
back a substitute, and the substitute for a bad request in this codebase has
already been measured once as plain `Segoe UI`, weight 400, `lfHeight = -12`
(a14, 2026-08-11, for the *unrelated* default/message font, cited in
`mod.rs:2016-2017`). Any `Role` reporting that same signature — plain `Segoe
UI`, not one of the `Segoe UI Variable …` family names — has silently fallen
back, which is exactly what this gate exists to catch and exactly why it is
named as a hard failure condition in the task brief rather than a soft one.
The seven wanted face names, sizes (in points, at the DPI captured), and
weights are in `build_fonts`:

| Role | Face | Size (pt) | Weight |
|---|---|---|---|
| `Title` | `Segoe UI Variable Display Semib` (truncated at 32 wchar) | 15 | Semibold |
| `Subtitle` | `Segoe UI Variable Text Semibold` | 18 | Semibold |
| `BodyStrong` | `Segoe UI Variable Text Semibold` | 14 | Semibold |
| `Body` | `Segoe UI Variable Text` | 14 | Regular |
| `Caption` | `Segoe UI Variable Small` | 12 | Regular |
| `Keycap` | `Segoe UI Variable Small Semibol` (truncated) | 11 | Semibold |
| `Chrome` | `Segoe Fluent Icons` | 10 | Regular |

Note the two names are deliberately spelled **as truncated at the 32-wchar
`lfFaceName` limit** (`build_fonts`'s own doc comment: "regularising" either
name to match its un-truncated sibling hands `make_font` a name GDI cannot
resolve) — so `Title` and `Keycap` are the two roles most likely to
demonstrate this exact failure mode if a future edit "fixes" the spelling.

**PASS** looks like: all seven roles read back one of the seven rows above,
not plain `Segoe UI`.

**FAIL** looks like: any role reporting plain `Segoe UI` at weight 400 —
per the task brief, this fails the gate regardless of how many of the other
six pass.

**Result: NOT YET RUN.**

---

## Gate 09 — Eight rows, no partial ninth, no horizontal scrollbar

**What to run.** Point `beckon-serve` at (or edit in place through the
window itself) a config with roughly 20 shortcuts, open the settings window,
and screenshot the list.

**The control.** Resize the window down toward `MIN_WIDTH` (753) /
`MIN_HEIGHT` (702) with the same 20-row config loaded, and screenshot again.
`tok::ROWS` is documented as a fixed eight rows at every DPI (measured, not
scaled from a token) — the control is what tells apart "eight rows because
the token says eight" from "eight rows because that's what fit at this one
window size and DPI by coincidence".

**PASS** looks like: exactly eight full rows visible at the default 900×740
size, no sliver of a ninth row, no horizontal scrollbar at either window
size tested (the 45/55 fixed-column split this design replaced is what used
to produce one, per the plan's own history); the list scrolls the other
twelve rows rather than clipping them without a scrollbar.

**FAIL** looks like: a partial ninth row, a horizontal scrollbar at either
size, or a row count that changes between the two window sizes tested
(meaning `ROWS` is not actually fixed against the geometry it claims to be
fixed against).

**Result: NOT YET RUN.**

---

## Gate 10 — Toggle still reports as a checkbox and responds to `Space`

**What to run.** Give `IDC_CAPS` keyboard focus (Tab to it, or click it), then
send `Space` via `SendInput` (not `WM_KEYDOWN` posted directly — `SendInput`
is what proves the real input path works, the same reasoning `caps_probe`
used for the Caps-tap gesture) and read `BM_GETCHECK` before and after.

**The control.** `settings_probe.rs` now asserts `IDC_CAPS`'s style bits
report `BS_AUTOCHECKBOX` (`GWL_STYLE & 0x0F == 0x03`) rather than
`BS_OWNERDRAW` — run the probe first and confirm that assertion prints no
`<<< FAIL` line. That is the STATIC half of this gate (the control is still
built the right way); driving it with `SendInput` and reading `BM_GETCHECK`
is the DYNAMIC half (the control still behaves the right way at runtime,
which no style-bit read alone can prove) — a `BS_OWNERDRAW` control would
still visually toggle under `NM_CUSTOMDRAW`, so a screenshot-only check
cannot tell the two apart; `BM_GETCHECK` can.

**PASS** looks like: the probe's style assertion is clean, and
`BM_GETCHECK` flips between `BST_UNCHECKED` and `BST_CHECKED` in step with
each `Space` press.

**FAIL** looks like: the probe's style assertion failing (`BS_OWNERDRAW`
present instead), or `BM_GETCHECK` staying constant / not answering
meaningfully across the `Space` presses.

**Result: NOT YET RUN.**

---

## Gate 11 — 15 px vertical slack at non-100 % DPI

Folded in from Task 10's review rather than the original ten-gate list; see
`progress.md`'s Task 10 entry for the exact finding. Overlaps gate 05's
"screenshot at 100 % and 150 %" instruction — recorded as its own line here
because it is a distinct claim (window-level vertical fit, not tick
centring) and the summary table should be able to say PASS/FAIL on it
independently of gate 05's tick-centring verdict.

**What to run.** At a non-100 % display scale (150 % is what the rest of this
plan's measurements use on a14; any non-100 % value exercises the same
untraced term), open the settings window with the banner showing (an
external-change condition — see `IDC_BANNER` / the banner-visible layout
path) and confirm all four cards, the eight-row list, and the keyboard
section still fit inside `MIN_HEIGHT` without clipping or a vertical
scrollbar on the window itself (as opposed to the list's own scrollbar,
which is expected).

**The control.** The 15 px figure is derived from **96-DPI fallback**
values for `list_row_height` / `list_header_height` because this host
cannot measure the live comctl32 control. `notes_height`'s live font
measurement is named explicitly as the untraceable term — so the control is
comparing the SAME layout at 100 % DPI (where the fallback and the live
value should coincide) against the non-100 % run: if the 100 % run has
visibly more slack than the non-100 % run, the fallback assumption was
optimistic and the 15 px margin is the thing to re-check against a real
build.

**PASS** looks like: no clipping, no unexpected scrollbar, at the non-100 %
scale with the banner showing.

**FAIL** looks like: any card or the keyboard section clipped, or a vertical
scrollbar appearing on the window itself at a DPI where the 100 % run showed
none.

**Result: NOT YET RUN.**

---

## Gate 12 — Hover tint renders at all

Folded in from Task 10's review (`progress.md`). `LVS_EX_TRACKSELECT` was
deliberately NOT added because it would change selection behaviour, and
without it `LVM_GETHOTITEM` may never populate — meaning the list's hover
tint may simply never render, as a known and accepted degradation rather
than a defect to fix.

**What to run.** Move the mouse over list rows (not clicking) and watch for
any hover-state visual change distinct from the selection highlight.

**The control.** Compare against a row that IS selected (click it first) —
the control is knowing what the SELECTED state looks like so a "nothing
happened" hover result cannot be confused with "the mouse landed on the
already-selected row". Also read `LVM_GETHOTITEM` directly (it is a bare
`SendMessageW` call, no remote buffer needed) while hovering, to check
whether the item index is populating at all versus populating-but-undrawn.

**PASS** looks like either: a visible hover tint distinct from selection
(better than the accepted baseline), OR confirmation that `LVM_GETHOTITEM`
simply never populates and no tint renders — both are acceptable outcomes
per the design note; what would NOT be acceptable is `LVM_GETHOTITEM`
populating while nothing visibly changes, which would mean a real, silent
paint defect rather than the documented no-op.

**FAIL** looks like: `LVM_GETHOTITEM` returning a valid item index with no
corresponding visual change.

**Result: NOT YET RUN.**

---

## Gate 13 — Flicker from the missing `WS_CLIPCHILDREN`

Folded in from Task 8's review (`progress.md`): the window class carries no
`CS_HREDRAW`/`CS_VREDRAW` and no `WS_CLIPCHILDREN`, so the parent now
overdraws the whole client over unclipped children on every repaint — a
degree change from before Task 8 (when the client had no size-dependent
painting) rather than a new class of bug, but the review flagged the
flicker as likely to be far more noticeable now that the whole client is a
painted layer with a bottom-anchored card.

**CORRECTED 2026-08-13, whole-branch review.** `WS_CLIPCHILDREN` stays off
— out of scope for the fix wave, see the review's followups — and with it
off, the mechanism above GUARANTEES visible repaint artefacts: the parent
now paints four `RoundRect` fills over roughly twenty unclipped children on
every full invalidate. **Flicker is therefore an EXPECTED result of a
decision already made, not an open question.** A "does it occur" gate
would either fail this shipped decision every run or get rubber-stamped
into meaninglessness; the useful question left is severity, so this gate
now asks that instead.

**What to run.** Resize the window (see gate 02), scroll the list (see gate
03), and toggle the banner (trigger an external-change condition while the
window is open) while watching HOW BAD the flicker/tearing is, not whether
it appears at all.

**The control.** Compare the SAME interactions on an unrelated, ordinary
Win32 window (e.g. Notepad) resized the same way, side by side or in quick
succession — the control is having a reference for "this is what a
CLIPPED window's repaint looks like on this exact hardware and refresh
rate" so the severity judgment below has a calibrated low end, not so the
settings window can be marked FAIL for being visibly different from it.

**Record**, rather than PASS/FAIL: a severity call — mild (a barely visible
flash, comparable to a busy dialog with several children), moderate (an
obvious flash of the old card position before the new one draws, but the
window is still usable), or bad (visible tearing, a child control
independently lagging its parent, or anything that reads as a rendering
fault rather than "an unclipped repaint"). Bad is the threshold at which
`WS_CLIPCHILDREN` should be revisited rather than left as a documented,
accepted trade-off.

**Result: NOT YET RUN.**

---

## Gate 14 — Disabled chip edge vs. enabled keycap edge, light mode

Folded in from Task 5's review (`progress.md`): the disabled chip's edge
token (`text_faint`, `#6F7785`) is darker than an enabled keycap's edge
token (`keycap_edge`, `#B6BFCF`) in light mode — the opposite of the usual
"disabled reads as fainter" expectation, and specifically flagged as needing
eyes on hardware rather than being resolved by contrast arithmetic alone
(both tokens pass their contrast-floor tests independently; the question is
the relative reading, which a floor check cannot capture).

**What to run.** In light mode, screenshot a row with no key selected
(disabled editor chips) next to a row with a key selected (enabled,
un-armed chips) — same screenshot pass as gate 05 is a reasonable place to
capture this.

**The control.** Compare the SAME two chip states in dark mode, where the
review found no such inversion — the dark-mode screenshot is the reference
for "this token relationship reads as intended" so the light-mode result can
be judged against a known-good case from the same build rather than in
isolation.

**PASS** looks like: whatever the light-mode edges look like, a human
reviewer confirms it does not read as "disabled controls have a STRONGER
outline than operable ones" in a way that is confusing at a glance. This is
inherently a judgement call, not a bit to compare — record the reviewer's
call explicitly rather than a bare PASS/FAIL.

**FAIL** looks like: a reviewer confirming the disabled chip visibly reads as
MORE prominent / more clearly outlined than an adjacent enabled keycap, to
the point of being misleading about which controls are operable.

**Result: NOT YET RUN.**

---

## Followups from the 2026-08-13 whole-branch review, out of this wave's scope

The review that produced the Must-Fix/Should-Fix wave landed on this file
(see `final-fix-report.md` in the same session's `.superpowers/sdd/`
directory for the full list) also raised three things it explicitly scoped
OUT of that wave. Recorded here rather than fixed, so they are not lost:

1. **Dropping `BS_GROUPBOX` for the two card captions lost
   `ROLE_SYSTEM_GROUPING` / UIA `ControlType.Group`.** The fix itself was
   right — two visual frames around one logical control set reads as two
   groups, not one — but this branch is otherwise careful about UIA roles:
   `IDC_CAPS` kept `BS_AUTOCHECKBOX` explicitly for its checkbox role (see
   `paint::toggle`'s own doc comment) and `settings_probe.rs` pins that
   style bit on hardware for exactly that reason. Silently dropping two
   grouping containers is inconsistent with that standard elsewhere in the
   same branch. No replacement UIA grouping semantics have been proposed;
   this needs a design decision, not a mechanical fix.

2. **`site/favicon.png` was not regenerated** and is now inconsistent with
   `assets/beckon.ico` (redrawn full-bleed with a 24 px frame, per this
   session's own commit `eabfe12`). `tools/check-site.sh` does not check
   icons, so nothing in CI will catch this drifting further.

3. **The horizontal budget was never re-derived.** `MIN_WIDTH` moved
   720→753 "proportionally" in Task 8 without simulation; Task 11 then grew
   the keyboard line by 26 px at 96 DPI on top of that. The review checked
   it by hand: at 753 the line consumes ≈547 px of a 705 px card interior,
   leaving `IDC_TAP` ≈150 px against its 200 px ceiling. **It fits, but by
   luck rather than by anyone's arithmetic**, and `"Use Caps Lock as a
   shortcut key"` is the widest measured string in the window. A sentence
   naming the keyboard line as the width-critical one has been added to
   `layout`'s own doc comment (`settings_window/layout.rs`) so the next
   person who moves `MIN_WIDTH` knows which line to re-check by hand; the
   re-derivation itself (simulating the line at a range of widths/DPIs, or
   deciding a real floor for it) is still open.

None of the three change behaviour and none are blocking; they are
recorded so a future session does not have to re-discover them from
scratch.
