//! The configfs gadget g1 as fp6-usbmoded sees it.
//!
//! Composition on the FP6 (stage 1, mobile-nixos): configs/c.1/rndis ->
//! functions/ncm.usb0 (the usb0 network, 172.16.42.1). At run time:
//!   ffs.adb               fp6-adbd.service (adb/fp6-adb-gadget)
//!   ffs.mtp               fp6-mtp.service (mtp-fp6.nix, umtprd)
//!   mass_storage.fp6drive fp6-usb-drived (usb-drive)
//! fp6-usbmoded changes only the NCM link itself; the FunctionFS functions
//! are switched by starting/stopping their services (their daemons must
//! write descriptors before the UDC binds). Every writer serializes on
//! /run/fp6-adb-gadget.lock. A configuration without any link cannot be
//! bound, so "no data" = UDC unbound (the phone still charges).

use crate::model::{Func, FuncSet};
use std::fs;
use std::os::fd::AsRawFd;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

pub const NCM_LINK: &str = "rndis";
pub const NCM_FUNCTION: &str = "ncm.usb0";

#[derive(Debug, Clone, PartialEq)]
pub struct Link {
    pub name: String,
    pub function: String,
}

pub fn func_of(function: &str) -> Option<Func> {
    if function == NCM_FUNCTION || function.starts_with("ncm.") || function.starts_with("rndis.") {
        Some(Func::Ncm)
    } else if function == "ffs.mtp" {
        Some(Func::Mtp)
    } else if function == "ffs.adb" {
        Some(Func::Adb)
    } else if function.starts_with("mass_storage.") {
        Some(Func::MassStorage)
    } else {
        None
    }
}

pub struct Gadget {
    pub root: PathBuf,
    pub config: String,
    pub udc_class: PathBuf,
    /// Tests: plain directories stand in for configfs.
    pub fake: bool,
    /// Tests: fail the next bind.
    pub fail_bind: std::cell::Cell<bool>,
}

impl Gadget {
    pub fn new(root: impl Into<PathBuf>, udc_class: impl Into<PathBuf>) -> Gadget {
        Gadget { root: root.into(), config: "c.1".into(), udc_class: udc_class.into(), fake: false,
                 fail_bind: std::cell::Cell::new(false) }
    }

    pub fn cfg_dir(&self) -> PathBuf {
        self.root.join("configs").join(&self.config)
    }

    pub fn exists(&self) -> bool {
        self.cfg_dir().is_dir()
    }

    pub fn links(&self) -> Vec<Link> {
        let Ok(rd) = fs::read_dir(self.cfg_dir()) else { return vec![] };
        let mut v: Vec<Link> = rd.filter_map(|e| e.ok())
            .filter(|e| e.file_type().map(|t| t.is_symlink()).unwrap_or(false))
            .filter_map(|e| {
                let t = fs::read_link(e.path()).ok()?;
                Some(Link { name: e.file_name().to_string_lossy().into_owned(),
                            function: t.file_name()?.to_string_lossy().into_owned() })
            })
            .collect();
        v.sort_by(|a, b| a.name.cmp(&b.name));
        v
    }

    /// Functions linked into the configuration (whether bound or not).
    pub fn functions(&self) -> FuncSet {
        self.links().iter().filter_map(|l| func_of(&l.function)).collect()
    }

    pub fn udc(&self) -> String {
        fs::read_to_string(self.root.join("UDC")).map(|s| s.trim().to_string()).unwrap_or_default()
    }

    pub fn first_controller(&self) -> Option<String> {
        let mut v: Vec<String> = fs::read_dir(&self.udc_class).ok()?
            .filter_map(|e| e.ok()).map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        v.sort();
        v.into_iter().next()
    }

    /// Controller state ("configured", "not attached", ...) of the bound or first controller.
    pub fn udc_state(&self) -> String {
        let u = self.udc();
        let c = if u.is_empty() { self.first_controller().unwrap_or_default() } else { u };
        if c.is_empty() {
            return String::new();
        }
        fs::read_to_string(self.udc_class.join(c).join("state")).map(|s| s.trim().to_string()).unwrap_or_default()
    }

    fn unbind(&self) -> Result<(), String> {
        if !self.udc().is_empty() {
            fs::write(self.root.join("UDC"), "\n").map_err(|e| format!("unbind UDC: {e}"))?;
        }
        Ok(())
    }

