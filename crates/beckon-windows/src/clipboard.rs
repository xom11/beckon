//! One Win32 convenience: put text on the clipboard.
//!
//! Its only caller is the About page's three copy buttons (design §3.4). It
//! lives beside `shell.rs` rather than inside `settings_window` for the same
//! reason that file does -- it is a one-shot OS operation with no window
//! state in it, and the settings window is already 9 000 lines.
//!
//! **Two `windows` crate features arrive with it**, `Win32_System_DataExchange`
//! (the clipboard API itself) and `Win32_System_Memory` (the `HGLOBAL` the
//! clipboard takes ownership of). Unlike the `Win32_Security` feature
//! `prefs.rs` declined to enable for one `None` argument, both of these are
//! the whole of what this file does -- there is no narrower call that skips
//! either.

/// `GlobalFree` is filed under `Foundation` while its three siblings
/// (`GlobalAlloc`, `GlobalLock`, `GlobalUnlock`) are under `System::Memory` --
/// a windows-rs split that follows the header the function is declared in
/// rather than what it does. Named here so the next reader does not go
/// looking for it beside the allocation it undoes.
use windows::Win32::Foundation::{GlobalFree, HANDLE, HGLOBAL};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData,
};
use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};

/// `CF_UNICODETEXT` (13). `windows` 0.61 files the `CF_*` constants under
/// `System::Ole`, not `System::DataExchange` where the clipboard functions
/// live -- so naming it here is the same call `settings_window` makes for
/// `SS_CENTERIMAGE` and `SS_PATHELLIPSIS`: one number rather than a whole
/// feature (`Win32_System_Ole`, which drags in the OLE surface) for one
/// constant.
const CF_UNICODETEXT_FORMAT: u32 = 13;

/// Replace the clipboard's contents with `text`.
///
/// **Ownership of the `HGLOBAL` passes to the system on success and must not
/// be freed after it**, which is the one rule this API has and the one way to
/// get it wrong. Every failure path below therefore frees the block itself,
/// and the success path deliberately does not -- a `GlobalFree` after
/// `SetClipboardData` succeeded is a use-after-free of memory the clipboard is
/// still handing out to other processes.
///
/// **`OpenClipboard(None)`, no owner window.** The clipboard's owner matters
/// for delayed rendering (`WM_RENDERFORMAT`), which this does not use: the
/// data is materialised here and now. Passing the settings window would make
/// it the owner and put it on the hook for messages it has no arm for.
///
/// **`CF_UNICODETEXT` only.** Windows synthesises `CF_TEXT` and `CF_OEMTEXT`
/// from it for any consumer that asks, so offering the ANSI formats by hand
/// would be three copies of one string, two of them lossy for a path with
/// non-ASCII in it -- which a user profile name routinely has.
///
/// A failure is worth reporting and never worth a dialog: what is lost is one
/// clipboard write the user can retry.
pub fn set_text(text: &str) -> Result<(), String> {
    // UTF-16 with the terminating NUL, which `CF_UNICODETEXT` requires -- the
    // clipboard carries no length, so the NUL is the length.
    let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
    let bytes = std::mem::size_of_val(&wide[..]);
    unsafe {
        // `GMEM_MOVEABLE` is not a preference: `SetClipboardData` documents
        // that the handle it is given must have been allocated with it, and a
        // fixed block silently misbehaves as an `HGLOBAL` handle.
        let h: HGLOBAL =
            GlobalAlloc(GMEM_MOVEABLE, bytes).map_err(|e| format!("GlobalAlloc failed: {e}"))?;
        let dst = GlobalLock(h);
        if dst.is_null() {
            let _ = GlobalFree(Some(h));
            return Err("GlobalLock failed".into());
        }
        std::ptr::copy_nonoverlapping(wide.as_ptr(), dst as *mut u16, wide.len());
        let _ = GlobalUnlock(h);

        if OpenClipboard(None).is_err() {
            // Another process holds it -- routine, not exceptional, and the
            // block is ours to free because nothing has taken it yet.
            let _ = GlobalFree(Some(h));
            return Err("another program is holding the clipboard".into());
        }
        if EmptyClipboard().is_err() {
            let _ = GlobalFree(Some(h));
            let _ = CloseClipboard();
            return Err("EmptyClipboard failed".into());
        }
        let r = SetClipboardData(CF_UNICODETEXT_FORMAT, Some(HANDLE(h.0)));
        let _ = CloseClipboard();
        // Only NOW is the block the system's. On the error arm it is still
        // ours, and leaking it would be a leak per click.
        match r {
            Ok(_) => Ok(()),
            Err(e) => {
                let _ = GlobalFree(Some(h));
                Err(format!("SetClipboardData failed: {e}"))
            }
        }
    }
}
