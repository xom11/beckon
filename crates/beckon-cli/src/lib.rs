//! `beckon`'s command surface, as a library so that both binaries in this
//! package can share it: `beckon` (console subsystem, the CLI) and
//! `beckon-serve` (GUI subsystem, the Windows tray app). Splitting it out
//! is what lets `serve.rs` have exactly one implementation.

use anyhow::{anyhow, Context, Result};
use beckon_core::certainty::{Certainty, NameReport};
use beckon_core::shortcuts::Shortcut;
use beckon_core::Backend;
use clap::{CommandFactory, Parser, Subcommand};

mod lockfile;
mod notify;
#[cfg(any(target_os = "macos", target_os = "windows"))]
mod serve;
mod serve_app;
mod stable_id;
#[cfg(any(target_os = "macos", target_os = "windows"))]
mod update;

#[cfg(target_os = "windows")]
pub use serve_app::serve_app_main;

/// Every subcommand name, and therefore every app Name that the bare
/// positional cannot reach. A closed list on purpose: each entry costs an app
/// name permanently, so new capabilities are flags on an existing verb, never
/// a new top-level verb. `help` is in here because clap injects it.
const RESERVED: &[&str] = &[
    "list",
    "installed",
    "search",
    "resolve",
    "doctor",
    "check",
    "serve",
    "help",
];

/// Cross-platform focus-or-launch app switcher.
///
/// Hot path: `beckon <id>` — focus an app if running, launch it if not, cycle
/// windows / toggle to previous app if already focused.
///
/// `<id>` is the raw OS identifier:
///   - sway / Wayland: `app_id` (e.g. `kitty`, `firefox`, `claude.ai__new`)
///   - macOS:          `bundle_id` (e.g. `com.anthropic.claudefordesktop`)
///   - Windows:        display name / exe / AUMID (e.g. `Terminal`)
///
/// Use `beckon list` to discover ids on the current machine.
#[derive(Parser, Debug)]
#[command(
    name = "beckon",
    // `env!`, not the bare `version` attribute, which is `CARGO_PKG_VERSION`
    // alone. `build.rs` appends the short sha when it can find one, because
    // the Cargo version cannot identify a build this project actually ships:
    // a nix flake pins a *rev*, and every rev between two releases prints the
    // identical `0.9.4`. See `emit_version` there for the two sources and for
    // the half of the problem a version string cannot fix.
    //
    // Both CI assertions on this output match a SUBSTRING -- ci.yml checks
    // `-notmatch "beckon"` on Windows and `*"$want"*` against
    // `nix eval .#beckon.version` -- so the suffix is safe by construction,
    // not by luck. A future check that compares for EQUALITY would break.
    version = env!("BECKON_VERSION"),
    about = "Cross-platform focus-or-launch app switcher",
    // Fires only on a genuinely empty argv. `beckon -v` parses clean to
    // (None, None) and is caught in `parse_checked` instead.
    arg_required_else_help = true,
    // Without this the usage line reads `[OPTIONS] [ID] [COMMAND]`, which
    // advertises a combination `parse_checked` goes on to reject.
    override_usage = "beckon [OPTIONS] <ID>\n       beckon [OPTIONS] <COMMAND>"
)]
// Deliberately NOT `args_conflicts_with_subcommands`. Measured on clap 4.6.1:
// that flag makes clap stop looking for a subcommand once any argument has
// been parsed (clap_builder/src/parser/parser.rs:592), so `beckon -v list`
// silently binds "list" to the ID positional and exits 0 — the very defect
// this surface exists to remove, and it would break the `-v` helper at
// testing/linux_live_test.py:509 that eight live focus tests run through.
// The id/subcommand conflict is enforced in `parse_checked`. See
// docs/superpowers/specs/2026-08-10-cli-subcommands-design.md.
struct Args {
    /// App identifier (sway app_id / macOS bundle_id / Windows name or AUMID).
    ///
    /// If the app is named after a subcommand (list, installed, search,
    /// resolve, doctor, check, serve, help) or the id starts with '-', pass it
    /// after a double dash:  beckon -- list
    #[arg(value_name = "ID")]
    id: Option<String>,

    #[command(subcommand)]
    command: Option<Command>,

    /// Verbose logging to stderr.
    #[arg(short = 'v', long, global = true)]
    verbose: bool,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// List currently running apps with their ids.
    List,

    /// List installed apps with launch ids.
    Installed,

    /// Fuzzy-search ids matching NAME across running and installed apps.
    Search {
        #[arg(value_name = "NAME")]
        name: String,
    },

    /// Validate an id and print metadata.
    Resolve {
        #[arg(value_name = "ID")]
        id: String,
    },

    /// Check the environment (compositor / IPC / permissions).
    Doctor,

    /// Validate a shortcuts TOML file (see `beckon serve`) and exit; 0 = valid.
    Check {
        #[arg(value_name = "CONFIG")]
        config: std::path::PathBuf,

        /// Also check that every app name resolves on this machine; exit 1
        /// if any does not.
        ///
        /// Opt-in, and it stays opt-in. Without it `check` validates syntax
        /// and chord uniqueness only, which is the same answer everywhere and
        /// is what makes it CI-friendly. Names are resolved against the local
        /// machine's installed-app metadata, and a CI runner has none of the
        /// apps — resolving by default would fail every run on a file that is
        /// perfectly good.
        ///
        /// Never asks the compositor, so it runs over SSH and in a headless
        /// VM. On macOS it also consults what is running, because that is the
        /// top of that OS's resolution ladder — `Finder` is installed where
        /// the bundle scan does not look and resolves only while it is up.
        #[arg(long)]
        resolve: bool,
    },

    /// Run as a resident hotkey service reading a shortcuts TOML file
    /// (macOS, Windows). Foreground; use launchd / Task Scheduler to
    /// daemonize.
    Serve {
        #[arg(value_name = "CONFIG")]
        config: std::path::PathBuf,

        /// Where this run's log is.
        ///
        /// **The flag means two different things, because the two platforms
        /// own different amounts of it, and that is worth reading before
        /// touching either.**
        ///
        /// On **Windows** it is an instruction: send stderr to PATH and
        /// detach the console. A Scheduled Task cannot redirect stderr, and
        /// stderr is the only place beckon reports how many hotkeys actually
        /// registered. Detaching is part of the same flag on purpose —
        /// detaching without redirecting would leave stderr pointing at a
        /// destroyed console, where a failed write panics instead of
        /// returning.
        ///
        /// On **macOS it is a declaration**, and beckon redirects nothing:
        /// launchd already owns the file through `StandardErrorPath`, and a
        /// second writer on the same fd is how a log gets interleaved
        /// garbage. What beckon lacks is not the redirect but the PATH —
        /// without it the tray's `Open log` and the System page's log row
        /// cannot be drawn at all, over a file that exists. The Homebrew
        /// formula passes the same path it gives launchd.
        ///
        /// Scoped to this subcommand, so it is rejected everywhere else
        /// structurally; it used to need `requires = "serve"`.
        #[cfg(any(target_os = "windows", target_os = "macos"))]
        #[arg(long, value_name = "PATH")]
        log: Option<std::path::PathBuf>,
    },
}

