//! qcom_battmgr power_supply class (FP6, kernel6): reading and the only write
//! this service ever does to the charger, the charge-control thresholds.
//!
//! Kernel6 qcom_battmgr (SM8550 variant) exposes, on qcom-battmgr-bat,
//! charge_control_start_threshold (50..95) and charge_control_end_threshold
//! (55..100). A write sends BATTMGR_CHG_CTRL_LIMIT_EN {enable=1, target_soc,
//! delta_soc} over PMIC GLINK to the ADSP battery manager; nothing is stored
//! in NV (the driver's nvmem cells are only read at probe). The firmware's
//! boot state reports 70/80 but is not enabled (it charges to 100 %).
//! There is no way to send enable=0 from userspace: "off" is end 100 with
//! start 95 (charge to 100 %, recharge below 95 %), and the service only
//! writes once a limit has been used since boot (see charge.rs).

use std::fs;
use std::path::{Path, PathBuf};

pub const END_MIN: u32 = 55;
pub const END_MAX: u32 = 100;
pub const START_MIN: u32 = 50;
pub const START_MAX: u32 = 95;
/// Recharge hysteresis used for every limit (the driver's own default delta).
pub const HYSTERESIS: u32 = 5;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Reading {
    pub time: i64,
    pub capacity: i32,
    pub status: String,
    /// Battery current in µA, positive while charging (battmgr sign).
    pub current_ua: i64,
    pub voltage_uv: i64,
    pub voltage_ocv_uv: i64,
    pub charge_now_uah: i64,
    pub charge_full_uah: i64,
    pub charge_full_design_uah: i64,
    pub cycle_count: i64,
    pub temp_dc: i64,
    pub soh: i64,
    pub resistance_uohm: i64,
    pub health: String,
    pub technology: String,
    pub model: String,
    pub ttf_s: i64,
    pub tte_s: i64,
    pub start_thr: Option<u32>,
    pub end_thr: Option<u32>,
    pub usb_online: bool,
    pub usb_type: String,
    pub usb_uv: i64,
    pub usb_ua: i64,
    pub usb_max_ua: i64,
    pub typec_type: String,
    pub typec_max_uv: i64,
    pub typec_max_ua: i64,
    pub wls_online: bool,
}

impl Reading {
    pub fn plugged(&self) -> bool {
        self.usb_online || self.wls_online
    }
    pub fn charging(&self) -> bool {
        self.status == "Charging"
    }
    /// Battery charge power (mW), 0 unless current flows into the battery.
    pub fn charge_mw(&self) -> i64 {
        if self.current_ua > 0 {
            self.current_ua * self.voltage_uv / 1_000_000_000
        } else {
            0
        }
    }
    /// Power drawn from the charger input (mW).
    pub fn input_mw(&self) -> i64 {
        if self.usb_online {
            self.usb_ua.max(0) * self.usb_uv.max(0) / 1_000_000_000
        } else {
            0
        }
    }
    /// Human charger description: "USB PD", "USB PD PPS", "USB charger (DCP)",
    /// "USB port (SDP)", "Wireless".
    pub fn charger(&self) -> String {
        if self.wls_online {
            return "Wireless".into();
        }
        if !self.usb_online {
            return String::new();
        }
        match self.typec_type.as_str() {
            "PD" => return "USB PD".into(),
            "PD_PPS" => return "USB PD (PPS)".into(),
            _ => {}
        }
        match self.usb_type.as_str() {
            "SDP" => "USB port (SDP)".into(),
            "DCP" => "USB charger (DCP)".into(),
            "CDP" => "USB charging port (CDP)".into(),
            "PD" | "PD_DRP" => "USB PD".into(),
            "PD_PPS" => "USB PD (PPS)".into(),
            "C" => "USB-C".into(),
            "" | "Unknown" => "USB".into(),
            other => format!("USB ({other})"),
        }
    }
}

/// The bracketed entry of a power_supply usb_type list: "Unknown [SDP] DCP" -> "SDP".
pub fn active_choice(list: &str) -> String {
    list.split_whitespace()
        .find(|w| w.starts_with('[') && w.ends_with(']'))
        .map(|w| w.trim_matches(|c| c == '[' || c == ']').to_string())
        .unwrap_or_else(|| list.trim().to_string())
}

