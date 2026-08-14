# `beckon check --resolve`: what this machine actually thinks of your config

**Date**: 2026-08-14
**Status**: design, not built
**Scope**: `crates/beckon-cli/src/lib.rs` (the `Check` variant, `cmd_check`,
`cmd_resolve`), a new `Certainty` vocabulary in `beckon-core`, and one
structured-report entry point per OS crate. Plus two rollout items in the
user's `~/.nix` repo.
**Chosen over**: a richer TOML grammar (candidate lists, a per-binding `match`
floor). Those remain the intended next steps; §9 records exactly what this
design leaves room for and why the order is tooling-first.

---

## 1. The problem, in the config's own words

Three comments in `~/.nix/configs/shortcuts/apps.*.toml` are each the written-up
result of a manual experiment:

- `apps.macos.toml:15` — *"DUNG doi thanh `Terminal`: tren mac no exact-match
  NHAM Terminal.app cua Apple"*
- `apps.macos.toml:16` — *"doi tu `Brave` 10/08/2026: `Brave` chi khop bang
  substring (tier thap nhat, phai quet catalog); `Brave Browser` khop
  running-name exact, khong cham catalog"*
- `apps.windows.toml:40` — *"exact tren Windows; `Telegram Web` KHONG resolve
  (Cap+t tung hong am tham vi the)"*

All three ask one question — **which tier does this name match at, on this
machine?** — and beckon already computes the answer on every invocation. It
just never exposes it outside `beckon resolve <ID>`, one id at a time.

`cmd_check` is six lines (`lib.rs:360-367`): read the file, `parse_shortcuts`,
print a count. It validates **shape** and never **meaning** — no app name is
resolved, no catalog is consulted, no OS is asked anything.

What that costs, from measurements already recorded in `CLAUDE.md`:

- Windows: an exact name is **~57 ms**; falling through to the catalog scan is
  **~400-945 ms**, *on every keypress*, for the life of the binding.
- macOS: the substring tier fires `installed_loader()` — the full
  `/Applications` + `/System/Applications` + `~/Applications` plist walk.
- And the dangerous case is not slow, it is **silent**: `Terminal` matches
  Apple's Terminal.app at tier 1 with no signal of any kind.

A binding can therefore be wrong, or 8x slower than it needs to be, for months,
and the only way to find out today is to suspect it and run `beckon resolve` by
hand.

## 2. What this design does not do

- **No TOML grammar change.** A value is still exactly one non-empty string.
  `parse_config` is untouched, so no older beckon anywhere rejects a file
  because of this work.
- **No hot-path change.** `Backend::beckon` keeps its signature; `serve`,
  `register_all` and every resolver's fast path are untouched.
- **No candidate lists and no `match` floor.** Those are separate steps.
- **No window matching.** `--resolve` answers *"what does this NAME resolve
  to"*, not *"which window would be focused"*. Windows' window-title-substring
  layer (`beckon-windows/src/backend.rs:216`, `:236`, `:288`) is explicitly out
  of scope and unchanged; §11 explains why that boundary is honest rather than
  convenient.
- **No new top-level verb.** `--resolve` is a flag on `check`, per the growth
  rule in `CLAUDE.md` — every verb costs an app name permanently.

## 3. `Certainty`: the one word this whole line of work turns on

Each OS crate already owns a private `MatchType` — five variants on macOS, four
on Windows, four on Linux — and each already has a `pub fn describe()`. What is
missing is a **cross-OS** answer to "is this a real match or a guess".

```rust
// crates/beckon-core/src/certainty.rs
pub enum Certainty {
    /// A definite match: equality against a name, id, filename or class.
    Exact,
    /// A substring match. Correct often, wrong silently, and slow — it is the
    /// tier that forces a full catalog scan on every OS.
    Guess,
    /// Nothing in the installed-app catalog claims this id.
    NoMatch,
}
```

`NoMatch` rather than `None`: this enum is matched in the same functions that
match `Option`, and two `None` patterns one line apart is a reading trap for no
gain.

Mapping, one function per OS crate over its own existing enum:

