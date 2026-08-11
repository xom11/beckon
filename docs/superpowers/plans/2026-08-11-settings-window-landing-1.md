# Settings window redesign — Landing 1: foundation and Caps hook repairs

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give `beckon.exe` and `beckon-serve.exe` an application manifest so Windows stops rendering them with 1999-era controls and starts telling them the real DPI, and fix three Caps-hook defects that are live in shipped code.

**Architecture:** Two independent halves that ship together. The **Caps half** is pure logic in `beckon-core` — a `Chord` type, a filler key in the modifier-release burst, a bound-key set fed from real registration results — and is tested by all three CI jobs. The **manifest half** is a resource file plus four arithmetic corrections in `settings_window.rs`, tested by `WINCHECK` and then measured on real hardware. Nothing in Landing 2 can be tuned until the measurement pass at the end of this plan has run.

**Tech Stack:** Rust (MSRV 1.75), `windows` 0.61, `embed-resource` 2, raw Win32, `toml`/`toml_edit`.

## Global Constraints

- **MSRV is 1.75** (`workspace.package.rust-version`). No API newer than that.
- **Pure logic goes in `beckon-core` / `beckon-cli`; Win32 goes in `beckon-windows`.** CI passes `--exclude beckon-windows` on the Linux and macOS jobs, so code placed in `beckon-windows` is tested by one job out of three.
- **`MACCHECK` is the host-native gate** and must pass at the end of every task:
  ```bash
  cargo test   --workspace --exclude beckon-linux --exclude beckon-windows
  cargo clippy --workspace --exclude beckon-linux --exclude beckon-windows --all-targets -- -D warnings
  ```
- **`WINCHECK` is two commands and both are required** after touching any Windows-conditional code:
  ```bash
  cargo check  --target x86_64-pc-windows-gnu -p beckon-windows -p beckon-cli --all-targets
  cargo clippy --target x86_64-pc-windows-gnu -p beckon-windows -p beckon-cli --all-targets -- -D warnings
  ```
  The `clippy` half is not optional: `check` reports `dead_code` as a warning and exits 0, while CI runs clippy with `-D warnings` where the same warning fails the build.
- **`cargo fmt --all` must be clean.** CI runs `cargo fmt --all -- --check` as its first job.
- **`serve` log messages stay ASCII.** Windows PowerShell 5.1's `Get-Content` defaults to ANSI. Applies to every `eprintln!` reachable from `serve`. Dialog text and window captions are not log output.
- **No new dependency.** Landing 1 adds none.
- **No new top-level CLI verb and no new flag.**
- **Config files stay portable.** `keyboard.caps_hold` must parse successfully on macOS and Linux, where it is ignored.
- **`cargo build --examples` does not build `[[bin]]` targets.** Use `--all-targets` when building anything to test on hardware, or you will test a stale `beckon-serve.exe`.

## File Structure

**Create:**

| Path | Responsibility |
|---|---|
| `crates/beckon-cli/beckon.exe.manifest` | The application manifest: comctl32 v6, PerMonitorV2 DPI, `asInvoker`, supportedOS, UTF-8, long paths. Embedded into every binary in the package. |

**Modify:**

| Path | Change |
|---|---|
| `crates/beckon-core/src/shortcuts.rs` | Add `Chord` (modifiers, no main key) and `KeyboardConfig.caps_hold`; parse and validate `keyboard.caps_hold`. |
| `crates/beckon-core/src/config_write.rs` | Write `caps_hold` only when it differs from the default. |
| `crates/beckon-core/src/caps.rs` | Filler key in `release_modifiers`; `chord`/`release_modifiers`/`decide` take a `Chord`; `CapsState.injected` becomes `Option<Chord>`; `bound_keys` reads registration results; unconditional Caps-down reinit. |
| `crates/beckon-cli/src/serve.rs` | `sync_caps_hook` passes `s.registered` and `caps_hold` to `bound_keys`/`set_bindings`. |
| `crates/beckon-windows/src/caps_hook.rs` | Carry the `Chord` through to `decide`; comment on `set_bindings`. |
| `crates/beckon-cli/beckon.rc` | One line embedding the manifest. |
| `crates/beckon-cli/build.rs` | One `rerun-if-changed` for the manifest. |
| `crates/beckon-windows/src/settings_window.rs` | Real DPI handling, window floor, height clamps, `CB_SETMINVISIBLE`, class icon. |
| `.github/workflows/ci.yml` | A step on the Windows job proving the manifest reached the binary. |

---

### Task 1: The filler key in `release_modifiers()`

`release_modifiers()` emits nothing but bare modifier-ups. In exactly the situation it exists to rescue — a truncated `SendInput` where `Win↓` landed and the key-down did not — a bare `Win↑` is the gesture that opens the Start menu. This is a live defect.

**Files:**
- Modify: `crates/beckon-core/src/caps.rs:179-194` (`release_modifiers`)
- Test: `crates/beckon-core/src/caps.rs` (`mod tests`, in-file)

**Interfaces:**
- Consumes: nothing.
- Produces: `caps::VK_NONAME: u32 = 0xFC`; `release_modifiers()` keeps its current signature `fn release_modifiers() -> Vec<Stroke>` (it gains the `Chord` parameter in Task 3, not here).

- [ ] **Step 1: Write the failing test**

Add to `mod tests` in `crates/beckon-core/src/caps.rs`:

```rust
/// The property that actually matters, stated correctly: every modifier in
/// a burst must have a non-modifier key between its own down and its own
/// up. `chord()` satisfies it by construction. `release_modifiers()` did
/// not -- it emitted only modifier-ups, and a bare Win-up is the gesture
/// that opens the Start menu.
#[test]
fn release_modifiers_never_starts_with_a_bare_modifier_up() {
    let out = release_modifiers();
    let first = out.first().expect("release burst must not be empty");
    assert_eq!(
        (first.vk, first.edge),
        (VK_NONAME, Edge::Down),
        "the burst must open with a filler key-down, or the Win-up that \
         follows is a bare Win tap and opens Start: {out:?}"
    );
    assert_eq!(
        (out[1].vk, out[1].edge),
        (VK_NONAME, Edge::Up),
        "the filler must be released immediately: {out:?}"
    );
    for vk in [VK_LCONTROL, VK_LWIN, VK_LMENU] {
        assert!(
            out.iter().any(|s| s.vk == vk && s.edge == Edge::Up),
            "still must release {vk:#x}: {out:?}"
        );
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p beckon-core release_modifiers_never_starts_with_a_bare_modifier_up`
Expected: FAIL — `cannot find value VK_NONAME in this scope`.

- [ ] **Step 3: Write minimal implementation**

Add the constant beside the other VK constants in `crates/beckon-core/src/caps.rs` (they sit around line 37-41):

```rust
/// `VK_NONAME` (0xFC) is documented as reserved and produces no character,
/// no navigation and no shell action. It exists here only to be a
/// non-modifier key between a modifier's down and its up.
pub const VK_NONAME: u32 = 0xFC;
```

Replace the body of `release_modifiers` and extend its doc comment:

```rust
/// Release the three modifiers the chord presses, unconditionally.
///
/// Emitted when Caps is released after at least one chord. Releasing a key
/// that is already up is a no-op, so the cost is one extra `SendInput`; the
/// cost of NOT doing it is a keyboard where every subsequent key is silently
/// a `ctrl+win+alt` chord, which is unrecoverable without killing beckon.
///
/// This exists because the chord's own key-ups are not guaranteed to land.
/// `SendInput` can insert fewer events than asked for -- UIPI blocks it
/// without setting an error, and another thread holding the input queue
/// makes it return zero -- and the `n` down in the middle of the burst fires
/// `WM_HOTKEY`, whose handler runs `backend.beckon()` (57 ms typical, 945 ms
/// on the miss path) and pumps the message queue while it does. Anything in
/// that window can reorder or drop what follows.
///
/// The burst OPENS WITH A FILLER KEY, and that is load-bearing. The
/// invariant `chord()` satisfies -- every modifier has a non-modifier key
/// between its own down and its own up -- spans both bursts, because the
/// down happened in the chord and the up happens here. In the exact case
/// this function exists for (the chord was truncated after `Win` down), a
/// bare `Win` up is the Start-menu gesture. `VK_NONAME` breaks the pair.
fn release_modifiers() -> Vec<Stroke> {
    vec![
        Stroke {
            vk: VK_NONAME,
            edge: Edge::Down,
        },
        Stroke {
            vk: VK_NONAME,
            edge: Edge::Up,
        },
        Stroke {
            vk: VK_LMENU,
            edge: Edge::Up,
        },
        Stroke {
            vk: VK_LWIN,
            edge: Edge::Up,
        },
        Stroke {
            vk: VK_LCONTROL,
            edge: Edge::Up,
        },
    ]
}
```

