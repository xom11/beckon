//! `serve`: resident hotkey service (macOS, Windows). Single-threaded by
//! design: hotkey dispatch and reload ticks all run on the main event loop,
//! so plain `Rc<RefCell<…>>` state needs no locking — with one exception,
//! enforced below.
//!
//! Reentrancy differs by platform:
//!   - macOS (Carbon): the hotkey callback runs on the main CFRunLoop and
//!     never re-enters — it must not pump the run loop itself, and nothing
//!     else does either, so a callback in flight is never interrupted by
//!     another tick or another hotkey.
//!   - Windows: `backend.beckon()` can pump this thread's message queue by
//!     design (CoInitializeEx puts it in an STA, and an out-of-process COM
//!     activation call pumps internally to avoid an RPC deadlock — see
//!     `beckon_windows::hotkey`'s module doc). That means the 1 Hz reload
//!     tick — and therefore `reload()` — can run RE-ENTRANTLY while
//!     `on_hotkey` is still on the stack.
//!
//! The invariant this file enforces because of the Windows case: **no
//! `RefCell` borrow may be held across a call to `backend.beckon()` or
//! `shell::open_path`.** The backend therefore lives OUTSIDE the `RefCell`
//! (a separate `Rc`, cloned into the hotkey closure), and `on_hotkey` takes
//! only a short borrow to clone what it needs, releasing it before calling
//! the backend. Holding `state.borrow()` across that call would let a
//! reentrant `reload()` call `state.borrow_mut()` and panic
//! (`BorrowMutError`) while unwinding across an `extern "system"` boundary —
//! which aborts the whole process, not just the callback.

use anyhow::{anyhow, Context, Result};
#[cfg(any(target_os = "macos", test))]
use beckon_core::certainty::NameReport;
use beckon_core::menu::MenuEntry;
use beckon_core::shortcuts::{parse_config, KeyboardConfig, Shortcut};
use beckon_core::Backend;
#[cfg(target_os = "macos")]
use beckon_macos::hotkey;
#[cfg(target_os = "windows")]
use beckon_windows::hotkey;
use hotkey::HotkeyManager;
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

/// Publish a one-line status where the user can see it without reading the
/// log: the tray tooltip on Windows, the menu bar item's tooltip on macOS.
#[cfg(target_os = "windows")]
fn set_tray_status(text: &str) {
    hotkey::set_status(text);
}
#[cfg(target_os = "macos")]
fn set_tray_status(text: &str) {
    beckon_macos::tray::set_status(text);
}
#[cfg(not(any(target_os = "windows", target_os = "macos")))]
fn set_tray_status(_text: &str) {}

#[cfg(target_os = "macos")]
fn set_tray_dimmed(dimmed: bool) {
    beckon_macos::tray::set_dimmed(dimmed);
}
#[cfg(not(target_os = "macos"))]
fn set_tray_dimmed(_dimmed: bool) {}

/// Capability + values needed to offer "Start with Windows", present only
/// when this process is one whose own path is safe to bake into the Run
/// key (`beckon-serve.exe`).
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub struct AutostartCapability {
    pub config: Option<PathBuf>,
    pub log: Option<PathBuf>,
}

/// What a front door does when the config file reads but does not parse.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrokenConfig {
    Refuse,
    #[cfg_attr(not(any(target_os = "windows", target_os = "macos")), allow(dead_code))]
    ServeAnyway,
}

/// What `cmd_serve_app` starts with.
struct Startup {
    shortcuts: Vec<Shortcut>,
    keyboard: KeyboardConfig,
    tray: bool,
    broken: Option<String>,
}

/// Read the config text into a startup plan, or refuse.
///
/// The `ServeAnyway` arm discards the parsed keyboard block along with the
/// shortcuts and forces `tray: true`, so a broken file never installs a
/// low-level keyboard hook and always keeps the tray icon reachable to open
/// and repair the config.
fn plan_startup(text: &str, on_broken: BrokenConfig) -> Result<Startup, String> {
    match parse_config(text) {
        Ok(cfg) => Ok(Startup {
            shortcuts: cfg.shortcuts,
            keyboard: cfg.keyboard,
            tray: cfg.tray,
            broken: None,
        }),
        Err(e) => match on_broken {
            BrokenConfig::Refuse => Err(e),
            BrokenConfig::ServeAnyway => Ok(Startup {
                shortcuts: Vec::new(),
                keyboard: KeyboardConfig::default(),
                tray: true,
                broken: Some(e),
            }),
        },
    }
}

/// The status phrase while the config file does not parse. ASCII, like every
/// `serve` log line: Windows PowerShell 5.1's `Get-Content` defaults to ANSI.
fn unreadable_phrase() -> String {
    format!("cannot read the config ({})", registration_phrase(0, 0))
}

struct ServeState {
    shortcuts: Vec<Shortcut>,
    keyboard: KeyboardConfig,
    tray: bool,
    config: PathBuf,
    paused: bool,
    /// The flock that says this process owns Caps on this machine, held for
    /// exactly as long as the tap is installed.
    #[cfg(target_os = "macos")]
    caps_owner: Option<std::fs::File>,
    log: Option<PathBuf>,
    last_phrase: String,
    registered: std::collections::HashMap<String, Result<(), String>>,
    #[cfg_attr(not(target_os = "windows"), allow(dead_code))]
    autostart: Option<AutostartCapability>,
    #[cfg(target_os = "macos")]
    menu_rows: Vec<Resolved>,
}

/// Take the single-instance lock, preserving the error's type so
/// `is_expected` can recognise `AcquireError::AlreadyRunning`.
fn acquire_lock(config: &Path) -> Result<std::fs::File> {
    Ok(crate::lockfile::acquire(config)?)
}

/// Which `BrokenConfig` the one macOS front door takes, from whether anyone
/// is watching its stderr.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) fn macos_broken_config(stderr_is_terminal: bool) -> BrokenConfig {
    if stderr_is_terminal {
        BrokenConfig::Refuse
    } else {
        BrokenConfig::ServeAnyway
    }
}

#[cfg(target_os = "macos")]
fn front_door_broken_config() -> BrokenConfig {
    macos_broken_config(std::io::IsTerminal::is_terminal(&std::io::stderr()))
}

#[cfg(not(target_os = "macos"))]
fn front_door_broken_config() -> BrokenConfig {
    BrokenConfig::Refuse
}

pub fn cmd_serve(config: &Path, log: Option<PathBuf>) -> Result<()> {
    cmd_serve_app(config, log, None, front_door_broken_config())
}

