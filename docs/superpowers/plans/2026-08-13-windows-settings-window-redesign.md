# Themed Settings Window (Windows) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give `beckon-serve`'s Windows settings window a client-drawn title bar, light/dark themes that follow Windows, cards, a toggle, and slight transparency — without changing its toolkit, its information architecture, or its high-contrast behaviour.

**Architecture:** Every *decision* (which theme, which backdrop tier, what colour) moves into `beckon-core` as plain data with real unit tests, so all three CI jobs enforce it. `beckon-windows` keeps only the drawing, and grows from one 6689-line file into a five-module directory. `Theme` has three branches — Light, Dark, and HighContrast; the third reads `GetSysColor` exactly as the window does today, so the rule the old design rested on survives as one arm of a `match`.

**Tech Stack:** Rust 2021, `windows` crate 0.61 (Win32 GDI / DWM / UxTheme / Registry), no new dependencies.

**Spec:** `docs/superpowers/specs/2026-08-13-windows-settings-window-redesign-design.md`

## Global Constraints

- **No new crate dependencies.** Everything needed is already in the `windows` 0.61 feature list, except the two features added in Task 4.
- **No new config keys.** The theme follows Windows and is not user-configurable. `keyboard.caps_hold` remains the only key this project has added.
- **`tok::ROWS = 8`** — fixed at every DPI. Never scaled, never derived from the config.
- **`CBS_SORT` is never set on `IDC_COMBO`.** `ComboView::key` is an index into `shortcuts::key_table()`; sorting shifts every index and writes a key the user did not choose, silently.
- **`cap::STOP` stays narrower than `cap::RECORD`.** A wider armed caption forces `layout` onto the capture path, and `layout` means `SetWindowPos` on the populated App combo — the measured data-loss path.
- **No Shift chip in the `Hold` row.** `Chord` carries only ctrl / super / alt.
- **Command-bar button order is unchanged:** `Save` · `Reload` · `Open config file` · `Close`.
- **Control ids 1001–1008, 1012, 1013 are unchanged** — `crates/beckon-windows/examples/settings_probe.rs` hard-codes them.
- **`Theme::HighContrast` reads `GetSysColor` verbatim.** No literal may reach the screen in that branch.
- **`crates/beckon-macos/src/settings_window.rs` is not touched by any task.**
- **Never call `layout` from a colour or theme change.** Nothing about a repaint moves a control.
- **Font faces must be spelled exactly** as given in Task 6 — `lfFaceName` holds 32 wchar and the truncation is not uniform. `"Segoe UI Variable Text Semib"` silently returns Arial.
- **The gate for every task, in this exact shape:**

  ```bash
  cargo fmt --all -- --check
  cargo clippy --workspace --exclude beckon-linux --exclude beckon-windows \
        --all-targets -- -D warnings
  cargo test -p beckon-core          # and -p beckon-cli if the task touches it
  cargo check --target aarch64-pc-windows-msvc --all-targets
  ```

  **CORRECTED 2026-08-13, mid-execution.** This plan originally specified a
  bare `cargo clippy --all-targets` and a bare `cargo test`, workspace-wide.
  Both are wrong for this repo, and the first cannot pass at all:

  - `crates/beckon-windows/src/lib.rs` leaves `pub mod shell;` **un-gated**
    while the `windows` crate is a `cfg(target_os = "windows")` dependency, so
    a bare workspace build on a macOS host fails with *"use of unresolved
    module or unlinked crate `windows`"* at `shell.rs:26`. CI never sees this
    because every CI job excludes the two crates that are not its host's —
    `ci.yml` uses `--exclude beckon-macos --exclude beckon-windows` on ubuntu,
    `--exclude beckon-linux --exclude beckon-windows` on macos, and
    `--exclude beckon-linux --exclude beckon-macos` on windows. The local
    command must have the same shape as the CI job for the host it runs on.
  - Workspace-wide `cargo test` is unreliable on this machine — see the
    SIGKILL note below.

  `--all-targets` on the cross-check is required: `--examples` does not build
  `[[bin]]` targets, and testing a stale `beckon-serve.exe` is a documented
  trap.

