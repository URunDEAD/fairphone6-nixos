//! USB-C port and USB power supply state (read from sysfs; only
//! `write_power_role` writes).
//!
//! FP6 (kernel 7.2, read on the phone 2026-10-27): one UCSI port
//! (/sys/class/typec/port0 via pmic_glink.ucsi.0, ADSP firmware), dual-role
//! ("source [sink]", "host [device]"), PD 3.0, Type-C 1.3. typec_operations
//! pr_set/dr_set are the UCSI SET_PDR / SET_UOR commands. The typec class
//! accepts a power_role write only when power_operation_mode is
//! "usb_power_delivery" (a PD contract with the partner), otherwise EIO.
//! The USB supply (qcom-battmgr-usb) reports the BC1.2 type: SDP/CDP = a
//! computer's data port, DCP = a wall charger, PD* = negotiated over PD.

use std::fs;
use std::path::{Path, PathBuf};

fn read(p: &Path) -> Option<String> {
    fs::read_to_string(p).ok().map(|s| s.trim().to_string())
}

/// "source [sink]" -> (["source", "sink"], Some("sink")); "[sink]" -> (["sink"], Some("sink")).
pub fn bracketed(s: &str) -> (Vec<String>, Option<String>) {
    let mut all = vec![];
    let mut sel = None;
    for w in s.split_whitespace() {
        if let Some(x) = w.strip_prefix('[').and_then(|x| x.strip_suffix(']')) {
            sel = Some(x.to_string());
            all.push(x.to_string());
        } else {
            all.push(w.to_string());
        }
    }
    (all, sel)
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Port {
    pub present: bool,
    /// Both power roles are offered (TYPEC_PORT_DRP).
    pub drp: bool,
    /// "source" | "sink" | "" (no port).
    pub power_role: String,
    /// "host" | "device" | "".
    pub data_role: String,
    /// "default" | "1.5A" | "3.0A" | "usb_power_delivery".
    pub opmode: String,
    pub pd_revision: String,
    pub partner: bool,
    pub partner_pd: bool,
    /// Discover Identity of the partner (id_header + product), if the
    /// driver registered it. Used only as a hashed per-host key.
    pub partner_identity: Option<String>,
}

impl Port {
    pub fn read(class: &Path, name: &str) -> Port {
        let p = class.join(name);
        if !p.is_dir() {
            return Port::default();
        }
        let (roles, role) = bracketed(&read(&p.join("power_role")).unwrap_or_default());
        let (_, data) = bracketed(&read(&p.join("data_role")).unwrap_or_default());
        let partner_dir = p.join(format!("{name}-partner"));
        let partner = partner_dir.is_dir();
        let partner_pd = partner && read(&partner_dir.join("supports_usb_power_delivery")).as_deref() == Some("yes");
        let partner_identity = if partner {
            let id = partner_dir.join("identity");
            let h = read(&id.join("id_header")).unwrap_or_default();
            let pr = read(&id.join("product")).unwrap_or_default();
            let zero = |s: &str| s.is_empty() || s.trim_start_matches("0x").chars().all(|c| c == '0');
            if zero(&h) && zero(&pr) { None } else { Some(format!("{h}:{pr}")) }
        } else {
            None
        };
        Port {
            present: true,
            drp: roles.len() == 2,
            power_role: role.unwrap_or_default(),
            data_role: data.unwrap_or_default(),
            opmode: read(&p.join("power_operation_mode")).unwrap_or_default(),
            pd_revision: read(&p.join("usb_power_delivery_revision")).unwrap_or_default(),
            partner,
            partner_pd,
            partner_identity,
        }
    }

    /// Ok when a power role swap can be asked for now; Err(reason) otherwise.
    pub fn power_swap_possible(&self) -> Result<(), &'static str> {
        if !self.present {
            return Err("no USB-C port in sysfs");
        }
        if !self.drp {
            return Err("the port is not dual-role");
        }
        if !self.partner {
            return Err("nothing connected");
        }
        if self.opmode != "usb_power_delivery" || !self.partner_pd {
            return Err("the connected device does not use USB Power Delivery, so the roles cannot be swapped");
        }
        Ok(())
    }
}

