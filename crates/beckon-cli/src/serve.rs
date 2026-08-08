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

pub fn cmd_serve(config: &Path) -> Result<()> {
    let _lock = crate::lockfile::acquire(config).map_err(|e| anyhow!(e))?;
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
    register_all(&mut mgr.borrow_mut(), &state.borrow().shortcuts);

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
        "beckon serve: {} shortcuts registered from {}",
        state.borrow().shortcuts.len(),
        config.display()
    );
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
        crate::notify_error(&msg);
    }
}

fn register_all(mgr: &mut HotkeyManager, shortcuts: &[Shortcut]) {
    for (i, sc) in shortcuts.iter().enumerate() {
        let c = &sc.combo;
        if let Err(e) = mgr.register(i as u32, c.ctrl, c.super_, c.alt, c.shift, c.key) {
            // One broken key loses one key, never the whole table.
            let msg = format!("cannot register `{}`: {e}", c.canonical());
            eprintln!("beckon serve: {msg}");
            crate::notify_error(&msg);
        }
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
            crate::notify_error(&msg);
        }
        Ok(new) => {
            let mut m = mgr.borrow_mut();
            m.unregister_all();
            state.borrow_mut().shortcuts = new;
            register_all(&mut m, &state.borrow().shortcuts);
            eprintln!(
                "beckon serve: reloaded {} shortcuts",
                state.borrow().shortcuts.len()
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsStr;

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
