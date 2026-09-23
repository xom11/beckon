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

  **NARROWED 2026-09-23: the control itself moved on.** Phase 2 (spec §5.1)
  replaces the `NSSegmentedControl` with a real `NSToolbar` in `.preference`
  style. The argument above still holds and now applies one level up: AppKit's
  own preference toolbar draws the centred pill group, the selection
  highlight, the SF Symbol + caption layout and a keyboard story in both
  appearances — what the segmented control bought over four hand-drawn pills
  is what the toolbar now brings over the segmented control, for the same
  reason. What the segmented control paid for that — a caption that could
  carry data, the Shortcuts segment's binding count and external-change dot —
  is retired with it; see "A 'does this platform consult that decision?' guard
  was REJECTED" below for where those two facts live now.
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

**NARROWED 2026-09-23: the reasoning above is about the settings WINDOW, and
the macOS menu bar now draws glyphs.** Spec §7 gives macOS a core renderer,
`combo_glyphs`, and `serve::BindingRow::chord` carries its output (`⇪C`,
`⌃⌥⇧⌘M`) into every menu row. The check-box argument does not reach there —
a menu has no check boxes to disagree with, and `NSMenu` draws its own key
equivalents as glyphs, so words would have been the odd spelling. The words
remain for the window, and remain on Windows, and `ModifierLabels::MAC` is
still what the menu's tooltips and VoiceOver strings (`BindingRow::spoken`)
are built from.

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

## The window grew again for the update check, and System pays for it — ACCEPTED, then HALVED

**NARROWED the same day by the About-compaction branch.** Everything below
describes the state Task 9 shipped and the reasoning that produced it; it is
still the derivation to read, because the compaction only changed one term in
it. What moved:

| | before the update check | Task 9 shipped | after compacting About |
|---|---|---|---|
| `about_plan` `content_h` (2-line) | 318 | 410 | **364** |
| About `card_h` | 340 | 432 | **386** |
| `MIN_HEIGHT` / `WINDOW_HEIGHT` | 480 / 500 | 572 / 592 | **526 / 546** |
| ground below System | 52 | 144 | **98** |
| `about - sys` gap | 14 | 106 | **60** |
| floor at 200 % scaling | 960 px | 1144 px | **1052 px** |

The `Licence` row is the whole difference — one row pitch, 46 px — and it went
because it restated `MIT OR Apache-2.0`, a string that ships beside the binary
and is one click away in the repo. **Half of Task 9's 92 px comes back and no
more**, because one row was removed against the two the update check added.
So System's ground is better than the 144 that shipped, still past the 60 px
ceiling this section derives, and still worse than the 52 it had before the
feature existed. The two candidate fixes named at the end of this section are
unchanged and both still open.

The 200 % floor is the item that changed character rather than degree: 1144 px
against a 1080p work area of ~1040 was unusable, 1052 is marginal, and 175 %
(920) is comfortable again. Still **simulated, not seen**.

2026-08-25 (the check-for-updates branch, Task 9). Same shape as the section
above, one level up: About needed two more real rows (the update status line
and `Check now`, then the upgrade-command row) and had nowhere to put them
inside the existing card, so `WINDOW_HEIGHT` moved **500 -> 592** and
`MIN_HEIGHT` **480 -> 572** — both by exactly 92 px, one row pitch (46) times
two. `beckon_core::page_plan::about_plan`'s `content_h` grew by the same 92;
`system_plan`'s did not move at all, because the System card has no new
content to grow into.

**The System door was already the shallower of the two fixed cards** —
`card_h` (`layout.rs`'s padded card height, not the bare `content_h`) is 326
for System against About's 432 at the shipped size — and the 60 px "ground"
ceiling the section above derived — one row pitch plus a card gap, past which
emptiness stops reading as margin — used to bound *both* doors. It now only
binds About, on purpose:

```text
ground(page) = WINDOW_HEIGHT - 122 - card_h(page)   -- 122 = layout.rs's own
                                                        `geometry()`: titlebar
                                                        + tab strip + a gap
                                                        above the card, plus
                                                        pad + a control row +
                                                        a gap below it. Both
                                                        halves are fixed
                                                        tokens, not functions
                                                        of h, so the SUM is
                                                        constant across every
                                                        window height.

System:  card_h = 326 (unchanged)
  before (WINDOW_HEIGHT 500):  ground = 500 - 122 - 326 =  52   (inside the 60 px ceiling)
  after  (WINDOW_HEIGHT 592):  ground = 592 - 122 - 326 = 144   (well past it)

About:   card_h = 432 (was 340 before this branch -- 14 more than System's
                        326, not equal to it; the two doors were already
                        unequal, just both inside the same 60 px ceiling)
  after: ground = 592 - 122 - 432 =  38   (still inside the ceiling; this is
                                            the page WINDOW_HEIGHT is derived
                                            from, so it has to stay inside it)
```

Keyboard has no new content either, so its own ground widens by the same flat
92 px the window grew by — same mechanism, no separate derivation needed.

**This is pinned as arithmetic, not left to be rediscovered**:
`beckon_core::page_plan`'s
`about_now_legitimately_outgrows_system_by_the_update_check` asserts the
`about - sys` gap is 106 (14 px pre-existing, from the two doors never having
been EXACTLY equal, plus the 92 the update check added) and says in its own
failure message to re-derive `MIN_HEIGHT` / `WINDOW_HEIGHT` and this margin
together if it ever moves; `layout.rs`'s
`the_fixed_doors_leave_no_room_for_a_second_card` pins System's ground at
`144` by name (not by the old 60 px bound, which it explicitly narrowed to
About alone) and About's at the ceiling as before.

**Accepted for this branch, on two grounds.** First, the update check itself
is not what bloated this: Windows' own contribution to About is already the
LEANER of the two platforms' — `about_plan`'s two rows (`update`, `command`)
against macOS's three logical rows in `about.rs`'s `build()`: the update
line (`update_status` + `check_now`, assembled into a local `update_row`
hstack that is never stored on `AboutControls` — the struct holds the two
fields separately), `command_row`, and `open_releases_row` (these last two
ARE stored `Retained<NSStackView>` fields on the struct). Windows still
needed 92 px for the leaner version, which is evidence the growth is real
content, not Windows-side excess — there was no cheaper way to build this on
this platform that was left on the table. Second, the change is a symptom
of a **content** decision (About needed two more rows) forcing a **window**
decision (grow to fit them), and no purely geometric fix on the System side
removes that dependency — it only moves the cost somewhere else, and nothing
clips or overlaps at the size this branch ships.

**Two ways it could be reduced, both deferred rather than rejected:**

1. **Give System a fourth content group**, so its card grows to use the room
   the window now has instead of leaving it empty. Requires a real fourth
   setting to put there; inventing one to fill space would be worse than the
   empty ground it replaces.
2. **Build real collapsing-row infrastructure for Windows**, so a page's card
   can shrink to what it actually contains instead of the window being sized
   for the tallest door. **`ShowWindow(SW_HIDE)` is not that infrastructure**
   and must not be reached for here: hiding a control raises no focus
   notification at all, which is the exact defect this codebase already paid
   to fix once, on the banner's `Reload` button (see the `repair_hidden_button`
   history in `mod.rs`, and the a14 measurement there — `DM_GETDEFID` kept
   answering `IDC_RELOAD` after the banner was dismissed, because hiding a
   window is invisible to `BN_SETFOCUS` / `BN_KILLFOCUS`). Collapsing rows
   would multiply that hazard by every row that can collapse, not just one
   banner; it needs its own focus-repair design before it is worth building,
   not a `SW_HIDE` call at each call site.

