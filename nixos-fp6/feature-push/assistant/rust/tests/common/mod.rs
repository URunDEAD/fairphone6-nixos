//! Test fixtures: a private D-Bus daemon, fake phone services (history,
//! telephony, indicators, on-device LLM runtime), a fake model server
//! speaking OpenAI-compatible and Anthropic SSE, and a scripted "UI" that
//! answers the assistant's dialogs. Synthetic data only (+999 numbers,
//! Test Contact, test@example.com).
#![allow(dead_code)]

use fp6_assistant::agent::Agent;
use fp6_assistant::keyring::Secrets;
use fp6_assistant::prompts::{HubEvent, PromptHub};
use fp6_assistant::settings::Paths;
use fp6_assistant::tools::Registry;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::BufRead;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use zbus::zvariant::{OwnedValue, Value as ZV};

pub struct Bus(Child, pub String);
impl Bus {
    pub fn new() -> Option<Self> {
        let mut child = Command::new("dbus-daemon")
            .args(["--config-file", concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/session.conf"), "--nofork", "--print-address=1", "--nopidfile"])
            .stdout(Stdio::piped())
            .spawn()
            .ok()?;
        let mut address = String::new();
        std::io::BufReader::new(child.stdout.take().unwrap()).read_line(&mut address).ok()?;
        Some(Bus(child, address.trim().into()))
    }
    pub async fn connection(&self) -> zbus::Connection {
        zbus::connection::Builder::address(self.1.as_str()).unwrap().build().await.unwrap()
    }
}
impl Drop for Bus {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

// ---------- fake phone services ----------

#[derive(Default)]
pub struct Phone {
    pub sent: Mutex<Vec<(String, String, Vec<String>)>>,
    pub calls: Mutex<Vec<String>>,
    pub flashlight: Mutex<bool>,
    /// Text events (threadId, senderId, message, timestamp).
    pub events: Mutex<Vec<(String, String, String, String)>>,
}

pub struct Telephony(pub Arc<Phone>);
#[zbus::interface(name = "com.lomiri.TelephonyServiceHandler")]
impl Telephony {
    fn account_ids(&self) -> Vec<String> {
        vec!["ofono/ofono/account0".into()]
    }
    fn send_message(&self, account: String, message: String, _att: Vec<(String, String, String)>, props: HashMap<String, OwnedValue>) -> String {
        let to: Vec<String> = props.get("participantIds").and_then(|v| Vec::<String>::try_from(v.try_clone().unwrap()).ok()).unwrap_or_default();
        self.0.sent.lock().unwrap().push((account, message, to));
        "/job/1".into()
    }
    fn start_call(&self, number: String, _account: String) {
        self.0.calls.lock().unwrap().push(number);
    }
}

pub struct History(pub Arc<Phone>);
#[zbus::interface(name = "com.lomiri.HistoryService")]
impl History {
    async fn query_events(&self, _kind: i32, _sort: HashMap<String, OwnedValue>, filter: HashMap<String, OwnedValue>, #[zbus(object_server)] server: &zbus::ObjectServer) -> String {
        let thread = filter.get("filterValue").and_then(|v| String::try_from(v.try_clone().unwrap()).ok());
        let items: Vec<HashMap<String, OwnedValue>> = self
            .0
            .events
            .lock()
            .unwrap()
            .iter()
            .filter(|e| thread.as_deref().map(|t| t == e.0).unwrap_or(true))
            .map(|(t, s, m, ts)| {
                HashMap::from([
                    ("threadId".to_string(), OwnedValue::try_from(ZV::from(t.as_str())).unwrap()),
                    ("senderId".to_string(), OwnedValue::try_from(ZV::from(s.as_str())).unwrap()),
                    ("message".to_string(), OwnedValue::try_from(ZV::from(m.as_str())).unwrap()),
                    ("timestamp".to_string(), OwnedValue::try_from(ZV::from(ts.as_str())).unwrap()),
                ])
            })
            .collect();
        let path = "/com/lomiri/HistoryService/eventview1";
        let _ = server.remove::<View, _>(path).await;
        server.at(path, View(Mutex::new(Some(items)))).await.unwrap();
        path.into()
    }
}

pub struct View(Mutex<Option<Vec<HashMap<String, OwnedValue>>>>);
#[zbus::interface(name = "com.lomiri.HistoryService.EventView")]
impl View {
    fn next_page(&self) -> Vec<HashMap<String, OwnedValue>> {
        self.0.lock().unwrap().take().unwrap_or_default()
    }
    fn destroy(&self) {}
}

pub struct Power(pub Arc<Phone>);
#[zbus::interface(name = "org.gtk.Actions")]
impl Power {
    fn describe(&self, action: String) -> zbus::fdo::Result<(bool, zbus::zvariant::Signature, Vec<OwnedValue>)> {
        if action != "flashlight" {
            return Err(zbus::fdo::Error::InvalidArgs("no such action".into()));
        }
        Ok((true, zbus::zvariant::Signature::try_from("b").unwrap(), vec![OwnedValue::from(*self.0.flashlight.lock().unwrap())]))
    }
    fn activate(&self, action: String, params: Vec<OwnedValue>, _platform: HashMap<String, OwnedValue>) {
        if action == "flashlight" {
            if let Some(b) = params.first().and_then(|v| bool::try_from(v).ok()) {
                *self.0.flashlight.lock().unwrap() = b;
            }
        }
    }
}

pub struct LocalLlm(pub String);
#[zbus::interface(name = "com.fairphone6.LocalLlm1")]
impl LocalLlm {
    fn endpoint(&self) -> (String, String) {
        (self.0.clone(), "qwen3-4b-test".into())
    }
    fn status(&self) -> String {
        json!({"state": "running", "model": "qwen3-4b-test"}).to_string()
    }
    fn unload(&self) {}
}

pub async fn serve_phone(bus: &Bus, phone: Arc<Phone>, llm_url: Option<String>) -> Vec<zbus::Connection> {
    let mut conns = Vec::new();
    let c = bus.connection().await;
    c.object_server().at("/com/lomiri/TelephonyServiceHandler", Telephony(phone.clone())).await.unwrap();
    c.request_name("com.lomiri.TelephonyServiceHandler").await.unwrap();
    conns.push(c);
    let c = bus.connection().await;
    c.object_server().at("/com/lomiri/HistoryService", History(phone.clone())).await.unwrap();
    c.request_name("com.lomiri.HistoryService").await.unwrap();
    conns.push(c);
    let c = bus.connection().await;
    c.object_server().at("/org/ayatana/indicator/power", Power(phone.clone())).await.unwrap();
    c.request_name("org.ayatana.indicator.power").await.unwrap();
    conns.push(c);
    if let Some(u) = llm_url {
        let c = bus.connection().await;
        c.object_server().at("/com/fairphone6/LocalLlm1", LocalLlm(u)).await.unwrap();
        c.request_name("com.fairphone6.LocalLlm1").await.unwrap();
        conns.push(c);
    }
    conns
}

// ---------- fake model server ----------

#[derive(Clone, Debug)]
pub struct Req {
    pub path: String,
    pub headers: HashMap<String, String>,
    pub body: Value,
}

pub type Script = Arc<dyn Fn(usize, &Req) -> String + Send + Sync>;

/// HTTP/1.1 on 127.0.0.1; each request answered by `script(n, req)` with
/// a complete response (status line, headers, body), then closed.
pub async fn model_server(script: Script) -> (String, Arc<Mutex<Vec<Req>>>) {
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", l.local_addr().unwrap());
    let log: Arc<Mutex<Vec<Req>>> = Default::default();
    let log2 = log.clone();
    tokio::spawn(async move {
        let mut n = 0usize;
        loop {
            let Ok((mut s, _)) = l.accept().await else { return };
            let mut buf = Vec::new();
            let mut tmp = [0u8; 8192];
            let (head, body) = loop {
                let k = s.read(&mut tmp).await.unwrap_or(0);
                if k == 0 {
                    break (String::new(), Vec::new());
                }
                buf.extend_from_slice(&tmp[..k]);
                if let Some(p) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                    let head = String::from_utf8_lossy(&buf[..p]).to_string();
                    let len = head.lines().find_map(|l| l.to_lowercase().strip_prefix("content-length:").map(|v| v.trim().parse::<usize>().unwrap_or(0))).unwrap_or(0);
                    let mut body = buf[p + 4..].to_vec();
                    while body.len() < len {
                        let k = s.read(&mut tmp).await.unwrap_or(0);
                        if k == 0 {
                            break;
                        }
                        body.extend_from_slice(&tmp[..k]);
                    }
                    break (head, body);
                }
            };
            let mut lines = head.lines();
            let path = lines.next().unwrap_or("").split_whitespace().nth(1).unwrap_or("").to_string();
            let headers = lines.filter_map(|l| l.split_once(':')).map(|(k, v)| (k.trim().to_lowercase(), v.trim().to_string())).collect();
            let req = Req { path, headers, body: serde_json::from_slice(&body).unwrap_or(Value::Null) };
            log2.lock().unwrap().push(req.clone());
            let resp = script(n, &req);
            n += 1;
            let _ = s.write_all(resp.as_bytes()).await;
            let _ = s.shutdown().await;
        }
    });
    (url, log)
}

pub fn sse(events: &[String]) -> String {
    let mut out = String::from("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n");
    for e in events {
        out.push_str(e);
        out.push_str("\n\n");
    }
    out
}

/// OpenAI-compatible streamed reply: optional text, optional tool call.
pub fn openai_reply(text: &str, tool: Option<(&str, Value)>) -> String {
    let mut ev = Vec::new();
    if !text.is_empty() {
        let (a, b) = text.split_at(text.len() / 2);
        for part in [a, b] {
            ev.push(format!("data: {}", json!({"choices": [{"index": 0, "delta": {"content": part}}]})));
        }
    }
    if let Some((name, args)) = tool {
        let s = args.to_string();
        let (a, b) = s.split_at(s.len() / 2);
        ev.push(format!("data: {}", json!({"choices": [{"index": 0, "delta": {"tool_calls": [{"index": 0, "id": format!("call_{name}"), "type": "function", "function": {"name": name, "arguments": a}}]}}]})));
        ev.push(format!("data: {}", json!({"choices": [{"index": 0, "delta": {"tool_calls": [{"index": 0, "function": {"arguments": b}}]}}]})));
        ev.push(format!("data: {}", json!({"choices": [{"index": 0, "delta": {}, "finish_reason": "tool_calls"}]})));
    } else {
        ev.push(format!("data: {}", json!({"choices": [{"index": 0, "delta": {}, "finish_reason": "stop"}]})));
    }
    ev.push("data: [DONE]".into());
    sse(&ev)
}

// ---------- the agent + a scripted UI ----------

pub struct Env {
    pub dir: tempfile::TempDir,
    pub agent: Arc<Agent>,
    pub prompts: Arc<Mutex<Vec<Value>>>,
}

/// `answer(prompt) -> answer` plays the user in the Assistant UI.
pub fn env(session: Option<zbus::Connection>, answer: Arc<dyn Fn(&Value) -> Value + Send + Sync>) -> Env {
    let dir = tempfile::tempdir().unwrap();
    let paths = Paths { config: dir.path().join("config"), data: dir.path().join("data") };
    let hub = PromptHub::new();
    let secrets = Arc::new(Secrets::file_only(paths.credentials_file()));
    let agent = Agent::new(paths, Registry::standard(&[]), hub.clone(), session, None, secrets);
    let prompts: Arc<Mutex<Vec<Value>>> = Default::default();
    let p2 = prompts.clone();
    let mut rx = hub.subscribe();
    let h2 = hub.clone();
    tokio::spawn(async move {
        while let Ok(ev) = rx.recv().await {
            if let HubEvent::Prompt(p) = ev {
                p2.lock().unwrap().push(p.clone());
                let a = answer(&p);
                let id = p["id"].as_str().unwrap_or("").to_string();
                h2.respond(None, &id, a);
            }
        }
    });
    Env { dir, agent, prompts }
}

/// Run one user message to completion; returns the final TurnState.
pub async fn turn(agent: &Arc<Agent>, chat: &str, text: &str) -> String {
    let mut rx = agent.hub.subscribe();
    agent.start_turn(chat, text.into(), vec![]).unwrap();
    loop {
        match tokio::time::timeout(std::time::Duration::from_secs(30), rx.recv()).await {
            Ok(Ok(HubEvent::TurnState { chat: c, state })) if c == chat && (state == "idle" || state.starts_with("error:")) => {
                // let the task deregister
                for _ in 0..50 {
                    if !agent.is_running(chat) {
                        break;
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                }
                return state;
            }
            Ok(Ok(_)) => {}
            Ok(Err(_)) => {}
            Err(_) => panic!("turn timed out"),
        }
    }
}

/// The scripted user: grants for this chat, confirms, no broker installed.
pub fn yes_user() -> Arc<dyn Fn(&Value) -> Value + Send + Sync> {
    Arc::new(|p: &Value| match p["kind"].as_str() {
        Some("permission") => json!("chat"),
        Some("confirm") => json!("confirm"),
        Some("ui") if p["op"] == "syscheck" => json!({"ok": true, "result": "absent"}),
        _ => json!({"ok": false, "error": "not in test"}),
    })
}
