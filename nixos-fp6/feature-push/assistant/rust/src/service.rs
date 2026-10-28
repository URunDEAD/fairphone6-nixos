//! D-Bus: com.fairphone6.Assistant1 on the session bus,
//! /com/fairphone6/Assistant1. Everything is JSON strings (the UI is QML).
//!
//! Callers: read-only queries are open; driving the agent, changing
//! settings/keys and answering dialogs need a trusted caller (a program
//! from the assistant's own package: the UI, the CLI). Answering dialogs
//! is further limited to the UI that registered (RegisterUi).

use crate::agent::Agent;
use crate::permissions;
use crate::prompts::HubEvent;
use crate::settings::Settings;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::Arc;
use zbus::object_server::SignalEmitter;
use zbus::{fdo, interface};

pub const BUS: &str = "com.fairphone6.Assistant1";
pub const PATH: &str = "/com/fairphone6/Assistant1";

pub struct Service {
    pub agent: Arc<Agent>,
    /// The bin/ directory of the package (trusted callers live there).
    pub bin_dir: Option<PathBuf>,
    pub trust_any: bool,
}

fn err(e: impl std::fmt::Display) -> fdo::Error {
    fdo::Error::Failed(e.to_string())
}

async fn caller_exe(conn: &zbus::Connection, hdr: &zbus::message::Header<'_>) -> Option<PathBuf> {
    let sender = hdr.sender()?.to_owned();
    let dbus = fdo::DBusProxy::new(conn).await.ok()?;
    let pid = dbus.get_connection_unix_process_id(sender.into()).await.ok()?;
    std::fs::read_link(format!("/proc/{pid}/exe")).ok()
}

/// A program of our own package? (wrapQtAppsHook renames the real binary
/// to .<name>-wrapped in the same directory.)
pub fn trusted_exe(exe: &std::path::Path, bin_dir: &std::path::Path, ui_only: bool) -> bool {
    if exe.parent() != Some(bin_dir) {
        return false;
    }
    let name = exe.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let ui = name == "fp6-assistant" || name == ".fp6-assistant-wrapped";
    ui || (!ui_only && (name == "fp6-assistant-ctl" || name == ".fp6-assistant-ctl-wrapped"))
}

impl Service {
    async fn check(&self, conn: &zbus::Connection, hdr: &zbus::message::Header<'_>, ui_only: bool) -> fdo::Result<()> {
        if self.trust_any {
            return Ok(());
        }
        let Some(bin) = &self.bin_dir else { return Err(fdo::Error::AccessDenied("no trusted programs known".into())) };
        match caller_exe(conn, hdr).await {
            Some(exe) if trusted_exe(&exe, bin, ui_only) => Ok(()),
            _ => Err(fdo::Error::AccessDenied("only the Assistant app may do this".into())),
        }
    }

    fn settings_view(&self, s: &Settings, keys: Value, keyring: &str) -> Value {
        json!({"settings": s, "keys": keys, "keyring": keyring})
    }
}

#[interface(name = "com.fairphone6.Assistant1")]
impl Service {
    async fn register_ui(&self, #[zbus(header)] hdr: zbus::message::Header<'_>, #[zbus(connection)] conn: &zbus::Connection) -> fdo::Result<String> {
        self.check(conn, &hdr, true).await?;
        let sender = hdr.sender().map(|s| s.to_string()).unwrap_or_default();
        self.agent.hub.register_ui(&sender);
        Ok(json!({"pending": self.agent.hub.pending()}).to_string())
    }

    async fn respond(&self, id: &str, answer: &str, #[zbus(header)] hdr: zbus::message::Header<'_>) -> bool {
        let sender = hdr.sender().map(|s| s.to_string());
        let v: Value = serde_json::from_str(answer).unwrap_or(Value::String(answer.to_string()));
        self.agent.hub.respond(sender.as_deref().or(Some("")), id, v)
    }

    async fn list_chats(&self) -> String {
        let running: Vec<String> = self.agent.store.list().iter().filter(|c| self.agent.is_running(&c.id)).map(|c| c.id.clone()).collect();
        let list: Vec<Value> = self
            .agent
            .store
            .list()
            .iter()
            .map(|c| {
                let mut s = c.summary();
                s["running"] = json!(running.contains(&c.id));
                s
            })
            .collect();
        Value::Array(list).to_string()
    }

    async fn get_chat(&self, id: &str) -> fdo::Result<String> {
        let c = self.agent.store.load(id).ok_or_else(|| err("no such chat"))?;
        let mut v = c.view();
        v["running"] = json!(self.agent.is_running(id));
        Ok(v.to_string())
    }

    async fn new_chat(&self, backend: &str, #[zbus(header)] hdr: zbus::message::Header<'_>, #[zbus(connection)] conn: &zbus::Connection) -> fdo::Result<String> {
        self.check(conn, &hdr, false).await?;
        self.agent.new_chat(backend).map(|c| c.id).map_err(err)
    }

