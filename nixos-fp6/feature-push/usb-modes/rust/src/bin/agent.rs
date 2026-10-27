//! fp6-usbmodes-agent (user session):
//!  * reports the Lomiri lock screen state (com.lomiri.LomiriGreeter
//!    IsActive; unknown = locked) to fp6-usbmoded, which turns data
//!    functions on only while unlocked;
//!  * bridges GSettings com.fairphone6.usb-modes (written by System
//!    Settings > USB) to the helper: preferences, and `request` =
//!    "functions:ncm,mtp@nonce" | "power:source@nonce" | "ask-dismissed@nonce";
//!  * on a new connection: an interactive notification "Charging via USB ·
//!    Tap for options" (tap opens settings:///system/usb-modes), or in
//!    "ask" mode, once unlocked, a snap decision "Use USB for…";
//!  * indicator com.fairphone6.indicator.usbmodes (while connected), which
//!    is also the quick-settings "USB" tile.
//! The USB drive (mass storage) is stopped through org.fairphone6.UsbDrive1
//! as the user (that helper's own polkit rule), started from its app.

use fp6_usbmodes::{BUS_NAME, IFACE, PATH};
use gio::prelude::*;
use glib::{Variant, VariantDict};
use serde_json::{json, Value};
use std::cell::RefCell;
use std::rc::Rc;

const IND_NAME: &str = "com.fairphone6.indicator.usbmodes";
const IND_PATH: &str = "/com/fairphone6/indicator/usbmodes";
const SCHEMA: &str = "com.fairphone6.usb-modes";
const SETTINGS_URL: &str = "settings:///system/usb-modes";

fn log(m: impl AsRef<str>) {
    eprintln!("fp6-usbmodes-agent: {}", m.as_ref());
}

pub fn func_name(f: &str) -> &'static str {
    match f {
        "ncm" => "USB network",
        "mtp" => "File transfer",
        "adb" => "USB debugging",
        "mass_storage" => "USB drive",
        _ => "USB function",
    }
}

fn ids(st: &Value) -> Vec<String> {
    st["functions"].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default()
}

/// "Charging via USB", "Charging the connected device", ...
pub fn summary(st: &Value) -> String {
    let p = &st["power"];
    if p["role"] == "source" {
        "Charging the connected device".into()
    } else if st["peer"] == "host" {
        "Charging via USB".into()
    } else {
        "USB connected".into()
    }
}

pub fn body(st: &Value) -> String {
    let f = ids(st);
    let data = if f.is_empty() {
        "No data transfer".to_string()
    } else {
        f.iter().map(|x| func_name(x)).collect::<Vec<_>>().join(", ")
    };
    format!("{data} · Tap for options")
}

fn header(st: &Value) -> Variant {
    let d = VariantDict::new(None);
    let connected = st["cable"] == true && (st["peer"] == "host" || st["peer"] == "device");
    d.insert_value("title", &"USB".to_variant());
    let icon = gio::ThemedIcon::from_names(&["stock_usb", "phone-usb-symbolic", "drive-removable-symbolic"]);
    if let Some(v) = icon.serialize() {
        d.insert_value("icon", &v);
    }
    d.insert_value("label", &"".to_variant());
    d.insert_value("accessible-desc", &format!("{}. {}", summary(st), body(st)).to_variant());
    d.insert_value("visible", &connected.to_variant());
    d.end()
}

struct Agent {
    session: gio::DBusConnection,
    system: gio::DBusConnection,
    settings: Option<gio::Settings>,
    header: gio::SimpleAction,
    info: gio::Menu,
    shown: Vec<String>,
    last: Value,
    locked: bool,
    notified_session: bool,
    ask_id: u32,
    info_id: u32,
}

impl Agent {
    fn call_helper(&self, method: &str, args: Option<Variant>, cb: impl FnOnce(Result<Variant, glib::Error>) + 'static) {
        self.system.call(Some(BUS_NAME), PATH, IFACE, method, args.as_ref(), None, gio::DBusCallFlags::NONE, 60000,
                         None::<&gio::Cancellable>, cb);
    }

