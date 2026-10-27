//! Persistent daemon state (/var/lib/fp6-updated/state.json), shared by the
//! D-Bus service and the boot-time health check; every read-modify-write
//! holds an exclusive flock on state.lock.

use crate::health::{Pending, Report};
use crate::manifest::{Closure, Manifest};
use crate::policy::Settings;
use crate::util;
use serde::{Deserialize, Serialize};
use std::fs;
use std::os::unix::io::AsRawFd;
use std::path::{Path, PathBuf};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct LastCheck {
    pub time: i64,
    pub ok: bool,
    pub message: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct Available {
    pub manifest: Manifest,
    pub manifest_url: String,
    pub cache_url: String,
    /// Paths of the closure not in the store at check time, and their sizes.
    pub missing_paths: u64,
    pub download_bytes: u64,
    pub unpacked_bytes: u64,
    pub downloaded: bool,
    #[serde(default)]
    pub closure: Closure,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct InstallRecord {
    pub time: i64,
    /// "ab" | "single" | "rollback-ab" | "rollback-single"
    pub mode: String,
    pub partition: String,
    pub release: u64,
    pub version: String,
    pub toplevel: String,
    pub previous_toplevel: String,
    #[serde(default)]
    pub previous_version: String,
    pub backup: Option<String>,
    pub backup_sha256: Option<String>,
    pub trimmed_backup: Option<String>,
    pub trimmed_sha256: Option<String>,
    /// "installed" (awaiting reboot) | "confirmed" | "failed" | "rolled-back"
    pub outcome: String,
    #[serde(default)]
    pub note: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct State {
    pub settings: Settings,
    /// Runtime override of the configured server (set by the coordinator).
    pub server: Option<String>,
    pub last_check: Option<LastCheck>,
    pub available: Option<Available>,
    pub install_tonight: bool,
    pub pending: Option<Pending>,
    pub history: Vec<InstallRecord>,
    /// Health of the last boot that passed without a pending update.
    pub baseline: Option<Report>,
    pub last_health: Option<Report>,
}

pub struct Store {
    dir: PathBuf,
}

pub struct Guard {
    _f: fs::File,
}

impl Store {
    pub fn new(dir: &Path) -> Store {
        Store { dir: dir.to_path_buf() }
    }

    fn path(&self) -> PathBuf {
        self.dir.join("state.json")
    }

    pub fn lock(&self) -> Result<Guard, String> {
        fs::create_dir_all(&self.dir).map_err(|e| format!("{}: {e}", self.dir.display()))?;
        let f = fs::OpenOptions::new().create(true).truncate(false).write(true).open(self.dir.join("state.lock"))
            .map_err(|e| format!("state lock: {e}"))?;
        if unsafe { libc::flock(f.as_raw_fd(), libc::LOCK_EX) } != 0 {
            return Err("state lock: flock failed".into());
        }
        Ok(Guard { _f: f })
    }

    /// Loads (a missing file is the default state; a corrupt one is kept
    /// aside and replaced by the default).
    pub fn load(&self) -> State {
        match fs::read(self.path()) {
            Ok(b) => serde_json::from_slice(&b).unwrap_or_else(|e| {
                util::log(format!("fp6-updated: state.json unreadable ({e}); keeping a copy and starting fresh"));
                let _ = fs::rename(self.path(), self.dir.join(format!("state.json.corrupt-{}", util::now())));
                State::default()
            }),
            Err(_) => State::default(),
        }
    }

    pub fn save(&self, s: &State) -> Result<(), String> {
        util::write_atomic(&self.path(), serde_json::to_string_pretty(s).unwrap().as_bytes(), 0o600)
    }

    pub fn update<T>(&self, f: impl FnOnce(&mut State) -> T) -> Result<T, String> {
        let _g = self.lock()?;
        let mut s = self.load();
        let r = f(&mut s);
        self.save(&s)?;
        Ok(r)
    }

    pub fn read(&self) -> State {
        let _g = self.lock().ok();
        self.load()
    }
}

pub fn push_history(s: &mut State, r: InstallRecord) {
    s.history.insert(0, r);
    s.history.truncate(10);
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn roundtrip_and_corruption() {
        let d = std::env::temp_dir().join(format!("fp6-state-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        let st = Store::new(&d);
        assert_eq!(st.read(), State::default());
        assert!(st.read().settings.auto_check && !st.read().settings.auto_download);
        st.update(|s| { s.install_tonight = true; s.server = Some("http://100.1.2.3:8742".into()) }).unwrap();
        let s = st.read();
        assert!(s.install_tonight);
        fs::write(d.join("state.json"), b"{not json").unwrap();
        assert_eq!(st.read(), State::default());
        assert!(fs::read_dir(&d).unwrap().flatten().any(|e| e.file_name().to_string_lossy().starts_with("state.json.corrupt-")));
        // old files without newer fields still load
        fs::write(d.join("state.json"), br#"{"installTonight":true}"#).unwrap();
        assert!(st.read().install_tonight);
        let _ = fs::remove_dir_all(&d);
    }
}
