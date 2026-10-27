//! qbootctl (linux-msm/qbootctl 0.2.2, nixpkgs `qbootctl`): output parsing
//! and the few calls the daemon makes.
//!
//! What the calls write (qbootctl 0.2.2 source, bootctrl_impl.c/gpt-utils.h):
//!  * `-s SLOT`: first opens /dev/bsg/ufs-bsg0 (fails before writing if it
//!    cannot), then for each pair in a FIXED list (abl aop apdp cmnlib
//!    cmnlib64 devcfg dtbo hyp keymaster msadp qupfw storsec tz vbmeta
//!    vbmeta_system boot system vendor modem system_ext product, where both
//!    _a and _b exist) swaps the GPT type GUIDs so the new slot gets the
//!    "active" one and sets its attribute byte (bits 48-55) to 0xF (priority
//!    3, active, retry count 1, successful and unbootable cleared); clears the
//!    active bit of the other slot. Primary and backup GPT of every LUN
//!    touched, CRCs recomputed. Then sets the UFS bBootLunEn attribute (LUN A
//!    or B: which xbl the primary boot loader runs).
//!  * `-m [SLOT]`: sets the successful bit on the same fixed list (and clears
//!    unbootable first if set); writes nothing if already successful.
//!  * `-u [SLOT]`: sets the unbootable bit.
//!  * no arguments: read-only dump (the format parsed here).
//! Partitions outside the list (on SM7635 for example vendor_boot, init_boot,
//! xbl_config, shrm, cpucp, uefi, dsp, bluetooth, featenabler, imagefv) are
//! NOT touched by -s; see the README's slot safety section.

use crate::util;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct SlotState {
    pub suffix: String,
    pub active: bool,
    pub successful: bool,
    pub bootable: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct Dump {
    /// "_a" / "_b" (None when qbootctl prints N/A).
    pub current: Option<String>,
    pub slots: Vec<SlotState>,
}

impl Dump {
    pub fn slot(&self, suffix: &str) -> Option<&SlotState> {
        self.slots.iter().find(|s| s.suffix == suffix)
    }
    pub fn active(&self) -> Option<&str> {
        let a: Vec<&SlotState> = self.slots.iter().filter(|s| s.active).collect();
        if a.len() == 1 { Some(&a[0].suffix) } else { None }
    }
}

fn flag(v: &str) -> Result<bool, String> {
    match v.trim() {
        "1" => Ok(true),
        "0" => Ok(false),
        x => Err(format!("unexpected qbootctl value {x:?}")),
    }
}

/// Parses `qbootctl` (no arguments):
/// ```text
/// Current slot: _b
/// SLOT _a:
///     Active      : 0
///     Successful  : 0
///     Bootable    : 1
/// SLOT _b:
///     ...
/// ```
pub fn parse_dump(out: &str) -> Result<Dump, String> {
    let mut d = Dump::default();
    let mut cur: Option<SlotState> = None;
    let mut saw_current = false;
    for line in out.lines() {
        let l = line.trim();
        if l.is_empty() {
            continue;
        }
        if let Some(v) = l.strip_prefix("Current slot:") {
            let v = v.trim();
            d.current = if v == "_a" || v == "_b" { Some(v.to_string()) } else { None };
            saw_current = true;
        } else if let Some(v) = l.strip_prefix("SLOT ") {
            if let Some(s) = cur.take() {
                d.slots.push(s);
            }
            let suffix = v.trim_end_matches(':').trim();
            if suffix != "_a" && suffix != "_b" {
                return Err(format!("unexpected slot {suffix:?}"));
            }
            cur = Some(SlotState { suffix: suffix.into(), ..Default::default() });
        } else if let Some((k, v)) = l.split_once(':') {
            let s = cur.as_mut().ok_or_else(|| format!("unexpected line {l:?}"))?;
            match k.trim() {
                "Active" => s.active = flag(v)?,
                "Successful" => s.successful = flag(v)?,
                "Bootable" => s.bootable = flag(v)?,
                _ => return Err(format!("unexpected line {l:?}")),
            }
        } else {
            return Err(format!("unexpected line {l:?}"));
        }
    }
    if let Some(s) = cur.take() {
        d.slots.push(s);
    }
    if !saw_current || d.slots.len() != 2 || d.slot("_a").is_none() || d.slot("_b").is_none() {
        return Err("qbootctl output is not a two-slot dump".into());
    }
    Ok(d)
}

pub fn other(suffix: &str) -> &'static str {
    if suffix == "_a" { "_b" } else { "_a" }
}