    fn notify(&self, summary: &str, body: &str, actions: &[(&str, &str)], hints: VariantDict, timeout: i32, replaces: u32) -> u32 {
        let a: Vec<String> = actions.iter().flat_map(|(k, l)| [k.to_string(), l.to_string()]).collect();
        let args = Variant::tuple_from_iter([
            "USB".to_variant(), replaces.to_variant(), "stock_usb".to_variant(), summary.to_variant(),
            body.to_variant(), a.to_variant(), hints.end(), timeout.to_variant(),
        ]);
        match self.session.call_sync(Some("org.freedesktop.Notifications"), "/org/freedesktop/Notifications",
                                     "org.freedesktop.Notifications", "Notify", Some(&args), None,
                                     gio::DBusCallFlags::NONE, 3000, None::<&gio::Cancellable>) {
            Ok(v) => v.child_value(0).get::<u32>().unwrap_or(0),
            Err(e) => {
                log(format!("Notify: {e}"));
                0
            }
        }
    }

    fn close(&self, id: u32) {
        if id != 0 {
            let _ = self.session.call_sync(Some("org.freedesktop.Notifications"), "/org/freedesktop/Notifications",
                                           "org.freedesktop.Notifications", "CloseNotification", Some(&(id,).to_variant()),
                                           None, gio::DBusCallFlags::NONE, 3000, None::<&gio::Cancellable>);
        }
    }

    fn notify_connected(&mut self, st: &Value) {
        if !self.settings.as_ref().map(|s| s.boolean("notify-on-connect")).unwrap_or(true) {
            return;
        }
        let h = VariantDict::new(None);
        // Lomiri "interactive" notification: tapping it invokes the action.
        h.insert_value("x-lomiri-switch-to-application", &true.to_variant());
        self.info_id = self.notify(&summary(st), &body(st), &[("options", "USB options")], h, 8000, self.info_id);
    }

    fn ask(&mut self) {
        if self.ask_id != 0 || self.locked {
            return;
        }
        let h = VariantDict::new(None);
        h.insert_value("x-lomiri-snap-decisions", &true.to_variant());
        h.insert_value("x-lomiri-snap-decisions-timeout", &60000i32.to_variant());
        h.insert_value("urgency", &1u8.to_variant());
        let mut acts = vec![("charge", "Charge only"), ("mtp", "File transfer")];
        if self.last["keep"].as_array().map(|a| !a.iter().any(|x| x == "ncm")).unwrap_or(true) {
            acts.push(("ncm", "USB network"));
        }
        acts.push(("options", "More options…"));
        self.ask_id = self.notify("Use USB for", "A computer is connected. Choose what it may use; the phone charges either way.",
                                  &acts, h, 60000, 0);
    }

    fn open_settings() {
        let _ = std::process::Command::new("lomiri-url-dispatcher").arg(SETTINGS_URL).spawn();
    }

    fn report_lock(&self) {
        let locked = self.locked;
        self.call_helper("SetSessionLocked", Some((locked,).to_variant()), move |r| {
            if let Err(e) = r {
                log(format!("SetSessionLocked({locked}): {e}"));
            }
        });
    }

    fn push_prefs(&self) {
        let Some(s) = &self.settings else { return };
        let mode = s.string("default-when-connected").to_string();
        let rem = s.boolean("remember-hosts");
        self.call_helper("SetPreferences", Some((mode, rem).to_variant()), |r| {
            if let Err(e) = r {
                log(format!("SetPreferences: {e}"));
            }
        });
    }

