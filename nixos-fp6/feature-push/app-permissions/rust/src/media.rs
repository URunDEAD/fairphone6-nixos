// SPDX-License-Identifier: GPL-3.0-or-later
//! fp6-media-gate's view of the PipeWire graph (from `pw-dump --monitor`)
//! and who each PipeWire client is.
//!
//! The gate publishes one verdict per client and permission in the PipeWire
//! metadata object `fp6-permissions`, which the WirePlumber script
//! (wireplumber/fp6-media-permissions.lua) enforces:
//!
//!   subject = the gate's pw-dump client, key `fp6.gate` = `active:<serial>`
//!                      (enforced only while that client exists)
//!   subject client id, key `fp6.microphone`  = `<object.serial>:<verdict>`
//!   subject client id, key `fp6.camera`      = `<object.serial>:<verdict>`
//!
//! verdict = `granted` | `denied` | `ask`. The client's object.serial (never
//! reused, unlike ids) ties a verdict to one connection. (Any unrestricted
//! PipeWire client could write these entries on purpose; see the README.)
//!
//! Who a client is (`identify`):
//!  * the portal's own connection, WirePlumber, PipeWire and the PulseAudio
//!    server's own connection: system;
//!  * a connection made by pipewire-pulse for a PulseAudio client: that
//!    client's process, from `pipewire.pulse.peer.pid` (SO_PEERCRED, set by
//!    pipewire-pulse-peer-pid.patch; a value the client cannot override) or,
//!    with an unpatched pipewire-pulse, the client's own
//!    `application.process.id` claim (none: the app "unknown", asked about);
//!  * a sandboxed client (`pipewire.access` flatpak/portal, set by the
//!    server) with a portal app id: that app id;
//!  * everything else: the connecting process (`pipewire.sec.pid`, from the
//!    socket).
//! The broker then maps the process to an app by its AppArmor label or app
//! cgroup (fp6-permd CheckPid / AuthorizePid); processes that are not apps
//! (user services, the shell, SSH) are system components and keep access.

use crate::model::Permission;
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::Path;

pub const METADATA_NAME: &str = "fp6-permissions";
pub const GATE_KEY: &str = "fp6.gate";

/// Metadata key of a permission's verdict.
pub fn key(perm: Permission) -> &'static str {
    match perm {
        Permission::Camera => "fp6.camera",
        _ => "fp6.microphone",
    }
}

/// The PipeWire-gated permissions.
pub const GATED: [Permission; 2] = [Permission::Microphone, Permission::Camera];

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Client {
    pub props: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Node {
    pub class: String,
    pub client: Option<u64>,
    pub props: BTreeMap<String, Value>,
}

/// What changed in one pw-dump document.
#[derive(Debug, Default, PartialEq)]
pub struct Changes {
    pub clients_added: Vec<u64>,
    pub clients_removed: Vec<u64>,
    /// New capture streams: (node id, client id, permission).
    pub captures: Vec<(u64, u64, Permission)>,
    /// The fp6-permissions metadata object (re)appeared: publish everything.
    pub metadata_added: bool,
}

#[derive(Debug, Default)]
pub struct Graph {
    pub clients: BTreeMap<u64, Client>,
    pub nodes: BTreeMap<u64, Node>,
    pub metadata: Option<u64>,
}

pub fn prop_str<'a>(props: &'a BTreeMap<String, Value>, key: &str) -> Option<&'a str> {
    props.get(key).and_then(Value::as_str)
}

