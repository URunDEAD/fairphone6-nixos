//! C ABI for the Qt shim. All strings are UTF-8, NUL-terminated; strings
//! returned by Rust must be released with fp6_string_free.

use crate::tools::Feed;
use std::ffi::{c_char, CStr, CString};

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

/// Creates a feed ("compass", "level", ...). NULL for an unknown kind.
#[no_mangle]
pub unsafe extern "C" fn fp6_feed_new(kind: *const c_char) -> *mut Feed {
    match Feed::new(from_c(kind)) {
        Some(f) => Box::into_raw(Box::new(f)),
        None => std::ptr::null_mut(),
    }
}

/// Starts (true) or stops (false) sampling.
#[no_mangle]
pub unsafe extern "C" fn fp6_feed_set_active(f: *mut Feed, on: bool) {
    if let Some(f) = f.as_mut() {
        f.set_active(on);
    }
}

/// Latest snapshot as JSON, or NULL if nothing new since the last call.
#[no_mangle]
pub unsafe extern "C" fn fp6_feed_take(f: *mut Feed) -> *mut c_char {
    match f.as_ref().and_then(|f| f.take()) {
        Some(s) => to_c(s),
        None => std::ptr::null_mut(),
    }
}

/// Runs a command; returns a JSON object.
#[no_mangle]
pub unsafe extern "C" fn fp6_feed_command(f: *mut Feed, name: *const c_char, arg: *const c_char) -> *mut c_char {
    match f.as_ref() {
        Some(f) => to_c(f.command(from_c(name), from_c(arg))),
        None => to_c("{}".into()),
    }
}

#[no_mangle]
pub unsafe extern "C" fn fp6_feed_free(f: *mut Feed) {
    if !f.is_null() {
        drop(Box::from_raw(f));
    }
}

#[no_mangle]
pub unsafe extern "C" fn fp6_string_free(s: *mut c_char) {
    if !s.is_null() {
        drop(CString::from_raw(s));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        unsafe {
            let k = CString::new("gps").unwrap();
            let f = fp6_feed_new(k.as_ptr());
            assert!(!f.is_null());
            let n = CString::new("format").unwrap();
            let a = CString::new("45.75,21.23").unwrap();
            let r = fp6_feed_command(f, n.as_ptr(), a.as_ptr());
            let s = CStr::from_ptr(r).to_str().unwrap().to_string();
            assert!(s.contains("\"utm\":\"34T"), "{s}");
            fp6_string_free(r);
            assert!(fp6_feed_take(f).is_null());
            fp6_feed_free(f);
            let bad = CString::new("nope").unwrap();
            assert!(fp6_feed_new(bad.as_ptr()).is_null());
        }
    }
}
