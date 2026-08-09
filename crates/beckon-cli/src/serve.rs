//! `--serve`: resident hotkey service (macOS, Windows). Single-threaded by
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
use beckon_core::shortcuts::{parse_shortcuts, Shortcut};
use beckon_core::Backend;
#[cfg(target_os = "macos")]
use beckon_macos::hotkey;
#[cfg(target_os = "windows")]
use beckon_windows::hotkey;
use hotkey::HotkeyManager;
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

struct ServeState {
    shortcuts: Vec<Shortcut>,
    config: PathBuf,
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

pub fn cmd_serve(config: &Path) -> Result<()> {
    let _lock = acquire_lock(config)?;
    let config = config
        .canonicalize()
        .with_context(|| format!("cannot resolve `{}`", config.display()))?;
    let text = std::fs::read_to_string(&config)
        .with_context(|| format!("cannot read `{}`", config.display()))?;
    let shortcuts = parse_shortcuts(&text).map_err(|e| anyhow!("{}: {}", config.display(), e))?;
    // Outside the RefCell on purpose — see module doc.
    let backend: Rc<Box<dyn Backend>> = Rc::new(crate::pick_backend()?);

    let state = Rc::new(RefCell::new(ServeState {
        shortcuts,
        config: config.clone(),
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

    eprintln!(
        "beckon serve: {} from {}",
        registration_phrase(outcome.ok, state.borrow().shortcuts.len()),
        config.display()
    );
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
}

/// "20 shortcuts registered" when clean; "17 of 20 shortcuts registered
/// (3 failed)" otherwise.
fn registration_phrase(ok: usize, total: usize) -> String {
    let failed = total - ok;
    if failed == 0 {
        format!("{total} shortcuts registered")
    } else {
        format!("{ok} of {total} shortcuts registered ({failed} failed)")
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
    for (i, sc) in shortcuts.iter().enumerate() {
        let c = &sc.combo;
        match mgr.register(i as u32, c.ctrl, c.super_, c.alt, c.shift, c.key) {
            Ok(()) => ok += 1,
            Err(e) => {
                // One broken key loses one key, never the whole table.
                // Per-key eprintln kept for the detailed log; the toast is
                // collapsed into a single summary by the caller instead of
                // firing here (see `failure_toast`).
                eprintln!("beckon serve: cannot register `{}`: {e}", c.canonical());
                failed.push(c.canonical());
            }
        }
    }
    RegisterOutcome { ok, failed }
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

fn reload(state: &Rc<RefCell<ServeState>>, mgr: &Rc<RefCell<HotkeyManager>>) {
    let config = state.borrow().config.clone();
    let parsed = std::fs::read_to_string(&config)
        .map_err(|e| format!("cannot read `{}`: {e}", config.display()))
        .and_then(|t| parse_shortcuts(&t).map_err(|e| format!("{}: {e}", config.display())));
    match parsed {
        Err(e) => {
            // Bad edit must not cost the user their working keys.
            let msg = format!("reload failed, keeping current shortcuts: {e}");
            eprintln!("beckon serve: {msg}");
            // The watcher drives this, not a person: anything that keeps
            // rewriting the config (an editor sync, a `home-manager switch`
            // loop) would otherwise post once per tick, forever.
            crate::notify::report(&msg, crate::notify::Cause::MachineRepeat);
        }
        Ok(new) => {
            let mut m = mgr.borrow_mut();
            m.unregister_all();
            state.borrow_mut().shortcuts = new;
            let outcome = register_all(&mut m, &state.borrow().shortcuts);
            eprintln!(
                "beckon serve: reloaded — {}",
                registration_phrase(outcome.ok, state.borrow().shortcuts.len())
            );
            if let Some(toast) = failure_toast(&outcome.failed) {
                crate::notify::report(&toast, crate::notify::Cause::MachineRepeat);
            }
        }
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
}
