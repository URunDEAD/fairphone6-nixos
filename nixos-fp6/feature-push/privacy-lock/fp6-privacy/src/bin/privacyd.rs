// SPDX-License-Identifier: GPL-3.0-only
//! fp6-privacyd (user service of the Lomiri session), GSettings
//! com.fairphone6.privacy:
//!
//!  * Emergency information: Settings writes `emergency-info` (JSON); while
//!    `emergency-on-lockscreen` is on, a sanitised copy goes to
//!    /var/lib/fp6-privacy/emergency.json (directory 2750 <user>:lightdm,
//!    file 0640) where both lock screens can read it: the session's own
//!    Lomiri (the user) and the LightDM greeter (group lightdm). Nobody else
//!    can. Off = the file holds {"show": false} only.
//!  * SOS: `sos-presses` quick power presses (com.fairphone6.PowerKey.Press
//!    from fp6-lomiri-powerd) -> a critical notification with a countdown and
//!    Cancel -> a call to `sos-number` (oFono VoiceCallManager.Dial), then
//!    optionally an SMS with the location to the emergency contacts.
//!    `sos-enabled` defaults to off. In test mode (`test-mode`, or
//!    --dry-run, or the sos-simulate command) the dialer is a mock: it only
//!    logs and notifies what it would do; no call, no SMS.
//!  * Wi-Fi MAC policy: publishes the saved Wi-Fi networks (`wifi-networks`)
//!    and applies `wifi-mac-randomization` / `wifi-mac-modes` per network
//!    with nmcli (takes effect at the next connection to that network).
//!
//! Commands: fp6-privacyd [run] [--dry-run] | sos-simulate | sms-text

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use fp6_privacy::sos::{self, Fix, PressCounter};
use fp6_privacy::{log, mac, write_atomic};
use gio::prelude::*;
use serde_json::{json, Value};
use zbus::message::Type as MessageType;
use zbus::zvariant::{ObjectPath, OwnedObjectPath, OwnedValue, Value as ZValue};
use zbus::MatchRule;

const SCHEMA: &str = "com.fairphone6.privacy";
const EMERGENCY_FILE: &str = "/var/lib/fp6-privacy/emergency.json";

#[derive(Clone, Debug, Default)]
struct Config {
    sos_enabled: bool,
    sos_presses: u32,
    sos_countdown: u32,
    sos_number: String,
    sos_send_sms: bool,
    test_mode: bool,
    owner: String,
    contacts: Vec<(String, String)>,
}

// ---- emergency info -----------------------------------------------------
fn clean(v: Option<&Value>, max: usize) -> String {
    let s = v.and_then(Value::as_str).unwrap_or("");
    s.chars().filter(|c| !c.is_control() || *c == '\n').take(max).collect::<String>().trim().to_owned()
}

/// The lock-screen copy: known fields only, bounded lengths, valid numbers.
fn lockscreen_doc(info: &Value, show: bool, test_mode: bool, sos_number: &str) -> Value {
    if !show {
        return json!({"show": false});
    }
    let contacts: Vec<Value> = info
        .get("contacts")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .take(5)
                .filter_map(|c| {
                    let number = clean(c.get("number"), 24).replace(' ', "");
                    sos::valid_number(&number).then(|| json!({"name": clean(c.get("name"), 60), "relation": clean(c.get("relation"), 40), "number": number}))
                })
                .collect()
        })
        .unwrap_or_default();
    json!({
        "show": true,
        "testMode": test_mode,
        "ownerName": clean(info.get("ownerName"), 80),
        "ownerMessage": clean(info.get("ownerMessage"), 300),
        "bloodType": clean(info.get("bloodType"), 12),
        "allergies": clean(info.get("allergies"), 300),
        "medications": clean(info.get("medications"), 300),
        "conditions": clean(info.get("conditions"), 300),
        "notes": clean(info.get("notes"), 500),
        "organDonor": info.get("organDonor").and_then(Value::as_bool).unwrap_or(false),
        "contacts": contacts,
        "emergencyNumber": if sos::valid_number(sos_number) { sos_number } else { "112" },
    })
}

