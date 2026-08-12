# Settings window — Landing 3a (recompose) + gate probes — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Land Part A of `docs/superpowers/specs/2026-08-12-settings-keycaps-design.md` — the editor becomes one titled group of two lines with a real empty state — plus the display-string half of §B.4, plus the two probes that unblock landing 3b.

**Architecture:** Three lanes. `beckon-core` gets two pure display functions with full unit tests that run on all three CI jobs. `beckon-windows` gets a `layout` recompose and two new `examples/` probes; it cannot be tested from a Mac but it **can** be type-checked and linted from one (see *Inner loop*). Everything that needs a desktop is deferred to a single hardware pass on a14, listed at the end in the order it must run.

**Tech Stack:** Rust, raw Win32 via `windows 0.61`, comctl32 v6, no new dependencies.

## Global Constraints

Copied from the spec; every task's requirements implicitly include these.

- **No new dependency.** The workspace `Cargo.toml` is not edited by this plan.
- **No config format change, no new config key.** `super` stays in the file.
- **No literal colour ships.** Every colour comes from `GetSysColor`. (Relevant to 3b; stated here so it is not lost between plans.)
- **Light only, `GetSysColor` throughout**; high contrast is the supported dark path.
- **UI language is English.** Vietnamese appears in commit messages and docs, never in a control caption.
- **Reserved control ids 1001–1007 are pinned** by `crates/beckon-windows/examples/settings_probe.rs`. New ids start at 1034 (1033 `IDC_RESET` is the current maximum).
- **Mnemonic uniqueness is maintained by hand** against the table in `mod cap` (`crates/beckon-windows/src/settings_window.rs:~370`). There is no test. Anything this plan adds carries **no** mnemonic.
- **`layout` must not be called on a data push.** `layout` reaches `SetWindowPos` on the populated App combo, which is the measured data-loss path guarded by `Ui::shown_external`. Caption changes go through `SetWindowTextW`, never through `layout`.
- **rustfmt covers cfg-gated modules.** `cargo fmt --all -- --check` sees `crates/beckon-windows/src/*` from a Mac — measured on rustfmt 1.9.0-stable, recorded in `CLAUDE.md`. Do not skip it.

## Inner loop (verified on this machine, 2026-08-12)

`beckon-windows` is `#[cfg(target_os = "windows")]`-gated out of `lib.rs`, so a plain `cargo check` on macOS compiles **none** of it. Cross-checking does, because `cargo check` never links and therefore never needs MSVC:

```bash
cargo check  -p beckon-windows -p beckon-cli --target aarch64-pc-windows-msvc --all-targets
cargo clippy -p beckon-windows              --target aarch64-pc-windows-msvc
cargo fmt --all -- --check
```

Measured: 7.6 s cold, 1.4 s warm. All three succeed on macOS 25.5 with only
`rustup target add aarch64-pc-windows-msvc`. `--all-targets` is what pulls in
`examples/` and the `[[bin]]` targets; without it the probes in Tasks 3 and 4
are never compiled.

**What this loop does NOT give you:** `cargo test -p beckon-windows` (the tests
are Windows binaries), linking, and anything with a window in it. Those are CI
(`windows-latest`, which runs `cargo build --workspace --all-targets` and
`cargo test --workspace`) and the hardware pass.

## Scope: why 3b is not in this plan

The spec's landing order is G1 → 3a → 3b → 3c. This plan is **G-probes + 3a**,
and stops there deliberately:

- §B.2's cap metrics (`min(row_h - s(6), s(19))`, `s(5)` padding, `s(3)`
  between caps) are written in 96-DPI units, and **G1 is the first time anyone
  will have looked at this window at 96 DPI**. Writing 3b's tasks now means
  writing them against numbers nobody has, which is the exact mistake G1 exists
  to prevent.
- §B.6's scope is decided by G3's answer, not chosen.

Task 2 pulls the one piece of 3b that has no such dependency — the display
string — forward into this landing, because it removes `super` from the
interface with zero drawing code. §7.2 of the spec already names it as the
honest fallback if 3b never happens.

Write the 3b plan after the hardware pass returns G1 and G3.

---

## File structure

| File | Change | Responsibility |
|---|---|---|
| `crates/beckon-core/src/shortcuts.rs` | Modify (append near `combo_view`, ~line 175) | `key_label`, `combo_caps`, `combo_display` + tests. Display only; must stay separable from `Combo::canonical` (line 235), which is what writes the file. |
| `crates/beckon-windows/src/settings_window.rs` | Modify | `IDC_GRP_EDITOR`, `role_of` entry, `mod cap` caption, `build_children` group box, `layout` band 4, `apply_state` empty state, `cells` display string, `tok` deletions |
| `crates/beckon-windows/examples/showhide_probe.rs` | Create | G2: does `ShowWindow` corrupt a populated `CBS_DROPDOWN`? |
| `crates/beckon-windows/examples/customdraw_probe.rs` | Create | G3: does `CDRF_SKIPDEFAULT` on subitem 0 remove the `LVS_EX_CHECKBOXES` tick? |

Both probes follow the house pattern set by `examples/combo_probe.rs`: build
the control in-process, drive it, print `KEY=VALUE` lines, and **always run a
control case in the same run** — a broken probe and a clean result are
indistinguishable without one.

---

## Task 1: Display labels for a chord (`beckon-core`)

**Files:**
- Modify: `crates/beckon-core/src/shortcuts.rs` (insert after `combo_view`, which ends ~line 176; tests go in the existing `#[cfg(test)] mod tests`)

**Interfaces:**
- Consumes: `Combo::parse`, `key_table()`, `ComboView` — all existing in this file.
- Produces:
  - `pub fn key_label(name: &str) -> String`
  - `pub fn combo_caps(s: &str) -> Vec<String>`
  - `pub fn combo_display(s: &str) -> String`

  Task 2 calls `combo_display`. The 3b plan will call `combo_caps`.

- [ ] **Step 1: Write the failing tests**

Append inside the existing `#[cfg(test)] mod tests` block in
`crates/beckon-core/src/shortcuts.rs`:

```rust
#[test]
fn caps_spell_the_chord_the_way_a_keyboard_does() {
    assert_eq!(
        combo_caps("ctrl+super+alt+t"),
        vec!["Ctrl", "Win", "Alt", "T"]
    );
    assert_eq!(
        combo_caps("ctrl+super+alt+shift+bracketright"),
        vec!["Ctrl", "Win", "Alt", "Shift", "]"]
    );
    // Modifier order is fixed by this function, not by the input string:
    // `Combo::parse` accepts free order, the display must not vary with it.
    assert_eq!(combo_caps("alt+ctrl+f10"), vec!["Ctrl", "Alt", "F10"]);
}

#[test]
fn an_unparseable_chord_yields_no_caps_rather_than_a_guess() {
    assert!(combo_caps("").is_empty());
    assert!(combo_caps("ctrl+").is_empty());
    assert!(combo_caps("ctrl+nosuchkey").is_empty());
    assert_eq!(combo_display("ctrl+nosuchkey"), "");
}

#[test]
fn display_joins_the_caps_the_way_the_window_reads_them_aloud() {
    assert_eq!(combo_display("ctrl+super+alt+t"), "Ctrl + Win + Alt + T");
    assert_eq!(combo_display("f1"), "F1");
}

/// Exhaustive over the 81-key table: every key must produce a non-empty,
/// ASCII label. ASCII on purpose -- `mark_glyph`'s comment records that the
/// window's faces are text fonts, not symbol fonts, and a missing glyph
/// reads as a rendering bug rather than as a key. That is why the arrow keys
/// are words and not arrows.
#[test]
fn every_key_in_the_table_has_an_ascii_label() {
    for k in key_table() {
        let l = key_label(&k.name);
        assert!(!l.is_empty(), "no label for `{}`", k.name);
        assert!(l.is_ascii(), "label for `{}` is not ASCII: {l}", k.name);
    }
}

/// **The display path must never reach the file.** `Combo::canonical` is the
/// serialiser; if these two ever merge, beckon writes `Win` into a TOML it
/// then cannot parse -- a config the user did not break and cannot obviously
/// fix. Spec §B.4.
#[test]
fn display_never_reaches_the_serialiser() {
    let c = Combo::parse("ctrl+super+alt+t").expect("valid combo");
    assert_eq!(c.canonical(), "ctrl+super+alt+t");
    assert!(c.canonical().contains("super"));
    assert!(!c.canonical().contains("Win"));
}
```

- [ ] **Step 2: Run the tests and verify they fail**

Run: `cargo test -p beckon-core shortcuts:: 2>&1 | tail -20`
Expected: FAIL — `cannot find function 'combo_caps' in this scope` (and the same for `key_label`, `combo_display`).

- [ ] **Step 3: Write the implementation**

Insert into `crates/beckon-core/src/shortcuts.rs` immediately after
`combo_view` (which ends around line 176):

