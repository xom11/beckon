# The settings window — decisions, corrections, and what must not be simplified

Extracted from `CLAUDE.md` 2026-08-17 (it lived under *Out of scope → GUI/TUI*
and had grown to ~560 lines there). The one-line rule in CLAUDE.md is still the
contract; this file is why each clause is worded the way it is.

Design specs: `docs/superpowers/specs/2026-08-11-windows-settings-window-and-caps-design.md`,
`2026-08-14-four-doors-settings-window-design.md`.

## Scope

CLI only, with one exception, which is `serve`'s control surface rather than a
launcher: the tray context menu (reload, pause, open the log, toggle autostart,
quit) and the settings window it opens.

**CORRECTED 2026-08-16: no longer "Windows-only".** The macOS window has all
four doors as of the four-doors port, against the same `beckon_core::settings`
contract — `Page`, `ControlState`, `Callbacks`, `SettingsCommand`,
`row_condition`, `probe_plan`, `command_bar_shown`, `caps_view_fold`,
`page_plan`. Everything below about *what the window decides* is therefore two
implementations of one design, and the place to change a decision is still
`beckon-core`.

## Where macOS deliberately differs

Each is a platform fact rather than a shortfall:

- **No `Dark mode` row.** Every colour in the AppKit window is a *semantic*
  `NSColor`, so it follows the system between light and dark with no control,
  no stored preference and no repaint path. The Win32 row exists because Win32
  has no appearance to follow — it needs `theme::pairs` and a `prefs.rs`
  registry value to do what `controlBackgroundColor()` does for free.
  `SystemState::dark` is read and discarded there.
- **No `Start at login` row**, by design §3.3's own rule that a capability this
  process does not have is omitted rather than greyed: the Homebrew formula's
  `service do` block owns the launch agent, and a switch here would be a second
  writer for a file beckon did not create. `SystemState::autostart` is `None`,
  which is the field's documented way of saying exactly that.
- **`NSSegmentedControl` for the tab strip**, not four hand-drawn pills. This
  closes a deviation rather than inheriting one — the design's own drawing
  shrink-wraps the trough around the pills, and Windows fills the band instead
  because hugging needs a width only its layout pass computes — and it brings
  the contrast, focus ring and keyboard story each Win32 pill state needed its
  own measurement to get right.
- **About draws an Accessibility row where Windows draws `HOOK_DISCLOSURE`.**
  With no `CGEventTap` built, *"the keyboard hook is installed only while…"*
  was vacuously true while telling the reader a keyboard hook is part of the
  program — worse than silence, on the one page whose job is disclosure. The
  Accessibility grant is this platform's version of the same question, and
  losing it silently on a rebuild is the largest single cause of "beckon does
  nothing" here.
- **The Keyboard door's first two groups are live here too, and what differs is
  the PERMISSION.** `beckon_macos::caps_tap` landed the same day and `serve`'s
  `sync_caps_hook` drives it from the same `keyboard.caps` / `caps_hold` /
  `caps_tap` settings the Windows hook reads. So the note under group 1 names
  **Input Monitoring** instead — a *separate* grant from Accessibility, in a
  separate System Settings pane, and without it the tap is created successfully
  and then receives nothing at all. That failure is silent, which makes it the
  one thing a reader cannot discover by ticking the box.

## Chord capture is on macOS too

**CORRECTED 2026-08-16.** `beckon_macos::caps_tap` grew a capture arm consulted
BEFORE the Caps arm, for two reasons that are both ordering rather than taste:
a recording must work on a machine where the user left `keyboard.caps = false`,
and the Caps arm returns early on `reaches_nothing()` — so anything after it
would never run there; and if Caps ran first, a `Caps+T` mid-recording would
inject `ctrl+cmd+opt+T` and the recorder would write down the ALIAS instead of
the key pressed.

**`capture::step` is REUSED rather than forked**, which is the opposite of
`caps::decide` and was the expensive half of the estimate before it was
measured. `crates/beckon-macos/examples/capture_probe.rs` on airm3 2026-08-16
answered the two questions that decided it: every keycode it saw was in
`key_table()` (which carries `mac` and `win` side by side), and a modifier's
edge reads straight off its own flag bit — unlike Caps, whose parity tracking
exists because suppression freezes the lock its flag reports.
`step_on(.., Platform::Mac)` is the one difference.

**`Platform` exists because the reserved-chord lists are not the same set and
one is not even the same KIND of refusal.** `Win+L` is `SystemChord` because
the hook cannot stop it; macOS stops `Cmd+Q` and `Ctrl+Cmd+Q` too WELL, so they
are `Reserved` — beckon's own limit, whose hint names no mechanism. macOS has
**no `SystemChord` members at all**, which is what keeps `HINT_SYSTEM_CHORD`
honest: its doc says naming `Win+L` in words is only truthful while that family
has one member. `ctrl+super+alt+q` stays recordable, because that is beckon's
own default chord shape and macOS quits only on `Cmd+Q` alone.

**Input Monitoring is per-BINARY and is NOT inherited from the terminal**,
unlike Accessibility — measured when `capture_probe`'s first run had no row of
its own, received nothing, and looked exactly like "macOS refuses to suppress
`Cmd+Q`". `IOHIDCheckAccess` only asks and never prompts, so a binary with no
row cannot acquire one through it. **Every fresh `cargo build` therefore loses
the tap's grant**, which is the same shape as Accessibility losing its
code-signature identity but a different pane and mechanism.

