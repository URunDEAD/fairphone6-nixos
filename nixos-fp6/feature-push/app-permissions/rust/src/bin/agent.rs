// SPDX-License-Identifier: GPL-3.0-or-later
//! fp6-permission-agent: the phone user's side of the permission broker
//! (systemd user service fp6-permission-agent.service).
//!
//!  * Registers with fp6-permd on the system bus and answers its
//!    org.fairphone.PermissionAgent1.Prompt calls: the question goes to the
//!    Lomiri shell (Fp6PermissionPrompt.qml) through the session action
//!    group com.fairphone6.Permissions ("prompt" state, "ack", "respond");
//!    if the shell does not pick it up within 1.5 s (shell without the
//!    patch, restarting), a snap-decision notification asks instead.
//!  * Forwards the focused app from the shell ("focus") for "Allow only
//!    while using".
//!  * Serves System Settings > Privacy > App permissions: "state" (broker
//!    state + installed apps with names/icons), "set", "net", "reset",
//!    "refresh".
//!  * Usage history for camera / location from the privacy indicator's
//!    files (/run/fp6-privacy/camera.json,
//!    $XDG_RUNTIME_DIR/fp6-privacy/location-apps.json).
//!  * Broker signal Changed(true): `systemctl --user daemon-reload`, so
//!    lomiri-app-launch units pick up the regenerated drop-ins.

use fp6_permissions::{AGENT_IFACE, AGENT_PATH, BUS_NAME, PATH, SESSION_NAME, SESSION_PATH};
use gio::prelude::*;
use glib::{Variant, VariantDict};
use serde_json::{json, Value};
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::rc::Rc;
use std::time::Duration;

const AGENT_XML: &str = r#"<node><interface name="org.fairphone.PermissionAgent1">
<method name="Prompt"><arg name="request" type="s" direction="in"/><arg name="answer" type="s" direction="out"/></method>
</interface></node>"#;
const NOTIFY: &str = "org.freedesktop.Notifications";
const NOTIFY_PATH: &str = "/org/freedesktop/Notifications";

fn log(msg: impl AsRef<str>) {
    eprintln!("fp6-permission-agent: {}", msg.as_ref());
}

struct Pending {
    id: u64,
    req: Value,
    inv: Option<gio::DBusMethodInvocation>,
    shown: bool,
    acked: bool,
    notif: u32,
}

struct Agent {
    session: gio::DBusConnection,
    system: gio::DBusConnection,
    state: gio::SimpleAction,
    prompt: gio::SimpleAction,
    queue: VecDeque<Pending>,
    next_id: u64,
    broker_up: bool,
    registered: bool,
    focus: String,
    /// The shell has reported focus at least once (without it the broker
    /// treats every app as in use, rather than none).
    shell_seen: bool,
    broker_state: Value,
    camera_seen: BTreeSet<String>,
    location_last: f64,
    names: BTreeMap<String, (String, String)>,
}

type A = Rc<RefCell<Agent>>;

/// Display name and QML icon URL of an app id (desktop file).
fn app_info(id: &str) -> Option<(String, String)> {
    let info = gio::DesktopAppInfo::new(&format!("{id}.desktop"))?;
    let name = info.name().to_string();
    let icon = info
        .icon()
        .and_then(|i| IconExt::to_string(&i))
        .map(|s| {
            let s = s.to_string();
            if s.starts_with('/') {
                format!("file://{s}")
            } else if s.contains("://") {
                s
            } else {
                format!("image://theme/{s}")
            }
        })
        .unwrap_or_default();
    Some((name, icon))
}

fn installed() -> Vec<Value> {
    let mut apps: Vec<Value> = gio::AppInfo::all()
        .into_iter()
        .filter(|a| a.should_show())
        .filter_map(|a| {
            let id = a.id()?.to_string();
            let id = id.strip_suffix(".desktop")?.to_owned();
            if !fp6_permissions::appid::valid_app_id(&id) {
                return None;
            }
            let (name, icon) = app_info(&id).unwrap_or((a.name().to_string(), String::new()));
            Some(json!({"id": id, "name": name, "icon": icon}))
        })
        .collect();
    apps.sort_by_key(|v| v["name"].as_str().unwrap_or("").to_lowercase());
    apps
}

