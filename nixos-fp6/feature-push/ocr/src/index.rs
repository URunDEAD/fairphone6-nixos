use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::Write,
    os::unix::fs::{DirBuilderExt, PermissionsExt},
    path::PathBuf,
};
use unicode_normalization::UnicodeNormalization;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Entry {
    pub path: String,
    pub text: String,
}
#[derive(Clone, Default, Serialize, Deserialize)]
struct Disk {
    enabled: bool,
    entries: Vec<Entry>,
}
pub struct Index {
    directory: PathBuf,
    disk: Disk,
}
impl Index {
    pub fn open(directory: PathBuf) -> Result<Self> {
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&directory)?;
        fs::set_permissions(&directory, fs::Permissions::from_mode(0o700))?;
        let path = directory.join("index.json");
        let disk = if path.exists() {
            if fs::metadata(&path)?.len() > 20 * 1024 * 1024 {
                anyhow::bail!("Index exceeds size limit");
            }
            serde_json::from_slice(&fs::read(path)?)
                .context("Index is corrupt; move index.json aside to reset it")?
        } else {
            Disk::default()
        };
        Ok(Self { directory, disk })
    }
    pub fn enabled(&self) -> bool {
        self.disk.enabled
    }
    fn save(&self, disk: &Disk) -> Result<()> {
        let mut file = tempfile::NamedTempFile::new_in(&self.directory)?;
        file.write_all(&serde_json::to_vec(disk)?)?;
        file.as_file().sync_all()?;
        file.persist(self.directory.join("index.json"))?;
        Ok(())
    }
    pub fn set_enabled(&mut self, enabled: bool) -> Result<()> {
        let disk = Disk {
            enabled,
            entries: if enabled {
                self.disk.entries.clone()
            } else {
                vec![]
            },
        };
        self.save(&disk)?;
        self.disk = disk;
        Ok(())
    }
    pub fn add(&mut self, path: String, text: &str) -> Result<()> {
        if !self.enabled() {
            return Ok(());
        }
        let mut disk = self.disk.clone();
        disk.entries.retain(|e| e.path != path);
        disk.entries.insert(
            0,
            Entry {
                path,
                text: text.chars().take(8000).collect(),
            },
        );
        disk.entries.truncate(500);
        self.save(&disk)?;
        self.disk = disk;
        Ok(())
    }
    pub fn search(&self, query: &str) -> Vec<Entry> {
        let normalize = |text: &str| text.nfc().collect::<String>().to_lowercase();
        let query = normalize(query.trim());
        if !self.enabled() || query.is_empty() {
            return vec![];
        }
        self.disk
            .entries
            .iter()
            .filter(|e| normalize(&e.text).contains(&query))
            .take(50)
            .cloned()
            .collect()
    }
}
