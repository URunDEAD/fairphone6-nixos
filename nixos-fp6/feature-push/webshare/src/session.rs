use crate::network::{self, LanInterface};
use anyhow::{Context, Result, bail};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    fs::{File, OpenOptions},
    os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use tempfile::NamedTempFile;
use tokio::sync::{Mutex as AsyncMutex, Semaphore};
use tokio_util::sync::CancellationToken;

pub const PORT: u16 = 8765;
pub const MAX_UPLOAD: u64 = 100 * 1024 * 1024;
pub const MAX_PENDING: usize = 8;
pub const MAX_TEXT: usize = 4096;
pub fn secret() -> String {
    let mut bytes = [0u8; 16];
    rand::rng().fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}
pub fn secure_eq(a: &str, b: &str) -> bool {
    use subtle::ConstantTimeEq;
    a.len() == b.len() && bool::from(a.as_bytes().ct_eq(b.as_bytes()))
}
pub fn private_dir(path: &Path) -> Result<()> {
    std::fs::create_dir_all(path)?;
    let meta = std::fs::symlink_metadata(path)?;
    if !meta.is_dir() || meta.file_type().is_symlink() {
        bail!("Private directory must not be a symlink");
    }
    // Compare ownership with a freshly created file, without unsafe getuid FFI.
    let probe = NamedTempFile::new_in(path)?;
    if probe.as_file().metadata()?.uid() != meta.uid() {
        bail!("Private directory has the wrong owner");
    }
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))?;
    Ok(())
}
pub fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 180
        && name != "."
        && name != ".."
        && !name
            .chars()
            .any(|c| c.is_control() || matches!(c, '/' | '\\' | ':' | '%'))
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct StartOptions {
    pub files: Vec<PathBuf>,
    pub interface: String,
    #[serde(default = "default_minutes")]
    pub minutes: u64,
    #[serde(default)]
    pub uploads: bool,
    #[serde(default)]
    pub text: bool,
}
fn default_minutes() -> u64 {
    10
}
pub struct SharedFile {
    pub file: Arc<File>,
    pub name: String,
    pub size: u64,
}
#[derive(Serialize, Clone)]
pub struct FileView {
    pub id: usize,
    pub name: String,
    pub size: u64,
    pub downloaded: bool,
}
#[derive(Serialize, Clone)]
pub struct PendingView {
    pub id: String,
    pub name: String,
    pub size: u64,
}
pub struct PendingUpload {
    pub view: PendingView,
    pub file: NamedTempFile,
}
pub struct SessionData {
    pub browser: Option<String>,
    pub downloaded: HashSet<usize>,
    pub pending: Vec<PendingUpload>,
    pub text: Option<String>,
    pub rate_start: Instant,
    pub requests: u32,
}
pub struct Session {
    pub invitation: String,
    pub url: String,
    pub interface: LanInterface,
    pub files: Vec<SharedFile>,
    pub uploads: bool,
    pub text_enabled: bool,
    pub deadline: Instant,
    pub active: AtomicBool,
    pub cancel: CancellationToken,
    pub closed: CancellationToken,
    pub data: Mutex<SessionData>,
    pub transfers: Arc<Semaphore>,
    pub staging: PathBuf,
    pub received: PathBuf,
}
#[derive(Serialize)]
pub struct Status {
    pub active: bool,
    pub url: String,
    pub seconds_left: u64,
    pub interface: String,
    pub claimed: bool,
    pub files: Vec<FileView>,
    pub uploads: bool,
    pub text_enabled: bool,
    pub pending: Vec<PendingView>,
    pub text: Option<String>,
    pub qr: Vec<Vec<bool>>,
}
impl Session {
    pub fn new(
        options: StartOptions,
        interface: LanInterface,
        files: Vec<SharedFile>,
        staging: PathBuf,
        received: PathBuf,
    ) -> Self {
        let invitation = secret();
        let url = format!("http://{}:{PORT}/s/{invitation}", interface.address);
        Self {
            invitation,
            url,
            interface,
            files,
            uploads: options.uploads,
            text_enabled: options.text,
            deadline: Instant::now() + Duration::from_secs(options.minutes * 60),
            active: AtomicBool::new(true),
            cancel: CancellationToken::new(),
            closed: CancellationToken::new(),
            data: Mutex::new(SessionData {
                browser: None,
                downloaded: HashSet::new(),
                pending: vec![],
                text: None,
                rate_start: Instant::now(),
                requests: 0,
            }),
            transfers: Arc::new(Semaphore::new(4)),
            staging,
            received,
        }
    }
    pub fn alive(&self) -> bool {
        self.active.load(Ordering::SeqCst) && Instant::now() < self.deadline
    }
    pub fn stop(&self) {
        self.active.store(false, Ordering::SeqCst);
        self.cancel.cancel();
        let mut d = self.data.lock().unwrap();
        d.pending.clear();
        d.text = None;
        d.browser = None;
    }
    pub fn status(&self) -> Status {
        let d = self.data.lock().unwrap();
        let code = qrcode::QrCode::new(self.url.as_bytes()).expect("bounded URL");
        let qr = (0..code.width())
            .map(|y| {
                (0..code.width())
                    .map(|x| code[(x, y)] == qrcode::Color::Dark)
                    .collect()
            })
            .collect();
        Status {
            active: self.alive(),
            url: if self.alive() {
                self.url.clone()
            } else {
                String::new()
            },
            seconds_left: self
                .deadline
                .saturating_duration_since(Instant::now())
                .as_secs(),
            interface: self.interface.name.clone(),
            claimed: d.browser.is_some(),
            files: self
                .files
                .iter()
                .enumerate()
                .map(|(id, f)| FileView {
                    id,
                    name: f.name.clone(),
                    size: f.size,
                    downloaded: d.downloaded.contains(&id),
                })
                .collect(),
            uploads: self.uploads,
            text_enabled: self.text_enabled,
            pending: d.pending.iter().map(|p| p.view.clone()).collect(),
            text: d.text.clone(),
            qr,
        }
    }
    pub fn downloaded(&self, id: usize) {
        let complete = {
            let mut d = self.data.lock().unwrap();
            d.downloaded.insert(id);
            d.downloaded.len() == self.files.len()
        };
        if complete && !self.uploads && !self.text_enabled {
            self.stop();
        }
    }
    pub fn approve(&self, id: &str) -> Result<PathBuf> {
        let mut d = self.data.lock().unwrap();
        if !self.alive() {
            bail!("Share expired");
        }
        let index = d
            .pending
            .iter()
            .position(|p| p.view.id == id)
            .context("Unknown upload")?;
        let pending = &d.pending[index];
        let target = self
            .received
            .join(format!("{}-{}", secret(), pending.view.name));
        // Copy to a temporary file on the destination filesystem, then atomically
        // persist without replacement. Staging may be on a different mount.
        let mut destination = NamedTempFile::new_in(&self.received)?;
        std::io::copy(&mut pending.file.reopen()?, destination.as_file_mut())?;
        destination.as_file().sync_all()?;
        destination
            .persist_noclobber(&target)
            .map_err(|e| e.error)?;
        d.pending.remove(index);
        drop(d);
        if self.files.is_empty() {
            self.stop();
        }
        Ok(target)
    }
    pub fn reject(&self, id: &str) -> Result<()> {
        let mut d = self.data.lock().unwrap();
        let index = d
            .pending
            .iter()
            .position(|p| p.view.id == id)
            .context("Unknown upload")?;
        d.pending.remove(index);
        Ok(())
    }
    pub fn take_text(&self) -> Result<String> {
        let mut d = self.data.lock().unwrap();
        if !self.alive() {
            bail!("Share expired");
        }
        let text = d.text.take().context("No text pending")?;
        drop(d);
        if self.files.is_empty() {
            self.stop();
        }
        Ok(text)
    }
}
pub fn open_files(paths: &[PathBuf]) -> Result<Vec<SharedFile>> {
    if paths.len() > 32 {
        bail!("At most 32 files per share");
    }
    paths
        .iter()
        .map(|path| {
            // Linux O_NOFOLLOW | O_NONBLOCK: reject final symlinks and never block on FIFOs.
            let file = OpenOptions::new()
                .read(true)
                .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
                .open(path)
                .context("Cannot open shared file")?;
            let meta = file.metadata()?;
            if !meta.is_file() {
                bail!("Only regular files may be shared");
            }
            let name = path
                .file_name()
                .context("Missing filename")?
                .to_string_lossy()
                .to_string();
            Ok(SharedFile {
                file: Arc::new(file),
                name,
                size: meta.len(),
            })
        })
        .collect()
}
pub struct Manager {
    pub current: AsyncMutex<Option<Arc<Session>>>,
    pub staging: PathBuf,
    pub received: PathBuf,
}
impl Manager {
    pub fn new(staging: PathBuf, received: PathBuf) -> Result<Arc<Self>> {
        private_dir(&staging)?;
        private_dir(&received)?;
        // The singleton daemon owns staging; remove files left by an unclean exit.
        for entry in std::fs::read_dir(&staging)? {
            let path = entry?.path();
            if std::fs::symlink_metadata(&path)?.is_file() {
                std::fs::remove_file(path)?;
            }
        }
        Ok(Arc::new(Self {
            current: AsyncMutex::new(None),
            staging,
            received,
        }))
    }
    pub async fn start(&self, options: StartOptions) -> Result<Status> {
        if !(1..=30).contains(&options.minutes) {
            bail!("Expiry must be 1–30 minutes");
        }
        if options.files.is_empty() && !options.uploads && !options.text {
            bail!("Select files or enable receiving");
        }
        let interface = network::select(&options.interface)?;
        let files = open_files(&options.files)?;
        let mut current = self.current.lock().await;
        if current.as_ref().is_some_and(|s| s.alive()) {
            bail!("Stop the current share first");
        }
        if let Some(old) = current.as_ref() {
            old.closed.cancelled().await;
        }
        let listener =
            network::listener(&interface, PORT).context("Cannot bind Wi-Fi address/port")?;
        let session = Arc::new(Session::new(
            options,
            interface,
            files,
            self.staging.clone(),
            self.received.clone(),
        ));
        let server_session = session.clone();
        tokio::spawn(async move {
            let router = crate::web::router(server_session.clone());
            let watch_session = server_session.clone();
            let watchdog = tokio::spawn(async move {
                loop {
                    tokio::select! {
                        _ = watch_session.cancel.cancelled() => break,
                        _ = tokio::time::sleep(Duration::from_secs(1)) => {
                            if !watch_session.alive() || !network::interfaces().unwrap_or_default().contains(&watch_session.interface) {
                                watch_session.stop(); break;
                            }
                        }
                    }
                }
            });
            crate::transport::serve(listener, router, server_session.cancel.clone()).await;
            watchdog.abort();
            server_session.stop();
            server_session.closed.cancel();
        });
        let status = session.status();
        *current = Some(session);
        Ok(status)
    }
    pub async fn session(&self) -> Result<Arc<Session>> {
        self.current
            .lock()
            .await
            .clone()
            .context("No share running")
    }
    pub async fn stop(&self) {
        if let Some(s) = self.current.lock().await.take() {
            s.stop();
            s.closed.cancelled().await;
        }
    }
    pub async fn status(&self) -> serde_json::Value {
        match self.current.lock().await.as_ref() {
            Some(s) => serde_json::to_value(s.status()).unwrap(),
            None => serde_json::json!({"active":false,"pending":[],"files":[],"text":null,"qr":[]}),
        }
    }
}
