// SPDX-License-Identifier: GPL-3.0-only
//! Local notification history (JSON lines, 0600) and the list of app names
//! seen (for the "apps allowed during Do Not Disturb" picker).

use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub const MAX_ENTRIES: usize = 5000;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    /// Unix time (s).
    pub t: i64,
    /// Notify() app_name (or the sender's process name when empty).
    pub app: String,
    pub summary: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub icon: String,
    /// "call" for incoming-call snap decisions, "dnd" if DND was on, etc.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
}

#[derive(Clone, Debug, Default)]
pub struct HistoryConfig {
    pub enabled: bool,
    pub body: bool,
    pub retention_days: i64,
    pub exclude: Vec<String>,
}

pub struct History {
    path: PathBuf,
    apps_path: PathBuf,
    pub entries: Vec<Entry>,
    /// app name -> last seen (Unix time).
    pub apps: BTreeMap<String, i64>,
    pub cfg: HistoryConfig,
}

fn private_dir(p: &Path) {
    if let Some(d) = p.parent() {
        let _ = fs::create_dir_all(d);
        let _ = fs::set_permissions(d, fs::Permissions::from_mode(0o700));
    }
}

pub(crate) fn write_private(path: &Path, data: &[u8]) -> std::io::Result<()> {
    private_dir(path);
    let tmp = path.with_extension("tmp");
    {
        let mut f = fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(&tmp)?;
        f.write_all(data)?;
        f.sync_all()?;
    }
    fs::rename(&tmp, path)
}

impl History {
    pub fn open(path: PathBuf, apps_path: PathBuf) -> History {
        let entries = fs::read_to_string(&path)
            .map(|s| s.lines().filter_map(|l| serde_json::from_str(l).ok()).collect())
            .unwrap_or_default();
        let apps = fs::read_to_string(&apps_path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        History { path, apps_path, entries, apps, cfg: HistoryConfig::default() }
    }

    fn save(&self) {
        let mut out = String::new();
        for e in &self.entries {
            if let Ok(l) = serde_json::to_string(e) {
                out.push_str(&l);
                out.push('\n');
            }
        }
        if self.entries.is_empty() {
            let _ = fs::remove_file(&self.path);
        } else if let Err(e) = write_private(&self.path, out.as_bytes()) {
            eprintln!("fp6-notifyd: history save: {e}");
        }
    }

    fn save_apps(&self) {
        if let Ok(s) = serde_json::to_string(&self.apps) {
            let _ = write_private(&self.apps_path, s.as_bytes());
        }
    }

    /// Applies a new config: disabling deletes the history, turning bodies off strips them.
    pub fn configure(&mut self, cfg: HistoryConfig, now: i64) {
        self.cfg = cfg;
        if !self.cfg.enabled {
            if !self.entries.is_empty() {
                self.entries.clear();
                self.save();
            }
            return;
        }
        let mut changed = false;
        if !self.cfg.body {
            for e in self.entries.iter_mut() {
                if e.body.take().is_some() {
                    changed = true;
                }
            }
        }
        let before = self.entries.len();
        let ex = self.cfg.exclude.clone();
        self.entries.retain(|e| !ex.contains(&e.app));
        changed |= before != self.entries.len();
        changed |= self.prune(now);
        if changed {
            self.save();
        }
    }

    /// Drops entries older than the retention; true if something was dropped.
    pub fn prune(&mut self, now: i64) -> bool {
        let days = self.cfg.retention_days.clamp(1, 365);
        let cutoff = now - days * 86400;
        let before = self.entries.len();
        self.entries.retain(|e| e.t >= cutoff);
        if self.entries.len() > MAX_ENTRIES {
            let n = self.entries.len() - MAX_ENTRIES;
            self.entries.drain(..n);
        }
        before != self.entries.len()
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.save();
    }

    /// Notes the app (always; names only) and records the entry if history is on.
    /// Returns true if the stored history changed.
    pub fn record(&mut self, mut e: Entry) -> bool {
        if !e.app.is_empty() {
            let new = self.apps.insert(e.app.clone(), e.t).is_none();
            if new {
                self.save_apps();
            }
        }
        if !self.cfg.enabled || self.cfg.exclude.contains(&e.app) {
            return false;
        }
        if !self.cfg.body {
            e.body = None;
        }
        // Updates of the same notification (same app + summary within 2 s) replace it.
        if let Some(last) = self.entries.last_mut() {
            if last.app == e.app && last.summary == e.summary && (e.t - last.t).abs() <= 2 {
                *last = e;
                self.save();
                return true;
            }
        }
        let now = e.t;
        self.entries.push(e);
        if self.prune(now) {
            self.save();
            return true;
        }
        // Append-only write for the common case.
        private_dir(&self.path);
        let line = serde_json::to_string(self.entries.last().unwrap()).unwrap_or_default();
        let ok = fs::OpenOptions::new()
            .append(true)
            .create(true)
            .mode(0o600)
            .open(&self.path)
            .and_then(|mut f| writeln!(f, "{line}"));
        if ok.is_err() {
            self.save();
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("fp6-notifyd-test-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        d
    }

    fn entry(t: i64, app: &str, s: &str) -> Entry {
        Entry { t, app: app.into(), summary: s.into(), body: Some("secret".into()), icon: String::new(), tags: vec![] }
    }

    #[test]
    fn off_by_default_records_only_app_names() {
        let d = tmp("off");
        let mut h = History::open(d.join("h.jsonl"), d.join("apps.json"));
        assert!(!h.record(entry(100, "a", "x")));
        assert!(h.entries.is_empty());
        assert!(h.apps.contains_key("a"));
        assert!(!d.join("h.jsonl").exists());
    }

    #[test]
    fn body_retention_clear_and_reload() {
        let d = tmp("on");
        let p = d.join("h.jsonl");
        let mut h = History::open(p.clone(), d.join("apps.json"));
        h.configure(HistoryConfig { enabled: true, body: false, retention_days: 1, exclude: vec!["hidden".into()] }, 1000);
        assert!(h.record(entry(1000, "a", "one")));
        assert!(!h.record(entry(1001, "hidden", "x")));
        assert_eq!(h.entries[0].body, None);
        assert!(h.record(entry(1001, "a", "one"))); // update replaces
        assert_eq!(h.entries.len(), 1);
        assert!(h.record(entry(1000 + 86400 + 10, "b", "two"))); // first one expires
        assert_eq!(h.entries.len(), 1);
        let mode = fs::metadata(&p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        let h2 = History::open(p.clone(), d.join("apps.json"));
        assert_eq!(h2.entries, h.entries);
        h.clear();
        assert!(!p.exists());
        // Disabling deletes everything.
        h.configure(HistoryConfig { enabled: true, body: true, retention_days: 7, exclude: vec![] }, 2000);
        h.record(entry(2000, "c", "three"));
        assert_eq!(h.entries[0].body.as_deref(), Some("secret"));
        h.configure(HistoryConfig { enabled: false, ..HistoryConfig::default() }, 2001);
        assert!(h.entries.is_empty() && !p.exists());
    }
}