fn contacts_of(info: &Value) -> Vec<(String, String)> {
    info.get("contacts")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|c| {
                    let n = clean(c.get("number"), 24).replace(' ', "");
                    sos::valid_number(&n).then(|| (clean(c.get("name"), 60), n))
                })
                .collect()
        })
        .unwrap_or_default()
}

fn write_emergency(s: &gio::Settings) {
    let info: Value = serde_json::from_str(&s.string("emergency-info")).unwrap_or(json!({}));
    let doc = lockscreen_doc(&info, s.boolean("emergency-on-lockscreen"), s.boolean("test-mode"), &s.string("sos-number"));
    let path = PathBuf::from(std::env::var("FP6_EMERGENCY_FILE").unwrap_or_else(|_| EMERGENCY_FILE.into()));
    match write_atomic(&path, doc.to_string().as_bytes(), 0o640) {
        Ok(()) => log(format!("lock-screen emergency info {}", if doc["show"] == true { "updated" } else { "hidden" })),
        Err(e) => log(format!("{}: {e}", path.display())),
    }
}

fn read_config(s: &gio::Settings) -> Config {
    let info: Value = serde_json::from_str(&s.string("emergency-info")).unwrap_or(json!({}));
    Config {
        sos_enabled: s.boolean("sos-enabled"),
        sos_presses: s.uint("sos-presses"),
        sos_countdown: s.uint("sos-countdown"),
        sos_number: s.string("sos-number").to_string(),
        sos_send_sms: s.boolean("sos-send-sms"),
        test_mode: s.boolean("test-mode"),
        owner: clean(info.get("ownerName"), 80),
        contacts: contacts_of(&info),
    }
}

// ---- notifications ----------------------------------------------------------
struct Notifier {
    conn: zbus::blocking::Connection,
}

impl Notifier {
    fn notify(&self, id: u32, summary: &str, body: &str, actions: &[&str], critical: bool) -> u32 {
        let mut hints: HashMap<&str, ZValue> = HashMap::new();
        hints.insert("urgency", ZValue::U8(if critical { 2 } else { 1 }));
        if actions.len() > 2 {
            hints.insert("x-lomiri-snap-decisions", ZValue::from("true"));
            hints.insert("x-lomiri-private-rejection-tint", ZValue::from("true"));
        }
        let actions: Vec<&str> = actions.to_vec();
        self.conn
            .call_method(
                Some("org.freedesktop.Notifications"),
                "/org/freedesktop/Notifications",
                Some("org.freedesktop.Notifications"),
                "Notify",
                &("fp6-privacy", id, "dialog-warning-symbolic", summary, body, actions, hints, if critical { 0i32 } else { 8000 }),
            )
            .ok()
            .and_then(|r| r.body().deserialize::<u32>().ok())
            .unwrap_or(0)
    }

    fn close(&self, id: u32) {
        let _ = self.conn.call_method(
            Some("org.freedesktop.Notifications"),
            "/org/freedesktop/Notifications",
            Some("org.freedesktop.Notifications"),
            "CloseNotification",
            &(id,),
        );
    }
}

fn vibrate(system: &zbus::blocking::Connection, ms: i32) {
    let _ = system.call_method(Some("com.lomiri.hfd"), "/com/lomiri/hfd", Some("com.lomiri.hfd.Vibrator"), "vibrate", &(ms,));
}

// ---- dialer: oFono or mock ----------------------------------------------------
trait Dialer {
    fn call(&self, number: &str) -> Result<(), String>;
    fn sms(&self, number: &str, text: &str) -> Result<(), String>;
    fn is_test(&self) -> bool;
}

struct MockDialer {
    log: Arc<Mutex<Vec<String>>>,
}

