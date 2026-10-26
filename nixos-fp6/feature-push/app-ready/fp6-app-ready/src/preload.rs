// SPDX-License-Identifier: MIT
//! Page-cache preload of an app's files.
//!
//! File list, in priority order (deduplicated, regular files only):
//!  1. learned: files the app had mapped while it ran (binary, shared
//!     libraries, QML disk caches, fonts, data), kept in
//!     ~/.cache/fp6-app-ready/<appid>.files;
//!  2. extra lists /etc/fp6-app-ready/preload.d/<appid>.list and
//!     ~/.config/fp6-app-ready/preload.d/<appid>.list (a file or a directory
//!     per line; directories are walked);
//!  3. fallback: the app's own Nix store path (from the desktop Exec), walked.
//!
//! Up to the budget (bytes, per app). Each file gets posix_fadvise(WILLNEED):
//! the kernel reads it in asynchronously and the pages stay reclaimable cache.
//! Optional mlock of a small hot set (first files of the list) keeps those
//! pages resident; the mappings live as long as the daemon holds them.

use std::collections::HashSet;
use std::fs::File;
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::io::AsRawFd;
use std::path::{Path, PathBuf};

use crate::apps;

const MAX_LEARNED: usize = 6000;
const MAX_WALK: usize = 20000;

pub fn cache_dir() -> PathBuf {
    let base = std::env::var_os("XDG_CACHE_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".cache"));
    base.join("fp6-app-ready")
}

fn config_dir() -> PathBuf {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".config"));
    base.join("fp6-app-ready")
}

fn safe_id(app_id: &str) -> bool {
    !app_id.is_empty() && !app_id.contains('/') && app_id != "." && app_id != ".."
}

pub fn learned_path(app_id: &str) -> PathBuf {
    cache_dir().join(format!("{app_id}.files"))
}

pub fn read_list(path: &Path) -> Vec<PathBuf> {
    std::fs::read_to_string(path)
        .map(|t| {
            t.lines()
                .map(str::trim)
                .filter(|l| l.starts_with('/'))
                .map(PathBuf::from)
                .collect()
        })
        .unwrap_or_default()
}

/// Merge newly seen mapped files into the learned list (old order first).
pub fn learn(app_id: &str, files: &[PathBuf]) -> std::io::Result<usize> {
    if !safe_id(app_id) {
        return Ok(0);
    }
    let path = learned_path(app_id);
    let mut list = read_list(&path);
    let mut seen: HashSet<PathBuf> = list.iter().cloned().collect();
    let before = list.len();
    for f in files {
        if list.len() >= MAX_LEARNED {
            break;
        }
        if seen.insert(f.clone()) && f.is_file() {
            list.push(f.clone());
        }
    }
    // Drop files that no longer exist (old store paths after an update).
    list.retain(|f| f.exists());
    if list.len() != before || list.len() != seen.len() {
        std::fs::create_dir_all(cache_dir())?;
        let tmp = path.with_extension("files.tmp");
        let text: String = list.iter().map(|p| format!("{}\n", p.display())).collect();
        std::fs::write(&tmp, text)?;
        std::fs::rename(&tmp, &path)?;
    }
    Ok(list.len().saturating_sub(before))
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>, limit: usize) {
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&d) else { continue };
        let mut entries: Vec<_> = entries.flatten().collect();
        entries.sort_by_key(|e| e.file_name());
        for e in entries {
            if out.len() >= limit {
                return;
            }
            let Ok(t) = e.file_type() else { continue };
            if t.is_dir() {
                stack.push(e.path());
            } else if t.is_file() {
                out.push(e.path());
            }
        }
    }
}

/// /nix/store/<hash>-<name> prefix of a path.
pub fn store_root(path: &Path) -> Option<PathBuf> {
    let mut comps = path.components();
    let mut root = PathBuf::new();
    for want in ["/", "nix", "store"] {
        let c = comps.next()?;
        if c.as_os_str() != want {
            return None;
        }
        root.push(c);
    }
    root.push(comps.next()?);
    Some(root)
}

/// Store paths a (wrapper) script points to: "/nix/store/<hash>-<name>" words.
fn script_targets(path: &Path) -> Vec<PathBuf> {
    let Ok(meta) = std::fs::metadata(path) else { return Vec::new() };
    if meta.len() > 1 << 20 {
        return Vec::new();
    }
    let Ok(bytes) = std::fs::read(path) else { return Vec::new() };
    if !bytes.starts_with(b"#!") {
        return Vec::new();
    }
    let text = String::from_utf8_lossy(&bytes);
    let mut out: Vec<PathBuf> = Vec::new();
    for (i, _) in text.match_indices("/nix/store/") {
        let rest = &text[i..];
        let end = rest
            .find(|c: char| c.is_whitespace() || matches!(c, '"' | '\'' | ':' | ';' | ')' | '}' | '$'))
            .unwrap_or(rest.len());
        if let Some(root) = store_root(Path::new(&rest[..end])) {
            if !out.contains(&root) {
                out.push(root);
            }
        }
    }
    out
}

