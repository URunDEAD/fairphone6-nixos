// SPDX-License-Identifier: GPL-3.0-only
//! fp6-privacy-indicator: status-bar indicator that appears while an app
//! uses the camera, the microphone or the location service, with one icon
//! per sensor. Its menu names the apps (tap one to switch to it) and links
//! to Settings > Security & Privacy.
//!
//! Sources:
//!  * camera: /run/fp6-privacy/camera.json, written by the root monitor
//!    (fp6-privacy-system camera-monitor: inotify on the camss capture nodes,
//!    /proc scan; covers libcamera inside apps, Waydroid's camera HAL), plus
//!    PipeWire video capture streams;
//!  * microphone: PipeWire capture streams of apps (`pw-dump --monitor`),
//!    PipeWire's own (echo canceller, loopbacks) and sink monitors excluded;
//!  * location: GeoClue Manager.InUse, apps from the GeoClue agent
//!    (fp6-location-indicator) authorisations in
//!    $XDG_RUNTIME_DIR/fp6-privacy/location-apps.json.
//! GSettings com.fairphone6.privacy indicators-enabled hides it.

use std::collections::HashMap;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use fp6_privacy::ayatana::{self, owned, themed_icon, Indicator, Item, Model};
use fp6_privacy::{inotify, log, procinfo, pw, User};
use gio::prelude::*;
use serde_json::Value;
use zbus::message::Type as MessageType;
use zbus::zvariant::{OwnedValue, Value as ZValue};
use zbus::MatchRule;

const BUS_NAME: &str = "com.lomiri.indicator.privacy";
const ACTIONS_PATH: &str = "/com/lomiri/indicator/privacy";
const MENU_PHONE: &str = "/com/lomiri/indicator/privacy/phone";
const MENU_DESKTOP: &str = "/com/lomiri/indicator/privacy/desktop";
const SCHEMA: &str = "com.fairphone6.privacy";
const CAMERA_STATE: &str = "/run/fp6-privacy/camera.json";
const GEOCLUE: &str = "org.freedesktop.GeoClue2";
const GEOCLUE_MANAGER: &str = "/org/freedesktop/GeoClue2/Manager";

#[derive(Default)]
struct Sources {
    camera_sys: Vec<User>,
    pw_camera: Vec<User>,
    mic: Vec<User>,
    location_in_use: bool,
    location_since: f64,
    location_apps: Vec<(String, f64)>,
}

struct App {
    sources: Mutex<Sources>,
    enabled: AtomicBool,
    indicator: OnceLock<Indicator>,
    /// app ids behind the "open-N" actions of the current menu
    targets: Mutex<Vec<String>>,
}

fn now() -> f64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0)
}

fn runtime_dir() -> PathBuf {
    std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(format!("/run/user/{}", unsafe { libc::getuid() })))
        .join("fp6-privacy")
}

fn location_users(s: &Sources) -> Vec<User> {
    if !s.location_in_use {
        return Vec::new();
    }
    let mut users: Vec<User> = s
        .location_apps
        .iter()
        .filter(|(_, t)| *t >= s.location_since - 5.0)
        .map(|(id, _)| User { name: procinfo::desktop_name(id).unwrap_or_else(|| id.clone()), id: id.clone(), pid: 0 })
        .collect();
    users.sort();
    users.dedup_by(|a, b| a.id == b.id);
    if users.is_empty() {
        users.push(User { name: "An app".into(), id: String::new(), pid: 0 });
    }
    users
}

fn merge(a: &[User], b: &[User]) -> Vec<User> {
    let mut v: Vec<User> = a.iter().chain(b).cloned().collect();
    v.sort();
    v.dedup_by(|x, y| x.name == y.name && x.id == y.id);
    v
}

