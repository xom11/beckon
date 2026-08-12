# beckon-serve settings window — keycaps, and the custom-draw pass B.5 deferred

Date: 2026-08-12
Follows landing 2b (v0.8.0) and
`docs/superpowers/2026-08-12-landing-2b-followups.md`.

Three directions were drawn and compared; this is the one chosen. The other
two are recorded in *Rejected directions* below rather than lost, because
each fixes something this one does not.

---

## Motivation

The v0.8.0 verdict was *"dùng cũng tạm rồi nhưng UI chưa thiết kế đẹp lắm"* —
every piece landed as a correct control in a correct place, and none of it
was composed as a whole. §1 of the followups lists five specific things. This
spec addresses four of them, and adds a fifth that §1 does not list:

**The most-read column in the window speaks the config file's language.**
The Shortcut column renders `ctrl+super+alt+t`. The user pressed three
physical keys; `super` is a word that appears on no keyboard, on no key cap,
and in no other Windows UI. It is correct as a TOML token and wrong as a
label, and it is the first thing the eye lands on eighteen times per open.

**The code has already booked this work three times.** All three notes point
at one deferred pass, and none of them can land alone:

| Where | What it says |
|---|---|
| `settings_window.rs:1401` (`mark_glyph`) | Fluent glyphs are ASCII for now because "spec B.5 defers those glyphs to the `NM_CUSTOMDRAW` pass that can give them their own font" |
| `settings_window.rs:1423` (same) | Exact glyph alignment "needs a glyph column drawn at a fixed x, which is the `NM_CUSTOMDRAW` work B.5 defers" |
| `settings_window.rs:3310` (`app_cell`) | The flag cannot take the Caption face because "a ListView draws a cell in the control's one font… Giving the flag its own would mean `NM_CUSTOMDRAW`, which B.5 explicitly defers to a later pass. **So this is a deferral, not an oversight: it lands with the Fluent glyphs or not at all**" |
| `2026-08-11-settings-window-redesign.md` §B.5 | "Fluent glyphs… are added later via `NM_CUSTOMDRAW` as decoration over text that already works" |

This spec is that pass. It also **amends** the `app_cell` sentence in bold
above — see §7.1 — because the measurement that sentence assumed nobody would
have to take turns out to be the one thing standing between the cheap half of
this work and the expensive half.

**Nothing here is a toolkit change.** Raw Win32, comctl32 v6, light-only with
`GetSysColor` throughout, high-contrast as the supported dark path — all
unchanged, and §C.2 makes the last one *more* true than it is today.

**Nothing here is a config change.** No new keys. No format change. The
`super` → `Win` substitution is display-only and §B.4 makes writing it to
disk structurally impossible.

---

## Decisions taken (and the alternatives rejected)

| Question | Decision | Rejected |
|---|---|---|
| The chord's rendering | Keycaps, drawn — `Ctrl` `Win` `Alt` `T` | Leaving the TOML string; a proportional `Ctrl+Win+Alt+T` string with no shapes (§7.2) |
| Where the shape rules live | One painter, called from both `WM_DRAWITEM` and `NM_CUSTOMDRAW` | A painter per call site |
| Chord colours | `GetSysColor` only — `COLOR_HIGHLIGHT` for an armed chip | The brand blue `#2563eb` the mockup used (§C.2) |
| Which subitem gets custom draw | Subitem 1 (Shortcut) this landing; subitem 0 gated on a measurement | Both at once (§B.6); a third column for the flag |
| What the cell still contains | The display string, as real `LVITEM` text under the caps | Empty text with the caps as the only content (§B.5) |
| Where `super` → `Win` lives | `beckon-core`, a display function separate from `Display for Combo` | A substitution at the Win32 call site |
| The four modifier chips | `BS_OWNERDRAW`, state tracked by the window | `BS_AUTOCHECKBOX` with a themed background — impossible, the styles are exclusive (§C.1) |
| The key list (`IDC_COMBO`, 1002) | **Untouched.** Stays a plain `CBS_DROPDOWNLIST` | `CBS_OWNERDRAWFIXED` so the current key draws as a cap (§7.3) |
| The notes strip's Fluent glyphs | Still deferred, and now explicitly out of scope (§7.1) | Bundling them in because `app_cell` said "or not at all" |
| Editor layout | Two lines in a titled group, App on its own line | Keeping one line and shrinking; a two-pane window (*Rejected directions*) |
| Empty state | Hide the group's controls, show one line | Keep greying them (§A.2) |

