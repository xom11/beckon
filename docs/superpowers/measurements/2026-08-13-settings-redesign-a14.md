# Settings-window redesign on a14 — what was measured, and what was not

Windows 11 Home build 26200, ARM64, 144 DPI (150 % scaling), session 1 via
scheduled task. Native build, `rustc 1.97.1`.

**This file replaces the runbook that stood here.** That version was written
from macOS with every result blank. Some of those gates have now been run and
several found defects; the ones still unrun are listed at the bottom, still
marked, because a half-filled file that quietly drops its markers is worse
than one that keeps them.

---

## The automated gate, on the target OS

| Command | Result |
|---|---|
| `cargo build --release --all-targets` | clean, 1 m 45 s cold |
| `cargo clippy --workspace --exclude beckon-linux --exclude beckon-macos --all-targets -- -D warnings` | **clean** |
| `cargo test --workspace --exclude beckon-linux --exclude beckon-macos` | **all pass** (248 core, 48 cli, rest 0-failure) |

This is the check the branch never had: every task before this ran on a macOS
cross-compile, which cannot see a warning in `beckon-windows` at all.

---

## Gates that PASSED

**Type ramp (gate 08).** `WM_GETFONT` + `GetObjectW` per control, reading
`lfFaceName` — the only field that discriminates, because `make_font`
preserves size and weight across a fallback and only the face reveals it.

| Role | Face read back | `lfHeight` | `lfWeight` |
|---|---|---|---|
| Subtitle | `Segoe UI Variable Text Semibold` | −27 | 600 |
| BodyStrong *(new)* | `Segoe UI Variable Text Semibold` | −21 | 600 |
| Body | `Segoe UI Variable Text` | −21 | 400 |
| Caption | `Segoe UI Variable Small` | −18 | 400 |

**Not one control reported plain `Segoe UI`**, so no role fell back. Both card
captions (`IDC_GRP_EDITOR`, `IDC_GRP_KEYBOARD`) read BodyStrong, confirming
Task 8's group-box-to-STATIC conversion took.

**Window geometry.** 1350 × 1110 at 144 DPI against a declared 900 × 740 —
exact, `× 1.5`. After the compaction pass: 1140 × 900 against 760 × 600, also
exact. Eight list rows present in both.

**`WS_MAXIMIZEBOX` genuinely absent** (`GetWindowLongW(GWL_STYLE) & 0x00010000
== 0`), so Task 7's style half was always working — which is what made the
next finding attributable.

---

## Gates that FAILED, and what they cost

**The client-drawn title bar never worked at all.** The window wore TWO title
bars — the system caption and its own, drawn underneath. Measured:
`ClientToScreen(0,0).y − GetWindowRect().top == 45`, which is `SM_CYCAPTION`
34 + `SM_CYSIZEFRAME` 5 + `SM_CXPADDEDBORDER` 6, i.e. the untouched default,
where a working handler gives 0.

The diagnosis took three wrong turns and they are the useful part:

1. **Blamed aliasing** — a `&mut NCCALCSIZE_PARAMS` held across
   `DefWindowProcW` letting the compiler drop the read-back. Plausible enough
   to write, commit and explain. Rewrote it with raw pointers: **still 45.**
2. Made the handler `return LRESULT(0)` unconditionally, which should have
   given client == window rect: **still 45.** So the handler was never called.
3. Logged from inside it, with a control in `WM_CREATE`. Control fired,
   handler did not — but the log line sat *after* the `wparam == 0` early
   return. Moved it to the first line: `wparam=0`, exactly once.

**Windows sends this window `WM_NCCALCSIZE` only in the `wParam == FALSE`
form**, and every published sample of the technique — and this code — handles
only `TRUE`. Fixed by handling both. `TOP INSET` went 45 → 0.

**Mica does not composite under a GDI-painted client (gate 01).** The window
came up fully opaque, `WS_EX_LAYERED` absent. This is the outcome the tier
design predicted rather than a surprise: DWM composites its backdrop *behind*
the window and this client is painted edge to edge, so the sheet-of-glass
margins have nothing to show through. `MICA_SUPPORTED` flipped to `false` —
the single flag that exists for exactly this — and nothing else changed.

