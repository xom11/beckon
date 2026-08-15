# The mock-up, measured — 2026-08-15

`specs/2026-08-14-four-doors-mockup.html` rendered in headless Chrome at its
own stated 680 px and read back with `getBoundingClientRect`, because two
constants were about to be changed on the strength of what it shows and
nobody had ever measured it.

**Why this was worth doing rather than reading.** The drawing's own hint line
says `680 × 600 at 100% DPI`, and the previous pass declined to shorten the
window on the grounds that *"the mock-up is drawn **without** a command bar"*
(tracking, `WINDOW_HEIGHT` row). Both statements are wrong about the file, and
each was load-bearing for a decision.

## Method

Chrome 1 tab over CDP, `Emulation.setDeviceMetricsOverride` 900×1400 at
`deviceScaleFactor: 2`, `--force-device-scale-factor=1`, each tab clicked in
turn and the visible `.page` measured. Script:
`scratchpad/measure-mockup.mjs` (not committed; ~120 lines of `node --eval`
against `/json/version`).

Control: the same run reports `.titlebar` at **34.0 px**, which is the one
figure in the drawing the design document independently fixes
(`chrome::TITLEBAR_H`). A probe that could not read a rect would not have
produced it.

## What it says

```
win           h = 496.9   w = 680
titlebar      h = 34      (design: TITLEBAR_H 34)          <- control
tabstrip      h = 47      trough 39, pill 29 (design: TABSTRIP_H 36)
cmdbar        h = 47      -- present, and a SIBLING of all four pages

page        page.h  cards.total  slack below last card  cards
p1 Shortcuts 368.9        350.9                     10      2
p2 Keyboard    336        233.5                   94.5      2
p3 System      374          364                     10      1
p4 About       336        322.3                   13.7      1

.srow  h = 46      .lrow h = 33      .divider h = 1 (+8 margin)
```

`34 + 47 + 368.9 + 47 = 496.9`.

## The three claims this settles

1. **The drawing is 497 px tall, not 600.** `.page` carries
   `min-height: 326px`, which is a floor that keeps the four doors from
   jumping as you click the strip — not a window height. Design §2's table
   derives the WIDTH at length and says nothing about the height, which came
   across from the pre-Four-Doors window.
2. **The command bar is on all four doors.** `.cmdbar` sits outside every
   `.page`, 47 px tall, carrying the service line, `Saved` and `Undo` — design
   §6.4's *replacement* for Save/Close, which is not the same as its absence.
   So "the mock-up has no command bar" cannot be used to explain away a page
   height again.
3. **The setting-row pitch is 46 px**, against `CTL + GAP` = 32 in the window.
   Over the System card's seven rows and two dividers that is the 110 px which,
   with the 103 px of window height, is the whole of the ground under it.

## What it does NOT settle

- **`.lrow` is 33 px and `tok::ROW_H` is 22.** The mock-up's header says
  "proportions inside a row are approximate", and the list's row height is not
  beckon's to choose anyway — comctl32 picks it from the live font, and
  `tok::ROW_H` is only the empty-list fallback. The drawing's six rows are a
  consequence of its height and its row, so they are **not** a target row
  count; see the §4 amendment.
- **`tabstrip` measures 47, not the design's `TABSTRIP_H 36`.** CSS padding
  and pill margins, not a disagreement worth chasing: the window reads the
  strip's own `strip_rect().bottom`, so it cannot drift from whatever
  `TABSTRIP_H` is.
- **Anything about the real window.** This is a picture. The four faults it was
  used against were found in photographs of the shipped binary
  (`fd-dark-*.png`), and the repair has not been photographed — see the
  hardware gate.

## Cross-check against the photographs

The hand-traced geometry was run against `fd-dark-*.png` (a14, 1020×900 @ 144
DPI, so ×1.5) before any constant moved. Predicted card bottoms in device px
against what the images show:

| Door | predicted | in the image |
|---|---|---|
| System | 498 | ~495 |
| About | 519 | ~517 |
| Keyboard | 234 | ~232 |

Within 3 px on all three, which is what makes the rest of the arithmetic in
`MIN_HEIGHT`'s comment trustworthy enough to change constants on.

## Still owed to hardware

The repaired window at 680 × 500 has **not** been photographed: a14 was
offline for this pass. What only a photograph can answer is the class of fault
that started this workstream — a card captioned `Keyboard` under a pill
captioned `Keyboard` is visible in `fd-dark-keyboard.png` and appears in no
test anywhere.
