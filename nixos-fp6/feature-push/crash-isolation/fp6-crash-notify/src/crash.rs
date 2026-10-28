// SPDX-License-Identifier: MIT
//! Pure logic of fp6-crash-notify: which unit died how, which app it was,
//! how the app is called, and how often we may say so.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// systemd's bus object path for units.
pub const UNIT_PATH_PREFIX: &str = "/org/freedesktop/systemd1/unit/";

/// Unit name from a systemd unit object path (`_2d` -> `-`, ...).
pub fn unit_from_path(path: &str) -> Option<String> {
    let escaped = path.strip_prefix(UNIT_PATH_PREFIX)?;
    let bytes = escaped.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'_' {
            let hex = std::str::from_utf8(bytes.get(i + 1..i + 3)?).ok()?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// What died.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Subject {
    /// A lomiri-app-launch app: `<prefix><type>--<appid>--<instance>.service`.
    App { app_id: String, kind: String },
    /// The shell (lomiri.service).
    Shell,
}

pub fn classify(unit: &str, app_prefix: &str, shell_unit: &str) -> Option<Subject> {
    if unit == shell_unit {
        return Some(Subject::Shell);
    }
    let rest = unit.strip_prefix(app_prefix)?.strip_suffix(".service")?;
    let mut parts = rest.splitn(2, "--");
    let kind = parts.next()?;
    let rest = parts.next()?;
    // The instance (after the last "--") may be empty; the app id is between.
    let (app_id, _instance) = rest.rsplit_once("--")?;
    if kind.is_empty() || app_id.is_empty() {
        return None;
    }
    Some(Subject::App { app_id: app_id.to_string(), kind: kind.to_string() })
}

/// si_code values systemd reports as ExecMainCode.
pub const CLD_EXITED: i32 = 1;
pub const CLD_KILLED: i32 = 2;
pub const CLD_DUMPED: i32 = 3;

/// Signals that mean "the program crashed". SIGKILL / SIGTERM / SIGINT /
/// SIGHUP are deliberate stops (user, app-ready's low-memory killer, the
/// kernel OOM killer, session end) and never produce a notification.
pub const CRASH_SIGNALS: [i32; 7] = [
    4,  // SIGILL
    5,  // SIGTRAP
    6,  // SIGABRT
    7,  // SIGBUS
    8,  // SIGFPE
    11, // SIGSEGV
    31, // SIGSYS
];

/// The crash signal, if the service's main process died of one.
/// `result` is the Service Result property, `code`/`status` ExecMainCode /
/// ExecMainStatus.
pub fn crash_signal(result: &str, code: Option<i32>, status: Option<i32>) -> Option<i32> {
    let crash = |s: i32| CRASH_SIGNALS.contains(&s).then_some(s);
    match (code, status) {
        (Some(CLD_KILLED) | Some(CLD_DUMPED), Some(s)) => crash(s),
        (Some(CLD_EXITED), _) => None,
        // No exec status in the signal: "core-dump" is only ever set for a
        // dumping signal; "signal" alone could be a SIGKILL.
        _ if result == "core-dump" => Some(6),
        _ => None,
    }
}

pub fn signal_name(sig: i32) -> &'static str {
    match sig {
        4 => "SIGILL",
        5 => "SIGTRAP",
        6 => "SIGABRT",
        7 => "SIGBUS",
        8 => "SIGFPE",
        11 => "SIGSEGV",
        31 => "SIGSYS",
        _ => "signal",
    }
}

/// Directories searched for `applications/<app_id>.desktop`, most specific first.
pub fn data_dirs(xdg_data_home: Option<&str>, home: Option<&str>, xdg_data_dirs: Option<&str>, user: Option<&str>) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    match (xdg_data_home, home) {
        (Some(d), _) if !d.is_empty() => dirs.push(PathBuf::from(d)),
        (_, Some(h)) => dirs.push(Path::new(h).join(".local/share")),
        _ => {}
    }
    for d in xdg_data_dirs.unwrap_or("").split(':').filter(|d| !d.is_empty()) {
        dirs.push(PathBuf::from(d));
    }
    if let Some(u) = user {
        dirs.push(PathBuf::from(format!("/etc/profiles/per-user/{u}/share")));
    }
    if let Some(h) = home {
        dirs.push(Path::new(h).join(".nix-profile/share"));
    }
    dirs.push(PathBuf::from("/run/current-system/sw/share"));
    let mut seen = std::collections::HashSet::new();
    dirs.retain(|d| seen.insert(d.clone()));
    dirs
}

/// Name= and Icon= of the [Desktop Entry] group.
pub fn parse_desktop_entry(text: &str) -> (Option<String>, Option<String>) {
    let (mut name, mut icon) = (None, None);
    let mut in_entry = false;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_entry = line == "[Desktop Entry]";
            continue;
        }
        if !in_entry {
            continue;
        }
        if let Some(v) = line.strip_prefix("Name=") {
            name.get_or_insert_with(|| v.trim().to_string());
        } else if let Some(v) = line.strip_prefix("Icon=") {
            icon.get_or_insert_with(|| v.trim().to_string());
        }
    }
    (name.filter(|n| !n.is_empty()), icon.filter(|i| !i.is_empty()))
}

/// Display name and icon of an app (the app id if no desktop file is found).
pub fn app_label(app_id: &str, dirs: &[PathBuf]) -> (String, String) {
    // An app id must not reach outside applications/.
    if !app_id.contains('/') {
        for d in dirs {
            let file = d.join("applications").join(format!("{app_id}.desktop"));
            if let Ok(text) = std::fs::read_to_string(&file) {
                let (name, icon) = parse_desktop_entry(&text);
                return (name.unwrap_or_else(|| app_id.to_string()), icon.unwrap_or_default());
            }
        }
    }
    (app_id.to_string(), String::new())
}

