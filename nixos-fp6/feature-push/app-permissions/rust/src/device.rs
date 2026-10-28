// SPDX-License-Identifier: GPL-3.0-or-later
//! USB device nodes handed out by the broker: which nodes qualify, what to
//! call them in the prompt, opening them for an app, and taking an open
//! handle back.
//!
//! Only nodes of devices on a USB bus qualify: raw usbfs nodes
//! (/dev/bus/usb/BBB/DDD), USB HID (hidrawN whose parent is a USB
//! interface) and USB serial (ttyACMn, ttyUSBn). Internal hidraw devices
//! (touch panel over I2C) and every other device are refused, and so are
//! hubs (raw hub access is never needed by an app).
//!
//! Taking a handle back: writing 0 and then 1 to the USB device's
//! `authorized` attribute disconnects and re-enumerates it; every open
//! usbfs / hidraw / tty descriptor of it then fails with ENODEV. (This also
//! affects other holders of the same device, there is no per-fd revoke.)

use std::fs;
use std::os::unix::fs::{FileTypeExt, MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Usbfs,
    Hidraw,
    Serial,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeviceInfo {
    pub node: String,
    pub kind: Kind,
    pub rdev: u64,
    /// The USB device's sysfs directory (has `authorized`, `idVendor`).
    pub usb_sysfs: PathBuf,
    pub vendor_id: String,
    pub product_id: String,
    pub product: String,
    pub manufacturer: String,
}

impl DeviceInfo {
    /// "Arduino Uno (2341:0043)" / "USB device 2341:0043".
    pub fn label(&self) -> String {
        let ids = format!("{}:{}", self.vendor_id, self.product_id);
        let name = if self.product.is_empty() {
            self.manufacturer.clone()
        } else if self.manufacturer.is_empty() || self.product.contains(&self.manufacturer) {
            self.product.clone()
        } else {
            format!("{} {}", self.manufacturer, self.product)
        };
        let kind = match self.kind {
            Kind::Usbfs => "",
            Kind::Hidraw => " · HID",
            Kind::Serial => " · serial",
        };
        if name.is_empty() {
            format!("USB device {ids}{kind}")
        } else {
            format!("{name} ({ids}){kind}")
        }
    }
}

/// Which kind of node a /dev path is, by name only.
pub fn classify(node: &str) -> Option<Kind> {
    let digits = |s: &str, n: Option<usize>| {
        !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()) && n.is_none_or(|n| s.len() == n) && s.len() <= 4
    };
    if let Some(rest) = node.strip_prefix("/dev/bus/usb/") {
        let (bus, dev) = rest.split_once('/')?;
        return (digits(bus, Some(3)) && digits(dev, Some(3))).then_some(Kind::Usbfs);
    }
    if let Some(n) = node.strip_prefix("/dev/hidraw") {
        return digits(n, None).then_some(Kind::Hidraw);
    }
    if let Some(n) = node.strip_prefix("/dev/ttyACM").or_else(|| node.strip_prefix("/dev/ttyUSB")) {
        return digits(n, None).then_some(Kind::Serial);
    }
    None
}

fn read_attr(dir: &Path, name: &str) -> String {
    fs::read_to_string(dir.join(name))
        .map(|s| s.trim().chars().filter(|c| !c.is_control()).take(80).collect())
        .unwrap_or_default()
}

/// The USB *device* directory (not an interface) at or above a sysfs path.
fn usb_device_dir(start: &Path, sys_root: &Path) -> Option<PathBuf> {
    let mut p = start.to_path_buf();
    for _ in 0..16 {
        if p.join("idVendor").is_file() && p.join("authorized").is_file() && p.join("bDeviceClass").is_file() {
            return Some(p);
        }
        if !p.pop() || p == sys_root || !p.starts_with(sys_root) {
            break;
        }
    }
    None
}

