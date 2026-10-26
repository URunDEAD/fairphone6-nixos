//! Opening an image the helper is asked to host, without trusting the path.
//!
//!  * The path must be absolute, lexically inside one of the allowed
//!    directories (configured by root), with no "." / ".." / empty
//!    components.
//!  * The allowed directory is opened with openat2(RESOLVE_NO_SYMLINKS) and
//!    the rest beneath it with RESOLVE_BENEATH | RESOLVE_NO_SYMLINKS |
//!    RESOLVE_NO_MAGICLINKS | RESOLVE_NO_XDEV: a symlink anywhere (also a
//!    replaced Images directory) is refused by the kernel, not by a racy
//!    check.
//!  * The opened file must be a regular file owned by the caller, with one
//!    link (no hard link to someone else's file), not empty.
//!  * The kernel is then given "/proc/self/fd/N" of exactly this open file,
//!    so a rename/symlink swap after the checks cannot change what is hosted.

use std::ffi::CString;
use std::fs::File;
use std::os::fd::{FromRawFd, OwnedFd, AsRawFd};
use std::os::unix::fs::MetadataExt;
use std::path::{Component, Path, PathBuf};

const RESOLVE_NO_XDEV: u64 = 0x01;
const RESOLVE_NO_MAGICLINKS: u64 = 0x02;
const RESOLVE_NO_SYMLINKS: u64 = 0x04;
const RESOLVE_BENEATH: u64 = 0x08;

#[repr(C)]
struct OpenHow {
    flags: u64,
    mode: u64,
    resolve: u64,
}

fn openat2(dirfd: i32, path: &str, flags: i32, resolve: u64) -> std::io::Result<OwnedFd> {
    let c = CString::new(path).map_err(|_| std::io::Error::from_raw_os_error(libc::EINVAL))?;
    let how = OpenHow { flags: (flags | libc::O_CLOEXEC) as u64, mode: 0, resolve };
    let fd = unsafe {
        libc::syscall(libc::SYS_openat2, dirfd, c.as_ptr(), &how as *const OpenHow, std::mem::size_of::<OpenHow>())
    };
    if fd < 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(unsafe { OwnedFd::from_raw_fd(fd as i32) })
    }
}

/// Lexical check; returns (allowed dir, path relative to it).
pub fn split_allowed<'a>(path: &'a str, allowed: &'a [PathBuf]) -> Result<(&'a Path, PathBuf), String> {
    if path.is_empty() || path.len() > 4000 || path.contains('\0') {
        return Err("invalid path".into());
    }
    let p = Path::new(path);
    if !p.is_absolute() {
        return Err("the image path must be absolute".into());
    }
    // Reject ".", ".." and "//" before anything is resolved.
    if path.split('/').skip(1).any(|c| c.is_empty() || c == "." || c == "..") {
        return Err("the image path must not contain '.', '..' or empty components".into());
    }
    for dir in allowed {
        if let Ok(rel) = p.strip_prefix(dir) {
            if rel.as_os_str().is_empty() {
                continue;
            }
            if rel.components().all(|c| matches!(c, Component::Normal(_))) {
                return Ok((dir.as_path(), rel.to_path_buf()));
            }
        }
    }
    Err(format!("{path} is not inside an allowed image folder"))
}

#[derive(Debug)]
pub struct Opened {
    pub file: File,
    pub size: u64,
}

