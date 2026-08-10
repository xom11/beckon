# beckon 0.6.0 — CLI: flags become subcommands

Status: design approved, not implemented.
Measured on macOS 26 / rustc 1.97.1 / clap 4.6.1, against the working tree at `866a0b3` (v0.5.4).

---

## Why

Mutual exclusion between beckon's seven commands is declared by hand, one clap
attribute at a time. There are **33** `conflicts_with_all` entries across
`main.rs:35,39,43,47,51,55,61`. `--check` and `--serve` name `"id"` among their
conflicts. `-l`, `-L`, `-s`, `-r`, `-d` do not.

`run()` (`main.rs:118-161`) is an if-ladder that tests the flags **before**
`args.id`. So the missing entries are not a lint warning, they are a behaviour:

| argv | exit | what happened |
|---|---|---|
| `beckon ThisAppDoesNotExist -l` | **0** | printed the running-app table, id discarded |
| `beckon ThisAppDoesNotExist -d` | **0** | ran doctor, id discarded |
| `beckon ThisAppDoesNotExist -r Finder` | **0** | resolved Finder, id discarded |
| `beckon ThisAppDoesNotExist -s Finder` | **0** | searched, id discarded |
| `beckon ThisAppDoesNotExist --check /nope.toml` | 2 | correct clap error |
| `beckon -l -d` | 2 | correct clap error |

Exit 0 is why this matters. No script, no CI, no supervisor can catch it. beckon
reports success and does the wrong thing.

The five missing entries could be added in five lines. That is not what this spec
does, because the hand-maintained matrix is the cause and it grows as O(n²) — every
new command means editing every old one, and a forgotten entry produces silent
wrong behaviour rather than a compile error. clap permits exactly one subcommand,
so under subcommands the 28 exclusions stop being something anyone declares.

Two further defects fall out for free: `-l`/`-L` differ only by case, and `--log`
needs `requires = "serve"` because a flag cannot nest under another flag.

## Scope

No backward-compatibility aliases of any kind. The repo owner is the sole user and
controls every deployment, so `--serve`, `--check`, `--log` and all short command
flags are cut outright. `-v/--verbose`, `-h/--help`, `-V/--version` stay.

The bare positional hot path `beckon <id>` does not change. It is 99% of
invocations, it is what every sway/AHK/Hammerspoon binding calls, and every
decision below is subordinate to keeping it byte-identical.

---

## The two things clap will not do for you

**This is the load-bearing section. Read it before writing any code.**

### 1. A positional and a subcommand can both be supplied, and clap accepts it

Measured on an unguarded probe of exactly the surface below:

```
$ probe5 Claude list     →  id: Some("Claude"), command: Some(List)     exit 0
$ probe5 Claude doctor   →  id: Some("Claude"), command: Some(Doctor)   exit 0
```

That is the original defect with new spelling. Migrating to subcommands does **not**
fix it on its own.

### 2. `args_conflicts_with_subcommands = true` is worse, not better

It is the one-line fix everyone reaches for. It is wrong:

```
$ probe4 -v list          →  id: Some("list"), command: None, verbose: true   exit 0
$ probe4 --verbose list   →  id: Some("list"), command: None                  exit 0
$ probe4 -v doctor        →  id: Some("doctor"), command: None                exit 0
```

Root cause is in clap itself, `clap_builder-4.6.0/src/parser/parser.rs:592`:

```rust
if !(self.cmd.is_args_conflicts_with_subcommands_set() && valid_arg_found) {
```

Once any argument has been parsed, subcommand lookup switches off for the rest of
the line. So `-v` before a verb silently converts the verb into an app id.

This is not a hypothetical. `testing/linux_live_test.py:509` is
`run([self.beckon, "-v", *args])` — the helper behind roughly eight focus-algorithm
tests. Setting this flag breaks all of them at once, and the failures look like a
backend regression rather than a parser one.

**Do not set `args_conflicts_with_subcommands`. Do not "simplify" the guard below
into it.** If a future session proposes it, the answer is this section plus test 6.

### 3. Therefore: a hand-written post-parse guard

