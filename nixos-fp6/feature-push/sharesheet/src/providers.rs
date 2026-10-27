use crate::{
    model::{require_single, Request, Target},
    registry,
};
use anyhow::{bail, ensure, Context, Result};
use async_trait::async_trait;
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};
use zbus::{
    zvariant::{OwnedObjectPath, OwnedValue, Value as BusValue},
    Connection, Proxy,
};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum Outcome {
    Message(String),
    Copy(String),
    Content(String),
    Ocr(String),
    Quick(String),
}
#[async_trait]
pub trait Provider: Send + Sync {
    fn id(&self) -> &'static str;
    async fn targets(&self, bus: &Connection, request: &Request) -> Result<Vec<Target>>;
    async fn activate(
        &self,
        bus: &Connection,
        request: &Request,
        target: &Target,
        option: &str,
    ) -> Result<Outcome>;
}

pub async fn available(bus: &Connection, name: &str) -> bool {
    let Ok(proxy) = zbus::fdo::DBusProxy::new(bus).await else {
        return false;
    };
    let Ok(name) = zbus::names::BusName::try_from(name) else {
        return false;
    };
    proxy.name_has_owner(name).await.unwrap_or(false)
}
async fn proxy<'a>(bus: &'a Connection, name: &'a str) -> Result<Proxy<'a>> {
    Ok(Proxy::new(bus, name, format!("/{}", name.replace('.', "/")), name).await?)
}
fn text(value: &Value, key: &str) -> String {
    value[key].as_str().unwrap_or_default().to_owned()
}