- [ ] **Step 4: Run the tests**

Run: `cargo test -p beckon-core caps::`
Expected: PASS. One existing test may assert the old 3-stroke vector — if `releasing_caps_after_a_chord_releases_every_modifier_it_pressed` fails on a length assertion, update it to assert the three modifier-ups are *present* rather than that the vector has length 3. Do not weaken it further; it must still name all three modifiers.

- [ ] **Step 5: Full host gate and commit**

```bash
cargo fmt --all
cargo test   --workspace --exclude beckon-linux --exclude beckon-windows
cargo clippy --workspace --exclude beckon-linux --exclude beckon-windows --all-targets -- -D warnings
git add crates/beckon-core/src/caps.rs
git commit -m "fix(caps): the release burst opened with a bare Win-up

Which is the gesture that opens the Start menu, in exactly the case
release_modifiers() exists to rescue: a truncated SendInput where the
Win-down landed and the key-down did not. The invariant spans two
bursts -- the modifier goes down in chord() and comes up here -- so the
filler has to be here."
```

---

### Task 2: `Chord`, and `keyboard.caps_hold`

**Files:**
- Modify: `crates/beckon-core/src/shortcuts.rs` (add `Chord`; extend `KeyboardConfig` and `parse_keyboard`)
- Modify: `crates/beckon-core/src/config_write.rs:80-85` (write it only when non-default)
- Test: both files, in-file `mod tests`

**Interfaces:**
- Consumes: `Combo`'s existing modifier vocabulary (`ctrl`, `super`, `alt`, `shift`).
- Produces:
  - `pub struct Chord { pub ctrl: bool, pub super_: bool, pub alt: bool }`
  - `impl Default for Chord` → ctrl + super + alt
  - `Chord::parse(&str) -> Result<Chord, String>`
  - `Chord::canonical(&self) -> String`
  - `Chord::is_default(&self) -> bool`
  - `KeyboardConfig { caps: bool, caps_tap: CapsTap, caps_hold: Chord }`

**Note on `shift`:** `Chord` has **no `shift` field at all**, which is stricter than spec §C.3's "refused when `caps = true`" and deliberately so. `caps_hold` is meaningful only when `caps` is true, so a conditional rule leaves a latent trap: a user sets `caps_hold = "ctrl+shift"` while Caps is off, ticks Caps months later, and gets an error about a line they do not remember writing. Making shift unrepresentable in the type removes the trap and matches the Decisions table ("`shift` refused because the hook must press and release it"). Shift remains available on individual bindings.

- [ ] **Step 1: Write the failing tests**

Add to `mod tests` in `crates/beckon-core/src/shortcuts.rs`:

```rust
#[test]
fn a_chord_is_modifiers_with_no_main_key() {
    let c = Chord::parse("ctrl+super+alt").unwrap();
    assert!(c.ctrl && c.super_ && c.alt);
    assert_eq!(c.canonical(), "ctrl+super+alt");
    assert!(c.is_default());
}

#[test]
fn chord_modifier_order_is_free_and_canonical_output_is_not() {
    assert_eq!(Chord::parse("alt+ctrl").unwrap().canonical(), "ctrl+alt");
}

#[test]
fn a_chord_needs_at_least_one_modifier() {
    let e = Chord::parse("").unwrap_err();
    assert!(e.contains("at least one modifier"), "{e}");
}

#[test]
fn shift_is_not_a_chord_modifier() {
    let e = Chord::parse("ctrl+shift").unwrap_err();
    assert!(e.contains("shift"), "{e}");
    assert!(
        e.contains("hold"),
        "the message must say WHERE shift is allowed instead: {e}"
    );
}

#[test]
fn a_main_key_in_a_chord_is_rejected() {
    let e = Chord::parse("ctrl+alt+t").unwrap_err();
    assert!(e.contains('t'), "{e}");
}

#[test]
fn caps_hold_defaults_when_absent_and_parses_when_present() {
    let d = parse_config("\"ctrl+alt+t\" = \"Terminal\"\n").unwrap();
    assert_eq!(d.keyboard.caps_hold, Chord::default());

    let c = parse_config(
        "keyboard.caps = true\nkeyboard.caps_hold = \"ctrl+alt\"\n\"ctrl+alt+t\" = \"Terminal\"\n",
    )
    .unwrap();
    assert_eq!(c.keyboard.caps_hold, Chord::parse("ctrl+alt").unwrap());
}

#[test]
fn an_invalid_caps_hold_names_the_key_in_the_error() {
    let e = parse_config("keyboard.caps_hold = \"ctrl+shift\"\n").unwrap_err();
    assert!(e.contains("caps_hold"), "{e}");
}

/// A Windows-only setting must not fail `beckon check` on another OS: one
/// config file is meant to travel between machines.
#[test]
fn caps_hold_parses_on_every_platform() {
    assert!(parse_config("keyboard.caps_hold = \"ctrl+super+alt\"\n").is_ok());
}
```

Add to `mod tests` in `crates/beckon-core/src/config_write.rs`:

```rust
#[test]
fn a_default_caps_hold_is_not_written_at_all() {
    let out = render("", &[], &KeyboardConfig::default()).unwrap();
    assert!(
        !out.contains("caps_hold"),
        "an untouched default must stay readable by older beckon binaries, \
         which reject unknown keys under `keyboard`:\n{out}"
    );
}

/// The removal branch, which the test above cannot reach: it starts from an
/// empty document, where `kb.remove` is a no-op. Resetting the chord to its
/// default on a file that already carries a non-default line must delete
/// that line -- otherwise an older beckon binary keeps rejecting the file
/// over a setting the user already turned off.
#[test]
fn resetting_caps_hold_to_default_removes_an_existing_line() {
    let original = "keyboard.caps_hold = \"ctrl+alt\"\n\"ctrl+alt+t\" = \"Terminal\"\n";
    let rows = vec![RowWrite {
        orig_key: Some("ctrl+alt+t".into()),
        combo: "ctrl+alt+t".into(),
        app: "Terminal".into(),
    }];
    let out = render(original, &rows, &KeyboardConfig::default()).unwrap();
    assert!(
        !out.contains("caps_hold"),
        "a stale non-default line survived a reset to default:\n{out}"
    );
    parse_config(&out).expect("the writer must emit what the reader accepts");
}

#[test]
fn a_non_default_caps_hold_is_written() {
    let kb = KeyboardConfig {
        caps: true,
        caps_tap: CapsTap::CapsLock,
        caps_hold: crate::shortcuts::Chord::parse("ctrl+alt").unwrap(),
    };
    let out = render("", &[], &kb).unwrap();
    assert!(out.contains("caps_hold"), "{out}");
    assert!(out.contains("ctrl+alt"), "{out}");
    parse_config(&out).expect("the writer must emit what the reader accepts");
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p beckon-core`
Expected: FAIL — `cannot find type Chord in this scope`.

- [ ] **Step 3: Write the implementation**

In `crates/beckon-core/src/shortcuts.rs`, add after the `Combo` impl block:

```rust
/// The modifiers holding Caps Lock stands for. No main key, and no `shift`.
///
/// Shift is absent from the type rather than rejected by a rule. The hook
/// has to press and release whatever is here, and releasing Shift while the
/// user is physically holding it tells Windows their Shift is up -- so
/// everything they type next arrives lowercase, silently, until they let go
/// and press it again. Making it unrepresentable means no configuration,
/// hand-written or otherwise, can reach that state. `shift` on an individual
/// binding is unaffected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Chord {
    pub ctrl: bool,
    pub super_: bool,
    pub alt: bool,
}

impl Default for Chord {
    fn default() -> Self {
        Chord {
            ctrl: true,
            super_: true,
            alt: true,
        }
    }
}

impl Chord {
    pub fn parse(s: &str) -> Result<Chord, String> {
        let (mut ctrl, mut super_, mut alt) = (false, false, false);
        for tok in s.split('+') {
            let slot = match tok {
                "ctrl" => &mut ctrl,
                "super" => &mut super_,
                "alt" => &mut alt,
                "shift" => {
                    return Err(format!(
                        "`shift` is not allowed in `{}` -- beckon has to press and \
                         release what you put here, and releasing Shift while you are \
                         holding it makes everything you type next lowercase. Put \
                         `shift` on the individual shortcut instead",
                        KEYBOARD_CAPS_HOLD
                    ))
                }
                "" => {
                    return Err(format!(
                        "`{KEYBOARD_CAPS_HOLD}` needs at least one modifier \
                         (`ctrl`, `super` or `alt`)"
                    ))
                }
                other => {
                    return Err(format!(
                        "expected a modifier in `{KEYBOARD_CAPS_HOLD}`, got `{other}` \
                         -- only `ctrl`, `super` and `alt` are allowed, and there is no \
                         main key here"
                    ))
                }
            };
            if *slot {
                return Err(format!(
                    "duplicate modifier `{tok}` in `{KEYBOARD_CAPS_HOLD}`"
                ));
            }
            *slot = true;
        }
        if !(ctrl || super_ || alt) {
            return Err(format!(
                "`{KEYBOARD_CAPS_HOLD}` needs at least one modifier \
                 (`ctrl`, `super` or `alt`)"
            ));
        }
        Ok(Chord { ctrl, super_, alt })
    }

    /// Same order `Combo::canonical` prints: ctrl, super, alt.
    pub fn canonical(&self) -> String {
        let mut parts: Vec<&str> = Vec::new();
        if self.ctrl {
            parts.push("ctrl");
        }
        if self.super_ {
            parts.push("super");
        }
        if self.alt {
            parts.push("alt");
        }
        parts.join("+")
    }

    pub fn is_default(&self) -> bool {
        *self == Chord::default()
    }
}

/// The dotted key name, used in every error message about it.
pub const KEYBOARD_CAPS_HOLD: &str = "keyboard.caps_hold";
```

