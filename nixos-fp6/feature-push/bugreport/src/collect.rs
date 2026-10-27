use crate::{
    config::{since_seconds, Options},
    redact::redact,
    runner::{Output, Runner, LIMIT},
};
use serde::{Deserialize, Serialize};
use std::{fs, io::Read, path::Path};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entry {
    pub name: String,
    pub status: String,
    pub text: String,
}
pub trait Sources: Runner {
    fn read(&self, path: &str) -> Output;
    fn children(&self, path: &str) -> Vec<String>;
}
impl Sources for crate::runner::RealRunner {
    fn read(&self, path: &str) -> Output {
        match fs::File::open(path) {
            Ok(file) => {
                let mut bytes = Vec::new();
                match file.take(LIMIT as u64).read_to_end(&mut bytes) {
                    Ok(_) => Output {
                        status: "ok".into(),
                        text: String::from_utf8_lossy(&bytes).into_owned(),
                    },
                    Err(_) => Output::unavailable("unavailable: read failed"),
                }
            }
            Err(_) => Output::unavailable("unavailable: file absent or restricted"),
        }
    }
    fn children(&self, path: &str) -> Vec<String> {
        let mut names: Vec<_> = fs::read_dir(path)
            .into_iter()
            .flatten()
            .filter_map(Result::ok)
            .filter_map(|e| e.file_name().into_string().ok())
            .collect();
        names.sort();
        names.truncate(64);
        names
    }
}
fn entry(name: &str, out: Output) -> Entry {
    Entry {
        name: name.into(),
        status: out.status,
        text: redact(&out.text),
    }
}
fn command(s: &impl Sources, entries: &mut Vec<Entry>, name: &str, program: &str, args: &[&str]) {
    entries.push(entry(name, s.run(program, args)));
}
fn file(s: &impl Sources, entries: &mut Vec<Entry>, name: &str, path: &str) {
    entries.push(entry(name, s.read(path)));
}
fn sysfs(s: &impl Sources, entries: &mut Vec<Entry>, name: &str, root: &str, keys: &[&str]) {
    let mut text = String::new();
    for child in s.children(root) {
        if child.contains('/') || child == "." || child == ".." {
            continue;
        }
        for key in keys {
            let out = s.read(&format!("{root}/{child}/{key}"));
            if out.status == "ok" {
                text.push_str(&format!("{child}/{key}: {}\n", out.text.trim()));
            }
        }
    }
    let status = if text.is_empty() {
        "unavailable: no readable devices"
    } else {
        "ok"
    };
    entries.push(entry(
        name,
        Output {
            status: status.into(),
            text,
        },
    ));
}
// Keep the duration useful without exempting arbitrary numbers from shared redaction.
fn format_uptime(out: Output) -> Output {
    if out.status != "ok" {
        return out;
    }
    let seconds = out
        .text
        .split_whitespace()
        .next()
        .and_then(|v| v.parse::<f64>().ok());
    match seconds {
        Some(seconds) if seconds.is_finite() && (0.0..=10_000_000_000.0).contains(&seconds) => {
            let seconds = seconds as u64;
            Output {
                status: "ok".into(),
                text: format!(
                    "Up for {}d {}h {}m {}s\n",
                    seconds / 86400,
                    seconds / 3600 % 24,
                    seconds / 60 % 60,
                    seconds % 60
                ),
            }
        }
        _ => Output::unavailable("unavailable: invalid uptime value"),
    }
}

