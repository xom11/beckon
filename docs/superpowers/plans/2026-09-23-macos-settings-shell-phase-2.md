# macOS Settings window shell, phase 2 — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the macOS Settings window's shell with the one spec §5.1-5.2
describes: an `NSToolbar` in `.preference` style instead of the
`NSSegmentedControl` tab strip, the open page's name as the window title, a
window that sizes itself to the page it is showing, and pages laid out as
grouped forms.

**Architecture:**
- **What each page DOES is untouched.** Every control, callback, selector and
  id stays exactly as it is; this phase moves them into different containers
  and changes what the window chrome looks like. The Shortcuts page's table
  and editor are phase 4's and are not reshaped here.
- **The one decision that belongs in core is the page's NAME** (`System` on
  Windows, `General` on macOS), as a per-platform table beside
  `ModifierLabels`. Everything else in this phase is drawing.
- **Two new layout primitives** in `widgets.rs` (`group`, `form_row`) express
  §5.2 once, and the three settings pages are rewritten in terms of them.

**Tech Stack:** Rust 2021, `objc2` 0.6 / `objc2-app-kit` 0.3.2 /
`objc2-foundation` 0.3.2.

**Spec:** `docs/superpowers/specs/2026-09-22-macos-ui-redesign-design.md`,
§5.1 and §5.2 (read both before Task 2), with §5.5's auto-save explicitly NOT
in this phase.

## Global Constraints

- **Behaviour is preserved.** No callback, selector, control id, enable/hide
  rule or `SettingsCommand` changes in this phase. If a rewrite tempts you to
  change what a row does, stop and report it.
- **The Shortcuts page's table, editor, banner and command bar are not
  reshaped.** They move into the new shell unchanged (phase 4 owns them).
- **`command_bar_shown(page)` still decides Save / Close / Open config file**,
  and the band itself still shows on all four pages (`show_page`,
  `mod.rs:608-611`).
- **Colours stay semantic `NSColor`s** — no Dark mode row, no stored theme
  (spec §5.2, and the note in `docs/notes/settings-window.md`).
- **Labels are ASCII**: `...` not `…`, `-` not `—`.
- **Do NOT add objc2 features to `crates/beckon-macos/Cargo.toml`.** Phase 1
  established that `objc2-app-kit` 0.3.2's `default` set already contains
  every class feature and this crate never sets `default-features = false`;
  the explicit list there is decorative. `NSToolbar` / `NSToolbarItem` are
  absent from that list and must still compile. **If they do not, that
  refutes the phase-1 finding** — record it in your report and in
  `docs/notes/macos-backend.md` rather than quietly adding the feature.
- **No `allow(dead_code)`**, ever, to silence a gate.
- **Windows is untouched.** Core changes keep their `WINDOWS` constant
  byte-identical, and `beckon-windows` must not need an edit.
- **Borrow discipline:** no `RefCell` borrow of `UI`/`CB` held across an
  AppKit call that can re-enter; `apply_state`'s PHASE 1 / PHASE 2 split
  (`mod.rs:2435`, `:2463`) is the pattern, and its comment says why.
- **Git:** `git branch --show-current` before every commit; `git commit --only
  <paths>`; verify with `git show --stat HEAD`. Do not push.
