# beckon-serve Settings Window + Caps-as-beckon-key Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give `beckon-serve.exe` a settings window that shows and edits the shortcuts file, and an opt-in tick box that puts the beckon chord on Caps Lock — so a non-developer never has to hand-edit TOML or install a second remapper.

**Architecture:** Every decision lives in a pure function in `beckon-core`/`beckon-cli` and is tested on all three CI jobs; `beckon-windows` holds only Win32 plumbing that consumes those decisions. The settings window writes the TOML file and nothing else — `serve`'s existing file watcher performs the reload, so there is no IPC. Caps Lock is implemented as an *alias*: a `WH_KEYBOARD_LL` hook injects the `ctrl+super+alt+<key>` chord that `RegisterHotKey` is already listening for, leaving `Combo`, `parse_shortcuts` and `register_all` untouched.

**Tech Stack:** Rust 2021 (rust-version 1.75), `toml` 0.8 + `toml_edit` 0.22, `windows` 0.61 (Win32), raw Win32 UI — no GUI toolkit.

**Spec:** `docs/superpowers/specs/2026-08-11-windows-settings-window-and-caps-design.md`

## Status — 2026-08-11, branch `settings-window-and-caps`

**All eleven tasks done. Verified on a14 (Windows 11 ARM64, build 26200),
in session 1, through scheduled tasks.**

159 tests on the macOS host, 179 natively on a14 (MSVC), both `WINCHECK`
commands clean.

### Task 6 — the measurements, taken

| # | Question | Result |
|---|---|---|
| **1** | Does an injected `SendInput` chord fire our **own** `RegisterHotKey`? | **PASS** — the gate the whole alias design rested on |
| **2** | Does the one-burst chord open the Start menu? | **PASS** — it does not. Control fired: a bare Win tap *did* open it (`SearchHost.exe`), so the detector was demonstrably not blind |
| **3** | Does swallowing physical Caps stop the Caps Lock toggle? | **PASS** — with the hook live and `caps_tap = "capslock"`, a tap still toggles, so the swallow-and-reinject works |
| **4** | Does an injected `VK_CAPITAL` flip the toggle? | **PASS** |
| **5** | UIPI behaviour with an elevated window focused | **PASS**, by hand — a scheduled task cannot give the UAC consent this needs. With Task Manager elevated and focused: `Caps+N` does nothing, the typed `ctrl+win+alt+N` still focuses Notepad. Run after a normal-window control, so "nothing happened" could not be a broken setup |
| **6** | Injection cost against the 300 ms `LowLevelHooksTimeout` | 13 ms cold, 5.2 ms warm. **The plan said "microseconds"; that was wrong by ~1000x and is corrected here.** Still 2–4 % of budget |

### Live end-to-end results

- **Caps hook**, with a proper before/after: without `beckon-serve`, `Caps+N`
  did nothing; with it, `Notepad.exe` came to the foreground. A single run
  could not have separated "the hook works" from "Windows would have done
  that anyway".
- **Settings window** opens from the tray double-click, builds all 21
  controls, hides the external-change banner until needed, and closes on
  `WM_CLOSE`.
- **Apply** writes the file, the `# comment` survives, `beckon check` accepts
  the result, and `serve` reloads within a second — read out of the log as
  `settings saved` → `reloaded - 3 shortcuts registered`.
- **Hook lifecycle**: `keyboard.caps = false` produced `caps hook removed`.

### Found by a person, after the automated tests were green

A hands-on session on a14 found what neither the unit tests nor the probes
could. Recorded because each is a class of bug, not a one-off.

4. **The keyboard stuck after `Caps+N`.** Every subsequent key behaved as
   `ctrl+win+alt+<key>` — a bare `n` kept beckoning Notepad, everything else
   stopped working, and the only recovery was killing `beckon-serve`. The
   chord's modifier key-ups are not guaranteed to land: `SendInput` can
   insert fewer events than asked for and reports it **only through a return
   value this code was discarding**, and the `n↓` in the middle of the burst
   fires `WM_HOTKEY`, whose handler runs `backend.beckon()` and pumps the
   message queue while it does. Caps-up after a chord now releases all three
   unconditionally. **A synthetic-input probe could not reproduce this** —
   slow and fast timings were both clean — which is why the fix ships with
   `BECKON_CAPS_DEBUG` tracing rather than another guess.
5. **`Caps` + an unbound key toggled Caps Lock on release**, because only
   *bound* keys marked Caps as used. Any key marks it now.
6. **Resting on Caps and letting go emitted the tap action**, however long it
   was held. Compared against the kanata config these users actually run
   (`~/.nix/configs/kanata`, `caps` = `tap-hold 200 200 esc @cap_alias`): a
   hold is decided on a clock. beckon now uses the same 200 ms.

That comparison also validated the design: kanata's own README documents the
*same* Windows-only failure — `(multi lmet lctl lalt)` held across time drops
the modifier from the second key onward — and its fix is a layer emitting
`(multi lmet lctl lalt $k)` per key, which is structurally beckon's burst.

### Three real defects the live tests found

None were visible to 159 green unit tests or to either `WINCHECK` command.

1. **`toml_edit` dropped trailing comments** — `doc[key] = value(..)`
   replaces the whole `Item`, and the decor is where `# comment` lives.
   Caught by a unit test on its first run.
2. **All three settings labels shared control id `-1`**, and `layout`
   positions through `GetDlgItem`, which resolves every `-1` to the same
   first match — so the App label and the Keyboard group box were never
   placed. Found by reading the live control list out of the running window.
3. **The App field showed one thing and recorded another.** Typing
   "Notepad" wrote `"d"` to the config while the screen said "Debuggable
   Package Manager"; `N` left "Narrator" on screen, `o` left "Obsidian".
   Papered over at the time by re-reading both fields at Apply and on
   kill-focus.

   **CORRECTED 2026-08-11 — the cause recorded here was wrong.** This entry
   read *"The combo box rewrote its own text without saying so. With the
   catalog loaded it jumps to the matching entry as you type."* It does not.
   Measured on a14 with `crates/beckon-windows/examples/combo_probe.rs`
   (comctl32 6.16, 121 items, session 1, real `SendInput` keystrokes): the
   field holds exactly what was typed, `CB_GETCURSEL` stays -1, and the child
   EDIT receives nothing but `WM_KEYDOWN`/`WM_CHAR`. The control
   re-synchronises its edit to the nearest catalogue item, and selects all of
   it, when it is **resized** — and `apply_state` ended with an unconditional
   `layout` that `SetWindowPos`es every control on every keystroke, so the
   next character replaced the whole selection. Fixed in landing 2a by
   `Ui::shown_external` + `Ui::shown_empty`. The wrong cause survived here
   long enough to send a later fix down the same path; see spec §7.15.

### Deviations from the plan as written

1. **`caps` and `settings` live in `beckon-core`**, not `beckon-cli`
   (commit `9d28f5a`). Both depend only on `beckon-core`, and
   `beckon-windows` may not depend on `beckon-cli` — leaving them where the
   plan put them would have forced a full mirror of `ControlState` and
   `Action` inside `beckon-windows`. CI coverage is identical.
2. **The probes are kept, not deleted.** `caps_probe`, `caps_live` and
   `settings_probe` under `crates/beckon-windows/examples/` are the only
   layer that can reach this code at all — the same argument `CLAUDE.md`
   already makes for `testing/linux_live_test.py`. They found all three
   defects above. They are examples, so no shipped binary contains them.
3. **A build trap worth remembering**: `cargo build --examples` does **not**
   build `[[bin]]` targets, so a run that looked like it was testing a fix
   was exercising a stale `beckon-serve.exe`. Use `cargo build --all-targets`
   when testing on hardware.

## Global Constraints

- **MSRV is 1.75** (`workspace.package.rust-version`). No API newer than that.
- **Pure logic goes in `beckon-core` / `beckon-cli`; Win32 goes in `beckon-windows`.** CI passes `--exclude beckon-windows` on the Linux and macOS jobs, so code placed in `beckon-windows` is tested by one job out of three.
- **`WINCHECK` is two commands and both are required** after touching any Windows-conditional code:
  ```bash
  cargo check  --target x86_64-pc-windows-gnu -p beckon-windows -p beckon-cli --all-targets
  cargo clippy --target x86_64-pc-windows-gnu -p beckon-windows -p beckon-cli --all-targets -- -D warnings
  ```
  The `clippy` half is not optional: `check` reports `dead_code` as a warning and exits 0, while CI runs clippy with `-D warnings` where the same warning fails the build. Verified working on unmodified `main`, 1.9 s.
- **`MACCHECK` is the host-native gate** for the pure tasks:
  ```bash
  cargo test  --workspace --exclude beckon-linux --exclude beckon-windows
  cargo clippy --workspace --exclude beckon-linux --exclude beckon-windows --all-targets -- -D warnings
  ```
- **`serve` log messages stay ASCII.** Windows PowerShell 5.1's `Get-Content` defaults to ANSI, so a UTF-8 em-dash arrives as `â€"` in the log. Applies to every `eprintln!` reachable from `serve`. Dialog text and window captions are not log output and may use any character.
- **No new top-level CLI verb and no new flag.** The window is reached from the tray menu only.
- **New dependency, exactly one:** `toml_edit = "0.22"` on `beckon-core`. It is already in `Cargo.lock` as a transitive dependency of `toml 0.8`; adding it directly must not change the lockfile's resolved version.
- **Config files stay portable across OSes.** `keyboard.caps = true` must parse successfully on macOS and Linux, where it is simply ignored.

## File Structure

**Create:**

| Path | Responsibility |
|---|---|
| `crates/beckon-core/src/config_write.rs` | Render a shortcuts file through `toml_edit`, preserving comments. Pure, no I/O. |
| `crates/beckon-cli/src/caps.rs` | `caps::decide` — the Caps hook's state machine as a pure function. |
| `crates/beckon-cli/src/settings.rs` | Settings-window model: rows, validation, runtime status, `Model → ControlState` projection. No Win32. |
| `crates/beckon-windows/examples/caps_probe.rs` | Throwaway measurement probe for Task 6. Deleted in Task 11. |
| `crates/beckon-windows/src/caps_hook.rs` | `SetWindowsHookExW` shim: `KBDLLHOOKSTRUCT` → `KeyEvent`, `Action` → `SendInput`. No decisions. |
| `crates/beckon-windows/src/settings_window.rs` | Win32 window: class, ListView, detail panel, controls, catalog worker thread. |

**Modify:**

| Path | Change |
|---|---|
| `crates/beckon-core/src/shortcuts.rs` | Add `CapsTap`, `KeyboardConfig`, `Config`, `parse_config`; `parse_shortcuts` becomes a wrapper. |
| `crates/beckon-core/src/lib.rs` | Declare `pub mod config_write;`. |
| `crates/beckon-core/Cargo.toml` | Add `toml_edit`. |
| `crates/beckon-cli/src/lib.rs` | Declare `mod caps;` and `mod settings;`. |
| `crates/beckon-cli/src/serve.rs` | `RegisterOutcome.results`; `ServeState.registered` + `.keyboard`; Caps hook lifecycle; menu row rename + `MENU_SETTINGS`. |
| `crates/beckon-windows/src/lib.rs` | Declare the two new modules. |
| `crates/beckon-windows/src/hotkey.rs` | `IsDialogMessage` in `run_forever`. |
| `crates/beckon-windows/Cargo.toml` | `Win32_UI_Controls` feature for the ListView. |
| `crates/beckon-cli/src/serve_app.rs` | Starter template gains a commented-out `keyboard.caps` line. |
| `README.md`, `CLAUDE.md`, the 2026-08-10 spec | Task 11. |

## Task ordering and the one hard gate

Tasks 1–5 are pure and fully verifiable on the macOS host. **Task 6 is a measurement on real Windows hardware and it gates Task 7 only.** Tasks 8–10 (the settings window) have no measurement gate and may proceed in parallel with Task 6. Do not write Task 7's `SendInput` path before Task 6 reports, because a failure there changes the design rather than the code.

---

## Task 1: `parse_config` — keyboard settings in the shortcuts file

**Files:**
- Modify: `crates/beckon-core/src/shortcuts.rs`

**Interfaces:**
- Produces: `CapsTap`, `KeyboardConfig`, `Config`, `parse_config(&str) -> Result<Config, String>`. `parse_shortcuts` keeps its exact current signature.

- [ ] **Step 1: Write the failing tests**

Append to the existing `mod tests` in `crates/beckon-core/src/shortcuts.rs`:

