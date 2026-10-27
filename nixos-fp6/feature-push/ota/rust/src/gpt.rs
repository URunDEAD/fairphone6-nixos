//! Read-only GPT parsing: partition names, type GUIDs and the Qualcomm A/B
//! attribute byte (bits 48-55 of the attribute field, as the ABL reads it):
//!   bits 48-49 priority, 50 active, 51-53 retry count, 54 successful,
//!   55 unbootable.
//! Never writes. Used for `fp6-updated slots` (supervised test, before/after
//! comparisons) and to verify what qbootctl did.

use serde::Serialize;
use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

/// The type GUID Qualcomm gives the inactive copy of an A/B pair.
pub const INACTIVE_TYPE_GUID: &str = "77036cd4-03d5-42bb-8ed1-37e5a88baa34";

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AbBits {
    pub priority: u8,
    pub active: bool,
    pub retries: u8,
    pub successful: bool,
    pub unbootable: bool,
}

pub fn ab_bits(attrs: u64) -> AbBits {
    let b = (attrs >> 48) as u8;
    AbBits {
        priority: b & 3,
        active: b & 0x04 != 0,
        retries: (b >> 3) & 7,
        successful: b & 0x40 != 0,
        unbootable: b & 0x80 != 0,
    }
}

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub index: u32,
    pub name: String,
    pub type_guid: String,
    pub unique_guid: String,
    pub first_lba: u64,
    pub last_lba: u64,
    pub attributes: u64,
    pub ab: AbBits,
    /// The backup table's attributes differ from the primary's.
    pub backup_differs: bool,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Disk {
    pub sector_size: u64,
    pub entries: Vec<Entry>,
    pub backup_ok: bool,
}

pub fn crc32(data: &[u8]) -> u32 {
    let mut c: u32 = 0xffff_ffff;
    for &b in data {
        c ^= b as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 { (c >> 1) ^ 0xedb8_8320 } else { c >> 1 };
        }
    }
    !c
}

pub fn guid(b: &[u8]) -> String {
    format!("{:08x}-{:04x}-{:04x}-{:02x}{:02x}-{}",
            u32::from_le_bytes(b[0..4].try_into().unwrap()),
            u16::from_le_bytes(b[4..6].try_into().unwrap()),
            u16::from_le_bytes(b[6..8].try_into().unwrap()),
            b[8], b[9], crate::util::hex(&b[10..16]))
}

fn le64(b: &[u8], o: usize) -> u64 {
    u64::from_le_bytes(b[o..o + 8].try_into().unwrap())
}
fn le32(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes(b[o..o + 4].try_into().unwrap())
}

struct Table {
    entries_lba: u64,
    count: u32,
    size: u32,
    alternate: u64,
    raw: Vec<u8>,
}

fn read_at(f: &mut fs::File, off: u64, len: usize) -> Result<Vec<u8>, String> {
    f.seek(SeekFrom::Start(off)).map_err(|e| e.to_string())?;
    let mut b = vec![0u8; len];
    f.read_exact(&mut b).map_err(|e| format!("read at {off}: {e}"))?;
    Ok(b)
}

fn table(f: &mut fs::File, ss: u64, lba: u64) -> Result<Table, String> {
    let h = read_at(f, lba * ss, ss as usize)?;
    if &h[0..8] != b"EFI PART" {
        return Err("no GPT signature".into());
    }
    let hsize = le32(&h, 12) as usize;
    if !(92..=ss as usize).contains(&hsize) {
        return Err("bad GPT header size".into());
    }
    let mut hc = h[..hsize].to_vec();
    hc[16..20].copy_from_slice(&[0; 4]);
    if crc32(&hc) != le32(&h, 16) {
        return Err("GPT header CRC mismatch".into());
    }
    let (entries_lba, count, size) = (le64(&h, 72), le32(&h, 80), le32(&h, 84));
    if size < 128 || count == 0 || count > 1024 {
        return Err("bad GPT entry geometry".into());
    }
    let raw = read_at(f, entries_lba * ss, (count * size) as usize)?;
    if crc32(&raw) != le32(&h, 88) {
        return Err("GPT entries CRC mismatch".into());
    }
    Ok(Table { entries_lba, count, size, alternate: le64(&h, 32), raw })
}