#[derive(Clone, Debug)]
pub struct Supply {
    pub bat: PathBuf,
    pub usb: Option<PathBuf>,
    pub wls: Option<PathBuf>,
    pub typec: Option<PathBuf>,
}

fn read(dir: &Path, name: &str) -> Option<String> {
    fs::read_to_string(dir.join(name)).ok().map(|s| s.trim().to_string())
}
fn int(dir: &Path, name: &str) -> i64 {
    read(dir, name).and_then(|s| s.parse().ok()).unwrap_or(0)
}

impl Supply {
    /// Finds the battmgr battery/usb/wireless supplies and the UCSI Type-C source.
    pub fn discover(root: &Path) -> Option<Supply> {
        let mut s = Supply { bat: PathBuf::new(), usb: None, wls: None, typec: None };
        let mut found = false;
        for e in fs::read_dir(root).ok()?.flatten() {
            let p = e.path();
            let name = e.file_name().to_string_lossy().to_string();
            let ty = read(&p, "type").unwrap_or_default();
            match (ty.as_str(), name.as_str()) {
                ("Battery", _) if name.contains("battmgr") || !found => {
                    s.bat = p;
                    found = true;
                }
                ("USB", n) if n.starts_with("ucsi") => s.typec = Some(p),
                ("USB", _) => s.usb = Some(p),
                ("Wireless", _) => s.wls = Some(p),
                _ => {}
            }
        }
        found.then_some(s)
    }

    pub fn read(&self, time: i64) -> Reading {
        let b = &self.bat;
        let mut r = Reading {
            time,
            capacity: int(b, "capacity") as i32,
            status: read(b, "status").unwrap_or_default(),
            current_ua: int(b, "current_now"),
            voltage_uv: int(b, "voltage_now"),
            voltage_ocv_uv: int(b, "voltage_ocv"),
            charge_now_uah: int(b, "charge_now").max(int(b, "charge_counter")),
            charge_full_uah: int(b, "charge_full"),
            charge_full_design_uah: int(b, "charge_full_design"),
            cycle_count: int(b, "cycle_count"),
            temp_dc: int(b, "temp"),
            soh: int(b, "state_of_health"),
            resistance_uohm: int(b, "internal_resistance"),
            health: read(b, "health").unwrap_or_default(),
            technology: read(b, "technology").unwrap_or_default(),
            model: read(b, "model_name").unwrap_or_default(),
            ttf_s: int(b, "time_to_full_avg"),
            tte_s: int(b, "time_to_empty_avg"),
            start_thr: read(b, "charge_control_start_threshold").and_then(|s| s.parse().ok()),
            end_thr: read(b, "charge_control_end_threshold").and_then(|s| s.parse().ok()),
            ..Default::default()
        };
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
        }
        r
    }

    /// (start, end) as the firmware reports them now.
    pub fn thresholds(&self) -> (Option<u32>, Option<u32>) {
        let b = &self.bat;
        (read(b, "charge_control_start_threshold").and_then(|s| s.parse().ok()),
         read(b, "charge_control_end_threshold").and_then(|s| s.parse().ok()))
    }

    pub fn has_charge_control(&self) -> bool {
        self.bat.join("charge_control_end_threshold").exists()
            && self.bat.join("charge_control_start_threshold").exists()
    }

    /// Sets end/start = (end, end - HYSTERESIS). Only values inside the
    /// driver's accepted ranges are ever written; the readback must match.
    pub fn set_limit(&self, end: u32) -> Result<(), String> {
        let (end, start) = limit_pair(end).ok_or_else(|| format!("unsupported charge limit {end}"))?;
        let cur_end = read(&self.bat, "charge_control_end_threshold").and_then(|s| s.parse::<u32>().ok());
        let cur_start = read(&self.bat, "charge_control_start_threshold").and_then(|s| s.parse::<u32>().ok());
        if cur_end == Some(end) && cur_start == Some(start) {
            return Ok(());
        }
        // End first: the driver keeps start = end - delta. Then start, which
        // (start <= end) sets delta = end - start without moving end.
        write(&self.bat.join("charge_control_end_threshold"), end)?;
        write(&self.bat.join("charge_control_start_threshold"), start)?;
        let e = read(&self.bat, "charge_control_end_threshold").and_then(|s| s.parse::<u32>().ok());
        let s = read(&self.bat, "charge_control_start_threshold").and_then(|s| s.parse::<u32>().ok());
        if e != Some(end) || s != Some(start) {
            return Err(format!("threshold readback {s:?}/{e:?}, wanted {start}/{end}"));
        }
        Ok(())
    }
}

