# Landing 2b-iv: the availability probe — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Tell the user whether the shortcut they are editing is actually
free, without ever claiming more than a registration can support.

**Architecture:** §F.6 says the **order** is load-bearing — F12 guard, then
the in-memory self-conflict check, and only then the live registration. That
order and every string live in `beckon-core`, where all three CI jobs test
them; `beckon-windows` contributes only the one thing it must, a
`RegisterHotKey` round trip.

**Tech Stack:** Rust. `beckon-core`, `beckon-windows`, `beckon-cli`. No new
dependencies.

**Spec:** `docs/superpowers/specs/2026-08-11-settings-window-redesign.md` §F.6.

## Global Constraints

- **ABORT-CLASS.** Never hold a `RefCell` borrow of `UI` or `ServeState`
  across any `SendMessageW` / `PostMessageW` / `SetWindowPos` / `SetFocus` /
  `SetWindowTextW`. A second borrow across the `extern "system"` wndproc
  boundary **aborts the process** rather than unwinding, and no tool sees it.
- **Do not change *when* `layout` runs.**
- **Display strings are ASCII**, and §F.6 gives them **verbatim** — copy them
  exactly, including punctuation. Comments and test assertion messages are
  exempt.
- **The verdict travels through `RuntimeStatus`, never through
  `Model::problems`.** `problems()` is pure and is what keeps `apply_enabled`
  testable on the Linux and macOS jobs; a live OS fact in it would make that
  untestable everywhere but Windows.
- **Never consult `ServeState.registered` to decide whether a chord is free.**
  `set_paused` and `reload` *clear* that map, so probing while paused would
  report beckon's own bound chord as free. `registered` explains why a row is
  red; it never decides availability.
- No new dependencies.
- Gates: `cargo fmt --all -- --check`, `cargo test -p beckon-core`,
  `cargo clippy -p beckon-core --all-targets -- -D warnings`,
  `cargo check --target x86_64-pc-windows-gnu -p beckon-windows --all-targets`,
  `cargo check --target x86_64-pc-windows-gnu -p beckon-cli`.
- `cargo test --workspace` is **already broken on macOS**; pre-existing, ignore it.

---

## File Structure

| File | Responsibility | Task |
|---|---|---|
| `crates/beckon-core/src/settings.rs` | `Availability`, `ProbeResult`, `probe_plan`, the strings, `RuntimeStatus::probe` | 1 |
| `crates/beckon-windows/src/hotkey.rs` | `probe_chord` — one `RegisterHotKey` round trip | 2 |
| `crates/beckon-windows/src/settings_window.rs` | ask for a probe when the shortcut changes | 2 |
| `crates/beckon-cli/src/serve.rs` | run the plan, store the verdict | 2 |

---

## Task 1: the decision, and every string

**Files:**
- Modify: `crates/beckon-core/src/settings.rs`
- Test: same file, `mod tests`

**Interfaces:**
- Consumes: `shortcuts::Combo`, `Row`.
- Produces: `Availability`, `ProbeResult`, `probe_plan`, `probe_notes`,
  `RuntimeStatus::probe`. Task 2 calls `probe_plan` and fills `probe`.

- [ ] **Step 1: Write the failing tests**

