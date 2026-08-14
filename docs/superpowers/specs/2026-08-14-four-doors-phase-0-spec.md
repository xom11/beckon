# Four Doors — Phase 0: the numbers everything else is derived from

**Status: spec, agreed 2026-08-14.** Input to
`docs/superpowers/plans/2026-08-14-four-doors-phase-0.md`.

Parent design: `2026-08-14-four-doors-settings-window-design.md` (the four-tab
window). Drawing: `2026-08-14-four-doors-mockup.html`.

**Phase 0 produces nothing visible, and that is the point.** Every one of the
four workstreams that follows derives a number, an id or a signature from
something this phase fixes. Landing it first means those four never re-derive
a figure that is already wrong in the source, and never collide on an id
range — which two earlier specs already did, ten levels deep, in a way
`GetDlgItem` resolves silently.

Nothing here changes what the window looks like. In particular **the window
stays 760 × 600**; the move to 680 belongs to the shell workstream, which will
make it by editing one constant because of what this phase does.

---

## 1. What is wrong today, with evidence

Every claim below was read out of the source in this session. Line numbers are
as of commit `e42f5bb`.

| # | Claim | Evidence |
|---|---|---|
| 1 | The window is 760 × 600 with a 660 × 560 floor | `settings_window/mod.rs:702-703`, `mod.rs:824-825` |
| 2 | A 60-line comment block immediately above those constants derives a **900 × 740** window in full, then says it is superseded but keeps the table | `mod.rs:642-701` |
| 3 | The `MIN_WIDTH` doc reasons from `720 * 900/860` and from "a **753 px** window with a 16 px frame", and derives a floor of **697 / 702** | `mod.rs:725-738`, `mod.rs:747-795` |
| 4 | The one place in the module that reasons about the width-critical line reads `MIN_WIDTH` as **753** — 93 px above the real 660 — and its own text admits the check "has never been checked against it by arithmetic" | `layout.rs:319-327` vs `mod.rs:824` |
| 5 | The hardware probe **prints 900 × 740 as the expected geometry** and 753 × 702 as the floor, and reports `MATCH` / `<<< FAIL` against them | `examples/settings_probe.rs:299-305`, used at `settings_probe.rs:1788-1804` |

Claims 2–5 are four independent copies of a geometry that no longer exists.
Claim 5 is the expensive one: the probe is the instrument the hardware gates
are read through, and today it would report `<<< FAIL` on a **correct** window.
A gate that fails on a healthy build is worse than no gate — it trains the
reader to ignore it.

**Why the existing safeguard did not catch it.** `settings_probe.rs:292-298`
states the copy is deliberate: an independent transcription, so that "a later
resize that changes one without the other shows up as a disagreement on
hardware rather than being absorbed silently". The reasoning is sound and the
mechanism is real — but it only fires when a person is at a14. The window was
resized on 2026-08-13 and the disagreement has been sitting in the file since,
undetected, because nobody has run the probe on hardware in between.

The fix is therefore **not** to delete the independent copy. It is to keep it
and add the check that can run without a person: §2.3.

**CORRECTED 2026-08-14, by this phase's own execution: the count is not five.
Fourteen copies have been corrected on this branch, and fourteen is not a
ceiling.** The table's five claims are each right about themselves — every cite
in it was re-read while fixing it and every one held. What is wrong is the
arithmetic in the sentence under it, and every later line that quotes the same
total: §2's heading and §2.2's "the four copies", and §7's "three of the five
geometry copies". Nine more copies turned up in the course of fixing the five,
in three waves. Line numbers in this table are as of `7d8eae3`, not `e42f5bb`,
because the earlier waves moved them; every cite below was checked with
`git show 7d8eae3:<file>`.

