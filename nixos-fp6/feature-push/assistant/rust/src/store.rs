//! Chats on disk: ~/.local/share/fp6-assistant/chats/<id>.json (0600).
//! A chat keeps the model history, the activity log of every tool call
//! (arguments, decision, result) and the grants that live as long as it.

use crate::llm::{Message, Part, Role};
use crate::permissions::ChatGrants;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::PathBuf;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Activity {
    pub t: u64,
    pub tool: String,
    pub args: Value,
    /// Resource keys the call needed.
    pub resources: Vec<String>,
    /// "allowed (chat)", "confirmed", "denied by you", "blocked: ...".
    pub decision: String,
    pub ok: bool,
    /// The result as the model saw it (truncated).
    pub result: String,
    pub ms: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct Chat {
    pub id: String,
    pub title: String,
    pub created: u64,
    pub updated: u64,
    pub backend: String,
    pub model: String,
    /// Data leaves the phone (cloud model / LAN machine).
    pub remote: bool,
    /// Who receives it ("Anthropic (api.anthropic.com)").
    pub provider: String,
    pub messages: Vec<Message>,
    pub activity: Vec<Activity>,
    pub grants: ChatGrants,
    /// Attachment paths per user message index (shown in the UI).
    pub attachments: Vec<(usize, Vec<String>)>,
    /// Last error shown under the conversation.
    pub error: String,
}

pub fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

pub fn new_id() -> String {
    let mut b = [0u8; 8];
    if let Ok(mut f) = std::fs::File::open("/dev/urandom") {
        use std::io::Read;
        let _ = f.read_exact(&mut b);
    }
    format!("{:x}-{}", now(), b.iter().map(|x| format!("{x:02x}")).collect::<String>())
}

fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() < 64 && id.chars().all(|c| c.is_ascii_hexdigit() || c == '-')
}

impl Chat {
    pub fn summary(&self) -> Value {
        let last = self.messages.iter().rev().find(|m| m.role != Role::Tool && !m.text().is_empty()).map(|m| m.text()).unwrap_or_default();
        json!({
            "id": self.id, "title": self.title, "updated": self.updated, "backend": self.backend,
            "model": self.model, "remote": self.remote, "provider": self.provider,
            "preview": crate::llm::truncate(&last.replace('\n', " "), 90),
        })
    }

    /// What the chat page shows: user/assistant bubbles and tool chips.
    pub fn view(&self) -> Value {
        let mut items = Vec::new();
        for (i, m) in self.messages.iter().enumerate() {
            match m.role {
                Role::User => {
                    let att = self.attachments.iter().find(|(idx, _)| *idx == i).map(|(_, a)| a.clone()).unwrap_or_default();
                    items.push(json!({"kind": "user", "text": m.text(), "attachments": att}));
                }
                Role::Assistant => {
                    let t = m.text();
                    if !t.trim().is_empty() {
                        items.push(json!({"kind": "assistant", "text": t}));
                    }
                    for p in &m.parts {
                        if let Part::ToolCall { id, name, .. } = p {
                            items.push(json!({"kind": "tool", "id": id, "name": name}));
                        }
                    }
                }
                Role::Tool => {}
            }
        }
        json!({
            "id": self.id, "title": self.title, "backend": self.backend, "model": self.model,
            "remote": self.remote, "provider": self.provider, "items": items,
            "activity": self.activity, "error": self.error,
            "grants": self.grants.allowed.iter().map(|r| json!({"key": r.key(), "title": r.title()})).collect::<Vec<_>>(),
        })
    }

    /// A stopped turn can leave tool calls without results; the APIs
    /// reject that, so close them.
    pub fn repair(&mut self) {
        let mut fixed: Vec<Message> = Vec::new();
        let n = self.messages.len();
        for i in 0..n {
            let m = self.messages[i].clone();
            let calls = if m.role == Role::Assistant { m.tool_calls() } else { vec![] };
            fixed.push(m);
            if !calls.is_empty() {
                let next_is_results = self.messages.get(i + 1).map(|x| x.role == Role::Tool).unwrap_or(false);
                if !next_is_results {
                    fixed.push(Message {
                        role: Role::Tool,
                        parts: calls
                            .into_iter()
                            .map(|(id, name, _)| Part::ToolResult { id, name, content: "Not run: the user stopped the assistant.".into(), is_error: true })
                            .collect(),
                    });
                }
            }
        }
        self.messages = fixed;
    }
}

pub struct Store {
    dir: PathBuf,
}

impl Store {
    pub fn new(dir: PathBuf) -> Self {
        Store { dir }
    }
    fn path(&self, id: &str) -> Option<PathBuf> {
        valid_id(id).then(|| self.dir.join(format!("{id}.json")))
    }
    pub fn load(&self, id: &str) -> Option<Chat> {
        let p = self.path(id)?;
        let b = std::fs::read(p).ok()?;
        serde_json::from_slice(&b).ok()
    }
    pub fn save(&self, chat: &Chat) -> Result<(), String> {
        let p = self.path(&chat.id).ok_or("bad chat id")?;
        crate::settings::save_json(&p, chat).map_err(|e| e.to_string())
    }
    pub fn delete(&self, id: &str) -> bool {
        self.path(id).map(|p| std::fs::remove_file(p).is_ok()).unwrap_or(false)
    }
    pub fn list(&self) -> Vec<Chat> {
        let mut out: Vec<Chat> = std::fs::read_dir(&self.dir)
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|e| {
                let name = e.file_name().to_string_lossy().to_string();
                let id = name.strip_suffix(".json")?.to_string();
                self.load(&id)
            })
            .collect();
        out.sort_by(|a, b| b.updated.cmp(&a.updated));
        out
    }
    pub fn clear(&self) -> usize {
        let chats = self.list();
        chats.iter().filter(|c| self.delete(&c.id)).count()
    }
    /// Retention: drop chats untouched for `days` (0 = keep all).
    pub fn expire(&self, days: u32, now: u64) -> usize {
        if days == 0 {
            return 0;
        }
        let limit = now.saturating_sub(days as u64 * 86400);
        self.list().iter().filter(|c| c.updated < limit && self.delete(&c.id)).count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn save_list_expire_clear() {
        let d = tempfile::tempdir().unwrap();
        let s = Store::new(d.path().join("chats"));
        let mut a = Chat { id: new_id(), title: "A".into(), updated: 100, ..Default::default() };
        s.save(&a).unwrap();
        let b = Chat { id: new_id(), title: "B".into(), updated: now(), ..Default::default() };
        s.save(&b).unwrap();
        assert_eq!(s.list()[0].title, "B");
        assert_eq!(s.expire(1, now()), 1);
        assert!(s.load(&a.id).is_none());
        a.updated = now();
        s.save(&a).unwrap();
        assert_eq!(s.clear(), 2);
        assert!(s.load("../../etc/passwd").is_none());
    }

    #[test]
    fn repair_closes_dangling_calls() {
        let mut c = Chat::default();
        c.messages.push(Message::user_text("x"));
        c.messages.push(Message { role: Role::Assistant, parts: vec![Part::ToolCall { id: "1".into(), name: "t".into(), arguments: json!({}) }] });
        c.repair();
        assert_eq!(c.messages.len(), 3);
        assert_eq!(c.messages[2].role, Role::Tool);
        c.repair();
        assert_eq!(c.messages.len(), 3);
    }
}
