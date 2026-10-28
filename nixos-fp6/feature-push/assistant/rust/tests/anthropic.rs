//! Anthropic Messages API against a fake server: headers, streaming,
//! tool use round trip, thinking blocks passed back, the key never stored
//! in chats or logs.

mod common;
use common::*;
use serde_json::{json, Value};
use std::sync::Arc;

fn ev(v: Value) -> String {
    format!("event: {}\ndata: {}", v["type"].as_str().unwrap(), v)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn anthropic_tool_round_trip() {
    let script: Script = Arc::new(|n, req| {
        assert_eq!(req.path, "/v1/messages");
        assert_eq!(req.headers.get("anthropic-version").map(String::as_str), Some("2023-06-01"));
        assert_eq!(req.headers.get("x-api-key").map(String::as_str), Some("test-key-not-real"));
        assert_eq!(req.body["model"], "claude-haiku-5-5");
        assert!(req.body.get("thinking").is_none());
        if n == 0 {
            sse(&[
                ev(json!({"type": "message_start", "message": {"id": "msg_1"}})),
                ev(json!({"type": "content_block_start", "index": 0, "content_block": {"type": "thinking", "thinking": "", "signature": ""}})),
                ev(json!({"type": "content_block_delta", "index": 0, "delta": {"type": "signature_delta", "signature": "sig-test"}})),
                ev(json!({"type": "content_block_stop", "index": 0})),
                ev(json!({"type": "content_block_start", "index": 1, "content_block": {"type": "text", "text": ""}})),
                ev(json!({"type": "content_block_delta", "index": 1, "delta": {"type": "text_delta", "text": "Checking the time."}})),
                ev(json!({"type": "content_block_start", "index": 2, "content_block": {"type": "tool_use", "id": "toolu_1", "name": "phone_status", "input": {}}})),
                ev(json!({"type": "content_block_delta", "index": 2, "delta": {"type": "input_json_delta", "partial_json": "{}"}})),
                ev(json!({"type": "message_delta", "delta": {"stop_reason": "tool_use"}})),
                ev(json!({"type": "message_stop"})),
            ])
        } else {
            let msgs = req.body["messages"].as_array().unwrap();
            assert_eq!(msgs[1]["content"][0]["signature"], "sig-test", "thinking block passed back unchanged");
            assert_eq!(msgs[2]["content"][0]["type"], "tool_result");
            assert_eq!(msgs[2]["content"][0]["tool_use_id"], "toolu_1");
            sse(&[
                ev(json!({"type": "message_start", "message": {"id": "msg_2"}})),
                ev(json!({"type": "content_block_start", "index": 0, "content_block": {"type": "text", "text": ""}})),
                ev(json!({"type": "content_block_delta", "index": 0, "delta": {"type": "text_delta", "text": "It is morning."}})),
                ev(json!({"type": "message_delta", "delta": {"stop_reason": "end_turn"}})),
                ev(json!({"type": "message_stop"})),
            ])
        }
    });
    let (url, reqs) = model_server(script).await;
    let e = env(None, yes_user());
    let mut s = e.agent.settings();
    let b = s.backends.iter_mut().find(|b| b.id == "anthropic").unwrap();
    // The fake server is http://127.0.0.1; settings validation wants https
    // for Anthropic, so bypass it here (tests only).
    b.base_url = url.clone();
    b.model = "claude-haiku-5-5".into();
    *e.agent.settings.write().unwrap() = s;
    e.agent.secrets.set("backend:anthropic", "test-key-not-real").await.unwrap();
    let chat = e.agent.new_chat("anthropic").unwrap();
    let st = turn(&e.agent, &chat.id, "What time is it?").await;
    assert_eq!(st, "idle");
    let c = e.agent.store.load(&chat.id).unwrap();
    assert_eq!(c.messages.last().unwrap().text(), "It is morning.");
    assert_eq!(reqs.lock().unwrap().len(), 2);
    // The key is in the keyring/credentials file only.
    let chat_file = std::fs::read_to_string(e.dir.path().join(format!("data/chats/{}.json", chat.id))).unwrap();
    assert!(!chat_file.contains("test-key-not-real"));
    let settings_file = std::fs::read_to_string(e.dir.path().join("config/settings.json")).unwrap_or_default();
    assert!(!settings_file.contains("test-key-not-real"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn http_errors_surface() {
    let script: Script = Arc::new(|_n, _req| "HTTP/1.1 401 Unauthorized\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n{\"type\":\"error\",\"error\":{\"type\":\"authentication_error\",\"message\":\"invalid x-api-key\"}}".to_string());
    let (url, _reqs) = model_server(script).await;
    let e = env(None, yes_user());
    let mut s = e.agent.settings();
    s.backends.iter_mut().find(|b| b.id == "anthropic").unwrap().base_url = url;
    *e.agent.settings.write().unwrap() = s;
    e.agent.secrets.set("backend:anthropic", "test-key-not-real").await.unwrap();
    let chat = e.agent.new_chat("anthropic").unwrap();
    let st = turn(&e.agent, &chat.id, "hi").await;
    assert!(st.contains("401") && st.contains("invalid x-api-key"), "{st}");
    assert!(!st.contains("test-key-not-real"));
}
