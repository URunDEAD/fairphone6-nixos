// SPDX-License-Identifier: GPL-3.0-or-later
//! Permissions, decisions, the per-app store and the usage history.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, VecDeque};

/// Runtime permissions handled by the broker. The string forms are the
/// D-Bus / store / QML ids and never change.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Permission {
    /// Raw USB devices (/dev/bus/usb), USB HID (hidraw), USB serial
    /// (ttyACM/ttyUSB) handed out as file descriptors.
    Usb,
    /// NFC (for an NFC daemon to check; no NFC stack on the phone yet).
    Nfc,
    /// Direct Bluetooth device access (L2CAP/RFCOMM sockets), beyond audio.
    Bluetooth,
    Contacts,
    Calendar,
    /// Call log and SMS/MMS.
    Messages,
    /// Files outside the app's own data (Documents, Pictures, Music,
    /// Videos, Downloads, SD card / USB storage).
    Files,
    /// Body and motion sensors (accelerometer, gyroscope, step counter, ...).
    Sensors,
    /// Camera: PipeWire video capture (fp6-media-gate + WirePlumber) and,
    /// for AppArmor-confined apps, the camera device nodes.
    Camera,
    /// Microphone: PipeWire audio capture from a source (fp6-media-gate +
    /// WirePlumber); recording what plays (sink monitors) is not covered.
    Microphone,
    /// Location: GeoClue (the location agent asks the broker).
    Location,
}

impl Permission {
    pub const ALL: [Permission; 11] = [
        Permission::Usb,
        Permission::Nfc,
        Permission::Bluetooth,
        Permission::Contacts,
        Permission::Calendar,
        Permission::Messages,
        Permission::Files,
        Permission::Sensors,
        Permission::Camera,
        Permission::Microphone,
        Permission::Location,
    ];

    /// Camera, microphone and location: enforced only where the media
    /// option of the module is on (fp6-media-gate, GeoClue agent), and
    /// prompted with "Only this time" instead of "Only while using".
    pub const MEDIA: [Permission; 3] = [Permission::Camera, Permission::Microphone, Permission::Location];

    pub fn is_media(self) -> bool {
        Permission::MEDIA.contains(&self)
    }

    /// Answers offered by the prompt.
    pub fn prompt_options(self) -> &'static [&'static str] {
        if self.is_media() {
            &["allow", "once", "deny"]
        } else {
            &["allow", "while_using", "deny"]
        }
    }

    pub fn id(self) -> &'static str {
        match self {
            Permission::Usb => "usb",
            Permission::Nfc => "nfc",
            Permission::Bluetooth => "bluetooth",
            Permission::Contacts => "contacts",
            Permission::Calendar => "calendar",
            Permission::Messages => "messages",
            Permission::Files => "files",
            Permission::Sensors => "sensors",
            Permission::Camera => "camera",
            Permission::Microphone => "microphone",
            Permission::Location => "location",
        }
    }

    pub fn parse(s: &str) -> Option<Permission> {
        Permission::ALL.into_iter().find(|p| p.id() == s)
    }

    /// Prompt wording: "Allow <app> to <phrase>?"
    pub fn phrase(self) -> &'static str {
        match self {
            Permission::Usb => "access a USB device",
            Permission::Nfc => "use NFC",
            Permission::Bluetooth => "connect to Bluetooth devices directly",
            Permission::Contacts => "access your contacts",
            Permission::Calendar => "access your calendar",
            Permission::Messages => "access your call log and messages",
            Permission::Files => "access files outside its own folder",
            Permission::Sensors => "use body and motion sensors",
            Permission::Camera => "use the camera",
            Permission::Microphone => "use the microphone",
            Permission::Location => "access this device's location",
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Permission::Usb => "USB devices",
            Permission::Nfc => "NFC",
            Permission::Bluetooth => "Bluetooth devices",
            Permission::Contacts => "Contacts",
            Permission::Calendar => "Calendar",
            Permission::Messages => "Call log & messages",
            Permission::Files => "Files & media",
            Permission::Sensors => "Body & motion sensors",
            Permission::Camera => "Camera",
            Permission::Microphone => "Microphone",
            Permission::Location => "Location",
        }
    }
}

/// The remembered answer for one (app, permission).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Decision {
    /// Not decided: the next request prompts (only from the foreground).
    #[default]
    Ask,
    Allow,
    /// Allowed while the app is the focused app (plus a short grace).
    WhileUsing,
    Deny,
}

impl Decision {
    pub fn id(self) -> &'static str {
        match self {
            Decision::Ask => "ask",
            Decision::Allow => "allow",
            Decision::WhileUsing => "while_using",
            Decision::Deny => "deny",
        }
    }
    pub fn parse(s: &str) -> Option<Decision> {
        [Decision::Ask, Decision::Allow, Decision::WhileUsing, Decision::Deny]
            .into_iter()
            .find(|d| d.id() == s)
    }
}

/// What the user picked in a prompt.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Answer {
    Allow,
    WhileUsing,
    /// "Only this time": granted until the app has been in the background
    /// longer than the grace period (or the broker restarts); not remembered,
    /// the app asks again after that.
    Once,
    Deny,
    /// Dismissed, timed out, screen locked: this request is refused, the
    /// question is asked again next time.
    Dismissed,
}

