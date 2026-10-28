// SPDX-License-Identifier: GPL-3.0-or-later
//! Enforcement files the broker writes from the store:
//!
//!  * AppArmor (confined apps): `/var/lib/fp6-permissions/apparmor/<profile>`,
//!    pulled into each FP6 profile through its `include if exists
//!    <local/<profile>>` hook. Grants what a passed descriptor needs (AppArmor
//!    re-checks descriptors an app receives) and denies data paths the user
//!    refused. Reloaded with `apparmor_parser --replace`, so it applies to
//!    running processes at once.
//!  * systemd drop-in (unconfined lomiri-app-launch apps, from the next app
//!    start): `RestrictAddressFamilies=~AF_BLUETOOTH AF_NFC` when the user
//!    denied Bluetooth / NFC.
//!  * `/run/fp6-permissions/network.json`, read by the netpolicy helper
//!    (nftables per app cgroup; netpolicy/DESIGN.md "App network permission").

use crate::model::{Decision, NetAccess, Permission};
use serde::Serialize;
use std::collections::BTreeMap;

pub type Decisions = BTreeMap<Permission, Decision>;

fn granted(d: &Decisions, p: Permission) -> bool {
    matches!(d.get(&p), Some(Decision::Allow | Decision::WhileUsing))
}
fn denied(d: &Decisions, p: Permission) -> bool {
    matches!(d.get(&p), Some(Decision::Deny))
}

/// Data paths behind each data permission (refused for confined apps when
/// denied; the services that own the data are reached over D-Bus, which
/// dbus-broker does not mediate here).
pub const DATA_PATHS: &[(Permission, &[&str])] = &[
    (Permission::Contacts, &[
        "@{HOME}/.local/share/evolution/addressbook/**",
        "@{HOME}/.local/share/address-book-service/**",
        "@{HOME}/.cache/evolution/addressbook/**",
    ]),
    (Permission::Calendar, &[
        "@{HOME}/.local/share/evolution/calendar/**",
        "@{HOME}/.local/share/evolution/tasks/**",
        "@{HOME}/.cache/evolution/calendar/**",
    ]),
    (Permission::Messages, &[
        "@{HOME}/.local/share/history-service/**",
        "@{HOME}/.local/share/TpLogger/**",
        "@{HOME}/.local/share/nuntium/**",
    ]),
    (Permission::Files, &[
        "@{HOME}/Documents/**",
        "@{HOME}/Downloads/**",
        "@{HOME}/Music/**",
        "@{HOME}/Pictures/**",
        "@{HOME}/Videos/**",
        "/media/**",
        "/run/media/**",
        "/mnt/**",
    ]),
    (Permission::Sensors, &[
        "/sys/bus/iio/devices/**",
        "/sys/devices/**/iio:device*/**",
        "/dev/iio:device*",
    ]),
];

/// AppArmor include for one confined app. Empty string = nothing to add.
pub fn apparmor_include(profile: &str, d: &Decisions, net: &NetAccess) -> String {
    let mut out = String::new();
    if granted(d, Permission::Usb) {
        out += "  # USB: descriptors handed out by fp6-permd (DAC still keeps the app from opening nodes itself)\n";
        out += "  /dev/bus/usb/[0-9][0-9][0-9]/[0-9][0-9][0-9] rw,\n";
        out += "  /dev/hidraw[0-9]* rw,\n";
        out += "  /dev/ttyACM[0-9]* rw,\n";
        out += "  /dev/ttyUSB[0-9]* rw,\n";
    }
    if granted(d, Permission::Bluetooth) {
        out += "  # Bluetooth: L2CAP/RFCOMM sockets created by fp6-permd\n";
        out += "  network bluetooth stream,\n  network bluetooth seqpacket,\n";
    } else if denied(d, Permission::Bluetooth) {
        out += "  deny network bluetooth,\n";
    }
    if granted(d, Permission::Nfc) {
        out += "  network nfc,\n";
    } else if denied(d, Permission::Nfc) {
        out += "  deny network nfc,\n";
    }
    if denied(d, Permission::Camera) {
        out += "  # Camera denied in Settings > Privacy > App permissions\n";
        out += "  deny /dev/video[0-9]* rw,\n  deny /dev/media[0-9]* rw,\n  deny /dev/v4l-subdev[0-9]* rw,\n";
    }
    if granted(d, Permission::Files) {
        out += "  # Files & media: read\n";
        out += "  @{HOME}/{Documents,Downloads,Music,Pictures,Videos}/{,**} r,\n";
        out += "  /{media,run/media}/{,**} r,\n";
    }
    for (perm, paths) in DATA_PATHS {
        if denied(d, *perm) {
            out += &format!("  # {} denied in Settings > Privacy > App permissions\n", perm.title());
            for p in *paths {
                out += &format!("  deny {p} rwkl,\n");
            }
        }
    }
    if net.none() {
        out += "  # Network denied (nftables drops the traffic as well)\n";
        out += "  deny network inet,\n  deny network inet6,\n  deny network packet,\n";
    }
    if out.is_empty() {
        return out;
    }
    format!("  # fp6-permd: runtime permissions of {profile} (generated, do not edit)\n{out}")
}

