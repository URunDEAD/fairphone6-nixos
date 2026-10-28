//! Model backends. One provider-neutral conversation model; each backend
//! translates it to its wire format and streams events back.
//!
//!  * `openai`: any OpenAI-compatible /v1/chat/completions (the on-device
//!    llama-server from com.fairphone6.LocalLlm1, a manual URL, Ollama's
//!    /v1 endpoint on a LAN machine, hosted providers).
//!  * `anthropic`: the Anthropic Messages API (streaming, tool use, thinking
//!    blocks passed back unchanged).
//!  * `local`: resolves the on-device runtime's endpoint over D-Bus, then
//!    talks OpenAI-compatible to it.

pub mod anthropic;
pub mod local;
pub mod openai;
pub mod sse;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::mpsc;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    User,
    Assistant,
    /// Results of tool calls (sent as `tool` messages / `tool_result` blocks).
    Tool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Part {
    Text { text: String },
    /// An attached image (base64, no data: prefix).
    Image { mime: String, data: String },
    ToolCall { id: String, name: String, arguments: Value },
    ToolResult { id: String, name: String, content: String, is_error: bool },
    /// A provider block that must be sent back verbatim on the next request
    /// (Anthropic `thinking` / `redacted_thinking` with their signatures).
    Opaque { provider: String, block: Value },
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Message {
    pub role: Role,
    pub parts: Vec<Part>,
}

impl Message {
    pub fn user_text(text: impl Into<String>) -> Self {
        Message { role: Role::User, parts: vec![Part::Text { text: text.into() }] }
    }
    pub fn text(&self) -> String {
        self.parts
            .iter()
            .filter_map(|p| match p {
                Part::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("")
    }
    pub fn tool_calls(&self) -> Vec<(String, String, Value)> {
        self.parts
            .iter()
            .filter_map(|p| match p {
                Part::ToolCall { id, name, arguments } => Some((id.clone(), name.clone(), arguments.clone())),
                _ => None,
            })
            .collect()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ToolDef {
    pub name: String,
    pub description: String,
    /// JSON Schema of the arguments (type: object).
    pub parameters: Value,
}

#[derive(Clone, Debug)]
pub struct Request {
    pub model: String,
    pub system: String,
    pub messages: Vec<Message>,
    pub tools: Vec<ToolDef>,
    pub max_tokens: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub enum StopReason {
    EndTurn,
    ToolUse,
    MaxTokens,
    Refusal,
    Other(String),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    TextDelta(String),
    /// The assistant message is complete (text, tool calls, opaque blocks).
    Done { message: Message, stop: StopReason },
}

#[derive(Debug)]
pub enum LlmError {
    Http(String),
    Status(u16, String),
    Protocol(String),
    Cancelled,
}

impl std::error::Error for LlmError {}

impl std::fmt::Display for LlmError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LlmError::Http(e) => write!(f, "connection failed: {e}"),
            LlmError::Status(code, body) => write!(f, "the model service answered HTTP {code}: {}", truncate(body, 300)),
            LlmError::Protocol(e) => write!(f, "unexpected reply from the model service: {e}"),
            LlmError::Cancelled => write!(f, "stopped"),
        }
    }
}

pub fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(max).collect();
        out.push('…');
        out
    }
}

/// Which wire protocol a configured backend speaks.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// The on-device runtime (com.fairphone6.LocalLlm1), falling back to `base_url`.
    Local,
    OpenAi,
    Anthropic,
    /// Ollama (uses its OpenAI-compatible /v1 API; lists models from /api/tags).
    Ollama,
}

/// A resolved, ready-to-call endpoint.
#[derive(Clone, Debug)]
pub struct Endpoint {
    pub kind: Kind,
    pub base_url: String,
    pub api_key: Option<String>,
    /// Only for Anthropic: the effort level ("low" keeps chat replies quick).
    pub effort: Option<String>,
}

impl Endpoint {
    /// True when requests leave the phone (anything but loopback).
    pub fn is_remote(&self) -> bool {
        !is_loopback_url(&self.base_url)
    }
}

pub fn is_loopback_url(url: &str) -> bool {
    let rest = url.split("://").nth(1).unwrap_or(url);
    let host_port = rest.split('/').next().unwrap_or("");
    let host = if host_port.starts_with('[') {
        host_port.trim_start_matches('[').split(']').next().unwrap_or("")
    } else {
        host_port.split(':').next().unwrap_or("")
    };
    host == "localhost" || host.parse::<std::net::IpAddr>().map(|ip| ip.is_loopback()).unwrap_or(false)
}

pub fn http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(15))
        .read_timeout(std::time::Duration::from_secs(300))
        .user_agent("fp6-assistant/0.1")
        .build()
        .expect("reqwest client")
}

/// Stream one model turn. Events go to `tx`; the final `Done` carries the
/// complete assistant message. Cancelling = dropping the future.
pub async fn stream(client: &reqwest::Client, ep: &Endpoint, req: &Request, tx: mpsc::Sender<Event>) -> Result<(), LlmError> {
    match ep.kind {
        Kind::Anthropic => anthropic::stream(client, ep, req, tx).await,
        Kind::OpenAi | Kind::Ollama | Kind::Local => openai::stream(client, ep, req, tx).await,
    }
}

/// Models the endpoint offers (for the Settings picker). Best effort.
pub async fn list_models(client: &reqwest::Client, ep: &Endpoint) -> Result<Vec<String>, LlmError> {
    match ep.kind {
        Kind::Anthropic => anthropic::list_models(client, ep).await,
        Kind::Ollama => openai::list_ollama_models(client, ep).await,
        _ => openai::list_models(client, ep).await,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn loopback_detection() {
        assert!(is_loopback_url("http://127.0.0.1:8080/v1"));
        assert!(is_loopback_url("http://localhost:11434"));
        assert!(is_loopback_url("http://[::1]:9000"));
        assert!(!is_loopback_url("http://192.168.1.20:11434"));
        assert!(!is_loopback_url("https://api.anthropic.com"));
        assert!(!is_loopback_url("https://127.0.0.1.example.com/"));
    }
}
