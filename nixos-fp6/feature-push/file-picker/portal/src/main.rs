//! xdg-desktop-portal-lomiri: org.freedesktop.impl.portal.FileChooser for
//! the Lomiri session on the Fairphone 6.
//!
//! Every OpenFile / SaveFile / SaveFiles call starts the Lomiri UI Toolkit
//! picker (`fp6-file-picker`) as its own Lomiri app window, hands it the
//! request as JSON on stdin and reads the answer as JSON from stdout. The
//! org.freedesktop.impl.portal.Request object at the request handle closes
//! the picker (SIGTERM) when the frontend cancels the call.
//!
//! Also org.freedesktop.impl.portal.Access (camera, location and other
//! permission-store portals), forwarded to fp6-permission-agent's prompt
//! (app-permissions); routed here only when portals.conf says so.

mod mime;
mod places;
mod request;
mod uri;

use request::{Caller, Mode, Options};
use serde_json::Value as Json;
use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use zbus::object_server::ObjectServer;
use zbus::zvariant::{OwnedObjectPath, OwnedValue};

const BUS_NAME: &str = "org.freedesktop.impl.portal.desktop.lomiri";
const OBJECT_PATH: &str = "/org/freedesktop/portal/desktop";

#[derive(Default)]
struct Pending {
    pid: Mutex<Option<u32>>,
    closed: AtomicBool,
}

struct Request {
    pending: Arc<Pending>,
}

#[zbus::interface(name = "org.freedesktop.impl.portal.Request")]
impl Request {
    fn close(&self) {
        self.pending.closed.store(true, Ordering::SeqCst);
        if let Some(pid) = *self.pending.pid.lock().unwrap() {
            // SAFETY: plain kill(2) of our own child.
            unsafe {
                libc::kill(pid as libc::pid_t, libc::SIGTERM);
            }
        }
    }
}

struct FileChooser {
    picker: PathBuf,
    desktop_hint: Option<String>,
}

fn env_or(name: &str, default: &str) -> String {
    std::env::var(name).ok().filter(|v| !v.is_empty()).unwrap_or_else(|| default.to_string())
}

/// Display name of the calling app from its desktop file (Name=), if any.
fn app_name(app_id: &str) -> String {
    if app_id.is_empty() || app_id.contains('/') {
        return String::new();
    }
    for dir in mime::data_dirs() {
        let f = dir.join("applications").join(format!("{app_id}.desktop"));
        if let Ok(text) = std::fs::read_to_string(&f) {
            let mut in_entry = false;
            for line in text.lines() {
                let line = line.trim();
                if line.starts_with('[') {
                    in_entry = line == "[Desktop Entry]";
                } else if in_entry {
                    if let Some(n) = line.strip_prefix("Name=") {
                        return n.to_string();
                    }
                }
            }
        }
    }
    String::new()
}