```rust
    // ---------- the availability probe ----------

    fn rows3() -> Model {
        Model::from_text(
            "\"ctrl+alt+a\"=\"Notepad\"\n\"ctrl+alt+b\"=\"Brave\"\n\"ctrl+alt+q\"=\"Weather\"\n",
        )
        .unwrap()
    }

    /// F12 is reserved for debuggers "at all times", so a successful
    /// registration proves nothing. It has to be refused BEFORE the OS is
    /// asked, or the probe reports a green Available on a key documented
    /// never to arrive.
    #[test]
    fn f12_is_refused_before_the_os_is_asked() {
        let m = rows3();
        assert_eq!(
            probe_plan(&m, 0, "ctrl+alt+f12"),
            ProbePlan::Verdict(Availability::F12)
        );
    }

    #[test]
    fn a_combo_already_in_this_file_is_a_self_conflict() {
        let m = rows3();
        // Row 0 is being edited to what row 1 already holds.
        assert_eq!(
            probe_plan(&m, 0, "ctrl+alt+b"),
            ProbePlan::Verdict(Availability::DuplicateInFile { app: "Brave".into() })
        );
    }

    /// A row keeping its own combo is not a conflict with itself.
    #[test]
    fn a_row_keeping_its_own_combo_is_unchanged_not_a_duplicate() {
        let m = rows3();
        assert_eq!(
            probe_plan(&m, 0, "ctrl+alt+a"),
            ProbePlan::Verdict(Availability::Unchanged)
        );
    }

    /// Only when nothing above matched may the OS be asked. Getting this
    /// order wrong is what makes a probe claim a reserved or duplicated
    /// chord is free.
    #[test]
    fn a_clean_combo_reaches_the_os() {
        let m = rows3();
        assert_eq!(probe_plan(&m, 0, "ctrl+alt+z"), ProbePlan::AskTheOs);
    }

    #[test]
    fn an_unparseable_combo_never_reaches_the_os() {
        let m = rows3();
        assert!(matches!(
            probe_plan(&m, 0, "banana"),
            ProbePlan::Verdict(_)
        ));
    }

    /// The strings are the spec's, verbatim, and a free verdict must never
    /// say the shortcut WORKS -- only that nothing else is holding it.
    #[test]
    fn a_free_verdict_does_not_claim_the_shortcut_works() {
        let n = probe_notes(&ProbeResult { combo: "ctrl+alt+z".into(), verdict: Availability::Free }, false);
        assert_eq!(n[0].text, "Available. Nothing else on this PC is using it.");
        assert!(
            !n.iter().any(|x| x.text.to_lowercase().contains("works")),
            "a registration proves nothing else holds the chord, not that it fires"
        );
    }

    #[test]
    fn a_windows_key_chord_says_windows_may_take_it_back() {
        let n = probe_notes(&ProbeResult { combo: "super+z".into(), verdict: Availability::FreeWithWin }, false);
        assert_eq!(
            n[0].text,
            "Available right now. Windows reserves Windows-key shortcuts and can take this one back after an update, so press it once after saving to be sure."
        );
    }

    #[test]
    fn a_taken_chord_does_not_name_a_program_it_cannot_know() {
        let n = probe_notes(&ProbeResult { combo: "ctrl+alt+z".into(), verdict: Availability::Taken }, false);
        assert_eq!(
            n[0].text,
            "Another program already has this shortcut. Windows does not tell beckon which one, so beckon cannot name it. Saved as-is, it will not fire."
        );
        assert_eq!(n[0].mark, Mark::Bad);
    }

    #[test]
    fn probing_while_paused_says_so() {
        let n = probe_notes(&ProbeResult { combo: "ctrl+alt+z".into(), verdict: Availability::Free }, true);
        assert!(
            n.iter().any(|x| x.text == "beckon is paused, so this shows what will happen when you resume."),
            "the verdict is about the future while paused, and must say so"
        );
    }

    /// No string may leak an API name or an error code.
    #[test]
    fn no_string_names_an_api() {
        for v in [
            Availability::Free,
            Availability::FreeWithWin,
            Availability::Unchanged,
            Availability::Taken,
            Availability::F12,
            Availability::CaptureSawNothing,
            Availability::DuplicateInFile { app: "X".into() },
        ] {
            for n in probe_notes(&ProbeResult { combo: "ctrl+alt+z".into(), verdict: v }, false) {
                for bad in ["RegisterHotKey", "UIPI", "0x", "HRESULT"] {
                    assert!(!n.text.contains(bad), "{bad} leaked into {:?}", n.text);
                }
            }
        }
    }
```

- [ ] **Step 2: Run and confirm they fail**

Run: `cargo test -p beckon-core --lib settings::tests 2>&1 | tail -20`

- [ ] **Step 3: Implement**

`Availability`, `ProbeResult { combo: String, verdict: Availability }` and
`ProbePlan { Verdict(Availability), AskTheOs }`, all `PartialEq` so the tests
above compare them.

`probe_plan(m: &Model, row: usize, combo: &str) -> ProbePlan`, **in this
order and no other**:

1. `Combo::parse` fails → `Verdict(...)`; the row's own `problems()` already
   explains why, so pick the verdict that says nothing new.
2. F12 anywhere in the chord → `Verdict(F12)`.
3. Equal (canonically) to what `m.rows[row]` already holds →
   `Verdict(Unchanged)`.
4. Equal (canonically) to any **other** row's combo →
   `Verdict(DuplicateInFile { app })`.
5. Otherwise → `AskTheOs`.

Write the ordering rationale as a comment: each earlier step is a fact the OS
cannot tell us, and asking the OS first would let a reserved or duplicated
chord come back green.

`probe_notes(r: &ProbeResult, paused: bool) -> Vec<Note>` returns the §F.6
strings verbatim, with the paused sentence appended when `paused`, and the
`Ctrl+Alt`-without-Win AltGr sentence appended when the combo has ctrl and
alt and no super.

`RuntimeStatus` gains:

```rust
    /// The last probe verdict, and the combo it was about. `None` until one
    /// has run -- **not-yet-probed is not the same as free**, the same
    /// distinction `catalog` makes.
    ///
    /// The combo is carried so a verdict for a chord the user has since
    /// changed can be ignored rather than shown against the new one.
    pub probe: Option<ProbeResult>,
```

