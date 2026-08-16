//! KDE Wayland backend, driven through KWin's own scripting engine.
//!
//! KWin, like Mutter, refuses to let an outside process focus a window. The
//! GNOME answer is a shell extension we ship and the user installs; KDE
//! needs no install at all, because KWin exposes a scripting engine on the
//! session bus:
//!
//!   org.kde.KWin  /Scripting  org.kde.kwin.Scripting
//!     loadScript(path, pluginName) -> i
//!     start()
//!     unloadScript(pluginName) -> b
//!     isScriptLoaded(pluginName) -> b
//!
//! A loaded script runs inside KWin with a `workspace` object, so it can read
//! `workspace.stackingOrder` and write `workspace.activeWindow` — everything
//! the shared algorithm needs, on the inside of the boundary that blocks us
//! from the outside.
//!
//! **Why scripting and not a Wayland protocol.** KWin advertises neither
//! `zwlr_foreign_toplevel_management_v1` (wlroots-only) nor its own
//! `org_kde_plasma_window_management` — the latter is simply absent from the
//! registry on a plain `kwin_wayland`, so a protocol client cannot enumerate
//! windows even though the protocol exists on paper. Scripting is the only
//! surface that is actually there.
//!
//! **How results get back out.** KWin scripts have no file I/O; `callDBus` is
//! the single escape hatch. So beckon serves a one-method interface on its
//! own connection, bakes its unique bus name into the generated script, and
//! the script calls back with the window list as JSON. beckon then decides,
//! and loads a second script that performs the action. Two script round
//! trips per invocation; a load+start+unload cycle measured at well under
//! 10 ms, so the hot path stays inside budget.
//!
//! **Window identity** is `Window.internalId`, a QUuid rendered as
//! `{xxxxxxxx-…}`. It is stable for the window's lifetime, which is all the
//! algorithm needs — but unlike every other backend's address it is not
//! numeric, so `algorithm::cmp_address` falls back to byte order. The cycle
//! ring is therefore stable but not in window-creation order on KDE.
//!
//! **Recency** comes from `workspace.stackingOrder` reversed (topmost first),
//! the same standing-in-for-MRU choice the X11 backend makes with
//! `_NET_CLIENT_LIST_STACKING`.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc::{channel, Sender};
use std::time::Duration;

use beckon_core::{Backend, BackendError, BeckonAction, InstalledApp, Result, RunningApp};
use serde::Deserialize;
use zbus::blocking::{connection, Connection, Proxy};

use crate::algorithm::{decide, Decision, WindowSnapshot};

const DEST: &str = "org.kde.KWin";
const PATH: &str = "/Scripting";
const IFACE: &str = "org.kde.kwin.Scripting";

/// Where beckon serves the reply sink the generated script calls back into.
const SINK_PATH: &str = "/com/github/xom11/beckon";
const SINK_IFACE: &str = "com.github.xom11.beckon.KWin";

/// KWin plugin names for the two scripts we load. Distinct so a read and a
/// write can never collide, and fixed so a crashed run leaves at most these
/// two behind — both are unloaded unconditionally before each load.
///
/// The FILES are a different matter: `script_path` appends the pid, so every
/// invocation writes its own pair and the act half is never safe to unlink
/// inline. `sweep_orphan_scripts_in` is what collects them.
const READ_PLUGIN: &str = "beckon-read";
const ACT_PLUGIN: &str = "beckon-act";

/// How long to wait for the script's `callDBus` reply. Generous: it only
/// matters when KWin is wedged, and the alternative is a hotkey that hangs.
const REPLY_TIMEOUT: Duration = Duration::from_secs(3);

/// One window as the generated script reports it.
#[derive(Debug, Clone, Deserialize)]
struct WindowRow {
    id: String,
    #[serde(default)]
    cls: String,
    /// KWin's `resourceName` — the `WM_CLASS` INSTANCE half for an X11 or
    /// XWayland client, where `cls` above is `resourceClass`. Read on its own
    /// rather than only as the fallback `cls` already uses it for, because
    /// for a browser-installed web app the two halves differ and only this
    /// one identifies the app; see `algorithm::WindowSnapshot::instance`.
    ///
    /// `#[serde(default)]` matters: a beckon that talks to an OLD generated
    /// script -- there is no such thing today, both halves ship in one
    /// binary, but a partially-updated checkout is a real state -- gets an
    /// empty string rather than a parse failure that would take the whole
    /// window list down with it.
    #[serde(default)]
    inst: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    active: bool,
}