impl Agent {
    fn name_of(&mut self, id: &str) -> (String, String) {
        if let Some(n) = self.names.get(id) {
            return n.clone();
        }
        let n = app_info(id).unwrap_or_else(|| (id.to_owned(), String::new()));
        self.names.insert(id.to_owned(), n.clone());
        n
    }

    fn publish_state(&mut self) {
        let mut st = self.broker_state.clone();
        if !st.is_object() {
            st = json!({});
        }
        let ids: Vec<String> = st["apps"].as_object().map(|m| m.keys().cloned().collect()).unwrap_or_default();
        let mut names = serde_json::Map::new();
        for id in ids {
            let (n, i) = self.name_of(&id);
            names.insert(id, json!({"name": n, "icon": i}));
        }
        st["names"] = Value::Object(names);
        st["installed"] = Value::Array(installed());
        st["broker"] = json!(self.broker_up && self.registered);
        self.state.set_state(&st.to_string().to_variant());
    }

    fn call_broker(this: &A, method: &str, args: Option<Variant>, done: impl FnOnce(&A, Result<Variant, glib::Error>) + 'static) {
        let sys = this.borrow().system.clone();
        let a = this.clone();
        sys.call(Some(BUS_NAME), PATH, BUS_NAME, method, args.as_ref(), None, gio::DBusCallFlags::NONE, 10000, None::<&gio::Cancellable>,
                 move |r| done(&a, r));
    }

    fn register(this: &A) {
        Agent::call_broker(this, "RegisterAgent", None, |a, r| match r {
            Ok(_) => {
                log("registered with fp6-permd");
                a.borrow_mut().registered = true;
                if a.borrow().shell_seen {
                    Agent::send_focus(a);
                }
                Agent::refresh(a);
            }
            Err(e) => {
                log(format!("RegisterAgent: {e}"));
                a.borrow_mut().registered = false;
                a.borrow_mut().publish_state();
            }
        });
    }

    fn refresh(this: &A) {
        if !this.borrow().registered {
            this.borrow_mut().publish_state();
            return;
        }
        Agent::call_broker(this, "GetState", None, |a, r| {
            match r.map(|v| v.child_value(0).get::<String>().unwrap_or_default()) {
                Ok(s) => a.borrow_mut().broker_state = serde_json::from_str(&s).unwrap_or(json!({})),
                Err(e) => log(format!("GetState: {e}")),
            }
            a.borrow_mut().publish_state();
        });
    }

    fn send_focus(this: &A) {
        let app = this.borrow().focus.clone();
        Agent::call_broker(this, "SetForeground", Some((app,).to_variant()), |_, r| {
            if let Err(e) = r {
                log(format!("SetForeground: {e}"));
            }
        });
    }

    fn admin(this: &A, method: &str, args: Variant) {
        let m = method.to_owned();
        Agent::call_broker(this, method, Some(args), move |a, r| {
            if let Err(e) = r {
                log(format!("{m}: {e}"));
            }
            Agent::refresh(a);
        });
    }

    // ---- prompts ---------------------------------------------------------
    fn prompt_json(&mut self, p: &Pending) -> Value {
        let app = p.req["app"].as_str().unwrap_or("").to_owned();
        let (name, icon) = self.name_of(&app);
        let phrase = p.req["phrase"].as_str().unwrap_or("use this");
        // Portal access dialogs bring their own wording and only two answers.
        let question = p.req["question"].as_str().map(str::to_owned).unwrap_or_else(|| format!("Allow {name} to {phrase}?"));
        let options = if p.req["options"].is_array() { p.req["options"].clone() } else { json!(["allow", "while_using", "deny"]) };
        json!({
            "id": p.id,
            "app": app,
            "name": name,
            "icon": icon,
            "perm": p.req["perm"],
            "title": p.req["title"],
            "question": question,
            "detail": p.req["detail"],
            "options": options,
            "grant_label": p.req["grant_label"],
            "deny_label": p.req["deny_label"],
        })
    }

