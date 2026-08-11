# beckon-serve settings window: foundation, redesign, beckon key, suggestions

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
| New config keys | Exactly one: `keyboard.beckon_key` | Per-OS overrides; a `[keyboard]` header |
| Beckon key scope | Configurable, including for the Caps hook; `shift` refused while Caps is on | Freezing the hook's chord (§7.4); allowing shift unconditionally |
| Shortcut editing | Key picked from a `CBS_DROPDOWNLIST` of the 81 canonical keys | Chord capture; single-key capture (§3.4) |
| Layout | Five horizontal bands, app-name first, in-line editor | List + detail pane; a scrolling card column; an editable grid |
| Suggestions | A chip row in the window, from taskbar pins + open windows | Seeding the starter file; a first-run wizard; UserAssist (§5) |

## Landing order

Three landings, and the order is load-bearing rather than a preference.

| Landing | Contents | Why here |
|---|---|---|
| **1** | Part A entire, plus Part D | The manifest changes what every constant in Part B *means*, so tuning spacing or fonts before it is on hardware is tuning against metrics the shipped binary will never use. Part D rides along because §D.1 is a live defect and its measurement runs on the same a14 pass. |
| **2** | Part B, plus Part C | The window redesign and the beckon key touch the same `layout` and the same `ControlState`; splitting them means writing the layout twice. |
| **3** | Part E | Suggestions depend on nothing above and are the easiest thing to cut. |

**§B.7's App-combo-box fix belongs in landing 2, ahead of Part E**, and the
sequencing is deliberate: the pain suggestions are aimed at ("I had to type
`Windows Terminal` and did not know that was the Name") is the same pain a
working autocomplete solves, for a quarter of the code. Ship the combo box
fix, then decide whether Part E is still wanted.

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
│  [banner: file changed on disk    (Reload) (Keep mine)]     │  only when needed
│  [ 🔍 Filter                                            ]   │  1
│ ┌─────────────────────────────────────────────────────────┐ │
│ │ App                                          Shortcut   │ │
│ │ Windows Terminal                      Ctrl Win Alt  T   │ │  2
│ │ File Explorer                         Ctrl Win Alt  E   │ │
│ │ Claude          ⚠ not installed       Ctrl Win Alt  C   │ │
│ └─────────────────────────────────────────────────────────┘ │
│  [ Claude              ▾ ] + [ C ▾ ]      [Remove] [+ Add]  │  3
│  ⚠ No installed app is named "Claude".                      │
│  Suggested  [+ VS Code] [+ Brave] [+ Notion] [+ Spotify]    │  4
│  Beckon key  [Ctrl][Win][Alt][Shift]  │  ☐ Caps Lock too    │  5
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
- **`Segoe UI Variable` exposes nine GDI family names, not three**:
  {Small, Text, Display} × {—, ` Light`, ` Semib`}, where "Semib" is
  truncated to fit `lfFaceName`'s 32-wchar buffer. Code that enumerates
  expecting three matches misses six.

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
| — | `custom` | A chord other than the beckon key |

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
    pub row: Option<usize>,   // None = file-scope (beckon key, caps)
    pub severity: Severity,
    pub message: String,
}

// apply_enabled = m.dirty() && !problems.iter().any(|p| p.severity == Severity::Error)
```

Two consequences that are the point of the change:

- A warning (a single-modifier beckon key; a zero-modifier row) no longer
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
- **Fix the App combo box autocomplete.** A populated `CBS_DROPDOWN` rewrites
  its own text as you type and the `CBN_EDITCHANGE` that arrives carries the
  text from *before* the rewrite — measured on a14: typing "Notepad" wrote
  `"d"` to the config while the screen showed something else. `commit_fields`
  papers over it at Apply time; the field itself still lies while you type.
  This may be the highest felt-value item in the whole spec, because the pain
  it causes ("I had to type `Windows Terminal` and did not know that was the
  Name") is the pain the suggestions feature in Part E is also aimed at.

---

# Part C — the beckon key

## C.1 The config file does not change

Combos stay spelled out in full. Every existing config file, every README
example and every hand-edit keeps working, and there is no migration.

```toml
# Written ONLY when it differs from the default, so an untouched file stays
# readable by every older beckon binary.
keyboard.beckon_key = "ctrl+super+alt"   # default
keyboard.caps = true                      # Windows only
keyboard.caps_tap = "capslock"            # capslock | escape | none

