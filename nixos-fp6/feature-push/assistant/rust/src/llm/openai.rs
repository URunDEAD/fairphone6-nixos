//! OpenAI-compatible chat completions (streaming, function calling).
//! Works with llama-server (--jinja), Ollama's /v1, vLLM, LM Studio and
//! hosted OpenAI-compatible services.

use super::{sse, Endpoint, Event, LlmError, Message, Part, Request, Role, StopReason};
use serde_json::{json, Value};
use tokio::sync::mpsc;

/// `base_url` may be given with or without the trailing /v1.
pub fn api_url(base: &str, path: &str) -> String {
    let b = base.trim_end_matches('/');
    if b.ends_with("/v1") {
        format!("{b}/{path}")
    } else {
        format!("{b}/v1/{path}")
    }
}

pub fn body(req: &Request) -> Value {
    body_with(req, true)
}

/// `images: false` for servers that reject multimodal parts (the on-device
/// runtime): images become a short note.
pub fn body_with(req: &Request, images: bool) -> Value {
    let mut messages = vec![json!({"role": "system", "content": req.system})];
    for m in &req.messages {
        match m.role {
            Role::User => {
                let has_image = images && m.parts.iter().any(|p| matches!(p, Part::Image { .. }));
                if has_image {
                    let content: Vec<Value> = m
                        .parts
                        .iter()
                        .filter_map(|p| match p {
                            Part::Text { text } => Some(json!({"type": "text", "text": text})),
                            Part::Image { mime, data } => Some(json!({"type": "image_url", "image_url": {"url": format!("data:{mime};base64,{data}")}})),
                            _ => None,
                        })
                        .collect();
                    messages.push(json!({"role": "user", "content": content}));
                } else {
                    let mut t = m.text();
                    if m.parts.iter().any(|p| matches!(p, Part::Image { .. })) {
                        t.push_str("\n[An image is attached, but this model cannot see images.]");
                    }
                    messages.push(json!({"role": "user", "content": t}));
                }
            }
            Role::Assistant => {
                let calls: Vec<Value> = m
                    .tool_calls()
                    .into_iter()
                    .map(|(id, name, args)| {
                        json!({"id": id, "type": "function", "function": {"name": name, "arguments": args.to_string()}})
                    })
                    .collect();
                let mut msg = json!({"role": "assistant", "content": m.text()});
                if !calls.is_empty() {
                    msg["tool_calls"] = Value::Array(calls);
                }
                messages.push(msg);
            }
            Role::Tool => {
                for p in &m.parts {
                    if let Part::ToolResult { id, content, .. } = p {
                        messages.push(json!({"role": "tool", "tool_call_id": id, "content": content}));
                    }
                }
                // Tool messages carry text only: images go in a user turn.
                let images_ok = images;
                let images: Vec<Value> = m
                    .parts
                    .iter()
                    .filter_map(|p| match p {
                        Part::Image { mime, data } => Some(json!({"type": "image_url", "image_url": {"url": format!("data:{mime};base64,{data}")}})),
                        _ => None,
                    })
                    .collect();
                if !images.is_empty() && !images_ok {
                    messages.push(json!({"role": "user", "content": "(The tool returned an image; this model cannot see images. Use ocr_image to read text in it.)"}));
                } else if !images.is_empty() {
                    let mut content = vec![json!({"type": "text", "text": "(Image returned by the tool above.)"})];
                    content.extend(images);
                    messages.push(json!({"role": "user", "content": content}));
                }
            }
        }
    }
    let mut body = json!({
        "model": req.model,
        "messages": messages,
        "stream": true,
        "max_tokens": req.max_tokens,
    });
    if !req.tools.is_empty() {
        body["tools"] = Value::Array(
            req.tools
                .iter()
                .map(|t| json!({"type": "function", "function": {"name": t.name, "description": t.description, "parameters": t.parameters}}))
                .collect(),
        );
        body["tool_choice"] = json!("auto");
    }
    body
}

#[derive(Default)]
struct PartialCall {
    id: String,
    name: String,
    args: String,
}