```rust
impl Args {
    /// `Args::parse()` plus the two invariants clap cannot express here.
    /// Both paths exit(2), matching clap's own usage-error code.
    fn parse_checked() -> Self {
        let args = Self::parse();
        match (&args.id, &args.command) {
            (None, None) => Self::command()
                .error(
                    clap::error::ErrorKind::MissingRequiredArgument,
                    "an app id or a subcommand is required",
                )
                .exit(),
            (Some(_), Some(_)) => Self::command()
                .error(
                    clap::error::ErrorKind::ArgumentConflict,
                    "an app id cannot be combined with a subcommand; \
                     use `beckon -- <ID>` if the app is literally named like one",
                )
                .exit(),
            _ => args,
        }
    }
}
```

Verified against the guarded probe:

```
$ beckon Claude list   →  exit 2, "an app id cannot be combined with a subcommand…"
$ beckon -- list       →  id: Some("list")   exit 0
$ beckon               →  usage on stderr    exit 2
```

The `(None, None)` arm exists because `arg_required_else_help` fires only on a
genuinely empty argv. `beckon -v` alone parses clean to `(None, None)` and would
otherwise be a silent exit-0 no-op.

---

## Verified clap mechanics

Every row was produced by running a probe binary, not by reading documentation.

| # | Question | Answer |
|---|---|---|
| 1 | `Option<String>` positional + `Option<Commands>` subcommand at top level? | Yes, **zero extra attributes**. Passes `debug_asserts`. |
| 2 | `beckon Claude` | → positional, exit 0 |
| 3 | `beckon list` | → subcommand, exit 0. Prefix inference is off, so `beckon li` → `id: Some("li")` |
| 4 | Does `--` force a verb-shaped word into the positional? | **Yes.** `-- list`, `-- doctor`, `-- help`, `-- -weird.id` all bind to `id`. Composes with `-v`. Escapes exactly one slot — quote multi-word names: `-- "Windows Terminal"` |
| 5 | `run <id>` as an escape? | Works for verbs, but **not** for a leading dash: `run -weird.id` → exit 2. Needs `run -- -weird.id` |
| 6 | `global = true` on `-v`? | All four of `-v <id>`, `<id> -v`, `-v list`, `list -v` work — **only while `args_conflicts_with_subcommands` is unset**. Without `global`, `-v` after a verb is an unknown argument |
| 7 | `beckon list doctor` | exit 2, `unexpected argument 'doctor'`, `Usage: beckon list [OPTIONS]` |
| 8 | `beckon Claude list` | **both accepted silently, exit 0** — see above |
| 9 | `arg_required_else_help` with subcommands? | Works on empty argv: help to **stderr**, exit 2. Does not cover `beckon -v` |
| 10 | `-- -weird.id` still works? | Yes, unchanged |
| 11 | `--log` scoped to `Serve`? | Rejected everywhere else structurally, no `requires =` needed. `beckon --log X`, `beckon list --log X`, `beckon check c --log X` all exit 2 |
| 12 | Is `help` reserved? | **Yes** — clap injects a builtin `help` subcommand. `beckon -- help` still reaches the id |
| 13 | Does an empty string still reach the id? | Yes: `beckon ""` → `id: Some("")`. **`require_id` must stay** on both entry points |

Row 13 is a trap worth naming. Changing `Option<String>` to `String` for
`search`/`resolve` makes clap enforce *presence*, not *non-emptiness*. Deleting
`require_id` would re-introduce the empty-id bug that CLAUDE.md's "Resolution
priority" section records as fixed — an empty id is a substring of every `Name`, so
`beckon "$APP"` with `$APP` unset launches whatever sorts first.

---

## The surface

```
beckon <id>                          # hot path — UNCHANGED
beckon -- <id>                       # escape: reserved name, or id starting with '-'

beckon list                          # was -l
beckon installed                     # was -L
beckon search <NAME>                 # was -s
beckon resolve <ID>                  # was -r
beckon doctor                        # was -d
beckon check <CONFIG>                # was --check
beckon serve <CONFIG> [--log PATH]   # was --serve / --log   (--log Windows-only, cfg-gated)

beckon -v, --verbose                 # global
beckon -h/--help, -V/--version       # clap builtins
```