| OS | `Exact` | `Guess` |
|---|---|---|
| macOS | `RunningName`, `RunningBundleId`, `InstalledName`, `InstalledBundleId` | `InstalledNameSubstring` |
| Windows | `InstalledName`, `InstalledAumid`, `InstalledExeStem` | `InstalledNameSubstring` |
| Linux | `NameExact`, `Filename`, `StartupWmClass` | `NameSubstring` |

Three facts make this mapping the right cut rather than an arbitrary one:

- Every backend has **exactly one** substring variant. The cut is not a judgement
  call; it falls on a line the code already draws.
- `InstalledExeStem` on Windows looks fuzzy and is not — it is equality against
  `exe_name` (`beckon-windows/src/apps.rs:373`), so it belongs in `Exact`.
- On Linux, `Certainty::None` is **not** the same as "will not work". When
  nothing resolves, `target_classes` falls back to `Target::new([raw_id])`
  (`beckon-linux/src/desktop.rs:213`), and `Target::matches` is case-insensitive
  **equality** (`algorithm.rs:111-114`) — the same strength as the `Filename`
  tier. That is what lets beckon focus an ad-hoc app shipping no `.desktop`
  file, exactly as `CLAUDE.md` promises. So `None` must be reported with its
  per-OS consequence, not as a bare ✗.

**Why `beckon-core` and not a per-OS type**: all three CI jobs compile
`beckon-core`, so the mapping is testable everywhere; and this is the vocabulary
a per-binding `match` floor consumes later (§9). A second, per-OS spelling of
the same idea is how the two drift.

## 4. Reports become values instead of side effects

Today the reporting path prints and returns nothing:

```rust
// crates/beckon-macos/src/backend.rs:319, beckon-windows/src/backend.rs:348
pub fn print_resolve_report(id: &str) -> Result<()>
```

and Linux does not have one at all — `cmd_resolve_linux` (`beckon-cli/src/lib.rs:451`)
assembles the report inside the CLI from `desktop::resolve_detailed` and
`desktop::name_substring_matches`.

Proposed, one pair per OS crate:

```rust
pub fn resolve_report(id: &str) -> Result<NameReport>;
pub fn resolve_reports(ids: &[&str]) -> Result<Vec<NameReport>>;   // ONE catalog scan
```

```rust
// beckon-core
pub struct NameReport {
    pub id: String,
    pub certainty: Certainty,
    /// What it resolved to: bundle id / AUMID / .desktop id. None when Certainty::None.
    pub target: Option<String>,
    /// The backend's own words — `MatchType::describe()`. Never parsed.
    pub tier: Option<&'static str>,
    /// What happens on a keypress given this certainty, on THIS OS. See §11.
    pub consequence: String,
    /// Closest names, for the "did you mean" line.
    pub suggestions: Vec<String>,
}
```

**`print_resolve_report` is not touched.** An earlier draft of this section had
it become a renderer over `NameReport`. Reading the two bodies killed that:
macOS prints bundle path, pid, an AX-permission warning and an ambiguity block;
Windows prints AUMID, launch mechanism, arguments, shortcut path and a window
count. Folding both into one shared type means either a `NameReport` carrying
every OS's fields or a lossy one — and either way the risk is the thing the
spec was trying to avoid, a working command changing its output.

Sharing the **computation** gets the guarantee without the risk: `resolve_reports`
calls the *same* resolver the printer calls — `apps::resolve` on macOS and
Windows, `desktop::resolve_detailed_in` on Linux — so the two paths cannot
disagree about which tier fired. They differ only in what they choose to print,
which is exactly the difference that should exist: `beckon resolve` is a
deep report on one id, `check --resolve` is one line each for twenty.

This also means no golden test is needed, and `beckon resolve` keeps working
while every task below lands.

**The plural form is the point.** `check --resolve` on a 21-line file must scan
the catalog once, not 21 times. The hoist points are verified and differ in cost:

- **Linux** — cheapest. `desktop::resolve_detailed_in(&entries, id)` and
  `desktop::scan()` are both already `pub` (`desktop.rs:151`, `:23`). Call
  `scan()` once, loop.
- **Windows** — easy. `apps::resolve(id, &installed)` takes a slice, and
  `scan_start_menu` / `scan_shell_apps` are `pub`. The discovery commands
  already build the full catalog this way.
