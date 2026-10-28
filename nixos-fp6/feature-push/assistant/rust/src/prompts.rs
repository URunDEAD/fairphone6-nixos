//! The link between the agent and the Assistant UI: permission dialogs,
//! action confirmations and UI-hosted operations (camera, calendar,
//! alarms, broker checks) are "prompts" the daemon publishes as D-Bus
//! signals; the UI answers with Respond(id, answer). Only the UI that
//! registered (RegisterUi) may answer. Unanswered prompts time out as a
//! denial. Streaming text, activity and state changes go out the same way.

use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::{broadcast, oneshot};

#[derive(Clone, Debug, PartialEq)]
pub enum HubEvent {
    Prompt(Value),
    PromptClosed(String),
    Delta { chat: String, text: String },
    ChatChanged(String),
    Activity { chat: String, entry: Value },
    TurnState { chat: String, state: String },
}

#[derive(Default)]
struct Inner {
    pending: HashMap<String, (Value, oneshot::Sender<Value>)>,
    /// Unique bus name of the registered UI.
    ui: Option<String>,
}

#[derive(Clone)]
pub struct PromptHub {
    inner: Arc<Mutex<Inner>>,
    events: broadcast::Sender<HubEvent>,
    seq: Arc<AtomicU64>,
    /// When no UI is registered: called to bring it up (notification).
    summon: Arc<Mutex<Option<Box<dyn Fn(&Value) + Send + Sync>>>>,
}

pub type UiLink = PromptHub;

impl Default for PromptHub {
    fn default() -> Self {
        Self::new()
    }
}

impl PromptHub {
    pub fn new() -> Self {
        let (events, _) = broadcast::channel(512);
        PromptHub { inner: Default::default(), events, seq: Arc::new(AtomicU64::new(1)), summon: Default::default() }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<HubEvent> {
        self.events.subscribe()
    }

    pub fn emit(&self, e: HubEvent) {
        let _ = self.events.send(e);
    }

    pub fn set_summon(&self, f: Box<dyn Fn(&Value) + Send + Sync>) {
        *self.summon.lock().unwrap() = Some(f);
    }

    pub fn register_ui(&self, name: &str) {
        self.inner.lock().unwrap().ui = Some(name.to_string());
        // A UI that comes up re-receives what is waiting.
        let pending: Vec<Value> = self.inner.lock().unwrap().pending.values().map(|(p, _)| p.clone()).collect();
        for p in pending {
            self.emit(HubEvent::Prompt(p));
        }
    }

    pub fn ui_gone(&self, name: &str) {
        let mut g = self.inner.lock().unwrap();
        if g.ui.as_deref() == Some(name) {
            g.ui = None;
        }
    }

    pub fn ui_present(&self) -> bool {
        self.inner.lock().unwrap().ui.is_some()
    }

    pub fn is_ui(&self, sender: &str) -> bool {
        self.inner.lock().unwrap().ui.as_deref() == Some(sender)
    }

    pub fn pending(&self) -> Vec<Value> {
        self.inner.lock().unwrap().pending.values().map(|(p, _)| p.clone()).collect()
    }

    /// Publish a prompt and wait for the answer (Value::Null on timeout or
    /// when the asking future is dropped: callers treat that as "deny").
    pub async fn ask(&self, mut prompt: Value, timeout: Duration) -> Value {
        let id = format!("p{}", self.seq.fetch_add(1, Ordering::Relaxed));
        prompt["id"] = json!(id);
        let (tx, rx) = oneshot::channel();
        self.inner.lock().unwrap().pending.insert(id.clone(), (prompt.clone(), tx));
        struct Close<'a>(&'a PromptHub, String);
        impl Drop for Close<'_> {
            fn drop(&mut self) {
                if self.0.inner.lock().unwrap().pending.remove(&self.1).is_some() {
                    self.0.emit(HubEvent::PromptClosed(self.1.clone()));
                }
            }
        }
        let _close = Close(self, id.clone());
        if !self.ui_present() {
            if let Some(f) = self.summon.lock().unwrap().as_ref() {
                f(&prompt);
            }
        }
        self.emit(HubEvent::Prompt(prompt));
        match tokio::time::timeout(timeout, rx).await {
            Ok(Ok(v)) => v,
            _ => Value::Null,
        }
    }

    /// The UI's answer. False when the id is unknown (already answered,
    /// timed out) or the caller is not the registered UI.
    pub fn respond(&self, sender: Option<&str>, id: &str, answer: Value) -> bool {
        let mut g = self.inner.lock().unwrap();
        if let Some(s) = sender {
            if g.ui.as_deref() != Some(s) {
                return false;
            }
        }
        match g.pending.remove(id) {
            Some((_, tx)) => {
                drop(g);
                let _ = tx.send(answer);
                self.emit(HubEvent::PromptClosed(id.to_string()));
                true
            }
            None => false,
        }
    }

    /// Cancel every prompt of a chat (Stop).
    pub fn cancel_chat(&self, chat: &str) {
        let ids: Vec<String> = self
            .inner
            .lock()
            .unwrap()
            .pending
            .iter()
            .filter(|(_, (p, _))| p["chat"] == chat)
            .map(|(k, _)| k.clone())
            .collect();
        for id in ids {
            self.respond(None, &id, Value::Null);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn only_registered_ui_answers() {
        let hub = PromptHub::new();
        hub.register_ui(":1.42");
        let mut rx = hub.subscribe();
        let h2 = hub.clone();
        let task = tokio::spawn(async move { h2.ask(json!({"kind": "permission", "chat": "c"}), Duration::from_secs(5)).await });
        let id = loop {
            if let HubEvent::Prompt(p) = rx.recv().await.unwrap() {
                break p["id"].as_str().unwrap().to_string();
            }
        };
        assert!(!hub.respond(Some(":1.99"), &id, json!("always")));
        assert!(hub.respond(Some(":1.42"), &id, json!("once")));
        assert_eq!(task.await.unwrap(), json!("once"));
        assert!(!hub.respond(Some(":1.42"), &id, json!("once")));
    }

    #[tokio::test]
    async fn timeout_and_cancel_are_null() {
        let hub = PromptHub::new();
        assert_eq!(hub.ask(json!({"chat": "c"}), Duration::from_millis(20)).await, Value::Null);
        assert!(hub.pending().is_empty());
        let h2 = hub.clone();
        let task = tokio::spawn(async move { h2.ask(json!({"chat": "c"}), Duration::from_secs(5)).await });
        tokio::time::sleep(Duration::from_millis(20)).await;
        hub.cancel_chat("c");
        assert_eq!(task.await.unwrap(), Value::Null);
    }
}
