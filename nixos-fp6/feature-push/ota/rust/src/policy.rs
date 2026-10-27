//! When checks, downloads and installs may run.
//!
//! * Battery saver (/run/fp6-battery/saver = "1") blocks every download and
//!   every automatic action. A manual "Check now" still works.
//! * "Only on Wi-Fi": downloads and automatic checks need Wi-Fi or Ethernet
//!   that NetworkManager does not consider metered.
//! * "Only while charging": automatic downloads need a charger. A manual
//!   download does not (the page says so).
//! * Installing needs 30 % battery or a charger; "tonight" needs a charger,
//!   20 % battery and the night window (02:00-05:00 local).

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Default)]
#[serde(rename_all = "lowercase")]
pub enum Network {
    Wifi,
    Ethernet,
    Cellular,
    Vpn,
    Other,
    #[default]
    None,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct Conditions {
    pub network: Network,
    pub metered: bool,
    pub charging: bool,
    pub saver: bool,
    /// Battery percentage, if known.
    pub battery: Option<u8>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub auto_check: bool,
    pub auto_download: bool,
    pub wifi_only: bool,
    pub charging_only: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings { auto_check: true, auto_download: false, wifi_only: true, charging_only: true }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Purpose {
    ManualCheck,
    AutoCheck,
    ManualDownload,
    AutoDownload,
    InstallNow,
    InstallTonight,
}

pub const NIGHT_START: u32 = 2;
pub const NIGHT_END: u32 = 5;
pub const INSTALL_MIN_BATTERY: u8 = 30;
pub const NIGHT_MIN_BATTERY: u8 = 20;
/// Automatic checks at most this often.
pub const AUTO_CHECK_INTERVAL: i64 = 20 * 3600;

fn unmetered_fixed(c: &Conditions) -> bool {
    matches!(c.network, Network::Wifi | Network::Ethernet) && !c.metered
}

/// Ok(()) or the reasons (user-facing) why not.
pub fn allowed(s: &Settings, c: &Conditions, p: Purpose, local_hour: u32) -> Result<(), Vec<String>> {
    use Purpose::*;
    let mut why = Vec::new();
    let needs_net = matches!(p, ManualCheck | AutoCheck | ManualDownload | AutoDownload);
    if needs_net && c.network == Network::None {
        why.push("No network connection".to_string());
    }
    if c.saver && !matches!(p, ManualCheck | InstallNow) {
        why.push("Battery saver is on".to_string());
    }
    if s.wifi_only && matches!(p, AutoCheck | ManualDownload | AutoDownload) && c.network != Network::None && !unmetered_fixed(c) {
        why.push(if c.metered && matches!(c.network, Network::Wifi | Network::Ethernet) {
            "This Wi-Fi network is metered".to_string()
        } else {
            "Not on Wi-Fi".to_string()
        });
    }
    if matches!(p, AutoCheck | AutoDownload) {
        let enabled = if p == AutoCheck { s.auto_check } else { s.auto_download };
        if !enabled {
            why.push("Turned off in the settings".to_string());
        }
    }
    if p == AutoDownload && s.charging_only && !c.charging {
        why.push("Not charging".to_string());
    }
    if p == InstallNow && !c.charging && c.battery.is_some_and(|b| b < INSTALL_MIN_BATTERY) {
        why.push(format!("Battery below {INSTALL_MIN_BATTERY} %: connect a charger"));
    }
    if p == InstallTonight {
        if !c.charging {
            why.push("Not charging".to_string());
        }
        if c.battery.is_some_and(|b| b < NIGHT_MIN_BATTERY) {
            why.push(format!("Battery below {NIGHT_MIN_BATTERY} %"));
        }
        if !(NIGHT_START..NIGHT_END).contains(&local_hour) {
            why.push("Outside the night window (02:00-05:00)".to_string());
        }
    }
    if why.is_empty() { Ok(()) } else { Err(why) }
}

pub fn auto_check_due(last_check: Option<i64>, now: i64) -> bool {
    last_check.is_none_or(|t| now - t >= AUTO_CHECK_INTERVAL || t > now)
}

#[cfg(test)]
mod tests {
    use super::*;
    use Purpose::*;

    fn wifi() -> Conditions {
        Conditions { network: Network::Wifi, metered: false, charging: false, saver: false, battery: Some(80) }
    }

    #[test]
    fn checks() {
        let s = Settings::default();
        assert!(allowed(&s, &wifi(), ManualCheck, 12).is_ok());
        let cell = Conditions { network: Network::Cellular, ..wifi() };
        assert!(allowed(&s, &cell, ManualCheck, 12).is_ok(), "a manual check is small: any network");
        assert_eq!(allowed(&s, &cell, AutoCheck, 12).unwrap_err(), vec!["Not on Wi-Fi"]);
        let off = Conditions { network: Network::None, ..wifi() };
        assert_eq!(allowed(&s, &off, ManualCheck, 12).unwrap_err(), vec!["No network connection"]);
        let saver = Conditions { saver: true, ..wifi() };
        assert!(allowed(&s, &saver, ManualCheck, 12).is_ok());
        assert_eq!(allowed(&s, &saver, AutoCheck, 12).unwrap_err(), vec!["Battery saver is on"]);
        let no_auto = Settings { auto_check: false, ..s.clone() };
        assert_eq!(allowed(&no_auto, &wifi(), AutoCheck, 12).unwrap_err(), vec!["Turned off in the settings"]);
    }

    #[test]
    fn downloads() {
        let s = Settings { auto_download: true, ..Settings::default() };
        assert!(allowed(&s, &wifi(), ManualDownload, 12).is_ok(), "manual download ignores charging-only");
        assert_eq!(allowed(&s, &wifi(), AutoDownload, 12).unwrap_err(), vec!["Not charging"]);
        let charging = Conditions { charging: true, ..wifi() };
        assert!(allowed(&s, &charging, AutoDownload, 12).is_ok());
        let metered = Conditions { metered: true, ..charging.clone() };
        assert_eq!(allowed(&s, &metered, AutoDownload, 12).unwrap_err(), vec!["This Wi-Fi network is metered"]);
        let cell = Conditions { network: Network::Cellular, ..charging.clone() };
        assert_eq!(allowed(&s, &cell, ManualDownload, 12).unwrap_err(), vec!["Not on Wi-Fi"]);
        let any_net = Settings { wifi_only: false, ..s.clone() };
        assert!(allowed(&any_net, &cell, ManualDownload, 12).is_ok());
        let vpn = Conditions { network: Network::Vpn, ..charging.clone() };
        assert!(allowed(&s, &vpn, AutoDownload, 12).is_err(), "a VPN as primary connection is not known to be Wi-Fi");
        let saver = Conditions { saver: true, ..charging.clone() };
        assert_eq!(allowed(&any_net, &saver, ManualDownload, 12).unwrap_err(), vec!["Battery saver is on"]);
        let eth = Conditions { network: Network::Ethernet, ..charging };
        assert!(allowed(&s, &eth, AutoDownload, 12).is_ok());
        let both = Conditions { saver: true, network: Network::Cellular, ..wifi() };
        assert_eq!(allowed(&s, &both, AutoDownload, 12).unwrap_err().len(), 3);
    }

    #[test]
    fn installs() {
        let s = Settings::default();
        let low = Conditions { battery: Some(12), ..wifi() };
        assert!(allowed(&s, &low, InstallNow, 12).is_err());
        assert!(allowed(&s, &Conditions { charging: true, ..low.clone() }, InstallNow, 12).is_ok());
        assert!(allowed(&s, &Conditions { saver: true, ..wifi() }, InstallNow, 12).is_ok(), "the user asked explicitly");
        let night = Conditions { charging: true, ..wifi() };
        assert!(allowed(&s, &night, InstallTonight, 3).is_ok());
        assert!(allowed(&s, &night, InstallTonight, 5).is_err());
        assert!(allowed(&s, &night, InstallTonight, 1).is_err());
        assert!(allowed(&s, &wifi(), InstallTonight, 3).is_err());
        assert!(allowed(&s, &Conditions { battery: Some(15), ..night.clone() }, InstallTonight, 3).is_err());
        assert!(allowed(&s, &Conditions { network: Network::None, ..night }, InstallTonight, 3).is_ok(), "installing needs no network");
    }

    #[test]
    fn auto_check_interval() {
        assert!(auto_check_due(None, 100));
        assert!(!auto_check_due(Some(100), 100 + 3600));
        assert!(auto_check_due(Some(100), 100 + AUTO_CHECK_INTERVAL));
        assert!(auto_check_due(Some(10_000), 100), "clock went backwards");
    }
}