pub fn letter(suffix: &str) -> &'static str {
    if suffix == "_a" { "a" } else { "b" }
}

/// The calls the daemon makes; a trait so tests and the laptop dry run can
/// use a fake.
pub trait BootControl: Send + Sync {
    fn dump(&self) -> Result<Dump, String>;
    fn set_active(&self, suffix: &str) -> Result<(), String>;
    fn mark_successful(&self) -> Result<(), String>;
}

pub struct Qbootctl {
    pub path: String,
}

impl BootControl for Qbootctl {
    fn dump(&self) -> Result<Dump, String> {
        parse_dump(&util::run(&self.path, &[])?)
    }
    fn set_active(&self, suffix: &str) -> Result<(), String> {
        util::log(format!("fp6-updated: {} -s {}", self.path, letter(suffix)));
        let out = util::run(&self.path, &["-s".into(), letter(suffix).into()])?;
        if !out.contains("Set as active slot") {
            return Err(format!("qbootctl -s: unexpected output {:?}", out.trim()));
        }
        Ok(())
    }
    fn mark_successful(&self) -> Result<(), String> {
        util::log(format!("fp6-updated: {} -m", self.path));
        // Prints "already marked successful" on stderr and exits 0 when set.
        util::run(&self.path, &["-m".into()]).map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Recorded format of qbootctl 0.2.2's dump_info() (synthetic values).
    const B_ACTIVE: &str = "Current slot: _b\nSLOT _a:\n\tActive      : 0\n\tSuccessful  : 0\n\tBootable    : 1\nSLOT _b:\n\tActive      : 1\n\tSuccessful  : 1\n\tBootable    : 1\n";
    const AFTER_SWITCH: &str = "Current slot: _b\nSLOT _a:\n\tActive      : 1\n\tSuccessful  : 0\n\tBootable    : 1\nSLOT _b:\n\tActive      : 0\n\tSuccessful  : 1\n\tBootable    : 1\n";
    const NA: &str = "Current slot: N/A\nSLOT _a:\n\tActive      : 0\n\tSuccessful  : 1\n\tBootable    : 0\nSLOT _b:\n\tActive      : 0\n\tSuccessful  : 1\n\tBootable    : 1\n";

    #[test]
    fn parses_dumps() {
        let d = parse_dump(B_ACTIVE).unwrap();
        assert_eq!(d.current.as_deref(), Some("_b"));
        assert_eq!(d.active(), Some("_b"));
        assert!(d.slot("_b").unwrap().successful);
        assert!(!d.slot("_a").unwrap().successful && d.slot("_a").unwrap().bootable);
        let d = parse_dump(AFTER_SWITCH).unwrap();
        assert_eq!(d.active(), Some("_a"));
        assert_eq!(d.current.as_deref(), Some("_b"));
        let d = parse_dump(NA).unwrap();
        assert_eq!(d.current, None);
        assert_eq!(d.active(), None, "no active slot");
        assert!(!d.slot("_a").unwrap().bootable);
    }

    #[test]
    fn rejects_garbage() {
        assert!(parse_dump("").is_err());
        assert!(parse_dump("No slots found, is this an A/B device?\n").is_err());
        assert!(parse_dump("Current slot: _a\nSLOT _a:\n\tActive      : 1\n").is_err(), "one slot only");
        assert!(parse_dump(&B_ACTIVE.replace("Successful  : 1", "Successful  : yes")).is_err());
        assert!(parse_dump(&B_ACTIVE.replace("SLOT _b:", "SLOT _c:")).is_err());
        assert!(parse_dump(&format!("{B_ACTIVE}\tRetry       : 7\n")).is_err());
    }
}