/// Shared implementation for both Windows front doors (`cmd_serve`, the CLI
/// path, and `serve_app_main`, the tray app) and for macOS `serve`.
pub fn cmd_serve_app(
    config: &Path,
    log: Option<PathBuf>,
    autostart: Option<AutostartCapability>,
    on_broken: BrokenConfig,
) -> Result<()> {
    let _lock = acquire_lock(config)?;
    let config = beckon_core::paths::plain(
        config
            .canonicalize()
            .with_context(|| format!("cannot resolve `{}`", config.display()))?,
    );
    let text = std::fs::read_to_string(&config)
        .with_context(|| format!("cannot read `{}`", config.display()))?;
    let plan =
        plan_startup(&text, on_broken).map_err(|e| anyhow!("{}: {}", config.display(), e))?;
    let broken = plan.broken;
    // Outside the RefCell on purpose — see module doc.
    let backend: Rc<Box<dyn Backend>> = Rc::new(crate::pick_backend()?);

    let state = Rc::new(RefCell::new(ServeState {
        #[cfg(target_os = "macos")]
        caps_owner: None,
        shortcuts: plan.shortcuts,
        keyboard: plan.keyboard,
        tray: plan.tray,
        config: config.clone(),
        paused: false,
        log,
        last_phrase: String::new(),
        registered: Default::default(),
        autostart,
        #[cfg(target_os = "macos")]
        menu_rows: Vec::new(),
    }));

    let mgr = {
        let st = Rc::clone(&state);
        let be = Rc::clone(&backend);
        HotkeyManager::install(Box::new(move |id| on_hotkey(&st, &be, id)))
            .map_err(|e| anyhow!(e))?
    };
    let mgr = Rc::new(RefCell::new(mgr));

    // After the hotkey manager exists (which puts the process in UIElement
    // activation policy), before anything is registered.
    #[cfg(target_os = "macos")]
    ask_for_accessibility_once();

    let outcome = register_all(&mut mgr.borrow_mut(), &state.borrow().shortcuts);

    let (tx, rx) = std::sync::mpsc::channel::<()>();
    let _watcher = watch_config(&config, tx)?; // lives as long as the loop below
    {
        let st = Rc::clone(&state);
        let mg = Rc::clone(&mgr);
        hotkey::add_tick(
            1.0,
            Box::new(move || {
                if rx.try_recv().is_err() {
                    return;
                }
                while rx.try_recv().is_ok() {} // drain the write+rename burst
                reload(&st, &mg);
            }),
        );
    }

    // Watch for Accessibility appearing so granting it restarts cleanly under
    // launchd without a second manual step.
    #[cfg(target_os = "macos")]
    if !beckon_macos::is_accessibility_trusted() {
        hotkey::add_tick(
            1.0,
            Box::new(move || {
                use beckon_core::grant::{grant_recovery, GrantRecovery};
                use std::io::IsTerminal;
                match grant_recovery(
                    true,
                    beckon_macos::is_accessibility_trusted(),
                    std::io::stderr().is_terminal(),
                ) {
                    GrantRecovery::Nothing => {}
                    GrantRecovery::TellAndStay => {
                        static TOLD: std::sync::atomic::AtomicBool =
                            std::sync::atomic::AtomicBool::new(false);
                        if !TOLD.swap(true, std::sync::atomic::Ordering::Relaxed) {
                            eprintln!(
                                "beckon serve: Accessibility granted. Restart beckon so \
                                 it can take effect -- macOS hands the permission to a \
                                 new process."
                            );
                        }
                    }
                    GrantRecovery::RestartUnderLaunchd => {
                        eprintln!(
                            "beckon serve: Accessibility granted; restarting so it \
                             takes effect."
                        );
                        std::process::exit(1);
                    }
                }
            }),
        );
    }

    #[cfg(target_os = "macos")]
    if state.borrow().tray {
        refresh_menu_rows(&state);
    }

    #[cfg(target_os = "windows")]
    install_tray_menu(&state, &mgr);

    if let Some(e) = &broken {
        eprintln!(
            "beckon serve: config does not parse, serving no shortcuts: {}: {e}",
            config.display()
        );
    }
    let phrase = match &broken {
        Some(_) => unreadable_phrase(),
        None => registration_phrase(outcome.ok, state.borrow().shortcuts.len()),
    };
    state.borrow_mut().last_phrase = phrase.clone();
    state.borrow_mut().registered = outcome.by_canonical();
    sync_caps_hook(&state);
    sync_tray(&state, &mgr);
    eprintln!("beckon serve: {} from {}", phrase, config.display());
    set_tray_status(&format!("beckon - {phrase}"));
    if let Some(toast) = failure_toast(&outcome.failed) {
        crate::notify::report(&toast, crate::notify::Cause::MachineRepeat);
    }
    if broken.is_some() {
        crate::notify::report(
            "no hotkeys registered - beckon cannot read the config. \
             Run `beckon check` or open the config from the tray icon.",
            crate::notify::Cause::MachineRepeat,
        );
    }
    HotkeyManager::run_forever();
}

fn on_hotkey(state: &Rc<RefCell<ServeState>>, backend: &Rc<Box<dyn Backend>>, id: u32) {
    // Short borrow only: cloned out and dropped BEFORE calling the backend.
    let (app, canonical, launch) = {
        let st = state.borrow();
        match st.shortcuts.get(id as usize) {
            Some(sc) => (sc.app.clone(), sc.combo.canonical(), sc.launch),
            None => return,
        }
    };
    match crate::beckon_ladder_with(
        backend.as_ref().as_ref(),
        &app,
        launch,
        beckon_core::verbose(),
    ) {
        Ok(beckon_core::BeckonAction::LaunchPending) => {
            set_tray_status(&format!("beckon - press {canonical} again to launch {app}"));
        }
        Ok(_) => {
            let phrase = state.borrow().last_phrase.clone();
            if !phrase.is_empty() {
                set_tray_status(&format!("beckon - {phrase}"));
            }
        }
        Err(e) => {
            let msg = format!("{app} ({canonical}): {e}");
            eprintln!("beckon serve: {msg}");
            crate::notify::report(&msg, crate::notify::Cause::HumanAction);
        }
    }
}

struct RegisterOutcome {
    ok: usize,
    failed: Vec<String>,
    results: Vec<(String, Result<(), String>)>,
}

impl RegisterOutcome {
    fn by_canonical(&self) -> std::collections::HashMap<String, Result<(), String>> {
        self.results.iter().cloned().collect()
    }
}

fn shortcut_noun(n: usize) -> &'static str {
    if n == 1 {
        "shortcut"
    } else {
        "shortcuts"
    }
}

fn shortcuts_count_phrase(n: usize) -> String {
    format!("{n} {}", shortcut_noun(n))
}

fn registration_phrase(ok: usize, total: usize) -> String {
    let failed = total - ok;
    let noun = shortcut_noun(total);
    if failed == 0 {
        format!("{total} {noun} registered")
    } else {
        format!("{ok} of {total} {noun} registered ({failed} failed)")
    }
}

fn failure_toast(failed: &[String]) -> Option<String> {
    if failed.is_empty() {
        return None;
    }
    const SHOWN: usize = 5;
    let n = failed.len();
    let listed = failed[..n.min(SHOWN)].join(", ");
    if n > SHOWN {
        let more = n - SHOWN;
        Some(format!(
            "{n} hotkeys failed to register: {listed} and {more} more"
        ))
    } else {
        Some(format!("{n} hotkeys failed to register: {listed}"))
    }
}

fn register_all(mgr: &mut HotkeyManager, shortcuts: &[Shortcut]) -> RegisterOutcome {
    let mut ok = 0;
    let mut failed = Vec::new();
    let mut results = Vec::with_capacity(shortcuts.len());
    for (i, sc) in shortcuts.iter().enumerate() {
        let c = &sc.combo;
        let canon = c.canonical();
        match mgr.register(i as u32, c.ctrl, c.super_, c.alt, c.shift, c.key) {
            Ok(()) => {
                ok += 1;
                results.push((canon, Ok(())));
            }
            Err(e) => {
                eprintln!("beckon serve: cannot register `{}`: {e}", canon);
                failed.push(canon.clone());
                results.push((canon, Err(e)));
            }
        }
    }
    RegisterOutcome {
        ok,
        failed,
        results,
    }
}

/// Does any changed path refer to our config file (by file name)? We watch
/// the PARENT directory, not the file: vim/sed replace the file by rename,
/// which kills an inode-level watch silently.
fn event_touches(paths: &[PathBuf], file_name: Option<&std::ffi::OsStr>) -> bool {
    let Some(name) = file_name else { return false };
    paths.iter().any(|p| p.file_name() == Some(name))
}

fn watch_config(
    config: &Path,
    tx: std::sync::mpsc::Sender<()>,
) -> Result<notify::RecommendedWatcher> {
    use notify::Watcher;
    let file_name = config.file_name().map(|s| s.to_owned());
    let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
        if let Ok(ev) = res {
            if event_touches(&ev.paths, file_name.as_deref()) {
                let _ = tx.send(());
            }
        }
    })
    .context("cannot create file watcher")?;
    let dir = config
        .parent()
        .ok_or_else(|| anyhow!("config has no parent directory"))?;
    watcher
        .watch(dir, notify::RecursiveMode::NonRecursive)
        .with_context(|| format!("cannot watch `{}`", dir.display()))?;
    Ok(watcher)
}