impl App {
    fn refresh(&self) {
        let Some(ind) = self.indicator.get() else { return };
        let (camera, mic, location) = {
            let s = self.sources.lock().unwrap();
            (merge(&s.camera_sys, &s.pw_camera), merge(&s.mic, &[]), location_users(&s))
        };
        let enabled = self.enabled.load(Ordering::SeqCst);
        let mut icons: Vec<ZValue> = Vec::new();
        let mut desc: Vec<String> = Vec::new();
        let mut items: Vec<Item> = Vec::new();
        let mut targets: Vec<String> = Vec::new();
        for (users, label, icon) in [
            (&camera, "Camera", ["camera-symbolic", "camera-photo-symbolic"]),
            (&mic, "Microphone", ["audio-input-microphone-symbolic", "microphone"]),
            (&location, "Location", ["location-active", "gps"]),
        ] {
            if users.is_empty() {
                continue;
            }
            icons.push(themed_icon(&icon));
            let names: Vec<&str> = users.iter().map(|u| u.name.as_str()).collect();
            desc.push(format!("{label} in use by {}", names.join(", ")));
            for u in users.iter() {
                let detail = if u.pid > 0 { format!(" (pid {})", u.pid) } else { String::new() };
                let action = if !u.id.is_empty() && procinfo::desktop_name(&u.id).is_some() {
                    targets.push(u.id.clone());
                    format!("open-{}", targets.len() - 1)
                } else {
                    String::new()
                };
                items.push(ayatana::item(&format!("{label}: {}{detail}", u.name), &action));
            }
        }
        if items.is_empty() {
            items.push(ayatana::item("No app is using the camera, microphone or location", ""));
        }
        items.push(ayatana::item("Privacy settings…", "settings"));
        let mut state: HashMap<String, OwnedValue> = HashMap::new();
        state.insert("title".into(), owned("Privacy"));
        state.insert(
            "accessible-desc".into(),
            owned(if desc.is_empty() { "No sensor in use".to_string() } else { desc.join("; ") }),
        );
        if !icons.is_empty() {
            state.insert("icons".into(), owned(ZValue::new(icons.clone())));
            state.insert("icon".into(), owned(icons[0].try_clone().unwrap()));
        }
        state.insert("visible".into(), owned(enabled && !desc.is_empty()));
        let mut actions: Vec<(String, bool)> = vec![("settings".into(), true)];
        actions.extend((0..targets.len()).map(|i| (format!("open-{i}"), true)));
        *self.targets.lock().unwrap() = targets;
        ind.update(state, items, actions);
    }

    fn activate(&self, name: &str) {
        let spawn = |cmd: &str, arg: &str| {
            if let Err(e) = Command::new(cmd).arg(arg).stdout(Stdio::null()).stderr(Stdio::null()).spawn() {
                log(format!("{cmd} {arg}: {e}"));
            }
        };
        if name == "settings" {
            spawn("lomiri-url-dispatcher", "settings:///system/security-privacy");
        } else if let Some(i) = name.strip_prefix("open-").and_then(|n| n.parse::<usize>().ok()) {
            if let Some(id) = self.targets.lock().unwrap().get(i).cloned() {
                spawn("lomiri-app-launch", &id);
            }
        }
    }
}

// ---- camera (root monitor's file) + location apps file --------------------
fn read_users(path: &Path) -> Vec<User> {
    let Ok(text) = std::fs::read_to_string(path) else { return Vec::new() };
    let Ok(Value::Array(items)) = serde_json::from_str::<Value>(&text) else { return Vec::new() };
    items
        .iter()
        .filter_map(|i| {
            Some(User {
                name: i.get("name")?.as_str()?.to_owned(),
                id: i.get("id").and_then(Value::as_str).unwrap_or("").to_owned(),
                pid: i.get("pid").and_then(Value::as_u64).unwrap_or(0) as u32,
            })
        })
        .collect()
}

fn read_location_apps(path: &Path) -> Vec<(String, f64)> {
    let Ok(text) = std::fs::read_to_string(path) else { return Vec::new() };
    let Ok(v) = serde_json::from_str::<Value>(&text) else { return Vec::new() };
    v.get("apps")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter(|i| i.get("granted").and_then(Value::as_bool).unwrap_or(true))
                .filter_map(|i| Some((i.get("id")?.as_str()?.to_owned(), i.get("time")?.as_f64()?)))
                .collect()
        })
        .unwrap_or_default()
}