/// Parses the primary GPT of a whole disk (or a disk image file), trying
/// 4096- then 512-byte sectors, and compares the backup table's attributes.
pub fn read_disk(path: &Path) -> Result<Disk, String> {
    let mut f = fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut last = String::new();
    for ss in [4096u64, 512] {
        let t = match table(&mut f, ss, 1) {
            Ok(t) => t,
            Err(e) => {
                if last.is_empty() || last == "no GPT signature" {
                    last = e;
                }
                continue;
            }
        };
        let backup = table(&mut f, ss, t.alternate).ok();
        let _ = t.entries_lba;
        let mut entries = Vec::new();
        for i in 0..t.count as usize {
            let e = &t.raw[i * t.size as usize..(i + 1) * t.size as usize];
            if e[0..16].iter().all(|b| *b == 0) {
                continue;
            }
            let name: Vec<u16> = e[56..128].chunks(2).map(|c| u16::from_le_bytes([c[0], c[1]])).take_while(|c| *c != 0).collect();
            let attributes = le64(e, 48);
            let backup_differs = match &backup {
                Some(b) if b.count == t.count && b.size == t.size => le64(&b.raw, i * b.size as usize + 48) != attributes,
                _ => true,
            };
            entries.push(Entry {
                index: i as u32 + 1,
                name: String::from_utf16_lossy(&name),
                type_guid: guid(&e[0..16]),
                unique_guid: guid(&e[16..32]),
                first_lba: le64(e, 32),
                last_lba: le64(e, 40),
                attributes,
                ab: ab_bits(attributes),
                backup_differs,
            });
        }
        return Ok(Disk { sector_size: ss, entries, backup_ok: backup.is_some() });
    }
    Err(format!("{}: {last}", path.display()))
}