pub fn prop_u64(props: &BTreeMap<String, Value>, key: &str) -> Option<u64> {
    match props.get(key)? {
        Value::Number(n) => n.as_u64(),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

pub fn prop_true(props: &BTreeMap<String, Value>, key: &str) -> bool {
    match props.get(key) {
        Some(Value::Bool(b)) => *b,
        Some(Value::String(s)) => s == "true" || s == "1" || s == "yes",
        Some(Value::Number(n)) => n.as_i64() == Some(1),
        _ => false,
    }
}

fn props_of(info: &Value) -> BTreeMap<String, Value> {
    info.get("props")
        .and_then(Value::as_object)
        .map(|o| o.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
        .unwrap_or_default()
}

/// The permission a capture stream needs, from its class and properties:
/// audio capture that is not a monitor of what plays, video capture.
/// (WirePlumber decides by the source the stream is actually linked to;
/// this only decides when to ask.)
pub fn capture_perm(class: &str, props: &BTreeMap<String, Value>) -> Option<Permission> {
    match class {
        "Stream/Input/Audio" if !prop_true(props, "stream.capture.sink") && !prop_true(props, "stream.monitor") => {
            Some(Permission::Microphone)
        }
        "Stream/Input/Video" => Some(Permission::Camera),
        _ => None,
    }
}

impl Graph {
    pub fn apply(&mut self, doc: &Value) -> Changes {
        let mut ch = Changes::default();
        let Some(items) = doc.as_array() else { return ch };
        for item in items {
            let Some(id) = item.get("id").and_then(Value::as_u64) else { continue };
            // Metadata objects carry "props" / "metadata" at the top, not "info".
            let info = item.get("info").filter(|i| !i.is_null()).or_else(|| {
                (item.get("props").is_some_and(|p| !p.is_null()) || item.get("metadata").is_some_and(|m| !m.is_null())).then_some(item)
            });
            let Some(info) = info else {
                // removed
                if self.clients.remove(&id).is_some() {
                    ch.clients_removed.push(id);
                }
                self.nodes.remove(&id);
                if self.metadata == Some(id) {
                    self.metadata = None;
                }
                continue;
            };
            match item.get("type").and_then(Value::as_str).unwrap_or("") {
                "PipeWire:Interface:Client" => {
                    let props = props_of(info);
                    let new = !self.clients.contains_key(&id);
                    let c = self.clients.entry(id).or_default();
                    if !props.is_empty() {
                        c.props = props;
                    }
                    if new {
                        ch.clients_added.push(id);
                    }
                }
                "PipeWire:Interface:Node" => {
                    let props = props_of(info);
                    let new = !self.nodes.contains_key(&id);
                    let n = self.nodes.entry(id).or_default();
                    if !props.is_empty() {
                        n.class = prop_str(&props, "media.class").unwrap_or("").to_owned();
                        n.client = prop_u64(&props, "client.id");
                        n.props = props;
                    }
                    if new {
                        if let (Some(c), Some(p)) = (n.client, capture_perm(&n.class, &n.props)) {
                            ch.captures.push((id, c, p));
                        }
                    }
                }
                "PipeWire:Interface:Metadata" => {
                    let props = props_of(info);
                    if prop_str(&props, "metadata.name") == Some(METADATA_NAME) && self.metadata != Some(id) {
                        self.metadata = Some(id);
                        ch.metadata_added = true;
                    }
                }
                _ => {}
            }
        }
        ch
    }
}

/// Who a PipeWire client acts for.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Who {
    /// Ask the broker about this process (CheckPid / AuthorizePid).
    Pid { pid: u32, claimed: bool },
    /// A sandboxed app known by its portal app id (CheckApp / AuthorizeApp).
    App(String),
    /// Not an app: keeps access.
    System(String),
}

/// systemd unit of a process (innermost cgroup component).
pub fn unit_of(proc_root: &Path, pid: u32) -> Option<String> {
    let text = std::fs::read_to_string(proc_root.join(pid.to_string()).join("cgroup")).ok()?;
    let cg = crate::appid::cgroup_of(&text)?;
    crate::appid::leaf_unit(&cg).map(str::to_owned)
}

/// Daemons whose own connections are system components.
const SYSTEM_UNITS: &[&str] = &["pipewire.service", "wireplumber.service", "pipewire-pulse.service", "xdg-desktop-portal.service"];
pub const PULSE_UNIT: &str = "pipewire-pulse.service";

pub fn identify(props: &BTreeMap<String, Value>, proc_root: &Path) -> Who {
    if prop_true(props, "pipewire.access.portal.is_portal") {
        return Who::System("portal".into());
    }
    // pipewire.access is set by the server and cannot be changed by clients.
    let access = prop_str(props, "pipewire.access").unwrap_or("");
    if matches!(access, "flatpak" | "portal") {
        if let Some(app) = prop_str(props, "pipewire.access.portal.app_id").filter(|a| crate::appid::valid_app_id(a)) {
            return Who::App(app.to_owned());
        }
    }
    let Some(sec_pid) = prop_u64(props, "pipewire.sec.pid").and_then(|p| u32::try_from(p).ok()) else {
        // in-process / internal clients carry no peer credentials
        return Who::System("internal".into());
    };
    let unit = unit_of(proc_root, sec_pid);
    if unit.as_deref() == Some(PULSE_UNIT) && prop_str(props, "client.api") == Some("pipewire-pulse") {
        if let Some(pid) = prop_u64(props, "pipewire.pulse.peer.pid").and_then(|p| u32::try_from(p).ok()).filter(|p| *p > 0) {
            return Who::Pid { pid, claimed: false };
        }
        if let Some(pid) = prop_u64(props, "application.process.id").and_then(|p| u32::try_from(p).ok()).filter(|p| *p > 1) {
            return Who::Pid { pid, claimed: true };
        }
        // Nobody we can name: never the pulse server's own (system) access.
        return Who::App("unknown".into());
    }
    if let Some(u) = unit.as_deref().filter(|u| SYSTEM_UNITS.contains(u)) {
        return Who::System(u.to_owned());
    }
    Who::Pid { pid: sec_pid, claimed: false }
}

/// Metadata value for a verdict.
pub fn value(serial: Option<u64>, verdict: &str) -> String {
    format!("{}:{verdict}", serial.unwrap_or(0))
}

pub fn valid_verdict(v: &str) -> &'static str {
    match v {
        "granted" => "granted",
        "denied" => "denied",
        _ => "ask",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::fs;

    fn client(id: u64, props: Value) -> Value {
        json!({"id": id, "type": "PipeWire:Interface:Client", "info": {"props": props}})
    }
    fn node(id: u64, props: Value) -> Value {
        json!({"id": id, "type": "PipeWire:Interface:Node", "info": {"state": "idle", "props": props}})
    }
    fn obj(m: Value) -> BTreeMap<String, Value> {
        m.as_object().unwrap().iter().map(|(k, v)| (k.clone(), v.clone())).collect()
    }

    #[test]
    fn graph_changes() {
        let mut g = Graph::default();
        let ch = g.apply(&json!([
            client(80, json!({"pipewire.sec.pid": 3395, "client.api": "pipewire-pulse", "object.serial": 82})),
            node(90, json!({"media.class": "Stream/Input/Audio", "client.id": 80})),
            node(91, json!({"media.class": "Stream/Input/Audio", "client.id": 80, "stream.capture.sink": "true"})),
            node(92, json!({"media.class": "Stream/Output/Audio", "client.id": 80})),
            node(93, json!({"media.class": "Stream/Input/Video", "client.id": "80"})),
            {"id": 40, "type": "PipeWire:Interface:Metadata", "props": {"metadata.name": "fp6-permissions"}, "metadata": []},
        ]));
        assert_eq!(ch.clients_added, vec![80]);
        assert_eq!(ch.captures, vec![(90, 80, Permission::Microphone), (93, 80, Permission::Camera)]);
        assert!(ch.metadata_added);
        // an update without props keeps what we know and adds nothing
        let ch = g.apply(&json!([{"id": 90, "type": "PipeWire:Interface:Node", "info": {"state": "running"}}]));
        assert_eq!(ch, Changes::default());
        assert_eq!(g.nodes[&90].client, Some(80));
        // removal
        let ch = g.apply(&json!([{"id": 80, "info": null}, {"id": 40, "info": null}]));
        assert_eq!(ch.clients_removed, vec![80]);
        assert_eq!(g.metadata, None);
        let ch = g.apply(&json!([{"id": 41, "type": "PipeWire:Interface:Metadata", "props": {"metadata.name": "fp6-permissions"}}]));
        assert!(ch.metadata_added);
        // a metadata update (entries only) is not a removal
        assert_eq!(g.apply(&json!([{"id": 41, "type": "PipeWire:Interface:Metadata", "metadata": [{"subject": 0, "key": "fp6.gate", "value": "active"}]}])), Changes::default());
        assert_eq!(g.metadata, Some(41));
        assert!(!g.apply(&json!([{"id": 42, "type": "PipeWire:Interface:Metadata", "info": {"props": {"metadata.name": "default"}}}])).metadata_added);
    }

    #[test]
    fn identities() {
        let root = std::env::temp_dir().join(format!("fp6-media-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let unit = |pid: u32, u: &str| {
            let d = root.join(pid.to_string());
            fs::create_dir_all(&d).unwrap();
            fs::write(d.join("cgroup"), format!("0::/user.slice/user-10000.slice/user@10000.service/session.slice/{u}\n")).unwrap();
        };
        unit(3395, "pipewire-pulse.service");
        unit(2915, "wireplumber.service");
        unit(4000, "lomiri-app-launch--application-legacy--org.example.Rec--.service");
        // pulse client: the peer pid pipewire-pulse saw, not the claim
        let pulse = obj(json!({"pipewire.sec.pid": 3395, "client.api": "pipewire-pulse", "pipewire.pulse.peer.pid": "4000", "application.process.id": 1}));
        assert_eq!(identify(&pulse, &root), Who::Pid { pid: 4000, claimed: false });
        // unpatched pipewire-pulse: the claim, marked as such; pid 1 is never taken
        let old = obj(json!({"pipewire.sec.pid": 3395, "client.api": "pipewire-pulse", "application.process.id": 4000}));
        assert_eq!(identify(&old, &root), Who::Pid { pid: 4000, claimed: true });
        let spoof = obj(json!({"pipewire.sec.pid": 3395, "client.api": "pipewire-pulse", "application.process.id": 1}));
        assert_eq!(identify(&spoof, &root), Who::App("unknown".into()));
        // the pulse server's own connection
        assert_eq!(identify(&obj(json!({"pipewire.sec.pid": 3395})), &root), Who::System("pipewire-pulse.service".into()));
        assert_eq!(identify(&obj(json!({"pipewire.sec.pid": 2915})), &root), Who::System("wireplumber.service".into()));
        // a native client claiming to be a pulse client is judged by its socket
        let fake = obj(json!({"pipewire.sec.pid": 4000, "client.api": "pipewire-pulse", "pipewire.pulse.peer.pid": 1}));
        assert_eq!(identify(&fake, &root), Who::Pid { pid: 4000, claimed: false });
        // portal app ids only from the server's access marking
        let sandboxed = obj(json!({"pipewire.sec.pid": 5000, "pipewire.access": "flatpak", "pipewire.access.portal.app_id": "org.gnome.Snapshot"}));
        assert_eq!(identify(&sandboxed, &root), Who::App("org.gnome.Snapshot".into()));
        let claim = obj(json!({"pipewire.sec.pid": 4000, "pipewire.access": "unrestricted", "pipewire.access.portal.app_id": "lomiri-camera-app"}));
        assert_eq!(identify(&claim, &root), Who::Pid { pid: 4000, claimed: false });
        assert_eq!(identify(&obj(json!({"pipewire.access.portal.is_portal": true, "pipewire.sec.pid": 6000})), &root), Who::System("portal".into()));
        assert_eq!(identify(&obj(json!({})), &root), Who::System("internal".into()));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn capture_kinds_and_values() {
        assert_eq!(capture_perm("Stream/Input/Audio", &obj(json!({}))), Some(Permission::Microphone));
        assert_eq!(capture_perm("Stream/Input/Audio", &obj(json!({"stream.monitor": true}))), None);
        assert_eq!(capture_perm("Stream/Input/Video", &obj(json!({}))), Some(Permission::Camera));
        assert_eq!(capture_perm("Stream/Output/Audio", &obj(json!({}))), None);
        assert_eq!(value(Some(82), "granted"), "82:granted");
        assert_eq!(valid_verdict("weird"), "ask");
        assert_eq!(key(Permission::Camera), "fp6.camera");
        assert_eq!(key(Permission::Microphone), "fp6.microphone");
    }
}