---

## Part A — the recompose

Part A draws nothing. It is pure `layout`, and it is what gives Part B a
place to sit. It is also worth landing on its own if Part B never happens.

### A.1 Band 4 becomes a titled group of two lines

```
┌ Editing "Windows Terminal" ─────────────────────────────────────────┐
│  App       [ Windows Terminal                                   v ] │
│  Shortcut  [Ctrl] [Win] [Alt] [Shift]  [ t    v ]   [Record] [Reset] │
│  ✓ Registered. Press Ctrl+Win+Alt+T to focus Windows Terminal.       │
└──────────────────────────────────────────────────────────────────────┘
```

One `BS_GROUPBOX`, new id `IDC_GRP_EDITOR` (1034 — above the 1001–1007 range
`examples/settings_probe.rs` pins, so the probe is unaffected).

The group's caption is written with `SetWindowTextW` on selection change.
That is a text write, not a geometry one: it does **not** call `layout`, and
therefore does not reach `SetWindowPos` on the populated App combo — the
measured data-loss call guarded by `Ui::shown_external`. A group box caption
is never measured by `layout` (it takes whatever the group's width is), so
there is no second path back in.

Three captions, and they are the whole vocabulary:

| State | Caption |
|---|---|
| A row is selected | `Editing "<app>"` — or `Editing this shortcut` when the app field is empty |
| `Add` was just pressed | `New shortcut` |
| Nothing selected | `No shortcut selected` |

### A.2 The empty state replaces the grey-out

Today, with no row selected, the whole strip greys — and a disabled
`CBS_DROPDOWNLIST` keeps a white field with dark text (measurements §56), so
it looks live beside greyed labels. That is the theme's behaviour and not a
bug, which is exactly why it cannot be fixed by disabling *better*.

Instead: `ShowWindow(SW_HIDE)` on the group's children, and one
`SS_CENTER | SS_CENTERIMAGE` STATIC in their place reading

> Select a shortcut above, or press **Add** to make a new one.

The group keeps its height in both states, so nothing below it moves.

**This needs a measurement before it can be trusted** — see *Gates*, G2.
`Ui::shown_external` records that `SetWindowPos` on a populated `CBS_DROPDOWN`
makes it re-synchronise its edit to the nearest matching item and select the
whole string. Whether `ShowWindow` does the same is **not known**; nobody has
run it. If it does, the App combo hides and re-shows carrying a catalogue
entry instead of what the user typed, which is the §7.15 defect wearing a new
message. The fallback if G2 comes back positive is in G2 itself.

### A.3 What A deletes

`tok::KEY_COL` and `tok::BTN_SM` exist for one reason, written out in their
own doc comments: band 4 is the densest line in the window and the App combo
absorbs whatever the other six controls leave, so both floors were derived by
arithmetic to stop it reaching zero inside `MIN_WIDTH`.

With App on its own line that pressure is gone. The App combo becomes
`cw - lw_app - lblgap`, which at `MIN_WIDTH` is roughly 600 px instead of the
~59 px §1 records.

- `tok::KEY_COL` (140) → delete; the key list returns to `tok::SHORTCUT_COL`,
  which is where it was before the arithmetic pushed it down.
- `tok::BTN_SM` (64) → delete; `Record` and `Reset` take `tok::BTN` (88) like
  every other button, so the window stops having two button sizes.

Both deletions are conditional on the second line still fitting at
`MIN_WIDTH`; §C.1 changes the chips' widths, so the arithmetic is re-derived
once, at the end of Part C, and not before.

### A.4 The notes cap

The measured state is a 1220×177 control holding one 258 px line. The notes
STATIC becomes a fixed two lines. Beyond two, the second line ends with
` (+N more)` and the whole set goes into the existing tooltip, which already
holds `tok::TOOLTIP_MAX` and already wraps.

Nobody has looked at what three notes at once reads like — §1 says so, and
the two-line cap is a guess made in the absence of that. It is a cheap guess
to revisit and an expensive band to leave empty.

### A.5 Height

The group adds one line plus its own inset over today's strip. At 96 DPI the
stack comes to roughly `PAD 16 + head 32 + gap 8 + list 240 + BAND 14 + group
152 + BAND 14 + keyboard 64 + BAND 14 + bar 32 + PAD 16` ≈ 602, inside
`WINDOW_HEIGHT` 640 with ~38 px of slack.

`MIN_HEIGHT` (460) must be re-derived, not nudged: the list already yields
its fixed height to `editor_min` in `layout`, and `editor_min` grows by a
line here.

---

## Part B — the custom-draw pass

### B.1 Scope

`NM_CUSTOMDRAW` on `IDC_LIST`, **subitem 1 only**. Subitem 0 is gated (§B.6).

The message arrives at the existing `WM_NOTIFY` arm
(`settings_window.rs:3954`), which already dispatches on
`nm.idFrom == IDC_LIST`. This is a new `nm.code` beside `LVN_ITEMCHANGED`,
not a new funnel.

The list already carries `LVS_EX_DOUBLEBUFFER` (`settings_window.rs:1964`),
so the flicker custom draw usually costs is already paid for.

### B.2 The draw

```
CDDS_PREPAINT                        → CDRF_NOTIFYITEMDRAW
CDDS_ITEMPREPAINT                    → CDRF_NOTIFYSUBITEMDRAW
CDDS_ITEMPREPAINT | CDDS_SUBITEM
    iSubItem == 1                    → paint, return CDRF_SKIPDEFAULT
    otherwise                        → CDRF_DODEFAULT
```

`CDRF_SKIPDEFAULT` means we own the cell's **background too**, not just its
text. The row background comes from the item state in `nmcd.uItemState`:
`CDIS_SELECTED` → `COLOR_HIGHLIGHT`, otherwise `COLOR_WINDOW`. Getting this
wrong shows up as a selected row with one un-highlighted cell, which is worse
than no keycaps at all.

One painter, `draw_cap(hdc, rect, text, on: bool, hc: bool)`, shared with
Part C so the chips in the editor and the caps in the list cannot drift:

- `RoundRect` at radius `s(4)`, height `min(row_h - s(6), s(19))`, centred
  vertically in the cell rect.
- Border `COLOR_BTNSHADOW`, fill `COLOR_BTNFACE` for a modifier, `COLOR_WINDOW`
  for the main key — the main key reads as the one you actually press.
- Text in the **Caption** role from `Fonts`, centred, with `s(5)` of
  horizontal padding either side.
- `s(3)` between caps.
- A 1 px bottom edge in `COLOR_BTNSHADOW` gives the cap its physicality. This
  is the only purely decorative line in the whole spec, and §C.2 turns it off
  in high contrast.

### B.3 The text fallback is structural, not a nicety

Measure the caps before drawing them. If the set is wider than the cell,
**draw the display string with `DT_END_ELLIPSIS` instead** and draw no caps at
all.

This is not defensive coding for a case that cannot happen. `SHORTCUT_COL` is
200 px but `layout` caps it at `inner / 2`, and `inner` is the list's own
client width minus a scroll bar. A narrow window, a high DPI, and a
five-modifier chord (`ctrl+super+alt+shift+t`) are each reachable
independently. Clipping a keycap looks like a rendering fault; an ellipsis
looks like a narrow column.

### B.4 `super` → `Win` lives in core, and cannot reach the file

New in `beckon-core::shortcuts`:

```rust
/// The chord as the user's keyboard spells it, for display only.
/// `ctrl+super+alt+t` → ["Ctrl", "Win", "Alt", "T"].
pub fn combo_caps(s: &str) -> Vec<String>

/// The same, joined for a screen reader and for the ellipsis fallback.
/// → "Ctrl + Win + Alt + T"
pub fn combo_display(s: &str) -> String
```

Both go through the existing `combo_view` / `key_table` pair, so an
unparseable string yields an empty vector and the cell falls back to the raw
text — the same "show it rather than guess" rule `ComboView::key = None`
already follows.

**Neither may ever be reachable from serialisation.** `Display for Combo` is
what writes the file, and if these two ever merge with it, beckon writes
`Win` into a TOML it then cannot parse — a config the user did not break and
cannot obviously fix. Pinned by a unit test that round-trips a config through
save and asserts the on-disk bytes still say `super`. That test belongs in
`beckon-core` so all three CI jobs run it, not just the Windows one.

### B.5 The cell keeps real text

`cells()` (`settings_window.rs:3292`) stays the one funnel and starts
returning `combo_display(&it.combo)` for column 1 instead of `it.combo`.

The caps are drawn *over* text that is really there. That is what B.5's
"decoration over text that already works" meant, and it buys three things
that are easy to lose by drawing into an empty cell:

- A screen reader announces `Ctrl + Win + Alt + T`, matching the screen.
  Today it announces `ctrl+super+alt+t`; either is a real string, but only
  one of them matches what is drawn.
- `LVM_GETITEMTEXT` keeps working, so `examples/settings_probe.rs` can still
  read the row.
- The diff path in the list push compares text and is untouched.

The model's `ListItem::combo` is **not** changed — it stays the config
string, because `Model` writes it back to the file.

### B.6 Subitem 0 is gated, and this is the sharp edge

Giving the flag its own Caption-size face needs `CDRF_SKIPDEFAULT` on subitem
0. `LVS_EX_CHECKBOXES` rides in column 0's state image
(`settings_window.rs:1953`), and the per-row tick is what makes `Remove` a
multi-delete.

**Whether skipping the default draw on subitem 0 also removes that tick is
not known.** It is plausible either way: the state image may be drawn during
the item prepaint rather than the subitem one. Nobody has measured it, and it
must not be guessed at — a lost tick is not a cosmetic regression, it is the
delete path.

Gate G3 answers it. Either outcome has a landing:

- **Tick survives** → subitem 0 joins this pass: app name in Body, flag in
  Caption, and the `app_cell` IOU closes.
- **Tick is lost** → subitem 0 stays default-drawn, `app_cell` keeps
  appending the flag in Body as it does today, and the IOU stays open with a
  measurement attached to it instead of a plan. Drawing the state image by
  hand from `LVM_GETIMAGELIST(LVSIL_STATE)` is the escape hatch, and it is
  not worth taking for a font size.

---

## Part C — the modifier chips

The four editor modifiers (`IDC_MOD_CTRL`..`IDC_MOD_SHIFT`, 1028–1031) become
`BS_OWNERDRAW` buttons drawn by the same `draw_cap` as the list.

### C.1 `BS_OWNERDRAW` is a type, so auto-check is gone

`BS_OWNERDRAW` replaces `BS_AUTOCHECKBOX`; they are alternative values of the
same style field, not flags that combine. Three consequences, all of which
must land together:

1. **Windows stops tracking checked state.** `DRAWITEMSTRUCT.itemState`
   carries `ODS_SELECTED` for *pressed*, which is not the same thing. The
   window tracks the four booleans itself.
2. **`BM_SETCHECK` / `BM_GETCHECK` stop meaning anything on these four.**
   `settings_window.rs:1151` reads check state and 3061 / 3184 / 3549 / 4495
   write it; each site must be checked for whether it names a chip.
3. **`BN_CLICKED` becomes the toggle.** The handler flips the model's
   `ComboView` field and `InvalidateRect`s the one chip.

This is less disruptive than it sounds, because the model is already the
source of truth: `ControlState` holds the `ComboView`, `apply_state` already
pushes it into the controls, and `commit_fields` already compares
`ComboView`s rather than strings. What changes is the push — `InvalidateRect`
where `BM_SETCHECK` used to be — and the same suppression discipline applies.

**Re-read `settings_window.rs:4495` before touching it.** Its comment reasons
about `BM_SETCHECK` not raising a notification. An owner-draw toggle raises
whatever we choose to raise, so that reasoning does not carry over
unexamined; it has to be re-decided, not ported.

### C.2 Colours come from `GetSysColor`, and the brand blue does not ship

The mockup drew an armed chip in beckon's `#2563eb`. **The implementation
uses `COLOR_HIGHLIGHT` / `COLOR_HIGHLIGHTTEXT` instead**, and this is a
deliberate change from the drawing rather than an omission:

- It is the user's own accent, so an armed chip matches the row highlight
  four pixels above it instead of competing with it.
- It is the window's existing rule. "Light only, `GetSysColor` throughout" is
  a recorded decision, and a hard-coded literal is the first crack in it.
- It makes high contrast work for free — see §C.4.

No colour in this spec is a literal. If a shade is needed that `GetSysColor`
does not offer, that is a signal the shape is wrong, not that the rule is.

### C.3 Focus, disabled, mnemonics

- `ODS_FOCUS` → `DrawFocusRect` inside the cap. An owner-draw button draws
  no focus indication of its own, and losing it would take the keyboard route
  with it.
- `ODS_DISABLED` → `COLOR_GRAYTEXT` and no fill. Note this actually *fixes*
  §1's complaint in one place: unlike the `CBS_DROPDOWNLIST` beside it, an
  owner-draw chip looks disabled when it is disabled, because we decide.
- **No mnemonics, and no new collision risk.** The four chips deliberately
  carry none today: `t`, `w` and `l` belong to the `Hold` chips and `s` to
  Save, per the table in `mod cap`. Owner-draw does not change that, and the
  captions stay byte-identical so `layout`'s `tw()` measurement still works.

### C.4 High contrast is nearly free, and the part that is not

`WM_SYSCOLORCHANGE`, `WM_THEMECHANGED` and
`WM_SETTINGCHANGE(SPI_SETHIGHCONTRAST)` already funnel through a shared tail
that forwards the message verbatim to every child and relayouts
(`settings_window.rs:3637`). Because every colour in Part B and C comes from
`GetSysColor`, an owner-draw chip re-reads the right colours on the next
paint with no new code.

The one thing that is not free is **shape**. High contrast themes assume flat
fills and hard borders; a rounded cap with a soft bottom edge reads as a
rendering artifact there. So `draw_cap` takes an `hc: bool` from
`SystemParametersInfoW(SPI_GETHIGHCONTRAST)`, and in that mode:

- `Rectangle` instead of `RoundRect`,
- 1 px `COLOR_WINDOWTEXT` border,
- no bottom shadow line.

Cached and refreshed on the existing `SPI_SETHIGHCONTRAST` arm
(`settings_window.rs:3846`), never queried per paint.

---

## Part D — the `Hold` chips

The Caps row's three `Hold` chips (`IDC_HOLD_CTRL`..`IDC_HOLD_ALT`,
1022–1024) take the same treatment and the same painter.