    /// Binds the UDC if any function is linked (an empty configuration
    /// cannot bind; that state is "charge only").
    fn bind_if_linked(&self, udc: &str) -> Result<(), String> {
        if self.links().is_empty() || !self.udc().is_empty() {
            return Ok(());
        }
        let c = if udc.is_empty() { self.first_controller().ok_or("no USB device controller")? } else { udc.to_string() };
        if self.fail_bind.replace(false) {
            return Err(format!("bind UDC {c}: injected failure"));
        }
        let mut last = String::new();
        for _ in 0..10 {
            match fs::write(self.root.join("UDC"), format!("{c}\n")) {
                Ok(()) if self.fake || self.udc() == c => return Ok(()),
                Ok(()) => last = "UDC did not take the controller".into(),
                Err(e) => last = e.to_string(),
            }
            std::thread::sleep(Duration::from_millis(200));
        }
        Err(format!("bind UDC {c}: {last}"))
    }

    pub fn ncm_linked(&self) -> bool {
        self.links().iter().any(|l| func_of(&l.function) == Some(Func::Ncm))
    }

    /// Adds or removes the NCM link (caller holds the gadget lock). The UDC
    /// is unbound for the change (~1 s re-enumeration) and rebound if any
    /// function is left. On failure the previous links and binding come back.
    pub fn set_ncm(&self, on: bool) -> Result<(), String> {
        if !self.exists() {
            return Err(format!("no gadget configuration {}", self.cfg_dir().display()));
        }
        if self.ncm_linked() == on {
            // Make sure a lone NCM is bound (e.g. after a partial earlier change).
            return self.bind_if_linked(&self.udc());
        }
        let udc = self.udc();
        let before = self.links();
        let r = (|| {
            self.unbind()?;
            if on {
                let f = self.root.join("functions").join(NCM_FUNCTION);
                if !f.is_dir() {
                    fs::create_dir(&f).map_err(|e| format!("create {NCM_FUNCTION}: {e}"))?;
                }
                std::os::unix::fs::symlink(&f, self.cfg_dir().join(NCM_LINK)).map_err(|e| format!("link {NCM_LINK}: {e}"))?;
            } else {
                for l in self.links().iter().filter(|l| func_of(&l.function) == Some(Func::Ncm)) {
                    fs::remove_file(self.cfg_dir().join(&l.name)).map_err(|e| format!("remove link {}: {e}", l.name))?;
                }
            }
            self.bind_if_linked(&udc)
        })();
        if let Err(e) = r {
            let rb = self.restore_links(&before, &udc);
            return Err(match rb {
                Ok(()) => format!("{e} (gadget restored)"),
                Err(r) => format!("{e} (restore failed: {r})"),
            });
        }
        Ok(())
    }