"ctrl+super+alt+t" = "Windows Terminal"
"ctrl+super+alt+e" = "File Explorer"
"ctrl+super+alt+shift+t" = "Telegram Web"   # a different chord -> shown as `custom`
```

**The short form is derived, never stored.** A row is displayed and edited as
a single key whenever its *resolved* combo equals beckon key + one key. The
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
    pub beckon_key: Chord,   // new
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
| no modifiers | `` `keyboard.beckon_key` needs at least one modifier `` |
| unknown token | reuse `Combo::parse`'s message verbatim, via the shared `parse_modifiers` |
| duplicate modifier | likewise |
| `caps = true` **and** `beckon_key` contains `shift` | `` `keyboard.caps = true` cannot be combined with `shift` in `keyboard.beckon_key` — beckon has to press Shift for you, and releasing it would drop a Shift you are holding `` |

**Warnings** (printed by `check`, shown in the window, do not block):

| Condition | Message |
|---|---|
| exactly one modifier | ``beckon key `ctrl` alone takes Ctrl+T from every application`` |
| a row with zero modifiers | ``` `t` takes the T key in every application ``` |

In the window, the `Shift` chip is shown struck through with the reason
stated inline when Caps is ticked, rather than silently refusing the click.

## C.4 The key is picked from a closed list

The shortcut field becomes a `CBS_DROPDOWNLIST` of the 81 canonical key
names. An invalid combo becomes *unrepresentable*, which deletes a class of
errors instead of reporting them better.

`DROPDOWNLIST` rather than `DROPDOWN` is deliberate and is the opposite of
the App field, which must stay `DROPDOWN` because beckon supports apps with
no Start Menu entry. A `DROPDOWNLIST` has **no edit control**, so the
`CBN_EDITCHANGE` defect in §B.7 is structurally impossible in this field, and
its built-in typeahead already gives the gesture people want: press `t`, it
selects `t`.

**Chord capture stays refused**, and the reason is unchanged:
`msctls_hotkey32` cannot capture the Windows key, and Explorer consumes
`Win+T` and its siblings before a normal window sees them — exactly the
chords beckon recommends. Note that single-*key* capture is now technically
possible, since a bare `T` has none of those problems; it is still not taken,
because `DROPDOWNLIST` typeahead delivers the same gesture with no new
failure mode.

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
- **A `custom` row whose chord happens to equal the beckon key is included**,
  because the test is on the resolved modifier set, not on how the line was
  spelled. That is the correct reading: Caps stands in for the beckon key, and
  that binding uses the beckon key.

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

# Error handling

| Failure | Response |
|---|---|
| Manifest fails to compile into the binary | Release gate fails the build (§A.1.1). Never silent. |
| `keyboard.beckon_key` invalid | `parse_config` error; `check` exits non-zero; `reload` keeps the running keys and posts one throttled toast — the existing path |
| `caps = true` with `shift` in the beckon key | Hard error at parse; the window disables the Shift chip with the reason inline, so the state is unreachable through the UI |
| Beckon key with a single modifier | Warning, does not block. Save is allowed |
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
  `keyboard.beckon_key`; a non-default one must.
- Round trip, extended: every valid `Model` → TOML → `parse_config` → the
  same `Model`, now including `beckon_key`.
- The short-form predicate: a row spelled `ctrl+super+alt+t` and a row spelled
  `alt+ctrl+super+t` both resolve to short form under the default beckon key;
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
- **README**: the beckon key is configurable; the settings window screenshot;
  the suggestion source boundary stated plainly, because a user is entitled
  to know what beckon reads.
- **`2026-08-11-windows-settings-window-and-caps-design.md`**: a pointer at
  §A.5 to this spec.

---

# What this spec does not do

- **No dark mode.** Light only, with `GetSysColor` throughout so high-contrast
  themes work. See §7.2.
- **No chord capture, and no single-key capture either.**
- **No `beckon+t` shorthand in the config file.** See §7.3.
- **No config-file migration**, automatic or offered. The short form is
  derived from the resolved combo, so no file needs changing.
- **No per-OS `beckon_key` override.** One file, one beckon key. `parse_keyboard`
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

**7.5 "Borrow Raycast's Record Hotkey."** `msctls_hotkey32` cannot capture the
Windows key and Explorer eats `Win+T` before a normal window sees it.
Single-key capture *is* now possible, but `DROPDOWNLIST` typeahead gives the
same gesture with no edit control and therefore no `CBN_EDITCHANGE` failure
mode (§C.4).

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
