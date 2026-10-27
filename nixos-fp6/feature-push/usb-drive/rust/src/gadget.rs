//! The mass_storage function in the existing configfs gadget (g1).
//!
//! The phone's gadget is composed in stage 1 (mobile-nixos: NCM as
//! configs/c.1/rndis -> functions/ncm.usb0, the coordinator's USB network)
//! and changed at run time by fp6-adb-gadget (ffs.adb) and fp6-mtp
//! (ffs.mtp). All of them serialize on /run/fp6-adb-gadget.lock (flock),
//! held only while the gadget is being changed.
//!
//! Hosting adds functions/mass_storage.fp6drive and a link to it in
//! configs/c.1, with the UDC unbound for the change (USB re-enumerates,
//! ~1 s). Exclusive mode also removes the other links (NCM, ADB, MTP) for
//! the time of hosting. A state file (/run/fp6-usb-drive/state.json) is
//! written BEFORE any change; restore() undoes exactly what hosting changed:
//! it removes our link and function and re-creates only the links WE
//! removed, so a function another component added meanwhile stays.

use serde_json::{json, Value};
use std::fs;
use std::os::fd::AsRawFd;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

pub const FUNCTION: &str = "mass_storage.fp6drive";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Cdrom,
    DiskRo,
    DiskRw,
}