```rust
/// One key's label as it appears on the keyboard, for display only.
///
/// **ASCII, exhaustively.** The settings window's faces are Segoe UI
/// Variable Text and Small -- text fonts, not symbol fonts -- and
/// `mark_glyph` already records what a missing glyph looks like there: a box
/// that reads as a rendering fault rather than as a key. So the punctuation
/// keys take their own ASCII symbol (which any text font has) and the arrow
/// keys take words (because an arrow is not ASCII).
///
/// Never used for serialisation. `Combo::canonical` is that.
pub fn key_label(name: &str) -> String {
    match name {
        "space" => "Space".to_string(),
        "comma" => ",".to_string(),
        "period" => ".".to_string(),
        "slash" => "/".to_string(),
        "minus" => "-".to_string(),
        "equal" => "=".to_string(),
        "semicolon" => ";".to_string(),
        "quote" => "'".to_string(),
        "bracketleft" => "[".to_string(),
        "bracketright" => "]".to_string(),
        "backslash" => "\\".to_string(),
        "grave" => "`".to_string(),
        "tab" => "Tab".to_string(),
        "return" => "Enter".to_string(),
        "escape" => "Esc".to_string(),
        "backspace" => "Backspace".to_string(),
        "delete" => "Del".to_string(),
        "home" => "Home".to_string(),
        "end" => "End".to_string(),
        "pageup" => "PgUp".to_string(),
        "pagedown" => "PgDn".to_string(),
        "up" => "Up".to_string(),
        "down" => "Down".to_string(),
        "left" => "Left".to_string(),
        "right" => "Right".to_string(),
        // Letters, digits and f1-f20: uppercase the whole thing. `t` -> `T`,
        // `f10` -> `F10`, `7` -> `7`.
        other => other.to_uppercase(),
    }
}

/// The chord as the user's keyboard spells it, one label per key.
///
/// `ctrl+super+alt+t` -> `["Ctrl", "Win", "Alt", "T"]`. **`super` is a valid
/// TOML token and a word on no keyboard**, which is the whole reason this
/// function exists.
///
/// Empty when the string does not parse -- the caller shows the raw text
/// instead, the same "show it rather than guess" rule `ComboView::key = None`
/// follows.
///
/// **Display only.** See `display_never_reaches_the_serialiser`.
pub fn combo_caps(s: &str) -> Vec<String> {
    let Ok(c) = Combo::parse(s) else {
        return Vec::new();
    };
    // Fixed order, independent of the order the string listed them in:
    // `Combo::parse` accepts free modifier order, and a display that varied
    // with it would make two identical chords look different.
    let mut v = Vec::with_capacity(5);
    if c.ctrl {
        v.push("Ctrl".to_string());
    }
    if c.super_ {
        v.push("Win".to_string());
    }
    if c.alt {
        v.push("Alt".to_string());
    }
    if c.shift {
        v.push("Shift".to_string());
    }
    v.push(key_label(&c.key.name));
    v
}

/// `combo_caps` joined for a screen reader, for a list cell, and for the
/// ellipsis fallback when the caps do not fit their column (spec §B.3).
///
/// Empty string when the chord does not parse.
pub fn combo_display(s: &str) -> String {
    combo_caps(s).join(" + ")
}
```

- [ ] **Step 4: Run the tests and verify they pass**

Run: `cargo test -p beckon-core shortcuts:: 2>&1 | tail -10`
Expected: PASS, and the pre-existing `shortcuts::` tests still pass.

- [ ] **Step 5: Full core suite plus fmt and clippy**

```bash
cargo test -p beckon-core
cargo fmt --all -- --check
cargo clippy -p beckon-core -- -D warnings
```
Expected: all clean. `key_label`'s `match` will draw a `clippy::match_same_arms` only if two arms collapse — none do.

- [ ] **Step 6: Commit**

```bash
git add crates/beckon-core/src/shortcuts.rs
git commit -m "feat(core): chord labels for display, separate from the serialiser

combo_caps / combo_display render ctrl+super+alt+t as Ctrl Win Alt T.
super is a valid TOML token and a word on no keyboard.

ASCII labels throughout: the window's faces are text fonts, and mark_glyph
already records that a missing glyph there reads as a rendering fault.

Pinned by a test asserting Combo::canonical still emits super, so the
display path cannot become the serialiser and write a config beckon
cannot parse."
```

---

## Task 2: The list cell shows the display string

**Files:**
- Modify: `crates/beckon-windows/src/settings_window.rs:3292` (`fn cells`)

**Interfaces:**
- Consumes: `beckon_core::shortcuts::combo_display` from Task 1.
- Produces: nothing new. `cells` keeps its signature `fn cells(it: &ListItem) -> Vec<String>`.

This is the whole of `super` leaving the interface, with no drawing. Spec §B.5
and §7.2.

- [ ] **Step 1: Read the current function and its doc comment**

Read `crates/beckon-windows/src/settings_window.rs:3280-3320`. The doc comment
above `cells` explains it is "the one funnel" for what a cell says, consulted
by both the rebuild path and the diff path. That property is what makes this a
one-line change and must be preserved.

- [ ] **Step 2: Change the column-1 source**

Replace the body of `cells`:

```rust
fn cells(it: &ListItem) -> Vec<String> {
    vec![app_cell(it), combo_cell(it)]
}

/// The Shortcut column's text: the chord as a keyboard spells it.
///
/// **`ListItem::combo` is unchanged** -- that is the config string, and
/// `Model` writes it back to the file. This is the display of it, and the
/// two must not be conflated: `beckon_core::shortcuts` keeps `combo_display`
/// separate from `Combo::canonical` for exactly this reason (spec §B.4).
///
/// Falls back to the raw string when the chord does not parse, so a row
/// whose stored text is not a valid combo still shows what is actually in
/// the file rather than an empty cell -- `Model::problems` is what says why.
///
/// **Real text, not a placeholder for a later custom draw.** Spec §B.5: the
/// keycaps land *over* text that is really there, which is what keeps
/// `LVM_GETITEMTEXT` working for `examples/settings_probe.rs` and keeps a
/// screen reader announcing what the screen shows.
fn combo_cell(it: &ListItem) -> String {
    let d = beckon_core::shortcuts::combo_display(&it.combo);
    if d.is_empty() {
        it.combo.clone()
    } else {
        d
    }
}
```

- [ ] **Step 3: Type-check and lint**

```bash
cargo check  -p beckon-windows --target aarch64-pc-windows-msvc --all-targets
cargo clippy -p beckon-windows --target aarch64-pc-windows-msvc
cargo fmt --all -- --check
```
Expected: clean. If `beckon_core` is imported under a different alias at the
top of the file, use that alias — check the existing `use` block rather than
adding a second import.

- [ ] **Step 4: Grep for anything that compares a cell to a config string**

```bash
grep -n 'cells(\|app_cell(\|\.combo' crates/beckon-windows/src/settings_window.rs
```
Expected: every hit is either the model side (`it.combo`, `row.combo` — the
config string, correct) or goes through `cells`. **If any code path compares a
cell's text to a combo string to decide something, it breaks here** — the diff
path compares cell-to-cell and is fine; a cell-to-model comparison is not.
Record what you found in the commit message.

- [ ] **Step 5: Commit**

```bash
git add crates/beckon-windows/src/settings_window.rs
git commit -m "feat(windows): the Shortcut column spells the chord, not the TOML

ctrl+super+alt+t reads as Ctrl + Win + Alt + T. super is a word on no
keyboard, and this is the most-read column in the window.

Real LVITEM text, not a placeholder: LVM_GETITEMTEXT keeps working for
settings_probe and a screen reader announces what is on screen.
ListItem::combo is untouched -- that is what Model writes to the file."
```

---

## Task 3: G2 probe — does `ShowWindow` corrupt a populated combo?

**Files:**
- Create: `crates/beckon-windows/examples/showhide_probe.rs`

**Interfaces:**
- Consumes: nothing from other tasks. Standalone binary.
- Produces: console output consumed by a human. No library surface.

Blocks Task 8. `Ui::shown_external` records that `SetWindowPos` on a populated
`CBS_DROPDOWN` makes it re-synchronise its edit to the nearest matching item
and select the whole string. Whether `ShowWindow` does the same is **unknown**.

- [ ] **Step 1: Write the probe**

Create `crates/beckon-windows/examples/showhide_probe.rs`:

```rust
//! G2 (spec `2026-08-12-settings-keycaps-design.md`): does `ShowWindow` on a
//! populated `CBS_DROPDOWN` rewrite its edit text the way `SetWindowPos`
//! does?
//!
//! `Ui::shown_external` in `settings_window.rs` records the `SetWindowPos`
//! half as measured. The empty state in spec §A.2 wants to hide and re-show
//! the App combo instead, and nobody has run that.
//!
//! **Runs a control in the same pass.** `SetWindowPos` is the known-bad call;
//! if the control comes back clean too, the probe is blind and its verdict on
//! `ShowWindow` means nothing. Reported as `CONTROL_CORRUPTED`, which MUST be
//! `True` for the run to be worth reading.
//!
//! Build: `cargo build -p beckon-windows --example showhide_probe --all-targets`
//! Run from **session 1** (an SSH shell is session 0 and has no desktop).

