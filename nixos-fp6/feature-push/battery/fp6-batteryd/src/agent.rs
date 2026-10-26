//! fp6-batteryd agent (user service of the Lomiri session).
//!
//!  * GSettings com.fairphone6.battery -> SetConfig on the system daemon
//!    (at start, on every change, and when the daemon reappears).
//!  * Next alarm: lomiri-indicator-datetime's phone menu (org.gtk.Menus on
//!    org.ayatana.indicator.datetime /org/ayatana/indicator/datetime/phone):
//!    items with x-ayatana-type "org.ayatana.indicator.alarm" carry
//!    x-ayatana-time (unix s). The indicator reads the Clock app's alarms
//!    (mkcal) and resolves recurrences; it lists the next 5 events of the
//!    coming month. Polled every 5 min -> SetNextAlarm.
//!  * Battery saver (SaverChanged): per-user extras off (boolean GSettings
//!    keys in saver-extras, default the shake gesture's accelerometer
//!    polling), restored afterwards unless the user changed them meanwhile;
//!    saver-active mirrors the state for other session components.
//!  * Indicator com.fairphone6.indicator.battery: visible while battery
//!    saver is on (icon) or, if charging-indicator is on, while charging
//!    (label "7.5 W"); the menu (also on the lock screen) shows charging
//!    speed, charger, time to full or the adaptive hold, a Battery saver
//!    switch and Battery settings.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use gio::prelude::*;
use glib::{Variant, VariantDict};

use crate::daemon::{BUS_NAME, IFACE, PATH};

pub const SCHEMA: &str = "com.fairphone6.battery";
const IND_NAME: &str = "com.fairphone6.indicator.battery";
const IND_PATH: &str = "/com/fairphone6/indicator/battery";

fn log(msg: impl AsRef<str>) {
    eprintln!("{}", msg.as_ref());
}

// ------------------------------------------------------------ JSON reading
/// Value text after `"key":` (first occurrence), for flat lookups in status.json.
fn raw<'a>(text: &'a str, key: &str) -> Option<&'a str> {
    let pat = format!("\"{key}\":");
    let i = text.find(&pat)? + pat.len();
    Some(&text[i..])
}
pub fn jnum(text: &str, key: &str) -> Option<i64> {
    let r = raw(text, key)?;
    let end = r.find(|c: char| !(c.is_ascii_digit() || c == '-')).unwrap_or(r.len());
    r[..end].parse().ok()
}
pub fn jbool(text: &str, key: &str) -> Option<bool> {
    let r = raw(text, key)?;
    if r.starts_with("true") {
        Some(true)
    } else if r.starts_with("false") {
        Some(false)
    } else {
        None
    }
}
pub fn jstr(text: &str, key: &str) -> Option<String> {
    let r = raw(text, key)?.strip_prefix('"')?;
    let mut out = String::new();
    let mut esc = false;
    for c in r.chars() {
        match (esc, c) {
            (true, 'n') => { out.push('\n'); esc = false; }
            (true, c) => { out.push(c); esc = false; }
            (false, '\\') => esc = true,
            (false, '"') => return Some(out),
            (false, c) => out.push(c),
        }
    }
    None
}

// --------------------------------------------------------------- alarms
/// Next alarm from an org.gtk.Menus.Start reply `(a(uuaa{sv}))`.
pub fn next_alarm_from_menus(reply: &Variant, now: i64) -> Option<i64> {
    let groups = reply.child_value(0);
    let mut best: Option<i64> = None;
    for g in groups.iter() {
        let items = g.child_value(2);
        for item in items.iter() {
            let dict = VariantDict::new(Some(&item));
            let ty: Option<String> = dict.lookup("x-ayatana-type").ok().flatten();
            if ty.as_deref() != Some("org.ayatana.indicator.alarm") {
                continue;
            }
            if let Some(t) = dict.lookup::<i64>("x-ayatana-time").ok().flatten() {
                if t > now && best.is_none_or(|b| t < b) {
                    best = Some(t);
                }
            }
        }
    }
    best
}