They are in scope for one reason and it is not consistency for its own sake:
the `Hold` chips and the editor's modifier chips name **the same three
modifiers**, sit eight lines apart, and after Part C would be drawn two
different ways in one window. That is worse than either way alone.

They keep their mnemonics (`t`, `w`, `l`), so `draw_cap` must render the `&`
rather than print it. Pass the caption to `DrawTextW` with the `&` intact and
**without** `DT_NOPREFIX`, which is what draws the underline; then add
`DT_HIDEPREFIX` whenever `SystemParametersInfoW(SPI_GETKEYBOARDCUES)` reports
cues are off, which is what themed controls do and what makes the underline
appear on `Alt` rather than always.

A permanent underline is the failure mode here, and it is a *visible*
difference from every other caption in the window — which is the only reason
this paragraph exists, since `draw_cap`'s other four callers pass captions
with no `&` in them at all and would never surface it.

---

## Gates

The order matters; G1 comes before any code.

**G1 — one pass at 96 DPI, on the current build, before anything changes.**
Every measurement in this project is at 150 %. `tok` is written in 96-DPI
units, so the base case is the untested one, and Part A re-derives those
tokens. Tuning them against a metric nobody has seen is the mistake A.1 of
the earlier spec made about the manifest, one layer down.

**G2 — does `ShowWindow` on a populated `CBS_DROPDOWN` corrupt it?**
Blocks §A.2. Probe: fill the App combo, type a partial name that is a prefix
of a catalogue entry, `SW_HIDE` then `SW_SHOW`, read back with
`SendMessage(WM_GETTEXT)` — `GetWindowText` returns the kernel-side caption
and reads back empty for a COMBOBOX. Control: the same sequence with
`SetWindowPos`, which is known to corrupt, so a clean result and a blind
probe are distinguishable.
*If positive*: the empty state hides the **group** and shows a sibling STATIC
covering the same rect, leaving the children mapped underneath. Costs one
`SetWindowPos` on the group, none on the combo.