The About page's disclosure sentence gained `or while you are recording a
shortcut` in the same pass. Without it the sentence is a false claim about when
beckon can see keystrokes, on the one page whose job is disclosure.

## Platform strings are tables, not literals

**Modifier names.** `beckon_core::shortcuts::ModifierLabels` — `WINDOWS` is
`Ctrl/Win/Alt/Shift` and `MAC` is `Ctrl/Cmd/Option/Shift`. `combo_caps`,
`combo_caps_folded` and `combo_display_folded` keep their signatures and
delegate to `*_with(.., WINDOWS)`, so no Windows string moved;
`the_default_labels_are_what_combo_caps_always_produced` pins that. Words
rather than glyphs (`Cmd`, not `⌘`): the editor's own check boxes read `Cmd`,
and a cell showing a symbol beside a box showing a word is two names for one
key on one screen. `key_label` was already neutral.

**CORRECTED 2026-08-16: the platform string in `theme.rs` is a table too, and
there is no local workaround left to name.** This paragraph used to end
*"`theme::TransparencyBlock::reason`'s `"Off in Windows settings"` is the one
string left in core that names a platform, worked around locally by
`beckon-macos`'s `block_reason` and wanting the same treatment"* — it got the
treatment. `theme::BlockReasons` is `ModifierLabels`' sibling, `WINDOWS` and
`MAC`; `reason()` delegates to `reason_with(BlockReasons::WINDOWS)`, so no
Windows string moved and `the_default_reasons_are_what_reason_always_returned`
pins that. The macOS System page calls `block.reason_with(BlockReasons::MAC)`
directly (`beckon-macos/src/settings_window/system.rs:199`), and grepping
`block_reason` in that crate now finds nothing — so a session going looking for
the workaround this entry promised finds an absence and cannot tell whether it
was deleted or never existed.

The macOS window is also where `on_command` is raised for the first time on
that platform: before the System and About doors, all eleven `SettingsCommand`
variants were unreachable there, and `open_target` / `reveal_target` were `{}`
stubs.

## What the window is, and is not

It shows the shortcut table with per-row registration state, edits it, and
writes the same TOML back through `toml_edit` so hand edits and window edits
stay interchangeable. It lists installed apps only to fill in a Name while
authoring a binding — the job `beckon search` already has — and never focuses
or launches anything.

**WIDENED 2026-08-15 — the window is `serve`'s control surface as well as its
editor.** Design §3.3's System page pauses and resumes the hotkeys, reloads the
config, toggles `Start with Windows`, sets the window's own theme and
transparency, and opens or reveals the config and log files. The paragraph
above is still true of the SHORTCUT table — nothing there focuses or launches —
and the additions are the tray menu's own commands reached from a page rather
than a menu: `Pause shortcuts` and `Reload` call `serve.rs`'s `set_paused` and
`reload`, the same two functions the tray calls, through `SettingsCommand`.
**They must never be re-implemented in the window**: `set_paused` does five
ordered things, one of which is CLEARING the registration map, and that cleared
map is what makes the `paused` status word load-bearing on every Shortcuts row.

`Start with Windows` is **omitted, not greyed**, under `beckon.exe serve`,
copying the tray's own reasoning — a capability this process does not have asks
"why is this greyed?" with no answer in the row. The log row is omitted the
same way when `serve` ran without `--log`. Both decisions live in
`beckon_core::settings::system_state`, so all three CI jobs test them.

**WIDENED AGAIN 2026-08-15 — the window also puts text on the clipboard and
opens a browser.** Design §3.4's About page has three copy buttons and three
links (`GitHub`, `Releases`, `Report a bug`). Two rules keep those from growing
into a second control surface:

- **The copy buttons act in the window and report afterwards**, like the theme
  switch and unlike everything else: `SettingsCommand` is `Copy + Eq` and
  carries no `String` by design, so a caller answering `Copy(Field)` would have
  to rebuild the page's state and be a second author for it.
  `beckon_core::settings::copy_text` is the one decision — **the row's bare
  payload, not the string on screen**, because `Location` shows a verdict
  clause and is shortened by `SS_PATHELLIPSIS`, and a copied path is for
  pasting into Explorer.
- **The links go through `SettingsCommand::Open(Target::…)`, never a new
  `Callbacks` field**: `beckon-macos/examples/settings_probe.rs` builds
  `Callbacks` as a complete literal with no `..`, so a new field is a hard
  E0063 on a CI job that has nothing to do with this page. The three addresses
  live in `Target::url` in core, where a test can read them, and
  `shell::open_url` refuses anything that is not `https://`.

**The hook disclosure lives on About**, moved off Keyboard by §3.4: *"The
keyboard hook is installed only while Caps Lock is on, or while you are
recording a shortcut. beckon keeps no record of what you type."* An unsigned
process that holds `WH_KEYBOARD_LL`, calls `SendInput` and writes an autorun
key owes the reader both halves, and **the second is a negative claim that no
icon, colour or control state can draw** — which is why it is a sentence and
why `HOOK_DISCLOSURE` has a test pinning both halves. "While Caps Lock is on"
means the SETTING, not the lock's LED.