```rust
    // ---------- keyboard settings ----------

    #[test]
    fn a_file_without_keyboard_settings_gets_the_defaults() {
        let c = parse_config(r#""ctrl+alt+t" = "Terminal""#).unwrap();
        assert_eq!(c.shortcuts.len(), 1);
        assert!(!c.keyboard.caps, "caps must be off unless asked for");
        assert_eq!(c.keyboard.caps_tap, CapsTap::CapsLock);
    }

    #[test]
    fn dotted_keys_set_the_keyboard_block() {
        let c = parse_config(
            "keyboard.caps = true\nkeyboard.caps_tap = \"escape\"\n\"ctrl+alt+t\" = \"Terminal\"\n",
        )
        .unwrap();
        assert!(c.keyboard.caps);
        assert_eq!(c.keyboard.caps_tap, CapsTap::Escape);
        assert_eq!(c.shortcuts.len(), 1, "the shortcut must survive alongside");
    }

    #[test]
    fn a_hand_written_keyboard_header_works_too() {
        let c = parse_config("\"ctrl+alt+t\" = \"Terminal\"\n\n[keyboard]\ncaps = true\n").unwrap();
        assert!(c.keyboard.caps);
        assert_eq!(c.shortcuts.len(), 1);
    }

    /// The footgun the spec exists to catch: a shortcut appended below a
    /// `[keyboard]` header is silently nested inside it and never registers.
    #[test]
    fn a_shortcut_nested_under_keyboard_is_a_named_error() {
        let err = parse_config("[keyboard]\ncaps = true\n\"ctrl+alt+t\" = \"Terminal\"\n")
            .unwrap_err();
        assert!(err.contains("ctrl+alt+t"), "must name the offending key: {err}");
        assert!(err.contains("keyboard"), "must say where it ended up: {err}");
    }

    #[test]
    fn an_unknown_keyboard_setting_is_rejected_not_ignored() {
        let err = parse_config("keyboard.caps_tab = \"escape\"\n").unwrap_err();
        assert!(err.contains("caps_tab"), "a typo must be named, not ignored: {err}");
    }

    #[test]
    fn caps_tap_takes_exactly_three_values() {
        for v in ["capslock", "escape", "none"] {
            parse_config(&format!("keyboard.caps_tap = \"{v}\"")).unwrap();
        }
        let err = parse_config("keyboard.caps_tap = \"esc\"").unwrap_err();
        assert!(err.contains("esc"), "{err}");
    }

    #[test]
    fn caps_must_be_a_boolean() {
        let err = parse_config("keyboard.caps = \"yes\"").unwrap_err();
        assert!(err.contains("caps"), "{err}");
    }

    #[test]
    fn keyboard_must_be_a_table() {
        let err = parse_config("keyboard = \"on\"").unwrap_err();
        assert!(err.contains("keyboard"), "{err}");
    }

    /// `parse_shortcuts` is the pre-existing API; every current caller and
    /// every current test must keep working through it.
    #[test]
    fn parse_shortcuts_still_ignores_the_keyboard_block() {
        let s = parse_shortcuts("keyboard.caps = true\n\"ctrl+alt+t\" = \"Terminal\"\n").unwrap();
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].app, "Terminal");
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

```bash
cargo test -p beckon-core
```

Expected: FAIL — `cannot find function 'parse_config' in this scope` and `cannot find type 'CapsTap'`.

- [ ] **Step 3: Implement**

In `crates/beckon-core/src/shortcuts.rs`, immediately after the `Shortcut` struct, add:

```rust
/// What a bare Caps Lock tap does when `keyboard.caps` is on. The hook must
/// swallow the physical Caps key to use it as a modifier, so the original
/// behaviour only exists if beckon puts it back.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CapsTap {
    /// Toggle Caps Lock, as if nothing had been remapped. The default,
    /// because a beginner ticking a box should not silently lose a key.
    #[default]
    CapsLock,
    Escape,
    None,
}

impl CapsTap {
    pub fn parse(s: &str) -> Result<CapsTap, String> {
        match s {
            "capslock" => Ok(CapsTap::CapsLock),
            "escape" => Ok(CapsTap::Escape),
            "none" => Ok(CapsTap::None),
            other => Err(format!(
                "unknown `keyboard.caps_tap` value `{other}` \
                 (expected \"capslock\", \"escape\" or \"none\")"
            )),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            CapsTap::CapsLock => "capslock",
            CapsTap::Escape => "escape",
            CapsTap::None => "none",
        }
    }
}

/// The `keyboard` block. Read only by Windows `serve`, parsed everywhere:
/// one config file is meant to travel between machines, so a Windows-only
/// setting must not fail `beckon check` on macOS or Linux.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct KeyboardConfig {
    pub caps: bool,
    pub caps_tap: CapsTap,
}

/// A whole shortcuts file.
#[derive(Debug, Clone, PartialEq)]
pub struct Config {
    pub shortcuts: Vec<Shortcut>,
    pub keyboard: KeyboardConfig,
}

/// The one top-level key that is a settings block rather than a combo.
pub const KEYBOARD_KEY: &str = "keyboard";
```

Replace the body of `parse_shortcuts` and add `parse_config` above it:

```rust
/// Parse a shortcuts file: every top-level key is a combo bound to one app
/// name, except `keyboard`, which is the settings block. First error wins.
/// Iteration order follows `toml::Table` (BTreeMap, sorted by key) —
/// registration order is irrelevant to hotkey behavior.
pub fn parse_config(text: &str) -> Result<Config, String> {
    let table: toml::Table = text.parse().map_err(|e: toml::de::Error| e.to_string())?;
    let mut seen: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    let mut out = Vec::with_capacity(table.len());
    let mut keyboard = KeyboardConfig::default();
    for (raw_key, value) in &table {
        if raw_key == KEYBOARD_KEY {
            keyboard = parse_keyboard(value)?;
            continue;
        }
        let combo = Combo::parse(raw_key)?;
        let canon = combo.canonical();
        if let Some(prev) = seen.get(&canon) {
            return Err(format!(
                "`{raw_key}` duplicates `{prev}` (both normalize to `{canon}`)"
            ));
        }
        seen.insert(canon, raw_key.clone());
        let app = match value {
            toml::Value::String(s) if !s.trim().is_empty() => s.clone(),
            toml::Value::String(_) => return Err(format!("empty app name for `{raw_key}`")),
            toml::Value::Array(_) => {
                return Err(format!(
                    "value for `{raw_key}` is an array — candidate lists are not supported, \
                     write exactly one app name"
                ))
            }
            other => {
                return Err(format!(
                    "value for `{raw_key}` must be a string (one app name), got {}",
                    other.type_str()
                ))
            }
        };
        out.push(Shortcut { combo, app });
    }
    Ok(Config {
        shortcuts: out,
        keyboard,
    })
}

fn parse_keyboard(value: &toml::Value) -> Result<KeyboardConfig, String> {
    let t = value.as_table().ok_or_else(|| {
        format!(
            "`keyboard` must be a table of settings, got {}",
            value.type_str()
        )
    })?;
    let mut kb = KeyboardConfig::default();
    for (k, v) in t {
        match k.as_str() {
            "caps" => {
                kb.caps = v.as_bool().ok_or_else(|| {
                    format!(
                        "`keyboard.caps` must be true or false, got {}",
                        v.type_str()
                    )
                })?
            }
            "caps_tap" => {
                let s = v.as_str().ok_or_else(|| {
                    format!("`keyboard.caps_tap` must be a string, got {}", v.type_str())
                })?;
                kb.caps_tap = CapsTap::parse(s)?;
            }
            other => {
                // TOML puts every bare key-value pair written after a
                // `[keyboard]` header INSIDE that table. A shortcut appended
                // to the bottom of such a file is silently nested here and
                // never registers, with no error anywhere. Say so.
                if Combo::parse(other).is_ok() {
                    return Err(format!(
                        "`{other}` is a shortcut but it is nested under `[keyboard]`. \
                         Move it above the `[keyboard]` header, or write the settings \
                         as `keyboard.caps = ...` instead of a `[keyboard]` section."
                    ));
                }
                return Err(format!(
                    "unknown setting `keyboard.{other}` (expected `caps` or `caps_tap`)"
                ));
            }
        }
    }
    Ok(kb)
}

/// Shortcuts only. Kept because `check` and `serve` want exactly this.
pub fn parse_shortcuts(text: &str) -> Result<Vec<Shortcut>, String> {
    parse_config(text).map(|c| c.shortcuts)
}
```

- [ ] **Step 4: Run the tests**

```bash
cargo test -p beckon-core
```

Expected: PASS, including every pre-existing test in the file.

- [ ] **Step 5: Run the full host gate**

```bash
cargo test  --workspace --exclude beckon-linux --exclude beckon-windows
cargo clippy --workspace --exclude beckon-linux --exclude beckon-windows --all-targets -- -D warnings
```

Expected: PASS. `serve.rs` and `serve_app.rs` call `parse_shortcuts`, whose signature did not change.

- [ ] **Step 6: Commit**

```bash
git add crates/beckon-core/src/shortcuts.rs
git commit -m "feat(core): parse a keyboard settings block in the shortcuts file

parse_config returns shortcuts plus a KeyboardConfig; parse_shortcuts
becomes a wrapper so no caller changes. The `keyboard` key is the single
top-level name that is a settings table rather than a combo.

Dotted keys (keyboard.caps = true) are the documented spelling because a
[keyboard] header swallows every bare pair written after it -- a shortcut
appended to the bottom of such a file silently becomes keyboard.\"ctrl+..\"
and never registers. A hand-written header still parses, and that exact
nesting mistake is now a hard error naming the key."
```

---

## Task 2: `config_write` — write the file without eating comments

**Files:**
- Create: `crates/beckon-core/src/config_write.rs`
- Modify: `crates/beckon-core/src/lib.rs`, `crates/beckon-core/Cargo.toml`

**Interfaces:**
- Consumes: `KeyboardConfig`, `CapsTap`, `KEYBOARD_KEY`, `parse_config` (Task 1).
- Produces: `config_write::RowWrite { orig_key: Option<String>, combo: String, app: String }` and `config_write::render(original: &str, rows: &[RowWrite], keyboard: &KeyboardConfig) -> Result<String, String>`.

`orig_key` is the raw key this row was loaded from (`None` for a row the user added). It exists so an untouched row keeps its original spelling, position and trailing comment: the file may say `alt+ctrl+t` where the canonical form is `ctrl+alt+t`, and rewriting that on an unrelated save would be a gratuitous diff in the user's own file.

- [ ] **Step 1: Add the dependency**

In `crates/beckon-core/Cargo.toml`, under `[dependencies]`:

```toml
toml_edit = "0.22"
```

Then confirm the lockfile does not move:

```bash
cargo tree -p beckon-core -i toml_edit --depth 0
git diff --stat Cargo.lock
```

Expected: `toml_edit v0.22.27`, and `Cargo.lock` shows no version change (it is already present as a transitive dependency of `toml 0.8`).

- [ ] **Step 2: Write the failing tests**

Create `crates/beckon-core/src/config_write.rs` with only the test module and the two public items unimplemented:

```rust
//! Write a shortcuts file back out without destroying what the user wrote.
//!
//! `toml::Table` loses every comment on re-serialization. This file is one
//! beckon invites people to edit by hand, so the settings window edits the
//! document in place through `toml_edit` instead.

use crate::shortcuts::{CapsTap, KeyboardConfig, KEYBOARD_KEY};

/// One row on its way back to disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RowWrite {
    /// The raw key this row was loaded from; `None` for a new row. An
    /// unchanged row keeps its original spelling and position.
    pub orig_key: Option<String>,
    pub combo: String,
    pub app: String,
}

