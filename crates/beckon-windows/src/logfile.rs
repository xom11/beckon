//! `--log <PATH>`: send this process's stderr and stdout to a file, then
//! detach from the console.
//!
//! Exists so a Scheduled Task can invoke `beckon.exe` directly. Before this,
//! the task went through `cmd.exe` purely for a `2>` redirect — Task Scheduler
//! discards a process's stderr, and stderr is where beckon reports how many
//! hotkeys actually registered — and then through a `wscript.exe` VBScript
//! shim purely to hide the console window `cmd.exe` left on the desktop.
//! VBScript is a deprecated feature-on-demand; this module removes both hops.
//!
//! **Why no call site had to change.** std's Windows stdio deliberately does
//! not cache the handle: `sys::stdio::windows::write` calls
//! `get_handle(STD_ERROR_HANDLE)` -> `GetStdHandle` on *every* write, with a
//! comment naming `SetStdHandle` as the reason (rust-lang/rust#40490). So
//! swapping the handle redirects every `eprintln!` in the process, including
//! the ones in beckon-cli and this crate. std pins this with its own
//! regression test, `library/std/tests/switch-stdout.rs`. Verified identical
//! at the workspace's 1.75 floor and at 1.97.
//!
//! **Ordering is load-bearing.** Everything that can fail runs while the
//! console is still attached, because `main` reports errors with `eprintln!`
//! and `std::io::stdio::print_to` panics rather than returning on a write
//! error. An `Err` from after the detach would turn a clean `exit(1)` into a
//! silent panic in a process nobody is watching.
//!
//! **That promise is void in a GUI-subsystem process** (`beckon-serve.exe`):
//! there is no console at any point, not even before the detach, so an
//! `Err` from here reaches nobody through stderr. `serve_app_main` owns its
//! own failure path — a MessageBox — and calls this before anything else
//! can print. `FreeConsole` itself simply fails there, which is why it is
//! already `let _ =`.

use anyhow::{Context, Result};
use std::fs::{File, OpenOptions};
use std::os::windows::io::AsRawHandle;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use windows::Win32::Foundation::HANDLE;
use windows::Win32::System::Console::{
    FreeConsole, SetStdHandle, STD_ERROR_HANDLE, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE,
};

/// The redirected file, parked in a `static` so it is never dropped.
///
/// `SetStdHandle` stores the handle value without duplicating it, so dropping
/// the `File` would `CloseHandle` it and leave stderr pointing at a dead — and
/// eventually recycled — handle value. A `static` is the cheapest way to say
/// "this lives as long as the process", which is exactly the lifetime the std
/// handle slots need.
static LOG_FILE: OnceLock<File> = OnceLock::new();

/// Roll the log aside once it passes this. Bounds the pair at twice this.
///
/// 5 MiB is roughly three months of a 5-minute watchdog, which is the only
/// writer that produces a line on a schedule rather than on an event.
const MAX_LOG_BYTES: u64 = 5 * 1024 * 1024;

/// Open `path` for appending, creating its parent directory if needed and
/// rolling the previous log aside once it gets too big.
///
/// Append, not truncate, and that is a deliberate change from the `cmd /c …
/// 2> log` this replaces: `2>` truncates on every start, so under the task's
/// `RestartOnFailure` the log explaining why the daemon died was destroyed by
/// the restart that followed it.
///
/// Creating the parent is not a convenience either. It removes the most likely
/// way this fails, and a failure here is the one message that cannot be
/// surfaced well: it happens before the redirect, on a console the task is
/// about to discard, and a console counts as a terminal so the notification
/// policy stays quiet about it.
fn open_log(path: &Path) -> std::io::Result<File> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    roll_if_oversized(path, MAX_LOG_BYTES);
    OpenOptions::new().create(true).append(true).open(path)
}

/// Rename `path` to `path` + `.1` once it exceeds `max`, discarding whatever
/// `.1` held before. One generation, no timestamps, no index shuffling.
///
/// **Checked when the file is opened, which is why no timer is needed.** The
/// check runs once per process start, and that frequency lands where the
/// growth is by itself: the daemon opens its log once per logon and writes a
/// couple of lines per boot, while the watchdog opens *its* log every five
/// minutes and is the only writer that produces a line on a schedule. So the
/// log that grows is also the one checked often, with no background thread,
/// and `beckon <id>` never reaches this code at all.
///
/// Best-effort throughout: a log that cannot be rolled is not a reason to
/// refuse to serve. On the rename losing a race with a live writer — std opens
/// files with `FILE_SHARE_DELETE`, so a rename succeeds even while another
/// process holds the file, and that process keeps writing to the renamed
/// inode until it reopens. Harmless here because each task owns its own log
/// and the watchdog is short-lived; point two long-lived writers at one path
/// and that stops being true.
fn roll_if_oversized(path: &Path, max: u64) {
    // A missing file is not oversized, and a stat that fails is not a reason
    // to refuse to log.
    if !std::fs::metadata(path).is_ok_and(|meta| meta.len() > max) {
        return;
    }
    // Append to the whole file name rather than `with_extension`, which would
    // turn `serve.log` into `serve.1` and lose which file it came from.
    let mut rolled = path.as_os_str().to_os_string();
    rolled.push(".1");
    let _ = std::fs::rename(path, PathBuf::from(rolled));
}

