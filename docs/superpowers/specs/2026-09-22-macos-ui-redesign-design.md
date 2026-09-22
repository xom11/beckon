# macOS UI redesign — menu bar, Settings, recorder, onboarding, keyboard map

Status: **design, approved in chat 2026-09-22; nothing built.** Three mockup
rounds were reviewed as an interactive page on claude.ai (private to the
author): round 1 listed every binding inline in the menu and was rejected as
too long; round 2 moved the list into a submenu; round 3 added the keyboard
map. The page renders the author's real 19-line `apps.toml` and the output of
`beckon check --resolve` on airm3 the same day.

## 0. The decision in one paragraph

On macOS, `serve`'s menu bar item becomes a short native `NSMenu`. It has four
parts: a header view with a status dot and a *Shortcuts on* switch; a *Needs
attention* section that exists only while some binding is broken; a
*Shortcuts ▸* submenu that is the whole table as a cheat sheet; and the
commands. The Settings window keeps its four `Page`s but moves to an
`NSToolbar` preference window with grouped forms, glyph keycaps (`⌃⌥⇧⌘`), a
shortcut **recorder** as the primary input, a resolve line under the App
field, and auto-save with Undo in place of Save / Close. A first-run Welcome
window, a menu warning and a Settings banner cover the two permissions. A new,
opt-in `keyboard.cheatsheet` chord (default `⇪?`) shows a keyboard-shaped HUD
of every binding. Every decision that is not drawing lands in `beckon-core`,
so Windows can follow in its own round without re-deciding anything.

## 1. Scope

**In:** macOS `beckon serve` — `tray.rs`, `settings_window/*`, one new HUD
panel, the permission surfaces — plus the `beckon-core` contract each of
them needs.

**Out, on purpose:**

- **Windows rendering.** Windows keeps today's menu and window this round.
  Core changes must leave its output byte-identical, and a test must pin
  that (§10).
- **Linux.** It has no `serve`. `keyboard.cheatsheet` parses there and is
  inert, like `keyboard.caps`. The author's nix generator reads this file at
  eval time to emit compositor bindings (see the header comment of
  `apps.toml`). It must already skip `keyboard.*` for `keyboard.caps`;
  **verify it also skips the new key** before the phase-6 merge, or the next
  rebuild binds a chord to an app called `ctrl+super+alt+shift+slash`.
- **Launching from any list.** Menu rows, submenu rows and HUD tiles never
  focus or launch anything. `CLAUDE.md`'s *"Nothing in the shortcut table
  focuses or launches anything"* extends to every new surface.