| Where | What it asserted | Corrected in |
|---|---|---|
| `mod.rs`, `notes_height`'s doc | the same `MIN_HEIGHT` relationship, anchored at **697 / 702** | `9e4e026` |
| `layout.rs:258` | the banner costs **76 px** | `9e4e026` |
| `mod.rs:3801`, in `apply_state` | the banner costs **~76 px** | `9e4e026` |
| `mod.rs:1068-1070`, `list_row_height`'s empty-list fallback | `scale(26, dpi)`, **39 px** at 144 DPI | `e50ed34` |
| `mod.rs:3365-3367`, the anchor sentence | **four** anchors — Task 8's **675** was lost when three re-derivation paragraphs were compressed into one | `e50ed34` |
| `layout.rs:23, 34, 38`, `mod tok`'s own doc | `CTL` is **32** (said twice), `GAP_CARD` (**12**), `GAP` (**8**) | this pass |
| `layout.rs:699`, the `IDC_CAPS` budget | `tok::GAP` (**8 px**) before the caption | this pass |
| `paint.rs:1336`, the paired half of that budget | the same sentence, in `toggle`'s doc | this pass |
| `examples/settings_probe.rs:801` | `chrome::TITLEBAR_H` (**40** @96 DPI) | this pass |

Shipped values, for reading that table against: `PAD 10`, `CARD_PAD 11`,
`GAP_CARD 8`, `GAP 6`, `LABEL 10`, `CTL 26`, `ROW_H 22`, `TITLEBAR_H 34`. All
eight moved in one commit, `1f46335` (2026-08-13).

**Why a hand-listed count was always going to be short**, which is the part
worth writing down. The five in the table are the *window's own dimensions*,
and they are searchable as such: `900`, `740`, `753`, `702` are four distinct
strings that occur for no other reason, so a grep for them finds every copy and
the sweep that produced the table was exactly that grep. Every one of the nine
is a different thing — a figure **derived** from a token, written out as a
literal, in prose that nothing compiles and no test reads. A card's
`CARD_PAD*2 + CTL`, `scale(ROW_H, dpi)` at 144 DPI, `s(50)` minus a track and
an inset: each lands on a small integer that occurs a hundred times for
unrelated reasons, so there is no string to search for and no compiler edge
between the token and the sentence. The first sweep looked for the window, and
found every copy of the window. It could not have found the tokens the window
is made of.

`e50ed34`'s note on `list_row_height` is the worked example: that one site went
stale twice, once per token move, for this exact reason, and the second time
there was nothing left in the tree to check it against.

Two consequences, neither of which changes an instruction below. **§2's list of
four is the list the first sweep found, not an inventory of the geometry prose**
— §2.2 stays as written and is executed as written; the other nine are fixed
alongside it. And **the test §2.3 adds cannot catch this class**: it compares
four named constants against four literals in one other file, which is the
window's dimensions again. Nothing in Phase 0 builds an instrument that reads a
token value out of `mod tok` and checks the prose against it.

**A comment that records what a value USED to be is not one of these.**
`mod tok` says "`BAND` (14) is gone" and "Was `BAND` (14) before Task 8"; both
are history, both are accurate, and neither is touched. The defect is a
sentence whose tense asserts what a value **is** while quoting a number that
has moved.

Two more facts this phase depends on:

| Claim | Evidence |
|---|---|
| Adding a field to `Callbacks` is a hard **E0063 on macos-latest** | `beckon-macos/examples/settings_probe.rs:112-192` builds the struct as a complete literal with no `..`; CI clippy runs `--all-targets` (`.github/workflows/ci.yml:93`) |
| `settings_window::open` has **two** macOS call sites | `beckon-macos/src/settings_window.rs:615`, `beckon-macos/examples/settings_probe.rs:194` |
| …and one Windows definition plus one shared caller | `beckon-windows/src/settings_window/mod.rs:1775`; `beckon-cli/src/serve.rs:1602` |
| `ServeState` already holds both paths the window needs | `serve.rs:198-216` — `config: PathBuf`, `log: Option<PathBuf>` |
| The filter matches the Shortcut column | `settings.rs:437-452`, pinned by the test at `settings.rs:2723-2733` |

---

## 2. Geometry: one source, four honest copies

### 2.1 The constants do not move

`WINDOW_WIDTH 760`, `WINDOW_HEIGHT 600`, `MIN_WIDTH 660`, `MIN_HEIGHT 560`
stay exactly as they are. Phase 0 changes only the things that **disagree**
with them.

### 2.2 What each of the four copies becomes