/// Accumulates streamed chunks into the final assistant message.
#[derive(Default)]
pub struct Accumulator {
    text: String,
    calls: Vec<PartialCall>,
    finish: Option<String>,
}

impl Accumulator {
    /// Returns the text delta (if any) contained in one chunk.
    pub fn chunk(&mut self, v: &Value) -> Option<String> {
        let choice = v.get("choices")?.get(0)?;
        if let Some(f) = choice.get("finish_reason").and_then(Value::as_str) {
            self.finish = Some(f.to_string());
        }
        let delta = choice.get("delta").or_else(|| choice.get("message"))?;
        if let Some(calls) = delta.get("tool_calls").and_then(Value::as_array) {
            for (n, c) in calls.iter().enumerate() {
                let idx = c.get("index").and_then(Value::as_u64).map(|i| i as usize).unwrap_or(n);
                while self.calls.len() <= idx {
                    self.calls.push(PartialCall::default());
                }
                let pc = &mut self.calls[idx];
                if let Some(id) = c.get("id").and_then(Value::as_str) {
                    pc.id = id.to_string();
                }
                if let Some(f) = c.get("function") {
                    if let Some(name) = f.get("name").and_then(Value::as_str) {
                        pc.name.push_str(name);
                    }
                    match f.get("arguments") {
                        Some(Value::String(a)) => pc.args.push_str(a),
                        Some(obj @ Value::Object(_)) => pc.args = obj.to_string(),
                        _ => {}
                    }
                }
            }
        }
        let t = delta.get("content").and_then(Value::as_str)?;
        if t.is_empty() {
            return None;
        }
        self.text.push_str(t);
        Some(t.to_string())
    }

    pub fn finish(self) -> (Message, StopReason) {
        let mut parts = Vec::new();
        if !self.text.is_empty() {
            parts.push(Part::Text { text: self.text });
        }
        let has_calls = self.calls.iter().any(|c| !c.name.is_empty());
        for (i, c) in self.calls.into_iter().enumerate() {
            if c.name.is_empty() {
                continue;
            }
            let args = if c.args.trim().is_empty() {
                json!({})
            } else {
                serde_json::from_str(&c.args).unwrap_or(Value::String(c.args))
            };
            let id = if c.id.is_empty() { format!("call_{i}") } else { c.id };
            parts.push(Part::ToolCall { id, name: c.name, arguments: args });
        }
        let stop = match self.finish.as_deref() {
            _ if has_calls => StopReason::ToolUse,
            Some("stop") | None => StopReason::EndTurn,
            Some("length") => StopReason::MaxTokens,
            Some("content_filter") => StopReason::Refusal,
            Some(o) => StopReason::Other(o.to_string()),
        };
        (Message { role: Role::Assistant, parts }, stop)
    }
}

fn auth(rb: reqwest::RequestBuilder, ep: &Endpoint) -> reqwest::RequestBuilder {
    match &ep.api_key {
        Some(k) if !k.is_empty() => rb.bearer_auth(k),
        _ => rb,
    }
}

pub async fn stream(client: &reqwest::Client, ep: &Endpoint, req: &Request, tx: mpsc::Sender<Event>) -> Result<(), LlmError> {
    let resp = auth(client.post(api_url(&ep.base_url, "chat/completions")), ep)
        .json(&body_with(req, ep.kind != super::Kind::Local))
        .send()
        .await
        .map_err(|e| LlmError::Http(e.to_string()))?;
    let status = resp.status();
    if !status.is_success() {
        let text = resp.text().await.unwrap_or_default();
        return Err(LlmError::Status(status.as_u16(), text));
    }
    let is_sse = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(|v| v.contains("event-stream"))
        .unwrap_or(false);
    let mut acc = Accumulator::default();
    if !is_sse {
        // Some servers ignore "stream": true for tool requests.
        let v: Value = resp.json().await.map_err(|e| LlmError::Protocol(e.to_string()))?;
        if let Some(t) = acc.chunk(&v) {
            let _ = tx.send(Event::TextDelta(t)).await;
        }
    } else {
        let mut events = sse::SseStream::new(resp);
        while let Some(ev) = events.next().await {
            let ev = ev?;
            if ev.data.trim() == "[DONE]" {
                break;
            }
            if ev.data.trim().is_empty() {
                continue;
            }
            let v: Value = serde_json::from_str(&ev.data).map_err(|e| LlmError::Protocol(format!("{e}: {}", super::truncate(&ev.data, 120))))?;
            if let Some(err) = v.get("error") {
                return Err(LlmError::Protocol(err.to_string()));
            }
            if let Some(t) = acc.chunk(&v) {
                let _ = tx.send(Event::TextDelta(t)).await;
            }
        }
    }
    let (message, stop) = acc.finish();
    let _ = tx.send(Event::Done { message, stop }).await;
    Ok(())
}

