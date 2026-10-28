//! Read-only view of the power_supply class (qcom_battmgr on the FP6:
//! qcom-battmgr-bat / -usb / -wls, plus the UCSI source supply for the
//! USB-C PD contract). Nothing here writes to sysfs.

use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Snapshot {
    pub capacity: i32,
    /// "Charging", "Discharging", "Not charging", "Full", "Unknown".
    pub status: String,
    /// Battery current in µA, positive into the battery (battmgr sign).
    pub current_ua: i64,
    pub voltage_uv: i64,
    pub charge_now_uah: i64,
    pub charge_full_uah: i64,
    /// Firmware time_to_full_avg (s), 0 or negative when unknown.
    pub ttf_fw: i64,
    pub usb_online: bool,
    pub usb_type: String,
    pub usb_uv: i64,
    pub usb_ua: i64,
    pub usb_max_ua: i64,
    /// UCSI: "PD" / "PD_PPS" once a contract is negotiated, else "C" or "".
    pub typec_type: String,
    pub typec_max_uv: i64,
    pub typec_max_ua: i64,
    pub wls_online: bool,
    pub wls_uv: i64,
    pub wls_ua: i64,
}

impl Snapshot {
    pub fn charge_mw(&self) -> i64 {
        if self.current_ua > 0 {
            self.current_ua * self.voltage_uv.max(0) / 1_000_000_000
        } else {
            0
        }
    }
    pub fn input_mw(&self) -> i64 {
        if self.wls_online {
            self.wls_ua.max(0) * self.wls_uv.max(0) / 1_000_000_000
        } else if self.usb_online {
            self.usb_ua.max(0) * self.usb_uv.max(0) / 1_000_000_000
        } else {
            0
        }
    }
    pub fn pd(&self) -> bool {
        matches!(self.typec_type.as_str(), "PD" | "PD_PPS") || matches!(self.usb_type.as_str(), "PD" | "PD_PPS" | "PD_DRP")
    }
    /// What the charger offers (mW): the PD contract, else 5 V × current_max.
    pub fn capability_mw(&self) -> i64 {
        if self.wls_online {
            return 0;
        }
        if self.pd() && self.typec_max_uv > 0 && self.typec_max_ua > 0 {
            return self.typec_max_uv * self.typec_max_ua / 1_000_000_000;
        }
        if self.usb_max_ua > 0 {
            return self.usb_max_ua * 5_000 / 1_000_000;
        }
        0
    }
    /// "USB PD", "USB charger", "USB port", "Wireless", "USB".
    pub fn charger(&self) -> &'static str {
        if self.wls_online {
            return "Wireless";
        }
        if !self.usb_online {
            return "";
        }
        if self.pd() {
            return if self.typec_type == "PD_PPS" || self.usb_type == "PD_PPS" { "USB PD (PPS)" } else { "USB PD" };
        }
        match self.usb_type.as_str() {
            "SDP" => "USB port",
            "DCP" => "USB charger",
            "CDP" => "USB charging port",
            _ => "USB",
        }
    }
}

/// The bracketed entry of a usb_type list: "Unknown [SDP] DCP" -> "SDP".
pub fn active_choice(list: &str) -> String {
    list.split_whitespace()
        .find(|w| w.starts_with('[') && w.ends_with(']'))
        .map(|w| w.trim_matches(|c| c == '[' || c == ']').to_string())
        .unwrap_or_else(|| list.trim().to_string())
}

fn read(dir: &Path, name: &str) -> Option<String> {
    fs::read_to_string(dir.join(name)).ok().map(|s| s.trim().to_string())
}
fn int(dir: &Path, name: &str) -> i64 {
    read(dir, name).and_then(|s| s.parse().ok()).unwrap_or(0)
}

#[derive(Clone, Debug, Default)]
pub struct Supply {
    pub bat: Option<PathBuf>,
    pub usb: Option<PathBuf>,
    pub wls: Option<PathBuf>,
    pub typec: Option<PathBuf>,
}

impl Supply {
    pub fn discover(root: &Path) -> Supply {
        let mut s = Supply::default();
        let Ok(dir) = fs::read_dir(root) else { return s };
        let mut entries: Vec<_> = dir.flatten().collect();
        entries.sort_by_key(|e| e.file_name());
        for e in entries {
            let p = e.path();
            let name = e.file_name().to_string_lossy().to_string();
            let ty = read(&p, "type").unwrap_or_default();
            match ty.as_str() {
                "Battery" if s.bat.is_none() || name.contains("battmgr") => s.bat = Some(p),
                "USB" if name.starts_with("ucsi") => s.typec = Some(p),
                "USB" | "USB_PD" | "USB_C" | "Mains" if s.usb.is_none() => s.usb = Some(p),
                "Wireless" => s.wls = Some(p),
                _ => {}
            }
        }
        s
    }

