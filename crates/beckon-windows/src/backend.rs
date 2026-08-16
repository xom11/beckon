//! `WindowsBackend` — implementation of the `Backend` trait for Windows.

use crate::apps::{self, InstalledAppInfo, MatchType, ResolvedMatch};
use crate::window_ops::{self, WindowInfo};
use beckon_core::{Backend, BackendError, BeckonAction, InstalledApp, Result, RunningApp};
use std::collections::{HashMap, HashSet};
use windows::core::PCWSTR;
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED,
};
use windows::Win32::UI::Shell::{
    ApplicationActivationManager, IApplicationActivationManager, ShellExecuteW, AO_NONE,
};
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

pub struct WindowsBackend;

const FILE_EXPLORER_AUMID: &str = "Microsoft.Windows.Explorer";

impl Backend for WindowsBackend {
    fn beckon(&self, id: &str) -> Result<BeckonAction> {
        // A Start Menu shortcut's display name is its filename, so the common
        // case — a hotkey aimed at an app that has a Start Menu entry — is
        // settled by one directory walk plus a single `.lnk` parse, instead of
        // COM-parsing every shortcut on the machine. Run it alongside window
        // enumeration; the hot path needs both and neither depends on the other.
        let fast_id = id.to_string();
        let scan_handle = std::thread::spawn(move || apps::resolve_start_menu_by_name(&fast_id));
        let all_windows = window_ops::enum_visible_windows()
            .map_err(|e| BackendError::Other(format!("EnumWindows failed: {}", e)))?;
        let fg_hwnd = window_ops::get_foreground_hwnd();

        // Miss: fall back to the full catalog. The name tier has already been
        // ruled out above, so `resolve_lazy` is guaranteed to call its loader
        // here — which means the AppsFolder enumeration can be started eagerly
        // and overlapped with the Start Menu parse instead of running after it.
        // The closure just joins the thread that is already doing the work.
        let resolved = scan_handle.join().unwrap_or(None).or_else(|| {
            let shell_handle = std::thread::spawn(apps::scan_shell_apps);
            let start_menu = apps::scan_start_menu();
            apps::resolve_lazy(id, &start_menu, move || {
                shell_handle.join().unwrap_or_default()
            })
        });

        // Find running windows that match the target.
        let matching: Vec<&WindowInfo> = match &resolved {
            Some(m) => windows_for_resolved(m, &all_windows),
            None => windows_by_literal_id(id, &all_windows),
        };

        // Step 3: not running -> launch.
        if matching.is_empty() {
            let m = resolved.ok_or_else(|| BackendError::NoMatch {
                id: id.to_string(),
                hint: format!(
                    "no running window and no installed Windows app matches `{}`. \
                     Run `beckon installed` to list installed apps, or `beckon search {}` \
                     to search.",
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
            window_ops::focus_window(matching[0].hwnd)
                .map_err(|e| BackendError::Other(format!("focus_window: {}", e)))?;
            return Ok(BeckonAction::Focused);
        }

        // Step 5a: focused, multiple windows -> cycle to next.
        if matching.len() > 1 {
            let current_idx = matching.iter().position(|w| w.hwnd == fg_hwnd).unwrap_or(0);
            let next_idx = (current_idx + 1) % matching.len();
            window_ops::focus_window(matching[next_idx].hwnd)
                .map_err(|e| BackendError::Other(format!("cycle: {}", e)))?;
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
            window_ops::focus_window(other.hwnd)
                .map_err(|e| BackendError::Other(format!("toggle-back: {}", e)))?;
            return Ok(BeckonAction::ToggledBack);
        }

        // Step 5c: nothing else -> minimize.
        window_ops::minimize_window(fg_hwnd)
            .map_err(|e| BackendError::Other(format!("minimize: {}", e)))?;
        Ok(BeckonAction::Hidden)
    }

    fn list_running(&self) -> Result<Vec<RunningApp>> {
        let windows = window_ops::enum_visible_windows()
            .map_err(|e| BackendError::Other(format!("EnumWindows: {}", e)))?;

        // Group packaged apps by AUMID and classic apps by exe name.
        // When multiple classic windows share one exe (e.g. browser PWAs),
        // list each title separately.
        let mut groups: HashMap<String, (String, usize)> = HashMap::new();
        let mut id_count: HashMap<String, usize> = HashMap::new();
        for w in &windows {
            let id = w.aumid.as_ref().unwrap_or(&w.exe_name);
            *id_count.entry(id.clone()).or_default() += 1;
        }
        for w in &windows {
            let id = w.aumid.as_ref().unwrap_or(&w.exe_name);
            let key = if w.aumid.is_none() && id_count.get(id).copied().unwrap_or(0) > 1 {
                // Shared exe — use title as the identity so each PWA shows up.
                format!("{}|{}", id, w.title)
            } else {
                id.clone()
            };
            let entry = groups.entry(key).or_insert_with(|| (w.title.clone(), 0));
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
        let apps = apps::scan_installed_apps();
        Ok(apps
            .into_iter()
            .map(|a| InstalledApp {
                id: a.aumid.clone().unwrap_or_else(|| a.exe_name.clone()),
                name: a.name,
                exec: Some(
                    a.aumid
                        .map_or(a.exe_path, |id| format!("AppUserModelID:{}", id)),
                ),
            })
            .collect())
    }
}

/// Find running windows matching a resolved installed app.
///
/// Four-tier matching:
///   1. AUMID     — reliable packaged-app identity.
///   2. Exe-only  — works for regular apps with unique exe names.
///   3. Exe+title — when multiple windows share the same exe (PWAs via
///      `chrome_proxy.exe` or `brave.exe`), narrows by title containing
///      the app name.
///   4. Title-only — when the .lnk target is a launcher stub that doesn't
///      stay running (e.g. `chrome_proxy.exe` launches `brave.exe`), falls
///      back to title match against all windows.
///
/// Chromium PWAs expose AUMIDs like `Vivaldi._crx_<id>`. A plain browser
/// shortcut such as `Vivaldi.lnk` must not match those PWA windows just
/// because they share the same browser executable.
fn windows_for_resolved<'a>(
    resolved: &ResolvedMatch,
    windows: &'a [WindowInfo],
) -> Vec<&'a WindowInfo> {
    if let Some(aumid) = &resolved.aumid {
        let by_aumid: Vec<&WindowInfo> = windows
            .iter()
            .filter(|w| {
                w.aumid
                    .as_deref()
                    .is_some_and(|id| id.eq_ignore_ascii_case(aumid))
            })
            .collect();
        if !by_aumid.is_empty() {
            return by_aumid;
        }
    }

    let by_exe: Vec<&WindowInfo> = if resolved.exe_name.is_empty() {
        Vec::new()
    } else {
        let exclude_chromium_pwas = should_exclude_chromium_pwa_windows(resolved);
        windows
            .iter()
            .filter(|w| {
                w.exe_name == resolved.exe_name
                    && !(exclude_chromium_pwas && is_chromium_pwa_window(w))
            })
            .collect()
    };

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
    let exclude_chromium_pwas = should_exclude_chromium_pwa_windows(resolved);
    windows
        .iter()
        .filter(|w| {
            apps::normalize(&w.title).contains(&name_lower)
                && !(exclude_chromium_pwas && is_chromium_pwa_window(w))
        })
        .collect()
}

fn should_exclude_chromium_pwa_windows(resolved: &ResolvedMatch) -> bool {
    resolved.aumid.is_none() && !is_chromium_pwa_shortcut(&resolved.arguments)
}

fn is_chromium_pwa_shortcut(arguments: &str) -> bool {
    let args = arguments.to_ascii_lowercase();
    args.contains("--app=") || args.contains("--app-id=")
}

fn is_chromium_pwa_window(window: &WindowInfo) -> bool {
    window
        .aumid
        .as_deref()
        .is_some_and(|aumid| aumid.to_ascii_lowercase().contains("._crx_"))
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

    let by_aumid: Vec<&WindowInfo> = windows
        .iter()
        .filter(|w| {
            w.aumid
                .as_deref()
                .is_some_and(|v| v.eq_ignore_ascii_case(id))
        })
        .collect();
    if !by_aumid.is_empty() {
        return by_aumid;
    }

    // Prefer exe name match over title match.
    let by_exe: Vec<&WindowInfo> = windows.iter().filter(|w| w.exe_name == with_exe).collect();
    if !by_exe.is_empty() {
        return by_exe;
    }

    // Fall back to title substring.
    windows
        .iter()
        .filter(|w| w.title.to_lowercase().contains(&lower))
        .collect()
}

/// Launch a classic app via `ShellExecuteW` or a packaged app via AUMID.
fn launch(m: &ResolvedMatch) -> std::result::Result<(), String> {
    if let Some(aumid) = &m.aumid {
        if aumid.eq_ignore_ascii_case(FILE_EXPLORER_AUMID) {
            return shell_execute("explorer.exe", "");
        }
        return launch_appx(aumid);
    }

    shell_execute(&m.exe_path, &m.arguments)
}

fn shell_execute(exe: &str, arguments: &str) -> std::result::Result<(), String> {
    let wide_exe = to_wide(exe);
    let wide_args = to_wide(arguments);
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
                ret.0 as usize, exe
            ));
        }
    }
    Ok(())
}

