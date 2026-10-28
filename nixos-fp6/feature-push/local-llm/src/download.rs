use crate::Model;
use anyhow::{bail, Context, Result};
use futures_util::StreamExt;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::path::Path;
use tokio::{
    fs,
    io::{AsyncReadExt, AsyncWriteExt},
};

#[derive(Clone, Debug, Serialize)]
pub struct Progress {
    pub id: String,
    pub received: u64,
    pub total: u64,
    pub state: String,
    pub message: String,
}
impl Progress {
    pub fn new(model: &Model, received: u64, state: &str, message: &str) -> Self {
        Self {
            id: model.id.clone(),
            received,
            total: model.bytes,
            state: state.into(),
            message: message.into(),
        }
    }
}
pub async fn verify(path: &Path, model: &Model) -> Result<()> {
    let mut file = fs::File::open(path).await?;
    if file.metadata().await?.len() != model.bytes {
        bail!("model size does not match catalogue");
    }
    let mut hasher = Sha256::new();
    let mut buf = vec![0; 1024 * 1024];
    loop {
        let n = file.read(&mut buf).await?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    if format!("{:x}", hasher.finalize()) != model.sha256 {
        bail!("SHA-256 mismatch; delete the partial model and retry");
    }
    Ok(())
}
// Only explicit Download calls reach here. Neither startup nor inference fetches anything.
pub async fn fetch(
    model: &Model,
    root: &Path,
    client: &reqwest::Client,
    mut progress: impl FnMut(Progress),
) -> Result<()> {
    fs::create_dir_all(root).await?;
    let part = model.part(root);
    let mut offset = fs::metadata(&part).await.map(|m| m.len()).unwrap_or(0);
    if offset > model.bytes {
        fs::remove_file(&part).await?;
        offset = 0;
    }
    if offset < model.bytes {
        let mut request = client.get(&model.url).header("Accept-Encoding", "identity");
        if offset > 0 {
            request = request.header("Range", format!("bytes={offset}-"));
        }
        let response = request
            .send()
            .await
            .context("model download failed")?
            .error_for_status()?;
        if response.status() == reqwest::StatusCode::PARTIAL_CONTENT {
            let range = response
                .headers()
                .get("Content-Range")
                .and_then(|h| h.to_str().ok())
                .unwrap_or("");
            let expected = format!("bytes {offset}-{}/{}", model.bytes - 1, model.bytes);
            if range != expected {
                bail!("unexpected Content-Range: {range}");
            }
        } else if response.status() == reqwest::StatusCode::OK {
            offset = 0;
        } else {
            bail!("unexpected download status {}", response.status());
        }
        let mut file = fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(offset == 0)
            .append(offset != 0)
            .open(&part)
            .await?;
        progress(Progress::new(model, offset, "downloading", ""));
        let mut stream = response.bytes_stream();
        let mut last = std::time::Instant::now();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            if offset + chunk.len() as u64 > model.bytes {
                bail!("download exceeds catalogue size");
            }
            file.write_all(&chunk).await?;
            offset += chunk.len() as u64;
            if last.elapsed().as_millis() >= 200 {
                progress(Progress::new(model, offset, "downloading", ""));
                last = std::time::Instant::now();
            }
        }
        file.sync_all().await?;
    }
    progress(Progress::new(
        model,
        offset,
        "verifying",
        "Checking SHA-256",
    ));
    verify(&part, model).await?;
    fs::rename(&part, model.path(root)).await?;
    progress(Progress::new(model, model.bytes, "complete", ""));
    Ok(())
}