/// The app's package and, through wrapper scripts (makeWrapper, NixOS
/// wrappers), the wrapped packages that carry the program, its QML and data.
/// Library dependencies are only covered by the learned list.
pub fn package_roots(bin: &Path) -> Vec<PathBuf> {
    let mut roots: Vec<PathBuf> = store_root(bin).into_iter().collect();
    let mut files = vec![bin.to_path_buf()];
    for _depth in 0..3 {
        let mut next = Vec::new();
        for f in &files {
            for root in script_targets(f) {
                // Only packages that ship a program (skip bash, coreutils, data-only deps).
                if roots.contains(&root) || !root.join("bin").is_dir() {
                    continue;
                }
                let name = root.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                if ["bash", "coreutils", "gnused", "gnugrep", "findutils", "glibc", "util-linux", "dbus"]
                    .iter()
                    .any(|skip| name.split_once('-').is_some_and(|(_, n)| n.starts_with(skip)))
                {
                    continue;
                }
                if let Ok(entries) = std::fs::read_dir(root.join("bin")) {
                    next.extend(entries.flatten().map(|e| e.path()));
                }
                roots.push(root);
            }
        }
        if next.is_empty() {
            break;
        }
        files = next;
    }
    roots
}

fn expand(entry: &Path, out: &mut Vec<PathBuf>) {
    if entry.is_dir() {
        walk(entry, out, MAX_WALK);
    } else if entry.is_file() {
        out.push(entry.to_path_buf());
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Plan {
    pub files: Vec<(PathBuf, u64)>,
    pub total: u64,
    pub learned: bool,
    pub skipped: usize,
}

/// The ordered, budgeted file list for an app.
pub fn plan(app_id: &str, budget: u64) -> Plan {
    let mut candidates: Vec<PathBuf> = Vec::new();
    let mut learned = false;
    if safe_id(app_id) {
        let l = read_list(&learned_path(app_id));
        learned = !l.is_empty();
        candidates.extend(l);
        for list in [
            PathBuf::from("/etc/fp6-app-ready/preload.d").join(format!("{app_id}.list")),
            config_dir().join("preload.d").join(format!("{app_id}.list")),
        ] {
            for entry in read_list(&list) {
                expand(&entry, &mut candidates);
            }
        }
        if !learned {
            if let Some(bin) = apps::find(app_id).and_then(|a| apps::exec_binary(&a.exec)) {
                candidates.push(bin.clone());
                for root in package_roots(&bin) {
                    walk(&root, &mut candidates, MAX_WALK);
                }
            }
        }
    }
    let mut seen = HashSet::new();
    let mut plan = Plan { learned, ..Plan::default() };
    for f in candidates {
        let Ok(canon) = std::fs::canonicalize(&f) else { continue };
        if !seen.insert(canon.clone()) {
            continue;
        }
        let Ok(meta) = std::fs::metadata(&canon) else { continue };
        if !meta.is_file() || meta.len() == 0 {
            continue;
        }
        if plan.total + meta.len() > budget {
            plan.skipped += 1;
            continue;
        }
        plan.total += meta.len();
        plan.files.push((canon, meta.len()));
    }
    plan
}

fn open_ro(path: &Path) -> Option<File> {
    std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_CLOEXEC | libc::O_NOATIME)
        .open(path)
        .or_else(|_| std::fs::OpenOptions::new().read(true).custom_flags(libc::O_CLOEXEC).open(path))
        .ok()
}

/// posix_fadvise(WILLNEED) over every planned file. Returns bytes requested.
pub fn warm(plan: &Plan) -> u64 {
    let mut bytes = 0;
    for (path, len) in &plan.files {
        let Some(f) = open_ro(path) else { continue };
        let rc = unsafe { libc::posix_fadvise(f.as_raw_fd(), 0, 0, libc::POSIX_FADV_WILLNEED) };
        if rc == 0 {
            bytes += len;
        }
    }
    bytes
}

fn page_size() -> u64 {
    let v = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
    if v > 0 {
        v as u64
    } else {
        4096
    }
}

/// Bytes of a file currently in the page cache (mincore over a mapping).
pub fn resident(path: &Path, len: u64) -> u64 {
    if len == 0 {
        return 0;
    }
    let Some(f) = open_ro(path) else { return 0 };
    let page = page_size();
    unsafe {
        let addr = libc::mmap(std::ptr::null_mut(), len as usize, libc::PROT_READ, libc::MAP_SHARED, f.as_raw_fd(), 0);
        if addr == libc::MAP_FAILED {
            return 0;
        }
        let pages = len.div_ceil(page) as usize;
        let mut vec = vec![0u8; pages];
        let rc = libc::mincore(addr, len as usize, vec.as_mut_ptr());
        libc::munmap(addr, len as usize);
        if rc != 0 {
            return 0;
        }
        (vec.iter().filter(|b| **b & 1 != 0).count() as u64 * page).min(len)
    }
}

pub fn resident_total(plan: &Plan) -> u64 {
    plan.files.iter().map(|(p, l)| resident(p, *l)).sum()
}

/// A locked mapping of one file (munlock + munmap on drop).
pub struct Locked {
    addr: *mut libc::c_void,
    len: usize,
}

// The mapping is only touched by the owning thread; raw pointers are not Send by default.
unsafe impl Send for Locked {}

impl Drop for Locked {
    fn drop(&mut self) {
        unsafe {
            libc::munlock(self.addr, self.len);
            libc::munmap(self.addr, self.len);
        }
    }
}

/// mlock the first files of the plan up to `budget` bytes.
pub fn lock_hot(plan: &Plan, budget: u64) -> (Vec<Locked>, u64) {
    let mut out = Vec::new();
    let mut used = 0u64;
    for (path, len) in &plan.files {
        if used + len > budget {
            continue;
        }
        let Some(f) = open_ro(path) else { continue };
        unsafe {
            let addr = libc::mmap(std::ptr::null_mut(), *len as usize, libc::PROT_READ, libc::MAP_SHARED, f.as_raw_fd(), 0);
            if addr == libc::MAP_FAILED {
                continue;
            }
            if libc::mlock(addr, *len as usize) != 0 {
                libc::munmap(addr, *len as usize);
                // RLIMIT_MEMLOCK reached: stop.
                break;
            }
            out.push(Locked { addr, len: *len as usize });
        }
        used += len;
    }
    (out, used)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrapper_targets() {
        let tmp = std::env::temp_dir().join(format!("fp6-app-ready-wrap-{}", std::process::id()));
        std::fs::create_dir_all(&tmp).unwrap();
        let w = tmp.join("w");
        std::fs::write(&w, "#!/bin/sh\nexec -a \"$0\" \"/nix/store/abc-morph-1.0/bin/.morph-wrapped\" \"$@\"\nPATH=/nix/store/def-qt-5/bin:$PATH\n").unwrap();
        assert_eq!(
            script_targets(&w),
            vec![PathBuf::from("/nix/store/abc-morph-1.0"), PathBuf::from("/nix/store/def-qt-5")]
        );
        std::fs::write(&w, [0x7fu8, b'E', b'L', b'F']).unwrap();
        assert!(script_targets(&w).is_empty());
        std::fs::remove_dir_all(tmp).unwrap();
    }

    #[test]
    fn store_roots() {
        assert_eq!(
            store_root(Path::new("/nix/store/abc-morph-1.0/bin/morph")),
            Some(PathBuf::from("/nix/store/abc-morph-1.0"))
        );
        assert_eq!(store_root(Path::new("/usr/bin/x")), None);
        assert_eq!(store_root(Path::new("/nix/store")), None);
    }

    #[test]
    fn learn_plan_warm_resident() {
        let tmp = std::env::temp_dir().join(format!("fp6-app-ready-test-{}", std::process::id()));
        std::fs::create_dir_all(&tmp).unwrap();
        std::env::set_var("XDG_CACHE_HOME", &tmp);
        std::env::set_var("XDG_CONFIG_HOME", &tmp);
        let a = tmp.join("a.bin");
        let b = tmp.join("b.bin");
        std::fs::write(&a, vec![1u8; 10000]).unwrap();
        std::fs::write(&b, vec![2u8; 20000]).unwrap();
        assert_eq!(learn("test-app", &[a.clone(), b.clone(), a.clone()]).unwrap(), 2);
        assert_eq!(learn("test-app", &[b.clone()]).unwrap(), 0);
        let p = plan("test-app", 15000);
        assert!(p.learned);
        assert_eq!(p.files.len(), 1);
        assert_eq!(p.skipped, 1);
        let p = plan("test-app", 1 << 30);
        assert_eq!(p.total, 30000);
        assert_eq!(warm(&p), 30000);
        // Just written, so cached.
        assert!(resident_total(&p) > 0);
        assert!(plan("../x", 1 << 30).files.is_empty());
        std::fs::remove_dir_all(tmp).unwrap();
    }
}
