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
//! `RefCell` borrow may be held across a call to `backend.beckon()`.** The
//! backend therefore lives OUTSIDE the `RefCell` (a separate `Rc`, cloned
//! into the hotkey closure), and `on_hotkey` takes only a short borrow to
//! clone what it needs, releasing it before calling the backend. Holding
//! `state.borrow()` across that call would let a reentrant `reload()` call
//! `state.borrow_mut()` and panic (`BorrowMutError`) while unwinding across
//! an `extern "system"` boundary — which aborts the whole process, not just
//! the callback. Harmless-but-cheap to apply on macOS too, since nothing
//! there ever re-enters.
//!
//! `reload()` itself is *not* restructured the same way: it holds
//! `mgr.borrow_mut()` and `state.borrow()`/`borrow_mut()` live across
//! `register_all()`. That is sound only because of a SECOND guarantee this
//! file does not itself provide: `beckon_windows::hotkey::dispatch_tick`
//! takes `TICK_CBS` out (`mem::take`) before invoking any tick callback, so
//! a nested/reentrant tick delivered while `reload()` is still running sees
//! an *empty* callback table and is a no-op rather than a second concurrent
//! call into `reload()`. That makes tick delivery — and therefore
//! `reload()` — effectively non-reentrant on Windows, unlike hotkey
//! delivery. This is a cross-crate dependency: `beckon_windows::hotkey`
//! currently documents that take-then-run behavior as a "limitation," but
//! for this file it is load-bearing. Changing it there (e.g. re-adding a
//! tick to the live table before running callbacks) would reopen the same
//! class of bug this module doc describes above, at `reload()` instead of
//! `on_hotkey`.

use anyhow::{anyhow, Context, Result};
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
/// log. Windows has the tray tooltip; macOS has nowhere to put it, and the
/// LaunchAgent's stderr already goes to a file launchd owns.
#[cfg(target_os = "windows")]
fn set_tray_status(text: &str) {
    hotkey::set_status(text);
}
#[cfg(not(target_os = "windows"))]
fn set_tray_status(_text: &str) {}

/// Capability + values needed to offer "Start with Windows", present only
/// when this process is one whose own path is safe to bake into the Run
/// key.
///
/// `ServeState::autostart` is `None` on the CLI path (`beckon.exe serve`):
/// `std::env::current_exe()` there resolves to `beckon.exe`, which has no
/// bare `serve`-with-no-argument form, so a Run value pointing at it would
/// exit via `arg_required_else_help` at next logon and never start
/// anything. It is `Some` only from `serve_app_main` (`beckon-serve.exe`),
/// which knows its own exe is a valid target. The tray menu reads
/// `Option::is_some()` to decide whether to show the row at all — see
/// `build_entries` — never to decide whether it's *ticked*, which comes
/// from `autostart::is_enabled()` regardless.
///
/// `config`/`log` are `Some` only when they differ from the defaults —
/// see `run_key_command_line`. Set on every platform (`cmd_serve_app`
/// takes it unconditionally) but only read by the Windows-only tray menu
/// below, so non-Windows builds see the whole type as write-only.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub struct AutostartCapability {
    pub config: Option<PathBuf>,
    pub log: Option<PathBuf>,
}

struct ServeState {
    shortcuts: Vec<Shortcut>,
    /// The `keyboard` block. `caps`/`caps_tap`/`caps_hold` today, and only
    /// Windows acts on them — but the file is parsed identically everywhere
    /// so one config can travel between machines.
    #[cfg_attr(not(target_os = "windows"), allow(dead_code))]
    keyboard: KeyboardConfig,
    config: PathBuf,
    /// Hotkeys deliberately unregistered from the tray menu. A reload while
    /// paused updates the table but must not re-register — a file save is
    /// not a request to un-pause.
    paused: bool,
    /// Where stderr went, when it went to a file. `None` leaves the menu's
    /// "Open log" greyed out rather than lying. Set on every platform
    /// (`cmd_serve_app` takes it unconditionally) but only read by the
    /// Windows-only tray menu below, so non-Windows builds see it as
    /// write-only.
    #[cfg_attr(not(target_os = "windows"), allow(dead_code))]
    log: Option<PathBuf>,
    /// The most recent `registration_phrase`, so the menu can show it
    /// without re-running a registration pass.
    last_phrase: String,
    /// Canonical combo -> last registration outcome. Read by the settings
    /// window so each row can show whether its key actually took. Cleared
    /// when paused, because nothing is registered then and a stale tick
    /// would claim otherwise. Set on every platform (`register_all` is
    /// shared) but only read by the Windows-only window, so non-Windows
    /// builds see it as write-only.
    #[cfg_attr(not(target_os = "windows"), allow(dead_code))]
    registered: std::collections::HashMap<String, Result<(), String>>,
    /// See `AutostartCapability`.
    #[cfg_attr(not(target_os = "windows"), allow(dead_code))]
    autostart: Option<AutostartCapability>,
    /// The settings window's model, present only while it is open. The
    /// window itself is stateless about content — it draws whatever
    /// `control_state` projects out of this.
    #[cfg_attr(not(target_os = "windows"), allow(dead_code))]
    settings: Option<beckon_core::settings::Model>,
    /// The window is open against a file that did not parse: the
    /// explanation, already in the window's own vocabulary
    /// (`explain_unreadable`). Computed once, where both the file text and
    /// the parser's message are in hand, because the window is pushed on
    /// every tick and re-deriving it there would re-read the file.
    ///
    /// **This and `settings` are one enum written as two fields**, and only
    /// three of the four combinations exist:
    ///
    /// | `settings` | this | meaning |
    /// |---|---|---|
    /// | `None` | `None` | the window is closed |
    /// | `Some` | `None` | open, editable |
    /// | `None` | `Some` | open, read only |
    /// | `Some` | `Some` | impossible -- `load_settings_model` sets both |
    ///
    /// Two fields rather than an enum because every callback in
    /// `open_settings` reaches for `settings.as_mut()`, and a real enum
    /// would put a match in each of them for a variant none of them can act
    /// on.
    #[cfg_attr(not(target_os = "windows"), allow(dead_code))]
    settings_unreadable: Option<Vec<beckon_core::settings::Note>>,
    /// Installed app names for the window's combo box. `None` until the
    /// worker thread reports, which is NOT the same as "nothing installed"
    /// — `control_state` renders the two differently on purpose.
    #[cfg_attr(not(target_os = "windows"), allow(dead_code))]
    catalog: Option<Vec<String>>,
    /// The file changed underneath an unsaved window. Shows the banner; the
    /// user chooses, beckon does not.
    #[cfg_attr(not(target_os = "windows"), allow(dead_code))]
    external_change: bool,
}