**Reserved names — a closed list of 8:**
`list` `installed` `search` `resolve` `doctor` `check` `serve` `help`

An app whose Name equals one of these is reachable only as `beckon -- <name>`.
Matching is byte-exact, so `beckon Resolve` still reaches the id — see open
decision D1, which is about what happens when it doesn't.

**Growth rule, to be recorded in CLAUDE.md:** new capabilities are flags on an
existing verb, never a new top-level verb. Every verb added costs an app name
permanently. No aliases, ever — an alias costs a name and saves nothing.

### `run <id>` was considered and dropped

The earlier draft of this design included `beckon run <id>` as the explicit escape
hatch. Measurement killed it. `--` escapes both reserved names *and* leading
dashes; `run` escapes only reserved names and still needs `run -- -weird.id` for a
dash. So `--` strictly dominates it, and keeping both would ship two mechanisms for
one job while spending a ninth reserved name. Do not re-add it without re-testing
row 5.

### `--log` moves under `serve`, and `requires` disappears

Today `--log` is a top-level flag with `requires = "serve"` (`main.rs:71`). As a
subcommand option it is rejected elsewhere structurally. Keep the existing
four-paragraph rationale comment from `main.rs:64-71` verbatim; only its first line
changes. Note the argument order flips: `beckon serve <cfg> --log <path>`, never
`beckon --log <path> serve <cfg>`.

### `run()` becomes a match

```rust
fn run(args: &Args) -> Result<()> {
    match &args.command {
        Some(Command::Serve { config, .. }) => { /* existing cfg blocks, verbatim */ }
        Some(Command::Check { config })     => cmd_check(config),
        Some(Command::Doctor)               => cmd_doctor(),
        Some(Command::List)                 => cmd_list(),
        Some(Command::Installed)            => cmd_list_installed(),
        Some(Command::Search { name })      => cmd_search(require_id(name, "search NAME")?),
        Some(Command::Resolve { id })       => cmd_resolve(require_id(id, "resolve ID")?),
        None => match args.id.as_deref() {
            Some(id) => cmd_beckon(require_id(id, "id")?, args.verbose),
            None => Err(anyhow!("no command given (use -h for help)")), // parse_checked rejects this
        },
    }
}
```

A match, not a second if-ladder. That is what makes the dispatch-order bug
unrepresentable rather than merely absent.

### One line that will silently invert if missed

`main.rs:91` selects the notification throttle:

```rust
let cause = if args.serve.is_some() { MachineRepeat } else { HumanAction };
```

becomes

```rust
let cause = if matches!(args.command, Some(Command::Serve { .. })) { MachineRepeat } else { HumanAction };
```

Get this wrong in the permissive direction and the 5-minute Windows watchdog posts
a desktop notification every five minutes forever. `notify_policy.rs`'s
`repeated_serve_startup_failures_notify_once` is the test that catches it.

Also verify `is_expected()` (`main.rs:111-116`) still suppresses the notification
for `AcquireError::AlreadyRunning` — that is the watchdog finding a healthy serve,
and it must stay silent.

---

## Decisions

All four were put to the repo owner on 2026-08-10 and settled in favour of the
recommendation. Recorded here with the reasoning that produced them, so a later
session can see what was weighed rather than only what was chosen.

### D1 — Intercept the case-shadowed verb, or ship clap's default error? → **B, intercept**

`beckon resolve` → exit 2, `error: the following required arguments were not
provided: <ID>`. The message never hints that an app name got shadowed. beckon's
own resolvers are all case-insensitive (`desktop.rs` tiers 1 and 4,
`beckon-macos/src/apps.rs`, the Windows name tier), so capitalisation alone decides
between "app" and "command", and CLAUDE.md's own examples bind lowercase ids
(`beckon kitty`).

- **A — ship clap's default.** Zero code.
- **B — intercept.** When `try_parse()` returns `MissingRequiredArgument` or
  `InvalidSubcommand` and argv is exactly two words, print
  `` `resolve` is a subcommand name, not an app id. If you meant the app, run: beckon -- resolve ``
  and exit 2. ~15 lines.