use windows::core::*;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Controls::*;
use windows::Win32::UI::WindowsAndMessaging::*;

/// Typed into the edit. A strict prefix of `PREFIX_OF`, so a combo that
/// re-synchronises has somewhere wrong to go.
const TYPED: &str = "Note";
const PREFIX_OF: &str = "Notepad";

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// `WM_GETTEXT`, never `GetWindowText`. `GetWindowText` returns the
/// kernel-side caption and reads back EMPTY for an EDIT or COMBOBOX -- the
/// trap recorded in CLAUDE.md.
unsafe fn text_of(h: HWND) -> String {
    let mut buf = [0u16; 512];
    let n = SendMessageW(
        h,
        WM_GETTEXT,
        Some(WPARAM(buf.len())),
        Some(LPARAM(buf.as_mut_ptr() as isize)),
    );
    String::from_utf16_lossy(&buf[..n.0.max(0) as usize])
}

unsafe fn make_combo(parent: HWND) -> HWND {
    let c = CreateWindowExW(
        WINDOW_EX_STYLE(0),
        w!("COMBOBOX"),
        w!(""),
        WS_CHILD
            | WS_VISIBLE
            | WS_VSCROLL
            | WS_TABSTOP
            | WINDOW_STYLE((CBS_DROPDOWN | CBS_AUTOHSCROLL | CBS_SORT) as u32),
        10,
        10,
        300,
        200,
        Some(parent),
        None,
        None,
        None,
    )
    .expect("CreateWindowExW COMBOBOX");
    // Same shape the real App combo has: a populated, sorted list where the
    // typed text is a strict prefix of an entry.
    for item in ["Narrator", PREFIX_OF, "Notes & To Do", "Paint"] {
        let t = wide(item);
        SendMessageW(
            c,
            CB_ADDSTRING,
            Some(WPARAM(0)),
            Some(LPARAM(t.as_ptr() as isize)),
        );
    }
    let t = wide(TYPED);
    SendMessageW(
        c,
        WM_SETTEXT,
        Some(WPARAM(0)),
        Some(LPARAM(t.as_ptr() as isize)),
    );
    c
}

unsafe fn pump() {
    let mut msg = MSG::default();
    while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
        let _ = TranslateMessage(&msg);
        DispatchMessageW(&msg);
    }
}

fn main() -> Result<()> {
    unsafe {
        let hinst = GetModuleHandleW(None)?;
        let cls = w!("BeckonShowHideProbe");
        let wc = WNDCLASSW {
            lpfnWndProc: Some(DefWindowProcW),
            hInstance: hinst.into(),
            lpszClassName: cls,
            hbrBackground: HBRUSH((COLOR_WINDOW.0 + 1) as isize as *mut _),
            ..Default::default()
        };
        RegisterClassW(&wc);
        let parent = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            cls,
            w!("showhide_probe"),
            WS_OVERLAPPEDWINDOW,
            100,
            100,
            420,
            160,
            None,
            None,
            Some(hinst.into()),
            None,
        )?;
        let _ = ShowWindow(parent, SW_SHOW);
        pump();

        // -- CONTROL: SetWindowPos, the call already measured as corrupting.
        let c1 = make_combo(parent);
        pump();
        let before_ctl = text_of(c1);
        let _ = SetWindowPos(c1, None, 10, 10, 300, 200, SWP_NOZORDER | SWP_NOACTIVATE);
        pump();
        let after_ctl = text_of(c1);
        let _ = DestroyWindow(c1);

        // -- SUBJECT: hide then show.
        let c2 = make_combo(parent);
        pump();
        let before_sub = text_of(c2);
        let _ = ShowWindow(c2, SW_HIDE);
        pump();
        let _ = ShowWindow(c2, SW_SHOW);
        pump();
        let after_sub = text_of(c2);
        let _ = DestroyWindow(c2);

        println!("TYPED={TYPED}");
        println!("CONTROL_BEFORE={before_ctl}");
        println!("CONTROL_AFTER={after_ctl}");
        println!("CONTROL_CORRUPTED={}", before_ctl != after_ctl);
        println!("SUBJECT_BEFORE={before_sub}");
        println!("SUBJECT_AFTER={after_sub}");
        println!("SUBJECT_CORRUPTED={}", before_sub != after_sub);
        println!();
        if before_ctl == after_ctl {
            println!("VERDICT=BLIND  the control did not reproduce the known-bad");
            println!("               SetWindowPos corruption, so this run says");
            println!("               nothing about ShowWindow. Fix the probe.");
        } else if before_sub == after_sub {
            println!("VERDICT=SAFE   ShowWindow does not corrupt. Task 8 ships as written.");
        } else {
            println!("VERDICT=UNSAFE ShowWindow corrupts too. Task 8 takes its fallback:");
            println!("               hide the GROUP and cover it with a STATIC, leaving");
            println!("               the children mapped underneath.");
        }

        let _ = DestroyWindow(parent);
        Ok(())
    }
}
```

- [ ] **Step 2: Type-check the probe**

```bash
cargo check -p beckon-windows --target aarch64-pc-windows-msvc --all-targets
cargo fmt --all -- --check
```
Expected: clean. Fix any `windows 0.61` signature mismatches here — the crate's
`Option<>`-wrapped parameters change between minor versions, and this is the
cheap place to find out.

- [ ] **Step 3: Commit**

```bash
git add crates/beckon-windows/examples/showhide_probe.rs
git commit -m "test(windows): probe whether ShowWindow corrupts a populated combo (G2)

Ui::shown_external records the SetWindowPos half as measured; the empty
state in spec A.2 wants ShowWindow instead and nobody has run it.

Carries its own control: SetWindowPos is the known-bad call, and a run
where the control comes back clean is blind rather than reassuring."
```

---

## Task 4: G3 probe — does skipping subitem 0 remove the checkbox?

**Files:**
- Create: `crates/beckon-windows/examples/customdraw_probe.rs`

**Interfaces:**
- Consumes: nothing. Standalone binary.
- Produces: console output. Decides §B.6's scope for the 3b plan.

`LVS_EX_CHECKBOXES` rides in column 0's state image
(`settings_window.rs:1953`). Whether `CDRF_SKIPDEFAULT` on subitem 0 takes the
tick with it is not known and must not be guessed — a lost tick is the delete
path, not a cosmetic regression.

- [ ] **Step 1: Write the probe**

Create `crates/beckon-windows/examples/customdraw_probe.rs`:

```rust
//! G3 (spec `2026-08-12-settings-keycaps-design.md` §B.6): does
//! `CDRF_SKIPDEFAULT` on subitem 0 of a `LVS_EX_CHECKBOXES` report-view
//! ListView remove the per-row tick?
//!
//! The tick is a state image in column 0 and it is what makes `Remove` a
//! multi-delete. Losing it is not cosmetic.
//!
//! **Reads pixels, not intentions.** The only honest answer comes from what
//! is on the screen, so the probe screen-captures the state-image rectangle
//! and counts non-background pixels. A drawn tick box is tens of dark
//! pixels; an absent one is zero.
//!
//! **REFUTED during landing 3a: `LVIR_ICON` is NOT the state image's rect.**
//! This brief originally said it was, and the probe shipped that way before
//! review caught it. comctl32 computes the report-view icon rect as
//! `Icon.left = Box.left + state_image_width`, with `Icon.right = Icon.left`
//! unless a small image list is set -- so the rect starts *after* the
//! checkbox, and this probe sets no image list. Either it comes back
//! zero-width (a `BLIND` verdict that burns the hardware trip with no clue
//! why), or it lands on the App label -- and since `CDRF_SKIPDEFAULT` on
//! subitem 0 suppresses the cell's *text* as well as its state image, the
//! subject row would read 0 while the control row read the ink of the word
//! "Claude": a confident **false `TICK_LOST`** closing off §B.6's first
//! branch for no reason.
//!
//! The rect is `[bounds.left, icon.left) x [bounds.top, bounds.bottom)`, from
//! two `LVM_GETITEMRECT` calls. That interval IS the state image by
//! construction, and it degenerates visibly (width 0) if the assumption is
//! ever wrong again. Do not re-add the `LVIR_ICON`-alone claim.
//!
//! **Carries a control:** row 0 is skipped, row 1 is default-drawn. Row 1 MUST
//! come back with ink. If it does not, the capture is broken and the verdict
//! on row 0 means nothing.
//!
//! Build: `cargo build -p beckon-windows --example customdraw_probe --all-targets`
//! Run from **session 1**.

use windows::core::*;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Controls::*;
use windows::Win32::UI::WindowsAndMessaging::*;

