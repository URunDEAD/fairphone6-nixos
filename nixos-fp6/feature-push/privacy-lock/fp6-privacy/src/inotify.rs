// SPDX-License-Identifier: GPL-3.0-only
//! Minimal blocking inotify (libc), enough to wait for "something changed".

use std::ffi::CString;
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

pub struct Inotify {
    fd: libc::c_int,
}

pub const OPEN: u32 = libc::IN_OPEN;
pub const CLOSE: u32 = libc::IN_CLOSE_WRITE | libc::IN_CLOSE_NOWRITE;
pub const DIR_CHANGES: u32 = libc::IN_CLOSE_WRITE | libc::IN_MOVED_TO | libc::IN_DELETE | libc::IN_CREATE;

impl Inotify {
    pub fn new() -> io::Result<Self> {
        let fd = unsafe { libc::inotify_init1(libc::IN_CLOEXEC) };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Self { fd })
    }

    pub fn add(&self, path: &Path, mask: u32) -> io::Result<i32> {
        let c = CString::new(path.as_os_str().as_bytes()).map_err(|_| io::ErrorKind::InvalidInput)?;
        let wd = unsafe { libc::inotify_add_watch(self.fd, c.as_ptr(), mask) };
        if wd < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(wd)
    }

    /// Wait up to `timeout_ms` (-1 = forever) for events; true if any came
    /// (they are drained and not decoded: callers rescan).
    pub fn wait(&self, timeout_ms: i32) -> io::Result<bool> {
        let mut pfd = libc::pollfd { fd: self.fd, events: libc::POLLIN, revents: 0 };
        let r = unsafe { libc::poll(&mut pfd, 1, timeout_ms) };
        if r < 0 {
            let e = io::Error::last_os_error();
            if e.kind() == io::ErrorKind::Interrupted {
                return Ok(false);
            }
            return Err(e);
        }
        if r == 0 {
            return Ok(false);
        }
        let mut buf = [0u8; 4096];
        let n = unsafe { libc::read(self.fd, buf.as_mut_ptr().cast(), buf.len()) };
        if n < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(true)
    }

    /// Drain everything that arrives within `ms` (debounce a burst).
    pub fn settle(&self, ms: i32) {
        while let Ok(true) = self.wait(ms) {}
    }
}

impl Drop for Inotify {
    fn drop(&mut self) {
        unsafe { libc::close(self.fd) };
    }
}
