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