impl Answer {
    pub fn parse(s: &str) -> Answer {
        match s {
            "allow" => Answer::Allow,
            "while_using" => Answer::WhileUsing,
            "once" => Answer::Once,
            "deny" => Answer::Deny,
            _ => Answer::Dismissed,
        }
    }
    pub fn remembered(self) -> Option<Decision> {
        match self {
            Answer::Allow => Some(Decision::Allow),
            Answer::WhileUsing => Some(Decision::WhileUsing),
            Answer::Deny => Some(Decision::Deny),
            Answer::Once | Answer::Dismissed => None,
        }
    }
    pub fn granted(self) -> bool {
        matches!(self, Answer::Allow | Answer::WhileUsing | Answer::Once)
    }
}

/// Per-app network permission. Everything allowed by default; a denied
/// class drops that app's traffic (netpolicy's nftables table).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct NetAccess {
    /// Wi-Fi and every other non-mobile, non-VPN link (Ethernet, USB, BT PAN).
    pub wifi: bool,
    pub mobile: bool,
    pub vpn: bool,
    /// Devices on the local network (private / link-local / multicast
    /// destinations over Wi-Fi and other local links).
    pub local: bool,
}

impl Default for NetAccess {
    fn default() -> Self {
        NetAccess { wifi: true, mobile: true, vpn: true, local: true }
    }
}

