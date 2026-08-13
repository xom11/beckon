# The modifier chips on a14, and five gates that had never been run

Date: 2026-08-13. Machine: a14, Windows 11 Home build 26200, ARM64, one
display at **144 DPI**. Binary: `target\release\beckon-serve.exe` built from
`012ef93` / `05e9c80`, whose settings-window code is byte-identical to what
shipped as **v0.9.2** — the only later commit was the version bump and a
`CLAUDE.md` correction.

Everything below ran through a **scheduled task in session 1**, registered
with `-LogonType Interactive` and
`New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries -Priority 4`. An SSH
shell is session 0 and cannot see session 1's desktop at all, so every result
taken there is a confident false negative. Scripts live in files under
`C:\Users\kln\hwpass\` and run with `-File`, because quoting through
`ssh` → PowerShell → `powershell.exe` is eaten.

## What was measured

| Gate | Verdict | How |
|---|---|---|
| Chips render as keycaps | **PASS** | screenshot, armed and disabled |
| Chips drive the model | **PASS** | `settings_probe`, `BM_CLICK` per chip |
| `WM_CHIP_STATE` cross-process | **PASS** | answered from PowerShell and from the probe |
| **G1** — 96 DPI | **PASS** | `__COMPAT_LAYER=DPIUNAWARE`, no code change |
| **G3** — `customdraw_probe` | **TICK_SURVIVES** | with a working control |
| **G4** — §F.4 by hand | **PASS** | `keybd_event` + `mouse_event` together |
| **eye3** — `&` in an app Name | **PASS** | `Editing "Notes & To Do"` |
| **eye4** — the `comma` cap | **PASS** | the cell says `,` |
| High contrast | **PASS** | theme toggled and restored, restore verified |

## The chips

Two screenshots, one with nothing selected and one with a row selected,
answer the visible half:

- **Armed** chips take `COLOR_HIGHLIGHT` with `COLOR_HIGHLIGHTTEXT` — the
  same accent the selected row four pixels away uses.
- **Disabled** chips (no row selected) draw the keycap outline in
  `COLOR_GRAYTEXT` with no fill. They read as keys that cannot be operated.
- The mnemonic underline on the three `Hold` chips is **absent** until Alt is
  pressed, which is what `WM_QUERYUISTATE`'s `UISF_HIDEACCEL` is for and is
  what every real control beside them does.

`settings_probe` drives the functional half, through `BM_CLICK` on each chip
so the click reaches the button's own wndproc rather than a synthesised
`WM_COMMAND`:

```
chip state: readable (WM_CHIP_STATE answered)
Shift with no key: list "" MATCH
a key alone:       list "A" MATCH
+ctrl:             list "Ctrl + A" MATCH
+super:            list "Ctrl + Win + A" MATCH
+alt:              list "Ctrl + Win + Alt + A" MATCH
+shift:            list "Ctrl + Win + Alt + Shift + A" MATCH
per-step agreement: shortcut controls PASS (0 wrong, 0 slow)
```

### The probe's own bug, and why it is written down

The first run reported **every chip unreadable while the window was
working** — the list cell walked correctly to `Ctrl + Win + Alt + Shift + A`
while the probe's expectation stayed `A`. `chip_armed` was sending
`WM_CHIP_STATE` to the **chip**, and the message is answered by `wndproc`
with the id in `WPARAM`; addressed to the button it reaches comctl32's
BUTTON, which does not know it and returns 0 — the same answer an older
`beckon-serve` gives.

This is the reason the reply is `0` / `1` / `2` rather than a bool, and the
reason `chips_readable` is asked once before any chord is printed. Without
that line the probe would have gone on printing chords, all of them missing
their modifiers, all of them plausible. **It went red on a real defect the
first time it ran**, which is the only evidence that it is load-bearing
rather than decorative.

## G1 — 96 DPI, without touching the machine's scaling

a14 has one display at 144 DPI and nobody had ever seen this window at 96.
Changing the desktop's scale is intrusive and needs a sign-out. The
app-compat shim does it per-launch instead:

```powershell
$env:__COMPAT_LAYER = 'DPIUNAWARE'
Start-Process -FilePath $exe -ArgumentList (...)
Remove-Item Env:\__COMPAT_LAYER
```

`GetDpiForWindow` then answers **96**, `layout` computes every position at
96, and the process rasterises its fonts at 96 — DWM upscales the finished
bitmap, so the capture is blurry but every geometry decision in it is the
real 96-DPI one. Nothing clipped, nothing overlapped, no horizontal scroll
bar; the chip row, the key list and `Record`/`Reset` all fit on line 2.

Chip rects at 96 (physical pixels, read from a PER_MONITOR_AWARE_V2 probe,
so 1.5× the logical figure): `Ctrl` 69, `Win` 72, `Alt` 62, `Shift` 77 —
against 68 / 73 / 62 / 78 at 144 DPI, i.e. the same ~46 logical px. The
layout is DPI-consistent.

**What this does not cover**: a genuinely 96-DPI panel would show it at 1:1
instead of upscaled. That is a display property, not a layout property, so
G1's question is answered; its sharpness is not.

## G4 — the gate that was written up as un-probeable

The handoff records G4 as needing "a keyboard and a mouse at once, so no
probe can do it". **`keybd_event` and `mouse_event` can do both**, and
`capture.rs:408` ignores only strokes carrying beckon's *own* `dwExtraInfo`
marker — an injected stroke from another process is treated as real. So:

1. `keybd_event(VK_CONTROL)` down, and held.
2. `SetCursorPos` to `Record`'s centre, `mouse_event` LEFTDOWN/LEFTUP.
   Caption flipped to `Stop`, notes read *Press the shortcut.*
3. `Alt` down, `T` down/up, `Alt` up — Ctrl still held throughout.

| | Ctrl | Win | Alt | Shift | key |
|---|---|---|---|---|---|
| before | ARMED | ARMED | ARMED | clear | index 19 (`t`) |
| after | **ARMED** | **clear** | **ARMED** | clear | index 19 (`t`) |

`ctrl+alt+t`, which is §F.4 exactly: Ctrl's key-down happened before the hook
existed and still reached the chord, while Win — which was not held —
correctly dropped out.

**The screenshot is the load-bearing half, not the table.** `WM_CHIP_STATE`
and `draw_chip` both read the same `CHIPS` word, so a failed
`InvalidateRect` would leave the message saying ARMED over a stale screen.
The capture shows `Win` genuinely un-filled and the list cell redrawn to
`Ctrl | Alt | T`, so `set_chip`'s repaint really ran. That path —
capture → `check` → `set_chip` → `WM_DRAWITEM` — has no other coverage.

## G3 — `customdraw_probe`, built 2026-08-12, first run today

```
FOREGROUND_OK=true
SUBJECT_INK=306
CONTROL_INK=306
VERDICT=TICK_SURVIVES
```

`CDRF_SKIPDEFAULT` on subitem 0 does **not** take the `LVS_EX_CHECKBOXES`
state image with it, and the control row proves the capture works. §B.6's
`app_cell` is unblocked. This gates nothing that shipped in 0.9.2.

## High contrast

Toggled with `SPI_SETHIGHCONTRAST` + `SPIF_SENDCHANGE`, which is also what
makes beckon's own `SPI_SETHIGHCONTRAST` arm run. Flags `0x7e` → `0x7f` →
restored to `0x7e`, **and the restore was read back rather than assumed**.

Every keycap switches to the hard-rectangle branch: no rounded corners, no
bottom edge. Armed chips are cyan `COLOR_HIGHLIGHT` with black text; unarmed
are black `COLOR_BTNFACE` with a white border. All readable.

One honest loss, inherent to the palette rather than to the code: in the
Shortcut column the "main key is brighter" rule disappears, because
`COLOR_WINDOW` and `COLOR_BTNFACE` are both black under a high-contrast
theme. The per-cap borders still separate them.

## The one decision that still wants eyes

**A disabled chip takes no fill, so it stops saying which way it is set.**
For the four editor chips nothing is lost — they are disabled only when no
row is selected, when they are all clear anyway. For the three `Hold` chips
it is a real loss: they are greyed whenever Caps is off while still
describing what Caps *would* do, and a greyed check box keeps showing its
tick.

Every alternative considered needs a colour pairing the system palette does
not define (accent-on-grey, `COLOR_GRAYTEXT` on `COLOR_HIGHLIGHT`), and this
window's rule is that every colour comes from `GetSysColor`. Left as the
handoff specified. Worth a human glance before it is called settled.

## Round two: the design board, fetched back

The chips shipped and still did not look like B. The board's URL
(`claude.ai/code/artifact/3aaeb923-…`) is not fetchable from here -- the
page is a private SPA shell and a plain fetch returns 0.1 KB of nothing --
but **the HTML it was published from was still in the previous session's
scratchpad**, and reading `.wtog` off it settled the question in one step.

The colour was never wrong. `COLOR_HIGHLIGHT` *is* the accent. Six numbers
were, five of them borrowed from the column's rule rather than the chip's:

| | `.wtog` | shipped 0.9.2 |
|---|---|---|
| height | 28 (fills the control) | 27, capped by the column's 19 |
| padding | `0 10px` | 5 |
| min width | 46 | none, so `Alt` was visibly smaller |
| bottom edge | `2px`, following the radius | 1 px hairline inset 2 px each side |
| resting face | `#fafafa` on a `#f3f3f3` window | `COLOR_BTNFACE`, **the window's own colour** |
| armed edge | `#1d4fc4` under `#2563eb` | none |