**G3 — does `CDRF_SKIPDEFAULT` on subitem 0 remove the checkbox?**
Blocks §B.6 only; the rest of Part B lands either way. Control: the same run
with subitem 1 skipped and subitem 0 default-drawn, which must show the tick.

**G4 — §F.4's `GetAsyncKeyState` union at commit.**
Blocks the *landing* of Part C, not its code. Hold `Ctrl`, click `Record`
with the mouse, press `Alt+T`: it records `alt+t`, because the hook never saw
the Ctrl-down. Part C puts the chips and `Record` at the centre of the
window's most-looked-at line; shipping a prettier control that silently
records the wrong chord makes the defect easier to hit and no easier to
notice. §2 of the followups already calls a silently wrong chord worse than a
refusal.

**G5 — `settings_probe` still reads everything.**
`examples/settings_probe.rs` pins ids 1001–1007 and reads via
`SendMessage(WM_GETTEXT)`, `GetClassNameW`, `GWL_STYLE` and
`CB_GETCURSEL`/`CB_GETLBTEXT`. Nothing in this spec renumbers a pinned id or
changes `IDC_COMBO`'s style, so the expectation is a clean run — which is
exactly why it must be run rather than assumed. Build with
`cargo build --all-targets`; `--examples` does not build `[[bin]]` targets and
will test a stale `beckon-serve.exe`.

