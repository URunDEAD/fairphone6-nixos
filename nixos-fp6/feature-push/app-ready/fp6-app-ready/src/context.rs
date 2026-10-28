// SPDX-License-Identifier: MIT
//! Location-free usage context for the prediction model: local hour and
//! weekday/weekend, charging yes/no, Wi-Fi connected yes/no, headphones
//! (Bluetooth audio or USB audio) yes/no. Nothing else is read: no SSID,
//! no device names or addresses, no location. Only the yes/no bits reach the
//! model (predict.rs).

use std::path::Path;
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Context {
    /// Local hour 0..23.
    pub hour: u8,
    pub weekend: bool,
    pub charging: bool,
    pub wifi: bool,
    pub headphones: bool,
}

/// Local hour and weekend flag of a unix time (libc localtime_r: follows /etc/localtime, TZ).
pub fn local_time(unix: u64) -> (u8, bool) {
    let t = unix as libc::time_t;
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    if unsafe { libc::localtime_r(&t, &mut tm) }.is_null() {
        return (((unix / 3600) % 24) as u8, false);
    }
    (tm.tm_hour.clamp(0, 23) as u8, tm.tm_wday == 0 || tm.tm_wday == 6)
}

/// Charging: any external supply online (USB, mains, wireless), or the battery says Charging/Full.
pub fn charging(power_supply: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(power_supply) else { return false };
    for e in entries.flatten() {
        let p = e.path();
        let kind = std::fs::read_to_string(p.join("type")).unwrap_or_default();
        let kind = kind.trim();
        if kind == "Battery" {
            let status = std::fs::read_to_string(p.join("status")).unwrap_or_default();
            if matches!(status.trim(), "Charging" | "Full") {
                return true;
            }
        } else if matches!(kind, "USB" | "Mains" | "Wireless" | "USB_PD" | "USB_C")
            && std::fs::read_to_string(p.join("online")).is_ok_and(|v| v.trim() == "1")
        {
            return true;
        }
    }
    false
}

/// Wi-Fi connected: a wireless interface (has a `wireless` or `phy80211`
/// entry) whose operstate is "up". The network's name is never read.
pub fn wifi(net: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(net) else { return false };
    entries.flatten().any(|e| {
        let p = e.path();
        (p.join("wireless").exists() || p.join("phy80211").exists())
            && std::fs::read_to_string(p.join("operstate")).is_ok_and(|s| s.trim() == "up")
    })
}

/// A USB audio card (USB-C headset or dongle) in /proc/asound/cards.
pub fn usb_audio(cards: &str) -> bool {
    cards.lines().any(|l| l.contains("USB-Audio"))
}

/// BlueZ: a connected device offering an audio sink / headset profile
/// (A2DP sink 0000110b, HFP/HSP 0000111e / 00001108, LE audio 0000184e).
/// Only the yes/no answer is kept.
pub fn bluetooth_audio(system: &zbus::blocking::Connection) -> bool {
    use std::collections::HashMap;
    use zbus::zvariant::{OwnedObjectPath, OwnedValue};
    type Objects = HashMap<OwnedObjectPath, HashMap<String, HashMap<String, OwnedValue>>>;
    let Ok(reply) = system.call_method(
        Some("org.bluez"),
        "/",
        Some("org.freedesktop.DBus.ObjectManager"),
        "GetManagedObjects",
        &(),
    ) else {
        return false;
    };
    let Ok(objects) = reply.body().deserialize::<Objects>() else { return false };
    const AUDIO: [&str; 4] = ["0000110b-", "0000111e-", "00001108-", "0000184e-"];
    objects.values().filter_map(|ifaces| ifaces.get("org.bluez.Device1")).any(|dev| {
        let connected = dev.get("Connected").and_then(|v| bool::try_from(v).ok()).unwrap_or(false);
        let uuids: Vec<String> = dev.get("UUIDs").and_then(|v| v.try_clone().ok()).and_then(|v| Vec::<String>::try_from(v).ok()).unwrap_or_default();
        connected && uuids.iter().any(|u| AUDIO.iter().any(|a| u.to_ascii_lowercase().starts_with(a)))
    })
}

/// Reads the context; the Bluetooth answer is cached for a minute (one D-Bus call).
pub struct Reader {
    system: Option<zbus::blocking::Connection>,
    bt: Option<(Instant, bool)>,
}

impl Reader {
    pub fn new(system: Option<zbus::blocking::Connection>) -> Reader {
        Reader { system, bt: None }
    }

    pub fn read(&mut self, unix: u64) -> Context {
        let (hour, weekend) = local_time(unix);
        let headphones = usb_audio(&std::fs::read_to_string("/proc/asound/cards").unwrap_or_default()) || self.bluetooth();
        Context {
            hour,
            weekend,
            charging: charging(Path::new("/sys/class/power_supply")),
            wifi: wifi(Path::new("/sys/class/net")),
            headphones,
        }
    }

    fn bluetooth(&mut self) -> bool {
        if let Some((at, v)) = self.bt {
            if at.elapsed() < Duration::from_secs(60) {
                return v;
            }
        }
        let v = self.system.as_ref().is_some_and(bluetooth_audio);
        self.bt = Some((Instant::now(), v));
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("fp6-app-ready-ctx-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn power_supply() {
        let d = tmp("ps");
        std::fs::create_dir_all(d.join("battery")).unwrap();
        std::fs::write(d.join("battery/type"), "Battery\n").unwrap();
        std::fs::write(d.join("battery/status"), "Discharging\n").unwrap();
        std::fs::create_dir_all(d.join("usb")).unwrap();
        std::fs::write(d.join("usb/type"), "USB\n").unwrap();
        std::fs::write(d.join("usb/online"), "0\n").unwrap();
        assert!(!charging(&d));
        std::fs::write(d.join("usb/online"), "1\n").unwrap();
        assert!(charging(&d));
        std::fs::write(d.join("usb/online"), "0\n").unwrap();
        std::fs::write(d.join("battery/status"), "Full\n").unwrap();
        assert!(charging(&d));
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn wifi_operstate() {
        let d = tmp("net");
        std::fs::create_dir_all(d.join("wlan0/phy80211")).unwrap();
        std::fs::create_dir_all(d.join("usb0")).unwrap();
        std::fs::write(d.join("usb0/operstate"), "up\n").unwrap();
        std::fs::write(d.join("wlan0/operstate"), "dormant\n").unwrap();
        assert!(!wifi(&d));
        std::fs::write(d.join("wlan0/operstate"), "up\n").unwrap();
        assert!(wifi(&d));
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn usb_audio_cards() {
        assert!(!usb_audio(" 0 [milos          ]: milos - milos\n"));
        assert!(usb_audio(" 0 [milos ]: x\n 1 [Headset ]: USB-Audio - USB-C Headset\n"));
    }

    #[test]
    fn local_hour_range() {
        let (h, _) = local_time(1_793_176_600);
        assert!(h < 24);
    }
}
