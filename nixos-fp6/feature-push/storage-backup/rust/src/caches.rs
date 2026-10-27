//! Clearing app caches (~/.cache/<app>). Runs as the user; never follows
//! symlinks (a symlinked entry is removed as a link).

use crate::util::{du, safe_name};
use std::fs;
use std::path::Path;

/// Entries "Clear all caches" leaves alone: Lomiri's own QML cache (kept by
/// boot-speed; it regenerates, but the next login would be slower).
pub const KEEP: &[&str] = &["UBports"];

fn remove_entry(p: &Path) -> std::io::Result<()> {
    let md = fs::symlink_metadata(p)?;
    if md.is_dir() {
        fs::remove_dir_all(p)
    } else {
        fs::remove_file(p)
    }
}

/// Clears one app's cache (`Some(id)`) or every cache entry except KEEP.
/// Returns the bytes freed (as counted before the removal).
pub fn clear(cache_dir: &Path, id: Option<&str>) -> Result<u64, String> {
    let md = fs::symlink_metadata(cache_dir).map_err(|e| format!("{}: {e}", cache_dir.display()))?;
    if !md.is_dir() {
        return Err(format!("{} is not a directory", cache_dir.display()));
    }
    let mut freed = 0;
    let mut errors = Vec::new();
    let targets: Vec<std::path::PathBuf> = match id {
        Some(id) => {
            if !safe_name(id) {
                return Err(format!("invalid cache name {id:?}"));
            }
            let p = cache_dir.join(id);
            if fs::symlink_metadata(&p).is_err() {
                return Ok(0);
            }
            vec![p]
        }
        None => fs::read_dir(cache_dir).map_err(|e| e.to_string())?.flatten()
            .filter(|e| !KEEP.contains(&e.file_name().to_string_lossy().as_ref()))
            .map(|e| e.path()).collect(),
    };
    for p in targets {
        let b = du(&p).bytes;
        match remove_entry(&p) {
            Ok(()) => freed += b,
            Err(e) => errors.push(format!("{}: {e}", p.file_name().unwrap_or_default().to_string_lossy())),
        }
    }
    if !errors.is_empty() && freed == 0 {
        return Err(errors.join("; "));
    }
    Ok(freed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::tempdir;

    #[test]
    fn clears_without_following_links() {
        let t = tempdir("cache");
        let c = t.join(".cache");
        let precious = t.join("precious");
        fs::create_dir_all(c.join("app1/sub")).unwrap();
        fs::create_dir_all(c.join("UBports/lomiri/qmlcache")).unwrap();
        fs::create_dir_all(&precious).unwrap();
        fs::write(precious.join("keep.txt"), "x").unwrap();
        fs::write(c.join("app1/sub/f"), vec![0u8; 5000]).unwrap();
        std::os::unix::fs::symlink(&precious, c.join("app2")).unwrap();
        std::os::unix::fs::symlink(&precious, c.join("app1/sub/link")).unwrap();
        assert!(clear(&c, Some("../precious")).is_err());
        assert!(clear(&c, Some("app1")).unwrap() >= 5000);
        assert!(!c.join("app1").exists());
        assert!(precious.join("keep.txt").exists());
        clear(&c, None).unwrap();
        assert!(fs::symlink_metadata(c.join("app2")).is_err());
        assert!(precious.join("keep.txt").exists());
        assert!(c.join("UBports/lomiri/qmlcache").exists());
        assert_eq!(clear(&c, Some("gone")).unwrap(), 0);
    }
}