pub async fn list_models(client: &reqwest::Client, ep: &Endpoint) -> Result<Vec<String>, LlmError> {
    let resp = auth(client.get(api_url(&ep.base_url, "models")), ep).send().await.map_err(|e| LlmError::Http(e.to_string()))?;
    if !resp.status().is_success() {
        return Err(LlmError::Status(resp.status().as_u16(), String::new()));
    }
    let v: Value = resp.json().await.map_err(|e| LlmError::Protocol(e.to_string()))?;
    Ok(v.get("data")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(|m| m.get("id").and_then(Value::as_str).map(String::from)).collect())
        .unwrap_or_default())
}

pub async fn list_ollama_models(client: &reqwest::Client, ep: &Endpoint) -> Result<Vec<String>, LlmError> {
    let base = ep.base_url.trim_end_matches('/').trim_end_matches("/v1");
    let resp = client.get(format!("{base}/api/tags")).send().await.map_err(|e| LlmError::Http(e.to_string()))?;
    let v: Value = resp.json().await.map_err(|e| LlmError::Protocol(e.to_string()))?;
    Ok(v.get("models")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(|m| m.get("name").and_then(Value::as_str).map(String::from)).collect())
        .unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::ToolDef;

    #[test]
    fn urls() {
        assert_eq!(api_url("http://127.0.0.1:8080", "chat/completions"), "http://127.0.0.1:8080/v1/chat/completions");
        assert_eq!(api_url("http://h:11434/v1/", "models"), "http://h:11434/v1/models");
    }

    #[test]
    fn body_shapes_tools_and_results() {
        let req = Request {
            model: "m".into(),
            system: "sys".into(),
            messages: vec![
                Message::user_text("hi"),
                Message { role: Role::Assistant, parts: vec![Part::ToolCall { id: "c1".into(), name: "clock_now".into(), arguments: json!({}) }] },
                Message { role: Role::Tool, parts: vec![Part::ToolResult { id: "c1".into(), name: "clock_now".into(), content: "12:00".into(), is_error: false }] },
            ],
            tools: vec![ToolDef { name: "clock_now".into(), description: "time".into(), parameters: json!({"type": "object", "properties": {}}) }],
            max_tokens: 100,
        };
        let b = body(&req);
        assert_eq!(b["messages"][0]["role"], "system");
        assert_eq!(b["messages"][2]["tool_calls"][0]["function"]["arguments"], "{}");
        assert_eq!(b["messages"][3]["role"], "tool");
        assert_eq!(b["messages"][3]["tool_call_id"], "c1");
        assert_eq!(b["tools"][0]["function"]["name"], "clock_now");
    }

    #[test]
    fn accumulates_streamed_tool_calls() {
        let mut acc = Accumulator::default();
        for chunk in [
            json!({"choices":[{"delta":{"content":"Let me "}}]}),
            json!({"choices":[{"delta":{"content":"check."}}]}),
            json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_a","function":{"name":"messages_search","arguments":"{\"que"}}]}}]}),
            json!({"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"ry\":\"bus\"}"}}]}}]}),
            json!({"choices":[{"delta":{},"finish_reason":"tool_calls"}]}),
        ] {
            acc.chunk(&chunk);
        }
        let (m, stop) = acc.finish();
        assert_eq!(stop, StopReason::ToolUse);
        assert_eq!(m.text(), "Let me check.");
        assert_eq!(m.tool_calls(), vec![("call_a".to_string(), "messages_search".to_string(), json!({"query": "bus"}))]);
    }
}