fn launch_appx(aumid: &str) -> std::result::Result<(), String> {
    let wide_aumid = to_wide(aumid);
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let activator: IApplicationActivationManager =
            CoCreateInstance(&ApplicationActivationManager, None, CLSCTX_INPROC_SERVER)
                .map_err(|e| format!("create AppX activation manager: {}", e))?;
        activator
            .ActivateApplication(PCWSTR(wide_aumid.as_ptr()), PCWSTR::null(), AO_NONE)
            .map_err(|e| format!("activate AppX `{}`: {}", aumid, e))?;
    }
    Ok(())
}

fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// `beckon resolve <id>` report on Windows.
pub fn print_resolve_report(id: &str) -> Result<()> {
    let installed = apps::scan_installed_apps();
    let resolved = apps::resolve(id, &installed);
    let subs = apps::name_substring_matches(id, &installed);
    let all_windows = window_ops::enum_visible_windows()
        .map_err(|e| BackendError::Other(format!("EnumWindows: {}", e)))?;

    let Some(m) = resolved else {
        println!("  no match for `{}`\n", id);

        // Check if there's a running window that matches by title/exe.
        let running = windows_by_literal_id(id, &all_windows);
        if !running.is_empty() {
            println!(
                "Note: {} running window(s) match by exe/title but no installed app found.",
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
        println!("Hint: `beckon installed` lists installed, `beckon list` lists running.");
        return Ok(());
    };

    let win_count = windows_for_resolved(&m, &all_windows).len();

    println!("  resolved");
    println!("   Input:        {}", id);
    println!("   Match type:   {}", m.match_type.describe());
    println!("   Name:         {}", m.name);
    if let Some(aumid) = &m.aumid {
        println!("   AUMID:        {}", aumid);
        if aumid.eq_ignore_ascii_case(FILE_EXPLORER_AUMID) {
            println!("   Launch:       explorer.exe");
        } else {
            println!("   Launch:       IApplicationActivationManager");
        }
    } else {
        println!("   Exe:          {}", m.exe_path);
    }
    if !m.arguments.is_empty() {
        println!("   Arguments:    {}", m.arguments);
    }
    if !m.shortcut_path.as_os_str().is_empty() {
        println!("   Shortcut:     {}", m.shortcut_path.display());
    }
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
        println!("   Hint: use the exact Name from `beckon installed` to disambiguate.");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use windows::Win32::Foundation::HWND;

    #[test]
    fn packaged_window_matches_aumid_without_exe_or_title_match() {
        let resolved = ResolvedMatch {
            name: "Terminal".to_string(),
            exe_path: String::new(),
            exe_name: String::new(),
            arguments: String::new(),
            shortcut_path: PathBuf::new(),
            aumid: Some("Microsoft.WindowsTerminal_8wekyb3d8bbwe!App".to_string()),
            match_type: MatchType::InstalledName,
        };
        let windows = vec![WindowInfo {
            hwnd: HWND::default(),
            pid: 1,
            title: "shell prompt".to_string(),
            class_name: "CASCADIA_HOSTING_WINDOW_CLASS".to_string(),
            exe_path: String::new(),
            exe_name: "applicationframehost.exe".to_string(),
            aumid: Some("Microsoft.WindowsTerminal_8wekyb3d8bbwe!App".to_string()),
        }];

        assert_eq!(windows_for_resolved(&resolved, &windows).len(), 1);
    }

    #[test]
    fn file_explorer_matches_only_its_aumid_window() {
        let resolved = ResolvedMatch {
            name: "File Explorer".to_string(),
            exe_path: String::new(),
            exe_name: String::new(),
            arguments: String::new(),
            shortcut_path: PathBuf::new(),
            aumid: Some("Microsoft.Windows.Explorer".to_string()),
            match_type: MatchType::InstalledName,
        };
        let windows = vec![
            WindowInfo {
                hwnd: HWND::default(),
                pid: 1,
                title: "Documents".to_string(),
                class_name: "CabinetWClass".to_string(),
                exe_path: String::new(),
                exe_name: "explorer.exe".to_string(),
                aumid: Some("Microsoft.Windows.Explorer".to_string()),
            },
            WindowInfo {
                hwnd: HWND::default(),
                pid: 1,
                title: "Desktop".to_string(),
                class_name: "Progman".to_string(),
                exe_path: String::new(),
                exe_name: "explorer.exe".to_string(),
                aumid: None,
            },
        ];

        assert_eq!(windows_for_resolved(&resolved, &windows).len(), 1);
    }

    #[test]
    fn classic_browser_shortcut_does_not_match_chromium_pwa_window_by_exe() {
        let resolved = ResolvedMatch {
            name: "Vivaldi".to_string(),
            exe_path: "C:\\Users\\test\\AppData\\Local\\Vivaldi\\Application\\vivaldi.exe"
                .to_string(),
            exe_name: "vivaldi.exe".to_string(),
            arguments: String::new(),
            shortcut_path: PathBuf::from(
                "C:\\Users\\test\\AppData\\Roaming\\Microsoft\\Windows\\Start Menu\\Programs\\Vivaldi.lnk",
            ),
            aumid: None,
            match_type: MatchType::InstalledName,
        };
        let windows = vec![WindowInfo {
            hwnd: HWND::default(),
            pid: 1,
            title: "Google Gemini - Google Gemini".to_string(),
            class_name: "Chrome_WidgetWin_1".to_string(),
            exe_path: "C:\\Users\\test\\AppData\\Local\\Vivaldi\\Application\\vivaldi.exe"
                .to_string(),
            exe_name: "vivaldi.exe".to_string(),
            aumid: Some("Vivaldi._crx_caidcmannjpmidmiecjcoiiigg".to_string()),
        }];

        assert!(windows_for_resolved(&resolved, &windows).is_empty());
    }
}