pub fn query_next_alarm(session: &gio::DBusConnection, now: i64) -> Result<Option<i64>, String> {
    let groups: Vec<u32> = (0..12).collect();
    let args = (groups.clone(),).to_variant();
    let path = "/org/ayatana/indicator/datetime/phone";
    let dest = "org.ayatana.indicator.datetime";
    let reply = session
        .call_sync(Some(dest), path, "org.gtk.Menus", "Start", Some(&args), None, gio::DBusCallFlags::NONE, 5000,
                   None::<&gio::Cancellable>)
        .map_err(|e| e.to_string())?;
    let _ = session.call_sync(Some(dest), path, "org.gtk.Menus", "End", Some(&args), None, gio::DBusCallFlags::NONE,
                              5000, None::<&gio::Cancellable>);
    Ok(next_alarm_from_menus(&reply, now))
}

// --------------------------------------------------------------- extras
fn state_file() -> PathBuf {
    let base = std::env::var_os("XDG_STATE_HOME").map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/state"));
    base.join("fp6-battery/saver-extras")
}

fn lookup_bool_key(schema: &str, key: &str) -> Option<gio::Settings> {
    let src = gio::SettingsSchemaSource::default()?;
    let s = src.lookup(schema, true)?;
    if !s.has_key(key) || s.key(key).value_type() != *glib::VariantTy::BOOLEAN {
        return None;
    }
    Some(gio::Settings::new_full(&s, None::<&gio::SettingsBackend>, None))
}

/// Saver on: switch the listed boolean keys off, remembering which were on.
fn extras_off(list: &[String]) {
    let mut saved = Vec::new();
    for entry in list {
        let Some((schema, key)) = entry.split_once(' ') else { continue };
        let Some(s) = lookup_bool_key(schema, key) else {
            log(format!("saver extra {entry}: schema/key not installed, skipped"));
            continue;
        };
        if s.boolean(key) {
            if s.set_boolean(key, false).is_ok() {
                saved.push(entry.clone());
                log(format!("saver: {entry} off"));
            }
        }
    }
    gio::Settings::sync();
    let f = state_file();
    if let Some(dir) = f.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    // Merge with an earlier list (agent restarted while saver was on).
    let mut all: Vec<String> = std::fs::read_to_string(&f).unwrap_or_default().lines().map(str::to_string).collect();
    for s in saved {
        if !all.contains(&s) {
            all.push(s);
        }
    }
    let _ = std::fs::write(&f, all.join("\n") + "\n");
}

/// Saver off: restore what extras_off switched off (if still off).
fn extras_restore() {
    let f = state_file();
    let Ok(text) = std::fs::read_to_string(&f) else { return };
    for entry in text.lines().filter(|l| !l.is_empty()) {
        let Some((schema, key)) = entry.split_once(' ') else { continue };
        if let Some(s) = lookup_bool_key(schema, key) {
            if !s.boolean(key) && s.set_boolean(key, true).is_ok() {
                log(format!("saver: {entry} restored"));
            }
        }
    }
    gio::Settings::sync();
    let _ = std::fs::remove_file(&f);
}

// ------------------------------------------------------------- indicator
fn fmt_duration(s: i64) -> String {
    let m = (s + 59) / 60;
    if m >= 60 {
        format!("{} h {:02} min", m / 60, m % 60)
    } else {
        format!("{m} min")
    }
}

fn fmt_clock(t: i64) -> String {
    let dt = glib::DateTime::from_unix_local(t).ok();
    dt.and_then(|d| d.format("%H:%M").ok()).map(|s| s.to_string()).unwrap_or_default()
}

