// SPDX-License-Identifier: MIT
//! QML disk caches of the apps (Qt 5 .qmlc / .jsc in ~/.cache/<app>/qmlcache
//! or ~/.cache/<org>/<app>/qmlcache).
//!
//! nixpkgs' Qt 5 only uses the QML disk cache with QML_FORCE_DISK_CACHE=1
//! (qtdeclarative-default-disable-qmlcache.patch): Qt checks a cache entry
//! against the source file's URL and mtime, and every store file has mtime 1.
//! app-ready-fp6.nix sets the variable for the apps (user manager
//! DefaultEnvironment); measured on the phone 2026-10-28, System Settings,
//! Messages, Phone, Gallery... had no cache at all, so every cold start
//! compiled all their QML again.
//! An app's own QML lives in its store path (a new URL when it changes), but
//! imports can resolve through /run/current-system/sw, whose URL stays the
//! same across updates. So: when the running system changes, every app
//! cache is emptied once (before the first app of the session starts);
//! Lomiri's own cache (UBports/lomiri) is left to boot-speed's prepare step.

use std::path::{Path, PathBuf};

pub const STAMP_FILE: &str = "qmlcache-stamp";

/// qmlcache directories under the cache home (two levels), except Lomiri's.
pub fn cache_dirs(cache_home: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(top) = std::fs::read_dir(cache_home) else { return out };
    for e in top.flatten() {
        let p = e.path();
        if !e.file_type().is_ok_and(|t| t.is_dir()) {
            continue;
        }
        if p.join("qmlcache").is_dir() {
            out.push(p.join("qmlcache"));
        }
        let Ok(sub) = std::fs::read_dir(&p) else { continue };
        for s in sub.flatten() {
            let q = s.path().join("qmlcache");
            if s.file_type().is_ok_and(|t| t.is_dir()) && q.is_dir() && !q.ends_with("UBports/lomiri/qmlcache") {
                out.push(q);
            }
        }
    }
    out.sort();
    out
}

/// Remove the compiled units in the app caches when `stamp` (the running
/// system) differs from the one recorded in `state_dir`. Returns the number
/// of files removed (0 when the stamp is unchanged).
pub fn wipe_if_system_changed(cache_home: &Path, state_dir: &Path, stamp: &str) -> std::io::Result<usize> {
    let stamp_path = state_dir.join(STAMP_FILE);
    if std::fs::read_to_string(&stamp_path).is_ok_and(|s| s.trim() == stamp.trim()) {
        return Ok(0);
    }
    let mut removed = 0;
    for dir in cache_dirs(cache_home) {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for f in entries.flatten() {
            let p = f.path();
            if p.extension().is_some_and(|x| x == "qmlc" || x == "jsc") && std::fs::remove_file(&p).is_ok() {
                removed += 1;
            }
        }
    }
    std::fs::create_dir_all(state_dir)?;
    std::fs::write(&stamp_path, format!("{}\n", stamp.trim()))?;
    Ok(removed)
}

/// The running system's store path (the stamp).
pub fn system_stamp() -> String {
    std::fs::read_link("/run/current-system").map(|p| p.display().to_string()).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wipe_on_change_only() {
        let t = std::env::temp_dir().join(format!("fp6-app-ready-qmlc-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&t);
        let cache = t.join("cache");
        let state = t.join("state");
        for d in ["lomiri-system-settings/qmlcache", "camera.ubports/qmlcache", "UBports/lomiri/qmlcache", "QtProject/Qml Runtime/qmlcache"] {
            std::fs::create_dir_all(cache.join(d)).unwrap();
            std::fs::write(cache.join(d).join("a.qmlc"), b"x").unwrap();
            std::fs::write(cache.join(d).join("b.jsc"), b"x").unwrap();
            std::fs::write(cache.join(d).join("keep.txt"), b"x").unwrap();
        }
        assert_eq!(cache_dirs(&cache).len(), 3);
        assert_eq!(wipe_if_system_changed(&cache, &state, "/nix/store/aaa-system").unwrap(), 6);
        // Lomiri's cache and other files stay
        assert!(cache.join("UBports/lomiri/qmlcache/a.qmlc").exists());
        assert!(cache.join("camera.ubports/qmlcache/keep.txt").exists());
        assert!(!cache.join("camera.ubports/qmlcache/a.qmlc").exists());
        // same system: nothing removed
        std::fs::write(cache.join("camera.ubports/qmlcache/c.qmlc"), b"x").unwrap();
        assert_eq!(wipe_if_system_changed(&cache, &state, "/nix/store/aaa-system").unwrap(), 0);
        assert!(cache.join("camera.ubports/qmlcache/c.qmlc").exists());
        // new system: removed
        assert_eq!(wipe_if_system_changed(&cache, &state, "/nix/store/bbb-system").unwrap(), 1);
        std::fs::remove_dir_all(t).unwrap();
    }
}
