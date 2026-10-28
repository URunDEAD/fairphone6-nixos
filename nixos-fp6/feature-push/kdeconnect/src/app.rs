use crate::{
    pairing::{Direction, Pending},
    protocol::{now_millis, Packet},
    store::{Peer, Store, PLUGINS},
    tls::{fingerprint, verification_code, Credentials},
};
use anyhow::{bail, ensure, Result};
use openssl::x509::X509;
use serde_json::{json, Value};
use std::{collections::BTreeMap, net::SocketAddr, sync::Arc};
use tokio::sync::{mpsc, Mutex, Notify};

pub const BUS_NAME: &str = "com.fairphone6.KdeConnect1";
pub const BUS_PATH: &str = "/com/fairphone6/KdeConnect1";

pub struct Device {
    pub identity: crate::protocol::Identity,
    pub fingerprint: String,
    pub cert: X509,
    pub address: SocketAddr,
    pub local: SocketAddr,
    pub session: u64,
    pub tx: mpsc::Sender<Packet>,
    pub pending: Option<Pending>,
    pub battery: Option<Value>,
}
pub struct Inner {
    pub store: Store,
    pub devices: BTreeMap<String, Device>,
    pub last_event: String,
    pub clipboard: String,
    pub clipboard_revision: u64,
    pub clipboard_timestamp: i64,
}
pub struct App {
    pub inner: Mutex<Inner>,
    pub credentials: Credentials,
    pub bus: zbus::Connection,
    pub discover: Notify,
    pub input: Mutex<crate::input::InputManager>,
    pub notifications: Mutex<BTreeMap<(String, String), u32>>,
    pub local_notifications: Mutex<BTreeMap<String, u32>>,
    /// Shown peer notifications that accept a reply: local id -> (device, requestReplyId).
    pub notification_replies: Mutex<BTreeMap<u32, (String, String)>>,
    pub ring: Mutex<Option<tokio::process::Child>>,
    pub downloads: std::path::PathBuf,
    pub interfaces: Vec<String>,
    pub transfers: Arc<tokio::sync::Semaphore>,
}
impl App {
    pub fn new(
        store: Store,
        credentials: Credentials,
        bus: zbus::Connection,
        interfaces: Vec<String>,
        downloads: std::path::PathBuf,
    ) -> Arc<Self> {
        Arc::new(Self {
            inner: Mutex::new(Inner {
                store,
                devices: BTreeMap::new(),
                last_event: "Ready. Connect to Wi-Fi to discover devices.".into(),
                clipboard: String::new(),
                clipboard_revision: 0,
                clipboard_timestamp: 0,
            }),
            credentials,
            bus,
            discover: Notify::new(),
            input: Mutex::new(crate::input::InputManager::default()),
            notifications: Mutex::new(BTreeMap::new()),
            local_notifications: Mutex::new(BTreeMap::new()),
            notification_replies: Mutex::new(BTreeMap::new()),
            ring: Mutex::new(None),
            interfaces,
            downloads,
            transfers: Arc::new(tokio::sync::Semaphore::new(2)),
        })
    }
    pub async fn snapshot(&self) -> Result<String> {
        let i = self.inner.lock().await;
        let mut devices = BTreeMap::new();
        for (id, p) in &i.store.peers {
            devices.insert(id.clone(), json!({"id":id, "name":p.name, "paired":true, "online":false, "fingerprint":p.fingerprint, "plugins":p.plugins, "pending":null}));
        }
        for (id, d) in &i.devices {
            let p = i.store.peers.get(id);
            devices.insert(id.clone(), json!({"id":id, "name":d.identity.name, "paired":p.is_some(), "online":true, "fingerprint":d.fingerprint, "version":d.identity.version, "plugins":p.map(|p|p.plugins.clone()).unwrap_or_default(), "pending":d.pending.as_ref().filter(|p|p.active(now_millis()/1000)), "battery":d.battery}));
        }
        Ok(serde_json::to_string(
            &json!({"id":i.store.id, "name":i.store.name, "fingerprint":fingerprint(&self.credentials.cert)?, "devices":devices.values().collect::<Vec<_>>(), "event":i.last_event, "plugins":PLUGINS}),
        )?)
    }
    pub async fn event(&self, text: impl Into<String>) {
        self.inner.lock().await.last_event = text.into();
    }
    pub async fn send(&self, id: &str, packet: Packet) -> Result<()> {
        let i = self.inner.lock().await;
        let d = i
            .devices
            .get(id)
            .ok_or_else(|| anyhow::anyhow!("device is offline"))?;
        d.tx.try_send(packet)
            .map_err(|_| anyhow::anyhow!("device send queue is unavailable"))
    }
    pub async fn allowed(&self, id: &str, plugin: &str) -> bool {
        let i = self.inner.lock().await;
        i.devices.contains_key(id) && i.store.peers.get(id).is_some_and(|p| p.allows(plugin))
    }
    pub async fn broadcast(&self, plugin: &str, packet: Packet) {
        let i = self.inner.lock().await;
        for (id, d) in &i.devices {
            if i.store.peers.get(id).is_some_and(|p| p.allows(plugin)) {
                let _ = d.tx.try_send(packet.clone());
            }
        }
    }
    pub async fn any_allowed(&self, plugin: &str) -> bool {
        let i = self.inner.lock().await;
        i.devices
            .keys()
            .any(|id| i.store.peers.get(id).is_some_and(|p| p.allows(plugin)))
    }
    pub async fn request_pair(&self, id: &str, expected_fingerprint: &str) -> Result<()> {
        let mut i = self.inner.lock().await;
        ensure!(!i.store.peers.contains_key(id), "device already paired");
        let d = i
            .devices
            .get_mut(id)
            .ok_or_else(|| anyhow::anyhow!("device offline"))?;
        ensure!(
            d.pending
                .as_ref()
                .is_none_or(|p| !p.active(now_millis() / 1000)),
            "pairing already in progress; use Accept or Reject"
        );
        ensure!(
            d.fingerprint == expected_fingerprint,
            "device certificate changed since it was displayed"
        );
        let now = now_millis() / 1000;
        let code = verification_code(&self.credentials.cert, &d.cert, now)?;
        d.tx.try_send(Packet::new(
            "kdeconnect.pair",
            json!({"pair":true,"timestamp":now}),
        ))?;
        d.pending = Some(Pending::new(Direction::Outgoing, now, now, code)?);
        i.last_event = "Pairing requested. Compare the verification code on both devices.".into();
        Ok(())
    }
    pub async fn accept_pair(&self, id: &str, expected_fingerprint: &str) -> Result<()> {
        let mut i = self.inner.lock().await;
        let d = i
            .devices
            .get_mut(id)
            .ok_or_else(|| anyhow::anyhow!("device offline"))?;
        ensure!(
            d.fingerprint == expected_fingerprint,
            "device certificate changed since it was displayed"
        );
        ensure!(
            d.pending
                .as_ref()
                .is_some_and(|p| p.can_accept(now_millis() / 1000)),
            "no active incoming pairing request"
        );
        let peer = Peer::new(
            d.identity.name.clone(),
            d.fingerprint.clone(),
            d.identity.version,
        );
        d.tx.try_send(Packet::new("kdeconnect.pair", json!({"pair":true})))?;
        d.pending = None;
        i.store.peers.insert(id.into(), peer);
        i.store.save()?;
        i.last_event = "Device paired. Choose which features to allow.".into();
        Ok(())
    }
    pub async fn unpair(&self, id: &str) -> Result<()> {
        self.remove_pair(id, true).await
    }
    async fn remove_pair(&self, id: &str, notify_peer: bool) -> Result<()> {
        self.input.lock().await.revoke(id);
        let mut i = self.inner.lock().await;
        if let Some(d) = i.devices.get_mut(id) {
            if notify_peer {
                let _ =
                    d.tx.try_send(Packet::new("kdeconnect.pair", json!({"pair":false})));
            }
            d.pending = None;
        }
        i.store.peers.remove(id);
        i.store.save()?;
        clear_clipboard_if_unused(&mut i);
        i.last_event = "Pairing removed.".into();
        Ok(())
    }
    pub async fn pair_packet(&self, id: &str, p: &Packet) -> Result<()> {
        let wants = p.body["pair"]
            .as_bool()
            .ok_or_else(|| anyhow::anyhow!("missing pair boolean"))?;
        if !wants {
            return self.remove_pair(id, false).await;
        }
        if p.body.get("timestamp").is_some() {
            self.input.lock().await.revoke(id);
        }
        let mut i = self.inner.lock().await;
        if i.store.peers.contains_key(id) {
            // A new request after peer reset must not silently retain permissions.
            if p.body.get("timestamp").is_none() {
                return Ok(());
            }
            i.store.peers.remove(id);
            i.store.save()?;
        }
        let d = i
            .devices
            .get_mut(id)
            .ok_or_else(|| anyhow::anyhow!("device offline"))?;
        let now = now_millis() / 1000;
        if d.pending.as_ref().is_some_and(|p| p.can_complete(now)) {
            let peer = Peer::new(
                d.identity.name.clone(),
                d.fingerprint.clone(),
                d.identity.version,
            );
            d.pending = None;
            i.store.peers.insert(id.into(), peer);
            i.store.save()?;
            i.last_event = "Pairing accepted by peer.".into();
        } else if d.pending.as_ref().is_some_and(|p| p.active(now)) {
            // Duplicate requests must not extend the consent window.
        } else {
            let timestamp = p.body["timestamp"]
                .as_i64()
                .or_else(|| (d.identity.version == 7).then_some(now))
                .ok_or_else(|| anyhow::anyhow!("v8 pairing request requires timestamp"))?;
            let code = verification_code(&self.credentials.cert, &d.cert, timestamp)?;
            d.pending = Some(Pending::new(Direction::Incoming, timestamp, now, code)?);
            i.last_event =
                "Pairing request received. Open the app to compare fingerprints and confirm."
                    .into();
        }
        Ok(())
    }
    pub async fn set_plugin(&self, id: &str, plugin: &str, enabled: bool) -> Result<()> {
        ensure!(PLUGINS.contains(&plugin), "unknown plugin");
        // Use one lock order for grant creation and revocation. A disconnect or
        // remote unpair cannot slip between trust validation and uinput creation.
        let mut input = self.input.lock().await;
        let mut i = self.inner.lock().await;
        ensure!(
            i.store.peers.contains_key(id),
            "pair before enabling plugins"
        );
        if plugin == "input" && enabled {
            ensure!(
                i.devices.contains_key(id),
                "input consent requires a connected device"
            );
            input.grant(id)?;
        } else if plugin == "input" {
            input.revoke(id);
        }
        let previous = i.store.peers[id].allows(plugin);
        i.store
            .peers
            .get_mut(id)
            .unwrap()
            .plugins
            .insert(plugin.into(), enabled);
        if let Err(e) = i.store.save() {
            i.store
                .peers
                .get_mut(id)
                .unwrap()
                .plugins
                .insert(plugin.into(), previous);
            if plugin == "input" {
                input.revoke(id);
                i.store
                    .peers
                    .get_mut(id)
                    .unwrap()
                    .plugins
                    .insert(plugin.into(), false);
            }
            return Err(e);
        }
        drop(input);
        clear_clipboard_if_unused(&mut i);
        i.last_event = if plugin == "input" && enabled {
            "Remote input allowed for 10 minutes or until disconnect.".into()
        } else {
            format!("{plugin} {}.", if enabled { "enabled" } else { "disabled" })
        };
        drop(i);
        if plugin == "input" {
            self.send(
                id,
                Packet::new(
                    "kdeconnect.mousepad.keyboardstate",
                    json!({"state":enabled}),
                ),
            )
            .await?;
        }
        Ok(())
    }
    pub async fn clipboard_state(&self) -> Value {
        let mut i = self.inner.lock().await;
        clear_clipboard_if_unused(&mut i);
        let enabled = i
            .devices
            .keys()
            .any(|id| i.store.peers.get(id).is_some_and(|p| p.allows("clipboard")));
        json!({"enabled":enabled, "content":i.clipboard, "revision":i.clipboard_revision})
    }
    pub async fn local_clipboard(&self, content: &str) -> Result<()> {
        ensure!(content.len() <= 16384, "clipboard exceeds 16 KiB");
        ensure!(
            self.any_allowed("clipboard").await,
            "clipboard has no consented recipient"
        );
        let mut i = self.inner.lock().await;
        if i.clipboard == content {
            return Ok(());
        }
        i.clipboard = content.into();
        i.clipboard_revision += 1;
        i.clipboard_timestamp = now_millis();
        drop(i);
        self.broadcast(
            "clipboard",
            Packet::new("kdeconnect.clipboard", json!({"content":content})),
        )
        .await;
        Ok(())
    }
    pub async fn action(self: &Arc<Self>, id: &str, action: &str, data: &str) -> Result<()> {
        let plugin = match action {
            "ping" => "ping",
            "ring" => "findmyphone",
            "share" => "share",
            "media" => "media",
            _ => bail!("unknown action"),
        };
        ensure!(
            self.allowed(id, plugin).await,
            "plugin is disabled or peer is offline"
        );
        match action {
            "ping" => {
                self.send(
                    id,
                    Packet::new("kdeconnect.ping", json!({"message":"Hello from Fairphone"})),
                )
                .await?
            }
            "ring" => {
                self.send(id, Packet::new("kdeconnect.findmyphone.request", json!({})))
                    .await?
            }
            "share" => crate::transfer::send_share(self.clone(), id, data).await?,
            "media" => {
                let body: Value = serde_json::from_str(data)?;
                self.send(id, Packet::new("kdeconnect.mpris.request", body))
                    .await?;
            }
            _ => unreachable!(),
        }
        Ok(())
    }
    pub async fn stop_ring(&self) -> Result<()> {
        if let Some(mut child) = self.ring.lock().await.take() {
            child.kill().await?;
        }
        Ok(())
    }
}
fn clear_clipboard_if_unused(i: &mut Inner) {
    if !i
        .devices
        .keys()
        .any(|id| i.store.peers.get(id).is_some_and(|p| p.allows("clipboard")))
    {
        if !i.clipboard.is_empty() {
            i.clipboard.clear();
            i.clipboard_revision += 1;
        }
    }
}
