#![allow(non_snake_case)]
//! fp6-assistant: the Lomiri UI (UITK 1.3 QML) and its bridge to
//! fp6-assistantd. qmetaobject's derive macros generate the Qt glue; the
//! handwritten code is safe Rust.
//!
//! The bridge (`assistant` in QML):
//!   request(token, method, argsJson)   -> reply(token, result, error)
//!     daemon methods with string arguments; plus, run in this process
//!     (so the broker and the dictation service see the Assistant app):
//!     "syscheck"  [permission]  -> granted | denied | absent  (fp6-permd)
//!     "dictationStart" / "dictationStop" / "dictationCancel"
//!   event(kind, json)                  daemon signals (Prompt, Delta, ...)
//!   initialUrl, previewScene           launch URL / offscreen preview

use qmetaobject::prelude::*;
use qmetaobject::{queued_callback, QPointer};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use std::time::Duration;

const BUS: &str = "com.fairphone6.Assistant1";
const PATH: &str = "/com/fairphone6/Assistant1";

type EventCb = Arc<dyn Fn((String, String)) + Send + Sync>;
type ReplyCb = Arc<dyn Fn((String, String, String)) + Send + Sync>;

#[derive(QObject, Default)]
struct Bridge {
    base: qt_base_class!(trait QObject),
    initialUrl: qt_property!(QString; CONST),
    previewScene: qt_property!(QString; CONST),
    screenshotPath: qt_property!(QString; CONST),
    event: qt_signal!(kind: QString, json: QString),
    reply: qt_signal!(token: QString, result: QString, error: QString),
    request: qt_method!(fn(&mut self, token: QString, method: QString, args: QString)),
    quit_ui: qt_method!(fn(&self)),
    worker: Option<Arc<Worker>>,
    reply_cb: Option<ReplyCb>,
}

struct Worker {
    conn: Mutex<Option<zbus::blocking::Connection>>,
    system: Mutex<Option<zbus::blocking::Connection>>,
    dictation: Mutex<Option<(zbus::blocking::Connection, u64)>>,
    preview: bool,
}

impl Bridge {
    fn request(&mut self, token: QString, method: QString, args: QString) {
        let (Some(w), Some(cb)) = (self.worker.clone(), self.reply_cb.clone()) else { return };
        let token = token.to_string();
        let method = method.to_string();
        let args: Vec<String> = serde_json::from_str::<Vec<Value>>(&args.to_string())
            .unwrap_or_default()
            .into_iter()
            .map(|v| match v {
                Value::String(s) => s,
                other => other.to_string(),
            })
            .collect();
        std::thread::spawn(move || {
            let r = w.run(&method, &args);
            match r {
                Ok(v) => cb((token, v, String::new())),
                Err(e) => cb((token, String::new(), e)),
            }
        });
    }
    fn quit_ui(&self) {
        if let Some(w) = &self.worker {
            let _ = w.run("UiClosed", &[]);
        }
    }
}

fn zerr(e: zbus::Error) -> String {
    let s = e.to_string();
    if s.contains("ServiceUnknown") || s.contains("NameHasNoOwner") {
        "The assistant service is not running".into()
    } else if let Some(i) = s.find(": ") {
        s[i + 2..].to_string()
    } else {
        s
    }
}

fn body_to_string(m: &zbus::Message) -> String {
    let b = m.body();
    if let Ok(s) = b.deserialize::<String>() {
        return s;
    }
    if let Ok(x) = b.deserialize::<bool>() {
        return x.to_string();
    }
    if let Ok(x) = b.deserialize::<u32>() {
        return x.to_string();
    }
    String::new()
}

impl Worker {
    fn conn(&self) -> Result<zbus::blocking::Connection, String> {
        let mut g = self.conn.lock().unwrap();
        if g.is_none() {
            *g = Some(zbus::blocking::connection::Builder::session().and_then(|b| b.method_timeout(Duration::from_secs(120)).build()).map_err(|e| e.to_string())?);
        }
        Ok(g.clone().unwrap())
    }

    fn run(&self, method: &str, args: &[String]) -> Result<String, String> {
        if self.preview {
            return Ok(preview_reply(method, args));
        }
        match method {
            "syscheck" => self.syscheck(args.first().map(String::as_str).unwrap_or("")),
            "dictationStart" => self.dictation_start(),
            "dictationStop" => self.dictation_stop(),
            "dictationCancel" => {
                if let Some((c, id)) = self.dictation.lock().unwrap().take() {
                    let _ = c.call_method(Some("com.fairphone6.Dictation1"), "/com/fairphone6/Dictation1", Some("com.fairphone6.Dictation1"), "Cancel", &(id,));
                }
                Ok(String::new())
            }
            _ => {
                let c = self.conn()?;
                let r = match args.len() {
                    0 => c.call_method(Some(BUS), PATH, Some(BUS), method, &()),
                    1 => c.call_method(Some(BUS), PATH, Some(BUS), method, &(args[0].as_str(),)),
                    2 => c.call_method(Some(BUS), PATH, Some(BUS), method, &(args[0].as_str(), args[1].as_str())),
                    _ => c.call_method(Some(BUS), PATH, Some(BUS), method, &(args[0].as_str(), args[1].as_str(), args[2].as_str())),
                };
                r.map(|m| body_to_string(&m)).map_err(zerr)
            }
        }
    }

