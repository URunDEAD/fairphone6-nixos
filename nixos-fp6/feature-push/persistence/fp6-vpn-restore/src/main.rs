//! fp6-vpn-restore: reconnects VPNs after a restart (user session agent).
//!
//! NetworkManager does not autoconnect VPN profiles, so the network menu's
//! VPN switch (e.g. NetBird) was off after every reboot. This agent follows
//! NM's VPN active connections on the system bus, keeps the user's last
//! choice per VPN in GSettings (com.fairphone6.vpn last-active, i.e. dconf,
//! persistent) and activates what should be on once a network is up. Modes
//! per VPN (startup-modes, Settings > VPN > Reconnect at startup) and the
//! retry rules: policy.rs.
//!
//! It runs in the user's session because that is where the choice lives and
//! where NM's polkit rules allow the user to activate connections (the same
//! permission the network indicator uses); the Lomiri session starts by
//! itself at boot. It publishes the VPN list for Settings in `connections`.

mod policy;

use gio::prelude::*;
use glib::variant::ObjectPath;
use policy::{Action, Policy};
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;
use std::time::Instant;

const NM: &str = "org.freedesktop.NetworkManager";
const NM_PATH: &str = "/org/freedesktop/NetworkManager";
const NM_SETTINGS: &str = "/org/freedesktop/NetworkManager/Settings";
const NM_SETTINGS_IFACE: &str = "org.freedesktop.NetworkManager.Settings";
const ACTIVE_IFACE: &str = "org.freedesktop.NetworkManager.Connection.Active";
const PROPS: &str = "org.freedesktop.DBus.Properties";
/// NM_STATE_CONNECTED_SITE: some network; the VPN finds out the rest.
const NET_UP_STATE: u32 = 60;
const SCHEMA: &str = "com.fairphone6.vpn";

fn log(msg: impl AsRef<str>) {
    eprintln!("fp6-vpn-restore: {}", msg.as_ref());
}

struct App {
    bus: gio::DBusConnection,
    settings: gio::Settings,
    policy: Policy,
    start: Instant,
    /// uuid -> NM settings object path (for ActivateConnection).
    paths: BTreeMap<String, String>,
    /// active connection object path -> (uuid, is_vpn).
    active: BTreeMap<String, (String, bool)>,
    timer: Option<glib::SourceId>,
}

type Shared = Rc<RefCell<App>>;

/// Value of `key` in an a{sv} or a{sa{sv}} dictionary ('v' unwrapped).
fn lookup(dict: &glib::Variant, key: &str) -> Option<glib::Variant> {
    (0..dict.n_children()).map(|i| dict.child_value(i)).find(|e| e.child_value(0).str() == Some(key)).map(|e| {
        let v = e.child_value(1);
        v.as_variant().unwrap_or(v)
    })
}

impl App {
    fn now(&self) -> u64 {
        self.start.elapsed().as_secs()
    }

    fn call(&self, path: &str, iface: &str, method: &str, args: Option<&glib::Variant>) -> Result<glib::Variant, glib::Error> {
        self.bus.call_sync(Some(NM), path, iface, method, args, None, gio::DBusCallFlags::NONE, 10_000,
                           gio::Cancellable::NONE)
    }

    fn get_prop(&self, path: &str, iface: &str, prop: &str) -> Option<glib::Variant> {
        let r = self.call(path, PROPS, "Get", Some(&(iface, prop).to_variant())).ok()?;
        r.child_value(0).as_variant()
    }

    /// NM's VPN profiles (uuid, id), and their settings paths.
    fn list_vpns(&mut self) -> Vec<(String, String)> {
        let mut out = Vec::new();
        self.paths.clear();
        let Ok(r) = self.call(NM_SETTINGS, NM_SETTINGS_IFACE, "ListConnections", None) else {
            return out;
        };
        let arr = r.child_value(0);
        for i in 0..arr.n_children() {
            let Some(path) = arr.child_value(i).str().map(str::to_string) else { continue };
            let Ok(s) = self.call(&path, "org.freedesktop.NetworkManager.Settings.Connection", "GetSettings", None) else {
                continue;
            };
            let Some(con) = lookup(&s.child_value(0), "connection") else { continue };
            let field = |k: &str| lookup(&con, k).and_then(|v| v.str().map(str::to_string)).unwrap_or_default();
            if field("type") != "vpn" {
                continue;
            }
            let (uuid, id) = (field("uuid"), field("id"));
            if uuid.is_empty() {
                continue;
            }
            self.paths.insert(uuid.clone(), path);
            out.push((uuid, id));
        }
        out
    }