`row_condition` folds `probe_notes` in for the **selected** row only, and
only when `probe.combo` equals that row's combo canonically.

- [ ] **Step 4: Run and confirm they pass**

- [ ] **Step 5: Break the order on purpose**

Move the F12 check after the self-conflict check and re-run: nothing should
change (they are independent). Then move `AskTheOs` to the top — i.e. return
it before every guard — and re-run: `f12_is_refused_before_the_os_is_asked`,
`a_combo_already_in_this_file_is_a_self_conflict`,
`a_row_keeping_its_own_combo_is_unchanged_not_a_duplicate` and
`an_unparseable_combo_never_reaches_the_os` must all FAIL. Restore.

Report both results. The first is the honest half: not every reordering is a
defect, and saying which are is more useful than claiming the whole order is
load-bearing.

- [ ] **Step 6: Gates and commit**

```bash
git add crates/beckon-core/src/settings.rs
git commit -m "feat(core): decide whether a shortcut is free, in the order F.6 mandates

F12 first, then the in-memory self-conflict check, and only then may the OS
be asked -- each earlier step is a fact RegisterHotKey cannot tell us, and
asking it first lets a reserved or duplicated chord come back green. VK_F12
is reserved for debuggers at all times, so a successful registration on it
proves nothing.

The verdict rides on RuntimeStatus, never on Model::problems, because
problems() is pure and is what keeps apply_enabled testable on the two CI
jobs that are not Windows. The strings are the spec's verbatim, and a test
pins that a free verdict never claims the shortcut WORKS -- a registration
proves only that nothing else is holding it."
```

---

## Task 2: ask the OS, once, safely

**Files:**
- Modify: `crates/beckon-windows/src/hotkey.rs`, `settings_window.rs`
- Modify: `crates/beckon-cli/src/serve.rs`

**Interfaces:**
- Consumes: `probe_plan`, `Availability`, `ProbeResult` (Task 1).
- Produces: `hotkey::probe_chord(hwnd, &Combo) -> Availability`.

- [ ] **Step 1: `probe_chord`**

One `RegisterHotKey` / `UnregisterHotKey` round trip, and the four rules
§F.6 gives are all structural:

- **It registers on the SETTINGS WINDOW's `HWND`**, with one fixed id — not
  on `tray_hwnd`. A hotkey is identified by the `(hWnd, id)` pair, so a
  different window makes a collision with the live table impossible by
  construction. Picking "an id high enough" on `tray_hwnd` would be a bet on
  config size, and the live ids are row indices. Getting the pair wrong is
  worse than it sounds: MSDN says a duplicate `(hWnd, id)` is *maintained
  alongside* the existing one, and `UnregisterHotKey` then removes an
  unspecified one of the two — a silently dead hotkey.
- **It unregisters on every exit path**, including the failure path. A
  cancelled probe must not leave a global hotkey claimed.
- **It runs on the thread that owns the window** — `RegisterHotKey` is
  thread-affine.
- **It is never called from inside a hook callback.**

Success → `Free`, or `FreeWithWin` when the combo carries the Windows key.
Failure → `Taken`. Do not decode the error into a message: §F.6's string says
Windows does not tell beckon which program holds it, and that is true.

- [ ] **Step 2: Ask when the shortcut changes**

The window asks for a probe when the shortcut controls change and a key is
selected. Debounce is not needed — the typed path changes on a click, not a
keystroke — but the probe must not run on every `apply_state` push: it is a
global OS mutation, however brief.

- [ ] **Step 3: Wire it**

`serve.rs` runs `probe_plan`, calls `probe_chord` only on `AskTheOs`, stores
a `ProbeResult` in `RuntimeStatus.probe`, and refreshes.

**After Apply, the probe verdict is replaced by the real registration
result** — `register_all` is the authority and the window already receives it
through `registered`. Clear `probe` on save so a stale "Available" cannot
outlive the thing that would disprove it.

- [ ] **Step 4: Gates, ABORT-class read-through, commit**

Run all five gates, then read your own diff against the ABORT-class rule and
say what you checked.

---

## Self-Review

**Spec coverage:** §F.6's ordering is Task 1 step 3; the `(hWnd, id)`
argument and the unregister-on-every-path rule are Task 2 step 1; the
`RuntimeStatus`-not-`problems()` rule is Task 1; the strings table is Task 1
with one test per row; "what the probe may not promise" is the
`a_free_verdict_does_not_claim_the_shortcut_works` test plus clearing `probe`
on save.

**Deliberately NOT here:** the `Working. beckon received {combo}.` string,
which §F.6 says only a real keypress after Apply may produce — that needs the
hook, and belongs with 2b-v. `Availability::CaptureSawNothing` is defined here
because it is one of the strings, but nothing produces it until capture exists.
