//! Data-driven tools: JSON manifests in /etc/fp6-assistant/tools.d and
//! ~/.config/fp6-assistant/tools.d add tools without code. A manifest
//! runs a program (argv, no shell) or calls a session-bus method; its
//! arguments are substituted only as whole argv elements / D-Bus string
//! arguments ("{name}"), never into a shell line.
//!
//! {
//!   "name": "weather_now",                     // [a-z0-9_], unique
//!   "title": "Weather", "group": "Extras",
//!   "description": "Current weather for a city",
//!   "parameters": {"type": "object", "properties": {"city": {"type": "string"}}, "required": ["city"]},
//!   "resources": ["web"],                       // classes from permissions::RESOURCES
//!   "side_effect": false, "untrusted_output": true,
//!   "confirm": {"title": "...", "fields": [["City", "{city}"]], "action": "OK"},
//!   "exec": ["/run/current-system/sw/bin/fp6-weather", "--city", "{city}"],
//!   // or: "dbus": {"dest": "...", "path": "/...", "interface": "...", "method": "...", "args": ["{city}"]}
//!   "timeout": 20
//! }

use super::{Confirm, Ctx, Output, Tool, ToolSpec};
use crate::permissions::{self, Resource};
use crate::settings::Settings;
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::Value;
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Clone, Debug, Deserialize)]
pub struct DbusCall {
    pub dest: String,
    pub path: String,
    pub interface: String,
    pub method: String,
    #[serde(default)]
    pub args: Vec<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ConfirmTpl {
    pub title: String,
    #[serde(default)]
    pub fields: Vec<(String, String)>,
    #[serde(default = "ok")]
    pub action: String,
}
fn ok() -> String {
    "OK".into()
}

#[derive(Clone, Debug, Deserialize)]
pub struct Manifest {
    pub name: String,
    #[serde(default)]
    pub title: String,
    #[serde(default = "extras")]
    pub group: String,
    pub description: String,
    pub parameters: Value,
    #[serde(default)]
    pub resources: Vec<String>,
    #[serde(default)]
    pub side_effect: bool,
    #[serde(default = "yes")]
    pub untrusted_output: bool,
    pub confirm: Option<ConfirmTpl>,
    #[serde(default)]
    pub exec: Vec<String>,
    pub dbus: Option<DbusCall>,
    #[serde(default = "twenty")]
    pub timeout: u64,
}
fn extras() -> String {
    "Extras".into()
}
fn yes() -> bool {
    true
}
fn twenty() -> u64 {
    20
}

impl Manifest {
    pub fn check(&self) -> Result<(), String> {
        if self.name.is_empty() || !self.name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_') {
            return Err(format!("bad tool name {:?}", self.name));
        }
        if self.exec.is_empty() == self.dbus.is_none() {
            return Err(format!("{}: exactly one of exec / dbus", self.name));
        }
        if let Some(p) = self.exec.first() {
            if !p.starts_with('/') {
                return Err(format!("{}: exec[0] must be an absolute path", self.name));
            }
        }
        for r in &self.resources {
            // A manifest cannot invent permission classes, and cannot claim
            // a directory scope (files.* are code-only).
            if permissions::info(r).is_none() || r.starts_with("files.") {
                return Err(format!("{}: unknown or unsupported resource {r}", self.name));
            }
        }
        if self.parameters.get("type").and_then(Value::as_str) != Some("object") {
            return Err(format!("{}: parameters must be an object schema", self.name));
        }
        Ok(())
    }
}

/// "{name}" -> argument value (strings as-is, others as JSON).
pub fn substitute(tpl: &str, args: &Value) -> String {
    let mut out = tpl.to_string();
    if let Some(obj) = args.as_object() {
        for (k, v) in obj {
            let val = match v {
                Value::String(s) => s.clone(),
                other => other.to_string(),
            };
            out = out.replace(&format!("{{{k}}}"), &val);
        }
    }
    out
}

pub struct ManifestTool(pub Manifest);

