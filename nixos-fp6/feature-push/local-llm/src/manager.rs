use crate::{
    download::{self, Progress},
    plan_context, Model, Settings, DEFAULT_CTX,
};
use anyhow::{bail, Context, Result};
use serde::Serialize;
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
use tokio::{
    fs,
    net::TcpListener,
    process::{Child, Command},
    sync::{broadcast, Mutex, RwLock},
};

#[derive(Clone)]
pub struct Options {
    pub root: PathBuf,
    pub server: PathBuf,
    pub vulkan_server: Option<PathBuf>,
    pub meminfo: PathBuf,
    pub pressure: PathBuf,
    pub saver_file: PathBuf,
    pub reserve_mib: u64,
    pub start_timeout: Duration,
    pub check_power_profiles: bool,
}
impl Options {
    pub fn new(root: PathBuf, server: PathBuf, vulkan_server: Option<PathBuf>) -> Self {
        Self {
            root,
            server,
            vulkan_server,
            meminfo: "/proc/meminfo".into(),
            pressure: "/proc/pressure/memory".into(),
            saver_file: "/run/fp6-battery/saver".into(),
            reserve_mib: 512,
            start_timeout: Duration::from_secs(120),
            check_power_profiles: true,
        }
    }
}
pub struct Runtime {
    child: Option<Child>,
    port: u16,
    /// The context the running llama-server was started with.
    n_ctx: u32,
    pub reason: String,
}
pub struct Manager {
    pub options: Options,
    pub models: Vec<Model>,
    pub settings: RwLock<Settings>,
    runtime: Mutex<Runtime>,
    verified: Mutex<HashSet<String>>,
    pub progress: RwLock<HashMap<String, Progress>>,
    downloading: Mutex<Option<String>>,
    pub events: broadcast::Sender<Progress>,
    pub endpoint: RwLock<String>,
    active_requests: AtomicUsize,
    last_use: std::sync::Mutex<Instant>,
    profile_saver: AtomicBool,
    pub http: reqwest::Client,
    pub downloader: reqwest::Client,
}
#[derive(Serialize)]
pub struct Status {
    pub active: Option<String>,
    pub loaded: bool,
    pub pid: Option<u32>,
    pub endpoint: String,
    pub idle_seconds: u64,
    pub use_gpu: bool,
    pub gpu_available: bool,
    pub battery_saver: bool,
    pub battery_saver_override: bool,
    pub available_mib: u64,
    pub storage_bytes: u64,
    pub requests: usize,
    pub reason: String,
    /// Effective context window (tokens): the running server's, or else what
    /// a load would use now given the active model and available RAM (the
    /// requested size when it cannot be estimated). Clients size their
    /// requests by this.
    pub n_ctx: u32,
    /// The saved setting: 0 = automatic.
    pub context_tokens: u32,
    /// What the setting asks for (automatic resolved to DEFAULT_CTX).
    pub context_requested: u32,
}
#[derive(Clone, Debug)]
pub struct Resources {
    pub available_mib: u64,
    pub pressure: bool,
    pub saver: bool,
}