**Recommendation: B.** DaVinci Resolve ships `Name=DaVinci Resolve` and would match
the tier-4 substring rule for the needle `resolve` (**UNVERIFIED** — not installed
on this machine). Do *not* reach for `ignore_case = true` on subcommands; that
makes `beckon Resolve` a subcommand too and closes the last escape.

### D2 — Two verbs (`list` + `installed`) or one (`list [--installed]`)? → **A, two verbs**

- **A — two verbs.** Mechanical 1:1 with `-l`/`-L`. No runtime output changes.
  Costs 2 reserved names. `installed` is the only adjective among verbs.
- **B — one verb.** 1 reserved name, matches `apt list --installed`. But
  `cmd_list` (`main.rs:214`) prints `ID WINS NAME` and `cmd_list_installed`
  (`main.rs:237`) prints `ID NAME` — merging changes runtime output, which is a
  behaviour change smuggled into a parsing release.

**Recommendation: A for 0.6.0.** Keep this release purely about parsing, so that
when something breaks on a14 or in the Linux VM there is exactly one class of
cause. Revisit in 0.7.0 if `installed` grates. Note the cost of deferring: with no
aliases, changing it later is another breaking migration.

### D3 — `check` or `validate`? → **`check`**

`check` is already spelled in `packaging/homebrew/beckon.rb.template:69`, four of
the owner's own TOML headers, and `~/.nix/.github/workflows/eval.yml:120` — all of
which are being edited anyway, so the marginal cost of renaming is small.
`validate` says its own postcondition in a CI log line; `check` collides
conceptually with `cargo check`.

**Recommendation: keep `check`.** Weak preference, flip freely.

### D4 — `beckon -v` with no id and no subcommand: exit 1 or 2? → **exit 2**

Measured on current main: **exit 1**, `beckon: no command given (use -h for help)`,
and it fires the notification path. The `parse_checked` guard makes it **exit 2**
with no notification, because clap exits before `main`'s error handler runs.

**Recommendation: exit 2.** "No command given" is a usage error. The notify path
exists for `serve` and for hotkey-invoked failures, not for this. Call it out in
the 0.6.0 release notes.

---

## Change inventory

205 sites. Ordered by blast radius, executed-first.

### Group A — executed, **outside this repo** (6 sites) ⚠

These break the owner's own machines and no amount of editing this repo touches
them. All four code sites confirmed present on 2026-08-10; re-confirm line numbers
at edit time, `~/.nix` moves independently.

| # | File | Change |
|---|---|---|
| A1 | `~/.nix/home-manager/programs/beckon-serve/default.nix:44` | `[bin "--serve" configFile]` → `[bin "serve" configFile]`. macOS launchd agent — breaks the hotkey host on `airm3` at next logon |
| A2 | `~/.nix/windows/modules/services/beckon-serve/module.ps1` | `--serve` → `serve`; `--log` must follow it. Keep the `conhost --headless` prefix |
| A3 | `~/.nix/windows/modules/services/beckon-serve-watchdog/module.ps1` | same |
| A4 | `~/.nix/.github/workflows/eval.yml:120` | `-- --check "$f"` → `-- check "$f"`. Pinned by `flake.lock`, so it breaks the instant the rev is bumped unless changed in the same commit — `~/.nix/CLAUDE.md:178` already codifies that rule for TOML changes |
| A5 | `~/.nix/home-manager/programs/beckon-serve/README.md:15` | `beckon -d` → `beckon doctor` |
| A6 | `~/.nix/CLAUDE.md` (~6 lines) | prose and table updates |

Verified **not** affected: `~/.nix/home-manager/environments/sway/launch-app.nix:25`
generates `exec beckon "<Name>"` — bare positional. Same for the GNOME variant and
the Hammerspoon spoon. The `configs/shortcuts/apps.*.toml` *values* are Names; only
their comment headers mention flags.

### Group B — executed, in this repo (7 sites)