    fn set_functions(this: &Rc<RefCell<Agent>>, want: Vec<String>) {
        let a = this.borrow();
        let active_drive = ids(&a.last).iter().any(|x| x == "mass_storage");
        let want_drive = want.iter().any(|x| x == "mass_storage");
        if want_drive && !active_drive {
            // Needs an image: the USB Drive app starts it.
            let _ = std::process::Command::new("lomiri-url-dispatcher").arg("application:///fp6-usb-drive.desktop").spawn();
        }
        if active_drive && !want_drive {
            a.system.call(Some("org.fairphone6.UsbDrive"), "/org/fairphone6/UsbDrive", "org.fairphone6.UsbDrive1", "Stop",
                          None, None, gio::DBusCallFlags::NONE, 30000, None::<&gio::Cancellable>,
                          |r| if let Err(e) = r { log(format!("USB drive Stop: {e}")) });
        }
        let set: Vec<String> = want.into_iter().filter(|x| x != "mass_storage").collect();
        let t = this.clone();
        a.call_helper("SetFunctions", Some((set,).to_variant()), move |r| {
            if let Err(e) = r {
                let msg = e.message().rsplit(": ").next().unwrap_or("").to_string();
                log(format!("SetFunctions: {msg}"));
                let h = VariantDict::new(None);
                t.borrow().notify("Couldn't change the USB mode", &msg, &[], h, 6000, 0);
            }
        });
    }

    fn handle_request(this: &Rc<RefCell<Agent>>, req: &str) {
        let body = req.rsplit_once('@').map(|(b, _)| b).unwrap_or(req);
        if let Some(list) = body.strip_prefix("functions:") {
            Agent::set_functions(this, list.split(',').filter(|x| !x.is_empty()).map(String::from).collect());
        } else if let Some(role) = body.strip_prefix("power:") {
            let t = this.clone();
            this.borrow().call_helper("SetPowerRole", Some((role.to_string(),).to_variant()), move |r| {
                if let Err(e) = r {
                    let msg = e.message().rsplit(": ").next().unwrap_or("").to_string();
                    let h = VariantDict::new(None);
                    t.borrow().notify("Couldn't change who charges whom", &msg, &[], h, 6000, 0);
                }
            });
        } else if body == "ask-dismissed" {
            this.borrow().call_helper("DismissAsk", None, |_| {});
        } else if !body.is_empty() {
            log(format!("unknown request {body:?}"));
        }
    }

    fn update(this: &Rc<RefCell<Agent>>, st: Value) {
        let mut a = this.borrow_mut();
        let connected = |v: &Value| v["cable"] == true && (v["peer"] == "host" || v["peer"] == "device");
        let now = connected(&st);
        if !now {
            a.notified_session = false;
            let (i, k) = (a.info_id, a.ask_id);
            a.close(k);
            a.ask_id = 0;
            if !a.last.is_null() && connected(&a.last) {
                a.close(i);
                a.info_id = 0;
            }
        }
        a.last = st.clone();
        if now && !a.notified_session && st["asking"] != true {
            a.notified_session = true;
            a.notify_connected(&st);
        }
        if st["asking"] == true && now {
            a.notified_session = true;
            a.ask();
        } else if a.ask_id != 0 {
            let k = a.ask_id;
            a.close(k);
            a.ask_id = 0;
        }
        a.header.set_state(&header(&st));
        let mut l = vec![summary(&st)];
        let f = ids(&st);
        l.push(if f.is_empty() { "No data transfer".into() } else { f.iter().map(|x| func_name(x)).collect::<Vec<_>>().join(", ") });
        if st["lastError"].as_str().map(|s| !s.is_empty()).unwrap_or(false) {
            l.push(format!("Last change failed: {}", st["lastError"].as_str().unwrap_or("")));
        }
        if !now {
            l.clear();
        }
        if l != a.shown {
            a.info.remove_all();
            for x in &l {
                a.info.append_item(&gio::MenuItem::new(Some(x), None));
            }
            a.shown = l;
        }
    }

    fn refresh(this: &Rc<RefCell<Agent>>) {
        let t = this.clone();
        this.borrow().system.call(Some(BUS_NAME), PATH, IFACE, "GetStatus", None, None, gio::DBusCallFlags::NONE, 5000,
                                  None::<&gio::Cancellable>, move |r| {
            let st = r.ok().and_then(|v| v.child_value(0).get::<String>()).and_then(|s| serde_json::from_str(&s).ok())
                .unwrap_or_else(|| json!({}));
            Agent::update(&t, st);
        });
    }
}