- **`signal: 9` / SIGKILL with no test failure is environmental, not your
  bug.** On this host, freshly-linked build scripts are killed on a cold
  cache; the victim rotates (`windows_aarch64_msvc`, `libc`, `beckon-cli`,
  `beckon-cli`'s own test binaries have all been observed). **Re-run the same
  command** — each run caches the scripts that did succeed, so it converges,
  and the Windows cross-check above needed five attempts from cold before
  finishing clean. Once warm it completes in seconds. Do not report BLOCKED
  for a SIGKILL until you have re-run the command at least three times and it
  stopped making progress. CI is the authority either way.

- **This host cannot run Windows.** No task may be reported as verified on the
  strength of a `cargo check`. Tasks 7–14 carry hardware gates settled in
  Task 15.

---

## File Structure

**Created:**

| File | Responsibility |
|---|---|
| `crates/beckon-core/src/theme.rs` | `Palette` data, `Theme`/`Backdrop` resolution, `contrast()`, and the CI-enforced contrast test |
| `crates/beckon-windows/src/settings_window/mod.rs` | window creation, wndproc, message routing, `Ui` state |
| `crates/beckon-windows/src/settings_window/theme.rs` | OS theme inputs, `COLORREF` conversion, brush/pen cache |
| `crates/beckon-windows/src/settings_window/chrome.rs` | client-drawn title bar: `WM_NCCALCSIZE`, `WM_NCHITTEST`, caption buttons |
| `crates/beckon-windows/src/settings_window/paint.rs` | card / keycap / toggle / pill / note / field-border primitives |
| `crates/beckon-windows/src/settings_window/layout.rs` | the `layout` function |
| `docs/superpowers/measurements/2026-08-13-settings-redesign-a14.md` | hardware results, written in Task 15 |

**Modified:**

| File | Change |
|---|---|
| `crates/beckon-core/src/lib.rs` | `pub mod theme;` |
| `crates/beckon-windows/src/settings_window.rs` | deleted; contents distributed across the directory above |
| `crates/beckon-windows/Cargo.toml` | two `windows` features |
| `crates/beckon-cli/src/beckon.rc` | icon path, Task 14 |
| `crates/beckon-windows/examples/settings_probe.rs` | style-bit expectations, Task 15 |

---

## Task 1: The palette, and a contrast test CI cannot skip

**Files:**
- Create: `crates/beckon-core/src/theme.rs`
- Modify: `crates/beckon-core/src/lib.rs`

**Interfaces:**
- Consumes: nothing.
- Produces: `pub struct Palette` with `pub` fields of type `u32` (`0xRRGGBB`), the constants `pub const LIGHT: Palette` and `pub const DARK: Palette`, and `pub fn contrast(fg: u32, bg: u32) -> f64`.

**Why this is in `beckon-core`:** a palette checked once by a script on a laptop is a palette that drifts on the next edit. Here the check is a `#[test]` on all three CI jobs. This is the same move `RuntimeStatus` already makes for `apply_enabled`.

- [ ] **Step 1: Write the failing test**

Create `crates/beckon-core/src/theme.rs`:

```rust
//! Colour as data, so the two CI jobs that are not Windows can check it.
//!
//! `beckon-windows` converts a token to `COLORREF` at its boundary and holds
//! no literal of its own. The contrast test at the bottom of this file is the
//! reason the table lives here: the first hand-written pass failed five pairs,
//! including a dark accent FILL too light to carry white text and a card
//! border invisible against the window ground.

/// Every colour the settings window draws, as `0xRRGGBB`.
///
/// `accent` and `accent_fill` are deliberately separate. A colour that reads
/// well as text on a card and a colour that carries white text on top of it
/// are different constraints, and in dark mode they resolve to different
/// values. Collapsing them is the defect this struct's shape prevents.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Palette {
    pub bg: u32,
    pub card: u32,
    pub card_border: u32,
    pub text: u32,
    pub text_muted: u32,
    pub text_faint: u32,
    pub accent: u32,
    pub accent_hover: u32,
    pub accent_fill: u32,
    pub accent_on: u32,
    pub accent_soft: u32,
    pub field: u32,
    pub field_border: u32,
    pub keycap: u32,
    pub keycap_border: u32,
    pub keycap_edge: u32,
    pub bad_bg: u32,
    pub bad: u32,
    pub warn_bg: u32,
    pub warn: u32,
    pub unk_bg: u32,
    pub unk: u32,
    pub ok: u32,
    pub divider: u32,
}

pub const LIGHT: Palette = Palette {
    bg: 0xF2F4F8,
    card: 0xFFFFFF,
    card_border: 0xDCE0E8,
    text: 0x15181E,
    text_muted: 0x5A6270,
    text_faint: 0x6F7785,
    accent: 0x2563EB,
    accent_hover: 0x1D4FD7,
    accent_fill: 0x2563EB,
    accent_on: 0xFFFFFF,
    accent_soft: 0xE8F0FF,
    field: 0xFFFFFF,
    field_border: 0xD2D8E3,
    keycap: 0xFFFFFF,
    keycap_border: 0xCDD4E1,
    keycap_edge: 0xB6BFCF,
    bad_bg: 0xFDE7E7,
    bad: 0xB42318,
    warn_bg: 0xFDF0D5,
    warn: 0x8A5406,
    unk_bg: 0xEDEFF4,
    unk: 0x5A6270,
    ok: 0x067647,
    divider: 0xE8EBF1,
};

pub const DARK: Palette = Palette {
    bg: 0x15171C,
    card: 0x1D2027,
    card_border: 0x2B303A,
    text: 0xE7E9EE,
    text_muted: 0x9FA6B4,
    text_faint: 0x7F8795,
    accent: 0x5B92F7,
    accent_hover: 0x7AA7F9,
    accent_fill: 0x3970E6,
    accent_on: 0xFFFFFF,
    accent_soft: 0x1B2A47,
    field: 0x23262E,
    field_border: 0x353A45,
    keycap: 0x292D36,
    keycap_border: 0x39404B,
    keycap_edge: 0x131519,
    bad_bg: 0x3A1C1C,
    bad: 0xFF9A92,
    warn_bg: 0x372911,
    warn: 0xF2C46B,
    unk_bg: 0x252932,
    unk: 0x9FA6B4,
    ok: 0x5CCB92,
    divider: 0x272B33,
};

fn channel(c: u32) -> f64 {
    let c = c as f64 / 255.0;
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

fn luminance(rgb: u32) -> f64 {
    let r = channel((rgb >> 16) & 0xFF);
    let g = channel((rgb >> 8) & 0xFF);
    let b = channel(rgb & 0xFF);
    0.2126 * r + 0.7152 * g + 0.0722 * b
}

/// WCAG 2.x relative contrast. Order-independent.
pub fn contrast(fg: u32, bg: u32) -> f64 {
    let (a, b) = (luminance(fg), luminance(bg));
    let (hi, lo) = if a > b { (a, b) } else { (b, a) };
    (hi + 0.05) / (lo + 0.05)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every foreground/background pair the window actually puts on screen,
    /// with the floor it must clear: 4.5 for text, 1.2 for a border whose job
    /// is only to be visible as an edge.
    fn pairs(p: &Palette) -> Vec<(&'static str, u32, u32, f64)> {
        vec![
            ("body text on card", p.text, p.card, 4.5),
            ("muted text on card", p.text_muted, p.card, 4.5),
            ("faint text on card", p.text_faint, p.card, 4.5),
            ("muted text on window bg", p.text_muted, p.bg, 4.5),
            ("accent text on card", p.accent, p.card, 4.5),
            ("white on accent fill", p.accent_on, p.accent_fill, 4.5),
            ("accent text on soft fill", p.accent, p.accent_soft, 4.5),
            ("bad pill", p.bad, p.bad_bg, 4.5),
            ("warn pill", p.warn, p.warn_bg, 4.5),
            ("unknown pill", p.unk, p.unk_bg, 4.5),
            ("ok note glyph", p.ok, p.card, 4.5),
            ("keycap letter", p.text, p.keycap, 4.5),
            ("card border on bg", p.card_border, p.bg, 1.2),
            ("field border on card", p.field_border, p.card, 1.2),
        ]
    }

    #[test]
    fn every_pair_clears_its_floor_in_both_themes() {
        let mut failures = Vec::new();
        for (name, p) in [("light", &LIGHT), ("dark", &DARK)] {
            for (label, fg, bg, floor) in pairs(p) {
                let r = contrast(fg, bg);
                if r < floor {
                    failures.push(format!(
                        "{name}: {label} = {r:.2} (need {floor}) \
                         #{fg:06X} on #{bg:06X}"
                    ));
                }
            }
        }
        assert!(failures.is_empty(), "contrast failures:\n{}", failures.join("\n"));
    }

    #[test]
    fn contrast_matches_known_values() {
        // White on black is the WCAG maximum, 21:1.
        assert!((contrast(0xFFFFFF, 0x000000) - 21.0).abs() < 0.001);
        // A colour against itself is 1:1.
        assert!((contrast(0x2563EB, 0x2563EB) - 1.0).abs() < 0.001);
        // Order does not matter.
        assert!(
            (contrast(0x2563EB, 0xFFFFFF) - contrast(0xFFFFFF, 0x2563EB)).abs() < 1e-9
        );
    }

    /// The two tokens exist because one hex cannot do both jobs. If a future
    /// edit makes them equal in DARK, the reason for the split has been lost.
    #[test]
    fn accent_and_accent_fill_are_distinct_in_dark() {
        assert_ne!(DARK.accent, DARK.accent_fill);
    }
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p beckon-core theme`
Expected: FAIL — `error[E0433]: failed to resolve: use of undeclared crate or module 'theme'`, because `lib.rs` does not declare the module yet.

- [ ] **Step 3: Declare the module**

In `crates/beckon-core/src/lib.rs`, add alongside the existing `pub mod` lines:

```rust
pub mod theme;
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test -p beckon-core theme -- --nocapture`
Expected: PASS, 3 tests. If `every_pair_clears_its_floor_in_both_themes` fails, the assertion prints each offending pair with its measured ratio — fix the palette, not the floor.

- [ ] **Step 5: Commit**

```bash
git add crates/beckon-core/src/theme.rs crates/beckon-core/src/lib.rs
git commit -m "feat(core): the settings palette, with contrast enforced by CI

Both themes as data, plus WCAG contrast and a test over every pair the
window actually draws. The hand-written first pass failed five of them --
a dark accent fill too light for white text, a card border invisible
against the window -- so the check belongs somewhere that runs on every
job, not in a script someone ran once."
```

---

## Task 2: Theme and backdrop resolution, as pure functions

**Files:**
- Modify: `crates/beckon-core/src/theme.rs`

**Interfaces:**
- Consumes: `Palette`, `LIGHT`, `DARK` from Task 1.
- Produces:
  - `pub enum Theme { Light, Dark, HighContrast }`
  - `pub fn Theme::palette(self) -> Option<&'static Palette>`
  - `pub struct ThemeInputs { pub high_contrast: bool, pub apps_use_light_theme: Option<u32> }`
  - `pub fn resolve(i: ThemeInputs) -> Theme`
  - `pub enum Backdrop { Mica, Alpha(u8), Opaque }`
  - `pub struct BackdropInputs { pub build: u32, pub high_contrast: bool, pub remote_session: bool, pub transparency_enabled: bool, pub mica_supported: bool }`
  - `pub fn backdrop(i: BackdropInputs) -> Backdrop`

**Why pure functions:** the ordering rules here are the whole feature, and none of them need Windows to be checked. High contrast outranking the registry, and *Transparency effects* being off forcing opacity, are both decisions a test can pin.

- [ ] **Step 1: Write the failing test**

Append to `crates/beckon-core/src/theme.rs`, above the existing `#[cfg(test)] mod tests`:

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Theme {
    Light,
    Dark,
    /// Reads `GetSysColor` on Windows, exactly as the window did before this
    /// design existed. There is no `Palette` for it, and that is the point:
    /// the branch cannot accidentally acquire a literal.
    HighContrast,
}

impl Theme {
    pub fn palette(self) -> Option<&'static Palette> {
        match self {
            Theme::Light => Some(&LIGHT),
            Theme::Dark => Some(&DARK),
            Theme::HighContrast => None,
        }
    }
}

/// What the OS reports. `apps_use_light_theme` is `None` when the registry
/// value is absent, which is the state of a fresh profile and means light.
#[derive(Clone, Copy, Debug)]
pub struct ThemeInputs {
    pub high_contrast: bool,
    pub apps_use_light_theme: Option<u32>,
}