#[cfg(target_os = "windows")]
fn set_autostart(state: &Rc<RefCell<ServeState>>, on: bool) {
    let result = if !on {
        beckon_windows::autostart::disable()
    } else {
        match std::env::current_exe() {
            Ok(exe) => {
                let exe = crate::serve_app::scoop_current_path(&exe);
                let s = state.borrow();
                let (cfg, log) = s
                    .autostart
                    .as_ref()
                    .map(|a| (a.config.as_deref(), a.log.as_deref()))
                    .unwrap_or((None, None));
                let cmd = crate::serve_app::run_key_command_line(&exe, cfg, log);
                drop(s);
                beckon_windows::autostart::enable(&cmd)
            }
            Err(e) => Err(format!("cannot find our own path: {e}")),
        }
    };
    if let Err(e) = result {
        eprintln!("beckon serve: autostart: {e}");
        crate::notify::report(
            &format!("could not change autostart: {e}"),
            crate::notify::Cause::HumanAction,
        );
    }
}

/// Open the active config file in whatever editor the OS associates with it.
/// Every `RefCell` borrow is dropped before calling into the shell.
#[cfg(target_os = "windows")]
fn open_config(state: &Rc<RefCell<ServeState>>) {
    let path = state.borrow().config.clone();
    if let Err(e) = beckon_windows::shell::open_path(&path) {
        eprintln!("beckon serve: {e}");
    }
}

#[cfg(target_os = "macos")]
fn open_config(state: &Rc<RefCell<ServeState>>) {
    let path = state.borrow().config.clone();
    if let Err(e) = beckon_macos::shell::open_path(&path) {
        eprintln!("beckon serve: {e}");
    }
}

/// Re-read `config`, replace the shortcut table, and (unless paused)
/// re-register every hotkey.
fn reload(state: &Rc<RefCell<ServeState>>, mgr: &Rc<RefCell<HotkeyManager>>) {
    let config = state.borrow().config.clone();
    let parsed = std::fs::read_to_string(&config)
        .map_err(|e| format!("cannot read `{}`: {e}", config.display()))
        .and_then(|t| parse_config(&t).map_err(|e| format!("{}: {e}", config.display())));
    match parsed {
        Err(e) => {
            // Bad edit must not cost the user their working keys.
            let msg = format!("reload failed, keeping current shortcuts: {e}");
            eprintln!("beckon serve: {msg}");
            crate::notify::report(&msg, crate::notify::Cause::MachineRepeat);
        }
        Ok(new) => {
            {
                let mut s = state.borrow_mut();
                s.shortcuts = new.shortcuts;
                s.keyboard = new.keyboard;
                s.tray = new.tray;
            }
            // Resolve before borrowing `mgr` or unregistering anything so
            // hotkeys stay live during the catalog scan and no RefCell borrow
            // spans the resolver.
            #[cfg(target_os = "macos")]
            if state.borrow().tray {
                refresh_menu_rows(state);
            }
            let paused = state.borrow().paused;
            if paused {
                {
                    let mut m = mgr.borrow_mut();
                    m.unregister_all();
                }
                let phrase = shortcuts_count_phrase(state.borrow().shortcuts.len());
                state.borrow_mut().last_phrase = phrase.clone();
                state.borrow_mut().registered.clear();
                sync_caps_hook(state);
                sync_tray(state, mgr);
                eprintln!("beckon serve: reloaded while paused - {phrase}");
                set_tray_status(&format!("beckon - paused ({phrase})"));
                return;
            }
            let outcome = {
                let mut m = mgr.borrow_mut();
                m.unregister_all();
                register_all(&mut m, &state.borrow().shortcuts)
            };
            let phrase = registration_phrase(outcome.ok, state.borrow().shortcuts.len());
            state.borrow_mut().last_phrase = phrase.clone();
            state.borrow_mut().registered = outcome.by_canonical();
            sync_caps_hook(state);
            sync_tray(state, mgr);
            eprintln!("beckon serve: reloaded - {phrase}");
            set_tray_status(&format!("beckon - {phrase}"));
            if let Some(toast) = failure_toast(&outcome.failed) {
                crate::notify::report(&toast, crate::notify::Cause::MachineRepeat);
            }
        }
    }
}

fn set_paused(state: &Rc<RefCell<ServeState>>, mgr: &Rc<RefCell<HotkeyManager>>, paused: bool) {
    let mut m = mgr.borrow_mut();
    if paused {
        m.unregister_all();
        state.borrow_mut().paused = true;
        let phrase = shortcuts_count_phrase(state.borrow().shortcuts.len());
        state.borrow_mut().last_phrase = phrase.clone();
        state.borrow_mut().registered.clear();
        sync_caps_hook(state);
        eprintln!("beckon serve: paused - {phrase}");
        set_tray_status(&format!("beckon - paused ({phrase})"));
        set_tray_dimmed(true);
    } else {
        state.borrow_mut().paused = false;
        let outcome = register_all(&mut m, &state.borrow().shortcuts);
        let phrase = registration_phrase(outcome.ok, state.borrow().shortcuts.len());
        state.borrow_mut().last_phrase = phrase.clone();
        state.borrow_mut().registered = outcome.by_canonical();
        sync_caps_hook(state);
        eprintln!("beckon serve: resumed - {phrase}");
        set_tray_status(&format!("beckon - {phrase}"));
        set_tray_dimmed(false);
    }
}

// ---------------------------------------------------------------------------
// Tray / menu bar menu.
// ---------------------------------------------------------------------------

const MENU_STATUS: u32 = 1;
const MENU_EDIT: u32 = 2;
const MENU_RELOAD: u32 = 3;
const MENU_LOG: u32 = 4;
const MENU_PAUSE: u32 = 5;
#[cfg(any(target_os = "windows", test))]
const MENU_AUTOSTART: u32 = 6;
const MENU_QUIT: u32 = 7;

#[cfg(any(target_os = "windows", test))]
#[derive(Clone)]
struct MenuModel {
    phrase: String,
    paused: bool,
    autostart: Option<bool>,
    log: Option<bool>,
}

/// Whether the tray draws `Open log`, and whether it is enabled.
///
/// Windows owns its log, so the row is always shown (greyed when no `--log`
/// was supplied). macOS does not (launchd writes `StandardErrorPath`), so the
/// row is omitted unless `--log` declared where it lives.
pub(crate) fn menu_log_row(has_log_path: bool, beckon_owns_the_log: bool) -> Option<bool> {
    if beckon_owns_the_log {
        Some(has_log_path)
    } else {
        has_log_path.then_some(true)
    }
}

#[cfg(any(target_os = "macos", test))]
#[derive(Debug, Clone, PartialEq, Eq, Default)]
struct Resolved {
    name: String,
    bundle_id: Option<String>,
    missing: bool,
}

#[cfg(any(target_os = "macos", test))]
fn resolve_rows(shortcuts: &[Shortcut], reports: &[NameReport]) -> Vec<Resolved> {
    use beckon_core::certainty::{chain_winner, Certainty};
    let grade: std::collections::HashMap<&str, &NameReport> =
        reports.iter().map(|r| (r.id.as_str(), r)).collect();
    shortcuts
        .iter()
        .map(|s| match beckon_core::candidates::split(&s.app) {
            Err(_) => Resolved {
                name: s.app.clone(),
                bundle_id: None,
                missing: true,
            },
            Ok(cands) => match chain_winner(&cands, |c| grade.get(c).copied()) {
                Some(r) if r.certainty != Certainty::NoMatch => Resolved {
                    name: r.id.clone(),
                    bundle_id: r.target.clone(),
                    missing: false,
                },
                Some(_) => Resolved {
                    name: cands[0].to_string(),
                    bundle_id: None,
                    missing: true,
                },
                None => Resolved {
                    name: cands[0].to_string(),
                    bundle_id: None,
                    missing: false,
                },
            },
        })
        .collect()
}