- **A search panel** (Maccy's `NSPanel` popup). See §11.
- **A `Start at login` row.** It is still omitted, not greyed: the Homebrew
  `service do` block owns the launch agent.
- **Running-state indicators** on rows.
- **Non-QWERTY labels on the keyboard map** (§4.4).

## 2. What was taken from Maccy

Read from source at `p0deje/Maccy` `c376789` (SwiftUI). Rectangle, rcmd and
Raycast were read from memory. No Claude Code skill or MCP server targets
menu bar apps specifically: the ones found are HIG summaries written for
SwiftUI, and `sosumi.ai` serves the HIG pages themselves.

| Maccy | beckon | why |
|---|---|---|
| 15×15 app icon leading a row, shortcut hint right-aligned at 70% (`ListItemView.swift`, `KeyboardShortcutView.swift`) | taken | This is the anatomy of a cheat sheet. Rectangle's menu is built the same way. |
| ⌥-click the icon to pause; the icon dims (`appearsDisabled`); VoiceOver says "paused" (`AppDelegate.swift:185-192`) | taken | Pausing is the most common action and should not cost opening the menu. |
| Toolbar-tab Settings with SF Symbols; min width derived from the tab labels (`sindresorhus/Settings`) | taken, as plain AppKit | `NSToolbar` with `toolbarStyle = .preference` needs no Swift library. |
| `KeyboardShortcuts.Recorder` | the form, not the code | The mechanism is `caps_tap::begin_capture`, which beckon already has. |
| Floating `NSPanel` popup with a search field (`FloatingPanel.swift`) | rejected as the menu (§11) | Built for hundreds of clipboard items. It also re-implements what `NSMenu` gives for free. |
| No permission onboarding (`Accessibility.check()` is empty) | not followed | A grant lost on rebuild is the largest single cause of "beckon does nothing". |
| rcmd assigns a letter from the app's name | taken | `+` pre-fills the first free letter of the new app's name (§5.4). |

## 3. The menu bar

### 3.1 Shape

```
● beckon                        (on)      <- header view (NSMenuItem.view)
  19 shortcuts · 2 missing
─────────────────────────────────
Needs attention                           <- only while a row is broken
  ? com.nousresearch.hermes  missing  ⇪H
  T Tao Monitor              missing  ⇪J
─────────────────────────────────
Shortcuts                          ›      <- submenu: every binding
─────────────────────────────────
Settings…                         ⌘,
Check for Updates…
Reload Config                     ⌘R
Open Log                                  <- only with --log, as today
─────────────────────────────────
Quit beckon                       ⌘Q
```

When nothing is broken the menu is nine rows. **It does not grow with the
file.** That was the objection to round 1, where 19 inline rows made a menu
about 600 pt tall that gained a row per binding.

### 3.2 Rows

- **Header** is an `NSMenuItem.view`: a status dot, `beckon`, a subtitle and
  an `NSSwitch`. The switch calls `serve.rs`'s `set_paused` through the same
  path `MENU_PAUSE` uses today (`serve.rs:1383-1427`). Pause is **never
  re-implemented**. The subtitle comes from one core function,
  `menu_headline`, with this precedence:
  1. `Paused — shortcuts are off`
  2. `Needs Accessibility to switch windows`
  3. the registration-failure phrase `registration_phrase` produces today
     (`serve.rs:700`)
  4. `N shortcuts · M missing`
  5. `N shortcuts`

  The dot is grey for 1, orange for 2 and 3, green otherwise.
- **Needs attention** is a section header plus at most three binding rows
  whose `row_condition` flag is `missing` or `in use`. More than three gives
  one extra row, `and K more…`.
  - It is absent while paused: `paused` outranks every word, and the header
    already says it.
  - `other chord` never qualifies. It is a view fact, not a fault
    (`docs/notes/settings-window.md`).
  - `NSMenuItem.sectionHeader(title:)` exists only from macOS 14, and
    `assets/macos/Info.plist:32` says `LSMinimumSystemVersion 11.0`. Below
    14, use a disabled row with a small secondary-coloured title.
- **Shortcuts ▸** lists every binding in file order. Each row carries the
  resolved app's icon, the winning candidate's name, the flag if any, and the
  chord right-aligned. The chord is drawn with a right tab stop in an
  attributed title, because `NSMenu` has no ⇪ modifier mask to put in a key
  equivalent. The submenu's footer has `Show Keyboard Map` (with `⇪?` when
  that chord is set) and `Edit Shortcuts…`.
- **Clicking any binding row**, in the section or the submenu, opens Settings
  on Shortcuts with that model row selected. It is carried like
  `pending_update_check` today (`serve.rs`, `MENU_UPDATE` arm): a
  `pending_select: Option<usize>` that `open_settings` consumes.
- **`Pause hotkeys` loses its checkbox row.** The header switch replaces it.
- Key equivalents (`⌘,`, `⌘R`, `⌘Q`) act only while the menu is open. In a
  status item that is harmless and conventional.

### 3.3 The status item

- **⌥-click toggles pause without opening the menu.** Today the menu is
  attached with `item.setMenu` (`tray.rs:282`), so every click opens it and no
  modifier can be seen. The button needs an action with
  `sendActionOn(leftMouseUp | rightMouseUp)`. The action pops the menu itself
  unless ⌥ is down. **Probe first**: popping a status item's menu by hand
  has known pitfalls around highlight state.
- **While paused**, `button.appearsDisabled = true` and the accessibility
  label reads `beckon — paused`.
- `set_status` still sets the tooltip.

### 3.4 Contract: `beckon_core::menu`

`MenuEntry` today is `{id, label, checked, enabled}` (`menu.rs:14-22`). It has
no images, submenus, key equivalents or views. This design extends it with:

- `kind`: `Item | SectionHeader | Header(Header { title, subtitle, dot, on })
  | Submenu`. A separator stays "a plain `Item` with an empty label".
