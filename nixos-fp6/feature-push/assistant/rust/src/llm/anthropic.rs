//! Anthropic Messages API (raw HTTP: Rust has no official SDK).
//! POST {base}/v1/messages, x-api-key, anthropic-version: 2023-06-01,
//! stream: true. Thinking stays at the model default (adaptive on the
//! current models); depth via output_config.effort. Thinking blocks are
//! kept as opaque parts and sent back unchanged. Forced tool_choice is not
//! used (current models reject it). Tool inputs stream eagerly and are
//! validated by the tool layer before anything runs.

use super::{sse, Endpoint, Event, LlmError, Message, Part, Request, Role, StopReason};
use serde_json::{json, Value};
use tokio::sync::mpsc;

pub const DEFAULT_BASE: &str = "https://api.anthropic.com";
pub const VERSION: &str = "2023-06-01";
/// Server-side refusal fallback (Claude API only; routes by refusal category).
const FALLBACK_BETA: &str = "server-side-fallback-2026-07-01";

/// Models offered in Settings before the key is entered (the list is
/// refreshed from GET /v1/models once a key works).
pub const DEFAULT_MODELS: &[&str] = &["claude-sonnet-5-5", "claude-haiku-5-5", "claude-opus-5-5"];

fn supports_fallback(model: &str) -> bool {
    model.starts_with("claude-sonnet-5-5") || model.starts_with("claude-opus-5-5") || model.starts_with("claude-fable-5-1")
}

fn is_first_party(base: &str) -> bool {
    base.trim_end_matches('/') == DEFAULT_BASE
}

pub fn body(req: &Request, ep: &Endpoint) -> Value {
    let mut messages: Vec<Value> = Vec::new();
    for m in &req.messages {
        let (role, blocks): (&str, Vec<Value>) = match m.role {
            Role::User => (
                "user",
                m.parts
                    .iter()
                    .filter_map(|p| match p {
                        Part::Text { text } => Some(json!({"type": "text", "text": text})),
                        Part::Image { mime, data } => Some(json!({"type": "image", "source": {"type": "base64", "media_type": mime, "data": data}})),
                        _ => None,
                    })
                    .collect(),
            ),
            Role::Assistant => (
                "assistant",
                m.parts
                    .iter()
                    .filter_map(|p| match p {
                        Part::Text { text } if !text.is_empty() => Some(json!({"type": "text", "text": text})),
                        Part::ToolCall { id, name, arguments } => {
                            let input = if arguments.is_object() { arguments.clone() } else { json!({}) };
                            Some(json!({"type": "tool_use", "id": id, "name": name, "input": input}))
                        }
                        Part::Opaque { provider, block } if provider == "anthropic" => Some(block.clone()),
                        _ => None,
                    })
                    .collect(),
            ),
            Role::Tool => (
                "user",
                m.parts
                    .iter()
                    .filter_map(|p| match p {
                        Part::ToolResult { id, content, is_error, .. } => {
                            Some(json!({"type": "tool_result", "tool_use_id": id, "content": content, "is_error": is_error}))
                        }
                        Part::Image { mime, data } => Some(json!({"type": "image", "source": {"type": "base64", "media_type": mime, "data": data}})),
                        _ => None,
                    })
                    .collect(),
            ),
        };
        if blocks.is_empty() {
            continue;
        }
        // Merge consecutive same-role turns (tool results followed by a new user prompt).
        if let Some(last) = messages.last_mut() {
            if last["role"] == role {
                last["content"].as_array_mut().unwrap().extend(blocks);
                continue;
            }
        }
        messages.push(json!({"role": role, "content": blocks}));
    }
    let mut body = json!({
        "model": req.model,
        "max_tokens": req.max_tokens,
        "system": req.system,
        "messages": messages,
        "stream": true,
    });
    if let Some(effort) = &ep.effort {
        body["output_config"] = json!({"effort": effort});
    }
    if !req.tools.is_empty() {
        body["tools"] = Value::Array(
            req.tools
                .iter()
                .map(|t| json!({"name": t.name, "description": t.description, "input_schema": t.parameters, "eager_input_streaming": true}))
                .collect(),
        );
    }
    if is_first_party(&ep.base_url) && supports_fallback(&req.model) {
        body["fallbacks"] = json!("default");
    }
    body
}

#[derive(Default)]
struct Block {
    kind: String,
    value: Value,
    json: String,
}

#[derive(Default)]
pub struct Accumulator {
    blocks: Vec<Block>,
    stop: Option<String>,
}