/// Take the single-instance lock, preserving the error's type.
///
/// Plain `?`, never `anyhow!(e)`: the error must reach `main` as a typed
/// `AcquireError` so `is_expected` can recognise the healthy "already
/// running" refusal and stay quiet. Flattening it to a message would put
/// every watchdog tick back in the notification centre — silently, since
/// nothing else in the program would change. `lock_error_keeps_its_type`
/// below is what stops that. (Wrapping with `.context()` is fine; anyhow
/// downcasts through context layers. Only stringifying breaks it.)
///
/// Extracted from `cmd_serve` purely so a test can reach it: `cmd_serve`
/// itself goes on to install a hotkey manager and never returns.
fn acquire_lock(config: &Path) -> Result<std::fs::File> {
    Ok(crate::lockfile::acquire(config)?)
}

pub fn cmd_serve(config: &Path, log: Option<PathBuf>) -> Result<()> {
    cmd_serve_app(config, log, None)
}

/// Shared implementation for both Windows front doors (`cmd_serve`, the CLI
/// path, and `serve_app_main`, the tray app) and for macOS `serve`.
///
/// `log` tells the tray menu's "Open log" where to point. `autostart` is
/// `None` when this process cannot meaningfully offer "Start with
/// Windows" — always true on macOS, and true for the CLI path on Windows
/// too — or `Some` with the Run-value overrides otherwise. See
/// `AutostartCapability`.
pub fn cmd_serve_app(
    config: &Path,
    log: Option<PathBuf>,
    autostart: Option<AutostartCapability>,
) -> Result<()> {
    let _lock = acquire_lock(config)?;
    let config = config
        .canonicalize()
        .with_context(|| format!("cannot resolve `{}`", config.display()))?;
    let text = std::fs::read_to_string(&config)
        .with_context(|| format!("cannot read `{}`", config.display()))?;
    let parsed = parse_config(&text).map_err(|e| anyhow!("{}: {}", config.display(), e))?;
    let shortcuts = parsed.shortcuts;
    // Outside the RefCell on purpose — see module doc.
    let backend: Rc<Box<dyn Backend>> = Rc::new(crate::pick_backend()?);

    let state = Rc::new(RefCell::new(ServeState {
        shortcuts,
        keyboard: parsed.keyboard,
        config: config.clone(),
        paused: false,
        log,
        last_phrase: String::new(),
        registered: Default::default(),
        autostart,
        settings: None,
        settings_unreadable: None,
        catalog: None,
        external_change: false,
    }));

    let mgr = {
        let st = Rc::clone(&state);
        let be = Rc::clone(&backend);
        HotkeyManager::install(Box::new(move |id| on_hotkey(&st, &be, id)))
            .map_err(|e| anyhow!(e))?
    };
    let mgr = Rc::new(RefCell::new(mgr));
    let outcome = register_all(&mut mgr.borrow_mut(), &state.borrow().shortcuts);

    let (tx, rx) = std::sync::mpsc::channel::<()>();
    let _watcher = watch_config(&config, tx)?; // lives as long as the loop below
    {
        let st = Rc::clone(&state);
        let mg = Rc::clone(&mgr);
        // Must run after `install` above: on Windows, add_tick registers a
        // window timer (SetTimer) against the hwnd install() creates (the
        // tray window), which does not exist yet before that call.
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

    #[cfg(target_os = "windows")]
    install_tray_menu(&state, &mgr);

    let phrase = registration_phrase(outcome.ok, state.borrow().shortcuts.len());
    state.borrow_mut().last_phrase = phrase.clone();
    state.borrow_mut().registered = outcome.by_canonical();
    sync_caps_hook(&state);
    eprintln!("beckon serve: {} from {}", phrase, config.display());
    set_tray_status(&format!("beckon - {phrase}"));
    if let Some(toast) = failure_toast(&outcome.failed) {
        crate::notify::report(&toast, crate::notify::Cause::MachineRepeat);
    }
    HotkeyManager::run_forever();
}

fn on_hotkey(state: &Rc<RefCell<ServeState>>, backend: &Rc<Box<dyn Backend>>, id: u32) {
    // Short borrow only: cloned out and dropped BEFORE calling the backend
    // below. See the module doc — on Windows, backend.beckon() can pump
    // this thread's message queue, letting reload() run reentrantly while
    // this function is still on the stack.
    let (app, canonical) = {
        let st = state.borrow();
        match st.shortcuts.get(id as usize) {
            Some(sc) => (sc.app.clone(), sc.combo.canonical()),
            None => return,
        }
    };
    if let Err(e) = backend.beckon(&app) {
        let msg = format!("{app} ({canonical}): {e}");
        eprintln!("beckon serve: {msg}");
        // A key the owner just pressed. Told every time, including the fifth,
        // because they pressed it a fifth time.
        crate::notify::report(&msg, crate::notify::Cause::HumanAction);
    }
}

/// Outcome of a registration pass: how many keys actually registered vs.
/// how many were attempted. Incident 2026-08-09: the startup log reported
/// the count of PARSED entries, not successful registrations — it said
/// "20 shortcuts registered" while 0/20 had actually registered, and the
/// outage stayed invisible for hours. Callers must report `ok`, not the
/// length of the input slice.
struct RegisterOutcome {
    ok: usize,
    failed: Vec<String>, // canonical combos, in registration order
    /// Every attempted combo by its canonical spelling, with what happened.
    /// Canonical rather than as-written because the settings window joins
    /// on this key and the file may spell the same combo differently.
    results: Vec<(String, Result<(), String>)>,
}

impl RegisterOutcome {
    fn by_canonical(&self) -> std::collections::HashMap<String, Result<(), String>> {
        self.results.iter().cloned().collect()
    }
}

/// "shortcut" for 1, "shortcuts" otherwise. The only pluralization this
/// file needs, so a lookup, not a framework.
fn shortcut_noun(n: usize) -> &'static str {
    if n == 1 {
        "shortcut"
    } else {
        "shortcuts"
    }
}