impl Args {
    /// `Args::parse()` plus the two invariants clap cannot express here.
    ///
    /// Both refusals exit 2, matching clap's own usage-error code.
    ///
    /// - `(Some, Some)` — measured on clap 4.6.1, a bare positional and a
    ///   subcommand can both be supplied and clap reports success. Without
    ///   this arm, `beckon Claude list` exits 0 and discards the id, which is
    ///   the 0.5.4 defect respelled.
    /// - `(None, None)` — `arg_required_else_help` covers only an empty argv,
    ///   so `beckon -v` alone would be a silent exit-0 no-op.
    fn parse_checked() -> Self {
        let argv: Vec<std::ffi::OsString> = std::env::args_os().collect();
        let args = match Self::try_parse_from(&argv) {
            Ok(a) => a,
            Err(e) => {
                explain_shadowed_verb(&e, &argv);
                e.exit()
            }
        };
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

/// Say so when a lone reserved word was probably meant as an app.
///
/// `beckon resolve` is ambiguous: a subcommand missing its operand, or an app
/// called Resolve. clap only ever reports the first, and "the following
/// required arguments were not provided: <ID>" sends the reader hunting for a
/// forgotten argument rather than telling them their app name is shadowed.
/// Subcommand matching is byte-exact while every beckon resolver is
/// case-insensitive, so capitalisation alone decides which reading applies —
/// `beckon Resolve` reaches the id, `beckon resolve` does not.
///
/// Only fires for `beckon <word>` with nothing else on the line; anything
/// longer is a real missing operand.
fn explain_shadowed_verb(e: &clap::Error, argv: &[std::ffi::OsString]) {
    if e.kind() != clap::error::ErrorKind::MissingRequiredArgument {
        return;
    }
    let Some(word) = shadowed_verb(argv) else {
        return;
    };
    let _ = e.print();
    eprintln!(
        "\nbeckon: `{word}` is a subcommand name, not an app id.\n\
         \x20       If you meant the app, run:  beckon -- {word}"
    );
    std::process::exit(2);
}

/// The word `explain_shadowed_verb` speaks up about, if any.
///
/// Split out because the rest of that function prints and then exits, so this
/// is the only half a test can reach — and it is the half that carries both
/// rules: the shape (`beckon <word>` and nothing else) and the membership.
fn shadowed_verb(argv: &[std::ffi::OsString]) -> Option<&str> {
    let [_, word] = argv else { return None };
    let word = word.to_str()?;
    RESERVED.contains(&word).then_some(word)
}

pub fn cli_main() {
    let args = Args::parse_checked();
    beckon_core::set_verbose(args.verbose);
    if let Err(e) = run(&args) {
        // Always to stderr.
        eprintln!("beckon: {e:#}");
        let message = format!("{e:#}");
        // `serve` is the one command a supervisor restarts on a fixed
        // interval forever (launchd KeepAlive, a Task Scheduler repetition),
        // so it is the one command whose failure here can repeat with nobody
        // asking. Every other command failed because a human just ran it.
        //
        // Widen this and the 5-minute Windows watchdog posts a desktop
        // notification every five minutes forever;
        // `notify_policy::repeated_serve_startup_failures_notify_once` is what
        // notices.
        let cause = if matches!(args.command, Some(Command::Serve { .. })) {
            notify::Cause::MachineRepeat
        } else {
            notify::Cause::HumanAction
        };
        notify::report_expected(&message, cause, is_expected(&e));
        std::process::exit(1);
    }
}

/// Is this a designed outcome wearing an error's clothes?
///
/// Such an error still prints to stderr and still exits non-zero — callers
/// and logs keep every bit of evidence — but it must not raise a desktop
/// notification, which is for things the owner has to act on.
///
/// Only one case so far: a watchdog tick finding the serve it guards already
/// alive (`AcquireError::AlreadyRunning`). Left as a function rather than
/// inlined so the policy is testable without a terminal, a held lock, or a
/// notification daemon.
fn is_expected(e: &anyhow::Error) -> bool {
    matches!(
        e.downcast_ref::<lockfile::AcquireError>(),
        Some(lockfile::AcquireError::AlreadyRunning { .. })
    )
}

/// A match, not an if-ladder.
///
/// The ladder this replaces tested every flag before `args.id`, so a command
/// flag that had forgotten to declare a conflict with the id silently won —
/// `beckon <id> -l` listed running apps and exited 0. Under one enum the
/// commands are exclusive by construction, and there is no order to get wrong.
fn run(args: &Args) -> Result<()> {
    match &args.command {
        Some(Command::Serve {
            config,
            #[cfg(any(target_os = "windows", target_os = "macos"))]
            log,
        }) => {
            #[cfg(target_os = "windows")]
            {
                // Before the lock, so the "already running" refusal is logged
                // too, and before anything else can fail — see the module doc
                // on `beckon_windows::logfile`.
                if let Some(log) = log.as_deref() {
                    beckon_windows::logfile::redirect_to_log(log)?;
                }
                // Threaded through so the tray menu's "Open log" (also
                // shown on this CLI path -- see `serve::install_tray_menu`)
                // knows where the file is instead of greying the row out
                // over a log that does, in fact, exist.
                serve::cmd_serve(config, log.clone())
            }
            #[cfg(target_os = "macos")]
            {
                // No `redirect_to_log` here, and that is the whole difference
                // from the Windows arm above: launchd already owns this file
                // through `StandardErrorPath`. The path is threaded through
                // only so the tray's `Open log` and the System page's log row
                // know where it is -- see the flag's own doc.
                serve::cmd_serve(config, log.clone())
            }
            #[cfg(not(any(target_os = "macos", target_os = "windows")))]
            {
                let _ = config;
                Err(anyhow!(
                    "`beckon serve` is only implemented on macOS and Windows"
                ))
            }
        }
        Some(Command::Check { config, resolve }) => cmd_check(config, *resolve),
        Some(Command::Doctor) => cmd_doctor(),
        Some(Command::List) => cmd_list(),
        Some(Command::Installed) => cmd_list_installed(),
        // `require_id` stays on both operands. clap enforces an operand's
        // presence, never its non-emptiness, so `String` instead of
        // `Option<String>` does not subsume the check.
        Some(Command::Search { name }) => cmd_search(require_id(name, "search NAME")?),
        Some(Command::Resolve { id }) => cmd_resolve(require_id(id, "resolve ID")?),
        None => match args.id.as_deref() {
            Some(id) => cmd_beckon(require_id(id, "id")?, args.verbose),
            // `parse_checked` rejects (None, None) before we get here.
            None => Err(anyhow!("no command given (use -h for help)")),
        },
    }
}

/// Reject an empty or whitespace-only id.
///
/// A dotfile doing `beckon "$APP"` with `$APP` unset used to resolve
/// through the Name-substring tier — an empty string is a substring of
/// every Name — and silently launch whatever app sorted first. An empty id
/// can never be what the user meant, so fail loudly instead.
fn require_id<'a>(value: &'a str, what: &str) -> Result<&'a str> {
    if value.trim().is_empty() {
        return Err(anyhow!("empty {what}: expected an app Name or id"));
    }
    Ok(value)
}

fn pick_backend() -> Result<Box<dyn Backend>> {
    #[cfg(target_os = "linux")]
    {
        beckon_linux::pick_backend().context("failed to pick a Linux backend")
    }
    #[cfg(target_os = "macos")]
    {
        beckon_macos::pick_backend().context("failed to pick the macOS backend")
    }
    #[cfg(target_os = "windows")]
    {
        beckon_windows::pick_backend().context("failed to pick the Windows backend")
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        Err(anyhow!("this OS is not supported"))
    }
}