pub fn render(
    _original: &str,
    _rows: &[RowWrite],
    _keyboard: &KeyboardConfig,
) -> Result<String, String> {
    unimplemented!()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shortcuts::parse_config;

    fn row(key: &str, app: &str) -> RowWrite {
        RowWrite {
            orig_key: Some(key.to_string()),
            combo: key.to_string(),
            app: app.to_string(),
        }
    }

    #[test]
    fn comments_and_spelling_survive_an_unrelated_edit() {
        let original = "# my keys\n\n\"alt+ctrl+t\" = \"Terminal\"  # the good one\n\"ctrl+alt+e\" = \"Explorer\"\n";
        let rows = vec![row("alt+ctrl+t", "Terminal"), row("ctrl+alt+e", "Files")];
        let out = render(original, &rows, &KeyboardConfig::default()).unwrap();
        assert!(out.contains("# my keys"), "header comment lost:\n{out}");
        assert!(out.contains("# the good one"), "trailing comment lost:\n{out}");
        assert!(
            out.contains("\"alt+ctrl+t\""),
            "an untouched row was re-spelled:\n{out}"
        );
        assert!(out.contains("Files"), "the edit did not land:\n{out}");
    }

    #[test]
    fn a_removed_row_disappears_and_the_rest_stay() {
        let original = "\"ctrl+alt+t\" = \"Terminal\"\n\"ctrl+alt+e\" = \"Explorer\"\n";
        let rows = vec![row("ctrl+alt+e", "Explorer")];
        let out = render(original, &rows, &KeyboardConfig::default()).unwrap();
        let c = parse_config(&out).unwrap();
        assert_eq!(c.shortcuts.len(), 1);
        assert_eq!(c.shortcuts[0].app, "Explorer");
    }

    #[test]
    fn a_new_row_is_appended() {
        let original = "\"ctrl+alt+t\" = \"Terminal\"\n";
        let mut rows = vec![row("ctrl+alt+t", "Terminal")];
        rows.push(RowWrite {
            orig_key: None,
            combo: "ctrl+alt+c".into(),
            app: "Claude".into(),
        });
        let out = render(original, &rows, &KeyboardConfig::default()).unwrap();
        let c = parse_config(&out).unwrap();
        assert_eq!(c.shortcuts.len(), 2);
        assert!(c.shortcuts.iter().any(|s| s.app == "Claude"));
    }

    #[test]
    fn a_retyped_combo_replaces_the_old_key() {
        let original = "\"ctrl+alt+t\" = \"Terminal\"\n";
        let rows = vec![RowWrite {
            orig_key: Some("ctrl+alt+t".into()),
            combo: "ctrl+alt+y".into(),
            app: "Terminal".into(),
        }];
        let out = render(original, &rows, &KeyboardConfig::default()).unwrap();
        let c = parse_config(&out).unwrap();
        assert_eq!(c.shortcuts.len(), 1);
        assert_eq!(c.shortcuts[0].combo.canonical(), "ctrl+alt+y");
    }

    #[test]
    fn keyboard_settings_are_written_as_dotted_keys_when_created() {
        let original = "\"ctrl+alt+t\" = \"Terminal\"\n";
        let kb = KeyboardConfig {
            caps: true,
            caps_tap: CapsTap::Escape,
        };
        let out = render(original, &[row("ctrl+alt+t", "Terminal")], &kb).unwrap();
        assert!(
            !out.contains("[keyboard]"),
            "a table header would swallow anything appended later:\n{out}"
        );
        assert!(out.contains("keyboard.caps"), "{out}");
        let c = parse_config(&out).unwrap();
        assert!(c.keyboard.caps);
        assert_eq!(c.keyboard.caps_tap, CapsTap::Escape);
    }

    /// The file may already contain a hand-written `[keyboard]` header. We
    /// must edit it in place rather than reformat someone's file, and a
    /// newly added shortcut must NOT end up nested inside it.
    #[test]
    fn an_existing_keyboard_header_is_edited_in_place_and_never_captures_new_rows() {
        let original = "\"ctrl+alt+t\" = \"Terminal\"\n\n[keyboard]\ncaps = false\n";
        let rows = vec![
            row("ctrl+alt+t", "Terminal"),
            RowWrite {
                orig_key: None,
                combo: "ctrl+alt+c".into(),
                app: "Claude".into(),
            },
        ];
        let kb = KeyboardConfig {
            caps: true,
            caps_tap: CapsTap::CapsLock,
        };
        let out = render(original, &rows, &kb).unwrap();
        let c = parse_config(&out).expect("must round-trip");
        assert!(c.keyboard.caps, "the header was not updated:\n{out}");
        assert_eq!(
            c.shortcuts.len(),
            2,
            "a new row was swallowed by [keyboard]:\n{out}"
        );
    }

    #[test]
    fn caps_off_is_still_written_so_unticking_persists() {
        let original = "keyboard.caps = true\n\"ctrl+alt+t\" = \"Terminal\"\n";
        let out = render(
            original,
            &[row("ctrl+alt+t", "Terminal")],
            &KeyboardConfig::default(),
        )
        .unwrap();
        let c = parse_config(&out).unwrap();
        assert!(!c.keyboard.caps, "unticking did not persist:\n{out}");
    }

    #[test]
    fn rendering_an_empty_file_produces_a_parseable_one() {
        let out = render("", &[], &KeyboardConfig::default()).unwrap();
        let c = parse_config(&out).unwrap();
        assert!(c.shortcuts.is_empty());
    }

    /// The load-bearing guarantee: whatever the writer emits, the reader
    /// accepts, and it means the same thing.
    #[test]
    fn round_trip_preserves_meaning() {
        let original = "# keep me\n\"ctrl+alt+t\" = \"Terminal\"\n";
        let rows = vec![
            row("ctrl+alt+t", "Windows Terminal"),
            RowWrite {
                orig_key: None,
                combo: "ctrl+super+alt+c".into(),
                app: "Claude".into(),
            },
        ];
        let kb = KeyboardConfig {
            caps: true,
            caps_tap: CapsTap::None,
        };
        let once = render(original, &rows, &kb).unwrap();
        let parsed = parse_config(&once).unwrap();
        assert_eq!(parsed.keyboard, kb);
        assert_eq!(parsed.shortcuts.len(), 2);

        // Rendering the already-rendered file with the same rows must be a
        // no-op, or every save would churn the file.
        let rows2: Vec<RowWrite> = rows
            .iter()
            .map(|r| RowWrite {
                orig_key: Some(r.combo.clone()),
                combo: r.combo.clone(),
                app: r.app.clone(),
            })
            .collect();
        let twice = render(&once, &rows2, &kb).unwrap();
        assert_eq!(once, twice, "saving twice changed the file");
    }
}
```

- [ ] **Step 3: Run the tests to verify they fail**

```bash
cargo test -p beckon-core config_write
```

Expected: FAIL — every test panics at `unimplemented!()`. (Declare the module first, next step, or the tests will not even be found.)

- [ ] **Step 4: Declare the module**

In `crates/beckon-core/src/lib.rs`, next to `pub mod shortcuts;`:

```rust
pub mod config_write;
```

- [ ] **Step 5: Implement `render`**

Replace the stub in `config_write.rs`:

```rust
pub fn render(
    original: &str,
    rows: &[RowWrite],
    keyboard: &KeyboardConfig,
) -> Result<String, String> {
    use toml_edit::{DocumentMut, Item, Table, Value};

    let mut doc: DocumentMut = if original.trim().is_empty() {
        DocumentMut::new()
    } else {
        original
            .parse()
            .map_err(|e: toml_edit::TomlError| e.to_string())?
    };

    // 1. Drop every shortcut key that no longer has a row. `keyboard` is
    //    never a shortcut and is handled separately below.
    let keep: std::collections::HashSet<&str> = rows
        .iter()
        .filter_map(|r| r.orig_key.as_deref())
        .filter(|k| rows.iter().any(|r| r.orig_key.as_deref() == Some(*k) && r.combo == *k))
        .collect();
    let doomed: Vec<String> = doc
        .as_table()
        .iter()
        .map(|(k, _)| k.to_string())
        .filter(|k| k != KEYBOARD_KEY && !keep.contains(k.as_str()))
        .collect();
    for k in doomed {
        doc.remove(&k);
    }

    // 2. Write every row. A row whose combo still matches the key it was
    //    loaded from is updated in place, keeping its position and decor;
    //    anything else is an insert at the end of the root table.
    for r in rows {
        doc[r.combo.as_str()] = toml_edit::value(r.app.as_str());
    }

    // 3. Keyboard settings. An existing `keyboard` item is edited in place
    //    whatever its shape; a fresh one is created DOTTED, never as a
    //    `[keyboard]` header -- a header captures every bare key-value pair
    //    written after it, which would silently swallow the next shortcut
    //    the user appends by hand.
    if doc.get(KEYBOARD_KEY).is_none() {
        let mut t = Table::new();
        t.set_dotted(true);
        doc.insert(KEYBOARD_KEY, Item::Table(t));
    }
    let kb = doc[KEYBOARD_KEY]
        .as_table_mut()
        .ok_or_else(|| "`keyboard` is not a table".to_string())?;
    kb["caps"] = toml_edit::value(Value::from(keyboard.caps));
    kb["caps_tap"] = toml_edit::value(Value::from(keyboard.caps_tap.as_str()));

    Ok(doc.to_string())
}
```

> **Two things this step is likely to get wrong; let the tests arbitrate.**
> 1. `doc[key] = value(..)` on a key that already exists must preserve that key's decor. If `comments_and_spelling_survive_an_unrelated_edit` fails, switch to fetching the existing `Item` and assigning only its value.
> 2. `an_existing_keyboard_header_is_edited_in_place_and_never_captures_new_rows` is the one that decides whether step 2 needs explicit positioning. `toml_edit` normally renders root key-values before sub-table headers regardless of insertion order; if that test fails, set `Table::set_position()` on the `keyboard` table to push it last.

- [ ] **Step 6: Run the tests**

```bash
cargo test -p beckon-core config_write
```

Expected: PASS, all ten.

- [ ] **Step 7: Run the full host gate**

```bash
cargo test  --workspace --exclude beckon-linux --exclude beckon-windows
cargo clippy --workspace --exclude beckon-linux --exclude beckon-windows --all-targets -- -D warnings
```

- [ ] **Step 8: Commit**

```bash
git add crates/beckon-core/src/config_write.rs crates/beckon-core/src/lib.rs crates/beckon-core/Cargo.toml Cargo.lock
git commit -m "feat(core): write the shortcuts file through toml_edit

toml::Table loses every comment on re-serialization, and this is a file
beckon invites people to edit by hand. render() edits the document in
place: untouched rows keep their original spelling, position and trailing
comments, so an unrelated save is not a gratuitous diff in the user's file.

Keyboard settings are created as dotted keys, never as a [keyboard] header,
because a header captures every bare pair written after it. An existing
hand-written header is edited in place instead of being reformatted, and a
test pins that a newly added shortcut does not get swallowed by it.

toml_edit 0.22 was already in Cargo.lock as a transitive dep of toml 0.8."
```

---

## Task 3: `caps::decide` — the hook's state machine, as a pure function

**Files:**
- Create: `crates/beckon-cli/src/caps.rs`
- Modify: `crates/beckon-cli/src/lib.rs`

**Interfaces:**
- Consumes: `beckon_core::shortcuts::{CapsTap, Shortcut}` (Task 1).
- Produces: `caps::{Edge, KeyEvent, Stroke, Action, CapsState, bound_keys, decide}`. Task 7's Win32 shim calls `decide` and nothing else.

- [ ] **Step 1: Write the failing tests**

Create `crates/beckon-cli/src/caps.rs`:

```rust
//! Caps Lock as the beckon key: the decision half.
//!
//! Caps is an ALIAS for `ctrl+super+alt`, not a fifth modifier. The hook
//! injects the chord `RegisterHotKey` is already listening for, so
//! `Combo`, `parse_shortcuts` and `register_all` are untouched -- and,
//! critically, the hook never calls `backend.beckon()`. A
//! `WH_KEYBOARD_LL` callback that outruns `LowLevelHooksTimeout` (300 ms
//! by default) is silently unhooked by Windows with no error anywhere,
//! and `backend.beckon()` was measured at ~57 ms typical / ~945 ms on the
//! miss path. Here the callback does a hash lookup and one `SendInput`;
//! the real work happens later on the ordinary `WM_HOTKEY` path.
//!
//! Windows-only in effect, but it lives in `beckon-cli` because CI passes
//! `--exclude beckon-windows` on the Linux and macOS jobs. A keyboard
//! state machine is the last thing that should be tested by one job in
//! three.

use beckon_core::shortcuts::{CapsTap, Shortcut};
use std::collections::HashSet;

pub const VK_CAPITAL: u32 = 0x14;
pub const VK_ESCAPE: u32 = 0x1B;
pub const VK_LCONTROL: u32 = 0xA2;
pub const VK_LWIN: u32 = 0x5B;
pub const VK_LMENU: u32 = 0xA4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edge {
    Down,
    Up,
}

/// One key transition as the hook sees it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyEvent {
    pub vk: u32,
    pub edge: Edge,
    /// True when this event carries our own `dwExtraInfo` marker, i.e. we
    /// injected it. Without this the first injected stroke would re-enter
    /// `decide` and the whole thing would spiral.
    pub injected_by_us: bool,
}

/// One key transition we are asking the OS to perform.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stroke {
    pub vk: u32,
    pub edge: Edge,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// Let it reach the application untouched.
    PassThrough,
    /// Eat it; the application never sees it.
    Swallow,
    /// Eat it and send these instead, in order, as one `SendInput` call.
    SwallowAndInject(Vec<Stroke>),
}

#[derive(Debug, Default)]
pub struct CapsState {
    held: bool,
    used: bool,
    consumed: HashSet<u32>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use beckon_core::shortcuts::{parse_shortcuts, Shortcut};

    fn shortcuts(text: &str) -> Vec<Shortcut> {
        parse_shortcuts(text).unwrap()
    }

    fn down(vk: u32) -> KeyEvent {
        KeyEvent { vk, edge: Edge::Down, injected_by_us: false }
    }
    fn up(vk: u32) -> KeyEvent {
        KeyEvent { vk, edge: Edge::Up, injected_by_us: false }
    }

    const VK_T: u32 = 0x54;
    const VK_F5: u32 = 0x74;
    const VK_SHIFT: u32 = 0x10;