Neither is scheduled. If a future door adds System content, re-derive the
table above rather than assume it still holds — the 122 px of fixed chrome is
what stays constant, not the ground numbers themselves.

**The accepted cost above is only the interior half. `MIN_HEIGHT` is also
the window's OS-enforced floor, and that half was not checked before the 92
px was accepted.** `WM_GETMINMAXINFO`'s handler in `mod.rs` sets
`mm.ptMinTrackSize.y = scale(MIN_HEIGHT, dpi)`, and `scale(v, dpi) = v *
dpi / 96` — so the floor the user cannot resize below scales WITH the
monitor's DPI, not just with the window's initial paint at 96 dpi.

```text
scale(572, 96)  =  572   -- the 1:1 case, same number as the interior math above
scale(572, 168) = 1001   -- 175% scaling
scale(572, 192) = 1144   -- 200% scaling
scale(480, 192) =  960   -- pre-branch MIN_HEIGHT, for comparison, at 200%
```

A 1080p display's work area (the screen height minus the taskbar) is roughly
1040 px. At 200% scaling the new floor, 1144, is past that entirely — the
window could not be resized down to fit the screen at all. Before this
branch the floor was 960, which did fit. At 175% it is 1001 against the same
~1040 px work area — close enough that a taller taskbar, or a second
monitor's different scale, could tip it over.

**This is simulated arithmetic, not a measurement — nobody has run this
branch on a real high-DPI Windows display.** Everything this session could
run stayed at 96 dpi. Check it on a14 at its real scaling before treating the
+92 px growth as fully accepted: if `ptMinTrackSize.y` genuinely exceeds the
work area there, the window cannot be shrunk to fit the screen at all, which
is a worse failure than the interior ground going past its 60 px ceiling.
This was made an explicit condition of accepting the growth above, not a
separate, lesser concern.

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

**NARROWED 2026-09-23: the surface this smoke test read is gone.** Phase 2
retires the `NSSegmentedControl` tab strip for an `NSToolbar` (spec §5.1), and
a toolbar item cannot carry a caption that changes with the data the way an
`AXSegment` could — so there is nothing left on this platform for an AX read
to catch either fact on. The Shortcuts segment used to read
`shortcuts_tab_label(binding_count, warn)`, e.g. `Shortcuts  19 •` for
nineteen bindings with the bullet appended whenever `warn_dot_shown` was true;
both the function and the segment it drew on are deleted with the strip.
**Both facts moved to the service line instead**, drawn on all four doors:
the count is what `Serving · N of N` already said (`base_service_line`), and
the external-change warning is now an appended clause — `service_line`,
`" -- the file changed on disk"` — rather than a caption glyph, raising the
line's mark to `Warn` unless it is already `Bad`. Neither reaches the tray's
own `Needs attention` section: that lists rows whose BINDING is broken, is
unrelated to a config file moving on disk, and predates this branch. The
smoke test this guard motivated is retired with its surface.

**CORRECTED 2026-09-23: this paragraph used to end by saying the macOS test
that replaced it "now pins only core's `banner_shown` / `warn_dot_shown`
partition". It pinned nothing at all.** That test asserted `banner ^ dot`,
and core DEFINES `warn_dot_shown` as `external_change && !banner_shown(..)`
-- so with `external_change` true the assertion is `B ^ !B`, true for every
input, a `banner_shown` returning `false` on every door included. Measured by
mutation, with both tests in ONE test binary so the build cannot be what
differs: replace `banner_shown`'s body with `false` and the old macOS test
PASSES while core's `the_warning_is_on_screen_from_every_door` FAILS. It is
the same shape core carries a dated `REWRITTEN 2026-08-14: it could not fail`
marker about, reintroduced one crate over -- and two documents, this one and
the test's own doc, stated the broken property as established fact.

The macOS test is now `the_moved_file_reaches_every_door_of_this_shell`, and
it asserts the two surfaces THIS shell draws rather than a relation between
two core functions: the Shortcuts door's banner row, and the service line on
all four doors. Every oracle is a constant written in the test. Under the
same mutation it goes red, in the same run in which the retired one stayed
green.

**The correction turned up a second fact worth recording: the macOS shell
never calls `banner_shown` at all.** `apply_state` hides `banner_row` on
`!external_change` alone (`mod.rs:2794`), and the row is a child of the
Shortcuts door's own view, so the per-door restriction is structural -- the
other three doors are hidden views. Core's projection and this window's
construction therefore agree by construction rather than by a call, which is
free to drift and is exactly what the new test's first half checks.

### `Bar::Readout` has zero production callers, and it KEEPS the arm

2026-09-23, decided rather than discovered — the whole-branch review found it
and asked for a ruling either way.

`command_bar_shown(page, Bar)` gained its second parameter in the auto-save
phase's Task 5 so macOS could ask whether its button row was on screen. Task 11
then **deleted the row** instead of gating it, and with it `beckonSave:`, the
`Save` / `Close` / `Open config file` views and every door that could have
asked. What is left naming `Bar::Readout` is core's own match arm, one core
test, and two comments in `beckon-macos/src/settings_window/mod.rs`.

That is the `warn_dot_shown` shape `CLAUDE.md` records, one crate over — and
the ruling is still to keep it, for two reasons that do not apply there:

- **There is no honest caller to add.** `warn_dot_shown` had three doors that
  each had something to draw and did not ask. Here macOS builds **no button
  row on any page**, so a call from the bar builder would have its answer
  discarded — a fake caller, which is the papering-over shape this repository
  already refuses elsewhere.
- **Retiring the variant would mean editing `beckon-windows` again.** Its four
  call sites pass `Bar::Buttons`; removing the parameter touches all four, and
  "Windows is untouched except the one call-site change Task 5 names" is a
  constraint of this branch.

What the arm buys, concretely: a macOS reader **cannot** call
`command_bar_shown(page)` and be told `true` on Shortcuts, because there is no
such call to make. They must name a `Bar`, and the one macOS names answers
`false` on every door. It is the two prose comments' fact in a form the
compiler carries. `the_button_row_is_windows_only_now` asserts it on all four
pages and is the arm's only reader.

**What would change the ruling**: a macOS command bar that is again built or
hidden per page. Then the builder asks, and the arm stops being
documentation.

### `pub` inside a `pub mod` is outside the dead-code lint, in both window crates

2026-09-23. `crates/beckon-macos/src/lib.rs:31` and
`crates/beckon-windows/src/lib.rs:76` both spell `pub mod settings_window;`, so
every `pub fn` inside is reachable from outside the crate and `dead_code` can
never fire for one — however many doors have stopped calling it. **This is how
an orphaned `ask_save` / `SaveChoice` pair survived two tasks on the macOS side
of the auto-save branch**, with the gate green the whole time.

Re-verified at the end of that branch and the hole is currently **empty**: all
12 top-level `pub` items in `beckon-macos/src/settings_window/mod.rs` have live
`swin::` call sites (`app_field_quiet_for` 2, `apply_about_state` 1,
`apply_state` 1, `apply_system_state` 1, `error` 8, `flush_paint` 2, `is_open`
1, `open` 4, `open_existing` 1, `open_path` 1, `post_catalog` 2,
`set_update_state` 1). The four submodules are private `mod`, so they do not
share the hole.

