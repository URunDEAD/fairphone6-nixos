#![forbid(unsafe_code)]
//! Quick Share LAN transport using a pinned, hardened rqs_lib protocol engine.
use anyhow::{anyhow, bail, Context, Result};
use get_if_addrs::{get_if_addrs, IfAddr};
use mdns_sd::{AddrType, IfKind, ServiceDaemon, ServiceEvent, ServiceInfo};
use rand::{distr::Alphanumeric, Rng};
pub use rqs_lib::{
    channel::{ChannelAction, ChannelDirection, ChannelMessage, TransferType},
    storage, OutboundPayload, State, TransferMetadata,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    net::{Ipv4Addr, SocketAddr},
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};
use tokio::{
    net::{TcpListener, TcpSocket},
    sync::{broadcast, Mutex, Semaphore},
};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

pub use rqs_lib::TextPayloadType as rqs_text_type;

pub const SERVICE: &str = "_FC9F5ED42C8A._tcp.local.";
pub const PORT: u16 = 53317;

#[derive(Clone, Debug)]
pub struct Config {
    pub name: String,
    pub directory: PathBuf,
    pub port: u16,
    /// Explicit interface allowlist; loopback is only useful for local tests.
    pub interfaces: Vec<String>,
    pub max_bytes: u64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Device {
    pub id: String,
    pub name: String,
    pub address: String,
}
#[derive(Clone, Debug)]
pub enum DeviceEvent {
    Found(Device),
    Lost(String),
}

pub struct Core {
    config: Config,
    pub events: broadcast::Sender<ChannelMessage>,
    pub devices: broadcast::Sender<DeviceEvent>,
    visible: Arc<AtomicBool>,
    shutdown: CancellationToken,
    mdns: ServiceDaemon,
    advertisement: std::sync::Mutex<ServiceInfo>,
    discovery: Mutex<Option<CancellationToken>>,
    endpoint: [u8; 4],
    local_port: u16,
    send_slots: Arc<Semaphore>,
}

impl Core {
    pub async fn start(config: Config) -> Result<Arc<Self>> {
        validate_name(&config.name)?;
        if config.interfaces.is_empty() {
            bail!("at least one Wi-Fi interface is required");
        }
        storage::secure_dir(&config.directory)?;
        let listener = TcpListener::bind((Ipv4Addr::UNSPECIFIED, config.port)).await?;
        let port = listener.local_addr()?.port();
        let endpoint: Vec<u8> = rand::rng().sample_iter(Alphanumeric).take(4).collect();
        let endpoint: [u8; 4] = endpoint.try_into().unwrap();
        let mdns = ServiceDaemon::new()?;
        mdns.disable_interface(IfKind::All)?;
        for interface in &config.interfaces {
            mdns.enable_interface(interface)?;
        }
        mdns.disable_interface(IfKind::IPv6)?;
        let advertisement = advertisement(endpoint, port, &config.name)?;
        let (events, _) = broadcast::channel(512);
        let (devices, _) = broadcast::channel(64);
        let core = Arc::new(Self {
            config,
            events,
            devices,
            visible: Arc::new(AtomicBool::new(false)),
            shutdown: CancellationToken::new(),
            mdns,
            advertisement: std::sync::Mutex::new(advertisement),
            discovery: Mutex::new(None),
            endpoint,
            local_port: port,
            send_slots: Arc::new(Semaphore::new(8)),
        });
        let weak = Arc::downgrade(&core);
        let shutdown = core.shutdown.clone();
        tokio::spawn(async move {
            let slots = Arc::new(Semaphore::new(8));
            loop {
                let incoming = tokio::select! { _ = shutdown.cancelled() => break, r = listener.accept() => r };
                let Ok((socket, _)) = incoming else { break };
                let Some(core) = weak.upgrade() else { break };
                if !core.visible.load(Ordering::SeqCst) {
                    continue;
                }
                let Ok(local) = socket.local_addr() else {
                    continue;
                };
                if !core
                    .interface_addresses()
                    .iter()
                    .any(|(ip, _)| *ip == local.ip())
                {
                    continue;
                }
                let Ok(permit) = slots.clone().try_acquire_owned() else {
                    continue;
                };
                let id = Uuid::new_v4().to_string();
                let sender = core.events.clone();
                let stop = core.shutdown.child_token();
                let policy = storage::ReceiveConfig {
                    directory: core.config.directory.clone(),
                    max_bytes: core.config.max_bytes,
                };
                tokio::spawn(async move {
                    let _permit = permit;
                    let mut request = rqs_lib::InboundRequest::with_config(
                        socket,
                        id.clone(),
                        sender.clone(),
                        policy,
                    );
                    let mut control = sender.subscribe();
                    let run = async {
                        loop {
                            let result =
                                tokio::time::timeout(Duration::from_secs(120), request.handle())
                                    .await;
                            if terminal(&request.state.state) {
                                break;
                            }
                            if !matches!(result, Ok(Ok(()))) {
                                if request.state.state == State::Initial {
                                    break;
                                }
                                eprintln!("incoming transfer {id}: {result:?}");
                                emit(&sender, &id, State::Disconnected);
                                break;
                            }
                        }
                    };
                    tokio::select! { _ = run => (), _ = stop.cancelled() => emit(&sender, &id, State::Cancelled), _ = cancelled(&mut control, &id) => emit(&sender, &id, State::Cancelled) }
                });
            }
        });
        Ok(core)
    }
    pub fn port(&self) -> u16 {
        self.local_port
    }
    pub fn is_visible(&self) -> bool {
        self.visible.load(Ordering::SeqCst)
    }
    pub fn set_visible(&self, visible: bool) -> Result<()> {
        if visible {
            self.mdns
                .register(self.advertisement.lock().unwrap().clone())?;
            self.visible.store(true, Ordering::SeqCst);
        } else {
            self.visible.store(false, Ordering::SeqCst);
            let _ = self
                .mdns
                .unregister(self.advertisement.lock().unwrap().get_fullname())?;
        }
        Ok(())
    }
    pub fn set_name(&self, name: &str) -> Result<()> {
        validate_name(name)?;
        let mut service = self.advertisement.lock().unwrap();
        let _ = self.mdns.unregister(service.get_fullname())?;
        *service = advertisement(self.endpoint, self.local_port, name)?;
        if self.is_visible() {
            self.mdns.register(service.clone())?;
        }
        Ok(())
    }
    fn interface_addresses(&self) -> Vec<(Ipv4Addr, Ipv4Addr)> {
        get_if_addrs()
            .unwrap_or_default()
            .into_iter()
            .filter(|a| self.config.interfaces.contains(&a.name))
            .filter_map(|a| match a.addr {
                IfAddr::V4(a) => Some((a.ip, a.netmask)),
                _ => None,
            })
            .collect()
    }
    fn source_for(&self, destination: Ipv4Addr) -> Option<Ipv4Addr> {
        self.interface_addresses()
            .into_iter()
            .find(|(ip, mask)| {
                (u32::from(*ip) & u32::from(*mask)) == (u32::from(destination) & u32::from(*mask))
            })
            .map(|(ip, _)| ip)
    }
    pub async fn start_discovery(self: &Arc<Self>) -> Result<()> {
        let mut guard = self.discovery.lock().await;
        if guard.is_some() {
            return Ok(());
        }
        let receiver = self.mdns.browse(SERVICE)?;
        let cancel = self.shutdown.child_token();
        *guard = Some(cancel.clone());
        let core = Arc::downgrade(self);
        tokio::spawn(async move {
            let mut cache = HashMap::<String, String>::new();
            loop {
                let event = tokio::select! { _ = cancel.cancelled() => break, r = receiver.recv_async() => r };
                let Some(core) = core.upgrade() else { break };
                match event {
                    Ok(ServiceEvent::ServiceResolved(info)) => {
                        if info.get_fullname() == core.advertisement.lock().unwrap().get_fullname()
                        {
                            continue;
                        }
                        let Some(ip) = info
                            .get_addresses_v4()
                            .into_iter()
                            .find(|ip| core.source_for(**ip).is_some())
                        else {
                            continue;
                        };
                        let Some(n) = info.get_property("n") else {
                            continue;
                        };
                        let Ok((_, name)) = rqs_lib::utils::parse_mdns_endpoint_info(n.val_str())
                        else {
                            continue;
                        };
                        let id = info.get_fullname().to_owned();
                        let device = Device {
                            id: id.clone(),
                            name,
                            address: format!("{ip}:{}", info.get_port()),
                        };
                        cache.insert(id.clone(), device.address.clone());
                        let _ = core.devices.send(DeviceEvent::Found(device));
                    }
                    Ok(ServiceEvent::ServiceRemoved(_, id)) => {
                        cache.remove(&id);
                        let _ = core.devices.send(DeviceEvent::Lost(id));
                    }
                    Err(_) => break,
                    _ => (),
                }
            }
        });
        Ok(())
    }
    pub async fn stop_discovery(&self) -> Result<()> {
        if let Some(token) = self.discovery.lock().await.take() {
            token.cancel();
            self.mdns.stop_browse(SERVICE)?;
        }
        Ok(())
    }
    pub fn action(&self, id: &str, action: ChannelAction) -> Result<()> {
        self.events
            .send(ChannelMessage {
                id: id.into(),
                direction: ChannelDirection::FrontToLib,
                action: Some(action),
                ..Default::default()
            })
            .map_err(|_| anyhow!("transfer no longer exists"))?;
        Ok(())
    }
    pub async fn send(
        self: &Arc<Self>,
        device: &Device,
        payload: OutboundPayload,
        local_name: String,
    ) -> Result<String> {
        let permit = self
            .send_slots
            .clone()
            .try_acquire_owned()
            .context("too many outgoing transfers")?;
        let address: SocketAddr = device.address.parse()?;
        let SocketAddr::V4(address) = address else {
            bail!("IPv4 LAN only");
        };
        let source = self
            .source_for(*address.ip())
            .context("target is not on an allowed local interface subnet")?;
        let socket = TcpSocket::new_v4()?;
        socket.bind(SocketAddr::from((source, 0)))?;
        let socket = tokio::time::timeout(
            Duration::from_secs(10),
            socket.connect(SocketAddr::V4(address)),
        )
        .await??;
        let id = Uuid::new_v4().to_string();
        let sender = self.events.clone();
        let mut request = rqs_lib::OutboundRequest::new(
            self.endpoint,
            socket,
            id.clone(),
            sender.clone(),
            payload,
            rqs_lib::utils::RemoteDeviceInfo {
                name: device.name.clone(),
                device_type: rqs_lib::DeviceType::Phone,
            },
        );
        request.local_name = local_name;
        let return_id = id.clone();
        let stop = self.shutdown.child_token();
        tokio::spawn(async move {
            let _permit = permit;
            let mut control = sender.subscribe();
            let run = async {
                let result: Result<()> = async {
                    request.send_connection_request().await?;
                    request.send_ukey2_client_init().await?;
                    loop {
                        let result =
                            tokio::time::timeout(Duration::from_secs(120), request.handle()).await;
                        if terminal(&request.state.state) {
                            break;
                        }
                        match result {
                            Ok(Ok(())) => (),
                            _ => bail!("transfer disconnected or timed out"),
                        }
                    }
                    Ok(())
                }
                .await;
                if let Err(error) = result {
                    eprintln!("outgoing transfer {id}: {error}");
                    emit(&sender, &id, State::Disconnected);
                }
            };
            tokio::select! { _ = run => (), _ = stop.cancelled() => emit(&sender, &id, State::Cancelled), _ = cancelled(&mut control, &id) => emit(&sender, &id, State::Cancelled) }
        });
        Ok(return_id)
    }
    pub async fn stop(&self) {
        self.visible.store(false, Ordering::SeqCst);
        self.shutdown.cancel();
        let _ = self.mdns.shutdown();
    }
}
impl Drop for Core {
    fn drop(&mut self) {
        self.shutdown.cancel();
        let _ = self.mdns.shutdown();
    }
}

fn advertisement(endpoint: [u8; 4], port: u16, name: &str) -> Result<ServiceInfo> {
    let instance = rqs_lib::utils::gen_mdns_name(endpoint);
    let host = format!("fp6-{}.local.", String::from_utf8_lossy(&endpoint));
    let n = rqs_lib::utils::gen_mdns_endpoint_info(1, name);
    Ok(
        ServiceInfo::new(SERVICE, &instance, &host, "", port, &[("n", n)][..])?
            .enable_addr_auto(AddrType::V4),
    )
}
pub fn validate_name(name: &str) -> Result<()> {
    if name.is_empty() || name.len() > 64 || name.chars().any(char::is_control) {
        bail!("device name must be 1–64 UTF-8 bytes without control characters");
    }
    Ok(())
}
pub fn terminal(state: &State) -> bool {
    matches!(
        state,
        State::Finished | State::Cancelled | State::Rejected | State::Disconnected
    )
}
fn emit(sender: &broadcast::Sender<ChannelMessage>, id: &str, state: State) {
    let _ = sender.send(ChannelMessage {
        id: id.into(),
        direction: ChannelDirection::LibToFront,
        state: Some(state),
        ..Default::default()
    });
}
async fn cancelled(receiver: &mut broadcast::Receiver<ChannelMessage>, id: &str) {
    loop {
        match receiver.recv().await {
            Ok(m)
                if m.id == id
                    && m.direction == ChannelDirection::FrontToLib
                    && m.action == Some(ChannelAction::CancelTransfer) =>
            {
                break
            }
            Err(broadcast::error::RecvError::Closed) => break,
            _ => (),
        }
    }
}

/// D-Bus/CLI payload convention: file:// URIs, text:literal, or url:https://… .
pub fn parse_payload(items: &[String]) -> Result<OutboundPayload> {
    if items.is_empty() || items.len() > 128 {
        bail!("send 1–128 attachments");
    }
    if items.len() == 1 {
        if let Some(text) = items[0].strip_prefix("text:") {
            if text.len() > 65536 {
                bail!("text exceeds 64 KiB");
            }
            return Ok(OutboundPayload::Text(text.into()));
        }
        if let Some(text) = items[0].strip_prefix("url:") {
            let url = url::Url::parse(text)?;
            if !matches!(url.scheme(), "http" | "https") || text.len() > 65536 {
                bail!("only HTTP(S) links are supported");
            }
            return Ok(OutboundPayload::Url(text.into()));
        }
    }
    let mut files = Vec::new();
    let mut total = 0u64;
    for item in items {
        let url = url::Url::parse(item).context("expected a file:// URI")?;
        let path = url
            .to_file_path()
            .map_err(|_| anyhow!("only local file:// URIs are supported"))?;
        let meta = std::fs::metadata(&path)?;
        if !meta.is_file() {
            bail!("attachment is not a regular file");
        }
        total = total.checked_add(meta.len()).context("size overflow")?;
        if total > 1024 * 1024 * 1024 {
            bail!("send exceeds 1 GiB");
        }
        files.push(path.to_str().context("filename is not UTF-8")?.to_owned());
    }
    Ok(OutboundPayload::Files(files))
}
