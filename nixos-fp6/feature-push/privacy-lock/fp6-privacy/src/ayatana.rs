// SPDX-License-Identifier: GPL-3.0-only
//! Small Ayatana/Lomiri indicator server (org.gtk.Menus + org.gtk.Actions)
//! with one root item and a flat, replaceable submenu.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use zbus::interface;
use zbus::zvariant::{OwnedValue, Signature, Value};

pub type Item = HashMap<String, OwnedValue>;
type ActionDescription = (bool, Signature, Vec<OwnedValue>);

pub fn owned<'a, T: Into<Value<'a>>>(v: T) -> OwnedValue {
    OwnedValue::try_from(v.into()).expect("plain values have no fds")
}

/// Serialized GIcon ("themed", [names]) as g_icon_serialize() makes it.
pub fn themed_icon(names: &[&str]) -> Value<'static> {
    Value::new(("themed", Value::new(names.iter().map(|s| s.to_string()).collect::<Vec<_>>())))
}

/// Menu item: label + action ("indicator.<name>", "" = none).
pub fn item(label: &str, action: &str) -> Item {
    let mut i: Item = HashMap::new();
    i.insert("label".into(), owned(label));
    if !action.is_empty() {
        i.insert("action".into(), owned(format!("indicator.{action}")));
    }
    i
}

#[derive(Default)]
pub struct Model {
    pub root_state: HashMap<String, OwnedValue>,
    pub items: Vec<Item>,
    /// Plain (stateless) actions and whether they are enabled.
    pub actions: Vec<(String, bool)>,
}

type Callback = dyn Fn(&str) + Send + Sync;

pub struct Indicator {
    pub model: Arc<Mutex<Model>>,
    conn: zbus::blocking::Connection,
    actions_path: String,
    menu_paths: Vec<String>,
}

struct Actions {
    model: Arc<Mutex<Model>>,
    on_activate: Arc<Callback>,
}

fn describe(m: &Model, name: &str) -> Option<ActionDescription> {
    if name == "root" {
        let state: HashMap<String, Value> =
            m.root_state.iter().map(|(k, v)| (k.clone(), Value::from(v.try_clone().unwrap()))).collect();
        return Some((true, Signature::Unit, vec![owned(state)]));
    }
    m.actions.iter().find(|(n, _)| n == name).map(|(_, en)| (*en, Signature::Unit, Vec::new()))
}

#[interface(name = "org.gtk.Actions")]
impl Actions {
    fn list(&self) -> Vec<String> {
        let m = self.model.lock().unwrap();
        std::iter::once("root".to_string()).chain(m.actions.iter().map(|(n, _)| n.clone())).collect()
    }

    fn describe(&self, name: &str) -> zbus::fdo::Result<ActionDescription> {
        describe(&self.model.lock().unwrap(), name)
            .ok_or_else(|| zbus::fdo::Error::InvalidArgs(format!("no action {name}")))
    }

    fn describe_all(&self) -> HashMap<String, ActionDescription> {
        let m = self.model.lock().unwrap();
        std::iter::once("root".to_string())
            .chain(m.actions.iter().map(|(n, _)| n.clone()))
            .filter_map(|n| describe(&m, &n).map(|d| (n, d)))
            .collect()
    }

    fn activate(&self, name: &str, _parameter: Vec<OwnedValue>, _platform_data: HashMap<String, OwnedValue>) {
        (self.on_activate)(name);
    }

    fn set_state(&self, _name: &str, _value: OwnedValue, _platform_data: HashMap<String, OwnedValue>) {}
}

struct Menus {
    model: Arc<Mutex<Model>>,
}

fn root_item() -> Item {
    let mut root: Item = HashMap::new();
    root.insert("action".into(), owned("indicator.root"));
    root.insert("x-ayatana-type".into(), owned("org.ayatana.indicator.root"));
    root.insert(":submenu".into(), owned((0u32, 1u32)));
    root
}

