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
use beckon_core::menu::MenuEntry;
use beckon_core::shortcuts::{parse_config, KeyboardConfig, Shortcut};
use beckon_core::Backend;
#[cfg(target_os = "macos")]
use beckon_macos::hotkey;
#[cfg(target_os = "macos")]
use beckon_macos::settings_window as swin;
#[cfg(target_os = "windows")]
use beckon_windows::hotkey;
#[cfg(target_os = "windows")]
use beckon_windows::settings_window as swin;
use hotkey::HotkeyManager;
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

/// Publish a one-line status where the user can see it without reading the
/// log: the tray tooltip on Windows, the menu bar item's tooltip on macOS.
///
/// On both, the same string is also the menu's first row, which is where it
/// is actually readable. Linux has no resident mode -- the compositor binds
/// the keys -- so there is nowhere to put it and nothing to put it for.
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

/// What a front door does when the config file reads but does not parse.
///
/// **The two front doors answer this differently on purpose.**
///
/// `beckon.exe serve` (`Refuse`) keeps the behaviour scripts already depend
/// on: the parse error goes to stderr, which it has, and the process exits
/// non-zero, which its caller can read. A Scheduled Task, a CI job or a shell
/// that today distinguishes "config broken" by that exit code must keep
/// doing so -- and `beckon check` is unaffected either way. Silently turning
/// that into a process that runs forever would be a breaking change nobody
/// asked for.
///
/// `beckon-serve.exe` (`ServeAnyway`) has neither: no console for the error
/// and no caller for the code. Measured on a14 2026-08-11, a broken config
/// there ended in a modal dialog with **no tray icon at all** -- which
/// stranded the one thing built for this situation, the read-only settings
/// window, behind a tray that was never installed. So it starts, registers
/// nothing, writes nothing, and says what is wrong in the tray and in the
/// window.
///
/// Both refuse a file that cannot be READ (deleted, locked, denied), on both
/// front doors: `load_settings_model` returns `Err` for that too, so
/// `open_settings` would refuse to open and a tray installed for its sake
/// could do nothing at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrokenConfig {
    Refuse,
    /// Constructed only by `serve_app_main` (`beckon-serve.exe`), which is
    /// Windows-only -- so the other two CI jobs see a variant nothing builds.
    /// Same annotation, and the same reasoning, as `ServeState::log`: the
    /// decision it selects is tested on all three jobs, only the caller is
    /// platform-bound. macOS `serve` runs under launchd with no tray and no
    /// settings window, so there would be nothing for it to rescue.
    #[cfg_attr(not(target_os = "windows"), allow(dead_code))]
    ServeAnyway,
}

/// What `cmd_serve_app` starts with.
///
/// Split out of `cmd_serve_app` so the decision is reachable from a test:
/// `cmd_serve_app` itself installs a hotkey manager and never returns, the
/// same reason `acquire_lock` is its own function.
struct Startup {
    shortcuts: Vec<Shortcut>,
    keyboard: KeyboardConfig,
    /// The parser's own message, when the file did not parse. `Some` means
    /// the two fields above are empty defaults and nothing will be
    /// registered -- there is no other way to reach that combination.
    broken: Option<String>,
}

/// Read the config text into a startup plan, or refuse.
///
/// `Err` carries the parser's message unprefixed, so the caller can put the
/// path in front of it and produce byte-for-byte the message `beckon serve`
/// has always printed for a broken file.
///
/// **The `ServeAnyway` arm discards the parsed keyboard block along with the
/// shortcuts**, and that is not laziness: a file that does not parse has no
/// trustworthy `keyboard.caps`, and `sync_caps_hook` reads exactly that field
/// to decide whether to install a low-level keyboard hook. Defaults keep the
/// hook off, which is the only safe reading of a file beckon could not
/// understand.
fn plan_startup(text: &str, on_broken: BrokenConfig) -> Result<Startup, String> {
    match parse_config(text) {
        Ok(cfg) => Ok(Startup {
            shortcuts: cfg.shortcuts,
            keyboard: cfg.keyboard,
            broken: None,
        }),
        Err(e) => match on_broken {
            BrokenConfig::Refuse => Err(e),
            BrokenConfig::ServeAnyway => Ok(Startup {
                shortcuts: Vec::new(),
                keyboard: KeyboardConfig::default(),
                broken: Some(e),
            }),
        },
    }
}

