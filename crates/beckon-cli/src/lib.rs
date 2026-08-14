//! `beckon`'s command surface, as a library so that both binaries in this
//! package can share it: `beckon` (console subsystem, the CLI) and
//! `beckon-serve` (GUI subsystem, the Windows tray app). Splitting it out
//! is what lets `serve.rs` have exactly one implementation.

use anyhow::{anyhow, Context, Result};
use beckon_core::shortcuts::Shortcut;
use beckon_core::Backend;
use clap::{CommandFactory, Parser, Subcommand};

mod lockfile;
mod notify;
#[cfg(any(target_os = "macos", target_os = "windows"))]
mod serve;
mod serve_app;
mod stable_id;

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
    version,
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

        /// Send stderr to PATH and detach the console (Windows).
        ///
        /// For supervisor-hosted runs: a Scheduled Task cannot redirect
        /// stderr, and stderr is the only place beckon reports how many
        /// hotkeys actually registered. Detaching the console is part of the
        /// same flag on purpose — detaching without redirecting would leave
        /// stderr pointing at a destroyed console, where a failed write panics
        /// instead of returning.
        ///
        /// Scoped to this subcommand, so it is rejected everywhere else
        /// structurally; it used to need `requires = "serve"`.
        #[cfg(target_os = "windows")]
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
    let [_, word] = argv else { return };
    let Some(word) = word.to_str() else { return };
    if !RESERVED.contains(&word) {
        return;
    }
    let _ = e.print();
    eprintln!(
        "\nbeckon: `{word}` is a subcommand name, not an app id.\n\
         \x20       If you meant the app, run:  beckon -- {word}"
    );
    std::process::exit(2);
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
            #[cfg(target_os = "windows")]
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
                serve::cmd_serve(config, None)
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

fn cmd_beckon(id: &str, verbose: bool) -> Result<()> {
    let backend = pick_backend()?;
    let action = backend
        .beckon(id)
        .with_context(|| format!("beckon failed for id `{id}`"))?;
    if verbose {
        eprintln!("action: {action:?}");
    }
    Ok(())
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
    check_resolution(&shortcuts, unresolved_names)
}

