//! The on-device runtime (fp6-llm, built separately): D-Bus
//! com.fairphone6.LocalLlm1 on the session bus. The assistant only needs
//! Endpoint() (starts llama-server on demand, 127.0.0.1) and Status();
//! Unload() frees the RAM when the user leaves the assistant. Status()
//! carries `n_ctx`, the runtime's effective context window (RAM-dependent:
//! 8192 tokens by default, less when memory is short).
//!
//! The reply shapes are read tolerantly (the runtime is developed in
//! parallel): Endpoint() may return (s url, s model) or one JSON string
//! {"url": ..., "model": ...}; Status() a JSON string or a dict.

use serde_json::Value;
use zbus::Connection;

pub const BUS: &str = "com.fairphone6.LocalLlm1";
pub const PATH: &str = "/com/fairphone6/LocalLlm1";
pub const IFACE: &str = "com.fairphone6.LocalLlm1";

#[derive(Debug, Clone, PartialEq)]
pub struct LocalEndpoint {
    pub url: String,
    pub model: String,
}

pub fn parse_endpoint_json(s: &str) -> Option<LocalEndpoint> {
    let v: Value = serde_json::from_str(s).ok()?;
    let url = v.get("url").or_else(|| v.get("endpoint")).or_else(|| v.get("base_url"))?.as_str()?.to_string();
    let model = v.get("model").or_else(|| v.get("active")).and_then(Value::as_str).unwrap_or("").to_string();
    Some(LocalEndpoint { url, model })
}

async fn call(conn: &Connection, method: &str) -> zbus::Result<zbus::Message> {
    conn.call_method(Some(BUS), PATH, Some(IFACE), method, &()).await
}

/// Resolve the runtime's endpoint (it starts the server if needed). The
/// URL must be loopback; anything else is refused (local means local).
pub async fn endpoint(conn: &Connection) -> Result<LocalEndpoint, String> {
    let reply = call(conn, "Endpoint").await.map_err(|e| {
        let s = e.to_string();
        if s.contains("ServiceUnknown") || s.contains("NameHasNoOwner") {
            "The on-device model service is not installed or not running. Set an address in Assistant > Settings > Models > On this phone, or choose another model.".to_string()
        } else {
            format!("on-device model service: {s}")
        }
    })?;
    let body = reply.body();
    let ep = if let Ok((url, model)) = body.deserialize::<(String, String)>() {
        LocalEndpoint { url, model }
    } else if let Ok(s) = body.deserialize::<String>() {
        parse_endpoint_json(&s).unwrap_or(LocalEndpoint { url: s, model: String::new() })
    } else {
        return Err("on-device model service: unexpected Endpoint reply".into());
    };
    if ep.url.is_empty() {
        return Err("no on-device model is active (Settings > On-device AI models)".into());
    }
    if !super::is_loopback_url(&ep.url) {
        return Err(format!("the on-device endpoint is not on this phone ({}); refusing", ep.url));
    }
    Ok(ep)
}

/// Status as JSON (for the Settings page); "" when the service is absent.
pub async fn status(conn: &Connection) -> String {
    let Ok(reply) = call(conn, "Status").await else { return String::new() };
    let body = reply.body();
    if let Ok(s) = body.deserialize::<String>() {
        return s;
    }
    if let Ok(map) = body.deserialize::<std::collections::HashMap<String, zbus::zvariant::OwnedValue>>() {
        let obj: serde_json::Map<String, Value> = map
            .into_iter()
            .map(|(k, v)| {
                let val = if let Ok(s) = String::try_from(v.try_clone().unwrap()) {
                    Value::String(s)
                } else if let Ok(b) = bool::try_from(&v) {
                    Value::Bool(b)
                } else if let Ok(n) = u64::try_from(&v) {
                    Value::from(n)
                } else {
                    Value::String(format!("{v:?}"))
                };
                (k, val)
            })
            .collect();
        return Value::Object(obj).to_string();
    }
    String::new()
}

/// The effective context window from a Status() JSON (`n_ctx`), if the
/// runtime reports one (older runtimes did not: the caller assumes 2048).
pub fn parse_n_ctx(status: &str) -> Option<u32> {
    let v: Value = serde_json::from_str(status).ok()?;
    let n = v.get("n_ctx")?.as_u64()?;
    (n >= 512).then(|| n.min(u32::MAX as u64) as u32)
}

pub async fn n_ctx(conn: &Connection) -> Option<u32> {
    parse_n_ctx(&status(conn).await)
}

pub async fn unload(conn: &Connection) {
    let _ = call(conn, "Unload").await;
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_json_endpoint() {
        let e = parse_endpoint_json(r#"{"url":"http://127.0.0.1:41234","model":"qwen3-4b-q4_k_m"}"#).unwrap();
        assert_eq!(e.url, "http://127.0.0.1:41234");
        assert_eq!(e.model, "qwen3-4b-q4_k_m");
    }
    #[test]
    fn parses_n_ctx_from_status() {
        assert_eq!(parse_n_ctx(r#"{"loaded":false,"n_ctx":8192,"context_tokens":0}"#), Some(8192));
        assert_eq!(parse_n_ctx(r#"{"state":"running"}"#), None);
        assert_eq!(parse_n_ctx(r#"{"n_ctx":0}"#), None);
        assert_eq!(parse_n_ctx(""), None);
    }
}
