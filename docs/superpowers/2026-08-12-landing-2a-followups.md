# Landing 2a — what it left owed

Date: 2026-08-12
Branch: `landing-2a-settings-window`, 31 commits, merged to `main`.
Hardware: a14 (Windows 11 26200, ARM64), 150 % scale, two verification passes.

Landing 2a rebuilt the `beckon-serve` settings window. This file is the residue:
work deliberately sequenced out, defects judged not worth blocking on, and the
things a probe cannot settle. Everything here was found and adjudicated during
the landing — none of it is speculation.

The permanent measurement record is
`docs/superpowers/measurements/2026-08-11-landing-1-a14.md`. Refuted claims live
in `docs/superpowers/specs/2026-08-11-settings-window-redesign.md` §7.

---

## 1. The filter box — deliberately not built, and the trap that is why

The plan's Task 6 lists a filter `EDIT` in band 2. It was split out rather than
shipped, and the final whole-branch review endorsed the split.

**The trap, which is the whole reason:** `on_select(i)` and `on_mark(i)` index
`Model.rows` **directly**. The moment the list shows a filtered subset, `i` is a
*view* index, and both callbacks address the wrong row — ticking one binding and
deleting another. On a destructive button with no confirm and no undo.

So the filter cannot land as a control. It lands as a control **plus** the
mapping that makes it safe:

- `ListItem` carries its model-row index.
- The window passes `items[i].row` to the callbacks instead of `i`.
- `beckon-core` tests pin the mapping, because `beckon-windows` compiles on one
  CI job in three.

Band 2's layout reserves no gap for it, and no code assumes it exists, so adding
it is additive rather than a rework.

## 2. Needs a person at the screen — RESOLVED 2026-08-12

All five are settled, on branch `landing-2a-followups`. **Only two of them
turned out to be aesthetic judgements**; the rest were answerable from the
source or from a measurement, which is the reusable lesson here — "a probe
cannot settle this" was true of two and assumed of five. Full numbers in
`docs/superpowers/measurements/2026-08-11-landing-1-a14.md` §36–§37.

- **Row density** — *judged, kept*. The framing was wrong and worth
  correcting: §22 read the row as "the tabulated 32 used **unscaled**",
  against buttons that scale to 48. But `list_row_height` uses no token at
  all while the list has rows — it asks comctl32 through `LVM_GETITEMRECT`,
  and comctl32 derives the row from the Body font, which *is* DPI-scaled.
  32 physical at 144 DPI is comctl32's answer, colliding numerically with
  `tok::CTL = 32` by chance. §13 measured **29** at the same DPI before the
  type ramp landed, and the row grew to 32 when the 14 px Body font was
  applied — which is what "derived from the font" predicts. So there was no
  asymmetry to fix, and no `LVSIL_SMALL` image list was needed.
- **`EDIT` vs `COMBOBOX` alignment** — *fixed*. The `EDIT` now takes the
  height the combo's theme picked, reusing the `GetWindowRect` `layout`
  already made in order to centre the combo, so this adds no input to
  `layout` and nothing new runs on a data push. A/B on hardware against a
  deliberately-disabled build: 43/36 → 36/36. Descenders verified unclipped
  at the smaller height.
- **Subtitle descenders** — *moot, structurally*. The only `Role::Subtitle`
  control is `IDC_LBL_SECTION`, whose text is the constant `"Shortcuts"` —
  a string with no descender in it. The band is `ctl = s(32)` against an
  `s(20)` font, so even one that had them would clear. Reopen only if that
  heading ever becomes a variable string.
- **`mark_glyph`'s `OK` prefix** — *was a false claim, not a matter of
  taste*. The comment asserted all four marks were "two columns wide", which
  is a monospace property; `OK` measured 15 px wider than `!!` at 144 DPI,
  and the other three were 2 px apart rather than equal. `Mark::Ok` is now
  two spaces.
- **The two long Caps captions** — *moot, do not tune*. Spec §F.8 deletes
  both strings in landing 2b: the three `IDC_TAP_*` radios go, and the
  phrase "beckon key" leaves the window. Worth noting what the screenshot
  shows in the meantime, because it is evidence §F.8 is right: radio 1's
  caption is `"&Tapping Caps alone: Caps Lock"`, so the question governing
  the group is glued to the first option — `Esc` and `nothing` do not read
  as answers to it, and the mnemonic sits on "Tapping" rather than on the
  option it selects.

Screenshots from the hardware pass: `C:\Users\kln\hwpass\settings-{valid,readonly}.png`.
Zoomed crops per question, and the after-shot of the repaired editor strip,
were cut with a stdlib-only PNG cropper — **PIL and ImageMagick both fail to
load on the mac**, their bundled dylibs refused by macOS system policy, and
that is not something the Claude sandbox flag turns off.

