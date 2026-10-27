// SPDX-License-Identifier: GPL-3.0-only
//! Call sources on the system bus:
//!  * oFono (org.ofono): Manager.GetModems -> VoiceCallManager.GetCalls,
//!    VoiceCall properties State, LineIdentification, Name, IncomingLine,
//!    Multiparty, Emergency. CS calls (and, once the planned qmimodem bridge
//!    exists, mirrored VoLTE calls).
//!  * imsd (net.catcrafts.IMS1, /net/catcrafts/IMS1): GetCalls -> uni,
//!    number, state, direction. VoLTE calls, which oFono does not show today.
//!
//! Both are only read.

use crate::calls::{Call, CallState, Direction, Source};
use std::collections::HashMap;
use std::sync::mpsc::Sender;
use zbus::blocking::{Connection, MessageIterator};
use zbus::zvariant::{OwnedObjectPath, OwnedValue};
use zbus::MatchRule;

pub const OFONO: &str = "org.ofono";
pub const IMSD: &str = "net.catcrafts.IMS1";
const IMSD_PATH: &str = "/net/catcrafts/IMS1";

type Props = HashMap<String, OwnedValue>;
type PathProps = Vec<(OwnedObjectPath, Props)>;

fn str_prop(p: &Props, k: &str) -> String {
    p.get(k).and_then(|v| String::try_from(v.try_clone().ok()?).ok()).unwrap_or_default()
}

fn bool_prop(p: &Props, k: &str) -> bool {
    p.get(k).and_then(|v| bool::try_from(v).ok()).unwrap_or(false)
}

pub fn has_owner(conn: &Connection, name: &str) -> bool {
    let Ok(bus_name) = zbus::names::BusName::try_from(name) else { return false };
    zbus::blocking::fdo::DBusProxy::new(conn)
        .and_then(|p| Ok(p.name_has_owner(bus_name)?))
        .unwrap_or(false)
}

pub fn ofono_call(modem: &str, path: &str, p: &Props) -> Call {
    Call {
        source: Source::Ofono,
        id: path.to_owned(),
        state: CallState::from_ofono(&str_prop(p, "State")),
        number: str_prop(p, "LineIdentification"),
        network_name: str_prop(p, "Name"),
        line: modem.to_owned(),
        incoming_line: str_prop(p, "IncomingLine"),
        multiparty: bool_prop(p, "Multiparty"),
        emergency: bool_prop(p, "Emergency"),
        direction: None,
    }
}

pub fn imsd_call(p: &Props) -> Call {
    let dir = match str_prop(p, "direction").as_str() {
        "incoming" => Some(Direction::In),
        "outgoing" => Some(Direction::Out),
        _ => None,
    };
    Call {
        source: Source::Imsd,
        id: str_prop(p, "uni"),
        state: CallState::from_imsd(&str_prop(p, "state")),
        number: str_prop(p, "number"),
        network_name: String::new(),
        line: "imsd".into(),
        incoming_line: String::new(),
        multiparty: false,
        emergency: false,
        direction: dir,
    }
}

pub fn ofono_calls(conn: &Connection) -> zbus::Result<Vec<Call>> {
    let modems: PathProps = conn.call_method(Some(OFONO), "/", Some("org.ofono.Manager"), "GetModems", &())?.body().deserialize()?;
    let mut out = Vec::new();
    for (path, props) in modems {
        let ifaces: Vec<String> = props
            .get("Interfaces")
            .and_then(|v| Vec::<String>::try_from(v.try_clone().ok()?).ok())
            .unwrap_or_default();
        if !ifaces.iter().any(|i| i == "org.ofono.VoiceCallManager") {
            continue;
        }
        let calls: PathProps = conn
            .call_method(Some(OFONO), path.as_str(), Some("org.ofono.VoiceCallManager"), "GetCalls", &())?
            .body()
            .deserialize()?;
        out.extend(calls.iter().map(|(cp, p)| ofono_call(path.as_str(), cp.as_str(), p)));
    }
    Ok(out)
}