## Shape: bands stacked top to bottom, not a split pane

Landing 2a, `settings_window.rs::layout`. The 45/55 column split it replaced
put 561 px of fixed columns inside a 482 px pane, so beckon shipped a
horizontal scroll bar and a clipped App column; widths are now a proportion of
the live list width, which is why that cannot recur.

**App leads, Shortcut follows** — the app is what the user is looking for.
Per-row `LVS_EX_CHECKBOXES` ride in column 0's state image and make Remove a
multi-delete: the whole decision is `Model::remove_pressed` — **ticks win, the
selection is the fallback** — because clicking a tick also moves the highlight,
so a selection-only Remove would delete a row the user never ticked and leave
the ticked ones behind. `remove_enabled` is
`selected.is_some() || marked_count() > 0`.

The caption stays the constant `Remove` rather than `Remove N`: `layout` sizes
buttons from `text_size` of their caption, so a live count would be a further
`layout` input, and calling `layout` on a data push means `SetWindowPos` on the
App combo — the measured data-loss path. That is not the only route to a live
count — reserving width for the widest caption at `layout` time and driving the
count with `SetWindowTextW` alone on pushes would honour it without `layout` or
`SetWindowPos` — just one not taken this pass.

`Save` (was `Apply`; the id is still `IDC_APPLY`, because
`examples/settings_probe.rs` hard-codes 1002-1007) is `BS_DEFPUSHBUTTON` and is
where the default ring RESTS — **not where it stays**. `default_button_of`
migrates the ring onto whichever push button has focus, so Enter saves from the
fields, the list and the check boxes, but Enter on a tabbed-to `Close` closes
and on `Reload` reloads. That is the point of two earlier fixes: Enter on a
focused `Reload` used to save and overwrite the external change the banner
existed to protect.

**CORRECTED 2026-08-15: "Only `Ctrl+S` is unconditional" is no longer true, and
it was the defect rather than the design.** `Ctrl+S` is an accelerator on the
WINDOW, so from the System or About door it reached `handle_command`'s
`IDC_APPLY` arm and wrote `apps.toml` with no Save on screen — two doors that
write no part of that file. `enabled()` could not have stopped it:
`apply_enabled` is `dirty && no errors` with no page term, and a hidden button
is not a disabled one. It is inert on those two doors now; the model stays
dirty, so the keystroke is waiting one `Ctrl+1` away rather than lost.

## The command bar follows design §1's split by STORE

`beckon_core::settings::command_bar_shown`, from `Page::writes_config`: `Save`
/ `Close` / `Open config file` are drawn on Shortcuts and Keyboard, and on
neither of the other two. `Close` and `Open config file` go with `Save` rather
than staying — `Close` is where "discard" lives (the dirty prompt is in
`WM_CLOSE`) and `Open config file` is a second route to a file the System page
already lists with its own two glyphs.

**The BAND stays on all four**: `compute_card_rects` reserves `pad + ctl`
whatever the page says, so `content_bottom` is one expression with one meaning,
and an empty bar carried no buttons and nothing else. **Since 2026-08-16 it is
not empty**: design §6.4's service line (`IDC_SERVICE_LINE`, 1044) is chrome
and is drawn on all four doors, so the band says whether the hotkeys are
working from every page. What §6.4 still owes is its right half — the `Saved`
readout and `Undo`, both of which belong to §6's auto-save.

Two consequences that are not obvious and are load-bearing:

- **`DefaultButton::HOME` is gone, replaced by
  `home(page) -> Option<DefaultButton>`**, and `default_button` returns
  `Option`. The old constant's own doc named this: *"`Save` is on every page —
  if it ever stops being, this early return is the line that breaks."* `None`
  is a real answer, not a missing one: System and About have no primary action,
  so Enter does nothing there until the user tabs onto a button. `NO_DEFAULT`
  (0) is the id that carries "nowhere" through `Ui::defid`.
- **`repair_default_button`'s successor is page-dependent.** It named
  `IDC_CLOSE` unconditionally — "always present" — which is now a hidden
  control on half the doors, and a repair that moves focus onto a hidden
  control is the exact fault it exists for. It falls back to the open door's
  own pill, which is `show_page`'s successor and is chrome.

Every door keeps two ways out: the caption `X` is chrome, and Escape arrives as
`IDCANCEL` from the dialog manager rather than from the button, so hiding
`Close` does not disarm the key.

## The band list and the row count were both wrong

**CORRECTED 2026-08-15 (branch `four-doors-phase-0`), each in its own way.**

The band list read *"Banner (external change; contributes no height when
hidden) / `Shortcuts` head with the filter, Remove and Add / the list / editor
strip / suggestion row (nothing built for it yet) / keyboard group / command
bar."* Two things falsify it. **The stack is page-dependent**:
`compute_card_rects` used to reserve the keyboard card's height on every page,
so Shortcuts carried a card-shaped hole above the command bar; the Shortcuts
door now stacks banner / list card / editor card down to the command bar and
the Keyboard door puts its own card at the same origin and nothing else. And
the **`Shortcuts` head has no heading in it** — the STATIC that said the word
(`IDC_LBL_SECTION`, 1020) sat in Subtitle directly beneath a tab pill captioned
`Shortcuts`, and design §3.1's drawing has no such heading. The row survives;
the filter leads it and Remove/Add close it.