/// "N shortcuts" / "1 shortcut" -- the count phrase `reload`'s paused
/// branch and `set_paused` both build around "paused (...)".
fn shortcuts_count_phrase(n: usize) -> String {
    format!("{n} {}", shortcut_noun(n))
}

/// "20 shortcuts registered" when clean; "17 of 20 shortcuts registered
/// (3 failed)" otherwise. Singular at total == 1: "1 shortcut registered",
/// "0 of 1 shortcut registered (1 failed)" -- the noun agrees with the
/// total, not the leading number, so it stays singular even when `ok` is 0.
fn registration_phrase(ok: usize, total: usize) -> String {
    let failed = total - ok;
    let noun = shortcut_noun(total);
    if failed == 0 {
        format!("{total} {noun} registered")
    } else {
        format!("{ok} of {total} {noun} registered ({failed} failed)")
    }
}

/// One toast line summarizing a failure wave; `None` when nothing failed.
/// Incident 2026-08-09: each failed key used to fire its own toast, so a
/// bad config reload spammed 20 separate notifications. This collapses a
/// whole wave into a single line, listing up to 5 combos by name and
/// folding the rest into a count.
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
                // One broken key loses one key, never the whole table.
                // Per-key eprintln kept for the detailed log; the toast is
                // collapsed into a single summary by the caller instead of
                // firing here (see `failure_toast`).
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

/// Re-read `config`, replace the shortcut table, and (unless paused)
/// re-register every hotkey.
///
/// **Borrow safety**: the `Ok` arm below holds `mgr.borrow_mut()` live
/// across two calls to `set_tray_status` (Windows: `hotkey::set_status`,
/// which bottoms out in `Shell_NotifyIconW(NIM_MODIFY)`). That is sound:
/// `NIM_MODIFY` is an in-process icon update, not the out-of-process shell
/// activation `ShellExecuteW` performs, so it does not pump this thread's
/// message queue and cannot let a reentrant call back into this same
/// `RefCell` while it is already borrowed -- the `BorrowMutError`-across-
/// `extern "system"` hazard the module doc above describes. Contrast
/// `beckon_windows::shell::open_path` (`ShellExecuteW`), which DOES pump:
/// see `install_tray_menu`, where every call site clones what it needs and
/// drops its borrow first, before calling it. `set_paused` holds the same
/// two borrows across the same non-pumping calls, for the same reason.
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
            // The watcher drives this, not a person: anything that keeps
            // rewriting the config (an editor sync, a `home-manager switch`
            // loop) would otherwise post once per tick, forever.
            crate::notify::report(&msg, crate::notify::Cause::MachineRepeat);
            // A settings window open READ ONLY is the one thing that wants
            // to hear about a failed reload: it is showing an explanation of
            // exactly this, and the user is editing the file to make it go
            // away. Restricted to that state on purpose -- calling the full
            // external-change path from here would hand an EDITABLE window a
            // file that does not parse, and its answer to that is an error
            // dialog, once per watcher tick.
            #[cfg(target_os = "windows")]
            settings_retry_unreadable(state);
        }
        Ok(new) => {
            let mut m = mgr.borrow_mut();
            m.unregister_all();
            {
                let mut s = state.borrow_mut();
                s.shortcuts = new.shortcuts;
                s.keyboard = new.keyboard;
            }
            let paused = state.borrow().paused;
            if paused {
                // A file save is not a request to un-pause. The table is
                // updated so resuming picks up the edit; nothing registers.
                let phrase = shortcuts_count_phrase(state.borrow().shortcuts.len());
                state.borrow_mut().last_phrase = phrase.clone();
                // unregister_all() above means nothing is registered; a
                // leftover map would show ticks for keys that are not live.
                state.borrow_mut().registered.clear();
                // Paused means paused: the hook stays off even though the
                // edit may have turned `keyboard.caps` on. Resuming picks
                // it up.
                sync_caps_hook(state);
                eprintln!("beckon serve: reloaded while paused - {phrase}");
                set_tray_status(&format!("beckon - paused ({phrase})"));
                return;
            }
            let outcome = register_all(&mut m, &state.borrow().shortcuts);
            let phrase = registration_phrase(outcome.ok, state.borrow().shortcuts.len());
            state.borrow_mut().last_phrase = phrase.clone();
            state.borrow_mut().registered = outcome.by_canonical();
            sync_caps_hook(state);
            eprintln!("beckon serve: reloaded - {phrase}");
            set_tray_status(&format!("beckon - {phrase}"));
            if let Some(toast) = failure_toast(&outcome.failed) {
                crate::notify::report(&toast, crate::notify::Cause::MachineRepeat);
            }
            // The file on disk just changed. A clean settings window follows
            // it silently; a dirty one raises a banner and lets the user
            // choose. No-op when the window is closed.
            #[cfg(target_os = "windows")]
            settings_saw_external_change(state);
        }
    }
}

