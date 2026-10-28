use crate::{
    dbus::{self, Objects},
    process::capture,
    report::{Check, Status},
};
use std::{collections::BTreeMap, fs};

pub const CHECKS: &[&str] = &[
    "release",
    "failed-system",
    "failed-user",
    "coredumps",
    "journal-system",
    "journal-user",
    "disk",
    "memory",
    "zram",
    "display-manager",
    "lomiri",
    "maliit",
    "audio-sink",
    "audio-source",
    "network",
    "bluetooth",
    "modem",
    "mobile-data",
    "gps",
    "fingerprint",
];
pub fn check(id: &str) -> Check {
    let result = match id {
        "release" => match fs::read("/etc/fp6-release") {
            Ok(v) if !v.is_empty() => (Status::Pass, "Release metadata is readable".into()),
            _ => (
                Status::Fail,
                "Release metadata is missing or unreadable".into(),
            ),
        },
        "failed-system" => failed_units(false),
        "failed-user" => failed_units(true),
        "coredumps" => journal(false, true),
        "journal-system" => journal(false, false),
        "journal-user" => journal(true, false),
        "disk" => disk(),
        "memory" => memory(),
        "zram" => zram(),
        "display-manager" => active("display-manager.service", false),
        "lomiri" => active("lomiri.service", true),
        "maliit" => match keyboard_running() {
            Some(true) => (Status::Pass, "Keyboard process is running".into()),
            Some(false) => (Status::Fail, "Keyboard process is absent".into()),
            None => (Status::Warn, "Process list cannot be inspected".into()),
        },
        "audio-sink" | "audio-source" => {
            let node = if id == "audio-sink" {
                "@DEFAULT_AUDIO_SINK@"
            } else {
                "@DEFAULT_AUDIO_SOURCE@"
            };
            match capture("wpctl", &["inspect", node]) {
                Ok(s) if s.contains("id ") => (
                    Status::Pass,
                    "PipeWire/WirePlumber default node exists; no audio played or recorded".into(),
                ),
                _ => (
                    Status::Warn,
                    "Default audio node unavailable; check PipeWire and WirePlumber".into(),
                ),
            }
        }
        "network" => network().unwrap_or((
            Status::Warn,
            "NetworkManager is unreachable or its state is unreadable".into(),
        )),
        "bluetooth" => bluetooth().unwrap_or((
            Status::Warn,
            "Bluetooth service or adapter is unavailable".into(),
        )),
        "modem" => modem(false).unwrap_or((Status::Warn, "oFono status could not be read".into())),
        "mobile-data" => {
            modem(true).unwrap_or((Status::Warn, "Mobile data status could not be read".into()))
        }
        "gps" => presence(
            "org.freedesktop.GeoClue2",
            "/org/freedesktop/GeoClue2/Manager",
            "org.freedesktop.GeoClue2.Manager",
        ),
        "fingerprint" => fingerprint().unwrap_or((
            Status::Warn,
            "Fingerprint daemon could not be inspected".into(),
        )),
        _ => (Status::Fail, "Unknown base check".into()),
    };
    Check::new(id, result.0, &result.1)
}
type Outcome = (Status, String);
/// Program name with Nix wrapper decorations removed: `.maliit-keyboard-wrapped`
/// and `..maliit-keyboard-wrapped-wrapped` both become `maliit-keyboard`.
pub fn program_name(path: &str) -> &str {
    let mut name = path.rsplit('/').next().unwrap_or("");
    name = name.trim_start_matches('.');
    while let Some(n) = name.strip_suffix("-wrapped") {
        name = n;
    }
    name
}
pub fn is_keyboard(argv0: &str) -> bool {
    matches!(program_name(argv0), "maliit-server" | "maliit-keyboard")
}
/// Scans /proc for the keyboard by argv[0] and executable (comm is truncated
/// to 15 bytes, which hides wrapped Nix store names). None if /proc is unreadable.
fn keyboard_running() -> Option<bool> {
    let entries = fs::read_dir("/proc").ok()?;
    for entry in entries.flatten() {
        let name = entry.file_name();
        if !name.to_string_lossy().bytes().all(|b| b.is_ascii_digit()) {
            continue;
        }
        let dir = entry.path();
        if let Ok(cmdline) = fs::read(dir.join("cmdline")) {
            let argv0 = cmdline.split(|b| *b == 0).next().unwrap_or(&[]);
            if is_keyboard(&String::from_utf8_lossy(argv0)) {
                return Some(true);
            }
        }
        if let Ok(exe) = fs::read_link(dir.join("exe")) {
            if is_keyboard(&exe.to_string_lossy()) {
                return Some(true);
            }
        }
    }
    Some(false)
}
fn active(unit: &str, user: bool) -> Outcome {
    let mut args = vec!["show", "--property=ActiveState", "--value", unit];
    if user {
        args.push("--user");
    }
    match capture("systemctl", &args) {
        Ok(s) if s.trim() == "active" => (Status::Pass, "Required service is active".into()),
        Ok(_) => (Status::Fail, "Required service is not active".into()),
        Err(_) => (Status::Warn, "Service manager is inaccessible".into()),
    }
}
fn failed_units(user: bool) -> Outcome {
    let mut args = vec![
        "--failed",
        "--no-legend",
        "--plain",
        "--no-pager",
        "list-units",
    ];
    if user {
        args.push("--user");
    }
    match capture("systemctl", &args) {
        Ok(s) => {
            let n = s.lines().filter(|l| !l.trim().is_empty()).count();
            (
                if n == 0 { Status::Pass } else { Status::Fail },
                format!("{n} failed units; unit identities suppressed"),
            )
        }
        Err(_) => (Status::Warn, "Service manager is inaccessible".into()),
    }
}
fn journal(user: bool, cores: bool) -> Outcome {
    let scope = if user { "--user" } else { "--system" };
    // An empty readable error stream alone must not imply journal visibility.
    match capture(
        "journalctl",
        &[
            scope,
            "-b",
            "-n",
            "1",
            "-q",
            "--no-pager",
            "-o",
            "json",
            "--output-fields=PRIORITY",
        ],
    ) {
        Ok(s) if !s.trim().is_empty() => {}
        _ => {
            return (
                Status::Warn,
                "Boot journal is empty or inaccessible; coverage is incomplete".into(),
            )
        }
    }
    let mut args = vec![
        scope,
        "-b",
        "-n",
        "1000",
        "-q",
        "--no-pager",
        "-o",
        "json",
        "--output-fields=_SYSTEMD_UNIT,_SYSTEMD_USER_UNIT,PRIORITY",
    ];
    if cores {
        args.extend(["-t", "systemd-coredump"]);
    } else {
        args.extend(["-p", "err"]);
    }
    match capture("journalctl", &args) {
        Ok(s) => journal_summary(&s, cores),
        Err(_) => (Status::Warn, "Boot journal could not be read".into()),
    }
}
pub fn journal_summary(s: &str, cores: bool) -> Outcome {
    let mut counts = BTreeMap::<&str, usize>::new();
    let mut n = 0;
    // Only these authored labels may be echoed. Template instance/unit names can
    // contain user identifiers and are always grouped into 'other'.
    let units = [
        ("lomiri.service", "shell"),
        ("display-manager.service", "display"),
        ("pipewire.service", "audio"),
        ("wireplumber.service", "audio-policy"),
        ("ofono.service", "modem"),
        ("NetworkManager.service", "network"),
        ("maliit-server.service", "keyboard"),
        ("fp6-quickshare.service", "quickshare"),
        ("fp6-webshare.service", "webshare"),
        ("lomiri-kdeconnect.service", "kdeconnect"),
        ("fairphone-ocr.service", "ocr"),
        ("fp6-dictation.service", "dictation"),
        ("fp6-share.service", "share"),
        ("fp6-netpolicy.service", "netpolicy"),
        ("fp6-calls.service", "calls"),
    ];
    for line in s.lines().filter(|s| !s.is_empty()) {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
            return (Status::Warn, "Journal reply was malformed".into());
        };
        n += 1;
        let unit = v
            .get("_SYSTEMD_USER_UNIT")
            .or_else(|| v.get("_SYSTEMD_UNIT"))
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let label = units
            .iter()
            .find(|(u, _)| *u == unit)
            .map(|(_, label)| *label)
            .unwrap_or("other");
        *counts.entry(label).or_default() += 1;
    }
    if n == 0 {
        return (
            Status::Pass,
            if cores {
                "No coredumps recorded since boot"
            } else {
                "No visible journal errors since boot"
            }
            .into(),
        );
    }
    let detail = if cores {
        format!("{n} coredump journal records since boot; no core files read")
    } else {
        format!(
            "{n} journal errors by unit group: {}",
            counts
                .iter()
                .map(|(k, v)| format!("{k}={v}"))
                .collect::<Vec<_>>()
                .join(", ")
        )
    };
    (
        Status::Warn,
        if n == 1000 {
            format!("{detail}; capped at 1000, additional records may exist")
        } else {
            detail
        },
    )
}
fn disk() -> Outcome {
    match capture("df", &["--output=pcent", "/", "/home"]) {
        Ok(s) => {
            let values: Vec<_> = s
                .lines()
                .skip(1)
                .filter_map(|v| v.trim().trim_end_matches('%').parse::<u32>().ok())
                .collect();
            if values.len() != 2 {
                return (Status::Warn, "Disk usage reply was malformed".into());
            }
            let max = *values.iter().max().unwrap();
            (
                if max >= 95 {
                    Status::Fail
                } else if max >= 85 {
                    Status::Warn
                } else {
                    Status::Pass
                },
                format!("Maximum root/home filesystem usage: {max}%"),
            )
        }
        Err(_) => (Status::Warn, "Disk usage could not be read".into()),
    }
}
fn memory() -> Outcome {
    fs::read_to_string("/proc/meminfo")
        .map(|s| memory_summary(&s))
        .unwrap_or((Status::Warn, "Memory counters unavailable".into()))
}
pub fn memory_summary(s: &str) -> Outcome {
    let get = |key: &str| {
        s.lines()
            .find(|l| l.starts_with(key))
            .and_then(|l| l.split_whitespace().nth(1))
            .and_then(|v| v.parse::<u64>().ok())
    };
    match (get("MemTotal:"), get("MemAvailable:")) {
        (Some(total), Some(available)) if total > 0 && available <= total => {
            let percent = available.saturating_mul(100) / total;
            (
                if percent < 5 {
                    Status::Fail
                } else if percent < 10 {
                    Status::Warn
                } else {
                    Status::Pass
                },
                format!("Available memory: {percent}%"),
            )
        }
        _ => (Status::Warn, "Memory counters malformed".into()),
    }
}
fn zram() -> Outcome {
    let Ok(swaps) = fs::read_to_string("/proc/swaps") else {
        return (Status::Warn, "Swap status unreadable".into());
    };
    let n = swaps
        .lines()
        .skip(1)
        .filter(|l| {
            l.split_whitespace()
                .next()
                .is_some_and(|s| s.starts_with("/dev/zram"))
        })
        .count();
    (
        if n > 0 { Status::Pass } else { Status::Warn },
        format!("{n} active zram swap devices"),
    )
}
fn network() -> zbus::Result<Outcome> {
    let c = dbus::connect("system")?;
    let Some(owner) = dbus::owner(&c, "org.freedesktop.NetworkManager")? else {
        return Ok((Status::Fail, "NetworkManager is not running".into()));
    };
    let state: u32 = dbus::proxy(
        &c,
        &owner,
        "/org/freedesktop/NetworkManager",
        "org.freedesktop.NetworkManager",
    )?
    .get_property("State")?;
    Ok(match state {
        50 | 60 | 70 => (
            Status::Pass,
            "NetworkManager reports a connected state; connectivity was not tested".into(),
        ),
        0 | 10 | 20 | 30 | 40 => (
            Status::Warn,
            "NetworkManager is offline or transitioning; no traffic was generated".into(),
        ),
        _ => (
            Status::Fail,
            "NetworkManager returned an unknown state".into(),
        ),
    })
}
fn bluetooth() -> zbus::Result<Outcome> {
    let c = dbus::connect("system")?;
    let Some(owner) = dbus::owner(&c, "org.bluez")? else {
        return Ok((
            Status::Skip,
            "Bluetooth service is not running; no activation requested".into(),
        ));
    };
    let objects: HashObjects = dbus::proxy(&c, &owner, "/", "org.freedesktop.DBus.ObjectManager")?
        .call("GetManagedObjects", &())?;
    let present = objects
        .values()
        .any(|v| v.contains_key("org.bluez.Adapter1"));
    Ok((
        if present { Status::Pass } else { Status::Warn },
        if present {
            "Bluetooth adapter present; no discovery requested"
        } else {
            "No Bluetooth adapter found"
        }
        .into(),
    ))
}
type HashObjects = std::collections::HashMap<
    zbus::zvariant::OwnedObjectPath,
    std::collections::HashMap<String, dbus::Props>,
