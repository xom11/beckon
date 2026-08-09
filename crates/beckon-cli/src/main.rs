use anyhow::{anyhow, Context, Result};
use beckon_core::Backend;
use clap::Parser;
use std::io::IsTerminal;

mod lockfile;
#[cfg(any(target_os = "macos", target_os = "windows"))]
mod serve;

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
/// Use `beckon -l` to discover ids on the current machine.
#[derive(Parser, Debug)]
#[command(
    name = "beckon",
    version,
    about = "Cross-platform focus-or-launch app switcher",
    arg_required_else_help = true
)]
struct Args {
    /// App identifier (sway app_id / macOS bundle_id / Windows name or AUMID).
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

    /// Validate a shortcuts TOML file (see --serve) and exit; 0 = valid.
    #[arg(long, value_name = "CONFIG", conflicts_with_all = ["id", "list", "list_installed", "search", "resolve", "doctor"])]
    check: Option<std::path::PathBuf>,

    /// Run as a resident hotkey service reading a shortcuts TOML file
    /// (macOS, Windows). Foreground; use launchd / Task Scheduler to
    /// daemonize.
    #[arg(long, value_name = "CONFIG", conflicts_with_all = ["id", "list", "list_installed", "search", "resolve", "doctor", "check"])]
    serve: Option<std::path::PathBuf>,

    /// Verbose logging to stderr.
    #[arg(short = 'v', long)]
    verbose: bool,
}

fn main() {
    let args = Args::parse();
    beckon_core::set_verbose(args.verbose);
    if let Err(e) = run(&args) {
        // Always to stderr.
        eprintln!("beckon: {e:#}");
        let message = format!("{e:#}");
        // `--serve` is the one command a supervisor restarts on a fixed
        // interval forever (launchd KeepAlive, a Task Scheduler repetition),
        // so it is the one command whose startup failure can repeat by
        // itself. Everything else fails because a human just asked for it,
        // and a human who asks twice deserves telling twice.
        let supervised = args.serve.is_some();
        if should_notify(std::io::stderr().is_terminal(), notifications_muted(), &e)
            && (!supervised || claim_repeat_slot(&message))
        {
            notify_error(&message);
        }
        std::process::exit(1);
    }
}

/// Environment variable that silences every desktop notification.
///
/// The integration tests run the real binary against deliberately broken
/// configs with stderr captured — which is exactly the shape of "nobody is
/// watching stderr", so each run used to throw four real notifications at
/// whoever typed `cargo test`. Measured on macOS 2026-08-09: the machine's
/// entire retained notification history was beckon's own test fixtures.
const MUTE_ENV: &str = "BECKON_NO_NOTIFY";

fn notifications_muted() -> bool {
    muted_by(std::env::var_os(MUTE_ENV).as_deref())
}

/// Any non-empty value mutes. Empty is treated as unset so that an exported
/// but blank variable does not silently disable notifications.
fn muted_by(value: Option<&std::ffi::OsStr>) -> bool {
    value.is_some_and(|v| !v.is_empty())
}

/// Does this failure deserve interrupting the user with a notification?
///
/// Kept free of I/O so the policy can be tested without a terminal, a lock or
/// a notification daemon.
fn should_notify(stderr_is_terminal: bool, muted: bool, e: &anyhow::Error) -> bool {
    // A terminal already showed the message; a notification would duplicate it.
    // Without one — a hotkey binding, a launchd agent, a scheduled task —
    // stderr goes to a log or to /dev/null and the failure would be invisible.
    !stderr_is_terminal && !muted && !is_expected(e)
}

/// How long an identical supervised failure stays "already reported".
///
/// Long enough that a restart loop nags rather than screams, short enough
/// that a fault left unfixed keeps reminding. Measured on macOS 2026-08-09:
/// launchd's `ThrottleInterval` of 60 turned one unreadable config into a
/// notification every minute — 1440 a day; the Windows watchdog's five-minute
/// repetition would give 288.
const REPEAT_WINDOW: std::time::Duration = std::time::Duration::from_secs(60 * 60);

/// Claim the right to report `message` now, or decline if an identical one
/// was reported within `REPEAT_WINDOW`.
///
/// The state has to live on disk: every restart in a supervised loop is a
/// fresh process, so an in-memory guard would reset on each one — the very
/// thing being guarded against. Best-effort throughout; a temp directory we
/// cannot write to costs a duplicate notification, never a missing one.
fn claim_repeat_slot(message: &str) -> bool {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    message.hash(&mut h);
    let stamp = std::env::temp_dir().join(format!("beckon-notify-{:016x}.stamp", h.finish()));

    let recent = std::fs::metadata(&stamp)
        .and_then(|m| m.modified())
        .is_ok_and(|t| t.elapsed().is_ok_and(|age| age < REPEAT_WINDOW));
    if recent {
        return false;
    }
    // Rewriting refreshes mtime, which is the clock this reads back.
    let _ = std::fs::write(&stamp, message);
    true
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
        Some(lockfile::AcquireError::AlreadyRunning(_))
    )
}

