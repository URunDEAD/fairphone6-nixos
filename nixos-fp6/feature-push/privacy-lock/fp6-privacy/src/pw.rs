// SPDX-License-Identifier: GPL-3.0-only
//! PipeWire graph from `pw-dump --monitor`: which apps capture the
//! microphone (Stream/Input/Audio linked to an Audio/Source) or a camera
//! (Stream/Input/Video linked to a Video/Source).
//!
//! Not counted: PipeWire's own streams (echo canceller, loopbacks: owned by
//! the pipewire/wireplumber processes), sink monitors (peak meters, "phone
//! sound" recording), and paused (corked) streams.

use std::collections::BTreeMap;

use serde_json::Value;

#[derive(Clone, Debug, Default)]
pub struct Node {
    pub class: String,
    pub state: String,
    pub props: BTreeMap<String, Value>,
}

#[derive(Default, Debug)]
pub struct Graph {
    pub nodes: BTreeMap<u64, Node>,
    /// link id -> (output node, input node)
    pub links: BTreeMap<u64, (u64, u64)>,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Capture {
    pub pid: u32,
    pub app_name: String,
    pub binary: String,
}

const INTERNAL_BINARIES: &[&str] = &["pipewire", "wireplumber", "pipewire-pulse"];

fn prop_str<'a>(props: &'a BTreeMap<String, Value>, key: &str) -> Option<&'a str> {
    props.get(key).and_then(Value::as_str)
}

fn prop_true(props: &BTreeMap<String, Value>, key: &str) -> bool {
    match props.get(key) {
        Some(Value::Bool(b)) => *b,
        Some(Value::String(s)) => s == "true" || s == "1",
        Some(Value::Number(n)) => n.as_i64() == Some(1),
        _ => false,
    }
}