- `detail: Option<String>`: the right-aligned chord text
- `key: Option<char>`: a key equivalent, always with ⌘
- `icon: Option<String>`: a bundle id, looked up through
  `NSWorkspace.URLForApplicationWithBundleIdentifier` then `iconForFile`
- `flag: Option<&'static str>`: one of `row_condition`'s words
- `tooltip: Option<String>`: the row in words, for hover and VoiceOver
- `children: Vec<MenuEntry>`: a submenu's rows

Two rules follow from that:

- **The new shape has its own composer, `build_mac_entries`.**
  `build_entries` is Windows', and it changes only by
  `..MenuEntry::default()` in its struct literals, so its existing tests are
  what pin the Windows menu. `beckon_windows::hotkey` ignores the new fields
  and never receives the new kinds. (The phase-1 plan chose a separate
  function over a `macos` branch inside `build_entries`: an unchanged
  function is a stronger guarantee than a branch.)
- **Binding row ids** are `MENU_BINDING_BASE + model_row`. The menu is rebuilt
  on every open (`menuNeedsUpdate:`, `tray.rs:84`), so an id always names a
  row of the current model.

### 3.5 Icons and display names: the cost

Each row needs the binding's resolved winner, for its icon and its real name.
That resolution runs **when the config loads or reloads, never when the menu
opens**. The result is cached per config generation and falls out of the same
catalog scan the settings window already does (`spawn_catalog_scan`,
`serve.rs:1720`). On macOS that scan is synchronous; this design must not add
a second one on menu open. Until the cache is filled, rows draw with the
generic application icon. The hot path, `beckon <id>`, is untouched.

## 4. The keyboard map (`⇪?`)

### 4.1 Config

```toml
keyboard.cheatsheet = "ctrl+super+alt+shift+slash"   # ⇪?  — absent = off
```

- It is a new field on `KeyboardConfig` (`shortcuts.rs:761`), a
  `cheatsheet: Option<Combo>` next to `caps`, `caps_tap` and `caps_hold`.
- It is written as a **dotted key, never a `[keyboard]` header**
  (`config_write.rs:94`).
- It is **not a binding line**, because the right-hand side of a binding is
  always an app Name. A magic Name (say `"@cheatsheet"`) would collide with
  Name-first resolution.
- It is **default off**, because beckon registers only chords the file names.
  The Settings switch writes the line, and turning it off removes it.
- The chord joins the duplicate check in `parse_config` (`shortcuts.rs:797`),
  so binding it to the same chord as an app is the parse error a chord bound
  twice already is. That makes `beckon check` validate it with no new code
  path.
- This is not a verb and not a flag, so the growth rule is not engaged.

### 4.2 Registration

`serve` registers the chord through the same `HotkeyManager` as bindings. Its
action is `show_keyboard_map()`, never `backend.beckon()`.

- **Pausing unregisters it too.** `set_paused` clears the whole map, and
  nothing special-cases this chord.
- **It must be in `caps::bound_keys_mac`'s set** (`caps.rs:170`), or
  Caps+Shift+/ is not injected at all. Shift bindings reach through Caps by
  design (`a_shift_binding_on_the_chord_is_still_reachable`), so `⇪?` works
  once it is in the bound set.

### 4.3 The panel

- An `NSPanel` with `.nonactivatingPanel`, a `.hudWindow` visual effect
  background and `[.canJoinAllSpaces, .fullScreenAuxiliary, .transient]`. It
  is centred on the screen of the key window, or of the mouse when no window
  is key.
- It **becomes key without activating beckon**, so Esc and `flagsChanged` (for
  the ⇧ layer) arrive while the frontmost app keeps focus. **Probe first.**
- **It toggles.** The chord shows it; the chord again, Esc, a click outside
  (`resignKey`) or any beckon hotkey firing closes it. Hold-to-show was
  rejected in §11.
- **Tiles are inert.** Clicking one does nothing.

### 4.4 Content

- **Layout:** rows `Q–P`, `A–L`, `Z–/`, then `Space`, positioned by the mac
  virtual key codes in `key_table()` and labelled by `key_label`. On a
  non-QWERTY layout the positions stay physically correct and the labels may
  drift. That is accepted for round one.