- **`mod.rs:642-701`** — the 900 × 740 derivation table is deleted, not
  annotated. It has already been superseded once in place, and the result is
  that the file now carries a full derivation of a window that does not exist
  plus a paragraph saying so. What is kept is the part the comment itself
  identifies as still useful — *which terms compose the height, in what
  order* — restated against the shipped tokens, plus the measured evidence
  (1140 × 900 at 144 DPI on a14, which is 760 × 600 × 1.5).
- **`mod.rs:725-738`** — the `720 * 900/860` proportionality argument and the
  "753 px window" worked example are replaced by the same argument at 660.
  The two zero points it computes (a card heading at raw client width ≈364, the
  editor key list at ≈551) are properties of `layout`, not of `MIN_WIDTH`, and
  survive unchanged; what changes is the margin arithmetic they are compared
  against.
- **`mod.rs:747-795`** — the derivation table sums to 697 and the text says
  "shipped as 702". Both are pre-compaction figures. The table is re-derived
  against the shipped tokens (`PAD 10`, `CARD_PAD 11`, `GAP_CARD 8`, `GAP 6`,
  `CTL 26`, `ROW_H 22`, `TITLEBAR_H 34`) and must land on **560**, or the
  discrepancy is reported rather than papered over.
- **`layout.rs:319-327`** — `MIN_WIDTH (753)` becomes `MIN_WIDTH (660)` and
  the "≈547 px of a 705 px card interior" worked example is re-run at 660.
  **This is the one whose conclusion may change**: the note says the Caps line
  clears its ceiling "by luck rather than by anyone's re-derivation" at a width
  that is 93 px wider than the real floor. If the arithmetic at 660 does not
  close, the plan says so in the commit message and gate **G1** decides;
  Phase 0 does not move `MIN_WIDTH` to make it close.

### 2.3 The probe keeps its copy and gains a test

`examples/settings_probe.rs:299-305` is corrected to `760 / 600 / 660 / 560`.
The independent-copy rationale in its doc comment stays.

New: a `#[test]` **inside `beckon-windows`** that reads the example's source
with `include_str!("../../examples/settings_probe.rs")` and asserts each of the
four `const … = <n>;` lines is present with the value the module holds. It runs
on the ordinary `cargo test` of the Windows CI job — no hardware, no person.

This is the piece the original design was missing: it preserves the runtime
independence (the probe still hard-codes its own numbers and can still catch a
binary that is older than the probe) while making a *source-level* drift
impossible to land. A comparison of two literals in one crate is exactly the
kind of check that belongs to a compiler run, not to a laptop in another room.

---

## 3. Control ids: one table, in `beckon-core`, with three tests

### 3.1 Why this is in Phase 0 and why it is in core

`layout` positions controls through `GetDlgItem`, which resolves duplicates to
the first match — so two controls sharing an id means one is placed and the
other is silently left at the origin. That failure has already shipped once in
this window (`mod.rs:315-317` records it: three labels sharing `-1`).

Two of the earlier Four Doors drafts each claimed `1060-1069` for a different
page. Nothing in the toolchain would have said a word.

The table lives in `beckon-core::settings` so the tests run on **all three** CI
jobs, not only the Windows one — the same reason `ControlState` and
`DefaultButton` are there.

### 3.2 The ranges

Disjoint by construction. A range is a page's, and a page's controls never
appear on another page.

| Range | Owner | Notes |
|---|---|---|
| 1001-1008 | in use, **pinned** | `examples/settings_probe.rs:229-242` hard-codes them |
| 1009-1011 | **retired forever** | the three `Tapping Caps alone` radios (`mod.rs:306-309`) |
| 1012, 1013 | in use, **pinned** | `IDC_OPENFILE`, `IDC_CLOSE` |
| 1014-1027 | in use, unpinned | banner, labels, filter, Hold/Tap |
| 1028-1031 | in use, **pinned** | the four modifier chips |
| 1032-1039 | in use, unpinned | Record/Revert, editor caption, count |
| **1040-1049** | **shell** | the tab strip and the command bar |
| **1050-1059** | **Shortcuts page** | reserved; the page reuses its existing ids |
| **1060-1069** | **Keyboard page** | |
| **1070-1099** | **System page** | |
| **1100-1119** | **About page** | |

### 3.3 The new ids

Assigned now, in full, so no later workstream has to pick one — that is what
made the collision possible. A workstream may leave an id unused; it may not
choose a different number for the control named here.

