//! SHA-256 sums: parsing GNU ("hex  name", "hex *name") and BSD
//! ("SHA256 (name) = hex") lines, finding a sum file next to an image, and
//! hashing a file with progress/cancel.

use crate::sha256::Sha256;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

pub fn is_hex64(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Normalizes user input: trims, lower-cases, accepts "sha256:HEX" and a
/// pasted line "HEX  name".
pub fn normalize_user_sum(s: &str) -> Option<String> {
    let t = s.trim();
    let t = t.strip_prefix("sha256:").or_else(|| t.strip_prefix("SHA256:")).unwrap_or(t).trim();
    let first = t.split_whitespace().next().unwrap_or("");
    if is_hex64(first) {
        return Some(first.to_ascii_lowercase());
    }
    parse_line(t).map(|(h, _)| h)
}

/// One sums-file line -> (lower-case hex, file name or "" when absent).
pub fn parse_line(line: &str) -> Option<(String, String)> {
    let l = line.trim();
    if l.is_empty() || l.starts_with('#') {
        return None;
    }
    // BSD: SHA256 (name) = hex
    if let Some(rest) = l.strip_prefix("SHA256 (").or_else(|| l.strip_prefix("SHA256(")) {
        let (name, tail) = rest.rsplit_once(") =").or_else(|| rest.rsplit_once(")="))?;
        let hex = tail.trim();
        return is_hex64(hex).then(|| (hex.to_ascii_lowercase(), name.to_string()));
    }
    // GNU: hex [space][space|*]name
    let (hex, rest) = l.split_at(l.find(char::is_whitespace).unwrap_or(l.len()));
    if !is_hex64(hex) {
        return None;
    }
    let name = rest.trim_start_matches([' ', '\t']).trim_start_matches('*').trim_end();
    let name = name.strip_prefix("./").unwrap_or(name);
    Some((hex.to_ascii_lowercase(), name.to_string()))
}

/// The sum for `name` in a sums-file text. A single-line file without (or
/// with any) name counts for `name` when `single_ok`.
pub fn find_in_sums(text: &str, name: &str, single_ok: bool) -> Option<String> {
    let entries: Vec<(String, String)> = text.lines().filter_map(parse_line).collect();
    if let Some((h, _)) = entries.iter().find(|(_, n)| n == name || Path::new(n).file_name().and_then(|f| f.to_str()) == Some(name)) {
        return Some(h.clone());
    }
    if single_ok && entries.len() == 1 && entries[0].1.is_empty() {
        return Some(entries[0].0.clone());
    }
    None
}

/// Candidate sum files for an image, most specific first.
pub fn sum_file_candidates(image: &Path) -> Vec<(PathBuf, bool)> {
    let dir = image.parent().unwrap_or(Path::new("."));
    let name = image.file_name().and_then(|n| n.to_str()).unwrap_or("");
    let mut v = Vec::new();
    for ext in ["sha256", "sha256sum", "SHA256", "sha256.txt"] {
        v.push((dir.join(format!("{name}.{ext}")), true));
    }
    for n in ["SHA256SUMS", "SHA256SUMS.txt", "sha256sums.txt", "sha256sum.txt", "CHECKSUM", "CHECKSUMS"] {
        v.push((dir.join(n), false));
    }
    // Fedora: Fedora-Workstation-43-1.6-x86_64-CHECKSUM
    if let Ok(rd) = std::fs::read_dir(dir) {
        let mut extra: Vec<PathBuf> = rd
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| {
                p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.ends_with("-CHECKSUM") || n.ends_with("SHA256SUMS"))
            })
            .collect();
        extra.sort();
        for p in extra {
            if !v.iter().any(|(q, _)| q == &p) {
                v.push((p, false));
            }
        }
    }
    v
}

