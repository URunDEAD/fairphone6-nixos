use crate::{
    history::History,
    model::{Request, Target},
    providers::{self, Outcome, Provider},
    BUS, PATH,
};
use anyhow::{Context, Result};
use futures_util::{future::join_all, StreamExt};
use serde::{Deserialize, Serialize};
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::process::Child;
use zbus::{fdo, message::Type, Connection, MatchRule};

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct Snapshot {
    pub revision: u64,
    pub request: Option<Request>,
    pub content_type: String,
    pub targets: Vec<Target>,
    pub recent: Vec<Target>,
    pub busy: bool,
    pub status: String,
}
struct State {
    snapshot: Snapshot,
    history: History,
    ui: Option<Child>,
    ui_owner: Option<String>,
    discovery_owned: bool,
}
pub struct Service {
    bus: Connection,
    state: Mutex<State>,
    providers: Vec<Box<dyn Provider>>,
    history_path: PathBuf,
    operation: tokio::sync::Mutex<()>,
}
fn failure(error: impl std::fmt::Display) -> fdo::Error {
    fdo::Error::Failed(error.to_string())
}
impl Service {
    pub fn new(bus: Connection, history_path: PathBuf) -> Arc<Self> {
        Arc::new(Self {
            bus,
            state: Mutex::new(State {
                snapshot: Snapshot::default(),
                history: History::load(&history_path),
                ui: None,
                ui_owner: None,
                discovery_owned: false,
            }),
            providers: providers::defaults(),
            history_path,
            operation: tokio::sync::Mutex::new(()),
        })
    }
    async fn collect(&self, r: &Request) -> Vec<Target> {
        join_all(self.providers.iter().map(|p| async {
            tokio::time::timeout(Duration::from_secs(3), p.targets(&self.bus, r))
                .await
                .ok()
                .and_then(Result::ok)
                .unwrap_or_default()
        }))
        .await
        .into_iter()
        .flatten()
        .collect()
    }
    pub async fn refresh(&self) {
        let snapshot = self.state.lock().unwrap().snapshot.clone();
        let Some(r) = snapshot.request else {
            return;
        };
        let targets = self.collect(&r).await;
        let mut s = self.state.lock().unwrap();
        if s.snapshot.revision == snapshot.revision && s.snapshot.request.is_some() {
            s.snapshot.recent = s.history.recent(&targets);
            s.snapshot.targets = targets;
        }
    }
    async fn stop_discovery(&self) {
        let owned = {
            let mut s = self.state.lock().unwrap();
            std::mem::take(&mut s.discovery_owned)
        };
        if owned {
            let _ = tokio::time::timeout(Duration::from_secs(2), async {
                let p = zbus::Proxy::new(
                    &self.bus,
                    "com.fairphone6.QuickShare1",
                    "/com/fairphone6/QuickShare1",
                    "com.fairphone6.QuickShare1",
                )
                .await?;
                p.call::<_, _, ()>("StopDiscovery", &()).await
            })
            .await;
        }
    }
    async fn reap_ui(&self) {
        let owner = self.state.lock().unwrap().ui_owner.clone();
        let owner_gone = if let Some(owner) = owner {
            !providers::available(&self.bus, &owner).await
        } else {
            false
        };
        let closed = {
            let mut s = self.state.lock().unwrap();
            let closed = !s.snapshot.busy
                && (owner_gone
                    || s.ui
                        .as_mut()
                        .is_some_and(|c| c.try_wait().ok().flatten().is_some()));
            if closed {
                s.ui = None;
                s.ui_owner = None;
                s.snapshot.request = None;
                s.snapshot.targets.clear();
                s.snapshot.recent.clear();
            }
            closed
        };
        if closed {
            self.stop_discovery().await;
        }
    }
    pub async fn background(self: Arc<Self>) {
        // Refresh periodically as well: recover from name-owner changes and lost signals.
        let rule = MatchRule::builder()
            .msg_type(Type::Signal)
            .interface("com.fairphone6.QuickShare1")
            .unwrap()
            .build();
        let mut signals = zbus::MessageStream::for_match_rule(rule, &self.bus, Some(32))
            .await
            .ok();
        let mut timer = tokio::time::interval(Duration::from_secs(2));
        loop {
            tokio::select! {
                _ = timer.tick() => {},
                _ = async { if let Some(stream) = &mut signals { stream.next().await; } else { std::future::pending::<()>().await; } } => {},
            }
            // Never reap a UI while Share is installing a new request.
            if let Ok(operation) = self.operation.try_lock() {
                self.reap_ui().await;
                drop(operation);
            }
            self.refresh().await;
        }
    }
}

