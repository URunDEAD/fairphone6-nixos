//! file:// URI <-> path (RFC 3986 percent-encoding of everything outside the
//! unreserved set and '/', as GLib's g_filename_to_uri does).

use std::ffi::OsString;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};

pub fn path_to_file_uri(p: &Path) -> String {
    let mut s = String::from("file://");
    for &b in p.as_os_str().as_bytes() {
        let keep = b.is_ascii_alphanumeric() || b"-._~/!$&'()*+,;=:@".contains(&b);
        if keep {
            s.push(b as char);
        } else {
            s.push_str(&format!("%{b:02X}"));
        }
    }
    s
}

pub fn file_uri_to_path(uri: &str) -> Option<PathBuf> {
    let rest = uri.strip_prefix("file://")?;
    // file://host/path: only an empty host or localhost is local.
    let rest = if rest.starts_with('/') {
        rest
    } else {
        let slash = rest.find('/')?;
        if &rest[..slash] != "localhost" {
            return None;
        }
        &rest[slash..]
    };
    let bytes = rest.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let h = std::str::from_utf8(&bytes[i + 1..i + 3]).ok()?;
            out.push(u8::from_str_radix(h, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    Some(PathBuf::from(OsString::from_vec(out)))
}

/// A NUL-terminated byte array (portal `ay` paths) as a path.
pub fn bytes_to_path(b: &[u8]) -> Option<PathBuf> {
    let b = b.strip_suffix(&[0]).unwrap_or(b);
    if b.is_empty() {
        None
    } else {
        Some(PathBuf::from(std::ffi::OsStr::from_bytes(b)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let p = Path::new("/home/u/My Files/ä#1%.torrent");
        let u = path_to_file_uri(p);
        assert_eq!(u, "file:///home/u/My%20Files/%C3%A4%231%25.torrent");
        assert_eq!(file_uri_to_path(&u).unwrap(), p);
        assert_eq!(file_uri_to_path("file://localhost/tmp/x").unwrap(), Path::new("/tmp/x"));
        assert!(file_uri_to_path("file://other/tmp/x").is_none());
        assert!(file_uri_to_path("https://x/y").is_none());
        assert_eq!(bytes_to_path(b"/tmp\0").unwrap(), Path::new("/tmp"));
        assert!(bytes_to_path(b"\0").is_none());
    }
}