**The structural close was attempted and left undone, deliberately.** Make the
module private and re-export a curated facade —
`mod settings_window; pub mod swin { pub use super::settings_window::{..}; }` —
after which anything added and not added to the list is flagged by the existing
gate, by construction. Measured cost before writing it: it is **not** the
one-line change it looks like.

- It renames a crate's public path (`beckon_macos::settings_window` →
  `beckon_macos::swin`), so every importer moves: `beckon-cli/src/serve.rs` and
  **five `beckon-macos/examples/`** (`about_update_probe`, `geom_probe`,
  `settings_drive`, `settings_probe`, `settings_shots`), which `--all-targets`
  compiles. Seven files, eight sites, plus the prose references.
- The same move on `beckon-windows` is what makes it a rule rather than a
  local habit, and that crate was off limits for the branch that found this.
  A "by construction" guarantee that holds for one of two twins, while the
  other needs the note anyway, is worse than one rule both are held to.

So the rule is recorded here instead: **a new `pub fn` in either
`settings_window` is invisible to the dead-code gate, and nothing but a reader
will notice when its last caller goes.** When the facade is done, do both
crates in one change and delete this paragraph.

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

### The vertical twin: 48 pt of dead band, and About was never the tallest door

**RETIRED 2026-09-23 (macOS): one shared height for all four doors is gone.**
Phase 2's toolbar (spec §5.1) lets three doors be smaller than Shortcuts, so
the window now takes each page's own height (`size_to_page`), floored at
`MIN_CONTENT_HEIGHT` rather than derived from the tallest door's fitting
size. The four measured page heights below are why one shared height was
wrong in the first place, and are kept as that record — they are no longer a
live derivation, `MIN_HEIGHT` no longer exists as a name, and neither does the
480/500 pair this section produces. **Windows' own geometry
(`page_plan`, `layout.rs`) is untouched** — this entry was always
macOS-specific, unlike the pixel-based `MIN_HEIGHT` / `WINDOW_HEIGHT`
derivation in the Windows section above. See `WINDOW_HEIGHT`'s own doc
comment in `crates/beckon-macos/src/settings_window/mod.rs` for the current
mechanism. The per-page heights this branch actually measured with the
toolbar installed moved four times during five fix rounds, and are recorded
in `docs/notes/macos-backend.md` as a shape rather than a table, for that
reason.

Same probe, the other axis. Measured on macmini 2026-08-17 with
`BECKON_PROBE_H` unset, i.e. at the shipped 500 pt content height:

```text
contentView 500 vs fittingSize 452  ->  48 pt unclaimed
  tabs   h  24
  page   h 360      (its own fittingSize is 360 -- NOT stretched)
  bar    h  24
  gap    60         = 12 pt bottom inset + the 48
```

`NSStackView` puts arranged subviews in the TOP gravity, so every point the
window has and the content does not collects underneath the last band. That is
the empty strip below `Serving · N of N` that a user photographed.

**Ask every door, including the hidden three** — `fittingSize` answers for a
hidden view, and that is the only way to see which one sets the floor:

| door | fitting height |
|---|---|
| **Shortcuts** | **360** |
| About | 306 |
| Keyboard | 190 |
| System | 144 |

So the comment on `MIN_HEIGHT` was wrong about its own subject. It was the Win32
twin's derivation carried across unchanged and it named **About**, reasoning that
card 1's list gives room up before anything else moves. Shortcuts is taller by
54 pt. The inherited 480 was safe only by accident — it sat 28 pt above what any
door actually needed, so nothing ever tested the claim.

The 48 pt went to the list (`ROWS` 8 → 10) rather than to a shorter window: the
list is the only band here that can use height, and a config of twenty bindings
showing eight of them is the case that wants it. After: `fittingSize` 492, 8 pt
unclaimed, the strip under the bar down from 60 to 20.

**Growing the list forces `MIN_HEIGHT` to be re-derived, and this is the trap.**
It is `setContentMinSize` — the height the user can drag DOWN to — so content has
to fit at *that* number, not at `WINDOW_HEIGHT`. Measured at the new floor:

```text
BECKON_PROBE_H=492  ->  fittingSize 492, 0 pt unclaimed        (exact)
BECKON_PROBE_H=480  ->  tabs sit at y=480, i.e. flush with the top edge
```

The 480 case does **not** clip: AppKit pays the 12 pt shortfall out of the
stack's top inset, so the failure mode is padding silently vanishing at one
window size and nowhere else. That is why the floor is now derived from the
tallest door instead of inherited.

The editor card's own 14 pt of blank — an empty `notes` label — was left alone
on purpose. The stack detaches hidden views, so hiding it when empty would move
the card's bottom border while the user typed in the card, and it buys nothing at
the floor because `MIN_HEIGHT` must still fit the note-shown case.

### A Save renamed onto the config path, which severs a symlinked config

`apply_settings` writes temp-then-rename, deliberately: a crash or a full disk
must not destroy a working config, and a rename is the shape `watch_config` was
built for. What it renamed ONTO was the path as given.

**`fs::rename` replaces its destination, and a destination that is a symlink is
replaced by a regular file.** Measured 2026-08-17 with the same call shape:

```text
before  lrwxr-xr-x  link.toml -> real.toml
after   -rw-r--r--  link.toml            (readlink empty)
        real.toml still holds the OLD text; the new text is in link.toml
```

This stopped being hypothetical the same day. Both of the author's Macs now reach
`~/.config/beckon/apps.toml` through a `mkOutOfStoreSymlink` into their dotfiles
repo — and that indirection is forced by beckon itself, because a plain
`home.file.".config/beckon/apps.toml".source = <path>` copies the file INTO the
nix store and links to a **read-only** path the settings window cannot write back
to. So the shared-config setup requires a symlink, and the naive rename would have
severed it: repo file left on the old text, and home-manager moving the orphan
aside under `backupFileExtension` on the next switch. A Save that disappears one
rebuild later, on the file every other host copies from.

### The same file, one defect further in: a Save with nothing to save still wrote

**Measured on macmini 2026-09-18**, on the same nix-managed
`launch-app.toml` — which is to say a file tracked in git, on the host every
other machine copies from.

The gesture was: turn the Caps switch on, turn it straight back off, Save.
No net change. The file came back with two lines it had never carried:

```diff
+keyboard.caps = false
+keyboard.caps_tap = "capslock"
```

`config_write::render` wrote `caps` and `caps_tap` unconditionally — not
because a value had changed, but because the fields exist. `caps_hold` one
line below had the correct shape the whole time (written only when it carries
information, removed when it does not), and **the asymmetry was not
deliberate**; it came of `caps_hold` having a backward-compatibility reason
written down and the other two having none.

The rule now: **a key is written when the FILE already spells it, or when the
VALUE carries information — never merely because the field exists.** Both
halves are load-bearing and both were already pinned by tests written before
this change:

- *already spells it* keeps unticking persistent
  (`caps_off_is_still_written_so_unticking_persists`: the file says
  `caps = true`, the model says false, the key exists, so `false` is written
  over it rather than the line being dropped back to an implied default), and
  it leaves a hand-written `caps = false` alone;
- *carries information* is what stops the first Save from being a diff.

