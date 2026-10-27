#![forbid(unsafe_code)]
use anyhow::Result;
use clap::Parser;
use fp6_quickshare::{Transfer, BUS, PATH};
use futures_util::StreamExt;
use quickshare_core::{
    ChannelAction, ChannelDirection, ChannelMessage, Config, Core, Device, DeviceEvent, State,
    TransferType,
};
use std::{
    collections::{BTreeMap, HashMap},
    path::PathBuf,
    sync::Arc,
    time::Duration,
};
use tokio::{sync::Mutex, time::Instant};
use zbus::{interface, object_server::SignalEmitter};

#[derive(Parser)]
struct Args {
    #[arg(long, default_value = "Fairphone 6")]
    device_name: String,
    #[arg(long, default_value_t = quickshare_core::PORT)]
    port: u16,
    #[arg(long, value_delimiter = ',', default_value = "wlan0")]
    interfaces: Vec<String>,
    #[arg(long)]
    download_dir: Option<PathBuf>,
    #[arg(long, default_value_t = 1024 * 1024 * 1024)]
    max_bytes: u64,
}
struct Data {
    visibility: String,
    deadline: Option<Instant>,
    name: String,
    discovering: bool,
    devices: BTreeMap<String, Device>,
    transfers: BTreeMap<String, Transfer>,
    pending: HashMap<String, Instant>,
}
struct Service {
    core: Arc<Core>,
    data: Arc<Mutex<Data>>,
}
fn failure(e: impl std::fmt::Display) -> zbus::fdo::Error {
    zbus::fdo::Error::Failed(e.to_string())
}