The list read *"a **fixed eight rows** (`tok::ROWS`) at every DPI, measured
rather than scaled from a token, so it does not grow with the config."*
**`tok::ROWS` is deleted.** `list_h` was `want.min(room)` with
`want = list_header_height + row_h * ROWS`, i.e. a cap, and design §4 makes the
list take the room the page leaves and scroll. The cap had to go in the same
commit as the four deletions above it: they return 110 px at 96 DPI, and with
the cap in place every one of those pixels would have re-appeared as empty
space *below* the editor card — the same void, moved down the window. What
survives of it is the whole-row **snap** (`list_h = avail − avail % row_h`),
which is what keeps `Ui::shown_empty` guarding a real transition.

## `MIN_HEIGHT` is 480, `WINDOW_HEIGHT` is 500

**CORRECTED 2026-08-15**, and the freeze was lifted by arithmetic rather than
by preference. The first photographs of all four doors show the System card
ending 224 px above the command bar and the About card 210 — a third of the
window, on two doors out of four. Two causes:

- **The window was 103 px taller than the drawing.** Measured in headless
  Chrome at the mock-up's own 680 px, `.win` is **496.9** — not the 600 its
  hint line claims. Design §2's table says 600 and derives only the width; the
  height came across from the pre-Four-Doors window unexamined.
- **The setting-row pitch was 32 where the drawing is 46.** `tok::ROW_GAP` (20)
  and `tok::DIV_GAP` (10) are that rhythm, for the System and About cards only.
  **Not** a regrid of `CTL` / `ROW_H` / `CARD_PAD` — design §10 rules those out
  because `ROW_H` feeds `ImageList_Create` and so moves the tick's cell.

`MIN_HEIGHT` also changed SUBJECT. Every earlier derivation solved the
Shortcuts page for a row count, which cannot be the binding constraint — card
1's list gives room up before anything else moves, so the door that runs out of
room first is one of the three whose card is FIXED. It is About, the only page
whose height depends on a text measurement, at a three-line disclosure:
`78 + 356 + 44` = 478, rounded to 480. The list's rows are a consequence now
(eight at 500, seven at the floor), not the derivation. `MIN_WIDTH` is
untouched and still waits on gate G1.

**`system_plan` and `about_plan` now live in `beckon_core::page_plan`.** They
were pure integer arithmetic inside a `cfg(windows)` module that had **zero
tests** — so the whole vertical geometry of four doors was untestable on two of
the three CI jobs and unrunnable on the dev machine. The evidence that this was
the cause and not a coincidence: `layout.rs` claimed the System card is "262 px
of interior" when the figure was 232, and no reading of the code produced 262.
`layout.rs` now has its first five tests (Windows job only); the seven in core
run everywhere.

## The filter box is a view, and the mapping is the feature

`IDC_FILTER` (1021, cue banner `Filter`, no label) matches case-insensitively
against the **app column only**. It lives in `Model`, not in `Ui`, because
`Model::remove_pressed`, `marked_count`, `ControlState::selected` and
`remove_enabled` all depend on what is visible — decisions that belong in the
crate all three CI jobs compile. **`ListItem` carries its model row, and
`LVN_ITEMCHANGED` maps `items[i].row` before calling `on_select` / `on_mark`** —
those callbacks take model indices, and a ListView only ever knows view
positions. Without that, one filtered keystroke ticks one binding and deletes
another.

**CORRECTED 2026-08-14 (`8d03d56`): the filter matched BOTH columns until this
branch.** That is where the feature started, and the argument for it was that
both columns is the rule `beckon search` already uses, so the program would
have no third matching dialect. What falsified it: **every beckon chord
contains `alt`**, so a filter of `a` — a plausible first keystroke of "brave" —
matched every row while the box looked as though it had narrowed the list, and
`Remove` takes the ticked rows. Measured with four bindings (`Brave` / `Kitty`
/ `Firefox` / `Discord`, all `ctrl+alt+<key>`) and filter `a`: `visible`
returned all four, so ticking what was on screen and pressing Remove deleted
the whole table. It now matches the app name only, pinned by
`the_filter_does_not_match_the_shortcut_column`. The two dialects differ on
purpose: `search`'s worst outcome is a long list, this window's is a deleted
binding.

**What that gives up is real, and is pinned rather than left to be
rediscovered.** The window can no longer answer "what already owns this chord?"
by filtering — that is what `filtering_by_a_key_name_finds_nothing` asserts. If
it bites, the way back is to match the chord's **key** (`f2`, `b`) — the half a
person searches for, and the half that is not `alt` on every row — and never
the whole chord as a substring again.

Two rules keep it safe, and both are functions rather than discipline.

- **Remove never deletes a row you cannot see:** ticks survive being filtered
  out but are inert while off screen, and `marked_count` / `remove_enabled` are
  scoped to the visible set too — otherwise the window says four are ticked
  while Remove takes one.