impl Graph {
    /// Apply one pw-dump JSON document (an array of objects).
    pub fn apply(&mut self, doc: &Value) {
        let Some(items) = doc.as_array() else { return };
        for item in items {
            let Some(id) = item.get("id").and_then(Value::as_u64) else { continue };
            let info = item.get("info");
            if info.map_or(true, Value::is_null) {
                self.nodes.remove(&id);
                self.links.remove(&id);
                continue;
            }
            let info = info.unwrap();
            match item.get("type").and_then(Value::as_str).unwrap_or("") {
                "PipeWire:Interface:Node" => {
                    let props: BTreeMap<String, Value> = info
                        .get("props")
                        .and_then(Value::as_object)
                        .map(|o| o.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
                        .unwrap_or_default();
                    let entry = self.nodes.entry(id).or_default();
                    if !props.is_empty() {
                        entry.class = prop_str(&props, "media.class").unwrap_or("").to_owned();
                        entry.props = props;
                    }
                    if let Some(state) = info.get("state").and_then(Value::as_str) {
                        entry.state = state.to_owned();
                    }
                }
                "PipeWire:Interface:Link" => {
                    let out = info.get("output-node-id").and_then(Value::as_u64);
                    let inp = info.get("input-node-id").and_then(Value::as_u64);
                    if let (Some(o), Some(i)) = (out, inp) {
                        self.links.insert(id, (o, i));
                    }
                }
                _ => {}
            }
        }
    }

    fn sources_of(&self, node: u64) -> impl Iterator<Item = &Node> {
        self.links
            .values()
            .filter(move |(_, i)| *i == node)
            .filter_map(|(o, _)| self.nodes.get(o))
    }

    fn captures(&self, stream_class: &str, source_prefix: &str) -> Vec<Capture> {
        let mut out: Vec<Capture> = self
            .nodes
            .iter()
            .filter(|(_, n)| n.class == stream_class && n.state == "running")
            .filter(|(_, n)| !prop_true(&n.props, "stream.monitor") && !prop_true(&n.props, "stream.capture.sink"))
            .filter(|(_, n)| {
                let bin = prop_str(&n.props, "application.process.binary").unwrap_or("");
                !INTERNAL_BINARIES.contains(&bin)
                    && !prop_str(&n.props, "node.name").unwrap_or("").starts_with("echo-cancel")
            })
            .filter(|(id, _)| {
                let mut sources = self.sources_of(**id).peekable();
                // Linked: only counts when fed by a real source (not a sink monitor).
                sources.peek().is_none()
                    || sources.any(|s| {
                        s.class.starts_with(source_prefix)
                            && !prop_str(&s.props, "node.name").unwrap_or("").ends_with(".monitor")
                    })
            })
            .map(|(_, n)| Capture {
                pid: prop_str(&n.props, "application.process.id")
                    .and_then(|p| p.parse().ok())
                    .or_else(|| n.props.get("application.process.id").and_then(Value::as_u64).map(|p| p as u32))
                    .unwrap_or(0),
                app_name: prop_str(&n.props, "application.name").unwrap_or("").to_owned(),
                binary: prop_str(&n.props, "application.process.binary").unwrap_or("").to_owned(),
            })
            .collect();
        out.sort();
        out.dedup();
        out
    }

    pub fn microphone(&self) -> Vec<Capture> {
        self.captures("Stream/Input/Audio", "Audio/Source")
    }

    pub fn camera(&self) -> Vec<Capture> {
        self.captures("Stream/Input/Video", "Video/Source")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn node(id: u64, class: &str, state: &str, props: Value) -> Value {
        let mut p = props;
        p["media.class"] = json!(class);
        json!({"id": id, "type": "PipeWire:Interface:Node", "info": {"state": state, "props": p}})
    }
    fn link(id: u64, o: u64, i: u64) -> Value {
        json!({"id": id, "type": "PipeWire:Interface:Link", "info": {"output-node-id": o, "input-node-id": i}})
    }

    #[test]
    fn mic_and_exclusions() {
        let mut g = Graph::default();
        g.apply(&json!([
            node(53, "Audio/Source", "running", json!({"node.name": "alsa_input.mic"})),
            node(52, "Audio/Sink", "running", json!({"node.name": "alsa_output.speaker"})),
            node(90, "Stream/Input/Audio", "running", json!({"application.name": "Voice Recorder", "application.process.binary": "voicerecorder", "application.process.id": "4242"})),
            node(91, "Stream/Input/Audio", "running", json!({"node.name": "echo-cancel-capture", "application.process.binary": "pipewire"})),
            node(92, "Stream/Input/Audio", "running", json!({"application.name": "Peak", "stream.monitor": true})),
            node(93, "Stream/Input/Audio", "running", json!({"application.name": "Recorder", "application.process.binary": "fp6-screenrecord"})),
            node(94, "Stream/Input/Audio", "suspended", json!({"application.name": "Paused"})),
            link(200, 53, 90), link(201, 53, 91),
            link(202, 52, 93),
        ]));
        // 93 records from a sink, not the mic: link to an Audio/Sink.
        let mic = g.microphone();
        assert_eq!(mic.len(), 1, "{mic:?}");
        assert_eq!(mic[0], Capture { pid: 4242, app_name: "Voice Recorder".into(), binary: "voicerecorder".into() });
        // stream goes away
        g.apply(&json!([{"id": 90, "info": null}]));
        assert!(g.microphone().is_empty());
    }

    #[test]
    fn camera_stream() {
        let mut g = Graph::default();
        g.apply(&json!([
            node(75, "Video/Source", "running", json!({"node.name": "libcamera_input.x"})),
            node(99, "Stream/Input/Video", "running", json!({"application.name": "Firefox", "application.process.binary": "firefox", "application.process.id": 7})),
            link(300, 75, 99),
        ]));
        assert_eq!(g.camera()[0].pid, 7);
        // state update only (no props) keeps the node's props
        g.apply(&json!([{"id": 99, "type": "PipeWire:Interface:Node", "info": {"state": "idle", "props": {}}}]));
        assert!(g.camera().is_empty());
        assert_eq!(g.nodes[&99].props.get("application.name").and_then(Value::as_str), Some("Firefox"));
    }
}