/// Indicator lines from status.json.
pub fn info_lines(status: &str) -> Vec<String> {
    let mut out = Vec::new();
    let plugged = jbool(status, "plugged").unwrap_or(false);
    let level = jnum(status, "level").unwrap_or(-1);
    if plugged {
        let mw = jnum(status, "batteryMilliwatts").unwrap_or(0);
        let input = jnum(status, "inputMilliwatts").unwrap_or(0);
        let charger = jstr(status, "charger").unwrap_or_default();
        let charging = jbool(status, "charging").unwrap_or(false);
        if charging && mw > 0 {
            out.push(format!("Charging at {:.1} W ({charger})", mw as f64 / 1000.0));
        } else {
            out.push(format!("Plugged in, not charging ({charger}, {:.1} W in)", input as f64 / 1000.0));
        }
        let mode = jstr(status, "mode").unwrap_or_default();
        let target = jnum(status, "target").unwrap_or(100);
        if mode == "hold" {
            let until = jnum(status, "holdUntil").unwrap_or(0);
            let alarm = jnum(status, "modeAlarm").unwrap_or(0);
            out.push(format!("Adaptive charging: holding at {target} %, to 100 % by {} (alarm {})",
                             fmt_clock(until + 0), fmt_clock(alarm)));
        } else if charging {
            let ttf = jnum(status, "timeToFull").unwrap_or(-1);
            if ttf > 0 && level < target {
                out.push(format!("{} until {target} % (about {})", fmt_duration(ttf), fmt_clock(glib::real_time() / 1_000_000 + ttf)));
            }
        } else if level >= target && target < 100 {
            out.push(format!("Charge limit {target} % reached"));
        }
    }
    out
}

struct Indicator {
    actions: gio::SimpleActionGroup,
    header: gio::SimpleAction,
    saver: gio::SimpleAction,
    info: gio::Menu,
    lines: Vec<String>,
}

fn header_state(saver: bool, label: &str, visible: bool) -> Variant {
    let d = VariantDict::new(None);
    d.insert_value("title", &"Battery".to_variant());
    if saver {
        let icon = gio::ThemedIcon::from_names(&["battery-saver", "power-profile-power-saver-symbolic", "battery-caution-symbolic", "battery-caution"]);
        if let Some(v) = icon.serialize() {
            d.insert_value("icon", &v);
        }
    }
    if !label.is_empty() {
        d.insert_value("label", &label.to_variant());
    }
    let desc = if saver { "Battery saver on" } else { "Charging" };
    d.insert_value("accessible-desc", &desc.to_variant());
    d.insert_value("visible", &visible.to_variant());
    d.end()
}

// ----------------------------------------------------------------- agent
struct Agent {
    settings: gio::Settings,
    system: Option<gio::DBusConnection>,
    session: gio::DBusConnection,
    ind: Indicator,
    saver_active: bool,
    last_alarm: Option<Option<i64>>,
    status_path: PathBuf,
}

fn variant_config(s: &gio::Settings) -> Variant {
    let d = VariantDict::new(None);
    for k in ["charge-limit", "adaptive-hold-level", "adaptive-margin-minutes", "saver-auto-level",
              "saver-cpu-percent", "saver-brightness-cap"] {
        d.insert_value(k, &s.int(k).to_variant());
    }
    for k in ["adaptive-charging", "saver-enabled", "saver-cpu-limit", "saver-stop-waydroid"] {
        d.insert_value(k, &s.boolean(k).to_variant());
    }
    // (a{sv}): a tuple of the dict itself, not of a boxed variant.
    Variant::tuple_from_iter([d.end()])
}

impl Agent {
    fn call(&self, method: &str, args: Option<&Variant>) -> Result<Variant, String> {
        let sys = self.system.as_ref().ok_or("no system bus")?;
        sys.call_sync(Some(BUS_NAME), PATH, IFACE, method, args, None, gio::DBusCallFlags::NONE, 5000,
                      None::<&gio::Cancellable>).map_err(|e| e.to_string())
    }

    fn push_config(&self) {
        if let Err(e) = self.call("SetConfig", Some(&variant_config(&self.settings))) {
            log(format!("SetConfig: {e}"));
        }
    }

