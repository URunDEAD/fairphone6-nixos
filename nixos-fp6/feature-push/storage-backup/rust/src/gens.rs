//! System generations and what must never be deleted.
//!
//! A generation is kept when its toplevel is the profile's current one, the
//! booted one (/run/booted-system), the running one (/run/current-system),
//! or the one a boot partition pins (mobile-nixos.generation= or init= in
//! the Android boot image's command line), or when its link cannot be
//! resolved. If any boot partition cannot be read, or none is found, the
//! plan is "uncertain" and nothing is deletable.

use serde::Serialize;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Generation {
    pub num: u32,
    pub target: Option<String>,
    pub date: i64,
}

/// `<dir>/<profile>-<N>-link` entries, sorted by number.
pub fn list_generations(dir: &Path, profile: &str) -> Vec<Generation> {
    use std::os::unix::fs::MetadataExt;
    let mut out = Vec::new();
    let prefix = format!("{profile}-");
    let Ok(rd) = fs::read_dir(dir) else { return out };
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        let Some(num) = name.strip_prefix(&prefix).and_then(|r| r.strip_suffix("-link")).and_then(|n| n.parse::<u32>().ok()) else {
            continue;
        };
        let link = e.path();
        let date = fs::symlink_metadata(&link).map(|m| m.mtime()).unwrap_or(0);
        let target = fs::read_link(&link).ok().map(|t| if t.is_absolute() { t } else { dir.join(t) })
            .and_then(|t| store_path_of(&t.to_string_lossy()));
        out.push(Generation { num, target, date });
    }
    out.sort_by_key(|g| g.num);
    out
}

/// Number of the generation `<dir>/<profile>` points to.
pub fn current_number(dir: &Path, profile: &str) -> Option<u32> {
    let t = fs::read_link(dir.join(profile)).ok()?;
    let n = t.file_name()?.to_string_lossy().into_owned();
    n.strip_prefix(&format!("{profile}-"))?.strip_suffix("-link")?.parse().ok()
}

/// "/nix/store/<hash>-<name>[/...]" -> "/nix/store/<hash>-<name>".
pub fn store_path_of(p: &str) -> Option<String> {
    let rest = p.strip_prefix("/nix/store/")?;
    let name = rest.split('/').next()?;
    let (hash, tail) = name.split_at_checked(32)?;
    if !hash.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit()) || !tail.starts_with('-') || tail.len() < 2 {
        return None;
    }
    Some(format!("/nix/store/{name}"))
}