    fn show_next(this: &A) {
        let (id, shown, timeout) = {
            let a = this.borrow();
            match a.queue.front() {
                None => {
                    a.prompt.set_state(&"{}".to_variant());
                    return;
                }
                Some(p) => (p.id, p.shown, p.req["timeout"].as_u64().unwrap_or(60).clamp(5, 600)),
            }
        };
        if shown {
            return;
        }
        {
            let mut a = this.borrow_mut();
            let p = a.queue.pop_front().unwrap();
            let j = a.prompt_json(&p);
            a.queue.push_front(Pending { shown: true, ..p });
            a.prompt.set_state(&j.to_string().to_variant());
        }
        // Shell did not take it: ask with a notification.
        let a = this.clone();
        glib::timeout_add_local_once(Duration::from_millis(1500), move || {
            let need = a.borrow().queue.front().is_some_and(|p| p.id == id && !p.acked && p.notif == 0);
            if need {
                Agent::notify(&a, id);
            }
        });
        let a = this.clone();
        glib::timeout_add_local_once(Duration::from_secs(timeout), move || Agent::finish(&a, id, "dismissed"));
    }

    fn notify(this: &A, id: u64) {
        let (session, j) = {
            let mut a = this.borrow_mut();
            let Some(p) = a.queue.iter().find(|p| p.id == id) else { return };
            let p = Pending { id: p.id, req: p.req.clone(), inv: None, shown: true, acked: false, notif: 0 };
            (a.session.clone(), a.prompt_json(&p))
        };
        let h = VariantDict::new(None);
        h.insert_value("x-lomiri-snap-decisions", &true.to_variant());
        h.insert_value("x-lomiri-snap-decisions-timeout", &(60000i32).to_variant());
        h.insert_value("urgency", &2u8.to_variant());
        // Lomiri: first action = positive button, second = negative, the
        // rest in the drop-down.
        let mut actions: Vec<String> = ["allow", "Allow", "deny", "Don't allow"].iter().map(|s| s.to_string()).collect();
        if j["options"].as_array().is_none_or(|o| o.iter().any(|x| x == "while_using")) {
            actions.extend(["while_using".to_string(), "Only while using the app".to_string()]);
        }
        let detail = j["detail"].as_str().unwrap_or("");
        let args = Variant::tuple_from_iter([
            "Permissions".to_variant(),
            0u32.to_variant(),
            j["icon"].as_str().unwrap_or("security-alert").replace("image://theme/", "").replace("file://", "").to_variant(),
            j["question"].as_str().unwrap_or("").to_variant(),
            detail.to_variant(),
            actions.to_variant(),
            h.end(),
            60000i32.to_variant(),
        ]);
        let a = this.clone();
        session.call(Some(NOTIFY), NOTIFY_PATH, NOTIFY, "Notify", Some(&args), None, gio::DBusCallFlags::NONE, 3000,
                     None::<&gio::Cancellable>, move |r| match r {
                         Ok(v) => {
                             let nid = v.child_value(0).get::<u32>().unwrap_or(0);
                             if let Some(p) = a.borrow_mut().queue.iter_mut().find(|p| p.id == id) {
                                 p.notif = nid;
                             }
                         }
                         Err(e) => log(format!("Notify: {e}")),
                     });
    }

    fn finish(this: &A, id: u64, answer: &str) {
        let (p, session) = {
            let mut a = this.borrow_mut();
            let Some(pos) = a.queue.iter().position(|p| p.id == id) else { return };
            (a.queue.remove(pos).unwrap(), a.session.clone())
        };
        let answer = match answer {
            "allow" | "while_using" | "deny" => answer,
            _ => "dismissed",
        };
        log(format!("{} {}: {answer}", p.req["app"].as_str().unwrap_or("?"), p.req["perm"].as_str().unwrap_or("?")));
        if let Some(inv) = p.inv {
            inv.return_value(Some(&(answer,).to_variant()));
        }
        if p.notif != 0 {
            session.call(Some(NOTIFY), NOTIFY_PATH, NOTIFY, "CloseNotification", Some(&(p.notif,).to_variant()), None,
                         gio::DBusCallFlags::NONE, 3000, None::<&gio::Cancellable>, |_| {});
        }
        this.borrow().prompt.set_state(&"{}".to_variant());
        Agent::show_next(this);
    }

