//! The only transcript persistence path: explicitly invoked by the Save action.
use anyhow::{bail, Result};
use std::{
    io::Write,
    os::unix::fs::{DirBuilderExt, OpenOptionsExt},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};
pub fn save_visible(text: &str) -> Result<PathBuf> {
    let dir = crate::models::directory()
        .parent()
        .expect("model directory parent")
        .join("saved");
    save_in(&dir, text)
}
fn save_in(dir: &Path, text: &str) -> Result<PathBuf> {
    if text.trim().is_empty() {
        bail!("No captions to save");
    }
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)?;
    let stamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    let path = dir.join(format!("captions-{stamp}.txt"));
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&path)?;
    writeln!(f, "{text}")?;
    Ok(path)
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    #[test]
    fn only_explicit_nonempty_saves_create_private_files() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("saved");
        assert!(save_in(&dir, "").is_err());
        assert!(!dir.exists());
        let first = save_in(&dir, "Bună ziua.").unwrap();
        let second = save_in(&dir, "Next caption.").unwrap();
        assert_ne!(first, second);
        assert_eq!(std::fs::read_to_string(&first).unwrap(), "Bună ziua.\n");
        assert_eq!(
            std::fs::metadata(&first).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(
            std::fs::metadata(&dir).unwrap().permissions().mode() & 0o777,
            0o700
        );
    }
}