/// Point stderr and stdout at `path`, then detach the console.
///
/// Call once, from the `serve` path only, before anything else can fail.
pub fn redirect_to_log(path: &Path) -> Result<()> {
    let file = open_log(path).with_context(|| format!("open log file `{}`", path.display()))?;
    // Park the owner before publishing the handle: the moment `SetStdHandle`
    // returns, any thread may write through it.
    let file = LOG_FILE.get_or_init(|| file);
    let handle = HANDLE(file.as_raw_handle());

    // The last fallible step, and it runs with the console still attached so a
    // failure can still be reported the ordinary way.
    unsafe { SetStdHandle(STD_ERROR_HANDLE, handle) }.context("SetStdHandle(stderr)")?;

    // Everything below is best-effort by design — see the module doc.
    unsafe {
        // stdout too, so the detach below leaves no slot pointing at the dead
        // console. `serve` logs through stderr; stdout is a `LineWriter` std
        // only flushes on a newline or at a normal exit, and `serve` exits by
        // being killed.
        let _ = SetStdHandle(STD_OUTPUT_HANDLE, handle);
        let _ = FreeConsole();
        // Re-arm: whether `FreeConsole` clears the slots is undocumented, and
        // two syscalls are cheaper than depending on the answer.
        let _ = SetStdHandle(STD_ERROR_HANDLE, handle);
        let _ = SetStdHandle(STD_OUTPUT_HANDLE, handle);
        // stdin has no replacement, and leaving it pointing at a closed console
        // handle would hand that stale value to any child spawned with the
        // default inherit behaviour. NULL is what std itself treats as "no
        // handle".
        let _ = SetStdHandle(STD_INPUT_HANDLE, HANDLE(std::ptr::null_mut()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_log_creates_missing_parent_directories() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("does/not/exist/serve.log");
        let _file = open_log(&path).expect("open_log");
        assert!(path.exists(), "open_log must create the parent directory");
    }

    #[test]
    fn open_log_appends_instead_of_truncating() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("serve.log");
        std::fs::write(&path, "previous run\n").unwrap();

        {
            use std::io::Write;
            let mut file = open_log(&path).expect("open_log");
            writeln!(file, "this run").unwrap();
        }

        let body = std::fs::read_to_string(&path).unwrap();
        assert!(
            body.contains("previous run") && body.contains("this run"),
            "append mode must keep the previous run's evidence: {body:?}"
        );
    }

    #[test]
    fn roll_leaves_a_file_under_the_limit_alone() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("serve.log");
        std::fs::write(&path, "small\n").unwrap();

        roll_if_oversized(&path, 1024);

        assert_eq!(std::fs::read_to_string(&path).unwrap(), "small\n");
        assert!(
            !dir.path().join("serve.log.1").exists(),
            "rolling a small log would throw away history for nothing"
        );
    }

    #[test]
    fn roll_moves_an_oversized_file_aside_and_starts_fresh() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("serve.log");
        std::fs::write(&path, "x".repeat(200)).unwrap();

        roll_if_oversized(&path, 100);

        assert!(
            !path.exists(),
            "the oversized log must be moved out of the way"
        );
        assert_eq!(
            std::fs::read_to_string(dir.path().join("serve.log.1"))
                .unwrap()
                .len(),
            200,
            "the previous generation keeps its contents, under `.log.1`"
        );
    }

    #[test]
    fn roll_keeps_exactly_one_generation() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("serve.log");
        let rolled = dir.path().join("serve.log.1");

        std::fs::write(&path, "first".repeat(50)).unwrap();
        roll_if_oversized(&path, 100);
        std::fs::write(&path, "second".repeat(50)).unwrap();
        roll_if_oversized(&path, 100);

        assert!(
            std::fs::read_to_string(&rolled).unwrap().contains("second"),
            "the second roll must replace the first: bounded at two files, not N"
        );
        assert!(
            !dir.path().join("serve.log.1.1").exists() && !dir.path().join("serve.log.2").exists(),
            "no index shuffling, no third file"
        );
    }

    #[test]
    fn open_log_rolls_before_it_appends() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("serve.log");
        // Bigger than the real limit, so this exercises `open_log`'s own
        // constant rather than a test-only one.
        std::fs::write(&path, vec![b'x'; (MAX_LOG_BYTES + 1) as usize]).unwrap();

        {
            use std::io::Write;
            let mut file = open_log(&path).expect("open_log");
            writeln!(file, "after the roll").unwrap();
        }

        let body = std::fs::read_to_string(&path).unwrap();
        assert_eq!(
            body, "after the roll\n",
            "the live log restarts empty; the old bytes live in .log.1"
        );
        assert!(dir.path().join("serve.log.1").exists());
    }
}