Run all of these from **session 1** through a scheduled task registered with
`New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries -Priority 4` — both
flags. SSH into a14 lands in session 0, which has no desktop, so every result
there is a confident false negative.

---

## Landing order

| Landing | Contents | Why here |
|---|---|---|
| **G1** | The 96-DPI pass, no code | Part A re-derives `tok`; deriving against an unmeasured base case is the whole mistake |
| **3a** | Part A entire | Pure `layout`. Stands alone, and is what §1 actually complains about. Gated on G2 |
| **3b** | Part B (§B.1–B.5), Part C, Part D | One painter serves all three; splitting them means writing `draw_cap` twice and shipping a window with two chip styles in it. Gated on G4 |
| **3c** | §B.6, if and only if G3 says the tick survives | The smallest piece, and the only one whose scope a measurement decides |

Part A is the fallback plan as well as the first landing: if 3b never happens,
the window still loses its seven-control line, still gets its App combo back,
and still deletes two derived tokens.

---

## Rejected directions

Both were drawn at full fidelity before this one was chosen; neither is
wrong, and each fixes something Part A–D does not.

**"Xếp lại" — Part A alone, no drawing.** This *is* landing 3a, so it is not
rejected so much as absorbed. Recorded here because it is the exit if G4
never clears: `super` stays visible in the UI, and everything else in §1 is
fixed.

