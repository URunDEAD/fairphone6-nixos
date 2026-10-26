//! fp6-batteryd: battery and charging for Lomiri on the Fairphone 6.
//!
//!   fp6-batteryd daemon [options]   system service (root): charge limit,
//!                                   adaptive charging, battery saver, logger
//!   fp6-batteryd agent              user service (Lomiri session): settings,
//!                                   next alarm, saver extras, indicator
//!   fp6-batteryd status             print /run/fp6-battery/status.json
//!   fp6-batteryd next-alarm         print the next Clock alarm (indicator)
//!
//! Notes: results/feature-push-20261024/battery/README.md

mod agent;
mod charge;
mod daemon;
mod json;
mod logger;
mod saver;
mod supply;

use std::path::PathBuf;

fn usage() -> ! {
    eprintln!("usage: fp6-batteryd daemon [--state-dir D] [--run-dir D] [--uid N] [--power-supply D] [--cpufreq D] [--cgroup D] [--gps-unit U]... [--waydroid-unit U] [--session-bus]\n       fp6-batteryd agent [--status FILE]\n       fp6-batteryd status [--run-dir D]\n       fp6-batteryd next-alarm");
    std::process::exit(2)
}

fn main() {
    let mut args = std::env::args().skip(1);
    let cmd = args.next().unwrap_or_default();
    let rest: Vec<String> = args.collect();
    let mut it = rest.iter();
    let mut o = daemon::Options {
        state_dir: "/var/lib/fp6-battery".into(),
        run_dir: "/run/fp6-battery".into(),
        uid: 10000,
        power_supply: "/sys/class/power_supply".into(),
        cpufreq: "/sys/devices/system/cpu/cpufreq".into(),
        cgroup: "/sys/fs/cgroup".into(),
        gps_units: Vec::new(),
        waydroid_unit: "waydroid-container.service".into(),
        session_bus: false,
    };
    let mut status: PathBuf = "/run/fp6-battery/status.json".into();
    while let Some(a) = it.next() {
        let mut val = || it.next().cloned().unwrap_or_else(|| usage());
        match a.as_str() {
            "--state-dir" => o.state_dir = val().into(),
            "--run-dir" => {
                o.run_dir = val().into();
                status = o.run_dir.join("status.json");
            }
            "--uid" => o.uid = val().parse().unwrap_or_else(|_| usage()),
            "--power-supply" => o.power_supply = val().into(),
            "--cpufreq" => o.cpufreq = val().into(),
            "--cgroup" => o.cgroup = val().into(),
            "--gps-unit" => o.gps_units.push(val()),
            "--waydroid-unit" => o.waydroid_unit = val(),
            "--session-bus" => o.session_bus = true,
            "--status" => status = val().into(),
            _ => usage(),
        }
    }
    let result = match cmd.as_str() {
        "daemon" => daemon::run(o),
        "agent" => agent::run(status),
        "status" => std::fs::read_to_string(&status).map(|s| println!("{s}")).map_err(|e| e.to_string()),
        "next-alarm" => gio::bus_get_sync(gio::BusType::Session, None::<&gio::Cancellable>)
            .map_err(|e| e.to_string())
            .and_then(|c| agent::query_next_alarm(&c, daemon::now()))
            .map(|a| match a {
                Some(t) => println!("{t}"),
                None => println!("none"),
            }),
        _ => usage(),
    };
    if let Err(e) = result {
        eprintln!("fp6-batteryd {cmd}: {e}");
        std::process::exit(1);
    }
}