- **Tiles:** a key with a binding on the hold chord (`keyboard.caps_hold`,
  default `ctrl+super+alt`) gets a tile with the app's icon and short name.
  A `+shift` binding on the same key shows as a small `⇧` badge in the tile's
  corner. Holding ⇧ swaps which layer is the tile and which is the badge. A
  `missing` row gets an orange dot. A key with no tile is free, and seeing
  that is half the point: it is where a new binding can go.
- **This map's own chord** appears on the ⇧ layer as a `?` tile.
- **Bindings on other chords** are not drawn. The footer counts them:
  `3 shortcuts on other chords — see Settings`.

### 4.5 Where it is reached

- The Keyboard page gets a row with a switch and a recorder for the chord
  (§5.6).
- The Shortcuts submenu's `Show Keyboard Map` works whether or not the chord
  is set.
- **Display:** `shift+slash` is drawn as `?`, the macOS convention (`⌘?` for
  Help). So the chord reads `⇪?`, not `⇪⇧/`.

## 5. The Settings window

### 5.1 Shell

- **Tabs:** an `NSToolbar` with `toolbarStyle = .preference` replaces the
  `NSSegmentedControl` tab strip (`mod.rs:1853-1871`). It has four items:
  Shortcuts (`command`), Keyboard (`keyboard`), General (`gearshape`) and
  About (`info.circle`). The symbols come from
  `NSImage(systemSymbolName:accessibilityDescription:)`, which is available
  on macOS 11. The window title is the open page's name.
- **`Page` is unchanged.** `Page::System` is labelled **General** on macOS
  through a per-platform label table, the same pattern as `ModifierLabels`.
- **Each page sizes the window to its own fitting height**, animated. The rule
  that `MIN_HEIGHT` is what the tallest door asks for (`mod.rs:132-162`) is
  retired on macOS.
- **The tab loses its warning mark.** `shortcuts_tab_label`'s count and " •"
  (`mod.rs:268`) cannot ride on a toolbar item. The footer's service line and
  the menu's *Needs attention* carry that fact instead.

### 5.2 Grouped form

- Rows sit in rounded groups: a label with optional secondary text on the
  left, the control on the right, and a hairline between rows. This is the
  AppKit form of what `page_plan`'s dividers already express on Windows.
- **Colours stay semantic `NSColor`s**, so there is still no Dark mode row on
  macOS.

### 5.3 Shortcuts page

- **Table columns:**
  - **App:** icon, the winning candidate's name, and `+N fallback` when the
    value is an `A || B` chain.
  - **Status:** the four words. `paused` is the exception: it becomes one
    banner above the table (`Shortcuts are paused. [Resume]`) with the rows
    dimmed, instead of the same word on every row. `row_condition`'s
    precedence does not change; only how `paused` is drawn changes.
  - **Shortcut:** glyph keycaps.
- **Ticks become multiple selection.** The tick column `COL_TICK` is replaced
  by `NSTableView` multiple selection, and the selected set is fed to
  `on_mark`. So "ticks win, the selection is the fallback" still holds: on
  macOS the selection is the tick set.
- **The filter still matches the App column only.**
- **Below the table:** `+` / `−` square buttons, and `Edit apps.toml…`, which
  sends `SettingsCommand::Open(Target::Config)`.
- **Editor:** an App field (an icon plus an `NSComboBox` of installed names)
  with a **resolve line** under it, then a Shortcut recorder with a `⋯` button
  (§5.4).
- **The resolve line** grades the selected row the way `check --resolve`
  does:
  - `✓ Exact` with the bundle id.
  - `~ Loose` with the winner and the hazard. Its action,
    `Use “<winner name>”`, writes the exact name.
  - `⚑ N apps`, listing the rivals. Choosing one writes that app's canonical
    bundle id in place of the Name, the remedy `check` already prints.
  - `✕ No match`, with the reason.
- **The resolve line needs data the window does not have.** Today
  `RuntimeStatus` carries only catalog names, and `catalog_hit` re-derives
  Exact or Loose from them (`settings.rs:2371`). This design adds a
  selected-row-only `resolution: Option<NameReport>`, computed by the same
  function `check --resolve` calls and requested on selection. It is not
  computed for all rows.

### 5.4 The recorder

- **The recorder is the primary input.** It replaces the `Record` / `Stop`
  button. Clicking it calls `caps_tap::begin_capture` (`caps_tap.rs:550`). A
  captured chord goes through the existing `on_probe_shortcut` then
  `on_edit_combo` path (`mod.rs:538`). Esc cancels.