- **`visible()` exempts the selected row from the filter:** without it, editing
  a row until it stops matching drops it from the view, and `apply_state`'s
  `None` arm then disables the field that has keyboard focus and blanks it,
  mid-word. That exemption also means the list cannot empty while a row is
  selected, so `Ui::shown_empty` never flips on a filter keystroke and `layout`
  never resizes the App combo there — the §7.15 path, closed rather than argued
  about.

`Add` still clears the filter, which is a different question: a new row is
empty and would match nothing.

## A "does this platform consult that decision?" guard was REJECTED

2026-08-17, on its own numbers. The motivating defect is real: `warn_dot_shown`
had **zero callers** in `crates/beckon-macos/` while the core test
`the_warning_is_on_screen_from_every_door` passed the whole time, because that
test asserts a property of two FUNCTIONS and neither window is reachable from
it. Generalised: *any* `beckon_core::settings` output a platform never consults
is invisible to the entire suite.

The obvious guard is a grep — for each `pub fn` in `settings.rs`, fail when
either window crate has no call site. **Measured before building it: 13 of the
25 public decisions are legitimately one-platform**, and every one has a design
reason already recorded — `opacity_alpha` / `opacity_label` because macOS has
no `Dark mode` row and uses semantic `NSColor`s; `default_button` because it is
a Win32 dialog concept; `image_identity` / `image_age` because macOS reaches
them through `about_state`; `app_cell` / `split_app_cell` / `flag_tone` because
Windows folds the status word INTO the app cell and macOS gives it its own
`COL_STATUS` column (`settings_window/mod.rs:596`), which is presentation
rather than a missing consult. An allowlist of 13 is where the fourteenth
hides, so the guard would cry wolf until someone deleted it.

**A naive count is worse still and is the trap to avoid**: grepping only the
two window crates reports 15 alarms, because `serve.rs` builds `ControlState`
and both windows consume the OUTPUT — `service_line`, `size_label` and
`explain_unreadable` have zero call sites in any window and are all correctly
reached.

So the route is a **smoke test per platform**, not a grep: push a state in and
read the rendered value back. That became possible on macOS only once the warn
mark rode in an `NSSegmentedControl` caption, which AX can read — and note the
two traps that make such a test silently vacuous: **an `AXRadioButton` with
`AXSubrole = AXSegment` answers `nil` for `AXTitle`** and carries its caption
in `AXDescription`; and `settings_saw_external_change` branches on `dirty`, so
a test that does not edit first takes the silent-reload path and asserts
nothing.

## The status vocabulary is four words, and a healthy row says nothing

`paused` > `in use` > `missing` > `other chord`, and that order IS the
precedence — a row can be several at once while the cell holds one word.
`paused` sits above the registration map deliberately: `serve` CLEARS that map
when it pauses, so consulting the map first would render every row "not
registered yet" and never say why.

**One function, `beckon_core::settings::row_condition`, produces the list flag
AND the editor's notes**, and derives `mark` at the end rather than assigning
it along the way — so "the cell and the note cannot disagree" is true by
construction rather than by discipline. It was not: `items` used to read only
the registration map while `detail` read the catalog too, and they contradicted
each other.

**CORRECTED 2026-08-15 (branch `four-doors-phase-0`), twice over.** The four
words were `paused` > `key in use` > `not installed` > `custom`; design §3.1
reworded three of them to the shorter forms above, and the precedence did not
move. All three renames are shorter than what they replace on purpose — the
word rides *inside* the App cell (`app_cell`), so every character it spends is
one the app name does not get.

And *"derives `mark` from the notes at the end"* stopped being the whole rule:
design §3.1 also deleted the note that merely repeated each word, so three of
the four words now say their piece in the cell and nowhere else, and `mark`
folds **the notes and every condition the row earned**. Fold only the word that
WON the cell and a paused row whose app is missing reports `Warn` where it used
to report `Bad` — the precedence is for the cell, not a claim that the
outranked problem stopped existing.
`a_paused_row_whose_app_is_missing_is_still_bad` is the pin.

### `missing` has now been wrong twice, both times about a tier the CLI passes

The catalog arm of `row_condition` asks one question — *does this machine have
this app?* — and `beckon check --resolve` asks the same one. Twice now they have
answered differently, and both times the window was the pessimist:

| measured | the window said | `check --resolve` said |
|---|---|---|
| airm3 2026-08-16 | `Settings` and `DeepSeek` `missing` | exit 0 for both |
| macmini 2026-08-17 | all six `\|\|` rows `missing` | `ok: every app name resolves` |

The first was the **substring tier**: the arm compared for equality while every
beckon resolver ends in a case-insensitive substring match that
`check --resolve` deliberately passes.

The second was **candidate chains**. `beckon_core::candidates::split` is called
in exactly three places, all in `beckon-cli` — the hot path, `check --resolve`
and `resolve` — and `settings.rs` contained no mention of the word *chain*. So
the arm compared the WHOLE value against installed names, and since no app is
called `Gmail || https://mail.google.com/`, every chain row in the file was
`missing`. `5a849c8` had fixed this same class for `resolve` a week earlier and
could not reach the window.

Two things the fix has to get right, both taken from `check_resolution`'s
`winner` rather than re-decided:

- **The row is graded by the candidate that WINS, i.e. the first that is not a
  miss** — not the first (which calls a working binding dead) and not the best
  (which hides a substring hazard a later exact candidate never gets the chance
  to beat).