const IDC_LIST: i32 = 1;
/// Row 0 is the subject (subitem 0 skipped); row 1 is the control.
const SUBJECT_ROW: i32 = 0;
const CONTROL_ROW: i32 = 1;

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if msg == WM_NOTIFY {
        let nm = &*(lp.0 as *const NMHDR);
        if nm.idFrom == IDC_LIST as usize && nm.code == NM_CUSTOMDRAW {
            let cd = &*(lp.0 as *const NMLVCUSTOMDRAW);
            let stage = cd.nmcd.dwDrawStage;
            if stage == CDDS_PREPAINT {
                return LRESULT(CDRF_NOTIFYITEMDRAW as isize);
            }
            if stage == CDDS_ITEMPREPAINT {
                return LRESULT(CDRF_NOTIFYSUBITEMDRAW as isize);
            }
            if stage == CDDS_ITEMPREPAINT | CDDS_SUBITEM {
                // Skip subitem 0 on the SUBJECT row only. Everything else
                // draws normally, which is what makes the control a control.
                if cd.nmcd.dwItemSpec as i32 == SUBJECT_ROW as isize as i32 && cd.iSubItem == 0 {
                    return LRESULT(CDRF_SKIPDEFAULT as isize);
                }
                return LRESULT(CDRF_DODEFAULT as isize);
            }
        }
    }
    DefWindowProcW(hwnd, msg, wp, lp)
}

/// Count pixels in `rc` (screen coords) that are not the window background.
unsafe fn ink_in(rc: RECT) -> u32 {
    let w = rc.right - rc.left;
    let h = rc.bottom - rc.top;
    if w <= 0 || h <= 0 {
        return 0;
    }
    let screen = GetDC(None);
    let mem = CreateCompatibleDC(Some(screen));
    let bmp = CreateCompatibleBitmap(screen, w, h);
    let old = SelectObject(mem, bmp.into());
    let _ = BitBlt(mem, 0, 0, w, h, Some(screen), rc.left, rc.top, SRCCOPY);
    let bg = GetSysColor(COLOR_WINDOW) & 0x00FF_FFFF;
    let mut ink = 0u32;
    for y in 0..h {
        for x in 0..w {
            let px = GetPixel(mem, x, y).0 & 0x00FF_FFFF;
            if px != bg {
                ink += 1;
            }
        }
    }
    SelectObject(mem, old);
    let _ = DeleteObject(bmp.into());
    let _ = DeleteDC(mem);
    ReleaseDC(None, screen);
    ink
}

/// The state-image rect for `row`, in SCREEN coordinates.
unsafe fn state_rect(list: HWND, row: i32) -> RECT {
    let mut rc = RECT {
        left: LVIR_ICON.0 as i32,
        top: 0,
        right: 0,
        bottom: 0,
    };
    SendMessageW(
        list,
        LVM_GETITEMRECT,
        Some(WPARAM(row as usize)),
        Some(LPARAM(&mut rc as *mut RECT as isize)),
    );
    let mut pts = [
        POINT {
            x: rc.left,
            y: rc.top,
        },
        POINT {
            x: rc.right,
            y: rc.bottom,
        },
    ];
    MapWindowPoints(Some(list), None, &mut pts);
    RECT {
        left: pts[0].x,
        top: pts[0].y,
        right: pts[1].x,
        bottom: pts[1].y,
    }
}

unsafe fn pump_for(ms: u32) {
    let end = GetTickCount64() + ms as u64;
    let mut msg = MSG::default();
    while GetTickCount64() < end {
        while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

fn main() -> Result<()> {
    unsafe {
        let icc = INITCOMMONCONTROLSEX {
            dwSize: std::mem::size_of::<INITCOMMONCONTROLSEX>() as u32,
            dwICC: ICC_LISTVIEW_CLASSES,
        };
        let _ = InitCommonControlsEx(&icc);

        let hinst = GetModuleHandleW(None)?;
        let cls = w!("BeckonCustomDrawProbe");
        let wc = WNDCLASSW {
            lpfnWndProc: Some(wndproc),
            hInstance: hinst.into(),
            lpszClassName: cls,
            hbrBackground: HBRUSH((COLOR_WINDOW.0 + 1) as isize as *mut _),
            ..Default::default()
        };
        RegisterClassW(&wc);
        let parent = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            cls,
            w!("customdraw_probe"),
            WS_OVERLAPPEDWINDOW,
            100,
            100,
            520,
            260,
            None,
            None,
            Some(hinst.into()),
            None,
        )?;

        let list = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            w!("SysListView32"),
            w!(""),
            WS_CHILD | WS_VISIBLE | WS_BORDER | WINDOW_STYLE(LVS_REPORT as u32),
            10,
            10,
            480,
            180,
            Some(parent),
            Some(HMENU(IDC_LIST as *mut _)),
            None,
            None,
        )?;
        SendMessageW(
            list,
            LVM_SETEXTENDEDLISTVIEWSTYLE,
            Some(WPARAM(0)),
            Some(LPARAM(
                (LVS_EX_FULLROWSELECT | LVS_EX_DOUBLEBUFFER | LVS_EX_CHECKBOXES) as isize,
            )),
        );

        for (i, title) in ["App", "Shortcut"].iter().enumerate() {
            let t = wide(title);
            let col = LVCOLUMNW {
                mask: LVCF_TEXT | LVCF_WIDTH,
                cx: 230,
                pszText: PWSTR(t.as_ptr() as *mut u16),
                ..Default::default()
            };
            SendMessageW(
                list,
                LVM_INSERTCOLUMNW,
                Some(WPARAM(i)),
                Some(LPARAM(&col as *const LVCOLUMNW as isize)),
            );
        }
        for (i, app) in ["Windows Terminal", "Claude"].iter().enumerate() {
            let t = wide(app);
            let it = LVITEMW {
                mask: LVIF_TEXT,
                iItem: i as i32,
                pszText: PWSTR(t.as_ptr() as *mut u16),
                ..Default::default()
            };
            SendMessageW(
                list,
                LVM_INSERTITEMW,
                Some(WPARAM(0)),
                Some(LPARAM(&it as *const LVITEMW as isize)),
            );
        }

        let _ = ShowWindow(parent, SW_SHOW);
        let _ = UpdateWindow(parent);
        pump_for(600);

        let subject = ink_in(state_rect(list, SUBJECT_ROW));
        let control = ink_in(state_rect(list, CONTROL_ROW));

        println!("SUBJECT_ROW={SUBJECT_ROW} (subitem 0 CDRF_SKIPDEFAULT)");
        println!("CONTROL_ROW={CONTROL_ROW} (default drawn)");
        println!("SUBJECT_INK={subject}");
        println!("CONTROL_INK={control}");
        println!();
        if control == 0 {
            println!("VERDICT=BLIND    the control row shows no tick either, so the");
            println!("                 capture is broken. Fix the probe before reading");
            println!("                 anything into SUBJECT_INK.");
        } else if subject == 0 {
            println!("VERDICT=TICK_LOST  subitem 0 stays default-drawn. Spec B.6 takes");
            println!("                   its second branch: app_cell keeps appending the");
            println!("                   flag in Body, and the IOU stays open.");
        } else {
            println!("VERDICT=TICK_SURVIVES  subitem 0 joins the custom-draw pass in 3b:");
            println!("                       app name in Body, flag in Caption, and the");
            println!("                       app_cell IOU closes.");
        }

        let _ = DestroyWindow(parent);
        Ok(())
    }
}
```

- [ ] **Step 2: Type-check the probe**

```bash
cargo check -p beckon-windows --target aarch64-pc-windows-msvc --all-targets
cargo clippy -p beckon-windows --target aarch64-pc-windows-msvc
cargo fmt --all -- --check
```
Expected: clean. The `windows 0.61` bindings for `MapWindowPoints`,
`LVM_GETITEMRECT` and the `HMENU` control id are the likely friction points;
resolve them here rather than on hardware.

- [ ] **Step 3: Commit**

```bash
git add crates/beckon-windows/examples/customdraw_probe.rs
git commit -m "test(windows): probe whether skipping subitem 0 drops the tick (G3)

LVS_EX_CHECKBOXES rides in column 0's state image, and the tick is what
makes Remove a multi-delete -- so whether CDRF_SKIPDEFAULT takes it away
decides spec B.6's scope and must not be guessed.

