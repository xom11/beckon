//! `WindowsBackend` — implementation of the `Backend` trait for Windows.

use crate::apps::{self, InstalledAppInfo, MatchType, ResolvedMatch};
use crate::window_ops::{self, WindowInfo};
use beckon_core::{Backend, BackendError, BeckonAction, InstalledApp, Result, RunningApp};
use std::collections::{HashMap, HashSet};
use windows::core::PCWSTR;
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

pub struct WindowsBackend;

impl Backend for WindowsBackend {
    fn beckon(&self, id: &str) -> Result<BeckonAction> {
        // scan_start_menu walks .lnk files via COM (~50–100ms on busy
        // installs); enum_visible_windows just iterates HWNDs. Run them
        // in parallel — they're independent and the hot path hits both.
        let scan_handle = std::thread::spawn(apps::scan_start_menu);
        let all_windows = window_ops::enum_visible_windows().map_err(|e| {
            BackendError::Other(format!("EnumWindows failed: {}", e))
        })?;
        let fg_hwnd = window_ops::get_foreground_hwnd();
        let installed = scan_handle.join().unwrap_or_default();

        // Resolve id against installed apps.
        let resolved = apps::resolve(id, &installed);

        // Find running windows that match the target.
        let matching: Vec<&WindowInfo> = match &resolved {
            Some(m) => windows_for_resolved(m, &all_windows),
            None => windows_by_literal_id(id, &all_windows),
        };

        // Step 3: not running -> launch.
        if matching.is_empty() {
            let m = resolved.ok_or_else(|| BackendError::LaunchFailed {
                id: id.to_string(),
                reason: format!(
                    "no running window and no Start Menu shortcut matches `{}`. \
                     Run `beckon -L` to list installed apps, or `beckon -s {}` to search.",
                    id, id
                ),
            })?;
            launch(&m).map_err(|e| BackendError::LaunchFailed {
                id: id.to_string(),
                reason: e,
            })?;
            return Ok(BeckonAction::Launched);
        }

        // Is the current foreground window one of ours?
        let fg_is_target = matching.iter().any(|w| w.hwnd == fg_hwnd);

        // Step 4: running but not focused -> focus.
        if !fg_is_target {
            window_ops::focus_window(matching[0].hwnd).map_err(|e| {
                BackendError::Other(format!("focus_window: {}", e))
            })?;
            return Ok(BeckonAction::Focused);
        }

        // Step 5a: focused, multiple windows -> cycle to next.
        if matching.len() > 1 {
            let current_idx = matching
                .iter()
                .position(|w| w.hwnd == fg_hwnd)
                .unwrap_or(0);
            let next_idx = (current_idx + 1) % matching.len();
            window_ops::focus_window(matching[next_idx].hwnd).map_err(|e| {
                BackendError::Other(format!("cycle: {}", e))
            })?;
            return Ok(BeckonAction::Cycled);
        }

        // Step 5b: single window -> toggle to most-recent OTHER app.
        // `all_windows` is in z-order (front-to-back); first window NOT in our
        // matching set is the most recently used other app. Using HWND set
        // (not exe name) so PWAs sharing chrome_proxy.exe toggle correctly.
        let matching_hwnds: HashSet<isize> = matching.iter().map(|w| w.hwnd.0 as isize).collect();
        if let Some(other) = all_windows
            .iter()
            .find(|w| !matching_hwnds.contains(&(w.hwnd.0 as isize)))
        {
            window_ops::focus_window(other.hwnd).map_err(|e| {
                BackendError::Other(format!("toggle-back: {}", e))
            })?;
            return Ok(BeckonAction::ToggledBack);
        }

        // Step 5c: nothing else -> minimize.
        window_ops::minimize_window(fg_hwnd).map_err(|e| {
            BackendError::Other(format!("minimize: {}", e))
        })?;
        Ok(BeckonAction::Hidden)
    }

    fn list_running(&self) -> Result<Vec<RunningApp>> {
        let windows = window_ops::enum_visible_windows().map_err(|e| {
            BackendError::Other(format!("EnumWindows: {}", e))
        })?;

        // Group by exe name. When multiple windows share the same exe
        // (e.g. PWAs via chrome_proxy.exe), list each title separately.
        let mut groups: HashMap<String, (String, usize)> = HashMap::new();
        let mut exe_count: HashMap<String, usize> = HashMap::new();
        for w in &windows {
            *exe_count.entry(w.exe_name.clone()).or_default() += 1;
        }
        for w in &windows {
            let key = if exe_count.get(&w.exe_name).copied().unwrap_or(0) > 1 {
                // Shared exe — use title as the identity so each PWA shows up.
                format!("{}|{}", w.exe_name, w.title)
            } else {
                w.exe_name.clone()
            };
            let entry = groups
                .entry(key)
                .or_insert_with(|| (w.title.clone(), 0));
            entry.1 += 1;
        }

        let mut apps: Vec<RunningApp> = groups
            .into_iter()
            .map(|(key, (title, count))| {
                let id = key.split('|').next().unwrap_or(&key).to_string();
                RunningApp {
                    id,
                    name: title,
                    window_count: count,
                }
            })
            .collect();
        apps.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(apps)
    }

    fn list_installed(&self) -> Result<Vec<InstalledApp>> {
        let apps = apps::scan_start_menu();
        Ok(apps
            .into_iter()
            .map(|a| InstalledApp {
                id: a.exe_name.clone(),
                name: a.name,
                exec: Some(a.exe_path),
            })
            .collect())
    }
}