- **A malformed chain (`"Gmail || "`) is a miss carrying the parser's own
  sentence.** `candidates::split` refuses that identical string before it
  reaches any backend, so the key really is dead — but `missing` alone points
  the reader at the app catalog, which is not what is wrong. Note that
  `Model::problems` does NOT flag it, so this note is the only thing that says
  so in the window.

The control that matters is `examples/catalog_probe.rs` against the author's
real `apps.shared.toml`, run on both sides of the change: six rows
`MISSING → present`, while the two rows that were already right (`Settings`,
`Brave || Brave Browser` — both loose) were byte-identical before and after.
Its own header records that this probe once over-claimed in the same direction
as the bug it exists to catch, which is the one failure a control must not have.

### macOS puts the word in a COLUMN, and that column cannot widen itself

Windows draws the flag as a pill *inside* the App cell (`app_cell`, `FLAG_SEP`),
which is why §3.1 shortened all four words. macOS added a fourth `NSTableColumn`
instead — and that column clipped `other chord` to `other cho`.

**Widening it does nothing, and this took three wrong guesses to establish.**
`NSTableView` tiles to the sum of the column widths plus one
`intercellSpacing.width` each, and when that exceeds the clip view the whole
overflow lands on the LAST column, which no horizontal scroller can reach. What
the status column gets is decided by the three columns before it and by the
gutters; its own declared width is not in the subtraction. Measured with
`examples/geom_probe.rs` on macmini 2026-08-17:

```text
spacing  widths          clip  table   status: x  drawn  VISIBLE
  17     20/170/250/ 80   567    603         499     94       68   <- shipped, clipped
  17     20/170/230/100   567    603         479    114       88   <- but truncates combo
   6     20/170/254/100   567    594         475    109       92   <- ships now
```

`intercellSpacing` defaults to **17 x 0** here, so four columns spent 68 pt on
gutters alone. Dropping it to 6 is what buys the room; the columns then get
their declared widths instead of the last one paying for the other three.

Three guesses this refuted, none of which a compiler or a test would have
caught — the overlay scroller was covering the column (it is not: the scroller
takes its own 17 pt lane, 584 frame vs 567 clip); the column was being shrunk by
`lastColumnOnlyAutoresizing` (it is not: the table stays at its tiled width and
simply overhangs); and `COL_COMBO` had spare room to donate (it does not).

That last one was a **wrong claim already in the file**: the comment named
`Ctrl + Cmd + Option + Shift + PgDn` as the longest string the column can hold.
Measured over all 81 entries of `key_table()` through the window's own
`combo_display_folded_with(.., MAC)`, the longest is
`Ctrl + Cmd + Option + Shift + Backspace` at 246.05 pt against `PgDn`'s 213.04 —
so at the old 250 less ~4 pt of cell inset, the longest binding in the table was
truncating by 0.05 pt. It is 254 now.

**`geom_probe` needs no Aqua session**, which is what makes any of this
measurable from a Claude session: Auto Layout is arithmetic and runs in the
Background namespace, while DRAWING is what does not. It must force the content
size, though — an un-ordered-front window reports every frame as 0, which reads
exactly like a broken window. It cannot say whether text is elided or hard-cut,
and it cannot see the scroller; both need pixels.

### `other chord` is a VIEW fact, and its old comment argued for it wrongly

**A `+shift` row is `other chord` and is reachable through Caps. Both, on
purpose.** Recorded 2026-08-17 because the two nearly got merged, and the merge
would have looked like a bug fix.

The word used to ask a private predicate in `settings.rs`, documented as
*"whether holding Caps Lock can reach this combo … and no `shift` on top (the
hook injects the chord and nothing else)"*. The parenthesis is **false**:
`caps::bound_keys` and `bound_keys_mac` both filter on `ctrl`/`super`/`alt` only,
and their own doc says why — the user's physical Shift is still down when the
hook injects the chord, so `Caps+Shift+T` arrives as `<chord>+shift+t` and lands
on a shift binding by itself. `a_shift_binding_on_the_chord_is_still_reachable`
has pinned that since the tap was written.

So the conclusion was right, the stated reason was wrong, and the obvious next
move was to delete the `shift` term. Two independent design sources say not to:

- `shortcuts::combo_caps_folded_with` carries the identical term, and its reason
  is visual: once the common chord is one `Caps` cap wide, a binding on any other
  chord is the one that still *looks* long, and spotting it costs no reading.
  Folding a superset destroys exactly that.
- The README ships `"ctrl+super+alt+shift+t" = "Telegram Web"` and the spec cites
  it as the reason this status word exists at all. Unflagging it would unflag the
  motivating example.

The fix was therefore to **share the predicate, not to change it**:
`shortcuts::combo_folds_to_caps` is now the one copy, called by the fold and by
`row_condition`. That makes the word and the cell beside it agree by
construction — a row says `other chord` exactly when its chord is not the one
the list would collapse — and it puts the corrected reasoning in one place
instead of two. `a_shift_row_is_other_chord_and_still_reachable_through_caps`
asserts both halves in one test so neither can be dropped as redundant.