impl Dialer for MockDialer {
    fn call(&self, number: &str) -> Result<(), String> {
        log(format!("TEST MODE: would call {number} (no call placed)"));
        self.log.lock().unwrap().push(format!("call {number}"));
        Ok(())
    }
    fn sms(&self, number: &str, text: &str) -> Result<(), String> {
        log(format!("TEST MODE: would send an SMS to {number}: {text}"));
        self.log.lock().unwrap().push(format!("sms {number}"));
        Ok(())
    }
    fn is_test(&self) -> bool {
        true
    }
}

struct OfonoDialer {
    system: zbus::blocking::Connection,
}

impl OfonoDialer {
    fn modem_with(&self, iface: &str) -> Result<OwnedObjectPath, String> {
        let reply = self
            .system
            .call_method(Some("org.ofono"), "/", Some("org.ofono.Manager"), "GetModems", &())
            .map_err(|e| format!("oFono GetModems: {e}"))?;
        let modems: Vec<(OwnedObjectPath, HashMap<String, OwnedValue>)> =
            reply.body().deserialize().map_err(|e| format!("oFono modems: {e}"))?;
        modems
            .into_iter()
            .find(|(_, props)| {
                props
                    .get("Interfaces")
                    .and_then(|v| <Vec<String>>::try_from(v.try_clone().ok()?).ok())
                    .is_some_and(|ifs| ifs.iter().any(|i| i == iface))
            })
            .map(|(p, _)| p)
            .ok_or_else(|| format!("no modem with {iface}"))
    }
}

impl Dialer for OfonoDialer {
    fn call(&self, number: &str) -> Result<(), String> {
        let modem = self.modem_with("org.ofono.VoiceCallManager")?;
        log(format!("SOS: dialling {number} on {}", modem.as_str()));
        self.system
            .call_method(Some("org.ofono"), modem.as_str(), Some("org.ofono.VoiceCallManager"), "Dial", &(number, ""))
            .map(|_| ())
            .map_err(|e| format!("Dial: {e}"))
    }
    fn sms(&self, number: &str, text: &str) -> Result<(), String> {
        let modem = self.modem_with("org.ofono.MessageManager")?;
        self.system
            .call_method(Some("org.ofono"), modem.as_str(), Some("org.ofono.MessageManager"), "SendMessage", &(number, text))
            .map(|_| ())
            .map_err(|e| format!("SendMessage: {e}"))
    }
    fn is_test(&self) -> bool {
        false
    }
}

// ---- location for the SOS SMS (GeoClue; refused while location is off) ---------
fn geoclue_fix(system: &zbus::blocking::Connection, timeout: Duration) -> Option<Fix> {
    let mgr = "/org/freedesktop/GeoClue2/Manager";
    let client: OwnedObjectPath = system
        .call_method(Some("org.freedesktop.GeoClue2"), mgr, Some("org.freedesktop.GeoClue2.Manager"), "CreateClient", &())
        .ok()?
        .body()
        .deserialize()
        .ok()?;
    let set = |name: &str, v: ZValue| {
        system.call_method(
            Some("org.freedesktop.GeoClue2"),
            client.as_str(),
            Some("org.freedesktop.DBus.Properties"),
            "Set",
            &("org.freedesktop.GeoClue2.Client", name, v),
        )
    };
    let _ = set("DesktopId", ZValue::from("fp6-privacy-sos"));
    let _ = set("RequestedAccuracyLevel", ZValue::U32(8));
    let rule = MatchRule::builder()
        .msg_type(MessageType::Signal)
        .interface("org.freedesktop.GeoClue2.Client")
        .ok()?
        .member("LocationUpdated")
        .ok()?
        .path(client.as_str())
        .ok()?
        .build();
    let iter = zbus::blocking::MessageIterator::for_match_rule(rule, system, Some(4)).ok()?;
    let started = system
        .call_method(Some("org.freedesktop.GeoClue2"), client.as_str(), Some("org.freedesktop.GeoClue2.Client"), "Start", &())
        .is_ok();
    let (tx, rx) = mpsc::channel();
    if started {
        std::thread::spawn(move || {
            for msg in iter.flatten() {
                if let Ok((_, new)) = msg.body().deserialize::<(OwnedObjectPath, OwnedObjectPath)>() {
                    let _ = tx.send(new);
                    break;
                }
            }
        });
    }
    let fix = rx.recv_timeout(timeout).ok().and_then(|loc| {
        let get = |name: &str| -> Option<f64> {
            let r = system
                .call_method(
                    Some("org.freedesktop.GeoClue2"),
                    loc.as_str(),
                    Some("org.freedesktop.DBus.Properties"),
                    "Get",
                    &("org.freedesktop.GeoClue2.Location", name),
                )
                .ok()?;
            let v: OwnedValue = r.body().deserialize().ok()?;
            f64::try_from(&v).ok()
        };
        Some(Fix { lat: get("Latitude")?, lon: get("Longitude")?, accuracy_m: get("Accuracy").unwrap_or(0.0) })
    });
    let _ = system.call_method(Some("org.freedesktop.GeoClue2"), client.as_str(), Some("org.freedesktop.GeoClue2.Client"), "Stop", &());
    let path = ObjectPath::try_from(client.as_str()).ok()?;
    let _ = system.call_method(Some("org.freedesktop.GeoClue2"), mgr, Some("org.freedesktop.GeoClue2.Manager"), "DeleteClient", &(path,));
    fix
}