impl NetAccess {
    pub fn restricted(&self) -> bool {
        !(self.wifi && self.mobile && self.vpn && self.local)
    }
    pub fn none(&self) -> bool {
        !(self.wifi || self.mobile || self.vpn || self.local)
    }
    pub fn set(&mut self, class: &str, on: bool) -> bool {
        match class {
            "wifi" => self.wifi = on,
            "mobile" => self.mobile = on,
            "vpn" => self.vpn = on,
            "local" => self.local = on,
            "all" => *self = if on { NetAccess::default() } else { NetAccess { wifi: false, mobile: false, vpn: false, local: false } },
            _ => return false,
        }
        true
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct PermEntry {
    pub decision: Decision,
    /// Unix seconds of the last decision change (prompt answer or Settings).
    pub changed: u64,
    /// Unix seconds of the last granted use.
    pub last_used: u64,
    /// Unix seconds of the last refused request.
    pub last_denied: u64,
    pub uses: u64,
    /// Short description of the last thing used ("Arduino Uno (2341:0043)").
    pub last_detail: String,
    /// The decision was made by the user (prompt answer or Settings), so a
    /// preset from the configuration no longer applies. Entries written
    /// before this field existed count as explicit once `changed` is set.
    pub explicit: bool,
}

impl PermEntry {
    pub fn is_explicit(&self) -> bool {
        self.explicit || self.changed != 0
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppEntry {
    pub perms: BTreeMap<Permission, PermEntry>,
    pub network: NetAccess,
    pub first_seen: u64,
    pub last_seen: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    /// Granted (remembered decision or prompt answer).
    Used,
    Denied,
    Prompted,
    /// Changed in Settings or by a prompt answer.
    Changed,
    /// An open device handle was taken back (background, Settings).
    Revoked,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Event {
    pub time: u64,
    pub app: String,
    /// Permission id, or "network", "camera", "microphone", "location".
    pub perm: String,
    pub kind: EventKind,
    #[serde(default)]
    pub detail: String,
}

pub const HISTORY_MAX: usize = 400;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Store {
    pub version: u32,
    pub apps: BTreeMap<String, AppEntry>,
    pub history: VecDeque<Event>,
}

impl Store {
    pub fn new() -> Store {
        Store { version: 1, ..Store::default() }
    }

    pub fn app(&mut self, app: &str, now: u64) -> &mut AppEntry {
        let e = self.apps.entry(app.to_owned()).or_default();
        if e.first_seen == 0 {
            e.first_seen = now;
        }
        e.last_seen = now;
        e
    }

    pub fn decision(&self, app: &str, perm: Permission) -> Decision {
        self.apps
            .get(app)
            .and_then(|a| a.perms.get(&perm))
            .map(|p| p.decision)
            .unwrap_or_default()
    }

    /// The user's own decision, if they made one (None: a preset or Ask
    /// applies).
    pub fn explicit_decision(&self, app: &str, perm: Permission) -> Option<Decision> {
        self.apps
            .get(app)
            .and_then(|a| a.perms.get(&perm))
            .filter(|p| p.is_explicit())
            .map(|p| p.decision)
    }

    pub fn set_decision(&mut self, app: &str, perm: Permission, d: Decision, now: u64) {
        let e = self.app(app, now).perms.entry(perm).or_default();
        if e.decision != d || !e.is_explicit() {
            e.decision = d;
            e.changed = now;
        }
        e.explicit = true;
    }

    pub fn log(&mut self, ev: Event) {
        // Collapse repeated identical uses within a minute (an app polling).
        if let Some(last) = self.history.back_mut() {
            if last.app == ev.app && last.perm == ev.perm && last.kind == ev.kind
                && last.detail == ev.detail && ev.time.saturating_sub(last.time) < 60
            {
                last.time = ev.time;
                return;
            }
        }
        self.history.push_back(ev);
        while self.history.len() > HISTORY_MAX {
            self.history.pop_front();
        }
    }

    pub fn note(&mut self, app: &str, perm: Permission, granted: bool, detail: &str, now: u64) {
        let e = self.app(app, now).perms.entry(perm).or_default();
        if granted {
            e.last_used = now;
            e.uses = e.uses.saturating_add(1);
            if !detail.is_empty() {
                e.last_detail = detail.chars().take(120).collect();
            }
        } else {
            e.last_denied = now;
        }
        self.log(Event {
            time: now,
            app: app.to_owned(),
            perm: perm.id().to_owned(),
            kind: if granted { EventKind::Used } else { EventKind::Denied },
            detail: detail.chars().take(120).collect(),
        });
    }

    /// Forget everything about one app (Settings "Reset permissions"):
    /// decisions back to Ask, network back to allowed. History is kept.
    pub fn reset(&mut self, app: &str) {
        self.apps.remove(app);
    }

    /// Apps whose network access is restricted.
    pub fn network_rules(&self) -> BTreeMap<String, NetAccess> {
        self.apps
            .iter()
            .filter(|(_, a)| a.network.restricted())
            .map(|(k, a)| (k.clone(), a.network))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_roundtrip() {
        for p in Permission::ALL {
            assert_eq!(Permission::parse(p.id()), Some(p));
            let j = serde_json::to_string(&p).unwrap();
            assert_eq!(j, format!("\"{}\"", p.id()));
        }
        for d in ["ask", "allow", "while_using", "deny"] {
            assert_eq!(Decision::parse(d).unwrap().id(), d);
        }
        assert_eq!(Permission::parse("camera"), Some(Permission::Camera));
        assert_eq!(Permission::parse("portal"), None);
        assert!(Permission::Microphone.is_media() && !Permission::Usb.is_media());
        assert_eq!(Permission::Location.prompt_options(), &["allow", "once", "deny"]);
        assert_eq!(Permission::Files.prompt_options(), &["allow", "while_using", "deny"]);
        assert_eq!(Answer::parse("once"), Answer::Once);
        assert!(Answer::Once.granted() && Answer::Once.remembered().is_none());
    }

    #[test]
    fn store_defaults_and_reset() {
        let mut s = Store::new();
        assert_eq!(s.decision("a", Permission::Usb), Decision::Ask);
        s.set_decision("a", Permission::Usb, Decision::WhileUsing, 10);
        assert_eq!(s.decision("a", Permission::Usb), Decision::WhileUsing);
        assert!(s.network_rules().is_empty());
        s.app("a", 11).network.set("mobile", false);
        assert_eq!(s.network_rules().len(), 1);
        let j = serde_json::to_string(&s).unwrap();
        let back: Store = serde_json::from_str(&j).unwrap();
        assert_eq!(back, s);
        s.reset("a");
        assert_eq!(s.decision("a", Permission::Usb), Decision::Ask);
        assert!(s.network_rules().is_empty());
    }

    #[test]
    fn history_is_bounded_and_collapses_polling() {
        let mut s = Store::new();
        for t in 0..10 {
            s.note("a", Permission::Sensors, true, "", 100 + t);
        }
        assert_eq!(s.history.len(), 1);
        assert_eq!(s.history[0].time, 109);
        assert_eq!(s.apps["a"].perms[&Permission::Sensors].uses, 10);
        for t in 0..(HISTORY_MAX as u64 + 50) {
            s.note(&format!("app{t}"), Permission::Usb, t % 2 == 0, "", 1000 + t);
        }
        assert_eq!(s.history.len(), HISTORY_MAX);
    }

    #[test]
    fn explicit_decisions() {
        let mut s = Store::new();
        // a use creates an entry, but that is not a decision
        s.note("a", Permission::Camera, true, "", 5);
        assert_eq!(s.explicit_decision("a", Permission::Camera), None);
        // choosing "ask" (the default value) is still the user's decision
        s.set_decision("a", Permission::Camera, Decision::Ask, 6);
        assert_eq!(s.explicit_decision("a", Permission::Camera), Some(Decision::Ask));
        // old store files: changed != 0 means decided
        let old: PermEntry = serde_json::from_str(r#"{"decision":"deny","changed":7}"#).unwrap();
        assert!(old.is_explicit());
        let old: PermEntry = serde_json::from_str(r#"{"decision":"ask","uses":3}"#).unwrap();
        assert!(!old.is_explicit());
    }

    #[test]
    fn net_access() {
        let mut n = NetAccess::default();
        assert!(!n.restricted());
        assert!(n.set("all", false));
        assert!(n.none());
        assert!(n.set("local", true));
        assert!(n.restricted() && !n.none());
        assert!(!n.set("bogus", true));
        // old store files without newer fields still load
        let n: NetAccess = serde_json::from_str(r#"{"wifi":false}"#).unwrap();
        assert!(!n.wifi && n.mobile && n.vpn && n.local);
    }
}