fn run(args: &Args) -> Result<()> {
    if let Some(path) = args.serve.as_deref() {
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        {
            return serve::cmd_serve(path);
        }
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        {
            let _ = path;
            return Err(anyhow!("--serve is only implemented on macOS and Windows"));
        }
    }
    if let Some(path) = args.check.as_deref() {
        return cmd_check(path);
    }
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
        return cmd_search(require_id(name, "--search")?);
    }
    if let Some(id) = args.resolve.as_deref() {
        return cmd_resolve(require_id(id, "--resolve")?);
    }
    if let Some(id) = args.id.as_deref() {
        return cmd_beckon(require_id(id, "id")?, args.verbose);
    }
    Err(anyhow!("no command given (use -h for help)"))
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

/// Escape a string for embedding inside a double-quoted AppleScript literal.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn applescript_escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

/// Fire a desktop notification. Best-effort: silent if `notify-send` is
/// not installed or the notification daemon is unreachable. Used when
/// stderr is not a terminal (i.e. invoked from a hotkey).
///
/// The `MUTE_ENV` check lives here rather than only at the call site in
/// `main`, because `serve` notifies directly from its own long-running loop.
/// One chokepoint means a future call site cannot forget it.
fn notify_error(message: &str) {
    if notifications_muted() {
        return;
    }
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
    #[cfg(target_os = "windows")]
    {
        // Best-effort toast notification via PowerShell.
        let _ = std::process::Command::new("powershell")
            .args([
                "-NoProfile",
                "-Command",
                &format!(
                    "[Windows.UI.Notifications.ToastNotificationManager, Windows.UI.Notifications, ContentType = WindowsRuntime] > $null; \
                     $xml = [Windows.UI.Notifications.ToastNotificationManager]::GetTemplateContent(0); \
                     $text = $xml.GetElementsByTagName('text'); \
                     $text.Item(0).AppendChild($xml.CreateTextNode('beckon: {}')) > $null; \
                     [Windows.UI.Notifications.ToastNotificationManager]::CreateToastNotifier('beckon').Show([Windows.UI.Notifications.ToastNotification]::new($xml))",
                    message.replace('\'', "''")
                ),
            ])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn();
    }
    #[cfg(target_os = "macos")]
    {
        let script = format!(
            r#"display notification "{}" with title "beckon""#,
            applescript_escape(message)
        );
        let _ = std::process::Command::new("osascript")
            .args(["-e", &script])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
    }
    #[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
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

fn cmd_check(path: &std::path::Path) -> Result<()> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("cannot read `{}`", path.display()))?;
    let shortcuts = beckon_core::shortcuts::parse_shortcuts(&text)
        .map_err(|e| anyhow!("{}: {}", path.display(), e))?;
    println!("ok: {} shortcuts", shortcuts.len());
    Ok(())
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
    fn applescript_escape_quotes_and_backslashes() {
        assert_eq!(
            applescript_escape(r#"say "hi" \ bye"#),
            r#"say \"hi\" \\ bye"#
        );
    }

    #[test]
    fn already_running_is_expected() {
        let e = anyhow::Error::new(lockfile::AcquireError::AlreadyRunning(
            std::path::PathBuf::from("/tmp/beckon-serve-0.lock"),
        ));
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

    #[test]
    fn mute_env_is_off_when_unset_or_empty() {
        assert!(!muted_by(None));
        assert!(!muted_by(Some(std::ffi::OsStr::new(""))));
    }

    #[test]
    fn mute_env_is_on_for_any_non_empty_value() {
        assert!(muted_by(Some(std::ffi::OsStr::new("1"))));
        assert!(muted_by(Some(std::ffi::OsStr::new("0"))));
        assert!(muted_by(Some(std::ffi::OsStr::new("yes"))));
    }

    #[test]
    fn should_notify_only_when_nobody_is_watching_stderr() {
        let real = anyhow!("boom");
        assert!(should_notify(false, false, &real), "the intended case");
        assert!(
            !should_notify(true, false, &real),
            "a terminal already showed it"
        );
        assert!(!should_notify(false, true, &real), "muted");

        let expected = anyhow::Error::new(lockfile::AcquireError::AlreadyRunning(
            std::path::PathBuf::from("/tmp/beckon-serve-0.lock"),
        ));
        assert!(!should_notify(false, false, &expected), "designed outcome");
    }

    #[test]
    fn repeat_slot_opens_once_then_closes() {
        let msg = format!("beckon-selftest-repeat-{}", std::process::id());
        assert!(claim_repeat_slot(&msg), "first sighting must notify");
        assert!(
            !claim_repeat_slot(&msg),
            "a supervisor restarting every minute must not notify again"
        );
    }

    #[test]
    fn repeat_slot_is_per_message() {
        let a = format!("beckon-selftest-a-{}", std::process::id());
        let b = format!("beckon-selftest-b-{}", std::process::id());
        assert!(claim_repeat_slot(&a));
        assert!(
            claim_repeat_slot(&b),
            "a different failure is news even inside the window"
        );
    }
}
