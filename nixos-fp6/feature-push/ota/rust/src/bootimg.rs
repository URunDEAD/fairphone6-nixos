//! Android boot images and boot partitions.
//!
//! * Header parsing (v0-v4): command line, pinned NixOS generation
//!   (mobile-nixos.generation=, else init=<toplevel>/init) and the image
//!   length (to keep a trimmed copy of a backup that fastboot can RAM-boot).
//! * Partition resolution by GPT label, with the size checked against the
//!   expected one every time (never trust historical device numbering).
//! * Full-partition backup, image write (no truncation; bytes after the image
//!   stay, exactly as the manual boot_b installs did), fsync, page-cache drop
//!   and a readback SHA-256 of the written length.
//! Only boot_a / boot_b are ever opened for writing, and only by `write`.

use crate::util::{self, sha256_file};
use serde::Serialize;
use std::fs;
use std::io::{Read, Seek, SeekFrom, Write};
use std::os::unix::fs::FileTypeExt;
use std::path::{Path, PathBuf};

pub const WRITABLE: [&str; 2] = ["boot_a", "boot_b"];

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Header {
    pub version: u32,
    pub cmdline: String,
    pub length: u64,
}

fn le32(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes(b[o..o + 4].try_into().unwrap())
}

fn align(n: u64, page: u64) -> u64 {
    n.div_ceil(page) * page
}

fn cstr(b: &[u8]) -> String {
    let end = b.iter().position(|c| *c == 0).unwrap_or(b.len());
    String::from_utf8_lossy(&b[..end]).into_owned()
}

/// Parses the first 4 KiB of a boot image.
pub fn parse_header(h: &[u8]) -> Result<Header, String> {
    if h.len() < 1660 || &h[0..8] != b"ANDROID!" {
        return Err("not an Android boot image".into());
    }
    let version = le32(h, 40);
    match version {
        0..=2 => {
            let page = le32(h, 36) as u64;
            if !page.is_power_of_two() || !(2048..=16384).contains(&page) {
                return Err(format!("bad page size {page}"));
            }
            let (k, r, s) = (le32(h, 8) as u64, le32(h, 16) as u64, le32(h, 24) as u64);
            let mut len = page + align(k, page) + align(r, page) + align(s, page);
            if version >= 1 {
                len += align(le32(h, 1632) as u64, page);
            }
            if version == 2 {
                len += align(le32(h, 1648) as u64, page);
            }
            Ok(Header { version, cmdline: format!("{}{}", cstr(&h[64..576]), cstr(&h[608..1632])), length: len })
        }
        3 | 4 => {
            let page = 4096;
            let (k, r) = (le32(h, 8) as u64, le32(h, 12) as u64);
            let mut len = page + align(k, page) + align(r, page);
            if version == 4 {
                len += align(le32(h, 1580) as u64, page);
            }
            Ok(Header { version, cmdline: cstr(&h[44..1580]), length: len })
        }
        v => Err(format!("unsupported boot image header version {v}")),
    }
}

pub fn generation_from_cmdline(cmd: &str) -> Option<String> {
    let mut init = None;
    for t in cmd.split_whitespace() {
        if let Some(v) = t.strip_prefix("mobile-nixos.generation=") {
            return Some(v.to_string());
        }
        if let Some(v) = t.strip_prefix("init=") {
            if v != "/init" {
                init = v.strip_suffix("/init").map(str::to_string);
            }
        }
    }
    init
}

pub fn read_header(p: &Path) -> Result<Header, String> {
    let mut f = fs::File::open(p).map_err(|e| format!("{}: {e}", p.display()))?;
    let mut h = vec![0u8; 4096];
    f.read_exact(&mut h).map_err(|e| format!("{}: {e}", p.display()))?;
    parse_header(&h)
}

