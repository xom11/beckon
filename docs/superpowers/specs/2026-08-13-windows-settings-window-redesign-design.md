# beckon-serve on Windows: a themed settings window

**Date**: 2026-08-13
**Status**: design, not built
**Scope**: `crates/beckon-windows/src/settings_window.rs` and a new
`beckon-core` palette module. Windows only.
**Reference**: [VKey](https://github.com/phatMT97/VKey) — a Vietnamese input
method whose UI is Sciter.JS (HTML/CSS) inside a Win32 exe.
**Mockup**: 1:1 light and dark specimens, published as an artifact.

---

## 1. What this reverses, and what it keeps

`settings_window.rs` repeats one rule throughout: **"Every colour is
`GetSysColor`."** The 2026-08-11 spec chose *"Dark mode: Light only,
`GetSysColor` throughout; high-contrast themes are the supported dark path"*
and rejected uxtheme ordinals outright.

This design overturns the *conclusion* and keeps the *reason*.

The reason `GetSysColor` was chosen is that it makes high contrast correct
for free. That property is preserved by making the theme **three branches**
rather than two:

| Branch | Source of colour |
|---|---|
| `Theme::Light` | the literal palette in §3 |
| `Theme::Dark` | the literal palette in §3 |
| `Theme::HighContrast` | `GetSysColor`, exactly as today |

The old rule becomes one arm of a `match`, not a deleted line. Nothing about
high contrast changes, and the accessibility path stays the one that is
already known to work.

**What is NOT reversed**: the toolkit. Spec §7.1 rejected egui / iced / Slint
/ WinUI 3, and VKey's own answer (Sciter.JS) is a fourth member of that
family. The 6689 lines in `settings_window.rs` encode behaviour measured on
real hardware — the `SetWindowPos`-on-a-populated-combo data-loss path, the
ListView custom-draw traps, `default_button_of`, the DPI arithmetic — none of
which survives a rewrite, and all of which would have to be rediscovered.
This is a repaint in the GDI already present.

---

## 2. Scope

**In**: custom title bar (icon, app name, version, minimize, close), light /
dark following Windows, slight transparency, rounded window corners, cards
replacing flat bands, a toggle switch for the one setting that is a setting,
button and field restyling, an extended type ramp, a refreshed app icon.

**Out**: information architecture. Band order, the controls in each band,
command-bar button order, and tab order are all unchanged. No tab bar, no
two-column layout, no i18n. `beckon-macos/src/settings_window.rs` is not
touched.

**No new config keys.** The theme follows Windows and is not configurable —
see §4. The 2026-08-11 decision *"New config keys: exactly one:
`keyboard.caps_hold`"* still holds.

---

## 3. Palette

Every foreground/background pair the window actually draws was checked
against WCAG by computation. **The first pass failed five pairs**; those five
tokens were stepped toward black or white until they passed. Final state:
28/28 pairs at AA (4.5:1 for text, 1.2:1 for structural borders).

The fixes are worth recording, because each is a trap:

| Pair | First value | Fixed to | Why it failed |
|---|---|---|---|
| light faint-on-card | `#757D8B` | `#6F7785` | 4.15 — a grey that "looks light enough" is not |
| dark faint-on-card | `#79818F` | `#7F8795` | 4.15 — same error, mirrored |
| light accent-on-soft | soft `#E7EFFE` | `#E8F0FF` | 4.47. **The soft fill gave way, not the accent** — darkening `accent` would have changed every accent word in the window |
| light card-border-on-bg | `#E3E7EF` | `#DCE0E8` | 1.13 — the card had no visible edge against the window |
| dark white-on-accent-fill | `#3B72E8` | `#3970E6` | 4.41 — the dark accent *fill* had drifted too light for white text |

### Tokens

| Token | Light | Dark | Notes |
|---|---|---|---|
| `bg` | `#F2F4F8` | `#15171C` | window base; the only surface that may be translucent |
| `card` | `#FFFFFF` | `#1D2027` | always opaque |
| `card_border` | `#DCE0E8` | `#2B303A` | 1 px; no drop shadow |
| `text` | `#15181E` | `#E7E9EE` | 17.78 / 13.42 |
| `text_muted` | `#5A6270` | `#9FA6B4` | 6.15 / 6.67 |
| `text_faint` | `#6F7785` | `#7F8795` | 4.51 / 4.50 |
| `accent` | `#2563EB` | `#5B92F7` | 5.17 / 5.36 — **text** colour |
| `accent_fill` | `#2563EB` | `#3970E6` | 5.17 / 4.53 with white — **fill** colour |
| `accent_soft` | `#E8F0FF` | `#1B2A47` | selected row, hover |
| `field` | `#FFFFFF` | `#23262E` | |
| `field_border` | `#D2D8E3` | `#353A45` | |
| `keycap` | `#FFFFFF` | `#292D36` | |
| `keycap_border` | `#CDD4E1` | `#39404B` | |
| `keycap_edge` | `#B6BFCF` | `#131519` | the 2 px bottom edge that makes it a key |
| `bad_bg` / `bad` | `#FDE7E7` / `#B42318` | `#3A1C1C` / `#FF9A92` | 5.56 / 7.55 |
| `warn_bg` / `warn` | `#FDF0D5` / `#8A5406` | `#372911` / `#F2C46B` | 5.55 / 8.66 |
| `unk_bg` / `unk` | `#EDEFF4` / `#5A6270` | `#252932` / `#9FA6B4` | 5.34 / 5.96 |
| `ok` | `#067647` | `#5CCB92` | 5.69 / 8.08 |
| `divider` | `#E8EBF1` | `#272B33` | |

**`accent` and `accent_fill` are separate tokens and must stay separate.** A
colour that reads well as text on a card and a colour that carries white text
on top of it are different constraints, and in dark mode they resolve to
different hexes. Collapsing them is the defect this table exists to prevent.

### The palette lives in `beckon-core`, and CI enforces it

`beckon_core::theme` owns the two palettes as plain data, plus a
`contrast(fg, bg) -> f64` function and a unit test that asserts every pair in
the table above clears its floor. That test runs on all three CI jobs, not
just the Windows one.

This is the same move `RuntimeStatus` already makes for `apply_enabled`: put
the decision where the two non-Windows jobs can compile it. A palette checked
once by a script on someone's laptop is a palette that drifts on the next
edit; a palette with a failing test is not.

`beckon-windows` converts a token to `COLORREF` at the boundary and holds no
literals of its own.

---

## 4. Theme detection

Read `HKCU\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize`
→ `AppsUseLightTheme` (DWORD; 0 = dark, absent = light).

React to:

- `WM_SETTINGCHANGE` where `lParam` is the string `"ImmersiveColorSet"` — the
  live light/dark flip, no restart.
- `WM_THEMECHANGED` — high contrast on/off.
- `SPI_GETHIGHCONTRAST` at startup and on the above; `HCF_HIGHCONTRASTON`
  selects `Theme::HighContrast`, which outranks the registry.

On any change: rebuild the brush/pen cache, call
`DwmSetWindowAttribute(DWMWA_USE_IMMERSIVE_DARK_MODE)` so the DWM border and
shadow follow, re-evaluate the transparency tier (§8), and `InvalidateRect`
the whole window. **Do not call `layout`** — nothing about a colour change
moves a control, and `layout` means `SetWindowPos` on the populated App
combo, which is the measured data-loss path.

High contrast additionally keeps its existing effect on keycap *shape*
(hard rectangle rather than rounded), already implemented.

---

## 5. Chrome: the custom title bar

40 px tall, drawn in the client area.

```
┌──────────────────────────────────────────────────────┐
│ ▣  beckon 0.9.3                            ─    ✕    │   40 px
└──────────────────────────────────────────────────────┘
   14  18px   15px   12px                  46×40  46×40
```

- Left: 14 px inset, the 18 px app icon, 10 px gap, `beckon` in **Title**
  (15 px Semibold, `accent`), then the version in **Caption** (12 px,
  `text_faint`), baseline-aligned.
- Right: two full-height 46 × 40 buttons. Glyphs from **Segoe Fluent Icons**
  — `U+E921` minimize, `U+E8BB` close — falling back to Segoe MDL2 Assets
  (same codepoints), then to GDI line drawing.
- Hover: minimize takes a 5 % surface tint; close takes `#C42B1C` with a
  white glyph. This is the Win11 convention and is a **deliberate divergence
  from VKey**, whose close button is a permanent red circle — that reads as
  macOS and sits oddly on a Windows taskbar.

### Messages

- `WM_NCCALCSIZE` (`wParam == TRUE`): take `DefWindowProc`'s result, then add
  the top inset back so the caption band becomes client area. Keep the left,
  right and bottom resize borders.
- `WM_NCHITTEST`: `HTCLOSE` / `HTMINBUTTON` over the two buttons, `HTCAPTION`
  over the drag strip, and the eight frame results over the borders.
  `DefWindowProc` first, then override.

### Maximize is removed, and that is the point

`WS_MAXIMIZEBOX` comes off the window. This is not a cosmetic cut — it closes
the two worst traps of a custom title bar at once:

1. No `HTMAXBUTTON`, so no Snap Layouts flyout obligation.
2. `WM_NCCALCSIZE` in the maximized state overflows the monitor by the frame
   thickness unless corrected by hand. With no way to maximize (the button is
   gone and `Win`+`Up` no longer applies), that state is unreachable.

The window stays resizable by dragging its edges; `layout` already handles
that. Double-click on the caption is suppressed.

`DwmSetWindowAttribute(DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND)` — one
call, no-op on Windows 10.

---

## 6. Type

The measured ramp is kept and one role is added. Every font goes through the
existing `make_font`, whose `GetTextFace` round-trip is **mandatory**.

| Role | Face to request | px | Used by |
|---|---|---|---|
| Title | `Segoe UI Variable Display Semib` | 15 | title-bar app name |
| Subtitle | `Segoe UI Variable Text Semibold` | 18 | `Shortcuts` (down from 20) |
| **BodyStrong** *(new)* | `Segoe UI Variable Text Semibold` | 14 | card captions, column headers, `Save` |
| Body | `Segoe UI Variable Text` | 14 | default |
| Caption | `Segoe UI Variable Small` | 12 | notes, version, count, pills |
| Keycap | `Segoe UI Variable Small Semibol` | 11 | keycap letters |
| Chrome | `Segoe Fluent Icons` | 10 | caption glyphs |

**The truncation is not uniform, and getting it wrong is silent.**
`lfFaceName` holds 32 wchar: `Text Semibold` fits at 31 characters, while
`Display Semib` and `Small Semibol` are cut. Measured on a14 2026-08-11,
`CreateFontW` for `"Segoe UI Variable Text Semib"` succeeded and returned
**Arial**. `make_font` catches this and falls back to the shell font with the
size and weight preserved; that fallback is the safety net, not the plan.

Segoe Fluent Icons and Segoe MDL2 Assets were both confirmed present on a14
in the same pass.

---

## 7. Geometry

Window **900 × 740** at 96 DPI, up from 860 × 640. Three cards at 16 px inner
padding need the room. Everything scales by the existing `s()` helper.

| Piece | Today | Proposed |
|---|---|---|
| window | 860 × 640 | 900 × 740 |
| title bar | system | 40 px, client-drawn |
| caption buttons | ─ □ ✕ | 46 × 40, ─ and ✕ only |
| card | flat band | radius 10, 1 px border, 16 px padding |
| gap between cards | `BAND` 14 | 12 |
| list row | ~22 px | 26 px |
| rows | 8 fixed | **8 fixed, unchanged** |
| list border | `WS_BORDER` | none — the card is the border |

Height budget, which comes out exact:

```
title bar                              40
body padding                        2× 16
  Shortcuts card    16+32+12+238+16 = 314
  gap                                  12
  editor card    16+20+10+30+10+30+12+40+16 = 194
  gap                                  12
  keyboard card     16+20+10+30+16 =   92
  gap                                  12
  command bar                          32
                                    -----
                                      740
```

The command bar keeps `margin-top: auto` semantics — slack lands above it, so
it stays anchored to the bottom edge exactly as the two bottom bands do
today.

**Row height is forced through the state image list.** The ListView takes its
row height from its small/state image list, so sizing that list to 26 px is
the lever. Whether the tick still centres afterwards is gate 05.

---

## 8. Transparency: three tiers, self-demoting

This is the riskiest part of the design and the honest statement is:
**Mica under a client area painted entirely with GDI is not a sure thing.**
DWM composites the backdrop *behind* the window, so any opaque GDI fill hides
it.

| Tier | Mechanism | Requires |
|---|---|---|
| **T1** | `DWMWA_SYSTEMBACKDROP_TYPE = DWMSBT_MAINWINDOW` plus `DwmExtendFrameIntoClientArea` with −1 margins; paint **only the cards** and leave the base untouched | Win11 22H2 (build 22621) |
| **T2** | `SetLayeredWindowAttributes(hwnd, 0, 245, LWA_ALPHA)` — uniform ~96 % | Win10 and up |
| **T3** | fully opaque | always available |

T1's known hazard — GDI text on glass loses its alpha and renders with black
fringing — is avoided by construction: every string in this window is drawn
inside an opaque card. If gate 01 shows the unpainted region does not
composite cleanly, T2 ships instead, and the design is unaffected because no
text ever depended on the effect.

**T3 is mandatory, not a fallback, when**: high contrast is on;
`GetSystemMetrics(SM_REMOTESESSION)` is non-zero; or the user has turned off
*Settings → Personalisation → Colours → Transparency effects*
(`…\Themes\Personalize\EnableTransparency` = 0). Honouring that last one is
correctness, not politeness.

---

## 9. Controls

**Toggle** (`IDC_CAPS` only). 40 × 20 pill, radius 10, 14 px knob inset 2.
Off: `field` track, `field_border` edge, `text_muted` knob. On: `accent_fill`
track, white knob. Focus: 2 px `accent` ring, offset 2.

It stays a real `BS_AUTOCHECKBOX` and is drawn through **`NM_CUSTOMDRAW`**,
not `BS_OWNERDRAW`. `BS_OWNERDRAW` cannot be combined with another button
type, and swapping it in would throw away the checkbox state machine and the
UIA role a screen reader reads. This is the only toggle in the window — the
per-row list ticks stay checkboxes, because they are a multi-select gesture,
not a setting.

VKey draws a `0` / `1` digit inside the knob. Skipped: the knob's position
already says everything the digit would.

**Buttons**, three tiers:

- accent (`Save`): `accent_fill`, white, radius 6, no border. Disabled:
  `field` fill, `text_faint` text.
- secondary (`Add`, `Remove`, `Reload`, `Open config file`, `Close`,
  `Keep mine`): `field` fill, 1 px `field_border`, radius 6. Hover:
  `accent_soft`.
- small outline (`Record`, `Reset`): transparent fill, 1 px `accent` border,
  `accent` text, height 26, radius 6. `Stop` swaps the border and text to
  `bad`.

**Fields.** `IDC_APP` (a `CBS_DROPDOWN` with an edit child) and `IDC_FILTER`
are **not owner-drawn**. They get their colours via `WM_CTLCOLOREDIT` /
`WM_CTLCOLORLISTBOX`, lose `WS_BORDER`, and the parent draws a rounded 1 px
border around each control's rect during its own `WM_PAINT`. Nothing reaches
inside the control — that is where the measured data-loss defect lives.

`IDC_COMBO` and `IDC_TAP` are both `CBS_DROPDOWNLIST` with no edit child and
are read by index, so they take `CBS_OWNERDRAWFIXED` + `WM_DRAWITEM` and are
fully themed. `settings_probe.rs` reads their style bits and must be updated
in the same change.

**List.** Header custom-drawn: `card` ground, `text_muted` BodyStrong
caption, 1 px `divider` underneath, no sort arrows. Selected row:
`accent_soft` fill plus a 2 px `accent` left bar — not a full accent fill,
which would fight the keycaps and the status pill. Hover: 4 % tint. No zebra
striping, no grid lines. `LVS_EX_DOUBLEBUFFER` on.
`SetWindowTheme(list, "DarkMode_Explorer", null)` for the scrollbar — a
public exported function, **not** one of the uxtheme ordinals the previous
spec rejected, though the theme class name itself is undocumented and the
call degrades silently.

**Status pills.** Colour encodes severity — `Mark::Bad` red, `Mark::Warn`
amber — and the word says which condition. `key in use` and `not installed`
therefore share a colour: they are both `Mark::Bad`, and the cell holds one
word. A healthy row shows no pill, which is the existing rule. The precedence
`paused` > `key in use` > `not installed` > `custom` is untouched and stays
in `row_condition`.

**Notes.** `IDC_NOTES` becomes `SS_OWNERDRAW`, one line per note with a 7 px
severity dot at a fixed x. This retires the `!` / `!!` prefix and its
alignment-preserving trailing space — alignment becomes structural.

---

## 10. Icon

Same mark, same blue. The square becomes a Win11-style rounded tile
(radius 22 %) with a vertical gradient `#3B82F6` → `#2563EB`, whose lower
stop **is** the `accent_fill` token rather than a colour that resembles it.

The stem and bowl are drawn as geometry — a rectangle, an outer circle, and a
counter circle painted with the same `userSpaceOnUse` gradient — not set in a
typeface. The counter therefore stays open at 16 px and the build needs no
font present. The 16 px tile is hand-tuned: a heavier stem (10.8 % vs 9.5 %)
and less rounding (17 % vs 22 %), so it neither mushes nor reads as a circle.

Rendered to **`assets/beckon-v2.ico`** at 16 / 32 / 48 / 256.
**Not yet wired into `beckon.rc`** — `assets/beckon.ico` is untouched, and
the swap is an implementation step.

---

## 11. Code organisation

`settings_window.rs` is 6689 lines before this change and would clear 8000
after. Split it as part of the work, isolating the new and risky code:

```
crates/beckon-windows/src/settings_window/
├── mod.rs       window creation, wndproc, message routing, state
├── theme.rs     Theme resolution, brush/pen cache, WM_SETTINGCHANGE
├── chrome.rs    custom title bar: NCCALCSIZE, NCHITTEST, caption buttons
├── paint.rs     card / keycap / toggle / pill / note / field-border drawing
└── layout.rs    the layout function, unchanged in behaviour
```

`beckon-core` gains `theme.rs`: the two palettes as data, `contrast()`, and
the CI-enforced contrast test from §3.

---

## 12. What only hardware can answer

Each needs a control that proves the probe can fail — a clean result from a
blind detector is indistinguishable from a broken one, which is the trap
`caps_probe` was built to avoid. Run through a scheduled task in **session 1**
with `-AllowStartIfOnBatteries -Priority 4`; an SSH shell lands in session 0
and every answer there is a confident false negative. Build with
`--all-targets`, not `--examples`.

1. **Mica under GDI.** Does the unpainted region composite the backdrop, or
   show garbage? Garbage → ship T2.
2. **Custom title bar.** A 1 px artefact along the top edge? Are the resize
   borders still grabbable? What happens dragging across monitors of
   different DPI?
3. **`SetWindowTheme(list, "DarkMode_Explorer")`.** Does the ListView
   scrollbar actually go dark on this build?
4. **`CBS_OWNERDRAWFIXED` on `IDC_COMBO` / `IDC_TAP`.** Does typeahead still
   move the selection as before, and does the index read back unchanged?
5. **26 px rows via the state image list.** Does the tick still centre?
6. **Chrome glyphs.** The fonts are confirmed present; they have not been
   rendered at 10 px in a title bar.

---

## 13. Invariants this design must not break

- `tok::ROWS = 8`, fixed at every DPI, measured rather than scaled.
- `CBS_SORT` is **never** set on `IDC_COMBO`. `ComboView::key` is an index
  into `shortcuts::key_table()`; sorting shifts every index and writes a key
  the user did not choose, silently.
- `cap::STOP` stays narrower than `cap::RECORD`. A wider armed caption forces
  `layout` onto the capture path, and `layout` means `SetWindowPos` on the
  populated App combo.
- **No Shift chip in the `Hold` row.** `Chord` carries only ctrl / super /
  alt, because the hook must release what it presses, and releasing Shift
  under the user's fingers makes everything they type next arrive lowercase.
- Command-bar button order unchanged: `Save` · `Reload` · `Open config file`
  · `Close`. Reordering changes tab order and is not a repaint's business.
- Control ids 1001–1008, 1012, 1013 unchanged — `settings_probe.rs`
  hard-codes them.
- `Theme::HighContrast` reads `GetSysColor` verbatim.
- `beckon-macos/src/settings_window.rs` untouched.

---

## 14. Deliberate divergences from VKey

| VKey | beckon | Reason |
|---|---|---|
| permanent red circular close | flat ✕, red `#C42B1C` on hover only | Win11 convention; a permanent red dot reads as macOS |
| `0`/`1` digit inside the toggle knob | plain knob | the digit adds nothing the knob position does not say |
| Sciter.JS | raw GDI | §1 |
| tab bar + two columns | existing band order | changes where functions live, not how they look |
| bilingual VI/EN | English | settled by the existing spec; i18n is a feature, not a repaint |

---

## 15. Next step

Implementation plan, then build behind the gates in §12. Nothing in §8 or §5
should be called done on the strength of a macOS `cargo check` —
`--target aarch64-pc-windows-msvc --all-targets` compiles this code without
MSVC, and that is the limit of what this host can assert.