Two things this is NOT. It is not the dirty flag: `Model::dirty` is sticky, so
a value edited and put back still arms `Save`, and that is unchanged —
`set_filter`'s own comment already states the principle this fix restores,
that an armed Save must not rewrite the file byte-identical. And it is not
specific to Caps: any edit at all — renaming one app, adding and removing a
row — carried the same two lines along with it.

The fix resolves the path first (`fs::canonicalize`) and renames onto the target,
with the temp file beside the TARGET rather than beside the link — `rename` cannot
cross filesystems and a link need not share one with its target. The function
returns the path it wrote, because "cannot write ~/.config/beckon/apps.toml" for a
permission problem three directories away in a dotfiles repo sends the reader to
the wrong place.

`saving_through_a_symlink_writes_the_target_and_keeps_the_link` is the guard, and
it was checked the only way that means anything: reverted to renaming onto the
path it FAILS, with the fix it passes. Its neighbour
`saving_a_plain_file_still_replaces_it_in_place` is the control, so the pair says
the change is about symlinks and not about the write in general.

Two things deliberately unchanged, both checked: the 1 Hz reload tick still brings
the new text back whichever directory the watcher sits on, and `event_touches`
still cannot fire on the temp file, whose file name differs from the config's.

### The key list was showing config tokens, on BOTH platforms

`key_label` exists to turn a config token into what the keyboard calls the key —
`j` → `J`, `pagedown` → `PgDn`, `bracketleft` → `[`, `return` → `Enter`. The list
column has always gone through it, via `combo_display_folded_with`. **The key
list — the control where you PICK a key — never did.** It was filled from
`k.name`, so the same window named the same key two ways, and the internal
spelling was the one shown at the moment of choosing.

Both platforms, and neither was a port of the other's mistake: macOS
`addItemWithTitle(&k.name)`, Windows `CB_ADDSTRING` of `wide(&k.name)` **and**
`paint::combo_item` pulling `k.name` for the owner-drawn row. Three sites, one
missing call.

Only the TITLE changes. `ComboView::key` is an index into `key_table()` and the
control's selection index is handed straight back, so the order stays
`key_table()`'s — that rule is untouched and is why this was safe to fix in three
places at once.

It also paid for itself in width: the popup's widest item stopped being
`bracketright` and became `Backspace`, so the control went 121.5 → 113.5 pt on
macOS and the 8 pt went to the app field beside it (239 → 247).

**Windows' half is compiled and lint-clean here but was not run** — it is a
display string from a function the same window already calls, with no index
touched, but it has not been seen on screen.

### What would be better, and what it costs

Two things about that control are still weak, and they want the **same** enabling
change, so they are recorded together rather than fixed piecemeal:

- **81 keys in one flat menu.** Letters, digits, F1–F20, navigation and
  punctuation with nothing between them. Separators would help a lot.
- **Nothing chosen shows an empty control.** `selectItemAtIndex(-1)` is honest —
  `ComboView::key = None` means the row has no key — but a blank popup beside the
  modifier chips reads as broken rather than as empty, and `NSPopUpButton` has no
  placeholder.

Both are blocked by the same thing: **the key's identity is carried by its
POSITION in the menu.** A separator or a disabled "pick a key" item occupies a
position, so either one silently shifts every index.

The fix is not an index-translation layer — that is the "second place the index
can be got wrong" the code already warns about. It is to make the index
**explicit**: put the `key_table()` index in each menu item's `tag`, and read the
selection's tag instead of `indexOfSelectedItem`. That is one changed read per
platform, it removes the positional coupling rather than adding a second mapping,
and only then do separators and a placeholder cost nothing.

It needs one new thing in `beckon-core`: a group per key, i.e. 81 entries
classified. Not built — recorded so that whoever wants grouping does not start by
translating indices.

### The editor is one row, and that took removing three things rather than one

The two-row split was measured, not taste, and the measurement stood: with a
`Shortcut` label, four WORDED check boxes, the key list and `Record`, one row
asks 540 pt of a 584 pt card and leaves the App combo **44 pt**. The combo is the
one control whose content has no length limit, so it is the view `NSStackView`
compresses first — an earlier attempt got it to 2.0 pt with the `App` label
clipped to `Ap`.

So "make it one row" is not a layout change, it is a **budget** question. Every
candidate measured offscreen on macmini 2026-08-17 against the same 584 pt card
and 8 pt gaps:

| modifiers | fixed | combo gets |
|---|---|---|
| worded check boxes + both labels (was) | 540.0 | 44.0 |
| worded check boxes, no labels | 438.0 | 146.0 |
| glyph check boxes, no labels | 367.0 | 217.0 |
| one segmented control, worded | 409.0 | 175.0 |
| **one segmented control, glyphs** | **338.0** | **246.0** |

Three changes, and the row needs all three:

1. **Four check boxes became one `NSSegmentedControl`** with
   `NSSegmentSwitchTracking::SelectAny` — AppKit's control for a set of
   independent toggles, and the same class the tab strip above already uses with
   `SelectOne`. `SelectAny` is load-bearing: `SelectOne` would make picking Cmd
   silently drop Ctrl, and a chord is a set.
2. **The chips read `⌃⌘⌥⇧`, not the words.** This is macOS-native rather than a
   width trick — the four are printed on the keys and appear in every menu in the
   OS. **The Windows font hazard does not carry over**, and it was checked rather
   than assumed: `CTFontGetGlyphsForCharacters` finds all four in the system font
   (U+2303 U+2318 U+2325 U+21E7). The rule that keeps `key_label` ASCII is about
   Segoe UI Variable, on the other platform.

   The words are also not lost from the window: the list's Shortcut column spells
   the same chord as `Ctrl + Cmd + Option + Y` two bands above, so the chips were
   repeating it. Each segment still carries its word as a tooltip, and the control
   carries `Modifiers` as its accessibility label because a glyph is what each
   segment is now called.
3. **The `Shortcut` and `App` labels are gone** — 86 pt of the saving. What
   replaced `App` costs no width at all: the combo's own placeholder, inside its
   slot. The chord side needs no label; `Record` names it, and the card only ever
   describes the row selected in the list above.

The combo is LAST with no spring beside it, which is what makes it the view that
takes the slack. Measured on the real window afterwards:
`seg 132.5 | key 121.5 | Record 67 | combo 239`, summing with three 8 pt gaps to
exactly the 584 pt card.

**The Keyboard door's `Hold` chips were deliberately left as worded check
boxes.** They sit on a row with room to spare, and they name a chord the user is
CONFIGURING rather than one they are reading back, so the words earn their width
there. `set_check` still exists for them.

**A row removed is 32 pt handed back, and it has to be spent or it becomes the
complaint it started as.** The editor card lost a 24 pt row and an 8 pt gap, which
took the tallest door's fitting height from 492 to 460 and put 40 pt back under
the command bar. `ROWS` went 10 → 12, which spends it exactly: fitting 500
against a 500 pt content view, 0 pt unclaimed, and the only gap below the bar is
the stack's own 12 pt inset.

`MIN_HEIGHT` is now spelled `WINDOW_HEIGHT` rather than repeated as a number.
Once `ROWS` spends the last of the slack the two ARE one quantity — the window is
exactly as tall as its tallest door — and writing `480` beside a 500 that had
grown to meet it is how the pair drifts apart again. The cost is that there is no
headroom left: a door that later grows by a point pushes fitting above content,
and the measured way that fails is AppKit paying the shortfall out of the stack's
TOP inset, so the tab strip goes flush with the window edge and nothing else
moves. Adding a band to any door means re-running `geom_probe`.