    fn bound_t() -> HashSet<u32> {
        bound_keys(&shortcuts(r#""ctrl+super+alt+t" = "Terminal""#))
    }

    // ---------- bound_keys ----------

    #[test]
    fn bound_keys_takes_the_beckon_chord_only() {
        let b = bound_keys(&shortcuts(
            "\"ctrl+super+alt+t\" = \"Terminal\"\n\"ctrl+alt+e\" = \"Explorer\"\n",
        ));
        assert!(b.contains(&VK_T));
        assert_eq!(b.len(), 1, "ctrl+alt+e is not reachable through Caps");
    }

    /// Shift is deliberately ignored when collecting bound keys: the user's
    /// physical Shift is still down while the chord is injected, so
    /// `Caps+Shift+T` naturally lands on a `ctrl+super+alt+shift+t` binding.
    #[test]
    fn bound_keys_ignores_shift() {
        let b = bound_keys(&shortcuts(r#""ctrl+super+alt+shift+t" = "Terminal""#));
        assert!(b.contains(&VK_T));
    }

    // ---------- the chord ----------

    #[test]
    fn caps_then_a_bound_key_injects_the_whole_chord_in_one_burst() {
        let mut st = CapsState::default();
        let b = bound_t();
        assert_eq!(decide(down(VK_CAPITAL), &mut st, &b, CapsTap::CapsLock), Action::Swallow);
        let a = decide(down(VK_T), &mut st, &b, CapsTap::CapsLock);
        let Action::SwallowAndInject(strokes) = a else {
            panic!("expected an injection, got {a:?}");
        };
        let expect = vec![
            Stroke { vk: VK_LCONTROL, edge: Edge::Down },
            Stroke { vk: VK_LWIN, edge: Edge::Down },
            Stroke { vk: VK_LMENU, edge: Edge::Down },
            Stroke { vk: VK_T, edge: Edge::Down },
            Stroke { vk: VK_T, edge: Edge::Up },
            Stroke { vk: VK_LMENU, edge: Edge::Up },
            Stroke { vk: VK_LWIN, edge: Edge::Up },
            Stroke { vk: VK_LCONTROL, edge: Edge::Up },
        ];
        assert_eq!(strokes, expect);
    }

    /// The Start-menu hazard, pinned. Win goes down and up inside one burst
    /// with a real key between them; it is never pressed on its own.
    #[test]
    fn the_windows_key_is_never_pressed_without_a_key_between_down_and_up() {
        let mut st = CapsState::default();
        let b = bound_t();
        decide(down(VK_CAPITAL), &mut st, &b, CapsTap::CapsLock);
        let Action::SwallowAndInject(s) = decide(down(VK_T), &mut st, &b, CapsTap::CapsLock) else {
            panic!("expected an injection");
        };
        let win_down = s.iter().position(|k| k.vk == VK_LWIN && k.edge == Edge::Down).unwrap();
        let win_up = s.iter().position(|k| k.vk == VK_LWIN && k.edge == Edge::Up).unwrap();
        assert!(
            s[win_down + 1..win_up].iter().any(|k| k.vk == VK_T),
            "a bare Win press opens the Start menu"
        );
    }

    #[test]
    fn an_unbound_key_passes_through_untouched_while_caps_is_held() {
        let mut st = CapsState::default();
        let b = bound_t();
        decide(down(VK_CAPITAL), &mut st, &b, CapsTap::CapsLock);
        assert_eq!(
            decide(down(VK_F5), &mut st, &b, CapsTap::CapsLock),
            Action::PassThrough,
            "Caps+F5 must still be F5, not a stray ctrl+win+alt chord"
        );
        assert_eq!(decide(up(VK_F5), &mut st, &b, CapsTap::CapsLock), Action::PassThrough);
    }

    #[test]
    fn auto_repeat_injects_once_not_thirty_times_a_second() {
        let mut st = CapsState::default();
        let b = bound_t();
        decide(down(VK_CAPITAL), &mut st, &b, CapsTap::CapsLock);
        assert!(matches!(
            decide(down(VK_T), &mut st, &b, CapsTap::CapsLock),
            Action::SwallowAndInject(_)
        ));
        for _ in 0..5 {
            assert_eq!(
                decide(down(VK_T), &mut st, &b, CapsTap::CapsLock),
                Action::Swallow,
                "auto-repeat must not re-fire the hotkey"
            );
        }
    }

    #[test]
    fn the_physical_key_up_is_swallowed_too() {
        let mut st = CapsState::default();
        let b = bound_t();
        decide(down(VK_CAPITAL), &mut st, &b, CapsTap::CapsLock);
        decide(down(VK_T), &mut st, &b, CapsTap::CapsLock);
        assert_eq!(
            decide(up(VK_T), &mut st, &b, CapsTap::CapsLock),
            Action::Swallow,
            "we already injected T-up; a second one would reach the app unmatched"
        );
    }

    #[test]
    fn a_key_up_after_caps_was_released_is_still_swallowed() {
        let mut st = CapsState::default();
        let b = bound_t();
        decide(down(VK_CAPITAL), &mut st, &b, CapsTap::CapsLock);
        decide(down(VK_T), &mut st, &b, CapsTap::CapsLock);
        decide(up(VK_CAPITAL), &mut st, &b, CapsTap::CapsLock);
        assert_eq!(
            decide(up(VK_T), &mut st, &b, CapsTap::CapsLock),
            Action::Swallow,
            "releasing Caps first must not leak a stray T-up into the app"
        );
    }

    // ---------- the bare tap ----------

    #[test]
    fn a_bare_tap_restores_caps_lock_by_default() {
        let mut st = CapsState::default();
        let b = bound_t();
        decide(down(VK_CAPITAL), &mut st, &b, CapsTap::CapsLock);
        assert_eq!(
            decide(up(VK_CAPITAL), &mut st, &b, CapsTap::CapsLock),
            Action::SwallowAndInject(vec![
                Stroke { vk: VK_CAPITAL, edge: Edge::Down },
                Stroke { vk: VK_CAPITAL, edge: Edge::Up },
            ])
        );
    }

    #[test]
    fn a_bare_tap_can_be_escape_or_nothing() {
        for (tap, expect) in [
            (CapsTap::Escape, Action::SwallowAndInject(vec![
                Stroke { vk: VK_ESCAPE, edge: Edge::Down },
                Stroke { vk: VK_ESCAPE, edge: Edge::Up },
            ])),
            (CapsTap::None, Action::Swallow),
        ] {
            let mut st = CapsState::default();
            let b = bound_t();
            decide(down(VK_CAPITAL), &mut st, &b, tap);
            assert_eq!(decide(up(VK_CAPITAL), &mut st, &b, tap), expect, "{tap:?}");
        }
    }

    #[test]
    fn caps_used_as_a_modifier_does_not_also_fire_the_tap_action() {
        let mut st = CapsState::default();
        let b = bound_t();
        decide(down(VK_CAPITAL), &mut st, &b, CapsTap::CapsLock);
        decide(down(VK_T), &mut st, &b, CapsTap::CapsLock);
        decide(up(VK_T), &mut st, &b, CapsTap::CapsLock);
        assert_eq!(
            decide(up(VK_CAPITAL), &mut st, &b, CapsTap::CapsLock),
            Action::Swallow,
            "Caps+T must not also toggle Caps Lock"
        );
    }

    #[test]
    fn a_second_tap_after_a_chord_still_taps() {
        let mut st = CapsState::default();
        let b = bound_t();
        decide(down(VK_CAPITAL), &mut st, &b, CapsTap::CapsLock);
        decide(down(VK_T), &mut st, &b, CapsTap::CapsLock);
        decide(up(VK_T), &mut st, &b, CapsTap::CapsLock);
        decide(up(VK_CAPITAL), &mut st, &b, CapsTap::CapsLock);
        decide(down(VK_CAPITAL), &mut st, &b, CapsTap::CapsLock);
        assert!(
            matches!(
                decide(up(VK_CAPITAL), &mut st, &b, CapsTap::CapsLock),
                Action::SwallowAndInject(_)
            ),
            "state leaked from the previous press"
        );
    }

    // ---------- recursion guard ----------

    #[test]
    fn our_own_injected_events_are_never_reprocessed() {
        let mut st = CapsState::default();
        let b = bound_t();
        decide(down(VK_CAPITAL), &mut st, &b, CapsTap::CapsLock);
        let mine = KeyEvent { vk: VK_LWIN, edge: Edge::Down, injected_by_us: true };
        assert_eq!(decide(mine, &mut st, &b, CapsTap::CapsLock), Action::PassThrough);
        let mine_caps = KeyEvent { vk: VK_CAPITAL, edge: Edge::Down, injected_by_us: true };
        assert_eq!(
            decide(mine_caps, &mut st, &b, CapsTap::CapsLock),
            Action::PassThrough,
            "the caps_tap injection must not re-enter the state machine"
        );
    }

    // ---------- inert when off ----------

    #[test]
    fn nothing_is_touched_when_no_key_is_bound_to_the_chord() {
        let mut st = CapsState::default();
        let b: HashSet<u32> = HashSet::new();
        decide(down(VK_CAPITAL), &mut st, &b, CapsTap::CapsLock);
        assert_eq!(decide(down(VK_T), &mut st, &b, CapsTap::CapsLock), Action::PassThrough);
    }

    #[test]
    fn modifiers_the_user_holds_themselves_pass_through() {
        let mut st = CapsState::default();
        let b = bound_t();
        decide(down(VK_CAPITAL), &mut st, &b, CapsTap::CapsLock);
        assert_eq!(
            decide(down(VK_SHIFT), &mut st, &b, CapsTap::CapsLock),
            Action::PassThrough,
            "Shift must stay physically down so Caps+Shift+T reaches a shift binding"
        );
    }
}
```

- [ ] **Step 2: Declare the module and run to verify failure**

In `crates/beckon-cli/src/lib.rs`, alongside the other `mod` lines:

```rust
mod caps;
```

```bash
cargo test -p beckon-cli caps
```

Expected: FAIL — `cannot find function 'bound_keys' in this scope`, `cannot find function 'decide'`.

- [ ] **Step 3: Implement**

Insert into `caps.rs` between `CapsState` and `mod tests`:

```rust
/// Keys reachable through Caps: the main key of every binding whose combo
/// carries ctrl + super + alt.
///
/// Shift is deliberately not part of the filter. The user's physical Shift
/// is still down while the chord is injected, so `Caps+Shift+T` arrives at
/// the system as `ctrl+super+alt+shift+t` and lands on a shift binding by
/// itself. Filtering shift out here would make that binding unreachable.
pub fn bound_keys(shortcuts: &[Shortcut]) -> HashSet<u32> {
    shortcuts
        .iter()
        .filter(|s| s.combo.ctrl && s.combo.super_ && s.combo.alt)
        .map(|s| s.combo.key.win)
        .collect()
}

fn chord(vk: u32) -> Vec<Stroke> {
    // One burst, deliberately: holding the modifiers across real time would
    // (a) make a bare Caps tap press and release Win alone, which is exactly
    // the gesture that opens the Start menu, and (b) turn Caps+<anything>
    // into a genuine ctrl+win+alt chord the shell may act on.
    vec![
        Stroke { vk: VK_LCONTROL, edge: Edge::Down },
        Stroke { vk: VK_LWIN, edge: Edge::Down },
        Stroke { vk: VK_LMENU, edge: Edge::Down },
        Stroke { vk, edge: Edge::Down },
        Stroke { vk, edge: Edge::Up },
        Stroke { vk: VK_LMENU, edge: Edge::Up },
        Stroke { vk: VK_LWIN, edge: Edge::Up },
        Stroke { vk: VK_LCONTROL, edge: Edge::Up },
    ]
}

fn tap(vk: u32) -> Vec<Stroke> {
    vec![
        Stroke { vk, edge: Edge::Down },
        Stroke { vk, edge: Edge::Up },
    ]
}

/// Decide what to do with one key transition. Pure apart from `st`.
pub fn decide(
    ev: KeyEvent,
    st: &mut CapsState,
    bound: &HashSet<u32>,
    caps_tap: CapsTap,
) -> Action {
    if ev.injected_by_us {
        return Action::PassThrough;
    }
    match (ev.vk, ev.edge) {
        (VK_CAPITAL, Edge::Down) => {
            if !st.held {
                st.held = true;
                st.used = false;
                st.consumed.clear();
            }
            Action::Swallow
        }
        (VK_CAPITAL, Edge::Up) => {
            let used = st.used;
            st.held = false;
            st.used = false;
            // `consumed` is deliberately NOT cleared here: a key released
            // after Caps must still have its physical key-up swallowed, or
            // the application receives an up with no matching down.
            if used {
                Action::Swallow
            } else {
                match caps_tap {
                    CapsTap::CapsLock => Action::SwallowAndInject(tap(VK_CAPITAL)),
                    CapsTap::Escape => Action::SwallowAndInject(tap(VK_ESCAPE)),
                    CapsTap::None => Action::Swallow,
                }
            }
        }
        (vk, Edge::Down) if st.held => {
            if !bound.contains(&vk) {
                return Action::PassThrough;
            }
            if st.consumed.contains(&vk) {
                return Action::Swallow; // auto-repeat
            }
            st.consumed.insert(vk);
            st.used = true;
            Action::SwallowAndInject(chord(vk))
        }
        (vk, Edge::Up) if st.consumed.contains(&vk) => {
            st.consumed.remove(&vk);
            Action::Swallow
        }
        _ => Action::PassThrough,
    }
}
```

- [ ] **Step 4: Run the tests**

```bash
cargo test -p beckon-cli caps
```

Expected: PASS, all fifteen.

- [ ] **Step 5: Full host gate + WINCHECK**

```bash
cargo test  --workspace --exclude beckon-linux --exclude beckon-windows
cargo clippy --workspace --exclude beckon-linux --exclude beckon-windows --all-targets -- -D warnings
cargo check  --target x86_64-pc-windows-gnu -p beckon-windows -p beckon-cli --all-targets
cargo clippy --target x86_64-pc-windows-gnu -p beckon-windows -p beckon-cli --all-targets -- -D warnings
```

> `caps.rs` is unused until Task 7, so clippy will report `dead_code` on both targets. Add `#![allow(dead_code)]`… **no** — that hides real rot later. Instead put `#[allow(dead_code)]` on the module declaration in `lib.rs` with a comment naming Task 7, and remove it in Task 7:
> ```rust
> // Wired up by the Caps hook in beckon-windows; unused until then.
> #[allow(dead_code)]
> mod caps;
> ```

- [ ] **Step 6: Commit**

```bash
git add crates/beckon-cli/src/caps.rs crates/beckon-cli/src/lib.rs
git commit -m "feat(cli): Caps-as-beckon-key state machine, as a pure function

Caps is an alias for ctrl+super+alt rather than a fifth modifier, so the
hook injects the chord RegisterHotKey already listens for and Combo,
parse_shortcuts and register_all are untouched.

Two hazards are removed by construction rather than guarded against:
injecting the whole chord as one burst means Win never goes down and up
with nothing between it (the Start-menu gesture), and only injecting for
keys actually bound to the chord means Caps+F5 is still F5.

Lives in beckon-cli, not beckon-windows: CI passes --exclude beckon-windows
on the Linux and macOS jobs, and a keyboard state machine should not be
tested by one job in three."
```

---

## Task 4: `settings::Model` — rows, validation, and the drawing projection

**Files:**
- Create: `crates/beckon-cli/src/settings.rs`
- Modify: `crates/beckon-cli/src/lib.rs`

**Interfaces:**
- Consumes: `beckon_core::shortcuts::{parse_config, CapsTap, Combo, KeyboardConfig}` (Task 1), `beckon_core::config_write::{render, RowWrite}` (Task 2).
- Produces:
  - `settings::Row { orig_key: Option<String>, combo: String, app: String }`
  - `settings::Model` with `from_text`, `set_combo`, `set_app`, `add_row`, `remove_row`, `set_caps`, `set_caps_tap`, `dirty`, `problems`, `render`
  - `settings::RuntimeStatus { registered: HashMap<String, Result<(), String>>, catalog: Option<Vec<String>> }`
  - `settings::{ControlState, ListItem, Mark, Detail, Note, control_state}`

Runtime status is keyed by **canonical combo string**, not by index: `ServeState.shortcuts` is in `toml::Table` order (BTreeMap, sorted) while the window's rows are in file order, and aligning two different orderings by position is a bug waiting to happen.

- [ ] **Step 1: Write the failing tests**

Create `crates/beckon-cli/src/settings.rs` containing the type declarations plus `unimplemented!()` bodies and this test module. Types first:

```rust
//! Settings-window model. Everything the window draws is computed here, so
//! the drawing is a pure function of a snapshot -- the same shape
//! `MenuModel`/`build_entries` already use for the tray menu, and for the
//! same reason: it can be tested without a window, a message loop or a
//! registry.

use beckon_core::config_write::{render, RowWrite};
use beckon_core::shortcuts::{parse_config, CapsTap, Combo, KeyboardConfig};
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    /// The raw key this row was loaded from; `None` for a row the user added.
    pub orig_key: Option<String>,
    pub combo: String,
    pub app: String,
}

#[derive(Debug, Clone)]
pub struct Model {
    pub rows: Vec<Row>,
    pub keyboard: KeyboardConfig,
    pub selected: Option<usize>,
    original: String,
    dirty: bool,
}

/// One reason a row cannot be saved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Problem {
    pub row: usize,
    pub message: String,
}

/// What `serve` knows that the file does not.
#[derive(Debug, Clone, Default)]
pub struct RuntimeStatus {
    /// Canonical combo -> registration outcome, from the last pass.
    pub registered: HashMap<String, Result<(), String>>,
    /// Installed app names. `None` until the catalog scan finishes -- which
    /// is NOT the same as "no apps installed", and the UI must not conflate
    /// them.
    pub catalog: Option<Vec<String>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mark {
    Ok,
    Bad,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListItem {
    pub combo: String,
    pub app: String,
    pub mark: Mark,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Note {
    pub mark: Mark,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Detail {
    pub combo: String,
    pub app: String,
    pub notes: Vec<Note>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ControlState {
    pub items: Vec<ListItem>,
    pub detail: Option<Detail>,
    pub caps_checked: bool,
    pub caps_tap: CapsTap,
    pub apply_enabled: bool,
    pub remove_enabled: bool,
}
```

Now the tests:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    const FILE: &str = "# mine\n\"ctrl+super+alt+t\" = \"Terminal\"\n\"ctrl+super+alt+e\" = \"File Explorer\"\n";

    fn model() -> Model {
        Model::from_text(FILE).unwrap()
    }

    #[test]
    fn loading_keeps_file_order_and_original_spelling() {
        let m = Model::from_text("\"alt+ctrl+t\" = \"Terminal\"\n").unwrap();
        assert_eq!(m.rows[0].orig_key.as_deref(), Some("alt+ctrl+t"));
        assert_eq!(m.rows[0].combo, "alt+ctrl+t");
        assert!(!m.dirty(), "just-loaded is not dirty");
    }

    #[test]
    fn editing_marks_dirty_and_saving_is_a_no_op_edit() {
        let mut m = model();
        assert!(!m.dirty());
        m.set_app(0, "Windows Terminal");
        assert!(m.dirty());
        let out = m.render().unwrap();
        assert!(out.contains("Windows Terminal"));
        assert!(out.contains("# mine"), "comment lost:\n{out}");
    }

    #[test]
    fn add_and_remove_rows() {
        let mut m = model();
        m.add_row();
        assert_eq!(m.rows.len(), 3);
        assert_eq!(m.selected, Some(2), "a new row selects itself");
        assert!(m.rows[2].orig_key.is_none());
        m.remove_row(2);
        assert_eq!(m.rows.len(), 2);
        assert!(m.dirty());
    }

    #[test]
    fn removing_the_last_row_clears_the_selection() {
        let mut m = Model::from_text("\"ctrl+alt+t\" = \"Terminal\"\n").unwrap();
        m.selected = Some(0);
        m.remove_row(0);
        assert_eq!(m.selected, None);
    }

    // ---------- validation ----------

    #[test]
    fn a_bad_combo_is_reported_verbatim_from_the_parser() {
        let mut m = model();
        m.set_combo(0, "ctrl+super+alt+T");
        let p = m.problems();
        assert_eq!(p.len(), 1);
        assert_eq!(p[0].row, 0);
        assert!(p[0].message.contains("uppercase"), "{}", p[0].message);
    }

    #[test]
    fn an_empty_app_is_a_problem() {
        let mut m = model();
        m.set_app(1, "   ");
        let p = m.problems();
        assert_eq!(p.len(), 1);
        assert_eq!(p[0].row, 1);
    }

    #[test]
    fn duplicates_flag_both_rows_and_name_the_canonical_form() {
        let mut m = model();
        m.set_combo(1, "alt+ctrl+super+t");
        let p = m.problems();
        assert_eq!(p.len(), 2, "both rows must be flagged, not just the second");
        assert!(p.iter().any(|x| x.row == 0));
        assert!(p.iter().any(|x| x.row == 1));
        assert!(
            p[0].message.contains("ctrl+super+alt+t"),
            "{}",
            p[0].message
        );
    }

    #[test]
    fn render_refuses_an_invalid_model() {
        let mut m = model();
        m.set_combo(0, "nope+t");
        assert!(m.render().is_err());
    }

    /// The load-bearing guarantee: what the UI writes, the parser accepts.
    #[test]
    fn every_valid_model_round_trips_through_the_real_parser() {
        let mut m = model();
        m.set_app(0, "Windows Terminal");
        m.add_row();
        m.set_combo(2, "ctrl+super+alt+c");
        m.set_app(2, "Claude");
        m.set_caps(true);
        m.set_caps_tap(CapsTap::Escape);
        assert!(m.problems().is_empty());

        let text = m.render().unwrap();
        let parsed = parse_config(&text).expect("the writer must emit what the reader accepts");
        assert_eq!(parsed.shortcuts.len(), 3);
        assert!(parsed.keyboard.caps);
        assert_eq!(parsed.keyboard.caps_tap, CapsTap::Escape);

        let reloaded = Model::from_text(&text).unwrap();
        assert_eq!(reloaded.rows.len(), 3);
        assert_eq!(reloaded.keyboard, m.keyboard);
        assert!(!reloaded.dirty());
    }

    // ---------- the drawing projection ----------

    fn status_all_ok() -> RuntimeStatus {
        let mut r = HashMap::new();
        r.insert("ctrl+super+alt+t".to_string(), Ok(()));
        r.insert("ctrl+super+alt+e".to_string(), Ok(()));
        RuntimeStatus {
            registered: r,
            catalog: Some(vec!["Terminal".into(), "File Explorer".into()]),
        }
    }

    #[test]
    fn a_healthy_row_is_marked_ok() {
        let cs = control_state(&model(), &status_all_ok());
        assert_eq!(cs.items.len(), 2);
        assert_eq!(cs.items[0].mark, Mark::Ok);
        assert!(!cs.apply_enabled, "nothing to apply on a clean model");
    }

    #[test]
    fn a_failed_registration_marks_the_right_row() {
        let mut st = status_all_ok();
        st.registered.insert(
            "ctrl+super+alt+e".into(),
            Err("hotkey already taken".into()),
        );
        let cs = control_state(&model(), &st);
        assert_eq!(cs.items[0].mark, Mark::Ok);
        assert_eq!(cs.items[1].mark, Mark::Bad);
    }

    /// A scan that did not run cannot prove an app is absent.
    #[test]
    fn an_unscanned_catalog_shows_unknown_not_missing() {
        let st = RuntimeStatus {
            registered: status_all_ok().registered,
            catalog: None,
        };
        let mut m = model();
        m.selected = Some(0);
        let cs = control_state(&m, &st);
        let note = cs
            .detail
            .unwrap()
            .notes
            .into_iter()
            .find(|n| n.text.contains("installed"))
            .expect("there must be an app-resolution note");
        assert_eq!(note.mark, Mark::Unknown);
    }

    #[test]
    fn an_app_missing_from_a_scanned_catalog_is_marked_bad() {
        let mut m = model();
        m.set_app(0, "Nonexistent App");
        m.selected = Some(0);
        let cs = control_state(&m, &status_all_ok());
        let note = cs
            .detail
            .unwrap()
            .notes
            .into_iter()
            .find(|n| n.text.contains("installed"))
            .unwrap();
        assert_eq!(note.mark, Mark::Bad);
    }

    #[test]
    fn catalog_matching_is_case_insensitive_like_every_beckon_resolver() {
        let mut m = model();
        m.set_app(0, "terminal");
        m.selected = Some(0);
        let cs = control_state(&m, &status_all_ok());
        let note = cs
            .detail
            .unwrap()
            .notes
            .into_iter()
            .find(|n| n.text.contains("installed"))
            .unwrap();
        assert_eq!(note.mark, Mark::Ok);
    }

    #[test]
    fn apply_needs_both_dirty_and_valid() {
        let mut m = model();
        assert!(!control_state(&m, &status_all_ok()).apply_enabled);
        m.set_app(0, "Windows Terminal");
        assert!(control_state(&m, &status_all_ok()).apply_enabled);
        m.set_combo(0, "bad+++");
        assert!(
            !control_state(&m, &status_all_ok()).apply_enabled,
            "a broken model must not be writable"
        );
    }

    #[test]
    fn remove_is_disabled_with_no_selection() {
        let mut m = model();
        assert!(!control_state(&m, &status_all_ok()).remove_enabled);
        m.selected = Some(1);
        assert!(control_state(&m, &status_all_ok()).remove_enabled);
    }

    #[test]
    fn the_keyboard_group_reflects_the_model() {
        let mut m = model();
        m.set_caps(true);
        m.set_caps_tap(CapsTap::None);
        let cs = control_state(&m, &status_all_ok());
        assert!(cs.caps_checked);
        assert_eq!(cs.caps_tap, CapsTap::None);
    }
}
```

- [ ] **Step 2: Declare and verify failure**

In `crates/beckon-cli/src/lib.rs`:

```rust
// Wired up by the settings window in beckon-windows; unused until then.
#[allow(dead_code)]
mod settings;
```

```bash
cargo test -p beckon-cli settings
```

Expected: FAIL at `unimplemented!()`.

- [ ] **Step 3: Implement**

```rust
impl Model {
    pub fn from_text(text: &str) -> Result<Model, String> {
        let cfg = parse_config(text)?;
        // parse_config returns BTreeMap order; the window shows file order,
        // which is what the user sees in their editor. Recover it by
        // matching each parsed shortcut back to where its key appears.
        let mut rows: Vec<Row> = Vec::with_capacity(cfg.shortcuts.len());
        for s in &cfg.shortcuts {
            rows.push(Row {
                orig_key: Some(raw_key_for(text, s.combo).unwrap_or_else(|| s.combo.canonical())),
                combo: raw_key_for(text, s.combo).unwrap_or_else(|| s.combo.canonical()),
                app: s.app.clone(),
            });
        }
        rows.sort_by_key(|r| {
            r.orig_key
                .as_deref()
                .and_then(|k| find_key_offset(text, k))
                .unwrap_or(usize::MAX)
        });
        Ok(Model {
            rows,
            keyboard: cfg.keyboard,
            selected: None,
            original: text.to_string(),
            dirty: false,
        })
    }

    pub fn dirty(&self) -> bool {
        self.dirty
    }

    pub fn set_combo(&mut self, i: usize, v: &str) {
        if self.rows[i].combo != v {
            self.rows[i].combo = v.to_string();
            self.dirty = true;
        }
    }

    pub fn set_app(&mut self, i: usize, v: &str) {
        if self.rows[i].app != v {
            self.rows[i].app = v.to_string();
            self.dirty = true;
        }
    }

    pub fn add_row(&mut self) {
        self.rows.push(Row {
            orig_key: None,
            combo: String::new(),
            app: String::new(),
        });
        self.selected = Some(self.rows.len() - 1);
        self.dirty = true;
    }

    pub fn remove_row(&mut self, i: usize) {
        self.rows.remove(i);
        self.selected = if self.rows.is_empty() {
            None
        } else {
            Some(i.min(self.rows.len() - 1))
        };
        self.dirty = true;
    }

    pub fn set_caps(&mut self, on: bool) {
        if self.keyboard.caps != on {
            self.keyboard.caps = on;
            self.dirty = true;
        }
    }

    pub fn set_caps_tap(&mut self, t: CapsTap) {
        if self.keyboard.caps_tap != t {
            self.keyboard.caps_tap = t;
            self.dirty = true;
        }
    }

    /// Every reason this model cannot be written, one per offending row.
    pub fn problems(&self) -> Vec<Problem> {
        let mut out = Vec::new();
        // Combo syntax and empty app names.
        let mut canon: Vec<Option<String>> = Vec::with_capacity(self.rows.len());
        for (i, r) in self.rows.iter().enumerate() {
            match Combo::parse(&r.combo) {
                Ok(c) => canon.push(Some(c.canonical())),
                Err(e) => {
                    canon.push(None);
                    out.push(Problem { row: i, message: e });
                }
            }
            if r.app.trim().is_empty() {
                out.push(Problem {
                    row: i,
                    message: "app name is empty".to_string(),
                });
            }
        }
        // Duplicates: flag EVERY row in a colliding group, not just the
        // later ones -- the user needs to see both ends of the collision.
        let mut groups: HashMap<&str, Vec<usize>> = HashMap::new();
        for (i, c) in canon.iter().enumerate() {
            if let Some(c) = c {
                groups.entry(c.as_str()).or_default().push(i);
            }
        }
        let mut dups: Vec<(&str, Vec<usize>)> = groups
            .into_iter()
            .filter(|(_, v)| v.len() > 1)
            .collect();
        dups.sort_by_key(|(c, _)| *c);
        for (c, rows) in dups {
            for i in rows {
                out.push(Problem {
                    row: i,
                    message: format!("duplicate shortcut: another row also means `{c}`"),
                });
            }
        }
        out.sort_by_key(|p| p.row);
        out
    }

    /// The file text this model would write. `Err` if the model is invalid
    /// or the writer refuses; never writes a file itself.
    pub fn render(&self) -> Result<String, String> {
        let problems = self.problems();
        if let Some(p) = problems.first() {
            return Err(format!("row {}: {}", p.row + 1, p.message));
        }
        let writes: Vec<RowWrite> = self
            .rows
            .iter()
            .map(|r| RowWrite {
                orig_key: r.orig_key.clone(),
                combo: r.combo.clone(),
                app: r.app.trim().to_string(),
            })
            .collect();
        let text = render(&self.original, &writes, &self.keyboard)?;
        // Validate through the real parser rather than a second rule set:
        // this is what makes "what the UI writes is what beckon reads" true
        // by construction.
        parse_config(&text)?;
        Ok(text)
    }
}

/// Where in `text` the raw key spelling for `combo` appears, if it does.
/// Used only to recover file order and original spelling for display.
fn raw_key_for(text: &str, combo: Combo) -> Option<String> {
    let want = combo.canonical();
    top_level_keys(text)
        .into_iter()
        .find(|k| Combo::parse(k).map(|c| c.canonical() == want).unwrap_or(false))
}

fn find_key_offset(text: &str, key: &str) -> Option<usize> {
    top_level_keys_with_offset(text)
        .into_iter()
        .find(|(k, _)| k == key)
        .map(|(_, o)| o)
}

fn top_level_keys(text: &str) -> Vec<String> {
    top_level_keys_with_offset(text)
        .into_iter()
        .map(|(k, _)| k)
        .collect()
}

/// Bare `"key" = value` lines at the root, in file order, with their byte
/// offsets. Deliberately a scanner and not a parser: it exists only to make
/// the window show rows in the order the user wrote them, and anything it
/// misses simply falls back to canonical order.
fn top_level_keys_with_offset(text: &str) -> Vec<(String, usize)> {
    let mut out = Vec::new();
    let mut offset = 0usize;
    for line in text.lines() {
        let t = line.trim_start();
        if t.starts_with('[') {
            break; // a table header: everything after belongs to it
        }
        if let Some(eq) = t.find('=') {
            let raw = t[..eq].trim();
            if !raw.starts_with('#') && !raw.is_empty() {
                let key = raw.trim_matches(|c| c == '"' || c == '\'');
                if !key.contains('.') {
                    out.push((key.to_string(), offset));
                }
            }
        }
        offset += line.len() + 1;
    }
    out
}

pub fn control_state(m: &Model, rt: &RuntimeStatus) -> ControlState {
    let problems = m.problems();
    let bad_rows: std::collections::HashSet<usize> = problems.iter().map(|p| p.row).collect();

    let items = m
        .rows
        .iter()
        .enumerate()
        .map(|(i, r)| ListItem {
            combo: r.combo.clone(),
            app: r.app.clone(),
            mark: if bad_rows.contains(&i) {
                Mark::Bad
            } else {
                match Combo::parse(&r.combo)
                    .ok()
                    .and_then(|c| rt.registered.get(&c.canonical()))
                {
                    Some(Ok(())) => Mark::Ok,
                    Some(Err(_)) => Mark::Bad,
                    None => Mark::Unknown,
                }
            },
        })
        .collect();

    let detail = m.selected.and_then(|i| m.rows.get(i).map(|r| {
        let mut notes = Vec::new();
        match Combo::parse(&r.combo) {
            Ok(c) => match rt.registered.get(&c.canonical()) {
                Some(Ok(())) => notes.push(Note { mark: Mark::Ok, text: "registered".into() }),
                Some(Err(e)) => notes.push(Note { mark: Mark::Bad, text: format!("not registered: {e}") }),
                None => notes.push(Note { mark: Mark::Unknown, text: "not registered yet".into() }),
            },
            Err(e) => notes.push(Note { mark: Mark::Bad, text: e }),
        }
        notes.push(match &rt.catalog {
            // A scan that has not run cannot prove absence.
            None => Note { mark: Mark::Unknown, text: "checking installed apps...".into() },
            Some(names) => {
                let want = r.app.trim().to_lowercase();
                if !want.is_empty() && names.iter().any(|n| n.to_lowercase() == want) {
                    Note { mark: Mark::Ok, text: "found in installed apps".into() }
                } else {
                    Note { mark: Mark::Bad, text: "no installed app has this name".into() }
                }
            }
        });
        for p in problems.iter().filter(|p| p.row == i) {
            notes.push(Note { mark: Mark::Bad, text: p.message.clone() });
        }
        Detail { combo: r.combo.clone(), app: r.app.clone(), notes }
    }));

    ControlState {
        items,
        detail,
        caps_checked: m.keyboard.caps,
        caps_tap: m.keyboard.caps_tap,
        apply_enabled: m.dirty() && problems.is_empty(),
        remove_enabled: m.selected.is_some(),
    }
}
```

- [ ] **Step 4: Run the tests**

```bash
cargo test -p beckon-cli settings
```

Expected: PASS, all seventeen.

> If `an_unscanned_catalog_shows_unknown_not_missing` or its siblings fail on the substring `"installed"`, the note wording drifted from the test. Change the **test's** substring only if the new wording is genuinely better for a user; otherwise fix the wording. Do not weaken the assertion to `notes.len() >= 2`.

- [ ] **Step 5: Full host gate + WINCHECK** (all four commands from Global Constraints)

- [ ] **Step 6: Commit**

```bash
git add crates/beckon-cli/src/settings.rs crates/beckon-cli/src/lib.rs
git commit -m "feat(cli): settings-window model and its drawing projection

Model owns rows, keyboard settings and the dirty flag; control_state
projects it plus runtime status into exactly what the window draws. Same
shape as MenuModel/build_entries, and for the same reason -- the drawing
becomes a pure function testable without a window or a message loop.

Runtime status is keyed by canonical combo string, not by index: ServeState
holds shortcuts in toml::Table order while the window shows file order, and
aligning two different orderings by position is a bug waiting to happen.

render() validates through parse_config rather than a second rule set, so
'what the UI writes is what beckon reads' holds by construction. A catalog
that has not been scanned yet reports Unknown, never Bad -- a scan that did
not run cannot prove an app is absent."
```

---

## Task 5: per-shortcut registration results reach `ServeState`

**Files:**
- Modify: `crates/beckon-cli/src/serve.rs`

**Interfaces:**
- Consumes: `settings::RuntimeStatus` (Task 4).
- Produces: `RegisterOutcome.results: Vec<(String, Result<(), String>)>` (canonical combo → outcome) and `ServeState.registered: HashMap<String, Result<(), String>>`, read by the window in Task 9.

- [ ] **Step 1: Write the failing test**

Add to `mod tests` in `crates/beckon-cli/src/serve.rs`:

```rust
    #[test]
    fn a_register_outcome_reports_every_combo_by_its_canonical_name() {
        let o = RegisterOutcome {
            ok: 1,
            failed: vec!["ctrl+alt+e".to_string()],
            results: vec![
                ("ctrl+alt+t".to_string(), Ok(())),
                ("ctrl+alt+e".to_string(), Err("taken".to_string())),
            ],
        };
        let map = o.by_canonical();
        assert!(map.get("ctrl+alt+t").unwrap().is_ok());
        assert!(map.get("ctrl+alt+e").unwrap().is_err());
        assert_eq!(map.len(), 2);
    }

    /// The window keys status by canonical string precisely so it does not
    /// depend on ordering; pin that the map survives a re-spelled key.
    #[test]
    fn canonical_keys_are_independent_of_how_the_user_spelled_them() {
        let o = RegisterOutcome {
            ok: 1,
            failed: vec![],
            results: vec![("ctrl+alt+t".to_string(), Ok(()))],
        };
        assert!(o.by_canonical().contains_key("ctrl+alt+t"));
    }
```

- [ ] **Step 2: Run to verify failure**

```bash
cargo test -p beckon-cli serve
```

Expected: FAIL — `struct 'RegisterOutcome' has no field named 'results'`.

- [ ] **Step 3: Implement**

Extend the struct (find it near `registration_phrase`) and add the accessor:

```rust
struct RegisterOutcome {
    ok: usize,
    failed: Vec<String>,
    /// Every attempted combo by its canonical spelling, with what happened.
    /// Canonical rather than as-written because the settings window joins
    /// on this key and the file may spell the same combo differently.
    results: Vec<(String, Result<(), String>)>,
}

impl RegisterOutcome {
    fn by_canonical(&self) -> std::collections::HashMap<String, Result<(), String>> {
        self.results.iter().cloned().collect()
    }
}
```

In `register_all`, record each result:

```rust
fn register_all(mgr: &mut HotkeyManager, shortcuts: &[Shortcut]) -> RegisterOutcome {
    let mut ok = 0;
    let mut failed = Vec::new();
    let mut results = Vec::with_capacity(shortcuts.len());
    for (i, sc) in shortcuts.iter().enumerate() {
        let c = &sc.combo;
        let canon = c.canonical();
        match mgr.register(i as u32, c.ctrl, c.super_, c.alt, c.shift, c.key) {
            Ok(()) => {
                ok += 1;
                results.push((canon, Ok(())));
            }
            Err(e) => {
                // One broken key loses one key, never the whole table.
                eprintln!("beckon serve: cannot register `{}`: {e}", canon);
                failed.push(canon.clone());
                results.push((canon, Err(e)));
            }
        }
    }
    RegisterOutcome { ok, failed, results }
}
```

Add the field to `ServeState`, after `last_phrase`:

```rust
    /// Canonical combo -> last registration outcome. Read by the settings
    /// window so each row can show whether its key actually took. Written
    /// on every registration pass, including the paused one (which clears
    /// it, because nothing is registered while paused).
    #[cfg_attr(not(target_os = "windows"), allow(dead_code))]
    registered: std::collections::HashMap<String, Result<(), String>>,
```

Initialise it in `cmd_serve_app` (`registered: Default::default()` in the struct literal), then set it wherever `last_phrase` is set:

- In `cmd_serve_app`, after `let outcome = register_all(...)`:
  ```rust
  state.borrow_mut().registered = outcome.by_canonical();
  ```
- In `reload`, in the paused branch, before the early `return`:
  ```rust
  state.borrow_mut().registered.clear();
  ```
  and in the success branch after `register_all`:
  ```rust
  state.borrow_mut().registered = outcome.by_canonical();
  ```
- In `set_paused`, clear it when pausing and set it from the new outcome when resuming.

> **Borrow discipline.** Each of those is a fresh `borrow_mut()` on its own line and is dropped immediately. Do not fold them into an existing `borrow_mut()` that is live across `register_all` — the module doc at the top of this file explains why holding a borrow across anything that can pump the Windows message queue aborts the process rather than panicking.

- [ ] **Step 4: Run the tests**

```bash
cargo test -p beckon-cli
```

Expected: PASS, including every pre-existing `serve` test.

- [ ] **Step 5: Full host gate + WINCHECK**

- [ ] **Step 6: Commit**

```bash
git add crates/beckon-cli/src/serve.rs
git commit -m "feat(cli): record per-shortcut registration results in ServeState

RegisterOutcome kept only a count and a list of failed combo strings, which
cannot tell the settings window which ROW failed. It now carries every
attempted combo with its outcome, keyed by canonical spelling so the window
can join on it without depending on ordering -- ServeState holds shortcuts
in toml::Table order while the window shows file order.

Pausing clears the map rather than leaving stale ticks behind: nothing is
registered while paused, and the window must not claim otherwise."
```

---

## Task 6: **GATE** — measure the Caps design on a14 before building it

**Files:**
- Create: `crates/beckon-windows/examples/caps_probe.rs` (deleted in Task 11)

**Interfaces:**
- Consumes: nothing.
- Produces: a yes/no answer to measurement #1. **Task 7 must not start until this reports.**

This task exists because the whole alias design rests on one unmeasured assumption: that a chord delivered by `SendInput` triggers our *own* `RegisterHotKey`. It is very likely true — AutoHotkey's `Send` triggers other applications' hotkeys by exactly this mechanism — but "very likely" is not measured, and if it is false the answer is a different design, not a bug fix.

> **Session hazard, recorded in memory as *a14 Windows remote testing*.** SSH into a14 lands in **session 0**, which has no desktop and no keyboard; hotkeys never fire there and this probe will report a confident false negative. Run it through a scheduled task in the interactive session, using `-EncodedCommand` to avoid quoting damage.

- [ ] **Step 1: Write the probe**

Create `crates/beckon-windows/examples/caps_probe.rs`:

```rust
//! Throwaway probe for the Caps-as-beckon-key design. Delete after use.
//!
//! Run in an INTERACTIVE session (session 0 has no desktop; hotkeys never
//! fire there and every answer below would be a false negative):
//!
//!     cargo run --example caps_probe
//!
//! Answers measurements 1, 3, 4 and 6 from the spec. Measurements 2 (Start
//! menu) and 5 (elevated window focus) need a person looking at the screen;
//! the probe prints what to look for.

fn main() {
    #[cfg(not(target_os = "windows"))]
    eprintln!("caps_probe only does anything on Windows");
    #[cfg(target_os = "windows")]
    win::run();
}

#[cfg(target_os = "windows")]
mod win {
    use std::time::Instant;
    use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
    use windows::Win32::UI::Input::KeyboardAndMouse::*;
    use windows::Win32::UI::WindowsAndMessaging::*;

    const ID: i32 = 0xBEC0;
    const MARK: usize = 0xBECC0DE;
    // f19 is in beckon's key table and is not a shell hotkey.
    const VK_F19: u16 = 0x82;

    fn stroke(vk: u16, up: bool) -> INPUT {
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VIRTUAL_KEY(vk),
                    wScan: 0,
                    dwFlags: if up { KEYEVENTF_KEYUP } else { KEYBD_EVENT_FLAGS(0) },
                    time: 0,
                    dwExtraInfo: MARK,
                },
            },
        }
    }