**Tier 2 is not a substitute, and that is a design finding.** At 91 % the
window was rejected on sight: *"trong suốt quá đà, và không có làm mờ nên rất
khó nhìn do xuyên qua."* A uniform alpha is not glass — Mica and Acrylic
*blur* what is behind them, which is what stops the window underneath
competing with the text on top, and `SetLayeredWindowAttributes` only dims. So
every step of visible transparency buys clutter and nothing else. Settled at
250/255 (98 %): a hint of depth, no legibility cost.

**Windows 11 draws its own caption buttons over a reclaimed client.** Zoomed
3× on the top-right strip: the close glyph was **two X's of different sizes
superimposed**, with orange/blue fringing where the two renderings disagreed,
plus a maximise button this design does not draw. `WM_NCCALCSIZE` reclaims the
SPACE; it does not stop DWM furnishing the buttons it believes a `WS_CAPTION`
window needs. Fixed by dropping `WS_CAPTION` for `WS_POPUP`.

**DWM paints the sizing border black without a caption.** A 10 px band of
`(0,0,0)` down the left, right and bottom of a `#15171C` window — the user's
"black box". `DWMWA_BORDER_COLOR` was tried and does **not** reach it (that
attribute tints the hairline around the window, not the sizing border). Fixed
by reclaiming the whole frame in `nccalcsize` and answering all eight resize
directions in `nchittest` — corners first, since a point in the bottom-left
corner is in both the left and bottom strips and answering `HTLEFT` there
costs the diagonal cursor.

**The class background brush flashed light.** `hbrBackground` was
`COLOR_BTNFACE + 1` — the only light thing left in the window's paint path. A
`#B1B1B1` strip 10 px wide appeared down the inside of the left and top edges
and was **gone on a re-measure after the window repainted**: transient, so an
erase rather than a paint. `DefWindowProc` erases newly-exposed regions with
that brush before `WM_PAINT` arrives, and reclaiming the frame made the client
10 px wider on three sides. Fixed with a null brush; `WM_ERASEBKGND` owns the
ground.

---

## Measured NOT fixable under the current constraints

**The ListView header and the three combo faces render light in dark mode.**
`SetWindowTheme` with `DarkMode_ItemsView` / `DarkMode_CFD` was added, built
and re-shot: **pixel-identical, no change.** Those classes are inert until the
*process* opts into dark mode through uxtheme's undocumented ordinals
(`SetPreferredAppMode` #135 / `AllowDarkModeForWindow` #133), which the
2026-08-11 spec rejects.

The calls are kept — harmless, already correct for the day that decision is
reopened, and deleting them deletes the measurement. Three ways out, all
design calls rather than code ones: owner-draw the header (its
`NM_CUSTOMDRAW` path is also not firing, which is a separate real bug),
replace the combos with owner-drawn controls, or reopen the ordinals decision.

---

## Two notes on method, both learned the hard way

**`PrintWindow` cannot see an erase bug.** It asks the window to render itself
fresh, so it showed the window's edges correctly painted in the same minute
the screen showed them `#B1B1B1`. Anything about *what is actually on screen*
has to be sampled from the screen.

**`CopyFromScreen` captured the wrong window twice.** The first probe lost the
foreground after a theme flip and captured a private chat window; both copies
were deleted immediately. A later run captured a different application again
when `SetForegroundWindow` was refused. `PrintWindow` was adopted in between
for exactly this reason, and screen sampling was then restricted to printing
pixel VALUES with no image written. Neither is safe unattended — prefer
`PrintWindow`, and when the screen is genuinely required, sample, do not save.

---

## Still NOT RUN

- **02** custom title bar: 1 px artefact, cross-DPI drag between monitors
- **02b** the eight resize directions and four corners, by hand — `nchittest`
  now owns these and nothing has dragged them
- **03** ListView scrollbar under `DarkMode_Explorer` (may be inert for the
  same ordinal reason as the header)
- **04** `CBS_OWNERDRAWFIXED` typeahead and index reads
- **05** the tick centred in a 22 px row
- **09** eight rows with a 20-row config, no partial ninth
- **10** the toggle still reporting as a checkbox to UIA, and `Space`
- **11** the 15 px vertical slack at a non-100 % DPI — now superseded by the
  compaction pass, and needs re-deriving before it can be checked at all
- **12** whether the hover tint ever renders (`LVM_GETHOTITEM` without
  `LVS_EX_TRACKSELECT`)
- **13** flicker, which is **expected** now that the parent overdraws the
  whole client over unclipped children — the question is how bad, not whether
- **14** the disabled chip's edge against an enabled keycap's, in light mode
