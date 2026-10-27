//! Small helpers: disk usage walks, time, atomic writes.

use std::collections::HashSet;
use std::fs;
use std::io::{self, Write};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};

pub fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Disk usage of `path` (allocated blocks, like `du`): never follows
/// symlinks, stays on the starting file system, counts hard links once.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Du {
    pub bytes: u64,
    pub files: u64,
}

pub fn du(path: &Path) -> Du {
    let mut seen = HashSet::new();
    let mut out = Du::default();
    let Ok(md) = fs::symlink_metadata(path) else { return out };
    walk(path, &md, md.dev(), &mut seen, &mut out);
    out
}

fn walk(path: &Path, md: &fs::Metadata, dev: u64, seen: &mut HashSet<(u64, u64)>, out: &mut Du) {
    if md.dev() != dev {
        return;
    }
    if md.nlink() > 1 && !md.is_dir() && !seen.insert((md.dev(), md.ino())) {
        return;
    }
    out.bytes += md.blocks() * 512;
    if md.is_file() {
        out.files += 1;
    }
    if md.is_dir() {
        let Ok(rd) = fs::read_dir(path) else { return };
        for e in rd.flatten() {
            let p = e.path();
            if let Ok(m) = fs::symlink_metadata(&p) {
                walk(&p, &m, dev, seen, out);
            }
        }
    }
}

/// Regular files under `root` (no symlinks followed, sorted), as paths
/// relative to `root`.
pub fn list_files(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![PathBuf::new()];
    while let Some(rel) = stack.pop() {
        let Ok(rd) = fs::read_dir(root.join(&rel)) else { continue };
        let mut names: Vec<_> = rd.flatten().map(|e| e.file_name()).collect();
        names.sort();
        for n in names.into_iter().rev() {
            let r = rel.join(&n);
            match fs::symlink_metadata(root.join(&r)) {
                Ok(m) if m.is_dir() => stack.push(r),
                Ok(m) if m.is_file() => out.push(r),
                _ => {}
            }
        }
    }
    out.sort();
    out
}

/// Writes `data` to `path` via a temporary file + rename (mode applied to
/// the new file).
pub fn write_atomic(path: &Path, data: &[u8], mode: u32) -> io::Result<()> {
    let dir = path.parent().unwrap_or(Path::new("."));
    fs::create_dir_all(dir)?;
    let tmp = dir.join(format!(".{}.tmp{}", path.file_name().and_then(|n| n.to_str()).unwrap_or("f"), std::process::id()));
    let _ = fs::remove_file(&tmp);
    {
        let mut f = fs::OpenOptions::new().write(true).create_new(true).mode(mode).open(&tmp)?;
        f.write_all(data)?;
        f.sync_all()?;
    }
    fs::rename(&tmp, path)?;
    if let Ok(d) = fs::File::open(dir) {
        let _ = d.sync_all();
    }
    Ok(())
}

pub fn fs_space(path: &Path) -> Option<(u64, u64, i64)> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    let c = CString::new(path.as_os_str().as_bytes()).ok()?;
    let mut s: libc::statfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::statfs(c.as_ptr(), &mut s) } != 0 {
        return None;
    }
    let bs = s.f_bsize as u64;
    Some((s.f_blocks as u64 * bs, s.f_bavail as u64 * bs, s.f_type as i64))
}

/// FAT file systems (SD cards, USB sticks) cannot hold files of 4 GiB or more.
pub const MSDOS_SUPER_MAGIC: i64 = 0x4d44;

/// A single path component that is safe to join (no separators, not . or ..).
pub fn safe_name(n: &str) -> bool {
    !n.is_empty() && n != "." && n != ".." && !n.contains('/') && !n.contains('\0') && n.len() <= 255
}

/// Relative path without "..", "." or absolute components.
pub fn safe_rel(p: &str) -> bool {
    !p.is_empty() && !p.starts_with('/') && p.split('/').all(safe_name)
}

#[cfg(test)]
pub use tests::tempdir;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn du_counts_hardlinks_once_and_skips_symlinks() {
        let t = tempdir("du");
        fs::create_dir_all(t.join("a/b")).unwrap();
        fs::write(t.join("a/b/f"), vec![1u8; 10000]).unwrap();
        fs::hard_link(t.join("a/b/f"), t.join("a/g")).unwrap();
        std::os::unix::fs::symlink("/etc", t.join("a/link")).unwrap();
        let d = du(&t.join("a"));
        assert_eq!(d.files, 1);
        assert!(d.bytes >= 10000 && d.bytes < 10000 + 3 * 8192 + 4096, "{d:?}");
        assert_eq!(list_files(&t.join("a")), vec![PathBuf::from("b/f"), PathBuf::from("g")]);
    }

    #[test]
    fn names() {
        assert!(safe_rel("Pictures/a b/c.jpg"));
        assert!(!safe_rel("../x"));
        assert!(!safe_rel("/etc/passwd"));
        assert!(!safe_rel("a//b"));
        assert!(!safe_rel("a/./b"));
        assert!(!safe_name(".."));
    }

    pub fn tempdir(tag: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("fp6sb-{tag}-{}-{}", std::process::id(), now_nanos()));
        fs::create_dir_all(&p).unwrap();
        p
    }
    fn now_nanos() -> u128 {
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
    }
}