    fn send(strokes: &[INPUT]) {
        unsafe {
            SendInput(strokes, std::mem::size_of::<INPUT>() as i32);
        }
    }

    pub fn run() {
        println!("== measurement 1: does a SendInput chord trigger our own RegisterHotKey? ==");
        unsafe {
            RegisterHotKey(
                None,
                ID,
                MOD_CONTROL | MOD_WIN | MOD_ALT,
                VK_F19 as u32,
            )
            .expect("RegisterHotKey(ctrl+win+alt+f19) failed");
        }

        let t0 = Instant::now();
        send(&[
            stroke(VK_LCONTROL.0, false),
            stroke(VK_LWIN.0, false),
            stroke(VK_LMENU.0, false),
            stroke(VK_F19, false),
            stroke(VK_F19, true),
            stroke(VK_LMENU.0, true),
            stroke(VK_LWIN.0, true),
            stroke(VK_LCONTROL.0, true),
        ]);
        let inject_us = t0.elapsed().as_micros();

        let mut fired = false;
        let deadline = Instant::now() + std::time::Duration::from_millis(1000);
        while Instant::now() < deadline {
            let mut msg = MSG::default();
            while unsafe { PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE) }.as_bool() {
                if msg.message == WM_HOTKEY && msg.wParam == WPARAM(ID as usize) {
                    fired = true;
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        unsafe {
            let _ = UnregisterHotKey(None, ID);
        }

        println!(
            "   RESULT 1: {} (SendInput of 8 strokes took {inject_us} us)",
            if fired { "PASS - the chord fired our hotkey" } else { "FAIL - DESIGN CHANGE REQUIRED" }
        );
        println!("   RESULT 6: injection cost {inject_us} us; budget is 300000 us");

        println!();
        println!("== measurements 3 and 4: Caps Lock toggle state ==");
        let before = unsafe { GetKeyState(VK_CAPITAL.0 as i32) } & 1;
        send(&[stroke(VK_CAPITAL.0, false), stroke(VK_CAPITAL.0, true)]);
        std::thread::sleep(std::time::Duration::from_millis(100));
        let after = unsafe { GetKeyState(VK_CAPITAL.0 as i32) } & 1;
        println!(
            "   RESULT 4: injected VK_CAPITAL {} the toggle (before={before}, after={after})",
            if before != after { "PASSES - flipped" } else { "FAILS - did not flip" }
        );
        // Put it back.
        if before != after {
            send(&[stroke(VK_CAPITAL.0, false), stroke(VK_CAPITAL.0, true)]);
        }
        println!("   RESULT 3 needs the hook itself; it is checked in Task 7 on hardware.");

        println!();
        println!("== measurements 2 and 5: watch the screen ==");
        println!("   2: press ctrl+win+alt+f19 by hand. If the Start menu opens, the burst");
        println!("      form is not enough on its own and a filler key is needed.");
        println!("   5: focus an elevated window (Task Manager run as admin), then repeat.");
        println!("      Expect: nothing happens. That confirms the documented UIPI gap.");
    }
}
```

- [ ] **Step 2: Typecheck it from the Mac**

```bash
cargo check --target x86_64-pc-windows-gnu -p beckon-windows --examples
```

Expected: compiles. Fix signature mismatches against `windows` 0.61 as the compiler demands — it is the arbiter, not this listing. `RegisterHotKey` returns `windows::core::Result<()>` in 0.61 and its first parameter is `Option<HWND>`.

- [ ] **Step 3: Run it on a14, in session 1**

Copy the repo (or just build on a14) and run through a scheduled task in the interactive session:

```powershell
$cmd = 'cd C:\path\to\beckon; cargo run --example caps_probe *> C:\Users\<user>\caps_probe.log'
$enc = [Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($cmd))
schtasks /Create /TN capsprobe /SC ONCE /ST 00:00 /TR "powershell -EncodedCommand $enc" /F /RL LIMITED
schtasks /Run /TN capsprobe
# wait, then read C:\Users\<user>\caps_probe.log
schtasks /Delete /TN capsprobe /F
```

- [ ] **Step 4: Record the result in this plan file**

Edit this task to state PASS or FAIL for each of 1, 2, 4, 5, 6, with the measured numbers. Commit:

```bash
git add docs/superpowers/plans/2026-08-11-windows-settings-window-and-caps.md
git commit -m "chore: record the Caps design measurements from a14"
```

- [ ] **Step 5: Branch on the result**

- **Measurement 1 PASS** → proceed to Task 7 as written.
- **Measurement 1 FAIL** → stop. Re-open the spec: the alias design is dead and the fallback is a real fifth `caps+` modifier, which changes `Combo`, `parse_shortcuts`, `canonical()`, `register_all` and makes the hook do its own binding lookup and `PostMessage`. That is a spec revision, not a code change.
- **Measurement 2 FAIL** (Start menu opens) → add a filler stroke to `caps::chord` in Task 3 and re-run its tests.
- **Measurement 4 FAIL** → `CapsTap::CapsLock` is not implementable; make `CapsTap::Escape` the default and say so in the README.

---

## Task 7: the Caps hook

**Blocked by:** Task 6, measurement 1.

**Files:**
- Create: `crates/beckon-windows/src/caps_hook.rs`
- Modify: `crates/beckon-windows/src/lib.rs`, `crates/beckon-cli/src/serve.rs`, `crates/beckon-cli/src/lib.rs`

**Interfaces:**
- Consumes: `caps::{decide, bound_keys, Action, CapsState, Edge, KeyEvent, Stroke}` (Task 3), `ServeState.keyboard` (Task 5 pattern).
- Produces: `beckon_windows::caps_hook::{install, uninstall, set_bindings}`.

The hook lives in `beckon-windows` because it is `SetWindowsHookExW`; it holds no decisions. It calls into a callback supplied by `beckon-cli`, which is where `decide` runs.

- [ ] **Step 1: Write the shim**

```rust
//! `WH_KEYBOARD_LL` shim for Caps-as-beckon-key. Decisions live in
//! `beckon_cli::caps`; this file only translates.
//!
//! The callback must return well inside `LowLevelHooksTimeout` (300 ms by
//! default) or Windows silently unhooks us with no error anywhere. It does
//! one closure call and at most one `SendInput`; nothing here may grow a
//! lock, an allocation loop, or a call into a backend.

use std::cell::RefCell;
use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::WindowsAndMessaging::*;

/// Marks the strokes we inject so the hook can ignore its own output.
pub const MARK: usize = 0xBECC0DE;

/// What the caller wants done with one key transition. Mirrors
/// `beckon_cli::caps::Action` without depending on it -- `beckon-windows`
/// must not depend on `beckon-cli`.
pub enum HookAction {
    PassThrough,
    Swallow,
    SwallowAndInject(Vec<(u32, bool)>), // (vk, is_up)
}

type Decider = Box<dyn FnMut(u32, bool, bool) -> HookAction>; // vk, is_up, injected

thread_local! {
    static HOOK: RefCell<Option<HHOOK>> = const { RefCell::new(None) };
    static DECIDER: RefCell<Option<Decider>> = const { RefCell::new(None) };
}

unsafe extern "system" fn proc_(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code as u32 == HC_ACTION {
        let kb = &*(lparam.0 as *const KBDLLHOOKSTRUCT);
        let is_up = matches!(wparam.0 as u32, WM_KEYUP | WM_SYSKEYUP);
        let injected = kb.dwExtraInfo == MARK;
        let action = DECIDER.with(|d| match d.borrow_mut().as_mut() {
            Some(f) => f(kb.vkCode, is_up, injected),
            None => HookAction::PassThrough,
        });
        match action {
            HookAction::PassThrough => {}
            HookAction::Swallow => return LRESULT(1),
            HookAction::SwallowAndInject(strokes) => {
                inject(&strokes);
                return LRESULT(1);
            }
        }
    }
    CallNextHookEx(None, code, wparam, lparam)
}

fn inject(strokes: &[(u32, bool)]) {
    let inputs: Vec<INPUT> = strokes
        .iter()
        .map(|&(vk, up)| INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VIRTUAL_KEY(vk as u16),
                    wScan: 0,
                    dwFlags: if up { KEYEVENTF_KEYUP } else { KEYBD_EVENT_FLAGS(0) },
                    time: 0,
                    dwExtraInfo: MARK,
                },
            },
        })
        .collect();
    unsafe {
        SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
    }
}

