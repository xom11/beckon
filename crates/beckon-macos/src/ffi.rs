//! Minimal C FFI to ApplicationServices (AX) and CoreGraphics (CGWindowList).
//!
//! We hand-roll just the few functions we actually use. The `objc2-*` family
//! covers AppKit / Foundation; ApplicationServices and the AX API in particular
//! has no clean Rust binding we want to depend on, and the surface here is
//! tiny (≈6 functions) so an `extern "C"` block keeps the dep graph small.
//!
//! All AX functions take/return CoreFoundation types. We rely on the
//! `core-foundation` crate for `CFType` / `CFString` / `CFArray` so RAII
//! handles ref counting.

#![allow(non_snake_case, non_upper_case_globals, dead_code)]

use core_foundation::array::{CFArray, CFArrayRef};
use core_foundation::base::{CFType, CFTypeRef, TCFType};
use core_foundation::dictionary::{CFDictionary, CFDictionaryRef};
use core_foundation::number::CFNumber;
use core_foundation::string::{CFString, CFStringRef};
use std::ffi::c_void;

// ---------- AX types & error codes ----------

pub type AXUIElementRef = *mut c_void;
pub type AXError = i32;

pub const K_AX_ERROR_SUCCESS: AXError = 0;
pub const K_AX_ERROR_API_DISABLED: AXError = -25211; // process not trusted
pub const K_AX_ERROR_INVALID_UI_ELEMENT: AXError = -25202;
pub const K_AX_ERROR_NO_VALUE: AXError = -25212;
pub const K_AX_ERROR_CANNOT_COMPLETE: AXError = -25204;

// ---------- CGWindowList option bits ----------

pub const K_CG_WINDOW_LIST_OPTION_ON_SCREEN_ONLY: u32 = 1 << 0;
pub const K_CG_WINDOW_LIST_EXCLUDE_DESKTOP_ELEMENTS: u32 = 1 << 4;
pub const K_CG_NULL_WINDOW_ID: u32 = 0;

// ---------- Linked symbols ----------

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    pub fn AXIsProcessTrusted() -> bool;
    pub fn AXIsProcessTrustedWithOptions(options: CFDictionaryRef) -> bool;
    pub fn AXUIElementCreateApplication(pid: i32) -> AXUIElementRef;
    pub fn AXUIElementCopyAttributeValue(
        element: AXUIElementRef,
        attribute: CFStringRef,
        value: *mut CFTypeRef,
    ) -> AXError;
    pub fn AXUIElementSetAttributeValue(
        element: AXUIElementRef,
        attribute: CFStringRef,
        value: CFTypeRef,
    ) -> AXError;
    pub fn AXUIElementPerformAction(element: AXUIElementRef, action: CFStringRef) -> AXError;
    pub fn CFRelease(cf: CFTypeRef);
}

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    pub fn CGWindowListCopyWindowInfo(option: u32, relativeToWindow: u32) -> CFArrayRef;
}

// ---------- Convenience wrappers ----------

/// Whether the current process has been granted Accessibility permission.
/// Used by `beckon -d`. Does not prompt the user.
pub fn ax_is_process_trusted() -> bool {
    unsafe { AXIsProcessTrusted() }
}

/// Same as [`ax_is_process_trusted`] but pops the system "grant access" panel
/// when `false`. Currently unused — beckon prefers a clear error message in
/// `-d` over an unprompted system dialog from the hot path.
pub fn ax_is_process_trusted_prompt() -> bool {
    let key = CFString::from_static_string("AXTrustedCheckOptionPrompt");
    let value = core_foundation::boolean::CFBoolean::true_value();
    let dict = CFDictionary::from_CFType_pairs(&[(
        key.as_CFType(),
        value.as_CFType(),
    )]);
    unsafe { AXIsProcessTrustedWithOptions(dict.as_concrete_TypeRef()) }
}

/// Make an AX element wrapper for the given PID. The returned value owns the
/// CF reference and releases it on drop.
pub struct AxElement(AXUIElementRef);

impl AxElement {
    pub fn for_pid(pid: i32) -> Option<Self> {
        let raw = unsafe { AXUIElementCreateApplication(pid) };
        if raw.is_null() {
            None
        } else {
            Some(Self(raw))
        }
    }