- **The `⋯` popover holds today's typed path, unchanged:** the modifier
  segmented control and the key popup filled from `key_table()` in order,
  never sorted, because `ComboView::key` is the index. Anyone who cannot
  produce a chord, and every key capture cannot see (a bare Esc, a bare Tab),
  still has it. **This narrows the notes' "the typed path stays primary":**
  both paths remain, and only their order on screen changes.
- **Capture needs Input Monitoring** (`begin_capture` raises that prompt). If
  it is not granted, the recorder reads `Needs Input Monitoring` with an
  action, and `⋯` works without it.
- **Registered hotkeys are not suspended during capture today.** A chord like
  `⇪C` is expected to be swallowed by the head-inserted session tap before
  `RegisterEventHotKey` sees it, but that is inferred, not measured on macOS.
  **Probe first**; if it fails, suspend registration for the length of a
  recording.
- **Conflict:** `probe_plan`'s "other rows" step marks the recorder red with
  `⇪D already opens Discord.`, and auto-save refuses the write (§5.5).
- **`+` suggests a key.** A new pure core function,
  `suggest_key(name, bindings, hold) -> Option<Key>`, returns the first
  letter of the app's name whose hold-chord combination is still free in the
  file. The recorder is pre-filled with it.

### 5.5 Auto-save and Undo

- The design is **`2026-08-14-four-doors-settings-window-design.md` §6**,
  written for Windows and not built anywhere yet (`settings.rs:627`,
  `ids.rs:117`; `SettingsCommand::Undo` is an empty arm at `serve.rs:2506`).
  It is implemented in `beckon-core`, and macOS consumes it first.
- On macOS, `Save`, `Close` and `Open config file` leave the command bar.
  Because Windows still draws them, `command_bar_shown` becomes
  platform-aware rather than changing for both.
- **Footer:** the service line on the left. On the right, one of
  `Saved just now · Undo`, `Not saved — <reason>`, or blank when the config
  does not parse.
- **Every write still goes through `write_config_text`**, which resolves a
  symlinked config before renaming.
  `saving_through_a_symlink_writes_the_target_and_keeps_the_link` must stay
  green.
- Close-while-incomplete and the external-change banner follow §6 there. They
  are not re-decided here.

### 5.6 Keyboard page

- **Caps Lock:** the use-Caps switch; the hold chord as keycaps with
  `Change…`; and the tap popup (`Toggles Caps Lock / Sends Escape / Does
  nothing`).
- **Show shortcuts as:** a segmented `⇪ C | ⌃⌥⌘C`.
  - **The store already exists.** `beckon_macos::prefs` keeps `CapsView`,
    beside `Opacity`, in `NSUserDefaults` domain `com.xom11.beckon`. Today's
    shorthand switch writes it (`settings_window/mod.rs:859`), and the
    segmented control writes the same key.
  - `SettingsCommand::SetCapsShorthand` stays an empty arm on macOS
    (`serve.rs:2504`), because the window writes the preference itself.
    **CORRECTED 2026-09-22:** an earlier draft of this spec called that arm a
    missing store. It is not.
- **Keyboard map:** a switch plus a recorder for `keyboard.cheatsheet` (§4).
- **Input Monitoring:** its state, and `Open Settings…` through
  `shell::open_input_monitoring`.

### 5.7 General page (`Page::System`)

- `Shortcuts on` sends `SettingsCommand::SetPaused`, and the Configuration row
  (load time, `Reload`) sends `ReloadNow`. As today, neither is
  re-implemented in the window.
- The Files group lists the config and the log, each with Reveal and Open. The
  log row is omitted without `--log`.
- The transparency row behaves exactly as today.

### 5.8 About page

- **Header:** icon, `Version <v> (<sha>)` and `Check for Updates…`, the
  existing flow.
- **Permissions:** Accessibility and Input Monitoring, each with its state
  and `Allow…` / `Open Settings…`. Wording comes from `accessibility_warning`
  and `input_monitoring_warning` as today.
- **This copy:**
  - The running image's path is still **not resolved** through the junction.
    That surface is the one that lied (`CLAUDE.md`).
  - It shows start time against mtime.
  - The Command row has `Copy`, which fills the `SettingsCommand::Copy` empty
    arm.