/// (end, start) for a requested limit, or None if the driver would clamp it.
pub fn limit_pair(end: u32) -> Option<(u32, u32)> {
    if !(END_MIN..=END_MAX).contains(&end) {
        return None;
    }
    let start = end - HYSTERESIS;
    (START_MIN..=START_MAX).contains(&start).then_some((end, start))
}

fn write(path: &Path, value: u32) -> Result<(), String> {
    fs::write(path, format!("{value}\n")).map_err(|e| format!("{}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn choices() {
        assert_eq!(active_choice("Unknown [SDP] DCP CDP ACA C PD PD_DRP PD_PPS BrickID"), "SDP");
        assert_eq!(active_choice("[C] PD PD_PPS"), "C");
        assert_eq!(active_choice("C [PD] PD_PPS"), "PD");
    }

    #[test]
    fn limits_stay_in_driver_range() {
        assert_eq!(limit_pair(100), Some((100, 95)));
        assert_eq!(limit_pair(80), Some((80, 75)));
        assert_eq!(limit_pair(55), Some((55, 50)));
        assert_eq!(limit_pair(54), None);
        assert_eq!(limit_pair(101), None);
        assert_eq!(limit_pair(0), None);
    }

    #[test]
    fn fake_sysfs() {
        let dir = std::env::temp_dir().join(format!("fp6-batt-test-{}", std::process::id()));
        let bat = dir.join("qcom-battmgr-bat");
        let usb = dir.join("qcom-battmgr-usb");
        let ucsi = dir.join("ucsi-source-psy-pmic_glink.ucsi.01");
        for d in [&bat, &usb, &ucsi] {
            fs::create_dir_all(d).unwrap();
        }
        for (f, v) in [("type", "Battery"), ("capacity", "77"), ("status", "Charging"), ("current_now", "1000000"),
                       ("voltage_now", "4000000"), ("charge_counter", "3000000"), ("charge_full", "4747000"),
                       ("charge_control_start_threshold", "70"), ("charge_control_end_threshold", "80")] {
            fs::write(bat.join(f), format!("{v}\n")).unwrap();
        }
        for (f, v) in [("type", "USB"), ("online", "1"), ("usb_type", "Unknown [DCP] SDP"), ("voltage_now", "5000000"), ("current_now", "1200000")] {
            fs::write(usb.join(f), format!("{v}\n")).unwrap();
        }
        for (f, v) in [("type", "USB"), ("online", "1"), ("usb_type", "C [PD] PD_PPS"), ("voltage_max", "9000000")] {
            fs::write(ucsi.join(f), format!("{v}\n")).unwrap();
        }
        let s = Supply::discover(&dir).unwrap();
        let r = s.read(1);
        assert_eq!(r.capacity, 77);
        assert!(r.plugged() && r.charging());
        assert_eq!(r.charge_mw(), 4000);
        assert_eq!(r.input_mw(), 6000);
        assert_eq!(r.charger(), "USB PD");
        s.set_limit(85).unwrap();
        assert_eq!(fs::read_to_string(bat.join("charge_control_end_threshold")).unwrap().trim(), "85");
        assert_eq!(fs::read_to_string(bat.join("charge_control_start_threshold")).unwrap().trim(), "80");
        assert!(s.set_limit(30).is_err());
        assert_eq!(fs::read_to_string(bat.join("charge_control_end_threshold")).unwrap().trim(), "85");
        fs::remove_dir_all(&dir).unwrap();
    }
}