pub fn resolve(i: ThemeInputs) -> Theme {
    // High contrast outranks the registry unconditionally. A user in high
    // contrast has asked the OS for specific colours; a palette of ours would
    // override exactly the thing they turned on.
    if i.high_contrast {
        return Theme::HighContrast;
    }
    match i.apps_use_light_theme {
        Some(0) => Theme::Dark,
        _ => Theme::Light,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Backdrop {
    /// Tier 1: DWM composites Mica behind an unpainted client area.
    Mica,
    /// Tier 2: one uniform alpha over the whole window.
    Alpha(u8),
    /// Tier 3: no transparency at all.
    Opaque,
}

/// Windows 11 22H2. `DWMWA_SYSTEMBACKDROP_TYPE` is ignored below this.
pub const MICA_MIN_BUILD: u32 = 22621;

/// The alpha for tier 2. 245/255 is visible against a busy wallpaper and
/// leaves text effectively solid.
pub const TIER2_ALPHA: u8 = 245;

#[derive(Clone, Copy, Debug)]
pub struct BackdropInputs {
    pub build: u32,
    pub high_contrast: bool,
    pub remote_session: bool,
    /// `Themes\Personalize\EnableTransparency`. False means the user turned
    /// transparency off in Settings.
    pub transparency_enabled: bool,
    /// Cleared by the caller once tier 1 has been shown not to work on this
    /// machine, so the decision has one home rather than two.
    pub mica_supported: bool,
}

pub fn backdrop(i: BackdropInputs) -> Backdrop {
    // Three refusals, each of which is correctness rather than taste.
    // High contrast: a translucent ground defeats the guaranteed contrast the
    // mode exists to provide. Remote session: every frame becomes a blend the
    // wire has to carry. Transparency off: the user already answered this
    // question in Settings.
    if i.high_contrast || i.remote_session || !i.transparency_enabled {
        return Backdrop::Opaque;
    }
    if i.mica_supported && i.build >= MICA_MIN_BUILD {
        return Backdrop::Mica;
    }
    Backdrop::Alpha(TIER2_ALPHA)
}
```

Add these tests inside the existing `mod tests`:

```rust
    fn ti(hc: bool, light: Option<u32>) -> ThemeInputs {
        ThemeInputs { high_contrast: hc, apps_use_light_theme: light }
    }

    #[test]
    fn registry_zero_is_dark_and_anything_else_is_light() {
        assert_eq!(resolve(ti(false, Some(0))), Theme::Dark);
        assert_eq!(resolve(ti(false, Some(1))), Theme::Light);
        // Absent on a fresh profile.
        assert_eq!(resolve(ti(false, None)), Theme::Light);
    }

    #[test]
    fn high_contrast_outranks_the_registry_both_ways() {
        assert_eq!(resolve(ti(true, Some(0))), Theme::HighContrast);
        assert_eq!(resolve(ti(true, Some(1))), Theme::HighContrast);
        assert_eq!(resolve(ti(true, None)), Theme::HighContrast);
    }

    #[test]
    fn high_contrast_has_no_palette_so_no_literal_can_reach_it() {
        assert!(Theme::HighContrast.palette().is_none());
        assert!(Theme::Light.palette().is_some());
        assert!(Theme::Dark.palette().is_some());
    }

    fn bi(build: u32) -> BackdropInputs {
        BackdropInputs {
            build,
            high_contrast: false,
            remote_session: false,
            transparency_enabled: true,
            mica_supported: true,
        }
    }

    #[test]
    fn mica_needs_22h2() {
        assert_eq!(backdrop(bi(MICA_MIN_BUILD)), Backdrop::Mica);
        assert_eq!(backdrop(bi(MICA_MIN_BUILD - 1)), Backdrop::Alpha(TIER2_ALPHA));
        // Windows 10 21H2.
        assert_eq!(backdrop(bi(19044)), Backdrop::Alpha(TIER2_ALPHA));
    }

    #[test]
    fn a_hardware_failure_demotes_to_tier_two_without_touching_the_build() {
        let i = BackdropInputs { mica_supported: false, ..bi(26200) };
        assert_eq!(backdrop(i), Backdrop::Alpha(TIER2_ALPHA));
    }

    #[test]
    fn three_conditions_force_opaque_even_on_a_capable_build() {
        let capable = bi(26200);
        assert_eq!(
            backdrop(BackdropInputs { high_contrast: true, ..capable }),
            Backdrop::Opaque
        );
        assert_eq!(
            backdrop(BackdropInputs { remote_session: true, ..capable }),
            Backdrop::Opaque
        );
        assert_eq!(
            backdrop(BackdropInputs { transparency_enabled: false, ..capable }),
            Backdrop::Opaque
        );
    }

    /// Opaque wins over Mica, not the other way round. Written as its own test
    /// because an `if` reordered during a refactor would still pass every test
    /// above.
    #[test]
    fn refusals_are_checked_before_capability() {
        let i = BackdropInputs {
            high_contrast: true,
            transparency_enabled: false,
            ..bi(26200)
        };
        assert_eq!(backdrop(i), Backdrop::Opaque);
    }
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p beckon-core theme`
Expected: PASS for the Task 1 tests, and the new ones compile and pass — because Step 1 added both the implementation and the tests in one edit. **Before running, delete the bodies** of `resolve` and `backdrop`, replacing each with `todo!()`, and confirm the run fails with a panic naming `not yet implemented`. Then restore them.

- [ ] **Step 3: Restore the implementations**

Put the `resolve` and `backdrop` bodies from Step 1 back.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p beckon-core theme`
Expected: PASS, 10 tests.

- [ ] **Step 5: Commit**

```bash
git add crates/beckon-core/src/theme.rs
git commit -m "feat(core): resolve the theme and the backdrop tier as pure functions

Two orderings carry the feature and neither needs Windows to check: high
contrast outranks the registry, and the three refusals (high contrast,
remote session, transparency turned off in Settings) are checked before
capability, so a capable build cannot argue its way past them.

HighContrast has no Palette. That is deliberate -- the branch cannot
accidentally acquire a literal."
```

---

## Task 3: Split `settings_window.rs` into a directory

**Files:**
- Create: `crates/beckon-windows/src/settings_window/mod.rs`, `theme.rs`, `chrome.rs`, `paint.rs`, `layout.rs`
- Delete: `crates/beckon-windows/src/settings_window.rs`

**Interfaces:**
- Consumes: nothing new.
- Produces: the same public surface `settings_window` has today — no item's path outside the module may change. `crates/beckon-windows/src/lib.rs` keeps `mod settings_window;` untouched.

**This task changes no behaviour.** It is a pure move, done first so the risky code in Tasks 7–13 lands in files small enough to hold in context. `settings_window.rs` is 6689 lines today and would clear 8000 otherwise.

- [ ] **Step 1: Record the baseline**

Run and save the output:

```bash
cargo check --target aarch64-pc-windows-msvc --all-targets 2>&1 | tail -20
cargo test -p beckon-core -p beckon-cli 2>&1 | tail -20
```

Expected: clean. This is the "after" you must match.

- [ ] **Step 2: Create the directory and move the file**

```bash
mkdir -p crates/beckon-windows/src/settings_window
git mv crates/beckon-windows/src/settings_window.rs \
       crates/beckon-windows/src/settings_window/mod.rs
```

- [ ] **Step 3: Verify the move alone compiles**

Run: `cargo check --target aarch64-pc-windows-msvc --all-targets`
Expected: identical to Step 1. Rust treats `foo/mod.rs` and `foo.rs` the same, so nothing should change.

- [ ] **Step 4: Extract `layout.rs`**

Move `unsafe fn layout`, `LayoutHandles`, and the `mod tok` block into `crates/beckon-windows/src/settings_window/layout.rs`. At the top of that file:

```rust
//! Where every control goes. Behaviour is unchanged by the split that created
//! this file; see the module header in `mod.rs` for the rules `layout` obeys,
//! and note the one that matters most: `layout` calls `SetWindowPos` on the
//! populated App combo, which is the measured data-loss path. Nothing may add
//! a new call site for it on a keystroke path.

use super::*;
```

In `mod.rs`, add `mod layout;` and `use layout::*;` so every existing call site resolves unchanged. Make `tok`, `layout` and `LayoutHandles` `pub(super)` or `pub(crate)` as the compiler demands — take the narrowest visibility that builds.

- [ ] **Step 5: Verify**

Run: `cargo check --target aarch64-pc-windows-msvc --all-targets && cargo fmt --all -- --check`
Expected: clean, no warnings.

- [ ] **Step 6: Extract `paint.rs`**

Move the existing drawing helpers — `draw_chip`, `draw_keycaps`, the flag-pill drawing added in `ef8140c`, the `WM_DRAWITEM` button painter, and the ListView `NM_CUSTOMDRAW` handler body — into `paint.rs` with the same `use super::*;` header. Leave the *message dispatch* in `mod.rs`; only the painting moves.

- [ ] **Step 7: Create the two empty modules**

`theme.rs`:

```rust
//! The OS's answer to "which theme", and the GDI objects that answer costs.
//! Filled in by Task 4.

use super::*;
```

`chrome.rs`:

```rust
//! The client-drawn title bar. Filled in by Task 7.

use super::*;
```

Add `mod theme;` and `mod chrome;` to `mod.rs`.

- [ ] **Step 8: Verify the whole gate**

Run:

```bash
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo check --target aarch64-pc-windows-msvc --all-targets
cargo test
```

Expected: all clean, and byte-identical output to Step 1 for the check. If `clippy` now reports a lint that the single file hid, fix it here rather than carrying it forward.

- [ ] **Step 9: Commit**

```bash
git add -A crates/beckon-windows/src
git commit -m "refactor(windows): settings_window becomes a directory

A pure move, no behaviour change, done before the redesign so the risky
new code lands in files small enough to read. 6689 lines in one file was
already past comfortable and the theme work would clear 8000.

layout and its tokens, and the existing paint helpers, get their own
files; theme.rs and chrome.rs are stubs for tasks 4 and 7."
```

---

## Task 4: Read the theme from the OS, and cache the GDI objects it implies

**Files:**
- Modify: `crates/beckon-windows/src/settings_window/theme.rs`
- Modify: `crates/beckon-windows/src/settings_window/mod.rs`
- Modify: `crates/beckon-windows/Cargo.toml`

**Interfaces:**
- Consumes: `beckon_core::theme::{Theme, ThemeInputs, resolve, Palette}` from Tasks 1–2.
- Produces:
  - `pub(super) fn colorref(rgb: u32) -> COLORREF`
  - `pub(super) struct ThemeCache` with `pub(super) fn rebuild(&mut self, t: Theme)`, `pub(super) fn theme(&self) -> Theme`, `pub(super) fn brush(&self, rgb: u32) -> HBRUSH`, and `pub(super) fn col(&self, pick: impl Fn(&Palette) -> u32, sys: SYS_COLOR_INDEX) -> COLORREF`
  - `pub(super) fn read_inputs() -> ThemeInputs`
  - `pub(super) fn apply_dwm_dark(hwnd: HWND, dark: bool)`

**The `col` signature is the whole design.** Every call site names *both* the palette token and the `GetSysColor` index to use in high contrast, so the third branch cannot be forgotten at any one site. There is no way to write a call that reads a literal and has no high-contrast answer.

- [ ] **Step 1: Add the two `windows` features**

In `crates/beckon-windows/Cargo.toml`, inside the existing feature list, add with the comment:

```toml
    # `SetWindowTheme` for the ListView's scrollbar in dark mode (task 10).
    # A public exported function, NOT one of the uxtheme ordinals the
    # 2026-08-11 spec rejected -- though the "DarkMode_Explorer" class name is
    # itself undocumented, and the call degrades silently on builds that do
    # not know it.
    "Win32_UI_Controls",
```

`Win32_UI_Controls` is already present — verify before adding, and if so add only:

```toml
    # `RtlGetVersion`, for the build number the Mica tier check needs.
    "Wdk_System_SystemServices",
```

- [ ] **Step 2: Write the `COLORREF` test**

There is one genuinely testable thing here and it is the classic bug: **`COLORREF` is `0x00BBGGRR`, not RGB.** Add to `crates/beckon-core/src/theme.rs`'s test module:

```rust
    /// COLORREF is 0x00BBGGRR. The window converts at its boundary, but the
    /// swap is easy to write the wrong way round and produces a plausible
    /// wrong colour rather than an obvious one -- beckon's blue #2563EB comes
    /// back as a muddy teal.
    #[test]
    fn the_bgr_swap_is_documented_by_a_case() {
        fn to_colorref(rgb: u32) -> u32 {
            ((rgb & 0xFF) << 16) | (rgb & 0xFF00) | ((rgb >> 16) & 0xFF)
        }
        assert_eq!(to_colorref(0x2563EB), 0x00EB6325);
        assert_eq!(to_colorref(0xFFFFFF), 0x00FFFFFF);
        assert_eq!(to_colorref(0x000000), 0x00000000);
        // Not a palindrome, so a no-op implementation fails.
        assert_ne!(to_colorref(LIGHT.accent), LIGHT.accent);
    }
```

- [ ] **Step 3: Run it to verify it fails, then passes**

Run: `cargo test -p beckon-core the_bgr_swap`
Expected: PASS immediately — the helper is defined inside the test. Now break it deliberately by changing `<< 16` to `<< 8`, re-run, and confirm FAIL. Restore.

This is the control: without it, a passing test and an absent test look the same.

- [ ] **Step 4: Implement `theme.rs`**

```rust
//! The OS's answer to "which theme", and the GDI objects that answer costs.

use super::*;
use beckon_core::theme::{self as core_theme, Palette, Theme, ThemeInputs};
use std::collections::HashMap;

/// `0xRRGGBB` to Win32's `0x00BBGGRR`.
pub(super) fn colorref(rgb: u32) -> COLORREF {
    COLORREF(((rgb & 0xFF) << 16) | (rgb & 0xFF00) | ((rgb >> 16) & 0xFF))
}

/// Ask Windows what it wants, as plain data for `core_theme::resolve`.
pub(super) fn read_inputs() -> ThemeInputs {
    let mut hc = HIGHCONTRASTW {
        cbSize: std::mem::size_of::<HIGHCONTRASTW>() as u32,
        ..Default::default()
    };
    let high_contrast = unsafe {
        SystemParametersInfoW(
            SPI_GETHIGHCONTRAST,
            hc.cbSize,
            Some(&mut hc as *mut _ as *mut _),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        )
        .is_ok()
            && hc.dwFlags.contains(HCF_HIGHCONTRASTON)
    };
    ThemeInputs { high_contrast, apps_use_light_theme: read_apps_use_light() }
}

/// `HKCU\...\Themes\Personalize\AppsUseLightTheme`. `None` when absent, which
/// is a fresh profile and means light.
fn read_apps_use_light() -> Option<u32> {
    read_personalize_dword("AppsUseLightTheme")
}

/// `EnableTransparency` from the same key, for the backdrop tier.
pub(super) fn read_transparency_enabled() -> bool {
    // Absent means on: transparency is the Windows default.
    read_personalize_dword("EnableTransparency") != Some(0)
}

fn read_personalize_dword(name: &str) -> Option<u32> {
    unsafe {
        let mut key = HKEY::default();
        let path = w!(
            "Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize"
        );
        if RegOpenKeyExW(HKEY_CURRENT_USER, path, Some(0), KEY_READ, &mut key)
            .is_err()
        {
            return None;
        }
        let mut value: u32 = 0;
        let mut size = std::mem::size_of::<u32>() as u32;
        let wide: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
        let ok = RegQueryValueExW(
            key,
            PCWSTR(wide.as_ptr()),
            None,
            None,
            Some(&mut value as *mut _ as *mut u8),
            Some(&mut size),
        )
        .is_ok();
        let _ = RegCloseKey(key);
        ok.then_some(value)
    }
}

/// Tell DWM which way the frame, border and shadow should lean. Needed even
/// with a client-drawn caption: the window BORDER is DWM's, not ours.
pub(super) fn apply_dwm_dark(hwnd: HWND, dark: bool) {
    const DWMWA_USE_IMMERSIVE_DARK_MODE: DWMWINDOWATTRIBUTE =
        DWMWINDOWATTRIBUTE(20);
    let on: BOOL = dark.into();
    unsafe {
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_USE_IMMERSIVE_DARK_MODE,
            &on as *const _ as *const _,
            std::mem::size_of::<BOOL>() as u32,
        );
    }
}

/// The current theme plus one solid brush per colour actually used.
///
/// Brushes are cached because a repaint of the list alone asks for the same
/// half-dozen colours once per row, and `CreateSolidBrush` per row per paint
/// is the kind of cost that only shows up on the slowest machine someone owns.
#[derive(Default)]
pub(super) struct ThemeCache {
    theme: Option<Theme>,
    brushes: HashMap<u32, HBRUSH>,
}

impl ThemeCache {
    pub(super) fn theme(&self) -> Theme {
        self.theme.unwrap_or(Theme::Light)
    }

    /// Swap the theme and drop every brush built for the old one.
    ///
    /// Returns true when the theme actually changed, so the caller can skip
    /// the invalidate. `WM_SETTINGCHANGE` fires for a great many things that
    /// are not the colour scheme.
    pub(super) fn rebuild(&mut self, t: Theme) -> bool {
        if self.theme == Some(t) {
            return false;
        }
        self.free();
        self.theme = Some(t);
        true
    }

    fn free(&mut self) {
        for (_, b) in self.brushes.drain() {
            unsafe {
                let _ = DeleteObject(HGDIOBJ(b.0));
            }
        }
    }

    /// A colour, named twice: once as a palette token and once as the
    /// `GetSysColor` index that stands in for it under high contrast.
    ///
    /// Both arguments are mandatory, which is what makes the third branch
    /// impossible to forget at a call site.
    pub(super) fn col(
        &self,
        pick: impl Fn(&Palette) -> u32,
        sys: SYS_COLOR_INDEX,
    ) -> COLORREF {
        match self.theme().palette() {
            Some(p) => colorref(pick(p)),
            None => COLORREF(unsafe { GetSysColor(sys) }),
        }
    }

    /// A cached solid brush for a resolved `COLORREF`.
    ///
    /// Never returns a system brush, so every handle here is ours to delete
    /// and `free` cannot leak or double-free one of Windows'.
    pub(super) fn brush(&mut self, c: COLORREF) -> HBRUSH {
        *self
            .brushes
            .entry(c.0)
            .or_insert_with(|| unsafe { CreateSolidBrush(c) })
    }
}

impl Drop for ThemeCache {
    fn drop(&mut self) {
        self.free();
    }
}
```

- [ ] **Step 5: Wire it into the window**

In `mod.rs`:

1. Add `theme: theme::ThemeCache` to the `Ui` struct, `Default`-initialised.
2. In the window-creation path, immediately after `CreateWindowExW` succeeds:

```rust
let t = beckon_core::theme::resolve(theme::read_inputs());
ui.theme.rebuild(t);
theme::apply_dwm_dark(hwnd, t == beckon_core::theme::Theme::Dark);
```

3. Add the two message arms to the wndproc:

```rust
// The live light/dark flip. lParam names WHICH setting changed and most of
// them are not ours -- comparing it is what keeps a mouse-speed change from
// rebuilding every brush in the window.
WM_SETTINGCHANGE => {
    if is_immersive_colour_set(lparam) {
        on_theme_changed(hwnd);
    }
    DefWindowProcW(hwnd, msg, wparam, lparam)
}
// High contrast on or off.
WM_THEMECHANGED => {
    on_theme_changed(hwnd);
    DefWindowProcW(hwnd, msg, wparam, lparam)
}
```

4. And the two helpers:

```rust
fn is_immersive_colour_set(lparam: LPARAM) -> bool {
    if lparam.0 == 0 {
        return false;
    }
    let p = PCWSTR(lparam.0 as *const u16);
    unsafe { p.to_string().map(|s| s == "ImmersiveColorSet").unwrap_or(false) }
}

/// Re-resolve, rebuild, repaint. **Never calls `layout`** -- no colour change
/// moves a control, and `layout` means `SetWindowPos` on the populated App
/// combo, which is the measured data-loss path.
unsafe fn on_theme_changed(hwnd: HWND) {
    let t = beckon_core::theme::resolve(theme::read_inputs());
    let changed = UI.with(|u| {
        u.borrow_mut()
            .as_mut()
            .map(|ui| ui.theme.rebuild(t))
            .unwrap_or(false)
    });
    if !changed {
        return;
    }
    theme::apply_dwm_dark(hwnd, t == beckon_core::theme::Theme::Dark);
    let _ = InvalidateRect(Some(hwnd), None, true);
}
```

**The `UI.with` borrow is taken and dropped on one expression.** `InvalidateRect` re-enters the wndproc, and a second `RefCell` borrow across an `extern "system"` boundary aborts the process instead of unwinding — the rule `layout` already documents.

- [ ] **Step 6: Verify**

Run:

```bash
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo check --target aarch64-pc-windows-msvc --all-targets
cargo test
```

Expected: clean. **This task has no runtime test** — nothing here can be exercised without Windows. Its gate is the compile, plus hardware gate 07 in Task 15.

- [ ] **Step 7: Commit**

```bash
git add crates/beckon-windows/src/settings_window/theme.rs \
        crates/beckon-windows/src/settings_window/mod.rs \
        crates/beckon-windows/Cargo.toml \
        crates/beckon-core/src/theme.rs
git commit -m "feat(windows): resolve and cache the theme, and follow the OS live

ThemeCache::col takes BOTH a palette token and a GetSysColor index, so the
high-contrast branch cannot be forgotten at a call site -- there is no way
to spell a call that reads a literal and has no answer for the third
branch.

WM_SETTINGCHANGE compares lParam against ImmersiveColorSet; without that a
mouse-speed change rebuilds every brush in the window. The handler never
calls layout: no colour moves a control, and layout means SetWindowPos on
the populated App combo.

Adds a test for the 0x00BBGGRR swap, which produces a plausible wrong
colour rather than an obvious one when written backwards."
```

---

## Task 5: Route every existing colour through the cache

**Files:**
- Modify: `crates/beckon-windows/src/settings_window/paint.rs`
- Modify: `crates/beckon-windows/src/settings_window/mod.rs`

**Interfaces:**
- Consumes: `ThemeCache::col`, `ThemeCache::brush` from Task 4.
- Produces: no new items. Every `GetSysColor` / `GetSysColorBrush` call in the drawing code is replaced by a `col` / `brush` pair.

**This is the task that makes dark mode visible.** Shape is unchanged; only the source of each colour moves.

- [ ] **Step 1: Enumerate the call sites**

Run:

```bash
grep -n 'GetSysColor\|GetSysColorBrush\|CreateSolidBrush' \
  crates/beckon-windows/src/settings_window/*.rs
```

Expected: roughly 30 hits across `paint.rs` and `mod.rs`, at the line numbers the spec's survey found (the keycap painter, the chip painter, the flag pill, the `Save` accent, the list `NM_CUSTOMDRAW`, and the `WM_CTLCOLOR*` arms). Write the list down — Step 4 checks it reaches zero.

- [ ] **Step 2: Replace them, one drawing function at a time**

The mapping, which is the substance of this task:

| Was | Becomes |
|---|---|
| `GetSysColor(COLOR_BTNFACE)` as a window ground | `col(\|p\| p.bg, COLOR_BTNFACE)` |
| `GetSysColor(COLOR_WINDOW)` as a card/list ground | `col(\|p\| p.card, COLOR_WINDOW)` |
| `GetSysColor(COLOR_WINDOWTEXT)` | `col(\|p\| p.text, COLOR_WINDOWTEXT)` |
| `GetSysColor(COLOR_GRAYTEXT)` | `col(\|p\| p.text_faint, COLOR_GRAYTEXT)` |
| `GetSysColor(COLOR_BTNSHADOW)` on a keycap edge | `col(\|p\| p.keycap_edge, COLOR_BTNSHADOW)` |
| `GetSysColor(COLOR_BTNSHADOW)` on a disabled chip | `col(\|p\| p.text_faint, COLOR_BTNSHADOW)` |
| `GetSysColor(COLOR_HIGHLIGHT)` on an armed chip or `Save` | `col(\|p\| p.accent_fill, COLOR_HIGHLIGHT)` |
| `GetSysColor(COLOR_HIGHLIGHTTEXT)` | `col(\|p\| p.accent_on, COLOR_HIGHLIGHTTEXT)` |
| a selected list row's fill | `col(\|p\| p.accent_soft, COLOR_HIGHLIGHT)` |

**Two of those are not mechanical and must not be batch-replaced.**
`COLOR_BTNSHADOW` appears in two roles — a keycap's bottom edge and a
disabled chip's text — and they resolve to different tokens. Handle each by
hand.

The flag pill's tone-by-word logic from `ef8140c` keeps its shape; only its
colours change, to `bad_bg`/`bad` and `warn_bg`/`warn`.

- [ ] **Step 3: Make `GetSysColorBrush` illegal in this module**

`GetSysColorBrush` returns a brush owned by Windows that must never be
deleted, while `ThemeCache::brush` returns one that must be. Mixing them is
how a double-free gets in. Delete every `GetSysColorBrush` call — the cache
handles the high-contrast branch itself — and add at the top of `paint.rs`:

```rust
// GetSysColorBrush must not appear in this file. It returns a brush owned by
// Windows, while ThemeCache::brush returns one owned by us; a call site that
// cannot tell them apart is a double-free waiting for a theme switch. Every
// colour goes through `col`, which answers the high-contrast branch itself.
```

- [ ] **Step 4: Verify the sweep is complete**

Run:

```bash
grep -n 'GetSysColor' crates/beckon-windows/src/settings_window/paint.rs
```

Expected: **no output**. Every remaining `GetSysColor` in the crate should
live inside `theme.rs`'s `col`.

Run:

```bash
grep -rn 'GetSysColor' crates/beckon-windows/src/settings_window/ | grep -v 'theme.rs'
```

Expected: no output, or only occurrences inside the `SYS_COLOR_INDEX`
arguments passed to `col`.

- [ ] **Step 5: Full gate**

```bash
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo check --target aarch64-pc-windows-msvc --all-targets
cargo test
```

- [ ] **Step 6: Commit**

```bash
git add crates/beckon-windows/src/settings_window
git commit -m "feat(windows): every drawn colour comes from the theme cache

Shape unchanged; only the source of each colour moves. GetSysColorBrush is
banned from paint.rs outright -- it returns a brush Windows owns while the
cache returns one we own, and a site that cannot tell them apart is a
double-free waiting for a theme switch.

COLOR_BTNSHADOW was two roles wearing one name -- a keycap's bottom edge
and a disabled chip's text -- and they resolve to different tokens. Done
by hand, not by sed."
```

---

## Task 6: The type ramp

**Files:**
- Modify: `crates/beckon-windows/src/settings_window/mod.rs`

**Interfaces:**
- Consumes: the existing `make_font`, `Role`, and `Fonts` from the pre-split file.
- Produces: three new `Role` variants — `Role::Title`, `Role::BodyStrong`, `Role::Chrome` — resolvable through the existing `Fonts::get`.

- [ ] **Step 1: Add the roles**

Extend the `Role` enum and the `Fonts` builder:

```rust
// Faces are spelled EXACTLY as measured on a14 2026-08-11. lfFaceName holds
// 32 wchar and the truncation is NOT uniform:
//
//   Segoe UI Variable Text Semibold     <- fits, 31 chars
//   Segoe UI Variable Display Semib     <- truncated
//   Segoe UI Variable Small Semibol     <- truncated
//
// A wrong spelling does not fail. CreateFontW succeeds and hands back Arial.
// `make_font`'s GetTextFace round-trip is what catches it, and it is the
// reason this table is written out rather than generated.
Role::Title      => make_font(hwnd, base, "Segoe UI Variable Display Semib", 15, 600, dpi),
Role::Subtitle   => make_font(hwnd, base, "Segoe UI Variable Text Semibold", 18, 600, dpi),
Role::BodyStrong => make_font(hwnd, base, "Segoe UI Variable Text Semibold", 14, 600, dpi),
Role::Body       => make_font(hwnd, base, "Segoe UI Variable Text",          14, 400, dpi),
Role::Caption    => make_font(hwnd, base, "Segoe UI Variable Small",         12, 400, dpi),
Role::Keycap     => make_font(hwnd, base, "Segoe UI Variable Small Semibol", 11, 600, dpi),
Role::Chrome     => make_font(hwnd, base, "Segoe Fluent Icons",              10, 400, dpi),
```

`Subtitle` drops from 20 px to 18 — an 18 px Semibold heading is Win11's own
proportion for a card head, and 20 fought the 14 px body around it.

- [ ] **Step 2: Assign the new roles**

- `Role::BodyStrong` — every card caption, both ListView column headers, and the `Save` caption.
- `Role::Title` — the title-bar app name (used in Task 7).
- `Role::Chrome` — the two caption-button glyphs (Task 7).

- [ ] **Step 3: Verify**

```bash
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo check --target aarch64-pc-windows-msvc --all-targets
```

**No runtime test.** Whether a face resolved is only visible through
`WM_GETFONT` + `GetObjectW` on hardware — gate 08 in Task 15, which reads
`lfFaceName` back per control and fails any role that reports plain
`Segoe UI`.

- [ ] **Step 4: Commit**

```bash
git add crates/beckon-windows/src/settings_window/mod.rs
git commit -m "feat(windows): add Title, BodyStrong and Chrome to the type ramp

Faces spelled exactly as measured -- Text Semibold fits in lfFaceName at 31
chars while Display and Small are cut, so one table cannot be derived from
another. A wrong spelling returns Arial without an error.

Subtitle drops 20 -> 18: Win11's own proportion for a card head, and 20
fought the 14 body around it."
```

---

## Task 7: The client-drawn title bar

**Files:**
- Modify: `crates/beckon-windows/src/settings_window/chrome.rs`
- Modify: `crates/beckon-windows/src/settings_window/mod.rs`

**Interfaces:**
- Consumes: `ThemeCache`, `Role::Title`, `Role::Caption`, `Role::Chrome`.
- Produces:
  - `pub(super) const TITLEBAR_H: i32 = 40;`
  - `pub(super) fn nccalcsize(hwnd: HWND, wparam: WPARAM, lparam: LPARAM) -> LRESULT`
  - `pub(super) fn nchittest(hwnd: HWND, pt: POINT) -> Option<LRESULT>`
  - `pub(super) fn paint(hwnd: HWND, hdc: HDC, cache: &mut ThemeCache, fonts: &Fonts, dpi: u32, hot: Option<i32>)`
  - `pub(super) fn hit_button(hwnd: HWND, pt: POINT, dpi: u32) -> Option<i32>` returning `HTCLOSE` or `HTMINBUTTON`

- [ ] **Step 1: Remove `WS_MAXIMIZEBOX`**

In `mod.rs`'s `CreateWindowExW`, change `WS_OVERLAPPEDWINDOW` to:

```rust
// WS_OVERLAPPEDWINDOW minus WS_MAXIMIZEBOX. Dropping maximize is
// LOAD-BEARING, not cosmetic: it removes the HTMAXBUTTON / Snap Layouts
// obligation AND makes the maximized state -- where WM_NCCALCSIZE overflows
// the monitor by the frame thickness unless corrected by hand -- unreachable.
// The window is still resizable by its edges; `layout` already handles that.
WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_THICKFRAME | WS_MINIMIZEBOX,
```

- [ ] **Step 2: Round the corners**

After creation:

```rust
const DWMWA_WINDOW_CORNER_PREFERENCE: DWMWINDOWATTRIBUTE = DWMWINDOWATTRIBUTE(33);
const DWMWCP_ROUND: u32 = 2;
unsafe {
    let pref = DWMWCP_ROUND;
    // No-op on Windows 10; the call returns an error we deliberately drop.
    let _ = DwmSetWindowAttribute(
        hwnd,
        DWMWA_WINDOW_CORNER_PREFERENCE,
        &pref as *const _ as *const _,
        std::mem::size_of::<u32>() as u32,
    );
}
```

- [ ] **Step 3: Implement `nccalcsize`**

```rust
/// Extend the client area over the caption, keeping the resize borders.
///
/// The maximized correction every other implementation of this needs is
/// absent because the state is unreachable: `WS_MAXIMIZEBOX` is off, so
/// neither the button nor Win+Up can produce it.
pub(super) fn nccalcsize(hwnd: HWND, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if wparam.0 == 0 {
        return unsafe { DefWindowProcW(hwnd, WM_NCCALCSIZE, wparam, lparam) };
    }
    let params = unsafe { &mut *(lparam.0 as *mut NCCALCSIZE_PARAMS) };
    let before = params.rgrc[0];
    let _ = unsafe { DefWindowProcW(hwnd, WM_NCCALCSIZE, wparam, lparam) };
    // Give the caption band back to the client. The side and bottom borders
    // stay whatever DefWindowProc made them, so resizing is untouched.
    params.rgrc[0].top = before.top;
    LRESULT(0)
}
```

- [ ] **Step 4: Implement hit testing**

```rust
/// `None` means "let DefWindowProc answer" -- which is what resolves the
/// eight resize borders, so they keep working without being restated here.
pub(super) fn nchittest(hwnd: HWND, pt: POINT) -> Option<LRESULT> {
    let dpi = unsafe { GetDpiForWindow(hwnd) }.max(96);
    let mut rc = RECT::default();
    if unsafe { GetWindowRect(hwnd, &mut rc) }.is_err() {
        return None;
    }
    let bar_h = rc.top + TITLEBAR_H * dpi as i32 / 96;
    if pt.y >= bar_h {
        return None;
    }
    // The resize border wins over the caption along the very top edge,
    // otherwise the window cannot be resized upward at all.
    let border = unsafe { GetSystemMetricsForDpi(SM_CYSIZEFRAME, dpi) };
    if pt.y < rc.top + border {
        return None;
    }
    if let Some(ht) = hit_button(hwnd, pt, dpi) {
        return Some(LRESULT(ht as isize));
    }
    Some(LRESULT(HTCAPTION as isize))
}
```

- [ ] **Step 5: Suppress double-click maximize**

```rust
// With no maximize box there is nothing for a caption double-click to do,
// and letting DefWindowProc try it is how the unreachable state gets reached.
WM_NCLBUTTONDBLCLK if wparam.0 as u32 == HTCAPTION => LRESULT(0),
```

- [ ] **Step 6: Paint the bar**

In `chrome::paint`, in this order: fill the band with `col(|p| p.bg, COLOR_BTNFACE)`; `DrawIconEx` the app icon at 18 px scaled, 14 px in; `Role::Title` for `beckon` in `col(|p| p.accent, COLOR_HIGHLIGHT)`; `Role::Caption` for `env!("CARGO_PKG_VERSION")` in `col(|p| p.text_faint, COLOR_GRAYTEXT)`; then the two 46 × 40 buttons right-aligned.

Glyphs, in `Role::Chrome`: `\u{E921}` minimize, `\u{E8BB}` close. Hover fill
for minimize is `col(|p| p.accent_soft, COLOR_HIGHLIGHT)`; for close it is the
literal `COLORREF(0x001C2BC4)` — that is `#C42B1C` in BGR — **except under
high contrast, where it must be `GetSysColor(COLOR_HIGHLIGHT)`**. This is the
one place a literal is correct, because Windows' own close button uses that
exact red regardless of accent colour, and it is guarded by the branch.

Track the hot button in `Ui` and repaint the bar on `WM_NCMOUSEMOVE` and
`WM_NCMOUSELEAVE`.

- [ ] **Step 7: Verify**

```bash
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo check --target aarch64-pc-windows-msvc --all-targets
```

**No runtime test.** Gates 02 and 06 in Task 15.

- [ ] **Step 8: Commit**

```bash
git add crates/beckon-windows/src/settings_window
git commit -m "feat(windows): a client-drawn title bar with the icon, name and version

Dropping WS_MAXIMIZEBOX is the load-bearing decision: no HTMAXBUTTON means
no Snap Layouts obligation, and an unreachable maximized state means the
WM_NCCALCSIZE monitor-overflow correction is not needed at all. The window
still resizes by its edges.

nchittest returns None for the top border strip so DefWindowProc keeps
answering the eight resize directions -- returning HTCAPTION there makes a
window that cannot be resized upward.

The close button's hover red is the one literal in the file, because
Windows uses that exact red regardless of accent -- and it is still behind
the high-contrast branch."
```

---

## Task 8: Cards, and the geometry that follows

**Files:**
- Modify: `crates/beckon-windows/src/settings_window/layout.rs`
- Modify: `crates/beckon-windows/src/settings_window/paint.rs`

**Interfaces:**
- Consumes: `ThemeCache`, `TITLEBAR_H`.
- Produces: `pub(super) fn card(hdc: HDC, rc: RECT, cache: &mut ThemeCache, dpi: u32)`, and the revised `tok` values below.

- [ ] **Step 1: Update the tokens**

```rust
// 900x740 at 96 DPI, up from 860x640. Three cards at 16 px inner padding need
// the room, and the budget below comes out exact rather than approximately:
//
//   title bar                                40
//   body padding                          2x 16
//     Shortcuts card   16+32+12+238+16 =    314
//     gap                                    12
//     editor card  16+20+10+30+10+30+12+40+16 = 194
//     gap                                    12
//     keyboard card    16+20+10+30+16 =      92
//     gap                                    12
//     command bar                            32
//                                          ----
//                                           740
const WINDOW_WIDTH: i32 = 900;
const WINDOW_HEIGHT: i32 = 740;

mod tok {
    pub const PAD: i32 = 16;
    /// Was BAND = 14. Cards carry their own 16 px inner padding, so the gap
    /// between them wants to be smaller than the old gap between bare bands.
    pub const GAP_CARD: i32 = 12;
    pub const GAP: i32 = 8;
    pub const LABEL: i32 = 12;
    pub const CTL: i32 = 32;
    pub const BTN: i32 = 88;
    pub const SHORTCUT_COL: i32 = 200;
    pub const CHIP_MIN: i32 = 46;
    /// Fixed at every DPI. Not scaled, not derived from the config.
    pub const ROWS: i32 = 8;
    pub const TOOLTIP_MAX: i32 = 420;
    /// New.
    pub const CARD_PAD: i32 = 16;
    pub const CARD_RADIUS: i32 = 10;
    pub const ROW_H: i32 = 26;
}
```

Update `WM_GETMINMAXINFO`'s floor proportionally.

- [ ] **Step 2: Draw a card**

```rust
/// A card: rounded fill plus a 1 px border. No drop shadow — Win11's own
/// cards use a border, and a GDI shadow costs a layered surface for an effect
/// nobody asked for.
pub(super) fn card(hdc: HDC, rc: RECT, cache: &mut ThemeCache, dpi: u32) {
    let r = tok::CARD_RADIUS * dpi as i32 / 96;
    let fill = cache.col(|p| p.card, COLOR_WINDOW);
    let edge = cache.col(|p| p.card_border, COLOR_BTNSHADOW);
    unsafe {
        let br = cache.brush(fill);
        let pen = CreatePen(PS_SOLID, 1, edge);
        let old_br = SelectObject(hdc, HGDIOBJ(br.0));
        let old_pen = SelectObject(hdc, HGDIOBJ(pen.0));
        // RoundRect strokes with the pen AND fills with the brush in one call,
        // so the border lands exactly on the fill's edge with no seam.
        let _ = RoundRect(hdc, rc.left, rc.top, rc.right, rc.bottom, r * 2, r * 2);
        SelectObject(hdc, old_br);
        SelectObject(hdc, old_pen);
        // The pen is ours; the brush belongs to the cache and must NOT be
        // deleted here.
        let _ = DeleteObject(HGDIOBJ(pen.0));
    }
}
```

- [ ] **Step 3: Restructure `layout` into three cards**

Bands 2+3 merge into card 1 (head row plus list), band 4 becomes card 2, band
5 becomes card 3, band 6 stays the anchored command bar. Band 1 (the banner)
becomes a fourth card above card 1 and still contributes no height when
hidden.

Every band's `y` now starts `CARD_PAD` inside its card rather than at the
window padding. Keep `place`, `place_h`, `clamp` and the `.max(cx)` rule
exactly as they are — those comments describe live hazards.

- [ ] **Step 4: Paint the cards in `WM_PAINT` before the controls**

Card rects are computed by the same arithmetic `layout` uses, so factor that
into `pub(super) fn card_rects(hwnd: HWND) -> [RECT; 4]` called by both. Two
copies of this arithmetic would drift and the drift would look like a
rendering bug.

- [ ] **Step 5: Verify**

```bash
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo check --target aarch64-pc-windows-msvc --all-targets
```

Gates 03 and 09 in Task 15.

- [ ] **Step 6: Commit**

```bash
git add crates/beckon-windows/src/settings_window
git commit -m "feat(windows): the bands become cards, and the window grows to 900x740

The height budget comes out exact rather than approximately; it is written
into the token comment so the next person to add a row can see what they
are spending.

card_rects is shared by layout and WM_PAINT. Two copies of that arithmetic
would drift, and the drift would look like a rendering bug rather than a
duplication one."
```

---

## Task 9: Buttons and fields

**Files:**
- Modify: `crates/beckon-windows/src/settings_window/paint.rs`
- Modify: `crates/beckon-windows/src/settings_window/mod.rs`

**Interfaces:**
- Consumes: `ThemeCache`, `card`.
- Produces: `pub(super) enum BtnTier { Accent, Secondary, Outline, Danger }` and `pub(super) fn button(di: &DRAWITEMSTRUCT, tier: BtnTier, cache: &mut ThemeCache, dpi: u32)`.

- [ ] **Step 1: The three tiers**

| Tier | Controls | Fill | Border | Text |
|---|---|---|---|---|
| `Accent` | `Save` | `accent_fill` | none | `accent_on`, BodyStrong |
| `Secondary` | `Add`, `Remove`, `Reload`, `Open config file`, `Close`, `Keep mine` | `field` | `field_border` | `text` |
| `Outline` | `Record`, `Reset` | none | `accent` | `accent`, height 26 |
| `Danger` | `Stop` | none | `bad` | `bad`, height 26 |

Radius 6. Disabled in every tier: `field` fill, `field_border` edge,
`text_faint` text. Focus: a 2 px `accent` ring inset 2 px, drawn last.

**`Stop` keeps its narrower caption.** `Danger` differs from `Outline` only in
colour, never in metrics, so the armed caption cannot become wider than
`Record` and force `layout` onto the capture path.

- [ ] **Step 2: Fields keep their insides**

`IDC_APP` and `IDC_FILTER` are **not** owner-drawn. Remove `WS_BORDER` from
each, answer `WM_CTLCOLOREDIT` and `WM_CTLCOLORLISTBOX` with the themed
background brush and `SetTextColor`, and draw a rounded 1 px border around
each control's rect from the parent's `WM_PAINT`:

```rust
// Round the control's own rect outward by 1 and stroke it from the PARENT.
// Nothing reaches inside the control -- that is where the measured data-loss
// defect lives, and an owner-drawn CBS_DROPDOWN with an edit child is exactly
// the shape that produced it.
pub(super) fn field_border(hdc: HDC, ctl: HWND, parent: HWND,
                           cache: &mut ThemeCache, focused: bool, dpi: u32)
```

- [ ] **Step 3: The two `DROPDOWNLIST`s are safe to own**

`IDC_COMBO` and `IDC_TAP` have no edit child and are read by index, so give
them `CBS_OWNERDRAWFIXED` and a `WM_DRAWITEM` arm.

**`CBS_SORT` must not appear.** Re-read the creation call and confirm.

- [ ] **Step 4: Verify**

```bash
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo check --target aarch64-pc-windows-msvc --all-targets
grep -n 'CBS_SORT' crates/beckon-windows/src/settings_window/mod.rs
```

Expected: the grep prints **nothing**.

Gate 04 in Task 15.

- [ ] **Step 5: Commit**

```bash
git add crates/beckon-windows/src/settings_window
git commit -m "feat(windows): three button tiers, and fields bordered from outside

The App combo and the filter box keep their insides: colours via
WM_CTLCOLOR*, WS_BORDER off, and a rounded border stroked from the parent.
An owner-drawn CBS_DROPDOWN with an edit child is the exact shape that
produced the measured data-loss defect, and this design never reaches in.

IDC_COMBO and IDC_TAP have no edit child and are read by index, so they
are owner-drawn fully. Danger differs from Outline only in colour, never
in metrics, so Stop cannot grow wider than Record."
```

---

## Task 10: The list

**Files:**
- Modify: `crates/beckon-windows/src/settings_window/paint.rs`
- Modify: `crates/beckon-windows/src/settings_window/mod.rs`

**Interfaces:**
- Consumes: `ThemeCache`, `Role::BodyStrong`.
- Produces: no new public items.

- [ ] **Step 1: 26 px rows**

A ListView takes its row height from its image list, so size the existing
state image list (the one holding the tick images) to 26 px:

```rust
// A ListView's row height comes from its image list, so the state list the
// per-row ticks already ride in is the lever. The tick images themselves stay
// 15 px and are centred inside the taller cell.
let il = ImageList_Create(s(16), s(tok::ROW_H), ILC_COLOR32, 2, 0);
```

Gate 05 checks the tick still centres.

- [ ] **Step 2: Remove `WS_BORDER`**

The card is the border now. Also set `LVS_EX_DOUBLEBUFFER`.

- [ ] **Step 3: Custom-draw the header**

Handle `NM_CUSTOMDRAW` from the header control: `card` ground, `text_muted`
in `Role::BodyStrong`, a 1 px `divider` along the bottom, no sort arrows.

- [ ] **Step 4: Row states**

Selected: `accent_soft` fill plus a 2 px `accent` bar down the left edge — not
a full accent fill, which would fight the keycaps and the status pill for the
same cell. Hover: `accent_soft` at reduced weight. No zebra striping, no grid
lines.

**`uItemState` claims every row is selected at the SUBITEM stage.** Read the
selection from `LVM_GETITEMSTATE` at the ITEM stage and carry it forward — the
existing custom-draw code already knows this and its comment must survive the
edit.

- [ ] **Step 5: The dark scrollbar**

```rust
// A public exported function, NOT one of the uxtheme ordinals the 2026-08-11
// spec rejected. The theme class name is undocumented and the call degrades
// silently on builds that do not know it, which is why nothing downstream
// depends on it having worked.
let _ = SetWindowTheme(list, w!("DarkMode_Explorer"), None);
```

Called on every theme change, with `w!("Explorer")` for light.

- [ ] **Step 6: Verify**

```bash
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo check --target aarch64-pc-windows-msvc --all-targets
cargo test
```

Gates 01, 05 in Task 15.

- [ ] **Step 7: Commit**

```bash
git add crates/beckon-windows/src/settings_window
git commit -m "feat(windows): the list gets 26 px rows, a drawn header and dark scrollbars

Row height comes from the image list, so the state list the ticks already
ride in is the lever; the tick images stay 15 px and centre in the taller
cell.

A selected row takes accent_soft plus a 2 px left bar rather than a full
accent fill, which would fight the keycaps and the status pill for the
same cell.

SetWindowTheme is a public export, not a uxtheme ordinal -- but the class
name is undocumented, so nothing downstream depends on it having worked."
```

---

## Task 11: The Caps Lock toggle

**Files:**
- Modify: `crates/beckon-windows/src/settings_window/paint.rs`
- Modify: `crates/beckon-windows/src/settings_window/mod.rs`

**Interfaces:**
- Consumes: `ThemeCache`.
- Produces: `pub(super) fn toggle(nm: &NMCUSTOMDRAW, on: bool, enabled: bool, focused: bool, cache: &mut ThemeCache, dpi: u32)`.

- [ ] **Step 1: Keep it a checkbox**

`IDC_CAPS` stays `BS_AUTOCHECKBOX` and is drawn through **`NM_CUSTOMDRAW`**.

```rust
// NOT BS_OWNERDRAW. That style cannot be combined with another button type,
// so adopting it would throw away both the checkbox state machine and the
// UIA role a screen reader announces. NM_CUSTOMDRAW repaints the same control
// and keeps both.
//
// This is the only toggle in the window. The per-row list ticks stay
// checkboxes because they are a multi-select gesture, not a setting.
```

- [ ] **Step 2: Draw it**

40 × 20 track at radius 10, 14 px knob inset 2. Off: `field` track,
`field_border` edge, `text_muted` knob. On: `accent_fill` track, white knob at
the right. Disabled: `field` track, `text_faint` knob. Focus: a 2 px `accent`
ring, offset 2.

The caption is drawn to the right of the track at the usual gap, in `text` or
`text_faint`.

**No `0`/`1` digit in the knob.** VKey draws one; the knob's position already
says everything it would.

- [ ] **Step 3: Verify**

```bash
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo check --target aarch64-pc-windows-msvc --all-targets
```

Gate 10 in Task 15 confirms the control still reports as a checkbox to UIA and
still toggles by `Space`.

- [ ] **Step 4: Commit**

```bash
git add crates/beckon-windows/src/settings_window
git commit -m "feat(windows): Use Caps Lock becomes a toggle, still a checkbox underneath

Drawn through NM_CUSTOMDRAW rather than BS_OWNERDRAW: that style cannot be
combined with another button type, and adopting it would throw away the
checkbox state machine and the UIA role a screen reader announces.

The one toggle in the window. Row ticks stay checkboxes -- they are a
multi-select gesture, not a setting."
```

---

## Task 12: Notes with severity dots

**Files:**
- Modify: `crates/beckon-windows/src/settings_window/paint.rs`
- Modify: `crates/beckon-windows/src/settings_window/mod.rs`

**Interfaces:**
- Consumes: `beckon_core::settings::{Note, Mark}`, `ThemeCache`.
- Produces: `pub(super) fn notes(di: &DRAWITEMSTRUCT, notes: &[Note], cache: &mut ThemeCache, dpi: u32)`.

- [ ] **Step 1: Make `IDC_NOTES` owner-drawn**

Add `SS_OWNERDRAW` to the static and handle it in `WM_DRAWITEM`. One line per
note: a 7 px dot at a fixed x, then the text in `Role::Caption` at
`text_muted`.

Dot colours: `Mark::Ok` → `ok`, `Warn` → `warn`, `Bad` → `bad`, `Unknown` →
`text_faint`.

- [ ] **Step 2: Retire the glyph prefix**

Delete `mark_glyph` and its callers.

```rust
// The `!` / `!!` prefixes and the trailing space that kept them aligned are
// gone: alignment is structural now, because the dot is drawn at a fixed x
// rather than composed into the string.
```

Keep `Mark` itself — `row_condition` derives the list flag from it and that is
untouched.

- [ ] **Step 3: Verify**

```bash
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo check --target aarch64-pc-windows-msvc --all-targets
cargo test
```

`cargo test` matters here: `beckon-core`'s tests over `row_condition` and
`control_state` must still pass unchanged. If deleting `mark_glyph` broke one,
the deletion reached into policy and must be narrowed.

- [ ] **Step 4: Commit**

```bash
git add crates/beckon-windows/src/settings_window crates/beckon-core/src
git commit -m "feat(windows): notes get severity dots instead of ! prefixes

The dot is drawn at a fixed x, so alignment is structural rather than
maintained by a trailing space inside the glyph string.

Mark itself stays -- row_condition derives the list flag from it, and this
change is about drawing, not policy. The core tests are the proof."
```

---

## Task 13: The transparency tiers

**Files:**
- Modify: `crates/beckon-windows/src/settings_window/theme.rs`
- Modify: `crates/beckon-windows/src/settings_window/mod.rs`

**Interfaces:**
- Consumes: `beckon_core::theme::{Backdrop, BackdropInputs, backdrop}` from Task 2.
- Produces: `pub(super) fn read_backdrop_inputs(mica_supported: bool) -> BackdropInputs` and `pub(super) fn apply_backdrop(hwnd: HWND, b: Backdrop)`.

- [ ] **Step 1: Gather the inputs**

Build number via `RtlGetVersion` — **not** `GetVersionEx`, which lies without
a manifest. Remote session via `GetSystemMetrics(SM_REMOTESESSION)`.
Transparency via `read_transparency_enabled()` from Task 4. High contrast from
`read_inputs()`.

- [ ] **Step 2: Apply the tier**

```rust
pub(super) fn apply_backdrop(hwnd: HWND, b: Backdrop) {
    const DWMWA_SYSTEMBACKDROP_TYPE: DWMWINDOWATTRIBUTE = DWMWINDOWATTRIBUTE(38);
    const DWMSBT_NONE: u32 = 1;
    const DWMSBT_MAINWINDOW: u32 = 2;
    unsafe {
        match b {
            Backdrop::Mica => {
                let ty = DWMSBT_MAINWINDOW;
                let _ = DwmSetWindowAttribute(
                    hwnd, DWMWA_SYSTEMBACKDROP_TYPE,
                    &ty as *const _ as *const _, 4);
                // Sheet of glass: DWM clears the whole client to the backdrop
                // before we paint. Every string in this window is inside an
                // opaque card, which is what keeps GDI text off the glass --
                // text drawn straight onto it loses its alpha and fringes
                // black.
                let m = MARGINS { cxLeftWidth: -1, cxRightWidth: -1,
                                  cyTopHeight: -1, cyBottomHeight: -1 };
                let _ = DwmExtendFrameIntoClientArea(hwnd, &m);
                set_layered(hwnd, None);
            }
            Backdrop::Alpha(a) => {
                let ty = DWMSBT_NONE;
                let _ = DwmSetWindowAttribute(
                    hwnd, DWMWA_SYSTEMBACKDROP_TYPE,
                    &ty as *const _ as *const _, 4);
                set_layered(hwnd, Some(a));
            }
            Backdrop::Opaque => {
                let ty = DWMSBT_NONE;
                let _ = DwmSetWindowAttribute(
                    hwnd, DWMWA_SYSTEMBACKDROP_TYPE,
                    &ty as *const _ as *const _, 4);
                set_layered(hwnd, None);
            }
        }
    }
}
```

`set_layered` adds or removes `WS_EX_LAYERED` and calls
`SetLayeredWindowAttributes`. **Removing the style matters**: a window left
layered after switching to Opaque keeps compositing for nothing.

- [ ] **Step 3: Under Mica, do not fill the window base**

In `WM_ERASEBKGND`, return 1 without painting when the tier is `Mica`. Under
`Alpha` and `Opaque`, fill with `col(|p| p.bg, COLOR_BTNFACE)` as usual. The
cards paint over it either way.

- [ ] **Step 4: Re-evaluate on theme change**

`on_theme_changed` from Task 4 also calls `apply_backdrop` — high contrast
turning on must force `Opaque` immediately.

- [ ] **Step 5: Verify**

```bash
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo check --target aarch64-pc-windows-msvc --all-targets
cargo test
```

**Gate 01 decides whether tier 1 ships at all.** If Mica does not composite
cleanly under GDI on a14, set `mica_supported: false` at the single call site
in `read_backdrop_inputs`, record the measurement, and ship tier 2. No other
code changes — that is what the flag is for.

- [ ] **Step 6: Commit**

```bash
git add crates/beckon-windows/src/settings_window
git commit -m "feat(windows): three backdrop tiers, chosen by a core function

Mica under a fully GDI-painted client area is NOT asserted. It is tier 1,
and the one flag mica_supported demotes the whole window to tier 2 if the
hardware gate says no -- no other code changes, which is the point of
resolving the tier in beckon-core rather than at the call site.

High contrast, a remote session, or Transparency effects turned off in
Settings all force Opaque, and they are checked before capability so a
capable build cannot argue past them."
```

---

## Task 14: Ship the new icon

**Files:**
- Modify: `crates/beckon-cli/src/beckon.rc`
- Delete: `assets/beckon.ico`
- Rename: `assets/beckon-v2.ico` → `assets/beckon.ico`

- [ ] **Step 1: Confirm the new file has all four sizes**

```bash
python3 -c "
from PIL import IcoImagePlugin
with open('assets/beckon-v2.ico','rb') as f:
    print(sorted(IcoImagePlugin.IcoFile(f).sizes()))
"
```

Expected: `[(16, 16), (32, 32), (48, 48), (256, 256)]`

- [ ] **Step 2: Swap**

```bash
git rm assets/beckon.ico
git mv assets/beckon-v2.ico assets/beckon.ico
```

`beckon.rc` names `assets/beckon.ico` at resource id 1 and needs no edit —
confirm by reading it. If it names `beckon-v2.ico` for any reason, fix it.

- [ ] **Step 3: Verify the resource still compiles in**

```bash
cargo check --target aarch64-pc-windows-msvc --all-targets
```

`build.rs` embeds the `.rc` on MSVC targets only, so this is the check that
exercises it. Expected: clean.

- [ ] **Step 4: Commit**

```bash
git add -A assets crates/beckon-cli
git commit -m "feat: the app icon becomes a rounded tile

Same mark, same blue -- the gradient's lower stop IS the accent_fill token
rather than a colour that resembles it. Stem and bowl are geometry, not a
typeface, so the counter stays open at 16 px and the build needs no font
present. The 16 px tile is hand-tuned: heavier stem, less rounding."
```

---

## Task 15: Update the probe, and run the gates on a14

**Files:**
- Modify: `crates/beckon-windows/examples/settings_probe.rs`
- Create: `docs/superpowers/measurements/2026-08-13-settings-redesign-a14.md`

**This is the only task that can report anything as working.** Every task
before it is gated on a compile.

- [ ] **Step 1: Update the probe's expectations**

`settings_probe.rs` reads style bits and control geometry. Three things moved:

1. `IDC_COMBO` and `IDC_TAP` now carry `CBS_OWNERDRAWFIXED` — update the
   asserted style mask, and keep the existing assertion that **`CBS_SORT` is
   absent**.
2. `IDC_CAPS` is unchanged as `BS_AUTOCHECKBOX` — assert that explicitly, so
   a future edit to `BS_OWNERDRAW` fails here.
3. The window is 900 × 740; update any size expectation.

Add a read of the title-bar region: `WM_NCCALCSIZE` having worked means
`GetClientRect` includes the caption band, so
`client_height == window_height - 2*border` rather than
`window_height - caption - 2*border`.

- [ ] **Step 2: Build for hardware**

```bash
cargo build --release --target aarch64-pc-windows-msvc --all-targets
```

**`--all-targets`, not `--examples`** — the latter does not build `[[bin]]`
targets and you will test a stale `beckon-serve.exe`.

- [ ] **Step 3: Run the gates in session 1**

SSH to a14 lands in session 0, which has no desktop and no keyboard, so every
result there is a confident false negative. Register a scheduled task with
**both** flags:

```powershell
New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries -Priority 4
```

`schtasks`' defaults refuse to start on battery and leave the task `Queued`
forever; separately, `New-ScheduledTask*` defaults to priority 7, and a task
left there on battery produces no diagnostic at all. Use `-EncodedCommand` for
the PowerShell and a `.bat` for anything with a redirect.

| # | Gate | Control that proves the probe can fail |
|---|---|---|
| 01 | Mica composites under GDI | run once with `DWMSBT_NONE` and confirm the region is *not* translucent |
| 02 | No 1 px artefact at the top; resize borders grabbable; DPI change across monitors | drag-resize from each of the four edges and record which succeed |
| 03 | ListView scrollbar goes dark | screenshot in light mode first |
| 04 | `CBS_OWNERDRAWFIXED` keeps index reads and typeahead | type `f1` into the key list and read `CB_GETCURSEL`; compare to the pre-change value |
| 05 | The tick centres in a 26 px row | screenshot at 100 % and 150 % |
| 06 | Chrome glyphs render at 10 px | request a face that does not exist and confirm the fallback is visibly different |
| 07 | Live light/dark flip with no restart | flip the OS setting while the window is open |
| 08 | Every type role resolved | `WM_GETFONT` + `GetObjectW` per control; **fail any role reporting plain `Segoe UI`** |
| 09 | Eight rows, no partial ninth, no horizontal scrollbar | screenshot with a 20-row config |
| 10 | Toggle still reports as a checkbox and responds to `Space` | drive it with `SendInput` and read `BM_GETCHECK` |

- [ ] **Step 4: Write the measurements up**

`docs/superpowers/measurements/2026-08-13-settings-redesign-a14.md`, one
section per gate, each recording the control alongside the result. A clean
result from a blind detector is indistinguishable from a broken one — that is
why every row above has a control column.

- [ ] **Step 5: Demote if gate 01 failed**

If Mica did not composite: set `mica_supported: false` in
`read_backdrop_inputs`, note it in the measurements, and re-run gate 01 to
confirm tier 2 renders.

- [ ] **Step 6: Commit**

```bash
git add crates/beckon-windows/examples/settings_probe.rs docs/superpowers/measurements
git commit -m "test(windows): the settings probe follows the redesign, and ten gates on a14

Every gate carries a control, because a clean result from a blind detector
and a clean result from a working one look identical.

The probe now asserts IDC_CAPS is still BS_AUTOCHECKBOX, so a future edit
to BS_OWNERDRAW -- which would silently drop the UIA checkbox role -- fails
here rather than in a screen reader."
```

---

## Self-Review

**Spec coverage.** Walked §1–§14 of the spec against the tasks:

| Spec section | Task |
|---|---|
| §1 three-branch theme | 2, 4 |
| §2 scope, no new config keys | Global Constraints |
| §3 palette + CI enforcement | 1 |
| §4 theme detection | 4 |
| §5 title bar, maximize removal, rounded corners | 7 |
| §6 type ramp | 6 |
| §7 geometry, cards, row height | 8, 10 |
| §8 transparency tiers | 2, 13 |
| §9 toggle, buttons, fields, list, pills, notes | 9, 10, 11, 12 |
| §10 icon | 14 |
| §11 module split | 3 |
| §12 hardware gates | 15 |
| §13 invariants | Global Constraints, plus greps in 9 and 15 |
| §14 divergences from VKey | 7 (close button), 11 (no digit) |

Two gaps found and closed while reviewing: the status-pill recolouring had no
home and is now explicit in Task 5's mapping table; and gate 10, that the
toggle still reads as a checkbox to UIA, was implied by Task 11's prose but not
actually measured — it is now a numbered gate with a `BM_GETCHECK` control.

**Placeholder scan.** No `TBD`, no "add error handling", no "similar to Task
N". Every code step carries real code. Task 3 is deliberately mechanical and
says so.

**Type consistency.** `Palette`, `Theme`, `ThemeInputs`, `Backdrop`,
`BackdropInputs`, `resolve`, `backdrop`, `contrast`, `ThemeCache::col`,
`ThemeCache::brush`, `colorref`, `card`, `card_rects`, `button`, `BtnTier`,
`field_border`, `toggle`, `notes`, `TITLEBAR_H`, `MICA_MIN_BUILD`,
`TIER2_ALPHA` — each is defined in exactly one task and spelled identically
where later tasks consume it. `ThemeCache::rebuild` returns `bool` in Task 4
and is used as a `bool` in `on_theme_changed`; `mica_supported` is the field
name in both Task 2 and Task 13.

One inconsistency found and fixed: Task 4's interface block originally gave
`rebuild(&mut self, t: Theme)` with no return type while the implementation
returned `bool`. The interface block now matches.

---

## Notes on testability

Tasks 1 and 2 have real unit tests — 11 of them — because they hold every
decision this feature makes. Tasks 3–14 have **no runtime test and say so**;
their gate is `cargo check --target aarch64-pc-windows-msvc --all-targets`
plus the numbered hardware gate in Task 15.

That imbalance is the design working, not a shortfall in the plan. The
alternative — leaving the theme and tier logic inside `settings_window.rs` —
would make all of it untestable rather than a quarter of it, and the palette
would go back to being checked by a script someone ran once.
