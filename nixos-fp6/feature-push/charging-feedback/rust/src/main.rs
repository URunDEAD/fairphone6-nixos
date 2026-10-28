//! fp6-charging-feedback: charger plug/unplug detection and charging state
//! for Lomiri's charging animation and lock-screen charging line (FP6).
//!
//!   fp6-charging-feedback [OPTIONS]          session agent (D-Bus, see agent.rs)
//!   fp6-charging-feedback status [OPTIONS]   print the current state JSON once
//!
//! Options: --sysfs DIR (default /sys/class/power_supply)
//!          --battery-status FILE (fp6-batteryd, /run/fp6-battery/status.json)
//!          --usb-status FILE (fp6-usbmoded, /run/fp6-usb-modes/status.json)
//!
//! Read-only: never writes sysfs; the only call it makes is the display
//! Peek that Lomiri asks for.

mod agent;
mod json;
mod model;
mod supply;

use gio::prelude::*;
use std::path::PathBuf;

fn main() {
    let mut paths = agent::Paths {
        sysfs: "/sys/class/power_supply".into(),
        battd: "/run/fp6-battery/status.json".into(),
        usb: "/run/fp6-usb-modes/status.json".into(),
    };
    let mut once = false;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        let mut val = |name: &str| -> PathBuf {
            args.next().map(PathBuf::from).unwrap_or_else(|| {
                eprintln!("{name} needs a value");
                std::process::exit(2)
            })
        };
        match a.as_str() {
            "status" => once = true,
            "--sysfs" => paths.sysfs = val("--sysfs"),
            "--battery-status" => paths.battd = val("--battery-status"),
            "--usb-status" => paths.usb = val("--usb-status"),
            "-h" | "--help" => {
                println!("usage: fp6-charging-feedback [status] [--sysfs DIR] [--battery-status FILE] [--usb-status FILE]");
                return;
            }
            other => {
                eprintln!("unknown argument {other:?}");
                std::process::exit(2);
            }
        }
    }
    if once {
        let state = gio::SimpleAction::new_stateful("state", None, &"{}".to_variant());
        let mut a = agent::Agent::new(paths, state, None);
        println!("{}", a.read(None).0);
        return;
    }
    if let Err(e) = agent::run(paths) {
        eprintln!("fp6-charging-feedback: {e}");
        std::process::exit(1);
    }
}