/// The object the KWin script calls back into. `callDBus` is the only way
/// data can leave a script, so this is the whole return channel.
struct ReplySink {
    tx: Sender<String>,
}

#[zbus::interface(name = "com.github.xom11.beckon.KWin")]
impl ReplySink {
    fn windows(&self, json: String) {
        // Ignore send errors: the receiver has already timed out and moved
        // on, which is not this side's problem.
        let _ = self.tx.send(json);
    }
}

pub struct KdeBackend {
    conn: Connection,
    /// Our unique bus name (`:1.42`), baked into every generated script so
    /// the callback lands here and not on some other beckon process.
    sink_name: String,
    rx: std::sync::mpsc::Receiver<String>,
}

impl KdeBackend {
    pub fn new() -> Result<Self> {
        let (tx, rx) = channel();
        let conn = connection::Builder::session()
            .map_err(|e| BackendError::Ipc(format!("session bus connect: {e}")))?
            .serve_at(SINK_PATH, ReplySink { tx })
            .map_err(|e| BackendError::Ipc(format!("serve reply sink: {e}")))?
            .build()
            .map_err(|e| BackendError::Ipc(format!("build connection: {e}")))?;

        let sink_name = conn
            .inner()
            .unique_name()
            .map(|n| n.to_string())
            .ok_or_else(|| BackendError::Ipc("session bus gave us no unique name".to_string()))?;

        // Probe KWin's scripting engine. `isScriptLoaded` is read-only and
        // costs nothing, while still proving both that org.kde.KWin owns the
        // name and that the scripting interface is present at the expected
        // path.
        {
            let proxy = Self::proxy(&conn)?;
            proxy
                .call::<_, _, bool>("isScriptLoaded", &(READ_PLUGIN,))
                .map_err(|e| {
                    BackendError::Ipc(format!(
                        "KWin scripting not reachable on D-Bus: {e}. beckon drives KDE Wayland \
                         through `org.kde.kwin.Scripting`, which is part of KWin itself — if \
                         this fails you are probably not on a KWin session."
                    ))
                })?;
        }

        // Now that we know this is a KWin session and are about to write a
        // script of our own, collect the ones earlier invocations left
        // behind. See `sweep_orphan_scripts_in` for why they are left.
        sweep_orphan_scripts();

        Ok(Self {
            conn,
            sink_name,
            rx,
        })
    }

