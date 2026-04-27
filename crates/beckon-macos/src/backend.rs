//! `MacBackend` — implementation of the `Backend` trait for macOS.

use crate::apps::{self, MatchType, ResolvedMatch, RunningAppInfo};
use crate::ffi;
use crate::windows;
use beckon_core::{Backend, BackendError, BeckonAction, InstalledApp, Result, RunningApp};
use objc2_app_kit::NSWorkspace;

pub struct MacBackend;

impl MacBackend {
    pub fn new() -> Result<Self> {
        Ok(Self)
    }
}

impl Backend for MacBackend {
    fn beckon(&self, id: &str) -> Result<BeckonAction> {
        // Resolve to bundle id. Match by Name first (cross-OS portable),
        // bundle id second, installed-name fallback last (see apps::resolve).
        let resolved = apps::resolve(id);

        // Step 3: not running → launch
        let running_for_target: Vec<RunningAppInfo> = match &resolved {
            Some(m) => apps::all_running_for_bundle(&m.bundle_id),
            None => Vec::new(),
        };

        if running_for_target.is_empty() {
            let m = resolved.ok_or_else(|| BackendError::LaunchFailed {
                id: id.to_string(),
                reason: format!(
                    "no running app and no installed bundle matches `{}`. \
                     Run `beckon -L` to list installed apps, or `beckon -s {}` to search.",
                    id, id
                ),
            })?;
            launch_bundle(&m).map_err(|e| BackendError::LaunchFailed {
                id: id.to_string(),
                reason: e,
            })?;
            return Ok(BeckonAction::Launched);
        }

        // Pick the canonical running entry (first PID — `activate` and `hide`
        // act on a single process; multi-PID apps are rare and the first
        // entry matches what `NSWorkspace.frontmostApplication` would return).
        let target = &running_for_target[0];
        let target_pid = target.pid;

        // What's frontmost right now (before any action)?
        let frontmost_pid = frontmost_pid();
        let target_is_focused = frontmost_pid == Some(target_pid)
            || running_for_target.iter().any(|a| Some(a.pid) == frontmost_pid);

        // Step 4: running but not focused → activate
        if !target_is_focused {
            if !windows::activate_app(target) {
                return Err(BackendError::Other(format!(
                    "NSRunningApplication.activate returned false for pid {}",
                    target_pid
                )));
            }
            return Ok(BeckonAction::Focused);
        }

        // Step 5a: same app, more than one window → AX-cycle to the next.
        // `cycle_to_next_window` returns false if the app has ≤1 window OR
        // if AX permission is missing. We can't distinguish those reliably
        // from this side, so we fall through to 5b on false — which is a
        // sane degradation: with a single-window app, falling through is
        // exactly the right thing; without permission, the user sees
        // toggle-back instead of cycle, which still moves them somewhere
        // useful.
        if windows::cycle_to_next_window(target_pid) {
            return Ok(BeckonAction::Cycled);
        }

        // Step 5b: only one window of this app → toggle to most-recent other app.
        // CGWindowListCopyWindowInfo gives us the front-to-back stack; the
        // first PID that isn't us (or one of our siblings sharing the bundle)
        // is the app the user came from.
        let target_pids: std::collections::HashSet<i32> =
            running_for_target.iter().map(|a| a.pid).collect();
        let stack = windows::pid_stack_front_to_back();
        if let Some(other_pid) = stack.into_iter().find(|p| !target_pids.contains(p)) {
            if let Some(other) = running_app_for_pid(other_pid) {
                if windows::activate_app(&other) {
                    return Ok(BeckonAction::ToggledBack);
                }
            }
        }

        // Step 5c: nothing else → hide.
        if windows::hide_app(target) {
            return Ok(BeckonAction::Hidden);
        }
        Err(BackendError::Other(format!(
            "could not cycle, toggle, or hide pid {}",
            target_pid
        )))
    }

    fn list_running(&self) -> Result<Vec<RunningApp>> {
        let mut apps = apps::running_apps();
        apps.sort_by(|a, b| a.bundle_id.cmp(&b.bundle_id));

        // Group windows by bundle id via AX (best-effort — needs permission).
        Ok(apps
            .into_iter()
            .map(|a| {
                let window_count = ax_window_count(a.pid).unwrap_or(0);
                RunningApp {
                    id: a.bundle_id,
                    name: a.name,
                    window_count,
                }
            })
            .collect())
    }

    fn list_installed(&self) -> Result<Vec<InstalledApp>> {
        let mut apps = apps::installed_apps();
        apps.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(apps
            .into_iter()
            .map(|a| InstalledApp {
                id: a.bundle_id,
                name: a.name,
                exec: Some(a.bundle_path.display().to_string()),
            })
            .collect())
    }
}