Worth naming the shape, because it is this file's recurring one: the hazard was
not an unmeasured claim, it was a **correct line with a false justification**
sitting next to a measured test that contradicted the justification. Reading
either one alone gives you a wrong next action.

## Starting on a config that does not parse

**`beckon-serve.exe` starts anyway** (commit `4f82b94`). It installs the tray,
registers no hotkeys, arms no Caps hook — the parsed `keyboard` block is
discarded along with the shortcuts, because a half-parsed file must not decide
whether to install a `WH_KEYBOARD_LL` hook — and writes nothing. The settings
window then opens read-only with the parse error as ordinary notes. Refusing
was measured on a14 to end in a modal dialog with *no tray icon*, which made
the one window built for exactly this file unreachable from the one starting
condition that most needs it. **`beckon.exe serve` still refuses and exits
non-zero** (`BrokenConfig::Refuse`): it has a console to print to and callers
that check the code. `beckon check` is untouched.

**CORRECTED 2026-08-16: macOS takes BOTH arms, chosen at run time.** This entry
used to read *"macOS `serve` refuses too — no tray, no window, nothing for a
tolerant start to rescue"*. That was true when it was written and false from
`db4aabc`, which gave the platform a tray and four working doors — so the
justification outlived the fact it rested on, in the direction that strands the
user. macOS has **one** binary where Windows has two, so it cannot split the
decision by PE subsystem; it asks whether anyone is watching stderr, which is
the same signal `notify.rs` already uses:

- **stderr is a terminal** — a person ran it by hand. `Refuse`, print the
  parser's message, exit non-zero. Unchanged, and that is the half that keeps
  this from being a regression.
- **stderr is not** — launchd, whose `StandardErrorPath` is a file. Nobody
  reads the message and nobody reads the code, so refusing spends both and buys
  nothing. `ServeAnyway`.

`macos_broken_config` is the whole decision, pure and ungated, so all three CI
jobs test it; only the caller that samples the terminal is platform-bound.
**Windows keeps `Refuse` unconditionally** and must not adopt this: there the
answer is already carried by which binary is running, and sampling the terminal
would let a shell redirect quietly change a documented exit code.

The restart interaction is worse on macOS than on Windows, and is the second
reason: `examples/windows/serve/beckon-serve.xml` caps `<RestartOnFailure>` at
`PT1M` x 3 and then gives up, but the Homebrew formula's launch agent is
`KeepAlive { SuccessfulExit: false }` with `ThrottleInterval 60` — **no cap at
all**. Pairing that with a deterministic exit 1 is an infinite restart loop,
once a minute, on a file only a human can repair, with no tray anywhere to say
so.

## The availability probe asks the OS last

Order, from `beckon_core::settings::probe_plan`: parse, the F12 guard, the
row's own chord, other rows in the file, the row's *saved* chord, and only then
`RegisterHotKey`. Every step before the last is a fact the OS cannot report,
and asking it first lets a reserved or already-duplicated chord come back
green. **`VK_F12` is reserved for debuggers at all times**, so a successful
registration on it proves nothing — and the F12 guard does **not** commute with
the own-row check: below it, a row bound to `ctrl+alt+f12` probing its own
chord answers `Unchanged` with `Mark::Ok`, a green tick on the one key the
guard exists for.

The probe registers on the **settings window's** `HWND` with one fixed id,
never `tray_hwnd`: a hotkey is `(hWnd, id)`, and MSDN keeps a duplicate pair
*alongside* the original, after which `UnregisterHotKey` frees an unspecified
one of the two — a silently dead hotkey. It unregisters on every exit path;
measurements §60 proves it does, with a control that shows the test can see a
held chord. The verdict rides on `RuntimeStatus`, never `Model::problems`,
which is what keeps `apply_enabled` testable on the two CI jobs that are not
Windows — and **`RuntimeStatus.registered` never decides availability**,
because pausing clears it and beckon's own chord would read as free.

macOS has nothing to ask at the last step; see `docs/notes/macos-backend.md`.

## The shortcut editor: four check boxes and a closed key list

Not a text field. Spec §C.4's typed path, which it calls primary: it makes an
invalid combo unrepresentable, it is the only path for someone who cannot
physically produce a chord, and a `CBS_DROPDOWNLIST` has no edit control, so
§7.15's resize defect is structurally impossible there. `IDC_COMBO` **kept its
number (1002) and changed class** — the id `settings_probe` pins still names
the shortcut control.

Two things hold it together and neither is visible to a unit test.

- **`ComboView::key` is an index into `shortcuts::key_table()` and the window
  passes the same integer to `CB_SETCURSEL`** — so the list must be filled from
  `key_table()` in order and **`CBS_SORT` must never be set**; sorted, `f10`
  moves ahead of `f2`, every index shifts, and the window writes a key the user
  did not choose, silently. `examples/settings_probe.rs` reads the style and
  the count on hardware because nothing in `beckon-core` can see either.
- **`commit_fields` compares `ComboView`s, not strings**: `Combo::parse`
  accepts free modifier order while the window rebuilds canonically, so a
  string compare made `"super+ctrl+alt+t"` look like an edit and lit up Save on
  a file nobody had touched.

The four boxes carry **no `&` mnemonic** — `Hold` already claimed `t`, `w` and
`l`, and the table in `mod cap` is the only guard there is.