#[cfg(target_os = "macos")]
fn refresh_menu_rows(state: &Rc<RefCell<ServeState>>) {
    let started = std::time::Instant::now();
    let shortcuts = state.borrow().shortcuts.clone();
    let mut names: Vec<&str> = shortcuts
        .iter()
        .filter_map(|s| beckon_core::candidates::split(&s.app).ok())
        .flatten()
        .collect();
    names.sort_unstable();
    names.dedup();
    let rows = match beckon_macos::resolve_reports(&names) {
        Ok(reports) => resolve_rows(&shortcuts, &reports),
        Err(e) => {
            eprintln!("beckon serve: menu rows not resolved ({e}); names shown as written");
            Vec::new()
        }
    };
    if beckon_core::verbose() {
        eprintln!(
            "beckon serve: resolved {} menu rows in {} ms",
            rows.len(),
            started.elapsed().as_millis()
        );
    }
    state.borrow_mut().menu_rows = rows;
}

#[cfg(any(target_os = "macos", test))]
#[derive(Debug, Clone, PartialEq, Eq)]
struct BindingRow {
    name: String,
    chord: String,
    spoken: String,
    flag: Option<&'static str>,
    bundle_id: Option<String>,
}

#[cfg(any(target_os = "macos", test))]
fn binding_rows(
    shortcuts: &[Shortcut],
    resolved: &[Resolved],
    registered: &std::collections::HashMap<String, Result<(), String>>,
    paused: bool,
    hold: Option<beckon_core::shortcuts::Chord>,
) -> Vec<BindingRow> {
    use beckon_core::shortcuts::{combo_display_folded_with, combo_glyphs, ModifierLabels};
    shortcuts
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let canon = s.combo.canonical();
            let r = resolved.get(i);
            let name = r.map_or_else(|| s.app.clone(), |r| r.name.clone());
            let in_use = matches!(registered.get(&canon), Some(Err(_)));
            let missing = r.is_some_and(|r| r.missing);
            BindingRow {
                chord: combo_glyphs(&canon, hold),
                spoken: format!(
                    "{name}, {}",
                    combo_display_folded_with(&canon, hold, ModifierLabels::MAC)
                ),
                flag: if paused {
                    None
                } else {
                    beckon_core::menu::row_flag(in_use, missing)
                },
                bundle_id: r.and_then(|r| r.bundle_id.clone()),
                name,
            }
        })
        .collect()
}

/// Windows' tray context menu.
#[cfg(any(target_os = "windows", test))]
fn build_entries(m: &MenuModel) -> Vec<MenuEntry> {
    let head = if m.paused {
        format!("beckon - paused ({})", m.phrase)
    } else {
        format!("beckon - {}", m.phrase)
    };
    let mut entries = vec![
        MenuEntry {
            id: MENU_STATUS,
            label: head,
            checked: None,
            enabled: false,
            ..MenuEntry::default()
        },
        MenuEntry::separator(),
        MenuEntry::item(MENU_EDIT, "Open config file"),
        MenuEntry::item(MENU_RELOAD, "Reload now"),
    ];
    if let Some(enabled) = m.log {
        entries.push(MenuEntry {
            id: MENU_LOG,
            label: "Open log".into(),
            checked: None,
            enabled,
            ..MenuEntry::default()
        });
    }
    entries.extend([
        MenuEntry::separator(),
        MenuEntry {
            id: MENU_PAUSE,
            label: "Pause hotkeys".into(),
            checked: Some(m.paused),
            enabled: true,
            ..MenuEntry::default()
        },
    ]);
    if let Some(checked) = m.autostart {
        entries.push(MenuEntry {
            id: MENU_AUTOSTART,
            label: "Start with Windows".into(),
            checked: Some(checked),
            enabled: true,
            ..MenuEntry::default()
        });
    }
    entries.push(MenuEntry::separator());
    entries.push(MenuEntry {
        id: MENU_QUIT,
        label: "Quit".into(),
        checked: None,
        enabled: true,
        ..MenuEntry::default()
    });
    entries
}

#[cfg(any(target_os = "macos", test))]
const MENU_SHORTCUTS: u32 = 9;
#[cfg(any(target_os = "macos", test))]
const MENU_EDIT_SHORTCUTS: u32 = 10;
#[cfg(any(target_os = "macos", test))]
const MENU_BINDING_BASE: u32 = 1000;

#[cfg(any(target_os = "macos", test))]
struct MacMenu {
    phrase: String,
    paused: bool,
    accessibility: bool,
    failed: usize,
    log: Option<bool>,
    rows: Vec<BindingRow>,
}

#[cfg(any(target_os = "macos", test))]
fn binding_entry(i: usize, r: &BindingRow) -> MenuEntry {
    MenuEntry {
        id: MENU_BINDING_BASE + i as u32,
        label: r.name.clone(),
        enabled: true,
        detail: Some(r.chord.clone()),
        flag: r.flag,
        icon: r.bundle_id.clone(),
        tooltip: Some(r.spoken.clone()),
        ..MenuEntry::default()
    }
}

/// The macOS menu bar menu: eight rows when nothing is broken, whatever the
/// config holds; the shortcut table sits in a submenu.
#[cfg(any(target_os = "macos", test))]
fn build_mac_entries(m: &MacMenu) -> Vec<MenuEntry> {
    use beckon_core::menu::{
        attention, menu_headline, EntryKind, Header, HeadlineInputs, ATTENTION_ROWS,
    };
    let missing = m.rows.iter().filter(|r| r.flag == Some("missing")).count();
    let head = menu_headline(HeadlineInputs {
        paused: m.paused,
        accessibility: m.accessibility,
        phrase: &m.phrase,
        failed: m.failed,
        missing,
        total: m.rows.len(),
    });
    let mut e = vec![
        MenuEntry {
            id: MENU_STATUS,
            label: "beckon".into(),
            kind: EntryKind::Header(Header {
                title: "beckon".into(),
                subtitle: head.text,
                dot: head.dot,
                on: !m.paused,
            }),
            ..MenuEntry::default()
        },
        MenuEntry::separator(),
    ];

    if !m.paused {
        let flags: Vec<Option<&str>> = m.rows.iter().map(|r| r.flag).collect();
        let a = attention(&flags, ATTENTION_ROWS);
        if !a.shown.is_empty() {
            e.push(MenuEntry::section_header("Needs attention"));
            e.extend(a.shown.iter().map(|&i| binding_entry(i, &m.rows[i])));
            if let Some(&first) = a.rest.first() {
                e.push(MenuEntry::item(
                    MENU_BINDING_BASE + first as u32,
                    format!("and {} more...", a.rest.len()),
                ));
            }
            e.push(MenuEntry::separator());
        }
    }

    let mut children: Vec<MenuEntry> = m
        .rows
        .iter()
        .enumerate()
        .map(|(i, r)| binding_entry(i, r))
        .collect();
    if !children.is_empty() {
        children.push(MenuEntry::separator());
    }
    children.push(MenuEntry::item(MENU_EDIT_SHORTCUTS, "Edit Shortcuts..."));
    e.push(MenuEntry {
        id: MENU_SHORTCUTS,
        label: "Shortcuts".into(),
        enabled: true,
        kind: EntryKind::Submenu,
        children,
        ..MenuEntry::default()
    });
    e.push(MenuEntry::separator());

    e.push(MenuEntry {
        key: Some(','),
        ..MenuEntry::item(MENU_EDIT, "Open Config")
    });
    e.push(MenuEntry {
        key: Some('r'),
        ..MenuEntry::item(MENU_RELOAD, "Reload Config")
    });
    if m.log == Some(true) {
        e.push(MenuEntry::item(MENU_LOG, "Open Log"));
    }
    e.push(MenuEntry::separator());
    e.push(MenuEntry {
        key: Some('q'),
        ..MenuEntry::item(MENU_QUIT, "Quit beckon")
    });
    e
}

