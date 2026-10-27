//! Facts from the running system (or the laptop dry run's fake phone):
//! network/charging/saver conditions, health facts, the booted slot, slot
//! control and reboots.

use crate::config::Config;
use crate::health::Facts;
use crate::policy::{Conditions, Network};
use crate::qbootctl::{self, BootControl, Dump, SlotState};
use crate::util;
use std::collections::{BTreeSet, HashMap};
use std::fs;
use std::path::Path;
use zbus::zvariant::OwnedValue;

pub fn booted_slot(cfg: &Config) -> Option<String> {
    let c = fs::read_to_string(&cfg.proc_cmdline).ok()?;
    c.split_whitespace().find_map(|t| t.strip_prefix("androidboot.slot_suffix=")).filter(|s| *s == "_a" || *s == "_b").map(str::to_string)
}

pub fn booted_toplevel(cfg: &Config) -> String {
    fs::canonicalize(&cfg.booted_system).map(|p| p.to_string_lossy().into_owned()).unwrap_or_default()
}

fn read_trim(p: &Path) -> Option<String> {
    fs::read_to_string(p).ok().map(|s| s.trim().to_string())
}

pub fn conditions(cfg: &Config) -> Conditions {
    if let Some(f) = cfg.fake.as_ref().and_then(|f| f.conditions.as_ref()) {
        return fs::read(f).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
    }
    let mut c = Conditions { saver: read_trim(&cfg.saver_file).as_deref() == Some("1"), ..Default::default() };
    if let Ok(rd) = fs::read_dir(&cfg.power_supply) {
        for e in rd.flatten() {
            let p = e.path();
            let ty = read_trim(&p.join("type")).unwrap_or_default();
            if ty == "Battery" {
                let st = read_trim(&p.join("status")).unwrap_or_default();
                if st == "Charging" || st == "Full" {
                    c.charging = true;
                }
                if let Some(cap) = read_trim(&p.join("capacity")).and_then(|v| v.parse::<u8>().ok()) {
                    c.battery = Some(cap.min(100));
                }
            } else if read_trim(&p.join("online")).as_deref() == Some("1") {
                c.charging = true;
            }
        }
    }
    if let Ok((net, metered)) = nm_network() {
        c.network = net;
        c.metered = metered;
    }
    c
}

fn nm_type(t: &str) -> Network {
    match t {
        "802-11-wireless" => Network::Wifi,
        "802-3-ethernet" => Network::Ethernet,
        "gsm" | "cdma" | "bluetooth" => Network::Cellular,
        "vpn" | "wireguard" | "tun" => Network::Vpn,
        "" => Network::None,
        _ => Network::Other,
    }
}

fn nm_network() -> Result<(Network, bool), String> {
    let c = zbus::blocking::Connection::system().map_err(|e| e.to_string())?;
    let nm = zbus::blocking::Proxy::new(&c, "org.freedesktop.NetworkManager", "/org/freedesktop/NetworkManager", "org.freedesktop.NetworkManager")
        .map_err(|e| e.to_string())?;
    let ty: String = nm.get_property("PrimaryConnectionType").map_err(|e| e.to_string())?;
    let metered: u32 = nm.get_property("Metered").unwrap_or(0);
    let mut net = nm_type(&ty);
    if net == Network::Vpn || net == Network::Other {
        // The VPN rides on something: Wi-Fi if any active connection is Wi-Fi.
        let acs: Vec<zbus::zvariant::OwnedObjectPath> = nm.get_property("ActiveConnections").unwrap_or_default();
        for p in acs {
            if let Ok(a) = zbus::blocking::Proxy::new(&c, "org.freedesktop.NetworkManager", p, "org.freedesktop.NetworkManager.Connection.Active") {
                if a.get_property::<String>("Type").ok().as_deref() == Some("802-11-wireless") {
                    net = Network::Wifi;
                }
            }
        }
    }
    Ok((net, metered == 1 || metered == 3))
}

fn systemctl(cfg: &Config, args: &[&str]) -> String {
    std::process::Command::new(&cfg.systemctl).args(args).output()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned()).unwrap_or_default()
}

pub fn unit_active(cfg: &Config, unit: &str) -> bool {
    systemctl(cfg, &["is-active", unit]).trim() == "active"
}

pub fn failed_units(cfg: &Config) -> Vec<String> {
    systemctl(cfg, &["list-units", "--failed", "--plain", "--no-legend", "--no-pager"]).lines()
        .filter_map(|l| l.split_whitespace().next()).map(str::to_string).collect()
}

