//! The tool layer. Each tool declares a JSON-schema, whether it has side
//! effects, the resources a call needs (computed from its arguments) and
//! the text of its confirmation dialog. Tools are registered in a
//! `Registry`; extra tools can be added without code through JSON
//! manifests (`manifest.rs`, /etc/fp6-assistant/tools.d and
//! ~/.config/fp6-assistant/tools.d).
//!
//! A tool never asks for permissions itself and never sees the answer to
//! a dialog: the agent (agent.rs) checks resources and confirmations first
//! and only then calls `run`.

pub mod apps;
pub mod calendar;
pub mod device;
pub mod email;
pub mod files;
pub mod manifest;
pub mod media;
pub mod messages;
pub mod notifications;
pub mod web;

use crate::permissions::Resource;
use crate::settings::Settings;
use crate::prompts::UiLink;
use async_trait::async_trait;
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::Arc;

#[derive(Clone, Debug, Serialize)]
pub struct ToolSpec {
    pub name: String,
    /// Settings group ("Messages", "Files", ...).
    pub group: String,
    pub title: String,
    pub description: String,
    pub parameters: Value,
    /// Sends, deletes, writes, calls, posts, toggles: confirmed per action.
    pub side_effect: bool,
    /// Its output comes from outside (messages, e-mail, web, files...):
    /// marks the turn as tainted.
    pub untrusted_output: bool,
}

/// What the confirmation dialog shows: exact recipient and content.
#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct Confirm {
    pub title: String,
    /// Ordered label/value rows ("To", "+999 555 0100"; "Message", "...").
    pub fields: Vec<(String, String)>,
    /// Button text ("Send", "Delete", "Call").
    pub action: String,
    /// Destructive styling (delete, call).
    pub destructive: bool,
}

#[derive(Clone, Debug, Default)]
pub struct Output {
    pub text: String,
    /// An image for vision-capable models (mime, base64).
    pub image: Option<(String, String)>,
    /// Local files produced (shown as attachments in the chat).
    pub files: Vec<String>,
}

impl Output {
    pub fn text(t: impl Into<String>) -> Self {
        Output { text: t.into(), ..Default::default() }
    }
    pub fn json(v: &Value) -> Self {
        Output::text(serde_json::to_string_pretty(v).unwrap_or_default())
    }
}

/// Everything a tool may use at run time.
pub struct Ctx {
    pub session: Option<zbus::Connection>,
    pub system: Option<zbus::Connection>,
    pub http: reqwest::Client,
    pub settings: Settings,
    pub secrets: Arc<crate::keyring::Secrets>,
    pub ui: UiLink,
    pub chat_id: String,
}

impl Ctx {
    pub fn session(&self) -> Result<&zbus::Connection, String> {
        self.session.as_ref().ok_or_else(|| "the session bus is not available".to_string())
    }
    pub fn system(&self) -> Result<&zbus::Connection, String> {
        self.system.as_ref().ok_or_else(|| "the system bus is not available".to_string())
    }

    /// Run an operation hosted by the Assistant UI (camera via Content Hub,
    /// calendar and alarms via QtOrganizer/UITK). The UI answers
    /// {"ok": true, ...} or {"ok": false, "error": "..."}.
    pub async fn ui_op(&self, op: &str, args: Value, timeout_s: u64) -> Result<Value, String> {
        let answer = self
            .ui
            .ask(serde_json::json!({"kind": "ui", "op": op, "args": args, "chat": self.chat_id}), std::time::Duration::from_secs(timeout_s))
            .await;
        if answer.is_null() {
            return Err("the Assistant app did not answer (it must be open for this)".into());
        }
        if answer.get("ok").and_then(Value::as_bool) == Some(true) {
            Ok(answer)
        } else {
            Err(answer.get("error").and_then(Value::as_str).unwrap_or("cancelled").to_string())
        }
    }
}

#[async_trait]
pub trait Tool: Send + Sync {
    fn spec(&self) -> ToolSpec;
    /// Resources this call needs. Errors are argument problems, reported
    /// back to the model.
    fn resources(&self, args: &Value, settings: &Settings) -> Result<Vec<Resource>, String>;
    /// The confirmation dialog (only consulted for side-effecting tools).
    fn confirm(&self, _args: &Value) -> Option<Confirm> {
        None
    }
    async fn run(&self, args: Value, ctx: &Ctx) -> Result<Output, String>;
}

pub struct Registry {
    tools: BTreeMap<String, Arc<dyn Tool>>,
}

impl Registry {
    pub fn empty() -> Self {
        Registry { tools: BTreeMap::new() }
    }

    /// All built-in tools plus manifest tools from the given directories.
    pub fn standard(manifest_dirs: &[std::path::PathBuf]) -> Self {
        let mut r = Registry::empty();
        for t in messages::tools() {
            r.add(t);
        }
        for t in email::tools() {
            r.add(t);
        }
        for t in files::tools() {
            r.add(t);
        }
        for t in media::tools() {
            r.add(t);
        }
        for t in apps::tools() {
            r.add(t);
        }
        for t in calendar::tools() {
            r.add(t);
        }
        for t in device::tools() {
            r.add(t);
        }
        for t in notifications::tools() {
            r.add(t);
        }
        for t in web::tools() {
            r.add(t);
        }
        for t in manifest::load_dirs(manifest_dirs) {
            // Built-ins win on a name clash.
            if !r.tools.contains_key(&t.spec().name) {
                r.add(t);
            }
        }
        r
    }

