//! Local receive policy. Directory-relative, exclusive creation avoids symlink
//! and check-then-create overwrite races. No payload is opened before consent.
use anyhow::{bail, Context, Result};
use rustix::fs::{self, AtFlags, Mode, OFlags};
use std::{
    fs::File,
    path::{Component, Path},
};

#[derive(Clone, Debug)]
pub struct ReceiveConfig {
    pub directory: std::path::PathBuf,
    pub max_bytes: u64,
}
impl ReceiveConfig {
    pub fn new(directory: std::path::PathBuf) -> Self {
        Self {
            directory,
            max_bytes: 1024 * 1024 * 1024,
        }
    }
}

pub fn sanitize_name(name: &str) -> Result<String> {
    if name.is_empty() || name == "." || name == ".." || name.contains(['/', '\\', '\0']) {
        bail!("unsafe filename");
    }
    let name: String = name
        .chars()
        .map(|c| {
            if c.is_control() || matches!(c, ':' | '*' | '?' | '"' | '<' | '>' | '|') {
                '_'
            } else {
                c
            }
        })
        .collect();
    // Leave room for collision suffixes and keep UTF-8 boundaries intact.
    let mut name = name.trim().trim_matches('.').to_owned();
    while name.len() > 180 {
        name.pop();
    }
    if name.is_empty() {
        bail!("empty filename after sanitizing");
    }
    Ok(name)
}

pub fn secure_dir(path: &Path) -> Result<File> {
    if !path.is_absolute() {
        bail!("receive directory must be absolute");
    }
    let mut dir = File::from(fs::open(
        "/",
        OFlags::RDONLY | OFlags::DIRECTORY,
        Mode::empty(),
    )?);
    for component in path.components() {
        let Component::Normal(name) = component else {
            if component == Component::RootDir {
                continue;
            }
            bail!("invalid receive directory");
        };
        match fs::mkdirat(&dir, name, Mode::from_bits_truncate(0o700)) {
            Ok(()) => (),
            Err(rustix::io::Errno::EXIST) => (),
            Err(e) => return Err(e.into()),
        }
        dir = File::from(
            fs::openat(
                &dir,
                name,
                OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                Mode::empty(),
            )
            .context("receive directory contains a symlink or is inaccessible")?,
        );
    }
    fs::fchmod(&dir, Mode::from_bits_truncate(0o700))?;
    Ok(dir)
}

pub fn check_space(dir: &File, size: u64, limit: u64) -> Result<()> {
    if size > limit {
        bail!("transfer exceeds configured size limit");
    }
    let stat = fs::fstatvfs(dir)?;
    let available = (stat.f_bavail as u64).saturating_mul(stat.f_frsize as u64);
    if size.saturating_add(16 * 1024 * 1024) > available {
        bail!("insufficient free space (16 MiB reserve)");
    }
    Ok(())
}

pub fn reserve_file(dir: &File, original: &str) -> Result<(File, String)> {
    let safe = sanitize_name(original)?;
    let path = Path::new(&safe);
    let stem = path.file_stem().unwrap_or_default().to_string_lossy();
    let ext = path
        .extension()
        .map(|v| format!(".{}", v.to_string_lossy()))
        .unwrap_or_default();
    for n in 0..10_000 {
        let name = if n == 0 {
            safe.clone()
        } else {
            format!("{stem} ({n}){ext}")
        };
        match fs::openat(
            dir,
            &name,
            OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::from_bits_truncate(0o600),
        ) {
            Ok(fd) => return Ok((File::from(fd), name)),
            Err(rustix::io::Errno::EXIST) => continue,
            Err(e) => return Err(e.into()),
        }
    }
    bail!("too many filename collisions")
}

pub fn remove_file(dir: &File, name: &str) {
    let _ = fs::unlinkat(dir, name, AtFlags::empty());
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::{symlink, PermissionsExt};
    #[test]
    fn rejects_traversal_and_sanitizes_controls() {
        for n in ["", ".", "..", "/etc/passwd", "../x", "x/y", "x\\y", "x\0y"] {
            assert!(sanitize_name(n).is_err(), "{n:?}");
        }
        assert_eq!(sanitize_name("a\nb?.txt").unwrap(), "a_b_.txt");
        assert!(sanitize_name(&"é".repeat(200)).unwrap().len() <= 180);
    }
    #[test]
    fn exclusive_creation_and_symlink_protection() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("Downloads/Quick Share");
        let dir = secure_dir(&path).unwrap();
        let (f, name) = reserve_file(&dir, "report.txt").unwrap();
        assert_eq!(name, "report.txt");
        assert_eq!(f.metadata().unwrap().permissions().mode() & 0o777, 0o600);
        symlink("report.txt", path.join("report (1).txt")).unwrap();
        assert_eq!(
            reserve_file(&dir, "report.txt").unwrap().1,
            "report (2).txt"
        );
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o700
        );
        symlink(&path, tmp.path().join("alias")).unwrap();
        assert!(secure_dir(&tmp.path().join("alias")).is_err());
        assert!(check_space(&dir, u64::MAX, u64::MAX).is_err());
        assert!(check_space(&dir, 101, 100).is_err());
    }
}