**Shell — 1040-1049**

| Id | Name | Control |
|---|---|---|
| 1040 | `IDC_TAB_SHORTCUTS` | pill; `BS_AUTORADIOBUTTON \| BS_PUSHLIKE \| WS_GROUP` |
| 1041 | `IDC_TAB_KEYBOARD` | pill |
| 1042 | `IDC_TAB_SYSTEM` | pill |
| 1043 | `IDC_TAB_ABOUT` | pill |
| 1044 | `IDC_SERVICE_LINE` | the left half of the command bar |
| 1045 | `IDC_SAVED` | the `Saved` readout |
| 1046 | `IDC_UNDO` | guard G-g's visible control |
| 1047-1049 | — | reserved for the shell |

**Keyboard — 1060-1069**

| Id | Name | Control |
|---|---|---|
| 1060 | `IDC_CAPS_SHORTHAND` | `Write shortcuts as [Caps] instead of [Ctrl][Win][Alt]` |
| 1061 | `IDC_TROUBLE_HEAD` | the expander header |
| 1062 | `IDC_TROUBLE_BODY` | its body |
| 1063-1069 | — | reserved for Keyboard |

`IDC_CAPS 1008`, `IDC_HOLD_* 1022-1024`, `IDC_TAP 1025`, `IDC_LBL_HOLD 1026`
and `IDC_LBL_TAP 1027` move to this page **without renumbering**.

**System — 1070-1099**

| Id | Name | Control |
|---|---|---|
| 1070 | `IDC_PAUSE` | switch → the tray's own `set_paused` |
| 1071 | `IDC_AUTOSTART` | switch; **omitted**, not disabled, under `beckon.exe serve` |
| 1072 | `IDC_SYS_RELOAD` | button → the tray's own `reload`. **Not** `IDC_RELOAD 1015`, which is the banner's "reload from disk" and answers a different question |
| 1073 | `IDC_DARK` | switch, default ON |
| 1074 | `IDC_OPACITY` | `msctls_trackbar32`, 85-100 |
| 1075 | `IDC_OPACITY_VALUE` | `96%`, or the reason it is forced off (rule 7 — same line, never a tooltip) |
| 1076 | `IDC_CONFIG_NAME` | `apps.windows.toml` |
| 1077 | `IDC_CONFIG_DIR` | `…\shortcuts\` |
| 1078 | `IDC_CONFIG_OPEN` | ↗ glyph |
| 1079 | `IDC_CONFIG_SHOW` | ▤ glyph |
| 1080 | `IDC_LOG_NAME` | `beckon-serve.log` |
| 1081 | `IDC_LOG_SIZE` | `112 KB` |
| 1082 | `IDC_LOG_OPEN` | ↗ glyph |
| 1083 | `IDC_LOG_SHOW` | ▤ glyph |
| 1084-1099 | — | reserved for System |

**About — 1100-1119**

| Id | Name | Control |
|---|---|---|
| 1100 | `IDC_ABOUT_MARK` | the app mark |
| 1101 | `IDC_ABOUT_NAME` | `beckon 0.9.3` |
| 1102 | `IDC_ABOUT_BUILD_LABEL` | `Build` |
| 1103 | `IDC_ABOUT_BUILD_VALUE` | `aarch64-pc-windows-msvc · 2026-08-13` |
| 1104 | `IDC_ABOUT_BUILD_COPY` | ⧉ |
| 1105 | `IDC_ABOUT_LOCATION_LABEL` | `Location` |
| 1106 | `IDC_ABOUT_LOCATION_VALUE` | the **running image** path, unresolved |
| 1107 | `IDC_ABOUT_LOCATION_COPY` | ⧉ |
| 1108 | `IDC_ABOUT_LICENCE_LABEL` | `Licence` |
| 1109 | `IDC_ABOUT_LICENCE_VALUE` | `MIT OR Apache-2.0` |
| 1110 | `IDC_ABOUT_LICENCE_COPY` | ⧉ |
| 1111 | `IDC_ABOUT_DISCLOSURE` | the hook + `SendInput` disclosure |
| 1112 | `IDC_ABOUT_GITHUB` | |
| 1113 | `IDC_ABOUT_RELEASES` | |
| 1114 | `IDC_ABOUT_BUG` | |
| 1115-1119 | — | reserved for About |

Every label above gets a real id rather than `-1`, per `mod.rs:315-317`.

**Retired, and never reclaimed: 1009, 1010, 1011.** Auto-save removes the
`Save` and `Close` buttons from the window, but **`IDC_APPLY 1007` and
`IDC_CLOSE 1013` are not retired and not reused** — `settings_probe.rs`
hard-codes both, and a probe that found a *different* control answering 1007
would report a confident wrong result. They become ids with no control.

### 3.4 Timers

`IDT_CAPTURE 1` (`mod.rs:392`) is joined by `IDT_AUTOSAVE 2`. The doc comment
at `mod.rs:390-391` — "The only timer this window owns, so the `WM_TIMER` arm
can identify it by id alone" — stops being true when the auto-save workstream
lands and is edited **there**, not here. Phase 0 reserves the number in the
table only.

### 3.5 The three tests

In `beckon-core::settings`, against a `pub const CONTROL_IDS: &[(&str, i32)]`
and a `pub const RETIRED_IDS: &[i32]`:

1. `ids_are_unique` — no value appears twice in `CONTROL_IDS`.
2. `retired_ids_stay_retired` — no value in `RETIRED_IDS` appears in
   `CONTROL_IDS`.
3. `probe_pinned_ids_have_not_moved` — the eleven ids
   `examples/settings_probe.rs` hard-codes still carry the names it expects:
   1001 `LIST`, 1002 `COMBO`, 1003 `APP`, 1005 `ADD`, 1006 `REMOVE`,
   1007 `APPLY`, 1008 `CAPS`, 1012 `OPENFILE`, 1013 `CLOSE`, and
   1028-1031 the four chips.

A fourth property — *the Windows module's own constants match this table* —
cannot be a core test, because core cannot see them. It is a `#[test]` in
`beckon-windows` that walks `CONTROL_IDS` and compares each entry against the
module constant of the same name. It runs on the Windows CI job.