/// The whole-disk device of a partition device (/dev/sde41 -> /dev/sde),
/// from sysfs.
pub fn parent_disk(part_dev: &Path) -> Option<std::path::PathBuf> {
    let name = part_dev.file_name()?.to_str()?;
    let link = fs::canonicalize(format!("/sys/class/block/{name}")).ok()?;
    let parent = link.parent()?.file_name()?.to_str()?.to_string();
    Some(Path::new("/dev").join(parent))
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use std::io::Write;

    /// A synthetic GPT disk image with the given (name, type guid bytes, attributes).
    pub fn make_disk(path: &Path, ss: u64, parts: &[(&str, [u8; 16], u64)]) {
        let count = 32u32;
        let esize = 128u32;
        let total_lba = 64u64;
        let mut entries = vec![0u8; (count * esize) as usize];
        for (i, (name, tg, attrs)) in parts.iter().enumerate() {
            let e = &mut entries[i * 128..(i + 1) * 128];
            e[0..16].copy_from_slice(tg);
            e[16] = i as u8 + 1;
            e[32..40].copy_from_slice(&(10 + i as u64).to_le_bytes());
            e[40..48].copy_from_slice(&(10 + i as u64).to_le_bytes());
            e[48..56].copy_from_slice(&attrs.to_le_bytes());
            for (j, c) in name.encode_utf16().enumerate() {
                e[56 + 2 * j..58 + 2 * j].copy_from_slice(&c.to_le_bytes());
            }
        }
        let ecrc = crc32(&entries);
        let header = |my: u64, alt: u64, elba: u64| {
            let mut h = vec![0u8; ss as usize];
            h[0..8].copy_from_slice(b"EFI PART");
            h[8..12].copy_from_slice(&0x0001_0000u32.to_le_bytes());
            h[12..16].copy_from_slice(&92u32.to_le_bytes());
            h[24..32].copy_from_slice(&my.to_le_bytes());
            h[32..40].copy_from_slice(&alt.to_le_bytes());
            h[72..80].copy_from_slice(&elba.to_le_bytes());
            h[80..84].copy_from_slice(&count.to_le_bytes());
            h[84..88].copy_from_slice(&esize.to_le_bytes());
            h[88..92].copy_from_slice(&ecrc.to_le_bytes());
            let c = crc32(&h[..92]);
            h[16..20].copy_from_slice(&c.to_le_bytes());
            h
        };
        let mut img = vec![0u8; (total_lba * ss) as usize];
        let elen = entries.len();
        img[ss as usize..2 * ss as usize].copy_from_slice(&header(1, total_lba - 1, 2));
        img[2 * ss as usize..2 * ss as usize + elen].copy_from_slice(&entries);
        let belba = total_lba - 1 - (elen as u64).div_ceil(ss);
        img[(belba * ss) as usize..(belba * ss) as usize + elen].copy_from_slice(&entries);
        let last = ((total_lba - 1) * ss) as usize;
        img[last..last + ss as usize].copy_from_slice(&header(total_lba - 1, 1, belba));
        fs::File::create(path).unwrap().write_all(&img).unwrap();
    }

    pub fn guid_bytes(s: &str) -> [u8; 16] {
        let h: String = s.chars().filter(|c| *c != '-').collect();
        let b: Vec<u8> = (0..16).map(|i| u8::from_str_radix(&h[2 * i..2 * i + 2], 16).unwrap()).collect();
        let mut o = [0u8; 16];
        o[0..4].copy_from_slice(&[b[3], b[2], b[1], b[0]]);
        o[4..6].copy_from_slice(&[b[5], b[4]]);
        o[6..8].copy_from_slice(&[b[7], b[6]]);
        o[8..16].copy_from_slice(&b[8..16]);
        o
    }

    #[test]
    fn crc_known_value() {
        assert_eq!(crc32(b"123456789"), 0xcbf4_3926);
    }

    #[test]
    fn reads_synthetic_ab_disk() {
        let d = std::env::temp_dir().join(format!("fp6-gpt-test-{}", std::process::id()));
        fs::create_dir_all(&d).unwrap();
        let boot_type = guid_bytes("20117f86-e985-4357-b9ee-374bc1d8487d");
        let inactive = guid_bytes(INACTIVE_TYPE_GUID);
        // boot_a inactive, not successful, 0 retries; boot_b: priority 3, active, 6 retries, successful
        let a_attrs = 0u64;
        let b_attrs = (0x77u64) << 48;
        for ss in [4096u64, 512] {
            let img = d.join(format!("disk{ss}.img"));
            make_disk(&img, ss, &[("boot_a", inactive, a_attrs), ("boot_b", boot_type, b_attrs), ("persist", boot_type, 0)]);
            let disk = read_disk(&img).unwrap();
            assert_eq!(disk.sector_size, ss);
            assert!(disk.backup_ok);
            assert_eq!(disk.entries.len(), 3);
            let a = &disk.entries[0];
            assert_eq!(a.name, "boot_a");
            assert_eq!(a.type_guid, INACTIVE_TYPE_GUID);
            assert!(!a.ab.active && !a.ab.successful);
            let b = &disk.entries[1];
            assert_eq!(b.type_guid, "20117f86-e985-4357-b9ee-374bc1d8487d");
            assert_eq!(b.ab, AbBits { priority: 3, active: true, retries: 6, successful: true, unbootable: false });
            assert!(!b.backup_differs);
        }
        // corrupt the primary entries: CRC mismatch is reported, never "repaired"
        let img = d.join("disk4096.img");
        let mut raw = fs::read(&img).unwrap();
        raw[2 * 4096 + 48] ^= 1;
        fs::write(&img, &raw).unwrap();
        let e = read_disk(&img).unwrap_err();
        assert!(e.contains("CRC"), "{e}");
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn qbootctl_active_value_decodes() {
        // qbootctl's AB_SLOT_ACTIVE_VAL 0xF in the attribute byte
        assert_eq!(ab_bits(0xFu64 << 48), AbBits { priority: 3, active: true, retries: 1, successful: false, unbootable: false });
        // ABL's MarkPtnActive (fastboot set_active): priority 3, active, 7 retries
        assert_eq!(ab_bits(0x3Fu64 << 48).retries, 7);
    }
}
