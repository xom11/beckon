//! Per-window operations via the macOS Accessibility API and CGWindowList.
//!
//! NSRunningApplication knows about apps and which one is active, but not
//! about individual windows. To cycle within the same app (step 5a) we need
//! AXUIElement: enumerate `AXWindows`, find which one is `AXMain`, and
//! `AXRaise` the next one.
//!
//! For step 5b (toggle to most-recent OTHER app), we read the front-to-back
//! window stack from `CGWindowListCopyWindowInfo` and pick the first window
//! whose owner pid is not ours.

use crate::apps::RunningAppInfo;
use crate::ffi::{self, AxElement};
use core_foundation::array::CFArray;
use core_foundation::base::{CFType, TCFType};
use core_foundation::boolean::CFBoolean;
use objc2_app_kit::NSApplicationActivationOptions;

/// AX handles for one window of an app. `main_index` flags which entry in
/// the `AXWindows` array is currently the focused (main) window.
struct AppWindows {
    elements: Vec<CFType>,
    main_index: Option<usize>,
}

fn collect_app_windows(pid: i32) -> Option<AppWindows> {
    let app = AxElement::for_pid(pid)?;
    let windows_value = app.copy_attribute("AXWindows")?;

    // AXWindows is a CFArray of AXUIElement. We can't downcast directly to
    // CFArray<AXUIElement> because AXUIElement isn't a `TCFType` in the
    // core-foundation crate, so we reinterpret as `CFArray<CFType>` and read
    // raw refs out.
    let array_ref = windows_value.as_concrete_TypeRef();
    let array: CFArray<CFType> = unsafe { CFArray::wrap_under_get_rule(array_ref as _) };

    let mut elements = Vec::with_capacity(array.len() as usize);
    let mut main_index: Option<usize> = None;
    for i in 0..array.len() {
        let Some(item) = array.get(i) else { continue };
        let raw = item.as_concrete_TypeRef();
        // Wrap the AXUIElement so its lifetime extends past the temporary
        // `windows_value`. Each AXUIElement we read here is retained by the
        // outer CFArray; we increment the ref count so the per-window CFType
        // owns its own reference and survives independently.
        let cf = unsafe { CFType::wrap_under_get_rule(raw) };

        // Probe AXMain on this window element (don't have a generic AxElement
        // wrapper for the window — just call the FFI directly).
        let win_elem = unsafe { AxElement::from_borrowed(raw as _) };
        if let Some(win) = win_elem {
            // from_owned takes ownership without retaining; since we already
            // bumped the ref count above for `cf`, we don't want a double
            // release. Forget the AxElement after using it.
            let is_main = win
                .copy_attribute("AXMain")
                .map(|v| {
                    let b = unsafe { CFBoolean::wrap_under_get_rule(v.as_concrete_TypeRef() as _) };
                    b.into()
                })
                .unwrap_or(false);
            std::mem::forget(win);
            if is_main && main_index.is_none() {
                main_index = Some(i as usize);
            }
        }
        elements.push(cf);
    }

    Some(AppWindows {
        elements,
        main_index,
    })
}

/// Try to raise the next window of the same app (step 5a). Returns `true`
/// when another window was raised, `false` when the app has only one window
/// (or AX permission is missing — can't tell the difference reliably).
pub fn cycle_to_next_window(pid: i32) -> bool {
    let Some(windows) = collect_app_windows(pid) else {
        return false;
    };
    if windows.elements.len() < 2 {
        return false;
    }
    let current = windows.main_index.unwrap_or(0);
    let next = (current + 1) % windows.elements.len();
    let target = &windows.elements[next];
    let raw = target.as_concrete_TypeRef();
    let elem = match unsafe { AxElement::from_borrowed(raw as _) } {
        Some(e) => e,
        None => return false,
    };
    let err = elem.perform_action("AXRaise");
    std::mem::forget(elem); // ref count owned by the CFType in `windows.elements`
    err == ffi::K_AX_ERROR_SUCCESS
}

/// Front-to-back app stack (PIDs of regular layer-0 windows, most recent
/// first, deduplicated). Used by step 5b to pick the most-recent OTHER app.
pub fn pid_stack_front_to_back() -> Vec<i32> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for w in ffi::cg_window_list_on_screen() {
        if seen.insert(w.pid) {
            out.push(w.pid);
        }
    }
    out
}

/// Hide the app (NSRunningApplication.hide). Returns true on success.
pub fn hide_app(app: &RunningAppInfo) -> bool {
    app.running.hide()
}

/// Activate (focus) the app. We pass empty options — that brings only the
/// main / key window forward, matching what step 5a wants. Passing
/// `ActivateAllWindows` would un-do whatever cycle/raise the user just did.
pub fn activate_app(app: &RunningAppInfo) -> bool {
    app.running
        .activateWithOptions(NSApplicationActivationOptions::empty())
}