/// The `--resolve` half of `check`: does every binding name something this
/// machine can actually find?
///
/// The resolver is passed in, so the whole flag — the batching, the counting
/// and the report — is testable on a machine that has none of the apps in
/// question, which is every CI runner and is the exact condition the flag
/// exists to describe.
fn check_resolution<'a>(
    shortcuts: &'a [Shortcut],
    unresolved: impl FnOnce(&[&'a str]) -> Result<Vec<&'a str>>,
) -> Result<()> {
    // Distinct names, asked in one call: several hotkeys aiming at one app is
    // the normal shape of a shortcuts file, and every backend answers a batch
    // with a single catalog scan.
    let mut names: Vec<&str> = shortcuts.iter().map(|s| s.app.as_str()).collect();
    names.sort_unstable();
    names.dedup();
    let missing: std::collections::HashSet<&str> = unresolved(&names)?.into_iter().collect();

    let dead: Vec<&Shortcut> = shortcuts
        .iter()
        .filter(|s| missing.contains(s.app.as_str()))
        .collect();
    if dead.is_empty() {
        println!("ok: every app name resolves on this machine");
        return Ok(());
    }
    print!("{}", unresolved_report(&dead));
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
fn unresolved_report(dead: &[&Shortcut]) -> String {
    let mut s = String::from("\nThese shortcuts name an app this machine has no match for:\n");
    for b in dead {
        s.push_str(&format!("   {:<30} {}\n", b.combo.canonical(), b.app));
    }
    s.push_str(
        "\nHint: `beckon resolve <ID>` explains one of them; \
         `beckon installed` lists what is installed.\n",
    );
    s
}

/// Which of `names` no installed app on this machine answers to.
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
fn unresolved_names<'a>(names: &[&'a str]) -> Result<Vec<&'a str>> {
    #[cfg(target_os = "linux")]
    {
        beckon_linux::unresolved_names(names)
            .map_err(|e| anyhow!("{e}"))
            .context("resolving app names failed")
    }
    #[cfg(target_os = "macos")]
    {
        beckon_macos::unresolved_names(names)
            .map_err(|e| anyhow!("{e}"))
            .context("resolving app names failed")
    }
    #[cfg(target_os = "windows")]
    {
        beckon_windows::unresolved_names(names)
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

fn cmd_search(name: &str) -> Result<()> {
    let backend = pick_backend()?;
    let needle = name.to_lowercase();

    let running = backend.list_running().unwrap_or_else(|e| {
        eprintln!("warning: list_running failed: {e}");
        Vec::new()
    });
    let installed = backend.list_installed().unwrap_or_else(|e| {
        eprintln!("warning: list_installed failed: {e}");
        Vec::new()
    });

    let mut hits = Vec::new();
    for a in &running {
        if a.id.to_lowercase().contains(&needle) || a.name.to_lowercase().contains(&needle) {
            hits.push(("running", a.id.as_str(), a.name.as_str()));
        }
    }
    for a in &installed {
        if a.id.to_lowercase().contains(&needle) || a.name.to_lowercase().contains(&needle) {
            hits.push(("installed", a.id.as_str(), a.name.as_str()));
        }
    }

    if hits.is_empty() {
        println!("no matches for `{name}`");
        return Ok(());
    }

    println!("{:<10} {:<40} NAME", "WHERE", "ID");
    for (where_, id, name) in hits {
        println!("{where_:<10} {id:<40} {name}");
    }
    Ok(())
}

fn cmd_resolve(id: &str) -> Result<()> {
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
    let backend = pick_backend()?;
    let running = backend.list_running().unwrap_or_default();
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
        let direct: Vec<&_> = running.iter().filter(|a| a.id == id).collect();
        if !direct.is_empty() {
            println!(
                "Note: a running window has app_id=`{}` but no .desktop matches it.",
                id
            );
            println!("      Focus will work; launch will not.");
        }
        println!("Hint: `beckon installed` lists installed, `beckon list` lists running.");
        return Ok(());
    };

    let runtime_id = &m.entry.id;
    let running_match: Option<&_> = running.iter().find(|a| a.id == *runtime_id);

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
        Some(app) => println!(
            "   Status:       running ({} window: \"{}\")",
            app.window_count, app.name
        ),
        None => println!("   Status:       not running"),
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

    /// The whole point of the flag: a file that parses can still be entirely
    /// dead on the machine it is installed on, and bare `check` reports that
    /// file as `ok: N shortcuts`.
    #[test]
    fn resolution_fails_when_a_binding_names_an_app_this_machine_does_not_have() {
        let s = shortcuts("\"ctrl+alt+t\" = \"Terminal\"\n\"ctrl+alt+c\" = \"Claude\"\n");
        let e = check_resolution(&s, |_| Ok(vec!["Claude"])).unwrap_err();
        assert_eq!(
            format!("{e}"),
            "1 of 2 shortcuts name an app that does not resolve on this machine"
        );
    }

    #[test]
    fn resolution_passes_when_every_name_resolves() {
        let s = shortcuts("\"ctrl+alt+t\" = \"Terminal\"\n\"ctrl+alt+c\" = \"Claude\"\n");
        assert!(check_resolution(&s, |_| Ok(Vec::new())).is_ok());
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
            Ok(vec!["Terminal"])
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
        let dead: Vec<&Shortcut> = s.iter().collect();
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
        let dead: Vec<&Shortcut> = s.iter().collect();
        let report = unresolved_report(&dead);
        assert!(report.contains("beckon resolve <ID>"), "{report}");
        assert!(report.contains("beckon installed"), "{report}");
    }
}