impl Accumulator {
    /// Handle one SSE event; returns a text delta when there is one.
    pub fn event(&mut self, name: &str, v: &Value) -> Result<Option<String>, LlmError> {
        let ty = v.get("type").and_then(Value::as_str).unwrap_or(name);
        match ty {
            "content_block_start" => {
                let idx = v["index"].as_u64().unwrap_or(self.blocks.len() as u64) as usize;
                while self.blocks.len() <= idx {
                    self.blocks.push(Block::default());
                }
                let cb = v["content_block"].clone();
                self.blocks[idx] = Block { kind: cb["type"].as_str().unwrap_or("").to_string(), value: cb, json: String::new() };
                Ok(None)
            }
            "content_block_delta" => {
                let idx = v["index"].as_u64().unwrap_or(0) as usize;
                let Some(b) = self.blocks.get_mut(idx) else { return Ok(None) };
                let d = &v["delta"];
                match d["type"].as_str().unwrap_or("") {
                    "text_delta" => {
                        let t = d["text"].as_str().unwrap_or("").to_string();
                        let cur = b.value["text"].as_str().unwrap_or("").to_string();
                        b.value["text"] = Value::String(cur + &t);
                        Ok(if t.is_empty() { None } else { Some(t) })
                    }
                    "input_json_delta" => {
                        b.json.push_str(d["partial_json"].as_str().unwrap_or(""));
                        Ok(None)
                    }
                    "thinking_delta" => {
                        let cur = b.value["thinking"].as_str().unwrap_or("").to_string();
                        b.value["thinking"] = Value::String(cur + d["thinking"].as_str().unwrap_or(""));
                        Ok(None)
                    }
                    "signature_delta" => {
                        let cur = b.value["signature"].as_str().unwrap_or("").to_string();
                        b.value["signature"] = Value::String(cur + d["signature"].as_str().unwrap_or(""));
                        Ok(None)
                    }
                    _ => Ok(None),
                }
            }
            "message_delta" => {
                if let Some(s) = v["delta"]["stop_reason"].as_str() {
                    self.stop = Some(s.to_string());
                }
                Ok(None)
            }
            "error" => Err(LlmError::Protocol(v["error"]["message"].as_str().unwrap_or("error").to_string())),
            _ => Ok(None),
        }
    }

    pub fn finish(self) -> (Message, StopReason) {
        let mut parts = Vec::new();
        for b in self.blocks {
            match b.kind.as_str() {
                "text" => {
                    let t = b.value["text"].as_str().unwrap_or("").to_string();
                    if !t.is_empty() {
                        parts.push(Part::Text { text: t });
                    }
                }
                "tool_use" => {
                    // Eager streaming: the input may be incomplete or invalid; the
                    // tool layer validates and reports errors back to the model.
                    let args = if b.json.trim().is_empty() {
                        b.value.get("input").cloned().unwrap_or(json!({}))
                    } else {
                        serde_json::from_str(&b.json).unwrap_or(Value::String(b.json.clone()))
                    };
                    parts.push(Part::ToolCall {
                        id: b.value["id"].as_str().unwrap_or("").to_string(),
                        name: b.value["name"].as_str().unwrap_or("").to_string(),
                        arguments: args,
                    });
                }
                "thinking" | "redacted_thinking" => parts.push(Part::Opaque { provider: "anthropic".into(), block: b.value }),
                _ => {}
            }
        }
        let stop = match self.stop.as_deref() {
            Some("end_turn") | Some("stop_sequence") | None => StopReason::EndTurn,
            Some("tool_use") => StopReason::ToolUse,
            Some("max_tokens") => StopReason::MaxTokens,
            Some("refusal") => StopReason::Refusal,
            Some(o) => StopReason::Other(o.to_string()),
        };
        (Message { role: Role::Assistant, parts }, stop)
    }
}

fn base(ep: &Endpoint) -> String {
    let b = ep.base_url.trim_end_matches('/');
    if b.is_empty() { DEFAULT_BASE.to_string() } else { b.trim_end_matches("/v1").to_string() }
}

pub async fn stream(client: &reqwest::Client, ep: &Endpoint, req: &Request, tx: mpsc::Sender<Event>) -> Result<(), LlmError> {
    let body = body(req, ep);
    let mut rb = client
        .post(format!("{}/v1/messages", base(ep)))
        .header("anthropic-version", VERSION)
        .header("x-api-key", ep.api_key.clone().unwrap_or_default());
    if body.get("fallbacks").is_some() {
        rb = rb.header("anthropic-beta", FALLBACK_BETA);
    }
    let resp = rb.json(&body).send().await.map_err(|e| LlmError::Http(e.to_string()))?;
    let status = resp.status();
    if !status.is_success() {
        let text = resp.text().await.unwrap_or_default();
        // Error bodies are {"type":"error","error":{"message":...}}; never echo headers.
        let msg = serde_json::from_str::<Value>(&text)
            .ok()
            .and_then(|v| v["error"]["message"].as_str().map(String::from))
            .unwrap_or(text);
        return Err(LlmError::Status(status.as_u16(), msg));
    }
    let mut acc = Accumulator::default();
    let mut events = sse::SseStream::new(resp);
    while let Some(ev) = events.next().await {
        let ev = ev?;
        if ev.data.trim().is_empty() {
            continue;
        }
        let v: Value = serde_json::from_str(&ev.data).map_err(|e| LlmError::Protocol(e.to_string()))?;
        if let Some(t) = acc.event(&ev.event, &v)? {
            let _ = tx.send(Event::TextDelta(t)).await;
        }
        if v["type"] == "message_stop" {
            break;
        }
    }
    let (message, stop) = acc.finish();
    let _ = tx.send(Event::Done { message, stop }).await;
    Ok(())
}