- **Links.**

## 6. Permissions and onboarding

- **Welcome window.** It is shown at `serve` start when
  `!is_accessibility_trusted()` and the defaults key `WelcomeShown` is unset.
  - An Accessibility row calls `request_accessibility()`.
  - An Input Monitoring row calls `request_input_monitoring()`, and is shown
    only when `keyboard.caps` is on.
  - Both rows re-check once a second while the window is open, since AX posts
    no reliable notification.
  - `Done` is enabled once Accessibility is granted. `Later` closes the window
    and sets the key.
- **The ask always comes with the pane.** macOS shows its prompt only when no
  answer is recorded, so every `Allow…` has an `Open Privacy & Security`
  beside it. This is the existing rule for `request_accessibility`, not a new
  one.
- **Menu:** an orange header dot, subtitle 2 of §3.2, and an
  `Allow Accessibility…` row directly under the header.
- **Settings:** a banner on every page while untrusted.
- **Trust is re-read on every menu open** (`menuNeedsUpdate:`) and never
  cached. Every rebuild invalidates it.

## 7. Platform strings

- **Glyphs:** a new core renderer, `combo_glyphs(combo, folded: bool)`,
  produces `⌃⌥⇧⌘T` in macOS order, `⇪T` when folded, and `?` for
  `shift+slash`. macOS uses it for every cell, menu row and keycap.
  `ModifierLabels::MAC`'s words (`shortcuts.rs:340`) remain for accessibility
  labels and tooltips.
- **Words stay on Windows.**
- **Page label:** `General` on macOS (§5.1).

## 8. Decisions this reverses or narrows (macOS only)

| today (recorded in repo) | now | cost / reason |
|---|---|---|
| Words `Cmd` / `Option`, not glyphs (`settings-window.md`, "Platform strings") | Glyphs `⌃⌥⇧⌘` | The stated reason was the editor's check boxes reading `Cmd`. The recorder replaces them as the primary input, and `NSMenu` draws key equivalents as glyphs. |
| Command bar Save / Close / Open config file | Auto-save and Undo | Design §6, macOS first. HIG: settings apply immediately. |
| "The typed path stays primary; capture is an accelerator" | Recorder primary, typed path behind `⋯` | Both paths remain; only the order on screen changes. |
| Tabs are an `NSSegmentedControl` | `NSToolbar` `.preference` | The platform convention. It also retires the tallest-door `MIN_HEIGHT` rule. |
| `paused` drawn on every row | One banner, rows dimmed | Precedence unchanged. Nineteen identical words were noise. |
| The menu carries commands only | *Needs attention* plus the *Shortcuts* submenu | Rows open Settings and never launch. `MenuEntry` grows kinds (§3.4); Windows is unchanged. |
| The only GUI exception is `serve`'s tray and Settings window (`CLAUDE.md`, "Out of scope") | Plus the keyboard map HUD, still part of `serve` | Display only. Opt-in through one TOML line, so beckon still registers only chords the file names. |
| `Page::System` labelled "System" | Labelled "General" on macOS | Label table only; `Page` unchanged. |
| `CLAUDE.md`, "What beckon reads and writes", names the TOML and the Windows registry only | `WelcomeShown` joins `Opacity` and `CapsView` in `beckon_macos::prefs` (`NSUserDefaults` `com.xom11.beckon`) | That domain already exists and the paragraph omits it. It gains the macOS list, which is also the reset list. |

**Kept, and must stay:**

- The four status words and their precedence.
- Omitted-not-greyed.
- `set_paused` and `reload` reached only through `SettingsCommand` / the tray
  arm.
- `toml_edit` writes that keep comments.
- The filter matching the App column only.
- The key list in `key_table()` order.
- The unresolved running-image path.
- The one-network-request rule for update checks.

`CLAUDE.md`'s *Out of scope → GUI / TUI* paragraph and the notes' affected
entries are updated **in the same commits** that change each behaviour, each
marked NARROWED or CORRECTED per the repo's convention.

## 9. Phases

Each phase is its own branch, mergeable and usable alone. *Probe* is what must
be run on a real Aqua session before code is written. SSH lands in a session
with no window server (`tray.rs` module doc).