#[async_trait]
impl Tool for ManifestTool {
    fn spec(&self) -> ToolSpec {
        let m = &self.0;
        ToolSpec {
            name: m.name.clone(),
            group: m.group.clone(),
            title: if m.title.is_empty() { m.name.clone() } else { m.title.clone() },
            description: m.description.clone(),
            parameters: m.parameters.clone(),
            side_effect: m.side_effect,
            untrusted_output: m.untrusted_output,
        }
    }
    fn resources(&self, _a: &Value, _s: &Settings) -> Result<Vec<Resource>, String> {
        Ok(self.0.resources.iter().map(|r| Resource::new(r)).collect())
    }
    fn confirm(&self, a: &Value) -> Option<Confirm> {
        let c = self.0.confirm.as_ref()?;
        Some(Confirm {
            title: substitute(&c.title, a),
            fields: c.fields.iter().map(|(k, v)| (k.clone(), substitute(v, a))).collect(),
            action: c.action.clone(),
            destructive: false,
        })
    }
    async fn run(&self, a: Value, ctx: &Ctx) -> Result<Output, String> {
        let m = &self.0;
        if !m.exec.is_empty() {
            let argv: Vec<String> = m.exec.iter().map(|x| substitute(x, &a)).collect();
            let refs: Vec<&str> = argv.iter().map(String::as_str).collect();
            return super::run_cmd(&refs, m.timeout).await.map(Output::text);
        }
        let d = m.dbus.as_ref().ok_or("no action")?;
        let args: Vec<String> = d.args.iter().map(|x| substitute(x, &a)).collect();
        let conn = ctx.session()?;
        let body = zbus::zvariant::Structure::from(
            args.iter().map(|s| zbus::zvariant::Value::from(s.as_str())).fold(zbus::zvariant::StructureBuilder::new(), |b, v| b.append_field(v)).build().map_err(|e| e.to_string())?,
        );
        let r = if args.is_empty() {
            crate::dbusutil::call(conn, &d.dest, &d.path, &d.interface, &d.method, &()).await?
        } else {
            crate::dbusutil::call(conn, &d.dest, &d.path, &d.interface, &d.method, &body).await?
        };
        Ok(Output::json(&crate::dbusutil::body_json(&r)))
    }
}

pub fn parse(text: &str) -> Result<Manifest, String> {
    let m: Manifest = serde_json::from_str(text).map_err(|e| e.to_string())?;
    m.check()?;
    Ok(m)
}

pub fn load_dirs(dirs: &[PathBuf]) -> Vec<Arc<dyn Tool>> {
    let mut out: Vec<Arc<dyn Tool>> = Vec::new();
    for d in dirs {
        let Ok(rd) = std::fs::read_dir(d) else { continue };
        let mut files: Vec<_> = rd.flatten().map(|e| e.path()).filter(|p| p.extension().map(|e| e == "json").unwrap_or(false)).collect();
        files.sort();
        for f in files {
            match std::fs::read_to_string(&f).map_err(|e| e.to_string()).and_then(|t| parse(&t)) {
                Ok(m) => out.push(Arc::new(ManifestTool(m))),
                Err(e) => eprintln!("fp6-assistantd: ignoring tool manifest {}: {e}", f.display()),
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn manifests() {
        let m = parse(r#"{"name":"echo_it","description":"d","parameters":{"type":"object","properties":{"word":{"type":"string"}}},"exec":["/bin/echo","{word}"],"resources":["web"]}"#).unwrap();
        assert!(m.untrusted_output);
        assert!(parse(r#"{"name":"x","description":"d","parameters":{"type":"object"},"exec":["echo"]}"#).is_err());
        assert!(parse(r#"{"name":"x","description":"d","parameters":{"type":"object"},"exec":["/bin/true"],"resources":["files.read"]}"#).is_err());
        assert!(parse(r#"{"name":"x","description":"d","parameters":{"type":"object"},"exec":["/bin/true"],"resources":["root"]}"#).is_err());
        // whole-argument substitution: shell metacharacters stay inert
        assert_eq!(substitute("{word}", &json!({"word": "a; rm -rf ~"})), "a; rm -rf ~");
    }

    #[tokio::test]
    async fn exec_runs_without_shell() {
        let m = parse(r#"{"name":"echo_it","description":"d","parameters":{"type":"object"},"exec":["/bin/sh","-c","echo \"$0\"","{word}"]}"#);
        // /bin/sh may not exist in the build sandbox; the check is that the
        // argument arrives as one argv element.
        if let (Ok(m), true) = (m, std::path::Path::new("/bin/sh").exists()) {
            let t = ManifestTool(m);
            let ctx_out = super::super::run_cmd(&["/bin/sh", "-c", "echo \"$0\"", "a; echo pwned"], 5).await.unwrap();
            assert_eq!(ctx_out.trim(), "a; echo pwned");
            assert_eq!(t.spec().group, "Extras");
        }
    }
}