The fifth is most of it: an unarmed chip was a grey box on a grey surface. B
makes the key LIGHTER than what it sits on, which is how a keycap catches
light. `COLOR_WINDOW` for the face and `shade(COLOR_HIGHLIGHT, 4, 5)` for the
armed edge -- derived, so a green accent gets a green shadow.

**One correction from looking at the result**: the light face was given to
disabled chips too, and three greyed `Hold` keys became the most prominent
thing in the band. Disabled sinks back to `COLOR_BTNFACE`; only the ink
fades. `.wtog.dis` says the same thing in CSS (`#f7f7f7` on `#f3f3f3`).

Bold on an armed chip (`font-weight:600`) is the one `.wtog` property not
implemented: it needs a fourth `HFONT` and a measurement that can disagree
with `layout`'s, for the smallest of the deltas.

## Round three: the flag pills, the accent Save, the count

- **`· N bindings`** beside the heading, a second STATIC because one has one
  font, greyed through a `WM_CTLCOLORSTATIC` arm scoped to that id alone.
- **`Save`** is accent-filled through **`NM_CUSTOMDRAW` on the button**, not
  `BS_OWNERDRAW`. Owner-draw replaces a button's TYPE, and Save's type is
  `BS_DEFPUSHBUTTON` -- the ring `set_default_id` moves with a `BM_SETSTYLE`
  read-modify-write. Custom draw leaves the type, the notifications and the
  ring untouched and replaces only pixels.
