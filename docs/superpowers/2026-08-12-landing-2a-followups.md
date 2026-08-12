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

## 2. Needs a person at the screen — a probe cannot settle these

The display on a14 is at 150 %; changing it would disturb a live signed-in
session, so nothing below was measured at 100 %.

- **Row density.** comctl32 derives the ListView row height from the font;
  measured 32 px physical at 144 DPI, while buttons scale to 48. Spec §B.2
  tabulates 32 as a 96-DPI token, which the plan's Task 6 supersedes with "use
  what Task 2 measured". If rows read too dense, the lever is a dummy
  `LVSIL_SMALL` image list sized to the target height — and the thing to check
  afterwards is whether the checkbox and the App text shifted right, because
  that image list reserves icon width in column 0.
- **`EDIT` vs `COMBOBOX` alignment** on the editor line: centres agree within
  0.5 px but the heights are 43 vs 36. A single-line `EDIT` top-aligns its text,
  which is why it was deliberately not stretched.
- **Whether the Subtitle loses descenders** at any scale. Band 2's height is a
  token and `SS_CENTERIMAGE` clips rather than grows. The ratio
  `scale(20)`:`scale(32)` holds at every DPI, so one look at 100 % settles all
  scales. Fix if needed: derive that band's height from its own `text_size`.
- **`mark_glyph`'s `OK` prefix** leading the note text, and whether `OK` / `!!` /
  `!` look aligned.
- **The two long Caps captions** at Body 14 px.

Screenshots from the hardware pass: `C:\Users\kln\hwpass\settings-{valid,readonly}.png`.

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
