use anyhow::{anyhow, Context, Result};
use beckon_core::{Backend, BeckonAction};
use clap::Parser;
use std::io::IsTerminal;

/// Cross-platform focus-or-launch app switcher.
///
/// Hot path: `beckon <id>` — focus an app if running, launch it if not, cycle
/// windows / toggle to previous app if already focused.
///
/// `<id>` is the raw OS identifier:
///   - sway / Wayland: `app_id` (e.g. `kitty`, `firefox`, `claude.ai__new`)
///   - macOS:          `bundle_id` (e.g. `com.anthropic.claudefordesktop`)
///   - Windows:        `exe` (e.g. `Claude.exe`)
///
/// Use `beckon -l` to discover ids on the current machine.
#[derive(Parser, Debug)]
#[command(
    name = "beckon",
    version,
    about = "Cross-platform focus-or-launch app switcher",
    arg_required_else_help = true
)]
struct Args {
    /// App identifier (sway app_id / macOS bundle_id / Windows exe).
    #[arg(value_name = "ID")]
    id: Option<String>,

    /// List currently running apps with their ids.
    #[arg(short = 'l', long, conflicts_with_all = ["list_installed", "search", "resolve", "doctor"])]
    list: bool,

    /// List installed apps with launch ids.
    #[arg(short = 'L', long = "list-installed", conflicts_with_all = ["list", "search", "resolve", "doctor"])]
    list_installed: bool,

    /// Fuzzy-search ids matching NAME across running and installed apps.
    #[arg(short = 's', long, value_name = "NAME", conflicts_with_all = ["list", "list_installed", "resolve", "doctor"])]
    search: Option<String>,

    /// Validate an id and print metadata.
    #[arg(short = 'r', long, value_name = "ID", conflicts_with_all = ["list", "list_installed", "search", "doctor"])]
    resolve: Option<String>,

    /// Check the environment (compositor / IPC / permissions).
    #[arg(short = 'd', long, conflicts_with_all = ["list", "list_installed", "search", "resolve"])]
    doctor: bool,

    /// Verbose logging to stderr.
    #[arg(short = 'v', long)]
    verbose: bool,
}

fn main() {
    let args = Args::parse();
    if let Err(e) = run(&args) {
        // Always to stderr.
        eprintln!("beckon: {:#}", e);
        // If invoked from a hotkey binding (no controlling terminal),
        // stderr goes to /dev/null and the user sees nothing. Fire a
        // desktop notification so the failure is visible.
        if !std::io::stderr().is_terminal() {
            notify_error(&format!("{:#}", e));
        }
        std::process::exit(1);
    }
}

fn run(args: &Args) -> Result<()> {
    if args.doctor {
        return cmd_doctor();
    }
    if args.list {
        return cmd_list();
    }
    if args.list_installed {
        return cmd_list_installed();
    }
    if let Some(name) = args.search.as_deref() {
        return cmd_search(name);
    }
    if let Some(id) = args.resolve.as_deref() {
        return cmd_resolve(id);
    }
    if let Some(id) = args.id.as_deref() {
        return cmd_beckon(id, args.verbose);
    }
    Err(anyhow!("no command given (use -h for help)"))
}

/// Fire a desktop notification. Best-effort: silent if `notify-send` is
/// not installed or the notification daemon is unreachable. Used when
/// stderr is not a terminal (i.e. invoked from a hotkey).
fn notify_error(message: &str) {
    #[cfg(target_os = "linux")]
    {
        let _ = std::process::Command::new("notify-send")
            .args([
                "--app-name=beckon",
                "--urgency=critical",
                "--icon=dialog-error",
                "beckon error",
                message,
            ])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn();
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = message;
    }
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
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        Err(anyhow!("this OS is not yet supported (phase 3 = Windows)"))
    }
}