- **Flag pills**: `key in use` red, `not installed` amber.

### `TICK_SURVIVES` did not mean what it was read to mean

G3 said `CDRF_SKIPDEFAULT` on subitem 0 keeps the check box, so the first
attempt took the cell over. **Measured in this window: every flagged row lost
its check box, and the selected row lost its keycaps too.** The probe builds
a ListView of its own with no owner-drawn neighbours; what it proved is
narrower than what it was read to prove. A green probe is a measurement of
the probe's window.

Two separate faults, and the second is the instructive one:

1. `LVM_GETSUBITEMRECT` with a subitem of **0** answers for the whole ITEM,
   every column -- so the fill erased the Shortcut column of the row it was
   drawing.
2. Owning subitem 0 at all costs the state image here.

`CDRF_NOTIFYPOSTPAINT` removes the question: comctl32 draws the tick, the
selection, the ellipsis and the text, and the pill is laid over the flag's
characters afterwards. Nothing this window draws can now cost a tick, which
is the delete path.

### Severity could not pick the colour

`key in use` and `not installed` are **both `Mark::Bad`** -- each pushes a
`Bad` note -- while the design draws one red and the other amber. So the
`lParam`-carried `Mark` was removed again and `beckon_core::settings::
flag_tone` decides, keyed on the flag word. That is not a second opinion
about severity; it is a property of a closed four-word vocabulary, and it
lives beside `FLAGS` with a test asserting the two `Bad` flags do NOT share
a tone.

Pill colours are the **only** literals in this window, because the system
palette has no `COLOR_WARNING`. High contrast and selected rows both fall
back to comctl32's own text -- verified: under a high-contrast theme the
pills vanish and the plain words remain.

## Machine state at close

- Scoop updated 0.9.1 → 0.9.2. **Verified against the trap**: the running
  process started 10:36:16, the `0.9.2` install directory was created
  10:36:08 — the process is 8 s newer than the image, so it is not a
  pre-update survivor. `--version` and the `current` junction were not
  trusted for this.
- `BeckonServeWatchdog` triggered by hand rather than waited on; serve is up
  on the real config with **18 of 18 registered**.
- `apps.windows.toml` is byte-identical to git HEAD (`git status --porcelain`
  is silent on it). Its mtime moved to 10:34:28 during the session and
  **that is unexplained** — no settings window was open on that file after
  10:28, and nothing here writes it. Recorded rather than guessed at; the
  content is unchanged, so nothing was lost.