fn clone_items(items: &[Item]) -> Vec<Item> {
    items
        .iter()
        .map(|i| i.iter().map(|(k, v)| (k.clone(), v.try_clone().unwrap())).collect())
        .collect()
}

#[interface(name = "org.gtk.Menus")]
impl Menus {
    fn start(&self, groups: Vec<u32>) -> Vec<(u32, u32, Vec<Item>)> {
        if !groups.contains(&0) {
            return Vec::new();
        }
        let m = self.model.lock().unwrap();
        vec![(0, 0, vec![root_item()]), (0, 1, clone_items(&m.items))]
    }

    fn end(&self, _groups: Vec<u32>) {}
}

impl Indicator {
    pub fn serve(
        bus_name: &str,
        actions_path: &str,
        menu_paths: &[&str],
        model: Model,
        on_activate: Arc<Callback>,
    ) -> zbus::Result<Self> {
        let model = Arc::new(Mutex::new(model));
        let mut builder = zbus::blocking::connection::Builder::session()?
            .serve_at(actions_path, Actions { model: model.clone(), on_activate })?;
        for p in menu_paths {
            builder = builder.serve_at(*p, Menus { model: model.clone() })?;
        }
        let conn = builder.name(bus_name)?.build()?;
        Ok(Self {
            model,
            conn,
            actions_path: actions_path.into(),
            menu_paths: menu_paths.iter().map(|s| s.to_string()).collect(),
        })
    }

    /// Replace root state and submenu; emits Changed signals for what differs.
    pub fn update(&self, root_state: HashMap<String, OwnedValue>, items: Vec<Item>, actions: Vec<(String, bool)>) {
        let (state_changed, old_len, items_changed, actions_changed) = {
            let mut m = self.model.lock().unwrap();
            let state_changed = m.root_state != root_state;
            let items_changed = m.items != items;
            let actions_changed = m.actions != actions;
            let old_len = m.items.len() as u32;
            m.root_state = root_state;
            m.items = items;
            m.actions = actions;
            (state_changed, old_len, items_changed, actions_changed)
        };
        if actions_changed {
            // Re-announce every plain action (removal + addition).
            let m = self.model.lock().unwrap();
            let names: Vec<String> = m.actions.iter().map(|(n, _)| n.clone()).collect();
            let adds: HashMap<String, ActionDescription> =
                m.actions.iter().filter_map(|(n, _)| describe(&m, n).map(|d| (n.clone(), d))).collect();
            drop(m);
            let body = (names, HashMap::<String, bool>::new(), HashMap::<String, OwnedValue>::new(), adds);
            let _ = self.conn.emit_signal(None::<()>, self.actions_path.as_str(), "org.gtk.Actions", "Changed", &body);
        }
        if state_changed {
            let m = self.model.lock().unwrap();
            let Some((_, _, state)) = describe(&m, "root") else { return };
            drop(m);
            let mut changes: HashMap<String, OwnedValue> = HashMap::new();
            changes.insert("root".into(), state.into_iter().next().unwrap());
            let body = (
                Vec::<String>::new(),
                HashMap::<String, bool>::new(),
                changes,
                HashMap::<String, ActionDescription>::new(),
            );
            if let Err(e) = self.conn.emit_signal(None::<()>, self.actions_path.as_str(), "org.gtk.Actions", "Changed", &body) {
                crate::log(format!("Actions.Changed: {e}"));
            }
        }
        if items_changed {
            let items = clone_items(&self.model.lock().unwrap().items);
            let body = (vec![(0u32, 1u32, 0u32, old_len, items)],);
            for p in &self.menu_paths {
                if let Err(e) = self.conn.emit_signal(None::<()>, p.as_str(), "org.gtk.Menus", "Changed", &body) {
                    crate::log(format!("Menus.Changed: {e}"));
                }
            }
        }
    }

    pub fn connection(&self) -> &zbus::blocking::Connection {
        &self.conn
    }
}