/// The toplevel a boot partition or image pins, if any.
pub fn pinned_generation(p: &Path) -> Result<Option<String>, String> {
    Ok(generation_from_cmdline(&read_header(p)?.cmdline))
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Partition {
    pub label: String,
    pub link: PathBuf,
    pub device: PathBuf,
    pub size: u64,
    pub block: bool,
}

fn blk_size(f: &fs::File) -> Option<u64> {
    use std::os::unix::io::AsRawFd;
    // BLKGETSIZE64 = _IOR(0x12, 114, size_t)
    const BLKGETSIZE64: libc::c_ulong = 0x8008_1272;
    let mut sz: u64 = 0;
    let r = unsafe { libc::ioctl(f.as_raw_fd(), BLKGETSIZE64 as _, &mut sz) };
    if r == 0 { Some(sz) } else { None }
}

/// Resolve `<by_partlabel>/<label>` now. Regular files are only accepted when
/// `allow_files` (the laptop dry run's fake phone).
pub fn resolve(by_partlabel: &Path, label: &str, allow_files: bool) -> Result<Partition, String> {
    if label.contains('/') || label.is_empty() {
        return Err("bad partition label".into());
    }
    let link = by_partlabel.join(label);
    let device = fs::canonicalize(&link).map_err(|e| format!("{}: {e}", link.display()))?;
    let md = fs::metadata(&device).map_err(|e| format!("{}: {e}", device.display()))?;
    let block = md.file_type().is_block_device();
    if !block && !(allow_files && md.is_file()) {
        return Err(format!("{} is not a block device", device.display()));
    }
    let size = if block {
        let f = fs::File::open(&device).map_err(|e| format!("{}: {e}", device.display()))?;
        blk_size(&f).ok_or_else(|| format!("{}: size unknown", device.display()))?
    } else {
        md.len()
    };
    Ok(Partition { label: label.into(), link, device, size, block })
}

/// Copies the whole partition to `dest` (0600), fsyncs, and returns the
/// SHA-256 of the partition as read and of the file as written (they must
/// match). Also keeps a trimmed copy (`<dest>.trimmed.img`, the image
/// length from the header) when the partition holds a boot image.
pub fn backup(part: &Partition, dest: &Path) -> Result<(String, Option<(PathBuf, String, u64)>), String> {
    use std::os::unix::fs::OpenOptionsExt;
    if let Some(d) = dest.parent() {
        fs::create_dir_all(d).map_err(|e| format!("{}: {e}", d.display()))?;
    }
    let mut src = fs::File::open(&part.device).map_err(|e| format!("{}: {e}", part.device.display()))?;
    let mut out = fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(dest)
        .map_err(|e| format!("{}: {e}", dest.display()))?;
    let mut buf = vec![0u8; 1 << 20];
    let mut n = 0u64;
    while n < part.size {
        let want = ((part.size - n) as usize).min(buf.len());
        src.read_exact(&mut buf[..want]).map_err(|e| format!("{}: {e}", part.device.display()))?;
        out.write_all(&buf[..want]).map_err(|e| format!("{}: {e}", dest.display()))?;
        n += want as u64;
    }
    out.sync_all().map_err(|e| format!("{}: {e}", dest.display()))?;
    drop(out);
    let (s_part, _) = sha256_file(&part.device, Some(part.size))?;
    let (s_file, len) = sha256_file(dest, None)?;
    if s_part != s_file || len != part.size {
        let _ = fs::remove_file(dest);
        return Err("backup does not match the partition".into());
    }
    let trimmed = match read_header(dest) {
        Ok(h) if h.length <= part.size && h.length >= 4096 => {
            let t = dest.with_extension("trimmed.img");
            let mut data = vec![0u8; h.length as usize];
            fs::File::open(dest).and_then(|mut f| f.read_exact(&mut data)).map_err(|e| e.to_string())?;
            util::write_atomic(&t, &data, 0o600)?;
            Some((t, util::sha256_bytes(&data), h.length))
        }
        _ => None,
    };
    Ok((s_part, trimmed))
}

fn drop_caches(f: &fs::File, block: bool) {
    use std::os::unix::io::AsRawFd;
    // BLKFLSBUF = _IO(0x12, 97)
    const BLKFLSBUF: libc::c_ulong = 0x1261;
    unsafe {
        if block {
            libc::ioctl(f.as_raw_fd(), BLKFLSBUF as _, 0);
        }
        libc::posix_fadvise(f.as_raw_fd(), 0, 0, libc::POSIX_FADV_DONTNEED);
    }
}

/// Writes `image` (a file whose SHA-256 must be `sha256`) to the start of
/// the partition, fsyncs, drops the cached pages and reads back.
pub fn write(part: &Partition, image: &Path, sha256: &str) -> Result<(), String> {
    if !WRITABLE.contains(&part.label.as_str()) {
        return Err(format!("refusing to write {}: only boot_a/boot_b are ever written", part.label));
    }
    let data = fs::read(image).map_err(|e| format!("{}: {e}", image.display()))?;
    if util::sha256_bytes(&data) != sha256 {
        return Err("boot image does not match its expected SHA-256".into());
    }
    parse_header(&data[..data.len().min(4096)])?;
    if data.len() as u64 > part.size {
        return Err(format!("boot image ({} B) larger than {} ({} B)", data.len(), part.label, part.size));
    }
    {
        // No create, no truncate: the partition (or the fake phone's file) must exist.
        let mut f = fs::OpenOptions::new().write(true).open(&part.device).map_err(|e| format!("{}: {e}", part.device.display()))?;
        f.seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;
        f.write_all(&data).map_err(|e| format!("writing {}: {e}", part.label))?;
        f.sync_all().map_err(|e| format!("fsync {}: {e}", part.label))?;
        drop_caches(&f, part.block);
    }
    let r = fs::File::open(&part.device).map_err(|e| e.to_string())?;
    drop_caches(&r, part.block);
    drop(r);
    let (back, _) = sha256_file(&part.device, Some(data.len() as u64))?;
    if back != sha256 {
        return Err(format!("readback of {} does not match ({back})", part.label));
    }
    util::log(format!("fp6-updated: wrote {} B to {} ({}), readback sha256 {back} OK", data.len(), part.label, part.device.display()));
    Ok(())
}

#[cfg(test)]
pub mod tests {
    use super::*;

    /// A v2 boot image like repack-boot.py's (page 4096).
    pub fn image(cmdline: &str, kernel: u32, ramdisk: u32, dtb: u32) -> Vec<u8> {
        let page = 4096u32;
        let mut h = vec![0u8; 4096];
        h[0..8].copy_from_slice(b"ANDROID!");
        h[8..12].copy_from_slice(&kernel.to_le_bytes());
        h[16..20].copy_from_slice(&ramdisk.to_le_bytes());
        h[36..40].copy_from_slice(&page.to_le_bytes());
        h[40..44].copy_from_slice(&2u32.to_le_bytes());
        let c = cmdline.as_bytes();
        let n = c.len().min(512);
        h[64..64 + n].copy_from_slice(&c[..n]);
        if c.len() > 512 {
            h[608..608 + c.len() - 512].copy_from_slice(&c[512..]);
        }
        h[1648..1652].copy_from_slice(&dtb.to_le_bytes());
        let body = (align(kernel as u64, 4096) + align(ramdisk as u64, 4096) + align(dtb as u64, 4096)) as usize;
        let mut v = h;
        v.extend((0..body).map(|i| (i % 251) as u8));
        v
    }

    #[test]
    fn header_and_generation() {
        let top = "/nix/store/z5av6a1y11jpm7m84yyqy59ffv5lc12f-nixos-system-fp6";
        let img = image(&format!("console=tty1 quiet mobile-nixos.generation={top}"), 5000, 9000, 100);
        let h = parse_header(&img[..4096]).unwrap();
        assert_eq!(h.version, 2);
        assert_eq!(h.length, 4096 * 7);
        assert_eq!(h.length as usize, img.len());
        assert_eq!(generation_from_cmdline(&h.cmdline).as_deref(), Some(top));
        let long = format!("{} mobile-nixos.generation={top}", "x".repeat(600));
        assert_eq!(generation_from_cmdline(&parse_header(&image(&long, 1, 1, 0)[..4096]).unwrap().cmdline).as_deref(), Some(top));
        assert_eq!(generation_from_cmdline(&format!("init={top}/init")).as_deref(), Some(top));
        assert_eq!(generation_from_cmdline("init=/init quiet"), None);
        assert!(parse_header(&[0u8; 4096]).is_err());
    }

    #[test]
    fn backup_write_readback_on_files() {
        let d = std::env::temp_dir().join(format!("fp6-bootimg-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(d.join("by-partlabel")).unwrap();
        let size = 1 << 20;
        let old = image("mobile-nixos.generation=/nix/store/old", 8000, 8000, 0);
        let mut part_content = old.clone();
        part_content.resize(size, 0xAB); // tail: leftover bytes of an older image
        fs::write(d.join("sde41"), &part_content).unwrap();
        std::os::unix::fs::symlink(d.join("sde41"), d.join("by-partlabel/boot_b")).unwrap();
        fs::write(d.join("sda6"), b"persist").unwrap();
        std::os::unix::fs::symlink(d.join("sda6"), d.join("by-partlabel/persist")).unwrap();

        assert!(resolve(&d.join("by-partlabel"), "boot_b", false).unwrap_err().contains("not a block device"));
        let p = resolve(&d.join("by-partlabel"), "boot_b", true).unwrap();
        assert_eq!(p.size, size as u64);

        let (sha, trimmed) = backup(&p, &d.join("bk/boot_b-before.img")).unwrap();
        assert_eq!(sha, util::sha256_bytes(&part_content));
        let (tp, tsha, tlen) = trimmed.unwrap();
        assert_eq!(tlen as usize, old.len());
        assert_eq!(tsha, util::sha256_bytes(&old));
        assert_eq!(fs::read(tp).unwrap(), old);
        assert!(backup(&p, &d.join("bk/boot_b-before.img")).is_err(), "never overwrites a backup");

        let new = image("mobile-nixos.generation=/nix/store/new", 4000, 4000, 0);
        fs::write(d.join("new.img"), &new).unwrap();
        let nsha = util::sha256_bytes(&new);
        assert!(write(&p, &d.join("new.img"), &"0".repeat(64)).unwrap_err().contains("expected SHA-256"));
        write(&p, &d.join("new.img"), &nsha).unwrap();
        let after = fs::read(d.join("sde41")).unwrap();
        assert_eq!(after.len(), size, "no truncation");
        assert_eq!(&after[..new.len()], &new[..]);
        assert_eq!(after[size - 1], 0xAB, "tail untouched");
        assert_eq!(pinned_generation(&d.join("sde41")).unwrap().as_deref(), Some("/nix/store/new"));

        let persist = resolve(&d.join("by-partlabel"), "persist", true).unwrap();
        assert!(write(&persist, &d.join("new.img"), &nsha).unwrap_err().contains("only boot_a/boot_b"));
        assert_eq!(fs::read(d.join("sda6")).unwrap(), b"persist");

        let mut big = image("x", 4000, 4000, 0);
        big.resize(size + 4096, 0);
        fs::write(d.join("big.img"), &big).unwrap();
        assert!(write(&p, &d.join("big.img"), &util::sha256_bytes(&big)).unwrap_err().contains("larger"));
        let _ = fs::remove_dir_all(&d);
    }
}
