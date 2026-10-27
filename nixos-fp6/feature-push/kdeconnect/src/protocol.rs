use anyhow::{bail, ensure, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::io::{AsyncBufRead, AsyncBufReadExt};

pub const MAX_PACKET: usize = 64 * 1024;
pub const MAX_IDENTITY: usize = 8192;
pub const PORT: u16 = 1716;
pub const MAX_PORT: u16 = 1764;
pub const INCOMING: &[&str] = &[
    "kdeconnect.ping",
    "kdeconnect.battery",
    "kdeconnect.battery.request",
    "kdeconnect.notification",
    "kdeconnect.notification.request",
    "kdeconnect.clipboard",
    "kdeconnect.clipboard.connect",
    "kdeconnect.share.request",
    "kdeconnect.findmyphone.request",
    "kdeconnect.mpris.request",
    "kdeconnect.mousepad.request",
];
pub const OUTGOING: &[&str] = &[
    "kdeconnect.ping",
    "kdeconnect.battery",
    "kdeconnect.notification",
    "kdeconnect.notification.request",
    "kdeconnect.clipboard",
    "kdeconnect.clipboard.connect",
    "kdeconnect.share.request",
    "kdeconnect.mpris",
    "kdeconnect.mousepad.keyboardstate",
];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Packet {
    pub id: i64,
    #[serde(rename = "type")]
    pub kind: String,
    pub body: Value,
    #[serde(rename = "payloadSize", skip_serializing_if = "Option::is_none")]
    pub payload_size: Option<u64>,
    #[serde(
        rename = "payloadTransferInfo",
        skip_serializing_if = "Option::is_none"
    )]
    pub payload_info: Option<Value>,
}
impl Packet {
    pub fn new(kind: &str, body: Value) -> Self {
        Self {
            id: now_millis(),
            kind: kind.into(),
            body,
            payload_size: None,
            payload_info: None,
        }
    }
    pub fn encode(&self) -> Result<Vec<u8>> {
        let mut bytes = serde_json::to_vec(self)?;
        ensure!(bytes.len() < MAX_PACKET, "packet exceeds limit");
        bytes.push(b'\n');
        Ok(bytes)
    }
    pub fn decode(bytes: &[u8]) -> Result<Self> {
        ensure!(bytes.len() <= MAX_PACKET, "packet exceeds limit");
        let p: Self = serde_json::from_slice(bytes)?;
        ensure!(
            p.kind.starts_with("kdeconnect.") && p.kind.len() <= 128 && p.body.is_object(),
            "invalid packet"
        );
        Ok(p)
    }
    pub fn identity(id: &str, name: &str, version: u8) -> Self {
        Self::new(
            "kdeconnect.identity",
            json!({
                "deviceId": id, "deviceName": name, "deviceType": "phone", "protocolVersion": version,
                "tcpPort": PORT, "incomingCapabilities": INCOMING, "outgoingCapabilities": OUTGOING
            }),
        )
    }
}
#[derive(Debug, Clone, Serialize)]
pub struct Identity {
    pub id: String,
    pub name: String,
    pub version: u8,
}
impl Identity {
    pub fn parse(p: &Packet) -> Result<Self> {
        ensure!(p.kind == "kdeconnect.identity", "expected identity");
        let id = string(&p.body, "deviceId")?;
        ensure!(valid_id(id), "invalid device id");
        let name = p.body["deviceName"].as_str().unwrap_or("Unknown device");
        let version = p.body["protocolVersion"].as_u64().unwrap_or(0);
        ensure!(version == 7 || version == 8, "unsupported protocol version");
        Ok(Self {
            id: id.into(),
            name: name.chars().filter(|c| !c.is_control()).take(64).collect(),
            version: version as u8,
        })
    }
    pub fn same_peer(&self, other: &Self) -> Result<()> {
        ensure!(
            self.id == other.id && self.version == other.version,
            "identity changed during TLS upgrade"
        );
        Ok(())
    }
}
pub fn valid_id(id: &str) -> bool {
    (8..=128).contains(&id.len())
        && id
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
}
pub fn string<'a>(v: &'a Value, key: &str) -> Result<&'a str> {
    v[key]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("missing string {key}"))
}
pub fn now_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}
/// Read one bounded line without consuming bytes from the following TLS handshake.
pub async fn read_packet<R: AsyncBufRead + Unpin>(reader: &mut R, limit: usize) -> Result<Packet> {
    let mut data = Vec::new();
    loop {
        let buf = reader.fill_buf().await?;
        if buf.is_empty() {
            bail!("peer disconnected");
        }
        let len = buf
            .iter()
            .position(|b| *b == b'\n')
            .map(|n| n + 1)
            .unwrap_or(buf.len());
        ensure!(data.len() + len <= limit, "packet exceeds limit");
        let done = buf[len - 1] == b'\n';
        data.extend_from_slice(&buf[..len]);
        reader.consume(len);
        if done {
            return Packet::decode(&data);
        }
    }
}
pub fn plugin_for(kind: &str) -> Option<&'static str> {
    Some(match kind {
        "kdeconnect.ping" => "ping",
        "kdeconnect.battery" | "kdeconnect.battery.request" => "battery",
        "kdeconnect.notification" | "kdeconnect.notification.request" => "notifications",
        "kdeconnect.clipboard" | "kdeconnect.clipboard.connect" => "clipboard",
        "kdeconnect.share.request" => "share",
        "kdeconnect.findmyphone.request" => "findmyphone",
        "kdeconnect.mpris.request" => "media",
        "kdeconnect.mousepad.request" => "input",
        _ => return None,
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn identities_are_bounded() {
        for id in ["../escape", "a", "hello\nworld", "abcdefgh/xx"] {
            assert!(!valid_id(id));
        }
        let a = Identity::parse(&Packet::identity("synthetic-phone", "Test", 8)).unwrap();
        let b = Identity::parse(&Packet::identity("synthetic-phone", "Test", 7)).unwrap();
        assert!(a.same_peer(&b).is_err());
        assert!(Identity::parse(&Packet::identity("synthetic-phone", "Test", 9)).is_err());
    }
    #[tokio::test]
    async fn framing_rejects_flood_and_preserves_next_packet() {
        let p = Packet::new("kdeconnect.ping", json!({})).encode().unwrap();
        let bytes = [p.clone(), p].concat();
        let mut r = &bytes[..];
        assert_eq!(
            read_packet(&mut r, MAX_PACKET).await.unwrap().kind,
            "kdeconnect.ping"
        );
        assert!(read_packet(&mut r, MAX_PACKET).await.is_ok());
        assert!(read_packet(&mut &vec![b'a'; 100][..], 20).await.is_err());
    }
}