fn watch_files(app: Arc<App>) {
    let rt = runtime_dir();
    let _ = std::fs::create_dir_all(&rt);
    let loc_file = rt.join("location-apps.json");
    loop {
        let ino = match inotify::Inotify::new() {
            Ok(i) => i,
            Err(e) => {
                log(format!("inotify: {e}"));
                return;
            }
        };
        let cam_dir = Path::new(CAMERA_STATE).parent().unwrap();
        let cam_ok = ino.add(cam_dir, inotify::DIR_CHANGES).is_ok();
        if !cam_ok {
            log(format!("{} not watchable (camera monitor not running?)", cam_dir.display()));
        }
        let _ = ino.add(&rt, inotify::DIR_CHANGES);
        loop {
            {
                let mut s = app.sources.lock().unwrap();
                s.camera_sys = read_users(Path::new(CAMERA_STATE));
                s.location_apps = read_location_apps(&loc_file);
            }
            app.refresh();
            match ino.wait(if cam_ok { -1 } else { 30_000 }) {
                Ok(_) => ino.settle(100),
                Err(e) => {
                    log(format!("inotify wait: {e}"));
                    std::thread::sleep(Duration::from_secs(5));
                }
            }
            if !cam_ok && cam_dir.exists() {
                break; // re-add the watch now that the directory exists
            }
        }
    }
}

// ---- PipeWire --------------------------------------------------------------
fn capture_users(caps: &[pw::Capture]) -> Vec<User> {
    caps.iter()
        .map(|c| {
            if c.pid > 0 && Path::new(&format!("/proc/{}", c.pid)).exists() {
                let mut u = procinfo::user_of(Path::new("/proc"), c.pid);
                if u.id == u.name && !c.app_name.is_empty() {
                    u.name = c.app_name.clone();
                }
                u
            } else {
                let name = if c.app_name.is_empty() { c.binary.clone() } else { c.app_name.clone() };
                User { name, id: c.binary.clone(), pid: c.pid }
            }
        })
        .collect()
}

fn watch_pipewire(app: Arc<App>) {
    let pw_dump = std::env::var("FP6_PW_DUMP").unwrap_or_else(|_| "pw-dump".into());
    loop {
        let child = Command::new(&pw_dump).arg("--monitor").arg("--no-colors").stdout(Stdio::piped()).stderr(Stdio::null()).spawn();
        match child {
            Ok(mut child) => {
                let out = child.stdout.take().unwrap();
                let mut graph = pw::Graph::default();
                let stream = serde_json::Deserializer::from_reader(BufReader::new(out)).into_iter::<Value>();
                for doc in stream {
                    let Ok(doc) = doc else { break };
                    graph.apply(&doc);
                    let mic = capture_users(&graph.microphone());
                    let cam = capture_users(&graph.camera());
                    let changed = {
                        let mut s = app.sources.lock().unwrap();
                        let changed = s.mic != mic || s.pw_camera != cam;
                        s.mic = mic;
                        s.pw_camera = cam;
                        changed
                    };
                    if changed {
                        app.refresh();
                    }
                }
                let _ = child.kill();
                let _ = child.wait();
                log("pw-dump ended; restarting in 5 s");
            }
            Err(e) => log(format!("{pw_dump}: {e}; retrying in 30 s")),
        }
        {
            let mut s = app.sources.lock().unwrap();
            s.mic.clear();
            s.pw_camera.clear();
        }
        app.refresh();
        std::thread::sleep(Duration::from_secs(5));
    }
}

// ---- GeoClue -----------------------------------------------------------------
fn geoclue_in_use(conn: &zbus::blocking::Connection) -> bool {
    let dbus = zbus::blocking::fdo::DBusProxy::new(conn);
    let running = dbus
        .ok()
        .and_then(|p| p.name_has_owner(GEOCLUE.try_into().unwrap()).ok())
        .unwrap_or(false);
    if !running {
        return false; // do not D-Bus-activate GeoClue just to ask
    }
    conn.call_method(Some(GEOCLUE), GEOCLUE_MANAGER, Some("org.freedesktop.DBus.Properties"), "Get", &("org.freedesktop.GeoClue2.Manager", "InUse"))
        .ok()
        .and_then(|r| r.body().deserialize::<OwnedValue>().ok())
        .and_then(|v| bool::try_from(&v).ok())
        .unwrap_or(false)
}