    /// Puts the NCM link back as it was in `before` and rebinds.
    fn restore_links(&self, before: &[Link], udc: &str) -> Result<(), String> {
        let _ = self.unbind();
        let had = before.iter().find(|l| func_of(&l.function) == Some(Func::Ncm));
        for l in self.links().iter().filter(|l| func_of(&l.function) == Some(Func::Ncm)) {
            if had.map(|h| h.name != l.name).unwrap_or(true) {
                let _ = fs::remove_file(self.cfg_dir().join(&l.name));
            }
        }
        if let Some(h) = had {
            if self.cfg_dir().join(&h.name).symlink_metadata().is_err() {
                std::os::unix::fs::symlink(self.root.join("functions").join(&h.function), self.cfg_dir().join(&h.name))
                    .map_err(|e| format!("re-link {}: {e}", h.name))?;
            }
        }
        if udc.is_empty() {
            return Ok(());
        }
        self.bind_if_linked(udc)
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
            if unsafe { libc::flock(f.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
                return Ok(GadgetLock { _f: f });
            }
            if t0.elapsed() > timeout {
                return Err("the USB gadget is busy (another USB change in progress); try again".into());
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::typec::tests::tmp;

    /// The FP6 gadget as read on 2026-10-27 (NCM only, bound, configured).
    pub fn fake(tag: &str) -> (Gadget, PathBuf) {
        let base = tmp(&format!("cfs-{tag}"));
        let g1 = base.join("g1");
        fs::create_dir_all(g1.join("configs/c.1/strings/0x409")).unwrap();
        fs::write(g1.join("configs/c.1/MaxPower"), "2\n").unwrap();
        fs::create_dir_all(g1.join("functions/ncm.usb0")).unwrap();
        std::os::unix::fs::symlink(g1.join("functions/ncm.usb0"), g1.join("configs/c.1/rndis")).unwrap();
        fs::write(g1.join("UDC"), "a600000.usb\n").unwrap();
        fs::create_dir_all(base.join("udc/a600000.usb")).unwrap();
        fs::write(base.join("udc/a600000.usb/state"), "configured\n").unwrap();
        let mut g = Gadget::new(&g1, base.join("udc"));
        g.fake = true;
        (g, base)
    }

    pub fn link(g: &Gadget, name: &str, function: &str) {
        fs::create_dir_all(g.root.join("functions").join(function)).unwrap();
        std::os::unix::fs::symlink(g.root.join("functions").join(function), g.cfg_dir().join(name)).unwrap();
    }

    fn names(g: &Gadget) -> Vec<String> {
        g.links().into_iter().map(|l| l.name).collect()
    }

    #[test]
    fn read_fp6_gadget() {
        let (g, base) = fake("read");
        assert_eq!(g.functions(), [Func::Ncm].into_iter().collect());
        assert_eq!(g.udc(), "a600000.usb");
        assert_eq!(g.udc_state(), "configured");
        link(&g, "mtp", "ffs.mtp");
        link(&g, "fp6drive", "mass_storage.fp6drive");
        assert_eq!(g.functions(), [Func::Ncm, Func::Mtp, Func::MassStorage].into_iter().collect());
        fs::remove_dir_all(base).ok();
    }

    #[test]
    fn ncm_off_last_function_leaves_udc_unbound() {
        let (g, base) = fake("off");
        g.set_ncm(false).unwrap();
        assert!(names(&g).is_empty());
        assert_eq!(g.udc(), "", "an empty configuration is not bound");
        assert_eq!(g.udc_state(), "configured", "state falls back to the first controller");
        g.set_ncm(true).unwrap();
        assert_eq!(names(&g), vec!["rndis"]);
        assert_eq!(g.udc(), "a600000.usb");
        fs::remove_dir_all(base).ok();
    }

    #[test]
    fn ncm_off_keeps_other_functions_bound() {
        let (g, base) = fake("keep");
        link(&g, "adb", "ffs.adb");
        g.set_ncm(false).unwrap();
        assert_eq!(names(&g), vec!["adb"]);
        assert_eq!(g.udc(), "a600000.usb");
        g.set_ncm(false).unwrap();
        fs::remove_dir_all(base).ok();
    }

    #[test]
    fn bind_failure_rolls_back() {
        let (g, base) = fake("rollback");
        link(&g, "adb", "ffs.adb");
        g.fail_bind.set(true);
        let e = g.set_ncm(false).unwrap_err();
        assert!(e.contains("injected") && e.contains("restored"), "{e}");
        assert_eq!(names(&g), vec!["adb", "rndis"]);
        assert_eq!(g.udc(), "a600000.usb");
        // Same for adding: start from unbound + no NCM function dir.
        g.set_ncm(false).unwrap();
        fs::remove_dir_all(g.root.join("functions/ncm.usb0")).unwrap();
        g.fail_bind.set(true);
        assert!(g.set_ncm(true).is_err());
        assert_eq!(names(&g), vec!["adb"]);
        g.set_ncm(true).unwrap();
        assert_eq!(names(&g), vec!["adb", "rndis"]);
        fs::remove_dir_all(base).ok();
    }

    #[test]
    fn missing_gadget() {
        let (g, base) = fake("missing");
        fs::remove_dir_all(g.cfg_dir()).unwrap();
        assert!(g.set_ncm(true).unwrap_err().contains("no gadget"));
        fs::remove_dir_all(base).ok();
    }

    #[test]
    fn lock_is_exclusive() {
        let d = tmp("lock");
        let p = d.join("lock");
        let a = GadgetLock::acquire(&p, Duration::from_millis(10)).unwrap();
        assert!(GadgetLock::acquire(&p, Duration::from_millis(150)).is_err());
        drop(a);
        GadgetLock::acquire(&p, Duration::from_millis(10)).unwrap();
        fs::remove_dir_all(d).ok();
    }
}