impl Service {
    async fn attach(&self, owner: String) -> fdo::Result<()> {
        let _operation = self.operation.lock().await;
        let previous = self.state.lock().unwrap().ui_owner.clone();
        if let Some(previous) = previous {
            if previous != owner && providers::available(&self.bus, &previous).await {
                return Err(failure("Share sheet already open"));
            }
        }
        self.state.lock().unwrap().ui_owner = Some(owner);
        Ok(())
    }
    async fn share(&self, items: Vec<String>, mime: String, source_app: String) -> fdo::Result<()> {
        let _operation = self.operation.lock().await;
        self.reap_ui().await;
        let request = Request::new(items, mime, source_app).map_err(failure)?;
        {
            let mut s = self.state.lock().unwrap();
            if s.snapshot.request.is_some() {
                return Err(failure(
                    "A share is already open; finish or cancel it first",
                ));
            }
            if s.ui.is_none() && s.ui_owner.is_none() {
                let executable = std::env::var_os("FP6_SHARE_UI")
                    .map(PathBuf::from)
                    .unwrap_or_else(|| {
                        std::env::current_exe()
                            .unwrap()
                            .with_file_name("fp6-share-ui")
                    });
                s.ui = Some(
                    tokio::process::Command::new(executable)
                        .kill_on_drop(true)
                        .spawn()
                        .map_err(failure)?,
                );
            }
            s.snapshot.revision += 1;
            s.snapshot.content_type = request.content_type().into();
            s.snapshot.request = Some(request);
            s.snapshot.status = "Choose where to share".into();
            s.snapshot.targets.clear();
            s.snapshot.recent.clear();
        }
        // Active discovery is scoped to a user-opened sheet, with no receive visibility changes.
        let owned = tokio::time::timeout(Duration::from_secs(2), async {
            if !providers::available(&self.bus, "com.fairphone6.QuickShare1").await {
                return false;
            }
            let Ok(p) = zbus::Proxy::new(
                &self.bus,
                "com.fairphone6.QuickShare1",
                "/com/fairphone6/QuickShare1",
                "com.fairphone6.QuickShare1",
            )
            .await
            else {
                return false;
            };
            if p.get_property::<String>("State").await.unwrap_or_default() != "idle" {
                return false;
            }
            p.call::<_, _, ()>("StartDiscovery", &()).await.is_ok()
        })
        .await
        .unwrap_or(false);
        self.state.lock().unwrap().discovery_owned = owned;
        self.refresh().await;
        Ok(())
    }
    fn snapshot(&self) -> String {
        serde_json::to_string(&self.state.lock().unwrap().snapshot).unwrap()
    }
    async fn select(&self, revision: u64, id: String, option: String) -> fdo::Result<String> {
        let _operation = self.operation.lock().await;
        let request = {
            let mut s = self.state.lock().unwrap();
            if s.snapshot.revision != revision || s.snapshot.busy {
                return Err(failure("Stale or busy share"));
            }
            let r = s
                .snapshot
                .request
                .clone()
                .ok_or_else(|| failure("No active share"))?;
            s.snapshot.busy = true;
            r
        };
        let result: Result<(Target, Outcome)> = async {
            let targets = self.collect(&request).await;
            let target = targets
                .into_iter()
                .find(|t| t.id == id)
                .context("Target is no longer available")?;
            let provider = self
                .providers
                .iter()
                .find(|p| p.id() == target.provider)
                .context("Unknown provider")?;
            let outcome = tokio::time::timeout(
                Duration::from_secs(310),
                provider.activate(&self.bus, &request, &target, &option),
            )
            .await
            .context("Action timed out")??;
            Ok((target, outcome))
        }
        .await;
        let mut s = self.state.lock().unwrap();
        s.snapshot.busy = false;
        match result {
            Ok((target, outcome)) => {
                s.history.record(&target);
                let history_error = s.history.save(&self.history_path).err();
                s.snapshot.recent = s.history.recent(&s.snapshot.targets);
                s.snapshot.status = history_error
                    .map(|_| "Action started; recent history could not be saved".into())
                    .unwrap_or_else(|| "Action started".into());
                Ok(serde_json::to_string(&outcome).unwrap())
            }
            Err(error) => {
                s.snapshot.status = error.to_string();
                Err(failure(error))
            }
        }
    }
    async fn cancel(&self, revision: u64) -> fdo::Result<()> {
        let _operation = self.operation.lock().await;
        {
            let mut s = self.state.lock().unwrap();
            if s.snapshot.revision != revision {
                return Err(failure("Stale share"));
            }
            if s.snapshot.busy {
                return Err(failure("Wait for the current action"));
            }
            s.snapshot.request = None;
            s.snapshot.targets.clear();
            s.snapshot.recent.clear();
            s.snapshot.status = "Share canceled".into();
        }
        self.stop_discovery().await;
        Ok(())
    }
    fn clear_history(&self) -> fdo::Result<()> {
        let mut s = self.state.lock().unwrap();
        s.history.ids.clear();
        s.snapshot.recent.clear();
        s.history.save(&self.history_path).map_err(failure)
    }
    async fn quick_job(&self, id: String) -> fdo::Result<String> {
        if id.len() > 128 {
            return Err(failure("Invalid transfer ID"));
        }
        providers::quick_status(&self.bus, &id)
            .await
            .map_err(failure)
    }
    async fn cancel_job(&self, provider: String, id: String) -> fdo::Result<()> {
        if id.len() > 128 {
            return Err(failure("Invalid job ID"));
        }
        providers::cancel_job(&self.bus, &provider, &id)
            .await
            .map_err(failure)
    }
    async fn ocr_job(&self, id: String) -> fdo::Result<String> {
        if id.len() > 128 {
            return Err(failure("Invalid OCR job"));
        }
        providers::ocr_status(&self.bus, &id).await.map_err(failure)
    }
}
pub async fn run() -> Result<()> {
    let bus = zbus::connection::Builder::session()?
        .method_timeout(Duration::from_secs(5))
        .build()
        .await?;
    let state_dir = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/state")
        });
    let service = Service::new(bus.clone(), state_dir.join("fp6-share/recent.json"));
    // Arc<...> is wrapped so the background task and interface share the same state.
    bus.object_server()
        .at(PATH, Shared(service.clone()))
        .await?;
    bus.request_name(BUS).await?;
    tokio::spawn(service.clone().background());
    let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    tokio::select! { result = tokio::signal::ctrl_c() => result?, _ = terminate.recv() => {} }

    service.stop_discovery().await;
    Ok(())
}
// zbus interfaces cannot be directly served through Arc<T>.
pub struct Shared(pub Arc<Service>);
#[zbus::interface(name = "com.fairphone6.Share1")]
impl Shared {
    async fn attach(&self, #[zbus(header)] header: zbus::message::Header<'_>) -> fdo::Result<()> {
        let owner = header
            .sender()
            .ok_or_else(|| failure("Missing sender"))?
            .to_string();
        self.0.attach(owner).await
    }

    async fn share(&self, items: Vec<String>, mime: String, source_app: String) -> fdo::Result<()> {
        self.0.share(items, mime, source_app).await
    }
    fn snapshot(&self) -> String {
        self.0.snapshot()
    }
    async fn select(&self, revision: u64, id: String, option: String) -> fdo::Result<String> {
        self.0.select(revision, id, option).await
    }
    async fn cancel(&self, revision: u64) -> fdo::Result<()> {
        self.0.cancel(revision).await
    }
    fn clear_history(&self) -> fdo::Result<()> {
        self.0.clear_history()
    }
    async fn quick_job(&self, id: String) -> fdo::Result<String> {
        self.0.quick_job(id).await
    }
    async fn cancel_job(&self, provider: String, id: String) -> fdo::Result<()> {
        self.0.cancel_job(provider, id).await
    }
    async fn ocr_job(&self, id: String) -> fdo::Result<String> {
        self.0.ocr_job(id).await
    }
}
