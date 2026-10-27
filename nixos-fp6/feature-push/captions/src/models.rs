//! Catalog and verified downloader adapted from fp6-dictation (MIT).
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    io::Read,
    path::{Path, PathBuf},
};
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Model {
    pub id: String,
    pub filename: String,
    pub bytes: u64,
    pub sha256: String,
}
pub fn catalog() -> Vec<Model> {
    serde_json::from_str(include_str!("../models.json")).expect("embedded catalog")
}
pub fn model(id: &str) -> Result<Model> {
    catalog()
        .into_iter()
        .find(|m| m.id == id)
        .context("Unknown model; choose tiny-q5_1 or base-q5_1")
}
pub fn directory() -> PathBuf {
    std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/share")
        })
        .join("fp6-captions/models")
}
pub fn verify(path: &Path, m: &Model) -> Result<()> {
    let mut file =
        std::fs::File::open(path).context("Model missing: use fp6-captions download MODEL")?;
    if file.metadata()?.len() != m.bytes {
        bail!("Model size mismatch");
    }
    let mut hash = Sha256::new();
    let mut chunk = [0; 65536];
    loop {
        let n = file.read(&mut chunk)?;
        if n == 0 {
            break;
        }
        hash.update(&chunk[..n]);
    }
    if format!("{:x}", hash.finalize()) != m.sha256 {
        bail!("Model checksum mismatch");
    }
    Ok(())
}
pub fn download(id: &str) -> Result<()> {
    let m = model(id)?;
    let dir = directory();
    std::fs::create_dir_all(&dir)?;
    let part = dir.join(format!("{}.{}.part", m.filename, std::process::id()));
    // Refuse a stale path/symlink before invoking curl.
    let _file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&part)?;
    let result = (|| -> Result<()> {
        let status = std::process::Command::new("curl")
            .args([
                "--fail",
                "--location",
                "--proto",
                "=https",
                "--proto-redir",
                "=https",
                "--retry",
                "2",
                "--max-time",
                "900",
                "--max-filesize",
                &m.bytes.to_string(),
                "--output",
            ])
            .arg(&part)
            .arg(format!(
                "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/{}",
                m.filename
            ))
            .status()?;
        if !status.success() {
            bail!("Download failed");
        }
        verify(&part, &m)?;
        std::fs::rename(&part, dir.join(&m.filename))?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&part);
    }
    result
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_tampering_and_traversal() {
        assert!(model("../tiny").is_err());
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("model");
        std::fs::write(&p, b"abc").unwrap();
        let mut m = Model {
            id: "test".into(),
            filename: "test".into(),
            bytes: 3,
            sha256: format!("{:x}", Sha256::digest(b"abc")),
        };
        verify(&p, &m).unwrap();
        std::fs::write(&p, b"abd").unwrap();
        assert!(verify(&p, &m).is_err());
        m.bytes = 4;
        assert!(verify(&p, &m).is_err());
    }
}
