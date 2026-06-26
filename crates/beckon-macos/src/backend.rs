//! `MacBackend` — implementation of the `Backend` trait for macOS.

use crate::apps::{self, MatchType, ResolvedMatch, RunningAppInfo};
use crate::ffi;
use crate::state;
use crate::windows;
use beckon_core::{Backend, BackendError, BeckonAction, InstalledApp, Result, RunningApp};
use objc2_app_kit::NSWorkspace;

pub struct MacBackend;

impl MacBackend {
    pub fn new() -> Result<Self> {
        Ok(Self)
    }
}

/// Step 5b MRU pick: should we toggle back to the recorded `previous` app?
/// Returns its bundle id when `previous` names a currently-running app that
/// isn't the target; `None` to fall through to the z-order stack.
///
/// Pure (the running check is injected) so it can be unit-tested without
/// NSWorkspace. The guard against `previous == target` mirrors the Linux
/// algorithm: after a toggle-back we persist the target as "previous", and we
/// must never toggle an app onto itself.
fn pick_mru_toggle_back(
    previous: Option<&str>,
    target_bundle: &str,
    is_running: impl Fn(&str) -> bool,
) -> Option<String> {
    let prev = previous?;
    if prev == target_bundle || !is_running(prev) {
        return None;
    }
    Some(prev.to_string())
}