// ---------------------------------------------------------------------------
// Tray menu (Windows only). macOS `serve` runs under launchd with no tray.
// ---------------------------------------------------------------------------

#[cfg(target_os = "windows")]
const MENU_STATUS: u32 = 1;
#[cfg(target_os = "windows")]
const MENU_EDIT: u32 = 2;
#[cfg(target_os = "windows")]
const MENU_RELOAD: u32 = 3;
#[cfg(target_os = "windows")]
const MENU_LOG: u32 = 4;
#[cfg(target_os = "windows")]
const MENU_PAUSE: u32 = 5;
#[cfg(target_os = "windows")]
const MENU_AUTOSTART: u32 = 6;
#[cfg(target_os = "windows")]
const MENU_QUIT: u32 = 7;

/// Everything the menu needs to draw itself, snapshotted out of `ServeState`
/// so the drawing is a pure function and can be tested without a tray, a
/// message loop or a registry.
#[cfg(target_os = "windows")]
#[derive(Clone)]
struct MenuModel {
    phrase: String,
    paused: bool,
    /// `None`: omit the "Start with Windows" row entirely -- this process
    /// cannot offer it (see `AutostartCapability`). `Some(checked)`: show
    /// it, ticked per `checked`. Omitted rather than shown disabled: a
    /// permanently greyed row invites "why is this greyed?" with no answer
    /// available in the menu itself.
    autostart: Option<bool>,
    has_log: bool,
}

#[cfg(target_os = "windows")]
fn build_entries(m: &MenuModel) -> Vec<hotkey::MenuEntry> {
    use hotkey::MenuEntry;
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
        },
        MenuEntry::separator(),
        MenuEntry {
            id: MENU_EDIT,
            label: "Settings...".into(),
            checked: None,
            enabled: true,
        },
        MenuEntry {
            id: MENU_RELOAD,
            label: "Reload now".into(),
            checked: None,
            enabled: true,
        },
        MenuEntry {
            id: MENU_LOG,
            label: "Open log".into(),
            checked: None,
            enabled: m.has_log,
        },
        MenuEntry::separator(),
        MenuEntry {
            id: MENU_PAUSE,
            label: "Pause hotkeys".into(),
            checked: Some(m.paused),
            enabled: true,
        },
    ];
    if let Some(checked) = m.autostart {
        entries.push(MenuEntry {
            id: MENU_AUTOSTART,
            label: "Start with Windows".into(),
            checked: Some(checked),
            enabled: true,
        });
    }
    entries.push(MenuEntry::separator());
    entries.push(MenuEntry {
        id: MENU_QUIT,
        label: "Quit".into(),
        checked: None,
        enabled: true,
    });
    entries
}

#[cfg(target_os = "windows")]
fn install_tray_menu(state: &Rc<RefCell<ServeState>>, mgr: &Rc<RefCell<HotkeyManager>>) {
    let st_build = Rc::clone(state);
    let build = Box::new(move || {
        let s = st_build.borrow();
        build_entries(&MenuModel {
            phrase: s.last_phrase.clone(),
            paused: s.paused,
            // `is_enabled()` is the ticked state; whether the row shows AT
            // ALL is a different question, answered by `autostart.is_some()`
            // (see `AutostartCapability`) -- capability, not registry state.
            autostart: s
                .autostart
                .as_ref()
                .map(|_| beckon_windows::autostart::is_enabled()),
            has_log: s.log.is_some(),
        })
    });

    let st = Rc::clone(state);
    let mg = Rc::clone(mgr);
    let on_click = Box::new(move |id: u32| {
        match id {
            // ShellExecuteW pumps this thread's queue, so the path is cloned
            // out and every borrow is dropped BEFORE the call -- the same
            // rule the module doc states for backend.beckon().
            MENU_EDIT | hotkey::MENU_ID_DOUBLE_CLICK => open_settings(&st),
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
                let result = if beckon_windows::autostart::is_enabled() {
                    beckon_windows::autostart::disable()
                } else {
                    match std::env::current_exe() {
                        Ok(exe) => {
                            let exe = crate::serve_app::scoop_current_path(&exe);
                            let s = st.borrow();
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
                    // The user just clicked this; tell them every time.
                    crate::notify::report(
                        &format!("could not change autostart: {e}"),
                        crate::notify::Cause::HumanAction,
                    );
                }
            }
            MENU_QUIT => {
                eprintln!("beckon serve: quit requested from the tray menu");
                hotkey::request_quit();
            }
            _ => {}
        }
    });

    hotkey::set_menu(build, on_click);
}

// ---------------------------------------------------------------------------
// Caps hook (Windows only)
// ---------------------------------------------------------------------------

/// Install, refresh or remove the Caps hook so it matches the current
/// config. Called at startup, after every reload, and on both edges of
/// pause.
///
/// Measured on a14 2026-08-11, which is what makes the alias design safe:
/// an injected `ctrl+win+alt+<key>` burst does fire our own
/// `RegisterHotKey`; the burst does not open the Start menu (verified
/// against a control that proved a bare Win tap does); and the injection
/// costs 5-13 ms against a 300 ms `LowLevelHooksTimeout`.
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
        if caps_hook::is_installed() {
            caps_hook::uninstall();
            eprintln!("beckon serve: caps hook removed");
        }
        return;
    }

    let keys = bound.len();
    caps_hook::set_bindings(bound, hold, tap);
    if caps_hook::is_installed() {
        return;
    }
    match caps_hook::install() {
        Ok(()) => eprintln!("beckon serve: caps hook active, {keys} keys reachable through Caps"),
        Err(e) => {
            eprintln!("beckon serve: {e}");
            // The user just ticked this; tell them every time.
            crate::notify::report(
                &format!("could not enable Caps Lock: {e}"),
                crate::notify::Cause::HumanAction,
            );
            // Do not leave the config claiming a feature that is not
            // running. The file still says `caps = true`; this only stops
            // the in-memory state from lying to the settings window.
            state.borrow_mut().keyboard.caps = false;
        }
    }
}