    /// The app-permissions broker (system bus). Asked from this process
    /// so it sees the Assistant app (and may show its own prompt).
    fn syscheck(&self, perm: &str) -> Result<String, String> {
        let mut g = self.system.lock().unwrap();
        if g.is_none() {
            *g = zbus::blocking::connection::Builder::system().and_then(|b| b.method_timeout(Duration::from_secs(120)).build()).ok();
        }
        let Some(c) = g.clone() else { return Ok(json!({"ok": true, "result": "absent"}).to_string()) };
        drop(g);
        let r = c.call_method(Some("org.fairphone.Permissions1"), "/org/fairphone/Permissions1", Some("org.fairphone.Permissions1"), "Request", &(perm,));
        let result = match r {
            Ok(m) => m.body().deserialize::<String>().unwrap_or_else(|_| "denied".into()),
            Err(e) => {
                let s = e.to_string();
                if s.contains("ServiceUnknown") || s.contains("NameHasNoOwner") || s.contains("UnknownMethod") {
                    "absent".into()
                } else if s.contains("InvalidArgs") {
                    // the broker does not know this permission class
                    "absent".into()
                } else {
                    "denied".into()
                }
            }
        };
        Ok(json!({"ok": true, "result": result}).to_string())
    }

    fn dictation_start(&self) -> Result<String, String> {
        // One connection per session: only the caller of Start may Stop.
        let c = zbus::blocking::connection::Builder::session().and_then(|b| b.method_timeout(Duration::from_secs(100)).build()).map_err(|e| e.to_string())?;
        let r = c.call_method(Some("com.fairphone6.Dictation1"), "/com/fairphone6/Dictation1", Some("com.fairphone6.Dictation1"), "Start", &()).map_err(zerr)?;
        let id: u64 = r.body().deserialize().map_err(|e| e.to_string())?;
        *self.dictation.lock().unwrap() = Some((c, id));
        Ok(id.to_string())
    }

    fn dictation_stop(&self) -> Result<String, String> {
        let (c, id) = self.dictation.lock().unwrap().take().ok_or("dictation is not running")?;
        let r = c.call_method(Some("com.fairphone6.Dictation1"), "/com/fairphone6/Dictation1", Some("com.fairphone6.Dictation1"), "Stop", &(id,)).map_err(zerr)?;
        r.body().deserialize::<String>().map_err(|e| e.to_string())
    }
}

/// Daemon signals -> QML (own connection, reconnects when the daemon restarts).
fn signal_thread(cb: EventCb) {
    std::thread::spawn(move || loop {
        let Ok(c) = zbus::blocking::Connection::session() else {
            std::thread::sleep(Duration::from_secs(2));
            continue;
        };
        let rule = zbus::MatchRule::builder().msg_type(zbus::message::Type::Signal).sender(BUS).and_then(|b| b.path(PATH)).map(|b| b.build());
        let owner_rule = zbus::MatchRule::builder().msg_type(zbus::message::Type::Signal).interface("org.freedesktop.DBus").and_then(|b| b.member("NameOwnerChanged")).and_then(|b| b.arg(0, BUS)).map(|b| b.build());
        let (Ok(rule), Ok(owner_rule)) = (rule, owner_rule) else { return };
        let Ok(it) = zbus::blocking::MessageIterator::for_match_rule(rule, &c, Some(256)) else { return };
        let Ok(owners) = zbus::blocking::MessageIterator::for_match_rule(owner_rule, &c, Some(16)) else { return };
        let cb2 = cb.clone();
        std::thread::spawn(move || {
            for m in owners.flatten() {
                if let Ok((_, _, new)) = m.body().deserialize::<(String, String, String)>() {
                    cb2(("Daemon".into(), json!({"up": !new.is_empty()}).to_string()));
                }
            }
        });
        for m in it.flatten() {
            let h = m.header();
            let name = h.member().map(|x| x.to_string()).unwrap_or_default();
            let b = m.body();
            let payload = match name.as_str() {
                "Prompt" | "PromptClosed" | "ChatChanged" => b.deserialize::<String>().ok().map(|s| if name == "Prompt" { s } else { json!(s).to_string() }),
                "Delta" => b.deserialize::<(String, String)>().ok().map(|(c, t)| json!({"chat": c, "text": t}).to_string()),
                "Activity" => b.deserialize::<(String, String)>().ok().map(|(c, j)| json!({"chat": c, "entry": serde_json::from_str::<Value>(&j).unwrap_or_default()}).to_string()),
                "TurnState" => b.deserialize::<(String, String)>().ok().map(|(c, s)| json!({"chat": c, "state": s}).to_string()),
                _ => None,
            };
            if let Some(p) = payload {
                cb((name, p));
            }
        }
        std::thread::sleep(Duration::from_secs(1));
    });
}