impl Backend for MacBackend {
    fn beckon(&self, id: &str) -> Result<BeckonAction> {
        // Snapshot running apps once and reuse it for resolve, the target
        // filter, and the MRU lookup — saves repeated
        // NSWorkspace.runningApplications round-trips on the hot path.
        let running = apps::running_apps();

        // Bundle id of whatever is frontmost right now, before we touch focus.
        // This becomes "previous" for the next invocation's toggle-back. We
        // read it from `frontmostApplication` (Space-independent) so a
        // fullscreen app on another Space is still captured correctly.
        let pre_frontmost = frontmost_pid()
            .and_then(|p| running.iter().find(|a| a.pid == p))
            .map(|a| a.bundle_id.clone());

        let action = self.beckon_inner(id, &running);

        // Persist the app we came from on every successful action. Best-effort:
        // a missing MRU only degrades toggle-back, never fails the hot path.
        if action.is_ok() {
            if let Some(prev) = pre_frontmost {
                state::write_previous(&prev);
            }
        }
        action
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

impl MacBackend {
    /// Core algorithm. Takes the pre-snapshotted `running` list so the outer
    /// `beckon` can also use it for MRU bookkeeping without re-querying.
    fn beckon_inner(&self, id: &str, running: &[RunningAppInfo]) -> Result<BeckonAction> {
        // Resolve to bundle id. Match by Name first (cross-OS portable),
        // bundle id second, installed-name fallback last (see apps::resolve).
        let resolved = apps::resolve_with_running(id, running);

        // Step 3: not running → launch
        let running_for_target: Vec<RunningAppInfo> = match &resolved {
            Some(m) => running
                .iter()
                .filter(|a| a.bundle_id == m.bundle_id)
                .cloned()
                .collect(),
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
            || running_for_target
                .iter()
                .any(|a| Some(a.pid) == frontmost_pid);

        // Step 4: running but not focused → bring it forward.
        //
        // Use the reopen path (`open -b`, the Dock-icon Apple Event) rather
        // than NSRunningApplication.activateWithOptions. The window server
        // silently drops activateWithOptions when the requesting process is not
        // itself frontmost — which is exactly beckon's situation when it is
        // spawned from a hotkey daemon (Hammerspoon `hs.task`) — so it focuses
        // only intermittently. `open -b` is honoured unconditionally and, as a
        // bonus, un-minimizes an existing window or spawns a fresh one when the
        // app has none to show (Chromium-family browsers keep running after
        // their last window is closed). Fall back to activate for apps
        // LaunchServices can't resolve by bundle id (ad-hoc CLI binaries with
        // no registered bundle).
        if !target_is_focused {
            if open_bundle_id(&target.bundle_id).is_ok() {
                return Ok(BeckonAction::Focused);
            }
            if !windows::activate_app(target) {
                return Err(BackendError::Other(format!(
                    "open -b and NSRunningApplication.activate both failed for pid {}",
                    target_pid
                )));
            }
            return Ok(BeckonAction::Focused);
        }

        // The target IS frontmost, but it may have no window on screen — every
        // window minimized, or running windowless. The cycle / toggle / hide
        // steps below all assume there is a visible window to act on, so reopen
        // first to surface one (un-minimize or spawn). Guarded on AX trust + a
        // zero visible-window count; without AX we can't tell an empty window
        // list from a permission error, so we leave the focused-but-blank case
        // alone rather than risk a spurious reopen.
        if ffi::ax_is_process_trusted()
            && windows::visible_standard_window_count(target_pid).unwrap_or(0) == 0
        {
            if open_bundle_id(&target.bundle_id).is_ok() {
                return Ok(BeckonAction::Focused);
            }
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
        if beckon_core::verbose() {
            eprintln!(
                "verbose: cycle_to_next_window returned false for pid {} \
                 (single-window app, OR Accessibility permission missing — \
                 run `beckon -d` to check)",
                target_pid
            );
        }

        // Step 5b: only one window of this app → toggle to most-recent other app.
        //
        // Prefer the MRU "previous" app recorded by the last invocation. This
        // is the only path that can land on a natively-fullscreen app: such an
        // app lives on its own Space and so is *absent* from the on-screen
        // z-order stack below, but `frontmostApplication` saw it when we
        // recorded it, and `running` (NSWorkspace) lists it regardless of
        // Space. Without this, toggle-back skips the fullscreen app and lands
        // on whatever else is visible.
        if let Some(prev_bundle) = pick_mru_toggle_back(state::read_previous().as_deref(), &target.bundle_id, |b| running.iter().any(|a| a.bundle_id == b)) {
            if let Some(other) = running.iter().find(|a| a.bundle_id == prev_bundle) {
                if windows::activate_app(other) {
                    return Ok(BeckonAction::ToggledBack);
                }
            }
        }

        // Fallback: no usable MRU (first run, or the previous app quit).
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
    // Count only standard windows — the same set step 5a cycles — so the
    // reported count never includes fullscreen/PWA helper windows.
    windows::standard_window_count(pid)
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
    open_bundle_id(&m.bundle_id)
}

/// Run `open -b <bundle_id>`. For a not-running app this launches it; for an
/// already-running one it sends the reopen Apple Event (identical to clicking
/// the Dock icon), which un-minimizes an existing window or spawns a fresh one
/// and activates the app — neither of which `activateWithOptions` does. Step 4
/// falls back to this when the target is running but has no visible window.
fn open_bundle_id(bundle_id: &str) -> std::result::Result<(), String> {
    let status = std::process::Command::new("/usr/bin/open")
        .arg("-b")
        .arg(bundle_id)
        .status()
        .map_err(|e| format!("failed to spawn `open`: {}", e))?;
    if !status.success() {
        return Err(format!("`open -b {}` exited with {}", bundle_id, status));
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
            println!(
                "Note: a running app has bundle id `{}` but no installed bundle matches.",
                id
            );
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
            println!(
                "   Status:       running (pid {}, {} window{})",
                app.pid,
                win_count,
                if win_count == 1 { "" } else { "s" }
            );
        }
        None => println!("   Status:       not running"),
    }

    // Ambiguity warning when there are multiple substring matches.
    let other_subs: Vec<&_> = subs.iter().filter(|e| e.bundle_id != m.bundle_id).collect();
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
        println!(
            "    to toggle-back. Grant in System Settings → Privacy & Security → Accessibility,"
        );
        println!("    or run `beckon -d` for the full check.");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::pick_mru_toggle_back;

    #[test]
    fn returns_previous_when_running_and_not_target() {
        let running = ["com.apple.Safari", "com.x.kitty"];
        let pick = pick_mru_toggle_back(Some("com.apple.Safari"), "com.x.kitty", |b| {
            running.contains(&b)
        });
        assert_eq!(pick.as_deref(), Some("com.apple.Safari"));
    }

    #[test]
    fn none_when_previous_is_target() {
        // After a toggle-back we persist the target as "previous"; never
        // toggle an app onto itself — fall through to the z-order stack.
        let pick = pick_mru_toggle_back(Some("com.x.kitty"), "com.x.kitty", |_| true);
        assert!(pick.is_none());
    }

    #[test]
    fn none_when_previous_not_running() {
        // The previously-recorded app has since quit.
        let pick = pick_mru_toggle_back(Some("com.gone.app"), "com.x.kitty", |_| false);
        assert!(pick.is_none());
    }

    #[test]
    fn none_when_no_previous_recorded() {
        let pick = pick_mru_toggle_back(None, "com.x.kitty", |_| true);
        assert!(pick.is_none());
    }

    #[test]
    fn fullscreen_app_on_another_space_is_still_a_valid_previous() {
        // The regression this fixes: a fullscreen app is absent from the
        // on-screen z-order stack, but it is still in `running` (NSWorkspace
        // is Space-independent) and was recorded as frontmost, so the MRU
        // pick must surface it.
        let running = ["com.google.Chrome", "com.youtube.pwa"];
        let pick = pick_mru_toggle_back(Some("com.youtube.pwa"), "com.google.Chrome", |b| {
            running.contains(&b)
        });
        assert_eq!(pick.as_deref(), Some("com.youtube.pwa"));
    }
}
