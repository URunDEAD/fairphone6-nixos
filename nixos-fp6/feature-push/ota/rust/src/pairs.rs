//! Slot pair comparison (read-only): every `<name>_a`/`<name>_b` GPT pair
//! except boot is hashed. A slot switch makes the boot chain use the other
//! slot's xbl (UFS boot LUN), abl, tz, hyp, aop, devcfg, ..., dtbo, vbmeta,
//! vendor_boot and modem; so the daemon only switches when all of them are
//! byte-identical (or a difference is explicitly allowed in the config after
//! the supervised analysis). See the README, "Slot safety".

use crate::util::sha256_file;
use serde::Serialize;
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

/// Pairs the updater itself writes (excluded from the comparison).
pub const UPDATER_WRITES: [&str; 1] = ["boot"];

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Pair {
    pub name: String,
    pub size_a: u64,
    pub size_b: u64,
    pub sha256_a: String,
    pub sha256_b: String,
    pub identical: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vbmeta_flags: Option<(u32, u32)>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Base names that exist with both suffixes.
pub fn list(by_partlabel: &Path) -> Result<Vec<String>, String> {
    let names: BTreeSet<String> = fs::read_dir(by_partlabel).map_err(|e| format!("{}: {e}", by_partlabel.display()))?
        .flatten().filter_map(|e| e.file_name().into_string().ok()).collect();
    Ok(names.iter().filter_map(|n| n.strip_suffix("_a")).filter(|b| names.contains(&format!("{b}_b"))).map(str::to_string).collect())
}

fn size_of(p: &Path) -> Result<u64, String> {
    let f = fs::File::open(p).map_err(|e| format!("{}: {e}", p.display()))?;
    use std::io::{Seek, SeekFrom};
    let mut f = f;
    f.seek(SeekFrom::End(0)).map_err(|e| e.to_string())
}

/// AVB vbmeta flags (big-endian u32 at offset 120 of "AVB0"):
/// 1 = hashtree disabled, 2 = verification disabled.
pub fn vbmeta_flags(p: &Path) -> Option<u32> {
    use std::io::Read;
    let mut b = [0u8; 124];
    fs::File::open(p).ok()?.read_exact(&mut b).ok()?;
    if &b[0..4] != b"AVB0" {
        return None;
    }
    Some(u32::from_be_bytes(b[120..124].try_into().unwrap()))
}

pub fn compare(by_partlabel: &Path, name: &str) -> Pair {
    let a = by_partlabel.join(format!("{name}_a"));
    let b = by_partlabel.join(format!("{name}_b"));
    let r = (|| -> Result<Pair, String> {
        let (sa, sb) = (size_of(&a)?, size_of(&b)?);
        let (ha, _) = sha256_file(&a, None)?;
        let (hb, _) = sha256_file(&b, None)?;
        let vb = if name.starts_with("vbmeta") { vbmeta_flags(&a).zip(vbmeta_flags(&b)) } else { None };
        Ok(Pair { name: name.into(), size_a: sa, size_b: sb, identical: sa == sb && ha == hb, sha256_a: ha, sha256_b: hb, vbmeta_flags: vb, error: None })
    })();
    r.unwrap_or_else(|e| Pair { name: name.into(), size_a: 0, size_b: 0, sha256_a: String::new(), sha256_b: String::new(), identical: false, vbmeta_flags: None, error: Some(e) })
}

pub fn compare_all(by_partlabel: &Path) -> Result<Vec<Pair>, String> {
    Ok(list(by_partlabel)?.iter().filter(|n| !UPDATER_WRITES.contains(&n.as_str())).map(|n| compare(by_partlabel, n)).collect())
}

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Verdict {
    pub safe: bool,
    pub differing: Vec<String>,
    pub allowed: Vec<String>,
    pub unreadable: Vec<String>,
    pub compared: usize,
}

/// Safe only if every pair (except those the config explicitly allows) is
/// identical and readable, and at least one firmware pair was compared.
pub fn verdict(pairs: &[Pair], allowed_differences: &[String]) -> Verdict {
    let mut v = Verdict { safe: false, differing: vec![], allowed: vec![], unreadable: vec![], compared: pairs.len() };
    for p in pairs {
        if p.error.is_some() {
            v.unreadable.push(p.name.clone());
        } else if !p.identical {
            if allowed_differences.contains(&p.name) {
                v.allowed.push(p.name.clone());
            } else {
                v.differing.push(p.name.clone());
            }
        }
    }
    v.safe = !pairs.is_empty() && v.differing.is_empty() && v.unreadable.is_empty();
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pairs_and_verdict() {
        let d = std::env::temp_dir().join(format!("fp6-pairs-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        let mut vb = vec![0u8; 256];
        vb[0..4].copy_from_slice(b"AVB0");
        let mut vb2 = vb.clone();
        vb2[123] = 2;
        for (n, data) in [
            ("abl_a", b"abl".to_vec()), ("abl_b", b"abl".to_vec()),
            ("boot_a", b"stock".to_vec()), ("boot_b", b"nixos".to_vec()),
            ("modem_a", b"modem-old".to_vec()), ("modem_b", b"modem-new".to_vec()),
            ("vbmeta_a", vb.clone()), ("vbmeta_b", vb2),
            ("persist", b"p".to_vec()), ("xbl_a", b"x".to_vec()),
        ] {
            fs::write(d.join(n), data).unwrap();
        }
        assert_eq!(list(&d).unwrap(), vec!["abl", "boot", "modem", "vbmeta"]);
        let pairs = compare_all(&d).unwrap();
        assert_eq!(pairs.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), vec!["abl", "modem", "vbmeta"]);
        assert!(pairs[0].identical && !pairs[1].identical);
        assert_eq!(pairs[2].vbmeta_flags, Some((0, 2)));
        let v = verdict(&pairs, &[]);
        assert!(!v.safe);
        assert_eq!(v.differing, vec!["modem", "vbmeta"]);
        let v = verdict(&pairs, &["modem".into(), "vbmeta".into()]);
        assert!(v.safe && v.allowed.len() == 2);
        assert!(!verdict(&[], &[]).safe, "nothing compared is not safe");
        fs::remove_file(d.join("abl_b")).unwrap();
        std::os::unix::fs::symlink(d.join("missing"), d.join("abl_b")).unwrap();
        let v = verdict(&[compare(&d, "abl")], &[]);
        assert_eq!(v.unreadable, vec!["abl"]);
        assert!(!v.safe);
        let _ = fs::remove_dir_all(&d);
    }
}