/// PID of the currently-active app, or `None` if nothing is active (rare —
/// usually means the Finder is the implicit frontmost).
fn frontmost_pid() -> Option<i32> {
    let workspace = NSWorkspace::sharedWorkspace();
    let app = workspace.frontmostApplication()?;
    Some(app.processIdentifier())
}

fn running_app_for_pid(pid: i32) -> Option<RunningAppInfo> {
    apps::running_apps().into_iter().find(|a| a.pid == pid)
}

fn ax_window_count(pid: i32) -> Option<usize> {
    use core_foundation::array::CFArray;
    use core_foundation::base::TCFType;
    let app = crate::ffi::AxElement::for_pid(pid)?;
    let value = app.copy_attribute("AXWindows")?;
    let array_ref = value.as_concrete_TypeRef();
    let array: CFArray<core_foundation::base::CFType> =
        unsafe { CFArray::wrap_under_get_rule(array_ref as _) };
    Some(array.len() as usize)
}

/// Launch the resolved app. Shells out to `/usr/bin/open` rather than going
/// through `NSWorkspace.openApplicationAtURL:configuration:completionHandler:`
/// which is async-only on modern macOS and would force us to spin a run
/// loop just to get a sync result. `open` is a tiny native binary that
/// returns once the LaunchServices request is dispatched (~10-20ms).
///
/// We prefer `-b <bundle_id>` because LaunchServices already knows the
/// canonical app for a given bundle id; falling back to `-a <name>` matches
/// the user's typed Name when we have no bundle id (rare — resolution
/// usually gives us one).
fn launch_bundle(m: &ResolvedMatch) -> std::result::Result<(), String> {
    let mut cmd = std::process::Command::new("/usr/bin/open");
    cmd.arg("-b").arg(&m.bundle_id);
    let status = cmd.status().map_err(|e| format!("failed to spawn `open`: {}", e))?;
    if !status.success() {
        return Err(format!("`open -b {}` exited with {}", m.bundle_id, status));
    }
    Ok(())
}

/// `beckon -r <id>` report on macOS.
pub fn print_resolve_report(id: &str) -> Result<()> {
    let running = apps::running_apps();
    let resolved = apps::resolve(id);
    let subs = apps::name_substring_matches(id);

    let Some(m) = resolved else {
        println!("❌ no match for `{}`\n", id);
        if !subs.is_empty() {
            println!("Closest by name (substring):");
            for e in subs.iter().take(5) {
                println!("   {:<40} ({})", e.name, e.bundle_id);
            }
            println!();
        }
        let direct: Vec<&_> = running.iter().filter(|a| a.bundle_id == id).collect();
        if !direct.is_empty() {
            println!("Note: a running app has bundle id `{}` but no installed bundle matches.", id);
            println!("      Focus may work; launch will not.");
        }
        println!("Hint: `beckon -L` lists installed, `beckon -l` lists running.");
        return Ok(());
    };

    let running_match: Option<&_> = running.iter().find(|a| a.bundle_id == m.bundle_id);

    println!("✅ resolved");
    println!("   Input:        {}", id);
    println!("   Match type:   {}", m.match_type.describe());
    println!("   Name:         {}", m.display_name);
    println!("   Bundle id:    {}", m.bundle_id);
    if let Some(p) = &m.bundle_path {
        println!("   Bundle path:  {}", p.display());
    }
    match running_match {
        Some(app) => {
            let win_count = ax_window_count(app.pid).unwrap_or(0);
            println!("   Status:       running (pid {}, {} window{})", app.pid, win_count, if win_count == 1 { "" } else { "s" });
        }
        None => println!("   Status:       not running"),
    }

    // Ambiguity warning when there are multiple substring matches.
    let other_subs: Vec<&_> = subs
        .iter()
        .filter(|e| e.bundle_id != m.bundle_id)
        .collect();
    if !other_subs.is_empty() && matches!(m.match_type, MatchType::InstalledNameSubstring) {
        println!();
        println!(
            "⚠️  {} other entr{} also match by name substring:",
            other_subs.len(),
            if other_subs.len() == 1 { "y" } else { "ies" }
        );
        for e in other_subs.iter().take(5) {
            println!("       {:<40} ({})", e.name, e.bundle_id);
        }
        println!("   Hint: use the exact Name from `beckon -L` to disambiguate.");
    }

    if !ffi::ax_is_process_trusted() {
        println!();
        println!("⚠️  Accessibility permission not granted — window cycling (5a) will fall back");
        println!("    to toggle-back. Grant in System Settings → Privacy & Security → Accessibility,");
        println!("    or run `beckon -d` for the full check.");
    }
    Ok(())
}