Extend `KeyboardConfig`. **Keep `#[derive(Default)]`** — a derived `Default` calls each field type's own `Default::default()`, and `Chord`'s is the hand-written impl above, so the derive already yields `caps_hold = ctrl+super+alt`. Writing the impl by hand would be byte-identical and `clippy::derivable_impls` would correctly reject it.

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct KeyboardConfig {
    pub caps: bool,
    pub caps_tap: CapsTap,
    /// What holding Caps Lock stands for. Meaningful only when `caps` is
    /// true; parsed everywhere so one config file travels between machines.
    pub caps_hold: Chord,
}
```

A test must still pin the behaviour, since the guarantee lives in `Chord`'s
`Default` rather than here:

```rust
#[test]
fn the_default_keyboard_config_holds_the_default_chord() {
    assert_eq!(KeyboardConfig::default().caps_hold, Chord::default());
    assert!(!KeyboardConfig::default().caps, "the hook stays opt-in");
}
```

In `parse_keyboard` — a `match k.as_str()` over the table's keys, with `kb` as the accumulator — add a third arm after `"caps_tap"`:

```rust
            "caps_hold" => {
                let s = v.as_str().ok_or_else(|| {
                    format!(
                        "`{KEYBOARD_CAPS_HOLD}` must be a string like \"ctrl+super+alt\", got {}",
                        v.type_str()
                    )
                })?;
                kb.caps_hold = Chord::parse(s)?;
            }
```

**And update the catch-all's message**, which currently enumerates the valid settings and would now be lying:

```rust
                return Err(format!(
                    "unknown setting `keyboard.{other}` \
                     (expected `caps`, `caps_tap` or `caps_hold`)"
                ));
```

Leave the branch above it — the one that detects a shortcut nested under a `[keyboard]` header — exactly as it is.

In `crates/beckon-core/src/config_write.rs`, after the existing `caps_tap` write:

```rust
    kb["caps"] = toml_edit::value(keyboard.caps);
    kb["caps_tap"] = toml_edit::value(keyboard.caps_tap.as_str());
    // Written ONLY when it carries information. Unknown keys under
    // `keyboard` are a hard error by design, so a file that always carried
    // this key would be rejected outright by any beckon built before it
    // existed -- a real scenario when one machine updates through Scoop and
    // another has not yet.
    if keyboard.caps_hold.is_default() {
        kb.remove("caps_hold");
    } else {
        kb["caps_hold"] = toml_edit::value(keyboard.caps_hold.canonical());
    }
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p beckon-core`
Expected: PASS. Any existing test constructing `KeyboardConfig { caps, caps_tap }` by struct literal now fails to compile — add `caps_hold: Chord::default()` to each.

- [ ] **Step 5: Full host gate and commit**

```bash
cargo fmt --all
cargo test   --workspace --exclude beckon-linux --exclude beckon-windows
cargo clippy --workspace --exclude beckon-linux --exclude beckon-windows --all-targets -- -D warnings
git add crates/beckon-core/src/shortcuts.rs crates/beckon-core/src/config_write.rs
git commit -m "feat(core): keyboard.caps_hold, and shift cannot be in it

Chord has no shift field rather than a rule rejecting one. The hook must
press and release whatever is here, and releasing Shift while the user
holds it makes everything they type next lowercase -- silently. A
conditional rule would leave the trap set: configure it while caps is
off, tick caps months later, get an error about a line you do not
remember writing.

Written only when it differs from the default, because unknown keys
under keyboard are a hard error by design and a file that always carried
it would be rejected by every beckon built before today."
```

---

### Task 3: The hook's chord becomes a parameter

**Files:**
- Modify: `crates/beckon-core/src/caps.rs` (`CapsState`, `chord`, `release_modifiers`, `decide`)
- Modify: `crates/beckon-windows/src/caps_hook.rs` (carry the chord to `decide`)
- Modify: `crates/beckon-cli/src/serve.rs:661-682` (`sync_caps_hook`)
- Test: `crates/beckon-core/src/caps.rs` (`mod tests`)

**Interfaces:**
- Consumes: `shortcuts::Chord` from Task 2; `caps::VK_NONAME` from Task 1.
- Produces:
  - `fn chord(hold: Chord, vk: u32) -> Vec<Stroke>` (private)
  - `fn release_modifiers(hold: Chord) -> Vec<Stroke>` (private)
  - `pub fn decide(ev: KeyEvent, st: &mut CapsState, bound: &HashSet<u32>, hold: Chord, caps_tap: CapsTap) -> Action`
  - `CapsState.injected: Option<Chord>` (private field)
  - `pub fn caps_hook::set_bindings(bound: HashSet<u32>, hold: Chord, tap: CapsTap)`

**Why `Option<Chord>` and not `bool`:** `reload()` can run at any moment from the file watcher, including between Caps-down and Caps-up. Releasing a different set of modifiers than were pressed leaves a modifier stuck down, which is unrecoverable without killing beckon. Recording the chord that was actually injected makes that unrepresentable.

- [ ] **Step 1: Write the failing tests**

Add to `mod tests` in `crates/beckon-core/src/caps.rs`:

```rust
const HOLD: Chord = Chord {
    ctrl: true,
    super_: true,
    alt: true,
};

/// The burst contains exactly the chord's modifiers and no others.
#[test]
fn a_two_modifier_chord_injects_two_modifiers() {
    let hold = Chord::parse("ctrl+alt").unwrap();
    let out = chord(hold, 0x4E);
    assert!(!out.iter().any(|s| s.vk == VK_LWIN), "no Win asked for: {out:?}");
    assert!(out.iter().any(|s| s.vk == VK_LCONTROL));
    assert!(out.iter().any(|s| s.vk == VK_LMENU));
}

/// The property, tested structurally rather than against a fixed vector, so
/// it holds for every chord shape and not just the default.
#[test]
fn every_modifier_in_a_burst_has_a_real_key_between_its_down_and_its_up() {
    for spec in ["ctrl", "ctrl+alt", "super+alt", "ctrl+super+alt"] {
        let hold = Chord::parse(spec).unwrap();
        let out = chord(hold, 0x4E);
        for m in [VK_LCONTROL, VK_LWIN, VK_LMENU] {
            let down = out.iter().position(|s| s.vk == m && s.edge == Edge::Down);
            let up = out.iter().position(|s| s.vk == m && s.edge == Edge::Up);
            let (Some(down), Some(up)) = (down, up) else {
                continue;
            };
            assert!(
                out[down..up]
                    .iter()
                    .any(|s| !matches!(s.vk, VK_LCONTROL | VK_LWIN | VK_LMENU)),
                "{spec}: modifier {m:#x} has no real key between its down and up: {out:?}"
            );
        }
    }
}

#[test]
fn release_names_exactly_the_modifiers_the_chord_pressed() {
    let hold = Chord::parse("ctrl+alt").unwrap();
    let rel = release_modifiers(hold);
    assert!(rel.iter().any(|s| s.vk == VK_LCONTROL && s.edge == Edge::Up));
    assert!(rel.iter().any(|s| s.vk == VK_LMENU && s.edge == Edge::Up));
    assert!(
        !rel.iter().any(|s| s.vk == VK_LWIN),
        "releasing a modifier the chord never pressed desyncs one the user \
         may be genuinely holding: {rel:?}"
    );
}