    async fn delete_chat(&self, id: &str, #[zbus(header)] hdr: zbus::message::Header<'_>, #[zbus(connection)] conn: &zbus::Connection) -> fdo::Result<bool> {
        self.check(conn, &hdr, false).await?;
        self.agent.stop(id);
        let ok = self.agent.store.delete(id);
        self.agent.hub.emit(HubEvent::ChatChanged(id.into()));
        Ok(ok)
    }

    async fn clear_history(&self, #[zbus(header)] hdr: zbus::message::Header<'_>, #[zbus(connection)] conn: &zbus::Connection) -> fdo::Result<u32> {
        self.check(conn, &hdr, false).await?;
        self.agent.stop_all();
        let n = self.agent.store.clear() as u32;
        self.agent.hub.emit(HubEvent::ChatChanged(String::new()));
        Ok(n)
    }

    async fn send(&self, chat: &str, text: &str, attachments: &str, #[zbus(header)] hdr: zbus::message::Header<'_>, #[zbus(connection)] conn: &zbus::Connection) -> fdo::Result<()> {
        self.check(conn, &hdr, false).await?;
        let att: Vec<String> = serde_json::from_str(attachments).unwrap_or_default();
        if text.trim().is_empty() && att.is_empty() {
            return Err(err("empty message"));
        }
        self.agent.start_turn(chat, text.to_string(), att).map_err(err)
    }

    /// Springboard: new chat on the default model + send. Returns its id.
    async fn quick(&self, text: &str, #[zbus(header)] hdr: zbus::message::Header<'_>, #[zbus(connection)] conn: &zbus::Connection) -> fdo::Result<String> {
        self.check(conn, &hdr, false).await?;
        let c = self.agent.new_chat("").map_err(err)?;
        self.agent.start_turn(&c.id, text.to_string(), vec![]).map_err(err)?;
        Ok(c.id)
    }

    async fn stop(&self, chat: &str) {
        // Stopping is always allowed (it only ever reduces what happens).
        self.agent.stop(chat);
    }

    async fn stop_all(&self) {
        self.agent.stop_all();
    }

    async fn get_settings(&self) -> String {
        let s = self.agent.settings();
        let mut keys = serde_json::Map::new();
        for b in &s.backends {
            keys.insert(b.secret_key(), json!(self.agent.secrets.has(&b.secret_key()).await));
        }
        for a in &s.email_accounts {
            keys.insert(a.secret_key(), json!(self.agent.secrets.has(&a.secret_key()).await));
        }
        let kr = self.agent.secrets.backend_name().await;
        self.settings_view(&s, Value::Object(keys), kr).to_string()
    }

    async fn set_settings(&self, json_text: &str, #[zbus(header)] hdr: zbus::message::Header<'_>, #[zbus(connection)] conn: &zbus::Connection) -> fdo::Result<()> {
        self.check(conn, &hdr, false).await?;
        let s: Settings = serde_json::from_str(json_text).map_err(|e| err(format!("settings: {e}")))?;
        self.agent.set_settings(s).map_err(err)?;
        let days = self.agent.settings().retention_days;
        self.agent.store.expire(days, crate::store::now());
        Ok(())
    }

    /// Store an API key / password ("backend:<id>" or "email:<id>").
    async fn set_key(&self, key: &str, value: &str, #[zbus(header)] hdr: zbus::message::Header<'_>, #[zbus(connection)] conn: &zbus::Connection) -> fdo::Result<()> {
        self.check(conn, &hdr, false).await?;
        if value.is_empty() && (key.starts_with("backend:") || key.starts_with("email:")) {
            // Deleting is fine even after the entry itself was removed.
            self.agent.secrets.delete(key).await;
            return Ok(());
        }
        let s = self.agent.settings();
        let known = s.backends.iter().any(|b| b.secret_key() == key) || s.email_accounts.iter().any(|a| a.secret_key() == key);
        if !known {
            return Err(err("unknown key name"));
        }
        self.agent.secrets.set(key, value.trim()).await.map_err(err)
    }

    async fn list_models(&self, backend: &str) -> fdo::Result<String> {
        let s = self.agent.settings();
        let b = s.backend(backend).ok_or_else(|| err("no such backend"))?;
        let chat = crate::store::Chat { backend: b.id.clone(), ..Default::default() };
        let (ep, _) = self.agent.endpoint(&chat).await.map_err(err)?;
        let models = crate::llm::list_models(&self.agent.http, &ep).await.map_err(err)?;
        Ok(json!(models).to_string())
    }

    async fn tools(&self) -> String {
        let s = self.agent.settings();
        let v: Vec<Value> = self
            .agent
            .registry
            .specs()
            .into_iter()
            .map(|t| {
                let pol = s.policy(&t.name);
                json!({"name": t.name, "group": t.group, "title": t.title, "description": t.description,
                       "side_effect": t.side_effect, "untrusted": t.untrusted_output,
                       "policy": pol.map(|p| serde_json::to_value(p).unwrap_or_default()).unwrap_or(json!(if t.side_effect { "always" } else { "default" }))})
            })
            .collect();
        Value::Array(v).to_string()
    }