`packaging/homebrew/beckon.rb.template:51` (the `service do` LaunchAgent), `:69`
(caveat text), `:33` (comment) · `examples/macos/serve/com.github.xom11.beckon.plist:14`
· `examples/windows/serve/beckon-serve.xml:101` (subcommand must come first) ·
`examples/linux/gnome-x11/setup.sh:54`.

**No change:** `packaging/homebrew/beckon.rb.template:81` is `beckon --version`, a
clap builtin. A mechanical pass must not rewrite it to `beckon version`, which will
not exist. `.github/workflows/ci.yml:20` is `cargo fmt --all -- --check`, not ours.

### Group C — CLI definition, `crates/beckon-cli/src/main.rs` (~14 edit points)

`:3` imports · `:13-21` long_about · `:23-28` `#[command]` — add `override_usage`
(without it the usage line reads `[OPTIONS] [ID] [COMMAND]`, which falsely implies
they combine), keep `arg_required_else_help`, **do not** add
`args_conflicts_with_subcommands` · `:30-32` id doc, must list the 8 reserved names
because `--help` prints it · `:34-62` delete 7 flag fields and all 33 conflict
entries · `:64-72` move `log` into `Serve` · `:75-77` add `global = true` · `:81`
`parse()` → `parse_checked()` · `:91` the `Cause` selection · `:118-161` ladder →
match · `:137`, `:153`, `:156` message text.

### Group D — runtime strings users read (16 sites) ⚠

Printed on **every** failed resolve, on every OS. The highest-frequency user-facing
text in the product; a stale `-L` here is what the owner sees most often after
0.6.0.

`main.rs:335,375` · `beckon-macos/src/backend.rs:120,203,337,376,385` ·
`beckon-windows/src/backend.rs:58,375,426` · `beckon-linux/src/i3ipc.rs:182` ·
`x11.rs:434` · `kde.rs:388` · `gnome.rs:190` · `hyprland.rs:203` ·
`beckon-cli/src/lockfile.rs:49`.

The launch-failure hint is **duplicated verbatim across seven backends**. That is a
pre-existing maintenance smell, not created by this change and not fixed by it —
but all seven must be updated identically, and an inconsistency between them is a
real defect. Worth a follow-up to hoist into `beckon-core`; out of scope here.

### Group E — doc comments (18 sites)

`beckon-macos/src/{backend.rs:317, lib.rs:16,52, ffi.rs:67}` ·
`beckon-windows/src/{backend.rs:346, lib.rs:44, logfile.rs:1,108,123,124}` ·
`beckon-core/src/shortcuts.rs:1,8` · `beckon-cli/src/{notify.rs:201,210,
lockfile.rs:3, serve.rs:1}` · `beckon-linux/src/{i3ipc.rs:248, desktop.rs:131}`.

### Group F — tests

See the TDD sequence. `check.rs` · `serve.rs` · `serve_log.rs` ·
`notify_policy.rs:43,60,85,107,130,131` · `hyprland_e2e.rs:544,582` only ·
`testing/linux_live_test.py` (19 lines listed in the sequence).

### Group G — markdown (~80 lines across 12 files)

`CLAUDE.md` (34 hits, including the `### CLI surface` block at `:83-101` — this is
the canonical listing every future session reads before touching the code) ·
`README.md` (18, including `:116` `nix run github:xom11/beckon -- -l` → `-- list`) ·
`examples/README.md` and the nine per-environment READMEs · the comment headers in
`examples/linux/{sway,i3,hyprland,openbox}/*`, `examples/windows/ahk/beckon.ahk`,
`examples/{macos,windows}/serve/apps.toml` · `test-i3-env.sh:18`.

### Deliberately not changed

| What | Why |
|---|---|
| `docs/superpowers/{plans,specs}/*.md` — 6 files, 72 hits | Historical record. Rewriting `--serve` there would make them describe a design that never shipped under that name and destroy the ability to date a decision. Leave byte-identical; add one dated line at the top of `2026-08-09-serve-background.md` noting the rename |
| Every hot-path binding in `examples/` and `~/.nix` | `exec beckon "<Name>"`, `hs.task.new(…, {name})`, `Run("beckon <Name>")` — bare positional, preserved by design |
| `testing/linux_live_test.py:509` (`-v` helper) | Survives **only** because `global = true` is set and `args_conflicts_with_subcommands` is not. This is the tripwire for the section-2 finding |
| `testing/linux_live_test.py:937-941` and `:18` | The test script's own argparse flags. A mechanical find-and-replace will corrupt them |