pub struct QuickShare;
#[async_trait]
impl Provider for QuickShare {
    fn id(&self) -> &'static str {
        "quick"
    }
    async fn targets(&self, bus: &Connection, r: &Request) -> Result<Vec<Target>> {
        if !r.is_files() && r.items.len() != 1 {
            return Ok(vec![]);
        }
        if !available(bus, "com.fairphone6.QuickShare1").await {
            return Ok(vec![]);
        }
        let p = proxy(bus, "com.fairphone6.QuickShare1").await?;
        let value: String = p.call("ListDevices", &()).await?;
        let devices: Vec<Value> = serde_json::from_str(&value)?;
        Ok(devices
            .iter()
            .filter(|d| !text(d, "id").is_empty())
            .map(|d| {
                Target::new(
                    self.id(),
                    &text(d, "id"),
                    &text(d, "name"),
                    "Nearby · Quick Share",
                    true,
                )
            })
            .collect())
    }
    async fn activate(
        &self,
        bus: &Connection,
        r: &Request,
        t: &Target,
        _: &str,
    ) -> Result<Outcome> {
        let p = proxy(bus, "com.fairphone6.QuickShare1").await?;
        let items = if r.is_files() {
            r.items.clone()
        } else {
            vec![format!(
                "{}:{}",
                if r.is_link() { "url" } else { "text" },
                r.items[0]
            )]
        };
        let id: String = p.call("Send", &(t.local_id()?, items)).await?;
        Ok(Outcome::Quick(id))
    }
}
pub struct KdeConnect;
#[async_trait]
impl Provider for KdeConnect {
    fn id(&self) -> &'static str {
        "kde"
    }
    async fn targets(&self, bus: &Connection, r: &Request) -> Result<Vec<Target>> {
        // The sibling API supports file URLs and HTTP(S), not arbitrary text.
        if !r.is_files() && !r.is_link() {
            return Ok(vec![]);
        }
        if !available(bus, "com.fairphone6.KdeConnect1").await {
            return Ok(vec![]);
        }
        let p = proxy(bus, "com.fairphone6.KdeConnect1").await?;
        let state: String = p.call("Devices", &()).await?;
        let state: Value = serde_json::from_str(&state)?;
        Ok(state["devices"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|d| d["online"] == true && d["paired"] == true && d["plugins"]["share"] == true)
            .map(|d| {
                Target::new(
                    self.id(),
                    &text(d, "id"),
                    &text(d, "name"),
                    "Paired · KDE Connect",
                    true,
                )
            })
            .collect())
    }
    async fn activate(
        &self,
        bus: &Connection,
        r: &Request,
        t: &Target,
        _: &str,
    ) -> Result<Outcome> {
        let p = proxy(bus, "com.fairphone6.KdeConnect1").await?;
        for (index, item) in r.items.iter().enumerate() {
            p.call::<_, _, ()>("Action", &(t.local_id()?, "share", item))
                .await
                .with_context(|| {
                    format!(
                        "KDE Connect failed at item {}; earlier items may already have been sent",
                        index + 1
                    )
                })?;
        }
        Ok(Outcome::Message(
            "Sent to KDE Connect. Check its transfer status.".into(),
        ))
    }
}
pub struct ContentHub;
#[async_trait]
impl Provider for ContentHub {
    fn id(&self) -> &'static str {
        "content"
    }
    async fn targets(&self, bus: &Connection, r: &Request) -> Result<Vec<Target>> {
        if !available(bus, registry::BUS).await {
            return Ok(vec![]);
        }
        registry::peers(bus, r.content_type()).await
    }
    async fn activate(&self, _: &Connection, _: &Request, t: &Target, _: &str) -> Result<Outcome> {
        Ok(Outcome::Content(t.local_id()?.into()))
    }
}
static WEB_WINDOWS: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(4);
pub fn stage_text(r: &Request) -> Result<tempfile::NamedTempFile> {
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
    let runtime = std::env::var_os("XDG_RUNTIME_DIR").context("No private runtime directory")?;
    let dir = PathBuf::from(runtime).join("fp6-share");
    std::fs::DirBuilder::new()
        .mode(0o700)
        .create(&dir)
        .or_else(|e| {
            if e.kind() == std::io::ErrorKind::AlreadyExists {
                Ok(())
            } else {
                Err(e)
            }
        })?;
    let meta = std::fs::symlink_metadata(&dir)?;
    ensure!(
        meta.is_dir() && !meta.file_type().is_symlink(),
        "Runtime directory must be an ordinary directory"
    );
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))?;
    stage_text_in(r, &dir)
}
pub fn stage_text_in(r: &Request, dir: &Path) -> Result<tempfile::NamedTempFile> {
    use std::io::Write;
    let mut file = tempfile::Builder::new()
        .prefix("Shared text-")
        .suffix(".txt")
        .tempfile_in(dir)?;
    file.write_all(r.items.join("\n").as_bytes())?;
    file.flush()?;
    Ok(file)
}
pub struct WebShare;
#[async_trait]
impl Provider for WebShare {
    fn id(&self) -> &'static str {
        "web"
    }
    async fn targets(&self, bus: &Connection, r: &Request) -> Result<Vec<Target>> {
        if (!r.is_files() && std::env::var_os("XDG_RUNTIME_DIR").is_none())
            || !available(bus, "com.fairphone6.WebShare1").await
            || executable("fp6-webshare").is_none()
        {
            return Ok(vec![]);
        }
        Ok(vec![Target::new(
            self.id(),
            "link",
            "Share via link / QR",
            "Choose Wi-Fi and expiry in WebShare",
            false,
        )])
    }
    async fn activate(&self, _: &Connection, r: &Request, _: &Target, _: &str) -> Result<Outcome> {
        // Hand off to its consent/configuration UI. No HTTP listener starts here.
        let permit = WEB_WINDOWS
            .try_acquire()
            .context("Close an earlier WebShare window first")?;
        let staged = if r.is_files() {
            None
        } else {
            Some(stage_text(r)?)
        };
        let files = if let Some(file) = &staged {
            vec![file.path().to_path_buf()]
        } else {
            r.files()?
        };
        let mut child = tokio::process::Command::new("fp6-webshare")
            .env_remove("APP_ID")
            .arg("gui")
            .args(files)
            .spawn()?;
        tokio::spawn(async move {
            let _permit = permit;
            let _staged = staged;
            // WebShare holds open file descriptors after Start, so it can finish
            // an active session even after this window and our temporary path close.
            let _ = child.wait().await;
        });
        Ok(Outcome::Message(
            "Continue in WebShare to choose Wi-Fi, expiry and show the QR code.".into(),
        ))
    }
}
pub struct Actions;
#[async_trait]
impl Provider for Actions {
    fn id(&self) -> &'static str {
        "action"
    }
    async fn targets(&self, bus: &Connection, r: &Request) -> Result<Vec<Target>> {
        let mut targets = vec![Target::new(
            self.id(),
            "copy",
            "Copy",
            "Copy text, links or file URIs",
            false,
        )];
        if r.items.len() == 1 && available(bus, "org.freedesktop.portal.Desktop").await {
            targets.push(Target::new(
                self.id(),
                "save",
                "Save to Files",
                "Choose a destination",
                false,
            ));
        }
        if r.is_files()
            && r.items.len() == 1
            && r.mime.starts_with("image/")
            && available(bus, "com.fairphone6.Ocr1").await
        {
            targets.push(Target::new(
                self.id(),
                "ocr",
                "Text recognition",
                "Read text from this image · on device",
                false,
            ));
        }
        if r.is_files()
            && (r.mime.starts_with("image/")
                || r.mime == "application/pdf"
                || r.mime == "text/plain")
            && cups_default().await.is_ok()
        {
            targets.push(Target::new(
                self.id(),
                "print",
                "Print",
                "Review before sending to the default printer",
                false,
            ));
        }
        Ok(targets)
    }
    async fn activate(
        &self,
        bus: &Connection,
        r: &Request,
        t: &Target,
        option: &str,
    ) -> Result<Outcome> {
        match t.local_id()? {
            "copy" => Ok(Outcome::Copy(r.items.join("\n"))),
            "save" => {
                save_file(bus, r).await?;
                Ok(Outcome::Message("File saved.".into()))
            }
            "print" => {
                ensure!(option == "confirm", "Confirm printing first");
                let printer = cups_default().await?;
                let output = tokio::process::Command::new("lp")
                    .arg("-d")
                    .arg(&printer)
                    .arg("--")
                    .args(r.files()?)
                    .output()
                    .await?;
                ensure!(output.status.success(), "CUPS rejected the print job");
                Ok(Outcome::Message(format!("Print job queued on {printer}.")))
            }
            "ocr" => {
                require_single(r)?;
                let p = proxy(bus, "com.fairphone6.Ocr1").await?;
                let path = r.files()?.remove(0);
                let id: String = p
                    .call(
                        "Recognize",
                        &(path.to_str().context("Invalid path")?, "eng+ron"),
                    )
                    .await?;
                Ok(Outcome::Ocr(id))
            }
            _ => bail!("Unknown action"),
        }
    }
}
pub fn executable(name: &str) -> Option<PathBuf> {
    std::env::var_os("PATH").and_then(|p| {
        std::env::split_paths(&p)
            .map(|p| p.join(name))
            .find(|p| p.is_file())
    })
}
async fn cups_default() -> Result<String> {
    ensure!(
        executable("lp").is_some() && executable("lpstat").is_some(),
        "CUPS is unavailable"
    );
    let out = tokio::process::Command::new("lpstat")
        .env("LC_ALL", "C")
        .arg("-d")
        .output()
        .await?;
    ensure!(out.status.success(), "No CUPS default printer");
    let text = String::from_utf8(out.stdout)?;
    let printer = text
        .strip_prefix("system default destination: ")
        .context("No default printer")?
        .trim();
    ensure!(!printer.is_empty(), "No default printer");
    Ok(printer.into())
}
pub async fn ocr_status(bus: &Connection, id: &str) -> Result<String> {
    let p = proxy(bus, "com.fairphone6.Ocr1").await?;
    Ok(p.call("GetJob", &(id,)).await?)
}
struct PortalRequest {
    bus: Connection,
    path: String,
}
impl Drop for PortalRequest {
    fn drop(&mut self) {
        let bus = self.bus.clone();
        let path = self.path.clone();
        tokio::spawn(async move {
            if let Ok(p) = Proxy::new(
                &bus,
                "org.freedesktop.portal.Desktop",
                path.as_str(),
                "org.freedesktop.portal.Request",
            )
            .await
            {
                let _ = p.call::<_, _, ()>("Close", &()).await;
            }
        });
    }
}
struct PendingFile {
    path: PathBuf,
    committed: bool,
}
impl Drop for PendingFile {
    fn drop(&mut self) {
        if !self.committed {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}
pub async fn quick_status(bus: &Connection, id: &str) -> Result<String> {
    let p = proxy(bus, "com.fairphone6.QuickShare1").await?;
    let transfers: String = p.call("ListTransfers", &()).await?;
    let transfers: Vec<Value> = serde_json::from_str(&transfers)?;
    let transfer = transfers
        .into_iter()
        .find(|t| t["id"] == id)
        .context("Transfer is no longer available")?;
    Ok(transfer.to_string())
}
pub async fn cancel_job(bus: &Connection, provider: &str, id: &str) -> Result<()> {
    let (name, method) = match provider {
        "quick" => ("com.fairphone6.QuickShare1", "Cancel"),
        "ocr" => ("com.fairphone6.Ocr1", "CancelJob"),
        _ => bail!("Unknown job provider"),
    };
    let p = proxy(bus, name).await?;
    p.call::<_, _, ()>(method, &(id,)).await?;
    Ok(())
}
static TOKEN: AtomicU64 = AtomicU64::new(1);
async fn save_file(bus: &Connection, r: &Request) -> Result<()> {
    require_single(r)?;
    let token = format!("fp6share{}", TOKEN.fetch_add(1, Ordering::Relaxed));
    let sender = bus
        .unique_name()
        .context("No bus name")?
        .as_str()
        .trim_start_matches(':')
        .replace('.', "_");
    let path = format!("/org/freedesktop/portal/desktop/request/{sender}/{token}");
    let _portal_lifetime = PortalRequest {
        bus: bus.clone(),
        path: path.clone(),
    };
    let request = Proxy::new(
        bus,
        "org.freedesktop.portal.Desktop",
        path.as_str(),
        "org.freedesktop.portal.Request",
    )
    .await?;
    // Subscribe before SaveFile to avoid losing an immediate Response.
    let mut response = request.receive_signal("Response").await?;
    let chooser = Proxy::new(
        bus,
        "org.freedesktop.portal.Desktop",
        "/org/freedesktop/portal/desktop",
        "org.freedesktop.portal.FileChooser",
    )
    .await?;
    let name = if r.is_files() {
        r.files()?[0]
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned()
    } else {
        "Shared text.txt".into()
    };
    let options = HashMap::from([
        ("handle_token", BusValue::from(token.as_str())),
        ("current_name", BusValue::from(name.as_str())),
        ("modal", BusValue::from(true)),
    ]);
    let returned: OwnedObjectPath = chooser
        .call("SaveFile", &("", "Save shared item", options))
        .await?;
    ensure!(
        returned.as_str() == path,
        "Portal returned an unexpected request path"
    );
    let message = tokio::time::timeout(Duration::from_secs(300), response.next())
        .await
        .context("Save dialog timed out")?
        .context("Save dialog closed")?;
    let (code, values): (u32, HashMap<String, OwnedValue>) = message.body().deserialize()?;
    if code == 1 {
        bail!("Save canceled");
    }
    ensure!(code == 0, "Save dialog failed");
    let uris = values
        .get("uris")
        .context("Portal returned no destination")?
        .try_clone()?;
    let uris: Vec<String> = uris.try_into()?;
    ensure!(uris.len() == 1, "Expected one destination");
    let destination = url::Url::parse(&uris[0])?
        .to_file_path()
        .map_err(|_| anyhow::anyhow!("Expected a local destination"))?;
    write_destination(r, &destination).await
}
pub async fn write_destination(r: &Request, destination: &Path) -> Result<()> {
    // Never follow or overwrite an existing final path, including a symlink.
    let mut output = tokio::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(destination)
        .await
        .context("Choose an unused filename; existing files are preserved")?;
    let mut pending = PendingFile {
        path: destination.to_path_buf(),
        committed: false,
    };
    let result: Result<()> = async {
        if r.is_files() {
            let mut input = tokio::fs::File::open(&r.files()?[0]).await?;
            tokio::io::copy(&mut input, &mut output).await?;
        } else {
            use tokio::io::AsyncWriteExt;
            output.write_all(r.items[0].as_bytes()).await?;
        }
        output.sync_all().await?;
        Ok(())
    }
    .await;
    pending.committed = result.is_ok();
    result
}
pub fn defaults() -> Vec<Box<dyn Provider>> {
    vec![
        Box::new(QuickShare),
        Box::new(KdeConnect),
        Box::new(ContentHub),
        Box::new(WebShare),
        Box::new(Actions),
    ]
}