#[cfg(target_os = "windows")]
fn install_tray_menu(state: &Rc<RefCell<ServeState>>, mgr: &Rc<RefCell<HotkeyManager>>) {
    let st_build = Rc::clone(state);
    let build = Box::new(move || {
        let s = st_build.borrow();
        build_entries(&MenuModel {
            phrase: s.last_phrase.clone(),
            paused: s.paused,
            autostart: s
                .autostart
                .as_ref()
                .map(|_| beckon_windows::autostart::is_enabled()),
            log: menu_log_row(s.log.is_some(), cfg!(target_os = "windows")),
        })
    });

    let st = Rc::clone(state);
    let mg = Rc::clone(mgr);
    let on_click = Box::new(move |id: u32| match id {
        MENU_EDIT | hotkey::MENU_ID_DOUBLE_CLICK => open_config(&st),
        MENU_LOG => {
            let path = st.borrow().log.clone();
            if let Some(path) = path {
                if let Err(e) = beckon_windows::shell::open_path(&path) {
                    eprintln!("beckon serve: {e}");
                }
            }
        }
        MENU_RELOAD => reload(&st, &mg),
        MENU_PAUSE => {
            let now = !st.borrow().paused;
            set_paused(&st, &mg, now);
        }
        MENU_AUTOSTART => {
            set_autostart(&st, !beckon_windows::autostart::is_enabled());
        }
        MENU_QUIT => {
            eprintln!("beckon serve: quit requested from the tray menu");
            hotkey::request_quit();
        }
        _ => {}
    });

    hotkey::set_menu(build, on_click);
}

#[cfg(target_os = "macos")]
fn install_tray_menu(state: &Rc<RefCell<ServeState>>, mgr: &Rc<RefCell<HotkeyManager>>) {
    use beckon_macos::tray;

    let st_build = Rc::clone(state);
    let build = Box::new(move || {
        let s = st_build.borrow();
        let hold = s.keyboard.caps.then_some(s.keyboard.caps_hold);
        build_mac_entries(&MacMenu {
            phrase: s.last_phrase.clone(),
            paused: s.paused,
            accessibility: beckon_macos::is_accessibility_trusted(),
            failed: s.registered.values().filter(|r| r.is_err()).count(),
            log: menu_log_row(s.log.is_some(), cfg!(target_os = "windows")),
            rows: binding_rows(&s.shortcuts, &s.menu_rows, &s.registered, s.paused, hold),
        })
    });

    let st = Rc::clone(state);
    let mg = Rc::clone(mgr);
    let on_click = Box::new(move |id: u32| match id {
        MENU_EDIT | MENU_EDIT_SHORTCUTS | beckon_core::menu::MENU_ID_DOUBLE_CLICK => {
            open_config(&st);
        }
        MENU_RELOAD => reload(&st, &mg),
        MENU_STATUS | MENU_PAUSE | beckon_core::menu::MENU_ID_ALT_CLICK => {
            let now = !st.borrow().paused;
            set_paused(&st, &mg, now);
        }
        MENU_LOG => {
            let path = st.borrow().log.clone();
            if let Some(path) = path {
                if let Err(e) = beckon_macos::shell::open_path(&path) {
                    eprintln!("beckon serve: {e}");
                }
            }
        }
        MENU_QUIT => {
            eprintln!("beckon serve: quit requested from the menu bar");
            tray::request_quit();
        }
        // Clicking any binding row in the menu opens the config file —
        // nothing in the shortcut menu launches or focuses an app.
        id if (MENU_BINDING_BASE..beckon_core::menu::MENU_ID_ALT_CLICK).contains(&id) => {
            open_config(&st);
        }
        _ => {}
    });

    if let Err(e) = tray::set_menu(build, on_click) {
        eprintln!("beckon serve: no menu bar item ({e}); hotkeys are unaffected");
    }
}

#[cfg(target_os = "windows")]
fn sync_tray(state: &Rc<RefCell<ServeState>>, _mgr: &Rc<RefCell<HotkeyManager>>) {
    let show = state.borrow().tray;
    hotkey::set_tray_visible(show);
}