Reads pixels in the state-image rect, with row 1 default-drawn as the
control: a run where the control shows no ink is blind, not clean."
```

---

## Task 5: The editor group box

**Files:**
- Modify: `crates/beckon-windows/src/settings_window.rs` — id block (~line 298), `mod cap` (~line 380), `role_of`, `build_children` (~line 1985), `apply_state`

**Interfaces:**
- Consumes: nothing from earlier tasks.
- Produces: `const IDC_GRP_EDITOR: i32 = 1034;` — Task 6 places it, Task 8 hides its children.

- [ ] **Step 1: Add the id**

After `const IDC_RESET: i32 = 1033;` in `crates/beckon-windows/src/settings_window.rs`:

```rust
/// The editor group box. Its caption says which row is being edited, so the
/// two lines inside it read as one thing rather than as seven controls that
/// happen to share a band.
///
/// 1034 because 1033 is the current maximum and 1001-1007 are pinned by
/// `examples/settings_probe.rs`. A group box is not operable, so it carries
/// no mnemonic and no entry in `mod cap`'s collision table.
const IDC_GRP_EDITOR: i32 = 1034;
```

- [ ] **Step 2: Add the three captions**

In `mod cap`, beside the existing constants:

```rust
    /// The editor group's caption in its three states. `EDITOR_ROW` takes
    /// the app name; `EDITOR_UNNAMED` is the same row before one is picked.
    ///
    /// **No `&` on any of the three.** A group box caption's mnemonic moves
    /// focus to the next control in tab order, which is the same reason the
    /// `App` and `Shortcut` labels carry none -- and the collision table
    /// above has no room to spare.
    pub const EDITOR_NONE: &str = "No shortcut selected";
    pub const EDITOR_NEW: &str = "New shortcut";
    pub const EDITOR_UNNAMED: &str = "Editing this shortcut";
```

The `Editing "<app>"` form is built at the call site with `format!`, not stored
here, because it is not a constant.

- [ ] **Step 3: Confirm the type role — and do NOT edit `role_of`**

`role_of` (`settings_window.rs:647`) ends in `_ => Role::Body`, and its comment
says so explicitly: "every BUTTON (push, check, and the group box) … and
anything added later that does not say otherwise."

So `IDC_GRP_EDITOR` already takes `Role::Body`, which is what
`IDC_GRP_KEYBOARD` takes, which is what makes the two group captions in this
window the same size. **Adding an arm for it would be a second opinion in the
one mapping that exists to prevent one.** Read the function, confirm the
catch-all, add nothing.

- [ ] **Step 4: Create the control**

In `build_children`, immediately **before** the `IDC_LBL_APP` STATIC (~line
1987), so the group is created first and therefore sits behind its children in
z-order:

```rust
    // -- Band 4: the editor group. The strip's two lines live inside it and
    // its caption names the row, so seven controls read as one thing (spec
    // A.1).
    //
    // Created BEFORE its children: a group box is a BUTTON that paints a
    // frame, and creation order is z-order, so a group created afterwards
    // paints over the controls it is supposed to surround.
    //
    // Not a tab stop, and deliberately no BS_NOTIFY: it is not operable, so
    // it must not join PUSH_BUTTONS and must never take the default ring.
    child(
        hwnd,
        w!("BUTTON"),
        cap::EDITOR_NONE,
        WINDOW_STYLE(BS_GROUPBOX as u32),
        IDC_GRP_EDITOR,
        &fonts,
    );
```

- [ ] **Step 5: Write the caption on every push**

In `apply_state`, beside the other text writes, add:

```rust
    // The group's caption, and it is a TEXT write, not a geometry one: it
    // must never reach `layout`, because `layout` means `SetWindowPos` on the
    // populated App combo -- the measured data-loss call (`Ui::shown_external`).
    // A group box caption is never measured by `layout`, so there is no
    // second path back in.
    let editor_caption = match &st.detail {
        None => cap::EDITOR_NONE.to_string(),
        Some(d) if d.app.trim().is_empty() => cap::EDITOR_UNNAMED.to_string(),
        Some(d) => format!("Editing \"{}\"", d.app.trim()),
    };
    set_text_if_changed(hwnd, IDC_GRP_EDITOR, &editor_caption);
```

`set_text_if_changed(parent: HWND, id: i32, s: &str)` already exists at
`settings_window.rs:4289` — that exact signature. Use it; do not add a second
guarded-write helper, and do not write unconditionally. An unconditional
`WM_SETTEXT` on every push is the mistake `ControlState::filter`'s doc comment
records for the filter box, and the argument carries to any repeated write.

`cap::EDITOR_NEW` is used when `Add` has just run and the row is empty; if
`Detail` carries no "is new" signal, leave `EDITOR_NEW` unused for now and
delete it rather than shipping a constant nothing reads.

- [ ] **Step 6: Type-check, lint, format**

```bash
cargo check  -p beckon-windows --target aarch64-pc-windows-msvc --all-targets
cargo clippy -p beckon-windows --target aarch64-pc-windows-msvc
cargo fmt --all -- --check
```
Expected: clean. The group box will be at position (0,0) size (0,0) until Task
6 places it — that is correct at this step and is why Tasks 5 and 6 are not one
commit.

- [ ] **Step 7: Commit**

```bash
git add crates/beckon-windows/src/settings_window.rs
git commit -m "feat(windows): add the editor group box, unplaced

IDC_GRP_EDITOR 1034, created before its children so z-order puts the
frame behind them. Caption written per push and names the row being
edited; that is a SetWindowTextW, never a layout, because layout reaches
SetWindowPos on the populated App combo.

Placed in the next commit."
```

---

## Task 6: `layout` band 4 becomes two lines

**Files:**
- Modify: `crates/beckon-windows/src/settings_window.rs:~2780-2880` (band 4 in `layout`)

**Interfaces:**
- Consumes: `IDC_GRP_EDITOR` from Task 5.
- Produces: nothing new. Task 7 deletes the tokens this stops using; Task 9 resizes the notes it places.

- [ ] **Step 1: Read the whole of band 4 first**

Read `crates/beckon-windows/src/settings_window.rs:2780-2880`. Two comments in
it are load-bearing and must survive the rewrite, moved rather than deleted:

- the `place_h(ui.app, …, field_h * 9)` comment explaining that a COMBOBOX's
  `cy` is the **dropped-down** height, capped by `CB_SETMINVISIBLE(8)`;
- the `edit_dy` / `edit_h` comment explaining that the chips and the key list
  share the fields' midline so App, key and filter are one box repeated.

The comment about `app_w` clamping to zero at ~613 px is the one this task
makes **false**. Delete it and put the new arithmetic in its place (Step 3).

- [ ] **Step 2: Replace band 4**

Replace from `// -- Band 4: the editor strip, one line, then the notes beneath
it.` down to and including `place_h(ui.notes, cx, y, cw, clamp(kb_y - band - y));`:

```rust
    // -- Band 4: the editor group. TWO lines inside a titled BS_GROUPBOX,
    // then the notes on a third line inside the same group.
    //
    //   +- Editing "Windows Terminal" ----------------------------------+
    //   |  App       [ ..................................... v ]        |
    //   |  Shortcut  [ ]Ctrl [ ]Win [ ]Alt [ ]Shift [ key v ]  [R] [R]  |
    //   |  ok  Registered. Press Ctrl + Win + Alt + T to focus it.      |
    //   +---------------------------------------------------------------+
    //
    // **App gets a line of its own, and that is the whole point.** On one
    // line it was the control that absorbed whatever the other six left --
    // about 209 px at 860, and ~59 px at MIN_WIDTH. Two derived tokens
    // (`tok::KEY_COL`, `tok::BTN_SM`) existed only to keep that figure above
    // zero, and Task 7 retires both.
    // Bound once: `y` is advanced past the group at the end of this band, so
    // anything placed relative to the group's TOP must read this and not `y`.
    // Task 8's empty-state STATIC is the second reader.
    let grp_y = y;
    let grp_x = cx;
    let grp_w = cw;
    // Caption inset, then the content, then a bottom inset the size of the
    // gap -- the same shape band 6's `kb_h` uses, so the two groups in this
    // window are one rule.
    let ins_x = grp_x + gap;
    let ins_w = clamp(grp_w - gap * 2);
    let notes_h = notes_height(hwnd, &ui, dpi);
    let grp_h = s(24) + ctl + gap + ctl + gap + notes_h + gap;
    place(IDC_GRP_EDITOR, grp_x, grp_y, grp_w, grp_h);

    // Both lines share one label column, so `App` and `Shortcut` left-align
    // with each other instead of each starting wherever its own line does.
    let lw_lbl = tw("Shortcut").max(tw("App")) + s(4);
    let fld_x = ins_x + lw_lbl + lblgap;
    let fld_w = clamp(ins_x + ins_w - fld_x);

    // Line 1: App, full width.
    let mut ly = grp_y + s(24);
    place(IDC_LBL_APP, ins_x, ly, lw_lbl, ctl);
    // A COMBOBOX's `cy` is the height of its DROPPED-DOWN list, not of the
    // closed control -- and under comctl32 v6 even that is capped by
    // `build_children`'s CB_SETMINVISIBLE(8). The closed height is the
    // system's to choose from the font, which is why `combo_h` above asks
    // what it took rather than guessing a chrome delta the next font change
    // would invalidate.
    place_h(ui.app, fld_x, ly + edit_dy, fld_w, field_h * 9);
    ly += ctl + gap;

    // Line 2: the shortcut. Chips left, then the key list, then the two
    // commands right-aligned -- the same "commands close the line" rule band
    // 2's Add/Remove follow.
    place(IDC_LBL_SHORTCUT, ins_x, ly, lw_lbl, ctl);
    let bw_record = btn(cap::RECORD);
    let bw_reset = btn(cap::RESET);
    let res_x = ins_x + clamp(ins_w - bw_reset);
    let rec_x = ins_x + clamp(ins_w - bw_reset - gap - bw_record);
    // Each chip is its caption plus the check box's own square, exactly as
    // band 6's `Hold` chips are sized -- same `glyph`, one rule.
    let w_mod_ctrl = tw(cap::MOD_CTRL) + glyph;
    let w_mod_win = tw(cap::MOD_WIN) + glyph;
    let w_mod_alt = tw(cap::MOD_ALT) + glyph;
    let w_mod_shift = tw(cap::MOD_SHIFT) + glyph;
    // Chips and key list share the fields' midline (`edit_dy`) and their
    // height, so App, the key list and the filter are ONE box repeated
    // rather than three boxes that happen to be concentric. A check box
    // centres its glyph and caption inside whatever rect it is given, so
    // `edit_h` needs no separate rule for the four of them.
    let mut mx = fld_x;
    place(IDC_MOD_CTRL, mx, ly + edit_dy, w_mod_ctrl, edit_h);
    mx += w_mod_ctrl + gap;
    place(IDC_MOD_WIN, mx, ly + edit_dy, w_mod_win, edit_h);
    mx += w_mod_win + gap;
    place(IDC_MOD_ALT, mx, ly + edit_dy, w_mod_alt, edit_h);
    mx += w_mod_alt + gap;
    place(IDC_MOD_SHIFT, mx, ly + edit_dy, w_mod_shift, edit_h);
    mx += w_mod_shift + gap;
    // The key list takes what is between the chips and the commands, under
    // the shortcut column's ceiling. It no longer needs a token of its own:
    // with App on line 1 there is nothing left on this line for it to starve.
    let key_w = s(tok::SHORTCUT_COL).min(clamp(rec_x - gap - mx));
    // `cy` is the DROPPED-DOWN height here too, capped by the same
    // CB_SETMINVISIBLE(8) the App combo carries.
    place_h(ui.combo, mx, ly + edit_dy, key_w, field_h * 9);
    // Buttons honour `cy` and look right at the band height, so they take
    // `ctl` directly and sit on the band line rather than on the fields'
    // midline -- the same rule the command bar's three follow.
    place(IDC_RECORD, rec_x, ly, bw_record, ctl);
    place(IDC_RESET, res_x, ly, bw_reset, ctl);
    ly += ctl + gap;

    // Line 3: the notes, inside the group and beside what they describe.
    // Fixed height -- see `notes_height`. It used to take every pixel down to
    // the keyboard group, which measured as a 1220x177 control holding one
    // 258 px line.
    place_h(ui.notes, ins_x, ly, ins_w, notes_h);

    y += grp_h + band;
    // `grp_h` is fixed rather than clamped, so an intermediate resize below
    // MIN_HEIGHT -- reachable through a `WM_DPICHANGED` suggested rect, which
    // never asks `WM_GETMINMAXINFO` -- can still land `y` past `kb_y`. Same
    // guard the strip carried before, for the same reason.
    y = y.min(kb_y);