/// Expected sum from a file next to the image: (hex, sum-file path).
pub fn expected_for(image: &Path) -> Option<(String, PathBuf)> {
    let name = image.file_name()?.to_str()?;
    for (cand, single_ok) in sum_file_candidates(image) {
        let Ok(meta) = std::fs::metadata(&cand) else { continue };
        if !meta.is_file() || meta.len() > 4 << 20 {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&cand) else { continue };
        if let Some(h) = find_in_sums(&text, name, single_ok) {
            return Some((h, cand));
        }
    }
    None
}

pub struct Progress {
    pub done: AtomicU64,
    pub total: AtomicU64,
    pub cancel: AtomicBool,
}

impl Default for Progress {
    fn default() -> Self {
        Progress { done: AtomicU64::new(0), total: AtomicU64::new(0), cancel: AtomicBool::new(false) }
    }
}

/// SHA-256 of a file. Err("cancelled") when cancelled.
pub fn hash_file(path: &Path, p: &Progress) -> Result<String, String> {
    let mut f = File::open(path).map_err(|e| e.to_string())?;
    let len = f.metadata().map(|m| m.len()).unwrap_or(0);
    p.total.store(len, Ordering::Relaxed);
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        if p.cancel.load(Ordering::Relaxed) {
            return Err("cancelled".into());
        }
        let n = f.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
        p.done.fetch_add(n as u64, Ordering::Relaxed);
    }
    Ok(h.hex())
}

#[cfg(test)]
mod tests {
    use super::*;
    const H: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

    #[test]
    fn lines() {
        assert_eq!(parse_line(&format!("{H}  ubuntu.iso")), Some((H.into(), "ubuntu.iso".into())));
        assert_eq!(parse_line(&format!("{H} *ubuntu.iso")), Some((H.into(), "ubuntu.iso".into())));
        assert_eq!(parse_line(&format!("{} ./a.iso", H.to_uppercase())), Some((H.into(), "a.iso".into())));
        assert_eq!(parse_line(&format!("SHA256 (Fedora-43.iso) = {H}")), Some((H.into(), "Fedora-43.iso".into())));
        assert_eq!(parse_line(H), Some((H.into(), "".into())));
        assert_eq!(parse_line("# comment"), None);
        assert_eq!(parse_line("-----BEGIN PGP SIGNED MESSAGE-----"), None);
        assert_eq!(parse_line("abc  x.iso"), None);
    }

    #[test]
    fn user_sum() {
        assert_eq!(normalize_user_sum(&format!("  {}\n", H.to_uppercase())), Some(H.into()));
        assert_eq!(normalize_user_sum(&format!("sha256:{H}")), Some(H.into()));
        assert_eq!(normalize_user_sum(&format!("{H}  foo.iso")), Some(H.into()));
        assert_eq!(normalize_user_sum("nope"), None);
    }

    #[test]
    fn sums_files() {
        let text = format!("-----BEGIN PGP SIGNED MESSAGE-----\nHash: SHA256\n\n# Fedora\nSHA256 (a.iso) = {}\n{H}  b.iso\n", "0".repeat(64));
        assert_eq!(find_in_sums(&text, "b.iso", false), Some(H.into()));
        assert_eq!(find_in_sums(&text, "a.iso", false), Some("0".repeat(64)));
        assert_eq!(find_in_sums(&text, "c.iso", false), None);
        assert_eq!(find_in_sums(H, "c.iso", true), Some(H.into()));
        assert_eq!(find_in_sums(H, "c.iso", false), None);
    }

    #[test]
    fn next_to_image() {
        let d = std::env::temp_dir().join(format!("fp6ud-sum-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let img = d.join("x.iso");
        std::fs::write(&img, b"abc").unwrap();
        assert_eq!(expected_for(&img), None);
        std::fs::write(d.join("SHA256SUMS"), format!("{H}  x.iso\n")).unwrap();
        assert_eq!(expected_for(&img).map(|x| x.0), Some(H.into()));
        let p = Progress::default();
        assert_eq!(hash_file(&img, &p).unwrap(), H);
        assert_eq!(p.done.load(Ordering::Relaxed), 3);
        std::fs::remove_dir_all(d).ok();
    }
}
