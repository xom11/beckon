# macOS menu bar, phase 1 — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the macOS `serve` menu with the short native menu from
spec §3. It has a header with a status dot and a *Shortcuts on* switch, a
*Needs attention* section that exists only while a binding is broken, and a
*Shortcuts ▸* submenu. It also adds ⌥-click on the icon to pause, and dims the
icon while paused.

**Architecture:**
- **Decisions go to `beckon-core`, all pure and unit-tested.** That covers
  glyph chords, the chain winner, the headline, the row flag, the attention
  cut and finding a model row by chord.
- **Composition stays in `serve.rs`**, next to today's `build_entries`, as a
  new `build_mac_entries`. Windows keeps calling `build_entries`, which does
  not change, so its menu is unchanged by construction.
- **`tray.rs` draws the new entry kinds.** Its correctness is established by a
  person looking at an Aqua session, not by a test (the `tray.rs` module doc
  explains why).

**Tech Stack:** Rust 2021, `objc2` 0.6 / `objc2-app-kit` 0.3.2 /
`objc2-foundation` 0.3.2. All their class features are on by default, so
nothing is added to `Cargo.toml`.

**Spec:** `docs/superpowers/specs/2026-09-22-macos-ui-redesign-design.md`,
§3 in full, plus §7's glyph renderer. Read §3 before Task 7.

## Global Constraints

- **Not in this phase, though the spec's §3 names them:**
  - the submenu's `Show Keyboard Map` row, and the `?` fold for `shift+slash`
    (phase 6);
  - the `Allow Accessibility...` row under the header (phase 5). Phase 1 does
    ship the header's *Needs Accessibility* subtitle, because it is one
    boolean read.
- **macOS only.** `build_entries` and `beckon_windows::hotkey` keep producing
  and drawing exactly today's Windows menu. `build_entries` gains nothing but
  `..MenuEntry::default()` in its struct literals.