/// The status phrase while the config file does not parse.
///
/// Built out of the same pieces as every other status line -- `set_paused`'s
/// "paused (N shortcuts)" is the shape, and the bracket is
/// `registration_phrase`'s own words -- so the tray tooltip and the menu head
/// say "beckon - cannot read the config (0 shortcuts registered)" with no new
/// vocabulary to learn and no claim that anything is running.
///
/// ASCII, like every `serve` log line: Windows PowerShell 5.1's
/// `Get-Content` defaults to ANSI.
fn unreadable_phrase() -> String {
    format!("cannot read the config ({})", registration_phrase(0, 0))
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
    /// "Open log" greyed out rather than lying, and is what the settings
    /// window's `Paths::log` carries so the System page can omit the row
    /// instead of showing a path that does not exist. Set on every platform
    /// that compiles this module (`cmd_serve_app` takes it unconditionally)
    /// and read on both of them: by the Windows-only tray menu below, and by
    /// `open_settings`, which the two windowed platforms share.
    ///
    /// **CORRECTED 2026-08-14: no build of this struct sees the field as
    /// write-only.** The paragraph above used to end "-- so a Linux build
    /// sees it as write-only", and the field carried
    /// `#[cfg_attr(not(any(target_os = "windows", target_os = "macos")),
    /// allow(dead_code))]` to match. The shape looked right because every
    /// neighbouring field here really is write-only somewhere and carries
    /// the same attribute one platform narrower -- `keyboard` above is read
    /// only by `sync_caps_hook`, and that read sits under
    /// `#[cfg(target_os = "windows")]`, so on macOS the field genuinely is
    /// written and never read. `log` is not in that position, on either
    /// count. `lib.rs:12-13` is
    /// `#[cfg(any(target_os = "macos", target_os = "windows"))] mod serve;`,
    /// so a Linux build never compiles `ServeState` at all and the
    /// `not(any(..))` predicate was unsatisfiable everywhere the field
    /// exists; and `open_settings` is gated `any(windows, macos)` and reads
    /// `s.log` when it builds `Paths`, so the field is live on both
    /// platforms that do compile it. Falsified by deleting the attribute and
    /// running `cargo clippy -p beckon-cli --all-targets -- -D warnings` on
    /// macOS, which stays clean. That covers the macOS arm only -- there is
    /// no Windows host here -- but the Windows arm is the one with the extra
    /// reads (`install_tray_menu`, below), so it cannot be the arm that
    /// warns.
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
    /// The last availability probe, and the chord it was about. `None` until
    /// one runs -- not-yet-probed is not the same as free -- and `None` again
    /// after a save, because `register_all` is the authority from that moment
    /// on and the window already hears its answer through `registered`; also
    /// `None` again the moment the selection moves to a different row, because
    /// a verdict is about the row it was requested for and carrying it to
    /// another row can only ever be wrong -- see `Callbacks::on_select`.
    ///
    /// Written by `probe_shortcut`, read by `refresh_settings`, and only ever
    /// on Windows: the settings window is the one thing that asks.
    #[cfg_attr(not(target_os = "windows"), allow(dead_code))]
    probe: Option<beckon_core::settings::ProbeResult>,
    /// The door the settings window was last showing, so the next open lands
    /// where the user left off.
    ///
    /// **Outlives the window on purpose.** It is not part of the window's own
    /// state -- the window is closed most of the time and forgets everything
    /// when it is -- and it is not in the config file either: which page
    /// someone was reading is a session fact, not a setting, and writing it
    /// to disk would mark a file dirty for a mouse click.
    ///
    /// `Page::default()` is `Shortcuts`, which is where a fresh `serve`
    /// opens. Written by `SettingsCommand::ShowPage`, read by
    /// `open_settings`; both are gated `any(windows, macos)`, and macOS
    /// currently discards the page it is handed, so this is live on Windows
    /// and merely honest on macOS.
    ///
    /// **It is a MIRROR, and it is allowed to lag the window.** `ShowPage`
    /// arrives through `with_cb`, which is take-then-run: a command raised
    /// while the callbacks are out of the slot is silently dropped, and they
    /// are out whenever a callback is running. `shell::ask_save` and
    /// `shell::error_dialog` call `MessageBoxW` with **no owner**
    /// (`shell.rs`), so the settings window stays enabled behind the box and a
    /// pill click there is dispatched from inside `on_close_request` -- one
    /// `ShowPage` raised into an empty slot, one door the mirror never hears
    /// about.
    ///
    /// **CORRECTED 2026-08-14.** This read "it is also the CURRENT door while
    /// the window is open, not only the last one, and `apply_settings` depends
    /// on that", and named `apply_settings` as a second writer. Both stopped
    /// being true when `save_press` was reverted: nothing reads this while the
    /// window is up any more, `SettingsCommand::ShowPage` is the only writer
    /// again, and the whole consequence of a dropped one is that the NEXT open
    /// lands on the door before last. Left as a mirror rather than re-derived
    /// from the window on demand, because a session's last-read page is worth
    /// exactly that much. Anything that starts making decisions from this
    /// field has to ask the window instead.
    settings_page: beckon_core::settings::Page,
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
    // `Refuse`: this front door has a console to print the parse error to and
    // a caller to hand the exit code to. See `BrokenConfig`.
    cmd_serve_app(config, log, None, BrokenConfig::Refuse)
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
    on_broken: BrokenConfig,
) -> Result<()> {
    let _lock = acquire_lock(config)?;
    // `canonicalize` is what makes every later consumer agree about which file
    // this is; `paths::plain` is what keeps the answer in a spelling a person
    // and the shell both accept. On Windows `canonicalize` is
    // `GetFinalPathNameByHandleW` and always returns `\\?\C:\...`, which leaked
    // to the startup log, the `Open config file` tooltip and the System page's
    // config row — and, less visibly, to `ShellExecuteW` and `explorer /select,`
    // behind that row's two glyphs, which are documented not to take it.
    //
    // **This is the only site that needs it, and deliberately not the one
    // inside `lockfile::acquire`.** The lock runs on the ORIGINAL `&Path` one
    // line above, does its own `canonicalize`, and hashes the result into the
    // lock file's NAME (`stable_id`). Simplifying there would rename the lock,
    // so an old and a new binary would stop seeing each other's lock and both
    // would serve — the exact failure `stable_id`'s module doc records.
    let config = beckon_core::paths::plain(
        config
            .canonicalize()
            .with_context(|| format!("cannot resolve `{}`", config.display()))?,
    );
    let text = std::fs::read_to_string(&config)
        .with_context(|| format!("cannot read `{}`", config.display()))?;
    // The path prefix is applied HERE rather than inside `plan_startup`, so
    // the refusal is byte-for-byte the message `beckon serve` has always
    // printed for a file that does not parse.
    let plan =
        plan_startup(&text, on_broken).map_err(|e| anyhow!("{}: {}", config.display(), e))?;
    let broken = plan.broken;
    // Outside the RefCell on purpose — see module doc.
    let backend: Rc<Box<dyn Backend>> = Rc::new(crate::pick_backend()?);

    let state = Rc::new(RefCell::new(ServeState {
        shortcuts: plan.shortcuts,
        keyboard: plan.keyboard,
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
        probe: None,
        settings_page: beckon_core::settings::Page::default(),
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

    #[cfg(any(target_os = "windows", target_os = "macos"))]
    install_tray_menu(&state, &mgr);

    // The parse error itself goes to the log first, in the same shape
    // `reload` logs one, so the log says WHAT is wrong and not merely that
    // something is. `broken` is `Some` only under `BrokenConfig::ServeAnyway`
    // -- the other front door returned above.
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
    eprintln!("beckon serve: {} from {}", phrase, config.display());
    set_tray_status(&format!("beckon - {phrase}"));
    if let Some(toast) = failure_toast(&outcome.failed) {
        crate::notify::report(&toast, crate::notify::Cause::MachineRepeat);
    }
    if broken.is_some() {
        // A tray icon is passive, and this process may have been started by
        // the Run key rather than by a person -- so say once that the keys
        // are gone, and where to look. `MachineRepeat` for the same reason
        // `reload`'s failure arm uses it: a logon loop must not become a
        // notification loop.
        crate::notify::report(
            "no hotkeys registered - beckon cannot read the config. \
             Open Settings from the tray icon to see why.",
            crate::notify::Cause::MachineRepeat,
        );
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
    // Through the ladder, not straight at the backend: a `serve` shortcut
    // whose value is a chain must fall through to the next candidate exactly
    // as a Linux compositor binding does. Same string, same rule, one
    // implementation -- and `verbose: false` because `serve` reports through
    // the log and the notification below, never to a terminal.
    if let Err(e) = crate::beckon_ladder(backend.as_ref().as_ref(), &app, false) {
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

/// Write or clear the `HKCU\…\Run` value, and say so if it fails.
///
/// **One function, two call sites**: the tray menu's `Start with Windows` row
/// and the System page's switch (design §3.3). The command line a Run value
/// has to carry -- the scoop `current` junction rather than a version
/// directory, the config path, the `--log` -- is `serve_app`'s policy, and
/// two callers building it separately is exactly how one of them ends up
/// writing a value that starts a binary that no longer exists.
///
/// **`on` is the state wanted, not a toggle.** The menu row has no state of
/// its own and passes `!is_enabled()`; the window's switch has already
/// flipped itself and passes what it now shows. A function that toggled
/// would make the second caller's own state a lie whenever the two
/// disagreed -- which is the ordinary case the moment anything else writes
/// that key, and Task Manager's Startup tab is exactly such a thing.
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
        // The user just clicked this; tell them every time.
        crate::notify::report(
            &format!("could not change autostart: {e}"),
            crate::notify::Cause::HumanAction,
        );
    }
}

/// macOS has no autostart to set: login lifecycle belongs to `brew services`
/// / launchd there, and beckon must not write a competing LaunchAgent behind
/// it (`MenuModel::autostart` is `None` for the same reason). The System page
/// does not exist on that window either, so nothing can reach this -- it is
/// here because `on_command`'s `match` is compiled on both platforms and an
/// arm that only exists on one would be a `cfg` inside the callback.
#[cfg(all(target_os = "macos", not(target_os = "windows")))]
fn set_autostart(_state: &Rc<RefCell<ServeState>>, _on: bool) {}

/// Open one of the window's files -- or one of its three links -- with
/// whatever is registered for it.
///
/// **The three URL targets are handled here since 2026-08-15**, and this
/// comment used to say the opposite: "they belong to the About page (design
/// §3.4), which has no controls yet; answering them here would be dead code
/// whose only test is that it compiles, and a link that opens the wrong page
/// is worse than one that is not built ... whoever builds About owns turning
/// it into three." About is built, so this is that turn.
///
/// **The addresses are `Target::url`'s, in `beckon-core`**, not three string
/// literals here: they are checkable on all three CI jobs there, and the
/// `None` that function returns for a file is what picks the path branch
/// below -- so "is this a link or a file" is one decision rather than two
/// matches that agree today.
///
/// **Every borrow is dropped before the call.** `ShellExecuteW` performs an
/// out-of-process shell activation and pumps this thread's message queue, so
/// a live `RefCell` borrow across it is the abort-across-`extern "system"`
/// hazard this module's own doc describes. The path is cloned out first, the
/// same discipline `install_tray_menu`'s `MENU_LOG` arm already follows -- and
/// a URL needs no borrow at all.
#[cfg(target_os = "windows")]
fn open_target(state: &Rc<RefCell<ServeState>>, target: beckon_core::settings::Target) {
    use beckon_core::settings::Target;
    if let Some(url) = target.url() {
        if let Err(e) = beckon_windows::shell::open_url(url) {
            eprintln!("beckon serve: {e}");
        }
        return;
    }
    let path = match target {
        Target::Config => Some(state.borrow().config.clone()),
        Target::Log => state.borrow().log.clone(),
        // Unreachable: every target with no `url()` is one of the two above.
        // Spelled rather than `unreachable!()`, because a panic here would be
        // inside a settings-window callback, i.e. across an `extern "system"`
        // boundary, where it aborts the daemon instead of unwinding.
        _ => None,
    };
    if let Some(p) = path {
        if let Err(e) = beckon_windows::shell::open_path(&p) {
            eprintln!("beckon serve: {e}");
        }
    }
}

/// Show one of the window's files in Explorer, selected.
///
/// The second glyph on each file row. Same target set and same borrow
/// discipline as `open_target`, and it keeps that function's OLD silence
/// about the three URL targets even now that `open_target` has stopped
/// keeping it: there is nothing to reveal about a URL, and no control raises
/// `Reveal` for one -- About's links are `Open`.
#[cfg(target_os = "windows")]
fn reveal_target(state: &Rc<RefCell<ServeState>>, target: beckon_core::settings::Target) {
    use beckon_core::settings::Target;
    let path = match target {
        Target::Config => Some(state.borrow().config.clone()),
        Target::Log => state.borrow().log.clone(),
        _ => None,
    };
    if let Some(p) = path {
        if let Err(e) = beckon_windows::shell::reveal_path(&p) {
            eprintln!("beckon serve: {e}");
        }
    }
}

/// The macOS twin.
///
/// **These were `{}` until the four doors landed**, with a comment saying
/// "the macOS window has no System page, so neither command can arrive
/// there". It does now, and both commands do arrive: the System door's two
/// file rows raise `Open`/`Reveal`, and the About door's three links raise
/// `Open` with a URL target.
///
/// Same borrow discipline as the Windows pair and for a related reason: the
/// path is cloned out before the call, because `/usr/bin/open` is spawned
/// from inside a settings-window callback and anything that re-entered this
/// module with the `RefCell` still held would panic rather than unwind
/// cleanly.
#[cfg(all(target_os = "macos", not(target_os = "windows")))]
fn open_target(state: &Rc<RefCell<ServeState>>, target: beckon_core::settings::Target) {
    use beckon_core::settings::Target;
    if let Some(url) = target.url() {
        if let Err(e) = beckon_macos::shell::open_url(url) {
            eprintln!("beckon serve: {e}");
        }
        return;
    }
    let path = match target {
        Target::Config => Some(state.borrow().config.clone()),
        Target::Log => state.borrow().log.clone(),
        // Unreachable: every target with no `url()` is one of the two above.
        // Spelled out rather than `unreachable!()`, because this runs inside
        // an Objective-C message send and a panic there aborts the daemon
        // instead of unwinding.
        _ => None,
    };
    if let Some(p) = path {
        if let Err(e) = beckon_macos::shell::open_path(&p) {
            eprintln!("beckon serve: {e}");
        }
    }
}

#[cfg(all(target_os = "macos", not(target_os = "windows")))]
fn reveal_target(state: &Rc<RefCell<ServeState>>, target: beckon_core::settings::Target) {
    use beckon_core::settings::Target;
    let path = match target {
        Target::Config => Some(state.borrow().config.clone()),
        Target::Log => state.borrow().log.clone(),
        // Nothing to reveal about a URL, and no control raises `Reveal` for
        // one -- About's links are `Open`.
        _ => None,
    };
    if let Some(p) = path {
        if let Err(e) = beckon_macos::shell::reveal_path(&p) {
            eprintln!("beckon serve: {e}");
        }
    }
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
///
/// **REATTACHED 2026-08-15.** This block spent a day above `set_autostart`:
/// the System pass inserted that function, `open_target` and `reveal_target`
/// between this doc and its own item, so rustdoc read a borrow-safety
/// argument about `mgr.borrow_mut()` as documentation for a registry write
/// that takes no `mgr` at all -- while `reload`, which `set_paused`'s doc
/// cross-references BY NAME for exactly this reasoning ("see `reload`'s doc
/// comment"), had none. That cross-reference is what makes the orphaning a
/// defect rather than a tidiness point: it pointed at an empty place.
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
// Tray / menu bar menu.
//
// Composition lives here and is compiled by all three CI jobs; only the
// drawing is per-OS (`beckon_windows::hotkey`, `beckon_macos::tray`). Linux
// has no resident mode at all -- the compositor binds the keys -- so it
// builds this code without ever calling it, which is exactly what makes the
// tests below run there.
// ---------------------------------------------------------------------------

const MENU_STATUS: u32 = 1;
const MENU_EDIT: u32 = 2;
const MENU_RELOAD: u32 = 3;
const MENU_LOG: u32 = 4;
const MENU_PAUSE: u32 = 5;
const MENU_AUTOSTART: u32 = 6;
const MENU_QUIT: u32 = 7;

/// Everything the menu needs to draw itself, snapshotted out of `ServeState`
/// so the drawing is a pure function and can be tested without a tray, a
/// message loop or a registry.
///
/// The three `Option`/`bool` fields are all "does this row exist at all",
/// not "is it greyed". A permanently greyed row invites "why is this
/// greyed?" with no answer available in the menu itself, so a capability
/// this process does not have is omitted instead.
#[derive(Clone)]
struct MenuModel {
    phrase: String,
    paused: bool,
    /// `None`: omit "Start with Windows" -- this process cannot offer it
    /// (see `AutostartCapability`), which is always the case on macOS.
    /// `Some(checked)`: show it, ticked per `checked`.
    autostart: Option<bool>,
    /// `None`: omit "Open log". macOS has no `--log` (the flag is
    /// `#[cfg(target_os = "windows")]`) and does not own its log there --
    /// launchd writes it -- so the row could only ever be dead.
    /// `Some(enabled)`: show it, greyed when this run has no log file,
    /// which is Windows' way of not lying about where the log is.
    log: Option<bool>,
    /// Whether a settings window exists to open. False on macOS until that
    /// window is built; a row that opens nothing is worse than no row.
    settings: bool,
}

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
        },
        MenuEntry::separator(),
    ];
    if m.settings {
        entries.push(MenuEntry::item(MENU_EDIT, "Settings..."));
    }
    entries.push(MenuEntry::item(MENU_RELOAD, "Reload now"));
    if let Some(enabled) = m.log {
        entries.push(MenuEntry {
            id: MENU_LOG,
            label: "Open log".into(),
            checked: None,
            enabled,
        });
    }
    entries.extend([
        MenuEntry::separator(),
        MenuEntry {
            id: MENU_PAUSE,
            label: "Pause hotkeys".into(),
            checked: Some(m.paused),
            enabled: true,
        },
    ]);
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
            // Shown-but-greyed when this run has no log, rather than
            // omitted: on Windows beckon DOES own its log, so the row is
            // always meaningful and a missing one would be the lie.
            log: Some(s.log.is_some()),
            settings: true,
        })
    });

    let st = Rc::clone(state);
    let mg = Rc::clone(mgr);
    let on_click = Box::new(move |id: u32| {
        match id {
            // ShellExecuteW pumps this thread's queue, so the path is cloned
            // out and every borrow is dropped BEFORE the call -- the same
            // rule the module doc states for backend.beckon().
            MENU_EDIT | hotkey::MENU_ID_DOUBLE_CLICK => open_settings(&st, &mg),
            MENU_LOG => {
                let path = st.borrow().log.clone();
                if let Some(path) = path {
                    if let Err(e) = beckon_windows::shell::open_path(&path) {
                        eprintln!("beckon serve: {e}");
                    }
                }
            }
            // No `refresh_settings` here and that is not an omission: the
            // `Ok` arm of `reload` ends in `settings_saw_external_change`,
            // which is the RIGHT answer for a reload -- the file may have
            // moved under an open window, and a plain re-projection would
            // redraw a stale model against a fresh registration map. The
            // `Err` arm reaches `settings_retry_unreadable`. Both directions
            // are covered; see the two arms.
            MENU_RELOAD => reload(&st, &mg),
            // **The push back to an open settings window, added 2026-08-15.**
            // Pause and autostart are now on TWO surfaces -- this menu and
            // design §3.3's switches -- so without it the window goes on
            // showing the state the switch had before the tray changed it,
            // and the two disagree on screen with no way to tell which is
            // real. Pushed from here rather than pulled by the window: the
            // window has nothing to pull on (it runs no timer, and Windows
            // broadcasts nothing when beckon's own flag moves), so a pull
            // would mean a timer ticking for the ~always that the window is
            // closed. `refresh_settings` already returns immediately when
            // there is no window, so the push costs a `RefCell` read.
            //
            // AFTER the mutator and never inside it: `set_paused` returns
            // holding no borrow, and a `refresh_settings` called while
            // `mgr.borrow_mut()` was still live would put a `SendMessageW`
            // fan-out inside that borrow -- the shape this module's own doc
            // rules out. This is the same order `on_command`'s two arms use.
            MENU_PAUSE => {
                let now = !st.borrow().paused;
                set_paused(&st, &mg, now);
                refresh_settings(&st);
            }
            // The menu row is a TOGGLE with no state of its own, so what it
            // wants is the opposite of what the registry currently says. The
            // settings window's switch knows its own new state and passes it
            // straight through; both end in `set_autostart`, which is what
            // keeps "what a Run value looks like" a single answer.
            MENU_AUTOSTART => {
                set_autostart(&st, !beckon_windows::autostart::is_enabled());
                refresh_settings(&st);
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

/// The macOS menu bar item.
///
/// Four rows against Windows' seven, and each omission is structural rather
/// than a preference -- see `MenuModel`. Failure is logged and swallowed:
/// hotkeys are the feature and this is the control surface, so losing the
/// icon must not take the daemon with it. The same rule Windows applies to
/// a config that will not parse (`BrokenConfig::ServeAnyway`).
#[cfg(target_os = "macos")]
fn install_tray_menu(state: &Rc<RefCell<ServeState>>, mgr: &Rc<RefCell<HotkeyManager>>) {
    use beckon_macos::tray;

    let st_build = Rc::clone(state);
    let build = Box::new(move || {
        let s = st_build.borrow();
        build_entries(&MenuModel {
            phrase: s.last_phrase.clone(),
            paused: s.paused,
            // Login lifecycle belongs to `brew services` / launchd here, and
            // beckon must not write a competing LaunchAgent behind it.
            autostart: None,
            log: None,
            settings: true,
        })
    });

    let st = Rc::clone(state);
    let mg = Rc::clone(mgr);
    let on_click = Box::new(move |id: u32| match id {
        MENU_EDIT | beckon_core::menu::MENU_ID_DOUBLE_CLICK => open_settings(&st, &mg),
        MENU_RELOAD => reload(&st, &mg),
        // The Windows arm's push, and it earns its place here for a weaker
        // but real reason: this window has no System page and so no pause
        // SWITCH, but every Shortcuts row's status word is derived from
        // `RuntimeStatus::paused`, and `set_paused` clears `registered`. So
        // a pause from the menu bar leaves an open window claiming nineteen
        // rows are registered. `settings_saw_external_change` is Windows-only
        // and is about the FILE, so it does not cover this.
        MENU_PAUSE => {
            let now = !st.borrow().paused;
            set_paused(&st, &mg, now);
            refresh_settings(&st);
        }
        MENU_QUIT => {
            eprintln!("beckon serve: quit requested from the menu bar");
            tray::request_quit();
        }
        // MENU_LOG / MENU_AUTOSTART never reach here: those rows are not
        // built on macOS at all.
        _ => {}
    });

    if let Err(e) = tray::set_menu(build, on_click) {
        eprintln!("beckon serve: no menu bar item ({e}); hotkeys are unaffected");
    }
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
        // Forget the key set BEFORE giving up the reason, and unconditionally
        // -- dropping the reason is not enough to stop Caps aliasing.
        //
        // The hook is shared. A chord capture installs it too, and
        // `hook_proc`'s capture arm is gated on `armed() &&
        // GetForegroundWindow() == hwnd`, so a capture armed while the
        // settings window is not frontmost falls through to `caps::decide`
        // with whatever `CONFIG` was last handed it. Without this line the
        // set the user just switched off stays loaded, and `Caps+T` starts
        // aliasing again -- seconds at a time, whenever a shortcut is
        // recorded -- under a config that says Caps is off. Same for pause,
        // which reaches this branch by the same `want`.
        //
        // `clear_bindings` resets `tap` to `capslock` as well, so a
        // configured `caps_tap = "escape"` stops remapping the key the
        // moment the feature it belongs to is off.
        caps_hook::clear_bindings();
        // Drop the Caps reason unconditionally. `is_installed()` reports the
        // HHOOK, which a chord capture may also be holding, so it cannot
        // stand in for "Caps holds it"; the before/after pair is only for
        // the log line, which is about the hook going away.
        let was_installed = caps_hook::is_installed();
        caps_hook::uninstall_for(caps_hook::HookReason::Caps);
        if was_installed && !caps_hook::is_installed() {
            eprintln!("beckon serve: caps hook removed");
        }
        return;
    }

    let keys = bound.len();
    caps_hook::set_bindings(bound, hold, tap);
    // Deliberately NOT `if is_installed() { return; }`. A capture may be
    // holding the hook, and returning there would leave the Caps reason
    // unregistered -- so when the capture ended it would take the hook away
    // from a Caps feature that is switched on. `install_for` is idempotent;
    // only the log line needs to know whether this call did the installing.
    let was_installed = caps_hook::is_installed();
    match caps_hook::install_for(caps_hook::HookReason::Caps) {
        Ok(()) => {
            if !was_installed {
                eprintln!("beckon serve: caps hook active, {keys} keys reachable through Caps");
            }
        }
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
fn sync_caps_hook(state: &Rc<RefCell<ServeState>>) {
    use beckon_macos::caps_tap;

    let (want, tap, hold, bound) = {
        let s = state.borrow();
        (
            s.keyboard.caps && !s.paused,
            s.keyboard.caps_tap,
            s.keyboard.caps_hold,
            // The macOS projection of the same set: Carbon keycodes rather
            // than Win32 virtual keys. `bound_keys_mac` selects exactly the
            // bindings `bound_keys` does -- pinned by
            // `the_two_projections_select_the_same_bindings`.
            beckon_core::caps::bound_keys_mac(&s.registered, s.keyboard.caps_hold),
        )
    };

    if !want {
        // Forget the key set BEFORE giving up the tap, and unconditionally.
        // An empty set makes the callback pass every event through whatever
        // else is true, so the window between disabling and the run loop
        // noticing cannot eat a keystroke. `clear_bindings` resets `tap` to
        // `capslock` too, so a configured `caps_tap = "escape"` stops
        // remapping the key the moment the feature it belongs to is off.
        caps_tap::clear_bindings();
        let was = caps_tap::is_installed();
        caps_tap::uninstall();
        if was {
            eprintln!("beckon serve: caps event tap removed");
        }
        return;
    }

    let keys = bound.len();
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
            // The user just ticked this; tell them every time.
            crate::notify::report(
                &format!("could not enable Caps Lock: {e}"),
                crate::notify::Cause::HumanAction,
            );
            // Do not leave the in-memory state claiming a feature that is not
            // running. The file still says `caps = true`; this only stops the
            // settings window being lied to.
            state.borrow_mut().keyboard.caps = false;
        }
    }
}

// ---------------------------------------------------------------------------
// Settings window
//
// The window itself is per-OS (`beckon_windows::settings_window`,
// `beckon_macos::settings_window`), aliased to `swin`. Everything below is
// shared, because it is all policy and `beckon_core::settings` already owns
// the decisions. Four calls genuinely differ, and each gets a shim here
// rather than a `cfg` in the middle of `open_settings`.
// ---------------------------------------------------------------------------

/// What the user chose when asked about unsaved edits on close.
#[cfg(any(target_os = "windows", target_os = "macos"))]
enum SaveChoice {
    Save,
    Discard,
    Cancel,
}

#[cfg(any(target_os = "windows", target_os = "macos"))]
fn ask_save_changes() -> SaveChoice {
    #[cfg(target_os = "windows")]
    {
        use beckon_windows::shell;
        match shell::ask_save("beckon", "Save your changes to the shortcuts file?") {
            shell::SaveChoice::Save => SaveChoice::Save,
            shell::SaveChoice::Discard => SaveChoice::Discard,
            shell::SaveChoice::Cancel => SaveChoice::Cancel,
        }
    }
    #[cfg(target_os = "macos")]
    {
        match swin::ask_save("beckon", "Save your changes to the shortcuts file?") {
            swin::SaveChoice::Save => SaveChoice::Save,
            swin::SaveChoice::Discard => SaveChoice::Discard,
            swin::SaveChoice::Cancel => SaveChoice::Cancel,
        }
    }
}

/// Open the config file in the user's editor.
#[cfg(any(target_os = "windows", target_os = "macos"))]
fn open_config_file(p: &Path) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        beckon_windows::shell::open_path(p)
    }
    #[cfg(target_os = "macos")]
    {
        swin::open_path(p)
    }
}

/// Scan the installed-app catalog off the UI thread.
///
/// `scan_installed_apps` was measured at ~370-500 ms on Windows and the
/// macOS bundle walk is the same order; the run loop that would stall is
/// the one dispatching hotkeys, so this never runs inline.
#[cfg(any(target_os = "windows", target_os = "macos"))]
fn spawn_catalog_scan() {
    #[cfg(target_os = "windows")]
    {
        // The Windows window is told through a posted message, which needs
        // its handle. `None` means it closed between opening and here.
        if let Some(h) = swin::hwnd() {
            let target = swin::WindowHandle(h);
            std::thread::spawn(move || {
                let names: Vec<String> = beckon_windows::apps::scan_installed_apps()
                    .into_iter()
                    .map(|a| a.name)
                    .collect();
                swin::post_catalog(target, names);
            });
        }
    }
    #[cfg(target_os = "macos")]
    {
        // Deliberately synchronous for now. The macOS window has no
        // main-queue hop yet (see `tray.rs`'s module doc), and AppKit may
        // only be touched from the main thread -- so a worker here would
        // have nowhere to deliver its answer. Scanning inline costs the
        // window-open, not the hot path: `beckon <id>` never reaches this
        // code, and hotkeys are not dispatched while a menu action runs.
        //
        // This is the first caller that will actually need that hop, and
        // it is why the hop is specified rather than built: it lands here.
        let names: Vec<String> = beckon_macos::installed_app_names();
        swin::post_catalog(names);
    }
}

// ---------------------------------------------------------------------------
// Settings model plumbing
// ---------------------------------------------------------------------------

/// Recompute what the window should show and push it. Every callback ends
/// here; nothing else touches the controls.
#[cfg(any(target_os = "windows", target_os = "macos"))]
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
            // `row_condition` shows this on the selected row only, and only
            // while that row still spells the chord it was about -- so a
            // verdict the user has typed past disappears rather than being
            // shown against its replacement.
            probe: s.probe.clone(),
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
    // The System page's two facts, read in the SAME borrow as everything
    // above and used after it is dropped. `paused` is the flag `set_paused`
    // owns; `autostart` is capability first and state second -- `None` omits
    // the row entirely, and `is_enabled()` only decides the tick, which is
    // `MenuModel::autostart`'s own distinction and the reason it is spelled
    // the same way here.
    #[cfg(target_os = "windows")]
    let autostart = s
        .autostart
        .as_ref()
        .map(|_| beckon_windows::autostart::is_enabled());
    let paused = s.paused;
    drop(s);
    swin::apply_state(&cs, external, catalog.as_deref());
    // **A second push, and design §1's split by store is why.** The System
    // page writes `HKCU\Software\beckon`, the Run key, or nothing -- never
    // `apps.toml` -- so it must keep working in the one state
    // `unreadable_state` describes, where there is no `Model` to project a
    // `ControlState` out of at all. Riding on the projection above would have
    // made a theme switch hostage to a TOML error.
    //
    // **Both platforms now.** This read "Windows only, and not through a
    // `swin` alias: the macOS settings window has no System page, so a no-op
    // there would be a function whose only purpose is to be called." The
    // macOS window has all four doors as of the four-doors port, so the call
    // is real on both and the alias is the right way to reach it.
    //
    // `autostart` is the one argument that still differs, and it differs in
    // core rather than here: it is gathered only on Windows and is therefore
    // `None` on macOS, which is `SystemState::autostart`'s documented way of
    // saying "this process cannot offer autostart at all" -- so the row is
    // omitted rather than greyed. On macOS that is not a shortfall: the
    // Homebrew formula's `service do` block owns the launch agent, and a
    // switch here would be a second writer for a file beckon did not create.
    #[cfg(not(target_os = "windows"))]
    let autostart: Option<bool> = None;
    swin::apply_system_state(paused, autostart);
    // **A third push, and it takes no arguments at all.** Every string on the
    // About page is something only the settings window's own process can know
    // -- its compiled-in version, its target triple, its `current_exe()` and
    // the two timestamps behind the stale-image verdict -- so there is
    // nothing here to hand over, and anything this function did pass would be
    // a copy of a fact that crate reads directly.
    //
    // Called on every refresh rather than once at open, because one of those
    // strings genuinely moves: the file at the launch path can be replaced
    // while the window is up, which is the whole subject of the `Location`
    // row.
    //
    // Both platforms, for `apply_system_state`'s reason: the macOS window has
    // an About door now.
    swin::apply_about_state();
}

