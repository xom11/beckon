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

    let (tx, rx) = std::sync::mpsc::channel::<()>();
    let _watcher = watch_config(&config, tx)?; // lives as long as the loop below
    {
        let st = Rc::clone(&state);
        let mg = Rc::clone(&mgr);
        beckon_macos::hotkey::add_tick(
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