---

## TDD sequence

Implementation order. "Red on main" means it fails against `866a0b3` for a
behavioural reason.

**Phase 1 — pin the defect and the hot path, before touching `main.rs`**

1. **`flag_style_invocation_is_rejected`** (new, `tests/cli_surface.rs`) — for each
   of `["X","-l"]`, `["X","-d"]`, `["X","-r","Y"]`, `["X","-s","Y"]`: exit **2**,
   stdout empty. **Red on main: all four exit 0 and print a table.** This is *the*
   regression test. Write it first, watch it fail, never delete it.
2. **`bare_positional_hot_path_survives`** — `beckon definitely-not-installed-zzz`
   exits **1** (not 2), stderr starts `beckon:` and does not contain
   `unexpected argument`. Green on main; a pin, not a red test. The single
   assertion proving the 99% path did not move.
3. **`empty_id_is_still_rejected`** — `beckon ""` exits non-zero with
   `empty id: expected an app Name or id`. Green on main; pins that `require_id`
   survives the match rewrite.

**Phase 2 — the parse invariants (compile-red until `Command` exists)**

4. **`id_and_subcommand_are_mutually_exclusive`** — over `["Claude","list"]`,
   `["Claude","doctor"]`, `["Claude","resolve","X"]`, `["Claude","doctor","-v"]`.
   **This is the only test that catches `parse_checked` being deleted** — measured,
   clap accepts all four silently without it. Note `try_parse_from` does *not* run
   `parse_checked`: either factor the check into a pure
   `fn validate(&self) -> Result<(), ErrorKind>` and unit-test that, or shell out
   and assert exit 2 plus `cannot be combined with a subcommand`.
5. **`no_id_and_no_subcommand_is_rejected`** — `beckon -v` exits **2**. Red on
   main: exits 1 (decision D4). Also assert bare `beckon` exits 2 with `Usage:` on
   **stderr**, stdout empty.
6. **`global_verbose_parses_in_every_position`** — all four of `-v <id>`,
   `<id> -v`, `-v list`, `list -v`. **This is the guard against
   `args_conflicts_with_subcommands` being added later**, and the reason
   `linux_live_test.py:509` keeps working.
7. **`dash_dash_escapes_reserved_names_and_leading_dashes`** — `beckon -- list`,
   `-- doctor`, `-- help`, `-- -weird.id`, `-v -- list` all reach the id path.

**Phase 3 — migrate the existing integration tests**

8. **`check.rs`** — helper `.arg("--check")` → `.arg("check")`. Then **strengthen
   `check_missing_file_exits_nonzero`**: it asserts only `!success`, which a clap
   exit-2 satisfies vacuously — pin `status.code() == Some(1)` **and** stderr
   contains `cannot read`. Without this it stays green while testing nothing.
9. **`serve.rs`** — same vacuous-pass hazard in
   `serve_missing_file_exits_nonzero_and_does_not_hang`. Rewrite
   `serve_conflicts_with_check` as `beckon serve /tmp/a.toml check /tmp/a.toml`;
   its old `cannot be used with` assertion is dead — the new error is
   `unexpected argument 'check' found` with `Usage: beckon serve [OPTIONS] <CONFIG>`.
   Assert both; the usage line proves the rejection came from `serve` and not from
   the top level.
10. **`serve_log.rs`** (Windows CI) — `--serve` → `serve`, `--log` now after it.
    Rewrite `log_without_serve_is_rejected` → `log_is_unknown_outside_serve`: the
    `stderr.contains("--serve")` assertion is dead because `requires` is gone. New
    cases: `beckon --log X`, `beckon list --log X`, `beckon check <cfg> --log X`
    each exit 2 naming `'--log'`.
