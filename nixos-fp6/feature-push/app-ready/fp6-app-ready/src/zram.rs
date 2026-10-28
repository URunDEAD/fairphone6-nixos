// SPDX-License-Identifier: MIT
//! The zram swap on/off switch: starts the system units fp6-zram-on.service
//! / fp6-zram-off.service (swapon / swapoff of the zram-generator device,
//! app-ready-fp6.nix) through systemd on the system bus; a polkit rule allows
//! exactly these two to the phone user.

use crate::log;

pub fn active() -> bool {
    std::fs::read_to_string("/proc/swaps").is_ok_and(|s| s.lines().any(|l| l.starts_with("/dev/zram0")))
}

pub fn apply(system: &zbus::blocking::Connection, enabled: bool) {
    if active() == enabled {
        return;
    }
    let unit = if enabled { "fp6-zram-on.service" } else { "fp6-zram-off.service" };
    match system.call_method(
        Some("org.freedesktop.systemd1"),
        "/org/freedesktop/systemd1",
        Some("org.freedesktop.systemd1.Manager"),
        "StartUnit",
        &(unit, "replace"),
    ) {
        Ok(_) => log(format!("zram swap: started {unit}")),
        Err(e) => log(format!("zram swap: {unit}: {e}")),
    }
}

/// Resize the zram swap to `percent` of RAM (fp6-zram-size@<percent>.service,
/// allowed by polkit for the sizes in profile::ZRAM_SIZES). The unit itself
/// refuses when the swapped-out data would not fit back into free RAM.
pub fn resize(system: &zbus::blocking::Connection, percent: u32) {
    if !crate::profile::ZRAM_SIZES.contains(&percent) {
        return log(format!("zram size {percent} %: not one of {:?}", crate::profile::ZRAM_SIZES));
    }
    start(system, &format!("fp6-zram-size@{percent}.service"));
}

/// Apply a memory profile's root-side settings (fp6-memory-profile@<name>.service).
pub fn apply_profile(system: &zbus::blocking::Connection, profile: crate::profile::Profile) {
    start(system, &format!("fp6-memory-profile@{}.service", profile.name()));
}

fn start(system: &zbus::blocking::Connection, unit: &str) {
    match system.call_method(
        Some("org.freedesktop.systemd1"),
        "/org/freedesktop/systemd1",
        Some("org.freedesktop.systemd1.Manager"),
        "StartUnit",
        &(unit, "replace"),
    ) {
        Ok(_) => log(format!("started {unit}")),
        Err(e) => log(format!("{unit}: {e}")),
    }
}

/// Current zram size as % of RAM (rounded), None when there is no zram device.
pub fn current_percent(disksize: u64, mem_total: u64) -> Option<u32> {
    if disksize == 0 || mem_total == 0 {
        return None;
    }
    Some(((disksize as f64 / mem_total as f64) * 100.0).round() as u32)
}

#[cfg(test)]
mod tests {
    #[test]
    fn percent() {
        // the phone: 3827302400 of 7655505920 bytes
        assert_eq!(super::current_percent(3_827_302_400, 7_655_505_920), Some(50));
        assert_eq!(super::current_percent(0, 1), None);
    }
}