/// Walk a candidate chain and act on the first rung this machine can serve.
///
/// The predicate is **"first that ACTS"**, not "first that resolves", and the
/// difference is not academic. `BackendError::NoMatch` is raised only after
/// the backend has looked at BOTH the installed-app catalog and the live
/// window list — on Linux the site sits inside the `Decision::Launch` arm —
/// so a running ad-hoc app that ships no `.desktop` file still wins its rung.
/// A ladder built on `certainty::Certainty` instead would grade that
/// `NoMatch` and step straight past a window that is on screen.
///
/// Only `NoMatch` is stepped over. Any other error aborts the whole press:
/// a broken IPC connection or a compositor refusal is not made better by
/// trying the next name against the same broken connection, and swallowing
/// it would turn one loud failure into several quiet ones.
///
/// Two properties fall out of putting this above the `Backend` trait, and
/// both were checked in the backends rather than assumed:
///
///  - **A skipped rung leaves nothing half-done.** The MRU write happens
///    after the match on Linux, only `if action.is_ok()` on macOS, and inside
///    the not-running branch on Windows.
///  - **A skipped rung fires no desktop notification.** The error reaches
///    `cli_main`'s reporting path only once every rung has failed. On Windows
///    that is the difference between the ~245 ms scan and the ~945 ms figure,
///    of which ~700 ms is the toast itself.
pub(crate) fn beckon_ladder(
    backend: &dyn beckon_core::Backend,
    id: &str,
    verbose: bool,
) -> Result<()> {
    let candidates = beckon_core::candidates::split(id).map_err(|e| anyhow!("{e}"))?;
    let chain = candidates.len() > 1;
    let mut misses: Vec<String> = Vec::new();

    for (i, cand) in candidates.iter().enumerate() {
        match backend.beckon(cand) {
            Ok(action) => {
                // Single-id output stays byte-identical: the extra line is
                // printed only for a chain, because
                // `testing/linux_live_test.py` greps `-v` output for
                // `action:` across eight live focus tests.
                if verbose {
                    if chain {
                        eprintln!("candidate {}/{} `{cand}`", i + 1, candidates.len());
                    }
                    eprintln!("action: {action:?}");
                }
                return Ok(());
            }
            Err(beckon_core::BackendError::NoMatch { hint, .. }) if chain => {
                if verbose {
                    eprintln!(
                        "candidate {}/{} `{cand}`: no match",
                        i + 1,
                        candidates.len()
                    );
                }
                misses.push(format!("`{cand}`: {hint}"));
            }
            Err(e) => {
                return Err(anyhow!(e)).with_context(|| format!("beckon failed for id `{cand}`"))
            }
        }
    }

    // Every rung missed. Name them all: reporting only the first would hide
    // that the fallback was tried, and reporting only the last would name the
    // candidate the user cares least about.
    Err(anyhow!(
        "no candidate of `{id}` matches anything on this machine:\n  {}",
        misses.join("\n  ")
    ))
}

fn cmd_beckon(id: &str, verbose: bool) -> Result<()> {
    let backend = pick_backend()?;
    beckon_ladder(backend.as_ref(), id, verbose)
}

fn cmd_list() -> Result<()> {
    let backend = pick_backend()?;
    let apps = backend.list_running().context("list_running failed")?;
    if apps.is_empty() {
        println!("(no running apps)");
        return Ok(());
    }
    println!("{:<40} {:>5}  NAME", "ID", "WINS");
    for a in apps {
        println!("{:<40} {:>5}  {}", a.id, a.window_count, a.name);
    }
    Ok(())
}

fn cmd_check(path: &std::path::Path, resolve: bool) -> Result<()> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("cannot read `{}`", path.display()))?;
    let shortcuts = beckon_core::shortcuts::parse_shortcuts(&text)
        .map_err(|e| anyhow!("{}: {}", path.display(), e))?;
    println!("ok: {} shortcuts", shortcuts.len());
    if !resolve {
        return Ok(());
    }
    check_resolution(&shortcuts, name_reports)
}