/// Writes power_role ("source" | "sink"). The kernel blocks until the UCSI
/// swap completes (5 s timeout).
pub fn write_power_role(class: &Path, name: &str, role: &str) -> Result<(), String> {
    if role != "source" && role != "sink" {
        return Err("role must be source or sink".into());
    }
    match fs::write(class.join(name).join("power_role"), format!("{role}\n")) {
        Ok(()) => Ok(()),
        Err(e) => Err(match e.raw_os_error() {
            Some(libc::EIO) => "the connected device has no Power Delivery contract (the kernel refused the swap)".into(),
            Some(libc::EOPNOTSUPP) => "power role swap is not supported by this kernel/port".into(),
            Some(libc::ETIMEDOUT) => "the charger firmware did not finish the swap in time".into(),
            Some(libc::EPROTO) => "the swap failed and the port was reset".into(),
            Some(libc::ENOTCONN) => "nothing connected".into(),
            _ => format!("power_role: {e}"),
        }),
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Supply {
    pub online: bool,
    /// Selected entry of usb_type ("SDP", "DCP", "PD", ...), "" if unknown.
    pub usb_type: String,
    pub current_ua: i64,
    pub voltage_uv: i64,
}

impl Supply {
    /// `dir`: e.g. /sys/class/power_supply/qcom-battmgr-usb.
    pub fn read(dir: &Path) -> Supply {
        let n = |f: &str| read(&dir.join(f)).and_then(|s| s.parse::<i64>().ok()).unwrap_or(0);
        let (_, t) = bracketed(&read(&dir.join("usb_type")).unwrap_or_default());
        Supply {
            online: read(&dir.join("online")).as_deref() == Some("1"),
            usb_type: t.filter(|t| t != "Unknown").unwrap_or_default(),
            current_ua: n("current_now"),
            voltage_uv: n("voltage_now"),
        }
    }

    /// The first type=USB supply whose name contains `prefer`, else the first USB one.
    pub fn find(class: &Path, prefer: &str) -> Option<PathBuf> {
        let mut v: Vec<PathBuf> = fs::read_dir(class).ok()?.filter_map(|e| e.ok()).map(|e| e.path())
            .filter(|p| read(&p.join("type")).as_deref() == Some("USB")).collect();
        v.sort();
        v.iter().find(|p| p.file_name().map(|n| n.to_string_lossy().contains(prefer)).unwrap_or(false))
            .cloned().or_else(|| v.into_iter().next())
    }

    pub fn data_port(&self) -> bool {
        self.online && (self.usb_type == "SDP" || self.usb_type == "CDP")
    }
}

/// Who is on the other end of the cable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Peer {
    None,
    /// Power only, as far as we can tell (wall charger, or a PD host while
    /// no USB function is bound: it is only recognized once it enumerates).
    Charger,
    /// A USB host (computer): the gadget is configured or BC1.2 says SDP/CDP.
    Host,
    /// We are the power source: an accessory, or a phone after a role swap.
    Device,
}

impl Peer {
    pub fn name(self) -> &'static str {
        match self {
            Peer::None => "none",
            Peer::Charger => "charger",
            Peer::Host => "host",
            Peer::Device => "device",
        }
    }
}

pub fn classify(port: &Port, supply: &Supply, udc_state: &str) -> Peer {
    if port.partner && port.power_role == "source" {
        return Peer::Device;
    }
    if udc_state == "configured" || supply.data_port() {
        return Peer::Host;
    }
    if supply.online || port.partner {
        return Peer::Charger;
    }
    Peer::None
}

/// A short stable key for per-host memory (FNV-1a 64 of the PD identity),
/// so no raw identifiers are stored.
pub fn host_key(port: &Port) -> Option<String> {
    let id = port.partner_identity.as_ref()?;
    let mut h: u64 = 0xcbf29ce484222325;
    for b in id.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    Some(format!("pd-{h:016x}"))
}

#[cfg(test)]
pub mod tests {
    use super::*;