    /// (uuid, is_vpn, state) of an active connection object.
    fn active_info(&self, path: &str) -> Option<(String, bool, u32)> {
        let r = self.call(path, PROPS, "GetAll", Some(&(ACTIVE_IFACE,).to_variant())).ok()?;
        let d = r.child_value(0);
        let uuid = lookup(&d, "Uuid")?.str()?.to_string();
        let vpn = lookup(&d, "Vpn").and_then(|v| v.get::<bool>()).unwrap_or(false)
            || lookup(&d, "Type").and_then(|v| v.str().map(|t| t == "vpn")).unwrap_or(false);
        let state = lookup(&d, "State").and_then(|v| v.get::<u32>()).unwrap_or(0);
        Some((uuid, vpn, state))
    }

    /// Full resync with NM (start, NM restart, profile list changes).
    fn resync(&mut self) {
        let now = self.now();
        let list = self.list_vpns();
        self.policy.set_connections(&list, now);
        self.active.clear();
        if let Some(v) = self.get_prop(NM_PATH, NM, "ActiveConnections") {
            for i in 0..v.n_children() {
                let Some(p) = v.child_value(i).str().map(str::to_string) else { continue };
                if let Some((uuid, vpn, state)) = self.active_info(&p) {
                    if vpn {
                        self.policy.vpn_state(&uuid, state, 0, now);
                    }
                    self.active.insert(p, (uuid, vpn));
                }
            }
        }
        let state = self.get_prop(NM_PATH, NM, "State").and_then(|v| v.get::<u32>()).unwrap_or(0);
        self.policy.set_network(state >= NET_UP_STATE, now);
    }

    fn reload_modes(&mut self) {
        let lines: Vec<String> = self.settings.strv("startup-modes").iter().map(|s| s.to_string()).collect();
        let now = self.now();
        self.policy.set_modes(policy::parse_modes(&lines), now);
    }

    fn set_strv_if_changed(&self, key: &str, v: &[String]) {
        let cur: Vec<String> = self.settings.strv(key).iter().map(|s| s.to_string()).collect();
        if cur != v {
            let refs: Vec<&str> = v.iter().map(String::as_str).collect();
            if let Err(e) = self.settings.set_strv(key, refs.as_slice()) {
                log(format!("cannot write {key}: {e}"));
            }
        }
    }

    fn publish(&mut self) {
        if self.policy.wanted_dirty {
            self.policy.wanted_dirty = false;
            let w = self.policy.wanted();
            log(format!("{} VPN(s) to restore at the next start", w.len()));
            self.set_strv_if_changed("last-active", &w);
        }
        let rows = self.policy.rows();
        self.set_strv_if_changed("connections", &rows);
    }
}

/// Runs due actions, persists, re-arms the timer. Call after every event.
fn step(app: &Shared) {
    let actions = {
        let mut a = app.borrow_mut();
        let now = a.now();
        a.policy.tick(now)
    };
    for Action::Activate(uuid) in actions {
        activate(app, &uuid);
    }
    let mut a = app.borrow_mut();
    a.publish();
    if let Some(t) = a.timer.take() {
        t.remove();
    }
    let now = a.now();
    if let Some(secs) = a.policy.next_wake(now) {
        let app2 = app.clone();
        a.timer = Some(glib::timeout_add_seconds_local_once(secs.clamp(1, 600) as u32, move || {
            app2.borrow_mut().timer = None;
            step(&app2);
        }));
    }
}

fn activate(app: &Shared, uuid: &str) {
    let a = app.borrow();
    let Some(path) = a.paths.get(uuid).cloned() else { return };
    let name = a.policy.conns.get(uuid).map(|c| c.name.clone()).unwrap_or_default();
    log(format!("connecting VPN \"{name}\""));
    let (Ok(conn), Ok(root)) = (ObjectPath::try_from(path), ObjectPath::try_from("/".to_string())) else { return };
    let args = glib::Variant::tuple_from_iter([conn.to_variant(), root.to_variant(), root.to_variant()]);
    let app2 = app.clone();
    let uuid = uuid.to_string();
    a.bus.call(Some(NM), NM_PATH, NM, "ActivateConnection", Some(&args), None, gio::DBusCallFlags::NONE, 30_000,
               gio::Cancellable::NONE, move |r| {
        if let Err(e) = r {
            log(format!("activating {uuid} failed: {e}"));
            {
                let mut a = app2.borrow_mut();
                let now = a.now();
                a.policy.failed(&uuid, now);
            }
            step(&app2);
        }
    });
}