**This table is documentation with a test attached, not the definition.** The
Windows constants stay where they are; making core the definition would put a
Win32 concept in the crate whose whole purpose is to be free of one.

---

## 4. `Callbacks` gains exactly one field

```rust
/// Everything the window can ask the caller to DO that is not an edit to a
/// binding. One field rather than fifteen: `beckon-macos`'s probe builds this
/// struct as a complete literal with no `..` (examples/settings_probe.rs:112)
/// and CI clippies it with --all-targets, so every added field is a hard
/// E0063 on macos-latest. That is a real cost paid by a real job, not a
/// hypothetical.
pub on_command: Box<dyn FnMut(SettingsCommand)>,
```

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsCommand {
    /// The user moved to another tab. The caller stores it so the next open
    /// lands where they left off.
    ShowPage(Page),
    SetPaused(bool),
    SetAutostart(bool),
    /// The System page's Reload — the tray's own, not the banner's.
    ReloadNow,
    SetDarkMode(bool),
    /// 85..=100. The window clamps; the caller may assume the range.
    SetOpacity(u8),
    SetCapsShorthand(bool),
    Open(Target),
    Reveal(Target),
    Copy(Field),
    Undo,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target { Config, Log, Github, Releases, BugReport }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field { Build, Location, Licence }
```

`Copy + Eq` so a caller can match, log and test one without cloning; no
variant carries a `String`, which is what keeps that true.

**`on_open_file` is not folded into `Open(Target::Config)` in this phase.**
It exists, it works, and rewiring it would put a behaviour change inside the
commit whose entire value is that it has none. The System workstream folds it
and deletes the field.

Phase 0 gives every call site a `Box::new(|_| {})` — except `serve.rs`, which
gets a `match` with one arm per variant and `todo!()`-free empty bodies, so
that adding a variant later is a non-exhaustive-match error at the one site
that must handle it.

---

## 5. `open(cb, &Paths, Page)`

```rust
/// Where the two files this window talks about live.
///
/// `log` is `None` when `serve` was started without `--log`; the System
/// page omits the row rather than showing a path that does not exist.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Paths {
    pub config: PathBuf,
    pub log: Option<PathBuf>,
}