### The About mark, and the discovery that a Background session CAN read pixels

The letter in the About door's accent tile sat off centre. Four measured causes,
and the last one only appeared because the first three were fixed first.

**`cacheDisplay(in:to:)` renders a view through CoreGraphics into an
`NSBitmapImageRep` and needs no window server.** That is the important finding
here and it narrows this file's own "layout yes, pixels no" rule: an agent shell
in the `Background` namespace can measure *drawn* output for any view it can
construct. What it cannot do is photograph a window that is already on screen.

The four causes, measured on macmini 2026-08-17 (`dy` negative means the ink
sits high):

```text
                                              dx      dy
shipped   margins 5, natural, 13pt         -5.75   -3.75
margins 0, center, 17pt, label is content  +0.25   -7.25   <- nearly shipped
margins 0, center, 17pt, centred host      +0.75   -0.25   <- ships
```

1. **`contentViewMargins` was the `NSBox` default 5x5, never zeroed.**
   `widgets::card` zeroes it by hand; this box did not, so the letter could only
   occupy the middle 24 pt of a 34 pt tile.
2. **`alignment` was `.natural`.** The box stretches its content view to fill, so
   a single glyph in a field wider than itself sits at the leading edge — the
   `-5.75` above, and the misalignment that was visible. The `beckon` label ten
   lines below already set `Center` explicitly.
3. **The letter was 13 pt in a 34 pt tile, a ratio of 0.382.** The design's ratio
   is 0.5 and the Win32 twin carries it as `18/36`, with the reason written at
   `paint.rs`: below it "the tile reads as a letter adrift in a box".
4. **`NSTextField` does not centre text vertically.** A single line in a taller
   frame draws at the TOP, and there is no alignment enum for that axis. The 5 pt
   margin had been accidentally paying for part of it, so **fixing 1 and 3 made
   the vertical worse — from -3.75 to -7.25.** The fix is `widgets::centred_both`:
   the glyph goes at its intrinsic size into a host view, pinned centreX/centreY,
   and the host is the box's content view.

**The control on every bitmap run: 4412 opaque pixels of 4624.** The tile drew,
and the 212 missing are its four 8 pt corners. A bitmap nothing drew into yields
"no ink", which reads exactly like a perfectly centred glyph — so the count is
printed beside the result, not assumed.

Two false starts worth keeping, because both looked like answers:

- `NSTextFieldCell::titleRect(forBounds:)` returns the whole bounds for a label
  (no bezel). It reports the rect the string is laid into, not the ink inside it,
  so it says `dx 0.00 dy 0.00` for a glyph that is visibly off centre.
- The first ink detector compared HSB `brightnessComponent`, and white against a
  saturated accent blue differs by less than the threshold — it reported NO INK
  for a tile that had a letter on it. RGB distance is what works.

The letter's **colour** is deliberately still unset, i.e. `labelColor`. Measured
contrast against `controlAccentColor`: 5.23:1 in light and 4.02:1 in dark, against
a fixed white's 4.02:1 in both. So the semantic colour is no worse anywhere and
better in light mode; the mark simply flips ink with the appearance.

`geom_probe` grew `BECKON_PROBE_PAGE` for this — the three hidden doors are
DETACHED from the root stack, so their subviews never get frames. `fittingSize`
answers for a hidden view and `frame` does not, and reading a zero frame as "the
view is broken" is the trap.

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

### The row asked macOS nothing, and its note answered a question it had not asked

**Measured on macmini 2026-09-18**, beckon 0.15.1 from Homebrew, uninstalled
and reinstalled from scratch to walk a new user's path. Two defects, opposite
in direction, sitting one row apart.

**Ticking the box asked for no permission.** `on_caps` read the switch and
wrote the model, and that was all of it. Driven through `hs.axuielement` on a
fresh install:

| | before the tick | after |
|---|---|---|
| switch | `value=0 enabled=true` | `value=1` |
| Ctrl / Cmd / Option | `[DISABLED]` | enabled |
| `Tap` | `[DISABLED]` | enabled |
| `Save` | disabled | enabled |
| a permission dialog | — | **none** |
| `kTCCServiceListenEvent` row | absent | **still absent** |

Every control said the feature was armed while beckon had not put a single
question to the OS. The grant reaches a tap only at the next launch, so the
remedy is to ask on the way ON and then say what is left to do — **not** to
block the switch: the config travels between machines, and authoring
`caps = true` for a Mac that will be granted later is legitimate.

**The note beside it said the opposite of the truth, permanently.**
`caps_note` was a fixed string ending *"beckon cannot read the Caps key
without it"*, drawn on every visit in both states. On this machine, with
Accessibility granted and **no `ListenEvent` row in TCC at all**, chord
capture recorded `ctrl+shift+F9` on the first attempt — which means
`install_for` had already passed the `input_monitoring_granted()` gate.

The control matters here, because "the warning vanished" and "the warning
broke" look identical. `examples/caps_probe` with a driver injecting F19:

```
Input Monitoring     : granted
events seen, ANY type : 2   last type=11 code=80
-> the tap IS live; the MATCHING is what is wrong.
```

`code=80` is F19, so the tap received exactly what was posted. **Running that
probe with no driver prints `CONTROL FAILED … receiving nothing at all`, which
reads as a damning result and is only "nobody typed"** — the same shape as
every other blind-detector trap in these notes. Its comment says *"The driver
injects F19 here"*; run it that way or do not cite it.

The sentence now follows the rule About's Accessibility line already followed
— a healthy grant says nothing — via
`beckon_core::settings::input_monitoring_warning`, and is re-asked on every
`apply` because the grant can arrive while the window is open.

**`Open Input Monitoring` deliberately does NOT follow the sentence**, and the
asymmetry with About is asserted by a test rather than left to be tidied away:
revoking the grant does not notify a running process, so a button conditioned
on that state would be absent exactly when it is needed, and a reader who
wants to *check* the switch has the same errand as one who needs to grant it.
The sentence can go quiet because it makes a claim that would be false; the
button only offers an errand, which stays true.

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

## Auto-save replaces Save, Close, and Open config file

`Save`, `Close` and `Open config file` are gone from the macOS window as of
Task 11, and **deleted, not hidden** — a hidden button with a live selector
is still reachable through the responder chain. `command_bar_shown` answers
per platform now, rather than one shared literal; Windows still draws all
three and still gates every write behind Save. In their place, every edit
in the model runs `beckon_core::settings::autosave_plan` behind eleven
guards from four-doors design §6 (G-a through G-k, disposed below), and a
bounded Undo (`Model::UNDO_DEPTH = 20`) replaces the discard half of the old
three-way close prompt.

### `controlTextDidChange:` replaced `commit_fields`, and that is why the button came down last

`commit_fields()` was the only thing that rescued text typed into the App
field but never committed to the model — the combo box's own action fires
on Enter or on a list pick, not per keystroke — and it was called from
`beckonSave:` alone. Removing Save before that rescue had a replacement
would have silently lost every partially-typed app name, which is the
reason the button came down last: only after
`NSControlTextEditingDelegate::controlTextDidChange:` was wired to the same
guard, the same read and the same `on_edit_app` callback `beckonApp:`
already used. The only thing it adds is recording `app_last_typed`, which
`autosave_is_deferred` reads for the 600 ms (`AUTOSAVE_QUIET_MS`) debounce
that keeps the write off the hot path of a keystroke — without that record,
someone who picks a name from the dropdown and never types a character
would leave the model dirty with nothing to wait for, a gap no test that
types could ever see.

