# beckon-serve settings window: foundation, redesign, Caps Lock, suggestions

Date: 2026-08-11
Status: design approved, not built
Supersedes: §A.5 *Layout* of
`2026-08-11-windows-settings-window-and-caps-design.md`. Everything else in
that spec stands; this one adds to it.

## Motivation

The settings window shipped and works. It is also 986 lines of correct Win32
that renders like Windows 2000, and the cause is not in that file: the binary
has no application manifest, so Windows loads comctl32 v5 for the process and
treats it as DPI-unaware. Every visual complaint traces back to that one
fact.

On top of the fix, three things the window's own target user — a
non-developer — needs and does not have:

1. **The chord is fixed at `ctrl+super+alt`** and every binding is a whole
   string typed by hand. There is no reason a person should type
   `ctrl+super+alt+` eighteen times.
2. **Every row shouts.** The status column says `OK` on healthy rows, so the
   one row that needs attention does not stand out.
3. **Nothing helps you find an app.** The right-hand side of a binding is a
   Name, and the only way to learn a Name is `beckon search` in a terminal —
   which is exactly the tool this window exists to replace.

## Decisions taken (and the alternatives rejected)

| Question | Decision | Rejected |
|---|---|---|
| Toolkit | Raw Win32, unchanged | egui / iced / Slint / WinUI 3 (see §7.1) |
| Dark mode | Light only, `GetSysColor` throughout; high-contrast themes are the supported dark path | uxtheme ordinals; IAT-patched scrollbars (§7.2) |
| UI language | English | Vietnamese; an i18n layer |
| Config format | **Unchanged.** Combos stay spelled out in full | A `beckon+t` shorthand token (§7.3) |
| New config keys | Exactly one: `keyboard.caps_hold` | Per-OS overrides; a `[keyboard]` header |
| Caps Hold chord | Configurable; `shift` refused because the hook must press and release it | Freezing the hook's chord (§7.4); allowing shift unconditionally |
| Shortcut editing | Full combo. Modifier checkboxes + a `CBS_DROPDOWNLIST` of the 81 keys as the primary path, **plus chord capture** via the shared LLHOOK as an accelerator (Part F) | A single-key field; a dropdown-only field with no capture; a capture-only field with no typed path |
| Capture mechanism | The existing `WH_KEYBOARD_LL`, one hook with two mutually exclusive modes | `msctls_hotkey32`; a second hook; message-queue capture |
| Chord validation | Live: F12 guard → own-table check → a real `RegisterHotKey` probe | A static table of reserved chords |
| Shortcut list | Fixed height, internal scroll, per-row checkboxes for multi-delete | Growing with the row count; multi-select instead of checkboxes |
| Layout | Five horizontal bands, app-name first, in-line editor | List + detail pane; a scrolling card column; an editable grid |
| Suggestions | A chip row in the window, from taskbar pins + open windows | Seeding the starter file; a first-run wizard; UserAssist (§5) |

## Landing order

Three landings, and the order is load-bearing rather than a preference.

| Landing | Contents | Why here |
|---|---|---|
| **1** | Part A entire, plus Part D | The manifest changes what every constant in Part B *means*, so tuning spacing or fonts before it is on hardware is tuning against metrics the shipped binary will never use. Part D rides along because §D.1 is a live defect and its measurement runs on the same a14 pass. |
| **2a** | Part B, plus Part C, plus §F.7 (the list) | The window redesign and the Caps row touch the same `layout` and the same `ControlState`; splitting them means writing the layout twice. §F.7's one-line `suppressed()` fix lands **first within 2a**, before any code writes list item state — it is abort-class. |
| **2b** | §F.1–F.6, F.8 (capture, the probe, the beckon-key row) | Capture needs the `caps_hook` lifecycle refcount, whose only consumer is here. Gated on a14 measurements 1, 2 and 4 (§F.5, §F.6) — this is the hardware measurement the 2026-08-10 spec demanded and that the window shipped without, because the feature needing it was not built. It is being built now, so the debt is due. |
| **3** | Part E | Suggestions depend on nothing above and are the easiest thing to cut. |

