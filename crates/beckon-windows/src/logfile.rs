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

use anyhow::{Context, Result};
use std::fs::{File, OpenOptions};
use std::os::windows::io::AsRawHandle;
use std::path::Path;
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

/// Open `path` for appending, creating its parent directory if needed.
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
    OpenOptions::new().create(true).append(true).open(path)
}

/// Point stderr and stdout at `path`, then detach the console.
///
/// Call once, from the `--serve` path only, before anything else can fail.
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
        // console. `--serve` logs through stderr; stdout is a `LineWriter` std
        // only flushes on a newline or at a normal exit, and `--serve` exits by
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
}