```

- [ ] **Step 3: Write the new floor arithmetic into the comment**

Compute the fixed part of line 2 at 96 DPI from the tokens as they will be
after Task 7, and write it into the band-4 comment where the old ~613 px
paragraph was. The inputs: `lw_lbl + lblgap` + four chips (each `tw(caption) +
glyph` where `glyph = s(24)`) + three `gap` + `gap` + `bw_record` + `gap` +
`bw_reset`, all at `tok::BTN = 88`. State what `key_w` clamps to zero at, and
compare it to `MIN_WIDTH` 720 the way the old comment did. **Do not copy a
number from this plan** — derive it from the tokens in the tree at the time.

- [ ] **Step 4: Type-check, lint, format**

```bash
cargo check  -p beckon-windows --target aarch64-pc-windows-msvc --all-targets
cargo clippy -p beckon-windows --target aarch64-pc-windows-msvc
cargo fmt --all -- --check
```
Expected: one error, `cannot find function 'notes_height'` — Task 9 writes it.
To keep this task independently checkable, add the stub **in this task** and
let Task 9 give it its real body:

```rust
/// Height of the notes line inside the editor group. Task 9 replaces this
/// with the two-line cap; the strip's old behaviour is one line.
unsafe fn notes_height(hwnd: HWND, ui: &LayoutHandles, dpi: u32) -> i32 {
    let _ = (hwnd, ui);
    32 * dpi as i32 / 96
}
```

Re-run the three commands. Expected: clean.

- [ ] **Step 5: Re-derive `MIN_HEIGHT`**

Spec §A.5: `MIN_HEIGHT` must be re-derived, **not nudged**. The group is one
`ctl + gap` taller than the strip it replaces, and `layout` already makes the
list yield its height to what is below it — so a floor that is too low does not
overlap, it starves the list to zero rows.

`MIN_HEIGHT` is `settings_window.rs:626`, consumed once at line 3742 by
`WM_GETMINMAXINFO` as `mm.ptMinTrackSize.y = scale(MIN_HEIGHT, dpi)`.

Derive it as the smallest client height at which the list still shows a usable
number of rows, from the tokens in the tree:

```
pad + head(ctl) + gap + list(header + N*row + border)
    + band + grp_h + band + kb_h + band + ctl + pad
```

**Use `N = 4`** — half of `tok::ROWS`. The floor is about what is still
*usable*, not what is still non-negative: a window whose list shows one row is
not a smaller version of this window, it is a broken one. Four rows is enough
to see a selection with context above and below it, and it puts the floor
roughly one list-half below the default rather than at a cliff.

Write the derivation into the comment beside the constant the way band 2 and
band 4 write theirs, and re-read the doc comment above `MIN_WIDTH` (line 624,
"below the point where `layout` starts overlapping controls"). After this
change the number no longer means "where controls start overlapping" — every
subtraction in `layout` is clamped and the list yields first — it means "where
the list stops being worth showing". Say that instead.

Then re-run the three commands from Step 4.

- [ ] **Step 6: Commit**

```bash
git add crates/beckon-windows/src/settings_window.rs
git commit -m "feat(windows): the editor is two lines in a group, not seven on one

App takes a line of its own, so it stops being the control that absorbs
whatever the other six leave -- 209 px at 860, 59 px at MIN_WIDTH before
this. Shortcut, chips, key list and the two commands share line 2; the
notes move onto line 3 inside the same group.

Placement only. The tokens that existed to keep the App combo above zero
come out in the next commit, once this arithmetic is on hardware."
```

---

## Task 7: Retire `tok::KEY_COL` and `tok::BTN_SM`

**Files:**
- Modify: `crates/beckon-windows/src/settings_window.rs` — `mod tok` (~line 540-570)

**Interfaces:**
- Consumes: Task 6's band 4, which no longer reads either token.
- Produces: nothing.

Do this as its own commit so that if the hardware pass says line 2 is too tight
at `MIN_WIDTH`, the revert is one commit and does not take the recompose with
it.

- [ ] **Step 1: Confirm nothing reads them**

```bash
grep -n 'KEY_COL\|BTN_SM' crates/beckon-windows/src/settings_window.rs
```
Expected: only the two definitions and their doc comments. **If `btn_sm` (the
closure) still exists in `layout`, Task 6 is incomplete** — go back and finish
it before deleting anything.

- [ ] **Step 2: Delete both constants and their doc comments**

Remove `pub const BTN_SM: i32 = 64;` and `pub const KEY_COL: i32 = 140;` from
`mod tok`, along with the multi-paragraph doc comments that derive them. Those
comments are an argument for a constraint that no longer exists; keeping them
would leave the next reader deriving against a line that is gone.

- [ ] **Step 3: Record the retirement where the argument lived**

`tok::SHORTCUT_COL`'s doc comment currently ends by contrasting itself with
`KEY_COL`. Replace that contrast with:

```rust
    /// The right-aligned `Shortcut` column, the editor field under it, and
    /// the key list's ceiling.
    ///
    /// The key list used to have a token of its own (`KEY_COL`, 140),
    /// derived rather than designed: band 4 was one line, the App combo
    /// absorbed whatever the other six controls left, and 60 px had to come
    /// from somewhere to pay for `Record` and `Reset` sharing that line.
    /// With App on a line of its own there is nothing left to starve, so the
    /// key list is back under this ceiling and the arithmetic is retired.
```

- [ ] **Step 4: Type-check, lint, format**

```bash
cargo check  -p beckon-windows --target aarch64-pc-windows-msvc --all-targets
cargo clippy -p beckon-windows --target aarch64-pc-windows-msvc
cargo fmt --all -- --check
```
Expected: clean.

- [ ] **Step 5: Commit**

```bash
git add crates/beckon-windows/src/settings_window.rs
git commit -m "refactor(windows): retire KEY_COL and BTN_SM