pub fn available_mib(text: &str) -> Result<u64> {
    for line in text.lines() {
        let mut parts = line.split_whitespace();
        if parts.next() == Some("MemAvailable:") {
            return Ok(parts
                .next()
                .context("MemAvailable missing value")?
                .parse::<u64>()?
                / 1024);
        }
    }
    bail!("cannot read MemAvailable")
}
pub fn memory_pressure(text: &str) -> bool {
    text.lines()
        .filter(|l| l.starts_with("full "))
        .flat_map(str::split_whitespace)
        .filter_map(|p| p.strip_prefix("avg10="))
        .any(|n| n.parse::<f64>().unwrap_or(100.0) >= 5.0)
}
impl Manager {
    pub async fn new(options: Options, models: Vec<Model>) -> Result<Arc<Self>> {
        fs::create_dir_all(options.root.join("models")).await?;
        let settings = Settings::load(&options.root)?;
        let (events, _) = broadcast::channel(64);
        Ok(Arc::new(Self {
            options,
            models,
            settings: RwLock::new(settings),
            runtime: Mutex::new(Runtime {
                child: None,
                port: 0,
                n_ctx: 0,
                reason: "unloaded".into(),
            }),
            verified: Mutex::new(HashSet::new()),
            progress: RwLock::new(HashMap::new()),
            downloading: Mutex::new(None),
            events,
            endpoint: RwLock::new(String::new()),
            active_requests: AtomicUsize::new(0),
            last_use: std::sync::Mutex::new(Instant::now()),
            profile_saver: AtomicBool::new(false),
            http: reqwest::Client::builder()
                .no_proxy()
                .redirect(reqwest::redirect::Policy::none())
                .connect_timeout(Duration::from_secs(2))
                .build()?,
            downloader: reqwest::Client::builder()
                .no_proxy()
                .https_only(true)
                .connect_timeout(Duration::from_secs(30))
                .read_timeout(Duration::from_secs(60))
                .build()?,
        }))
    }
    pub fn model(&self, id: &str) -> Result<Model> {
        self.models
            .iter()
            .find(|m| m.id == id)
            .cloned()
            .context("unknown catalogue model")
    }
    pub fn model_root(&self) -> PathBuf {
        self.options.root.join("models")
    }
    pub async fn resources(&self) -> Result<Resources> {
        let available_mib = available_mib(&fs::read_to_string(&self.options.meminfo).await?)?;
        let pressure = memory_pressure(
            &fs::read_to_string(&self.options.pressure)
                .await
                .unwrap_or_default(),
        );
        let saver = fs::read_to_string(&self.options.saver_file)
            .await
            .unwrap_or_default()
            .trim()
            == "1"
            || self.profile_saver.load(Ordering::Relaxed)
            || self.settings.read().await.battery_saver;
        Ok(Resources {
            available_mib,
            pressure,
            saver,
        })
    }
    pub async fn status(&self) -> Status {
        let settings = self.settings.read().await.clone();
        let resources = self.resources().await.ok();
        let mut rt = self.runtime.lock().await;
        if let Some(child) = rt.child.as_mut() {
            if child.try_wait().ok().flatten().is_some() {
                rt.child = None;
                rt.reason = "llama-server exited".into();
            }
        }
        let mut storage_bytes = 0;
        if let Ok(mut entries) = fs::read_dir(self.model_root()).await {
            while let Ok(Some(entry)) = entries.next_entry().await {
                if let Ok(meta) = entry.metadata().await {
                    if meta.is_file() {
                        storage_bytes += meta.len();
                    }
                }
            }
        }
        let requested = requested_ctx(&settings);
        let n_ctx = if rt.child.is_some() && rt.n_ctx > 0 {
            rt.n_ctx
        } else {
            match (
                settings.active.as_deref().and_then(|id| self.model(id).ok()),
                resources.as_ref(),
            ) {
                (Some(m), Some(r)) => {
                    plan_context(&m, settings.context_tokens, r.available_mib, self.options.reserve_mib)
                        .unwrap_or(crate::MIN_CTX)
                }
                _ => requested,
            }
        };
        Status {
            n_ctx,
            context_tokens: settings.context_tokens,
            context_requested: requested,
            active: settings.active,
            loaded: rt.child.is_some(),
            pid: rt.child.as_ref().and_then(Child::id),
            endpoint: self.endpoint.read().await.clone(),
            idle_seconds: settings.idle_seconds,
            use_gpu: settings.use_gpu,
            gpu_available: self.options.vulkan_server.is_some(),
            battery_saver: resources.as_ref().is_some_and(|r| r.saver),
            battery_saver_override: settings.battery_saver,
            available_mib: resources.map(|r| r.available_mib).unwrap_or(0),
            storage_bytes,
            requests: self.active_requests.load(Ordering::Relaxed),
            reason: rt.reason.clone(),
        }
    }
    pub async fn list_models(&self) -> serde_json::Value {
        let (active, requested) = {
            let s = self.settings.read().await;
            (s.active.clone(), requested_ctx(&s))
        };
        let progress = self.progress.read().await;
        let mut result = Vec::new();
        for m in &self.models {
            let installed = fs::metadata(m.path(&self.model_root()))
                .await
                .is_ok_and(|s| s.len() == m.bytes);
            let partial_bytes = fs::metadata(m.part(&self.model_root()))
                .await
                .map(|s| s.len())
                .unwrap_or(0);
            let mut value = serde_json::to_value(m).unwrap();
            let obj = value.as_object_mut().unwrap();
            obj.insert("installed".into(), installed.into());
            obj.insert("active".into(), (active.as_deref() == Some(&m.id)).into());
            obj.insert("partial_bytes".into(), partial_bytes.into());
            obj.insert("ram_mib_ctx".into(), m.ram_mib_at(requested).into());
            obj.insert(
                "progress".into(),
                serde_json::to_value(progress.get(&m.id)).unwrap(),
            );
            result.push(value);
        }
        result.into()
    }
    pub async fn set_active(&self, id: &str) -> Result<()> {
        let m = self.model(id)?;
        // Serialize with deletion and downloads so an active model cannot disappear mid-switch.
        let _operation = self.downloading.lock().await;
        download::verify(&m.path(&self.model_root()), &m)
            .await
            .context("download and verify this model first")?;
        let mut rt = self.runtime.lock().await;
        let mut settings = self.settings.write().await;
        let mut next = settings.clone();
        next.active = Some(id.into());
        next.save(&self.options.root)?;
        Self::stop(&mut rt, "model changed").await;
        *settings = next;
        self.verified.lock().await.insert(id.into());
        Ok(())
    }
    pub async fn configure(&self, idle: u64, gpu: bool, battery_saver: bool) -> Result<()> {
        if gpu && self.options.vulkan_server.is_none() {
            bail!("Vulkan package is not enabled by the system module");
        }
        let mut rt = self.runtime.lock().await;
        let mut settings = self.settings.write().await;
        let mut next = settings.clone();
        next.idle_seconds = idle;
        next.use_gpu = gpu;
        next.battery_saver = battery_saver;
        next.save(&self.options.root)?;
        if next.use_gpu != settings.use_gpu || battery_saver {
            Self::stop(&mut rt, "settings changed").await;
        }
        *settings = next;
        Ok(())
    }
    /// Persist the requested context (0 = automatic) and unload so the next
    /// request restarts llama-server with it.
    pub async fn set_context(&self, tokens: u32) -> Result<()> {
        let mut rt = self.runtime.lock().await;
        let mut settings = self.settings.write().await;
        let mut next = settings.clone();
        next.context_tokens = tokens;
        next.save(&self.options.root)?;
        if next.context_tokens != settings.context_tokens {
            Self::stop(&mut rt, "context size changed").await;
        }
        *settings = next;
        Ok(())
    }
    pub async fn delete(&self, id: &str) -> Result<()> {
        let m = self.model(id)?;
        let operation = self.downloading.lock().await;
        if operation.as_deref() == Some(id) {
            bail!("model is downloading; wait for completion or failure");
        }
        let mut rt = self.runtime.lock().await;
        let mut settings = self.settings.write().await;
        if settings.active.as_deref() == Some(id) {
            let mut next = settings.clone();
            next.active = None;
            next.save(&self.options.root)?;
            Self::stop(&mut rt, "active model deleted").await;
            *settings = next;
        }
        for path in [m.path(&self.model_root()), m.part(&self.model_root())] {
            match fs::remove_file(path).await {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(e.into()),
            }
        }
        self.verified.lock().await.remove(id);
        self.progress.write().await.remove(id);
        Ok(())
    }
    pub async fn download(self: &Arc<Self>, id: &str) -> Result<()> {
        let model = self.model(id)?;
        let mut downloading = self.downloading.lock().await;
        if downloading.is_some() {
            bail!("another model download is in progress");
        }
        if model.path(&self.model_root()).exists() {
            bail!("model is already installed; delete it before downloading again");
        }
        *downloading = Some(id.into());
        let event = Progress::new(&model, 0, "queued", "");
        self.progress.write().await.insert(id.into(), event.clone());
        let _ = self.events.send(event);
        let manager = self.clone();
        tokio::spawn(async move {
            // Progress updates are sent to the bus immediately; snapshots are also retained for late clients.
            let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
            let snapshots = manager.clone();
            let update = tokio::spawn(async move {
                while let Some(p) = rx.recv().await {
                    let p: Progress = p;
                    snapshots
                        .progress
                        .write()
                        .await
                        .insert(p.id.clone(), p.clone());
                    let _ = snapshots.events.send(p);
                }
            });
            let result = download::fetch(&model, &manager.model_root(), &manager.downloader, |p| {
                let _ = tx.send(p);
            })
            .await;
            drop(tx);
            let _ = update.await;
            if let Err(e) = result {
                let received = fs::metadata(model.part(&manager.model_root()))
                    .await
                    .map(|m| m.len())
                    .unwrap_or(0);
                let p = Progress::new(&model, received, "failed", &e.to_string());
                manager
                    .progress
                    .write()
                    .await
                    .insert(model.id.clone(), p.clone());
                let _ = manager.events.send(p);
            } else {
                manager.verified.lock().await.insert(model.id.clone());
            }
            *manager.downloading.lock().await = None;
        });
        Ok(())
    }
    async fn stop(rt: &mut Runtime, reason: &str) {
        if let Some(mut child) = rt.child.take() {
            let _ = child.kill().await;
            let _ = child.wait().await;
        }
        rt.reason = reason.into();
    }
    pub async fn unload(&self, reason: &str) {
        let mut rt = self.runtime.lock().await;
        Self::stop(&mut rt, reason).await;
    }
    // The returned guard is retained by the response stream, including after HTTP headers arrive.
    pub async fn acquire(self: &Arc<Self>) -> Result<(String, RequestGuard)> {
        let mut rt = self.runtime.lock().await;
        let settings = self.settings.read().await.clone();
        let model = self.model(
            settings
                .active
                .as_deref()
                .context("choose a downloaded model first")?,
        )?;
        let resources = match self.resources().await {
            Ok(resources) => resources,
            Err(e) => {
                Self::stop(&mut rt, "memory information unavailable").await;
                return Err(e);
            }
        };
        if resources.saver
            || resources.pressure
            || resources.available_mib < self.options.reserve_mib
        {
            Self::stop(&mut rt, "battery saver or memory pressure").await;
            bail!("local inference paused: battery saver or memory pressure");
        }
        if let Some(child) = rt.child.as_mut() {
            if child.try_wait()?.is_some() {
                rt.child = None;
            }
        }
        if rt.child.is_none() {
            // RAM admission: the requested context, halved until it fits.
            let Some(n_ctx) = plan_context(
                &model,
                settings.context_tokens,
                resources.available_mib,
                self.options.reserve_mib,
            ) else {
                bail!(
                    "insufficient available RAM: need {} MiB including reserve",
                    model.ram_mib_at(crate::MIN_CTX) + self.options.reserve_mib
                );
            };
            rt.n_ctx = n_ctx;
            let verified = self.verified.lock().await.contains(&model.id);
            if !verified {
                download::verify(&model.path(&self.model_root()), &model).await?;
                self.verified.lock().await.insert(model.id.clone());
            }
            // llama.cpp cannot accept an inherited socket; reserve an ephemeral loopback port, then
            // release it immediately before spawn. A bind race is detected by child/readiness checks.
            let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).await?;
            rt.port = listener.local_addr()?.port();
            drop(listener);
            let server = if settings.use_gpu {
                self.options
                    .vulkan_server
                    .as_ref()
                    .context("Vulkan unavailable")?
            } else {
                &self.options.server
            };
            let mut cmd = Command::new(server);
            cmd.env_clear();
            for key in [
                "PATH",
                "HOME",
                "XDG_RUNTIME_DIR",
                "XDG_DATA_DIRS",
                "VK_ICD_FILENAMES",
                "VK_DRIVER_FILES",
            ] {
                if let Some(v) = std::env::var_os(key) {
                    cmd.env(key, v);
                }
            }
            cmd.args([
                "--host",
                "127.0.0.1",
                "--port",
                &rt.port.to_string(),
                "--model",
            ])
            .arg(model.path(&self.model_root()))
            .args([
                "--alias",
                &model.id,
                "--jinja",
                "--offline",
                "--ctx-size",
                &n_ctx.to_string(),
                "--parallel",
                "1",
                "--threads",
                "4",
                "--threads-batch",
                "8",
                "--batch-size",
                "256",
                "--ubatch-size",
                "128",
                "--n-gpu-layers",
                if settings.use_gpu { "99" } else { "0" },
                "--no-webui",
            ])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::inherit())
            .kill_on_drop(true);
            rt.child = Some(cmd.spawn().context("start llama-server")?);
            rt.reason = "loading".into();
            let start = Instant::now();
            loop {
                let r = match self.resources().await {
                    Ok(resources) => resources,
                    Err(e) => {
                        Self::stop(&mut rt, "memory information unavailable during load").await;
                        return Err(e);
                    }
                };
                if r.saver || r.pressure || r.available_mib < self.options.reserve_mib {
                    Self::stop(&mut rt, "memory pressure or battery saver during load").await;
                    bail!("resource pressure during model load");
                }
                if rt.child.as_mut().unwrap().try_wait()?.is_some() {
                    Self::stop(&mut rt, "llama-server failed to load").await;
                    bail!("llama-server exited during startup; see user journal");
                }
                if self
                    .http
                    .get(format!("http://127.0.0.1:{}/health", rt.port))
                    .timeout(Duration::from_secs(1))
                    .send()
                    .await
                    .is_ok_and(|r| r.status().is_success())
                {
                    break;
                }
                if start.elapsed() > self.options.start_timeout {
                    Self::stop(&mut rt, "startup timeout").await;
                    bail!("llama-server startup timed out");
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        }
        rt.reason = "ready".into();
        *self.last_use.lock().unwrap() = Instant::now();
        self.active_requests.fetch_add(1, Ordering::SeqCst);
        Ok((
            format!("http://127.0.0.1:{}", rt.port),
            RequestGuard(self.clone()),
        ))
    }
    pub async fn maintain(&self) {
        if self.options.check_power_profiles {
            self.profile_saver
                .store(power_saver().await, Ordering::Relaxed);
        }
        let settings = self.settings.read().await.clone();
        let resources = self.resources().await;
        let mut rt = self.runtime.lock().await;
        if rt.child.is_none() {
            return;
        }
        match resources {
            Err(_) => Self::stop(&mut rt, "memory information unavailable").await,
            Ok(r) if r.saver || r.pressure || r.available_mib < self.options.reserve_mib => {
                Self::stop(&mut rt, "battery saver or memory pressure").await
            }
            _ if self.active_requests.load(Ordering::SeqCst) == 0
                && self.last_use.lock().unwrap().elapsed().as_secs() >= settings.idle_seconds =>
            {
                Self::stop(&mut rt, "idle timeout").await
            }
            _ => {}
        }
    }
}
fn requested_ctx(settings: &Settings) -> u32 {
    if settings.context_tokens == 0 {
        DEFAULT_CTX
    } else {
        settings.context_tokens
    }
}
pub struct RequestGuard(Arc<Manager>);
impl Drop for RequestGuard {
    fn drop(&mut self) {
        // Timestamp before decrement; maintain cannot observe zero requests with an old timestamp.
        *self.0.last_use.lock().unwrap() = Instant::now();
        self.0.active_requests.fetch_sub(1, Ordering::SeqCst);
    }
}
async fn power_saver() -> bool {
    tokio::time::timeout(Duration::from_secs(1), async {
        let connection = zbus::Connection::system().await.ok()?;
        for (name, path) in [
            (
                "org.freedesktop.UPower.PowerProfiles",
                "/org/freedesktop/UPower/PowerProfiles",
            ),
            ("net.hadess.PowerProfiles", "/net/hadess/PowerProfiles"),
        ] {
            if let Ok(proxy) = zbus::Proxy::new(&connection, name, path, name).await {
                if let Ok(profile) = proxy.get_property::<String>("ActiveProfile").await {
                    return Some(profile == "power-saver");
                }
            }
        }
        None
    })
    .await
    .ok()
    .flatten()
    .unwrap_or(false)
}