impl Mode {
    pub fn parse(s: &str) -> Option<Mode> {
        match s {
            "cdrom" => Some(Mode::Cdrom),
            "disk-ro" => Some(Mode::DiskRo),
            "disk-rw" => Some(Mode::DiskRw),
            _ => None,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Mode::Cdrom => "cdrom",
            Mode::DiskRo => "disk-ro",
            Mode::DiskRw => "disk-rw",
        }
    }
    pub fn read_only(self) -> bool {
        self != Mode::DiskRw
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Link {
    pub name: String,
    pub function: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Saved {
    pub udc: String,
    pub links: Vec<Link>,
    pub removed: Vec<Link>,
    pub exclusive: bool,
    pub path: String,
    pub mode: String,
    pub since: i64,
}

impl Saved {
    pub fn to_json(&self) -> Value {
        let l = |v: &Vec<Link>| v.iter().map(|x| json!([x.name, x.function])).collect::<Vec<_>>();
        json!({"version": 1, "udc": self.udc, "links": l(&self.links), "removed": l(&self.removed),
               "exclusive": self.exclusive, "path": self.path, "mode": self.mode, "since": self.since})
    }
    pub fn from_json(v: &Value) -> Option<Saved> {
        let l = |k: &str| -> Option<Vec<Link>> {
            v.get(k)?.as_array()?.iter().map(|x| {
                let a = x.as_array()?;
                Some(Link { name: a.first()?.as_str()?.into(), function: a.get(1)?.as_str()?.into() })
            }).collect()
        };
        Some(Saved {
            udc: v.get("udc")?.as_str()?.into(),
            links: l("links")?,
            removed: l("removed")?,
            exclusive: v.get("exclusive")?.as_bool()?,
            path: v.get("path").and_then(|x| x.as_str()).unwrap_or("").into(),
            mode: v.get("mode").and_then(|x| x.as_str()).unwrap_or("").into(),
            since: v.get("since").and_then(|x| x.as_i64()).unwrap_or(0),
        })
    }
}

pub struct Gadget {
    pub root: PathBuf,
    pub config: String,
    pub udc_class: PathBuf,
    pub state_path: PathBuf,
    /// Test mode: plain directories stand in for configfs (lun.0 is created
    /// by us instead of the kernel).
    pub fake: bool,
    /// Test hook (fake mode only): fail right after linking the function.
    pub fail_after_link: bool,
}

pub struct HostSpec<'a> {
    /// What the kernel opens: "/proc/self/fd/N" of the helper's own fd.
    pub file: &'a str,
    pub mode: Mode,
    pub exclusive: bool,
    pub inquiry: &'a str,
    /// For the state file and status only.
    pub display_path: &'a str,
}

fn log(m: impl AsRef<str>) {
    eprintln!("fp6-usb-drived: {}", m.as_ref());
}

fn now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

/// SCSI INQUIRY string: vendor (8) + product (16) + revision (4).
pub fn inquiry_string(vendor: &str, product: &str, rev: &str) -> String {
    let clip = |s: &str, n: usize| -> String {
        let a: String = s.chars().filter(|c| c.is_ascii_graphic() || *c == ' ').take(n).collect();
        format!("{a:<n$}")
    };
    format!("{}{}{}", clip(vendor, 8), clip(product, 16), clip(rev, 4))
}

impl Gadget {
    pub fn new(root: impl Into<PathBuf>, state_path: impl Into<PathBuf>) -> Self {
        Gadget { root: root.into(), config: "c.1".into(), udc_class: "/sys/class/udc".into(),
                 state_path: state_path.into(), fake: false, fail_after_link: false }
    }

    fn cfg_dir(&self) -> PathBuf {
        self.root.join("configs").join(&self.config)
    }
    pub fn function_dir(&self) -> PathBuf {
        self.root.join("functions").join(FUNCTION)
    }
    fn our_link(&self) -> PathBuf {
        self.cfg_dir().join(FUNCTION)
    }
    fn lun(&self) -> PathBuf {
        self.function_dir().join("lun.0")
    }

    pub fn udc(&self) -> String {
        fs::read_to_string(self.root.join("UDC")).map(|s| s.trim().to_string()).unwrap_or_default()
    }

    /// Controller state ("configured", "not attached", ...).
    pub fn udc_state(&self, udc: &str) -> String {
        if udc.is_empty() {
            return String::new();
        }
        fs::read_to_string(self.udc_class.join(udc).join("state")).map(|s| s.trim().to_string()).unwrap_or_default()
    }

    pub fn first_controller(&self) -> Option<String> {
        let mut v: Vec<String> = fs::read_dir(&self.udc_class).ok()?
            .filter_map(|e| e.ok()).map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        v.sort();
        v.into_iter().next()
    }

    /// Links in configs/c.1 in directory order.
    pub fn links(&self) -> Vec<Link> {
        let Ok(rd) = fs::read_dir(self.cfg_dir()) else { return vec![] };
        rd.filter_map(|e| e.ok())
            .filter(|e| e.file_type().map(|t| t.is_symlink()).unwrap_or(false))
            .filter_map(|e| {
                let t = fs::read_link(e.path()).ok()?;
                Some(Link { name: e.file_name().to_string_lossy().into_owned(),
                            function: t.file_name()?.to_string_lossy().into_owned() })
            })
            .collect()
    }

    pub fn hosting(&self) -> bool {
        self.function_dir().exists() || self.our_link().symlink_metadata().is_ok()
    }

    fn unbind(&self) -> Result<(), String> {
        if !self.udc().is_empty() {
            fs::write(self.root.join("UDC"), "\n").map_err(|e| format!("unbind UDC: {e}"))?;
        }
        Ok(())
    }

    fn bind(&self, udc: &str) -> Result<(), String> {
        if udc.is_empty() || !self.udc().is_empty() {
            return Ok(());
        }
        let mut last = String::new();
        // FunctionFS functions can need a moment after a link change.
        for _ in 0..10 {
            match fs::write(self.root.join("UDC"), format!("{udc}\n")) {
                Ok(()) if self.fake || self.udc() == udc => return Ok(()),
                Ok(()) => last = "UDC did not take the controller".into(),
                Err(e) => last = e.to_string(),
            }
            std::thread::sleep(Duration::from_millis(200));
        }
        Err(format!("bind UDC {udc}: {last}"))
    }

    fn write_attr(&self, name: &str, value: &str) -> Result<(), String> {
        fs::write(self.lun().join(name), format!("{value}\n")).map_err(|e| format!("lun.0/{name}: {e}"))
    }

    fn save(&self, s: &Saved) -> Result<(), String> {
        if let Some(d) = self.state_path.parent() {
            fs::create_dir_all(d).map_err(|e| format!("state dir: {e}"))?;
        }
        let tmp = self.state_path.with_extension("tmp");
        fs::write(&tmp, s.to_json().to_string()).map_err(|e| format!("state: {e}"))?;
        fs::rename(&tmp, &self.state_path).map_err(|e| format!("state: {e}"))
    }

    pub fn load_saved(&self) -> Option<Saved> {
        let t = fs::read_to_string(&self.state_path).ok()?;
        Saved::from_json(&serde_json::from_str(&t).ok()?)
    }

    /// Adds the mass_storage function. Caller holds the gadget lock. On any
    /// error the previous gadget is restored before returning.
    pub fn host(&self, spec: &HostSpec) -> Result<Saved, String> {
        if !self.cfg_dir().is_dir() {
            return Err(format!("no gadget configuration {}", self.cfg_dir().display()));
        }
        if self.hosting() || self.state_path.exists() {
            return Err("an image is already being hosted".into());
        }
        let mut udc = self.udc();
        if udc.is_empty() {
            // Unbound with nothing linked = USB modes "charging only"; with
            // links it is another change in progress.
            if !self.links().is_empty() {
                return Err("the USB controller is not bound (another USB change in progress?)".into());
            }
            udc = self.first_controller().ok_or("no USB device controller")?;
        }
        let links = self.links();
        let removed = if spec.exclusive { links.clone() } else { vec![] };
        let saved = Saved { udc, links, removed, exclusive: spec.exclusive, path: spec.display_path.into(),
                            mode: spec.mode.name().into(), since: now() };
        self.save(&saved)?;
        match self.apply(&saved, spec) {
            Ok(()) => Ok(saved),
            Err(e) => {
                log(format!("host failed: {e}; restoring"));
                if let Err(r) = self.restore(&saved) {
                    log(format!("restore after failure: {r}"));
                    return Err(format!("{e} (restore: {r})"));
                }
                Err(e)
            }
        }
    }

    fn apply(&self, saved: &Saved, spec: &HostSpec) -> Result<(), String> {
        fs::create_dir(self.function_dir()).map_err(|e| format!("create function {FUNCTION}: {e}"))?;
        if self.fake && !self.lun().exists() {
            fs::create_dir(self.lun()).map_err(|e| e.to_string())?;
        }
        // Attributes before `file`: ro and cdrom are refused while a file is open.
        self.write_attr("removable", "1")?;
        self.write_attr("cdrom", if spec.mode == Mode::Cdrom { "1" } else { "0" })?;
        self.write_attr("ro", if spec.mode.read_only() { "1" } else { "0" })?;
        self.write_attr("nofua", "1")?;
        if self.lun().join("inquiry_string").exists() || self.fake {
            self.write_attr("inquiry_string", spec.inquiry)?;
        }
        self.write_attr("file", spec.file)?;
        self.unbind()?;
        std::os::unix::fs::symlink(self.function_dir(), self.our_link()).map_err(|e| format!("link {FUNCTION}: {e}"))?;
        if self.fake && self.fail_after_link {
            return Err("injected failure after link".into());
        }
        for l in &saved.removed {
            fs::remove_file(self.cfg_dir().join(&l.name)).map_err(|e| format!("remove link {}: {e}", l.name))?;
        }
        self.bind(&saved.udc)?;
        log(format!("hosting {} as {}{} on {}", saved.path, saved.mode,
                    if saved.exclusive { " (exclusive)" } else { "" }, saved.udc));
        Ok(())
    }

    /// Undoes host(). Caller holds the gadget lock. Idempotent.
    pub fn restore(&self, saved: &Saved) -> Result<(), String> {
        let mut errors = Vec::new();
        // Let the host see the medium go away first (best effort).
        if self.lun().join("forced_eject").exists() && !self.fake {
            let _ = fs::write(self.lun().join("forced_eject"), "1\n");
        }
        if let Err(e) = self.unbind() {
            errors.push(e);
        }
        if self.our_link().symlink_metadata().is_ok() {
            if let Err(e) = fs::remove_file(self.our_link()) {
                errors.push(format!("remove link: {e}"));
            }
        }
        let present = self.links();
        for l in &saved.removed {
            if present.iter().any(|p| p.name == l.name) {
                continue;
            }
            let target = self.root.join("functions").join(&l.function);
            if !target.is_dir() {
                errors.push(format!("function {} vanished; not re-linked", l.function));
                continue;
            }
            if let Err(e) = std::os::unix::fs::symlink(&target, self.cfg_dir().join(&l.name)) {
                errors.push(format!("re-link {}: {e}", l.name));
            }
        }
        if self.function_dir().exists() {
            if self.lun().join("file").exists() {
                let _ = fs::write(self.lun().join("file"), "\n");
            }
            if self.fake {
                let _ = fs::remove_dir_all(self.function_dir());
            } else if let Err(e) = fs::remove_dir(self.function_dir()) {
                errors.push(format!("remove function: {e}"));
            }
        }
        let udc = if saved.udc.is_empty() { self.first_controller().unwrap_or_default() } else { saved.udc.clone() };
        // Nothing left to offer (hosted from "charging only"): stay unbound,
        // an empty configuration cannot bind.
        if !self.links().is_empty() {
            if let Err(e) = self.bind(&udc) {
                errors.push(e);
            }
        }
        if errors.is_empty() || !self.hosting() {
            let _ = fs::remove_file(&self.state_path);
        }
        if errors.is_empty() {
            log(format!("gadget restored: {}", self.links().iter().map(|l| l.name.as_str()).collect::<Vec<_>>().join(" ")));
            Ok(())
        } else {
            Err(errors.join("; "))
        }
    }

    /// At start: undo a hosting session left by a crash.
    pub fn recover(&self) -> Result<bool, String> {
        if let Some(s) = self.load_saved() {
            log("found state from a previous run; restoring");
            return self.restore(&s).map(|_| true);
        }
        if self.hosting() {
            log("found a leftover mass_storage function without state; removing");
            let s = Saved { udc: self.udc(), links: self.links(), removed: vec![], exclusive: false,
                            path: String::new(), mode: String::new(), since: 0 };
            return self.restore(&s).map(|_| true);
        }
        let _ = fs::remove_file(&self.state_path);
        Ok(false)
    }

    pub fn lun_file(&self) -> String {
        fs::read_to_string(self.lun().join("file")).map(|s| s.trim().to_string()).unwrap_or_default()
    }
}

/// flock on the shared gadget lock file, with a timeout.
pub struct GadgetLock {
    _f: fs::File,
}

impl GadgetLock {
    pub fn acquire(path: &Path, timeout: Duration) -> Result<GadgetLock, String> {
        let f = fs::OpenOptions::new().create(true).write(true).truncate(false).open(path)
            .map_err(|e| format!("lock {}: {e}", path.display()))?;
        let t0 = Instant::now();
        loop {
            let r = unsafe { libc::flock(f.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
            if r == 0 {
                return Ok(GadgetLock { _f: f });
            }
            if t0.elapsed() > timeout {
                return Err("the USB gadget is busy (ADB or MTP is being switched); try again".into());
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;

    pub fn fake(tag: &str) -> (Gadget, PathBuf) {
        let base = std::env::temp_dir().join(format!("fp6ud-gadget-{}-{tag}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        let g1 = base.join("g1");
        fs::create_dir_all(g1.join("configs/c.1")).unwrap();
        fs::create_dir_all(g1.join("functions/ncm.usb0")).unwrap();
        fs::create_dir_all(g1.join("functions/ffs.adb")).unwrap();
        std::os::unix::fs::symlink(g1.join("functions/ncm.usb0"), g1.join("configs/c.1/rndis")).unwrap();
        std::os::unix::fs::symlink(g1.join("functions/ffs.adb"), g1.join("configs/c.1/adb")).unwrap();
        fs::write(g1.join("UDC"), "a600000.usb\n").unwrap();
        fs::create_dir_all(base.join("udc/a600000.usb")).unwrap();
        fs::write(base.join("udc/a600000.usb/state"), "configured\n").unwrap();
        let mut g = Gadget::new(&g1, base.join("run/state.json"));
        g.udc_class = base.join("udc");
        g.fake = true;
        (g, base)
    }

    fn names(g: &Gadget) -> Vec<String> {
        let mut v: Vec<String> = g.links().into_iter().map(|l| l.name).collect();
        v.sort();
        v
    }

    fn spec(exclusive: bool) -> HostSpec<'static> {
        HostSpec { file: "/proc/self/fd/7", mode: Mode::Cdrom, exclusive, inquiry: "FP6     DriveDroid      0100",
                   display_path: "/home/user/Images/a.iso" }
    }

    #[test]
    fn host_and_restore_keeps_other_functions() {
        let (g, base) = fake("plain");
        let s = g.host(&spec(false)).unwrap();
        assert_eq!(names(&g), vec!["adb", FUNCTION, "rndis"]);
        assert_eq!(g.udc(), "a600000.usb");
        assert_eq!(fs::read_to_string(g.lun().join("cdrom")).unwrap(), "1\n");
        assert_eq!(fs::read_to_string(g.lun().join("ro")).unwrap(), "1\n");
        assert_eq!(fs::read_to_string(g.lun().join("file")).unwrap(), "/proc/self/fd/7\n");
        assert!(g.state_path.exists());
        assert!(g.host(&spec(false)).is_err(), "second host refused");
        // ADB switched off while hosting: restore must not bring it back.
        fs::remove_file(g.cfg_dir().join("adb")).unwrap();
        g.restore(&s).unwrap();
        assert_eq!(names(&g), vec!["rndis"]);
        assert!(!g.function_dir().exists() && !g.state_path.exists());
        assert_eq!(g.udc(), "a600000.usb");
        fs::remove_dir_all(base).ok();
    }

    #[test]
    fn exclusive_removes_and_restores() {
        let (g, base) = fake("excl");
        let s = g.host(&HostSpec { mode: Mode::DiskRw, ..spec(true) }).unwrap();
        assert_eq!(names(&g), vec![FUNCTION]);
        assert_eq!(fs::read_to_string(g.lun().join("ro")).unwrap(), "0\n");
        assert_eq!(s.removed.len(), 2);
        // Restored from the state file, as after a helper crash.
        let (g2, _) = (Gadget { root: g.root.clone(), config: "c.1".into(), udc_class: g.udc_class.clone(),
                                state_path: g.state_path.clone(), fake: true, fail_after_link: false }, ());
        assert!(g2.recover().unwrap());
        assert_eq!(names(&g), vec!["adb", "rndis"]);
        assert!(!g.recover().unwrap(), "nothing left to recover");
        fs::remove_dir_all(base).ok();
    }

    #[test]
    fn failure_rolls_back() {
        let (mut g, base) = fake("fail");
        g.fail_after_link = true;
        let e = g.host(&spec(true)).unwrap_err();
        assert!(e.contains("injected"), "{e}");
        assert_eq!(names(&g), vec!["adb", "rndis"]);
        assert_eq!(g.udc(), "a600000.usb");
        assert!(!g.function_dir().exists() && !g.state_path.exists());
        fs::remove_dir_all(base).ok();
    }

    #[test]
    fn unbound_udc_refused() {
        let (g, base) = fake("unbound");
        fs::write(g.root.join("UDC"), "\n").unwrap();
        assert!(g.host(&spec(false)).is_err());
        assert!(!g.state_path.exists());
        fs::remove_dir_all(base).ok();
    }

    #[test]
    fn host_from_charging_only() {
        // USB modes "charging only": no links, UDC unbound.
        let (g, base) = fake("chargeonly");
        fs::remove_file(g.cfg_dir().join("rndis")).unwrap();
        fs::remove_file(g.cfg_dir().join("adb")).unwrap();
        fs::write(g.root.join("UDC"), "\n").unwrap();
        let s = g.host(&spec(false)).unwrap();
        assert_eq!(names(&g), vec![FUNCTION]);
        assert_eq!(g.udc(), "a600000.usb");
        g.restore(&s).unwrap();
        assert!(names(&g).is_empty());
        assert_eq!(g.udc(), "", "back to charging only");
        fs::remove_dir_all(base).ok();
    }

    #[test]
    fn inquiry() {
        let s = inquiry_string("FP6", "DriveDroid", "0100");
        assert_eq!(s.len(), 28);
        assert_eq!(s, "FP6     DriveDroid      0100");
        assert_eq!(inquiry_string("Fairphone6x", "\u{e9}vil\nname", "1").len(), 28);
    }

    #[test]
    fn saved_roundtrip() {
        let s = Saved { udc: "u".into(), links: vec![Link { name: "rndis".into(), function: "ncm.usb0".into() }],
                        removed: vec![], exclusive: true, path: "/p".into(), mode: "cdrom".into(), since: 5 };
        assert_eq!(Saved::from_json(&s.to_json()), Some(s));
    }
}
