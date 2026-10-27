//! oFono (system bus) call state: org.ofono.Manager, VoiceCallManager,
//! VoiceCall "State" and CallVolume. Only states, mute and volume are read;
//! LineIdentification/Name (phone numbers, contacts) are never looked at.

use crate::state::{ModemSnapshot, Snapshot};
use std::collections::HashMap;
use std::sync::mpsc::Sender;
use zbus::blocking::{Connection, MessageIterator};
use zbus::zvariant::{OwnedObjectPath, OwnedValue};
use zbus::MatchRule;

type PathProps = Vec<(OwnedObjectPath, HashMap<String, OwnedValue>)>;

fn call(conn: &Connection, path: &str, iface: &str, method: &str) -> zbus::Result<zbus::message::Message> {
    conn.call_method(Some("org.ofono"), path, Some(iface), method, &())
}

fn str_prop(p: &HashMap<String, OwnedValue>, k: &str) -> Option<String> {
    p.get(k).and_then(|v| String::try_from(v.try_clone().ok()?).ok())
}

pub fn snapshot(conn: &Connection) -> zbus::Result<Snapshot> {
    let modems: PathProps = call(conn, "/", "org.ofono.Manager", "GetModems")?.body().deserialize()?;
    let mut snap = Snapshot::default();
    for (path, props) in modems {
        let ifaces: Vec<String> = props
            .get("Interfaces")
            .and_then(|v| Vec::<String>::try_from(v.try_clone().ok()?).ok())
            .unwrap_or_default();
        if !ifaces.iter().any(|i| i == "org.ofono.VoiceCallManager") {
            continue;
        }
        let mut m = ModemSnapshot { path: path.to_string(), ..Default::default() };
        let calls: PathProps = call(conn, path.as_str(), "org.ofono.VoiceCallManager", "GetCalls")?.body().deserialize()?;
        m.call_states = calls.iter().filter_map(|(_, p)| str_prop(p, "State")).collect();
        if ifaces.iter().any(|i| i == "org.ofono.CallVolume") {
            if let Ok(msg) = call(conn, path.as_str(), "org.ofono.CallVolume", "GetProperties") {
                if let Ok(p) = msg.body().deserialize::<HashMap<String, OwnedValue>>() {
                    m.muted = p.get("Muted").and_then(|v| bool::try_from(v).ok());
                    m.speaker_volume = p.get("SpeakerVolume").and_then(|v| u8::try_from(v).ok());
                }
            }
        }
        snap.modems.push(m);
    }
    Ok(snap)
}

pub fn ofono_present(conn: &Connection) -> bool {
    zbus::blocking::fdo::DBusProxy::new(conn)
        .and_then(|p| Ok(p.name_has_owner("org.ofono".try_into().unwrap())?))
        .unwrap_or(false)
}

/// Blocks forever: sends `()` for every oFono signal that can change the
/// call/volume state, and for oFono appearing/vanishing on the bus.
pub fn watch(conn: Connection, tx: Sender<()>) -> zbus::Result<()> {
    let ofono_signals = MatchRule::builder().msg_type(zbus::message::Type::Signal).sender("org.ofono")?.build();
    let owner = MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .interface("org.freedesktop.DBus")?
        .member("NameOwnerChanged")?
        .arg(0, "org.ofono")?
        .build();
    let a = MessageIterator::for_match_rule(ofono_signals, &conn, Some(64))?;
    let b = MessageIterator::for_match_rule(owner, &conn, Some(8))?;
    let tx2 = tx.clone();
    std::thread::spawn(move || {
        for _ in b {
            if tx2.send(()).is_err() {
                break;
            }
        }
    });
    for msg in a {
        let Ok(msg) = msg else { continue };
        let h = msg.header();
        let relevant = match h.interface().map(|i| i.as_str()) {
            Some("org.ofono.VoiceCallManager") | Some("org.ofono.VoiceCall") | Some("org.ofono.CallVolume") => true,
            Some("org.ofono.Manager") => true,
            Some("org.ofono.Modem") => h.member().is_some_and(|m| m.as_str() == "PropertyChanged"),
            _ => false,
        };
        if relevant && tx.send(()).is_err() {
            break;
        }
    }
    Ok(())
}

/// Android resets the uplink mute when the last call ends; oFono keeps it.
/// Clear it so the next call does not start muted.
pub fn clear_mute(conn: &Connection, path: &str) -> zbus::Result<()> {
    conn.call_method(
        Some("org.ofono"),
        path,
        Some("org.ofono.CallVolume"),
        "SetProperty",
        &("Muted", zbus::zvariant::Value::from(false)),
    )?;
    Ok(())
}