**"Hai khoang" — list left, detail pane right.** The only direction that
fixes two things this one does not: the list stops being eight fixed rows
(with 18 shortcuts the user scrolls on every open), and the notes get a
vertical column where three at once are readable rather than a strip where
they are not. Rejected because it reopens two recorded decisions — "five
horizontal bands" and "fixed height, internal scroll" — and because at
`MIN_WIDTH` 720 the split lands at roughly 400/312, which is the same
territory that produced the horizontal-scroll-bar defect (561 px of columns
inside a 482 px pane). Worth reconsidering if scrolling turns out to be the
real complaint; the notes half of it can be taken separately and cheaply.

---

## 7. Claims this spec amends

Written up in the house pattern: the claim, why it was reasonable, and what
changed. Do not silently re-add any of them.

### 7.1 `app_cell`: "it lands with the Fluent glyphs or not at all"

`settings_window.rs:3310` bundles two things — the flag at Caption size, and
the notes strip's Fluent glyphs — on the grounds that both need
`NM_CUSTOMDRAW`. The bundling was reasonable when neither had a plan.

It does not survive contact with the two surfaces being different controls.
The flag lives in a **ListView cell**, reachable by `NM_CUSTOMDRAW` subitem
draw. The glyphs live in the **notes STATIC**, which `NM_CUSTOMDRAW` cannot
reach at all — a STATIC has no per-run font either, so those glyphs need the
notes to become owner-drawn or to split into a glyph STATIC per line. That is
a different piece of work with a different cost, and it is **out of scope
here**.