/// The `--resolve` half of `check`: what grade does every binding's name earn
/// on this machine?
///
/// The resolver is passed in, so the whole flag — the batching, the counting
/// and the report — is testable on a machine that has none of the apps in
/// question, which is every CI runner and is the exact condition the flag
/// exists to describe.
///
/// **Only `NoMatch` gates the exit code.** A `Guess` resolves; it is slow and
/// fragile, not dead, and two of this user's own bindings live on that tier on
/// purpose. Failing on it would turn a correct file red, which is how a check
/// stops being run.
fn check_resolution<'a>(
    shortcuts: &'a [Shortcut],
    report: impl FnOnce(&[&'a str]) -> Result<Vec<NameReport>>,
) -> Result<()> {
    // A value may be a CANDIDATE CHAIN, so the batch below is every candidate
    // of every binding, not every value. Split once, here, keeping the
    // failures: a malformed chain is graded as a miss further down rather
    // than dropped, because `beckon_ladder` refuses the identical string
    // before it reaches any backend — the key is permanently dead, and this
    // is the one command whose job is to say so. It does not abort the batch
    // either: a check that refuses to grade eighteen good bindings because
    // the nineteenth has a stray `||` is a check that stops being run.
    let chains: Vec<(&Shortcut, std::result::Result<Vec<&str>, String>)> = shortcuts
        .iter()
        .map(|s| (s, beckon_core::candidates::split(&s.app)))
        .collect();

    // Distinct names, asked in one call: several hotkeys aiming at one app is
    // the normal shape of a shortcuts file, and every backend answers a batch
    // with a single catalog scan.
    let mut names: Vec<&str> = chains
        .iter()
        .filter_map(|(_, cands)| cands.as_ref().ok())
        .flatten()
        .copied()
        .collect();
    names.sort_unstable();
    names.dedup();
    let reports = report(&names)?;
    let grade: std::collections::HashMap<&str, &NameReport> =
        reports.iter().map(|r| (r.id.as_str(), r)).collect();

    // The candidate that will WIN at runtime, graded as the binding's grade.
    //
    // This mirrors `beckon_ladder` exactly: the ladder stops at the first
    // candidate that is not a miss, so that is the one whose certainty the
    // user will actually live with. Grading a chain by its FIRST candidate
    // would call a working binding dead — which is the whole failure this
    // feature exists to remove, reintroduced one layer up — and grading it by
    // its BEST would hide a `Guess` that a later exact candidate never gets
    // the chance to beat.
    // A name the resolver did not answer for is an error, not a binding that
    // quietly disappears: every backend documents one report per name, in the
    // order given, and a silent drop takes the binding out of both the dead
    // list and the count.
    let winner = |cands: &[&str]| -> Result<&NameReport> {
        let mut last = None;
        for c in cands {
            let r = *grade
                .get(c)
                .with_context(|| format!("the resolver returned no report for `{c}`"))?;
            if r.certainty != Certainty::NoMatch {
                return Ok(r);
            }
            last = Some(r);
        }
        // Every rung missed. Report the LAST one: it is the candidate the
        // user added as the fallback, so it is the one whose absence is news.
        // `split` never yields an empty chain, so there is always one.
        last.ok_or_else(|| anyhow!("`{}` names no candidate at all", cands.join(" || ")))
    };

    // One grade per binding, in file order. Both arms answer the same
    // question — what will this key do on this machine? — so a chain that
    // does not split is a `NoMatch` carrying the parser's own sentence as its
    // consequence, which is exactly the shape `unresolved_report` prints.
    let mut graded: Vec<(&Shortcut, NameReport)> = Vec::with_capacity(chains.len());
    for (s, cands) in &chains {
        let r = match cands {
            Ok(cands) => winner(cands)?.clone(),
            Err(e) => NameReport {
                id: s.app.clone(),
                certainty: Certainty::NoMatch,
                target: None,
                tier: None,
                consequence: e.clone(),
                suggestions: Vec::new(),
            },
        };
        graded.push((*s, r));
    }

    let dead: Vec<(&Shortcut, &NameReport)> = graded
        .iter()
        .filter(|(_, r)| r.certainty == Certainty::NoMatch)
        .map(|(s, r)| (*s, r))
        .collect();
    let guessed: Vec<(&Shortcut, &NameReport)> = graded
        .iter()
        .filter(|(_, r)| r.certainty == Certainty::Guess)
        .map(|(s, r)| (*s, r))
        .collect();

    if dead.is_empty() {
        println!("ok: every app name resolves on this machine");
    } else {
        print!("{}", unresolved_report(&dead));
    }
    if !guessed.is_empty() {
        print!("{}", guess_report(&guessed));
    }
    if dead.is_empty() {
        return Ok(());
    }
    // The counts live here and nowhere else: the block above lists the
    // bindings, `main` prints this to stderr, and the two do not repeat
    // each other.
    Err(anyhow!(
        "{} of {} shortcuts name an app that does not resolve on this machine",
        dead.len(),
        shortcuts.len()
    ))
}

/// The block `check --resolve` prints for the bindings that cannot fire.
///
/// One line per binding, not per name: the question being asked is which
/// *hotkeys* are dead, and one uninstalled app can account for several. The
/// hint mirrors `beckon resolve`'s own "no match" output, which is where a
/// reader is sent for the detail on any single one — printing all of
/// `resolve`'s suggestion blocks here would bury the answer under fourteen
/// of them.
fn unresolved_report(dead: &[(&Shortcut, &NameReport)]) -> String {
    let mut s = String::from("\nThese shortcuts name an app this machine has no match for:\n");
    for (b, r) in dead {
        match r.tier {
            Some(t) => s.push_str(&format!(
                "   {:<30} {}  ({})\n",
                b.combo.canonical(),
                b.app,
                t
            )),
            None => s.push_str(&format!("   {:<30} {}\n", b.combo.canonical(), b.app)),
        }
        if !r.consequence.is_empty() {
            s.push_str(&format!("   {:<30} {}\n", "", r.consequence));
        }
    }
    s.push_str(
        "\nHint: `beckon resolve <ID>` explains one of them; \
         `beckon installed` lists what is installed.\n",
    );
    s
}

/// The block for bindings that resolve, but only by substring.
///
/// Separate from `unresolved_report` because it says something different: not
/// "this key is dead" but "this key works today for a reason the config does
/// not state". Each line carries the reason, because the reason is what makes
/// it actionable — a lone substring match invites a future install to steal
/// the name, while several candidates means the winner is already decided by
/// sort order rather than by anything the user wrote.
fn guess_report(guessed: &[(&Shortcut, &NameReport)]) -> String {
    let mut s = String::from("\nThese shortcuts resolve, but only loosely:\n");
    for (b, r) in guessed {
        match r.tier {
            Some(t) => s.push_str(&format!(
                "   {:<30} {}  ({})\n",
                b.combo.canonical(),
                b.app,
                t
            )),
            None => s.push_str(&format!("   {:<30} {}\n", b.combo.canonical(), b.app)),
        }
        if !r.consequence.is_empty() {
            s.push_str(&format!("   {:<30} {}\n", "", r.consequence));
        }
        for other in &r.suggestions {
            s.push_str(&format!("   {:<30} also matches: {}\n", "", other));
        }
    }
    s.push_str("\nThey do not fail this check. Naming the app exactly makes them exact.\n");
    s
}

/// One resolution report per name, from whichever backend this OS has.
///
/// Deliberately does NOT go through `pick_backend`. Resolution reads
/// installed-app metadata — `.desktop` files, LaunchServices, the Start menu
/// — which is on disk whether or not a session is running, so `check
/// --resolve` runs over SSH, in a headless VM and on a CI runner. Taking a
/// backend would make the flag fail for a reason that has nothing to do with
/// the question it asks, and the only alternative to failing there would be to
/// pass silently, which is worse than not having the flag at all.
///
/// The answer is not invariant, though, and the flag must not claim to be:
/// the macOS ladder starts at the running apps, so a bundle installed where
/// `installed_apps()` does not walk — `/System/Library/CoreServices/Finder.app`,
/// measured — resolves only while it is running. That is `resolve`'s own
/// behaviour, which this has to agree with, not something to fix here.
///
/// An OS with no backend crate is the one case left, and it errors: an empty
/// list would read as "every name resolved".
fn name_reports(names: &[&str]) -> Result<Vec<NameReport>> {
    #[cfg(target_os = "linux")]
    {
        beckon_linux::resolve_reports(names)
            .map_err(|e| anyhow!("{e}"))
            .context("resolving app names failed")
    }
    #[cfg(target_os = "macos")]
    {
        beckon_macos::resolve_reports(names)
            .map_err(|e| anyhow!("{e}"))
            .context("resolving app names failed")
    }
    #[cfg(target_os = "windows")]
    {
        beckon_windows::resolve_reports(names)
            .map_err(|e| anyhow!("{e}"))
            .context("resolving app names failed")
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        let _ = names;
        Err(anyhow!(
            "`beckon check --resolve` needs a backend, and this OS is not supported"
        ))
    }
}

fn cmd_list_installed() -> Result<()> {
    let backend = pick_backend()?;
    let apps = backend.list_installed().context("list_installed failed")?;
    if apps.is_empty() {
        println!("(no installed apps reported — backend may not implement list_installed yet)");
        return Ok(());
    }
    println!("{:<40} NAME", "ID");
    for a in apps {
        println!("{:<40} {}", a.id, a.name);
    }
    Ok(())
}

/// One printed row of `beckon search`: which list the app came from, its id
/// and its name.
///
/// Borrowed rather than owned: `cmd_search` holds both lists for the whole
/// call, and the table is the only consumer.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct SearchHit<'a> {
    pub where_: &'static str,
    pub id: &'a str,
    pub name: &'a str,
}

/// The pure half of `beckon search` — everything except the two backend
/// calls and the printing.
///
/// **Running apps lead the catalog**, and within each half the backend's own
/// order is kept. That is the ranking the table has always had and it is the
/// one worth keeping: a running app is the one the user can act on now, and
/// each backend already sorts its own list deterministically
/// (`desktop::scan()` by id, for the reason recorded at that function).
///
/// **An empty needle matches nothing.** `require_id` rejects it one call
/// above, so this arm is unreachable through the CLI today — but an empty
/// string is a substring of every id and every name, which is exactly the
/// tier-4 failure the Linux resolver was fixed for, so the property is
/// pinned here where a test can read it rather than resting on one caller.
pub(crate) fn search_hits<'a>(
    running: &'a [beckon_core::RunningApp],
    installed: &'a [beckon_core::InstalledApp],
    name: &str,
) -> Vec<SearchHit<'a>> {
    let needle = name.to_lowercase();
    if needle.is_empty() {
        return Vec::new();
    }
    let matches = |id: &str, name: &str| {
        id.to_lowercase().contains(&needle) || name.to_lowercase().contains(&needle)
    };

    let mut hits = Vec::new();
    for a in running {
        if matches(&a.id, &a.name) {
            hits.push(SearchHit {
                where_: "running",
                id: a.id.as_str(),
                name: a.name.as_str(),
            });
        }
    }
    for a in installed {
        if matches(&a.id, &a.name) {
            hits.push(SearchHit {
                where_: "installed",
                id: a.id.as_str(),
                name: a.name.as_str(),
            });
        }
    }
    hits
}

fn cmd_search(name: &str) -> Result<()> {
    let backend = pick_backend()?;

    let running = backend.list_running().unwrap_or_else(|e| {
        eprintln!("warning: list_running failed: {e}");
        Vec::new()
    });
    let installed = backend.list_installed().unwrap_or_else(|e| {
        eprintln!("warning: list_installed failed: {e}");
        Vec::new()
    });

    let hits = search_hits(&running, &installed, name);

    if hits.is_empty() {
        println!("no matches for `{name}`");
        return Ok(());
    }

    println!("{:<10} {:<40} NAME", "WHERE", "ID");
    for hit in hits {
        println!("{:<10} {:<40} {}", hit.where_, hit.id, hit.name);
    }
    Ok(())
}