fn main() {
    let bus = match gio::bus_get_sync(gio::BusType::System, gio::Cancellable::NONE) {
        Ok(b) => b,
        Err(e) => {
            log(format!("no system bus: {e}"));
            std::process::exit(1);
        }
    };
    let settings = gio::Settings::new(SCHEMA);
    let wanted: Vec<String> = settings.strv("last-active").iter().map(|s| s.to_string()).collect();
    let modes = policy::parse_modes(&settings.strv("startup-modes").iter().map(|s| s.to_string()).collect::<Vec<_>>());
    let app: Shared = Rc::new(RefCell::new(App {
        bus: bus.clone(), settings: settings.clone(), policy: Policy::new(&wanted, modes), start: Instant::now(),
        paths: BTreeMap::new(), active: BTreeMap::new(), timer: None,
    }));
    log(format!("started; last on: {}", wanted.len()));

    // NM state (network up/down).
    let a = app.clone();
    #[allow(deprecated)]
    bus.signal_subscribe(Some(NM), Some(NM), Some("StateChanged"), Some(NM_PATH), None, gio::DBusSignalFlags::NONE,
        move |_, _, _, _, _, params| {
            if let Some((state,)) = params.get::<(u32,)>() {
                {
                    let mut x = a.borrow_mut();
                    let now = x.now();
                    x.policy.set_network(state >= NET_UP_STATE, now);
                }
                step(&a);
            }
        });

    // Active connection state changes (any path).
    let a = app.clone();
    #[allow(deprecated)]
    bus.signal_subscribe(Some(NM), Some(ACTIVE_IFACE), Some("StateChanged"), None, None, gio::DBusSignalFlags::NONE,
        move |_, _, path, _, _, params| {
            let Some((state, reason)) = params.get::<(u32, u32)>() else { return };
            let known = a.borrow().active.get(path).cloned();
            let info = match known {
                Some(k) => Some(k),
                None => {
                    let i = a.borrow().active_info(path).map(|(u, v, _)| (u, v));
                    if let Some(i) = &i {
                        a.borrow_mut().active.insert(path.to_string(), i.clone());
                    }
                    i
                }
            };
            let Some((uuid, vpn)) = info else { return };
            if state == policy::ST_DEACTIVATED {
                a.borrow_mut().active.remove(path);
            }
            if !vpn {
                return;
            }
            log(format!("VPN {uuid}: state {state} reason {reason}"));
            {
                let mut x = a.borrow_mut();
                let now = x.now();
                x.policy.vpn_state(&uuid, state, reason, now);
            }
            step(&a);
        });

    // Profiles added, removed or renamed.
    let resync = {
        let a = app.clone();
        move || {
            a.borrow_mut().resync();
            step(&a);
        }
    };
    for member in ["NewConnection", "ConnectionRemoved"] {
        let r = resync.clone();
        #[allow(deprecated)]
        bus.signal_subscribe(Some(NM), Some(NM_SETTINGS_IFACE), Some(member), Some(NM_SETTINGS), None,
                             gio::DBusSignalFlags::NONE, move |_, _, _, _, _, _| r());
    }
    let r = resync.clone();
    #[allow(deprecated)]
    bus.signal_subscribe(Some(NM), Some("org.freedesktop.NetworkManager.Settings.Connection"), Some("Updated"), None,
                         None, gio::DBusSignalFlags::NONE, move |_, _, _, _, _, _| r());

    // NM (re)appears: resync (also the initial sync).
    let r = resync.clone();
    let watch = gio::bus_watch_name_on_connection(&bus, NM, gio::BusNameWatcherFlags::NONE,
                                                  move |_, _, _| r(), |_, _| log("NetworkManager gone"));
    std::mem::forget(watch);

    let a = app.clone();
    settings.connect_changed(Some("startup-modes"), move |_, _| {
        a.borrow_mut().reload_modes();
        step(&a);
    });

    glib::MainLoop::new(None, false).run();
}