pub fn collect(options: &Options, s: &impl Sources) -> Result<Vec<Entry>, String> {
    options.validate()?;
    let mut entries = Vec::new();
    let since = format!("{} seconds ago", since_seconds(&options.since)?);
    if options.includes("system") {
        file(s, &mut entries, "system/release.txt", "/etc/fp6-release");
        entries.push(entry(
            "system/uptime.txt",
            format_uptime(s.read("/proc/uptime")),
        ));
        file(s, &mut entries, "system/kernel.txt", "/proc/version");
    }
    if options.includes("units") {
        command(
            s,
            &mut entries,
            "units/system.txt",
            "systemctl",
            &["--failed", "--no-pager", "--no-legend", "--plain"],
        );
        command(
            s,
            &mut entries,
            "units/user.txt",
            "systemctl",
            &["--user", "--failed", "--no-pager", "--no-legend", "--plain"],
        );
    }
    if options.includes("journal") {
        command(
            s,
            &mut entries,
            "journal/user.txt",
            "journalctl",
            &[
                "--user",
                "--since",
                &since,
                "-n",
                "3000",
                "--no-pager",
                "-o",
                "cat",
            ],
        );
        command(
            s,
            &mut entries,
            "journal/system.txt",
            "journalctl",
            &[
                "--system",
                "--since",
                &since,
                "-n",
                "3000",
                "--no-pager",
                "-o",
                "cat",
            ],
        );
    }
    if options.includes("kernel") {
        command(
            s,
            &mut entries,
            "kernel/dmesg.txt",
            "dmesg",
            &["--color=never"],
        );
    }
    if options.includes("coredumps") {
        command(
            s,
            &mut entries,
            "coredumps/list.txt",
            "coredumpctl",
            &["--no-pager", "--no-legend", "--since", &since, "list"],
        );
    }
    if options.includes("power") {
        sysfs(
            s,
            &mut entries,
            "power/battery.txt",
            "/sys/class/power_supply",
            &[
                "type",
                "status",
                "capacity",
                "voltage_now",
                "current_now",
                "temp",
                "health",
                "charge_now",
                "charge_full",
            ],
        );
        sysfs(
            s,
            &mut entries,
            "power/thermal.txt",
            "/sys/class/thermal",
            &["type", "temp", "cur_state", "max_state"],
        );
    }
    if options.includes("modem") {
        command(
            s,
            &mut entries,
            "modem/ofono-service.txt",
            "systemctl",
            &[
                "show",
                "ofono.service",
                "--property=LoadState,ActiveState,SubState,Result",
            ],
        );
        // oFono's manager returns modem properties; all text is redacted before storage.
        entries.push(ofono_state(s));
    }
    if options.includes("network") {
        command(
            s,
            &mut entries,
            "network/links.txt",
            "ip",
            &["-brief", "link"],
        );
        command(
            s,
            &mut entries,
            "network/addresses.txt",
            "ip",
            &["-brief", "address"],
        );
        command(
            s,
            &mut entries,
            "network/routes.txt",
            "ip",
            &["route", "show"],
        );
        command(
            s,
            &mut entries,
            "network/routes6.txt",
            "ip",
            &["-6", "route", "show"],
        );
        command(
            s,
            &mut entries,
            "network/manager.txt",
            "nmcli",
            &[
                "--colors",
                "no",
                "--fields",
                "STATE,CONNECTIVITY,WIFI-HW,WIFI,WWAN-HW,WWAN",
                "general",
            ],
        );
    }
    if options.includes("audio") {
        command(s, &mut entries, "audio/wpctl.txt", "wpctl", &["status"]);
    }
    if options.includes("display") {
        command(
            s,
            &mut entries,
            "display/kernel-errors.txt",
            "journalctl",
            &[
                "-k",
                "--since",
                &since,
                "-p",
                "warning",
                "-n",
                "1000",
                "--no-pager",
                "-o",
                "cat",
                "--grep",
                "(?i)drm|gpu|msm|display|dsi|adreno",
            ],
        );
        command(
            s,
            &mut entries,
            "display/shell-errors.txt",
            "journalctl",
            &[
                "--user",
                "--since",
                &since,
                "-n",
                "1000",
                "--no-pager",
                "-o",
                "cat",
                "--grep",
                "(?i)lomiri|qt|glyph|render|egl|wayland",
            ],
        );
    }
    if options.includes("memory") {
        for (name, path) in [
            ("meminfo", "/proc/meminfo"),
            ("vmstat", "/proc/vmstat"),
            ("pressure", "/proc/pressure/memory"),
            ("zram", "/sys/block/zram0/mm_stat"),
        ] {
            file(s, &mut entries, &format!("memory/{name}.txt"), path);
        }
        command(
            s,
            &mut entries,
            "memory/app-ready.txt",
            "systemctl",
            &[
                "--user",
                "list-units",
                "--all",
                "--no-pager",
                "--no-legend",
                "*app-ready*",
            ],
        );
        command(
            s,
            &mut entries,
            "memory/oom.txt",
            "journalctl",
            &[
                "--since",
                &since,
                "-n",
                "300",
                "--no-pager",
                "-o",
                "cat",
                "--grep",
                "(?i)oom|out of memory|killed process",
            ],
        );
    }
    if options.includes("storage") {
        command(
            s,
            &mut entries,
            "storage/usage.txt",
            "df",
            &[
                "-h",
                "--output=fstype,size,used,avail,pcent,target",
                "-x",
                "tmpfs",
                "-x",
                "devtmpfs",
            ],
        );
    }
    Ok(entries)
}