#[cfg(not(target_os = "windows"))]
fn sync_caps_hook(_state: &Rc<RefCell<ServeState>>) {}

// ---------------------------------------------------------------------------
// Settings window (Windows only)
// ---------------------------------------------------------------------------

/// Recompute what the window should show and push it. Every callback ends
/// here; nothing else touches the controls.
#[cfg(target_os = "windows")]
fn refresh_settings(state: &Rc<RefCell<ServeState>>) {
    let s = state.borrow();
    // Two projections, one push. The read-only one has no `Model` behind it
    // -- `Model::from_text` failed -- which is exactly why `unreadable_state`
    // exists: the window is given a `ControlState` either way and never
    // learns that there are two ways to arrive at one.
    let cs = if let Some(model) = s.settings.as_ref() {
        let rt = beckon_core::settings::RuntimeStatus {
            registered: s.registered.clone(),
            catalog: s.catalog.clone(),
            // Pausing CLEARS `registered`, so without this the window would
            // show every row as "not registered yet" and never say why.
            paused: s.paused,
        };
        beckon_core::settings::control_state(model, &rt)
    } else if let Some(notes) = s.settings_unreadable.as_ref() {
        beckon_core::settings::unreadable_state(notes.clone())
    } else {
        // The window is closed.
        return;
    };
    let external = s.external_change;
    let catalog = s.catalog.clone();
    drop(s);
    beckon_windows::settings_window::apply_state(&cs, external, catalog.as_deref());
}

/// Read the config file into whichever of the window's two states it
/// deserves: a `Model` when it parses, the read-only explanation when it
/// does not.
///
/// **A file that does not parse is not an error here, and that is the whole
/// point of this function.** `open_settings` used to refuse outright --
/// *"Fix it in a text editor first"* -- which told someone who has never
/// seen TOML to go and do the thing the window exists to save them from.
/// beckon still never writes over a file it cannot read; it just says so
/// with the file open in front of them.
///
/// `Err` is reserved for a file that could not be READ at all -- deleted,
/// locked, permission denied. There is nothing to show for that, so the
/// caller reports it and does not open.
#[cfg(target_os = "windows")]
fn load_settings_model(state: &Rc<RefCell<ServeState>>) -> Result<(), String> {
    let path = state.borrow().config.clone();
    let text = std::fs::read_to_string(&path)
        .map_err(|e| format!("Cannot read {}:\n\n{e}", path.display()))?;
    let parsed = beckon_core::settings::Model::from_text(&text);
    let mut s = state.borrow_mut();
    match parsed {
        Ok(m) => {
            s.settings = Some(m);
            s.settings_unreadable = None;
        }
        Err(e) => {
            s.settings = None;
            s.settings_unreadable = Some(beckon_core::settings::explain_unreadable(&text, &e));
        }
    }
    s.external_change = false;
    Ok(())
}

/// The window is closing, or failed to open. Both halves of the
/// `settings` / `settings_unreadable` pair go, together: leaving the
/// explanation behind would make `settings_saw_external_change` believe a
/// window is still open and push into a destroyed one.
#[cfg(target_os = "windows")]
fn forget_settings(state: &Rc<RefCell<ServeState>>) {
    let mut s = state.borrow_mut();
    s.settings = None;
    s.settings_unreadable = None;
}

/// Write the model to disk, atomically.
///
/// Deliberately no direct `reload()` call afterwards: `watch_config` fires
/// on the rename and the 1 Hz tick reloads within a second. A shortcut path
/// here would buy under a second at the cost of a second code path, and the
/// watcher would run anyway.
#[cfg(target_os = "windows")]
fn apply_settings(state: &Rc<RefCell<ServeState>>) {
    let rendered = {
        let s = state.borrow();
        let Some(model) = s.settings.as_ref() else {
            return;
        };
        model.render().map(|t| (t, s.config.clone()))
    };
    let (text, path) = match rendered {
        Ok(v) => v,
        Err(e) => {
            beckon_windows::settings_window::error(&format!("Cannot save:\n\n{e}"));
            return;
        }
    };
    // Temp-then-rename: a crash or a full disk must not destroy a working
    // config, and a rename is the write shape `watch_config` was built for
    // -- it watches the parent directory by file name precisely because
    // editors replace files that way.
    let tmp = path.with_extension("toml.beckon-tmp");
    let wrote = std::fs::write(&tmp, &text).and_then(|()| std::fs::rename(&tmp, &path));
    if let Err(e) = wrote {
        let _ = std::fs::remove_file(&tmp);
        beckon_windows::settings_window::error(&format!("Cannot write {}:\n\n{e}", path.display()));
        return;
    }
    // The model is now what is on disk, so re-seed it from the text we just
    // wrote: that clears `dirty` and gives every row a fresh `orig_key`.
    let mut s = state.borrow_mut();
    if let Ok(m) = beckon_core::settings::Model::from_text(&text) {
        let selected = s.settings.as_ref().and_then(|old| old.selected);
        s.settings = Some(m);
        if let Some(m) = s.settings.as_mut() {
            m.selected = selected.filter(|i| *i < m.rows.len());
        }
    }
    s.external_change = false;
    drop(s);
    refresh_settings(state);
    eprintln!("beckon serve: settings saved");
}