/// Opens the image read-only or read-write after all checks. `caller_uid`
/// must own the file (root may host anything inside the allowed dirs).
pub fn open_image(path: &str, allowed: &[PathBuf], caller_uid: u32, write: bool) -> Result<Opened, String> {
    let (dir, rel) = split_allowed(path, allowed)?;
    let dirfd = openat2(libc::AT_FDCWD, dir.to_str().ok_or("bad dir")?, libc::O_PATH | libc::O_DIRECTORY, RESOLVE_NO_SYMLINKS)
        .map_err(|e| format!("image folder {}: {e}", dir.display()))?;
    let flags = if write { libc::O_RDWR } else { libc::O_RDONLY } | libc::O_NOCTTY | libc::O_NOFOLLOW;
    let fd = openat2(dirfd.as_raw_fd(), rel.to_str().ok_or("bad path")?, flags,
                     RESOLVE_BENEATH | RESOLVE_NO_SYMLINKS | RESOLVE_NO_MAGICLINKS | RESOLVE_NO_XDEV)
        .map_err(|e| match e.raw_os_error() {
            Some(libc::ELOOP) | Some(libc::EXDEV) => format!("{path}: symbolic links and other file systems are not allowed"),
            _ => format!("{path}: {e}"),
        })?;
    let file = File::from(fd);
    let m = file.metadata().map_err(|e| e.to_string())?;
    if !m.file_type().is_file() {
        return Err(format!("{path} is not a regular file"));
    }
    if caller_uid != 0 && m.uid() != caller_uid {
        return Err(format!("{path} is not owned by you"));
    }
    if m.nlink() != 1 {
        return Err(format!("{path} has more than one hard link"));
    }
    if m.len() == 0 {
        return Err(format!("{path} is empty"));
    }
    if write && m.permissions().readonly() {
        return Err(format!("{path} is read-only; make it writable to host it read-write"));
    }
    Ok(Opened { size: m.len(), file })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn me() -> u32 {
        unsafe { libc::getuid() }
    }

    #[test]
    fn lexical() {
        let allowed = vec![PathBuf::from("/home/user/Images")];
        assert!(split_allowed("/home/user/Images/a.iso", &allowed).is_ok());
        assert!(split_allowed("/home/user/Images/sub/a.iso", &allowed).is_ok());
        for bad in ["/home/user/Images", "/home/user/Images/", "/home/user/Images/../x", "/home/user/Images/./a",
                    "/home/user/Images//a", "relative/a.iso", "/home/user/ImagesX/a.iso", "/etc/shadow", ""] {
            assert!(split_allowed(bad, &allowed).is_err(), "{bad}");
        }
    }

    #[test]
    fn opening() {
        let base = std::env::temp_dir().join(format!("fp6ud-safe-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let img = base.join("Images");
        std::fs::create_dir_all(img.join("sub")).unwrap();
        std::fs::write(img.join("a.iso"), b"data").unwrap();
        std::fs::write(img.join("empty.img"), b"").unwrap();
        std::fs::write(base.join("outside"), b"secret").unwrap();
        std::os::unix::fs::symlink(base.join("outside"), img.join("link.iso")).unwrap();
        std::os::unix::fs::symlink(&base, img.join("sub/up")).unwrap();
        std::fs::hard_link(base.join("outside"), img.join("hard.iso")).unwrap();
        let allowed = vec![img.clone()];
        let p = |n: &str| img.join(n).to_string_lossy().into_owned();
        let ok = open_image(&p("a.iso"), &allowed, me(), false).unwrap();
        assert_eq!(ok.size, 4);
        assert!(open_image(&p("a.iso"), &allowed, me(), true).is_ok());
        assert!(open_image(&p("link.iso"), &allowed, me(), false).unwrap_err().contains("symbolic"));
        assert!(open_image(&p("sub/up/outside"), &allowed, me(), false).is_err());
        assert!(open_image(&p("hard.iso"), &allowed, me(), false).unwrap_err().contains("hard link"));
        assert!(open_image(&p("empty.img"), &allowed, me(), false).unwrap_err().contains("empty"));
        assert!(open_image(&p("sub"), &allowed, me(), false).is_err());
        if me() != 0 {
            assert!(open_image(&p("a.iso"), &allowed, me() + 1, false).unwrap_err().contains("owned"));
        }
        // The allowed folder itself replaced by a symlink.
        let img2 = base.join("Images2");
        std::os::unix::fs::symlink(&img, &img2).unwrap();
        let allowed2 = vec![img2.clone()];
        assert!(open_image(&img2.join("a.iso").to_string_lossy(), &allowed2, me(), false).is_err());
        std::fs::remove_dir_all(base).ok();
    }
}