Both were derived, not designed: they existed only to keep the App combo
above zero width inside MIN_WIDTH while band 4 was one line. It is two
lines now, so the key list returns to SHORTCUT_COL and Record/Reset take
tok::BTN like every other button -- the window stops having two button
sizes.

Its own commit so a revert does not take the recompose with it."
```

---

## Task 8: The empty state replaces the grey-out

**Files:**
- Modify: `crates/beckon-windows/src/settings_window.rs` — id block, `mod cap`, `build_children`, `layout`, `apply_state` (~line 1124 is the `EnableWindow` helper)

**Interfaces:**
- Consumes: `IDC_GRP_EDITOR` (Task 5), band 4's `ins_x` / `ins_w` (Task 6).
- Produces: `const IDC_EMPTY: i32 = 1035;`

> **GATED ON G2.** Do not start until `showhide_probe` has run on a14 and
> printed `VERDICT=SAFE`. On `VERDICT=UNSAFE`, Step 3 changes — see the note in
> that step. On `VERDICT=BLIND`, fix the probe and re-run; do not proceed.

- [ ] **Step 1: Add the id and the caption**

```rust
/// The one line shown where the editor's controls are when no row is
/// selected. It replaces greying them: a disabled `CBS_DROPDOWNLIST` keeps a
/// white field and dark text (measurements 56), so the strip used to look
/// half-alive beside greyed labels -- and that is the theme's behaviour, not
/// a bug, which is why it cannot be fixed by disabling more carefully.
const IDC_EMPTY: i32 = 1035;
```

In `mod cap`:

```rust
    /// **No `&`.** A STATIC's mnemonic moves focus to the next control in tab
    /// order, and the collision table above has no letter spare. `Add` is one
    /// Tab away and already carries `A`.
    pub const EMPTY_HINT: &str = "Select a shortcut above, or press Add to make a new one.";
```

**Do not touch `role_of`** — its `_ => Role::Body` catch-all already covers a
new STATIC, and Task 5 Step 3 explains why adding an arm is worse than adding
nothing. (`IDC_NOTES => Role::Caption` is the one STATIC that opts out; this
hint is Body, like the labels it sits among.)

- [ ] **Step 2: Create the STATIC**

Win32 has no `SS_CENTER` constant in this file yet; add it beside the others
near line 122:

```rust
const SS_CENTER_STYLE: WINDOW_STYLE = WINDOW_STYLE(0x0001);
```

In `build_children`, immediately after the notes STATIC:

```rust
    // Shown only when nothing is selected, in place of the group's controls.
    // `SS_CENTERIMAGE` centres it vertically inside whatever rect `layout`
    // gives it; `SS_CENTER` centres the text horizontally. `SS_NOPREFIX` for
    // the same reason the notes carry it -- an `&` in an app name is not a
    // mnemonic.
    child(
        hwnd,
        w!("STATIC"),
        cap::EMPTY_HINT,
        SS_CENTER_STYLE | SS_CENTERIMAGE_STYLE | SS_NOPREFIX_STYLE,
        IDC_EMPTY,
        &fonts,
    );
```

- [ ] **Step 3: Place it over the group's content area**

In `layout`, after the notes placement in band 4:

```rust
    // Covers exactly the group's content -- both field lines and the notes --
    // so the swap does not move the group's frame or anything below it.
    place(
        IDC_EMPTY,
        ins_x,
        grp_y + s(24),
        ins_w,
        clamp(grp_h - s(24) - gap),
    );