/// `resolve` speaks the same `||` chain the shortcuts TOML is written in.
///
/// It was the one caller that did not. The hot path splits (see the call in
/// `cmd_beckon`) and so does `check --resolve`, so a chain worked everywhere
/// except in the command a person runs to find out *why* a line does not
/// work: it looked for an app literally named `"A || B"`, which nothing can
/// be, and answered `no match` about the whole line while the real answer was
/// about one candidate inside it.
///
/// Every candidate is reported, not just the first that resolves. `resolve`
/// is a discovery command — "which of these does this machine have?" is the
/// question, and stopping at the first hit answers a different one. That also
/// keeps this function out of the business of predicting the hot path: the
/// winner there is the first candidate that *acts*, which a report cannot
/// know, so it is not claimed.
///
/// A single id — the overwhelmingly common case — prints exactly what it
/// always did, with no heading. `split` yields one element for a string with
/// no separator, so that falls out of the loop rather than needing an arm.
fn cmd_resolve(id: &str) -> Result<()> {
    let candidates = beckon_core::candidates::split(id).map_err(|e| anyhow!("{e}"))?;
    let total = candidates.len();

    for (i, candidate) in candidates.iter().enumerate() {
        if total > 1 {
            if i > 0 {
                println!();
            }
            println!("── candidate {} of {}: `{}`", i + 1, total, candidate);
        }
        resolve_one(candidate)?;
    }
    Ok(())
}

fn resolve_one(id: &str) -> Result<()> {
    #[cfg(target_os = "linux")]
    {
        cmd_resolve_linux(id)
    }
    #[cfg(target_os = "macos")]
    {
        beckon_macos::print_resolve_report(id)
            .map_err(|e| anyhow!("{e}"))
            .context("resolve failed")
    }
    #[cfg(target_os = "windows")]
    {
        beckon_windows::print_resolve_report(id)
            .map_err(|e| anyhow!("{e}"))
            .context("resolve failed")
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        let backend = pick_backend()?;
        let running = backend.list_running().unwrap_or_default();
        if let Some(app) = running.iter().find(|a| a.id == id) {
            println!("running: {} ({} window)", app.id, app.window_count);
            return Ok(());
        }
        println!("id `{}` not found", id);
        Ok(())
    }
}

#[cfg(target_os = "linux")]
fn cmd_resolve_linux(id: &str) -> Result<()> {
    // The backend is OPTIONAL, and that is the whole point of this command
    // being usable where it is most needed. Resolution reads `.desktop` files
    // off the disk; only the running-app line needs a compositor. Taking
    // `pick_backend()?` here made `beckon resolve` fail outright over SSH, in
    // a headless VM and in a container — with "no supported display server",
    // an error about the half of the answer the caller did not ask for —
    // while `check --resolve`, which deliberately takes no backend, answered
    // the same question fine from the same shell. Measured on rog 2026-08-16.
    //
    // `None` is not "not running": it is "nobody asked the compositor", and
    // the Status line below says so rather than guessing.
    let backend = pick_backend().ok();
    let running: Option<Vec<_>> = backend.map(|b| b.list_running().unwrap_or_default());
    let resolved = beckon_linux::desktop::resolve_detailed(id);
    let subs = beckon_linux::desktop::name_substring_matches(id);

    let Some(m) = resolved else {
        println!("❌ no match for `{}`\n", id);
        if !subs.is_empty() {
            println!("Closest by name (substring):");
            for e in subs.iter().take(5) {
                println!("   {:<40} ({})", e.name, e.id);
            }
            println!();
        }
        if let Some(running) = &running {
            let direct: Vec<&_> = running.iter().filter(|a| a.id == id).collect();
            if !direct.is_empty() {
                println!(
                    "Note: a running window has app_id=`{}` but no .desktop matches it.",
                    id
                );
                println!("      Focus will work; launch will not.");
            }
        }
        println!("Hint: `beckon installed` lists installed, `beckon list` lists running.");
        return Ok(());
    };

    let runtime_id = &m.entry.id;
    let running_match = running
        .as_ref()
        .map(|r| r.iter().find(|a| a.id == *runtime_id));

    println!("✅ resolved");
    println!("   Input:        {}", id);
    println!("   Match type:   {}", m.match_type.describe());
    println!("   Name:         {}", m.entry.name);
    println!("   Runtime id:   {}", runtime_id);
    if let Some(wm) = &m.entry.startup_wm_class {
        if wm != runtime_id {
            println!("   StartupWMClass: {} (often ignored on Wayland)", wm);
        }
    }
    match running_match {
        Some(Some(app)) => println!(
            "   Status:       running ({} window: \"{}\")",
            app.window_count, app.name
        ),
        Some(None) => println!("   Status:       not running"),
        // Three states, not two. "not running" is a claim about the
        // compositor, and without one there is nothing to claim.
        None => println!("   Status:       unknown (no display server in this shell)"),
    }
    println!("   Exec:         {}", m.entry.exec);

    // Ambiguity warning: more than one Name-substring candidate exists,
    // and the user picked one via priority. Other matches might be what
    // they meant.
    let other_subs: Vec<&_> = subs.iter().filter(|e| e.id != m.entry.id).collect();
    if !other_subs.is_empty() {
        println!();
        println!(
            "⚠️  {} other entr{} also match by Name substring:",
            other_subs.len(),
            if other_subs.len() == 1 { "y" } else { "ies" }
        );
        for e in other_subs.iter().take(5) {
            println!("       {:<40} ({})", e.name, e.id);
        }
        println!("   Hint: use the exact Name from `beckon installed` to disambiguate.");
    }
    Ok(())
}