/// Load the model from disk into the window, discarding in-memory edits.
#[cfg(target_os = "windows")]
fn reload_settings_from_disk(state: &Rc<RefCell<ServeState>>) {
    let path = state.borrow().config.clone();
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) => {
            beckon_windows::settings_window::error(&format!(
                "Cannot read {}:\n\n{e}",
                path.display()
            ));
            return;
        }
    };
    match beckon_core::settings::Model::from_text(&text) {
        Ok(m) => {
            let mut s = state.borrow_mut();
            s.settings = Some(m);
            // Keeps the pair's invariant unconditional rather than relying
            // on this path being unreachable from the read-only state (the
            // banner that owns `Reload` is hidden there).
            s.settings_unreadable = None;
            s.external_change = false;
        }
        // Deliberately NOT the read-only state: `Reload` means "discard my
        // in-memory edits", and there IS a model here to discard. Dropping
        // it for a file that no longer parses would throw away work the user
        // can still save once they fix the file. The dialog says so and the
        // model stays.
        Err(e) => {
            beckon_windows::settings_window::error(&format!(
                "{} is not valid:\n\n{e}",
                path.display()
            ));
            return;
        }
    }
    refresh_settings(state);
}

/// The file changed while the window is open READ ONLY: try it again.
///
/// There is no model here, so there is nothing to lose and nothing to ask
/// about -- which is why this needs neither the banner nor a prompt. It is
/// what makes the read-only notes' own advice true: fix the file in a text
/// editor and the window turns editable by itself, with no reopen.
///
/// Silent either way, deliberately. On success the window simply becomes
/// editable; on a second failure the notes are replaced with the new
/// explanation. A dialog per write, from an editor that saves as you type,
/// would be unusable.
///
/// Returns `false` when the window is not in that state, so callers that
/// have their own answer for the editable window can carry on.
#[cfg(target_os = "windows")]
fn settings_retry_unreadable(state: &Rc<RefCell<ServeState>>) -> bool {
    if state.borrow().settings_unreadable.is_none() {
        return false;
    }
    let _ = load_settings_model(state);
    refresh_settings(state);
    true
}

/// Called from `reload()` after the file changed on disk. A clean window
/// follows the file silently; a dirty one raises the banner and lets the
/// user choose. beckon never picks for them.
///
/// A read-only window is neither: it has no model to follow the file WITH
/// and no edits to protect, so it goes through `settings_retry_unreadable`
/// and never reaches the dirty test below.
#[cfg(target_os = "windows")]
fn settings_saw_external_change(state: &Rc<RefCell<ServeState>>) {
    if settings_retry_unreadable(state) {
        return;
    }
    let dirty = match state.borrow().settings.as_ref() {
        Some(m) => m.dirty(),
        None => return,
    };
    if dirty {
        state.borrow_mut().external_change = true;
        refresh_settings(state);
    } else {
        reload_settings_from_disk(state);
    }
}

/// Scan the installed-app catalog off the UI thread.
///
/// `scan_installed_apps` was measured at ~370-500 ms and `run_forever`'s
/// message loop is the same thread that dispatches `WM_HOTKEY`; scanning
/// inline would stall every hotkey for half a second each time the window
/// opens. The worker gets its own STA -- an MTA worker would be handed a
/// marshalling proxy back to the host apartment and serialise anyway.
#[cfg(target_os = "windows")]
fn spawn_catalog_scan(target: beckon_windows::settings_window::WindowHandle) {
    std::thread::spawn(move || {
        let names: Vec<String> = beckon_windows::apps::scan_installed_apps()
            .into_iter()
            .map(|a| a.name)
            .collect();
        beckon_windows::settings_window::post_catalog(target, names);
    });
}

