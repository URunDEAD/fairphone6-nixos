// SPDX-License-Identifier: MIT
//! Running Lomiri apps: lomiri-app-launch starts every app as a systemd user
//! unit `lomiri-app-launch--application-<type>--<appid>--<instance>.service`
//! in the user manager's app.slice. Its cgroup gives the PIDs and memory use;
//! /proc/<pid>/oom_score_adj is writable by the user for values above the
//! inherited minimum (apps start at 0).

use std::path::{Path, PathBuf};

const UNIT_PREFIX: &str = "lomiri-app-launch--application-";

#[derive(Clone, Debug, PartialEq)]
pub struct AppUnit {
    pub app_id: String,
    pub unit: String,
    pub cgroup: PathBuf,
}

/// app.slice of this user's systemd manager, from our own cgroup.
pub fn app_slice() -> Option<PathBuf> {
    let text = std::fs::read_to_string("/proc/self/cgroup").ok()?;
    let rel = text.lines().find_map(|l| l.strip_prefix("0::"))?;
    let mut path = PathBuf::from("/sys/fs/cgroup").join(rel.trim_start_matches('/'));
    loop {
        if path.file_name().is_some_and(|n| n == "app.slice") {
            return Some(path);
        }
        if path.file_name().is_some_and(|n| n.to_string_lossy().starts_with("user@")) {
            return Some(path.join("app.slice"));
        }
        if !path.pop() || path == Path::new("/sys/fs/cgroup") {
            break;
        }
    }
    let uid = unsafe { libc::getuid() };
    Some(PathBuf::from(format!(
        "/sys/fs/cgroup/user.slice/user-{uid}.slice/user@{uid}.service/app.slice"
    )))
}

/// systemd unit-name unescaping (\xNN).
fn unescape(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\\' && i + 3 < bytes.len() && bytes[i + 1] == b'x' {
            if let Ok(v) = u8::from_str_radix(&s[i + 2..i + 4], 16) {
                out.push(v);
                i += 4;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// App id of a lomiri-app-launch unit name.
pub fn parse_unit(name: &str) -> Option<String> {
    let rest = name.strip_prefix(UNIT_PREFIX)?.strip_suffix(".service")?;
    let (_kind, rest) = rest.split_once("--")?;
    let (app, _instance) = rest.rsplit_once("--")?;
    if app.is_empty() {
        return None;
    }
    let app = unescape(app);
    // Click/snap ids are package_app_version; Lomiri compares the short id.
    Some(app)
}

pub fn scan(slice: &Path) -> Vec<AppUnit> {
    let Ok(entries) = std::fs::read_dir(slice) else { return Vec::new() };
    let mut apps: Vec<AppUnit> = entries
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let app_id = parse_unit(&name)?;
            Some(AppUnit { app_id, unit: name, cgroup: e.path() })
        })
        .collect();
    apps.sort_by(|a, b| a.app_id.cmp(&b.app_id));
    apps
}

/// All PIDs in a cgroup subtree.
pub fn pids(cgroup: &Path) -> Vec<i32> {
    let mut out = Vec::new();
    collect_pids(cgroup, &mut out, 0);
    out
}

fn collect_pids(dir: &Path, out: &mut Vec<i32>, depth: u32) {
    if let Ok(text) = std::fs::read_to_string(dir.join("cgroup.procs")) {
        out.extend(text.lines().filter_map(|l| l.trim().parse::<i32>().ok()));
    }
    if depth > 4 {
        return;
    }
    if let Ok(entries) = std::fs::read_dir(dir) {
        for e in entries.flatten() {
            if e.file_type().is_ok_and(|t| t.is_dir()) {
                collect_pids(&e.path(), out, depth + 1);
            }
        }
    }
}

pub fn read_u64(path: &Path) -> u64 {
    std::fs::read_to_string(path).ok().and_then(|s| s.trim().parse().ok()).unwrap_or(0)
}

/// Resident + swapped memory of the app's cgroup (bytes).
pub fn memory(cgroup: &Path) -> (u64, u64) {
    (read_u64(&cgroup.join("memory.current")), read_u64(&cgroup.join("memory.swap.current")))
}

/// Process state letter from /proc/<pid>/stat ('T' = stopped by SIGSTOP).
pub fn state(pid: i32) -> Option<char> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let after = stat.rsplit_once(')')?.1;
    after.trim_start().chars().next()
}

/// True when every process of the app is stopped (frozen).
pub fn all_stopped(pids: &[i32]) -> bool {
    !pids.is_empty() && pids.iter().all(|p| matches!(state(*p), Some('T') | Some('t')))
}

pub fn oom_adj(pid: i32) -> Option<i32> {
    std::fs::read_to_string(format!("/proc/{pid}/oom_score_adj")).ok()?.trim().parse().ok()
}

/// Write oom_score_adj where it differs. Returns how many were changed.
pub fn set_oom_adj(pids: &[i32], value: i32) -> usize {
    let mut changed = 0;
    for &pid in pids {
        if oom_adj(pid) == Some(value) {
            continue;
        }
        if std::fs::write(format!("/proc/{pid}/oom_score_adj"), value.to_string()).is_ok() {
            changed += 1;
        }
    }
    changed
}