pub async fn list_models(client: &reqwest::Client, ep: &Endpoint) -> Result<Vec<String>, LlmError> {
    let resp = client
        .get(format!("{}/v1/models?limit=100", base(ep)))
        .header("anthropic-version", VERSION)
        .header("x-api-key", ep.api_key.clone().unwrap_or_default())
        .send()
        .await
        .map_err(|e| LlmError::Http(e.to_string()))?;
    if !resp.status().is_success() {
        return Err(LlmError::Status(resp.status().as_u16(), String::new()));
    }
    let v: Value = resp.json().await.map_err(|e| LlmError::Protocol(e.to_string()))?;
    Ok(v["data"].as_array().map(|a| a.iter().filter_map(|m| m["id"].as_str().map(String::from)).collect()).unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::{Kind, ToolDef};

    fn ep() -> Endpoint {
        Endpoint { kind: Kind::Anthropic, base_url: DEFAULT_BASE.into(), api_key: Some("test-key-not-real".into()), effort: Some("low".into()) }
    }

    #[test]
    fn body_merges_tool_results_and_keeps_thinking() {
        let thinking = json!({"type": "thinking", "thinking": "", "signature": "sig"});
        let req = Request {
            model: "claude-sonnet-5-5".into(),
            system: "s".into(),
            messages: vec![
                Message::user_text("time?"),
                Message {
                    role: Role::Assistant,
                    parts: vec![
                        Part::Opaque { provider: "anthropic".into(), block: thinking.clone() },
                        Part::ToolCall { id: "toolu_1".into(), name: "clock_now".into(), arguments: json!({}) },
                    ],
                },
                Message { role: Role::Tool, parts: vec![Part::ToolResult { id: "toolu_1".into(), name: "clock_now".into(), content: "12:00".into(), is_error: false }] },
                Message::user_text("thanks"),
            ],
            tools: vec![ToolDef { name: "clock_now".into(), description: "d".into(), parameters: json!({"type": "object"}) }],
            max_tokens: 1000,
        };
        let b = body(&req, &ep());
        assert_eq!(b["messages"].as_array().unwrap().len(), 3);
        assert_eq!(b["messages"][1]["content"][0], thinking);
        assert_eq!(b["messages"][2]["content"][0]["type"], "tool_result");
        assert_eq!(b["messages"][2]["content"][1]["text"], "thanks");
        assert_eq!(b["output_config"]["effort"], "low");
        assert_eq!(b["fallbacks"], "default");
        assert!(b.get("thinking").is_none());
        assert!(b.get("tool_choice").is_none());
    }

    #[test]
    fn accumulates_stream() {
        let mut acc = Accumulator::default();
        let evs = [
            json!({"type":"message_start","message":{}}),
            json!({"type":"content_block_start","index":0,"content_block":{"type":"thinking","thinking":"","signature":""}}),
            json!({"type":"content_block_delta","index":0,"delta":{"type":"signature_delta","signature":"abc"}}),
            json!({"type":"content_block_start","index":1,"content_block":{"type":"text","text":""}}),
            json!({"type":"content_block_delta","index":1,"delta":{"type":"text_delta","text":"Checking"}}),
            json!({"type":"content_block_start","index":2,"content_block":{"type":"tool_use","id":"toolu_9","name":"files_list","input":{}}}),
            json!({"type":"content_block_delta","index":2,"delta":{"type":"input_json_delta","partial_json":"{\"path\": \"~/Doc"}}),
            json!({"type":"content_block_delta","index":2,"delta":{"type":"input_json_delta","partial_json":"uments\"}"}}),
            json!({"type":"message_delta","delta":{"stop_reason":"tool_use"}}),
        ];
        let mut text = String::new();
        for e in evs.iter() {
            if let Some(t) = acc.event("", e).unwrap() {
                text += &t;
            }
        }
        let (m, stop) = acc.finish();
        assert_eq!(text, "Checking");
        assert_eq!(stop, StopReason::ToolUse);
        assert!(matches!(&m.parts[0], Part::Opaque { block, .. } if block["signature"] == "abc"));
        assert_eq!(m.tool_calls()[0].2, json!({"path": "~/Documents"}));
    }
}