fn greeter_active(session: &gio::DBusConnection) -> bool {
    let r = session.call_sync(Some("com.lomiri.LomiriGreeter"), "/com/lomiri/LomiriGreeter", "org.freedesktop.DBus.Properties",
                              "Get", Some(&("com.lomiri.LomiriGreeter", "IsActive").to_variant()), None,
                              gio::DBusCallFlags::NONE, 2000, None::<&gio::Cancellable>);
    match r.ok().and_then(|v| v.child_value(0).as_variant()).and_then(|v| v.get::<bool>()) {
        Some(b) => b,
        // No answer: treat as locked, never enable data blind.
        None => true,
    }
}

fn settings() -> Option<gio::Settings> {
    let src = gio::SettingsSchemaSource::default()?;
    let s = src.lookup(SCHEMA, true)?;
    Some(gio::Settings::new_full(&s, None::<&gio::SettingsBackend>, None))
}

fn bus(env: &str, t: gio::BusType) -> gio::DBusConnection {
    let r = match std::env::var(env) {
        Ok(a) => gio::DBusConnection::for_address_sync(&a,
            gio::DBusConnectionFlags::AUTHENTICATION_CLIENT | gio::DBusConnectionFlags::MESSAGE_BUS_CONNECTION,
            None::<&gio::DBusAuthObserver>, None::<&gio::Cancellable>),
        Err(_) => gio::bus_get_sync(t, None::<&gio::Cancellable>),
    };
    r.unwrap_or_else(|e| {
        log(format!("{env}: {e}"));
        std::process::exit(1);
    })
}

thread_local! {
    static KEEP: RefCell<Vec<Box<dyn std::any::Any>>> = const { RefCell::new(Vec::new()) };
}