/// The reason `injected` is an Option<Chord> and not a bool: the file
/// watcher can reload between Caps-down and Caps-up.
#[test]
fn changing_the_chord_mid_hold_releases_what_was_actually_pressed() {
    let old = Chord::parse("ctrl+super+alt").unwrap();
    let new = Chord::parse("ctrl+alt").unwrap();
    let mut st = CapsState::default();
    let b: HashSet<u32> = [0x4E].into_iter().collect();

    decide(down(VK_CAPITAL), &mut st, &b, old, CapsTap::CapsLock);
    decide(down(0x4E), &mut st, &b, old, CapsTap::CapsLock);
    // reload() lands here and swaps the chord.
    let act = decide(at(VK_CAPITAL, Edge::Up, 10), &mut st, &b, new, CapsTap::CapsLock);
    let Action::SwallowAndInject(rel) = act else {
        panic!("a chord was injected, so the modifiers must be released: {act:?}");
    };
    assert!(
        rel.iter().any(|s| s.vk == VK_LWIN && s.edge == Edge::Up),
        "Win was pressed by the OLD chord and must still be released: {rel:?}"
    );
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p beckon-core caps::`
Expected: FAIL — `this function takes 2 arguments but 1 was supplied` on `chord`.

- [ ] **Step 3: Write the implementation**

In `crates/beckon-core/src/caps.rs`:

```rust
use crate::shortcuts::{CapsTap, Chord};
```

Change the `injected` field:

```rust
    /// The chord that was actually injected, if any -- not merely whether
    /// one was. `reload()` can swap the configured chord between Caps-down
    /// and Caps-up, and releasing a different set than was pressed leaves a
    /// modifier stuck down, which the user cannot recover from without
    /// killing beckon.
    injected: Option<Chord>,
```

Replace `chord` and `release_modifiers`:

```rust
/// The modifier VKs a chord presses, in a fixed order.
fn modifier_vks(hold: Chord) -> Vec<u32> {
    let mut v = Vec::with_capacity(3);
    if hold.ctrl {
        v.push(VK_LCONTROL);
    }
    if hold.super_ {
        v.push(VK_LWIN);
    }
    if hold.alt {
        v.push(VK_LMENU);
    }
    v
}

fn chord(hold: Chord, vk: u32) -> Vec<Stroke> {
    let mods = modifier_vks(hold);
    let mut out = Vec::with_capacity(mods.len() * 2 + 2);
    for &m in &mods {
        out.push(Stroke {
            vk: m,
            edge: Edge::Down,
        });
    }
    out.push(Stroke {
        vk,
        edge: Edge::Down,
    });
    out.push(Stroke { vk, edge: Edge::Up });
    for &m in mods.iter().rev() {
        out.push(Stroke {
            vk: m,
            edge: Edge::Up,
        });
    }
    out
}

fn release_modifiers(hold: Chord) -> Vec<Stroke> {
    let mut out = vec![
        Stroke {
            vk: VK_NONAME,
            edge: Edge::Down,
        },
        Stroke {
            vk: VK_NONAME,
            edge: Edge::Up,
        },
    ];
    for m in modifier_vks(hold).into_iter().rev() {
        out.push(Stroke {
            vk: m,
            edge: Edge::Up,
        });
    }
    out
}
```

Update `decide`'s signature and the three `injected` sites:

```rust
pub fn decide(
    ev: KeyEvent,
    st: &mut CapsState,
    bound: &HashSet<u32>,
    hold: Chord,
    caps_tap: CapsTap,
) -> Action {
```

- in the `(VK_CAPITAL, Edge::Down)` arm: `st.injected = None;`
- in the `(VK_CAPITAL, Edge::Up)` arm: `let injected = st.injected.take();` and then
  ```rust
                if let Some(pressed) = injected {
                    Action::SwallowAndInject(release_modifiers(pressed))
                } else {
                    Action::Swallow
                }
  ```
- in the bound-key arm: `st.injected = Some(hold);` and `Action::SwallowAndInject(chord(hold, vk))`

In `crates/beckon-windows/src/caps_hook.rs`, the hook's settings live in a thread-local `CONFIG: RefCell<Config>` holding `{ bound, tap }` (declared at `:35`, read at `:60`, replaced at `:147`). Give `Config` a third field and thread it through:

```rust
struct Config {
    bound: HashSet<u32>,
    hold: Chord,
    tap: CapsTap,
}
```

- the `thread_local!` initialiser at `:35` gains `hold: Chord::default()`
- the `decide` call inside the `CONFIG.with(...)` closure at `:60` gains `c.hold` in the new fourth position
- `set_bindings` becomes:

```rust
pub fn set_bindings(bound: HashSet<u32>, hold: Chord, tap: CapsTap) {
    CONFIG.with(|c| *c.borrow_mut() = Config { bound, hold, tap });
}
```

Add `Chord` to the `beckon_core::shortcuts` import at the top of the file.

In `crates/beckon-cli/src/serve.rs`, `sync_caps_hook`, pass the chord from the parsed config:

```rust
    caps_hook::set_bindings(bound, s.keyboard.caps_hold, tap);
```

(Read `caps_hold` under the same short borrow that already reads `tap`, then drop it before the call — the borrow discipline this module documents.)

- [ ] **Step 4: Run the tests and WINCHECK**

```bash
cargo test -p beckon-core caps::
cargo check  --target x86_64-pc-windows-gnu -p beckon-windows -p beckon-cli --all-targets
cargo clippy --target x86_64-pc-windows-gnu -p beckon-windows -p beckon-cli --all-targets -- -D warnings
```
Expected: tests PASS; both `WINCHECK` commands exit 0.

- [ ] **Step 5: Full host gate and commit**

```bash
cargo fmt --all
cargo test   --workspace --exclude beckon-linux --exclude beckon-windows
cargo clippy --workspace --exclude beckon-linux --exclude beckon-windows --all-targets -- -D warnings
git add crates/beckon-core/src/caps.rs crates/beckon-windows/src/caps_hook.rs crates/beckon-cli/src/serve.rs
git commit -m "feat(caps): the injected chord is configurable, and recorded

chord() and release_modifiers() take the chord instead of hard-coding
three VKs, and CapsState remembers which chord it injected rather than
that it injected one. The file watcher can reload between Caps-down and
Caps-up; releasing a different set than was pressed leaves a modifier
stuck down and the user cannot recover from that without killing beckon.

The burst tests are now properties over every chord shape rather than
one golden vector, so they keep meaning something when the shape is not
ctrl+super+alt."
```

---

### Task 4: `bound_keys` reads what `RegisterHotKey` actually holds

Today a key enters the Caps set because it appears in the *file*. If its registration failed, pressing `Caps+<key>` injects a chord nobody is listening for.

**Files:**
- Modify: `crates/beckon-core/src/caps.rs:104-110` (`bound_keys`)
- Modify: `crates/beckon-cli/src/serve.rs:661-682` (`sync_caps_hook`)
- Test: `crates/beckon-core/src/caps.rs` (`mod tests`)

**Interfaces:**
- Consumes: `Chord` (Task 2); `serve.rs`'s existing `RegisterOutcome::by_canonical() -> HashMap<String, Result<(), String>>` (`serve.rs:279`), already assigned to `ServeState.registered` one line before `sync_caps_hook` is called (`serve.rs:455`).
- Produces: `pub fn bound_keys(registered: &HashMap<String, Result<(), String>>, hold: Chord) -> HashSet<u32>`

- [ ] **Step 1: Write the failing tests**

```rust
fn reg(pairs: &[(&str, bool)]) -> std::collections::HashMap<String, Result<(), String>> {
    pairs
        .iter()
        .map(|(k, ok)| {
            (
                k.to_string(),
                if *ok {
                    Ok(())
                } else {
                    Err("hotkey already registered".to_string())
                },
            )
        })
        .collect()
}

#[test]
fn only_successfully_registered_keys_are_reachable_through_caps() {
    let m = reg(&[
        ("ctrl+super+alt+t", true),
        ("ctrl+super+alt+e", false),
    ]);
    let b = bound_keys(&m, Chord::default());
    assert!(b.contains(&0x54), "T registered, so Caps+T must inject");
    assert!(
        !b.contains(&0x45),
        "E failed to register; injecting its chord sends a burst nobody is \
         listening for"
    );
}

#[test]
fn a_binding_on_a_different_chord_is_not_reachable_through_caps() {
    let m = reg(&[("ctrl+alt+x", true)]);
    assert!(bound_keys(&m, Chord::default()).is_empty());
}

/// The test is on the resolved modifier set, not on how the line was
/// spelled: Caps stands in for the chord, and this binding uses the chord.
#[test]
fn a_row_that_happens_to_use_the_chord_is_reachable_however_it_was_written() {
    let m = reg(&[("ctrl+super+alt+j", true)]);
    assert!(bound_keys(&m, Chord::default()).contains(&0x4A));
}

/// Shift is deliberately not part of the filter: the user's physical Shift
/// is still down while the chord is injected, so `Caps+Shift+T` arrives as
/// ctrl+super+alt+shift+t and lands on a shift binding by itself.
#[test]
fn a_shift_binding_on_the_chord_is_still_reachable() {
    let m = reg(&[("ctrl+super+alt+shift+t", true)]);
    assert!(bound_keys(&m, Chord::default()).contains(&0x54));
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p beckon-core caps::`
Expected: FAIL — `bound_keys` expects `&[Shortcut]`.

- [ ] **Step 3: Write the implementation**

```rust
/// Keys reachable through Caps: the main key of every binding that BOTH
/// carries the configured chord AND actually registered.
///
/// Keyed off the registration results rather than the file, so the contract
/// "Caps injects the chord `RegisterHotKey` is listening for" is true
/// literally instead of by assumption. A row whose registration failed is
/// absent, and pressing `Caps+<that key>` therefore injects nothing rather
/// than a burst nobody is listening for.
///
/// Shift is deliberately not part of the filter. The user's physical Shift
/// is still down while the chord is injected, so `Caps+Shift+T` arrives at
/// the system as `<chord>+shift+t` and lands on a shift binding by itself.
/// Filtering shift out here would make that binding unreachable.
pub fn bound_keys(
    registered: &std::collections::HashMap<String, Result<(), String>>,
    hold: Chord,
) -> HashSet<u32> {
    registered
        .iter()
        .filter(|(_, outcome)| outcome.is_ok())
        .filter_map(|(canonical, _)| Combo::parse(canonical).ok())
        .filter(|c| c.ctrl == hold.ctrl && c.super_ == hold.super_ && c.alt == hold.alt)
        .map(|c| c.key.win)
        .collect()
}
```

Add `use crate::shortcuts::Combo;` to the imports at the top of `caps.rs`. The `Shortcut` import may become unused — remove it if clippy says so.

In `crates/beckon-cli/src/serve.rs`, `sync_caps_hook` (around line 669), replace `beckon_core::caps::bound_keys(&s.shortcuts)` with:

```rust
            beckon_core::caps::bound_keys(&s.registered, s.keyboard.caps_hold),
```

`s.registered` is assigned from `outcome.by_canonical()` on the line before every `sync_caps_hook` call on the live path, and deliberately cleared on the paused path — where an empty bound set is the correct answer, because paused means no hotkeys are registered at all.

- [ ] **Step 4: Run the tests and WINCHECK**

```bash
cargo test -p beckon-core caps::
cargo check  --target x86_64-pc-windows-gnu -p beckon-windows -p beckon-cli --all-targets
cargo clippy --target x86_64-pc-windows-gnu -p beckon-windows -p beckon-cli --all-targets -- -D warnings
```
Expected: PASS and exit 0. Existing `bound_keys` tests that build a `Vec<Shortcut>` need rewriting onto `reg(...)`; keep every behaviour they asserted.

- [ ] **Step 5: Full host gate and commit**

```bash
cargo fmt --all
cargo test   --workspace --exclude beckon-linux --exclude beckon-windows
cargo clippy --workspace --exclude beckon-linux --exclude beckon-windows --all-targets -- -D warnings
git add crates/beckon-core/src/caps.rs crates/beckon-cli/src/serve.rs
git commit -m "fix(caps): Caps only injects chords RegisterHotKey actually holds

bound_keys read the file, so a binding whose registration failed still
got a chord injected for it -- a burst nobody is listening for. serve
already computes outcome.by_canonical() one line before calling
sync_caps_hook; passing it in makes the contract literal."
```

---

### Task 5: Caps-down reinitialises unconditionally

A lost Caps-up is real: a `WH_KEYBOARD_LL` hook is bound to the desktop of the thread that installed it and receives nothing while the secure desktop is up — UAC, Ctrl+Alt+Del, the lock screen.

**Files:**
- Modify: `crates/beckon-core/src/caps.rs:202-211` (the `(VK_CAPITAL, Edge::Down)` arm)
- Modify: `crates/beckon-windows/src/caps_hook.rs:146` (comment on `set_bindings`)
- Test: `crates/beckon-core/src/caps.rs` (`mod tests`)

**Interfaces:**
- Consumes: `CapsState` from Task 3.
- Produces: no signature change.

- [ ] **Step 1: Write the failing test**

```rust
/// A second Caps-down with no up in between is either auto-repeat -- where
/// re-stamping is harmless -- or a Caps-up that was lost. Losing one is
/// real: the hook is bound to the desktop of the thread that installed it
/// and sees nothing while the secure desktop is up (UAC, Ctrl+Alt+Del, the
/// lock screen). Treating the second down as noise leaves `down_at` pinned
/// to the first, so the next release is judged a hold and the tap is eaten.
#[test]
fn a_second_caps_down_restamps_the_clock() {
    let mut st = CapsState::default();
    let b: HashSet<u32> = HashSet::new();
    decide(at(VK_CAPITAL, Edge::Down, 0), &mut st, &b, HOLD, CapsTap::CapsLock);
    // The Caps-up here is lost to a secure-desktop excursion.
    decide(at(VK_CAPITAL, Edge::Down, 5_000), &mut st, &b, HOLD, CapsTap::CapsLock);
    let act = decide(at(VK_CAPITAL, Edge::Up, 5_050), &mut st, &b, HOLD, CapsTap::CapsLock);
    assert!(
        matches!(act, Action::SwallowAndInject(ref v) if v[0].vk == VK_CAPITAL),
        "50 ms after the second press is a tap, not a 5-second hold: {act:?}"
    );
}

/// `consumed` must survive, or a key released after Caps reaches the
/// application as an up with no matching down.
#[test]
fn a_second_caps_down_keeps_consumed() {
    let mut st = CapsState::default();
    let b: HashSet<u32> = [0x4E].into_iter().collect();
    decide(at(VK_CAPITAL, Edge::Down, 0), &mut st, &b, HOLD, CapsTap::CapsLock);
    decide(at(0x4E, Edge::Down, 1), &mut st, &b, HOLD, CapsTap::CapsLock);
    decide(at(VK_CAPITAL, Edge::Down, 2), &mut st, &b, HOLD, CapsTap::CapsLock);
    assert_eq!(
        decide(at(0x4E, Edge::Up, 3), &mut st, &b, HOLD, CapsTap::CapsLock),
        Action::Swallow,
        "the physical key-up must still be swallowed"
    );
}
```

- [ ] **Step 2: Run the tests to verify the first fails**

Run: `cargo test -p beckon-core caps::a_second_caps_down`
Expected: `a_second_caps_down_restamps_the_clock` FAILS (a 5-second hold, so no tap is injected); `a_second_caps_down_keeps_consumed` passes already.

- [ ] **Step 3: Write the implementation**

Replace the `(VK_CAPITAL, Edge::Down)` arm:

```rust
        (VK_CAPITAL, Edge::Down) => {
            // Unconditional, deliberately. A second down with no up in
            // between is either auto-repeat -- where re-stamping every field
            // is harmless -- or a Caps-up that was lost, and losing one is
            // real: this hook is bound to the desktop of the thread that
            // installed it and sees nothing at all while the secure desktop
            // is up (UAC, Ctrl+Alt+Del, the lock screen). Guarding on
            // `!st.held` pins `down_at` to the first press, so the next
            // release is judged a multi-second hold and the user's tap is
            // silently eaten.
            //
            // `consumed` is the exception and must NOT be cleared: a key
            // released after Caps must still have its physical key-up
            // swallowed, or the application receives an up with no down.
            st.held = true;
            st.used = false;
            st.injected = None;
            st.down_at = ev.time_ms;
            Action::Swallow
        }
```

In `crates/beckon-windows/src/caps_hook.rs`, above `set_bindings`:

```rust
/// Replace the binding set and the keyboard settings the hook decides with.
///
/// This function must NEVER touch `STATE`. Clearing `CapsState.consumed`
/// mid-stream leaks an unpaired key-up into whichever application has focus:
/// the key-down was swallowed, so the up must be too, and only the next
/// Caps-down may clear the set. A reload arriving while a key is held is
/// ordinary, not exceptional.
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p beckon-core caps::`
Expected: PASS.

- [ ] **Step 5: Full host gate and commit**

```bash
cargo fmt --all
cargo test   --workspace --exclude beckon-linux --exclude beckon-windows
cargo clippy --workspace --exclude beckon-linux --exclude beckon-windows --all-targets -- -D warnings
cargo check  --target x86_64-pc-windows-gnu -p beckon-windows -p beckon-cli --all-targets
cargo clippy --target x86_64-pc-windows-gnu -p beckon-windows -p beckon-cli --all-targets -- -D warnings
git add crates/beckon-core/src/caps.rs crates/beckon-windows/src/caps_hook.rs
git commit -m "fix(caps): a lost Caps-up left the clock pinned to the first press

The hook is bound to the desktop of the thread that installed it and
sees nothing while the secure desktop is up, so a Caps-up really can go
missing. Guarding the reinit on !held then judged the next release a
multi-second hold and ate the tap. consumed still survives, because a
key released after Caps must have its up swallowed too."
```

---

### Task 6: The application manifest

**Files:**
- Create: `crates/beckon-cli/beckon.exe.manifest`
- Modify: `crates/beckon-cli/beckon.rc`
- Modify: `crates/beckon-cli/build.rs`

**Interfaces:**
- Consumes: nothing.
- Produces: a process that loads comctl32 v6 and is per-monitor-v2 DPI aware. Task 8 depends on this; do not reorder.

- [ ] **Step 1: Write the manifest**

Create `crates/beckon-cli/beckon.exe.manifest`:

```xml
<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <assemblyIdentity type="win32" name="beckon" version="1.0.0.0" processorArchitecture="*"/>

  <!-- Visual styles. Without this the process loads comctl32 v5 and every
       control renders in the Windows 2000 style: raised 3D bevels, sunken
       bitmap checkboxes, an etched group box, a ListView header drawn as a
       push button. LVS_EX_DOUBLEBUFFER is also a v6-only flag and is
       silently ignored without it. -->
  <dependency>
    <dependentAssembly>
      <assemblyIdentity type="win32"
                        name="Microsoft.Windows.Common-Controls"
                        version="6.0.0.0"
                        processorArchitecture="*"
                        publicKeyToken="6595b64144ccf1df"
                        language="*"/>
    </dependentAssembly>
  </dependency>

  <!-- beckon runs at normal integrity by design. The Caps hook's UIPI gap
       is documented, not worked around. -->
  <trustInfo xmlns="urn:schemas-microsoft-com:asm.v3">
    <security>
      <requestedPrivileges>
        <requestedExecutionLevel level="asInvoker" uiAccess="false"/>
      </requestedPrivileges>
    </security>
  </trustInfo>

  <!-- Windows 10 and 11 share this GUID. Declaring it turns off the
       version-lie shim; beckon calls neither GetVersionEx nor
       VerifyVersionInfo, which Step 2 verifies. -->
  <compatibility xmlns="urn:schemas-microsoft-com:compatibility.v1">
    <application>
      <supportedOS Id="{8e0f7a12-bfb3-4fe8-b9a5-48fd50a15a9a}"/>
    </application>
  </compatibility>

  <application xmlns="urn:schemas-microsoft-com:asm.v3">
    <windowsSettings>
      <!-- ORDER IS LOAD-BEARING. Older Windows reads dpiAware and ignores
           dpiAwareness; newer Windows lets dpiAwareness win. Swap these two
           and per-monitor-v2 is silently lost on Windows 10. -->
      <dpiAware xmlns="http://schemas.microsoft.com/SMI/2005/WindowsSettings">true/pm</dpiAware>
      <dpiAwareness xmlns="http://schemas.microsoft.com/SMI/2016/WindowsSettings">PerMonitorV2</dpiAwareness>
      <longPathAware xmlns="http://schemas.microsoft.com/SMI/2016/WindowsSettings">true</longPathAware>
      <activeCodePage xmlns="http://schemas.microsoft.com/SMI/2019/WindowsSettings">UTF-8</activeCodePage>
    </windowsSettings>
  </application>
</assembly>
```

- [ ] **Step 2: Verify the supportedOS declaration is safe**

```bash
grep -rn "GetVersionEx\|VerifyVersionInfo\|RtlGetVersion" --include="*.rs" crates/
```
Expected: no output. If there is any, stop and report — declaring `supportedOS` changes what those calls return.

- [ ] **Step 3: Embed it**

`crates/beckon-cli/beckon.rc` becomes:

```
1 ICON "../../assets/beckon.ico"
1 24 "beckon.exe.manifest"
```

`24` is `RT_MANIFEST`; id `1` is `CREATEPROCESS_MANIFEST_RESOURCE_ID`, the correct id for an EXE.

In `crates/beckon-cli/build.rs`, add beside the two existing `rerun-if-changed` lines:

```rust
    println!("cargo:rerun-if-changed=beckon.exe.manifest");
```

rustc passes no manifest flags of its own — the MSVC target's `pre_link_args` is `["/NOLOGO"]` — so nothing conflicts and no `/MANIFEST:NO` is needed. `LNK4078` is a warning about duplicate section names and is unrelated; do not add guards for it.

- [ ] **Step 4: Verify the build still works on both routes**

```bash
cargo check  --target x86_64-pc-windows-gnu -p beckon-windows -p beckon-cli --all-targets
cargo clippy --target x86_64-pc-windows-gnu -p beckon-windows -p beckon-cli --all-targets -- -D warnings
cargo test   --workspace --exclude beckon-linux --exclude beckon-windows
```
Expected: all exit 0. `build.rs` only compiles the resource on `-msvc`, so the GNU route must be unaffected.

- [ ] **Step 5: Commit**

```bash
cargo fmt --all
git add crates/beckon-cli/beckon.exe.manifest crates/beckon-cli/beckon.rc crates/beckon-cli/build.rs
git commit -m "feat(windows): embed an application manifest

beckon.rc had exactly one line. Without RT_MANIFEST the process loads
comctl32 v5, so every control renders in the Windows 2000 style and
LVS_EX_DOUBLEBUFFER is silently ignored; and it is DPI-unaware, so
GetDpiForWindow returns a hard 96 and the careful scaling in
settings_window::layout has been the identity function on every machine
ever tested.

dpiAware precedes dpiAwareness on purpose: older Windows reads the first
and ignores the second, newer Windows lets the second win. Reversed,
per-monitor-v2 is lost on Windows 10 with no error."
```

---

### Task 7: Prove the manifest reached the binary

`embed-resource` 2.5.2 swallows resource-compilation failures silently. A missing icon shows up in Explorer within seconds; a missing manifest is invisible until someone looks at a 150 % display. The build must fail instead.

**Files:**
- Modify: `.github/workflows/ci.yml`

**Interfaces:**
- Consumes: the manifest from Task 6.
- Produces: a CI step that fails when `RT_MANIFEST` is absent from the built `beckon-serve.exe`.

- [ ] **Step 1: Add the failing gate**

In `.github/workflows/ci.yml`, in the `build & test` job, after the build step, add a Windows-only step:

```yaml
      - name: manifest reached the binary
        if: matrix.os == 'windows-latest'
        shell: pwsh
        run: |
          # embed-resource swallows resource-compilation failures silently, so
          # the only trustworthy check is reading the artefact back. The
          # manifest is stored as raw UTF-8 XML in the resource section, so a
          # byte search for its most distinctive literal is sufficient --
          # nothing else in beckon contains this string. It cannot tell
          # RT_MANIFEST from an incidental copy, which is a limit worth
          # accepting for two lines of CI.
          $exe = "target/debug/beckon-serve.exe"
          if (-not (Test-Path $exe)) { throw "not built: $exe" }
          $bytes = [IO.File]::ReadAllBytes($exe)
          $text  = [Text.Encoding]::UTF8.GetString($bytes)
          foreach ($needle in @("Microsoft.Windows.Common-Controls", "PerMonitorV2")) {
            if ($text -notmatch [regex]::Escape($needle)) {
              throw "manifest missing from $exe (no '$needle'). embed-resource fails silently; check beckon.rc and beckon.exe.manifest."
            }
          }
          Write-Host "manifest present"
```

- [ ] **Step 2: Prove the gate catches a missing manifest**

Locally, temporarily comment out the manifest line in `beckon.rc`, then run the same logic against a native Windows build if one is available. If no Windows host is available, verify the *logic* by running the PowerShell body against a file known not to contain the strings:

```bash
pwsh -c '$bytes=[IO.File]::ReadAllBytes("Cargo.toml"); $t=[Text.Encoding]::UTF8.GetString($bytes); if ($t -notmatch "PerMonitorV2") { Write-Host "gate would fail: correct" } else { throw "gate is broken" }'
```
Expected: `gate would fail: correct`. Restore `beckon.rc` afterwards.

A gate that has never been seen to fail is not a gate. If neither check is possible in this environment, record that explicitly in the commit message rather than claiming it was verified.

- [ ] **Step 3: Commit**

```bash
git add .github/workflows/ci.yml
git commit -m "ci: fail the build when the manifest does not reach the binary

embed-resource 2.5.2 swallows resource-compilation failures silently. A
missing icon is visible in Explorer in seconds; a missing manifest is
invisible until someone looks at a 150% display. Reading the artefact
back is the only check that does not trust the build tool."
```

---

### Task 8: DPI, for the first time

`GetDpiForWindow` has returned a hard 96 because the process was DPI-unaware, so `let s = |v| v * dpi as i32 / 96` at `settings_window.rs:524-525` has been the identity function on every machine ever tested. Task 6 changes that, and four sites must change with it.

**Files:**
- Modify: `crates/beckon-windows/src/settings_window.rs` — `ui_font` (~260-279), `child` column setup (~334-352), `create` (~237-251), `wndproc`'s `WM_SIZE | WM_DPICHANGED` arm (~809-812), `Ui` (add a `dpi` field)

**Interfaces:**
- Consumes: the manifest from Task 6.
- Produces: no public signature change. `Ui` gains `dpi: u32`.

- [ ] **Step 1: Rebuild the font per DPI, and stop leaking it**

`ui_font` currently uses `SystemParametersInfoW(SPI_GETNONCLIENTMETRICS)`, which is the wrong API for a per-monitor process, and the `HFONT` it returns is created once in `build_children` and never recreated — one is leaked per window open. Replace with:

```rust
/// The shell's UI font at a specific DPI.
///
/// `SystemParametersInfoForDpi` and not `SystemParametersInfoW`: the latter
/// answers for the system DPI, which is the wrong number for a
/// per-monitor-v2 process on a secondary display.
unsafe fn ui_font(dpi: u32) -> HFONT {
    let mut ncm = NONCLIENTMETRICSW {
        cbSize: std::mem::size_of::<NONCLIENTMETRICSW>() as u32,
        ..Default::default()
    };
    let ok = SystemParametersInfoForDpi(
        SPI_GETNONCLIENTMETRICS.0,
        ncm.cbSize,
        Some(&mut ncm as *mut _ as *mut _),
        0,
        dpi,
    )
    .is_ok();
    if ok {
        let f = CreateFontIndirectW(&ncm.lfMessageFont);
        if !f.is_invalid() {
            return f;
        }
    }
    HFONT(GetStockObject(DEFAULT_GUI_FONT).0)
}
```

Add `Win32_UI_HiDpi` items to the `use` list as needed (`SystemParametersInfoForDpi` lives beside `GetDpiForWindow`).

- [ ] **Step 2: Give `WM_DPICHANGED` its own arm**

It is currently folded in with `WM_SIZE`, calls `layout` and returns, discarding `lParam` — which is where Windows puts the rectangle the window must move to. Replace the arm:

```rust
            WM_SIZE => {
                layout(hwnd);
                LRESULT(0)
            }
            WM_DPICHANGED => {
                // HIWORD(wParam) is the new DPI; lParam is a RECT with the
                // position and size Windows wants. Ignoring lParam leaves
                // the window the wrong size on the new monitor, and no
                // second message arrives to correct it.
                let dpi = ((wp.0 >> 16) & 0xFFFF) as u32;
                let font = ui_font(dpi);
                let old = UI.with(|u| {
                    u.borrow_mut().as_mut().map(|ui| {
                        let prev = ui.font;
                        ui.font = font;
                        ui.dpi = dpi;
                        prev
                    })
                });
                // Every child must be told, including ones `layout` places
                // through GetDlgItem rather than a stored handle.
                let mut child = GetWindow(hwnd, GW_CHILD).unwrap_or_default();
                while !child.is_invalid() {
                    SendMessageW(child, WM_SETFONT, Some(WPARAM(font.0 as usize)), Some(LPARAM(1)));
                    child = GetWindow(child, GW_HWNDNEXT).unwrap_or_default();
                }
                if let Some(prev) = old {
                    let _ = DeleteObject(HGDIOBJ(prev.0));
                }
                let rc = &*(lp.0 as *const RECT);
                let _ = SetWindowPos(
                    hwnd,
                    None,
                    rc.left,
                    rc.top,
                    rc.right - rc.left,
                    rc.bottom - rc.top,
                    SWP_NOZORDER | SWP_NOACTIVATE,
                );
                layout(hwnd);
                LRESULT(0)
            }
```

In `build_children`, take the font from `ui_font(GetDpiForWindow(hwnd).max(96))` and store that DPI in `Ui.dpi`.

- [ ] **Step 3: Scale the creation size and the columns**

In `create`, the literal `760, 560` are physical pixels under per-monitor-v2, so on a 192-DPI display the window comes out half the intended size and no `WM_DPICHANGED` arrives to fix it. `CreateWindowExW` is called before there is an `HWND` to query, so use the DPI of the monitor under the cursor:

```rust
    // CW_USEDEFAULT for position, but the SIZE must be scaled by hand:
    // under per-monitor-v2 these are physical pixels, and no WM_DPICHANGED
    // arrives to correct a window that was born the wrong size.
    let dpi = {
        let mut pt = POINT::default();
        let _ = GetCursorPos(&mut pt);
        let mon = MonitorFromPoint(pt, MONITOR_DEFAULTTOPRIMARY);
        let (mut x, mut y) = (96u32, 96u32);
        let _ = GetDpiForMonitor(mon, MDT_EFFECTIVE_DPI, &mut x, &mut y);
        x.max(96)
    };
    let w = MulDiv(760, dpi as i32, 96);
    let h = MulDiv(560, dpi as i32, 96);
```

and pass `w, h` to `CreateWindowExW`.

In `build_children`, the ListView column widths `(34, 190, 150)` are raw literals. Scale them with the same DPI stored in `Ui`:

```rust
    let dpi = GetDpiForWindow(hwnd).max(96);
    let sx = |v: i32| MulDiv(v, dpi as i32, 96);
    for (i, (title, cx)) in [("", 34), ("Shortcut", 190), ("App", 150)]
        .iter()
        .enumerate()
    {
        // ...
        let col = LVCOLUMNW {
            cx: sx(*cx),
            // ...
        };
```

- [ ] **Step 4: WINCHECK**

```bash
cargo check  --target x86_64-pc-windows-gnu -p beckon-windows -p beckon-cli --all-targets
cargo clippy --target x86_64-pc-windows-gnu -p beckon-windows -p beckon-cli --all-targets -- -D warnings
```
Expected: both exit 0. If `SystemParametersInfoForDpi` or `GetDpiForMonitor` needs a `windows` feature not yet enabled, add it to `crates/beckon-windows/Cargo.toml` — this does not count against the "no new dependency" constraint, which is about crates.

- [ ] **Step 5: Commit**

```bash
cargo fmt --all
git add crates/beckon-windows/src/settings_window.rs crates/beckon-windows/Cargo.toml
git commit -m "fix(windows): make the DPI code actually run

s() has been the identity function on every machine ever tested,
because a DPI-unaware process is told 96 whatever the display does. The
manifest changes that and four sites had to change with it:
WM_DPICHANGED was folded in with WM_SIZE and threw away the rectangle
Windows puts in lParam; the font was built once with the system-DPI API
and leaked one HFONT per window open; the creation size was in physical
pixels, so a 192-DPI display got a half-size window and no message to
correct it; and the ListView columns never went through s() at all."
```

---

### Task 9: A window floor, height clamps, `CB_SETMINVISIBLE`, and an icon

**Files:**
- Modify: `crates/beckon-windows/src/settings_window.rs` — `wndproc` (add `WM_GETMINMAXINFO`), `layout` (~519-612), `build_children` (combo box), `create` (`WNDCLASSW`)

**Interfaces:**
- Consumes: Task 8's `Ui.dpi`.
- Produces: no signature change.

- [ ] **Step 1: Add the window floor**

The window has `WS_THICKFRAME` and no minimum. Drag it below roughly 274 px of client height and the notes `STATIC` receives a negative `cy`; below 170 px the list does. Add to `wndproc`:

```rust
            WM_GETMINMAXINFO => {
                // A frame promise, not an arithmetic one -- Step 2 clamps
                // independently, because a floor does not make subtraction
                // safe, it only makes it unlikely.
                let dpi = UI.with(|u| u.borrow().as_ref().map(|x| x.dpi)).unwrap_or(96);
                let mm = &mut *(lp.0 as *mut MINMAXINFO);
                mm.ptMinTrackSize.x = MulDiv(720, dpi as i32, 96);
                mm.ptMinTrackSize.y = MulDiv(460, dpi as i32, 96);
                LRESULT(0)
            }
```

- [ ] **Step 2: Clamp every computed height at zero**

In `layout`, wrap the derived heights. Add near the top, after `s` is defined:

```rust
    // Independent of WM_GETMINMAXINFO: the floor is about the frame, and a
    // clamp is about the arithmetic. Either alone leaves a negative cy
    // reachable -- SetWindowPos with one produces a control the user can
    // never see or focus again.
    let clamp = |v: i32| v.max(0);
```

and apply `clamp(...)` to `mid_h`, to the list height `mid_h - btn_h - s(6)`, and to the notes height `mid_h - (y - top) - btn_h - s(6)`.

- [ ] **Step 3: `CB_SETMINVISIBLE` on the App combo**

Under comctl32 v6 a combo box's drop-down height is no longer governed by the `cy` passed to `CreateWindow`/`SetWindowPos`; it is governed by the minimum-visible-items count, default 30. `layout` passes `row * 8` at line 590 for exactly that purpose and stops working. After creating the `app` combo in `build_children`:

```rust
    // Under comctl32 v6 the `cy` passed to SetWindowPos no longer decides
    // how tall the drop-down is; this does. Without it the list opens at
    // the default 30 items regardless of the height layout computes.
    SendMessageW(app, CB_SETMINVISIBLE, Some(WPARAM(8)), Some(LPARAM(0)));
```

- [ ] **Step 4: Give the window class an icon**

`WNDCLASSW.hIcon` and `hIconSm` are null, so the title bar, taskbar and Alt-Tab show the default icon while the tray shows beckon's. In `create`, before `RegisterClassW`:

```rust
    // Resource id 1, the same icon beckon.rc embeds and the tray already
    // uses. Null here is why the title bar and Alt-Tab show the default
    // while the tray shows beckon's.
    let icon = LoadIconW(Some(hinst.into()), PCWSTR(1 as *const u16)).unwrap_or_default();
```

and set `hIcon: icon, hIconSm: icon` in the `WNDCLASSW` literal.

- [ ] **Step 5: WINCHECK and commit**

```bash
cargo fmt --all
cargo check  --target x86_64-pc-windows-gnu -p beckon-windows -p beckon-cli --all-targets
cargo clippy --target x86_64-pc-windows-gnu -p beckon-windows -p beckon-cli --all-targets -- -D warnings
cargo test   --workspace --exclude beckon-linux --exclude beckon-windows
git add crates/beckon-windows/src/settings_window.rs
git commit -m "fix(windows): a floor, a clamp, a drop-down height and an icon

The window had WS_THICKFRAME and no minimum, so dragging it small handed
SetWindowPos a negative cy and produced controls that could never be
seen or focused again. The floor and the clamp are independent on
purpose: one is a promise about the frame, the other about the
arithmetic.

CB_SETMINVISIBLE because under v6 the drop-down height stops obeying the
cy layout computes -- this is the metric v6 actually breaks, not a
constant that drifts. And the class had no icon, so the title bar and
Alt-Tab showed the default while the tray showed beckon's."
```

---

### Task 10: The measurement pass on a14

Nothing in Landing 2 can be tuned until this has run. Every number in Part B of the spec is currently guessed against metrics the shipped binary has never used.

**Files:**
- Modify: `crates/beckon-windows/examples/caps_probe.rs` (add the release-burst case)
- Create: `docs/superpowers/measurements/2026-08-XX-landing-1-a14.md` (results)

**Interfaces:**
- Consumes: everything above.
- Produces: the font face and size, the real DPI, themed control heights under v6, and a verdict on the `VK_NONAME` filler. Landing 2's spacing tokens are derived from these.

**How to run anything on a14:** SSH lands in **session 0**, which has no desktop and no keyboard, so every UI result there is a confident false negative. Go through a scheduled task in session 1, registered with `New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries` — `schtasks`' defaults refuse to start on battery and leave the task `Queued` forever on a laptop. Use `-EncodedCommand` for PowerShell, and a `.bat` for anything with a redirect. Build with `cargo build --all-targets`; `--examples` does not build `[[bin]]` targets and you will measure a stale `beckon-serve.exe`.

- [ ] **Step 1: Capture the "before" control run**

Check out the commit **before** Task 6, build, and record: a screenshot of the settings window at 100 % and at 150 % scaling, plus the output of a one-off probe printing `GetDpiForWindow` and `lfMessageFont.lfFaceName` / `lfHeight`.

Without this control, a broken probe and a clean result are indistinguishable — the lesson the 2026-08-11 tray measurements already paid for.

- [ ] **Step 2: Capture the "after" run**

Return to `HEAD`, rebuild, and record the same four things. Expected: `GetDpiForWindow` now reports the real DPI; the screenshots show themed controls.

- [ ] **Step 3: Measure the metrics Landing 2 needs**

Record, at 100 % and 150 %:
- `lfMessageFont.lfFaceName` and `lfHeight` (expected `Segoe UI`, `-12` at 96 DPI — Segoe UI Variable reaches the shell through DirectWrite, never through `NONCLIENTMETRICS`)
- the ListView row height (`LVM_GETITEMRECT` on item 0) and header height
- a themed `BUTTON`'s natural height, and an `EDIT`'s

- [ ] **Step 4: Measure the `VK_NONAME` filler, both ways**

Extend `caps_probe` with a case that deliberately truncates a chord burst — send only `Ctrl↓ Win↓ Alt↓`, no main key — then emits `release_modifiers()`, and asks whether the Start menu opened. Run it **with and without** the filler.

The control is a bare Win tap already proven to open Start; it must be in the same run. A result of "Start did not open" from a probe that cannot detect Start opening at all looks identical to success.

- [ ] **Step 5: Write the results down and commit**

Record every number in `docs/superpowers/measurements/2026-08-XX-landing-1-a14.md`, including anything that came out differently from what this plan predicted. Then update the spec's Part B token table (`§B.2`) with the real row and control heights, replacing the guesses.

```bash
git add docs/superpowers/measurements/ docs/superpowers/specs/2026-08-11-settings-window-redesign.md crates/beckon-windows/examples/caps_probe.rs
git commit -m "test(a14): measure what the manifest changed, and the filler key

Landing 2's spacing tokens were guesses against metrics the shipped
binary had never used, because s() was the identity function. These are
the real numbers, with a pre-manifest control run so a broken probe and
a clean result cannot look the same."
```

---

## Self-review

**Spec coverage — Landing 1 (Part A + Part D + the parsing half of Part C):**

| Spec section | Task |
|---|---|
| §A.1 the manifest | 6 |
| §A.1.1 verification gate | 7 |
| §A.1.2 `CB_SETMINVISIBLE` | 9 |
| §A.2 DPI, four sites | 8 |
| §A.3 floor, clamps, class icon | 9 |
| §C.2 `Chord`, `caps_hold`, write-when-non-default | 2 |
| §C.3 validation | 2 |
| §D.1 filler key | 1 |
| §D.2 chord as a parameter, `Option<Chord>` | 3 |
| §D.3 `bound_keys` from registration | 4 |
| §D.4 unconditional Caps-down reinit | 5 |
| §D.5 `set_bindings` comment | 5 |
| Measurement gate (§F.5, §F.6, Testing 1–2) | 10 |

**Not in this plan, by design:** Part B (window layout), §C.1/§C.4 (short-form rendering, the editor), Part E (suggestions), Part F (capture, probe, list checkboxes). Those are Landings 2a, 2b and 3 and each needs its own plan; 2a's constants come from Task 10.

**Deviations from the spec, both tightenings, both recorded in the tasks:**
1. `Chord` has no `shift` field at all, rather than rejecting shift when `caps = true` (§C.3). Reason in Task 2.
2. The measurement pass is a task rather than a checklist item, because it gates the next plan.

**Type consistency:** `Chord` (Task 2) is consumed with the same field names by `modifier_vks`/`chord`/`release_modifiers`/`decide` (Task 3) and `bound_keys` (Task 4). `decide`'s parameter order — `(ev, st, bound, hold, caps_tap)` — is used identically in Task 3's and Task 5's tests. `caps_hook::set_bindings(bound, hold, tap)` (Task 3) matches its one call site in `sync_caps_hook` (Tasks 3 and 4). `Ui.dpi` is introduced in Task 8 and read in Task 9.

**Placeholder scan:** clean. Every code step carries the code. The one instruction that names a range rather than quoting it — "apply `clamp(...)` to `mid_h`, the list height and the notes height" in Task 9 Step 2 — gives the three exact expressions to wrap.

**Two facts checked after the first draft, both of which changed a task:**
1. `parse_keyboard` is a `match k.as_str()` whose catch-all message enumerates the valid settings (`"expected \`caps\` or \`caps_tap\`"`). Adding a key without updating that string leaves the error lying to the next person who typos one. Task 2 Step 3 now says so.
2. `caps_hook.rs` keeps its settings in `thread_local! { static CONFIG: RefCell<Config> }` with `Config { bound, tap }` at `:35`, `:60` and `:147`. Task 3 Step 3 now names all three sites instead of saying "store it in the config".