11. **`notify_policy.rs`** — `:43,60,107` `--check` → `check`; `:85,130,131`
    `--serve` → `serve`. `repeated_serve_startup_failures_notify_once` is what
    proves the `Cause` rewrite survived. `muting_wins_over_everything` at `:60`
    asserts `is_empty()`, which a clap exit-2 also satisfies — migrate it even
    though CI would not flag it.
12. **`hyprland_e2e.rs`** — `:544` (`-l`) and `:582` (`-d`) only. The other ten
    tests use `run_beckon(&["claude"])` and must stay byte-identical; they are the
    hot-path net.

**Phase 4 — live suite**

13. **`testing/linux_live_test.py`** — mechanical at
    641/643/645/648/650/652/655/657/660/662/665/668/864/868/872/874/877/883/886.
    Leave `:509` and `:937-941` alone. Add `beckon -- list` reaching the id path,
    and re-verify `beckon -- -weird.id` (`:679`) rather than assuming it.

---

## Risks

Ranked by how long until the owner notices.

| # | Risk | Latency | Mitigation |
|---|---|---|---|
| R1 | **Group A still passes `--serve`.** After `nix flake update beckon`, hotkeys die on `airm3` at next logon; on a14 the failure is *silent* because stderr goes to `--log`, and the watchdog restarts it every 5 minutes forever | Immediate on macOS; **days** on Windows — the tray icon is the only liveness signal | Edit all of Group A in the same commit as the `flake.lock` bump. Verify with `launchctl list \| grep beckon-serve` and `Get-ScheduledTask \BeckonServe \| Get-ScheduledTaskInfo` |
| R2 | **`args_conflicts_with_subcommands` gets added** — by this implementation or a future session "fixing" `(Some,Some)` the obvious way. 8+ live tests fail at once and look like a backend regression | Immediate but **misattributed** — hours debugging the wrong layer | Test 6, plus the `parser.rs:592` citation as a code comment on the `#[command]` attribute so the next reader sees the mechanism, not just the prohibition |
| R3 | **`(Some,Some)` left unguarded** — ships and reproduces the original defect with new spelling, exit 0 | **Never noticed on its own** — which is the entire reason this migration exists | Tests 1 and 4 |
| R4 | **Vacuous-pass tests.** `check_missing_file_exits_nonzero`, `serve_missing_file_exits_nonzero_and_does_not_hang`, `muting_wins_over_everything` all assert something a clap exit-2 satisfies the moment their flag stops existing | **Never** — they go green forever | Steps 8, 9, 11 |
| R5 | **`~/.nix` CI job breaks** — `eval.yml:120` runs against the rev pinned in `flake.lock` | Next `~/.nix` PR. Loud, but blocks unrelated work | Same-commit rule |
| R6 | **Stale `-L`/`-l`/`-d` in the 16 Group D strings** | Days to weeks, on the first typo'd app name | Release gate: `grep -rn 'beckon -[lLdsr][^a-z]' crates/` returns zero |
| R7 | **Case shadowing** — `beckon resolve` exits 2 with a message that never mentions the app | Whenever an app Name matches a verb | Decision D1 option B |
| R8 | **`help` became reserved and nobody wrote it down** | Only with an app named `Help` (UNVERIFIED) | The 8 names in CLAUDE.md *and* in the id positional's doc comment, which `--help` prints |
| R9 | **`--log` position regression** — it is now scoped to `serve`, and `module.ps1` builds the argument string by concatenation. The console-flash work means the failure is a *missing* window, not a visible error | Immediate on a14, invisible | Test 10; after deploying, confirm a fresh registration line in the log |
| R10 | **`beckon -v` exit 1 → 2, notification dropped** (D4) | Probably never | Test 5 makes it deliberate; note it in the release notes |

---

## Deployment checklist

**Stage 0 — before writing code**

1. On a14, confirm no Start Menu shortcut collides with the 8 reserved names:
   ```powershell
   Get-ChildItem -Recurse -Filter *.lnk `
     "$env:ProgramData\Microsoft\Windows\Start Menu\Programs", `
     "$env:APPDATA\Microsoft\Windows\Start Menu\Programs" |
     Where-Object BaseName -match '^(list|installed|search|resolve|doctor|check|serve|help)$' |
     Select-Object BaseName, FullName
   ```