    // ---- camera / location usage from the privacy indicator ---------------
    fn poll_usage(this: &A) {
        let mut events = vec![];
        {
            let mut a = this.borrow_mut();
            let cam: Vec<Value> = std::fs::read_to_string("/run/fp6-privacy/camera.json")
                .ok()
                .and_then(|t| serde_json::from_str(&t).ok())
                .and_then(|v: Value| v.as_array().cloned())
                .unwrap_or_default();
            let now_ids: BTreeSet<String> = cam
                .iter()
                .filter_map(|u| u["id"].as_str().filter(|s| !s.is_empty()).or(u["name"].as_str()).map(str::to_owned))
                .collect();
            for id in now_ids.difference(&a.camera_seen) {
                events.push((id.clone(), "camera", String::new()));
            }
            a.camera_seen = now_ids;
            let rt = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| format!("/run/user/{}", unsafe { libc::getuid() }));
            let loc: Value = std::fs::read_to_string(format!("{rt}/fp6-privacy/location-apps.json"))
                .ok()
                .and_then(|t| serde_json::from_str(&t).ok())
                .unwrap_or(json!({}));
            let mut newest = a.location_last;
            for e in loc["apps"].as_array().into_iter().flatten() {
                let t = e["time"].as_f64().unwrap_or(0.0);
                if t > a.location_last {
                    if let Some(id) = e["id"].as_str() {
                        let granted = e["granted"].as_bool().unwrap_or(true);
                        events.push((id.trim_end_matches(".desktop").to_owned(), "location", if granted { String::new() } else { "refused (location off)".into() }));
                    }
                    newest = newest.max(t);
                }
            }
            a.location_last = newest;
        }
        if !this.borrow().registered {
            return;
        }
        for (app, what, detail) in events {
            Agent::call_broker(this, "LogUse", Some((app, what, detail).to_variant()), |_, r| {
                if let Err(e) = r {
                    log(format!("LogUse: {e}"));
                }
            });
        }
    }
}

const DIALOG_XML: &str = r#"<node><interface name="com.fairphone6.Permissions.Dialog">
<method name="Ask"><arg name="request" type="s" direction="in"/><arg name="answer" type="s" direction="out"/></method>
</interface></node>"#;

/// The caller runs in xdg-desktop-portal-lomiri.service (its cgroup).
fn sender_is_portal(conn: &gio::DBusConnection, sender: Option<&str>) -> bool {
    let Some(sender) = sender else { return false };
    let pid = conn
        .call_sync(Some("org.freedesktop.DBus"), "/org/freedesktop/DBus", "org.freedesktop.DBus", "GetConnectionUnixProcessID",
                   Some(&(sender,).to_variant()), None, gio::DBusCallFlags::NONE, 2000, None::<&gio::Cancellable>)
        .ok()
        .and_then(|v| v.child_value(0).get::<u32>());
    let Some(pid) = pid else { return false };
    std::fs::read_to_string(format!("/proc/{pid}/cgroup"))
        .ok()
        .and_then(|t| fp6_permissions::appid::cgroup_of(&t))
        .is_some_and(|cg| fp6_permissions::appid::leaf_unit(&cg) == Some("xdg-desktop-portal-lomiri.service"))
}

fn split3(s: &str) -> Option<(String, String, String)> {
    let mut it = s.splitn(3, '\t');
    Some((it.next()?.to_owned(), it.next()?.to_owned(), it.next()?.to_owned()))
}