// ---- SOS flow ------------------------------------------------------------------
struct Sos {
    config: Arc<Mutex<Config>>,
    session: zbus::blocking::Connection,
    system: zbus::blocking::Connection,
    force_test: bool,
    running: AtomicBool,
    cancel: Mutex<Option<mpsc::Sender<String>>>,
    notification: Mutex<u32>,
    mock_log: Arc<Mutex<Vec<String>>>,
}

impl Sos {
    fn dialer(&self, cfg: &Config) -> Box<dyn Dialer> {
        if self.force_test || cfg.test_mode {
            Box::new(MockDialer { log: self.mock_log.clone() })
        } else {
            Box::new(OfonoDialer { system: self.system.clone() })
        }
    }

    fn action_invoked(&self, id: u32, action: &str) {
        if id != 0 && id == *self.notification.lock().unwrap() {
            if let Some(tx) = self.cancel.lock().unwrap().as_ref() {
                let _ = tx.send(action.to_owned());
            }
        }
    }

    fn notification_closed(&self, id: u32) {
        // Dismissing the countdown (swipe) counts as cancel.
        self.action_invoked(id, "cancel");
    }

    /// Countdown + call + SMS. Returns what happened (for logs and tests).
    fn run(&self, cfg: Config) -> &'static str {
        if self.running.swap(true, Ordering::SeqCst) {
            return "already running";
        }
        let result = self.run_inner(cfg);
        self.running.store(false, Ordering::SeqCst);
        *self.cancel.lock().unwrap() = None;
        result
    }

    fn run_inner(&self, cfg: Config) -> &'static str {
        let dialer = self.dialer(&cfg);
        let test = dialer.is_test();
        let number = if sos::valid_number(&cfg.sos_number) { cfg.sos_number.clone() } else { "112".into() };
        let notifier = Notifier { conn: self.session.clone() };
        let (tx, rx) = mpsc::channel::<String>();
        *self.cancel.lock().unwrap() = Some(tx);
        let title = if test { "Emergency SOS (test mode)" } else { "Emergency SOS" };
        let mut id = 0;
        let countdown = cfg.sos_countdown.clamp(3, 30);
        log(format!("SOS triggered: countdown {countdown} s, number {number}{}", if test { ", TEST MODE" } else { "" }));
        let deadline = Instant::now() + Duration::from_secs(countdown as u64);
        loop {
            let left = deadline.saturating_duration_since(Instant::now()).as_secs_f64().ceil() as u64;
            if left == 0 {
                break;
            }
            let body = if test {
                format!("Test mode: no call will be placed. Would call {number} in {left} s.")
            } else {
                format!("Calling {number} in {left} s. Tap Cancel to stop.")
            };
            id = notifier.notify(id, title, &body, &["cancel", "Cancel", "now", if test { "Test now" } else { "Call now" }], true);
            *self.notification.lock().unwrap() = id;
            vibrate(&self.system, 300);
            match rx.recv_timeout(Duration::from_secs(1)) {
                Ok(a) if a == "now" => break,
                Ok(_) => {
                    notifier.close(id);
                    log("SOS cancelled");
                    let _ = notifier.notify(0, title, "Emergency call cancelled.", &[], false);
                    return "cancelled";
                }
                Err(_) => {}
            }
        }
        notifier.close(id);
        *self.notification.lock().unwrap() = 0;
        if let Err(e) = dialer.call(&number) {
            log(format!("SOS call failed: {e}"));
            let _ = notifier.notify(0, title, &format!("Could not call {number}: {e}. Dial it from the lock screen (Emergency)."), &[], true);
            return "call failed";
        }
        if test {
            let _ = notifier.notify(0, title, &format!("Test: would now call {number}."), &[], false);
        }
        if cfg.sos_send_sms && !cfg.contacts.is_empty() {
            let fix = geoclue_fix(&self.system, Duration::from_secs(30));
            let text = sos::sms_text(&cfg.owner, &number, fix.as_ref());
            let mut sent = 0;
            for (name, n) in &cfg.contacts {
                match dialer.sms(n, &text) {
                    Ok(()) => sent += 1,
                    Err(e) => log(format!("SOS SMS to {name}: {e}")),
                }
            }
            let what = if test { "Test: would have sent" } else { "Sent" };
            let _ = notifier.notify(0, title, &format!("{what} the SOS message to {sent} emergency contact(s){}.", if fix.is_some() { " with your location" } else { " (location unavailable)" }), &[], false);
        }
        "called"
    }
}