impl FileChooser {
    /// Runs the picker for one request (blocking; called on a worker thread).
    fn run_picker(&self, req: &Json, pending: &Pending) -> Json {
        let mut cmd = Command::new(&self.picker);
        cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::inherit());
        // The user manager has no WAYLAND_DISPLAY in this session (see
        // privacy-lock-fp6.nix); Lomiri's socket is wayland-0.
        if std::env::var_os("WAYLAND_DISPLAY").map_or(true, |v| v.is_empty()) {
            cmd.env("WAYLAND_DISPLAY", "wayland-0");
        }
        if std::env::var_os("QT_QPA_PLATFORM").map_or(true, |v| v.is_empty()) {
            cmd.env("QT_QPA_PLATFORM", "wayland");
        }
        // qtmir maps the client pid to an app through DESKTOP_FILE_HINT
        // (application_manager.cpp tryFindApp): the picker gets its own
        // name and icon in the spread instead of a "no desktop file" entry.
        if let Some(hint) = &self.desktop_hint {
            cmd.env("DESKTOP_FILE_HINT", hint);
        }
        cmd.env("APP_ID", "fp6-file-picker");
        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => {
                eprintln!("xdg-desktop-portal-lomiri: cannot start {}: {e}", self.picker.display());
                return serde_json::json!({ "response": 2 });
            }
        };
        *pending.pid.lock().unwrap() = Some(child.id());
        if pending.closed.load(Ordering::SeqCst) {
            let _ = child.kill();
        }
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(req.to_string().as_bytes());
            // dropping stdin closes it: the picker reads to EOF
        }
        let mut out = String::new();
        if let Some(mut stdout) = child.stdout.take() {
            let _ = stdout.read_to_string(&mut out);
        }
        let status = child.wait();
        *pending.pid.lock().unwrap() = None;
        if pending.closed.load(Ordering::SeqCst) {
            return serde_json::json!({ "response": 1 });
        }
        // The answer is the last JSON line (Qt may print warnings to stdout).
        let answer = out
            .lines()
            .rev()
            .find_map(|l| serde_json::from_str::<Json>(l.trim()).ok().filter(Json::is_object));
        match answer {
            Some(a) => a,
            None => {
                eprintln!("xdg-desktop-portal-lomiri: picker ended without an answer ({status:?})");
                // Swiped away in the spread or crashed: treat as cancelled.
                serde_json::json!({ "response": 1 })
            }
        }
    }

    async fn handle(
        &self,
        server: &ObjectServer,
        mode: Mode,
        handle: OwnedObjectPath,
        app_id: String,
        title: String,
        options: HashMap<String, OwnedValue>,
    ) -> (u32, HashMap<String, OwnedValue>) {
        let opts = Options::parse(&options);
        let caller = Caller { name: app_name(&app_id), app_id };
        let db = mime::MimeDb::load();
        let mut req = request::build(mode, &title, &caller, &opts, &db);
        req["places"] = Json::from(places::places());
        req["recent"] = Json::from(places::recent(80));
        req["home"] = Json::from(places::home().to_string_lossy().into_owned());
        req["contentHub"] = Json::from(env_or("FP6_PICKER_CONTENT_HUB", "1") == "1");

        let pending = Arc::new(Pending::default());
        let registered = server.at(&handle, Request { pending: pending.clone() }).await.unwrap_or(false);
        let picker = FileChooser { picker: self.picker.clone(), desktop_hint: self.desktop_hint.clone() };
        let p2 = pending.clone();
        let answer = blocking::unblock(move || picker.run_picker(&req, &p2)).await;
        if registered {
            let _ = server.remove::<Request, _>(&handle).await;
        }
        let out = request::results(mode, &opts, &answer);
        eprintln!(
            "xdg-desktop-portal-lomiri: {mode:?} for '{}' -> response {} ({} uri(s))",
            caller.app_id,
            out.0,
            out.1.get("uris").and_then(|u| <Vec<String>>::try_from(u.try_clone().ok()?).ok()).map_or(0, |v| v.len())
        );
        out
    }
}

#[zbus::interface(name = "org.freedesktop.impl.portal.FileChooser")]
impl FileChooser {
    async fn open_file(
        &self,
        #[zbus(object_server)] server: &ObjectServer,
        handle: OwnedObjectPath,
        app_id: String,
        _parent_window: String,
        title: String,
        options: HashMap<String, OwnedValue>,
    ) -> (u32, HashMap<String, OwnedValue>) {
        self.handle(server, Mode::Open, handle, app_id, title, options).await
    }

    async fn save_file(
        &self,
        #[zbus(object_server)] server: &ObjectServer,
        handle: OwnedObjectPath,
        app_id: String,
        _parent_window: String,
        title: String,
        options: HashMap<String, OwnedValue>,
    ) -> (u32, HashMap<String, OwnedValue>) {
        self.handle(server, Mode::Save, handle, app_id, title, options).await
    }

    async fn save_files(
        &self,
        #[zbus(object_server)] server: &ObjectServer,
        handle: OwnedObjectPath,
        app_id: String,
        _parent_window: String,
        title: String,
        options: HashMap<String, OwnedValue>,
    ) -> (u32, HashMap<String, OwnedValue>) {
        self.handle(server, Mode::SaveFiles, handle, app_id, title, options).await
    }
}

/// org.freedesktop.impl.portal.Access: the access dialog xdg-desktop-portal
/// shows for the camera, location and other permission-store portals. The
/// question goes to fp6-permission-agent (app-permissions), which shows it as
/// the Lomiri permission prompt; xdg-desktop-portal remembers the answer in
/// its permission store. Without the agent the request is refused.
struct Access;

const AGENT_NAME: &str = "com.fairphone6.Permissions";
const AGENT_PATH: &str = "/com/fairphone6/Permissions";

fn opt_str(options: &HashMap<String, OwnedValue>, key: &str) -> String {
    options
        .get(key)
        .and_then(|v| String::try_from(v.try_clone().ok()?).ok())
        .unwrap_or_default()
}