fn main() {
    let session = gio::bus_get_sync(gio::BusType::Session, None::<&gio::Cancellable>).unwrap_or_else(|e| {
        log(format!("session bus: {e}"));
        std::process::exit(1)
    });
    let system = gio::bus_get_sync(gio::BusType::System, None::<&gio::Cancellable>).unwrap_or_else(|e| {
        log(format!("system bus: {e}"));
        std::process::exit(1)
    });
    let group = gio::SimpleActionGroup::new();
    let state = gio::SimpleAction::new_stateful("state", None, &"{}".to_variant());
    let prompt = gio::SimpleAction::new_stateful("prompt", None, &"{}".to_variant());
    group.add_action(&state);
    group.add_action(&prompt);
    let agent: A = Rc::new(RefCell::new(Agent {
        session: session.clone(),
        system: system.clone(),
        state,
        prompt,
        queue: VecDeque::new(),
        next_id: 1,
        broker_up: false,
        registered: false,
        focus: String::new(),
        shell_seen: false,
        broker_state: json!({}),
        camera_seen: BTreeSet::new(),
        location_last: std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0),
        names: BTreeMap::new(),
    }));

    type Handler = Box<dyn Fn(&A, String)>;
    let str_action = |name: &str, f: Handler| {
        let act = gio::SimpleAction::new(name, Some(glib::VariantTy::STRING));
        let a = agent.clone();
        act.connect_activate(move |_, p| f(&a, p.and_then(|v| v.get::<String>()).unwrap_or_default()));
        group.add_action(&act);
    };
    str_action("ack", Box::new(|a, id| {
        let id: u64 = id.trim().parse().unwrap_or(0);
        let notif = {
            let mut ag = a.borrow_mut();
            match ag.queue.iter_mut().find(|p| p.id == id) {
                Some(p) => {
                    p.acked = true;
                    std::mem::take(&mut p.notif)
                }
                None => 0,
            }
        };
        if notif != 0 {
            let s = a.borrow().session.clone();
            s.call(Some(NOTIFY), NOTIFY_PATH, NOTIFY, "CloseNotification", Some(&(notif,).to_variant()), None,
                   gio::DBusCallFlags::NONE, 3000, None::<&gio::Cancellable>, |_| {});
        }
    }));
    str_action("respond", Box::new(|a, s| {
        if let Some((id, ans)) = s.split_once(' ') {
            if let Ok(id) = id.parse() {
                Agent::finish(a, id, ans.trim());
            }
        }
    }));
    str_action("focus", Box::new(|a, app| {
        let app = if fp6_permissions::appid::valid_app_id(&app) { app } else { String::new() };
        let changed = a.borrow().focus != app || !a.borrow().shell_seen;
        a.borrow_mut().focus = app;
        a.borrow_mut().shell_seen = true;
        if changed && a.borrow().registered {
            Agent::send_focus(a);
        }
    }));
    str_action("set", Box::new(|a, s| {
        if let Some((app, perm, d)) = split3(&s) {
            Agent::admin(a, "SetDecision", (app, perm, d).to_variant());
        }
    }));
    str_action("net", Box::new(|a, s| {
        if let Some((app, class, on)) = split3(&s) {
            Agent::admin(a, "SetNetwork", (app, class, on == "1" || on == "true").to_variant());
        }
    }));
    str_action("reset", Box::new(|a, app| Agent::admin(a, "ResetApp", (app,).to_variant())));
    let refresh = gio::SimpleAction::new("refresh", None);
    let a = agent.clone();
    refresh.connect_activate(move |_, _| {
        a.borrow_mut().names.clear();
        Agent::refresh(&a)
    });
    group.add_action(&refresh);

    if let Err(e) = session.export_action_group(SESSION_PATH, &group) {
        log(format!("export: {e}"));
        std::process::exit(1);
    }
    let _owner = gio::bus_own_name_on_connection(&session, SESSION_NAME, gio::BusNameOwnerFlags::NONE, |_, _| {},
                                                 |_, n| log(format!("lost {n}")));

    // Prompt method on the system bus (called by fp6-permd only: system bus
    // policy lets root send this interface; the sender is checked too).
    let node = gio::DBusNodeInfo::for_xml(AGENT_XML).expect("agent xml");
    let iface = node.lookup_interface(AGENT_IFACE).expect("agent iface");
    let a = agent.clone();
    let reg = system
        .register_object(AGENT_PATH, &iface)
        .method_call(move |conn, sender, _path, _iface, method, params, inv| {
            if method != "Prompt" {
                inv.return_dbus_error("org.freedesktop.DBus.Error.UnknownMethod", "unknown method");
                return;
            }
            // Only the broker's connection may ask.
            let owner = conn
                .call_sync(Some("org.freedesktop.DBus"), "/org/freedesktop/DBus", "org.freedesktop.DBus", "GetNameOwner",
                           Some(&(BUS_NAME,).to_variant()), None, gio::DBusCallFlags::NONE, 2000, None::<&gio::Cancellable>)
                .ok()
                .and_then(|v| v.child_value(0).get::<String>());
            if owner.is_none() || owner.as_deref() != sender {
                inv.return_dbus_error("org.freedesktop.DBus.Error.AccessDenied", "not the permission broker");
                return;
            }
            let req: Value = params.child_value(0).get::<String>().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or(json!({}));
            let id = {
                let mut ag = a.borrow_mut();
                let id = ag.next_id;
                ag.next_id += 1;
                ag.queue.push_back(Pending { id, req, inv: Some(inv), shown: false, acked: false, notif: 0 });
                id
            };
            log(format!("prompt {id} queued"));
            Agent::show_next(&a);
        })
        .build();
    if let Err(e) = reg {
        log(format!("register_object: {e}"));
        std::process::exit(1);
    }

    // xdg-desktop-portal access dialogs (camera, location, ... portals),
    // from xdg-desktop-portal-lomiri's org.freedesktop.impl.portal.Access:
    // com.fairphone6.Permissions.Dialog.Ask(s json) -> s answer.
    let dnode = gio::DBusNodeInfo::for_xml(DIALOG_XML).expect("dialog xml");
    let diface = dnode.lookup_interface("com.fairphone6.Permissions.Dialog").expect("dialog iface");
    let a = agent.clone();
    let dreg = session
        .register_object(SESSION_PATH, &diface)
        .method_call(move |conn, sender, _path, _iface, method, params, inv| {
            if method != "Ask" || !sender_is_portal(&conn, sender) {
                inv.return_dbus_error("org.freedesktop.DBus.Error.AccessDenied", "only the Lomiri portal backend may ask");
                return;
            }
            let mut req: Value = params.child_value(0).get::<String>().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or(json!({}));
            req["options"] = json!(["allow", "deny"]);
            req["timeout"] = json!(60);
            let id = {
                let mut ag = a.borrow_mut();
                let id = ag.next_id;
                ag.next_id += 1;
                ag.queue.push_back(Pending { id, req, inv: Some(inv), shown: false, acked: false, notif: 0 });
                id
            };
            log(format!("portal dialog {id} queued"));
            Agent::show_next(&a);
        })
        .build();
    if let Err(e) = dreg {
        log(format!("register dialog: {e}"));
    }

    // Snap-decision answers.
    let a = agent.clone();
    session.signal_subscribe(Some(NOTIFY), Some(NOTIFY), None, Some(NOTIFY_PATH), None, gio::DBusSignalFlags::NONE,
                             move |_, _, _, _, signal, params| {
        let nid = params.child_value(0).get::<u32>().unwrap_or(0);
        let id = a.borrow().queue.iter().find(|p| p.notif == nid && nid != 0).map(|p| p.id);
        let Some(id) = id else { return };
        match signal {
            "ActionInvoked" => {
                let key = params.child_value(1).get::<String>().unwrap_or_default();
                Agent::finish(&a, id, &key);
            }
            "NotificationClosed" => {
                // Closed without an answer (swiped away, expired).
                let still = a.borrow().queue.iter().any(|p| p.id == id && !p.acked);
                if still {
                    Agent::finish(&a, id, "dismissed");
                }
            }
            _ => {}
        }
    });

    // Broker changes.
    let a = agent.clone();
    system.signal_subscribe(Some(BUS_NAME), Some(BUS_NAME), Some("Changed"), Some(PATH), None, gio::DBusSignalFlags::NONE,
                            move |_, _, _, _, _, params| {
        if params.child_value(0).get::<bool>().unwrap_or(false) {
            match std::process::Command::new("systemctl").args(["--user", "daemon-reload"]).status() {
                Ok(s) if s.success() => log("drop-ins changed: user manager reloaded"),
                Ok(s) => log(format!("systemctl --user daemon-reload: {s}")),
                Err(e) => log(format!("systemctl: {e}")),
            }
        }
        Agent::refresh(&a);
    });
    let a1 = agent.clone();
    let a2 = agent.clone();
    let _watch = gio::bus_watch_name_on_connection(&system, BUS_NAME, gio::BusNameWatcherFlags::NONE,
        move |_, _, _| {
            a1.borrow_mut().broker_up = true;
            Agent::register(&a1);
        },
        move |_, _| {
            let mut ag = a2.borrow_mut();
            ag.broker_up = false;
            ag.registered = false;
            ag.publish_state();
        });

    let a = agent.clone();
    glib::timeout_add_seconds_local(5, move || {
        Agent::poll_usage(&a);
        glib::ControlFlow::Continue
    });
    // Installed apps change rarely; refresh the Settings view now and then.
    let a = agent.clone();
    glib::timeout_add_seconds_local(120, move || {
        a.borrow_mut().names.clear();
        Agent::refresh(&a);
        glib::ControlFlow::Continue
    });
    agent.borrow_mut().publish_state();
    glib::MainLoop::new(None, false).run();
}