pub fn imsd_calls(conn: &Connection) -> zbus::Result<Vec<Call>> {
    let calls: Vec<Props> = conn.call_method(Some(IMSD), IMSD_PATH, Some(IMSD), "GetCalls", &())?.body().deserialize()?;
    Ok(calls.iter().map(imsd_call).filter(|c| !c.id.is_empty()).collect())
}

/// All calls now. A source that is not on the bus contributes nothing; a
/// failing query is an error (the caller keeps its previous state, so a
/// transient bus error never ends a recording).
pub fn snapshot(conn: &Connection) -> Result<Vec<Call>, String> {
    let mut calls = Vec::new();
    if has_owner(conn, OFONO) {
        calls.extend(ofono_calls(conn).map_err(|e| format!("oFono: {e}"))?);
    }
    if has_owner(conn, IMSD) {
        calls.extend(imsd_calls(conn).map_err(|e| format!("imsd: {e}"))?);
    }
    Ok(calls)
}

/// Blocks: sends `()` for every oFono/imsd signal that can change call
/// state and whenever either service appears or vanishes.
pub fn watch(conn: Connection, tx: Sender<()>) -> zbus::Result<()> {
    let mut iters = Vec::new();
    for sender in [OFONO, IMSD] {
        iters.push(MessageIterator::for_match_rule(
            MatchRule::builder().msg_type(zbus::message::Type::Signal).sender(sender)?.build(),
            &conn,
            Some(64),
        )?);
        iters.push(MessageIterator::for_match_rule(
            MatchRule::builder()
                .msg_type(zbus::message::Type::Signal)
                .interface("org.freedesktop.DBus")?
                .member("NameOwnerChanged")?
                .arg(0, sender)?
                .build(),
            &conn,
            Some(8),
        )?);
    }
    let mut handles = Vec::new();
    for it in iters {
        let tx = tx.clone();
        handles.push(std::thread::spawn(move || {
            for msg in it {
                let Ok(msg) = msg else { continue };
                let h = msg.header();
                let relevant = match h.interface().map(|i| i.as_str()) {
                    Some("org.ofono.VoiceCallManager") | Some("org.ofono.VoiceCall") | Some("org.ofono.Manager") => true,
                    Some("org.ofono.Modem") => h.member().is_some_and(|m| m.as_str() == "PropertyChanged"),
                    Some(IMSD) => h.member().is_some_and(|m| m.as_str().starts_with("Call")),
                    Some("org.freedesktop.DBus") => true,
                    _ => false,
                };
                if relevant && tx.send(()).is_err() {
                    break;
                }
            }
        }));
    }
    for h in handles {
        let _ = h.join();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use zbus::zvariant::Value;

    fn ov<'a>(v: impl Into<Value<'a>>) -> OwnedValue {
        OwnedValue::try_from(v.into()).unwrap()
    }

    #[test]
    fn map_ofono() {
        let mut p = Props::new();
        p.insert("State".into(), ov("alerting"));
        p.insert("LineIdentification".into(), ov("+99955501"));
        p.insert("Multiparty".into(), ov(false));
        let c = ofono_call("/qrtrsoc_0", "/qrtrsoc_0/voicecall01", &p);
        assert_eq!(c.state, CallState::Dialing);
        assert_eq!(c.number, "+99955501");
        assert_eq!(c.line, "/qrtrsoc_0");
        assert_eq!(c.direction, None);
    }

    #[test]
    fn map_imsd() {
        let mut p = Props::new();
        p.insert("uni".into(), ov("c7"));
        p.insert("state".into(), ov("active"));
        p.insert("number".into(), ov("+99955502"));
        p.insert("direction".into(), ov("incoming"));
        let c = imsd_call(&p);
        assert_eq!((c.source, c.state, c.direction), (Source::Imsd, CallState::Active, Some(Direction::In)));
        assert_eq!(c.key(), "Imsd:c7");
    }
}