fn watch_power_key(sos: Arc<Sos>) -> zbus::Result<()> {
    let rule = MatchRule::builder()
        .msg_type(MessageType::Signal)
        .interface("com.fairphone6.PowerKey")?
        .member("Press")?
        .path("/com/fairphone6/PowerKey")?
        .build();
    let iter = zbus::blocking::MessageIterator::for_match_rule(rule, &sos.system, Some(32))?;
    let mut counter = PressCounter::new(5, 1.0);
    // Only fp6-lomiri-powerd (owner of com.lomiri.Repowerd, root) may count:
    // the system bus lets any client broadcast a look-alike signal.
    let dbus = zbus::blocking::fdo::DBusProxy::new(&sos.system)?;
    let powerd_owner = |dbus: &zbus::blocking::fdo::DBusProxy| -> Option<String> {
        dbus.get_name_owner("com.lomiri.Repowerd".try_into().ok()?).ok().map(|n| n.to_string())
    };
    let mut owner = powerd_owner(&dbus);
    for msg in iter.flatten() {
        let header = msg.header();
        let sender = header.sender().map(|s| s.to_string());
        if sender.is_none() || sender != owner {
            owner = powerd_owner(&dbus); // powerd may have restarted
            if sender.is_none() || sender != owner {
                log(format!("ignoring PowerKey.Press from {:?} (not fp6-lomiri-powerd)", sender));
                continue;
            }
        }
        let Ok((t, _on)) = msg.body().deserialize::<(f64, bool)>() else { continue };
        let cfg = sos.config.lock().unwrap().clone();
        if !cfg.sos_enabled {
            counter.reset();
            continue;
        }
        if counter.needed != cfg.sos_presses.clamp(3, 10) as usize {
            counter = PressCounter::new(cfg.sos_presses.clamp(3, 10) as usize, 1.0);
        }
        if counter.press(t) {
            let sos = sos.clone();
            std::thread::spawn(move || {
                let r = sos.run(cfg);
                log(format!("SOS: {r}"));
            });
        }
    }
    Ok(())
}