/// Install the hook on the CURRENT thread, which must have a message loop.
/// Idempotent: installing twice replaces the decider and keeps one hook.
pub fn install(decider: Decider) -> Result<(), String> {
    DECIDER.with(|d| *d.borrow_mut() = Some(decider));
    let already = HOOK.with(|h| h.borrow().is_some());
    if already {
        return Ok(());
    }
    let h = unsafe { SetWindowsHookExW(WH_KEYBOARD_LL, Some(proc_), None, 0) }
        .map_err(|e| format!("SetWindowsHookExW(WH_KEYBOARD_LL) failed: {e}"))?;
    HOOK.with(|s| *s.borrow_mut() = Some(h));
    Ok(())
}

/// Remove the hook. Safe to call when it is not installed.
pub fn uninstall() {
    HOOK.with(|s| {
        if let Some(h) = s.borrow_mut().take() {
            unsafe {
                let _ = UnhookWindowsHookEx(h);
            }
        }
    });
    DECIDER.with(|d| *d.borrow_mut() = None);
}

pub fn is_installed() -> bool {
    HOOK.with(|h| h.borrow().is_some())
}
```

- [ ] **Step 2: Declare it and typecheck**

`crates/beckon-windows/src/lib.rs`: `pub mod caps_hook;`

```bash
cargo check  --target x86_64-pc-windows-gnu -p beckon-windows --all-targets
cargo clippy --target x86_64-pc-windows-gnu -p beckon-windows --all-targets -- -D warnings
```

> Let the compiler correct every signature. In `windows` 0.61 `SetWindowsHookExW` takes `Option<HINSTANCE>` and returns `windows::core::Result<HHOOK>`; `UnhookWindowsHookEx` returns `windows::core::Result<()>`; `CallNextHookEx` takes `Option<HHOOK>`. Do not fight it.

- [ ] **Step 3: Wire the lifecycle in `serve.rs`**

Add near the other Windows-only helpers:

```rust
/// Install, refresh or remove the Caps hook to match the current config.
/// Called at startup, after every reload, and on both edges of pause.
#[cfg(target_os = "windows")]
fn sync_caps_hook(state: &Rc<RefCell<ServeState>>) {
    use beckon_windows::caps_hook::{self, HookAction};

    let (want, tap, bound) = {
        let s = state.borrow();
        (
            s.keyboard.caps && !s.paused,
            s.keyboard.caps_tap,
            crate::caps::bound_keys(&s.shortcuts),
        )
    };

    if !want {
        if caps_hook::is_installed() {
            caps_hook::uninstall();
            eprintln!("beckon serve: caps hook removed");
        }
        return;
    }

    // The state machine lives in the closure, not in ServeState: the hook
    // callback must never touch a RefCell that reload() also borrows.
    let mut st = crate::caps::CapsState::default();
    let decider = Box::new(move |vk: u32, is_up: bool, injected: bool| {
        let ev = crate::caps::KeyEvent {
            vk,
            edge: if is_up { crate::caps::Edge::Up } else { crate::caps::Edge::Down },
            injected_by_us: injected,
        };
        match crate::caps::decide(ev, &mut st, &bound, tap) {
            crate::caps::Action::PassThrough => HookAction::PassThrough,
            crate::caps::Action::Swallow => HookAction::Swallow,
            crate::caps::Action::SwallowAndInject(s) => HookAction::SwallowAndInject(
                s.into_iter()
                    .map(|k| (k.vk, matches!(k.edge, crate::caps::Edge::Up)))
                    .collect(),
            ),
        }
    });

    match caps_hook::install(decider) {
        Ok(()) => eprintln!("beckon serve: caps hook active"),
        Err(e) => {
            eprintln!("beckon serve: {e}");
            crate::notify::report(
                &format!("could not enable Caps Lock: {e}"),
                crate::notify::Cause::HumanAction,
            );
            state.borrow_mut().keyboard.caps = false;
        }
    }
}

