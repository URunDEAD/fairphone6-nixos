//! Notification history: fp6-notifyd's history.jsonl (only when History
//! is on in Settings > Notifications), else its status snapshot.

use super::{opt_u64, schema, spec, Ctx, Output, Tool, ToolSpec};
use crate::permissions::Resource;
use crate::settings::Settings;
use async_trait::async_trait;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::Arc;

pub fn tools() -> Vec<Arc<dyn Tool>> {
    vec![Arc::new(Recent)]
}

pub fn history_file() -> PathBuf {
    if let Ok(p) = std::env::var("FP6_ASSISTANT_NOTIFY_HISTORY") {
        return p.into();
    }
    let data = std::env::var("XDG_DATA_HOME").map(PathBuf::from).unwrap_or_else(|_| PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".local/share"));
    data.join("fp6-notifyd/history.jsonl")
}

pub fn read_history(text: &str, limit: usize, app: Option<&str>) -> Vec<Value> {
    let mut v: Vec<Value> = text
        .lines()
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .filter(|e| app.map(|a| e["app"].as_str().map(|x| x.to_lowercase().contains(&a.to_lowercase())).unwrap_or(false)).unwrap_or(true))
        .map(|e| json!({"time": e["t"], "app": e["app"], "title": e["summary"], "text": e.get("body").cloned().unwrap_or(Value::Null)}))
        .collect();
    let n = v.len();
    v.drain(..n.saturating_sub(limit));
    v.reverse();
    v
}

struct Recent;
#[async_trait]
impl Tool for Recent {
    fn spec(&self) -> ToolSpec {
        spec(
            "notifications_recent",
            "Notifications",
            "Recent notifications",
            "Read the recent notifications (app, title, text; newest first). Works when notification history is on.",
            schema(&[("limit", "integer", "How many (default 20, max 100)", false), ("app", "string", "Only this app", false)]),
            false,
            true,
        )
    }
    fn resources(&self, _a: &Value, _s: &Settings) -> Result<Vec<Resource>, String> {
        Ok(vec![Resource::new("notifications.read")])
    }
    async fn run(&self, a: Value, _ctx: &Ctx) -> Result<Output, String> {
        let limit = opt_u64(&a, "limit", 20, 100) as usize;
        match std::fs::read_to_string(history_file()) {
            Ok(t) => Ok(Output::json(&json!(read_history(&t, limit, a.get("app").and_then(Value::as_str))))),
            Err(_) => Ok(Output::text("Notification history is off. The user can turn it on in System Settings > Notifications > History.")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn newest_first_and_filter() {
        let t = "{\"t\":1,\"app\":\"Messages\",\"summary\":\"Test Contact\",\"body\":\"hi\"}\nnot json\n{\"t\":2,\"app\":\"Clock\",\"summary\":\"Alarm\"}\n";
        let v = read_history(t, 10, None);
        assert_eq!(v[0]["time"], 2);
        assert_eq!(read_history(t, 10, Some("mess")).len(), 1);
        assert_eq!(read_history(t, 1, None).len(), 1);
    }
}