Wiring it needed two marker impls that look deletable and are not.
`NSComboBox` is an `NSTextField` subclass and the delegate protocol chain is
`NSComboBoxDelegate: NSTextFieldDelegate: NSControlTextEditingDelegate`, but
objc2's protocol traits do not coerce along their supertrait chain the way
Objective-C's own `id<Protocol>` does — `setDelegate` wants
`Option<&ProtocolObject<dyn NSComboBoxDelegate>>`, and `Target` has to name
every protocol in that chain as its own Rust trait. `unsafe impl
NSTextFieldDelegate for Target {}` and `unsafe impl NSComboBoxDelegate for
Target {}` are both empty — every method either protocol adds is
`#[optional]`, and the one method that matters lives on
`NSControlTextEditingDelegate` above — but deleting either one fails to
compile at the `setDelegate` call site. The App combo box had never had a
delegate at all before this phase.

### Escape has never closed this window, so removing Close disarmed nothing

Verified with a control: at the commit before the command bar's buttons came
down, with `Close` still present, Escape did not close the window either —
no `keyEquivalent`, no `cancelOperation:` override, the window is not
modal, and there is no menu bar to route an Escape through. The title bar's
red `X` (`windowShouldClose:` → `may_close()`) was already the only way
out, so retiring `Close` removed a redundant route, not the only one.

### Two AX traps a live-driving session found, both worth knowing before scripting this window again

Setting a text field's value through the Accessibility API does not fire
its action, so the model never sees it: a filter set that way stays empty
in `Model` and the window behaves exactly as if unfiltered, which reads
like a guard failing to fire rather than like the wrong tool having been
used to type.

And `button 1 of window 1` is not a stable address. Deleting the three
command-bar buttons shifted every AX index after them — `Undo` is now
index 1, `AXCloseButton` is index 2 — so a script written against the old
layout that presses index 1 expecting the close button presses Undo
instead, and reports that nothing closed.

## The eleven guards, and three that are not what four-doors design says

Design §6 lists G-a through G-k as preconditions, not polish. Their
disposition after this phase:

| guard | what it does | status |
|---|---|---|
| G-a | compare-and-swap: refuse a write whose base moved | built |
| G-b | reseed the view by identity, not index | built — first attempt was wrong in the case it exists for |
| G-c | in-place key rename, not delete-and-append | built |
| G-d | never inject a bare `keyboard.caps` / `keyboard.caps_tap` line | **already built before this phase** |
| G-e | hold a write when the selected app just went missing | built — **design's own worked example does not fire** |
| G-f | confirm a risky Remove | built |
| G-g | bounded Undo, depth 20 | built |
| G-h | `<config>.bak` written once at window open | built |
| G-i | flush a pending write on session end | **built on macOS after all** — both the close and Quit are session ends (CORRECTED, below); still nothing to do for `WM_QUERYENDSESSION` |
| G-j | one close prompt | built — **fires only on `NotSaved::CannotWrite`**, and the close flushes before it asks |
| G-k | carry the filter/selection/marks across a reload | built, same identity rule as G-b |

Three of these read as omissions if the design document is taken as the
whole story, and are not — each is recorded below rather than left to be
"fixed" back toward the design's own wording.

### G-d was already built before this phase started

`write_caps` / `write_tap` in `crates/beckon-core/src/config_write.rs`
already computed whether to emit a `keyboard.caps` / `keyboard.caps_tap`
line from `spelled(..) || value != default`, and the doc comment already
named the nix-config-in-git case that rule exists for. Nothing in this
phase changed that logic. The one addition is a regression test,
`a_file_that_never_spelled_caps_does_not_gain_it`, pinning it against a
later change that reaches for the field-exists shortcut this guard was
already refusing. **Do not rebuild it.**

### G-e: four-doors design's own worked example does not fire on this machine

The design illustrates G-e with `"ctrl+super+alt+c" = "B"` reaching disk
while the user is mid-word on "Brave". Measured against
`selected_app_went_missing` with a catalog of `Brave`, `Anki`, `System
Settings`: it does not fire. `B` is a substring of `Brave`, every beckon
resolver ends in a case-insensitive substring tier, so the name still
resolves and the row is not `missing`. Pinned by
`a_prefix_of_the_intended_name_still_resolves_and_is_not_held` in
`crates/beckon-core/src/settings.rs`, next to the test that shows what the
guard actually catches: a name edited into something that matches nothing
in the catalog at all (`an_app_edited_into_a_name_nothing_matches_holds_the_write`).

