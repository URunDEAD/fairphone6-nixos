// SPDX-License-Identifier: GPL-3.0-only
//! Notification observer: a separate session-bus connection that becomes a
//! monitor (org.freedesktop.DBus.Monitoring.BecomeMonitor) for
//! org.freedesktop.Notifications.Notify method calls only. It never sends
//! anything after BecomeMonitor; it cannot change or block notifications.

use std::collections::HashMap;

use zbus::zvariant::OwnedValue;

/// One Notify() call as seen on the bus.
#[derive(Clone, Debug)]
pub struct Notify {
    pub sender: String,
    pub app_name: String,
    pub replaces_id: u32,
    pub icon: String,
    pub summary: String,
    pub body: String,
    pub hints: HashMap<String, String>,
}

type NotifyArgs = (String, u32, String, String, String, Vec<String>, HashMap<String, OwnedValue>, i32);

fn hint_string(v: &OwnedValue) -> String {
    if let Ok(s) = <&str>::try_from(v) {
        return s.to_string();
    }
    if let Ok(b) = bool::try_from(v) {
        return b.to_string();
    }
    if let Ok(i) = i32::try_from(v) {
        return i.to_string();
    }
    if let Ok(u) = u8::try_from(v) {
        return u.to_string();
    }
    String::new()
}

pub const RULE: &str = "type='method_call',interface='org.freedesktop.Notifications',member='Notify'";

/// Runs forever (until the bus goes away), calling `f` for each Notify.
pub fn run(mut f: impl FnMut(Notify)) -> zbus::Result<()> {
    let conn = zbus::blocking::Connection::session()?;
    conn.call_method(
        Some("org.freedesktop.DBus"),
        "/org/freedesktop/DBus",
        Some("org.freedesktop.DBus.Monitoring"),
        "BecomeMonitor",
        &(vec![RULE], 0u32),
    )?;
    eprintln!("fp6-notifyd: notification monitor ready");
    for msg in zbus::blocking::MessageIterator::from(&conn) {
        let Ok(msg) = msg else { continue };
        let h = msg.header();
        if h.message_type() != zbus::message::Type::MethodCall
            || h.member().map(|m| m.as_str()) != Some("Notify")
            || h.interface().map(|i| i.as_str()) != Some("org.freedesktop.Notifications")
        {
            continue;
        }
        let sender = h.sender().map(|s| s.to_string()).unwrap_or_default();
        let Ok((app_name, replaces_id, icon, summary, body, _actions, hints, _timeout)) =
            msg.body().deserialize::<NotifyArgs>()
        else {
            continue;
        };
        let hints = hints.iter().map(|(k, v)| (k.clone(), hint_string(v))).collect();
        f(Notify { sender, app_name, replaces_id, icon, summary, body, hints });
    }
    Ok(())
}

/// Process name of a bus client (for Notify calls with an empty app_name).
pub fn sender_comm(conn: &zbus::blocking::Connection, sender: &str) -> Option<String> {
    let reply = conn
        .call_method(
            Some("org.freedesktop.DBus"),
            "/org/freedesktop/DBus",
            Some("org.freedesktop.DBus"),
            "GetConnectionUnixProcessID",
            &(sender,),
        )
        .ok()?;
    let pid: u32 = reply.body().deserialize().ok()?;
    let comm = std::fs::read_to_string(format!("/proc/{pid}/comm")).ok()?;
    Some(comm.trim().trim_start_matches('.').trim_end_matches("-wrapped").to_string())
}

/// Shell-internal notifications that are not worth keeping (volume/brightness OSD).
pub fn is_osd(n: &Notify) -> bool {
    ["x-lomiri-private-synchronous", "x-canonical-private-synchronous", "synchronous"]
        .iter()
        .any(|k| n.hints.contains_key(*k))
}