pub fn helper_path() -> &'static str {
    option_env!("FP6_HELPER_PATH").unwrap_or("/run/current-system/sw/libexec/fp6-bugreport-helper")
}
/// The helper can ONLY read these four fixed data sources. No paths or commands from the caller.
pub fn privileged_collect(section: &str, since: &str, s: &impl Runner) -> Result<Entry, String> {
    let since = format!("{} seconds ago", since_seconds(since)?);
    let (name, program, args) = match section {
        "kernel" => ("privileged/kernel.txt", "dmesg", vec!["--color=never"]),
        "journal" => (
            "privileged/journal.txt",
            "journalctl",
            vec![
                "--system",
                "--since",
                &since,
                "-n",
                "3000",
                "--no-pager",
                "-o",
                "cat",
            ],
        ),
        "display" => (
            "privileged/display.txt",
            "journalctl",
            vec![
                "-k",
                "--since",
                &since,
                "-p",
                "warning",
                "-n",
                "1000",
                "--no-pager",
                "-o",
                "cat",
                "--grep",
                "(?i)drm|gpu|msm|display|dsi|adreno",
            ],
        ),
        "memory" => (
            "privileged/oom.txt",
            "journalctl",
            vec![
                "-k",
                "--since",
                &since,
                "-n",
                "300",
                "--no-pager",
                "-o",
                "cat",
                "--grep",
                "(?i)oom|out of memory|killed process",
            ],
        ),
        _ => return Err("Helper section not allowed".into()),
    };
    Ok(entry(name, s.run(program, &args)))
}
pub fn collect_privileged(options: &Options, s: &impl Runner) -> Vec<Entry> {
    let sections: Vec<_> = ["kernel", "journal", "display", "memory"]
        .into_iter()
        .filter(|x| options.includes(x))
        .collect();
    if sections.is_empty() {
        return Vec::new();
    }
    let joined = sections.join(",");
    let out = s.run(
        "pkexec",
        &[
            helper_path(),
            "--since",
            &options.since,
            "--sections",
            &joined,
        ],
    );
    if out.status == "ok" {
        if let Ok(entries) = serde_json::from_str::<Vec<Entry>>(&out.text) {
            if entries.iter().all(|e| safe_entry_name(&e.name)) {
                return entries
                    .into_iter()
                    .map(|e| Entry {
                        text: redact(&e.text),
                        status: redact(&e.status),
                        ..e
                    })
                    .collect();
            }
        }
    }
    vec![Entry {
        name: "privileged/status.txt".into(),
        status: "unavailable: permission denied, helper missing, or collection failed".into(),
        text: String::new(),
    }]
}
pub fn safe_entry_name(name: &str) -> bool {
    !name.is_empty()
        && !Path::new(name).is_absolute()
        && Path::new(name)
            .components()
            .all(|c| matches!(c, std::path::Component::Normal(_)))
}

