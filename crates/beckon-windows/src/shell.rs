//! Two one-shot Win32 conveniences the tray app needs and the CLI does not:
//! opening a file with its registered handler, and telling the user
//! something when there is no stderr to tell them through.

use std::path::Path;
use windows::core::{HSTRING, PCWSTR};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::{
    MessageBoxW, IDNO, IDYES, MB_ICONERROR, MB_ICONINFORMATION, MB_ICONWARNING, MB_OK,
    MB_YESNOCANCEL, SW_SHOWNORMAL,
};

/// Open `path` with whatever the user has registered for it — the editor for
/// a `.toml`, the log viewer for a `.log`.
///
/// **This pumps the calling thread's message queue.** ShellExecuteW performs
/// an out-of-process shell activation, and the caller must therefore hold no
/// `RefCell` borrow across it; see `beckon-cli/src/serve.rs`'s module doc for
/// why that is a process-abort rather than a panic.
pub fn open_path(path: &Path) -> Result<(), String> {
    let wide = HSTRING::from(path.as_os_str());
    // ShellExecuteW returns a fake HINSTANCE; <= 32 means failure.
    let rc = unsafe {
        ShellExecuteW(
            None,
            windows::core::w!("open"),
            PCWSTR(wide.as_ptr()),
            PCWSTR::null(),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        )
    };
    if rc.0 as usize <= 32 {
        return Err(format!("ShellExecuteW failed for `{}`", path.display()));
    }
    Ok(())
}

/// Open Explorer with `path` selected, rather than opening `path` itself.
///
/// The System page's second glyph on each file row (design §3.3). "Show" and
/// "Open" are different questions -- one answers *where is it*, the other
/// *what does it say* -- and a settings window that could only do the second
/// leaves the user with a path they cannot act on.
///
/// **`explorer.exe /select,<path>`, not a shell verb**, because there is no
/// verb for it: `ShellExecuteW`'s `"explore"` opens a FOLDER and selects
/// nothing, and the documented route to a selected item is `IShellWindows` +
/// `SHOpenFolderAndSelectItems`, which is ~60 lines of COM for what one
/// command line does. The trade is that a malformed path becomes an Explorer
/// window at the user's Documents rather than an error -- acceptable, since
/// every path this is called with came from `serve`'s own `Paths`.
///
/// **No space after the comma, and no quotes around the path.** Explorer
/// parses `/select,` by taking the whole remainder of the command line as the
/// item, so a quoted argument is looked up WITH its quotes and a space after
/// the comma is part of the name it looks for. This is the one place in
/// beckon where the usual quoting rule is inverted, which is why it is
/// written down here rather than left to look like an omission.
///
/// **This pumps the calling thread's message queue**, exactly like
/// `open_path` above and with the same consequence for `RefCell` borrows.
pub fn reveal_path(path: &Path) -> Result<(), String> {
    let args = HSTRING::from(format!("/select,{}", path.display()));
    let rc = unsafe {
        ShellExecuteW(
            None,
            windows::core::w!("open"),
            windows::core::w!("explorer.exe"),
            PCWSTR(args.as_ptr()),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        )
    };
    if rc.0 as usize <= 32 {
        return Err(format!("cannot show `{}` in Explorer", path.display()));
    }
    Ok(())
}

/// A modal error box.
///
/// The GUI-subsystem binary has no stderr before its log is open and no
/// console ever, so for the handful of failures that happen before or
/// instead of logging, this is the only channel that reaches a person.
pub fn error_dialog(title: &str, body: &str) {
    let title = HSTRING::from(title);
    let body = HSTRING::from(body);
    unsafe {
        MessageBoxW(
            None,
            PCWSTR(body.as_ptr()),
            PCWSTR(title.as_ptr()),
            MB_OK | MB_ICONERROR,
        )
    };
}

/// What the user chose when asked whether to save before closing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaveChoice {
    Save,
    Discard,
    Cancel,
}

/// Ask before throwing away unsaved edits.
///
/// Three buttons rather than two on purpose: "don't close after all" and
/// "close and lose my edits" are different answers, and collapsing them
/// makes the safe one unreachable. An unrecognised return (the title-bar X,
/// Esc) is `Cancel` -- the choice that changes nothing.
pub fn ask_save(title: &str, body: &str) -> SaveChoice {
    let title = HSTRING::from(title);
    let body = HSTRING::from(body);
    let r = unsafe {
        MessageBoxW(
            None,
            PCWSTR(body.as_ptr()),
            PCWSTR(title.as_ptr()),
            MB_YESNOCANCEL | MB_ICONWARNING,
        )
    };
    match r {
        IDYES => SaveChoice::Save,
        IDNO => SaveChoice::Discard,
        _ => SaveChoice::Cancel,
    }
}

/// A modal informational box: same shape as `error_dialog`, without the red
/// error icon. For clap's own `--help` / `--version` output, which is not a
/// failure -- the GUI-subsystem binary has no console for that text to
/// print to, so it needs a dialog too, but not one that looks like a bug
/// report for someone who just typed `--version`.
pub fn info_dialog(title: &str, body: &str) {
    let title = HSTRING::from(title);
    let body = HSTRING::from(body);
    unsafe {
        MessageBoxW(
            None,
            PCWSTR(body.as_ptr()),
            PCWSTR(title.as_ptr()),
            MB_OK | MB_ICONINFORMATION,
        )
    };
}