    async fn grants(&self) -> String {
        let g = self.agent.always.lock().unwrap().clone();
        let v: Vec<Value> = g.grants.iter().map(|x| json!({"key": x.resource.key(), "title": x.resource.title(), "since": x.granted_at})).collect();
        let res: Vec<Value> = permissions::RESOURCES.iter().map(|r| json!({"class": r.class, "title": r.title, "explain": r.explain, "system": r.system})).collect();
        json!({"always": v, "resources": res}).to_string()
    }

    async fn revoke_grant(&self, key: &str) -> bool {
        // Revoking only ever reduces access: open to every caller.
        let ok = self.agent.always.lock().unwrap().revoke(key);
        if ok {
            self.agent.save_always();
        }
        ok
    }

    async fn revoke_chat_grant(&self, chat: &str, key: &str) -> bool {
        let Some(mut c) = self.agent.store.load(chat) else { return false };
        let before = c.grants.allowed.len();
        c.grants.allowed.retain(|r| r.key() != key);
        let ok = before != c.grants.allowed.len() && self.agent.store.save(&c).is_ok();
        self.agent.hub.emit(HubEvent::ChatChanged(chat.into()));
        ok
    }

    async fn suggest(&self, text: &str) -> String {
        let apps = crate::tools::apps::installed_apps();
        // Contacts only when Springboard contact suggestions are allowed:
        // that is an "Always allow" grant for contacts (the user's choice).
        let contacts = match crate::springboard::contact_query(text) {
            Some(q) if self.agent.always.lock().unwrap().covers(&permissions::Resource::new("contacts.read")) => {
                let db = crate::tools::messages::contacts_db();
                tokio::task::spawn_blocking(move || crate::tools::messages::search_contacts(&db, &q, 3)).await.ok().and_then(Result::ok).unwrap_or_default()
            }
            _ => vec![],
        };
        let s = self.agent.settings();
        Value::Array(crate::springboard::suggest(text, &apps, &contacts, &s.web_search_url)).to_string()
    }

    async fn local_status(&self) -> String {
        match &self.agent.session {
            Some(c) => crate::llm::local::status(c).await,
            None => String::new(),
        }
    }

    async fn pending(&self) -> String {
        Value::Array(self.agent.hub.pending()).to_string()
    }

    /// The UI closed: free the on-device model's RAM if configured.
    async fn ui_closed(&self) {
        if self.agent.settings().unload_on_close {
            if let Some(c) = &self.agent.session {
                crate::llm::local::unload(c).await;
            }
        }
    }

    #[zbus(signal)]
    pub async fn prompt(e: &SignalEmitter<'_>, json: &str) -> zbus::Result<()>;
    #[zbus(signal)]
    pub async fn prompt_closed(e: &SignalEmitter<'_>, id: &str) -> zbus::Result<()>;
    #[zbus(signal)]
    pub async fn delta(e: &SignalEmitter<'_>, chat: &str, text: &str) -> zbus::Result<()>;
    #[zbus(signal)]
    pub async fn chat_changed(e: &SignalEmitter<'_>, chat: &str) -> zbus::Result<()>;
    #[zbus(signal)]
    pub async fn activity(e: &SignalEmitter<'_>, chat: &str, json: &str) -> zbus::Result<()>;
    #[zbus(signal)]
    pub async fn turn_state(e: &SignalEmitter<'_>, chat: &str, state: &str) -> zbus::Result<()>;
}

/// Forward hub events to D-Bus signals.
pub async fn pump(conn: zbus::Connection, agent: Arc<Agent>) {
    let mut rx = agent.hub.subscribe();
    let Ok(iface) = conn.object_server().interface::<_, Service>(PATH).await else { return };
    let e = iface.signal_emitter();
    loop {
        let ev = match rx.recv().await {
            Ok(ev) => ev,
            Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
            Err(_) => return,
        };
        let _ = match ev {
            HubEvent::Prompt(p) => Service::prompt(e, &p.to_string()).await,
            HubEvent::PromptClosed(id) => Service::prompt_closed(e, &id).await,
            HubEvent::Delta { chat, text } => Service::delta(e, &chat, &text).await,
            HubEvent::ChatChanged(c) => Service::chat_changed(e, &c).await,
            HubEvent::Activity { chat, entry } => Service::activity(e, &chat, &entry.to_string()).await,
            HubEvent::TurnState { chat, state } => Service::turn_state(e, &chat, &state).await,
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn trusted_callers() {
        let bin = std::path::Path::new("/nix/store/x-fp6-assistant/bin");
        assert!(trusted_exe(&bin.join(".fp6-assistant-wrapped"), bin, true));
        assert!(trusted_exe(&bin.join("fp6-assistant-ctl"), bin, false));
        assert!(!trusted_exe(&bin.join("fp6-assistant-ctl"), bin, true));
        assert!(!trusted_exe(std::path::Path::new("/nix/store/y-other/bin/fp6-assistant"), bin, true));
        assert!(!trusted_exe(&bin.join("bash"), bin, false));
    }
}