/// SIGKILL the whole app cgroup (cgroup.kill, Linux 5.14+); signal fallback.
pub fn kill(cgroup: &Path) -> bool {
    if std::fs::write(cgroup.join("cgroup.kill"), "1").is_ok() {
        return true;
    }
    let list = pids(cgroup);
    for &pid in &list {
        unsafe {
            libc::kill(pid, libc::SIGKILL);
        }
    }
    !list.is_empty()
}

/// Paths of files mapped by a process (shared libraries, mmapped data, QML caches).
pub fn mapped_files(pid: i32) -> Vec<PathBuf> {
    let Ok(text) = std::fs::read_to_string(format!("/proc/{pid}/maps")) else { return Vec::new() };
    let mut out: Vec<PathBuf> = Vec::new();
    for line in text.lines() {
        // address perms offset dev inode pathname
        let mut it = line.splitn(6, char::is_whitespace);
        let inode = it.nth(4).unwrap_or("0");
        let path = it.next().unwrap_or("").trim();
        if inode == "0" || !path.starts_with('/') || path.ends_with("(deleted)") {
            continue;
        }
        if ["/dev/", "/proc/", "/sys/", "/memfd:", "/run/user/", "/tmp/", "/dmabuf"].iter().any(|p| path.starts_with(p)) {
            continue;
        }
        let p = PathBuf::from(path);
        if !out.contains(&p) {
            out.push(p);
        }
    }
    out
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct MemInfo {
    pub total: u64,
    pub available: u64,
    pub swap_total: u64,
    pub swap_free: u64,
}

pub fn parse_meminfo(text: &str) -> MemInfo {
    let mut m = MemInfo::default();
    for line in text.lines() {
        let mut it = line.split_whitespace();
        let (Some(key), Some(value)) = (it.next(), it.next()) else { continue };
        let kb: u64 = value.parse().unwrap_or(0);
        match key {
            "MemTotal:" => m.total = kb * 1024,
            "MemAvailable:" => m.available = kb * 1024,
            "SwapTotal:" => m.swap_total = kb * 1024,
            "SwapFree:" => m.swap_free = kb * 1024,
            _ => {}
        }
    }
    m
}

pub fn meminfo() -> MemInfo {
    parse_meminfo(&std::fs::read_to_string("/proc/meminfo").unwrap_or_default())
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Zram {
    pub active: bool,
    pub disksize: u64,
    pub orig: u64,
    pub compr: u64,
    pub used: u64,
}

pub fn zram() -> Zram {
    let disksize = read_u64(Path::new("/sys/block/zram0/disksize"));
    let active = std::fs::read_to_string("/proc/swaps").is_ok_and(|s| s.lines().any(|l| l.starts_with("/dev/zram0")));
    let mm = std::fs::read_to_string("/sys/block/zram0/mm_stat").unwrap_or_default();
    let v: Vec<u64> = mm.split_whitespace().filter_map(|x| x.parse().ok()).collect();
    Zram {
        active,
        disksize,
        orig: v.first().copied().unwrap_or(0),
        compr: v.get(1).copied().unwrap_or(0),
        used: v.get(2).copied().unwrap_or(0),
    }
}

/// avg10 of the "some" and "full" lines of a PSI file.
pub fn psi_avg10(text: &str) -> (f64, f64) {
    let get = |kind: &str| {
        text.lines()
            .find(|l| l.starts_with(kind))
            .and_then(|l| l.split_whitespace().find_map(|f| f.strip_prefix("avg10=")))
            .and_then(|v| v.parse().ok())
            .unwrap_or(0.0)
    };
    (get("some"), get("full"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unit_names() {
        assert_eq!(
            parse_unit("lomiri-app-launch--application-legacy--lomiri-terminal-app--.service").as_deref(),
            Some("lomiri-terminal-app")
        );
        assert_eq!(
            parse_unit("lomiri-app-launch--application-legacy--morph-browser--12345.service").as_deref(),
            Some("morph-browser")
        );
        assert_eq!(
            parse_unit("lomiri-app-launch--application-legacy--de.haeckerfelix.Fragments--.service").as_deref(),
            Some("de.haeckerfelix.Fragments")
        );
        assert_eq!(
            parse_unit("lomiri-app-launch--application-click--com.ubuntu.x_x_1.0--.service").as_deref(),
            Some("com.ubuntu.x_x_1.0")
        );
        assert_eq!(parse_unit("lomiri-app-launch--application-legacy--a\\x2db--.service").as_deref(), Some("a-b"));
        assert_eq!(parse_unit("dconf.service"), None);
        assert_eq!(parse_unit("lomiri-app-launch--application-legacy----.service"), None);
    }

    #[test]
    fn meminfo_and_psi() {
        let m = parse_meminfo("MemTotal:  7475200 kB\nMemFree: 1 kB\nMemAvailable: 5000000 kB\nSwapTotal: 0 kB\nSwapFree: 0 kB\n");
        assert_eq!(m.total, 7475200 * 1024);
        assert_eq!(m.available, 5000000 * 1024);
        let (s, f) = psi_avg10("some avg10=1.50 avg60=0.00 avg300=0.00 total=10\nfull avg10=0.25 avg60=0.00 avg300=0.00 total=3\n");
        assert_eq!((s, f), (1.5, 0.25));
    }

    #[test]
    fn own_maps() {
        let files = mapped_files(std::process::id() as i32);
        assert!(!files.is_empty());
        assert!(files.iter().all(|f| f.is_absolute()));
    }
}
