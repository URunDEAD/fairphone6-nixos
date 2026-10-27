use crate::{model::Cellular, service::Shared};
use futures_util::StreamExt;
use std::{
    collections::HashMap,
    time::{Duration, Instant},
};
use zbus::{
    message::Type,
    zvariant::{OwnedObjectPath, OwnedValue},
    Connection, MatchRule, MessageStream,
};
type Props = HashMap<String, OwnedValue>;
type Objects = Vec<(OwnedObjectPath, Props)>;
fn state(props: &Props) -> String {
    props
        .get("State")
        .and_then(|v| <&str>::try_from(v).ok())
        .unwrap_or("unknown")
        .into()
}
fn aggregate(calls: &HashMap<String, String>) -> Cellular {
    if calls.values().any(|s| s != "disconnected") {
        Cellular::Busy
    } else {
        Cellular::Idle
    }
}
async fn enumerate(connection: &Connection) -> zbus::Result<HashMap<String, String>> {
    let manager = zbus::Proxy::new(connection, "org.ofono", "/", "org.ofono.Manager").await?;
    let modems: Objects = manager.call("GetModems", &()).await?;
    let mut calls = HashMap::new();
    let mut managers = 0;
    for (path, props) in modems {
        let interfaces = props
            .get("Interfaces")
            .and_then(|v| Vec::<String>::try_from(v.try_clone().ok()?).ok())
            .unwrap_or_default();
        if !interfaces.iter().any(|i| i == "org.ofono.VoiceCallManager") {
            continue;
        }
        managers += 1;
        let proxy =
            zbus::Proxy::new(connection, "org.ofono", path, "org.ofono.VoiceCallManager").await?;
        let entries: Objects = proxy.call("GetCalls", &()).await?;
        for (path, props) in entries {
            calls.insert(path.to_string(), state(&props));
        }
    }
    if managers == 0 {
        return Err(zbus::Error::Failure("no voice call manager".into()));
    }
    Ok(calls)
}
async fn session(model: &Shared, start: Instant) -> zbus::Result<()> {
    let connection = Connection::system().await?;
    let rule = MatchRule::builder()
        .msg_type(Type::Signal)
        .sender("org.ofono")?
        .build();
    let mut signals = MessageStream::for_match_rule(rule, &connection, Some(128)).await?;
    let mut calls = tokio::time::timeout(Duration::from_secs(2), enumerate(&connection))
        .await
        .map_err(|_| zbus::Error::Failure("oFono timeout".into()))??;
    model
        .lock()
        .await
        .set_cellular(aggregate(&calls), start.elapsed().as_secs());
    let bus = zbus::fdo::DBusProxy::new(&connection).await?;
    let initial_owner = bus.get_name_owner("org.ofono".try_into().unwrap()).await?;
    let mut heartbeat = tokio::time::interval(Duration::from_secs(1));
    loop {
        tokio::select! {
            _ = heartbeat.tick() => {
                if bus.get_name_owner("org.ofono".try_into().unwrap()).await? != initial_owner {
                    return Err(zbus::Error::Failure("oFono restarted".into()));
                }
            }
            msg = signals.next() => {
                let msg = msg.ok_or_else(|| zbus::Error::Failure("oFono stream ended".into()))??;
                let header = msg.header();
                let iface = header.interface().map(|v| v.as_str()).unwrap_or("");
                let member = header.member().map(|v| v.as_str()).unwrap_or("");
                match (iface, member) {
                    ("org.ofono.VoiceCallManager", "CallAdded") => {
                        let (path, props): (OwnedObjectPath, Props) = msg.body().deserialize()?;
                        calls.insert(path.to_string(), state(&props));
                    }
                    ("org.ofono.VoiceCallManager", "CallRemoved") => {
                        let (path,): (OwnedObjectPath,) = msg.body().deserialize()?;
                        calls.remove(path.as_str());
                    }
                    ("org.ofono.VoiceCall", "PropertyChanged") => {
                        let (key, value): (String, OwnedValue) = msg.body().deserialize()?;
                        if key == "State" {
                            let value = <&str>::try_from(&value).unwrap_or("unknown");
                            if let Some(path) = header.path() { calls.insert(path.to_string(), value.into()); }
                        }
                    }
                    ("org.ofono.Manager", "ModemAdded" | "ModemRemoved") |
                    ("org.ofono.Modem", "PropertyChanged") => {
                        // Topology changes require a fresh snapshot, with a new subscription.
                        return Err(zbus::Error::Failure("modem topology changed".into()));
                    }
                    _ => continue,
                }
                model.lock().await.set_cellular(aggregate(&calls), start.elapsed().as_secs());
            }
        }
    }
}
pub async fn observe(model: Shared, start: Instant) {
    loop {
        let _ = session(&model, start).await;
        model
            .lock()
            .await
            .set_cellular(Cellular::Unknown, start.elapsed().as_secs());
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}