#[cfg(not(target_os = "windows"))]
fn sync_caps_hook(_state: &Rc<RefCell<ServeState>>) {}
```

`ServeState` gains `keyboard: KeyboardConfig`, set from `parse_config` in `cmd_serve_app` and refreshed in `reload`. Change `cmd_serve_app` to call `parse_config` instead of `parse_shortcuts`, and `reload` likewise. Call `sync_caps_hook(&state)`:

- in `cmd_serve_app`, right after the first `register_all`;
- at the end of both `reload` branches that succeed;
- at the end of `set_paused`, both edges.

> `sync_caps_hook` takes a fresh short borrow and drops it before `install`. `install` does not pump the message queue, but keeping the same discipline as every other call site in this file costs nothing and is what the module doc requires.

Remove the `#[allow(dead_code)]` from `mod caps;` in `lib.rs` — it is used now.

- [ ] **Step 4: Full gate**

All four commands. Then push and read the `windows-latest` CI job.

- [ ] **Step 5: Verify on a14, session 1**

- Tick `keyboard.caps = true` in the config by hand, save, confirm `caps hook active` appears in the log within a second.
- Hold Caps + T → Terminal focuses.
- Tap Caps alone → Caps Lock toggles (measurement 3 confirmed on hardware).
- Caps + F5 → still F5.
- Pause from the tray → Caps does nothing and Caps Lock behaves normally again.
- Set `caps = false`, save → `caps hook removed` in the log.

- [ ] **Step 6: Commit**

```bash
git add crates/beckon-windows/src/caps_hook.rs crates/beckon-windows/src/lib.rs crates/beckon-cli/src/serve.rs crates/beckon-cli/src/lib.rs
git commit -m "feat(windows): Caps Lock as the beckon key, behind keyboard.caps

A WH_KEYBOARD_LL hook translates Caps into the ctrl+super+alt chord that
RegisterHotKey already listens for. This reverses the recorded 'no event
tap, no LLHOOK' decision for one opt-in feature that is off by default.

The hook holds no decisions -- beckon_cli::caps::decide does, and is tested
on all three CI jobs. The callback does one closure call and at most one
SendInput, so it stays orders of magnitude inside LowLevelHooksTimeout;
backend.beckon() is never reached from here.

Pausing unhooks. Leaving the hook installed while paused would swallow Caps
while nothing works, which is the worst available state."
```

---

## Task 8: the settings window — frame, list, detail panel

**Files:**
- Create: `crates/beckon-windows/src/settings_window.rs`
- Modify: `crates/beckon-windows/src/lib.rs`, `crates/beckon-windows/Cargo.toml`, `crates/beckon-windows/src/hotkey.rs`

**Interfaces:**
- Consumes: `settings::{ControlState, ListItem, Mark, Detail, Note}` (Task 4).
- Produces: `settings_window::{open, is_open, apply_state, WindowCallbacks}` where `apply_state(&ControlState)` repaints and `WindowCallbacks` carries the closures the window fires on edit/add/remove/tick/apply/close.

