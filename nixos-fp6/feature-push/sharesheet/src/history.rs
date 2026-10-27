use crate::model::Target;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    os::unix::fs::{DirBuilderExt, PermissionsExt},
    path::Path,
};

// Deliberately stores identifiers only: no shared content, names, paths or timestamps.
#[derive(Default, Debug, Serialize, Deserialize)]
pub struct History {
    pub ids: Vec<String>,
}
impl History {
    pub fn load(path: &Path) -> Self {
        use std::io::Read;
        let mut bytes = Vec::new();
        let mut value: Self = fs::File::open(path)
            .ok()
            .and_then(|file| file.take(16385).read_to_end(&mut bytes).ok())
            .filter(|_| bytes.len() <= 16384)
            .and_then(|_| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        let mut seen = std::collections::HashSet::new();
        value.ids.retain(|id| {
            !id.is_empty() && id.len() <= 1024 && id.contains(':') && seen.insert(id.clone())
        });
        value.ids.truncate(8);
        value
    }
    pub fn record(&mut self, target: &Target) {
        if !target.direct || target.id.len() > 1024 {
            return;
        }
        self.ids.retain(|id| id != &target.id);
        self.ids.insert(0, target.id.clone());
        self.ids.truncate(8);
    }
    pub fn recent(&self, available: &[Target]) -> Vec<Target> {
        self.ids
            .iter()
            .filter_map(|id| available.iter().find(|t| &t.id == id && t.direct).cloned())
            .collect()
    }
    pub fn save(&self, path: &Path) -> Result<()> {
        let dir = path.parent().context("History needs a parent directory")?;
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(dir)?;
        fs::set_permissions(dir, fs::Permissions::from_mode(0o700))?;
        // A unique RAII temporary file survives neither errors nor a stale
        // fixed .new filename left by an interrupted earlier daemon.
        let mut file = tempfile::NamedTempFile::new_in(dir)?;
        use std::io::Write;
        file.write_all(&serde_json::to_vec(self)?)?;
        file.as_file().sync_all()?;
        file.persist(path)?;
        fs::File::open(dir)?.sync_all()?;
        Ok(())
    }
}