- **macOS** — needs one visibility decision. `resolve_inner` already accepts
  `impl FnOnce() -> Vec<InstalledAppInfo>` (`apps.rs:262`) but is `pub(crate)`.
  **Put `resolve_reports` inside `beckon-macos`** rather than raising the
  visibility; the loader seam stays private and the crate keeps owning its own
  resolution rules.

**The symmetry this buys is exactly one function wide, and that is the point.**
The existing report paths stay asymmetric — macOS and Windows print from
`print_resolve_report` inside their crate, Linux prints from `cmd_resolve_linux`
inside the CLI (`beckon-cli/src/lib.rs:451`) — because tidying that is a
separate change with its own risk and no user-visible payoff. What all three
gain is `resolve_reports` with an identical signature, so `cmd_check` holds one
code path behind three `#[cfg]` arms instead of three renderers.

## 5. CLI surface

```rust
Check {
    #[arg(value_name = "CONFIG")]
    config: std::path::PathBuf,

    /// Resolve every app name against this machine's installed apps and
    /// report the tier each one matches at.
    #[arg(long)]
    resolve: bool,
},
```

Declared inside the `Check` variant, so it is rejected everywhere else
structurally — the same reason `--log` lives inside `Serve`. Both
`beckon check --resolve F` and `beckon check F --resolve` parse; the documented
form is verb-operand-flag, matching `beckon serve C --log P`.

Unlike `--log`, the flag is **not** `#[cfg]`-gated to one OS: per-machine
diagnosis is the point, and it is wanted on all three.

## 6. Output and exit code

```
$ beckon check --resolve configs/shortcuts/apps.macos.toml
ok: 20 shortcuts

  !  ctrl+super+alt+b      "Brave"    installed app name substring
                                      full catalog scan on every press; did you mean "Brave Browser"?
  !  ctrl+super+alt+space  "kitty"    running app localizedName (exact)
                                      note: "Terminal" also matches exactly here (com.apple.Terminal)
  x  ctrl+super+alt+z      "Zalo"     no match
                                      macOS: this key will error and launch nothing
     18 others: exact

2 guess, 1 no match
```

The exact column widths and wording are an implementation detail; what the
design fixes is the content — tier, consequence, suggestion — and that combos
are printed in their canonical spelling (`Combo::canonical`), never in the
display form `combo_display` produces, per the `display_never_reaches_the_serialiser`
rule.

**The exit code does not change.** A parse failure is still non-zero, exactly as
today. A `Guess` or a `None` is a **finding, not a failure**:

- `Zalo` legitimately does not resolve on macOS right now and the user knows it.
- A check that goes red on a file its author considers correct is a check people
  stop running, and `beckon check` is already wired into CI where a false red is
  expensive.

If a hard gate is wanted later it is a **second** flag (`--strict`), not a
change to this one. Deliberately not in this design.

## 7. What CI can and cannot do — a correction

`--resolve` is **inherently a local command**. `ubuntu-latest` cannot resolve a
macOS or Windows app name; the only catalog it can consult is the runner's own.
So `--resolve` does **not** go into CI, and any plan that assumes it does is
wrong.

What CI keeps doing is the shape check it does today — which is worth exactly as
much as the beckon it runs (§8).

## 8. `~/.nix` rollout items

These live in the other repo and are independent of the beckon change; they are
recorded here because they are the reason the tooling-first order was chosen.

**8.1 The pin is a landmine, and it is live today.**
`~/.nix/flake.lock` pins beckon at `ad1d0ce` — **v0.6.0, 330 commits behind
HEAD** — and that build contains **zero** occurrences of `KEYBOARD_KEY`: it does
not know the `keyboard` block exists and rejects any file carrying one. CI
(`eval.yml:118-124`) validates every `apps.*.toml` with **that** binary. It is
green only because no file currently has a `keyboard` line — and the Windows
settings window can write one at any time. `~/.nix/CLAUDE.md:660` records that
beckon 0.8.0 already did exactly that. **Bump the pin.** That is the whole fix,
and it is worth doing whatever happens to the schema.