/// Decide whether `combo` is free for the row being edited, asking the OS
/// only when nothing beckon already knows can settle it, and keep the
/// verdict for the next push.
///
/// **Order, and why there is no `refresh_settings` here.** The window sends
/// this BEFORE `on_edit_combo` (see `Callbacks::on_probe_shortcut`), so the
/// model still holds the row's previous chord -- which is what makes
/// `probe_plan`'s `Unchanged` mean "this row already uses it" instead of
/// "the model was updated a moment ago". The push that draws the verdict is
/// therefore the one `on_edit_combo` does immediately afterwards, by which
/// time the row spells the probed chord and `row_condition` folds the note
/// in. Pushing here as well would draw a state where the two disagree.
///
/// **Every borrow is dropped before `probe_chord`.** `RegisterHotKey` is a
/// call into the OS from a wndproc callback, and a second `RefCell` borrow
/// taken across an `extern "system"` boundary aborts the process rather than
/// unwinding -- the rule this module's doc states for `backend.beckon()`.
///
/// **Known false alarm, from reading MSDN rather than from hardware --
/// narrowed, not gone.** `RegisterHotKey` refuses a chord that is *already
/// registered anywhere on this desktop*, and that includes beckon's own live
/// table on `tray_hwnd` -- the separate HWND only rules out an `(hWnd, id)`
/// identity collision, never a chord collision. The live table is the SAVED
/// file while `probe_plan` reads the EDITED model, so a chord `serve` holds
/// and no model row currently spells reaches the OS and comes back `Taken` --
/// "Another program already has this shortcut" -- about beckon itself.
///
/// `probe_plan` now settles two of the three cases without asking: a chord
/// another row still spells (step 4) and the edited row's OWN saved chord
/// (step 4b, which is what the edit-away-and-back sequence hits, since the
/// probe runs before `on_edit_combo` and the row therefore still holds its
/// previous chord). What remains is a chord some OTHER row was saved with and
/// has since been edited away from -- or whose row was deleted outright,
/// which leaves no `orig_key` anywhere in `m.rows` for step 4b to find at
/// all. Closing that needs the probe to read `ServeState::shortcuts`, which
/// is a policy §F.6 has no verdict or string for; it stays written up in the
/// task 2 report rather than guessed at here.
#[cfg(any(target_os = "windows", target_os = "macos"))]
fn probe_shortcut(state: &Rc<RefCell<ServeState>>, combo: String) {
    use beckon_core::settings::{ProbePlan, ProbeResult};

    // One borrow, taken and dropped before anything below touches the OS.
    let plan = {
        let s = state.borrow();
        let Some(m) = s.settings.as_ref() else {
            return; // read-only window, or no window at all
        };
        let Some(row) = m.selected else {
            return; // nothing is being edited, so there is nothing to ask about
        };
        beckon_core::settings::probe_plan(m, row, &combo)
    };
    let verdict = match plan {
        ProbePlan::Verdict(v) => v,
        #[cfg(target_os = "macos")]
        ProbePlan::AskTheOs => {
            // **No macOS probe, and this is now MEASURED rather than
            // cautious.** The comment here used to say the behaviour was
            // unmeasured and that assuming Carbon matched `RegisterHotKey`
            // was the kind of claim this repo has had to retract. It was
            // right, and `examples/hotkey_conflict_probe.rs` is why it can
            // stop being a hedge -- run in an Aqua session on 2026-08-16,
            // with the control first:
            //
            //   Ctrl+Cmd+Opt+F19            ACCEPTED   <- control: it works
            //   Ctrl+Cmd+Opt+F19 (again)    REFUSED    <- OSStatus -9878
            //   Cmd+Space   (Spotlight)     ACCEPTED
            //   Ctrl+Up     (Mission Ctrl)  ACCEPTED
            //
            // So `RegisterEventHotKey` refuses a duplicate **from the same
            // process** (`eventHotKeyExistsErr`) and accepts a chord another
            // application already owns. It cannot answer the question the
            // probe asks, and a successful registration would be a guess
            // dressed as a measurement -- the exact failure `probe_plan`
            // exists to prevent by asking the OS last.
            //
            // The same-process refusal is not a second-best signal either:
            // "another row in this file already uses it" is step four, which
            // core answers before this arm is ever reached.
            //
            // Leaving `probe` as it is renders "not yet probed", which
            // `row_condition` already distinguishes from "free". The five
            // steps BEFORE this one -- parse, the F12 guard, the row's own
            // chord, other rows in the file, the row's saved chord -- all
            // still run, and they are the ones that catch real mistakes.
            //
            // Do not re-open this without re-running that probe.
            return;
        }
        #[cfg(target_os = "windows")]
        ProbePlan::AskTheOs => {
            // The SETTINGS window's handle, never the tray's -- see
            // `hotkey::probe_chord`, which is where that rule and its reason
            // live. `None` means the window closed between the notification
            // and here, which leaves nothing to ask on and nobody to tell.
            let Some(h) = swin::hwnd() else {
                return;
            };
            // `AskTheOs` is only reachable for a chord `probe_plan` parsed,
            // so this cannot fail -- but a control is not a proof, and an
            // `expect` here would abort `serve` for a bad shortcut string.
            let Ok(c) = beckon_core::shortcuts::Combo::parse(&combo) else {
                return;
            };
            beckon_windows::hotkey::probe_chord(h, &c)
        }
    };
    state.borrow_mut().probe = Some(ProbeResult { combo, verdict });
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
#[cfg(any(target_os = "windows", target_os = "macos"))]
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
#[cfg(any(target_os = "windows", target_os = "macos"))]
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
///
/// **This function is the ONE funnel every Save goes through.** There are two
/// routes to it and only one is a button: `on_apply` (the Save press and the
/// `Ctrl+S` accelerator pointed at it), and `on_close_request`'s
/// `SaveChoice::Save` -- the prompt raised on the way out, which reaches this
/// without going near `IDC_APPLY`.
///
/// **REVERTED 2026-08-14: there is no external-change guard here any more.**
/// `aa9fbd6` put one in -- `save_press` refused the press once and switched the
/// window to the door the banner was drawn on -- because Task 4 had made the
/// announcement Shortcuts-only while Save stayed on all four pages. The
/// protection is real and is still owed; what changed is where it is paid. It
/// is paid in the WINDOW, by there being no door from which Save can be pressed
/// with nothing on screen saying the file moved -- so the funnel needs no
/// opinion about which door is open. That is one page-switch route removed
/// rather than one added, and the two defects the added route carried -- a
/// switch that moves no focus, and a switch that changes card geometry -- go
/// with it.
///
/// **AMENDED 2026-08-14, Task 6**, because the mechanism changed under this
/// paragraph and the conclusion did not. It read "`banner_shown` now draws the
/// announcement on EVERY page", which was the wide holding position. The
/// announcement is back on `BANNER_PAGE` alone, and what covers the other three
/// doors is the warn dot on the Shortcuts pill: `banner_shown` and
/// `warn_dot_shown` partition `external_change`, so exactly one of them is up
/// on any door and never neither. `settings::the_warning_is_on_screen_from_
/// every_door` is the assertion, and it is the reason this function still needs
/// no guard.
#[cfg(any(target_os = "windows", target_os = "macos"))]
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
            swin::error(&format!("Cannot save:\n\n{e}"));
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
        swin::error(&format!("Cannot write {}:\n\n{e}", path.display()));
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
    // The probe is superseded the moment the file is saved: the watcher and
    // the 1 Hz tick bring `reload` along in under a second, and
    // `register_all` is the authority on whether a chord took -- an answer
    // the window already receives through `registered`. Another process may
    // also have claimed the chord since the probe ran (the TOCTOU §F.6 names),
    // so a green "Available" that outlived the thing which would disprove it
    // is exactly the claim beckon must not make.
    s.probe = None;
    drop(s);
    refresh_settings(state);
    eprintln!("beckon serve: settings saved");
}