fn cmd_beckon(id: &str, verbose: bool) -> Result<()> {
    let backend = pick_backend()?;
    let action = backend
        .beckon(id)
        .with_context(|| format!("beckon failed for id `{}`", id))?;
    if verbose {
        eprintln!("action: {:?}", action);
    }
    // Exit code conveys outcome category for scripting; 0 always for success.
    let _ = action_exit_code(action);
    Ok(())
}

fn action_exit_code(action: BeckonAction) -> i32 {
    // Reserved for future use if we need granular exit codes.
    match action {
        BeckonAction::Launched => 0,
        BeckonAction::Focused => 0,
        BeckonAction::Cycled => 0,
        BeckonAction::ToggledBack => 0,
        BeckonAction::Hidden => 0,
    }
}

fn cmd_list() -> Result<()> {
    let backend = pick_backend()?;
    let apps = backend.list_running().context("list_running failed")?;
    if apps.is_empty() {
        println!("(no running apps)");
        return Ok(());
    }
    println!("{:<40} {:>5}  {}", "ID", "WINS", "NAME");
    for a in apps {
        println!("{:<40} {:>5}  {}", a.id, a.window_count, a.name);
    }
    Ok(())
}

fn cmd_list_installed() -> Result<()> {
    let backend = pick_backend()?;
    let apps = backend.list_installed().context("list_installed failed")?;
    if apps.is_empty() {
        println!("(no installed apps reported — backend may not implement list_installed yet)");
        return Ok(());
    }
    println!("{:<40} {}", "ID", "NAME");
    for a in apps {
        println!("{:<40} {}", a.id, a.name);
    }
    Ok(())
}

fn cmd_search(name: &str) -> Result<()> {
    let backend = pick_backend()?;
    let needle = name.to_lowercase();

    let running = backend.list_running().unwrap_or_default();
    let installed = backend.list_installed().unwrap_or_default();

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
        println!("no matches for `{}`", name);
        return Ok(());
    }

    println!("{:<10} {:<40} {}", "WHERE", "ID", "NAME");
    for (where_, id, name) in hits {
        println!("{:<10} {:<40} {}", where_, id, name);
    }
    Ok(())
}

fn cmd_resolve(id: &str) -> Result<()> {
    #[cfg(target_os = "linux")]
    {
        return cmd_resolve_linux(id);
    }
    #[cfg(target_os = "macos")]
    {
        return beckon_macos::print_resolve_report(id)
            .map_err(|e| anyhow!("{}", e))
            .context("resolve failed");
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let backend = pick_backend()?;
        let running = backend.list_running().unwrap_or_default();
        if let Some(app) = running.iter().find(|a| a.id == id) {
            println!("✅ running: {} ({} window)", app.id, app.window_count);
            return Ok(());
        }
        println!("❌ id `{}` not found", id);
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
            println!("Note: a running window has app_id=`{}` but no .desktop matches it.", id);
            println!("      Focus will work; launch will not.");
        }
        println!("Hint: `beckon -L` lists installed, `beckon -l` lists running.");
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
    let other_subs: Vec<&_> = subs
        .iter()
        .filter(|e| e.id != m.entry.id)
        .collect();
    if !other_subs.is_empty() {
        println!();
        println!("⚠️  {} other entr{} also match by Name substring:",
            other_subs.len(),
            if other_subs.len() == 1 { "y" } else { "ies" });
        for e in other_subs.iter().take(5) {
            println!("       {:<40} ({})", e.name, e.id);
        }
        println!("   Hint: use the exact Name from `beckon -L` to disambiguate.");
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
                    Ok(apps) => println!("✅ IPC working — {} running window(s) detected.", apps.iter().map(|a| a.window_count).sum::<usize>()),
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
            println!("    Add the binary you invoke from Hammerspoon (the path that runs `beckon`).");
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
            Err(e) => println!("⚠️  list_running failed: {}", e),
        }
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        println!("⚠️  This OS is not yet supported by beckon (phase 3 = Windows).");
    }
    Ok(())
}