## The Caps Lock row is one line, and `Hold` has three chips

`[x] Use Caps Lock as a shortcut key   Hold [Ctrl][Win][Alt]   Tap [v]`. It
replaced a check box plus three radios whose first caption embedded the
question governing all three, so the other two did not read as answers to it.

**There is no Shift chip and there must never be one**: `Chord` has exactly
`ctrl`/`super_`/`alt`, because the hook has to release whatever it presses, and
releasing Shift under the user's fingers makes everything they type next arrive
lowercase. Spec §F.8 sketches four chips; the type is right and the sketch is
wrong.

`Tap` is a `CBS_DROPDOWNLIST` read and written **by index**, never by text —
even a `DROPDOWNLIST` has typeahead, which moves the selection. Enablement
follows the check box, and note that a **disabled `CBS_DROPDOWNLIST` still
renders white with dark text**, so it looks live beside greyed labels:
measurements §56, and do not "fix" it.

## Chord capture is in, as `Record` / `Stop`

**REVERSED 2026-08-12.** This entry used to read *"Chord capture stays out.
Combos are typed as text. `msctls_hotkey32` cannot capture the Windows key, and
`Win+T` and its siblings are shell hotkeys Explorer consumes before a normal
window sees them — so a capture field would fail on precisely the chords beckon
recommends."*

**Both facts are true and both are about a window receiving `WM_KEYDOWN`, which
is not the layer capture uses.** A `WH_KEYBOARD_LL` callback runs before the
keystroke reaches any queue and before shell hotkey processing, sees `VK_LWIN`
as an ordinary `vkCode`, and suppresses the key by returning 1 — and beckon
already owns that hook for the Caps feature. Measured on a14 2026-08-12 with a
person at the keyboard: `Win+T`, `Win+X`, `Win+D`, `Win+E`, `Win+R`, `Win+Tab`,
`Alt+Tab` and `Ctrl+Shift+Esc` all came back
`SEEN=True SWALLOWED=True ACTED=False`, with `Win+R` appearing twice in one run
— passed through it opened the Run dialog, swallowed it did not — as the
control that carries the claim. Do not re-add the old entry without re-running
that probe.

**This widens the LLHOOK exception from one feature to two**, because capture
arms the hook on machines where the user deliberately left
`keyboard.caps = false`. Three things keep that narrow and none may be
"simplified" away:

1. there is exactly **one** hook with a two-reason refcount
   (`capture::HookOwners`) — a second `WH_KEYBOARD_LL` chains and would record
   the alias `Caps+T` injects instead of the key pressed;
2. the capture arm of `hook_proc` is consulted **before** `caps::decide` for
   that same reason;
3. the `caps::decide` arm is **skipped entirely** when Caps is not wanted and
   `CapsState::at_rest()` agrees nothing is owed, so a capture on a Caps-off
   machine cannot make a Caps tap toggle the lock through a synthesized stroke.
   The `at_rest` half is not optional: skipping while a swallowed key-down is
   still owed its swallowed key-up leaks an unpaired up into whatever has
   focus.

**What is refused rather than recorded**, in `capture::is_reserved`: `Win+L`
and `Ctrl+Alt+Del`, and the three lock keys as main keys. `Win+L` is a
**block-list, not blindness** — measured, the hook *does* see it, and returning
1 does not stop the lock, so without the list beckon would cheerfully write a
binding that can never fire.

**The hook must never outlive the window**, and it does not: `end_capture` is
idempotent and is called by the `Stop` button, all three of §F.4's focus
layers, a 10 s watchdog, `WM_CLOSE` (before the save prompt — that prompt is a
modal loop on the hook's own thread), `WM_DESTROY`, both `std::process::exit`
arms of `hotkey::run_forever` (Quit from the tray never reaches a
`WM_DESTROY`) — and, since the tab strip landed (2026-08-14, `fa16bf3`), **a
page switch**: `settings_window::show_page` calls it after the unchanged-door
guard and before anything is hidden.

That one is not redundant with the three focus layers, and this is why it had
to be added rather than assumed: **`WM_KILLFOCUS`, `WM_ACTIVATE` and
`WM_ACTIVATEAPP` are all about the WINDOW losing focus, and a pill click is a
child-to-child focus move inside one window** — none of the three fires. `Stop`
is `IDC_RECORD` wearing another caption and `IDC_RECORD` is a Shortcuts-page
control, so the switch takes the only visible way out of a recording off the
screen while the hook is still swallowing every keystroke; the mouse reaches
the pills freely because the hook swallows the keyboard only.

The watchdog is a weak bound on that, not a substitute: `CAPTURE_TIMEOUT_MS`
bounds SILENCE and `on_capture` re-arms the timer for every outcome the hook
posts, so a held modifier keeps the clock running. Worse, a chord completed
behind another door still ran `Outcome::Captured` all the way into
`push_shortcut`. The watchdog itself is not belt-and-braces either:
`is_installed()` can lie, because past `LowLevelHooksTimeout` Windows removes
the hook silently and there is no API to ask.

The typed path stays primary — capture is an accelerator, not a replacement.
Someone who cannot physically produce a chord still has the four check boxes
and the key list, and keys capture can never see (bare `escape`, bare `tab`)
remain selectable there.