2. Same on the Linux VM and on macOS: `beckon -L | grep -iE '^(list|installed|search|resolve|doctor|check|serve|help)\b'`.
   An exact hit means that verb needs `beckon -- <name>` documented next to the
   binding, or the verb needs renaming before it ships.

**Stage 1 — beckon repo**

3. Tests first, in the order above. `cargo test --workspace` green on macOS;
   `cargo check -p beckon-cli --target aarch64-pc-windows-msvc` clean.
4. Groups C → D → E → F → G. Gate:
   `grep -rn -e 'beckon -[lLdsr][^a-z]' -e '\-\-serve' -e '\-\-check' crates/ examples/ packaging/ testing/ README.md CLAUDE.md`
   returns only `packaging/homebrew/beckon.rb.template:81` (`--version`).
   `docs/superpowers/` excluded by design.
5. Live suite in the Lima VM (`LIMA_HOME=/Volumes/ssd/lima`), all four
   compositors: **19/19 must pass**. If the eight tests behind `:509` fail, R2
   happened.
6. Bump to `0.6.0`, tag, push. Check `PACKAGER_TOKEN` has not expired (90-day
   default) *before* tagging.

**Stage 2 — macOS `airm3`, the loudest failure**

7. Edit A1. 8. `nix flake update beckon` in the **same commit**.
9. `sudo darwin-rebuild switch --flake .#airm3 --impure`.
10. Verify in order: `launchctl list | grep beckon-serve` shows a pid; the serve
    log shows a registration line dated *after* the switch; press one hotkey. If
    throttled, `ThrottleInterval = 60` means wait a minute before concluding.
11. By hand: `beckon list`, `beckon doctor`, `beckon -- list`.

**Stage 3 — `~/.nix` CI, before it blocks anything**

12. A4, ideally in the same commit as step 8. 13. A5 and the four TOML comment headers.

**Stage 4 — Windows `a14`, the silent failure**

14. `scoop update beckon`; confirm `beckon --version` reports 0.6.0.
15. Edit A2 and A3. Keep `conhost --headless` in front and keep pointing at
    `scoop\apps\beckon\current\beckon.exe`, **not** the shim — the shim holds the
    console and defeats `FreeConsole`.
16. `~/.nix/windows/apply.ps1`.
17. `Get-ScheduledTask \BeckonServe | Get-ScheduledTaskInfo` shows
    `LastTaskResult 0x0`; the log shows a registration line after the apply; press
    one hotkey. `--log` **appends**, so read timestamps, not just the tail. SSH
    lands in session 0 — use the scheduled-task probe pattern for anything needing
    window visibility.

**Stage 5 — Linux hosts**

18. `home-manager switch --flake .#<host>` on `rog` / `desktop` / `zenbook-a14`.
    Nothing to edit; bindings are bare positional.
19. `beckon doctor` on each.

**Stage 6 — repo hygiene**

20. Record in CLAUDE.md: the closed 8-verb list; the growth rule; and the measured
    reason `args_conflicts_with_subcommands` is forbidden, with the
    `parser.rs:592` citation.
21. One dated line at the top of `docs/superpowers/plans/2026-08-09-serve-background.md`
    noting the 0.6.0 rename; the document stays byte-identical below it.

---

## For future sessions

- **Do not add `args_conflicts_with_subcommands`.** Measured: it makes
  `beckon -v list` bind `"list"` to the id positional. See section 2 and test 6.
- **Do not delete `parse_checked`.** clap accepts a positional *and* a subcommand
  together, silently, exit 0. Test 4 is the only thing that catches its removal.
- **Do not delete `require_id`.** clap enforces presence, not non-emptiness.
- **Do not add a new top-level verb.** Every verb permanently costs an app name.
  New capabilities are flags on an existing verb.
- **Do not add aliases.** An alias costs a name and saves nothing.
- **`run <id>` was considered and dropped** because `--` strictly dominates it.
  Re-read row 5 before re-adding it.