The window is **modeless** and created on the `serve` thread, so `run_forever` dispatches its messages and hotkeys keep firing while it is open.

- [ ] **Step 1: Add the ListView feature**

`crates/beckon-windows/Cargo.toml`, in the `windows` feature list:

```toml
    "Win32_UI_Controls",
```

- [ ] **Step 2: `IsDialogMessage` in `run_forever`**

In `crates/beckon-windows/src/hotkey.rs`, inside the message pump, before `TranslateMessage`:

```rust
        // Give the settings window (modeless) first refusal so Tab, Esc and
        // arrow navigation work inside it. IsDialogMessage returns true when
        // it consumed the message; WM_HOTKEY is not a dialog message and is
        // never consumed here, so hotkeys keep working while it is open.
        let dlg = crate::settings_window::hwnd();
        if let Some(h) = dlg {
            if unsafe { IsDialogMessageW(h, &msg) }.as_bool() {
                continue;
            }
        }
```

- [ ] **Step 3: Write the window**

`settings_window.rs` registers a class, creates the frame, and builds children:

| Control | Class | Notes |
|---|---|---|
| List | `SysListView32`, `LVS_REPORT \| LVS_SINGLESEL \| LVS_SHOWSELALWAYS` | three columns: mark, shortcut, app |
| Shortcut field | `EDIT`, `ES_AUTOHSCROLL` | plain text; validated by `settings::Model` |
| App field | `COMBOBOX`, `CBS_DROPDOWN` | `CBS_DROPDOWN`, **not** `CBS_DROPDOWNLIST` — free typing must stay possible for apps with no catalog entry |
| Notes | `STATIC`, multiline | rendered from `Detail::notes` |
| `[+]` `[−]` `[Apply]` | `BUTTON` | |
| Caps tick | `BUTTON`, `BS_AUTOCHECKBOX` | |
| Tap radios | `BUTTON`, `BS_AUTORADIOBUTTON`, first with `WS_GROUP` | |
| `[Open config file]` `[Close]` | `BUTTON` | |

Rules this file must follow:

- **One window only.** `open()` checks `hwnd()` first and calls `SetForegroundWindow` on the existing one.
- **`apply_state(&ControlState)`** is the only path that changes what is on screen. It never reads the model directly.
- `WM_NOTIFY` / `LVN_ITEMCHANGED` → `on_select(index)`; `EN_CHANGE` / `CBN_EDITCHANGE` → `on_edit`; `BN_CLICKED` → the matching callback.
- **`WM_CLOSE` asks the caller**, via `on_close_request() -> bool`, whether it may close. The caller shows the save prompt; the window does not own that policy.
- Everything measured in dialog units scaled by the window's DPI, so 150 % scaling is not an afterthought.

- [ ] **Step 4: Typecheck**

```bash
cargo check  --target x86_64-pc-windows-gnu -p beckon-windows -p beckon-cli --all-targets
cargo clippy --target x86_64-pc-windows-gnu -p beckon-windows -p beckon-cli --all-targets -- -D warnings
```

- [ ] **Step 5: Commit**

```bash
git add crates/beckon-windows/src/settings_window.rs crates/beckon-windows/src/lib.rs crates/beckon-windows/Cargo.toml crates/beckon-windows/src/hotkey.rs
git commit -m "feat(windows): settings window frame, list and detail panel

Modeless and created on the serve thread, so run_forever dispatches its
messages and hotkeys keep firing while it is open -- which is the whole
reason it is not a dialog box. run_forever gains IsDialogMessage so Tab,
Esc and arrows work inside it; WM_HOTKEY is not a dialog message and is
never consumed there.

The app field is CBS_DROPDOWN rather than CBS_DROPDOWNLIST: beckon
deliberately supports apps with no Start Menu entry, so free typing must
stay possible even once the catalog has loaded.

apply_state(&ControlState) is the only path that changes what is on screen;
the window never reads the model."
```

---

## Task 9: wire the window to `serve` — catalog thread, Apply, external changes

**Files:**
- Modify: `crates/beckon-cli/src/serve.rs`, `crates/beckon-windows/src/settings_window.rs`

**Interfaces:**
- Consumes: everything from Tasks 4, 5, 8.
- Produces: `MENU_SETTINGS` behaviour; a `Model` owned by `ServeState` while the window is open.

- [ ] **Step 1: Menu**

Rename `MENU_EDIT`'s label from `"Edit shortcuts..."` to `"Settings..."`, keep the id, and point both it and `MENU_ID_DOUBLE_CLICK` at `open_settings(&state, &mgr)` instead of `open_path`. Add a test beside the existing `build_entries` tests:

```rust
    #[cfg(target_os = "windows")]
    #[test]
    fn the_first_action_row_opens_settings_not_notepad() {
        let m = MenuModel {
            phrase: "2 shortcuts registered".into(),
            paused: false,
            autostart: Some(false),
            has_log: true,
        };
        let rows = build_entries(&m);
        let edit = rows.iter().find(|r| r.id == MENU_EDIT).unwrap();
        assert_eq!(edit.label, "Settings...");
    }
```

- [ ] **Step 2: Catalog worker thread**

```rust
#[cfg(target_os = "windows")]
fn spawn_catalog_scan() {
    std::thread::spawn(|| {
        // Its own STA: an MTA worker would get a marshalling proxy back to
        // the host apartment and serialise anyway.
        let names: Vec<String> = beckon_windows::apps::scan_installed_apps()
            .into_iter()
            .map(|a| a.name)
            .collect();
        beckon_windows::settings_window::post_catalog(names);
    });
}
```

`scan_installed_apps` was measured at ~370–500 ms and `run_forever`'s message loop is the same thread that dispatches `WM_HOTKEY`; scanning inline would stall every hotkey for half a second each time the window opens. `post_catalog` does a `PostMessage(WM_APP+2)` with a boxed `Vec<String>`; the window's `wndproc` takes ownership, stores it in `RuntimeStatus.catalog` and repaints.

- [ ] **Step 3: Apply**

```rust
#[cfg(target_os = "windows")]
fn apply_settings(state: &Rc<RefCell<ServeState>>) {
    let (text, path) = {
        let s = state.borrow();
        let Some(m) = s.settings.as_ref() else { return };
        match m.render() {
            Ok(t) => (t, s.config.clone()),
            Err(e) => {
                beckon_windows::shell::error_dialog("beckon", &format!("Cannot save: {e}"));
                return;
            }
        }
    };
    // Temp-then-rename: a crash or a full disk must not destroy a working
    // config, and a rename is the write shape watch_config was built for
    // (it watches the parent directory by file name precisely because
    // editors replace files by rename).
    let tmp = path.with_extension("toml.tmp");
    if let Err(e) = std::fs::write(&tmp, &text)
        .and_then(|()| std::fs::rename(&tmp, &path))
    {
        let _ = std::fs::remove_file(&tmp);
        beckon_windows::shell::error_dialog("beckon", &format!("Cannot write {}: {e}", path.display()));
        return;
    }
    // Deliberately no direct reload() call: the watcher fires on the rename
    // and the 1 Hz tick reloads within a second. One code path.
}
```

- [ ] **Step 4: External changes**

`reload()` gains, at the end of its success branch:

```rust
    #[cfg(target_os = "windows")]
    notify_settings_of_external_change(state);
```

which reloads the window's model silently when it is not dirty, and otherwise shows the `File changed on disk [Reload] [Keep mine]` bar. The window never decides this itself.

- [ ] **Step 5: Full gate, then a14**

All four commands, then CI, then on a14: open from the tray and by double-click; hotkeys still fire while open; Apply → reload inside a second → marks update; edit in Notepad while open in both dirty and clean states; 150 % scaling.

- [ ] **Step 6: Commit**

```bash
git add crates/beckon-cli/src/serve.rs crates/beckon-windows/src/settings_window.rs
git commit -m "feat: open the settings window from the tray and write through it

The catalog scan runs on its own STA worker and arrives by PostMessage:
scan_installed_apps was measured at ~370-500 ms and run_forever's message
loop is the same thread that dispatches WM_HOTKEY, so scanning inline would
stall every hotkey for half a second each time the window opened.

Apply writes temp-then-rename. That protects a working config from a crash
mid-write, and a rename is the write shape watch_config was built for. There
is deliberately no direct reload() call -- the watcher fires anyway, so a
shortcut path would only buy under a second at the cost of a second code
path."
```

---

## Task 10: starter template mentions the tick box

**Files:**
- Modify: `crates/beckon-cli/src/serve_app.rs`

- [ ] **Step 1: Extend `starter_template`**

Add above the two bindings:

```
# Prefer Caps Lock to this three-finger chord? Tick "Use Caps Lock as the
# beckon key" in Settings, or set it here:
#
#   keyboard.caps = true
#   keyboard.caps_tap = "capslock"   # or "escape", or "none"
```

- [ ] **Step 2: Verify the existing test still passes**

`the_starter_template_is_a_valid_shortcuts_file` already parses the template. Add:

```rust
    #[test]
    fn the_starter_template_documents_the_caps_option_without_enabling_it() {
        let t = starter_template();
        assert!(t.contains("keyboard.caps"), "the option must be discoverable");
        let parsed = beckon_core::shortcuts::parse_config(t).unwrap();
        assert!(!parsed.keyboard.caps, "it must be commented out, not on");
    }
```

```bash
cargo test -p beckon-cli serve_app
```

- [ ] **Step 3: Commit**

```bash
git add crates/beckon-cli/src/serve_app.rs
git commit -m "feat(cli): the starter template points at the Caps option

Commented out, not enabled: a hook that swallows Caps Lock is not something
a first run should turn on without being asked. A test pins both halves --
the text is present and the setting is off."
```

---

## Task 11: documentation, and delete the probe

**Files:**
- Modify: `README.md`, `CLAUDE.md`, `docs/superpowers/specs/2026-08-10-windows-serve-app-design.md`
- Delete: `crates/beckon-windows/examples/caps_probe.rs`

- [ ] **Step 1: `CLAUDE.md`**

- *Out of scope → GUI / TUI*: rewrite the exception. `beckon-serve.exe` now has a tray menu **and** a settings window; neither is a launcher, and the window lists installed apps only to fill in a name during authoring — the job `beckon search` already has.
- *Known constraints*: new entry for the Caps hook — the UIPI gap (an elevated window has focus → the hook never sees the key, while the typed chord still works because `RegisterHotKey` is not subject to UIPI), conflicts with kanata/PowerToys/AHK, and the EDR keylogger signature. State plainly that "no event tap, no LLHOOK" was reversed deliberately, for one opt-in feature that is off by default.
- *Crate dependencies*: `toml_edit = "0.22"` under the resident-mode block.
- The line "The **only** file beckon reads is the `serve` shortcuts TOML" stays true and gains: it is now also the only file beckon writes.

- [ ] **Step 2: `README.md`**

Windows resident mode gains the settings window and the Caps tick box, **including the UIPI fallback** — a user whose Caps stops working in front of an admin window needs to know the typed chord still works and that this is not a bug.

- [ ] **Step 3: Point the old spec forward**

At the end of *Deferred: the settings window* in `2026-08-10-windows-serve-app-design.md`:

```markdown
**Built 2026-08-11** — see
`docs/superpowers/specs/2026-08-11-windows-settings-window-and-caps-design.md`.
The measurement demanded above was never needed: the window takes combos as
typed text, so chord capture — the feature that required it — was not built.
```

- [ ] **Step 4: Delete the probe**

```bash
git rm crates/beckon-windows/examples/caps_probe.rs
```

- [ ] **Step 5: Full gate**

All four commands, plus `cargo test --workspace --exclude beckon-linux --exclude beckon-windows`.

- [ ] **Step 6: Commit**

```bash
git add README.md CLAUDE.md docs/superpowers/specs/2026-08-10-windows-serve-app-design.md
git commit -m "docs: settings window and Caps Lock, and retire the probe

CLAUDE.md's 'GUI/TUI out of scope' exception is rewritten again, Known
constraints gains the LLHOOK entry with its UIPI gap, and the 2026-08-10
spec's deferred settings window is closed out -- its measurement gate went
unused because chord capture, the feature that needed it, was not built."
```

---

## Self-review

**Spec coverage.** §A.1 schema → Task 1. §A.2 parser → Task 1. §A.3 writing → Task 2. §A.4 model → Task 4 (`RegisterOutcome` per-index → Task 5). §A.5 layout and button semantics → Task 8. §A.6 worker thread, modeless, no IPC → Tasks 8 and 9. §A.7 external edits → Task 9 step 4. §A.8 entry point → Task 9 step 1. §B.1 mechanism → Task 3. §B.2 module split → Tasks 3 and 7. §B.3 lifecycle → Task 7 step 3. §B.4 README gaps → Task 11. §B.5 measurements → Task 6. Error-handling table → distributed across Tasks 1, 4, 7, 9. Testing table → Tasks 1–5 unit, Tasks 7–9 manual. Documentation → Task 11. Starter template is not in the spec but follows from it and is Task 10.

**Type consistency.** `CapsTap`, `KeyboardConfig`, `Config`, `KEYBOARD_KEY` defined in Task 1 and used unchanged in 2, 3, 4, 7, 10. `RowWrite { orig_key, combo, app }` defined in Task 2, constructed in Task 4. `Row` in Task 4 has the same three fields deliberately, so the mapping in `Model::render` is field-for-field. `Action`/`Stroke`/`Edge`/`KeyEvent` defined in Task 3 and consumed in Task 7 via `HookAction`, which is a separate mirror type because `beckon-windows` must not depend on `beckon-cli`. `ControlState`/`Mark`/`Note` defined in Task 4 and consumed in Task 8.

**Known soft spot.** `top_level_keys_with_offset` in Task 4 is a line scanner, not a TOML parser: it will mis-order rows in a file using multi-line strings or inline tables as keys. That is acceptable because its only job is display order, its failure mode is "rows appear in canonical order instead of file order", and a real second parser here would be a second source of truth about the file — exactly what `render`'s validate-through-`parse_config` step exists to avoid.