/// Find running windows matching a resolved Start Menu app.
///
/// Three-tier matching:
///   1. Exe-only  — works for regular apps with unique exe names.
///   2. Exe+title — when multiple windows share the same exe (PWAs via
///      `chrome_proxy.exe` or `brave.exe`), narrows by title containing
///      the app name.
///   3. Title-only — when the .lnk target is a launcher stub that doesn't
///      stay running (e.g. `chrome_proxy.exe` launches `brave.exe`), falls
///      back to title match against all windows.
fn windows_for_resolved<'a>(
    resolved: &ResolvedMatch,
    windows: &'a [WindowInfo],
) -> Vec<&'a WindowInfo> {
    let by_exe: Vec<&WindowInfo> = windows
        .iter()
        .filter(|w| w.exe_name == resolved.exe_name)
        .collect();

    // Tier 2: narrow by title when multiple windows share this exe.
    if by_exe.len() > 1 {
        let name_lower = apps::normalize(&resolved.name);
        let by_title: Vec<&WindowInfo> = by_exe
            .iter()
            .filter(|w| apps::normalize(&w.title).contains(&name_lower))
            .copied()
            .collect();
        if !by_title.is_empty() {
            return by_title;
        }
    }

    if !by_exe.is_empty() {
        return by_exe;
    }

    // Tier 3: exe matched nothing — the .lnk target is likely a launcher
    // stub (e.g. chrome_proxy.exe → brave.exe). Fall back to title match.
    let name_lower = apps::normalize(&resolved.name);
    windows
        .iter()
        .filter(|w| apps::normalize(&w.title).contains(&name_lower))
        .collect()
}

/// Fallback: match by literal id against exe name or window title.
/// Used when no Start Menu shortcut matched.
fn windows_by_literal_id<'a>(id: &str, windows: &'a [WindowInfo]) -> Vec<&'a WindowInfo> {
    let lower = id.to_lowercase();
    let with_exe = if lower.ends_with(".exe") {
        lower.clone()
    } else {
        format!("{}.exe", lower)
    };

    // Prefer exe name match over title match.
    let by_exe: Vec<&WindowInfo> = windows
        .iter()
        .filter(|w| w.exe_name == with_exe)
        .collect();
    if !by_exe.is_empty() {
        return by_exe;
    }

    // Fall back to title substring.
    windows
        .iter()
        .filter(|w| w.title.to_lowercase().contains(&lower))
        .collect()
}

/// Launch an app via `ShellExecuteW` using the shortcut's target.
fn launch(m: &ResolvedMatch) -> std::result::Result<(), String> {
    let wide_exe = to_wide(&m.exe_path);
    let wide_args = to_wide(&m.arguments);
    let wide_verb = to_wide("open");

    unsafe {
        let ret = ShellExecuteW(
            None,
            PCWSTR(wide_verb.as_ptr()),
            PCWSTR(wide_exe.as_ptr()),
            PCWSTR(wide_args.as_ptr()),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        );
        // ShellExecuteW returns HINSTANCE; values > 32 mean success.
        if ret.0 as usize <= 32 {
            return Err(format!(
                "ShellExecuteW returned {} for `{}`",
                ret.0 as usize, m.exe_path
            ));
        }
    }
    Ok(())
}

fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// `beckon -r <id>` report on Windows.
pub fn print_resolve_report(id: &str) -> Result<()> {
    let installed = apps::scan_start_menu();
    let resolved = apps::resolve(id, &installed);
    let subs = apps::name_substring_matches(id, &installed);
    let all_windows = window_ops::enum_visible_windows().map_err(|e| {
        BackendError::Other(format!("EnumWindows: {}", e))
    })?;

    let Some(m) = resolved else {
        println!("  no match for `{}`\n", id);

        // Check if there's a running window that matches by title/exe.
        let running = windows_by_literal_id(id, &all_windows);
        if !running.is_empty() {
            println!(
                "Note: {} running window(s) match by exe/title but no Start Menu shortcut found.",
                running.len()
            );
            println!("      Focus will work; launch will not.");
            println!();
        }

        if !subs.is_empty() {
            println!("Closest by name (substring):");
            for e in subs.iter().take(5) {
                println!("   {:<40} ({})", e.name, e.exe_name);
            }
            println!();
        }
        println!("Hint: `beckon -L` lists installed, `beckon -l` lists running.");
        return Ok(());
    };

    // Count windows matching this exe.
    let win_count = all_windows
        .iter()
        .filter(|w| w.exe_name == m.exe_name)
        .count();

    println!("  resolved");
    println!("   Input:        {}", id);
    println!("   Match type:   {}", m.match_type.describe());
    println!("   Name:         {}", m.name);
    println!("   Exe:          {}", m.exe_path);
    if !m.arguments.is_empty() {
        println!("   Arguments:    {}", m.arguments);
    }
    println!("   Shortcut:     {}", m.shortcut_path.display());
    if win_count > 0 {
        println!(
            "   Status:       running ({} window{})",
            win_count,
            if win_count == 1 { "" } else { "s" }
        );
    } else {
        println!("   Status:       not running");
    }

    // Ambiguity warning.
    let other_subs: Vec<&InstalledAppInfo> = subs
        .iter()
        .filter(|e| apps::normalize(&e.name) != apps::normalize(&m.name))
        .collect();
    if !other_subs.is_empty() && matches!(m.match_type, MatchType::InstalledNameSubstring) {
        println!();
        println!(
            "   {} other entr{} also match by name substring:",
            other_subs.len(),
            if other_subs.len() == 1 { "y" } else { "ies" }
        );
        for e in other_subs.iter().take(5) {
            println!("       {:<40} ({})", e.name, e.exe_name);
        }
        println!("   Hint: use the exact Name from `beckon -L` to disambiguate.");
    }
    Ok(())
}
