# Four Doors — the beckon-serve settings window, Windows

**Status: design agreed, nothing built.** This document is the input to a
spec-and-plan session. It records what was decided, what was measured, and what
is still a guess — the three are marked and must not be blurred.

**The drawing is `2026-08-14-four-doors-mockup.html`, beside this file.** Open
it in a browser: all four pages, both themes, at the agreed 680 px, with the
exact final wording of every label. Its header comment says what in it is real
and what is only a picture.

Date: 2026-08-14. Supersedes the layout half of
`2026-08-11-windows-settings-window-and-caps-design.md`; that document's
measured Win32 traps all still apply.

---

## 0. The decision in one paragraph

The Windows settings window gains **four tabs in a horizontal owner-drawn pill
strip** below the existing client-drawn title bar: **Shortcuts / Keyboard /
System / About**. The window narrows to **680 px**, defaults to **dark**, the
list **scrolls** instead of growing, and there is **no Save and no Close** —
every valid change is written to `apps.toml` immediately. Everything is
hand-written Win32 + GDI as it is today: no new crates, no new runtime, and the
`cargo check --target aarch64-pc-windows-msvc` loop from macOS is untouched.

Rejected, with reasons in §11: a WebView2/Sciter UI, a two-column master/detail
layout, a left navigation rail, and a fifth "Activity" page.

---

## 1. Why tabs at all

The VKey look — rounded cards, a client-drawn title bar, light/dark palettes
with WCAG-tested tokens, keycap chips, three button tiers — **shipped on
2026-08-13**. What did not ship is information architecture, and that is what
this change is. A second thing falls out of it for free: today a config file
that does not parse makes the *entire* window read-only, including the theme
switch and Start-with-Windows, which have nothing to do with that file. Splitting
by store fixes that as a side effect.

**Split by STORE, not by topic.** Shortcuts and Keyboard write `apps.toml`;
System and About write `HKCU\Software\beckon`, the Run key, or nothing.

---

## 2. The window

| | |
|---|---|
| Size | **680 × 600** at 100 % DPI (was 760 × 600) |
| Minimum | 660 × 560 unchanged — **but see §9 gate G1**, it may need to fall |
| Chrome | extended client, client-drawn 34 px title bar (`chrome.rs`, unchanged) |
| Maximize | still off. `WS_MAXIMIZEBOX` absent; `chrome.rs:99` records that the maximized `WM_NCCALCSIZE` correction is deliberately absent *because* the state is unreachable |
| Backdrop | opaque by default; a transparency slider, see §5 |
| Theme | **dark by default** — a behaviour change, see §5 |

**680 px is affordable, and the reason I first gave for it was wrong.** The
first draft claimed the Caps shorthand paid for the narrowing. It does not: that
toggle is **off by default**, so the window must still fit the full four-chip
chord. The real slack comes from what was cut — the list's column headers, the
editor's field labels, and the Save/Close buttons. Arithmetic:
`680 − 20 (PAD both sides) − 22 (CARD_PAD both sides) = 638` px of card
interior; minus `SHORTCUT_COL 200` leaves ~438 px for the app name, which is
comfortable. **Estimated, not measured** — see §9 G1.

### Palette

**Do not copy the palette into this document.**
`crates/beckon-windows/src/settings_window/theme.rs` is the authority, it
already carries both themes, and it has contrast tests. A second copy is exactly
the kind of duplication that drifts and then looks like a rendering bug. The
mockup's CSS token block mirrors those values for viewing only, and says so.

**Two tokens are new** and must land with their `pairs()` contrast rows *before*
anything draws them:

| Token | Light | Dark | Role |
|---|---|---|---|
| `strip` | `#D9DDE7` | `#2E323D` | the trough the tab pills sit in |
| `strip_hover` | `#CBD1DE` | `#3A3F4C` | hover on an inactive pill |

**In dark mode the trough is LIGHTER than the card, and that is forced by
arithmetic, not taste.** A first proposal of `#101216` measures **1.046**
against `DARK.bg` where the floor is 1.2 — and pure black is only **1.171**, so
no dark trough can pass. Anyone who "fixes" this to a darker value is
reintroducing a failure the contrast test will catch.