/// Load the model from disk into the window, discarding in-memory edits.
#[cfg(any(target_os = "windows", target_os = "macos"))]
fn reload_settings_from_disk(state: &Rc<RefCell<ServeState>>) {
    let path = state.borrow().config.clone();
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) => {
            swin::error(&format!("Cannot read {}:\n\n{e}", path.display()));
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
            swin::error(&format!("{} is not valid:\n\n{e}", path.display()));
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

#[cfg(any(target_os = "windows", target_os = "macos"))]
fn open_settings(state: &Rc<RefCell<ServeState>>, mgr: &Rc<RefCell<HotkeyManager>>) {
    use beckon_core::settings::SettingsCommand;

    // Already open: raise it, do not build a second model.
    if swin::is_open() {
        let _ = swin::open_existing();
        return;
    }

    // A file that does not parse opens READ ONLY rather than being refused.
    // Only a file that cannot be read at all stops us here.
    if let Err(e) = load_settings_model(state) {
        swin::error(&e);
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

    let cb = beckon_core::settings::Callbacks {
        // Not `edit!`: a probe verdict is about the row it was requested
        // for, and `row_condition` only re-checks the COMBO, not which row
        // asked -- two rows sharing a chord means a verdict stored for row A
        // still passes row B's `same_chord` gate once B is selected, and B
        // would be told about its own chord as though it belonged to
        // someone else. Clearing here, on every selection change, is what
        // keeps a verdict from ever outliving the row it answered for.
        on_select: Box::new({
            let st = Rc::clone(state);
            move |i: usize| {
                {
                    let mut s = st.borrow_mut();
                    if let Some(m) = s.settings.as_mut() {
                        m.selected = Some(i);
                    }
                    s.probe = None;
                }
                refresh_settings(&st);
            }
        }),
        // Two arguments, so not `edit!` -- same discipline written out:
        // mutate under a short borrow_mut, drop it, then refresh.
        on_mark: Box::new({
            let st = Rc::clone(state);
            move |i: usize, on: bool| {
                {
                    let mut s = st.borrow_mut();
                    if let Some(m) = s.settings.as_mut() {
                        // `set_marked` indexes `rows` directly. The window
                        // has already mapped the view row to a model row, so
                        // this guards a stale push rather than a filter.
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
        // Not `edit!`: this one must run OUTSIDE a `settings` borrow (it
        // calls into the OS) and must not refresh (the model has not caught
        // up with the chord yet). See `probe_shortcut`.
        on_probe_shortcut: Box::new({
            let st = Rc::clone(state);
            move |t: String| probe_shortcut(&st, t)
        }),
        on_edit_app: Box::new(edit!(
            state,
            |m: &mut beckon_core::settings::Model, t: String| {
                if let Some(i) = m.selected {
                    m.set_app(i, &t);
                }
            }
        )),
        on_filter: Box::new(edit!(
            state,
            |m: &mut beckon_core::settings::Model, t: String| m.set_filter(&t)
        )),
        on_caps: Box::new(edit!(
            state,
            |m: &mut beckon_core::settings::Model, on: bool| m.set_caps(on)
        )),
        on_caps_tap: Box::new(edit!(state, |m: &mut beckon_core::settings::Model, t| m
            .set_caps_tap(t))),
        // The return value says whether the model took the chord, and it is
        // deliberately dropped: `set_caps_hold` refuses one with no
        // modifiers, so unticking the last chip leaves the previous chord in
        // place and the `apply_state` that follows re-ticks the box. The
        // window needs no separate answer to hear that.
        on_caps_hold: Box::new(edit!(state, |m: &mut beckon_core::settings::Model, c| {
            m.set_caps_hold(c);
        })),
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
                        // Ticks first, selection as the fallback -- the whole
                        // decision is `Model::remove_pressed`, in beckon-core
                        // where all three CI jobs test it.
                        m.remove_pressed();
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
                if let Err(e) = open_config_file(&p) {
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
                match ask_save_changes() {
                    SaveChoice::Save => {
                        apply_settings(&st);
                        // Only leave if the write actually succeeded --
                        // apply_settings clears `dirty` by reseeding the
                        // model, so a still-dirty model means it failed and
                        // the user's edits are only in memory.
                        //
                        // **CORRECTED 2026-08-14: back to one way, not two.**
                        // This read "there are now two ways for it not to have
                        // written" -- a failed write, and a Save refused
                        // because the file had moved while the user was behind
                        // a door that hid the announcement (`save_press`,
                        // `aa9fbd6`). That refusal is gone and stayed gone:
                        // the window guarantees the warning is on screen from
                        // every door -- the banner on `BANNER_PAGE`, the
                        // Shortcuts pill's warn dot on the other three -- so
                        // there is no door to be behind and `apply_settings`
                        // writes or fails. The test itself never depended on
                        // which, which is why it is unchanged either time.
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
                    SaveChoice::Discard => {
                        forget_settings(&st);
                        true
                    }
                    SaveChoice::Cancel => false,
                }
            }
        }),
        // An exhaustive `match`, not a `_ => {}`: every variant added later
        // is a compile error at this one site, which is the site that has to
        // handle it. Phase 0 made the channel exist with every arm empty;
        // the tab strip fills the first of them, and the three workstreams
        // that follow fill the rest.
        on_command: Box::new({
            let st = Rc::clone(state);
            let mg = Rc::clone(mgr);
            move |c| match c {
                // Remembered, not acted on: the window has already switched
                // itself by the time this arrives, and what the caller owns
                // is where the NEXT open lands. Cheap enough to store
                // unconditionally -- a page switch is a mouse click, not a
                // keystroke.
                SettingsCommand::ShowPage(p) => st.borrow_mut().settings_page = p,
                // **The tray's own two, and that is the point of routing
                // them through here rather than letting the window act.**
                // `set_paused` does five ordered things -- unregister, set
                // the flag, rewrite the status phrase, CLEAR `registered`,
                // sync the Caps hook -- and the cleared map is what makes the
                // `paused` status word load-bearing on every Shortcuts row.
                // A parallel implementation in the window would flip a flag
                // and leave nineteen rows claiming to be registered.
                //
                // Both end in `refresh_settings`, which is what puts the
                // consequence back on screen: the switch's own state, the
                // status words on the Shortcuts list, and the pill's count.
                SettingsCommand::SetPaused(on) => {
                    set_paused(&st, &mg, on);
                    refresh_settings(&st);
                }
                SettingsCommand::ReloadNow => {
                    reload(&st, &mg);
                    // The file may have changed under the window as well as
                    // under `serve`, so the model is re-read rather than
                    // merely re-projected. This is the same call the banner's
                    // `Reload` makes, and here it is safe without a prompt
                    // for a different reason: the user pressed a button
                    // captioned `Reload` on a page about the service, not one
                    // that appeared to warn them.
                    reload_settings_from_disk(&st);
                }
                // The Run value, written by the same code the tray menu's
                // own row writes -- `set_autostart` is that shared function,
                // and both call sites report a failure the same way.
                SettingsCommand::SetAutostart(on) => {
                    set_autostart(&st, on);
                    refresh_settings(&st);
                }
                // **Applied by the window, recorded by nobody here yet.**
                // Both are the settings window's own look, stored in
                // `HKCU\Software\beckon` by the window itself -- see
                // `beckon_windows::prefs`. The commands exist so the caller
                // CAN react (a tray that follows the same theme is the
                // obvious next reader) and so the channel is exercised by
                // the control that raises it; there is nothing for `serve`
                // to do about either today, and inventing a second store for
                // them here would be a second place they could disagree.
                SettingsCommand::SetDarkMode(_) | SettingsCommand::SetOpacity(_) => {}
                SettingsCommand::Open(t) => open_target(&st, t),
                SettingsCommand::Reveal(t) => reveal_target(&st, t),
                // **`Copy` is a notification, not a request**, and the empty
                // arm is the design rather than a gap. The window has already
                // put the text on the clipboard by the time this arrives --
                // it has to, because `SettingsCommand` is `Copy + Eq` and
                // carries no `String` (see its own doc), so answering here
                // would mean rebuilding `AboutState` in this file and having
                // two authors for one page. What the command buys is a caller
                // that CAN react, and the exercise of the channel by the
                // control that raises it.
                //
                // Keyboard's shorthand toggle (§3.2) and auto-save's undo
                // (§6) are the other two, and those genuinely have no control
                // yet. All three are left as an empty arm rather than folded
                // into a `_`, so the day one needs answering the compiler
                // names this site.
                SettingsCommand::SetCapsShorthand(_)
                | SettingsCommand::Copy(_)
                | SettingsCommand::Undo => {}
            }
        }),
    };

    // The paths are what name the window (`beckon - shortcuts.toml`) and what
    // the System page's two file rows show. Handed over once, at open:
    // `ServeState::config` is what nothing can repoint while the window is
    // up, and `log` is `None` exactly when `serve` was started without
    // `--log`.
    //
    // The borrow is scoped and dropped before `swin::open`, which re-enters
    // `ServeState` through the callbacks above. A live `borrow()` across that
    // is the failure `settings_window::layout`'s `LayoutHandles` documents:
    // the second borrow panics inside an `extern "system"` wndproc, where a
    // panic aborts the process instead of unwinding, so it surfaces as
    // neither a panic nor a test failure.
    let (paths, page) = {
        let s = state.borrow();
        (
            beckon_core::settings::Paths {
                config: s.config.clone(),
                log: s.log.clone(),
            },
            // Where the user left off, which is `Shortcuts` until they move.
            // Read in the same borrow as the paths, and both are read BEFORE
            // `swin::open` for the reason the comment above gives.
            s.settings_page,
        )
    };
    if let Err(e) = swin::open(cb, &paths, page) {
        eprintln!("beckon serve: cannot open settings: {e}");
        swin::error(&format!("Cannot open settings:\n\n{e}"));
        forget_settings(state);
        return;
    }
    spawn_catalog_scan();
    refresh_settings(state);
}

/// Unregister or re-register every hotkey, and say so in the tooltip.
///
/// Neither `unregister_all` nor `register_all` pumps the message queue, and
/// neither does the `set_tray_status` call below -- see `reload`'s doc
/// comment for why holding `state`/`mgr` borrows across a tooltip update is
/// sound while holding them across `beckon_windows::shell::open_path`
/// (`ShellExecuteW`) is not.
///
/// Shared with macOS since the menu bar item arrived: the Caps hook it
/// syncs is a no-op off Windows, and the status call is the cross-platform
/// one. `Pause` means the same thing on both -- the hotkeys go away and the
/// head row says so.
#[cfg(any(target_os = "windows", target_os = "macos"))]
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
        set_tray_status(&format!("beckon - paused ({phrase})"));
    } else {
        state.borrow_mut().paused = false;
        let outcome = register_all(&mut m, &state.borrow().shortcuts);
        let phrase = registration_phrase(outcome.ok, state.borrow().shortcuts.len());
        state.borrow_mut().last_phrase = phrase.clone();
        state.borrow_mut().registered = outcome.by_canonical();
        sync_caps_hook(state);
        eprintln!("beckon serve: resumed - {phrase}");
        set_tray_status(&format!("beckon - {phrase}"));
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

    // -----------------------------------------------------------------
    // Starting on a config that does not parse
    //
    // Measured on a14 2026-08-11: `beckon-serve <broken.toml>` ended in a
    // modal dialog with NO tray icon, so the read-only settings window --
    // which exists precisely for a file that does not parse -- was
    // unreachable from the one starting condition that most needs it. These
    // tests pin the decision half of the fix, and they run on all three CI
    // jobs; the tray and the window compile on one.
    // -----------------------------------------------------------------

    const GOOD: &str = "\"ctrl+alt+t\" = \"Terminal\"\nkeyboard.caps = true\n";
    /// Broken the way a real config breaks -- a value the user had not
    /// finished typing -- and carrying `keyboard.caps = true` so the test
    /// below can prove nothing in it is honoured.
    const BROKEN: &str = "\"ctrl+alt+t\" = \"Terminal\"\nkeyboard.caps = true\n\"ctrl+alt+e\" = \n";

    #[test]
    fn a_file_that_parses_is_served_whatever_the_policy_says() {
        for policy in [BrokenConfig::Refuse, BrokenConfig::ServeAnyway] {
            let plan = plan_startup(GOOD, policy).expect("this file parses");
            assert_eq!(plan.shortcuts.len(), 1);
            assert!(plan.keyboard.caps, "the keyboard block is carried through");
            assert_eq!(plan.broken, None, "nothing is wrong with this file");
        }
    }

    #[test]
    fn the_console_front_door_still_refuses_a_broken_file() {
        // `beckon.exe serve` must keep exiting non-zero for scripts. The
        // message is the parser's own, unprefixed, so `cmd_serve_app` can put
        // the path in front of it and print exactly what it always printed.
        let err = plan_startup(BROKEN, BrokenConfig::Refuse)
            .err()
            .expect("a broken file must be refused on this front door");
        assert_eq!(
            err,
            parse_config(BROKEN).unwrap_err(),
            "the refusal must carry the parser's own words, unprefixed"
        );
    }

    #[test]
    fn the_gui_front_door_starts_with_nothing_registered() {
        let plan =
            plan_startup(BROKEN, BrokenConfig::ServeAnyway).expect("this front door must start");
        assert!(
            plan.shortcuts.is_empty(),
            "a file beckon cannot read binds no keys"
        );
        assert!(plan.broken.is_some(), "the reason travels with the plan");
        assert_eq!(
            plan.broken.as_deref(),
            Some(parse_config(BROKEN).unwrap_err()).as_deref()
        );
    }

    /// The safety property, not a formatting one: `sync_caps_hook` reads
    /// `keyboard.caps` to decide whether to install a `WH_KEYBOARD_LL` hook.
    /// A half-parsed file must never arm one.
    #[test]
    fn a_broken_start_cannot_arm_the_caps_hook() {
        assert!(
            BROKEN.contains("keyboard.caps = true"),
            "precondition: the broken file asks for the hook"
        );
        let plan = plan_startup(BROKEN, BrokenConfig::ServeAnyway).unwrap();
        assert_eq!(
            plan.keyboard,
            KeyboardConfig::default(),
            "nothing in a file that does not parse is honoured"
        );
        assert!(!plan.keyboard.caps);
    }

    #[test]
    fn the_unreadable_phrase_is_honest_and_ascii() {
        let p = unreadable_phrase();
        assert_eq!(p, "cannot read the config (0 shortcuts registered)");
        // What the tray tooltip and the menu head become. Neither may claim
        // a registration that did not happen.
        assert_eq!(
            format!("beckon - {p}"),
            "beckon - cannot read the config (0 shortcuts registered)"
        );
        assert!(
            p.is_ascii(),
            "serve status lines are read through ANSI tools"
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

    #[test]
    fn menu_shows_the_phrase_and_reflects_pause() {
        let m = MenuModel {
            phrase: "5 shortcuts registered".into(),
            paused: false,
            autostart: Some(false),
            log: Some(true),
            settings: true,
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

    /// The macOS menu says nothing it cannot do, and each absence is
    /// structural: `--log` is `#[cfg(target_os = "windows")]` so beckon
    /// never owns a log path there, and login lifecycle belongs to
    /// `brew services`. (`Settings...` WAS absent while the window did not
    /// exist; it is built now, which is why this model has `settings:
    /// true` and the flag survives as a capability rather than a constant.)
    ///
    /// Asserted as a whole-shape equality rather than three separate
    /// "row is absent" checks, because the failure this guards against is a
    /// row being ADDED — which no absence check can see.
    #[test]
    fn the_macos_menu_says_nothing_it_cannot_do() {
        let rows = build_entries(&MenuModel {
            phrase: "18 shortcuts registered".into(),
            paused: false,
            autostart: None,
            log: None,
            settings: true,
        });
        let shape: Vec<&str> = rows.iter().map(|r| r.label.as_str()).collect();
        assert_eq!(
            shape,
            vec![
                "beckon - 18 shortcuts registered",
                "",
                "Settings...",
                "Reload now",
                "",
                "Pause hotkeys",
                "",
                "Quit",
            ]
        );
        assert!(!rows[0].enabled, "the status row is a label, not a button");
    }

    /// The status row is the one place the phrase is actually readable —
    /// the tooltip is the redundant copy — so it must survive the macOS
    /// shape, including while paused.
    #[test]
    fn the_macos_menu_still_reports_pause_in_its_head_row() {
        let rows = build_entries(&MenuModel {
            phrase: "18 shortcuts registered".into(),
            paused: true,
            autostart: None,
            log: None,
            settings: true,
        });
        assert_eq!(rows[0].label, "beckon - paused (18 shortcuts registered)");
        assert_eq!(
            rows.iter().find(|r| r.id == MENU_PAUSE).unwrap().checked,
            Some(true)
        );
    }

    /// A menu must never end on a separator or show two in a row: AppKit
    /// draws both, so the bug is visible rather than inert. Checked for
    /// every combination of the three capability flags, because the
    /// omissions interact — dropping `Settings...` while keeping
    /// `Open log` is a different row list from dropping both.
    #[test]
    fn no_capability_combination_produces_a_stray_separator() {
        for settings in [true, false] {
            for log in [None, Some(true), Some(false)] {
                for autostart in [None, Some(true), Some(false)] {
                    let rows = build_entries(&MenuModel {
                        phrase: "p".into(),
                        paused: false,
                        autostart,
                        log,
                        settings,
                    });
                    let case = format!("settings={settings} log={log:?} autostart={autostart:?}");
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
    }

    /// Fix for the CRITICAL bug: the CLI path (`beckon.exe serve`) used to
    /// show "Start with Windows" unconditionally, and ticking it there
    /// wrote a Run value that could never start anything (see
    /// `AutostartCapability`). The row must not exist at all when the
    /// capability is absent -- disabled-and-unexplained is not an
    /// acceptable substitute for omitted.
    #[test]
    fn autostart_row_exists_only_when_the_capability_does() {
        let base = MenuModel {
            phrase: "5 shortcuts registered".into(),
            paused: false,
            autostart: None,
            log: Some(true),
            settings: true,
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
    #[test]
    fn no_real_entry_collides_with_the_reserved_double_click_id() {
        let m = MenuModel {
            phrase: "5 shortcuts registered".into(),
            paused: false,
            autostart: Some(false),
            log: Some(true),
            settings: true,
        };
        for row in build_entries(&m) {
            assert_ne!(
                row.id,
                beckon_core::menu::MENU_ID_DOUBLE_CLICK,
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
    #[test]
    fn the_first_action_row_opens_settings_not_notepad() {
        let m = MenuModel {
            phrase: "2 shortcuts registered".into(),
            paused: false,
            autostart: Some(false),
            log: Some(true),
            settings: true,
        };
        let rows = build_entries(&m);
        let edit = rows.iter().find(|r| r.id == MENU_EDIT).unwrap();
        assert_eq!(edit.label, "Settings...");
    }

    #[test]
    fn open_log_is_disabled_when_there_is_no_log() {
        let m = MenuModel {
            phrase: "0 shortcuts registered".into(),
            paused: false,
            autostart: Some(false),
            log: Some(false),
            settings: true,
        };
        let rows = build_entries(&m);
        assert!(!rows.iter().find(|r| r.id == MENU_LOG).unwrap().enabled);
    }
}