/// (comm, pid) of processes whose comm is one of `names`.
pub fn processes(names: &[String]) -> Vec<(String, u32)> {
    let mut out = Vec::new();
    if let Ok(rd) = fs::read_dir("/proc") {
        for e in rd.flatten() {
            let Ok(pid) = e.file_name().to_string_lossy().parse::<u32>() else { continue };
            if let Some(comm) = read_trim(&e.path().join("comm")) {
                if names.iter().any(|n| *n == comm) {
                    out.push((comm, pid));
                }
            }
        }
    }
    out
}

/// A TCP socket in LISTEN state on `port` (/proc/net/tcp, tcp6).
pub fn tcp_listening(port: u16, tables: &[String]) -> bool {
    let want = format!(":{port:04X}");
    tables.iter().any(|t| t.lines().skip(1).any(|l| {
        let f: Vec<&str> = l.split_whitespace().collect();
        f.len() > 3 && f[1].ends_with(&want) && f[3] == "0A"
    }))
}

fn iface_up(name: &str) -> Option<bool> {
    let flags = read_trim(Path::new(&format!("/sys/class/net/{name}/flags")))?;
    let v = u32::from_str_radix(flags.trim_start_matches("0x"), 16).ok()?;
    Some(v & 1 != 0)
}

/// (sim_present, powered, online) of the first modem ofono reports.
fn ofono() -> Result<(Option<bool>, Option<bool>, Option<bool>), String> {
    let c = zbus::blocking::Connection::system().map_err(|e| e.to_string())?;
    let r = c.call_method(Some("org.ofono"), "/", Some("org.ofono.Manager"), "GetModems", &()).map_err(|e| e.to_string())?;
    let modems: Vec<(zbus::zvariant::OwnedObjectPath, HashMap<String, OwnedValue>)> = r.body().deserialize().map_err(|e| e.to_string())?;
    let Some((path, props)) = modems.into_iter().next() else { return Ok((None, None, None)) };
    let b = |m: &HashMap<String, OwnedValue>, k: &str| m.get(k).and_then(|v| bool::try_from(v).ok());
    let powered = b(&props, "Powered");
    let online = b(&props, "Online");
    let sim = c.call_method(Some("org.ofono"), path.as_str(), Some("org.ofono.SimManager"), "GetProperties", &())
        .ok().and_then(|m| m.body().deserialize::<HashMap<String, OwnedValue>>().ok()).and_then(|m| b(&m, "Present"));
    Ok((Some(sim.unwrap_or(false)), powered, online))
}

/// One snapshot of the health facts; also returns the shell pids.
pub fn gather(cfg: &Config) -> (Facts, BTreeSet<u32>) {
    if let Some(f) = cfg.fake.as_ref().and_then(|f| f.health_facts.as_ref()) {
        let facts: Facts = fs::read(f).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
        return (facts, BTreeSet::from([1]));
    }
    let procs = processes(&cfg.shell_processes);
    let failed = failed_units(cfg);
    let tables: Vec<String> = ["/proc/net/tcp", "/proc/net/tcp6"].iter().filter_map(|p| fs::read_to_string(p).ok()).collect();
    let (sim, powered, online) = ofono().unwrap_or((None, None, None));
    let netbird_active = cfg.netbird_unit.as_ref().map(|u| unit_active(cfg, u));
    let netbird_connected = netbird_active.map(|_| iface_up(&cfg.netbird_interface).unwrap_or(false));
    let mut names: Vec<String> = procs.iter().map(|p| p.0.clone()).collect();
    names.sort();
    names.dedup();
    let facts = Facts {
        display_manager_active: unit_active(cfg, "display-manager.service"),
        shell_processes: names,
        shell_stable: true,
        failed_critical_units: failed.iter().filter(|u| cfg.critical_units.contains(u)).cloned().collect(),
        failed_units: failed,
        sim_present: sim,
        modem_powered: powered,
        modem_online: online,
        sshd_active: unit_active(cfg, "sshd.service"),
        ssh_listening: tcp_listening(cfg.ssh_port, &tables),
        netbird_active,
        netbird_connected,
    };
    (facts, procs.into_iter().map(|p| p.1).collect())
}

/// Slot control backed by a JSON file (laptop dry run): the same state
/// transitions qbootctl makes.
pub struct FakeBootControl {
    pub file: std::path::PathBuf,
    pub cmdline: std::path::PathBuf,
}