- **Menu rows never focus or launch anything.** A binding row opens Settings
  on that binding (`CLAUDE.md`: "Nothing in the shortcut table focuses or
  launches anything").
- **`set_paused` and `reload` are called, never re-implemented** (`CLAUDE.md`,
  "Out of scope → GUI").
- **Status words come from `beckon_core::settings::FLAGS`** and keep its
  precedence: `paused` > `in use` > `missing` > `other chord`.
- **Labels are ASCII:** `...` not `…`, and `-` not `—`, per
  `beckon_core::menu::update_label`'s doc.
  - The chord glyphs `⌃⌥⇧⌘⇪` are the one sanctioned exception. They are
    macOS-only, and they never reach a log line or `set_status`.
- **Borrow discipline:** drop every `RefCell` borrow before a call that can
  re-enter `serve` or AppKit (`open_settings`, `set_paused`, `reload`,
  `dispatch`). That is the `serve.rs` module doc's rule, and the reason
  `tray.rs` takes callbacks out of its slot before running them.
- **Resolution runs at config load and reload, never when the menu opens**
  (spec §3.5). The hot path, `beckon <id>`, is untouched.
- **Git:**
  - Run `git branch --show-current` before every commit.
  - Commit with `git commit --only <paths>`.
  - Verify with `git show --stat HEAD`, and after a push with
    `git ls-remote --heads origin <branch>`.
- **Toolchain:** CI runs today's `stable`, and this machine's default is
  `rustc 1.95.0`, which is older and has fewer lints. Every gate below runs
  as `cargo +stable …`.
- **Target dirs:**
  - Use `CARGO_TARGET_DIR=~/Documents/dev/beckon/target` for
    check/clippy/fmt/test.
  - Use a **private** target dir for any binary you intend to run.
  - The first exec of a freshly linked binary on this machine is killed
    (exit 137, empty output); run it twice.

---

## Before you start: workspace

- [ ] **Merge the spec, then branch from it.** The spec and this plan live on
  `macos-ui-redesign-spec`.

```bash
cd ~/Documents/dev/beckon
git fetch --all
git -C .worktrees/macos-ui-redesign-spec status --porcelain   # must be empty
git merge --ff-only origin/macos-ui-redesign-spec             # in the PRIMARY checkout, on main
git push origin main
git ls-remote --heads origin main                             # verify it moved
git worktree add .worktrees/macos-menu-phase-1 -b macos-menu-phase-1 origin/main
cd .worktrees/macos-menu-phase-1
git branch --show-current                                     # macos-menu-phase-1
```

- [ ] **Look for company** (the `CLAUDE.md` block): `git worktree list`,
  `git branch -a`, `git log --all --oneline -20`, `ListAgents`. If another
  branch touches `tray.rs`, `menu.rs` or `serve.rs`'s menu section, stop and
  reconcile first.

- [ ] **Install CI's toolchain, with the Windows target:**

```bash
rustup toolchain install stable
rustup target add aarch64-pc-windows-msvc --toolchain stable
rustc +stable --version
export CARGO_TARGET_DIR=~/Documents/dev/beckon/target
cargo +stable test -p beckon-core -q 2>&1 | tail -3             # baseline: green
```

---

### Task 1: `combo_glyphs` — a chord in macOS menu glyphs

**Files:**
- Modify: `crates/beckon-core/src/shortcuts.rs`. Add after
  `combo_display_folded_with`, currently around line 469.
- Test: the same file, in a new `#[cfg(test)] mod glyph_tests`.

**Interfaces:**
- Consumes: `Combo::parse`, `key_label`, `combo_folds_to_caps`, `Chord`
  (all in `shortcuts.rs`).
- Produces:
  - `pub fn combo_glyphs(s: &str, hold: Option<Chord>) -> String`
  - `pub const CAPS_GLYPH: &str = "⇪";`

- [ ] **Step 1: Write the failing tests.** Append to `shortcuts.rs`:

```rust
#[cfg(test)]
mod glyph_tests {
    use super::*;

    const HOLD: Chord = Chord {
        ctrl: true,
        super_: true,
        alt: true,
    };

    /// Control, Option, Shift, Command -- the order AppKit draws a key
    /// equivalent in, whatever order the file wrote.
    #[test]
    fn macos_order_is_control_option_shift_command() {
        assert_eq!(combo_glyphs("super+shift+alt+ctrl+t", None), "⌃⌥⇧⌘T");
    }

    #[test]
    fn the_hold_chord_folds_to_one_caps_glyph() {
        assert_eq!(combo_glyphs("ctrl+super+alt+c", Some(HOLD)), "⇪C");
    }

    /// The same predicate as the list's fold, so a shift row stays long here
    /// for the reason `combo_folds_to_caps` gives.
    #[test]
    fn a_shift_row_does_not_fold() {
        assert_eq!(
            combo_glyphs("ctrl+super+alt+shift+m", Some(HOLD)),
            "⌃⌥⇧⌘M"
        );
    }

    #[test]
    fn no_hold_means_no_fold() {
        assert_eq!(combo_glyphs("ctrl+super+alt+c", None), "⌃⌥⌘C");
    }

    /// A word key glued to glyphs reads as one token; a single character
    /// must not get a gap.
    #[test]
    fn a_word_key_is_set_apart_and_a_letter_is_not() {
        assert_eq!(combo_glyphs("ctrl+super+alt+space", Some(HOLD)), "⇪ Space");
        assert_eq!(combo_glyphs("ctrl+alt+space", None), "⌃⌥ Space");
        assert_eq!(combo_glyphs("ctrl+alt+slash", None), "⌃⌥/");
    }

    #[test]
    fn an_unparseable_chord_is_empty() {
        assert_eq!(combo_glyphs("ctrl+nosuchkey", None), "");
    }
}
```

- [ ] **Step 2: Run them and see them fail**

Run: `cargo +stable test -p beckon-core glyph_tests`
Expected: compile error, `cannot find function combo_glyphs`.

- [ ] **Step 3: Implement.** Insert after `combo_display_folded_with`:

```rust
/// The Caps Lock glyph a folded chord wears on macOS. `CAPS_CAP` is the word
/// for the same thing in the list.
pub const CAPS_GLYPH: &str = "⇪";

/// The chord in macOS menu glyphs: `⌃⌥⇧⌘T`, or `⇪T` when `hold` folds it.
///
/// **macOS order** -- Control, Option, Shift, Command -- which is the order
/// AppKit draws a key equivalent in and NOT `combo_caps`' order (Ctrl, Super,
/// Alt). Folding asks `combo_folds_to_caps`, so the menu and the list cannot
/// disagree about which rows are the common chord.
///
/// **Display only, and macOS only.** Glyphs are not ASCII; the ASCII rule for
/// display strings (`ModifierLabels::MAC`'s doc) exists because of a Windows
/// log, and this string never reaches a log line. Anything spoken or logged
/// uses `combo_display_folded_with(.., ModifierLabels::MAC)` instead.
///
/// Empty when the string does not parse, like `combo_caps`.
pub fn combo_glyphs(s: &str, hold: Option<Chord>) -> String {
    let Ok(c) = Combo::parse(s) else {
        return String::new();
    };
    let key = key_label(&c.key.name);
    let sep = if key.chars().count() > 1 { " " } else { "" };
    if let Some(h) = hold {
        if combo_folds_to_caps(&c, h) {
            return format!("{CAPS_GLYPH}{sep}{key}");
        }
    }
    let mut g = String::new();
    if c.ctrl {
        g.push('⌃');
    }
    if c.alt {
        g.push('⌥');
    }
    if c.shift {
        g.push('⇧');
    }
    if c.super_ {
        g.push('⌘');
    }
    format!("{g}{sep}{key}")
}
```

- [ ] **Step 4: Run the tests and see them pass**

Run: `cargo +stable test -p beckon-core glyph_tests`
Expected: 6 passed.

- [ ] **Step 5: Commit**

```bash
git branch --show-current
git commit --only crates/beckon-core/src/shortcuts.rs -m "feat(core): combo_glyphs draws a chord the way a macOS menu does"
git show --stat HEAD
```

---

### Task 2: `chain_winner` — one spelling of "which candidate wins"

**Files:**
- Modify: `crates/beckon-core/src/certainty.rs`. Add a public fn after
  `NameReport`'s impls, before `#[cfg(test)] mod tests`, plus tests inside
  that module, which already has a `report(id, certainty)` helper at
  around line 201.
- Modify: `crates/beckon-cli/src/lib.rs:556-571`, the `winner` closure in
  `check_resolution`.

**Interfaces:**
- Produces: `pub fn chain_winner<'a>(cands: &[&str], grade: impl Fn(&str) -> Option<&'a NameReport>) -> Option<&'a NameReport>`.
  Task 6 uses it.

- [ ] **Step 1: Write the failing tests.** Add inside `certainty.rs`'s
  `mod tests`:

```rust
    // ---------- chain_winner ----------

    fn graded<'a>(rs: &'a [NameReport]) -> impl Fn(&str) -> Option<&'a NameReport> {
        move |c| rs.iter().find(|r| r.id == c)
    }

    #[test]
    fn the_first_candidate_that_is_not_a_miss_wins() {
        let rs = [
            report("File Explorer", Certainty::NoMatch),
            report("Finder", Certainty::Exact),
            report("Files", Certainty::Exact),
        ];
        let w = chain_winner(&["File Explorer", "Finder", "Files"], graded(&rs)).unwrap();
        assert_eq!(w.id, "Finder");
    }

    /// A Guess resolves: the ladder stops there, and so must the grade.
    #[test]
    fn a_guess_stops_the_ladder() {
        let rs = [report("Brave", Certainty::Guess), report("Brave Browser", Certainty::Exact)];
        assert_eq!(chain_winner(&["Brave", "Brave Browser"], graded(&rs)).unwrap().id, "Brave");
    }

    /// Every rung missed: the LAST candidate is the one whose absence is news.
    #[test]
    fn when_every_candidate_misses_the_last_one_is_reported() {
        let rs = [report("A", Certainty::NoMatch), report("B", Certainty::NoMatch)];
        let w = chain_winner(&["A", "B"], graded(&rs)).unwrap();
        assert_eq!((w.id.as_str(), w.certainty), ("B", Certainty::NoMatch));
    }

    #[test]
    fn an_unanswered_candidate_or_an_empty_chain_is_none() {
        let rs = [report("A", Certainty::NoMatch)];
        assert!(chain_winner(&["A", "B"], graded(&rs)).is_none());
        assert!(chain_winner(&[], graded(&rs)).is_none());
    }
```

- [ ] **Step 2: Run them and see them fail**

Run: `cargo +stable test -p beckon-core chain_winner`
Expected: compile error, `cannot find function chain_winner`.

- [ ] **Step 3: Implement.** Add to `certainty.rs` above `#[cfg(test)]`:

```rust
/// The candidate a binding will actually live with: the first of `cands`
/// that is not a `NoMatch`, else the LAST one -- the fallback the user added,
/// whose absence is the news.
///
/// `None` when `cands` is empty or when `grade` has no report for a
/// candidate the ladder reached. **Those are not "missing"**: not-answered is
/// not the same as not-installed, and a caller that must tell the two apart
/// checks `grade` itself (`check --resolve` does, and errors).
///
/// This is `beckon_ladder`'s rule. `check --resolve` and the macOS menu both
/// ask it, so a check line and a menu row cannot disagree about which app a
/// key opens.
pub fn chain_winner<'a>(
    cands: &[&str],
    grade: impl Fn(&str) -> Option<&'a NameReport>,
) -> Option<&'a NameReport> {
    let mut last = None;
    for c in cands {
        let r = grade(c)?;
        if r.certainty != Certainty::NoMatch {
            return Some(r);
        }
        last = Some(r);
    }
    last
}
```

- [ ] **Step 4: Point `check_resolution` at it.** In
  `crates/beckon-cli/src/lib.rs`, replace the body of the `winner` closure
  (the `let mut last = None; for c in cands { … } … last.ok_or_else(…)`
  lines). Keep the comment block above the closure:

```rust
    let winner = |cands: &[&str]| -> Result<&NameReport> {
        // Checked first so the error still names the candidate: `chain_winner`
        // folds "not answered" into `None`, and here that is a resolver
        // contract violation, not a miss.
        if let Some(c) = cands.iter().find(|c| !grade.contains_key(**c)) {
            return Err(anyhow!("the resolver returned no report for `{c}`"));
        }
        beckon_core::certainty::chain_winner(cands, |c| grade.get(c).copied())
            .ok_or_else(|| anyhow!("`{}` names no candidate at all", cands.join(" || ")))
    };
```

  If `Context` is now unused in `lib.rs`, clippy will say so; only then
  remove it from the `use anyhow::{…}` line.

- [ ] **Step 5: Run both crates' tests**

Run: `cargo +stable test -p beckon-core chain_winner && cargo +stable test -p beckon-cli check`
Expected: the 4 new tests pass, and every existing `check` test in
`beckon-cli` passes unchanged. They are what pins that the refactor kept its
behaviour.

- [ ] **Step 6: Commit**

```bash
git branch --show-current
git commit --only crates/beckon-core/src/certainty.rs crates/beckon-cli/src/lib.rs \
  -m "refactor(core): chain_winner is the one spelling of which candidate a key opens"
git show --stat HEAD
```

---

### Task 3: `MenuEntry` grows the kinds macOS needs

**Files:**
- Modify: `crates/beckon-core/src/menu.rs`.
- Modify (literals only): `crates/beckon-cli/src/serve.rs:1198,1218,1227,1235,1243`
  and `crates/beckon-macos/examples/tray_probe.rs:77,85`.

**Interfaces:**
- Produces:
  - `MenuEntry` with these new fields, all defaulting to "absent":
    `kind: EntryKind`, `detail: Option<String>`, `flag: Option<&'static str>`,
    `icon: Option<String>` (a bundle id), `key: Option<char>` (a ⌘ key
    equivalent), `tooltip: Option<String>`, `children: Vec<MenuEntry>`.
  - `#[derive(Default)] pub enum EntryKind { #[default] Item, SectionHeader, Header(Header), Submenu }`
  - `pub struct Header { pub title: String, pub subtitle: String, pub dot: Dot, pub on: bool }`
  - `#[derive(Clone, Copy)] pub enum Dot { Ok, Warn, Off }`
  - `MenuEntry::section_header(label)`
  - `pub const MENU_ID_ALT_CLICK: u32 = u32::MAX - 1;`
  - `impl Default for MenuEntry`

- [ ] **Step 1: Write the failing tests.** Add to `menu.rs`'s `mod tests`:

```rust
    /// The whole Windows-safety claim of this change: the rows Windows draws
    /// are built exactly as before.
    #[test]
    fn a_separator_and_an_item_are_what_they_always_were() {
        let s = MenuEntry::separator();
        assert_eq!((s.id, s.label.as_str(), s.checked, s.enabled), (0, "", None, false));
        assert!(s.is_separator());
        let i = MenuEntry::item(7, "Quit");
        assert_eq!((i.id, i.label.as_str(), i.checked, i.enabled), (7, "Quit", None, true));
        assert_eq!(i.kind, EntryKind::Item);
        assert!(i.children.is_empty() && i.detail.is_none() && i.key.is_none());
    }

    /// Only a plain row with an empty label is a rule. A header or a section
    /// title must never be mistaken for one, whatever its label.
    #[test]
    fn only_a_plain_empty_row_is_a_separator() {
        let h = MenuEntry {
            kind: EntryKind::Header(Header {
                title: String::new(),
                subtitle: String::new(),
                dot: Dot::Ok,
                on: true,
            }),
            ..MenuEntry::default()
        };
        assert!(!h.is_separator());
        assert!(!MenuEntry::section_header("").is_separator());
    }

    #[test]
    fn the_two_reserved_ids_are_distinct_and_at_the_top() {
        assert_ne!(MENU_ID_ALT_CLICK, MENU_ID_DOUBLE_CLICK);
        assert!(MENU_ID_ALT_CLICK > 1_000_000);
    }
```

- [ ] **Step 2: Run them and see them fail**

Run: `cargo +stable test -p beckon-core menu::`
Expected: compile errors for `EntryKind`, `Header`, `Dot`, `section_header`
and `MENU_ID_ALT_CLICK`.

- [ ] **Step 3: Implement.** In `menu.rs`, replace the `MenuEntry` struct, its
  `impl` and the `MENU_ID_DOUBLE_CLICK` const with:

```rust
/// One row of the context menu.
///
/// **The first four fields are the whole of what Windows draws**, and
/// `serve::build_entries` fills only those, so the Windows menu is unchanged
/// by everything below them. The rest are macOS rows (spec
/// `2026-09-22-macos-ui-redesign-design.md` §3.4). Each defaults to "absent",
/// which is what lets a row that does not use a field say nothing.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MenuEntry {
    pub id: u32,
    pub label: String,
    /// `None` for a plain item, `Some(bool)` for a check box.
    pub checked: Option<bool>,
    pub enabled: bool,
    /// What kind of row. `Item` for every row Windows draws.
    pub kind: EntryKind,
    /// Right-aligned secondary text: the binding's chord, in glyphs.
    pub detail: Option<String>,
    /// One word from `settings::FLAGS`, drawn beside the label.
    pub flag: Option<&'static str>,
    /// A bundle id whose application icon leads the row.
    pub icon: Option<String>,
    /// A key equivalent AppKit draws and honours while the menu is open,
    /// always with Command (`,` `r` `q`).
    pub key: Option<char>,
    /// Words for the row, for hover and for VoiceOver -- the glyphs in
    /// `detail` are not something a screen reader should be handed.
    pub tooltip: Option<String>,
    /// The rows of a submenu. Non-empty only for `EntryKind::Submenu`.
    pub children: Vec<MenuEntry>,
}

/// What a row is. Everything but `Item` is macOS-only in practice.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum EntryKind {
    #[default]
    Item,
    /// A small grey title over the rows that follow (`Needs attention`).
    SectionHeader,
    /// The menu's first row: a status dot, a title, a subtitle and a switch.
    /// The switch reports this entry's `id` when flipped.
    Header(Header),
    /// A row that opens `children`.
    Submenu,
}

/// The header row's content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Header {
    pub title: String,
    pub subtitle: String,
    pub dot: Dot,
    /// Whether the switch is on -- i.e. shortcuts are NOT paused.
    pub on: bool,
}

/// The header's status dot. Grey while paused, orange when something needs
/// the user, green otherwise (spec §3.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dot {
    Ok,
    Warn,
    Off,
}

impl MenuEntry {
    /// A horizontal rule: a plain row with an empty label.
    pub fn separator() -> Self {
        Self::default()
    }

    /// A plain, enabled row.
    pub fn item(id: u32, label: impl Into<String>) -> Self {
        Self {
            id,
            label: label.into(),
            enabled: true,
            ..Self::default()
        }
    }

    /// A section title. It carries no id and nothing to click.
    pub fn section_header(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            kind: EntryKind::SectionHeader,
            ..Self::default()
        }
    }

    /// Recognised by an empty label on a PLAIN row. A header or a section
    /// title with an empty label is still that thing, not a rule.
    pub fn is_separator(&self) -> bool {
        matches!(self.kind, EntryKind::Item) && self.label.is_empty()
    }
}

/// Delivered to `on_click` when the icon is double-clicked. Callers must
/// number their real entries below both reserved ids.
pub const MENU_ID_DOUBLE_CLICK: u32 = u32::MAX;

/// Delivered to `on_click` when the icon is ⌥-clicked (macOS). It never opens
/// the menu; `serve` toggles pause on it.
pub const MENU_ID_ALT_CLICK: u32 = u32::MAX - 1;
```

- [ ] **Step 4: Fix the seven struct literals.** Each `MenuEntry { id: …, label: …, checked: …, enabled: … }`
  in `serve.rs` (`build_entries`, five of them) and in
  `examples/tray_probe.rs` (two) gains a last line `..MenuEntry::default()`.
  Change nothing else in `build_entries`.

- [ ] **Step 5: Run everything that builds a menu**

```bash
cargo +stable test -p beckon-core menu::
cargo +stable test -p beckon-cli menu
cargo +stable clippy --target aarch64-pc-windows-msvc -p beckon-windows -p beckon-cli --all-targets -- -D warnings
cargo +stable check -p beckon-macos --examples
```

Expected: the new tests pass. Every existing `build_entries` test in
`serve.rs` passes unchanged, which is the Windows-output claim. The Windows
cross-clippy is clean, and `tray_probe` compiles.

- [ ] **Step 6: Commit**

```bash
git branch --show-current
git commit --only crates/beckon-core/src/menu.rs crates/beckon-cli/src/serve.rs crates/beckon-macos/examples/tray_probe.rs \
  -m "feat(core): MenuEntry grows headers, sections, submenus and chords -- Windows draws what it did"
git show --stat HEAD
```

---

### Task 4: the menu's three decisions — headline, row flag, attention

**Files:**
- Modify: `crates/beckon-core/src/menu.rs`, plus tests in its `mod tests`.

**Interfaces:**
- Consumes: `Dot` (Task 3) and `crate::settings::FLAGS`.
- Produces:
  - `pub struct HeadlineInputs<'a> { pub paused: bool, pub accessibility: bool, pub phrase: &'a str, pub failed: usize, pub missing: usize, pub total: usize }`
  - `pub struct Headline { pub text: String, pub dot: Dot }`
  - `pub fn menu_headline(i: HeadlineInputs) -> Headline`
  - `pub fn row_flag(in_use: bool, missing: bool) -> Option<&'static str>`
  - `pub const ATTENTION_ROWS: usize = 3;`
  - `pub struct Attention { pub shown: Vec<usize>, pub rest: Vec<usize> }`
  - `pub fn attention(flags: &[Option<&str>], max: usize) -> Attention`

- [ ] **Step 1: Write the failing tests**

```rust
    fn inputs() -> HeadlineInputs<'static> {
        HeadlineInputs {
            paused: false,
            accessibility: true,
            phrase: "19 shortcuts registered",
            failed: 0,
            missing: 0,
            total: 19,
        }
    }

    /// One test per rung, each proving it outranks the next (spec §3.2).
    #[test]
    fn paused_outranks_everything() {
        let h = menu_headline(HeadlineInputs { paused: true, accessibility: false, failed: 2, missing: 2, ..inputs() });
        assert_eq!(h, Headline { text: "Paused - shortcuts are off".into(), dot: Dot::Off });
    }

    #[test]
    fn missing_accessibility_outranks_a_failed_registration() {
        let h = menu_headline(HeadlineInputs { accessibility: false, failed: 2, ..inputs() });
        assert_eq!(h, Headline { text: "Needs Accessibility to switch windows".into(), dot: Dot::Warn });
    }

    /// A failed registration is said in `serve`'s own words, verbatim.
    #[test]
    fn a_failed_registration_uses_the_registration_phrase() {
        let h = menu_headline(HeadlineInputs {
            phrase: "17 of 19 shortcuts registered (2 failed)",
            failed: 2,
            missing: 1,
            ..inputs()
        });
        assert_eq!(h.text, "17 of 19 shortcuts registered (2 failed)");
        assert_eq!(h.dot, Dot::Warn);
    }

    /// Missing apps are counted but do not turn the dot orange: the keys are
    /// registered and serving.
    #[test]
    fn missing_apps_are_counted_on_a_green_dot() {
        let h = menu_headline(HeadlineInputs { missing: 2, ..inputs() });
        assert_eq!(h, Headline { text: "19 shortcuts, 2 missing".into(), dot: Dot::Ok });
    }

    #[test]
    fn a_clean_file_just_counts_and_agrees_in_number() {
        assert_eq!(menu_headline(inputs()).text, "19 shortcuts");
        assert_eq!(menu_headline(HeadlineInputs { total: 1, ..inputs() }).text, "1 shortcut");
    }

    #[test]
    fn every_headline_is_ascii() {
        for i in [
            inputs(),
            HeadlineInputs { paused: true, ..inputs() },
            HeadlineInputs { accessibility: false, ..inputs() },
            HeadlineInputs { missing: 3, ..inputs() },
        ] {
            assert!(menu_headline(i).text.is_ascii());
        }
    }

    /// The indices into FLAGS are what `row_flag` depends on; pin the words.
    #[test]
    fn row_flag_speaks_the_settings_vocabulary_in_its_precedence() {
        assert_eq!(crate::settings::FLAGS[1], "in use");
        assert_eq!(crate::settings::FLAGS[2], "missing");
        assert_eq!(row_flag(true, true), Some("in use"));
        assert_eq!(row_flag(false, true), Some("missing"));
        assert_eq!(row_flag(false, false), None);
    }

    #[test]
    fn attention_keeps_file_order_and_cuts_at_max() {
        let flags = [None, Some("missing"), None, Some("in use"), Some("missing"), Some("missing"), Some("missing")];
        let a = attention(&flags, ATTENTION_ROWS);
        assert_eq!(a.shown, vec![1, 3, 4]);
        assert_eq!(a.rest, vec![5, 6]);
        assert_eq!(attention(&[None, None], 3), Attention::default());
    }
```

- [ ] **Step 2: Run them and see them fail**

Run: `cargo +stable test -p beckon-core menu::`
Expected: compile errors for `menu_headline`, `row_flag` and `attention`.

- [ ] **Step 3: Implement.** Add to `menu.rs`, above `#[cfg(test)]`:

```rust
/// What the macOS header says, and the colour of its dot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Headline {
    pub text: String,
    pub dot: Dot,
}

/// Everything `menu_headline` weighs, already counted by `serve`.
#[derive(Debug, Clone, Copy)]
pub struct HeadlineInputs<'a> {
    pub paused: bool,
    pub accessibility: bool,
    /// `serve`'s `last_phrase`, said verbatim when a registration failed so
    /// the header and the tooltip use one sentence.
    pub phrase: &'a str,
    /// Bindings whose registration returned an error.
    pub failed: usize,
    /// Bindings whose every candidate is a `NoMatch`.
    pub missing: usize,
    pub total: usize,
}

/// The header's subtitle and dot. The precedence is spec §3.2, and each rung
/// hides the ones below it because the one above explains more.
pub fn menu_headline(i: HeadlineInputs) -> Headline {
    if i.paused {
        return Headline {
            text: "Paused - shortcuts are off".into(),
            dot: Dot::Off,
        };
    }
    if !i.accessibility {
        return Headline {
            text: "Needs Accessibility to switch windows".into(),
            dot: Dot::Warn,
        };
    }
    if i.failed > 0 {
        return Headline {
            text: i.phrase.to_string(),
            dot: Dot::Warn,
        };
    }
    let noun = if i.total == 1 { "shortcut" } else { "shortcuts" };
    let text = if i.missing > 0 {
        format!("{} {noun}, {} missing", i.total, i.missing)
    } else {
        format!("{} {noun}", i.total)
    };
    Headline { text, dot: Dot::Ok }
}

/// The one word a binding row carries in the menu, from `settings::FLAGS`,
/// in `row_condition`'s precedence.
///
/// `paused` is absent on purpose -- the header says it once, instead of every
/// row saying it. `other chord` is absent because it is a view fact about the
/// list, not a fault (`docs/notes/settings-window.md`).
pub fn row_flag(in_use: bool, missing: bool) -> Option<&'static str> {
    if in_use {
        Some(crate::settings::FLAGS[1])
    } else if missing {
        Some(crate::settings::FLAGS[2])
    } else {
        None
    }
}

/// How many broken rows the *Needs attention* section lists before folding
/// the rest into one `and N more...` row.
pub const ATTENTION_ROWS: usize = 3;

/// Which rows *Needs attention* shows, in file order, and which it folds.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Attention {
    pub shown: Vec<usize>,
    pub rest: Vec<usize>,
}

/// Split the flagged rows of `flags` (one per binding, in file order) into
/// the first `max` and the rest. Both empty means the section is not drawn.
pub fn attention(flags: &[Option<&str>], max: usize) -> Attention {
    let flagged: Vec<usize> = flags
        .iter()
        .enumerate()
        .filter(|(_, f)| f.is_some())
        .map(|(i, _)| i)
        .collect();
    let cut = flagged.len().min(max);
    Attention {
        shown: flagged[..cut].to_vec(),
        rest: flagged[cut..].to_vec(),
    }
}
```

- [ ] **Step 4: Run the tests and see them pass**

Run: `cargo +stable test -p beckon-core menu::`
Expected: everything passes.

- [ ] **Step 5: Commit**

```bash
git branch --show-current
git commit --only crates/beckon-core/src/menu.rs -m "feat(core): the macOS menu's headline, row flag and attention cut"
git show --stat HEAD
```

---

### Task 5: `Model::row_for_combo`

**Files:**
- Modify: `crates/beckon-core/src/settings.rs`. Add the method inside
  `impl Model`, which starts at around line 1892, and a test in its
  `mod tests`, next to `fn three()` at around line 6194.

**Interfaces:**
- Produces: `pub fn row_for_combo(&self, canonical: &str) -> Option<usize>`
  on `Model`. It returns a MODEL index, the same kind of index as
  `Model::selected`. Task 10 uses it.

- [ ] **Step 1: Write the failing test**

```rust
    /// A menu row names its binding by chord, and the file may spell that
    /// chord in any modifier order.
    #[test]
    fn a_row_is_found_by_its_chord_however_the_file_spelled_it() {
        let m = Model::from_text("\"alt+ctrl+a\"=\"Notepad\"\n\"ctrl+alt+b\"=\"Brave\"\n").unwrap();
        assert_eq!(m.row_for_combo("ctrl+alt+a"), Some(0));
        assert_eq!(m.row_for_combo("ctrl+alt+b"), Some(1));
        assert_eq!(m.row_for_combo("ctrl+alt+z"), None);
    }
```

- [ ] **Step 2: Run it and see it fail**

Run: `cargo +stable test -p beckon-core a_row_is_found_by_its_chord`
Expected: compile error, `no method named row_for_combo`.

- [ ] **Step 3: Implement.** Add inside `impl Model`:

```rust
    /// The MODEL row whose chord is `canonical` (`Combo::canonical`
    /// spelling), however the file wrote it. A row whose chord does not parse
    /// matches nothing.
    ///
    /// A model index, like `selected` -- not a view index, which the filter
    /// moves (`selected_is_a_view_index_while_filtered`).
    pub fn row_for_combo(&self, canonical: &str) -> Option<usize> {
        self.rows.iter().position(|r| {
            crate::shortcuts::Combo::parse(&r.combo)
                .map(|c| c.canonical() == canonical)
                .unwrap_or(false)
        })
    }
```

- [ ] **Step 4: Run the test and see it pass**

Run: `cargo +stable test -p beckon-core a_row_is_found_by_its_chord`
Expected: 1 passed.

- [ ] **Step 5: Commit**

```bash
git branch --show-current
git commit --only crates/beckon-core/src/settings.rs -m "feat(core): find a settings row by its chord"
git show --stat HEAD
```

---

### Task 6: what each binding resolved to, and how its row reads

**Files:**
- Modify: `crates/beckon-cli/src/serve.rs`. Add a new section just above
  `fn build_entries`, the "Tray / menu bar menu" block at around line 1120.
- Test: the same file's `mod tests`, at the end.

**Interfaces:**
- Consumes: `beckon_core::certainty::chain_winner` (Task 2),
  `beckon_core::menu::row_flag` (Task 4), `beckon_core::shortcuts::combo_glyphs`
  (Task 1), `combo_display_folded_with` and `ModifierLabels::MAC`.
- Produces:
  - `struct Resolved { name: String, bundle_id: Option<String>, missing: bool }`,
    which derives `Debug, Clone, PartialEq, Eq, Default`
  - `fn resolve_rows(shortcuts: &[Shortcut], reports: &[NameReport]) -> Vec<Resolved>`
  - `struct BindingRow { name: String, chord: String, spoken: String, flag: Option<&'static str>, bundle_id: Option<String> }`
  - `fn binding_rows(shortcuts: &[Shortcut], resolved: &[Resolved], registered: &HashMap<String, Result<(), String>>, paused: bool, hold: Option<Chord>) -> Vec<BindingRow>`
  - All of them are gated `#[cfg(any(target_os = "macos", test))]`. Windows
    CI still runs their tests.

- [ ] **Step 1: Write the failing tests.** Append to `serve.rs`'s `mod tests`:

```rust
    use beckon_core::certainty::{Certainty, NameReport};
    use beckon_core::shortcuts::{parse_shortcuts, Chord};

    fn nr(id: &str, certainty: Certainty, target: Option<&str>) -> NameReport {
        NameReport {
            id: id.into(),
            certainty,
            target: target.map(Into::into),
            tier: None,
            consequence: String::new(),
            suggestions: Vec::new(),
            rivals: Vec::new(),
            cold: None,
        }
    }

    #[test]
    fn a_chain_row_is_named_by_the_candidate_that_wins() {
        let s = parse_shortcuts("\"ctrl+alt+f\" = \"File Explorer || Finder\"\n").unwrap();
        let r = resolve_rows(
            &s,
            &[
                nr("File Explorer", Certainty::NoMatch, None),
                nr("Finder", Certainty::Exact, Some("com.apple.finder")),
            ],
        );
        assert_eq!(
            r,
            vec![Resolved { name: "Finder".into(), bundle_id: Some("com.apple.finder".into()), missing: false }]
        );
    }

    /// Named by the FIRST candidate: that is the app the user thinks of the
    /// key as, even though `check --resolve` reports the last.
    #[test]
    fn a_row_whose_every_candidate_misses_is_missing_under_its_first_name() {
        let s = parse_shortcuts("\"ctrl+alt+j\" = \"Tao Monitor || https://tao.example/\"\n").unwrap();
        let r = resolve_rows(
            &s,
            &[
                nr("Tao Monitor", Certainty::NoMatch, None),
                nr("https://tao.example/", Certainty::NoMatch, None),
            ],
        );
        assert_eq!(r[0], Resolved { name: "Tao Monitor".into(), bundle_id: None, missing: true });
    }

    #[test]
    fn a_guess_resolves_rather_than_going_missing() {
        let s = parse_shortcuts("\"ctrl+alt+b\" = \"Brave\"\n").unwrap();
        let r = resolve_rows(&s, &[nr("Brave", Certainty::Guess, Some("com.brave.Browser"))]);
        assert!(!r[0].missing);
        assert_eq!(r[0].bundle_id.as_deref(), Some("com.brave.Browser"));
    }

    /// Not answered is not the same as not installed -- the same distinction
    /// as "not yet probed is not free".
    #[test]
    fn an_unanswered_name_is_unknown_not_missing() {
        let s = parse_shortcuts("\"ctrl+alt+b\" = \"Brave\"\n").unwrap();
        assert_eq!(resolve_rows(&s, &[])[0], Resolved { name: "Brave".into(), bundle_id: None, missing: false });
    }

    /// A chain that does not split can never launch anything, which is what
    /// `missing` means. `beckon_ladder` refuses it before any backend.
    #[test]
    fn a_chain_that_does_not_split_is_missing_under_its_whole_text() {
        let s = parse_shortcuts("\"ctrl+alt+b\" = \"Brave ||\"\n").unwrap();
        assert_eq!(resolve_rows(&s, &[])[0], Resolved { name: "Brave ||".into(), bundle_id: None, missing: true });
    }

    const HOLD: Chord = Chord { ctrl: true, super_: true, alt: true };

    fn table() -> Vec<Shortcut> {
        parse_shortcuts(
            "\"ctrl+super+alt+c\" = \"Claude\"\n\"ctrl+super+alt+h\" = \"Hermes\"\n\"ctrl+super+alt+shift+m\" = \"Gmail\"\n",
        )
        .unwrap()
    }

    fn resolved() -> Vec<Resolved> {
        vec![
            Resolved { name: "Claude".into(), bundle_id: Some("com.anthropic.claudefordesktop".into()), missing: false },
            Resolved { name: "Hermes".into(), bundle_id: None, missing: true },
            Resolved { name: "Gmail".into(), bundle_id: None, missing: false },
        ]
    }

    #[test]
    fn a_binding_row_carries_glyphs_words_icon_and_its_one_word() {
        let rows = binding_rows(&table(), &resolved(), &HashMap::new(), false, Some(HOLD));
        assert_eq!(rows[0].chord, "⇪C");
        assert_eq!(rows[0].spoken, "Claude, Caps + C");
        assert_eq!(rows[0].bundle_id.as_deref(), Some("com.anthropic.claudefordesktop"));
        assert_eq!(rows[0].flag, None);
        assert_eq!(rows[1].flag, Some("missing"));
        assert_eq!(rows[2].chord, "⌃⌥⇧⌘M", "a shift row does not fold");
    }

    #[test]
    fn a_refused_registration_is_in_use_and_outranks_missing() {
        let mut reg = HashMap::new();
        reg.insert("ctrl+super+alt+h".to_string(), Err("taken".to_string()));
        let rows = binding_rows(&table(), &resolved(), &reg, false, Some(HOLD));
        assert_eq!(rows[1].flag, Some("in use"));
    }

    #[test]
    fn a_paused_table_carries_no_words() {
        let rows = binding_rows(&table(), &resolved(), &HashMap::new(), true, Some(HOLD));
        assert!(rows.iter().all(|r| r.flag.is_none()));
    }

    /// A resolve that failed outright leaves an empty cache. The rows still
    /// draw, named as written, and claim nothing.
    #[test]
    fn an_empty_cache_still_draws_every_row_and_claims_nothing() {
        let rows = binding_rows(&table(), &[], &HashMap::new(), false, None);
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[1].name, "Hermes");
        assert!(rows.iter().all(|r| r.flag.is_none() && r.bundle_id.is_none()));
        assert_eq!(rows[0].chord, "⌃⌥⌘C");
    }
```

  If `mod tests` already imports `HashMap` or `Shortcut`, do not import them
  twice. If it does not, add `use std::collections::HashMap;`.

- [ ] **Step 2: Run them and see them fail**

Run: `cargo +stable test -p beckon-cli`
Expected: compile errors for `Resolved`, `resolve_rows`, `binding_rows` and
`BindingRow`.

- [ ] **Step 3: Implement.** Insert above `fn build_entries`:

```rust
/// What config load resolved for one binding (spec §3.5), cached in
/// `ServeState::menu_rows` so the macOS menu never resolves when it opens.
#[cfg(any(target_os = "macos", test))]
#[derive(Debug, Clone, PartialEq, Eq, Default)]
struct Resolved {
    /// The candidate the key will open, as the file spells it. When every
    /// candidate misses, the FIRST one: the app the user thinks of the key as.
    name: String,
    /// The winner's canonical id, for its icon. `None` when nothing resolved.
    bundle_id: Option<String>,
    /// Every candidate is a `NoMatch`, or the chain does not split: the key
    /// will error and launch nothing.
    missing: bool,
}

/// One `Resolved` per binding, in file order, graded by
/// `certainty::chain_winner` -- the rule `check --resolve` uses, so the menu
/// and the check cannot disagree about a key.
#[cfg(any(target_os = "macos", test))]
fn resolve_rows(shortcuts: &[Shortcut], reports: &[NameReport]) -> Vec<Resolved> {
    use beckon_core::certainty::{chain_winner, Certainty};
    let grade: std::collections::HashMap<&str, &NameReport> =
        reports.iter().map(|r| (r.id.as_str(), r)).collect();
    shortcuts
        .iter()
        .map(|s| match beckon_core::candidates::split(&s.app) {
            Err(_) => Resolved {
                name: s.app.clone(),
                bundle_id: None,
                missing: true,
            },
            Ok(cands) => match chain_winner(&cands, |c| grade.get(c).copied()) {
                Some(r) if r.certainty != Certainty::NoMatch => Resolved {
                    name: r.id.clone(),
                    bundle_id: r.target.clone(),
                    missing: false,
                },
                Some(_) => Resolved {
                    name: cands[0].to_string(),
                    bundle_id: None,
                    missing: true,
                },
                // Not answered: unknown, which is not missing.
                None => Resolved {
                    name: cands[0].to_string(),
                    bundle_id: None,
                    missing: false,
                },
            },
        })
        .collect()
}

/// One binding as the macOS menu draws it.
#[cfg(any(target_os = "macos", test))]
#[derive(Debug, Clone, PartialEq, Eq)]
struct BindingRow {
    name: String,
    /// `combo_glyphs`: `⇪C`, `⌃⌥⇧⌘M`.
    chord: String,
    /// The same row in words, for the tooltip and VoiceOver.
    spoken: String,
    flag: Option<&'static str>,
    bundle_id: Option<String>,
}

/// Every binding, in file order, as a menu row.
///
/// `resolved` is indexed like `shortcuts`. A shorter cache -- a resolve that
/// failed outright -- leaves the tail named as written and claiming nothing.
/// `hold` is `settings::caps_view_fold`'s answer.
#[cfg(any(target_os = "macos", test))]
fn binding_rows(
    shortcuts: &[Shortcut],
    resolved: &[Resolved],
    registered: &std::collections::HashMap<String, Result<(), String>>,
    paused: bool,
    hold: Option<beckon_core::shortcuts::Chord>,
) -> Vec<BindingRow> {
    use beckon_core::shortcuts::{combo_display_folded_with, combo_glyphs, ModifierLabels};
    shortcuts
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let canon = s.combo.canonical();
            let r = resolved.get(i);
            let name = r.map_or_else(|| s.app.clone(), |r| r.name.clone());
            let in_use = matches!(registered.get(&canon), Some(Err(_)));
            let missing = r.is_some_and(|r| r.missing);
            BindingRow {
                chord: combo_glyphs(&canon, hold),
                spoken: format!(
                    "{name}, {}",
                    combo_display_folded_with(&canon, hold, ModifierLabels::MAC)
                ),
                flag: if paused {
                    None
                } else {
                    beckon_core::menu::row_flag(in_use, missing)
                },
                bundle_id: r.and_then(|r| r.bundle_id.clone()),
                name,
            }
        })
        .collect()
}
```

  Add `use beckon_core::certainty::NameReport;` at the top of `serve.rs`,
  gated `#[cfg(any(target_os = "macos", test))]` if the Windows cross-clippy
  reports it unused.

- [ ] **Step 4: Run the tests and see them pass**

Run: `cargo +stable test -p beckon-cli`
Expected: the new tests pass, and nothing else changes.

- [ ] **Step 5: Windows still compiles clean**

Run: `cargo +stable clippy --target aarch64-pc-windows-msvc -p beckon-cli --all-targets -- -D warnings`
Expected: clean. The `cfg(any(macos, test))` gates keep these items out of
Windows' non-test build.

- [ ] **Step 6: Commit**

```bash
git branch --show-current
git commit --only crates/beckon-cli/src/serve.rs -m "feat(serve): resolve each binding once for the menu, and read its row"
git show --stat HEAD
```

---

### Task 7: `build_mac_entries` — the menu, composed

**Files:**
- Modify: `crates/beckon-cli/src/serve.rs`, below `build_entries`.
- Modify: `crates/beckon-cli/src/serve.rs` tests. Delete
  `the_macos_menu_says_nothing_it_cannot_do` and
  `the_macos_menu_still_reports_pause_in_its_head_row`, both of which describe
  a function macOS no longer calls, and add the tests below.

**Interfaces:**
- Consumes: `BindingRow` (Task 6); `menu_headline`, `HeadlineInputs`,
  `attention`, `ATTENTION_ROWS`, `EntryKind`, `Header` and `MENU_ID_ALT_CLICK`
  (Tasks 3–4); `menu_log_row`.
- Produces:
  - `struct MacMenu { phrase: String, paused: bool, accessibility: bool, failed: usize, log: Option<bool>, rows: Vec<BindingRow> }`
  - `fn build_mac_entries(m: &MacMenu) -> Vec<MenuEntry>`
  - `const MENU_SHORTCUTS: u32 = 9;`, `const MENU_EDIT_SHORTCUTS: u32 = 10;`,
    `const MENU_BINDING_BASE: u32 = 1000;`
  - The header entry uses id `MENU_STATUS`, and its switch reports that id.
  - A binding row's id is `MENU_BINDING_BASE + index into shortcuts`.

- [ ] **Step 1: Write the failing tests**

```rust
    use beckon_core::menu::EntryKind;

    fn mac(rows: Vec<BindingRow>) -> MacMenu {
        MacMenu {
            phrase: "19 shortcuts registered".into(),
            paused: false,
            accessibility: true,
            failed: 0,
            log: None,
            rows,
        }
    }

    fn brow(name: &str, flag: Option<&'static str>) -> BindingRow {
        BindingRow {
            name: name.into(),
            chord: "⇪X".into(),
            spoken: format!("{name}, Caps + X"),
            flag,
            bundle_id: None,
        }
    }

    fn labels(v: &[MenuEntry]) -> Vec<&str> {
        v.iter().map(|e| e.label.as_str()).collect()
    }

    /// Spec §3.1: nine rows when nothing is broken, whatever the file holds.
    /// That was round 1's objection: 19 inline rows, one more per binding.
    #[test]
    fn the_healthy_macos_menu_is_nine_rows_and_does_not_grow_with_the_file() {
        let small = build_mac_entries(&mac(vec![brow("kitty", None)]));
        let big = build_mac_entries(&mac((0..19).map(|i| brow(&format!("app{i}"), None)).collect()));
        assert_eq!(labels(&small), labels(&big));
        assert_eq!(
            labels(&small),
            vec!["beckon", "", "Shortcuts", "", "Settings...", "Check for Updates...", "Reload Config", "", "Quit beckon"]
        );
    }

    #[test]
    fn the_header_carries_the_headline_and_the_switch_state() {
        let e = build_mac_entries(&MacMenu { paused: true, ..mac(vec![brow("kitty", None)]) });
        let EntryKind::Header(h) = &e[0].kind else { panic!("first row is the header") };
        assert_eq!(e[0].id, MENU_STATUS);
        assert_eq!(h.subtitle, "Paused - shortcuts are off");
        assert!(!h.on, "paused means the switch is off");
    }

    #[test]
    fn needs_attention_appears_only_while_a_row_is_broken() {
        let e = build_mac_entries(&mac(vec![brow("kitty", None), brow("Hermes", Some("missing"))]));
        let i = e.iter().position(|x| x.kind == EntryKind::SectionHeader).expect("section");
        assert_eq!(e[i].label, "Needs attention");
        assert_eq!(e[i + 1].id, MENU_BINDING_BASE + 1);
        assert_eq!(e[i + 1].flag, Some("missing"));
        assert!(e[i + 2].is_separator());
        assert!(build_mac_entries(&mac(vec![brow("kitty", None)]))
            .iter()
            .all(|x| x.kind != EntryKind::SectionHeader));
    }

    #[test]
    fn more_than_three_broken_rows_fold_into_one_more_row_that_opens_the_fourth() {
        let rows = (0..5).map(|i| brow(&format!("app{i}"), Some("missing"))).collect();
        let e = build_mac_entries(&mac(rows));
        let i = e.iter().position(|x| x.kind == EntryKind::SectionHeader).unwrap();
        assert_eq!(e[i + 4].label, "and 2 more...");
        assert_eq!(e[i + 4].id, MENU_BINDING_BASE + 3);
    }

    /// `paused` outranks every word, and the header already says it.
    #[test]
    fn pausing_hides_needs_attention() {
        let e = build_mac_entries(&MacMenu { paused: true, ..mac(vec![brow("Hermes", Some("missing"))]) });
        assert!(e.iter().all(|x| x.kind != EntryKind::SectionHeader));
    }

    #[test]
    fn every_binding_is_in_the_submenu_in_file_order_and_opens_its_own_row() {
        let e = build_mac_entries(&mac(vec![brow("kitty", None), brow("Claude", None)]));
        let sub = e.iter().find(|x| x.id == MENU_SHORTCUTS).unwrap();
        assert_eq!(sub.kind, EntryKind::Submenu);
        assert_eq!(labels(&sub.children), vec!["kitty", "Claude", "", "Edit Shortcuts..."]);
        assert_eq!(sub.children[1].id, MENU_BINDING_BASE + 1);
        assert_eq!(sub.children[1].detail.as_deref(), Some("⇪X"));
        assert_eq!(sub.children[1].tooltip.as_deref(), Some("Claude, Caps + X"));
        assert_eq!(sub.children[3].id, MENU_EDIT_SHORTCUTS);
    }

    #[test]
    fn an_empty_table_still_offers_edit_shortcuts_without_a_stray_separator() {
        let e = build_mac_entries(&mac(vec![]));
        let sub = e.iter().find(|x| x.id == MENU_SHORTCUTS).unwrap();
        assert_eq!(labels(&sub.children), vec!["Edit Shortcuts..."]);
    }

    #[test]
    fn open_log_is_there_only_when_this_run_has_a_log() {
        let without = build_mac_entries(&mac(vec![]));
        assert!(without.iter().all(|x| x.id != MENU_LOG));
        let with = build_mac_entries(&MacMenu { log: Some(true), ..mac(vec![]) });
        let reload = with.iter().position(|x| x.id == MENU_RELOAD).unwrap();
        assert_eq!(with[reload + 1].label, "Open Log");
    }

    #[test]
    fn the_window_rows_keep_their_command_key_equivalents() {
        let e = build_mac_entries(&mac(vec![]));
        let key = |id| e.iter().find(|x| x.id == id).unwrap().key;
        assert_eq!((key(MENU_EDIT), key(MENU_RELOAD), key(MENU_QUIT)), (Some(','), Some('r'), Some('q')));
    }

    fn walk(v: &[MenuEntry], f: &mut dyn FnMut(&[MenuEntry])) {
        f(v);
        for e in v {
            walk(&e.children, f);
        }
    }

    #[test]
    fn no_macos_menu_level_starts_or_ends_on_a_rule_or_doubles_one() {
        for paused in [false, true] {
            for log in [None, Some(true)] {
                for rows in [vec![], vec![brow("a", None)], vec![brow("a", Some("missing"))]] {
                    let e = build_mac_entries(&MacMenu { paused, log, ..mac(rows) });
                    walk(&e, &mut |lvl| {
                        assert!(!lvl.first().unwrap().is_separator());
                        assert!(!lvl.last().unwrap().is_separator());
                        assert!(!lvl.windows(2).any(|w| w[0].is_separator() && w[1].is_separator()));
                    });
                }
            }
        }
    }

    #[test]
    fn no_macos_row_collides_with_a_reserved_id_or_another_row() {
        let rows = (0..25).map(|i| brow(&format!("app{i}"), (i % 2 == 0).then_some("missing"))).collect();
        let e = build_mac_entries(&mac(rows));
        let mut seen = std::collections::HashMap::new();
        walk(&e, &mut |lvl| {
            for x in lvl.iter().filter(|x| !x.is_separator() && x.kind != EntryKind::SectionHeader) {
                assert_ne!(x.id, beckon_core::menu::MENU_ID_DOUBLE_CLICK);
                assert_ne!(x.id, beckon_core::menu::MENU_ID_ALT_CLICK);
                // A binding appears twice (attention + submenu) on purpose;
                // it must be the SAME binding both times.
                // `and N more...` deliberately shares the id of the first
                // binding it folds, so it opens that one.
                if let Some(prev) = seen.insert(x.id, x.label.clone()) {
                    let folded = prev.starts_with("and ") || x.label.starts_with("and ");
                    assert!(prev == x.label || folded, "id {} is two rows", x.id);
                }
            }
        });
    }
```

- [ ] **Step 2: Run them and see them fail**

Run: `cargo +stable test -p beckon-cli`
Expected: compile errors for `MacMenu`, `build_mac_entries`,
`MENU_SHORTCUTS`, `MENU_EDIT_SHORTCUTS` and `MENU_BINDING_BASE`.

- [ ] **Step 3: Implement.** Below `build_entries`:

```rust
#[cfg(any(target_os = "macos", test))]
const MENU_SHORTCUTS: u32 = 9;
#[cfg(any(target_os = "macos", test))]
const MENU_EDIT_SHORTCUTS: u32 = 10;
/// A binding row's id is this plus its index into `ServeState::shortcuts`.
/// Safe because the menu is rebuilt on every open (`menuNeedsUpdate:`), so an
/// id always names a row of the table the menu was built from.
#[cfg(any(target_os = "macos", test))]
const MENU_BINDING_BASE: u32 = 1000;

/// What the macOS menu needs, snapshotted out of `ServeState` like
/// `MenuModel`, so the composition is a pure function.
#[cfg(any(target_os = "macos", test))]
struct MacMenu {
    phrase: String,
    paused: bool,
    accessibility: bool,
    failed: usize,
    /// `menu_log_row`'s answer. Only `Some(true)` draws on macOS.
    log: Option<bool>,
    rows: Vec<BindingRow>,
}

#[cfg(any(target_os = "macos", test))]
fn binding_entry(i: usize, r: &BindingRow) -> MenuEntry {
    MenuEntry {
        id: MENU_BINDING_BASE + i as u32,
        label: r.name.clone(),
        enabled: true,
        detail: Some(r.chord.clone()),
        flag: r.flag,
        icon: r.bundle_id.clone(),
        tooltip: Some(r.spoken.clone()),
        ..MenuEntry::default()
    }
}

/// The macOS menu (spec §3.1). Nine rows when nothing is broken, whatever
/// the file holds; the table itself is a submenu.
#[cfg(any(target_os = "macos", test))]
fn build_mac_entries(m: &MacMenu) -> Vec<MenuEntry> {
    use beckon_core::menu::{
        attention, menu_headline, EntryKind, Header, HeadlineInputs, ATTENTION_ROWS,
    };
    let missing = m.rows.iter().filter(|r| r.flag == Some("missing")).count();
    let head = menu_headline(HeadlineInputs {
        paused: m.paused,
        accessibility: m.accessibility,
        phrase: &m.phrase,
        failed: m.failed,
        missing,
        total: m.rows.len(),
    });
    let mut e = vec![
        MenuEntry {
            id: MENU_STATUS,
            label: "beckon".into(),
            enabled: true,
            kind: EntryKind::Header(Header {
                title: "beckon".into(),
                subtitle: head.text,
                dot: head.dot,
                on: !m.paused,
            }),
            ..MenuEntry::default()
        },
        MenuEntry::separator(),
    ];

    if !m.paused {
        let flags: Vec<Option<&str>> = m.rows.iter().map(|r| r.flag).collect();
        let a = attention(&flags, ATTENTION_ROWS);
        if !a.shown.is_empty() {
            e.push(MenuEntry::section_header("Needs attention"));
            e.extend(a.shown.iter().map(|&i| binding_entry(i, &m.rows[i])));
            if let Some(&first) = a.rest.first() {
                e.push(MenuEntry::item(
                    MENU_BINDING_BASE + first as u32,
                    format!("and {} more...", a.rest.len()),
                ));
            }
            e.push(MenuEntry::separator());
        }
    }

    let mut children: Vec<MenuEntry> = m
        .rows
        .iter()
        .enumerate()
        .map(|(i, r)| binding_entry(i, r))
        .collect();
    if !children.is_empty() {
        children.push(MenuEntry::separator());
    }
    children.push(MenuEntry::item(MENU_EDIT_SHORTCUTS, "Edit Shortcuts..."));
    e.push(MenuEntry {
        id: MENU_SHORTCUTS,
        label: "Shortcuts".into(),
        enabled: true,
        kind: EntryKind::Submenu,
        children,
        ..MenuEntry::default()
    });
    e.push(MenuEntry::separator());

    e.push(MenuEntry {
        key: Some(','),
        ..MenuEntry::item(MENU_EDIT, "Settings...")
    });
    e.push(MenuEntry::item(MENU_UPDATE, beckon_core::menu::update_label(true)));
    e.push(MenuEntry {
        key: Some('r'),
        ..MenuEntry::item(MENU_RELOAD, "Reload Config")
    });
    if m.log == Some(true) {
        e.push(MenuEntry::item(MENU_LOG, "Open Log"));
    }
    e.push(MenuEntry::separator());
    e.push(MenuEntry {
        key: Some('q'),
        ..MenuEntry::item(MENU_QUIT, "Quit beckon")
    });
    e
}
```

- [ ] **Step 4: Do NOT gate `build_entries` yet.** macOS's
  `install_tray_menu` still calls it until Task 10 rewires it, so gating it
  here would break the macOS build for three tasks. That gate is Task 10
  Step 5.

- [ ] **Step 5: Delete the two superseded tests,**
  `the_macos_menu_says_nothing_it_cannot_do` and
  `the_macos_menu_still_reports_pause_in_its_head_row`. Their claims now live
  in `the_healthy_macos_menu_is_nine_rows…`, `open_log_is_there_only…` and
  `the_header_carries_the_headline…`. Keep `the_update_row_appears_next_to_settings`:
  it tests `build_entries`' order and `update_label(true)`'s spelling, and
  both still hold.

- [ ] **Step 6: Run the tests and see them pass**

Run: `cargo +stable test -p beckon-cli`
Expected: everything passes.

- [ ] **Step 7: Windows lints clean**

Run: `cargo +stable clippy --target aarch64-pc-windows-msvc -p beckon-cli --all-targets -- -D warnings`
Expected: clean. On Windows' non-test build every Task 6–7 item is compiled
out.

**Do not expect macOS clippy to be clean until Task 10.** `Resolved`,
`resolve_rows`, `BindingRow`, `binding_rows`, `MacMenu`, `binding_entry`,
`build_mac_entries` and the three constants are dead on macOS's non-test build
until Task 10 wires them. Those warnings are the reminder that the wiring is
still owed. Do not silence them with `allow`.

- [ ] **Step 8: Commit**

```bash
git branch --show-current
git commit --only crates/beckon-cli/src/serve.rs -m "feat(serve): compose the short macOS menu -- header, needs attention, shortcuts submenu"
git show --stat HEAD
```

---

### Task 8: `tray.rs` draws the new kinds

**Files:**
- Modify: `crates/beckon-macos/src/tray.rs`.
- Modify: `crates/beckon-macos/examples/tray_probe.rs`, whose menu becomes a
  sample of every new kind.

**Interfaces:**
- Consumes: `MenuEntry`, `EntryKind`, `Header` and `Dot` (Task 3).
- Produces: no public API change. `set_menu`, `set_status` and
  `request_quit` keep their signatures. Header switches dispatch their
  entry's `id` through the existing `on_click`.

**This task has no unit test.** An `NSMenu` built without a window server
constructs happily and proves nothing (`tray.rs` module doc). Its test is a
person looking, through `tray_probe`, in an Aqua session. `launchctl
managername` must print `Aqua`, and `tray_probe` refuses otherwise.

- [ ] **Step 1: Make `tray_probe` a sample of every kind.** Replace its
  `let build = Box::new(|| { vec![ … ] });` with the block below. It needs a
  `Cell` to show that the switch round-trips:

```rust
        use beckon_core::menu::{Dot, EntryKind, Header};
        use std::cell::Cell;
        use std::rc::Rc;

        let on = Rc::new(Cell::new(true));
        let on_b = Rc::clone(&on);
        let row = |id: u32, name: &str, chord: &str, flag: Option<&'static str>, icon: Option<&str>| MenuEntry {
            id,
            label: name.into(),
            enabled: true,
            detail: Some(chord.into()),
            flag,
            icon: icon.map(Into::into),
            tooltip: Some(format!("{name}, {chord}")),
            ..MenuEntry::default()
        };
        let build = Box::new(move || {
            let on = on_b.get();
            vec![
                MenuEntry {
                    id: 1,
                    label: "beckon".into(),
                    enabled: true,
                    kind: EntryKind::Header(Header {
                        title: "beckon".into(),
                        subtitle: if on { "3 shortcuts, 1 missing".into() } else { "Paused - shortcuts are off".into() },
                        dot: if on { Dot::Ok } else { Dot::Off },
                        on,
                    }),
                    ..MenuEntry::default()
                },
                MenuEntry::separator(),
                MenuEntry::section_header("Needs attention"),
                row(1001, "Hermes", "⇪H", Some("missing"), None),
                MenuEntry::separator(),
                MenuEntry {
                    id: 9,
                    label: "Shortcuts".into(),
                    enabled: true,
                    kind: EntryKind::Submenu,
                    children: vec![
                        row(1000, "Finder", "⇪F", None, Some("com.apple.finder")),
                        row(1001, "Hermes", "⇪H", Some("missing"), None),
                        row(1002, "System Settings", "⌃⌥⇧⌘S", None, Some("com.apple.systempreferences")),
                        MenuEntry::separator(),
                        MenuEntry::item(10, "Edit Shortcuts..."),
                    ],
                    ..MenuEntry::default()
                },
                MenuEntry::separator(),
                MenuEntry { key: Some(','), ..MenuEntry::item(2, "Settings...") },
                MenuEntry::separator(),
                MenuEntry { key: Some('q'), ..MenuEntry::item(4, "Quit beckon") },
            ]
        });
        let on_c = Rc::clone(&on);
        let on_click = Box::new(move |id: u32| {
            println!("menu click: id={id}");
            if id == 1 {
                on_c.set(!on_c.get());
            }
            if id == 4 {
                println!("quitting on request");
                std::process::exit(0);
            }
        });
```

  Delete the old `on_click` that this replaces.

- [ ] **Step 2: Render every kind in `populate`.** In `tray.rs`, make these
  changes.

  **(a) Imports:**

```rust
use beckon_core::menu::{Dot, EntryKind, Header, MenuEntry, MENU_ID_DOUBLE_CLICK};
use objc2_app_kit::{
    NSApplication, NSAttributedStringNSStringDrawing, NSColor, NSControl, NSFont,
    NSFontAttributeName, NSForegroundColorAttributeName, NSImage, NSLayoutAttribute, NSMenu,
    NSMenuDelegate, NSMenuItem, NSMutableParagraphStyle, NSParagraphStyleAttributeName,
    NSStackView, NSStatusBar, NSStatusItem, NSSwitch, NSTextAlignment, NSTextField, NSTextTab,
    NSUserInterfaceLayoutOrientation, NSVariableStatusItemLength, NSView, NSWorkspace,
};
use objc2_foundation::{
    MainThreadMarker, NSArray, NSAttributedString, NSAttributedStringKey, NSData, NSDictionary,
    NSEdgeInsets, NSMutableAttributedString, NSObject, NSObjectProtocol, NSOperatingSystemVersion,
    NSPoint, NSProcessInfo, NSRect, NSSize, NSString,
};
use std::collections::HashMap;
```

  **(b) State.** Add to `struct Tray`:

```rust
    /// App icons by bundle id, fetched once. `iconForFile` goes to disk.
    icons: HashMap<String, Retained<NSImage>>,
    /// The header's live parts, so flipping its switch can update the
    /// subtitle in place while the menu stays open.
    header: Option<HeaderParts>,
```

  and add a new struct:

```rust
struct HeaderParts {
    dot: Retained<NSTextField>,
    subtitle: Retained<NSTextField>,
    switch: Retained<NSSwitch>,
}
```

  In `set_menu`'s `Tray { … }` literal, add `icons: HashMap::new(), header: None,`.

  **(c) One take-and-restore for `build`.** Add this helper and use it in
  `menu_needs_update` in place of its inline copy:

```rust
/// Run `build` with the tray borrow released -- the rule `dispatch` states.
fn built() -> Option<Vec<MenuEntry>> {
    let build = TRAY.with(|t| {
        t.borrow_mut()
            .as_mut()
            .map(|x| std::mem::replace(&mut x.build, Box::new(Vec::new)))
    })?;
    let entries = build();
    TRAY.with(|t| {
        if let Some(x) = t.borrow_mut().as_mut() {
            x.build = build;
        }
    });
    Some(entries)
}
```

  `menu_needs_update` becomes:
  `let Some(entries) = built() else { return }; populate(menu, &entries, self, mtm);`.
  Keep its doc comment.

  **(d) The header switch's action.** Add to `impl MenuTarget` in
  `define_class!`:

```rust
        /// The header's switch. Its tag is the header entry's id.
        #[unsafe(method(beckonControlAction:))]
        fn beckon_control_action(&self, sender: &NSControl) {
            let id = sender.tag() as u32;
            dispatch(id);
            refresh_header();
        }
```

  **(e) `populate`, recursive.** Replace `populate` with the version below,
  plus the helpers after it:

```rust
/// Replace `menu`'s rows with `entries`, recursing into submenus.
fn populate(menu: &NSMenu, entries: &[MenuEntry], target: &MenuTarget, mtm: MainThreadMarker) {
    menu.removeAllItems();
    let tab = tab_stop(entries);
    for e in entries {
        let item = match &e.kind {
            EntryKind::SectionHeader => section_header(&e.label, mtm),
            EntryKind::Header(h) => header_item(e.id, h, target, mtm),
            EntryKind::Submenu => {
                let item = plain_item(e, target, tab, mtm);
                let sub = NSMenu::new(mtm);
                sub.setAutoenablesItems(false);
                populate(&sub, &e.children, target, mtm);
                item.setSubmenu(Some(&sub));
                item
            }
            EntryKind::Item if e.is_separator() => NSMenuItem::separatorItem(mtm),
            EntryKind::Item => plain_item(e, target, tab, mtm),
        };
        menu.addItem(&item);
    }
}

fn plain_item(e: &MenuEntry, target: &MenuTarget, tab: f64, mtm: MainThreadMarker) -> Retained<NSMenuItem> {
    let key = e.key.map(String::from).unwrap_or_default();
    let item = unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            NSMenuItem::alloc(mtm),
            &NSString::from_str(&e.label),
            // A disabled row still gets the action: AppKit decides
            // enablement from `setEnabled:` below, and leaving the selector
            // off would ALSO grey the row, making `enabled` impossible to
            // observe separately.
            Some(sel!(beckonMenuAction:)),
            &NSString::from_str(&key),
        )
    };
    unsafe {
        item.setTarget(Some(target as &AnyObject));
        item.setTag(e.id as isize);
    }
    item.setEnabled(e.enabled);
    if let Some(checked) = e.checked {
        item.setState(if checked { 1 } else { 0 });
    }
    if e.detail.is_some() || e.flag.is_some() {
        item.setAttributedTitle(Some(&row_title(e, tab)));
    }
    // Every binding row gets an image, so the column lines up even when a
    // bundle is unknown -- a gap would read as a rendering fault.
    if e.detail.is_some() {
        let img = e.icon.as_deref().and_then(app_icon).or_else(unknown_app_icon);
        item.setImage(img.as_deref());
    }
    if let Some(t) = &e.tooltip {
        item.setToolTip(Some(&NSString::from_str(t)));
    }
    item
}

/// Label, then the flag in orange, then a tab to a right-aligned chord.
///
/// **The label carries no colour attribute on purpose**, so AppKit still
/// inverts it on the highlighted row. The flag and the chord are coloured and
/// will NOT invert -- that is Probe P2 below.
fn row_title(e: &MenuEntry, tab: f64) -> Retained<NSAttributedString> {
    let font = NSFont::menuFontOfSize(0.0);
    let para = NSMutableParagraphStyle::new();
    let stop = unsafe {
        NSTextTab::initWithTextAlignment_location_options(
            NSTextTab::alloc(),
            NSTextAlignment::Right,
            tab,
            &NSDictionary::new(),
        )
    };
    para.setTabStops(Some(&NSArray::from_retained_slice(&[stop])));
    let out = NSMutableAttributedString::new();
    append(&out, &e.label, &font, None, &para);
    if let Some(f) = e.flag {
        append(&out, &format!("  {f}"), &NSFont::menuFontOfSize(11.0), Some(&NSColor::systemOrangeColor()), &para);
    }
    if let Some(d) = &e.detail {
        append(&out, &format!("\t{d}"), &font, Some(&NSColor::secondaryLabelColor()), &para);
    }
    Retained::into_super(out)
}

fn append(out: &NSMutableAttributedString, text: &str, font: &NSFont, color: Option<&NSColor>, para: &NSMutableParagraphStyle) {
    let mut keys: Vec<&NSAttributedStringKey> = vec![unsafe { NSFontAttributeName }, unsafe { NSParagraphStyleAttributeName }];
    let mut vals: Vec<&AnyObject> = vec![font.as_ref(), para.as_ref()];
    if let Some(c) = color {
        keys.push(unsafe { NSForegroundColorAttributeName });
        vals.push(c.as_ref());
    }
    let attrs = NSDictionary::from_slices(&keys, &vals);
    let piece = unsafe {
        NSAttributedString::initWithString_attributes(NSAttributedString::alloc(), &NSString::from_str(text), Some(&attrs))
    };
    out.appendAttributedString(&piece);
}

/// Where the right-aligned chord column ends: the widest label plus flag,
/// a gap, and the widest chord, all in the menu font. Measured per menu
/// level, so the submenu's column is its own.
fn tab_stop(entries: &[MenuEntry]) -> f64 {
    const GAP: f64 = 28.0;
    let font = NSFont::menuFontOfSize(0.0);
    let small = NSFont::menuFontOfSize(11.0);
    let rows = entries.iter().filter(|e| e.detail.is_some());
    let label = rows
        .clone()
        .map(|e| width(&e.label, &font) + e.flag.map_or(0.0, |f| width(&format!("  {f}"), &small)))
        .fold(0.0, f64::max);
    let chord = rows.filter_map(|e| e.detail.as_deref()).map(|d| width(d, &font)).fold(0.0, f64::max);
    label + GAP + chord
}

fn width(s: &str, font: &NSFont) -> f64 {
    let attrs = NSDictionary::from_slices(&[unsafe { NSFontAttributeName }], &[font.as_ref() as &AnyObject]);
    let a = unsafe { NSAttributedString::initWithString_attributes(NSAttributedString::alloc(), &NSString::from_str(s), Some(&attrs)) };
    a.size().width
}

/// macOS 14's real section header. Below 14 -- `Info.plist` allows 11 -- a
/// disabled row reads as a heading.
fn section_header(title: &str, mtm: MainThreadMarker) -> Retained<NSMenuItem> {
    let t = NSString::from_str(title);
    let v14 = NSOperatingSystemVersion { majorVersion: 14, minorVersion: 0, patchVersion: 0 };
    if NSProcessInfo::processInfo().isOperatingSystemAtLeastVersion(v14) {
        return NSMenuItem::sectionHeaderWithTitle(&t, mtm);
    }
    let item = unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(NSMenuItem::alloc(mtm), &t, None, &NSString::from_str(""))
    };
    item.setEnabled(false);
    item
}

/// A bundle's icon at menu size, cached by id.
fn app_icon(bundle_id: &str) -> Option<Retained<NSImage>> {
    let cached = TRAY.with(|t| t.borrow().as_ref().and_then(|x| x.icons.get(bundle_id).cloned()));
    if cached.is_some() {
        return cached;
    }
    let ws = NSWorkspace::sharedWorkspace();
    let url = ws.URLForApplicationWithBundleIdentifier(&NSString::from_str(bundle_id))?;
    let path = url.path()?;
    let img = ws.iconForFile(&path);
    img.setSize(NSSize::new(16.0, 16.0));
    TRAY.with(|t| {
        if let Some(x) = t.borrow_mut().as_mut() {
            x.icons.insert(bundle_id.to_string(), img.clone());
        }
    });
    Some(img)
}

/// The placeholder for a binding whose app did not resolve. It is a template
/// SF Symbol, so it tints with the menu.
fn unknown_app_icon() -> Option<Retained<NSImage>> {
    let img = NSImage::imageWithSystemSymbolName_accessibilityDescription(&NSString::from_str("app.dashed"), None)?;
    img.setSize(NSSize::new(16.0, 16.0));
    Some(img)
}

fn dot_color(d: Dot) -> Retained<NSColor> {
    match d {
        Dot::Ok => NSColor::systemGreenColor(),
        Dot::Warn => NSColor::systemOrangeColor(),
        Dot::Off => NSColor::tertiaryLabelColor(),
    }
}

const HEADER_WIDTH: f64 = 260.0;

/// The header: dot, title over subtitle, and the switch hard right.
fn header_item(id: u32, h: &Header, target: &MenuTarget, mtm: MainThreadMarker) -> Retained<NSMenuItem> {
    let item = unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(NSMenuItem::alloc(mtm), &NSString::from_str(&h.title), None, &NSString::from_str(""))
    };
    let dot = NSTextField::labelWithString(&NSString::from_str("\u{25CF}"), mtm);
    dot.setTextColor(Some(&dot_color(h.dot)));
    let title = NSTextField::labelWithString(&NSString::from_str(&h.title), mtm);
    title.setFont(Some(&NSFont::boldSystemFontOfSize(13.0)));
    let subtitle = NSTextField::labelWithString(&NSString::from_str(&h.subtitle), mtm);
    subtitle.setFont(Some(&NSFont::systemFontOfSize(11.0)));
    subtitle.setTextColor(Some(&NSColor::secondaryLabelColor()));
    let text = NSStackView::stackViewWithViews(&NSArray::from_slice(&[title.as_ref() as &NSView, subtitle.as_ref()]), mtm);
    text.setOrientation(NSUserInterfaceLayoutOrientation::Vertical);
    text.setAlignment(NSLayoutAttribute::Leading);
    text.setSpacing(1.0);
    // The text column is what stretches, so the switch sits hard right.
    text.setContentHuggingPriority_forOrientation(1.0, objc2_app_kit::NSLayoutConstraintOrientation::Horizontal);
    let switch = NSSwitch::new(mtm);
    switch.setState(if h.on { 1 } else { 0 });
    unsafe {
        switch.setTarget(Some(target as &AnyObject));
        switch.setAction(Some(sel!(beckonControlAction:)));
    }
    switch.setTag(id as isize);
    switch.setAccessibilityLabel(Some(&NSString::from_str("Shortcuts on")));
    let row = NSStackView::stackViewWithViews(&NSArray::from_slice(&[dot.as_ref() as &NSView, text.as_ref(), switch.as_ref()]), mtm);
    row.setOrientation(NSUserInterfaceLayoutOrientation::Horizontal);
    row.setSpacing(8.0);
    row.setEdgeInsets(NSEdgeInsets { top: 6.0, left: 14.0, bottom: 6.0, right: 14.0 });
    row.setFrame(NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(HEADER_WIDTH, 44.0)));
    item.setView(Some(&row));
    TRAY.with(|t| {
        if let Some(x) = t.borrow_mut().as_mut() {
            x.header = Some(HeaderParts { dot, subtitle, switch });
        }
    });
    item
}

/// After the switch was flipped: rebuild, and move the header's three parts
/// to what the rebuilt header says. The rest of the open menu stays as drawn
/// until the next open, which is when `menuNeedsUpdate:` rebuilds it anyway.
fn refresh_header() {
    let Some(entries) = built() else { return };
    let Some(h) = entries.iter().find_map(|e| match &e.kind {
        EntryKind::Header(h) => Some(h.clone()),
        _ => None,
    }) else {
        return;
    };
    let parts = TRAY.with(|t| {
        t.borrow().as_ref().and_then(|x| x.header.as_ref().map(|p| (p.dot.clone(), p.subtitle.clone(), p.switch.clone())))
    });
    if let Some((dot, subtitle, switch)) = parts {
        dot.setTextColor(Some(&dot_color(h.dot)));
        subtitle.setStringValue(&NSString::from_str(&h.subtitle));
        switch.setState(if h.on { 1 } else { 0 });
    }
}
```

  The exact `objc2` spellings to check against `objc2-app-kit` 0.3.2 as you
  compile:
  - which calls need an `unsafe` block;
  - `NSDictionary::from_slices`;
  - whether the attribute-name statics need `unsafe` to read;
  - `setContentHuggingPriority_forOrientation`'s priority type.

  Fix them to match the generated signatures. Do not change the design.

- [ ] **Step 3: Compile everything**

Run: `cargo +stable clippy -p beckon-macos --all-targets -- -D warnings`
Expected: clean.

- [ ] **Step 4: Probe P1–P4, by eye.** Build into a PRIVATE target dir and
  run it twice (the first exec may be killed):

```bash
CARGO_TARGET_DIR=/private/tmp/beckon-phase1 cargo +stable build -p beckon-macos --example tray_probe
/private/tmp/beckon-phase1/debug/examples/tray_probe nsapp || /private/tmp/beckon-phase1/debug/examples/tray_probe nsapp
```

  Then ask the person at the machine to open the probe's icon and answer each
  of these, and record the answers verbatim for Task 11:

  | probe | question | if NO |
  |---|---|---|
  | P1 | Does flipping the header switch leave the menu open, flip the subtitle to *Paused - shortcuts are off*, and grey the dot? | Close the menu from `beckon_control_action` with `menu.cancelTracking()` after `dispatch`, and drop `refresh_header`. |
  | P2 | In *Shortcuts ▸*, are the chords right-aligned in one column, and is a highlighted row still legible (label white; chord and flag readable)? | If the column is ragged, add 8 pt to `GAP`. If a highlighted chord is unreadable, drop the chord's colour attribute. |
  | P3 | Does *Needs attention* draw as a small grey section title (macOS 14+)? | Record the OS. The fallback row is expected below 14. |
  | P4 | Does Finder show its real icon, System Settings its icon, and Hermes a dashed placeholder, all 16 pt and aligned? | Check `URLForApplicationWithBundleIdentifier` for that id with `mdfind kMDItemCFBundleIdentifier=<id>`. |

  **Control:** before relying on a YES for P4, confirm that the placeholder
  path is really exercised. Hermes must show the dashed symbol, not a blank
  gap. A blank gap and a working icon look alike from a distance.

- [ ] **Step 5: Commit**

```bash
git branch --show-current
git commit --only crates/beckon-macos/src/tray.rs crates/beckon-macos/examples/tray_probe.rs \
  -m "feat(macos): the menu bar draws headers, sections, submenus, icons and chords"
git show --stat HEAD
```

---

### Task 9: ⌥-click pauses; the icon dims while paused

**Files:**
- Modify: `crates/beckon-macos/src/tray.rs`.

**Interfaces:**
- Consumes: `MENU_ID_ALT_CLICK` (Task 3).
- Produces:
  - `pub fn set_dimmed(dimmed: bool)` in `beckon_macos::tray`.
  - ⌥-click dispatches `MENU_ID_ALT_CLICK` to `on_click` and never opens the
    menu. Any other click, including a right-click, opens the menu as
    before.

- [ ] **Step 1: Keep the menu, and stop attaching it permanently.** In
  `struct Tray`, rename `_item` to `item`. It is now read in two new places,
  so update `set_status`'s `x._item` accordingly. Also add:

```rust
    /// The menu, shown on demand by `pop_menu` rather than attached for
    /// good -- attached, AppKit opens it on every click and no modifier can
    /// be seen (spec §3.3).
    menu: Retained<NSMenu>,
```

  In `set_menu`:
  - delete `item.setMenu(Some(&menu));`;
  - store `menu` in the `Tray` literal;
  - after the image is set, route clicks to the target:

```rust
    unsafe {
        button.setTarget(Some(&*target as &AnyObject));
        button.setAction(Some(sel!(beckonStatusClick:)));
    }
    let _ = button.sendActionOn(
        objc2_app_kit::NSEventMask::LeftMouseUp | objc2_app_kit::NSEventMask::RightMouseUp,
    );
```

- [ ] **Step 2: The click handler.** Add to `impl MenuTarget`:

```rust
        /// Every click on the icon. ⌥ + left click is the pause toggle and
        /// opens nothing; anything else opens the menu.
        #[unsafe(method(beckonStatusClick:))]
        fn beckon_status_click(&self, _sender: &AnyObject) {
            let Some(mtm) = MainThreadMarker::new() else { return };
            let ev = NSApplication::sharedApplication(mtm).currentEvent();
            let option = ev
                .as_ref()
                .is_some_and(|e| e.modifierFlags().contains(objc2_app_kit::NSEventModifierFlags::Option));
            let right = ev
                .as_ref()
                .is_some_and(|e| e.r#type() == objc2_app_kit::NSEventType::RightMouseUp);
            if option && !right {
                dispatch(beckon_core::menu::MENU_ID_ALT_CLICK);
                return;
            }
            pop_menu(mtm);
        }
```

  And the free function:

```rust
/// Show the menu under the icon. Attaching it for the length of one
/// `performClick` is what gives it the system's own placement and highlight;
/// detaching it again is what lets the next click reach
/// `beckonStatusClick:`.
fn pop_menu(mtm: MainThreadMarker) {
    let parts = TRAY.with(|t| t.borrow().as_ref().map(|x| (x.item.clone(), x.menu.clone())));
    let Some((item, menu)) = parts else { return };
    item.setMenu(Some(&menu));
    if let Some(b) = item.button(mtm) {
        unsafe { b.performClick(None) };
    }
    item.setMenu(None);
}
```

- [ ] **Step 3: `set_dimmed`.** Add a public function next to `set_status`:

```rust
/// Dim the icon while paused, the way Maccy does, and say so to VoiceOver.
/// ASCII, like every other display string here.
pub fn set_dimmed(dimmed: bool) {
    let Some(mtm) = MainThreadMarker::new() else { return };
    let item = TRAY.with(|t| t.borrow().as_ref().map(|x| x.item.clone()));
    if let Some(button) = item.and_then(|i| i.button(mtm)) {
        button.setAppearsDisabled(dimmed);
        button.setAccessibilityLabel(Some(&NSString::from_str(if dimmed { "beckon - paused" } else { "beckon" })));
    }
}
```

- [ ] **Step 4: Compile**

Run: `cargo +stable clippy -p beckon-macos --all-targets -- -D warnings`
Expected: clean.

- [ ] **Step 5: Probe P5–P6, by eye.** In `tray_probe`, add
  `let dimmed = Rc::new(Cell::new(false)); let dimmed_c = Rc::clone(&dimmed);`
  before `on_click`. Move `dimmed_c` into it, and add this as its first
  statement:

```rust
            if id == beckon_core::menu::MENU_ID_ALT_CLICK {
                println!("alt-click");
                dimmed_c.set(!dimmed_c.get());
                tray::set_dimmed(dimmed_c.get());
                return;
            }
```

  Rebuild into the private target dir, run it twice, and ask:

  | probe | question | if NO |
  |---|---|---|
  | P5 | Does a plain click open the menu, placed and highlighted exactly as before this task? Does a right-click open it too? | Revert Steps 1–2 to `setMenu`, and use `menuWillOpen:` with `NSEvent::modifierFlags_class()` + `menu.cancelTracking()` for ⌥ instead. Record which one shipped. |
  | P6 | Does ⌥-click print `alt-click`, open NO menu, and dim the icon? Does a second ⌥-click undim it? | Record it. P6 failing with P5 passing means the modifier read is wrong, not the routing. |

  **Control for P6:** a plain click must NOT print `alt-click`. Without that,
  a handler that always dispatches would pass.

- [ ] **Step 6: Commit**

```bash
git branch --show-current
git commit --only crates/beckon-macos/src/tray.rs crates/beckon-macos/examples/tray_probe.rs \
  -m "feat(macos): option-click the menu bar icon to pause; it dims while paused"
git show --stat HEAD
```

---

### Task 10: `serve` wires the macOS menu

**Files:**
- Modify: `crates/beckon-cli/src/serve.rs`, in these places:
  - `ServeState`: the fields after `pending_update_check` (around line 368),
    and their initialisers after `pending_update_check: false,` (around
    line 497);
  - the startup call before `install_tray_menu(&state, &mgr);` (around line
    597);
  - `reload`'s `Ok` arm (lines 1078–1082);
  - `set_paused` (around line 2563);
  - `open_settings` (around line 2176), and the fresh-open path after
    `load_settings_model`;
  - the macOS `install_tray_menu` (lines 1357–1432).

**Interfaces:**
- Consumes: everything above, plus `beckon_macos::resolve_reports`,
  `beckon_macos::is_accessibility_trusted`, `beckon_macos::prefs::caps_view`,
  `beckon_core::settings::caps_view_fold`, `Model::row_for_combo` and
  `tray::set_dimmed`.
- Produces: the shipped behaviour. No new public API.

- [ ] **Step 1: State.** Add to `struct ServeState`, after
  `pending_update_check`:

```rust
    /// Each binding's resolution, for the macOS menu's names, icons and its
    /// `missing` word (spec §3.5). Filled by `refresh_menu_rows` at startup
    /// and on every reload -- **never when the menu opens**.
    #[cfg(target_os = "macos")]
    menu_rows: Vec<Resolved>,
    /// A menu row asked Settings to open on this binding: an index into
    /// `shortcuts`. Consumed by `open_settings`, like `pending_update_check`.
    pending_select: Option<usize>,
```

  Initialise them after `pending_update_check: false,`:

```rust
        #[cfg(target_os = "macos")]
        menu_rows: Vec::new(),
        pending_select: None,
```

  Also remove the `#[cfg_attr(not(target_os = "windows"), allow(dead_code))]`
  on `keyboard` and on `registered`. macOS reads both now, from the menu's
  build closure. Correct their two stale sentences ("only Windows acts on
  them" / "only read by the Windows-only window") to say macOS's menu reads
  them too.

- [ ] **Step 2: Resolve at load and at reload, never at open.** Add:

```rust
/// Re-resolve every binding for the macOS menu (spec §3.5): one batch, one
/// catalog scan, on config load and reload -- the same call `check
/// --resolve` makes. A failure leaves an empty cache, which `binding_rows`
/// draws as rows named as written that claim nothing.
#[cfg(target_os = "macos")]
fn refresh_menu_rows(state: &Rc<RefCell<ServeState>>) {
    let started = std::time::Instant::now();
    // Cloned out so no borrow is held across the resolver.
    let shortcuts = state.borrow().shortcuts.clone();
    let mut names: Vec<&str> = shortcuts
        .iter()
        .filter_map(|s| beckon_core::candidates::split(&s.app).ok())
        .flatten()
        .collect();
    names.sort_unstable();
    names.dedup();
    let rows = match beckon_macos::resolve_reports(&names) {
        Ok(reports) => resolve_rows(&shortcuts, &reports),
        Err(e) => {
            eprintln!("beckon serve: menu rows not resolved ({e}); names shown as written");
            Vec::new()
        }
    };
    if beckon_core::verbose() {
        eprintln!("beckon serve: resolved {} menu rows in {:?}", rows.len(), started.elapsed());
    }
    state.borrow_mut().menu_rows = rows;
}
```

  Call it at startup, immediately before `install_tray_menu(&state, &mgr);`:

```rust
    #[cfg(target_os = "macos")]
    refresh_menu_rows(&state);
```

  And in `reload`'s `Ok` arm, immediately after the block that sets
  `s.shortcuts = new.shortcuts; s.keyboard = new.keyboard;` and before
  `let paused = …`:

```rust
            #[cfg(target_os = "macos")]
            refresh_menu_rows(state);
```

- [ ] **Step 3: Dim on pause.** Add next to `set_tray_status`:

```rust
#[cfg(target_os = "macos")]
fn set_tray_dimmed(paused: bool) {
    beckon_macos::tray::set_dimmed(paused);
}
#[cfg(not(target_os = "macos"))]
fn set_tray_dimmed(_paused: bool) {}
```

  In `set_paused`, add `set_tray_dimmed(true);` after the paused arm's
  `set_tray_status(…)`, and `set_tray_dimmed(false);` after the resumed arm's.

- [ ] **Step 4: Open Settings on a binding.** Add the helper, gated
  `#[cfg(any(target_os = "windows", target_os = "macos"))]`:

```rust
/// Point the settings model's selection at `shortcuts[i]`, found by CHORD
/// rather than position, and drop any probe verdict -- a verdict is about the
/// row it was asked for (`Callbacks::on_select`'s rule).
#[cfg(any(target_os = "windows", target_os = "macos"))]
fn select_binding(state: &Rc<RefCell<ServeState>>, i: usize) {
    let mut s = state.borrow_mut();
    let Some(canon) = s.shortcuts.get(i).map(|x| x.combo.canonical()) else {
        return;
    };
    if let Some(m) = s.settings.as_mut() {
        if let Some(row) = m.row_for_combo(&canon) {
            m.selected = Some(row);
        }
    }
    s.probe = None;
}
```

  In `open_settings`, directly after the `wanted_check` block:

```rust
    // A menu row asked for its own binding. Hoisted out of any `if` for the
    // reason `wanted_check` is.
    let wanted_row = state.borrow_mut().pending_select.take();
    if wanted_row.is_some() {
        state.borrow_mut().settings_page = beckon_core::settings::Page::Shortcuts;
    }
```

  In the already-open branch, before its `if wanted_check { … }`:

```rust
        // Selection reaches an open window through the model; the PAGE does
        // not, for the same known limitation the comment above records.
        if let Some(i) = wanted_row {
            select_binding(state, i);
            refresh_settings(state);
        }
```

  And right after the `if let Err(e) = load_settings_model(state) { … }`
  block:

```rust
    if let Some(i) = wanted_row {
        select_binding(state, i);
    }
```

- [ ] **Step 5: The macOS `install_tray_menu`.** Replace its `build` closure:

```rust
    let st_build = Rc::clone(state);
    let build = Box::new(move || {
        let s = st_build.borrow();
        let hold = beckon_core::settings::caps_view_fold(
            beckon_macos::prefs::caps_view(),
            s.keyboard.caps,
            s.keyboard.caps_hold,
        );
        build_mac_entries(&MacMenu {
            phrase: s.last_phrase.clone(),
            paused: s.paused,
            // Read on every open, never cached: every rebuild of the binary
            // invalidates the grant (spec §6).
            accessibility: beckon_macos::is_accessibility_trusted(),
            failed: s.registered.values().filter(|r| r.is_err()).count(),
            log: menu_log_row(s.log.is_some(), cfg!(target_os = "windows")),
            rows: binding_rows(&s.shortcuts, &s.menu_rows, &s.registered, s.paused, hold),
        })
    });
```

  In `on_click`, merge the pause arm and add three arms. The binding arm goes
  last, before `_ => {}`:

```rust
        MENU_EDIT_SHORTCUTS => {
            st.borrow_mut().settings_page = beckon_core::settings::Page::Shortcuts;
            open_settings(&st, &mg);
        }
        // Three doors, one toggle: the header's switch (it carries the
        // header's id), the old checkbox id, and option-click on the icon.
        MENU_STATUS | MENU_PAUSE | beckon_core::menu::MENU_ID_ALT_CLICK => {
            let now = !st.borrow().paused;
            set_paused(&st, &mg, now);
            refresh_settings(&st);
        }
        // A binding row, in the attention section or the submenu. It opens
        // Settings on that binding and NEVER launches it.
        id if (MENU_BINDING_BASE..beckon_core::menu::MENU_ID_ALT_CLICK).contains(&id) => {
            st.borrow_mut().pending_select = Some((id - MENU_BINDING_BASE) as usize);
            open_settings(&st, &mg);
        }
```

  Replace the doc comment above the function. The stale "Five rows against
  Windows' eight" becomes:

```rust
/// The macOS menu bar item: `build_mac_entries`' short menu (spec
/// `2026-09-22-macos-ui-redesign-design.md` §3). Failure is logged and
/// swallowed: hotkeys are the feature and this is the control surface, so
/// losing the icon must not take the daemon with it -- the same rule Windows
/// applies to a config that will not parse (`BrokenConfig::ServeAnyway`).
```

  Now that macOS no longer calls it, give `build_entries` **and** `MenuModel`
  `#[cfg(any(target_os = "windows", test))]`. macOS test builds still
  exercise both, including `update_label(true)`'s spelling test, and macOS's
  non-test build stops carrying dead code. Leave `menu_log_row` ungated; both
  platforms use it.

  `MENU_AUTOSTART` is then read only by `build_entries` and Windows'
  `on_click`, so give it the same `#[cfg(any(target_os = "windows", test))]`.
  Do the same for any other item that the macOS clippy run in Step 6 reports
  as unused because of this gate. Never add `allow(dead_code)` to silence one.

- [ ] **Step 6: The full gate**

```bash
export CARGO_TARGET_DIR=~/Documents/dev/beckon/target
cargo +stable fmt --all -- --check
cargo +stable clippy --workspace --all-targets -- -D warnings
cargo +stable clippy --target aarch64-pc-windows-msvc --all-targets -- -D warnings
cargo +stable check --workspace --all-targets
cargo +stable test --workspace
```

Expected: all clean. `serve.rs`'s tests run on this host, so a failure in
`build_entries`' tests here is a Windows regression.

- [ ] **Step 7: Commit**

```bash
git branch --show-current
git commit --only crates/beckon-cli/src/serve.rs -m "feat(serve): the macOS menu bar shows the short menu, resolves at load, opens a binding in Settings"
git show --stat HEAD
```

---

### Task 11: live check, notes, and push

**Files:**
- Modify: `docs/notes/macos-backend.md`, adding a section `## The menu bar
  menu (2026-09-22 redesign, phase 1)`.
- Modify: `CLAUDE.md`, in two places named below.

- [ ] **Step 1: Run the real `serve` in this Aqua session.** The installed
  one is a brew service, and holds the config lock.

```bash
brew services stop beckon
CARGO_TARGET_DIR=/private/tmp/beckon-phase1 cargo +stable build -p beckon-cli --bin beckon
B=/private/tmp/beckon-phase1/debug/beckon
$B --version || $B --version            # first exec may be killed
$B -v serve ~/.config/beckon/apps.toml 2>&1 | tee /private/tmp/beckon-phase1/serve.log
```

  This fresh binary has **no Accessibility grant**. That makes it the live
  check of the headline's rung 2 for free: the header must read *Needs
  Accessibility to switch windows* on an orange dot. Ask the person to check,
  and record the answers:

  | check | expected |
  |---|---|
  | L1 | The header is orange, *Needs Accessibility to switch windows* (this binary has no grant). |
  | L2 | *Needs attention* lists `com.nousresearch.hermes` and `Tao Monitor`, both `missing`. This matches `beckon check --resolve` on airm3, 2026-09-22. |
  | L3 | *Shortcuts ▸* has all 19 rows in file order. Folded chords (`⇪C`) show when Settings > Keyboard's shorthand is on; `⌃⌥⌘C` shows otherwise. The four `+shift` rows stay long. |
  | L4 | Clicking *Claude* in the submenu opens Settings on Shortcuts with the Claude row selected. **Claude does not launch or focus.** |
  | L5 | The header switch pauses: the icon dims, the subtitle reads *Paused - shortcuts are off*, and ⇪C does nothing. Switching it back makes ⇪C work again. |
  | L6 | ⌥-click on the icon toggles pause without opening the menu. |
  | L7 | Saving `apps.toml` (touch it) reloads, and the log shows `resolved 19 menu rows in …`. Record the duration. |

  **Control for L4:** before clicking, make sure Claude is NOT frontmost, so a
  launch or focus would be visible.

  **Control for L2:** confirm that a healthy row, for example `kitty`, is
  absent from *Needs attention*. A section listing every row would pass the
  L2 read.

  Then restore the service:

```bash
# Ctrl-C the dev serve first
brew services start beckon
```

- [ ] **Step 2: Write the note.** Record in `docs/notes/macos-backend.md`:
  - P1–P6 and L1–L7, verbatim, with the date, `airm3`, and the macOS version
    from `sw_vers -productVersion`;
  - which ⌥-click mechanism shipped (P5);
  - the L7 resolve duration;
  - one paragraph on why the menu resolves at load and not at open (spec
    §3.5).

  Follow the file's existing voice: **what was measured, where, and what it
  does not prove**.

- [ ] **Step 3: `CLAUDE.md`.**
  - In "Out of scope → GUI / TUI", the sentence listing the tray menu's
    contents gains: *"and, on macOS, the whole table as a `Shortcuts`
    submenu plus a `Needs attention` section that exists only while a
    binding is broken -- every row opens Settings on its binding, never the
    app"*.
  - In "What beckon reads and writes", after the Windows registry paragraph,
    add the macOS store that the spec found undocumented:
    *"**macOS keeps the same kind of store in `NSUserDefaults`**, domain
    `com.xom11.beckon` (`crates/beckon-macos/src/prefs.rs`): `Opacity` and
    `CapsView`. As on Windows, that list is the reset list."*

- [ ] **Step 4: Commit the docs, then push**

```bash
git branch --show-current
git commit --only docs/notes/macos-backend.md CLAUDE.md -m "docs: the macOS menu bar menu, measured"
git show --stat HEAD
git push -u origin macos-menu-phase-1
git ls-remote --heads origin macos-menu-phase-1
```

- [ ] **Step 5: Hand back.** Report to the user in Vietnamese:
  - what shipped;
  - P1–P6 and L1–L7, including any NO and the fallback taken;
  - whether CI is green.

  Then offer the fast-forward merge. The standing authorization covers the
  merge and push once they agree, and cleanup follows `CLAUDE.md`'s order.