fn main() {
    let session = bus("FP6_USBMODES_SESSION_BUS", gio::BusType::Session);
    // Tests: a private bus with the helper.
    let system = bus("FP6_USBMODES_SYSTEM_BUS", gio::BusType::System);
    let settings = settings();
    if settings.is_none() {
        log("GSettings schema com.fairphone6.usb-modes missing; settings page requests are ignored");
    }

    let actions = gio::SimpleActionGroup::new();
    let head = gio::SimpleAction::new_stateful("phone-header", None, &header(&json!({})));
    actions.add_action(&head);
    let open = gio::SimpleAction::new("options", None);
    open.connect_activate(|_, _| Agent::open_settings());
    actions.add_action(&open);
    let info = gio::Menu::new();
    let controls = gio::Menu::new();
    controls.append_item(&gio::MenuItem::new(Some("USB options…"), Some("indicator.options")));
    let sub = gio::Menu::new();
    sub.append_section(None, &info);
    sub.append_section(None, &controls);
    let root = gio::MenuItem::new(None, Some("indicator.phone-header"));
    root.set_attribute_value("x-ayatana-type", Some(&"org.ayatana.indicator.root".to_variant()));
    root.set_submenu(Some(&sub));
    let menu = gio::Menu::new();
    menu.append_item(&root);
    if let Err(e) = session.export_action_group(IND_PATH, &actions) {
        log(format!("export actions: {e}"));
    }
    for profile in ["phone", "phone_greeter", "desktop", "desktop_greeter"] {
        if let Err(e) = session.export_menu_model(&format!("{IND_PATH}/{profile}"), &menu) {
            log(format!("export menu: {e}"));
        }
    }
    let _owner = gio::bus_own_name_on_connection(&session, IND_NAME, gio::BusNameOwnerFlags::NONE,
                                                 |_, _| {}, |_, n| log(format!("lost bus name {n}")));

    let locked = greeter_active(&session);
    let agent = Rc::new(RefCell::new(Agent {
        session: session.clone(), system: system.clone(), settings: settings.clone(), header: head, info,
        shown: vec![], last: Value::Null, locked, notified_session: false, ask_id: 0, info_id: 0,
    }));
    {
        let a = agent.borrow();
        a.report_lock();
        a.push_prefs();
    }
    Agent::refresh(&agent);

    // Lock screen.
    {
        let a = agent.clone();
        #[allow(deprecated)]
        session.signal_subscribe(None, Some("org.freedesktop.DBus.Properties"), Some("PropertiesChanged"),
                                 Some("/com/lomiri/LomiriGreeter"), None, gio::DBusSignalFlags::NONE,
                                 move |c, _, _, _, _, _| {
            let l = greeter_active(c);
            let changed = a.borrow().locked != l;
            if changed {
                a.borrow_mut().locked = l;
                a.borrow().report_lock();
                if !l && a.borrow().last["asking"] == true {
                    a.borrow_mut().ask();
                }
            }
        });
    }
    // Belt and braces: the greeter property signal is not guaranteed.
    {
        let a = agent.clone();
        glib::timeout_add_seconds_local(5, move || {
            let l = greeter_active(&a.borrow().session);
            if a.borrow().locked != l {
                a.borrow_mut().locked = l;
                a.borrow().report_lock();
                if !l && a.borrow().last["asking"] == true {
                    a.borrow_mut().ask();
                }
            }
            glib::ControlFlow::Continue
        });
    }
    // Helper status.
    {
        let a = agent.clone();
        #[allow(deprecated)]
        system.signal_subscribe(Some(BUS_NAME), Some(IFACE), Some("StateChanged"), Some(PATH), None,
                                gio::DBusSignalFlags::NONE, move |_, _, _, _, _, params| {
            if let Some((s,)) = params.get::<(String,)>() {
                Agent::update(&a, serde_json::from_str(&s).unwrap_or_default());
            }
        });
    }
    {
        let a = agent.clone();
        let w = gio::bus_watch_name_on_connection(&system, BUS_NAME, gio::BusNameWatcherFlags::NONE,
            move |_, _, _| {
                {
                    let b = a.borrow();
                    b.report_lock();
                    b.push_prefs();
                }
                Agent::refresh(&a);
            },
            |_, _| {});
        KEEP.with(|k| k.borrow_mut().push(Box::new(w)));
    }
    // Notification actions.
    {
        let a = agent.clone();
        #[allow(deprecated)]
        session.signal_subscribe(Some("org.freedesktop.Notifications"), Some("org.freedesktop.Notifications"), None,
                                 Some("/org/freedesktop/Notifications"), None, gio::DBusSignalFlags::NONE,
                                 move |_, _, _, _, member, params| {
            let id = params.child_value(0).get::<u32>().unwrap_or(0);
            let (info_id, ask_id) = (a.borrow().info_id, a.borrow().ask_id);
            if id == 0 || (id != info_id && id != ask_id) {
                return;
            }
            if member == "NotificationClosed" {
                if id == ask_id {
                    a.borrow_mut().ask_id = 0;
                    a.borrow().call_helper("DismissAsk", None, |_| {});
                } else {
                    a.borrow_mut().info_id = 0;
                }
                return;
            }
            if member != "ActionInvoked" {
                return;
            }
            let key = params.child_value(1).get::<String>().unwrap_or_default();
            if id == ask_id {
                a.borrow_mut().ask_id = 0;
            }
            let keep: Vec<String> = a.borrow().last["keep"].as_array()
                .map(|v| v.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default();
            match key.as_str() {
                "options" => {
                    if id == ask_id {
                        a.borrow().call_helper("DismissAsk", None, |_| {});
                    }
                    Agent::open_settings();
                }
                "charge" => Agent::set_functions(&a, keep),
                "mtp" | "ncm" => {
                    let mut v = keep;
                    v.push(key);
                    Agent::set_functions(&a, v);
                }
                _ => {}
            }
        });
    }
    // Settings page.
    if let Some(s) = settings {
        let a = agent.clone();
        s.connect_changed(None, move |s, key| match key {
            "default-when-connected" | "remember-hosts" => a.borrow().push_prefs(),
            "request" => {
                let r = s.string("request").to_string();
                if !r.is_empty() {
                    Agent::handle_request(&a, &r);
                }
            }
            _ => {}
        });
        KEEP.with(|k| k.borrow_mut().push(Box::new(s)));
    }
    log(format!("ready ({})", if locked { "locked" } else { "unlocked" }));
    glib::MainLoop::new(None, false).run();
}