    fn proxy(conn: &Connection) -> Result<Proxy<'_>> {
        Proxy::new(conn, DEST, PATH, IFACE)
            .map_err(|e| BackendError::Ipc(format!("D-Bus proxy: {e}")))
    }

    /// Write `source` to a private file, load it under `plugin`, run it, then
    /// unload. KWin reads the script off disk, so it has to be a real file;
    /// `$XDG_RUNTIME_DIR` keeps it off any shared or persistent path.
    fn run_script(&self, plugin: &str, source: &str) -> Result<()> {
        let path = script_path(plugin);
        std::fs::write(&path, source)
            .map_err(|e| BackendError::Ipc(format!("write KWin script {}: {e}", path.display())))?;

        let proxy = Self::proxy(&self.conn)?;

        // Unload first: KWin refuses to load a plugin name that is already
        // registered, and a previous run that died mid-flight would have
        // left ours behind.
        let _ = proxy.call::<_, _, bool>("unloadScript", &(plugin,));

        let load = proxy
            .call::<_, _, i32>("loadScript", &(path.to_string_lossy().as_ref(), plugin))
            .map_err(|e| BackendError::Ipc(format!("loadScript: {e}")));

        // Whatever happens next, don't leave the file lying around.
        let cleanup = |proxy: &Proxy<'_>| {
            let _ = proxy.call::<_, _, bool>("unloadScript", &(plugin,));
            let _ = std::fs::remove_file(&path);
        };

        if let Err(e) = load {
            cleanup(&proxy);
            return Err(e);
        }

        let started = proxy
            .call::<_, _, ()>("start", &())
            .map_err(|e| BackendError::Ipc(format!("start: {e}")));
        if let Err(e) = started {
            cleanup(&proxy);
            return Err(e);
        }
        Ok(())
    }

    /// Ask KWin for its window list and wait for the script to call back.
    fn list_windows(&self) -> Result<Vec<WindowRow>> {
        // Drain anything a previous (timed-out) call left behind, so we can
        // never read a stale window list as if it were fresh.
        while self.rx.try_recv().is_ok() {}

        self.run_script(READ_PLUGIN, &read_script(&self.sink_name))?;

        let json: Result<String> = self.rx.recv_timeout(REPLY_TIMEOUT).map_err(|_| {
            BackendError::Ipc(format!(
                "KWin script did not call back within {:?}. The script engine accepted the \
                 script but nothing reached {SINK_IFACE} — check KWin's stderr, where a \
                 script's own errors are printed.",
                REPLY_TIMEOUT
            ))
        });

        // Unload the read script now that it has had its say; `run_script`
        // only clears it on the error paths so that a slow script still has
        // its chance to fire. This runs whether or not the reply arrived, and
        // the `?` deliberately comes after it: it used to sit on the line
        // above, so a timed-out read returned early and left
        // `beckon-read-<pid>.js` in `$XDG_RUNTIME_DIR` for the rest of the
        // session — and the wait itself is what proves the script has had its
        // chance, so there is nothing left to race.
        if let Ok(proxy) = Self::proxy(&self.conn) {
            let _ = proxy.call::<_, _, bool>("unloadScript", &(READ_PLUGIN,));
        }
        let _ = std::fs::remove_file(script_path(READ_PLUGIN));

        serde_json::from_str(&json?)
            .map_err(|e| BackendError::Ipc(format!("bad window JSON from KWin script: {e}")))
    }

    fn activate(&self, id: &str) -> Result<()> {
        self.run_script(ACT_PLUGIN, &act_script(id, Act::Activate))
    }

    fn minimize(&self, id: &str) -> Result<()> {
        self.run_script(ACT_PLUGIN, &act_script(id, Act::Minimize))
    }
}

#[derive(Clone, Copy)]
enum Act {
    Activate,
    Minimize,
}

/// Where the generated scripts live: `$XDG_RUNTIME_DIR` when it is usable
/// (absolute, per the basedir spec), the system temp dir otherwise.
fn script_dir() -> PathBuf {
    std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(std::env::temp_dir)
}

fn script_path(plugin: &str) -> PathBuf {
    script_dir().join(format!("{plugin}-{}.js", std::process::id()))
}

/// The pid encoded in one of *our* script file names, or `None` for anything
/// else in the directory.
///
/// Deliberately strict, because the sweep below deletes whatever this returns
/// a pid for and `$XDG_RUNTIME_DIR` is everybody's: the name has to be one of
/// the two plugin names, a `-`, digits that round-trip, and `.js`. `parse`
/// alone would accept `beckon-act-007.js` and `beckon-act-+7.js`, neither of
/// which beckon ever wrote.
fn script_file_pid(file_name: &str) -> Option<u32> {
    let stem = file_name.strip_suffix(".js")?;
    let digits = [READ_PLUGIN, ACT_PLUGIN]
        .into_iter()
        .find_map(|plugin| stem.strip_prefix(plugin))?
        .strip_prefix('-')?;
    let pid: u32 = digits.parse().ok()?;
    (digits == pid.to_string()).then_some(pid)
}

/// Does this pid name a live process? `/proc/<pid>` is the answer on the only
/// platform this backend runs on, and costs no dependency.
fn pid_is_alive(pid: u32) -> bool {
    Path::new("/proc").join(pid.to_string()).exists()
}

