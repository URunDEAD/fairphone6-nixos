use anyhow::{ensure, Result};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
};

pub const PLUGINS: &[&str] = &[
    "ping",
    "battery",
    "notifications",
    "clipboard",
    "share",
    "findmyphone",
    "media",
    "input",
];
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Peer {
    pub name: String,
    pub fingerprint: String,
    pub version: u8,
    #[serde(default)]
    pub plugins: BTreeMap<String, bool>,
}
impl Peer {
    pub fn new(name: String, fingerprint: String, version: u8) -> Self {
        Self {
            name,
            fingerprint,
            version,
            plugins: PLUGINS
                .iter()
                .map(|p| (p.to_string(), matches!(*p, "ping" | "battery")))
                .collect(),
        }
    }
    pub fn allows(&self, plugin: &str) -> bool {
        self.plugins.get(plugin).copied().unwrap_or(false)
    }
}
#[derive(Debug, Serialize, Deserialize)]
pub struct Store {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub peers: BTreeMap<String, Peer>,
    #[serde(skip)]
    pub directory: PathBuf,
}
impl Store {
    pub fn load(directory: PathBuf) -> Result<Self> {
        fs::create_dir_all(&directory)?;
        fs::set_permissions(&directory, fs::Permissions::from_mode(0o700))?;
        let path = directory.join("state.json");
        let mut s = if path.exists() {
            let meta = fs::symlink_metadata(&path)?;
            ensure!(
                meta.is_file() && meta.len() <= 1024 * 1024,
                "invalid state file"
            );
            serde_json::from_slice::<Self>(&fs::read(path)?)?
        } else {
            Self {
                id: uuid::Uuid::new_v4().simple().to_string(),
                name: "Fairphone 6".into(),
                peers: BTreeMap::new(),
                directory: directory.clone(),
            }
        };
        ensure!(crate::protocol::valid_id(&s.id), "invalid saved identity");
        s.directory = directory;
        // Input consent is session-local; never restore it after a restart.
        for peer in s.peers.values_mut() {
            peer.plugins.insert("input".into(), false);
        }
        s.save()?;
        Ok(s)
    }
    pub fn save(&self) -> Result<()> {
        atomic_write(
            &self.directory.join("state.json"),
            &serde_json::to_vec_pretty(self)?,
        )
    }
    pub fn verify(&self, id: &str, fingerprint: &str, version: u8) -> Result<()> {
        if let Some(peer) = self.peers.get(id) {
            ensure!(
                peer.fingerprint == fingerprint,
                "pinned certificate changed; unpair locally before replacing it"
            );
            ensure!(version >= peer.version, "protocol downgrade rejected");
        }
        Ok(())
    }
}
pub fn atomic_write(path: &Path, data: &[u8]) -> Result<()> {
    use std::io::Write;
    let tmp = path.with_extension(format!("tmp-{}", uuid::Uuid::new_v4()));
    let result = (|| {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&tmp)?;
        file.write_all(data)?;
        file.sync_all()?;
        fs::rename(&tmp, path)?;
        fs::File::open(path.parent().unwrap())?.sync_all()?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(tmp);
    }
    result
}
pub fn state_directory() -> PathBuf {
    std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join(".local/state"))
        .join("lomiri-kdeconnect")
}
pub fn home() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .expect("HOME is required")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn privacy_and_pinning_survive_reload() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = Store::load(dir.path().into()).unwrap();
        let id = s.id.clone();
        let mut p = Peer::new("Synthetic".into(), "abc".into(), 8);
        assert!(!p.allows("clipboard") && !p.allows("notifications") && !p.allows("input"));
        p.plugins.insert("input".into(), true);
        s.peers.insert("synthetic-peer".into(), p);
        s.save().unwrap();
        let s = Store::load(dir.path().into()).unwrap();
        assert_eq!(id, s.id);
        assert!(!s.peers["synthetic-peer"].allows("input"));
        assert!(s.verify("synthetic-peer", "abc", 8).is_ok());
        assert!(s.verify("synthetic-peer", "bad", 8).is_err());
        assert!(s.verify("synthetic-peer", "abc", 7).is_err());
        assert_eq!(
            fs::metadata(dir.path().join("state.json"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
}