**High contrast has no palette at all** — `Theme::HighContrast` returns `None`
from `palette()` by construction, so a literal cannot leak in. Every new surface
resolves through `ThemeCache::col(token, GetSysColor index)`, and **fill and ink
must use DIFFERENT indices**: five invisible-text collisions were found by hand
on the last redesign and by no compiler. Under high contrast a switch also
renders the words On/Off, because a track-and-thumb shape carries no information
once both are system colours.

### The tab strip

Sits **below** the title bar, never inside the caption. VKey put its window
controls in a custom caption with a hard-coded 70 px drag exclusion, so its
admin badge sits inside `HTCAPTION` and can never be clicked; keeping the strip
out of the caption means `chrome::button_rects` stays the one geometry function
for the caption and the strip needs no drag-zone arithmetic at all.

Proposed tokens, **all estimates**: `TABSTRIP_H 36`, `TAB_PAD_X 14`,
`TAB_PAD_Y 2`, `TAB_VISUAL 26`, `FOCUS_SLACK 3`, `TAB_GAP 0` (the perceived gap
is `2 × FOCUS_SLACK = 6 = tok::GAP`).

**Control class: `BS_AUTORADIOBUTTON | BS_PUSHLIKE`, not `BS_OWNERDRAW`.** Two
facts already measured in this source force it: owner-draw never receives
`ODS_HOTLIGHT` (`mod.rs:4375-4385` calls it "the one bit a REAL `WM_DRAWITEM`
never carries"), so there would be no hover state; and owner-draw kills
`BM_GETCHECK`, which is why `WM_CHIP_STATE` had to be invented
(`mod.rs:277-296`). A pushlike radio group also gives arrow-key auto-select and
the UIA RadioButton role for free. Named fallback if the `CDIS_HOT` gate fails:
`BS_PUSHBUTTON + BS_NOTIFY` plus a `BN_SETFOCUS` arm and `TrackMouseEvent`.

The Shortcuts pill carries a **count badge** (`Shortcuts 19`) — the number moved
out of a card heading so it reads from all four pages — and a **warn dot** when
the external-change banner is up on a page you are not looking at.

Navigation: `Ctrl+Tab` / `Ctrl+Shift+Tab` / `Ctrl+1..4`; the strip is ONE tab
stop with Left/Right between pills. **No `&` mnemonics on tab names** — the
collision table in `mod cap` (`mod.rs:457-474`) is hand-maintained with no test,
`A M U C O S E R K T W L D` are claimed, and a duplicate does not fail: it makes
`Alt+<letter>` cycle focus, which reads as a broken keyboard.

---

## 3. The four pages

Wording below is the **exact final on-screen text**. English only.

### 3.1 Shortcuts

The page the window opens on, including — especially — when `apps.toml` does not
parse.

```
┌──────────────────────────────────────────────────────────┐
│  ⌕ Filter                          [ Remove ]  [ Add ]   │
│  ┌────────────────────────────────────────────────────┐  │
│  │ ☐  Brave                     [Ctrl][Win][Alt][ B ] ││ │
│  │ ☐  Windows Terminal          [Ctrl][Win][Alt][ T ] ││ │
│  │ ▸  Discord          in use   [Ctrl][Win][Alt][ D ] ││ │
│  │ ☐  Obsidian         missing  [Ctrl][Win][Alt][ O ] ││ │
│  │ ☐  Claude                    [Ctrl][Win][Alt][ C ] ││ │
│  │ ☐  Telegram Web        [Ctrl][Win][Alt][Shift][T ] ││ │
│  └────────────────────────────────────────────────────┘  │
│  [ Discord            ] [✓Ctrl][✓Win][✓Alt][ Shift][D ▾] │
│                                   [ Record ]  [ Revert ] │
│  ● Another program owns this key. Windows will not say   │
│    which.                                                │
└──────────────────────────────────────────────────────────┘
```

- **No column headers.** Keycap chips look like keys and app names look like app
  names.
- **No `Editing "…"` caption** on the editor card.
- **No field labels** in the editor: the App combo's cue banner reads `App` only
  while empty; the key list sits at the end of the modifier run, where a key
  goes.
- `Reset` → **`Revert`** (names the effect, not the mechanism).
- **Status words**: `paused` > `in use` > `missing` > `other chord`, that
  precedence, from `row_condition`. A healthy row's cell is **empty**.
- **Notes are silent when healthy.** `Registered and working.` is deleted. Notes
  appear only for a condition the status word does not already state.
- The `Win+L is reserved` fact is **not** a bullet anywhere: it is what Record
  says at the moment you press `Win+L`.
- The list is **short and scrolls** (see §4).

### 3.2 Keyboard

```
┌──────────────────────────────────────────────────────────┐
│  Use Caps Lock as a shortcut key                 ( ●══)  │
│  ──────────────────────────────────────────────────────  │
│  Hold [✓Ctrl][✓Win][✓Alt]        Tap [ Caps Lock    ▾ ]  │
│  ──────────────────────────────────────────────────────  │
│  Write shortcuts as [Caps] instead of                    │
│  [Ctrl] [Win] [Alt]                              (══● )  │
├──────────────────────────────────────────────────────────┤
│  ▸  If Caps Lock does nothing                            │
│     ● An admin window has focus — Caps is blocked there. │
│       Type the chord instead; that always works.         │
│     ● Another remapper holds Caps — kanata, PowerToys,   │
│       AutoHotkey.                                        │
└──────────────────────────────────────────────────────────┘
```

- **Three Hold chips and never four.** `Chord` has exactly `ctrl` / `super_` /
  `alt`, because the hook has to release what it presses and releasing Shift
  under the user's fingers lowercases everything they type next.
- `Tap` is a `CBS_DROPDOWNLIST` read and written **by index, never by text**. A
  disabled `CBS_DROPDOWNLIST` still renders white with dark text — measured,
  correct, and must not be "fixed".
- **`Write shortcuts as [Caps] instead of [Ctrl][Win][Alt]` — a toggle, default
  OFF.** The label illustrates itself with real chips. It is a **view
  preference** (`HKCU`), not a binding: `apps.toml` still says
  `ctrl+super+alt+b`, and the editor still shows all four real modifiers. When
  ON, bindings on the caps chord collapse to `[Caps][B]`; bindings on any other
  chord do **not** collapse, which is what makes them visible at a glance — and
  is why the `other chord` status word can go.
- The expander header is **the question the reader arrives with**, so its
  collapsed state is already half an answer.
- **Deleted from this page** by decision: the hook-disclosure line and the
  `[Caps]+[B] → Brave` equation. The disclosure **moves to About**; it is not
  dropped (§3.4).

### 3.3 System

Five rows. No Save is drawn; every row applies on change.

```
┌──────────────────────────────────────────────────────────┐
│  Pause shortcuts                                 (══● )  │
│  Start with Windows                              ( ●══)  │
│                                            [ Reload ]    │
│  ──────────────────────────────────────────────────────  │
│  Dark mode                                       ( ●══)  │
│  Window transparency                96%   ────●────      │
│  ──────────────────────────────────────────────────────  │
│  apps.windows.toml    …\shortcuts\             ↗    ▤    │
│  beckon-serve.log     112 KB                   ↗    ▤    │
└──────────────────────────────────────────────────────────┘
```

- **Pause / Reload call the existing `set_paused` and `reload` the tray already
  calls** — never a parallel implementation. `set_paused` does five ordered
  things a re-implementation gets wrong, including CLEARING `registered`, which
  is what makes the `paused` flag load-bearing everywhere else.
- **Start with Windows is OMITTED, not greyed**, when the process is
  `beckon.exe serve` rather than `beckon-serve.exe` — copy the tray's own
  reasoning.
- **Dark mode is one switch, default ON.** This is a **behaviour change**:
  beckon currently follows Windows. After this it defaults to dark and does not
  ask Windows. If "follow system" must survive, it has to be a third state and
  the control goes back to three.
- **Window transparency is a slider, 85–100 %, default 96 %**, with the
  percentage shown. See §5 for why this reverses an earlier "never a slider".
  Forced off, **with the reason in the control's own slot on the same line**,
  under any of: high contrast, `SM_REMOTESESSION` non-zero, or
  `Themes\Personalize\EnableTransparency = 0`. Never a tooltip — a disabled
  Win32 control does not receive mouse messages, so a tooltip there silently
  never appears.
- **File rows**: the filename identifies the row, so there is no `Config` /
  `Log` label and no `Open` / `Show` words — two glyph buttons with tooltips.
  The log's size is worth showing because `roll_if_oversized` caps the pair at
  10 MiB.
- **Deleted by decision, as rows, with their behaviour kept ON:**
  *Remember size and position* (every app does it; nobody turns it off) and
  *Show error notifications* (the toast IS the answer to "I pressed the key and
  nothing happened"; a switch to disable it is a switch to blind yourself).
- **`Copy diagnostics` deleted**: everything it copied is already on screen —
  version/arch/install path on About with per-row copy, both file paths here,
  registration state in the status line on all four pages. If a one-click report
  is ever wanted, extend `beckon doctor` (which today prints only backend,
  window count and catalog count — **not** version or paths) rather than adding
  a third button here.

### 3.4 About

```
┌──────────────────────────────────────────────────────────┐
│                          ▣                               │
│                    beckon 0.9.3                          │
│  ──────────────────────────────────────────────────────  │
│  Build      aarch64-pc-windows-msvc · 2026-08-13      ⧉  │
│  Location   …\scoop\apps\beckon\current\              ⧉  │
│  Licence    MIT OR Apache-2.0                         ⧉  │
│  ──────────────────────────────────────────────────────  │
│  ● The keyboard hook is installed only while Caps Lock   │
│    is on, or while you are recording a shortcut. beckon  │
│    keeps no record of what you type.                     │
│                                                          │
│      [ GitHub ]    [ Releases ]    [ Report a bug ]      │
└──────────────────────────────────────────────────────────┘
```

- **`Location` is the highest-value row** and must carry the **running image
  path** plus a stale-image verdict. Recorded real failure: a watchdog-started
  beckon ran the 0.8.0 image for three hours while `beckon --version` said 0.9.0
  and the scoop `current` junction pointed at 0.9.0 — **both obvious surfaces
  lied**. Take the path from `current_exe()` and deliberately do **not** resolve
  it through `GetFinalPathNameByHandleW`: resolving now reports today's target,
  which is the thing that lies.
- **The hook disclosure lives here**, moved off Keyboard. An unsigned process
  that holds `WH_KEYBOARD_LL`, calls `SendInput` and writes an autorun key owes
  the user both halves: when it holds the hook, and what it does not keep. The
  second half is a negative claim — no icon, colour or state can draw it.

---

## 4. The list: short and scrolling

The earlier fix in this redesign **uncapped** the list so it grows with window
height (`layout.rs:238` is `let want = list_header_height(..) + row_h *
tok::ROWS;` — **verified**: the list is frozen at 8 rows at *every* window
height, so a 1400 px-tall window still shows 8). That fix stands; the window is
simply shorter now, so fewer rows fit and the scrollbar does the rest.

**Verified**: uncapping contradicts neither CLAUDE.md nor the editor's
`editor_min`/`room` negotiation. CLAUDE.md's commitment reads "does not grow
with the **config**"; after the change `list_h` is a function of the client
rect, the DPI and the row height, and `st.items.len()` never enters it.

`tok::ROWS` should be **deleted**, not kept: the Windows job runs `-D warnings`
and line 238 is its only consumer.

**Open**: with the snap gone, `list_h` stops depending on `row_h`, which makes
`Ui::shown_empty` (`mod.rs:1116-1157`) an inert guard. Keep the whole-row snap
or delete the guard — do not leave a guard that guards nothing.

---

## 5. Two reversals, both deliberate

### 5.1 The transparency slider

An earlier pass in this session said "never a 0–100 slider". **That was
imprecise and is corrected here.** There is no opacity parameter for the
*backdrop material* — `DWMWA_SYSTEMBACKDROP_TYPE` is a five-value enum — and
Mica is **measured dead** on this window (Gate 01 on a14: fully opaque,
`WS_EX_LAYERED` absent, because DWM composites its backdrop *behind* a client
that GDI paints edge to edge). But `SetLayeredWindowAttributes` **does** take an
alpha 0–255 and it works.

So a slider is buildable. It **dims without blurring**, which is why 91 % was
rejected by eye earlier ("trong suốt quá đà, và không có làm mờ nên rất khó
nhìn"). Hence **85–100 %, not 0–100 %**, default 96 %, percentage always shown.
The slider actually answers the old objection: the user picks the level they can
live with instead of someone picking one for them.

### 5.2 Dark by default

beckon currently follows the OS. This makes dark the default and stops asking.
A user on light Windows gets a dark window. Flagged as a behaviour change, not
a tidy-up.

---

## 6. Auto-save — the dangerous part

**No Save, no Close.** Every valid change lands in `apps.toml` on its own. An
adversarial pass found **seven** ways to lose data with this design as first
drafted. The verdict was: *ship it, but not by deleting the button* — what is
lost with Save is not a button, it is a safety net, and the new net has to be
strung before the old one comes down.

### 6.1 Mechanism

1. **Discrete gestures flush immediately.** Chips, the key list, caps, remove —
   all already discrete.
2. **Only the App field is debounced**: `AUTOSAVE_QUIET_MS = 600` (chosen, not
   measured), on a `SetTimer` with a new `IDT_AUTOSAVE = 2` beside the existing
   `IDT_CAPTURE = 1`. `mod.rs:390-391`'s claim that `IDT_CAPTURE` is "the only
   timer this window owns" stops being true and must be edited.
3. **One thread only.** `ServeState` is `Rc<RefCell<..>>` and therefore not
   `Send`; the window is modeless on the tray/hotkey thread. No worker thread,
   no channel.
4. **`KillTimer` before the write, never after** — `apply_settings` can reach a
   `MessageBoxW`, a modal loop that pumps this thread, so a second `WM_TIMER`
   can re-enter mid-write.
5. **The timer writes the MODEL, never `commit_fields()`.** `WM_APP_EDITED`
   already updates the model per keystroke; `commit_fields` re-reads a control
   that is known to rewrite its own edit text after a layout, and a timer doing
   that persists the corruption before the user can see it.
6. **Validity is already solved**: `Model::render` refuses on `Severity::Error`
   and drops unfinished new rows, so a half-typed row simply produces no write.
   The render-failure `MessageBox` must go **silent** — a modal dialog per
   keystroke would kill the feature.

### 6.2 Guards that are preconditions, not polish

| # | Guard | Prevents |
|---|---|---|
| G-a | **Compare-and-swap**: re-read the file immediately before writing; if it differs from `Model::original()`, abandon the write and take the external-change path | Silently overwriting a hand edit made in the ≤1000 ms before the 1 Hz reload tick — and self-write suppression is what would hide it |
| G-b | **Reseed the selection by identity, not index** | A write can reorder the file; a raw index then points the editor at a different binding while the user is still typing into it |
| G-c | **Make a retyped chord an in-place key rename** in `config_write`, not drop-and-reinsert | Fixes the selection swap, the destroyed trailing comment and the lost file position at once |
| G-d | **Do not inject `keyboard.caps` / `keyboard.caps_tap`** into a file that never spelled them while the model holds defaults | The shipped starter config gains two settings the user never chose — and an unknown `keyboard.*` key is a **hard error** on another machine running an older beckon |
| G-e | **Hold the write** while the selected row's app was valid and is now `missing` | `"ctrl+super+alt+c" = "B"` reaching disk and going live as a broken hotkey while the user is mid-word |
| G-f | **Confirm Remove** when it takes more than one row, or when a filter is active | See §6.3 |
| G-g | **Undo is a bounded stack pushed before EVERY write**, with a visible control beside the saved readout | "Close without saving" was a full-file, always-available rollback; one level scoped to Remove does not replace it |
| G-h | **`<config>.bak` written unconditionally at window open**, surfaced on System | Session-level rollback. Note it does **not** protect against G-a's stale-base clobber |
| G-i | **`WM_QUERYENDSESSION` / `WM_ENDSESSION` on the flush list** (the list `end_capture` is already on) | Logoff and a Windows Update restart lose up to 600 ms of typing |
| G-j | **Keep exactly one prompt**: refuse the close when the model is dirty AND the last write failed | A dismissed write-failure dialog turning a whole session of edits into a silent loss |
| G-k | **Carry filter, ticks and selection across an external reload** | "The file wins" costing the user their view state as well |

### 6.3 The bug that is already shipping

**`Remove` under a filter can delete the whole config, today, before any of this
is built.** `Model::visible` (`settings.rs:437-452`) matches the filter against
**both** the app and the combo, and **every beckon chord contains `alt`** — so
typing `a`, a plausible start of "brave", leaves the window *looking* filtered
while showing everything. Tick "the visible rows", press Remove, lose the table.
Measured: four bindings, filter `a`, `control_state` returned all four.

`remove_pressed`'s own doc argues a no-confirm multi-delete is acceptable
because its effect is visible and Save is still a gate. **Auto-save falsifies the
premise.** Fix it regardless of when auto-save lands.

### 6.4 What replaces the command bar

Left: the live service line — `● Serving · 18 of 19` / `⏸ Paused` / a
broken-config phrase — on **all four pages**. The dot is a **drawn GDI
`Ellipse`, never the character `●`**: an em-dash in `serve --log` already came
back as `?"` once, and a text face draws a missing glyph as a box.

Right: a `Saved` readout, plus the **Undo** control from G-g. Wording for the
failure states: `Not saved — finish the row below` (render refused) and
`Not saved — cannot write the file` (I/O). Blank when the config did not parse:
there is nothing to save and nothing to claim.

**`draw_notes` cannot be reused verbatim for that line** — it hard-codes a
`card` ground and the command bar is `bg`. Give it a `ground` parameter.

---

## 7. The seven editing rules

These are the reason the window went from **259 to 83 on-screen words with zero
facts deleted**. Apply them to any row invented later.

1. **A label is a name, not a sentence.** If a control needs a sub-line to be
   understood, the sub-line is a bug report about the label. Cover the sub-line;
   if the label is still ambiguous, the label is the defect.
2. **Silence is the healthy state.** `row_condition`'s rule, promoted from one
   column to the whole window. Text is a symptom.
3. **A fact about this machine is a value, not a sentence.** Paths, sizes,
   counts, versions and OS settings go in a right-hand value slot.
4. **A caveat lives at the failure, not at the setting.** Route it by the
   question the user asks when it bites them, and make that question the
   heading.
5. **A group heading is the word every row beneath it does not repeat.** Under a
   tab called Keyboard, no label says "Caps Lock" again.
6. **A choice nobody turns off is not a choice.** Delete the control, keep the
   behaviour on.
7. **A disabled control explains itself in its own slot** — not a sub-line, and
   never a tooltip, because a disabled Win32 control receives no mouse messages.

### What must never be cut

- **The UIPI limitation, with the half that gets the user unstuck**: an elevated
  window blocks Caps, **and the typed chord still works there** because
  `RegisterHotKey` is not subject to UIPI. A limitation stated without its
  fallback sends the user to uninstall rather than to their keyboard.
- **The other-remapper cause.** beckon cannot detect kanata / PowerToys / AHK; a
  cause that cannot be detected must be named.
- **The hook + `SendInput` disclosure** (now on About).
- **The four status words.** Colour cannot replace them: `in use` and `missing`
  are the same severity but need completely different fixes.
- **The reason a disabled control is disabled.**

### The known risk of this cut

The old draft **shouted the exceptions at everyone**; this one risks
**whispering them to nobody**. The person who needs the UIPI sentence is by
construction *not* on the Keyboard tab — they are looking at the Shortcuts list,
where the row is green and silent because it genuinely **is** registered.
Mitigation, and it is rule 2 read backwards: silence while healthy, but when a
registration actually fails, the status line becomes a one-line dismissible
strip that **arrives uninvited**.

Second risk: nearly every positive confirmation was cut. Someone who changes a
setting and sees the window go quiet has no signal. That is why the `Saved`
readout exists and must flash.

---

## 8. Verified in the source during this session

Cite these; do not re-derive them.

| Claim | Evidence |
|---|---|
| The list is frozen at 8 rows at every window height | `layout.rs:238` — `let want = list_header_height(..) + row_h * tok::ROWS;` |
| A doc comment reasons about the width-critical Caps line from a `MIN_WIDTH` that is **93 px wrong** | `layout.rs:319` says `MIN_WIDTH (753)`; `mod.rs:824` is `const MIN_WIDTH: i32 = 660` |
| Current constants | `mod.rs:702` `WINDOW_WIDTH 760`; `mod.rs:824/825` `MIN 660/560`; `layout.rs:66` `tok::ROWS 8` |
| Adding a `Callbacks` field is a **hard E0063 on macos-latest** | `beckon-macos/examples/settings_probe.rs:112` builds the struct as a complete literal with no `..`, and CI builds it with `--all-targets` |
| `settings_window::open` has **two** macOS call sites, not one | `beckon-macos/src/settings_window.rs:615` and `examples/settings_probe.rs:194` |
| The filter matches the Shortcut column too, and every chord contains `alt` | `settings.rs:437-452`; measured with four bindings and filter `a` |
| `Model::render` renders from a **snapshot**, not the file | `settings.rs:730` renders from `self.original` |
| The watcher does not reload promptly — it drains on a 1 Hz tick | `serve.rs:549-570` sends on a channel; `serve.rs:365-374` drains |
| `config_write` unconditionally writes `keyboard.caps` / `caps_tap` | `config_write.rs:75-94`; the shipped starter template has them commented out on purpose |
| Owner-draw buttons never receive `ODS_HOTLIGHT` | `mod.rs:4375-4385` |
| `beckon doctor` on Windows prints backend + window count + catalog count only | `beckon-cli/src/lib.rs:519+`, Windows arm |
| Mica is dead on this window | Gate 01, a14: fully opaque, `WS_EX_LAYERED` absent |

---

## 9. Hardware gates (a14, Windows 11 ARM64)

Every visual observation goes through a scheduled task in **session 1** —
an SSH shell is session 0 and `EnumWindows` there sees nothing. Register with
`New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries -Priority 4` (**both**
flags). Set DPI awareness before measuring any rect. **Every gate needs a
control** — a blind probe and a clean result look identical without one.

| # | Gate | Control that makes it trustworthy |
|---|---|---|
| G1 | Does `Use Caps Lock as a shortcut key` fit at 680 px, at 96 and 144 DPI? `GetTextExtentPoint32W`, not estimation | The same measurement at 760 px, where it is known to fit |
| G2 | Does `CDIS_HOT` reach a `BS_PUSHLIKE` radio? | An ordinary `BS_PUSHBUTTON` in the same run, which does get it |
| G3 | ~~Is the client rect the window rect?~~ **Settled by reading, 2026-08-14 — confirm only.** Does the invisible ~8 px resize strip, now painted like ordinary client, read wrong to a person? | `GetClientRect` and `GetWindowRect` logged side by side in one run — `measure_geometry` already does this and asserts a 0 px top inset |
| G4 | Does `IsDialogMessageW` dispatch Enter to a hidden-but-enabled default button? | The documented disabled case, which is known not to dispatch |
| G5 | Auto-save round trip: type, wait 600 ms, confirm one write and no reload storm | A build with the debounce disabled |
| G6 | The 11 gates the 2026-08-13 pass left unrun — **4 must clear**, 2 are superseded, 5 written off by name | as recorded there |

**G3, corrected 2026-08-14.** The row read *"Is the client rect the window
rect? `chrome::nccalcsize` returns `LRESULT(0)` without `DefWindowProcW`, but
`mod.rs:770-776` still subtracts an 8 px bottom frame"*. The contradiction was
real and is the reason it was a gate; it is now resolved, and not in the
direction the second half implied. `chrome::nccalcsize` reads neither
parameter and never calls `DefWindowProcW`, so the proposed window rect
survives untouched as the client rect on all four edges — client == window.
The 8 px described the handler as it stood before `c523e8e` (2026-08-13,
"reclaim the whole frame, and hit-test the eight resize edges"), and every
copy of it in the crate has been retired against that reading. Nothing here
needs hardware to decide any more; the run is confirmation. What is left is a
question the reclaim created rather than answered: `chrome::nchittest` still
treats a ~8 px strip along each edge as a resize direction, and that strip is
now painted like ordinary client area — the window ground and the title-bar
band run under it, and the outermost column of the Close button falls inside
`HTRIGHT`. Whether that reads wrong is a look question, and only a person can
answer it.

Note: **a14 cannot be rebooted unattended into a signed-in state**; plan on a
person being present. Kill `beckon*` before `cargo build` or the link fails on
the locked exe **and leaves the old binary in place**, so the next measurement
silently tests stale code.

---

## 10. Not in scope

- A scrolling **content area** on any page (the shortcut *list* scrolls; the
  pages do not). New machinery, and every scroll step re-places controls, which
  is a `layout()` call site the design does not permit.
- `SysTabControl32` — ignores `SetWindowTheme(DarkMode_Explorer)`, its
  `TCM_ADJUSTRECT` is a second copy of the layout arithmetic, and its idiomatic
  form breaks `GetDlgItem`.
- New `Alt` mnemonics on any new control, until a mnemonic-uniqueness `#[test]`
  lands (~0.25 d, same shape as the id test).
- Renumbering ids `1001-1008`, `1012`, `1013`, `1025`, `1028-1031`, or
  reclaiming `1009-1011`.
- Regridding `CTL 26` / `ROW_H 22` / `CARD_PAD 11` — `ROW_H` in particular is
  the lever fed to `ImageList_Create`, so moving it changes the tick's cell,
  which is the delete path.
- Any caption that changes with data (`Remove 3`): `layout` sizes buttons from
  their caption, so a live count makes the caption a `layout` input, and
  `layout` on a data push is the measured data-loss path.
- Vietnamese UI text; a keystroke history of any kind; an opacity slider below
  85 %; maximize.

---

## 11. Rejected, and why

| Direction | Why it lost |
|---|---|
| **Glass** (WebView2/wry) | 42 days with no partial ship, plus an uncosted code-signing project. The macOS cross-check loop **measured broken** — a `wry`-only crate SIGKILLs on this Mac. 3 125 lines of live cross-process probes become unwritable. Scoop cannot express a WebView2 runtime dependency for the x86_64 build that exists for Windows 10 machines |
| **Bench** (two columns, VKey's shape) | Fails its own test: the tab design already shows the whole list while you are *on* Shortcuts. What a permanent column buys is the list visible on the three pages where it is irrelevant. Costs: App column 485 → 260 px so long PWA names truncate, `MIN_WIDTH` 660 → 780 on the axis the code proves is scarce, and the editor rebuilt from a 3-line strip into a 6-line stack |
| **Side rail** | A rail's only structural advantage is scaling past overflow; four items never overflow. 124 px — 16 % of the width — on the scarce axis: `layout.rs` clamps five widths and flexes exactly one height |
| **Five Doors** (an Activity page) | Deferred, not rejected. Revisit after the four-tab window has been used. Its keystroke ring is permanently out |

---

## 12. Still open

1. `MIN_WIDTH` at 680 px — G1 decides whether the minimum must fall below 660.
2. Keep the whole-row snap, or delete `Ui::shown_empty` (§4).
3. Where `Page` lives. If it goes in `beckon-core`,
   `DefaultButton::visible(external_change, page)` becomes testable on all three
   CI jobs, which is the stated reason `DefaultButton` is in core at all.
4. What the macOS settings window does with the new `ControlState` fields. Both
   compile there for free; whether macOS should *draw* the service line is a
   cross-platform call nobody has made.
5. Whether `SetWindowPos` on a populated `CBS_DROPDOWN` also raises
   `CBN_EDITCHANGE` — recorded nowhere; `combo_probe` would settle it.
