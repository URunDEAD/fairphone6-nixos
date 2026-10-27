use crate::{
    client::{BUS_NAME, PATH},
    model::*,
};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::Mutex;
use zbus::{fdo, message::Header, object_server::SignalEmitter, Connection};

pub const SHELL: &str = "com.fairphone6.CallsShell1";
pub const PLATFORM: &str = "com.fairphone6.CallsPlatform1";
pub const NOTIFY: &str = "com.fairphone6.Notifications1";
pub type Shared = Arc<Mutex<Model>>;
fn error(message: &str) -> fdo::Error {
    fdo::Error::Failed(message.into())
}
fn sender(header: &Header<'_>) -> fdo::Result<String> {
    header
        .sender()
        .map(ToString::to_string)
        .ok_or_else(|| fdo::Error::AccessDenied("missing sender".into()))
}
async fn controller(connection: &Connection, header: &Header<'_>) -> fdo::Result<()> {
    let bus = fdo::DBusProxy::new(connection).await?;
    let owner = bus.get_name_owner(SHELL.try_into().unwrap()).await?;
    if sender(header)? != owner.as_str() {
        return Err(fdo::Error::AccessDenied("shell only".into()));
    }
    Ok(())
}
fn json(value: &impl serde::Serialize) -> fdo::Result<String> {
    serde_json::to_string(value).map_err(|_| error("serialization failed"))
}
pub struct Service {
    pub model: Shared,
    pub platform_ready: Arc<std::sync::atomic::AtomicBool>,
    start: Instant,
}
impl Service {
    pub fn new(model: Shared) -> Self {
        Self {
            model,
            platform_ready: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            start: Instant::now(),
        }
    }
    fn now(&self) -> u64 {
        self.start.elapsed().as_secs()
    }
}
#[zbus::interface(name = "com.fairphone6.Calls1")]
impl Service {
    async fn report_incoming(
        &self,
        app: &str,
        display_name: &str,
        #[zbus(header)] header: Header<'_>,
    ) -> fdo::Result<u64> {
        self.model
            .lock()
            .await
            .report(&sender(&header)?, app, display_name, true, self.now())
            .map_err(error)
    }
    async fn report_outgoing(
        &self,
        app: &str,
        display_name: &str,
        #[zbus(header)] header: Header<'_>,
    ) -> fdo::Result<u64> {
        self.model
            .lock()
            .await
            .report(&sender(&header)?, app, display_name, false, self.now())
            .map_err(error)
    }
    async fn report_connected(
        &self,
        id: u64,
        #[zbus(header)] header: Header<'_>,
    ) -> fdo::Result<()> {
        self.model
            .lock()
            .await
            .connected(&sender(&header)?, id, self.now())
            .map_err(error)
    }
    async fn report_ended(&self, id: u64, #[zbus(header)] header: Header<'_>) -> fdo::Result<()> {
        let mut model = self.model.lock().await;
        model.owned(&sender(&header)?, id).map_err(error)?;
        model.end(id, "remote_end", self.now());
        Ok(())
    }
    async fn calls(&self, #[zbus(header)] header: Header<'_>) -> fdo::Result<String> {
        let owner = sender(&header)?;
        json(
            &self
                .model
                .lock()
                .await
                .calls
                .values()
                .filter(|c| c.owner == owner)
                .collect::<Vec<_>>(),
        )
    }
    async fn commands(&self, id: u64, #[zbus(header)] header: Header<'_>) -> fdo::Result<String> {
        json(
            &self
                .model
                .lock()
                .await
                .owned(&sender(&header)?, id)
                .map_err(error)?
                .pending,
        )
    }
    async fn complete(
        &self,
        id: u64,
        serial: u64,
        success: bool,
        #[zbus(header)] header: Header<'_>,
    ) -> fdo::Result<()> {
        self.model
            .lock()
            .await
            .complete(&sender(&header)?, id, serial, success, self.now())
            .map_err(error)
    }
    async fn snapshot(
        &self,
        #[zbus(connection)] connection: &Connection,
        #[zbus(header)] header: Header<'_>,
    ) -> fdo::Result<String> {
        controller(connection, &header).await?;
        json(
            &self.model.lock().await.snapshot(
                self.platform_ready
                    .load(std::sync::atomic::Ordering::Relaxed),
            ),
        )
    }
    async fn control(
        &self,
        id: u64,
        action: &str,
        #[zbus(connection)] connection: &Connection,
        #[zbus(header)] header: Header<'_>,
    ) -> fdo::Result<u64> {
        controller(connection, &header).await?;
        if action.len() > 128 {
            return Err(error("invalid action"));
        }
        let action: Action = serde_json::from_str(action).map_err(|_| error("invalid action"))?;
        self.model
            .lock()
            .await
            .request(id, action, self.now())
            .map_err(error)
    }
    async fn history(
        &self,
        #[zbus(connection)] connection: &Connection,
        #[zbus(header)] header: Header<'_>,
    ) -> fdo::Result<String> {
        controller(connection, &header).await?;
        json(&self.model.lock().await.history)
    }
    async fn clear_history(
        &self,
        #[zbus(connection)] connection: &Connection,
        #[zbus(header)] header: Header<'_>,
    ) -> fdo::Result<()> {
        controller(connection, &header).await?;
        let mut model = self.model.lock().await;
        model.history.clear();
        model.revision += 1;
        Ok(())
    }
    async fn set_proximity(
        &self,
        near: bool,
        #[zbus(connection)] connection: &Connection,
        #[zbus(header)] header: Header<'_>,
    ) -> fdo::Result<()> {
        controller(connection, &header).await?;
        let mut model = self.model.lock().await;
        if model.near != near {
            model.near = near;
            model.revision += 1;
        }
        Ok(())
    }
    #[zbus(signal)]
    pub async fn changed(emitter: &SignalEmitter<'_>, revision: u64) -> zbus::Result<()>;
}
/// Assumed fp6-notifyd shim contract. A missing/slow/malformed reply is silent.
/// The category is fixed; a provider cannot claim a favourite or emergency exception.
pub async fn should_alert(connection: &Connection) -> Alert {
    let request = async {
        let proxy =
            zbus::Proxy::new(connection, NOTIFY, "/com/fairphone6/Notifications1", NOTIFY).await?;
        proxy
            .call::<_, _, (bool, bool)>("ShouldAlert", &("internet-call",))
            .await
    };
    match tokio::time::timeout(Duration::from_millis(300), request).await {
        Ok(Ok((sound, vibration))) => Alert { sound, vibration },
        _ => Alert::default(),
    }
}
/// Platform must replace desired effects atomically and release them after lease_ms
/// without a renewal, including on broker crash. No caller identity is sent here.
pub async fn apply_effects(connection: &Connection, effects: &Effects) -> bool {
    let request = async {
        let proxy = zbus::Proxy::new(
            connection,
            PLATFORM,
            "/com/fairphone6/CallsPlatform1",
            PLATFORM,
        )
        .await?;
        proxy
            .call::<_, _, ()>("Apply", &(serde_json::to_string(effects).unwrap(), 2500u32))
            .await
    };
    matches!(
        tokio::time::timeout(Duration::from_millis(300), request).await,
        Ok(Ok(()))
    )
}
/// Run on the caller's session bus. Demo mode is explicit and never accesses oFono.
pub async fn run(demo: bool) -> std::result::Result<(), Box<dyn std::error::Error>> {
    let model = Arc::new(Mutex::new(Model::default()));
    if demo {
        model.lock().await.cellular = Cellular::Idle;
    }
    let service = Service::new(model.clone());
    let start = service.start;
    let ready = service.platform_ready.clone();
    let connection = zbus::connection::Builder::session()?
        .name(BUS_NAME)?
        .serve_at(PATH, service)?
        .build()
        .await?;
    let observer = if demo {
        None
    } else {
        let model = model.clone();
        Some(tokio::spawn(async move {
            crate::ofono::observe(model, start).await;
        }))
    };
    let bus = fdo::DBusProxy::new(&connection).await?;
    let emitter = SignalEmitter::new(&connection, PATH)?;
    let mut interval = tokio::time::interval(Duration::from_millis(100));
    let mut last_maintenance = Instant::now() - Duration::from_secs(2);
    let mut revision = u64::MAX;
    let mut last_effects: Option<Effects> = None;
    let mut last_apply = Instant::now() - Duration::from_secs(2);
    let mut last_alert = Alert::default();
    let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => break,
            _ = terminate.recv() => break,
            _ = interval.tick() => {},
        }
        if last_maintenance.elapsed() >= Duration::from_secs(1) {
            let owners = bus
                .list_names()
                .await?
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>();
            {
                let mut m = model.lock().await;
                m.tick(start.elapsed().as_secs(), &owners);
                if !owners.iter().any(|owner| owner == SHELL) && m.near {
                    m.near = false;
                    m.revision += 1;
                }
            }
            last_alert = should_alert(&connection).await;
            last_maintenance = Instant::now();
        }
        let (effects, current_revision) = {
            let mut m = model.lock().await;
            let ids: Vec<_> = m.calls.keys().copied().collect();
            for id in ids {
                m.set_alert(id, last_alert);
            }
            (m.effects(), m.revision)
        };
        if last_effects.as_ref() != Some(&effects) || last_apply.elapsed() >= Duration::from_secs(1)
        {
            let available = apply_effects(&connection, &effects).await;
            if ready.swap(available, std::sync::atomic::Ordering::Relaxed) != available {
                model.lock().await.revision += 1;
            }
            last_effects = Some(effects);
            last_apply = Instant::now();
        }
        if revision != current_revision {
            Service::changed(&emitter, current_revision).await?;
            revision = current_revision;
        }
    }
    if let Some(observer) = observer {
        observer.abort();
    }
    let _ = apply_effects(&connection, &Effects::default()).await;
    Ok(())
}