impl FakeBootControl {
    fn load(&self) -> Result<Dump, String> {
        let mut d: Dump = serde_json::from_slice(&fs::read(&self.file).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        d.current = fs::read_to_string(&self.cmdline).ok()
            .and_then(|c| c.split_whitespace().find_map(|t| t.strip_prefix("androidboot.slot_suffix=").map(str::to_string)));
        Ok(d)
    }
    fn save(&self, d: &Dump) -> Result<(), String> {
        util::write_atomic(&self.file, serde_json::to_string_pretty(d).unwrap().as_bytes(), 0o644)
    }
}

impl BootControl for FakeBootControl {
    fn dump(&self) -> Result<Dump, String> {
        self.load()
    }
    fn set_active(&self, suffix: &str) -> Result<(), String> {
        let mut d = self.load()?;
        for s in d.slots.iter_mut() {
            if s.suffix == suffix {
                *s = SlotState { suffix: s.suffix.clone(), active: true, successful: false, bootable: true };
            } else {
                s.active = false;
            }
        }
        util::log(format!("fp6-updated: (fake) qbootctl -s {}", qbootctl::letter(suffix)));
        self.save(&d)
    }
    fn mark_successful(&self) -> Result<(), String> {
        let mut d = self.load()?;
        let cur = d.current.clone().ok_or("no current slot")?;
        for s in d.slots.iter_mut().filter(|s| s.suffix == cur) {
            s.successful = true;
            s.bootable = true;
        }
        util::log("fp6-updated: (fake) qbootctl -m");
        self.save(&d)
    }
}

pub fn boot_control(cfg: &Config) -> Box<dyn BootControl> {
    match cfg.fake.as_ref().and_then(|f| f.slots.clone()) {
        Some(file) => Box::new(FakeBootControl { file, cmdline: cfg.proc_cmdline.clone() }),
        None => Box::new(qbootctl::Qbootctl { path: cfg.qbootctl.clone() }),
    }
}

pub fn reboot(cfg: &Config, why: &str) -> Result<(), String> {
    util::log(format!("fp6-updated: rebooting ({why})"));
    if let Some(f) = cfg.fake.as_ref().and_then(|f| f.reboot_log.as_ref()) {
        use std::io::Write;
        let mut o = fs::OpenOptions::new().create(true).append(true).open(f).map_err(|e| e.to_string())?;
        writeln!(o, "{} reboot: {why}", util::now()).map_err(|e| e.to_string())?;
        return Ok(());
    }
    util::run(&cfg.systemctl, &["reboot".into()]).map(|_| ())
}

pub fn local_hour(cfg: &Config) -> u32 {
    cfg.fake.as_ref().and_then(|f| f.hour_file.as_ref()).and_then(|p| read_trim(p)).and_then(|h| h.parse().ok())
        .unwrap_or_else(|| util::local_hour(util::now()))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tcp_listen_parsing() {
        let t = "  sl  local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode\n   0: 00000000:0016 00000000:0000 0A 00000000:00000000 00:00000000 00000000     0        0 1 1\n   1: 0100007F:1F90 00000000:0000 01 00000000:00000000 00:00000000 00000000     0        0 2 1\n".to_string();
        assert!(tcp_listening(22, &[t.clone()]));
        assert!(!tcp_listening(8080, &[t.clone()]), "established, not listening");
        assert!(!tcp_listening(2222, &[t]));
        let t6 = "  sl  local_address                         remote_address                        st\n   0: 00000000000000000000000000000000:0016 00000000000000000000000000000000:0000 0A 0\n".to_string();
        assert!(tcp_listening(22, &[t6]));
    }

    #[test]
    fn nm_types() {
        assert_eq!(nm_type("802-11-wireless"), Network::Wifi);
        assert_eq!(nm_type("gsm"), Network::Cellular);
        assert_eq!(nm_type(""), Network::None);
        assert_eq!(nm_type("vpn"), Network::Vpn);
    }

    #[test]
    fn fake_slot_transitions() {
        let d = std::env::temp_dir().join(format!("fp6-fakeslots-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        let dump = Dump { current: None, slots: vec![
            SlotState { suffix: "_a".into(), active: false, successful: false, bootable: true },
            SlotState { suffix: "_b".into(), active: true, successful: true, bootable: true },
        ] };
        fs::write(d.join("slots.json"), serde_json::to_vec(&dump).unwrap()).unwrap();
        fs::write(d.join("cmdline"), "quiet androidboot.slot_suffix=_b\n").unwrap();
        let b = FakeBootControl { file: d.join("slots.json"), cmdline: d.join("cmdline") };
        assert_eq!(b.dump().unwrap().current.as_deref(), Some("_b"));
        b.set_active("_a").unwrap();
        let x = b.dump().unwrap();
        assert_eq!(x.active(), Some("_a"));
        assert!(!x.slot("_a").unwrap().successful && x.slot("_b").unwrap().successful);
        fs::write(d.join("cmdline"), "androidboot.slot_suffix=_a").unwrap();
        b.mark_successful().unwrap();
        assert!(b.dump().unwrap().slot("_a").unwrap().successful);
        let _ = fs::remove_dir_all(&d);
    }
}