#[cfg(target_os = "windows")]
fn open_settings(state: &Rc<RefCell<ServeState>>) {
    use beckon_windows::settings_window as win;

    // Already open: raise it, do not build a second model.
    if win::hwnd().is_some() {
        let _ = win::open_existing();
        return;
    }

    // A file that does not parse opens READ ONLY rather than being refused.
    // Only a file that cannot be read at all stops us here.
    if let Err(e) = load_settings_model(state) {
        win::error(&e);
        return;
    }

    // One helper per callback so the borrow discipline is written once:
    // mutate under a short borrow_mut, drop it, then refresh.
    macro_rules! edit {
        ($st:expr, $body:expr) => {{
            let st = Rc::clone($st);
            move |arg| {
                {
                    let mut s = st.borrow_mut();
                    if let Some(m) = s.settings.as_mut() {
                        #[allow(clippy::redundant_closure_call)]
                        ($body)(m, arg);
                    }
                }
                refresh_settings(&st);
            }
        }};
    }

    let cb = win::Callbacks {
        on_select: Box::new(edit!(state, |m: &mut beckon_core::settings::Model, i| {
            m.selected = Some(i);
        })),
        // Two arguments, so not `edit!` -- same discipline written out:
        // mutate under a short borrow_mut, drop it, then refresh.
        on_mark: Box::new({
            let st = Rc::clone(state);
            move |i: usize, on: bool| {
                {
                    let mut s = st.borrow_mut();
                    if let Some(m) = s.settings.as_mut() {
                        // `set_marked` indexes `rows` directly, and a panic
                        // here would unwind out of a wndproc callback.
                        if i < m.rows.len() {
                            m.set_marked(i, on);
                        }
                    }
                }
                refresh_settings(&st);
            }
        }),
        on_edit_combo: Box::new(edit!(
            state,
            |m: &mut beckon_core::settings::Model, t: String| {
                if let Some(i) = m.selected {
                    m.set_combo(i, &t);
                }
            }
        )),
        on_edit_app: Box::new(edit!(
            state,
            |m: &mut beckon_core::settings::Model, t: String| {
                if let Some(i) = m.selected {
                    m.set_app(i, &t);
                }
            }
        )),
        on_caps: Box::new(edit!(
            state,
            |m: &mut beckon_core::settings::Model, on: bool| m.set_caps(on)
        )),
        on_caps_tap: Box::new(edit!(state, |m: &mut beckon_core::settings::Model, t| m
            .set_caps_tap(t))),
        on_add: Box::new({
            let st = Rc::clone(state);
            move || {
                {
                    let mut s = st.borrow_mut();
                    if let Some(m) = s.settings.as_mut() {
                        m.add_row();
                    }
                }
                refresh_settings(&st);
            }
        }),
        on_remove: Box::new({
            let st = Rc::clone(state);
            move || {
                {
                    let mut s = st.borrow_mut();
                    if let Some(m) = s.settings.as_mut() {
                        if let Some(i) = m.selected {
                            m.remove_row(i);
                        }
                    }
                }
                refresh_settings(&st);
            }
        }),
        on_apply: Box::new({
            let st = Rc::clone(state);
            move || apply_settings(&st)
        }),
        on_catalog: Box::new({
            let st = Rc::clone(state);
            move |names: Vec<String>| {
                st.borrow_mut().catalog = Some(names);
                refresh_settings(&st);
            }
        }),
        on_open_file: Box::new({
            let st = Rc::clone(state);
            move || {
                // ShellExecuteW pumps this thread's queue, so the path is
                // cloned out and the borrow dropped BEFORE the call -- the
                // rule this module's doc states for backend.beckon().
                let p = st.borrow().config.clone();
                if let Err(e) = beckon_windows::shell::open_path(&p) {
                    eprintln!("beckon serve: {e}");
                }
            }
        }),
        on_reload_from_disk: Box::new({
            let st = Rc::clone(state);
            move || reload_settings_from_disk(&st)
        }),
        on_keep_mine: Box::new({
            let st = Rc::clone(state);
            move || {
                st.borrow_mut().external_change = false;
                refresh_settings(&st);
            }
        }),
        on_close_request: Box::new({
            let st = Rc::clone(state);
            move || {
                // A read-only window has no model, so `dirty` is false and
                // this is the arm it leaves by: no save prompt for changes
                // that could not have been made.
                let dirty = st
                    .borrow()
                    .settings
                    .as_ref()
                    .map(|m| m.dirty())
                    .unwrap_or(false);
                if !dirty {
                    forget_settings(&st);
                    return true;
                }
                match beckon_windows::shell::ask_save(
                    "beckon",
                    "Save your changes to the shortcuts file?",
                ) {
                    beckon_windows::shell::SaveChoice::Save => {
                        apply_settings(&st);
                        // Only leave if the write actually succeeded --
                        // apply_settings clears `dirty` by reseeding the
                        // model, so a still-dirty model means it failed and
                        // the user's edits are only in memory.
                        let still_dirty = st
                            .borrow()
                            .settings
                            .as_ref()
                            .map(|m| m.dirty())
                            .unwrap_or(false);
                        if !still_dirty {
                            forget_settings(&st);
                        }
                        !still_dirty
                    }
                    beckon_windows::shell::SaveChoice::Discard => {
                        forget_settings(&st);
                        true
                    }
                    beckon_windows::shell::SaveChoice::Cancel => false,
                }
            }
        }),
    };

    // The path is what names the window (`beckon - shortcuts.toml`) and what
    // its `Open config file` tooltip shows. Handed over once, at open: it is
    // `ServeState::config`, which nothing can repoint while the window is up.
    let path = state.borrow().config.clone();
    if let Err(e) = win::open(cb, &path.to_string_lossy()) {
        eprintln!("beckon serve: cannot open settings: {e}");
        beckon_windows::settings_window::error(&format!("Cannot open settings:\n\n{e}"));
        forget_settings(state);
        return;
    }
    if let Some(h) = win::hwnd() {
        spawn_catalog_scan(win::WindowHandle(h));
    }
    refresh_settings(state);
}

