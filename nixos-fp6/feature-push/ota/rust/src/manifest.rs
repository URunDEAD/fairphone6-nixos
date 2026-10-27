//! The signed release manifest, the closure list it pins, and the release
//! file baked into every system (/etc/fp6-release).

use crate::minisig::PublicKey;
use crate::util::{is_sha256_hex, is_store_path, sha256_bytes};
use serde::{Deserialize, Serialize};

pub const FORMAT: u32 = 1;
/// The boot partitions are 96 MiB; repack-boot.py refuses images >= 96 MiB.
pub const MAX_BOOT_IMAGE: u64 = 96 * 1024 * 1024;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct BootImage {
    pub path: String,
    pub sha256: String,
    pub size: u64,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct Kernel {
    pub release: String,
    /// Free text: minimum/maximum kernel notes (firmware, DTB, module ABI).
    #[serde(default)]
    pub notes: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct ClosureRef {
    /// File name next to manifest.json.
    pub file: String,
    pub sha256: String,
    pub paths: u64,
    pub nar_size: u64,
    pub file_size: u64,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    pub format: u32,
    pub device: String,
    pub channel: String,
    /// Monotonic release counter: the only thing compared for "newer".
    pub release: u64,
    pub version: String,
    pub label: String,
    pub date: String,
    pub timestamp: i64,
    pub toplevel: String,
    pub boot_image: BootImage,
    pub kernel: Kernel,
    /// The phone must already run at least this release (0: any).
    #[serde(default)]
    pub min_release: u64,
    #[serde(default)]
    pub changelog: String,
    pub closure: ClosureRef,
    /// Binary cache URL; relative URLs resolve against the manifest URL.
    #[serde(default)]
    pub cache: Option<String>,
    /// Unix time after which the phone treats the manifest as stale.
    #[serde(default)]
    pub expires: Option<i64>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct ClosureEntry {
    pub path: String,
    pub nar_size: u64,
    #[serde(default)]
    pub file_size: u64,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct Closure {
    pub paths: Vec<ClosureEntry>,
}

/// /etc/fp6-release, written by the ota module into every system.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct ReleaseInfo {
    pub device: String,
    pub channel: String,
    pub release: u64,
    pub version: String,
    pub label: String,
    #[serde(default)]
    pub kernel: String,
}

impl Manifest {
    pub fn trusted_comment(&self, manifest_bytes: &[u8]) -> String {
        format!("fp6-ota device={} channel={} release={} sha256={}", self.device, self.channel, self.release, sha256_bytes(manifest_bytes))
    }

    /// Structural checks, independent of the phone's state.
    pub fn validate(&self) -> Result<(), String> {
        if self.format != FORMAT {
            return Err(format!("unsupported manifest format {}", self.format));
        }
        if self.release == 0 {
            return Err("release counter must be at least 1".into());
        }
        for (what, p) in [("toplevel", &self.toplevel), ("boot image", &self.boot_image.path)] {
            if !is_store_path(p) {
                return Err(format!("{what} is not a store path: {p}"));
            }
        }
        if !self.toplevel.contains("-nixos-system-") {
            return Err("toplevel is not a NixOS system".into());
        }
        if !is_sha256_hex(&self.boot_image.sha256) || !is_sha256_hex(&self.closure.sha256) {
            return Err("malformed sha256".into());
        }
        if self.boot_image.size < 4096 || self.boot_image.size >= MAX_BOOT_IMAGE {
            return Err(format!("boot image size {} out of range", self.boot_image.size));
        }
        if self.closure.file.contains('/') || self.closure.file.starts_with('.') || self.closure.file.is_empty() {
            return Err("closure file must be a plain file name".into());
        }
        if self.changelog.len() > 16384 || self.version.len() > 200 || self.label.len() > 200 || self.kernel.notes.len() > 4096 {
            return Err("manifest text fields too long".into());
        }
        for s in [&self.device, &self.channel] {
            if s.is_empty() || !s.chars().all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c)) {
                return Err(format!("bad device/channel name {s:?}"));
            }
        }
        Ok(())
    }
}

/// Verify a fetched manifest end to end: signature with the baked-in key,
/// trusted comment bound to the bytes, structure, device and channel, and
/// expiry. Returns the parsed manifest.
pub fn verify(bytes: &[u8], minisig: &str, key: &PublicKey, device: &str, channel: &str, now: i64) -> Result<Manifest, String> {
    let trusted = key.verify(bytes, minisig)?;
    let m: Manifest = serde_json::from_slice(bytes).map_err(|e| format!("manifest: {e}"))?;
    if trusted != m.trusted_comment(bytes) {
        return Err("trusted comment does not describe this manifest".into());
    }
    m.validate()?;
    if m.device != device {
        return Err(format!("manifest is for device {:?}, this is {device:?}", m.device));
    }
    if m.channel != channel {
        return Err(format!("manifest is for channel {:?}, this phone follows {channel:?}", m.channel));
    }
    if let Some(e) = m.expires {
        if now > e {
            return Err("manifest has expired (the update server is not publishing fresh manifests)".into());
        }
    }
    Ok(m)
}

/// Closure list, checked against the sha256 the signed manifest pins.
pub fn verify_closure(bytes: &[u8], m: &Manifest) -> Result<Closure, String> {
    if sha256_bytes(bytes) != m.closure.sha256 {
        return Err("closure list does not match the manifest".into());
    }
    let c: Closure = serde_json::from_slice(bytes).map_err(|e| format!("closure: {e}"))?;
    if c.paths.len() as u64 != m.closure.paths || c.paths.iter().any(|p| !is_store_path(&p.path)) {
        return Err("closure list malformed".into());
    }
    for need in [&m.toplevel, &m.boot_image.path] {
        if !c.paths.iter().any(|p| &p.path == need) {
            return Err(format!("closure list lacks {need}"));
        }
    }
    Ok(c)
}

#[derive(Debug, PartialEq, Clone, Copy)]
pub enum Offer {
    Newer,
    Same,
    Older,
    /// Newer, but the phone is below min_release.
    TooOld,
}

pub fn compare(m: &Manifest, running: &ReleaseInfo) -> Offer {
    if m.release == running.release {
        Offer::Same
    } else if m.release < running.release {
        Offer::Older
    } else if m.min_release > running.release {
        Offer::TooOld
    } else {
        Offer::Newer
    }
}

/// Resolve a possibly relative URL against the manifest URL's directory.
pub fn resolve_url(base: &str, rel: &str) -> String {
    if rel.contains("://") {
        return rel.to_string();
    }
    let dir = match base.rfind('/') {
        Some(i) => &base[..=i],
        None => "",
    };
    let mut out = format!("{dir}{rel}");
    // collapse "x/../"
    while let Some(i) = out.find("/../") {
        let head = &out[..i];
        let cut = head.rfind('/').map(|j| j + 1).unwrap_or(0);
        if head[cut..].contains(':') || cut == 0 {
            break;
        }
        out = format!("{}{}", &out[..cut], &out[i + 4..]);
    }
    out
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::minisig::SecretKey;

    pub fn sample() -> Manifest {
        Manifest {
            format: 1,
            device: "fairphone-fp6".into(),
            channel: "kernel6".into(),
            release: 3,
            version: "r3 full-features-kernel6".into(),
            label: "full-features-kernel6-lomiri-gpu-26.11pre1031299.0bb7ec54c848".into(),
            date: "2026-10-27T10:00:00Z".into(),
            timestamp: 1_793_095_200,
            toplevel: "/nix/store/z5av6a1y11jpm7m84yyqy59ffv5lc12f-nixos-system-fp6-full-features".into(),
            boot_image: BootImage { path: "/nix/store/0bb7rc54c8480bb7rc54c8480bb7rc54-fp6-nixos-boot.img".into(), sha256: "a".repeat(64), size: 25_763_840 },
            kernel: Kernel { release: "7.2.0-fp6-nixos6".into(), notes: "DTB unchanged".into() },
            min_release: 1,
            changelog: "Fixes.".into(),
            closure: ClosureRef { file: "closure.json".into(), sha256: "b".repeat(64), paths: 2, nar_size: 10, file_size: 5 },
            cache: Some("../../cache".into()),
            expires: None,
        }
    }

    fn signed(m: &Manifest, k: &SecretKey) -> (Vec<u8>, String) {
        let b = serde_json::to_vec_pretty(m).unwrap();
        let s = k.sign(&b, &m.trusted_comment(&b)).unwrap();
        (b, s)
    }

    #[test]
    fn verify_accepts_good_and_rejects_bad() {
        let k = SecretKey::from_seed([1; 8], [2; 32]);
        let p = k.public();
        let m = sample();
        let (b, s) = signed(&m, &k);
        assert_eq!(verify(&b, &s, &p, "fairphone-fp6", "kernel6", 0).unwrap(), m);
        assert!(verify(&b, &s, &p, "fairphone-fp5", "kernel6", 0).unwrap_err().contains("device"));
        assert!(verify(&b, &s, &p, "fairphone-fp6", "kernel5", 0).unwrap_err().contains("channel"));
        // a byte changed after signing
        let mut t = b.clone();
        let i = t.iter().position(|c| *c == b'3').unwrap();
        t[i] = b'4';
        assert!(verify(&t, &s, &p, "fairphone-fp6", "kernel6", 0).is_err());
        // signature of another manifest replayed with this one's comment: the
        // trusted comment pins the sha256 of the signed bytes
        let mut m2 = m.clone();
        m2.release = 1;
        let (b2, _) = signed(&m2, &k);
        let s_wrong = k.sign(&b2, &m.trusted_comment(&b)).unwrap();
        assert!(verify(&b2, &s_wrong, &p, "fairphone-fp6", "kernel6", 0).unwrap_err().contains("trusted comment"));
        // expiry
        let mut m3 = m.clone();
        m3.expires = Some(100);
        let (b3, s3) = signed(&m3, &k);
        assert!(verify(&b3, &s3, &p, "fairphone-fp6", "kernel6", 50).is_ok());
        assert!(verify(&b3, &s3, &p, "fairphone-fp6", "kernel6", 101).unwrap_err().contains("expired"));
    }

    #[test]
    fn validate_rejects_malformed() {
        let mut m = sample();
        m.toplevel = "/nix/store/../etc".into();
        assert!(m.validate().is_err());
        let mut m = sample();
        m.boot_image.size = MAX_BOOT_IMAGE;
        assert!(m.validate().is_err());
        let mut m = sample();
        m.closure.file = "../x".into();
        assert!(m.validate().is_err());
        let mut m = sample();
        m.toplevel = "/nix/store/z5av6a1y11jpm7m84yyqy59ffv5lc12f-hello".into();
        assert!(m.validate().unwrap_err().contains("NixOS"));
        let mut m = sample();
        m.release = 0;
        assert!(m.validate().is_err());
        assert!(sample().validate().is_ok());
    }

    #[test]
    fn closure_pinned_by_manifest() {
        let mut m = sample();
        let c = Closure { paths: vec![
            ClosureEntry { path: m.toplevel.clone(), nar_size: 6, file_size: 3 },
            ClosureEntry { path: m.boot_image.path.clone(), nar_size: 4, file_size: 2 },
        ] };
        let b = serde_json::to_vec(&c).unwrap();
        m.closure.sha256 = sha256_bytes(&b);
        assert_eq!(verify_closure(&b, &m).unwrap(), c);
        let mut b2 = b.clone();
        b2.push(b' ');
        assert!(verify_closure(&b2, &m).is_err());
        let c3 = Closure { paths: vec![c.paths[0].clone(), ClosureEntry { path: "/nix/store/z5av6a1y11jpm7m84yyqy59ffv5lc12f-other".into(), ..Default::default() }] };
        let b3 = serde_json::to_vec(&c3).unwrap();
        m.closure.sha256 = sha256_bytes(&b3);
        assert!(verify_closure(&b3, &m).unwrap_err().contains("lacks"));
    }

    #[test]
    fn offers() {
        let m = sample();
        let r = |n| ReleaseInfo { release: n, ..Default::default() };
        assert_eq!(compare(&m, &r(2)), Offer::Newer);
        assert_eq!(compare(&m, &r(3)), Offer::Same);
        assert_eq!(compare(&m, &r(4)), Offer::Older);
        assert_eq!(compare(&m, &r(0)), Offer::TooOld);
    }

    #[test]
    fn urls() {
        assert_eq!(resolve_url("http://h:1/channels/k6/manifest.json", "closure.json"), "http://h:1/channels/k6/closure.json");
        assert_eq!(resolve_url("http://h:1/channels/k6/manifest.json", "../../cache"), "http://h:1/cache");
        assert_eq!(resolve_url("file:///srv/ota/channels/k6/manifest.json", "../../cache"), "file:///srv/ota/cache");
        assert_eq!(resolve_url("http://h/x/manifest.json", "s3://bucket?region=x"), "s3://bucket?region=x");
    }
}