>;
fn modem(data: bool) -> zbus::Result<Outcome> {
    let c = dbus::connect("system")?;
    let Some(owner) = dbus::owner(&c, "org.ofono")? else {
        return Ok((Status::Fail, "oFono is not running".into()));
    };
    let modems: Objects =
        dbus::proxy(&c, &owner, "/", "org.ofono.Manager")?.call("GetModems", &())?;
    if modems.is_empty() {
        return Ok((Status::Warn, "No modem is exposed by oFono".into()));
    }
    let mut enabled = 0;
    let mut unhealthy = 0;
    for (path, props) in modems {
        if data {
            let props = dbus::properties(&c, &owner, &path, "org.ofono.ConnectionManager")?;
            match dbus::bool_prop(&props, "Powered") {
                Some(false) => continue,
                Some(true) => enabled += 1,
                None => {
                    return Ok((
                        Status::Fail,
                        "Mobile data enabled state missing or malformed".into(),
                    ))
                }
            }
            let contexts: Objects = dbus::proxy(&c, &owner, &path, "org.ofono.ConnectionManager")?
                .call("GetContexts", &())?;
            if !contexts.iter().any(|(_, p)| {
                dbus::str_prop(p, "Type") == Some("internet")
                    && dbus::bool_prop(p, "Active") == Some(true)
            }) {
                unhealthy += 1;
            }
        } else {
            if dbus::bool_prop(&props, "Powered") != Some(true)
                || dbus::bool_prop(&props, "Online") != Some(true)
            {
                unhealthy += 1;
                continue;
            }
            let sim = dbus::properties(&c, &owner, &path, "org.ofono.SimManager")?;
            if dbus::bool_prop(&sim, "Present") != Some(true) {
                unhealthy += 1;
                continue;
            }
            let registration =
                dbus::properties(&c, &owner, &path, "org.ofono.NetworkRegistration")?;
            if !matches!(
                dbus::str_prop(&registration, "Status"),
                Some("registered" | "roaming")
            ) {
                unhealthy += 1;
            }
        }
    }
    Ok(if data && enabled == 0 {
        (
            Status::Skip,
            "Mobile data is disabled; no context activation requested".into(),
        )
    } else if unhealthy > 0 {
        (
            Status::Warn,
            if data {
                "Enabled mobile data has no active internet context"
            } else {
                "Modem is offline, unpowered, SIM absent, or not registered"
            }
            .into(),
        )
    } else {
        (
            Status::Pass,
            if data {
                "Enabled mobile data has an active internet context"
            } else {
                "Modem powered and online, SIM present, registered; identifiers discarded"
            }
            .into(),
        )
    })
}
fn presence(name: &str, path: &str, interface: &str) -> Outcome {
    let p = crate::config::Probe {
        id: "presence".into(),
        bus: "system".into(),
        name: name.into(),
        path: path.into(),
        interface: interface.into(),
        unit: None,
        method: None,
        property: None,
        shape: None,
        fields: Default::default(),
        values: vec![],
        required: false,
        unverified: false,
    };
    let r = dbus::check(&p);
    if r.detail.starts_with("Interface reachable") {
        (
            Status::Pass,
            "Service interface present; no sensor request made".into(),
        )
    } else {
        (r.status, r.detail)
    }
}
fn fingerprint() -> zbus::Result<Outcome> {
    let c = dbus::connect("system")?;
    let Some(owner) = dbus::owner(&c, "net.reactivated.Fprint")? else {
        return Ok(presence(
            "net.reactivated.Fprint",
            "/net/reactivated/Fprint/Manager",
            "net.reactivated.Fprint.Manager",
        ));
    };
    let devices: Vec<zbus::zvariant::OwnedObjectPath> = dbus::proxy(
        &c,
        &owner,
        "/net/reactivated/Fprint/Manager",
        "net.reactivated.Fprint.Manager",
    )?
    .call("GetDevices", &())?;
    Ok((
        if devices.is_empty() {
            Status::Warn
        } else {
            Status::Pass
        },
        if devices.is_empty() {
            "Fingerprint daemon reachable, but no devices exposed"
        } else {
            "Fingerprint daemon reachable; device present, no biometric operations"
        }
        .into(),
    ))
}