    /// Wrap a raw AXUIElement WITHOUT bumping the ref count. The caller MUST
    /// `mem::forget` the returned value before drop, or arrange for some
    /// other CF wrapper to keep the underlying ref alive — otherwise `Drop`
    /// will `CFRelease` a ref this struct doesn't own and leave the original
    /// owner with a dangling pointer.
    ///
    /// Used internally to perform AX calls on a window AXUIElement that is
    /// already retained by an enclosing `CFArray<CFType>`.
    pub unsafe fn from_borrowed(raw: AXUIElementRef) -> Option<Self> {
        if raw.is_null() {
            None
        } else {
            Some(Self(raw))
        }
    }

    pub fn as_raw(&self) -> AXUIElementRef {
        self.0
    }

    /// Read an attribute as a generic CF type. Returns `None` if the attribute
    /// doesn't exist or AX returns an error (commonly because the process is
    /// not trusted).
    pub fn copy_attribute(&self, attribute: &str) -> Option<CFType> {
        let attr = CFString::new(attribute);
        let mut out: CFTypeRef = std::ptr::null();
        let err = unsafe {
            AXUIElementCopyAttributeValue(self.0, attr.as_concrete_TypeRef(), &mut out)
        };
        if err != K_AX_ERROR_SUCCESS || out.is_null() {
            return None;
        }
        // Take ownership of the returned reference.
        Some(unsafe { CFType::wrap_under_create_rule(out) })
    }

    pub fn set_attribute(&self, attribute: &str, value: &CFType) -> AXError {
        let attr = CFString::new(attribute);
        unsafe {
            AXUIElementSetAttributeValue(
                self.0,
                attr.as_concrete_TypeRef(),
                value.as_CFTypeRef(),
            )
        }
    }

    pub fn perform_action(&self, action: &str) -> AXError {
        let act = CFString::new(action);
        unsafe { AXUIElementPerformAction(self.0, act.as_concrete_TypeRef()) }
    }
}

impl Drop for AxElement {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { CFRelease(self.0 as CFTypeRef) };
        }
    }
}

/// Snapshot of every on-screen window, front-to-back.
///
/// Each entry is a CFDictionary keyed by `kCGWindow*` strings. We only need
/// `kCGWindowOwnerPID` (CFNumber) and `kCGWindowLayer` (CFNumber) — layer 0
/// is the normal app layer; menubar/dock items live on higher layers and we
/// skip them.
pub fn cg_window_list_on_screen() -> Vec<WindowSnapshot> {
    let raw = unsafe {
        CGWindowListCopyWindowInfo(
            K_CG_WINDOW_LIST_OPTION_ON_SCREEN_ONLY
                | K_CG_WINDOW_LIST_EXCLUDE_DESKTOP_ELEMENTS,
            K_CG_NULL_WINDOW_ID,
        )
    };
    if raw.is_null() {
        return Vec::new();
    }
    let array: CFArray<CFDictionary> = unsafe { CFArray::wrap_under_create_rule(raw) };
    let mut out = Vec::with_capacity(array.len() as usize);
    for i in 0..array.len() {
        let Some(dict_ref) = array.get(i) else {
            continue;
        };
        let dict: &CFDictionary = &dict_ref;
        let pid = dict_get_i64(dict, "kCGWindowOwnerPID");
        let layer = dict_get_i64(dict, "kCGWindowLayer").unwrap_or(0);
        if let Some(pid) = pid {
            // Layer 0 is the normal application layer. Anything else is
            // menubar / dock / floating UI — not what we want to focus.
            if layer != 0 {
                continue;
            }
            out.push(WindowSnapshot { pid: pid as i32 });
        }
    }
    out
}

/// One front-to-back entry from CGWindowListCopyWindowInfo.
pub struct WindowSnapshot {
    pub pid: i32,
}

fn dict_get_i64(dict: &CFDictionary, key: &str) -> Option<i64> {
    let key = CFString::new(key);
    let value = dict.find(key.as_concrete_TypeRef() as *const c_void)?;
    let raw = *value as CFTypeRef;
    if raw.is_null() {
        return None;
    }
    let num = unsafe { CFNumber::wrap_under_get_rule(raw as _) };
    num.to_i64()
}