/// The JSON fp6-permission-agent's Dialog.Ask takes.
fn access_request(app_id: &str, title: &str, subtitle: &str, body: &str, options: &HashMap<String, OwnedValue>) -> String {
    let detail = [subtitle, body].iter().filter(|s| !s.is_empty()).cloned().collect::<Vec<_>>().join("\n");
    serde_json::json!({
        "app": app_id.trim_end_matches(".desktop"),
        "perm": "portal",
        "title": title,
        "question": title,
        "detail": detail,
        "grant_label": opt_str(options, "grant_label"),
        "deny_label": opt_str(options, "deny_label"),
    })
    .to_string()
}

/// Portal response: 0 granted, 1 refused by the user, 2 other (no agent).
fn access_response(answer: Option<&str>) -> u32 {
    match answer {
        Some("allow") | Some("while_using") => 0,
        Some(_) => 1,
        None => 2,
    }
}

#[zbus::interface(name = "org.freedesktop.impl.portal.Access")]
impl Access {
    #[allow(clippy::too_many_arguments)]
    async fn access_dialog(
        &self,
        #[zbus(connection)] conn: &zbus::Connection,
        _handle: OwnedObjectPath,
        app_id: String,
        _parent_window: String,
        title: String,
        subtitle: String,
        body: String,
        options: HashMap<String, OwnedValue>,
    ) -> (u32, HashMap<String, OwnedValue>) {
        let req = access_request(&app_id, &title, &subtitle, &body, &options);
        let reply = conn
            .call_method(Some(AGENT_NAME), AGENT_PATH, Some("com.fairphone6.Permissions.Dialog"), "Ask", &(req,))
            .await;
        let answer = match reply {
            Ok(m) => m.body().deserialize::<String>().ok(),
            Err(e) => {
                eprintln!("xdg-desktop-portal-lomiri: access dialog for {app_id:?}: {e}");
                None
            }
        };
        eprintln!("xdg-desktop-portal-lomiri: access dialog for {app_id:?}: {answer:?}");
        (access_response(answer.as_deref()), HashMap::new())
    }
}

fn main() -> zbus::Result<()> {
    if std::env::args().any(|a| a == "--version") {
        println!("xdg-desktop-portal-lomiri {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    let chooser = FileChooser {
        picker: PathBuf::from(env_or("FP6_FILE_PICKER", option_env!("FP6_FILE_PICKER_BIN").unwrap_or("fp6-file-picker"))),
        desktop_hint: Some(env_or(
            "FP6_FILE_PICKER_DESKTOP",
            option_env!("FP6_FILE_PICKER_DESKTOP_FILE").unwrap_or("fp6-file-picker.desktop"),
        ))
        .filter(|s| s != "none"),
    };
    let conn = zbus::blocking::connection::Builder::session()?
        .serve_at(OBJECT_PATH, chooser)?
        .serve_at(OBJECT_PATH, Access)?
        .name(BUS_NAME)?
        .build()?;
    eprintln!("xdg-desktop-portal-lomiri: serving {BUS_NAME}");
    // Block until the bus connection closes (session end), then exit instead
    // of lingering without a bus.
    for msg in zbus::blocking::MessageIterator::from(&conn) {
        if msg.is_err() {
            break;
        }
    }
    eprintln!("xdg-desktop-portal-lomiri: session bus closed, exiting");
    Ok(())
}

#[cfg(test)]
mod access_tests {
    use super::*;

    #[test]
    fn access_answers_map_to_portal_responses() {
        assert_eq!(access_response(Some("allow")), 0);
        assert_eq!(access_response(Some("while_using")), 0);
        assert_eq!(access_response(Some("deny")), 1);
        assert_eq!(access_response(Some("dismissed")), 1);
        assert_eq!(access_response(None), 2);
    }

    #[test]
    fn access_request_json() {
        let mut o = HashMap::new();
        o.insert("deny_label".to_string(), OwnedValue::try_from(zbus::zvariant::Value::from("Deny Access")).unwrap());
        let j: serde_json::Value = serde_json::from_str(&access_request("org.gnome.Snapshot.desktop", "Allow Snapshot to use the camera?", "", "Access can be changed later.", &o)).unwrap();
        assert_eq!(j["app"], "org.gnome.Snapshot");
        assert_eq!(j["question"], "Allow Snapshot to use the camera?");
        assert_eq!(j["detail"], "Access can be changed later.");
        assert_eq!(j["deny_label"], "Deny Access");
        assert_eq!(j["grant_label"], "");
    }
}