/// Which door the window opens on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Page {
    #[default]
    Shortcuts,
    Keyboard,
    System,
    About,
}
```

Both in `beckon-core::settings`. `Page` in core settles §12 open question 3
the way that question itself points: it makes
`DefaultButton::visible(external_change, page)` testable on all three CI jobs,
which is the stated reason `DefaultButton` is in core at all.

New signature, identical on both platforms:

```rust
pub fn open(cb: Callbacks, paths: &Paths, page: Page) -> Result<(), String>
```

Four files, one commit — the signature cannot be landed in pieces:

| File | Change |
|---|---|
| `beckon-windows/src/settings_window/mod.rs:1775` | definition; `CFG` holds a `Paths` instead of a `String`; `title_base` takes `&paths.config` |
| `beckon-macos/src/settings_window.rs:615` | definition; the title at `:824` reads `paths.config.display()` |
| `beckon-macos/examples/settings_probe.rs:194` | call site |
| `beckon-cli/src/serve.rs:1602` | call site; builds `Paths` from `ServeState::{config, log}` (`serve.rs:198-216`) |

`page` is accepted and stored in this phase; **nothing reads it yet**, because
there is nothing to switch. The shell workstream is what gives it an effect.
Storing it now is what lets that workstream be a change to one module.

---

## 6. The shipping bug: `Remove` under a filter

`Model::visible` (`settings.rs:437-452`) matches the filter against `r.app`
**and** `r.combo`. Every beckon chord contains `alt`, so a filter of `a` — a
plausible first keystroke of "brave" — matches every row: the box looks
filtered and the list is showing everything. Tick "the visible rows", press
Remove, lose the table. Measured with four bindings.

**Fix, decided 2026-08-14: `visible()` matches `r.app` only.**

```rust
.filter(|(i, r)| self.selected == Some(*i) || r.app.to_lowercase().contains(&f))
```

The selected-row exemption at `settings.rs:446` stays; `settings.rs:425-434`
records what it prevents and none of that changes.

**What this costs, stated rather than buried.** The test being replaced
(`settings.rs:2723-2733`) carries a real justification: *"the question this
file is usually opened to answer is what a key is already bound to."* After
this change, `beckon`'s own window cannot answer it by filtering — the user
reads the Shortcut column instead, which for a list this size is what they
were doing anyway. If that turns out to bite, the way back is **not** to
restore substring matching on the whole chord: it is to match the chord's
**key** only (`f2`, `b`), which is the half a person actually searches for and
the half that is not `alt` on every row.

This fix is independent of auto-save and lands in Phase 0 regardless. Guard
**G-f** (confirm a multi-row Remove) is still owed by the auto-save
workstream; this makes the trap far less likely, not impossible.

---

## 7. Gates

Phase 0 is all software and needs **no hardware**. Its gate is the local
tri-target build, in CI's own shape (a bare workspace clippy cannot pass on
macOS):

```sh
cargo fmt --all -- --check
cargo test  --workspace --exclude beckon-linux --exclude beckon-windows
cargo clippy --workspace --exclude beckon-linux --exclude beckon-windows \
      --all-targets -- -D warnings
cargo clippy --target aarch64-pc-windows-msvc -p beckon-windows \
      --all-targets -- -D warnings
```

The fourth line is what makes `beckon-windows` compile at all on this Mac, and
`--all-targets` is what compiles `examples/` — where three of the five geometry
copies and every pinned id live.

**One hardware item is created, not run**: after §2.3, running
`settings_probe.rs` on a14 must print `MATCH` for the window size. It is
recorded against gate G1, whose measurement it is a precondition for — a probe
that prints `<<< FAIL` on a healthy window makes G1 unreadable.

---

## 8. Explicitly not in Phase 0

- Any change to `WINDOW_WIDTH` (760 → 680 is the shell workstream's).
- Any tab, page, switch, slider or auto-save behaviour.
- Deleting `tok::ROWS`, un-capping the list, or touching `Ui::shown_empty`
  (§4 and §12 open question 2 of the parent design).
- Folding `on_open_file` into `SettingsCommand::Open` (§4).
- Editing `IDT_CAPTURE`'s "the only timer" comment (§3.4).
- New `Alt` mnemonics of any kind.