**§B.7's App-combo-box fix belongs in landing 2, ahead of Part E**, and the
sequencing is deliberate: the pain suggestions are aimed at ("I had to type
`Windows Terminal` and did not know that was the Name") is the same pain a
working autocomplete solves, for a quarter of the code. Ship the combo box
fix, then decide whether Part E is still wanted.

**Read that with §7.15 beside it.** "A working autocomplete" is written here
as if the control had a broken one. It does not have one at all — a
`CBS_DROPDOWN` never autocompletes, measured — so this sentence is a case for
*adding* type-ahead, not for repairing it, and §B.7's fix does not deliver it.
Part E is therefore still open on its own merits.

---

# Part A — the foundation

Land this alone, first. Every spacing and font constant in Part B is tuned
blind until this runs on hardware.

## A.1 The manifest

`crates/beckon-cli/beckon.rc` gains one line beside the existing icon:

```
1 ICON "../../assets/beckon.ico"
1 24 "beckon.exe.manifest"
```

`24` is `RT_MANIFEST`; id `1` is `CREATEPROCESS_MANIFEST_RESOURCE_ID`, which
is the correct id for an EXE. `build.rs` gains a matching
`cargo:rerun-if-changed=beckon.exe.manifest` for the same reason the icon has
one: `embed-resource` emits no rerun annotation of its own.

The manifest declares, in this order:

- `Microsoft.Windows.Common-Controls` version `6.0.0.0` as a dependent
  assembly — visual styles.
- `<dpiAware>true/pm</dpiAware>` **followed by**
  `<dpiAwareness>PerMonitorV2</dpiAwareness>`. The order is load-bearing:
  older Windows reads the first and ignores the second, newer Windows lets
  the second win. Reversing them silently loses per-monitor v2 on Windows 10.
- `supportedOS` GUIDs for Windows 10/11.
- `activeCodePage` = `UTF-8`, `longPathAware` = `true`.
- `trustInfo` / `requestedExecutionLevel level="asInvoker"`. beckon must stay
  at normal integrity; the Caps hook's UIPI gap is documented, not fixed.

**rustc passes no manifest flags.** The MSVC target's `pre_link_args` is
`["/NOLOGO"]`, so there is nothing to conflict with and no `/MANIFEST:NO` is
needed. `LNK4078` is a warning about duplicate section names and is unrelated
to manifests; do not add guards for it.

`supportedOS` disables the version-lie shim, so `GetVersionEx` /
`VerifyVersionInfo` would start reporting the real version. Grep before
landing; the expectation is zero hits.

### A.1.1 The verification gate is not optional

**`embed-resource` 2.5.2 swallows resource-compilation failures silently.**
A missing icon is visible in Explorer within seconds; a missing manifest is
invisible until someone looks at a 150 % display. Either bump to
`embed-resource` 3 and call `.manifest_required().unwrap()`, or add a
release-workflow step that reads `RT_MANIFEST` back out of the built `.exe`
and fails if it is absent. One of the two must exist before this lands.

### A.1.2 What v6 breaks

- **`CB_SETMINVISIBLE` becomes necessary.** Under comctl32 v6 a combo box's
  drop-down height is no longer governed by the `cy` passed to
  `CreateWindow`/`SetWindowPos`; it is governed by the minimum-visible-items
  count, default 30. `settings_window.rs:590` passes `row * 8` for exactly
  that purpose and stops working.
- Themed `EDIT` / `BUTTON` / `COMBOBOX` have different border and padding
  metrics. Every height constant in `layout` must be re-measured, not
  adjusted by eye.
- `activeCodePage=UTF-8` applies to `beckon.exe` as well, since `beckon.rc`
  is embedded into every binary in the package. This does **not** relax the
  rule that `serve` log messages stay ASCII: Windows PowerShell 5.1's
  `Get-Content` still defaults to ANSI.

## A.2 DPI, for the first time

Today `GetDpiForWindow` returns a hard 96 because the process is
DPI-unaware, so `let s = |v| v * dpi as i32 / 96` at
`settings_window.rs:524-525` is the identity function on every machine ever
tested. The comment above it describes behaviour the process cannot have.
Enabling `dpiAwareness` changes every number at once, and four sites must
change with it:

1. **`WM_DPICHANGED` gets its own arm.** It is currently folded in with
   `WM_SIZE` (`settings_window.rs:809-812`), calls `layout` and returns,
   discarding `lParam`. It must instead read the new DPI from
   `HIWORD(wParam)`, rebuild the font, broadcast `WM_SETFONT` to every child,
   and `SetWindowPos` to the suggested `RECT` in `lParam`.
2. **The font must be per-DPI and must be freed.** `ui_font()` uses
   `SystemParametersInfoW(SPI_GETNONCLIENTMETRICS)`, which is the wrong API
   for a per-monitor process; it becomes
   `SystemParametersInfoForDpi(..., GetDpiForWindow(hwnd))`. The old `HFONT`
   must be `DeleteObject`ed — today one is leaked per window open.
3. **The creation size must be scaled.** `CreateWindowExW` passes literal
   `760, 560`, which under PerMonitorV2 are physical pixels. On a 192-DPI
   display the window comes out half the intended size and no
   `WM_DPICHANGED` arrives to correct it. `MulDiv` in `WM_CREATE`.
4. **The ListView columns must go through `s()`.** `(34, 190, 150)` at
   `settings_window.rs:334-352` are raw literals.

## A.3 Layout floor

- **`WM_GETMINMAXINFO`.** The window has `WS_THICKFRAME` and no minimum. Drag
  it below ~274 px of client height and the notes `STATIC` receives a
  negative `cy`; below ~170 px the list does. Set the floor and clamp every
  computed height at zero regardless — a floor is a promise about the frame,
  not about the arithmetic.
- **The default size already overflows.** Columns total 34+190+150 = 374 px
  inside a list pane that is `(744-30)*45/100` = 321 px wide. beckon ships
  with a horizontal scroll bar and a clipped App column. Part B's full-width
  table removes this structurally; until then it is a real defect.
- **`WNDCLASSW.hIcon` / `hIconSm` are null**, so the title bar, taskbar and
  Alt-Tab show the default icon while the tray shows beckon's. Five lines.

---

# Part B — the window

## B.1 Shape: five bands, not four cards

```
┌ beckon — shortcuts.toml ──────────────────────────── ─ □ ✕ ┐
│ ☐ Caps Lock  Hold [Ctrl][Win][Alt][Shift]  Tap [Caps Lock ▾] │  1  a setting
├─────────────────────────────────────────────────────────────┤     about the
                                                                    list below
│  [banner: file changed on disk    (Reload) (Keep mine)]     │     only when needed
│  Shortcuts              [🔍 Filter] [Remove 2] [+ Add]      │  2
│ ┌─────────────────────────────────────────────────────────┐ │
│ │ App                                          Shortcut   │ │
│ │ ☐ Windows Terminal                    Ctrl Win Alt  T   │ │  3  fixed height,
│ │ ☑ File Explorer                       Ctrl Win Alt  E   │ │     scrolls inside
│ │ ☐ Claude        ⚠ not installed       Ctrl Win Alt  C   │ │
│ │ ☐ Telegram Web  custom          Ctrl Win Alt Shift  T   │ │
│ └─────────────────────────────────────────────────────────┘ │
│ [Claude ▾] + ☑Ctrl ☑Win ☑Alt ☐Shift [c ▾] [●Record][Reset]  │  4
│  ⚠ No installed app is named "Claude".                      │
│  Suggested  [+ VS Code] [+ Brave] [+ Notion] [+ Spotify]    │  5
├─────────────────────────────────────────────────────────────┤
│  [Open config file]                        [Close]  [Save]  │
└─────────────────────────────────────────────────────────────┘
```

Three decisions carry this layout, and each replaces something specific:

**App name first, chord right-aligned.** The old order led with an unnamed
34 px status column and put the app last. People scan for "what key does
Claude have", not "what app owns C". This is the shape Raycast's settings
list uses, and it is why the filter box earns its place: with 18–20 bindings,
filtering is the difference between reading and scanning.

**Healthy rows say nothing.** The status column is deleted. A row with a
problem carries a short flag next to the app name (`⚠ not installed`,
`⚠ key in use`, `paused`, `custom`); a healthy row carries nothing. Today
every row says `OK`, which is noise that hides the one row that matters.

**The editor is one line, directly under the list, laid out to mirror a
row.** The detail card is gone. This is Raycast's "edit where you are
looking" without the in-place-grid cost the original spec correctly refused:
overlaying an `EDIT` on a ListView subitem means hand-handling horizontal
scroll, column resize, Tab-to-next-cell and the ambiguous click-outside, for
400–600 lines. A strip below the list is a fixed set of controls positioned
by the same `layout` function as everything else.

The chord renders as keycaps with the beckon-key modifiers dimmed and the
variable key emphasised, so it is readable without knowing that `super` means
the Windows key.

Full-width columns computed as a proportion of the list width make the
overflow defect in §A.3 structurally impossible.

## B.2 Spacing, on a 4 px grid

Current spacing is `pad = 10` plus scattered `s(6)` and `s(4)` corrections.
Nothing lands on any grid, and one of those corrections makes a label overlap
its own field by 6 px — invisible today only because `SS_LEFT` top-aligns the
text, and revealed by any font change.

Every value is a named token and every token passes through `s()`:

| Token | Value at 96 DPI |
|---|---|
| Surface padding | 16 |
| Band gap | 14 |
| Control gap | 8 |
| Label → control | 12 |
| Control height | 32 |
| ListView row height | 32 |
| Minimum button width | 88 |
| Window creation size | 860 × 560 |
| Minimum window size | 720 × 460 |

Columns: `App` takes the remaining width, `Shortcut` is a fixed 200 at 96 DPI,
both scaled.

## B.3 Type: three roles, probed not assumed

Every control currently gets `lfMessageFont` — one size, one weight, no
hierarchy at all.

| Role | Size | Used for |
|---|---|---|
| Subtitle | 20, semibold | band headings |
| Body | 14 | list, fields, buttons |
| Caption | 12 | flags, notes, the suggestion row |
| Keys | mono, 12 | keycaps and combos |

Two measured facts constrain this:

- **`SPI_GETNONCLIENTMETRICS` still returns plain `Segoe UI` at 9 pt on
  Windows 11.** Segoe UI Variable reaches the shell through DirectWrite and
  XAML, never through `NONCLIENTMETRICS`. A Win32 app must ask for it by
  name.
- **`Segoe UI Variable` exposes TWELVE GDI family names, not three and not
  nine.** Measured on a14 2026-08-11, correcting an earlier draft of this
  line that said nine: it is {Small, Text, Display} × {—, Light, Semibold,
  Semilight} — the Semilight axis was missed entirely. And **the truncation
  to 31 characters is not uniform**, which is the part that bites:
  `Segoe UI Variable Text Semibold` survives in full while
  `Segoe UI Variable Display Semib` and `Segoe UI Variable Small Semibol`
  do not. Code keyed on the "Semib" spelling therefore finds Display and
  misses Text — and asking GDI for `Segoe UI Variable Text Semib` returns
  **Arial**, measured, against a `This Font Does Not Exist` control that
  also returned Arial. This is exactly why the face must be confirmed with
  a `SelectObject` → `GetTextFace` round trip: `CreateFontW` succeeded for
  the wrong name and handed back Arial without complaint.

**Faces must be verified, not requested.** GDI's font mapper never fails; it
substitutes silently, so a successful `CreateFontW` proves nothing. Each
candidate is confirmed with a `SelectObject` → `GetTextFace` round trip and
falls back to `lfMessageFont`.

## B.4 Colour: `GetSysColor`, always

Not one literal colour in the window code. `COLOR_BTNFACE`, `COLOR_WINDOW`,
`COLOR_WINDOWTEXT`, `COLOR_GRAYTEXT`, `COLOR_HIGHLIGHT`,
`COLOR_HIGHLIGHTTEXT`, `COLOR_HOTLIGHT`.

This is the necessary and sufficient condition for Windows' own
high-contrast themes to work, and those themes *are* the supported dark path
(see §7.2). Contrast guarantees only hold when foreground and background come
from the same palette; hard-code half a pair and the result is an arbitrary,
possibly invisible combination the user has no way to fix.

New arms: `WM_SYSCOLORCHANGE`, `WM_THEMECHANGED`, and `WM_SETTINGCHANGE` with
`SPI_SETHIGHCONTRAST` — invalidate and re-read.

**Do not add a `WM_CTLCOLORSTATIC` handler that returns
`GetSysColorBrush(COLOR_BTNFACE)`.** `DefWindowProcW` already returns that
brush and the class background is the same brush; such a handler is a no-op.

## B.5 Status vocabulary

`Mark::Unknown` currently carries three distinct meanings at once — "not
registered yet", "the catalog scan has not finished", and "beckon is paused"
(`serve` clears the `registered` map on pause) — and the window has nowhere
that says beckon is paused at all.

| Today | Proposed | Means |
|---|---|---|
| `OK` | *(nothing)* | Registered, and the app resolves |
| `!!` | `⚠ key in use` | `RegisterHotKey` failed; another program owns the chord |
| `!!` | `⚠ not installed` | No installed app has that Name |
| `..` | `checking…` | The catalog scan is still running |
| `..` | `paused` | beckon is paused from the tray |
| `..` | *(nothing)* | A new row with nothing picked yet |
| — | `custom` | A chord other than what holding Caps sends |

A screen reader currently announces `!!` verbatim. Words are the payload;
Fluent glyphs (`Segoe Fluent Icons`, falling back to `Segoe MDL2 Assets` —
identical code points, so only `lfFaceName` changes) are added later via
`NM_CUSTOMDRAW` as decoration over text that already works.

**The list mark and the editor note must agree.** They can currently
contradict each other: `control_state`'s `items` computes its mark from
`problems()` plus the registration map and never reads the catalog, while
`detail` does read the catalog. A row that registered fine but has no
matching app shows `OK` in the list and `!! no installed app has this name`
below it. One severity function in `beckon-core` feeds both, pinned by a unit
test.

## B.6 `Problem` gains a severity

```rust
pub enum Severity { Error, Warning }

pub struct Problem {
    pub row: Option<usize>,   // None = file-scope (the Caps row)
    pub severity: Severity,
    pub message: String,
}

// apply_enabled = m.dirty() && !problems.iter().any(|p| p.severity == Severity::Error)
```

Two consequences that are the point of the change:

- A warning (a single-modifier Caps Hold chord; a zero-modifier row) no longer
  blocks Save.
- **An unfinished row no longer blocks the rest of the file.** Today `Add`
  appends a blank row that immediately produces two errors and disables Save
  for everything, including edits made elsewhere before pressing Add. A new
  row is neutral until the user commits to it.

## B.7 Smaller changes in the same pass

- **Save moves to the command bar** as the default button, beside Close, with
  `Open config file` on the far left. Today `Apply` sits mid-window sharing a
  row with `Remove` — a destructive button with no confirm and no undo is the
  visual peer of the one that writes to disk — while the bottom bar holds
  only `Close`, so people aim there, press Close, and the save prompt becomes
  the real save path.
- **Mnemonics and accelerators.** No label currently has an `&`, there is no
  accelerator table, and `Apply` carries `BS_DEFPUSHBUTTON` — which *promises*
  Enter — while the window is not a dialog and does not handle `DM_GETDEFID`,
  so Enter does nothing. Add `&` mnemonics, `Ctrl+S`, and Enter. Esc and
  Close must keep going through the same `WM_CLOSE` path so the save prompt is
  asked exactly once.
- **Children are created in visual order.** The banner's `Reload` /
  `Keep mine` are created last today, so the one pair that responds to an
  urgent event sits at the *end* of the Tab order. Close the radio group with
  `WS_GROUP` on the following control; only `IDC_TAP_CAPSLOCK` has one today,
  so arrow navigation runs off the end of the group into the command bar.
- **Title bar** reads `beckon — shortcuts.toml`, prefixed with `●` when
  dirty, full path in the tooltip. `serve` can be pointed at any path and
  nothing on screen says which.
- **Notes get `SS_NOPREFIX` and `SS_ENDELLIPSIS`.** Without `SS_NOPREFIX` an
  app Name containing `&` renders wrong, and Start Menu names really do
  contain them.
- **An unparseable file opens read-only.** `open_settings` currently refuses
  with *"Fix it in a text editor first"* — precisely the moment a
  non-developer most needs the GUI. Open, state the parse error in plain
  language with the offending line, offer `Open config file`, and keep
  editing disabled until it parses. beckon still never writes over something
  it does not understand.
- **Fix the App field's typing defect.** Typing "Notepad" wrote `"d"` to the
  config while the screen showed "Debuggable Package Manager" — measured on
  a14. `commit_fields` papers over it at Apply time; the field itself still
  lies while you type. This may be the highest felt-value item in the whole
  spec, because the pain it causes ("I had to type `Windows Terminal` and did
  not know that was the Name") is the pain the suggestions feature in Part E
  is also aimed at.

  **CORRECTED 2026-08-11 — the cause named here was wrong.** This bullet, and
  its heading, used to read *"Fix the App combo box autocomplete. A populated
  `CBS_DROPDOWN` rewrites its own text as you type and the `CBN_EDITCHANGE`
  that arrives carries the text from before the rewrite."* Both sentences are
  false, and the first is what sent the first fix attempt (deferring the read)
  down a path that could not work — it changed nothing on hardware, because
  the read was never wrong. The combo box does not autocomplete while you
  type: `combo_probe` on a14, comctl32 6.16, 121 items, session 1, real
  `SendInput` keystrokes, found the field holding exactly what was typed,
  `CB_GETCURSEL` at -1, and the child EDIT receiving nothing but
  `WM_KEYDOWN`/`WM_CHAR`. What it *does* do is re-synchronise its edit to the
  closest catalogue item, and select the whole string, when it is **resized** —
  and `apply_state` ended with an unconditional `layout`, which `SetWindowPos`es
  every control on every keystroke. See §7.15, `Ui::shown_external` /
  `Ui::shown_empty`, and
  `docs/superpowers/measurements/2026-08-11-landing-1-a14.md` §24–26.

---

# Part C — what holding Caps stands for

## C.1 The config file does not change

Combos stay spelled out in full. Every existing config file, every README
example and every hand-edit keeps working, and there is no migration.

```toml
# Written ONLY when it differs from the default, so an untouched file stays
# readable by every older beckon binary.
keyboard.caps_hold = "ctrl+super+alt"   # default
keyboard.caps = true                      # Windows only
keyboard.caps_tap = "capslock"            # capslock | escape | none

"ctrl+super+alt+t" = "Windows Terminal"
"ctrl+super+alt+e" = "File Explorer"
"ctrl+super+alt+shift+t" = "Telegram Web"   # a different chord -> shown as `custom`
```

**The short form is derived, never stored.** A row is displayed and edited as
a single key whenever its *resolved* combo equals the Caps Hold chord + one key. The
test runs against the parsed `Combo`, not against how the line is spelled, so
an existing 18-binding file displays in the new short form with not one byte
changed. Rows on any other chord keep their full keycaps and a `custom` flag —
which the project's own README needs, since it documents
`"ctrl+super+alt+shift+t" = "Telegram Web"`.

## C.2 The one new key

```rust
/// Modifiers with no main key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Chord { pub ctrl: bool, pub super_: bool, pub alt: bool, pub shift: bool }

impl Default for Chord { /* ctrl + super + alt */ }
impl Chord {
    pub fn parse(s: &str) -> Result<Chord, String>;  // shares parse_modifiers with Combo
    pub fn canonical(&self) -> String;               // "ctrl+super+alt"
    pub fn is_default(&self) -> bool;
}

pub struct KeyboardConfig {
    pub caps: bool,
    pub caps_tap: CapsTap,
    pub caps_hold: Chord,   // new; meaningful only when `caps` is true
}
```

It exists for exactly two jobs: telling the Caps hook which chord to inject,
and telling the window which chord counts as short-form.

**`config_write::render` writes it only when `!chord.is_default()`.** The
existing spec makes any unknown key inside `keyboard` a hard error — on
purpose, so a typo like `caps_tab` cannot silently do nothing. That means a
file written by a new beckon and read by an older binary would be rejected
outright. Writing the key only when it carries information keeps every
untouched file readable by every shipped version. One `if`.

## C.3 Validation

**Errors** (block `beckon check`, block Save):

| Condition | Message |
|---|---|
| no modifiers | `` `keyboard.caps_hold` needs at least one modifier `` |
| unknown token | reuse `Combo::parse`'s message verbatim, via the shared `parse_modifiers` |
| duplicate modifier | likewise |
| `caps = true` **and** `caps_hold` contains `shift` | `` `keyboard.caps = true` cannot be combined with `shift` in `keyboard.caps_hold` — beckon has to press Shift for you, and releasing it would drop a Shift you are holding `` |

**Warnings** (printed by `check`, shown in the window, do not block):

| Condition | Message |
|---|---|
| exactly one modifier | ``holding Caps as `ctrl` alone takes Ctrl+T from every application`` |
| a row with zero modifiers | ``` `t` takes the T key in every application ``` |

In the window, the `Shift` chip is shown struck through with the reason
stated inline when Caps is ticked, rather than silently refusing the click.

## C.4 Two ways to set a shortcut, and they are two views of one value

**Superseded note, kept deliberately.** An earlier draft of this section made
the shortcut field a single `CBS_DROPDOWNLIST` of the 81 key names and
refused capture outright, because "`msctls_hotkey32` cannot capture the
Windows key, and Explorer consumes `Win+T` before a normal window sees them".
Both halves are true and both describe **a normal window receiving
`WM_KEYDOWN`** — which is not the layer beckon has to use. See Part F. The
sentence is preserved here so a later session does not re-derive the
dropdown-only design as the safe option and quietly reverse the reversal.

The editor line carries the full combo, not a single key, because a binding
may legitimately not use the Caps chord at all — `Win+X` is a case the user
named, and the README already documents
`"ctrl+super+alt+shift+t" = "Telegram Web"`.

Everything sits on **one row**. Remove and Add move up beside the list they
act on, which frees the width; a second row for the typed controls was drawn
and rejected as clutter.

```
[ Claude          ▾ ]  +  ☑Ctrl ☑Win ☑Alt ☐Shift  [ c ▾ ]  [● Record] [Reset]
                          └───── the same value, set without pressing ─────┘
```

**The typed path is primary; capture is an accelerator.** Four modifier
checkboxes plus a `CBS_DROPDOWNLIST` of the 81 key names are always present,
always Tab-navigable, and always readable by a screen reader. They make an
invalid combo *unrepresentable*, and they are the only path that works for
someone who cannot physically produce the chord — an on-screen keyboard, a
switch device, a keyboard that cannot reach `Win+Alt+F13`. A capture-only
control has no answer for that person, and `Open config file` is not one: it
leaves the application and gives up in-place validation.

This is also what PowerToys Keyboard Manager does — the dropdown is the
default, the capture button is the shortcut — and its own bug list
(#6837, #5624, #7088) is evidence that capture is hard *even when* a
non-capture path exists.

`DROPDOWNLIST` rather than `DROPDOWN` remains deliberate, and is the opposite
of the App field, which must stay `DROPDOWN` because beckon supports apps
with no Start Menu entry. A `DROPDOWNLIST` has **no edit control** — so the
App-field defect in §B.7 is structurally impossible here, and its typeahead
gives the fast gesture: press `t`, it selects `t`.

> **Both paragraphs used to name that defect by a refuted mechanism** — "the
> `CBN_EDITCHANGE` defect", i.e. a combo rewriting its own edit text as you
> type and delivering stale text to the notification. §7.15 records the
> measurement that falsified it. The true mechanism, and the one the
> reasoning below should be read against: a **populated combo re-synchronises
> its edit field to the closest matching list item, and selects the whole
> string, when it is RESIZED**. Both conclusions survive the correction —
> `DROPDOWNLIST` has no edit field to corrupt at all, and the two-writer rule
> is exactly the rule `layout`-on-the-keystroke-path broke — but they survive
> for the true reason, not the believed one.

**The two views must never both write at once.** Two writers on one value is
what produced the App-field defect: the model wrote what the user typed and
`layout`'s `SetWindowPos` made the control rewrite it back. While a capture
is armed, the checkboxes and the dropdown are `EnableWindow(false)`; on
commit or cancel they are restored from the model and re-enabled.

Keys capture can never see — bare `escape`, bare `tab`, and anything the
shell swallows — remain selectable from the dropdown. That is the JetBrains
"special keys" picker, which beckon gets for free because the closed key list
already exists.

---

# Part D — Caps hook repairs

These are independent of the redesign and two of them are live defects.

## D.1 The invariant was stated too narrowly

`caps.rs:112-120` frames the safety property around the Windows key. The
property that actually matters is:

> Every modifier in the burst must have at least one non-modifier key between
> **its own** press and **its own** release.

A bare Win-up is the gesture that opens Start; a bare Alt-up focuses the menu
bar. `chord()` satisfies this by construction. **`release_modifiers()` does
not** — it emits nothing but bare modifier-ups, and in exactly the situation
it exists to rescue (a partial `SendInput`: the Win-down landed, the key-down
did not) a bare Win-up is the Start gesture.

Fix: one harmless filler key (`VK_NONAME`, 0xFC) down/up at the head of the
release burst, taking it from three strokes to five.

## D.2 The chord becomes a parameter

`chord()`, `release_modifiers()` and `bound_keys()` take a `Chord`;
`decide()` takes it too.

The adversarial review argued for freezing the hook's chord as a constant, on
the grounds that it replaces one measured shape with fifteen unmeasured ones.
That is rejected: the invariant in §D.1 holds **structurally** for any
modifier set, because the burst is always all-modifiers-down → key-down →
key-up → all-modifiers-up. What is genuinely dangerous is Shift specifically,
so Shift specifically is refused (§C.3) rather than configurability in
general.

Two consequences that must be handled, not assumed away:

- **`CapsState` records the chord it actually injected.** `reload()` can run
  at any time from the file watcher, including between Caps-down and Caps-up.
  Releasing a different set of modifiers than were pressed leaves a stuck
  modifier, which is unrecoverable without killing beckon. The existing
  `injected: bool` becomes `injected: Option<Chord>`.
- The measurements recorded in `CLAUDE.md` for 2026-08-11 (the burst does not
  open Start; an injected chord does fire our own `RegisterHotKey`; 13 ms cold
  / 5.2 ms warm) were taken against `ctrl+super+alt` and continue to describe
  the default. They are not claims about every chord.

## D.3 `bound_keys` is fed from registration results, not the file

`serve.rs:452-456` already computes `outcome.by_canonical()` one line before
calling `sync_caps_hook`. Pass it in, and a key enters the Caps set only when
`RegisterHotKey` actually holds the corresponding chord. The contract "Caps
injects the chord `RegisterHotKey` is listening for" becomes true literally
rather than by assumption, and a failed registration stops producing an inert
injected burst.

Combined with §D.2 the signature is:

```rust
pub fn bound_keys(
    registered: &HashMap<String, Result<(), String>>,   // canonical combo -> outcome
    chord: Chord,
) -> HashSet<u32>
```

It walks the successful entries, parses each canonical string back to a
`Combo`, and keeps the virtual-key code of every combo whose modifier set
equals `chord` exactly. Two consequences worth stating, because both are
behaviour changes:

- **A row that failed to register is no longer in the Caps set.** Today it is,
  and pressing `Caps+<that key>` injects a chord nobody is listening for.
- **A `custom` row whose chord happens to equal the Caps Hold chord is included**,
  because the test is on the resolved modifier set, not on how the line was
  spelled. That is the correct reading: Caps stands in for that chord, and
  that binding uses it.

## D.4 Caps-down reinitialises unconditionally

Drop the `if !st.held` guard around `down_at` / `used` / `injected`; keep
`consumed`. A second Caps-down with no intervening up is either auto-repeat
(re-stamping is harmless) or a lost Caps-up — and lost Caps-ups are real: a
`WH_KEYBOARD_LL` hook is bound to the desktop of the thread that installed it
and receives nothing while the secure desktop is up (UAC, Ctrl+Alt+Del, the
lock screen).

## D.5 `set_bindings` must never touch `STATE`

Add the comment. Clearing `consumed` mid-stream leaks an unpaired key-up into
the focused application — which is what the existing "consumed is
deliberately NOT cleared here" note guards, and a reader who does not know
why will remove it.

---

# Part E — suggestions

## E.1 The line beckon does not cross

Windows keeps a great deal of evidence about what a person runs. Most of it
is out of bounds here for one reason: **beckon already installs a low-level
keyboard hook.** Reading the registry keys incident responders use to
reconstruct someone's session would be a second bad signal on the same
binary.

> beckon reads only what the user made by hand (their pins) and what is on
> screen right now (their open windows). Nothing the OS quietly accumulates
> about their behaviour.

This sentence goes in a comment above `scan_taskbar_pins()`, so a later
session does not "improve" the suggester with UserAssist.

| Source | Signal | | Reason |
|---|---|---|---|
| Taskbar pins, `%APPDATA%\Microsoft\Internet Explorer\Quick Launch\User Pinned\TaskBar\*.lnk` | high | **in** | A statement of intent the user made by hand — the same statement a hotkey binding makes. Reuses `walk_lnk_paths` + `parse_lnk`. Provides **the set**. |
| Open windows, `window_ops::enum_visible_windows` | high for order | **in** | z-order is MRU for free; `beckon list` already uses it. Provides **the ranking**. |
| `UserAssist` | best available | out | Run counts, focus counts, focus time. The first key a responder pulls. Out on optics — and separately disabled by `Start_TrackProgs = 0`, which this audience is likeliest to have set. |
| `MuiCache`, `AppCompatFlags`, Recent Items, jump lists, `wpndatabase.db` | mixed | out | All execution evidence. Same line, same reason. |
| SRUM, Prefetch, BAM/DAM | good | out | Unreadable at normal integrity, which beckon runs at by design. |
| `start2.bin` (Start pins) | medium | out | Readable but unparseable — opaque format, no public parser. |
| `Taskband\Favorites` (pin order) | medium | out | Undocumented PIDL blob, changes between builds, reported gone in 24H2. Pin *order* is therefore not used. |
| Timeline `ActivitiesCache.db` | — | out | Dead on Windows 11; the database exists and is empty. |
| Desktop shortcuts, `RunMRU`, `Uninstall\*`, Search index | low | out | Installers and OEM debris, or no usage signal at all. |

## E.2 A panel, not a seeder

**First run does not change.** `ensure_config` writes the same static
two-binding template. Suggestions live only in the settings window, as a
chip row; clicking a chip calls `add_row()` + `set_app()` + `set_combo()` and
produces an ordinary **unsaved** row. Nothing reaches disk before Save, through
the existing `Model::render()` path.

Four reasons this beats writing bindings automatically, in order of weight:

1. **A binding claims a machine-global, exclusive, first-come resource at the
   moment beckon knows least about the user.** If beckon takes
   `ctrl+super+alt+d` at autostart, some other application's shortcut simply
   stops working, with nothing in that application pointing back at beckon —
   so it is never reported as a beckon bug. The cost is asymmetric and it
   lands on someone else.
2. **The two hard-coded bindings are a format demo, not a counterexample to
   "beckon never picks for the user".** Terminal and File Explorer exist on
   every Windows machine, so the starter file always contains a working
   example. Replacing them with detected apps turns "here is how the format
   works" (cannot be wrong) into "here is a config I made for you" (can be).
3. **A seeder is dead code after one write per machine.** `ensure_config`
   returns `false` forever after. A panel keeps paying off, because nobody
   picks their hotkeys on day one — they pick them the week they start using
   a new tool.
4. **The recovery loop already exists and is already tested** in the window:
   `add_row`, the "no installed app has this name" note, the per-row
   registration error, `problems()` flagging both ends of a collision, and
   every write behind Save. A seeder would need a new, separately tested
   error surface.

A **first-run wizard is refused** for the same reasons plus its own: a second
modal window in a codebase whose only existing window needed hardware probes
to get right (three labels sharing control id `-1` meant two were never
placed), which runs once per machine and is therefore never exercised after
shipping, and which on the autostart path appears ~13 s after logon on every
new machine.

## E.3 The failure that matters

A suggested binding whose Name beckon's own resolver cannot later find is
worse than no suggestion. The rule that prevents it:

> The pipeline is **catalog × evidence → ranked catalog entries**, never
> **evidence → string**.

Four gates, independent of one another:

1. **The Name is taken verbatim from `InstalledAppInfo.name`** — the same
   namespace `resolve` tier 1 searches — never from a pin filename, an exe
   stem or a window title. A pin created by dragging an `.exe` onto the
   taskbar is named from `FileDescription` ("Code", not "Visual Studio
   Code"), and `RunningApp.name` is the window title, which changes as the
   user works.
2. **Every candidate round-trips** through `apps::resolve(name, &catalog)`
   against the **full** `scan_installed_apps()` — not `resolve_lazy`, which
   exists to dodge 370 ms on the hot path and does not apply here — and must
   return `MatchType::InstalledName` pointing at the entry that produced it.
   A miss drops the candidate entirely; it is never downgraded to a comment.
3. **For a running app**, `backend::windows_for_resolved` must contain the
   originating `HWND`. This is the same function the hot path uses to decide,
   so it is the strongest available evidence.
4. **Normalize collisions are dropped.** `merge_shell_apps` does not dedupe
   across sources, so a Start Menu shortcut and an AppsFolder entry can
   normalize to the same Name; `resolve` tier 1 takes the shortcut. A
   candidate coming from the AppsFolder side of such a pair would write a
   Name that **silently opens the wrong app**. Detect the collision, drop the
   candidate.

Structural filters as well: drop stems containing `Uninstall` (they point at
real `.exe` files, so the existing filter misses them) and targets under
`System32`. `.url`, `.chm` and directories are already excluded, since
`walk_lnk_paths` accepts only `.lnk` and `parse_lnk` bails when the target is
not an `.exe`.

Drop any Name already in the model, and never propose a combo already in the
model. `problems()` catches duplicates *after* they exist; the suggester must
not create them.

Ranking: apps with an open window first, in z-order, then pinned-only, then
byte order of `name` — the same tie-break `name_substring_matches` uses.

## E.4 Key assignment

- The suggested key is the **first `a-z` character of
  `apps::normalize(name)`**. `normalize` lowercases and strips 8 format/bidi
  marks; it does **not** fold diacritics, and the workspace has no
  `unicode-normalization` dependency. Do not write "NFD fold" into an
  implementation plan — it does not exist.
- **Pre-claim every combo already in the model**, including the template's
  `t` and `e`. `File Explorer` is bound to `e` rather than `f` on purpose, to
  inherit `Win+E` muscle memory; a naive pass produces `f` and breaks a
  shipped default.
- **A collision leaves the key blank** with the Name filled in. There is no
  second-letter fallback: a second letter is a positional key in disguise,
  which is the thing Vimium's filtered hints exist to fix. The user types one
  key, which they must do for every other row anyway.
- **A Name with no `a-z` at all** (CJK, Cyrillic, Arabic) also leaves the key
  blank. No transliteration.
- **No letter blocklist.** `RegisterHotKey` is the only honest oracle: it
  returns `ERROR_HOTKEY_ALREADY_REGISTERED`, `hotkey.rs:498` turns that into
  a per-row string, and the window renders it. Propose, register, report.
  Note for anyone extending this to function keys later: `VK_F12` is reserved
  for the debugger and can be taken away *after* a successful registration,
  so registration stops being a sufficient oracle there.

## E.5 Where it runs

`spawn_catalog_scan` already runs on its own STA worker thread because
`scan_installed_apps` costs 370–500 ms and the message loop is the same
thread that dispatches `WM_HOTKEY`. Pin scanning and window enumeration join
it there: about +20 ms on a task already costing 400.

`RuntimeStatus` gains `suggestions: Option<Vec<Suggestion>>`. **`Option`, not
an empty `Vec`** — the existing comment on `catalog` names exactly this trap:
a scan that has not run is not the same as nothing to suggest. The UI shows
`checking…` for `None` and hides the band for `Some(vec![])`.

Hot-path cost: zero. First-run cost: zero.

---

# Part F — capture, the availability probe, and the list

## F.1 Capture is possible, and the old reason was about the wrong layer

`msctls_hotkey32` has no bit for the Windows key — `HOTKEYF_*` covers only
ALT / CONTROL / SHIFT / EXT — and a normal window's message queue never sees
`Win+T` because Explorer consumes it first. Both facts are true. Both are
about **a window receiving `WM_KEYDOWN`**.

A `WH_KEYBOARD_LL` callback runs before the keystroke reaches any queue and
before shell hotkey processing, sees `VK_LWIN` as an ordinary `vkCode`, and
suppresses the key by returning `1`. beckon already owns that hook for the
Caps feature. Suppression is not incidental — without it, capturing
`ctrl+super+alt+t` would focus Windows Terminal while you were binding it.

**This widens the LLHOOK exception from one feature to two**, because capture
arms the hook on machines where the user deliberately left
`keyboard.caps = false`. That is a real cost and it is stated in the UI and in
`CLAUDE.md`, not buried.

## F.2 One hook, two mutually exclusive modes

**Do not install a second hook.** `WH_KEYBOARD_LL` hooks chain, so a separate
capture hook runs alongside the Caps one and two things break:

- Pressing `Caps+T` to *bind* it is swallowed by the Caps arm of
  `caps::decide` and injected as `ctrl+super+alt+t`, so the field records the
  **alias instead of the key the user pressed**.
- The capture hook swallows and does not call `CallNextHookEx`, so the Caps
  arm sees Caps-down and never sees Caps-up, leaving `CapsState.held` stuck
  `true` after capture ends.

Instead, `hook_proc` checks a capture mode **first**, before `caps::decide` is
consulted:

```rust
if capture::armed() && GetForegroundWindow() == settings_window::hwnd() {
    let act = beckon_core::capture::step(ev, &mut CAP_STATE.borrow_mut());
    if act.post { PostMessageW(Some(hwnd), WM_CAPTURE, w, l); }
    return LRESULT(1);          // swallow everything: down, up, modifier or not
}
```

The decision logic is `beckon_core::capture::step` — pure, beside
`caps::decide`, for the reason `caps.rs:14-15` already gives: *"a keyboard
state machine is the last thing that should be tested by one job in three"*.
The callback does three things only: read `vkCode`, update a fixed-size held
array, `PostMessage`. No allocation, no string formatting, no
`CallNextHookEx`. Everything visible is built on the UI thread when
`WM_CAPTURE` arrives.

**The install lifecycle needs a two-reason refcount.** Today `sync_caps_hook`
is the sole owner, `install()` early-returns when already installed and
`uninstall()` unhooks unconditionally. Capture wants the hook transiently
while Caps may want it resident. Calling `uninstall()` during capture is
wrong twice: it resets `CapsState`, and a config reload mid-capture calls
`sync_caps_hook` and reinstalls the hook underneath the capture.

**`is_installed()` can lie.** Past `LowLevelHooksTimeout` — 300 ms by
default, raised to a 1000 ms ceiling since Windows 10 1709 — Windows removes
the hook silently and there is no API to ask. For capture the watchdog covers
it; for Caps this is a known latent issue, recorded not fixed.

**The invariant is broader than "the callback must be short."** The callback
is dispatched by the message loop of the thread that installed the hook, and
that is the thread hosting the settings window and `WM_HOTKEY`. So *nothing
on the hook's thread may block* — a modal loop or a synchronous scan starves
the callback just as effectively as a slow callback does.

## F.3 The capture state machine

Displayed strings are verbatim and ASCII, for the reason `mark_glyph` is
ASCII: the window inherits the shell font and a missing glyph reads as a
rendering bug.

**Idle** — checkboxes and key dropdown enabled and showing the current combo.
`Record` and `Reset` available. `Reset` clears the combo and leaves the row
without a shortcut.

**Armed** — entered by pressing `Record`. Checkboxes and dropdown
`EnableWindow(false)`; button reads `Stop`; hint reads
`Press the shortcut. Esc stops recording.` Hook armed.

- A non-modifier pressed with **no** modifier held → stay Armed,
  `MessageBeep(MB_ICONWARNING)`, hint becomes
  `A alone is not a shortcut - hold Ctrl, Win or Alt as well. Press Record and try again.`
  **The pressed key is still shown as a keycap.** Showing what beckon heard
  and then explaining why it is not acceptable is the whole point; silently
  refusing to record reads as a broken keyboard.
- A key with no name in the 81-key table (numpad, media, IME,
  `VK_PROCESSKEY`) → stay Armed, beep, hint
  `beckon has no name for that key. Pick one from the Key list.`
- Bare `VK_ESCAPE` → **Cancelled**. Because the hook swallows it, it never
  becomes a `MSG`, so `IsDialogMessageW` never turns it into `IDCANCEL` and
  the window does not close.

**Holding** — at least one modifier down, no main key yet. The field shows the
partial combo live, in canonical order, exactly as it will be written to the
TOML: `ctrl+super+alt+...`. Releasing every modifier returns to Armed and is
**not an error** — a double-tap of Ctrl shows `ctrl+...` and returns to the
prompt.

**Committed** — on the **first non-modifier key-down while at least one
modifier is held**. Not on key-up (release order is not press order), no
timer, no hold-to-confirm, and no Enter key to confirm because `return` is
itself a bindable key. The row becomes dirty; the probe runs; the checkboxes
and dropdown are set from the captured combo and re-enabled.

**Draining** — after Committed or Cancelled, the hook keeps swallowing until
every held key is released, then disarms. This is what makes `Alt+Tab` safe:
the `alt` down was swallowed, so the `alt` up is swallowed too, and the system
never sees a bare Alt-up — the same class of gesture §D.1 exists to prevent.
`WM_DESTROY` skips draining and unhooks immediately; there is no window left
to protect and holding the hook one beat longer leaves a swallowed keyboard.

**If `SetWindowsHookExW` fails**, do not enter Armed and do not silently fall
back to message-queue capture — that path cannot see the Windows key, so it
fails on precisely the chords beckon recommends. Hint:
`Cannot record here. Use the modifier boxes and the Key list instead.`

## F.4 Edge cases, decided

| Case | Decision |
|---|---|
| **Esc** | Armed: stops recording, window stays open. Not armed: unchanged, closes the window. `escape` is still bindable from the dropdown. An Esc injected by `caps_tap = "escape"` carries beckon's own `dwExtraInfo` marker and is ignored by capture. |
| **Tab** | Armed: swallowed like any key, so `alt+tab` and `ctrl+tab` are recordable. Bare `tab` comes from the dropdown. Not armed: `filter_dialog_message` keeps Tab navigation. |
| **Modifier-only** | Not an error, no beep, nothing recorded. Holding while held, Armed on release. |
| **Auto-repeat** | `KBDLLHOOKSTRUCT` carries no repeat count. The held-set is the filter: a key-down for a vk already held is swallowed and changes nothing. |
| **Sticky Keys** | At commit the modifier set is the held-set **unioned** with `GetAsyncKeyState` for the five modifier VKs. A latched modifier may have sent its key-up before the main key went down; the union recovers it without needing to know Sticky Keys is on. Needs measuring. |
| **Losing focus** | Three independent layers, all → Cancelled: `WM_KILLFOCUS`, `WM_ACTIVATE`/`WM_ACTIVATEAPP` with `WA_INACTIVE`, and the per-event `GetForegroundWindow()` gate inside the hook — which is the only one that fires when a UAC prompt or an elevated window steals foreground without sending either message. |
| **Window close** | `WM_CLOSE` disarms **before** the save prompt; `WM_DESTROY` disarms skipping the drain. There is no path where the window dies with the hook armed. |
| **Process killed mid-capture** | Not defensible, and not claimed. The hook lives in the killing process's address space and win32k is expected to clean it up, but MSDN does not promise it — so this is a measurement, not a README sentence. The real mitigation is the 10 s watchdog bounding the armed window. |
| **Left/right modifiers** | Normalised. The TOML cannot express the distinction. |

## F.5 What cannot be captured

**MEASURED 2026-08-12 on a14, with a person at the keyboard.** Results in
`docs/superpowers/measurements/2026-08-11-landing-1-a14.md` §47–§50. The eight
chords named below the table are confirmed capturable; **the `Win+L` row was
wrong and is rewritten**; `Win+G` is untested for a nameable reason. The rows
that were never in doubt (`Fn`, the lock keys, the Office/Copilot keys, UIPI,
other remappers) are unchanged.

| | Why | What the user sees |
|---|---|---|
| `Win+L` — **REVISED, the old mechanism was disproved** | The hook **does** see it: `SEEN=True`, measured. The key-down is delivered on the normal desktop before winlogon acts. What returning 1 cannot do is stop the lock — the machine locked anyway. The old entry claimed the hook "sees **nothing**, including key-ups", which is false | **A block-list, not blindness.** Because capture *can* see it, it would happily record `Win+L` and hand the user a binding that can never fire. It must be refused explicitly at commit, with the help line saying so |
| `Ctrl+Alt+Del` | The SAS is monitored by winlogon below the hook chain. **Not tested** — it shares the secure-desktop story with `Win+L`, and that story has now been disproved for `Win+L`, so this row is unverified rather than merely untested. Deliberately not run: far more disruptive to a live session than the one measurement it would add | Treat as refused, like `Win+L`, until measured |
| Anything typed while a secure desktop is up (UAC, lock screen) | Same mechanism. The hook sees **nothing, including key-ups** | The state machine must **re-seed** from `GetAsyncKeyState` on regaining foreground, never resume |
| `Fn` | Handled in keyboard firmware; emits no scan code | Pressing it does nothing |
| `Num Lock`, `Caps Lock`, `Scroll Lock` as part of a combo | The lock state toggles **before** the hook runs, so swallowing does not undo the light | Excluded from the capturable set rather than replayed |
| `Win+G` — **UNTESTED, not refuted** | Claimed: Game Bar opens even when reassigned. On a14 it did not open **even with the key passed through**, so the swallowed run proved nothing — `Microsoft.XboxGamingOverlay` is not installed there. The pass-through control is the only reason this reads as untested rather than as a refutation | Unchanged pending a machine with the overlay |
| Office key, Copilot key | The physical key emits a whole chord — Office is `Win+Ctrl+Alt+Shift`, Copilot is `Win+C` or `LShift+Win+F23` | Capture records what the keyboard actually sent, which is the honest answer; one explanatory line, not a bug |
| Anything, while an elevated window has focus | UIPI. Measured on a14 2026-08-11 | The watchdog fires |
| Anything another remapper claimed first | kanata / PowerToys / AHK started after beckon sits ahead of it in the chain | The field silently records the **wrong** chord. This is the existing "other remappers" gap in a new guise |

`Win+T`, `Win+X`, `Win+D`, `Win+E`, `Win+R`, `Win+Tab`, `Alt+Tab` and
`Ctrl+Shift+Esc` are **not** on this list, and that is now **measured rather
than inferred**: all eight came back `SEEN=True SWALLOWED=True ACTED=False` on
a14. The inference this replaces — that PowerToys Keyboard Manager is a plain
`WH_KEYBOARD_LL` returning 1 plus `SendInput`, and remaps Windows-key
shortcuts globally — happened to be right, which is not the same as having
been checked.

The control that carries the claim is `Win+R` appearing **twice in one run**:
passed through it opened the Run dialog, swallowed it did not. One chord, one
session, one variable.

~~**This table is not shipped until it is measured on a14.**~~ Measured.
§F.5 no longer gates Landing 2b — with two rows carried forward as open:
`Ctrl+Alt+Del` (unverified, and its shared explanation is now disproved) and
`Win+G` (untested, no Game Bar on the only machine available).

## F.6 The availability probe

Order is load-bearing:

1. **F12 guard, before anything.** MSDN reserves `VK_F12` for debuggers *"at
   all times… even when you are not debugging"*, so a successful registration
   proves nothing. One line, and it prevents the worst outcome: a green
   `Available` on a key documented never to arrive.
2. **Compare against the in-memory shortcut table** for a self-conflict.
   **Do not consult `ServeState.registered`** — `set_paused` and `reload`
   *clear* that map, so probing while paused would report beckon's own bound
   chord as free. `registered` explains why a row is red; it never decides
   whether a chord is free.
3. **Only if there is no self-match**, run the live probe.

The probe registers on the **settings window's HWND** with a fixed id, while
the live table registers on `tray_hwnd`. A hotkey is identified by the
`(hWnd, id)` pair, so a different HWND makes collision impossible by
construction — whereas picking "an id high enough" on `tray_hwnd` is a bet on
config size, and the live ids are row indices. Getting the pair wrong is
worse than it sounds: *"If a hot key already exists with the same hWnd and id
parameters, it is maintained along with the new hot key"* — so the probe
would create a second registration under the same id and its
`UnregisterHotKey` would remove an unspecified one of the two. That is a
silently dead hotkey, the same class as the "20 shortcuts registered" incident.

The probe runs on the thread owning the window (`RegisterHotKey` is
thread-affine) and **never inside a hook callback**. It unregisters on every
exit path; a cancelled capture must not leave a global hotkey claimed.

**The verdict travels through `RuntimeStatus`, not `problems()`.**
`Model::problems()` is pure and that is what keeps `apply_enabled` testable on
the Linux and macOS CI jobs. `RuntimeStatus` already exists for exactly this
class of runtime fact, and the probe field is `Option` for the same reason
`catalog` is: not-yet-probed is not the same as free.

### Strings, verbatim

| Outcome | Mark | String |
|---|---|---|
| Free | OK | `Available. Nothing else on this PC is using it.` |
| Free, contains the Windows key | OK | `Available right now. Windows reserves Windows-key shortcuts and can take this one back after an update, so press it once after saving to be sure.` |
| Unchanged | OK | `Unchanged - this row already uses it.` |
| Duplicate within this file | !! | `Already used by "{app}" in this file. A shortcut can only mean one thing.` |
| Taken by another process | !! | `Another program already has this shortcut. Windows does not tell beckon which one, so beckon cannot name it. Saved as-is, it will not fire.` |
| F12 in the chord | !! | `F12 is reserved for debugging tools and never reaches beckon. Pick a different key.` |
| Capture saw nothing | .. | `Windows handled that shortcut itself, so beckon never saw it. A few shortcuts, like Win+L, cannot be reassigned by any program.` |
| Probed while paused | .. | appended: `beckon is paused, so this shows what will happen when you resume.` |
| Ctrl+Alt with no Win | .. | `On international layouts this is Alt Gr, so typing an accented character will fire it.` |

No string contains `RegisterHotKey`, `UIPI`, or an error code except the
catch-all, where the code is the only information Windows gives.

### What the probe may not promise

**A successful registration must never be reported as "this shortcut
works."** The strongest claim it licenses is *nothing else is holding it*,
which is what the strings say. Three independent reasons a registered chord
still may not fire: F12 (guarded); a chord eaten by a hook or the shell's
input path above hotkey dispatch, where **nobody registered it** so the probe
succeeds; and TOCTOU, since another process may claim it between the probe and
Apply.

Because of TOCTOU, **the probe label is replaced by the real registration
result after Apply**, not left standing. `register_all` is the authority and
the window already receives it through `registered`.

**UIPI is not a probe caveat.** Measured on a14 2026-08-11 with Task Manager
elevated and focused: the typed chord still fired while `Caps+N` did nothing,
against a normal-window control run. The elevation gap belongs to the Caps
hook, not to a registered chord — and the window must say so, because a user
who read the Caps caveat will assume it applies here.

Only a real keypress after Apply may say `Working. beckon received {combo}.`

## F.7 The list: checkboxes, fixed height, and one abort-class bug

**Internal scrolling costs nothing.** A report-mode ListView already scrolls
internally, and `layout()` already derives the list height from
`GetClientRect`. The requirement's real content is that the list band must not
take its height from the row count, and that the rebuild must stop destroying
state.

**Checkboxes** are `LVS_EX_CHECKBOXES`, one more bit in the existing
`LVM_SETEXTENDEDLISTVIEWSTYLE` call. **Keep `LVS_SINGLESEL`**: check state is
independent of selection, so multi-delete works without multi-select and the
editor strip keeps having exactly one "current row". **Never set
`LVS_EX_AUTOCHECKSELECT`** — it ties checking to selection, which is the
ambiguity being avoided. The checkbox is column 0's state image, not a new
column, so deleting the 34 px status column is compatible.

**Do not port `ListView_GetCheckState`.** It is `(state >> 12) - 1` on an
unsigned value, so an item that was never given a state image returns
`0xFFFFFFFF`, not `0`.

Check and selection changes both arrive as `LVN_ITEMCHANGED` with
`LVIF_STATE` and cannot be told apart by `uChanged`. Distinguish by bit:
`(uOldState ^ uNewState) & LVIS_STATEIMAGEMASK` for a check,
`& LVIS_SELECTED` for a selection. They are independent and can arrive in one
message — test each, never `else if`.

### The bug this exposes, which must land first

**The `WM_NOTIFY` arm does not consult `suppressed()`**, unlike every
`WM_COMMAND` arm. Once `apply_state` writes selection or check state with
`LVM_SETITEMSTATE`, comctl32 fires `LVN_ITEMCHANGED` **synchronously inside
`apply_state`** → `on_select` → `refresh_settings` → `apply_state`: unbounded
recursion across an `extern "system"` boundary, where a `RefCell`
double-borrow **aborts the process** rather than unwinding. One line —
`if suppressed() { return LRESULT(0); }` at the top of the arm — and it must
land before either of the other two changes.

### Ticks live in the Model

`Row` gains `marked: bool`. `apply_state` stays a pure push, and "ticks lost
on rebuild" becomes unrepresentable rather than compensated for. This is not
a preference: the file's own header says *"every decision it draws comes from
`ControlState`… This file holds no policy"*, and a tick set living only in a
control's state-image bit would be the one piece of window state that is not a
projection.

`set_marked` **must not set `dirty`** — a tick changes nothing on disk, and
`apply_enabled = dirty && …`, so marking would enable Save for an empty edit
and rewrite an unchanged file. `RowWrite` does not gain a field; marks are UI
state, and `Model::from_text` defaults them to `false`, so an external reload
drops them, which is correct.

### Stop rebuilding on every keystroke

Cache the last-pushed `Vec<ListItem>`. When the item count is unchanged —
which is every text edit — send `LVM_SETITEMTEXTW` only for cells whose text
actually differs, and never `LVM_DELETEALLITEMS`. Scroll position and check
state are then never disturbed because nothing is destroyed. Count is the
discriminator that keeps this trivial: no keyed reconciliation, no ids in
`LVITEM.lParam`.

Only Add, Remove and reload change the count, and only they rebuild. For that
path, restore scroll with a **pair** of `LVM_ENSUREVISIBLE` — read
`LVM_GETTOPINDEX` and `LVM_GETCOUNTPERPAGE` first, then ensure
`min(top + per - 1, count - 1)` and then `top`. A single
`ENSUREVISIBLE(top)` is a no-op because after a rebuild `top` is already on
screen. Wrap the rebuild in `WM_SETREDRAW`.

**A pre-existing defect this pass must also fix:** the selection highlight is
destroyed by `LVM_DELETEALLITEMS` on every refresh and never restored — the
reinsert loop sets `mask: LVIF_TEXT` only. Typing one character into the App
field loses the highlight while `Model.selected` still says otherwise.

## F.8 The Caps Lock row: Hold and Tap

**The phrase "beckon key" leaves the window entirely.** It was internal
vocabulary — a name for the chord that Caps stands in for — and it made the
row explain itself in prose. Naming the two things a key can do says the same
thing in two words:

```
☐ Caps Lock    Hold  [Ctrl] [Win] [Alt] [Shift]    Tap  [ Caps Lock ▾ ]
```

That reads as a definition of the key, needs no sentence under it, and fits on
one line. Everything after the checkbox is greyed while it is unticked,
exactly as the tap radios are greyed today.

The three `IDC_TAP_*` radios are deleted; `Tap` becomes a three-value
`CBS_DROPDOWNLIST` — `Caps Lock`, `Esc`, `Nothing` — read with `CB_GETCURSEL`
on `CBN_SELCHANGE`, never by reading its text, because even a `DROPDOWNLIST`
has typeahead that moves the selection.

Defaults when ticked: **Hold** = `Ctrl + Win + Alt`, **Tap** = `Caps Lock`,
so ticking the box costs nothing that was there before — the key still does
what it always did when tapped.

`Caps Lock` is a static label, not a control. It is the only key the alias can
be; the whole feature is that the key under the left little finger is
otherwise wasted.

### The config keys follow the labels

Renamed to match, since the UI no longer has a "beckon key" to name:

```toml
keyboard.caps = true
keyboard.caps_hold = "ctrl+super+alt"   # default; what holding Caps stands for
keyboard.caps_tap = "capslock"          # capslock | escape | none
```

`caps_hold` replaces `beckon_key` from §C.2. It is a strict improvement:
symmetric with `caps_tap`, self-documenting beside it, and honest about scope
— it is meaningful **only** when `caps = true`, which is exactly how the UI
greys it. `caps` and `caps_tap` are unchanged, so the only new key is
`caps_hold`, still written only when it differs from the default.

### What the window loses with "beckon key", and why that is fine

Two jobs went with the name, and neither needed it:

- **Prefilling a new row.** With Caps on, `Add` prefills `caps_hold`. With
  Caps off, the row starts empty and the user presses `Record` or ticks the
  modifier boxes. That is one keystroke's difference in the rarer case.
- **Dimming a shared prefix in the list.** The rule no longer consults any
  setting: **the last keycap is the main key and is highlighted; every
  modifier before it is dimmed.** Purely structural, true for every row
  including `custom` ones, and it removes a coupling between the list's
  appearance and a keyboard setting three sections away.

### Default off, and the invitation is the checkbox itself

`keyboard.caps` stays `false` by default. A default of `true` would mean every
fresh install calls `SetWindowsHookExW`, and today a default install **never**
does — that is the load-bearing half of the EDR argument for a binary shipped
through Scoop and GitHub releases. That decision stands.

What changes is that the option stops hiding: it is the first row of the
window instead of a group box below the fold, and when ticked a single line
states the cost in plain words — *"Turning this on installs a keyboard hook
while beckon runs. Untick it and beckon never installs one."* A user is
entitled to know what they just installed.

Deliberately **not** a first-run prompt; that is a second modal moment on a
path this spec already refuses one for (§E.2).

Deleting the radios changes which control must close the keyboard group with
`WS_GROUP` — fix it in the same pass as §B.7.

**The Hold chips must not become a capture field.** A `Chord` is *modifiers
with no main key*, which a capture field cannot express — and releasing
modifiers with nothing between them is exactly the gesture §D.1 exists to
prevent.

---

# Error handling

| Failure | Response |
|---|---|
| Manifest fails to compile into the binary | Release gate fails the build (§A.1.1). Never silent. |
| `keyboard.caps_hold` invalid | `parse_config` error; `check` exits non-zero; `reload` keeps the running keys and posts one throttled toast — the existing path |
| `caps = true` with `shift` in `caps_hold` | Hard error at parse; the window disables the Shift chip with the reason inline, so the state is unreachable through the UI |
| A single-modifier Caps Hold chord | Warning, does not block. Save is allowed |
| A row's app Name is empty, or the key is unset | The row is neutral, not an error. Save proceeds for the rest of the file |
| Two rows resolve to the same combo | Both flagged, naming the shared canonical form — unchanged |
| Config file does not parse | Window opens **read-only** with the parse error and the offending line, plus `Open config file` |
| Write fails | `MessageBoxW`; the old file survives because the write is rename-atomic; running shortcuts unaffected |
| Catalog scan fails or returns nothing | `checking…` resolves to no suggestions; free typing in the App field still works; the app column shows unknown, never "not installed" |
| Suggestion round-trip gate fails | Candidate dropped silently. A suggestion that cannot be resolved must never be offered |
| `SetWindowsHookExW` fails | Log + toast, untick the box so the UI does not lie, keep serving — unchanged |
| File changed on disk while dirty | Banner with Reload / Keep mine, now at the *top* of the Tab order |

---

# Testing

**Unit, all three CI jobs** (`beckon-core` / `beckon-cli`):

- `Chord::parse` — every valid subset; empty; unknown token; duplicate
  modifier; the shared `parse_modifiers` produces byte-identical messages to
  `Combo::parse`.
- `is_default` gates the write: rendering a default `Chord` must not emit
  `keyboard.caps_hold`; a non-default one must.
- Round trip, extended: every valid `Model` → TOML → `parse_config` → the
  same `Model`, now including `caps_hold`.
- The short-form predicate: a row spelled `ctrl+super+alt+t` and a row spelled
  `alt+ctrl+super+t` both resolve to short form under the default Caps Hold chord;
  `ctrl+super+alt+shift+t` does not.
- `Severity` — a warning leaves `apply_enabled` true, an error does not; an
  unfinished row does not block a valid sibling.
- One severity function feeds both the list mark and the editor note; assert
  they cannot disagree for any row.
- `caps::decide` — the existing suite, plus three property tests: every
  modifier in a `chord()` burst has a non-modifier key between its own down
  and up; the same for `release_modifiers()` after the filler; and
  `release_modifiers()` names exactly the modifiers `chord()` pressed. Keep
  the existing 8-stroke assertion as a golden vector for the default chord.
- `CapsState.injected: Option<Chord>` — a chord change between Caps-down and
  Caps-up releases the modifiers that were actually pressed.
- Suggestions: the whole of §E.3 and §E.4 against a synthetic catalog and a
  neutral `Vec<Candidate>`, so it runs on the Linux and macOS runners.

**Manual on a14, session 1, via a scheduled task** with
`New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries` and
`-EncodedCommand`, built with `cargo build --all-targets` — `--examples` does
not build `[[bin]]` targets and a stale `beckon-serve.exe` produces
confidently wrong results:

1. **Manifest, before and after, with a control run taken first.** Print
   `lfMessageFont.lfFaceName` and `lfHeight`; print `GetDpiForWindow`;
   screenshot at 100 % and 150 %; measure themed control heights under v6 —
   those are what row and button heights get tuned to. Without the
   pre-manifest control, a broken probe and a clean result look identical.
2. **`release_modifiers()` both ways.** Extend `caps_probe` to truncate a
   chord burst deliberately (modifiers down, no key), then fire the release,
   and ask whether Start opened — with and without the filler. The control is
   a bare Win-tap already proven to open Start.
3. **`bound_keys` from registration results.** Register a combo from another
   process so beckon's registration fails, then press `Caps+<that key>` and
   confirm **nothing is injected**, rather than an inert chord.
4. **`settings_probe` against the new controls.** Drive the filter box, the
   key `DROPDOWNLIST` and the modifier chips; Apply; read the file back. Read
   control text with `SendMessage(WM_GETTEXT)` — `GetWindowText` returns the
   kernel-side caption and reads back empty for an `EDIT` or `COMBOBOX`.
5. **Hotkeys still fire while the window is open**, Tab reaches every
   control, Esc closes, and 150 % scaling is checked — the previous spec
   listed high-DPI as unverified and this is the pass that settles it.

---

# Documentation to update

- **`CLAUDE.md`**
  - *Known constraints*: a new entry recording that the settings window is
    light-only by decision, that Windows high-contrast themes are the
    supported dark path, and that undocumented `uxtheme` ordinals were
    considered and refused on EDR grounds given the existing LLHOOK. Write it
    so a later session does not re-open the argument from scratch.
  - *Caps Lock … the LLHOOK exception*: the chord is now configurable; the
    invariant is restated as in §D.1; `shift` is refused while Caps is on.
  - *Phase 3 Windows notes*: the manifest, and that `s()` was dead code
    before it.
  - *Out of scope → GUI/TUI*: the window now also shows suggestions, which
    are not a launcher — it still never focuses or launches anything.
- **README**: what holding Caps sends is configurable; the settings window screenshot;
  the suggestion source boundary stated plainly, because a user is entitled
  to know what beckon reads.
- **`2026-08-11-windows-settings-window-and-caps-design.md`**: a pointer at
  §A.5 to this spec.

---

# What this spec does not do

- **No dark mode.** Light only, with `GetSysColor` throughout so high-contrast
  themes work. See §7.2.
- **No capture-only shortcut field.** Capture is an accelerator; the modifier
  checkboxes and the key dropdown remain the primary, accessible path (§C.4).
- **No two-stroke chord sequences** in the VS Code sense. beckon's grammar is
  modifiers plus one key, and the TOML cannot express anything else.
- **No modal capture dialog.** The window is modeless on purpose, and a modal
  loop on the hook's thread starves the hook callback (§F.2).
- **No delivery of non-registerable combos through the LLHOOK.** Capture makes
  it easy to author a chord `RegisterHotKey` refuses; beckon reports that and
  stops. Delivering it via the hook instead would turn an opt-in feature into
  a mandatory always-on one and reopen every EDR, UIPI and timeout trade-off.
- **No `beckon+t` shorthand in the config file.** See §7.3.
- **No config-file migration**, automatic or offered. The short form is
  derived from the resolved combo, so no file needs changing.
- **No per-OS `caps_hold` override.** One file, one Caps chord. `parse_keyboard`
  is structured so that shape stays open if it is ever genuinely needed.
- **No seeded starter file and no first-run wizard.** See §E.2.
- **No macOS equivalent** of the window or the suggestions.

---

# 7. Refuted claims

Recorded so they are not proposed again. Across three research passes: 45
agents, ~250 verified findings, 20 refuted.

**7.1 "Rewrite it in egui / iced / Slint."** The replaceable surface is 986
lines and the pure model already lives in `beckon-core`, so a toolkit buys
almost no code while costing 76–195 crates and at least +4.2 MiB on a 1.37 MiB
binary. Worse: a GPU toolkit initialising on the thread that hosts
`WH_KEYBOARD_LL` can exceed the 300 ms `LowLevelHooksTimeout`, and Windows
unhooks silently with no error anywhere — Caps Lock would die and nothing
would say why. USER32 also provides UIA/MSAA, IME, caret, clipboard and
high-contrast for free.

**7.2 "Dark mode is a solved problem in 2026."** There is still no documented
Microsoft API that makes Win32 common controls render dark. Everything that
works goes through undocumented `uxtheme.dll` ordinals — ordinal 135 has
already changed signature once (1809 `AllowDarkModeForApp(BOOL)` → 1903
`SetPreferredAppMode(enum)`), and calling the old ABI gives wrong values, not
a link error. Dark scroll bars additionally require patching comctl32's
import table, a textbook EDR signature on a binary that already carries a
keyboard hook. Half-dark (dark title bar, light client) is worse than none.

**7.3 "Add a `beckon+t` shorthand."** Dropped by the user, and the review
confirmed it would have cost more than it looked: `toml::Table` is a
`BTreeMap`, so `"beckon+t"` sorts *before* `"keyboard"` and `"super+t"`
*after* — a single-pass parse would resolve two different chords inside one
file, and fixing it requires splitting `parse_config` into two passes.
Keeping literal combos means there is nothing to resolve at parse time.

**7.4 "Freeze the Caps hook's chord as a constant."** Over-cautious. The
safety property holds structurally for any modifier set (§D.2). Shift is the
dangerous element and is refused specifically.

**7.5 — REVERSED 2026-08-11. "Chord capture cannot work, because
`msctls_hotkey32` cannot capture the Windows key and Explorer eats `Win+T`."**
Both halves are true and both are about **a normal window receiving
`WM_KEYDOWN`**, which is not the layer beckon uses. A `WH_KEYBOARD_LL`
callback — which beckon already owns — runs before any queue and before shell
hotkey processing, and suppresses the key by returning 1. Capture is in; see
Part F.

~~"`DROPDOWNLIST` typeahead gives the same gesture with no edit control and
therefore no `CBN_EDITCHANGE` failure mode."~~ Struck through deliberately:
this is the sentence a later session would quote to reverse the reversal. The
dropdown survives, but as the accessible primary path beside capture, not
instead of it (§C.4).

**7.5a "The `msctls_hotkey32` objection applies to a capture field."** It does
not, and this entry exists so the refuted claim cannot be re-refuted. The
objection is about a control and a message queue. Do not remove capture on
that basis without re-testing it against the hook path.

**7.5b "Capture can replace the typed path."** No. Four modifier checkboxes
plus the key dropdown stay primary: they are the only path for someone who
cannot physically produce the chord, and they are the only path that works
when `SetWindowsHookExW` fails. PowerToys Keyboard Manager makes the dropdown
the default and capture the shortcut, and its own bug list is evidence that
capture is hard even with a typed path present.

**7.6 "`SetWindowTheme(list, \"Explorer\")` is a one-line native-look win."**
Wrong twice: without a manifest it is a no-op because the control is not
themed at all, and Windows 11's rounded selection backplate comes from the
`"ItemsView"` class — `"Explorer"` gives the Vista/7 look.

**7.7 "The missing `WM_CTLCOLORSTATIC` handler is a bug."** `DefWindowProcW`
already returns the `COLOR_BTNFACE` brush and the class background is the same
brush. Adding a handler that returns it again is a no-op.

**7.8 "`Segoe UI Variable` exposes three GDI family names."** Nine.

**7.9 "The system font on Windows 11 is Segoe UI Variable."**
`SPI_GETNONCLIENTMETRICS` still returns plain Segoe UI at 9 pt.

**7.10 "Guard against `LNK4078` with `/MANIFEST:NO`."** `LNK4078` is about
duplicate section names; rustc passes no manifest flags.

**7.11 "GDI rounded corners are always jagged, and worse at high DPI."**
Backwards — at 150 % and 200 % the same radius is sampled by more, smaller
physical pixels, so aliasing decreases. Three antialiased paths ship in-box.

**7.12 "Read UserAssist for the most-used apps."** The best signal available,
refused on optics — and separately disabled by `Start_TrackProgs = 0`, which
the Scoop-installing audience is likeliest to have set.

**7.13 "Seed the starter config from detected apps."** See §E.2.

**7.14 "Use `Taskband\Favorites` for pin order."** Undocumented PIDL blob,
shape changes between builds, reported gone in 24H2.

**7.15 — REFUTED 2026-08-11, by measurement. "A populated `CBS_DROPDOWN`
rewrites its own edit text as you type, and the `CBN_EDITCHANGE` that arrives
carries the text from before the rewrite."** This spec asserted it in §B.7 and
the code asserted it in four comments; it is false, and it cost a day. It was
inferred from the outside-in symptom (typing "Notepad" left `"d"` in the
config and "Debuggable Package Manager" on screen) and never checked, and the
fix it motivated — deferring the `CBN_EDITCHANGE` read through a posted
message — produced a byte-identical failure on hardware, because the read was
never wrong.

What is true: a `CBS_DROPDOWN` does **not** autocomplete while you type.
`crates/beckon-windows/examples/combo_probe.rs` builds the control in-process
with beckon's exact styles, subclasses its child EDIT, and reports the field
holding exactly what was typed, `CB_GETCURSEL` at -1, and the EDIT receiving
nothing but `WM_KEYDOWN`/`WM_CHAR` — no `WM_SETTEXT`, no `EM_REPLACESEL`, no
`EM_SETSEL`. It ran with an empty combo and a plain EDIT as controls, under
comctl32 **5.82 and 6.16** (same binary, manifest stamped by `mt.exe`), in
session 1 with real focus and `SendInput` keystrokes — because a control that
never runs and a clean result look identical.

The control *does* re-synchronise its edit field to the closest matching item,
and select the whole string, when it is **resized**. `apply_state` ran on every
keystroke and ended with an unconditional `layout`, which `SetWindowPos`es
every control — so each character was replaced by a catalogue entry and
reselected, leaving the next character to replace the lot. `combo_probe`'s
`ModelLoopWithLayout` scenario reproduces the exact `"d"` signature from first
principles, with the no-layout run beside it as the control. The fix is
`Ui::shown_external` (banner visibility) plus `Ui::shown_empty` (the list's
row height, the fourth input to `layout`). `layout` has a **fifth** input —
the list's own client width, which loses `SM_CXVSCROLL` when the ListView
grows a scroll bar — and it is deliberately unguarded: its error is always a
stale gutter, never a clipped column, and guarding it would put `layout`, and
so `SetWindowPos` on the combo, back on more data pushes. The argument is
written out at the column sizing inside `layout`.

Two things follow, and both are the reason this entry is long. **Do not
re-derive the old mechanism from a gap** — every site that stated it now says
what was believed and what replaced it. And **`WM_APP_EDITED`, `Ui::app_epoch`
and the deferred read are now deferred debt**, kept only because collapsing
them would have to re-establish the `CBN_CLOSEUP` ordering `05db60b` fixed;
their survival is not evidence they are load-bearing. Full record:
`docs/superpowers/measurements/2026-08-11-landing-1-a14.md` §24–26 and
`.superpowers/sdd/2026-08-11-settings-window-landing-2a/combo-investigation.md`.
