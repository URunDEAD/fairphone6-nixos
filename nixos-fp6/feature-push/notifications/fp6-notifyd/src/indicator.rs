// SPDX-License-Identifier: GPL-3.0-only
//! Status-bar menu (Ayatana indicator, org.gtk.Menus + org.gtk.Actions):
//! "Do Not Disturb" and "Bedtime mode" switches and a link to the Settings
//! page. The moon icon shows in the collapsed status bar while DND is on.
//! Also the com.fairphone6.Dnd service the telephony daemons ask.

use std::collections::HashMap;
use std::sync::Arc;

use zbus::interface;
use zbus::zvariant::{OwnedValue, Signature, Value};

use crate::Shared;

pub const BUS_NAME: &str = "com.lomiri.indicator.dnd";
pub const ACTIONS_PATH: &str = "/com/lomiri/indicator/dnd";
pub const MENU_PHONE: &str = "/com/lomiri/indicator/dnd/phone";
pub const MENU_DESKTOP: &str = "/com/lomiri/indicator/dnd/desktop";
pub const DND_NAME: &str = "com.fairphone6.Dnd";
pub const DND_PATH: &str = "/com/fairphone6/Dnd";
pub const SETTINGS_URL: &str = "settings:///system/do-not-disturb";

type Item = HashMap<String, OwnedValue>;
type ActionDescription = (bool, Signature, Vec<OwnedValue>);

fn owned<'a, T: Into<Value<'a>>>(v: T) -> OwnedValue {
    OwnedValue::try_from(v.into()).expect("plain values have no fds")
}

const ACTIONS: [&str; 4] = ["root", "dnd", "bedtime", "settings"];

pub fn reason_text(reason: &str) -> String {
    match reason {
        "manual" => "Do Not Disturb is on".into(),
        "bedtime" => "Do Not Disturb is on (bedtime)".into(),
        r if r.starts_with("schedule:") => format!("Do Not Disturb is on ({})", &r[9..]),
        _ => "Do Not Disturb is off".into(),
    }
}

fn root_state(s: &Shared) -> OwnedValue {
    let eff = s.eff.lock().unwrap().clone();
    let icons: Vec<&str> = if eff.bedtime {
        vec!["weather-clear-night-symbolic", "night-mode", "notification"]
    } else {
        vec!["night-mode", "weather-clear-night-symbolic", "notification"]
    };
    let mut state: HashMap<&str, Value> = HashMap::new();
    state.insert("title", Value::new("Do Not Disturb"));
    state.insert("icon", Value::new(("themed", Value::new(icons))));
    state.insert("accessible-desc", Value::new(reason_text(&eff.reason)));
    // Collapsed status bar: only while DND (or bedtime) is on.
    state.insert("visible", Value::new(eff.dnd || eff.bedtime));
    owned(state)
}

fn describe(s: &Shared, name: &str) -> Option<ActionDescription> {
    let unit = Signature::Unit;
    let eff = s.eff.lock().unwrap().clone();
    match name {
        "root" => Some((true, unit, vec![root_state(s)])),
        "dnd" => Some((true, unit, vec![owned(eff.dnd)])),
        "bedtime" => Some((true, unit, vec![owned(eff.bedtime)])),
        "settings" => Some((true, unit, vec![])),
        _ => None,
    }
}

/// Requests handled on the GLib main thread (GSettings writes).
pub enum Request {
    Dnd(bool),
    Bedtime(bool),
}

pub struct Actions(pub Arc<Shared>);

impl Actions {
    fn request(&self, r: Request) {
        glib::MainContext::default().invoke(move || crate::handle_request(r));
    }
}

#[interface(name = "org.gtk.Actions")]
impl Actions {
    fn list(&self) -> Vec<String> {
        ACTIONS.iter().map(|s| s.to_string()).collect()
    }

    fn describe(&self, name: &str) -> zbus::fdo::Result<ActionDescription> {
        describe(&self.0, name).ok_or_else(|| zbus::fdo::Error::InvalidArgs(format!("no action {name}")))
    }

    fn describe_all(&self) -> HashMap<String, ActionDescription> {
        ACTIONS.iter().filter_map(|n| describe(&self.0, n).map(|d| (n.to_string(), d))).collect()
    }

    async fn activate(&self, name: &str, _parameter: Vec<OwnedValue>, _platform_data: HashMap<String, OwnedValue>) {
        let eff = self.0.eff.lock().unwrap().clone();
        match name {
            "dnd" => self.request(Request::Dnd(!eff.dnd)),
            "bedtime" => self.request(Request::Bedtime(!eff.bedtime)),
            "settings" => {
                if let Some(conn) = self.0.conn.get() {
                    let _ = conn
                        .inner()
                        .call_method(
                            Some("com.lomiri.URLDispatcher"),
                            "/com/lomiri/URLDispatcher",
                            Some("com.lomiri.URLDispatcher"),
                            "DispatchURL",
                            &(SETTINGS_URL, ""),
                        )
                        .await;
                }
            }
            _ => {}
        }
    }