## 3. Deferred defects — real, judged not to block

- **A registration failure that is not a collision** (reserved key, bad vk) is
  described to the user as "Another program already has this shortcut." The raw
  OS string still reaches `--log`, so nothing is lost diagnostically, and the
  plan's vocabulary table mandates that string verbatim — so this is
  plan-mandated, not an oversight. Wants an `Err(e)` arm that appends the code
  when the cause is not a known collision.
- **`layout` has five inputs and the guard tracks four.** Column widths come
  from `GetClientRect(list)`, which shrinks by `SM_CXVSCROLL` when the ListView
  grows a vertical scrollbar — a transition that changes neither
  `shown_external` nor `shown_empty`. Documented rather than guarded, on three
  grounds: the error is always a **gutter, never a clipped column**; guarding
  would mean running `layout` on more data pushes, re-entering the
  `SetWindowPos` path that caused the combo-box data loss; and there was no
  hardware time to validate a new trigger. If it is ever fixed, the shape is a
  `shown_list_w` sibling, not a wider `layout`.
- **`WM_APP_EDITED` + `app_epoch`** are belt-and-braces now that the real cause
  is known. Their comment says "deferred debt, not settled design" in those
  words. Collapsing them has to re-establish the `CBN_CLOSEUP` ordering that an
  earlier fix settled. A state push between post and dispatch still drops one
  keystroke.
- **`add_row()` sets `dirty`**, so pressing Add alone enables Save on a file
  that would be written back byte-identical.
- **`settings_probe` ends with `FAIL: Apply stayed disabled`.** The probe is
  wrong, not beckon — it types a combo another row already uses, so
  `apply_enabled` is correctly false. Proven by A/B over two configs differing
  by exactly that row. One line to fix in the probe.
- **`pick_from_dropdown`'s doc comment overclaims**: arrowing updates the edit
  field, so the precondition that made the backstop destructive is absent. The
  **mouse** pick still has no automated observer.
- Minor: `BN_PAINT == 1 == CMD_FROM_ACCELERATOR`, so a comment describes a
  filter that does not exist — harmless (`BN_PAINT` needs `BS_OWNERDRAW`), and
  **do not "fix" it by dropping code 1**. Two duplicated `GW_CHILD` enumeration
  loops under a "one funnel" comment. `suppress` is a `bool` rather than a
  counter, safe only because nothing `apply_state` sends raises `BN_CLICKED`.
  `sync_list` builds two `Vec<String>` per row per keystroke.

## 4. Behaviour changes worth a changelog line

- **`beckon-serve.exe` now starts on a config that does not parse** — tray
  installed, no hotkeys registered, nothing written, settings window read-only.
  `beckon.exe serve` still exits non-zero and `beckon check` is unchanged. The
  old exit-1 interacted badly with `<RestartOnFailure>` in
  `examples/windows/serve/beckon-serve.xml`, producing a restart loop; that is
  gone.
- **Every push button now fires once per double-click**, not twice, because
  `BS_NOTIFY` delivers the second click as `BN_DBLCLK`. Deliberate: `Remove` is
  destructive with no confirm and no undo.
- **Enter is not unconditional.** The default ring migrates to whichever push
  button has focus, so Tab to `Close` then Enter closes. Only `Ctrl+S` always
  saves. This exists because Enter on a focused `Reload` used to save and
  overwrite the external change the banner was protecting.

## 5. The lesson this landing paid for six times

Six separate checks could not distinguish success from a broken detector:

1. "Confirm the view stays put after Add" — the opposite of the requirement.
2. "Three distinct `HFONT`s at 20:14:12" — satisfied equally by total silent
   fallback, because the fallback preserves size and weight and surrenders only
   the face. Only the **face name** discriminates.
3. A probe using a synchronous control for an asynchronous subject, so a short
   sleep read exactly like the defect.
4. `pick_from_dropdown` claiming a path whose precondition it does not create.
5. A step asserting `VK_SPACE` does nothing, while the regression it guarded
   was about `VK_TAB`.
6. Verifying a formatting-gate claim by running the formatter on already-clean
   files — which proves the files are clean and says nothing about the gate.

The one that worked every time: **break it on purpose and check the detector
notices.** That is how the manifest bug was caught in Landing 1, how the
`cargo fmt` claim was refuted, how the combo root cause was isolated (two
scenarios differing by one `SetWindowPos`), and how the multi-delete test was
validated (revert the fix, watch it fail).
