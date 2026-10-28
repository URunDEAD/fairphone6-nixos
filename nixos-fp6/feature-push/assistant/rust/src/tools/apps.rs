//! Apps and links: installed apps as lomiri-app-launch sees them (the app
//! id is the .desktop basename), started through lomiri-url-dispatcher
//! (application:///<id>.desktop); links opened the same way.

use super::{arg_str, schema, spec, Ctx, Output, Tool, ToolSpec};
use crate::dbusutil::call;
use crate::permissions::Resource;
use crate::settings::Settings;
use async_trait::async_trait;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

pub fn tools() -> Vec<Arc<dyn Tool>> {
    vec![Arc::new(ListApps), Arc::new(OpenApp), Arc::new(OpenUrl)]
}

#[derive(Clone, Debug, PartialEq)]
pub struct App {
    pub id: String,
    pub name: String,
    pub icon: String,
    pub keywords: Vec<String>,
}

pub fn data_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    match std::env::var_os("XDG_DATA_HOME") {
        Some(h) if !h.is_empty() => dirs.push(PathBuf::from(h)),
        _ => {
            if let Some(h) = std::env::var_os("HOME") {
                dirs.push(PathBuf::from(h).join(".local/share"));
            }
        }
    }
    if let Some(list) = std::env::var_os("XDG_DATA_DIRS") {
        dirs.extend(std::env::split_paths(&list));
    }
    dirs.push(PathBuf::from("/run/current-system/sw/share"));
    let mut seen = Vec::new();
    dirs.retain(|d| if seen.contains(d) { false } else { seen.push(d.clone()); true });
    dirs
}

pub fn parse_desktop(id: &str, text: &str) -> Option<App> {
    let mut in_entry = false;
    let mut keys: BTreeMap<String, String> = BTreeMap::new();
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_entry = line == "[Desktop Entry]";
            continue;
        }
        if !in_entry || line.starts_with('#') {
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            keys.entry(k.trim().to_string()).or_insert_with(|| v.trim().to_string());
        }
    }
    let truthy = |k: &str| keys.get(k).map_or(false, |v| v == "true");
    if keys.get("Type").map(String::as_str) != Some("Application") || truthy("NoDisplay") || truthy("Hidden") {
        return None;
    }
    if let Some(only) = keys.get("OnlyShowIn") {
        if !only.split(';').any(|d| d == "Lomiri" || d == "Unity") {
            return None;
        }
    }
    Some(App {
        id: id.into(),
        name: keys.get("Name")?.clone(),
        icon: keys.get("Icon").cloned().unwrap_or_default(),
        keywords: keys.get("Keywords").map(|k| k.split(';').filter(|s| !s.is_empty()).map(|s| s.to_lowercase()).collect()).unwrap_or_default(),
    })
}

pub fn installed_apps() -> Vec<App> {
    let mut seen = BTreeMap::new();
    for d in data_dirs() {
        let Ok(rd) = std::fs::read_dir(d.join("applications")) else { continue };
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            let Some(id) = name.strip_suffix(".desktop") else { continue };
            if seen.contains_key(id) {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(e.path()) else { continue };
            if let Some(app) = parse_desktop(id, &text) {
                seen.insert(id.to_string(), app);
            }
        }
    }
    let mut v: Vec<App> = seen.into_values().collect();
    v.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    v
}

/// Best matches for a name ("camera", "messages", "the browser").
pub fn find_apps(query: &str, apps: &[App]) -> Vec<App> {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return vec![];
    }
    let mut scored: Vec<(u32, &App)> = apps
        .iter()
        .filter_map(|a| {
            let n = a.name.to_lowercase();
            let score = if n == q || a.id.to_lowercase() == q {
                100
            } else if n.starts_with(&q) {
                80
            } else if n.contains(&q) {
                60
            } else if a.keywords.iter().any(|k| k.starts_with(&q)) {
                40
            } else if a.id.to_lowercase().contains(&q) {
                30
            } else {
                return None;
            };
            Some((score, a))
        })
        .collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.name.cmp(&b.1.name)));
    scored.into_iter().map(|(_, a)| a.clone()).collect()
}

pub fn valid_app_id(id: &str) -> bool {
    !id.is_empty() && id.len() < 256 && id.chars().all(|c| c.is_ascii_alphanumeric() || "._-+".contains(c))
}

pub async fn dispatch(conn: &zbus::Connection, url: &str) -> Result<(), String> {
    call(conn, "com.lomiri.URLDispatcher", "/com/lomiri/URLDispatcher", "com.lomiri.URLDispatcher", "DispatchURL", &(url, "")).await.map(|_| ())
}

