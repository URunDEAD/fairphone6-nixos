use crate::{
    app::App,
    network::connect_from,
    protocol::{Packet, MAX_PORT},
    tls,
};
use anyhow::{ensure, Result};
use serde_json::json;
use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

pub const MAX_FILE: u64 = 256 * 1024 * 1024;
pub fn safe_filename(name: &str) -> Result<String> {
    ensure!(
        !name.is_empty() && name.len() <= 180 && name != "." && name != "..",
        "invalid filename length"
    );
    ensure!(
        !name
            .chars()
            .any(|c| c.is_control() || matches!(c, '/' | '\\'))
            && !name.starts_with('.'),
        "unsafe filename"
    );
    Ok(name.into())
}
pub fn validated_url(text: &str) -> Result<url::Url> {
    ensure!(text.len() <= 8192, "URL too long");
    let url = url::Url::parse(text)?;
    ensure!(
        matches!(url.scheme(), "http" | "https")
            && url.host_str().is_some()
            && url.username().is_empty()
            && url.password().is_none(),
        "only HTTP(S) URLs without credentials may be shared"
    );
    Ok(url)
}
fn download_directory(app: &App) -> Result<PathBuf> {
    let path = app.downloads.clone();
    std::fs::create_dir_all(&path)?;
    ensure!(
        !std::fs::symlink_metadata(&path)?.file_type().is_symlink(),
        "download directory may not be a symlink"
    );
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700))?;
    Ok(path)
}
async fn peer(app: &App, id: &str) -> Result<(std::net::SocketAddr, std::net::SocketAddr, String)> {
    ensure!(app.allowed(id, "share").await, "sharing no longer allowed");
    let i = app.inner.lock().await;
    let d = i
        .devices
        .get(id)
        .ok_or_else(|| anyhow::anyhow!("peer disconnected"))?;
    Ok((d.address, d.local, d.fingerprint.clone()))
}
async fn still_allowed(app: &App, id: &str, pin: &str) -> Result<()> {
    ensure!(app.allowed(id, "share").await, "sharing permission revoked");
    let i = app.inner.lock().await;
    ensure!(
        i.store.peers.get(id).is_some_and(|p| p.fingerprint == pin),
        "pairing changed during transfer"
    );
    Ok(())
}
pub async fn receive(app: Arc<App>, id: &str, packet: &Packet) -> Result<()> {
    let filename = safe_filename(crate::protocol::string(&packet.body, "filename")?)?;
    let size = packet
        .payload_size
        .ok_or_else(|| anyhow::anyhow!("missing payload size"))?;
    ensure!(size <= MAX_FILE, "file exceeds 256 MiB limit");
    let port = packet
        .payload_info
        .as_ref()
        .and_then(|v| v["port"].as_u64())
        .filter(|p| (1716..=1764).contains(p))
        .ok_or_else(|| anyhow::anyhow!("invalid payload port"))? as u16;
    let permit = app.transfers.clone().try_acquire_owned()?;
    let (mut address, local, pin) = peer(&app, id).await?;
    address.set_port(port);
    let id = id.to_owned();
    tokio::spawn(async move {
        let _permit = permit;
        let result: Result<PathBuf> = tokio::time::timeout(Duration::from_secs(300), async {
            let tcp = connect_from(local.ip(), address).await?;
            let mut stream = app.credentials.handshake(tcp, false).await?;
            let cert = stream
                .ssl()
                .peer_certificate()
                .ok_or_else(|| anyhow::anyhow!("missing transfer certificate"))?;
            ensure!(
                tls::fingerprint(&cert)? == pin && tls::common_name(&cert)? == id,
                "payload certificate differs from paired device"
            );
            still_allowed(&app, &id, &pin).await?;
            let directory = download_directory(&app)?;
            let temp = tempfile::NamedTempFile::new_in(&directory)?;
            let mut file = tokio::fs::File::from_std(temp.reopen()?);
            let mut remaining = size;
            let mut buffer = vec![0; 32768];
            while remaining > 0 {
                still_allowed(&app, &id, &pin).await?;
                let cap = remaining.min(buffer.len() as u64) as usize;
                let count =
                    tokio::time::timeout(Duration::from_secs(20), stream.read(&mut buffer[..cap]))
                        .await??;
                ensure!(count > 0, "truncated payload");
                file.write_all(&buffer[..count]).await?;
                remaining -= count as u64;
            }
            file.sync_all().await?;
            still_allowed(&app, &id, &pin).await?;
            drop(file);
            // persist_noclobber atomically fails on existing names and symlinks.
            let target = directory.join(format!("{}-{}", uuid::Uuid::new_v4().simple(), filename));
            temp.persist_noclobber(&target)?;
            Ok(target)
        })
        .await
        .map_err(anyhow::Error::from)
        .and_then(|v| v);
        match result {
            Ok(path) => {
                app.event(format!(
                    "Received {}",
                    path.file_name().unwrap().to_string_lossy()
                ))
                .await
            }
            Err(e) => app.event(format!("File receive failed: {e}")).await,
        }
    });
    Ok(())
}
pub async fn send_share(app: Arc<App>, id: &str, text: &str) -> Result<()> {
    let url = url::Url::parse(text)?;
    if url.scheme() != "file" {
        validated_url(text)?;
        return app
            .send(
                id,
                Packet::new("kdeconnect.share.request", json!({"url":text})),
            )
            .await;
    }
    let path = url
        .to_file_path()
        .map_err(|_| anyhow::anyhow!("invalid local file URL"))?;
    let filename = safe_filename(
        path.file_name()
            .and_then(|v| v.to_str())
            .ok_or_else(|| anyhow::anyhow!("invalid filename"))?,
    )?;
    let mut file = tokio::fs::File::open(&path).await?;
    let meta = file.metadata().await?;
    ensure!(
        meta.is_file() && meta.len() <= MAX_FILE,
        "only regular files up to 256 MiB may be sent"
    );
    let size = meta.len();
    let permit = app.transfers.clone().try_acquire_owned()?;
    let (address, local, pin) = peer(&app, id).await?;
    let mut listener = None;
    for port in 1739..=MAX_PORT {
        if let Ok(l) = TcpListener::bind((local.ip(), port)).await {
            listener = Some(l);
            break;
        }
    }
    let listener = listener.ok_or_else(|| anyhow::anyhow!("no payload port available"))?;
    let mut packet = Packet::new(
        "kdeconnect.share.request",
        json!({"filename":filename, "numberOfFiles":1, "totalPayloadSize":size}),
    );
    packet.payload_size = Some(size);
    packet.payload_info = Some(json!({"port":listener.local_addr()?.port()}));
    app.send(id, packet).await?;
    let id = id.to_owned();
    tokio::spawn(async move {
        let _permit = permit;
        let result: Result<()> = tokio::time::timeout(Duration::from_secs(300), async {
            // Unexpected clients cannot consume the one transfer slot: keep
            // listening until the paired host presents the pinned certificate.
            let mut stream = loop {
                let (tcp, source) =
                    tokio::time::timeout(Duration::from_secs(30), listener.accept()).await??;
                if source.ip() != address.ip() {
                    continue;
                }
                let Ok(stream) = app.credentials.handshake(tcp, true).await else {
                    continue;
                };
                let cert = stream
                    .ssl()
                    .peer_certificate()
                    .ok_or_else(|| anyhow::anyhow!("missing payload certificate"))?;
                if tls::fingerprint(&cert)? == pin && tls::common_name(&cert)? == id {
                    break stream;
                }
            };
            let mut remaining = size;
            let mut buffer = vec![0u8; 32768];
            while remaining > 0 {
                still_allowed(&app, &id, &pin).await?;
                let cap = remaining.min(buffer.len() as u64) as usize;
                let n = file.read(&mut buffer[..cap]).await?;
                ensure!(n > 0, "source file changed during transfer");
                tokio::time::timeout(Duration::from_secs(20), stream.write_all(&buffer[..n]))
                    .await??;
                remaining -= n as u64;
            }
            let _ = stream.shutdown().await;
            Ok(())
        })
        .await
        .map_err(anyhow::Error::from)
        .and_then(|v| v);
        app.event(match result {
            Ok(()) => "File sent.".into(),
            Err(e) => format!("File send failed: {e}"),
        })
        .await;
    });
    Ok(())
}
pub fn local_file_url(path: &Path) -> Result<String> {
    Ok(url::Url::from_file_path(path.canonicalize()?)
        .map_err(|_| anyhow::anyhow!("invalid file path"))?
        .into())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn untrusted_paths_and_urls_are_rejected() {
        for name in [
            "../escape",
            "/etc/passwd",
            "..",
            "a\\b",
            ".secret",
            "a\n.txt",
        ] {
            assert!(safe_filename(name).is_err());
        }
        assert!(safe_filename("Synthetic photo.jpg").is_ok());
        for u in [
            "file:///etc/passwd",
            "javascript:alert(1)",
            "https://user:pass@example.invalid",
            "ftp://example.invalid",
        ] {
            assert!(validated_url(u).is_err());
        }
        assert!(validated_url("https://example.invalid/path").is_ok());
    }
}