    pub fn found(&self) -> bool {
        self.bat.is_some()
    }

    pub fn read(&self) -> Snapshot {
        let mut r = Snapshot::default();
        if let Some(b) = &self.bat {
            r.capacity = int(b, "capacity") as i32;
            r.status = read(b, "status").unwrap_or_default();
            r.current_ua = int(b, "current_now");
            r.voltage_uv = int(b, "voltage_now");
            r.charge_now_uah = int(b, "charge_now").max(int(b, "charge_counter"));
            r.charge_full_uah = int(b, "charge_full");
            r.ttf_fw = int(b, "time_to_full_avg").max(int(b, "time_to_full_now"));
        }
        if let Some(u) = &self.usb {
            r.usb_online = int(u, "online") == 1;
            r.usb_type = active_choice(&read(u, "usb_type").unwrap_or_default());
            r.usb_uv = int(u, "voltage_now");
            r.usb_ua = int(u, "current_now");
            r.usb_max_ua = int(u, "current_max");
        }
        if let Some(t) = &self.typec {
            if int(t, "online") == 1 {
                r.typec_type = active_choice(&read(t, "usb_type").unwrap_or_default());
                r.typec_max_uv = int(t, "voltage_max");
                r.typec_max_ua = int(t, "current_max");
            }
        }
        if let Some(w) = &self.wls {
            r.wls_online = int(w, "online") == 1;
            r.wls_uv = int(w, "voltage_now");
            r.wls_ua = int(w, "current_now");
        }
        r
    }
}

#[cfg(test)]
pub mod fake {
    use std::fs;
    use std::path::{Path, PathBuf};

    pub fn dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("fp6-cf-test-{}-{tag}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    pub fn put(root: &Path, supply: &str, attrs: &[(&str, &str)]) {
        let d = root.join(supply);
        fs::create_dir_all(&d).unwrap();
        for (k, v) in attrs {
            fs::write(d.join(k), format!("{v}\n")).unwrap();
        }
    }

    /// FP6-like tree: battery 45 %, USB offline.
    pub fn fp6(tag: &str) -> PathBuf {
        let r = dir(tag);
        put(&r, "qcom-battmgr-bat", &[("type", "Battery"), ("capacity", "45"), ("status", "Discharging"),
            ("current_now", "-300000"), ("voltage_now", "3900000"), ("charge_now", "2100000"),
            ("charge_full", "4747000"), ("time_to_full_avg", "0")]);
        put(&r, "qcom-battmgr-usb", &[("type", "USB"), ("online", "0"), ("usb_type", "[Unknown] SDP DCP CDP"),
            ("voltage_now", "0"), ("current_now", "0"), ("current_max", "0")]);
        put(&r, "qcom-battmgr-wls", &[("type", "Wireless"), ("online", "0")]);
        put(&r, "ucsi-source-psy-pmic_glink.ucsi.01", &[("type", "USB"), ("online", "0"), ("usb_type", "[C] PD PD_PPS"),
            ("voltage_max", "5000000"), ("current_max", "3000000")]);
        r
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discovers_fp6_tree_and_reads() {
        let r = fake::fp6("discover");
        let s = Supply::discover(&r);
        assert!(s.bat.as_ref().unwrap().ends_with("qcom-battmgr-bat"));
        assert!(s.usb.as_ref().unwrap().ends_with("qcom-battmgr-usb"));
        assert!(s.typec.as_ref().unwrap().ends_with("ucsi-source-psy-pmic_glink.ucsi.01"));
        assert!(s.wls.is_some());
        let snap = s.read();
        assert_eq!(snap.capacity, 45);
        assert!(!snap.usb_online && !snap.wls_online);
        assert_eq!(snap.usb_type, "Unknown");
        assert_eq!(snap.typec_type, "");

        fake::put(&r, "qcom-battmgr-usb", &[("online", "1"), ("usb_type", "Unknown [SDP] DCP"), ("voltage_now", "4680000"),
            ("current_now", "637000"), ("current_max", "1500000")]);
        let snap = s.read();
        assert!(snap.usb_online);
        assert_eq!(snap.charger(), "USB port");
        assert_eq!(snap.input_mw(), 2981);
        assert_eq!(snap.capability_mw(), 7500);
        let _ = fs::remove_dir_all(r);
    }

    #[test]
    fn pd_capability() {
        let snap = Snapshot { usb_online: true, typec_type: "PD".into(), typec_max_uv: 9_000_000, typec_max_ua: 3_000_000,
                              ..Default::default() };
        assert!(snap.pd());
        assert_eq!(snap.capability_mw(), 27_000);
        assert_eq!(snap.charger(), "USB PD");
    }
}