/// Delete `beckon-{read,act}-<pid>.js` files whose process is gone.
///
/// The act script's file cannot be unlinked inline the way `list_windows`
/// unlinks the read script's: `act_script` contains no `callDBus`, so there
/// is no completion signal to wait on, and removing the file straight after
/// `start()` can beat KWin to reading it. So it outlives the invocation that
/// wrote it — one file per `beckon <id>` press — and somebody has to collect
/// them. The next beckon to start does.
///
/// Two ways this can be wrong, and doing less answers both. Pids are reused,
/// so a live pid is not proof the file is still in use — but the mistake it
/// causes is keeping a file that was already dead, which costs one inode
/// until the next matching pid dies. And the directory is shared, so only
/// names `script_file_pid` recognises are considered at all. It is
/// best-effort throughout: a sweep that fails must never fail the keypress it
/// is riding on.
fn sweep_orphan_scripts_in(dir: &Path, is_alive: impl Fn(u32) -> bool) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let self_pid = std::process::id();
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(pid) = name.to_str().and_then(script_file_pid) else {
            continue;
        };
        // Our own file is never an orphan, whatever the liveness oracle says.
        if pid == self_pid || is_alive(pid) {
            continue;
        }
        let _ = std::fs::remove_file(entry.path());
    }
}

fn sweep_orphan_scripts() {
    sweep_orphan_scripts_in(&script_dir(), pid_is_alive);
}

/// JS string literal escaping for the few values we interpolate. Window ids
/// are KWin-minted UUIDs and the bus name is a unique name, so neither can
/// realistically contain a quote — but neither is a compile-time constant,
/// and building source code by concatenation without escaping is how that
/// stops being true.
fn js_quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Script that reports every normal window, most-recently-raised first.
fn read_script(sink_name: &str) -> String {
    format!(
        r#"(function () {{
    // stackingOrder is bottom-to-top; reverse so index 0 is the topmost
    // window, which is the closest thing KWin gives us to "most recent".
    // windowList() is the fallback if a KWin build lacks the property.
    var wins = (typeof workspace.stackingOrder !== "undefined" && workspace.stackingOrder)
        ? workspace.stackingOrder.slice().reverse()
        : workspace.windowList();
    var active = workspace.activeWindow;
    var out = [];
    for (var i = 0; i < wins.length; i++) {{
        var w = wins[i];
        // Panels, docks, OSDs and the desktop itself are not apps: KWin
        // refuses to activate them, so letting one through would make
        // step 5b "toggle back" to something that never gets focus.
        if (!w.normalWindow) continue;
        if (w.skipTaskbar) continue;
        var cls = w.resourceClass || w.resourceName || "";
        if (!cls) continue;
        out.push({{
            id: String(w.internalId),
            cls: String(cls),
            inst: String(w.resourceName || ""),
            title: String(w.caption || ""),
            active: (active !== null && active !== undefined && w === active)
        }});
    }}
    callDBus({sink}, {path}, {iface}, "Windows", JSON.stringify(out));
}})();
"#,
        sink = js_quote(sink_name),
        path = js_quote(SINK_PATH),
        iface = js_quote(SINK_IFACE),
    )
}

/// Script that acts on one window, found by `internalId`.
fn act_script(id: &str, act: Act) -> String {
    let body = match act {
        // Unminimize before activating. Assigning activeWindow to a
        // minimized window is not defined to restore it, and the X11 backend
        // already taught us not to assume a focus request de-iconifies.
        Act::Activate => "w.minimized = false; workspace.activeWindow = w;",
        Act::Minimize => "w.minimized = true;",
    };
    format!(
        r#"(function () {{
    var target = {id};
    var wins = workspace.windowList();
    for (var i = 0; i < wins.length; i++) {{
        var w = wins[i];
        if (String(w.internalId) === target) {{
            {body}
            return;
        }}
    }}
}})();
"#,
        id = js_quote(id),
        body = body,
    )
}

/// Fully-detached child for the `.desktop` `Exec` line — same recipe as the
/// other Linux backends.
fn launch_exec(exec: &str) -> Result<()> {
    Command::new("/bin/sh")
        .arg("-c")
        .arg(format!("setsid -f {} >/dev/null 2>&1", exec))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| BackendError::LaunchFailed {
            id: exec.to_string(),
            reason: format!("spawn /bin/sh: {e}"),
        })?;
    Ok(())
}