#[cfg(target_os = "macos")]
fn sync_tray(state: &Rc<RefCell<ServeState>>, mgr: &Rc<RefCell<HotkeyManager>>) {
    let (show, paused) = {
        let s = state.borrow();
        (s.tray, s.paused)
    };
    if show {
        install_tray_menu(state, mgr);
        set_tray_dimmed(paused);
    } else {
        beckon_macos::tray::remove();
    }
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
fn sync_tray(_state: &Rc<RefCell<ServeState>>, _mgr: &Rc<RefCell<HotkeyManager>>) {}

// ---------------------------------------------------------------------------
// Caps hook / event tap
// ---------------------------------------------------------------------------

#[cfg(target_os = "windows")]
fn sync_caps_hook(state: &Rc<RefCell<ServeState>>) {
    use beckon_windows::caps_hook;

    let (want, tap, hold, bound) = {
        let s = state.borrow();
        (
            s.keyboard.caps && !s.paused,
            s.keyboard.caps_tap,
            s.keyboard.caps_hold,
            beckon_core::caps::bound_keys(&s.registered, s.keyboard.caps_hold),
        )
    };

    if !want {
        caps_hook::clear_bindings();
        let was_installed = caps_hook::is_installed();
        caps_hook::uninstall();
        if was_installed && !caps_hook::is_installed() {
            eprintln!("beckon serve: caps hook removed");
        }
        return;
    }

    let keys = bound.len();
    caps_hook::set_bindings(bound, hold, tap);
    let was_installed = caps_hook::is_installed();
    match caps_hook::install() {
        Ok(()) => {
            if !was_installed {
                eprintln!("beckon serve: caps hook active, {keys} keys reachable through Caps");
            }
        }
        Err(e) => {
            eprintln!("beckon serve: {e}");
            crate::notify::report(
                &format!("could not enable Caps Lock: {e}"),
                crate::notify::Cause::HumanAction,
            );
            state.borrow_mut().keyboard.caps = false;
        }
    }
}

#[cfg(target_os = "macos")]
fn ask_for_accessibility_once() {
    if beckon_macos::is_accessibility_trusted() {
        return;
    }
    if !beckon_macos::request_accessibility() {
        eprintln!(
            "beckon serve: Accessibility is not granted, so hotkeys cannot focus \
             anything. Allow beckon in the dialog, or in System Settings > Privacy \
             & Security > Accessibility, then start beckon again."
        );
    }
}

/// Ask macOS for Input Monitoring at most once per process when the user
/// enables `keyboard.caps = true`. `IOHIDCheckAccess` alone never creates a
/// TCC row, leaving the user with nothing to toggle in System Settings.
#[cfg(target_os = "macos")]
fn ask_for_input_monitoring_once() {
    use std::sync::atomic::{AtomicBool, Ordering};
    static ASKED: AtomicBool = AtomicBool::new(false);
    if !ASKED.swap(true, Ordering::Relaxed) {
        let _ = beckon_macos::caps_tap::request_input_monitoring();
    }
}

#[cfg(not(target_os = "windows"))]
fn sync_caps_hook(state: &Rc<RefCell<ServeState>>) {
    use beckon_macos::caps_tap;

    let (want, tap, hold, bound) = {
        let s = state.borrow();
        (
            s.keyboard.caps && !s.paused,
            s.keyboard.caps_tap,
            s.keyboard.caps_hold,
            beckon_core::caps::bound_keys_mac(&s.registered, s.keyboard.caps_hold),
        )
    };

    if !want {
        caps_tap::clear_bindings();
        let was = caps_tap::is_installed();
        caps_tap::uninstall();
        state.borrow_mut().caps_owner = None;
        if was && !caps_tap::is_installed() {
            eprintln!("beckon serve: caps event tap removed");
        }
        return;
    }

    let keys = bound.len();

    if state.borrow().caps_owner.is_none() {
        match crate::lockfile::acquire_caps() {
            Some(f) => state.borrow_mut().caps_owner = Some(f),
            None => {
                if caps_tap::is_installed() {
                    caps_tap::uninstall();
                }
                eprintln!(
                    "beckon serve: another beckon owns Caps on this machine; \
                     Caps shortcuts are off here (the typed chord still works)"
                );
                return;
            }
        }
    }

    if !caps_tap::input_monitoring_granted() {
        ask_for_input_monitoring_once();
    }

    caps_tap::set_bindings(bound, hold, tap);
    let was = caps_tap::is_installed();
    match caps_tap::install() {
        Ok(()) => {
            if !was {
                eprintln!(
                    "beckon serve: caps event tap active, {keys} keys reachable through Caps"
                );
            }
        }
        Err(e) => {
            eprintln!("beckon serve: {e}");
            crate::notify::report(
                &format!("could not enable Caps Lock: {e}"),
                crate::notify::Cause::HumanAction,
            );
            state.borrow_mut().keyboard.caps = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use beckon_core::certainty::{Certainty, NameReport};
    use beckon_core::menu::EntryKind;
    use beckon_core::shortcuts::{parse_shortcuts, Chord};
    use std::collections::HashMap;
    use std::ffi::OsStr;

    #[test]
    fn registration_phrase_all_ok() {
        assert_eq!(registration_phrase(20, 20), "20 shortcuts registered");
    }

    #[test]
    fn registration_phrase_some_failed() {
        assert_eq!(
            registration_phrase(17, 20),
            "17 of 20 shortcuts registered (3 failed)"
        );
    }

    #[test]
    fn registration_phrase_singular_all_ok() {
        assert_eq!(registration_phrase(1, 1), "1 shortcut registered");
    }

    #[test]
    fn registration_phrase_singular_total_failed() {
        assert_eq!(
            registration_phrase(0, 1),
            "0 of 1 shortcut registered (1 failed)"
        );
    }

    #[test]
    fn shortcuts_count_phrase_agrees_with_count() {
        assert_eq!(shortcuts_count_phrase(1), "1 shortcut");
        assert_eq!(shortcuts_count_phrase(5), "5 shortcuts");
    }

    #[test]
    fn lock_error_keeps_its_type_through_anyhow() {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("apps.toml");
        std::fs::write(&config, "").unwrap();

        let _held = acquire_lock(&config).expect("first lock");
        let err = acquire_lock(&config).expect_err("second lock must be refused");

        assert!(
            crate::is_expected(&err),
            "the refusal must still be recognisable as a designed outcome, \
             got `{err:#}`"
        );
        let canonical = config.canonicalize().unwrap();
        assert!(
            format!("{err:#}").contains(&canonical.display().to_string()),
            "the message must name the config, not just the lock hash: {err:#}"
        );
    }

    #[test]
    fn failure_toast_empty_is_none() {
        assert_eq!(failure_toast(&[]), None);
    }

    #[test]
    fn failure_toast_lists_all_up_to_five() {
        let failed = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        assert_eq!(
            failure_toast(&failed),
            Some("3 hotkeys failed to register: a, b, c".to_string())
        );
    }

    #[test]
    fn failure_toast_caps_at_five_then_more() {
        let failed: Vec<String> = ["a", "b", "c", "d", "e", "f", "g"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(
            failure_toast(&failed),
            Some("7 hotkeys failed to register: a, b, c, d, e and 2 more".to_string())
        );
    }

    const GOOD: &str = "\"ctrl+alt+t\" = \"Terminal\"\nkeyboard.caps = true\ntray = false\n";
    const BROKEN: &str =
        "\"ctrl+alt+t\" = \"Terminal\"\nkeyboard.caps = true\ntray = false\n\"ctrl+alt+e\" = \n";

    #[test]
    fn a_file_that_parses_is_served_whatever_the_policy_says() {
        for policy in [BrokenConfig::Refuse, BrokenConfig::ServeAnyway] {
            let plan = plan_startup(GOOD, policy).expect("this file parses");
            assert_eq!(plan.shortcuts.len(), 1);
            assert!(plan.keyboard.caps, "the keyboard block is carried through");
            assert!(!plan.tray, "the tray flag is carried through");
            assert_eq!(plan.broken, None, "nothing is wrong with this file");
        }
    }

    #[test]
    fn macos_serve_under_launchd_starts_on_a_broken_file() {
        assert_eq!(macos_broken_config(false), BrokenConfig::ServeAnyway);
    }

    #[test]
    fn macos_serve_from_a_terminal_still_refuses() {
        assert_eq!(macos_broken_config(true), BrokenConfig::Refuse);
    }

    #[test]
    fn the_log_row_is_omitted_where_beckon_does_not_own_the_log() {
        assert_eq!(menu_log_row(false, false), None);
        assert_eq!(menu_log_row(true, false), Some(true));
        assert_eq!(menu_log_row(false, true), Some(false));
        assert_eq!(menu_log_row(true, true), Some(true));
    }

    #[test]
    fn the_console_front_door_still_refuses_a_broken_file() {
        let err = plan_startup(BROKEN, BrokenConfig::Refuse)
            .err()
            .expect("a broken file must be refused on this front door");
        assert_eq!(err, parse_config(BROKEN).unwrap_err());
    }

    #[test]
    fn the_gui_front_door_starts_with_nothing_registered_and_keeps_tray_on() {
        let plan =
            plan_startup(BROKEN, BrokenConfig::ServeAnyway).expect("this front door must start");
        assert!(plan.shortcuts.is_empty());
        assert!(plan.tray, "a broken file must not hide the tray icon");
        assert!(plan.broken.is_some());
        assert_eq!(
            plan.broken.as_deref(),
            Some(parse_config(BROKEN).unwrap_err()).as_deref()
        );
    }

    #[test]
    fn a_broken_start_cannot_arm_the_caps_hook() {
        assert!(BROKEN.contains("keyboard.caps = true"));
        let plan = plan_startup(BROKEN, BrokenConfig::ServeAnyway).unwrap();
        assert_eq!(plan.keyboard, KeyboardConfig::default());
        assert!(!plan.keyboard.caps);
    }

    #[test]
    fn the_unreadable_phrase_is_honest_and_ascii() {
        let p = unreadable_phrase();
        assert_eq!(p, "cannot read the config (0 shortcuts registered)");
        assert_eq!(
            format!("beckon - {p}"),
            "beckon - cannot read the config (0 shortcuts registered)"
        );
        assert!(p.is_ascii());
    }

    #[test]
    fn event_touches_matches_by_file_name_only() {
        let name = Some(OsStr::new("apps.macos.toml"));
        assert!(event_touches(
            &[PathBuf::from("/x/y/apps.macos.toml")],
            name
        ));
        assert!(!event_touches(
            &[PathBuf::from("/x/y/.apps.macos.toml.swp")],
            name
        ));
        assert!(!event_touches(&[PathBuf::from("/x/y/other.toml")], name));
        assert!(!event_touches(
            &[PathBuf::from("/x/y/apps.macos.toml")],
            None
        ));
    }

    #[test]
    fn menu_shows_the_phrase_and_reflects_pause() {
        let m = MenuModel {
            phrase: "5 shortcuts registered".into(),
            paused: false,
            autostart: Some(false),
            log: Some(true),
        };
        let rows = build_entries(&m);
        assert_eq!(rows[0].label, "beckon - 5 shortcuts registered");
        assert!(!rows[0].enabled);
        let pause = rows.iter().find(|r| r.id == MENU_PAUSE).unwrap();
        assert_eq!(pause.checked, Some(false));

        let paused = MenuModel {
            paused: true,
            ..m.clone()
        };
        let rows = build_entries(&paused);
        assert_eq!(rows[0].label, "beckon - paused (5 shortcuts registered)");
        assert_eq!(
            rows.iter().find(|r| r.id == MENU_PAUSE).unwrap().checked,
            Some(true)
        );

        let on = MenuModel {
            autostart: Some(true),
            ..m
        };
        assert_eq!(
            build_entries(&on)
                .iter()
                .find(|r| r.id == MENU_AUTOSTART)
                .unwrap()
                .checked,
            Some(true)
        );
    }

    #[test]
    fn no_capability_combination_produces_a_stray_separator() {
        for log in [None, Some(true), Some(false)] {
            for autostart in [None, Some(true), Some(false)] {
                let rows = build_entries(&MenuModel {
                    phrase: "p".into(),
                    paused: false,
                    autostart,
                    log,
                });
                let case = format!("log={log:?} autostart={autostart:?}");
                assert!(
                    !rows.last().unwrap().is_separator(),
                    "{case}: menu ends on a separator"
                );
                assert!(
                    !rows.first().unwrap().is_separator(),
                    "{case}: menu starts with a separator"
                );
                assert!(
                    !rows
                        .windows(2)
                        .any(|w| w[0].is_separator() && w[1].is_separator()),
                    "{case}: two separators in a row"
                );
            }
        }
    }

    #[test]
    fn autostart_row_exists_only_when_the_capability_does() {
        let base = MenuModel {
            phrase: "5 shortcuts registered".into(),
            paused: false,
            autostart: None,
            log: Some(true),
        };
        assert!(build_entries(&base).iter().all(|r| r.id != MENU_AUTOSTART));

        let with_capability = MenuModel {
            autostart: Some(true),
            ..base
        };
        let rows = build_entries(&with_capability);
        let hits: Vec<_> = rows.iter().filter(|r| r.id == MENU_AUTOSTART).collect();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].checked, Some(true));
    }

    #[test]
    fn no_real_entry_collides_with_the_reserved_double_click_id() {
        let m = MenuModel {
            phrase: "5 shortcuts registered".into(),
            paused: false,
            autostart: Some(false),
            log: Some(true),
        };
        for row in build_entries(&m) {
            assert_ne!(row.id, beckon_core::menu::MENU_ID_DOUBLE_CLICK);
        }
    }

    #[test]
    fn a_register_outcome_reports_every_combo_by_its_canonical_name() {
        let o = RegisterOutcome {
            ok: 1,
            failed: vec!["ctrl+alt+e".to_string()],
            results: vec![
                ("ctrl+alt+t".to_string(), Ok(())),
                ("ctrl+alt+e".to_string(), Err("taken".to_string())),
            ],
        };
        let map = o.by_canonical();
        assert!(map.get("ctrl+alt+t").unwrap().is_ok());
        assert!(map.get("ctrl+alt+e").unwrap().is_err());
        assert_eq!(map.len(), 2);
    }

    #[test]
    fn results_are_keyed_by_canonical_spelling_not_by_how_the_file_wrote_it() {
        let shortcuts =
            beckon_core::shortcuts::parse_shortcuts("\"alt+ctrl+t\" = \"Terminal\"\n").unwrap();
        assert_eq!(shortcuts[0].combo.canonical(), "ctrl+alt+t");
    }

    #[test]
    fn the_first_action_row_opens_the_config_file() {
        let m = MenuModel {
            phrase: "2 shortcuts registered".into(),
            paused: false,
            autostart: Some(false),
            log: Some(true),
        };
        let rows = build_entries(&m);
        let edit = rows.iter().find(|r| r.id == MENU_EDIT).unwrap();
        assert_eq!(edit.label, "Open config file");
    }

    #[test]
    fn open_log_is_disabled_when_there_is_no_log() {
        let m = MenuModel {
            phrase: "0 shortcuts registered".into(),
            paused: false,
            autostart: Some(false),
            log: Some(false),
        };
        let rows = build_entries(&m);
        assert!(!rows.iter().find(|r| r.id == MENU_LOG).unwrap().enabled);
    }

    fn nr(id: &str, certainty: Certainty, target: Option<&str>) -> NameReport {
        NameReport {
            id: id.into(),
            certainty,
            target: target.map(Into::into),
            tier: None,
            consequence: String::new(),
            suggestions: Vec::new(),
            rivals: Vec::new(),
            cold: None,
        }
    }

    #[test]
    fn a_chain_row_is_named_by_the_candidate_that_wins() {
        let s = parse_shortcuts("\"ctrl+alt+f\" = \"File Explorer || Finder\"\n").unwrap();
        let r = resolve_rows(
            &s,
            &[
                nr("File Explorer", Certainty::NoMatch, None),
                nr("Finder", Certainty::Exact, Some("com.apple.finder")),
            ],
        );
        assert_eq!(
            r,
            vec![Resolved {
                name: "Finder".into(),
                bundle_id: Some("com.apple.finder".into()),
                missing: false
            }]
        );
    }

    #[test]
    fn a_row_whose_every_candidate_misses_is_missing_under_its_first_name() {
        let s =
            parse_shortcuts("\"ctrl+alt+j\" = \"Tao Monitor || https://tao.example/\"\n").unwrap();
        let r = resolve_rows(
            &s,
            &[
                nr("Tao Monitor", Certainty::NoMatch, None),
                nr("https://tao.example/", Certainty::NoMatch, None),
            ],
        );
        assert_eq!(
            r[0],
            Resolved {
                name: "Tao Monitor".into(),
                bundle_id: None,
                missing: true
            }
        );
    }

    #[test]
    fn a_guess_resolves_rather_than_going_missing() {
        let s = parse_shortcuts("\"ctrl+alt+b\" = \"Brave\"\n").unwrap();
        let r = resolve_rows(
            &s,
            &[nr("Brave", Certainty::Guess, Some("com.brave.Browser"))],
        );
        assert!(!r[0].missing);
        assert_eq!(r[0].bundle_id.as_deref(), Some("com.brave.Browser"));
    }

    #[test]
    fn an_unanswered_name_is_unknown_not_missing() {
        let s = parse_shortcuts("\"ctrl+alt+b\" = \"Brave\"\n").unwrap();
        assert_eq!(
            resolve_rows(&s, &[])[0],
            Resolved {
                name: "Brave".into(),
                bundle_id: None,
                missing: false
            }
        );
    }

    #[test]
    fn a_chain_that_does_not_split_is_missing_under_its_whole_text() {
        let s = parse_shortcuts("\"ctrl+alt+b\" = \"Brave ||\"\n").unwrap();
        assert_eq!(
            resolve_rows(&s, &[])[0],
            Resolved {
                name: "Brave ||".into(),
                bundle_id: None,
                missing: true
            }
        );
    }

    const HOLD: Chord = Chord {
        ctrl: true,
        super_: true,
        alt: true,
    };

    fn table() -> Vec<Shortcut> {
        parse_shortcuts(
            "\"ctrl+super+alt+c\" = \"Claude\"\n\"ctrl+super+alt+h\" = \"Hermes\"\n\"ctrl+super+alt+shift+m\" = \"Gmail\"\n",
        )
        .unwrap()
    }

    fn resolved() -> Vec<Resolved> {
        vec![
            Resolved {
                name: "Claude".into(),
                bundle_id: Some("com.anthropic.claudefordesktop".into()),
                missing: false,
            },
            Resolved {
                name: "Hermes".into(),
                bundle_id: None,
                missing: true,
            },
            Resolved {
                name: "Gmail".into(),
                bundle_id: None,
                missing: false,
            },
        ]
    }

    #[test]
    fn a_binding_row_carries_glyphs_words_icon_and_its_one_word() {
        let rows = binding_rows(&table(), &resolved(), &HashMap::new(), false, Some(HOLD));
        assert_eq!(rows[0].chord, "⇪C");
        assert_eq!(rows[0].spoken, "Claude, Caps + C");
        assert_eq!(
            rows[0].bundle_id.as_deref(),
            Some("com.anthropic.claudefordesktop")
        );
        assert_eq!(rows[0].flag, None);
        assert_eq!(rows[1].flag, Some("missing"));
        assert_eq!(rows[2].chord, "⌃⌥⇧⌘M", "a shift row does not fold");
    }

    #[test]
    fn a_refused_registration_is_in_use_and_outranks_missing() {
        let mut reg = HashMap::new();
        reg.insert("ctrl+super+alt+h".to_string(), Err("taken".to_string()));
        let rows = binding_rows(&table(), &resolved(), &reg, false, Some(HOLD));
        assert_eq!(rows[1].flag, Some("in use"));
    }

    #[test]
    fn a_paused_table_carries_no_words() {
        let rows = binding_rows(&table(), &resolved(), &HashMap::new(), true, Some(HOLD));
        assert!(rows.iter().all(|r| r.flag.is_none()));
    }

    #[test]
    fn an_empty_cache_still_draws_every_row_and_claims_nothing() {
        let rows = binding_rows(&table(), &[], &HashMap::new(), false, None);
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[1].name, "Hermes");
        assert!(rows
            .iter()
            .all(|r| r.flag.is_none() && r.bundle_id.is_none()));
        assert_eq!(rows[0].chord, "⌃⌥⌘C");
    }

    fn mac(rows: Vec<BindingRow>) -> MacMenu {
        MacMenu {
            phrase: "19 shortcuts registered".into(),
            paused: false,
            accessibility: true,
            failed: 0,
            log: None,
            rows,
        }
    }

    fn brow(name: &str, flag: Option<&'static str>) -> BindingRow {
        BindingRow {
            name: name.into(),
            chord: "⇪X".into(),
            spoken: format!("{name}, Caps + X"),
            flag,
            bundle_id: None,
        }
    }

    fn labels(v: &[MenuEntry]) -> Vec<&str> {
        v.iter().map(|e| e.label.as_str()).collect()
    }

    #[test]
    fn the_healthy_macos_menu_is_eight_rows_and_does_not_grow_with_the_file() {
        let small = build_mac_entries(&mac(vec![brow("kitty", None)]));
        let big = build_mac_entries(&mac((0..19)
            .map(|i| brow(&format!("app{i}"), None))
            .collect()));
        assert_eq!(labels(&small), labels(&big));
        assert_eq!(
            labels(&small),
            vec![
                "beckon",
                "",
                "Shortcuts",
                "",
                "Open Config",
                "Reload Config",
                "",
                "Quit beckon"
            ]
        );
    }

    #[test]
    fn the_header_carries_the_headline_and_the_switch_state() {
        let e = build_mac_entries(&MacMenu {
            paused: true,
            ..mac(vec![brow("kitty", None)])
        });
        let EntryKind::Header(h) = &e[0].kind else {
            panic!("first row is the header")
        };
        assert_eq!(e[0].id, MENU_STATUS);
        assert_eq!(h.subtitle, "Paused - shortcuts are off");
        assert!(!h.on, "paused means the switch is off");
    }

    #[test]
    fn needs_attention_appears_only_while_a_row_is_broken() {
        let e = build_mac_entries(&mac(vec![
            brow("kitty", None),
            brow("Hermes", Some("missing")),
        ]));
        let i = e
            .iter()
            .position(|x| x.kind == EntryKind::SectionHeader)
            .expect("section");
        assert_eq!(e[i].label, "Needs attention");
        assert_eq!(e[i + 1].id, MENU_BINDING_BASE + 1);
        assert_eq!(e[i + 1].flag, Some("missing"));
        assert!(e[i + 2].is_separator());
        assert!(build_mac_entries(&mac(vec![brow("kitty", None)]))
            .iter()
            .all(|x| x.kind != EntryKind::SectionHeader));
    }

    #[test]
    fn more_than_three_broken_rows_fold_into_one_more_row_that_opens_the_fourth() {
        let rows = (0..5)
            .map(|i| brow(&format!("app{i}"), Some("missing")))
            .collect();
        let e = build_mac_entries(&mac(rows));
        let i = e
            .iter()
            .position(|x| x.kind == EntryKind::SectionHeader)
            .unwrap();
        assert_eq!(e[i + 4].label, "and 2 more...");
        assert_eq!(e[i + 4].id, MENU_BINDING_BASE + 3);
    }

    #[test]
    fn pausing_hides_needs_attention() {
        let e = build_mac_entries(&MacMenu {
            paused: true,
            ..mac(vec![brow("Hermes", Some("missing"))])
        });
        assert!(e.iter().all(|x| x.kind != EntryKind::SectionHeader));
    }

    #[test]
    fn every_binding_is_in_the_submenu_in_file_order() {
        let e = build_mac_entries(&mac(vec![brow("kitty", None), brow("Claude", None)]));
        let sub = e.iter().find(|x| x.id == MENU_SHORTCUTS).unwrap();
        assert_eq!(sub.kind, EntryKind::Submenu);
        assert_eq!(
            labels(&sub.children),
            vec!["kitty", "Claude", "", "Edit Shortcuts..."]
        );
        assert_eq!(sub.children[1].id, MENU_BINDING_BASE + 1);
        assert_eq!(sub.children[1].detail.as_deref(), Some("⇪X"));
        assert_eq!(sub.children[1].tooltip.as_deref(), Some("Claude, Caps + X"));
        assert_eq!(sub.children[3].id, MENU_EDIT_SHORTCUTS);
    }

    #[test]
    fn an_empty_table_still_offers_edit_shortcuts_without_a_stray_separator() {
        let e = build_mac_entries(&mac(vec![]));
        let sub = e.iter().find(|x| x.id == MENU_SHORTCUTS).unwrap();
        assert_eq!(labels(&sub.children), vec!["Edit Shortcuts..."]);
    }

    #[test]
    fn open_log_is_there_only_when_this_run_has_a_log() {
        let without = build_mac_entries(&mac(vec![]));
        assert!(without.iter().all(|x| x.id != MENU_LOG));
        let with = build_mac_entries(&MacMenu {
            log: Some(true),
            ..mac(vec![])
        });
        let reload = with.iter().position(|x| x.id == MENU_RELOAD).unwrap();
        assert_eq!(with[reload + 1].label, "Open Log");
    }

    #[test]
    fn the_menu_rows_keep_their_command_key_equivalents() {
        let e = build_mac_entries(&mac(vec![]));
        let key = |id| e.iter().find(|x| x.id == id).unwrap().key;
        assert_eq!(
            (key(MENU_EDIT), key(MENU_RELOAD), key(MENU_QUIT)),
            (Some(','), Some('r'), Some('q'))
        );
    }

    fn walk(v: &[MenuEntry], f: &mut dyn FnMut(&[MenuEntry])) {
        f(v);
        for e in v {
            if !e.children.is_empty() {
                walk(&e.children, f);
            }
        }
    }

    #[test]
    fn no_macos_menu_level_starts_or_ends_on_a_rule_or_doubles_one() {
        for paused in [false, true] {
            for log in [None, Some(true)] {
                for rows in [
                    vec![],
                    vec![brow("a", None)],
                    vec![brow("a", Some("missing"))],
                ] {
                    let e = build_mac_entries(&MacMenu {
                        paused,
                        log,
                        ..mac(rows)
                    });
                    walk(&e, &mut |lvl| {
                        assert!(!lvl.first().unwrap().is_separator());
                        assert!(!lvl.last().unwrap().is_separator());
                        assert!(!lvl
                            .windows(2)
                            .any(|w| w[0].is_separator() && w[1].is_separator()));
                    });
                }
            }
        }
    }

    #[test]
    fn no_macos_row_collides_with_a_reserved_id_or_another_row() {
        let rows = (0..25)
            .map(|i| brow(&format!("app{i}"), (i % 2 == 0).then_some("missing")))
            .collect();
        let e = build_mac_entries(&mac(rows));
        let mut seen = std::collections::HashMap::new();
        walk(&e, &mut |lvl| {
            for x in lvl
                .iter()
                .filter(|x| !x.is_separator() && x.kind != EntryKind::SectionHeader)
            {
                assert_ne!(x.id, beckon_core::menu::MENU_ID_DOUBLE_CLICK);
                assert_ne!(x.id, beckon_core::menu::MENU_ID_ALT_CLICK);
                if let Some(prev) = seen.insert(x.id, x.label.clone()) {
                    let folded = prev.starts_with("and ") || x.label.starts_with("and ");
                    assert!(prev == x.label || folded, "id {} is two rows", x.id);
                }
            }
        });
    }
}