/// Unregister or re-register every hotkey, and say so in the tooltip.
///
/// Neither `unregister_all` nor `register_all` pumps the message queue, and
/// neither does the `hotkey::set_status` call below -- see `reload`'s doc
/// comment for why holding `state`/`mgr` borrows across a tooltip update is
/// sound while holding them across `beckon_windows::shell::open_path`
/// (`ShellExecuteW`) is not.
#[cfg(target_os = "windows")]
fn set_paused(state: &Rc<RefCell<ServeState>>, mgr: &Rc<RefCell<HotkeyManager>>, paused: bool) {
    let mut m = mgr.borrow_mut();
    if paused {
        m.unregister_all();
        state.borrow_mut().paused = true;
        // Not `last_phrase` verbatim: that string is a *registration*
        // phrase ("N shortcuts registered") left over from before this
        // unregister_all, and reusing it here would claim registration
        // while nothing is registered. Match reload()'s honest "N
        // shortcuts" spelling for the paused case instead, and update
        // `last_phrase` to it so the menu head (`build_entries`) agrees.
        let phrase = shortcuts_count_phrase(state.borrow().shortcuts.len());
        state.borrow_mut().last_phrase = phrase.clone();
        state.borrow_mut().registered.clear();
        // Pausing MUST unhook. Leaving it installed would keep swallowing
        // Caps while nothing acts on it -- the worst available state.
        sync_caps_hook(state);
        eprintln!("beckon serve: paused - {phrase}");
        hotkey::set_status(&format!("beckon - paused ({phrase})"));
    } else {
        state.borrow_mut().paused = false;
        let outcome = register_all(&mut m, &state.borrow().shortcuts);
        let phrase = registration_phrase(outcome.ok, state.borrow().shortcuts.len());
        state.borrow_mut().last_phrase = phrase.clone();
        state.borrow_mut().registered = outcome.by_canonical();
        sync_caps_hook(state);
        eprintln!("beckon serve: resumed - {phrase}");
        hotkey::set_status(&format!("beckon - {phrase}"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsStr;

    // Incident 2026-08-09: the startup log said "20 shortcuts registered"
    // while 0/20 keys had actually registered, because the count came from
    // the parsed list, not from `HotkeyManager::register`'s outcome. These
    // two pure helpers turn `RegisterOutcome` into the strings the log and
    // the toast use, so the phrasing itself is unit-testable without a real
    // `HotkeyManager`.

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
        // The noun agrees with the total (1), not the leading number (0):
        // this must read as real English, not "0 of 1 shortcuts".
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

    /// The load-bearing `?` in `acquire_lock`.
    ///
    /// Swap it for `map_err(|e| anyhow!("{e}"))` and everything still
    /// compiles, every other test still passes, and the watchdog storm comes
    /// straight back — the type is the only thing carrying "this is expected".
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
        // Against the *canonical* spelling: that is what `acquire` hashes and
        // what it reports, and on Windows canonicalizing turns `C:\...` into
        // the extended-length `\\?\C:\...` form, so comparing with the path as
        // written passes on Unix and fails there.
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

    #[test]
    fn event_touches_matches_by_file_name_only() {
        let name = Some(OsStr::new("apps.macos.toml"));
        assert!(event_touches(
            &[PathBuf::from("/x/y/apps.macos.toml")],
            name
        ));
        // Editors that write via a temp file + rename report sibling paths —
        // those must NOT trigger, the rename onto our name does.
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

    #[cfg(target_os = "windows")]
    #[test]
    fn menu_shows_the_phrase_and_reflects_pause() {
        let m = MenuModel {
            phrase: "5 shortcuts registered".into(),
            paused: false,
            autostart: Some(false),
            has_log: true,
        };
        let rows = build_entries(&m);
        assert_eq!(rows[0].label, "beckon - 5 shortcuts registered");
        assert!(!rows[0].enabled, "the status row is a label, not a button");
        let pause = rows.iter().find(|r| r.id == MENU_PAUSE).unwrap();
        assert_eq!(pause.checked, Some(false));

        // `..m.clone()`, not `..m`: Task 5 appends another case to this test
        // and needs `m` to still be alive.
        let paused = MenuModel {
            paused: true,
            ..m.clone()
        };
        let rows = build_entries(&paused);
        // Previously unverified despite the test's name: only the un-paused
        // head format was asserted above, so a regression in the paused
        // spelling (e.g. losing the "beckon - " prefix, or reusing a
        // registration phrase here -- see `set_paused`) would not have
        // been caught.
        assert_eq!(rows[0].label, "beckon - paused (5 shortcuts registered)");
        assert_eq!(
            rows.iter().find(|r| r.id == MENU_PAUSE).unwrap().checked,
            Some(true)
        );

        let on = MenuModel {
            autostart: Some(true),
            ..m.clone()
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

    /// Fix for the CRITICAL bug: the CLI path (`beckon.exe serve`) used to
    /// show "Start with Windows" unconditionally, and ticking it there
    /// wrote a Run value that could never start anything (see
    /// `AutostartCapability`). The row must not exist at all when the
    /// capability is absent -- disabled-and-unexplained is not an
    /// acceptable substitute for omitted.
    #[cfg(target_os = "windows")]
    #[test]
    fn autostart_row_exists_only_when_the_capability_does() {
        let base = MenuModel {
            phrase: "5 shortcuts registered".into(),
            paused: false,
            autostart: None,
            has_log: true,
        };
        assert!(
            build_entries(&base).iter().all(|r| r.id != MENU_AUTOSTART),
            "no row may exist when this process cannot offer autostart"
        );

        let with_capability = MenuModel {
            autostart: Some(true),
            ..base
        };
        let rows = build_entries(&with_capability);
        let hits: Vec<_> = rows.iter().filter(|r| r.id == MENU_AUTOSTART).collect();
        assert_eq!(hits.len(), 1, "exactly one Start with Windows row");
        assert_eq!(hits[0].checked, Some(true));
    }

    /// Moved here from Task 3, where the same intent could only be written
    /// as an assertion about a constant. Here it runs against the real
    /// entry list, so adding a menu row that collides with the reserved
    /// double-click id fails the build instead of silently making
    /// double-click fire that row.
    #[cfg(target_os = "windows")]
    #[test]
    fn no_real_entry_collides_with_the_reserved_double_click_id() {
        let m = MenuModel {
            phrase: "5 shortcuts registered".into(),
            paused: false,
            autostart: Some(false),
            has_log: true,
        };
        for row in build_entries(&m) {
            assert_ne!(
                row.id,
                hotkey::MENU_ID_DOUBLE_CLICK,
                "entry {:?} shadows the reserved double-click id",
                row.label
            );
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

    /// `register_all` is what actually fills `results`, and the settings
    /// window joins on the canonical spelling -- not on how the file wrote
    /// it. Pin that the key really is canonicalized.
    #[test]
    fn results_are_keyed_by_canonical_spelling_not_by_how_the_file_wrote_it() {
        let shortcuts =
            beckon_core::shortcuts::parse_shortcuts("\"alt+ctrl+t\" = \"Terminal\"\n").unwrap();
        assert_eq!(shortcuts[0].combo.canonical(), "ctrl+alt+t");
    }

    /// The row used to open Notepad; it now opens the settings window, and
    /// the label has to say so. Same id on purpose -- renaming the id would
    /// have broken the double-click alias that shares it.
    #[cfg(target_os = "windows")]
    #[test]
    fn the_first_action_row_opens_settings_not_notepad() {
        let m = MenuModel {
            phrase: "2 shortcuts registered".into(),
            paused: false,
            autostart: Some(false),
            has_log: true,
        };
        let rows = build_entries(&m);
        let edit = rows.iter().find(|r| r.id == MENU_EDIT).unwrap();
        assert_eq!(edit.label, "Settings...");
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn open_log_is_disabled_when_there_is_no_log() {
        let m = MenuModel {
            phrase: "0 shortcuts registered".into(),
            paused: false,
            autostart: Some(false),
            has_log: false,
        };
        let rows = build_entries(&m);
        assert!(!rows.iter().find(|r| r.id == MENU_LOG).unwrap().enabled);
    }
}
