// SPDX-License-Identifier: GPL-3.0-only
//! Who has a device node open, and what app a process belongs to.

use std::collections::BTreeSet;
use std::fs;
use std::os::unix::fs::{FileTypeExt, MetadataExt};
use std::path::{Path, PathBuf};

use crate::User;

/// Character-device numbers of the given nodes (missing nodes are skipped).
pub fn rdevs(nodes: &[PathBuf]) -> BTreeSet<u64> {
    nodes
        .iter()
        .filter_map(|n| fs::metadata(n).ok())
        .filter(|m| m.file_type().is_char_device())
        .map(|m| m.rdev())
        .collect()
}

/// PIDs (in /proc's namespace) that hold one of `devs` open.
pub fn holders(proc_root: &Path, devs: &BTreeSet<u64>) -> BTreeSet<u32> {
    let mut out = BTreeSet::new();
    let Ok(entries) = fs::read_dir(proc_root) else { return out };
    for entry in entries.flatten() {
        let Some(pid) = entry.file_name().to_str().and_then(|s| s.parse::<u32>().ok()) else { continue };
        let Ok(fds) = fs::read_dir(entry.path().join("fd")) else { continue };
        for fd in fds.flatten() {
            // Cheap filter first: only links that point into a /dev.
            let Ok(target) = fs::read_link(fd.path()) else { continue };
            if !target.to_string_lossy().contains("/dev/") {
                continue;
            }
            if let Ok(m) = fs::metadata(fd.path()) {
                if m.file_type().is_char_device() && devs.contains(&m.rdev()) {
                    out.insert(pid);
                    break;
                }
            }
        }
    }
    out
}

fn read_lossy(path: &Path) -> Option<String> {
    fs::read(path).ok().map(|b| String::from_utf8_lossy(&b).into_owned())
}

/// APP_ID from the process environment (set by lomiri-app-launch).
pub fn app_id_of(proc_root: &Path, pid: u32) -> Option<String> {
    let env = fs::read(proc_root.join(pid.to_string()).join("environ")).ok()?;
    env.split(|b| *b == 0).find_map(|kv| {
        kv.strip_prefix(b"APP_ID=")
            .map(|v| String::from_utf8_lossy(v).into_owned())
            .filter(|v| !v.is_empty())
    })
}

pub fn comm_of(proc_root: &Path, pid: u32) -> String {
    read_lossy(&proc_root.join(pid.to_string()).join("comm"))
        .map(|s| s.trim().to_owned())
        .unwrap_or_default()
}

/// Inside the Waydroid (LXC) container?
pub fn in_waydroid(proc_root: &Path, pid: u32) -> bool {
    read_lossy(&proc_root.join(pid.to_string()).join("cgroup"))
        .map(|c| c.contains("waydroid") || c.contains("lxc.payload"))
        .unwrap_or(false)
}

/// "Name=" of `<id>.desktop` in the XDG data dirs (Lomiri app ids may carry
/// a click version suffix: "pkg_app_1.2" -> also try "pkg_app").
pub fn desktop_name(id: &str) -> Option<String> {
    let mut candidates = vec![id.to_owned()];
    let parts: Vec<&str> = id.split('_').collect();
    if parts.len() == 3 {
        candidates.push(format!("{}_{}", parts[0], parts[1]));
    }
    let dirs = std::env::var("XDG_DATA_DIRS").unwrap_or_else(|_| "/run/current-system/sw/share".into());
    let home = std::env::var("XDG_DATA_HOME")
        .ok()
        .or_else(|| std::env::var("HOME").ok().map(|h| format!("{h}/.local/share")));
    for dir in home.into_iter().chain(dirs.split(':').map(str::to_owned)) {
        for c in &candidates {
            let path = Path::new(&dir).join("applications").join(format!("{c}.desktop"));
            if let Some(text) = read_lossy(&path) {
                if let Some(name) = desktop_entry_name(&text) {
                    return Some(name);
                }
            }
        }
    }
    None
}

/// The untranslated Name= of the [Desktop Entry] group.
pub fn desktop_entry_name(text: &str) -> Option<String> {
    let mut in_entry = false;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_entry = line == "[Desktop Entry]";
            continue;
        }
        if in_entry {
            if let Some(v) = line.strip_prefix("Name=") {
                return Some(v.trim().to_owned());
            }
        }
    }
    None
}

/// Human name for a process: its app's desktop name, else the binary.
pub fn user_of(proc_root: &Path, pid: u32) -> User {
    if in_waydroid(proc_root, pid) {
        return User { name: "Android app (Waydroid)".into(), id: comm_of(proc_root, pid), pid };
    }
    let comm = comm_of(proc_root, pid);
    if let Some(app) = app_id_of(proc_root, pid) {
        let name = desktop_name(&app).unwrap_or_else(|| app.clone());
        return User { name, id: app, pid };
    }
    let exe = fs::read_link(proc_root.join(pid.to_string()).join("exe"))
        .ok()
        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
        .unwrap_or_else(|| comm.clone());
    let bin = exe.trim_start_matches('.').trim_end_matches("-wrapped").to_owned();
    let name = desktop_name(&bin).unwrap_or_else(|| bin.clone());
    User { name, id: bin, pid }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desktop_name_parsing() {
        let t = "[Desktop Entry]\nType=Application\nName[ro]=Cameră\nName=Camera\n[Desktop Action x]\nName=Other\n";
        assert_eq!(desktop_entry_name(t).as_deref(), Some("Camera"));
        assert_eq!(desktop_entry_name("[Other]\nName=x\n"), None);
    }

    #[test]
    fn holders_of_dev_null() {
        // This test process holds /dev/null open via the file below.
        let f = fs::File::open("/dev/null").unwrap();
        let devs = rdevs(&[PathBuf::from("/dev/null")]);
        let me = std::process::id();
        assert!(holders(Path::new("/proc"), &devs).contains(&me));
        drop(f);
    }
}
