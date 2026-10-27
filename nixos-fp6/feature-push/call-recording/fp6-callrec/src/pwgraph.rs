// SPDX-License-Identifier: GPL-3.0-only
//! The PipeWire graph as `pw-dump` prints it: nodes, ports, links and the
//! default sink/source. Only what the taps need to decide which ports to
//! link to the recorder streams.

use serde_json::Value;
use std::collections::BTreeMap;
use std::process::Command;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Node {
    pub id: u32,
    pub name: String,
    pub class: String,
    pub role: String,
    pub binary: String,
    pub pid: u32,
    pub description: String,
    pub state: String,
    /// Negotiated rate (params.Format), when known.
    pub rate: Option<u32>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Port {
    pub id: u32,
    pub node: u32,
    pub output: bool,
    pub monitor: bool,
    pub channel: String,
    pub name: String,
}

#[derive(Clone, Debug, Default)]
pub struct Graph {
    pub nodes: BTreeMap<u32, Node>,
    pub ports: BTreeMap<u32, Port>,
    /// (link id, output port, input port)
    pub links: Vec<(u32, u32, u32)>,
    pub default_sink: Option<String>,
    pub default_source: Option<String>,
}

/// Node names of the recorder's own streams start with this.
pub const OWN_PREFIX: &str = "fp6-callrec";

fn s(v: &Value, k: &str) -> String {
    match v.get(k) {
        Some(Value::String(x)) => x.clone(),
        Some(Value::Number(n)) => n.to_string(),
        Some(Value::Bool(b)) => b.to_string(),
        _ => String::new(),
    }
}

fn truthy(v: &Value, k: &str) -> bool {
    match v.get(k) {
        Some(Value::Bool(b)) => *b,
        Some(Value::String(x)) => x == "true" || x == "1",
        Some(Value::Number(n)) => n.as_i64() == Some(1),
        _ => false,
    }
}

impl Graph {
    pub fn parse(text: &str) -> Result<Graph, String> {
        let doc: Value = serde_json::from_str(text).map_err(|e| format!("pw-dump JSON: {e}"))?;
        Ok(Graph::from_value(&doc))
    }

    pub fn from_value(doc: &Value) -> Graph {
        let mut g = Graph::default();
        let Some(items) = doc.as_array() else { return g };
        for item in items {
            let Some(id) = item.get("id").and_then(Value::as_u64).map(|i| i as u32) else { continue };
            let Some(info) = item.get("info").filter(|i| !i.is_null()) else { continue };
            let empty = Value::Null;
            let props = info.get("props").unwrap_or(&empty);
            match item.get("type").and_then(Value::as_str).unwrap_or("") {
                "PipeWire:Interface:Node" => {
                    let rate = info
                        .get("params")
                        .and_then(|p| p.get("Format"))
                        .and_then(Value::as_array)
                        .and_then(|a| a.first())
                        .and_then(|f| f.get("rate"))
                        .and_then(Value::as_u64)
                        .map(|r| r as u32);
                    g.nodes.insert(
                        id,
                        Node {
                            id,
                            name: s(props, "node.name"),
                            class: s(props, "media.class"),
                            role: s(props, "media.role"),
                            binary: s(props, "application.process.binary"),
                            pid: s(props, "application.process.id").parse().unwrap_or(0),
                            description: s(props, "node.description"),
                            state: info.get("state").and_then(Value::as_str).unwrap_or("").to_owned(),
                            rate,
                        },
                    );
                }
                "PipeWire:Interface:Port" => {
                    let node = s(props, "node.id").parse().unwrap_or(u32::MAX);
                    g.ports.insert(
                        id,
                        Port {
                            id,
                            node,
                            output: info.get("direction").and_then(Value::as_str) == Some("output"),
                            monitor: truthy(props, "port.monitor"),
                            channel: s(props, "audio.channel"),
                            name: s(props, "port.name"),
                        },
                    );
                }
                "PipeWire:Interface:Link" => {
                    let o = info.get("output-port-id").and_then(Value::as_u64);
                    let i = info.get("input-port-id").and_then(Value::as_u64);
                    if let (Some(o), Some(i)) = (o, i) {
                        g.links.push((id, o as u32, i as u32));
                    }
                }
                "PipeWire:Interface:Metadata" => {
                    if s(props, "metadata.name") != "default" {
                        continue;
                    }
                    for m in item.get("metadata").and_then(Value::as_array).into_iter().flatten() {
                        // {"name": ...} (JSON metadata) or the same as a JSON string.
                        let value = match m.get("value") {
                            Some(Value::String(s)) => serde_json::from_str::<Value>(s).unwrap_or(Value::Null),
                            Some(v) => v.clone(),
                            None => Value::Null,
                        };
                        let name = value.get("name").and_then(Value::as_str).map(str::to_owned);
                        match m.get("key").and_then(Value::as_str) {
                            Some("default.audio.sink") => g.default_sink = name,
                            Some("default.audio.source") => g.default_source = name,
                            _ => {}
                        }
                    }
                }
                _ => {}
            }
        }
        g
    }

    /// Run `pw-dump` once (no monitor).
    pub fn dump() -> Result<Graph, String> {
        let out = Command::new("pw-dump").arg("--no-colors").output().map_err(|e| format!("pw-dump: {e}"))?;
        if !out.status.success() {
            return Err(format!("pw-dump failed: {}", String::from_utf8_lossy(&out.stderr).trim()));
        }
        Graph::parse(&String::from_utf8_lossy(&out.stdout))
    }

    pub fn node_by_name(&self, name: &str) -> Option<&Node> {
        self.nodes.values().find(|n| n.name == name)
    }

    fn own(n: &Node) -> bool {
        n.name.starts_with(OWN_PREFIX)
    }

    /// Call audio streams (media.role Phone, as imsd's pw-play/pw-record
    /// set it): (playback = downlink, capture = uplink), newest first.
    pub fn phone_streams(&self) -> (Vec<&Node>, Vec<&Node>) {
        let phone = |n: &&Node| !Self::own(n) && n.role.eq_ignore_ascii_case("phone");
        let mut play: Vec<&Node> = self.nodes.values().filter(phone).filter(|n| n.class == "Stream/Output/Audio").collect();
        let mut cap: Vec<&Node> = self.nodes.values().filter(phone).filter(|n| n.class == "Stream/Input/Audio").collect();
        // pw-play/pw-record (pw-cat) streams first (imsd's), newest first.
        let key = |n: &&Node| (!n.binary.starts_with("pw-"), std::cmp::Reverse(n.id));
        play.sort_by_key(key);
        cap.sort_by_key(key);
        (play, cap)
    }

    pub fn default_sink_node(&self) -> Option<&Node> {
        self.default_sink.as_deref().and_then(|n| self.node_by_name(n)).or_else(|| {
            self.nodes.values().find(|n| n.class == "Audio/Sink")
        })
    }

    pub fn default_source_node(&self) -> Option<&Node> {
        self.default_source
            .as_deref()
            .and_then(|n| self.node_by_name(n))
            .or_else(|| self.nodes.values().find(|n| n.class == "Audio/Source"))
            .or_else(|| self.nodes.values().find(|n| n.class.starts_with("Audio/Source")))
    }

    fn ports_of(&self, node: u32, output: bool, monitor: bool) -> Vec<&Port> {
        let mut v: Vec<&Port> = self.ports.values().filter(|p| p.node == node && p.output == output && p.monitor == monitor).collect();
        v.sort_by_key(|p| p.id);
        v
    }

    /// The one output port to tap for a mono recording of `node`: MONO, else
    /// FL, else the first. Summing both channels of a stereo source would
    /// risk clipping. `monitor` selects a sink's monitor ports.
    pub fn tap_port(&self, node: u32, monitor: bool) -> Option<u32> {
        let ports = self.ports_of(node, true, monitor);
        ports
            .iter()
            .find(|p| p.channel == "MONO")
            .or_else(|| ports.iter().find(|p| p.channel == "FL"))
            .or_else(|| ports.first())
            .map(|p| p.id)
    }

    pub fn input_ports(&self, node: u32) -> Vec<u32> {
        self.ports_of(node, false, false).iter().map(|p| p.id).collect()
    }

    /// Output ports currently linked into `node`'s inputs, one per feeding
    /// node (the tap port of that node, if it is among them).
    pub fn feeding_ports(&self, node: u32) -> Vec<u32> {
        let inputs = self.input_ports(node);
        let mut feeders: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
        for (_, o, i) in &self.links {
            if inputs.contains(i) {
                if let Some(p) = self.ports.get(o) {
                    if self.nodes.get(&p.node).is_some_and(|n| !Self::own(n)) {
                        feeders.entry(p.node).or_default().push(*o);
                    }
                }
            }
        }
        feeders
            .into_iter()
            .filter_map(|(n, ports)| {
                let monitor = ports.iter().any(|p| self.ports.get(p).is_some_and(|p| p.monitor));
                self.tap_port(n, monitor).filter(|t| ports.contains(t)).or_else(|| ports.first().copied())
            })
            .collect()
    }

    pub fn linked(&self, out: u32, inp: u32) -> bool {
        self.links.iter().any(|(_, o, i)| *o == out && *i == inp)
    }

    /// Links into input port `inp`: (link id, output port).
    pub fn links_into(&self, inp: u32) -> Vec<(u32, u32)> {
        self.links.iter().filter(|(_, _, i)| *i == inp).map(|(id, o, _)| (*id, *o)).collect()
    }
}

/// Remove a link object.
pub fn unlink(id: u32) -> Result<(), String> {
    let o = Command::new("pw-cli").arg("destroy").arg(id.to_string()).output().map_err(|e| format!("pw-cli: {e}"))?;
    if o.status.success() {
        Ok(())
    } else {
        Err(format!("pw-cli destroy {id}: {}", String::from_utf8_lossy(&o.stderr).trim()))
    }
}

/// Create a link (lingering: it stays after pw-link exits and goes away with
/// either node).
pub fn link(out: u32, inp: u32) -> Result<(), String> {
    let o = Command::new("pw-link").arg(out.to_string()).arg(inp.to_string()).output().map_err(|e| format!("pw-link: {e}"))?;
    if o.status.success() {
        Ok(())
    } else {
        let err = String::from_utf8_lossy(&o.stderr);
        // "File exists": already linked (raced with another maintain pass).
        if err.contains("exists") {
            Ok(())
        } else {
            Err(format!("pw-link {out} {inp}: {}", err.trim()))
        }
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use serde_json::json;

    pub fn node(id: u32, name: &str, class: &str, extra: Value) -> Value {
        let mut props = json!({"node.name": name, "media.class": class});
        if let Value::Object(m) = extra {
            for (k, v) in m {
                props[k] = v;
            }
        }
        json!({"id": id, "type": "PipeWire:Interface:Node", "info": {"state": "running", "props": props}})
    }

    pub fn port(id: u32, node: u32, dir: &str, channel: &str, monitor: bool) -> Value {
        json!({"id": id, "type": "PipeWire:Interface:Port", "info": {"direction": dir,
            "props": {"node.id": node, "audio.channel": channel, "port.monitor": monitor, "port.name": format!("{dir}_{channel}")}}})
    }

    pub fn link_v(id: u32, o: u32, i: u32) -> Value {
        json!({"id": id, "type": "PipeWire:Interface:Link", "info": {"output-port-id": o, "input-port-id": i}})
    }

    /// Speaker sink (stereo + monitors), mic source, imsd's two streams, the
    /// mic linked into imsd's capture, imsd's playback into the sink, and
    /// our two recorder streams.
    pub fn fixture() -> Value {
        json!([
            node(40, "alsa_output.speaker", "Audio/Sink", json!({})),
            port(401, 40, "input", "FL", false), port(402, 40, "input", "FR", false),
            port(403, 40, "output", "FL", true), port(404, 40, "output", "FR", true),
            node(50, "alsa_input.mic", "Audio/Source", json!({})),
            port(501, 50, "output", "FL", false), port(502, 50, "output", "FR", false),
            {"id": 60, "type": "PipeWire:Interface:Node", "info": {"state": "running",
                "props": {"node.name": "pw-play", "media.class": "Stream/Output/Audio", "media.role": "Phone",
                          "application.process.binary": "pw-play", "application.process.id": "777"},
                "params": {"Format": [{"rate": 16000, "channels": 1}]}}},
            port(601, 60, "output", "MONO", false),
            node(70, "pw-record", "Stream/Input/Audio", json!({"media.role": "Phone", "application.process.binary": "pw-record"})),
            port(701, 70, "input", "MONO", false),
            node(80, "fp6-callrec-1-uplink", "Stream/Input/Audio", json!({})),
            port(801, 80, "input", "MONO", false),
            node(81, "fp6-callrec-1-downlink", "Stream/Input/Audio", json!({})),
            port(811, 81, "input", "MONO", false),
            node(90, "Peak", "Stream/Input/Audio", json!({"media.role": "phone"})),
            link_v(1, 501, 701), link_v(2, 502, 701),
            link_v(3, 601, 401), link_v(4, 601, 402),
            {"id": 0, "type": "PipeWire:Interface:Metadata", "props": {"metadata.name": "default"}, "info": {"props": {"metadata.name": "default"}},
             "metadata": [{"subject": 0, "key": "default.audio.sink", "value": {"name": "alsa_output.speaker"}},
                          {"subject": 0, "key": "default.audio.source", "value": {"name": "alsa_input.mic"}}]},
            {"id": 99, "type": "PipeWire:Interface:Node", "info": null}
        ])
    }

    #[test]
    fn parse_and_query() {
        let g = Graph::from_value(&fixture());
        assert_eq!(g.default_sink_node().unwrap().id, 40);
        assert_eq!(g.default_source_node().unwrap().id, 50);
        let (play, cap) = g.phone_streams();
        assert_eq!(play.iter().map(|n| n.id).collect::<Vec<_>>(), vec![60]);
        assert_eq!(cap.iter().map(|n| n.id).collect::<Vec<_>>(), vec![70, 90]);
        assert_eq!(play[0].rate, Some(16000));
        assert_eq!(play[0].pid, 777);
        assert_eq!(g.tap_port(40, true), Some(403));
        assert_eq!(g.tap_port(40, false), None);
        assert_eq!(g.tap_port(50, false), Some(501));
        assert_eq!(g.tap_port(60, false), Some(601));
        assert_eq!(g.input_ports(80), vec![801]);
        // the mic feeds imsd's capture through both channels: tap FL once
        assert_eq!(g.feeding_ports(70), vec![501]);
        assert!(g.linked(601, 401));
        assert!(!g.linked(601, 811));
        assert_eq!(g.links_into(701), vec![(1, 501), (2, 502)]);
    }
}