#[interface(name = "com.fairphone6.QuickShare1")]
impl Service {
    #[zbus(property)]
    async fn visibility(&self) -> String {
        self.data.lock().await.visibility.clone()
    }
    #[zbus(property)]
    async fn set_visibility(&self, value: &str) -> zbus::fdo::Result<()> {
        if !matches!(value, "off" | "everyone-for-10-min" | "everyone") {
            return Err(zbus::fdo::Error::InvalidArgs("use off, everyone-for-10-min, or everyone; contacts requires Google identity and is unsupported".into()));
        }
        let mut data = self.data.lock().await;
        self.core.set_visible(value != "off").map_err(failure)?;
        data.visibility = value.into();
        data.deadline =
            (value == "everyone-for-10-min").then(|| Instant::now() + Duration::from_secs(600));
        if value == "off" {
            for id in data.pending.keys() {
                let _ = self.core.action(id, ChannelAction::RejectTransfer);
            }
            data.pending.clear();
        }
        Ok(())
    }
    #[zbus(property)]
    async fn device_name(&self) -> String {
        self.data.lock().await.name.clone()
    }
    #[zbus(property)]
    async fn set_device_name(&self, value: &str) -> zbus::fdo::Result<()> {
        self.core.set_name(value).map_err(failure)?;
        self.data.lock().await.name = value.into();
        Ok(())
    }
    #[zbus(property)]
    async fn state(&self) -> String {
        let data = self.data.lock().await;
        if data.transfers.values().any(|t| !is_terminal(&t.status)) {
            "transferring"
        } else if data.discovering {
            "discovering"
        } else {
            "idle"
        }
        .into()
    }
    async fn start_discovery(
        &self,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> zbus::fdo::Result<()> {
        self.core.start_discovery().await.map_err(failure)?;
        self.data.lock().await.discovering = true;
        self.state_changed(&emitter).await.map_err(failure)?;
        Ok(())
    }
    async fn stop_discovery(
        &self,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> zbus::fdo::Result<()> {
        self.core.stop_discovery().await.map_err(failure)?;
        let ids = {
            let mut data = self.data.lock().await;
            data.discovering = false;
            std::mem::take(&mut data.devices)
                .into_keys()
                .collect::<Vec<_>>()
        };
        for id in ids {
            Self::device_lost(&emitter, &id).await.map_err(failure)?;
        }
        self.state_changed(&emitter).await.map_err(failure)?;
        Ok(())
    }
    async fn snapshot(&self) -> String {
        let d = self.data.lock().await;
        serde_json::json!({"visibility": d.visibility, "deviceName": d.name,
            "devices": d.devices.values().collect::<Vec<_>>(),
            "transfers": d.transfers.values().collect::<Vec<_>>() })
        .to_string()
    }
    async fn list_devices(&self) -> String {
        serde_json::to_string(&self.data.lock().await.devices.values().collect::<Vec<_>>()).unwrap()
    }
    async fn list_transfers(&self) -> String {
        serde_json::to_string(
            &self
                .data
                .lock()
                .await
                .transfers
                .values()
                .collect::<Vec<_>>(),
        )
        .unwrap()
    }
    async fn send(&self, target: &str, items: Vec<String>) -> zbus::fdo::Result<String> {
        let payload = quickshare_core::parse_payload(&items).map_err(failure)?;
        let (device, name) = {
            let data = self.data.lock().await;
            if data
                .transfers
                .values()
                .filter(|t| !is_terminal(&t.status))
                .count()
                >= 8
            {
                return Err(failure("too many active transfers"));
            }
            (
                data.devices
                    .get(target)
                    .cloned()
                    .ok_or_else(|| failure("unknown target; discover devices first"))?,
                data.name.clone(),
            )
        };
        let id = self
            .core
            .send(&device, payload, name)
            .await
            .map_err(failure)?;
        let mut data = self.data.lock().await;
        data.transfers.entry(id.clone()).or_insert(Transfer {
            id: id.clone(),
            direction: "outbound".into(),
            sender: device.name,
            status: "connecting".into(),
            ..Default::default()
        });
        Ok(id)
    }
    async fn accept(&self, transfer_id: &str) -> zbus::fdo::Result<()> {
        self.decide(transfer_id, ChannelAction::AcceptTransfer)
            .await
    }
    async fn reject(&self, transfer_id: &str) -> zbus::fdo::Result<()> {
        self.decide(transfer_id, ChannelAction::RejectTransfer)
            .await
    }
    async fn cancel(&self, transfer_id: &str) -> zbus::fdo::Result<()> {
        let data = self.data.lock().await;
        let t = data
            .transfers
            .get(transfer_id)
            .ok_or_else(|| failure("unknown transfer"))?;
        if is_terminal(&t.status) {
            return Err(failure("transfer has finished"));
        }
        self.core
            .action(transfer_id, ChannelAction::CancelTransfer)
            .map_err(failure)
    }
    #[zbus(signal)]
    async fn device_found(emitter: &SignalEmitter<'_>, id: &str, name: &str) -> zbus::Result<()>;
    #[zbus(signal)]
    async fn device_lost(emitter: &SignalEmitter<'_>, id: &str) -> zbus::Result<()>;
    #[zbus(signal)]
    async fn incoming_request(
        emitter: &SignalEmitter<'_>,
        transfer_id: &str,
        sender: &str,
        files: Vec<String>,
        pin: &str,
    ) -> zbus::Result<()>;
    #[zbus(signal)]
    async fn progress(
        emitter: &SignalEmitter<'_>,
        transfer_id: &str,
        bytes: u64,
        total: u64,
    ) -> zbus::Result<()>;
    #[zbus(signal)]
    async fn finished(
        emitter: &SignalEmitter<'_>,
        transfer_id: &str,
        status: &str,
    ) -> zbus::Result<()>;
}
impl Service {
    async fn decide(&self, id: &str, action: ChannelAction) -> zbus::fdo::Result<()> {
        let mut data = self.data.lock().await;
        if !self.core.is_visible() || !data.pending.contains_key(id) {
            return Err(failure(
                "transfer is not awaiting consent or visibility is off",
            ));
        }
        self.core.action(id, action.clone()).map_err(failure)?;
        data.pending.remove(id);
        if let Some(t) = data.transfers.get_mut(id) {
            t.status = if action == ChannelAction::AcceptTransfer {
                "accepting"
            } else {
                "declining"
            }
            .into();
        }
        Ok(())
    }
}
fn expire_visibility(core: &Core, data: &mut Data) -> bool {
    if data
        .deadline
        .is_some_and(|deadline| deadline <= Instant::now())
    {
        let _ = core.set_visible(false);
        data.visibility = "off".into();
        data.deadline = None;
        for id in data.pending.keys() {
            let _ = core.action(id, ChannelAction::RejectTransfer);
        }
        data.pending.clear();
        true
    } else {
        false
    }
}
fn is_terminal(s: &str) -> bool {
    matches!(s, "finished" | "failed" | "cancelled" | "rejected")
}

async fn handle_message(
    service: &Service,
    emitter: &SignalEmitter<'_>,
    message: ChannelMessage,
    connection: &zbus::Connection,
) -> Result<()> {
    if message.direction != ChannelDirection::LibToFront {
        return Ok(());
    }
    let Some(state) = message.state else {
        return Ok(());
    };
    let mut data = service.data.lock().await;
    let inbound = message.rtype == Some(TransferType::Inbound);
    let transfer = data
        .transfers
        .entry(message.id.clone())
        .or_insert_with(|| Transfer {
            id: message.id.clone(),
            direction: if inbound { "inbound" } else { "outbound" }.into(),
            ..Default::default()
        });
    if is_terminal(&transfer.status) {
        return Ok(());
    }
    let previous = transfer.status.clone();
    transfer.status = match state {
        State::WaitingForUserConsent if inbound => "waiting",
        State::Finished => "finished",
        State::Rejected => "rejected",
        State::Cancelled => "cancelled",
        State::Disconnected => "failed",
        State::ReceivingFiles => "receiving",
        State::SendingFiles => "sending",
        _ => "connecting",
    }
    .into();
    if let Some(meta) = message.meta {
        if let Some(source) = meta.source {
            transfer.sender = source.name;
        }
        if let Some(files) = meta.files {
            transfer.files = files;
        }
        if let Some(pin) = meta.pin_code {
            transfer.pin = pin;
        }
        transfer.total = meta.total_bytes;
        transfer.bytes = meta.ack_bytes;
        if let Some(directory) = meta.destination {
            transfer.directory = directory;
        }
        if let Some(text) = meta.text_payload {
            transfer.text = text;
        }
        transfer.is_url = matches!(meta.text_type, Some(quickshare_core::rqs_text_type::Url));
    }
    let transfer = transfer.clone();
    let incoming = inbound && transfer.status == "waiting" && previous != "waiting";
    if incoming {
        if !service.core.is_visible() {
            let _ = service
                .core
                .action(&transfer.id, ChannelAction::RejectTransfer);
            return Ok(());
        }
        data.pending.insert(
            transfer.id.clone(),
            Instant::now() + Duration::from_secs(60),
        );
    }
    if is_terminal(&transfer.status) {
        data.pending.remove(&transfer.id);
    }
    // Bounded in-memory history; no payloads or device history are persisted.
    while data.transfers.len() > 128 {
        let candidate = data
            .transfers
            .iter()
            .find(|(id, t)| is_terminal(&t.status) && **id != transfer.id)
            .map(|(id, _)| id.clone());
        if let Some(id) = candidate {
            data.transfers.remove(&id);
        } else {
            break;
        }
    }
    drop(data);
    if incoming {
        Service::incoming_request(
            emitter,
            &transfer.id,
            &transfer.sender,
            transfer.files.clone(),
            &transfer.pin,
        )
        .await?;
        if let Ok(notification) = notify(
            connection,
            &format!("Quick Share request from {}", plain(&transfer.sender)),
            &format!(
                "Open Quick Share to compare PIN {} and accept or decline.",
                transfer.pin
            ),
            vec!["default", "Review"],
        )
        .await
        {
            notification_action(connection.clone(), notification, transfer.id.clone());
        }
    }
    Service::progress(emitter, &transfer.id, transfer.bytes, transfer.total).await?;
    if is_terminal(&transfer.status) {
        Service::finished(emitter, &transfer.id, &transfer.status).await?;
        if transfer.direction == "inbound" && transfer.status == "finished" {
            let actions = if transfer.text.is_empty() {
                vec!["open", "Open"]
            } else if transfer.is_url {
                vec!["copy", "Copy", "open", "Open"]
            } else {
                vec!["copy", "Copy"]
            };
            if let Ok(notification) = notify(
                connection,
                "Quick Share received",
                &plain(if transfer.text.is_empty() {
                    &transfer.sender
                } else {
                    &transfer.text
                }),
                actions,
            )
            .await
            {
                notification_action(connection.clone(), notification, transfer.id.clone());
            }
        }
    }
    service.state_changed(emitter).await?;
    Ok(())
}
fn plain(text: &str) -> String {
    text.chars()
        .take(200)
        .filter(|c| !c.is_control())
        .collect::<String>()
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}
async fn notify(
    connection: &zbus::Connection,
    title: &str,
    body: &str,
    actions: Vec<&str>,
) -> Result<u32> {
    let proxy = zbus::Proxy::new(
        connection,
        "org.freedesktop.Notifications",
        "/org/freedesktop/Notifications",
        "org.freedesktop.Notifications",
    )
    .await?;
    Ok(tokio::time::timeout(
        Duration::from_secs(3),
        proxy.call(
            "Notify",
            &(
                "Quick Share",
                0u32,
                "folder-download",
                title,
                body,
                actions,
                HashMap::<String, zbus::zvariant::Value>::new(),
                -1i32,
            ),
        ),
    )
    .await??)
}
fn notification_action(connection: zbus::Connection, notification: u32, transfer_id: String) {
    tokio::spawn(async move {
        let Ok(proxy) = zbus::Proxy::new(
            &connection,
            "org.freedesktop.Notifications",
            "/org/freedesktop/Notifications",
            "org.freedesktop.Notifications",
        )
        .await
        else {
            return;
        };
        let Ok(mut stream) = proxy.receive_signal("ActionInvoked").await else {
            return;
        };
        let wait = async {
            while let Some(message) = stream.next().await {
                if let Ok((id, action)) = message.body().deserialize::<(u32, String)>() {
                    if id == notification && matches!(action.as_str(), "copy" | "open" | "default")
                    {
                        let mut command = tokio::process::Command::new("fp6-quickshare-app");
                        command.args(["--show-transfer", &transfer_id]);
                        if action == "copy" {
                            command.arg("--copy");
                        } else if action == "open" {
                            command.arg("--open");
                        }
                        let _ = command.spawn();
                        break;
                    }
                }
            }
        };
        let _ = tokio::time::timeout(Duration::from_secs(600), wait).await;
    });
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    let directory = match args.download_dir {
        Some(path) => path,
        None => {
            PathBuf::from(std::env::var_os("HOME").ok_or_else(|| anyhow::anyhow!("HOME is unset"))?)
                .join("Downloads/Quick Share")
        }
    };
    let core = Core::start(Config {
        name: args.device_name.clone(),
        directory,
        port: args.port,
        interfaces: args.interfaces,
        max_bytes: args.max_bytes,
    })
    .await?;
    let data = Arc::new(Mutex::new(Data {
        visibility: "off".into(),
        deadline: None,
        name: args.device_name,
        discovering: false,
        devices: BTreeMap::new(),
        transfers: BTreeMap::new(),
        pending: HashMap::new(),
    }));
    let mut events = core.events.subscribe();
    let mut devices = core.devices.subscribe();
    let connection = zbus::connection::Builder::session()?
        .name(BUS)?
        .serve_at(
            PATH,
            Service {
                core: core.clone(),
                data: data.clone(),
            },
        )?
        .build()
        .await?;
    let service = connection
        .object_server()
        .interface::<_, Service>(PATH)
        .await?;
    let mut tick = tokio::time::interval(Duration::from_secs(1));
    let mut sigterm = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => break,
            _ = sigterm.recv() => break,
            message = events.recv() => match message {
                Ok(message) => { if let Err(error) = handle_message(&*service.get().await, service.signal_emitter(), message, &connection).await { eprintln!("D-Bus event: {error}"); } }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => { eprintln!("transfer event overflow; rejecting pending requests"); let mut d = data.lock().await; for id in d.pending.keys() { let _ = core.action(id, ChannelAction::RejectTransfer); } d.pending.clear(); }
                Err(_) => break,
            },
            event = devices.recv() => match event {
                Ok(DeviceEvent::Found(device)) => { data.lock().await.devices.insert(device.id.clone(), device.clone()); Service::device_found(service.signal_emitter(), &device.id, &device.name).await?; }
                Ok(DeviceEvent::Lost(id)) => { data.lock().await.devices.remove(&id); Service::device_lost(service.signal_emitter(), &id).await?; }
                _ => (),
            },
            _ = tick.tick() => {
                let mut d = data.lock().await;
                if expire_visibility(&core, &mut d) {
                    drop(d); service.get().await.visibility_changed(service.signal_emitter()).await?;
                } else {
                    let expired = d.pending.iter().filter(|(_, deadline)| **deadline <= Instant::now()).map(|(id, _)| id.clone()).collect::<Vec<_>>();
                    for id in expired { let _ = core.action(&id, ChannelAction::RejectTransfer); d.pending.remove(&id); }
                }
            }
        }
    }
    core.stop().await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test(start_paused = true)]
    async fn ten_minute_visibility_expires_and_contacts_fail_closed() {
        let tmp = tempfile::tempdir().unwrap();
        let core = Core::start(Config {
            name: "Synthetic phone".into(),
            directory: tmp.path().join("received"),
            port: 0,
            interfaces: vec!["lo".into()],
            max_bytes: 1024,
        })
        .await
        .unwrap();
        let service = Service {
            core: core.clone(),
            data: Arc::new(Mutex::new(Data {
                visibility: "off".into(),
                deadline: None,
                name: "Synthetic phone".into(),
                discovering: false,
                devices: BTreeMap::new(),
                transfers: BTreeMap::new(),
                pending: HashMap::new(),
            })),
        };
        assert!(!core.is_visible());
        assert!(service.set_visibility("contacts").await.is_err());
        service.set_visibility("everyone-for-10-min").await.unwrap();
        tokio::time::advance(Duration::from_secs(599)).await;
        assert!(!expire_visibility(&core, &mut *service.data.lock().await));
        tokio::time::advance(Duration::from_secs(1)).await;
        assert!(expire_visibility(&core, &mut *service.data.lock().await));
        assert!(!core.is_visible());
        assert_eq!(service.visibility().await, "off");
        assert!(service.accept("unknown").await.is_err());
        core.stop().await;
    }
}