| phase | builds | touches | probe first |
|---|---|---|---|
| 1 · Menu | header view and switch, *Needs attention*, *Shortcuts* submenu with icons and chords, ⌥-click, dim when paused | `core::menu`, `serve.rs` `build_entries`, `tray.rs` | Does a click on an `NSSwitch` inside `NSMenuItem.view` keep the menu open? A right-tab-stop chord column in an attributed title. Manual menu pop from a button action (highlight state). |
| 2 · Settings shell | toolbar, grouped form, per-page height, footer | `settings_window/*` | Resize animation between pages. Toolbar rendering on macOS 26. |
| 3 · Auto-save + Undo | four-doors §6 in core, consumed by macOS | `core::settings`, `config_write` | The symlink test stays green. |
| 4 · Recorder + App field | recorder, `⋯` popover, resolve line, `suggest_key`, multi-select | `core::capture`, `certainty`, `candidates`, `settings_window/mod.rs` | Does a registered chord reach the recorder during capture, or fire its app? |
| 5 · Onboarding | Welcome, banner, menu warning | `lib.rs` permissions, `serve.rs` | Welcome shows once. Polling stops when the window closes. |
| 6 · Keyboard map | `keyboard.cheatsheet`, HUD, ⇧ layer, submenu row, Keyboard page row | `core::shortcuts`, `config_write`, `caps.rs`, new panel module | Does a non-activating panel get Esc and `flagsChanged` without activating beckon? Does `⇪?` arrive through the Caps tap? |

Phases 1, 5 and 6 depend only on core pieces they introduce themselves.
Phase 4 needs phase 2's shell. Phase 3 is independent of the drawing.

## 10. Testing

**Core unit tests** (all three CI jobs):

- **Windows is unchanged:** `build_entries` itself is untouched apart from
  `..MenuEntry::default()`, and its existing tests stay green unmodified.
- **The macOS menu shape:** header, attention (zero, three, and more than
  three rows), submenu, and the `Open Log` omission.
- **`menu_headline`'s precedence**, one test per step.
- **`suggest_key`**, including a name whose letters are all taken.
- **`keyboard.cheatsheet`:**
  - parse, write and remove, always as a dotted key;
  - a round trip through a file with a hand-written `[keyboard]` header, the
    existing `config_write.rs:357` pattern;
  - a duplicate-chord parse error against a binding;
  - membership in `bound_keys_mac`.
- **`combo_glyphs`:** order, folding, and the `?` fold.
- **The auto-save state machine**, per four-doors §6.

**Live, on macOS** (extend `testing/macos_settings_drive.lua` and
`testing/macos_tray_probe.sh`):

- the switch pauses and resumes;
- ⌥-click pauses without opening the menu;
- `⇪?` shows the HUD, Esc closes it, and ⇧ flips the layer;
- a binding row opens Settings on that row.

**Always run a control** — for example, assert the HUD is *absent* before the
chord. A blind detector and a pass print the same thing.

**Gates:**

- The bare `cargo check --workspace --all-targets`.
- `cargo clippy --target aarch64-pc-windows-msvc --all-targets -- -D warnings`,
  because `MenuEntry`'s new fields compile into `beckon-windows`.
- CI's current stable toolchain, not a stale local one.

## 11. Rejected

- **Every binding inline in the main menu** (round 1). About 600 pt at 19
  bindings, a row more per binding, and the thing the author objected to.
- **A Maccy-style `NSPanel` popup with search.** Filtering 19 rows buys
  little, and the rows are not launch targets, so a filtered list has nothing
  to "pick". It would re-implement keyboard navigation, VoiceOver, dismissal
  and multi-display placement that `NSMenu` already has.
- **A SwiftUI helper or a WKWebView UI.** A second toolchain in nix and CI, an
  IPC protocol, and a second binary to grant permissions to. WebView2 was
  rejected on Windows for the same feel reasons.
- **Rows that launch.** They would reverse the table rule, and the hotkey
  already does it.
- **A magic app Name for the cheat sheet.** It collides with Name-first
  resolution (§4.1).
- **A hold-to-show HUD.** Key-up through the Caps tap's injection is not a
  semantics beckon has measured. Toggle is unambiguous.
- **A default-on cheat sheet chord.** It would register a chord the file does
  not name.
- **Running-state dots** on rows. They cost a resolution on every open for a
  fact the hotkey itself reveals.
