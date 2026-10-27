//! Small helpers: time, hashing, atomic files, commands.

use sha2::{Digest, Sha256};
use std::fs;
use std::io::{Read, Write};
use std::path::Path;
use std::process::Command;

pub fn now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

pub fn log(m: impl AsRef<str>) {
    eprintln!("{}", m.as_ref());
}

pub fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

pub fn sha256_bytes(b: &[u8]) -> String {
    hex(&Sha256::digest(b))
}

/// SHA-256 of the first `limit` bytes of a file or block device (all of it
/// when None), read in 1 MiB chunks.
pub fn sha256_file(p: &Path, limit: Option<u64>) -> Result<(String, u64), String> {
    let mut f = fs::File::open(p).map_err(|e| format!("{}: {e}", p.display()))?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    let mut n: u64 = 0;
    loop {
        let want = match limit {
            Some(l) if l - n < buf.len() as u64 => (l - n) as usize,
            _ => buf.len(),
        };
        if want == 0 {
            break;
        }
        let r = f.read(&mut buf[..want]).map_err(|e| format!("{}: {e}", p.display()))?;
        if r == 0 {
            break;
        }
        h.update(&buf[..r]);
        n += r as u64;
    }
    if let Some(l) = limit {
        if n != l {
            return Err(format!("{}: short read ({n} of {l} bytes)", p.display()));
        }
    }
    Ok((hex(&h.finalize()), n))
}

/// Writes a file through a temporary name, fsyncs it and renames it over
/// the target (and fsyncs the directory).
pub fn write_atomic(p: &Path, data: &[u8], mode: u32) -> Result<(), String> {
    use std::os::unix::fs::OpenOptionsExt;
    let dir = p.parent().unwrap_or(Path::new("."));
    fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let tmp = dir.join(format!(".{}.tmp{}", p.file_name().and_then(|n| n.to_str()).unwrap_or("f"), std::process::id()));
    {
        let mut f = fs::OpenOptions::new().write(true).create(true).truncate(true).mode(mode).open(&tmp)
            .map_err(|e| format!("{}: {e}", tmp.display()))?;
        f.write_all(data).map_err(|e| format!("{}: {e}", tmp.display()))?;
        f.sync_all().map_err(|e| format!("{}: {e}", tmp.display()))?;
    }
    fs::rename(&tmp, p).map_err(|e| format!("{}: {e}", p.display()))?;
    if let Ok(d) = fs::File::open(dir) {
        let _ = d.sync_all();
    }
    Ok(())
}

/// Runs a command; Ok(stdout) on success, Err(last stderr line) otherwise.
pub fn run(cmd: &str, args: &[String]) -> Result<String, String> {
    let out = Command::new(cmd).args(args).output().map_err(|e| format!("{cmd}: {e}"))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        let last = err.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("").trim().to_string();
        return Err(format!("{cmd} {} failed ({}): {last}", args.first().map(String::as_str).unwrap_or(""), out.status));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Local time hour (0..23) of a unix time.
pub fn local_hour(t: i64) -> u32 {
    let tt = t as libc::time_t;
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    unsafe { libc::localtime_r(&tt, &mut tm) };
    tm.tm_hour as u32
}

/// RFC 3339 UTC date of a unix time (civil-from-days, no time zone database).
pub fn rfc3339(t: i64) -> String {
    let days = t.div_euclid(86400);
    let secs = t.rem_euclid(86400);
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z", secs / 3600, secs % 3600 / 60, secs % 60)
}

/// A store path: /nix/store/<32 base32 chars>-<name>.
pub fn is_store_path(p: &str) -> bool {
    let Some(rest) = p.strip_prefix("/nix/store/") else { return false };
    let (hash, name) = match rest.split_once('-') {
        Some(x) => x,
        None => return false,
    };
    hash.len() == 32
        && hash.chars().all(|c| "0123456789abcdfghijklmnpqrsvwxyz".contains(c))
        && !name.is_empty()
        && name.len() <= 211
        && name.chars().all(|c| c.is_ascii_alphanumeric() || "+-._?=".contains(c))
        && !name.starts_with('.')
}

pub fn is_sha256_hex(s: &str) -> bool {
    s.len() == 64 && s.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dates_and_paths() {
        assert_eq!(rfc3339(0), "1970-01-01T00:00:00Z");
        assert_eq!(rfc3339(1_793_000_000), "2026-10-26T07:33:20Z");
        assert_eq!(rfc3339(951_782_400), "2000-02-29T00:00:00Z");
        assert!(is_store_path("/nix/store/z5av6a1y11jpm7m84yyqy59ffv5lc12f-nixos-system-fp6-26.11pre1031299.0bb7ec54c848"));
        assert!(!is_store_path("/nix/store/z5av6a1y11jpm7m84yyqy59ffv5lc12e-x")); // 'e' is not nix base32
        assert!(!is_store_path("/nix/store/short-x"));
        assert!(!is_store_path("/nix/store/z5av6a1y11jpm7m84yyqy59ffv5lc12f-a/../../etc"));
        assert!(!is_store_path("/tmp/z5av6a1y11jpm7m84yyqy59ffv5lc12f-x"));
        assert!(is_sha256_hex(&sha256_bytes(b"x")));
        assert!(!is_sha256_hex("ABC"));
    }
}