```

**`grp_y`, never `y`.** Task 6 binds `grp_y` at the top of band 4 precisely
because `y` is advanced past the group (`y += grp_h + band`) before the band
ends. If this placement is inserted after that line and reads `y`, the hint
lands on the keyboard group instead — and it only shows when nothing is
selected, so a hasty check with a row selected will not see it.

> **If G2 came back `UNSAFE`:** do not hide the children. Place `IDC_EMPTY`
> exactly as above but leave the controls mapped underneath it and rely on
> z-order plus `SWP_SHOWWINDOW`/`SWP_HIDEWINDOW` **on the STATIC alone**. The
> children stay visible-but-covered, which costs one `SetWindowPos` on the
> group and none on the App combo. Step 4's `ShowWindow` loop then applies to
> `IDC_EMPTY` only.

- [ ] **Step 4: Swap on every push**

First add the helper beside `enable` (`settings_window.rs:1121`), matching its
shape exactly — same parameter names, same "a control that is missing is not an
error" tolerance:

```rust
/// Show or hide one control. The sibling of `enable`, and the reason the
/// editor's empty state is a swap rather than a grey-out: a disabled
/// `CBS_DROPDOWNLIST` keeps a white field and dark text (measurements 56), so
/// disabling the strip made it look half-alive rather than inactive.
fn show(parent: HWND, id: i32, on: bool) {
    if let Ok(h) = unsafe { GetDlgItem(Some(parent), id) } {
        unsafe {
            let _ = ShowWindow(h, if on { SW_SHOW } else { SW_HIDE });
        }
    }
}
```

The site is `apply_state`'s `match &st.detail` — the `None` arm begins around
`settings_window.rs:3097` with

```rust
            None => {
                for id in SHORTCUT_CONTROLS {
                    enable(hwnd, id, false);
                }
                enable(hwnd, IDC_APP, false);
```

Replace the **disabling in that arm only** with hiding, driven off the same
`SHORTCUT_CONTROLS` const the arm already uses rather than a second hand-written
list:

```rust
    // Hide, do not disable, when there is no row. See `IDC_EMPTY`.
    //
    // Order matters: show the hint BEFORE hiding the fields.
    // `ShowWindow(SW_HIDE)` on a focused control moves focus off it, the same
    // way `EnableWindow(FALSE)` does (see the note at the `enabled` helper),
    // and Windows needs somewhere inside this window to put it.
    let editing = st.detail.is_some();
    show(hwnd, IDC_EMPTY, !editing);
    show(hwnd, IDC_LBL_APP, editing);
    show(hwnd, IDC_APP, editing);
    show(hwnd, IDC_LBL_SHORTCUT, editing);
    show(hwnd, IDC_NOTES, editing);
    for id in SHORTCUT_CONTROLS {
        show(hwnd, id, editing);
    }
```

**`st.editable` disabling stays exactly as it is.** A read-only config
(`editable == false`) still has a selected row, so its controls stay *visible*
and stay greyed — that is a different state with a different meaning, and
§A.2 only claims the no-selection case. The disabled-dropdown-looks-live
complaint survives there; say so in the commit message rather than quietly
widening the change.

**Leave every other `enable` call alone** — `IDC_APPLY`, `IDC_REMOVE`,
`IDC_ADD`, `IDC_FILTER`, `IDC_LIST`, `IDC_CAPS`, and the capture-time
disabling at lines 4346/4350/4383. §C.3's rule that an armed capture disables
the chips still holds, and it holds on *visible* chips.

- [ ] **Step 4b: Fix the comment that now describes the old behaviour**

`settings_window.rs:4105-4106` reads "`apply_state`'s `None` arm calls
`enable(hwnd, IDC_COMBO, false)` / `enable(hwnd, IDC_APP, false)` whenever
`st.detail` is `None`". After Step 4 that is false. Read the whole comment —
it is reasoning about focus, which hiding also moves — and rewrite it for what
the arm does now rather than deleting it.

- [ ] **Step 5: Type-check, lint, format**

```bash
cargo check  -p beckon-windows --target aarch64-pc-windows-msvc --all-targets
cargo clippy -p beckon-windows --target aarch64-pc-windows-msvc
cargo fmt --all -- --check
```
Expected: clean.

- [ ] **Step 6: Commit**

```bash
git add crates/beckon-windows/src/settings_window.rs
git commit -m "feat(windows): empty state instead of greying the editor

A disabled CBS_DROPDOWNLIST keeps a white field and dark text, so the
strip looked half-alive beside greyed labels. That is the theme, not a
bug, so it is fixed by not disabling: the controls hide and one line
takes their place.

Gated on the showhide probe, which measured that ShowWindow does not
corrupt a populated CBS_DROPDOWN the way SetWindowPos does."
```

---

## Task 9: The notes get a fixed two-line cap

**Files:**
- Modify: `crates/beckon-windows/src/settings_window.rs` — `notes_height` (the stub from Task 6), and wherever the notes text is composed

**Interfaces:**
- Consumes: the `notes_height` stub from Task 6.
- Produces: `unsafe fn notes_height(hwnd: HWND, ui: &LayoutHandles, dpi: u32) -> i32` with its real body.

- [ ] **Step 1: Give `notes_height` its body**

```rust
/// Height of the notes line inside the editor group: exactly two lines of the
/// notes' own face, whatever the DPI and whatever the face.
///
/// **Fixed, not flexing.** It used to take every pixel between the strip and
/// the keyboard group, which measured on a14 as a 1220x177 control holding a
/// single 258 px line -- a large blank band whose only job was to exist.
///
/// Two is a guess and is worth revisiting: nobody has looked at what three
/// notes at once reads like, which is exactly the gap the followups record.
/// It is a cheap guess to change and an expensive band to leave empty.
unsafe fn notes_height(hwnd: HWND, ui: &LayoutHandles, dpi: u32) -> i32 {
    let line = text_size(hwnd, ui.fonts.get(Role::Caption), dpi, "Ag").1;
    line * 2 + (4 * dpi as i32 / 96)
}
```

- [ ] **Step 2: Cap the composed text at two lines**

The site is `apply_state`, inside the `Some(d)` arm's `match &cap_notes`, at
`settings_window.rs:3087-3094`:

```rust
                    None => {
                        let body: Vec<String> = d
                            .notes
                            .iter()
                            .map(|n| format!("{}  {}", mark_glyph(n.mark), n.text))
                            .collect();
                        set_text(notes, &body.join("\r\n"));
                    }
```

Replace with:

```rust
                    None => {
                        // Two lines, because that is what `notes_height`
                        // reserves. A third would draw outside the rect and be
                        // clipped, which reads as a rendering fault rather
                        // than as "there is more".
                        const NOTE_LINES: usize = 2;
                        let body: Vec<String> = d
                            .notes
                            .iter()
                            .take(NOTE_LINES)
                            .map(|n| format!("{}  {}", mark_glyph(n.mark), n.text))
                            .collect();
                        let mut text = body.join("\r\n");
                        if d.notes.len() > NOTE_LINES {
                            text.push_str(&format!("  (+{} more)", d.notes.len() - NOTE_LINES));
                        }
                        set_text(notes, &text);
                    }
```

`mark_glyph` and its two-space separator are untouched: its advance table was
measured on a14 and the trailing space on `Warn` is load-bearing.

**The `Some(t) => set_text(notes, t)` arm above it is not capped.** That is the
capture prompt, which is one line by construction and outranks the row's notes
while a capture is live.

- [ ] **Step 3: Do NOT add a tooltip — and read why before deciding otherwise**

The obvious next move is to hang the full note list off `IDC_NOTES` using the
existing `add_tooltip` helper (`settings_window.rs:2356`). **It is a trap, and
it is out of scope for this task.**

`add_tooltip`'s own doc records the reason: "**`text` is borrowed, not
copied.** `TTM_ADDTOOLW` stores the `lpszText` pointer; the buffer must outlive
the tooltip." That is affordable for the one existing tooltip because the
config path is computed once and never changes — `Ui` holds the buffer for the
window's life.

The notes change on **every selection**. A tooltip over them needs either
`TTM_UPDATETIPTEXTW` on each push, or a buffer kept at a stable address whose
contents are rewritten in place — and a `Vec<u16>` that reallocates while
comctl32 holds a pointer into it is a use-after-free that will not show up in
any of the checks this plan runs.

`(+N more)` ships alone. It is already strictly better than a clipped third
line, and the followups record that nobody has yet seen three notes at once —
so the right next step is to look at that state, not to build a surface for it
first. Listed under *After the pass*.

- [ ] **Step 4: Type-check, lint, format**

```bash
cargo check  -p beckon-windows --target aarch64-pc-windows-msvc --all-targets
cargo clippy -p beckon-windows --target aarch64-pc-windows-msvc
cargo fmt --all -- --check
```
Expected: clean.

- [ ] **Step 5: Commit**

```bash
git add crates/beckon-windows/src/settings_window.rs
git commit -m "feat(windows): the notes are two lines, not the rest of the window

Measured on a14: a 1220x177 control holding one 258 px line. The band
existed to be empty. Two lines of the notes' own face, and beyond that
'(+N more)'.

Two is a guess -- nobody has seen three notes at once, which the
followups record as unmeasured. Cheap to change, unlike the blank band."
```

---

## Hardware pass on a14

Everything above is type-checked and unit-tested; **none of it has been
looked at.** Run these in order, in one sitting, from **session 1**.

An SSH shell lands in session 0, which has no desktop and no keyboard, so every
result there is a confident false negative. Go through a scheduled task
registered with `New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries
-Priority 4` — **both flags**: `schtasks`' defaults refuse to start on battery
and leave the task `Queued` forever, and `New-ScheduledTask*` defaults to
priority 7, where a task on battery produces no diagnostic at all. Use
`-EncodedCommand` for PowerShell and a `.bat` for anything with a redirect.

Build with `cargo build --workspace --all-targets` — **not `--examples`**,
which does not build `[[bin]]` targets and will leave you testing a stale
`beckon-serve.exe`.

| # | What | Blocks | Read |
|---|---|---|---|
| **G1** | Open the settings window at **96 DPI** on the current build, before any of this lands. Screenshot it. Note every place the layout differs from the 150 % shots in `measurements/2026-08-11-landing-1-a14.md`. | Task 6 Step 3's arithmetic | Every `tok` constant is written in 96-DPI units and every measurement in this project was taken at 150 %. This is the base case, and it is the untested one. |
| **G2** | `showhide_probe.exe` | Task 8 | `CONTROL_CORRUPTED` must be `True` or the run is blind. |
| **G3** | `customdraw_probe.exe` | the 3b plan's §B.6 scope | `CONTROL_INK` must be non-zero or the run is blind. |
| **G5** | `settings_probe.exe`, unchanged | nothing in this plan; it is the regression check | Expect a clean run. Nothing here renumbers a pinned id or changes `IDC_COMBO`'s style — which is exactly why it must be run rather than assumed. |
| **G6** | Re-run `hwpass\GlyphWidth.cs` (§36 — already builds `Segoe UI Variable Small` at 96 DPI and calls `GetTextExtentPoint32W`, the exact call `text_size` makes) and additionally record `sz.cy`, not only `sz.cx`. No new probe needed. | `notes_height`'s 96-DPI line-height figure (see its doc comment in `settings_window.rs`) | §36's own table shows the face's 144→96 advance ratio is not a clean 1.5 (35→22, 20→14, 18→12, i.e. 1.43–1.59, integer rounding at small sizes), so read the ratio-derived 16 px as 15–17, not exact. Either way the disagreement is bounded, not alarming: `MIN_HEIGHT`'s doc shows any real line height up to 18 px is absorbed by the shipped +4 slack, and 19 px costs one list row and nothing else. |
| **eye** | Open the rebuilt window at 96 DPI **and** 150 %: group caption tracks the selection; empty state on deselect; two lines never overlap at `MIN_WIDTH` 720; `Ctrl + Win + Alt + T` in the Shortcut column; the notes cap reads as "there is more," not as a rendering fault. | — | The two-line cap and the `MIN_WIDTH` floor are the two things unit tests structurally cannot see. |
| **eye2** | Select a row whose probe returns `Availability::CaptureSawNothing`, at `MIN_WIDTH` 720, with ≥3 notes on that row: does the note's own text wrap past two rendered lines, does a third note clip, and is `(+N more)` itself visible or clipped? | — | `IDC_NOTES` is `SS_LEFT` and word-wraps: the cap in `apply_state` bounds NOTES, not RENDERED lines, so one wide note can already overflow the two-line box on its own. This is the specific setup that exposes it — deliberately not left to whichever row happens to be selected. |

**G4 is not in this table.** §F.4's `GetAsyncKeyState` union at commit — hold
`Ctrl`, click `Record` with the mouse, press `Alt+T`, get `alt+t` — blocks
landing 3b's Part C, not anything here. It needs its own plan; nothing in this
one makes it better or worse.

Write the results into
`docs/superpowers/measurements/2026-08-11-landing-1-a14.md` as §66 onward,
following the numbering already there, and **suspect the probe before the
product** when one reports a failure: that lesson was paid for six times in two
days and is written up in `2026-08-12-landing-2b-followups.md` §5.

---

## After the pass

1. Update `CLAUDE.md`'s Windows section with the cross-check loop from *Inner
   loop* — that `cargo check --target aarch64-pc-windows-msvc --all-targets`
   works from macOS is not written down anywhere and would have saved this
   session a CI round trip per edit.
2. Write `docs/superpowers/plans/YYYY-MM-DD-settings-keycaps-landing-3b.md`
   using G1's 96-DPI numbers for §B.2's cap metrics and G3's verdict for
   §B.6's scope.
3. If the eye pass says line 2 is too tight at `MIN_WIDTH` 720, revert Task 7
   alone and re-derive — that is why it is its own commit.
4. **Look at three notes at once**, which nobody has. Then decide whether
   `(+N more)` needs a tooltip behind it (Task 9 Step 3 explains the
   `TTM_ADDTOOLW` lifetime hazard that makes it a task of its own), or whether
   two lines plus a count is simply the right answer.
5. `Role`'s doc comment (`settings_window.rs:628`) says "There is no fourth:
   the `Keys` role the spec table also lists belongs to keycap rendering,
   **which this window does not do**." Landing 3b makes that sentence false.
   It is correct today and must be amended there, not here.