    fn refresh_alarm(&mut self, force: bool) {
        let now = glib::real_time() / 1_000_000;
        let alarm = match query_next_alarm(&self.session, now) {
            Ok(a) => a,
            Err(e) => {
                log(format!("next alarm: {e}"));
                None
            }
        };
        if force || self.last_alarm != Some(alarm) || alarm.is_some() {
            // Re-sent every poll while set: the daemon treats it as stale after 30 min.
            if self.last_alarm != Some(alarm) {
                log(format!("next alarm: {}", alarm.map(fmt_clock).unwrap_or_else(|| "none".into())));
            }
            if let Err(e) = self.call("SetNextAlarm", Some(&(alarm.unwrap_or(0),).to_variant())) {
                log(format!("SetNextAlarm: {e}"));
            }
            self.last_alarm = Some(alarm);
        }
    }

    fn saver_changed(&mut self, active: bool) {
        if active == self.saver_active {
            return;
        }
        self.saver_active = active;
        if active {
            extras_off(&self.settings.strv("saver-extras").iter().map(|s| s.to_string()).collect::<Vec<_>>());
        } else {
            extras_restore();
        }
        let _ = self.settings.set_boolean("saver-active", active);
        self.ind.saver.set_state(&active.to_variant());
        self.update_indicator();
    }

    fn status(&self) -> String {
        std::fs::read_to_string(&self.status_path).unwrap_or_default()
    }

    fn update_indicator(&mut self) {
        let status = self.status();
        let plugged = jbool(&status, "plugged").unwrap_or(false);
        let charging = jbool(&status, "charging").unwrap_or(false);
        let show_charge = self.settings.boolean("charging-indicator") && plugged;
        let mw = jnum(&status, "batteryMilliwatts").unwrap_or(0);
        let label = if show_charge && charging && mw > 0 { format!("{:.1} W", mw as f64 / 1000.0) } else { String::new() };
        let visible = self.saver_active || (show_charge && !label.is_empty());
        self.ind.header.set_state(&header_state(self.saver_active, &label, visible));
        let mut lines = info_lines(&status);
        if self.saver_active {
            let auto = jbool(&status, "auto").unwrap_or(false);
            lines.insert(0, if auto { "Battery saver is on (low battery)".into() } else { "Battery saver is on".into() });
        }
        if lines != self.ind.lines {
            self.ind.info.remove_all();
            for l in &lines {
                let item = gio::MenuItem::new(Some(l), None);
                self.ind.info.append_item(&item);
            }
            self.ind.lines = lines;
        }
    }
}

fn build_indicator(agent_cell: &Rc<RefCell<Option<Agent>>>) -> Indicator {
    let actions = gio::SimpleActionGroup::new();
    let header = gio::SimpleAction::new_stateful("phone-header", None, &header_state(false, "", false));
    actions.add_action(&header);
    let saver = gio::SimpleAction::new_stateful("saver", None, &false.to_variant());
    let a = agent_cell.clone();
    saver.connect_change_state(move |act, v| {
        let Some(v) = v.and_then(|v| v.get::<bool>()) else { return };
        let mut g = a.borrow_mut();
        let Some(agent) = g.as_mut() else { return };
        let status = agent.status();
        if !v && jbool(&status, "auto").unwrap_or(false) {
            let _ = agent.call("DismissAutoSaver", None);
        }
        let _ = agent.settings.set_boolean("saver-enabled", v);
        act.set_state(&v.to_variant());
    });
    actions.add_action(&saver);
    let settings = gio::SimpleAction::new("settings", None);
    settings.connect_activate(|_, _| {
        let _ = std::process::Command::new("lomiri-url-dispatcher").arg("settings:///system/battery").spawn();
    });
    actions.add_action(&settings);

    let info = gio::Menu::new();
    let controls = gio::Menu::new();
    let sw = gio::MenuItem::new(Some("Battery saver"), Some("indicator.saver"));
    sw.set_attribute_value("x-ayatana-type", Some(&"org.ayatana.indicator.switch".to_variant()));
    controls.append_item(&sw);
    controls.append_item(&gio::MenuItem::new(Some("Battery settings…"), Some("indicator.settings")));
    let sub = gio::Menu::new();
    sub.append_section(None, &info);
    sub.append_section(None, &controls);
    let root = gio::MenuItem::new(None, Some("indicator.phone-header"));
    root.set_attribute_value("x-ayatana-type", Some(&"org.ayatana.indicator.root".to_variant()));
    root.set_submenu(Some(&sub));
    let menu = gio::Menu::new();
    menu.append_item(&root);
    // Kept alive by the exports below.
    unsafe { MENU = Some(menu) };
    Indicator { actions, header, saver, info, lines: Vec::new() }
}