fn watch_notifications(sos: Arc<Sos>) -> zbus::Result<()> {
    for (member, closed) in [("ActionInvoked", false), ("NotificationClosed", true)] {
        let rule = MatchRule::builder()
            .msg_type(MessageType::Signal)
            .interface("org.freedesktop.Notifications")?
            .member(member)?
            .build();
        let iter = zbus::blocking::MessageIterator::for_match_rule(rule, &sos.session, Some(16))?;
        let sos = sos.clone();
        std::thread::spawn(move || {
            for msg in iter.flatten() {
                if closed {
                    if let Ok((id, _reason)) = msg.body().deserialize::<(u32, u32)>() {
                        sos.notification_closed(id);
                    }
                } else if let Ok((id, action)) = msg.body().deserialize::<(u32, String)>() {
                    sos.action_invoked(id, &action);
                }
            }
        });
    }
    Ok(())
}

// ---- Wi-Fi MAC policy -------------------------------------------------------------
#[derive(Debug, Clone)]
struct Network {
    uuid: String,
    name: String,
    nm: String,
}

fn nmcli(args: &[&str]) -> Option<String> {
    let out = Command::new("nmcli").args(args).output().ok()?;
    if !out.status.success() {
        log(format!("nmcli {}: {}", args.join(" "), String::from_utf8_lossy(&out.stderr).trim()));
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Saved Wi-Fi client networks (hotspot/AP profiles skipped).
fn wifi_networks() -> Vec<Network> {
    let Some(list) = nmcli(&["-t", "-e", "no", "-f", "UUID,TYPE", "connection", "show"]) else { return Vec::new() };
    let mut out = Vec::new();
    for line in list.lines() {
        let Some((uuid, ty)) = line.split_once(':') else { continue };
        if ty != "802-11-wireless" {
            continue;
        }
        let Some(info) = nmcli(&["-g", "connection.id,802-11-wireless.mode,802-11-wireless.cloned-mac-address", "connection", "show", uuid]) else { continue };
        let fields: Vec<&str> = info.lines().collect();
        let (name, mode, nm) = (fields.first().copied().unwrap_or(""), fields.get(1).copied().unwrap_or(""), fields.get(2).copied().unwrap_or(""));
        if matches!(mode, "ap" | "adhoc" | "mesh") {
            continue;
        }
        out.push(Network { uuid: uuid.into(), name: name.into(), nm: nm.into() });
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// This boot's address of the Wi-Fi interface (ethtool's permanent address,
/// which on the FP6 is the firmware's made-up MAC of this boot).
fn device_mac() -> String {
    let iface = std::fs::read_dir("/sys/class/net")
        .ok()
        .and_then(|d| d.flatten().find(|e| e.path().join("wireless").exists()))
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .unwrap_or_else(|| "wlan0".into());
    Command::new("ethtool")
        .args(["-P", &iface])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .and_then(|t| t.rsplit(' ').next().map(|m| m.trim().to_owned()))
        .filter(|m| mac::is_mac(m))
        .or_else(|| std::fs::read_to_string(format!("/sys/class/net/{iface}/address")).ok().map(|m| m.trim().to_owned()))
        .unwrap_or_default()
}

fn apply_mac_policy(s: &gio::Settings) {
    let master = s.boolean("wifi-mac-randomization");
    let stored: Vec<String> = s.strv("wifi-mac-modes").iter().map(|g| g.to_string()).collect();
    let mut modes: HashMap<String, String> = stored
        .iter()
        .filter_map(|l| l.split_once('\t').map(|(u, m)| (u.to_owned(), m.to_owned())))
        .collect();
    let networks = wifi_networks();
    let mut modes_changed = false;
    let mut published = Vec::new();
    for n in &networks {
        let current = mac::mode_of(&n.nm);
        // First sight of a network: adopt what it has (networks saved before
        // the module were pinned to "device" by wifi-mac-migrate).
        let chosen = modes.entry(n.uuid.clone()).or_insert_with(|| {
            modes_changed = true;
            if master { current.to_owned() } else { "device".to_owned() }
        });
        let want = if master { chosen.as_str() } else { "device" };
        if want == "device" {
            // Fixed address: an explicit MAC, kept once set (ath11k makes up
            // a new "device" MAC on every boot, see mac.rs).
            match mac::fixed_value(&n.nm, &device_mac()) {
                Some(fixed) if fixed != n.nm.trim().to_ascii_lowercase() => {
                    if nmcli(&["connection", "modify", &n.uuid, "802-11-wireless.cloned-mac-address", &fixed]).is_some() {
                        log(format!("Wi-Fi \"{}\": fixed MAC address (next connection)", n.name));
                    }
                }
                Some(_) => {}
                None => log("no Wi-Fi device address yet; fixed address deferred"),
            }
        } else if want != current && want != "custom" {
            if let Some(v) = mac::nm_value(want) {
                if nmcli(&["connection", "modify", &n.uuid, "802-11-wireless.cloned-mac-address", v]).is_some() {
                    log(format!("Wi-Fi \"{}\": MAC {} (next connection)", n.name, want));
                }
            }
        }
        published.push(format!("{}\t{}\t{}", n.uuid, n.name, if master { chosen.as_str() } else { "device" }));
    }
    let known: std::collections::HashSet<&str> = networks.iter().map(|n| n.uuid.as_str()).collect();
    let before = modes.len();
    modes.retain(|k, _| known.contains(k.as_str()));
    if modes_changed || modes.len() != before {
        let mut list: Vec<String> = modes.iter().map(|(u, m)| format!("{u}\t{m}")).collect();
        list.sort();
        let _ = s.set_strv("wifi-mac-modes", list.iter().map(String::as_str).collect::<Vec<_>>().as_slice());
    }
    let current: Vec<String> = s.strv("wifi-networks").iter().map(|g| g.to_string()).collect();
    if current != published {
        let _ = s.set_strv("wifi-networks", published.iter().map(String::as_str).collect::<Vec<_>>().as_slice());
    }
}

fn watch_nm(system: zbus::blocking::Connection, tx: mpsc::Sender<()>) -> zbus::Result<()> {
    let rule = MatchRule::builder()
        .msg_type(MessageType::Signal)
        .sender("org.freedesktop.NetworkManager")?
        .interface("org.freedesktop.NetworkManager.Settings")?
        .build();
    let removed = MatchRule::builder()
        .msg_type(MessageType::Signal)
        .sender("org.freedesktop.NetworkManager")?
        .interface("org.freedesktop.NetworkManager.Settings.Connection")?
        .member("Removed")?
        .build();
    let a = zbus::blocking::MessageIterator::for_match_rule(rule, &system, Some(16))?;
    let b = zbus::blocking::MessageIterator::for_match_rule(removed, &system, Some(16))?;
    let tx2 = tx.clone();
    std::thread::spawn(move || {
        for _ in b.flatten() {
            let _ = tx2.send(());
        }
    });
    for _ in a.flatten() {
        let _ = tx.send(());
    }
    Ok(())
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cmd = args.iter().find(|a| !a.starts_with("--")).cloned().unwrap_or_else(|| "run".into());
    let dry_run = args.iter().any(|a| a == "--dry-run") || std::env::var("FP6_SOS_DRY_RUN").is_ok_and(|v| v == "1");
    if cmd == "sms-text" {
        println!("{}", sos::sms_text("", "112", Some(&Fix { lat: 0.0, lon: 0.0, accuracy_m: 0.0 })));
        return;
    }
    let (session, system) = match (zbus::blocking::Connection::session(), zbus::blocking::Connection::system()) {
        (Ok(a), Ok(b)) => (a, b),
        (a, b) => {
            log(format!("D-Bus: {:?} {:?}", a.err(), b.err()));
            std::process::exit(1);
        }
    };
    let settings = gio::SettingsSchemaSource::default()
        .and_then(|s| s.lookup(SCHEMA, true))
        .map(|_| gio::Settings::new(SCHEMA));
    let config = Arc::new(Mutex::new(settings.as_ref().map(read_config).unwrap_or_default()));
    let sos = Arc::new(Sos {
        config: config.clone(),
        session: session.clone(),
        system: system.clone(),
        force_test: dry_run || cmd == "sos-simulate",
        running: AtomicBool::new(false),
        cancel: Mutex::new(None),
        notification: Mutex::new(0),
        mock_log: Arc::new(Mutex::new(Vec::new())),
    });
    if let Err(e) = watch_notifications(sos.clone()) {
        log(format!("notification signals: {e}"));
    }
    if cmd == "sos-simulate" {
        // Forced mock dialer: exercises the countdown, Cancel, location and
        // SMS text with no call and no SMS, whatever the settings say.
        let mut cfg = config.lock().unwrap().clone();
        cfg.test_mode = true;
        if let Some(n) = args.iter().position(|a| a == "--countdown").and_then(|i| args.get(i + 1)).and_then(|v| v.parse().ok()) {
            cfg.sos_countdown = n;
        }
        let r = sos.run(cfg);
        println!("{r}; mock dialer: {:?}", sos.mock_log.lock().unwrap());
        return;
    }
    let Some(settings) = settings else {
        log(format!("GSettings schema {SCHEMA} is not installed"));
        std::process::exit(1);
    };
    if dry_run {
        log("--dry-run: SOS never calls or texts (mock dialer)");
    }
    write_emergency(&settings);
    {
        let config = config.clone();
        settings.connect_changed(None, move |s, key| {
            *config.lock().unwrap() = read_config(s);
            if key.starts_with("emergency") || key == "test-mode" || key == "sos-number" {
                write_emergency(s);
            }
            if key == "wifi-mac-randomization" || key == "wifi-mac-modes" {
                apply_mac_policy(s);
            }
        });
    }
    {
        let sos = sos.clone();
        std::thread::spawn(move || loop {
            if let Err(e) = watch_power_key(sos.clone()) {
                log(format!("PowerKey watch: {e}"));
            }
            std::thread::sleep(Duration::from_secs(10));
        });
    }
    let (tx, rx) = mpsc::channel::<()>();
    {
        let system = system.clone();
        std::thread::spawn(move || loop {
            if let Err(e) = watch_nm(system.clone(), tx.clone()) {
                log(format!("NetworkManager watch: {e}"));
            }
            std::thread::sleep(Duration::from_secs(10));
        });
    }
    apply_mac_policy(&settings);
    // NM change signals arrive on worker threads; GSettings lives here.
    let pending = std::cell::Cell::new(None::<Instant>);
    let s2 = settings.clone();
    glib::timeout_add_local(Duration::from_millis(500), move || {
        while rx.try_recv().is_ok() {
            pending.set(Some(Instant::now()));
        }
        if pending.get().is_some_and(|t| t.elapsed() > Duration::from_millis(1500)) {
            pending.set(None);
            apply_mac_policy(&s2);
        }
        glib::ControlFlow::Continue
    });
    let _keep = Path::new(EMERGENCY_FILE);
    log("ready");
    glib::MainLoop::new(None, false).run();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lockscreen_copy_is_sanitised() {
        let info = json!({
            "ownerName": "Test Owner\u{7}", "bloodType": "0 Rh+", "allergies": "Test allergy",
            "contacts": [{"name": "Test Contact", "number": "+999 123 456 789"}, {"name": "bad", "number": "call me"}],
            "secret": "not copied"
        });
        let d = lockscreen_doc(&info, true, true, "112");
        assert_eq!(d["ownerName"], "Test Owner");
        assert_eq!(d["contacts"].as_array().unwrap().len(), 1);
        assert_eq!(d["contacts"][0]["number"], "+999123456789");
        assert!(d.get("secret").is_none());
        assert_eq!(d["testMode"], true);
        assert_eq!(lockscreen_doc(&info, false, false, "112"), json!({"show": false}));
        assert_eq!(lockscreen_doc(&info, true, false, "abc")["emergencyNumber"], "112");
    }
}