fn cmd_doctor() -> Result<()> {
    println!("=== beckon doctor ===\n");

    #[cfg(target_os = "linux")]
    {
        let sway_sock = std::env::var("SWAYSOCK").ok();
        let i3_sock = std::env::var("I3SOCK").ok();
        let hypr = std::env::var("HYPRLAND_INSTANCE_SIGNATURE").ok();
        let wayland = std::env::var("WAYLAND_DISPLAY").ok();
        let display = std::env::var("DISPLAY").ok();

        println!("Display server detection:");
        println!("  SWAYSOCK                    = {:?}", sway_sock);
        println!("  I3SOCK                      = {:?}", i3_sock);
        println!("  HYPRLAND_INSTANCE_SIGNATURE = {:?}", hypr);
        println!(
            "  NIRI_SOCKET                 = {:?}",
            std::env::var("NIRI_SOCKET").ok()
        );
        println!(
            "  MANGO_INSTANCE_SIGNATURE   = {:?}",
            std::env::var("MANGO_INSTANCE_SIGNATURE").ok()
        );
        println!("  WAYLAND_DISPLAY             = {:?}", wayland);
        println!("  DISPLAY                     = {:?}", display);
        println!();

        match beckon_linux::pick_backend() {
            Ok(backend) => {
                println!("✅ Backend selected.");
                match backend.list_running() {
                    Ok(apps) => println!(
                        "✅ IPC working — {} running window(s) detected.",
                        apps.iter().map(|a| a.window_count).sum::<usize>()
                    ),
                    Err(e) => println!("⚠️  Backend selected but list_running failed: {}", e),
                }
            }
            Err(e) => println!("❌ {}", e),
        }
    }
    #[cfg(target_os = "macos")]
    {
        println!("OS: macOS\n");
        let trusted = beckon_macos::is_accessibility_trusted();
        if trusted {
            println!("✅ Accessibility permission granted — window cycling (5a) is available.");
        } else {
            println!("⚠️  Accessibility permission NOT granted.");
            println!("    Cycling between windows of the same app (step 5a) requires it.");
            println!("    Without it, beckon falls back to toggle-back / hide.");
            println!();
            println!("    Grant in: System Settings → Privacy & Security → Accessibility");
            println!(
                "    Add the binary you invoke from Hammerspoon (the path that runs `beckon`)."
            );
            println!("    macOS binds the permission to the binary's code signature, so a fresh");
            println!("    `cargo build` may invalidate the grant — re-add after rebuilds.");
        }
        println!();

        let backend = pick_backend()?;
        match backend.list_running() {
            Ok(apps) => println!(
                "✅ NSWorkspace working — {} regular running app(s).",
                apps.len()
            ),
            Err(e) => println!("⚠️  list_running failed: {e}"),
        }
    }
    #[cfg(target_os = "windows")]
    {
        println!("OS: Windows\n");

        match beckon_windows::pick_backend() {
            Ok(backend) => {
                println!("Backend selected.\n");
                match backend.list_running() {
                    Ok(apps) => {
                        let total_wins: usize = apps.iter().map(|a| a.window_count).sum();
                        println!(
                            "EnumWindows working -- {} app(s), {} window(s) detected.",
                            apps.len(),
                            total_wins
                        );
                    }
                    Err(e) => println!("Backend selected but list_running failed: {}", e),
                }
                match backend.list_installed() {
                    Ok(apps) => println!(
                        "Windows app catalog working -- {} app(s) found.",
                        apps.len()
                    ),
                    Err(e) => println!("list_installed failed: {}", e),
                }
            }
            Err(e) => println!("{}", e),
        }
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        println!("This OS is not supported by beckon.");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use beckon_core::shortcuts::parse_shortcuts;

    #[test]
    fn already_running_is_expected() {
        let e = anyhow::Error::new(lockfile::AcquireError::AlreadyRunning {
            config: std::path::PathBuf::from("/tmp/apps.toml"),
            lock: std::path::PathBuf::from("/tmp/beckon-serve-0.lock"),
        });
        assert!(
            is_expected(&e),
            "a live serve refusing a probe is not a fault"
        );
    }

    #[test]
    fn lock_open_failure_is_not_expected() {
        let e = anyhow::Error::new(lockfile::AcquireError::Open {
            path: std::path::PathBuf::from("/tmp/beckon-serve-0.lock"),
            source: std::io::Error::from(std::io::ErrorKind::PermissionDenied),
        });
        assert!(
            !is_expected(&e),
            "an unopenable lock file is a real fault and must still notify"
        );
    }

    #[test]
    fn ordinary_errors_are_not_expected() {
        assert!(!is_expected(&anyhow!("no command given (use -h for help)")));
    }

    fn shortcuts(text: &str) -> Vec<Shortcut> {
        beckon_core::shortcuts::parse_shortcuts(text).expect("test fixture must parse")
    }

    fn report(id: &str, certainty: Certainty) -> NameReport {
        NameReport {
            id: id.to_string(),
            certainty,
            target: None,
            tier: None,
            consequence: if certainty == Certainty::Exact {
                String::new()
            } else {
                "because".to_string()
            },
            suggestions: Vec::new(),
        }
    }

    /// The whole point of the flag: a file that parses can still be entirely
    /// dead on the machine it is installed on, and bare `check` reports that
    /// file as `ok: N shortcuts`.
    #[test]
    fn resolution_fails_when_a_binding_names_an_app_this_machine_does_not_have() {
        let s = shortcuts("\"ctrl+alt+t\" = \"Terminal\"\n\"ctrl+alt+c\" = \"Claude\"\n");
        let e = check_resolution(&s, |names| {
            Ok(names
                .iter()
                .map(|n| {
                    if *n == "Claude" {
                        report(n, Certainty::NoMatch)
                    } else {
                        report(n, Certainty::Exact)
                    }
                })
                .collect())
        })
        .unwrap_err();
        assert_eq!(
            format!("{e}"),
            "1 of 2 shortcuts name an app that does not resolve on this machine"
        );
    }

    #[test]
    fn resolution_passes_when_every_name_resolves() {
        let s = shortcuts("\"ctrl+alt+t\" = \"Terminal\"\n\"ctrl+alt+c\" = \"Claude\"\n");
        assert!(check_resolution(&s, |names| {
            Ok(names.iter().map(|n| report(n, Certainty::Exact)).collect())
        })
        .is_ok());
    }

    /// A backend that cannot answer must not be read as "all clear" — the
    /// unsupported-OS arm and every backend's off-target stub return an error
    /// for exactly this reason.
    #[test]
    fn resolution_propagates_a_resolver_that_could_not_answer() {
        let s = shortcuts("\"ctrl+alt+t\" = \"Terminal\"\n");
        let e = check_resolution(&s, |_| Err(anyhow!("no backend here"))).unwrap_err();
        assert_eq!(format!("{e}"), "no backend here");
    }

    /// Several hotkeys aiming at one app is the normal shape of a shortcuts
    /// file, and the catalog scan is the expensive half of resolving — so the
    /// resolver is asked once, for the distinct names only.
    #[test]
    fn resolution_asks_the_resolver_once_for_each_distinct_name() {
        let s = shortcuts(
            "\"ctrl+alt+t\" = \"Terminal\"\n\
             \"ctrl+alt+u\" = \"Terminal\"\n\
             \"ctrl+alt+c\" = \"Claude\"\n",
        );
        let mut asked: Vec<String> = Vec::new();
        let e = check_resolution(&s, |names| {
            asked = names.iter().map(|n| n.to_string()).collect();
            Ok(names
                .iter()
                .map(|n| {
                    if *n == "Terminal" {
                        report(n, Certainty::NoMatch)
                    } else {
                        report(n, Certainty::Exact)
                    }
                })
                .collect())
        })
        .unwrap_err();
        assert_eq!(asked, ["Claude", "Terminal"]);
        // Both bindings on the missing app are dead, not one.
        assert_eq!(
            format!("{e}"),
            "2 of 3 shortcuts name an app that does not resolve on this machine"
        );
    }

    #[test]
    fn the_report_names_every_dead_binding_with_the_key_that_will_not_work() {
        let s = shortcuts("\"ctrl+super+alt+c\" = \"Claude\"\n\"ctrl+super+alt+g\" = \"Gmail\"\n");
        let claude = report("Claude", Certainty::NoMatch);
        let gmail = report("Gmail", Certainty::NoMatch);
        let dead: Vec<(&Shortcut, &NameReport)> = vec![(&s[0], &claude), (&s[1], &gmail)];
        let report = unresolved_report(&dead);
        assert!(report.contains("ctrl+super+alt+c"), "{report}");
        assert!(report.contains("Claude"), "{report}");
        assert!(report.contains("ctrl+super+alt+g"), "{report}");
        assert!(report.contains("Gmail"), "{report}");
    }

    /// `beckon resolve <ID>` is where the substring suggestions live; the
    /// report says so instead of repeating them once per dead binding.
    #[test]
    fn the_report_sends_the_reader_to_resolve_for_the_detail() {
        let s = shortcuts("\"ctrl+alt+c\" = \"Claude\"\n");
        let claude = report("Claude", Certainty::NoMatch);
        let dead: Vec<(&Shortcut, &NameReport)> = vec![(&s[0], &claude)];
        let report = unresolved_report(&dead);
        assert!(report.contains("beckon resolve <ID>"), "{report}");
        assert!(report.contains("beckon installed"), "{report}");
    }

    /// Each backend's `MISS_CONSEQUENCE` says something genuinely different
    /// per OS — a macOS miss errors, a Linux miss can still focus a live
    /// window by class, a Windows miss can still fall through to exe/title
    /// matching — so the report has to print it, not just the key and app.
    #[test]
    fn the_report_prints_a_dead_bindings_consequence() {
        let s = shortcuts("\"ctrl+alt+c\" = \"Claude\"\n");
        let mut claude = report("Claude", Certainty::NoMatch);
        claude.consequence = "no match; this key will error and launch nothing".to_string();
        let dead: Vec<(&Shortcut, &NameReport)> = vec![(&s[0], &claude)];
        let report = unresolved_report(&dead);
        assert!(
            report.contains("no match; this key will error and launch nothing"),
            "{report}"
        );
    }

    /// A guess resolves. It is slow and fragile, not dead — so it is printed
    /// and the exit code is untouched. This is the rule the whole three-grade
    /// change exists to express, and the one that separates it from the
    /// boolean it replaces.
    #[test]
    fn a_guess_is_reported_and_does_not_fail_the_check() {
        let shortcuts = parse_shortcuts("\"ctrl+alt+a\" = \"Brave\"\n").unwrap();
        let out = check_resolution(&shortcuts, |names| {
            Ok(names.iter().map(|n| report(n, Certainty::Guess)).collect())
        });
        assert!(out.is_ok(), "a guess must not fail the check: {out:?}");
    }

    /// And a guess alongside a miss must not soften the miss.
    #[test]
    fn a_miss_still_fails_even_when_another_row_is_only_a_guess() {
        let shortcuts =
            parse_shortcuts("\"ctrl+alt+a\" = \"Brave\"\n\"ctrl+alt+b\" = \"Zalo\"\n").unwrap();
        let err = check_resolution(&shortcuts, |names| {
            Ok(names
                .iter()
                .map(|n| {
                    if *n == "Zalo" {
                        report(n, Certainty::NoMatch)
                    } else {
                        report(n, Certainty::Guess)
                    }
                })
                .collect())
        })
        .unwrap_err();
        assert!(
            format!("{err}").contains("1 of 2 shortcuts"),
            "the count must name only the dead ones: {err}"
        );
    }

    // ---- candidate chains ----

    /// The failure this feature exists to remove, at the CHECK layer.
    ///
    /// Grading a chain by its first candidate would call a binding dead while
    /// the hotkey works perfectly -- the same wrong answer, one layer up, on
    /// exactly the bindings the chain was added to rescue.
    #[test]
    fn a_chain_is_graded_by_the_candidate_that_will_actually_win() {
        let s = shortcuts("\"ctrl+alt+k\" = \"Google Keep || https://keep.google.com/\"\n");
        let out = check_resolution(&s, |names| {
            // Both candidates are asked for in one batch.
            assert!(names.contains(&"Google Keep"), "{names:?}");
            assert!(names.contains(&"https://keep.google.com/"), "{names:?}");
            Ok(vec![
                report("Google Keep", Certainty::NoMatch),
                report("https://keep.google.com/", Certainty::Exact),
            ])
        });
        assert!(out.is_ok(), "the fallback resolves, so the file is fine");
    }

    /// And a chain whose every rung misses is still dead.
    #[test]
    fn a_chain_with_no_surviving_candidate_still_fails() {
        let s = shortcuts("\"ctrl+alt+k\" = \"Nope || Also nope\"\n");
        let e = check_resolution(&s, |_| {
            Ok(vec![
                report("Nope", Certainty::NoMatch),
                report("Also nope", Certainty::NoMatch),
            ])
        })
        .unwrap_err();
        assert!(e.to_string().contains("1 of 1"), "{e}");
    }

    /// A `Guess` that wins is reported as a Guess, not upgraded by a later
    /// exact candidate the ladder will never reach. Grading by the BEST
    /// candidate would hide the substring hazard `guess_report` exists to
    /// name.
    #[test]
    fn a_winning_guess_is_not_upgraded_by_a_later_exact_candidate() {
        let s = shortcuts("\"ctrl+alt+n\" = \"Notion || https://www.notion.so/\"\n");
        let out = check_resolution(&s, |_| {
            Ok(vec![
                report("Notion", Certainty::Guess),
                report("https://www.notion.so/", Certainty::Exact),
            ])
        });
        // Still exit 0 -- a Guess resolves -- but it must have been graded on
        // the Guess, which is what the printed block reports.
        assert!(out.is_ok());
    }

    /// A plain id is unchanged in every respect, which is the case that
    /// covers every binding the user already has.
    #[test]
    fn a_plain_id_is_graded_exactly_as_before() {
        let s = shortcuts("\"ctrl+alt+t\" = \"Terminal\"\n");
        let e = check_resolution(&s, |names| {
            assert_eq!(names, ["Terminal"]);
            Ok(vec![report("Terminal", Certainty::NoMatch)])
        })
        .unwrap_err();
        assert!(e.to_string().contains("1 of 1"), "{e}");
    }

    /// A trailing separator is the typo `candidates::split` was written to
    /// refuse, and `beckon_ladder` refuses it before any backend is asked —
    /// so the key can never fire. It used to vanish from both the dead list
    /// and the count, and `check --resolve` printed the green line.
    #[test]
    fn a_chain_that_does_not_split_is_reported_dead_rather_than_dropped() {
        let s = shortcuts("\"ctrl+super+alt+k\" = \"Google Keep || \"\n");
        let e = check_resolution(&s, |names| {
            // Nothing to ask about: the one binding contributes no candidate.
            assert!(names.is_empty(), "{names:?}");
            Ok(Vec::new())
        })
        .unwrap_err();
        assert!(e.to_string().contains("1 of 1"), "{e}");
    }

    /// And it must not take the rest of the file down with it: the eighteen
    /// good bindings are still graded, which is the reason the split is not
    /// simply propagated.
    #[test]
    fn a_malformed_chain_does_not_stop_the_other_bindings_being_graded() {
        let s = shortcuts("\"ctrl+alt+k\" = \"Google Keep || \"\n\"ctrl+alt+t\" = \"Terminal\"\n");
        let e = check_resolution(&s, |names| {
            assert_eq!(names, ["Terminal"]);
            Ok(vec![report("Terminal", Certainty::Exact)])
        })
        .unwrap_err();
        assert!(e.to_string().contains("1 of 2"), "{e}");
    }

    /// The parser's own sentence is what the reader gets: the report has to
    /// say `||`, or a dead row reads as an uninstalled app.
    #[test]
    fn the_report_prints_why_a_malformed_chain_cannot_fire() {
        let s = shortcuts("\"ctrl+alt+k\" = \"Google Keep || \"\n");
        let split = beckon_core::candidates::split(&s[0].app).unwrap_err();
        let r = NameReport {
            id: s[0].app.clone(),
            certainty: Certainty::NoMatch,
            target: None,
            tier: None,
            consequence: split,
            suggestions: Vec::new(),
        };
        let out = unresolved_report(&[(&s[0], &r)]);
        assert!(out.contains("ctrl+alt+k"), "{out}");
        assert!(out.contains("empty candidate"), "{out}");
    }

    /// One report per name is every backend's documented contract. A resolver
    /// that returns fewer used to make the binding disappear from the dead
    /// list AND from the count, which is the same silent-pass failure as the
    /// malformed chain above.
    #[test]
    fn a_resolver_that_skips_a_name_fails_rather_than_dropping_the_binding() {
        let s = shortcuts("\"ctrl+alt+t\" = \"Terminal\"\n");
        let e = check_resolution(&s, |_| Ok(Vec::new())).unwrap_err();
        assert!(e.to_string().contains("no report for `Terminal`"), "{e}");
    }

    // ---- the ladder ----

    /// Answers `beckon` from a script, and counts how many rungs were asked.
    struct FakeBackend {
        answers: std::cell::RefCell<Vec<beckon_core::Result<beckon_core::BeckonAction>>>,
        asked: std::cell::RefCell<Vec<String>>,
    }

    impl FakeBackend {
        fn new(answers: Vec<beckon_core::Result<beckon_core::BeckonAction>>) -> Self {
            Self {
                answers: std::cell::RefCell::new(answers),
                asked: std::cell::RefCell::new(Vec::new()),
            }
        }
    }

    impl beckon_core::Backend for FakeBackend {
        fn list_running(&self) -> beckon_core::Result<Vec<beckon_core::RunningApp>> {
            Ok(Vec::new())
        }
        fn list_installed(&self) -> beckon_core::Result<Vec<beckon_core::InstalledApp>> {
            Ok(Vec::new())
        }
        fn beckon(&self, id: &str) -> beckon_core::Result<beckon_core::BeckonAction> {
            self.asked.borrow_mut().push(id.to_string());
            self.answers.borrow_mut().remove(0)
        }
    }

    fn no_match(id: &str) -> beckon_core::BackendError {
        beckon_core::BackendError::NoMatch {
            id: id.to_string(),
            hint: "nothing here answers to it".to_string(),
        }
    }

    /// The whole reason `NoMatch` is its own variant: the second spelling of
    /// the app is tried, and it wins.
    #[test]
    fn a_no_match_steps_to_the_next_rung_of_the_chain() {
        let b = FakeBackend::new(vec![
            Err(no_match("Google Keep")),
            Ok(beckon_core::BeckonAction::Focused),
        ]);
        beckon_ladder(&b, "Google Keep || https://keep.google.com/", false).unwrap();
        assert_eq!(
            *b.asked.borrow(),
            ["Google Keep", "https://keep.google.com/"]
        );
    }

    /// And every other error aborts the press. Retrying the next name against
    /// the same dead socket turns one loud failure into several quiet ones,
    /// and hides the connection error the user actually needs — so the second
    /// rung must never be asked.
    #[test]
    fn any_error_that_is_not_a_no_match_aborts_the_whole_chain() {
        let b = FakeBackend::new(vec![Err(beckon_core::BackendError::Ipc(
            "socket went away".to_string(),
        ))]);
        let e = beckon_ladder(&b, "Google Keep || https://keep.google.com/", false).unwrap_err();
        assert_eq!(*b.asked.borrow(), ["Google Keep"]);
        assert!(format!("{e:#}").contains("socket went away"), "{e:#}");
        assert!(
            format!("{e:#}").contains("beckon failed for id `Google Keep`"),
            "{e:#}"
        );
    }

    /// A single id has no chain to step through, so even a `NoMatch` takes
    /// the abort arm and keeps the wording beckon has always printed.
    #[test]
    fn a_single_id_reports_a_no_match_without_the_chain_summary() {
        let b = FakeBackend::new(vec![Err(no_match("Claude"))]);
        let e = beckon_ladder(&b, "Claude", false).unwrap_err();
        let text = format!("{e:#}");
        assert!(text.contains("beckon failed for id `Claude`"), "{text}");
        assert!(!text.contains("no candidate of"), "{text}");
    }

    /// Every rung missing names them all: the first alone hides that a
    /// fallback was tried, the last alone names the candidate the user cares
    /// least about.
    #[test]
    fn a_chain_whose_every_rung_misses_names_all_of_them() {
        let b = FakeBackend::new(vec![Err(no_match("Nope")), Err(no_match("Also nope"))]);
        let e = beckon_ladder(&b, "Nope || Also nope", false).unwrap_err();
        let text = format!("{e:#}");
        assert!(
            text.contains("no candidate of `Nope || Also nope`"),
            "{text}"
        );
        assert!(text.contains("`Nope`"), "{text}");
        assert!(text.contains("`Also nope`"), "{text}");
    }

    /// A malformed chain never reaches the backend at all — which is the
    /// runtime half of `a_chain_that_does_not_split_is_reported_dead_rather_than_dropped`.
    #[test]
    fn a_chain_that_does_not_split_never_reaches_the_backend() {
        let b = FakeBackend::new(Vec::new());
        let e = beckon_ladder(&b, "Google Keep || ", false).unwrap_err();
        assert!(b.asked.borrow().is_empty(), "{:?}", b.asked.borrow());
        assert!(format!("{e:#}").contains("empty candidate"), "{e:#}");
    }

    // ---- search ----

    fn running(id: &str, name: &str) -> beckon_core::RunningApp {
        beckon_core::RunningApp {
            id: id.to_string(),
            name: name.to_string(),
            window_count: 1,
        }
    }

    fn installed(id: &str, name: &str) -> beckon_core::InstalledApp {
        beckon_core::InstalledApp {
            id: id.to_string(),
            name: name.to_string(),
            exec: None,
        }
    }

    /// Every beckon resolver is case-insensitive and `search` is the command
    /// people use to find the spelling to put in a dotfile, so it has to
    /// match the way the resolver will.
    #[test]
    fn search_matches_the_id_and_the_name_case_insensitively() {
        let inst = vec![
            installed("org.mozilla.firefox", "Firefox"),
            installed("kitty", "Terminal"),
        ];
        let hits = search_hits(&[], &inst, "FIRE");
        assert_eq!(hits.len(), 1, "matched the Name through a case difference");
        assert_eq!(hits[0].id, "org.mozilla.firefox");

        let hits = search_hits(&[], &inst, "KiTtY");
        assert_eq!(hits.len(), 1, "matched the id through a case difference");
        assert_eq!(hits[0].name, "Terminal");
    }

    /// The table's ranking, such as it is: a running app is the one the user
    /// can act on now, so it comes first even when the catalog carries the
    /// same app under the same name.
    #[test]
    fn running_apps_lead_the_installed_catalog() {
        let run = [running("kitty", "kitty")];
        let inst = [installed("kitty.desktop", "kitty")];
        let hits = search_hits(&run, &inst, "kitty");
        assert_eq!(
            hits.iter().map(|h| h.where_).collect::<Vec<_>>(),
            vec!["running", "installed"]
        );
    }

    /// The tier-4 bug respelled: an empty string is a substring of every id
    /// and every name, so a needle that says nothing must not select
    /// everything. `require_id` also rejects it at the CLI boundary — this
    /// pins the function, which is what a future caller would reach for.
    #[test]
    fn an_empty_needle_matches_nothing() {
        let run = [running("kitty", "kitty")];
        let inst = [installed("firefox", "Firefox")];
        let hits = search_hits(&run, &inst, "");
        assert!(hits.is_empty(), "an empty needle listed the whole machine");
    }

    /// The live suite's `search runs` case passes on zero matches, so this is
    /// the only place that distinguishes "nothing matched" from "the search
    /// matched nothing because it matches nothing".
    #[test]
    fn a_needle_that_matches_nothing_returns_nothing() {
        let run = [running("kitty", "kitty")];
        let inst = [installed("firefox", "Firefox")];
        let hits = search_hits(&run, &inst, "nothing-is-called-this");
        assert!(hits.is_empty());
    }

    // ---- reserved words ----

    fn argv(words: &[&str]) -> Vec<std::ffi::OsString> {
        words.iter().map(std::ffi::OsString::from).collect()
    }

    /// The case the migration plan named as the proof this explanation
    /// survived: without it `beckon resolve` reports only clap's "the
    /// following required arguments were not provided: <ID>", which sends the
    /// reader hunting for a forgotten argument.
    #[test]
    fn a_bare_reserved_word_is_explained_as_a_shadowed_app_name() {
        assert_eq!(
            shadowed_verb(&argv(&["beckon", "resolve"])),
            Some("resolve")
        );
        assert_eq!(shadowed_verb(&argv(&["beckon", "help"])), Some("help"));
    }

    /// Anything longer is a real missing operand, and `beckon search` with a
    /// name is not ambiguous at all.
    #[test]
    fn a_reserved_word_with_anything_after_it_is_a_real_missing_operand() {
        assert_eq!(shadowed_verb(&argv(&["beckon", "resolve", "Claude"])), None);
        assert_eq!(shadowed_verb(&argv(&["beckon"])), None);
    }

    /// Subcommand matching is byte-exact while every beckon resolver is
    /// case-insensitive, so capitalisation alone decides the reading.
    #[test]
    fn a_word_that_is_not_a_subcommand_is_left_to_clap() {
        assert_eq!(shadowed_verb(&argv(&["beckon", "Resolve"])), None);
        assert_eq!(shadowed_verb(&argv(&["beckon", "Claude"])), None);
    }

    /// `RESERVED` is hand-written and nothing tied it to the surface it
    /// describes, so a ninth subcommand would cost an app name and explain
    /// nothing. `help` is in the list because clap injects it — this is what
    /// notices if it ever stops.
    #[test]
    fn reserved_names_every_subcommand_clap_knows_about() {
        for c in Args::command().get_subcommands() {
            assert!(
                RESERVED.contains(&c.get_name()),
                "`{}` is a subcommand but not in RESERVED",
                c.get_name()
            );
        }
    }
}