So: the flag is unbundled and gated on G3; the notes glyphs stay deferred,
now with a reason rather than a pointer to a pass that was never going to
include them. `mark_glyph`'s ASCII marks and its measured advance table stand
unchanged.

### 7.2 "Keycaps are decoration"

Anticipating the objection, because it is the right one to raise about any
custom draw. The test it has to pass is whether it changes what the column
*says*, not how it looks — and it does: `super` leaves the interface. A
proportional `Ctrl+Win+Alt+T` string would achieve that alone, with no
`NM_CUSTOMDRAW` and no painter.

The shapes earn their cost on the second reading, which is the one that
happens eighteen times: caps segment the chord so the *key* is separable from
the *modifiers* at a glance, in a column where every row shares the same
three-modifier prefix. A string does not, and the prefix is 75 % of the
characters.

If G4 or G3 make Part B expensive, **the string alone is the honest fallback**
— `combo_display` from §B.4 with no painter, one line in `cells()`, and most
of the value.

### 7.3 The key list is not becoming a keycap

`IDC_COMBO` (1002) is the shortcut key `CBS_DROPDOWNLIST`. Drawing its current
selection as a cap would need `CBS_OWNERDRAWFIXED`, and that is refused here
for reasons that are worth writing down before someone adds it for symmetry:

- `CBS_OWNERDRAWFIXED` **without `CBS_HASSTRINGS` makes `CB_GETLBTEXT`
  meaningless**, and `examples/settings_probe.rs` reads this exact control
  with `CB_GETCURSEL` + `CB_GETLBTEXT`, deliberately, because a
  `CBS_DROPDOWNLIST` answers `WM_GETTEXT` with the selected item's text and
  the probe wants the index.
- 1002 is a pinned id whose style the probe reads and asserts on
  (`CBS_DROPDOWNLIST` present, `CBS_SORT` absent — and `ComboView::key` is an
  index into `key_table()`, so a sorted list writes a key the user did not
  choose, silently).
- The gain is one cap on a control that already reads correctly.

Three ways to break a measured invariant for a decoration. No.

### 7.4 The mockup's brand blue

The comparison drawing used `#2563eb`, taken from `assets/beckon.ico`. §C.2
replaces it with `COLOR_HIGHLIGHT` throughout. The drawing is not corrected —
it did its job — but nothing in it should be treated as a colour
specification. **No literal colour ships.**
