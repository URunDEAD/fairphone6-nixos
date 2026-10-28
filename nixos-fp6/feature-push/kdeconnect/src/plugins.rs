use crate::{
    app::App,
    protocol::{now_millis, Packet},
};
use anyhow::{ensure, Result};
use futures_util::StreamExt;
use serde_json::{json, Value};
use std::{collections::HashMap, sync::Arc, time::Duration};
use zbus::{zvariant::OwnedValue, Proxy};

pub async fn handle(app: Arc<App>, id: &str, p: &Packet) -> Result<()> {
    match p.kind.as_str() {
        "kdeconnect.ping" => {
            notify(
                &app,
                "KDE Connect",
                &format!("Ping from {}", peer_name(&app, id).await),
            )
            .await?;
            app.event("Ping received.").await;
        }
        "kdeconnect.battery" => {
            let charge = p.body["currentCharge"]
                .as_i64()
                .filter(|v| (0..=100).contains(v))
                .ok_or_else(|| anyhow::anyhow!("invalid battery charge"))?;
            if let Some(d) = app.inner.lock().await.devices.get_mut(id) {
                d.battery = Some(
                    json!({"charge":charge, "charging":p.body["isCharging"].as_bool().unwrap_or(false)}),
                );
            }
        }
        "kdeconnect.battery.request" => {
            if let Some(p) = battery_packet() {
                app.send(id, p).await?;
            }
        }
        "kdeconnect.notification" => receive_notification(&app, id, &p.body).await?,
        "kdeconnect.notification.request" => {
            if let Some(remote) = p.body["cancel"].as_str() {
                if let Some(local) = app.local_notifications.lock().await.remove(remote) {
                    let proxy = notification_proxy(&app).await?;
                    let _: () = proxy.call("CloseNotification", &(local,)).await?;
                }
            }
            // Existing phone notification history is never replayed.
        }
        "kdeconnect.clipboard" | "kdeconnect.clipboard.connect" => {
            let content = crate::protocol::string(&p.body, "content")?;
            ensure!(content.len() <= 16384, "clipboard exceeds 16 KiB");
            let mut i = app.inner.lock().await;
            let timestamp = if p.kind == "kdeconnect.clipboard.connect" {
                let timestamp = p.body["timestamp"].as_i64().unwrap_or(0).min(now_millis());
                if timestamp <= i.clipboard_timestamp {
                    return Ok(());
                }
                timestamp
            } else {
                now_millis()
            };
            i.clipboard_timestamp = timestamp;
            if i.clipboard != content {
                i.clipboard = content.into();
                i.clipboard_revision += 1;
            }
        }
        "kdeconnect.share.request" => {
            if p.payload_size.is_some() {
                crate::transfer::receive(app.clone(), id, p).await?;
            } else if let Some(url) = p.body["url"].as_str() {
                crate::transfer::validated_url(url)?;
                notify(&app, "Shared link", url).await?;
                app.event(format!("Shared link: {url}")).await;
                // Never launch a browser from untrusted network data.
            } else if let Some(text) = p.body["text"].as_str() {
                ensure!(text.len() <= 16384, "shared text too long");
                notify(&app, "Shared text", text).await?;
                app.event(format!("Shared text: {text}")).await;
            }
        }
        "kdeconnect.findmyphone.request" => ring(&app).await?,
        "kdeconnect.mpris.request" => mpris_request(&app, id, &p.body).await?,
        "kdeconnect.mousepad.request" => app.input.lock().await.inject(id, &p.body)?,
        _ => {}
    }
    Ok(())
}
async fn peer_name(app: &App, id: &str) -> String {
    app.inner
        .lock()
        .await
        .devices
        .get(id)
        .map(|d| d.identity.name.clone())
        .unwrap_or_else(|| "Device".into())
}
async fn notification_proxy(app: &App) -> Result<Proxy<'_>> {
    Ok(Proxy::new(
        &app.bus,
        "org.freedesktop.Notifications",
        "/org/freedesktop/Notifications",
        "org.freedesktop.Notifications",
    )
    .await?)
}
fn escape_markup(s: &str) -> String {
    s.chars()
        .take(4096)
        .collect::<String>()
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}
pub async fn notify(app: &App, title: &str, body: &str) -> Result<u32> {
    let hints: HashMap<&str, OwnedValue> = [
        ("x-kdeconnect-origin", OwnedValue::from(true)),
        ("transient", OwnedValue::from(true)),
    ]
    .into_iter()
    .collect();
    Ok(notification_proxy(app)
        .await?
        .call(
            "Notify",
            &(
                "KDE Connect",
                0u32,
                "smartphone",
                title,
                escape_markup(body),
                Vec::<String>::new(),
                hints,
                8000i32,
            ),
        )
        .await?)
}
async fn receive_notification(app: &App, id: &str, body: &Value) -> Result<()> {
    let remote_id = crate::protocol::string(body, "id")?;
    ensure!(remote_id.len() <= 256, "notification id too long");
    let key = (id.into(), remote_id.into());
    if body["isCancel"].as_bool() == Some(true) {
        if let Some(local) = app.notifications.lock().await.remove(&key) {
            app.notification_replies.lock().await.remove(&local);
            let _: () = notification_proxy(app)
                .await?
                .call("CloseNotification", &(local,))
                .await?;
        }
        return Ok(());
    }
    let title = body["title"]
        .as_str()
        .or(body["appName"].as_str())
        .unwrap_or("Remote notification");
    let text = body["text"]
        .as_str()
        .or(body["ticker"].as_str())
        .unwrap_or("");
    let existing = app
        .notifications
        .lock()
        .await
        .get(&key)
        .copied()
        .unwrap_or(0);
    let reply_id = reply_request_id(body);
    let app_name = body["appName"].as_str().unwrap_or("the device");
    let mut hints: HashMap<&str, OwnedValue> = [
        ("x-kdeconnect-origin", OwnedValue::from(true)),
        ("transient", OwnedValue::from(true)),
    ]
    .into_iter()
    .collect();
    // Inline reply (Android's requestReplyId): the shell's reply field answers
    // with NotificationReplied, reply_listener sends kdeconnect.notification.reply.
    let actions: Vec<String> = if reply_id.is_some() {
        if let Ok(v) = OwnedValue::try_from(zbus::zvariant::Value::from(format!(
            "Reply to {}",
            app_name.chars().take(64).collect::<String>()
        ))) {
            hints.insert("x-kde-reply-placeholder-text", v);
        }
        vec!["inline-reply".into(), "Reply".into()]
    } else {
        Vec::new()
    };
    let local: u32 = notification_proxy(app)
        .await?
        .call(
            "Notify",
            &(
                "KDE Connect",
                existing,
                "smartphone",
                title.chars().take(128).collect::<String>(),
                escape_markup(text),
                actions,
                hints,
                8000i32,
            ),
        )
        .await?;
    {
        let mut replies = app.notification_replies.lock().await;
        replies.remove(&existing);
        if let Some(rid) = reply_id {
            if replies.len() >= 256 {
                replies.clear();
            }
            replies.insert(local, (id.to_string(), rid));
        }
    }
    let mut map = app.notifications.lock().await;
    if map.len() >= 256 {
        map.clear();
    }
    map.insert(key, local);
    Ok(())
}
/// The peer's reply handle for a notification (Android: requestReplyId), if usable.
fn reply_request_id(body: &Value) -> Option<String> {
    body["requestReplyId"]
        .as_str()
        .filter(|s| !s.is_empty() && s.len() <= 256 && !s.chars().any(char::is_control))
        .map(str::to_string)
}
fn reply_packet(request_reply_id: &str, text: &str) -> Packet {
    Packet::new(
        "kdeconnect.notification.reply",
        json!({"requestReplyId": request_reply_id, "message": text.chars().take(4096).collect::<String>()}),
    )
}
/// Sends the shell's inline replies (org.freedesktop.Notifications
/// NotificationReplied, from the notification server only) for peer
/// notifications back to the peer, if it still allows notifications.
pub async fn reply_listener(app: Arc<App>) -> Result<()> {
    let rule = zbus::MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .sender("org.freedesktop.Notifications")?
        .interface("org.freedesktop.Notifications")?
        .path("/org/freedesktop/Notifications")?
        .build();
    let mut stream = zbus::MessageStream::for_match_rule(rule, &app.bus, Some(64)).await?;
    while let Some(message) = stream.next().await {
        let message = message?;
        let header = message.header();
        match header.member().map(|m| m.as_str()) {
            Some("NotificationReplied") => {
                let Ok((local, text)) = message.body().deserialize::<(u32, String)>() else {
                    continue;
                };
                let Some((device, rid)) = app.notification_replies.lock().await.remove(&local)
                else {
                    continue;
                };
                if text.trim().is_empty() || !app.allowed(&device, "notifications").await {
                    continue;
                }
                match app.send(&device, reply_packet(&rid, &text)).await {
                    Ok(()) => app.event("Reply sent.").await,
                    Err(e) => app.event(format!("Reply not sent: {e}")).await,
                }
            }
            Some("NotificationClosed") => {
                if let Ok((local, _)) = message.body().deserialize::<(u32, u32)>() {
                    app.notification_replies.lock().await.remove(&local);
                }
            }
            _ => {}
        }
    }
    Ok(())
}
/// Monitor only the notification method calls. The actual shell notification
/// service remains the owner; this daemon never steals its D-Bus name.
pub async fn notification_monitor(app: Arc<App>) -> Result<()> {
    while !app.any_allowed("notifications").await {
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
    notification_monitor_on(app, zbus::Connection::session().await?).await
}
/// A dedicated monitor connection; separate from the daemon's control bus.
/// Taking the connection explicitly also permits isolated synthetic bus tests.
pub async fn notification_monitor_on(app: Arc<App>, connection: zbus::Connection) -> Result<()> {
    let monitor = Proxy::new(
        &connection,
        "org.freedesktop.DBus",
        "/org/freedesktop/DBus",
        "org.freedesktop.DBus.Monitoring",
    )
    .await?;
    let rules = vec![
        "type='method_call',interface='org.freedesktop.Notifications',member='Notify'",
        "type='method_call',interface='org.freedesktop.Notifications',member='CloseNotification'",
        "type='method_return',sender='org.freedesktop.Notifications'",
        "type='signal',interface='org.freedesktop.Notifications',member='NotificationClosed'",
    ];
    let _: () = monitor.call("BecomeMonitor", &(rules, 0u32)).await?;
    let mut stream = zbus::MessageStream::from(&connection);
    let mut pending = HashMap::<(String, u32), (String, bool)>::new();
    let mut active = HashMap::<u32, String>::new();
    app.local_notifications.lock().await.clear();
    loop {
        let message = tokio::select! {
            message = stream.next() => match message {Some(m)=>m?,None=>return Ok(())},
            _ = tokio::time::sleep(Duration::from_secs(2)) => {
                if !app.any_allowed("notifications").await {return Ok(());}
                continue;
            }
        };
        if !app.any_allowed("notifications").await {
            return Ok(());
        }
        let h = message.header();
        match h.member().map(|m| m.as_str()) {
            Some("Notify") => {
                type NotifyBody = (
                    String,
                    u32,
                    String,
                    String,
                    String,
                    Vec<String>,
                    HashMap<String, OwnedValue>,
                    i32,
                );
                let Ok((name, replaces, _, summary, body, _, hints, _)) =
                    message.body().deserialize::<NotifyBody>()
                else {
                    continue;
                };
                // A remote notification and our own status notices must never
                // be sent back to the peer, including across multiple devices.
                let origin = name == "KDE Connect" || hints.contains_key("x-kdeconnect-origin");
                let remote = active.get(&replaces).cloned().unwrap_or_else(|| {
                    format!(
                        "lomiri-{}-{}",
                        h.sender().map(|s| s.as_str()).unwrap_or("local"),
                        h.primary().serial_num()
                    )
                });
                if pending.len() >= 256 {
                    pending.clear();
                }
                pending.insert(
                    (
                        h.sender().map(|s| s.to_string()).unwrap_or_default(),
                        h.primary().serial_num().get(),
                    ),
                    (remote.clone(), origin),
                );
                if origin {
                    continue;
                }
                let p = Packet::new(
                    "kdeconnect.notification",
                    json!({"id":remote,"appName":name.chars().take(128).collect::<String>(),"title":summary.chars().take(128).collect::<String>(),"text":body.chars().take(4096).collect::<String>(),"ticker":summary.chars().take(128).collect::<String>(),"isClearable":true,"time":now_millis().to_string()}),
                );
                app.broadcast("notifications", p).await;
            }
            Some("CloseNotification") | Some("NotificationClosed") => {
                let local = if h
                    .member()
                    .is_some_and(|m| m.as_str() == "CloseNotification")
                {
                    message.body().deserialize::<(u32,)>().map(|v| v.0).ok()
                } else {
                    message.body().deserialize::<(u32, u32)>().map(|v| v.0).ok()
                };
                if let Some(local) = local {
                    app.local_notifications
                        .lock()
                        .await
                        .retain(|_, value| *value != local);
                    let remote_notifications: Vec<_> = {
                        let mut map = app.notifications.lock().await;
                        let matches: Vec<_> = map
                            .iter()
                            .filter(|(_, value)| **value == local)
                            .map(|(key, _)| key.clone())
                            .collect();
                        for key in &matches {
                            map.remove(key);
                        }
                        matches
                    };
                    let user_dismissed = message
                        .body()
                        .deserialize::<(u32, u32)>()
                        .ok()
                        .is_some_and(|(_, reason)| reason == 2);
                    if user_dismissed {
                        for (id, remote) in remote_notifications {
                            if app.allowed(&id, "notifications").await {
                                app.send(
                                    &id,
                                    Packet::new(
                                        "kdeconnect.notification.request",
                                        json!({"cancel":remote}),
                                    ),
                                )
                                .await?;
                            }
                        }
                    }
                    if let Some(remote) = active.remove(&local) {
                        app.broadcast(
                            "notifications",
                            Packet::new(
                                "kdeconnect.notification",
                                json!({"id":remote,"isCancel":true}),
                            ),
                        )
                        .await;
                    }
                }
            }
            _ => {
                if let (Some(serial), Some(destination)) = (h.reply_serial(), h.destination()) {
                    if let Some((remote, origin)) =
                        pending.remove(&(destination.to_string(), serial.get()))
                    {
                        if !origin {
                            if let Ok((local,)) = message.body().deserialize::<(u32,)>() {
                                if active.len() >= 256 {
                                    active.clear();
                                }
                                let mut local_map = app.local_notifications.lock().await;
                                if local_map.len() >= 256 {
                                    local_map.clear();
                                }
                                local_map.insert(remote.clone(), local);
                                active.insert(local, remote);
                            }
                        }
                    }
                }
            }
        }
    }
}
pub fn battery_packet_from(root: &std::path::Path) -> Option<Packet> {
    for entry in std::fs::read_dir(root).ok()?.flatten() {
        let path = entry.path();
        if std::fs::read_to_string(path.join("type")).ok()?.trim() != "Battery" {
            continue;
        }
        let charge: i32 = std::fs::read_to_string(path.join("capacity"))
            .ok()?
            .trim()
            .parse()
            .ok()?;
        if !(0..=100).contains(&charge) {
            continue;
        }
        let status = std::fs::read_to_string(path.join("status")).ok()?;
        return Some(Packet::new(
            "kdeconnect.battery",
            json!({"currentCharge":charge,"isCharging":matches!(status.trim(),"Charging"|"Full"),"thresholdEvent":if charge <= 15 {1} else {0}}),
        ));
    }
    None
}
fn battery_packet() -> Option<Packet> {
    battery_packet_from(std::path::Path::new("/sys/class/power_supply"))
}
async fn ring(app: &App) -> Result<()> {
    let mut slot = app.ring.lock().await;
    if slot
        .as_mut()
        .is_some_and(|c| c.try_wait().ok().flatten().is_none())
    {
        if let Some(mut c) = slot.take() {
            c.kill().await?;
        }
        return Ok(());
    }
    let path = app.inner.lock().await.store.directory.join("ring.wav");
    if !path.exists() {
        crate::store::atomic_write(&path, &ring_wave())?;
    }
    *slot = Some(
        tokio::process::Command::new("pw-play")
            .arg("--volume=0.7")
            .arg(&path)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true)
            .spawn()?,
    );
    app.event("Phone is ringing. Tap Stop ringing to silence it.".to_string())
        .await;
    Ok(())
}
fn ring_wave() -> Vec<u8> {
    let rate = 16000u32;
    let samples = rate * 20;
    let mut bytes = Vec::new();
    bytes.extend(b"RIFF");
    bytes.extend((36 + samples * 2).to_le_bytes());
    bytes.extend(b"WAVEfmt ");
    bytes.extend(16u32.to_le_bytes());
    bytes.extend(1u16.to_le_bytes());
    bytes.extend(1u16.to_le_bytes());
    bytes.extend(rate.to_le_bytes());
    bytes.extend((rate * 2).to_le_bytes());
    bytes.extend(2u16.to_le_bytes());
    bytes.extend(16u16.to_le_bytes());
    bytes.extend(b"data");
    bytes.extend((samples * 2).to_le_bytes());
    for n in 0..samples {
        let t = n as f32 / rate as f32;
        let value = if n % rate < rate * 3 / 4 {
            (t * std::f32::consts::TAU
                * if n / (rate / 4) % 2 == 0 {
                    880.0
                } else {
                    660.0
                })
            .sin()
                * 12000.0
        } else {
            0.0
        };
        bytes.extend((value as i16).to_le_bytes());
    }
    bytes
}
async fn players(app: &App) -> Result<Vec<String>> {
    let dbus = zbus::fdo::DBusProxy::new(&app.bus).await?;
    Ok(dbus
        .list_names()
        .await?
        .into_iter()
        .map(|n| n.to_string())
        .filter(|n| n.starts_with("org.mpris.MediaPlayer2.") && !n.ends_with(".playerctld"))
        .take(32)
        .collect())
}
async fn mpris_proxy<'a>(app: &'a App, player: &'a str) -> Result<Proxy<'a>> {
    ensure!(
        players(app).await?.iter().any(|p| p == player),
        "unknown media player"
    );
    Ok(Proxy::new(
        &app.bus,
        player,
        "/org/mpris/MediaPlayer2",
        "org.mpris.MediaPlayer2.Player",
    )
    .await?)
}
async fn player_state(app: &App, player: &str) -> Result<Packet> {
    let p = mpris_proxy(app, player).await?;
    let metadata: HashMap<String, OwnedValue> =
        p.get_property("Metadata").await.unwrap_or_default();
    let title = metadata
        .get("xesam:title")
        .and_then(|v| <&str>::try_from(v).ok())
        .unwrap_or("");
    let artists = metadata
        .get("xesam:artist")
        .and_then(|v| v.try_clone().ok())
        .and_then(|v| Vec::<String>::try_from(v).ok())
        .unwrap_or_default();
    let length = metadata
        .get("mpris:length")
        .and_then(|v| i64::try_from(v).ok())
        .unwrap_or(0)
        / 1000;
    let volume: f64 = p.get_property("Volume").await.unwrap_or(0.0);
    let position: i64 = p.get_property("Position").await.unwrap_or(0);
    let status: String = p.get_property("PlaybackStatus").await.unwrap_or_default();
    Ok(Packet::new(
        "kdeconnect.mpris",
        json!({"player":player,"title":title,"artist":artists.join(", "),"nowPlaying":title,"length":length,"pos":position/1000,"volume":(volume*100.0).round(),"isPlaying":status == "Playing","canPlay":p.get_property::<bool>("CanPlay").await.unwrap_or(false),"canPause":p.get_property::<bool>("CanPause").await.unwrap_or(false),"canGoNext":p.get_property::<bool>("CanGoNext").await.unwrap_or(false),"canGoPrevious":p.get_property::<bool>("CanGoPrevious").await.unwrap_or(false),"canSeek":p.get_property::<bool>("CanSeek").await.unwrap_or(false)}),
    ))
}
async fn mpris_request(app: &App, id: &str, body: &Value) -> Result<()> {
    if body["requestPlayerList"].as_bool() == Some(true) || body["player"].as_str().is_none() {
        app.send(
            id,
            Packet::new(
                "kdeconnect.mpris",
                json!({"playerList":players(app).await?,"supportAlbumArtPayload":false}),
            ),
        )
        .await?;
        return Ok(());
    }
    let player = crate::protocol::string(body, "player")?;
    let p = mpris_proxy(app, player).await?;
    if let Some(action) = body["action"].as_str() {
        ensure!(
            ["Play", "Pause", "PlayPause", "Stop", "Next", "Previous"].contains(&action),
            "media action not permitted"
        );
        let _: () = p.call(action, &()).await?;
    }
    if let Some(volume) = body["setVolume"].as_f64() {
        ensure!((0.0..=100.0).contains(&volume), "invalid volume");
        p.set_property("Volume", volume / 100.0).await?;
    }
    if let Some(offset) = body["Seek"].as_i64() {
        ensure!(offset.abs_diff(0) <= 86_400_000_000, "seek out of range");
        let _: () = p.call("Seek", &(offset,)).await?;
    }
    if let Some(ms) = body["setPosition"].as_i64() {
        ensure!((0..=86_400_000).contains(&ms), "position out of range");
        let metadata: HashMap<String, OwnedValue> = p.get_property("Metadata").await?;
        if let Some(track) = metadata
            .get("mpris:trackid")
            .and_then(|v| v.try_clone().ok())
            .and_then(|v| zbus::zvariant::OwnedObjectPath::try_from(v).ok())
        {
            let _: () = p.call("SetPosition", &(track, ms * 1000)).await?;
        }
    }
    app.send(id, player_state(app, player).await?).await?;
    Ok(())
}
pub async fn periodic(app: Arc<App>) {
    let mut ticker = tokio::time::interval(Duration::from_secs(5));
    let mut rounds = 0u64;
    loop {
        ticker.tick().await;
        rounds += 1;
        let expired = app.input.lock().await.expire();
        for id in expired {
            let _ = app.set_plugin(&id, "input", false).await;
        }
        {
            let mut i = app.inner.lock().await;
            for d in i.devices.values_mut() {
                if d.pending
                    .as_ref()
                    .is_some_and(|p| !p.active(now_millis() / 1000))
                {
                    d.pending = None;
                    let _ =
                        d.tx.try_send(Packet::new("kdeconnect.pair", json!({"pair":false})));
                }
            }
        }
        if rounds % 6 == 1 {
            if let Some(packet) = battery_packet() {
                app.broadcast("battery", packet).await;
            }
        }
        if app.any_allowed("media").await {
            if let Ok(Ok(list)) = tokio::time::timeout(Duration::from_secs(2), players(&app)).await
            {
                app.broadcast(
                    "media",
                    Packet::new(
                        "kdeconnect.mpris",
                        json!({"playerList":list,"supportAlbumArtPayload":false}),
                    ),
                )
                .await;
                for player in list {
                    if let Ok(Ok(packet)) =
                        tokio::time::timeout(Duration::from_secs(2), player_state(&app, &player))
                            .await
                    {
                        app.broadcast("media", packet).await;
                    }
                }
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn battery_uses_only_synthetic_sysfs() {
        let root = tempfile::tempdir().unwrap();
        let b = root.path().join("synthetic");
        std::fs::create_dir(&b).unwrap();
        for (name, value) in [
            ("type", "Battery"),
            ("capacity", "42"),
            ("status", "Charging"),
        ] {
            std::fs::write(b.join(name), value).unwrap();
        }
        let p = battery_packet_from(root.path()).unwrap();
        assert_eq!(p.body["currentCharge"], 42);
        assert_eq!(p.body["isCharging"], true);
        std::fs::write(b.join("capacity"), "200").unwrap();
        assert!(battery_packet_from(root.path()).is_none());
    }
    #[test]
    fn reply_ids_and_packets() {
        assert_eq!(
            reply_request_id(&json!({"requestReplyId":"abc-1"})).as_deref(),
            Some("abc-1")
        );
        assert_eq!(reply_request_id(&json!({"requestReplyId":""})), None);
        assert_eq!(reply_request_id(&json!({"requestReplyId":"a\nb"})), None);
        assert_eq!(
            reply_request_id(&json!({"requestReplyId":"x".repeat(300)})),
            None
        );
        assert_eq!(reply_request_id(&json!({"id":"1"})), None);
        let p = reply_packet("abc-1", &"y".repeat(5000));
        assert_eq!(p.kind, "kdeconnect.notification.reply");
        assert_eq!(p.body["requestReplyId"], "abc-1");
        assert_eq!(p.body["message"].as_str().unwrap().len(), 4096);
        assert!(crate::protocol::OUTGOING.contains(&p.kind.as_str()));
        assert_eq!(crate::protocol::plugin_for(&p.kind), Some("notifications"));
    }
    #[test]
    fn notification_markup_is_inert() {
        assert_eq!(escape_markup("<b>A&B</b>"), "&lt;b&gt;A&amp;B&lt;/b&gt;");
    }
}