/// Look up a node: name rules, a real character device, a USB ancestor.
/// `dev_root`/`sys_root` are "/" in production (tests use a fake tree).
pub fn inspect(node: &str, dev_root: &Path, sys_root: &Path) -> Result<DeviceInfo, String> {
    let kind = classify(node).ok_or("not a USB device node")?;
    let path = dev_root.join(node.trim_start_matches('/'));
    let meta = fs::symlink_metadata(&path).map_err(|e| format!("{node}: {e}"))?;
    if !meta.file_type().is_char_device() {
        return Err(format!("{node}: not a character device"));
    }
    let rdev = meta.rdev();
    let (maj, min) = (libc::major(rdev as libc::dev_t), libc::minor(rdev as libc::dev_t));
    let link = sys_root.join(format!("sys/dev/char/{maj}:{min}"));
    let real = fs::canonicalize(&link).map_err(|e| format!("{node}: no sysfs entry ({e})"))?;
    let sys_devices = sys_root.join("sys/devices");
    if !real.starts_with(&sys_devices) {
        return Err(format!("{node}: unexpected sysfs path"));
    }
    let usb = usb_device_dir(&real, &sys_devices).ok_or_else(|| format!("{node}: not on a USB bus"))?;
    if read_attr(&usb, "bDeviceClass") == "09" {
        return Err(format!("{node}: USB hubs are not handed out"));
    }
    Ok(DeviceInfo {
        node: node.to_owned(),
        kind,
        rdev,
        vendor_id: read_attr(&usb, "idVendor"),
        product_id: read_attr(&usb, "idProduct"),
        product: read_attr(&usb, "product"),
        manufacturer: read_attr(&usb, "manufacturer"),
        usb_sysfs: usb,
    })
}

/// Open for the app. The descriptor is re-checked against the inspected
/// device number (the node could have been replaced in between).
pub fn open(info: &DeviceInfo, dev_root: &Path, write: bool) -> Result<fs::File, String> {
    let path = dev_root.join(info.node.trim_start_matches('/'));
    let mut flags = libc::O_CLOEXEC | libc::O_NOFOLLOW;
    if info.kind == Kind::Serial {
        flags |= libc::O_NOCTTY | libc::O_NONBLOCK;
    }
    let f = fs::OpenOptions::new()
        .read(true)
        .write(write)
        .custom_flags(flags)
        .open(&path)
        .map_err(|e| format!("{}: {e}", info.node))?;
    let m = f.metadata().map_err(|e| e.to_string())?;
    if !m.file_type().is_char_device() || m.rdev() != info.rdev {
        return Err(format!("{}: device changed while opening", info.node));
    }
    Ok(f)
}

/// Take every open handle of the device back (see module docs).
pub fn revoke(usb_sysfs: &Path, sys_root: &Path) -> Result<(), String> {
    if !usb_sysfs.starts_with(sys_root.join("sys/devices")) || !usb_sysfs.join("idVendor").is_file() {
        return Err("not a USB device".into());
    }
    let a = usb_sysfs.join("authorized");
    fs::write(&a, "0").map_err(|e| format!("{}: {e}", a.display()))?;
    std::thread::sleep(std::time::Duration::from_millis(200));
    fs::write(&a, "1").map_err(|e| format!("{}: {e}", a.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names() {
        assert_eq!(classify("/dev/bus/usb/001/004"), Some(Kind::Usbfs));
        assert_eq!(classify("/dev/bus/usb/001/4"), None);
        assert_eq!(classify("/dev/bus/usb/001/004/x"), None);
        assert_eq!(classify("/dev/bus/usb/../../sda"), None);
        assert_eq!(classify("/dev/hidraw3"), Some(Kind::Hidraw));
        assert_eq!(classify("/dev/hidraw"), None);
        assert_eq!(classify("/dev/ttyACM0"), Some(Kind::Serial));
        assert_eq!(classify("/dev/ttyUSB12"), Some(Kind::Serial));
        assert_eq!(classify("/dev/ttyS0"), None);
        assert_eq!(classify("/dev/ttyACM0/../sda"), None);
        assert_eq!(classify("/dev/video0"), None);
    }

    #[test]
    fn labels() {
        let mut d = DeviceInfo {
            node: "/dev/ttyACM0".into(),
            kind: Kind::Serial,
            rdev: 0,
            usb_sysfs: PathBuf::new(),
            vendor_id: "2341".into(),
            product_id: "0043".into(),
            product: "Uno".into(),
            manufacturer: "Arduino".into(),
        };
        assert_eq!(d.label(), "Arduino Uno (2341:0043) · serial");
        d.product = "Arduino Uno".into();
        d.kind = Kind::Usbfs;
        assert_eq!(d.label(), "Arduino Uno (2341:0043)");
        d.product.clear();
        d.manufacturer.clear();
        assert_eq!(d.label(), "USB device 2341:0043");
    }
}
