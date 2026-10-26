// SPDX-License-Identifier: MIT
//! The zram swap on/off switch: the system's dev-zram0.swap unit (NixOS
//! zramSwap, zram-generator), started/stopped through systemd on the system
//! bus. A polkit rule (app-ready-fp6.nix) allows exactly these two units to
//! the phone user.

use crate::log;

pub const UNIT: &str = "dev-zram0.swap";

pub fn active() -> bool {
    std::fs::read_to_string("/proc/swaps").is_ok_and(|s| s.lines().any(|l| l.starts_with("/dev/zram0")))
}

pub fn apply(system: &zbus::blocking::Connection, enabled: bool) {
    if active() == enabled {
        return;
    }
    let method = if enabled { "StartUnit" } else { "StopUnit" };
    match system.call_method(
        Some("org.freedesktop.systemd1"),
        "/org/freedesktop/systemd1",
        Some("org.freedesktop.systemd1.Manager"),
        method,
        &(UNIT, "replace"),
    ) {
        Ok(_) => log(format!("zram swap: {method} {UNIT}")),
        Err(e) => log(format!("zram swap: {method} {UNIT}: {e}")),
    }
}
