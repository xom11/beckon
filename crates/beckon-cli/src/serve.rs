//! `--serve`: resident hotkey service (macOS). Single-threaded by design:
//! hotkey dispatch and reload ticks all run on the main run loop, so plain
//! Rc<RefCell<…>> state needs no locking.

use anyhow::{anyhow, Context, Result};
use beckon_core::shortcuts::{parse_shortcuts, Shortcut};
use beckon_core::Backend;
use beckon_macos::hotkey::HotkeyManager;
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

struct ServeState {
    shortcuts: Vec<Shortcut>,
    backend: Box<dyn Backend>,
    // Not read yet — reserved for the watch-and-reload path (Task 8), which
    // needs to know which file to re-read on change.
    #[allow(dead_code)]
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
    let backend = crate::pick_backend()?;

    let state = Rc::new(RefCell::new(ServeState {
        shortcuts,
        backend,
        config: config.clone(),
    }));

    let mgr = {
        let st = Rc::clone(&state);
        // The hotkey callback runs on the main CFRunLoop that also delivers
        // this event, so it must never pump the run loop itself (e.g. no
        // nested RunApplicationEventLoop / modal calls). `backend.beckon()`
        // is a synchronous, non-run-loop-pumping call, so it is safe here.
        HotkeyManager::install(Box::new(move |id| on_hotkey(&st, id))).map_err(|e| anyhow!(e))?
    };
    let mgr = Rc::new(RefCell::new(mgr));
    register_all(&mut mgr.borrow_mut(), &state.borrow().shortcuts);

    eprintln!(
        "beckon serve: {} shortcuts registered from {}",
        state.borrow().shortcuts.len(),
        config.display()
    );
    HotkeyManager::run_forever();
}

fn on_hotkey(state: &Rc<RefCell<ServeState>>, id: u32) {
    let st = state.borrow();
    let Some(sc) = st.shortcuts.get(id as usize) else {
        return;
    };
    if let Err(e) = st.backend.beckon(&sc.app) {
        let msg = format!("{} ({}): {}", sc.app, sc.combo.canonical(), e);
        eprintln!("beckon serve: {msg}");
        crate::notify_error(&msg);
    }
}

fn register_all(mgr: &mut HotkeyManager, shortcuts: &[Shortcut]) {
    for (i, sc) in shortcuts.iter().enumerate() {
        let c = &sc.combo;
        if let Err(e) = mgr.register(i as u32, c.ctrl, c.super_, c.alt, c.shift, c.key.mac) {
            // One broken key loses one key, never the whole table.
            let msg = format!("cannot register `{}`: {e}", c.canonical());
            eprintln!("beckon serve: {msg}");
            crate::notify_error(&msg);
        }
    }
}