/// Resolves a symlink chain (e.g. /run/booted-system) to its store path.
/// Falls back to following the links textually when the target is not
/// present (a store path that was already collected still names a toplevel).
pub fn resolve(p: &Path) -> Option<String> {
    if let Ok(t) = fs::canonicalize(p) {
        if let Some(s) = store_path_of(&t.to_string_lossy()) {
            return Some(s);
        }
    }
    let mut cur = p.to_path_buf();
    for _ in 0..16 {
        if let Some(s) = store_path_of(&cur.to_string_lossy()) {
            return Some(s);
        }
        let t = fs::read_link(&cur).ok()?;
        cur = if t.is_absolute() { t } else { cur.parent()?.join(t) };
    }
    None
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum BootPin {
    /// The image pins this toplevel.
    Pinned { path: String },
    /// A boot image without a NixOS generation (e.g. postmarketOS) or not an
    /// Android boot image at all: it pins nothing in this store.
    NoGeneration { note: String },
    /// Could not be read: unsure what it boots.
    Unreadable { error: String },
}

/// Kernel command line of an Android boot image (header v0..v4).
pub fn boot_cmdline(img: &[u8]) -> Result<Option<String>, String> {
    if img.len() < 1632 || &img[..8] != b"ANDROID!" {
        if img.len() >= 8 && &img[..8] == b"VNDRBOOT" {
            return Err("vendor boot image".into());
        }
        return Ok(None);
    }
    let version = u32::from_le_bytes(img[40..44].try_into().unwrap());
    let cstr = |b: &[u8]| {
        let end = b.iter().position(|&c| c == 0).unwrap_or(b.len());
        String::from_utf8_lossy(&b[..end]).into_owned()
    };
    let cmd = match version {
        0..=2 => {
            // cmdline[512] then extra_cmdline[1024], continued without a separator.
            let mut s = cstr(&img[64..576]);
            s.push_str(&cstr(&img[608..1632]));
            s
        }
        3 | 4 => cstr(&img[44..44 + 1536]),
        v => return Err(format!("unknown boot image header version {v}")),
    };
    Ok(Some(cmd))
}

/// The toplevel a command line boots: mobile-nixos.generation=<path>, else
/// init=<toplevel>/init.
pub fn generation_from_cmdline(cmd: &str) -> Option<String> {
    let mut init = None;
    for t in cmd.split_ascii_whitespace() {
        if let Some(v) = t.strip_prefix("mobile-nixos.generation=") {
            return store_path_of(v);
        }
        if let Some(v) = t.strip_prefix("init=") {
            init = store_path_of(v);
        }
    }
    init
}

pub fn pin_from_image(bytes: &[u8]) -> BootPin {
    match boot_cmdline(bytes) {
        Err(e) => BootPin::Unreadable { error: e },
        Ok(None) => BootPin::NoGeneration { note: "not an Android boot image".into() },
        Ok(Some(cmd)) => match generation_from_cmdline(&cmd) {
            Some(path) => BootPin::Pinned { path },
            None => BootPin::NoGeneration { note: "no NixOS generation on the command line".into() },
        },
    }
}

/// Reads the first 4 KiB of a boot partition (read-only, never written).
pub fn read_pin(dev: &Path) -> BootPin {
    use std::os::unix::fs::OpenOptionsExt;
    let r = fs::OpenOptions::new().read(true).custom_flags(libc::O_CLOEXEC).open(dev).and_then(|mut f| {
        let mut buf = vec![0u8; 4096];
        let mut n = 0;
        while n < buf.len() {
            let k = f.read(&mut buf[n..])?;
            if k == 0 {
                break;
            }
            n += k;
        }
        buf.truncate(n);
        Ok(buf)
    });
    match r {
        Ok(b) => pin_from_image(&b),
        Err(e) => BootPin::Unreadable { error: e.to_string() },
    }
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct PlanEntry {
    pub num: u32,
    pub target: Option<String>,
    pub date: i64,
    pub keep: bool,
    pub reasons: Vec<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Plan {
    pub entries: Vec<PlanEntry>,
    pub certain: bool,
    pub uncertain_reason: String,
    pub deletable: Vec<u32>,
    /// Toplevels that must stay alive (also made GC roots before a GC).
    pub protected: Vec<String>,
}

pub struct Facts<'a> {
    pub current_num: Option<u32>,
    pub current: Option<String>,
    pub booted: Option<String>,
    pub running: Option<String>,
    /// (partition label, pin); an empty list means no boot partition was found.
    pub pins: &'a [(String, BootPin)],
}

pub fn plan(gens: &[Generation], f: &Facts) -> Plan {
    let mut protected: Vec<(String, String)> = Vec::new();
    for (p, why) in [(&f.current, "current"), (&f.booted, "booted"), (&f.running, "running")] {
        if let Some(p) = p {
            protected.push((p.clone(), why.into()));
        }
    }
    let mut certain = true;
    let mut uncertain_reason = String::new();
    if f.current_num.is_none() || f.current.is_none() {
        certain = false;
        uncertain_reason = "the current system profile could not be resolved".into();
    }
    if f.pins.is_empty() {
        certain = false;
        uncertain_reason = "no boot partition was found".into();
    }
    for (label, pin) in f.pins {
        match pin {
            BootPin::Pinned { path } => protected.push((path.clone(), label.clone())),
            BootPin::NoGeneration { .. } => {}
            BootPin::Unreadable { error } => {
                certain = false;
                uncertain_reason = format!("{label} could not be read ({error})");
            }
        }
    }
    let mut entries = Vec::new();
    for g in gens {
        let mut reasons = Vec::new();
        if Some(g.num) == f.current_num {
            reasons.push("current".to_string());
        }
        match &g.target {
            None => reasons.push("unresolved".into()),
            Some(t) => {
                for (p, why) in &protected {
                    if p == t && !reasons.contains(why) {
                        reasons.push(why.clone());
                    }
                }
            }
        }
        let keep = !reasons.is_empty() || !certain;
        entries.push(PlanEntry { num: g.num, target: g.target.clone(), date: g.date, keep, reasons });
    }
    let deletable = if certain { entries.iter().filter(|e| !e.keep).map(|e| e.num).collect() } else { vec![] };
    let mut prot: Vec<String> = protected.into_iter().map(|(p, _)| p).collect();
    prot.sort();
    prot.dedup();
    Plan { entries, certain, uncertain_reason, deletable, protected: prot }
}

/// The generations of `requested` that the plan allows to delete. Anything
/// the plan keeps is dropped, whatever the caller asked for.
pub fn filter_request(plan: &Plan, requested: &[u32]) -> Vec<u32> {
    let mut v: Vec<u32> = requested.iter().copied().filter(|n| plan.deletable.contains(n)).collect();
    v.sort();
    v.dedup();
    v
}

pub fn pins_for(parts: &[(String, PathBuf)]) -> Vec<(String, BootPin)> {
    parts.iter().filter(|(_, p)| p.exists()).map(|(l, p)| (l.clone(), read_pin(p))).collect()
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::util::tempdir;

    pub const H: [&str; 5] = [
        "/nix/store/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa-nixos-system-fp6-1",
        "/nix/store/bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb-nixos-system-fp6-2",
        "/nix/store/cccccccccccccccccccccccccccccccc-nixos-system-fp6-3",
        "/nix/store/dddddddddddddddddddddddddddddddd-nixos-system-fp6-4",
        "/nix/store/eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee-nixos-system-fp6-5",
    ];

    /// Synthetic Android boot image, header v2, command line split as
    /// repack-new-kernel.py does (512 + 1024 bytes).
    pub fn boot_image(cmdline: &str) -> Vec<u8> {
        let mut b = vec![0u8; 4096];
        b[..8].copy_from_slice(b"ANDROID!");
        b[40..44].copy_from_slice(&2u32.to_le_bytes());
        b[36..40].copy_from_slice(&4096u32.to_le_bytes());
        let c = cmdline.as_bytes();
        let a = c.len().min(512);
        b[64..64 + a].copy_from_slice(&c[..a]);
        if c.len() > 512 {
            b[608..608 + c.len() - 512].copy_from_slice(&c[512..]);
        }
        b
    }

    #[test]
    fn cmdline_parsing() {
        let long = format!("{} mobile-nixos.generation={}", "x".repeat(600), H[1]);
        assert_eq!(generation_from_cmdline(&boot_cmdline(&boot_image(&long)).unwrap().unwrap()), Some(H[1].to_string()));
        // split exactly across the 512-byte boundary
        let pad = 512 - "console=ttyMSM0 mobile-nixos.generation=/nix/st".len();
        let edge = format!("{}console=ttyMSM0 mobile-nixos.generation={}", " ".repeat(pad), H[2]);
        assert_eq!(pin_from_image(&boot_image(&edge)), BootPin::Pinned { path: H[2].into() });
        assert_eq!(pin_from_image(&boot_image(&format!("quiet init={}/init", H[3]))), BootPin::Pinned { path: H[3].into() });
        assert!(matches!(pin_from_image(&boot_image("pmos_boot_uuid=1234 quiet")), BootPin::NoGeneration { .. }));
        assert!(matches!(pin_from_image(b"garbage"), BootPin::NoGeneration { .. }));
        assert_eq!(store_path_of("/nix/store/short-x"), None);
        assert_eq!(store_path_of(&format!("{}/init", H[0])), Some(H[0].into()));
    }

    fn gens() -> Vec<Generation> {
        (0..5).map(|i| Generation { num: i as u32 + 1, target: Some(H[i].into()), date: 0 }).collect()
    }

    #[test]
    fn keeps_current_booted_and_boot_pin() {
        let pins = vec![("boot_a".to_string(), BootPin::NoGeneration { note: String::new() }),
                        ("boot_b".to_string(), BootPin::Pinned { path: H[1].into() })];
        let f = Facts { current_num: Some(5), current: Some(H[4].into()), booted: Some(H[2].into()), running: Some(H[4].into()), pins: &pins };
        let p = plan(&gens(), &f);
        assert!(p.certain);
        assert_eq!(p.deletable, vec![1, 4]);
        assert_eq!(p.entries[1].reasons, vec!["boot_b"]);
        assert_eq!(p.entries[2].reasons, vec!["booted"]);
        assert!(p.entries[4].reasons.contains(&"current".to_string()));
        assert_eq!(filter_request(&p, &[1, 2, 3, 4, 5, 99]), vec![1, 4]);
        assert!(p.protected.contains(&H[1].to_string()));
    }

    #[test]
    fn unsure_keeps_everything() {
        let pins = vec![("boot_b".to_string(), BootPin::Unreadable { error: "EIO".into() })];
        let f = Facts { current_num: Some(5), current: Some(H[4].into()), booted: Some(H[4].into()), running: None, pins: &pins };
        let p = plan(&gens(), &f);
        assert!(!p.certain);
        assert!(p.deletable.is_empty());
        assert!(p.entries.iter().all(|e| e.keep));
        assert!(filter_request(&p, &[1, 2]).is_empty());
        // no boot partition at all
        let f = Facts { pins: &[], ..f };
        assert!(plan(&gens(), &f).deletable.is_empty());
        // unresolved current profile
        let pins = vec![("boot_b".to_string(), BootPin::Pinned { path: H[4].into() })];
        let f = Facts { current_num: None, current: None, booted: None, running: None, pins: &pins };
        assert!(plan(&gens(), &f).deletable.is_empty());
    }

    #[test]
    fn unresolved_link_is_kept() {
        let mut g = gens();
        g[0].target = None;
        let pins = vec![("boot_b".to_string(), BootPin::Pinned { path: H[4].into() })];
        let f = Facts { current_num: Some(5), current: Some(H[4].into()), booted: Some(H[4].into()), running: None, pins: &pins };
        let p = plan(&g, &f);
        assert_eq!(p.deletable, vec![2, 3, 4]);
        assert_eq!(p.entries[0].reasons, vec!["unresolved"]);
    }

    #[test]
    fn lists_profile_links() {
        let t = tempdir("gens");
        for (i, h) in H.iter().enumerate().take(3) {
            std::os::unix::fs::symlink(h, t.join(format!("system-{}-link", i + 1))).unwrap();
        }
        std::os::unix::fs::symlink("system-3-link", t.join("system")).unwrap();
        std::os::unix::fs::symlink(H[0], t.join("other-1-link")).unwrap();
        fs::write(t.join("system-x-link"), "").unwrap();
        let g = list_generations(&t, "system");
        assert_eq!(g.iter().map(|g| g.num).collect::<Vec<_>>(), vec![1, 2, 3]);
        assert_eq!(g[1].target.as_deref(), Some(H[1]));
        assert_eq!(current_number(&t, "system"), Some(3));
        assert_eq!(resolve(&t.join("system")).as_deref(), Some(H[2]));
        std::os::unix::fs::symlink(t.join("system"), t.join("booted")).unwrap();
        assert_eq!(resolve(&t.join("booted")).as_deref(), Some(H[2]));
        assert_eq!(resolve(&t.join("nothing")), None);
        let img = t.join("boot_b.img");
        fs::write(&img, boot_image(&format!("mobile-nixos.generation={}", H[0]))).unwrap();
        let pins = pins_for(&[("boot_b".into(), img), ("boot_a".into(), t.join("missing"))]);
        assert_eq!(pins, vec![("boot_b".to_string(), BootPin::Pinned { path: H[0].into() })]);
    }
}