/// systemd drop-in for an unconfined lomiri-app-launch app, or None.
pub fn systemd_dropin(d: &Decisions) -> Option<String> {
    let mut fams = vec![];
    if denied(d, Permission::Bluetooth) {
        fams.push("AF_BLUETOOTH");
    }
    if denied(d, Permission::Nfc) {
        fams.push("AF_NFC");
    }
    if fams.is_empty() {
        return None;
    }
    Some(format!(
        "# fp6-permd: permissions denied in Settings (generated, applies from the next start)\n[Service]\nRestrictAddressFamilies=~{}\n",
        fams.join(" ")
    ))
}

/// Drop-in directory name covering every instance of a lomiri-app-launch
/// app: systemd also reads `<prefix>-.service.d` for `<prefix>-<x>.service`,
/// and the instance is the last dash-separated part of the unit name.
pub fn dropin_dir(app_id: &str) -> Option<String> {
    if !crate::appid::valid_app_id(app_id) {
        return None;
    }
    // Same escaping as lomiri-app-launch's escapeUnitComponent for ids we accept.
    Some(format!("lomiri-app-launch--application-legacy--{app_id}--.service.d"))
}

#[derive(Serialize)]
pub struct NetworkFile<'a> {
    pub version: u32,
    pub uid: u32,
    pub apps: &'a BTreeMap<String, NetAccess>,
}

/// The netpolicy contract file (only restricted apps are listed).
pub fn network_json(uid: u32, rules: &BTreeMap<String, NetAccess>) -> String {
    serde_json::to_string_pretty(&NetworkFile { version: 1, uid, apps: rules }).unwrap() + "\n"
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dec(pairs: &[(Permission, Decision)]) -> Decisions {
        pairs.iter().copied().collect()
    }

    #[test]
    fn nothing_by_default() {
        assert_eq!(apparmor_include("x", &Decisions::new(), &NetAccess::default()), "");
        assert_eq!(systemd_dropin(&Decisions::new()), None);
        // Ask adds nothing either
        let d = dec(&[(Permission::Usb, Decision::Ask), (Permission::Files, Decision::Ask)]);
        assert_eq!(apparmor_include("x", &d, &NetAccess::default()), "");
    }

    #[test]
    fn grants_and_denials() {
        let d = dec(&[
            (Permission::Usb, Decision::WhileUsing),
            (Permission::Bluetooth, Decision::Deny),
            (Permission::Contacts, Decision::Deny),
        ]);
        let inc = apparmor_include("morph-browser", &d, &NetAccess::default());
        assert!(inc.contains("/dev/bus/usb/[0-9][0-9][0-9]/[0-9][0-9][0-9] rw,"));
        assert!(inc.contains("deny network bluetooth,"));
        assert!(inc.contains("deny @{HOME}/.local/share/evolution/addressbook/** rwkl,"));
        assert!(!inc.contains("calendar"));
        assert!(!inc.contains("network inet"));
        assert!(!inc.contains("/dev/video"));
        let cam = apparmor_include("x", &dec(&[(Permission::Camera, Decision::Deny)]), &NetAccess::default());
        assert!(cam.contains("deny /dev/video[0-9]* rw,") && cam.contains("deny /dev/media[0-9]* rw,"));
        // allowing the camera or the microphone adds nothing (profiles and
        // PipeWire decide)
        let ok = dec(&[(Permission::Camera, Decision::Allow), (Permission::Microphone, Decision::Deny), (Permission::Location, Decision::Deny)]);
        assert_eq!(apparmor_include("x", &ok, &NetAccess::default()), "");
        assert_eq!(
            systemd_dropin(&d).unwrap().lines().last(),
            Some("RestrictAddressFamilies=~AF_BLUETOOTH")
        );
        let mut none = NetAccess::default();
        none.set("all", false);
        assert!(apparmor_include("x", &Decisions::new(), &none).contains("deny network inet6,"));
        // partial network denial is nftables-only
        let mut part = NetAccess::default();
        part.set("mobile", false);
        assert_eq!(apparmor_include("x", &Decisions::new(), &part), "");
    }

    #[test]
    fn every_rule_line_is_terminated() {
        let all: Decisions = Permission::ALL.iter().map(|p| (*p, Decision::Deny)).collect();
        let mut none = NetAccess::default();
        none.set("all", false);
        for line in apparmor_include("x", &all, &none).lines() {
            let l = line.trim();
            assert!(l.starts_with('#') || l.ends_with(','), "{l}");
        }
        let all: Decisions = Permission::ALL.iter().map(|p| (*p, Decision::Allow)).collect();
        for line in apparmor_include("x", &all, &NetAccess::default()).lines() {
            let l = line.trim();
            assert!(l.starts_with('#') || l.ends_with(','), "{l}");
        }
    }

    #[test]
    fn dropin_names() {
        assert_eq!(
            dropin_dir("morph-browser").unwrap(),
            "lomiri-app-launch--application-legacy--morph-browser--.service.d"
        );
        assert_eq!(dropin_dir("../x"), None);
        assert_eq!(dropin_dir("a b"), None);
    }

    #[test]
    fn network_file() {
        let mut m = BTreeMap::new();
        let mut n = NetAccess::default();
        n.set("wifi", false);
        m.insert("chat".to_string(), n);
        let j: serde_json::Value = serde_json::from_str(&network_json(10000, &m)).unwrap();
        assert_eq!(j["version"], 1);
        assert_eq!(j["uid"], 10000);
        assert_eq!(j["apps"]["chat"]["wifi"], false);
        assert_eq!(j["apps"]["chat"]["mobile"], true);
    }
}