    pub fn add(&mut self, t: Arc<dyn Tool>) {
        self.tools.insert(t.spec().name.clone(), t);
    }

    pub fn get(&self, name: &str) -> Option<Arc<dyn Tool>> {
        self.tools.get(name).cloned()
    }

    pub fn specs(&self) -> Vec<ToolSpec> {
        self.tools.values().map(|t| t.spec()).collect()
    }
}

// ---- argument helpers shared by the tools ----

pub fn arg_str<'a>(args: &'a Value, key: &str) -> Result<&'a str, String> {
    args.get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| format!("missing required argument \"{key}\""))
}

pub fn opt_str<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key).and_then(Value::as_str).filter(|s| !s.trim().is_empty())
}

pub fn opt_u64(args: &Value, key: &str, default: u64, max: u64) -> u64 {
    args.get(key)
        .and_then(|v| v.as_u64().or_else(|| v.as_str().and_then(|s| s.parse().ok())))
        .unwrap_or(default)
        .min(max)
}

/// Validate arguments against the tool's schema subset (object with
/// required keys and primitive types). The models sometimes send strings
/// for numbers; those are accepted when they parse.
pub fn validate(schema: &Value, args: &Value) -> Result<(), String> {
    let Some(obj) = args.as_object() else {
        return Err("arguments must be a JSON object".into());
    };
    if let Some(req) = schema.get("required").and_then(Value::as_array) {
        for k in req.iter().filter_map(Value::as_str) {
            match obj.get(k) {
                None | Some(Value::Null) => return Err(format!("missing required argument \"{k}\"")),
                _ => {}
            }
        }
    }
    if let Some(props) = schema.get("properties").and_then(Value::as_object) {
        for (k, v) in obj {
            let Some(p) = props.get(k) else { continue };
            let ok = match p.get("type").and_then(Value::as_str) {
                Some("string") => v.is_string(),
                Some("integer") => v.is_i64() || v.is_u64() || v.as_str().map(|s| s.parse::<i64>().is_ok()).unwrap_or(false),
                Some("number") => v.is_number() || v.as_str().map(|s| s.parse::<f64>().is_ok()).unwrap_or(false),
                Some("boolean") => v.is_boolean(),
                Some("array") => v.is_array(),
                Some("object") => v.is_object(),
                _ => true,
            };
            if !ok {
                return Err(format!("argument \"{k}\" has the wrong type"));
            }
            if let (Some(allowed), Some(s)) = (p.get("enum").and_then(Value::as_array), v.as_str()) {
                if !allowed.iter().any(|a| a.as_str() == Some(s)) {
                    return Err(format!("argument \"{k}\" must be one of {}", Value::Array(allowed.clone())));
                }
            }
        }
    }
    Ok(())
}

/// Build a schema quickly: props = [(name, type, description, required)].
pub fn schema(props: &[(&str, &str, &str, bool)]) -> Value {
    let mut p = serde_json::Map::new();
    let mut req = Vec::new();
    for (name, ty, desc, required) in props {
        p.insert(name.to_string(), serde_json::json!({"type": ty, "description": desc}));
        if *required {
            req.push(Value::String(name.to_string()));
        }
    }
    serde_json::json!({"type": "object", "properties": p, "required": req, "additionalProperties": false})
}

/// Add an enum to a property of a schema built with `schema`.
pub fn with_enum(mut s: Value, prop: &str, values: &[&str]) -> Value {
    s["properties"][prop]["enum"] = Value::Array(values.iter().map(|v| Value::String(v.to_string())).collect());
    s
}

pub fn spec(name: &str, group: &str, title: &str, description: &str, parameters: Value, side_effect: bool, untrusted: bool) -> ToolSpec {
    ToolSpec {
        name: name.into(),
        group: group.into(),
        title: title.into(),
        description: description.into(),
        parameters,
        side_effect,
        untrusted_output: untrusted,
    }
}

/// Run a program (no shell) with a timeout; stdout on success.
pub async fn run_cmd(argv: &[&str], timeout_s: u64) -> Result<String, String> {
    let (prog, rest) = argv.split_first().ok_or("empty command")?;
    let fut = tokio::process::Command::new(prog).args(rest).stdin(std::process::Stdio::null()).kill_on_drop(true).output();
    let out = tokio::time::timeout(std::time::Duration::from_secs(timeout_s), fut)
        .await
        .map_err(|_| format!("{prog} timed out"))?
        .map_err(|e| format!("{prog}: {e}"))?;
    if !out.status.success() {
        return Err(format!("{prog} failed: {}", String::from_utf8_lossy(&out.stderr).trim()));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn validates_required_and_types() {
        let s = with_enum(schema(&[("to", "string", "", true), ("n", "integer", "", false), ("kind", "string", "", false)]), "kind", &["a", "b"]);
        assert!(validate(&s, &json!({"to": "+999"})).is_ok());
        assert!(validate(&s, &json!({"to": "+999", "n": "3"})).is_ok());
        assert!(validate(&s, &json!({})).unwrap_err().contains("to"));
        assert!(validate(&s, &json!({"to": 5})).is_err());
        assert!(validate(&s, &json!({"to": "x", "kind": "c"})).is_err());
        assert!(validate(&s, &json!("str")).is_err());
    }
}