/// One notification per subject per `per_key`, at most `burst` per `window`.
pub struct RateLimit {
    per_key: Duration,
    window: Duration,
    burst: usize,
    last: HashMap<String, Instant>,
    recent: Vec<Instant>,
}

impl RateLimit {
    pub fn new(per_key: Duration, window: Duration, burst: usize) -> Self {
        RateLimit { per_key, window, burst, last: HashMap::new(), recent: Vec::new() }
    }

    pub fn allow(&mut self, key: &str, now: Instant) -> bool {
        if let Some(t) = self.last.get(key) {
            if now.duration_since(*t) < self.per_key {
                return false;
            }
        }
        self.recent.retain(|t| now.duration_since(*t) < self.window);
        if self.recent.len() >= self.burst {
            return false;
        }
        self.recent.push(now);
        self.last.insert(key.to_string(), now);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_unit_paths() {
        assert_eq!(
            unit_from_path("/org/freedesktop/systemd1/unit/lomiri_2dapp_2dlaunch_2d_2dapplication_2dlegacy_2d_2dmorph_2dbrowser_2d_2d_2eservice").as_deref(),
            Some("lomiri-app-launch--application-legacy--morph-browser--.service")
        );
        assert_eq!(unit_from_path("/org/freedesktop/systemd1/unit/lomiri_2eservice").as_deref(), Some("lomiri.service"));
        assert_eq!(unit_from_path("/org/freedesktop/systemd1/job/12"), None);
        assert_eq!(unit_from_path("/org/freedesktop/systemd1/unit/bad_2"), None);
    }

    #[test]
    fn classifies_units() {
        let p = "lomiri-app-launch--";
        assert_eq!(classify("lomiri.service", p, "lomiri.service"), Some(Subject::Shell));
        assert_eq!(
            classify("lomiri-app-launch--application-legacy--morph-browser--.service", p, "lomiri.service"),
            Some(Subject::App { app_id: "morph-browser".into(), kind: "application-legacy".into() })
        );
        assert_eq!(
            classify("lomiri-app-launch--application-click--com.example.app_app_1.0--1234.service", p, "lomiri.service"),
            Some(Subject::App { app_id: "com.example.app_app_1.0".into(), kind: "application-click".into() })
        );
        assert_eq!(classify("lomiri-keyboard.service", p, "lomiri.service"), None);
        assert_eq!(classify("lomiri-app-launch--x.service", p, "lomiri.service"), None);
        assert_eq!(classify("lomiri-app-launch--application-legacy----.service", p, "lomiri.service"), None);
    }

    #[test]
    fn only_crash_signals_count() {
        assert_eq!(crash_signal("core-dump", Some(CLD_DUMPED), Some(11)), Some(11));
        assert_eq!(crash_signal("signal", Some(CLD_KILLED), Some(6)), Some(6));
        // app-ready's low-memory killer, the kernel OOM killer, a user close.
        assert_eq!(crash_signal("signal", Some(CLD_KILLED), Some(9)), None);
        assert_eq!(crash_signal("oom-kill", Some(CLD_KILLED), Some(9)), None);
        assert_eq!(crash_signal("signal", Some(CLD_KILLED), Some(15)), None);
        // Compositor gone: apps exit with a status.
        assert_eq!(crash_signal("exit-code", Some(CLD_EXITED), Some(255)), None);
        assert_eq!(crash_signal("success", None, None), None);
        assert_eq!(crash_signal("core-dump", None, None), Some(6));
        assert_eq!(crash_signal("signal", None, None), None);
    }

    #[test]
    fn reads_desktop_entries() {
        let text = "[Desktop Entry]\nType=Application\nName=Browser\nName[de]=Browser DE\nIcon=morph-browser\n[Desktop Action New]\nName=New window\n";
        assert_eq!(parse_desktop_entry(text), (Some("Browser".into()), Some("morph-browser".into())));
        assert_eq!(parse_desktop_entry("[Other]\nName=x\n"), (None, None));

        let dir = std::env::temp_dir().join(format!("fp6-crash-notify-test-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("applications")).unwrap();
        std::fs::write(dir.join("applications/mock.desktop"), text).unwrap();
        let dirs = vec![PathBuf::from("/nonexistent"), dir.clone()];
        assert_eq!(app_label("mock", &dirs), ("Browser".to_string(), "morph-browser".to_string()));
        assert_eq!(app_label("missing", &dirs), ("missing".to_string(), String::new()));
        assert_eq!(app_label("../applications/mock", &dirs).0, "../applications/mock");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn data_dir_order() {
        let d = data_dirs(None, Some("/home/u"), Some("/a:/b::/a"), Some("u"));
        assert_eq!(d[0], PathBuf::from("/home/u/.local/share"));
        assert_eq!(d[1], PathBuf::from("/a"));
        assert_eq!(d[2], PathBuf::from("/b"));
        assert!(d.contains(&PathBuf::from("/run/current-system/sw/share")));
        assert_eq!(d.iter().filter(|p| **p == PathBuf::from("/a")).count(), 1);
    }

    #[test]
    fn rate_limits() {
        let mut r = RateLimit::new(Duration::from_secs(30), Duration::from_secs(60), 3);
        let t = Instant::now();
        assert!(r.allow("a", t));
        assert!(!r.allow("a", t + Duration::from_secs(10)));
        assert!(r.allow("b", t + Duration::from_secs(1)));
        assert!(r.allow("c", t + Duration::from_secs(2)));
        assert!(!r.allow("d", t + Duration::from_secs(3)));
        assert!(r.allow("a", t + Duration::from_secs(61)));
    }
}