The guard is right; the illustration is not. It is a TRANSITION guard, not
a broken-row guard — a row the file already has broken is deliberately NOT
held, because holding it would mean that chord could never be edited again
once Save is gone, every keystroke refused, with the remedy being to fix an
app name the user may have no intention of fixing. Do not "fix" this by
comparing whole strings instead of resolving through the catalog tiers —
that is the defect measured on macmini 2026-08-17 (see "`missing` has now
been wrong twice" above) that made every candidate-chain row say `missing`.

### G-j: the design's own wording covers four refusals, and the build reads it as covering one

Design §6.2's G-j row says: "Keep exactly one prompt: refuse the close when
the model is dirty AND **the last write failed**", against "a dismissed
write-failure dialog turning a whole session of edits into a silent loss."
`autosave_plan` can leave the model dirty for four different reasons —
`FinishTheRow`, `FileMoved`, `AppWentMissing`, `CannotWrite` — and only the
last of those is a write that was attempted and errored; the other three
never tried to write at all. "The last write failed" reads naturally as
covering all four (nothing got saved, in each), and the build instead reads
it as the narrow sense — an attempted write that came back an error — so
only `NotSaved::CannotWrite` prompts on close. In
`crates/beckon-macos/src/settings_window/mod.rs`, `AMENDED 2026-09-23,
Task 10` sits on `may_close()`'s own doc comment — the private function
whose doc explains that `windowShouldClose:` is its one caller — and
states the reading directly: this replaces gating on `dirty` alone, the
way the old three-way Save/Cancel/Discard prompt did, because under
auto-save a dirty model is the ROUTINE state — the debounce window before
a keystroke's write lands, and every one of the four hold reasons leaves
the model dirty too — so prompting on the broad reading would fire on
nearly every close and be trained away, which is exactly the failure
G-j's own "Prevents" column names.

The other three refusals are never silent regardless: the footer readout is
drawn on all four doors, unconditionally, with its own phrase per refusal.
`AppWentMissing` is the closest case — it holds a write that WOULD have
rendered — and it still does not prompt, for a reason stronger than
visibility: writing it would put a broken hotkey live, which is the exact
harm the guard exists to prevent in the first place. `FinishTheRow` in
particular must not prompt: it is true during every half-typed row, and a
prompt that fires constantly is one people dismiss reflexively — the same
training-away failure G-j was written against, arriving from the opposite
direction.

**AMENDED 2026-09-23 (final review, C1 and I6): the close FLUSHES first,
and the predicate is now a tested core function.** Two things this section
described are no longer the whole story.

The predicate was an inline `dirty && matches!(last, Some(CannotWrite))` in
`serve.rs` — the only one of the eleven guards with neither a unit test nor
an on-screen run of its refusal arm, so deleting it would have turned
nothing red. It is `beckon_core::settings::close_is_refused(dirty, last)`
now, beside `remove_needs_confirm`, with four tests of its own (one per
input class, including a CLEAN model over a `CannotWrite` — a failed
`undo_pressed` leaves exactly that, and there is nothing in memory left to
lose) and two in the driver that make the failure with a read-only
directory rather than by setting the field.

And the close **writes before it asks**. The narrow reading above is right
about which refusals should stop a close, and it silently assumed the
dirty-but-not-refused state is one the user can afford to close over. It is
not, in one case: the App field's debounce leaves a keystroke pending for up
to `AUTOSAVE_QUIET_MS` plus a tick, and closing there dropped the model with
the edit in it — with the footer reading `Saved just now`, because nothing
had failed. That is design §6's G-i loss arriving through the ordinary
close, which Task 11 made the only way out. `close_verdict` now calls
`autosave` before answering, so the pending write lands; a hold still holds
(with its own footer phrase), and a failed flush is what the refusal above
is then about.

### G-a: the compare-and-swap, and why the read sits immediately before the write

`autosave`'s own doc comment (`crates/beckon-cli/src/serve.rs`) states it
plainly: **the read is immediately before the write** — that pairing IS the
compare-and-swap guard, and anything inserted between `read_to_string` and
`write_config_text` widens the window in which somebody else's edit lands
unseen and gets silently overwritten. Do not "simplify" by moving a log
line, a notification, or a UI refresh between them.

Two more orderings sit beside it and are equally load-bearing: the undo
entry is pushed BEFORE the write, never after (an entry pushed after a
half-failed write describes a file state that never existed, and Undo
would restore it over a good file — the failure arm takes the entry
straight back off, the same rule from the other side); and the reseed comes
from the text that was WRITTEN, never from a second read of the file
(re-reading invites exactly the race the guard just closed — a write
landing between our rename and our re-read would be adopted as our own
base and silently overwritten by the next keystroke).

### The second `Nothing` arm: dirty is not "the file would change"

`autosave_plan` answers two different questions and used to conflate them
into one. `add_row` marks the model dirty, and `render` correctly drops
the unfinished row it just added — so the rendered text came back
byte-identical to what was already on disk, and the write that followed
cost the user the row they had just added, within one tick: the reseed
threw it away, the selection went to `None`, an `orig_key` was gratuitously
respelled, and a no-op entry landed on the undo stack. Measured 2026-09-23:
`rows 2 -> 1`, `selected Some(1) -> None`.

The fix is the second `Nothing` arm in `autosave_plan`, sitting after
`render` (it needs the rendered text) and before the `AppWentMissing`
check (a write that changes nothing is not a write to refuse — putting
"Not saved - that app name matches nothing" on screen for a no-op would be
a complaint about something that was never going to happen): if the
rendered text equals what is already on disk, answer `Nothing` rather than
`Write`. It belongs in `autosave_plan` rather than in the driver, because
"would this write change the file" is a decision, and a driver owning it is
the shape this phase's constraints forbid.

### G-b: reseed by identity, and the first implementation was wrong in the case it exists for

`Model::view_state` captures what the user is looking at so a reseed can
restore it, and its whole correctness is keying on `combo`, not
`orig_key`. The first implementation keyed on `orig_key`, and
`restore_view_state` matches the reseeded model's `orig_key` — which,
after a write, IS the key the file now carries, the very thing `render`
wrote from `combo` a moment before. Keying capture on `orig_key` therefore
recorded what the row used to be called, which matches nothing the moment
a chord is edited: the selection went to `None` on the exact keystroke
that moved it — G-b failing in precisely the case it was written for.
Measured 2026-09-23, fixed the same day.

### G-c: `toml_edit` 0.22 has no key-rename accessor, and this is why the rename matters

A `Key`'s string IS its identity in the backing `IndexMap`: `Hash`, `Eq`
and `Borrow<str>` all resolve through it, `KeyMut` exposes only decor
accessors, and `Table::insert_formatted` on a spelling not already present
always lands in indexmap's `Vacant` arm — i.e. appends. There is no public
index-aware insert. So a retyped chord is rendered as a rename by capturing
the top-level key order, emptying the table in that order with
order-preserving `remove_entry`, and replaying each pair with
`insert_formatted`, substituting a fresh `Key` (carrying the old key's leaf
and dotted decor) only for the renamed row. Because the table is emptied
first, every append lands back at the walk's current position, reproducing
the original order with the rename substituted in; the trailing `# comment`
lives on the VALUE's decor, never touched, so it survives regardless.

Measured end to end on the real window, airm3, 2026-09-23: changing one
binding's key rewrote a 97-line config with a one-line diff, the binding
still on line 41. Before this guard, the same gesture deleted line 41 and
appended the new spelling at the end of the file — destroying that line's
trailing comment and producing a large diff in a config that, on this
machine, lives inside a git repository.

`config_write::render` documents a precondition it always relied on and
never stated until now: `Model::render` enforces unique combos and filters
unfinished rows before building the `RowWrite`s it passes down. A second
caller that skipped that filtering would get an unrelated key rescued from
deletion and its value overwritten by the wrong row — pinned by a
`debug_assert!` at the top of `render` rather than left to be discovered
by a second caller.

### `autosave_plan` takes `base` and `on_disk` separately, and `Keep mine` is why

"Has the base moved under me?" and "would this write change the file?"
look like one question and are not — `Keep mine` is the gesture that
separates them, overriding the first while leaving the second untouched.
With one shared parameter this happened: the user edits, then undoes the
edits by hand so the render nets back to the model's own `original()`,
while the file on disk holds another party's text. `Keep mine` substituted
the base, the plan saw text equal to THAT base, answered `Nothing`, and the
other party's text was never overwritten — the opposite of what the button
promises — while `external_change` was cleared regardless, so nothing on
screen said so.

The plan now takes `base` and `on_disk` as two separate `Option<&str>`
arguments and the no-op arm compares only against the real file. **Do not
re-merge them.** The old test,
`keep_mine_writes_and_leaves_the_other_text_on_the_undo_stack`, could not
have caught this: under the mutation that reinstates the conflation it
stays green, because its own model never nets back to its own base. A new
test, `keep_mine_writes_when_the_edits_net_back_to_the_base`, was needed —
not a stronger assertion on the old one.

### G-g: Undo pops without pushing, and an external change clears the stack

Both look like omissions and are settled decisions. **Undo pops and does
not push** — recording the restore itself would make the control a
two-state toggle rather than a stack, and Redo is deliberately absent for
the same reason.

**An external change clears the undo stack.** Every entry's base is the
model's `original`; once another writer has touched the file, restoring an
entry would clobber that edit — the exact loss G-a exists to prevent,
arriving through the Undo button instead of a stale write. This is
distinct from a successful auto-save reseed, which does NOT clear the
stack: `carry_undo` moves it, oldest-entry-first, from the model being
replaced onto its replacement, because a reseed is what clears `dirty` and
gives every row a fresh `orig_key`, and without carrying the stack across
that boundary every successful write would empty the history it had just
added to.

### G-h: `<config>.bak` is a session rollback, and it does not cover the stale-base case

`backup_config` copies the config beside itself as `<name>.bak`, once, at
window open — beside the RESOLVED target, for the same reason
`write_config_text`'s temp file is (see "A Save renamed onto the config
path" above): `with_extension` swaps to `.toml.bak`, and `watch_config`,
which compares by file name, cannot mistake the copy for a config write.
The file is overwritten every time the window opens, so at any moment it
holds exactly one snapshot — the state the config was in a moment before
THIS session's first edit.

That makes it a session rollback, and nothing more: one file, good for
undoing a whole session of auto-saved edits the user wants out wholesale,
gone the next time the window opens. **It does not cover the stale-base
case, and a reader must not assume it does.** The compare-and-swap guard
(G-a) is the one actually protecting against another writer's edit landing
unseen — refusing a write whose base moved, with `Keep mine` as the
explicit override. `.bak` cannot stand in for that, and the reason is
timing, not mechanism: `.bak` is written once, at open, before either
version of the later conflict exists. A stale-base clobber happens later,
mid-session, after an external edit has already landed — so by the moment
G-a's guard would matter, `.bak` already predates BOTH the model's current
text and whatever the other writer put on disk. Restoring from it recovers
neither. Best effort, and deliberately so: a failure to write it is not
worth refusing to open the window over, and there is nothing a user could
do about a failed backup from inside this window anyway.

### Two guards that need no separate story: G-f and G-i

G-f (`remove_needs_confirm`) asks before a Remove when more than one row is
ticked, or when any deletion happens while the view is filtered — the
filter arm is the sharper one, because "I am looking at a subset" is the
state in which a multi-delete surprises somebody, per four-doors design
6.3's own measurement of a filter that looked complete while it was not.
Auto-save is what makes this load-bearing at all: the old reasoning for a
silent multi-row delete rested on Save still being a gate, and auto-save
falsifies that — the delete reaches the file before the user has looked.

G-i (flush on session end) is a Win32 message, and Windows still keeps its
Save button in this phase, so there is nothing for it to flush ahead of.
Revisit when Windows consumes this auto-save layer.

**CORRECTED 2026-09-23: that disposition was right about the MECHANISM and
wrong about the HAZARD, and it cost this phase its one Critical.**
`WM_QUERYENDSESSION` is indeed a Win32 message with nothing behind it here.
But G-i is about *a pending debounced write dying when the session ends*,
and on macOS **closing the window ends the model's session as finally as a
logoff does** — `forget_settings` drops the model, and there is no later
tick to write what the debounce was still holding. Task 11 then made that
close the only way out of the window. So the phase's own headline hazard
arrived through the phase's own new front door, with the footer reading
`Saved just now` while the keystroke was discarded.

The fix is in `serve.rs`'s `close_verdict` (C1): flush through `autosave`
before deciding, guards and all. The lesson generalises past this one
guard, and it is the same one `CLAUDE.md` records about carrying a measured
sentence across a platform boundary: **a disposition written about one
mechanism does not dispose of the hazard.** When a guard is scoped out,
name the hazard and ask which gestures on THIS platform produce it, rather
than which API the design names.

**And asking that question found a second door the same day: Quit.**
`beckon_macos::tray::request_quit` is `std::process::exit(0)` and its own
comment already said **"`Quit` never reaches a window delegate"** — so
`windowShouldClose:`, `close_request` and the flush it had just gained were
not on that path at all. Same loss, more final door. The flush is therefore
**one body with two callers**: `flush_pending_write` in `serve.rs`, called by
`close_verdict` (which then decides whether to refuse, G-j) and by
`quit_flush` (which cannot refuse anything and returns). Two spellings of
that rule would have been two chances to fix only one of them.

Three things about the Quit half are worth keeping:

- **A last chance is not a licence.** The flush goes through `autosave`, so a
  stale base still holds and an unrenderable row still writes nothing. Quit
  proceeds regardless: what is lost is the in-memory edit, and what told the
  user is the footer, which had been naming the refusal continuously up to
  the click. Nothing new is said at the moment of the quit — the window goes
  with the process.
- **`process::exit` runs no destructors, and nothing on this path needs one.**
  `write_config_text` is `fs::write` (write_all and the close, both inside
  the call) then `fs::rename`: two synchronous syscalls that have returned
  before `autosave` does, before `quit_flush` does, and before the menu arm
  reaches `request_quit()`. There is no buffered writer and nothing spawned
  or queued. Durability across a power cut is a different question and has
  never been claimed — no `fsync`, here or anywhere else beckon writes.
- **The third `process::exit` on macOS was assessed and left alone.**
  `GrantRecovery::RestartUnderLaunchd` (`serve.rs`) exits so launchd can
  restart beckon once Accessibility is granted. Reaching it with a pending
  write needs the grant to land in System Settings within `AUTOSAVE_QUIET_MS`
  of a keystroke in beckon's own window — the user must switch apps and click
  a toggle inside 600 ms, and the 0.25 s `autosave_tick` is running the whole
  time. It also only exists in a session that started without the grant.
  Adding the flush there is one line if that judgement ever looks wrong.

## CORRECTED 2026-09-23: the service line lagging the registration map was blamed on the wrong commit

After an auto-save write the footer showed `Serving - 19 of 20` in warning
orange and stayed there — including after Undo restored the file — while
`serve.log` said `reloaded - 20 shortcuts registered` three times over.
Closing and reopening the window showed `20 of 20`.

**The first diagnosis blamed a commit from 2026-08-16. That account is
wrong, and is kept here rather than deleted because that is what a CORRECTED
marker is for.** `git log -S "let ours" -- crates/beckon-cli/src/serve.rs`
finds exactly one commit that ever introduced that string: `67ab70c`,
`feat(serve): every edit runs the autosave plan (G-a, G-h)` — this phase's
own auto-save commit, not anything from a month earlier. Before `67ab70c`,
`settings_saw_external_change`'s self-write suppression (the `ours` check
comparing `model.original()` against the file's live bytes) did not exist:
a clean model — exactly the state right after a successful write reseeds —
fell through unconditionally to `reload_settings_from_disk`, which always
ends in a full `refresh_settings`. `67ab70c` added `ours` to stop the
banner firing about beckon's own write on every keystroke, a real fix that
still stands, and in the same change dropped the refresh the old
unconditional path had carried. **The gap arrived with the self-write
suppression and was fixed the same day**, not months later.

The fix keeps the suppression exactly as it was — by content equality, not
a timer or a flag, because if the file's bytes already equal what the model
holds there is genuinely nothing external to report — and adds back a
`refresh_settings(state)` call inside the `ours` branch. `reload()` calls
`settings_saw_external_change` AFTER re-registering and writing the fresh
outcome into `ServeState::registered`, which is the one thing that made the
write worth catching in the first place, and the open window had not been
told about it.

One present-tense fact survives from the wrong account unchanged: **Save
shows the same stale count today**, because `apply_settings` never touches
`s.registered` at all — only `reload()` does — so a chord edit followed by
Save reaches the same stale map.

**NARROWED 2026-09-23: that is a Windows sentence.** It was written after
the macOS button came down and still said "on both platforms, this function
being `any(windows, macos)`" — true of `settings_saw_external_change`, which
is where the stale map is read, and false of the gesture. There is no Save
press on macOS to follow a chord edit with: `apply_settings` is
`#[cfg(target_os = "windows")]` and `cb.on_apply` is raised from one place in
the program, `beckon-windows`' `IDC_APPLY` handler. On macOS the only thing
that reaches this branch is auto-save's own write, which is what the
`refresh_settings` call above was added for.