    pub fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("fp6um-{}-{tag}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    /// The FP6 as read on 2026-10-27, connected to a laptop USB-A port (no PD).
    pub fn fp6_port(class: &Path, pd: bool) {
        let p = class.join("port0");
        fs::create_dir_all(p.join("port0-partner")).unwrap();
        fs::write(p.join("power_role"), "source [sink]\n").unwrap();
        fs::write(p.join("data_role"), "host [device]\n").unwrap();
        fs::write(p.join("power_operation_mode"), if pd { "usb_power_delivery\n" } else { "default\n" }).unwrap();
        fs::write(p.join("usb_power_delivery_revision"), "3.0\n").unwrap();
        fs::write(p.join("port0-partner/supports_usb_power_delivery"), if pd { "yes\n" } else { "no\n" }).unwrap();
    }

    pub fn fp6_supply(class: &Path, usb_type: &str, online: bool) -> PathBuf {
        let d = class.join("qcom-battmgr-usb");
        fs::create_dir_all(&d).unwrap();
        fs::write(d.join("type"), "USB\n").unwrap();
        fs::write(d.join("online"), if online { "1\n" } else { "0\n" }).unwrap();
        let all = ["Unknown", "SDP", "DCP", "CDP", "ACA", "C", "PD", "PD_DRP", "PD_PPS", "BrickID"];
        let s: Vec<String> = all.iter().map(|t| if *t == usb_type { format!("[{t}]") } else { t.to_string() }).collect();
        fs::write(d.join("usb_type"), s.join(" ") + "\n").unwrap();
        fs::write(d.join("current_now"), "244000\n").unwrap();
        fs::write(d.join("voltage_now"), "5025000\n").unwrap();
        let bat = class.join("qcom-battmgr-bat");
        fs::create_dir_all(&bat).unwrap();
        fs::write(bat.join("type"), "Battery\n").unwrap();
        d
    }

    #[test]
    fn bracket_parsing() {
        assert_eq!(bracketed("source [sink]"), (vec!["source".into(), "sink".into()], Some("sink".into())));
        assert_eq!(bracketed("[source]"), (vec!["source".into()], Some("source".into())));
        assert_eq!(bracketed(""), (vec![], None));
    }

    #[test]
    fn fp6_laptop_no_pd() {
        let d = tmp("port-nopd");
        fp6_port(&d.join("typec"), false);
        let s = Supply::read(&fp6_supply(&d.join("ps"), "SDP", true));
        let p = Port::read(&d.join("typec"), "port0");
        assert!(p.present && p.drp && p.partner && !p.partner_pd);
        assert_eq!((p.power_role.as_str(), p.data_role.as_str()), ("sink", "device"));
        assert!(p.power_swap_possible().unwrap_err().contains("Power Delivery"));
        assert_eq!(s.usb_type, "SDP");
        assert_eq!(classify(&p, &s, "not attached"), Peer::Host, "SDP is a host even with the UDC unbound");
        assert_eq!(host_key(&p), None);
        fs::remove_dir_all(d).ok();
    }

    #[test]
    fn pd_partner_and_identity() {
        let d = tmp("port-pd");
        fp6_port(&d.join("typec"), true);
        let id = d.join("typec/port0/port0-partner/identity");
        fs::create_dir_all(&id).unwrap();
        fs::write(id.join("id_header"), "0x6c0018d1\n").unwrap();
        fs::write(id.join("product"), "0x4ee70000\n").unwrap();
        let p = Port::read(&d.join("typec"), "port0");
        assert!(p.power_swap_possible().is_ok());
        let k = host_key(&p).unwrap();
        assert!(k.starts_with("pd-") && k.len() == 19);
        // PD charger (DCP/PD), UDC idle: charger
        let s = Supply::read(&fp6_supply(&d.join("ps"), "PD", true));
        assert_eq!(classify(&p, &s, "not attached"), Peer::Charger);
        assert_eq!(classify(&p, &s, "configured"), Peer::Host);
        // After a swap the phone is the source.
        fs::write(d.join("typec/port0/power_role"), "[source] sink\n").unwrap();
        let p = Port::read(&d.join("typec"), "port0");
        assert_eq!(classify(&p, &s, "configured"), Peer::Device);
        fs::remove_dir_all(d).ok();
    }

    #[test]
    fn zero_identity_ignored_and_nothing_connected() {
        let d = tmp("port-zero");
        fp6_port(&d.join("typec"), true);
        let id = d.join("typec/port0/port0-partner/identity");
        fs::create_dir_all(&id).unwrap();
        fs::write(id.join("id_header"), "0x00000000\n").unwrap();
        fs::write(id.join("product"), "0x00000000\n").unwrap();
        assert_eq!(Port::read(&d.join("typec"), "port0").partner_identity, None);
        fs::remove_dir_all(d.join("typec/port0/port0-partner")).unwrap();
        let p = Port::read(&d.join("typec"), "port0");
        let s = Supply::read(&fp6_supply(&d.join("ps"), "Unknown", false));
        assert_eq!(classify(&p, &s, "not attached"), Peer::None);
        assert_eq!(p.power_swap_possible(), Err("nothing connected"));
        assert_eq!(Supply::find(&d.join("ps"), "battmgr-usb").unwrap().file_name().unwrap(), "qcom-battmgr-usb");
        fs::remove_dir_all(d).ok();
    }

    #[test]
    fn power_role_write_on_plain_file() {
        let d = tmp("port-write");
        fp6_port(&d.join("typec"), true);
        write_power_role(&d.join("typec"), "port0", "source").unwrap();
        assert_eq!(fs::read_to_string(d.join("typec/port0/power_role")).unwrap(), "source\n");
        assert!(write_power_role(&d.join("typec"), "port0", "both").is_err());
        fs::remove_dir_all(d).ok();
    }
}