    async fn set_state(&self, name: &str, value: OwnedValue, _platform_data: HashMap<String, OwnedValue>) {
        let Ok(on) = bool::try_from(&value) else { return };
        match name {
            "dnd" => self.request(Request::Dnd(on)),
            "bedtime" => self.request(Request::Bedtime(on)),
            _ => {}
        }
    }
}

pub struct Menus;

fn item(pairs: &[(&str, OwnedValue)]) -> Item {
    pairs.iter().map(|(k, v)| (k.to_string(), v.try_clone().unwrap())).collect()
}

#[interface(name = "org.gtk.Menus")]
impl Menus {
    fn start(&self, groups: Vec<u32>) -> Vec<(u32, u32, Vec<Item>)> {
        if !groups.contains(&0) {
            return Vec::new();
        }
        let root = item(&[
            ("action", owned("indicator.root")),
            ("x-ayatana-type", owned("org.ayatana.indicator.root")),
            (":submenu", owned((0u32, 1u32))),
        ]);
        let dnd = item(&[
            ("label", owned("Do Not Disturb")),
            ("action", owned("indicator.dnd")),
            ("x-ayatana-type", owned("org.ayatana.indicator.switch")),
        ]);
        let bedtime = item(&[
            ("label", owned("Bedtime mode")),
            ("action", owned("indicator.bedtime")),
            ("x-ayatana-type", owned("org.ayatana.indicator.switch")),
        ]);
        let settings = item(&[
            ("label", owned("Do Not Disturb settings…")),
            ("action", owned("indicator.settings")),
        ]);
        vec![(0, 0, vec![root]), (0, 1, vec![dnd, bedtime, settings])]
    }

    fn end(&self, _groups: Vec<u32>) {}
}

/// com.fairphone6.Dnd: asked by the patched telephony approver / indicator.
pub struct Dnd(pub Arc<Shared>);

#[interface(name = "com.fairphone6.Dnd")]
impl Dnd {
    /// kind: "call" or "message"; id: phone number / sender id.
    /// true = ring/vibrate/wake as usual.
    fn should_alert(&self, kind: &str, id: &str, is_contact: bool, is_favourite: bool) -> bool {
        let cfg = self.0.cfg.lock().unwrap().clone();
        let eff = self.0.eff.lock().unwrap().clone();
        let now = crate::now();
        let ok = crate::policy::should_alert(&cfg, &eff, &mut self.0.calls.lock().unwrap(), kind, id, is_contact, is_favourite, now);
        if eff.dnd {
            // No numbers in the journal.
            eprintln!("fp6-notifyd: DND {kind}: {}", if ok { "allowed" } else { "silenced" });
        }
        ok
    }

    fn set_active(&self, on: bool) {
        glib::MainContext::default().invoke(move || crate::handle_request(Request::Dnd(on)));
    }

    fn set_bedtime(&self, on: bool) {
        glib::MainContext::default().invoke(move || crate::handle_request(Request::Bedtime(on)));
    }

    #[zbus(property)]
    fn active(&self) -> bool {
        self.0.eff.lock().unwrap().dnd
    }

    #[zbus(property)]
    fn reason(&self) -> String {
        self.0.eff.lock().unwrap().reason.clone()
    }

    #[zbus(property)]
    fn bedtime(&self) -> bool {
        self.0.eff.lock().unwrap().bedtime
    }
}

/// Tells the menus (and com.fairphone6.Dnd listeners) about a new state.
pub fn emit(conn: &zbus::blocking::Connection, s: &Shared) {
    let eff = s.eff.lock().unwrap().clone();
    let mut changes: HashMap<String, OwnedValue> = HashMap::new();
    changes.insert("dnd".into(), owned(eff.dnd));
    changes.insert("bedtime".into(), owned(eff.bedtime));
    changes.insert("root".into(), root_state(s));
    let body = (Vec::<String>::new(), HashMap::<String, bool>::new(), changes, HashMap::<String, ActionDescription>::new());
    if let Err(e) = conn.emit_signal(None::<()>, ACTIONS_PATH, "org.gtk.Actions", "Changed", &body) {
        eprintln!("fp6-notifyd: Changed: {e}");
    }
    let _ = conn.emit_signal(None::<()>, DND_PATH, "com.fairphone6.Dnd", "Changed", &(eff.dnd, eff.reason.as_str(), eff.bedtime));
}