fn snapshots_from(rows: &[WindowRow]) -> Vec<WindowSnapshot> {
    rows.iter()
        .enumerate()
        .map(|(idx, r)| {
            WindowSnapshot::new(r.id.clone(), &r.cls, idx as i32)
                .with_instance(Some(r.inst.as_str()))
        })
        .collect()
}

fn persist_previous(class: Option<&str>) {
    if let Some(c) = class {
        crate::state::write_previous(c);
    }
}

impl Backend for KdeBackend {
    fn beckon(&self, id: &str) -> Result<BeckonAction> {
        let rows = self.list_windows()?;

        let active_addr = rows.iter().find(|r| r.active).map(|r| r.id.clone());
        let pre_focused_class = rows.iter().find(|r| r.active).map(|r| r.cls.clone());
        let previous_app = crate::state::read_previous();

        let entry = crate::desktop::resolve(id);
        // KWin sets `resourceClass` from the Wayland app_id for native
        // clients and from WM_CLASS for XWayland ones — the same two-source
        // situation sway has, so the same candidate set applies.
        let target = crate::desktop::target_classes(entry.as_ref(), id);

        let snapshots = snapshots_from(&rows);
        let decision = decide(
            &snapshots,
            active_addr.as_deref(),
            target,
            previous_app.as_deref(),
        );

        let action = match decision {
            Decision::Launch => {
                let entry = entry.ok_or_else(|| BackendError::NoMatch {
                    id: id.to_string(),
                    hint: format!(
                        "no .desktop entry matches `{0}` and no running window has class=`{0}`. \
                         Run `beckon installed` to list installed apps, \
                         or `beckon search {0}` to search.",
                        id
                    ),
                })?;
                launch_exec(&entry.exec)?;
                BeckonAction::Launched
            }
            Decision::Focus(addr) => {
                self.activate(&addr)?;
                BeckonAction::Focused
            }
            Decision::Cycle(addr) => {
                self.activate(&addr)?;
                BeckonAction::Cycled
            }
            Decision::ToggleBack(addr) => {
                self.activate(&addr)?;
                BeckonAction::ToggledBack
            }
            Decision::Hide(addr) => {
                self.minimize(&addr)?;
                BeckonAction::Hidden
            }
        };

        persist_previous(pre_focused_class.as_deref());
        Ok(action)
    }

    fn list_running(&self) -> Result<Vec<RunningApp>> {
        let rows = self.list_windows()?;
        let mut by_class: std::collections::BTreeMap<String, (String, usize)> = Default::default();
        for r in rows {
            let entry = by_class
                .entry(r.cls)
                .or_insert_with(|| (r.title.clone(), 0));
            entry.1 += 1;
        }
        Ok(by_class
            .into_iter()
            .map(|(id, (name, window_count))| RunningApp {
                id,
                name,
                window_count,
            })
            .collect())
    }