- **Toolchain:** `cargo +stable` for every gate (CI's stable is 1.98.1; this
  machine's default is older).
- **Target dirs:** `CARGO_TARGET_DIR=~/Documents/dev/beckon/target` for
  check/clippy/fmt/test; a private dir for anything you run.
- **You do not open the GUI.** Implementers compile, lint, test and commit.
  The controller drives the real window (AppleScript + `screencapture`) and
  reports what it sees. Say in your report what you expect it to look like.

## The gate (every task ends with all of it clean)

```bash
export CARGO_TARGET_DIR=~/Documents/dev/beckon/target
cargo +stable fmt --all -- --check
cargo +stable clippy --workspace --all-targets -- -D warnings
cargo +stable clippy --target aarch64-pc-windows-msvc --all-targets -- -D warnings
cargo +stable check --workspace --all-targets
cargo +stable test --workspace
```

---

## Before you start

- [ ] **Workspace.** The branch and worktree already exist:
  `/Users/kln/Documents/dev/beckon/.worktrees/macos-settings-shell`, branch
  `macos-settings-shell`, forked from `origin/main` (`09d0a93`). Confirm with
  `git branch --show-current`, and look for company:
  `git worktree list`, `git branch -a`, `git log --all --oneline -10`.
- [ ] **Read the shell you are replacing** before Task 2:
  `crates/beckon-macos/src/settings_window/mod.rs` — `open()` (`:1528`), the
  tab strip (`:1865-1883`), `show_page` (`:588-615`), `apply_state`
  (`:2431`), the root stack (`:1886-1926`), and the `MIN_HEIGHT` doc
  (`:130-159`).

---

### Task 1: the page's name is a per-platform table in core

**Files:**
- Modify: `crates/beckon-core/src/settings.rs` — add beside `Page`'s impl
  (`:430-490`).
- Test: the same file's `mod tests`.

**Interfaces:**
- Produces:
  - `pub struct PageLabels { pub shortcuts: &'static str, pub keyboard: &'static str, pub system: &'static str, pub about: &'static str }`
  - `PageLabels::WINDOWS` = `{"Shortcuts", "Keyboard", "System", "About"}`
  - `PageLabels::MAC` = the same but `system: "General"`
  - `pub fn page_label(page: Page, l: PageLabels) -> &'static str`
  Task 2 uses `page_label(p, PageLabels::MAC)`.

- [ ] **Step 1: Write the failing tests.** In `settings.rs`'s `mod tests`:

```rust
    /// The four captions were literals in each window. They are a table for
    /// the reason `ModifierLabels` is one: only ONE of them differs by
    /// platform, and a reader must be able to see which.
    #[test]
    fn only_the_system_page_is_named_differently_on_macos() {
        let w = PageLabels::WINDOWS;
        let m = PageLabels::MAC;
        assert_eq!((w.shortcuts, w.keyboard, w.about), (m.shortcuts, m.keyboard, m.about));
        assert_eq!(w.system, "System");
        assert_eq!(m.system, "General");
    }

    #[test]
    fn page_label_reads_the_table_it_is_given() {
        for (p, w, m) in [
            (Page::Shortcuts, "Shortcuts", "Shortcuts"),
            (Page::Keyboard, "Keyboard", "Keyboard"),
            (Page::System, "System", "General"),
            (Page::About, "About", "About"),
        ] {
            assert_eq!(page_label(p, PageLabels::WINDOWS), w);
            assert_eq!(page_label(p, PageLabels::MAC), m);
        }
    }

    #[test]
    fn every_page_label_is_ascii() {
        for l in [PageLabels::WINDOWS, PageLabels::MAC] {
            for p in [Page::Shortcuts, Page::Keyboard, Page::System, Page::About] {
                assert!(page_label(p, l).is_ascii());
            }
        }
    }
```

- [ ] **Step 2: Run them and see them fail**

Run: `cargo +stable test -p beckon-core page_label`
Expected: compile error, `cannot find type PageLabels`.

- [ ] **Step 3: Implement.** After `impl Page`'s closing brace:

```rust
/// What each door is CALLED on a given platform.
///
/// The same shape as `shortcuts::ModifierLabels`, and for the same reason:
/// exactly one of the four differs, so a table shows which one at a glance
/// where four literals in two windows would not. `Page` itself is unchanged
/// -- this is display text, not identity.
///
/// **`System` is `General` on macOS** because that is what the platform calls
/// the door holding "everything that is not a document" (spec
/// `2026-09-22-macos-ui-redesign-design.md` §5.1). Windows keeps `System`,
/// and `WINDOWS` below is byte-identical to the literals its window already
/// draws.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PageLabels {
    pub shortcuts: &'static str,
    pub keyboard: &'static str,
    pub system: &'static str,
    pub about: &'static str,
}

impl PageLabels {
    /// What the Win32 window has always drawn.
    pub const WINDOWS: PageLabels = PageLabels {
        shortcuts: "Shortcuts",
        keyboard: "Keyboard",
        system: "System",
        about: "About",
    };

    /// macOS, where the third door is `General`.
    pub const MAC: PageLabels = PageLabels {
        shortcuts: "Shortcuts",
        keyboard: "Keyboard",
        system: "General",
        about: "About",
    };
}

/// One door's caption, in a named platform's words.
pub fn page_label(page: Page, l: PageLabels) -> &'static str {
    match page {
        Page::Shortcuts => l.shortcuts,
        Page::Keyboard => l.keyboard,
        Page::System => l.system,
        Page::About => l.about,
    }
}
```

- [ ] **Step 4: Run the tests and see them pass**

Run: `cargo +stable test -p beckon-core page_label && cargo +stable test -p beckon-core`
Expected: the three new tests pass; the crate's whole suite stays green.

- [ ] **Step 5: Commit**

```bash
git branch --show-current
git commit --only crates/beckon-core/src/settings.rs -m "feat(core): the four door captions are a per-platform table"
git show --stat HEAD
```

---

### Task 2: an `NSToolbar` in preference style replaces the tab strip

**Files:**
- Modify: `crates/beckon-macos/src/settings_window/mod.rs` — the tab strip
  (`:1865-1883`), the root stack (`:1913-1925`), `show_page` (`:588-615`),
  `on_page` (`:874-882`), `apply_state`'s segment-label write (`:2495-2501`),
  `Controls`' `tabs` field (`:181+`), the window build (`:1928-1987`).
- Delete: `shortcuts_tab_label` (`mod.rs:268`) and the two tests that pin it
  (`the_shortcuts_tab_carries_the_warning_when_the_file_moved`,
  `the_tab_mark_is_the_complement_of_the_banner` — see Step 6 before deleting
  the second).

**Interfaces:**
- Consumes: `beckon_core::settings::{page_label, PageLabels, Page}` (Task 1).
- Produces, inside `mod.rs`:
  - `struct ToolbarTarget` (an `NSObject` subclass implementing
    `NSToolbarDelegate`), stored in `Controls` so the delegate stays alive —
    `NSToolbar`'s delegate reference is weak, exactly like `NSMenu`'s in
    `tray.rs`.
  - `fn page_identifier(p: Page) -> &'static str` and
    `fn page_from_identifier(s: &str) -> Option<Page>`
  - `fn page_symbol(p: Page) -> &'static str` — `command`, `keyboard`,
    `gearshape`, `info.circle`
  - `Controls::toolbar: Retained<NSToolbar>` replacing `tabs`
  Task 3 calls `show_page`, which now also sets the window title.

- [ ] **Step 1: Write the failing tests** (the pure halves — the drawing has
  no unit test, see the module doc at `mod.rs:2660`). In `mod.rs`'s
  `mod tests`:

```rust
    /// Identifiers are a closed round trip: the toolbar hands back the
    /// identifier it was given, and the action turns it into a Page again.
    /// A typo in either direction would silently select nothing.
    #[test]
    fn every_page_identifier_round_trips() {
        for p in [Page::Shortcuts, Page::Keyboard, Page::System, Page::About] {
            assert_eq!(page_from_identifier(page_identifier(p)), Some(p));
        }
        assert_eq!(page_from_identifier("beckon.nope"), None);
    }

    /// Four distinct identifiers and four distinct symbols: a duplicate
    /// would make two doors one item.
    #[test]
    fn the_four_items_are_distinct() {
        let ids: Vec<&str> = [Page::Shortcuts, Page::Keyboard, Page::System, Page::About]
            .iter()
            .map(|p| page_identifier(*p))
            .collect();
        let mut sorted = ids.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), 4, "identifiers must be distinct: {ids:?}");

        let syms: Vec<&str> = [Page::Shortcuts, Page::Keyboard, Page::System, Page::About]
            .iter()
            .map(|p| page_symbol(*p))
            .collect();
        let mut s2 = syms.clone();
        s2.sort_unstable();
        s2.dedup();
        assert_eq!(s2.len(), 4, "symbols must be distinct: {syms:?}");
    }

    /// The captions come from core's table, not from literals here -- the
    /// third door reads `General` on this platform.
    #[test]
    fn the_toolbar_captions_come_from_the_mac_table() {
        use beckon_core::settings::{page_label, PageLabels};
        assert_eq!(page_label(Page::System, PageLabels::MAC), "General");
        assert_eq!(page_label(Page::Shortcuts, PageLabels::MAC), "Shortcuts");
    }
```

- [ ] **Step 2: Run them and see them fail**

Run: `cargo +stable test -p beckon-macos toolbar`
Expected: compile errors for `page_identifier` / `page_symbol`.

- [ ] **Step 3: Add the pure helpers** to `mod.rs`, next to `page_at` /
  `page_index`:

```rust
/// The toolbar item identifier for a door. Namespaced because an
/// `NSToolbar`'s identifiers share a space with AppKit's own
/// (`NSToolbarFlexibleSpaceItem` and friends).
fn page_identifier(p: Page) -> &'static str {
    match p {
        Page::Shortcuts => "beckon.shortcuts",
        Page::Keyboard => "beckon.keyboard",
        Page::System => "beckon.general",
        Page::About => "beckon.about",
    }
}

/// The inverse. `None` for anything AppKit added on its own.
fn page_from_identifier(s: &str) -> Option<Page> {
    [Page::Shortcuts, Page::Keyboard, Page::System, Page::About]
        .into_iter()
        .find(|p| page_identifier(*p) == s)
}

/// The SF Symbol each door wears. All four ship in macOS 11, which is
/// `LSMinimumSystemVersion` (`assets/macos/Info.plist`).
fn page_symbol(p: Page) -> &'static str {
    match p {
        Page::Shortcuts => "command",
        Page::Keyboard => "keyboard",
        Page::System => "gearshape",
        Page::About => "info.circle",
    }
}
```

- [ ] **Step 4: Write the delegate class.** Near `define_class!`'s other
  classes in `mod.rs`, add:

```rust
define_class!(
    // SAFETY:
    // - NSObject has no subclassing requirements.
    // - ToolbarTarget does not implement Drop.
    #[unsafe(super(NSObject))]
    // NSToolbarDelegate is main-thread-only, and so is everything this
    // class reaches for through `controls()`.
    #[thread_kind = MainThreadOnly]
    #[name = "BeckonToolbarTarget"]
    struct ToolbarTarget;

    unsafe impl NSObjectProtocol for ToolbarTarget {}

    unsafe impl NSToolbarDelegate for ToolbarTarget {
        /// Build one item. AppKit asks once per identifier and keeps it.
        #[unsafe(method(toolbar:itemForItemIdentifier:willBeInsertedIntoToolbar:))]
        fn item_for_identifier(
            &self,
            _toolbar: &NSToolbar,
            id: &NSString,
            _inserted: bool,
        ) -> Option<Retained<NSToolbarItem>> {
            let mtm = MainThreadMarker::new()?;
            let p = page_from_identifier(&id.to_string())?;
            let item = unsafe {
                NSToolbarItem::initWithItemIdentifier(NSToolbarItem::alloc(), id)
            };
            let caption = NSString::from_str(page_label(p, PageLabels::MAC));
            item.setLabel(&caption);
            item.setPaletteLabel(&caption);
            let sym = NSString::from_str(page_symbol(p));
            if let Some(img) =
                NSImage::imageWithSystemSymbolName_accessibilityDescription(&sym, Some(&caption))
            {
                item.setImage(Some(&img));
            }
            unsafe {
                item.setTarget(Some(self as &AnyObject));
                item.setAction(Some(sel!(beckonToolbarPage:)));
            }
            let _ = mtm;
            Some(item)
        }

        #[unsafe(method(toolbarDefaultItemIdentifiers:))]
        fn default_identifiers(&self, _t: &NSToolbar) -> Retained<NSArray<NSString>> {
            page_identifier_array()
        }

        #[unsafe(method(toolbarAllowedItemIdentifiers:))]
        fn allowed_identifiers(&self, _t: &NSToolbar) -> Retained<NSArray<NSString>> {
            page_identifier_array()
        }

        /// Without this the preference style draws no selection pill at all.
        #[unsafe(method(toolbarSelectableItemIdentifiers:))]
        fn selectable_identifiers(&self, _t: &NSToolbar) -> Retained<NSArray<NSString>> {
            page_identifier_array()
        }
    }

    impl ToolbarTarget {
        /// A door was clicked. The identifier IS the door.
        #[unsafe(method(beckonToolbarPage:))]
        fn on_toolbar_page(&self, sender: &NSToolbarItem) {
            let id = unsafe { sender.itemIdentifier() };
            if let Some(p) = page_from_identifier(&id.to_string()) {
                show_page(p);
            }
        }
    }
);

/// The four identifiers, in door order, as AppKit wants them.
fn page_identifier_array() -> Retained<NSArray<NSString>> {
    let ids: Vec<Retained<NSString>> = [Page::Shortcuts, Page::Keyboard, Page::System, Page::About]
        .into_iter()
        .map(|p| NSString::from_str(page_identifier(p)))
        .collect();
    NSArray::from_retained_slice(&ids)
}
```

  Match the exact generated spellings as you compile (`NSToolbarItem::alloc`
  takes no `mtm`; `initWithItemIdentifier`, `setLabel`, `setImage` are safe;
  `setTarget`/`setAction` are `unsafe`; `itemIdentifier`'s safety and the
  `#[unsafe(method(...))]` selector strings are what to check first). Keep
  the design; if a piece cannot exist as written, report NEEDS_CONTEXT.

- [ ] **Step 5: Install the toolbar and delete the tab strip.**
  - In `open()`, replace the `NSSegmentedControl` block (`:1865-1883`) with:

```rust
    let toolbar_target: Retained<ToolbarTarget> =
        unsafe { msg_send![ToolbarTarget::alloc(mtm), init] };
    let toolbar = unsafe {
        NSToolbar::initWithIdentifier(NSToolbar::alloc(), &NSString::from_str("beckon.settings"))
    };
    toolbar.setDelegate(Some(ProtocolObject::from_ref(&*toolbar_target)));
    toolbar.setAllowsUserCustomization(false);
    toolbar.setDisplayMode(NSToolbarDisplayMode::IconAndLabel);
```

  - Remove `tabs` from the root stack's arranged subviews (`:1913-1925`) so
    the root holds the four pages and the command bar only.
  - After `setContentView` in the window build, add:

```rust
    window.setToolbar(Some(&toolbar));
    window.setToolbarStyle(NSWindowToolbarStyle::Preference);
```

  - In `Controls`, replace `tabs: Retained<NSSegmentedControl>` with
    `toolbar: Retained<NSToolbar>` and add
    `_toolbar_target: Retained<ToolbarTarget>` with the comment that the
    delegate reference is weak and dropping this freezes the toolbar.
  - Delete `on_page` (`:874-882`) and the `beckonPage:` method it served.

- [ ] **Step 6: Move the warning the tab used to carry, then delete
  `shortcuts_tab_label`.** `apply_state` (`:2495-2501`) currently writes
  segment 0's label from `shortcuts_tab_label(count, warn_dot_shown(..))`.
  A toolbar item cannot carry it (spec §5.1). Replace that block with
  nothing — the service line already reports the same condition, and the
  menu's *Needs attention* carries it too (phase 1).
  - Delete `shortcuts_tab_label` and the test
    `the_shortcuts_tab_carries_the_warning_when_the_file_moved`.
  - **Keep `the_tab_mark_is_the_complement_of_the_banner`**, which tests
    `beckon_core::settings::warn_dot_shown` against `banner_shown` — a core
    partition that still holds and is the thing that note warns about
    (`CLAUDE.md`: `warn_dot_shown` once had zero callers). Rename it
    `the_warn_dot_is_the_complement_of_the_banner` and add a line to its doc
    saying the macOS SHELL no longer draws the dot, so this now pins core's
    partition only.
  - **Then check whether `warn_dot_shown` has any caller left**, and say so in
    your report:
    `grep -rn "warn_dot_shown" crates/ | grep -v "settings.rs"`.
    `CLAUDE.md` records that this exact function once had **zero** callers in
    `beckon-macos` while a core test kept passing — the shape this phase can
    recreate. If Windows still calls it, nothing is owed. If nobody does,
    report it as a finding for the final review rather than deleting it or
    inventing a use.

- [ ] **Step 7: `show_page` selects the item and titles the window.** In
  `show_page` (`:588-615`), replace the `setSelectedSegment` line with:

```rust
    unsafe {
        c.toolbar
            .setSelectedItemIdentifier(Some(&NSString::from_str(page_identifier(p))));
    }
    c.window
        .setTitle(&NSString::from_str(page_label(p, PageLabels::MAC)));
```

  Everything else in `show_page` stays exactly as it is (the `stop_recording`
  first, the page hiding loop, the command-bar buttons, the `ShowPage`
  command).

- [ ] **Step 8: Keep the file name visible.** The title used to be
  `beckon - {file}` and `apply_state` appended `" *"` when dirty
  (`:2614-2619`). The title is now the page name, so:
  - set the file name as the window's SUBTITLE once, in `open()`:
    `window.setSubtitle(&NSString::from_str(&file_name));` — verify
    `NSWindow::setSubtitle` exists in the 0.3.2 bindings (macOS 11+); if it
    does not, keep the file name out of the chrome and say so in your report,
    because the config row on the General page already names the file.
  - delete the `" *"` title rewriting in `apply_state`. `setDocumentEdited`
    (`:2505`) is already called and is the native dirty marker.

- [ ] **Step 9: Gate and commit**

```bash
cargo +stable fmt --all -- --check
cargo +stable clippy --workspace --all-targets -- -D warnings
cargo +stable clippy --target aarch64-pc-windows-msvc --all-targets -- -D warnings
cargo +stable test --workspace
git branch --show-current
git commit --only crates/beckon-macos/src/settings_window/mod.rs -m "feat(macos): a preference-style toolbar replaces the tab strip"
git show --stat HEAD
```

Report what the controller should see: four centred items with icons and
captions (`Shortcuts`, `Keyboard`, `General`, `About`), a selection pill on
the open one, and the window title reading that page's name.

---

### Task 3: the window sizes itself to the page

**Files:**
- Modify: `crates/beckon-macos/src/settings_window/mod.rs` — `MIN_HEIGHT` and
  its doc (`:130-162`), the window build (`:1983`), `show_page` (`:588-615`),
  the post-open sizing (`:2063-2066`).
- Test: `mod.rs`'s `mod tests` (the arithmetic only).

**Interfaces:**
- Produces: `fn content_height(page_fitting: f64, bar: f64, insets: f64, spacing: f64) -> f64`
  and `const MIN_CONTENT_HEIGHT: f64` — the floor no page may go below.

- [ ] **Step 1: Write the failing tests**

```rust
    /// The window's content height is the page plus the command band plus
    /// the root stack's own chrome -- one expression, so a page that grows
    /// cannot be clipped by a constant somebody forgot to move.
    #[test]
    fn content_height_is_the_page_plus_the_band_and_the_chrome() {
        // root insets are 12 top + 12 bottom, and one 10 pt gap between the
        // page and the band.
        assert_eq!(content_height(360.0, 24.0, 24.0, 10.0), 418.0);
        assert_eq!(content_height(144.0, 24.0, 24.0, 10.0), 202.0);
    }

    /// A page shorter than the floor still gets the floor: a 150 pt window
    /// with a title bar and a toolbar is not a window.
    #[test]
    fn no_page_shrinks_the_window_below_the_floor() {
        assert!(MIN_CONTENT_HEIGHT >= 200.0);
        assert_eq!(content_height(10.0, 24.0, 24.0, 10.0), MIN_CONTENT_HEIGHT);
    }
```

- [ ] **Step 2: Run them and see them fail**

Run: `cargo +stable test -p beckon-macos content_height`
Expected: compile error, `cannot find function content_height`.

- [ ] **Step 3: Implement the arithmetic**

```rust
/// The shortest content height the window will take, whatever the page asks
/// for. Below this a titled window with a preference toolbar is mostly
/// chrome, and `setContentMinSize` would let the user drag into that.
const MIN_CONTENT_HEIGHT: f64 = 200.0;

/// What the window's CONTENT height must be to show one page whole.
///
/// `page_fitting` is the page view's `fittingSize().height`, `bar` the
/// command band's, `insets` the root stack's top plus bottom, and `spacing`
/// the one gap between the page and the band. One expression with one
/// meaning, for the reason `page_plan`'s `content_h` exists on Windows: three
/// spellings of "how tall is this door" drift, and the drift reads as a
/// rendering fault.
fn content_height(page_fitting: f64, bar: f64, insets: f64, spacing: f64) -> f64 {
    (page_fitting + bar + insets + spacing).max(MIN_CONTENT_HEIGHT)
}
```

- [ ] **Step 4: Run the tests and see them pass**

Run: `cargo +stable test -p beckon-macos content_height`
Expected: 2 passed.

- [ ] **Step 5: Retire the tallest-door rule.** In `mod.rs`:
  - Replace `const MIN_HEIGHT: f64 = WINDOW_HEIGHT;` (`:162`) and its doc
    (`:130-159`) with a short doc saying the rule is **RETIRED as of phase 2
    (2026-09-23)**: the window now takes each page's own height, so the floor
    is `MIN_CONTENT_HEIGHT` rather than the tallest door's content. Keep the
    measured table from the old doc (`Shortcuts 360 / About 306 / Keyboard
    190 / System 144`) as the record of what was measured, with the note that
    those numbers are what made one shared height wrong.
  - `WINDOW_HEIGHT` stays: it is the first-open height for the Shortcuts
    page, which is still the tallest.

- [ ] **Step 6: Resize on every page change.** Add to `mod.rs`:

```rust
/// Size the window to the page it is showing, keeping the title bar where
/// it is (AppKit frames grow DOWNWARD from the origin, so the origin must
/// move by the same amount the height does).
///
/// `animate` is false for the first layout -- a window that resizes while it
/// is being shown reads as a glitch, not as a transition.
fn size_to_page(c: &Controls, p: Page, animate: bool) {
    let page = c.pages[page_index(p)].fittingSize().height;
    let bar = c.bar.fittingSize().height;
    let want = content_height(page, bar, ROOT_INSET_Y * 2.0, ROOT_SPACING);
    c.window
        .setContentMinSize(NSSize::new(MIN_WIDTH, want));
    let frame = c.window.frame();
    let chrome = frame.size.height - c.window.contentRectForFrameRect(frame).size.height;
    let height = want + chrome;
    let top = frame.origin.y + frame.size.height;
    let new = NSRect::new(
        NSPoint::new(frame.origin.x, top - height),
        NSSize::new(frame.size.width, height),
    );
    c.window.setFrame_display_animate(new, true, animate);
}
```

  - Add `const ROOT_INSET_Y: f64 = 12.0;`, `const ROOT_SPACING: f64 = 10.0;`
    and `const MIN_WIDTH: f64 = 560.0;` next to the other constants, and use
    them where `open()` currently hard-codes 12.0 / 10.0 / 560.0
    (`:1888-1912`, `:1983`) so the two cannot drift.
  - Add `bar: Retained<NSStackView>` to `Controls` (the band is built at
    `:1817` and currently only lives in the root stack).
  - Call it at the end of `show_page`: `size_to_page(&c, p, true);`
  - In `open()`'s post-`show_page` block (`:2063-2066`), replace
    `setContentSize(WINDOW_WIDTH, WINDOW_HEIGHT)` with
    `c.window.setContentSize(NSSize::new(WINDOW_WIDTH, WINDOW_HEIGHT)); size_to_page(&c, page, false);`
    then `center()` — the first open is not animated, and the comment at
    `:2049-2062` about ordering still applies.

- [ ] **Step 7: Gate and commit** (the full gate from the top of this plan).

```bash
git commit --only crates/beckon-macos/src/settings_window/mod.rs -m "feat(macos): the settings window takes each page's own height"
git show --stat HEAD
```

Report what the controller should check: opening on Shortcuts looks as before;
switching to Keyboard/General/About shrinks the window with an animation and
no clipped content; dragging the window smaller stops at the page's own
height.

---

### Task 4: the grouped-form primitives

**Files:**
- Modify: `crates/beckon-macos/src/settings_window/widgets.rs` — add two
  functions beside `card` (`:303`) and `divider` (`:337`).

**Interfaces:**
- Produces:
  - `pub(super) fn form_row(label: &NSView, control: &NSView, mtm: MainThreadMarker) -> Retained<NSStackView>`
  - `pub(super) fn labelled(title: &str, note: Option<&str>, mtm: MainThreadMarker) -> Retained<NSStackView>`
  - `pub(super) fn group(rows: &[&NSView], mtm: MainThreadMarker) -> Retained<NSBox>`
  Tasks 5-7 build every page row out of these three.

- [ ] **Step 1: Implement** (no unit test is possible for drawing — the
  module doc at `mod.rs:2660` says why; Tasks 5-7 are verified on screen):

```rust
/// One row of a grouped form: a label block on the left, a control hard
/// right (spec §5.2).
///
/// The spring is what pins the control right, the same primitive every row
/// in this window already uses -- this function exists so the THREE pages
/// stop spelling it out, not because the shape is new.
pub(super) fn form_row(
    label: &NSView,
    control: &NSView,
    mtm: MainThreadMarker,
) -> Retained<NSStackView> {
    hstack(&[label, &*spring(mtm), control], mtm)
}

/// The left half of a form row: a title, and under it the secondary line
/// that explains it when one is needed.
///
/// `None` is the common case and costs nothing: a row whose meaning its own
/// title carries says nothing more, which is the same rule the settings
/// list's status words follow.
pub(super) fn labelled(
    title: &str,
    note: Option<&str>,
    mtm: MainThreadMarker,
) -> Retained<NSStackView> {
    let t = label(title, mtm);
    match note {
        None => vstack(&[&*t], 2.0, mtm),
        Some(n) => {
            let s = secondary(n, mtm);
            s.setFont(Some(&NSFont::systemFontOfSize(11.0)));
            vstack(&[&*t, &*s], 2.0, mtm)
        }
    }
}

/// A rounded group with a hairline between each pair of rows (spec §5.2).
///
/// `card` draws the ground; this adds the dividers, so a page never
/// interleaves `divider()` calls with its rows by hand -- which is how the
/// Windows twin's `system_plan` ends up owning divider offsets, and is the
/// same defect in a different spelling.
pub(super) fn group(rows: &[&NSView], mtm: MainThreadMarker) -> Retained<NSBox> {
    let mut stacked: Vec<Retained<NSView>> = Vec::new();
    for (i, r) in rows.iter().enumerate() {
        if i > 0 {
            stacked.push(Retained::into_super(divider(mtm)));
        }
        stacked.push((*r).retain());
    }
    let refs: Vec<&NSView> = stacked.iter().map(|v| &**v).collect();
    card(&*vstack(&refs, 10.0, mtm), mtm)
}
```

  Check `Retained::into_super` / `retain()` spellings against objc2 0.6 as
  you compile; keep the shape.

  **Leave `CARD_TEXT_WIDTH` (`widgets.rs:104`) alone.** It hard-codes the
  640 pt window width, and the wrapping labels that use it are being turned
  into `labelled`'s secondary lines by Tasks 5-7. If a secondary line wraps
  at the wrong width once a page is rebuilt, report it — do not retune the
  constant blind.

- [ ] **Step 2: Gate and commit**

```bash
cargo +stable clippy -p beckon-macos --all-targets -- -D warnings
cargo +stable fmt --all -- --check
git commit --only crates/beckon-macos/src/settings_window/widgets.rs -m "feat(macos): grouped-form primitives for the settings pages"
git show --stat HEAD
```

  macOS will report `group` / `form_row` / `labelled` as unused until Task 5
  — that is expected; do NOT add `allow(dead_code)`. Run the clippy gate
  anyway and say in your report what it printed.

---

### Task 5: the Keyboard page becomes a grouped form

**Files:**
- Modify: `crates/beckon-macos/src/settings_window/keyboard.rs` —
  `build` (`:92-203`) and `apply` (`:220+`) only if a control's storage
  changes (it must not).

**Interfaces:**
- Consumes: `widgets::{group, form_row, labelled}` (Task 4).
- Produces: the same `KeyboardControls` fields, unchanged, so `apply` and
  every selector keep working.

- [ ] **Step 1: Rewrite `build`'s body** to these three groups, keeping every
  control construction line exactly as it is today (same `w::switch`,
  `w::check`, `NSPopUpButton`, same selectors, same field names):

```rust
    // Group 1 -- Caps Lock as the beckon key.
    let caps_row = w::form_row(
        &*w::labelled(
            "Use Caps Lock as a shortcut key",
            Some("Hold Caps Lock and press a key instead of the chord below."),
            mtm,
        ),
        &*caps,
        mtm,
    );
    let hold_row = w::form_row(&*w::labelled("While held, Caps Lock presses", None, mtm), &*mods_row, mtm);
    let tap_row = w::form_row(&*w::labelled("When tapped alone", None, mtm), &*tap, mtm);
    let g_caps = w::group(&[&*caps_row, &*hold_row, &*tap_row], mtm);

    // Group 2 -- how the list writes a bound chord.
    let shorthand_row = w::form_row(
        &*w::labelled("Show shortcuts as Caps", None, mtm),
        &*shorthand,
        mtm,
    );
    let g_view = w::group(&[&*shorthand_row], mtm);

    // Group 3 -- the grant this page's feature needs.
    let im_row = w::form_row(&*w::labelled("Input Monitoring", Some(&note_text), mtm), &*im_button, mtm);
    let g_grant = w::group(&[&*im_row], mtm);

    let page = w::vstack(&[&*g_caps, &*g_view, &*g_grant], 12.0, mtm);
```

  Where each name in that block comes from (build them from today's
  `keyboard.rs` lines; do not invent new controls):

  | name | today |
  |---|---|
  | `caps` | the existing `w::switch` stored in `KeyboardControls::caps` |
  | `mods_row` | NEW `w::hstack` holding today's three `w::check` modifier boxes, unchanged |
  | `tap` | the existing `NSPopUpButton` for "when tapped alone" |
  | `shorthand` | the existing `w::switch` stored in `KeyboardControls::shorthand` |
  | `im_button` | the existing `w::push` from today's `note_row` ("Open Input Monitoring") |
  | `note_text` | the sentence today's `w::wrapping` note shows — keep taking it from `beckon_core::settings` (`input_monitoring_warning` / `input_monitoring_after_asking`), never inline it |

  So today's `caps_row`, `note`, `note_row`, `hold_row` and `shorthand_row`
  all survive as rows: the free-standing wrapping note becomes the Input
  Monitoring row's secondary line, and its button becomes that row's control.
  **If a control's text is currently produced by `beckon_core::settings`
  (the Input Monitoring warning), keep taking it from there — do not inline
  the sentence.**

- [ ] **Step 2: Compile and gate**

```bash
cargo +stable clippy -p beckon-macos --all-targets -- -D warnings
cargo +stable test --workspace
cargo +stable fmt --all -- --check
```

- [ ] **Step 3: Commit**

```bash
git commit --only crates/beckon-macos/src/settings_window/keyboard.rs -m "feat(macos): the Keyboard page is a grouped form"
git show --stat HEAD
```

Report: the three groups in order, and which of today's widgets became which
row, so the controller can check nothing vanished.

---

### Task 6: the General page becomes a grouped form

**Files:**
- Modify: `crates/beckon-macos/src/settings_window/system.rs` — `build`
  (`:74-167`); `apply` (`:191+`) only if storage changes (it must not).

**Interfaces:**
- Consumes: `widgets::{group, form_row, labelled}`.
- Produces: the same `SystemControls` fields, unchanged.

- [ ] **Step 1: Rewrite `build`'s body** into three groups, reusing every
  existing control line (`w::switch` for pause, `w::push` for reload, the
  slider plus its value label, the two `w::glyph`s per file row):

```rust
    let g_service = w::group(
        &[
            &*w::form_row(
                &*w::labelled("Shortcuts on", Some("Option-click the menu bar icon to toggle."), mtm),
                &*pause,
                mtm,
            ),
            &*w::form_row(&*w::labelled("Configuration", None, mtm), &*reload, mtm),
        ],
        mtm,
    );

    let g_files = w::group(
        &[
            &*w::form_row(&*w::labelled("Config file", None, mtm), &*config_tail, mtm),
            &*w::form_row(&*w::labelled("Log", None, mtm), &*log_tail, mtm),
        ],
        mtm,
    );

    let g_look = w::group(
        &[&*w::form_row(&*w::labelled("Window transparency", None, mtm), &*opacity_tail, mtm)],
        mtm,
    );

    let page = w::vstack(&[&*g_service, &*g_files, &*g_look], 12.0, mtm);
```

  Where each name comes from (all built from today's `system.rs` lines):

  | name | today |
  |---|---|
  | `pause` | the existing `w::switch` in `SystemControls::pause` |
  | `reload` | the existing `w::push` ("Reload") |
  | `config_tail` | NEW `w::hstack` of today's config-row tail: the `w::value` path label and its two `w::glyph`s (Reveal, Open) |
  | `log_tail` | the same for the log row |
  | `opacity_tail` | NEW `w::hstack` of today's `w::slider` plus its `w::value` readout, keeping the `pin_min_width` calls (44 and 160) they already carry |
  **Keep `log_row.setHidden(true)`'s behaviour**: the row is hidden when this
  run has no log (`system.rs:150`) — hide the whole `form_row` you built for
  it, and say in your report which view now carries that `setHidden`, because
  the field it is stored in must still be the one `apply` toggles.

- [ ] **Step 2: Compile and gate** (as Task 5 Step 2).

- [ ] **Step 3: Commit**

```bash
git commit --only crates/beckon-macos/src/settings_window/system.rs -m "feat(macos): the General page is a grouped form"
git show --stat HEAD
```

---

### Task 7: the About page becomes a grouped form

**Files:**
- Modify: `crates/beckon-macos/src/settings_window/about.rs` — `build`
  (`:154-367`); `apply` (`:387+`) only if storage changes (it must not).

**Interfaces:**
- Consumes: `widgets::{group, form_row, labelled}`.
- Produces: the same `AboutControls` fields, unchanged.

- [ ] **Step 1: Rewrite `build`'s body** as a header block plus three groups:

```rust
    // The header stays free-standing: a mark, the name, the version line and
    // the update button are not a form.
    let header = w::vstack(&[&*w::centred(&*mark, mtm), &*name, &*build_row, &*update_row], 6.0, mtm);

    let g_perm = w::group(
        &[
            &*w::form_row(&*w::labelled("Accessibility", Some(&access_text), mtm), &*grant, mtm),
        ],
        mtm,
    );

    let g_copy = w::group(
        &[
            &*w::form_row(&*w::labelled("Running from", None, mtm), &*loc_tail, mtm),
            &*w::form_row(&*w::labelled("Command", None, mtm), &*command_tail, mtm),
        ],
        mtm,
    );

    let page = w::vstack(&[&*header, &*g_perm, &*g_copy, &*links], 12.0, mtm);
```

  Where each name comes from (all from today's `about.rs`):

  | name | today |
  |---|---|
  | `mark`, `name`, `build_row`, `update_row` | unchanged, including `pin_height(&mark, 34)` / `pin_exact_width(&mark, 34)` |
  | `access_text` | the sentence today's `access` wrapping label shows, still from `beckon_core::settings::accessibility_warning` |
  | `grant` | the existing "Grant Accessibility..." `w::push` |
  | `loc_tail`, `command_tail` | NEW `w::hstack`s of today's `value_row` tail (`about.rs:139`): the value label plus its Copy `w::glyph` |
  | `links` | today's link row, unchanged | The `hook` sentence (`about.rs`'s `w::wrapping`) becomes the
  secondary line of a row in `g_perm` if it is still drawn — check whether
  phase 1 left it in place, and if it is, give it its own `form_row` with no
  control rather than deleting it.

- [ ] **Step 2: Compile and gate** (as Task 5 Step 2).

- [ ] **Step 3: Commit**

```bash
git commit --only crates/beckon-macos/src/settings_window/about.rs -m "feat(macos): the About page is a grouped form"
git show --stat HEAD
```

---

### Task 8: the probes and the examples follow the new shell

**Files:**
- Modify: `crates/beckon-macos/examples/settings_drive.rs:481-496` — the
  assertion that the window opens at 640x532.
- Modify: `testing/macos_settings_drive.lua` and `testing/macos_settings_*.sh`
  if they name the tab strip, a segment index, or a fixed window height
  (grep for `segment`, `tab`, `532`, `500` first and report what you found).

- [ ] **Step 1: Find every place that encodes the old shell**

```bash
grep -rn "532\|setSelectedSegment\|selectedSegment\|shortcuts_tab_label\|segment" \
  crates/beckon-macos/examples testing/ | grep -v '\.png'
```

- [ ] **Step 2: Update the geometry assertion.** The window no longer has one
  height: it opens on Shortcuts and resizes per page. Replace the exact
  `640x532` check with: width is `WINDOW_WIDTH`, height is within 4 pt of the
  Shortcuts page's own height, and the height CHANGES after switching to
  General. Print both heights so a human reading the probe output sees the
  two numbers rather than a bare pass.

- [ ] **Step 3: Update the driver script** so it selects a page through the
  toolbar (AX: `click button "General" of toolbar 1 of window 1`) instead of
  a segmented control, wherever it does that today.

- [ ] **Step 4: Gate and commit**

```bash
cargo +stable check -p beckon-macos --examples
cargo +stable clippy -p beckon-macos --all-targets -- -D warnings
# Syntax-check whatever you touched, with the right tool for it:
#   shell:  bash -n testing/<file>.sh
#   lua:    luac -p testing/macos_settings_drive.lua   (skip, and say so, if luac is absent)
git commit --only crates/beckon-macos/examples/settings_drive.rs testing/ -m "test(macos): the probes drive a toolbar, not a tab strip"
git show --stat HEAD
```

---

### Task 9: the notes record what this phase measured

**Files:**
- Modify: `docs/notes/settings-window.md` — the entries this phase overturns.
- Modify: `docs/notes/macos-backend.md` — a short section for what the
  controller measured on screen (the controller supplies the numbers; write
  the section around them).

- [ ] **Step 1: Mark the entries this phase changes.** In
  `docs/notes/settings-window.md`:
  - The `MIN_HEIGHT` / tallest-door entry: **RETIRED 2026-09-23 (macOS)** —
    each page now sizes the window, and the four measured page heights are
    why one shared height was wrong. Windows' geometry is untouched.
  - The tab-strip entry (`NSSegmentedControl`, "not four hand-drawn pills"):
    **NARROWED 2026-09-23** — the argument against hand-drawn pills still
    holds; macOS now uses the platform's own preference toolbar instead, and
    what the segmented control bought (contrast, focus ring, keyboard) the
    toolbar also brings.
  - The status-vocabulary entry that mentions the Shortcuts TAB's count and
    warning dot: **NARROWED** — a toolbar item cannot carry them; the service
    line and the menu's *Needs attention* do.
- [ ] **Step 2: Write the measurement section** in
  `docs/notes/macos-backend.md` from the controller's screenshots: what the
  toolbar looks like at the preference style, the per-page heights actually
  measured, whether the resize animation reads as a transition, and anything
  refuted along the way. Follow that file's voice: what was measured, on which
  machine, and what it does not prove.
- [ ] **Step 3: Commit**

```bash
git commit --only docs/notes/settings-window.md docs/notes/macos-backend.md -m "docs: the settings window's new shell, measured"
git show --stat HEAD
```

---

## What the controller verifies on screen (no implementer opens the GUI)

Recorded here so the tasks above can name it. The controller runs the real
`serve`, drives the window with AppleScript and captures the window's own
AX bounds:

| # | check | control |
|---|---|---|
| S1 | Four toolbar items, centred, icon over caption, `General` third | the caption is `General`, not `System` |
| S2 | The open page's item wears the selection pill, and the title is that page's name | switch pages; both must move |
| S3 | Switching to Keyboard/General/About resizes the window, animated, nothing clipped | measure each page's height from the screenshots |
| S4 | The Shortcuts page is unchanged: table, editor, banner, command bar | compare against phase 1's screenshot |
| S5 | Save / Close / Open config file show on Shortcuts and Keyboard only | check General and About have the band but no buttons |
| S6 | Each page's rows sit in rounded groups with hairlines, control hard right | a page with one group and a page with three |
| S7 | Dragging the window shorter stops at the page's own height | try it on General, the shortest page |