struct ListApps;
#[async_trait]
impl Tool for ListApps {
    fn spec(&self) -> ToolSpec {
        spec("apps_list", "Apps", "List apps", "List installed apps (id and name), optionally filtered by a name.", schema(&[("query", "string", "Part of the name", false)]), false, false)
    }
    fn resources(&self, _a: &Value, _s: &Settings) -> Result<Vec<Resource>, String> {
        Ok(vec![])
    }
    async fn run(&self, a: Value, _ctx: &Ctx) -> Result<Output, String> {
        let apps = installed_apps();
        let list = match a.get("query").and_then(Value::as_str).filter(|q| !q.is_empty()) {
            Some(q) => find_apps(q, &apps),
            None => apps,
        };
        Ok(Output::json(&json!(list.iter().map(|a| json!({"id": a.id, "name": a.name})).collect::<Vec<_>>())))
    }
}

struct OpenApp;
#[async_trait]
impl Tool for OpenApp {
    fn spec(&self) -> ToolSpec {
        spec("app_open", "Apps", "Open an app", "Start an app by id (from apps_list) or by name.", schema(&[("app", "string", "App id or name", true)]), false, false)
    }
    fn resources(&self, _a: &Value, _s: &Settings) -> Result<Vec<Resource>, String> {
        Ok(vec![Resource::new("apps")])
    }
    async fn run(&self, a: Value, ctx: &Ctx) -> Result<Output, String> {
        let q = arg_str(&a, "app")?;
        let apps = installed_apps();
        let app = apps.iter().find(|x| x.id == q).cloned().or_else(|| find_apps(q, &apps).into_iter().next()).ok_or_else(|| format!("no app called \"{q}\""))?;
        if !valid_app_id(&app.id) {
            return Err("invalid app id".into());
        }
        dispatch(ctx.session()?, &format!("application:///{}.desktop", app.id)).await?;
        Ok(Output::text(format!("Opened {}.", app.name)))
    }
}

/// Schemes the assistant may open directly. tel:/sms:/mailto: are left to
/// the dedicated, confirmed tools.
pub fn url_allowed(url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    ["https://", "http://", "geo:", "settings:///", "alarm://", "calendar://", "music://", "video://"].iter().any(|p| lower.starts_with(p)) && !url.chars().any(|c| c.is_control())
}

struct OpenUrl;
#[async_trait]
impl Tool for OpenUrl {
    fn spec(&self) -> ToolSpec {
        spec(
            "url_open",
            "Apps",
            "Open a link",
            "Open a link in the right app: web pages (browser), geo:lat,lon (maps), settings:///system/<page> (System Settings pages, e.g. wifi, bluetooth, sound, battery).",
            schema(&[("url", "string", "The link", true)]),
            false,
            false,
        )
    }
    fn resources(&self, a: &Value, _s: &Settings) -> Result<Vec<Resource>, String> {
        let u = arg_str(a, "url")?;
        if !url_allowed(u) {
            return Err("only http(s), geo:, settings:/// and app links can be opened (use the call/message tools for numbers)".into());
        }
        Ok(vec![Resource::new("apps")])
    }
    async fn run(&self, a: Value, ctx: &Ctx) -> Result<Output, String> {
        let u = arg_str(&a, "url")?;
        dispatch(ctx.session()?, u).await?;
        Ok(Output::text(format!("Opened {u}.")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desktop_parsing_and_search() {
        let cam = parse_desktop("lomiri-camera-app", "[Desktop Entry]\nType=Application\nName=Camera\nKeywords=photo;picture;\nIcon=camera-app\n").unwrap();
        let msg = parse_desktop("lomiri-messaging-app", "[Desktop Entry]\nType=Application\nName=Messaging\n").unwrap();
        assert!(parse_desktop("x", "[Desktop Entry]\nType=Application\nName=X\nNoDisplay=true\n").is_none());
        assert!(parse_desktop("y", "[Desktop Entry]\nType=Application\nName=Y\nOnlyShowIn=GNOME;\n").is_none());
        let apps = vec![cam.clone(), msg];
        assert_eq!(find_apps("cam", &apps)[0].id, "lomiri-camera-app");
        assert_eq!(find_apps("photo", &apps)[0].id, "lomiri-camera-app");
        assert!(find_apps("zzz", &apps).is_empty());
    }

    #[test]
    fn urls() {
        assert!(url_allowed("https://example.com"));
        assert!(url_allowed("settings:///system/wifi"));
        assert!(!url_allowed("tel:+999"));
        assert!(!url_allowed("file:///etc/passwd"));
        assert!(!url_allowed("javascript:alert(1)"));
    }
}