    fn list_installed(&self) -> Result<Vec<InstalledApp>> {
        let mut entries = crate::desktop::visible(crate::desktop::scan());
        entries.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(entries
            .into_iter()
            .map(|e| InstalledApp {
                id: e.id,
                name: e.name,
                exec: Some(e.exec),
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn js_quote_escapes_the_dangerous_characters() {
        assert_eq!(js_quote("plain"), "\"plain\"");
        assert_eq!(js_quote("a\"b"), "\"a\\\"b\"");
        assert_eq!(js_quote("a\\b"), "\"a\\\\b\"");
        assert_eq!(js_quote("a\nb"), "\"a\\nb\"");
    }

    /// The generated script has to carry *this* process's bus name, or the
    /// reply lands on another beckon.
    #[test]
    fn read_script_embeds_the_sink_name() {
        let s = read_script(":1.42");
        assert!(s.contains("\":1.42\""));
        assert!(s.contains(SINK_IFACE));
        assert!(s.contains("callDBus"));
    }

    #[test]
    fn act_script_distinguishes_activate_from_minimize() {
        let id = "{0a3e0e14-b2de-47a0-a6b5-f1c205c66bc6}";
        let a = act_script(id, Act::Activate);
        assert!(a.contains("workspace.activeWindow = w"));
        assert!(a.contains("w.minimized = false"));
        assert!(a.contains(id));

        let m = act_script(id, Act::Minimize);
        assert!(m.contains("w.minimized = true"));
        assert!(!m.contains("workspace.activeWindow"));
    }

    #[test]
    fn window_rows_parse_from_the_script_payload() {
        let json = r#"[
            {"id":"{aaa}","cls":"foot","title":"foot","active":false},
            {"id":"{bbb}","cls":"konsole","title":"~","active":true}
        ]"#;
        let rows: Vec<WindowRow> = serde_json::from_str(json).expect("parses");
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[1].cls, "konsole");
        assert!(rows[1].active);

        let snaps = snapshots_from(&rows);
        assert_eq!(snaps[0].recency, 0);
        assert_eq!(snaps[1].address, "{bbb}");
    }

    /// Missing optional fields must not fail the whole list — a window with
    /// no caption is normal.
    #[test]
    fn window_rows_tolerate_missing_optional_fields() {
        let rows: Vec<WindowRow> = serde_json::from_str(r#"[{"id":"{a}"}]"#).expect("parses");
        assert_eq!(rows[0].id, "{a}");
        assert!(rows[0].cls.is_empty());
        assert!(!rows[0].active);
    }

    /// The sweep deletes what this matches, and `$XDG_RUNTIME_DIR` belongs to
    /// every program on the machine.
    #[test]
    fn only_the_names_beckon_writes_carry_a_pid() {
        assert_eq!(script_file_pid("beckon-read-1234.js"), Some(1234));
        assert_eq!(script_file_pid("beckon-act-7.js"), Some(7));

        assert_eq!(script_file_pid("beckon-act.js"), None);
        assert_eq!(script_file_pid("beckon-act-.js"), None);
        assert_eq!(script_file_pid("beckon-act-x.js"), None);
        assert_eq!(script_file_pid("beckon-act-7.txt"), None);
        assert_eq!(script_file_pid("kwin-act-7.js"), None);
        assert_eq!(script_file_pid("pulse-7.js"), None);
        // `parse` accepts both of these; beckon writes neither.
        assert_eq!(script_file_pid("beckon-act-007.js"), None);
        assert_eq!(script_file_pid("beckon-act-+7.js"), None);
    }

    #[test]
    fn the_sweep_takes_dead_pids_and_leaves_everything_else() {
        let dir = scratch_dir("sweep");
        let ours = script_path(READ_PLUGIN);
        let ours = dir.join(ours.file_name().unwrap());
        for name in [
            "beckon-act-4242.js",  // dead beckon: the leak this collects
            "beckon-read-4242.js", // same
            "beckon-act-99.js",    // another beckon, still running
            "beckon-act-notes.js", // not ours: no pid
            "kwin-act-4242.js",    // not ours: someone else's prefix
        ] {
            std::fs::write(dir.join(name), "//").unwrap();
        }
        std::fs::write(&ours, "//").unwrap();

        // Only pid 99 is alive — and our own pid must survive regardless,
        // which is what makes the sweep safe to run before we write.
        sweep_orphan_scripts_in(&dir, |pid| pid == 99);

        assert!(!dir.join("beckon-act-4242.js").exists());
        assert!(!dir.join("beckon-read-4242.js").exists());
        assert!(dir.join("beckon-act-99.js").exists());
        assert!(dir.join("beckon-act-notes.js").exists());
        assert!(dir.join("kwin-act-4242.js").exists());
        assert!(ours.exists());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_sweep_survives_a_directory_that_is_not_there() {
        // Best-effort means best-effort: nothing here may fail a keypress.
        sweep_orphan_scripts_in(Path::new("/nonexistent/beckon/sweep"), |_| false);
    }

    /// Per-test scratch directory, same shape as `state.rs`'s — avoids a
    /// `tempfile` dependency for two tests.
    fn scratch_dir(label: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let dir = std::env::temp_dir().join(format!(
            "beckon-kde-test-{}-{}-{}",
            label,
            std::process::id(),
            nanos
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }
}