**8.2 Generate the README table instead of maintaining it.**
`configs/shortcuts/README.md` has already drifted: its `Cap+Shift` table lists
`d = DeepSeek` for Windows, while `apps.windows.toml` has no
`ctrl+super+alt+shift+d` row at all — lines 45-47 of that file explain the
deliberate omission (`d` = OneDrive, inside the Office-key set). The README's own
header says *"Sua apps.*.toml thi sua ca bang nay — khong con sync.sh kiem tra"*,
and it did not survive that instruction.

Generate it with `builtins.fromTOML` — the same parser the three `launch-app.nix`
generators already use. No new dependency, no second parser to keep in sync, and
it runs anywhere, so it *can* be a CI gate where `--resolve` cannot. This is the
small, honest version of the deleted `sync.sh`.

**8.3 Tripwire, recorded not fixed.** The CI glob `configs/shortcuts/apps.*.toml`
does **not** match a bare `apps.toml`. It is correct for today's three-file
layout; if the layout ever changes, the job silently iterates zero files and
exits 0.

## 9. Room deliberately left for the next steps

This design is step 1 of four. It is shaped so the following steps extend it
rather than rework it:

- **Step 2 — writer safety.** `config_write::render` deletes every top-level key
  that is neither `keyboard` nor a live row (`config_write.rs:46-54`) and
  overwrites every row's value with a plain string via `as_value_mut()`
  (`:61-67`), edited or not. Measured: an array is flattened, an inline table is
  flattened, a `[defaults]` block vanishes — all on a file where nothing was
  edited. This is **latent today only because `parse_config` refuses those
  shapes first**, and it must be fixed before any grammar widening. Nothing in
  this design depends on it, and nothing in this design fixes it.
- **Step 3 — the `match` floor.** It consumes `Certainty` unchanged: `match =
  "exact"` means "refuse `Certainty::Guess`". No second vocabulary.
- **Step 4 — candidate lists.** `resolve_reports(&[&str])` is already the plural,
  one-scan shape a candidate loop needs.
- **Design rule that constrains both**: every knob in the TOML must have a CLI
  spelling, because on Linux the compositor binds the key and Nix translates the
  config into `beckon "<id>"` invocations at **eval** time. A knob with no
  command-line form is macOS/Windows-only by construction.

## 10. Testing

- **Unit, per OS job**: the `Certainty` mapping is exhaustive over that OS's
  `MatchType` — a `match` with no wildcard arm, so adding a variant fails to
  compile rather than silently landing in `Exact`.
- **Unit, all three jobs**: `beckon-core` is excluded from no CI runner, so
  `Certainty` and the summary line are tested on Linux, macOS and Windows.
- **One scan for N names**: assert the loader closure runs at most once for a
  multi-id `resolve_reports` (macOS and Windows already take a loader; Linux
  takes a slice, so the assertion is on `scan()` call count).
- **Live (Linux)**: `testing/linux_live_test.py` already drives resolution
  against a real session; extend it with a `check --resolve` case over a
  temporary TOML.

## 11. Known traps

- **Do not call `AXIsProcessTrusted()` on the macOS path.** It is ~20 ms
  (`CLAUDE.md`) and has nothing to do with name resolution. The existing
  step-4.5 ordering comment exists for this reason.
- **Use the full Windows catalog, not `resolve_lazy`.** `resolve_lazy` is a
  hot-path optimisation that can return `InstalledName` without ever consulting
  AppsFolder. Reporting through it would make `check --resolve` disagree with
  what `beckon <id>` actually does on the miss path. Discovery commands already
  follow this rule (`CLAUDE.md`: *"correctness and completeness beat latency
  there"*).
- **`Certainty::None` needs a per-OS sentence, not a shared ✗.** macOS errors;
  Windows falls through to exe-name and window-title matching; Linux treats the
  raw id as a window class and can still focus a live window. One shared line
  would be wrong on two OSes out of three — which is why `NameReport` carries
  `consequence` as text rather than deriving it from the enum.
- **The report is about names, not windows.** On Windows a name that reports
  `Certainty::None` may still focus something through the title-substring layer.
  The report must not claim otherwise; suppressing that layer is step 3's job
  and needs a boolean threaded into two private functions
  (`windows_for_resolved`, `windows_by_literal_id`), which is why it is not
  smuggled in here.