// busctl JSON is decoded before selection: device identifiers never enter the report.
fn variant(value: &serde_json::Value) -> &serde_json::Value {
    value.get("data").unwrap_or(value)
}
fn selected_properties(value: &serde_json::Value, output: &mut String) {
    match value {
        serde_json::Value::Object(map) => {
            for (key, value) in map {
                let value = variant(value);
                let safe = match key.as_str() {
                    "Online" | "Powered" | "Lockdown" | "Emergency" | "Present" => {
                        value.is_boolean()
                    }
                    "Strength" => value.as_u64().is_some_and(|n| n <= 100),
                    "Status" => value.as_str().is_some_and(|v| {
                        [
                            "unregistered",
                            "registered",
                            "searching",
                            "denied",
                            "unknown",
                            "roaming",
                        ]
                        .contains(&v)
                    }),
                    "Technology" => value.as_str().is_some_and(|v| {
                        [
                            "gsm", "edge", "umts", "hspa", "hspap", "lte", "nr", "unknown",
                        ]
                        .contains(&v)
                    }),
                    "PinRequired" => value.as_str().is_some_and(|v| {
                        ["none", "pin", "puk", "pin2", "puk2", "phone", "network"].contains(&v)
                    }),
                    _ => false,
                };
                if safe {
                    output.push_str(&format!("{key}={value}\n"));
                } else if value.is_object() || value.is_array() {
                    selected_properties(value, output);
                }
            }
        }
        serde_json::Value::Array(values) => {
            for v in values {
                selected_properties(v, output);
            }
        }
        _ => (),
    }
}
fn modem_paths(value: &serde_json::Value, paths: &mut Vec<String>) {
    match value {
        serde_json::Value::Array(values) => {
            if let Some(path) = values.first().and_then(|v| v.as_str()) {
                if path.starts_with('/')
                    && path.len() <= 128
                    && path
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '/')
                    && paths.len() < 4
                    && !paths.iter().any(|p| p == path)
                {
                    paths.push(path.into());
                }
            }
            for v in values {
                modem_paths(v, paths);
            }
        }
        serde_json::Value::Object(map) => {
            for v in map.values() {
                modem_paths(v, paths);
            }
        }
        _ => (),
    }
}
fn ofono_state(s: &impl Sources) -> Entry {
    let out = s.run(
        "busctl",
        &[
            "--system",
            "--json=short",
            "call",
            "org.ofono",
            "/",
            "org.ofono.Manager",
            "GetModems",
        ],
    );
    let mut text = String::new();
    let mut status = out.status;
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(&out.text) {
        selected_properties(&value, &mut text);
        let mut paths = Vec::new();
        modem_paths(&value, &mut paths);
        text.push_str(&format!("Modems found (up to 4): {}\n", paths.len()));
        for (i, path) in paths.iter().enumerate() {
            for interface in ["org.ofono.NetworkRegistration", "org.ofono.SimManager"] {
                let state = s.run(
                    "busctl",
                    &[
                        "--system",
                        "--json=short",
                        "call",
                        "org.ofono",
                        path,
                        interface,
                        "GetProperties",
                    ],
                );
                text.push_str(&format!("modem-{} {interface}: {}\n", i + 1, state.status));
                if let Ok(value) = serde_json::from_str::<serde_json::Value>(&state.text) {
                    selected_properties(&value, &mut text);
                }
            }
        }
    } else {
        status = "unavailable: oFono did not return valid JSON".into();
    }
    entry("modem/ofono.txt", Output { status, text })
}

#[cfg(test)]
mod ofono_tests {
    use super::*;
    #[test]
    fn numeric_uptime_is_formatted_without_bypassing_redaction() {
        let out = format_uptime(Output {
            status: "ok".into(),
            text: "93784.12 1234567.89".into(),
        });
        assert_eq!(redact(&out.text), "Up for 1d 2h 3m 4s\n");
        for value in ["NaN", "inf", "-5", "private-text"] {
            assert!(format_uptime(Output {
                status: "ok".into(),
                text: value.into()
            })
            .status
            .starts_with("unavailable:"));
        }
    }
    #[test]
    fn only_allowlisted_typed_modem_properties_survive() {
        let value = serde_json::json!({"data": [["/synthetic_0", {
            "Online": {"type": "b", "data": true}, "Powered": {"type": "b", "data": false},
            "Serial": {"type": "s", "data": "123456789012345"},
            "SubscriberIdentity": {"type": "s", "data": "001019876543210"},
            "Name": {"type": "s", "data": "Private operator"},
            "Status": {"type": "s", "data": "registered"},
            "Technology": {"type": "s", "data": "lte"},
            "Strength": {"type": "y", "data": 75},
            "LocationAreaCode": {"type": "q", "data": 1234}
        }]]});
        let mut out = String::new();
        selected_properties(&value, &mut out);
        assert!(out.contains("Online=true"));
        assert!(out.contains("registered"));
        assert!(out.contains("Strength=75"));
        for secret in [
            "123456789012345",
            "001019876543210",
            "Private operator",
            "1234",
            "synthetic_0",
        ] {
            assert!(!out.contains(secret));
        }
    }
}