fn set_location(app: &App, in_use: bool) {
    let changed = {
        let mut s = app.sources.lock().unwrap();
        let changed = s.location_in_use != in_use;
        if changed && in_use {
            s.location_since = now();
        }
        s.location_in_use = in_use;
        changed
    };
    if changed {
        app.refresh();
    }
}

fn watch_geoclue(app: Arc<App>) -> zbus::Result<()> {
    let conn = zbus::blocking::Connection::system()?;
    let props = MatchRule::builder()
        .msg_type(MessageType::Signal)
        .interface("org.freedesktop.DBus.Properties")?
        .member("PropertiesChanged")?
        .path(GEOCLUE_MANAGER)?
        .build();
    let owners = MatchRule::builder()
        .msg_type(MessageType::Signal)
        .sender("org.freedesktop.DBus")?
        .interface("org.freedesktop.DBus")?
        .member("NameOwnerChanged")?
        .arg(0, GEOCLUE)?
        .build();
    let prop_iter = zbus::blocking::MessageIterator::for_match_rule(props, &conn, Some(16))?;
    let owner_iter = zbus::blocking::MessageIterator::for_match_rule(owners, &conn, Some(16))?;
    {
        let app = app.clone();
        let conn = conn.clone();
        std::thread::spawn(move || {
            for msg in owner_iter.flatten() {
                if let Ok((_, _, new_owner)) = msg.body().deserialize::<(String, String, String)>() {
                    set_location(&app, !new_owner.is_empty() && geoclue_in_use(&conn));
                }
            }
        });
    }
    set_location(&app, geoclue_in_use(&conn));
    for msg in prop_iter.flatten() {
        if let Ok((iface, changed, _)) = msg.body().deserialize::<(String, HashMap<String, OwnedValue>, Vec<String>)>() {
            if iface == "org.freedesktop.GeoClue2.Manager" {
                if let Some(v) = changed.get("InUse").and_then(|v| bool::try_from(v).ok()) {
                    set_location(&app, v);
                }
            }
        }
    }
    Ok(())
}

fn main() {
    let app = Arc::new(App {
        sources: Mutex::new(Sources::default()),
        enabled: AtomicBool::new(true),
        indicator: OnceLock::new(),
        targets: Mutex::new(Vec::new()),
    });
    let settings = gio::SettingsSchemaSource::default()
        .and_then(|s| s.lookup(SCHEMA, true))
        .map(|_| gio::Settings::new(SCHEMA));
    if let Some(s) = &settings {
        app.enabled.store(s.boolean("indicators-enabled"), Ordering::SeqCst);
        let a = app.clone();
        s.connect_changed(Some("indicators-enabled"), move |s, key| {
            a.enabled.store(s.boolean(key), Ordering::SeqCst);
            a.refresh();
        });
    } else {
        log(format!("GSettings schema {SCHEMA} missing: indicator always enabled"));
    }
    let on_activate = {
        let app = app.clone();
        Arc::new(move |name: &str| app.activate(name))
    };
    let indicator = match Indicator::serve(BUS_NAME, ACTIONS_PATH, &[MENU_PHONE, MENU_DESKTOP], Model::default(), on_activate) {
        Ok(i) => i,
        Err(e) => {
            log(format!("cannot serve {BUS_NAME}: {e}"));
            std::process::exit(1);
        }
    };
    let _ = app.indicator.set(indicator);
    app.refresh();
    for f in [watch_files as fn(Arc<App>), watch_pipewire] {
        let a = app.clone();
        std::thread::spawn(move || f(a));
    }
    {
        let a = app.clone();
        std::thread::spawn(move || loop {
            if let Err(e) = watch_geoclue(a.clone()) {
                log(format!("GeoClue watch: {e}"));
            }
            std::thread::sleep(Duration::from_secs(10));
        });
    }
    log("ready");
    glib::MainLoop::new(None, false).run();
}