// ---------- offscreen previews (canned, synthetic data; no daemon) ----------

fn preview_reply(method: &str, args: &[String]) -> String {
    match method {
        "RegisterUi" => json!({"pending": []}).to_string(),
        "ListChats" => json!([
            {"id": "c1", "title": "Running late", "updated": 1793170000, "backend": "local", "model": "qwen3-4b", "remote": false, "provider": "this phone", "preview": "Sent to Test Contact.", "running": false},
            {"id": "c2", "title": "Summarise my unread e-mail", "updated": 1793160000, "backend": "anthropic", "model": "claude-sonnet-5-5", "remote": true, "provider": "Anthropic (api.anthropic.com)", "preview": "You have 3 unread messages from test@example.com…", "running": false},
            {"id": "c3", "title": "Flashlight on", "updated": 1793150000, "backend": "local", "model": "qwen3-4b", "remote": false, "provider": "this phone", "preview": "The flashlight is on.", "running": false}
        ]).to_string(),
        "GetChat" => {
            let remote = args.first().map(|a| a == "c2").unwrap_or(false);
            json!({
                "id": args.first().cloned().unwrap_or_default(), "title": if remote { "Summarise my unread e-mail" } else { "Running late" },
                "backend": if remote { "anthropic" } else { "local" }, "model": if remote { "claude-sonnet-5-5" } else { "qwen3-4b" },
                "remote": remote, "provider": if remote { "Anthropic (api.anthropic.com)" } else { "this phone" }, "running": false, "error": "",
                "items": if remote { json!([
                    {"kind": "user", "text": "Summarise my unread e-mail", "attachments": []},
                    {"kind": "tool", "id": "t1", "name": "email_list"},
                    {"kind": "assistant", "text": "You have **3 unread** e-mails:\n\n1. **Test Contact** – *Lunch on Friday?* asks if 12:30 works.\n2. **test@example.com** – invoice for October.\n3. A newsletter.\n\nShall I reply to Test Contact?"}
                ]) } else { json!([
                    {"kind": "user", "text": "Tell Test I'm running 10 minutes late", "attachments": []},
                    {"kind": "tool", "id": "t1", "name": "contacts_search"},
                    {"kind": "tool", "id": "t2", "name": "messages_send"},
                    {"kind": "assistant", "text": "Sent to **Test Contact** (+999 555 0100):\n\n> Running 10 minutes late"}
                ]) },
                "activity": [
                    {"t": 1793170000, "tool": "contacts_search", "args": {"query": "Test"}, "resources": ["contacts.read"], "decision": "contacts.read: allowed for this chat", "ok": true, "result": "[{\"name\":\"Test Contact\",\"phones\":[\"+999 555 0100\"]}]", "ms": 12},
                    {"t": 1793170004, "tool": "messages_send", "args": {"to": "+999 555 0100", "text": "Running 10 minutes late"}, "resources": ["messages.send"], "decision": "messages.send: allowed once, confirmed", "ok": true, "result": "Message to +999 555 0100 handed to the phone for sending.", "ms": 230},
                    {"t": 1793170010, "tool": "files_list", "args": {"path": "~/.ssh"}, "resources": [], "decision": "invalid arguments", "ok": false, "result": "~/.ssh is not accessible (SSH keys)", "ms": 0}
                ],
                "grants": [{"key": "contacts.read", "title": "Contacts"}]
            })
            .to_string()
        }
        "GetSettings" => json!({
            "settings": {
                "backends": [
                    {"id": "local", "name": "On this phone", "kind": "local", "base_url": "", "model": "", "effort": "", "models": []},
                    {"id": "anthropic", "name": "Claude (Anthropic)", "kind": "anthropic", "base_url": "https://api.anthropic.com", "model": "claude-sonnet-5-5", "effort": "low", "models": ["claude-sonnet-5-5", "claude-haiku-5-5", "claude-opus-5-5"]},
                    {"id": "lan", "name": "Ollama (desktop)", "kind": "ollama", "base_url": "http://192.168.1.20:11434", "model": "qwen3:8b", "effort": "", "models": []}
                ],
                "default_backend": "local", "local_only": false, "tool_policies": {"settings_set": "relaxed", "web_fetch": "disabled"},
                "retention_days": 90, "limits": {"max_steps": 12, "tool_calls_per_turn": 24, "actions_per_10_min": 6, "prompts_per_turn": 6},
                "email_accounts": [{"id": "test", "name": "Test", "address": "test@example.com", "imap_host": "imap.example.com", "imap_port": 993, "smtp_host": "smtp.example.com", "smtp_port": 465, "username": "", "smtp_security": "tls"}],
                "web_search_url": "https://html.duckduckgo.com/html/?q={q}", "custom_instructions": "", "unload_on_close": true, "max_tokens": 4096
            },
            "keys": {"backend:anthropic": true, "backend:local": false, "backend:lan": false, "email:test": true},
            "keyring": "keyring"
        })
        .to_string(),
        "Tools" => json!([
            {"name": "messages_send", "group": "Messages", "title": "Send a text message", "description": "", "side_effect": true, "untrusted": false, "policy": "always"},
            {"name": "messages_read", "group": "Messages", "title": "Read messages", "description": "", "side_effect": false, "untrusted": true, "policy": "default"},
            {"name": "call_place", "group": "Calls", "title": "Make a phone call", "description": "", "side_effect": true, "untrusted": false, "policy": "always"},
            {"name": "email_send", "group": "E-mail", "title": "Send an e-mail", "description": "", "side_effect": true, "untrusted": false, "policy": "always"},
            {"name": "settings_set", "group": "Settings", "title": "Change a phone setting", "description": "", "side_effect": true, "untrusted": false, "policy": "relaxed"},
            {"name": "web_fetch", "group": "Internet", "title": "Fetch a web page", "description": "", "side_effect": false, "untrusted": true, "policy": "disabled"}
        ]).to_string(),
        "Grants" => json!({"always": [
            {"key": "files.read:/home/user/Documents", "title": "Files: read ~/Documents", "since": 1793100000},
            {"key": "contacts.read", "title": "Contacts", "since": 1793000000}
        ], "resources": []}).to_string(),
        "Suggest" => {
            let t = args.first().cloned().unwrap_or_default();
            json!([
                {"kind": "ask", "title": "Ask the assistant", "subtitle": t},
                {"kind": "call", "title": "Call Test Contact", "subtitle": "+999 555 0100", "url": "tel:///+9995550100"},
                {"kind": "message", "title": "Message Test Contact", "subtitle": "+999 555 0100", "url": "message:///+9995550100"},
                {"kind": "search", "title": format!("Search the web for \u{201c}{t}\u{201d}"), "url": "https://duckduckgo.com/?q=test"}
            ]).to_string()
        }
        "LocalStatus" => json!({"state": "idle", "model": "qwen3-4b-instruct-q4_k_m", "n_ctx": 8192}).to_string(),
        "syscheck" => json!({"ok": true, "result": "absent"}).to_string(),
        _ => String::new(),
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let flag = |f: &str| args.iter().position(|a| a == f).and_then(|i| args.get(i + 1)).cloned();
    let preview = flag("--preview");
    let screenshot = flag("--screenshot").unwrap_or_default();
    let initial = args.iter().skip(1).find(|a| a.starts_with("assistant:")).cloned().unwrap_or_default();
    let worker = Arc::new(Worker { conn: Mutex::new(None), system: Mutex::new(None), dictation: Mutex::new(None), preview: preview.is_some() });
    let bridge = QObjectBox::new(Bridge {
        initialUrl: initial.into(),
        previewScene: preview.clone().unwrap_or_default().into(),
        screenshotPath: screenshot.into(),
        worker: Some(worker),
        ..Default::default()
    });
    let pinned = bridge.pinned();
    // The C++ object must exist before a QPointer to it can be taken.
    pinned.get_or_create_cpp_object();
    let ptr = QPointer::from(&*pinned.borrow());
    let p2 = ptr.clone();
    let ev = queued_callback(move |(kind, json): (String, String)| {
        if let Some(b) = p2.as_pinned() {
            b.borrow().event(kind.into(), json.into());
        }
    });
    let rp = queued_callback(move |(token, result, error): (String, String, String)| {
        if let Some(b) = ptr.as_pinned() {
            b.borrow().reply(token.into(), result.into(), error.into());
        }
    });
    let ev: EventCb = Arc::new(ev);
    pinned.borrow_mut().reply_cb = Some(Arc::new(rp));
    if preview.is_none() {
        signal_thread(ev);
    }
    let mut engine = QmlEngine::new();
    engine.set_object_property("assistant".into(), pinned);
    let dir = std::env::var("FP6_ASSISTANT_QML_DIR").unwrap_or_else(|_| format!("{}/../qml", env!("CARGO_MANIFEST_DIR")));
    engine.load_file(format!("{dir}/Launcher.qml").into());
    engine.exec();
}