static mut MENU: Option<gio::Menu> = None;

pub fn run(status_path: PathBuf) -> Result<(), String> {
    let src = gio::SettingsSchemaSource::default().ok_or("no GSettings schemas")?;
    let schema = src.lookup(SCHEMA, true).ok_or(format!("schema {SCHEMA} not installed"))?;
    let settings = gio::Settings::new_full(&schema, None::<&gio::SettingsBackend>, None);
    let session = gio::bus_get_sync(gio::BusType::Session, None::<&gio::Cancellable>).map_err(|e| e.to_string())?;
    let system = gio::bus_get_sync(gio::BusType::System, None::<&gio::Cancellable>).ok();
    let cell: Rc<RefCell<Option<Agent>>> = Rc::new(RefCell::new(None));
    let ind = build_indicator(&cell);

    session.export_action_group(IND_PATH, &ind.actions).map_err(|e| e.to_string())?;
    #[allow(static_mut_refs)]
    let menu = unsafe { MENU.clone() }.unwrap();
    for profile in ["phone", "phone_greeter", "desktop", "desktop_greeter"] {
        session.export_menu_model(&format!("{IND_PATH}/{profile}"), &menu).map_err(|e| e.to_string())?;
    }
    let _owner = gio::bus_own_name_on_connection(&session, IND_NAME, gio::BusNameOwnerFlags::NONE,
                                                 |_, _| {}, |_, name| log(format!("lost bus name {name}")));

    let status = std::fs::read_to_string(&status_path).unwrap_or_default();
    let saver_on = status.contains("\"saver\":{\"active\":true");
    *cell.borrow_mut() = Some(Agent { settings: settings.clone(), system: system.clone(), session, ind,
                                      saver_active: false, last_alarm: None, status_path });
    {
        let mut g = cell.borrow_mut();
        let a = g.as_mut().unwrap();
        a.push_config();
        a.saver_changed(saver_on);
        a.ind.saver.set_state(&saver_on.to_variant());
        if !saver_on {
            // A previous agent may have died with extras switched off.
            extras_restore();
        }
        a.refresh_alarm(true);
        a.update_indicator();
    }

    // GSettings -> daemon (debounced).
    let pending: Rc<RefCell<Option<glib::SourceId>>> = Rc::new(RefCell::new(None));
    {
        let c = cell.clone();
        let p = pending.clone();
        settings.connect_changed(None, move |_, key| {
            if key == "saver-active" {
                return;
            }
            if key == "charging-indicator" {
                if let Some(a) = c.borrow_mut().as_mut() {
                    a.update_indicator();
                }
                return;
            }
            if key == "saver-dismiss" {
                if let Some(a) = c.borrow().as_ref() {
                    let _ = a.call("DismissAutoSaver", None);
                }
                return;
            }
            if let Some(id) = p.borrow_mut().take() {
                id.remove();
            }
            let c2 = c.clone();
            let p2 = p.clone();
            let id = glib::timeout_add_local_once(std::time::Duration::from_millis(300), move || {
                p2.borrow_mut().take();
                if let Some(a) = c2.borrow().as_ref() {
                    a.push_config();
                }
            });
            *p.borrow_mut() = Some(id);
        });
    }

    if let Some(sys) = &system {
        let c = cell.clone();
        #[allow(deprecated)]
        sys.signal_subscribe(Some(BUS_NAME), Some(IFACE), Some("SaverChanged"), Some(PATH), None,
                             gio::DBusSignalFlags::NONE, move |_, _, _, _, _, params| {
            if let Some((active, _auto)) = params.get::<(bool, bool)>() {
                if let Some(a) = c.borrow_mut().as_mut() {
                    a.saver_changed(active);
                }
            }
        });
        // Daemon restarted: resend the settings and the alarm.
        let c = cell.clone();
        let _watch = gio::bus_watch_name_on_connection(sys, BUS_NAME, gio::BusNameWatcherFlags::NONE,
            move |_, _, _| {
                if let Some(a) = c.borrow_mut().as_mut() {
                    a.push_config();
                    a.refresh_alarm(true);
                }
            }, |_, _| {});
        std::mem::forget(_watch);
    }

    // At login the datetime indicator may not be up yet ("not activatable"):
    // ask again after 60 s instead of waiting for the 5 min poll.
    {
        let c = cell.clone();
        glib::timeout_add_seconds_local_once(60, move || {
            if let Some(a) = c.borrow_mut().as_mut() {
                a.refresh_alarm(false);
            }
        });
    }

    // Alarm poll every 5 min; indicator refresh every 30 s while plugged or
    // saving (status.json changes), every 5 min otherwise.
    {
        let c = cell.clone();
        glib::timeout_add_seconds_local(300, move || {
            if let Some(a) = c.borrow_mut().as_mut() {
                a.refresh_alarm(false);
            }
            glib::ControlFlow::Continue
        });
        let c = cell.clone();
        let tick = Rc::new(RefCell::new(0u32));
        glib::timeout_add_seconds_local(30, move || {
            let mut t = tick.borrow_mut();
            *t += 1;
            if let Some(a) = c.borrow_mut().as_mut() {
                let busy = a.saver_active || jbool(&a.status(), "plugged").unwrap_or(false);
                if busy || *t % 10 == 0 {
                    a.update_indicator();
                }
            }
            glib::ControlFlow::Continue
        });
    }
    log("fp6-batteryd agent ready");
    glib::MainLoop::new(None, false).run();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_signature() {
        let d = VariantDict::new(None);
        d.insert_value("charge-limit", &85i32.to_variant());
        assert_eq!(Variant::tuple_from_iter([d.end()]).type_().as_str(), "(a{sv})");
    }

    #[test]
    fn json_lookup() {
        let s = r#"{"time":5,"level":77,"charging":{"plugged":true,"charger":"USB \"PD\"","batteryMilliwatts":7500,"timeToFull":-1},"saver":{"active":false}}"#;
        assert_eq!(jnum(s, "level"), Some(77));
        assert_eq!(jnum(s, "timeToFull"), Some(-1));
        assert_eq!(jbool(s, "plugged"), Some(true));
        assert_eq!(jstr(s, "charger").as_deref(), Some("USB \"PD\""));
        assert_eq!(jbool(s, "missing"), None);
        let l = info_lines(s);
        assert_eq!(l[0], "Plugged in, not charging (USB \"PD\", 0.0 W in)");
    }

    #[test]
    fn alarm_from_menu_reply() {
        // (a(uuaa{sv})): one group with an appointment and two alarms.
        let item = |ty: &str, t: i64| {
            let d = VariantDict::new(None);
            d.insert_value("x-ayatana-type", &ty.to_variant());
            d.insert_value("x-ayatana-time", &t.to_variant());
            d.insert_value("label", &"x".to_variant());
            d.end()
        };
        let items = Variant::array_from_iter_with_type(glib::VariantTy::VARDICT, [
            item("org.ayatana.indicator.appointment", 1500),
            item("org.ayatana.indicator.alarm", 3000),
            item("org.ayatana.indicator.alarm", 2000),
            item("org.ayatana.indicator.alarm", 500),
        ]);
        let group = Variant::tuple_from_iter([1u32.to_variant(), 2u32.to_variant(), items]);
        let groups = Variant::array_from_iter_with_type(group.type_(), [group.clone()]);
        let reply = Variant::tuple_from_iter([groups]);
        assert_eq!(next_alarm_from_menus(&reply, 1000), Some(2000));
        assert_eq!(next_alarm_from_menus(&reply, 5000), None);
    }
}
