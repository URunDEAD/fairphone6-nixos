//! C ABI for the Qt shim (app/). All strings are UTF-8 JSON; strings
//! returned by Rust are released with fp6_usbdrive_string_free.

use crate::ops::Backend;
use std::ffi::{c_char, CStr, CString};
use std::sync::Arc;

pub struct Handle(Arc<Backend>);

fn to_c(s: String) -> *mut c_char {
    CString::new(s.replace('\0', "")).map(|c| c.into_raw()).unwrap_or(std::ptr::null_mut())
}

unsafe fn from_c<'a>(p: *const c_char) -> &'a str {
    if p.is_null() {
        ""
    } else {
        CStr::from_ptr(p).to_str().unwrap_or("")
    }
}

#[no_mangle]
pub extern "C" fn fp6_usbdrive_new() -> *mut Handle {
    Box::into_raw(Box::new(Handle(Arc::new(Backend::new()))))
}

/// Runs a command (name, JSON object argument); returns JSON.
///
/// # Safety
/// `h` from fp6_usbdrive_new; `name`/`arg` NUL-terminated or NULL.
#[no_mangle]
pub unsafe extern "C" fn fp6_usbdrive_command(h: *mut Handle, name: *const c_char, arg: *const c_char) -> *mut c_char {
    let Some(h) = h.as_ref() else { return to_c("{}".into()) };
    let arg: serde_json::Value = serde_json::from_str(from_c(arg)).unwrap_or(serde_json::json!({}));
    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| h.0.command(from_c(name), &arg)))
        .unwrap_or_else(|_| serde_json::json!({"ok": false, "error": "internal error"}));
    to_c(r.to_string())
}

/// # Safety
/// `h` from fp6_usbdrive_new, not used afterwards.
#[no_mangle]
pub unsafe extern "C" fn fp6_usbdrive_free(h: *mut Handle) {
    if !h.is_null() {
        drop(Box::from_raw(h));
    }
}

/// # Safety
/// `s` returned by fp6_usbdrive_command, freed once.
#[no_mangle]
pub unsafe extern "C" fn fp6_usbdrive_string_free(s: *mut c_char) {
    if !s.is_null() {
        drop(CString::from_raw(s));
    }
}
